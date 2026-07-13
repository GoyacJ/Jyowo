use futures::future::BoxFuture;
use harness_contracts::{
    AgentId, NetworkAccess, RenderedSkill as ContractRenderedSkill, SkillFilter, SkillInjectionId,
    SkillInvocationReceipt, SkillRegistryCap, SkillScriptRunDeclaration, SkillScriptRunFile,
    SkillScriptRunPreparation, SkillShellInvocation, SkillSummary, SkillView, ToolError,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{
    RenderError, Skill, SkillMetricsSink, SkillRegistry, SkillRegistrySnapshot, SkillRenderer,
    SkillScriptNetworkPolicy,
};

const MAX_SCRIPT_ARGUMENT_BYTES: usize = 64 * 1024;
const MAX_SCRIPT_PACKAGE_FILES: usize = 4096;
const MAX_SCRIPT_PACKAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
pub struct SkillRegistryService {
    registry: SkillRegistry,
    renderer: SkillRenderer,
    metrics_sink: Option<Arc<dyn SkillMetricsSink>>,
    snapshot: Option<Arc<SkillRegistrySnapshot>>,
}

impl SkillRegistryService {
    #[must_use]
    pub fn new(registry: SkillRegistry, renderer: SkillRenderer) -> Self {
        Self {
            registry,
            renderer,
            metrics_sink: None,
            snapshot: None,
        }
    }

    #[must_use]
    pub fn with_metrics_sink(mut self, metrics_sink: Arc<dyn SkillMetricsSink>) -> Self {
        self.metrics_sink = Some(metrics_sink);
        self
    }

    #[must_use]
    pub fn with_snapshot(mut self, snapshot: Arc<SkillRegistrySnapshot>) -> Self {
        self.snapshot = Some(snapshot);
        self
    }

    #[must_use]
    pub fn list_summaries(&self, agent: &AgentId, filter: SkillFilter) -> Vec<SkillSummary> {
        match &self.snapshot {
            Some(snapshot) => self
                .registry
                .list_summaries_for_agent_in_snapshot(agent, filter, snapshot),
            None => self.registry.list_summaries_for_agent(agent, filter),
        }
    }

    #[must_use]
    pub fn view(&self, agent: &AgentId, name: &str, full: bool) -> Option<SkillView> {
        if let Some(metrics) = &self.metrics_sink {
            metrics.skill_view(name);
        }
        match &self.snapshot {
            Some(snapshot) => self.registry.view_in_snapshot(agent, name, full, snapshot),
            None => self.registry.view(agent, name, full),
        }
    }

    pub async fn render(
        &self,
        agent: &AgentId,
        name: &str,
        params: Value,
    ) -> Result<ContractRenderedSkill, RenderError> {
        let skill = match &self.snapshot {
            Some(snapshot) => {
                if self
                    .registry
                    .view_in_snapshot(agent, name, false, snapshot)
                    .is_none()
                {
                    return Err(RenderError::SkillNotVisible(name.to_owned()));
                }
                snapshot.entries.get(name).cloned()
            }
            None => {
                if self.registry.view(agent, name, false).is_none() {
                    return Err(RenderError::SkillNotVisible(name.to_owned()));
                }
                self.registry.get(name)
            }
        }
        .ok_or_else(|| RenderError::SkillNotVisible(name.to_owned()))?;
        if let Some(snapshot) = &self.snapshot {
            if let Some(harness_contracts::SkillStatus::PrerequisiteMissing {
                config_keys, ..
            }) = snapshot.status.get(&skill.id)
            {
                if !config_keys.is_empty() {
                    return Err(RenderError::MissingConfig {
                        skill_id: skill.id.0.clone(),
                        config_keys: config_keys.clone(),
                    });
                }
            }
        }
        self.renderer
            .render(&skill, params)
            .await
            .map(ContractRenderedSkill::from)
    }

    pub async fn invoke(
        &self,
        agent: &AgentId,
        name: &str,
        params: Value,
    ) -> Result<SkillInvocationReceipt, RenderError> {
        if let Some(metrics) = &self.metrics_sink {
            metrics.skill_invocation(name);
        }
        let rendered = self.render(agent, name, params).await?;
        Ok(SkillInvocationReceipt {
            skill_name: rendered.skill_name,
            injection_id: SkillInjectionId(new_injection_id(name)),
            bytes_injected: rendered.content.len() as u64,
            consumed_config_keys: rendered.consumed_config_keys,
        })
    }

    pub async fn prepare_script(
        &self,
        agent: &AgentId,
        name: &str,
        script_id: &str,
        arguments: Value,
    ) -> Result<SkillScriptRunPreparation, ToolError> {
        if !arguments.is_object() {
            return Err(ToolError::Validation(
                "skill script arguments must be an object".to_owned(),
            ));
        }
        let argument_bytes = serde_json::to_vec(&arguments)
            .map_err(|error| ToolError::Validation(error.to_string()))?;
        if argument_bytes.len() > MAX_SCRIPT_ARGUMENT_BYTES {
            return Err(ToolError::Validation(format!(
                "skill script arguments exceed {MAX_SCRIPT_ARGUMENT_BYTES} bytes"
            )));
        }

        let skill = self
            .visible_skill(agent, name)
            .ok_or_else(|| ToolError::Validation(format!("skill not visible: {name}")))?;
        if let Some(snapshot) = &self.snapshot {
            if let Some(harness_contracts::SkillStatus::PrerequisiteMissing {
                config_keys, ..
            }) = snapshot.status.get(&skill.id)
            {
                if !config_keys.is_empty() {
                    return Err(ToolError::Validation(format!(
                        "skill `{}` is missing required config: {config_keys:?}",
                        skill.id.0
                    )));
                }
            }
        }
        let declaration = skill
            .frontmatter
            .scripts
            .iter()
            .find(|declaration| declaration.id == script_id)
            .cloned()
            .ok_or_else(|| {
                ToolError::Validation(format!(
                    "undeclared script `{script_id}` for skill `{name}`"
                ))
            })?;
        let package = collect_script_package(&skill, &declaration.path)?;
        let (env, secret_env_keys) = self
            .renderer
            .resolve_script_environment(&skill, &declaration)
            .await
            .map_err(|error| ToolError::Validation(error.to_string()))?;
        let network_access = match declaration.network {
            SkillScriptNetworkPolicy::Deny => NetworkAccess::None,
        };
        Ok(SkillScriptRunPreparation {
            skill_id: skill.id.clone(),
            skill_name: skill.name.clone(),
            script_id: declaration.id.clone(),
            package_hash: package.hash,
            arguments,
            declaration: SkillScriptRunDeclaration {
                path: declaration.path,
                timeout_seconds: declaration.timeout_seconds,
                max_stdout_bytes: declaration.max_stdout_bytes,
                max_stderr_bytes: declaration.max_stderr_bytes,
                max_output_bytes: declaration.max_output_bytes,
                max_artifact_count: declaration.max_artifact_count,
                max_artifact_bytes: declaration.max_artifact_bytes,
                network_access,
                env_config_keys: declaration
                    .env
                    .iter()
                    .map(|(env_name, mapping)| (env_name.clone(), mapping.config.clone()))
                    .collect(),
                secret_env_keys,
            },
            files: package.files,
            env,
        })
    }

    fn visible_skill(&self, agent: &AgentId, name: &str) -> Option<Arc<Skill>> {
        match &self.snapshot {
            Some(snapshot) => {
                self.registry
                    .view_in_snapshot(agent, name, false, snapshot)?;
                snapshot.entries.get(name).cloned()
            }
            None => {
                self.registry.view(agent, name, false)?;
                self.registry.get(name)
            }
        }
    }
}

impl SkillRegistryCap for SkillRegistryService {
    fn list_summaries(&self, agent: &AgentId, filter: SkillFilter) -> Vec<SkillSummary> {
        self.list_summaries(agent, filter)
    }

    fn view(&self, agent: &AgentId, name: &str, full: bool) -> Option<SkillView> {
        self.view(agent, name, full)
    }

    fn render(
        &self,
        agent: &AgentId,
        name: String,
        params: Value,
    ) -> BoxFuture<'static, Result<ContractRenderedSkill, ToolError>> {
        let service = self.clone();
        let agent = *agent;
        Box::pin(async move {
            service
                .render(&agent, &name, params)
                .await
                .map_err(|error| ToolError::Validation(error.to_string()))
        })
    }

    fn prepare_script(
        &self,
        agent: &AgentId,
        name: String,
        script_id: String,
        arguments: Value,
    ) -> BoxFuture<'static, Result<SkillScriptRunPreparation, ToolError>> {
        let service = self.clone();
        let agent = *agent;
        Box::pin(async move {
            service
                .prepare_script(&agent, &name, &script_id, arguments)
                .await
        })
    }
}

struct ScriptPackage {
    hash: String,
    files: Vec<SkillScriptRunFile>,
}

fn collect_script_package(skill: &Skill, declared_path: &Path) -> Result<ScriptPackage, ToolError> {
    let raw_path = skill.raw_path.as_ref().ok_or_else(|| {
        ToolError::Validation(format!(
            "skill `{}` has no local package for script execution",
            skill.name
        ))
    })?;
    reject_symlink(raw_path)?;
    let package_root = raw_path.parent().ok_or_else(|| {
        ToolError::Validation(format!(
            "skill `{}` package root is unavailable",
            skill.name
        ))
    })?;
    let canonical_root = package_root
        .canonicalize()
        .map_err(|error| ToolError::Validation(format!("resolve skill package: {error}")))?;
    let declared_file = package_root.join(declared_path);
    reject_symlink(&declared_file)?;
    let canonical_declared = declared_file
        .canonicalize()
        .map_err(|error| ToolError::Validation(format!("resolve declared script: {error}")))?;
    if !canonical_declared.starts_with(&canonical_root) || !canonical_declared.is_file() {
        return Err(ToolError::Validation(format!(
            "declared script `{}` escapes the skill package",
            declared_path.display()
        )));
    }

    let mut paths = Vec::new();
    collect_package_paths(package_root, package_root, &mut paths)?;
    paths.sort_by(|left, right| left.0.cmp(&right.0));
    if paths.len() > MAX_SCRIPT_PACKAGE_FILES {
        return Err(ToolError::Validation(format!(
            "skill package exceeds {MAX_SCRIPT_PACKAGE_FILES} files"
        )));
    }
    let mut total_bytes = 0usize;
    let mut files = Vec::with_capacity(paths.len());
    let mut hasher = blake3::Hasher::new();
    hash_field(&mut hasher, b"jyowo.skill_script.package.v1");
    for (relative, path) in paths {
        let bytes = std::fs::read(&path)
            .map_err(|error| ToolError::Validation(format!("read skill package file: {error}")))?;
        total_bytes = total_bytes.saturating_add(bytes.len());
        if total_bytes > MAX_SCRIPT_PACKAGE_BYTES {
            return Err(ToolError::Validation(format!(
                "skill package exceeds {MAX_SCRIPT_PACKAGE_BYTES} bytes"
            )));
        }
        let content = String::from_utf8(bytes).map_err(|_| {
            ToolError::Validation(format!(
                "skill package file `{}` is not UTF-8",
                relative.display()
            ))
        })?;
        let relative = relative.to_string_lossy().replace('\\', "/");
        hash_field(&mut hasher, relative.as_bytes());
        hash_field(&mut hasher, content.as_bytes());
        files.push(SkillScriptRunFile {
            path: relative,
            content,
        });
    }
    if !files
        .iter()
        .any(|file| Path::new(&file.path) == declared_path)
    {
        return Err(ToolError::Validation(
            "declared script is missing from the prepared package".to_owned(),
        ));
    }
    Ok(ScriptPackage {
        hash: hasher.finalize().to_hex().to_string(),
        files,
    })
}

fn collect_package_paths(
    root: &Path,
    directory: &Path,
    paths: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), ToolError> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| ToolError::Validation(format!("read skill package: {error}")))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| ToolError::Validation(format!("read skill package: {error}")))?;
        let path = entry.path();
        let metadata = entry
            .file_type()
            .map_err(|error| ToolError::Validation(format!("inspect skill package: {error}")))?;
        if metadata.is_symlink() {
            return Err(ToolError::Validation(format!(
                "skill package symlink is not executable: {}",
                path.display()
            )));
        }
        if metadata.is_dir() {
            collect_package_paths(root, &path, paths)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| ToolError::Validation("invalid skill package path".to_owned()))?
                .to_path_buf();
            paths.push((relative, path));
        }
        if paths.len() > MAX_SCRIPT_PACKAGE_FILES {
            return Err(ToolError::Validation(format!(
                "skill package exceeds {MAX_SCRIPT_PACKAGE_FILES} files"
            )));
        }
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), ToolError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| ToolError::Validation(format!("inspect skill package path: {error}")))?;
    if metadata.file_type().is_symlink() {
        return Err(ToolError::Validation(format!(
            "skill package symlink is not executable: {}",
            path.display()
        )));
    }
    Ok(())
}

fn hash_field(hasher: &mut blake3::Hasher, value: &[u8]) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value);
}

impl From<crate::RenderedSkill> for ContractRenderedSkill {
    fn from(rendered: crate::RenderedSkill) -> Self {
        Self {
            skill_id: rendered.skill_id,
            skill_name: rendered.skill_name,
            content: rendered.content,
            shell_invocations: rendered
                .shell_invocations
                .into_iter()
                .map(|invocation| SkillShellInvocation {
                    command: invocation.command,
                    stdout_truncated: invocation.stdout_truncated,
                    exit_code: invocation.exit_code,
                })
                .collect(),
            consumed_config_keys: rendered.consumed_config_keys,
        }
    }
}

fn new_injection_id(name: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("skill:{name}:{nanos}")
}

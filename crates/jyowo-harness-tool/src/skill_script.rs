use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use harness_contracts::{
    CorrelationId, Event, NetworkAccess, SandboxError, SkillScriptRunPreparation,
};
use harness_sandbox::{
    execute_skill_script, EventSink, ExecContext, SandboxBackend, SkillScriptPackFile,
    SkillScriptSandboxRequest, SkillScriptSandboxResult,
};
use harness_skill::{SkillScriptDecl, SkillScriptEnvDecl, SkillScriptNetworkPolicy};

use crate::ToolContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillScriptLocalPathPolicy {
    pub authorized_roots: Vec<PathBuf>,
}

#[must_use]
pub fn skill_script_local_path_authorized(
    candidate_path: &Path,
    policy: &SkillScriptLocalPathPolicy,
) -> bool {
    let candidate = lexical_normalize(candidate_path);
    policy
        .authorized_roots
        .iter()
        .map(|root| lexical_normalize(root))
        .any(|root| candidate.starts_with(root))
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(std::path::MAIN_SEPARATOR.to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

pub async fn run_prepared_skill_script(
    prepared: SkillScriptRunPreparation,
    sandbox: Arc<dyn SandboxBackend>,
    ctx: &ToolContext,
) -> Result<SkillScriptSandboxResult, SandboxError> {
    let request = sandbox_request_from_preparation(prepared)?;
    execute_skill_script(
        sandbox,
        request,
        ExecContext {
            session_id: ctx.session_id,
            run_id: ctx.run_id,
            tool_use_id: Some(ctx.tool_use_id),
            tenant_id: ctx.tenant_id,
            // The runner creates a private child workspace and narrows the
            // sandbox root to it before starting the process.
            workspace_root: ctx.workspace_root.clone(),
            correlation_id: CorrelationId::new(),
            event_sink: Arc::new(NullEventSink),
            redactor: Arc::clone(&ctx.redactor),
            blob_store: None,
            execution_id: 0,
        },
    )
    .await
}

fn sandbox_request_from_preparation(
    prepared: SkillScriptRunPreparation,
) -> Result<SkillScriptSandboxRequest, SandboxError> {
    if prepared.declaration.network_access != NetworkAccess::None {
        return Err(SandboxError::CapabilityMismatch {
            capability: "network_policy".to_owned(),
            detail: "skill script runner supports only denied network access".to_owned(),
        });
    }
    if prepared.env.len() != prepared.declaration.env_config_keys.len()
        || prepared
            .env
            .keys()
            .any(|name| !prepared.declaration.env_config_keys.contains_key(name))
        || prepared
            .declaration
            .secret_env_keys
            .iter()
            .any(|name| !prepared.env.contains_key(name))
    {
        return Err(SandboxError::Message(
            "prepared skill script environment does not match its declaration".to_owned(),
        ));
    }
    let env = prepared
        .declaration
        .env_config_keys
        .iter()
        .map(|(env_name, config_key)| {
            (
                env_name.clone(),
                SkillScriptEnvDecl {
                    config: config_key.clone(),
                    secret: prepared.declaration.secret_env_keys.contains(env_name),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    Ok(SkillScriptSandboxRequest {
        declaration: SkillScriptDecl {
            id: prepared.script_id,
            path: prepared.declaration.path,
            timeout_seconds: prepared.declaration.timeout_seconds,
            network: SkillScriptNetworkPolicy::Deny,
            env,
            max_stdout_bytes: prepared.declaration.max_stdout_bytes,
            max_stderr_bytes: prepared.declaration.max_stderr_bytes,
            max_output_bytes: prepared.declaration.max_output_bytes,
            max_artifact_count: prepared.declaration.max_artifact_count,
            max_artifact_bytes: prepared.declaration.max_artifact_bytes,
        },
        input: prepared.arguments,
        files: prepared
            .files
            .into_iter()
            .map(|file| SkillScriptPackFile {
                path: file.path,
                content: file.content,
            })
            .collect(),
        env: prepared.env,
    })
}

struct NullEventSink;

impl EventSink for NullEventSink {
    fn emit(&self, _event: Event) -> Result<(), SandboxError> {
        Ok(())
    }
}

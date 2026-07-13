use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures::{stream::BoxStream, StreamExt};
use harness_contracts::{
    KillScope, NetworkAccess, ResourceLimits, SandboxError, SandboxExitStatus, SandboxMode,
    SandboxPolicy, SandboxScope, WorkspaceAccess,
};
use harness_skill::{
    skill_script_path_has_reserved_component, SkillScriptDecl, SkillScriptNetworkPolicy,
    MAX_SKILL_SCRIPT_ARTIFACT_BYTES, MAX_SKILL_SCRIPT_ARTIFACT_COUNT,
    MAX_SKILL_SCRIPT_OUTPUT_BYTES, MAX_SKILL_SCRIPT_STREAM_BYTES, MAX_SKILL_SCRIPT_TIMEOUT_SECONDS,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    execute_with_lifecycle, ExecContext, ExecSpec, OutputOverflowPolicy, OutputPolicy,
    SandboxBackend, StdioSpec,
};

static TEMP_WORKSPACE_COUNTER: AtomicU64 = AtomicU64::new(0);
const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_millis(100);
const BACKEND_TIMEOUT_GRACE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillScriptPackFile {
    pub path: String,
    pub content: String,
}

pub struct SkillScriptSandboxRequest {
    pub declaration: SkillScriptDecl,
    pub input: Value,
    pub files: Vec<SkillScriptPackFile>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillScriptStatus {
    Succeeded,
    Failed,
    TimedOut,
    OutputLimitExceeded,
    ArtifactLimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillScriptArtifact {
    pub path: String,
    pub content: String,
    pub byte_size: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillScriptEnforcedPolicy {
    pub backend_id: String,
    pub timeout_ms: u64,
    pub network: SkillScriptNetworkPolicy,
    pub max_stdout_bytes: u64,
    pub max_stderr_bytes: u64,
    pub max_output_bytes: u64,
    pub max_artifact_count: u64,
    pub max_artifact_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillScriptSandboxResult {
    pub status: SkillScriptStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub elapsed_ms: u64,
    pub enforced_policy: SkillScriptEnforcedPolicy,
    pub mounted_files: Vec<String>,
    pub artifacts: Vec<SkillScriptArtifact>,
}

pub async fn execute_skill_script(
    backend: Arc<dyn SandboxBackend>,
    request: SkillScriptSandboxRequest,
    mut ctx: ExecContext,
) -> Result<SkillScriptSandboxResult, SandboxError> {
    validate_declaration(&request.declaration)?;
    validate_environment(&request)?;

    let workspace = TempSkillWorkspace::create(&ctx.workspace_root)?;
    let root = workspace.path().to_path_buf();
    if ctx.workspace_root.as_os_str().is_empty() {
        ctx.workspace_root = workspace.base().to_path_buf();
    }
    let script_path = safe_relative_path(&request.declaration.path)?;
    let mut mounted_files = Vec::with_capacity(request.files.len());
    let mut baseline = BTreeSet::new();

    for file in &request.files {
        let relative = safe_relative_path(Path::new(&file.path))?;
        let normalized = path_to_string(&relative);
        if !baseline.insert(normalized.clone()) {
            return Err(SandboxError::HostPathDenied {
                path: file.path.clone(),
            });
        }
        let target = root.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io_error)?;
        }
        std::fs::write(target, file.content.as_bytes()).map_err(io_error)?;
        mounted_files.push(normalized);
    }
    mounted_files.sort();

    let script_path_string = path_to_string(&script_path);
    if !baseline.contains(&script_path_string) || !root.join(&script_path).is_file() {
        return Err(SandboxError::HostPathDenied {
            path: script_path_string,
        });
    }

    let input_path = root.join(".jyowo-input.json");
    std::fs::write(
        input_path,
        serde_json::to_vec(&request.input)
            .map_err(|error| SandboxError::Message(error.to_string()))?,
    )
    .map_err(io_error)?;

    let timeout = Duration::from_secs(request.declaration.timeout_seconds);
    let timeout_ms = duration_ms(timeout);
    let stream_backend_limit = request
        .declaration
        .max_stdout_bytes
        .max(request.declaration.max_stderr_bytes);
    let authorized_env_keys = request.env.keys().cloned().collect();
    let spec = ExecSpec {
        command: command_for_script(&script_path).to_owned(),
        args: vec![script_path_string],
        env: request.env,
        authorized_env_keys,
        cwd: Some(root.clone()),
        stdin: StdioSpec::Null,
        stdout: StdioSpec::Piped,
        stderr: StdioSpec::Piped,
        timeout: Some(timeout),
        activity_timeout: None,
        policy: SandboxPolicy {
            mode: SandboxMode::None,
            scope: SandboxScope::WorkspaceOnly,
            network: NetworkAccess::None,
            resource_limits: ResourceLimits {
                max_memory_bytes: None,
                max_cpu_cores: None,
                max_pids: None,
                max_wall_clock_ms: Some(timeout_ms),
                max_open_files: None,
            },
            denied_host_paths: Vec::new(),
        },
        workspace_access: WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: vec![workspace.relative_path().to_path_buf()],
        },
        output_policy: OutputPolicy {
            max_inline_bytes: stream_backend_limit,
            overflow: OutputOverflowPolicy::Truncate,
            redact_secrets: true,
        },
        required_kill_scope: Some(KillScope::ProcessGroup),
    };

    let backend_id = backend.backend_id().to_owned();
    let started = Instant::now();
    let mut handle = execute_with_lifecycle(backend, spec, ctx).await?;
    let stdout_task = tokio::spawn(collect_stream(
        handle.stdout.take(),
        request.declaration.max_stdout_bytes,
    ));
    let stderr_task = tokio::spawn(collect_stream(
        handle.stderr.take(),
        request.declaration.max_stderr_bytes,
    ));

    let activity = Arc::clone(&handle.activity);
    let mut wait = Box::pin(activity.wait());
    let outcome = match tokio::time::timeout(
        timeout.saturating_add(BACKEND_TIMEOUT_GRACE),
        &mut wait,
    )
    .await
    {
        Ok(outcome) => {
            activity.kill(9, KillScope::ProcessGroup).await?;
            Some(outcome?)
        }
        Err(_) => {
            activity.kill(9, KillScope::ProcessGroup).await?;
            tokio::time::timeout(BACKEND_TIMEOUT_GRACE, &mut wait)
                .await
                .map_err(|_| {
                    SandboxError::Message(
                        "skill script process group did not terminate after kill".to_owned(),
                    )
                })??;
            None
        }
    };
    let (mut stdout, mut stderr) = join_streams(stdout_task, stderr_task).await?;
    let output_limited = bound_combined_output(
        &mut stdout,
        &mut stderr,
        request.declaration.max_output_bytes,
    );
    let output_limited = output_limited || stdout.truncated || stderr.truncated;
    let (artifacts, artifact_limited) = collect_artifacts(
        &root,
        &baseline,
        request.declaration.max_artifact_count,
        request.declaration.max_artifact_bytes,
    )?;
    let elapsed_ms = duration_ms(started.elapsed());

    let (status, exit_code) = result_status(outcome.as_ref(), output_limited, artifact_limited);
    Ok(SkillScriptSandboxResult {
        status,
        exit_code,
        stdout: String::from_utf8_lossy(&stdout.bytes).into_owned(),
        stderr: String::from_utf8_lossy(&stderr.bytes).into_owned(),
        elapsed_ms,
        enforced_policy: SkillScriptEnforcedPolicy {
            backend_id,
            timeout_ms,
            network: request.declaration.network,
            max_stdout_bytes: request.declaration.max_stdout_bytes,
            max_stderr_bytes: request.declaration.max_stderr_bytes,
            max_output_bytes: request.declaration.max_output_bytes,
            max_artifact_count: request.declaration.max_artifact_count,
            max_artifact_bytes: request.declaration.max_artifact_bytes,
        },
        mounted_files,
        artifacts,
    })
}

fn validate_declaration(declaration: &SkillScriptDecl) -> Result<(), SandboxError> {
    safe_relative_path(&declaration.path)?;
    if declaration.id.trim().is_empty()
        || !(1..=MAX_SKILL_SCRIPT_TIMEOUT_SECONDS).contains(&declaration.timeout_seconds)
        || !(1..=MAX_SKILL_SCRIPT_STREAM_BYTES).contains(&declaration.max_stdout_bytes)
        || !(1..=MAX_SKILL_SCRIPT_STREAM_BYTES).contains(&declaration.max_stderr_bytes)
        || !(1..=MAX_SKILL_SCRIPT_OUTPUT_BYTES).contains(&declaration.max_output_bytes)
        || !(1..=MAX_SKILL_SCRIPT_ARTIFACT_COUNT).contains(&declaration.max_artifact_count)
        || !(1..=MAX_SKILL_SCRIPT_ARTIFACT_BYTES).contains(&declaration.max_artifact_bytes)
    {
        return Err(SandboxError::Message(
            "skill script declaration is outside enforced bounds".to_owned(),
        ));
    }
    match declaration.network {
        SkillScriptNetworkPolicy::Deny => Ok(()),
    }
}

fn validate_environment(request: &SkillScriptSandboxRequest) -> Result<(), SandboxError> {
    if let Some(name) = request
        .env
        .keys()
        .find(|name| !request.declaration.env.contains_key(*name))
    {
        return Err(SandboxError::Message(format!(
            "undeclared environment variable `{name}`"
        )));
    }
    Ok(())
}

fn command_for_script(path: &Path) -> &'static str {
    match path.extension().and_then(|value| value.to_str()) {
        Some("py") => "python3",
        _ => "/bin/sh",
    }
}

struct BoundedStream {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn collect_stream(
    stream: Option<BoxStream<'static, bytes::Bytes>>,
    limit: u64,
) -> BoundedStream {
    let mut bounded = BoundedStream {
        bytes: Vec::with_capacity(to_usize(limit)),
        truncated: false,
    };
    let Some(mut stream) = stream else {
        return bounded;
    };
    while let Some(chunk) = stream.next().await {
        let remaining = to_usize(limit).saturating_sub(bounded.bytes.len());
        let take = remaining.min(chunk.len());
        bounded.bytes.extend_from_slice(&chunk[..take]);
        bounded.truncated |= take < chunk.len();
    }
    bounded
}

async fn join_streams(
    mut stdout_task: tokio::task::JoinHandle<BoundedStream>,
    mut stderr_task: tokio::task::JoinHandle<BoundedStream>,
) -> Result<(BoundedStream, BoundedStream), SandboxError> {
    let joined = tokio::time::timeout(OUTPUT_DRAIN_TIMEOUT, async {
        tokio::join!(&mut stdout_task, &mut stderr_task)
    })
    .await;
    match joined {
        Ok((stdout, stderr)) => Ok((
            stdout.map_err(output_task_error)?,
            stderr.map_err(output_task_error)?,
        )),
        Err(_) => {
            stdout_task.abort();
            stderr_task.abort();
            Ok((
                BoundedStream {
                    bytes: Vec::new(),
                    truncated: true,
                },
                BoundedStream {
                    bytes: Vec::new(),
                    truncated: true,
                },
            ))
        }
    }
}

fn output_task_error(error: tokio::task::JoinError) -> SandboxError {
    SandboxError::Message(format!("skill script output task: {error}"))
}

fn bound_combined_output(
    stdout: &mut BoundedStream,
    stderr: &mut BoundedStream,
    max_output_bytes: u64,
) -> bool {
    let max = to_usize(max_output_bytes);
    let mut limited = false;
    if stdout.bytes.len() > max {
        stdout.bytes.truncate(max);
        stdout.truncated = true;
        limited = true;
    }
    let remaining = max.saturating_sub(stdout.bytes.len());
    if stderr.bytes.len() > remaining {
        stderr.bytes.truncate(remaining);
        stderr.truncated = true;
        limited = true;
    }
    limited
}

fn result_status(
    outcome: Option<&crate::ExecOutcome>,
    output_limited: bool,
    artifact_limited: bool,
) -> (SkillScriptStatus, Option<i32>) {
    let Some(outcome) = outcome else {
        return (SkillScriptStatus::TimedOut, None);
    };
    let exit_code = match outcome.exit_status {
        SandboxExitStatus::Code(code) => Some(code),
        _ => None,
    };
    let status = match outcome.exit_status {
        SandboxExitStatus::Timeout | SandboxExitStatus::InactivityTimeout => {
            SkillScriptStatus::TimedOut
        }
        SandboxExitStatus::OutputBudgetExceeded => SkillScriptStatus::OutputLimitExceeded,
        _ if output_limited => SkillScriptStatus::OutputLimitExceeded,
        _ if artifact_limited => SkillScriptStatus::ArtifactLimitExceeded,
        SandboxExitStatus::Code(0) => SkillScriptStatus::Succeeded,
        _ => SkillScriptStatus::Failed,
    };
    (status, exit_code)
}

fn collect_artifacts(
    root: &Path,
    baseline: &BTreeSet<String>,
    max_count: u64,
    max_bytes: u64,
) -> Result<(Vec<SkillScriptArtifact>, bool), SandboxError> {
    let mut paths = list_files(root)?;
    paths.sort();
    let mut artifacts = Vec::new();
    let mut remaining_bytes = max_bytes;
    let mut limited = false;

    for path in paths {
        let relative = path
            .strip_prefix(root)
            .map_err(|error| SandboxError::Message(error.to_string()))?;
        let relative_string = path_to_string(relative);
        if relative_string.starts_with(".jyowo-") || baseline.contains(&relative_string) {
            continue;
        }
        if artifacts.len() as u64 >= max_count || remaining_bytes == 0 {
            limited = true;
            break;
        }
        let original_size = std::fs::metadata(&path).map_err(io_error)?.len();
        let take = original_size.min(remaining_bytes);
        let mut bytes = Vec::with_capacity(to_usize(take));
        std::fs::File::open(&path)
            .map_err(io_error)?
            .take(take)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        let truncated = original_size > take;
        limited |= truncated;
        remaining_bytes = remaining_bytes.saturating_sub(bytes.len() as u64);
        artifacts.push(SkillScriptArtifact {
            path: relative_string,
            content: String::from_utf8_lossy(&bytes).into_owned(),
            byte_size: bytes.len() as u64,
            truncated,
        });
    }
    Ok((artifacts, limited))
}

fn list_files(root: &Path) -> Result<Vec<PathBuf>, SandboxError> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let entry_path = entry.path();
            let file_type = entry.file_type().map_err(io_error)?;
            if file_type.is_dir() {
                pending.push(entry_path);
            } else if file_type.is_file() {
                files.push(entry_path);
            }
        }
    }
    Ok(files)
}

fn safe_relative_path(value: &Path) -> Result<PathBuf, SandboxError> {
    let display = path_to_string(value);
    let normalized_value = display.replace('\\', "/");
    let path = Path::new(&normalized_value);
    let windows_absolute = normalized_value
        .as_bytes()
        .get(1)
        .is_some_and(|value| *value == b':');
    if path.is_absolute() || windows_absolute || normalized_value.starts_with("//") {
        return Err(SandboxError::HostPathDenied { path: display });
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(SandboxError::HostPathDenied { path: display });
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(SandboxError::HostPathDenied { path: display });
    }
    if skill_script_path_has_reserved_component(&normalized) {
        return Err(SandboxError::HostPathDenied { path: display });
    }
    Ok(normalized)
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn to_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

fn io_error(error: std::io::Error) -> SandboxError {
    SandboxError::Message(error.to_string())
}

struct TempSkillWorkspace {
    base: PathBuf,
    relative_path: PathBuf,
    path: PathBuf,
}

impl TempSkillWorkspace {
    fn create(workspace_root: &Path) -> Result<Self, SandboxError> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| SandboxError::Message(error.to_string()))?
            .as_nanos();
        let sequence = TEMP_WORKSPACE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            "jyowo-skill-script-{}-{unique}-{sequence}",
            std::process::id()
        );
        let (base, relative_path) = if workspace_root.as_os_str().is_empty() {
            (std::env::temp_dir(), PathBuf::from(name))
        } else {
            (
                workspace_root.to_path_buf(),
                PathBuf::from(".jyowo").join("skill-script-runs").join(name),
            )
        };
        let path = base.join(&relative_path);
        std::fs::create_dir_all(&path).map_err(io_error)?;
        Ok(Self {
            base,
            relative_path,
            path,
        })
    }

    fn base(&self) -> &Path {
        &self.base
    }

    fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempSkillWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

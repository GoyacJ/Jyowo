use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

#[cfg(test)]
static TEMP_WORKSPACE_COUNTER: AtomicU64 = AtomicU64::new(0);
const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_millis(100);
const BACKEND_TIMEOUT_GRACE: Duration = Duration::from_millis(250);
const MAX_ARTIFACT_SCAN_ENTRIES: usize = 4096;
const MAX_SECRET_REDACTION_LOOKAHEAD_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy)]
struct ArtifactBaseline {
    byte_size: u64,
    content_hash: [u8; 32],
}

struct ArtifactFileScan {
    paths: Vec<PathBuf>,
    limited: bool,
}

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
    let secret_values = declared_secret_values(&request)?;
    let secret_lookahead = secret_values
        .first()
        .map_or(0, |value| value.len().saturating_sub(1) as u64);
    let capabilities = backend.capabilities();
    if !capabilities
        .supports_synchronous_kill_scope
        .contains(&KillScope::ProcessGroup)
    {
        return Err(SandboxError::CapabilityMismatch {
            capability: "synchronous_kill".to_owned(),
            detail: format!(
                "sandbox backend `{}` cannot synchronously cancel a skill script process group",
                backend.backend_id()
            ),
        });
    }
    if !capabilities.host_filesystem_isolation {
        return Err(SandboxError::CapabilityMismatch {
            capability: "host_filesystem".to_owned(),
            detail: format!(
                "sandbox backend `{}` cannot isolate skill scripts from host files",
                backend.backend_id()
            ),
        });
    }

    let workspace = TempSkillWorkspace::create(&ctx.workspace_root)?;
    let root = workspace.path().to_path_buf();
    // The backend sees only the private materialized package as its workspace.
    // The caller's workspace is never part of the script sandbox scope.
    ctx.workspace_root = root.clone();
    let script_path = safe_relative_path(&request.declaration.path)?;
    let mut mounted_files = Vec::with_capacity(request.files.len());
    let mut baseline = BTreeMap::new();

    for file in &request.files {
        let relative = safe_relative_path(Path::new(&file.path))?;
        let normalized = path_to_string(&relative);
        if baseline.contains_key(&normalized) {
            return Err(SandboxError::HostPathDenied {
                path: file.path.clone(),
            });
        }
        let content = file.content.as_bytes();
        baseline.insert(
            normalized.clone(),
            ArtifactBaseline {
                byte_size: content.len() as u64,
                content_hash: *blake3::hash(content).as_bytes(),
            },
        );
        let target = root.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io_error)?;
        }
        std::fs::write(target, file.content.as_bytes()).map_err(io_error)?;
        mounted_files.push(normalized);
    }
    mounted_files.sort();

    let script_path_string = path_to_string(&script_path);
    if !baseline.contains_key(&script_path_string) || !root.join(&script_path).is_file() {
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
        .max(request.declaration.max_stderr_bytes)
        .saturating_add(secret_lookahead);
    let authorized_env_keys = request.env.keys().cloned().collect();
    let secret_env_keys = request
        .env
        .keys()
        .filter(|name| {
            request
                .declaration
                .env
                .get(*name)
                .is_some_and(|declaration| declaration.secret)
        })
        .cloned()
        .collect();
    let spec = ExecSpec {
        command: command_for_script(&script_path).to_owned(),
        args: vec![script_path_string],
        env: request.env,
        authorized_env_keys,
        secret_env_keys,
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
            allowed_writable_subpaths: vec![PathBuf::from(".")],
        },
        output_policy: OutputPolicy {
            max_inline_bytes: stream_backend_limit,
            overflow: OutputOverflowPolicy::Truncate,
            redact_secrets: true,
        },
        required_kill_scope: Some(KillScope::ProcessGroup),
        required_synchronous_kill_scope: Some(KillScope::ProcessGroup),
    };

    let backend_id = backend.backend_id().to_owned();
    let started = Instant::now();
    let mut handle = execute_with_lifecycle(backend, spec, ctx).await?;
    let activity = Arc::clone(&handle.activity);
    let mut kill_on_drop = ProcessGroupKillOnDrop::new(Arc::clone(&activity));
    let stdout_task = tokio::spawn(collect_stream(
        handle.stdout.take(),
        request
            .declaration
            .max_stdout_bytes
            .saturating_add(secret_lookahead),
    ));
    let stderr_task = tokio::spawn(collect_stream(
        handle.stderr.take(),
        request
            .declaration
            .max_stderr_bytes
            .saturating_add(secret_lookahead),
    ));

    let mut wait = Box::pin(activity.wait());
    let outcome = match tokio::time::timeout(
        timeout.saturating_add(BACKEND_TIMEOUT_GRACE),
        &mut wait,
    )
    .await
    {
        Ok(outcome) => {
            activity.kill(9, KillScope::ProcessGroup).await?;
            kill_on_drop.disarm();
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
            kill_on_drop.disarm();
            None
        }
    };
    let (stdout, stderr) = join_streams(stdout_task, stderr_task).await?;
    let mut output_limited = stdout.truncated
        || stderr.truncated
        || stdout.bytes.len() as u64 > request.declaration.max_stdout_bytes
        || stderr.bytes.len() as u64 > request.declaration.max_stderr_bytes
        || (stdout.bytes.len() as u64).saturating_add(stderr.bytes.len() as u64)
            > request.declaration.max_output_bytes;
    let (mut artifacts, mut artifact_limited) = collect_artifacts(
        &root,
        &baseline,
        request.declaration.max_artifact_count,
        request
            .declaration
            .max_artifact_bytes
            .saturating_add(secret_lookahead),
    )?;
    artifact_limited |= artifacts
        .iter()
        .map(|artifact| artifact.content.len() as u64)
        .sum::<u64>()
        > request.declaration.max_artifact_bytes;
    let elapsed_ms = duration_ms(started.elapsed());
    let mut stdout = redact_secret_values(&String::from_utf8_lossy(&stdout.bytes), &secret_values);
    let mut stderr = redact_secret_values(&String::from_utf8_lossy(&stderr.bytes), &secret_values);
    output_limited |= bound_redacted_output(
        &mut stdout,
        &mut stderr,
        request.declaration.max_stdout_bytes,
        request.declaration.max_stderr_bytes,
        request.declaration.max_output_bytes,
    );
    if artifacts.iter().any(|artifact| {
        secret_values
            .iter()
            .any(|secret| artifact.path.contains(secret))
    }) {
        return Err(SandboxError::CapabilityMismatch {
            capability: "secret_redaction".to_owned(),
            detail: "skill script artifact path contains a declared secret".to_owned(),
        });
    }
    for artifact in &mut artifacts {
        artifact.content = redact_secret_values(&artifact.content, &secret_values);
        artifact.byte_size = artifact.content.len() as u64;
    }
    artifact_limited |=
        bound_redacted_artifacts(&mut artifacts, request.declaration.max_artifact_bytes);

    let (status, exit_code) = result_status(outcome.as_ref(), output_limited, artifact_limited);
    Ok(SkillScriptSandboxResult {
        status,
        exit_code,
        stdout,
        stderr,
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

fn declared_secret_values(
    request: &SkillScriptSandboxRequest,
) -> Result<Vec<String>, SandboxError> {
    let mut values = request
        .declaration
        .env
        .iter()
        .filter(|(_, declaration)| declaration.secret)
        .filter_map(|(name, _)| request.env.get(name))
        .filter(|value| !value.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    if values
        .iter()
        .any(|value| value.len() > MAX_SECRET_REDACTION_LOOKAHEAD_BYTES)
    {
        return Err(SandboxError::CapabilityMismatch {
            capability: "secret_redaction".to_owned(),
            detail: format!(
                "skill script secret exceeds the {MAX_SECRET_REDACTION_LOOKAHEAD_BYTES}-byte redaction limit"
            ),
        });
    }
    values.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    values.dedup();
    Ok(values)
}

fn redact_secret_values(value: &str, secrets: &[String]) -> String {
    let mut redacted = String::with_capacity(value.len());
    let mut offset = 0;
    while offset < value.len() {
        if let Some(secret) = secrets
            .iter()
            .find(|secret| value[offset..].starts_with(secret.as_str()))
        {
            redacted.push_str("[REDACTED]");
            offset += secret.len();
            continue;
        }
        let character = value[offset..]
            .chars()
            .next()
            .expect("offset must remain on a character boundary");
        redacted.push(character);
        offset += character.len_utf8();
    }
    redacted
}

fn bound_redacted_output(
    stdout: &mut String,
    stderr: &mut String,
    max_stdout_bytes: u64,
    max_stderr_bytes: u64,
    max_output_bytes: u64,
) -> bool {
    let mut limited = truncate_string(stdout, max_stdout_bytes);
    limited |= truncate_string(stderr, max_stderr_bytes);
    limited |= truncate_string(stdout, max_output_bytes);
    let remaining = max_output_bytes.saturating_sub(stdout.len() as u64);
    limited |= truncate_string(stderr, remaining);
    limited
}

fn bound_redacted_artifacts(artifacts: &mut Vec<SkillScriptArtifact>, max_bytes: u64) -> bool {
    let mut remaining = max_bytes;
    let mut limited = false;
    let mut keep = artifacts.len();
    for (index, artifact) in artifacts.iter_mut().enumerate() {
        if remaining == 0 && !artifact.content.is_empty() {
            keep = index;
            limited = true;
            break;
        }
        if artifact.content.len() as u64 > remaining {
            limited |= truncate_string(&mut artifact.content, remaining);
            artifact.truncated = true;
        }
        artifact.byte_size = artifact.content.len() as u64;
        remaining = remaining.saturating_sub(artifact.byte_size);
    }
    artifacts.truncate(keep);
    limited
}

fn truncate_string(value: &mut String, max_bytes: u64) -> bool {
    let max = to_usize(max_bytes);
    if value.len() <= max {
        return false;
    }
    let mut boundary = max;
    while !value.is_char_boundary(boundary) {
        boundary = boundary.saturating_sub(1);
    }
    value.truncate(boundary);
    true
}

struct ProcessGroupKillOnDrop {
    activity: Arc<dyn crate::ActivityHandle>,
    armed: bool,
}

impl ProcessGroupKillOnDrop {
    fn new(activity: Arc<dyn crate::ActivityHandle>) -> Self {
        Self {
            activity,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ProcessGroupKillOnDrop {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let _ = self.activity.kill_sync(9, KillScope::ProcessGroup);
    }
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
    baseline: &BTreeMap<String, ArtifactBaseline>,
    max_count: u64,
    max_bytes: u64,
) -> Result<(Vec<SkillScriptArtifact>, bool), SandboxError> {
    let scan = list_files_bounded(root, MAX_ARTIFACT_SCAN_ENTRIES)?;
    let mut paths = scan.paths;
    paths.sort();
    let mut artifacts = Vec::new();
    let mut remaining_bytes = max_bytes;
    let mut limited = scan.limited;

    for path in paths {
        let relative = path
            .strip_prefix(root)
            .map_err(|error| SandboxError::Message(error.to_string()))?;
        let relative_string = path_to_string(relative);
        if relative_string.starts_with(".jyowo-") {
            continue;
        }
        let metadata = std::fs::metadata(&path).map_err(io_error)?;
        if baseline
            .get(&relative_string)
            .is_some_and(|baseline| file_matches_baseline(&path, &metadata, *baseline))
        {
            continue;
        }
        if artifacts.len() as u64 >= max_count || remaining_bytes == 0 {
            limited = true;
            break;
        }
        let original_size = metadata.len();
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

fn file_matches_baseline(
    path: &Path,
    metadata: &std::fs::Metadata,
    baseline: ArtifactBaseline,
) -> bool {
    metadata.len() == baseline.byte_size
        && file_content_hash(path).is_ok_and(|hash| hash == baseline.content_hash)
}

fn file_content_hash(path: &Path) -> Result<[u8; 32], SandboxError> {
    let mut file = std::fs::File::open(path).map_err(io_error)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            return Ok(*hasher.finalize().as_bytes());
        }
        hasher.update(&buffer[..read]);
    }
}

fn list_files_bounded(root: &Path, max_entries: usize) -> Result<ArtifactFileScan, SandboxError> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut entries_scanned = 0_usize;
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path).map_err(io_error)? {
            if entries_scanned >= max_entries {
                return Ok(ArtifactFileScan {
                    paths: files,
                    limited: true,
                });
            }
            let entry = entry.map_err(io_error)?;
            entries_scanned += 1;
            let entry_path = entry.path();
            let file_type = entry.file_type().map_err(io_error)?;
            if file_type.is_dir() {
                pending.push(entry_path);
            } else if file_type.is_file() {
                files.push(entry_path);
            }
        }
    }
    Ok(ArtifactFileScan {
        paths: files,
        limited: false,
    })
}

#[cfg(test)]
mod artifact_scan_tests {
    use super::*;

    #[test]
    fn artifact_scan_stops_at_its_entry_budget() {
        let root = std::env::temp_dir().join(format!(
            "jyowo-artifact-scan-budget-{}-{}",
            std::process::id(),
            TEMP_WORKSPACE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("scan root must be created");
        for index in 0..64 {
            std::fs::write(root.join(format!("artifact-{index:03}.txt")), [])
                .expect("artifact must be created");
        }

        let scan = list_files_bounded(&root, 8).expect("bounded scan must succeed");

        assert!(scan.limited);
        assert!(scan.paths.len() <= 8);
        std::fs::remove_dir_all(root).expect("scan root must be removed");
    }
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
    directory: tempfile::TempDir,
}

impl TempSkillWorkspace {
    fn create(_workspace_root: &Path) -> Result<Self, SandboxError> {
        let mut builder = tempfile::Builder::new();
        builder.prefix("jyowo-skill-script-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(std::fs::Permissions::from_mode(0o700));
        }
        let directory = builder.tempdir().map_err(io_error)?;
        Ok(Self { directory })
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }
}

#[cfg(all(test, unix))]
mod temp_skill_workspace_tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    #[test]
    fn workspace_is_atomic_private_and_ignores_attacker_symlinks() {
        let caller_workspace = tempfile::tempdir().unwrap();
        let attacker_target = tempfile::tempdir().unwrap();
        std::fs::create_dir(caller_workspace.path().join(".jyowo")).unwrap();
        symlink(
            attacker_target.path(),
            caller_workspace.path().join(".jyowo/skill-script-runs"),
        )
        .unwrap();

        let workspace = TempSkillWorkspace::create(caller_workspace.path()).unwrap();
        let path = workspace.path().to_path_buf();
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert!(!metadata.file_type().is_symlink());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        assert!(!path.starts_with(attacker_target.path()));

        drop(workspace);
        assert!(!path.exists());
    }
}

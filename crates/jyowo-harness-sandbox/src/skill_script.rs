use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use harness_contracts::SandboxError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::process::Command;

static TEMP_WORKSPACE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillScriptPackFile {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillScriptSandboxRequest {
    pub script_path: String,
    pub input: Value,
    pub files: Vec<SkillScriptPackFile>,
    pub timeout: Duration,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
    pub network_allowed: bool,
    pub memory_limit_mb: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillScriptStatus {
    Succeeded,
    Failed,
    TimedOut,
    OutputLimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillScriptArtifact {
    pub path: String,
    pub content: String,
    pub byte_size: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillScriptSandboxResult {
    pub status: SkillScriptStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub elapsed_ms: u64,
    pub memory_mb: f64,
    pub memory_limit_mb: Option<u64>,
    pub network_enabled: bool,
    pub workspace_path: PathBuf,
    pub mounted_files: Vec<String>,
    pub artifacts: Vec<SkillScriptArtifact>,
}

pub async fn execute_skill_script(
    request: SkillScriptSandboxRequest,
) -> Result<SkillScriptSandboxResult, SandboxError> {
    let workspace = TempSkillWorkspace::create()?;
    let root = workspace.path().to_path_buf();
    let script_path = safe_relative_path(&request.script_path)?;
    let input_path = root.join(".jyowo-input.json");
    let stdout_path = root.join(".jyowo-stdout");
    let stderr_path = root.join(".jyowo-stderr");
    let mut mounted_files = Vec::new();
    let mut baseline = BTreeMap::new();

    for file in &request.files {
        let relative = safe_relative_path(&file.path)?;
        let target = root.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io_error)?;
        }
        std::fs::write(&target, &file.content).map_err(io_error)?;
        let normalized = path_to_string(&relative);
        baseline.insert(normalized.clone(), file_fingerprint(&target)?);
        mounted_files.push(normalized);
    }
    mounted_files.sort();

    std::fs::write(
        &input_path,
        serde_json::to_vec(&request.input)
            .map_err(|error| SandboxError::Message(error.to_string()))?,
    )
    .map_err(io_error)?;
    baseline.insert(
        ".jyowo-input.json".to_owned(),
        file_fingerprint(&input_path)?,
    );

    let started = Instant::now();
    let output = run_script_command(
        &root,
        &script_path,
        &input_path,
        &stdout_path,
        &stderr_path,
        &request,
    )
    .await?;
    let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let (status, exit_code, stdout, stderr) = match output {
        ScriptCommandOutput::TimedOut => (
            SkillScriptStatus::TimedOut,
            None,
            String::new(),
            "Skill script timed out".to_owned(),
        ),
        ScriptCommandOutput::Finished {
            exit_code,
            stdout,
            stderr,
        } => {
            let stdout_limited = limit_output(&stdout, request.max_stdout_bytes);
            let stderr_limited = limit_output(&stderr, request.max_stderr_bytes);
            let output_limited = stdout_limited.limited || stderr_limited.limited;
            let status = if output_limited {
                SkillScriptStatus::OutputLimitExceeded
            } else if exit_code == Some(0) {
                SkillScriptStatus::Succeeded
            } else {
                SkillScriptStatus::Failed
            };
            (
                status,
                exit_code,
                stdout_limited.value,
                stderr_limited.value,
            )
        }
    };

    Ok(SkillScriptSandboxResult {
        status,
        exit_code,
        stdout,
        stderr,
        elapsed_ms,
        memory_mb: f64::from(
            u32::try_from(request.memory_limit_mb.unwrap_or(0)).unwrap_or(u32::MAX),
        ),
        memory_limit_mb: request.memory_limit_mb,
        network_enabled: request.network_allowed,
        workspace_path: root.clone(),
        mounted_files,
        artifacts: collect_artifacts(&root, &baseline)?,
    })
}

enum ScriptCommandOutput {
    Finished {
        exit_code: Option<i32>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    TimedOut,
}

async fn run_script_command(
    root: &Path,
    script_path: &Path,
    input_path: &Path,
    stdout_path: &Path,
    stderr_path: &Path,
    request: &SkillScriptSandboxRequest,
) -> Result<ScriptCommandOutput, SandboxError> {
    let script = root.join(script_path);
    if !script.is_file() {
        return Err(SandboxError::HostPathDenied {
            path: path_to_string(script_path),
        });
    }

    let mut command = command_for_script(script_path, &script);
    command
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            std::fs::File::create(stdout_path).map_err(io_error)?,
        ))
        .stderr(Stdio::from(
            std::fs::File::create(stderr_path).map_err(io_error)?,
        ))
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin")
        .env("JYOWO_SKILL_INPUT", input_path)
        .env(
            "JYOWO_NETWORK_DISABLED",
            if request.network_allowed { "0" } else { "1" },
        )
        .kill_on_drop(true);

    let mut child = command.spawn().map_err(io_error)?;
    match tokio::time::timeout(request.timeout, child.wait()).await {
        Ok(Ok(status)) => Ok(ScriptCommandOutput::Finished {
            exit_code: status.code(),
            stdout: std::fs::read(stdout_path).map_err(io_error)?,
            stderr: std::fs::read(stderr_path).map_err(io_error)?,
        }),
        Ok(Err(error)) => Err(io_error(error)),
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            Ok(ScriptCommandOutput::TimedOut)
        }
    }
}

fn command_for_script(script_path: &Path, script: &Path) -> Command {
    match script_path.extension().and_then(|value| value.to_str()) {
        Some("py") => {
            let mut command = Command::new("python3");
            command.arg(script);
            command
        }
        _ => {
            let mut command = Command::new("/bin/sh");
            command.arg(script);
            command
        }
    }
}

struct LimitedOutput {
    value: String,
    limited: bool,
}

fn limit_output(bytes: &[u8], max_bytes: usize) -> LimitedOutput {
    let limited = bytes.len() > max_bytes;
    let end = if limited { max_bytes } else { bytes.len() };
    LimitedOutput {
        value: String::from_utf8_lossy(&bytes[..end]).to_string(),
        limited,
    }
}

fn collect_artifacts(
    root: &Path,
    baseline: &BTreeMap<String, FileFingerprint>,
) -> Result<Vec<SkillScriptArtifact>, SandboxError> {
    let mut artifacts = Vec::new();
    for path in list_files(root)? {
        let relative = path
            .strip_prefix(root)
            .map_err(|error| SandboxError::Message(error.to_string()))?;
        let relative_string = path_to_string(relative);
        if relative_string.starts_with(".jyowo-") {
            continue;
        }
        let fingerprint = file_fingerprint(&path)?;
        if baseline.get(&relative_string) == Some(&fingerprint) {
            continue;
        }
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        artifacts.push(SkillScriptArtifact {
            path: relative_string,
            content,
            byte_size: fingerprint.len,
        });
    }
    artifacts.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(artifacts)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileFingerprint {
    len: u64,
    modified_ns: u128,
}

fn file_fingerprint(path: &Path) -> Result<FileFingerprint, SandboxError> {
    let metadata = std::fs::metadata(path).map_err(io_error)?;
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());
    Ok(FileFingerprint {
        len: metadata.len(),
        modified_ns,
    })
}

fn safe_relative_path(value: &str) -> Result<PathBuf, SandboxError> {
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(SandboxError::HostPathDenied {
            path: value.to_owned(),
        });
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(SandboxError::HostPathDenied {
                    path: value.to_owned(),
                });
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(SandboxError::HostPathDenied {
            path: value.to_owned(),
        });
    }
    Ok(normalized)
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn io_error(error: std::io::Error) -> SandboxError {
    SandboxError::Message(error.to_string())
}

struct TempSkillWorkspace {
    path: PathBuf,
}

impl TempSkillWorkspace {
    fn create() -> Result<Self, SandboxError> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| SandboxError::Message(error.to_string()))?
            .as_nanos();
        let sequence = TEMP_WORKSPACE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "jyowo-skill-script-{}-{unique}-{sequence}",
            std::process::id(),
        ));
        std::fs::create_dir_all(&path).map_err(io_error)?;
        Ok(Self { path })
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

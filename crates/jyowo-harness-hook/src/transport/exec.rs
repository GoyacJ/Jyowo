use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use harness_contracts::{
    HookError, HookEventKind, HookFailureMode, TransportFailureKind, TrustLevel,
};
use tokio::io::AsyncWriteExt;
use tokio::process::{ChildStdin, Command};

use crate::{HookContext, HookEvent, HookHandler, HookOutcome, HookRegistrationKind};

use super::protocol::{decode_response, encode_request};
use super::{HookOutput, HookPayload, HookProtocolVersion, HookTransport};

#[derive(Debug, Clone)]
pub struct HookExecSpec {
    pub handler_id: String,
    pub interested_events: Vec<HookEventKind>,
    pub failure_mode: HookFailureMode,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub working_dir: WorkingDir,
    pub timeout: Duration,
    pub resource_limits: HookExecResourceLimits,
    pub signal_policy: HookExecSignalPolicy,
    pub protocol_version: HookProtocolVersion,
    pub trust: TrustLevel,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum WorkingDir {
    SessionWorkspace,
    Pinned(PathBuf),
    EphemeralTemp,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookExecResourceLimits {
    pub cpu_time: Duration,
    pub memory_bytes: u64,
    pub max_stdio_bytes: u64,
    pub max_open_files: u32,
    pub allow_network: bool,
}

impl Default for HookExecResourceLimits {
    fn default() -> Self {
        Self {
            cpu_time: Duration::from_secs(10),
            memory_bytes: 128 * 1024 * 1024,
            max_stdio_bytes: 1024 * 1024,
            max_open_files: 64,
            allow_network: false,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum HookExecSignalPolicy {
    GracefulThenKill { grace: Duration },
    ImmediateKill,
}

impl Default for HookExecSignalPolicy {
    fn default() -> Self {
        Self::GracefulThenKill {
            grace: Duration::from_secs(1),
        }
    }
}

#[derive(Clone)]
pub struct ExecHookTransport {
    spec: HookExecSpec,
}

impl ExecHookTransport {
    pub fn new(spec: HookExecSpec) -> Result<Self, HookError> {
        validate_spec(&spec)?;
        Ok(Self { spec })
    }

    pub fn spec(&self) -> &HookExecSpec {
        &self.spec
    }

    pub fn handler_id(&self) -> &str {
        &self.spec.handler_id
    }

    pub fn interested_events(&self) -> &[HookEventKind] {
        &self.spec.interested_events
    }
}

#[async_trait]
impl HookTransport for ExecHookTransport {
    async fn invoke(&self, payload: HookPayload) -> HookOutput {
        let request = encode_request(&payload, self.spec.protocol_version);
        let stdin = serde_json::to_vec(&request)
            .map_err(|error| HookError::ProtocolParse(error.to_string()))?;

        let mut command = Command::new(&self.spec.command);
        command.args(&self.spec.args);
        command.envs(&self.spec.env);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command.kill_on_drop(true);
        if let Some(cwd) = working_dir(&self.spec.working_dir, &payload) {
            command.current_dir(cwd);
        }

        let mut child = command.spawn().map_err(|error| HookError::Transport {
            kind: TransportFailureKind::NetworkError,
            detail: error.to_string(),
        })?;

        write_child_stdin(child.stdin.take(), stdin).await?;

        let output = tokio::time::timeout(self.effective_timeout(), child.wait_with_output())
            .await
            .map_err(|_| HookError::Timeout {
                handler_id: self.spec.handler_id.clone(),
            })?
            .map_err(|error| HookError::Transport {
                kind: TransportFailureKind::NetworkError,
                detail: error.to_string(),
            })?;

        if !output.status.success() {
            return Err(HookError::Transport {
                kind: TransportFailureKind::NonZeroExit {
                    code: output.status.code().unwrap_or(-1),
                },
                detail: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }
        if output.stdout.len().saturating_add(output.stderr.len()) as u64
            > self.spec.resource_limits.max_stdio_bytes
        {
            return Err(HookError::Transport {
                kind: TransportFailureKind::BodyTooLarge,
                detail: "exec hook stdio exceeded max_stdio_bytes".to_owned(),
            });
        }

        decode_response(&output.stdout, self.spec.protocol_version)
    }
}

impl ExecHookTransport {
    fn effective_timeout(&self) -> Duration {
        self.spec.timeout.min(self.spec.resource_limits.cpu_time)
    }
}

#[async_trait]
impl HookHandler for ExecHookTransport {
    fn handler_id(&self) -> &str {
        self.handler_id()
    }

    fn interested_events(&self) -> &[HookEventKind] {
        self.interested_events()
    }

    fn failure_mode(&self) -> HookFailureMode {
        self.spec.failure_mode
    }

    fn registration_kind(&self) -> HookRegistrationKind {
        HookRegistrationKind::Exec
    }

    fn declared_trust(&self) -> Option<TrustLevel> {
        Some(self.spec.trust)
    }

    async fn handle(&self, event: HookEvent, ctx: HookContext) -> Result<HookOutcome, HookError> {
        self.invoke(HookPayload { event, ctx }).await
    }
}

async fn write_child_stdin(
    child_stdin: Option<ChildStdin>,
    stdin: Vec<u8>,
) -> Result<(), HookError> {
    let Some(mut child_stdin) = child_stdin else {
        return Ok(());
    };
    match child_stdin.write_all(&stdin).await {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::BrokenPipe => return Ok(()),
        Err(error) => {
            return Err(HookError::Transport {
                kind: TransportFailureKind::NetworkError,
                detail: error.to_string(),
            });
        }
    }
    match child_stdin.shutdown().await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(HookError::Transport {
            kind: TransportFailureKind::NetworkError,
            detail: error.to_string(),
        }),
    }
}

fn validate_spec(spec: &HookExecSpec) -> Result<(), HookError> {
    if spec.trust != TrustLevel::AdminTrusted {
        return Err(HookError::Unauthorized(
            "exec hooks require admin trust".to_owned(),
        ));
    }
    if contains_shell_metacharacter(&spec.command) {
        return Err(HookError::Transport {
            kind: TransportFailureKind::NetworkError,
            detail: "exec hook command contains shell metacharacter".to_owned(),
        });
    }
    if spec.handler_id.trim().is_empty() {
        return Err(HookError::Message(
            "handler_id must not be empty".to_owned(),
        ));
    }
    if spec.interested_events.is_empty() {
        return Err(HookError::Message(
            "interested_events must not be empty".to_owned(),
        ));
    }
    validate_portable_resource_limits(spec)?;
    Ok(())
}

fn validate_portable_resource_limits(spec: &HookExecSpec) -> Result<(), HookError> {
    let defaults = HookExecResourceLimits::default();
    if spec.resource_limits.memory_bytes != defaults.memory_bytes {
        return unsupported_limit("memory_bytes");
    }
    if spec.resource_limits.max_open_files != defaults.max_open_files {
        return unsupported_limit("max_open_files");
    }
    Ok(())
}

fn unsupported_limit<T>(limit: &str) -> Result<T, HookError> {
    Err(HookError::Transport {
        kind: TransportFailureKind::UnsupportedLimit {
            limit: limit.to_owned(),
        },
        detail: "exec transport cannot enforce this resource limit portably".to_owned(),
    })
}

fn contains_shell_metacharacter(path: &std::path::Path) -> bool {
    let path = path.to_string_lossy();
    path.chars()
        .any(|ch| matches!(ch, '$' | ';' | '|' | '&' | '`' | '<' | '>' | '\n'))
}

fn working_dir(working_dir: &WorkingDir, payload: &HookPayload) -> Option<PathBuf> {
    match working_dir {
        WorkingDir::SessionWorkspace => payload.ctx.view.workspace_root().map(ToOwned::to_owned),
        WorkingDir::Pinned(path) => Some(path.clone()),
        WorkingDir::EphemeralTemp => Some(std::env::temp_dir()),
    }
}

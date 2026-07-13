//! Process sandbox backend contracts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::stream::BoxStream;
use harness_contracts::{
    BlobRef, BlobStore, CorrelationId, Event, ExecFingerprint, KillScope, NetworkAccess,
    NoopRedactor, RedactRules, Redactor, ResourceLimits, RunId, SandboxBackendFailedEvent,
    SandboxBackendFailurePhase, SandboxError, SandboxExitStatus, SandboxMode, SandboxPolicy,
    SandboxPolicyHash, SandboxPolicySummary, SandboxPostExecutionFailedEvent,
    SandboxPreflightFailedEvent, SandboxPreflightPassedEvent, SandboxPreflightStatus, SandboxScope,
    SessionId, SessionSnapshotKind, TenantId, ToolUseId, WorkspaceAccess,
};
use tokio::sync::Mutex;

use crate::cwd::CwdMarkerLine;

pub type Signal = i32;
pub type ProcessId = u32;
pub type ExitStatus = SandboxExitStatus;
pub type BoxStdin = Pin<Box<dyn tokio::io::AsyncWrite + Send + 'static>>;

#[async_trait]
pub trait SandboxBackend: Send + Sync + 'static {
    fn backend_id(&self) -> &str;

    fn candidate_backend_ids(&self) -> Vec<String> {
        vec![self.backend_id().to_owned()]
    }

    fn capabilities(&self) -> SandboxCapabilities;

    fn base_config(&self) -> SandboxBaseConfig {
        SandboxBaseConfig::default()
    }

    fn preflight_execute(&self, spec: &ExecSpec) -> Result<(), SandboxError> {
        validate_preflight_capabilities(self.backend_id(), &self.capabilities(), spec)
    }

    async fn before_execute(
        &self,
        _spec: &ExecSpec,
        _ctx: &ExecContext,
    ) -> Result<(), SandboxError> {
        Ok(())
    }

    async fn execute(
        &self,
        spec: ExecSpec,
        ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError>;

    async fn after_execute(
        &self,
        _outcome: &ExecOutcome,
        _ctx: &ExecContext,
    ) -> Result<(), SandboxError> {
        Ok(())
    }

    async fn snapshot_session(
        &self,
        spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError>;

    async fn restore_session(&self, snapshot: &SessionSnapshotFile) -> Result<(), SandboxError>;

    async fn shutdown(&self) -> Result<(), SandboxError>;
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum StdioSpec {
    Null,
    Piped,
    Inherit,
    File(PathBuf),
}

#[derive(Clone, PartialEq)]
pub struct ExecSpec {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub authorized_env_keys: BTreeSet<String>,
    /// Explicitly authorized environment keys whose values are secret.
    ///
    /// Secret values are injected into the target process but are excluded from
    /// deterministic fingerprints and host-visible launcher arguments.
    pub secret_env_keys: BTreeSet<String>,
    pub cwd: Option<PathBuf>,
    pub stdin: StdioSpec,
    pub stdout: StdioSpec,
    pub stderr: StdioSpec,
    pub timeout: Option<Duration>,
    pub activity_timeout: Option<Duration>,
    pub policy: SandboxPolicy,
    pub workspace_access: WorkspaceAccess,
    pub output_policy: OutputPolicy,
    pub required_kill_scope: Option<KillScope>,
    pub required_synchronous_kill_scope: Option<KillScope>,
}

impl std::fmt::Debug for ExecSpec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let env = self
            .env
            .iter()
            .map(|(key, value)| {
                (
                    key,
                    if self.secret_env_keys.contains(key) {
                        "[REDACTED]"
                    } else {
                        value.as_str()
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        formatter
            .debug_struct("ExecSpec")
            .field("command", &self.command)
            .field("args", &self.args)
            .field("env", &env)
            .field("authorized_env_keys", &self.authorized_env_keys)
            .field("secret_env_keys", &self.secret_env_keys)
            .field("cwd", &self.cwd)
            .field("stdin", &self.stdin)
            .field("stdout", &self.stdout)
            .field("stderr", &self.stderr)
            .field("timeout", &self.timeout)
            .field("activity_timeout", &self.activity_timeout)
            .field("policy", &self.policy)
            .field("workspace_access", &self.workspace_access)
            .field("output_policy", &self.output_policy)
            .field("required_kill_scope", &self.required_kill_scope)
            .field(
                "required_synchronous_kill_scope",
                &self.required_synchronous_kill_scope,
            )
            .finish()
    }
}

impl ExecSpec {
    pub fn canonical_fingerprint(&self, base: &SandboxBaseConfig) -> ExecFingerprint {
        let mut hasher = blake3::Hasher::new();
        write_field(&mut hasher, b"jyowo.exec_fingerprint.v2");
        write_string(&mut hasher, &self.command);
        write_usize(&mut hasher, self.args.len());
        for arg in &self.args {
            write_string(&mut hasher, arg);
        }

        write_usize(&mut hasher, self.authorized_env_keys.len());
        for key in &self.authorized_env_keys {
            write_string(&mut hasher, key);
        }
        write_usize(&mut hasher, self.secret_env_keys.len());
        for key in &self.secret_env_keys {
            write_string(&mut hasher, key);
        }
        let filtered_env = self.env.iter().filter(|(key, _)| {
            base.passthrough_env_keys.contains(*key) || self.authorized_env_keys.contains(*key)
        });
        write_usize(&mut hasher, filtered_env.clone().count());
        for (key, value) in filtered_env {
            write_string(&mut hasher, key);
            if self.secret_env_keys.contains(key) {
                write_field(&mut hasher, b"env:secret:present");
            } else {
                write_field(&mut hasher, b"env:public");
                write_string(&mut hasher, value);
            }
        }

        match &self.cwd {
            Some(cwd) => {
                write_field(&mut hasher, b"cwd:some");
                write_path(&mut hasher, &lexical_normalize_path(cwd));
            }
            None => write_field(&mut hasher, b"cwd:none"),
        }

        write_workspace_access(&mut hasher, &self.workspace_access);

        ExecFingerprint(*hasher.finalize().as_bytes())
    }
}

impl Default for ExecSpec {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            env: BTreeMap::new(),
            authorized_env_keys: BTreeSet::new(),
            secret_env_keys: BTreeSet::new(),
            cwd: None,
            stdin: StdioSpec::Piped,
            stdout: StdioSpec::Piped,
            stderr: StdioSpec::Piped,
            timeout: None,
            activity_timeout: None,
            policy: default_sandbox_policy(),
            workspace_access: WorkspaceAccess::None,
            output_policy: OutputPolicy::default(),
            required_kill_scope: None,
            required_synchronous_kill_scope: None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct OutputPolicy {
    pub max_inline_bytes: u64,
    pub overflow: OutputOverflowPolicy,
    pub redact_secrets: bool,
}

impl Default for OutputPolicy {
    fn default() -> Self {
        Self {
            max_inline_bytes: 1_048_576,
            overflow: OutputOverflowPolicy::SpillToBlob {
                head_bytes: 4096,
                tail_bytes: 4096,
            },
            redact_secrets: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum OutputOverflowPolicy {
    SpillToBlob { head_bytes: u32, tail_bytes: u32 },
    Truncate,
    AbortExec,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct OutputOverflow {
    pub stream: OutputStream,
    pub original_bytes: u64,
    pub effective_limit: u64,
    pub blob_ref: Option<BlobRef>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum OutputStream {
    Stdout,
    Stderr,
    Combined,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SandboxBaseConfig {
    pub passthrough_env_keys: BTreeSet<String>,
    pub denied_host_paths: Vec<PathBuf>,
    pub default_resource_limits: ResourceLimits,
    pub default_output_policy: OutputPolicy,
}

impl Default for SandboxBaseConfig {
    fn default() -> Self {
        Self {
            passthrough_env_keys: ["PATH", "LANG", "LC_ALL", "TERM"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            denied_host_paths: Vec::new(),
            default_resource_limits: default_resource_limits(),
            default_output_policy: OutputPolicy::default(),
        }
    }
}

pub struct ProcessHandle {
    pub pid: Option<ProcessId>,
    pub stdout: Option<BoxStream<'static, Bytes>>,
    pub stderr: Option<BoxStream<'static, Bytes>>,
    pub stdin: Option<BoxStdin>,
    pub cwd_marker: Option<BoxStream<'static, CwdMarkerLine>>,
    pub activity: Arc<dyn ActivityHandle>,
}

#[async_trait]
pub trait ActivityHandle: Send + Sync + 'static {
    async fn wait(&self) -> Result<ExecOutcome, SandboxError>;

    async fn kill(&self, signal: Signal, scope: KillScope) -> Result<(), SandboxError>;

    fn kill_sync(&self, _signal: Signal, scope: KillScope) -> Result<(), SandboxError> {
        Err(SandboxError::CapabilityMismatch {
            capability: "synchronous_kill".to_owned(),
            detail: format!("activity cannot synchronously kill execution scope: {scope:?}"),
        })
    }

    fn touch(&self);

    fn last_activity(&self) -> Instant;
}

pub async fn execute_with_lifecycle(
    backend: Arc<dyn SandboxBackend>,
    spec: ExecSpec,
    mut ctx: ExecContext,
) -> Result<ProcessHandle, SandboxError> {
    ctx.execution_id = NEXT_EXECUTION_ID.fetch_add(1, Ordering::Relaxed);
    let backend_id = backend.backend_id().to_owned();
    preflight_exec(backend.as_ref(), &spec, &ctx)?;
    if let Err(error) = backend.before_execute(&spec, &ctx).await {
        emit_backend_failed(
            &ctx,
            &backend_id,
            SandboxBackendFailurePhase::Execute,
            error.clone(),
        );
        return Err(error);
    }
    let mut handle = match backend.execute(spec, ctx.clone()).await {
        Ok(handle) => handle,
        Err(error) => {
            emit_backend_failed(
                &ctx,
                &backend_id,
                SandboxBackendFailurePhase::Execute,
                error.clone(),
            );
            return Err(error);
        }
    };
    handle.activity = Arc::new(LifecycleActivity {
        backend,
        backend_id,
        inner: handle.activity,
        ctx,
        outcome: Mutex::new(None),
        after_execute_started: AtomicBool::new(false),
    });
    Ok(handle)
}

#[derive(Debug, Clone, PartialEq)]
pub struct SandboxPreflightReport {
    pub backend_id: String,
    pub policy: SandboxPolicySummary,
    pub policy_hash: SandboxPolicyHash,
    pub capabilities: SandboxCapabilities,
}

pub fn preflight_exec(
    backend: &dyn SandboxBackend,
    spec: &ExecSpec,
    ctx: &ExecContext,
) -> Result<SandboxPreflightReport, SandboxError> {
    let backend_id = backend.backend_id().to_owned();
    let capabilities = backend.capabilities();
    let policy = sandbox_policy_summary(&spec.policy);
    let policy_hash = sandbox_policy_hash(&backend_id, &spec.policy);
    let report = SandboxPreflightReport {
        backend_id: backend_id.clone(),
        policy: policy.clone(),
        policy_hash: policy_hash.clone(),
        capabilities,
    };

    match backend.preflight_execute(spec) {
        Ok(()) => {
            ctx.event_sink
                .emit(Event::SandboxPreflightPassed(SandboxPreflightPassedEvent {
                    session_id: ctx.session_id,
                    run_id: ctx.run_id,
                    tool_use_id: ctx.tool_use_id,
                    backend_id,
                    status: SandboxPreflightStatus::Passed,
                    policy,
                    policy_hash,
                    at: Utc::now(),
                }))?;
            Ok(report)
        }
        Err(error) => {
            let redacted_error = redact_sandbox_error(error.clone(), ctx);
            ctx.event_sink
                .emit(Event::SandboxPreflightFailed(SandboxPreflightFailedEvent {
                    session_id: ctx.session_id,
                    run_id: ctx.run_id,
                    tool_use_id: ctx.tool_use_id,
                    backend_id,
                    status: SandboxPreflightStatus::Failed,
                    policy,
                    policy_hash,
                    reason: redacted_error.to_string(),
                    at: Utc::now(),
                }))?;
            Err(error)
        }
    }
}

pub async fn snapshot_with_lifecycle(
    backend: Arc<dyn SandboxBackend>,
    spec: &SnapshotSpec,
    ctx: &ExecContext,
) -> Result<SessionSnapshotFile, SandboxError> {
    let backend_id = backend.backend_id().to_owned();
    match backend.snapshot_session(spec).await {
        Ok(snapshot) => Ok(snapshot),
        Err(error) => {
            emit_backend_failed(
                ctx,
                &backend_id,
                SandboxBackendFailurePhase::Snapshot,
                error.clone(),
            );
            Err(error)
        }
    }
}

pub async fn restore_with_lifecycle(
    backend: Arc<dyn SandboxBackend>,
    snapshot: &SessionSnapshotFile,
    ctx: &ExecContext,
) -> Result<(), SandboxError> {
    let backend_id = backend.backend_id().to_owned();
    match backend.restore_session(snapshot).await {
        Ok(()) => Ok(()),
        Err(error) => {
            emit_backend_failed(
                ctx,
                &backend_id,
                SandboxBackendFailurePhase::Restore,
                error.clone(),
            );
            Err(error)
        }
    }
}

pub async fn shutdown_with_lifecycle(
    backend: Arc<dyn SandboxBackend>,
    ctx: &ExecContext,
) -> Result<(), SandboxError> {
    let backend_id = backend.backend_id().to_owned();
    match backend.shutdown().await {
        Ok(()) => Ok(()),
        Err(error) => {
            emit_backend_failed(
                ctx,
                &backend_id,
                SandboxBackendFailurePhase::Shutdown,
                error.clone(),
            );
            Err(error)
        }
    }
}

struct LifecycleActivity {
    backend: Arc<dyn SandboxBackend>,
    backend_id: String,
    inner: Arc<dyn ActivityHandle>,
    ctx: ExecContext,
    outcome: Mutex<Option<ExecOutcome>>,
    after_execute_started: AtomicBool,
}

#[async_trait]
impl ActivityHandle for LifecycleActivity {
    async fn wait(&self) -> Result<ExecOutcome, SandboxError> {
        let mut cached = self.outcome.lock().await;
        if let Some(outcome) = cached.clone() {
            return Ok(outcome);
        }

        let outcome = match self.inner.wait().await {
            Ok(outcome) => outcome,
            Err(error) => {
                emit_backend_failed(
                    &self.ctx,
                    &self.backend_id,
                    SandboxBackendFailurePhase::Wait,
                    error.clone(),
                );
                return Err(error);
            }
        };
        if !self.after_execute_started.swap(true, Ordering::SeqCst) {
            if let Err(error) = self.backend.after_execute(&outcome, &self.ctx).await {
                let _ = self.ctx.event_sink.emit(Event::SandboxPostExecutionFailed(
                    SandboxPostExecutionFailedEvent {
                        session_id: self.ctx.session_id,
                        run_id: self.ctx.run_id,
                        tool_use_id: self.ctx.tool_use_id,
                        backend_id: self.backend_id.clone(),
                        error: redact_sandbox_error(error, &self.ctx),
                        at: Utc::now(),
                    },
                ));
            }
        }
        *cached = Some(outcome.clone());
        Ok(outcome)
    }

    async fn kill(&self, signal: Signal, scope: KillScope) -> Result<(), SandboxError> {
        self.inner.kill(signal, scope).await
    }

    fn kill_sync(&self, signal: Signal, scope: KillScope) -> Result<(), SandboxError> {
        self.inner.kill_sync(signal, scope)
    }

    fn touch(&self) {
        self.inner.touch();
    }

    fn last_activity(&self) -> Instant {
        self.inner.last_activity()
    }
}

fn emit_backend_failed(
    ctx: &ExecContext,
    backend_id: &str,
    phase: SandboxBackendFailurePhase,
    error: SandboxError,
) {
    let _ = ctx
        .event_sink
        .emit(Event::SandboxBackendFailed(SandboxBackendFailedEvent {
            session_id: ctx.session_id,
            run_id: ctx.run_id,
            tool_use_id: ctx.tool_use_id,
            backend_id: backend_id.to_owned(),
            phase,
            error: redact_sandbox_error(error, ctx),
            at: Utc::now(),
        }));
}

pub fn validate_preflight_capabilities(
    backend_id: &str,
    capabilities: &SandboxCapabilities,
    spec: &ExecSpec,
) -> Result<(), SandboxError> {
    if capabilities.max_concurrent_execs == 0 {
        return Err(SandboxError::CapabilityMismatch {
            capability: "execute".to_owned(),
            detail: format!("sandbox backend `{backend_id}` does not support execution"),
        });
    }

    if !capabilities.network.supports(&spec.policy.network) {
        return Err(SandboxError::CapabilityMismatch {
            capability: "network".to_owned(),
            detail: format!(
                "sandbox backend `{backend_id}` cannot enforce network policy: {:?}",
                spec.policy.network
            ),
        });
    }

    if !capabilities.workspace.supports(&spec.workspace_access) {
        return Err(SandboxError::CapabilityMismatch {
            capability: "workspace_access".to_owned(),
            detail: format!(
                "sandbox backend `{backend_id}` does not support workspace access policy: {:?}",
                spec.workspace_access
            ),
        });
    }

    validate_secret_environment(backend_id, spec)?;

    if !spec.authorized_env_keys.is_empty() && !capabilities.supports_per_exec_env {
        return Err(SandboxError::CapabilityMismatch {
            capability: "environment".to_owned(),
            detail: format!(
                "sandbox backend `{backend_id}` cannot inject explicitly authorized environment variables"
            ),
        });
    }

    if let Some(required_scope) = spec.required_kill_scope {
        if !capabilities.supports_kill_scope.contains(&required_scope) {
            return Err(SandboxError::CapabilityMismatch {
                capability: "kill_scope".to_owned(),
                detail: format!(
                    "sandbox backend `{backend_id}` cannot kill execution scope: {required_scope:?}"
                ),
            });
        }
    }

    if let Some(required_scope) = spec.required_synchronous_kill_scope {
        if !capabilities
            .supports_synchronous_kill_scope
            .contains(&required_scope)
        {
            return Err(SandboxError::CapabilityMismatch {
                capability: "synchronous_kill".to_owned(),
                detail: format!(
                    "sandbox backend `{backend_id}` cannot synchronously kill execution scope: {required_scope:?}"
                ),
            });
        }
    }

    validate_resource_preflight(capabilities, &spec.policy.resource_limits)?;
    Ok(())
}

pub(crate) fn validate_secret_environment(
    backend_id: &str,
    spec: &ExecSpec,
) -> Result<(), SandboxError> {
    if spec.secret_env_keys.is_subset(&spec.authorized_env_keys)
        && spec
            .secret_env_keys
            .iter()
            .all(|key| spec.env.contains_key(key))
    {
        return Ok(());
    }
    Err(SandboxError::CapabilityMismatch {
        capability: "secret_environment".to_owned(),
        detail: format!(
            "sandbox backend `{backend_id}` requires every secret environment key to be explicitly authorized and present"
        ),
    })
}

fn validate_resource_preflight(
    capabilities: &SandboxCapabilities,
    limits: &ResourceLimits,
) -> Result<(), SandboxError> {
    let unsupported = if limits.max_memory_bytes.is_some()
        && !capabilities.resource_limit_support.memory
    {
        Some("memory")
    } else if limits.max_cpu_cores.is_some() && !capabilities.resource_limit_support.cpu {
        Some("cpu")
    } else if limits.max_pids.is_some() && !capabilities.resource_limit_support.pids {
        Some("pids")
    } else if limits.max_wall_clock_ms.is_some() && !capabilities.resource_limit_support.wall_clock
    {
        Some("wall_clock")
    } else if limits.max_open_files.is_some() && !capabilities.resource_limit_support.open_files {
        Some("open_files")
    } else {
        None
    };

    if let Some(limit) = unsupported {
        return Err(SandboxError::CapabilityMismatch {
            capability: "resource_limits".to_owned(),
            detail: format!("sandbox backend cannot enforce {limit} resource limit"),
        });
    }
    Ok(())
}

fn sandbox_policy_summary(policy: &SandboxPolicy) -> SandboxPolicySummary {
    SandboxPolicySummary {
        mode: policy.mode.clone(),
        scope: policy.scope.clone(),
        network: policy.network.clone(),
        resource_limits: policy.resource_limits.clone(),
    }
}

fn sandbox_policy_hash(backend_id: &str, policy: &SandboxPolicy) -> SandboxPolicyHash {
    let mut hasher = blake3::Hasher::new();
    write_field(&mut hasher, b"jyowo.sandbox_policy.v1");
    write_string(&mut hasher, backend_id);
    let policy_json = serde_json::to_vec(policy).unwrap_or_default();
    write_field(&mut hasher, &policy_json);
    SandboxPolicyHash::from_bytes(*hasher.finalize().as_bytes())
}

fn redact_sandbox_error(error: SandboxError, ctx: &ExecContext) -> SandboxError {
    let redact = |value: String| ctx.redactor.redact(&value, &RedactRules::default());

    match error {
        SandboxError::Message(message) => SandboxError::Message(redact(message)),
        SandboxError::Unavailable { backend, detail } => SandboxError::Unavailable {
            backend: redact(backend),
            detail: redact(detail),
        },
        SandboxError::CapabilityMismatch { capability, detail } => {
            SandboxError::CapabilityMismatch {
                capability: redact(capability),
                detail: redact(detail),
            }
        }
        SandboxError::Timeout { detail } => SandboxError::Timeout {
            detail: redact(detail),
        },
        SandboxError::InactivityTimeout { detail } => SandboxError::InactivityTimeout {
            detail: redact(detail),
        },
        SandboxError::OutputBudgetExceeded { limit } => {
            SandboxError::OutputBudgetExceeded { limit }
        }
        SandboxError::HostPathDenied { path } => {
            SandboxError::HostPathDenied { path: redact(path) }
        }
        SandboxError::ResourceLimitExceeded { limit, detail } => {
            SandboxError::ResourceLimitExceeded {
                limit: redact(limit),
                detail: redact(detail),
            }
        }
        SandboxError::SnapshotUnsupported { kind } => {
            SandboxError::SnapshotUnsupported { kind: redact(kind) }
        }
        SandboxError::ContainerLifecycleError { detail } => SandboxError::ContainerLifecycleError {
            detail: redact(detail),
        },
        SandboxError::WorkspaceSyncFailed {
            direction,
            program,
            detail,
        } => SandboxError::WorkspaceSyncFailed {
            direction: redact(direction),
            program: redact(program),
            detail: redact(detail),
        },
        SandboxError::CodeRuntime { detail } => SandboxError::CodeRuntime {
            detail: redact(detail),
        },
        other => SandboxError::Message(redact(other.to_string())),
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ExecOutcome {
    pub exit_status: ExitStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub stdout_bytes_observed: u64,
    pub stderr_bytes_observed: u64,
    pub overflow: Option<OutputOverflow>,
}

impl Default for ExecOutcome {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            exit_status: SandboxExitStatus::Code(0),
            started_at: now,
            finished_at: now,
            stdout_bytes_observed: 0,
            stderr_bytes_observed: 0,
            overflow: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SandboxCapabilities {
    pub supports_streaming: bool,
    pub supports_stdin: bool,
    pub supports_cwd_tracking: bool,
    pub cwd_marker_support: CwdMarkerSupport,
    pub supports_activity_heartbeat: bool,
    pub supports_interactive_shell: bool,
    pub supports_per_exec_env: bool,
    pub network: NetworkPolicySupport,
    pub workspace: WorkspacePolicySupport,
    /// The backend prevents a process from reading arbitrary host files outside
    /// the materialized sandbox workspace.
    pub host_filesystem_isolation: bool,
    pub supports_gpu: bool,
    pub supports_pty: bool,
    pub supports_detach: bool,
    pub supports_workspace_sync: bool,
    pub supports_session_snapshot: bool,
    pub max_concurrent_execs: u32,
    pub supports_kill_scope: Vec<KillScope>,
    pub supports_synchronous_kill_scope: Vec<KillScope>,
    pub snapshot_kinds: BTreeSet<SessionSnapshotKind>,
    pub resource_limit_support: ResourceLimitSupport,
    pub default_timeout: Duration,
}

impl Default for SandboxCapabilities {
    fn default() -> Self {
        Self {
            supports_streaming: false,
            supports_stdin: false,
            supports_cwd_tracking: false,
            cwd_marker_support: CwdMarkerSupport::Disabled,
            supports_activity_heartbeat: false,
            supports_interactive_shell: false,
            supports_per_exec_env: false,
            network: NetworkPolicySupport::default(),
            workspace: WorkspacePolicySupport::default(),
            host_filesystem_isolation: false,
            supports_gpu: false,
            supports_pty: false,
            supports_detach: false,
            supports_workspace_sync: false,
            supports_session_snapshot: false,
            max_concurrent_execs: 0,
            supports_kill_scope: vec![KillScope::Process],
            supports_synchronous_kill_scope: Vec::new(),
            snapshot_kinds: BTreeSet::new(),
            resource_limit_support: ResourceLimitSupport::default(),
            default_timeout: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash)]
pub enum CwdMarkerSupport {
    #[default]
    Disabled,
    FinalShellCwd,
    CommandTimeline,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash)]
pub struct ResourceLimitSupport {
    pub memory: bool,
    pub cpu: bool,
    pub pids: bool,
    pub wall_clock: bool,
    pub open_files: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct NetworkPolicySupport {
    pub none: bool,
    pub loopback_only: bool,
    pub allowlist: bool,
    pub unrestricted: bool,
}

impl Default for NetworkPolicySupport {
    fn default() -> Self {
        Self {
            none: false,
            loopback_only: false,
            allowlist: false,
            unrestricted: false,
        }
    }
}

impl NetworkPolicySupport {
    pub fn supports(&self, access: &NetworkAccess) -> bool {
        match access {
            NetworkAccess::None => self.none,
            NetworkAccess::LoopbackOnly => self.loopback_only,
            NetworkAccess::AllowList(_) => self.allowlist,
            NetworkAccess::Unrestricted => self.unrestricted,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct WorkspacePolicySupport {
    pub read_write_all: bool,
    pub read_only: bool,
    pub writable_subpaths: bool,
}

impl Default for WorkspacePolicySupport {
    fn default() -> Self {
        Self {
            read_write_all: false,
            read_only: false,
            writable_subpaths: false,
        }
    }
}

impl WorkspacePolicySupport {
    pub fn supports(&self, access: &WorkspaceAccess) -> bool {
        match access {
            WorkspaceAccess::ReadWrite {
                allowed_writable_subpaths,
            } if allowed_writable_subpaths.is_empty() => self.read_write_all,
            WorkspaceAccess::ReadOnly => self.read_only,
            WorkspaceAccess::ReadWrite { .. } => self.writable_subpaths,
            WorkspaceAccess::None => true,
            _ => false,
        }
    }
}

static NEXT_EXECUTION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct ExecContext {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: Option<ToolUseId>,
    pub tenant_id: TenantId,
    pub workspace_root: PathBuf,
    pub correlation_id: CorrelationId,
    pub event_sink: Arc<dyn EventSink>,
    pub redactor: Arc<dyn Redactor>,
    pub blob_store: Option<Arc<dyn BlobStore>>,
    /// Internal per-execution id assigned by `execute_with_lifecycle` before preflight.
    /// Not a public serde contract; must not be exposed to the frontend.
    #[doc(hidden)]
    pub execution_id: u64,
}

impl ExecContext {
    pub fn new(event_sink: Arc<dyn EventSink>) -> Self {
        Self {
            session_id: SessionId::new(),
            run_id: RunId::new(),
            tool_use_id: None,
            tenant_id: TenantId::SINGLE,
            workspace_root: PathBuf::new(),
            correlation_id: CorrelationId::new(),
            event_sink,
            redactor: Arc::new(NoopRedactor),
            blob_store: None,
            execution_id: NEXT_EXECUTION_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub fn for_test(event_sink: Arc<dyn EventSink>) -> Self {
        Self::new(event_sink)
    }
}

pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: Event) -> Result<(), SandboxError>;
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct SnapshotSpec {
    pub session_id: SessionId,
    pub kind: SessionSnapshotKind,
    pub target_path: Option<PathBuf>,
}

impl Default for SnapshotSpec {
    fn default() -> Self {
        Self {
            session_id: SessionId::new(),
            kind: SessionSnapshotKind::FilesystemImage,
            target_path: None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct SessionSnapshotFile {
    pub session_id: SessionId,
    pub kind: SessionSnapshotKind,
    pub path: PathBuf,
    pub metadata: SnapshotMetadata,
}

impl Default for SessionSnapshotFile {
    fn default() -> Self {
        Self {
            session_id: SessionId::new(),
            kind: SessionSnapshotKind::FilesystemImage,
            path: PathBuf::new(),
            metadata: SnapshotMetadata::default(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct SnapshotMetadata {
    pub size_bytes: u64,
    pub content_hash: [u8; 32],
    pub created_at: DateTime<Utc>,
}

impl Default for SnapshotMetadata {
    fn default() -> Self {
        Self {
            size_bytes: 0,
            content_hash: [0; 32],
            created_at: Utc::now(),
        }
    }
}

fn default_sandbox_policy() -> SandboxPolicy {
    SandboxPolicy {
        mode: SandboxMode::None,
        scope: SandboxScope::WorkspaceOnly,
        network: NetworkAccess::None,
        resource_limits: default_resource_limits(),
        denied_host_paths: Vec::new(),
    }
}

fn default_resource_limits() -> ResourceLimits {
    ResourceLimits {
        max_memory_bytes: None,
        max_cpu_cores: None,
        max_pids: None,
        max_wall_clock_ms: None,
        max_open_files: None,
    }
}

#[allow(dead_code)]
pub(crate) fn apply_wall_clock_resource_limit(spec: &mut ExecSpec, defaults: &ResourceLimits) {
    let wall_clock_ms = spec
        .policy
        .resource_limits
        .max_wall_clock_ms
        .or(defaults.max_wall_clock_ms);
    let Some(resource_timeout) = wall_clock_ms.map(Duration::from_millis) else {
        return;
    };
    spec.timeout = Some(match spec.timeout {
        Some(timeout) => timeout.min(resource_timeout),
        None => resource_timeout,
    });
}

#[allow(dead_code)]
pub(crate) fn has_non_wall_clock_resource_limits(limits: &ResourceLimits) -> bool {
    limits.max_memory_bytes.is_some()
        || limits.max_cpu_cores.is_some()
        || limits.max_pids.is_some()
        || limits.max_open_files.is_some()
}

#[allow(dead_code)]
pub(crate) fn unsupported_resource_limits(detail: impl Into<String>) -> SandboxError {
    SandboxError::CapabilityMismatch {
        capability: "resource_limits".to_owned(),
        detail: detail.into(),
    }
}

fn write_workspace_access(hasher: &mut blake3::Hasher, access: &WorkspaceAccess) {
    match access {
        WorkspaceAccess::None => write_field(hasher, b"workspace_access:none"),
        WorkspaceAccess::ReadOnly => write_field(hasher, b"workspace_access:read_only"),
        WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths,
        } => {
            write_field(hasher, b"workspace_access:read_write");
            let mut paths = allowed_writable_subpaths
                .iter()
                .map(|path| lexical_normalize_path(path))
                .collect::<Vec<_>>();
            paths.sort();
            write_usize(hasher, paths.len());
            for path in paths {
                write_path(hasher, &path);
            }
        }
        _ => write_field(hasher, b"workspace_access:unknown"),
    }
}

pub(crate) fn lexical_normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let popped = normalized.pop();
                if !popped {
                    normalized.push("..");
                }
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
        }
    }

    normalized
}

fn write_path(hasher: &mut blake3::Hasher, path: &Path) {
    write_string(hasher, &path.to_string_lossy());
}

fn write_string(hasher: &mut blake3::Hasher, value: &str) {
    write_field(hasher, value.as_bytes());
}

fn write_field(hasher: &mut blake3::Hasher, value: &[u8]) {
    write_usize(hasher, value.len());
    hasher.update(value);
}

fn write_usize(hasher: &mut blake3::Hasher, value: usize) {
    hasher.update(&(value as u64).to_le_bytes());
}

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::future::Future;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
#[cfg(unix)]
use command_fds::{CommandFdExt, FdMapping};
use futures::StreamExt;
use harness_contracts::{
    BlobMeta, BlobRef, BlobRetention, Event, ExecFingerprint, KillScope, NetworkAccess,
    RedactRules, ResourceLimits, SandboxActivityHeartbeatEvent, SandboxActivityTimeoutFiredEvent,
    SandboxBackpressureAppliedEvent, SandboxError, SandboxExecutionCompletedEvent,
    SandboxExecutionStartedEvent, SandboxExitStatus, SandboxOutputSpilledEvent,
    SandboxOutputStream, SandboxOverflowSummary, SandboxPolicySummary, SandboxScope,
    SandboxSnapshotCreatedEvent, SessionSnapshotKind, WorkspaceAccess,
};
use parking_lot::Mutex as SyncMutex;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tokio::task::JoinHandle;
use tokio_util::io::ReaderStream;

use super::LocalIsolation;
use super::LocalSandbox;
use crate::cwd::CwdMarkerLine;
use crate::{
    backend::{apply_wall_clock_resource_limit, lexical_normalize_path},
    ActivityHandle, CwdMarkerSupport, ExecContext, ExecOutcome, ExecSpec, NetworkPolicySupport,
    OutputOverflow, OutputOverflowPolicy, OutputStream, ProcessHandle, ResourceLimitSupport,
    SandboxBackend, SandboxBaseConfig, SandboxCapabilities, SessionSnapshotFile, Signal,
    SnapshotMetadata, SnapshotSpec, StdioSpec, WorkspacePolicySupport, WrappedCommand,
};

const BACKEND_ID: &str = "local";
const NO_CACHED_SIGNAL: i32 = i32::MIN;

#[async_trait]
impl SandboxBackend for LocalSandbox {
    fn backend_id(&self) -> &str {
        BACKEND_ID
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            supports_streaming: true,
            supports_stdin: true,
            supports_cwd_tracking: cfg!(unix),
            cwd_marker_support: if cfg!(unix) {
                CwdMarkerSupport::FinalShellCwd
            } else {
                CwdMarkerSupport::Disabled
            },
            supports_activity_heartbeat: true,
            supports_interactive_shell: cfg!(unix),
            supports_per_exec_env: true,
            network: network_policy_support_for_isolation(self.isolation),
            workspace: workspace_policy_support_for_isolation(self.isolation),
            supports_gpu: false,
            supports_pty: false,
            supports_detach: false,
            supports_workspace_sync: false,
            supports_session_snapshot: true,
            max_concurrent_execs: u32::MAX,
            supports_kill_scope: local_kill_scopes(self.isolation),
            supports_synchronous_kill_scope: local_synchronous_kill_scopes(self.isolation),
            snapshot_kinds: BTreeSet::from([
                SessionSnapshotKind::FilesystemImage,
                SessionSnapshotKind::ShellState,
            ]),
            resource_limit_support: ResourceLimitSupport {
                wall_clock: true,
                ..ResourceLimitSupport::default()
            },
            default_timeout: Duration::from_secs(300),
        }
    }

    fn base_config(&self) -> SandboxBaseConfig {
        self.base.clone()
    }

    async fn execute(
        &self,
        mut spec: ExecSpec,
        ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        validate_local_exec(self, &spec)?;
        apply_supported_resource_limits(&mut spec, &self.base.default_resource_limits);

        let cwd = resolve_cwd(&self.root, spec.cwd.as_deref(), &spec.policy.scope)?;
        let environment = filtered_env(&self.base.passthrough_env_keys, &spec)
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        let (mut command, cwd_marker) =
            wrapped_command_for_local(&spec, self.isolation, &self.root, &cwd, &environment)?
                .into_parts();
        let process_group = ProcessGroupKeeper::for_spec(&spec)?;
        configure_process_group(&mut command, process_group.as_ref());
        command
            .current_dir(cwd)
            .stdin(stdio(&spec.stdin)?)
            .stdout(stdio(&spec.stdout)?)
            .stderr(stdio(&spec.stderr)?)
            .env_clear();
        if !self.isolation.is_os_level() {
            command.envs(&environment);
        }

        let mut child = command.spawn().map_err(sandbox_error)?;
        let pid = child.id();
        let stdin = child
            .stdin
            .take()
            .map(|stdin| Box::pin(stdin) as crate::BoxStdin);
        let stdout_reader = child.stdout.take();
        let stderr_reader = child.stderr.take();
        let fingerprint = spec.canonical_fingerprint(&self.base);

        let activity = Arc::new(LocalActivity::new(
            child,
            process_group,
            spec.clone(),
            ctx.clone(),
            fingerprint,
        ));
        LocalActivity::start_periodic_heartbeat(&activity);
        let stdout = child_stream(stdout_reader, Arc::clone(&activity), OutputStream::Stdout);
        let stderr = child_stream(stderr_reader, Arc::clone(&activity), OutputStream::Stderr);

        ctx.event_sink.emit(Event::SandboxExecutionStarted(
            SandboxExecutionStartedEvent {
                session_id: ctx.session_id,
                run_id: ctx.run_id,
                tool_use_id: ctx.tool_use_id,
                backend_id: BACKEND_ID.to_owned(),
                fingerprint,
                policy: SandboxPolicySummary {
                    mode: spec.policy.mode.clone(),
                    scope: spec.policy.scope.clone(),
                    network: spec.policy.network.clone(),
                    resource_limits: spec.policy.resource_limits.clone(),
                },
                at: Utc::now(),
            },
        ))?;

        Ok(ProcessHandle {
            pid,
            stdout,
            stderr,
            stdin,
            cwd_marker,
            activity,
        })
    }

    async fn before_execute(
        &self,
        spec: &ExecSpec,
        _ctx: &ExecContext,
    ) -> Result<(), SandboxError> {
        validate_local_exec(self, spec)
    }

    async fn snapshot_session(
        &self,
        spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        let snapshot = create_local_snapshot(&self.root, spec)?;
        if let Some(event_sink) = &self.snapshot_event_sink {
            event_sink.emit(Event::SandboxSnapshotCreated(SandboxSnapshotCreatedEvent {
                session_id: snapshot.session_id,
                backend_id: BACKEND_ID.to_owned(),
                kind: snapshot.kind,
                size_bytes: snapshot.metadata.size_bytes,
                content_hash: snapshot.metadata.content_hash,
                at: Utc::now(),
            }))?;
        }
        Ok(snapshot)
    }

    async fn restore_session(&self, snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        restore_local_snapshot(&self.root, snapshot)
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

fn validate_local_exec(sandbox: &LocalSandbox, spec: &ExecSpec) -> Result<(), SandboxError> {
    let cwd = resolve_cwd(&sandbox.root, spec.cwd.as_deref(), &spec.policy.scope)?;
    validate_network_policy(sandbox.isolation, &spec.policy.network)?;
    validate_resource_policy(sandbox.isolation, &spec.policy.resource_limits)?;
    validate_resource_policy(sandbox.isolation, &sandbox.base.default_resource_limits)?;
    validate_isolation(sandbox.isolation)?;
    validate_workspace_access(
        sandbox.isolation,
        &sandbox.root,
        &spec.policy.scope,
        &spec.workspace_access,
    )?;
    validate_denied_paths(&sandbox.root, &cwd, spec, &sandbox.base.denied_host_paths)?;
    validate_denied_paths(&sandbox.root, &cwd, spec, &spec.policy.denied_host_paths)?;
    Ok(())
}

pub struct LocalActivity {
    pub(crate) child: AsyncMutex<Option<Child>>,
    process_group: AsyncMutex<Option<ProcessGroupKeeper>>,
    process_group_target: Option<ProcessGroupTarget>,
    spec: ExecSpec,
    ctx: ExecContext,
    started_at: chrono::DateTime<Utc>,
    started_instant: Instant,
    fingerprint: ExecFingerprint,
    last_activity_ms: AtomicU64,
    stdout_bytes: AtomicU64,
    stderr_bytes: AtomicU64,
    outcome: AsyncMutex<Option<ExecOutcome>>,
    overflow: AsyncMutex<Option<OutputOverflow>>,
    spill: AsyncMutex<Option<SpillState>>,
    killed_signal: AtomicI32,
    output_tasks: SyncMutex<Vec<JoinHandle<()>>>,
}

struct SpillState {
    stream: OutputStream,
    path: Option<PathBuf>,
    bytes: Vec<u8>,
}

struct SpillPreview {
    limit: u64,
    head_limit: usize,
    tail_limit: usize,
    observed: u64,
    overflowed: bool,
    inline: Vec<u8>,
    head: Vec<u8>,
    tail: Vec<u8>,
}

impl SpillPreview {
    fn new(limit: u64, head_bytes: u32, tail_bytes: u32) -> Self {
        let head_limit = u64::from(head_bytes).min(limit) as usize;
        let remaining = limit.saturating_sub(head_limit as u64);
        let tail_limit = u64::from(tail_bytes).min(remaining) as usize;
        Self {
            limit,
            head_limit,
            tail_limit,
            observed: 0,
            overflowed: false,
            inline: Vec::new(),
            head: Vec::new(),
            tail: Vec::new(),
        }
    }

    fn push_inline(&mut self, bytes: &[u8]) -> bool {
        self.inline.extend_from_slice(bytes);
        self.observed += bytes.len() as u64;
        self.observed > self.limit
    }

    fn feed_overflow_bytes(&mut self, bytes: &[u8]) -> Option<Vec<u8>> {
        let mut rest = bytes;
        if self.head.len() < self.head_limit {
            let take = (self.head_limit - self.head.len()).min(rest.len());
            self.head.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
        }
        if rest.is_empty() {
            return None;
        }
        if self.tail_limit == 0 {
            return Some(rest.to_vec());
        }
        self.tail.extend_from_slice(rest);
        if self.tail.len() <= self.tail_limit {
            return None;
        }
        let spill_len = self.tail.len() - self.tail_limit;
        Some(self.tail.drain(..spill_len).collect())
    }

    fn finish(mut self) -> Option<Bytes> {
        let bytes = if self.overflowed {
            self.head.extend_from_slice(&self.tail);
            self.head
        } else {
            self.inline
        };
        if bytes.is_empty() {
            None
        } else {
            Some(Bytes::from(bytes))
        }
    }
}

impl LocalActivity {
    fn new(
        child: Child,
        process_group: Option<ProcessGroupKeeper>,
        spec: ExecSpec,
        ctx: ExecContext,
        fingerprint: ExecFingerprint,
    ) -> Self {
        let process_group_target = process_group.as_ref().map(ProcessGroupKeeper::target);
        Self {
            child: AsyncMutex::new(Some(child)),
            process_group: AsyncMutex::new(process_group),
            process_group_target,
            spec,
            ctx,
            started_at: Utc::now(),
            started_instant: Instant::now(),
            fingerprint,
            last_activity_ms: AtomicU64::new(0),
            stdout_bytes: AtomicU64::new(0),
            stderr_bytes: AtomicU64::new(0),
            outcome: AsyncMutex::new(None),
            overflow: AsyncMutex::new(None),
            spill: AsyncMutex::new(None),
            killed_signal: AtomicI32::new(NO_CACHED_SIGNAL),
            output_tasks: SyncMutex::new(Vec::new()),
        }
    }

    async fn process_output(&self, stream: OutputStream, bytes: Bytes) -> Option<Bytes> {
        let bytes = self.redact_output(bytes);
        let previous = match stream {
            OutputStream::Stdout => self
                .stdout_bytes
                .fetch_add(bytes.len() as u64, Ordering::Relaxed),
            OutputStream::Stderr => self
                .stderr_bytes
                .fetch_add(bytes.len() as u64, Ordering::Relaxed),
            OutputStream::Combined => 0,
        };
        self.touch();

        let limit = self.spec.output_policy.max_inline_bytes;
        let observed = previous + bytes.len() as u64;
        if observed <= limit {
            return Some(bytes);
        }

        let new_overflow = self.record_overflow(stream, observed, limit, None).await;
        if new_overflow {
            self.emit_backpressure(observed.saturating_sub(limit), Duration::ZERO);
        }

        match self.spec.output_policy.overflow {
            OutputOverflowPolicy::Truncate => prefix_within_limit(bytes, previous, limit),
            OutputOverflowPolicy::SpillToBlob { .. } => None,
            OutputOverflowPolicy::AbortExec => {
                if let Some(child) = self.child.lock().await.as_mut() {
                    let _ = child.start_kill();
                }
                None
            }
        }
    }

    async fn process_spill_output(
        &self,
        state: &mut SpillPreview,
        stream: OutputStream,
        bytes: Bytes,
    ) {
        let bytes = self.redact_output(bytes);
        match stream {
            OutputStream::Stdout => {
                self.stdout_bytes
                    .fetch_add(bytes.len() as u64, Ordering::Relaxed);
            }
            OutputStream::Stderr => {
                self.stderr_bytes
                    .fetch_add(bytes.len() as u64, Ordering::Relaxed);
            }
            OutputStream::Combined => {}
        }
        self.touch();

        if !state.overflowed {
            if !state.push_inline(&bytes) {
                return;
            }
            state.overflowed = true;
            if self
                .record_overflow(stream, state.observed, state.limit, None)
                .await
            {
                self.emit_backpressure(state.observed.saturating_sub(state.limit), Duration::ZERO);
            }
            let buffered = std::mem::take(&mut state.inline);
            if let Some(spill) = state.feed_overflow_bytes(&buffered) {
                let _ = self.append_spill(stream, &spill).await;
            }
            return;
        }

        state.observed += bytes.len() as u64;
        if let Some(spill) = state.feed_overflow_bytes(&bytes) {
            let _ = self.append_spill(stream, &spill).await;
        }
    }

    async fn record_overflow(
        &self,
        stream: OutputStream,
        original_bytes: u64,
        effective_limit: u64,
        blob_ref: Option<BlobRef>,
    ) -> bool {
        let mut overflow = self.overflow.lock().await;
        if overflow.is_some() {
            return false;
        }
        *overflow = Some(OutputOverflow {
            stream,
            original_bytes,
            effective_limit,
            blob_ref,
        });
        true
    }

    async fn append_spill(&self, stream: OutputStream, bytes: &[u8]) -> Result<(), SandboxError> {
        let path = {
            let mut spill = self.spill.lock().await;
            if spill.is_none() {
                let path = if self.ctx.blob_store.is_some() {
                    None
                } else {
                    let blob_id = harness_contracts::BlobId::new();
                    let dir = self
                        .ctx
                        .workspace_root
                        .join(".jyowo")
                        .join("sandbox-output");
                    std::fs::create_dir_all(&dir).map_err(sandbox_error)?;
                    Some(dir.join(format!("{blob_id}.bin")))
                };
                *spill = Some(SpillState {
                    stream,
                    path,
                    bytes: Vec::new(),
                });
            }
            if let Some(spill) = spill.as_mut() {
                if self.ctx.blob_store.is_some() {
                    spill.bytes.extend_from_slice(bytes);
                    return Ok(());
                }
                spill.path.clone().expect("file spill path initialized")
            } else {
                unreachable!("spill initialized")
            }
        };

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(sandbox_error)?;
        file.write_all(bytes).map_err(sandbox_error)
    }

    async fn finalize_overflow(&self) -> Result<Option<OutputOverflow>, SandboxError> {
        let mut overflow = self.overflow.lock().await;
        let Some(mut overflow_value) = overflow.clone() else {
            return Ok(None);
        };
        overflow_value.original_bytes = match overflow_value.stream {
            OutputStream::Stdout => self.stdout_bytes.load(Ordering::Relaxed),
            OutputStream::Stderr => self.stderr_bytes.load(Ordering::Relaxed),
            OutputStream::Combined => {
                self.stdout_bytes.load(Ordering::Relaxed)
                    + self.stderr_bytes.load(Ordering::Relaxed)
            }
        };

        let spill = self.spill.lock().await.as_ref().map(|spill| {
            (
                spill.stream,
                spill.path.clone(),
                Bytes::copy_from_slice(&spill.bytes),
            )
        });
        if let Some((stream, path, bytes)) = spill {
            let (head_bytes, tail_bytes) = match self.spec.output_policy.overflow {
                OutputOverflowPolicy::SpillToBlob {
                    head_bytes,
                    tail_bytes,
                } => effective_spill_preview_limits(
                    self.spec.output_policy.max_inline_bytes,
                    head_bytes,
                    tail_bytes,
                ),
                OutputOverflowPolicy::Truncate | OutputOverflowPolicy::AbortExec => (0, 0),
            };
            let blob_ref = if let Some(blob_store) = &self.ctx.blob_store {
                blob_ref_for_store(
                    Arc::clone(blob_store),
                    self.ctx.tenant_id,
                    self.ctx.session_id,
                    bytes,
                )
                .await?
            } else {
                blob_ref_for_file(path.as_deref().expect("file spill path initialized"))?
            };
            self.ctx
                .event_sink
                .emit(Event::SandboxOutputSpilled(SandboxOutputSpilledEvent {
                    session_id: self.ctx.session_id,
                    run_id: self.ctx.run_id,
                    tool_use_id: self.ctx.tool_use_id,
                    stream: sandbox_output_stream(stream),
                    blob_ref: blob_ref.clone(),
                    head_bytes,
                    tail_bytes,
                    original_bytes: overflow_value.original_bytes,
                    at: Utc::now(),
                }))?;
            overflow_value.blob_ref = Some(blob_ref);
        }

        *overflow = Some(overflow_value.clone());
        Ok(Some(overflow_value))
    }

    fn cached_signal(&self) -> Option<Signal> {
        match self.killed_signal.load(Ordering::Relaxed) {
            NO_CACHED_SIGNAL => None,
            signal => Some(signal),
        }
    }

    fn elapsed_since_start_ms(&self) -> u64 {
        self.started_instant.elapsed().as_millis() as u64
    }

    fn emit_backpressure(&self, queued_bytes: u64, paused_for: Duration) {
        let _ = self.ctx.event_sink.emit(Event::SandboxBackpressureApplied(
            SandboxBackpressureAppliedEvent {
                session_id: self.ctx.session_id,
                run_id: self.ctx.run_id,
                tool_use_id: self.ctx.tool_use_id,
                queued_bytes,
                paused_for_ms: paused_for.as_millis() as u64,
                at: Utc::now(),
            },
        ));
    }

    fn register_output_task(&self, task: JoinHandle<()>) {
        self.output_tasks.lock().push(task);
    }

    async fn wait_for_abort_output_tasks(&self) {
        if self.spec.output_policy.overflow != OutputOverflowPolicy::AbortExec {
            return;
        }
        let tasks = std::mem::take(&mut *self.output_tasks.lock());
        for task in tasks {
            let _ = task.await;
        }
    }

    fn redact_output(&self, bytes: Bytes) -> Bytes {
        if !self.spec.output_policy.redact_secrets {
            return bytes;
        }
        let input = String::from_utf8_lossy(&bytes);
        Bytes::from(self.ctx.redactor.redact(&input, &RedactRules::default()))
    }
}

#[async_trait]
impl ActivityHandle for LocalActivity {
    async fn wait(&self) -> Result<ExecOutcome, SandboxError> {
        if let Some(outcome) = self.outcome.lock().await.clone() {
            return Ok(outcome);
        }

        let mut child = self
            .child
            .lock()
            .await
            .take()
            .ok_or_else(|| SandboxError::Message("local process already claimed".to_owned()))?;

        let exit_status = self.wait_child(&mut child).await?;
        self.emit_heartbeat();
        self.wait_for_abort_output_tasks().await;
        let overflow = self.finalize_overflow().await?;
        let budget_exceeded = self.spec.output_policy.overflow == OutputOverflowPolicy::AbortExec
            && overflow.is_some();
        let outcome = ExecOutcome {
            exit_status,
            started_at: self.started_at,
            finished_at: Utc::now(),
            stdout_bytes_observed: self.stdout_bytes.load(Ordering::Relaxed),
            stderr_bytes_observed: self.stderr_bytes.load(Ordering::Relaxed),
            overflow: overflow.clone(),
        };

        self.ctx.event_sink.emit(Event::SandboxExecutionCompleted(
            SandboxExecutionCompletedEvent {
                session_id: self.ctx.session_id,
                run_id: self.ctx.run_id,
                tool_use_id: self.ctx.tool_use_id,
                backend_id: BACKEND_ID.to_owned(),
                fingerprint: self.fingerprint,
                exit_status: outcome.exit_status.clone(),
                stdout_bytes_observed: outcome.stdout_bytes_observed,
                stderr_bytes_observed: outcome.stderr_bytes_observed,
                duration_ms: self.started_instant.elapsed().as_millis() as u64,
                overflow: overflow.map(sandbox_overflow_summary),
                at: Utc::now(),
            },
        ))?;

        *self.outcome.lock().await = Some(outcome.clone());
        if budget_exceeded {
            return Err(SandboxError::OutputBudgetExceeded {
                limit: self.spec.output_policy.max_inline_bytes,
            });
        }
        Ok(outcome)
    }

    async fn kill(&self, signal: Signal, scope: KillScope) -> Result<(), SandboxError> {
        self.killed_signal.store(signal, Ordering::Relaxed);
        if scope == KillScope::ProcessGroup {
            if !cfg!(unix) {
                return Err(SandboxError::CapabilityMismatch {
                    capability: "kill_scope".to_owned(),
                    detail: "local sandbox cannot enforce process-group kill on this platform"
                        .to_owned(),
                });
            }
            if let Some(group) = self.process_group.lock().await.as_mut() {
                return group.signal(signal).await;
            }
        }
        if let Some(child) = self.child.lock().await.as_mut() {
            match scope {
                KillScope::Process => child.start_kill().map_err(sandbox_error)?,
                KillScope::ProcessGroup => kill_process_group(child, signal).await?,
                _ => {
                    return Err(SandboxError::Message(format!(
                        "unsupported kill scope for local sandbox: {scope:?}"
                    )));
                }
            }
        }
        Ok(())
    }

    fn kill_sync(&self, signal: Signal, scope: KillScope) -> Result<(), SandboxError> {
        self.killed_signal.store(signal, Ordering::Relaxed);
        match (scope, &self.process_group_target) {
            (KillScope::ProcessGroup, Some(target)) => target.signal_sync(signal),
            _ => Err(SandboxError::CapabilityMismatch {
                capability: "synchronous_kill".to_owned(),
                detail: format!("local activity cannot synchronously kill scope: {scope:?}"),
            }),
        }
    }

    fn touch(&self) {
        self.last_activity_ms
            .store(self.elapsed_since_start_ms(), Ordering::Relaxed);
    }

    fn last_activity(&self) -> Instant {
        let elapsed = Duration::from_millis(self.last_activity_ms.load(Ordering::Relaxed));
        self.started_instant + elapsed
    }
}

impl LocalActivity {
    fn start_periodic_heartbeat(activity: &Arc<Self>) {
        let weak = Arc::downgrade(activity);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                let Some(activity) = weak.upgrade() else {
                    break;
                };
                if activity.outcome.lock().await.is_some() {
                    break;
                }
                activity.emit_heartbeat();
            }
        });
    }

    fn emit_heartbeat(&self) {
        let last_io_ms = self.last_activity_ms.load(Ordering::Relaxed);
        let _ = self.ctx.event_sink.emit(Event::SandboxActivityHeartbeat(
            SandboxActivityHeartbeatEvent {
                session_id: self.ctx.session_id,
                run_id: self.ctx.run_id,
                tool_use_id: self.ctx.tool_use_id,
                backend_id: BACKEND_ID.to_owned(),
                since_last_io_ms: self.elapsed_since_start_ms().saturating_sub(last_io_ms),
                at: Utc::now(),
            },
        ));
    }
}

impl LocalActivity {
    async fn wait_child(&self, child: &mut Child) -> Result<SandboxExitStatus, SandboxError> {
        let timeout = timeout_future(self.spec.timeout, self.started_instant);
        let activity_timeout = activity_timeout_future(self.spec.activity_timeout, self);

        tokio::select! {
            result = child.wait() => {
                let exit_status = match result {
                    Ok(status) => {
                        if let Some(signal) = self.cached_signal() {
                            Ok(SandboxExitStatus::Signal(signal))
                        } else if let Some(code) = status.code() {
                            Ok(SandboxExitStatus::Code(code))
                        } else {
                            Ok(SandboxExitStatus::BackendError)
                        }
                    }
                    Err(error) => Err(sandbox_error(error)),
                }?;
                self.terminate_owned_process_group(9).await?;
                Ok(exit_status)
            }
            interrupt = timeout => {
                match interrupt {
                    WaitInterrupt::Timeout => {
                        self.signal_process_group(child, 9).await?;
                        let _ = child.wait().await;
                        self.reap_owned_process_group().await?;
                        Ok(SandboxExitStatus::Timeout)
                    }
                    WaitInterrupt::InactivityTimeout => unreachable!("timeout future cannot return inactivity"),
                }
            }
            interrupt = activity_timeout => {
                match interrupt {
                    WaitInterrupt::InactivityTimeout => {
                        self.signal_process_group(child, 9).await?;
                        let _ = child.wait().await;
                        self.reap_owned_process_group().await?;
                        self.ctx.event_sink.emit(Event::SandboxActivityTimeoutFired(
                            SandboxActivityTimeoutFiredEvent {
                                session_id: self.ctx.session_id,
                                run_id: self.ctx.run_id,
                                tool_use_id: self.ctx.tool_use_id,
                                backend_id: BACKEND_ID.to_owned(),
                                configured_timeout: self.spec.activity_timeout.unwrap_or_default(),
                                kill_scope: local_timeout_kill_scope(),
                                at: Utc::now(),
                            },
                        ))?;
                        Ok(SandboxExitStatus::InactivityTimeout)
                    }
                    WaitInterrupt::Timeout => unreachable!("activity timeout future cannot return timeout"),
                }
            }
        }
    }

    async fn signal_process_group(
        &self,
        child: &mut Child,
        signal: Signal,
    ) -> Result<(), SandboxError> {
        if let Some(group) = self.process_group.lock().await.as_mut() {
            group.signal(signal).await
        } else {
            kill_process_group(child, signal).await
        }
    }

    async fn terminate_owned_process_group(&self, signal: Signal) -> Result<(), SandboxError> {
        let mut group = self.process_group.lock().await.take();
        let Some(group) = group.as_mut() else {
            return Ok(());
        };
        group.signal(signal).await?;
        group.reap().await
    }

    async fn reap_owned_process_group(&self) -> Result<(), SandboxError> {
        let mut group = self.process_group.lock().await.take();
        match group.as_mut() {
            Some(group) => group.reap().await,
            None => Ok(()),
        }
    }
}

enum WaitInterrupt {
    Timeout,
    InactivityTimeout,
}

fn timeout_future(
    timeout: Option<Duration>,
    started: Instant,
) -> Pin<Box<dyn Future<Output = WaitInterrupt> + Send>> {
    Box::pin(async move {
        match timeout {
            Some(timeout) => {
                let deadline = started + timeout;
                tokio::time::sleep_until(deadline.into()).await;
                WaitInterrupt::Timeout
            }
            None => std::future::pending().await,
        }
    })
}

fn activity_timeout_future(
    timeout: Option<Duration>,
    activity: &LocalActivity,
) -> Pin<Box<dyn Future<Output = WaitInterrupt> + Send + '_>> {
    Box::pin(async move {
        match timeout {
            Some(timeout) => loop {
                let elapsed = activity.last_activity().elapsed();
                if elapsed >= timeout {
                    break WaitInterrupt::InactivityTimeout;
                }
                tokio::time::sleep(timeout.saturating_sub(elapsed)).await;
            },
            None => std::future::pending().await,
        }
    })
}

fn child_stream(
    reader: Option<impl tokio::io::AsyncRead + Send + 'static>,
    activity: Arc<LocalActivity>,
    stream: OutputStream,
) -> Option<futures::stream::BoxStream<'static, Bytes>> {
    reader.map(|reader| {
        let (tx, rx) = mpsc::channel(1);
        let task_activity = Arc::clone(&activity);
        let drop_when_full =
            task_activity.spec.output_policy.overflow == OutputOverflowPolicy::AbortExec;
        let task = tokio::spawn(async move {
            let reader = ReaderStream::new(reader);
            futures::pin_mut!(reader);
            if let OutputOverflowPolicy::SpillToBlob {
                head_bytes,
                tail_bytes,
            } = task_activity.spec.output_policy.overflow
            {
                let mut preview = SpillPreview::new(
                    task_activity.spec.output_policy.max_inline_bytes,
                    head_bytes,
                    tail_bytes,
                );
                while let Some(chunk) = reader.next().await {
                    let bytes = match chunk {
                        Ok(bytes) => bytes,
                        Err(_) => break,
                    };
                    task_activity
                        .process_spill_output(&mut preview, stream, bytes)
                        .await;
                }
                if let Some(bytes) = preview.finish() {
                    send_output(&tx, &task_activity, bytes, false).await;
                }
            } else {
                while let Some(chunk) = reader.next().await {
                    let bytes = match chunk {
                        Ok(bytes) => bytes,
                        Err(_) => break,
                    };
                    let Some(bytes) = task_activity.process_output(stream, bytes).await else {
                        continue;
                    };
                    send_output(&tx, &task_activity, bytes, drop_when_full).await;
                }
            }
        });
        activity.register_output_task(task);
        futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|bytes| (bytes, rx))
        })
        .boxed()
    })
}

async fn send_output(
    tx: &mpsc::Sender<Bytes>,
    activity: &LocalActivity,
    bytes: Bytes,
    drop_when_full: bool,
) {
    match tx.try_send(bytes) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(bytes)) => {
            if drop_when_full {
                activity.emit_backpressure(bytes.len() as u64, Duration::ZERO);
                return;
            }
            let started = Instant::now();
            if tx.send(bytes).await.is_ok() {
                activity.emit_backpressure(1, started.elapsed());
            }
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {}
    }
}

fn wrapped_command_for_local(
    spec: &ExecSpec,
    isolation: LocalIsolation,
    root: &Path,
    cwd: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<WrappedCommand, SandboxError> {
    let (program, args, cwd_marker_reader) = command_argv_with_cwd_marker(spec)?;
    let mut command = isolated_command(
        isolation,
        root,
        cwd,
        &spec.policy.network,
        &spec.policy.scope,
        &spec.workspace_access,
        environment,
        program,
        args,
    )?;
    if let Some(writer) = cwd_marker_reader.as_ref().map(|(_, writer)| writer) {
        #[cfg(unix)]
        command
            .fd_mappings(vec![FdMapping {
                parent_fd: writer.try_clone().map_err(sandbox_error)?.into(),
                child_fd: 3,
            }])
            .map_err(|error| SandboxError::Message(error.to_string()))?;
    }
    Ok(WrappedCommand::new(
        command,
        cwd_marker_reader.map(|(reader, _)| cwd_marker_stream(reader)),
    ))
}

#[cfg(unix)]
fn command_argv_with_cwd_marker(
    spec: &ExecSpec,
) -> Result<
    (
        String,
        Vec<String>,
        Option<(os_pipe::PipeReader, os_pipe::PipeWriter)>,
    ),
    SandboxError,
> {
    #[cfg(unix)]
    {
        if let Some(script) = shell_script(spec) {
            let (reader, writer) = os_pipe::pipe().map_err(sandbox_error)?;
            return Ok((
                spec.command.clone(),
                vec!["-c".to_owned(), wrap_shell_script_for_cwd(script)],
                Some((reader, writer)),
            ));
        }
    }

    Ok((spec.command.clone(), spec.args.clone(), None))
}

#[cfg(not(unix))]
fn command_argv_with_cwd_marker(
    spec: &ExecSpec,
) -> Result<(String, Vec<String>, Option<()>), SandboxError> {
    Ok((spec.command.clone(), spec.args.clone(), None))
}

fn isolated_command(
    isolation: LocalIsolation,
    root: &Path,
    cwd: &Path,
    network: &NetworkAccess,
    scope: &SandboxScope,
    access: &WorkspaceAccess,
    environment: &BTreeMap<String, String>,
    program: String,
    args: Vec<String>,
) -> Result<Command, SandboxError> {
    match isolation {
        LocalIsolation::None => {
            let mut command = Command::new(program);
            command.args(args);
            Ok(command)
        }
        LocalIsolation::Bubblewrap => bubblewrap_command(
            root,
            cwd,
            network,
            scope,
            access,
            environment,
            program,
            args,
        ),
        LocalIsolation::Seatbelt => {
            seatbelt_command(root, network, scope, access, environment, program, args)
        }
        LocalIsolation::JobObject => jobobject_command(program, args),
    }
}

#[cfg(target_os = "linux")]
fn bubblewrap_command(
    root: &Path,
    cwd: &Path,
    network: &NetworkAccess,
    scope: &SandboxScope,
    access: &WorkspaceAccess,
    environment: &BTreeMap<String, String>,
    program: String,
    args: Vec<String>,
) -> Result<Command, SandboxError> {
    let mut command = Command::new(resolve_host_binary_path("bwrap")?);
    command.args(bubblewrap_args_for_workspace_policy(
        root,
        cwd,
        network,
        scope,
        access,
        environment,
        &program,
        &args,
    )?);
    Ok(command)
}

#[cfg(not(target_os = "linux"))]
fn bubblewrap_command(
    _root: &Path,
    _cwd: &Path,
    _network: &NetworkAccess,
    _scope: &SandboxScope,
    _access: &WorkspaceAccess,
    _environment: &BTreeMap<String, String>,
    _program: String,
    _args: Vec<String>,
) -> Result<Command, SandboxError> {
    Err(SandboxError::CapabilityMismatch {
        capability: "local_isolation".to_owned(),
        detail: "Bubblewrap is only supported on Linux".to_owned(),
    })
}

#[cfg(target_os = "macos")]
fn seatbelt_command(
    root: &Path,
    network: &NetworkAccess,
    scope: &SandboxScope,
    access: &WorkspaceAccess,
    environment: &BTreeMap<String, String>,
    program: String,
    args: Vec<String>,
) -> Result<Command, SandboxError> {
    let profile = seatbelt_profile_for_workspace_policy(root, network, scope, access)?;
    let env = resolve_absolute_host_binary(&["/usr/bin/env", "/bin/env"])?;
    let mut command = Command::new(resolve_host_binary_path("sandbox-exec")?);
    command.arg("-p").arg(profile).arg(env).arg("-i");
    command.args(
        environment
            .iter()
            .map(|(key, value)| format!("{key}={value}")),
    );
    command.arg(program).args(args);
    Ok(command)
}

#[cfg(not(target_os = "macos"))]
fn seatbelt_command(
    _root: &Path,
    _network: &NetworkAccess,
    _scope: &SandboxScope,
    _access: &WorkspaceAccess,
    _environment: &BTreeMap<String, String>,
    _program: String,
    _args: Vec<String>,
) -> Result<Command, SandboxError> {
    Err(SandboxError::CapabilityMismatch {
        capability: "local_isolation".to_owned(),
        detail: "Seatbelt is only supported on macOS".to_owned(),
    })
}

#[cfg(windows)]
fn jobobject_command(program: String, args: Vec<String>) -> Result<Command, SandboxError> {
    let mut command = Command::new(program);
    command.args(args);
    Ok(command)
}

#[cfg(not(windows))]
fn jobobject_command(_program: String, _args: Vec<String>) -> Result<Command, SandboxError> {
    Err(SandboxError::CapabilityMismatch {
        capability: "local_isolation".to_owned(),
        detail: "JobObject is only supported on Windows".to_owned(),
    })
}

fn local_kill_scopes(isolation: LocalIsolation) -> Vec<KillScope> {
    if local_process_group_supported(isolation) {
        vec![KillScope::Process, KillScope::ProcessGroup]
    } else {
        vec![KillScope::Process]
    }
}

fn local_synchronous_kill_scopes(isolation: LocalIsolation) -> Vec<KillScope> {
    if local_process_group_supported(isolation) {
        vec![KillScope::ProcessGroup]
    } else {
        Vec::new()
    }
}

fn local_process_group_supported(isolation: LocalIsolation) -> bool {
    cfg!(unix)
        && !matches!(isolation, LocalIsolation::Seatbelt)
        && ProcessGroupTools::resolve().is_ok()
}

fn local_timeout_kill_scope() -> KillScope {
    if cfg!(unix) {
        KillScope::ProcessGroup
    } else {
        KillScope::Process
    }
}

struct ProcessGroupKeeper {
    #[cfg(unix)]
    id: u32,
    #[cfg(unix)]
    child: Child,
    #[cfg(unix)]
    kill: PathBuf,
    #[cfg(unix)]
    terminal_kill_sent: Arc<SyncMutex<bool>>,
}

#[cfg(unix)]
#[derive(Clone)]
struct ProcessGroupTools {
    shell: PathBuf,
    sleep: PathBuf,
    kill: PathBuf,
}

#[cfg(unix)]
impl ProcessGroupTools {
    fn resolve() -> Result<Self, SandboxError> {
        Ok(Self {
            shell: resolve_absolute_host_binary(&["/bin/sh"])?,
            sleep: resolve_host_binary_path("sleep")?,
            kill: resolve_host_binary_path("kill")?,
        })
    }
}

#[cfg(unix)]
#[derive(Clone)]
struct ProcessGroupTarget {
    id: u32,
    kill: PathBuf,
    terminal_kill_sent: Arc<SyncMutex<bool>>,
}

#[cfg(not(unix))]
#[derive(Clone)]
struct ProcessGroupTarget;

#[cfg(unix)]
impl ProcessGroupTarget {
    fn signal_sync(&self, signal: Signal) -> Result<(), SandboxError> {
        signal_process_group_sync(&self.kill, self.id, signal, &self.terminal_kill_sent)
    }
}

impl ProcessGroupKeeper {
    fn for_spec(spec: &ExecSpec) -> Result<Option<Self>, SandboxError> {
        if spec.required_kill_scope != Some(KillScope::ProcessGroup) {
            return Ok(None);
        }
        #[cfg(unix)]
        {
            let tools = ProcessGroupTools::resolve()?;
            let mut command = Command::new(&tools.shell);
            command
                .args([
                    "-c",
                    "trap '' HUP INT QUIT TERM; exec \"$1\" 2147483647",
                    "jyowo-process-group-keeper",
                ])
                .arg(&tools.sleep)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .process_group(0);
            let child = command.spawn().map_err(sandbox_error)?;
            let id = child.id().ok_or_else(|| {
                SandboxError::Message("local process group keeper has no pid".to_owned())
            })?;
            return Ok(Some(Self {
                id,
                child,
                kill: tools.kill,
                terminal_kill_sent: Arc::new(SyncMutex::new(false)),
            }));
        }
        #[cfg(not(unix))]
        {
            Err(SandboxError::CapabilityMismatch {
                capability: "kill_scope".to_owned(),
                detail: "local sandbox cannot enforce process-group kill on this platform"
                    .to_owned(),
            })
        }
    }

    async fn signal(&mut self, signal: Signal) -> Result<(), SandboxError> {
        #[cfg(unix)]
        {
            signal_process_group(&self.kill, self.id, signal, &self.terminal_kill_sent).await
        }
        #[cfg(not(unix))]
        {
            let _ = signal;
            Err(SandboxError::CapabilityMismatch {
                capability: "kill_scope".to_owned(),
                detail: "local sandbox cannot enforce process-group kill on this platform"
                    .to_owned(),
            })
        }
    }

    async fn reap(&mut self) -> Result<(), SandboxError> {
        #[cfg(unix)]
        {
            self.child.wait().await.map(|_| ()).map_err(sandbox_error)
        }
        #[cfg(not(unix))]
        {
            Ok(())
        }
    }

    fn target(&self) -> ProcessGroupTarget {
        #[cfg(unix)]
        {
            ProcessGroupTarget {
                id: self.id,
                kill: self.kill.clone(),
                terminal_kill_sent: Arc::clone(&self.terminal_kill_sent),
            }
        }
        #[cfg(not(unix))]
        {
            ProcessGroupTarget
        }
    }
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command, group: Option<&ProcessGroupKeeper>) {
    command.process_group(group.map_or(0, |group| group.id as i32));
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command, _group: Option<&ProcessGroupKeeper>) {}

#[cfg(unix)]
async fn signal_process_group(
    kill: &Path,
    id: u32,
    signal: Signal,
    terminal_kill_sent: &SyncMutex<bool>,
) -> Result<(), SandboxError> {
    signal_process_group_sync(kill, id, signal, terminal_kill_sent)
}

#[cfg(unix)]
fn signal_process_group_sync(
    kill: &Path,
    id: u32,
    signal: Signal,
    terminal_kill_sent: &SyncMutex<bool>,
) -> Result<(), SandboxError> {
    let mut terminal_kill_sent = terminal_kill_sent.lock();
    if signal == 9 && *terminal_kill_sent {
        return Ok(());
    }
    let status = std::process::Command::new(kill)
        .arg(format!("-{signal}"))
        .arg(format!("-{id}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(sandbox_error)?;
    if status.success() {
        if signal == 9 {
            *terminal_kill_sent = true;
        }
        Ok(())
    } else {
        Err(SandboxError::Message(format!(
            "failed to signal owned process group {id}"
        )))
    }
}

async fn kill_process_group(child: &mut Child, signal: Signal) -> Result<(), SandboxError> {
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            let kill = resolve_host_binary_path("kill")?;
            if signal_process_group_sync(&kill, pid, signal, &SyncMutex::new(false)).is_ok() {
                return Ok(());
            }
        }
    }
    child.start_kill().map_err(sandbox_error)
}

#[cfg(unix)]
fn shell_script(spec: &ExecSpec) -> Option<&str> {
    let command = Path::new(&spec.command).file_name()?.to_str()?;
    if !matches!(command, "sh" | "bash" | "zsh") {
        return None;
    }
    if spec.args.first().map(String::as_str) != Some("-c") {
        return None;
    }
    spec.args.get(1).map(String::as_str)
}

#[cfg(unix)]
fn wrap_shell_script_for_cwd(script: &str) -> String {
    format!("{script}\n__jyowo_status=$?\nprintf '1\\t%s\\n' \"$PWD\" >&3\nexit $__jyowo_status")
}

#[cfg(unix)]
fn cwd_marker_stream(
    reader: os_pipe::PipeReader,
) -> futures::stream::BoxStream<'static, CwdMarkerLine> {
    futures::stream::once(async move {
        let line = tokio::task::spawn_blocking(move || {
            let mut reader = reader;
            let mut line = String::new();
            reader.read_to_string(&mut line).map(|_| line)
        })
        .await
        .ok()?
        .ok()?;
        parse_cwd_marker_line(line.lines().next().unwrap_or_default())
    })
    .filter_map(|line| async move { line })
    .boxed()
}

#[cfg(unix)]
fn parse_cwd_marker_line(line: &str) -> Option<CwdMarkerLine> {
    let (sequence, cwd) = line.trim_end().split_once('\t')?;
    Some(CwdMarkerLine {
        sequence: sequence.parse().ok()?,
        cwd: PathBuf::from(cwd),
        at: Utc::now(),
    })
}

fn prefix_within_limit(bytes: Bytes, previous: u64, limit: u64) -> Option<Bytes> {
    if previous >= limit {
        return None;
    }
    let allowed = (limit - previous).min(bytes.len() as u64) as usize;
    Some(bytes.slice(..allowed))
}

fn effective_spill_preview_limits(limit: u64, head_bytes: u32, tail_bytes: u32) -> (u32, u32) {
    let head = u64::from(head_bytes).min(limit);
    let tail = u64::from(tail_bytes).min(limit.saturating_sub(head));
    (head as u32, tail as u32)
}

fn blob_ref_for_file(path: &Path) -> Result<BlobRef, SandboxError> {
    let bytes = std::fs::read(path).map_err(sandbox_error)?;
    let hash = blake3::hash(&bytes);
    Ok(BlobRef {
        id: harness_contracts::BlobId::new(),
        size: bytes.len() as u64,
        content_hash: *hash.as_bytes(),
        content_type: Some("application/octet-stream".to_owned()),
    })
}

async fn blob_ref_for_store(
    store: Arc<dyn harness_contracts::BlobStore>,
    tenant: harness_contracts::TenantId,
    session_id: harness_contracts::SessionId,
    bytes: Bytes,
) -> Result<BlobRef, SandboxError> {
    let hash = blake3::hash(&bytes);
    let meta = BlobMeta {
        content_type: Some("application/octet-stream".to_owned()),
        size: bytes.len() as u64,
        content_hash: *hash.as_bytes(),
        created_at: Utc::now(),
        retention: BlobRetention::SessionScoped(session_id),
    };
    store
        .put(tenant, bytes, meta)
        .await
        .map_err(|error| SandboxError::Message(error.to_string()))
}

fn sandbox_output_stream(stream: OutputStream) -> SandboxOutputStream {
    match stream {
        OutputStream::Stdout => SandboxOutputStream::Stdout,
        OutputStream::Stderr => SandboxOutputStream::Stderr,
        OutputStream::Combined => SandboxOutputStream::Combined,
    }
}

fn sandbox_overflow_summary(overflow: OutputOverflow) -> SandboxOverflowSummary {
    SandboxOverflowSummary {
        stream: sandbox_output_stream(overflow.stream),
        original_bytes: overflow.original_bytes,
        effective_limit: overflow.effective_limit,
        blob_ref: overflow.blob_ref,
    }
}

fn filtered_env<'a>(
    allowed: &'a BTreeSet<String>,
    spec: &'a ExecSpec,
) -> impl Iterator<Item = (&'a String, &'a String)> + 'a {
    spec.env.iter().filter(|(key, _)| {
        allowed.contains(key.as_str()) || spec.authorized_env_keys.contains(key.as_str())
    })
}

fn create_local_snapshot(
    root: &Path,
    spec: &SnapshotSpec,
) -> Result<SessionSnapshotFile, SandboxError> {
    match spec.kind {
        SessionSnapshotKind::FilesystemImage => create_filesystem_snapshot(root, spec),
        SessionSnapshotKind::ShellState => create_shell_state_snapshot(root, spec),
        _ => Err(SandboxError::SnapshotUnsupported {
            kind: format!("{:?}", spec.kind),
        }),
    }
}

fn restore_local_snapshot(root: &Path, snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
    match snapshot.kind {
        SessionSnapshotKind::FilesystemImage => restore_filesystem_snapshot(root, snapshot),
        SessionSnapshotKind::ShellState => restore_shell_state_snapshot(root, snapshot),
        _ => Err(SandboxError::SnapshotUnsupported {
            kind: format!("{:?}", snapshot.kind),
        }),
    }
}

fn create_filesystem_snapshot(
    root: &Path,
    spec: &SnapshotSpec,
) -> Result<SessionSnapshotFile, SandboxError> {
    if spec.kind != SessionSnapshotKind::FilesystemImage {
        return Err(SandboxError::SnapshotUnsupported {
            kind: format!("{:?}", spec.kind),
        });
    }

    let target_path = spec.target_path.clone().unwrap_or_else(|| {
        root.join(".jyowo")
            .join("snapshots")
            .join(format!("{}.tar", spec.session_id))
    });
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent).map_err(sandbox_error)?;
    }

    let file = std::fs::File::create(&target_path).map_err(sandbox_error)?;
    let mut builder = tar::Builder::new(file);
    append_snapshot_entries(&mut builder, root, root, &target_path)?;
    builder.finish().map_err(sandbox_error)?;

    let metadata = snapshot_metadata(&target_path)?;
    Ok(SessionSnapshotFile {
        session_id: spec.session_id,
        kind: spec.kind,
        path: target_path,
        metadata,
    })
}

fn shell_state_path(root: &Path) -> PathBuf {
    root.join(".jyowo-shell-state")
}

fn create_shell_state_snapshot(
    root: &Path,
    spec: &SnapshotSpec,
) -> Result<SessionSnapshotFile, SandboxError> {
    let target_path = spec.target_path.clone().unwrap_or_else(|| {
        root.join(".jyowo")
            .join("snapshots")
            .join(format!("{}-shell-state", spec.session_id))
    });
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent).map_err(sandbox_error)?;
    }
    let state = shell_state_path(root);
    if state.exists() {
        std::fs::copy(&state, &target_path).map_err(sandbox_error)?;
    } else {
        std::fs::write(&target_path, b"").map_err(sandbox_error)?;
    }
    let metadata = snapshot_metadata(&target_path)?;
    Ok(SessionSnapshotFile {
        session_id: spec.session_id,
        kind: spec.kind,
        path: target_path,
        metadata,
    })
}

fn restore_shell_state_snapshot(
    root: &Path,
    snapshot: &SessionSnapshotFile,
) -> Result<(), SandboxError> {
    let target = shell_state_path(root);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(sandbox_error)?;
    }
    std::fs::copy(&snapshot.path, target).map_err(sandbox_error)?;
    Ok(())
}

fn append_snapshot_entries(
    builder: &mut tar::Builder<std::fs::File>,
    root: &Path,
    current: &Path,
    target_path: &Path,
) -> Result<(), SandboxError> {
    for entry in std::fs::read_dir(current).map_err(sandbox_error)? {
        let entry = entry.map_err(sandbox_error)?;
        let path = entry.path();
        if path == target_path || path.starts_with(root.join(".jyowo").join("snapshots")) {
            continue;
        }
        let relative = path.strip_prefix(root).map_err(|error| {
            SandboxError::Message(format!("snapshot path escaped root: {error}"))
        })?;
        if path.is_dir() {
            builder.append_dir(relative, &path).map_err(sandbox_error)?;
            append_snapshot_entries(builder, root, &path, target_path)?;
        } else if path.is_file() {
            builder
                .append_path_with_name(&path, relative)
                .map_err(sandbox_error)?;
        }
    }
    Ok(())
}

fn restore_filesystem_snapshot(
    root: &Path,
    snapshot: &SessionSnapshotFile,
) -> Result<(), SandboxError> {
    if snapshot.kind != SessionSnapshotKind::FilesystemImage {
        return Err(SandboxError::SnapshotUnsupported {
            kind: format!("{:?}", snapshot.kind),
        });
    }

    validate_snapshot_archive(&snapshot.path)?;
    clear_root_for_restore(root, &snapshot.path)?;

    let file = std::fs::File::open(&snapshot.path).map_err(sandbox_error)?;
    let mut archive = tar::Archive::new(file);
    for entry in archive.entries().map_err(sandbox_error)? {
        let mut entry = entry.map_err(sandbox_error)?;
        let path = entry.path().map_err(sandbox_error)?;
        ensure_relative_archive_path(&path)?;
        entry.unpack_in(root).map_err(sandbox_error)?;
    }
    Ok(())
}

fn validate_snapshot_archive(path: &Path) -> Result<(), SandboxError> {
    let file = std::fs::File::open(path).map_err(sandbox_error)?;
    let mut archive = tar::Archive::new(file);
    for entry in archive.entries().map_err(sandbox_error)? {
        let entry = entry.map_err(sandbox_error)?;
        let path = entry.path().map_err(sandbox_error)?;
        ensure_relative_archive_path(&path)?;
    }
    Ok(())
}

fn ensure_relative_archive_path(path: &Path) -> Result<(), SandboxError> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(SandboxError::Message(format!(
            "snapshot path escapes sandbox root: {}",
            path.display()
        )));
    }
    Ok(())
}

fn clear_root_for_restore(root: &Path, snapshot_path: &Path) -> Result<(), SandboxError> {
    std::fs::create_dir_all(root).map_err(sandbox_error)?;
    for entry in std::fs::read_dir(root).map_err(sandbox_error)? {
        let entry = entry.map_err(sandbox_error)?;
        let path = entry.path();
        if path == snapshot_path || snapshot_path.starts_with(&path) {
            continue;
        }
        if path.is_dir() {
            std::fs::remove_dir_all(path).map_err(sandbox_error)?;
        } else {
            std::fs::remove_file(path).map_err(sandbox_error)?;
        }
    }
    Ok(())
}

fn snapshot_metadata(path: &Path) -> Result<SnapshotMetadata, SandboxError> {
    let mut file = std::fs::File::open(path).map_err(sandbox_error)?;
    let mut hasher = blake3::Hasher::new();
    let mut size = 0;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer).map_err(sandbox_error)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        hasher.update(&buffer[..read]);
    }
    Ok(SnapshotMetadata {
        size_bytes: size,
        content_hash: *hasher.finalize().as_bytes(),
        created_at: Utc::now(),
    })
}

fn stdio(spec: &StdioSpec) -> Result<Stdio, SandboxError> {
    match spec {
        StdioSpec::Null => Ok(Stdio::null()),
        StdioSpec::Piped => Ok(Stdio::piped()),
        StdioSpec::Inherit => Ok(Stdio::inherit()),
        StdioSpec::File(path) => {
            let file = std::fs::File::create(path).map_err(sandbox_error)?;
            Ok(Stdio::from(file))
        }
    }
}

fn resolve_cwd(
    root: &Path,
    cwd: Option<&Path>,
    scope: &SandboxScope,
) -> Result<PathBuf, SandboxError> {
    let relative = cwd.map_or_else(PathBuf::new, lexical_normalize_path);
    let resolved = if relative.is_absolute() {
        relative
    } else {
        if relative.starts_with("..") {
            return Err(SandboxError::HostPathDenied {
                path: relative.display().to_string(),
            });
        }
        lexical_normalize_path(&root.join(relative))
    };
    if !path_allowed_by_scope(root, &resolved, scope) {
        return Err(SandboxError::HostPathDenied {
            path: resolved.display().to_string(),
        });
    }
    Ok(resolved)
}

fn path_allowed_by_scope(root: &Path, path: &Path, scope: &SandboxScope) -> bool {
    let path = lexical_normalize_path(path);
    let root = lexical_normalize_path(root);
    match scope {
        SandboxScope::WorkspaceOnly => path.starts_with(root),
        SandboxScope::WorkspacePlus(extra) => {
            path.starts_with(&root)
                || extra.iter().any(|allowed| {
                    let allowed = normalize_policy_path(&root, allowed);
                    path.starts_with(allowed)
                })
        }
        SandboxScope::Unrestricted => true,
        _ => false,
    }
}

fn validate_workspace_access(
    isolation: LocalIsolation,
    root: &Path,
    scope: &SandboxScope,
    access: &WorkspaceAccess,
) -> Result<(), SandboxError> {
    if matches!(
        isolation,
        LocalIsolation::Bubblewrap | LocalIsolation::Seatbelt
    ) {
        workspace_write_paths_for_os_isolation(root, scope, access)?;
        return Ok(());
    }

    let writable_paths = match access {
        WorkspaceAccess::None => return Ok(()),
        WorkspaceAccess::ReadOnly => {
            return Err(unsupported_workspace_access(
                "local read-only workspace access is not enforceable",
            ));
        }
        WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths,
        } if allowed_writable_subpaths.is_empty() => vec![PathBuf::new()],
        WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths,
        } => {
            for writable in allowed_writable_subpaths {
                let path = normalize_policy_path(root, writable);
                if !path_allowed_by_scope(root, &path, scope) {
                    return Err(SandboxError::HostPathDenied {
                        path: path.display().to_string(),
                    });
                }
            }
            return Err(unsupported_workspace_access(
                "local writable subpath access is not enforceable",
            ));
        }
        _ => {
            return Err(SandboxError::Message(
                "unsupported workspace access policy".to_owned(),
            ));
        }
    };

    for writable in writable_paths {
        let path = normalize_policy_path(root, &writable);
        if !path_allowed_by_scope(root, &path, scope) {
            return Err(SandboxError::HostPathDenied {
                path: path.display().to_string(),
            });
        }
    }
    Ok(())
}

fn unsupported_workspace_access(detail: &str) -> SandboxError {
    SandboxError::CapabilityMismatch {
        capability: "workspace_access".to_owned(),
        detail: detail.to_owned(),
    }
}

fn validate_denied_paths(
    root: &Path,
    cwd: &Path,
    spec: &ExecSpec,
    denied_paths: &[PathBuf],
) -> Result<(), SandboxError> {
    if denied_paths.is_empty() {
        return Ok(());
    }

    let mut candidates = Vec::from([cwd.to_path_buf()]);
    if Path::new(&spec.command).is_absolute() {
        candidates.push(PathBuf::from(&spec.command));
    }
    for stdio in [&spec.stdin, &spec.stdout, &spec.stderr] {
        if let StdioSpec::File(path) = stdio {
            candidates.push(if path.is_absolute() {
                lexical_normalize_path(path)
            } else {
                lexical_normalize_path(&cwd.join(path))
            });
        }
    }

    for candidate in candidates {
        let candidate = lexical_normalize_path(&candidate);
        for denied in denied_paths {
            let denied = normalize_policy_path(root, denied);
            if candidate == denied || candidate.starts_with(&denied) {
                return Err(SandboxError::HostPathDenied {
                    path: candidate.display().to_string(),
                });
            }
        }
    }
    Ok(())
}

fn validate_network_policy(
    isolation: LocalIsolation,
    network: &NetworkAccess,
) -> Result<(), SandboxError> {
    match network {
        NetworkAccess::Unrestricted => Ok(()),
        NetworkAccess::None if isolation.is_os_level() => Ok(()),
        NetworkAccess::None => Err(SandboxError::CapabilityMismatch {
            capability: "network".to_owned(),
            detail: format!("local network policy unsupported without OS isolation: {network:?}"),
        }),
        NetworkAccess::LoopbackOnly | NetworkAccess::AllowList(_) => {
            Err(SandboxError::CapabilityMismatch {
                capability: "network".to_owned(),
                detail: format!("local network policy is not implemented: {network:?}"),
            })
        }
        _ => Err(SandboxError::CapabilityMismatch {
            capability: "network".to_owned(),
            detail: "unsupported local network policy".to_owned(),
        }),
    }
}

fn validate_isolation(isolation: LocalIsolation) -> Result<(), SandboxError> {
    match isolation {
        LocalIsolation::None => Ok(()),
        LocalIsolation::Bubblewrap if cfg!(target_os = "linux") => {
            validate_host_binary("bwrap", isolation)
        }
        LocalIsolation::Seatbelt if cfg!(target_os = "macos") => {
            validate_host_binary("sandbox-exec", isolation)
        }
        LocalIsolation::JobObject if cfg!(windows) => Ok(()),
        _ => Err(SandboxError::CapabilityMismatch {
            capability: "local_isolation".to_owned(),
            detail: format!("{isolation:?} is not supported on this host platform"),
        }),
    }
}

fn validate_host_binary(binary: &str, isolation: LocalIsolation) -> Result<(), SandboxError> {
    resolve_host_binary_path(binary)
        .map(|_| ())
        .map_err(|_| SandboxError::Unavailable {
            backend: BACKEND_ID.to_owned(),
            detail: format!("{isolation:?} requires host binary `{binary}`"),
        })
}

fn resolve_host_binary_path(binary: &str) -> Result<PathBuf, SandboxError> {
    let path = std::env::var_os("PATH").ok_or_else(|| SandboxError::Unavailable {
        backend: BACKEND_ID.to_owned(),
        detail: format!("host PATH is unavailable while resolving `{binary}`"),
    })?;
    resolve_host_binary_in_path(binary, &path).ok_or_else(|| SandboxError::Unavailable {
        backend: BACKEND_ID.to_owned(),
        detail: format!("host binary `{binary}` is unavailable on PATH"),
    })
}

fn resolve_host_binary_in_path(binary: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|directory| directory.join(binary))
        .find(|candidate| host_path_is_executable(candidate))
}

fn resolve_absolute_host_binary(candidates: &[&str]) -> Result<PathBuf, SandboxError> {
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|candidate| candidate.is_absolute() && host_path_is_executable(candidate))
        .ok_or_else(|| SandboxError::Unavailable {
            backend: BACKEND_ID.to_owned(),
            detail: format!(
                "required host binary is unavailable at: {}",
                candidates.join(", ")
            ),
        })
}

fn host_path_is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn validate_resource_policy(
    _isolation: LocalIsolation,
    limits: &ResourceLimits,
) -> Result<(), SandboxError> {
    if limits.max_memory_bytes.is_some()
        || limits.max_cpu_cores.is_some()
        || limits.max_pids.is_some()
        || limits.max_open_files.is_some()
    {
        return Err(SandboxError::CapabilityMismatch {
            capability: "resource_limits".to_owned(),
            detail: "local resource limits are not implemented beyond wall-clock".to_owned(),
        });
    }
    Ok(())
}

fn apply_supported_resource_limits(spec: &mut ExecSpec, defaults: &ResourceLimits) {
    apply_wall_clock_resource_limit(spec, defaults);
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn bubblewrap_args_for_workspace_policy(
    root: &Path,
    cwd: &Path,
    network: &NetworkAccess,
    scope: &SandboxScope,
    access: &WorkspaceAccess,
    environment: &BTreeMap<String, String>,
    program: &str,
    program_args: &[String],
) -> Result<Vec<String>, SandboxError> {
    let write_paths = workspace_write_paths_for_os_isolation(root, scope, access)?;
    let mut args = vec![
        "--die-with-parent".to_owned(),
        "--unshare-user".to_owned(),
        "--unshare-ipc".to_owned(),
        "--unshare-pid".to_owned(),
        "--unshare-uts".to_owned(),
    ];
    if matches!(network, NetworkAccess::None) {
        args.push("--unshare-net".to_owned());
    }
    push_bubblewrap_environment(&mut args, environment);
    args.extend([
        "--proc".to_owned(),
        "/proc".to_owned(),
        "--dev".to_owned(),
        "/dev".to_owned(),
        "--ro-bind".to_owned(),
        "/".to_owned(),
        "/".to_owned(),
    ]);
    let root = lexical_normalize_path(root);
    if write_paths.len() == 1 && write_paths[0] == root {
        push_mount(&mut args, "--bind", &root);
    } else {
        push_mount(&mut args, "--ro-bind", &root);
        for path in write_paths {
            push_mount(&mut args, "--bind", &path);
        }
    }
    args.extend([
        "--chdir".to_owned(),
        cwd.display().to_string(),
        "--".to_owned(),
        program.to_owned(),
    ]);
    args.extend(program_args.iter().cloned());
    Ok(args)
}

fn push_bubblewrap_environment(args: &mut Vec<String>, environment: &BTreeMap<String, String>) {
    args.push("--clearenv".to_owned());
    for (key, value) in environment {
        args.extend(["--setenv".to_owned(), key.clone(), value.clone()]);
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn push_mount(args: &mut Vec<String>, flag: &str, path: &Path) {
    let path = path.display().to_string();
    args.push(flag.to_owned());
    args.push(path.clone());
    args.push(path);
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn seatbelt_profile_for_workspace_policy(
    root: &Path,
    network: &NetworkAccess,
    scope: &SandboxScope,
    access: &WorkspaceAccess,
) -> Result<String, SandboxError> {
    let write_paths = workspace_write_paths_for_os_isolation(root, scope, access)?;
    let network_rule = if matches!(network, NetworkAccess::Unrestricted) {
        "(allow network*)\n"
    } else {
        ""
    };
    let mut profile = format!(
        "(version 1)\n\
         (deny default)\n\
         (allow process*)\n\
         (allow file-read*)\n\
         (allow file-write* (literal \"/dev/null\"))\n\
         {network_rule}"
    );
    for path in write_paths {
        let _ = writeln!(
            profile,
            "(allow file-write* (subpath \"{}\"))",
            seatbelt_escape_path(&path)
        );
    }
    Ok(profile)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn seatbelt_escape_path(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn workspace_write_paths_for_os_isolation(
    root: &Path,
    scope: &SandboxScope,
    access: &WorkspaceAccess,
) -> Result<Vec<PathBuf>, SandboxError> {
    let root = lexical_normalize_path(root);
    let write_paths = match access {
        WorkspaceAccess::None => vec![root.clone()],
        WorkspaceAccess::ReadOnly => Vec::new(),
        WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths,
        } if allowed_writable_subpaths.is_empty() => vec![root.clone()],
        WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths,
        } => {
            let mut paths = Vec::with_capacity(allowed_writable_subpaths.len());
            for writable in allowed_writable_subpaths {
                let path = normalize_policy_path(&root, writable);
                if !path_allowed_by_scope(&root, &path, scope) {
                    return Err(SandboxError::HostPathDenied {
                        path: path.display().to_string(),
                    });
                }
                if !path.exists() {
                    return Err(SandboxError::HostPathDenied {
                        path: path.display().to_string(),
                    });
                }
                paths.push(path);
            }
            paths
        }
        _ => {
            return Err(SandboxError::Message(
                "unsupported workspace access policy".to_owned(),
            ));
        }
    };
    Ok(write_paths)
}

fn normalize_policy_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        lexical_normalize_path(path)
    } else {
        lexical_normalize_path(&root.join(path))
    }
}

fn sandbox_error(error: std::io::Error) -> SandboxError {
    SandboxError::Message(error.to_string())
}

fn network_policy_support_for_isolation(isolation: LocalIsolation) -> NetworkPolicySupport {
    match isolation {
        LocalIsolation::None => NetworkPolicySupport {
            none: false,
            loopback_only: false,
            allowlist: false,
            unrestricted: true,
        },
        LocalIsolation::Bubblewrap | LocalIsolation::Seatbelt => NetworkPolicySupport {
            none: true,
            loopback_only: false,
            allowlist: false,
            unrestricted: true,
        },
        LocalIsolation::JobObject => NetworkPolicySupport {
            none: false,
            loopback_only: false,
            allowlist: false,
            unrestricted: true,
        },
    }
}

fn workspace_policy_support_for_isolation(isolation: LocalIsolation) -> WorkspacePolicySupport {
    match isolation {
        LocalIsolation::Bubblewrap | LocalIsolation::Seatbelt => WorkspacePolicySupport {
            read_write_all: true,
            read_only: true,
            writable_subpaths: true,
        },
        LocalIsolation::None | LocalIsolation::JobObject => WorkspacePolicySupport {
            read_write_all: true,
            ..WorkspacePolicySupport::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("jyowo-local-policy-{}-{name}", std::process::id()));
        std::fs::create_dir_all(root.join("tmp")).expect("tmp should exist");
        std::fs::create_dir_all(root.join("cache")).expect("cache should exist");
        root
    }

    fn scope() -> SandboxScope {
        SandboxScope::WorkspaceOnly
    }

    #[cfg(unix)]
    #[test]
    fn process_group_helpers_are_resolved_from_path_and_must_be_executable() {
        use std::os::unix::fs::PermissionsExt;

        let root = root("process-group-tools");
        let executable = root.join("sleep");
        let non_executable = root.join("kill");
        std::fs::write(&executable, "#!/bin/sh\n").expect("helper must be written");
        std::fs::write(&non_executable, "#!/bin/sh\n").expect("helper must be written");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .expect("helper must be executable");
        let path = std::env::join_paths([&root]).expect("test PATH must be valid");

        assert_eq!(
            resolve_host_binary_in_path("sleep", &path),
            Some(executable)
        );
        assert_eq!(resolve_host_binary_in_path("kill", &path), None);
    }

    #[test]
    fn bubblewrap_receives_request_environment_as_inner_arguments() {
        let mut args = Vec::new();
        push_bubblewrap_environment(
            &mut args,
            &BTreeMap::from([
                ("PATH".to_owned(), "/untrusted/bin".to_owned()),
                ("TOKEN".to_owned(), "secret".to_owned()),
            ]),
        );

        assert_eq!(
            args,
            [
                "--clearenv",
                "--setenv",
                "PATH",
                "/untrusted/bin",
                "--setenv",
                "TOKEN",
                "secret",
            ]
        );
    }

    #[test]
    fn bubblewrap_process_tree_containment_uses_a_private_pid_namespace() {
        let root = root("bwrap-process-tree");
        let args = bubblewrap_args_for_workspace_policy(
            &root,
            &root,
            &NetworkAccess::None,
            &scope(),
            &WorkspaceAccess::ReadOnly,
            &BTreeMap::new(),
            "/bin/sh",
            &["-c".to_owned(), "setsid sleep 30 & wait".to_owned()],
        )
        .expect("bubblewrap process containment should be expressible");

        assert!(args.iter().any(|arg| arg == "--die-with-parent"));
        assert!(args.iter().any(|arg| arg == "--unshare-pid"));
        assert!(args.iter().any(|arg| arg == "--unshare-net"));
    }

    #[test]
    fn bubblewrap_read_only_workspace_uses_ro_bind_for_root() {
        let root = root("bwrap-read-only");
        let args = bubblewrap_args_for_workspace_policy(
            &root,
            &root,
            &NetworkAccess::Unrestricted,
            &scope(),
            &WorkspaceAccess::ReadOnly,
            &BTreeMap::new(),
            "tool",
            &["--flag".to_owned()],
        )
        .expect("read-only workspace policy should be expressible");

        let text = args.join(" ");
        let root = root.display();
        assert!(text.contains(&format!("--ro-bind {root} {root}")));
        assert!(!text.contains(&format!("--bind {root} {root}")));
    }

    #[test]
    fn bubblewrap_scoped_writes_bind_only_allowed_subpaths() {
        let root = root("bwrap-scoped");
        let access = WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: vec![PathBuf::from("tmp"), PathBuf::from("cache")],
        };
        let args = bubblewrap_args_for_workspace_policy(
            &root,
            &root,
            &NetworkAccess::Unrestricted,
            &scope(),
            &access,
            &BTreeMap::new(),
            "tool",
            &[],
        )
        .expect("scoped workspace policy should be expressible");
        let text = args.join(" ");
        let root_display = root.display();
        let tmp = root.join("tmp");
        let cache = root.join("cache");
        let tmp = tmp.display();
        let cache = cache.display();

        assert!(text.contains(&format!("--ro-bind {root_display} {root_display}")));
        assert!(text.contains(&format!("--bind {tmp} {tmp}")));
        assert!(text.contains(&format!("--bind {cache} {cache}")));
        assert!(!text.contains(&format!("--bind {root_display} {root_display}")));
    }

    #[test]
    fn seatbelt_read_only_profile_omits_workspace_write_rule() {
        let root = root("seatbelt-read-only");
        let profile = seatbelt_profile_for_workspace_policy(
            &root,
            &NetworkAccess::Unrestricted,
            &scope(),
            &WorkspaceAccess::ReadOnly,
        )
        .expect("read-only workspace policy should be expressible");

        assert!(profile.contains("(allow file-read*)"));
        assert!(!profile.contains(&format!("file-write* (subpath \"{}\")", root.display())));
    }

    #[test]
    fn seatbelt_scoped_writes_allow_only_configured_subpaths() {
        let root = root("seatbelt-scoped");
        let access = WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: vec![PathBuf::from("tmp")],
        };
        let profile = seatbelt_profile_for_workspace_policy(
            &root,
            &NetworkAccess::Unrestricted,
            &scope(),
            &access,
        )
        .expect("scoped workspace policy should be expressible");
        let tmp = root.join("tmp");

        assert!(profile.contains(&format!("file-write* (subpath \"{}\")", tmp.display())));
        assert!(!profile.contains(&format!("file-write* (subpath \"{}\")", root.display())));
    }

    #[test]
    fn os_workspace_policy_rejects_out_of_scope_writable_subpath() {
        let root = root("out-of-scope");
        let access = WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: vec![PathBuf::from("../outside")],
        };
        let error = workspace_write_paths_for_os_isolation(&root, &scope(), &access)
            .expect_err("out-of-scope writable path should be denied");

        assert!(matches!(error, SandboxError::HostPathDenied { .. }));
    }

    #[test]
    fn jobobject_workspace_policy_remains_fail_closed_without_filesystem_rules() {
        let root = root("jobobject");
        let error = validate_workspace_access(
            LocalIsolation::JobObject,
            &root,
            &scope(),
            &WorkspaceAccess::ReadOnly,
        )
        .expect_err("jobobject filesystem policy is not implemented");

        assert!(matches!(
            error,
            SandboxError::CapabilityMismatch {
                ref capability,
                ..
            } if capability == "workspace_access"
        ));
    }
}

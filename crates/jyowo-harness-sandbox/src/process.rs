use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{
    BlobMeta, BlobRef, BlobRetention, Event, ExecFingerprint, KillScope, RedactRules,
    SandboxActivityHeartbeatEvent, SandboxActivityTimeoutFiredEvent,
    SandboxBackpressureAppliedEvent, SandboxError, SandboxExecutionCompletedEvent,
    SandboxExecutionStartedEvent, SandboxExitStatus, SandboxOutputSpilledEvent,
    SandboxOutputStream, SandboxOverflowSummary, SandboxPolicySummary,
};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tokio_util::io::ReaderStream;

use crate::{
    ActivityHandle, ExecContext, ExecOutcome, ExecSpec, OutputOverflow, OutputOverflowPolicy,
    OutputStream, ProcessHandle, SandboxBaseConfig, Signal, StdioSpec,
};

const NO_CACHED_SIGNAL: i32 = i32::MIN;

pub(crate) fn stdio(spec: &StdioSpec) -> Result<Stdio, SandboxError> {
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

pub(crate) fn sandbox_error(error: std::io::Error) -> SandboxError {
    SandboxError::Message(error.to_string())
}

pub(crate) async fn spawn_backend_process(
    backend_id: &'static str,
    mut command: Command,
    spec: ExecSpec,
    ctx: ExecContext,
    base: SandboxBaseConfig,
) -> Result<ProcessHandle, SandboxError> {
    command
        .stdin(stdio(&spec.stdin)?)
        .stdout(stdio(&spec.stdout)?)
        .stderr(stdio(&spec.stderr)?);

    let mut child = command.spawn().map_err(sandbox_error)?;
    let pid = child.id();
    let stdin = child
        .stdin
        .take()
        .map(|stdin| Box::pin(stdin) as crate::BoxStdin);
    let stdout_reader = child.stdout.take();
    let stderr_reader = child.stderr.take();
    let fingerprint = spec.canonical_fingerprint(&base);
    let activity = Arc::new(ManagedProcessActivity::new(
        backend_id,
        child,
        spec,
        ctx,
        fingerprint,
    ));
    let stdout = child_stream(stdout_reader, Arc::clone(&activity), OutputStream::Stdout);
    let stderr = child_stream(stderr_reader, Arc::clone(&activity), OutputStream::Stderr);

    activity
        .ctx
        .event_sink
        .emit(Event::SandboxExecutionStarted(
            SandboxExecutionStartedEvent {
                session_id: activity.ctx.session_id,
                run_id: activity.ctx.run_id,
                tool_use_id: activity.ctx.tool_use_id,
                backend_id: backend_id.to_owned(),
                fingerprint,
                policy: SandboxPolicySummary {
                    mode: activity.spec.policy.mode.clone(),
                    scope: activity.spec.policy.scope.clone(),
                    network: activity.spec.policy.network.clone(),
                    resource_limits: activity.spec.policy.resource_limits.clone(),
                },
                at: Utc::now(),
            },
        ))?;

    Ok(ProcessHandle {
        pid,
        stdout,
        stderr,
        stdin,
        cwd_marker: None,
        activity,
    })
}

struct ManagedProcessActivity {
    backend_id: &'static str,
    child: AsyncMutex<Option<Child>>,
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

impl ManagedProcessActivity {
    fn new(
        backend_id: &'static str,
        child: Child,
        spec: ExecSpec,
        ctx: ExecContext,
        fingerprint: ExecFingerprint,
    ) -> Self {
        Self {
            backend_id,
            child: AsyncMutex::new(Some(child)),
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

    async fn wait_child(&self, child: &mut Child) -> Result<SandboxExitStatus, SandboxError> {
        let timeout = timeout_future(self.spec.timeout, self.started_instant);
        let activity_timeout = activity_timeout_future(self.spec.activity_timeout, self);

        tokio::select! {
            result = child.wait() => {
                match result {
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
                }
            }
            interrupt = timeout => {
                match interrupt {
                    WaitInterrupt::Timeout => {
                        child.start_kill().map_err(sandbox_error)?;
                        let _ = child.wait().await;
                        Ok(SandboxExitStatus::Timeout)
                    }
                    WaitInterrupt::InactivityTimeout => unreachable!("timeout future cannot return inactivity"),
                }
            }
            interrupt = activity_timeout => {
                match interrupt {
                    WaitInterrupt::InactivityTimeout => {
                        child.start_kill().map_err(sandbox_error)?;
                        let _ = child.wait().await;
                        self.ctx.event_sink.emit(Event::SandboxActivityTimeoutFired(
                            SandboxActivityTimeoutFiredEvent {
                                session_id: self.ctx.session_id,
                                run_id: self.ctx.run_id,
                                tool_use_id: self.ctx.tool_use_id,
                                backend_id: self.backend_id.to_owned(),
                                configured_timeout: self.spec.activity_timeout.unwrap_or_default(),
                                kill_scope: KillScope::Process,
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

    fn redact_output(&self, bytes: Bytes) -> Bytes {
        if !self.spec.output_policy.redact_secrets {
            return bytes;
        }
        let input = String::from_utf8_lossy(&bytes);
        Bytes::from(self.ctx.redactor.redact(&input, &RedactRules::default()))
    }
}

#[async_trait]
impl ActivityHandle for ManagedProcessActivity {
    async fn wait(&self) -> Result<ExecOutcome, SandboxError> {
        if let Some(outcome) = self.outcome.lock().await.clone() {
            return Ok(outcome);
        }

        let mut child = self.child.lock().await.take().ok_or_else(|| {
            SandboxError::Message(format!("{} process already claimed", self.backend_id))
        })?;

        let exit_status = self.wait_child(&mut child).await?;
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
                backend_id: self.backend_id.to_owned(),
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
        if scope != KillScope::Process {
            return Err(SandboxError::Message(format!(
                "unsupported kill scope for {}: {scope:?}",
                self.backend_id
            )));
        }

        self.killed_signal.store(signal, Ordering::Relaxed);
        if let Some(child) = self.child.lock().await.as_mut() {
            child.start_kill().map_err(sandbox_error)?;
        }
        Ok(())
    }

    fn touch(&self) {
        let previous = self
            .last_activity_ms
            .swap(self.elapsed_since_start_ms(), Ordering::Relaxed);
        let _ = self.ctx.event_sink.emit(Event::SandboxActivityHeartbeat(
            SandboxActivityHeartbeatEvent {
                session_id: self.ctx.session_id,
                run_id: self.ctx.run_id,
                tool_use_id: self.ctx.tool_use_id,
                backend_id: self.backend_id.to_owned(),
                since_last_io_ms: self.elapsed_since_start_ms().saturating_sub(previous),
                at: Utc::now(),
            },
        ));
    }

    fn last_activity(&self) -> Instant {
        let elapsed = Duration::from_millis(self.last_activity_ms.load(Ordering::Relaxed));
        self.started_instant + elapsed
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
    activity: &ManagedProcessActivity,
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
    activity: Arc<ManagedProcessActivity>,
    stream: OutputStream,
) -> Option<futures::stream::BoxStream<'static, Bytes>> {
    reader.map(|reader| {
        let (tx, rx) = mpsc::channel(1);
        tokio::spawn(async move {
            let reader = ReaderStream::new(reader);
            futures::pin_mut!(reader);
            if let OutputOverflowPolicy::SpillToBlob {
                head_bytes,
                tail_bytes,
            } = activity.spec.output_policy.overflow
            {
                let mut preview = SpillPreview::new(
                    activity.spec.output_policy.max_inline_bytes,
                    head_bytes,
                    tail_bytes,
                );
                while let Some(chunk) = reader.next().await {
                    let bytes = match chunk {
                        Ok(bytes) => bytes,
                        Err(_) => break,
                    };
                    activity
                        .process_spill_output(&mut preview, stream, bytes)
                        .await;
                }
                if let Some(bytes) = preview.finish() {
                    send_output(&tx, &activity, bytes).await;
                }
            } else {
                while let Some(chunk) = reader.next().await {
                    let bytes = match chunk {
                        Ok(bytes) => bytes,
                        Err(_) => break,
                    };
                    let Some(bytes) = activity.process_output(stream, bytes).await else {
                        continue;
                    };
                    send_output(&tx, &activity, bytes).await;
                }
            }
        });
        futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|bytes| (bytes, rx))
        })
        .boxed()
    })
}

async fn send_output(tx: &mpsc::Sender<Bytes>, activity: &ManagedProcessActivity, bytes: Bytes) {
    match tx.try_send(bytes) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(bytes)) => {
            let started = Instant::now();
            if tx.send(bytes).await.is_ok() {
                activity.emit_backpressure(1, started.elapsed());
            }
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {}
    }
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

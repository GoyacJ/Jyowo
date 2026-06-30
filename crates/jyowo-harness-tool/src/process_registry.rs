use std::collections::{HashMap, VecDeque};
use std::path::{Component, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use bytes::Bytes;
use futures::{future::BoxFuture, StreamExt};
use harness_contracts::{
    CorrelationId, Event, ProcessReadInvocation, ProcessReadResult, ProcessRuntimeStatus,
    ProcessStartInvocation, ProcessStartResult, ProcessStopInvocation, ProcessStopResult,
    RedactRules, Redactor, RunId, RunScopedProcessRegistryCap, SandboxError, SandboxExitStatus,
    SessionId, TenantId, ToolError, WorkspaceAccess,
};
use harness_sandbox::{
    execute_with_lifecycle, ActivityHandle, EventSink, ExecContext, ExecSpec, KillScope,
    OutputOverflowPolicy, StdioSpec,
};
use parking_lot::Mutex;

const DEFAULT_BUFFER_BYTES: usize = 64 * 1024;
const MAX_BUFFER_BYTES: usize = 1024 * 1024;
const DEFAULT_READ_BYTES: usize = 64 * 1024;
const MAX_READ_BYTES: usize = 128 * 1024;

#[derive(Clone)]
pub struct DefaultRunScopedProcessRegistry {
    sandbox: Arc<dyn harness_sandbox::SandboxBackend>,
    inner: Arc<RegistryInner>,
}

impl DefaultRunScopedProcessRegistry {
    #[must_use]
    pub fn new(sandbox: Arc<dyn harness_sandbox::SandboxBackend>) -> Self {
        Self {
            sandbox,
            inner: Arc::new(RegistryInner::default()),
        }
    }

    #[must_use]
    pub fn active_process_count(&self) -> usize {
        self.inner.processes.lock().len()
    }
}

#[derive(Default)]
struct RegistryInner {
    next_id: AtomicU64,
    processes: Mutex<HashMap<String, Arc<ProcessEntry>>>,
}

struct ProcessEntry {
    tenant_id: TenantId,
    session_id: SessionId,
    run_id: RunId,
    process_id: String,
    pid: Option<u32>,
    activity: Arc<dyn ActivityHandle>,
    stdout: Mutex<RingBuffer>,
    stderr: Mutex<RingBuffer>,
    status: Mutex<ProcessStatus>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum ProcessStatus {
    Running,
    Exited(SandboxExitStatus),
    Stopped(Option<SandboxExitStatus>),
}

impl ProcessStatus {
    fn public_status(&self) -> ProcessRuntimeStatus {
        match self {
            Self::Running => ProcessRuntimeStatus::Running,
            Self::Exited(_) => ProcessRuntimeStatus::Exited,
            Self::Stopped(_) => ProcessRuntimeStatus::Stopped,
        }
    }

    fn exit_status(&self) -> Option<SandboxExitStatus> {
        match self {
            Self::Running => None,
            Self::Exited(status) => Some(status.clone()),
            Self::Stopped(status) => status.clone(),
        }
    }
}

impl RunScopedProcessRegistryCap for DefaultRunScopedProcessRegistry {
    fn start_process(
        &self,
        invocation: ProcessStartInvocation,
        redactor: Arc<dyn Redactor>,
    ) -> BoxFuture<'_, Result<ProcessStartResult, ToolError>> {
        Box::pin(async move {
            let spec = exec_spec(&invocation)?;
            let output_redactor = Arc::clone(&redactor);
            let event_sink = Arc::new(RecordingEventSink::default());
            let exec_ctx = ExecContext {
                session_id: invocation.session_id,
                run_id: invocation.run_id,
                tool_use_id: Some(invocation.tool_use_id),
                tenant_id: invocation.tenant_id,
                workspace_root: invocation.workspace_root.clone(),
                correlation_id: CorrelationId::new(),
                event_sink: event_sink.clone(),
                redactor,
                blob_store: None,
            };
            let mut handle = execute_with_lifecycle(Arc::clone(&self.sandbox), spec, exec_ctx)
                .await
                .map_err(ToolError::Sandbox)?;
            let process_id = format!(
                "proc-{}",
                self.inner.next_id.fetch_add(1, Ordering::SeqCst) + 1
            );
            let buffer_bytes = buffer_bytes(invocation.request.buffer_bytes);
            let entry = Arc::new(ProcessEntry {
                tenant_id: invocation.tenant_id,
                session_id: invocation.session_id,
                run_id: invocation.run_id,
                process_id: process_id.clone(),
                pid: handle.pid,
                activity: Arc::clone(&handle.activity),
                stdout: Mutex::new(RingBuffer::new(buffer_bytes)),
                stderr: Mutex::new(RingBuffer::new(buffer_bytes)),
                status: Mutex::new(ProcessStatus::Running),
            });
            self.inner
                .processes
                .lock()
                .insert(process_id.clone(), Arc::clone(&entry));

            if let Some(stdout) = handle.stdout.take() {
                tokio::spawn(read_stream(
                    stdout,
                    Arc::clone(&entry),
                    OutputKind::Stdout,
                    Arc::clone(&output_redactor),
                ));
            }
            if let Some(stderr) = handle.stderr.take() {
                tokio::spawn(read_stream(
                    stderr,
                    Arc::clone(&entry),
                    OutputKind::Stderr,
                    output_redactor,
                ));
            }
            tokio::spawn(wait_for_exit(Arc::clone(&entry)));

            Ok(ProcessStartResult {
                process_id,
                pid: entry.pid,
                status: ProcessRuntimeStatus::Running,
                sandbox_events: event_sink.events(),
            })
        })
    }

    fn read_process(
        &self,
        invocation: ProcessReadInvocation,
        redactor: Arc<dyn Redactor>,
    ) -> BoxFuture<'_, Result<ProcessReadResult, ToolError>> {
        Box::pin(async move {
            let entry = self.entry_for_run(
                invocation.tenant_id,
                invocation.session_id,
                invocation.run_id,
                &invocation.request.process_id,
            )?;
            let max_bytes = read_bytes(invocation.request.max_bytes);
            let stdout = entry.stdout.lock().snapshot(max_bytes);
            let stderr = entry.stderr.lock().snapshot(max_bytes);
            let status = entry.status.lock().clone();
            Ok(ProcessReadResult {
                process_id: entry.process_id.clone(),
                status: status.public_status(),
                stdout: redact_bytes(stdout.bytes, redactor.as_ref()),
                stderr: redact_bytes(stderr.bytes, redactor.as_ref()),
                stdout_truncated: stdout.truncated,
                stderr_truncated: stderr.truncated,
                exit_status: status.exit_status(),
            })
        })
    }

    fn stop_process(
        &self,
        invocation: ProcessStopInvocation,
    ) -> BoxFuture<'_, Result<ProcessStopResult, ToolError>> {
        Box::pin(async move {
            let entry = self.entry_for_run(
                invocation.tenant_id,
                invocation.session_id,
                invocation.run_id,
                &invocation.request.process_id,
            )?;
            kill_activity(&entry.activity)
                .await
                .map_err(ToolError::Sandbox)?;
            let mut status = entry.status.lock();
            if matches!(*status, ProcessStatus::Running) {
                *status = ProcessStatus::Stopped(None);
            }
            Ok(ProcessStopResult {
                process_id: entry.process_id.clone(),
                status: ProcessRuntimeStatus::Stopped,
            })
        })
    }

    fn cleanup_run(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
    ) -> BoxFuture<'_, Result<(), ToolError>> {
        Box::pin(async move {
            let entries = {
                let mut processes = self.inner.processes.lock();
                let process_ids: Vec<String> = processes
                    .iter()
                    .filter_map(|(process_id, entry)| {
                        (entry.tenant_id == tenant_id
                            && entry.session_id == session_id
                            && entry.run_id == run_id)
                            .then(|| process_id.clone())
                    })
                    .collect();
                process_ids
                    .into_iter()
                    .filter_map(|process_id| processes.remove(&process_id))
                    .collect::<Vec<_>>()
            };

            for entry in entries {
                let _ = kill_activity(&entry.activity).await;
                let mut status = entry.status.lock();
                if matches!(*status, ProcessStatus::Running) {
                    *status = ProcessStatus::Stopped(None);
                }
            }
            Ok(())
        })
    }
}

impl DefaultRunScopedProcessRegistry {
    fn entry_for_run(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
        process_id: &str,
    ) -> Result<Arc<ProcessEntry>, ToolError> {
        let entry = self
            .inner
            .processes
            .lock()
            .get(process_id)
            .cloned()
            .ok_or_else(|| ToolError::Message(format!("process not found: {process_id}")))?;
        if entry.tenant_id != tenant_id || entry.session_id != session_id || entry.run_id != run_id
        {
            return Err(ToolError::Message(format!(
                "process not found for this run: {process_id}"
            )));
        }
        Ok(entry)
    }
}

fn exec_spec(invocation: &ProcessStartInvocation) -> Result<ExecSpec, ToolError> {
    let cwd = invocation
        .request
        .cwd
        .as_deref()
        .map(valid_relative_cwd)
        .transpose()?;
    Ok(ExecSpec {
        command: invocation.request.command.clone(),
        args: invocation.request.args.clone(),
        cwd,
        stdin: StdioSpec::Null,
        stdout: StdioSpec::Piped,
        stderr: StdioSpec::Piped,
        workspace_access: WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: Vec::new(),
        },
        output_policy: harness_sandbox::OutputPolicy {
            max_inline_bytes: 0,
            overflow: OutputOverflowPolicy::Truncate,
            redact_secrets: true,
        },
        ..ExecSpec::default()
    })
}

fn valid_relative_cwd(value: &str) -> Result<PathBuf, ToolError> {
    let path = PathBuf::from(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(ToolError::Validation(
            "cwd must be a workspace-relative path".to_owned(),
        ));
    }
    Ok(path)
}

async fn read_stream(
    mut stream: futures::stream::BoxStream<'static, Bytes>,
    entry: Arc<ProcessEntry>,
    kind: OutputKind,
    redactor: Arc<dyn Redactor>,
) {
    while let Some(chunk) = stream.next().await {
        let redacted = redactor.redact(&String::from_utf8_lossy(&chunk), &RedactRules::default());
        match kind {
            OutputKind::Stdout => entry.stdout.lock().push(redacted.as_bytes()),
            OutputKind::Stderr => entry.stderr.lock().push(redacted.as_bytes()),
        }
    }
}

async fn wait_for_exit(entry: Arc<ProcessEntry>) {
    if let Ok(outcome) = entry.activity.wait().await {
        let mut status = entry.status.lock();
        match *status {
            ProcessStatus::Running => {
                *status = ProcessStatus::Exited(outcome.exit_status);
            }
            ProcessStatus::Stopped(ref mut existing) => {
                *existing = Some(outcome.exit_status);
            }
            ProcessStatus::Exited(_) => {}
        }
    }
}

async fn kill_activity(activity: &Arc<dyn ActivityHandle>) -> Result<(), SandboxError> {
    if activity.kill(9, KillScope::ProcessGroup).await.is_err() {
        activity.kill(9, KillScope::Process).await?;
    }
    Ok(())
}

enum OutputKind {
    Stdout,
    Stderr,
}

struct RingBuffer {
    bytes: VecDeque<u8>,
    capacity: usize,
    truncated: bool,
}

impl RingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            bytes: VecDeque::with_capacity(capacity),
            capacity,
            truncated: false,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        for byte in chunk {
            if self.bytes.len() == self.capacity {
                self.bytes.pop_front();
                self.truncated = true;
            }
            self.bytes.push_back(*byte);
        }
    }

    fn snapshot(&self, max_bytes: usize) -> OutputSnapshot {
        let take = self.bytes.len().min(max_bytes);
        let skip = self.bytes.len().saturating_sub(take);
        OutputSnapshot {
            bytes: self.bytes.iter().skip(skip).copied().collect(),
            truncated: self.truncated || skip > 0,
        }
    }
}

struct OutputSnapshot {
    bytes: Vec<u8>,
    truncated: bool,
}

fn buffer_bytes(value: Option<u32>) -> usize {
    value
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_BUFFER_BYTES)
        .min(MAX_BUFFER_BYTES)
}

fn read_bytes(value: Option<u32>) -> usize {
    value
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_READ_BYTES)
        .min(MAX_READ_BYTES)
}

fn redact_bytes(bytes: Vec<u8>, redactor: &dyn Redactor) -> String {
    let text = String::from_utf8_lossy(&bytes);
    redactor.redact(&text, &RedactRules::default())
}

#[derive(Default)]
struct RecordingEventSink {
    events: Mutex<Vec<Event>>,
}

impl RecordingEventSink {
    fn events(&self) -> Vec<Event> {
        self.events.lock().clone()
    }
}

impl EventSink for RecordingEventSink {
    fn emit(&self, event: Event) -> Result<(), SandboxError> {
        self.events.lock().push(event);
        Ok(())
    }
}

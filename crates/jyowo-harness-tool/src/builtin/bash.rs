use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use futures::{stream, stream::BoxStream, StreamExt};
use harness_contracts::{
    ActionResource, CorrelationId, DecisionScope, Event, MessagePart, NetworkAccess,
    PermissionSubject, Redactor, SandboxError, SandboxExitStatus, ToolActionPlan, ToolCapability,
    ToolDescriptor, ToolError, ToolGroup, ToolResult, WorkspaceAccess,
};
use harness_permission::{DangerousPatternLibrary, PermissionCheck};
use harness_sandbox::{
    execute_with_lifecycle, ActivityHandle, EventSink, ExecContext, ExecOutcome, ExecSpec,
    KillScope, OutputOverflow, OutputStream, ProcessHandle, StdioSpec,
};
use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::{
    action_plan_from_permission_check, AuthorizedToolInput, InterruptToken, Tool, ToolContext,
    ToolEvent, ToolStream, ValidationError,
};
use harness_contracts::ToolExecutionChannel;

#[derive(Clone)]
pub struct BashTool {
    descriptor: ToolDescriptor,
}

impl Default for BashTool {
    fn default() -> Self {
        Self {
            descriptor: super::with_long_running(
                super::with_output_schema(
                    super::descriptor(
                        "Bash",
                        "Bash",
                        "Execute a shell command through the configured sandbox.",
                        ToolGroup::Shell,
                        false,
                        false,
                        true,
                        256_000,
                        Vec::new(),
                        super::object_schema(
                            &["command"],
                            json!({
                                "command": { "type": "string" },
                                "cwd": { "type": "string" }
                            }),
                        ),
                    ),
                    json!({
                        "type": "object",
                        "required": ["exit_status", "stdout_bytes_observed", "stderr_bytes_observed"],
                        "properties": {
                            "exit_status": { "type": "object" },
                            "stdout_bytes_observed": { "type": "integer", "minimum": 0 },
                            "stderr_bytes_observed": { "type": "integer", "minimum": 0 },
                            "overflow": {
                                "type": ["object", "null"],
                                "properties": {
                                    "stream": { "type": "string", "enum": ["stdout", "stderr", "combined"] },
                                    "original_bytes": { "type": "integer", "minimum": 0 },
                                    "effective_limit": { "type": "integer", "minimum": 0 },
                                    "blob_ref": { "type": "object" }
                                }
                            }
                        },
                        "additionalProperties": false
                    }),
                ),
                super::long_running_policy(Duration::from_secs(5), Duration::from_secs(600)),
            ),
        }
    }
}

#[async_trait]
impl Tool for BashTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        command(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let spec = exec_spec_for_input(input);
        if let Some(rule) = DangerousPatternLibrary::default_unix().detect_command(&spec.command) {
            return action_plan_from_permission_check(
                &self.descriptor,
                input,
                ctx,
                PermissionCheck::DangerousCommand {
                    command: spec.command.clone(),
                    pattern: rule.id.clone(),
                    severity: rule.severity,
                },
                vec![command_resource(spec, ctx)],
                WorkspaceAccess::ReadWrite {
                    allowed_writable_subpaths: Vec::new(),
                },
                NetworkAccess::None,
                ToolExecutionChannel::ProcessSandbox,
            );
        }

        let base = ctx
            .sandbox
            .as_ref()
            .map(|sandbox| sandbox.base_config())
            .unwrap_or_default();
        let fingerprint = spec.canonical_fingerprint(&base);
        action_plan_from_permission_check(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::AskUser {
                subject: PermissionSubject::CommandExec {
                    command: spec.command.clone(),
                    argv: Vec::new(),
                    cwd: spec.cwd.clone(),
                    fingerprint: Some(fingerprint),
                },
                scope: DecisionScope::ExactCommand {
                    command: spec.command.clone(),
                    cwd: spec.cwd.clone(),
                },
            },
            vec![ActionResource::Command {
                command: spec.command,
                argv: Vec::new(),
                cwd: spec.cwd,
                fingerprint,
            }],
            WorkspaceAccess::ReadWrite {
                allowed_writable_subpaths: Vec::new(),
            },
            NetworkAccess::None,
            ToolExecutionChannel::ProcessSandbox,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let sandbox = ctx.sandbox.clone().ok_or_else(|| {
            ToolError::CapabilityMissing(ToolCapability::Custom("sandbox_backend".to_owned()))
        })?;
        let spec = exec_spec_from_plan(&authorized, &ctx)?;
        let event_sink = Arc::new(RecordingEventSink::default());
        let exec_ctx = exec_context(&ctx, event_sink.clone());

        let handle = execute_with_lifecycle(sandbox, spec, exec_ctx.clone())
            .await
            .map_err(ToolError::Sandbox)?;
        Ok(stream_process_output(
            handle,
            event_sink,
            ctx.interrupt.clone(),
        ))
    }
}

fn exec_spec_from_plan(
    authorized: &AuthorizedToolInput,
    ctx: &ToolContext,
) -> Result<ExecSpec, ToolError> {
    let Some(ActionResource::Command {
        command,
        argv,
        cwd,
        fingerprint,
    }) = authorized
        .action_plan()
        .resources
        .iter()
        .find_map(|resource| {
            matches!(resource, ActionResource::Command { .. }).then_some(resource)
        })
    else {
        return Err(ToolError::PermissionDenied(
            "authorized command resource missing".to_owned(),
        ));
    };
    let spec = ExecSpec {
        command: command.clone(),
        args: argv.clone(),
        cwd: cwd.clone(),
        stdin: StdioSpec::Null,
        stdout: StdioSpec::Piped,
        stderr: StdioSpec::Piped,
        policy: authorized.action_plan().sandbox_policy.clone(),
        workspace_access: authorized.action_plan().workspace_access.clone(),
        ..ExecSpec::default()
    };
    let base = ctx
        .sandbox
        .as_ref()
        .map(|sandbox| sandbox.base_config())
        .unwrap_or_default();
    if spec.canonical_fingerprint(&base) != *fingerprint {
        return Err(ToolError::PermissionDenied(
            "authorized command fingerprint mismatch".to_owned(),
        ));
    }
    Ok(spec)
}

fn exec_spec_for_input(input: &Value) -> ExecSpec {
    ExecSpec {
        command: command(input).unwrap_or_default().to_owned(),
        cwd: cwd(input),
        stdin: StdioSpec::Null,
        stdout: StdioSpec::Piped,
        stderr: StdioSpec::Piped,
        workspace_access: WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: Vec::new(),
        },
        ..ExecSpec::default()
    }
}

fn command_resource(spec: ExecSpec, ctx: &ToolContext) -> ActionResource {
    let base = ctx
        .sandbox
        .as_ref()
        .map(|sandbox| sandbox.base_config())
        .unwrap_or_default();
    let fingerprint = spec.canonical_fingerprint(&base);
    ActionResource::Command {
        command: spec.command,
        argv: Vec::new(),
        cwd: spec.cwd,
        fingerprint,
    }
}

pub(super) fn exec_context(ctx: &ToolContext, event_sink: Arc<dyn EventSink>) -> ExecContext {
    ExecContext {
        session_id: ctx.session_id,
        run_id: ctx.run_id,
        tool_use_id: Some(ctx.tool_use_id),
        tenant_id: ctx.tenant_id,
        workspace_root: ctx.workspace_root.clone(),
        correlation_id: CorrelationId::new(),
        event_sink,
        redactor: Arc::clone(&ctx.redactor) as Arc<dyn Redactor>,
        blob_store: None,
        execution_id: 0,
    }
}

fn stream_process_output(
    handle: ProcessHandle,
    event_sink: Arc<RecordingEventSink>,
    interrupt: InterruptToken,
) -> ToolStream {
    let activity = handle.activity.clone();
    let state = BashStreamState {
        journal_events: VecDeque::new(),
        journal_offset: 0,
        event_sink,
        stdout: handle.stdout,
        stderr: handle.stderr,
        stdout_decoder: Utf8ChunkDecoder::default(),
        stderr_decoder: Utf8ChunkDecoder::default(),
        phase: BashStreamPhase::JournalBeforeOutput,
        activity: handle.activity,
        interrupt,
        kill_on_drop: KillOnDrop::new(activity),
        outcome: None,
    };

    Box::pin(stream::unfold(state, |mut state| async move {
        loop {
            match state.phase {
                BashStreamPhase::JournalBeforeOutput => {
                    if let Some(event) = next_journal_event(&mut state) {
                        return Some((ToolEvent::Journal(event), state));
                    }
                    state.phase = BashStreamPhase::Stdout;
                }
                BashStreamPhase::Stdout => {
                    match next_text_partial(
                        &mut state.stdout,
                        &mut state.stdout_decoder,
                        &state.activity,
                        &state.interrupt,
                    )
                    .await
                    {
                        StreamChunk::Text(text) => {
                            return Some((ToolEvent::Partial(MessagePart::Text(text)), state));
                        }
                        StreamChunk::Done => state.phase = BashStreamPhase::Stderr,
                        StreamChunk::Interrupted => state.phase = BashStreamPhase::WaitOutcome,
                        StreamChunk::Error(error) => {
                            state.phase = BashStreamPhase::Done;
                            return Some((ToolEvent::Error(error), state));
                        }
                    }
                }
                BashStreamPhase::Stderr => {
                    match next_text_partial(
                        &mut state.stderr,
                        &mut state.stderr_decoder,
                        &state.activity,
                        &state.interrupt,
                    )
                    .await
                    {
                        StreamChunk::Text(text) => {
                            return Some((ToolEvent::Partial(MessagePart::Text(text)), state));
                        }
                        StreamChunk::Done => state.phase = BashStreamPhase::WaitOutcome,
                        StreamChunk::Interrupted => state.phase = BashStreamPhase::WaitOutcome,
                        StreamChunk::Error(error) => {
                            state.phase = BashStreamPhase::Done;
                            return Some((ToolEvent::Error(error), state));
                        }
                    }
                }
                BashStreamPhase::WaitOutcome => {
                    let outcome =
                        match wait_outcome_or_interrupt(&state.activity, &state.interrupt).await {
                            Ok(outcome) => outcome,
                            Err(error) => {
                                state.kill_on_drop.disarm();
                                state.phase = BashStreamPhase::Done;
                                return Some((ToolEvent::Error(ToolError::Sandbox(error)), state));
                            }
                        };
                    state.kill_on_drop.disarm();
                    state.outcome = Some(outcome);
                    state.phase = BashStreamPhase::JournalAfterWait;
                }
                BashStreamPhase::JournalAfterWait => {
                    if let Some(event) = next_journal_event(&mut state) {
                        return Some((ToolEvent::Journal(event), state));
                    }
                    state.phase = BashStreamPhase::Final;
                }
                BashStreamPhase::Final => {
                    state.phase = BashStreamPhase::Done;
                    let outcome = state
                        .outcome
                        .as_ref()
                        .expect("outcome is set before final result");
                    return Some((ToolEvent::Final(outcome_result(outcome)), state));
                }
                BashStreamPhase::Done => return None,
            }
        }
    }))
}

fn next_journal_event(state: &mut BashStreamState) -> Option<Event> {
    if let Some(event) = state.journal_events.pop_front() {
        return Some(event);
    }

    let events = state.event_sink.events_from(state.journal_offset);
    state.journal_offset += events.len();
    state.journal_events = VecDeque::from(events);
    state.journal_events.pop_front()
}

enum BashStreamPhase {
    JournalBeforeOutput,
    Stdout,
    Stderr,
    WaitOutcome,
    JournalAfterWait,
    Final,
    Done,
}

struct BashStreamState {
    journal_events: VecDeque<Event>,
    journal_offset: usize,
    event_sink: Arc<RecordingEventSink>,
    stdout: Option<BoxStream<'static, Bytes>>,
    stderr: Option<BoxStream<'static, Bytes>>,
    stdout_decoder: Utf8ChunkDecoder,
    stderr_decoder: Utf8ChunkDecoder,
    phase: BashStreamPhase,
    activity: Arc<dyn ActivityHandle>,
    interrupt: InterruptToken,
    kill_on_drop: KillOnDrop,
    outcome: Option<ExecOutcome>,
}

enum StreamChunk {
    Text(String),
    Done,
    Interrupted,
    Error(ToolError),
}

async fn next_text_partial(
    stream: &mut Option<BoxStream<'static, Bytes>>,
    decoder: &mut Utf8ChunkDecoder,
    activity: &Arc<dyn ActivityHandle>,
    interrupt: &InterruptToken,
) -> StreamChunk {
    loop {
        let Some(output) = stream.as_mut() else {
            return match decoder.finish() {
                Ok(Some(text)) => StreamChunk::Text(text),
                Ok(None) => StreamChunk::Done,
                Err(error) => StreamChunk::Error(error),
            };
        };

        let next = output.next();
        tokio::pin!(next);
        let chunk = tokio::select! {
            chunk = &mut next => chunk,
            _ = tokio::time::sleep(std::time::Duration::from_millis(5)) => {
                if interrupt.is_interrupted() {
                    return match kill_activity(activity).await {
                        Ok(()) => StreamChunk::Interrupted,
                        Err(error) => StreamChunk::Error(ToolError::Sandbox(error)),
                    };
                }
                continue;
            }
        };

        match chunk {
            Some(chunk) => match decoder.push(chunk) {
                Ok(Some(text)) => return StreamChunk::Text(text),
                Ok(None) => {}
                Err(error) => return StreamChunk::Error(error),
            },
            None => {
                *stream = None;
            }
        }
    }
}

pub(super) async fn wait_outcome_or_interrupt(
    activity: &Arc<dyn ActivityHandle>,
    interrupt: &InterruptToken,
) -> Result<ExecOutcome, SandboxError> {
    let mut kill_sent = false;
    let wait = activity.wait();
    tokio::pin!(wait);
    loop {
        tokio::select! {
            outcome = &mut wait => return outcome,
            _ = tokio::time::sleep(std::time::Duration::from_millis(5)), if !kill_sent => {
                if interrupt.is_interrupted() {
                    kill_sent = true;
                    kill_activity(activity).await?;
                }
            }
        }
    }
}

async fn kill_activity(activity: &Arc<dyn ActivityHandle>) -> Result<(), SandboxError> {
    if activity.kill(9, KillScope::ProcessGroup).await.is_err() {
        activity.kill(9, KillScope::Process).await?;
    }
    Ok(())
}

#[derive(Default)]
struct Utf8ChunkDecoder {
    pending: Vec<u8>,
}

impl Utf8ChunkDecoder {
    fn push(&mut self, chunk: Bytes) -> Result<Option<String>, ToolError> {
        self.pending.extend_from_slice(&chunk);
        match std::str::from_utf8(&self.pending) {
            Ok(text) => {
                let text = text.to_owned();
                self.pending.clear();
                Ok(non_empty_text(text))
            }
            Err(error) => {
                if error.error_len().is_some() {
                    return Err(ToolError::Message(error.to_string()));
                }
                let valid_up_to = error.valid_up_to();
                if valid_up_to == 0 {
                    return Ok(None);
                }
                let text = String::from_utf8(self.pending[..valid_up_to].to_vec())
                    .map_err(|error| ToolError::Message(error.to_string()))?;
                self.pending.drain(..valid_up_to);
                Ok(non_empty_text(text))
            }
        }
    }

    fn finish(&mut self) -> Result<Option<String>, ToolError> {
        if self.pending.is_empty() {
            return Ok(None);
        }
        let text = String::from_utf8(std::mem::take(&mut self.pending))
            .map_err(|error| ToolError::Message(error.to_string()))?;
        Ok(non_empty_text(text))
    }
}

fn non_empty_text(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

pub(super) struct KillOnDrop {
    activity: Arc<dyn ActivityHandle>,
    armed: bool,
}

impl KillOnDrop {
    pub(super) fn new(activity: Arc<dyn ActivityHandle>) -> Self {
        Self {
            activity,
            armed: true,
        }
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let activity = Arc::clone(&self.activity);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                if activity.kill(9, KillScope::ProcessGroup).await.is_err() {
                    let _ = activity.kill(9, KillScope::Process).await;
                }
            });
        }
    }
}

fn outcome_result(outcome: &ExecOutcome) -> ToolResult {
    ToolResult::Structured(json!({
        "exit_status": exit_status_json(&outcome.exit_status),
        "stdout_bytes_observed": outcome.stdout_bytes_observed,
        "stderr_bytes_observed": outcome.stderr_bytes_observed,
        "overflow": outcome.overflow.as_ref().map(output_overflow_json)
    }))
}

fn output_overflow_json(overflow: &OutputOverflow) -> Value {
    json!({
        "stream": output_stream_name(overflow.stream),
        "original_bytes": overflow.original_bytes,
        "effective_limit": overflow.effective_limit,
        "blob_ref": overflow.blob_ref,
    })
}

fn output_stream_name(stream: OutputStream) -> &'static str {
    match stream {
        OutputStream::Stdout => "stdout",
        OutputStream::Stderr => "stderr",
        OutputStream::Combined => "combined",
    }
}

fn exit_status_json(status: &SandboxExitStatus) -> Value {
    match status {
        SandboxExitStatus::Code(code) => json!({ "code": code }),
        SandboxExitStatus::Signal(signal) => json!({ "signal": signal }),
        SandboxExitStatus::Timeout => json!({ "timeout": true }),
        SandboxExitStatus::InactivityTimeout => json!({ "inactivity_timeout": true }),
        SandboxExitStatus::OutputBudgetExceeded => json!({ "output_budget_exceeded": true }),
        SandboxExitStatus::Cancelled => json!({ "cancelled": true }),
        SandboxExitStatus::BackendError => json!({ "backend_error": true }),
        _ => json!({ "unknown": true }),
    }
}

fn command(input: &Value) -> Result<&str, ValidationError> {
    input
        .get("command")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ValidationError::from("command is required"))
}

fn cwd(input: &Value) -> Option<PathBuf> {
    input.get("cwd").and_then(Value::as_str).map(PathBuf::from)
}

#[derive(Default)]
pub(super) struct RecordingEventSink {
    events: Mutex<Vec<Event>>,
}

impl RecordingEventSink {
    pub(super) fn events_from(&self, offset: usize) -> Vec<Event> {
        self.events.lock().iter().skip(offset).cloned().collect()
    }
}

impl EventSink for RecordingEventSink {
    fn emit(&self, event: Event) -> Result<(), SandboxError> {
        self.events.lock().push(event);
        Ok(())
    }
}

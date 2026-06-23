use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::StreamExt;
use harness_context::{ContextEngine, ContextSessionView};
use harness_contracts::{
    AssistantDeltaProducedEvent, AssistantMessageCompletedEvent, BlobStore, CapabilityRegistry,
    CausationId, CorrelationId, DecidedBy, Decision, DecisionId, DeltaChunk, DenyReason, EndReason,
    Event, EventId, FallbackPolicy, InteractivityLevel, Message, MessageContent, MessageId,
    MessageMetadata, MessagePart, MessageRole, NoopRedactor, PermissionMode,
    PermissionRequestedEvent, PermissionResolvedEvent, Redactor, RunEndedEvent, RunId,
    RunStartedEvent, SessionError, SessionId, StopReason, TeamId, TenantId, ToolDescriptor,
    ToolError, ToolErrorPayload, ToolResult, ToolUseApprovedEvent, ToolUseCompletedEvent,
    ToolUseDeniedEvent, ToolUseFailedEvent, ToolUseId, ToolUseRequestedEvent, ToolUseSummary,
    TrustLevel, TurnInput, UsageSnapshot,
};
use harness_hook::{
    HookContext, HookDispatcher, HookEvent, HookMessageView, HookOutcome, HookSessionView,
    ReplayMode, ToolDescriptorView,
};
use harness_model::{
    ContentDelta, InferContext, ModelModality, ModelProtocol, ModelProvider, ModelRequest,
    ModelStreamEvent,
};
use harness_permission::{
    PermissionBroker, PermissionContext, PermissionRequest, PersistedDecision, RuleSnapshot,
};
use harness_sandbox::SandboxBackend;
use harness_tool::{
    InterruptToken, OrchestratorContext, ToolCall, ToolEventEmitter, ToolOrchestrator, ToolPool,
    ToolResultEnvelope as RuntimeToolResultEnvelope,
};
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};

use crate::Session;

const DEFAULT_CONVERSATION_TURN_DEADLINE: Duration = Duration::from_secs(15 * 60);

#[derive(Clone)]
pub struct SessionTurnRuntime {
    pub context: ContextEngine,
    pub hooks: HookDispatcher,
    pub model: Arc<dyn ModelProvider>,
    pub tools: ToolPool,
    pub permission_broker: Arc<dyn PermissionBroker>,
    pub sandbox: Option<Arc<dyn SandboxBackend>>,
    pub cap_registry: Arc<CapabilityRegistry>,
    pub blob_store: Option<Arc<dyn BlobStore>>,
    pub model_id: String,
    pub model_extra: Value,
    pub protocol: ModelProtocol,
    pub system_prompt: Option<String>,
}

pub(crate) async fn run_turn(
    session: &Session,
    runtime: SessionTurnRuntime,
    parts: Vec<MessagePart>,
    client_message_id: Option<String>,
) -> Result<(), SessionError> {
    let run_id = RunId::new();
    let projection = session.projection().await;
    let prompt = text_from_parts(&parts);
    let turn_input = TurnInput {
        message: Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts,
            created_at: harness_contracts::now(),
        },
        metadata: turn_metadata(
            next_turn_index(&projection.messages),
            client_message_id.clone(),
        ),
    };

    let run_started = Event::RunStarted(RunStartedEvent {
        run_id,
        session_id: session.session_id(),
        tenant_id: session.tenant_id(),
        parent_run_id: None,
        input: turn_input.clone(),
        snapshot_id: session.config_snapshot_id(),
        effective_config_hash: session.effective_config_hash(),
        started_at: harness_contracts::now(),
        correlation_id: CorrelationId::new(),
    });
    session
        .append_events(std::slice::from_ref(&run_started))
        .await?;
    let mut projection_events = vec![run_started];

    let hook_result = match runtime
        .hooks
        .dispatch(
            HookEvent::UserPromptSubmit {
                run_id,
                input: json!({ "prompt": prompt }),
            },
            hook_context(session, run_id, &projection.messages),
        )
        .await
    {
        Ok(result) => result,
        Err(error) => {
            return finalize_run_error(session, run_id, &mut projection_events, error).await
        }
    };
    if let HookOutcome::Block { reason } = hook_result.final_outcome {
        let ended = Event::RunEnded(RunEndedEvent {
            run_id,
            reason: EndReason::Error(reason),
            usage: Some(UsageSnapshot::default()),
            ended_at: harness_contracts::now(),
        });
        session.append_events(std::slice::from_ref(&ended)).await?;
        projection_events.push(ended);
        session.apply_projection_events(&projection_events).await;
        return Err(SessionError::Message("run blocked by hook".to_owned()));
    }

    let turn_input = match apply_steering(session, run_id, turn_input).await {
        Ok(turn_input) => turn_input,
        Err(error) => {
            return finalize_run_error(session, run_id, &mut projection_events, error).await
        }
    };

    let prompt_view = TurnContextView {
        tenant_id: session.tenant_id(),
        session_id: session.session_id(),
        user_id: session.options().user_id.clone(),
        team_id: session.options().team_id,
        system: runtime.system_prompt.clone(),
        messages: projection.messages.clone(),
        tools: runtime
            .tools
            .iter()
            .map(|tool| tool.descriptor().clone())
            .collect(),
    };
    let assembled = match runtime.context.assemble(&prompt_view, &turn_input).await {
        Ok(assembled) => assembled,
        Err(error) => {
            return finalize_run_error(session, run_id, &mut projection_events, error).await
        }
    };
    if let Err(error) = validate_model_input_modalities(
        &assembled.messages,
        &runtime
            .model
            .snapshot_for_model(&runtime.model_id)
            .conversation_capability
            .input_modalities,
    ) {
        return finalize_run_error(session, run_id, &mut projection_events, error).await;
    }

    let user_message_appended =
        Event::UserMessageAppended(harness_contracts::UserMessageAppendedEvent {
            run_id,
            message_id: turn_input.message.id,
            content: message_content(&turn_input.message),
            metadata: message_metadata(client_message_id.as_deref()),
            at: harness_contracts::now(),
        });
    session
        .append_events(std::slice::from_ref(&user_message_appended))
        .await?;
    projection_events.push(user_message_appended);

    let request = ModelRequest {
        model_id: runtime.model_id.clone(),
        messages: assembled.messages,
        tools: (!assembled.tools_snapshot.is_empty()).then_some(assembled.tools_snapshot),
        system: assembled.system,
        temperature: None,
        max_tokens: None,
        stream: true,
        cache_breakpoints: assembled.cache_breakpoints,
        protocol: runtime.protocol,
        extra: runtime.model_extra.clone(),
    };
    let mut infer_ctx = InferContext::for_test();
    infer_ctx.tenant_id = session.tenant_id();
    infer_ctx.session_id = Some(session.session_id());
    infer_ctx.run_id = Some(run_id);
    infer_ctx.blob_store = runtime.blob_store.clone();
    if infer_ctx.deadline.is_none() {
        infer_ctx.deadline = Some(Instant::now() + DEFAULT_CONVERSATION_TURN_DEADLINE);
    }

    let mut stream = match runtime.model.infer(request, infer_ctx).await {
        Ok(stream) => stream,
        Err(error) => {
            return finalize_run_error(session, run_id, &mut projection_events, error).await
        }
    };
    let mut assistant_text = String::new();
    let assistant_message_id = MessageId::new();
    let mut tool_calls = Vec::new();
    let mut usage = UsageSnapshot::default();
    let mut stop_reason = StopReason::EndTurn;

    while let Some(event) = stream.next().await {
        match event {
            ModelStreamEvent::MessageStart {
                usage: start_usage, ..
            } => add_usage(&mut usage, &start_usage),
            ModelStreamEvent::ContentBlockDelta { delta, .. } => match delta {
                ContentDelta::Text(text) => {
                    assistant_text.push_str(&text);
                    let delta_event = Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                        run_id,
                        message_id: assistant_message_id,
                        delta: DeltaChunk::Text(text),
                        at: harness_contracts::now(),
                    });
                    session
                        .append_events(std::slice::from_ref(&delta_event))
                        .await?;
                    projection_events.push(delta_event);
                }
                ContentDelta::Thinking(_) => {}
                ContentDelta::ToolUseComplete { id, name, input } => {
                    tool_calls.push(ToolCall {
                        tool_use_id: id,
                        tool_name: name,
                        input,
                    });
                }
                ContentDelta::ToolUseStart { .. } | ContentDelta::ToolUseInputJson(_) => {}
            },
            ModelStreamEvent::MessageDelta {
                stop_reason: next_stop_reason,
                usage_delta,
            } => {
                add_usage(&mut usage, &usage_delta);
                if let Some(next_stop_reason) = next_stop_reason {
                    stop_reason = next_stop_reason;
                }
            }
            ModelStreamEvent::StreamError { error, class, .. } => {
                return finalize_run_error(
                    session,
                    run_id,
                    &mut projection_events,
                    format!("model stream error ({class:?}): {error}"),
                )
                .await;
            }
            ModelStreamEvent::MessageStop => break,
            ModelStreamEvent::ContentBlockStart { .. }
            | ModelStreamEvent::ContentBlockStop { .. } => {}
        }
    }

    let mut pre_tool_events = Vec::with_capacity(tool_calls.len());
    for call in &tool_calls {
        let Some(descriptor) = runtime.tools.descriptor(&call.tool_name) else {
            return finalize_run_error(
                session,
                run_id,
                &mut projection_events,
                format!("tool descriptor missing: {}", call.tool_name),
            )
            .await;
        };
        pre_tool_events.push(Event::ToolUseRequested(ToolUseRequestedEvent {
            run_id,
            tool_use_id: call.tool_use_id,
            tool_name: call.tool_name.clone(),
            input: call.input.clone(),
            properties: descriptor.properties.clone(),
            causation_id: EventId::new(),
            at: harness_contracts::now(),
        }));
    }
    session.append_events(&pre_tool_events).await?;
    projection_events.extend(pre_tool_events);

    let permission_recorder = Arc::new(RecordingPermissionBroker::new(runtime.permission_broker));
    let tool_event_emitter = Arc::new(RecordingToolEventEmitter::new());
    let tool_results = ToolOrchestrator::default()
        .dispatch(
            tool_calls.clone(),
            OrchestratorContext {
                pool: runtime.tools.clone(),
                tool_context: harness_tool::ToolContext {
                    tool_use_id: ToolUseId::new(),
                    run_id,
                    session_id: session.session_id(),
                    tenant_id: session.tenant_id(),
                    correlation_id: CorrelationId::new(),
                    agent_id: harness_contracts::AgentId::from_u128(1),
                    subagent_depth: 0,
                    workspace_root: session.options().workspace_root.clone(),
                    sandbox: runtime.sandbox.clone(),
                    permission_broker: permission_recorder.clone(),
                    cap_registry: runtime.cap_registry.clone(),
                    interrupt: InterruptToken::new(),
                    parent_run: None,
                },
                permission_context: permission_context(session, run_id),
                blob_store: runtime.blob_store.clone(),
                event_emitter: tool_event_emitter.clone(),
            },
        )
        .await;

    let mut post_tool_events = permission_events(run_id, permission_recorder.records().await);
    for emitted in tool_event_emitter.take().await {
        post_tool_events.push(emitted);
    }
    for result in &tool_results {
        post_tool_events.extend(tool_result_events(result));
    }
    session.append_events(&post_tool_events).await?;
    projection_events.extend(post_tool_events);

    if let Err(error) = runtime
        .context
        .after_turn(&prompt_view, &context_tool_results(&tool_results))
        .await
    {
        return finalize_run_error(session, run_id, &mut projection_events, error).await;
    }

    let tool_summaries = tool_results
        .iter()
        .map(|result| ToolUseSummary {
            tool_use_id: result.tool_use_id,
            tool_name: result.tool_name.clone(),
        })
        .collect::<Vec<_>>();
    let answer = assistant_answer(assistant_text, &tool_results);
    let final_events = vec![
        Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
            run_id,
            message_id: assistant_message_id,
            content: MessageContent::Text(answer),
            tool_uses: tool_summaries,
            usage: usage.clone(),
            pricing_snapshot_id: None,
            stop_reason,
            at: harness_contracts::now(),
        }),
        Event::RunEnded(RunEndedEvent {
            run_id,
            reason: EndReason::Completed,
            usage: Some(usage),
            ended_at: harness_contracts::now(),
        }),
    ];
    session.append_events(&final_events).await?;
    projection_events.extend(final_events);
    session.apply_projection_events(&projection_events).await;
    Ok(())
}

fn text_from_parts(parts: &[MessagePart]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            MessagePart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn validate_model_input_modalities(
    messages: &[Message],
    supported: &[ModelModality],
) -> Result<(), SessionError> {
    for message in messages {
        for part in &message.parts {
            let required = match part {
                MessagePart::Image { .. } => Some(ModelModality::Image),
                MessagePart::Video { .. } => Some(ModelModality::Video),
                MessagePart::File { .. } => Some(ModelModality::File),
                MessagePart::Text(_)
                | MessagePart::ToolUse { .. }
                | MessagePart::ToolResult { .. }
                | MessagePart::Thinking(_) => None,
                _ => None,
            };
            if let Some(required) = required {
                if !supported.contains(&required) {
                    return Err(SessionError::Message(format!(
                        "model does not support {required:?} input"
                    )));
                }
            }
        }
    }
    Ok(())
}

async fn finalize_run_error(
    session: &Session,
    run_id: RunId,
    projection_events: &mut Vec<Event>,
    error: impl std::fmt::Display,
) -> Result<(), SessionError> {
    let message = error.to_string();
    let ended = Event::RunEnded(RunEndedEvent {
        run_id,
        reason: EndReason::Error(message.clone()),
        usage: Some(UsageSnapshot::default()),
        ended_at: harness_contracts::now(),
    });
    session.append_events(std::slice::from_ref(&ended)).await?;
    projection_events.push(ended);
    session.apply_projection_events(projection_events).await;
    Err(SessionError::Message(message))
}

#[derive(Clone)]
struct TurnContextView {
    tenant_id: TenantId,
    session_id: SessionId,
    user_id: Option<String>,
    team_id: Option<TeamId>,
    system: Option<String>,
    messages: Vec<Message>,
    tools: Vec<ToolDescriptor>,
}

impl ContextSessionView for TurnContextView {
    fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }

    fn session_id(&self) -> Option<SessionId> {
        Some(self.session_id)
    }

    fn user_id(&self) -> Option<String> {
        self.user_id.clone()
    }

    fn team_id(&self) -> Option<TeamId> {
        self.team_id
    }

    fn system(&self) -> Option<String> {
        self.system.clone()
    }

    fn messages(&self) -> Vec<Message> {
        self.messages.clone()
    }

    fn tools_snapshot(&self) -> Vec<ToolDescriptor> {
        self.tools.clone()
    }
}

struct TurnHookView {
    workspace_root: PathBuf,
    messages: Vec<Message>,
    redactor: NoopRedactor,
}

impl HookSessionView for TurnHookView {
    fn workspace_root(&self) -> Option<&Path> {
        Some(&self.workspace_root)
    }

    fn recent_messages(&self, limit: usize) -> Vec<HookMessageView> {
        self.messages
            .iter()
            .rev()
            .take(limit)
            .map(|message| HookMessageView {
                role: message.role,
                text_snippet: message_text(message),
                tool_use_id: None,
            })
            .collect()
    }

    fn permission_mode(&self) -> PermissionMode {
        PermissionMode::Default
    }

    fn redacted(&self) -> &dyn Redactor {
        &self.redactor
    }

    fn current_tool_descriptor(&self) -> Option<ToolDescriptorView> {
        None
    }
}

#[derive(Clone)]
struct PermissionDecisionRecord {
    request: PermissionRequest,
    decision: Decision,
}

struct RecordingPermissionBroker {
    inner: Arc<dyn PermissionBroker>,
    records: Mutex<Vec<PermissionDecisionRecord>>,
}

impl RecordingPermissionBroker {
    fn new(inner: Arc<dyn PermissionBroker>) -> Self {
        Self {
            inner,
            records: Mutex::new(Vec::new()),
        }
    }

    async fn records(&self) -> Vec<PermissionDecisionRecord> {
        self.records.lock().await.clone()
    }
}

#[async_trait]
impl PermissionBroker for RecordingPermissionBroker {
    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        let decision = self.inner.decide(request.clone(), ctx).await;
        self.records.lock().await.push(PermissionDecisionRecord {
            request,
            decision: decision.clone(),
        });
        decision
    }

    async fn persist(
        &self,
        decision: PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        self.inner.persist(decision).await
    }
}

struct RecordingToolEventEmitter {
    sender: mpsc::UnboundedSender<Event>,
    receiver: Mutex<mpsc::UnboundedReceiver<Event>>,
}

impl RecordingToolEventEmitter {
    fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            sender,
            receiver: Mutex::new(receiver),
        }
    }

    async fn take(&self) -> Vec<Event> {
        let mut receiver = self.receiver.lock().await;
        let mut events = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            events.push(event);
        }
        events
    }
}

impl ToolEventEmitter for RecordingToolEventEmitter {
    fn emit(&self, event: Event) {
        let _ignored = self.sender.send(event);
    }
}

fn hook_context(session: &Session, run_id: RunId, messages: &[Message]) -> HookContext {
    HookContext {
        tenant_id: session.tenant_id(),
        session_id: session.session_id(),
        run_id: Some(run_id),
        turn_index: Some(next_turn_index(messages)),
        correlation_id: CorrelationId::new(),
        causation_id: CausationId::new(),
        trust_level: TrustLevel::UserControlled,
        permission_mode: PermissionMode::Default,
        interactivity: InteractivityLevel::NoInteractive,
        at: harness_contracts::now(),
        view: Arc::new(TurnHookView {
            workspace_root: session.options().workspace_root.clone(),
            messages: messages.to_vec(),
            redactor: NoopRedactor,
        }),
        upstream_outcome: None,
        replay_mode: ReplayMode::Live,
    }
}

fn permission_context(session: &Session, run_id: RunId) -> PermissionContext {
    PermissionContext {
        permission_mode: PermissionMode::Default,
        previous_mode: None,
        session_id: session.session_id(),
        tenant_id: session.tenant_id(),
        run_id: Some(run_id),
        interactivity: InteractivityLevel::NoInteractive,
        timeout_policy: None,
        fallback_policy: FallbackPolicy::DenyAll,
        rule_snapshot: Arc::new(RuleSnapshot {
            rules: Vec::new(),
            generation: 0,
            built_at: harness_contracts::now(),
        }),
        hook_overrides: Vec::new(),
    }
}

fn permission_events(run_id: RunId, records: Vec<PermissionDecisionRecord>) -> Vec<Event> {
    let mut events = Vec::with_capacity(records.len() * 3);
    for record in records {
        events.push(Event::PermissionRequested(PermissionRequestedEvent {
            request_id: record.request.request_id,
            run_id,
            session_id: record.request.session_id,
            tenant_id: record.request.tenant_id,
            tool_use_id: record.request.tool_use_id,
            tool_name: record.request.tool_name.clone(),
            subject: record.request.subject.clone(),
            severity: record.request.severity,
            scope_hint: record.request.scope_hint.clone(),
            fingerprint: None,
            presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
            interactivity: InteractivityLevel::NoInteractive,
            causation_id: EventId::new(),
            at: harness_contracts::now(),
        }));
        events.push(Event::PermissionResolved(PermissionResolvedEvent {
            request_id: record.request.request_id,
            decision: record.decision.clone(),
            decided_by: DecidedBy::Broker {
                broker_id: "session-turn-runtime".to_owned(),
            },
            scope: record.request.scope_hint.clone(),
            fingerprint: None,
            rationale: None,
            at: harness_contracts::now(),
        }));
        if decision_allows(&record.decision) {
            events.push(Event::ToolUseApproved(ToolUseApprovedEvent {
                tool_use_id: record.request.tool_use_id,
                decision_id: DecisionId::new(),
                scope: record.request.scope_hint,
                at: harness_contracts::now(),
            }));
        } else {
            events.push(Event::ToolUseDenied(ToolUseDeniedEvent {
                tool_use_id: record.request.tool_use_id,
                reason: DenyReason::UserDenied,
                at: harness_contracts::now(),
            }));
        }
    }
    events
}

fn tool_result_events(result: &RuntimeToolResultEnvelope) -> Vec<Event> {
    match &result.result {
        Ok(tool_result) => vec![Event::ToolUseCompleted(ToolUseCompletedEvent {
            tool_use_id: result.tool_use_id,
            result: tool_result.clone(),
            usage: None,
            duration_ms: result.duration.as_millis().min(u128::from(u64::MAX)) as u64,
            at: harness_contracts::now(),
        })],
        Err(error) => vec![Event::ToolUseFailed(ToolUseFailedEvent {
            tool_use_id: result.tool_use_id,
            error: tool_error_payload(error),
            at: harness_contracts::now(),
        })],
    }
}

fn context_tool_results(
    results: &[RuntimeToolResultEnvelope],
) -> Vec<harness_contracts::ToolResultEnvelope> {
    results
        .iter()
        .map(|result| harness_contracts::ToolResultEnvelope {
            result: result
                .result
                .clone()
                .unwrap_or_else(|error| ToolResult::Text(error.to_string())),
            usage: None,
            is_error: result.result.is_err(),
            overflow: result.overflow.clone(),
        })
        .collect()
}

fn assistant_answer(
    mut assistant_text: String,
    tool_results: &[RuntimeToolResultEnvelope],
) -> String {
    for result in tool_results {
        if !assistant_text.is_empty() {
            assistant_text.push('\n');
        }
        match &result.result {
            Ok(tool_result) => {
                let _ = write!(assistant_text, "{}", tool_result_summary(tool_result));
            }
            Err(error) => {
                let _ = error;
                assistant_text.push_str("Tool error withheld from conversation transcript.");
            }
        }
    }
    assistant_text
}

fn tool_result_summary(result: &ToolResult) -> String {
    let _ = result;
    "Tool result withheld from conversation transcript.".to_owned()
}

fn message_content(message: &Message) -> MessageContent {
    if let [MessagePart::Text(text)] = message.parts.as_slice() {
        return MessageContent::Text(text.clone());
    }
    MessageContent::Multimodal(message.parts.clone())
}

fn turn_metadata(turn_index: u32, client_message_id: Option<String>) -> Value {
    let mut metadata = json!({ "turn": turn_index });
    if let Some(client_message_id) = client_message_id.filter(|value| is_uuid_v4_like(value)) {
        metadata["clientMessageId"] = json!(client_message_id);
    }
    metadata
}

fn message_metadata(client_message_id: Option<&str>) -> MessageMetadata {
    let mut metadata = MessageMetadata::default();
    if let Some(client_message_id) = client_message_id.filter(|value| is_uuid_v4_like(value)) {
        metadata
            .labels
            .insert("clientMessageId".to_owned(), client_message_id.to_owned());
    }
    metadata
}

fn is_uuid_v4_like(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }

    for index in [8, 13, 18, 23] {
        if bytes[index] != b'-' {
            return false;
        }
    }
    if bytes[14] != b'4' || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B') {
        return false;
    }

    bytes
        .iter()
        .enumerate()
        .filter(|(index, _)| !matches!(index, 8 | 13 | 18 | 23))
        .all(|(_, byte)| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{message_metadata, turn_metadata};

    #[test]
    fn turn_and_message_metadata_keep_only_uuid_v4_client_message_ids() {
        let uuid_v4 = "00000000-0000-4000-8000-000000000001";
        let uuid_v1 = "00000000-0000-1000-8000-000000000001";

        assert_eq!(
            turn_metadata(1, Some(uuid_v4.to_owned()))["clientMessageId"],
            uuid_v4
        );
        assert!(turn_metadata(1, Some(uuid_v1.to_owned()))
            .get("clientMessageId")
            .is_none());
        assert_eq!(
            message_metadata(Some(uuid_v4))
                .labels
                .get("clientMessageId")
                .map(String::as_str),
            Some(uuid_v4)
        );
        assert!(!message_metadata(Some(uuid_v1))
            .labels
            .contains_key("clientMessageId"));
    }
}

#[cfg(feature = "steering")]
async fn apply_steering(
    session: &Session,
    run_id: RunId,
    mut turn_input: TurnInput,
) -> Result<TurnInput, SessionError> {
    if let Some(merged) = session.drain_and_merge(run_id).await? {
        append_text_to_message(&mut turn_input.message, &merged.body);
    }
    Ok(turn_input)
}

#[cfg(not(feature = "steering"))]
async fn apply_steering(
    _session: &Session,
    _run_id: RunId,
    turn_input: TurnInput,
) -> Result<TurnInput, SessionError> {
    Ok(turn_input)
}

#[cfg(feature = "steering")]
fn append_text_to_message(message: &mut Message, text: &str) {
    if let Some(MessagePart::Text(existing)) = message
        .parts
        .iter_mut()
        .find(|part| matches!(part, MessagePart::Text(_)))
    {
        if !existing.is_empty() && !text.is_empty() {
            existing.push('\n');
        }
        existing.push_str(text);
        return;
    }
    message.parts.push(MessagePart::Text(text.to_owned()));
}

fn message_text(message: &Message) -> String {
    message
        .parts
        .iter()
        .filter_map(|part| match part {
            MessagePart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn next_turn_index(messages: &[Message]) -> u32 {
    messages
        .iter()
        .filter(|message| message.role == MessageRole::User)
        .count()
        .saturating_add(1)
        .min(u32::MAX as usize) as u32
}

fn add_usage(total: &mut UsageSnapshot, delta: &UsageSnapshot) {
    total.input_tokens = total.input_tokens.saturating_add(delta.input_tokens);
    total.output_tokens = total.output_tokens.saturating_add(delta.output_tokens);
    total.cache_read_tokens = total
        .cache_read_tokens
        .saturating_add(delta.cache_read_tokens);
    total.cache_write_tokens = total
        .cache_write_tokens
        .saturating_add(delta.cache_write_tokens);
    total.cost_micros = total.cost_micros.saturating_add(delta.cost_micros);
    total.tool_calls = total.tool_calls.saturating_add(delta.tool_calls);
}

fn decision_allows(decision: &Decision) -> bool {
    matches!(
        decision,
        Decision::AllowOnce | Decision::AllowSession | Decision::AllowPermanent
    )
}

fn tool_error_payload(error: &ToolError) -> ToolErrorPayload {
    ToolErrorPayload {
        code: match error {
            ToolError::Validation(_) => "validation",
            ToolError::PermissionDenied(_) => "permission_denied",
            ToolError::Sandbox(_) => "sandbox",
            ToolError::Timeout => "timeout",
            ToolError::Interrupted => "interrupted",
            ToolError::ResultTooLarge { .. } => "result_too_large",
            ToolError::OffloadFailed(_) => "offload_failed",
            ToolError::CapabilityMissing(_) => "capability_missing",
            ToolError::SchemaResolution(_) => "schema_resolution",
            ToolError::Internal(_) => "internal",
            ToolError::Message(_) => "message",
            _ => "unknown",
        }
        .to_owned(),
        message: error.to_string(),
        retriable: matches!(error, ToolError::Timeout | ToolError::Interrupted),
    }
}

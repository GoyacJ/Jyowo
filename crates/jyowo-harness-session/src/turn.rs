use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use futures::StreamExt;
use harness_context::{ContextEngine, ContextSessionView};
use harness_contracts::{
    AssistantDeltaProducedEvent, AssistantMessageCompletedEvent, BlobStore, CapabilityRegistry,
    CausationId, ConversationAttachmentReference, CorrelationId, DeltaChunk, EndReason, Event,
    EventId, FallbackPolicy, InteractivityLevel, Message, MessageContent, MessageId,
    MessageMetadata, MessagePart, MessageRole, PermissionActorSource, PermissionMode, RedactRules,
    Redactor, RunEndedEvent, RunId, RunModelSnapshot, RunStartedEvent, SessionError, SessionId,
    StopReason, TeamId, TenantId, ToolDescriptor, ToolError, ToolErrorPayload, ToolResult,
    ToolUseCompletedEvent, ToolUseFailedEvent, ToolUseId, ToolUseRequestedEvent, ToolUseSummary,
    TrustLevel, TurnInput, UsageSnapshot,
};
use harness_execution::{
    AuthorizationContext, AuthorizationEventSink, AuthorizationService, ExecutionError,
    TicketLedger,
};
use harness_hook::{
    HookContext, HookDispatcher, HookEvent, HookMessageView, HookOutcome, HookSessionView,
    ReplayMode, ToolDescriptorView,
};
use harness_model::{
    ContentDelta, InferContext, ModelModality, ModelProtocol, ModelProvider, ModelRequest,
    ModelStreamEvent,
};
use harness_permission::{NoopDecisionPersistence, PermissionAuthority, PermissionBroker};
use harness_sandbox::SandboxBackend;
use harness_tool::{
    AuthorizedTicketSummary, AuthorizedToolCall, AuthorizedToolInput, InterruptToken,
    OrchestratorContext, ToolCall, ToolEventEmitter, ToolOrchestrator, ToolPool,
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
    pub redactor: Arc<dyn Redactor>,
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
    attachments: Vec<ConversationAttachmentReference>,
    permission_mode: PermissionMode,
    _permission_actor_source: PermissionActorSource,
) -> Result<(), SessionError> {
    let run_id = RunId::new();
    let projection = session.projection().await;
    let prompt = text_from_parts(&parts);
    let model = run_model_snapshot(&runtime)?;
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
        model,
        input: turn_input.clone(),
        snapshot_id: session.config_snapshot_id(),
        effective_config_hash: session.effective_config_hash(),
        started_at: harness_contracts::now(),
        correlation_id: CorrelationId::new(),
        permission_mode,
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
                input: redact_json_strings(json!({ "prompt": prompt }), runtime.redactor.as_ref()),
            },
            hook_context(
                session,
                &runtime,
                run_id,
                &projection.messages,
                permission_mode,
            ),
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
    let model_snapshot = match runtime.model.snapshot_for_model(&runtime.model_id) {
        Ok(model_snapshot) => model_snapshot,
        Err(error) => {
            return finalize_run_error(session, run_id, &mut projection_events, error).await
        }
    };
    if let Err(error) = validate_model_input_modalities(
        &assembled.messages,
        &model_snapshot.conversation_capability.input_modalities,
    ) {
        return finalize_run_error(session, run_id, &mut projection_events, error).await;
    }

    let user_message_appended =
        Event::UserMessageAppended(harness_contracts::UserMessageAppendedEvent {
            run_id,
            message_id: turn_input.message.id,
            content: message_content(&turn_input.message),
            metadata: message_metadata(client_message_id.as_deref()),
            attachments,
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
                ContentDelta::Thinking(_) | ContentDelta::ReasoningSummary(_) => {}
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

    let (tool_event_emitter, mut tool_event_receiver) = ChannelToolEventEmitter::channel();
    let orchestrator = ToolOrchestrator::default();
    let (authorized_tool_calls, mut tool_results) = authorize_tool_calls(
        session,
        &runtime,
        run_id,
        permission_mode,
        &tool_calls,
        &mut projection_events,
    )
    .await?;
    let mut dispatch = Box::pin(orchestrator.dispatch(
        authorized_tool_calls,
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
                cap_registry: runtime.cap_registry.clone(),
                redactor: runtime.redactor.clone(),
                interrupt: InterruptToken::new(),
                parent_run: None,
                model: session.turn_model_snapshot(),
                model_config_id: session.turn_model_config_id(),
                actor_source: harness_contracts::PermissionActorSource::ParentRun,
            },
            blob_store: runtime.blob_store.clone(),
            event_emitter: tool_event_emitter,
        },
    ));
    let executed_tool_results = loop {
        tokio::select! {
            results = &mut dispatch => break results,
            Some(event) = tool_event_receiver.recv() => {
                session.append_events(std::slice::from_ref(&event)).await?;
                projection_events.push(event);
            }
        }
    };
    while let Ok(event) = tool_event_receiver.try_recv() {
        session.append_events(std::slice::from_ref(&event)).await?;
        projection_events.push(event);
    }
    tool_results.extend(executed_tool_results);

    let mut post_tool_events = Vec::new();
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

async fn authorize_tool_calls(
    session: &Session,
    runtime: &SessionTurnRuntime,
    run_id: RunId,
    permission_mode: PermissionMode,
    tool_calls: &[ToolCall],
    projection_events: &mut Vec<Event>,
) -> Result<(Vec<AuthorizedToolCall>, Vec<RuntimeToolResultEnvelope>), SessionError> {
    let Some(sandbox_backend) = runtime.sandbox.clone() else {
        let results = tool_calls
            .iter()
            .map(|call| {
                authorization_failure_result(
                    call,
                    ToolError::PermissionDenied(
                        "sandbox backend is required before tool authorization".to_owned(),
                    ),
                )
            })
            .collect();
        return Ok((Vec::new(), results));
    };
    let authority = PermissionAuthority::builder()
        .with_policy_broker(runtime.permission_broker.clone())
        .with_transient_decision_store(Arc::new(NoopDecisionPersistence))
        .build()
        .map_err(|error| SessionError::Message(error.to_string()))?;
    let ticket_ledger = Arc::new(TicketLedger::default());
    let event_sink = RecordingAuthorizationEventSink::default();
    let authorization = AuthorizationService::new(
        Arc::new(authority),
        sandbox_backend,
        Arc::new(event_sink.clone()),
        ticket_ledger.clone(),
    );
    let auth_context = AuthorizationContext {
        tenant_id: session.tenant_id(),
        session_id: session.session_id(),
        run_id,
        permission_mode,
        interactivity: interactivity_for_permission_mode(permission_mode),
        fallback_policy: FallbackPolicy::DenyAll,
        workspace_root: session.options().workspace_root.clone(),
    };

    let mut authorized = Vec::new();
    let mut failures = Vec::new();
    for call in tool_calls {
        let result = async {
            let tool = runtime.tools.get(&call.tool_name).ok_or_else(|| {
                ToolError::Internal(format!("tool not found: {}", call.tool_name))
            })?;
            let mut tool_ctx = harness_tool::ToolContext {
                tool_use_id: call.tool_use_id,
                run_id,
                session_id: session.session_id(),
                tenant_id: session.tenant_id(),
                correlation_id: CorrelationId::new(),
                agent_id: harness_contracts::AgentId::from_u128(1),
                subagent_depth: 0,
                workspace_root: session.options().workspace_root.clone(),
                sandbox: runtime.sandbox.clone(),
                cap_registry: runtime.cap_registry.clone(),
                redactor: runtime.redactor.clone(),
                interrupt: InterruptToken::new(),
                parent_run: None,
                model: session.turn_model_snapshot(),
                model_config_id: session.turn_model_config_id(),
                actor_source: harness_contracts::PermissionActorSource::ParentRun,
            };
            tool.validate(&call.input, &tool_ctx)
                .await
                .map_err(|error| ToolError::Validation(error.to_string()))?;
            let plan = tool.plan(&call.input, &tool_ctx).await?;
            tool_ctx.tool_use_id = plan.tool_use_id;
            let outcome = authorization
                .authorize_plan(auth_context.clone(), plan.clone())
                .await
                .map_err(authorization_error_to_tool_error)?;
            let consumed = ticket_ledger
                .consume(outcome.ticket.id, &outcome.ticket.claims, Utc::now())
                .map_err(authorization_error_to_tool_error)?;
            let authorized_input = AuthorizedToolInput::new(
                call.input.clone(),
                plan,
                AuthorizedTicketSummary {
                    ticket_id: consumed.id,
                    tenant_id: consumed.claims.tenant_id,
                    session_id: consumed.claims.session_id,
                    run_id: consumed.claims.run_id,
                    tool_use_id: consumed.claims.tool_use_id,
                    tool_name: consumed.claims.tool_name,
                    action_plan_hash: consumed.claims.action_plan_hash,
                    consumed_at: Utc::now(),
                },
            )?;
            Ok::<AuthorizedToolCall, ToolError>(AuthorizedToolCall {
                tool_use_id: call.tool_use_id,
                tool_name: call.tool_name.clone(),
                input: authorized_input,
            })
        }
        .await;
        let auth_events = event_sink.drain().await;
        if !auth_events.is_empty() {
            session.append_events(&auth_events).await?;
            projection_events.extend(auth_events);
        }
        match result {
            Ok(call) => authorized.push(call),
            Err(error) => failures.push(authorization_failure_result(call, error)),
        }
    }
    Ok((authorized, failures))
}

fn interactivity_for_permission_mode(permission_mode: PermissionMode) -> InteractivityLevel {
    if matches!(
        permission_mode,
        PermissionMode::BypassPermissions | PermissionMode::DontAsk
    ) {
        InteractivityLevel::NoInteractive
    } else {
        InteractivityLevel::FullyInteractive
    }
}

fn authorization_error_to_tool_error(error: ExecutionError) -> ToolError {
    match error {
        ExecutionError::PermissionDenied { decision, .. } => {
            ToolError::PermissionDenied(format!("authorization denied: {decision:?}"))
        }
        ExecutionError::SandboxPreflightFailed { reason, .. } => {
            ToolError::PermissionDenied(format!("sandbox preflight failed: {reason}"))
        }
        other => ToolError::Internal(other.to_string()),
    }
}

fn authorization_failure_result(call: &ToolCall, error: ToolError) -> RuntimeToolResultEnvelope {
    RuntimeToolResultEnvelope {
        tool_use_id: call.tool_use_id,
        tool_name: call.tool_name.clone(),
        result: Err(error),
        overflow: None,
        duration: Duration::ZERO,
        progress_emitted: 0,
    }
}

fn run_model_snapshot(runtime: &SessionTurnRuntime) -> Result<RunModelSnapshot, SessionError> {
    let provider_id = runtime.model.provider_id().to_owned();
    let descriptor = runtime
        .model
        .supported_models()
        .into_iter()
        .find(|descriptor| {
            descriptor.provider_id == provider_id && descriptor.model_id == runtime.model_id
        })
        .ok_or_else(|| {
            SessionError::Message(format!(
                "unsupported model id for provider {provider_id}: {}",
                runtime.model_id
            ))
        })?;
    Ok(RunModelSnapshot {
        model_config_id: None,
        provider_id: descriptor.provider_id,
        model_id: descriptor.model_id,
        display_name: descriptor.display_name,
        protocol: runtime.protocol,
        context_window: descriptor.context_window,
        max_output_tokens: descriptor.max_output_tokens,
        conversation_capability: descriptor.conversation_capability,
    })
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
    redactor: Arc<dyn Redactor>,
    permission_mode: PermissionMode,
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
                text_snippet: self
                    .redactor
                    .redact(&message_text(message), &RedactRules::default()),
                tool_use_id: None,
            })
            .collect()
    }

    fn permission_mode(&self) -> PermissionMode {
        self.permission_mode
    }

    fn redacted(&self) -> &dyn Redactor {
        self.redactor.as_ref()
    }

    fn current_tool_descriptor(&self) -> Option<ToolDescriptorView> {
        None
    }
}

struct ChannelToolEventEmitter {
    sender: mpsc::UnboundedSender<Event>,
}

impl ChannelToolEventEmitter {
    fn channel() -> (Arc<Self>, mpsc::UnboundedReceiver<Event>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (Arc::new(Self { sender }), receiver)
    }
}

impl ToolEventEmitter for ChannelToolEventEmitter {
    fn emit(&self, event: Event) {
        let _ignored = self.sender.send(event);
    }
}

#[derive(Clone, Default)]
struct RecordingAuthorizationEventSink {
    events: Arc<Mutex<Vec<Event>>>,
}

impl RecordingAuthorizationEventSink {
    async fn drain(&self) -> Vec<Event> {
        self.events.lock().await.drain(..).collect()
    }
}

#[async_trait]
impl AuthorizationEventSink for RecordingAuthorizationEventSink {
    async fn emit_batch(
        &self,
        _tenant_id: TenantId,
        _session_id: SessionId,
        events: Vec<Event>,
    ) -> Result<(), ExecutionError> {
        self.events.lock().await.extend(events);
        Ok(())
    }
}

fn hook_context(
    session: &Session,
    runtime: &SessionTurnRuntime,
    run_id: RunId,
    messages: &[Message],
    permission_mode: PermissionMode,
) -> HookContext {
    HookContext {
        tenant_id: session.tenant_id(),
        session_id: session.session_id(),
        run_id: Some(run_id),
        turn_index: Some(next_turn_index(messages)),
        correlation_id: CorrelationId::new(),
        causation_id: CausationId::new(),
        trust_level: TrustLevel::UserControlled,
        permission_mode,
        interactivity: InteractivityLevel::NoInteractive,
        at: harness_contracts::now(),
        view: Arc::new(TurnHookView {
            workspace_root: session.options().workspace_root.clone(),
            messages: messages.to_vec(),
            redactor: runtime.redactor.clone(),
            permission_mode,
        }),
        upstream_outcome: None,
        replay_mode: ReplayMode::Live,
    }
}

fn redact_json_strings(value: Value, redactor: &dyn Redactor) -> Value {
    match value {
        Value::String(text) => Value::String(redactor.redact(&text, &RedactRules::default())),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| redact_json_strings(value, redactor))
                .collect(),
        ),
        Value::Object(entries) => Value::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key, redact_json_strings(value, redactor)))
                .collect(),
        ),
        value => value,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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

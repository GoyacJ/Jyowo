use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use futures::{stream, StreamExt};
use harness_context::ContextSessionView;
use harness_contracts::{
    ArtifactCreatedEvent, ArtifactSource, ArtifactStatus, AssistantDeltaProducedEvent,
    AssistantMessageCompletedEvent, BlobRef, BudgetKind, CausationId, ContextPatchLifecycle,
    ContextPatchRequest, ContextPatchSinkCap, ContextPatchSource, DecidedBy, Decision, DecisionId,
    DeltaChunk, DenyReason, EndReason, Event, EventId, ExecFingerprint, FallbackPolicy,
    HookContextPatchEvent, HookEventKind, HookFailedEvent, HookOutcomeInconsistentEvent,
    HookOutcomeSummary, HookPermissionConflictEvent, HookReturnedUnsupportedEvent,
    HookRewroteInputEvent, HookTriggeredEvent, InteractivityLevel, Message, MessageContent,
    MessageId, MessageMetadata, MessagePart, MessageRole, ModelError, ModelRef, PermissionMode,
    PermissionRequestSuppressedEvent, PermissionRequestedEvent, PermissionResolvedEvent,
    PricingSnapshotId, RedactRules, Redactor, RequestId, RunEndedEvent, RunId, RunStartedEvent,
    SessionId, StopReason, SuppressionReason, TeamId, TenantId, ToolDescriptor, ToolError,
    ToolErrorPayload, ToolResult, ToolResultPart, ToolUseApprovedEvent, ToolUseCompletedEvent,
    ToolUseDeniedEvent, ToolUseFailedEvent, ToolUseId, ToolUseRequestedEvent, TrustLevel,
    TurnInput, UsageAccumulatedEvent, UsageSnapshot,
};
use harness_hook::{
    DispatchResult, HookContext, HookEvent, HookFailureCause, HookMessageView, HookOutcome,
    HookPermissionConflict, HookPermissionOverride, HookSessionView, ReplayMode,
    ToolDescriptorView, ToolErrorView,
};
use harness_journal::EventStore;
use harness_model::{
    apply_before_request_middlewares, apply_request_end_middlewares, wrap_stream_with_middlewares,
    BillingMode, InferContext, ModelModality, ModelRequest, PricingSnapshotResolveContext,
    PricingSource, Ratio, StreamAggregate, StreamAggregator,
};
use harness_observability::{DefaultRedactor, Span, SpanAttributes};
use harness_permission::{
    canonical_permission_fingerprint, PermissionBroker, PermissionContext, PermissionRequest,
    PersistedDecision, RuleSnapshot,
};
use harness_tool::{
    InterruptToken, OrchestratorContext, ToolCall, ToolEventEmitter, ToolOrchestrator,
    ToolResultEnvelope as RuntimeToolResultEnvelope,
};
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};

use crate::{
    end_reason_for_interrupt, result_inject, Engine, EngineError, EventStream, RunContext,
    SessionHandle,
};

pub(crate) async fn run_turn(
    engine: &Engine,
    session: SessionHandle,
    input: TurnInput,
    ctx: RunContext,
) -> Result<EventStream, EngineError> {
    if session.tenant_id != ctx.tenant_id || session.session_id != ctx.session_id {
        return Err(engine_error(
            "context mismatch between session handle and run context",
        ));
    }
    let _span = TurnSpanGuard::new(engine);

    let mut emitted = Vec::new();
    let client_message_id = input
        .metadata
        .get("clientMessageId")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let correlation_id = ctx.correlation_id;
    let run_started = Event::RunStarted(RunStartedEvent {
        run_id: ctx.run_id,
        session_id: session.session_id,
        tenant_id: session.tenant_id,
        parent_run_id: ctx.parent_run_id,
        input: input.clone(),
        snapshot_id: ctx.config_snapshot_id,
        effective_config_hash: ctx.effective_config_hash,
        started_at: harness_contracts::now(),
        correlation_id,
    });
    append(
        engine,
        session.tenant_id,
        session.session_id,
        &mut emitted,
        vec![run_started],
    )
    .await?;
    let mut usage = UsageSnapshot::default();
    let started_at = Instant::now();
    let mut dispatched_tool_calls = 0_u64;

    if append_interrupt_if_cancelled(engine, &session, &mut emitted, &ctx, usage.clone()).await? {
        return Ok(Box::pin(stream::iter(emitted)));
    }

    dispatch_user_prompt_hook(engine, &session, &mut emitted, &ctx, &input, &[]).await?;

    if append_interrupt_if_cancelled(engine, &session, &mut emitted, &ctx, usage.clone()).await? {
        return Ok(Box::pin(stream::iter(emitted)));
    }

    let mut working_messages = ctx.context_seed.clone();
    working_messages.extend(collected_messages(&emitted));
    let mut next_input = input;
    let mut grace_active = false;
    let mut iterations = 0;
    let mut appended_user_messages = HashSet::new();

    loop {
        if let Some(kind) = budget_exhausted(
            ctx.budget_limits.as_ref(),
            &usage,
            dispatched_tool_calls,
            started_at.elapsed(),
        ) {
            append_run_end(
                engine,
                &session,
                &mut emitted,
                ctx.run_id,
                EndReason::BudgetExhausted(kind),
                usage.clone(),
            )
            .await?;
            return Ok(Box::pin(stream::iter(emitted)));
        }

        if iterations >= engine.max_iterations {
            append_run_end(
                engine,
                &session,
                &mut emitted,
                ctx.run_id,
                EndReason::MaxIterationsReached,
                usage.clone(),
            )
            .await?;
            return Ok(Box::pin(stream::iter(emitted)));
        }

        if !grace_active && iterations + 1 >= engine.max_iterations {
            let grace = Event::GraceCallTriggered(harness_contracts::GraceCallTriggeredEvent {
                run_id: ctx.run_id,
                session_id: session.session_id,
                tenant_id: session.tenant_id,
                current_iteration: iterations,
                max_iterations: engine.max_iterations,
                usage_snapshot: usage.clone(),
                at: harness_contracts::now(),
                correlation_id,
            });
            append(
                engine,
                session.tenant_id,
                session.session_id,
                &mut emitted,
                vec![grace],
            )
            .await?;
            grace_active = true;
        }

        if append_interrupt_if_cancelled(engine, &session, &mut emitted, &ctx, usage.clone())
            .await?
        {
            return Ok(Box::pin(stream::iter(emitted)));
        }

        apply_steering(
            engine,
            &session,
            &mut emitted,
            &ctx,
            &mut working_messages,
            &mut next_input,
        )
        .await?;

        let prompt_view = TurnContextView {
            tenant_id: session.tenant_id,
            session_id: session.session_id,
            user_id: ctx.user_id.clone(),
            team_id: ctx.team_id,
            system: engine.system_prompt.clone(),
            messages: working_messages.clone(),
            tools: prompt_visible_tools_for_model(engine),
        };
        let assembled = engine
            .context
            .assemble(&prompt_view, &next_input)
            .await
            .map_err(engine_error)?;
        if !assembled.events.is_empty() {
            append(
                engine,
                session.tenant_id,
                session.session_id,
                &mut emitted,
                assembled.events.clone(),
            )
            .await?;
        }
        validate_model_input_modalities(
            &assembled.messages,
            &engine
                .model_snapshot
                .conversation_capability
                .input_modalities,
        )?;

        let mut request = ModelRequest {
            model_id: engine.model_id.clone(),
            messages: assembled.messages,
            tools: model_request_tools(engine, assembled.tools_snapshot),
            system: assembled.system,
            temperature: None,
            max_tokens: None,
            stream: true,
            cache_breakpoints: assembled.cache_breakpoints,
            protocol: engine.protocol,
            extra: model_extra_with_relay_logical_call_key(
                engine.model_extra.clone(),
                ctx.run_id,
                iterations,
            ),
        };
        let mut infer_ctx = InferContext::for_test();
        infer_ctx.tenant_id = session.tenant_id;
        infer_ctx.session_id = Some(session.session_id);
        infer_ctx.run_id = Some(ctx.run_id);
        infer_ctx.middlewares = engine.model_middlewares.clone();
        infer_ctx.blob_store = engine.blob_store.clone();

        let pre_model_hook_events = dispatch_pre_model_hooks(
            engine,
            &session,
            &ctx,
            &mut request,
            &infer_ctx,
            &working_messages,
        )
        .await?;
        if !pre_model_hook_events.is_empty() {
            append(
                engine,
                session.tenant_id,
                session.session_id,
                &mut emitted,
                pre_model_hook_events,
            )
            .await?;
        }

        if append_interrupt_if_cancelled(engine, &session, &mut emitted, &ctx, usage.clone())
            .await?
        {
            return Ok(Box::pin(stream::iter(emitted)));
        }

        if let Err(error) = apply_before_request_middlewares(&mut request, &mut infer_ctx).await {
            finalize_run_error(engine, &session, &mut emitted, ctx.run_id, &error).await?;
            return Err(engine_error(error));
        }

        append_user_message_if_needed(
            engine,
            &session,
            &ctx,
            &mut emitted,
            &next_input,
            &mut appended_user_messages,
            client_message_id.as_deref(),
        )
        .await?;

        let mut model_call_started = Instant::now();
        let mut stream = match engine.model.infer(request.clone(), infer_ctx.clone()).await {
            Ok(stream) => stream,
            Err(ModelError::ContextTooLong { tokens, max }) => {
                record_model_infer(
                    engine,
                    model_call_started.elapsed(),
                    &UsageSnapshot::default(),
                );
                record_model_error(engine, "context_too_long");
                let compacted = engine
                    .context
                    .emergency_compact_prompt(
                        session.tenant_id,
                        session.session_id,
                        request.system.clone(),
                        request.tools.clone().unwrap_or_default(),
                        request.messages.clone(),
                        tokens as u64,
                        max as u64,
                    )
                    .await
                    .map_err(engine_error)?;
                if !compacted.prompt.events.is_empty() {
                    append(
                        engine,
                        session.tenant_id,
                        session.session_id,
                        &mut emitted,
                        compacted.prompt.events.clone(),
                    )
                    .await?;
                }
                request = ModelRequest {
                    model_id: engine.model_id.clone(),
                    messages: compacted.prompt.messages,
                    tools: model_request_tools(engine, compacted.prompt.tools_snapshot),
                    system: compacted.prompt.system,
                    temperature: None,
                    max_tokens: None,
                    stream: true,
                    cache_breakpoints: compacted.prompt.cache_breakpoints,
                    protocol: engine.protocol,
                    extra: model_extra_with_relay_logical_call_key(
                        engine.model_extra.clone(),
                        ctx.run_id,
                        iterations,
                    ),
                };
                if let Err(error) =
                    apply_before_request_middlewares(&mut request, &mut infer_ctx).await
                {
                    finalize_run_error(engine, &session, &mut emitted, ctx.run_id, &error).await?;
                    return Err(engine_error(error));
                }
                model_call_started = Instant::now();
                match engine.model.infer(request.clone(), infer_ctx.clone()).await {
                    Ok(stream) => stream,
                    Err(error) => {
                        record_model_infer(
                            engine,
                            model_call_started.elapsed(),
                            &UsageSnapshot::default(),
                        );
                        record_model_error(engine, model_error_class(&error));
                        let post_api_hook_events = dispatch_post_api_hook(
                            engine,
                            &session,
                            &ctx,
                            infer_ctx.request_id,
                            500,
                            &working_messages,
                        )
                        .await?;
                        if !post_api_hook_events.is_empty() {
                            append(
                                engine,
                                session.tenant_id,
                                session.session_id,
                                &mut emitted,
                                post_api_hook_events,
                            )
                            .await?;
                        }
                        finalize_run_error(engine, &session, &mut emitted, ctx.run_id, &error)
                            .await?;
                        return Err(engine_error(error));
                    }
                }
            }
            Err(error) => {
                record_model_infer(
                    engine,
                    model_call_started.elapsed(),
                    &UsageSnapshot::default(),
                );
                record_model_error(engine, model_error_class(&error));
                let post_api_hook_events = dispatch_post_api_hook(
                    engine,
                    &session,
                    &ctx,
                    infer_ctx.request_id,
                    500,
                    &working_messages,
                )
                .await?;
                if !post_api_hook_events.is_empty() {
                    append(
                        engine,
                        session.tenant_id,
                        session.session_id,
                        &mut emitted,
                        post_api_hook_events,
                    )
                    .await?;
                }
                finalize_run_error(engine, &session, &mut emitted, ctx.run_id, &error).await?;
                return Err(engine_error(error));
            }
        };
        stream = wrap_stream_with_middlewares(stream, &infer_ctx);

        let assistant_message_id = MessageId::new();
        let mut assistant_text = String::new();
        let mut tool_calls = Vec::new();
        let mut stream_aggregator = StreamAggregator::default();
        let mut stop_reason = StopReason::EndTurn;
        let mut model_call_usage = UsageSnapshot::default();

        while let Some(event) = stream.next().await {
            for aggregate in stream_aggregator.push(event) {
                match aggregate {
                    StreamAggregate::MessageStart { usage: start_usage } => {
                        add_usage(&mut usage, &start_usage);
                        add_usage(&mut model_call_usage, &start_usage);
                    }
                    StreamAggregate::TextChunk { text } => {
                        assistant_text.push_str(&text);
                        append(
                            engine,
                            session.tenant_id,
                            session.session_id,
                            &mut emitted,
                            vec![Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                                run_id: ctx.run_id,
                                message_id: assistant_message_id,
                                delta: DeltaChunk::Text(text),
                                at: harness_contracts::now(),
                            })],
                        )
                        .await?;
                    }
                    StreamAggregate::ThinkingChunk { thinking } => {
                        let has_private_thinking_signal = thinking
                            .text
                            .as_deref()
                            .is_some_and(|text| !text.is_empty())
                            || thinking.provider_native.is_some()
                            || thinking.signature.is_some();
                        if !has_private_thinking_signal {
                            continue;
                        }
                        append(
                            engine,
                            session.tenant_id,
                            session.session_id,
                            &mut emitted,
                            vec![Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                                run_id: ctx.run_id,
                                message_id: assistant_message_id,
                                delta: DeltaChunk::Thought(harness_contracts::ThoughtChunk {
                                    text: None,
                                    provider_id: "harness_model".to_owned(),
                                    provider_native: None,
                                    signature: None,
                                }),
                                at: harness_contracts::now(),
                            })],
                        )
                        .await?;
                    }
                    StreamAggregate::ToolCallReady {
                        tool_use_id,
                        tool_name,
                        input,
                    } => {
                        tool_calls.push(ToolCall {
                            tool_use_id,
                            tool_name,
                            input,
                        });
                        append(
                            engine,
                            session.tenant_id,
                            session.session_id,
                            &mut emitted,
                            vec![Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                                run_id: ctx.run_id,
                                message_id: assistant_message_id,
                                delta: DeltaChunk::ToolUseEnd { tool_use_id },
                                at: harness_contracts::now(),
                            })],
                        )
                        .await?;
                    }
                    StreamAggregate::ReasoningSummaryChunk { summary } => {
                        if summary.text.is_empty() {
                            continue;
                        }
                        append(
                            engine,
                            session.tenant_id,
                            session.session_id,
                            &mut emitted,
                            vec![Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                                run_id: ctx.run_id,
                                message_id: assistant_message_id,
                                delta: DeltaChunk::ReasoningSummary(
                                    harness_contracts::ReasoningSummaryChunk {
                                        text: summary.text,
                                        provider_id: "harness_model".to_owned(),
                                        provider_native: None,
                                    },
                                ),
                                at: harness_contracts::now(),
                            })],
                        )
                        .await?;
                    }
                    StreamAggregate::ToolUseStart {
                        tool_use_id,
                        tool_name,
                    } => {
                        append(
                            engine,
                            session.tenant_id,
                            session.session_id,
                            &mut emitted,
                            vec![Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                                run_id: ctx.run_id,
                                message_id: assistant_message_id,
                                delta: DeltaChunk::ToolUseStart {
                                    tool_use_id,
                                    tool_name,
                                },
                                at: harness_contracts::now(),
                            })],
                        )
                        .await?;
                    }
                    StreamAggregate::ToolUseInputDelta { tool_use_id, delta } => {
                        append(
                            engine,
                            session.tenant_id,
                            session.session_id,
                            &mut emitted,
                            vec![Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                                run_id: ctx.run_id,
                                message_id: assistant_message_id,
                                delta: DeltaChunk::ToolUseInputDelta { tool_use_id, delta },
                                at: harness_contracts::now(),
                            })],
                        )
                        .await?;
                    }
                    StreamAggregate::MessageDelta {
                        stop_reason: next_stop_reason,
                        usage_delta,
                    } => {
                        add_usage(&mut usage, &usage_delta);
                        add_usage(&mut model_call_usage, &usage_delta);
                        if let Some(next_stop_reason) = next_stop_reason {
                            stop_reason = next_stop_reason;
                        }
                    }
                    StreamAggregate::StreamError { error, class, .. } => {
                        record_model_infer(engine, model_call_started.elapsed(), &model_call_usage);
                        record_model_stream_error(engine, &format!("{class:?}"));
                        let message = format!("model stream error ({class:?}): {error}");
                        finalize_run_error(engine, &session, &mut emitted, ctx.run_id, &message)
                            .await?;
                        return Err(engine_error(message));
                    }
                    StreamAggregate::MessageDone => {}
                }
            }
            if append_interrupt_if_cancelled(engine, &session, &mut emitted, &ctx, usage.clone())
                .await?
            {
                return Ok(Box::pin(stream::iter(emitted)));
            }
        }
        record_model_infer(engine, model_call_started.elapsed(), &model_call_usage);

        if let Err(error) = apply_request_end_middlewares(&usage, &infer_ctx).await {
            finalize_run_error(engine, &session, &mut emitted, ctx.run_id, &error).await?;
            return Err(engine_error(error));
        }
        let pricing_snapshot_id = pricing_snapshot_for_model(engine, &session, &ctx).await;
        let mut priced_model_call_usage = model_call_usage.clone();
        if let Some(cost_micros) = cost_micros_for_usage(
            engine,
            &priced_model_call_usage,
            pricing_snapshot_id.as_ref(),
        ) {
            priced_model_call_usage.cost_micros = cost_micros;
            usage.cost_micros = usage.cost_micros.saturating_add(cost_micros);
        }
        append_usage_accumulated(
            engine,
            &session,
            &ctx,
            &mut emitted,
            priced_model_call_usage.clone(),
            pricing_snapshot_id.clone(),
        )
        .await?;

        let post_model_hook_events = dispatch_post_model_hooks(
            engine,
            &session,
            &ctx,
            infer_ctx.request_id,
            &usage,
            &working_messages,
        )
        .await?;
        if !post_model_hook_events.is_empty() {
            append(
                engine,
                session.tenant_id,
                session.session_id,
                &mut emitted,
                post_model_hook_events,
            )
            .await?;
        }

        if append_interrupt_if_cancelled(engine, &session, &mut emitted, &ctx, usage.clone())
            .await?
        {
            return Ok(Box::pin(stream::iter(emitted)));
        }

        working_messages.push(next_input.message.clone());

        if tool_calls.is_empty() {
            append(
                engine,
                session.tenant_id,
                session.session_id,
                &mut emitted,
                vec![Event::AssistantMessageCompleted(
                    AssistantMessageCompletedEvent {
                        run_id: ctx.run_id,
                        message_id: assistant_message_id,
                        content: MessageContent::Text(assistant_text),
                        tool_uses: Vec::new(),
                        usage: usage.clone(),
                        pricing_snapshot_id: pricing_snapshot_id.clone(),
                        stop_reason,
                        at: harness_contracts::now(),
                    },
                )],
            )
            .await?;
            if let Some(kind) = budget_exhausted(
                ctx.budget_limits.as_ref(),
                &usage,
                dispatched_tool_calls,
                started_at.elapsed(),
            ) {
                append_run_end(
                    engine,
                    &session,
                    &mut emitted,
                    ctx.run_id,
                    EndReason::BudgetExhausted(kind),
                    usage,
                )
                .await?;
                return Ok(Box::pin(stream::iter(emitted)));
            }
            append_run_end(
                engine,
                &session,
                &mut emitted,
                ctx.run_id,
                EndReason::Completed,
                usage,
            )
            .await?;
            return Ok(Box::pin(stream::iter(emitted)));
        }

        let pre_tool_application =
            apply_pre_tool_use_hooks(engine, &session, &ctx, &tool_calls, &working_messages)
                .await?;
        if !pre_tool_application.events.is_empty() {
            append(
                engine,
                session.tenant_id,
                session.session_id,
                &mut emitted,
                pre_tool_application.events,
            )
            .await?;
        }
        if let Some(reason) = pre_tool_application.blocked_reason {
            append_run_end(
                engine,
                &session,
                &mut emitted,
                ctx.run_id,
                EndReason::Error(format!("tool use blocked by hook: {reason}")),
                usage,
            )
            .await?;
            return Ok(Box::pin(stream::iter(emitted)));
        }
        tool_calls = pre_tool_application.calls;
        let permission_overrides = pre_tool_application.permission_overrides;

        let assistant_tool_message = result_inject::assistant_tool_message(
            assistant_message_id,
            assistant_text.clone(),
            &tool_calls,
        );
        append(
            engine,
            session.tenant_id,
            session.session_id,
            &mut emitted,
            vec![Event::AssistantMessageCompleted(
                AssistantMessageCompletedEvent {
                    run_id: ctx.run_id,
                    message_id: assistant_message_id,
                    content: result_inject::assistant_tool_content(assistant_text, &tool_calls),
                    tool_uses: tool_calls
                        .iter()
                        .map(|call| harness_contracts::ToolUseSummary {
                            tool_use_id: call.tool_use_id,
                            tool_name: call.tool_name.clone(),
                        })
                        .collect(),
                    usage: usage.clone(),
                    pricing_snapshot_id: pricing_snapshot_id.clone(),
                    stop_reason,
                    at: harness_contracts::now(),
                },
            )],
        )
        .await?;
        working_messages.push(assistant_tool_message);

        if let Some(kind) = budget_exhausted(
            ctx.budget_limits.as_ref(),
            &usage,
            dispatched_tool_calls,
            started_at.elapsed(),
        ) {
            append_run_end(
                engine,
                &session,
                &mut emitted,
                ctx.run_id,
                EndReason::BudgetExhausted(kind),
                usage,
            )
            .await?;
            return Ok(Box::pin(stream::iter(emitted)));
        }

        if append_interrupt_if_cancelled(engine, &session, &mut emitted, &ctx, usage.clone())
            .await?
        {
            return Ok(Box::pin(stream::iter(emitted)));
        }

        if grace_active {
            append_run_end(
                engine,
                &session,
                &mut emitted,
                ctx.run_id,
                EndReason::MaxIterationsReached,
                usage,
            )
            .await?;
            return Ok(Box::pin(stream::iter(emitted)));
        }

        for call in &tool_calls {
            let Some(descriptor) = engine.tools.descriptor(&call.tool_name) else {
                let message = format!("tool descriptor missing: {}", call.tool_name);
                finalize_run_error(engine, &session, &mut emitted, ctx.run_id, &message).await?;
                return Err(engine_error(message));
            };
            append(
                engine,
                session.tenant_id,
                session.session_id,
                &mut emitted,
                vec![Event::ToolUseRequested(ToolUseRequestedEvent {
                    run_id: ctx.run_id,
                    tool_use_id: call.tool_use_id,
                    tool_name: call.tool_name.clone(),
                    input: call.input.clone(),
                    properties: descriptor.properties.clone(),
                    causation_id: EventId::new(),
                    at: harness_contracts::now(),
                })],
            )
            .await?;
        }

        let permission_recorder = Arc::new(RecordingPermissionBroker::new(
            engine.permission_broker.clone(),
            permission_overrides,
            engine.event_store.clone(),
            ctx.run_id,
            ctx.interactivity,
        ));
        let (tool_event_emitter, mut tool_event_receiver) = ChannelToolEventEmitter::channel();
        let tool_interrupt = InterruptToken::new();
        let orchestrator = ToolOrchestrator::default();
        let mut flushed_permission_requested_events = 0;
        let mut flushed_permission_records = 0;
        let mut dispatch = Box::pin(
            orchestrator.dispatch(
                tool_calls.clone(),
                OrchestratorContext {
                    pool: engine.tools.clone(),
                    tool_context: harness_tool::ToolContext {
                        tool_use_id: ToolUseId::new(),
                        run_id: ctx.run_id,
                        session_id: session.session_id,
                        tenant_id: session.tenant_id,
                        correlation_id,
                        agent_id: harness_contracts::AgentId::from_u128(1),
                        subagent_depth: ctx.subagent_depth,
                        workspace_root: engine.workspace_root.clone(),
                        sandbox: engine.sandbox.clone(),
                        permission_broker: permission_recorder.clone(),
                        cap_registry: engine.cap_registry.clone(),
                        redactor: engine
                            .observer
                            .as_ref()
                            .map(|observer| Arc::clone(&observer.redactor))
                            .unwrap_or_else(|| Arc::new(DefaultRedactor::default())),
                        interrupt: tool_interrupt.clone(),
                        parent_run: ctx
                            .parent_run_id
                            .map(|run_id| harness_tool::ParentRunHandle {
                                run_id,
                                session_id: session.session_id,
                            }),
                    },
                    permission_context: permission_context(&session, &ctx),
                    blob_store: engine.blob_store.clone(),
                    event_emitter: tool_event_emitter,
                },
            ),
        );
        let mut tool_results = loop {
            tokio::select! {
                results = &mut dispatch => break results,
                cause = ctx.cancellation.cancelled() => {
                while let Ok(event) = tool_event_receiver.try_recv() {
                    flush_engine_permission_events(
                        engine,
                        &session,
                        &ctx,
                        &mut emitted,
                        permission_recorder.as_ref(),
                        &mut flushed_permission_requested_events,
                        &mut flushed_permission_records,
                        &working_messages,
                    )
                    .await?;
                    append(
                        engine,
                        session.tenant_id,
                        session.session_id,
                        &mut emitted,
                        vec![event],
                    )
                    .await?;
                }
                flush_engine_permission_events(
                    engine,
                    &session,
                    &ctx,
                    &mut emitted,
                    permission_recorder.as_ref(),
                    &mut flushed_permission_requested_events,
                    &mut flushed_permission_records,
                    &working_messages,
                )
                .await?;
                tool_interrupt.interrupt();
                let interrupt_grace = tokio::time::sleep(Duration::from_secs(5));
                tokio::pin!(interrupt_grace);
                loop {
                    tokio::select! {
                        results = &mut dispatch => {
                            drop(results);
                            break;
                        }
                        Some(event) = tool_event_receiver.recv() => {
                            flush_engine_permission_events(
                                engine,
                                &session,
                                &ctx,
                                &mut emitted,
                                permission_recorder.as_ref(),
                                &mut flushed_permission_requested_events,
                                &mut flushed_permission_records,
                                &working_messages,
                            )
                            .await?;
                            append(
                                engine,
                                session.tenant_id,
                                session.session_id,
                                &mut emitted,
                                vec![event],
                            )
                            .await?;
                        }
                        _ = &mut interrupt_grace => break,
                    }
                }
                while let Ok(event) = tool_event_receiver.try_recv() {
                    flush_engine_permission_events(
                        engine,
                        &session,
                        &ctx,
                        &mut emitted,
                        permission_recorder.as_ref(),
                        &mut flushed_permission_requested_events,
                        &mut flushed_permission_records,
                        &working_messages,
                    )
                    .await?;
                    append(
                        engine,
                        session.tenant_id,
                        session.session_id,
                        &mut emitted,
                        vec![event],
                    )
                    .await?;
                }
                flush_engine_permission_events(
                    engine,
                    &session,
                    &ctx,
                    &mut emitted,
                    permission_recorder.as_ref(),
                    &mut flushed_permission_requested_events,
                    &mut flushed_permission_records,
                    &working_messages,
                )
                .await?;
                append_run_end(
                    engine,
                    &session,
                    &mut emitted,
                    ctx.run_id,
                    end_reason_for_interrupt(cause),
                    usage,
                )
                .await?;
                return Ok(Box::pin(stream::iter(emitted)));
                }
                Some(event) = tool_event_receiver.recv() => {
                    flush_engine_permission_events(
                        engine,
                        &session,
                        &ctx,
                        &mut emitted,
                        permission_recorder.as_ref(),
                        &mut flushed_permission_requested_events,
                        &mut flushed_permission_records,
                        &working_messages,
                    )
                    .await?;
                    append(
                        engine,
                        session.tenant_id,
                        session.session_id,
                        &mut emitted,
                        vec![event],
                    )
                    .await?;
                }
            };
        };
        while let Ok(event) = tool_event_receiver.try_recv() {
            flush_engine_permission_events(
                engine,
                &session,
                &ctx,
                &mut emitted,
                permission_recorder.as_ref(),
                &mut flushed_permission_requested_events,
                &mut flushed_permission_records,
                &working_messages,
            )
            .await?;
            append(
                engine,
                session.tenant_id,
                session.session_id,
                &mut emitted,
                vec![event],
            )
            .await?;
        }
        flush_engine_permission_events(
            engine,
            &session,
            &ctx,
            &mut emitted,
            permission_recorder.as_ref(),
            &mut flushed_permission_requested_events,
            &mut flushed_permission_records,
            &working_messages,
        )
        .await?;

        let mut post_tool_events = Vec::new();
        post_tool_events.extend(
            apply_post_tool_hooks(engine, &session, &ctx, &mut tool_results, &working_messages)
                .await?,
        );
        for result in &tool_results {
            post_tool_events.extend(tool_result_events(result, session.session_id, ctx.run_id));
        }
        append(
            engine,
            session.tenant_id,
            session.session_id,
            &mut emitted,
            post_tool_events,
        )
        .await?;
        dispatched_tool_calls =
            dispatched_tool_calls.saturating_add(tool_calls.len().try_into().unwrap_or(u64::MAX));
        usage.tool_calls = dispatched_tool_calls;
        if let Some(kind) = budget_exhausted(
            ctx.budget_limits.as_ref(),
            &usage,
            dispatched_tool_calls,
            started_at.elapsed(),
        ) {
            append_run_end(
                engine,
                &session,
                &mut emitted,
                ctx.run_id,
                EndReason::BudgetExhausted(kind),
                usage,
            )
            .await?;
            return Ok(Box::pin(stream::iter(emitted)));
        }

        let reinjected_messages = result_inject::tool_result_messages(&tool_results);
        let mut context_messages = working_messages.clone();
        context_messages.extend(reinjected_messages.clone());
        let post_tool_prompt_view = TurnContextView {
            tenant_id: session.tenant_id,
            session_id: session.session_id,
            user_id: ctx.user_id.clone(),
            team_id: ctx.team_id,
            system: engine.system_prompt.clone(),
            messages: context_messages,
            tools: prompt_visible_tools_for_model(engine),
        };
        if let Err(error) = engine
            .context
            .after_turn(&post_tool_prompt_view, &context_tool_results(&tool_results))
            .await
        {
            finalize_run_error(engine, &session, &mut emitted, ctx.run_id, &error).await?;
            return Err(engine_error(error));
        }

        let mut reinjected = reinjected_messages;
        let next_message = reinjected
            .pop()
            .ok_or_else(|| engine_error("tool dispatch produced no results"))?;
        working_messages.extend(reinjected);
        next_input = result_inject::turn_input_from_message(next_message);
        iterations = iterations.saturating_add(1);
    }
}

struct TurnSpanGuard(Option<Box<dyn Span>>);

impl TurnSpanGuard {
    fn new(engine: &Engine) -> Self {
        Self(
            engine
                .tracer
                .as_ref()
                .map(|tracer| tracer.start_span("engine.run_turn", SpanAttributes::default())),
        )
    }
}

impl Drop for TurnSpanGuard {
    fn drop(&mut self) {
        if let Some(span) = self.0.take() {
            span.end();
        }
    }
}

async fn dispatch_user_prompt_hook(
    engine: &Engine,
    session: &SessionHandle,
    emitted: &mut Vec<Event>,
    ctx: &RunContext,
    input: &TurnInput,
    messages: &[Message],
) -> Result<(), EngineError> {
    let redactor = hook_redactor(engine);
    let result = engine
        .hooks
        .dispatch(
            HookEvent::UserPromptSubmit {
                run_id: ctx.run_id,
                input: redact_json_strings(
                    json!({ "prompt": message_text(&input.message) }),
                    redactor.as_ref(),
                ),
            },
            hook_context(engine, session, ctx, messages),
        )
        .await
        .map_err(engine_error)?;
    append(
        engine,
        session.tenant_id,
        session.session_id,
        emitted,
        hook_events(HookEventKind::UserPromptSubmit, &result, None),
    )
    .await?;
    if let HookOutcome::Block { reason } = result.final_outcome {
        return Err(engine_error(format!("run blocked by hook: {reason}")));
    }
    Ok(())
}

struct PreToolUseApplication {
    calls: Vec<ToolCall>,
    permission_overrides: Vec<HookPermissionDecisionOverride>,
    events: Vec<Event>,
    blocked_reason: Option<String>,
}

#[derive(Clone)]
struct HookPermissionDecisionOverride {
    tool_use_id: ToolUseId,
    override_decision: HookPermissionOverride,
    conflict: Option<HookPermissionConflict>,
}

async fn apply_pre_tool_use_hooks(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
    calls: &[ToolCall],
    messages: &[Message],
) -> Result<PreToolUseApplication, EngineError> {
    let mut staged_calls = Vec::with_capacity(calls.len());
    let mut permission_overrides = Vec::new();
    let mut events = Vec::new();

    for call in calls {
        let redactor = hook_redactor(engine);
        let result = engine
            .hooks
            .dispatch(
                HookEvent::PreToolUse {
                    tool_use_id: call.tool_use_id,
                    tool_name: call.tool_name.clone(),
                    input: redact_json_strings(call.input.clone(), redactor.as_ref()),
                },
                hook_context(engine, session, ctx, messages),
            )
            .await
            .map_err(engine_error)?;
        events.extend(hook_events(HookEventKind::PreToolUse, &result, None));

        match result.final_outcome {
            HookOutcome::Continue => staged_calls.push(call.clone()),
            HookOutcome::Block { reason } => {
                return Ok(PreToolUseApplication {
                    calls: calls.to_vec(),
                    permission_overrides,
                    events,
                    blocked_reason: Some(reason),
                });
            }
            HookOutcome::PreToolUse(outcome) => {
                if let Some(reason) = outcome.block {
                    return Ok(PreToolUseApplication {
                        calls: calls.to_vec(),
                        permission_overrides,
                        events,
                        blocked_reason: Some(reason),
                    });
                }
                let mut next = call.clone();
                if let Some(input) = outcome.rewrite_input {
                    events.push(Event::HookRewroteInput(HookRewroteInputEvent {
                        tool_use_id: call.tool_use_id,
                        before_hash: hash_value(&call.input),
                        after_hash: hash_value(&input),
                        causation_id: EventId::new(),
                        at: harness_contracts::now(),
                    }));
                    next.input = input;
                }
                if let Some(override_decision) = result.permission_override.clone() {
                    permission_overrides.push(HookPermissionDecisionOverride {
                        tool_use_id: call.tool_use_id,
                        override_decision,
                        conflict: result.permission_conflict.clone(),
                    });
                }
                if let Some(context) = outcome.additional_context {
                    push_hook_context_patch(
                        engine,
                        session,
                        ctx,
                        HookEventKind::PreToolUse,
                        "pre-tool-use",
                        &context.content,
                    )
                    .await?;
                    events.push(context_patch_event(
                        HookEventKind::PreToolUse,
                        "pre-tool-use",
                        &context.content,
                    ));
                }
                staged_calls.push(next);
            }
            _ => staged_calls.push(call.clone()),
        }
    }

    Ok(PreToolUseApplication {
        calls: staged_calls,
        permission_overrides,
        events,
        blocked_reason: None,
    })
}

async fn apply_post_tool_hooks(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
    results: &mut [RuntimeToolResultEnvelope],
    messages: &[Message],
) -> Result<Vec<Event>, EngineError> {
    let mut events = Vec::new();
    let redactor = hook_redactor(engine);
    for result in results {
        match &mut result.result {
            Ok(tool_result) => {
                let hook_tool_result = redact_tool_result(tool_result.clone(), redactor.as_ref());
                if let Some(raw) = terminal_bytes(&hook_tool_result) {
                    let dispatch = engine
                        .hooks
                        .dispatch(
                            HookEvent::TransformTerminalOutput {
                                tool_use_id: result.tool_use_id,
                                raw,
                            },
                            hook_context(engine, session, ctx, messages),
                        )
                        .await
                        .map_err(engine_error)?;
                    events.extend(hook_events(
                        HookEventKind::TransformTerminalOutput,
                        &dispatch,
                        None,
                    ));
                    if let HookOutcome::Transform(value) = dispatch.final_outcome {
                        *tool_result = tool_result_from_transform(value);
                    }
                }

                let dispatch = engine
                    .hooks
                    .dispatch(
                        HookEvent::TransformToolResult {
                            tool_use_id: result.tool_use_id,
                            result: redact_tool_result(tool_result.clone(), redactor.as_ref()),
                        },
                        hook_context(engine, session, ctx, messages),
                    )
                    .await
                    .map_err(engine_error)?;
                events.extend(hook_events(
                    HookEventKind::TransformToolResult,
                    &dispatch,
                    None,
                ));
                if let HookOutcome::Transform(value) = dispatch.final_outcome {
                    *tool_result = tool_result_from_transform(value);
                }

                let dispatch = engine
                    .hooks
                    .dispatch(
                        HookEvent::PostToolUse {
                            tool_use_id: result.tool_use_id,
                            result: redact_tool_result(tool_result.clone(), redactor.as_ref()),
                        },
                        hook_context(engine, session, ctx, messages),
                    )
                    .await
                    .map_err(engine_error)?;
                events.extend(hook_events(HookEventKind::PostToolUse, &dispatch, None));
                if let HookOutcome::AddContext(context) = dispatch.final_outcome {
                    push_hook_context_patch(
                        engine,
                        session,
                        ctx,
                        HookEventKind::PostToolUse,
                        "post-tool-use",
                        &context.content,
                    )
                    .await?;
                    events.push(context_patch_event(
                        HookEventKind::PostToolUse,
                        "post-tool-use",
                        &context.content,
                    ));
                }
            }
            Err(error) => {
                let message = redactor.redact(&error.to_string(), &RedactRules::default());
                let dispatch = engine
                    .hooks
                    .dispatch(
                        HookEvent::PostToolUseFailure {
                            tool_use_id: result.tool_use_id,
                            error: ToolErrorView { message },
                        },
                        hook_context(engine, session, ctx, messages),
                    )
                    .await
                    .map_err(engine_error)?;
                events.extend(hook_events(
                    HookEventKind::PostToolUseFailure,
                    &dispatch,
                    None,
                ));
            }
        }
    }
    Ok(events)
}

async fn dispatch_pre_model_hooks(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
    request: &mut ModelRequest,
    infer_ctx: &InferContext,
    messages: &[Message],
) -> Result<Vec<Event>, EngineError> {
    let mut events = Vec::new();
    let llm = engine
        .hooks
        .dispatch(
            HookEvent::PreLlmCall {
                run_id: ctx.run_id,
                request_view: model_request_view(engine, request),
            },
            hook_context(engine, session, ctx, messages),
        )
        .await
        .map_err(engine_error)?;
    events.extend(hook_events(HookEventKind::PreLlmCall, &llm, None));
    match llm.final_outcome {
        HookOutcome::Block { reason } => {
            return Err(engine_error(format!(
                "model call blocked by hook: {reason}"
            )));
        }
        HookOutcome::RewriteInput(value) => apply_model_request_patch(request, value),
        _ => {}
    }

    let api = engine
        .hooks
        .dispatch(
            HookEvent::PreApiRequest {
                request_id: infer_ctx.request_id,
                endpoint: model_endpoint(engine, request),
            },
            hook_context(engine, session, ctx, messages),
        )
        .await
        .map_err(engine_error)?;
    events.extend(hook_events(HookEventKind::PreApiRequest, &api, None));
    if let HookOutcome::Block { reason } = api.final_outcome {
        return Err(engine_error(format!(
            "api request blocked by hook: {reason}"
        )));
    }
    Ok(events)
}

async fn dispatch_post_model_hooks(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
    request_id: RequestId,
    usage: &UsageSnapshot,
    messages: &[Message],
) -> Result<Vec<Event>, EngineError> {
    let mut events = Vec::new();
    let llm = engine
        .hooks
        .dispatch(
            HookEvent::PostLlmCall {
                run_id: ctx.run_id,
                usage: usage.clone(),
            },
            hook_context(engine, session, ctx, messages),
        )
        .await
        .map_err(engine_error)?;
    events.extend(hook_events(HookEventKind::PostLlmCall, &llm, None));
    events.extend(dispatch_post_api_hook(engine, session, ctx, request_id, 200, messages).await?);
    Ok(events)
}

async fn dispatch_post_api_hook(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
    request_id: RequestId,
    status: u16,
    messages: &[Message],
) -> Result<Vec<Event>, EngineError> {
    let result = engine
        .hooks
        .dispatch(
            HookEvent::PostApiRequest { request_id, status },
            hook_context(engine, session, ctx, messages),
        )
        .await
        .map_err(engine_error)?;
    Ok(hook_events(HookEventKind::PostApiRequest, &result, None))
}

async fn dispatch_permission_hooks(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
    records: &[PermissionDecisionRecord],
    messages: &[Message],
) -> Result<Vec<Event>, EngineError> {
    let mut events = Vec::new();
    let redactor = hook_redactor(engine);
    for record in records {
        let detail = redactor.redact(
            &format!("{:?}", record.request.subject),
            &RedactRules::default(),
        );
        let result = engine
            .hooks
            .dispatch(
                HookEvent::PermissionRequest {
                    request_id: record.request.request_id,
                    subject: record.request.tool_name.clone(),
                    detail: Some(detail),
                },
                hook_context(engine, session, ctx, messages),
            )
            .await
            .map_err(engine_error)?;
        events.extend(hook_events(HookEventKind::PermissionRequest, &result, None));
    }
    Ok(events)
}

async fn flush_engine_permission_events(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
    emitted: &mut Vec<Event>,
    permission_recorder: &RecordingPermissionBroker,
    flushed_requested_events: &mut usize,
    flushed_permission_records: &mut usize,
    messages: &[Message],
) -> Result<(), EngineError> {
    let requested_events = permission_recorder.requested_events().await;
    if requested_events.len() > *flushed_requested_events {
        emitted.extend(
            requested_events[*flushed_requested_events..]
                .iter()
                .cloned(),
        );
        *flushed_requested_events = requested_events.len();
    }

    let records = permission_recorder.records().await;
    if records.len() <= *flushed_permission_records {
        return Ok(());
    }

    let new_records = records[*flushed_permission_records..].to_vec();
    *flushed_permission_records = records.len();
    let mut events =
        dispatch_permission_hooks(engine, session, ctx, &new_records, messages).await?;
    events.extend(permission_events(ctx.run_id, new_records));
    append(
        engine,
        session.tenant_id,
        session.session_id,
        emitted,
        events,
    )
    .await
}

fn hook_events(
    kind: HookEventKind,
    result: &DispatchResult,
    fail_closed_denied: Option<EventId>,
) -> Vec<Event> {
    let mut events = Vec::with_capacity(result.trail.len() + result.failures.len());
    for record in &result.trail {
        events.push(Event::HookTriggered(HookTriggeredEvent {
            hook_event_kind: kind.clone(),
            handler_id: record.handler_id.clone(),
            outcome_summary: outcome_summary(&record.outcome),
            duration_ms: duration_ms(record.duration),
            at: harness_contracts::now(),
        }));
    }
    for failure in &result.failures {
        let causation_id = EventId::new();
        events.push(Event::HookFailed(HookFailedEvent {
            hook_event_kind: kind.clone(),
            handler_id: failure.handler_id.clone(),
            failure_mode: failure.mode,
            cause_kind: failure.cause_kind,
            cause_detail: failure_detail(&failure.cause),
            duration_ms: duration_ms(failure.duration),
            fail_closed_denied,
            at: harness_contracts::now(),
        }));
        match &failure.cause {
            HookFailureCause::Unsupported {
                kind: returned_kind,
            } => {
                events.push(Event::HookReturnedUnsupported(
                    HookReturnedUnsupportedEvent {
                        hook_event_kind: kind.clone(),
                        handler_id: failure.handler_id.clone(),
                        returned_kind: returned_kind.clone(),
                        causation_id,
                        at: harness_contracts::now(),
                    },
                ));
            }
            HookFailureCause::Inconsistent { reason } => {
                events.push(Event::HookOutcomeInconsistent(
                    HookOutcomeInconsistentEvent {
                        hook_event_kind: kind.clone(),
                        handler_id: failure.handler_id.clone(),
                        reason: reason.clone(),
                        causation_id,
                        at: harness_contracts::now(),
                    },
                ));
            }
            _ => {}
        }
    }
    events
}

fn outcome_summary(outcome: &HookOutcome) -> HookOutcomeSummary {
    match outcome {
        HookOutcome::Continue => HookOutcomeSummary {
            continued: true,
            blocked_reason: None,
            rewrote_input: false,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: false,
        },
        HookOutcome::Block { reason } => HookOutcomeSummary {
            continued: false,
            blocked_reason: Some(reason.clone()),
            rewrote_input: false,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: false,
        },
        HookOutcome::PreToolUse(outcome) => HookOutcomeSummary {
            continued: outcome.is_continue(),
            blocked_reason: outcome.block.clone(),
            rewrote_input: outcome.rewrite_input.is_some(),
            overrode_permission: outcome.override_permission.clone(),
            added_context_bytes: outcome
                .additional_context
                .as_ref()
                .map(|context| context.content.len() as u64),
            transformed: false,
        },
        HookOutcome::RewriteInput(_) => HookOutcomeSummary {
            continued: false,
            blocked_reason: None,
            rewrote_input: true,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: false,
        },
        HookOutcome::OverridePermission(decision) => HookOutcomeSummary {
            continued: false,
            blocked_reason: None,
            rewrote_input: false,
            overrode_permission: Some(decision.clone()),
            added_context_bytes: None,
            transformed: false,
        },
        HookOutcome::AddContext(context) => HookOutcomeSummary {
            continued: false,
            blocked_reason: None,
            rewrote_input: false,
            overrode_permission: None,
            added_context_bytes: Some(context.content.len() as u64),
            transformed: false,
        },
        HookOutcome::Transform(_) => HookOutcomeSummary {
            continued: false,
            blocked_reason: None,
            rewrote_input: false,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: true,
        },
    }
}

fn failure_detail(cause: &HookFailureCause) -> String {
    match cause {
        HookFailureCause::Unsupported { kind } => format!("unsupported outcome: {kind:?}"),
        HookFailureCause::Inconsistent { reason } => format!("inconsistent outcome: {reason:?}"),
        HookFailureCause::Panicked { snippet } => snippet.clone(),
        HookFailureCause::Timeout => "timeout".to_owned(),
        HookFailureCause::Transport { kind, detail } => format!("{kind:?}: {detail}"),
        HookFailureCause::Unauthorized { capability } => format!("unauthorized: {capability}"),
    }
}

fn context_patch_event(kind: HookEventKind, handler_id: &str, content: &str) -> Event {
    Event::HookReturnedAdditionalContext(HookContextPatchEvent {
        hook_event_kind: kind,
        handler_id: handler_id.to_owned(),
        context_blob: None,
        byte_size: content.len() as u64,
        causation_id: EventId::new(),
        at: harness_contracts::now(),
    })
}

async fn push_hook_context_patch(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
    kind: HookEventKind,
    handler_id: &str,
    content: &str,
) -> Result<(), EngineError> {
    engine
        .context
        .push_patch(ContextPatchRequest {
            tenant_id: session.tenant_id,
            session_id: session.session_id,
            run_id: ctx.run_id,
            source: ContextPatchSource::HookAddContext {
                handler_id: handler_id.to_owned(),
                hook_event_kind: kind,
            },
            body: content.to_owned(),
            lifecycle: ContextPatchLifecycle::Transient,
        })
        .await
        .map_err(engine_error)
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn terminal_bytes(result: &ToolResult) -> Option<Bytes> {
    match result {
        ToolResult::Text(text) => Some(Bytes::from(text.clone())),
        _ => None,
    }
}

fn tool_result_from_transform(value: Value) -> ToolResult {
    match value {
        Value::String(text) => ToolResult::Text(text),
        other => ToolResult::Structured(other),
    }
}

fn model_request_view(engine: &Engine, request: &ModelRequest) -> harness_hook::ModelRequestView {
    harness_hook::ModelRequestView {
        provider_id: engine.model.provider_id().to_owned(),
        model_id: request.model_id.clone(),
        message_count: request.messages.len().try_into().unwrap_or(u32::MAX),
        tool_count: request
            .tools
            .as_ref()
            .map(Vec::len)
            .unwrap_or(0)
            .try_into()
            .unwrap_or(u32::MAX),
    }
}

fn model_endpoint(engine: &Engine, request: &ModelRequest) -> String {
    format!(
        "{}:{:?}:{}",
        engine.model.provider_id(),
        request.protocol,
        request.model_id
    )
}

fn apply_model_request_patch(request: &mut ModelRequest, value: Value) {
    let Some(object) = value.as_object() else {
        return;
    };
    if let Some(system) = object.get("system").and_then(Value::as_str) {
        request.system = Some(system.to_owned());
    }
    if let Some(extra) = object.get("extra") {
        request.extra = extra.clone();
    }
}

fn hash_value(value: &Value) -> [u8; 32] {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    *blake3::hash(&bytes).as_bytes()
}

async fn apply_steering(
    engine: &Engine,
    session: &SessionHandle,
    emitted: &mut Vec<Event>,
    ctx: &RunContext,
    working_messages: &mut Vec<Message>,
    next_input: &mut TurnInput,
) -> Result<(), EngineError> {
    let Some(steering_drain) = &engine.steering_drain else {
        return Ok(());
    };
    let target_message_id = if next_input.message.role == MessageRole::User {
        next_input.message.id
    } else {
        MessageId::new()
    };
    let Some(merge) = steering_drain
        .drain_and_merge(session, ctx.run_id, target_message_id)
        .await?
    else {
        return Ok(());
    };

    if merge.already_persisted {
        emitted.push(merge.applied_event);
    } else {
        append(
            engine,
            session.tenant_id,
            session.session_id,
            emitted,
            vec![merge.applied_event],
        )
        .await?;
    }

    if merge.body.is_empty() {
        return Ok(());
    }
    if next_input.message.role == MessageRole::User {
        append_text_to_message(&mut next_input.message, &merge.body);
    } else {
        working_messages.push(next_input.message.clone());
        *next_input = TurnInput {
            message: Message {
                id: target_message_id,
                role: MessageRole::User,
                parts: vec![MessagePart::Text(merge.body)],
                created_at: harness_contracts::now(),
            },
            metadata: json!({ "source": "steering" }),
        };
        dispatch_user_prompt_hook(engine, session, emitted, ctx, next_input, working_messages)
            .await?;
    }
    Ok(())
}

async fn append_user_message_if_needed(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
    emitted: &mut Vec<Event>,
    next_input: &TurnInput,
    appended_user_messages: &mut HashSet<MessageId>,
    client_message_id: Option<&str>,
) -> Result<(), EngineError> {
    if next_input.message.role != MessageRole::User
        || !appended_user_messages.insert(next_input.message.id)
    {
        return Ok(());
    }

    append(
        engine,
        session.tenant_id,
        session.session_id,
        emitted,
        vec![Event::UserMessageAppended(
            harness_contracts::UserMessageAppendedEvent {
                run_id: ctx.run_id,
                message_id: next_input.message.id,
                content: message_content(&next_input.message),
                metadata: message_metadata(client_message_id),
                at: harness_contracts::now(),
            },
        )],
    )
    .await
}

fn append_text_to_message(message: &mut Message, text: &str) {
    if let Some(MessagePart::Text(existing)) = message
        .parts
        .iter_mut()
        .find(|part| matches!(part, MessagePart::Text(_)))
    {
        if !existing.is_empty() {
            existing.push('\n');
        }
        existing.push_str(text);
        return;
    }
    message.parts.push(MessagePart::Text(text.to_owned()));
}

async fn append(
    engine: &Engine,
    tenant_id: TenantId,
    session_id: harness_contracts::SessionId,
    emitted: &mut Vec<Event>,
    events: Vec<Event>,
) -> Result<(), EngineError> {
    engine
        .event_store
        .append(tenant_id, session_id, &events)
        .await
        .map_err(engine_error)?;
    emitted.extend(events);
    Ok(())
}

async fn append_run_end(
    engine: &Engine,
    session: &SessionHandle,
    emitted: &mut Vec<Event>,
    run_id: harness_contracts::RunId,
    reason: EndReason,
    usage: UsageSnapshot,
) -> Result<(), EngineError> {
    append(
        engine,
        session.tenant_id,
        session.session_id,
        emitted,
        vec![Event::RunEnded(RunEndedEvent {
            run_id,
            reason,
            usage: Some(usage),
            ended_at: harness_contracts::now(),
        })],
    )
    .await
}

async fn append_interrupt_if_cancelled(
    engine: &Engine,
    session: &SessionHandle,
    emitted: &mut Vec<Event>,
    ctx: &RunContext,
    usage: UsageSnapshot,
) -> Result<bool, EngineError> {
    let Some(cause) = ctx.cancellation.cause().await else {
        return Ok(false);
    };
    append_run_end(
        engine,
        session,
        emitted,
        ctx.run_id,
        end_reason_for_interrupt(cause),
        usage,
    )
    .await?;
    Ok(true)
}

async fn finalize_run_error(
    engine: &Engine,
    session: &SessionHandle,
    emitted: &mut Vec<Event>,
    run_id: harness_contracts::RunId,
    error: impl std::fmt::Display,
) -> Result<(), EngineError> {
    append_run_end(
        engine,
        session,
        emitted,
        run_id,
        EndReason::Error(error.to_string()),
        UsageSnapshot::default(),
    )
    .await
}

fn prompt_visible_tools(tools: &harness_tool::ToolPool) -> Vec<ToolDescriptor> {
    tools.prompt_visible_descriptors()
}

fn prompt_visible_tools_for_model(engine: &Engine) -> Vec<ToolDescriptor> {
    if !engine.model_snapshot.conversation_capability.tool_calling {
        return Vec::new();
    }
    prompt_visible_tools(&engine.tools)
}

fn model_request_tools(
    engine: &Engine,
    tools_snapshot: Vec<ToolDescriptor>,
) -> Option<Vec<ToolDescriptor>> {
    if !engine.model_snapshot.conversation_capability.tool_calling || tools_snapshot.is_empty() {
        return None;
    }
    Some(tools_snapshot)
}

fn validate_model_input_modalities(
    messages: &[Message],
    supported: &[ModelModality],
) -> Result<(), EngineError> {
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
                    return Err(engine_error(format!(
                        "model does not support {required:?} input"
                    )));
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
struct TurnContextView {
    tenant_id: TenantId,
    session_id: harness_contracts::SessionId,
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

    fn session_id(&self) -> Option<harness_contracts::SessionId> {
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
    permission_mode: PermissionMode,
    redactor: Arc<dyn Redactor>,
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

fn hook_context(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
    messages: &[Message],
) -> HookContext {
    let redactor = hook_redactor(engine);
    HookContext {
        tenant_id: session.tenant_id,
        session_id: session.session_id,
        run_id: Some(ctx.run_id),
        turn_index: Some(next_turn_index(messages)),
        correlation_id: ctx.correlation_id,
        causation_id: CausationId::new(),
        trust_level: TrustLevel::UserControlled,
        permission_mode: ctx.permission_mode,
        interactivity: ctx.interactivity,
        at: harness_contracts::now(),
        view: Arc::new(TurnHookView {
            workspace_root: engine.workspace_root.clone(),
            messages: messages.to_vec(),
            permission_mode: ctx.permission_mode,
            redactor,
        }),
        upstream_outcome: None,
        replay_mode: ReplayMode::Live,
    }
}

fn hook_redactor(engine: &Engine) -> Arc<dyn Redactor> {
    engine
        .observer
        .as_ref()
        .map(|observer| Arc::clone(&observer.redactor))
        .unwrap_or_else(|| Arc::new(DefaultRedactor::default()))
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

fn redact_tool_result(result: ToolResult, redactor: &dyn Redactor) -> ToolResult {
    match result {
        ToolResult::Text(text) => ToolResult::Text(redactor.redact(&text, &RedactRules::default())),
        ToolResult::Structured(value) => {
            ToolResult::Structured(redact_json_strings(value, redactor))
        }
        ToolResult::Blob {
            content_type,
            blob_ref,
        } => ToolResult::Blob {
            content_type,
            blob_ref,
        },
        ToolResult::Mixed(parts) => ToolResult::Mixed(
            parts
                .into_iter()
                .map(|part| redact_tool_result_part(part, redactor))
                .collect(),
        ),
        result => result,
    }
}

fn redact_tool_result_part(part: ToolResultPart, redactor: &dyn Redactor) -> ToolResultPart {
    match part {
        ToolResultPart::Text { text } => ToolResultPart::Text {
            text: redactor.redact(&text, &RedactRules::default()),
        },
        ToolResultPart::Structured { value, schema_ref } => ToolResultPart::Structured {
            value: redact_json_strings(value, redactor),
            schema_ref,
        },
        ToolResultPart::Blob {
            content_type,
            blob_ref,
            summary,
        } => ToolResultPart::Blob {
            content_type,
            blob_ref,
            summary: summary.map(|text| redactor.redact(&text, &RedactRules::default())),
        },
        ToolResultPart::Code { language, text } => ToolResultPart::Code {
            language,
            text: redactor.redact(&text, &RedactRules::default()),
        },
        ToolResultPart::Reference {
            reference_kind,
            title,
            summary,
        } => ToolResultPart::Reference {
            reference_kind,
            title: title.map(|text| redactor.redact(&text, &RedactRules::default())),
            summary: summary.map(|text| redactor.redact(&text, &RedactRules::default())),
        },
        ToolResultPart::Table {
            headers,
            rows,
            caption,
        } => ToolResultPart::Table {
            headers: headers
                .into_iter()
                .map(|text| redactor.redact(&text, &RedactRules::default()))
                .collect(),
            rows: rows
                .into_iter()
                .map(|row| {
                    row.into_iter()
                        .map(|value| redact_json_strings(value, redactor))
                        .collect()
                })
                .collect(),
            caption: caption.map(|text| redactor.redact(&text, &RedactRules::default())),
        },
        ToolResultPart::Progress {
            stage,
            ratio,
            detail,
        } => ToolResultPart::Progress {
            stage: redactor.redact(&stage, &RedactRules::default()),
            ratio,
            detail: detail.map(|text| redactor.redact(&text, &RedactRules::default())),
        },
        ToolResultPart::Error {
            code,
            message,
            retriable,
        } => ToolResultPart::Error {
            code: redactor.redact(&code, &RedactRules::default()),
            message: redactor.redact(&message, &RedactRules::default()),
            retriable,
        },
        part => part,
    }
}

#[derive(Clone)]
struct PermissionDecisionRecord {
    request: PermissionRequest,
    decision: Decision,
    decided_by: DecidedBy,
    hook_conflict: Option<HookPermissionConflict>,
    fingerprint: ExecFingerprint,
    suppressed: Option<SuppressedPermissionRecord>,
}

#[derive(Clone)]
struct SuppressedPermissionRecord {
    original_request_id: RequestId,
    reason: SuppressionReason,
}

struct RecordingPermissionBroker {
    inner: Arc<dyn PermissionBroker>,
    overrides: Vec<HookPermissionDecisionOverride>,
    records: Mutex<Vec<PermissionDecisionRecord>>,
    event_store: Arc<dyn EventStore>,
    requested_events: Mutex<Vec<Event>>,
    run_id: harness_contracts::RunId,
    interactivity: InteractivityLevel,
}

impl RecordingPermissionBroker {
    fn new(
        inner: Arc<dyn PermissionBroker>,
        overrides: Vec<HookPermissionDecisionOverride>,
        event_store: Arc<dyn EventStore>,
        run_id: harness_contracts::RunId,
        interactivity: InteractivityLevel,
    ) -> Self {
        Self {
            inner,
            overrides,
            records: Mutex::new(Vec::new()),
            event_store,
            requested_events: Mutex::new(Vec::new()),
            run_id,
            interactivity,
        }
    }

    async fn records(&self) -> Vec<PermissionDecisionRecord> {
        self.records.lock().await.clone()
    }

    async fn requested_events(&self) -> Vec<Event> {
        self.requested_events.lock().await.clone()
    }
}

#[async_trait]
impl PermissionBroker for RecordingPermissionBroker {
    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        let fingerprint = canonical_permission_fingerprint(&request);
        if request.tenant_id != ctx.tenant_id || request.session_id != ctx.session_id {
            self.records.lock().await.push(PermissionDecisionRecord {
                request,
                decision: Decision::DenyOnce,
                decided_by: DecidedBy::Broker {
                    broker_id: "permission-context".to_owned(),
                },
                hook_conflict: None,
                fingerprint,
                suppressed: None,
            });
            return Decision::DenyOnce;
        }

        if let Some(previous) = self.reusable_previous_decision(fingerprint).await {
            let decision = previous.decision.clone();
            self.records.lock().await.push(PermissionDecisionRecord {
                request,
                decision: decision.clone(),
                decided_by: DecidedBy::Broker {
                    broker_id: "dedup-gate".to_owned(),
                },
                hook_conflict: None,
                fingerprint,
                suppressed: Some(SuppressedPermissionRecord {
                    original_request_id: previous.request.request_id,
                    reason: suppression_reason_for_decision(&decision),
                }),
            });
            return decision;
        }

        let requested_event = permission_requested_event(
            self.run_id,
            &request,
            fingerprint,
            self.interactivity,
            ctx.tenant_id,
            ctx.session_id,
        );
        if self
            .event_store
            .append(
                ctx.tenant_id,
                ctx.session_id,
                std::slice::from_ref(&requested_event),
            )
            .await
            .is_err()
        {
            self.records.lock().await.push(PermissionDecisionRecord {
                request,
                decision: Decision::DenyOnce,
                decided_by: DecidedBy::Broker {
                    broker_id: "permission-event-store".to_owned(),
                },
                hook_conflict: None,
                fingerprint,
                suppressed: None,
            });
            return Decision::DenyOnce;
        }
        self.requested_events.lock().await.push(requested_event);

        let (decision, decided_by, hook_conflict) = if let Some(override_decision) = self
            .overrides
            .iter()
            .find(|override_decision| override_decision.tool_use_id == request.tool_use_id)
        {
            (
                override_decision.override_decision.decision.clone(),
                DecidedBy::Hook {
                    handler_id: override_decision.override_decision.handler_id.clone(),
                },
                override_decision.conflict.clone(),
            )
        } else {
            (
                self.inner.decide(request.clone(), ctx).await,
                DecidedBy::Broker {
                    broker_id: "engine-turn-runtime".to_owned(),
                },
                None,
            )
        };
        self.records.lock().await.push(PermissionDecisionRecord {
            request,
            decision: decision.clone(),
            decided_by,
            hook_conflict,
            fingerprint,
            suppressed: None,
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

impl RecordingPermissionBroker {
    async fn reusable_previous_decision(
        &self,
        fingerprint: ExecFingerprint,
    ) -> Option<PermissionDecisionRecord> {
        self.records
            .lock()
            .await
            .iter()
            .find(|record| {
                record.fingerprint == fingerprint
                    && matches!(
                        record.decision,
                        Decision::AllowOnce
                            | Decision::AllowSession
                            | Decision::AllowPermanent
                            | Decision::DenyOnce
                            | Decision::DenyPermanent
                    )
            })
            .cloned()
    }
}

fn suppression_reason_for_decision(decision: &Decision) -> SuppressionReason {
    match decision {
        Decision::AllowOnce | Decision::AllowSession | Decision::AllowPermanent => {
            SuppressionReason::RecentlyAllowed
        }
        Decision::DenyOnce | Decision::DenyPermanent | Decision::Escalate | _ => {
            SuppressionReason::RecentlyDenied
        }
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

fn permission_context(session: &SessionHandle, ctx: &RunContext) -> PermissionContext {
    PermissionContext {
        permission_mode: ctx.permission_mode,
        previous_mode: None,
        session_id: session.session_id,
        tenant_id: session.tenant_id,
        run_id: Some(ctx.run_id),
        interactivity: ctx.interactivity,
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

fn permission_requested_event(
    run_id: harness_contracts::RunId,
    request: &PermissionRequest,
    fingerprint: ExecFingerprint,
    interactivity: InteractivityLevel,
    tenant_id: TenantId,
    session_id: SessionId,
) -> Event {
    Event::PermissionRequested(PermissionRequestedEvent {
        request_id: request.request_id,
        run_id,
        session_id,
        tenant_id,
        tool_use_id: request.tool_use_id,
        tool_name: request.tool_name.clone(),
        subject: request.subject.clone(),
        severity: request.severity,
        scope_hint: request.scope_hint.clone(),
        fingerprint: Some(fingerprint),
        presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
        interactivity,
        causation_id: EventId::new(),
        at: harness_contracts::now(),
    })
}

fn permission_events(
    run_id: harness_contracts::RunId,
    records: Vec<PermissionDecisionRecord>,
) -> Vec<Event> {
    let mut events = Vec::with_capacity(records.len() * 3);
    for record in records {
        if let Some(suppressed) = &record.suppressed {
            events.push(Event::PermissionRequestSuppressed(
                PermissionRequestSuppressedEvent {
                    request_id: record.request.request_id,
                    run_id,
                    session_id: record.request.session_id,
                    tenant_id: record.request.tenant_id,
                    tool_use_id: record.request.tool_use_id,
                    tool_name: record.request.tool_name.clone(),
                    subject: record.request.subject.clone(),
                    severity: record.request.severity,
                    scope_hint: record.request.scope_hint.clone(),
                    original_request_id: suppressed.original_request_id,
                    original_decision_id: None,
                    reused_decision: Some(record.decision.clone()),
                    reason: suppressed.reason.clone(),
                    causation_id: EventId::new(),
                    at: harness_contracts::now(),
                },
            ));
            continue;
        }
        let resolved_event_id = EventId::new();
        events.push(Event::PermissionResolved(PermissionResolvedEvent {
            request_id: record.request.request_id,
            decision: record.decision.clone(),
            decided_by: record.decided_by.clone(),
            scope: record.request.scope_hint.clone(),
            fingerprint: Some(record.fingerprint),
            rationale: None,
            at: harness_contracts::now(),
        }));
        if let Some(conflict) = record.hook_conflict {
            events.push(Event::HookPermissionConflict(HookPermissionConflictEvent {
                hook_event_kind: HookEventKind::PreToolUse,
                priority: conflict.priority,
                participants: conflict.participants,
                winner: conflict.winner,
                resolved_event_id,
                at: harness_contracts::now(),
            }));
        }
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

fn tool_result_events(
    result: &RuntimeToolResultEnvelope,
    session_id: SessionId,
    run_id: RunId,
) -> Vec<Event> {
    match &result.result {
        Ok(tool_result) => {
            let at = harness_contracts::now();
            let mut events = vec![Event::ToolUseCompleted(ToolUseCompletedEvent {
                tool_use_id: result.tool_use_id,
                result: tool_result.clone(),
                usage: None,
                duration_ms: result.duration.as_millis().min(u128::from(u64::MAX)) as u64,
                at,
            })];
            if let Some(blob_ref) = image_artifact_blob(&result.tool_name, tool_result) {
                events.push(Event::ArtifactCreated(ArtifactCreatedEvent {
                    session_id,
                    run_id,
                    artifact_id: format!("artifact:{}", result.tool_use_id),
                    title: "生成的图片".to_owned(),
                    kind: "image".to_owned(),
                    status: ArtifactStatus::Ready,
                    source: ArtifactSource::Tool,
                    source_message_id: None,
                    source_tool_use_id: Some(result.tool_use_id),
                    content_hash: Some(blob_ref.content_hash.to_vec()),
                    blob_ref: Some(blob_ref),
                    preview: Some("生成的图片".to_owned()),
                    at,
                }));
            }
            events
        }
        Err(error) => vec![Event::ToolUseFailed(ToolUseFailedEvent {
            tool_use_id: result.tool_use_id,
            error: tool_error_payload(error),
            at: harness_contracts::now(),
        })],
    }
}

fn image_artifact_blob(tool_name: &str, result: &ToolResult) -> Option<BlobRef> {
    if !is_image_artifact_tool(tool_name) {
        return None;
    }
    match result {
        ToolResult::Blob {
            content_type,
            blob_ref,
        } if is_image_content_type(content_type, blob_ref.content_type.as_deref()) => {
            Some(blob_ref.clone())
        }
        ToolResult::Mixed(parts) => parts.iter().find_map(|part| match part {
            ToolResultPart::Blob {
                content_type,
                blob_ref,
                ..
            } if is_image_content_type(content_type, blob_ref.content_type.as_deref()) => {
                Some(blob_ref.clone())
            }
            _ => None,
        }),
        _ => None,
    }
}

fn is_image_artifact_tool(tool_name: &str) -> bool {
    let normalized = tool_name.to_ascii_lowercase();
    normalized.contains("image") || normalized.contains("minimax")
}

fn is_image_content_type(content_type: &str, blob_content_type: Option<&str>) -> bool {
    is_safe_image_content_type(content_type)
        || blob_content_type.is_some_and(is_safe_image_content_type)
}

fn is_safe_image_content_type(content_type: &str) -> bool {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase();
    matches!(
        mime.as_str(),
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/avif"
    )
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

fn message_content(message: &Message) -> MessageContent {
    if let [MessagePart::Text(text)] = message.parts.as_slice() {
        return MessageContent::Text(text.clone());
    }
    MessageContent::Multimodal(message.parts.clone())
}

fn message_metadata(client_message_id: Option<&str>) -> MessageMetadata {
    let mut metadata = MessageMetadata::default();
    if let Some(client_message_id) = client_message_id {
        metadata
            .labels
            .insert("clientMessageId".to_owned(), client_message_id.to_owned());
    }
    metadata
}

fn collected_messages(events: &[Event]) -> Vec<Message> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::UserMessageAppended(event) => Some(Message {
                id: event.message_id,
                role: MessageRole::User,
                parts: message_parts(event.content.clone()),
                created_at: event.at,
            }),
            Event::AssistantMessageCompleted(event) => Some(Message {
                id: event.message_id,
                role: MessageRole::Assistant,
                parts: message_parts(event.content.clone()),
                created_at: event.at,
            }),
            _ => None,
        })
        .collect()
}

fn message_parts(content: MessageContent) -> Vec<MessagePart> {
    match content {
        MessageContent::Text(text) => vec![MessagePart::Text(text)],
        MessageContent::Structured(value) => vec![MessagePart::Text(value.to_string())],
        MessageContent::Multimodal(parts) => parts,
    }
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

async fn append_usage_accumulated(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
    emitted: &mut Vec<Event>,
    mut delta: UsageSnapshot,
    pricing_snapshot_id: Option<PricingSnapshotId>,
) -> Result<(), EngineError> {
    if usage_is_zero(&delta) {
        return Ok(());
    }
    let model_ref = engine.model_ref();
    if let Some(observer) = &engine.observer {
        if let Some(pricing) = pricing_entry_for_model(engine) {
            observer.usage.register_pricing(pricing);
        }
        if let Some(cost) =
            observer
                .usage
                .compute_cost(&model_ref, pricing_snapshot_id.as_ref(), &delta)
        {
            delta.cost_micros = cost.cost_micros;
        }
        observer.usage.record_scopes_with_pricing(
            [
                harness_observability::UsageScope::Tenant(session.tenant_id),
                harness_observability::UsageScope::Session(session.session_id),
                harness_observability::UsageScope::Run(ctx.run_id),
                harness_observability::UsageScope::Model(model_usage_key(&model_ref)),
            ],
            Some(model_ref.clone()),
            pricing_snapshot_id.clone(),
            delta.clone(),
        );
    }
    append(
        engine,
        session.tenant_id,
        session.session_id,
        emitted,
        vec![Event::UsageAccumulated(UsageAccumulatedEvent {
            session_id: session.session_id,
            run_id: Some(ctx.run_id),
            delta,
            model_ref: Some(model_ref),
            pricing_snapshot_id,
            at: harness_contracts::now(),
        })],
    )
    .await
}

async fn pricing_snapshot_for_model(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
) -> Option<PricingSnapshotId> {
    if let Some(resolver) = &engine.pricing_snapshot_resolver {
        let context = PricingSnapshotResolveContext {
            tenant_id: session.tenant_id,
            session_id: session.session_id,
            run_id: Some(ctx.run_id),
            model_ref: engine.model_ref(),
        };
        let snapshot = resolver.resolve(context.clone()).await;
        if snapshot.is_none() {
            resolver.record_miss(context).await;
        }
        return snapshot;
    }
    engine.model_snapshot.pricing_snapshot_id()
}

fn model_usage_key(model_ref: &ModelRef) -> String {
    format!("{}/{}", model_ref.provider_id, model_ref.model_id)
}

fn record_model_infer(engine: &Engine, duration: Duration, usage: &UsageSnapshot) {
    if let Some(observer) = &engine.observer {
        observer
            .model_metrics
            .record_infer(model_usage_key(&engine.model_ref()), duration, usage);
    }
}

fn record_model_error(engine: &Engine, class: &str) {
    if let Some(observer) = &engine.observer {
        observer
            .model_metrics
            .record_model_error(model_usage_key(&engine.model_ref()), class);
    }
}

fn record_model_stream_error(engine: &Engine, class: &str) {
    if let Some(observer) = &engine.observer {
        observer
            .model_metrics
            .record_stream_error(model_usage_key(&engine.model_ref()), class);
    }
}

fn model_error_class(error: &ModelError) -> &'static str {
    match error {
        ModelError::Message(_) => "message",
        ModelError::RateLimited(_) => "rate_limited",
        ModelError::ContextTooLong { .. } => "context_too_long",
        ModelError::InvalidRequest(_) => "invalid_request",
        ModelError::AllCredentialsBanned => "all_credentials_banned",
        ModelError::AuxModelNotConfigured => "aux_model_not_configured",
        ModelError::AuthExpired(_) => "auth_expired",
        ModelError::ProviderUnavailable(_) => "provider_unavailable",
        ModelError::UnexpectedResponse(_) => "unexpected_response",
        ModelError::Cancelled => "cancelled",
        ModelError::DeadlineExceeded(_) => "deadline_exceeded",
        ModelError::Io(_) => "io",
        _ => "unknown",
    }
}

fn pricing_entry_for_model(engine: &Engine) -> Option<harness_observability::PricingTableEntry> {
    let pricing = engine.model_snapshot.pricing.as_ref()?;
    Some(harness_observability::PricingTableEntry {
        pricing_id: pricing.pricing_id.clone(),
        pricing_version: pricing.pricing_version,
        input_per_million: pricing.input_per_million,
        output_per_million: pricing.output_per_million,
        cache_creation_per_million: pricing.cache_creation_per_million,
        cache_read_per_million: pricing.cache_read_per_million,
        last_updated: pricing.last_updated,
        source: match &pricing.source {
            PricingSource::Hardcoded => harness_observability::PricingSource::Hardcoded,
            PricingSource::ProviderApi => harness_observability::PricingSource::ProviderApi,
            PricingSource::ManualOverride => harness_observability::PricingSource::ManualOverride,
            PricingSource::BusinessProvided => {
                harness_observability::PricingSource::BusinessProvided
            }
        },
        billing_mode: match &pricing.billing_mode {
            BillingMode::Standard => harness_observability::PricingBillingMode::Standard,
            BillingMode::Cached {
                cache_read_discount,
            } => harness_observability::PricingBillingMode::Cached {
                cache_read_discount: ratio_to_observability(*cache_read_discount),
            },
            BillingMode::Batched { discount } => {
                harness_observability::PricingBillingMode::Batched {
                    discount: ratio_to_observability(*discount),
                }
            }
            BillingMode::Tiered { thresholds } => {
                harness_observability::PricingBillingMode::Tiered {
                    thresholds: thresholds.clone(),
                }
            }
        },
    })
}

fn ratio_to_observability(ratio: Ratio) -> harness_observability::Ratio {
    harness_observability::Ratio(ratio.0)
}

fn cost_micros_for_usage(
    engine: &Engine,
    usage: &UsageSnapshot,
    pricing_snapshot_id: Option<&PricingSnapshotId>,
) -> Option<u64> {
    let observer = engine.observer.as_ref()?;
    if let Some(pricing) = pricing_entry_for_model(engine) {
        observer.usage.register_pricing(pricing);
    }
    observer
        .usage
        .compute_cost(&engine.model_ref(), pricing_snapshot_id, usage)
        .map(|cost| cost.cost_micros)
}

fn usage_is_zero(usage: &UsageSnapshot) -> bool {
    usage.input_tokens == 0
        && usage.output_tokens == 0
        && usage.cache_read_tokens == 0
        && usage.cache_write_tokens == 0
        && usage.cost_micros == 0
        && usage.tool_calls == 0
}

fn model_extra_with_relay_logical_call_key(
    extra: Value,
    run_id: harness_contracts::RunId,
    iteration: u32,
) -> Value {
    let key = format!("engine_turn:{run_id}:{iteration}");
    match extra {
        Value::Object(mut object) => {
            object
                .entry("relay_logical_call_key".to_owned())
                .or_insert_with(|| json!(key));
            Value::Object(object)
        }
        _ => json!({ "relay_logical_call_key": key }),
    }
}

fn budget_exhausted(
    limits: Option<&crate::RunBudgetLimits>,
    usage: &UsageSnapshot,
    tool_calls: u64,
    elapsed: Duration,
) -> Option<BudgetKind> {
    let limits = limits?;
    let tokens = usage
        .input_tokens
        .saturating_add(usage.output_tokens)
        .saturating_add(usage.cache_read_tokens)
        .saturating_add(usage.cache_write_tokens);
    if limits.max_tokens.is_some_and(|max| tokens >= max) {
        return Some(BudgetKind::Tokens);
    }
    if limits.max_tool_calls.is_some_and(|max| tool_calls >= max) {
        return Some(BudgetKind::ToolCalls);
    }
    if limits.max_duration.is_some_and(|max| elapsed >= max) {
        return Some(BudgetKind::WallClock);
    }
    if limits
        .max_cost_micros
        .is_some_and(|max| usage.cost_micros >= max)
    {
        return Some(BudgetKind::Cost);
    }
    None
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

fn engine_error(error: impl std::fmt::Display) -> EngineError {
    EngineError::Message(error.to_string())
}

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use futures::{stream, StreamExt};
use harness_context::{ContextSessionView, TokenBudget};
#[cfg(feature = "recall-memory")]
use harness_contracts::MemoryThreadSettings;
use harness_contracts::{
    ArtifactCreatedEvent, ArtifactRevisionId, ArtifactSource, ArtifactStatus,
    AssistantMessageCompletedEvent, BlobRef, BudgetKind, CausationId, ContentHash,
    ContextPatchLifecycle, ContextPatchRequest, ContextPatchSinkCap, ContextPatchSource,
    ConversationAttachmentReference, CorrelationId, DenyReason, EndReason, Event, EventId,
    FallbackPolicy, HookContextPatchEvent, HookEventKind, HookFailedEvent,
    HookOutcomeInconsistentEvent, HookOutcomeSummary, HookReturnedUnsupportedEvent,
    HookRewroteInputEvent, HookTriggeredEvent, MemoryId, MemoryModelRequestPreview,
    MemoryModelRequestPreviewSection, MemorySource, MemoryTraceId, Message, MessageContent,
    MessageId, MessageMetadata, MessagePart, MessageRole, ModelError, ModelRef, PermissionMode,
    PricingSnapshotId, RedactRules, Redactor, RequestId, RunEndedEvent, RunId, RunModelSnapshot,
    RunStartedEvent, SessionId, TeamId, TenantId, ToolDescriptor, ToolError, ToolErrorPayload,
    ToolResult, ToolResultPart, ToolUseCompletedEvent, ToolUseDeniedEvent, ToolUseFailedEvent,
    ToolUseId, ToolUseRequestedEvent, TrustLevel, TurnInput, UsageAccumulatedEvent, UsageSnapshot,
};
use harness_execution::{AuthorizationContext, ExecutionError};
use harness_hook::{
    DispatchResult, HookContext, HookEvent, HookFailureCause, HookMessageView, HookOutcome,
    HookSessionView, ReplayMode, ToolDescriptorView, ToolErrorView,
};
use harness_model::{
    apply_before_request_middlewares, apply_request_end_middlewares, wrap_stream_with_middlewares,
    BillingMode, InferContext, ModelModality, ModelRequest, ModelRuntimeSnapshot,
    PricingSnapshotResolveContext, PricingSource, ProviderRequestContext, Ratio,
    ReasoningProtocolSemantics,
};
use harness_observability::{DefaultRedactor, Span, SpanAttributes};
use harness_provider_state::{
    ProviderContinuationKind, ProviderContinuationQuery, ProviderContinuationRecord,
    ProviderContinuationScope,
};
use harness_tool::{
    AuthorizedToolCall, InterruptToken, OrchestratorContext, ToolCall, ToolEventEmitter,
    ToolOrchestrator, ToolResultEnvelope as RuntimeToolResultEnvelope,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::{
    end_reason_for_interrupt, result_inject, turn_assembly::TurnAssembly, Engine, EngineError,
    EventStream, RunContext, SessionHandle,
};

const MISSING_PROVIDER_CONTINUATION_ERROR: &str =
    "provider continuation required for assistant tool replay but missing";

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
        model: ctx.model.clone().unwrap_or_else(|| RunModelSnapshot {
            model_config_id: None,
            provider_id: engine.model_snapshot.provider_id.clone(),
            model_id: engine.model_snapshot.model_id.clone(),
            display_name: engine.model_snapshot.display_name.clone(),
            protocol: engine.model_snapshot.protocol,
            context_window: engine.model_snapshot.context_window,
            max_output_tokens: engine.model_snapshot.max_output_tokens,
            conversation_capability: engine.model_snapshot.conversation_capability.clone(),
        }),
        input: input.clone(),
        snapshot_id: ctx.config_snapshot_id,
        effective_config_hash: ctx.effective_config_hash,
        started_at: harness_contracts::now(),
        correlation_id,
        permission_mode: ctx.permission_mode,
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
            #[cfg(feature = "recall-memory")]
            memory_thread_settings: ctx.memory_thread_settings.clone(),
            system: engine.system_prompt.clone(),
            messages: working_messages.clone(),
            tools: prompt_visible_tools_for_model(engine),
        };
        let mut assembled = engine
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
        let budget = engine.context.budget();
        let trigger_tokens = soft_budget_trigger_tokens(budget);
        if assembled.tokens_estimate >= trigger_tokens {
            let compacted = engine
                .context
                .proactive_compact_prompt(
                    session.tenant_id,
                    session.session_id,
                    assembled,
                    trigger_tokens,
                )
                .await
                .map_err(engine_error)?;
            assembled = compacted.prompt;
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
        }
        validate_model_input_modalities(
            &assembled.messages,
            &engine
                .model_snapshot
                .conversation_capability
                .input_modalities,
        )?;
        let assembled_tools = model_request_tools(engine, assembled.tools_snapshot);
        let provider_context = provider_request_context_for_prompt(
            engine,
            &session,
            &ctx,
            &assembled.messages,
            assembled_tools
                .as_ref()
                .is_some_and(|tools| !tools.is_empty()),
        )
        .await?;

        let mut request = ModelRequest {
            model_id: engine.model_id.clone(),
            messages: assembled.messages,
            tools: assembled_tools,
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
            provider_context,
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

        record_model_request_preview(
            engine,
            session.tenant_id,
            session.session_id,
            ctx.run_id,
            &request,
            final_model_request_token_estimate(&request),
            latest_memory_trace_id(&assembled.events),
        )
        .await;

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
        let mut stream = match infer_or_interrupt(
            engine,
            &session,
            &mut emitted,
            &ctx,
            request.clone(),
            infer_ctx.clone(),
            usage.clone(),
        )
        .await?
        {
            None => return Ok(Box::pin(stream::iter(emitted))),
            Some(Ok(stream)) => stream,
            Some(Err(ModelError::ContextTooLong { tokens, max })) => {
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
                let compacted_tools = model_request_tools(engine, compacted.prompt.tools_snapshot);
                let provider_context = provider_request_context_for_prompt(
                    engine,
                    &session,
                    &ctx,
                    &compacted.prompt.messages,
                    compacted_tools
                        .as_ref()
                        .is_some_and(|tools| !tools.is_empty()),
                )
                .await?;
                request = ModelRequest {
                    model_id: engine.model_id.clone(),
                    messages: compacted.prompt.messages,
                    tools: compacted_tools,
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
                    provider_context,
                };
                if let Err(error) =
                    apply_before_request_middlewares(&mut request, &mut infer_ctx).await
                {
                    finalize_run_error(engine, &session, &mut emitted, ctx.run_id, &error).await?;
                    return Err(engine_error(error));
                }
                model_call_started = Instant::now();
                match infer_or_interrupt(
                    engine,
                    &session,
                    &mut emitted,
                    &ctx,
                    request.clone(),
                    infer_ctx.clone(),
                    usage.clone(),
                )
                .await?
                {
                    None => return Ok(Box::pin(stream::iter(emitted))),
                    Some(Ok(stream)) => stream,
                    Some(Err(error)) => {
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
            Some(Err(error)) => {
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

        let mut assembly = TurnAssembly::new(MessageId::new());

        loop {
            let event = tokio::select! {
                event = stream.next() => event,
                cause = ctx.cancellation.cancelled() => {
                    append_run_end(
                        engine,
                        &session,
                        &mut emitted,
                        ctx.run_id,
                        end_reason_for_interrupt(cause),
                        usage.clone(),
                    )
                    .await?;
                    return Ok(Box::pin(stream::iter(emitted)));
                }
            };
            let Some(event) = event else {
                break;
            };
            let step = assembly.push_event(ctx.run_id, event);
            add_usage(&mut usage, &step.usage_delta);
            if !step.events.is_empty() {
                append(
                    engine,
                    session.tenant_id,
                    session.session_id,
                    &mut emitted,
                    step.events,
                )
                .await?;
            }
            if let Some(stream_error) = step.stream_error {
                record_model_infer(
                    engine,
                    model_call_started.elapsed(),
                    assembly.model_call_usage(),
                );
                record_model_stream_error(engine, &format!("{:?}", stream_error.class));
                let message = format!(
                    "model stream error ({:?}): {}",
                    stream_error.class, stream_error.error
                );
                finalize_run_error(engine, &session, &mut emitted, ctx.run_id, &message).await?;
                return Err(engine_error(message));
            }
            if append_interrupt_if_cancelled(engine, &session, &mut emitted, &ctx, usage.clone())
                .await?
            {
                return Ok(Box::pin(stream::iter(emitted)));
            }
        }
        record_model_infer(
            engine,
            model_call_started.elapsed(),
            assembly.model_call_usage(),
        );

        if let Err(error) = apply_request_end_middlewares(&usage, &infer_ctx).await {
            finalize_run_error(engine, &session, &mut emitted, ctx.run_id, &error).await?;
            return Err(engine_error(error));
        }
        let pricing_snapshot_id = pricing_snapshot_for_model(engine, &session, &ctx).await;
        let mut priced_model_call_usage = assembly.model_call_usage().clone();
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

        if assembly.tool_calls().is_empty() {
            append(
                engine,
                session.tenant_id,
                session.session_id,
                &mut emitted,
                vec![Event::AssistantMessageCompleted(
                    AssistantMessageCompletedEvent {
                        run_id: ctx.run_id,
                        message_id: assembly.assistant_message_id(),
                        content: MessageContent::Text(assembly.assistant_text().to_owned()),
                        tool_uses: Vec::new(),
                        usage: usage.clone(),
                        pricing_snapshot_id: pricing_snapshot_id.clone(),
                        stop_reason: assembly.stop_reason(),
                        at: harness_contracts::now(),
                    },
                )],
            )
            .await?;
            store_provider_continuations(
                engine,
                &session,
                &ctx,
                &request.provider_context,
                &assembly,
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

        let pre_tool_application = apply_pre_tool_use_hooks(
            engine,
            &session,
            &ctx,
            assembly.tool_calls(),
            &working_messages,
        )
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
        assembly.replace_tool_calls(pre_tool_application.calls);

        let assistant_tool_message = result_inject::assistant_tool_message(
            assembly.assistant_message_id(),
            assembly.assistant_text().to_owned(),
            assembly.tool_calls(),
        );
        append(
            engine,
            session.tenant_id,
            session.session_id,
            &mut emitted,
            vec![Event::AssistantMessageCompleted(
                AssistantMessageCompletedEvent {
                    run_id: ctx.run_id,
                    message_id: assembly.assistant_message_id(),
                    content: result_inject::assistant_tool_content(
                        assembly.assistant_text().to_owned(),
                        assembly.tool_calls(),
                    ),
                    tool_uses: assembly
                        .tool_calls()
                        .iter()
                        .map(|call| harness_contracts::ToolUseSummary {
                            tool_use_id: call.tool_use_id,
                            tool_name: call.tool_name.clone(),
                        })
                        .collect(),
                    usage: usage.clone(),
                    pricing_snapshot_id: pricing_snapshot_id.clone(),
                    stop_reason: assembly.stop_reason(),
                    at: harness_contracts::now(),
                },
            )],
        )
        .await?;
        store_provider_continuations(engine, &session, &ctx, &request.provider_context, &assembly)
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

        for call in assembly.tool_calls() {
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

        let (tool_event_emitter, mut tool_event_receiver) = ChannelToolEventEmitter::channel();
        let tool_interrupt = InterruptToken::new();
        let orchestrator = ToolOrchestrator::default();
        let (authorized_tool_calls, mut authorization_failures) = authorize_tool_calls(
            engine,
            &session,
            &ctx,
            correlation_id,
            assembly.tool_calls(),
            &mut emitted,
        )
        .await?;
        let mut dispatch = Box::pin(
            orchestrator.dispatch(
                authorized_tool_calls,
                OrchestratorContext {
                    pool: engine.tools.clone(),
                    tool_context: harness_tool::ToolContext {
                        tool_use_id: ToolUseId::new(),
                        run_id: ctx.run_id,
                        session_id: session.session_id,
                        tenant_id: session.tenant_id,
                        model: ctx.model.clone(),
                        model_config_id: ctx.model_config_id.clone(),
                        memory_thread_settings: run_context_memory_thread_settings(&ctx),
                        correlation_id,
                        agent_id: harness_contracts::AgentId::from_u128(1),
                        subagent_depth: ctx.subagent_depth,
                        workspace_root: engine.workspace_root.clone(),
                        project_workspace_root: engine.project_workspace_root.clone(),
                        sandbox: engine.sandbox.clone(),
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
                        actor_source: ctx.permission_actor_source.clone(),
                    },
                    blob_store: engine.blob_store.clone(),
                    event_emitter: tool_event_emitter,
                },
            ),
        );
        let tool_results = loop {
            tokio::select! {
                results = &mut dispatch => break results,
                cause = ctx.cancellation.cancelled() => {
                while let Ok(event) = tool_event_receiver.try_recv() {                    append(
                        engine,
                        session.tenant_id,
                        session.session_id,
                        &mut emitted,
                        vec![event],
                    )
                    .await?;
                }                tool_interrupt.interrupt();
                let interrupt_grace = tokio::time::sleep(Duration::from_secs(5));
                tokio::pin!(interrupt_grace);
                loop {
                    tokio::select! {
                        results = &mut dispatch => {
                            drop(results);
                            break;
                        }
                        Some(event) = tool_event_receiver.recv() => {                            append(
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
                while let Ok(event) = tool_event_receiver.try_recv() {                    append(
                        engine,
                        session.tenant_id,
                        session.session_id,
                        &mut emitted,
                        vec![event],
                    )
                    .await?;
                }                append_run_end(
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
                Some(event) = tool_event_receiver.recv() => {                    append(
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
            append(
                engine,
                session.tenant_id,
                session.session_id,
                &mut emitted,
                vec![event],
            )
            .await?;
        }
        authorization_failures.extend(tool_results);
        let mut tool_results = authorization_failures;
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
        dispatched_tool_calls = dispatched_tool_calls
            .saturating_add(assembly.tool_calls().len().try_into().unwrap_or(u64::MAX));
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
            #[cfg(feature = "recall-memory")]
            memory_thread_settings: ctx.memory_thread_settings.clone(),
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
    events: Vec<Event>,
    blocked_reason: Option<String>,
}

async fn apply_pre_tool_use_hooks(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
    calls: &[ToolCall],
    messages: &[Message],
) -> Result<PreToolUseApplication, EngineError> {
    let mut staged_calls = Vec::with_capacity(calls.len());
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
                    events,
                    blocked_reason: Some(reason),
                });
            }
            HookOutcome::PreToolUse(outcome) => {
                if let Some(reason) = outcome.block {
                    return Ok(PreToolUseApplication {
                        calls: calls.to_vec(),
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

async fn authorize_tool_calls(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
    correlation_id: CorrelationId,
    tool_calls: &[ToolCall],
    _emitted: &mut Vec<Event>,
) -> Result<(Vec<AuthorizedToolCall>, Vec<RuntimeToolResultEnvelope>), EngineError> {
    let auth_context = AuthorizationContext {
        tenant_id: session.tenant_id,
        session_id: session.session_id,
        run_id: ctx.run_id,
        permission_mode: ctx.permission_mode,
        interactivity: ctx.interactivity,
        fallback_policy: FallbackPolicy::AskUser,
        workspace_root: engine.workspace_root.clone(),
    };

    let mut authorized = Vec::new();
    let mut failures = Vec::new();
    for call in tool_calls {
        let result = async {
            let tool = engine.tools.get(&call.tool_name).ok_or_else(|| {
                ToolError::Internal(format!("tool not found: {}", call.tool_name))
            })?;
            let tool_ctx = harness_tool::ToolContext {
                tool_use_id: call.tool_use_id,
                run_id: ctx.run_id,
                session_id: session.session_id,
                tenant_id: session.tenant_id,
                model: ctx.model.clone(),
                model_config_id: ctx.model_config_id.clone(),
                memory_thread_settings: run_context_memory_thread_settings(ctx),
                correlation_id,
                agent_id: harness_contracts::AgentId::from_u128(1),
                subagent_depth: ctx.subagent_depth,
                workspace_root: engine.workspace_root.clone(),
                project_workspace_root: engine.project_workspace_root.clone(),
                sandbox: engine.sandbox.clone(),
                cap_registry: engine.cap_registry.clone(),
                redactor: engine
                    .observer
                    .as_ref()
                    .map(|observer| Arc::clone(&observer.redactor))
                    .unwrap_or_else(|| Arc::new(DefaultRedactor::default())),
                interrupt: InterruptToken::new(),
                parent_run: ctx
                    .parent_run_id
                    .map(|run_id| harness_tool::ParentRunHandle {
                        run_id,
                        session_id: session.session_id,
                    }),
                actor_source: ctx.permission_actor_source.clone(),
            };
            tool.validate(&call.input, &tool_ctx)
                .await
                .map_err(|error| ToolError::Validation(error.to_string()))?;
            let plan = tool.plan(&call.input, &tool_ctx).await?;
            let authorized_input = engine
                .authorization_service
                .authorize_tool_input(auth_context.clone(), plan, call.input.clone())
                .await
                .map_err(authorization_error_to_tool_error)?;
            Ok::<AuthorizedToolCall, ToolError>(AuthorizedToolCall {
                tool_use_id: call.tool_use_id,
                tool_name: call.tool_name.clone(),
                input: authorized_input,
            })
        }
        .await;
        match result {
            Ok(call) => authorized.push(call),
            Err(error) => failures.push(authorization_failure_result(call, error)),
        }
    }
    Ok((authorized, failures))
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
                attachments: attachments_from_turn_metadata(&next_input.metadata),
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

fn attachments_from_turn_metadata(metadata: &Value) -> Vec<ConversationAttachmentReference> {
    metadata
        .get("attachments")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
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

fn soft_budget_trigger_tokens(budget: TokenBudget) -> u64 {
    if budget.max_tokens_per_turn == 0 {
        return 1;
    }
    ((budget.max_tokens_per_turn as f64) * f64::from(budget.soft_budget_ratio))
        .ceil()
        .max(1.0) as u64
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

async fn infer_or_interrupt(
    engine: &Engine,
    session: &SessionHandle,
    emitted: &mut Vec<Event>,
    ctx: &RunContext,
    request: ModelRequest,
    infer_ctx: InferContext,
    usage: UsageSnapshot,
) -> Result<Option<Result<harness_model::ModelStream, ModelError>>, EngineError> {
    tokio::select! {
        result = engine.model.infer(request, infer_ctx) => Ok(Some(result)),
        cause = ctx.cancellation.cancelled() => {
            append_run_end(
                engine,
                session,
                emitted,
                ctx.run_id,
                end_reason_for_interrupt(cause),
                usage,
            )
            .await?;
            Ok(None)
        }
    }
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

fn run_context_memory_thread_settings(
    ctx: &RunContext,
) -> Option<harness_contracts::MemoryThreadSettings> {
    #[cfg(feature = "recall-memory")]
    {
        ctx.memory_thread_settings.clone()
    }
    #[cfg(not(feature = "recall-memory"))]
    {
        let _ = ctx;
        None
    }
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

fn provider_continuation_query_for_prompt(
    model_snapshot: &ModelRuntimeSnapshot,
    model_config_id: Option<String>,
    tenant_id: TenantId,
    session_id: SessionId,
    final_messages: &[Message],
) -> Option<ProviderContinuationQuery> {
    let (continuation_kind, _) = private_replay_kind(model_snapshot)?;
    let message_ids = assistant_tool_replay_message_ids(final_messages);
    if message_ids.is_empty() {
        return None;
    }

    Some(ProviderContinuationQuery {
        provider_id: model_snapshot.provider_id.clone(),
        model_config_id,
        protocol: model_snapshot.protocol,
        dialect: provider_continuation_dialect(model_snapshot),
        tenant_id,
        session_id,
        message_ids,
        kinds: vec![continuation_kind],
    })
}

async fn provider_request_context_for_prompt(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
    final_messages: &[Message],
    tools_can_produce_assistant_tool_replay: bool,
) -> Result<ProviderRequestContext, EngineError> {
    let model_config_id = ctx.model_config_id.clone();
    let dialect = provider_continuation_dialect(&engine.model_snapshot);
    let mut provider_context = ProviderRequestContext {
        provider_id: engine.model_snapshot.provider_id.clone(),
        model_config_id: model_config_id.clone(),
        dialect: Some(dialect),
        continuations: Vec::new(),
    };

    let Some((continuation_kind, replay_required)) = private_replay_kind(&engine.model_snapshot)
    else {
        return Ok(provider_context);
    };

    let replay_message_ids = assistant_tool_replay_message_ids(final_messages);
    let requires_store = replay_required
        && (!replay_message_ids.is_empty() || tools_can_produce_assistant_tool_replay);
    let Some(store) = engine.provider_continuation_store.as_ref() else {
        if requires_store {
            return Err(engine_error(ModelError::InvalidRequest(
                MISSING_PROVIDER_CONTINUATION_ERROR.to_owned(),
            )));
        }
        return Ok(provider_context);
    };

    let Some(query) = provider_continuation_query_for_prompt(
        &engine.model_snapshot,
        model_config_id,
        session.tenant_id,
        session.session_id,
        final_messages,
    ) else {
        return Ok(provider_context);
    };

    let records = store.load_for_messages(query).await.map_err(engine_error)?;
    if !replay_required {
        provider_context.continuations = records;
        return Ok(provider_context);
    }

    let exact_matches: HashSet<(MessageId, ProviderContinuationKind)> = records
        .iter()
        .map(|record| (record.message_id, record.kind.clone()))
        .collect();
    for message_id in replay_message_ids {
        if !exact_matches.contains(&(message_id, continuation_kind.clone())) {
            return Err(engine_error(ModelError::InvalidRequest(
                MISSING_PROVIDER_CONTINUATION_ERROR.to_owned(),
            )));
        }
    }
    provider_context.continuations = records;
    Ok(provider_context)
}

fn required_private_replay_kind(
    model_snapshot: &ModelRuntimeSnapshot,
) -> Option<ProviderContinuationKind> {
    private_replay_kind(model_snapshot)
        .filter(|(_, replay_required)| *replay_required)
        .map(|(continuation_kind, _)| continuation_kind)
}

fn private_replay_kind(
    model_snapshot: &ModelRuntimeSnapshot,
) -> Option<(ProviderContinuationKind, bool)> {
    match &model_snapshot.runtime_semantics.reasoning_protocol {
        ReasoningProtocolSemantics::ProviderPrivateReplay {
            continuation_kind,
            required_for_assistant_tool_replay,
        } => Some((
            continuation_kind.clone(),
            *required_for_assistant_tool_replay,
        )),
        _ => None,
    }
}

fn provider_continuation_dialect(model_snapshot: &ModelRuntimeSnapshot) -> String {
    model_snapshot
        .runtime_semantics
        .provider_continuation_dialect
        .clone()
        .unwrap_or_else(|| model_snapshot.provider_id.clone())
}

fn assistant_tool_replay_message_ids(messages: &[Message]) -> Vec<MessageId> {
    messages
        .iter()
        .filter(|message| {
            message.role == MessageRole::Assistant
                && message
                    .parts
                    .iter()
                    .any(|part| matches!(part, MessagePart::ToolUse { .. }))
        })
        .map(|message| message.id)
        .collect()
}

async fn store_provider_continuations(
    engine: &Engine,
    session: &SessionHandle,
    ctx: &RunContext,
    request_context: &ProviderRequestContext,
    assembly: &TurnAssembly,
) -> Result<(), EngineError> {
    if assembly.provider_continuations().is_empty() {
        return Ok(());
    }
    let Some(store) = engine.provider_continuation_store.as_ref() else {
        return Err(engine_error(ModelError::InvalidRequest(
            MISSING_PROVIDER_CONTINUATION_ERROR.to_owned(),
        )));
    };

    let dialect = request_context
        .dialect
        .clone()
        .unwrap_or_else(|| provider_continuation_dialect(&engine.model_snapshot));
    let records = assembly
        .provider_continuations()
        .iter()
        .map(|capture| ProviderContinuationRecord {
            provider_id: engine.model_snapshot.provider_id.clone(),
            model_config_id: ctx.model_config_id.clone(),
            protocol: engine.model_snapshot.protocol,
            dialect: dialect.clone(),
            tenant_id: session.tenant_id,
            session_id: session.session_id,
            producing_run_id: ctx.run_id,
            message_id: assembly.assistant_message_id(),
            scope: ProviderContinuationScope::Conversation,
            kind: capture.kind.clone(),
            payload: capture.payload.clone(),
            created_at: harness_contracts::now(),
        })
        .collect();
    store.append_batch(records).await.map_err(engine_error)
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
    #[cfg(feature = "recall-memory")]
    memory_thread_settings: Option<MemoryThreadSettings>,
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

    #[cfg(feature = "recall-memory")]
    fn memory_thread_settings(&self) -> Option<MemoryThreadSettings> {
        self.memory_thread_settings.clone()
    }
}

struct TurnHookView {
    workspace_root: Option<PathBuf>,
    messages: Vec<Message>,
    permission_mode: PermissionMode,
    redactor: Arc<dyn Redactor>,
}

impl HookSessionView for TurnHookView {
    fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
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
            workspace_root: engine.project_workspace_root.clone(),
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
        ToolResultPart::Artifact {
            artifact_kind,
            content_type,
            blob_ref,
            title,
            preview,
        } => ToolResultPart::Artifact {
            artifact_kind,
            content_type,
            blob_ref,
            title: redactor.redact(&title, &RedactRules::default()),
            preview: preview.map(|text| redactor.redact(&text, &RedactRules::default())),
        },
        part => part,
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
            if let Some(artifact) = artifact_from_tool_result(tool_result) {
                events.push(Event::ArtifactCreated(ArtifactCreatedEvent {
                    revision_id: ArtifactRevisionId::new(),
                    session_id,
                    run_id,
                    artifact_id: format!("artifact:{}", result.tool_use_id),
                    title: artifact.title,
                    kind: artifact.kind,
                    status: ArtifactStatus::Ready,
                    source: ArtifactSource::Tool,
                    source_message_id: None,
                    source_tool_use_id: Some(result.tool_use_id),
                    content_hash: Some(artifact.blob_ref.content_hash.to_vec()),
                    blob_ref: Some(artifact.blob_ref),
                    preview: artifact.preview,
                    at,
                }));
            }
            events
        }
        Err(ToolError::PermissionDenied(_)) => {
            vec![Event::ToolUseDenied(ToolUseDeniedEvent {
                tool_use_id: result.tool_use_id,
                reason: DenyReason::PolicyDenied,
                at: harness_contracts::now(),
            })]
        }
        Err(error) => vec![Event::ToolUseFailed(ToolUseFailedEvent {
            tool_use_id: result.tool_use_id,
            error: tool_error_payload(error),
            at: harness_contracts::now(),
        })],
    }
}

struct TypedArtifactOutput {
    kind: String,
    blob_ref: BlobRef,
    title: String,
    preview: Option<String>,
}

fn artifact_from_tool_result(result: &ToolResult) -> Option<TypedArtifactOutput> {
    let ToolResult::Mixed(parts) = result else {
        return None;
    };
    let artifact = parts.iter().find_map(|part| match part {
        ToolResultPart::Artifact {
            artifact_kind,
            content_type,
            blob_ref,
            title,
            preview,
        } => validated_typed_artifact_output(
            *artifact_kind,
            content_type,
            blob_ref,
            title,
            preview.as_deref(),
        ),
        _ => None,
    })?;
    Some(artifact)
}

fn validated_typed_artifact_output(
    artifact_kind: ModelModality,
    content_type: &str,
    blob_ref: &BlobRef,
    title: &str,
    preview: Option<&str>,
) -> Option<TypedArtifactOutput> {
    let kind = artifact_kind_label(artifact_kind)?;
    if !artifact_content_type_matches_kind(kind, content_type) {
        return None;
    }
    if !artifact_content_type_matches_kind(
        kind,
        blob_ref.content_type.as_deref().unwrap_or(content_type),
    ) {
        return None;
    }
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    Some(TypedArtifactOutput {
        kind: kind.to_owned(),
        blob_ref: blob_ref.clone(),
        title: title.to_owned(),
        preview: preview
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned),
    })
}

fn artifact_kind_label(kind: ModelModality) -> Option<&'static str> {
    match kind {
        ModelModality::Image => Some("image"),
        ModelModality::Video => Some("video"),
        ModelModality::Audio => Some("audio"),
        ModelModality::File => Some("file"),
        ModelModality::Text | ModelModality::Embedding => None,
    }
}

fn artifact_content_type_matches_kind(kind: &str, content_type: &str) -> bool {
    let mime = normalized_mime_type(content_type);
    match kind {
        "image" => is_safe_image_content_type(&mime),
        "video" => matches!(
            mime.as_str(),
            "video/mp4" | "video/webm" | "video/quicktime"
        ),
        "audio" => matches!(
            mime.as_str(),
            "audio/mpeg" | "audio/mp4" | "audio/ogg" | "audio/wav" | "audio/webm"
        ),
        "file" => matches!(
            mime.as_str(),
            "text/plain"
                | "text/markdown"
                | "text/csv"
                | "application/json"
                | "application/pdf"
                | "application/zip"
                | "application/octet-stream"
        ),
        _ => false,
    }
}

fn normalized_mime_type(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase()
}

fn is_safe_image_content_type(content_type: &str) -> bool {
    matches!(
        normalized_mime_type(content_type).as_str(),
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

fn final_model_request_token_estimate(request: &ModelRequest) -> u64 {
    let mut chars = request.system.as_deref().map(str::len).unwrap_or_default();
    for message in &request.messages {
        for part in &message.parts {
            chars += match part {
                MessagePart::Text(text) => text.len(),
                MessagePart::Image { .. } => 512,
                MessagePart::Video { .. } | MessagePart::File { .. } => 0,
                MessagePart::ToolUse { input, .. } => input.to_string().len(),
                MessagePart::ToolResult { content, .. } => format!("{content:?}").len(),
                MessagePart::Thinking(thinking) => {
                    thinking.text.as_deref().map(str::len).unwrap_or(0)
                }
                _ => 0,
            };
        }
    }
    std::cmp::max(1, chars.div_ceil(4) as u64)
}

async fn record_model_request_preview(
    engine: &Engine,
    _tenant_id: TenantId,
    session_id: SessionId,
    run_id: RunId,
    request: &ModelRequest,
    token_estimate: u64,
    trace_id: Option<MemoryTraceId>,
) {
    let Some(sink) = &engine.model_request_preview_sink else {
        return;
    };
    let preview =
        model_request_preview_from_request(session_id, run_id, request, token_estimate, trace_id);
    let _ = sink.record_model_request_preview(preview).await;
}

fn model_request_preview_from_request(
    session_id: SessionId,
    run_id: RunId,
    request: &ModelRequest,
    token_estimate: u64,
    trace_id: Option<MemoryTraceId>,
) -> MemoryModelRequestPreview {
    let mut sections = Vec::new();
    if let Some(system) = request.system.as_deref() {
        sections.push(MemoryModelRequestPreviewSection {
            source: MemorySource::Imported,
            provider_id: None,
            memory_ids: Vec::new(),
            redacted_content: format!("[redacted system section: chars={}]", system.len()),
        });
    }
    for message in &request.messages {
        let text = message_text(message);
        if text.contains("<memory-context>") {
            let memory_references = memory_references_from_memory_context(&text);
            let provider_id = memory_context_provider_id(&memory_references);
            sections.push(MemoryModelRequestPreviewSection {
                source: MemorySource::ExternalRetrieval,
                provider_id,
                memory_ids: memory_references
                    .iter()
                    .map(|reference| reference.memory_id)
                    .collect(),
                redacted_content: format!(
                    "[redacted memory context message: role={:?}, chars={}]",
                    message.role,
                    text.len()
                ),
            });
        }
    }
    let mut tool_names = request
        .tools
        .as_ref()
        .map(|tools| {
            tools
                .iter()
                .map(|tool| tool.name.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    tool_names.sort();
    tool_names.dedup();
    let redacted_count = sections.len() as u32;
    let content_hash = model_request_preview_hash(&sections, &tool_names, token_estimate);
    MemoryModelRequestPreview {
        session_id,
        run_id,
        trace_id,
        sections,
        redacted_count,
        token_estimate,
        tool_names,
        policy_decisions: Vec::new(),
        content_hash,
    }
}

fn latest_memory_trace_id(events: &[Event]) -> Option<MemoryTraceId> {
    events.iter().rev().find_map(|event| match event {
        Event::MemoryRecalled(event) => event.trace_id,
        _ => None,
    })
}

#[derive(Debug, Clone)]
struct MemoryContextReference {
    memory_id: MemoryId,
    provider_id: Option<String>,
}

fn memory_references_from_memory_context(text: &str) -> Vec<MemoryContextReference> {
    let mut references = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("## [reference|memory|") else {
            continue;
        };
        let Some(header) = rest.strip_suffix(']') else {
            continue;
        };
        let parts = header.split('|').collect::<Vec<_>>();
        let Some(id_text) = parts.first().copied() else {
            continue;
        };
        if let Ok(id) = MemoryId::parse(id_text) {
            let provider_id = (parts.get(1) == Some(&"provider"))
                .then(|| parts.get(2).map(|value| (*value).to_owned()))
                .flatten()
                .filter(|value| !value.is_empty());
            references.push(MemoryContextReference {
                memory_id: id,
                provider_id,
            });
        }
    }
    references.sort_by_key(|reference| reference.memory_id.to_string());
    references.dedup_by_key(|reference| reference.memory_id);
    references
}

fn memory_context_provider_id(references: &[MemoryContextReference]) -> Option<String> {
    let mut provider_ids = references
        .iter()
        .filter_map(|reference| reference.provider_id.as_deref())
        .collect::<Vec<_>>();
    provider_ids.sort_unstable();
    provider_ids.dedup();
    match provider_ids.as_slice() {
        [provider_id] => Some((*provider_id).to_owned()),
        _ => None,
    }
}

fn model_request_preview_hash(
    sections: &[MemoryModelRequestPreviewSection],
    tool_names: &[String],
    token_estimate: u64,
) -> ContentHash {
    let mut hasher = blake3::Hasher::new();
    for section in sections {
        hasher.update(format!("{:?}", section.source).as_bytes());
        if let Some(provider_id) = &section.provider_id {
            hasher.update(provider_id.as_bytes());
        }
        for memory_id in &section.memory_ids {
            hasher.update(memory_id.to_string().as_bytes());
        }
        hasher.update(section.redacted_content.as_bytes());
    }
    for tool_name in tool_names {
        hasher.update(tool_name.as_bytes());
    }
    hasher.update(&token_estimate.to_be_bytes());
    let hash = hasher.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(hash.as_bytes());
    ContentHash(bytes)
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
            diagnostic: false,
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
        ModelError::InsufficientBalance(_) => "insufficient_balance",
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

#[cfg(test)]
mod artifact_tests {
    use super::*;
    use harness_contracts::{BlobId, BlobRef, ModelModality, ToolResult, ToolResultPart};

    fn image_blob_ref() -> BlobRef {
        BlobRef {
            id: BlobId::new(),
            size: 128,
            content_hash: [7; 32],
            content_type: Some("image/png".to_owned()),
        }
    }

    #[test]
    fn artifact_from_tool_result_accepts_typed_image_output() {
        let result = ToolResult::Mixed(vec![ToolResultPart::Artifact {
            artifact_kind: ModelModality::Image,
            content_type: "image/png".to_owned(),
            blob_ref: image_blob_ref(),
            title: "Generated image".to_owned(),
            preview: Some("Generated image".to_owned()),
        }]);
        let artifact = artifact_from_tool_result(&result).expect("typed image artifact");
        assert_eq!(artifact.kind, "image");
        assert_eq!(artifact.title, "Generated image");
    }

    #[test]
    fn artifact_from_tool_result_accepts_typed_video_output() {
        let result = ToolResult::Mixed(vec![ToolResultPart::Artifact {
            artifact_kind: ModelModality::Video,
            content_type: "video/mp4".to_owned(),
            blob_ref: BlobRef {
                id: BlobId::new(),
                size: 2048,
                content_hash: [8; 32],
                content_type: Some("video/mp4".to_owned()),
            },
            title: "Generated video".to_owned(),
            preview: None,
        }]);
        let artifact = artifact_from_tool_result(&result).expect("typed video artifact");
        assert_eq!(artifact.kind, "video");
    }

    #[test]
    fn artifact_from_tool_result_rejects_mismatched_content_type() {
        let result = ToolResult::Mixed(vec![ToolResultPart::Artifact {
            artifact_kind: ModelModality::Image,
            content_type: "video/mp4".to_owned(),
            blob_ref: BlobRef {
                id: BlobId::new(),
                size: 128,
                content_hash: [9; 32],
                content_type: Some("video/mp4".to_owned()),
            },
            title: "Bad image".to_owned(),
            preview: None,
        }]);
        assert!(artifact_from_tool_result(&result).is_none());
    }
}

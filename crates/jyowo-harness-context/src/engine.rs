use std::collections::{HashMap, HashSet};
#[cfg(feature = "recall-memory")]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
#[cfg(feature = "recall-memory")]
use std::time::Duration;
use std::time::Instant;

#[cfg(feature = "recall-memory")]
use harness_contracts::MemoryActor;
#[cfg(feature = "recall-memory")]
use harness_contracts::RunId;
use harness_contracts::{
    BlobStore, BudgetExceedanceSource, BudgetKind, ContextBudgetExceededEvent, ContextError,
    ContextPatchAppliedEvent, ContextPatchLifecycle, ContextPatchRequest, ContextPatchSinkCap,
    ContextPatchSource, ContextStageId, ContextStageOutcome, ContextStageTransitionedEvent,
    DeferredToolsDeltaAttachment, Event, Message, MessageId, MessagePart, MessageRole, SessionId,
    SkillInvokedEvent, TenantId, ToolDescriptor, ToolError, ToolResultEnvelope, TurnInput,
};
#[cfg(feature = "recall-memory")]
use harness_contracts::{
    MemoryRecallDegradedEvent, MemoryRecallDegradedReason, MessageView, UserMessageView,
};
#[cfg(feature = "recall-memory")]
use harness_memory::{
    MemoryKindFilter, MemoryManager, MemoryQuery, MemoryRecallOutcome, MemoryVisibilityFilter,
};
use harness_model::{
    AuxModelProvider, BreakpointReason, CacheBreakpoint, ModelMetricsSink, PromptCacheStyle,
};
use parking_lot::Mutex;

use crate::{
    AssembledPrompt, AutocompactProvider, CollapseProvider, CompactHint, ContentLifecycle,
    ContextBuffer, ContextOutcome, ContextPatch, ContextProvider, ContextSessionView,
    FrozenContext, MicrocompactProvider, PromptCachePolicy, SnipProvider, TokenBudget,
    ToolResultBudgetProvider,
};

const COMPACT_STAGE_ORDER: [ContextStageId; 5] = [
    ContextStageId::ToolResultBudget,
    ContextStageId::Snip,
    ContextStageId::Microcompact,
    ContextStageId::Collapse,
    ContextStageId::Autocompact,
];
const DEFAULT_TOOL_RESULT_BUDGET_CHARS: u64 = 16 * 1024;
const DEFAULT_COLLAPSE_THRESHOLD_CHARS: usize = 24 * 1024;

#[derive(Clone)]
pub struct ContextEngine {
    providers: Vec<Arc<dyn ContextProvider>>,
    budget: TokenBudget,
    cache_policy: PromptCachePolicy,
    state: Arc<Mutex<HashMap<ContextStateKey, StoredContext>>>,
    #[cfg(feature = "recall-memory")]
    turn_counter: Arc<AtomicU64>,
    #[cfg(feature = "recall-memory")]
    memory_manager: Option<Arc<MemoryManager>>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
struct ContextStateKey {
    tenant_id: TenantId,
    session_id: SessionId,
}

#[derive(Debug, Clone, Default)]
struct StoredContext {
    buffer: ContextBuffer,
    pending_events: Vec<Event>,
    #[cfg(feature = "recall-memory")]
    active_turn: Option<TurnIdentity>,
}

#[cfg(feature = "recall-memory")]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct TurnIdentity {
    run_id: RunId,
    turn: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmergencyCompactPrompt {
    pub prompt: AssembledPrompt,
    pub outcome: ContextOutcome,
}

impl ContextEngine {
    pub fn builder() -> ContextEngineBuilder {
        ContextEngineBuilder::default()
    }

    #[must_use]
    pub fn clone_with_budget(&self, budget: TokenBudget) -> Self {
        Self {
            providers: self.providers.clone(),
            budget,
            cache_policy: self.cache_policy.clone(),
            state: self.state.clone(),
            #[cfg(feature = "recall-memory")]
            turn_counter: self.turn_counter.clone(),
            #[cfg(feature = "recall-memory")]
            memory_manager: self.memory_manager.clone(),
        }
    }

    pub fn compact_stage_order() -> &'static [ContextStageId] {
        &COMPACT_STAGE_ORDER
    }

    pub async fn compact(
        &self,
        ctx: &mut ContextBuffer,
        hint: CompactHint,
    ) -> Result<ContextOutcome, ContextError> {
        self.compact_internal(ctx, hint, false)
            .await
            .map(|outcome| outcome.0)
    }

    async fn compact_internal(
        &self,
        ctx: &mut ContextBuffer,
        hint: CompactHint,
        collect_events: bool,
    ) -> Result<(ContextOutcome, Vec<Event>), ContextError> {
        let mut bytes_saved = 0_u64;
        let mut modified = false;
        let mut events = Vec::new();

        for stage in &COMPACT_STAGE_ORDER {
            let stage_providers = self
                .providers
                .iter()
                .filter(|provider| provider.stage() == *stage)
                .collect::<Vec<_>>();
            if stage_providers.is_empty() {
                if collect_events {
                    events.push(Event::ContextStageTransitioned(
                        ContextStageTransitionedEvent {
                            session_id: ctx.identity.session_id,
                            stage: stage.clone(),
                            provider_id: skipped_provider_id(stage).to_owned(),
                            outcome: ContextStageOutcome::SkippedNoAuxProvider,
                            before_tokens: ctx.bookkeeping.estimated_tokens,
                            after_tokens: ctx.bookkeeping.estimated_tokens,
                            bytes_saved: 0,
                            duration_ms: 0,
                            at: harness_contracts::now(),
                        },
                    ));
                }
                continue;
            }

            for provider in stage_providers {
                let frozen_before = ctx.frozen.clone();
                let before_tokens = ctx.bookkeeping.estimated_tokens;
                let started = Instant::now();
                let outcome = provider.apply(ctx, &hint).await?;
                self.refresh_context(ctx, &frozen_before)?;
                let stage_bytes_saved = match &outcome {
                    ContextOutcome::Modified { bytes_saved } => *bytes_saved,
                    _ => 0,
                };
                if collect_events {
                    events.push(Event::ContextStageTransitioned(
                        ContextStageTransitionedEvent {
                            session_id: ctx.identity.session_id,
                            stage: provider.stage(),
                            provider_id: provider.provider_id().to_owned(),
                            outcome: context_stage_outcome(&outcome),
                            before_tokens,
                            after_tokens: ctx.bookkeeping.estimated_tokens,
                            bytes_saved: stage_bytes_saved,
                            duration_ms: duration_ms(started),
                            at: harness_contracts::now(),
                        },
                    ));
                }

                match outcome {
                    ContextOutcome::NoChange => {}
                    ContextOutcome::Modified { bytes_saved: saved } => {
                        modified = true;
                        bytes_saved = bytes_saved.saturating_add(saved);
                    }
                    forked @ ContextOutcome::Forked { .. } => return Ok((forked, events)),
                }
            }
        }

        if modified {
            Ok((ContextOutcome::Modified { bytes_saved }, events))
        } else {
            Ok((ContextOutcome::NoChange, events))
        }
    }

    fn refresh_context(
        &self,
        ctx: &mut ContextBuffer,
        frozen_before: &FrozenContext,
    ) -> Result<(), ContextError> {
        if &ctx.frozen != frozen_before {
            return Err(ContextError::Internal(
                "context provider mutated frozen context".to_owned(),
            ));
        }

        ctx.rebuild_tool_use_pairs();
        ctx.bookkeeping.estimated_tokens =
            estimate_tokens(ctx.frozen.system_header.as_deref(), &ctx.active.history);
        ctx.bookkeeping.budget_snapshot = self.budget;
        Ok(())
    }

    pub async fn assemble(
        &self,
        session: &dyn ContextSessionView,
        turn_input: &TurnInput,
    ) -> Result<AssembledPrompt, ContextError> {
        let key = context_state_key(session.tenant_id(), session.session_id());
        #[cfg(feature = "recall-memory")]
        let turn_identity = self.turn_identity(turn_input);
        let mut messages = session.messages();
        let mut turn_message = turn_input.message.clone();
        sanitize_turn_message(&mut turn_message);
        let (mut patches, mut events) = (Vec::new(), Vec::new());
        #[cfg(feature = "recall-memory")]
        self.call_memory_turn_start(session, &turn_message, turn_identity, &mut events)
            .await;
        #[cfg(feature = "recall-memory")]
        let (recall_patches, recall_events) = self
            .memory_recall_patch(session, &turn_message, turn_identity)
            .await;
        #[cfg(not(feature = "recall-memory"))]
        let (recall_patches, recall_events) = self
            .memory_recall_patch(session, turn_input, &turn_message)
            .await;
        patches.extend(recall_patches);
        events.extend(recall_events);
        let (stored_patches, stored_events) = self.take_pending_patches(key)?;
        patches.extend(stored_patches);
        events.extend(stored_events);

        if turn_message.role == MessageRole::User {
            apply_patches_to_user_message(&mut turn_message, &patches);
            messages.push(turn_message);
        } else {
            messages.push(turn_message);
            if !patches.is_empty() {
                messages.push(patch_carrier_message(&patches));
            }
        }

        let tokens_estimate = estimate_tokens(session.system().as_deref(), &messages);
        let budget_utilization =
            budget_utilization(tokens_estimate, self.budget.max_tokens_per_turn);
        self.store_assembled_prompt(key, session, &messages, tokens_estimate)?;
        #[cfg(feature = "recall-memory")]
        self.store_active_turn_identity(key, turn_identity)?;

        Ok(AssembledPrompt {
            cache_breakpoints: self
                .select_cache_breakpoints(session.system().as_deref(), &messages),
            messages,
            system: session.system(),
            tools_snapshot: session.tools_snapshot(),
            tokens_estimate,
            budget_utilization,
            events,
        })
    }

    pub async fn after_turn(
        &self,
        session: &dyn ContextSessionView,
        results: &[ToolResultEnvelope],
    ) -> Result<ContextOutcome, ContextError> {
        let key = context_state_key(session.tenant_id(), session.session_id());
        {
            let mut state = self.state.lock();
            let stored = state.entry(key).or_insert_with(|| StoredContext {
                buffer: ContextBuffer::new(key.tenant_id, key.session_id),
                pending_events: Vec::new(),
                #[cfg(feature = "recall-memory")]
                active_turn: None,
            });
            stored.buffer.active.history = session.messages();
            stored
                .buffer
                .active
                .history
                .extend(results.iter().map(tool_result_message).collect::<Vec<_>>());
            stored.buffer.rebuild_tool_use_pairs();
            stored.buffer.bookkeeping.estimated_tokens = estimate_tokens(
                stored.buffer.frozen.system_header.as_deref(),
                &stored.buffer.active.history,
            );
            stored.buffer.bookkeeping.budget_snapshot = self.budget;
        }
        #[cfg(feature = "recall-memory")]
        self.enqueue_tool_result_memory_recall(session, results)
            .await?;
        Ok(ContextOutcome::Modified { bytes_saved: 0 })
    }

    fn take_pending_patches(
        &self,
        key: ContextStateKey,
    ) -> Result<(Vec<ContextPatch>, Vec<Event>), ContextError> {
        let mut state = self.state.lock();
        let stored = state.entry(key).or_insert_with(|| StoredContext {
            buffer: ContextBuffer::new(key.tenant_id, key.session_id),
            pending_events: Vec::new(),
            #[cfg(feature = "recall-memory")]
            active_turn: None,
        });
        let patches = stored.buffer.patches.clone();
        expire_persistent_patches(&mut stored.buffer.patches);
        let events = std::mem::take(&mut stored.pending_events);
        Ok((patches, events))
    }

    fn store_assembled_prompt(
        &self,
        key: ContextStateKey,
        session: &dyn ContextSessionView,
        messages: &[Message],
        tokens_estimate: u64,
    ) -> Result<(), ContextError> {
        let mut state = self.state.lock();
        let stored = state.entry(key).or_insert_with(|| StoredContext {
            buffer: ContextBuffer::new(key.tenant_id, key.session_id),
            pending_events: Vec::new(),
            #[cfg(feature = "recall-memory")]
            active_turn: None,
        });
        stored.buffer.identity = crate::ContextIdentity {
            tenant_id: key.tenant_id,
            session_id: key.session_id,
        };
        stored.buffer.frozen.system_header = session.system().map(Arc::from);
        stored.buffer.frozen.tools_snapshot = Arc::new(crate::ContextToolSnapshot {
            descriptors: session.tools_snapshot(),
        });
        stored.buffer.active.history = messages.to_vec();
        stored.buffer.rebuild_tool_use_pairs();
        stored.buffer.bookkeeping.estimated_tokens = tokens_estimate;
        stored.buffer.bookkeeping.budget_snapshot = self.budget;
        Ok(())
    }

    fn push_patch_request(&self, request: ContextPatchRequest) -> Result<(), ToolError> {
        let key = ContextStateKey {
            tenant_id: request.tenant_id,
            session_id: request.session_id,
        };
        let (patch, event) = context_patch_from_request(request);
        let mut state = self.state.lock();
        let stored = state.entry(key).or_insert_with(|| StoredContext {
            buffer: ContextBuffer::new(key.tenant_id, key.session_id),
            pending_events: Vec::new(),
            #[cfg(feature = "recall-memory")]
            active_turn: None,
        });
        stored.buffer.patches.push(patch);
        if let Some(event) = event {
            stored.pending_events.push(event);
        }
        Ok(())
    }

    #[cfg(feature = "recall-memory")]
    fn store_active_turn_identity(
        &self,
        key: ContextStateKey,
        identity: TurnIdentity,
    ) -> Result<(), ContextError> {
        let mut state = self.state.lock();
        let stored = state.entry(key).or_insert_with(|| StoredContext {
            buffer: ContextBuffer::new(key.tenant_id, key.session_id),
            pending_events: Vec::new(),
            active_turn: None,
        });
        stored.active_turn = Some(identity);
        Ok(())
    }

    pub fn push_deferred_tools_delta(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        delta: DeferredToolsDeltaAttachment,
    ) -> Result<(), ContextError> {
        if delta.is_empty() {
            return Ok(());
        }
        let key = ContextStateKey {
            tenant_id,
            session_id,
        };
        let mut state = self.state.lock();
        let stored = state.entry(key).or_insert_with(|| StoredContext {
            buffer: ContextBuffer::new(key.tenant_id, key.session_id),
            pending_events: Vec::new(),
            #[cfg(feature = "recall-memory")]
            active_turn: None,
        });
        stored
            .buffer
            .patches
            .retain(|patch| !matches!(patch, ContextPatch::DeferredToolsDelta { .. }));
        stored.buffer.deferred_tools_delta = Some(delta.clone());
        stored
            .buffer
            .patches
            .push(ContextPatch::DeferredToolsDelta {
                body: delta.to_attachment_text(),
                lifecycle: ContentLifecycle::Transient,
            });
        Ok(())
    }

    pub async fn emergency_compact_prompt(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        system: Option<String>,
        tools_snapshot: Vec<ToolDescriptor>,
        messages: Vec<Message>,
        reported_tokens: u64,
        max_tokens: u64,
    ) -> Result<EmergencyCompactPrompt, ContextError> {
        let before_tokens = reported_tokens.max(estimate_tokens(system.as_deref(), &messages));
        let mut buffer = ContextBuffer::new(tenant_id, session_id);
        buffer.frozen.system_header = system.clone().map(Arc::from);
        buffer.frozen.tools_snapshot = Arc::new(crate::ContextToolSnapshot {
            descriptors: tools_snapshot.clone(),
        });
        buffer.active.history = messages;
        buffer.bookkeeping.estimated_tokens = before_tokens;
        buffer.bookkeeping.budget_snapshot = self.budget;
        buffer.rebuild_tool_use_pairs();
        let mut lifecycle_events = Vec::new();
        #[cfg(feature = "recall-memory")]
        if let Some(facts) = self
            .call_memory_pre_compress(
                session_id,
                RunId::new(),
                &buffer.active.history,
                &mut lifecycle_events,
            )
            .await
        {
            buffer
                .active
                .history
                .insert(0, memory_pre_compress_message(facts));
            buffer.rebuild_tool_use_pairs();
            buffer.bookkeeping.estimated_tokens = estimate_tokens(
                buffer.frozen.system_header.as_deref(),
                &buffer.active.history,
            );
        }
        let (outcome, mut events) = self
            .compact_internal(
                &mut buffer,
                CompactHint {
                    estimated_tokens: before_tokens,
                    target_tokens: Some(max_tokens.saturating_mul(9).saturating_div(10).max(1)),
                },
                true,
            )
            .await?;
        lifecycle_events.append(&mut events);
        let mut events = lifecycle_events;
        events.insert(
            0,
            Event::ContextBudgetExceeded(ContextBudgetExceededEvent {
                session_id,
                budget_kind: BudgetKind::PerTurnTokens,
                source: BudgetExceedanceSource::ProviderReport { reported_tokens },
                requested: reported_tokens,
                max: max_tokens,
                at: harness_contracts::now(),
            }),
        );
        let tokens_estimate = estimate_tokens(system.as_deref(), &buffer.active.history);
        Ok(EmergencyCompactPrompt {
            prompt: AssembledPrompt {
                cache_breakpoints: self
                    .select_cache_breakpoints(system.as_deref(), &buffer.active.history),
                messages: buffer.active.history,
                system,
                tools_snapshot,
                tokens_estimate,
                budget_utilization: budget_utilization(
                    tokens_estimate,
                    self.budget.max_tokens_per_turn,
                ),
                events,
            },
            outcome,
        })
    }

    fn select_cache_breakpoints(
        &self,
        _system: Option<&str>,
        messages: &[Message],
    ) -> Vec<CacheBreakpoint> {
        if self.cache_policy.max_breakpoints == 0
            || matches!(self.cache_policy.style, PromptCacheStyle::None)
        {
            return Vec::new();
        }

        match self.cache_policy.breakpoint_strategy {
            crate::BreakpointStrategy::SystemOnly => Vec::new(),
            crate::BreakpointStrategy::SystemAnd3 => {
                let limit = self.cache_policy.max_breakpoints.min(3);
                let mut selected = messages
                    .iter()
                    .filter(|message| message.role != MessageRole::System)
                    .rev()
                    .take(limit)
                    .map(|message| message.id)
                    .collect::<Vec<_>>();
                selected.reverse();
                breakpoints_from_ids(selected)
            }
            crate::BreakpointStrategy::EveryN(n) => {
                if n == 0 {
                    return Vec::new();
                }
                let mut seen = HashSet::new();
                let mut selected = Vec::new();
                for (index, message) in messages
                    .iter()
                    .filter(|message| message.role != MessageRole::System)
                    .enumerate()
                {
                    if (index + 1) % n != 0 || !seen.insert(message.id) {
                        continue;
                    }
                    selected.push(message.id);
                    if selected.len() == self.cache_policy.max_breakpoints {
                        break;
                    }
                }
                breakpoints_from_ids(selected)
            }
        }
    }
}

impl ContextPatchSinkCap for ContextEngine {
    fn push_patch(
        &self,
        request: ContextPatchRequest,
    ) -> futures::future::BoxFuture<'static, Result<(), ToolError>> {
        let engine = self.clone();
        Box::pin(async move { engine.push_patch_request(request) })
    }
}

fn breakpoints_from_ids(ids: Vec<harness_contracts::MessageId>) -> Vec<CacheBreakpoint> {
    ids.into_iter()
        .map(|after_message_id| CacheBreakpoint {
            after_message_id,
            reason: BreakpointReason::RecentMessage,
        })
        .collect()
}

#[derive(Default)]
pub struct ContextEngineBuilder {
    providers: Vec<Arc<dyn ContextProvider>>,
    aux_provider: Option<Arc<dyn AuxModelProvider>>,
    model_metrics_sink: Option<Arc<dyn ModelMetricsSink>>,
    default_compaction_blob_store: Option<Arc<dyn BlobStore>>,
    install_default_compaction: bool,
    budget: TokenBudget,
    cache_policy: PromptCachePolicy,
    #[cfg(feature = "recall-memory")]
    memory_manager: Option<Arc<MemoryManager>>,
}

impl ContextEngineBuilder {
    #[must_use]
    pub fn with_provider(mut self, provider: impl ContextProvider) -> Self {
        self.providers.push(Arc::new(provider));
        self
    }

    #[must_use]
    pub fn with_budget(mut self, budget: TokenBudget) -> Self {
        self.budget = budget;
        self
    }

    #[must_use]
    pub fn with_cache_policy(mut self, cache_policy: PromptCachePolicy) -> Self {
        self.cache_policy = cache_policy;
        self
    }

    #[must_use]
    pub fn with_aux_provider(mut self, aux_provider: Arc<dyn AuxModelProvider>) -> Self {
        self.aux_provider = Some(aux_provider);
        self
    }

    #[must_use]
    pub fn with_model_metrics_sink(mut self, metrics_sink: Arc<dyn ModelMetricsSink>) -> Self {
        self.model_metrics_sink = Some(metrics_sink);
        self
    }

    #[must_use]
    pub fn with_default_compaction(mut self, blob_store: Option<Arc<dyn BlobStore>>) -> Self {
        self.install_default_compaction = true;
        self.default_compaction_blob_store = blob_store;
        self
    }

    #[cfg(feature = "recall-memory")]
    #[must_use]
    pub fn with_memory_manager(mut self, memory_manager: Arc<MemoryManager>) -> Self {
        self.memory_manager = Some(memory_manager);
        self
    }

    pub fn build(mut self) -> Result<ContextEngine, ContextError> {
        if self.install_default_compaction {
            self.install_default_compaction_providers();
        }
        if let Some(aux_provider) = &self.aux_provider {
            let mut microcompact = MicrocompactProvider::new(aux_provider.clone());
            let mut autocompact = AutocompactProvider::new(Some(aux_provider.clone()));
            if let Some(metrics_sink) = &self.model_metrics_sink {
                microcompact = microcompact.with_model_metrics_sink(Arc::clone(metrics_sink));
                autocompact = autocompact.with_model_metrics_sink(Arc::clone(metrics_sink));
            }
            self.providers.push(Arc::new(microcompact));
            self.providers.push(Arc::new(autocompact));
        }
        self.providers
            .sort_by(|left, right| compare_providers(left.as_ref(), right.as_ref()));

        Ok(ContextEngine {
            providers: self.providers,
            budget: self.budget,
            cache_policy: self.cache_policy,
            state: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(feature = "recall-memory")]
            turn_counter: Arc::new(AtomicU64::new(1)),
            #[cfg(feature = "recall-memory")]
            memory_manager: self.memory_manager,
        })
    }

    fn install_default_compaction_providers(&mut self) {
        if !self.has_stage(ContextStageId::ToolResultBudget) {
            let blob_store = self
                .default_compaction_blob_store
                .take()
                .unwrap_or_else(|| Arc::new(harness_journal::InMemoryBlobStore::default()));
            self.providers.push(Arc::new(ToolResultBudgetProvider::new(
                DEFAULT_TOOL_RESULT_BUDGET_CHARS,
                blob_store,
            )));
        }
        if !self.has_stage(ContextStageId::Snip) {
            self.providers.push(Arc::new(SnipProvider::new(
                crate::stages::PROTECTED_RECENT_N,
            )));
        }
        if !self.has_stage(ContextStageId::Microcompact) && self.aux_provider.is_none() {
            self.providers
                .push(Arc::new(MicrocompactProvider::without_aux()));
        }
        if !self.has_stage(ContextStageId::Collapse) {
            self.providers.push(Arc::new(CollapseProvider::new(
                DEFAULT_COLLAPSE_THRESHOLD_CHARS,
            )));
        }
        if !self.has_stage(ContextStageId::Autocompact) && self.aux_provider.is_none() {
            self.providers
                .push(Arc::new(AutocompactProvider::new(None)));
        }
    }

    fn has_stage(&self, stage: ContextStageId) -> bool {
        self.providers
            .iter()
            .any(|provider| provider.stage() == stage)
    }
}

impl ContextEngine {
    #[cfg(feature = "recall-memory")]
    async fn call_memory_turn_start(
        &self,
        session: &dyn ContextSessionView,
        turn_message: &Message,
        identity: TurnIdentity,
        events: &mut Vec<Event>,
    ) {
        let Some(memory_manager) = &self.memory_manager else {
            return;
        };
        if turn_message.role != MessageRole::User {
            return;
        }
        let text = message_text(turn_message);
        let turn = identity.turn as u32;
        let view = UserMessageView {
            text: &text,
            turn,
            at: turn_message.created_at,
        };
        if let Err(error) = memory_manager.on_turn_start(turn, &view).await {
            events.push(Event::MemoryRecallDegraded(MemoryRecallDegradedEvent {
                session_id: session.session_id().unwrap_or_else(SessionId::new),
                run_id: identity.run_id,
                turn,
                provider_id: memory_manager
                    .provider_id()
                    .unwrap_or_else(|| "external".to_owned()),
                reason: MemoryRecallDegradedReason::ProviderError(error.to_string()),
                at: harness_contracts::now(),
            }));
        }
    }

    #[cfg(feature = "recall-memory")]
    async fn call_memory_pre_compress(
        &self,
        session_id: SessionId,
        run_id: RunId,
        messages: &[Message],
        events: &mut Vec<Event>,
    ) -> Option<String> {
        let Some(memory_manager) = &self.memory_manager else {
            return None;
        };
        let snippets = messages.iter().map(message_text).collect::<Vec<_>>();
        let views = messages
            .iter()
            .zip(snippets.iter())
            .map(|(message, snippet)| MessageView {
                role: message.role,
                text_snippet: snippet,
                tool_use_id: first_tool_use_id(message),
            })
            .collect::<Vec<_>>();
        match memory_manager.on_pre_compress(&views).await {
            Ok(Some(facts)) if !facts.trim().is_empty() => Some(facts),
            Ok(_) => None,
            Err(error) => {
                events.push(Event::MemoryRecallDegraded(MemoryRecallDegradedEvent {
                    session_id,
                    run_id,
                    turn: 0,
                    provider_id: memory_manager
                        .provider_id()
                        .unwrap_or_else(|| "external".to_owned()),
                    reason: MemoryRecallDegradedReason::ProviderError(error.to_string()),
                    at: harness_contracts::now(),
                }));
                None
            }
        }
    }

    #[cfg(feature = "recall-memory")]
    async fn memory_recall_patch(
        &self,
        session: &dyn ContextSessionView,
        turn_message: &Message,
        identity: TurnIdentity,
    ) -> (Vec<ContextPatch>, Vec<Event>) {
        let Some(memory_manager) = &self.memory_manager else {
            return (Vec::new(), Vec::new());
        };

        let user_text = message_text(turn_message);
        if user_text.trim().is_empty() {
            return (Vec::new(), Vec::new());
        }
        let run_id = identity.run_id;
        let turn = identity.turn;
        let recall_policy = memory_manager.recall_policy();
        if !memory_manager.has_external() {
            return (
                Vec::new(),
                vec![Event::MemoryRecallSkipped(
                    harness_contracts::MemoryRecallSkippedEvent {
                        session_id: session.session_id().unwrap_or_else(SessionId::new),
                        run_id,
                        turn: turn as u32,
                        reason: harness_contracts::RecallSkipReason::NoExternalProvider,
                        at: harness_contracts::now(),
                    },
                )],
            );
        }

        let query = MemoryQuery {
            text: user_text.clone(),
            kind_filter: Some(MemoryKindFilter::Any),
            visibility_filter: MemoryVisibilityFilter::EffectiveFor(MemoryActor {
                tenant_id: session.tenant_id(),
                user_id: session.user_id(),
                team_id: session.team_id(),
                session_id: session.session_id(),
            }),
            max_records: 8,
            min_similarity: 0.0,
            tenant_id: session.tenant_id(),
            session_id: session.session_id(),
            deadline: None,
        };

        match memory_manager
            .recall_once_per_turn_outcome(turn, query)
            .await
        {
            MemoryRecallOutcome::Skipped => (
                Vec::new(),
                vec![Event::MemoryRecallSkipped(
                    harness_contracts::MemoryRecallSkippedEvent {
                        session_id: session.session_id().unwrap_or_else(SessionId::new),
                        run_id,
                        turn: turn as u32,
                        reason: harness_contracts::RecallSkipReason::PolicyDecidedSkip,
                        at: harness_contracts::now(),
                    },
                )],
            ),
            MemoryRecallOutcome::Recalled(records) if records.is_empty() => {
                (Vec::new(), Vec::new())
            }
            MemoryRecallOutcome::Recalled(records) => {
                let fence = harness_memory::wrap_memory_context(&records);
                let kinds_returned = records
                    .iter()
                    .map(|record| record.kind.clone())
                    .collect::<Vec<_>>();
                (
                    vec![ContextPatch::MemoryRecall {
                        fence: fence.clone(),
                        lifecycle: ContentLifecycle::Transient,
                    }],
                    vec![Event::MemoryRecalled(
                        harness_contracts::MemoryRecalledEvent {
                            session_id: session.session_id().unwrap_or_else(SessionId::new),
                            run_id,
                            turn: turn as u32,
                            provider_id: memory_manager
                                .provider_id()
                                .unwrap_or_else(|| "external".to_owned()),
                            query_text_hash: content_hash(&user_text),
                            returned_count: records.len() as u32,
                            kept_count: records.len() as u32,
                            injected_chars: fence.len() as u32,
                            deadline_used_ms: recall_policy
                                .default_deadline
                                .as_millis()
                                .min(u128::from(u32::MAX))
                                as u32,
                            min_similarity: recall_policy.min_similarity,
                            kinds_returned,
                            at: harness_contracts::now(),
                        },
                    )],
                )
            }
            MemoryRecallOutcome::Degraded(error) => (
                Vec::new(),
                vec![Event::MemoryRecallDegraded(
                    harness_contracts::MemoryRecallDegradedEvent {
                        session_id: session.session_id().unwrap_or_else(SessionId::new),
                        run_id,
                        turn: turn as u32,
                        provider_id: memory_manager
                            .provider_id()
                            .unwrap_or_else(|| "external".to_owned()),
                        reason: memory_degraded_reason(&error),
                        at: harness_contracts::now(),
                    },
                )],
            ),
        }
    }

    #[cfg(feature = "recall-memory")]
    async fn enqueue_tool_result_memory_recall(
        &self,
        session: &dyn ContextSessionView,
        results: &[ToolResultEnvelope],
    ) -> Result<(), ContextError> {
        let Some(memory_manager) = &self.memory_manager else {
            return Ok(());
        };
        if !memory_manager.has_external() {
            return Ok(());
        }
        let Some(hint_text) = tool_result_memory_hint_text(results) else {
            return Ok(());
        };

        let identity = self.current_turn_identity(session);
        let run_id = identity.run_id;
        let turn = identity.turn;
        let recall_policy = memory_manager.recall_policy();
        let query = MemoryQuery {
            text: hint_text.clone(),
            kind_filter: Some(MemoryKindFilter::Any),
            visibility_filter: MemoryVisibilityFilter::EffectiveFor(MemoryActor {
                tenant_id: session.tenant_id(),
                user_id: session.user_id(),
                team_id: session.team_id(),
                session_id: session.session_id(),
            }),
            max_records: 8,
            min_similarity: 0.0,
            tenant_id: session.tenant_id(),
            session_id: session.session_id(),
            deadline: Some(Duration::from_millis(200)),
        };

        match memory_manager
            .recall_once_per_turn_outcome(turn, query)
            .await
        {
            MemoryRecallOutcome::Recalled(records) if records.is_empty() => Ok(()),
            MemoryRecallOutcome::Recalled(records) => {
                let fence = harness_memory::wrap_memory_context(&records);
                let kinds_returned = records
                    .iter()
                    .map(|record| record.kind.clone())
                    .collect::<Vec<_>>();
                self.push_pending_memory_recall(
                    session,
                    ContextPatch::MemoryRecall {
                        fence: fence.clone(),
                        lifecycle: ContentLifecycle::Transient,
                    },
                    Event::MemoryRecalled(harness_contracts::MemoryRecalledEvent {
                        session_id: session.session_id().unwrap_or_else(SessionId::new),
                        run_id,
                        turn: turn as u32,
                        provider_id: memory_manager
                            .provider_id()
                            .unwrap_or_else(|| "external".to_owned()),
                        query_text_hash: content_hash(&hint_text),
                        returned_count: records.len() as u32,
                        kept_count: records.len() as u32,
                        injected_chars: fence.len() as u32,
                        deadline_used_ms: 200,
                        min_similarity: recall_policy.min_similarity,
                        kinds_returned,
                        at: harness_contracts::now(),
                    }),
                )
            }
            MemoryRecallOutcome::Skipped => Ok(()),
            MemoryRecallOutcome::Degraded(error) => self.push_pending_memory_recall(
                session,
                ContextPatch::MemoryRecall {
                    fence: String::new(),
                    lifecycle: ContentLifecycle::Transient,
                },
                Event::MemoryRecallDegraded(MemoryRecallDegradedEvent {
                    session_id: session.session_id().unwrap_or_else(SessionId::new),
                    run_id,
                    turn: turn as u32,
                    provider_id: memory_manager
                        .provider_id()
                        .unwrap_or_else(|| "external".to_owned()),
                    reason: memory_degraded_reason(&error),
                    at: harness_contracts::now(),
                }),
            ),
        }
    }

    #[cfg(feature = "recall-memory")]
    fn push_pending_memory_recall(
        &self,
        session: &dyn ContextSessionView,
        patch: ContextPatch,
        event: Event,
    ) -> Result<(), ContextError> {
        let key = context_state_key(session.tenant_id(), session.session_id());
        let mut state = self.state.lock();
        let stored = state.entry(key).or_insert_with(|| StoredContext {
            buffer: ContextBuffer::new(key.tenant_id, key.session_id),
            pending_events: Vec::new(),
            active_turn: None,
        });
        if let ContextPatch::MemoryRecall { fence, .. } = &patch {
            if fence.is_empty() {
                stored.pending_events.push(event);
                return Ok(());
            }
        }
        stored.buffer.patches.push(patch);
        stored.pending_events.push(event);
        Ok(())
    }

    #[cfg(not(feature = "recall-memory"))]
    async fn memory_recall_patch(
        &self,
        _session: &dyn ContextSessionView,
        _turn_input: &TurnInput,
        _turn_message: &Message,
    ) -> (Vec<ContextPatch>, Vec<Event>) {
        (Vec::new(), Vec::new())
    }

    #[cfg(feature = "recall-memory")]
    fn turn_key(&self, turn_input: &TurnInput) -> u64 {
        turn_input
            .metadata
            .get("turn")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(|| self.turn_counter.fetch_add(1, Ordering::SeqCst) + 1)
    }

    #[cfg(feature = "recall-memory")]
    fn turn_identity(&self, turn_input: &TurnInput) -> TurnIdentity {
        TurnIdentity {
            run_id: run_id_from_metadata(turn_input).unwrap_or_else(RunId::new),
            turn: self.turn_key(turn_input),
        }
    }

    #[cfg(feature = "recall-memory")]
    fn current_turn_identity(&self, session: &dyn ContextSessionView) -> TurnIdentity {
        let key = context_state_key(session.tenant_id(), session.session_id());
        if let Some(identity) = self
            .state
            .lock()
            .get(&key)
            .and_then(|stored| stored.active_turn)
        {
            return identity;
        }
        TurnIdentity {
            run_id: RunId::new(),
            turn: self.turn_counter.load(Ordering::SeqCst).max(1),
        }
    }
}

fn compare_providers(
    left: &dyn ContextProvider,
    right: &dyn ContextProvider,
) -> std::cmp::Ordering {
    stage_rank(left.stage())
        .cmp(&stage_rank(right.stage()))
        .then_with(|| left.provider_id().cmp(right.provider_id()))
}

fn stage_rank(stage: ContextStageId) -> usize {
    COMPACT_STAGE_ORDER
        .iter()
        .position(|candidate| *candidate == stage)
        .unwrap_or(COMPACT_STAGE_ORDER.len())
}

fn estimate_tokens(system: Option<&str>, messages: &[harness_contracts::Message]) -> u64 {
    let mut chars = system.map(str::len).unwrap_or_default();
    for message in messages {
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

pub(crate) fn estimate_message_tokens(message: &harness_contracts::Message) -> u64 {
    estimate_tokens(None, std::slice::from_ref(message))
}

fn budget_utilization(tokens_estimate: u64, max_tokens: u64) -> f32 {
    if max_tokens == 0 {
        return 0.0;
    }
    let per_mille = tokens_estimate
        .saturating_mul(1_000)
        .checked_div(max_tokens)
        .unwrap_or_default()
        .min(u64::from(u16::MAX)) as u16;
    f32::from(per_mille) / 1_000.0
}

fn context_state_key(tenant_id: TenantId, session_id: Option<SessionId>) -> ContextStateKey {
    ContextStateKey {
        tenant_id,
        session_id: session_id.unwrap_or_default(),
    }
}

fn context_patch_from_request(request: ContextPatchRequest) -> (ContextPatch, Option<Event>) {
    let event_source = request.source.clone();
    let event_lifecycle = request.lifecycle.clone();
    let body_bytes = request.body.len() as u64;
    let lifecycle = context_lifecycle(request.lifecycle);
    match request.source {
        ContextPatchSource::MemoryRecall { .. } => (
            ContextPatch::MemoryRecall {
                fence: request.body,
                lifecycle,
            },
            None,
        ),
        ContextPatchSource::SkillInjection {
            skill_id,
            skill_name,
            injection_id,
            tool_use_id,
            consumed_config_keys,
        } => {
            let bytes_injected = request.body.len() as u64;
            (
                ContextPatch::SkillInjection {
                    skill_id: skill_id.0.clone(),
                    skill_name: skill_name.clone(),
                    body: request.body,
                    lifecycle,
                },
                Some(Event::SkillInvoked(SkillInvokedEvent {
                    session_id: request.session_id,
                    run_id: request.run_id,
                    tool_use_id,
                    skill_id,
                    skill_name,
                    injection_id,
                    bytes_injected,
                    consumed_config_keys,
                    at: harness_contracts::now(),
                })),
            )
        }
        ContextPatchSource::HookAddContext {
            handler_id,
            hook_event_kind: _,
        } => (
            ContextPatch::HookAddContext {
                handler_id,
                body: request.body,
                lifecycle,
            },
            None,
        ),
        ContextPatchSource::KnowledgeRetrieval {
            provider_id,
            knowledge_base_ids,
            reference_chunk_count,
        } => (
            ContextPatch::KnowledgeRetrieval {
                provider_id,
                knowledge_base_ids,
                reference_chunk_count,
                body: request.body,
                lifecycle,
            },
            Some(Event::ContextPatchApplied(ContextPatchAppliedEvent {
                session_id: request.session_id,
                run_id: request.run_id,
                source: event_source,
                lifecycle: event_lifecycle,
                body_bytes,
                at: harness_contracts::now(),
            })),
        ),
    }
}

fn context_lifecycle(lifecycle: ContextPatchLifecycle) -> ContentLifecycle {
    match lifecycle {
        ContextPatchLifecycle::Transient => ContentLifecycle::Transient,
        ContextPatchLifecycle::Persistent { ttl_turns } => {
            ContentLifecycle::Persistent { ttl_turns }
        }
    }
}

fn patch_lifecycle_mut(patch: &mut ContextPatch) -> &mut ContentLifecycle {
    match patch {
        ContextPatch::MemoryRecall { lifecycle, .. }
        | ContextPatch::KnowledgeRetrieval { lifecycle, .. }
        | ContextPatch::SkillInjection { lifecycle, .. }
        | ContextPatch::HookAddContext { lifecycle, .. }
        | ContextPatch::DeferredToolsDelta { lifecycle, .. } => lifecycle,
    }
}

fn expire_persistent_patches(patches: &mut Vec<ContextPatch>) {
    patches.retain_mut(|patch| match patch_lifecycle_mut(patch) {
        ContentLifecycle::Transient => false,
        ContentLifecycle::Persistent { ttl_turns: None } => true,
        ContentLifecycle::Persistent {
            ttl_turns: Some(ttl),
        } => {
            if *ttl == 0 {
                false
            } else {
                *ttl -= 1;
                true
            }
        }
    });
}

fn sanitize_turn_message(message: &mut Message) {
    #[cfg(feature = "recall-memory")]
    sanitize_memory_context(message);
    #[cfg(not(feature = "recall-memory"))]
    {
        let _ = message;
    }
}

fn apply_patches_to_user_message(message: &mut Message, patches: &[ContextPatch]) {
    if patches.is_empty() {
        return;
    }
    prepend_to_user_message(message, &render_patches(patches, PatchRenderKind::Inline));
}

fn patch_carrier_message(patches: &[ContextPatch]) -> Message {
    Message {
        id: MessageId::new(),
        role: MessageRole::User,
        parts: vec![MessagePart::Text(render_patches(
            patches,
            PatchRenderKind::Carrier,
        ))],
        created_at: harness_contracts::now(),
    }
}

#[derive(Debug, Clone, Copy)]
enum PatchRenderKind {
    Inline,
    Carrier,
}

fn render_patches(patches: &[ContextPatch], kind: PatchRenderKind) -> String {
    patches
        .iter()
        .map(|patch| render_patch(patch, patch_render_kind(patch, kind)))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn patch_render_kind(patch: &ContextPatch, default: PatchRenderKind) -> PatchRenderKind {
    match patch {
        ContextPatch::MemoryRecall { .. }
        | ContextPatch::KnowledgeRetrieval { .. }
        | ContextPatch::DeferredToolsDelta { .. } => PatchRenderKind::Inline,
        ContextPatch::SkillInjection { .. } | ContextPatch::HookAddContext { .. } => default,
    }
}

fn render_patch(patch: &ContextPatch, _kind: PatchRenderKind) -> String {
    match patch {
        ContextPatch::MemoryRecall { fence, .. } => fence.clone(),
        ContextPatch::SkillInjection {
            skill_id,
            skill_name,
            body,
            ..
        } => {
            format!(
                "---SKILL-BEGIN: {skill_name}---\n{body}\n---SKILL-END: {skill_name} ({skill_id})---"
            )
        }
        ContextPatch::HookAddContext {
            handler_id, body, ..
        } => {
            format!("<hook-add-context handler=\"{handler_id}\">\n{body}\n</hook-add-context>")
        }
        ContextPatch::KnowledgeRetrieval {
            provider_id,
            knowledge_base_ids,
            reference_chunk_count,
            body,
            ..
        } => {
            let knowledge_base_ids = knowledge_base_ids.join(",");
            format!(
                "<knowledge-retrieval provider_id=\"{provider_id}\" knowledge_base_ids=\"{knowledge_base_ids}\" reference_chunk_count=\"{reference_chunk_count}\">\n{body}\n</knowledge-retrieval>"
            )
        }
        ContextPatch::DeferredToolsDelta { body, .. } => body.clone(),
    }
}

fn prepend_to_user_message(message: &mut Message, prefix: &str) {
    if let Some(MessagePart::Text(text)) = message
        .parts
        .iter_mut()
        .find(|part| matches!(part, MessagePart::Text(_)))
    {
        *text = format!("{prefix}\n{text}");
        return;
    }

    message
        .parts
        .insert(0, MessagePart::Text(format!("{prefix}\n")));
}

fn tool_result_message(result: &ToolResultEnvelope) -> Message {
    Message {
        id: MessageId::new(),
        role: MessageRole::Tool,
        parts: vec![MessagePart::Text(format!("{:?}", result.result))],
        created_at: harness_contracts::now(),
    }
}

#[cfg(feature = "recall-memory")]
fn memory_pre_compress_message(facts: String) -> Message {
    Message {
        id: MessageId::new(),
        role: MessageRole::User,
        parts: vec![MessagePart::Text(format!(
            "[MEMORY_PRE_COMPRESS]\n{}",
            facts.trim()
        ))],
        created_at: harness_contracts::now(),
    }
}

fn context_stage_outcome(outcome: &ContextOutcome) -> ContextStageOutcome {
    match outcome {
        ContextOutcome::NoChange => ContextStageOutcome::NoChange,
        ContextOutcome::Modified { .. } => ContextStageOutcome::Modified,
        ContextOutcome::Forked { new_session_id } => ContextStageOutcome::Forked {
            child: *new_session_id,
        },
    }
}

fn skipped_provider_id(stage: &ContextStageId) -> &'static str {
    match stage {
        ContextStageId::ToolResultBudget => "tool-result-budget",
        ContextStageId::Snip => "snip",
        ContextStageId::Microcompact => "microcompact",
        ContextStageId::Collapse => "collapse",
        ContextStageId::Autocompact => "autocompact",
        _ => "unknown",
    }
}

fn duration_ms(started: Instant) -> u32 {
    started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32
}

#[cfg(feature = "recall-memory")]
fn content_hash(text: &str) -> harness_contracts::ContentHash {
    harness_contracts::ContentHash(*blake3::hash(text.as_bytes()).as_bytes())
}

#[cfg(feature = "recall-memory")]
fn run_id_from_metadata(turn_input: &TurnInput) -> Option<RunId> {
    turn_input
        .metadata
        .get("run_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| RunId::parse(value).ok())
}

#[cfg(feature = "recall-memory")]
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

#[cfg(feature = "recall-memory")]
fn tool_result_memory_hint_text(results: &[ToolResultEnvelope]) -> Option<String> {
    results
        .iter()
        .filter(|result| !result.is_error)
        .filter_map(|result| tool_result_text_for_memory_hint(&result.result))
        .find(|text| text.contains("需要查阅历史"))
}

#[cfg(feature = "recall-memory")]
fn tool_result_text_for_memory_hint(result: &harness_contracts::ToolResult) -> Option<String> {
    match result {
        harness_contracts::ToolResult::Text(text) => Some(text.clone()),
        harness_contracts::ToolResult::Structured(value) => Some(value.to_string()),
        harness_contracts::ToolResult::Mixed(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| match part {
                    harness_contracts::ToolResultPart::Text { text } => Some(text.clone()),
                    harness_contracts::ToolResultPart::Structured { value, .. } => {
                        Some(value.to_string())
                    }
                    harness_contracts::ToolResultPart::Code { text, .. } => Some(text.clone()),
                    harness_contracts::ToolResultPart::Reference { summary, .. }
                    | harness_contracts::ToolResultPart::Blob { summary, .. } => summary.clone(),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
        harness_contracts::ToolResult::Blob { .. } => None,
        _ => None,
    }
}

#[cfg(feature = "recall-memory")]
fn first_tool_use_id(message: &Message) -> Option<harness_contracts::ToolUseId> {
    message.parts.iter().find_map(|part| match part {
        MessagePart::ToolUse { id, .. }
        | MessagePart::ToolResult {
            tool_use_id: id, ..
        } => Some(*id),
        _ => None,
    })
}

#[cfg(feature = "recall-memory")]
fn memory_degraded_reason(
    error: &harness_contracts::MemoryError,
) -> harness_contracts::MemoryRecallDegradedReason {
    let message = error.to_string();
    if message.contains("deadline exceeded") {
        harness_contracts::MemoryRecallDegradedReason::Timeout
    } else {
        harness_contracts::MemoryRecallDegradedReason::ProviderError(message)
    }
}

#[cfg(feature = "recall-memory")]
fn sanitize_memory_context(message: &mut Message) {
    for part in &mut message.parts {
        if let MessagePart::Text(text) = part {
            *text = harness_memory::sanitize_context(text)
                .trim_start()
                .to_owned();
        }
    }
}

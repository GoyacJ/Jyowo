use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(feature = "consolidation")]
use harness_contracts::MemoryConsolidationRanEvent;
use harness_contracts::{
    ContentHash, Event, MemdirFileTag, MemoryActor, MemoryActorContext, MemoryCandidateTrace,
    MemoryError, MemoryEvidence, MemoryExportedEvent, MemoryId, MemoryPermissionContext,
    MemoryPolicyDecision, MemoryProviderSelectionPolicy, MemoryProviderTrace, MemoryScoreBreakdown,
    MemorySessionCtx, MemorySource, MemoryThreadSettings, MemoryTraceId, MemoryUpsertedEvent,
    MemoryVisibility, MemoryWriteAction, MemoryWriteTarget, MessageView, RunId, SessionId,
    SessionSummaryView, TakesEffect, UserMessageView, WriteDestination,
};
#[cfg(feature = "threat-scanner")]
use harness_contracts::{MemoryThreatDetectedEvent, ThreatAction, ThreatDirection};
use tokio::sync::{watch, Mutex, RwLock};
use tokio::time::timeout;

#[cfg(feature = "builtin")]
use crate::BuiltinMemory;
#[cfg(feature = "consolidation")]
use crate::ConsolidationHook;
#[cfg(feature = "threat-scanner")]
use crate::MemoryThreatScanner;
use crate::{
    content_preview, visibility_matches, MemoryEventSink, MemoryKindFilter, MemoryListScope,
    MemoryMetric, MemoryMetricsSink, MemoryPolicyEngine, MemoryProvider, MemoryProviderRegistry,
    MemoryQuery, MemoryRecallMetricOutcome, MemoryRecallTraceBuilder, MemoryRecallTraceCollector,
    MemoryRecord, MemorySummary, MemoryVisibilityFilter,
};

pub struct MemoryManager {
    #[cfg(feature = "builtin")]
    builtin: RwLock<Option<BuiltinMemory>>,
    provider_registry: RwLock<MemoryProviderRegistry>,
    policy_engine: RwLock<MemoryPolicyEngine>,
    recall_policy: RecallPolicy,
    #[cfg(feature = "consolidation")]
    consolidation_hook: Option<Arc<dyn ConsolidationHook>>,
    recall_gate: Mutex<Option<TurnRecallGate>>,
    event_sink: Option<Arc<dyn MemoryEventSink>>,
    metrics_sink: Option<Arc<dyn MemoryMetricsSink>>,
    #[cfg(feature = "threat-scanner")]
    threat_scanner: Option<Arc<MemoryThreatScanner>>,
    trace_collector: Arc<MemoryRecallTraceCollector>,
}

#[derive(Debug, Clone)]
pub struct MemoryOperationPolicy {
    pub thread: MemoryThreadSettings,
    pub actor: MemoryActor,
    pub permission: MemoryPermissionContext,
    pub evidence: MemoryEvidence,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryExportPreparation {
    pub summaries: Vec<MemorySummary>,
    pub event: MemoryExportedEvent,
}

type RecallResult = MemoryRecallResult;

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryRecallResult {
    pub outcome: MemoryRecallOutcome,
    pub trace_id: Option<MemoryTraceId>,
    pub sources: Vec<MemoryRecallSource>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryRecallSource {
    pub record: MemoryRecord,
    pub provider_id: String,
}

struct TurnRecallGate {
    turn: u64,
    phase: TurnRecallPhase,
}

enum TurnRecallPhase {
    InFlight(watch::Receiver<Option<RecallResult>>),
    Completed,
}

enum RecallGateAction {
    Lead(watch::Sender<Option<RecallResult>>),
    Wait(watch::Receiver<Option<RecallResult>>),
    Skip,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MemoryRecallOutcome {
    Recalled(Vec<MemoryRecord>),
    Skipped,
    Degraded(MemoryError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecallPolicy {
    pub max_records_per_turn: u32,
    pub max_chars_per_turn: u32,
    pub default_deadline: Duration,
    pub min_similarity: f32,
    pub fail_open: FailMode,
    pub trigger: RecallTriggerStrategy,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FailMode {
    Skip,
    Surface,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RecallTriggerStrategy {
    AlwaysOnUserMessage,
    OnQuestionMark,
    Custom(String),
}

impl Default for RecallPolicy {
    fn default() -> Self {
        Self {
            max_records_per_turn: 8,
            max_chars_per_turn: 4_000,
            default_deadline: Duration::from_millis(300),
            min_similarity: 0.65,
            fail_open: FailMode::Skip,
            trigger: RecallTriggerStrategy::AlwaysOnUserMessage,
        }
    }
}

impl Default for MemoryManager {
    fn default() -> Self {
        Self {
            #[cfg(feature = "builtin")]
            builtin: RwLock::new(None),
            provider_registry: RwLock::new(MemoryProviderRegistry::new()),
            policy_engine: RwLock::new(MemoryPolicyEngine::new(
                harness_contracts::MemoryGlobalSettings {
                    use_memories: true,
                    generate_memories: true,
                    disable_generation_when_external_context_used: false,
                    retention_days: None,
                    max_memory_bytes: 10_000_000,
                    max_recall_records_per_turn: 20,
                    max_recall_chars_per_turn: 50_000,
                },
            )),
            recall_policy: RecallPolicy::default(),
            #[cfg(feature = "consolidation")]
            consolidation_hook: None,
            recall_gate: Mutex::new(None),
            event_sink: None,
            metrics_sink: None,
            #[cfg(feature = "threat-scanner")]
            threat_scanner: None,
            trace_collector: Arc::new(MemoryRecallTraceCollector::new()),
        }
    }
}

impl MemoryManager {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_provider(&self, provider: Arc<dyn MemoryProvider>) -> Result<(), MemoryError> {
        self.provider_registry
            .try_write()
            .map_err(|_| MemoryError::Message("provider registry lock busy".to_owned()))?
            .register(provider)?;
        self.record_metric(MemoryMetric::ExternalProviderConfigured { configured: true });
        Ok(())
    }

    pub fn provider_registry(&self) -> Option<MemoryProviderRegistry> {
        self.provider_registry
            .try_read()
            .ok()
            .map(|guard| guard.clone())
    }

    #[cfg(feature = "builtin")]
    #[must_use]
    pub fn with_builtin_memory(mut self, mut memory: BuiltinMemory) -> Self {
        if let Some(event_sink) = &self.event_sink {
            memory = memory.with_event_sink(Arc::clone(event_sink));
        }
        if let Some(metrics_sink) = &self.metrics_sink {
            memory = memory.with_metrics_sink(Arc::clone(metrics_sink));
        }
        #[cfg(feature = "threat-scanner")]
        if let Some(scanner) = &self.threat_scanner {
            memory = memory.with_threat_scanner(Arc::clone(scanner));
        }
        *self.builtin.get_mut() = Some(memory);
        self
    }

    #[cfg(feature = "builtin")]
    pub fn builtin(&self) -> Option<BuiltinMemory> {
        self.builtin.try_read().ok().and_then(|slot| slot.clone())
    }

    #[cfg(feature = "builtin")]
    #[must_use]
    pub fn has_builtin(&self) -> bool {
        self.builtin().is_some()
    }

    pub fn provider_id(&self) -> Option<String> {
        let mut ids = self
            .provider_registry
            .try_read()
            .ok()?
            .providers()
            .map(|provider| provider.provider_id)
            .collect::<Vec<_>>();
        ids.sort();
        match ids.len() {
            0 => None,
            1 => ids.pop(),
            _ => Some(ids.join(",")),
        }
    }

    #[must_use]
    pub fn recall_policy(&self) -> RecallPolicy {
        self.recall_policy.clone()
    }

    #[must_use]
    pub fn should_recall_text(&self, text: &str) -> bool {
        match &self.recall_policy.trigger {
            RecallTriggerStrategy::AlwaysOnUserMessage => true,
            RecallTriggerStrategy::OnQuestionMark => text.contains('?') || text.contains('？'),
            RecallTriggerStrategy::Custom(_) => false,
        }
    }

    #[must_use]
    pub fn has_external(&self) -> bool {
        self.provider_registry
            .try_read()
            .map(|registry| !registry.is_empty())
            .unwrap_or(false)
    }

    #[must_use]
    pub fn trace_collector(&self) -> Arc<MemoryRecallTraceCollector> {
        Arc::clone(&self.trace_collector)
    }

    #[must_use]
    pub fn with_trace_collector(mut self, trace_collector: MemoryRecallTraceCollector) -> Self {
        self.trace_collector = Arc::new(trace_collector);
        self
    }

    pub fn with_durable_trace_collector(mut self, db_path: &str) -> Result<Self, MemoryError> {
        self.trace_collector = Arc::new(
            MemoryRecallTraceCollector::open(db_path)
                .map_err(|e| MemoryError::Message(format!("open recall trace collector: {e}")))?,
        );
        Ok(self)
    }

    #[must_use]
    pub fn with_policy_engine(mut self, engine: MemoryPolicyEngine) -> Self {
        self.policy_engine = RwLock::new(engine);
        self
    }

    pub fn set_policy_settings(&self, settings: harness_contracts::MemoryGlobalSettings) {
        if let Ok(mut engine) = self.policy_engine.try_write() {
            *engine = MemoryPolicyEngine::new(settings);
        }
    }

    #[must_use]
    pub fn with_recall_policy(mut self, policy: RecallPolicy) -> Self {
        self.recall_policy = policy;
        self
    }

    #[cfg(feature = "consolidation")]
    #[must_use]
    pub fn with_consolidation_hook(mut self, hook: Arc<dyn ConsolidationHook>) -> Self {
        self.consolidation_hook = Some(hook);
        self
    }

    #[must_use]
    pub fn with_event_sink(mut self, event_sink: Arc<dyn MemoryEventSink>) -> Self {
        #[cfg(feature = "builtin")]
        if let Some(memory) = self.builtin.get_mut().take() {
            *self.builtin.get_mut() = Some(memory.with_event_sink(Arc::clone(&event_sink)));
        }
        self.event_sink = Some(event_sink);
        self
    }

    #[must_use]
    pub fn with_metrics_sink(mut self, metrics_sink: Arc<dyn MemoryMetricsSink>) -> Self {
        #[cfg(feature = "builtin")]
        if let Some(memory) = self.builtin.get_mut().take() {
            *self.builtin.get_mut() = Some(memory.with_metrics_sink(Arc::clone(&metrics_sink)));
        }
        self.metrics_sink = Some(metrics_sink);
        self
    }

    #[cfg(feature = "threat-scanner")]
    #[must_use]
    pub fn with_threat_scanner(mut self, scanner: Arc<MemoryThreatScanner>) -> Self {
        #[cfg(feature = "builtin")]
        if let Some(memory) = self.builtin.get_mut().take() {
            *self.builtin.get_mut() = Some(memory.with_threat_scanner(Arc::clone(&scanner)));
        }
        self.threat_scanner = Some(scanner);
        self
    }

    pub async fn recall(&self, query: MemoryQuery) -> Result<Vec<MemoryRecord>, MemoryError> {
        self.records_from_outcome(self.recall_outcome(query).await)
    }

    pub async fn recall_sources(
        &self,
        query: MemoryQuery,
    ) -> Result<Vec<MemoryRecallSource>, MemoryError> {
        self.sources_from_result(self.recall_result(query, None).await)
    }

    pub async fn recall_with_policy(
        &self,
        query: MemoryQuery,
        thread: &MemoryThreadSettings,
        actor: &MemoryActor,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        let decision = self
            .policy_engine
            .read()
            .await
            .evaluate_recall(thread, actor);
        if !matches!(decision, MemoryPolicyDecision::Allow) {
            self.record_metric(MemoryMetric::Recall {
                provider_id: None,
                outcome: MemoryRecallMetricOutcome::Skipped,
                duration_ms: 0,
                returned_count: 0,
            });
            return Ok(Vec::new());
        }
        self.recall(query).await
    }

    pub async fn recall_with_policy_sources(
        &self,
        query: MemoryQuery,
        thread: &MemoryThreadSettings,
        actor: &MemoryActor,
    ) -> Result<Vec<MemoryRecallSource>, MemoryError> {
        let decision = self
            .policy_engine
            .read()
            .await
            .evaluate_recall(thread, actor);
        if !matches!(decision, MemoryPolicyDecision::Allow) {
            self.record_metric(MemoryMetric::Recall {
                provider_id: None,
                outcome: MemoryRecallMetricOutcome::Skipped,
                duration_ms: 0,
                returned_count: 0,
            });
            return Ok(Vec::new());
        }
        self.recall_sources(query).await
    }

    pub async fn upsert_with_policy(
        &self,
        record: MemoryRecord,
        run_id: Option<RunId>,
        policy: &MemoryOperationPolicy,
    ) -> Result<harness_contracts::MemoryId, MemoryError> {
        self.upsert_with_policy_and_provider_selection(
            record,
            run_id,
            policy,
            &MemoryProviderSelectionPolicy::PolicySelected,
        )
        .await
    }

    pub async fn upsert_with_policy_and_provider_selection(
        &self,
        mut record: MemoryRecord,
        run_id: Option<RunId>,
        policy: &MemoryOperationPolicy,
        provider_policy: &MemoryProviderSelectionPolicy,
    ) -> Result<harness_contracts::MemoryId, MemoryError> {
        let decision = self.policy_engine.read().await.evaluate_write(
            &policy.thread,
            &policy.actor,
            &policy.evidence,
            &policy.permission,
            &record.visibility,
        );
        ensure_direct_memory_policy_allows(decision)?;
        record.metadata.evidence = Some(policy.evidence.clone());
        let provider = self.write_provider_for_record_with_policy(&record, provider_policy)?;
        self.upsert_to_provider_required_audit(provider, record, run_id)
            .await
    }

    pub async fn upsert_with_policy_required_audit(
        &self,
        mut record: MemoryRecord,
        run_id: Option<RunId>,
        policy: &MemoryOperationPolicy,
    ) -> Result<harness_contracts::MemoryId, MemoryError> {
        let decision = self.policy_engine.read().await.evaluate_write(
            &policy.thread,
            &policy.actor,
            &policy.evidence,
            &policy.permission,
            &record.visibility,
        );
        ensure_direct_memory_policy_allows(decision)?;
        record.metadata.evidence = Some(policy.evidence.clone());
        let provider = self.write_provider_for_record(&record)?;
        self.upsert_to_provider_required_audit(provider, record, run_id)
            .await
    }

    pub async fn upsert_provider_with_policy_required_audit(
        &self,
        provider: Arc<dyn MemoryProvider>,
        mut record: MemoryRecord,
        run_id: Option<RunId>,
        policy: &MemoryOperationPolicy,
    ) -> Result<harness_contracts::MemoryId, MemoryError> {
        let decision = self.policy_engine.read().await.evaluate_write(
            &policy.thread,
            &policy.actor,
            &policy.evidence,
            &policy.permission,
            &record.visibility,
        );
        ensure_direct_memory_policy_allows(decision)?;
        record.metadata.evidence = Some(policy.evidence.clone());
        self.upsert_to_provider_required_audit(provider, record, run_id)
            .await
    }

    pub async fn upsert(
        &self,
        record: MemoryRecord,
        run_id: Option<RunId>,
    ) -> Result<harness_contracts::MemoryId, MemoryError> {
        let provider = self.write_provider_for_record(&record)?;
        self.upsert_to_provider(provider, record, run_id).await
    }

    async fn upsert_to_provider(
        &self,
        provider: Arc<dyn MemoryProvider>,
        mut record: MemoryRecord,
        run_id: Option<RunId>,
    ) -> Result<harness_contracts::MemoryId, MemoryError> {
        let now = chrono::Utc::now();
        record.updated_at = now;
        let session_id = record
            .visibility
            .session_id()
            .or_else(|| source_session_id(&record.metadata.source))
            .unwrap_or_else(SessionId::new);
        let metric_kind = record.kind.clone();
        let metric_visibility = record.visibility.clone();

        let provider_id = provider.provider_id().to_owned();
        #[cfg(feature = "threat-scanner")]
        let mut provider_record = record.clone();
        #[cfg(not(feature = "threat-scanner"))]
        let provider_record = record.clone();
        #[cfg(feature = "threat-scanner")]
        self.scan_record_before_write(
            &mut provider_record,
            session_id,
            run_id,
            Some(provider_id.clone()),
        )
        .await?;
        let content_hash = content_hash(&provider_record.content);
        let bytes_written = provider_record.content.len() as u64;
        let target = MemoryWriteTarget {
            kind: provider_record.kind.clone(),
            visibility: provider_record.visibility.clone(),
            destination: WriteDestination::External {
                provider_id: provider_id.clone(),
            },
        };
        let id = provider.upsert(provider_record.clone()).await?;
        provider
            .on_memory_write(MemoryWriteAction::Upsert, &target, content_hash.clone())
            .await?;
        if let Some(sink) = &self.event_sink {
            sink.emit(Event::MemoryUpserted(MemoryUpsertedEvent {
                session_id: provider_record
                    .visibility
                    .session_id()
                    .or_else(|| source_session_id(&provider_record.metadata.source))
                    .unwrap_or(session_id),
                run_id,
                memory_id: id,
                kind: provider_record.kind.clone(),
                visibility: provider_record.visibility.clone(),
                action: MemoryWriteAction::Upsert,
                provider_id,
                source: provider_record.metadata.source.clone(),
                content_hash: content_hash.clone(),
                bytes_written,
                takes_effect: TakesEffect::CurrentSession,
                at: now,
            }))
            .await;
        }
        self.record_metric(MemoryMetric::Upsert {
            kind: metric_kind,
            visibility: metric_visibility,
        });
        Ok(id)
    }

    async fn upsert_to_provider_required_audit(
        &self,
        provider: Arc<dyn MemoryProvider>,
        mut record: MemoryRecord,
        run_id: Option<RunId>,
    ) -> Result<harness_contracts::MemoryId, MemoryError> {
        let Some(sink) = &self.event_sink else {
            return Err(MemoryError::Provider {
                provider: "audit".to_owned(),
                source_message: "required audit sink is not configured".to_owned(),
            });
        };
        let prior_record = match provider.get(record.id).await {
            Ok(existing) => Some(existing),
            Err(MemoryError::NotFound(_)) => None,
            Err(error) => return Err(error),
        };
        let now = chrono::Utc::now();
        record.updated_at = now;
        let session_id = record
            .visibility
            .session_id()
            .or_else(|| source_session_id(&record.metadata.source))
            .unwrap_or_else(SessionId::new);
        let metric_kind = record.kind.clone();
        let metric_visibility = record.visibility.clone();

        let provider_id = provider.provider_id().to_owned();
        #[cfg(feature = "threat-scanner")]
        let mut provider_record = record.clone();
        #[cfg(not(feature = "threat-scanner"))]
        let provider_record = record.clone();
        #[cfg(feature = "threat-scanner")]
        self.scan_record_before_write(
            &mut provider_record,
            session_id,
            run_id,
            Some(provider_id.clone()),
        )
        .await?;
        let content_hash = content_hash(&provider_record.content);
        let bytes_written = provider_record.content.len() as u64;
        let target = MemoryWriteTarget {
            kind: provider_record.kind.clone(),
            visibility: provider_record.visibility.clone(),
            destination: WriteDestination::External {
                provider_id: provider_id.clone(),
            },
        };
        let id = provider.upsert(provider_record.clone()).await?;
        if let Err(error) = sink
            .emit_required(Event::MemoryUpserted(MemoryUpsertedEvent {
                session_id: provider_record
                    .visibility
                    .session_id()
                    .or_else(|| source_session_id(&provider_record.metadata.source))
                    .unwrap_or(session_id),
                run_id,
                memory_id: id,
                kind: provider_record.kind.clone(),
                visibility: provider_record.visibility.clone(),
                action: MemoryWriteAction::Upsert,
                provider_id: provider_id.clone(),
                source: provider_record.metadata.source.clone(),
                content_hash: content_hash.clone(),
                bytes_written,
                takes_effect: TakesEffect::CurrentSession,
                at: now,
            }))
            .await
        {
            let rollback = match prior_record {
                Some(existing) => provider.rollback_uncommitted_forget(existing).await,
                None => provider.rollback_uncommitted_upsert(id).await,
            };
            if let Err(rollback_error) = rollback {
                return Err(MemoryError::Provider {
                    provider: "audit".to_owned(),
                    source_message: format!(
                        "required audit append failed: {error}; provider rollback failed: {rollback_error}"
                    ),
                });
            }
            return Err(error);
        }
        provider
            .on_memory_write(MemoryWriteAction::Upsert, &target, content_hash)
            .await?;
        self.record_metric(MemoryMetric::Upsert {
            kind: metric_kind,
            visibility: metric_visibility,
        });
        Ok(id)
    }

    pub async fn list_for_actor(
        &self,
        actor: MemoryActorContext,
    ) -> Result<Vec<crate::MemorySummary>, MemoryError> {
        Ok(self
            .list_for_actor_sources(actor)
            .await?
            .into_iter()
            .map(|source| memory_summary_from_record(&source.record))
            .collect())
    }

    pub async fn list_for_actor_sources(
        &self,
        actor: MemoryActorContext,
    ) -> Result<Vec<MemoryRecallSource>, MemoryError> {
        let providers = self.readable_providers()?;
        if providers.is_empty() {
            return Err(MemoryError::ExternalProviderNotConfigured);
        }

        let mut visible = Vec::new();
        let mut seen = HashSet::new();
        for provider in providers {
            let provider_id = provider.provider_id().to_owned();
            let summaries = provider
                .list(MemoryListScope::ForActor(actor.clone()))
                .await?;
            for summary in summaries {
                let record = provider.get(summary.id).await?;
                if record_visible_to_actor(&record, &actor) {
                    if !seen.insert(record.id) {
                        continue;
                    }
                    let scanned = self
                        .scan_records(vec![record], provider_id.clone(), actor.session_id)
                        .await;
                    if let Some(record) = scanned.into_iter().next() {
                        visible.push(MemoryRecallSource {
                            record,
                            provider_id: provider_id.clone(),
                        });
                    }
                }
            }
        }

        Ok(visible)
    }

    pub async fn get_for_actor(
        &self,
        id: MemoryId,
        actor: MemoryActorContext,
    ) -> Result<MemoryRecord, MemoryError> {
        self.get_for_actor_with_provider(id, actor)
            .await
            .map(|source| source.record)
    }

    pub async fn get_for_actor_with_provider(
        &self,
        id: MemoryId,
        actor: MemoryActorContext,
    ) -> Result<MemoryRecallSource, MemoryError> {
        let providers = self.readable_providers()?;
        if providers.is_empty() {
            return Err(MemoryError::ExternalProviderNotConfigured);
        }

        for provider in providers {
            let provider_id = provider.provider_id().to_owned();
            let record = match provider.get(id).await {
                Ok(record) => record,
                Err(MemoryError::NotFound(_)) => continue,
                Err(error) => return Err(error),
            };
            if !record_visible_to_actor(&record, &actor) {
                continue;
            }

            let records = self
                .scan_records(vec![record], provider_id.clone(), actor.session_id)
                .await;
            if let Some(record) = records.into_iter().next() {
                return Ok(MemoryRecallSource {
                    record,
                    provider_id,
                });
            }
        }

        Err(MemoryError::NotFound(id))
    }

    pub async fn update_content_for_actor(
        &self,
        id: MemoryId,
        actor: MemoryActorContext,
        content: impl Into<String>,
        run_id: Option<RunId>,
    ) -> Result<MemoryRecord, MemoryError> {
        let (provider, mut record) = self
            .writable_provider_record_for_actor(id, actor.clone())
            .await?;
        record.content = content.into();
        record.updated_at = chrono::Utc::now();
        self.upsert_to_provider(provider, record, run_id).await?;
        self.get_for_actor(id, actor).await
    }

    pub async fn update_content_for_actor_with_policy(
        &self,
        id: MemoryId,
        actor: MemoryActorContext,
        content: impl Into<String>,
        run_id: Option<RunId>,
        policy: &MemoryOperationPolicy,
    ) -> Result<MemoryRecord, MemoryError> {
        let (provider, mut record) = self
            .writable_provider_record_for_actor(id, actor.clone())
            .await?;
        record.content = content.into();
        record.updated_at = chrono::Utc::now();
        self.upsert_provider_with_policy_required_audit(provider, record, run_id, policy)
            .await?;
        self.get_for_actor(id, actor).await
    }

    pub async fn forget_for_actor(
        &self,
        id: MemoryId,
        actor: MemoryActorContext,
        run_id: Option<RunId>,
    ) -> Result<(), MemoryError> {
        let providers = self.writable_providers()?;
        if providers.is_empty() {
            return Err(MemoryError::ExternalProviderNotConfigured);
        }

        let mut found_target = None;
        for provider in providers {
            let record = match provider.get(id).await {
                Ok(record) => record,
                Err(MemoryError::NotFound(_)) => continue,
                Err(error) => return Err(error),
            };
            if record_visible_to_actor(&record, &actor) {
                found_target = Some((provider, record));
                break;
            }
        }
        let Some((provider, record)) = found_target else {
            return Err(MemoryError::NotFound(id));
        };

        let now = chrono::Utc::now();
        let content_hash = content_hash(&record.content);
        let Some(sink) = &self.event_sink else {
            return Err(MemoryError::Provider {
                provider: "audit".to_owned(),
                source_message: "required audit sink is not configured".to_owned(),
            });
        };
        let provider_id = provider.provider_id().to_owned();
        let target = MemoryWriteTarget {
            kind: record.kind.clone(),
            visibility: record.visibility.clone(),
            destination: WriteDestination::External {
                provider_id: provider_id.clone(),
            },
        };
        match provider.forget(id).await {
            Ok(()) | Err(MemoryError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
        if let Err(error) = sink
            .emit_required(Event::MemoryUpserted(MemoryUpsertedEvent {
                session_id: actor
                    .session_id
                    .or_else(|| record.visibility.session_id())
                    .or_else(|| source_session_id(&record.metadata.source))
                    .unwrap_or_else(SessionId::new),
                run_id,
                memory_id: id,
                kind: record.kind.clone(),
                visibility: record.visibility.clone(),
                action: MemoryWriteAction::Forget,
                provider_id,
                source: record.metadata.source.clone(),
                content_hash: content_hash.clone(),
                bytes_written: 0,
                takes_effect: TakesEffect::CurrentSession,
                at: now,
            }))
            .await
        {
            if let Err(rollback_error) = provider.rollback_uncommitted_forget(record.clone()).await
            {
                return Err(MemoryError::Provider {
                    provider: "audit".to_owned(),
                    source_message: format!(
                        "required audit append failed: {error}; provider rollback failed: {rollback_error}"
                    ),
                });
            }
            return Err(error);
        }
        provider
            .on_memory_write(MemoryWriteAction::Forget, &target, content_hash)
            .await?;
        Ok(())
    }

    pub async fn forget_for_actor_with_policy(
        &self,
        id: MemoryId,
        actor: MemoryActorContext,
        run_id: Option<RunId>,
        policy: &MemoryOperationPolicy,
    ) -> Result<(), MemoryError> {
        let decision = self.policy_engine.read().await.evaluate_delete(
            &policy.thread,
            &policy.actor,
            &policy.permission,
        );
        ensure_direct_memory_policy_allows(decision)?;
        self.forget_for_actor(id, actor, run_id).await
    }

    pub async fn prepare_export_for_actor(
        &self,
        actor: MemoryActorContext,
    ) -> Result<MemoryExportPreparation, MemoryError> {
        let providers = self.readable_providers()?;
        if providers.is_empty() {
            return Err(MemoryError::ExternalProviderNotConfigured);
        }
        let mut records = Vec::new();
        let mut seen = HashSet::new();
        for provider in providers {
            let summaries = provider
                .list(MemoryListScope::ForActor(actor.clone()))
                .await?;
            let mut provider_records = Vec::new();
            for summary in summaries {
                let record = provider.get(summary.id).await?;
                if record_visible_to_actor(&record, &actor) {
                    if !seen.insert(record.id) {
                        continue;
                    }
                    provider_records.push(record);
                }
            }
            records.extend(
                self.scan_records(
                    provider_records,
                    provider.provider_id().to_owned(),
                    actor.session_id,
                )
                .await,
            );
        }
        let Some(sink) = &self.event_sink else {
            return Err(MemoryError::Provider {
                provider: "audit".to_owned(),
                source_message: "required audit sink is not configured".to_owned(),
            });
        };
        let _ = sink;
        let event = MemoryExportedEvent {
            session_id: actor.session_id.unwrap_or_else(SessionId::new),
            tenant_id: actor.tenant_id,
            provider_id: self.provider_id().unwrap_or_else(|| "registry".to_owned()),
            item_count: records.len().min(u32::MAX as usize) as u32,
            content_hashes: records
                .iter()
                .map(|record| content_hash(&record.content))
                .collect(),
            bytes_exported: records
                .iter()
                .map(|record| record.content.len() as u64)
                .sum(),
            at: chrono::Utc::now(),
        };

        Ok(MemoryExportPreparation {
            summaries: records
                .iter()
                .map(redacted_memory_summary_from_record)
                .collect(),
            event,
        })
    }

    pub async fn emit_export_audit(&self, event: MemoryExportedEvent) -> Result<(), MemoryError> {
        let Some(sink) = &self.event_sink else {
            return Err(MemoryError::Provider {
                provider: "audit".to_owned(),
                source_message: "required audit sink is not configured".to_owned(),
            });
        };
        sink.emit_required(Event::MemoryExported(event)).await
    }

    pub async fn initialize_session(&self, ctx: &MemorySessionCtx<'_>) -> Result<(), MemoryError> {
        for provider in self.all_providers()? {
            provider.initialize(ctx).await?;
        }
        Ok(())
    }

    pub async fn on_turn_start(
        &self,
        turn: u32,
        message: &UserMessageView<'_>,
    ) -> Result<(), MemoryError> {
        for provider in self.all_providers()? {
            provider.on_turn_start(turn, message).await?;
        }
        Ok(())
    }

    pub async fn on_pre_compress(
        &self,
        messages: &[MessageView<'_>],
    ) -> Result<Option<String>, MemoryError> {
        for provider in self.all_providers()? {
            if let Some(content) = provider.on_pre_compress(messages).await? {
                return Ok(Some(content));
            }
        }
        Ok(None)
    }

    pub async fn on_memory_write(
        &self,
        action: MemoryWriteAction,
        target: &MemoryWriteTarget,
        content_hash: ContentHash,
    ) -> Result<(), MemoryError> {
        for provider in self.all_providers()? {
            provider
                .on_memory_write(action.clone(), target, content_hash.clone())
                .await?;
        }
        Ok(())
    }

    pub async fn on_delegation(
        &self,
        task: &str,
        result: &str,
        child_session: SessionId,
    ) -> Result<(), MemoryError> {
        for provider in self.all_providers()? {
            provider.on_delegation(task, result, child_session).await?;
        }
        Ok(())
    }

    pub async fn on_session_end(
        &self,
        ctx: &MemorySessionCtx<'_>,
        summary: &SessionSummaryView<'_>,
    ) -> Result<(), MemoryError> {
        for provider in self.all_providers()? {
            provider.on_session_end(ctx, summary).await?;
            provider.shutdown().await?;
        }
        #[cfg(feature = "consolidation")]
        self.run_consolidation(ctx, summary).await?;
        Ok(())
    }

    #[cfg(feature = "consolidation")]
    pub async fn run_consolidation(
        &self,
        ctx: &MemorySessionCtx<'_>,
        summary: &SessionSummaryView<'_>,
    ) -> Result<Option<crate::ConsolidationOutcome>, MemoryError> {
        let Some(hook) = self.consolidation_hook.as_ref().cloned() else {
            return Ok(None);
        };

        let started = Instant::now();
        let outcome = hook.on_session_end(ctx, summary).await?;
        let duration_ms = elapsed_ms(started);
        let hook_id = hook.hook_id().to_owned();
        if let Some(sink) = &self.event_sink {
            sink.emit(Event::MemoryConsolidationRan(MemoryConsolidationRanEvent {
                session_id: ctx.session_id,
                hook_id: hook_id.clone(),
                promoted: outcome.promoted.clone(),
                demoted: outcome.demoted.clone(),
                inbox_candidates_created: outcome.inbox_candidates_created,
                duration_ms,
                at: chrono::Utc::now(),
            }))
            .await;
        }
        self.record_metric(MemoryMetric::ConsolidationRan {
            hook_id,
            promoted: outcome.promoted.len().min(u32::MAX as usize) as u32,
            demoted: outcome.demoted.len().min(u32::MAX as usize) as u32,
        });
        Ok(Some(outcome))
    }

    pub fn record_memdir_overflow(&self, file: MemdirFileTag, current_chars: u64, threshold: u64) {
        self.record_metric(MemoryMetric::MemdirOverflow {
            file,
            current_chars,
            threshold,
        });
    }

    pub async fn recall_outcome(&self, query: MemoryQuery) -> MemoryRecallOutcome {
        self.recall_result(query, None).await.outcome
    }

    pub async fn recall_outcome_with_trace(
        &self,
        query: MemoryQuery,
        run_id: RunId,
        turn: u32,
    ) -> MemoryRecallResult {
        self.recall_result(query, Some((run_id, turn))).await
    }

    async fn recall_result(
        &self,
        query: MemoryQuery,
        trace_context: Option<(RunId, u32)>,
    ) -> MemoryRecallResult {
        let started = Instant::now();
        if !self.should_recall_text(&query.text) {
            let outcome = MemoryRecallOutcome::Skipped;
            self.record_recall_metric(None, &outcome, started);
            return MemoryRecallResult {
                outcome,
                trace_id: None,
                sources: Vec::new(),
            };
        }

        let request_deadline = query
            .deadline
            .unwrap_or(self.recall_policy.default_deadline);
        if request_deadline.is_zero() {
            let outcome = MemoryRecallOutcome::Skipped;
            self.record_recall_metric(None, &outcome, started);
            return MemoryRecallResult {
                outcome,
                trace_id: None,
                sources: Vec::new(),
            };
        }

        let providers = match self.readable_providers() {
            Ok(providers) => providers,
            Err(error) => {
                let outcome = MemoryRecallOutcome::Degraded(error);
                self.record_recall_metric(None, &outcome, started);
                return MemoryRecallResult {
                    outcome,
                    trace_id: None,
                    sources: Vec::new(),
                };
            }
        };
        if providers.is_empty() {
            let outcome = MemoryRecallOutcome::Degraded(MemoryError::ExternalProviderNotConfigured);
            self.record_recall_metric(None, &outcome, started);
            return MemoryRecallResult {
                outcome,
                trace_id: None,
                sources: Vec::new(),
            };
        }

        let mut base_provider_query = query.clone();
        base_provider_query.max_records = base_provider_query
            .max_records
            .min(self.recall_policy.max_records_per_turn);
        base_provider_query.min_similarity = base_provider_query
            .min_similarity
            .max(self.recall_policy.min_similarity);

        let mut collected = Vec::new();
        let mut trace_builder = trace_context.map(|(run_id, turn)| {
            MemoryRecallTraceBuilder::new_for_tenant(
                query.tenant_id,
                query.session_id.unwrap_or_else(SessionId::new),
                run_id,
                turn,
                content_hash(&query.text),
            )
        });
        let mut last_error = None;
        let mut recall_tasks = Vec::new();
        let mut recall_results = Vec::new();
        let has_runtime = tokio::runtime::Handle::try_current().is_ok();
        for provider in providers {
            let provider_id = provider.provider_id().to_owned();
            let descriptor = provider.descriptor();
            let provider_deadline =
                request_deadline.min(Duration::from_millis(u64::from(descriptor.timeout_ms)));
            if provider_deadline.is_zero() {
                let error = MemoryError::RecallDeadlineExceeded {
                    provider: provider_id.clone(),
                };
                last_error = Some((provider_id.clone(), error.clone()));
                let outcome = MemoryRecallOutcome::Degraded(error);
                self.record_recall_metric(Some(&provider_id), &outcome, started);
                if let Some(builder) = trace_builder.take() {
                    trace_builder = Some(builder.add_provider_result(MemoryProviderTrace {
                        provider_id: provider_id.clone(),
                        trust_level: descriptor.trust_level,
                        readable: descriptor.readable,
                        writable: descriptor.writable,
                        requested_count: 0,
                        returned_count: 0,
                        timed_out: true,
                        error_kind: Some("deadline_exceeded".to_owned()),
                        latency_ms: 0,
                    }));
                }
                continue;
            }
            let mut provider_query = base_provider_query.clone();
            provider_query.max_records = provider_query
                .max_records
                .min(descriptor.max_records_per_recall);
            provider_query.deadline = Some(provider_deadline);
            if has_runtime {
                recall_tasks.push((
                    provider_id,
                    descriptor,
                    provider_query.max_records,
                    tokio::spawn(recall_provider_with_deadline(
                        provider,
                        provider_query,
                        provider_deadline,
                    )),
                ));
            } else {
                let run = recall_provider_with_deadline(
                    provider,
                    provider_query.clone(),
                    provider_deadline,
                )
                .await;
                recall_results.push((provider_id, descriptor, provider_query.max_records, run));
            }
        }

        for (provider_id, descriptor, requested_count, task) in recall_tasks {
            let run = match task.await {
                Ok(run) => run,
                Err(error) => ProviderRecallRun {
                    result: Err(MemoryError::Message(format!(
                        "memory provider task failed: {error}"
                    ))),
                    timed_out: false,
                    latency_ms: 0,
                },
            };
            recall_results.push((provider_id, descriptor, requested_count, run));
        }

        for (provider_id, descriptor, requested_count, run) in recall_results {
            if run.timed_out {
                let error = MemoryError::RecallDeadlineExceeded {
                    provider: provider_id.clone(),
                };
                last_error = Some((provider_id.clone(), error.clone()));
                let outcome = MemoryRecallOutcome::Degraded(error);
                self.record_recall_metric(Some(&provider_id), &outcome, started);
                if let Some(builder) = trace_builder.take() {
                    trace_builder = Some(builder.add_provider_result(MemoryProviderTrace {
                        provider_id: provider_id.clone(),
                        trust_level: descriptor.trust_level,
                        readable: descriptor.readable,
                        writable: descriptor.writable,
                        requested_count,
                        returned_count: 0,
                        timed_out: true,
                        error_kind: Some("deadline_exceeded".to_owned()),
                        latency_ms: run.latency_ms,
                    }));
                }
                continue;
            }

            let recalled = match run.result {
                Ok(records) => records,
                Err(error) => {
                    let error_kind = error.to_string();
                    last_error = Some((provider_id.clone(), error));
                    let outcome = MemoryRecallOutcome::Degraded(
                        last_error.as_ref().expect("error just set").1.clone(),
                    );
                    self.record_recall_metric(Some(&provider_id), &outcome, started);
                    if let Some(builder) = trace_builder.take() {
                        trace_builder = Some(builder.add_provider_result(MemoryProviderTrace {
                            provider_id: provider_id.clone(),
                            trust_level: descriptor.trust_level,
                            readable: descriptor.readable,
                            writable: descriptor.writable,
                            requested_count,
                            returned_count: 0,
                            timed_out: false,
                            error_kind: Some(error_kind),
                            latency_ms: run.latency_ms,
                        }));
                    }
                    continue;
                }
            };
            let returned_count = recalled.len().min(u32::MAX as usize) as u32;

            let records = recalled
                .into_iter()
                .filter(|record| record_matches_query(record, &query))
                .collect::<Vec<_>>();
            let records = apply_provider_budgets(records, &descriptor);
            let records = self
                .scan_records(records, provider_id.clone(), query.session_id)
                .await;
            if let Some(builder) = trace_builder.take() {
                let mut builder = builder.add_provider_result(MemoryProviderTrace {
                    provider_id: provider_id.clone(),
                    trust_level: descriptor.trust_level,
                    readable: descriptor.readable,
                    writable: descriptor.writable,
                    requested_count,
                    returned_count,
                    timed_out: false,
                    error_kind: None,
                    latency_ms: run.latency_ms,
                });
                for record in &records {
                    builder = builder.add_candidate(MemoryCandidateTrace {
                        memory_id: record.id,
                        provider_id: provider_id.clone(),
                        content_hash: content_hash(&record.content),
                        score: score_breakdown(record),
                        policy_decision: MemoryPolicyDecision::Allow,
                    });
                }
                trace_builder = Some(builder);
            }
            let provider_outcome = MemoryRecallOutcome::Recalled(records.clone());
            self.record_recall_metric(Some(&provider_id), &provider_outcome, started);
            collected.extend(
                records
                    .into_iter()
                    .map(|record| (record, provider_id.clone())),
            );
        }

        if collected.is_empty() {
            if let Some((_, error)) = last_error {
                return MemoryRecallResult {
                    outcome: MemoryRecallOutcome::Degraded(error),
                    trace_id: None,
                    sources: Vec::new(),
                };
            }
        }

        sort_record_sources_by_score(&mut collected);
        let records_with_provider = dedupe_record_sources(collected);
        let provider_by_id = records_with_provider
            .iter()
            .map(|(record, provider_id)| (record.id, provider_id.clone()))
            .collect::<std::collections::HashMap<_, _>>();
        let records = records_with_provider
            .into_iter()
            .map(|(record, _)| record)
            .take(self.recall_policy.max_records_per_turn as usize)
            .collect::<Vec<_>>();
        let records = apply_char_budget(records, self.recall_policy.max_chars_per_turn);
        let sources = records
            .iter()
            .map(|record| MemoryRecallSource {
                record: record.clone(),
                provider_id: provider_by_id
                    .get(&record.id)
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_owned()),
            })
            .collect::<Vec<_>>();
        let mut trace_id = None;
        if let Some(builder) = trace_builder {
            let mut builder = builder
                .set_redacted(
                    records
                        .iter()
                        .map(|record| record.metadata.redacted_segments)
                        .sum(),
                )
                .set_injected_chars(recall_trace_injected_chars(&records))
                .set_deadline_ms(elapsed_ms(started));
            for record in &records {
                let provider_id = provider_by_id
                    .get(&record.id)
                    .map(String::as_str)
                    .unwrap_or("unknown");
                builder = builder.add_injected(
                    record.id,
                    provider_id,
                    content_hash(&record.content),
                    record.content.len().min(u32::MAX as usize) as u32,
                    "memory-context",
                );
            }
            let trace = builder.build();
            trace_id = Some(trace.trace_id);
            self.trace_collector.add(trace);
        }
        let outcome = MemoryRecallOutcome::Recalled(records);
        self.record_recall_metric(None, &outcome, started);
        MemoryRecallResult {
            outcome,
            trace_id,
            sources,
        }
    }

    pub async fn recall_once_per_turn(
        &self,
        turn: u64,
        query: MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        self.records_from_outcome(self.recall_once_per_turn_outcome(turn, query).await)
    }

    pub async fn recall_once_per_turn_outcome(
        &self,
        turn: u64,
        query: MemoryQuery,
    ) -> MemoryRecallOutcome {
        self.recall_once_per_turn_result(turn, query, None)
            .await
            .outcome
    }

    pub async fn recall_once_per_turn_outcome_with_trace(
        &self,
        turn: u64,
        run_id: RunId,
        query: MemoryQuery,
    ) -> MemoryRecallResult {
        self.recall_once_per_turn_result(turn, query, Some(run_id))
            .await
    }

    pub async fn recall_once_per_turn_outcome_with_policy_and_trace(
        &self,
        turn: u64,
        run_id: RunId,
        query: MemoryQuery,
        thread: &MemoryThreadSettings,
        actor: &MemoryActor,
    ) -> MemoryRecallResult {
        let decision = self
            .policy_engine
            .read()
            .await
            .evaluate_recall(thread, actor);
        if !matches!(decision, MemoryPolicyDecision::Allow) {
            self.record_metric(MemoryMetric::Recall {
                provider_id: None,
                outcome: MemoryRecallMetricOutcome::Skipped,
                duration_ms: 0,
                returned_count: 0,
            });
            return MemoryRecallResult {
                outcome: MemoryRecallOutcome::Skipped,
                trace_id: None,
                sources: Vec::new(),
            };
        }
        self.recall_once_per_turn_result(turn, query, Some(run_id))
            .await
    }

    async fn recall_once_per_turn_result(
        &self,
        turn: u64,
        query: MemoryQuery,
        run_id: Option<RunId>,
    ) -> MemoryRecallResult {
        let action = {
            let mut gate = self.recall_gate.lock().await;
            match gate.as_ref() {
                Some(TurnRecallGate {
                    turn: gate_turn,
                    phase: TurnRecallPhase::InFlight(receiver),
                }) if *gate_turn == turn => RecallGateAction::Wait(receiver.clone()),
                Some(TurnRecallGate {
                    turn: gate_turn,
                    phase: TurnRecallPhase::Completed,
                }) if *gate_turn == turn => RecallGateAction::Skip,
                _ => {
                    let (sender, receiver) = watch::channel(None);
                    *gate = Some(TurnRecallGate {
                        turn,
                        phase: TurnRecallPhase::InFlight(receiver),
                    });
                    RecallGateAction::Lead(sender)
                }
            }
        };

        match action {
            RecallGateAction::Lead(sender) => {
                let result = if let Some(run_id) = run_id {
                    self.recall_outcome_with_trace(
                        query,
                        run_id,
                        turn.min(u64::from(u32::MAX)) as u32,
                    )
                    .await
                } else {
                    MemoryRecallResult {
                        outcome: self.recall_outcome(query).await,
                        trace_id: None,
                        sources: Vec::new(),
                    }
                };
                sender.send_replace(Some(result.clone()));

                let mut gate = self.recall_gate.lock().await;
                if gate.as_ref().is_some_and(|gate| gate.turn == turn) {
                    *gate = match result.outcome {
                        MemoryRecallOutcome::Skipped => None,
                        _ => Some(TurnRecallGate {
                            turn,
                            phase: TurnRecallPhase::Completed,
                        }),
                    };
                }

                result
            }
            RecallGateAction::Wait(receiver) => wait_for_recall_result(receiver).await,
            RecallGateAction::Skip => MemoryRecallResult {
                outcome: MemoryRecallOutcome::Skipped,
                trace_id: None,
                sources: Vec::new(),
            },
        }
    }

    fn records_from_outcome(
        &self,
        outcome: MemoryRecallOutcome,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        match outcome {
            MemoryRecallOutcome::Recalled(records) => Ok(records),
            MemoryRecallOutcome::Skipped => Ok(Vec::new()),
            MemoryRecallOutcome::Degraded(error) => self.handle_recall_failure(error),
        }
    }

    fn sources_from_result(
        &self,
        result: MemoryRecallResult,
    ) -> Result<Vec<MemoryRecallSource>, MemoryError> {
        match result.outcome {
            MemoryRecallOutcome::Recalled(_) => Ok(result.sources),
            MemoryRecallOutcome::Skipped => Ok(Vec::new()),
            MemoryRecallOutcome::Degraded(error) => {
                self.handle_recall_failure(error).map(|_| Vec::new())
            }
        }
    }

    fn handle_recall_failure(&self, error: MemoryError) -> Result<Vec<MemoryRecord>, MemoryError> {
        match self.recall_policy.fail_open {
            FailMode::Skip => Ok(Vec::new()),
            FailMode::Surface => Err(error),
        }
    }

    fn all_providers(&self) -> Result<Vec<Arc<dyn MemoryProvider>>, MemoryError> {
        Ok(self
            .provider_registry
            .try_read()
            .map_err(|_| MemoryError::Message("provider registry lock busy".to_owned()))?
            .provider_arcs_sorted())
    }

    fn readable_providers(&self) -> Result<Vec<Arc<dyn MemoryProvider>>, MemoryError> {
        Ok(self
            .provider_registry
            .try_read()
            .map_err(|_| MemoryError::Message("provider registry lock busy".to_owned()))?
            .readable_provider_arcs_sorted())
    }

    fn writable_providers(&self) -> Result<Vec<Arc<dyn MemoryProvider>>, MemoryError> {
        Ok(self
            .provider_registry
            .try_read()
            .map_err(|_| MemoryError::Message("provider registry lock busy".to_owned()))?
            .writable_providers_sorted())
    }

    async fn writable_provider_record_for_actor(
        &self,
        id: MemoryId,
        actor: MemoryActorContext,
    ) -> Result<(Arc<dyn MemoryProvider>, MemoryRecord), MemoryError> {
        let providers = self.writable_providers()?;
        if providers.is_empty() {
            return Err(MemoryError::ExternalProviderNotConfigured);
        }

        for provider in providers {
            let record = match provider.get(id).await {
                Ok(record) => record,
                Err(MemoryError::NotFound(_)) => continue,
                Err(error) => return Err(error),
            };
            if record_visible_to_actor(&record, &actor) {
                return Ok((provider, record));
            }
        }

        Err(MemoryError::NotFound(id))
    }

    fn write_provider_for_record(
        &self,
        record: &MemoryRecord,
    ) -> Result<Arc<dyn MemoryProvider>, MemoryError> {
        self.write_provider_for_record_with_policy(
            record,
            &MemoryProviderSelectionPolicy::PolicySelected,
        )
    }

    fn write_provider_for_record_with_policy(
        &self,
        record: &MemoryRecord,
        provider_policy: &MemoryProviderSelectionPolicy,
    ) -> Result<Arc<dyn MemoryProvider>, MemoryError> {
        match provider_policy {
            MemoryProviderSelectionPolicy::PolicySelected
            | MemoryProviderSelectionPolicy::DenyModelSelectedProvider => self
                .provider_registry
                .try_read()
                .map_err(|_| MemoryError::Message("provider registry lock busy".to_owned()))?
                .select_write_provider_for_visibility(&record.visibility)
                .ok_or(MemoryError::ExternalProviderNotConfigured),
            MemoryProviderSelectionPolicy::RequireProvider { provider_id } => {
                let provider = self
                    .provider_registry
                    .try_read()
                    .map_err(|_| MemoryError::Message("provider registry lock busy".to_owned()))?
                    .get(provider_id)
                    .ok_or_else(|| MemoryError::Provider {
                        provider: provider_id.clone(),
                        source_message: "required memory provider is not registered".to_owned(),
                    })?;
                ensure_provider_can_write_record(&provider, record)?;
                Ok(provider)
            }
            _ => Err(MemoryError::ExternalProviderNotConfigured),
        }
    }
}

fn ensure_provider_can_write_record(
    provider: &Arc<dyn MemoryProvider>,
    record: &MemoryRecord,
) -> Result<(), MemoryError> {
    let descriptor = provider.descriptor();
    if !descriptor.writable || !descriptor.supports_evidence {
        return Err(MemoryError::Provider {
            provider: descriptor.provider_id,
            source_message: "required memory provider is not writable".to_owned(),
        });
    }
    let Some(visibility_class) = memory_visibility_class(&record.visibility) else {
        return Err(MemoryError::ExternalProviderNotConfigured);
    };
    if !descriptor.allowed_visibility.contains(&visibility_class) {
        return Err(MemoryError::Provider {
            provider: descriptor.provider_id,
            source_message: "required memory provider does not allow record visibility".to_owned(),
        });
    }
    Ok(())
}

fn memory_visibility_class(
    visibility: &MemoryVisibility,
) -> Option<harness_contracts::MemoryVisibilityClass> {
    match visibility {
        MemoryVisibility::Private { .. } => Some(harness_contracts::MemoryVisibilityClass::Private),
        MemoryVisibility::User { .. } => Some(harness_contracts::MemoryVisibilityClass::User),
        MemoryVisibility::Team { .. } => Some(harness_contracts::MemoryVisibilityClass::Team),
        MemoryVisibility::Tenant => Some(harness_contracts::MemoryVisibilityClass::Tenant),
        _ => None,
    }
}

impl MemoryManager {
    #[cfg(feature = "threat-scanner")]
    async fn scan_record_before_write(
        &self,
        record: &mut MemoryRecord,
        session_id: SessionId,
        run_id: Option<RunId>,
        provider_id: Option<String>,
    ) -> Result<(), MemoryError> {
        let Some(scanner) = &self.threat_scanner else {
            return Ok(());
        };

        let content_hash = content_hash(&record.content);
        let report = scanner.scan(&record.content);
        self.emit_threat_events(
            session_id,
            run_id,
            ThreatDirection::OnWrite,
            provider_id,
            content_hash,
            &report,
        )
        .await;
        if report.action == ThreatAction::Block {
            let hit = report.hits.first();
            return Err(MemoryError::ThreatDetected {
                pattern_id: hit
                    .map(|hit| hit.pattern_id.clone())
                    .unwrap_or_else(|| "unknown".to_owned()),
                category: hit
                    .map(|hit| hit.category)
                    .unwrap_or(harness_contracts::ThreatCategory::PromptInjection),
                action: ThreatAction::Block,
            });
        }

        if report.action == ThreatAction::Redact {
            if let Some(redacted_content) = report.redacted_content {
                record.content = redacted_content;
                record.metadata.redacted_segments += report
                    .hits
                    .iter()
                    .filter(|hit| hit.action == ThreatAction::Redact)
                    .count() as u32;
            }
        }

        Ok(())
    }

    #[cfg(feature = "threat-scanner")]
    async fn scan_records(
        &self,
        records: Vec<MemoryRecord>,
        provider_id: String,
        query_session_id: Option<SessionId>,
    ) -> Vec<MemoryRecord> {
        let Some(scanner) = &self.threat_scanner else {
            return records;
        };

        let mut out = Vec::with_capacity(records.len());
        for mut record in records {
            let session_id = query_session_id
                .or_else(|| record.visibility.session_id())
                .or_else(|| source_session_id(&record.metadata.source))
                .unwrap_or_else(SessionId::new);
            let content_hash = content_hash(&record.content);
            let report = scanner.scan(&record.content);
            self.emit_threat_events(
                session_id,
                None,
                ThreatDirection::OnRecall,
                Some(provider_id.clone()),
                content_hash,
                &report,
            )
            .await;
            if report.action == ThreatAction::Block {
                continue;
            }

            if report.action == ThreatAction::Redact {
                if let Some(redacted_content) = report.redacted_content {
                    record.content = redacted_content;
                    record.metadata.redacted_segments += report
                        .hits
                        .iter()
                        .filter(|hit| hit.action == ThreatAction::Redact)
                        .count() as u32;
                }
            }

            out.push(record);
        }
        out
    }

    #[cfg(feature = "threat-scanner")]
    async fn emit_threat_events(
        &self,
        session_id: SessionId,
        run_id: Option<RunId>,
        direction: ThreatDirection,
        provider_id: Option<String>,
        content_hash: ContentHash,
        report: &crate::ThreatScanReport,
    ) {
        if report.hits.is_empty() {
            return;
        }
        let Some(sink) = &self.event_sink else {
            return;
        };

        for hit in &report.hits {
            sink.emit(Event::MemoryThreatDetected(MemoryThreatDetectedEvent {
                session_id,
                run_id,
                pattern_id: hit.pattern_id.clone(),
                category: hit.category,
                severity: hit.severity,
                action: hit.action,
                direction,
                provider_id: provider_id.clone(),
                content_hash: content_hash.clone(),
                at: chrono::Utc::now(),
            }))
            .await;
            self.record_metric(MemoryMetric::ThreatDetected {
                category: hit.category,
                action: hit.action,
            });
        }
    }

    #[cfg(not(feature = "threat-scanner"))]
    async fn scan_records(
        &self,
        records: Vec<MemoryRecord>,
        _provider_id: String,
        _query_session_id: Option<SessionId>,
    ) -> Vec<MemoryRecord> {
        records
    }
}

impl MemoryManager {
    fn record_recall_metric(
        &self,
        provider_id: Option<&str>,
        outcome: &MemoryRecallOutcome,
        started: Instant,
    ) {
        let (outcome, returned_count) = match outcome {
            MemoryRecallOutcome::Recalled(records) if records.is_empty() => {
                (MemoryRecallMetricOutcome::Empty, 0)
            }
            MemoryRecallOutcome::Recalled(records) => (
                MemoryRecallMetricOutcome::Recalled,
                records.len().min(u32::MAX as usize) as u32,
            ),
            MemoryRecallOutcome::Skipped => (MemoryRecallMetricOutcome::Skipped, 0),
            MemoryRecallOutcome::Degraded(error) => {
                self.record_metric(MemoryMetric::RecallDegraded {
                    provider_id: provider_id.map(ToOwned::to_owned),
                    reason: error.to_string(),
                });
                (MemoryRecallMetricOutcome::Degraded, 0)
            }
        };

        self.record_metric(MemoryMetric::Recall {
            provider_id: provider_id.map(ToOwned::to_owned),
            outcome,
            duration_ms: elapsed_ms(started),
            returned_count,
        });
        if matches!(
            outcome,
            MemoryRecallMetricOutcome::Recalled | MemoryRecallMetricOutcome::Empty
        ) {
            self.record_metric(MemoryMetric::RecallHitRateSample {
                provider_id: provider_id.map(ToOwned::to_owned),
                hit: outcome == MemoryRecallMetricOutcome::Recalled,
            });
        }
    }

    fn record_metric(&self, metric: MemoryMetric) {
        if let Some(sink) = &self.metrics_sink {
            sink.record(metric);
        }
    }
}

trait MemoryVisibilitySessionId {
    fn session_id(&self) -> Option<SessionId>;
}

impl MemoryVisibilitySessionId for MemoryVisibility {
    fn session_id(&self) -> Option<SessionId> {
        match self {
            MemoryVisibility::Private { session_id } => Some(*session_id),
            _ => None,
        }
    }
}

fn source_session_id(source: &MemorySource) -> Option<SessionId> {
    match source {
        MemorySource::SubagentDerived { child_session } => Some(*child_session),
        _ => None,
    }
}

fn memory_summary_from_record(record: &MemoryRecord) -> MemorySummary {
    MemorySummary {
        id: record.id,
        kind: record.kind.clone(),
        visibility: record.visibility.clone(),
        content_preview: content_preview(&record.content),
        metadata: record.metadata.clone(),
        updated_at: record.updated_at,
    }
}

fn redacted_memory_summary_from_record(record: &MemoryRecord) -> MemorySummary {
    let mut summary = memory_summary_from_record(record);
    summary.content_preview = "[redacted memory content]".to_owned();
    summary
}

fn record_visible_to_actor(record: &MemoryRecord, actor: &MemoryActorContext) -> bool {
    record.tenant_id == actor.tenant_id && visibility_matches(&record.visibility, actor)
}

fn content_hash(content: &str) -> ContentHash {
    ContentHash(*blake3::hash(content.as_bytes()).as_bytes())
}

fn ensure_direct_memory_policy_allows(decision: MemoryPolicyDecision) -> Result<(), MemoryError> {
    match decision {
        MemoryPolicyDecision::Allow => Ok(()),
        MemoryPolicyDecision::Deny { reason } | MemoryPolicyDecision::CandidateOnly { reason } => {
            Err(MemoryError::Message(format!(
                "memory write denied by policy: {reason:?}"
            )))
        }
        _ => Err(MemoryError::Message(
            "memory write denied by policy".to_owned(),
        )),
    }
}

fn dedupe_record_sources(records: Vec<(MemoryRecord, String)>) -> Vec<(MemoryRecord, String)> {
    let mut seen_ids = HashSet::new();
    let mut seen_content_context = HashSet::new();
    let mut deduped = Vec::with_capacity(records.len());

    for (record, provider_id) in records {
        let content_key = (
            content_hash(&record.content),
            record.metadata.source.clone(),
            record
                .metadata
                .evidence
                .as_ref()
                .and_then(|evidence| serde_json::to_string(evidence).ok()),
        );
        if seen_ids.insert(record.id) && seen_content_context.insert(content_key) {
            deduped.push((record, provider_id));
        }
    }

    deduped
}

fn sort_record_sources_by_score(records: &mut [(MemoryRecord, String)]) {
    records.sort_by(|(left, _), (right, _)| {
        score_cmp(right.metadata.recall_score, left.metadata.recall_score)
    });
}

fn score_cmp(left: f32, right: f32) -> Ordering {
    left.partial_cmp(&right).unwrap_or(Ordering::Equal)
}

struct ProviderRecallRun {
    result: Result<Vec<MemoryRecord>, MemoryError>,
    timed_out: bool,
    latency_ms: u32,
}

async fn recall_provider_with_deadline(
    provider: Arc<dyn MemoryProvider>,
    query: MemoryQuery,
    deadline: Duration,
) -> ProviderRecallRun {
    let started = Instant::now();
    match timeout(deadline, provider.recall(query)).await {
        Ok(result) => ProviderRecallRun {
            result,
            timed_out: false,
            latency_ms: elapsed_ms(started),
        },
        Err(_) => ProviderRecallRun {
            result: Err(MemoryError::RecallDeadlineExceeded {
                provider: provider.provider_id().to_owned(),
            }),
            timed_out: true,
            latency_ms: elapsed_ms(started),
        },
    }
}

async fn wait_for_recall_result(
    mut receiver: watch::Receiver<Option<RecallResult>>,
) -> RecallResult {
    loop {
        if let Some(result) = receiver.borrow().clone() {
            return result;
        }

        if receiver.changed().await.is_err() {
            return MemoryRecallResult {
                outcome: MemoryRecallOutcome::Skipped,
                trace_id: None,
                sources: Vec::new(),
            };
        }
    }
}

fn record_matches_query(record: &MemoryRecord, query: &MemoryQuery) -> bool {
    record.tenant_id == query.tenant_id
        && kind_matches(record, query.kind_filter.as_ref())
        && visibility_filter_matches(record, &query.visibility_filter)
}

fn kind_matches(record: &MemoryRecord, filter: Option<&MemoryKindFilter>) -> bool {
    match filter {
        None | Some(MemoryKindFilter::Any) => true,
        Some(MemoryKindFilter::OnlyKinds(kinds)) => kinds.contains(&record.kind),
    }
}

fn visibility_filter_matches(record: &MemoryRecord, filter: &MemoryVisibilityFilter) -> bool {
    match filter {
        MemoryVisibilityFilter::EffectiveFor(actor) => {
            record.tenant_id == actor.tenant_id && visibility_matches(&record.visibility, actor)
        }
        MemoryVisibilityFilter::Exact(visibility) => &record.visibility == visibility,
    }
}

fn apply_char_budget(records: Vec<MemoryRecord>, max_chars: u32) -> Vec<MemoryRecord> {
    let mut used = 0usize;
    let max_chars = max_chars as usize;

    records
        .into_iter()
        .filter(|record| {
            let record_chars = record.content.chars().count();
            if record_chars > max_chars || used + record_chars > max_chars {
                return false;
            }

            used += record_chars;
            true
        })
        .collect()
}

fn apply_provider_budgets(
    records: Vec<MemoryRecord>,
    descriptor: &crate::MemoryProviderDescriptor,
) -> Vec<MemoryRecord> {
    let mut used_chars = 0usize;
    let max_chars = descriptor.max_chars_per_recall as usize;
    let max_bytes = descriptor.max_bytes_per_record as usize;
    let max_records = descriptor.max_records_per_recall as usize;

    records
        .into_iter()
        .filter(|record| record.content.len() <= max_bytes)
        .filter(|record| {
            let record_chars = record.content.chars().count();
            if record_chars > max_chars || used_chars + record_chars > max_chars {
                return false;
            }

            used_chars += record_chars;
            true
        })
        .take(max_records)
        .collect()
}

fn score_breakdown(record: &MemoryRecord) -> MemoryScoreBreakdown {
    let final_score = record.metadata.recall_score;
    MemoryScoreBreakdown {
        lexical_score: final_score,
        vector_score: None,
        confidence_score: record.metadata.confidence,
        recency_score: 0.0,
        access_score: (record.metadata.access_count as f32).min(1.0),
        source_trust_score: 1.0,
        explicit_selection_boost: 0.0,
        final_score,
    }
}

#[cfg(feature = "builtin")]
fn recall_trace_injected_chars(records: &[MemoryRecord]) -> u32 {
    crate::wrap_memory_context(records)
        .len()
        .min(u32::MAX as usize) as u32
}

#[cfg(not(feature = "builtin"))]
fn recall_trace_injected_chars(records: &[MemoryRecord]) -> u32 {
    records
        .iter()
        .map(|record| record.content.len())
        .sum::<usize>()
        .min(u32::MAX as usize) as u32
}

fn elapsed_ms(started: Instant) -> u32 {
    started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32
}

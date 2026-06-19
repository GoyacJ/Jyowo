use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(feature = "consolidation")]
use harness_contracts::MemoryConsolidationRanEvent;
use harness_contracts::{
    ContentHash, Event, MemdirFileTag, MemoryActor, MemoryError, MemoryExportedEvent, MemoryId,
    MemorySessionCtx, MemorySource, MemoryUpsertedEvent, MemoryVisibility, MemoryWriteAction,
    MemoryWriteTarget, MessageView, RunId, SessionId, SessionSummaryView, TakesEffect,
    UserMessageView, WriteDestination,
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
    MemoryMetric, MemoryMetricsSink, MemoryProvider, MemoryQuery, MemoryRecallMetricOutcome,
    MemoryRecord, MemorySummary, MemoryVisibilityFilter,
};

pub struct MemoryManager {
    #[cfg(feature = "builtin")]
    builtin: RwLock<Option<BuiltinMemory>>,
    external: RwLock<Option<Arc<dyn MemoryProvider>>>,
    recall_policy: RecallPolicy,
    #[cfg(feature = "consolidation")]
    consolidation_hook: Option<Arc<dyn ConsolidationHook>>,
    recall_gate: Mutex<Option<TurnRecallGate>>,
    event_sink: Option<Arc<dyn MemoryEventSink>>,
    metrics_sink: Option<Arc<dyn MemoryMetricsSink>>,
    #[cfg(feature = "threat-scanner")]
    threat_scanner: Option<Arc<MemoryThreatScanner>>,
}

type RecallResult = MemoryRecallOutcome;

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
            external: RwLock::new(None),
            recall_policy: RecallPolicy::default(),
            #[cfg(feature = "consolidation")]
            consolidation_hook: None,
            recall_gate: Mutex::new(None),
            event_sink: None,
            metrics_sink: None,
            #[cfg(feature = "threat-scanner")]
            threat_scanner: None,
        }
    }
}

impl MemoryManager {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_external(&self, provider: Arc<dyn MemoryProvider>) -> Result<(), MemoryError> {
        let mut slot = self
            .external
            .try_write()
            .map_err(|_| MemoryError::ExternalSlotLockBusy)?;
        if slot.is_some() {
            return Err(MemoryError::ExternalSlotOccupied);
        }

        *slot = Some(provider);
        self.record_metric(MemoryMetric::ExternalProviderConfigured { configured: true });
        Ok(())
    }

    pub fn external(&self) -> Option<Arc<dyn MemoryProvider>> {
        self.external.try_read().ok().and_then(|slot| slot.clone())
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
        self.external()
            .as_ref()
            .map(|provider| provider.provider_id().to_owned())
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
        self.external().is_some()
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

    pub async fn upsert(
        &self,
        mut record: MemoryRecord,
        run_id: Option<RunId>,
    ) -> Result<harness_contracts::MemoryId, MemoryError> {
        let Some(provider) = self.external.read().await.clone() else {
            return Err(MemoryError::ExternalProviderNotConfigured);
        };
        let now = chrono::Utc::now();
        record.updated_at = now;
        let session_id = record
            .visibility
            .session_id()
            .or_else(|| source_session_id(&record.metadata.source))
            .unwrap_or_else(SessionId::new);
        #[cfg(feature = "threat-scanner")]
        self.scan_record_before_write(
            &mut record,
            session_id,
            run_id,
            Some(provider.provider_id().to_owned()),
        )
        .await?;
        let content_hash = content_hash(&record.content);
        let bytes_written = record.content.len() as u64;
        let target = MemoryWriteTarget {
            kind: record.kind.clone(),
            visibility: record.visibility.clone(),
            destination: WriteDestination::External {
                provider_id: provider.provider_id().to_owned(),
            },
        };
        let metric_kind = record.kind.clone();
        let metric_visibility = record.visibility.clone();
        let id = provider.upsert(record.clone()).await?;
        provider
            .on_memory_write(MemoryWriteAction::Upsert, &target, content_hash.clone())
            .await?;
        if let Some(sink) = &self.event_sink {
            sink.emit(Event::MemoryUpserted(MemoryUpsertedEvent {
                session_id: record
                    .visibility
                    .session_id()
                    .or_else(|| source_session_id(&record.metadata.source))
                    .unwrap_or(session_id),
                run_id,
                memory_id: id,
                kind: record.kind.clone(),
                visibility: record.visibility.clone(),
                action: MemoryWriteAction::Upsert,
                provider_id: provider.provider_id().to_owned(),
                source: record.metadata.source.clone(),
                content_hash,
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

    pub async fn list_for_actor(
        &self,
        actor: MemoryActor,
    ) -> Result<Vec<crate::MemorySummary>, MemoryError> {
        let Some(provider) = self.external.read().await.clone() else {
            return Err(MemoryError::ExternalProviderNotConfigured);
        };

        let summaries = provider
            .list(MemoryListScope::ForActor(actor.clone()))
            .await?;
        let mut visible = Vec::with_capacity(summaries.len());
        for summary in summaries {
            let record = provider.get(summary.id).await?;
            if record_visible_to_actor(&record, &actor) {
                let scanned = self
                    .scan_records(
                        vec![record],
                        provider.provider_id().to_owned(),
                        actor.session_id,
                    )
                    .await;
                if let Some(record) = scanned.into_iter().next() {
                    visible.push(memory_summary_from_record(&record));
                }
            }
        }

        Ok(visible)
    }

    pub async fn get_for_actor(
        &self,
        id: MemoryId,
        actor: MemoryActor,
    ) -> Result<MemoryRecord, MemoryError> {
        let Some(provider) = self.external.read().await.clone() else {
            return Err(MemoryError::ExternalProviderNotConfigured);
        };
        let record = provider.get(id).await?;
        if !record_visible_to_actor(&record, &actor) {
            return Err(MemoryError::NotFound(id));
        }

        let records = self
            .scan_records(
                vec![record],
                provider.provider_id().to_owned(),
                actor.session_id,
            )
            .await;
        records.into_iter().next().ok_or(MemoryError::NotFound(id))
    }

    pub async fn update_content_for_actor(
        &self,
        id: MemoryId,
        actor: MemoryActor,
        content: impl Into<String>,
        run_id: Option<RunId>,
    ) -> Result<MemoryRecord, MemoryError> {
        let mut record = self.get_for_actor(id, actor.clone()).await?;
        record.content = content.into();
        record.updated_at = chrono::Utc::now();
        self.upsert(record, run_id).await?;
        self.get_for_actor(id, actor).await
    }

    pub async fn forget_for_actor(
        &self,
        id: MemoryId,
        actor: MemoryActor,
        run_id: Option<RunId>,
    ) -> Result<(), MemoryError> {
        let Some(provider) = self.external.read().await.clone() else {
            return Err(MemoryError::ExternalProviderNotConfigured);
        };
        let record = provider.get(id).await?;
        if !record_visible_to_actor(&record, &actor) {
            return Err(MemoryError::NotFound(id));
        }

        let now = chrono::Utc::now();
        let content_hash = content_hash(&record.content);
        let target = MemoryWriteTarget {
            kind: record.kind.clone(),
            visibility: record.visibility.clone(),
            destination: WriteDestination::External {
                provider_id: provider.provider_id().to_owned(),
            },
        };
        let Some(sink) = &self.event_sink else {
            return Err(MemoryError::Provider {
                provider: "audit".to_owned(),
                source_message: "required audit sink is not configured".to_owned(),
            });
        };
        provider.forget(id).await?;
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
                provider_id: provider.provider_id().to_owned(),
                source: record.metadata.source.clone(),
                content_hash: content_hash.clone(),
                bytes_written: 0,
                takes_effect: TakesEffect::CurrentSession,
                at: now,
            }))
            .await
        {
            let _ = provider.upsert(record).await;
            return Err(error);
        }
        provider
            .on_memory_write(MemoryWriteAction::Forget, &target, content_hash.clone())
            .await?;
        Ok(())
    }

    pub async fn export_for_actor(
        &self,
        actor: MemoryActor,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        let Some(provider) = self.external.read().await.clone() else {
            return Err(MemoryError::ExternalProviderNotConfigured);
        };
        let summaries = provider
            .list(MemoryListScope::ForActor(actor.clone()))
            .await?;
        let mut records = Vec::with_capacity(summaries.len());
        for summary in summaries {
            let record = provider.get(summary.id).await?;
            if record_visible_to_actor(&record, &actor) {
                records.push(record);
            }
        }
        let records = self
            .scan_records(records, provider.provider_id().to_owned(), actor.session_id)
            .await;
        let Some(sink) = &self.event_sink else {
            return Err(MemoryError::Provider {
                provider: "audit".to_owned(),
                source_message: "required audit sink is not configured".to_owned(),
            });
        };
        sink.emit_required(Event::MemoryExported(MemoryExportedEvent {
            session_id: actor.session_id.unwrap_or_else(SessionId::new),
            tenant_id: actor.tenant_id,
            provider_id: provider.provider_id().to_owned(),
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
        }))
        .await?;

        Ok(records)
    }

    pub async fn initialize_session(&self, ctx: &MemorySessionCtx<'_>) -> Result<(), MemoryError> {
        if let Some(provider) = self.external.read().await.clone() {
            provider.initialize(ctx).await?;
        }
        Ok(())
    }

    pub async fn on_turn_start(
        &self,
        turn: u32,
        message: &UserMessageView<'_>,
    ) -> Result<(), MemoryError> {
        if let Some(provider) = self.external.read().await.clone() {
            provider.on_turn_start(turn, message).await?;
        }
        Ok(())
    }

    pub async fn on_pre_compress(
        &self,
        messages: &[MessageView<'_>],
    ) -> Result<Option<String>, MemoryError> {
        if let Some(provider) = self.external.read().await.clone() {
            return provider.on_pre_compress(messages).await;
        }
        Ok(None)
    }

    pub async fn on_memory_write(
        &self,
        action: MemoryWriteAction,
        target: &MemoryWriteTarget,
        content_hash: ContentHash,
    ) -> Result<(), MemoryError> {
        if let Some(provider) = self.external.read().await.clone() {
            provider
                .on_memory_write(action, target, content_hash)
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
        if let Some(provider) = self.external.read().await.clone() {
            provider.on_delegation(task, result, child_session).await?;
        }
        Ok(())
    }

    pub async fn on_session_end(
        &self,
        ctx: &MemorySessionCtx<'_>,
        summary: &SessionSummaryView<'_>,
    ) -> Result<(), MemoryError> {
        if let Some(provider) = self.external.read().await.clone() {
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
                draft_dreams_chars: outcome.draft_dreams.chars().count().min(u32::MAX as usize)
                    as u32,
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
        let started = Instant::now();
        if !self.should_recall_text(&query.text) {
            let outcome = MemoryRecallOutcome::Skipped;
            self.record_recall_metric(None, &outcome, started);
            return outcome;
        }

        let deadline = query
            .deadline
            .unwrap_or(self.recall_policy.default_deadline);
        if deadline.is_zero() {
            let outcome = MemoryRecallOutcome::Skipped;
            self.record_recall_metric(None, &outcome, started);
            return outcome;
        }

        let Some(provider) = self.external.read().await.clone() else {
            let outcome = MemoryRecallOutcome::Skipped;
            self.record_recall_metric(None, &outcome, started);
            return outcome;
        };
        let provider_id = provider.provider_id().to_owned();

        let mut provider_query = query.clone();
        provider_query.max_records = provider_query
            .max_records
            .min(self.recall_policy.max_records_per_turn);
        provider_query.min_similarity = provider_query
            .min_similarity
            .max(self.recall_policy.min_similarity);

        let recalled = match timeout(deadline, provider.recall(provider_query)).await {
            Ok(Ok(records)) => records,
            Ok(Err(error)) => {
                let outcome = MemoryRecallOutcome::Degraded(error);
                self.record_recall_metric(Some(&provider_id), &outcome, started);
                return outcome;
            }
            Err(_) => {
                let outcome = MemoryRecallOutcome::Degraded(MemoryError::RecallDeadlineExceeded {
                    provider: provider_id.clone(),
                });
                self.record_recall_metric(Some(&provider_id), &outcome, started);
                return outcome;
            }
        };

        let records = recalled
            .into_iter()
            .filter(|record| record_matches_query(record, &query))
            .take(self.recall_policy.max_records_per_turn as usize)
            .collect::<Vec<_>>();
        let records = self
            .scan_records(records, provider_id.clone(), query.session_id)
            .await;
        let outcome = MemoryRecallOutcome::Recalled(apply_char_budget(
            records,
            self.recall_policy.max_chars_per_turn,
        ));
        self.record_recall_metric(Some(&provider_id), &outcome, started);
        outcome
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
                let result = self.recall_outcome(query).await;
                sender.send_replace(Some(result.clone()));

                let mut gate = self.recall_gate.lock().await;
                if gate.as_ref().is_some_and(|gate| gate.turn == turn) {
                    *gate = match result {
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
            RecallGateAction::Skip => MemoryRecallOutcome::Skipped,
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

    fn handle_recall_failure(&self, error: MemoryError) -> Result<Vec<MemoryRecord>, MemoryError> {
        match self.recall_policy.fail_open {
            FailMode::Skip => Ok(Vec::new()),
            FailMode::Surface => Err(error),
        }
    }

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

fn record_visible_to_actor(record: &MemoryRecord, actor: &MemoryActor) -> bool {
    record.tenant_id == actor.tenant_id && visibility_matches(&record.visibility, actor)
}

fn content_hash(content: &str) -> ContentHash {
    ContentHash(*blake3::hash(content.as_bytes()).as_bytes())
}

async fn wait_for_recall_result(
    mut receiver: watch::Receiver<Option<RecallResult>>,
) -> RecallResult {
    loop {
        if let Some(result) = receiver.borrow().clone() {
            return result;
        }

        if receiver.changed().await.is_err() {
            return MemoryRecallOutcome::Skipped;
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

fn elapsed_ms(started: Instant) -> u32 {
    started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32
}

#![cfg(all(
    feature = "builtin",
    feature = "consolidation",
    feature = "external-slot",
    feature = "threat-scanner"
))]

use std::collections::BTreeSet;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{
    EndReason, Event, MemdirFileTag, MemoryActorContext, MemoryError, MemoryId, MemoryKind,
    MemorySessionCtx, MemorySource, MemoryVisibility, MemoryWriteAction, SessionId,
    SessionSummaryView, TenantId, ThreatAction, ThreatCategory, UsageSnapshot,
};
use harness_memory::{
    BuiltinMemory, ConsolidationHook, ConsolidationOutcome, MemdirFile, MemoryEventSink,
    MemoryKindFilter, MemoryLifecycle, MemoryListScope, MemoryManager, MemoryMetadata,
    MemoryMetric, MemoryMetricsSink, MemoryQuery, MemoryRecallMetricOutcome, MemoryRecord,
    MemoryStore, MemorySummary, MemoryThreatScanner, MemoryVisibilityFilter, ThreatPattern,
};

#[tokio::test]
async fn consolidation_hook_runs_on_session_end_and_emits_event() {
    let events = Arc::new(Events::default());
    let metrics = Arc::new(Metrics::default());
    let promoted = MemoryId::new();
    let hook = Arc::new(TestConsolidationHook {
        calls: AtomicUsize::new(0),
        promoted,
    });
    let manager = MemoryManager::new()
        .with_event_sink(events.clone())
        .with_metrics_sink(metrics.clone())
        .with_consolidation_hook(hook.clone());

    manager
        .on_session_end(&session_ctx(), &session_summary())
        .await
        .unwrap();

    assert_eq!(hook.calls.load(Ordering::SeqCst), 1);
    assert!(events.events().iter().any(|event| matches!(
        event,
        Event::MemoryConsolidationRan(ran)
            if ran.hook_id == "test-consolidation"
                && ran.promoted == vec![promoted]
                && ran.inbox_candidates_created == 2
    )));
    assert!(metrics.metrics().iter().any(|metric| matches!(
        metric,
        MemoryMetric::ConsolidationRan { hook_id, promoted: 1, demoted: 0 }
            if hook_id == "test-consolidation"
    )));
}

#[tokio::test]
async fn memory_metrics_cover_recall_degraded_threat_memdir_and_overflow() {
    let metrics = Arc::new(Metrics::default());
    let manager = MemoryManager::new().with_metrics_sink(metrics.clone());
    manager
        .set_external(Arc::new(StaticProvider::ok(vec![record("recall hit")])))
        .unwrap();
    manager
        .recall(query(Duration::from_millis(200)))
        .await
        .unwrap();
    manager.record_memdir_overflow(MemdirFileTag::Memory, 12_000, 8_000);

    let degraded = MemoryManager::new().with_metrics_sink(metrics.clone());
    degraded
        .set_external(Arc::new(StaticProvider::err("provider down")))
        .unwrap();
    degraded
        .recall(query(Duration::from_millis(200)))
        .await
        .unwrap();

    let root = tempfile::tempdir().unwrap();
    let scanner = MemoryThreatScanner::from_patterns(vec![ThreatPattern::new(
        "warn-memory",
        "warn-me",
        ThreatCategory::PromptInjection,
        harness_contracts::Severity::Medium,
        ThreatAction::Warn,
    )
    .unwrap()]);
    BuiltinMemory::at(root.path(), TenantId::SINGLE)
        .with_metrics_sink(metrics.clone())
        .with_threat_scanner(Arc::new(scanner))
        .append_section(MemdirFile::Memory, "threat", "warn-me")
        .await
        .unwrap();

    let recorded = metrics.metrics();
    assert!(recorded.iter().any(|metric| matches!(
        metric,
        MemoryMetric::ExternalProviderConfigured { configured: true }
    )));
    assert!(recorded.iter().any(|metric| matches!(
        metric,
        MemoryMetric::Recall {
            outcome: MemoryRecallMetricOutcome::Recalled,
            returned_count: 1,
            ..
        }
    )));
    assert!(recorded.iter().any(|metric| matches!(
        metric,
        MemoryMetric::RecallDegraded { reason, .. } if reason.contains("provider down")
    )));
    assert!(recorded.iter().any(|metric| matches!(
        metric,
        MemoryMetric::MemdirWrite {
            file: MemdirFileTag::Memory,
            action: MemoryWriteAction::AppendSection { section },
            bytes_written,
        } if section == "threat" && *bytes_written > 0
    )));
    assert!(recorded.iter().any(|metric| matches!(
        metric,
        MemoryMetric::ThreatDetected {
            category: ThreatCategory::PromptInjection,
            action: ThreatAction::Warn,
        }
    )));
    assert!(recorded.iter().any(|metric| matches!(
        metric,
        MemoryMetric::MemdirOverflow {
            file: MemdirFileTag::Memory,
            current_chars: 12_000,
            threshold: 8_000,
        }
    )));
}

#[tokio::test]
async fn memory_metrics_cover_spec_recall_and_memdir_lock_metrics() {
    let metrics = Arc::new(Metrics::default());

    let recalled = MemoryManager::new().with_metrics_sink(metrics.clone());
    recalled
        .set_external(Arc::new(StaticProvider::ok(vec![record("recall hit")])))
        .unwrap();
    recalled
        .recall(query(Duration::from_millis(200)))
        .await
        .unwrap();

    let empty = MemoryManager::new().with_metrics_sink(metrics.clone());
    empty
        .set_external(Arc::new(StaticProvider::ok(Vec::new())))
        .unwrap();
    empty
        .recall(query(Duration::from_millis(200)))
        .await
        .unwrap();

    let root = tempfile::tempdir().unwrap();
    let memory = BuiltinMemory::at(root.path(), TenantId::SINGLE)
        .with_metrics_sink(metrics.clone())
        .with_concurrency_policy(harness_memory::MemdirConcurrencyPolicy {
            lock_timeout: Duration::from_millis(25),
            retry_max: 1,
            retry_jitter_ms: 1..=1,
        });
    memory
        .append_section(MemdirFile::Memory, "metric", "content")
        .await
        .unwrap();
    memory.read_all().await.unwrap();

    let lock_path = root
        .path()
        .join(TenantId::SINGLE.to_string())
        .join(".locks/MEMORY.md.lock");
    let lock_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .unwrap();
    fs2::FileExt::lock_exclusive(&lock_file).unwrap();
    let error = memory
        .append_section(MemdirFile::Memory, "blocked", "content")
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        MemoryError::ConcurrentWriteLockFailed { retries: 1 }
    ));

    let recorded = metrics.metrics();
    assert!(recorded.iter().any(|metric| matches!(
        metric,
        MemoryMetric::Recall {
            outcome: MemoryRecallMetricOutcome::Empty,
            returned_count: 0,
            ..
        }
    )));
    assert!(recorded
        .iter()
        .any(|metric| matches!(metric, MemoryMetric::RecallHitRateSample { hit: true, .. })));
    assert!(recorded
        .iter()
        .any(|metric| matches!(metric, MemoryMetric::RecallHitRateSample { hit: false, .. })));
    assert!(recorded.iter().any(|metric| matches!(
        metric,
        MemoryMetric::MemdirBytes {
            file: MemdirFileTag::Memory,
            bytes,
        } if *bytes > 0
    )));
    assert!(recorded.iter().any(|metric| matches!(
        metric,
        MemoryMetric::MemdirLockWait {
            file: MemdirFileTag::Memory,
            waited_ms: _,
        }
    )));
    assert!(recorded.iter().any(|metric| matches!(
        metric,
        MemoryMetric::MemdirLockFailed {
            file: MemdirFileTag::Memory,
            retries: 1,
        }
    )));
}

#[derive(Default)]
struct Events {
    events: Mutex<Vec<Event>>,
}

impl Events {
    fn events(&self) -> Vec<Event> {
        self.events.lock().unwrap().clone()
    }
}

#[async_trait]
impl MemoryEventSink for Events {
    async fn emit(&self, event: Event) {
        self.events.lock().unwrap().push(event);
    }
}

#[derive(Default)]
struct Metrics {
    metrics: Mutex<Vec<MemoryMetric>>,
}

impl Metrics {
    fn metrics(&self) -> Vec<MemoryMetric> {
        self.metrics.lock().unwrap().clone()
    }
}

impl MemoryMetricsSink for Metrics {
    fn record(&self, metric: MemoryMetric) {
        self.metrics.lock().unwrap().push(metric);
    }
}

struct TestConsolidationHook {
    calls: AtomicUsize,
    promoted: MemoryId,
}

#[async_trait]
impl ConsolidationHook for TestConsolidationHook {
    fn hook_id(&self) -> &str {
        "test-consolidation"
    }

    async fn on_session_end(
        &self,
        _ctx: &MemorySessionCtx<'_>,
        _summary: &SessionSummaryView<'_>,
    ) -> Result<ConsolidationOutcome, MemoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ConsolidationOutcome {
            promoted: vec![self.promoted],
            demoted: Vec::new(),
            inbox_candidates_created: 2,
        })
    }
}

struct StaticProvider {
    result: Result<Vec<MemoryRecord>, MemoryError>,
}

impl StaticProvider {
    fn ok(records: Vec<MemoryRecord>) -> Self {
        Self {
            result: Ok(records),
        }
    }

    fn err(message: &str) -> Self {
        Self {
            result: Err(MemoryError::Message(message.to_owned())),
        }
    }
}

#[async_trait]
impl MemoryStore for StaticProvider {
    fn provider_id(&self) -> &'static str {
        "static"
    }

    async fn recall(&self, _query: MemoryQuery) -> Result<Vec<MemoryRecord>, MemoryError> {
        self.result.clone()
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _id: MemoryId) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn list(&self, _scope: MemoryListScope) -> Result<Vec<MemorySummary>, MemoryError> {
        Ok(Vec::new())
    }
}

impl MemoryLifecycle for StaticProvider {}

fn query(deadline: Duration) -> MemoryQuery {
    MemoryQuery {
        text: "recall?".to_owned(),
        kind_filter: Some(MemoryKindFilter::OnlyKinds(BTreeSet::from([
            MemoryKind::UserPreference,
        ]))),
        visibility_filter: MemoryVisibilityFilter::EffectiveFor(MemoryActorContext {
            tenant_id: TenantId::SINGLE,
            user_id: None,
            team_id: None,
            session_id: Some(SessionId::new()),
        }),
        max_records: 8,
        min_similarity: 0.0,
        tenant_id: TenantId::SINGLE,
        session_id: Some(SessionId::new()),
        deadline: Some(deadline),
    }
}

fn record(content: &str) -> MemoryRecord {
    let now = Utc::now();
    MemoryRecord {
        id: MemoryId::new(),
        tenant_id: TenantId::SINGLE,
        kind: MemoryKind::UserPreference,
        visibility: MemoryVisibility::Tenant,
        content: content.to_owned(),
        metadata: MemoryMetadata {
            tags: Vec::new(),
            source: MemorySource::UserInput,
            confidence: 1.0,
            access_count: 0,
            last_accessed_at: None,
            recall_score: 1.0,
            ttl: None,
            redacted_segments: 0,
        },
        created_at: now,
        updated_at: now,
    }
}

fn session_ctx() -> MemorySessionCtx<'static> {
    MemorySessionCtx {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        workspace_id: None,
        user_id: None,
        team_id: None,
    }
}

fn session_summary() -> SessionSummaryView<'static> {
    SessionSummaryView {
        end_reason: EndReason::Completed,
        turn_count: 1,
        tool_use_count: 0,
        usage: UsageSnapshot::default(),
        final_assistant_text: Some("done"),
    }
}

#![cfg(feature = "provider-registry")]

use std::collections::BTreeSet;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{
    MemoryActorContext, MemoryError, MemoryId, MemoryKind, MemoryProviderDurability,
    MemoryProviderKind, MemoryProviderTrust, MemorySource, MemoryThreadMode, MemoryThreadSettings,
    MemoryVisibility, MemoryVisibilityClass, RunId, SessionId, TenantId,
};
use harness_memory::{
    FailMode, MemoryKindFilter, MemoryLifecycle, MemoryListScope, MemoryManager, MemoryMetadata,
    MemoryProviderDescriptor, MemoryQuery, MemoryRecord, MemoryStore, MemorySummary,
    MemoryVisibilityFilter, RecallPolicy,
};

#[cfg(feature = "threat-scanner")]
use harness_contracts::{Severity, ThreatAction, ThreatCategory};
#[cfg(feature = "threat-scanner")]
use harness_memory::{MemoryThreatScanner, ThreatPattern};

struct CountingProvider {
    id: &'static str,
    calls: AtomicUsize,
    upserts: AtomicUsize,
    readable: bool,
    writable: bool,
    priority: i32,
    timeout_ms: u32,
    max_chars_per_recall: u32,
    max_bytes_per_record: u64,
    delay: Duration,
    result: Result<Vec<MemoryRecord>, MemoryError>,
}

impl CountingProvider {
    fn ok(records: Vec<MemoryRecord>) -> Self {
        Self {
            id: "counting",
            calls: AtomicUsize::new(0),
            upserts: AtomicUsize::new(0),
            readable: true,
            writable: true,
            priority: 0,
            timeout_ms: 5000,
            max_chars_per_recall: 100_000,
            max_bytes_per_record: 1024 * 1024,
            delay: Duration::ZERO,
            result: Ok(records),
        }
    }

    fn ok_with_id(id: &'static str, records: Vec<MemoryRecord>) -> Self {
        Self {
            id,
            ..Self::ok(records)
        }
    }

    fn error(message: &str) -> Self {
        Self {
            id: "counting",
            calls: AtomicUsize::new(0),
            upserts: AtomicUsize::new(0),
            readable: true,
            writable: true,
            priority: 0,
            timeout_ms: 5000,
            max_chars_per_recall: 100_000,
            max_bytes_per_record: 1024 * 1024,
            delay: Duration::ZERO,
            result: Err(MemoryError::Message(message.to_owned())),
        }
    }

    fn delayed(delay: Duration, records: Vec<MemoryRecord>) -> Self {
        Self {
            id: "counting",
            calls: AtomicUsize::new(0),
            upserts: AtomicUsize::new(0),
            readable: true,
            writable: true,
            priority: 0,
            timeout_ms: 5000,
            max_chars_per_recall: 100_000,
            max_bytes_per_record: 1024 * 1024,
            delay,
            result: Ok(records),
        }
    }

    fn delayed_with_id(id: &'static str, delay: Duration, records: Vec<MemoryRecord>) -> Self {
        Self {
            id,
            ..Self::delayed(delay, records)
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn upserts(&self) -> usize {
        self.upserts.load(Ordering::SeqCst)
    }

    fn read_only(mut self) -> Self {
        self.writable = false;
        self
    }

    fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    fn timeout_ms(mut self, timeout_ms: u32) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    fn max_chars_per_recall(mut self, max_chars_per_recall: u32) -> Self {
        self.max_chars_per_recall = max_chars_per_recall;
        self
    }

    fn max_bytes_per_record(mut self, max_bytes_per_record: u64) -> Self {
        self.max_bytes_per_record = max_bytes_per_record;
        self
    }
}

#[async_trait]
impl MemoryStore for CountingProvider {
    fn provider_id(&self) -> &str {
        self.id
    }

    async fn recall(&self, _: MemoryQuery) -> Result<Vec<MemoryRecord>, MemoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        self.result.clone()
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        self.upserts.fetch_add(1, Ordering::SeqCst);
        Ok(record.id)
    }

    async fn forget(&self, _: MemoryId) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn list(&self, _: MemoryListScope) -> Result<Vec<MemorySummary>, MemoryError> {
        Ok(Vec::new())
    }
}

impl MemoryLifecycle for CountingProvider {}

impl harness_memory::MemoryProvider for CountingProvider {
    fn descriptor(&self) -> MemoryProviderDescriptor {
        MemoryProviderDescriptor {
            provider_id: self.id.to_owned(),
            provider_kind: MemoryProviderKind::Local,
            priority: self.priority,
            trust_level: MemoryProviderTrust::BuiltIn,
            tenant_scope: None,
            workspace_scope: None,
            durability: MemoryProviderDurability::Durable,
            readable: self.readable,
            writable: self.writable,
            allowed_visibility: vec![
                MemoryVisibilityClass::Private,
                MemoryVisibilityClass::User,
                MemoryVisibilityClass::Tenant,
            ],
            supports_evidence: true,
            supports_raw_content_export: false,
            timeout_ms: self.timeout_ms,
            max_records_per_recall: 50,
            max_chars_per_recall: self.max_chars_per_recall,
            max_bytes_per_record: self.max_bytes_per_record,
        }
    }
}

#[tokio::test]
async fn recall_outcome_without_readable_provider_is_degraded() {
    let manager = MemoryManager::new();

    let outcome = manager
        .recall_outcome(query(Duration::from_millis(200), 8))
        .await;

    assert!(matches!(
        outcome,
        harness_memory::MemoryRecallOutcome::Degraded(MemoryError::ExternalProviderNotConfigured)
    ));
}

#[tokio::test]
async fn fail_open_recall_without_readable_provider_returns_empty() {
    let manager = MemoryManager::new();

    assert!(manager
        .recall(query(Duration::from_millis(200), 8))
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn zero_deadline_bypasses_provider() {
    let manager = MemoryManager::new();
    let provider = Arc::new(CountingProvider::ok(vec![record("kept")]));
    manager.register_provider(provider.clone()).unwrap();

    let recalled = manager.recall(query(Duration::ZERO, 8)).await.unwrap();

    assert!(recalled.is_empty());
    assert_eq!(provider.calls(), 0);
}

#[tokio::test]
async fn default_fail_safe_skips_provider_errors_and_timeouts() {
    let error_manager = MemoryManager::new();
    let error_provider = Arc::new(CountingProvider::error("provider unavailable"));
    error_manager
        .register_provider(error_provider.clone())
        .unwrap();

    assert!(error_manager
        .recall(query(Duration::from_millis(200), 8))
        .await
        .unwrap()
        .is_empty());
    assert_eq!(error_provider.calls(), 1);

    let timeout_manager = MemoryManager::new();
    let timeout_provider = Arc::new(CountingProvider::delayed(
        Duration::from_millis(50),
        vec![record("late")],
    ));
    timeout_manager
        .register_provider(timeout_provider.clone())
        .unwrap();

    assert!(timeout_manager
        .recall(query(Duration::from_millis(1), 8))
        .await
        .unwrap()
        .is_empty());
    assert_eq!(timeout_provider.calls(), 1);
}

#[tokio::test]
async fn recall_fans_out_to_all_readable_providers() {
    let manager = MemoryManager::new();
    let left = Arc::new(CountingProvider::ok_with_id("left", vec![record("left")]));
    let right = Arc::new(CountingProvider::ok_with_id("right", vec![record("right")]));
    manager.register_provider(left.clone()).unwrap();
    manager.register_provider(right.clone()).unwrap();

    let recalled = manager
        .recall(query(Duration::from_millis(200), 8))
        .await
        .unwrap();

    assert_eq!(left.calls(), 1);
    assert_eq!(right.calls(), 1);
    let mut contents = recalled
        .iter()
        .map(|record| record.content.as_str())
        .collect::<Vec<_>>();
    contents.sort_unstable();
    assert_eq!(contents, vec!["left", "right"]);
}

#[tokio::test]
async fn upsert_uses_one_policy_selected_writable_provider() {
    let manager = MemoryManager::new();
    let writable_a = Arc::new(CountingProvider::ok_with_id("writable-a", Vec::new()).priority(10));
    let read_only = Arc::new(CountingProvider::ok_with_id("read-only", Vec::new()).read_only());
    let writable_b = Arc::new(CountingProvider::ok_with_id("writable-b", Vec::new()).priority(100));
    let record = record("write me");

    manager.register_provider(writable_a.clone()).unwrap();
    manager.register_provider(read_only.clone()).unwrap();
    manager.register_provider(writable_b.clone()).unwrap();

    manager.upsert(record.clone(), None).await.unwrap();

    assert_eq!(writable_a.upserts(), 0);
    assert_eq!(read_only.upserts(), 0);
    assert_eq!(writable_b.upserts(), 1);
}

#[tokio::test]
async fn surface_policy_returns_provider_errors() {
    let manager = MemoryManager::new().with_recall_policy(RecallPolicy {
        fail_open: FailMode::Surface,
        ..RecallPolicy::default()
    });
    manager
        .register_provider(Arc::new(CountingProvider::error("provider unavailable")))
        .unwrap();

    let error = manager
        .recall(query(Duration::from_millis(200), 8))
        .await
        .unwrap_err();

    assert!(
        matches!(error, MemoryError::Message(message) if message.contains("provider unavailable"))
    );
}

#[tokio::test]
async fn surface_policy_returns_typed_recall_deadline_error() {
    let manager = MemoryManager::new().with_recall_policy(RecallPolicy {
        fail_open: FailMode::Surface,
        ..RecallPolicy::default()
    });
    manager
        .register_provider(Arc::new(CountingProvider::delayed(
            Duration::from_millis(50),
            vec![record("late")],
        )))
        .unwrap();

    let error = manager
        .recall(query(Duration::from_millis(1), 8))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        MemoryError::RecallDeadlineExceeded { provider } if provider == "counting"
    ));
}

#[tokio::test]
async fn recall_uses_provider_descriptor_deadline_per_provider() {
    let manager = MemoryManager::new();
    let slow = Arc::new(
        CountingProvider::delayed(Duration::from_millis(50), vec![record("late")]).timeout_ms(1),
    );
    manager.register_provider(slow.clone()).unwrap();

    let recalled = manager
        .recall(query(Duration::from_millis(500), 8))
        .await
        .unwrap();

    assert!(recalled.is_empty());
    assert_eq!(slow.calls(), 1);
}

#[tokio::test]
async fn slow_provider_timeout_does_not_skip_next_provider() {
    let manager = MemoryManager::new();
    let slow = Arc::new(
        CountingProvider::delayed_with_id("slow", Duration::from_millis(50), vec![record("late")])
            .timeout_ms(1)
            .priority(100),
    );
    let fast = Arc::new(CountingProvider::ok_with_id("fast", vec![record("kept")]).priority(10));
    manager.register_provider(slow.clone()).unwrap();
    manager.register_provider(fast.clone()).unwrap();

    let recalled = manager
        .recall(query(Duration::from_millis(500), 8))
        .await
        .unwrap();

    assert_eq!(slow.calls(), 1);
    assert_eq!(fast.calls(), 1);
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].content, "kept");
}

#[tokio::test]
async fn provider_fanout_latency_does_not_accumulate_serially() {
    let manager = MemoryManager::new();
    let left = Arc::new(
        CountingProvider::delayed_with_id("left", Duration::from_millis(60), vec![record("left")])
            .priority(100),
    );
    let right = Arc::new(
        CountingProvider::delayed_with_id(
            "right",
            Duration::from_millis(60),
            vec![record("right")],
        )
        .priority(90),
    );
    manager.register_provider(left.clone()).unwrap();
    manager.register_provider(right.clone()).unwrap();

    let started = Instant::now();
    let recalled = manager
        .recall(query(Duration::from_millis(500), 8))
        .await
        .unwrap();

    assert!(
        started.elapsed() < Duration::from_millis(100),
        "provider recall should fan out concurrently"
    );
    assert_eq!(left.calls(), 1);
    assert_eq!(right.calls(), 1);
    assert_eq!(recalled.len(), 2);
}

#[tokio::test]
async fn recall_reranks_globally_and_dedupes_to_best_scored_record() {
    let manager = MemoryManager::new().with_recall_policy(RecallPolicy {
        max_records_per_turn: 1,
        ..RecallPolicy::default()
    });
    let duplicate = MemoryId::new();
    let high_priority = Arc::new(
        CountingProvider::ok_with_id(
            "high-priority",
            vec![record_with_id_and_score(duplicate, "same memory", 0.1)],
        )
        .priority(100),
    );
    let low_priority = Arc::new(
        CountingProvider::ok_with_id(
            "low-priority",
            vec![record_with_id_and_score(duplicate, "same memory", 0.9)],
        )
        .priority(10),
    );
    manager.register_provider(high_priority).unwrap();
    manager.register_provider(low_priority).unwrap();

    let recalled = manager
        .recall(query(Duration::from_millis(200), 10))
        .await
        .unwrap();

    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].metadata.recall_score, 0.9);
}

#[tokio::test]
async fn recall_dedupes_content_only_with_same_provider_source_and_evidence() {
    let manager = MemoryManager::new();
    manager
        .register_provider(Arc::new(CountingProvider::ok_with_id(
            "user-provider",
            vec![record_with_source("same memory", MemorySource::UserInput)],
        )))
        .unwrap();
    manager
        .register_provider(Arc::new(CountingProvider::ok_with_id(
            "plugin-provider",
            vec![record_with_source(
                "same memory",
                MemorySource::PluginOutput,
            )],
        )))
        .unwrap();

    let recalled = manager
        .recall(query(Duration::from_millis(200), 10))
        .await
        .unwrap();

    assert_eq!(recalled.len(), 2);
}

#[tokio::test]
async fn recall_dedupes_matching_content_source_and_evidence_across_providers() {
    let manager = MemoryManager::new();
    manager
        .register_provider(Arc::new(CountingProvider::ok_with_id(
            "low-provider",
            vec![record_with_id_and_score(
                MemoryId::new(),
                "same memory",
                0.1,
            )],
        )))
        .unwrap();
    manager
        .register_provider(Arc::new(CountingProvider::ok_with_id(
            "high-provider",
            vec![record_with_id_and_score(
                MemoryId::new(),
                "same memory",
                0.9,
            )],
        )))
        .unwrap();

    let recalled = manager
        .recall(query(Duration::from_millis(200), 10))
        .await
        .unwrap();

    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].metadata.recall_score, 0.9);
}

#[tokio::test]
async fn recall_enforces_provider_character_budget_before_global_merge() {
    let manager = MemoryManager::new().with_recall_policy(RecallPolicy {
        max_chars_per_turn: 100,
        ..RecallPolicy::default()
    });
    manager
        .register_provider(Arc::new(
            CountingProvider::ok(vec![record("abc"), record("def"), record("g")])
                .max_chars_per_recall(6),
        ))
        .unwrap();

    let recalled = manager
        .recall(query(Duration::from_millis(200), 10))
        .await
        .unwrap();

    assert_eq!(
        recalled
            .iter()
            .map(|record| record.content.as_str())
            .collect::<Vec<_>>(),
        vec!["abc", "def"]
    );
}

#[tokio::test]
async fn recall_enforces_provider_record_byte_budget_before_global_merge() {
    let manager = MemoryManager::new();
    manager
        .register_provider(Arc::new(
            CountingProvider::ok(vec![record("tiny"), record("too-large")]).max_bytes_per_record(4),
        ))
        .unwrap();

    let recalled = manager
        .recall(query(Duration::from_millis(200), 10))
        .await
        .unwrap();

    assert_eq!(
        recalled
            .iter()
            .map(|record| record.content.as_str())
            .collect::<Vec<_>>(),
        vec!["tiny"]
    );
}

#[tokio::test]
async fn recall_with_policy_sources_preserves_per_record_provider_ids() {
    let manager = MemoryManager::new();
    manager
        .register_provider(Arc::new(CountingProvider::ok_with_id(
            "left",
            vec![record("left")],
        )))
        .unwrap();
    manager
        .register_provider(Arc::new(CountingProvider::ok_with_id(
            "right",
            vec![record("right")],
        )))
        .unwrap();

    let sources = manager
        .recall_with_policy_sources(
            query(Duration::from_millis(200), 10),
            &thread_settings(SessionId::new()),
            &harness_contracts::MemoryActor::Model,
        )
        .await
        .unwrap();

    let mut provider_ids = sources
        .into_iter()
        .map(|source| (source.record.content, source.provider_id))
        .collect::<Vec<_>>();
    provider_ids.sort();
    assert_eq!(
        provider_ids,
        vec![
            ("left".to_owned(), "left".to_owned()),
            ("right".to_owned(), "right".to_owned())
        ]
    );
}

#[tokio::test]
async fn recall_trace_preserves_per_record_provider_ids() {
    let manager = MemoryManager::new();
    manager
        .register_provider(Arc::new(CountingProvider::ok_with_id(
            "left",
            vec![record("left")],
        )))
        .unwrap();
    manager
        .register_provider(Arc::new(CountingProvider::ok_with_id(
            "right",
            vec![record("right")],
        )))
        .unwrap();

    let result = manager
        .recall_outcome_with_trace(query(Duration::from_millis(200), 10), RunId::new(), 1)
        .await;
    let trace = manager
        .trace_collector()
        .get(TenantId::SINGLE, result.trace_id.expect("trace id"))
        .expect("recall trace");

    let mut candidate_providers = trace
        .candidates
        .iter()
        .map(|candidate| candidate.provider_id.as_str())
        .collect::<Vec<_>>();
    candidate_providers.sort_unstable();
    let mut injected_providers = trace
        .injected
        .iter()
        .map(|injected| injected.provider_id.as_str())
        .collect::<Vec<_>>();
    injected_providers.sort_unstable();

    assert_eq!(candidate_providers, vec!["left", "right"]);
    assert_eq!(injected_providers, vec!["left", "right"]);
}

#[tokio::test]
async fn recall_once_per_turn_deduplicates_provider_calls() {
    let manager = MemoryManager::new();
    let provider = Arc::new(CountingProvider::ok(vec![record("once")]));
    manager.register_provider(provider.clone()).unwrap();

    assert_eq!(
        manager
            .recall_once_per_turn(7, query(Duration::from_millis(200), 8))
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(manager
        .recall_once_per_turn(7, query(Duration::from_millis(200), 8))
        .await
        .unwrap()
        .is_empty());
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn recall_once_per_turn_merges_concurrent_calls_into_first_result() {
    let manager = MemoryManager::new();
    let provider = Arc::new(CountingProvider::delayed(
        Duration::from_millis(25),
        vec![record("merged")],
    ));
    manager.register_provider(provider.clone()).unwrap();

    let (left, right) = tokio::join!(
        manager.recall_once_per_turn(8, query(Duration::from_millis(200), 8)),
        manager.recall_once_per_turn(8, query(Duration::from_millis(200), 8)),
    );

    assert_eq!(left.unwrap()[0].content, "merged");
    assert_eq!(right.unwrap()[0].content, "merged");
    assert_eq!(provider.calls(), 1);
    assert!(manager
        .recall_once_per_turn(8, query(Duration::from_millis(200), 8))
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn recall_applies_record_and_character_budgets() {
    let manager = MemoryManager::new().with_recall_policy(RecallPolicy {
        max_records_per_turn: 3,
        max_chars_per_turn: 9,
        ..RecallPolicy::default()
    });
    manager
        .register_provider(Arc::new(CountingProvider::ok(vec![
            record("abcd"),
            record("efgh"),
            record("too-large"),
            record("ignored"),
        ])))
        .unwrap();

    let recalled = manager
        .recall(query(Duration::from_millis(200), 10))
        .await
        .unwrap();

    assert_eq!(
        recalled
            .iter()
            .map(|record| record.content.as_str())
            .collect::<Vec<_>>(),
        vec!["abcd", "efgh"]
    );
}

#[tokio::test]
async fn recall_does_not_update_access_metadata_through_provider_upsert() {
    let manager = MemoryManager::new();
    let provider = Arc::new(CountingProvider::ok(vec![record("read-only")]));
    manager.register_provider(provider.clone()).unwrap();

    let recalled = manager
        .recall(query(Duration::from_millis(200), 8))
        .await
        .unwrap();

    assert_eq!(recalled.len(), 1);
    assert_eq!(provider.calls(), 1);
    assert_eq!(provider.upserts(), 0);
}

#[cfg(feature = "threat-scanner")]
#[tokio::test]
async fn recall_scans_blocks_and_redacts_records() {
    let scanner = MemoryThreatScanner::from_patterns(vec![
        ThreatPattern::new(
            "block",
            "block-me",
            ThreatCategory::PromptInjection,
            Severity::Critical,
            ThreatAction::Block,
        )
        .unwrap(),
        ThreatPattern::new(
            "redact",
            "secret=[A-Z0-9]+",
            ThreatCategory::Credential,
            Severity::High,
            ThreatAction::Redact,
        )
        .unwrap(),
    ]);
    let manager = MemoryManager::new().with_threat_scanner(Arc::new(scanner));
    manager
        .register_provider(Arc::new(CountingProvider::ok(vec![
            record("safe"),
            record("block-me"),
            record("secret=ABCDEF123456"),
        ])))
        .unwrap();

    let recalled = manager
        .recall(query(Duration::from_millis(200), 8))
        .await
        .unwrap();

    assert_eq!(recalled.len(), 2);
    assert_eq!(recalled[0].content, "safe");
    assert_eq!(recalled[1].content, "[REDACTED:credential]");
    assert_eq!(recalled[1].metadata.redacted_segments, 1);
}

fn query(deadline: Duration, max_records: u32) -> MemoryQuery {
    let session_id = SessionId::new();
    MemoryQuery {
        text: "memory".to_owned(),
        kind_filter: Some(MemoryKindFilter::OnlyKinds(BTreeSet::from([
            MemoryKind::UserPreference,
        ]))),
        visibility_filter: MemoryVisibilityFilter::EffectiveFor(MemoryActorContext {
            tenant_id: TenantId::SINGLE,
            user_id: Some("user-1".to_owned()),
            team_id: None,
            session_id: Some(session_id),
        }),
        max_records,
        min_similarity: 0.0,
        tenant_id: TenantId::SINGLE,
        session_id: Some(session_id),
        deadline: Some(deadline),
    }
}

fn thread_settings(session_id: SessionId) -> MemoryThreadSettings {
    MemoryThreadSettings {
        session_id,
        use_memories: None,
        generate_memories: None,
        memory_mode: MemoryThreadMode::ReadWrite,
    }
}

fn record(content: &str) -> MemoryRecord {
    record_with_id_and_score(MemoryId::new(), content, 1.0)
}

fn record_with_source(content: &str, source: MemorySource) -> MemoryRecord {
    let mut record = record(content);
    record.metadata.source = source;
    record
}

fn record_with_id_and_score(id: MemoryId, content: &str, recall_score: f32) -> MemoryRecord {
    let now = Utc::now();
    MemoryRecord {
        id,
        tenant_id: TenantId::SINGLE,
        kind: MemoryKind::UserPreference,
        visibility: MemoryVisibility::Tenant,
        content: content.to_owned(),
        metadata: MemoryMetadata {
            tags: Vec::new(),
            source: MemorySource::UserInput,
            confidence: 1.0,
            evidence: None,
            access_count: 0,
            last_accessed_at: None,
            recall_score,
            ttl: None,
            redacted_segments: 0,
        },
        created_at: now,
        updated_at: now,
    }
}

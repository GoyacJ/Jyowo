#![cfg(feature = "provider-registry")]

use std::collections::BTreeSet;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{
    MemoryActorContext, MemoryError, MemoryId, MemoryKind, MemoryProviderTrust, MemorySource,
    MemoryVisibility, MemoryVisibilityClass, SessionId, TenantId,
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
            delay,
            result: Ok(records),
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
            priority: 0,
            trust_level: MemoryProviderTrust::BuiltIn,
            readable: self.readable,
            writable: self.writable,
            allowed_visibility: vec![
                MemoryVisibilityClass::Private,
                MemoryVisibilityClass::User,
                MemoryVisibilityClass::Tenant,
            ],
            timeout_ms: 5000,
            max_records_per_recall: 50,
            max_chars_per_recall: 100_000,
            max_bytes_per_record: 1024 * 1024,
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
async fn upsert_writes_to_every_writable_provider_and_skips_read_only() {
    let manager = MemoryManager::new();
    let writable_a = Arc::new(CountingProvider::ok_with_id("writable-a", Vec::new()));
    let read_only = Arc::new(CountingProvider::ok_with_id("read-only", Vec::new()).read_only());
    let writable_b = Arc::new(CountingProvider::ok_with_id("writable-b", Vec::new()));
    let record = record("write me");

    manager.register_provider(writable_a.clone()).unwrap();
    manager.register_provider(read_only.clone()).unwrap();
    manager.register_provider(writable_b.clone()).unwrap();

    manager.upsert(record.clone(), None).await.unwrap();

    assert_eq!(writable_a.upserts(), 1);
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

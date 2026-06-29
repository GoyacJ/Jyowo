#![cfg(feature = "external-slot")]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{
    Event, MemoryActor, MemoryId, MemoryKind, MemorySource, MemoryVisibility, SessionId, TenantId,
};
#[cfg(feature = "threat-scanner")]
use harness_contracts::{Severity, ThreatAction, ThreatCategory};
use harness_memory::{
    content_preview, InMemoryMemoryProvider, MemoryEventSink, MemoryKindFilter, MemoryLifecycle,
    MemoryListScope, MemoryManager, MemoryMetadata, MemoryQuery, MemoryRecord, MemoryStore,
    MemorySummary, MemoryVisibilityFilter,
};
#[cfg(feature = "threat-scanner")]
use harness_memory::{MemoryThreatScanner, ThreatPattern};
use parking_lot::Mutex;

#[tokio::test]
async fn memory_manager_emits_upsert_and_updates_access_metadata() {
    let session_id = SessionId::new();
    let sink = Arc::new(RecordingSink::default());
    let provider = Arc::new(InMemoryMemoryProvider::new("test"));
    let manager = MemoryManager::new().with_event_sink(sink.clone());
    manager.set_external(provider.clone()).unwrap();
    let record = memory_record(session_id, "prefers concise answers");

    let id = manager.upsert(record.clone(), None).await.unwrap();

    assert_eq!(id, record.id);
    assert!(sink.events.lock().iter().any(|event| {
        matches!(event, Event::MemoryUpserted(upserted)
            if upserted.memory_id == record.id
                && upserted.provider_id == "test"
                && upserted.content_hash.0 != [0; 32])
    }));

    let recalled = manager
        .recall(query(session_id, "concise"))
        .await
        .expect("recall should succeed");
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].metadata.access_count, 1);
    assert!(recalled[0].metadata.last_accessed_at.is_some());

    let stored = provider
        .list(MemoryListScope::ForActor(actor(session_id)))
        .await
        .unwrap();
    assert_eq!(stored[0].metadata.access_count, 1);
    assert!(stored[0].metadata.last_accessed_at.is_some());
}

#[tokio::test]
async fn memory_manager_browser_api_enforces_actor_visibility_and_edits_visible_items() {
    let session_id = SessionId::new();
    let other_session_id = SessionId::new();
    let provider = Arc::new(InMemoryMemoryProvider::new("test"));
    let manager = MemoryManager::new();
    manager.set_external(provider.clone()).unwrap();
    let visible = memory_record(session_id, "prefers concise answers");
    let hidden = memory_record(other_session_id, "hidden session note");
    manager.upsert(visible.clone(), None).await.unwrap();
    manager.upsert(hidden, None).await.unwrap();

    let listed = manager.list_for_actor(actor(session_id)).await.unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, visible.id);
    assert_eq!(listed[0].content_preview, "prefers concise answers");

    let detail = manager
        .get_for_actor(visible.id, actor(session_id))
        .await
        .unwrap();
    assert_eq!(detail.content, "prefers concise answers");

    let updated = manager
        .update_content_for_actor(visible.id, actor(session_id), "prefers terse answers", None)
        .await
        .unwrap();
    assert_eq!(updated.content, "prefers terse answers");

    let hidden_result = manager
        .get_for_actor(visible.id, actor(other_session_id))
        .await
        .unwrap_err();
    assert!(
        matches!(hidden_result, harness_contracts::MemoryError::NotFound(id) if id == visible.id)
    );
}

#[tokio::test]
async fn memory_manager_list_does_not_trust_provider_visibility_filtering() {
    let session_id = SessionId::new();
    let other_session_id = SessionId::new();
    let visible = memory_record(session_id, "visible session note");
    let hidden = memory_record(other_session_id, "hidden session note");
    let provider = Arc::new(LeakyListProvider::new(
        "leaky",
        vec![visible.clone(), hidden.clone()],
    ));
    let manager = MemoryManager::new();
    manager.set_external(provider).unwrap();

    let listed = manager.list_for_actor(actor(session_id)).await.unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, visible.id);
    assert_eq!(listed[0].content_preview, "visible session note");
}

#[tokio::test]
async fn memory_manager_delete_and_export_emit_audit_events_without_raw_content() {
    let session_id = SessionId::new();
    let sink = Arc::new(RecordingSink::default());
    let provider = Arc::new(InMemoryMemoryProvider::new("test"));
    let manager = MemoryManager::new().with_event_sink(sink.clone());
    manager.set_external(provider).unwrap();
    let record = memory_record(session_id, "secret export fact");
    manager.upsert(record.clone(), None).await.unwrap();

    let exported = manager.export_for_actor(actor(session_id)).await.unwrap();

    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].id, record.id);
    assert_eq!(exported[0].content, "secret export fact");
    assert!(sink.events.lock().iter().any(|event| {
        matches!(event, Event::MemoryExported(exported)
            if exported.provider_id == "test"
                && exported.item_count == 1
                && exported.content_hashes.len() == 1)
    }));

    manager
        .forget_for_actor(record.id, actor(session_id), None)
        .await
        .unwrap();

    assert!(sink.events.lock().iter().any(|event| {
        matches!(event, Event::MemoryUpserted(upserted)
            if upserted.memory_id == record.id
                && upserted.action == harness_contracts::MemoryWriteAction::Forget)
    }));
    let serialized = format!("{:?}", &*sink.events.lock());
    assert!(!serialized.contains("secret export fact"));
}

#[tokio::test]
async fn memory_manager_delete_and_export_fail_when_required_audit_fails() {
    let session_id = SessionId::new();
    let provider = Arc::new(InMemoryMemoryProvider::new("test"));
    let manager = MemoryManager::new().with_event_sink(Arc::new(FailingRequiredSink));
    manager.set_external(provider.clone()).unwrap();
    let record = memory_record(session_id, "audited delete fact");
    manager.upsert(record.clone(), None).await.unwrap();

    let export_error = manager
        .export_for_actor(actor(session_id))
        .await
        .unwrap_err();
    assert!(matches!(
        export_error,
        harness_contracts::MemoryError::Provider { provider, .. } if provider == "audit"
    ));

    let delete_error = manager
        .forget_for_actor(record.id, actor(session_id), None)
        .await
        .unwrap_err();
    assert!(matches!(
        delete_error,
        harness_contracts::MemoryError::Provider { provider, .. } if provider == "audit"
    ));
    assert!(provider.get(record.id).await.is_ok());
}

#[tokio::test]
async fn memory_manager_does_not_emit_delete_audit_when_provider_forget_fails() {
    let session_id = SessionId::new();
    let sink = Arc::new(RecordingSink::default());
    let record = memory_record(session_id, "delete failure fact");
    let provider = Arc::new(ForgetFailingProvider::new("forget-fails", record.clone()));
    let manager = MemoryManager::new().with_event_sink(sink.clone());
    manager.set_external(provider.clone()).unwrap();

    let error = manager
        .forget_for_actor(record.id, actor(session_id), None)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        harness_contracts::MemoryError::Provider { provider, .. } if provider == "forget-fails"
    ));
    assert!(provider.get(record.id).await.is_ok());
    assert!(!sink.events.lock().iter().any(|event| {
        matches!(event, Event::MemoryUpserted(upserted)
            if upserted.memory_id == record.id
                && upserted.action == harness_contracts::MemoryWriteAction::Forget)
    }));
}

#[cfg(feature = "threat-scanner")]
#[tokio::test]
async fn memory_manager_browser_read_paths_scan_and_redact_visible_content() {
    let session_id = SessionId::new();
    let scanner = MemoryThreatScanner::from_patterns(vec![ThreatPattern::new(
        "credential",
        "api_key_[A-Za-z0-9]+",
        ThreatCategory::Credential,
        Severity::High,
        ThreatAction::Redact,
    )
    .unwrap()]);
    let provider = Arc::new(InMemoryMemoryProvider::new("test"));
    let manager = MemoryManager::new()
        .with_event_sink(Arc::new(RecordingSink::default()))
        .with_threat_scanner(Arc::new(scanner));
    manager.set_external(provider.clone()).unwrap();
    let record = memory_record(session_id, "token api_key_12345");
    provider.upsert(record.clone()).await.unwrap();

    let listed = manager.list_for_actor(actor(session_id)).await.unwrap();
    assert_eq!(listed[0].content_preview, "token [REDACTED:credential]");

    let detail = manager
        .get_for_actor(record.id, actor(session_id))
        .await
        .unwrap();
    assert_eq!(detail.content, "token [REDACTED:credential]");
    assert_eq!(detail.metadata.redacted_segments, 1);

    let exported = manager.export_for_actor(actor(session_id)).await.unwrap();
    assert_eq!(exported[0].content, "token [REDACTED:credential]");
    assert_eq!(exported[0].metadata.redacted_segments, 1);
}

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<Event>>,
}

#[async_trait]
impl MemoryEventSink for RecordingSink {
    async fn emit(&self, event: Event) {
        self.events.lock().push(event);
    }
}

struct FailingRequiredSink;

#[async_trait]
impl MemoryEventSink for FailingRequiredSink {
    async fn emit(&self, _event: Event) {}

    async fn emit_required(&self, _event: Event) -> Result<(), harness_contracts::MemoryError> {
        Err(harness_contracts::MemoryError::Provider {
            provider: "audit".to_owned(),
            source_message: "append failed".to_owned(),
        })
    }
}

struct LeakyListProvider {
    provider_id: String,
    records: Vec<MemoryRecord>,
}

impl LeakyListProvider {
    fn new(provider_id: impl Into<String>, records: Vec<MemoryRecord>) -> Self {
        Self {
            provider_id: provider_id.into(),
            records,
        }
    }
}

#[async_trait]
impl MemoryStore for LeakyListProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    async fn recall(
        &self,
        _query: MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, harness_contracts::MemoryError> {
        Ok(Vec::new())
    }

    async fn get(&self, id: MemoryId) -> Result<MemoryRecord, harness_contracts::MemoryError> {
        self.records
            .iter()
            .find(|record| record.id == id)
            .cloned()
            .ok_or(harness_contracts::MemoryError::NotFound(id))
    }

    async fn upsert(
        &self,
        record: MemoryRecord,
    ) -> Result<MemoryId, harness_contracts::MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _id: MemoryId) -> Result<(), harness_contracts::MemoryError> {
        Ok(())
    }

    async fn list(
        &self,
        _scope: MemoryListScope,
    ) -> Result<Vec<MemorySummary>, harness_contracts::MemoryError> {
        Ok(self
            .records
            .iter()
            .map(|record| MemorySummary {
                id: record.id,
                kind: record.kind.clone(),
                visibility: record.visibility.clone(),
                content_preview: content_preview(&record.content),
                metadata: record.metadata.clone(),
                updated_at: record.updated_at,
            })
            .collect())
    }
}

impl MemoryLifecycle for LeakyListProvider {}

struct ForgetFailingProvider {
    provider_id: String,
    record: MemoryRecord,
}

impl ForgetFailingProvider {
    fn new(provider_id: impl Into<String>, record: MemoryRecord) -> Self {
        Self {
            provider_id: provider_id.into(),
            record,
        }
    }
}

#[async_trait]
impl MemoryStore for ForgetFailingProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    async fn recall(
        &self,
        _query: MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, harness_contracts::MemoryError> {
        Ok(Vec::new())
    }

    async fn get(&self, id: MemoryId) -> Result<MemoryRecord, harness_contracts::MemoryError> {
        if self.record.id == id {
            return Ok(self.record.clone());
        }

        Err(harness_contracts::MemoryError::NotFound(id))
    }

    async fn upsert(
        &self,
        record: MemoryRecord,
    ) -> Result<MemoryId, harness_contracts::MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _id: MemoryId) -> Result<(), harness_contracts::MemoryError> {
        Err(harness_contracts::MemoryError::Provider {
            provider: self.provider_id.clone(),
            source_message: "forget failed".to_owned(),
        })
    }

    async fn list(
        &self,
        _scope: MemoryListScope,
    ) -> Result<Vec<MemorySummary>, harness_contracts::MemoryError> {
        Ok(vec![MemorySummary {
            id: self.record.id,
            kind: self.record.kind.clone(),
            visibility: self.record.visibility.clone(),
            content_preview: content_preview(&self.record.content),
            metadata: self.record.metadata.clone(),
            updated_at: self.record.updated_at,
        }])
    }
}

impl MemoryLifecycle for ForgetFailingProvider {}

fn query(session_id: SessionId, text: &str) -> MemoryQuery {
    MemoryQuery {
        text: text.to_owned(),
        kind_filter: Some(MemoryKindFilter::OnlyKinds(BTreeSet::from([
            MemoryKind::UserPreference,
        ]))),
        visibility_filter: MemoryVisibilityFilter::EffectiveFor(actor(session_id)),
        max_records: 3,
        min_similarity: 0.75,
        tenant_id: TenantId::SINGLE,
        session_id: Some(session_id),
        deadline: Some(Duration::from_secs(1)),
    }
}

fn actor(session_id: SessionId) -> MemoryActor {
    MemoryActor {
        tenant_id: TenantId::SINGLE,
        user_id: Some("user-1".to_owned()),
        team_id: None,
        session_id: Some(session_id),
    }
}

fn memory_record(session_id: SessionId, content: &str) -> MemoryRecord {
    MemoryRecord {
        id: MemoryId::new(),
        tenant_id: TenantId::SINGLE,
        kind: MemoryKind::UserPreference,
        visibility: MemoryVisibility::Private { session_id },
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
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

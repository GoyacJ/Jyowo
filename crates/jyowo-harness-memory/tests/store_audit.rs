#![cfg(feature = "testing")]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{
    ContentHash, Event, MemoryActor, MemoryActorContext, MemoryEvidence, MemoryEvidenceOrigin,
    MemoryId, MemoryKind, MemoryPermissionContext, MemorySource, MemoryThreadMode,
    MemoryThreadSettings, MemoryVisibility, SessionId, TenantId,
};
#[cfg(feature = "threat-scanner")]
use harness_contracts::{Severity, ThreatAction, ThreatCategory};
use harness_memory::{
    content_preview, InMemoryMemoryProvider, LocalMemoryProvider, MemoryEventSink,
    MemoryKindFilter, MemoryLifecycle, MemoryListScope, MemoryManager, MemoryMetadata,
    MemoryOperationPolicy, MemoryQuery, MemoryRecord, MemoryStore, MemorySummary,
    MemoryVisibilityFilter,
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
    manager.register_provider(provider.clone()).unwrap();
    let record = memory_record(session_id, "prefers concise answers");

    let id = manager
        .upsert_with_policy(
            record.clone(),
            None,
            &explicit_user_policy(session_id, &record),
        )
        .await
        .unwrap();

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
    let manager = MemoryManager::new().with_event_sink(Arc::new(RecordingSink::default()));
    manager.register_provider(provider.clone()).unwrap();
    let visible = memory_record(session_id, "prefers concise answers");
    let hidden = memory_record(other_session_id, "hidden session note");
    provider.upsert(visible.clone()).await.unwrap();
    provider.upsert(hidden).await.unwrap();

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
        .update_content_for_actor_with_policy(
            visible.id,
            actor(session_id),
            "prefers terse answers",
            None,
            &explicit_user_policy(session_id, &visible),
        )
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
async fn memory_manager_updates_existing_record_in_owning_provider() {
    let session_id = SessionId::new();
    let sink = Arc::new(RecordingSink::default());
    let first_provider = Arc::new(InMemoryMemoryProvider::new("aaa"));
    let owning_provider = Arc::new(InMemoryMemoryProvider::new("zzz"));
    let manager = MemoryManager::new().with_event_sink(sink);
    manager.register_provider(first_provider.clone()).unwrap();
    manager.register_provider(owning_provider.clone()).unwrap();
    let record = memory_record(session_id, "prefers concise answers");
    owning_provider.upsert(record.clone()).await.unwrap();
    let policy = explicit_user_policy(session_id, &record);

    let updated = manager
        .update_content_for_actor_with_policy(
            record.id,
            actor(session_id),
            "prefers terse answers",
            None,
            &policy,
        )
        .await
        .unwrap();

    assert_eq!(updated.content, "prefers terse answers");
    assert!(first_provider.get(record.id).await.is_err());
    assert_eq!(
        owning_provider.get(record.id).await.unwrap().content,
        "prefers terse answers"
    );

    manager
        .forget_for_actor_with_policy(record.id, actor(session_id), None, &policy)
        .await
        .unwrap();

    assert!(manager
        .list_for_actor(actor(session_id))
        .await
        .unwrap()
        .is_empty());
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
    manager.register_provider(provider).unwrap();

    let listed = manager.list_for_actor(actor(session_id)).await.unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, visible.id);
    assert_eq!(listed[0].content_preview, "visible session note");
}

#[tokio::test]
async fn invisible_duplicate_id_does_not_hide_later_visible_record() {
    let session_id = SessionId::new();
    let other_session_id = SessionId::new();
    let visible = memory_record(session_id, "visible duplicate note");
    let mut hidden = memory_record(other_session_id, "hidden duplicate note");
    hidden.id = visible.id;
    let first_provider = Arc::new(LeakyListProvider::new("aaa", vec![hidden]));
    let second_provider = Arc::new(LeakyListProvider::new("zzz", vec![visible.clone()]));
    let manager = MemoryManager::new().with_event_sink(Arc::new(RecordingSink::default()));
    manager.register_provider(first_provider).unwrap();
    manager.register_provider(second_provider).unwrap();

    let listed = manager.list_for_actor(actor(session_id)).await.unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, visible.id);
    assert_eq!(listed[0].content_preview, "visible duplicate note");

    let exported = manager
        .prepare_export_for_actor(actor(session_id), "session", "json", false)
        .await
        .unwrap()
        .summaries;
    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].id, visible.id);
}

#[tokio::test]
async fn memory_manager_delete_and_export_emit_audit_events_without_raw_content() {
    let session_id = SessionId::new();
    let sink = Arc::new(RecordingSink::default());
    let provider = Arc::new(InMemoryMemoryProvider::new("test"));
    let manager = MemoryManager::new().with_event_sink(sink.clone());
    manager.register_provider(provider).unwrap();
    let record = memory_record(session_id, "secret export fact");
    manager
        .upsert_with_policy(
            record.clone(),
            None,
            &explicit_user_policy(session_id, &record),
        )
        .await
        .unwrap();

    let export = manager
        .prepare_export_for_actor(actor(session_id), "session", "json", false)
        .await
        .unwrap();
    let exported = export.summaries.clone();

    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].id, record.id);
    assert_eq!(exported[0].content_preview, "[redacted memory content]");
    assert!(!sink
        .events
        .lock()
        .iter()
        .any(|event| matches!(event, Event::MemoryExported(_))));

    manager.emit_export_audit(export.event).await.unwrap();

    assert!(sink.events.lock().iter().any(|event| {
        matches!(event, Event::MemoryExported(exported)
            if exported.provider_id == "test"
                && exported.item_count == 1
                && exported.content_hashes.len() == 1)
    }));

    manager
        .forget_for_actor_with_policy(
            record.id,
            actor(session_id),
            None,
            &explicit_user_policy(session_id, &record),
        )
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
    manager.register_provider(provider.clone()).unwrap();
    let record = memory_record(session_id, "audited delete fact");
    provider.upsert(record.clone()).await.unwrap();

    let export = manager
        .prepare_export_for_actor(actor(session_id), "session", "json", false)
        .await
        .unwrap();
    let export_error = manager.emit_export_audit(export.event).await.unwrap_err();
    assert!(matches!(
        export_error,
        harness_contracts::MemoryError::Provider { provider, .. } if provider == "audit"
    ));

    let delete_error = manager
        .forget_for_actor_with_policy(
            record.id,
            actor(session_id),
            None,
            &explicit_user_policy(session_id, &record),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        delete_error,
        harness_contracts::MemoryError::Provider { provider, .. } if provider == "audit"
    ));
    assert!(provider.get(record.id).await.is_ok());
}

#[tokio::test]
async fn policy_write_paths_fail_closed_when_required_audit_fails() {
    let session_id = SessionId::new();
    let provider = Arc::new(InMemoryMemoryProvider::new("test"));
    let manager = MemoryManager::new().with_event_sink(Arc::new(FailingRequiredSink));
    manager.register_provider(provider.clone()).unwrap();
    let record = memory_record(session_id, "audited create fact");
    let policy = explicit_user_policy(session_id, &record);

    let create_error = manager
        .upsert_with_policy(record.clone(), None, &policy)
        .await
        .unwrap_err();
    assert!(matches!(
        create_error,
        harness_contracts::MemoryError::Provider { provider, .. } if provider == "audit"
    ));
    assert!(provider.get(record.id).await.is_err());

    provider.upsert(record.clone()).await.unwrap();
    let update_error = manager
        .update_content_for_actor_with_policy(
            record.id,
            actor(session_id),
            "updated without audit",
            None,
            &policy,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        update_error,
        harness_contracts::MemoryError::Provider { provider, .. } if provider == "audit"
    ));
    let stored = provider.get(record.id).await.unwrap();
    assert_eq!(stored.content, "audited create fact");
}

#[tokio::test]
async fn local_provider_required_audit_create_failure_leaves_no_tombstone_barrier() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.sqlite3");
    let provider = Arc::new(
        LocalMemoryProvider::open(&db_path.to_string_lossy(), TenantId::SINGLE)
            .expect("local provider"),
    );
    let manager = MemoryManager::new().with_event_sink(Arc::new(FailingRequiredSink));
    manager.register_provider(provider.clone()).unwrap();
    let session_id = SessionId::new();
    let record = memory_record(session_id, "local audited create fact");
    let policy = explicit_user_policy(session_id, &record);

    let error = manager
        .upsert_with_policy(record.clone(), None, &policy)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        harness_contracts::MemoryError::Provider { provider, .. } if provider == "audit"
    ));
    assert!(provider.get(record.id).await.is_err());
    provider
        .upsert(record.clone())
        .await
        .expect("audit rollback must not tombstone failed create");
}

#[tokio::test]
async fn local_provider_required_audit_delete_failure_restores_record_without_tombstone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.sqlite3");
    let provider = Arc::new(
        LocalMemoryProvider::open(&db_path.to_string_lossy(), TenantId::SINGLE)
            .expect("local provider"),
    );
    let manager = MemoryManager::new().with_event_sink(Arc::new(FailingRequiredSink));
    manager.register_provider(provider.clone()).unwrap();
    let session_id = SessionId::new();
    let record = memory_record(session_id, "local audited delete fact");
    let policy = explicit_user_policy(session_id, &record);
    provider.upsert(record.clone()).await.expect("seed record");

    let error = manager
        .forget_for_actor_with_policy(record.id, actor(session_id), None, &policy)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        harness_contracts::MemoryError::Provider { provider, .. } if provider == "audit"
    ));
    assert_eq!(
        provider.get(record.id).await.unwrap().content,
        record.content
    );
    let regenerated = memory_record(session_id, "local audited delete fact");
    provider
        .upsert(regenerated)
        .await
        .expect("audit rollback must remove failed delete tombstone");
}

#[tokio::test]
async fn memory_manager_does_not_emit_delete_audit_when_provider_forget_fails() {
    let session_id = SessionId::new();
    let sink = Arc::new(RecordingSink::default());
    let record = memory_record(session_id, "delete failure fact");
    let provider = Arc::new(ForgetFailingProvider::new("forget-fails", record.clone()));
    let manager = MemoryManager::new().with_event_sink(sink.clone());
    manager.register_provider(provider.clone()).unwrap();

    let error = manager
        .forget_for_actor_with_policy(
            record.id,
            actor(session_id),
            None,
            &explicit_user_policy(session_id, &record),
        )
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

#[tokio::test]
async fn memory_manager_delete_does_not_fan_out_to_other_providers_with_same_id() {
    let session_id = SessionId::new();
    let sink = Arc::new(RecordingSink::default());
    let manager = MemoryManager::new().with_event_sink(sink);
    let first_provider = Arc::new(InMemoryMemoryProvider::new("first"));
    let second_provider = Arc::new(InMemoryMemoryProvider::new("second"));
    manager.register_provider(first_provider.clone()).unwrap();
    manager.register_provider(second_provider.clone()).unwrap();

    let first_record = memory_record(session_id, "delete only this provider");
    let second_record = MemoryRecord {
        content: "same id in other provider must remain".to_owned(),
        ..first_record.clone()
    };
    first_provider.upsert(first_record.clone()).await.unwrap();
    second_provider.upsert(second_record.clone()).await.unwrap();

    manager
        .forget_for_actor_with_policy(
            first_record.id,
            actor(session_id),
            None,
            &explicit_user_policy(session_id, &first_record),
        )
        .await
        .unwrap();

    assert!(first_provider.get(first_record.id).await.is_err());
    let preserved = second_provider
        .get(second_record.id)
        .await
        .expect("delete must not fan out to unrelated provider record");
    assert_eq!(preserved.content, "same id in other provider must remain");
}

#[tokio::test]
async fn memory_manager_delete_stops_when_provider_lookup_errors() {
    let session_id = SessionId::new();
    let sink = Arc::new(RecordingSink::default());
    let manager = MemoryManager::new().with_event_sink(sink);
    let record = memory_record(session_id, "must remain when lookup fails");
    let failing_provider = Arc::new(GetFailingProvider::new("aaa-get-fails"));
    let second_provider = Arc::new(InMemoryMemoryProvider::new("second"));
    second_provider.upsert(record.clone()).await.unwrap();
    manager.register_provider(failing_provider).unwrap();
    manager.register_provider(second_provider.clone()).unwrap();

    let error = manager
        .forget_for_actor_with_policy(
            record.id,
            actor(session_id),
            None,
            &explicit_user_policy(session_id, &record),
        )
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        harness_contracts::MemoryError::Provider { provider, .. } if provider == "aaa-get-fails"
    ));
    assert!(second_provider.get(record.id).await.is_ok());
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
    manager.register_provider(provider.clone()).unwrap();
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

    let exported = manager
        .prepare_export_for_actor(actor(session_id), "session", "json", false)
        .await
        .unwrap()
        .summaries;
    assert_eq!(exported[0].content_preview, "[redacted memory content]");
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

    async fn emit_required(&self, event: Event) -> Result<(), harness_contracts::MemoryError> {
        self.emit(event).await;
        Ok(())
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
            .map(|record| memory_summary(record, Some(self.provider_id.clone())))
            .collect())
    }
}

impl MemoryLifecycle for LeakyListProvider {}

impl harness_memory::MemoryProvider for LeakyListProvider {}

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
        Ok(vec![memory_summary(
            &self.record,
            Some(self.provider_id.clone()),
        )])
    }
}

impl MemoryLifecycle for ForgetFailingProvider {}

impl harness_memory::MemoryProvider for ForgetFailingProvider {}

struct GetFailingProvider {
    provider_id: String,
}

impl GetFailingProvider {
    fn new(provider_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
        }
    }
}

#[async_trait]
impl MemoryStore for GetFailingProvider {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    async fn recall(
        &self,
        _query: MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, harness_contracts::MemoryError> {
        Ok(Vec::new())
    }

    async fn get(&self, _id: MemoryId) -> Result<MemoryRecord, harness_contracts::MemoryError> {
        Err(harness_contracts::MemoryError::Provider {
            provider: self.provider_id.clone(),
            source_message: "lookup failed".to_owned(),
        })
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
        Ok(Vec::new())
    }
}

impl MemoryLifecycle for GetFailingProvider {}

impl harness_memory::MemoryProvider for GetFailingProvider {}

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

fn actor(session_id: SessionId) -> MemoryActorContext {
    MemoryActorContext {
        tenant_id: TenantId::SINGLE,
        user_id: Some("user-1".to_owned()),
        team_id: None,
        session_id: Some(session_id),
    }
}

fn memory_summary(record: &MemoryRecord, provider_id: Option<String>) -> MemorySummary {
    MemorySummary {
        id: record.id,
        provider_id,
        kind: record.kind.clone(),
        visibility: record.visibility.clone(),
        content_preview: content_preview(&record.content),
        content_hash: ContentHash(*blake3::hash(record.content.as_bytes()).as_bytes()),
        metadata: record.metadata.clone(),
        expires_at: record
            .metadata
            .ttl
            .and_then(|ttl| chrono::Duration::from_std(ttl).ok())
            .map(|ttl| record.created_at + ttl),
        deleted: false,
        updated_at: record.updated_at,
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
            evidence: None,
            confidence: 1.0,
            access_count: 0,
            last_accessed_at: None,
            recall_score: 1.0,
            recall_score_breakdown: None,
            ttl: None,
            redacted_segments: 0,
        },
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn explicit_user_policy(session_id: SessionId, record: &MemoryRecord) -> MemoryOperationPolicy {
    MemoryOperationPolicy {
        thread: MemoryThreadSettings {
            session_id,
            use_memories: None,
            generate_memories: None,
            memory_mode: MemoryThreadMode::ReadWrite,
        },
        actor: MemoryActor::User {
            user_label: Some("user-1".to_owned()),
        },
        permission: MemoryPermissionContext {
            explicit_user_instruction: true,
            include_raw_content: false,
            action_plan_id: Some(harness_contracts::ActionPlanId::new()),
            authorization_ticket_id: Some(harness_contracts::AuthorizationTicketId::new()),
            non_interactive_policy_grant: false,
        },
        evidence: MemoryEvidence {
            source: record.metadata.source.clone(),
            origin: MemoryEvidenceOrigin::UserMessage {
                session_id,
                run_id: harness_contracts::RunId::new(),
                message_id: harness_contracts::MessageId::new(),
            },
            content_hash: ContentHash(blake3::hash(record.content.as_bytes()).into()),
            session_id: Some(session_id),
            run_id: None,
            message_id: None,
            tool_use_id: None,
        },
    }
}

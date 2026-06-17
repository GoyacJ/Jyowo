#![cfg(feature = "external-slot")]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{
    Event, MemoryActor, MemoryId, MemoryKind, MemorySource, MemoryVisibility, SessionId, TenantId,
};
use harness_memory::{
    MemoryEventSink, MemoryKindFilter, MemoryListScope, MemoryManager, MemoryMetadata, MemoryQuery,
    MemoryRecord, MemoryStore, MemoryVisibilityFilter, MockMemoryProvider,
};
use parking_lot::Mutex;

#[tokio::test]
async fn memory_manager_emits_upsert_and_updates_access_metadata() {
    let session_id = SessionId::new();
    let sink = Arc::new(RecordingSink::default());
    let provider = Arc::new(MockMemoryProvider::new("mock"));
    let manager = MemoryManager::new().with_event_sink(sink.clone());
    manager.set_external(provider.clone()).unwrap();
    let record = memory_record(session_id, "prefers concise answers");

    let id = manager.upsert(record.clone(), None).await.unwrap();

    assert_eq!(id, record.id);
    assert!(sink.events.lock().iter().any(|event| {
        matches!(event, Event::MemoryUpserted(upserted)
            if upserted.memory_id == record.id
                && upserted.provider_id == "mock"
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

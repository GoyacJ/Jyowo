use std::sync::Arc;

use futures::StreamExt;
use harness_contracts::{
    CacheImpact, EndReason, Event, ForkReason, MessageContent, MessageId, MessageMetadata,
    NoopRedactor, RunId, SessionForkedEvent, SessionId, TenantId,
};
use harness_journal::{EventStore, InMemoryEventStore, ReplayCursor};
use harness_session::{Session, SessionOptions, SessionProjection};

#[tokio::test]
async fn fork_writes_lineage_and_inherits_parent_history() {
    let root = tempfile::tempdir().unwrap();
    let tenant = TenantId::SINGLE;
    let parent_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let parent = Session::builder()
        .with_options(
            SessionOptions::new(root.path())
                .with_tenant_id(tenant)
                .with_session_id(parent_id),
        )
        .with_event_store(store.clone())
        .build()
        .await
        .unwrap();
    store
        .append(
            tenant,
            parent_id,
            &[Event::UserMessageAppended(
                harness_contracts::UserMessageAppendedEvent {
                    run_id: RunId::new(),
                    message_id: MessageId::new(),
                    content: MessageContent::Text("parent history".to_owned()),
                    metadata: MessageMetadata::default(),
                    at: harness_contracts::now(),
                },
            )],
        )
        .await
        .unwrap();

    let child = parent.fork(ForkReason::UserRequested).await.unwrap();

    let child_projection = child.projection().await;
    assert_ne!(child_projection.session_id, parent_id);
    assert_eq!(child.projection().await.messages.len(), 1);
    assert!(store
        .snapshot(tenant, child_projection.session_id)
        .await
        .unwrap()
        .is_some());

    let parent_events = store
        .read_envelopes(tenant, parent_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    assert!(parent_events
        .iter()
        .any(|envelope| matches!(envelope.payload, Event::SessionForked(_))));
}

#[tokio::test]
async fn snapshot_id_is_stable_for_same_projection_state() {
    let tenant = TenantId::SINGLE;
    let session = SessionId::new();
    let fork = Event::SessionForked(SessionForkedEvent {
        parent_session_id: session,
        child_session_id: SessionId::new(),
        tenant_id: tenant,
        fork_reason: ForkReason::Isolation,
        from_offset: harness_contracts::JournalOffset(3),
        config_delta_hash: None,
        cache_impact: CacheImpact {
            prompt_cache_invalidated: false,
            reason: None,
        },
        at: harness_contracts::now(),
    });
    let envelopes = [
        harness_journal::EventEnvelope {
            offset: harness_contracts::JournalOffset(0),
            event_id: harness_contracts::EventId::new(),
            session_id: session,
            tenant_id: tenant,
            run_id: None,
            correlation_id: harness_contracts::CorrelationId::new(),
            causation_id: None,
            schema_version: harness_journal::SchemaVersion::CURRENT,
            recorded_at: harness_contracts::now(),
            payload: fork.clone(),
        },
        harness_journal::EventEnvelope {
            offset: harness_contracts::JournalOffset(0),
            event_id: harness_contracts::EventId::new(),
            session_id: session,
            tenant_id: tenant,
            run_id: None,
            correlation_id: harness_contracts::CorrelationId::new(),
            causation_id: None,
            schema_version: harness_journal::SchemaVersion::CURRENT,
            recorded_at: harness_contracts::now(),
            payload: fork,
        },
    ];

    let first = SessionProjection::replay(envelopes[..1].to_vec()).unwrap();
    let second = SessionProjection::replay(envelopes[1..].to_vec()).unwrap();

    assert_eq!(first.snapshot_id, second.snapshot_id);
}

#[tokio::test]
async fn forked_session_starts_without_end_reason() {
    let root = tempfile::tempdir().unwrap();
    let tenant = TenantId::SINGLE;
    let parent = Session::builder()
        .with_options(SessionOptions::new(root.path()).with_tenant_id(tenant))
        .with_event_store(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .build()
        .await
        .unwrap();
    parent.end(EndReason::Interrupted).await.unwrap();

    let child = parent
        .fork(ForkReason::RetryFromCheckpoint(parent.last_offset().await))
        .await
        .unwrap();

    assert_eq!(child.projection().await.end_reason, None);
}

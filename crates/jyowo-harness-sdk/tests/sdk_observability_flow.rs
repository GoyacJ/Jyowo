#![cfg(all(feature = "observability-redactor", feature = "testing"))]

use std::sync::Arc;

use futures::StreamExt;
use harness_contracts::{Event, TenantId, UnexpectedErrorEvent};
use harness_journal::{EventStore, InMemoryEventStore, ReplayCursor};
use jyowo_harness_sdk::{testing, Harness, SessionId};

#[tokio::test]
async fn sdk_observability_flow_redacts_stream_and_replays_stored_events_deterministically() {
    let store = Arc::new(InMemoryEventStore::new(Arc::new(testing::NoopRedactor)));
    let session_id = SessionId::new();
    let raw_secret = "failed with sk-abcdefghijklmnopqrstuvwxyz";
    store
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::UnexpectedError(UnexpectedErrorEvent {
                session_id: Some(session_id),
                run_id: None,
                error: raw_secret.to_owned(),
                at: harness_contracts::now(),
            })],
        )
        .await
        .expect("raw event should be stored");

    let harness = Harness::builder()
        .with_model(testing::TestModelProvider::default())
        .with_store_arc(store.clone())
        .with_sandbox(testing::NoopSandbox::new())
        .build()
        .await
        .expect("harness should build");

    let streamed = harness
        .event_stream(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("event stream should open")
        .collect::<Vec<_>>()
        .await;
    let Event::UnexpectedError(redacted) = &streamed[0] else {
        panic!("unexpected event type");
    };
    assert!(!redacted.error.contains("sk-abcdefghijklmnopqrstuvwxyz"));
    assert!(redacted.error.contains("[REDACTED]"));

    let first_replay = stored_events(&store, session_id).await;
    let second_replay = stored_events(&store, session_id).await;
    assert_eq!(first_replay, second_replay);
    let Event::UnexpectedError(stored) = &first_replay[0] else {
        panic!("unexpected stored event type");
    };
    assert_eq!(stored.error, raw_secret);
}

async fn stored_events(store: &Arc<InMemoryEventStore>, session_id: SessionId) -> Vec<Event> {
    store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("stored events should be readable")
        .collect()
        .await
}

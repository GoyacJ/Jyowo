#![cfg(all(feature = "observability-redactor", feature = "testing"))]

use std::sync::Arc;

use futures::StreamExt;
use harness_contracts::{Event, NoopRedactor, TenantId, UnexpectedErrorEvent};
use harness_journal::{EventStore, InMemoryEventStore, ReplayCursor};
use harness_observability::Observer;
use jyowo_harness_sdk::{
    testing, ConversationEventsPageRequest, Harness, SessionId, SessionOptions,
};

#[tokio::test]
async fn event_stream_redaction_redacts_business_visible_event_copy_without_mutating_journal() {
    let store = Arc::new(InMemoryEventStore::new(Arc::new(testing::NoopRedactor)));
    let session_id = SessionId::new();
    let raw_secret = "token sk-abcdefghijklmnopqrstuvwxyz and Bearer abcdefghijklmnopqrstuvwxyz";
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
        .expect("append raw event");

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
    assert!(!redacted.error.contains("Bearer abcdefghijklmnopqrstuvwxyz"));
    assert!(redacted.error.contains("[REDACTED]"));

    let stored = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("stored event should remain readable")
        .collect::<Vec<_>>()
        .await;
    let Event::UnexpectedError(raw) = &stored[0] else {
        panic!("unexpected stored event type");
    };
    assert_eq!(raw.error, raw_secret);
}

#[tokio::test]
async fn event_stream_redaction_is_deterministic_and_preserves_existing_replacement() {
    let store = Arc::new(InMemoryEventStore::new(Arc::new(testing::NoopRedactor)));
    let session_id = SessionId::new();
    store
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::UnexpectedError(UnexpectedErrorEvent {
                session_id: Some(session_id),
                run_id: None,
                error: "already [REDACTED]".to_owned(),
                at: harness_contracts::now(),
            })],
        )
        .await
        .expect("append redacted event");

    let harness = Harness::builder()
        .with_model(testing::TestModelProvider::default())
        .with_store_arc(store)
        .with_sandbox(testing::NoopSandbox::new())
        .build()
        .await
        .expect("harness should build");

    let first = harness
        .event_stream(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("first stream should open")
        .collect::<Vec<_>>()
        .await;
    let second = harness
        .event_stream(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("second stream should open")
        .collect::<Vec<_>>()
        .await;

    assert_eq!(first, second);
    let Event::UnexpectedError(event) = &first[0] else {
        panic!("unexpected event type");
    };
    assert_eq!(event.error, "already [REDACTED]");
}

#[tokio::test]
async fn conversation_event_page_applies_default_redaction_after_noop_observer() {
    let store = Arc::new(InMemoryEventStore::new(Arc::new(testing::NoopRedactor)));
    let session_id = SessionId::new();
    let workspace = std::env::temp_dir().join(format!(
        "jyowo-sdk-conversation-page-redaction-{}",
        SessionId::new()
    ));
    std::fs::create_dir_all(&workspace).expect("workspace should be created");
    let raw_secret = "token sk-abcdefghijklmnopqrstuvwxyz";
    let observer = Arc::new(
        Observer::builder()
            .with_redactor(Arc::new(NoopRedactor))
            .build()
            .expect("observer should build"),
    );
    let harness = Harness::builder()
        .with_model(testing::TestModelProvider::default())
        .with_store_arc(store.clone())
        .with_sandbox(testing::NoopSandbox::new())
        .with_observer(observer)
        .build()
        .await
        .expect("harness should build");
    let options = SessionOptions::new(&workspace).with_session_id(session_id);
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .expect("session should open");
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
        .expect("append raw event");

    let page = harness
        .page_conversation_events(ConversationEventsPageRequest {
            options,
            after_event_id: None,
            limit: 20,
        })
        .await
        .expect("event page should load");
    let Some(Event::UnexpectedError(redacted)) =
        page.events
            .iter()
            .find_map(|envelope| match &envelope.payload {
                Event::UnexpectedError(event) => Some(Event::UnexpectedError(event.clone())),
                _ => None,
            })
    else {
        panic!("unexpected error event should be present");
    };
    assert!(!redacted.error.contains("sk-abcdefghijklmnopqrstuvwxyz"));
    assert!(redacted.error.contains("[REDACTED]"));
}

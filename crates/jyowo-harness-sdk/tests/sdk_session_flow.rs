#![cfg(feature = "testing")]

use std::sync::Arc;

use futures::StreamExt;
use harness_contracts::{DeltaChunk, EndReason, Event, TenantId};
use harness_journal::{EventStore, ReplayCursor};
use harness_model::{ContentDelta, ModelStreamEvent};
use jyowo_harness_sdk::{prelude::*, testing::*};

#[tokio::test]
async fn sdk_session_flow_runs_turn_and_writes_journal_events() {
    let workspace = unique_workspace("sdk-session-flow");
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

    let harness = Harness::builder()
        .with_model(MockProvider::default().with_events(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("business answer".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ]))
        .with_store_arc(store.clone())
        .with_sandbox(NoopSandbox::new())
        .build()
        .await
        .expect("harness should build");

    let session = harness
        .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
        .await
        .expect("session should be created");

    session
        .run_turn("answer through public SDK")
        .await
        .expect("turn should run");

    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("journal should be readable")
        .collect()
        .await;

    assert!(events.iter().any(
        |event| matches!(event, Event::SessionCreated(created) if created.session_id == session_id)
    ));
    assert!(events.iter().any(
        |event| matches!(event, Event::RunStarted(started) if started.session_id == session_id)
    ));
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::AssistantDeltaProduced(delta)
                if matches!(&delta.delta, DeltaChunk::Text(text) if text == "business answer")
        )),
        "streaming model output should be journaled"
    );
    assert!(events.iter().any(|event| matches!(
        event,
        Event::RunEnded(ended) if matches!(ended.reason, EndReason::Completed)
    )));
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("{name}-{}", SessionId::new()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
}

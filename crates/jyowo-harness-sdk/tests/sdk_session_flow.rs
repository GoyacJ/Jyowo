#![cfg(feature = "testing")]

use std::sync::Arc;

use futures::StreamExt;
use harness_contracts::{
    ConversationContextReference, DeltaChunk, EndReason, Event, MemoryId, TenantId,
};
use harness_journal::{EventStore, ReplayCursor};
use harness_model::{ContentDelta, ModelStreamEvent, ScriptedProvider, ScriptedResponse};
use jyowo_harness_sdk::{prelude::*, testing::*};

#[tokio::test]
async fn sdk_session_flow_runs_turn_and_writes_journal_events() {
    let workspace = unique_workspace("sdk-session-flow");
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model(TestModelProvider::default().with_events(vec![
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

#[tokio::test]
async fn submit_conversation_turn_rejects_second_active_turn_for_same_session() {
    let workspace = unique_workspace("sdk-session-active-run");
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let model = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::WaitForCancel]));

    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model_arc(model.clone())
        .with_store_arc(store.clone())
        .with_sandbox(NoopSandbox::new())
        .build()
        .await
        .expect("harness should build");

    let options = SessionOptions::new(&workspace).with_session_id(session_id);
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .expect("session should be created");

    let first_harness = harness.clone();
    let first_options = options.clone();
    let first_turn = tokio::spawn(async move {
        first_harness
            .submit_conversation_turn(ConversationTurnRequest::from_prompt(
                first_options,
                ConversationRunOptions::default(),
                "first",
            ))
            .await
    });

    for _ in 0..100 {
        if !model.requests().await.is_empty() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(model.requests().await.len(), 1);

    let second = harness
        .submit_conversation_turn(ConversationTurnRequest::from_prompt(
            options,
            ConversationRunOptions::default(),
            "second",
        ))
        .await;

    first_turn.abort();
    let _ = first_turn.await;

    let error = second.expect_err("second active turn should be rejected");
    assert!(
        error
            .to_string()
            .contains("conversation run already active"),
        "unexpected error: {error}"
    );

    let run_started_count = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("journal should be readable")
        .filter(|event| {
            futures::future::ready(matches!(
                event,
                Event::RunStarted(started) if started.session_id == session_id
            ))
        })
        .count()
        .await;
    assert_eq!(run_started_count, 1);
}

#[tokio::test]
async fn rejected_second_active_turn_does_not_apply_memory_context_patch() {
    let workspace = unique_workspace("sdk-session-active-run-memory-patch");
    let session_id = SessionId::new();
    let memory_id = MemoryId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let model = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::WaitForCancel]));

    let harness = Harness::builder()
        .with_workspace_root(&workspace)
        .with_model_arc(model.clone())
        .with_store_arc(store.clone())
        .with_sandbox(NoopSandbox::new())
        .build()
        .await
        .expect("harness should build");

    let options = SessionOptions::new(&workspace).with_session_id(session_id);
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .expect("session should be created");

    let first_harness = harness.clone();
    let first_options = options.clone();
    let first_turn = tokio::spawn(async move {
        first_harness
            .submit_conversation_turn(ConversationTurnRequest::from_prompt(
                first_options,
                ConversationRunOptions::default(),
                "first",
            ))
            .await
    });

    for _ in 0..100 {
        if !model.requests().await.is_empty() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(model.requests().await.len(), 1);

    let mut second =
        ConversationTurnRequest::from_prompt(options, ConversationRunOptions::default(), "second");
    second.input.context_references = vec![ConversationContextReference::Memory {
        id: memory_id.to_string(),
        label: "existing memory".to_owned(),
        resolved_content: Some("should not be injected".to_owned()),
    }];

    let error = harness
        .submit_conversation_turn(second)
        .await
        .expect_err("second active turn should be rejected before context patches");

    first_turn.abort();
    let _ = first_turn.await;

    assert!(
        error
            .to_string()
            .contains("conversation run already active"),
        "unexpected error: {error}"
    );

    let context_patch_count = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("journal should be readable")
        .filter(|event| futures::future::ready(matches!(event, Event::ContextPatchApplied(_))))
        .count()
        .await;
    assert_eq!(context_patch_count, 0);
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("{name}-{}", SessionId::new()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
}

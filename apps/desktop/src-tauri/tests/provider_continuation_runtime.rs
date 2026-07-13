use std::sync::Arc;

use harness_contracts::{ConversationTurnInput, NoopRedactor, SessionId, UsageSnapshot};
use harness_journal::InMemoryEventStore;
use harness_model::{ModelStreamEvent, TestModelProvider};
use harness_provider_state::{ProviderContinuationKind, ProviderContinuationRecord};
use harness_sandbox::NoopSandbox;
use jyowo_harness_sdk::{ConversationRunOptions, ConversationTurnRequest, Harness, SessionOptions};
use serde_json::json;

#[path = "../src/commands/provider_continuation_runtime.rs"]
mod provider_continuation_runtime;

#[tokio::test]
async fn desktop_runtime_builder_persists_provider_continuations_in_its_runtime_root() {
    let workspace = tempfile::tempdir().unwrap();
    let runtime_root = workspace.path().join("runtime");
    std::fs::create_dir(&runtime_root).unwrap();
    let model = TestModelProvider::default().with_events(vec![
        ModelStreamEvent::MessageStart {
            message_id: "desktop-continuation".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ProviderContinuationDelta {
            kind: ProviderContinuationKind::ReasoningReplay,
            payload: json!({ "private": "desktop-runtime" }),
        },
        ModelStreamEvent::MessageStop,
    ]);
    let builder = Harness::builder()
        .with_workspace_root(workspace.path())
        .with_model(model)
        .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
        .with_sandbox(NoopSandbox::new());
    let harness = provider_continuation_runtime::with_file_provider_continuation_store(
        builder,
        &runtime_root,
    )
    .unwrap()
    .build()
    .await
    .unwrap();
    let options = SessionOptions::new(workspace.path()).with_session_id(SessionId::new());
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .unwrap();

    harness
        .submit_conversation_turn(ConversationTurnRequest {
            run_options: ConversationRunOptions::from_session_options(&options),
            options,
            input: ConversationTurnInput::ask("persist private continuation"),
            permission_actor_source: None,
        })
        .await
        .unwrap();

    let path = runtime_root.join("provider-continuations.jsonl");
    let contents = std::fs::read_to_string(&path).unwrap();
    let records = contents
        .lines()
        .map(|line| serde_json::from_str::<ProviderContinuationRecord>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].payload["private"], "desktop-runtime");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

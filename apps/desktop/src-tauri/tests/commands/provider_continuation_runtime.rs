use super::*;
use harness_provider_state::{
    FileProviderContinuationStore, ProviderContinuationKind, ProviderContinuationQuery,
    ProviderContinuationRecord, ProviderContinuationScope, ProviderContinuationStore,
};

#[tokio::test]
async fn provider_continuation_delete_conversation_uses_sdk_prune_path() {
    let workspace = canonical_unique_workspace("provider-continuation-delete-prune");
    std::fs::create_dir_all(&workspace).unwrap();
    write_sentinel(
        &workspace
            .join(".jyowo/runtime")
            .join("provider-continuation-runtime.version"),
    );
    std::fs::write(
        workspace
            .join(".jyowo/runtime")
            .join("provider-continuation-runtime.version"),
        "1\n",
    )
    .unwrap();

    let session_id = SessionId::new();
    let message_id = MessageId::new();
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let provider_store = Arc::new(
        FileProviderContinuationStore::open_runtime_dir(workspace.join(".jyowo").join("runtime"))
            .unwrap(),
    );
    let harness = Arc::new(
        Harness::builder()
            .with_options(test_harness_options(&workspace))
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_provider_continuation_store_arc(provider_store.clone())
            .with_stream_permission_broker_arc(
                stream_permission_runtime.broker(),
                stream_permission_runtime.resolver_handle(),
            )
            .build()
            .await
            .expect("harness should build"),
    );
    harness
        .open_or_create_conversation_session(
            SessionOptions::new(&workspace).with_session_id(session_id),
        )
        .await
        .expect("session should open");
    provider_store
        .append_batch(vec![ProviderContinuationRecord {
            provider_id: "deepseek".to_owned(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            protocol: ModelProtocol::ChatCompletions,
            dialect: "deepseek".to_owned(),
            tenant_id: TenantId::SINGLE,
            session_id,
            producing_run_id: RunId::new(),
            message_id,
            scope: ProviderContinuationScope::Conversation,
            kind: ProviderContinuationKind::ReasoningReplay,
            payload: serde_json::json!({ "private": "desktop-prune-sentinel" }),
            created_at: now(),
        }])
        .await
        .expect("record should append to real provider continuation store");

    let state = DesktopRuntimeState::with_harness_and_stream_permission_runtime_for_workspace(
        workspace.clone(),
        harness,
        stream_permission_runtime,
    )
    .expect("state should use harness permission broker");

    let deleted = delete_conversation_with_runtime_state(
        DeleteConversationRequest {
            conversation_id: session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("delete should go through SDK");

    assert_eq!(deleted.status, "deleted");
    let remaining = provider_store
        .load_for_messages(ProviderContinuationQuery {
            provider_id: "deepseek".to_owned(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            protocol: ModelProtocol::ChatCompletions,
            dialect: "deepseek".to_owned(),
            tenant_id: TenantId::SINGLE,
            session_id,
            message_ids: vec![message_id],
            kinds: vec![ProviderContinuationKind::ReasoningReplay],
        })
        .await
        .expect("store should remain readable after prune");
    assert!(remaining.is_empty());
    assert!(workspace
        .join(".jyowo/runtime/provider-continuations.jsonl")
        .exists());
}

fn write_sentinel(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, "sentinel").unwrap();
}

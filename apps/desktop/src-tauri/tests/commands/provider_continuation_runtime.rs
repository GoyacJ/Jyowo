use super::*;
use harness_provider_state::{
    FileProviderContinuationStore, ProviderContinuationKind, ProviderContinuationQuery,
    ProviderContinuationRecord, ProviderContinuationScope, ProviderContinuationStore,
};

#[tokio::test]
async fn provider_continuation_dev_reset_clears_legacy_conversation_runtime_once() {
    let workspace = canonical_unique_workspace("provider-continuation-dev-reset-once");
    let runtime_dir = workspace.join(".jyowo/runtime");
    std::fs::create_dir_all(runtime_dir.join("events")).unwrap();
    std::fs::create_dir_all(runtime_dir.join("sessions")).unwrap();

    let allowed_files = [
        runtime_dir.join("events/legacy.jsonl"),
        runtime_dir.join("sessions/legacy.json"),
        runtime_dir.join("conversation-read-model.sqlite"),
        runtime_dir.join("conversation-read-model.sqlite-shm"),
        runtime_dir.join("conversation-read-model.sqlite-wal"),
        runtime_dir.join("conversation-metadata.json"),
        runtime_dir.join("provider-continuations.jsonl"),
    ];
    for path in &allowed_files {
        write_sentinel(path);
    }

    reset_legacy_conversation_runtime_for_provider_continuations(&workspace)
        .expect("first reset should succeed");

    assert_eq!(
        std::fs::read_to_string(runtime_dir.join("provider-continuation-runtime.version"))
            .unwrap()
            .trim(),
        "1"
    );
    for path in &allowed_files {
        assert!(!path.exists(), "{} should be cleared", path.display());
    }
    assert!(runtime_dir.join("events").is_dir());
    assert!(runtime_dir.join("sessions").is_dir());

    let new_event = runtime_dir.join("events/current.jsonl");
    let new_session = runtime_dir.join("sessions/current.json");
    write_sentinel(&new_event);
    write_sentinel(&new_session);

    reset_legacy_conversation_runtime_for_provider_continuations(&workspace)
        .expect("second reset should no-op when marker is current");

    assert!(new_event.exists());
    assert!(new_session.exists());
}

#[tokio::test]
async fn provider_continuation_dev_reset_preserves_user_configuration_and_non_conversation_state() {
    let workspace = canonical_unique_workspace("provider-continuation-dev-reset-preserve");
    let runtime_dir = workspace.join(".jyowo/runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();

    let allowed = runtime_dir.join("provider-continuations.jsonl");
    write_sentinel(&allowed);

    let forbidden_files = [
        runtime_dir.join("provider-settings.json"),
        runtime_dir.join("provider-capability-routes.json"),
        runtime_dir.join("execution-settings.json"),
        runtime_dir.join("provider-diagnostics.json"),
        runtime_dir.join("provider-quota-cache.json"),
        runtime_dir.join("agent-profiles.json"),
    ];
    for path in &forbidden_files {
        write_sentinel(path);
    }
    let forbidden_skill = runtime_dir.join("skills/enabled/test/SKILL.md");
    let forbidden_plugin = runtime_dir.join("plugins/enabled/test/plugin.json");
    write_sentinel(&forbidden_skill);
    write_sentinel(&forbidden_plugin);

    reset_legacy_conversation_runtime_for_provider_continuations(&workspace)
        .expect("reset should succeed");

    assert!(!allowed.exists());
    for path in &forbidden_files {
        assert!(path.exists(), "{} should be preserved", path.display());
    }
    assert!(forbidden_skill.exists());
    assert!(forbidden_plugin.exists());
}

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
    let provider_store = Arc::new(FileProviderContinuationStore::open(&workspace).unwrap());
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

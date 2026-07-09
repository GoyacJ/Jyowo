use super::*;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

const DEEPSEEK_CONFIG_ID: &str = "deepseek-run-config";
const MINIMAX_CONFIG_ID: &str = "minimax-run-config";
const MINIMAX_PRIMARY_CONFIG_ID: &str = "minimax-primary-run-config";
const MINIMAX_SECONDARY_CONFIG_ID: &str = "minimax-secondary-run-config";

#[tokio::test]
async fn list_conversations_with_runtime_state_returns_startable_conversation_id() {
    let state = runtime_state_with_harness().await;
    let created = create_conversation_with_runtime_state(&state)
        .await
        .expect("draft conversation should be created");
    let payload = list_conversations_with_runtime_state(&state).await;
    let conversation_id = created.conversation.id;

    let session_id =
        SessionId::parse(&conversation_id).expect("conversation id should be a session id");
    assert_eq!(session_id.to_string(), conversation_id);
    assert!(payload
        .conversations
        .iter()
        .any(|conversation| conversation.id == conversation_id));

    let run = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id,
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Continue implementation".to_owned(),
        },
        &state,
    )
    .await
    .expect("listed conversation should be startable");

    assert_eq!(run.status, "started");
    assert_eq!(
        RunId::parse(&run.run_id)
            .expect("run id should be canonical")
            .to_string(),
        run.run_id
    );
    let detail = get_conversation_with_runtime_state(
        GetConversationRequest {
            conversation_id: session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("started draft conversation should read runtime messages");
    assert!(!detail.conversation.messages.is_empty());
}

#[tokio::test]
async fn create_conversation_with_runtime_state_persists_draft_metadata_only() {
    let state = runtime_state_with_harness().await;

    let created = create_conversation_with_runtime_state(&state)
        .await
        .expect("create conversation should write draft metadata");
    let conversation_id = created.conversation.id.clone();
    assert!(created.conversation.is_empty);
    let session_id =
        SessionId::parse(&conversation_id).expect("conversation id should be a session id");
    let events: Vec<_> = state
        .harness()
        .expect("harness should be available")
        .event_store()
        .read_envelopes(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("event store should be readable")
        .collect()
        .await;
    assert!(
        events
            .iter()
            .all(|envelope| !matches!(envelope.payload, Event::SessionCreated(_))),
        "draft creation must not write SessionCreated",
    );

    let listed = list_conversations_with_runtime_state(&state).await;
    assert!(listed
        .conversations
        .iter()
        .any(|conversation| conversation.id == conversation_id));

    let detail = get_conversation_with_runtime_state(
        GetConversationRequest {
            conversation_id: conversation_id.clone(),
        },
        &state,
    )
    .await
    .expect("created empty conversation should be readable");

    assert_eq!(detail.conversation.id, conversation_id);
    assert!(detail.conversation.messages.is_empty());
}

async fn mounted_chat_completion_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "data: {\"id\":\"chat_1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1}}\n\n",
                        "data: [DONE]\n\n",
                    ),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;
    server
}

async fn runtime_state_with_provider_configs(base_url: &str) -> DesktopRuntimeState {
    let workspace = unique_workspace("run-model-config");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let provider_settings_store: Arc<dyn ProviderSettingsStore> =
        Arc::new(provider_settings_store_for_workspace(&workspace));
    provider_settings_store
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some(DEEPSEEK_CONFIG_ID.to_owned()),
            configs: vec![deepseek_config(base_url), minimax_config(base_url)],
        })
        .expect("provider settings should save");
    runtime_state_from_stream_permission_runtime_with_provider_settings_store_for_test(
        workspace,
        Arc::new(StreamPermissionRuntime::default()),
        provider_settings_store,
    )
    .await
    .expect("runtime should start from provider settings")
}

async fn runtime_state_with_duplicate_minimax_configs(
    primary_base_url: &str,
    secondary_base_url: &str,
) -> DesktopRuntimeState {
    let workspace = unique_workspace("run-model-config-duplicate-provider");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let provider_settings_store: Arc<dyn ProviderSettingsStore> =
        Arc::new(provider_settings_store_for_workspace(&workspace));
    provider_settings_store
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some(MINIMAX_PRIMARY_CONFIG_ID.to_owned()),
            configs: vec![
                minimax_config_with_id(MINIMAX_PRIMARY_CONFIG_ID, primary_base_url),
                minimax_config_with_id(MINIMAX_SECONDARY_CONFIG_ID, secondary_base_url),
            ],
        })
        .expect("provider settings should save");
    runtime_state_from_stream_permission_runtime_with_provider_settings_store_for_test(
        workspace,
        Arc::new(StreamPermissionRuntime::default()),
        provider_settings_store,
    )
    .await
    .expect("runtime should start from provider settings")
}

fn deepseek_config(base_url: &str) -> ProviderConfigRecord {
    chat_provider_config_record(
        DEEPSEEK_CONFIG_ID,
        "deepseek",
        "deepseek-v4-flash",
        "DeepSeek V4 Flash",
        Some(base_url.to_owned()),
        "provider-key",
    )
}

fn minimax_config(base_url: &str) -> ProviderConfigRecord {
    minimax_config_with_id(MINIMAX_CONFIG_ID, base_url)
}

fn minimax_config_with_id(config_id: &str, base_url: &str) -> ProviderConfigRecord {
    chat_provider_config_record(
        config_id,
        "minimax",
        "MiniMax-M2.7",
        "MiniMax M2.7",
        Some(base_url.to_owned()),
        "provider-key",
    )
}

async fn wait_for_received_request_count(server: &MockServer, expected: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let requests = server
            .received_requests()
            .await
            .expect("wiremock requests should be readable");
        if requests.len() == expected {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "expected {expected} received requests, got {}",
                requests.len()
            );
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

async fn run_started_models(
    state: &DesktopRuntimeState,
    session_id: SessionId,
) -> Vec<RunModelSnapshot> {
    state
        .harness()
        .expect("runtime harness should be available")
        .event_store()
        .read_envelopes(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("events should be readable")
        .filter_map(|envelope| async move {
            match envelope.payload {
                Event::RunStarted(started) => Some(started.model),
                _ => None,
            }
        })
        .collect()
        .await
}

fn provider_config_json(config: &ProviderConfigRecord) -> Value {
    serde_json::to_value(config).expect("provider config should serialize")
}

fn write_provider_settings_json(workspace: &Path, value: Value) {
    let runtime_dir = workspace.join(".jyowo").join("runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::write(
        runtime_dir.join("provider-settings.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn start_run_uses_request_model_config_for_first_draft_run() {
    let server = mounted_chat_completion_server().await;
    let state = runtime_state_with_provider_configs(&server.uri()).await;
    let created = create_conversation_with_runtime_state(&state)
        .await
        .expect("draft conversation should be created");
    let session_id = SessionId::parse(&created.conversation.id).unwrap();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: created.conversation.id.clone(),
            model_config_id: Some(MINIMAX_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Use MiniMax for this run".to_owned(),
        },
        &state,
    )
    .await
    .expect("MiniMax run should start from a DeepSeek-default draft");

    let started = run_started_models(&state, session_id).await;
    let model = started.last().expect("RunStarted should be recorded");
    assert_eq!(model.model_config_id.as_deref(), Some(MINIMAX_CONFIG_ID));
    assert_eq!(model.provider_id, "minimax");
    assert_eq!(model.model_id, "MiniMax-M2.7");
    let detail = get_conversation_with_runtime_state(
        GetConversationRequest {
            conversation_id: created.conversation.id,
        },
        &state,
    )
    .await
    .expect("conversation should be readable");
    assert_eq!(
        detail.conversation.model_config_id.as_deref(),
        Some(MINIMAX_CONFIG_ID)
    );
}

#[tokio::test]
async fn start_run_allows_active_conversation_to_switch_models_per_run() {
    let server = mounted_chat_completion_server().await;
    let state = runtime_state_with_provider_configs(&server.uri()).await;
    let created = create_conversation_with_runtime_state(&state)
        .await
        .expect("draft conversation should be created");
    let session_id = SessionId::parse(&created.conversation.id).unwrap();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: created.conversation.id.clone(),
            model_config_id: Some(MINIMAX_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "First MiniMax run".to_owned(),
        },
        &state,
    )
    .await
    .expect("first run should start");
    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: created.conversation.id.clone(),
            model_config_id: Some(DEEPSEEK_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Second DeepSeek run".to_owned(),
        },
        &state,
    )
    .await
    .expect("second run should start with a different model config");

    let started = run_started_models(&state, session_id).await;
    assert!(started.len() >= 2, "expected two RunStarted events");
    assert_eq!(
        started[started.len() - 2].model_config_id.as_deref(),
        Some(MINIMAX_CONFIG_ID)
    );
    assert_eq!(started[started.len() - 2].provider_id, "minimax");
    assert_eq!(
        started[started.len() - 1].model_config_id.as_deref(),
        Some(DEEPSEEK_CONFIG_ID)
    );
    assert_eq!(started[started.len() - 1].provider_id, "deepseek");
}

#[tokio::test]
async fn start_run_rebuilds_harness_for_same_provider_model_with_different_config() {
    let primary_server = mounted_chat_completion_server().await;
    let secondary_server = mounted_chat_completion_server().await;
    let state = runtime_state_with_duplicate_minimax_configs(
        &primary_server.uri(),
        &secondary_server.uri(),
    )
    .await;
    let created = create_conversation_with_runtime_state(&state)
        .await
        .expect("draft conversation should be created");

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: created.conversation.id.clone(),
            model_config_id: Some(MINIMAX_PRIMARY_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Use primary MiniMax config".to_owned(),
        },
        &state,
    )
    .await
    .expect("primary MiniMax run should start");
    wait_for_received_request_count(&primary_server, 1).await;

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: created.conversation.id.clone(),
            model_config_id: Some(MINIMAX_SECONDARY_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Use secondary MiniMax config".to_owned(),
        },
        &state,
    )
    .await
    .expect("secondary MiniMax run should start");
    wait_for_received_request_count(&secondary_server, 1).await;

    let primary_requests = primary_server
        .received_requests()
        .await
        .expect("primary requests should be readable");
    assert_eq!(
        primary_requests.len(),
        1,
        "second run must not reuse the first config harness",
    );
}

#[tokio::test]
async fn start_run_rejects_invalid_model_config_without_activating_draft() {
    let server = mounted_chat_completion_server().await;
    let state = runtime_state_with_provider_configs(&server.uri()).await;
    let created = create_conversation_with_runtime_state(&state)
        .await
        .expect("draft conversation should be created");

    let missing = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: created.conversation.id.clone(),
            model_config_id: Some("missing-config".to_owned()),
            permission_mode: None,
            prompt: "Should fail".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("unknown model config should fail closed");
    assert_eq!(missing.code, "INVALID_PAYLOAD");

    let detail = get_conversation_with_runtime_state(
        GetConversationRequest {
            conversation_id: created.conversation.id.clone(),
        },
        &state,
    )
    .await
    .expect("draft should remain readable");
    assert!(detail.conversation.messages.is_empty());
    assert!(detail.conversation.model_config_id.is_none());

    let no_key_config_id = "minimax-no-key";
    write_provider_settings_json(
        state.workspace_root(),
        json!({
            "defaultConfigId": DEEPSEEK_CONFIG_ID,
            "configs": [
                provider_config_json(&deepseek_config(&server.uri())),
                {
                    "apiKey": "",
                    "protocol": "chat_completions",
                    "baseUrl": server.uri(),
                    "displayName": "MiniMax no key",
                    "id": no_key_config_id,
                    "modelId": "MiniMax-M2.7",
                    "providerId": "minimax",
                    "modelDescriptor": {
                        "protocol": "chat_completions",
                        "conversationCapability": {
                            "inputModalities": ["text"],
                            "outputModalities": ["text"],
                            "contextWindow": 128000,
                            "maxOutputTokens": 8192,
                            "streaming": true,
                            "toolCalling": true,
                            "reasoning": false,
                            "promptCache": false,
                            "structuredOutput": false
                        },
                        "contextWindow": 128000,
                        "displayName": "MiniMax M2.7",
                        "lifecycle": { "kind": "stable" },
                        "maxOutputTokens": 8192,
                        "modelId": "MiniMax-M2.7",
                        "providerId": "minimax"
                    }
                }
            ]
        }),
    );
    let no_key = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: created.conversation.id.clone(),
            model_config_id: Some(no_key_config_id.to_owned()),
            permission_mode: None,
            prompt: "Should fail without key".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("model config without api key should fail closed");
    assert_eq!(no_key.code, "INVALID_PAYLOAD");

    let detail = get_conversation_with_runtime_state(
        GetConversationRequest {
            conversation_id: created.conversation.id,
        },
        &state,
    )
    .await
    .expect("draft should still remain readable");
    assert!(detail.conversation.messages.is_empty());
    assert!(detail.conversation.model_config_id.is_none());
}

#[tokio::test]
async fn create_conversation_with_runtime_state_does_not_bind_default_model_config() {
    let workspace = unique_workspace("create-conversation-default-model");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    provider_settings_store_for_workspace(&workspace)
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai-work".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI Work".to_owned(),
                id: "openai-work".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                provider_defaults: None,
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;

    let created = create_conversation_with_runtime_state(&state)
        .await
        .expect("create conversation should write draft metadata");
    let detail = get_conversation_with_runtime_state(
        GetConversationRequest {
            conversation_id: created.conversation.id,
        },
        &state,
    )
    .await
    .expect("created conversation should be readable");

    assert_eq!(detail.conversation.model_config_id, None);
}

#[tokio::test]
async fn list_conversations_with_runtime_state_returns_empty_list_without_harness() {
    let workspace = unique_workspace("no-harness");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("workspace state should initialize without a harness");
    let payload = list_conversations_with_runtime_state(&state).await;

    assert!(payload.conversations.is_empty());
}

#[tokio::test]
async fn list_conversations_with_runtime_state_returns_empty_list_without_auto_runtime_session() {
    let state = runtime_state_with_harness().await;
    let payload = list_conversations_with_runtime_state(&state).await;

    assert!(payload.conversations.is_empty());
}

#[tokio::test]
async fn draft_conversation_can_list_get_and_delete() {
    let state = runtime_state_with_harness().await;
    let created = create_conversation_with_runtime_state(&state)
        .await
        .expect("draft conversation should be created");
    let conversation_id = created.conversation.id.clone();

    let listed = list_conversations_with_runtime_state(&state).await;
    assert!(listed
        .conversations
        .iter()
        .any(|conversation| conversation.id == conversation_id));

    let detail = get_conversation_with_runtime_state(
        GetConversationRequest {
            conversation_id: conversation_id.clone(),
        },
        &state,
    )
    .await
    .expect("draft conversation should be readable");

    assert_eq!(detail.conversation.id, conversation_id);
    assert!(detail.conversation.messages.is_empty());
    assert_eq!(detail.conversation.title, "New conversation");
    assert!(created.conversation.is_empty);
    let serialized = serde_json::to_value(&listed).expect("payload should serialize");
    assert_eq!(
        serialized["conversations"][0].get("lastMessagePreview"),
        None,
        "empty conversation preview should be omitted instead of serialized as null",
    );

    let deleted = delete_conversation_with_runtime_state(
        DeleteConversationRequest {
            conversation_id: conversation_id.clone(),
        },
        &state,
    )
    .await
    .expect("draft conversation should delete");
    assert_eq!(deleted.status, "deleted");
    let listed_after_delete = list_conversations_with_runtime_state(&state).await;
    assert!(!listed_after_delete
        .conversations
        .iter()
        .any(|conversation| conversation.id == conversation_id));
}

#[tokio::test]
async fn delete_conversation_with_runtime_state_removes_session_from_runtime_list() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Deleted conversation should not return".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let conversation_id = state.default_conversation_id().to_string();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: conversation_id.clone(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Create a conversation".to_owned(),
        },
        &state,
    )
    .await
    .expect("conversation should be created before deletion");

    let deleted = delete_conversation_with_runtime_state(
        DeleteConversationRequest {
            conversation_id: conversation_id.clone(),
        },
        &state,
    )
    .await
    .expect("conversation deletion should succeed");

    assert_eq!(deleted.conversation_id, conversation_id);
    assert_eq!(deleted.status, "deleted");

    let payload = list_conversations_with_runtime_state(&state).await;
    assert!(!payload
        .conversations
        .iter()
        .any(|conversation| conversation.id == conversation_id));

    let detail_error = get_conversation_with_runtime_state(
        GetConversationRequest {
            conversation_id: conversation_id.clone(),
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(detail_error.code, "NOT_FOUND");

    let restart_error = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id,
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Do not recreate a deleted conversation".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(restart_error.code, "NOT_FOUND");
}

#[tokio::test]
async fn get_and_delete_conversation_with_runtime_state_survive_runtime_option_changes() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Readable after runtime option change".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let conversation_id = state.default_conversation_id().to_string();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: conversation_id.clone(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Create a conversation before changing runtime options".to_owned(),
        },
        &state,
    )
    .await
    .expect("conversation should be created before runtime option changes");

    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    state.replace_harness(harness, "test-model".to_owned(), ModelProtocol::Responses);

    let detail = get_conversation_with_runtime_state(
        GetConversationRequest {
            conversation_id: conversation_id.clone(),
        },
        &state,
    )
    .await
    .expect("conversation reads should survive runtime option changes");
    assert!(detail.conversation.messages.iter().any(|message| message
        .body
        .contains("Readable after runtime option change")));

    let deleted = delete_conversation_with_runtime_state(
        DeleteConversationRequest {
            conversation_id: conversation_id.clone(),
        },
        &state,
    )
    .await
    .expect("conversation delete should survive runtime option changes");
    assert_eq!(deleted.conversation_id, conversation_id);
    assert_eq!(deleted.status, "deleted");
}

#[tokio::test]
async fn listed_empty_conversation_returns_empty_activity() {
    let state = runtime_state_with_harness().await;
    let created = create_conversation_with_runtime_state(&state)
        .await
        .expect("draft conversation should be created");
    let conversation_id = created.conversation.id;

    let activity = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(conversation_id),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("listed empty conversation activity should be readable");

    assert!(activity.events.is_empty());
}

#[tokio::test]
async fn listed_empty_conversation_returns_workspace_context() {
    let state = runtime_state_with_harness().await;
    let created = create_conversation_with_runtime_state(&state)
        .await
        .expect("draft conversation should be created");
    let conversation_id = created.conversation.id;

    let context = get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id: Some(conversation_id),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("listed empty conversation context should be readable");

    assert!(!context.project.is_empty());
    assert!(context.active_artifact.is_none());
}

#[tokio::test]
async fn get_conversation_with_runtime_state_returns_runtime_messages() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Ready".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Tell me status".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

    loop {
        let payload = get_conversation_with_runtime_state(
            GetConversationRequest {
                conversation_id: session_id.to_string(),
            },
            &state,
        )
        .await
        .unwrap();

        if payload.conversation.messages.len() >= 2 {
            assert_eq!(payload.conversation.messages[0].author, "user");
            assert_eq!(payload.conversation.messages[0].body, "Tell me status");
            assert_eq!(payload.conversation.messages[1].author, "assistant");
            assert!(payload.conversation.messages[1].body.contains("Ready"));
            assert!(!payload.conversation.updated_at.is_empty());
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("conversation detail should include runtime messages");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn list_conversations_with_runtime_state_projects_runtime_summary() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Ready from runtime".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Tell me status\nwith details".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let payload = list_conversations_with_runtime_state(&state).await;
        let Some(summary) = payload
            .conversations
            .iter()
            .find(|conversation| conversation.id == session_id.to_string())
        else {
            if tokio::time::Instant::now() >= deadline {
                panic!("started session should be listed");
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
            continue;
        };

        if summary.last_message_preview.as_deref() == Some("Ready from runtime") {
            assert!(!summary.is_empty);
            assert_eq!(summary.title, "Tell me status");
            assert_ne!(summary.updated_at, "2026-06-17T00:00:00.000Z");
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("conversation summary should include runtime message projection");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn conversation_payloads_with_runtime_state_redact_private_paths() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Read /home/goya/.ssh/config".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Read /Users/goya/.ssh/config".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let detail = get_conversation_with_runtime_state(
            GetConversationRequest {
                conversation_id: session_id.to_string(),
            },
            &state,
        )
        .await
        .unwrap();

        if detail.conversation.messages.len() >= 2 {
            assert_eq!(detail.conversation.messages[0].body, "Read [REDACTED]");
            assert_eq!(detail.conversation.messages[1].body, "Read [REDACTED]");

            let list = list_conversations_with_runtime_state(&state).await;
            let Some(summary) = list
                .conversations
                .iter()
                .find(|conversation| conversation.id == session_id.to_string())
            else {
                if tokio::time::Instant::now() >= deadline {
                    panic!("started session should be listed");
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
                continue;
            };
            assert_eq!(summary.title, "Read [REDACTED]");
            assert_eq!(
                summary.last_message_preview.as_deref(),
                Some("Read [REDACTED]")
            );
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("conversation payloads should include redacted runtime messages");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn get_conversation_with_runtime_state_includes_safe_client_message_id() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Done".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let client_message_id = "00000000-0000-4000-8000-000000000001".to_owned();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: Some(client_message_id.clone()),
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Complete the task".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        let payload = get_conversation_with_runtime_state(
            GetConversationRequest {
                conversation_id: session_id.to_string(),
            },
            &state,
        )
        .await
        .expect("conversation should load");

        if let Some(message) = payload
            .conversation
            .messages
            .iter()
            .find(|message| message.author == "user")
        {
            assert_eq!(
                message.client_message_id.as_deref(),
                Some(client_message_id.as_str())
            );
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("user message should be available");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

use super::*;

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
            agent_options: None,
            context_references: None,
            conversation_id,
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

#[tokio::test]
async fn create_conversation_with_runtime_state_does_not_bind_default_model_config() {
    let workspace = unique_workspace("create-conversation-default-model");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    DesktopProviderSettingsStore::new(workspace.clone())
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
            agent_options: None,
            context_references: None,
            conversation_id: conversation_id.clone(),
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
            agent_options: None,
            context_references: None,
            conversation_id,
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
            agent_options: None,
            context_references: None,
            conversation_id: conversation_id.clone(),
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
            agent_options: None,
            context_references: None,
            conversation_id: session_id.to_string(),
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
            agent_options: None,
            context_references: None,
            conversation_id: session_id.to_string(),
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
            agent_options: None,
            context_references: None,
            conversation_id: session_id.to_string(),
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
            agent_options: None,
            context_references: None,
            conversation_id: session_id.to_string(),
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

use super::*;

#[test]
fn context_file_payload_skips_missing_optional_state() {
    let value = serde_json::to_value(jyowo_desktop_shell::commands::ContextFilePayload {
        label: "apps/desktop/src/main.tsx".to_owned(),
        state: None,
    })
    .unwrap();

    assert_eq!(
        value,
        json!({
            "label": "apps/desktop/src/main.tsx"
        })
    );
}

#[tokio::test]
async fn get_context_snapshot_with_runtime_state_returns_workspace_metadata_without_session() {
    let workspace = unique_workspace("context-snapshot-no-session");
    std::fs::create_dir_all(workspace.join("apps/desktop/src")).unwrap();
    std::fs::write(
        workspace.join("apps/desktop/src/main.tsx"),
        "console.log('ready')",
    )
    .unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace.clone()).await;
    let session_id = SessionId::new();

    let context = get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("missing conversation events should still return workspace metadata");

    assert_eq!(
        context.project,
        workspace.file_name().unwrap().to_string_lossy()
    );
    assert_eq!(context.path, "workspace://local");
    assert!(context.active_artifact.is_none());
    assert!(context.decisions.is_empty());
    assert_eq!(context.next_actions, vec!["Continue the conversation"]);
    assert_eq!(
        context.files,
        vec![jyowo_desktop_shell::commands::ContextFilePayload {
            label: "apps/desktop/src/main.tsx".to_owned(),
            state: Some("ready"),
        }]
    );
}

#[tokio::test]
async fn get_context_snapshot_with_runtime_state_does_not_project_assistant_reply_as_artifact() {
    let workspace = unique_workspace("context-snapshot");
    std::fs::create_dir_all(workspace.join("apps/desktop/src")).unwrap();
    std::fs::write(
        workspace.join("apps/desktop/src/main.tsx"),
        "export const app = 'jyowo';",
    )
    .unwrap();
    let state = runtime_state_with_scripted_model_for_workspace(
        workspace.clone(),
        vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("# Runtime context artifact\n\nReady.".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ])],
    )
    .await;
    let session_id = state.default_conversation_id();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
            permission_mode: None,
            prompt: "Create a context artifact".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    loop {
        let conversation = get_conversation_with_runtime_state(
            GetConversationRequest {
                conversation_id: session_id.to_string(),
            },
            &state,
        )
        .await
        .expect("runtime conversation should load");
        if conversation
            .conversation
            .messages
            .iter()
            .any(|message| message.body.contains("Runtime context artifact"))
        {
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("runtime assistant output should complete");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    let payload = get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("runtime context snapshot should load");

    assert_eq!(payload.active_artifact, None);
    assert_eq!(
        payload.project,
        workspace.file_name().unwrap().to_string_lossy()
    );
    assert_eq!(payload.path, "workspace://local");
    assert!(payload
        .files
        .iter()
        .any(|file| { file.label == "apps/desktop/src/main.tsx" && file.state == Some("ready") }));
    assert!(payload
        .next_actions
        .iter()
        .any(|action| action == "Continue the conversation"));
}

#[tokio::test]
async fn get_context_snapshot_with_runtime_state_exposes_pending_decisions() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "NeedsPermission".to_owned(),
                input: json!({ "command": "pwd" }),
            },
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
            permission_mode: None,
            prompt: "Run a command".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let pending = wait_for_pending_permission_for_session(&state, session_id).await;

    let payload = get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(started.run_id),
        },
        &state,
    )
    .await
    .expect("runtime context snapshot should load pending decisions");

    assert!(payload.decisions.iter().any(|decision| {
        decision.title == "Approve NeedsPermission"
            && decision
                .detail
                .contains(&pending.request.request_id.to_string())
            && decision.request_id.as_deref() == Some(&pending.request.request_id.to_string())
    }));

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Deny,
            request_id: pending.request.request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn get_context_snapshot_with_runtime_state_redacts_pending_decision_tool_names() {
    let state = runtime_state_with_harness().await;
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let session_id = state.default_conversation_id();
    let run_id = RunId::new();
    open_conversation_session(&state, session_id).await;
    let mut request = permission_request();
    request.session_id = session_id;
    request.tool_name = "sk-abcdefghijklmnopqrstuvwxyz".to_owned();
    let request_id = request.request_id;
    let expected_title = format!(
        "Approve {}",
        DefaultRedactor::default().redact(
            &request.tool_name,
            &RedactRules {
                scope: RedactScope::EventBody,
                replacement: "[REDACTED]".to_owned(),
                pattern_set: RedactPatternSet::AllBuiltins,
            },
        )
    );

    let decision_task = tokio::spawn(async move {
        broker
            .decide(request, permission_context_with_run_id(Some(run_id)))
            .await
    });
    wait_for_pending_permission(&state, request_id).await;

    let payload = get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("runtime context snapshot should load pending decisions");
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(payload
        .decisions
        .iter()
        .any(|decision| decision.title == expected_title));
    assert!(!serialized.contains("sk-abcdefghijklmnopqrstuvwxyz"));

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Deny,
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(decision_task.await.unwrap(), Decision::DenyOnce);
}

#[tokio::test]
async fn get_context_snapshot_with_runtime_state_redacts_workspace_display_fields() {
    let secret_workspace_segment = "sk-abcdefghijklmnopqrstuvwxyz";
    let workspace = unique_workspace(&format!("context-snapshot-{secret_workspace_segment}"));
    let state = runtime_state_with_harness_for_workspace(workspace).await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;

    let payload = get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("runtime context snapshot should load workspace display fields");
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(!serialized.contains(secret_workspace_segment));
    assert_eq!(payload.path, "workspace://local");
    assert!(payload.project.contains("[REDACTED]"));
}

#[tokio::test]
async fn get_context_snapshot_with_runtime_state_hides_runtime_read_errors() {
    let state = runtime_state_with_harness().await;

    let payload = get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id: Some(state.default_conversation_id().to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("missing conversation session should still return workspace metadata");

    assert_eq!(
        payload.project,
        state
            .workspace_root()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap()
    );
    assert_eq!(payload.path, "workspace://local");
    assert!(payload.files.is_empty());
    assert!(payload.decisions.is_empty());
}

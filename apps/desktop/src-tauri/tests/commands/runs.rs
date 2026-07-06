#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

#[test]
fn start_run_payload_validates_prompt_and_requires_runtime() {
    let error = start_run_payload(StartRunRequest {
        client_message_id: None,
        attachments: None,
        context_references: Some(vec![ContextReferencePayload::WorkspaceFile {
            label: "Desktop app".to_owned(),
            path: "apps/desktop".to_owned(),
        }]),
        conversation_id: SessionId::new().to_string(),
        model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
        permission_mode: None,
        prompt: "Continue implementation".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");

    let error = start_run_payload(StartRunRequest {
        client_message_id: None,
        attachments: None,
        context_references: None,
        conversation_id: SessionId::new().to_string(),
        model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
        permission_mode: None,
        prompt: String::new(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");

    let error = start_run_payload(StartRunRequest {
        client_message_id: Some("00000000-0000-1000-8000-000000000001".to_owned()),
        attachments: None,
        context_references: None,
        conversation_id: SessionId::new().to_string(),
        model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
        permission_mode: None,
        prompt: "Continue implementation".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn create_attachment_from_path_writes_workspace_file_to_blob_store() {
    let workspace = unique_workspace("attachment-workspace-file");
    let attachment_path = workspace.join("notes.txt");
    std::fs::create_dir_all(attachment_path.parent().unwrap()).unwrap();
    std::fs::write(&attachment_path, "local notes").unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace.clone()).await;

    let payload = create_attachment_from_path_with_runtime_state(
        CreateAttachmentFromPathRequest {
            path: attachment_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .expect("workspace file should become an attachment reference");

    assert_eq!(payload.attachment.name, "notes.txt");
    assert_eq!(payload.attachment.mime_type, "text/plain");

    let record_path = workspace
        .join(".jyowo")
        .join("runtime")
        .join("attachments")
        .join("records")
        .join(format!("{}.json", payload.attachment.id));
    let record: Value = serde_json::from_slice(&std::fs::read(record_path).unwrap()).unwrap();
    assert_eq!(
        record["blobRef"]["size"].as_u64(),
        Some("local notes".len() as u64)
    );
    assert_eq!(
        record["attachment"]["blobRef"]["contentType"].as_str(),
        Some("text/plain")
    );
    assert_eq!(
        record["blobRef"]["content_type"].as_str(),
        Some("text/plain")
    );
}

#[tokio::test]
async fn create_attachment_from_path_rejects_external_file_before_read() {
    let workspace = unique_workspace("attachment-external-workspace");
    let external = unique_workspace("attachment-external-source");
    let attachment_path = external.join("outside.txt");
    std::fs::create_dir_all(attachment_path.parent().unwrap()).unwrap();
    std::fs::write(&attachment_path, "external notes").unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace.clone()).await;

    let error = create_attachment_from_path_with_runtime_state(
        CreateAttachmentFromPathRequest {
            path: attachment_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("workspace"));
}

#[tokio::test]
async fn create_attachment_from_path_does_not_reveal_external_path_existence() {
    let workspace = unique_workspace("attachment-existence-workspace");
    let external = unique_workspace("attachment-existence-source");
    let existing_path = external.join("outside.txt");
    let missing_path = external.join("missing.txt");
    std::fs::create_dir_all(existing_path.parent().unwrap()).unwrap();
    std::fs::write(&existing_path, "external notes").unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;

    let existing_error = create_attachment_from_path_with_runtime_state(
        CreateAttachmentFromPathRequest {
            path: existing_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();
    let missing_error = create_attachment_from_path_with_runtime_state(
        CreateAttachmentFromPathRequest {
            path: missing_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(existing_error.code, "INVALID_PAYLOAD");
    assert_eq!(missing_error.code, "INVALID_PAYLOAD");
    assert_eq!(existing_error.message, missing_error.message);
    assert!(existing_error.message.contains("workspace"));
}

#[tokio::test]
async fn create_attachment_from_path_rejects_files_larger_than_five_mb() {
    let workspace = unique_workspace("attachment-too-large");
    let attachment_path = workspace.join("large.txt");
    std::fs::create_dir_all(attachment_path.parent().unwrap()).unwrap();
    std::fs::write(&attachment_path, vec![b'x'; 5 * 1024 * 1024 + 1]).unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;

    let error = create_attachment_from_path_with_runtime_state(
        CreateAttachmentFromPathRequest {
            path: attachment_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("5 MB"));
}

#[tokio::test]
async fn start_run_with_runtime_state_rejects_untrusted_attachment_id_before_record_read() {
    let state = runtime_state_with_harness_for_workspace(unique_workspace("attachment-id")).await;

    let error = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: Some(vec![AttachmentReferencePayload {
                id: "../escape".to_owned(),
                mime_type: "text/plain".to_owned(),
                name: "notes.txt".to_owned(),
                size_bytes: 128,
                blob_ref: test_attachment_blob_ref(128, "text/plain"),
            }]),
            context_references: None,
            conversation_id: SessionId::new().to_string(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Use this attachment".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("generated attachment id"));
}

#[tokio::test]
async fn list_reference_candidates_includes_workspace_files() {
    let workspace = unique_workspace("reference-candidates");
    let file_path = workspace.join("apps/desktop/src-tauri/src/commands/mod.rs");
    std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    std::fs::write(&file_path, "fn main() {}").unwrap();
    let state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;
    register_test_skill(&state, "shell-state", "Shell state");
    register_test_tool(&state, "list_dir", "List directory");
    let state_for_command = state.clone();
    run_with_mcp_transport_approval(&state, async move {
        save_mcp_server_with_runtime_state(
            SaveMcpServerRequest {
                enabled: true,
                display_name: "Workspace Stdio".to_owned(),
                id: "stdio".to_owned(),
                scope: "global".to_owned(),
                transport: SaveMcpServerTransportConfig::Stdio {
                    command: "/bin/sh".to_owned(),
                    args: vec!["-c".to_owned(), stdio_mcp_fixture_script()],
                    env: Vec::new(),
                    inherit_env: Vec::new(),
                    working_dir: None,
                },
            },
            &state_for_command,
        )
        .await
    })
    .await
    .expect("mcp server should register");

    let payload = list_reference_candidates_with_runtime_state(
        ListReferenceCandidatesRequest {
            conversation_id: state.default_conversation_id().to_string(),
        },
        &state,
    )
    .await
    .expect("reference candidates should load");

    assert!(payload.files.iter().any(|candidate| {
        candidate.path.as_deref() == Some("apps/desktop/src-tauri/src/commands/mod.rs")
    }));
    assert!(payload
        .skills
        .iter()
        .any(|candidate| candidate.id.as_deref() == Some("shell-state")));
    assert!(payload
        .tools
        .iter()
        .any(|candidate| candidate.id.as_deref() == Some("mcp__stdio__echo")));
    assert!(payload
        .mcp_servers
        .iter()
        .any(|candidate| candidate.id.as_deref() == Some("stdio")));
}

#[tokio::test]
async fn list_reference_candidates_accepts_conversation_beyond_summary_page() {
    let state = runtime_state_with_harness().await;
    let file_path = state.workspace_root().join("apps/desktop/src/main.tsx");
    std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    std::fs::write(&file_path, "export {}").unwrap();
    let requested_session_id = SessionId::new();
    open_conversation_session(&state, requested_session_id).await;
    for _ in 0..60 {
        open_conversation_session(&state, SessionId::new()).await;
    }

    let payload = list_reference_candidates_with_runtime_state(
        ListReferenceCandidatesRequest {
            conversation_id: requested_session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("reference candidates should load for existing conversations beyond summaries");

    assert!(payload
        .files
        .iter()
        .any(|candidate| candidate.path.as_deref() == Some("apps/desktop/src/main.tsx")));
}

#[tokio::test]
async fn start_run_with_runtime_state_accepts_structured_context_and_attachments() {
    let workspace = unique_workspace("structured-start-run");
    let workspace_file = workspace.join("docs/notes.txt");
    std::fs::create_dir_all(workspace_file.parent().unwrap()).unwrap();
    std::fs::write(&workspace_file, "workspace context").unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;
    let attachment = create_attachment_from_path_with_runtime_state(
        CreateAttachmentFromPathRequest {
            path: workspace_file.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .expect("attachment should be stored")
    .attachment;
    let session_id = SessionId::new();

    let payload = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: Some(vec![attachment]),
            context_references: Some(vec![ContextReferencePayload::WorkspaceFile {
                label: "Notes".to_owned(),
                path: "docs/notes.txt".to_owned(),
            }]),
            conversation_id: session_id.to_string(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Run the relevant checks".to_owned(),
        },
        &state,
    )
    .await
    .expect("structured composer draft should start a run");

    assert_eq!(payload.status, "started");
    assert!(RunId::parse(&payload.run_id).is_ok());
    assert!(state.pending_permission_requests().is_empty());
}

#[tokio::test]
async fn start_run_with_runtime_state_rejects_workspace_file_reference_outside_workspace() {
    let workspace = unique_workspace("reference-workspace");
    let external = unique_workspace("reference-external");
    let external_file = external.join("outside.txt");
    std::fs::create_dir_all(external_file.parent().unwrap()).unwrap();
    std::fs::write(&external_file, "outside").unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;

    let error = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: Some(vec![ContextReferencePayload::WorkspaceFile {
                label: "Outside".to_owned(),
                path: external_file.to_string_lossy().to_string(),
            }]),
            conversation_id: SessionId::new().to_string(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Use this file".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("inside the workspace"));
}

#[tokio::test]
async fn start_run_with_runtime_state_returns_real_run_id_for_conversation() {
    let state = runtime_state_with_harness().await;
    let context_file = state.workspace_root().join("apps/desktop/src/main.tsx");
    std::fs::create_dir_all(context_file.parent().unwrap()).unwrap();
    std::fs::write(&context_file, "export {}").unwrap();
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let session_id = SessionId::new();
    let conversation_id = session_id.to_string();

    let payload = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: Some(vec![ContextReferencePayload::WorkspaceFile {
                label: "Desktop app".to_owned(),
                path: "apps/desktop/src/main.tsx".to_owned(),
            }]),
            conversation_id: conversation_id.clone(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Continue implementation".to_owned(),
        },
        &state,
    )
    .await
    .expect("runtime state should start a conversation run");

    assert_eq!(payload.status, "started");
    let run_id = RunId::parse(&payload.run_id).expect("run id should be canonical");
    assert_eq!(run_id.to_string(), payload.run_id);

    let page = harness
        .page_conversation_events(ConversationEventsPageRequest {
            options: state.conversation_session_options(session_id),
            after_event_id: None,
            limit: 20,
        })
        .await
        .expect("conversation events should be readable after start_run");

    assert!(page.events.iter().any(|envelope| {
        matches!(
            &envelope.payload,
            Event::RunStarted(started)
                if started.session_id == session_id && started.run_id == run_id
        )
    }));
    assert_eq!(conversation_id, session_id.to_string());
}

#[test]
fn cancel_run_payload_validates_and_requires_runtime() {
    let error = cancel_run_payload(CancelRunRequest {
        run_id: "run-001".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");

    let error = cancel_run_payload(CancelRunRequest {
        run_id: String::new(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn cancel_run_with_runtime_state_cancels_active_run_through_sdk() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "NeedsPermission".to_owned(),
                input: json!({ "command": "printf cancel-me" }),
            },
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = state.default_conversation_id();
    let started = tokio::time::timeout(Duration::from_secs(1), async {
        start_run_with_runtime_state(
            StartRunRequest {
                client_message_id: None,
                attachments: None,
                context_references: None,
                conversation_id: session_id.to_string(),
                model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
                permission_mode: None,
                prompt: "Run a cancellable command".to_owned(),
            },
            &state,
        )
        .await
    })
    .await
    .expect("start_run should return while permission is pending")
    .expect("start_run should start a cancellable run");

    let payload = cancel_run_with_runtime_state(
        CancelRunRequest {
            run_id: started.run_id.clone(),
        },
        &state,
    )
    .await
    .expect("active run should cancel through runtime state");

    assert_eq!(payload.run_id, started.run_id);
    assert_eq!(payload.status, "cancelled");
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_state_async_uses_explicit_workspace_root() {
    let _lock = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace_root = unique_workspace("explicit-workspace-root");
    std::fs::create_dir_all(&workspace_root).unwrap();
    let _env = EnvVarGuard::set(WORKSPACE_ROOT_ENV, workspace_root.as_os_str());

    let state = runtime_state_async()
        .await
        .expect("runtime state should initialize with explicit workspace root");
    let options = state.conversation_session_options(SessionId::new());

    assert_eq!(
        options.workspace_root,
        workspace_root.canonicalize().unwrap()
    );
}

#[test]
fn start_run_request_deserializes_permission_mode_override() {
    let conversation_id = SessionId::new().to_string();
    let request: StartRunRequest = serde_json::from_value(json!({
        "conversationId": conversation_id,
        "modelConfigId": TEST_MODEL_CONFIG_ID,
        "prompt": "Run checks",
        "permissionMode": "bypass_permissions",
    }))
    .expect("start_run request should deserialize");

    assert_eq!(
        request.permission_mode,
        Some(PermissionMode::BypassPermissions)
    );
    assert_eq!(request.conversation_id, conversation_id);
}

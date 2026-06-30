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

#[tokio::test]
async fn subscribe_conversation_events_emits_live_batches_and_unsubscribes() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    open_conversation_session(&state, session_id).await;
    let conversation_id = session_id.to_string();
    let batches = Arc::new(Mutex::new(Vec::<ConversationEventBatchPayload>::new()));
    let emitted_batches = Arc::clone(&batches);

    let subscription = subscribe_conversation_events_for_window_with_runtime_state(
        SubscribeConversationEventsRequest {
            conversation_id: conversation_id.clone(),
            after_cursor: None,
        },
        "main".to_owned(),
        Arc::new(move |batch| {
            emitted_batches.lock().unwrap().push(batch);
            Ok(())
        }),
        &state,
    )
    .await
    .expect("subscription should be accepted");

    assert_eq!(subscription.conversation_id, conversation_id);
    assert!(subscription.replay_events.is_empty());
    assert!(!subscription.gap);

    let started = start_run_with_runtime_state(
        StartRunRequest {
            attachments: None,
            client_message_id: Some("00000000-0000-4000-8000-000000000001".to_owned()),
            context_references: None,
            conversation_id: conversation_id.clone(),
            permission_mode: None,
            prompt: "Continue implementation".to_owned(),
        },
        &state,
    )
    .await
    .expect("run should start after subscribing");

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if batches.lock().unwrap().iter().any(|batch| {
                batch.subscription_id == subscription.subscription_id
                    && batch.conversation_id == conversation_id
                    && batch.phase == "live"
                    && batch.events.iter().any(|event| {
                        event.run_id == started.run_id && event.event_type == "run.started"
                    })
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("live subscription should emit the new run event");

    let emitted = batches.lock().unwrap();
    let live_events = emitted
        .iter()
        .filter(|batch| batch.subscription_id == subscription.subscription_id)
        .flat_map(|batch| batch.events.iter())
        .collect::<Vec<_>>();
    let live_run_started = live_events
        .iter()
        .find(|event| event.run_id == started.run_id && event.event_type == "run.started")
        .expect("live batch should include the started run event");
    assert!(live_run_started.conversation_sequence > 0);
    assert!(live_events
        .windows(2)
        .all(|pair| pair[0].conversation_sequence < pair[1].conversation_sequence));

    let unsubscribed = unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: subscription.subscription_id.clone(),
        },
        "main".to_owned(),
        &state,
    )
    .await
    .expect("unsubscribe should succeed");
    assert_eq!(unsubscribed.status, "unsubscribed");

    let already_closed = unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: subscription.subscription_id,
        },
        "main".to_owned(),
        &state,
    )
    .await
    .expect("unsubscribe should be idempotent");
    assert_eq!(already_closed.status, "alreadyClosed");
}

#[tokio::test]
async fn unsubscribe_conversation_events_rejects_other_window_subscription() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    open_conversation_session(&state, session_id).await;
    let conversation_id = session_id.to_string();
    let subscription = subscribe_conversation_events_for_window_with_runtime_state(
        SubscribeConversationEventsRequest {
            conversation_id,
            after_cursor: None,
        },
        "main".to_owned(),
        Arc::new(|_batch| Ok(())),
        &state,
    )
    .await
    .expect("subscription should be created");

    let denied = unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: subscription.subscription_id.clone(),
        },
        "secondary".to_owned(),
        &state,
    )
    .await
    .expect_err("another window must not close the subscription");
    assert_eq!(denied.code, "INVALID_PAYLOAD");

    let unsubscribed = unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: subscription.subscription_id,
        },
        "main".to_owned(),
        &state,
    )
    .await
    .expect("owning window can close the subscription");
    assert_eq!(unsubscribed.status, "unsubscribed");
}

#[tokio::test]
async fn subscribe_conversation_events_accepts_cursor_after_replayed_permission_request() {
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
    let conversation_id = session_id.to_string();

    start_run_with_runtime_state(
        StartRunRequest {
            attachments: None,
            client_message_id: Some("00000000-0000-4000-8000-000000000001".to_owned()),
            context_references: None,
            conversation_id: conversation_id.clone(),
            permission_mode: None,
            prompt: "Run a command".to_owned(),
        },
        &state,
    )
    .await
    .expect("run should start and wait on permission");
    let pending = wait_for_pending_permission_for_session(&state, session_id).await;

    let first_subscription = subscribe_conversation_events_for_window_with_runtime_state(
        SubscribeConversationEventsRequest {
            conversation_id: conversation_id.clone(),
            after_cursor: None,
        },
        "main".to_owned(),
        Arc::new(|_batch| Ok(())),
        &state,
    )
    .await
    .expect("subscription replay should include pending permission");
    assert!(first_subscription
        .replay_events
        .iter()
        .any(|event| event.event_type == "permission.requested"));
    let cursor = first_subscription
        .cursor
        .clone()
        .expect("subscription replay should return a cursor");

    let second_subscription = subscribe_conversation_events_for_window_with_runtime_state(
        SubscribeConversationEventsRequest {
            conversation_id: conversation_id.clone(),
            after_cursor: Some(cursor),
        },
        "main".to_owned(),
        Arc::new(|_batch| Ok(())),
        &state,
    )
    .await
    .expect("cursor from permission replay should be accepted by the next subscription");
    assert!(second_subscription.replay_events.is_empty());

    unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: first_subscription.subscription_id,
        },
        "main".to_owned(),
        &state,
    )
    .await
    .unwrap();
    unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: second_subscription.subscription_id,
        },
        "main".to_owned(),
        &state,
    )
    .await
    .unwrap();
    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id,
            decision: PermissionDecision::Deny,
            request_id: pending.request.request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn start_run_with_runtime_state_exposes_runtime_permission_request_to_activity() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "NeedsPermission".to_owned(),
                input: json!({ "command": "printf desktop-permission" }),
            },
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let conversation_id = session_id.to_string();

    let started = tokio::time::timeout(Duration::from_secs(1), async {
        start_run_with_runtime_state(
            StartRunRequest {
                client_message_id: None,
                attachments: None,
                context_references: None,
                conversation_id: conversation_id.clone(),
                permission_mode: None,
                prompt: "Run a command".to_owned(),
            },
            &state,
        )
        .await
    })
    .await
    .expect("start_run should return while permission is pending")
    .expect("start_run should start a conversation run");
    let run_id = RunId::parse(&started.run_id).expect("run id should be canonical");

    let pending = wait_for_pending_permission_for_session(&state, session_id).await;
    let request_id = pending.request.request_id;
    assert_eq!(pending.context.run_id, Some(run_id));
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let page = harness
        .page_conversation_events(ConversationEventsPageRequest {
            options: state.conversation_session_options(session_id),
            after_event_id: None,
            limit: 20,
        })
        .await
        .expect("conversation events should be readable while permission is pending");
    assert!(page.events.iter().any(|envelope| {
        matches!(
            &envelope.payload,
            Event::PermissionRequested(requested) if requested.request_id == request_id
        )
    }));

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(conversation_id),
            run_id: Some(started.run_id),
        },
        &state,
    )
    .await
    .unwrap();
    let value = serde_json::to_value(&payload).unwrap();

    let permission_event = value["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["type"] == "permission.requested")
        .expect("activity should include the pending permission event");
    assert_eq!(
        permission_event["payload"]["requestId"],
        serde_json::Value::String(request_id.to_string())
    );
    assert_eq!(
        permission_event["payload"]["toolUseId"],
        serde_json::Value::String(pending.request.tool_use_id.to_string())
    );
    assert_eq!(
        permission_event["payload"]["operation"],
        serde_json::Value::String("Execute command".to_owned())
    );
    assert_eq!(
        permission_event["payload"]["target"],
        serde_json::Value::String("printf".to_owned())
    );
    assert!(permission_event["payload"].get("command").is_none());

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .unwrap();
    let value = serde_json::to_value(&payload).unwrap();
    let permission_event = value["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["type"] == "permission.requested")
        .expect("run-filtered activity should include the pending permission event");
    assert_eq!(
        permission_event["payload"]["requestId"],
        serde_json::Value::String(request_id.to_string())
    );
    assert_eq!(
        permission_event["payload"]["toolUseId"],
        serde_json::Value::String(pending.request.tool_use_id.to_string())
    );

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

#[test]
fn resolve_permission_payload_requires_runtime_permission_broker() {
    let conversation_id = SessionId::new().to_string();
    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id: conversation_id.clone(),
        decision: PermissionDecision::Approve,
        request_id: "01HZ0000000000000000000001".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");

    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id: conversation_id.clone(),
        decision: PermissionDecision::Deny,
        request_id: "01HZ0000000000000000000001".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");

    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id,
        decision: PermissionDecision::Approve,
        request_id: String::new(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[test]
fn resolve_permission_payload_rejects_invalid_request_id_before_runtime() {
    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id: SessionId::new().to_string(),
        decision: PermissionDecision::Approve,
        request_id: "permission-001".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[test]
fn resolve_permission_payload_rejects_noncanonical_request_id_before_runtime() {
    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id: SessionId::new().to_string(),
        decision: PermissionDecision::Approve,
        request_id: "01hz0000000000000000000001".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn runtime_state_routes_permission_decisions_to_permission_broker_resolver() {
    let workspace = unique_workspace("runtime-state-routes");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace)
        .await
        .expect("runtime state should initialize");
    assert!(state.harness().is_some());

    let error = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: SessionId::new().to_string(),
            decision: PermissionDecision::Approve,
            request_id: "01HZ0000000000000000000001".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "NOT_FOUND");
    assert!(error.message.contains("permission request not found"));
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

#[tokio::test(flavor = "current_thread")]
async fn runtime_state_async_rejects_missing_explicit_workspace_root() {
    let _lock = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace_root = unique_workspace("missing-explicit-workspace-root");
    let _env = EnvVarGuard::set(WORKSPACE_ROOT_ENV, workspace_root.as_os_str());

    let error = match runtime_state_async().await {
        Ok(_) => panic!("runtime state should reject missing explicit workspace root"),
        Err(error) => error,
    };

    assert_eq!(error.code, "RUNTIME_INIT_FAILED");
    assert!(error.message.contains(WORKSPACE_ROOT_ENV));
    assert!(error
        .message
        .contains(&workspace_root.display().to_string()));
}

#[tokio::test]
async fn runtime_state_resolves_pending_permission_from_harness_broker() {
    let state = runtime_state_with_harness().await;
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let request = permission_request();
    let request_id = request.request_id;
    let request_session_id = request.session_id;

    let decision_task =
        tokio::spawn(async move { broker.decide(request, permission_context()).await });

    wait_for_pending_permission(&state, request_id).await;

    let payload = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Approve,
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(payload.status, "resolved");
    assert_eq!(decision_task.await.unwrap(), Decision::AllowOnce);
}

#[tokio::test]
async fn runtime_state_rejects_permission_resolution_for_wrong_conversation() {
    let state = runtime_state_with_harness().await;
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let request = permission_request();
    let request_id = request.request_id;
    let request_session_id = request.session_id;

    let decision_task =
        tokio::spawn(async move { broker.decide(request, permission_context()).await });

    wait_for_pending_permission(&state, request_id).await;

    let error = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: SessionId::new().to_string(),
            decision: PermissionDecision::Approve,
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error
        .message
        .contains("permission request does not belong to conversationId"));

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
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
async fn runtime_state_requires_window_subscription_before_permission_resolution() {
    let state = runtime_state_with_harness().await;
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let request = permission_request();
    let request_id = request.request_id;
    let request_session_id = request.session_id;
    let conversation_id = request_session_id.to_string();
    open_conversation_session(&state, request_session_id).await;

    let decision_task =
        tokio::spawn(async move { broker.decide(request, permission_context()).await });

    wait_for_pending_permission(&state, request_id).await;

    let error = resolve_permission_for_window_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: conversation_id.clone(),
            decision: PermissionDecision::Approve,
            request_id: request_id.to_string(),
        },
        "main".to_owned(),
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error
        .message
        .contains("permission request is not visible in this window"));

    let subscription = subscribe_conversation_events_for_window_with_runtime_state(
        SubscribeConversationEventsRequest {
            conversation_id: conversation_id.clone(),
            after_cursor: None,
        },
        "main".to_owned(),
        Arc::new(|_batch| Ok(())),
        &state,
    )
    .await
    .unwrap();

    let payload = resolve_permission_for_window_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id,
            decision: PermissionDecision::Approve,
            request_id: request_id.to_string(),
        },
        "main".to_owned(),
        &state,
    )
    .await
    .unwrap();

    assert_eq!(payload.status, "resolved");
    assert_eq!(decision_task.await.unwrap(), Decision::AllowOnce);

    let _ = unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: subscription.subscription_id,
        },
        "main".to_owned(),
        &state,
    )
    .await;
}

#[tokio::test]
async fn start_run_permission_mode_override_wins_over_saved_default() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "NeedsPermission".to_owned(),
                input: json!({ "command": "printf override-default" }),
            },
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::BypassPermissions,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        },
        &DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf()),
    )
    .expect("execution settings should save");
    let session_id = SessionId::new();

    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Run a command".to_owned(),
            permission_mode: Some(PermissionMode::Default),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let pending = wait_for_pending_permission_for_session(&state, session_id).await;

    assert_eq!(
        pending.context.run_id,
        Some(RunId::parse(&started.run_id).unwrap())
    );
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
async fn start_run_rejects_auto_without_runtime_support() {
    let state = runtime_state_with_harness().await;
    let error = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: SessionId::new().to_string(),
            prompt: "Run a command".to_owned(),
            permission_mode: Some(PermissionMode::Auto),
        },
        &state,
    )
    .await
    .expect_err("auto mode should be rejected without runtime support");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("auto"));
    assert!(error.message.contains("not available"));
}

#[tokio::test]
async fn start_run_bypass_permission_mode_finishes_without_pending_permission() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "NeedsPermission".to_owned(),
                input: json!({ "command": "printf bypass" }),
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
            prompt: "Run a command".to_owned(),
            permission_mode: Some(PermissionMode::BypassPermissions),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let payload = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let payload = list_activity_with_runtime_state(
                ListActivityRequest {
                    conversation_id: Some(session_id.to_string()),
                    run_id: Some(started.run_id.clone()),
                },
                &state,
            )
            .await
            .unwrap();
            if payload
                .events
                .iter()
                .any(|event| event.event_type == "run.ended")
            {
                break payload;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("bypass run should finish instead of waiting for permission");

    assert!(state.pending_permission_requests().is_empty());
    assert!(payload
        .events
        .iter()
        .any(|event| event.event_type == "permission.requested"));
    assert!(payload
        .events
        .iter()
        .any(|event| event.event_type == "permission.resolved"));
}

#[tokio::test]
async fn runtime_state_rejects_harness_and_stream_permission_runtime_from_different_brokers() {
    let workspace = unique_workspace("mismatched-broker");
    std::fs::create_dir_all(&workspace).unwrap();
    let harness_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let state_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let harness = Arc::new(
        Harness::builder()
            .with_options(test_harness_options(&workspace))
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_stream_permission_broker_arc(
                harness_runtime.broker(),
                harness_runtime.resolver_handle(),
            )
            .build()
            .await
            .expect("harness should build with stream permission runtime"),
    );

    let error = match DesktopRuntimeState::with_harness_and_stream_permission_runtime(
        harness,
        state_runtime,
    ) {
        Ok(_) => panic!("state should reject mismatched permission broker origins"),
        Err(error) => error,
    };

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");
}

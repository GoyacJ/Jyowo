#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

const VALID_PERMISSION_OPTION_ID: &str = "01HZ0000000000000000000002";

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
                model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
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
            option_id: deny_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap();
}

#[test]
fn resolve_permission_payload_requires_runtime_permission_broker() {
    let conversation_id = SessionId::new().to_string();
    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id: conversation_id.clone(),
        decision: PermissionDecision::Approve,
        option_id: VALID_PERMISSION_OPTION_ID.to_owned(),
        request_id: "01HZ0000000000000000000001".to_owned(),
        confirmation_text: None,
    })
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");

    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id: conversation_id.clone(),
        decision: PermissionDecision::Deny,
        option_id: VALID_PERMISSION_OPTION_ID.to_owned(),
        request_id: "01HZ0000000000000000000001".to_owned(),
        confirmation_text: None,
    })
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");

    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id,
        decision: PermissionDecision::Approve,
        option_id: VALID_PERMISSION_OPTION_ID.to_owned(),
        request_id: String::new(),
        confirmation_text: None,
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[test]
fn resolve_permission_payload_rejects_invalid_request_id_before_runtime() {
    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id: SessionId::new().to_string(),
        decision: PermissionDecision::Approve,
        option_id: VALID_PERMISSION_OPTION_ID.to_owned(),
        request_id: "permission-001".to_owned(),
        confirmation_text: None,
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[test]
fn resolve_permission_payload_rejects_noncanonical_request_id_before_runtime() {
    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id: SessionId::new().to_string(),
        decision: PermissionDecision::Approve,
        option_id: VALID_PERMISSION_OPTION_ID.to_owned(),
        request_id: "01hz0000000000000000000001".to_owned(),
        confirmation_text: None,
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[test]
fn resolve_permission_request_requires_option_id_when_deserialized() {
    let error = serde_json::from_value::<ResolvePermissionRequest>(json!({
        "conversationId": SessionId::new().to_string(),
        "decision": "approve",
        "requestId": "01HZ0000000000000000000001",
    }))
    .unwrap_err();

    assert!(error.to_string().contains("optionId"));
}

#[test]
fn resolve_permission_payload_rejects_invalid_option_id_before_runtime() {
    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id: SessionId::new().to_string(),
        decision: PermissionDecision::Approve,
        option_id: "not-an-option".to_owned(),
        request_id: "01HZ0000000000000000000001".to_owned(),
        confirmation_text: None,
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("optionId"));
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
            option_id: VALID_PERMISSION_OPTION_ID.to_owned(),
            request_id: "01HZ0000000000000000000001".to_owned(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "NOT_FOUND");
    assert!(error.message.contains("permission request not found"));
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

    let decision_task = tokio::spawn(async move {
        let ctx = permission_context_for_request(&request, None);
        broker.decide(request, ctx).await
    });

    let pending = wait_for_pending_permission(&state, request_id).await;

    let payload = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Approve,
            option_id: approve_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(payload.status, "resolved");
    assert_eq!(decision_task.await.unwrap(), Decision::AllowOnce);
}

#[tokio::test]
async fn runtime_state_rejects_invalid_permission_option_without_removing_pending_request() {
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

    let decision_task = tokio::spawn(async move {
        let ctx = permission_context_for_request(&request, None);
        broker.decide(request, ctx).await
    });

    let pending = wait_for_pending_permission(&state, request_id).await;

    let error = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Approve,
            option_id: PermissionOptionId::new().to_string(),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "PERMISSION_RESOLVE_FAILED");
    assert!(state
        .pending_permission_requests()
        .iter()
        .any(|pending| pending.request.request_id == request_id));

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Deny,
            option_id: deny_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(decision_task.await.unwrap(), Decision::DenyOnce);
}

#[tokio::test]
async fn runtime_state_rejects_permission_decision_kind_conflict_without_removing_pending_request()
{
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

    let decision_task = tokio::spawn(async move {
        let ctx = permission_context_for_request(&request, None);
        broker.decide(request, ctx).await
    });

    let pending = wait_for_pending_permission(&state, request_id).await;

    let error = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Deny,
            option_id: approve_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "PERMISSION_RESOLVE_FAILED");
    assert!(error.message.contains("does not match option"));
    assert!(state
        .pending_permission_requests()
        .iter()
        .any(|pending| pending.request.request_id == request_id));

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Approve,
            option_id: approve_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(decision_task.await.unwrap(), Decision::AllowOnce);
}

#[tokio::test]
async fn runtime_state_rejects_stale_permission_resolution_after_success() {
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

    let decision_task = tokio::spawn(async move {
        let ctx = permission_context_for_request(&request, None);
        broker.decide(request, ctx).await
    });

    let pending = wait_for_pending_permission(&state, request_id).await;
    let option_id = approve_permission_option_id(&pending);

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Approve,
            option_id: option_id.clone(),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(decision_task.await.unwrap(), Decision::AllowOnce);

    let error = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Approve,
            option_id,
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "NOT_FOUND");
    assert!(error.message.contains("permission request not found"));
}

#[tokio::test]
async fn runtime_state_waits_for_pending_permission_before_not_found() {
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

    let decision_task = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let ctx = permission_context_for_request(&request, None);
        broker.decide(request, ctx).await
    });

    let pending = wait_for_pending_permission(&state, request_id).await;

    let payload = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Approve,
            option_id: approve_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
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

    let decision_task = tokio::spawn(async move {
        let ctx = permission_context_for_request(&request, None);
        broker.decide(request, ctx).await
    });

    let pending = wait_for_pending_permission(&state, request_id).await;

    let error = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: SessionId::new().to_string(),
            decision: PermissionDecision::Approve,
            option_id: approve_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
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
            option_id: deny_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
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

    let decision_task = tokio::spawn(async move {
        let ctx = permission_context_for_request(&request, None);
        broker.decide(request, ctx).await
    });

    let pending = wait_for_pending_permission(&state, request_id).await;

    let error = resolve_permission_for_window_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: conversation_id.clone(),
            decision: PermissionDecision::Approve,
            option_id: approve_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
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
            option_id: approve_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
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
async fn list_activity_with_runtime_state_hides_pending_permission_without_durable_request_event() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let request = permission_request();
    let request_id = request.request_id;
    let request_session_id = request.session_id;
    let conversation_id = session_id.to_string();

    let decision_task = tokio::spawn(async move {
        let ctx = permission_context_for_request(&request, None);
        broker.decide(request, ctx).await
    });

    let pending = wait_for_pending_permission(&state, request_id).await;

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(conversation_id),
            run_id: None,
        },
        &state,
    )
    .await
    .unwrap();

    assert!(payload.events.is_empty());

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Deny,
            option_id: deny_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(decision_task.await.unwrap(), Decision::DenyOnce);
}

#[tokio::test]
async fn list_activity_with_runtime_state_reads_journaled_permission_requests_by_run_id() {
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
    let run_id = RunId::parse(&started.run_id).expect("run id should be canonical");
    let pending = wait_for_pending_permission_for_session(&state, session_id).await;
    let request_id = pending.request.request_id;

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
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
    assert_eq!(pending.context.run_id, Some(run_id));

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Deny,
            option_id: deny_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap();
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
        None,
    )
    .expect("execution settings should save");
    let session_id = SessionId::new();

    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
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
            option_id: deny_permission_option_id(&pending),
            request_id: pending.request.request_id.to_string(),
            confirmation_text: None,
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
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
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
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
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
async fn list_activity_with_runtime_state_includes_permission_auto_resolved() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let tool_use_id = ToolUseId::new();
    let request_id = RequestId::new();
    open_conversation_session(&state, session_id).await;
    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::RunStarted(test_run_started_event(session_id, run_id)),
                Event::PermissionRequested(PermissionRequestedEvent {
                    at: now(),
                    causation_id: EventId::new(),
                    fingerprint: None,
                    interactivity: InteractivityLevel::FullyInteractive,
                    auto_resolved: true,
                    actor_source: PermissionActorSource::ParentRun,
                    action_plan_hash: Default::default(),
                    review: Default::default(),
                    effective_mode: Default::default(),
                    sandbox_policy: Default::default(),
                    presented_options: vec![PermissionDecisionOption {
                        option_id: PermissionOptionId::new(),
                        decision: Decision::AllowOnce,
                        scope: DecisionScope::Any,
                        lifetime: DecisionLifetime::Once,
                        matcher_summary: DecisionMatcherSummary {
                            kind: DecisionMatcherKind::Any,
                            label: "allow once".to_owned(),
                        },
                        label: "Allow once".to_owned(),
                        requires_confirmation: false,
                        action_plan_hash: ActionPlanHash::default(),
                        fingerprint: None,
                    }],
                    request_id,
                    run_id,
                    scope_hint: DecisionScope::ToolName("shell".to_owned()),
                    session_id,
                    severity: Severity::Low,
                    subject: PermissionSubject::CommandExec {
                        argv: vec!["pwd".to_owned()],
                        command: "pwd".to_owned(),
                        cwd: None,
                        fingerprint: None,
                    },
                    tenant_id: TenantId::SINGLE,
                    tool_name: "shell".to_owned(),
                    tool_use_id,
                }),
            ],
        )
        .await
        .expect("activity events should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("activity should load");
    let permission_event = payload
        .events
        .iter()
        .find(|event| event.event_type == "permission.requested")
        .expect("permission request should be projected");

    assert_eq!(permission_event.payload["autoResolved"], true);
    assert_eq!(
        permission_event.payload["reason"],
        "已按本次授权模式自动允许。"
    );
}

#[tokio::test]
async fn list_activity_with_runtime_state_does_not_expose_other_conversation_pending_permissions() {
    let state = runtime_state_with_harness().await;
    let other_session_id = SessionId::new();
    open_conversation_session(&state, other_session_id).await;
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let request = permission_request();
    let request_id = request.request_id;
    let request_session_id = request.session_id;

    let decision_task = tokio::spawn(async move {
        let ctx = permission_context_for_request(&request, None);
        broker.decide(request, ctx).await
    });

    let pending = wait_for_pending_permission(&state, request_id).await;

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(other_session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .unwrap();

    assert!(payload.events.is_empty());

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Deny,
            option_id: deny_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(decision_task.await.unwrap(), Decision::DenyOnce);
}

#[tokio::test]
async fn list_activity_with_runtime_state_rejects_conflicting_conversation_and_run_filters() {
    let state = runtime_state_with_harness().await;
    let other_session_id = SessionId::new();
    open_conversation_session(&state, other_session_id).await;
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let request = permission_request();
    let request_id = request.request_id;
    let request_session_id = request.session_id;
    let run_id = RunId::new();
    let run_id_string = run_id.to_string();

    let decision_task = tokio::spawn(async move {
        let ctx = permission_context_for_request(&request, Some(run_id));
        broker.decide(request, ctx).await
    });

    let pending = wait_for_pending_permission(&state, request_id).await;

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(other_session_id.to_string()),
            run_id: Some(run_id_string),
        },
        &state,
    )
    .await
    .unwrap();

    assert!(payload.events.is_empty());

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Deny,
            option_id: deny_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(decision_task.await.unwrap(), Decision::DenyOnce);
}

#[tokio::test]
async fn runtime_state_rejects_harness_and_stream_permission_runtime_from_different_resolvers() {
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
        Ok(_) => panic!("state should reject mismatched permission resolver origins"),
        Err(error) => error,
    };

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");
}

#[test]
fn list_activity_payload_returns_empty_typed_event_list() {
    let payload = list_activity_payload(ListActivityRequest {
        conversation_id: Some("conversation-001".to_owned()),
        run_id: None,
    })
    .unwrap();

    assert!(payload.events.is_empty());

    let error = list_activity_payload(ListActivityRequest {
        conversation_id: Some(String::new()),
        run_id: None,
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

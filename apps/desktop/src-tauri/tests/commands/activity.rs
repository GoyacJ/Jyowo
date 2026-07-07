#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

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
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
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
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
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
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
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
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
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
async fn list_activity_with_runtime_state_requires_conversation_id() {
    let state = runtime_state_with_harness().await;

    let error = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: None,
            run_id: Some(RunId::new().to_string()),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn list_activity_with_runtime_state_reads_durable_run_events() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::MessageStart {
            message_id: "message-usage".to_owned(),
            usage: UsageSnapshot {
                input_tokens: 11,
                output_tokens: 0,
                cache_read_tokens: 3,
                cache_write_tokens: 5,
                cost_micros: 0,
                tool_calls: 0,
            },
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Done".to_owned()),
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: None,
            usage_delta: UsageSnapshot {
                input_tokens: 0,
                output_tokens: 7,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                cost_micros: 260,
                tool_calls: 0,
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
            .any(|event| event.event_type == "assistant.completed")
        {
            let run_started = payload
                .events
                .iter()
                .find(|event| event.event_type == "run.started")
                .expect("activity should include run started event");
            assert_eq!(run_started.payload["permissionMode"], json!("default"));
            let run_ended = payload
                .events
                .iter()
                .find(|event| event.event_type == "run.ended")
                .expect("activity should include run ended event");
            assert_eq!(run_ended.payload["usage"]["inputTokens"], json!(11));
            assert_eq!(run_ended.payload["usage"]["outputTokens"], json!(7));
            assert_eq!(run_ended.payload["usage"]["cacheReadTokens"], json!(3));
            assert_eq!(run_ended.payload["usage"]["cacheWriteTokens"], json!(5));
            assert_eq!(run_ended.payload["usage"]["costMicros"], json!(260));
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("activity should include durable run events");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn list_activity_with_runtime_state_maps_artifact_lifecycle_events() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
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
                Event::ArtifactCreated(ArtifactCreatedEvent {
                    revision_id: ArtifactRevisionId::new(),
                    artifact_id: "artifact-runtime-notes".to_owned(),
                    at: now(),
                    blob_ref: None,
                    content_hash: None,
                    kind: "markdown javascript:alert(1)".to_owned(),
                    preview: Some(
                        "Blob:.jyowo/runtime/blobs/blob-001 log/tmp/provider-output".to_owned(),
                    ),
                    run_id,
                    session_id,
                    source: ArtifactSource::Assistant,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: ArtifactStatus::Running,
                    title: "Runtime notes https://provider.example/artifact".to_owned(),
                }),
                Event::ArtifactUpdated(ArtifactUpdatedEvent {
                    revision_id: ArtifactRevisionId::new(),
                    artifact_id: "artifact-runtime-notes".to_owned(),
                    at: now(),
                    blob_ref: None,
                    content_hash: None,
                    kind: Some("markdown /tmp/provider-output".to_owned()),
                    preview: Some(
                        "Updated 路径：.jyowo/runtime/blobs/blob-002 blob:null/provider".to_owned(),
                    ),
                    run_id,
                    session_id,
                    source: ArtifactSource::Assistant,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: Some(ArtifactStatus::Ready),
                    title: Some("Updated链接https://provider.example/updated".to_owned()),
                }),
                Event::ArtifactCreated(ArtifactCreatedEvent {
                    revision_id: ArtifactRevisionId::new(),
                    artifact_id: "artifact-wrong-session".to_owned(),
                    at: now(),
                    blob_ref: None,
                    content_hash: None,
                    kind: "markdown".to_owned(),
                    preview: Some("Wrong session".to_owned()),
                    run_id,
                    session_id: SessionId::new(),
                    source: ArtifactSource::Assistant,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: ArtifactStatus::Ready,
                    title: "Wrong session".to_owned(),
                }),
            ],
        )
        .await
        .expect("artifact event should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("activity should load");

    assert!(!payload
        .events
        .iter()
        .any(|event| event.payload["artifactId"] == json!("artifact-wrong-session")));
    let artifact_created = payload
        .events
        .iter()
        .find(|event| event.event_type == "artifact.created")
        .expect("activity should include artifact lifecycle event");
    assert_eq!(artifact_created.source, "engine");
    assert_eq!(artifact_created.visibility, "public");
    assert_eq!(
        artifact_created.payload["artifactId"],
        json!("artifact-runtime-notes")
    );
    assert_eq!(artifact_created.payload["status"], json!("running"));
    assert_eq!(
        artifact_created.payload["kind"],
        json!("markdown [REDACTED]")
    );
    assert_eq!(
        artifact_created.payload["title"],
        json!("Runtime notes [REDACTED]")
    );
    assert_eq!(
        artifact_created.payload["summary"],
        json!("[REDACTED] log[REDACTED]")
    );

    let artifact_updated = payload
        .events
        .iter()
        .find(|event| event.event_type == "artifact.updated")
        .expect("activity should include artifact update event");
    assert_eq!(
        artifact_updated.payload["artifactId"],
        json!("artifact-runtime-notes")
    );
    assert_eq!(artifact_updated.payload["status"], json!("ready"));
    assert_eq!(
        artifact_updated.payload["kind"],
        json!("markdown [REDACTED]")
    );
    assert_eq!(
        artifact_updated.payload["title"],
        json!("Updated链接[REDACTED]")
    );
    assert_eq!(
        artifact_updated.payload["summary"],
        json!("Updated 路径：[REDACTED] [REDACTED]")
    );
    let serialized = serde_json::to_string(&payload).unwrap();
    assert!(!serialized.contains("provider.example"));
    assert!(!serialized.contains(".jyowo/runtime/blobs"));
    assert!(!serialized.contains("/tmp/provider-output"));
    assert!(!serialized.contains("blob:null"));
    assert!(!serialized.contains("javascript:"));
}

#[tokio::test]
async fn list_activity_with_runtime_state_maps_assistant_interaction_events() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let review_request_id = RequestId::new();
    let clarification_request_id = RequestId::new();
    let notice_id = RequestId::new();
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
                Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                    at: now(),
                    delta: DeltaChunk::ReasoningSummary(ReasoningSummaryChunk {
                        provider_id: "test".to_owned(),
                        provider_native: None,
                        text: "Checked https://provider.example/image，路径：.jyowo/runtime/blobs/blob-001 log/tmp/provider-output"
                            .to_owned(),
                    }),
                    message_id: MessageId::new(),
                    run_id,
                }),
                Event::AssistantReviewRequested(AssistantReviewRequestedEvent {
                    run_id,
                    request_id: review_request_id,
                    title: UiSafeText::from_redacted_display(
                        "Review https://provider.example/review",
                        &DefaultRedactor::default(),
                    ),
                    body: Some(UiSafeText::from_redacted_display(
                        "Approve blob:.jyowo/runtime/blobs/blob-001?",
                        &DefaultRedactor::default(),
                    )),
                    at: now(),
                }),
                Event::AssistantClarificationRequested(AssistantClarificationRequestedEvent {
                    run_id,
                    request_id: clarification_request_id,
                    prompt: UiSafeText::from_redacted_display(
                        "Which size链接https://provider.example/prompt?",
                        &DefaultRedactor::default(),
                    ),
                    at: now(),
                }),
                Event::AssistantNotice(AssistantNoticeEvent {
                    run_id,
                    notice_id,
                    body: UiSafeText::from_redacted_display(
                        "Generation queued at 路径：.jyowo/runtime/blobs/blob-002.",
                        &DefaultRedactor::default(),
                    ),
                    code: None,
                    at: now(),
                }),
            ],
        )
        .await
        .expect("assistant interaction events should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("activity should load");

    let event_types = payload
        .events
        .iter()
        .map(|event| event.event_type)
        .collect::<Vec<_>>();
    assert!(event_types.contains(&"assistant.review.requested"));
    assert!(event_types.contains(&"assistant.clarification.requested"));
    assert!(event_types.contains(&"assistant.notice"));
    let review = payload
        .events
        .iter()
        .find(|event| event.event_type == "assistant.review.requested")
        .expect("activity should include review");
    assert_eq!(review.payload["title"], json!("Review http[REDACTED]"));
    assert!(review.payload["body"]
        .as_str()
        .is_some_and(|body| body.contains("[REDACTED]")));
    let clarification = payload
        .events
        .iter()
        .find(|event| event.event_type == "assistant.clarification.requested")
        .expect("activity should include clarification");
    assert!(clarification.payload["prompt"]
        .as_str()
        .is_some_and(|prompt| prompt.contains("[REDACTED]")));
    let notice = payload
        .events
        .iter()
        .find(|event| event.event_type == "assistant.notice")
        .expect("activity should include notice");
    assert!(notice.payload["body"]
        .as_str()
        .is_some_and(|body| body.contains("[REDACTED]")));
    let serialized = serde_json::to_string(&payload).unwrap();
    assert!(!serialized.contains("provider.example"));
    assert!(!serialized.contains(".jyowo/runtime/blobs"));
    assert!(!serialized.contains("/tmp/provider-output"));

    let replay = get_replay_timeline_with_runtime_state(
        ReplayTimelineRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("replay should load");
    let thinking = replay
        .events
        .iter()
        .find(|event| event.event_type == "assistant.thinking.delta")
        .expect("replay should include safe reasoning summary");
    assert_eq!(
        thinking.payload["safeSummaryDelta"],
        json!("Checked [REDACTED]，路径：[REDACTED] log[REDACTED]")
    );
    let replay_serialized = serde_json::to_string(&replay).unwrap();
    assert!(!replay_serialized.contains("provider.example"));
    assert!(!replay_serialized.contains(".jyowo/runtime/blobs"));
    assert!(!replay_serialized.contains("/tmp/provider-output"));
}

#[tokio::test]
async fn list_activity_with_runtime_state_filters_run_events_by_started_session() {
    let state = runtime_state_with_harness().await;
    let requested_session_id = SessionId::new();
    let other_session_id = SessionId::new();
    let requested_run_id = RunId::new();
    let other_run_id = RunId::new();
    open_conversation_session(&state, requested_session_id).await;
    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            requested_session_id,
            &[
                Event::RunStarted(test_run_started_event(other_session_id, other_run_id)),
                Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                    at: now(),
                    delta: DeltaChunk::Text("Wrong session answer".to_owned()),
                    message_id: MessageId::new(),
                    run_id: other_run_id,
                }),
                Event::RunStarted(test_run_started_event(
                    requested_session_id,
                    requested_run_id,
                )),
                Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                    at: now(),
                    delta: DeltaChunk::Text("Requested session answer".to_owned()),
                    message_id: MessageId::new(),
                    run_id: requested_run_id,
                }),
            ],
        )
        .await
        .expect("activity events should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(requested_session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("activity should load");
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(serialized.contains("Requested session answer"));
    assert!(!serialized.contains("Wrong session answer"));
    assert!(!payload
        .events
        .iter()
        .any(|event| event.run_id == other_run_id.to_string()));
}

#[tokio::test]
async fn list_activity_with_runtime_state_filters_tool_and_permission_events_by_started_session() {
    let state = runtime_state_with_harness().await;
    let requested_session_id = SessionId::new();
    let other_session_id = SessionId::new();
    let requested_run_id = RunId::new();
    let other_run_id = RunId::new();
    let requested_tool_use_id = ToolUseId::new();
    let other_tool_use_id = ToolUseId::new();
    let requested_request_id = RequestId::new();
    let other_request_id = RequestId::new();
    open_conversation_session(&state, requested_session_id).await;
    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            requested_session_id,
            &[
                Event::RunStarted(test_run_started_event(other_session_id, other_run_id)),
                Event::ToolUseRequested(test_tool_use_requested_event(
                    other_run_id,
                    other_tool_use_id,
                    "wrong-session-tool",
                )),
                Event::PermissionRequested(test_permission_requested_event(
                    other_session_id,
                    other_run_id,
                    other_tool_use_id,
                    other_request_id,
                    "wrong-session-permission",
                )),
                Event::RunStarted(test_run_started_event(
                    requested_session_id,
                    requested_run_id,
                )),
                Event::ToolUseRequested(test_tool_use_requested_event(
                    requested_run_id,
                    requested_tool_use_id,
                    "requested-tool",
                )),
                Event::PermissionRequested(test_permission_requested_event(
                    requested_session_id,
                    requested_run_id,
                    requested_tool_use_id,
                    requested_request_id,
                    "requested-permission",
                )),
            ],
        )
        .await
        .expect("activity events should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(requested_session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("activity should load");
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(serialized.contains("requested-tool"));
    assert!(serialized.contains("requested-permission"));
    assert!(serialized.contains(&requested_request_id.to_string()));
    assert!(!serialized.contains("wrong-session-tool"));
    assert!(!serialized.contains("wrong-session-permission"));
    assert!(!serialized.contains(&other_request_id.to_string()));
    assert!(!payload
        .events
        .iter()
        .any(|event| event.run_id == other_run_id.to_string()));
}

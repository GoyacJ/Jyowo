#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

async fn list_activity_with_runtime_state_does_not_expose_thinking_deltas() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Thinking(ThinkingDelta {
                text: Some("private chain of thought".to_owned()),
                provider_native: Some(json!({ "thinking": "provider native secret" })),
                signature: Some("signature-secret".to_owned()),
            }),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 1,
            delta: ContentDelta::Text("Visible answer".to_owned()),
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
            prompt: "Think privately".to_owned(),
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
            let serialized = serde_json::to_string(&payload).unwrap();
            assert!(payload.events.iter().any(|event| {
                event.event_type == "assistant.delta"
                    && event.payload["text"] == json!("Visible answer")
                    && event.payload["messageId"].as_str().is_some()
            }));
            assert!(!serialized.contains("private chain of thought"));
            assert!(!serialized.contains("provider native secret"));
            assert!(!serialized.contains("signature-secret"));
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("activity should include completed assistant event");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn list_activity_with_runtime_state_redacts_private_paths_from_assistant_deltas() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text(
                "Read /Users/alice/.ssh/config 链接https://provider.example/signed log/tmp/provider-output blob:.jyowo/runtime/blobs/blob-001"
                    .to_owned(),
            ),
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
            prompt: "Summarize path".to_owned(),
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

        if let Some(delta) = payload
            .events
            .iter()
            .find(|event| event.event_type == "assistant.delta")
        {
            let serialized = serde_json::to_string(&payload).unwrap();
            assert!(!serialized.contains("/Users/alice/.ssh/config"));
            assert!(!serialized.contains("provider.example"));
            assert!(!serialized.contains("/tmp/provider-output"));
            assert!(!serialized.contains(".jyowo/runtime/blobs"));
            assert_eq!(
                delta.payload["text"],
                json!("Read [REDACTED] 链接[REDACTED] log[REDACTED] [REDACTED]")
            );
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("activity should include assistant delta event");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn list_activity_with_runtime_state_redacts_unsafe_artifact_media_mime_type() {
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
                    artifact_id: "artifact-image".to_owned(),
                    at: now(),
                    blob_ref: Some(harness_contracts::BlobRef {
                        id: harness_contracts::BlobId::new(),
                        size: 42,
                        content_hash: [7; 32],
                        content_type: Some(
                            "image/png /tmp/provider-output https://provider.example/blob"
                                .to_owned(),
                        ),
                    }),
                    content_hash: Some(vec![9; 32]),
                    kind: "image".to_owned(),
                    preview: Some("Generated image".to_owned()),
                    run_id,
                    session_id,
                    source: ArtifactSource::Tool,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: ArtifactStatus::Ready,
                    title: "Generated image".to_owned(),
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
    let artifact_created = payload
        .events
        .iter()
        .find(|event| event.event_type == "artifact.created")
        .expect("activity should include artifact lifecycle event");
    let serialized = serde_json::to_string(&payload).unwrap();

    assert_eq!(artifact_created.payload["media"]["kind"], json!("image"));
    assert_eq!(
        artifact_created.payload["media"]["mimeType"],
        json!("image/png")
    );
    assert_eq!(artifact_created.payload["media"]["sizeBytes"], json!(42));
    assert!(!serialized.contains("/tmp/provider-output"));
    assert!(!serialized.contains("provider.example"));
}

#[tokio::test]
async fn list_activity_with_runtime_state_does_not_project_secret_like_artifact_media_mime_token() {
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
                    artifact_id: "artifact-video".to_owned(),
                    at: now(),
                    blob_ref: Some(harness_contracts::BlobRef {
                        id: harness_contracts::BlobId::new(),
                        size: 42,
                        content_hash: [7; 32],
                        content_type: Some(
                            "video/sk-abcdefghijklmnopqrstuvwxyz0123456789".to_owned(),
                        ),
                    }),
                    content_hash: Some(vec![9; 32]),
                    kind: "video".to_owned(),
                    preview: Some("Generated video".to_owned()),
                    run_id,
                    session_id,
                    source: ArtifactSource::Tool,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: ArtifactStatus::Ready,
                    title: "Generated video".to_owned(),
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
    let artifact_created = payload
        .events
        .iter()
        .find(|event| event.event_type == "artifact.created")
        .expect("activity should include artifact lifecycle event");
    let serialized = serde_json::to_string(&payload).unwrap();

    assert_eq!(artifact_created.payload["media"]["kind"], json!("video"));
    assert_eq!(
        artifact_created.payload["media"]["mimeType"],
        json!("video/mp4")
    );
    assert!(!serialized.contains("sk-abcdefghijklmnopqrstuvwxyz0123456789"));
}

#[tokio::test]
async fn list_activity_with_runtime_state_redacts_permission_decision_scope_values() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let tool_use_id = ToolUseId::new();
    let request_id = RequestId::new();
    let secret_scope = "secret-internal-tool-name";
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
                    auto_resolved: false,
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
                    scope_hint: DecisionScope::ToolName(secret_scope.to_owned()),
                    session_id,
                    severity: Severity::Low,
                    subject: PermissionSubject::CommandExec {
                        argv: vec!["pwd".to_owned()],
                        command: "pwd".to_owned(),
                        cwd: None,
                        fingerprint: None,
                    },
                    tenant_id: TenantId::SINGLE,
                    tool_name: "pwd".to_owned(),
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
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(!serialized.contains(secret_scope));
    assert!(serialized.contains("\"decisionScope\":\"this tool\""));
    assert!(serialized.contains("\"target\":\"pwd\""));
}

#[tokio::test]
async fn list_activity_with_runtime_state_redacts_file_permission_targets() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    open_conversation_session(&state, session_id).await;

    let file_write_secret = "sk-abcdefghijklmnopqrstuvwxyz";
    let file_delete_data_url = "data:text,secret";
    let file_delete_script_url = "javascript:alert(1)";
    let permission_event =
        |request_id: RequestId, tool_use_id: ToolUseId, subject: PermissionSubject| {
            Event::PermissionRequested(PermissionRequestedEvent {
                at: now(),
                causation_id: EventId::new(),
                fingerprint: None,
                interactivity: InteractivityLevel::FullyInteractive,
                auto_resolved: false,
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
                scope_hint: DecisionScope::PathPrefix(PathBuf::from("workspace")),
                session_id,
                severity: Severity::Medium,
                subject,
                tenant_id: TenantId::SINGLE,
                tool_name: "file-tool".to_owned(),
                tool_use_id,
            })
        };

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::RunStarted(test_run_started_event(session_id, run_id)),
                permission_event(
                    RequestId::new(),
                    ToolUseId::new(),
                    PermissionSubject::FileWrite {
                        path: PathBuf::from(format!("workspace/{file_write_secret}")),
                        bytes_preview: b"secret".to_vec(),
                    },
                ),
                permission_event(
                    RequestId::new(),
                    ToolUseId::new(),
                    PermissionSubject::FileDelete {
                        path: PathBuf::from(format!("workspace/{file_delete_data_url}")),
                    },
                ),
                permission_event(
                    RequestId::new(),
                    ToolUseId::new(),
                    PermissionSubject::FileDelete {
                        path: PathBuf::from(format!("workspace/{file_delete_script_url}")),
                    },
                ),
            ],
        )
        .await
        .expect("activity events should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("activity should load");
    let serialized = serde_json::to_string(&payload).unwrap();
    let targets = payload
        .events
        .iter()
        .filter(|event| event.event_type == "permission.requested")
        .map(|event| event.payload["target"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();

    assert_eq!(targets.len(), 3);
    assert!(targets.iter().all(|target| target.contains("[REDACTED]")));
    assert!(!serialized.contains(file_write_secret));
    assert!(!serialized.contains(file_delete_data_url));
    assert!(!serialized.contains(file_delete_script_url));
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
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
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

#[tokio::test]
async fn list_activity_with_runtime_state_withholds_tool_failure_messages() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let tool_use_id = ToolUseId::new();
    let raw_error = "failed with AKIAIOSFODNN7EXAMPLE";
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
                Event::ToolUseRequested(test_tool_use_requested_event(
                    run_id,
                    tool_use_id,
                    "ReadFile",
                )),
                Event::ToolUseFailed(ToolUseFailedEvent {
                    at: now(),
                    error: ToolErrorPayload {
                        code: "execution".to_owned(),
                        message: raw_error.to_owned(),
                        retriable: false,
                    },
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
    let serialized = serde_json::to_string(&payload).unwrap();
    let failed = payload
        .events
        .iter()
        .find(|event| event.event_type == "tool.failed")
        .expect("tool failure should be projected");

    assert!(!serialized.contains(raw_error));
    assert_eq!(
        failed.payload["message"],
        json!("Tool error withheld from conversation timeline.")
    );
}

#[tokio::test]
async fn public_runtime_event_views_redact_unsafe_tool_display_labels() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let message_id = MessageId::new();
    let tool_use_id = ToolUseId::new();
    let request_id = RequestId::new();
    let unsafe_tool_name =
        "UnsafeTool https://provider.example/.jyowo /Users/alice/private data:text/plain,secret";
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
                Event::ToolUseRequested(test_tool_use_requested_event(
                    run_id,
                    tool_use_id,
                    unsafe_tool_name,
                )),
                Event::PermissionRequested(PermissionRequestedEvent {
                    request_id,
                    run_id,
                    session_id,
                    tenant_id: TenantId::SINGLE,
                    tool_use_id,
                    tool_name: unsafe_tool_name.to_owned(),
                    subject: PermissionSubject::ToolInvocation {
                        tool: unsafe_tool_name.to_owned(),
                        input: json!({}),
                    },
                    severity: Severity::Medium,
                    scope_hint: DecisionScope::ToolName(unsafe_tool_name.to_owned()),
                    fingerprint: None,
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
                    interactivity: InteractivityLevel::FullyInteractive,
                    auto_resolved: false,
                    actor_source: PermissionActorSource::ParentRun,
                    action_plan_hash: Default::default(),
                    review: Default::default(),
                    effective_mode: Default::default(),
                    sandbox_policy: Default::default(),
                    causation_id: EventId::new(),
                    at: now(),
                }),
                Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
                    run_id,
                    message_id,
                    content: MessageContent::Text("Tool requested.".to_owned()),
                    tool_uses: vec![ToolUseSummary {
                        tool_use_id,
                        tool_name: unsafe_tool_name.to_owned(),
                    }],
                    usage: UsageSnapshot::default(),
                    pricing_snapshot_id: None,
                    stop_reason: StopReason::ToolUse,
                    at: now(),
                }),
            ],
        )
        .await
        .expect("activity events should append");

    let activity = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("activity should load");
    let replay = get_replay_timeline_with_runtime_state(
        ReplayTimelineRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("replay should load");
    let timeline = page_conversation_timeline_with_runtime_state(
        PageConversationTimelineRequest {
            conversation_id: session_id.to_string(),
            after_cursor: None,
            limit: Some(20),
        },
        &state,
    )
    .await
    .expect("timeline should load");

    for serialized in [
        serde_json::to_string(&activity).unwrap(),
        serde_json::to_string(&replay).unwrap(),
        serde_json::to_string(&timeline).unwrap(),
    ] {
        assert!(!serialized.contains("provider.example"));
        assert!(!serialized.contains(".jyowo"));
        assert!(!serialized.contains("/Users/alice/private"));
        assert!(!serialized.contains("data:text/plain"));
    }
}

#[tokio::test]
async fn list_activity_with_runtime_state_withholds_failed_run_end_reason() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Error(
        ModelError::InvalidRequest("provider failed".to_owned()),
    )])
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
            prompt: "Trigger a provider failure".to_owned(),
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

        if let Some(run_ended) = payload
            .events
            .iter()
            .find(|event| event.event_type == "run.ended")
        {
            assert_eq!(
                run_ended.payload["reason"],
                json!("Run error withheld from conversation timeline.")
            );
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("activity should include failed run ended event");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn list_activity_with_runtime_state_redacts_private_paths_from_engine_failed_events() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let private_path = "/Users/alice/workspace/secret.txt";
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
                Event::EngineFailed(EngineFailedEvent {
                    at: now(),
                    error: EngineError::Message(format!("failed to read {private_path}")),
                    run_id: Some(run_id),
                    session_id: Some(session_id),
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
    let serialized = serde_json::to_string(&payload).unwrap();
    let failed = payload
        .events
        .iter()
        .find(|event| event.event_type == "engine.failed")
        .expect("engine failure should be projected");

    assert!(!serialized.contains(private_path));
    assert_eq!(
        failed.payload["message"],
        json!("Engine error withheld from conversation timeline.")
    );
}

#[tokio::test]
async fn list_activity_with_runtime_state_withholds_engine_failed_raw_message() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let raw_error = "provider error Authorization: Bearer secret-token path=/Users/alice/private";
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
                Event::EngineFailed(EngineFailedEvent {
                    at: now(),
                    error: EngineError::Message(raw_error.to_owned()),
                    run_id: Some(run_id),
                    session_id: Some(session_id),
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
    let serialized = serde_json::to_string(&payload).unwrap();
    let failed = payload
        .events
        .iter()
        .find(|event| event.event_type == "engine.failed")
        .expect("engine failure should be projected");

    assert!(!serialized.contains(raw_error));
    assert!(!serialized.contains("secret-token"));
    assert!(!serialized.contains("/Users/alice/private"));
    assert_eq!(
        failed.payload["message"],
        json!("Engine error withheld from conversation timeline.")
    );
}

#[tokio::test]
async fn list_activity_with_runtime_state_redacts_pending_permission_display_text() {
    let secret_command =
        "git push https://ghp_abcdefghijklmnopqrstuvwxyz0123456789@github.com/org/repo";
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "NeedsPermission".to_owned(),
                input: json!({ "command": secret_command }),
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
    let value = serde_json::to_string(&payload).unwrap();

    assert!(!value.contains("ghp_abcdefghijklmnopqrstuvwxyz0123456789"));
    assert!(!value.contains(secret_command));
    assert!(value.contains("\"target\":\"git\""));

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

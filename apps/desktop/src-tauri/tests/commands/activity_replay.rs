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

    let decision_task =
        tokio::spawn(async move { broker.decide(request, permission_context()).await });

    wait_for_pending_permission(&state, request_id).await;

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
            request_id: request_id.to_string(),
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
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
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
async fn get_replay_timeline_with_runtime_state_does_not_expose_raw_thinking_delta_text() {
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
    let request = ReplayTimelineRequest {
        conversation_id: Some(session_id.to_string()),
        run_id: Some(started.run_id),
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    loop {
        let payload = get_replay_timeline_with_runtime_state(request.clone(), &state)
            .await
            .unwrap();

        if payload
            .events
            .iter()
            .any(|event| event.event_type == "assistant.completed")
        {
            let serialized = serde_json::to_string(&payload).unwrap();
            let thinking = payload
                .events
                .iter()
                .find(|event| event.event_type == "assistant.thinking.delta")
                .expect("thinking status event should be projected");
            assert_eq!(thinking.payload["status"], json!("running"));
            assert!(thinking.payload.get("text").is_none());
            assert!(thinking.payload.get("providerNative").is_none());
            assert!(thinking.payload.get("signature").is_none());
            assert!(!serialized.contains("private chain of thought"));
            assert!(!serialized.contains("provider native secret"));
            assert!(!serialized.contains("signature-secret"));
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("replay should include completed assistant event");
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
                    title: UiSafeText::from_trusted_redacted(
                        "Review https://provider.example/review",
                    ),
                    body: Some(UiSafeText::from_trusted_redacted(
                        "Approve blob:.jyowo/runtime/blobs/blob-001?",
                    )),
                    at: now(),
                }),
                Event::AssistantClarificationRequested(AssistantClarificationRequestedEvent {
                    run_id,
                    request_id: clarification_request_id,
                    prompt: UiSafeText::from_trusted_redacted(
                        "Which size链接https://provider.example/prompt?",
                    ),
                    at: now(),
                }),
                Event::AssistantNotice(AssistantNoticeEvent {
                    run_id,
                    notice_id,
                    body: UiSafeText::from_trusted_redacted(
                        "Generation queued at 路径：.jyowo/runtime/blobs/blob-002.",
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
async fn page_conversation_timeline_with_runtime_state_accepts_assistant_interaction_events() {
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
                Event::AssistantReviewRequested(AssistantReviewRequestedEvent {
                    run_id,
                    request_id: RequestId::new(),
                    title: UiSafeText::from_trusted_redacted(
                        "Review Authorization: Bearer synthetic-token",
                    ),
                    body: Some(UiSafeText::from_trusted_redacted(
                        "Approve /Users/example/private?",
                    )),
                    at: now(),
                }),
                Event::AssistantClarificationRequested(AssistantClarificationRequestedEvent {
                    run_id,
                    request_id: RequestId::new(),
                    prompt: UiSafeText::from_trusted_redacted("Which size uses sk-synthetic?"),
                    at: now(),
                }),
                Event::AssistantNotice(AssistantNoticeEvent {
                    run_id,
                    notice_id: RequestId::new(),
                    body: UiSafeText::from_trusted_redacted(
                        "Generation queued from /home/example/private.",
                    ),
                    code: None,
                    at: now(),
                }),
            ],
        )
        .await
        .expect("assistant interaction events should append");

    let page = page_conversation_timeline_with_runtime_state(
        PageConversationTimelineRequest {
            conversation_id: session_id.to_string(),
            after_cursor: None,
            limit: None,
        },
        &state,
    )
    .await
    .expect("timeline page should load");

    let event_types = page
        .events
        .iter()
        .map(|event| event.event_type)
        .collect::<Vec<_>>();
    assert!(event_types.contains(&"assistant.review.requested"));
    assert!(event_types.contains(&"assistant.clarification.requested"));
    assert!(event_types.contains(&"assistant.notice"));
    let review = page
        .events
        .iter()
        .find(|event| event.event_type == "assistant.review.requested")
        .expect("review event should be mapped");
    assert_eq!(
        review.payload["title"],
        json!("Review [REDACTED] [REDACTED] [REDACTED]")
    );
    assert_eq!(review.payload["body"], json!("Approve [REDACTED]"));
    let clarification = page
        .events
        .iter()
        .find(|event| event.event_type == "assistant.clarification.requested")
        .expect("clarification event should be mapped");
    assert_eq!(
        clarification.payload["prompt"],
        json!("Which size uses [REDACTED]")
    );
    let notice = page
        .events
        .iter()
        .find(|event| event.event_type == "assistant.notice")
        .expect("notice event should be mapped");
    assert_eq!(
        notice.payload["body"],
        json!("Generation queued from [REDACTED]")
    );
}

#[tokio::test]
async fn page_conversation_timeline_keeps_background_started_before_real_run_started() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let attempt_id = RunId::new();
    let run_id = RunId::new();
    let background_agent_id = harness_contracts::BackgroundAgentId::new();
    open_conversation_session(&state, session_id).await;
    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::BackgroundAgentStarted(harness_contracts::BackgroundAgentStartedEvent {
                    background_agent_id,
                    conversation_id: session_id,
                    attempt_id,
                    title: UiSafeText::from_trusted_redacted("Background run"),
                    at: now(),
                }),
                Event::RunStarted(test_run_started_event(session_id, run_id)),
            ],
        )
        .await
        .expect("background and run events should append");

    let page = page_conversation_timeline_with_runtime_state(
        PageConversationTimelineRequest {
            conversation_id: session_id.to_string(),
            after_cursor: None,
            limit: None,
        },
        &state,
    )
    .await
    .expect("timeline page should load");

    let background_started = page
        .events
        .iter()
        .find(|event| event.event_type == "background.started")
        .expect("background start should be preserved before the real run starts");
    assert_eq!(
        background_started.payload["backgroundAgentId"],
        json!(background_agent_id.to_string())
    );
    assert_eq!(background_started.run_id, attempt_id.to_string());
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
                    presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
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
    assert_eq!(permission_event.payload["actorSource"]["type"], "parentRun");
    assert_eq!(
        permission_event.payload["reason"],
        "已按本次授权模式自动允许。"
    );
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
                    presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
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
                presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
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
                    presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
                    interactivity: InteractivityLevel::FullyInteractive,
                    auto_resolved: false,
                    actor_source: PermissionActorSource::ParentRun,
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
async fn page_conversation_worktree_with_runtime_state_returns_safe_turn_tree() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let user_message_id = MessageId::new();
    let assistant_message_id = MessageId::new();
    let empty_assistant_message_id = MessageId::new();
    let tool_use_id = ToolUseId::new();
    let request_id = RequestId::new();
    let artifact_blob_ref = harness_contracts::BlobRef {
        id: harness_contracts::BlobId::new(),
        size: 42,
        content_hash: [7; 32],
        content_type: Some("image/png".to_owned()),
    };
    let raw_error = "failed at /Users/alice/private with token=secret-token";
    open_conversation_session(&state, session_id).await;
    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::UserMessageAppended(UserMessageAppendedEvent {
                    run_id,
                    message_id: user_message_id,
                    content: MessageContent::Text("请生成图片".to_owned()),
                    metadata: MessageMetadata::default(),
                    attachments: Vec::new(),
                    at: now(),
                }),
                Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
                    run_id,
                    message_id: empty_assistant_message_id,
                    content: MessageContent::Text("".to_owned()),
                    tool_uses: Vec::new(),
                    usage: UsageSnapshot::default(),
                    pricing_snapshot_id: None,
                    stop_reason: StopReason::ToolUse,
                    at: now(),
                }),
                Event::ToolUseRequested(test_tool_use_requested_event(
                    run_id,
                    tool_use_id,
                    "MiniMaxTextToImage",
                )),
                Event::PermissionRequested(PermissionRequestedEvent {
                    request_id,
                    run_id,
                    session_id,
                    tenant_id: TenantId::SINGLE,
                    tool_use_id,
                    tool_name: "MiniMaxTextToImage".to_owned(),
                    subject: PermissionSubject::ToolInvocation {
                        tool: "MiniMaxTextToImage".to_owned(),
                        input: json!({ "prompt": "image generation" }),
                    },
                    severity: Severity::Medium,
                    scope_hint: DecisionScope::Any,
                    fingerprint: None,
                    presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
                    interactivity: InteractivityLevel::FullyInteractive,
                    auto_resolved: false,
                    actor_source: PermissionActorSource::ParentRun,
                    causation_id: EventId::new(),
                    at: now(),
                }),
                Event::PermissionResolved(PermissionResolvedEvent {
                    request_id,
                    decision: Decision::AllowOnce,
                    decided_by: DecidedBy::User,
                    scope: DecisionScope::Any,
                    fingerprint: None,
                    rationale: None,
                    at: now(),
                }),
                Event::ToolUseFailed(ToolUseFailedEvent {
                    at: now(),
                    error: ToolErrorPayload {
                        code: "execution".to_owned(),
                        message: raw_error.to_owned(),
                        retriable: false,
                    },
                    tool_use_id,
                }),
                Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
                    run_id,
                    message_id: assistant_message_id,
                    content: MessageContent::Text("图片工具当前不可用。".to_owned()),
                    tool_uses: Vec::new(),
                    usage: UsageSnapshot::default(),
                    pricing_snapshot_id: None,
                    stop_reason: StopReason::EndTurn,
                    at: now(),
                }),
                Event::ArtifactCreated(ArtifactCreatedEvent {
                    artifact_id: "artifact-minimax-prompt".to_owned(),
                    at: now(),
                    blob_ref: Some(artifact_blob_ref.clone()),
                    content_hash: Some(vec![9; 32]),
                    kind: "image_prompt".to_owned(),
                    preview: Some("可复用的图像生成提示词已准备好。".to_owned()),
                    run_id,
                    session_id,
                    source: ArtifactSource::Assistant,
                    source_message_id: Some(assistant_message_id),
                    source_tool_use_id: Some(tool_use_id),
                    status: ArtifactStatus::Ready,
                    title: "海报生成提示词".to_owned(),
                }),
            ],
        )
        .await
        .expect("events should append");

    let page = page_conversation_worktree_with_runtime_state(
        PageConversationWorktreeRequest {
            conversation_id: session_id.to_string(),
            page_cursor: None,
            direction: PageConversationWorktreeDirection::After,
            limit: Some(1),
        },
        &state,
    )
    .await
    .expect("worktree should load");
    let serialized = serde_json::to_string(&page).unwrap();

    assert_eq!(page.turns.len(), 1);
    assert_eq!(page.turns[0].user.body.as_str(), "请生成图片");
    let assistant = page.turns[0].assistant.as_ref().unwrap();
    assert_eq!(assistant.id, format!("assistant:{run_id}"));
    assert!(!serialized.contains(raw_error));
    assert!(!serialized.contains("/Users/alice/private"));
    assert!(!serialized.contains(&artifact_blob_ref.id.to_string()));
    assert!(!serialized.contains("Tool error withheld from conversation timeline."));
    assert!(!serialized.contains(&empty_assistant_message_id.to_string()));

    let tool = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            harness_contracts::AssistantSegment::ToolGroup(group) => group.attempts.first(),
            _ => None,
        })
        .expect("tool attempt should be nested");
    assert_eq!(tool.tool_use_id, tool_use_id.to_string());
    assert_eq!(
        tool.permission.as_ref().unwrap().request_id,
        request_id.to_string()
    );
    assert_eq!(
        tool.failure_summary.as_ref().unwrap().as_str(),
        "工具执行失败。可在详情中查看。"
    );

    assert!(
        assistant
            .segments
            .iter()
            .all(|segment| !matches!(segment, harness_contracts::AssistantSegment::Artifact(_))),
        "ready image artifacts should be projected inside process steps"
    );
    let artifact_step = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            harness_contracts::AssistantSegment::Process(process) => {
                process.steps.iter().find_map(|step| match &step.detail {
                    Some(harness_contracts::ProcessStepDetail::Artifact { artifact_id, media }) => {
                        Some((step, artifact_id, media))
                    }
                    _ => None,
                })
            }
            _ => None,
        })
        .expect("process artifact step should be present");
    assert_eq!(artifact_step.0.title.as_str(), "海报生成提示词");
    assert_eq!(artifact_step.1, "artifact-minimax-prompt");
    assert_eq!(
        artifact_step.2.kind,
        harness_contracts::ArtifactMediaKind::Image
    );
}

#[tokio::test]
async fn page_conversation_worktree_with_runtime_state_rejects_malformed_conversation_id_before_runtime(
) {
    let workspace = unique_workspace("worktree-malformed-conversation-id");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("workspace state should initialize without a harness");

    let error = page_conversation_worktree_with_runtime_state(
        PageConversationWorktreeRequest {
            conversation_id: "not-a-session-id".to_owned(),
            page_cursor: None,
            direction: PageConversationWorktreeDirection::After,
            limit: Some(1),
        },
        &state,
    )
    .await
    .expect_err("malformed conversation id should fail closed");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(
        error.message.contains("conversation session id"),
        "unexpected error message: {}",
        error.message
    );
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
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn get_replay_timeline_with_runtime_state_reads_redacted_journal_events_without_running_tools(
) {
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

    let payload = get_replay_timeline_with_runtime_state(
        ReplayTimelineRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(started.run_id.clone()),
        },
        &state,
    )
    .await
    .unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(payload.replayed);
    let run_started = payload
        .events
        .iter()
        .find(|event| event.event_type == "run.started")
        .expect("replay should include run started event");
    assert_eq!(run_started.payload["permissionMode"], json!("default"));
    assert!(payload
        .events
        .iter()
        .any(|event| event.event_type == "permission.requested"));
    assert!(!serialized.contains("ghp_abcdefghijklmnopqrstuvwxyz0123456789"));
    assert!(!serialized.contains(secret_command));
    assert!(serialized.contains("\"target\":\"git\""));
    assert_eq!(
        state.pending_permission_requests().len(),
        1,
        "replay read mode must not resolve or execute pending tools"
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

#[tokio::test]
async fn get_replay_timeline_with_runtime_state_reads_beyond_first_event_page() {
    let mut stream_events = (0..205)
        .map(|index| ModelStreamEvent::ContentBlockDelta {
            index,
            delta: ContentDelta::Text(format!("delta-{index}")),
        })
        .collect::<Vec<_>>();
    stream_events.push(ModelStreamEvent::MessageStop);
    let state =
        runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(stream_events)]).await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
            permission_mode: None,
            prompt: "Write many deltas".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let request = ReplayTimelineRequest {
        conversation_id: Some(session_id.to_string()),
        run_id: Some(started.run_id),
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

    loop {
        let payload = get_replay_timeline_with_runtime_state(request.clone(), &state)
            .await
            .unwrap();
        let serialized = serde_json::to_string(&payload).unwrap();
        if payload.events.len() > 200 && serialized.contains("delta-204") {
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("replay timeline should include events past the first page");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn replay_and_support_bundle_require_conversation_id_with_run_filter() {
    let state = runtime_state_with_harness().await;

    let replay_error = get_replay_timeline_with_runtime_state(
        ReplayTimelineRequest {
            conversation_id: None,
            run_id: Some(RunId::new().to_string()),
        },
        &state,
    )
    .await
    .unwrap_err();
    let export_error = export_support_bundle_with_runtime_state(
        ExportSupportBundleRequest {
            conversation_id: None,
            run_id: Some(RunId::new().to_string()),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(replay_error.code, "INVALID_PAYLOAD");
    assert_eq!(export_error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn export_support_bundle_with_runtime_state_writes_redacted_files_under_workspace_exports() {
    let _lock = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("support-bundle-export");
    std::fs::create_dir_all(&workspace).unwrap();
    let secret_command =
        "git push https://ghp_abcdefghijklmnopqrstuvwxyz0123456789@github.com/org/repo";
    let state = runtime_state_with_scripted_model_for_workspace(
        workspace.clone(),
        vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::ToolUseComplete {
                    id: ToolUseId::new(),
                    name: "NeedsPermission".to_owned(),
                    input: json!({ "command": secret_command }),
                },
            },
            ModelStreamEvent::MessageStop,
        ])],
    )
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

    let payload = export_support_bundle_with_runtime_state(
        ExportSupportBundleRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(started.run_id),
        },
        &state,
    )
    .await
    .unwrap();

    assert!(payload.redacted);
    assert!(payload.event_count >= 2);
    assert!(payload.bundle_path.starts_with(".jyowo/runtime/exports/"));
    assert!(payload.bundle_path.contains("support-bundle-"));
    assert!(payload.jsonl_path.starts_with(".jyowo/runtime/exports/"));
    assert!(payload.markdown_path.starts_with(".jyowo/runtime/exports/"));

    let bundle = std::fs::read_to_string(workspace.join(&payload.bundle_path)).unwrap();
    let jsonl = std::fs::read_to_string(workspace.join(&payload.jsonl_path)).unwrap();
    let markdown = std::fs::read_to_string(workspace.join(&payload.markdown_path)).unwrap();
    let exported = format!("{bundle}\n{jsonl}\n{markdown}");

    assert!(!exported.contains("ghp_abcdefghijklmnopqrstuvwxyz0123456789"));
    assert!(!exported.contains(secret_command));
    assert!(!exported.contains("Run a command"));
    assert!(exported.contains("\"target\":\"git\""));
    assert!(exported.contains("\"redacted\":true"));

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

#[tokio::test]
async fn support_bundle_agent_redaction_exports_child_agent_summaries_without_internals() {
    let _lock = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("support-bundle-agent-redaction");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace.clone()).await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let subagent_id = harness_contracts::SubagentId::new();
    let team_id = harness_contracts::TeamId::new();
    let background_agent_id = harness_contracts::BackgroundAgentId::new();
    let request_id = RequestId::new();
    let secret = "sk-abcdefghijklmnopqrstuvwxyz";
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
                Event::SubagentSpawned(harness_contracts::SubagentSpawnedEvent {
                    subagent_id,
                    parent_session_id: session_id,
                    parent_run_id: run_id,
                    agent_ref: harness_contracts::AgentRef {
                        id: harness_contracts::AgentId::new(),
                        name: format!("Reviewer {secret}"),
                    },
                    spec_snapshot_id: SnapshotId::new(),
                    spec_hash: [0; 32],
                    depth: 1,
                    trigger_tool_use_id: None,
                    trigger_tool_name: Some("agent".to_owned()),
                    at: now(),
                }),
                Event::SubagentAnnounced(harness_contracts::SubagentAnnouncedEvent {
                    subagent_id,
                    parent_session_id: session_id,
                    status: harness_contracts::SubagentStatus::Completed,
                    summary: format!("child completed with {secret}"),
                    result: Some(json!({ "rawOutput": secret })),
                    usage: UsageSnapshot::default(),
                    transcript_ref: Some(harness_contracts::TranscriptRef {
                        blob: harness_contracts::BlobRef {
                            id: harness_contracts::BlobId::new(),
                            size: 64,
                            content_hash: [1; 32],
                            content_type: Some("application/json".to_owned()),
                        },
                        from_offset: harness_contracts::JournalOffset(1),
                        to_offset: harness_contracts::JournalOffset(2),
                    }),
                    context_report: None,
                    renderer_id: "default".to_owned(),
                    at: now(),
                }),
                Event::TeamCreated(harness_contracts::TeamCreatedEvent {
                    team_id,
                    tenant_id: TenantId::SINGLE,
                    name: format!("Team {secret}"),
                    topology_kind: harness_contracts::TopologyKind::CoordinatorWorker,
                    member_specs_hash: [0; 32],
                    created_at: now(),
                }),
                Event::TeamTaskUpdated(harness_contracts::TeamTaskUpdatedEvent {
                    team_id,
                    task_id: format!("task-{secret}"),
                    title: format!("Audit {secret}"),
                    status: "running".to_owned(),
                    assignee_profile_id: Some(format!("worker-{secret}")),
                    at: now(),
                }),
                Event::PermissionRequested(harness_contracts::PermissionRequestedEvent {
                    request_id,
                    run_id,
                    session_id,
                    tenant_id: TenantId::SINGLE,
                    tool_use_id: ToolUseId::new(),
                    tool_name: "NeedsPermission".to_owned(),
                    subject: harness_contracts::PermissionSubject::ToolInvocation {
                        tool: "NeedsPermission".to_owned(),
                        input: json!({}),
                    },
                    severity: harness_contracts::Severity::Medium,
                    scope_hint: harness_contracts::DecisionScope::ToolName(
                        "NeedsPermission".to_owned(),
                    ),
                    fingerprint: None,
                    presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
                    interactivity: harness_contracts::InteractivityLevel::FullyInteractive,
                    auto_resolved: false,
                    actor_source: PermissionActorSource::TeamMember {
                        team_id,
                        agent_id: harness_contracts::AgentId::new(),
                        role: format!("reviewer {secret}"),
                        parent_run_id: Some(run_id),
                    },
                    causation_id: EventId::new(),
                    at: now(),
                }),
                Event::BackgroundAgentStarted(harness_contracts::BackgroundAgentStartedEvent {
                    background_agent_id,
                    conversation_id: session_id,
                    attempt_id: run_id,
                    title: UiSafeText::from_redacted_display(
                        format!("Background {secret}"),
                        &DefaultRedactor::default(),
                    ),
                    at: now(),
                }),
                Event::BackgroundAgentPermissionRequested(
                    harness_contracts::BackgroundAgentPermissionRequestedEvent {
                        background_agent_id,
                        tenant_id: TenantId::SINGLE,
                        conversation_id: session_id,
                        request_id,
                        attempt_id: Some(run_id),
                        reason: UiSafeText::from_redacted_display(
                            format!("permission {secret}"),
                            &DefaultRedactor::default(),
                        ),
                        at: now(),
                    },
                ),
            ],
        )
        .await
        .unwrap();

    let payload = export_support_bundle_with_runtime_state(
        ExportSupportBundleRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .unwrap();

    let bundle = std::fs::read_to_string(workspace.join(&payload.bundle_path)).unwrap();
    let jsonl = std::fs::read_to_string(workspace.join(&payload.jsonl_path)).unwrap();
    let exported = format!("{bundle}\n{jsonl}");

    assert!(exported.contains("subagent.spawned"));
    assert!(exported.contains("subagent.announced"));
    assert!(exported.contains("team.task.updated"));
    assert!(exported.contains("background.permission.requested"));
    assert!(exported.contains(&subagent_id.to_string()));
    assert!(exported.contains(&team_id.to_string()));
    assert!(exported.contains(&background_agent_id.to_string()));
    assert!(!exported.contains(secret));
    assert!(!exported.contains("child completed"));
    assert!(!exported.contains("rawOutput"));
    assert!(!exported.contains("transcriptRef"));
}

#[cfg(unix)]
#[tokio::test]
async fn export_support_bundle_with_runtime_state_rejects_symlink_export_directory() {
    let _lock = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("support-bundle-symlink-export");
    let external = unique_workspace("support-bundle-external-target");
    std::fs::create_dir_all(workspace.join(".jyowo").join("runtime")).unwrap();
    std::fs::create_dir_all(&external).unwrap();
    std::os::unix::fs::symlink(
        &external,
        workspace.join(".jyowo").join("runtime").join("exports"),
    )
    .unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;
    open_conversation_session(&state, state.default_conversation_id()).await;

    let error = export_support_bundle_with_runtime_state(
        ExportSupportBundleRequest {
            conversation_id: Some(state.default_conversation_id().to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert_eq!(std::fs::read_dir(external).unwrap().count(), 0);
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

    let decision_task =
        tokio::spawn(async move { broker.decide(request, permission_context()).await });

    wait_for_pending_permission(&state, request_id).await;

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
            request_id: request_id.to_string(),
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
        broker
            .decide(request, permission_context_with_run_id(Some(run_id)))
            .await
    });

    wait_for_pending_permission(&state, request_id).await;

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
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(decision_task.await.unwrap(), Decision::DenyOnce);
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

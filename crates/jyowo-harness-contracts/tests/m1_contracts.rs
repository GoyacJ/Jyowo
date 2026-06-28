use bytes::Bytes;
use futures::{future::BoxFuture, stream::BoxStream};
use harness_contracts::*;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn ids_roundtrip_and_tenant_sentinels_are_stable() {
    let session = SessionId::new();
    let encoded = session.to_string();
    let parsed = SessionId::parse(&encoded).expect("session id parses");

    assert_eq!(session, parsed);
    assert_eq!(
        serde_json::from_str::<SessionId>(&serde_json::to_string(&session).unwrap()).unwrap(),
        session
    );
    assert_ne!(TenantId::SINGLE, TenantId::SHARED);
    assert_eq!(
        TenantId::parse(&TenantId::SINGLE.to_string()).unwrap(),
        TenantId::SINGLE
    );
}

#[test]
fn key_events_serialize_with_type_tag() {
    let event = Event::RunEnded(RunEndedEvent {
        run_id: RunId::new(),
        reason: EndReason::Cancelled {
            initiator: CancelInitiator::User,
        },
        usage: None,
        ended_at: chrono::Utc::now(),
    });

    let value = serde_json::to_value(event).unwrap();
    assert_eq!(value["type"], "run_ended");

    let post_execution_failure =
        Event::SandboxPostExecutionFailed(SandboxPostExecutionFailedEvent {
            session_id: SessionId::new(),
            run_id: RunId::new(),
            tool_use_id: Some(ToolUseId::new()),
            backend_id: "ssh".to_owned(),
            error: SandboxError::Message("cleanup failed".to_owned()),
            at: chrono::Utc::now(),
        });
    let value = serde_json::to_value(post_execution_failure).unwrap();
    assert_eq!(value["type"], "sandbox_post_execution_failed");

    let backend_failure = Event::SandboxBackendFailed(SandboxBackendFailedEvent {
        session_id: SessionId::new(),
        run_id: RunId::new(),
        tool_use_id: Some(ToolUseId::new()),
        backend_id: "local".to_owned(),
        phase: SandboxBackendFailurePhase::Execute,
        error: SandboxError::Message("spawn failed".to_owned()),
        at: chrono::Utc::now(),
    });
    let value = serde_json::to_value(backend_failure).unwrap();
    assert_eq!(value["type"], "sandbox_backend_failed");
    assert_eq!(value["phase"], "execute");

    let grace = GraceCallTriggeredEvent {
        run_id: RunId::new(),
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
        current_iteration: 4,
        max_iterations: 3,
        usage_snapshot: UsageSnapshot::default(),
        at: chrono::Utc::now(),
        correlation_id: CorrelationId::new(),
    };
    assert_eq!(grace.current_iteration, 4);
}

#[test]
fn assistant_review_requested_segment_source_events_serialize_with_stable_tags() {
    let run_id = RunId::new();
    let at = chrono::Utc::now();

    let review = Event::AssistantReviewRequested(AssistantReviewRequestedEvent {
        run_id,
        request_id: RequestId::new(),
        title: UiSafeText::from_trusted_redacted("Review changes"),
        body: Some(UiSafeText::from_trusted_redacted(
            "Confirm before applying.",
        )),
        at,
    });
    let value = serde_json::to_value(review).unwrap();
    assert_eq!(value["type"], "assistant_review_requested");
    assert_eq!(value["title"], "Review changes");
    assert_eq!(value["body"], "Confirm before applying.");
    assert!(value.get("session_id").is_none());

    let clarification =
        Event::AssistantClarificationRequested(AssistantClarificationRequestedEvent {
            run_id,
            request_id: RequestId::new(),
            prompt: UiSafeText::from_trusted_redacted("Which style should I use?"),
            at,
        });
    let value = serde_json::to_value(clarification).unwrap();
    assert_eq!(value["type"], "assistant_clarification_requested");
    assert_eq!(value["prompt"], "Which style should I use?");
    assert!(value.get("session_id").is_none());

    let notice = Event::AssistantNotice(AssistantNoticeEvent {
        run_id,
        notice_id: RequestId::new(),
        body: UiSafeText::from_trusted_redacted("Tool output was summarized."),
        code: Some(AssistantNoticeCode::ContextCompacted),
        at,
    });
    let value = serde_json::to_value(notice).unwrap();
    assert_eq!(value["type"], "assistant_notice");
    assert_eq!(value["body"], "Tool output was summarized.");
    assert_eq!(value["code"], "contextCompacted");
    assert!(value.get("session_id").is_none());
}

#[test]
fn redactor_is_dyn_safe_and_noop_preserves_input() {
    let redactor: &dyn Redactor = &NoopRedactor;
    assert_eq!(redactor.redact("secret", &RedactRules::default()), "secret");
}

#[test]
fn schema_export_contains_required_surface() {
    let schemas = export_all_schemas();

    assert!(schemas.len() >= 60);
    assert!(schemas.contains_key("event"));
    assert!(schemas.contains_key("tool_descriptor"));
    assert!(schemas.contains_key("tool_use_requested"));
    assert!(schemas.contains_key("artifact_created"));
    assert!(schemas.contains_key("artifact_updated"));
    assert!(schemas.contains_key("assistant_review_requested"));
    assert!(schemas.contains_key("assistant_clarification_requested"));
    assert!(schemas.contains_key("assistant_notice"));
    assert!(schemas.contains_key("credential_pool_shared_across_tenants"));
    assert!(schemas.contains_key("manifest_validation_failed"));
    assert!(schemas.contains_key("hook_failed"));
    assert!(schemas.contains_key("clarify_prompt"));
    assert!(schemas.contains_key("user_message_delivery"));
    assert!(schemas.contains_key("skill_filter"));
    assert!(schemas.contains_key("skill_summary"));
    assert!(schemas.contains_key("skill_status"));
    assert!(schemas.contains_key("skill_view"));
    assert!(schemas.contains_key("skill_parameter_info"));
    assert!(schemas.contains_key("skill_injection_id"));
    assert!(schemas.contains_key("skill_invocation_receipt"));
    assert!(schemas.contains_key("rendered_skill"));
    assert!(schemas.contains_key("skill_shell_invocation"));
    assert!(schemas.contains_key("conversation_context_reference"));
    assert!(schemas.contains_key("conversation_attachment_reference"));
    assert!(schemas.contains_key("conversation_turn_input"));
    assert!(schemas.contains_key("ui_safe_text"));
    assert!(schemas.contains_key("conversation_cursor"));
    assert!(schemas.contains_key("conversation_summary"));
    assert!(schemas.contains_key("conversation_message"));
    assert!(schemas.contains_key("conversation_snapshot"));
    assert!(schemas.contains_key("conversation_timeline_event"));
    assert!(schemas.contains_key("conversation_timeline_page"));
    assert!(!schemas.contains_key("conversation_intent_mode"));
    assert!(schemas.contains_key("sandbox_post_execution_failed"));
    assert!(schemas.contains_key("sandbox_backend_failed"));
    assert!(schemas.contains_key("model_protocol"));
    assert!(schemas.contains_key("model_modality"));
    assert!(schemas.contains_key("conversation_model_capability"));
    assert!(schemas.contains_key("provider_service_capability"));
    assert!(schemas.contains_key("provider_runtime_capability"));

    for key in [
        "run_started",
        "run_ended",
        "assistant_delta_produced",
        "assistant_message_completed",
        "tool_use_requested",
        "tool_use_approved",
        "tool_use_denied",
        "tool_use_completed",
        "tool_use_failed",
        "permission_requested",
        "permission_resolved",
        "engine_failed",
    ] {
        assert!(schemas.contains_key(key), "missing MVP event schema: {key}");
    }
}

#[test]
fn ui_safe_text_redacts_private_paths_and_obvious_secrets() {
    let redactor: &dyn Redactor = &NoopRedactor;

    for value in [
        "open /Users/goya/.ssh/config",
        "read /home/alice/.aws/credentials",
        "tail /private/var/folders/token.txt",
        "type C:\\Users\\alice\\.ssh\\config",
        "type C:/Users/alice/.ssh/config",
        "Authorization: Bearer abcdef123456",
        "Bearer abcdefghijklmnop",
        "Basic abcdefghijklmnop",
        "eyJabcdefgh.eyJijklmnop.eyJqrstuvwx",
        "\"eyJabcdefgh.eyJijklmnop.eyJqrstuvwx\"",
        "postgres://user:password@example.com/app",
        "client_secret: verysecretvalue",
        "client_secret : verysecretvalue",
        "client_secret = 'verysecretvalue'",
        "password: supersecret",
        "password : supersecret",
        "password: \"supersecret\"",
        "password: \"my secret phrase\"",
        "token: abcdefghijklmnop",
        "rk_live_abcdefghijklmnop",
        "ghp_abcdefghijklmnopqrstuvwxyz",
        "github_pat_abcdefghijklmnopqrstuvwxyz",
        "xoxb-abcdefghijklmnop",
        "xoxs-abcdefghijklmnop",
        "npm_abcdefghijklmnopqrst",
        "lin_api_abcdefghijklmnopqrst",
        "secret_abcdefghijklmnopqrst",
        "sk_live_abcdefghijklmnop",
        "code=abcdefghijkl",
        "ASIAABCDEFGHIJKLMNOP",
        "A3TABCDEFGHIJKLMNOPQ",
        "\"ASIAABCDEFGHIJKLMNOP\"",
        "[A3TABCDEFGHIJKLMNOPQ]",
        "api_key = sk-abcdefghijkl",
    ] {
        let text = UiSafeText::from_redacted_display(value, redactor);
        assert_eq!(text.as_str(), "[REDACTED]");
    }

    let text = UiSafeText::from_redacted_display("plain project note", redactor);
    assert_eq!(text.as_str(), "plain project note");
}

#[test]
fn conversation_read_model_contracts_use_stable_wire_shape() {
    let cursor = ConversationCursor {
        event_id: EventId::new(),
        conversation_sequence: 12,
    };
    let message = ConversationMessage {
        author: ConversationMessageAuthor::User,
        body: UiSafeText::from_trusted_redacted("hello"),
        client_message_id: Some("550e8400-e29b-41d4-a716-446655440000".to_owned()),
        id: MessageId::new().to_string(),
        timestamp: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
        conversation_sequence: 11,
    };
    let snapshot = ConversationSnapshot {
        id: SessionId::new().to_string(),
        messages: vec![message],
        model_config_id: Some("provider-config-1".to_owned()),
        title: UiSafeText::from_trusted_redacted("hello"),
        updated_at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
        cursor: Some(cursor),
    };

    let value = serde_json::to_value(snapshot).unwrap();

    assert_eq!(value["messages"][0]["author"], "user");
    assert_eq!(value["messages"][0]["body"], "hello");
    assert_eq!(value["messages"][0]["conversationSequence"], 11);
    assert_eq!(value["cursor"]["conversationSequence"], 12);
}

#[test]
fn conversation_worktree_contracts_use_stable_wire_shape() {
    let event_cursor = ConversationCursor {
        event_id: EventId::new(),
        conversation_sequence: 42,
    };
    let event_ref = ConversationEventRef {
        event_id: "event-1".to_owned(),
        cursor: event_cursor,
    };
    let permission = ToolPermissionState {
        id: "permission:request-1".to_owned(),
        request_id: "request-1".to_owned(),
        tool_use_id: "tool-use-1".to_owned(),
        status: ToolPermissionStatus::Approved,
        summary: Some(UiSafeText::from_trusted_redacted("Approved once")),
        event_refs: vec![event_ref.clone()],
    };
    let page = ConversationWorktreePage {
        turns: vec![ConversationTurn {
            id: "turn:user-message-1".to_owned(),
            conversation_id: "conversation-1".to_owned(),
            position: 7,
            user: ConversationTurnUserMessage {
                id: "user:user-message-1".to_owned(),
                message_id: "user-message-1".to_owned(),
                body: UiSafeText::from_trusted_redacted("Generate an image"),
                client_message_id: Some("client-1".to_owned()),
                attachments: vec![ConversationAttachmentReference {
                    id: "attachment-001".to_owned(),
                    name: "reference.png".to_owned(),
                    mime_type: "image/png".to_owned(),
                    size_bytes: 128,
                    blob_ref: test_blob_ref(128, "image/png"),
                }],
                timestamp: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
                event_refs: vec![event_ref.clone()],
            },
            assistant: Some(AssistantWork {
                id: "assistant:run-1".to_owned(),
                run_id: "run-1".to_owned(),
                status: AssistantWorkStatus::Running,
                segments: vec![
                    AssistantSegment::Process(ProcessSegment {
                        id: "segment:process:run-1".to_owned(),
                        order: 0,
                        status: ProcessSegmentStatus::Running,
                        summary: UiSafeText::from_trusted_redacted("正在处理请求"),
                        steps: vec![
                            ProcessStep {
                                id: "process-step:run-1:reasoning".to_owned(),
                                order: 0,
                                kind: ProcessStepKind::Reasoning,
                                status: ProcessStepStatus::Running,
                                title: UiSafeText::from_trusted_redacted("分析请求"),
                                body: Some(UiSafeText::from_trusted_redacted(
                                    "确认需要生成图片并展示结果。",
                                )),
                                detail: None,
                                event_refs: vec![event_ref.clone()],
                            },
                            ProcessStep {
                                id: "process-step:run-1:artifact".to_owned(),
                                order: 1,
                                kind: ProcessStepKind::Artifact,
                                status: ProcessStepStatus::Complete,
                                title: UiSafeText::from_trusted_redacted("生成的图片"),
                                body: None,
                                detail: Some(ProcessStepDetail::Artifact {
                                    artifact_id: "artifact-1".to_owned(),
                                    media: ArtifactMediaPreview {
                                        kind: ArtifactMediaKind::Image,
                                        mime_type: "image/png".to_owned(),
                                        size_bytes: 128,
                                    },
                                }),
                                event_refs: vec![event_ref.clone()],
                            },
                        ],
                        event_refs: vec![event_ref.clone()],
                    }),
                    AssistantSegment::Thinking(ThinkingSegment {
                        id: "segment:thinking:run-1".to_owned(),
                        order: 1,
                        status: ThinkingSegmentStatus::Running,
                        summary: ThinkingSummary {
                            text: UiSafeText::from_trusted_redacted("Checking available tools"),
                        },
                        steps: vec![ThinkingStep {
                            id: "thinking-step:run-1:summary".to_owned(),
                            order: 0,
                            kind: ThinkingStepKind::ReasoningSummary,
                            status: ThinkingStepStatus::Running,
                            title: UiSafeText::from_trusted_redacted("推理过程"),
                            body: Some(UiSafeText::from_trusted_redacted(
                                "Checking available tools",
                            )),
                            event_refs: vec![event_ref.clone()],
                        }],
                        event_refs: vec![event_ref.clone()],
                    }),
                    AssistantSegment::Text(TextSegment {
                        id: "segment:text:assistant-message-1".to_owned(),
                        order: 2,
                        message_id: "assistant-message-1".to_owned(),
                        body: UiSafeText::from_trusted_redacted("I can help with that."),
                        event_refs: vec![event_ref.clone()],
                    }),
                    AssistantSegment::ToolGroup(ToolGroupSegment {
                        id: "segment:tools:tool-use-1".to_owned(),
                        order: 3,
                        attempts: vec![ToolAttempt {
                            id: "tool:tool-use-1".to_owned(),
                            order: 0,
                            tool_use_id: "tool-use-1".to_owned(),
                            tool_name: UiSafeText::from_trusted_redacted("MiniMaxTextToImage"),
                            status: ToolAttemptStatus::Completed,
                            permission: Some(permission),
                            failure_summary: None,
                            event_refs: vec![event_ref.clone()],
                        }],
                        event_refs: vec![event_ref.clone()],
                    }),
                    AssistantSegment::Artifact(ArtifactSegment {
                        id: "segment:artifact:artifact-1".to_owned(),
                        order: 4,
                        artifact_id: "artifact-1".to_owned(),
                        kind: "image".to_owned(),
                        status: ArtifactStatus::Ready,
                        source: ArtifactSource::Tool,
                        title: UiSafeText::from_trusted_redacted("Generated image"),
                        summary: Some(UiSafeText::from_trusted_redacted("Image artifact ready")),
                        media: Some(ArtifactMediaPreview {
                            kind: ArtifactMediaKind::Image,
                            mime_type: "image/png".to_owned(),
                            size_bytes: 128,
                        }),
                        event_refs: vec![event_ref.clone()],
                    }),
                    AssistantSegment::ReviewRequest(ReviewRequestSegment {
                        id: "segment:review:review-1".to_owned(),
                        order: 5,
                        request_id: "review-1".to_owned(),
                        title: UiSafeText::from_trusted_redacted("Review changes"),
                        body: Some(UiSafeText::from_trusted_redacted(
                            "Confirm before applying.",
                        )),
                        event_refs: vec![event_ref.clone()],
                    }),
                    AssistantSegment::ClarificationRequest(ClarificationRequestSegment {
                        id: "segment:clarification:clarification-1".to_owned(),
                        order: 6,
                        request_id: "clarification-1".to_owned(),
                        prompt: UiSafeText::from_trusted_redacted("Which style should I use?"),
                        event_refs: vec![event_ref.clone()],
                    }),
                    AssistantSegment::Notice(NoticeSegment {
                        id: "segment:notice:event-1".to_owned(),
                        order: 7,
                        body: UiSafeText::from_trusted_redacted("Tool output was summarized."),
                        code: Some(AssistantNoticeCode::ContextCompacted),
                        event_refs: vec![event_ref.clone()],
                    }),
                    AssistantSegment::Error(ErrorSegment {
                        id: "segment:error:event-2".to_owned(),
                        order: 8,
                        body: UiSafeText::from_trusted_redacted("Tool execution failed."),
                        event_refs: vec![event_ref.clone()],
                    }),
                ],
                event_refs: vec![event_ref],
            }),
        }],
        page_cursor: Some(ConversationTurnCursor {
            turn_id: "turn:user-message-1".to_owned(),
            position: 7,
        }),
        event_cursor: Some(event_cursor),
        has_more_before: false,
        has_more_after: true,
        gap: false,
    };

    let value = serde_json::to_value(page).unwrap();

    assert_eq!(value["turns"][0]["id"], "turn:user-message-1");
    assert_eq!(value["turns"][0]["position"], 7);
    assert_eq!(value["turns"][0]["user"]["id"], "user:user-message-1");
    assert_eq!(
        value["turns"][0]["user"]["attachments"][0]["name"],
        "reference.png"
    );
    assert_eq!(
        value["turns"][0]["user"]["attachments"][0]["mime_type"],
        "image/png"
    );
    assert_eq!(value["turns"][0]["assistant"]["id"], "assistant:run-1");
    assert_eq!(value["turns"][0]["assistant"]["status"], "running");
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][0]["kind"],
        "process"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][0]["steps"][0]["kind"],
        "reasoning"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][0]["steps"][1]["detail"]["type"],
        "artifact"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][0]["steps"][1]["detail"]["media"]["kind"],
        "image"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][1]["kind"],
        "thinking"
    );
    assert_eq!(value["turns"][0]["assistant"]["segments"][0]["order"], 0);
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][1]["status"],
        "running"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][1]["steps"][0]["kind"],
        "reasoningSummary"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][1]["steps"][0]["body"],
        "Checking available tools"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][3]["kind"],
        "toolGroup"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][3]["attempts"][0]["permission"]["requestId"],
        "request-1"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][3]["attempts"][0]["permission"]["toolUseId"],
        "tool-use-1"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][4]["kind"],
        "artifact"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][4]["media"]["mimeType"],
        "image/png"
    );
    assert_eq!(value["pageCursor"]["turnId"], "turn:user-message-1");
    assert_eq!(value["eventCursor"]["conversationSequence"], 42);
    assert_eq!(value["hasMoreBefore"], false);
    assert_eq!(value["hasMoreAfter"], true);
    assert_eq!(value["gap"], false);
}

#[test]
fn conversation_worktree_fixture_uses_stable_wire_shape() {
    let raw = include_str!("fixtures/conversation_worktree_page.json");
    let page: ConversationWorktreePage = serde_json::from_str(raw).unwrap();
    let value = serde_json::to_value(page).unwrap();

    assert_eq!(value["turns"][0]["id"], "turn:user-message-1");
    assert_eq!(value["turns"][0]["conversationId"], "conversation-1");
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][0]["kind"],
        "process"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][1]["kind"],
        "thinking"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][3]["kind"],
        "toolGroup"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][4]["kind"],
        "artifact"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][5]["kind"],
        "reviewRequest"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][6]["kind"],
        "clarificationRequest"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][7]["kind"],
        "notice"
    );
    assert_eq!(
        value["turns"][0]["assistant"]["segments"][8]["kind"],
        "error"
    );
    assert_eq!(value["pageCursor"]["turnId"], "turn:user-message-1");
    assert_eq!(value["eventCursor"]["conversationSequence"], 54);
    assert_eq!(value["hasMoreBefore"], true);
    assert_eq!(value["hasMoreAfter"], true);
}

#[test]
fn thinking_worktree_segment_supports_all_public_statuses() {
    for (status, expected) in [
        (ThinkingSegmentStatus::Running, "running"),
        (ThinkingSegmentStatus::Complete, "complete"),
        (ThinkingSegmentStatus::Withheld, "withheld"),
    ] {
        let value = serde_json::to_value(status).unwrap();
        assert_eq!(value, expected);
    }
}

#[test]
fn thinking_step_contracts_use_stable_wire_shape() {
    let step = ThinkingStep {
        id: "thinking-step:run-1:summary".to_owned(),
        order: 0,
        kind: ThinkingStepKind::ReasoningSummary,
        status: ThinkingStepStatus::Complete,
        title: UiSafeText::from_trusted_redacted("推理过程"),
        body: Some(UiSafeText::from_trusted_redacted("Checked context.")),
        event_refs: Vec::new(),
    };

    let value = serde_json::to_value(step).unwrap();

    assert_eq!(value["kind"], "reasoningSummary");
    assert_eq!(value["status"], "complete");
    assert_eq!(value["title"], "推理过程");
    assert_eq!(value["body"], "Checked context.");
    assert!(value.get("eventRefs").is_none());
}

#[test]
fn process_segment_contracts_use_stable_wire_shape() {
    let step = ProcessStep {
        id: "process-step:run-1:command".to_owned(),
        order: 0,
        kind: ProcessStepKind::Command,
        status: ProcessStepStatus::Complete,
        title: UiSafeText::from_trusted_redacted("运行测试"),
        body: Some(UiSafeText::from_trusted_redacted("cargo test 通过")),
        detail: Some(ProcessStepDetail::Command {
            command: UiSafeText::from_trusted_redacted("cargo test"),
            output: Some(UiSafeText::from_trusted_redacted("test result: ok")),
            exit_code: Some(0),
            duration_ms: Some(1200),
        }),
        event_refs: Vec::new(),
    };
    let segment = ProcessSegment {
        id: "segment:process:run-1".to_owned(),
        order: 0,
        status: ProcessSegmentStatus::Complete,
        summary: UiSafeText::from_trusted_redacted("已完成工作过程"),
        steps: vec![step],
        event_refs: Vec::new(),
    };

    let value = serde_json::to_value(AssistantSegment::Process(segment)).unwrap();

    assert_eq!(value["kind"], "process");
    assert_eq!(value["status"], "complete");
    assert_eq!(value["summary"], "已完成工作过程");
    assert_eq!(value["steps"][0]["kind"], "command");
    assert_eq!(value["steps"][0]["detail"]["type"], "command");
    assert_eq!(value["steps"][0]["detail"]["command"], "cargo test");
    assert_eq!(value["steps"][0]["detail"]["exitCode"], 0);
    assert!(value["steps"][0].get("eventRefs").is_none());
}

#[test]
fn conversation_worktree_schema_is_exported() {
    let schemas = export_all_schemas();

    for key in [
        "conversation_worktree_page",
        "conversation_turn_cursor",
        "conversation_turn",
        "conversation_turn_user_message",
        "assistant_work",
        "assistant_segment",
        "process_segment",
        "process_segment_status",
        "process_step",
        "process_step_kind",
        "process_step_status",
        "process_step_detail",
        "artifact_media_preview",
        "artifact_media_kind",
        "thinking_segment",
        "thinking_summary",
        "thinking_step",
        "thinking_step_kind",
        "thinking_step_status",
        "text_segment",
        "tool_group_segment",
        "tool_attempt",
        "tool_permission_state",
        "artifact_segment",
        "review_request_segment",
        "clarification_request_segment",
        "notice_segment",
        "error_segment",
        "conversation_event_ref",
    ] {
        assert!(schemas.contains_key(key), "missing worktree schema: {key}");
    }
}

#[test]
fn model_capability_contract_rejects_old_flat_payload() {
    let capability = ConversationModelCapability {
        input_modalities: vec![ModelModality::Text, ModelModality::Image],
        output_modalities: vec![ModelModality::Text],
        context_window: 128_000,
        max_output_tokens: 16_384,
        streaming: true,
        tool_calling: true,
        reasoning: false,
        prompt_cache: true,
        structured_output: true,
    };

    let value = serde_json::to_value(&capability).unwrap();
    assert_eq!(value["input_modalities"][0], "text");
    assert_eq!(value["tool_calling"], true);

    let old_flat_payload = json!({
        "supports_tools": true,
        "supports_vision": false,
        "supports_thinking": false,
        "supports_streaming": true,
        "supports_structured_output": true,
        "supports_json_mode": true,
        "input_modalities": ["text"],
        "output_modalities": ["text"]
    });

    assert!(serde_json::from_value::<ConversationModelCapability>(old_flat_payload).is_err());
}

#[test]
fn provider_capability_contracts_use_stable_wire_names() {
    let service = ProviderServiceCapability {
        operation_id: "minimax.image_generation".to_owned(),
        category: ProviderServiceCategory::Image,
        input_modalities: vec![ModelModality::Text, ModelModality::Image],
        output_artifact: ModelModality::Image,
        execution: ProviderServiceExecution::AsyncJob,
        requires_polling: true,
        permission_subject: "network:minimax".to_owned(),
        cost_risk: ProviderServiceCostRisk::High,
    };
    let runtime = ProviderRuntimeCapability {
        auth_scheme: ProviderAuthScheme::Bearer,
        base_url_regions: vec![
            ProviderBaseUrlRegion {
                id: "global".to_owned(),
                label: "Global".to_owned(),
                base_url: "https://api.minimax.io".to_owned(),
            },
            ProviderBaseUrlRegion {
                id: "cn".to_owned(),
                label: "China".to_owned(),
                base_url: "https://api.minimaxi.com".to_owned(),
            },
        ],
        supports_live_validation: true,
        supports_streaming_validation: true,
        secret_reveal_supported: true,
    };

    let service_value = serde_json::to_value(service).unwrap();
    let runtime_value = serde_json::to_value(runtime).unwrap();

    assert_eq!(service_value["category"], "image");
    assert_eq!(service_value["execution"], "async_job");
    assert_eq!(runtime_value["auth_scheme"], "bearer");
    assert_eq!(
        runtime_value["base_url_regions"][0]["base_url"],
        "https://api.minimax.io"
    );
}

#[test]
fn conversation_turn_input_keeps_stable_wire_shape() {
    let input = ConversationTurnInput {
        client_message_id: None,
        prompt: "Summarize this file".to_owned(),
        context_references: vec![
            ConversationContextReference::WorkspaceFile {
                path: "apps/desktop/src/features/conversation/Composer.tsx".to_owned(),
                label: "Composer.tsx".to_owned(),
            },
            ConversationContextReference::Artifact {
                id: "artifact-001".to_owned(),
                label: "Generated notes".to_owned(),
            },
            ConversationContextReference::Skill {
                id: "skill-review".to_owned(),
                label: "Code review skill".to_owned(),
            },
            ConversationContextReference::Tool {
                id: "builtin.grep".to_owned(),
                label: "Search files".to_owned(),
            },
            ConversationContextReference::McpServer {
                id: "mcp-filesystem".to_owned(),
                label: "Filesystem MCP".to_owned(),
            },
        ],
        attachments: vec![ConversationAttachmentReference {
            id: "attachment-001".to_owned(),
            name: "notes.txt".to_owned(),
            mime_type: "text/plain".to_owned(),
            size_bytes: 128,
            blob_ref: test_blob_ref(128, "text/plain"),
        }],
    };

    let value = serde_json::to_value(&input).expect("conversation turn input should serialize");

    assert_eq!(value["prompt"], "Summarize this file");
    assert!(value.get("intent_mode").is_none());
    assert_eq!(value["context_references"][0]["kind"], "workspace_file");
    assert_eq!(
        value["context_references"][0]["path"],
        "apps/desktop/src/features/conversation/Composer.tsx"
    );
    assert_eq!(value["context_references"][1]["kind"], "artifact");
    assert_eq!(value["context_references"][2]["kind"], "skill");
    assert_eq!(value["context_references"][3]["kind"], "tool");
    assert_eq!(value["context_references"][4]["kind"], "mcp_server");
    assert_eq!(value["attachments"][0]["mime_type"], "text/plain");

    let roundtrip: ConversationTurnInput =
        serde_json::from_value(value).expect("conversation turn input should deserialize");
    assert_eq!(roundtrip, input);
}

#[test]
fn user_message_appended_event_keeps_attachment_metadata() {
    let event = Event::UserMessageAppended(UserMessageAppendedEvent {
        run_id: RunId::new(),
        message_id: MessageId::new(),
        content: MessageContent::Text("Summarize this file".to_owned()),
        metadata: MessageMetadata::default(),
        attachments: vec![ConversationAttachmentReference {
            id: "attachment-001".to_owned(),
            name: "notes.txt".to_owned(),
            mime_type: "text/plain".to_owned(),
            size_bytes: 128,
            blob_ref: test_blob_ref(128, "text/plain"),
        }],
        at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
    });

    let value = serde_json::to_value(&event).expect("event should serialize");

    assert_eq!(value["type"], "user_message_appended");
    assert_eq!(value["attachments"][0]["name"], "notes.txt");
    assert_eq!(value["attachments"][0]["mime_type"], "text/plain");

    let roundtrip: Event = serde_json::from_value(value).expect("event should deserialize");
    assert_eq!(roundtrip, event);
}

fn test_blob_ref(size: u64, content_type: &str) -> BlobRef {
    BlobRef {
        id: BlobId::new(),
        size,
        content_hash: [7; 32],
        content_type: Some(content_type.to_owned()),
    }
}

#[test]
fn tool_descriptor_is_contract_surface() {
    let descriptor = ToolDescriptor {
        name: "read_file".to_owned(),
        display_name: "Read file".to_owned(),
        description: "Read a workspace file".to_owned(),
        category: "filesystem".to_owned(),
        group: ToolGroup::FileSystem,
        version: "1.0.0".to_owned(),
        input_schema: json!({ "type": "object" }),
        output_schema: None,
        dynamic_schema: false,
        properties: ToolProperties {
            is_concurrency_safe: true,
            is_read_only: true,
            is_destructive: false,
            long_running: None,
            defer_policy: DeferPolicy::AlwaysLoad,
        },
        trust_level: TrustLevel::AdminTrusted,
        required_capabilities: vec![ToolCapability::BlobReader],
        budget: ResultBudget {
            metric: BudgetMetric::Chars,
            limit: 8192,
            on_overflow: OverflowAction::Offload,
            preview_head_chars: 1024,
            preview_tail_chars: 1024,
        },
        provider_restriction: ProviderRestriction::All,
        origin: ToolOrigin::Builtin,
        search_hint: Some("read file path".to_owned()),
    };

    let value = serde_json::to_value(&descriptor).unwrap();
    assert_eq!(value["name"], "read_file");

    let roundtrip: ToolDescriptor = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, descriptor);
}

#[test]
fn model_error_variants_are_contract_surface() {
    let error = ModelError::ContextTooLong {
        tokens: 200_000,
        max: 128_000,
    };

    let value = serde_json::to_value(&error).unwrap();
    assert_eq!(value["context_too_long"]["tokens"], 200_000);

    let roundtrip: ModelError = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, error);
    assert_eq!(
        ModelError::AuxModelNotConfigured.to_string(),
        "aux model not configured"
    );
}

#[test]
fn sandbox_error_variants_are_contract_surface() {
    let cases = [
        SandboxError::Unavailable {
            backend: "docker".to_owned(),
            detail: "missing binary".to_owned(),
        },
        SandboxError::CapabilityMismatch {
            capability: "network".to_owned(),
            detail: "unsupported".to_owned(),
        },
        SandboxError::Timeout {
            detail: "wall clock exceeded".to_owned(),
        },
        SandboxError::InactivityTimeout {
            detail: "no output observed".to_owned(),
        },
        SandboxError::OutputBudgetExceeded { limit: 3 },
        SandboxError::HostPathDenied {
            path: "/secret".to_owned(),
        },
        SandboxError::ResourceLimitExceeded {
            limit: "memory".to_owned(),
            detail: "unsupported".to_owned(),
        },
        SandboxError::SnapshotUnsupported {
            kind: "shell_state".to_owned(),
        },
        SandboxError::ContainerLifecycleError {
            detail: "container failed".to_owned(),
        },
        SandboxError::CodeRuntime {
            detail: "forbidden symbol".to_owned(),
        },
    ];

    for error in cases {
        let value = serde_json::to_value(&error).unwrap();
        let roundtrip: SandboxError = serde_json::from_value(value).unwrap();
        assert_eq!(roundtrip, error);
    }
}

struct TestBlobReaderCap;

impl BlobReaderCap for TestBlobReaderCap {
    fn read_blob(
        &self,
        _tenant_id: TenantId,
        _blob: BlobRef,
    ) -> BoxFuture<'_, Result<BoxStream<'static, Bytes>, ToolError>> {
        Box::pin(async { Ok(Box::pin(futures::stream::empty()) as BoxStream<'static, Bytes>) })
    }
}

struct TestBlobWriterCap;

impl BlobWriterCap for TestBlobWriterCap {
    fn write_blob(
        &self,
        _tenant_id: TenantId,
        _bytes: bytes::Bytes,
        _meta: BlobMeta,
    ) -> BoxFuture<'_, Result<BlobRef, ToolError>> {
        Box::pin(async {
            Ok(BlobRef {
                id: BlobId::new(),
                size: 0,
                content_hash: [0; 32],
                content_type: Some("image/png".to_owned()),
            })
        })
    }
}

struct TestSkillRegistryCap;

impl SkillRegistryCap for TestSkillRegistryCap {
    fn list_summaries(&self, _agent: &AgentId, _filter: SkillFilter) -> Vec<SkillSummary> {
        Vec::new()
    }

    fn view(&self, _agent: &AgentId, _name: &str, _full: bool) -> Option<SkillView> {
        None
    }

    fn render(
        &self,
        _agent: &AgentId,
        name: String,
        _params: serde_json::Value,
    ) -> BoxFuture<'static, Result<RenderedSkill, ToolError>> {
        Box::pin(async move { Err(ToolError::Validation(format!("skill not found: {name}"))) })
    }
}

#[test]
fn capability_registry_stores_and_recovers_dyn_capabilities() {
    let mut registry = CapabilityRegistry::default();
    let reader: Arc<dyn BlobReaderCap> = Arc::new(TestBlobReaderCap);
    let writer: Arc<dyn BlobWriterCap> = Arc::new(TestBlobWriterCap);

    registry.install(ToolCapability::BlobReader, Arc::clone(&reader));
    registry.install(ToolCapability::BlobWriter, Arc::clone(&writer));

    let recovered_reader = registry
        .get::<dyn BlobReaderCap>(&ToolCapability::BlobReader)
        .expect("installed capability is available");
    let recovered_writer = registry
        .get::<dyn BlobWriterCap>(&ToolCapability::BlobWriter)
        .expect("installed writer capability is available");

    assert!(Arc::ptr_eq(&reader, &recovered_reader));
    assert!(Arc::ptr_eq(&writer, &recovered_writer));
    assert!(registry
        .get::<dyn BlobReaderCap>(&ToolCapability::SubagentRunner)
        .is_none());
}

#[test]
fn capability_registry_stores_and_recovers_skill_registry_capability() {
    let mut registry = CapabilityRegistry::default();
    let cap: Arc<dyn SkillRegistryCap> = Arc::new(TestSkillRegistryCap);

    registry.install(ToolCapability::SkillRegistry, Arc::clone(&cap));

    let recovered = registry
        .get::<dyn SkillRegistryCap>(&ToolCapability::SkillRegistry)
        .expect("installed skill registry capability is available");

    assert!(Arc::ptr_eq(&cap, &recovered));
}

#[test]
fn tool_error_variants_cover_m3_tool_surface() {
    let missing = ToolError::CapabilityMissing(ToolCapability::BlobReader);
    assert_eq!(
        missing.to_string(),
        "required capability missing: blob_reader"
    );

    let too_large = ToolError::ResultTooLarge {
        original: 4096,
        limit: 1024,
        metric: BudgetMetric::Bytes,
    };
    let value = serde_json::to_value(&too_large).unwrap();

    assert_eq!(value["result_too_large"]["original"], 4096);
    assert_eq!(value["result_too_large"]["metric"], "bytes");
}

#[test]
fn tool_result_part_uses_semantic_whitelist_shape() {
    let part = ToolResultPart::Structured {
        value: json!({ "ok": true }),
        schema_ref: Some("#/properties/ok".to_owned()),
    };

    let value = serde_json::to_value(part).unwrap();
    assert_eq!(value["kind"], "structured");
}

#[test]
fn tool_event_shape_matches_spec_and_rejects_legacy_fields() {
    let event = ToolUseRequestedEvent {
        run_id: RunId::new(),
        tool_use_id: ToolUseId::new(),
        tool_name: "read_file".to_owned(),
        input: json!({ "path": "README.md" }),
        properties: ToolProperties {
            is_concurrency_safe: true,
            is_read_only: true,
            is_destructive: false,
            long_running: Some(LongRunningPolicy {
                stall_threshold: Duration::from_secs(30),
                hard_timeout: Duration::from_secs(120),
            }),
            defer_policy: DeferPolicy::AlwaysLoad,
        },
        causation_id: EventId::new(),
        at: chrono::Utc::now(),
    };

    let value = serde_json::to_value(event).unwrap();
    assert!(value.get("properties").is_some());
    assert!(value.get("causation_id").is_some());
    assert!(value.get("session_id").is_none());
    assert!(value.get("origin").is_none());
}

#[test]
fn grace_call_does_not_default_required_fields() {
    let value = json!({
        "run_id": RunId::new(),
        "current_iteration": 4,
        "max_iterations": 5,
        "usage_snapshot": UsageSnapshot::default(),
        "at": chrono::Utc::now(),
        "correlation_id": CorrelationId::new(),
    });

    assert!(serde_json::from_value::<GraceCallTriggeredEvent>(value).is_err());
}

#[test]
fn message_and_reference_parts_keep_provider_native_contracts() {
    let thinking = ThinkingBlock {
        text: None,
        provider_id: "anthropic".to_owned(),
        provider_native: Some(json!({ "encrypted_content": "opaque" })),
        signature: Some("sig".to_owned()),
    };

    let part = MessagePart::Thinking(thinking.clone());
    let roundtrip: MessagePart =
        serde_json::from_value(serde_json::to_value(part).unwrap()).unwrap();
    assert_eq!(roundtrip, MessagePart::Thinking(thinking));

    let reference = ToolResultPart::Reference {
        reference_kind: ReferenceKind::Url {
            url: "https://example.test".to_owned(),
        },
        title: Some("example".to_owned()),
        summary: None,
    };
    let value = serde_json::to_value(reference).unwrap();
    assert_eq!(value["kind"], "reference");
    assert!(value.get("reference_kind").is_some());
}

#[test]
fn memory_lifecycle_views_are_public_contracts() {
    let _user = UserMessageView {
        text: "remember this preference",
        turn: 7,
        at: chrono::Utc::now(),
    };
    let _message = MessageView {
        role: MessageRole::Tool,
        text_snippet: "tool output",
        tool_use_id: Some(ToolUseId::new()),
    };
    let _summary = SessionSummaryView {
        end_reason: EndReason::Completed,
        turn_count: 3,
        tool_use_count: 1,
        usage: UsageSnapshot::default(),
        final_assistant_text: Some("done"),
    };
    let _ctx = MemorySessionCtx {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        workspace_id: Some(WorkspaceId::new()),
        user_id: Some("user-1"),
        team_id: Some(TeamId::new()),
    };
}

struct TestBlobStore;

#[async_trait::async_trait]
impl BlobStore for TestBlobStore {
    fn store_id(&self) -> &'static str {
        "test"
    }

    async fn put(
        &self,
        _tenant: TenantId,
        _bytes: Bytes,
        meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        Ok(BlobRef {
            id: BlobId::new(),
            size: meta.size,
            content_hash: meta.content_hash,
            content_type: meta.content_type,
        })
    }

    async fn get(
        &self,
        _tenant: TenantId,
        _blob: &BlobRef,
    ) -> Result<BoxStream<'static, Bytes>, BlobError> {
        Ok(Box::pin(futures::stream::once(async {
            Bytes::from_static(b"ok")
        })))
    }

    async fn head(&self, _tenant: TenantId, blob: &BlobRef) -> Result<Option<BlobMeta>, BlobError> {
        Ok(Some(BlobMeta {
            content_type: blob.content_type.clone(),
            size: blob.size,
            content_hash: blob.content_hash,
            created_at: chrono::Utc::now(),
            retention: BlobRetention::TenantScoped,
        }))
    }

    async fn delete(&self, _tenant: TenantId, _blob: &BlobRef) -> Result<(), BlobError> {
        Ok(())
    }
}

#[test]
fn blob_store_trait_is_async_and_object_safe() {
    let store: &dyn BlobStore = &TestBlobStore;
    let blob = futures::executor::block_on(store.put(
        TenantId::SINGLE,
        Bytes::from_static(b"ok"),
        BlobMeta {
            content_type: Some("text/plain".to_owned()),
            size: 2,
            content_hash: [7; 32],
            created_at: chrono::Utc::now(),
            retention: BlobRetention::TenantScoped,
        },
    ))
    .unwrap();

    assert_eq!(blob.size, 2);
    assert_eq!(store.store_id(), "test");
}

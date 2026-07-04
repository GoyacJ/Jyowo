#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

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
                Event::PermissionResolved(PermissionResolvedEvent {
                    request_id,
                    decision: Decision::AllowOnce,
                    decided_by: DecidedBy::User,
                    scope: DecisionScope::Any,
                    fingerprint: None,
                    rationale: None,
                    action_plan_hash: Default::default(),
                    decision_id: Default::default(),
                    auto_resolved: false,
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

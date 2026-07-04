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
                    revision_id: ArtifactRevisionId::new(),
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
async fn page_conversation_worktree_refs_fetch_bounded_evidence_pages() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let user_message_id = MessageId::new();
    let command_tool_use_id = ToolUseId::new();
    let diff_tool_use_id = ToolUseId::new();
    let revision_id = ArtifactRevisionId::new();
    let command_stdout = "line 1\nline 2";
    let command_stderr = "warning: still safe";
    let patch = "@@\n- old\n+ new\n+ another\n";
    let artifact_content = b"fn generated() -> &'static str { \"ready\" }\n".to_vec();
    let artifact_hash = blake3::hash(&artifact_content);
    let mut artifact_content_hash = [0u8; 32];
    artifact_content_hash.copy_from_slice(artifact_hash.as_bytes());

    open_conversation_session(&state, session_id).await;
    let artifact_blob_ref = state
        .harness()
        .expect("runtime harness should exist")
        .blob_store()
        .expect("test harness should expose blob store")
        .put(
            TenantId::SINGLE,
            bytes::Bytes::from(artifact_content.clone()),
            BlobMeta {
                content_type: Some("text/rust".to_owned()),
                size: artifact_content.len() as u64,
                content_hash: artifact_content_hash,
                created_at: now(),
                retention: BlobRetention::TenantScoped,
            },
        )
        .await
        .expect("artifact blob should be stored");

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
                    content: MessageContent::Text(
                        "run command, patch file, create artifact".to_owned(),
                    ),
                    metadata: MessageMetadata::default(),
                    attachments: Vec::new(),
                    at: now(),
                }),
                Event::ToolUseRequested(test_tool_use_requested_event(
                    run_id,
                    command_tool_use_id,
                    "shell",
                )),
                Event::ToolUseCompleted(ToolUseCompletedEvent {
                    tool_use_id: command_tool_use_id,
                    result: ToolResult::Structured(json!({
                        "exitCode": 0,
                        "stdout": command_stdout,
                        "stderr": command_stderr,
                    })),
                    usage: None,
                    duration_ms: 21,
                    at: now(),
                }),
                Event::ToolUseRequested(test_tool_use_requested_event(
                    run_id,
                    diff_tool_use_id,
                    "apply_patch",
                )),
                Event::ToolUseCompleted(ToolUseCompletedEvent {
                    tool_use_id: diff_tool_use_id,
                    result: ToolResult::Structured(json!({
                        "diff": {
                            "files": [
                                {
                                    "path": "src/lib.rs",
                                    "addedLines": 2,
                                    "removedLines": 1,
                                    "preview": "+ new",
                                    "patch": patch,
                                }
                            ]
                        }
                    })),
                    usage: None,
                    duration_ms: 12,
                    at: now(),
                }),
                Event::ArtifactCreated(ArtifactCreatedEvent {
                    revision_id,
                    artifact_id: "artifact-code".to_owned(),
                    at: now(),
                    blob_ref: Some(artifact_blob_ref.clone()),
                    content_hash: Some(artifact_content_hash.to_vec()),
                    kind: "code".to_owned(),
                    preview: Some("Generated Rust code".to_owned()),
                    run_id,
                    session_id,
                    source: ArtifactSource::Assistant,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: ArtifactStatus::Ready,
                    title: "generated.rs".to_owned(),
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

    assert!(!serialized.contains(command_stdout));
    assert!(!serialized.contains(command_stderr));
    assert!(!serialized.contains(patch));
    assert!(!serialized.contains(&String::from_utf8_lossy(&artifact_content).to_string()));
    assert!(!serialized.contains(&artifact_blob_ref.id.to_string()));

    let assistant = page.turns[0].assistant.as_ref().unwrap();
    let command_ref = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            harness_contracts::AssistantSegment::Process(process) => {
                process.steps.iter().find_map(|step| match &step.detail {
                    Some(harness_contracts::ProcessStepDetail::Command(command)) => {
                        command.full_output_ref.as_ref().map(ToString::to_string)
                    }
                    _ => None,
                })
            }
            _ => None,
        })
        .expect("command output ref should be projected");
    let diff_ref = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            harness_contracts::AssistantSegment::Process(process) => {
                process.steps.iter().find_map(|step| match &step.detail {
                    Some(harness_contracts::ProcessStepDetail::Diff(change_set)) => change_set
                        .files
                        .first()
                        .and_then(|file| file.full_patch_ref.as_ref())
                        .map(ToString::to_string),
                    _ => None,
                })
            }
            _ => None,
        })
        .expect("diff patch ref should be projected");
    let content_ref = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            harness_contracts::AssistantSegment::Artifact(artifact) => artifact
                .revision
                .content_ref
                .as_ref()
                .map(ToString::to_string),
            _ => None,
        })
        .expect("artifact content ref should be projected");

    let command_output = get_conversation_command_output_with_runtime_state(
        GetConversationCommandOutputRequest {
            conversation_id: session_id.to_string(),
            cursor: None,
            full_output_ref: command_ref,
            max_bytes: None,
        },
        &state,
    )
    .await
    .expect("command output evidence should fetch");
    assert_eq!(
        command_output.output,
        format!("{command_stdout}\n{command_stderr}")
    );
    assert_eq!(command_output.byte_length, command_output.content_bytes);
    assert_eq!(command_output.offset_bytes, 0);
    assert_eq!(command_output.limit_bytes, 65_536);
    assert_eq!(command_output.total_bytes, command_output.content_bytes);
    assert_eq!(command_output.returned_bytes, command_output.content_bytes);
    assert_eq!(command_output.hash_algorithm, "blake3");
    assert!(!command_output.content_hash.is_empty());
    assert!(!command_output.truncated);
    assert!(!command_output.has_more);
    assert_eq!(command_output.next_cursor, None);
    assert_eq!(command_output.redaction_state, "clean");

    let diff_patch = get_conversation_diff_patch_with_runtime_state(
        GetConversationDiffPatchRequest {
            conversation_id: session_id.to_string(),
            cursor: None,
            full_patch_ref: diff_ref,
            max_bytes: None,
        },
        &state,
    )
    .await
    .expect("diff patch evidence should fetch");
    assert_eq!(diff_patch.patch, patch);
    assert_eq!(diff_patch.byte_length, diff_patch.content_bytes);
    assert_eq!(diff_patch.offset_bytes, 0);
    assert_eq!(diff_patch.limit_bytes, 65_536);
    assert_eq!(diff_patch.total_bytes, diff_patch.content_bytes);
    assert_eq!(diff_patch.returned_bytes, diff_patch.content_bytes);
    assert_eq!(diff_patch.hash_algorithm, "blake3");
    assert!(!diff_patch.content_hash.is_empty());
    assert!(!diff_patch.truncated);
    assert!(!diff_patch.has_more);
    assert_eq!(diff_patch.next_cursor, None);
    assert_eq!(diff_patch.redaction_state, "clean");

    let artifact = get_artifact_revision_content_with_runtime_state(
        GetArtifactRevisionContentRequest {
            content_ref,
            conversation_id: session_id.to_string(),
            cursor: None,
            max_bytes: None,
        },
        &state,
    )
    .await
    .expect("artifact content evidence should fetch");
    assert_eq!(artifact.content.as_bytes(), artifact_content.as_slice());
    assert_eq!(artifact.content_type, "text/rust");
    assert_eq!(artifact.byte_length, artifact.content_bytes);
    assert_eq!(artifact.offset_bytes, 0);
    assert_eq!(artifact.limit_bytes, 65_536);
    assert_eq!(artifact.total_bytes, artifact.content_bytes);
    assert_eq!(artifact.returned_bytes, artifact.content_bytes);
    assert_eq!(artifact.hash_algorithm, "blake3");
    assert!(!artifact.content_hash.is_empty());
    assert!(!artifact.truncated);
    assert!(!artifact.has_more);
    assert_eq!(artifact.next_cursor, None);
    assert_eq!(artifact.redaction_state, "clean");

    let command_page = get_conversation_command_output_with_runtime_state(
        GetConversationCommandOutputRequest {
            conversation_id: session_id.to_string(),
            cursor: None,
            full_output_ref: command_output.ref_id.clone(),
            max_bytes: Some(8),
        },
        &state,
    )
    .await
    .expect("command output evidence page should fetch");
    assert_eq!(command_page.output, "line 1\nl");
    assert_eq!(command_page.content_bytes, 33);
    assert_eq!(command_page.offset_bytes, 0);
    assert_eq!(command_page.limit_bytes, 8);
    assert_eq!(command_page.total_bytes, 33);
    assert_eq!(command_page.returned_bytes, 8);
    assert_eq!(command_page.max_bytes, 8);
    assert!(command_page.truncated);
    assert!(command_page.has_more);
    assert_eq!(command_page.next_cursor.as_deref(), Some("8"));

    let exported = export_conversation_evidence_with_runtime_state(
        ExportConversationEvidenceRequest {
            conversation_id: session_id.to_string(),
            kind: "command-output".to_owned(),
            ref_id: command_output.ref_id.clone(),
        },
        &state,
    )
    .await
    .expect("command output evidence should export through backend");
    assert_eq!(exported.ref_id, command_output.ref_id);
    assert_eq!(exported.kind, "command-output");
    assert_eq!(exported.byte_length, command_output.content_bytes);
    assert_eq!(exported.content_type, command_output.content_type);
    assert!(exported.path.starts_with(".jyowo/runtime/exports/"));
    let exported_content =
        std::fs::read_to_string(state.workspace_root().join(&exported.path)).unwrap();
    assert_eq!(
        exported_content,
        format!("{command_stdout}\n{command_stderr}")
    );
}

#[tokio::test]
async fn export_conversation_evidence_with_runtime_state_writes_multiple_bounded_windows() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let user_message_id = MessageId::new();
    let tool_use_id = ToolUseId::new();
    let large_stdout = "0123456789abcdef".repeat(5_000);

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
                    content: MessageContent::Text("run large command".to_owned()),
                    metadata: MessageMetadata::default(),
                    attachments: Vec::new(),
                    at: now(),
                }),
                Event::ToolUseRequested(test_tool_use_requested_event(
                    run_id,
                    tool_use_id,
                    "shell",
                )),
                Event::ToolUseCompleted(ToolUseCompletedEvent {
                    tool_use_id,
                    result: ToolResult::Structured(json!({
                        "exitCode": 0,
                        "stdout": large_stdout,
                        "stderr": "",
                    })),
                    usage: None,
                    duration_ms: 21,
                    at: now(),
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
    let assistant = page.turns[0].assistant.as_ref().unwrap();
    let command_ref = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            harness_contracts::AssistantSegment::Process(process) => {
                process.steps.iter().find_map(|step| match &step.detail {
                    Some(harness_contracts::ProcessStepDetail::Command(command)) => {
                        command.full_output_ref.as_ref().map(ToString::to_string)
                    }
                    _ => None,
                })
            }
            _ => None,
        })
        .expect("command output ref should be projected");

    let first_page = get_conversation_command_output_with_runtime_state(
        GetConversationCommandOutputRequest {
            conversation_id: session_id.to_string(),
            cursor: None,
            full_output_ref: command_ref.clone(),
            max_bytes: None,
        },
        &state,
    )
    .await
    .expect("first evidence page should fetch");
    assert_eq!(first_page.returned_bytes, 65_536);
    assert_eq!(first_page.total_bytes, large_stdout.len() as u64);
    assert!(first_page.has_more);
    assert_eq!(first_page.next_cursor.as_deref(), Some("65536"));

    let exported = export_conversation_evidence_with_runtime_state(
        ExportConversationEvidenceRequest {
            conversation_id: session_id.to_string(),
            kind: "command-output".to_owned(),
            ref_id: command_ref,
        },
        &state,
    )
    .await
    .expect("large command output should export through bounded windows");
    assert_eq!(exported.byte_length, large_stdout.len() as u64);
    assert!(exported.path.starts_with(".jyowo/runtime/exports/"));
    let exported_content =
        std::fs::read_to_string(state.workspace_root().join(&exported.path)).unwrap();
    assert_eq!(exported_content, large_stdout);
}

#[tokio::test]
async fn export_conversation_evidence_with_runtime_state_rejects_invalid_kind() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    open_conversation_session(&state, session_id).await;

    let error = export_conversation_evidence_with_runtime_state(
        ExportConversationEvidenceRequest {
            conversation_id: session_id.to_string(),
            kind: "raw-event".to_owned(),
            ref_id: "evidence-command-output-001".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("invalid export kind should be rejected before evidence read");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(
        error.message.contains("evidence export kind"),
        "unexpected error message: {}",
        error.message
    );
}

#[tokio::test]
async fn export_conversation_evidence_with_runtime_state_rejects_missing_ref() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    open_conversation_session(&state, session_id).await;

    let error = export_conversation_evidence_with_runtime_state(
        ExportConversationEvidenceRequest {
            conversation_id: session_id.to_string(),
            kind: "command-output".to_owned(),
            ref_id: "evidence-command-output-missing".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("missing export ref should fail closed");

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");
    assert!(
        error.message.contains("evidence ref not found"),
        "unexpected error message: {}",
        error.message
    );
}

#[tokio::test]
async fn get_conversation_evidence_with_runtime_state_rejects_invalid_ref() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    open_conversation_session(&state, session_id).await;

    let error = get_conversation_command_output_with_runtime_state(
        GetConversationCommandOutputRequest {
            conversation_id: session_id.to_string(),
            cursor: None,
            full_output_ref: "evidence:command-output:missing:00000000".to_owned(),
            max_bytes: None,
        },
        &state,
    )
    .await
    .expect_err("invalid evidence ref should fail closed");

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");
    assert!(
        error.message.contains("evidence ref not found"),
        "unexpected error message: {}",
        error.message
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

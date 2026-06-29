use harness_contracts::*;
use harness_journal::project_conversation_worktree_snapshot;
use serde_json::json;

fn event(
    sequence: u64,
    id: &str,
    run_id: &str,
    event_type: &str,
    payload: serde_json::Value,
) -> ConversationTimelineEvent {
    ConversationTimelineEvent {
        id: id.to_owned(),
        cursor: ConversationCursor {
            event_id: EventId::new(),
            conversation_sequence: sequence,
        },
        payload,
        run_id: run_id.to_owned(),
        sequence,
        source: "assistant".to_owned(),
        timestamp: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH
            + chrono::Duration::seconds(sequence as i64),
        event_type: event_type.to_owned(),
        visibility: "public".to_owned(),
    }
}

fn user_event(
    sequence: u64,
    id: &str,
    run_id: &str,
    message_id: &str,
    body: &str,
) -> ConversationTimelineEvent {
    let mut event = event(
        sequence,
        id,
        run_id,
        "user.message.appended",
        json!({
            "messageId": message_id,
            "body": body,
            "clientMessageId": "client-1",
        }),
    );
    event.source = "user".to_owned();
    event
}

fn user_event_with_attachment(
    sequence: u64,
    id: &str,
    run_id: &str,
    message_id: &str,
    body: &str,
) -> ConversationTimelineEvent {
    let blob_id = BlobId::new().to_string();
    let mut event = event(
        sequence,
        id,
        run_id,
        "user.message.appended",
        json!({
            "messageId": message_id,
            "body": body,
            "attachments": [
                {
                    "id": "attachment-001",
                    "name": "notes.txt",
                    "mimeType": "text/plain",
                    "sizeBytes": 128,
                    "blobRef": {
                        "id": blob_id,
                        "size": 128,
                        "contentHash": [7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7],
                        "contentType": "text/plain"
                    }
                }
            ],
        }),
    );
    event.source = "user".to_owned();
    event
}

#[test]
fn user_message_attachments_project_to_turn_user() {
    let events = vec![user_event_with_attachment(
        1,
        "event-user",
        "run-1",
        "user-1",
        "Summarize the attachment",
    )];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let attachment = &projection.turns[0].user.attachments[0];

    assert_eq!(attachment.id, "attachment-001");
    assert_eq!(attachment.name, "notes.txt");
    assert_eq!(attachment.mime_type, "text/plain");
    assert_eq!(attachment.size_bytes, 128);
    assert_eq!(attachment.blob_ref.size, 128);
}

fn assistant_completed(
    sequence: u64,
    id: &str,
    run_id: &str,
    message_id: &str,
    body: &str,
) -> ConversationTimelineEvent {
    event(
        sequence,
        id,
        run_id,
        "assistant.completed",
        json!({ "messageId": message_id, "body": body }),
    )
}

fn assistant_completed_with_tool_uses(
    sequence: u64,
    id: &str,
    run_id: &str,
    message_id: &str,
    body: &str,
    tool_use_id: &str,
) -> ConversationTimelineEvent {
    event(
        sequence,
        id,
        run_id,
        "assistant.completed",
        json!({
            "messageId": message_id,
            "body": body,
            "toolUses": [
                {
                    "id": tool_use_id,
                    "name": "read_file"
                }
            ],
        }),
    )
}

fn tool_requested(
    sequence: u64,
    id: &str,
    run_id: &str,
    tool_use_id: &str,
) -> ConversationTimelineEvent {
    event(
        sequence,
        id,
        run_id,
        "tool.requested",
        json!({
            "toolUseId": tool_use_id,
            "toolName": "MiniMaxTextToImage",
            "argumentsSummary": "Input withheld from conversation timeline.",
        }),
    )
}

fn tool_group(segment: &AssistantSegment) -> Option<&ToolGroupSegment> {
    match segment {
        AssistantSegment::ToolGroup(group) => Some(group),
        _ => None,
    }
}

fn process_segment(segment: &AssistantSegment) -> Option<&ProcessSegment> {
    match segment {
        AssistantSegment::Process(process) => Some(process),
        _ => None,
    }
}

fn text_segment(segment: &AssistantSegment) -> Option<&TextSegment> {
    match segment {
        AssistantSegment::Text(text) => Some(text),
        _ => None,
    }
}

fn artifact_segment(segment: &AssistantSegment) -> Option<&ArtifactSegment> {
    match segment {
        AssistantSegment::Artifact(artifact) => Some(artifact),
        _ => None,
    }
}

fn review_segment(segment: &AssistantSegment) -> Option<&ReviewRequestSegment> {
    match segment {
        AssistantSegment::ReviewRequest(review) => Some(review),
        _ => None,
    }
}

fn clarification_segment(segment: &AssistantSegment) -> Option<&ClarificationRequestSegment> {
    match segment {
        AssistantSegment::ClarificationRequest(clarification) => Some(clarification),
        _ => None,
    }
}

fn notice_segment(segment: &AssistantSegment) -> Option<&NoticeSegment> {
    match segment {
        AssistantSegment::Notice(notice) => Some(notice),
        _ => None,
    }
}

#[test]
fn projects_one_user_prompt_and_tool_loop_into_one_assistant_work_tree() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "帮我生成一张图"),
        assistant_completed(2, "event-assistant-1", "run-1", "assistant-1", "当然可以。"),
        tool_requested(3, "event-tool-a", "run-1", "tool-a"),
        event(
            4,
            "event-permission-requested",
            "run-1",
            "permission.requested",
            json!({
                "requestId": "request-a",
                "toolUseId": "tool-a",
                "reason": "The runtime requires approval before continuing.",
            }),
        ),
        event(
            5,
            "event-permission-resolved",
            "run-1",
            "permission.resolved",
            json!({ "requestId": "request-a", "decision": "approve" }),
        ),
        event(
            6,
            "event-tool-failed",
            "run-1",
            "tool.failed",
            json!({
                "toolUseId": "tool-a",
                "message": "Tool error withheld from conversation timeline.",
            }),
        ),
        assistant_completed(
            7,
            "event-assistant-2",
            "run-1",
            "assistant-2",
            "非常抱歉，目前工具不可用。",
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);

    assert_eq!(projection.turns.len(), 1);
    let turn = &projection.turns[0];
    assert_eq!(turn.id, "turn:user-1");
    assert_eq!(turn.user.body.as_str(), "帮我生成一张图");

    let assistant = turn.assistant.as_ref().expect("assistant work exists");
    assert_eq!(assistant.id, "assistant:run-1");
    assert_eq!(assistant.status, AssistantWorkStatus::Running);
    assert_eq!(
        assistant.segments.iter().filter_map(text_segment).count(),
        2
    );

    let group = assistant
        .segments
        .iter()
        .find_map(tool_group)
        .expect("tool group exists");
    assert_eq!(group.id, "segment:tools:tool-a");
    assert_eq!(group.attempts.len(), 1);
    assert_eq!(group.attempts[0].id, "tool:tool-a");
    assert_eq!(group.attempts[0].order, 0);
    assert_eq!(group.attempts[0].status, ToolAttemptStatus::Failed);
    assert_eq!(
        group.attempts[0].permission.as_ref().unwrap().status,
        ToolPermissionStatus::Approved
    );
    assert_eq!(group.attempts[0].permission.as_ref().unwrap().summary, None);
    assert_eq!(
        group.attempts[0].failure_summary.as_ref().unwrap().as_str(),
        "工具执行失败。可在详情中查看。"
    );
}

#[test]
fn pending_permission_preserves_request_summary() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "use tool"),
        tool_requested(2, "event-tool-a", "run-1", "tool-a"),
        event(
            3,
            "event-permission-requested",
            "run-1",
            "permission.requested",
            json!({
                "requestId": "request-a",
                "toolUseId": "tool-a",
                "reason": "需要批准后才能继续。",
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let group = assistant
        .segments
        .iter()
        .find_map(tool_group)
        .expect("tool group exists");
    let permission = group.attempts[0].permission.as_ref().unwrap();

    assert_eq!(permission.status, ToolPermissionStatus::Pending);
    assert_eq!(
        permission.summary.as_ref().map(|summary| summary.as_str()),
        Some("需要批准后才能继续。")
    );
}

#[test]
fn completed_tool_attempt_remains_projected_for_timeline_grouping() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "read file"),
        tool_requested(2, "event-tool-requested", "run-1", "tool-a"),
        event(
            3,
            "event-tool-completed",
            "run-1",
            "tool.completed",
            json!({
                "toolUseId": "tool-a",
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let group = assistant
        .segments
        .iter()
        .find_map(tool_group)
        .expect("completed tool group should stay visible to the timeline");

    assert_eq!(group.attempts.len(), 1);
    assert_eq!(group.attempts[0].status, ToolAttemptStatus::Completed);
}

#[test]
fn completed_run_with_failed_process_step_projects_mixed_failure_summary() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "run checks"),
        tool_requested(2, "event-tool-requested", "run-1", "tool-a"),
        event(
            3,
            "event-tool-failed",
            "run-1",
            "tool.failed",
            json!({
                "toolUseId": "tool-a",
                "message": "Tool error withheld from conversation timeline.",
            }),
        ),
        event(
            4,
            "event-run-ended",
            "run-1",
            "run.ended",
            json!({ "reason": "completed" }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    assert_eq!(assistant.status, AssistantWorkStatus::Complete);

    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process segment should exist");
    assert_eq!(process.status, ProcessSegmentStatus::Failed);
    assert_eq!(process.summary.as_str(), "已结束但存在失败步骤");
}

#[test]
fn merges_late_user_message_into_synthetic_turn_for_assistant_first_events() {
    let events = vec![
        event(
            1,
            "event-delta",
            "run-1",
            "assistant.delta",
            json!({ "messageId": "assistant-1", "text": "你好！" }),
        ),
        user_event(2, "event-user", "run-1", "user-1", "你好"),
        assistant_completed(
            3,
            "event-assistant",
            "run-1",
            "assistant-1",
            "你好，我可以继续处理。",
        ),
        event(
            4,
            "event-run-ended",
            "run-1",
            "run.ended",
            json!({ "reason": "completed" }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);

    assert_eq!(projection.turns.len(), 1);
    let turn = &projection.turns[0];
    assert_eq!(turn.id, "turn:user-1");
    assert_eq!(turn.position, 2);
    assert_eq!(turn.user.id, "user:user-1");
    assert_eq!(turn.user.message_id, "user-1");
    assert_eq!(turn.user.body.as_str(), "你好");
    assert_eq!(turn.user.client_message_id.as_deref(), Some("client-1"));
    assert_eq!(turn.user.event_refs.len(), 1);

    let assistant = turn.assistant.as_ref().expect("assistant work exists");
    assert_eq!(assistant.status, AssistantWorkStatus::Complete);
    let text_segments = assistant
        .segments
        .iter()
        .filter_map(text_segment)
        .collect::<Vec<_>>();
    assert_eq!(text_segments.len(), 1);
    assert_eq!(text_segments[0].message_id, "assistant-1");
    assert_eq!(text_segments[0].body.as_str(), "你好，我可以继续处理。");
    assert_eq!(text_segments[0].event_refs.len(), 2);
}

#[test]
fn engine_failed_projects_generic_failure_without_activity_label() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "run task"),
        event(
            2,
            "event-engine-failed",
            "run-1",
            "engine.failed",
            json!({
                "code": "runtime_error",
                "message": "raw runtime failure",
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let error = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            AssistantSegment::Error(error) => Some(error),
            _ => None,
        })
        .expect("error segment should be projected");

    assert_eq!(error.body.as_str(), "执行失败。可在详情中查看。");
}

#[test]
fn assistant_deltas_with_same_message_id_merge_into_one_text_segment() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "hello"),
        event(
            2,
            "event-delta-1",
            "run-1",
            "assistant.delta",
            json!({ "messageId": "assistant-1", "text": "Hello " }),
        ),
        event(
            3,
            "event-delta-2",
            "run-1",
            "assistant.delta",
            json!({ "messageId": "assistant-1", "text": "world" }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let text_segments = assistant
        .segments
        .iter()
        .filter_map(text_segment)
        .collect::<Vec<_>>();

    assert_eq!(text_segments.len(), 1);
    assert_eq!(text_segments[0].message_id, "assistant-1");
    assert_eq!(text_segments[0].body.as_str(), "Hello world");
    assert_eq!(text_segments[0].event_refs.len(), 2);
}

#[test]
fn assistant_completed_finalizes_existing_delta_segment_without_duplicate_text() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "hello"),
        event(
            2,
            "event-delta",
            "run-1",
            "assistant.delta",
            json!({ "messageId": "assistant-1", "text": "partial" }),
        ),
        assistant_completed(3, "event-completed", "run-1", "assistant-1", "final answer"),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let text_segments = assistant
        .segments
        .iter()
        .filter_map(text_segment)
        .collect::<Vec<_>>();

    assert_eq!(text_segments.len(), 1);
    assert_eq!(text_segments[0].message_id, "assistant-1");
    assert_eq!(text_segments[0].body.as_str(), "final answer");
    assert_eq!(text_segments[0].event_refs.len(), 2);
}

#[test]
fn empty_assistant_completed_does_not_clear_existing_delta_text() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "hello"),
        event(
            2,
            "event-delta",
            "run-1",
            "assistant.delta",
            json!({ "messageId": "assistant-1", "text": "partial" }),
        ),
        assistant_completed(3, "event-completed", "run-1", "assistant-1", ""),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let text_segments = assistant
        .segments
        .iter()
        .filter_map(text_segment)
        .collect::<Vec<_>>();

    assert_eq!(text_segments.len(), 1);
    assert_eq!(text_segments[0].message_id, "assistant-1");
    assert_eq!(text_segments[0].body.as_str(), "partial");
}

#[test]
fn safe_summary_delta_projects_into_process_reasoning_step() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "think"),
        event(
            2,
            "event-thinking-1",
            "run-1",
            "assistant.thinking.delta",
            json!({ "safeSummaryDelta": "Checked ", "status": "running" }),
        ),
        event(
            3,
            "event-thinking-2",
            "run-1",
            "assistant.thinking.delta",
            json!({ "safeSummaryDelta": "project context.", "status": "running" }),
        ),
        event(
            4,
            "event-run-ended",
            "run-1",
            "run.ended",
            json!({ "reason": "completed" }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");

    assert_eq!(process.status, ProcessSegmentStatus::Complete);
    assert_eq!(process.summary.as_str(), "已完成工作过程");
    assert_eq!(process.steps.len(), 1);
    assert_eq!(process.steps[0].kind, ProcessStepKind::Reasoning);
    assert_eq!(process.steps[0].status, ProcessStepStatus::Complete);
    assert_eq!(
        process.steps[0].body.as_ref().unwrap().as_str(),
        "Checked project context."
    );
    assert!(assistant
        .segments
        .iter()
        .all(|segment| !matches!(segment, AssistantSegment::Thinking(_))));
}

#[test]
fn assistant_completed_with_tool_uses_becomes_process_reasoning_not_final_text() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "use tool"),
        event(
            2,
            "event-delta",
            "run-1",
            "assistant.delta",
            json!({ "messageId": "assistant-tools", "text": "I need to inspect files." }),
        ),
        assistant_completed_with_tool_uses(
            3,
            "event-assistant-tools",
            "run-1",
            "assistant-tools",
            "I need to inspect files.",
            "tool-1",
        ),
        assistant_completed(
            4,
            "event-assistant-final",
            "run-1",
            "assistant-final",
            "最终回答。",
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");
    let text_segments = assistant
        .segments
        .iter()
        .filter_map(text_segment)
        .collect::<Vec<_>>();

    assert_eq!(text_segments.len(), 1);
    assert_eq!(text_segments[0].message_id, "assistant-final");
    assert_eq!(text_segments[0].body.as_str(), "最终回答。");
    assert!(process.steps.iter().any(|step| {
        step.kind == ProcessStepKind::Reasoning
            && step
                .body
                .as_ref()
                .is_some_and(|body| body.as_str() == "I need to inspect files.")
    }));
}

#[test]
fn ready_image_artifact_suppresses_redacted_only_final_text_and_keeps_media_metadata() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "generate image"),
        assistant_completed(
            2,
            "event-redacted-final",
            "run-1",
            "assistant-redacted",
            "[REDACTED]",
        ),
        event(
            3,
            "event-artifact-created",
            "run-1",
            "artifact.created",
            json!({
                "artifactId": "artifact-image",
                "title": "生成的图片",
                "summary": "图片已生成。",
                "kind": "image",
                "status": "ready",
                "source": "tool",
                "media": {
                    "kind": "image",
                    "mimeType": "image/png",
                    "sizeBytes": 256
                }
            }),
        ),
        event(
            4,
            "event-run-ended",
            "run-1",
            "run.ended",
            json!({ "reason": "completed" }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    assert_eq!(
        assistant.segments.iter().filter_map(text_segment).count(),
        0
    );
    assert_eq!(
        assistant
            .segments
            .iter()
            .filter_map(artifact_segment)
            .count(),
        0
    );
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");
    let artifact_step = process
        .steps
        .iter()
        .find(|step| step.kind == ProcessStepKind::Artifact)
        .expect("artifact step exists");
    let Some(ProcessStepDetail::Artifact { media, .. }) = &artifact_step.detail else {
        panic!("artifact step should include media");
    };
    assert_eq!(media.kind, ArtifactMediaKind::Image);
    assert_eq!(media.mime_type, "image/png");
}

#[test]
fn assistant_deltas_with_different_message_ids_remain_separate_text_segments() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "hello"),
        event(
            2,
            "event-delta-1",
            "run-1",
            "assistant.delta",
            json!({ "messageId": "assistant-1", "text": "first" }),
        ),
        event(
            3,
            "event-delta-2",
            "run-1",
            "assistant.delta",
            json!({ "messageId": "assistant-2", "text": "second" }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let text_segments = assistant
        .segments
        .iter()
        .filter_map(text_segment)
        .collect::<Vec<_>>();

    assert_eq!(text_segments.len(), 2);
    assert_eq!(text_segments[0].message_id, "assistant-1");
    assert_eq!(text_segments[1].message_id, "assistant-2");
    assert_eq!(text_segments[0].body.as_str(), "first");
    assert_eq!(text_segments[1].body.as_str(), "second");
}

#[test]
fn turns_remain_sorted_when_late_user_message_updates_synthetic_position() {
    let events = vec![
        event(
            1,
            "event-run-a-delta",
            "run-a",
            "assistant.delta",
            json!({ "text": "late assistant" }),
        ),
        user_event(2, "event-user-b", "run-b", "user-b", "visible second"),
        assistant_completed(3, "event-assistant-b", "run-b", "assistant-b", "done b"),
        event(
            4,
            "event-run-b-ended",
            "run-b",
            "run.ended",
            json!({ "reason": "completed" }),
        ),
        user_event(5, "event-user-a", "run-a", "user-a", "late first"),
        assistant_completed(6, "event-assistant-a", "run-a", "assistant-a", "done a"),
        event(
            7,
            "event-run-a-ended",
            "run-a",
            "run.ended",
            json!({ "reason": "completed" }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);

    assert_eq!(
        projection
            .turns
            .iter()
            .map(|turn| (turn.position, turn.user.body.as_str()))
            .collect::<Vec<_>>(),
        vec![(2, "visible second"), (5, "late first")]
    );
    assert!(projection.turns.iter().all(|turn| {
        turn.assistant
            .as_ref()
            .is_some_and(|assistant| assistant.status == AssistantWorkStatus::Complete)
    }));
}

#[test]
fn permission_request_without_tool_use_id_does_not_bind_to_latest_tool() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "run tools"),
        tool_requested(2, "event-tool-a", "run-1", "tool-a"),
        tool_requested(3, "event-tool-b", "run-1", "tool-b"),
        event(
            4,
            "event-permission-requested",
            "run-1",
            "permission.requested",
            json!({
                "requestId": "request-a",
                "reason": "The runtime requires approval before continuing.",
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0]
        .assistant
        .as_ref()
        .expect("assistant work exists");
    let group = assistant
        .segments
        .iter()
        .find_map(tool_group)
        .expect("tool group exists");

    assert_eq!(group.attempts.len(), 2);
    assert!(group
        .attempts
        .iter()
        .all(|attempt| attempt.permission.is_none()));
}

#[test]
fn permission_request_without_tool_use_id_binds_to_unique_tool() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "run tool"),
        tool_requested(2, "event-tool-a", "run-1", "tool-a"),
        event(
            3,
            "event-permission-requested",
            "run-1",
            "permission.requested",
            json!({
                "requestId": "request-a",
                "reason": "The runtime requires approval before continuing.",
            }),
        ),
        event(
            4,
            "event-permission-resolved",
            "run-1",
            "permission.resolved",
            json!({
                "requestId": "request-a",
                "decision": "approve",
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0]
        .assistant
        .as_ref()
        .expect("assistant work exists");
    let group = assistant
        .segments
        .iter()
        .find_map(tool_group)
        .expect("tool group exists");
    let attempt = group.attempts.first().expect("tool attempt exists");

    assert_eq!(attempt.tool_use_id, "tool-a");
    assert_eq!(attempt.status, ToolAttemptStatus::Running);
    assert_eq!(
        attempt.permission.as_ref().unwrap().status,
        ToolPermissionStatus::Approved
    );
}

#[test]
fn permission_request_with_tool_use_id_binds_to_matching_tool_not_latest_tool() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "run tools"),
        tool_requested(2, "event-tool-a", "run-1", "tool-a"),
        tool_requested(3, "event-tool-b", "run-1", "tool-b"),
        event(
            4,
            "event-permission-requested",
            "run-1",
            "permission.requested",
            json!({
                "requestId": "request-a",
                "toolUseId": "tool-a",
                "reason": "The runtime requires approval before continuing.",
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0]
        .assistant
        .as_ref()
        .expect("assistant work exists");
    let group = assistant
        .segments
        .iter()
        .find_map(tool_group)
        .expect("tool group exists");

    let tool_a = group
        .attempts
        .iter()
        .find(|attempt| attempt.tool_use_id == "tool-a")
        .expect("tool-a attempt exists");
    let tool_b = group
        .attempts
        .iter()
        .find(|attempt| attempt.tool_use_id == "tool-b")
        .expect("tool-b attempt exists");

    assert_eq!(
        tool_a.permission.as_ref().unwrap().request_id.as_str(),
        "request-a"
    );
    assert!(tool_b.permission.is_none());
}

#[test]
fn tool_call_only_assistant_completed_does_not_create_empty_text_segment() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "run tool"),
        assistant_completed(2, "event-empty-assistant", "run-1", "assistant-empty", ""),
        tool_requested(3, "event-tool-a", "run-1", "tool-a"),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();

    assert_eq!(
        assistant.segments.iter().filter_map(text_segment).count(),
        0
    );
}

#[test]
fn artifact_events_create_and_update_one_safe_artifact_segment() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "generate"),
        event(
            2,
            "event-artifact-created",
            "run-1",
            "artifact.created",
            json!({
                "artifactId": "artifact-1",
                "title": "Generated image",
                "summary": "Image artifact ready",
                "blobRef": "blob-secret",
                "contentHash": "hash-secret",
            }),
        ),
        event(
            3,
            "event-artifact-updated",
            "run-1",
            "artifact.updated",
            json!({
                "artifactId": "artifact-1",
                "title": "Updated image",
                "summary": "Preview refreshed",
                "privatePath": "/Users/goya/private/generated.png",
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let artifacts = assistant
        .segments
        .iter()
        .filter_map(artifact_segment)
        .collect::<Vec<_>>();

    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].id, "segment:artifact:artifact-1");
    assert_eq!(artifacts[0].order, 0);
    assert_eq!(artifacts[0].artifact_id, "artifact-1");
    assert_eq!(artifacts[0].title.as_str(), "Updated image");
    assert_eq!(
        artifacts[0].summary.as_ref().unwrap().as_str(),
        "Preview refreshed"
    );
    assert_eq!(artifacts[0].event_refs.len(), 2);
    let serialized = serde_json::to_value(artifacts[0]).unwrap();
    assert!(serialized.get("blobRef").is_none());
    assert!(serialized.get("contentHash").is_none());
    assert!(serialized.get("privatePath").is_none());
}

#[test]
fn review_clarification_and_notice_events_create_ordered_segments() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "continue"),
        event(
            2,
            "event-thinking",
            "run-1",
            "assistant.thinking.delta",
            json!({ "safeSummary": "Checking state", "status": "running" }),
        ),
        event(
            3,
            "event-review",
            "run-1",
            "assistant.review.requested",
            json!({
                "requestId": "review-1",
                "title": "Review changes",
                "body": "Confirm before applying.",
            }),
        ),
        event(
            4,
            "event-clarification",
            "run-1",
            "assistant.clarification.requested",
            json!({
                "requestId": "clarification-1",
                "prompt": "Which style should I use?",
            }),
        ),
        event(
            5,
            "event-notice",
            "run-1",
            "assistant.notice",
            json!({
                "noticeId": "notice-1",
                "body": "Tool output was summarized.",
                "code": "contextCompacted",
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let review = assistant
        .segments
        .iter()
        .find_map(review_segment)
        .expect("review segment exists");
    let clarification = assistant
        .segments
        .iter()
        .find_map(clarification_segment)
        .expect("clarification segment exists");
    let notice = assistant
        .segments
        .iter()
        .find_map(notice_segment)
        .expect("notice segment exists");

    assert_eq!(review.id, "segment:review:review-1");
    assert_eq!(review.order, 1);
    assert_eq!(review.request_id, "review-1");
    assert_eq!(review.title.as_str(), "Review changes");
    assert_eq!(
        review.body.as_ref().unwrap().as_str(),
        "Confirm before applying."
    );
    assert_eq!(review.event_refs[0].event_id, "event-review");
    assert_eq!(clarification.id, "segment:clarification:clarification-1");
    assert_eq!(clarification.order, 2);
    assert_eq!(clarification.prompt.as_str(), "Which style should I use?");
    assert_eq!(notice.id, "segment:notice:notice-1");
    assert_eq!(notice.order, 3);
    assert_eq!(notice.body.as_str(), "Tool output was summarized.");
    assert_eq!(notice.code, Some(AssistantNoticeCode::ContextCompacted));
}

#[test]
fn review_clarification_notice_and_artifact_segments_redact_unsafe_payload_text() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "continue"),
        event(
            2,
            "event-artifact",
            "run-1",
            "artifact.created",
            json!({
                "artifactId": "artifact-1",
                "title": "Artifact at /Users/example/private/out.md",
                "summary": "summary token=synthetic-token",
            }),
        ),
        event(
            3,
            "event-review",
            "run-1",
            "assistant.review.requested",
            json!({
                "requestId": "review-1",
                "title": "Review Authorization: Bearer synthetic-token",
                "body": "Confirm /home/example/private.",
            }),
        ),
        event(
            4,
            "event-clarification",
            "run-1",
            "assistant.clarification.requested",
            json!({
                "requestId": "clarification-1",
                "prompt": "Which value for sk-synthetic?",
            }),
        ),
        event(
            5,
            "event-notice",
            "run-1",
            "assistant.notice",
            json!({
                "noticeId": "notice-1",
                "body": "Read /private/var/example/cache.",
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let artifact = assistant
        .segments
        .iter()
        .find_map(artifact_segment)
        .expect("artifact segment exists");
    let review = assistant
        .segments
        .iter()
        .find_map(review_segment)
        .expect("review segment exists");
    let clarification = assistant
        .segments
        .iter()
        .find_map(clarification_segment)
        .expect("clarification segment exists");
    let notice = assistant
        .segments
        .iter()
        .find_map(notice_segment)
        .expect("notice segment exists");

    assert_eq!(artifact.title.as_str(), "Artifact at [REDACTED]");
    assert_eq!(
        artifact.summary.as_ref().unwrap().as_str(),
        "summary [REDACTED]"
    );
    assert_eq!(
        review.title.as_str(),
        "Review [REDACTED] [REDACTED] [REDACTED]"
    );
    assert_eq!(review.body.as_ref().unwrap().as_str(), "Confirm [REDACTED]");
    assert_eq!(clarification.prompt.as_str(), "Which value for [REDACTED]");
    assert_eq!(notice.body.as_str(), "Read [REDACTED]");
}

#[test]
fn public_segments_redact_urls_and_blob_paths() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "continue"),
        event(
            2,
            "event-reasoning",
            "run-1",
            "assistant.thinking.delta",
            json!({
                "safeSummaryDelta": "Checked https://provider.example/image，链接https://provider.example/tight and 路径：.jyowo/runtime/blobs/blob-001 log/tmp/provider-output",
                "status": "running",
            }),
        ),
        event(
            3,
            "event-artifact",
            "run-1",
            "artifact.created",
            json!({
                "artifactId": "artifact-1",
                "kind": "image javascript:alert(1)",
                "title": "Image at https://provider.example/image data:image/svg+xml,<svg onload=alert(1)>。",
                "summary": "Blob path .jyowo/runtime/blobs/blob-001 .JYOWO/runtime/blobs/blob-002 blob:null/provider",
            }),
        ),
        event(
            4,
            "event-review",
            "run-1",
            "assistant.review.requested",
            json!({
                "requestId": "review-1",
                "title": "Review https://provider.example/review",
                "body": "Confirm blob:.jyowo/runtime/blobs/blob-001",
            }),
        ),
        event(
            5,
            "event-clarification",
            "run-1",
            "assistant.clarification.requested",
            json!({
                "requestId": "clarification-1",
                "prompt": "Use链接https://provider.example/prompt",
            }),
        ),
        event(
            6,
            "event-notice",
            "run-1",
            "assistant.notice",
            json!({
                "noticeId": "notice-1",
                "body": "Read 路径：.jyowo/runtime/blobs/blob-001 and .JYOWO/runtime/blobs/blob-002",
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process segment exists");
    let artifact = assistant
        .segments
        .iter()
        .find_map(artifact_segment)
        .expect("artifact segment exists");
    let review = assistant
        .segments
        .iter()
        .find_map(review_segment)
        .expect("review segment exists");
    let clarification = assistant
        .segments
        .iter()
        .find_map(clarification_segment)
        .expect("clarification segment exists");
    let notice = assistant
        .segments
        .iter()
        .find_map(notice_segment)
        .expect("notice segment exists");

    assert_eq!(
        process.steps[0].body.as_ref().unwrap().as_str(),
        "Checked [REDACTED]，链接[REDACTED] and 路径：[REDACTED] log[REDACTED]"
    );
    assert_eq!(artifact.title.as_str(), "Image at [REDACTED] [REDACTED]。");
    assert_eq!(
        artifact.summary.as_ref().unwrap().as_str(),
        "Blob path [REDACTED] [REDACTED] [REDACTED]"
    );
    assert_eq!(artifact.kind, "image [REDACTED]");
    assert_eq!(review.title.as_str(), "Review [REDACTED]");
    assert_eq!(review.body.as_ref().unwrap().as_str(), "Confirm [REDACTED]");
    assert_eq!(clarification.prompt.as_str(), "Use链接[REDACTED]");
    assert_eq!(notice.body.as_str(), "Read 路径：[REDACTED] and [REDACTED]");
    for value in [
        process.steps[0].body.as_ref().unwrap().as_str(),
        artifact.kind.as_str(),
        artifact.title.as_str(),
        artifact.summary.as_ref().unwrap().as_str(),
        review.title.as_str(),
        review.body.as_ref().unwrap().as_str(),
        clarification.prompt.as_str(),
        notice.body.as_str(),
    ] {
        assert!(!value.contains("provider.example"));
        assert!(!value.to_ascii_lowercase().contains(".jyowo/runtime/blobs"));
        assert!(!value.contains("data:image"));
        assert!(!value.contains("blob:null"));
        assert!(!value.contains("javascript:"));
    }
}

#[test]
fn minimax_style_generation_flow_stays_in_one_safe_assistant_work_tree() {
    let events = vec![
        user_event(
            1,
            "event-user",
            "run-minimax",
            "user-1",
            "帮我生成一张海报图",
        ),
        event(
            2,
            "event-thinking",
            "run-minimax",
            "assistant.thinking.delta",
            json!({ "safeSummary": "正在检查可用的图像工具", "status": "running" }),
        ),
        tool_requested(3, "event-tool-requested", "run-minimax", "tool-minimax"),
        event(
            4,
            "event-permission-requested",
            "run-minimax",
            "permission.requested",
            json!({
                "requestId": "permission-minimax",
                "toolUseId": "tool-minimax",
                "reason": "Need approval to call the image generation tool.",
            }),
        ),
        event(
            5,
            "event-permission-resolved",
            "run-minimax",
            "permission.resolved",
            json!({ "requestId": "permission-minimax", "decision": "approve" }),
        ),
        event(
            6,
            "event-tool-failed",
            "run-minimax",
            "tool.failed",
            json!({
                "toolUseId": "tool-minimax",
                "message": "raw provider failure at /Users/alice/private with token=secret-token",
            }),
        ),
        assistant_completed(
            7,
            "event-final-text",
            "run-minimax",
            "assistant-final",
            "图像工具失败后，我保留了可复用的提示词和下一步建议。",
        ),
        event(
            8,
            "event-artifact-created",
            "run-minimax",
            "artifact.created",
            json!({
                "artifactId": "artifact-minimax",
                "title": "海报生成提示词",
                "summary": "可复用的图像生成提示词已准备好。",
                "blobRef": "blob-secret",
                "contentHash": "hash-secret",
                "privatePath": "/Users/alice/private/poster.png",
            }),
        ),
        event(
            9,
            "event-run-ended",
            "run-minimax",
            "run.ended",
            json!({ "reason": "completed" }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-minimax", events);

    assert_eq!(projection.turns.len(), 1);
    let turn = &projection.turns[0];
    assert_eq!(turn.user.body.as_str(), "帮我生成一张海报图");
    let assistant = turn.assistant.as_ref().expect("assistant work exists");
    assert_eq!(assistant.id, "assistant:run-minimax");
    assert_eq!(assistant.status, AssistantWorkStatus::Complete);

    let group = assistant
        .segments
        .iter()
        .find_map(tool_group)
        .expect("tool attempt should be nested in assistant work");
    assert_eq!(group.attempts.len(), 1);
    let attempt = &group.attempts[0];
    assert_eq!(attempt.tool_use_id, "tool-minimax");
    assert_eq!(attempt.status, ToolAttemptStatus::Failed);
    assert_eq!(
        attempt.permission.as_ref().unwrap().request_id,
        "permission-minimax"
    );
    assert_eq!(
        attempt.failure_summary.as_ref().unwrap().as_str(),
        "工具执行失败。可在详情中查看。"
    );

    let final_text = assistant
        .segments
        .iter()
        .filter_map(text_segment)
        .find(|segment| segment.message_id == "assistant-final")
        .expect("final assistant text should stay in the same work tree");
    assert_eq!(
        final_text.body.as_str(),
        "图像工具失败后，我保留了可复用的提示词和下一步建议。"
    );

    let artifact = assistant
        .segments
        .iter()
        .find_map(artifact_segment)
        .expect("artifact segment should be present");
    assert_eq!(artifact.artifact_id, "artifact-minimax");
    assert_eq!(artifact.title.as_str(), "海报生成提示词");

    let serialized = serde_json::to_string(&projection.turns).unwrap();
    assert!(!serialized.contains("raw provider failure"));
    assert!(!serialized.contains("/Users/alice/private"));
    assert!(!serialized.contains("secret-token"));
    assert!(!serialized.contains("blob-secret"));
    assert!(!serialized.contains("hash-secret"));
}

#[test]
fn process_segments_use_status_or_withheld_summary_without_raw_thought_text() {
    let mut withheld = event(
        2,
        "event-thinking",
        "run-1",
        "assistant.thinking.delta",
        json!({ "text": "raw hidden chain of thought with /Users/goya/private" }),
    );
    withheld.visibility = "withheld".to_owned();
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "think"),
        withheld,
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");

    assert_eq!(process.status, ProcessSegmentStatus::Withheld);
    assert_eq!(process.summary.as_str(), "过程内容已折叠");
    assert!(!process.summary.as_str().contains("raw hidden"));
    assert!(!process.summary.as_str().contains("/Users/"));
}

#[test]
fn process_segments_merge_safe_summary_delta_into_reasoning_step() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "think"),
        event(
            2,
            "event-thinking-1",
            "run-1",
            "assistant.thinking.delta",
            json!({ "safeSummaryDelta": "Checked ", "status": "running" }),
        ),
        event(
            3,
            "event-thinking-2",
            "run-1",
            "assistant.thinking.delta",
            json!({ "safeSummaryDelta": "project context.", "status": "running" }),
        ),
        event(
            4,
            "event-run-ended",
            "run-1",
            "run.ended",
            json!({ "reason": "completed" }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");

    assert_eq!(process.status, ProcessSegmentStatus::Complete);
    assert_eq!(process.summary.as_str(), "已完成工作过程");
    assert_eq!(process.steps.len(), 1);
    assert_eq!(process.steps[0].kind, ProcessStepKind::Reasoning);
    assert_eq!(process.steps[0].status, ProcessStepStatus::Complete);
    assert_eq!(
        process.steps[0].body.as_ref().unwrap().as_str(),
        "Checked project context."
    );
}

#[test]
fn tool_lifecycle_adds_safe_process_steps_without_payloads() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "use tool"),
        event(
            2,
            "event-tool-requested",
            "run-1",
            "tool.requested",
            json!({
                "toolUseId": "tool-1",
                "toolName": "read_file",
                "argumentsSummary": "secret path /Users/goya/private"
            }),
        ),
        event(
            3,
            "event-tool-completed",
            "run-1",
            "tool.completed",
            json!({
                "toolUseId": "tool-1",
                "toolName": "read_file",
                "outputSummary": "secret-token"
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");
    let serialized = serde_json::to_string(&process.steps).unwrap();

    assert!(serialized.contains("已读取文件"));
    assert_eq!(process.steps.len(), 1);
    assert_eq!(process.steps[0].kind, ProcessStepKind::FileRead);
    assert_eq!(process.steps[0].status, ProcessStepStatus::Complete);
    assert!(!serialized.contains("/Users/goya/private"));
    assert!(!serialized.contains("secret-token"));
}

#[test]
fn completed_tool_attempt_stays_projected_when_permission_resolves_late() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "use tool"),
        event(
            2,
            "event-tool-requested",
            "run-1",
            "tool.requested",
            json!({
                "toolUseId": "tool-1",
                "toolName": "read_file",
                "argumentsSummary": "Input withheld from conversation timeline."
            }),
        ),
        event(
            3,
            "event-permission-requested",
            "run-1",
            "permission.requested",
            json!({
                "requestId": "request-tool-1",
                "toolUseId": "tool-1",
                "reason": "The runtime requires approval before continuing."
            }),
        ),
        event(
            4,
            "event-tool-completed",
            "run-1",
            "tool.completed",
            json!({
                "toolUseId": "tool-1",
                "toolName": "read_file",
                "itemCount": 1,
                "outputSummary": "Output withheld from conversation timeline."
            }),
        ),
        event(
            5,
            "event-permission-resolved",
            "run-1",
            "permission.resolved",
            json!({ "requestId": "request-tool-1", "decision": "approve" }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();

    let group = assistant
        .segments
        .iter()
        .find_map(tool_group)
        .expect("completed tool group should stay projected");
    assert_eq!(group.attempts.len(), 1);
    assert_eq!(group.attempts[0].status, ToolAttemptStatus::Completed);
    assert_eq!(
        group.attempts[0].permission.as_ref().unwrap().status,
        ToolPermissionStatus::Approved
    );

    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");
    assert_eq!(process.steps.len(), 1);
    assert_eq!(process.steps[0].kind, ProcessStepKind::FileRead);
    assert_eq!(process.steps[0].status, ProcessStepStatus::Complete);
}

#[test]
fn tool_lifecycle_redacts_secret_like_tool_names() {
    let tool_name = "sk-abcdefghijklmnopqrstuvwxyz";
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "use tool"),
        event(
            2,
            "event-tool-requested",
            "run-1",
            "tool.requested",
            json!({
                "toolUseId": "tool-1",
                "toolName": tool_name,
                "argumentsSummary": "Input withheld from conversation timeline."
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");
    let group = assistant
        .segments
        .iter()
        .find_map(tool_group)
        .expect("tool group exists");
    let serialized_steps = serde_json::to_string(&process.steps).unwrap();
    let serialized_group = serde_json::to_string(&group.attempts).unwrap();

    assert!(!serialized_steps.contains(tool_name));
    assert!(!serialized_group.contains(tool_name));
    assert!(serialized_steps.contains("[REDACTED]"));
    assert!(serialized_group.contains("[REDACTED]"));
}

#[test]
fn command_tool_projects_command_process_detail() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "run tests"),
        event(
            2,
            "event-tool-requested",
            "run-1",
            "tool.requested",
            json!({
                "toolUseId": "tool-1",
                "toolName": "Bash",
                "command": "pnpm check:desktop",
                "argumentsSummary": "Input withheld from conversation timeline."
            }),
        ),
        event(
            3,
            "event-tool-completed",
            "run-1",
            "tool.completed",
            json!({
                "toolUseId": "tool-1",
                "toolName": "Bash",
                "durationMs": 100,
                "exitCode": 0,
                "outputSummary": "desktop checks passed"
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");
    let step = process.steps.first().expect("process step exists");

    assert_eq!(step.kind, ProcessStepKind::Command);
    assert_eq!(step.status, ProcessStepStatus::Complete);
    assert_eq!(step.title.as_str(), "命令已完成");
    let Some(ProcessStepDetail::Command {
        command,
        output,
        exit_code,
        ..
    }) = step.detail.as_ref()
    else {
        panic!("command detail should be projected");
    };
    assert_eq!(command.as_str(), "pnpm check:desktop");
    assert_eq!(
        output.as_ref().map(|value| value.as_str()),
        Some("desktop checks passed")
    );
    assert_eq!(*exit_code, Some(0));
}

#[test]
fn completed_file_search_tools_are_aggregated_by_kind() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "search"),
        event(
            2,
            "event-tool-a-requested",
            "run-1",
            "tool.requested",
            json!({
                "toolUseId": "tool-a",
                "toolName": "grep",
                "argumentsSummary": "Input withheld from conversation timeline."
            }),
        ),
        event(
            3,
            "event-tool-a-completed",
            "run-1",
            "tool.completed",
            json!({
                "toolUseId": "tool-a",
                "toolName": "grep",
                "itemCount": 2,
                "outputSummary": "2 matches"
            }),
        ),
        event(
            4,
            "event-tool-b-requested",
            "run-1",
            "tool.requested",
            json!({
                "toolUseId": "tool-b",
                "toolName": "glob",
                "argumentsSummary": "Input withheld from conversation timeline."
            }),
        ),
        event(
            5,
            "event-tool-b-completed",
            "run-1",
            "tool.completed",
            json!({
                "toolUseId": "tool-b",
                "toolName": "glob",
                "itemCount": 3,
                "outputSummary": "3 files"
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");
    let file_search_steps = process
        .steps
        .iter()
        .filter(|step| step.kind == ProcessStepKind::FileSearch)
        .collect::<Vec<_>>();

    assert_eq!(file_search_steps.len(), 1);
    assert_eq!(file_search_steps[0].status, ProcessStepStatus::Complete);
    let Some(ProcessStepDetail::Activity { item_count, .. }) = &file_search_steps[0].detail else {
        panic!("file search should use activity detail");
    };
    assert_eq!(*item_count, Some(5));
}

#[test]
fn file_edit_with_safe_diff_projects_diff_step() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "edit"),
        event(
            2,
            "event-tool-requested",
            "run-1",
            "tool.requested",
            json!({
                "toolUseId": "tool-1",
                "toolName": "apply_patch",
                "argumentsSummary": "Input withheld from conversation timeline."
            }),
        ),
        event(
            3,
            "event-tool-completed",
            "run-1",
            "tool.completed",
            json!({
                "toolUseId": "tool-1",
                "toolName": "apply_patch",
                "outputSummary": "Updated 1 file",
                "diff": {
                    "files": [
                        {
                            "path": "apps/desktop/src/features/conversation/timeline/process-panel.tsx",
                            "addedLines": 2,
                            "removedLines": 1,
                            "preview": "@@\\n- old\\n+ new"
                        }
                    ]
                }
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");

    assert!(process
        .steps
        .iter()
        .any(|step| step.kind == ProcessStepKind::FileEdit));
    let diff = process
        .steps
        .iter()
        .find(|step| step.kind == ProcessStepKind::Diff)
        .expect("diff step exists");
    let Some(ProcessStepDetail::Diff { files }) = &diff.detail else {
        panic!("diff step should include diff detail");
    };
    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0].path.as_str(),
        "apps/desktop/src/features/conversation/timeline/process-panel.tsx"
    );
    assert_eq!(files[0].added_lines, 2);
    assert_eq!(files[0].removed_lines, 1);
}

#[test]
fn ready_image_artifact_projects_process_step_without_duplicate_artifact_segment() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "generate image"),
        event(
            2,
            "event-artifact-created",
            "run-1",
            "artifact.created",
            json!({
                "artifactId": "artifact-image",
                "title": "Generated image",
                "kind": "image",
                "status": "ready",
                "source": "tool",
                "media": {
                    "kind": "image",
                    "mimeType": "image/png",
                    "sizeBytes": 68
                }
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();

    assert_eq!(
        assistant
            .segments
            .iter()
            .filter_map(artifact_segment)
            .count(),
        0
    );
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");
    let artifact_step = process
        .steps
        .iter()
        .find(|step| step.kind == ProcessStepKind::Artifact)
        .expect("artifact step exists");
    let Some(ProcessStepDetail::Artifact { artifact_id, media }) = &artifact_step.detail else {
        panic!("artifact step should include artifact detail");
    };
    assert_eq!(artifact_id, "artifact-image");
    assert_eq!(media.kind, ArtifactMediaKind::Image);
    assert_eq!(media.mime_type, "image/png");
}

#[test]
fn ready_image_artifact_redacts_unsafe_media_mime_type() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "generate image"),
        event(
            2,
            "event-artifact-created",
            "run-1",
            "artifact.created",
            json!({
                "artifactId": "artifact-image",
                "title": "Generated image",
                "kind": "image",
                "status": "ready",
                "source": "tool",
                "media": {
                    "kind": "image",
                    "mimeType": "image/png /tmp/provider-output https://provider.example/blob",
                    "sizeBytes": 68
                }
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");
    let artifact_step = process
        .steps
        .iter()
        .find(|step| step.kind == ProcessStepKind::Artifact)
        .expect("artifact step exists");
    let Some(ProcessStepDetail::Artifact { media, .. }) = &artifact_step.detail else {
        panic!("artifact step should include artifact detail");
    };

    assert_eq!(media.kind, ArtifactMediaKind::Image);
    assert_eq!(media.mime_type, "image/png");
    assert!(!media.mime_type.contains("/tmp/provider-output"));
    assert!(!media.mime_type.contains("provider.example"));
}

#[test]
fn artifact_media_preview_does_not_project_secret_like_mime_token() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "generate video"),
        event(
            2,
            "event-artifact-created",
            "run-1",
            "artifact.created",
            json!({
                "artifactId": "artifact-video",
                "title": "Generated video",
                "kind": "video",
                "status": "ready",
                "source": "tool",
                "media": {
                    "kind": "video",
                    "mimeType": "video/sk-abcdefghijklmnopqrstuvwxyz0123456789",
                    "sizeBytes": 68
                }
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let artifact = assistant
        .segments
        .iter()
        .find_map(artifact_segment)
        .expect("video artifact remains a metadata segment");
    let media = artifact.media.as_ref().expect("media should project");

    assert_eq!(media.kind, ArtifactMediaKind::Video);
    assert_eq!(media.mime_type, "video/mp4");
    assert!(!media
        .mime_type
        .contains("sk-abcdefghijklmnopqrstuvwxyz0123456789"));
}

#[test]
fn artifact_media_preview_preserves_allowlisted_file_mime_type() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "generate file"),
        event(
            2,
            "event-artifact-created",
            "run-1",
            "artifact.created",
            json!({
                "artifactId": "artifact-file",
                "title": "Generated file",
                "kind": "file",
                "status": "ready",
                "source": "tool",
                "media": {
                    "kind": "file",
                    "mimeType": "text/plain",
                    "sizeBytes": 68
                }
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let artifact = assistant
        .segments
        .iter()
        .find_map(artifact_segment)
        .expect("file artifact remains a metadata segment");
    let media = artifact.media.as_ref().expect("media should project");

    assert_eq!(media.kind, ArtifactMediaKind::File);
    assert_eq!(media.mime_type, "text/plain");
}

#[test]
fn ready_image_artifact_update_uses_existing_media_without_duplicate_artifact_segment() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "generate image"),
        event(
            2,
            "event-artifact-created",
            "run-1",
            "artifact.created",
            json!({
                "artifactId": "artifact-image",
                "title": "Generated image",
                "kind": "image",
                "status": "pending",
                "source": "tool",
                "media": {
                    "kind": "image",
                    "mimeType": "image/png",
                    "sizeBytes": 68
                }
            }),
        ),
        event(
            3,
            "event-artifact-updated",
            "run-1",
            "artifact.updated",
            json!({
                "artifactId": "artifact-image",
                "status": "ready",
                "source": "tool"
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();

    assert_eq!(
        assistant
            .segments
            .iter()
            .filter_map(artifact_segment)
            .count(),
        0
    );
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");
    let artifact_step = process
        .steps
        .iter()
        .find(|step| step.kind == ProcessStepKind::Artifact)
        .expect("artifact step exists");

    assert_eq!(artifact_step.title.as_str(), "Generated image");
    let Some(ProcessStepDetail::Artifact { artifact_id, media }) = &artifact_step.detail else {
        panic!("artifact step should include artifact detail");
    };
    assert_eq!(artifact_id, "artifact-image");
    assert_eq!(media.kind, ArtifactMediaKind::Image);
    assert_eq!(media.mime_type, "image/png");
}

#[test]
fn ready_image_artifact_partial_update_keeps_existing_process_step() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "generate image"),
        event(
            2,
            "event-artifact-created",
            "run-1",
            "artifact.created",
            json!({
                "artifactId": "artifact-image",
                "title": "Generated image",
                "kind": "image",
                "status": "ready",
                "source": "tool",
                "media": {
                    "kind": "image",
                    "mimeType": "image/png",
                    "sizeBytes": 68
                }
            }),
        ),
        event(
            3,
            "event-artifact-updated",
            "run-1",
            "artifact.updated",
            json!({
                "artifactId": "artifact-image",
                "status": "ready",
                "source": "tool"
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();

    assert_eq!(
        assistant
            .segments
            .iter()
            .filter_map(artifact_segment)
            .count(),
        0
    );
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");
    let artifact_steps = process
        .steps
        .iter()
        .filter(|step| step.kind == ProcessStepKind::Artifact)
        .collect::<Vec<_>>();

    assert_eq!(artifact_steps.len(), 1);
    assert_eq!(artifact_steps[0].title.as_str(), "Generated image");
}

#[test]
fn run_end_updates_assistant_status_and_duplicate_event_ids_are_idempotent() {
    let duplicate = user_event(1, "event-user", "run-1", "user-1", "hello");
    let events = vec![
        duplicate.clone(),
        duplicate,
        assistant_completed(2, "event-assistant", "run-1", "assistant-1", "hi"),
        event(
            3,
            "event-run-ended",
            "run-1",
            "run.ended",
            json!({ "reason": "completed" }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);

    assert_eq!(projection.turns.len(), 1);
    assert_eq!(
        projection.turns[0].assistant.as_ref().unwrap().status,
        AssistantWorkStatus::Complete
    );
    assert_eq!(
        projection.event_cursor.unwrap().conversation_sequence,
        3,
        "latest unique event cursor is retained"
    );
}

#[test]
fn run_end_marks_running_process_complete() {
    let events = vec![
        user_event(1, "event-user", "run-1", "user-1", "hello"),
        event(
            2,
            "event-thinking",
            "run-1",
            "assistant.thinking.delta",
            json!({ "status": "running" }),
        ),
        assistant_completed(3, "event-assistant", "run-1", "assistant-1", "hi"),
        event(
            4,
            "event-run-ended",
            "run-1",
            "run.ended",
            json!({ "reason": "completed" }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conversation-1", events);
    let assistant = projection.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(process_segment)
        .expect("process exists");

    assert_eq!(process.status, ProcessSegmentStatus::Complete);
    assert_eq!(process.summary.as_str(), "已完成工作过程");
}

#[test]
fn run_end_marks_failed_or_cancelled_process_status() {
    for (reason, expected_status, expected_process_status) in [
        (
            "failed",
            AssistantWorkStatus::Failed,
            ProcessSegmentStatus::Failed,
        ),
        (
            "cancelled",
            AssistantWorkStatus::Cancelled,
            ProcessSegmentStatus::Cancelled,
        ),
    ] {
        let events = vec![
            user_event(1, "event-user", "run-1", "user-1", "hello"),
            event(
                2,
                "event-thinking",
                "run-1",
                "assistant.thinking.delta",
                json!({ "status": "running" }),
            ),
            event(
                3,
                "event-run-ended",
                "run-1",
                "run.ended",
                json!({ "reason": reason }),
            ),
        ];

        let projection = project_conversation_worktree_snapshot("conversation-1", events);
        let assistant = projection.turns[0].assistant.as_ref().unwrap();
        let process = assistant
            .segments
            .iter()
            .find_map(process_segment)
            .expect("process exists");

        assert_eq!(assistant.status, expected_status);
        assert_eq!(process.status, expected_process_status);
        assert_eq!(process.summary.as_str(), "正在处理请求");
    }
}

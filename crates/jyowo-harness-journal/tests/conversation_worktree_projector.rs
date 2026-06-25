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

fn text_segment(segment: &AssistantSegment) -> Option<&TextSegment> {
    match segment {
        AssistantSegment::Text(text) => Some(text),
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
    assert_eq!(
        group.attempts[0].failure_summary.as_ref().unwrap().as_str(),
        "工具执行失败。详情可在 Activity 中查看。"
    );
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
fn thinking_segments_use_status_or_withheld_summary_without_raw_thought_text() {
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
    let thinking = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            AssistantSegment::Thinking(thinking) => Some(thinking),
            _ => None,
        })
        .expect("thinking exists");

    assert_eq!(thinking.status, ThinkingSegmentStatus::Withheld);
    assert_eq!(thinking.summary.text.as_str(), "思考内容已折叠");
    assert!(!thinking.summary.text.as_str().contains("raw hidden"));
    assert!(!thinking.summary.text.as_str().contains("/Users/"));
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

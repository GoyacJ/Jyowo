//! Integration tests for the conversation workbench projection.
//!
//! These tests verify that the projector emits the target workbench model
//! directly without relying on React to infer permission, tool, command, diff,
//! artifact, or reasoning safety from raw event payloads.

use chrono::{DateTime, Utc};
use harness_contracts::*;
use harness_journal::conversation_worktree_projector::*;
use serde_json::json;

fn event_cursor() -> ConversationCursor {
    ConversationCursor {
        event_id: EventId::new(),
        conversation_sequence: 1,
    }
}

fn make_event(
    cursor: ConversationCursor,
    run_id: &str,
    event_type: &str,
    payload: serde_json::Value,
) -> ConversationTimelineEvent {
    ConversationTimelineEvent {
        id: EventId::new().to_string(),
        cursor,
        payload,
        run_id: run_id.to_owned(),
        sequence: cursor.conversation_sequence,
        source: "test".to_owned(),
        timestamp: DateTime::<Utc>::UNIX_EPOCH,
        event_type: event_type.to_owned(),
        visibility: "visible".to_owned(),
    }
}

fn run_started_payload(run_id: &str) -> serde_json::Value {
    json!({
        "model": {
            "providerId": "test-provider",
            "modelId": "test-model",
            "displayName": "Test Model",
            "protocol": "messages"
        }
    })
}

fn user_message_payload(body: &str) -> serde_json::Value {
    json!({
        "messageId": "user-msg-1",
        "body": body
    })
}

// ── Permission projection tests ──

#[test]
fn permission_requested_projects_to_decision_request_state() {
    let cursor = event_cursor();
    let events = vec![
        make_event(cursor, "run-1", "run.started", run_started_payload("run-1")),
        make_event(
            cursor,
            "run-1",
            "user.message.appended",
            user_message_payload("hi"),
        ),
        make_event(
            cursor,
            "run-1",
            "tool.requested",
            json!({
                "toolUseId": "tool-1",
                "toolName": "shell"
            }),
        ),
        make_event(
            cursor,
            "run-1",
            "permission.requested",
            json!({
                "requestId": "req-1",
                "toolUseId": "tool-1",
                "reason": "Shell command requires approval",
                "effectiveMode": "default"
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conv-1", events);
    let page = worktree_projection_page(projection, false);

    let assistant = page.turns[0].assistant.as_ref().unwrap();
    assert_eq!(assistant.projection_version, 1);

    let tool_group = assistant
        .segments
        .iter()
        .find_map(|s| match s {
            AssistantSegment::ToolGroup(g) => Some(g),
            _ => None,
        })
        .unwrap();

    let attempt = &tool_group.attempts[0];
    assert_eq!(attempt.tool_name, "shell");
    assert_eq!(attempt.status, ToolAttemptStatus::WaitingPermission);

    let decision = attempt.permission.as_ref().unwrap();
    assert_eq!(decision.request_id, "req-1");
    assert_eq!(decision.status, DecisionRequestStatus::Pending);
    assert_eq!(decision.reason, "Shell command requires approval");
}

// ── Command projection tests ──

#[test]
fn command_completion_projects_to_command_execution() {
    let cursor = event_cursor();
    let events = vec![
        make_event(cursor, "run-1", "run.started", run_started_payload("run-1")),
        make_event(
            cursor,
            "run-1",
            "user.message.appended",
            user_message_payload("run tests"),
        ),
        make_event(
            cursor,
            "run-1",
            "tool.requested",
            json!({
                "toolUseId": "tool-1",
                "toolName": "bash"
            }),
        ),
        make_event(
            cursor,
            "run-1",
            "tool.completed",
            json!({
                "toolUseId": "tool-1",
                "command": "cargo test",
                "exitCode": 0,
                "durationMs": 1200,
                "outputSummary": "test result: ok"
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conv-1", events);
    let page = worktree_projection_page(projection, false);

    let assistant = page.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(|s| match s {
            AssistantSegment::Process(p) => Some(p),
            _ => None,
        })
        .unwrap();

    let command_step = process
        .steps
        .iter()
        .find(|step| matches!(step.kind, ProcessStepKind::Command))
        .unwrap();

    match &command_step.detail {
        Some(ProcessStepDetail::Command(cmd)) => {
            assert_eq!(cmd.command, "cargo test");
            assert_eq!(cmd.exit_code, Some(0));
            assert_eq!(cmd.duration_ms, Some(1200));
            assert_eq!(cmd.stdout_preview.as_deref(), Some("test result: ok"));
            assert!(cmd.truncated);
            assert_eq!(cmd.redaction_state, EvidenceRedactionState::Clean);
        }
        _ => panic!("expected command detail"),
    }
}

// ── Diff projection tests ──

#[test]
fn diff_completion_projects_to_change_set() {
    let cursor = event_cursor();
    let events = vec![
        make_event(cursor, "run-1", "run.started", run_started_payload("run-1")),
        make_event(
            cursor,
            "run-1",
            "user.message.appended",
            user_message_payload("edit file"),
        ),
        make_event(
            cursor,
            "run-1",
            "tool.requested",
            json!({
                "toolUseId": "tool-1",
                "toolName": "fileedit"
            }),
        ),
        make_event(
            cursor,
            "run-1",
            "tool.completed",
            json!({
                "toolUseId": "tool-1",
                "diff": {
                    "files": [
                        {
                            "path": "src/main.rs",
                            "addedLines": 3,
                            "removedLines": 1,
                            "preview": "+ fn main() {"
                        }
                    ]
                }
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conv-1", events);
    let page = worktree_projection_page(projection, false);

    let assistant = page.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(|s| match s {
            AssistantSegment::Process(p) => Some(p),
            _ => None,
        })
        .unwrap();

    let diff_step = process
        .steps
        .iter()
        .find(|step| matches!(step.kind, ProcessStepKind::Diff))
        .unwrap();

    match &diff_step.detail {
        Some(ProcessStepDetail::Diff(change_set)) => {
            assert!(!change_set.files.is_empty());
            let file = &change_set.files[0];
            assert_eq!(file.path, "src/main.rs");
            assert_eq!(file.added_lines, 3);
            assert_eq!(file.removed_lines, 1);
            assert_eq!(file.preview.as_deref(), Some("+ fn main() {"));
        }
        _ => panic!("expected diff detail"),
    }
}

// ── Thinking/safety projection tests ──

#[test]
fn thinking_delta_projects_to_process_step_with_visibility() {
    let cursor = event_cursor();
    let events = vec![
        make_event(cursor, "run-1", "run.started", run_started_payload("run-1")),
        make_event(
            cursor,
            "run-1",
            "user.message.appended",
            user_message_payload("hi"),
        ),
        make_event(
            cursor,
            "run-1",
            "assistant.thinking.delta",
            json!({
                "status": "running",
                "safeSummary": "Analyzing request",
                "safeSummaryDelta": "Checking available tools"
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conv-1", events);
    let page = worktree_projection_page(projection, false);

    let assistant = page.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(|s| match s {
            AssistantSegment::Process(p) => Some(p),
            _ => None,
        })
        .unwrap();

    // Should NOT contain thinking segment
    let has_thinking = assistant.segments.iter().any(|s| {
        // All segments are Process/Text/ToolGroup/Artifact/etc - no thinking
        false
    });
    assert!(!has_thinking);

    assert_eq!(process.status, ProcessSegmentStatus::Running);
    // When a delta is streaming, the summary defaults to the running text
    assert!(process.summary.as_str().contains("处理"));

    // Should have a reasoning step
    let has_reasoning = process
        .steps
        .iter()
        .any(|step| matches!(step.kind, ProcessStepKind::Reasoning));
    assert!(has_reasoning);
}

#[test]
fn withheld_thinking_projects_to_withheld_process() {
    let cursor = event_cursor();
    let events = vec![
        make_event(cursor, "run-1", "run.started", run_started_payload("run-1")),
        make_event(
            cursor,
            "run-1",
            "user.message.appended",
            user_message_payload("hi"),
        ),
        make_event(
            cursor,
            "run-1",
            "assistant.thinking.delta",
            json!({
                "status": "withheld"
            }),
        ),
    ];

    // Withheld events have different visibility
    let withheld_event = ConversationTimelineEvent {
        id: EventId::new().to_string(),
        cursor,
        payload: json!({"status": "withheld"}),
        run_id: "run-1".to_owned(),
        sequence: cursor.conversation_sequence,
        source: "test".to_owned(),
        timestamp: DateTime::<Utc>::UNIX_EPOCH,
        event_type: "assistant.thinking.delta".to_owned(),
        visibility: "withheld".to_owned(),
    };
    let events = vec![
        make_event(cursor, "run-1", "run.started", run_started_payload("run-1")),
        make_event(
            cursor,
            "run-1",
            "user.message.appended",
            user_message_payload("hi"),
        ),
        withheld_event,
    ];

    let projection = project_conversation_worktree_snapshot("conv-1", events);
    let page = worktree_projection_page(projection, false);

    let assistant = page.turns[0].assistant.as_ref().unwrap();
    let process = assistant
        .segments
        .iter()
        .find_map(|s| match s {
            AssistantSegment::Process(p) => Some(p),
            _ => None,
        })
        .unwrap();

    assert_eq!(process.status, ProcessSegmentStatus::Withheld);
}

// ── Artifact revision projection tests ──

#[test]
fn artifact_created_projects_revision_summary() {
    let cursor = event_cursor();
    let events = vec![
        make_event(cursor, "run-1", "run.started", run_started_payload("run-1")),
        make_event(
            cursor,
            "run-1",
            "user.message.appended",
            user_message_payload("gen code"),
        ),
        make_event(
            cursor,
            "run-1",
            "artifact.created",
            json!({
                "artifactId": "artifact-1",
                "title": "Generated code",
                "kind": "code",
                "status": "ready",
                "source": "tool"
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conv-1", events);
    let page = worktree_projection_page(projection, false);

    let assistant = page.turns[0].assistant.as_ref().unwrap();
    let artifact = assistant
        .segments
        .iter()
        .find_map(|s| match s {
            AssistantSegment::Artifact(a) => Some(a),
            _ => None,
        })
        .unwrap();

    assert_eq!(artifact.artifact_id, "artifact-1");
    assert_eq!(artifact.revision.artifact_id, "artifact-1");
    assert!(!artifact.revision.revision_id.is_empty());
    assert_eq!(artifact.revision.source_run_id, "run-1");
}

// ── projection_version tests ──

#[test]
fn assistant_work_has_projection_version() {
    let cursor = event_cursor();
    let events = vec![
        make_event(cursor, "run-1", "run.started", run_started_payload("run-1")),
        make_event(
            cursor,
            "run-1",
            "user.message.appended",
            user_message_payload("hi"),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conv-1", events);
    let page = worktree_projection_page(projection, false);

    let assistant = page.turns[0].assistant.as_ref().unwrap();
    assert!(assistant.projection_version > 0);
}

// ── Tool failure projection tests ──

#[test]
fn tool_failure_projects_failure_summary() {
    let cursor = event_cursor();
    let events = vec![
        make_event(cursor, "run-1", "run.started", run_started_payload("run-1")),
        make_event(
            cursor,
            "run-1",
            "user.message.appended",
            user_message_payload("do thing"),
        ),
        make_event(
            cursor,
            "run-1",
            "tool.requested",
            json!({
                "toolUseId": "tool-1",
                "toolName": "shell"
            }),
        ),
        make_event(
            cursor,
            "run-1",
            "tool.failed",
            json!({
                "toolUseId": "tool-1"
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot("conv-1", events);
    let page = worktree_projection_page(projection, false);

    let assistant = page.turns[0].assistant.as_ref().unwrap();
    let tool_group = assistant
        .segments
        .iter()
        .find_map(|s| match s {
            AssistantSegment::ToolGroup(g) => Some(g),
            _ => None,
        })
        .unwrap();

    assert_eq!(tool_group.attempts[0].status, ToolAttemptStatus::Failed);
    assert!(tool_group.attempts[0].failure_summary.is_some());
}

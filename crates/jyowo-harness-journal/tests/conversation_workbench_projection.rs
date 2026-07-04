//! Integration tests for the conversation workbench projection.
//!
//! These tests verify that the projector emits the target workbench model
//! directly without relying on React to infer permission, tool, command, diff,
//! artifact, or reasoning safety from raw event payloads.

use chrono::{DateTime, Utc};
use harness_contracts::*;
use harness_journal::conversation_worktree_projector::*;
use harness_journal::{EvidenceRefStore, InMemoryBlobStore, InMemoryEvidenceRefRegistry};
use serde_json::json;
use std::sync::Arc;

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

fn run_started_payload(_run_id: &str) -> serde_json::Value {
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

fn evidence_store() -> Arc<EvidenceRefStore> {
    Arc::new(EvidenceRefStore::new(
        Arc::new(InMemoryEvidenceRefRegistry::new()),
        Arc::new(InMemoryBlobStore::default()),
    ))
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

#[tokio::test]
async fn command_completion_projects_full_output_ref_without_inline_output() {
    let cursor = event_cursor();
    let store = evidence_store();
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
                "toolName": "bash",
                "command": "cargo test",
                "exitCode": 0,
                "durationMs": 1200,
                "stdout": "full stdout\nline 2",
                "stderr": "full stderr",
                "outputSummary": "test result: ok"
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot_with_evidence(
        "conv-1",
        events,
        TenantId::SINGLE,
        store.clone(),
    )
    .await
    .unwrap();
    let page = worktree_projection_page(projection, false);

    let assistant = page.turns[0].assistant.as_ref().unwrap();
    let command = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            AssistantSegment::Process(process) => process.steps.iter().find_map(|step| {
                if let Some(ProcessStepDetail::Command(command)) = &step.detail {
                    Some(command)
                } else {
                    None
                }
            }),
            _ => None,
        })
        .expect("command detail");

    assert_eq!(command.stdout_preview.as_deref(), Some("test result: ok"));
    assert_ne!(
        command.stdout_preview.as_deref(),
        Some("full stdout\nline 2")
    );
    let full_output_ref = command
        .full_output_ref
        .as_ref()
        .expect("full output evidence ref");
    assert_eq!(
        full_output_ref
            .to_string()
            .rsplit(':')
            .next()
            .unwrap()
            .len(),
        64
    );
    let read = store
        .read_evidence(
            TenantId::SINGLE,
            "conv-1",
            full_output_ref,
            EvidenceRefKind::CommandOutput,
        )
        .await
        .unwrap();
    assert_eq!(
        String::from_utf8(read.bytes).unwrap(),
        "full stdout\nline 2\nfull stderr"
    );
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

#[tokio::test]
async fn diff_completion_projects_full_patch_ref_without_inline_patch() {
    let cursor = event_cursor();
    let store = evidence_store();
    let patch = "@@\n- old\n+ new\n+ another\n";
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
                "toolName": "apply_patch"
            }),
        ),
        make_event(
            cursor,
            "run-1",
            "tool.completed",
            json!({
                "toolUseId": "tool-1",
                "toolName": "apply_patch",
                "diff": {
                    "files": [
                        {
                            "path": "src/main.rs",
                            "addedLines": 2,
                            "removedLines": 1,
                            "preview": "@@\n- old\n+ new",
                            "patch": patch
                        }
                    ]
                }
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot_with_evidence(
        "conv-1",
        events,
        TenantId::SINGLE,
        store.clone(),
    )
    .await
    .unwrap();
    let page = worktree_projection_page(projection, false);

    let assistant = page.turns[0].assistant.as_ref().unwrap();
    let file = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            AssistantSegment::Process(process) => process.steps.iter().find_map(|step| {
                if let Some(ProcessStepDetail::Diff(change_set)) = &step.detail {
                    change_set.files.first()
                } else {
                    None
                }
            }),
            _ => None,
        })
        .expect("diff file");

    assert_eq!(file.preview.as_deref(), Some("@@\n- old\n+ new"));
    assert_ne!(file.preview.as_deref(), Some(patch));
    let full_patch_ref = file.full_patch_ref.as_ref().expect("full patch ref");
    assert_eq!(
        full_patch_ref.to_string().rsplit(':').next().unwrap().len(),
        64
    );
    let read = store
        .read_evidence(
            TenantId::SINGLE,
            "conv-1",
            full_patch_ref,
            EvidenceRefKind::DiffPatch,
        )
        .await
        .unwrap();
    assert_eq!(String::from_utf8(read.bytes).unwrap(), patch);
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

    // Thinking deltas are projected as process steps, not as standalone segments.

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
                "revisionId": "revision-real-1",
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

#[tokio::test]
async fn artifact_projection_uses_event_revision_id_and_content_ref() {
    use bytes::Bytes;
    use harness_contracts::{BlobMeta, BlobRetention, BlobStore};

    let cursor = event_cursor();
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let store = Arc::new(EvidenceRefStore::new(
        Arc::new(InMemoryEvidenceRefRegistry::new()),
        blob_store.clone(),
    ));
    let artifact_bytes = Bytes::from_static(b"artifact body");
    let artifact_hash = *blake3::hash(&artifact_bytes).as_bytes();
    let blob_ref = blob_store
        .put(
            TenantId::SINGLE,
            artifact_bytes,
            BlobMeta {
                content_type: Some("text/markdown".to_owned()),
                size: "artifact body".len() as u64,
                content_hash: artifact_hash,
                created_at: DateTime::<Utc>::UNIX_EPOCH,
                retention: BlobRetention::TenantScoped,
            },
        )
        .await
        .unwrap();
    let events = vec![
        make_event(cursor, "run-1", "run.started", run_started_payload("run-1")),
        make_event(
            cursor,
            "run-1",
            "user.message.appended",
            user_message_payload("gen doc"),
        ),
        make_event(
            cursor,
            "run-1",
            "artifact.created",
            json!({
                "revisionId": "revision-real-1",
                "artifactId": "artifact-1",
                "title": "Generated doc",
                "kind": "document",
                "status": "ready",
                "source": "tool",
                "blobRef": blob_ref,
                "contentHash": artifact_hash
            }),
        ),
    ];

    let projection = project_conversation_worktree_snapshot_with_evidence(
        "conv-1",
        events,
        TenantId::SINGLE,
        store.clone(),
    )
    .await
    .unwrap();
    let page = worktree_projection_page(projection, false);

    let assistant = page.turns[0].assistant.as_ref().unwrap();
    let artifact = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            AssistantSegment::Artifact(artifact) => Some(artifact),
            _ => None,
        })
        .expect("artifact segment");

    assert_eq!(artifact.revision.revision_id, "revision-real-1");
    assert_ne!(artifact.revision.revision_id, "rev:artifact-1");
    let content_ref = artifact.revision.content_ref.as_ref().expect("content ref");
    let read = store
        .read_evidence(
            TenantId::SINGLE,
            "conv-1",
            content_ref,
            EvidenceRefKind::ArtifactContent,
        )
        .await
        .unwrap();
    assert_eq!(String::from_utf8(read.bytes).unwrap(), "artifact body");
    assert_eq!(read.content_type, "text/markdown");
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

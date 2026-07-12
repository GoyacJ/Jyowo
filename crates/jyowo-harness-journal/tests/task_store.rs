#![cfg(feature = "sqlite")]

use chrono::Utc;
use harness_contracts::{ClientId, CommandId, RunSegmentId, RunTerminalReason, TaskId};
use harness_journal::{
    AcceptedCommand, NewTaskEvent, SegmentExecutionClaim, SegmentExecutionTerminal, TaskStore,
};
use serde_json::json;

#[test]
fn public_store_preserves_global_order_across_tasks() {
    let path = temp_path("store");
    let task_a = TaskId::new();
    let task_b = TaskId::new();
    let store = TaskStore::open(&path).unwrap();
    for task_id in [task_a, task_b] {
        store
            .transact_command(command(task_id), |_| {
                Ok(vec![NewTaskEvent::task_created("Task")])
            })
            .unwrap();
    }

    let offsets = store
        .events_after(0, 16)
        .unwrap()
        .into_iter()
        .map(|event| event.global_offset)
        .collect::<Vec<_>>();
    assert_eq!(offsets, vec![1, 2]);

    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn segment_execution_claim_is_durable_and_returns_the_terminal_outcome() {
    let path = temp_path("segment-execution");
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let terminal = SegmentExecutionTerminal {
        terminal_reason: RunTerminalReason::Failed,
        incomplete_output: true,
        ended_at: Utc::now(),
    };
    let store = TaskStore::open(&path).unwrap();

    assert_eq!(
        store
            .claim_segment_execution(task_id, segment_id, "request-digest")
            .unwrap(),
        SegmentExecutionClaim::Claimed
    );
    assert_eq!(
        store
            .claim_segment_execution(task_id, segment_id, "request-digest")
            .unwrap(),
        SegmentExecutionClaim::InProgress
    );
    store
        .complete_segment_execution(task_id, segment_id, "request-digest", &terminal)
        .unwrap();
    drop(store);

    let reopened = TaskStore::open(&path).unwrap();
    assert_eq!(
        reopened
            .claim_segment_execution(task_id, segment_id, "request-digest")
            .unwrap(),
        SegmentExecutionClaim::Completed(terminal)
    );
    assert!(reopened
        .claim_segment_execution(task_id, segment_id, "different-digest")
        .is_err());

    drop(reopened);
    let _ = std::fs::remove_file(path);
}

fn command(task_id: TaskId) -> AcceptedCommand {
    AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("idem-{}", CommandId::new()),
        expected_stream_version: 0,
        authority: TaskStore::user_authority(ClientId::new()),
        payload: json!({ "create": true }),
    }
}

fn temp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-task-public-{name}-{}-{}.db",
        std::process::id(),
        TaskId::new()
    ))
}

use std::sync::Arc;

use harness_contracts::{CommandId, RunSegmentId, TaskId};
use harness_daemon::{CheckpointService, CheckpointState, ContextCompactionService};
use harness_journal::{AcceptedCommand, CommandOutcome, NewTaskEvent, TaskBlobStore, TaskStore};
use serde_json::json;

#[test]
fn context_compaction_keeps_canonical_events_and_replaces_only_with_a_valid_summary() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("compact context"),
            NewTaskEvent::run_started(segment_id, chrono::Utc::now()),
            NewTaskEvent::task_title_changed("compact context safely"),
        ],
    );
    let canonical_before = store.task_events_after(task_id, 0, 100).unwrap();
    let blobs =
        TaskBlobStore::open(Arc::clone(&store), task_id, root.path().join("blobs")).unwrap();
    let first_blob = blobs
        .put(
            "application/vnd.jyowo.context-summary+json",
            br#"{"summary":"first"}"#,
        )
        .unwrap();
    let service = ContextCompactionService::new(Arc::clone(&store));

    let first = service.activate(task_id, 1, 3, first_blob.id).unwrap();

    assert_eq!(first.source_start_global_offset, 1);
    assert_eq!(first.source_end_global_offset, 3);
    assert_eq!(first.blob_id, first_blob.id);
    let checkpoint = CheckpointService::new(Arc::clone(&store))
        .persist(
            task_id,
            segment_id,
            CheckpointState {
                context_cursor: 3,
                context_blob_id: Some(first_blob.id),
                ..CheckpointState::default()
            },
        )
        .unwrap();
    assert_eq!(checkpoint.context_blob_id, Some(first_blob.id));
    assert_eq!(
        store.task_events_after(task_id, 0, 100).unwrap(),
        canonical_before
    );
    let wrong_media_blob = blobs.put("text/plain", b"not a context summary").unwrap();
    assert!(service
        .activate(task_id, 1, 3, wrong_media_blob.id)
        .is_err());
    assert_eq!(
        store.active_context_summary(task_id).unwrap(),
        Some(first.clone())
    );
    let replacement_blob = blobs
        .put(
            "application/vnd.jyowo.context-summary+json",
            br#"{"summary":"invalid replacement"}"#,
        )
        .unwrap();
    assert!(service
        .activate(task_id, 1, 4, replacement_blob.id)
        .is_err());
    assert_eq!(store.active_context_summary(task_id).unwrap(), Some(first));
}

fn append(store: &TaskStore, task_id: TaskId, version: u64, events: Vec<NewTaskEvent>) {
    let command = AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("test-{}", CommandId::new()),
        expected_stream_version: version,
        authority: TaskStore::supervisor_authority(),
        payload: json!({ "type": "test_setup" }),
    };
    assert!(matches!(
        store.transact_command(command, |_| Ok(events)).unwrap(),
        CommandOutcome::Accepted { .. }
    ));
}

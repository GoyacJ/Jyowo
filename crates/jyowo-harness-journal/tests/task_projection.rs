use harness_contracts::{
    ClientId, CommandId, PermissionMode, QueueItemId, RunId, RunSegmentId, SessionId, TaskId,
    TaskState, WorkspaceMode, WorkspaceSelection,
};
use harness_journal::{AcceptedCommand, NewTaskEvent, TaskStore};
use serde_json::json;

#[test]
fn public_projection_reflects_committed_user_events() {
    let path = temp_path("projection");
    let task_id = TaskId::new();
    let store = TaskStore::open(&path).unwrap();
    store
        .transact_command(command(task_id, 0), |_| {
            Ok(vec![NewTaskEvent::task_created("Projected")])
        })
        .unwrap();

    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(projection.title, "Projected");
    assert_eq!(projection.state, TaskState::Idle);
    assert_eq!(projection.stream_version, 1);

    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn consumed_message_retains_the_runtime_input_for_its_segment() {
    let path = temp_path("segment-run-input");
    let task_id = TaskId::new();
    let queue_item_id = QueueItemId::new();
    let segment_id = RunSegmentId::new();
    let store = TaskStore::open(&path).unwrap();
    store
        .transact_command(command(task_id, 0), |_| {
            Ok(vec![NewTaskEvent::task_created("Runtime input")])
        })
        .unwrap();
    store
        .transact_command(command(task_id, 1), |_| {
            Ok(vec![NewTaskEvent::message_queued_with_runtime(
                queue_item_id,
                "inspect the workspace",
                Vec::new(),
                vec!["src/lib.rs".into()],
                Some("provider-config-001".into()),
                PermissionMode::AcceptEdits,
                chrono::Utc::now(),
            )])
        })
        .unwrap();
    store
        .transact_command(supervisor_command(task_id, 2), |_| {
            Ok(vec![
                NewTaskEvent::run_started(segment_id, chrono::Utc::now()),
                NewTaskEvent::message_consumed(queue_item_id, 1, segment_id),
            ])
        })
        .unwrap();

    let input = store
        .queue_item_for_segment(task_id, segment_id)
        .unwrap()
        .expect("consumed message remains queryable");
    assert_eq!(input.content, "inspect the workspace");
    assert_eq!(input.context_references, vec!["src/lib.rs"]);
    assert_eq!(
        input.model_config_id.as_deref(),
        Some("provider-config-001")
    );
    assert_eq!(input.permission_mode, PermissionMode::AcceptEdits);

    drop(store);
    let _ = std::fs::remove_file(path);
}

#[test]
fn segment_start_outbox_freezes_the_complete_normal_run_input() {
    let path = temp_path("segment-start-outbox-run-input");
    let task_id = TaskId::new();
    let queue_item_id = QueueItemId::new();
    let segment_id = RunSegmentId::new();
    let workspace = WorkspaceSelection {
        mode: WorkspaceMode::Current,
        root: "/workspace/project".into(),
    };
    let store = TaskStore::open(&path).unwrap();
    store
        .transact_command(command(task_id, 0), |_| {
            Ok(vec![NewTaskEvent::task_created_in_workspace(
                "Durable runtime input",
                workspace.clone(),
            )])
        })
        .unwrap();
    store
        .transact_command(supervisor_command(task_id, 1), |_| {
            Ok(vec![
                NewTaskEvent::run_started(segment_id, chrono::Utc::now()),
                NewTaskEvent::message_queued_with_runtime(
                    queue_item_id,
                    "inspect the workspace",
                    Vec::new(),
                    vec!["src/lib.rs".into()],
                    Some("provider-config-001".into()),
                    PermissionMode::AcceptEdits,
                    chrono::Utc::now(),
                ),
                NewTaskEvent::message_consumed(queue_item_id, 1, segment_id),
            ])
        })
        .unwrap();

    let pending = store
        .pending_segment_start(task_id, segment_id)
        .unwrap()
        .expect("normal segment start remains pending until delivery");
    assert_eq!(pending.task_id, task_id);
    assert_eq!(pending.segment_id, segment_id);
    assert!(pending.indeterminate_tools.is_empty());
    assert_eq!(pending.input.queue_item_id, Some(queue_item_id));
    assert_eq!(pending.input.content, "inspect the workspace");
    assert_eq!(pending.input.context_references, vec!["src/lib.rs"]);
    assert_eq!(
        pending.input.model_config_id.as_deref(),
        Some("provider-config-001")
    );
    assert_eq!(pending.input.permission_mode, PermissionMode::AcceptEdits);
    assert_eq!(pending.input.workspace, Some(workspace));
    assert_eq!(
        pending.input.session_id,
        SessionId::from_u128(u128::from_be_bytes(task_id.as_bytes()))
    );
    assert_eq!(
        pending.input.run_id,
        RunId::from_u128(u128::from_be_bytes(segment_id.as_bytes()))
    );
    assert_eq!(pending.input.workspace_lease_id, None);

    drop(store);
    let _ = std::fs::remove_file(path);
}

fn command(task_id: TaskId, expected_stream_version: u64) -> AcceptedCommand {
    AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("idem-{}", CommandId::new()),
        expected_stream_version,
        authority: TaskStore::user_authority(ClientId::new()),
        payload: json!({ "expected": expected_stream_version }),
    }
}

fn supervisor_command(task_id: TaskId, expected_stream_version: u64) -> AcceptedCommand {
    AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("supervisor-{}", CommandId::new()),
        expected_stream_version,
        authority: TaskStore::supervisor_authority(),
        payload: json!({ "expected": expected_stream_version }),
    }
}

fn temp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-task-public-{name}-{}-{}.db",
        std::process::id(),
        TaskId::new()
    ))
}

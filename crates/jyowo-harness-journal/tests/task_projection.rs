use harness_contracts::{ClientId, CommandId, TaskId, TaskState};
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

fn temp_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-task-public-{name}-{}-{}.db",
        std::process::id(),
        TaskId::new()
    ))
}

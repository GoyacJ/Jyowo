#![cfg(feature = "sqlite")]

use harness_contracts::{ClientId, CommandId, TaskId};
use harness_journal::{AcceptedCommand, CommandOutcome, NewTaskEvent, TaskStore};
use serde_json::json;

#[test]
fn public_user_command_commits_atomically() {
    let path = temp_path("command");
    let task_id = TaskId::new();
    let store = TaskStore::open(&path).unwrap();
    let outcome = store
        .transact_command(command(task_id, 0), |_| {
            Ok(vec![NewTaskEvent::task_created("Public")])
        })
        .unwrap();

    assert!(matches!(
        outcome,
        CommandOutcome::Accepted {
            stream_version: 1,
            ..
        }
    ));
    assert_eq!(store.latest_global_offset().unwrap(), 1);

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

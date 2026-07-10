use std::path::PathBuf;

use harness_contracts::{EventSource, EventSourceKind, TaskId};
use harness_journal::{NewTaskEvent, TaskStore, TaskStoreError};
use serde_json::json;

#[test]
fn task_events_have_global_order_and_per_task_versions_across_reopen() {
    let root = temp_root("ordering");
    std::fs::create_dir_all(&root).expect("create temp root");
    let path = root.join("tasks.db");
    let task_a = TaskId::new();
    let task_b = TaskId::new();

    {
        let store = TaskStore::open(&path).expect("open task store");
        let first = store
            .append(task_a, 0, source(), vec![event("a.1"), event("a.2")])
            .expect("append task A events");
        let second = store
            .append(task_b, 0, source(), vec![event("b.1")])
            .expect("append task B event");

        let offsets = first
            .iter()
            .chain(&second)
            .map(|event| event.global_offset)
            .collect::<Vec<_>>();
        assert_eq!(offsets, vec![1, 2, 3]);
        assert_eq!(store.stream_version(task_a).unwrap(), 2);
        assert_eq!(store.stream_version(task_b).unwrap(), 1);

        assert!(matches!(
            store.append(task_a, 0, source(), vec![event("stale")]),
            Err(TaskStoreError::WrongExpectedVersion {
                expected: 0,
                actual: 2
            })
        ));

        let after_one = store
            .events_after(1, 100)
            .expect("read events after offset");
        assert_eq!(
            after_one
                .iter()
                .map(|event| event.global_offset)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(store.events_after(0, 0).unwrap().len(), 1);
    }

    let reopened = TaskStore::open(&path).expect("reopen task store");
    let fourth = reopened
        .append(task_a, 2, source(), vec![event("a.3")])
        .expect("append after reopen");
    assert_eq!(fourth[0].global_offset, 4);
    assert_eq!(fourth[0].stream_sequence, 3);

    drop(reopened);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn task_store_rejects_negative_persisted_sequences() {
    let root = temp_root("negative-sequence");
    std::fs::create_dir_all(&root).expect("create temp root");
    let path = root.join("tasks.db");
    drop(TaskStore::open(&path).expect("open task store"));

    let task_id = TaskId::new();
    let connection = rusqlite::Connection::open(&path).expect("open raw database");
    connection
        .execute(
            "INSERT INTO event_log (
                global_offset, task_id, stream_sequence, event_id, event_type,
                schema_version, recorded_at, source_json, payload_json
             ) VALUES (1, ?1, -1, ?2, 'corrupt', 1, '2026-07-10T00:00:00Z', ?3, '{}')",
            rusqlite::params![
                task_id.to_string(),
                harness_contracts::EventId::new().to_string(),
                serde_json::to_string(&source()).unwrap(),
            ],
        )
        .expect("insert corrupt row");
    drop(connection);

    let store = TaskStore::open(&path).expect("reopen task store");
    assert!(matches!(
        store.events_after(0, 10),
        Err(TaskStoreError::IntegerOutOfRange)
    ));

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn task_store_initializes_one_strict_wal_schema() {
    let root = temp_root("schema");
    std::fs::create_dir_all(&root).expect("create temp root");
    let path = root.join("tasks.db");
    drop(TaskStore::open(&path).expect("open task store"));

    let connection = rusqlite::Connection::open(&path).expect("inspect task database");
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode, "wal");

    let tables = [
        "event_log",
        "command_inbox",
        "task_projection",
        "run_projection",
        "queue_projection",
        "permission_projection",
        "subagent_projection",
        "workspace_projection",
        "timeline_projection",
        "checkpoints",
        "blob_metadata",
        "workspace_leases",
    ];
    for table in tables {
        let strict: i64 = connection
            .query_row(
                "SELECT strict FROM pragma_table_list WHERE name = ?1",
                [table],
                |row| row.get(0),
            )
            .unwrap_or_else(|error| panic!("missing table {table}: {error}"));
        assert_eq!(strict, 1, "table {table} is not strict");
    }

    drop(connection);
    let _ = std::fs::remove_dir_all(root);
}

fn source() -> EventSource {
    EventSource {
        kind: EventSourceKind::User,
        actor_id: None,
        client_id: None,
    }
}

fn event(event_type: &str) -> NewTaskEvent {
    NewTaskEvent {
        event_type: event_type.to_owned(),
        schema_version: 1,
        payload: json!({ "label": event_type }),
    }
}

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-task-store-{name}-{}-{}",
        std::process::id(),
        TaskId::new()
    ))
}

use std::path::PathBuf;

use crate::{EventAuthority, NewTaskEvent, TaskStore, TaskStoreError};
use chrono::Utc;
use harness_contracts::{
    ClientId, EventSource, EventSourceKind, QueueItemId, RunSegmentId, RunTerminalReason, TaskId,
};

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
            .append(
                task_a,
                0,
                &authority(),
                vec![
                    NewTaskEvent::task_created("A"),
                    NewTaskEvent::task_title_changed("A2"),
                ],
            )
            .expect("append task A events");
        let second = store
            .append(
                task_b,
                0,
                &authority(),
                vec![NewTaskEvent::task_created("B")],
            )
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
            store.append(
                task_a,
                0,
                &authority(),
                vec![NewTaskEvent::task_title_changed("stale")]
            ),
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
        .append(
            task_a,
            2,
            &authority(),
            vec![NewTaskEvent::task_title_changed("A3")],
        )
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
fn task_store_bounds_transaction_bytes_and_persisted_payloads() {
    let root = temp_root("bounded-payloads");
    std::fs::create_dir_all(&root).expect("create temp root");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let store = TaskStore::open(&path).unwrap();
    store
        .append(
            task_id,
            0,
            &authority(),
            vec![NewTaskEvent::task_created("bounded")],
        )
        .unwrap();

    let content = "x".repeat(64 * 1024);
    let events = (0..129)
        .map(|_| {
            NewTaskEvent::message_queued(
                QueueItemId::new(),
                content.clone(),
                vec![],
                vec![],
                Utc::now(),
            )
        })
        .collect();
    assert!(matches!(
        store.append(task_id, 1, &authority(), events),
        Err(TaskStoreError::InvalidInput(_))
    ));
    assert_eq!(store.latest_global_offset().unwrap(), 1);
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO event_log (
                task_id, stream_sequence, event_id, event_type, schema_version,
                recorded_at, source_json, payload_json
             ) VALUES (?1, 2, ?2, 'task.title_changed', 1,
                '2026-07-10T00:00:00Z', ?3, ?4)",
            rusqlite::params![
                task_id.to_string(),
                harness_contracts::EventId::new().to_string(),
                serde_json::to_string(&source()).unwrap(),
                format!("{{\"title\":\"{}\"}}", "x".repeat(1024 * 1024)),
            ],
        )
        .unwrap();
    drop(connection);

    let store = TaskStore::open(&path).unwrap();
    assert!(matches!(
        store.events_after(1, 1),
        Err(TaskStoreError::InvalidInput(_))
    ));

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn task_store_bounds_public_event_pages() {
    let root = temp_root("bounded-event-page");
    std::fs::create_dir_all(&root).expect("create temp root");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let store = TaskStore::open(&path).unwrap();
    let mut events = vec![NewTaskEvent::task_created("page")];
    events.extend((0..20).map(|index| NewTaskEvent::task_title_changed(index.to_string())));
    store
        .append(task_id, 0, &authority(), events)
        .expect("seed one page plus more events");

    assert_eq!(store.events_after(0, usize::MAX).unwrap().len(), 16);

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn task_audit_event_pages_walk_backwards_without_cross_task_leaks() {
    let root = temp_root("task-audit-event-pages");
    std::fs::create_dir_all(&root).expect("create temp root");
    let path = root.join("tasks.db");
    let store = TaskStore::open(&path).unwrap();
    let task_id = TaskId::new();
    let other_task_id = TaskId::new();
    let mut task_events = vec![NewTaskEvent::task_created("audit page")];
    task_events.extend((0..20).map(|index| NewTaskEvent::task_title_changed(index.to_string())));
    store.append(task_id, 0, &authority(), task_events).unwrap();
    store
        .append(
            other_task_id,
            0,
            &authority(),
            vec![NewTaskEvent::task_created("other task")],
        )
        .unwrap();

    let (latest, next_before_offset) = store
        .task_event_page_before(task_id, None, usize::MAX)
        .unwrap();
    assert_eq!(latest.len(), 16);
    assert!(latest
        .windows(2)
        .all(|pair| pair[0].global_offset < pair[1].global_offset));
    assert!(latest.iter().all(|event| event.task_id == task_id));

    let (older, final_cursor) = store
        .task_event_page_before(task_id, next_before_offset, usize::MAX)
        .unwrap();
    assert_eq!(older.len(), 5);
    assert!(older.iter().all(|event| event.task_id == task_id));
    assert_eq!(final_cursor, None);
    assert!(older.last().unwrap().global_offset < latest.first().unwrap().global_offset);

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn nonterminal_task_projection_pages_are_bounded_and_exclude_terminal_tasks() {
    let root = temp_root("nonterminal-projection-pages");
    std::fs::create_dir_all(&root).expect("create temp root");
    let path = root.join("tasks.db");
    let store = TaskStore::open(&path).unwrap();
    let supervisor = TaskStore::supervisor_authority();
    let mut active_task_ids = Vec::new();
    for index in 0..20 {
        let task_id = TaskId::new();
        active_task_ids.push(task_id);
        store
            .append(
                task_id,
                0,
                &supervisor,
                vec![
                    NewTaskEvent::task_created(format!("active {index}")),
                    NewTaskEvent::run_started(RunSegmentId::new(), Utc::now()),
                ],
            )
            .unwrap();
    }
    for index in 0..5 {
        let task_id = TaskId::new();
        let segment_id = RunSegmentId::new();
        store
            .append(
                task_id,
                0,
                &supervisor,
                vec![
                    NewTaskEvent::task_created(format!("terminal {index}")),
                    NewTaskEvent::run_started(segment_id, Utc::now()),
                    NewTaskEvent::run_completed(
                        segment_id,
                        Utc::now(),
                        RunTerminalReason::Completed,
                        false,
                    ),
                ],
            )
            .unwrap();
    }

    let first = store
        .nonterminal_task_projections_after(None, usize::MAX)
        .unwrap();
    assert_eq!(first.len(), 16);
    let second = store
        .nonterminal_task_projections_after(Some(first.last().unwrap().task_id), usize::MAX)
        .unwrap();
    assert_eq!(second.len(), 4);
    let mut actual = first
        .into_iter()
        .chain(second)
        .map(|projection| projection.task_id)
        .collect::<Vec<_>>();
    actual.sort_by_key(ToString::to_string);
    active_task_ids.sort_by_key(ToString::to_string);
    assert_eq!(actual, active_task_ids);

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
        "blob_ownership",
        "blob_staging",
        "blob_store_config",
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
        client_id: Some(ClientId::new()),
    }
}

fn authority() -> EventAuthority {
    TaskStore::user_authority(ClientId::new())
}

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-task-store-{name}-{}-{}",
        std::process::id(),
        TaskId::new()
    ))
}

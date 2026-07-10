use std::path::{Path, PathBuf};

use crate::{
    AcceptedCommand, EventAuthority, NewTaskEvent, ProjectionCounts, TaskStore, TaskStoreError,
};
use chrono::{TimeZone, Utc};
use harness_contracts::{
    ActorId, BlobId, CheckpointId, ClientId, CommandId, PermissionProjection, PermissionRoute,
    QueueItemId, QueueItemState, RequestId, RunSegmentId, RunState, RunTerminalReason, TaskId,
    TaskState, WorkspaceLeaseId, WorkspaceLeaseProjection, WorkspaceMode,
};
use rusqlite::params;
use serde_json::json;

#[test]
fn typed_events_reduce_complete_task_run_queue_and_permission_state() {
    let root = temp_root("complete-reducers");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let queue_item_id = QueueItemId::new();
    let request_id = RequestId::new();
    let actor_id = ActorId::new();
    let lease_id = WorkspaceLeaseId::new();
    let blob_hash = blake3::hash(b"projection attachment");
    let mut blob_id_bytes = [0_u8; 16];
    blob_id_bytes.copy_from_slice(&blob_hash.as_bytes()[..16]);
    let blob_id = BlobId::from_u128(u128::from_be_bytes(blob_id_bytes));
    let started_at = Utc.with_ymd_and_hms(2026, 7, 10, 1, 2, 3).unwrap();
    let ended_at = Utc.with_ymd_and_hms(2026, 7, 10, 1, 4, 5).unwrap();
    let next_started_at = Utc.with_ymd_and_hms(2026, 7, 10, 1, 5, 0).unwrap();
    let next_ended_at = Utc.with_ymd_and_hms(2026, 7, 10, 1, 6, 0).unwrap();
    let next_segment_id = RunSegmentId::new();
    let queued_at = Utc.with_ymd_and_hms(2026, 7, 10, 1, 3, 0).unwrap();
    let store = TaskStore::open(&path).unwrap();
    let blob_id_text = blob_id.to_string();
    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Projected"),
    );
    store
        .stage_blob(
            task_id,
            blob_id,
            "text/plain",
            21,
            *blob_hash.as_bytes(),
            &format!("{}/{}.blob", &blob_id_text[..2], blob_id_text),
        )
        .unwrap();
    transact(
        &store,
        task_id,
        1,
        supervisor_source(),
        NewTaskEvent::run_started(segment_id, started_at),
    );
    transact(
        &store,
        task_id,
        2,
        user_source(),
        NewTaskEvent::message_queued(
            queue_item_id,
            "first",
            vec![blob_id],
            vec!["src/main.rs".into()],
            queued_at,
        ),
    );
    transact(
        &store,
        task_id,
        3,
        user_source(),
        NewTaskEvent::message_edited(
            queue_item_id,
            2,
            "edited",
            vec![blob_id],
            vec!["src/lib.rs".into()],
        ),
    );
    transact(
        &store,
        task_id,
        4,
        permission_source(),
        NewTaskEvent::permission_requested(PermissionProjection {
            request_id,
            revision: 1,
            route: PermissionRoute::ForegroundTask,
        }),
    );
    transact(
        &store,
        task_id,
        5,
        supervisor_source(),
        NewTaskEvent::subagent_spawned(actor_id, started_at),
    );
    transact(
        &store,
        task_id,
        6,
        supervisor_source(),
        NewTaskEvent::workspace_acquired(WorkspaceLeaseProjection {
            lease_id,
            task_id,
            mode: WorkspaceMode::ManagedWorktree,
            canonical_root: "/workspace".into(),
            worktree_path: Some("/workspace/.worktrees/task".into()),
            writable: true,
        }),
    );

    let before_resolution = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(before_resolution.title, "Projected");
    assert_eq!(before_resolution.state, TaskState::WaitingPermission);
    let run = before_resolution.current_run.unwrap();
    assert_eq!(run.segment_id, segment_id);
    assert_eq!(run.state, RunState::WaitingPermission);
    assert_eq!(run.started_at, started_at);
    assert_eq!(run.ended_at, None);
    let queue_item = &before_resolution.queue[0];
    assert_eq!(queue_item.queue_item_id, queue_item_id);
    assert_eq!(queue_item.state, QueueItemState::Queued);
    assert_eq!(queue_item.revision, 2);
    assert_eq!(queue_item.content, "edited");
    assert_eq!(queue_item.attachments, vec![blob_id]);
    assert_eq!(queue_item.context_references, vec!["src/lib.rs"]);
    assert_eq!(queue_item.created_at, queued_at);
    assert_eq!(
        before_resolution.pending_permission.unwrap().request_id,
        request_id
    );
    assert_eq!(
        store.projection_counts().unwrap(),
        ProjectionCounts {
            tasks: 1,
            runs: 1,
            queue_items: 1,
            permissions: 1,
            subagents: 1,
            workspaces: 1,
            timeline_items: 5,
        }
    );

    transact(
        &store,
        task_id,
        7,
        user_source(),
        NewTaskEvent::permission_resolved(request_id, 1),
    );
    transact(
        &store,
        task_id,
        8,
        supervisor_source(),
        NewTaskEvent::run_completed(segment_id, ended_at, RunTerminalReason::Completed, false),
    );
    transact_events(
        &store,
        task_id,
        9,
        supervisor_source(),
        vec![
            NewTaskEvent::run_started(next_segment_id, next_started_at),
            NewTaskEvent::message_consumed(queue_item_id, 3, next_segment_id),
        ],
    );
    transact(
        &store,
        task_id,
        11,
        supervisor_source(),
        NewTaskEvent::run_completed(
            next_segment_id,
            next_ended_at,
            RunTerminalReason::Completed,
            false,
        ),
    );
    transact(
        &store,
        task_id,
        12,
        user_source(),
        NewTaskEvent::task_archived(true),
    );

    let final_projection = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(final_projection.state, TaskState::Completed);
    assert!(final_projection.archived);
    assert!(final_projection.pending_permission.is_none());
    let run = final_projection.current_run.as_ref().unwrap();
    assert_eq!(run.segment_id, next_segment_id);
    assert_eq!(run.state, RunState::Completed);
    assert_eq!(run.terminal_reason, Some(RunTerminalReason::Completed));
    assert_eq!(run.started_at, next_started_at);
    assert_eq!(run.ended_at, Some(next_ended_at));
    assert!(final_projection.queue.is_empty());
    assert_eq!(final_projection.stream_version, 13);
    assert_eq!(final_projection.last_global_offset, 13);
    assert_eq!(store.projection_counts().unwrap().runs, 2);
    assert_eq!(store.projection_counts().unwrap().timeline_items, 11);
    let timeline = timeline(&path, task_id);
    let user_messages = timeline
        .iter()
        .filter(|item| item.kind == harness_contracts::TimelineEventKind::UserMessage)
        .collect::<Vec<_>>();
    assert_eq!(user_messages.len(), 1);
    assert_eq!(user_messages[0].summary, "edited");
    assert_eq!(user_messages[0].run_segment_id, Some(next_segment_id));

    let before = store.projection_counts().unwrap();
    store.rebuild_projections().unwrap();
    assert_eq!(store.projection_counts().unwrap(), before);
    assert_eq!(
        store.task_projection(task_id).unwrap().unwrap(),
        final_projection
    );
    for (authority, event) in [
        (
            permission_source(),
            NewTaskEvent::permission_requested(PermissionProjection {
                request_id,
                revision: 2,
                route: PermissionRoute::ForegroundTask,
            }),
        ),
        (
            supervisor_source(),
            NewTaskEvent::subagent_spawned(actor_id, next_started_at),
        ),
        (
            supervisor_source(),
            NewTaskEvent::workspace_acquired(WorkspaceLeaseProjection {
                lease_id,
                task_id,
                mode: WorkspaceMode::ManagedWorktree,
                canonical_root: "/other".into(),
                worktree_path: None,
                writable: false,
            }),
        ),
    ] {
        assert!(matches!(
            store.transact_command(command(task_id, 13, authority), |_| Ok(vec![event])),
            Err(TaskStoreError::Projector(_))
        ));
    }
    assert_eq!(store.stream_version(task_id).unwrap(), 13);

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_runs_require_permission_resolution_and_queue_consumption_requires_active_run() {
    let root = temp_root("transition-invariants");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let queue_item_id = QueueItemId::new();
    let request_id = RequestId::new();
    let now = Utc::now();
    let store = TaskStore::open(&path).unwrap();
    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Transitions"),
    );
    transact(
        &store,
        task_id,
        1,
        supervisor_source(),
        NewTaskEvent::run_started(segment_id, now),
    );
    transact(
        &store,
        task_id,
        2,
        user_source(),
        NewTaskEvent::message_queued(queue_item_id, "queued", vec![], vec![], now),
    );
    transact(
        &store,
        task_id,
        3,
        permission_source(),
        NewTaskEvent::permission_requested(PermissionProjection {
            request_id,
            revision: 1,
            route: PermissionRoute::ForegroundTask,
        }),
    );

    assert!(matches!(
        store.transact_command(command(task_id, 4, supervisor_source()), |_| Ok(vec![
            NewTaskEvent::run_completed(segment_id, now, RunTerminalReason::Completed, false,)
        ]),),
        Err(TaskStoreError::Projector(_))
    ));
    assert_eq!(store.stream_version(task_id).unwrap(), 4);
    assert!(store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .pending_permission
        .is_some());

    transact(
        &store,
        task_id,
        4,
        user_source(),
        NewTaskEvent::permission_resolved(request_id, 1),
    );
    assert!(matches!(
        store.transact_command(command(task_id, 5, user_source()), |_| {
            Ok(vec![NewTaskEvent::message_consumed(
                queue_item_id,
                2,
                RunSegmentId::new(),
            )])
        }),
        Err(TaskStoreError::Projector(_))
    ));
    assert_eq!(store.stream_version(task_id).unwrap(), 5);

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn run_segments_are_unique_and_timeline_preserves_terminal_reason() {
    let root = temp_root("run-identity-and-reason");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let now = Utc::now();
    let store = TaskStore::open(&path).unwrap();
    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Run history"),
    );
    transact(
        &store,
        task_id,
        1,
        supervisor_source(),
        NewTaskEvent::run_started(segment_id, now),
    );
    transact(
        &store,
        task_id,
        2,
        supervisor_source(),
        NewTaskEvent::run_completed(segment_id, now, RunTerminalReason::Failed, false),
    );

    assert!(matches!(
        store.transact_command(command(task_id, 3, supervisor_source()), |_| {
            Ok(vec![NewTaskEvent::run_started(segment_id, now)])
        }),
        Err(TaskStoreError::Projector(_))
    ));
    assert_eq!(store.stream_version(task_id).unwrap(), 3);
    let timeline = timeline(&path, task_id);
    assert_eq!(timeline.last().unwrap().summary, "Run failed");

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn consumed_queue_item_ids_cannot_be_reused() {
    let root = temp_root("queue-identity");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let queue_item_id = QueueItemId::new();
    let segment_id = RunSegmentId::new();
    let now = Utc::now();
    let store = TaskStore::open(&path).unwrap();
    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Queue identity"),
    );
    transact(
        &store,
        task_id,
        1,
        user_source(),
        NewTaskEvent::message_queued(queue_item_id, "first", vec![], vec![], now),
    );
    transact(
        &store,
        task_id,
        2,
        supervisor_source(),
        NewTaskEvent::run_started(segment_id, now),
    );
    transact(
        &store,
        task_id,
        3,
        user_source(),
        NewTaskEvent::message_consumed(queue_item_id, 2, segment_id),
    );

    assert!(matches!(
        store.transact_command(command(task_id, 4, user_source()), |_| {
            Ok(vec![NewTaskEvent::message_queued(
                queue_item_id,
                "reused",
                vec![],
                vec![],
                now,
            )])
        }),
        Err(TaskStoreError::Projector(_))
    ));
    assert_eq!(store.stream_version(task_id).unwrap(), 4);

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn typed_event_boundary_rejects_unknown_events_and_invalid_entity_ids() {
    assert!(matches!(
        NewTaskEvent::from_parts(
            "permission.requested",
            1,
            json!({
                "requestId": "not-a-ulid",
                "revision": 1,
                "route": "foreground_task"
            })
        ),
        Err(TaskStoreError::InvalidId(_)) | Err(TaskStoreError::Json(_))
    ));
    assert!(matches!(
        NewTaskEvent::from_parts("permission.anything", 1, json!({})),
        Err(TaskStoreError::UnsupportedEvent { .. })
    ));
    assert!(matches!(
        NewTaskEvent::from_parts("task.created", 2, json!({ "title": "future" })),
        Err(TaskStoreError::UnsupportedEvent { .. })
    ));
    assert!(matches!(
        NewTaskEvent::from_parts("task.created", 1, json!({ "title": "x".repeat(4097) })),
        Err(TaskStoreError::InvalidInput(_))
    ));
    assert!(matches!(
        NewTaskEvent::from_parts(
            "message.queued",
            1,
            json!({
                "queueItemId": QueueItemId::new(),
                "content": "x".repeat(64 * 1024 + 1),
                "attachments": [],
                "contextReferences": [],
                "createdAt": Utc::now(),
            })
        ),
        Err(TaskStoreError::InvalidInput(_))
    ));
}

#[test]
fn active_queue_is_bounded_and_rebuild_repairs_projection_corruption() {
    let root = temp_root("bounded-queue-repair");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let store = TaskStore::open(&path).unwrap();
    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Canonical"),
    );
    let queue_events = (0..65)
        .map(|_| {
            NewTaskEvent::message_queued(QueueItemId::new(), "queued", vec![], vec![], Utc::now())
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        store.transact_command(command(task_id, 1, user_source()), |_| Ok(queue_events)),
        Err(TaskStoreError::Projector(_))
    ));
    assert_eq!(store.stream_version(task_id).unwrap(), 1);
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    let mut projection: serde_json::Value = serde_json::from_str(
        &connection
            .query_row(
                "SELECT projection_json FROM task_projection WHERE task_id = ?1",
                [task_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
    )
    .unwrap();
    projection["title"] = json!("Tampered");
    connection
        .execute(
            "UPDATE task_projection SET projection_json = ?2 WHERE task_id = ?1",
            params![task_id.to_string(), projection.to_string()],
        )
        .unwrap();
    drop(connection);

    let store = TaskStore::open(&path).unwrap();
    store.rebuild_projections().unwrap();
    assert_eq!(
        store.task_projection(task_id).unwrap().unwrap().title,
        "Canonical"
    );

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn task_stream_must_start_with_task_created() {
    let root = temp_root("create-first");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let store = TaskStore::open(&path).unwrap();

    assert!(matches!(
        store.append(
            task_id,
            0,
            &supervisor_source(),
            vec![NewTaskEvent::run_started(RunSegmentId::new(), Utc::now())],
        ),
        Err(TaskStoreError::Projector(_))
    ));
    assert_eq!(store.latest_global_offset().unwrap(), 0);
    assert!(store.task_projection(task_id).unwrap().is_none());

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rebuild_preserves_all_non_projection_tables() {
    let root = temp_root("rebuild-preserves-truth");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let store = TaskStore::open(&path).unwrap();
    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Projected"),
    );
    drop(store);

    seed_non_projection_rows(&path, task_id, segment_id);
    let before = non_projection_dump(&path);
    let store = TaskStore::open(&path).unwrap();
    store.rebuild_projections().unwrap();
    drop(store);
    let after = non_projection_dump(&path);
    assert_eq!(after, before);

    let _ = std::fs::remove_dir_all(root);
}

fn transact(
    store: &TaskStore,
    task_id: TaskId,
    expected_stream_version: u64,
    authority: EventAuthority,
    event: NewTaskEvent,
) {
    transact_events(
        store,
        task_id,
        expected_stream_version,
        authority,
        vec![event],
    );
}

fn transact_events(
    store: &TaskStore,
    task_id: TaskId,
    expected_stream_version: u64,
    authority: EventAuthority,
    events: Vec<NewTaskEvent>,
) {
    store
        .transact_command(command(task_id, expected_stream_version, authority), |_| {
            Ok(events)
        })
        .unwrap();
}

fn command(
    task_id: TaskId,
    expected_stream_version: u64,
    authority: EventAuthority,
) -> AcceptedCommand {
    AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("idem-{}", CommandId::new()),
        expected_stream_version,
        authority,
        payload: json!({ "event": expected_stream_version + 1 }),
    }
}

fn timeline(path: &Path, task_id: TaskId) -> Vec<harness_contracts::TimelineItemProjection> {
    let connection = rusqlite::Connection::open(path).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT projection_json FROM timeline_projection
             WHERE task_id = ?1 ORDER BY global_offset",
        )
        .unwrap();
    statement
        .query_map([task_id.to_string()], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
        .collect()
}

fn user_source() -> EventAuthority {
    TaskStore::user_authority(ClientId::new())
}

fn supervisor_source() -> EventAuthority {
    TaskStore::supervisor_authority()
}

fn permission_source() -> EventAuthority {
    TaskStore::permission_broker_authority()
}

fn seed_non_projection_rows(path: &Path, task_id: TaskId, segment_id: RunSegmentId) {
    let connection = rusqlite::Connection::open(path).unwrap();
    let blob_id = BlobId::new();
    connection
        .execute(
            "INSERT INTO checkpoints (
                checkpoint_id, task_id, run_segment_id, committed_global_offset,
                checkpoint_json, created_at
             ) VALUES (?1, ?2, ?3, 1, '{}', '2026-07-10T00:00:00Z')",
            params![
                CheckpointId::new().to_string(),
                task_id.to_string(),
                segment_id.to_string()
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO blob_metadata (
                blob_id, media_type, byte_size, content_hash, relative_path, created_at
             ) VALUES (?1, 'text/plain', 4, 'hash', 'blob/path', '2026-07-10T00:00:00Z')",
            [blob_id.to_string()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO blob_ownership (task_id, blob_id, media_type, created_at)
             VALUES (?1, ?2, 'text/plain', '2026-07-10T00:00:00Z')",
            params![task_id.to_string(), blob_id.to_string()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO blob_store_config (singleton, store_id, canonical_root)
             VALUES (1, 'store-id', '/app/blobs')
             ON CONFLICT(singleton) DO UPDATE SET canonical_root = excluded.canonical_root",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO workspace_leases (
                workspace_lease_id, task_id, canonical_root, mode, writable, state,
                acquired_at, expires_at, lease_json
             ) VALUES (?1, ?2, '/workspace', 'current', 1, 'active',
                '2026-07-10T00:00:00Z', NULL, '{}')",
            params![WorkspaceLeaseId::new().to_string(), task_id.to_string()],
        )
        .unwrap();
}

fn non_projection_dump(path: &Path) -> Vec<String> {
    let connection = rusqlite::Connection::open(path).unwrap();
    [
        ("event_log", "global_offset"),
        ("command_inbox", "command_id"),
        ("checkpoints", "checkpoint_id"),
        ("blob_metadata", "blob_id"),
        ("blob_ownership", "task_id, blob_id"),
        ("blob_staging", "task_id, blob_id"),
        ("blob_store_config", "singleton"),
        ("workspace_leases", "workspace_lease_id"),
    ]
    .into_iter()
    .flat_map(|(table, order)| {
        let mut statement = connection
            .prepare(&format!("SELECT * FROM {table} ORDER BY {order}"))
            .unwrap();
        let column_count = statement.column_count();
        statement
            .query_map([], |row| {
                let mut values = Vec::with_capacity(column_count);
                for index in 0..column_count {
                    values.push(row.get::<_, rusqlite::types::Value>(index)?);
                }
                Ok(format!("{table}:{values:?}"))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    })
    .collect()
}

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "jyowo-task-projection-{name}-{}-{}",
        std::process::id(),
        TaskId::new()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

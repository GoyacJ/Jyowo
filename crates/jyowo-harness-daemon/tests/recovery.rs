use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use chrono::Utc;
use harness_contracts::{
    ActorId, ClientId, CommandId, DeferPolicy, Event, EventId, IndeterminateToolDecision,
    IndeterminateToolResolution, NoopRedactor, PermissionMode, PermissionProjection,
    PermissionRoute, QueueItemId, QueueItemState, RequestId, RunId, RunSegmentId, RunState,
    RunTerminalReason, SessionId, TaskId, TenantId, ToolProperties, ToolResult,
    ToolUseCompletedEvent, ToolUseId, ToolUseRequestedEvent, ToolUseStartedEvent,
};
use harness_daemon::{
    CheckpointService, CheckpointState, RecoveryService, RunCoordinatorFactory, RunningSegment,
    StartSegmentRequest, Supervisor, SupervisorError, SupervisorQuotas, ValidatedTaskCommand,
    WorkspaceBaseline,
};
use harness_journal::{
    AcceptedCommand, CommandOutcome, EventStore, NewTaskEvent, TaskEventStoreAdapter, TaskStore,
};
use serde_json::json;

#[test]
fn startup_recovery_interrupts_a_running_segment() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("recover running"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );

    let report = RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();

    assert_eq!(report.recovered_tasks.len(), 1);
    assert_eq!(report.recovered_tasks[0].task_id, task_id);
    let projection = store.task_projection(task_id).unwrap().unwrap();
    let run = projection.current_run.unwrap();
    assert_eq!(run.state, RunState::Interrupted);
    assert_eq!(
        run.terminal_reason,
        Some(RunTerminalReason::InterruptedByRestart)
    );
}

#[test]
fn startup_recovery_processes_more_than_one_task_page() {
    let (store, _root) = test_store();
    let mut task_ids = Vec::new();
    for index in 0..20 {
        let task_id = TaskId::new();
        task_ids.push(task_id);
        append(
            &store,
            task_id,
            0,
            vec![
                NewTaskEvent::task_created(format!("paged recovery {index}")),
                NewTaskEvent::run_started(RunSegmentId::new(), Utc::now()),
            ],
        );
    }

    let report = RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();

    assert_eq!(report.recovered_tasks.len(), task_ids.len());
    for task_id in task_ids {
        assert_eq!(
            store
                .task_projection(task_id)
                .unwrap()
                .unwrap()
                .current_run
                .unwrap()
                .state,
            RunState::Interrupted
        );
    }
}

#[test]
fn startup_recovery_returns_a_promoting_message_to_the_queue() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let queue_item_id = QueueItemId::new();
    let now = Utc::now();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("recover promotion"),
            NewTaskEvent::run_started(segment_id, now),
            NewTaskEvent::message_queued(
                queue_item_id,
                "queued after restart",
                Vec::new(),
                Vec::new(),
                now,
            ),
            NewTaskEvent::message_promoted(queue_item_id, 1),
            NewTaskEvent::run_yield_requested(segment_id, false, now),
        ],
    );

    RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();

    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(projection.current_run.unwrap().state, RunState::Interrupted);
    assert_eq!(projection.queue[0].state, QueueItemState::Queued);
}

#[test]
fn startup_recovery_invalidates_a_pending_runtime_permission() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let request_id = RequestId::new();
    let now = Utc::now();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("recover permission"),
            NewTaskEvent::run_started(segment_id, now),
        ],
    );
    append_permission(
        &store,
        task_id,
        2,
        vec![NewTaskEvent::permission_requested(PermissionProjection {
            request_id,
            revision: 1,
            route: PermissionRoute::ForegroundTask,
            details: None,
        })],
    );

    RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();

    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert!(projection.pending_permission.is_none());
    assert_eq!(projection.current_run.unwrap().state, RunState::Interrupted);
    assert!(store
        .events_after(0, 100)
        .unwrap()
        .iter()
        .any(|event| event.event_type == "permission.invalidated"));
}

#[tokio::test]
async fn startup_recovery_rejects_tool_history_corrupted_before_the_latest_checkpoint() {
    let (store, root) = test_store();
    let database_path = root.path().join("tasks.sqlite");
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("bounded tool recovery"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );
    let session_id = SessionId::new();
    let tool_use_id = ToolUseId::new();
    TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    )
    .append(
        TenantId::SINGLE,
        session_id,
        &[
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id: RunId::new(),
                tool_use_id,
                tool_name: "write_file".into(),
                input: json!({ "path": "result.txt" }),
                properties: ToolProperties {
                    is_concurrency_safe: false,
                    is_read_only: false,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                causation_id: EventId::new(),
                at: Utc::now(),
            }),
            Event::ToolUseStarted(ToolUseStartedEvent {
                run_id: RunId::new(),
                tool_use_id,
                at: Utc::now(),
            }),
        ],
    )
    .await
    .unwrap();
    let request_id = RequestId::new();
    append_permission(
        &store,
        task_id,
        4,
        vec![NewTaskEvent::permission_requested(PermissionProjection {
            request_id,
            revision: 1,
            route: PermissionRoute::ForegroundTask,
            details: None,
        })],
    );
    assert_eq!(
        store
            .latest_checkpoint(task_id)
            .unwrap()
            .unwrap()
            .incomplete_tool_use_ids,
        vec![tool_use_id]
    );
    rusqlite::Connection::open(&database_path)
        .unwrap()
        .execute(
            "UPDATE event_log SET payload_json = '{}'
             WHERE task_id = ?1 AND event_type = 'engine.tool_use_started'",
            [task_id.to_string()],
        )
        .unwrap();

    assert!(matches!(
        RecoveryService::new(Arc::clone(&store)).recover_startup(),
        Err(harness_journal::TaskStoreError::ProjectionIntegrity(_))
    ));
}

#[test]
fn startup_recovery_keeps_a_consumed_message_consumed() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let queue_item_id = QueueItemId::new();
    let now = Utc::now();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("recover consumed message"),
            NewTaskEvent::run_started(segment_id, now),
            NewTaskEvent::message_queued(
                queue_item_id,
                "already consumed",
                Vec::new(),
                Vec::new(),
                now,
            ),
            NewTaskEvent::message_consumed(queue_item_id, 1, segment_id),
        ],
    );

    RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();

    let item = store
        .queue_item_projection(task_id, queue_item_id)
        .unwrap()
        .unwrap();
    assert_eq!(item.state, QueueItemState::Consumed);
}

#[test]
fn startup_recovery_does_not_invalidate_a_resolved_permission() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let request_id = RequestId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("recover resolved permission"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );
    append_permission(
        &store,
        task_id,
        2,
        vec![
            NewTaskEvent::permission_requested(PermissionProjection {
                request_id,
                revision: 1,
                route: PermissionRoute::ForegroundTask,
                details: None,
            }),
            NewTaskEvent::permission_resolved(request_id, 1),
        ],
    );

    RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();

    assert!(!store
        .events_after(0, 100)
        .unwrap()
        .iter()
        .any(|event| event.event_type == "permission.invalidated"));
}

#[test]
fn startup_recovery_preserves_child_actor_references() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let child_actor_id = ActorId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("recover child actor"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
            NewTaskEvent::subagent_spawned(child_actor_id, Utc::now()),
        ],
    );
    CheckpointService::new(Arc::clone(&store))
        .persist(
            task_id,
            segment_id,
            CheckpointState {
                child_actor_refs: vec![child_actor_id],
                ..CheckpointState::default()
            },
        )
        .unwrap();

    RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();

    assert_eq!(
        store
            .latest_checkpoint(task_id)
            .unwrap()
            .unwrap()
            .child_actor_refs,
        vec![child_actor_id]
    );
    assert_eq!(
        store
            .task_events_after(task_id, 0, 100)
            .unwrap()
            .iter()
            .filter(|event| event.event_type == "subagent.spawned")
            .count(),
        1
    );
}

#[test]
fn startup_recovery_leaves_a_terminal_segment_unchanged() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let now = Utc::now();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("terminal task"),
            NewTaskEvent::run_started(segment_id, now),
            NewTaskEvent::run_completed(segment_id, now, RunTerminalReason::Completed, false),
        ],
    );
    let offset_before = store.latest_global_offset().unwrap();

    let report = RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();

    assert!(report.recovered_tasks.is_empty());
    assert_eq!(store.latest_global_offset().unwrap(), offset_before);
    let run = store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .current_run
        .unwrap();
    assert_eq!(run.state, RunState::Completed);
    assert_eq!(run.terminal_reason, Some(RunTerminalReason::Completed));
}

#[tokio::test]
async fn checkpoint_round_trips_the_safe_recovery_state() {
    let (store, root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let queue_item_id = QueueItemId::new();
    let incomplete_tool_use_id = ToolUseId::new();
    let child_actor_id = ActorId::new();
    let now = Utc::now();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("checkpoint"),
            NewTaskEvent::run_started(segment_id, now),
            NewTaskEvent::message_queued(
                queue_item_id,
                "revision one",
                Vec::new(),
                Vec::new(),
                now,
            ),
            NewTaskEvent::message_edited(queue_item_id, 2, "revision two", Vec::new(), Vec::new()),
            NewTaskEvent::message_consumed(queue_item_id, 2, segment_id),
        ],
    );
    let session_id = SessionId::new();
    TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    )
    .append(
        TenantId::SINGLE,
        session_id,
        &[Event::ToolUseStarted(ToolUseStartedEvent {
            run_id: RunId::new(),
            tool_use_id: incomplete_tool_use_id,
            at: now,
        })],
    )
    .await
    .unwrap();
    let checkpoint = CheckpointService::new(Arc::clone(&store))
        .persist(
            task_id,
            segment_id,
            CheckpointState {
                context_cursor: 17,
                workspace_baseline: Some(WorkspaceBaseline {
                    revision: "git:0123456789abcdef".into(),
                }),
                incomplete_tool_use_ids: vec![incomplete_tool_use_id],
                child_actor_refs: vec![child_actor_id],
                context_blob_id: None,
            },
        )
        .unwrap();

    assert_eq!(checkpoint.committed_global_offset, 6);
    assert_eq!(checkpoint.queue_revision, 2);
    assert!(CheckpointService::new(Arc::clone(&store))
        .persist(
            task_id,
            segment_id,
            CheckpointState {
                incomplete_tool_use_ids: vec![incomplete_tool_use_id, incomplete_tool_use_id,],
                child_actor_refs: vec![child_actor_id, child_actor_id],
                ..CheckpointState::default()
            },
        )
        .is_err());
    drop(store);
    let reopened = TaskStore::open(root.path().join("tasks.sqlite")).unwrap();
    let restored = reopened.latest_checkpoint(task_id).unwrap().unwrap();
    assert_eq!(restored, checkpoint);
    assert_eq!(
        restored.incomplete_tool_use_ids,
        vec![incomplete_tool_use_id]
    );
    assert_eq!(restored.child_actor_refs, vec![child_actor_id]);
}

#[tokio::test]
async fn supervisor_recovers_an_active_run_when_the_task_is_first_routed() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("startup recovery"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );

    let supervisor = Supervisor::start(
        Arc::clone(&store),
        Arc::new(RecordingFactory::default()),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    assert_eq!(
        store
            .task_projection(task_id)
            .unwrap()
            .unwrap()
            .current_run
            .unwrap()
            .state,
        RunState::Running
    );
    let queue_item_id = QueueItemId::new();
    let outcome = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::Queue {
                command: AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: format!("route-{}", CommandId::new()),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                    authority: TaskStore::user_authority(ClientId::new()),
                    payload: json!({ "type": "queue" }),
                },
                queue_item_id,
                queue_command: harness_daemon::QueueCommand::Submit {
                    queue_item_id,
                    content: "recover before routing".into(),
                    attachments: Vec::new(),
                    context_references: Vec::new(),
                    created_at: Utc::now(),
                },
            },
        )
        .await
        .unwrap();
    assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    assert_eq!(
        store
            .task_projection(task_id)
            .unwrap()
            .unwrap()
            .current_run
            .unwrap()
            .state,
        RunState::Interrupted
    );
    drop(supervisor);
}

#[test]
fn startup_recovery_repairs_a_missing_checkpoint_after_the_interrupt_committed() {
    let (store, root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("repair recovery checkpoint"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );
    RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();
    rusqlite::Connection::open(root.path().join("tasks.sqlite"))
        .unwrap()
        .execute(
            "DELETE FROM checkpoints WHERE task_id = ?1",
            [task_id.to_string()],
        )
        .unwrap();
    assert!(store.latest_checkpoint(task_id).unwrap().is_none());

    RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();

    let checkpoint = store.latest_checkpoint(task_id).unwrap().unwrap();
    assert_eq!(checkpoint.run_segment_id, segment_id);
    assert_eq!(
        checkpoint.committed_global_offset,
        store
            .task_projection(task_id)
            .unwrap()
            .unwrap()
            .last_global_offset
    );
}

#[tokio::test]
async fn starting_a_segment_persists_its_committed_checkpoint() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    append(
        &store,
        task_id,
        0,
        vec![NewTaskEvent::task_created("checkpoint started segment")],
    );
    let segment_id = RunSegmentId::new();
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        Arc::new(RecordingFactory::default()),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    let outcome = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::StartSegment {
                command: AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: format!("start-{}", CommandId::new()),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                    authority: TaskStore::user_authority(ClientId::new()),
                    payload: json!({ "type": "start_segment" }),
                },
                segment_id,
                started_at: Utc::now(),
            },
        )
        .await
        .unwrap();

    assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
    let checkpoint = store.latest_checkpoint(task_id).unwrap().unwrap();
    assert_eq!(checkpoint.run_segment_id, segment_id);
    assert_eq!(
        checkpoint.committed_global_offset,
        store.latest_global_offset().unwrap()
    );
}

#[tokio::test]
async fn checkpoint_failure_rolls_back_run_start_before_coordinator_spawn() {
    let (store, root) = test_store();
    let database_path = root.path().join("tasks.sqlite");
    let task_id = TaskId::new();
    let completed_segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("atomic run start"),
            NewTaskEvent::run_started(completed_segment_id, Utc::now()),
            NewTaskEvent::run_completed(
                completed_segment_id,
                Utc::now(),
                RunTerminalReason::Completed,
                false,
            ),
        ],
    );
    let factory = Arc::new(RecordingFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();
    let duplicate_tool_id = ToolUseId::new();
    rusqlite::Connection::open(&database_path)
        .unwrap()
        .execute(
            "UPDATE checkpoints
             SET checkpoint_json = json_set(
                 checkpoint_json,
                 '$.incompleteToolUseIds',
                 json_array(?1, ?1)
             )
             WHERE task_id = ?2",
            rusqlite::params![duplicate_tool_id.to_string(), task_id.to_string()],
        )
        .unwrap();
    let stream_version = store.stream_version(task_id).unwrap();
    let command_id = CommandId::new();

    let result = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::StartSegment {
                command: AcceptedCommand {
                    command_id,
                    task_id,
                    idempotency_key: format!("atomic-start-{command_id}"),
                    expected_stream_version: stream_version,
                    authority: TaskStore::user_authority(ClientId::new()),
                    payload: json!({ "type": "start_segment" }),
                },
                segment_id: RunSegmentId::new(),
                started_at: Utc::now(),
            },
        )
        .await;

    assert!(matches!(&result, Err(SupervisorError::ActorStopped)));
    let event_types = store
        .task_events_after(task_id, 0, 100)
        .unwrap()
        .into_iter()
        .map(|event| event.event_type)
        .collect::<Vec<_>>();
    assert_eq!(
        event_types
            .iter()
            .filter(|event_type| event_type.as_str() == "run.started")
            .count(),
        1,
        "result={result:?}, events={event_types:?}"
    );
    assert_eq!(
        store
            .task_projection(task_id)
            .unwrap()
            .unwrap()
            .current_run
            .unwrap()
            .segment_id,
        completed_segment_id
    );
    assert!(factory.requests.lock().unwrap().is_empty());
    let accepted: bool = rusqlite::Connection::open(&database_path)
        .unwrap()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM command_inbox WHERE command_id = ?1)",
            [command_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!accepted);
}

#[tokio::test]
async fn startup_recovery_marks_a_started_tool_without_a_terminal_event_indeterminate() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("recover tool"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );
    let session_id = SessionId::new();
    let tool_use_id = ToolUseId::new();
    let adapter = TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    );
    adapter
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::ToolUseRequested(ToolUseRequestedEvent {
                    run_id: RunId::new(),
                    tool_use_id,
                    tool_name: "write_file".to_owned(),
                    input: json!({ "path": "result.txt" }),
                    properties: ToolProperties {
                        is_concurrency_safe: false,
                        is_read_only: false,
                        is_destructive: false,
                        long_running: None,
                        defer_policy: DeferPolicy::AlwaysLoad,
                    },
                    causation_id: EventId::new(),
                    at: Utc::now(),
                }),
                Event::ToolUseStarted(ToolUseStartedEvent {
                    run_id: RunId::new(),
                    tool_use_id,
                    at: Utc::now(),
                }),
            ],
        )
        .await
        .unwrap();

    let report = RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();

    assert_eq!(
        report.recovered_tasks[0].indeterminate_tool_use_ids,
        vec![tool_use_id]
    );
    assert!(store
        .events_after(0, 100)
        .unwrap()
        .iter()
        .any(|event| event.event_type == "tool.indeterminate"));
}

#[tokio::test]
async fn startup_recovery_does_not_mark_an_undispatched_tool_indeterminate() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("recover undispatched tool"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );
    let session_id = SessionId::new();
    let tool_use_id = ToolUseId::new();
    TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    )
    .append(
        TenantId::SINGLE,
        session_id,
        &[Event::ToolUseRequested(ToolUseRequestedEvent {
            run_id: RunId::new(),
            tool_use_id,
            tool_name: "write_file".into(),
            input: json!({ "path": "result.txt" }),
            properties: ToolProperties {
                is_concurrency_safe: false,
                is_read_only: false,
                is_destructive: false,
                long_running: None,
                defer_policy: DeferPolicy::AlwaysLoad,
            },
            causation_id: EventId::new(),
            at: Utc::now(),
        })],
    )
    .await
    .unwrap();

    let report = RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();

    assert!(report.recovered_tasks[0]
        .indeterminate_tool_use_ids
        .is_empty());
    assert!(!store
        .events_after(0, 100)
        .unwrap()
        .iter()
        .any(|event| event.event_type == "tool.indeterminate"));
}

#[tokio::test]
async fn startup_recovery_preserves_a_completed_tool_result() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("recover completed tool"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );
    let session_id = SessionId::new();
    let tool_use_id = ToolUseId::new();
    let adapter = TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    );
    adapter
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::ToolUseRequested(ToolUseRequestedEvent {
                    run_id: RunId::new(),
                    tool_use_id,
                    tool_name: "read_file".to_owned(),
                    input: json!({ "path": "result.txt" }),
                    properties: ToolProperties {
                        is_concurrency_safe: false,
                        is_read_only: true,
                        is_destructive: false,
                        long_running: None,
                        defer_policy: DeferPolicy::AlwaysLoad,
                    },
                    causation_id: EventId::new(),
                    at: Utc::now(),
                }),
                Event::ToolUseCompleted(ToolUseCompletedEvent {
                    tool_use_id,
                    result: ToolResult::Text("persisted result".into()),
                    usage: None,
                    duration_ms: 1,
                    at: Utc::now(),
                }),
            ],
        )
        .await
        .unwrap();

    let report = RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();

    assert!(report.recovered_tasks[0]
        .indeterminate_tool_use_ids
        .is_empty());
    let events = store.events_after(0, 100).unwrap();
    assert!(!events
        .iter()
        .any(|event| event.event_type == "tool.indeterminate"));
    let completed = events
        .iter()
        .find(|event| event.event_type == "engine.tool_use_completed")
        .unwrap();
    assert_eq!(
        completed.payload["event"]["result"],
        json!({ "text": "persisted result" })
    );
}

#[tokio::test]
async fn checkpoint_rejects_a_completed_tool_forged_as_incomplete() {
    let (store, root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("reject forged incomplete tool"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );
    let session_id = SessionId::new();
    let tool_use_id = ToolUseId::new();
    TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    )
    .append(
        TenantId::SINGLE,
        session_id,
        &[
            Event::ToolUseStarted(ToolUseStartedEvent {
                run_id: RunId::new(),
                tool_use_id,
                at: Utc::now(),
            }),
            Event::ToolUseCompleted(ToolUseCompletedEvent {
                tool_use_id,
                result: ToolResult::Text("persisted result".into()),
                usage: None,
                duration_ms: 1,
                at: Utc::now(),
            }),
        ],
    )
    .await
    .unwrap();
    let checkpoint = store.latest_checkpoint(task_id).unwrap().unwrap();
    assert!(checkpoint.incomplete_tool_use_ids.is_empty());

    rusqlite::Connection::open(root.path().join("tasks.sqlite"))
        .unwrap()
        .execute(
            "UPDATE checkpoints
             SET checkpoint_json = json_set(
                 checkpoint_json,
                 '$.incompleteToolUseIds',
                 json_array(?1)
             )
             WHERE checkpoint_id = ?2",
            rusqlite::params![
                tool_use_id.to_string(),
                checkpoint.checkpoint_id.to_string()
            ],
        )
        .unwrap();

    assert!(matches!(
        store.latest_checkpoint(task_id),
        Err(harness_journal::TaskStoreError::ProjectionIntegrity(_))
    ));
    assert!(matches!(
        store.transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: format!("test-{}", CommandId::new()),
                expected_stream_version: store.stream_version(task_id).unwrap(),
                authority: TaskStore::supervisor_authority(),
                payload: json!({ "type": "test_setup" }),
            },
            |_| Ok(vec![NewTaskEvent::run_completed(
                segment_id,
                Utc::now(),
                RunTerminalReason::Completed,
                false,
            )]),
        ),
        Err(harness_journal::TaskStoreError::ProjectionIntegrity(_))
    ));
}

#[test]
fn checkpoint_offset_must_belong_to_its_declared_run_segment() {
    let (store, root) = test_store();
    let task_id = TaskId::new();
    let first_segment_id = RunSegmentId::new();
    let second_segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("bind checkpoint segment"),
            NewTaskEvent::run_started(first_segment_id, Utc::now()),
            NewTaskEvent::run_completed(
                first_segment_id,
                Utc::now(),
                RunTerminalReason::Completed,
                false,
            ),
            NewTaskEvent::run_started(second_segment_id, Utc::now()),
        ],
    );
    let checkpoint = store.latest_checkpoint(task_id).unwrap().unwrap();
    let first_run_offset = store
        .run_started_global_offset(task_id, first_segment_id)
        .unwrap()
        .unwrap();
    rusqlite::Connection::open(root.path().join("tasks.sqlite"))
        .unwrap()
        .execute(
            "UPDATE checkpoints
             SET committed_global_offset = ?1,
                 checkpoint_json = json_set(
                     checkpoint_json,
                     '$.committedGlobalOffset',
                     ?1
                 )
             WHERE checkpoint_id = ?2",
            rusqlite::params![
                i64::try_from(first_run_offset).unwrap(),
                checkpoint.checkpoint_id.to_string(),
            ],
        )
        .unwrap();

    assert!(matches!(
        store.latest_checkpoint(task_id),
        Err(harness_journal::TaskStoreError::ProjectionIntegrity(_))
    ));
}

#[tokio::test]
async fn completed_tool_call_advances_the_safe_checkpoint() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("checkpoint completed tool"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );
    CheckpointService::new(Arc::clone(&store))
        .persist(
            task_id,
            segment_id,
            CheckpointState {
                context_cursor: 9,
                ..CheckpointState::default()
            },
        )
        .unwrap();
    let session_id = SessionId::new();
    let tool_use_id = ToolUseId::new();

    TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    )
    .append(
        TenantId::SINGLE,
        session_id,
        &[
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id: RunId::new(),
                tool_use_id,
                tool_name: "read_file".into(),
                input: json!({ "path": "result.txt" }),
                properties: ToolProperties {
                    is_concurrency_safe: false,
                    is_read_only: true,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                causation_id: EventId::new(),
                at: Utc::now(),
            }),
            Event::ToolUseCompleted(ToolUseCompletedEvent {
                tool_use_id,
                result: ToolResult::Text("persisted result".into()),
                usage: None,
                duration_ms: 1,
                at: Utc::now(),
            }),
        ],
    )
    .await
    .unwrap();

    let checkpoint = store.latest_checkpoint(task_id).unwrap().unwrap();
    assert_eq!(checkpoint.committed_global_offset, 4);
    assert_eq!(checkpoint.context_cursor, 9);
    assert!(checkpoint.incomplete_tool_use_ids.is_empty());
}

#[test]
fn permission_decision_advances_the_safe_checkpoint() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let request_id = RequestId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("checkpoint permission"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );
    CheckpointService::new(Arc::clone(&store))
        .persist(task_id, segment_id, CheckpointState::default())
        .unwrap();
    append_permission(
        &store,
        task_id,
        2,
        vec![NewTaskEvent::permission_requested(PermissionProjection {
            request_id,
            revision: 1,
            route: PermissionRoute::ForegroundTask,
            details: None,
        })],
    );
    append_permission(
        &store,
        task_id,
        3,
        vec![NewTaskEvent::permission_resolved(request_id, 1)],
    );

    assert_eq!(
        store
            .latest_checkpoint(task_id)
            .unwrap()
            .unwrap()
            .committed_global_offset,
        4
    );
}

#[test]
fn subagent_state_change_advances_the_checkpoint_and_child_refs() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let child_actor_id = ActorId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("checkpoint subagent"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );
    CheckpointService::new(Arc::clone(&store))
        .persist(task_id, segment_id, CheckpointState::default())
        .unwrap();
    append(
        &store,
        task_id,
        2,
        vec![NewTaskEvent::subagent_spawned(child_actor_id, Utc::now())],
    );

    let checkpoint = store.latest_checkpoint(task_id).unwrap().unwrap();
    assert_eq!(checkpoint.committed_global_offset, 3);
    assert_eq!(checkpoint.child_actor_refs, vec![child_actor_id]);
}

#[tokio::test]
async fn continue_task_requires_all_indeterminate_tool_decisions_and_starts_a_new_segment() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let interrupted_segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("continue recovered task"),
            NewTaskEvent::run_started(interrupted_segment_id, Utc::now()),
        ],
    );
    let session_id = SessionId::new();
    let tool_use_id = ToolUseId::new();
    TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    )
    .append(
        TenantId::SINGLE,
        session_id,
        &[
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id: RunId::new(),
                tool_use_id,
                tool_name: "write_file".into(),
                input: json!({ "path": "result.txt" }),
                properties: ToolProperties {
                    is_concurrency_safe: false,
                    is_read_only: false,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                causation_id: EventId::new(),
                at: Utc::now(),
            }),
            Event::ToolUseStarted(ToolUseStartedEvent {
                run_id: RunId::new(),
                tool_use_id,
                at: Utc::now(),
            }),
        ],
    )
    .await
    .unwrap();
    RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();
    let factory = Arc::new(RecordingFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    let bypass = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::StartSegment {
                command: AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: format!("bypass-{}", CommandId::new()),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                    authority: TaskStore::user_authority(ClientId::new()),
                    payload: json!({ "type": "start_segment" }),
                },
                segment_id: RunSegmentId::new(),
                started_at: Utc::now(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(bypass, CommandOutcome::Rejected { .. }));
    assert!(factory.requests.lock().unwrap().is_empty());

    let rejected_segment_id = RunSegmentId::new();
    let rejected = supervisor
        .dispatch(
            task_id,
            continue_command(&store, task_id, rejected_segment_id, Vec::new()),
        )
        .await
        .unwrap();
    assert!(matches!(rejected, CommandOutcome::Rejected { .. }));
    assert!(factory.requests.lock().unwrap().is_empty());

    let continued_segment_id = RunSegmentId::new();
    let accepted = supervisor
        .dispatch(
            task_id,
            continue_command(
                &store,
                task_id,
                continued_segment_id,
                vec![IndeterminateToolDecision {
                    tool_use_id: tool_use_id.to_string(),
                    resolution: IndeterminateToolResolution::TreatAsFailed,
                }],
            ),
        )
        .await
        .unwrap();
    assert!(matches!(accepted, CommandOutcome::Accepted { .. }));
    assert_ne!(continued_segment_id, interrupted_segment_id);
    let requests = factory.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].task_id, task_id);
    assert_eq!(requests[0].segment_id, continued_segment_id);
    assert_eq!(
        requests[0].indeterminate_tools,
        vec![IndeterminateToolDecision {
            tool_use_id: tool_use_id.to_string(),
            resolution: IndeterminateToolResolution::TreatAsFailed,
        }]
    );
    assert!(store
        .pending_segment_start(task_id, continued_segment_id)
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .task_events_after(task_id, 0, 100)
            .unwrap()
            .iter()
            .filter(|event| event.event_type == "engine.tool_use_requested")
            .count(),
        1
    );
}

#[tokio::test]
async fn continue_task_reuses_the_interrupted_segments_immutable_run_input() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let queue_item_id = QueueItemId::new();
    let interrupted_segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![NewTaskEvent::task_created("continue immutable input")],
    );
    append(
        &store,
        task_id,
        1,
        vec![
            NewTaskEvent::run_started(interrupted_segment_id, Utc::now()),
            NewTaskEvent::message_queued_with_runtime(
                queue_item_id,
                "resume this exact prompt",
                Vec::new(),
                vec!["context:durable".into()],
                Some("provider-config-continue".into()),
                PermissionMode::AcceptEdits,
                Utc::now(),
            ),
            NewTaskEvent::message_consumed(queue_item_id, 1, interrupted_segment_id),
        ],
    );
    RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();

    let factory = Arc::new(RecordingFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();
    let continued_segment_id = RunSegmentId::new();
    assert!(matches!(
        supervisor
            .dispatch(
                task_id,
                continue_command(&store, task_id, continued_segment_id, Vec::new()),
            )
            .await
            .unwrap(),
        CommandOutcome::Accepted { .. }
    ));

    let requests = factory.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].input.queue_item_id, Some(queue_item_id));
    assert_eq!(requests[0].input.content, "resume this exact prompt");
    assert_eq!(
        requests[0].input.context_references,
        vec!["context:durable"]
    );
    assert_eq!(
        requests[0].input.model_config_id.as_deref(),
        Some("provider-config-continue")
    );
    assert_eq!(
        requests[0].input.permission_mode,
        PermissionMode::AcceptEdits
    );
    assert_eq!(
        requests[0].input.session_id,
        SessionId::from_u128(u128::from_be_bytes(task_id.as_bytes()))
    );
    assert_eq!(
        requests[0].input.run_id,
        RunId::from_u128(u128::from_be_bytes(continued_segment_id.as_bytes()))
    );
}

#[tokio::test]
async fn committed_continue_decisions_are_redelivered_after_restart_before_spawn() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let interrupted_segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("redeliver committed continuation"),
            NewTaskEvent::run_started(interrupted_segment_id, Utc::now()),
        ],
    );
    let session_id = SessionId::new();
    let tool_use_id = ToolUseId::new();
    TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    )
    .append(
        TenantId::SINGLE,
        session_id,
        &[Event::ToolUseStarted(ToolUseStartedEvent {
            run_id: RunId::new(),
            tool_use_id,
            at: Utc::now(),
        })],
    )
    .await
    .unwrap();
    RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();
    let continued_segment_id = RunSegmentId::new();
    let decisions = vec![IndeterminateToolDecision {
        tool_use_id: tool_use_id.to_string(),
        resolution: IndeterminateToolResolution::ExecuteAgain,
    }];
    let command = AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("continue-{}", CommandId::new()),
        expected_stream_version: store.stream_version(task_id).unwrap(),
        authority: TaskStore::supervisor_authority(),
        payload: json!({
            "type": "continue_task",
            "segmentId": continued_segment_id,
            "indeterminateTools": decisions,
        }),
    };
    assert!(matches!(
        store.transact_command(command, |_| {
            Ok(vec![NewTaskEvent::run_started_with_recovery(
                continued_segment_id,
                Utc::now(),
                decisions.clone(),
            )])
        }),
        Ok(CommandOutcome::Accepted { .. })
    ));
    let expected_input = store
        .pending_segment_start(task_id, continued_segment_id)
        .unwrap()
        .unwrap()
        .input;

    let factory = Arc::new(RecordingFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();
    let queue_item_id = QueueItemId::new();
    let _ = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::Queue {
                command: AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: format!("route-{}", CommandId::new()),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                    authority: TaskStore::user_authority(ClientId::new()),
                    payload: json!({ "type": "queue" }),
                },
                queue_item_id,
                queue_command: harness_daemon::QueueCommand::Submit {
                    queue_item_id,
                    content: "route actor".into(),
                    attachments: Vec::new(),
                    context_references: Vec::new(),
                    created_at: Utc::now(),
                },
            },
        )
        .await;

    assert_eq!(
        factory.requests.lock().unwrap().as_slice(),
        &[StartSegmentRequest {
            task_id,
            segment_id: continued_segment_id,
            input: expected_input,
            indeterminate_tools: decisions,
        }]
    );
}

#[tokio::test]
async fn pending_continue_outbox_is_retried_after_actor_failure() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let interrupted_segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("retry pending continuation"),
            NewTaskEvent::run_started(interrupted_segment_id, Utc::now()),
            NewTaskEvent::run_completed(
                interrupted_segment_id,
                Utc::now(),
                RunTerminalReason::InterruptedByRestart,
                true,
            ),
        ],
    );
    let continued_segment_id = RunSegmentId::new();
    let command = AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("continue-{}", CommandId::new()),
        expected_stream_version: store.stream_version(task_id).unwrap(),
        authority: TaskStore::supervisor_authority(),
        payload: json!({
            "type": "continue_task",
            "segmentId": continued_segment_id,
            "indeterminateTools": [],
        }),
    };
    assert!(matches!(
        store.transact_command(command, |_| {
            Ok(vec![NewTaskEvent::run_started_with_recovery(
                continued_segment_id,
                Utc::now(),
                Vec::new(),
            )])
        }),
        Ok(CommandOutcome::Accepted { .. })
    ));

    let factory = Arc::new(PanicThreeTimesFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();
    let queue_item_id = QueueItemId::new();
    let dispatch = supervisor.dispatch(
        task_id,
        ValidatedTaskCommand::Queue {
            command: AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: format!("route-{}", CommandId::new()),
                expected_stream_version: store.stream_version(task_id).unwrap(),
                authority: TaskStore::user_authority(ClientId::new()),
                payload: json!({ "type": "queue" }),
            },
            queue_item_id,
            queue_command: harness_daemon::QueueCommand::Submit {
                queue_item_id,
                content: "route actor".into(),
                attachments: Vec::new(),
                context_references: Vec::new(),
                created_at: Utc::now(),
            },
        },
    );
    tokio::pin!(dispatch);
    tokio::select! {
        biased;
        _ = factory.first_attempt.notified() => {}
        _ = &mut dispatch => {
            panic!("dispatch returned before the first delivery attempt was observed");
        }
    }

    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(factory.attempts.load(Ordering::SeqCst), 1);

    tokio::time::timeout(Duration::from_secs(1), async {
        while factory.attempts.load(Ordering::SeqCst) < 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("pending segment start should be retried");
    drop(dispatch);
    assert!(store
        .pending_segment_start(task_id, continued_segment_id)
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .task_projection(task_id)
            .unwrap()
            .unwrap()
            .current_run
            .unwrap()
            .state,
        RunState::Running
    );
}

#[test]
fn pending_continue_outbox_must_match_the_canonical_run_start() {
    let (store, root) = test_store();
    let task_id = TaskId::new();
    let interrupted_segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("bind continuation outbox"),
            NewTaskEvent::run_started(interrupted_segment_id, Utc::now()),
            NewTaskEvent::run_completed(
                interrupted_segment_id,
                Utc::now(),
                RunTerminalReason::InterruptedByRestart,
                true,
            ),
        ],
    );
    let continued_segment_id = RunSegmentId::new();
    let command = AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("continue-{}", CommandId::new()),
        expected_stream_version: store.stream_version(task_id).unwrap(),
        authority: TaskStore::supervisor_authority(),
        payload: json!({
            "type": "continue_task",
            "segmentId": continued_segment_id,
            "indeterminateTools": [],
        }),
    };
    assert!(matches!(
        store.transact_command(command, |_| {
            Ok(vec![NewTaskEvent::run_started_with_recovery(
                continued_segment_id,
                Utc::now(),
                Vec::new(),
            )])
        }),
        Ok(CommandOutcome::Accepted { .. })
    ));
    rusqlite::Connection::open(root.path().join("tasks.sqlite"))
        .unwrap()
        .execute(
            "UPDATE segment_start_outbox
             SET request_json = json_set(
                 request_json,
                 '$.indeterminateTools',
                 json_array(json_object('toolUseId', ?1, 'resolution', 'execute_again'))
             )
             WHERE task_id = ?2 AND run_segment_id = ?3",
            rusqlite::params![
                ToolUseId::new().to_string(),
                task_id.to_string(),
                continued_segment_id.to_string(),
            ],
        )
        .unwrap();

    assert!(matches!(
        store.pending_segment_start(task_id, continued_segment_id),
        Err(harness_journal::TaskStoreError::ProjectionIntegrity(_))
    ));
    rusqlite::Connection::open(root.path().join("tasks.sqlite"))
        .unwrap()
        .execute(
            "DELETE FROM segment_start_outbox WHERE task_id = ?1 AND run_segment_id = ?2",
            rusqlite::params![task_id.to_string(), continued_segment_id.to_string()],
        )
        .unwrap();
    assert!(matches!(
        store.pending_segment_start(task_id, continued_segment_id),
        Err(harness_journal::TaskStoreError::ProjectionIntegrity(_))
    ));
}

#[derive(Default)]
struct RecordingFactory {
    requests: Mutex<Vec<StartSegmentRequest>>,
    senders: Mutex<Vec<tokio::sync::mpsc::UnboundedSender<harness_daemon::RunCoordinatorEvent>>>,
}

impl RunCoordinatorFactory for RecordingFactory {
    fn spawn_idempotent(
        &self,
        request: StartSegmentRequest,
        _workspace_tools: harness_daemon::WorkspaceToolDispatcher,
        _subagent_runner: Arc<dyn harness_subagent::SubagentRunner>,
    ) -> RunningSegment {
        self.requests.lock().unwrap().push(request);
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        self.senders.lock().unwrap().push(sender);
        RunningSegment::new(receiver)
    }
}

#[derive(Default)]
struct PanicThreeTimesFactory {
    attempts: AtomicUsize,
    first_attempt: tokio::sync::Notify,
    senders: Mutex<Vec<tokio::sync::mpsc::UnboundedSender<harness_daemon::RunCoordinatorEvent>>>,
}

impl RunCoordinatorFactory for PanicThreeTimesFactory {
    fn spawn_idempotent(
        &self,
        _request: StartSegmentRequest,
        _workspace_tools: harness_daemon::WorkspaceToolDispatcher,
        _subagent_runner: Arc<dyn harness_subagent::SubagentRunner>,
    ) -> RunningSegment {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
        if attempt == 0 {
            self.first_attempt.notify_one();
        }
        if attempt < 3 {
            panic!("simulated crash before outbox acknowledgement");
        }
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        self.senders.lock().unwrap().push(sender);
        RunningSegment::new(receiver)
    }
}

fn continue_command(
    store: &TaskStore,
    task_id: TaskId,
    segment_id: RunSegmentId,
    indeterminate_tools: Vec<IndeterminateToolDecision>,
) -> ValidatedTaskCommand {
    ValidatedTaskCommand::ContinueTask {
        command: AcceptedCommand {
            command_id: CommandId::new(),
            task_id,
            idempotency_key: format!("continue-{}", CommandId::new()),
            expected_stream_version: store.stream_version(task_id).unwrap(),
            authority: TaskStore::user_authority(ClientId::new()),
            payload: json!({ "type": "continue_task" }),
        },
        segment_id,
        started_at: Utc::now(),
        indeterminate_tools,
    }
}

fn append(store: &TaskStore, task_id: TaskId, version: u64, events: Vec<NewTaskEvent>) {
    append_with_authority(
        store,
        task_id,
        version,
        events,
        TaskStore::supervisor_authority(),
    );
}

fn append_permission(store: &TaskStore, task_id: TaskId, version: u64, events: Vec<NewTaskEvent>) {
    append_with_authority(
        store,
        task_id,
        version,
        events,
        TaskStore::permission_broker_authority(),
    );
}

fn append_with_authority(
    store: &TaskStore,
    task_id: TaskId,
    version: u64,
    events: Vec<NewTaskEvent>,
    authority: harness_journal::EventAuthority,
) {
    let previous_segment_id = store
        .task_projection(task_id)
        .unwrap()
        .and_then(|projection| projection.current_run)
        .map(|run| run.segment_id);
    let command = AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("test-{}", CommandId::new()),
        expected_stream_version: version,
        authority,
        payload: json!({ "type": "test_setup" }),
    };
    assert!(matches!(
        store.transact_command(command, |_| Ok(events)).unwrap(),
        CommandOutcome::Accepted { .. }
    ));
    let current_segment_id = store
        .task_projection(task_id)
        .unwrap()
        .and_then(|projection| projection.current_run)
        .map(|run| run.segment_id);
    if current_segment_id != previous_segment_id {
        if let Some(segment_id) = current_segment_id.filter(|segment_id| {
            store
                .pending_segment_start(task_id, *segment_id)
                .unwrap()
                .is_some()
        }) {
            store
                .mark_segment_start_delivered(task_id, segment_id)
                .unwrap();
        }
    }
}

fn test_store() -> (Arc<TaskStore>, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    (store, root)
}

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{
    AcceptedCommand, AppendMetadata, EventAuthority, EventStore, NewTaskEvent, ProjectionCounts,
    TaskEventStoreAdapter, TaskStore, TaskStoreError,
};
use chrono::{TimeZone, Utc};
use harness_contracts::{
    ActorId, AssistantDeltaProducedEvent, AssistantMessageCompletedEvent, BlobId, CheckpointId,
    ClientId, CommandId, DeltaChunk, EndReason, Event, MessageContent, MessageId, NoopRedactor,
    PermissionProjection, PermissionRoute, QueueItemId, QueueItemState, RequestId, RunEndedEvent,
    RunId, RunSegmentId, RunState, RunTerminalReason, SessionId, StopReason, TaskId, TaskState,
    TenantId, UsageSnapshot, WorkspaceLeaseId, WorkspaceLeaseProjection, WorkspaceLeaseState,
    WorkspaceMode,
};
use rusqlite::params;
use serde_json::json;

#[test]
fn engine_assistant_events_project_text_without_internal_lifecycle_notices() {
    let root = temp_root("engine-assistant-timeline");
    let path = root.join("tasks.db");
    let store = Arc::new(TaskStore::open(&path).unwrap());
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let run_id = RunId::new();
    let session_id = SessionId::new();
    let message_id = MessageId::new();
    let completion_only_id = MessageId::new();
    let at = Utc.with_ymd_and_hms(2026, 7, 12, 1, 2, 3).unwrap();

    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Engine output"),
    );
    transact(
        &store,
        task_id,
        1,
        supervisor_source(),
        NewTaskEvent::run_started(segment_id, at),
    );
    store
        .append_engine_events(
            task_id,
            TenantId::SINGLE,
            session_id,
            Some(segment_id),
            AppendMetadata {
                run_id: Some(run_id),
                ..AppendMetadata::default()
            },
            Some(0),
            &[
                Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                    run_id,
                    message_id,
                    delta: DeltaChunk::Text("First ".into()),
                    at,
                }),
                Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                    run_id,
                    message_id,
                    delta: DeltaChunk::Text("answer".into()),
                    at,
                }),
                Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
                    run_id,
                    message_id,
                    content: MessageContent::Text("First answer".into()),
                    tool_uses: Vec::new(),
                    usage: UsageSnapshot::default(),
                    pricing_snapshot_id: None,
                    stop_reason: StopReason::EndTurn,
                    at,
                }),
                Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
                    run_id,
                    message_id: completion_only_id,
                    content: MessageContent::Text("Completion fallback".into()),
                    tool_uses: Vec::new(),
                    usage: UsageSnapshot::default(),
                    pricing_snapshot_id: None,
                    stop_reason: StopReason::EndTurn,
                    at,
                }),
                Event::RunEnded(RunEndedEvent {
                    run_id,
                    reason: EndReason::Completed,
                    usage: Some(UsageSnapshot::default()),
                    ended_at: at,
                }),
            ],
        )
        .unwrap();

    let projected = timeline(&path, task_id)
        .into_iter()
        .filter(|item| item.kind == harness_contracts::TimelineEventKind::AssistantText)
        .collect::<Vec<_>>();
    assert_eq!(projected.len(), 3);
    assert_eq!(
        projected
            .iter()
            .map(|item| item.summary.as_str())
            .collect::<Vec<_>>(),
        vec!["First ", "answer", "Completion fallback"]
    );
    assert!(projected
        .iter()
        .all(|item| item.run_segment_id == Some(segment_id)));
    assert_eq!(
        projected
            .iter()
            .map(|item| item.semantic_group_id.clone())
            .collect::<Vec<_>>(),
        vec![
            Some(message_id.to_string()),
            Some(message_id.to_string()),
            Some(completion_only_id.to_string()),
        ]
    );
    assert_eq!(
        projected
            .iter()
            .map(|item| item.incomplete)
            .collect::<Vec<_>>(),
        vec![true, false, false]
    );
    assert!(!timeline(&path, task_id)
        .iter()
        .any(|item| item.summary == "run ended"));

    let before_rebuild = timeline(&path, task_id);
    store.rebuild_projections().unwrap();
    assert_eq!(timeline(&path, task_id), before_rebuild);

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn rebuild_preserves_legacy_engine_timeline_without_explicit_segment_binding() {
    let root = temp_root("legacy-engine-segment-fallback");
    let path = root.join("tasks.db");
    let store = Arc::new(TaskStore::open(&path).unwrap());
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let run_id = RunId::new();
    let session_id = SessionId::new();
    let message_id = MessageId::new();
    let at = Utc.with_ymd_and_hms(2026, 7, 12, 1, 2, 3).unwrap();
    let adapter = TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    );

    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Legacy engine output"),
    );
    transact(
        &store,
        task_id,
        1,
        supervisor_source(),
        NewTaskEvent::run_started(segment_id, at),
    );
    adapter
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                run_id,
                message_id,
                delta: DeltaChunk::Text("Legacy answer".into()),
                at,
            })],
        )
        .await
        .unwrap();

    let projected = timeline(&path, task_id);
    assert!(projected.iter().any(|item| {
        item.summary == "Legacy answer" && item.run_segment_id == Some(segment_id)
    }));

    store.rebuild_projections().unwrap();
    assert_eq!(timeline(&path, task_id), projected);

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn rebuild_maps_late_legacy_engine_events_to_the_completed_run_segment() {
    let root = temp_root("late-legacy-engine-completed-run");
    let path = root.join("tasks.db");
    let store = Arc::new(TaskStore::open(&path).unwrap());
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let run_id = RunId::new();
    let session_id = SessionId::new();
    let at = Utc.with_ymd_and_hms(2026, 7, 12, 1, 2, 3).unwrap();
    let adapter = TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    );

    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Late legacy output"),
    );
    transact(
        &store,
        task_id,
        1,
        supervisor_source(),
        NewTaskEvent::run_started(segment_id, at),
    );
    adapter
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                run_id,
                message_id: MessageId::new(),
                delta: DeltaChunk::Text("Before completion".into()),
                at,
            })],
        )
        .await
        .unwrap();
    transact(
        &store,
        task_id,
        3,
        supervisor_source(),
        NewTaskEvent::run_completed(segment_id, at, RunTerminalReason::Completed, false),
    );
    adapter
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                run_id,
                message_id: MessageId::new(),
                delta: DeltaChunk::Text("After completion".into()),
                at,
            })],
        )
        .await
        .unwrap();

    store.rebuild_projections().unwrap();
    let projected = timeline(&path, task_id)
        .into_iter()
        .filter(|item| item.kind == harness_contracts::TimelineEventKind::AssistantText)
        .map(|item| (item.summary, item.run_segment_id))
        .collect::<Vec<_>>();
    assert_eq!(
        projected,
        vec![
            ("Before completion".into(), Some(segment_id)),
            ("After completion".into(), Some(segment_id)),
        ]
    );

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn rebuild_does_not_map_late_legacy_engine_events_to_the_next_run_segment() {
    let root = temp_root("late-legacy-engine-next-run");
    let path = root.join("tasks.db");
    let store = Arc::new(TaskStore::open(&path).unwrap());
    let task_id = TaskId::new();
    let first_segment_id = RunSegmentId::new();
    let second_segment_id = RunSegmentId::new();
    let first_run_id = RunId::new();
    let second_run_id = RunId::new();
    let session_id = SessionId::new();
    let at = Utc.with_ymd_and_hms(2026, 7, 12, 1, 2, 3).unwrap();
    let adapter = TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    );

    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Late output after next run"),
    );
    transact(
        &store,
        task_id,
        1,
        supervisor_source(),
        NewTaskEvent::run_started(first_segment_id, at),
    );
    adapter
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                run_id: first_run_id,
                message_id: MessageId::new(),
                delta: DeltaChunk::Text("First run".into()),
                at,
            })],
        )
        .await
        .unwrap();
    transact(
        &store,
        task_id,
        3,
        supervisor_source(),
        NewTaskEvent::run_completed(first_segment_id, at, RunTerminalReason::Completed, false),
    );
    transact(
        &store,
        task_id,
        4,
        supervisor_source(),
        NewTaskEvent::run_started(second_segment_id, at),
    );
    adapter
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                run_id: second_run_id,
                message_id: MessageId::new(),
                delta: DeltaChunk::Text("Second run".into()),
                at,
            })],
        )
        .await
        .unwrap();
    adapter
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                run_id: first_run_id,
                message_id: MessageId::new(),
                delta: DeltaChunk::Text("Late first run".into()),
                at,
            })],
        )
        .await
        .unwrap();

    store.rebuild_projections().unwrap();
    let projected = timeline(&path, task_id)
        .into_iter()
        .filter(|item| item.kind == harness_contracts::TimelineEventKind::AssistantText)
        .map(|item| (item.summary, item.run_segment_id))
        .collect::<Vec<_>>();
    assert_eq!(
        projected,
        vec![
            ("First run".into(), Some(first_segment_id)),
            ("Second run".into(), Some(second_segment_id)),
            ("Late first run".into(), Some(first_segment_id)),
        ]
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn engine_assistant_projection_uses_bound_segment_for_late_and_reused_message_events() {
    let root = temp_root("engine-assistant-segment-binding");
    let path = root.join("tasks.db");
    let store = TaskStore::open(&path).unwrap();
    let task_id = TaskId::new();
    let first_segment_id = RunSegmentId::new();
    let second_segment_id = RunSegmentId::new();
    let first_run_id = RunId::new();
    let second_run_id = RunId::new();
    let session_id = SessionId::new();
    let message_id = MessageId::new();
    let at = Utc.with_ymd_and_hms(2026, 7, 12, 1, 2, 3).unwrap();

    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Segment-bound output"),
    );
    transact(
        &store,
        task_id,
        1,
        supervisor_source(),
        NewTaskEvent::run_started(first_segment_id, at),
    );
    store
        .append_engine_events(
            task_id,
            TenantId::SINGLE,
            session_id,
            Some(first_segment_id),
            AppendMetadata {
                run_id: Some(first_run_id),
                ..AppendMetadata::default()
            },
            Some(0),
            &[Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                run_id: first_run_id,
                message_id,
                delta: DeltaChunk::Text("First run".into()),
                at,
            })],
        )
        .unwrap();
    transact(
        &store,
        task_id,
        3,
        supervisor_source(),
        NewTaskEvent::run_completed(first_segment_id, at, RunTerminalReason::Completed, false),
    );
    transact(
        &store,
        task_id,
        4,
        supervisor_source(),
        NewTaskEvent::run_started(second_segment_id, at),
    );
    store
        .append_engine_events(
            task_id,
            TenantId::SINGLE,
            session_id,
            Some(second_segment_id),
            AppendMetadata {
                run_id: Some(second_run_id),
                ..AppendMetadata::default()
            },
            Some(1),
            &[Event::AssistantMessageCompleted(
                AssistantMessageCompletedEvent {
                    run_id: second_run_id,
                    message_id,
                    content: MessageContent::Text("Second run".into()),
                    tool_uses: Vec::new(),
                    usage: UsageSnapshot::default(),
                    pricing_snapshot_id: None,
                    stop_reason: StopReason::EndTurn,
                    at,
                },
            )],
        )
        .unwrap();
    store
        .append_engine_events(
            task_id,
            TenantId::SINGLE,
            session_id,
            Some(first_segment_id),
            AppendMetadata {
                run_id: Some(first_run_id),
                ..AppendMetadata::default()
            },
            Some(2),
            &[Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                run_id: first_run_id,
                message_id,
                delta: DeltaChunk::Text(" late".into()),
                at,
            })],
        )
        .unwrap();

    let projected = timeline(&path, task_id)
        .into_iter()
        .filter(|item| item.kind == harness_contracts::TimelineEventKind::AssistantText)
        .map(|item| (item.summary, item.run_segment_id, item.incomplete))
        .collect::<Vec<_>>();
    assert_eq!(
        projected,
        vec![
            ("First run".into(), Some(first_segment_id), true),
            ("Second run".into(), Some(second_segment_id), false),
            (" late".into(), Some(first_segment_id), true),
        ]
    );

    let before_rebuild = timeline(&path, task_id);
    store.rebuild_projections().unwrap();
    assert_eq!(timeline(&path, task_id), before_rebuild);
    let _ = std::fs::remove_dir_all(root);
}

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
            details: None,
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
            actor_id,
            mode: WorkspaceMode::ManagedWorktree,
            canonical_root: "/workspace".into(),
            worktree_path: Some("/workspace/.worktrees/task".into()),
            branch: Some("jyowo/task".into()),
            writable: true,
            state: WorkspaceLeaseState::Active,
            requested_at: started_at,
            acquired_at: Some(started_at),
            expires_at: None,
            baseline_commit: Some("abc".into()),
            baseline_status: String::new(),
            patch_path: None,
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
        permission_source(),
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
            NewTaskEvent::message_consumed(queue_item_id, 2, next_segment_id),
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
                details: None,
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
                actor_id,
                mode: WorkspaceMode::ManagedWorktree,
                canonical_root: "/other".into(),
                worktree_path: None,
                branch: None,
                writable: false,
                state: WorkspaceLeaseState::Active,
                requested_at: next_started_at,
                acquired_at: Some(next_started_at),
                expires_at: None,
                baseline_commit: None,
                baseline_status: String::new(),
                patch_path: None,
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
            details: None,
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
        permission_source(),
        NewTaskEvent::permission_resolved(request_id, 1),
    );
    assert!(matches!(
        store.transact_command(command(task_id, 5, supervisor_source()), |_| {
            let wrong_segment = RunSegmentId::new();
            Ok(vec![
                NewTaskEvent::run_started(wrong_segment, now),
                NewTaskEvent::message_consumed(queue_item_id, 2, wrong_segment),
            ])
        }),
        Err(TaskStoreError::Projector(_))
    ));
    assert_eq!(store.stream_version(task_id).unwrap(), 5);

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn message_consumed_rejects_user_authority() {
    let root = temp_root("consumed-authority");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let queue_item_id = QueueItemId::new();
    let now = Utc::now();
    let store = TaskStore::open(&path).unwrap();
    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Consumed authority"),
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

    assert!(matches!(
        store.transact_command(command(task_id, 3, user_source()), |_| {
            Ok(vec![NewTaskEvent::message_consumed(
                queue_item_id,
                1,
                segment_id,
            )])
        }),
        Err(TaskStoreError::InvalidInput(_))
    ));
    assert_eq!(store.stream_version(task_id).unwrap(), 3);

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn queued_message_consumption_requires_run_started_in_the_same_command() {
    let root = temp_root("queued-consumption-atomicity");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let queue_item_id = QueueItemId::new();
    let now = Utc::now();
    let store = TaskStore::open(&path).unwrap();
    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Queued consumption atomicity"),
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

    assert!(matches!(
        store.transact_command(command(task_id, 3, supervisor_source()), |_| {
            Ok(vec![NewTaskEvent::message_consumed(
                queue_item_id,
                1,
                segment_id,
            )])
        }),
        Err(TaskStoreError::InvalidInput(_))
    ));
    assert_eq!(store.stream_version(task_id).unwrap(), 3);

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn promoted_message_consumption_requires_run_started_in_the_same_command() {
    let root = temp_root("promoted-consumption-atomicity");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let active_segment_id = RunSegmentId::new();
    let queue_item_id = QueueItemId::new();
    let now = Utc::now();
    let store = TaskStore::open(&path).unwrap();
    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Promoted consumption atomicity"),
    );
    transact(
        &store,
        task_id,
        1,
        supervisor_source(),
        NewTaskEvent::run_started(active_segment_id, now),
    );
    transact(
        &store,
        task_id,
        2,
        user_source(),
        NewTaskEvent::message_queued(queue_item_id, "promoted", vec![], vec![], now),
    );
    transact(
        &store,
        task_id,
        3,
        user_source(),
        NewTaskEvent::message_promoted(queue_item_id, 1),
    );

    assert!(matches!(
        store.transact_command(command(task_id, 4, supervisor_source()), |_| {
            Ok(vec![NewTaskEvent::message_consumed(
                queue_item_id,
                1,
                active_segment_id,
            )])
        }),
        Err(TaskStoreError::InvalidInput(_))
    ));
    assert_eq!(store.stream_version(task_id).unwrap(), 4);

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn message_recovered_rejects_non_recovery_authority() {
    let root = temp_root("recovered-authority");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let queue_item_id = QueueItemId::new();
    let now = Utc::now();
    let store = TaskStore::open(&path).unwrap();
    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Recovered authority"),
    );
    transact(
        &store,
        task_id,
        1,
        user_source(),
        NewTaskEvent::message_queued(queue_item_id, "queued", vec![], vec![], now),
    );
    transact(
        &store,
        task_id,
        2,
        user_source(),
        NewTaskEvent::message_promoted(queue_item_id, 1),
    );

    assert!(matches!(
        store.transact_command(command(task_id, 3, supervisor_source()), |_| {
            Ok(vec![NewTaskEvent::message_recovered(queue_item_id, 1)])
        }),
        Err(TaskStoreError::InvalidInput(_))
    ));
    assert_eq!(store.stream_version(task_id).unwrap(), 3);

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn actor_failure_clears_pending_permission_from_every_projection() {
    let root = temp_root("actor-failure-permission");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let request_id = RequestId::new();
    let started_at = Utc::now();
    let store = TaskStore::open(&path).unwrap();
    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Actor failure"),
    );
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
        permission_source(),
        NewTaskEvent::permission_requested(PermissionProjection {
            request_id,
            revision: 1,
            route: PermissionRoute::ForegroundTask,
            details: None,
        }),
    );
    transact(
        &store,
        task_id,
        3,
        supervisor_source(),
        NewTaskEvent::task_actor_failed(Some(segment_id), Utc::now()),
    );

    assert!(store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .pending_permission
        .is_none());
    let connection = rusqlite::Connection::open(&path).unwrap();
    let permission_rows: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM permission_projection WHERE task_id = ?1",
            [task_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(permission_rows, 0);

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn actor_failure_without_a_segment_cannot_leave_an_active_run() {
    let root = temp_root("actor-failure-active-run");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let store = TaskStore::open(&path).unwrap();
    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Actor failure invariant"),
    );
    transact(
        &store,
        task_id,
        1,
        supervisor_source(),
        NewTaskEvent::run_started(segment_id, Utc::now()),
    );

    assert!(matches!(
        store.transact_command(command(task_id, 2, supervisor_source()), |_| {
            Ok(vec![NewTaskEvent::task_actor_failed(None, Utc::now())])
        }),
        Err(TaskStoreError::Projector(_))
    ));
    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(projection.state, TaskState::Running);
    assert_eq!(projection.current_run.unwrap().state, RunState::Running);

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
    transact_events(
        &store,
        task_id,
        2,
        supervisor_source(),
        vec![
            NewTaskEvent::run_started(segment_id, now),
            NewTaskEvent::message_consumed(queue_item_id, 1, segment_id),
        ],
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
fn task_metadata_projects_and_rebuilds() {
    let root = temp_root("task-metadata");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let store = TaskStore::open(&path).unwrap();

    transact(
        &store,
        task_id,
        0,
        user_source(),
        NewTaskEvent::task_created("Original"),
    );
    transact_events(
        &store,
        task_id,
        1,
        user_source(),
        vec![
            NewTaskEvent::task_pinned(true),
            NewTaskEvent::task_title_changed("Renamed"),
            NewTaskEvent::task_archived(true),
            NewTaskEvent::task_removed(true),
        ],
    );

    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(projection.title, "Renamed");
    assert!(projection.pinned);
    assert!(projection.archived);
    assert!(projection.removed);

    store.rebuild_projections().unwrap();
    let rebuilt = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(rebuilt.title, "Renamed");
    assert!(rebuilt.pinned);
    assert!(rebuilt.archived);
    assert!(rebuilt.removed);

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn task_metadata_requires_an_existing_task() {
    let root = temp_root("task-metadata-create-first");
    let path = root.join("tasks.db");
    let store = TaskStore::open(&path).unwrap();

    for event in [
        NewTaskEvent::task_pinned(true),
        NewTaskEvent::task_removed(true),
    ] {
        let task_id = TaskId::new();
        assert!(matches!(
            store.append(task_id, 0, &user_source(), vec![event]),
            Err(TaskStoreError::Projector(_))
        ));
        assert!(store.task_projection(task_id).unwrap().is_none());
    }

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

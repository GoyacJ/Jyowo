//! Synchronous projections derived from the canonical task event log.

use chrono::{DateTime, Utc};
use harness_contracts::{
    ActorId, ChildAttachment, DeltaChunk, DenyReason, Event, MessageContent, MessagePart,
    PromotionMode, QueueItemProjection, QueueItemState, RunProjection, RunState, RunTerminalReason,
    SubagentProjection, TaskEventEnvelope, TaskId, TaskProjection, TaskState,
    TimelineArtifactPresentation, TimelineArtifactProjection, TimelineArtifactSurface,
    TimelineContentBlock, TimelineEventKind, TimelineItemProjection, TimelineNoticeLevel,
    TimelineTextFormat, TimelineToolOperation, TimelineToolProjection, TimelineToolStatus,
    ToolResult, ToolResultPart,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::task_event::{EngineEventPayload, TaskEvent};
use crate::TaskStoreError;

const MAX_TIMELINE_SUMMARY_CHARS: usize = 4096;
const LEGACY_CHILD_ATTACHMENT_MIGRATION: &str = "legacy_child_attachment_projection_v1";
pub const MAX_ACTIVE_QUEUE_ITEMS: usize = 64;

struct ReducedTask {
    projection: TaskProjection,
    terminal_queue_item: Option<QueueItemProjection>,
    recovered_queue_items: Vec<QueueItemProjection>,
}

pub trait TaskProjector: Send + Sync {
    fn apply(
        &self,
        transaction: &Transaction<'_>,
        event: &TaskEventEnvelope,
    ) -> Result<(), TaskStoreError>;
}

#[derive(Debug, Default)]
pub struct SynchronousTaskProjector;

pub(crate) fn migrate_legacy_child_attachment_projections(
    connection: &mut Connection,
) -> Result<(), TaskStoreError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let already_applied = transaction
        .query_row(
            "SELECT 1 FROM task_store_migrations WHERE migration_name = ?1",
            [LEGACY_CHILD_ATTACHMENT_MIGRATION],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if already_applied {
        transaction.commit()?;
        return Ok(());
    }

    let legacy_rows = {
        let mut statement = transaction.prepare(
            "SELECT projection.task_id,
                    projection.last_global_offset,
                    projection.projection_json,
                    EXISTS(
                        SELECT 1
                        FROM event_log AS event
                        WHERE event.task_id = projection.task_id
                          AND event.event_type = 'subagent.backgrounded'
                          AND json_extract(event.payload_json, '$.detached') = 1
                    )
             FROM task_projection AS projection
             WHERE json_type(projection.projection_json, '$.parent') = 'object'
               AND json_type(projection.projection_json, '$.parent.attachment') IS NULL",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    for (task_id, last_global_offset, original, detached) in legacy_rows {
        let mut projection: serde_json::Value = serde_json::from_str(&original)?;
        let parent = projection
            .get_mut("parent")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| {
                TaskStoreError::ProjectionIntegrity(format!(
                    "task {task_id} legacy child projection has no parent object"
                ))
            })?;
        parent.insert(
            "attachment".into(),
            serde_json::Value::String(if detached { "detached" } else { "attached" }.into()),
        );
        let projection = serde_json::to_string(&projection)?;
        let offset =
            u64::try_from(last_global_offset).map_err(|_| TaskStoreError::IntegerOutOfRange)?;
        let projection_digest = task_projection_digest(offset, &projection);
        let updated = transaction.execute(
            "UPDATE task_projection
             SET projection_json = ?4, projection_digest = ?5
             WHERE task_id = ?1 AND last_global_offset = ?2 AND projection_json = ?3",
            params![
                task_id,
                last_global_offset,
                original,
                projection,
                projection_digest
            ],
        )?;
        if updated != 1 {
            return Err(TaskStoreError::ProjectionIntegrity(
                "legacy child attachment projection changed during migration".into(),
            ));
        }
    }

    transaction.execute(
        "INSERT INTO task_store_migrations (migration_name, applied) VALUES (?1, 1)",
        [LEGACY_CHILD_ATTACHMENT_MIGRATION],
    )?;
    transaction.commit()?;
    Ok(())
}

impl TaskProjector for SynchronousTaskProjector {
    fn apply(
        &self,
        transaction: &Transaction<'_>,
        envelope: &TaskEventEnvelope,
    ) -> Result<(), TaskStoreError> {
        let event = TaskEvent::decode(
            &envelope.event_type,
            envelope.schema_version,
            envelope.payload.clone(),
        )?;
        event.validate_source(&envelope.source)?;
        let reduced = reduce_task(transaction, envelope, &event)?;
        persist_task(transaction, envelope, &reduced.projection)?;
        project_entity_tables(transaction, envelope, &event, &reduced)?;
        project_timeline(transaction, envelope, &event, &reduced)?;
        Ok(())
    }
}

fn reduce_task(
    transaction: &Transaction<'_>,
    envelope: &TaskEventEnvelope,
    event: &TaskEvent,
) -> Result<ReducedTask, TaskStoreError> {
    let existing = load_task_projection(transaction, envelope.task_id)?;
    let mut projection = if let Some(projection) = existing {
        if projection.task_id != envelope.task_id {
            return integrity("task projection contains another task id");
        }
        let expected = projection
            .stream_version
            .checked_add(1)
            .ok_or(TaskStoreError::IntegerOutOfRange)?;
        if envelope.stream_sequence != expected {
            return integrity(format!(
                "task {} projection expected stream sequence {expected}, got {}",
                envelope.task_id, envelope.stream_sequence
            ));
        }
        if envelope.global_offset <= projection.last_global_offset {
            return integrity("task projection offsets are not strictly increasing");
        }
        projection
    } else {
        if envelope.stream_sequence != 1 {
            return integrity(format!(
                "task {} has no projection at stream sequence {}",
                envelope.task_id, envelope.stream_sequence
            ));
        }
        if !matches!(event, TaskEvent::TaskCreated { .. }) {
            return invalid_transition("task stream must start with task.created");
        }
        empty_task_projection(envelope.task_id)
    };

    let mut terminal_queue_item = None;
    let mut recovered_queue_items = Vec::new();
    match event {
        TaskEvent::TaskCreated { title, workspace } => {
            if projection.stream_version != 0 || !projection.title.is_empty() {
                return invalid_transition("task.created requires a new task stream");
            }
            projection.title.clone_from(title);
            projection.workspace.clone_from(workspace);
            projection.state = TaskState::Idle;
            projection.actor_id = Some(ActorId::from_u128(u128::from_be_bytes(
                envelope.task_id.as_bytes(),
            )));
        }
        TaskEvent::TaskTitleChanged { title } => {
            if projection.stream_version == 0 {
                return invalid_transition("task.title_changed requires an existing task");
            }
            projection.title.clone_from(title);
        }
        TaskEvent::TaskPinned { pinned } => projection.pinned = *pinned,
        TaskEvent::TaskArchived { archived } => projection.archived = *archived,
        TaskEvent::TaskRemoved { removed } => projection.removed = *removed,
        TaskEvent::TaskActorFailed {
            segment_id,
            failed_at,
        } => {
            if let Some(segment_id) = segment_id {
                let run = projection.current_run.as_mut().ok_or_else(|| {
                    projector_error("task.actor_failed references a missing active run")
                })?;
                if run.segment_id != *segment_id
                    || !matches!(
                        run.state,
                        RunState::Running
                            | RunState::WaitingPermission
                            | RunState::WaitingInput
                            | RunState::Yielding
                    )
                {
                    return invalid_transition("task.actor_failed does not match the active run");
                }
                if *failed_at < run.started_at {
                    return invalid_transition("task.actor_failed precedes run.started");
                }
                let was_yielding = run.state == RunState::Yielding;
                run.state = RunState::Failed;
                run.terminal_reason = Some(RunTerminalReason::Failed);
                run.ended_at = Some(*failed_at);
                run.incomplete_output = true;
                if was_yielding {
                    for item in projection
                        .queue
                        .iter_mut()
                        .filter(|item| item.state == QueueItemState::Promoting)
                    {
                        item.state = QueueItemState::Queued;
                        recovered_queue_items.push(item.clone());
                    }
                }
            } else if projection.current_run.as_ref().is_some_and(|run| {
                matches!(
                    run.state,
                    RunState::Running
                        | RunState::WaitingPermission
                        | RunState::WaitingInput
                        | RunState::Yielding
                )
            }) {
                return invalid_transition("task.actor_failed requires the active run segment id");
            }
            projection.pending_permission = None;
            projection.pending_question = None;
            projection.state = TaskState::Failed;
        }
        TaskEvent::RunStarted {
            segment_id,
            started_at,
            ..
        } => {
            if projection.pending_permission.is_some() || projection.pending_question.is_some() {
                return invalid_transition("run.started requires no pending interaction");
            }
            if projection.current_run.as_ref().is_some_and(|run| {
                matches!(
                    run.state,
                    RunState::Running
                        | RunState::WaitingPermission
                        | RunState::WaitingInput
                        | RunState::Yielding
                )
            }) {
                return invalid_transition("run.started requires no active run");
            }
            projection.current_run = Some(RunProjection {
                segment_id: *segment_id,
                state: RunState::Running,
                promotion_mode: None,
                terminal_reason: None,
                started_at: *started_at,
                ended_at: None,
                incomplete_output: false,
            });
            projection.state = TaskState::Running;
        }
        TaskEvent::RunCompleted {
            segment_id,
            ended_at,
            terminal_reason,
            incomplete_output,
        } => {
            if projection.pending_permission.is_some() {
                return invalid_transition(
                    "run.completed requires permission resolution or invalidation",
                );
            }
            if projection.pending_question.is_some() {
                return invalid_transition(
                    "run.completed requires question resolution or invalidation",
                );
            }
            let run = projection
                .current_run
                .as_mut()
                .ok_or_else(|| projector_error("run.completed requires an active run"))?;
            if run.segment_id != *segment_id
                || !matches!(run.state, RunState::Running | RunState::Yielding)
            {
                return invalid_transition("run.completed does not match the active run");
            }
            if *ended_at < run.started_at {
                return invalid_transition("run.completed precedes run.started");
            }
            run.state = match terminal_reason {
                RunTerminalReason::Completed => RunState::Completed,
                RunTerminalReason::Failed => RunState::Failed,
                RunTerminalReason::Superseded
                | RunTerminalReason::ForcedInterruption
                | RunTerminalReason::InterruptedByRestart
                | RunTerminalReason::Cancelled => RunState::Interrupted,
            };
            run.terminal_reason = Some(terminal_reason.clone());
            run.ended_at = Some(*ended_at);
            run.incomplete_output = *incomplete_output;
            projection.state = match run.state {
                RunState::Completed => TaskState::Completed,
                RunState::Failed => TaskState::Failed,
                RunState::Interrupted => TaskState::Interrupted,
                _ => return integrity("terminal event produced a non-terminal run"),
            };
        }
        TaskEvent::RunYieldRequested {
            segment_id, force, ..
        } => {
            let run = projection
                .current_run
                .as_mut()
                .ok_or_else(|| projector_error("run.yield_requested requires an active run"))?;
            if run.segment_id != *segment_id || run.state != RunState::Running {
                return invalid_transition(
                    "run.yield_requested must match the current running segment",
                );
            }
            run.state = RunState::Yielding;
            run.promotion_mode = Some(if *force {
                PromotionMode::ForceStop
            } else {
                PromotionMode::SafePoint
            });
            projection.state = TaskState::Running;
        }
        TaskEvent::RunSafePointReached {
            segment_id, forced, ..
        } => {
            let run = projection
                .current_run
                .as_ref()
                .ok_or_else(|| projector_error("run.safe_point_reached requires an active run"))?;
            if run.segment_id != *segment_id || run.state != RunState::Yielding {
                return invalid_transition(
                    "run.safe_point_reached must match the current yielding segment",
                );
            }
            let expected_mode = if *forced {
                PromotionMode::ForceStop
            } else {
                PromotionMode::SafePoint
            };
            if run.promotion_mode.as_ref() != Some(&expected_mode) {
                return invalid_transition(
                    "run.safe_point_reached does not match the requested promotion mode",
                );
            }
        }
        TaskEvent::RunForceStopTimedOut { segment_id, .. } => {
            let run = projection.current_run.as_ref().ok_or_else(|| {
                projector_error("run.force_stop_timed_out requires an active run")
            })?;
            if run.segment_id != *segment_id
                || run.state != RunState::Yielding
                || run.promotion_mode.as_ref() != Some(&PromotionMode::ForceStop)
            {
                return invalid_transition(
                    "run.force_stop_timed_out must match a force-stopping segment",
                );
            }
        }
        TaskEvent::ToolIndeterminate { .. } => {}
        TaskEvent::MessageQueued {
            queue_item_id,
            content,
            attachments,
            context_references,
            created_at,
            ..
        } => {
            if projection.queue.len() >= MAX_ACTIVE_QUEUE_ITEMS {
                return invalid_transition(format!(
                    "a task may contain at most {MAX_ACTIVE_QUEUE_ITEMS} active queue items"
                ));
            }
            if projection
                .queue
                .iter()
                .any(|item| item.queue_item_id == *queue_item_id)
            {
                return invalid_transition("message.queued requires a new queue item id");
            }
            projection.queue.push(QueueItemProjection {
                queue_item_id: *queue_item_id,
                state: QueueItemState::Queued,
                revision: 1,
                content: content.clone(),
                attachments: attachments.clone(),
                context_references: context_references.clone(),
                created_at: *created_at,
                created_global_offset: envelope.global_offset,
                consumed_by: None,
            });
        }
        TaskEvent::MessageEdited {
            queue_item_id,
            revision,
            content,
            attachments,
            context_references,
        } => {
            let item = queue_item_mut(&mut projection, *queue_item_id)?;
            require_queue_revision(item, *revision, QueueItemState::Queued, "message.edited")?;
            item.revision = *revision;
            item.content.clone_from(content);
            item.attachments.clone_from(attachments);
            item.context_references.clone_from(context_references);
        }
        TaskEvent::MessagePromoted {
            queue_item_id,
            revision,
        } => {
            let item = queue_item_mut(&mut projection, *queue_item_id)?;
            require_exact_queue_revision(
                item,
                *revision,
                QueueItemState::Queued,
                "message.promoted",
            )?;
            item.state = QueueItemState::Promoting;
        }
        TaskEvent::MessageRecovered {
            queue_item_id,
            revision,
        } => {
            let item = queue_item_mut(&mut projection, *queue_item_id)?;
            require_exact_queue_revision(
                item,
                *revision,
                QueueItemState::Promoting,
                "message.recovered",
            )?;
            item.state = QueueItemState::Queued;
        }
        TaskEvent::MessageDeleted {
            queue_item_id,
            revision,
        } => {
            let index = queue_item_index(&projection, *queue_item_id)?;
            let mut item = projection.queue.remove(index);
            require_exact_queue_revision(
                &item,
                *revision,
                QueueItemState::Queued,
                "message.deleted",
            )?;
            item.state = QueueItemState::Deleted;
            terminal_queue_item = Some(item);
        }
        TaskEvent::MessageConsumed {
            queue_item_id,
            revision,
            run_segment_id,
        } => {
            if !projection.current_run.as_ref().is_some_and(|run| {
                run.segment_id == *run_segment_id && run.state == RunState::Running
            }) {
                return invalid_transition("message.consumed requires its matching active run");
            }
            let index = queue_item_index(&projection, *queue_item_id)?;
            let mut item = projection.queue.remove(index);
            if !matches!(
                item.state,
                QueueItemState::Queued | QueueItemState::Promoting
            ) || item.revision != *revision
            {
                return invalid_transition(
                    "message.consumed requires a queued or promoting item at its current revision",
                );
            }
            item.state = QueueItemState::Consumed;
            item.consumed_by = Some(*run_segment_id);
            terminal_queue_item = Some(item);
        }
        TaskEvent::PermissionRequested { permission } => {
            if projection.pending_permission.is_some() {
                return invalid_transition("permission.requested requires no pending request");
            }
            projection.pending_permission = Some(permission.clone());
            if let Some(run) = projection.current_run.as_mut() {
                if run.state != RunState::Running {
                    return invalid_transition("permission.requested requires a running run");
                }
                run.state = RunState::WaitingPermission;
            }
            projection.state = TaskState::WaitingPermission;
        }
        TaskEvent::PermissionResolved {
            request_id,
            revision,
            ..
        }
        | TaskEvent::PermissionInvalidated {
            request_id,
            revision,
            ..
        } => {
            let pending = projection
                .pending_permission
                .as_ref()
                .ok_or_else(|| projector_error("permission.resolved requires a pending request"))?;
            if pending.request_id != *request_id || pending.revision != *revision {
                return invalid_transition("permission.resolved does not match pending request");
            }
            projection.pending_permission = None;
            projection.state = if let Some(run) = projection.current_run.as_mut() {
                if run.state != RunState::WaitingPermission {
                    return invalid_transition("pending permission run is not waiting");
                }
                run.state = RunState::Running;
                TaskState::Running
            } else {
                TaskState::Idle
            };
        }
        TaskEvent::QuestionRequested { question } => {
            if projection.pending_question.is_some() {
                return invalid_transition("question.requested requires no pending question");
            }
            if projection.pending_permission.is_some() {
                return invalid_transition("question.requested conflicts with pending permission");
            }
            let run = projection
                .current_run
                .as_mut()
                .ok_or_else(|| projector_error("question.requested requires an active run"))?;
            if run.segment_id != question.segment_id || run.state != RunState::Running {
                return invalid_transition(
                    "question.requested requires the current running segment",
                );
            }
            projection.pending_question = Some(question.clone());
            run.state = RunState::WaitingInput;
            projection.state = TaskState::WaitingInput;
        }
        TaskEvent::QuestionResolved {
            request_id,
            revision,
            ..
        }
        | TaskEvent::QuestionInvalidated {
            request_id,
            revision,
            ..
        } => {
            let pending = projection
                .pending_question
                .as_ref()
                .ok_or_else(|| projector_error("question resolution requires a pending request"))?;
            if pending.request_id != *request_id || pending.revision != *revision {
                return invalid_transition("question resolution does not match pending request");
            }
            projection.pending_question = None;
            projection.state = if let Some(run) = projection.current_run.as_mut() {
                if run.state != RunState::WaitingInput {
                    return invalid_transition("pending question run is not waiting for input");
                }
                run.state = RunState::Running;
                TaskState::Running
            } else {
                TaskState::Idle
            };
        }
        TaskEvent::SubagentSpawned { child, .. } => {
            if let Some(child) = child {
                if child.parent_task_id != envelope.task_id {
                    return integrity("subagent.spawned child belongs to another parent");
                }
                if projection
                    .subagents
                    .iter()
                    .any(|existing| existing.child_task_id == child.child_task_id)
                {
                    return invalid_transition("subagent.spawned child is already linked");
                }
                if child.state != harness_contracts::SubagentActorState::Starting
                    || child.detached
                    || child.summary.is_some()
                    || child.ended_at.is_some()
                    || child.workspace_lease_id.is_some()
                {
                    return invalid_transition("subagent.spawned requires a pristine child actor");
                }
                if projection.actor_id == Some(child.actor_id)
                    || projection
                        .current_run
                        .as_ref()
                        .is_some_and(|run| run.segment_id == child.segment_id)
                {
                    return invalid_transition(
                        "subagent child actor and segment identities must be independent",
                    );
                }
                projection.subagents.push(child.clone());
            }
        }
        TaskEvent::SubagentLinked {
            actor_id,
            context_cursor,
            parent,
        } => {
            if projection.parent.is_some() {
                return invalid_transition("subagent.linked cannot relink a child task");
            }
            if parent.parent_task_id == envelope.task_id {
                return invalid_transition("subagent.linked cannot make a task its own parent");
            }
            projection.actor_id = Some(*actor_id);
            projection.context_cursor = *context_cursor;
            projection.parent = Some(parent.clone());
        }
        TaskEvent::SubagentStateChanged { child }
        | TaskEvent::SubagentSummaryUpdated { child }
        | TaskEvent::SubagentBackgrounded { child }
        | TaskEvent::SubagentTerminal { child } => {
            apply_subagent_update(&mut projection, envelope.task_id, child)?;
        }
        TaskEvent::Engine { .. } => {}
        TaskEvent::WorkspacePreparing { lease }
        | TaskEvent::WorkspaceAcquired { lease }
        | TaskEvent::WorkspaceCleanupPending { lease } => {
            if lease.task_id != envelope.task_id {
                return integrity("workspace lease belongs to another task");
            }
        }
        TaskEvent::WorkspaceWaiting { lease } => {
            if lease.task_id != envelope.task_id {
                return integrity("waiting workspace lease belongs to another task");
            }
        }
        TaskEvent::WorkspaceReleased { .. } => {}
        TaskEvent::WorkspaceCleanupBlocked { .. } => {}
        TaskEvent::WorkspaceOverrideApplied { .. } => {}
    }

    projection.stream_version = envelope.stream_sequence;
    projection.last_global_offset = envelope.global_offset;
    Ok(ReducedTask {
        projection,
        terminal_queue_item,
        recovered_queue_items,
    })
}

fn queue_item_index(
    projection: &TaskProjection,
    queue_item_id: harness_contracts::QueueItemId,
) -> Result<usize, TaskStoreError> {
    projection
        .queue
        .iter()
        .position(|item| item.queue_item_id == queue_item_id)
        .ok_or_else(|| projector_error("queue event references an unknown item"))
}

fn queue_item_mut(
    projection: &mut TaskProjection,
    queue_item_id: harness_contracts::QueueItemId,
) -> Result<&mut QueueItemProjection, TaskStoreError> {
    projection
        .queue
        .iter_mut()
        .find(|item| item.queue_item_id == queue_item_id)
        .ok_or_else(|| projector_error("queue event references an unknown item"))
}

fn require_queue_revision(
    item: &QueueItemProjection,
    revision: u64,
    required_state: QueueItemState,
    event_type: &str,
) -> Result<(), TaskStoreError> {
    let expected_revision = item
        .revision
        .checked_add(1)
        .ok_or(TaskStoreError::IntegerOutOfRange)?;
    if item.state != required_state || revision != expected_revision {
        return invalid_transition(format!(
            "{event_type} requires state {required_state:?} revision {expected_revision}"
        ));
    }
    Ok(())
}

fn require_exact_queue_revision(
    item: &QueueItemProjection,
    revision: u64,
    required_state: QueueItemState,
    event_type: &str,
) -> Result<(), TaskStoreError> {
    if item.state != required_state || revision != item.revision {
        return invalid_transition(format!(
            "{event_type} requires state {required_state:?} revision {}",
            item.revision
        ));
    }
    Ok(())
}

fn persist_task(
    transaction: &Transaction<'_>,
    envelope: &TaskEventEnvelope,
    projection: &TaskProjection,
) -> Result<(), TaskStoreError> {
    let projection_json = serde_json::to_string(projection)?;
    let projection_digest = task_projection_digest(envelope.global_offset, &projection_json);
    transaction.execute(
        "INSERT INTO task_projection (
            task_id, last_global_offset, projection_json, projection_digest
         ) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(task_id) DO UPDATE SET
            last_global_offset = excluded.last_global_offset,
            projection_json = excluded.projection_json,
            projection_digest = excluded.projection_digest",
        params![
            envelope.task_id.to_string(),
            sqlite_integer(envelope.global_offset)?,
            projection_json,
            projection_digest,
        ],
    )?;
    Ok(())
}

fn project_entity_tables(
    transaction: &Transaction<'_>,
    envelope: &TaskEventEnvelope,
    event: &TaskEvent,
    reduced: &ReducedTask,
) -> Result<(), TaskStoreError> {
    match event {
        TaskEvent::RunStarted { segment_id, .. }
        | TaskEvent::RunCompleted { segment_id, .. }
        | TaskEvent::RunYieldRequested { segment_id, .. }
        | TaskEvent::RunSafePointReached { segment_id, .. }
        | TaskEvent::RunForceStopTimedOut { segment_id, .. } => {
            let run = reduced
                .projection
                .current_run
                .as_ref()
                .filter(|run| run.segment_id == *segment_id)
                .ok_or_else(|| projector_error("run projection is missing after reduction"))?;
            upsert_entity(
                transaction,
                "run_projection",
                "run_segment_id",
                envelope,
                &segment_id.to_string(),
                run,
            )?;
        }
        TaskEvent::TaskActorFailed { segment_id, .. } => {
            transaction.execute(
                "DELETE FROM permission_projection WHERE task_id = ?1",
                [envelope.task_id.to_string()],
            )?;
            if let Some(segment_id) = segment_id {
                let run = reduced
                    .projection
                    .current_run
                    .as_ref()
                    .filter(|run| run.segment_id == *segment_id)
                    .ok_or_else(|| projector_error("failed run projection is missing"))?;
                upsert_entity(
                    transaction,
                    "run_projection",
                    "run_segment_id",
                    envelope,
                    &segment_id.to_string(),
                    run,
                )?;
            }
            for item in &reduced.recovered_queue_items {
                upsert_entity(
                    transaction,
                    "queue_projection",
                    "queue_item_id",
                    envelope,
                    &item.queue_item_id.to_string(),
                    item,
                )?;
            }
        }
        TaskEvent::MessageQueued { queue_item_id, .. }
        | TaskEvent::MessageEdited { queue_item_id, .. }
        | TaskEvent::MessagePromoted { queue_item_id, .. }
        | TaskEvent::MessageRecovered { queue_item_id, .. } => {
            let item = reduced
                .projection
                .queue
                .iter()
                .find(|item| item.queue_item_id == *queue_item_id)
                .ok_or_else(|| projector_error("queue projection is missing after reduction"))?;
            upsert_entity(
                transaction,
                "queue_projection",
                "queue_item_id",
                envelope,
                &queue_item_id.to_string(),
                item,
            )?;
        }
        TaskEvent::MessageDeleted { queue_item_id, .. }
        | TaskEvent::MessageConsumed { queue_item_id, .. } => {
            let item = reduced
                .terminal_queue_item
                .as_ref()
                .filter(|item| item.queue_item_id == *queue_item_id)
                .ok_or_else(|| projector_error("consumed queue item is missing after reduction"))?;
            upsert_entity(
                transaction,
                "queue_projection",
                "queue_item_id",
                envelope,
                &queue_item_id.to_string(),
                item,
            )?;
        }
        TaskEvent::PermissionRequested { permission } => {
            upsert_entity(
                transaction,
                "permission_projection",
                "permission_request_id",
                envelope,
                &permission.request_id.to_string(),
                permission,
            )?;
        }
        TaskEvent::PermissionResolved { request_id, .. }
        | TaskEvent::PermissionInvalidated { request_id, .. } => {
            transaction.execute(
                "DELETE FROM permission_projection
                 WHERE task_id = ?1 AND permission_request_id = ?2",
                params![envelope.task_id.to_string(), request_id.to_string()],
            )?;
        }
        TaskEvent::QuestionRequested { .. }
        | TaskEvent::QuestionResolved { .. }
        | TaskEvent::QuestionInvalidated { .. } => {}
        TaskEvent::SubagentSpawned {
            actor_id,
            started_at,
            child,
        } => {
            if let Some(child) = child {
                upsert_entity(
                    transaction,
                    "subagent_projection",
                    "actor_id",
                    envelope,
                    &actor_id.to_string(),
                    child,
                )?;
            } else {
                let legacy = LegacySubagentProjection {
                    actor_id: *actor_id,
                    state: LegacySubagentState::Running,
                    started_at: *started_at,
                    ended_at: None,
                };
                upsert_entity(
                    transaction,
                    "subagent_projection",
                    "actor_id",
                    envelope,
                    &actor_id.to_string(),
                    &legacy,
                )?;
            }
        }
        TaskEvent::SubagentStateChanged { child }
        | TaskEvent::SubagentSummaryUpdated { child }
        | TaskEvent::SubagentBackgrounded { child }
        | TaskEvent::SubagentTerminal { child } => {
            upsert_entity(
                transaction,
                "subagent_projection",
                "actor_id",
                envelope,
                &child.actor_id.to_string(),
                child,
            )?;
        }
        TaskEvent::SubagentLinked { .. } => {}
        TaskEvent::WorkspacePreparing { lease }
        | TaskEvent::WorkspaceAcquired { lease }
        | TaskEvent::WorkspaceCleanupPending { lease } => {
            upsert_entity(
                transaction,
                "workspace_projection",
                "workspace_lease_id",
                envelope,
                &lease.lease_id.to_string(),
                lease,
            )?;
        }
        TaskEvent::WorkspaceWaiting { lease }
        | TaskEvent::WorkspaceReleased { lease, .. }
        | TaskEvent::WorkspaceCleanupBlocked { lease, .. } => {
            upsert_entity(
                transaction,
                "workspace_projection",
                "workspace_lease_id",
                envelope,
                &lease.lease_id.to_string(),
                lease,
            )?;
        }
        TaskEvent::WorkspaceOverrideApplied { .. } => {}
        TaskEvent::TaskCreated { .. }
        | TaskEvent::TaskTitleChanged { .. }
        | TaskEvent::TaskPinned { .. }
        | TaskEvent::TaskArchived { .. }
        | TaskEvent::TaskRemoved { .. }
        | TaskEvent::ToolIndeterminate { .. }
        | TaskEvent::Engine { .. } => {}
    }
    Ok(())
}

fn upsert_entity<T: Serialize>(
    transaction: &Transaction<'_>,
    table: &str,
    id_column: &str,
    envelope: &TaskEventEnvelope,
    id: &str,
    projection: &T,
) -> Result<(), TaskStoreError> {
    let sql = format!(
        "INSERT INTO {table} (task_id, {id_column}, last_global_offset, projection_json)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(task_id, {id_column}) DO UPDATE SET
            last_global_offset = excluded.last_global_offset,
            projection_json = excluded.projection_json"
    );
    transaction.execute(
        &sql,
        params![
            envelope.task_id.to_string(),
            id,
            sqlite_integer(envelope.global_offset)?,
            serde_json::to_string(projection)?,
        ],
    )?;
    Ok(())
}

fn project_timeline(
    transaction: &Transaction<'_>,
    envelope: &TaskEventEnvelope,
    event: &TaskEvent,
    reduced: &ReducedTask,
) -> Result<(), TaskStoreError> {
    if let TaskEvent::Engine { payload, .. } = event {
        let active_segment_id = reduced.projection.current_run.as_ref().and_then(|run| {
            matches!(
                run.state,
                RunState::Running
                    | RunState::WaitingPermission
                    | RunState::WaitingInput
                    | RunState::Yielding
            )
            .then_some(run.segment_id)
        });
        let legacy_segment_id = if payload.run_segment_id.is_none() {
            legacy_engine_run_segment_id(transaction, envelope, payload, active_segment_id)?
        } else {
            None
        };
        return project_engine_timeline(transaction, envelope, payload, legacy_segment_id);
    }
    if matches!(
        event,
        TaskEvent::MessageQueued { .. }
            | TaskEvent::MessageEdited { .. }
            | TaskEvent::MessagePromoted { .. }
            | TaskEvent::MessageDeleted { .. }
            | TaskEvent::MessageRecovered { .. }
    ) {
        return Ok(());
    }
    let (kind, summary, run_segment_id, incomplete) = match event {
        TaskEvent::TaskCreated { .. } => (
            TimelineEventKind::Notice,
            "Task created".into(),
            None,
            false,
        ),
        TaskEvent::TaskTitleChanged { .. } => (
            TimelineEventKind::Notice,
            "Task title changed".into(),
            None,
            false,
        ),
        TaskEvent::TaskPinned { pinned } => (
            TimelineEventKind::Notice,
            if *pinned {
                "Task pinned"
            } else {
                "Task unpinned"
            }
            .into(),
            None,
            false,
        ),
        TaskEvent::TaskArchived { archived } => (
            TimelineEventKind::Notice,
            if *archived {
                "Task archived"
            } else {
                "Task restored"
            }
            .into(),
            None,
            false,
        ),
        TaskEvent::TaskRemoved { removed } => (
            TimelineEventKind::Notice,
            if *removed {
                "Task removed"
            } else {
                "Task restored"
            }
            .into(),
            None,
            false,
        ),
        TaskEvent::TaskActorFailed { segment_id, .. } => (
            TimelineEventKind::Error,
            "Task actor failed".into(),
            *segment_id,
            true,
        ),
        TaskEvent::RunStarted { segment_id, .. } => (
            TimelineEventKind::Notice,
            "Run started".into(),
            Some(*segment_id),
            false,
        ),
        TaskEvent::RunCompleted {
            segment_id,
            terminal_reason,
            incomplete_output,
            ..
        } => (
            TimelineEventKind::Notice,
            run_terminal_summary(terminal_reason).into(),
            Some(*segment_id),
            *incomplete_output,
        ),
        TaskEvent::RunYieldRequested {
            segment_id, force, ..
        } => (
            TimelineEventKind::Notice,
            if *force {
                "Run force-stop requested"
            } else {
                "Run yield requested"
            }
            .into(),
            Some(*segment_id),
            false,
        ),
        TaskEvent::RunSafePointReached {
            segment_id,
            forced,
            incomplete_output,
            ..
        } => (
            TimelineEventKind::Notice,
            if *forced {
                "Run force-stopped"
            } else {
                "Run safe point reached"
            }
            .into(),
            Some(*segment_id),
            *incomplete_output,
        ),
        TaskEvent::RunForceStopTimedOut { segment_id, .. } => (
            TimelineEventKind::Notice,
            "Run force-stop timed out".into(),
            Some(*segment_id),
            true,
        ),
        TaskEvent::ToolIndeterminate { run_segment_id, .. } => (
            TimelineEventKind::ToolActivity,
            "Tool outcome is indeterminate after restart".into(),
            Some(*run_segment_id),
            true,
        ),
        TaskEvent::MessageQueued { .. }
        | TaskEvent::MessageEdited { .. }
        | TaskEvent::MessagePromoted { .. }
        | TaskEvent::MessageDeleted { .. }
        | TaskEvent::MessageRecovered { .. } => return Ok(()),
        TaskEvent::MessageConsumed { run_segment_id, .. } => (
            TimelineEventKind::UserMessage,
            reduced
                .terminal_queue_item
                .as_ref()
                .map(|item| bounded_summary(&item.content))
                .ok_or_else(|| projector_error("consumed queue timeline item is missing"))?,
            Some(*run_segment_id),
            false,
        ),
        TaskEvent::PermissionRequested { .. } => (
            TimelineEventKind::Permission,
            "Permission requested".into(),
            None,
            false,
        ),
        TaskEvent::PermissionResolved { .. } => (
            TimelineEventKind::Permission,
            "Permission resolved".into(),
            None,
            false,
        ),
        TaskEvent::PermissionInvalidated { .. } => (
            TimelineEventKind::Permission,
            "Permission expired after restart".into(),
            None,
            false,
        ),
        TaskEvent::QuestionRequested { question } => (
            TimelineEventKind::Notice,
            question.questions.first().map_or_else(
                || "User input requested".into(),
                |item| item.question.clone(),
            ),
            Some(question.segment_id),
            false,
        ),
        TaskEvent::QuestionResolved { .. } => (
            TimelineEventKind::Notice,
            "User input received".into(),
            reduced
                .projection
                .current_run
                .as_ref()
                .map(|run| run.segment_id),
            false,
        ),
        TaskEvent::QuestionInvalidated { .. } => (
            TimelineEventKind::Notice,
            "User input request expired".into(),
            reduced
                .projection
                .current_run
                .as_ref()
                .map(|run| run.segment_id),
            false,
        ),
        TaskEvent::SubagentSpawned { .. } => (
            TimelineEventKind::Subagent,
            "Subagent started".into(),
            None,
            false,
        ),
        TaskEvent::SubagentLinked { .. } => (
            TimelineEventKind::Subagent,
            "Subagent linked".into(),
            None,
            false,
        ),
        TaskEvent::SubagentStateChanged { child } => (
            TimelineEventKind::Subagent,
            format!("Subagent {:?}", child.state),
            Some(child.segment_id),
            false,
        ),
        TaskEvent::SubagentSummaryUpdated { child } => (
            TimelineEventKind::Subagent,
            child
                .summary
                .as_deref()
                .map(bounded_summary)
                .unwrap_or_else(|| "Subagent summary updated".into()),
            Some(child.segment_id),
            false,
        ),
        TaskEvent::SubagentBackgrounded { child } => (
            TimelineEventKind::Subagent,
            "Subagent continuing in background".into(),
            Some(child.segment_id),
            false,
        ),
        TaskEvent::SubagentTerminal { child } => (
            TimelineEventKind::Subagent,
            child
                .summary
                .as_deref()
                .map(bounded_summary)
                .unwrap_or_else(|| format!("Subagent {:?}", child.state)),
            Some(child.segment_id),
            child.state == harness_contracts::SubagentActorState::Failed,
        ),
        TaskEvent::WorkspaceAcquired { .. } => (
            TimelineEventKind::Notice,
            "Workspace acquired".into(),
            None,
            false,
        ),
        TaskEvent::WorkspacePreparing { .. } => (
            TimelineEventKind::Notice,
            "Workspace preparing".into(),
            None,
            false,
        ),
        TaskEvent::WorkspaceWaiting { .. } => (
            TimelineEventKind::Notice,
            "Workspace lease waiting".into(),
            None,
            false,
        ),
        TaskEvent::WorkspaceReleased { .. } => (
            TimelineEventKind::Notice,
            "Workspace released".into(),
            None,
            false,
        ),
        TaskEvent::WorkspaceCleanupBlocked { .. } => (
            TimelineEventKind::Notice,
            "Workspace cleanup blocked".into(),
            None,
            false,
        ),
        TaskEvent::WorkspaceCleanupPending { .. } => (
            TimelineEventKind::Notice,
            "Workspace cleanup pending".into(),
            None,
            false,
        ),
        TaskEvent::WorkspaceOverrideApplied { .. } => (
            TimelineEventKind::Notice,
            "Workspace write override applied".into(),
            None,
            false,
        ),
        TaskEvent::Engine { .. } => unreachable!("engine events are projected above"),
    };
    let attachments = reduced
        .terminal_queue_item
        .as_ref()
        .map(|item| item.attachments.as_slice())
        .unwrap_or_default();
    let blob_id = attachments.first().copied();
    let content_blocks = if kind == TimelineEventKind::UserMessage {
        user_message_content_blocks(&summary, attachments)
    } else {
        default_content_blocks(kind.clone(), &summary, blob_id, None)
    };
    let timeline = TimelineItemProjection {
        id: envelope.event_id.to_string(),
        kind,
        global_offset: envelope.global_offset,
        run_segment_id,
        semantic_group_id: None,
        summary,
        blob_id,
        incomplete,
        tool: None,
        content_blocks,
    };
    insert_timeline_item(transaction, envelope, &timeline)
}

fn legacy_engine_run_segment_id(
    transaction: &Transaction<'_>,
    envelope: &TaskEventEnvelope,
    payload: &EngineEventPayload,
    active_segment_id: Option<harness_contracts::RunSegmentId>,
) -> Result<Option<harness_contracts::RunSegmentId>, TaskStoreError> {
    let run_id = match payload.run_id {
        Some(run_id) => Some(run_id.to_string()),
        None => transaction
            .query_row(
                "SELECT json_extract(payload_json, '$.event.run_id')
                 FROM event_log
                 WHERE task_id = ?1 AND global_offset = ?2",
                params![
                    envelope.task_id.to_string(),
                    sqlite_integer(envelope.global_offset)?,
                ],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten(),
    };
    let Some(run_id) = run_id else {
        return Ok(active_segment_id);
    };
    let first_engine_offset = transaction.query_row(
        "SELECT MIN(global_offset)
         FROM event_log
         WHERE task_id = ?1
           AND event_type GLOB 'engine.*'
           AND COALESCE(
                 json_extract(payload_json, '$.runId'),
                 json_extract(payload_json, '$.event.run_id')
               ) = ?2
           AND global_offset <= ?3",
        params![
            envelope.task_id.to_string(),
            run_id,
            sqlite_integer(envelope.global_offset)?,
        ],
        |row| row.get::<_, Option<i64>>(0),
    )?;
    let Some(first_engine_offset) = first_engine_offset else {
        return Ok(active_segment_id);
    };
    transaction
        .query_row(
            "SELECT json_extract(payload_json, '$.segmentId')
             FROM event_log
             WHERE task_id = ?1
               AND event_type = 'run.started'
               AND global_offset < ?2
             ORDER BY global_offset DESC
             LIMIT 1",
            params![envelope.task_id.to_string(), first_engine_offset],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|segment_id| {
            harness_contracts::RunSegmentId::parse(&segment_id).map_err(TaskStoreError::from)
        })
        .transpose()
        .map(|segment_id| segment_id.or(active_segment_id))
}

fn project_engine_timeline(
    transaction: &Transaction<'_>,
    envelope: &TaskEventEnvelope,
    payload: &EngineEventPayload,
    legacy_segment_id: Option<harness_contracts::RunSegmentId>,
) -> Result<(), TaskStoreError> {
    // Pre-binding journals did not persist runSegmentId. Their runId is resolved
    // against the run.started that preceded that run's first engine envelope.
    let Some(run_segment_id) = payload.run_segment_id.or(legacy_segment_id) else {
        return Ok(());
    };
    let run_segment_id = Some(run_segment_id);
    let item = match &payload.event {
        Event::AssistantDeltaProduced(delta) => {
            let DeltaChunk::Text(text) = &delta.delta else {
                return Ok(());
            };
            if text.is_empty() {
                return Ok(());
            }
            TimelineItemProjection {
                id: envelope.event_id.to_string(),
                kind: TimelineEventKind::AssistantText,
                global_offset: envelope.global_offset,
                run_segment_id,
                semantic_group_id: Some(delta.message_id.to_string()),
                summary: text.clone(),
                blob_id: None,
                incomplete: true,
                tool: None,
                content_blocks: vec![TimelineContentBlock::Text {
                    format: TimelineTextFormat::Markdown,
                    text: text.clone(),
                }],
            }
        }
        Event::AssistantMessageCompleted(completed) => {
            let semantic_group_id = completed.message_id.to_string();
            let content_blocks = message_content_blocks(&completed.content);
            if content_blocks.is_empty() {
                return Ok(());
            }
            if complete_assistant_group(
                transaction,
                envelope.task_id,
                run_segment_id,
                &semantic_group_id,
                &content_blocks,
            )? {
                return Ok(());
            }
            let summary = content_blocks_summary(&content_blocks);
            let blob_id = first_artifact_blob_id(&content_blocks);
            TimelineItemProjection {
                id: envelope.event_id.to_string(),
                kind: TimelineEventKind::AssistantText,
                global_offset: envelope.global_offset,
                run_segment_id,
                semantic_group_id: Some(semantic_group_id),
                summary,
                blob_id,
                incomplete: false,
                tool: None,
                content_blocks,
            }
        }
        Event::AssistantNotice(notice) => engine_timeline_item(
            envelope,
            TimelineEventKind::Notice,
            run_segment_id,
            notice.body.as_str().to_owned(),
            None,
            false,
        ),
        Event::AssistantReviewRequested(review) => engine_timeline_item(
            envelope,
            TimelineEventKind::Notice,
            run_segment_id,
            review.title.as_str().to_owned(),
            None,
            false,
        ),
        Event::AssistantClarificationRequested(clarification) => engine_timeline_item(
            envelope,
            TimelineEventKind::Notice,
            run_segment_id,
            clarification.prompt.as_str().to_owned(),
            None,
            false,
        ),
        Event::ArtifactCreated(artifact) => {
            let title = artifact.title.clone();
            let descriptor = timeline_artifact_projection(
                Some(artifact.artifact_id.clone()),
                title.clone(),
                Some(artifact.kind.clone()),
                artifact.blob_ref.as_ref(),
                artifact.preview.clone(),
                artifact.source_tool_use_id.map(|id| id.to_string()),
            );
            artifact_timeline_item(
                envelope,
                artifact_timeline_kind(&artifact.kind),
                run_segment_id,
                title,
                descriptor,
            )
        }
        Event::ArtifactUpdated(artifact) => {
            let title = artifact
                .title
                .clone()
                .unwrap_or_else(|| "Artifact updated".into());
            let descriptor = timeline_artifact_projection(
                Some(artifact.artifact_id.clone()),
                title.clone(),
                artifact.kind.clone(),
                artifact.blob_ref.as_ref(),
                artifact.preview.clone(),
                artifact.source_tool_use_id.map(|id| id.to_string()),
            );
            artifact_timeline_item(
                envelope,
                artifact
                    .kind
                    .as_deref()
                    .map(artifact_timeline_kind)
                    .unwrap_or(TimelineEventKind::Artifact),
                run_segment_id,
                title,
                descriptor,
            )
        }
        Event::ToolUseRequested(tool) => {
            let operation = timeline_tool_operation(&tool.tool_name);
            let projection = TimelineToolProjection {
                tool_use_id: tool.tool_use_id.to_string(),
                tool_name: tool.tool_name.clone(),
                operation,
                status: TimelineToolStatus::Requested,
                command: timeline_tool_command(operation, &tool.input),
                subject: timeline_tool_subject(operation, &tool.input),
                output: None,
                result_summary: None,
                duration_ms: None,
            };
            return insert_timeline_item(
                transaction,
                envelope,
                &tool_timeline_item(envelope, run_segment_id, projection),
            );
        }
        Event::ToolUseStarted(started) => {
            return update_tool_timeline(
                transaction,
                envelope,
                run_segment_id,
                started.tool_use_id.to_string(),
                TimelineToolStatus::Running,
                None,
                None,
                None,
                false,
            );
        }
        Event::ToolUseDenied(denied) => {
            let reason = denied_tool_result_message(&denied.reason);
            return update_tool_timeline(
                transaction,
                envelope,
                run_segment_id,
                denied.tool_use_id.to_string(),
                TimelineToolStatus::Denied,
                Some(bounded_tool_text(&reason)),
                Some(bounded_tool_preview(&reason)),
                None,
                false,
            );
        }
        Event::ToolUseCompleted(completed) => {
            let command_failed = tool_result_command_failed(&completed.result);
            return update_tool_timeline(
                transaction,
                envelope,
                run_segment_id,
                completed.tool_use_id.to_string(),
                TimelineToolStatus::Completed,
                Some(tool_result_summary(&completed.result)),
                tool_result_command_output(&completed.result),
                Some(completed.duration_ms),
                command_failed,
            );
        }
        Event::ToolUseFailed(failed) => {
            return update_tool_timeline(
                transaction,
                envelope,
                run_segment_id,
                failed.tool_use_id.to_string(),
                TimelineToolStatus::Failed,
                Some(bounded_tool_text(&failed.error.message)),
                Some(bounded_tool_preview(&failed.error.message)),
                None,
                false,
            );
        }
        Event::CompactionApplied(_) => engine_timeline_item(
            envelope,
            TimelineEventKind::Compaction,
            run_segment_id,
            "Context compacted".into(),
            None,
            false,
        ),
        Event::UnexpectedError(error) => engine_timeline_item(
            envelope,
            TimelineEventKind::Error,
            run_segment_id,
            bounded_summary(&error.error),
            None,
            true,
        ),
        _ => return Ok(()),
    };
    insert_timeline_item(transaction, envelope, &item)
}

fn engine_timeline_item(
    envelope: &TaskEventEnvelope,
    kind: TimelineEventKind,
    run_segment_id: Option<harness_contracts::RunSegmentId>,
    summary: String,
    blob_id: Option<harness_contracts::BlobId>,
    incomplete: bool,
) -> TimelineItemProjection {
    let content_blocks = default_content_blocks(kind.clone(), &summary, blob_id, None);
    TimelineItemProjection {
        id: envelope.event_id.to_string(),
        kind,
        global_offset: envelope.global_offset,
        run_segment_id,
        semantic_group_id: None,
        summary,
        blob_id,
        incomplete,
        tool: None,
        content_blocks,
    }
}

fn artifact_timeline_item(
    envelope: &TaskEventEnvelope,
    kind: TimelineEventKind,
    run_segment_id: Option<harness_contracts::RunSegmentId>,
    summary: String,
    artifact: TimelineArtifactProjection,
) -> TimelineItemProjection {
    let blob_id = artifact.blob_id;
    TimelineItemProjection {
        id: envelope.event_id.to_string(),
        kind,
        global_offset: envelope.global_offset,
        run_segment_id,
        semantic_group_id: None,
        summary,
        blob_id,
        incomplete: false,
        tool: None,
        content_blocks: vec![TimelineContentBlock::Artifact { artifact }],
    }
}

fn tool_timeline_item(
    envelope: &TaskEventEnvelope,
    run_segment_id: Option<harness_contracts::RunSegmentId>,
    tool: TimelineToolProjection,
) -> TimelineItemProjection {
    let content_blocks = vec![TimelineContentBlock::ToolActivity {
        activity: tool.clone(),
    }];
    TimelineItemProjection {
        id: envelope.event_id.to_string(),
        kind: TimelineEventKind::ToolActivity,
        global_offset: envelope.global_offset,
        run_segment_id,
        semantic_group_id: Some(tool.tool_use_id.clone()),
        summary: tool_timeline_summary(&tool),
        blob_id: None,
        incomplete: matches!(
            tool.status,
            TimelineToolStatus::Requested | TimelineToolStatus::Running
        ),
        tool: Some(tool),
        content_blocks,
    }
}

fn update_tool_timeline(
    transaction: &Transaction<'_>,
    envelope: &TaskEventEnvelope,
    run_segment_id: Option<harness_contracts::RunSegmentId>,
    tool_use_id: String,
    status: TimelineToolStatus,
    result_summary: Option<String>,
    output: Option<String>,
    duration_ms: Option<u64>,
    command_failed: bool,
) -> Result<(), TaskStoreError> {
    let mut item = {
        let mut statement = transaction.prepare(
            "SELECT projection_json FROM timeline_projection
             WHERE task_id = ?1 ORDER BY global_offset DESC",
        )?;
        let rows = statement.query_map([envelope.task_id.to_string()], |row| {
            row.get::<_, String>(0)
        })?;
        let mut found = None;
        for row in rows {
            let candidate: TimelineItemProjection = serde_json::from_str(&row?)?;
            if candidate
                .tool
                .as_ref()
                .map(|tool| tool.tool_use_id.as_str())
                == Some(tool_use_id.as_str())
            {
                found = Some(candidate);
                break;
            }
        }
        found
    };

    let Some(item) = item.as_mut() else {
        let fallback = TimelineToolProjection {
            tool_use_id,
            tool_name: "tool".into(),
            operation: TimelineToolOperation::Other,
            status,
            command: None,
            subject: None,
            output: None,
            result_summary,
            duration_ms,
        };
        return insert_timeline_item(
            transaction,
            envelope,
            &tool_timeline_item(envelope, run_segment_id, fallback),
        );
    };

    let Some(tool) = item.tool.as_mut() else {
        return integrity("tool timeline item lost its tool projection");
    };
    let status = if command_failed && tool.operation == TimelineToolOperation::Command {
        TimelineToolStatus::Failed
    } else {
        status
    };
    tool.status = status;
    if result_summary.is_some() {
        tool.result_summary = result_summary;
    }
    if tool.operation == TimelineToolOperation::Command && output.is_some() {
        tool.output = output;
    }
    if duration_ms.is_some() {
        tool.duration_ms = duration_ms;
    }
    item.summary = tool_timeline_summary(tool);
    item.incomplete = matches!(
        status,
        TimelineToolStatus::Requested | TimelineToolStatus::Running
    );
    item.content_blocks = vec![TimelineContentBlock::ToolActivity {
        activity: tool.clone(),
    }];
    transaction.execute(
        "UPDATE timeline_projection SET projection_json = ?3
         WHERE task_id = ?1 AND global_offset = ?2",
        params![
            envelope.task_id.to_string(),
            sqlite_integer(item.global_offset)?,
            serde_json::to_string(item)?,
        ],
    )?;
    Ok(())
}

fn timeline_tool_operation(tool_name: &str) -> TimelineToolOperation {
    let name = tool_name.to_ascii_lowercase();
    if name.contains("edit") || name.contains("write") || name.contains("patch") {
        TimelineToolOperation::Edit
    } else if name.contains("read") || name.contains("load") {
        TimelineToolOperation::Read
    } else if name.contains("search")
        || name.contains("find")
        || name.contains("grep")
        || name == "rg"
        || name.contains("glob")
    {
        TimelineToolOperation::Search
    } else if name.contains("exec")
        || name.contains("command")
        || name.contains("shell")
        || name.contains("terminal")
        || name == "bash"
    {
        TimelineToolOperation::Command
    } else if name.contains("browser") || name.contains("fetch") || name.contains("web") {
        TimelineToolOperation::Browse
    } else if name.contains("image") || name.contains("generate") {
        TimelineToolOperation::Generate
    } else if name.contains("agent") || name.contains("delegate") || name.contains("spawn") {
        TimelineToolOperation::Delegate
    } else {
        TimelineToolOperation::Other
    }
}

fn timeline_tool_subject(
    operation: TimelineToolOperation,
    input: &serde_json::Value,
) -> Option<String> {
    let object = input.as_object()?;
    let keys = match operation {
        TimelineToolOperation::Read | TimelineToolOperation::Edit => {
            ["path", "file_path", "filePath", "filename"]
        }
        TimelineToolOperation::Generate | TimelineToolOperation::Delegate => {
            ["name", "target", "output_path", "outputPath"]
        }
        _ => return None,
    };
    keys.into_iter()
        .find_map(|key| object.get(key).and_then(serde_json::Value::as_str))
        .map(safe_tool_subject)
}

fn timeline_tool_command(
    operation: TimelineToolOperation,
    input: &serde_json::Value,
) -> Option<String> {
    if operation != TimelineToolOperation::Command {
        return None;
    }
    let object = input.as_object()?;
    ["cmd", "command", "script"]
        .into_iter()
        .find_map(|key| object.get(key).and_then(serde_json::Value::as_str))
        .map(bounded_tool_preview)
}

fn safe_tool_subject(value: &str) -> String {
    let normalized = value.replace('\\', "/");
    let parts = normalized
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let start = parts.len().saturating_sub(2);
    bounded_tool_text(&parts[start..].join("/"))
}

fn tool_timeline_summary(tool: &TimelineToolProjection) -> String {
    let action = match (tool.status, tool.operation) {
        (TimelineToolStatus::Completed, TimelineToolOperation::Read) => "Read",
        (TimelineToolStatus::Completed, TimelineToolOperation::Edit) => "Edited",
        (TimelineToolStatus::Completed, TimelineToolOperation::Search) => "Searched",
        (TimelineToolStatus::Completed, TimelineToolOperation::Command) => "Ran command",
        (TimelineToolStatus::Completed, TimelineToolOperation::Browse) => "Browsed",
        (TimelineToolStatus::Completed, TimelineToolOperation::Generate) => "Generated",
        (TimelineToolStatus::Completed, TimelineToolOperation::Delegate) => "Delegated",
        (TimelineToolStatus::Completed, TimelineToolOperation::Other) => "Used tool",
        (TimelineToolStatus::Denied, _) => "Tool denied",
        (TimelineToolStatus::Failed, _) => "Tool failed",
        (_, TimelineToolOperation::Read) => "Reading",
        (_, TimelineToolOperation::Edit) => "Editing",
        (_, TimelineToolOperation::Search) => "Searching",
        (_, TimelineToolOperation::Command) => "Running command",
        (_, TimelineToolOperation::Browse) => "Browsing",
        (_, TimelineToolOperation::Generate) => "Generating",
        (_, TimelineToolOperation::Delegate) => "Delegating",
        (_, TimelineToolOperation::Other) => "Using tool",
    };
    match tool.subject.as_deref() {
        Some(subject) => format!("{action} {subject}"),
        None if tool.operation == TimelineToolOperation::Other => {
            format!("{action} {}", tool.tool_name)
        }
        None => action.into(),
    }
}

fn tool_result_summary(result: &ToolResult) -> String {
    match result {
        ToolResult::Text(text) => format!("{} lines returned", text.lines().count().max(1)),
        ToolResult::Structured(_) => "Structured result".into(),
        ToolResult::Blob { content_type, .. } => content_type.clone(),
        ToolResult::Mixed(parts) => format!("{} result parts", parts.len()),
        _ => "Result received".into(),
    }
}

fn tool_result_command_failed(result: &ToolResult) -> bool {
    match result {
        ToolResult::Structured(value) => structured_result_failed(value),
        ToolResult::Mixed(parts) => parts.iter().any(|part| {
            matches!(
                part,
                ToolResultPart::Structured { value, .. } if structured_result_failed(value)
            )
        }),
        _ => false,
    }
}

fn structured_result_failed(value: &serde_json::Value) -> bool {
    value.get("success").and_then(serde_json::Value::as_bool) == Some(false)
}

fn denied_tool_result_message(reason: &DenyReason) -> String {
    match reason {
        DenyReason::UserDenied => "tool use denied by user".to_owned(),
        DenyReason::RuleDenied => "tool use denied by rule".to_owned(),
        DenyReason::DefaultModeDenied => "tool use denied by permission mode".to_owned(),
        DenyReason::HookBlocked { handler_id } => {
            format!("tool use blocked by hook `{handler_id}`")
        }
        DenyReason::SubagentBlocked => "tool use denied for subagent".to_owned(),
        DenyReason::PolicyDenied => "tool use denied by runtime policy".to_owned(),
        DenyReason::Other(message) => format!("tool use denied: {message}"),
        _ => "tool use denied".to_owned(),
    }
}

fn tool_result_command_output(result: &ToolResult) -> Option<String> {
    let preview = match result {
        ToolResult::Text(text) => Some(text.clone()),
        ToolResult::Mixed(parts) => {
            let text = parts
                .iter()
                .filter_map(tool_result_part_command_output)
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
        ToolResult::Blob { .. } => None,
        _ => None,
    }?;
    Some(bounded_tool_preview(&preview))
}

fn tool_result_part_command_output(part: &ToolResultPart) -> Option<String> {
    match part {
        ToolResultPart::Text { text } | ToolResultPart::Code { text, .. } => Some(text.clone()),
        ToolResultPart::Error { message, .. } => Some(message.clone()),
        _ => None,
    }
}

fn bounded_tool_text(value: &str) -> String {
    value.chars().take(160).collect()
}

fn bounded_tool_preview(value: &str) -> String {
    const MAX_TOOL_PREVIEW_CHARS: usize = 8_000;
    let mut preview = value
        .chars()
        .take(MAX_TOOL_PREVIEW_CHARS + 1)
        .collect::<String>();
    if preview.chars().count() > MAX_TOOL_PREVIEW_CHARS {
        preview = preview.chars().take(MAX_TOOL_PREVIEW_CHARS).collect();
        preview.push('…');
    }
    preview
}

fn artifact_timeline_kind(kind: &str) -> TimelineEventKind {
    match kind.to_ascii_lowercase().as_str() {
        "image" | "screenshot" => TimelineEventKind::Image,
        "command" | "terminal" => TimelineEventKind::Command,
        "diff" | "patch" => TimelineEventKind::Diff,
        "file" => TimelineEventKind::File,
        _ => TimelineEventKind::Artifact,
    }
}

fn timeline_artifact_projection(
    artifact_id: Option<String>,
    title: String,
    artifact_kind: Option<String>,
    blob_ref: Option<&harness_contracts::BlobRef>,
    preview: Option<String>,
    source_tool_use_id: Option<String>,
) -> TimelineArtifactProjection {
    let media_type = blob_ref
        .and_then(|blob| blob.content_type.clone())
        .unwrap_or_else(|| "application/octet-stream".into());
    let preferred_surface = preferred_artifact_surface(artifact_kind.as_deref(), &media_type);
    TimelineArtifactProjection {
        artifact_id,
        blob_id: blob_ref.map(|blob| blob.id),
        title,
        artifact_kind,
        media_type,
        size: blob_ref.map(|blob| blob.size),
        format: None,
        preview,
        presentation: Some(TimelineArtifactPresentation {
            preferred_surface: Some(preferred_surface),
            preview_blob_id: None,
        }),
        source_tool_use_id,
    }
}

fn preferred_artifact_surface(
    artifact_kind: Option<&str>,
    media_type: &str,
) -> TimelineArtifactSurface {
    let kind = artifact_kind.unwrap_or_default().to_ascii_lowercase();
    let media_type = media_type.to_ascii_lowercase();
    if matches!(
        kind.as_str(),
        "audio" | "image" | "map" | "screenshot" | "video"
    ) || media_type.starts_with("audio/")
        || media_type.starts_with("image/")
        || media_type.starts_with("video/")
        || media_type.contains("geo+json")
        || media_type.contains("geojson")
    {
        TimelineArtifactSurface::Inline
    } else {
        TimelineArtifactSurface::Card
    }
}

fn default_content_blocks(
    kind: TimelineEventKind,
    summary: &str,
    blob_id: Option<harness_contracts::BlobId>,
    tool: Option<&TimelineToolProjection>,
) -> Vec<TimelineContentBlock> {
    if let Some(tool) = tool {
        return vec![TimelineContentBlock::ToolActivity {
            activity: tool.clone(),
        }];
    }
    match kind {
        TimelineEventKind::AssistantText => vec![TimelineContentBlock::Text {
            format: TimelineTextFormat::Markdown,
            text: summary.into(),
        }],
        TimelineEventKind::UserMessage => vec![TimelineContentBlock::Text {
            format: TimelineTextFormat::Plain,
            text: summary.into(),
        }],
        TimelineEventKind::Artifact
        | TimelineEventKind::Command
        | TimelineEventKind::Diff
        | TimelineEventKind::File
        | TimelineEventKind::Image
            if blob_id.is_some() =>
        {
            let artifact_kind = match kind {
                TimelineEventKind::Artifact => "artifact",
                TimelineEventKind::Command => "command",
                TimelineEventKind::Diff => "diff",
                TimelineEventKind::File => "file",
                TimelineEventKind::Image => "image",
                _ => unreachable!(),
            };
            vec![TimelineContentBlock::Artifact {
                artifact: TimelineArtifactProjection {
                    artifact_id: None,
                    blob_id,
                    title: summary.into(),
                    artifact_kind: Some(artifact_kind.into()),
                    media_type: "application/octet-stream".into(),
                    size: None,
                    format: None,
                    preview: None,
                    presentation: Some(TimelineArtifactPresentation {
                        preferred_surface: Some(if kind == TimelineEventKind::Image {
                            TimelineArtifactSurface::Inline
                        } else {
                            TimelineArtifactSurface::Card
                        }),
                        preview_blob_id: None,
                    }),
                    source_tool_use_id: None,
                },
            }]
        }
        _ => vec![TimelineContentBlock::Notice {
            level: match kind {
                TimelineEventKind::Error => TimelineNoticeLevel::Error,
                TimelineEventKind::Permission => TimelineNoticeLevel::Warning,
                _ => TimelineNoticeLevel::Info,
            },
            text: summary.into(),
        }],
    }
}

fn user_message_content_blocks(
    summary: &str,
    attachments: &[harness_contracts::BlobId],
) -> Vec<TimelineContentBlock> {
    let mut blocks = vec![TimelineContentBlock::Text {
        format: TimelineTextFormat::Plain,
        text: summary.into(),
    }];
    for (index, blob_id) in attachments.iter().enumerate() {
        blocks.push(TimelineContentBlock::Artifact {
            artifact: TimelineArtifactProjection {
                artifact_id: None,
                blob_id: Some(*blob_id),
                title: if attachments.len() == 1 {
                    summary.into()
                } else {
                    format!("Attachment {}", index + 1)
                },
                artifact_kind: Some("file".into()),
                media_type: "application/octet-stream".into(),
                size: None,
                format: None,
                preview: None,
                presentation: Some(TimelineArtifactPresentation {
                    preferred_surface: Some(TimelineArtifactSurface::Card),
                    preview_blob_id: None,
                }),
                source_tool_use_id: None,
            },
        });
    }
    blocks
}

fn message_content_blocks(content: &MessageContent) -> Vec<TimelineContentBlock> {
    match content {
        MessageContent::Text(text) => (!text.is_empty())
            .then(|| TimelineContentBlock::Text {
                format: TimelineTextFormat::Markdown,
                text: text.clone(),
            })
            .into_iter()
            .collect(),
        MessageContent::Structured(value) => vec![TimelineContentBlock::Text {
            format: TimelineTextFormat::Plain,
            text: value.to_string(),
        }],
        MessageContent::Multimodal(parts) => parts
            .iter()
            .filter_map(|part| match part {
                MessagePart::Text(text) if !text.is_empty() => Some(TimelineContentBlock::Text {
                    format: TimelineTextFormat::Markdown,
                    text: text.clone(),
                }),
                MessagePart::Image {
                    mime_type,
                    blob_ref,
                } => Some(message_artifact_block(
                    "Image", "image", mime_type, blob_ref,
                )),
                MessagePart::Video {
                    mime_type,
                    blob_ref,
                } => Some(message_artifact_block(
                    "Video", "video", mime_type, blob_ref,
                )),
                MessagePart::File {
                    mime_type,
                    blob_ref,
                } => Some(message_artifact_block("File", "file", mime_type, blob_ref)),
                _ => None,
            })
            .collect(),
    }
}

fn message_artifact_block(
    title: &str,
    artifact_kind: &str,
    mime_type: &str,
    blob_ref: &harness_contracts::BlobRef,
) -> TimelineContentBlock {
    let media_type = if mime_type.trim().is_empty() {
        blob_ref
            .content_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".into())
    } else {
        mime_type.to_owned()
    };
    TimelineContentBlock::Artifact {
        artifact: TimelineArtifactProjection {
            artifact_id: None,
            blob_id: Some(blob_ref.id),
            title: title.into(),
            artifact_kind: Some(artifact_kind.into()),
            media_type: media_type.clone(),
            size: Some(blob_ref.size),
            format: None,
            preview: None,
            presentation: Some(TimelineArtifactPresentation {
                preferred_surface: Some(preferred_artifact_surface(
                    Some(artifact_kind),
                    &media_type,
                )),
                preview_blob_id: None,
            }),
            source_tool_use_id: None,
        },
    }
}

fn content_blocks_summary(blocks: &[TimelineContentBlock]) -> String {
    let text = blocks
        .iter()
        .filter_map(|block| match block {
            TimelineContentBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    if !text.is_empty() {
        return text;
    }
    blocks
        .iter()
        .find_map(|block| match block {
            TimelineContentBlock::Artifact { artifact } => Some(artifact.title.clone()),
            TimelineContentBlock::Notice { text, .. } => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn first_artifact_blob_id(blocks: &[TimelineContentBlock]) -> Option<harness_contracts::BlobId> {
    blocks.iter().find_map(|block| match block {
        TimelineContentBlock::Artifact { artifact } => artifact.blob_id,
        _ => None,
    })
}

fn complete_assistant_group(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    run_segment_id: Option<harness_contracts::RunSegmentId>,
    semantic_group_id: &str,
    completed_blocks: &[TimelineContentBlock],
) -> Result<bool, TaskStoreError> {
    let mut completed = {
        let mut statement = transaction.prepare(
            "SELECT projection_json FROM timeline_projection
             WHERE task_id = ?1 ORDER BY global_offset DESC",
        )?;
        let rows = statement.query_map([task_id.to_string()], |row| row.get::<_, String>(0))?;
        let mut found = None;
        for row in rows {
            let item: TimelineItemProjection = serde_json::from_str(&row?)?;
            if item.kind == TimelineEventKind::AssistantText
                && item.run_segment_id == run_segment_id
                && item.semantic_group_id.as_deref() == Some(semantic_group_id)
            {
                found = Some(item);
                break;
            }
        }
        found
    };
    let Some(item) = completed.as_mut() else {
        return Ok(false);
    };
    item.incomplete = false;
    item.summary = content_blocks_summary(completed_blocks);
    item.blob_id = first_artifact_blob_id(completed_blocks);
    item.content_blocks = completed_blocks.to_vec();
    transaction.execute(
        "UPDATE timeline_projection SET projection_json = ?3
         WHERE task_id = ?1 AND global_offset = ?2",
        params![
            task_id.to_string(),
            sqlite_integer(item.global_offset)?,
            serde_json::to_string(item)?,
        ],
    )?;
    Ok(true)
}

fn insert_timeline_item(
    transaction: &Transaction<'_>,
    envelope: &TaskEventEnvelope,
    timeline: &TimelineItemProjection,
) -> Result<(), TaskStoreError> {
    transaction.execute(
        "INSERT INTO timeline_projection (task_id, global_offset, projection_json)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(task_id, global_offset) DO UPDATE SET projection_json = excluded.projection_json",
        params![
            envelope.task_id.to_string(),
            sqlite_integer(envelope.global_offset)?,
            serde_json::to_string(&timeline)?,
        ],
    )?;
    Ok(())
}

const fn run_terminal_summary(reason: &RunTerminalReason) -> &'static str {
    match reason {
        RunTerminalReason::Completed => "Run completed",
        RunTerminalReason::Failed => "Run failed",
        RunTerminalReason::Superseded => "Run superseded",
        RunTerminalReason::ForcedInterruption => "Run force-stopped",
        RunTerminalReason::InterruptedByRestart => "Run interrupted by restart",
        RunTerminalReason::Cancelled => "Run cancelled",
    }
}

fn bounded_summary(summary: &str) -> String {
    summary.chars().take(MAX_TIMELINE_SUMMARY_CHARS).collect()
}

fn apply_subagent_update(
    projection: &mut TaskProjection,
    task_id: TaskId,
    child: &SubagentProjection,
) -> Result<(), TaskStoreError> {
    if task_id == child.parent_task_id {
        let existing = projection
            .subagents
            .iter_mut()
            .find(|existing| existing.child_task_id == child.child_task_id)
            .ok_or_else(|| projector_error("subagent update references an unlinked child"))?;
        if existing.actor_id != child.actor_id
            || existing.segment_id != child.segment_id
            || existing.parent_segment_id != child.parent_segment_id
            || existing.delegation_id != child.delegation_id
            || existing.context_cursor > child.context_cursor
            || existing.workspace_lease_id.is_some()
                && existing.workspace_lease_id != child.workspace_lease_id
            || existing.detached && !child.detached
            || existing.ended_at.is_some() && existing.ended_at != child.ended_at
            || existing.summary.is_some() && child.summary.is_none()
            || !valid_subagent_state_transition(existing.state, child.state)
        {
            return invalid_transition("subagent update violates immutable or monotonic state");
        }
        *existing = child.clone();
        return Ok(());
    }
    if task_id == child.child_task_id {
        projection.actor_id = Some(child.actor_id);
        projection.context_cursor = child.context_cursor;
        if let Some(parent) = projection.parent.as_mut() {
            parent.attachment = if child.detached {
                ChildAttachment::Detached
            } else {
                ChildAttachment::Attached
            };
        }
        return Ok(());
    }
    integrity("subagent update belongs to another task")
}

fn valid_subagent_state_transition(
    current: harness_contracts::SubagentActorState,
    next: harness_contracts::SubagentActorState,
) -> bool {
    use harness_contracts::SubagentActorState::{
        Background, Cancelled, Completed, Failed, Running, Starting, Yielding,
    };
    current == next
        || matches!(
            (current, next),
            (Starting, Running | Yielding | Cancelled | Failed)
                | (
                    Running,
                    Yielding | Background | Completed | Cancelled | Failed
                )
                | (Yielding, Background | Completed | Cancelled | Failed)
                | (Background, Completed | Cancelled | Failed)
        )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacySubagentState {
    Running,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacySubagentProjection {
    actor_id: ActorId,
    state: LegacySubagentState,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
}

pub(crate) fn load_task_projection(
    transaction: &Transaction<'_>,
    task_id: TaskId,
) -> Result<Option<TaskProjection>, TaskStoreError> {
    Ok(load_task_projection_row(transaction, task_id)?.map(|(_, projection)| projection))
}

pub(crate) fn load_task_projection_row(
    transaction: &Transaction<'_>,
    task_id: TaskId,
) -> Result<Option<(u64, TaskProjection)>, TaskStoreError> {
    let row = transaction
        .query_row(
            "SELECT last_global_offset, projection_json, projection_digest
             FROM task_projection WHERE task_id = ?1",
            [task_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    row.map(|(offset, body, stored_digest)| {
        let offset = u64::try_from(offset).map_err(|_| TaskStoreError::IntegerOutOfRange)?;
        let projection: TaskProjection = serde_json::from_str(&body)?;
        if projection.task_id != task_id
            || projection.last_global_offset != offset
            || stored_digest != task_projection_digest(offset, &body)
        {
            return Err(TaskStoreError::ProjectionIntegrity(format!(
                "task {task_id} projection row failed its digest or identity check"
            )));
        }
        Ok((offset, projection))
    })
    .transpose()
}

fn task_projection_digest(last_global_offset: u64, projection_json: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&last_global_offset.to_be_bytes());
    hasher.update(projection_json.as_bytes());
    hasher.finalize().to_hex().to_string()
}

pub(crate) fn empty_task_projection(task_id: TaskId) -> TaskProjection {
    TaskProjection {
        task_id,
        title: String::new(),
        state: TaskState::Idle,
        pinned: false,
        archived: false,
        removed: false,
        stream_version: 0,
        last_global_offset: 0,
        current_run: None,
        pending_permission: None,
        pending_question: None,
        queue: Vec::new(),
        workspace: None,
        actor_id: None,
        context_cursor: 0,
        parent: None,
        subagents: Vec::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionCounts {
    pub tasks: u64,
    pub runs: u64,
    pub queue_items: u64,
    pub permissions: u64,
    pub subagents: u64,
    pub workspaces: u64,
    pub timeline_items: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectionSnapshot {
    counts: ProjectionCounts,
    digest: blake3::Hash,
}

pub(crate) const PROJECTION_TABLES: [&str; 7] = [
    "task_projection",
    "run_projection",
    "queue_projection",
    "permission_projection",
    "subagent_projection",
    "workspace_projection",
    "timeline_projection",
];

pub(crate) fn projection_counts(
    transaction: &Transaction<'_>,
) -> Result<ProjectionCounts, TaskStoreError> {
    Ok(ProjectionCounts {
        tasks: table_count(transaction, "task_projection")?,
        runs: table_count(transaction, "run_projection")?,
        queue_items: table_count(transaction, "queue_projection")?,
        permissions: table_count(transaction, "permission_projection")?,
        subagents: table_count(transaction, "subagent_projection")?,
        workspaces: table_count(transaction, "workspace_projection")?,
        timeline_items: table_count(transaction, "timeline_projection")?,
    })
}

fn table_count(transaction: &Transaction<'_>, table: &str) -> Result<u64, TaskStoreError> {
    let count: i64 =
        transaction.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })?;
    u64::try_from(count).map_err(|_| TaskStoreError::IntegerOutOfRange)
}

pub(crate) fn projection_snapshot(
    transaction: &Transaction<'_>,
) -> Result<ProjectionSnapshot, TaskStoreError> {
    let queries = [
        ("task_projection", "task_id"),
        ("run_projection", "task_id, run_segment_id"),
        ("queue_projection", "task_id, queue_item_id"),
        ("permission_projection", "task_id, permission_request_id"),
        ("subagent_projection", "task_id, actor_id"),
        ("workspace_projection", "task_id, workspace_lease_id"),
        ("timeline_projection", "task_id, global_offset"),
    ];
    let mut hasher = blake3::Hasher::new();
    for (table, order) in queries {
        hasher.update(table.as_bytes());
        let sql = format!("SELECT * FROM {table} ORDER BY {order}");
        let mut statement = transaction.prepare(&sql)?;
        let column_count = statement.column_count();
        let rows = statement.query_map([], |row| {
            let mut values = Vec::with_capacity(column_count);
            for index in 0..column_count {
                values.push(row.get::<_, rusqlite::types::Value>(index)?);
            }
            Ok(values)
        })?;
        for row in rows {
            hasher.update(format!("{:?}", row?).as_bytes());
        }
    }
    Ok(ProjectionSnapshot {
        counts: projection_counts(transaction)?,
        digest: hasher.finalize(),
    })
}

fn sqlite_integer(value: u64) -> Result<i64, TaskStoreError> {
    i64::try_from(value).map_err(|_| TaskStoreError::IntegerOutOfRange)
}

fn projector_error(message: impl Into<String>) -> TaskStoreError {
    TaskStoreError::Projector(message.into())
}

fn invalid_transition<T>(message: impl Into<String>) -> Result<T, TaskStoreError> {
    Err(projector_error(message))
}

fn integrity<T>(message: impl Into<String>) -> Result<T, TaskStoreError> {
    Err(TaskStoreError::ProjectionIntegrity(message.into()))
}

#[cfg(test)]
mod tests {
    use super::{
        artifact_timeline_kind, preferred_artifact_surface, timeline_artifact_projection,
        tool_result_command_failed, tool_result_command_output,
    };
    use harness_contracts::{
        BlobId, BlobRef, TimelineArtifactSurface, TimelineEventKind, ToolResult, ToolResultPart,
    };
    use serde_json::json;

    #[test]
    fn detects_unsuccessful_command_results_with_streamed_output() {
        let result = ToolResult::Mixed(vec![
            ToolResultPart::Text {
                text: "command output".into(),
            },
            ToolResultPart::Structured {
                value: json!({ "success": false, "exit_status": { "code": 127 } }),
                schema_ref: None,
            },
        ]);

        assert!(tool_result_command_failed(&result));
        assert_eq!(
            tool_result_command_output(&result).as_deref(),
            Some("command output")
        );
        assert!(!tool_result_command_failed(&ToolResult::Structured(
            json!({ "success": true, "exit_status": { "code": 0 } })
        )));
        assert!(tool_result_command_output(&ToolResult::Structured(json!({
            "success": true,
            "exit_status": { "code": 0 }
        })))
        .is_none());
    }

    #[test]
    fn preserves_object_identity_for_generated_artifacts() {
        assert_eq!(artifact_timeline_kind("image"), TimelineEventKind::Image);
        assert_eq!(
            artifact_timeline_kind("command"),
            TimelineEventKind::Command
        );
        assert_eq!(artifact_timeline_kind("diff"), TimelineEventKind::Diff);
        assert_eq!(artifact_timeline_kind("file"), TimelineEventKind::File);
        assert_eq!(artifact_timeline_kind("video"), TimelineEventKind::Artifact);
    }

    #[test]
    fn projects_renderer_metadata_without_changing_event_kind() {
        let blob_id = BlobId::new();
        let artifact = timeline_artifact_projection(
            Some("artifact-1".into()),
            "demo.mp4".into(),
            Some("video".into()),
            Some(&BlobRef {
                id: blob_id,
                size: 42,
                content_hash: [1; 32],
                content_type: Some("video/mp4".into()),
            }),
            None,
            None,
        );

        assert_eq!(artifact_timeline_kind("video"), TimelineEventKind::Artifact);
        assert_eq!(artifact.blob_id, Some(blob_id));
        assert_eq!(artifact.media_type, "video/mp4");
        assert_eq!(artifact.size, Some(42));
        assert_eq!(
            artifact
                .presentation
                .and_then(|presentation| presentation.preferred_surface),
            Some(TimelineArtifactSurface::Inline)
        );
        assert_eq!(
            preferred_artifact_surface(Some("file"), "application/zip"),
            TimelineArtifactSurface::Card
        );
    }
}

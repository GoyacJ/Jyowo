//! Synchronous projections derived from the canonical task event log.

use chrono::{DateTime, Utc};
use harness_contracts::{
    ActorId, QueueItemProjection, QueueItemState, RunProjection, RunState, RunTerminalReason,
    TaskEventEnvelope, TaskId, TaskProjection, TaskState, TimelineEventKind,
    TimelineItemProjection,
};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use crate::task_event::TaskEvent;
use crate::TaskStoreError;

const MAX_TIMELINE_SUMMARY_CHARS: usize = 4096;
const MAX_ACTIVE_QUEUE_ITEMS: usize = 64;

struct ReducedTask {
    projection: TaskProjection,
    consumed_queue_item: Option<QueueItemProjection>,
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

    let mut consumed_queue_item = None;
    match event {
        TaskEvent::TaskCreated { title } => {
            if projection.stream_version != 0 || !projection.title.is_empty() {
                return invalid_transition("task.created requires a new task stream");
            }
            projection.title.clone_from(title);
            projection.state = TaskState::Idle;
        }
        TaskEvent::TaskTitleChanged { title } => {
            if projection.stream_version == 0 {
                return invalid_transition("task.title_changed requires an existing task");
            }
            projection.title.clone_from(title);
        }
        TaskEvent::TaskArchived { archived } => projection.archived = *archived,
        TaskEvent::RunStarted {
            segment_id,
            started_at,
        } => {
            if projection.pending_permission.is_some() {
                return invalid_transition("run.started requires no pending permission");
            }
            if projection.current_run.as_ref().is_some_and(|run| {
                matches!(
                    run.state,
                    RunState::Running | RunState::WaitingPermission | RunState::Yielding
                )
            }) {
                return invalid_transition("run.started requires no active run");
            }
            projection.current_run = Some(RunProjection {
                segment_id: *segment_id,
                state: RunState::Running,
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
        TaskEvent::MessageQueued {
            queue_item_id,
            content,
            attachments,
            context_references,
            created_at,
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
            let index = projection
                .queue
                .iter()
                .position(|item| item.queue_item_id == *queue_item_id)
                .ok_or_else(|| projector_error("queue event references an unknown item"))?;
            let mut item = projection.queue.remove(index);
            require_queue_revision(&item, *revision, QueueItemState::Queued, "message.consumed")?;
            item.revision = *revision;
            item.state = QueueItemState::Consumed;
            item.consumed_by = Some(*run_segment_id);
            consumed_queue_item = Some(item);
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
        TaskEvent::SubagentSpawned { .. } => {}
        TaskEvent::Engine { .. } => {}
        TaskEvent::WorkspaceAcquired { lease } => {
            if lease.task_id != envelope.task_id {
                return integrity("workspace lease belongs to another task");
            }
        }
    }

    projection.stream_version = envelope.stream_sequence;
    projection.last_global_offset = envelope.global_offset;
    Ok(ReducedTask {
        projection,
        consumed_queue_item,
    })
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
        TaskEvent::RunStarted { segment_id, .. } | TaskEvent::RunCompleted { segment_id, .. } => {
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
        TaskEvent::MessageQueued { queue_item_id, .. }
        | TaskEvent::MessageEdited { queue_item_id, .. } => {
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
        TaskEvent::MessageConsumed { queue_item_id, .. } => {
            let item = reduced
                .consumed_queue_item
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
        TaskEvent::PermissionResolved { request_id, .. } => {
            transaction.execute(
                "DELETE FROM permission_projection
                 WHERE task_id = ?1 AND permission_request_id = ?2",
                params![envelope.task_id.to_string(), request_id.to_string()],
            )?;
        }
        TaskEvent::SubagentSpawned {
            actor_id,
            started_at,
        } => {
            let subagent = SubagentProjection {
                actor_id: *actor_id,
                state: SubagentState::Running,
                started_at: *started_at,
                ended_at: None,
            };
            upsert_entity(
                transaction,
                "subagent_projection",
                "actor_id",
                envelope,
                &actor_id.to_string(),
                &subagent,
            )?;
        }
        TaskEvent::WorkspaceAcquired { lease } => {
            upsert_entity(
                transaction,
                "workspace_projection",
                "workspace_lease_id",
                envelope,
                &lease.lease_id.to_string(),
                lease,
            )?;
        }
        TaskEvent::TaskCreated { .. }
        | TaskEvent::TaskTitleChanged { .. }
        | TaskEvent::TaskArchived { .. }
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
    if matches!(
        event,
        TaskEvent::MessageQueued { .. } | TaskEvent::MessageEdited { .. }
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
        TaskEvent::MessageQueued { .. } | TaskEvent::MessageEdited { .. } => return Ok(()),
        TaskEvent::MessageConsumed { run_segment_id, .. } => (
            TimelineEventKind::UserMessage,
            reduced
                .consumed_queue_item
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
        TaskEvent::SubagentSpawned { .. } => (
            TimelineEventKind::Subagent,
            "Subagent started".into(),
            None,
            false,
        ),
        TaskEvent::WorkspaceAcquired { .. } => (
            TimelineEventKind::Notice,
            "Workspace acquired".into(),
            None,
            false,
        ),
        TaskEvent::Engine { event_type, .. } => (
            TimelineEventKind::Notice,
            event_type
                .strip_prefix("engine.")
                .unwrap_or(event_type)
                .replace('_', " "),
            None,
            false,
        ),
    };
    let blob_id = reduced
        .consumed_queue_item
        .as_ref()
        .and_then(|item| item.attachments.first().copied());
    let timeline = TimelineItemProjection {
        id: envelope.event_id.to_string(),
        kind,
        global_offset: envelope.global_offset,
        run_segment_id,
        summary,
        blob_id,
        incomplete,
    };
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SubagentState {
    Running,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubagentProjection {
    actor_id: ActorId,
    state: SubagentState,
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
        archived: false,
        stream_version: 0,
        last_global_offset: 0,
        current_run: None,
        pending_permission: None,
        queue: Vec::new(),
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

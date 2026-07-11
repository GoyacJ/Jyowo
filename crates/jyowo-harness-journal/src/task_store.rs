//! Unified `SQLite` event store for daemon tasks.

use std::collections::{HashMap, HashSet};
#[cfg(feature = "blob-file")]
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
#[cfg(feature = "blob-file")]
use fs2::FileExt;
use harness_contracts::{
    now, ActorId, BlobId, CheckpointId, ClientId, CommandId, Event, EventId, EventSource,
    EventSourceKind, IdParseError, IndeterminateToolDecision, PermissionMode, QueueItemId,
    QueueItemProjection, RedactRules, Redactor, RunSegmentId, RunState, RunTerminalReason,
    SessionId, SubagentActorState, SubagentId, SubagentParentProjection, SubagentProjection,
    TaskEventEnvelope, TaskId, TaskProjection, TenantId, TimelineItemProjection, ToolUseId,
    WorkspaceLeaseId, WorkspaceLeaseProjection, WorkspaceLeaseState, WorkspaceMode,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use thiserror::Error;

use crate::task_event::{NewTaskEvent, TaskBlobReference, TaskEvent, MAX_EVENT_PAYLOAD_BYTES};
use crate::task_projection::{
    empty_task_projection, load_task_projection, load_task_projection_row, projection_counts,
    projection_snapshot, ProjectionCounts, SynchronousTaskProjector, TaskProjector,
    PROJECTION_TABLES,
};
use crate::task_schema::initialize_task_schema;

const MAX_COMMAND_PAYLOAD_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_EVENTS_PER_TRANSACTION: usize = 256;
pub(crate) const MAX_TOTAL_EVENT_BYTES_PER_TRANSACTION: usize = 8 * 1024 * 1024;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;
const MAX_EVENT_TYPE_BYTES: usize = 128;
const MAX_SOURCE_JSON_BYTES: usize = 4096;
const MAX_READ_PAGE_SIZE: usize = 16;
const REBUILD_PAGE_SIZE: usize = 16;
const CONTEXT_SUMMARY_MEDIA_TYPE: &str = "application/vnd.jyowo.context-summary+json";
#[cfg(any(test, feature = "blob-file"))]
const MAX_TASK_BLOB_BYTES: u64 = 1024 * 1024 * 1024;
#[cfg(any(test, feature = "blob-file"))]
const MAX_GLOBAL_BLOB_BYTES: u64 = 8 * 1024 * 1024 * 1024;
#[cfg(feature = "blob-file")]
const BLOB_OPERATION_LOCK_COUNT: usize = 64;
#[cfg(feature = "blob-file")]
const BLOB_ROOT_CLAIM_DIRECTORY: &str = ".jyowo-task-store";
#[cfg(feature = "blob-file")]
const BLOB_ROOT_LOCK_FILE: &str = ".jyowo-task-store.lock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventAuthority {
    source: EventSource,
    principal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentRunInput {
    pub queue_item_id: QueueItemId,
    pub content: String,
    pub attachments: Vec<BlobId>,
    pub context_references: Vec<String>,
    pub model_config_id: Option<String>,
    pub permission_mode: PermissionMode,
}

impl EventAuthority {
    fn source(&self) -> &EventSource {
        &self.source
    }

    fn principal_id(&self) -> &str {
        &self.principal_id
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AcceptedCommand {
    pub command_id: CommandId,
    pub task_id: TaskId,
    pub idempotency_key: String,
    pub expected_stream_version: u64,
    pub authority: EventAuthority,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandOutcome {
    Accepted {
        command_id: CommandId,
        task_id: TaskId,
        stream_version: u64,
        committed_offset: u64,
    },
    Rejected {
        command_id: CommandId,
        task_id: TaskId,
        rejection: CommandRejection,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandRejection {
    WrongExpectedVersion { expected: u64, actual: u64 },
    StaleQueueRevision { latest: Box<QueueItemProjection> },
    InvalidCommand { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceBaseline {
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskCheckpoint {
    pub checkpoint_id: CheckpointId,
    pub task_id: TaskId,
    pub run_segment_id: RunSegmentId,
    pub committed_global_offset: u64,
    pub context_cursor: u64,
    pub queue_revision: u64,
    pub workspace_baseline: Option<WorkspaceBaseline>,
    pub incomplete_tool_use_ids: Vec<ToolUseId>,
    pub child_actor_refs: Vec<ActorId>,
    pub context_blob_id: Option<BlobId>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingSegmentStart {
    pub task_id: TaskId,
    pub segment_id: RunSegmentId,
    pub indeterminate_tools: Vec<IndeterminateToolDecision>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContinueTaskCommandPayload {
    #[serde(rename = "type")]
    command_type: String,
    segment_id: RunSegmentId,
    indeterminate_tools: Vec<IndeterminateToolDecision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextSummary {
    pub summary_id: CheckpointId,
    pub task_id: TaskId,
    pub source_start_global_offset: u64,
    pub source_end_global_offset: u64,
    pub blob_id: BlobId,
    pub created_at: DateTime<Utc>,
}

pub type TaskWorkspaceLeaseState = WorkspaceLeaseState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskWorkspaceLease {
    pub lease_id: WorkspaceLeaseId,
    pub task_id: TaskId,
    pub actor_id: ActorId,
    pub mode: WorkspaceMode,
    pub canonical_root: String,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub writable: bool,
    pub state: TaskWorkspaceLeaseState,
    pub requested_at: DateTime<Utc>,
    pub acquired_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub baseline_commit: Option<String>,
    pub baseline_status: String,
    pub patch_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquireTaskWorkspaceLease {
    pub lease_id: WorkspaceLeaseId,
    pub task_id: TaskId,
    pub actor_id: ActorId,
    pub mode: WorkspaceMode,
    pub canonical_root: String,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub writable: bool,
    pub requested_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub baseline_commit: Option<String>,
    pub baseline_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskWorkspaceAcquireOutcome {
    Acquired(TaskWorkspaceLease),
    Waiting(TaskWorkspaceLease),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseTaskWorkspaceLeaseOutcome {
    pub released: TaskWorkspaceLease,
    pub acquired: Vec<TaskWorkspaceLease>,
}

pub struct TaskStore {
    database_path: PathBuf,
    connection: Mutex<Connection>,
    projector: Arc<dyn TaskProjector>,
    workspace_dispatches: Mutex<HashMap<WorkspaceLeaseId, usize>>,
    #[cfg(feature = "blob-file")]
    blob_operation_locks: [Mutex<()>; BLOB_OPERATION_LOCK_COUNT],
    #[cfg(feature = "blob-file")]
    database_identity: String,
    #[cfg(feature = "blob-file")]
    blob_root_lock: Mutex<Option<std::fs::File>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateSubagentActorRequest {
    pub child_task_id: TaskId,
    pub actor_id: ActorId,
    pub segment_id: RunSegmentId,
    pub parent_task_id: TaskId,
    pub parent_segment_id: RunSegmentId,
    pub parent_actor_id: ActorId,
    pub parent_workspace_lease_id: WorkspaceLeaseId,
    pub delegation_id: SubagentId,
    pub context_cursor: u64,
    pub title: String,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactedSubagentSummary(String);

impl RedactedSubagentSummary {
    #[must_use]
    pub fn new(redactor: &dyn Redactor, input: &str) -> Self {
        Self(
            redactor
                .redact(input, &RedactRules::default())
                .chars()
                .take(256)
                .collect(),
        )
    }

    fn into_inner(self) -> String {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentLifecycleAuthority {
    Supervisor,
    Actor(ActorId),
    Recovery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentLifecycleCommand {
    pub parent_task_id: TaskId,
    pub child_task_id: TaskId,
    pub actor_id: ActorId,
    pub authority: SubagentLifecycleAuthority,
    pub transition: SubagentLifecycleTransition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubagentLifecycleTransition {
    Running {
        workspace_lease_id: WorkspaceLeaseId,
        context_cursor: u64,
    },
    Yielding,
    Background,
    Completed {
        summary: RedactedSubagentSummary,
        ended_at: DateTime<Utc>,
    },
    Cancelled {
        ended_at: DateTime<Utc>,
    },
    Failed {
        ended_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedParentStopSubagent {
    pub child_task_id: TaskId,
    pub actor_id: ActorId,
    pub expected_state: SubagentActorState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentSubagentStopMode {
    SafePoint,
    Force { ended_at: DateTime<Utc> },
}

fn subagent_lifecycle_authority(
    authority: SubagentLifecycleAuthority,
    actor_id: ActorId,
) -> Result<EventAuthority, TaskStoreError> {
    match authority {
        SubagentLifecycleAuthority::Supervisor => Ok(TaskStore::supervisor_authority()),
        SubagentLifecycleAuthority::Actor(claimed) if claimed == actor_id => {
            Ok(TaskStore::subagent_authority(actor_id))
        }
        SubagentLifecycleAuthority::Actor(_) => Err(TaskStoreError::InvalidInput(
            "subagent lifecycle authority does not match the durable actor".into(),
        )),
        SubagentLifecycleAuthority::Recovery => Ok(TaskStore::recovery_authority()),
    }
}

fn validate_subagent_lifecycle_authority(
    authority: SubagentLifecycleAuthority,
    transition: &SubagentLifecycleTransition,
) -> Result<(), TaskStoreError> {
    if matches!(authority, SubagentLifecycleAuthority::Actor(_))
        && matches!(
            transition,
            SubagentLifecycleTransition::Yielding | SubagentLifecycleTransition::Background
        )
    {
        return Err(TaskStoreError::InvalidInput(
            "only the supervisor may yield or background a subagent".into(),
        ));
    }
    if authority == SubagentLifecycleAuthority::Recovery
        && !matches!(
            transition,
            SubagentLifecycleTransition::Cancelled { .. }
                | SubagentLifecycleTransition::Failed { .. }
        )
    {
        return Err(TaskStoreError::InvalidInput(
            "recovery authority may only terminate a subagent".into(),
        ));
    }
    Ok(())
}

fn require_subagent_state(
    actual: SubagentActorState,
    expected: SubagentActorState,
) -> Result<(), TaskStoreError> {
    if actual != expected {
        return Err(TaskStoreError::InvalidInput(format!(
            "subagent lifecycle expected {expected:?}, found {actual:?}"
        )));
    }
    Ok(())
}

fn is_nonterminal_subagent_state(state: SubagentActorState) -> bool {
    matches!(
        state,
        SubagentActorState::Starting
            | SubagentActorState::Running
            | SubagentActorState::Yielding
            | SubagentActorState::Background
    )
}

fn require_nonterminal_subagent_state(state: SubagentActorState) -> Result<(), TaskStoreError> {
    if !is_nonterminal_subagent_state(state) {
        return Err(TaskStoreError::InvalidInput(
            "subagent lifecycle transition requires a nonterminal actor".into(),
        ));
    }
    Ok(())
}

fn validate_subagent_end_time(
    child: &SubagentProjection,
    ended_at: DateTime<Utc>,
) -> Result<(), TaskStoreError> {
    if ended_at < child.started_at {
        return Err(TaskStoreError::InvalidInput(
            "subagent terminal time precedes its start".into(),
        ));
    }
    Ok(())
}

fn subagent_run_completed(child: &SubagentProjection) -> Option<NewTaskEvent> {
    let ended_at = child.ended_at?;
    let terminal_reason = match child.state {
        SubagentActorState::Completed => RunTerminalReason::Completed,
        SubagentActorState::Cancelled => RunTerminalReason::Cancelled,
        SubagentActorState::Failed => RunTerminalReason::Failed,
        _ => return None,
    };
    Some(NewTaskEvent::run_completed(
        child.segment_id,
        ended_at,
        terminal_reason,
        child.state != SubagentActorState::Completed,
    ))
}

fn mark_subagent_workspace_cleanup_pending_in_transaction(
    transaction: &Transaction<'_>,
    child: &SubagentProjection,
) -> Result<Option<NewTaskEvent>, TaskStoreError> {
    let Some(lease_id) = child.workspace_lease_id else {
        return Ok(None);
    };
    let mut lease = workspace_lease_in_transaction(transaction, lease_id)?;
    if lease.task_id != child.child_task_id
        || lease.actor_id != child.actor_id
        || lease.mode != WorkspaceMode::ManagedWorktree
    {
        return Err(TaskStoreError::ProjectionIntegrity(
            "subagent terminal workspace lease ownership mismatch".into(),
        ));
    }
    if lease.state == TaskWorkspaceLeaseState::CleanupPending {
        return Ok(None);
    }
    if !matches!(
        lease.state,
        TaskWorkspaceLeaseState::Active
            | TaskWorkspaceLeaseState::Preparing
            | TaskWorkspaceLeaseState::Expired
            | TaskWorkspaceLeaseState::CleanupBlocked
    ) {
        return Err(TaskStoreError::InvalidInput(format!(
            "subagent workspace lease {lease_id} cannot enter terminal cleanup"
        )));
    }
    lease.state = TaskWorkspaceLeaseState::CleanupPending;
    update_workspace_lease_in_transaction(transaction, &lease)?;
    Ok(Some(NewTaskEvent::workspace_cleanup_pending(
        workspace_lease_projection(&lease),
    )))
}

pub struct WorkspaceDispatchGuard<'a> {
    store: &'a TaskStore,
    lease_id: WorkspaceLeaseId,
}

impl Drop for WorkspaceDispatchGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut dispatches) = self.store.workspace_dispatches.lock() {
            let remove = dispatches.get_mut(&self.lease_id).is_some_and(|count| {
                *count = count.saturating_sub(1);
                *count == 0
            });
            if remove {
                dispatches.remove(&self.lease_id);
            }
        }
    }
}

#[cfg(feature = "blob-file")]
pub(crate) struct StoredBlobMetadata {
    pub(crate) media_type: String,
    pub(crate) byte_size: u64,
    pub(crate) content_hash: [u8; 32],
    pub(crate) relative_path: String,
}

struct StagedBlobMetadata {
    media_type: String,
    byte_size: u64,
    content_hash: String,
    relative_path: String,
}

impl TaskStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TaskStoreError> {
        Self::open_with_projector(path, Arc::new(SynchronousTaskProjector))
    }

    pub fn create_subagent_actor_checked(
        &self,
        request: CreateSubagentActorRequest,
    ) -> Result<(), TaskStoreError> {
        if request.child_task_id == request.parent_task_id {
            return Err(TaskStoreError::InvalidInput(
                "subagent child task must differ from its parent".into(),
            ));
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if stream_version_in_transaction(&transaction, request.child_task_id)? != 0 {
            return Err(TaskStoreError::InvalidInput(format!(
                "subagent child task {} already exists",
                request.child_task_id
            )));
        }
        let parent_projection = load_task_projection(&transaction, request.parent_task_id)?
            .ok_or_else(|| {
                TaskStoreError::InvalidInput(format!(
                    "subagent parent task {} does not exist",
                    request.parent_task_id
                ))
            })?;
        if parent_projection.actor_id != Some(request.parent_actor_id) {
            return Err(TaskStoreError::InvalidInput(
                "subagent parent actor does not match the durable task actor".into(),
            ));
        }
        if !parent_projection.current_run.as_ref().is_some_and(|run| {
            run.segment_id == request.parent_segment_id && run.state == RunState::Running
        }) {
            return Err(TaskStoreError::InvalidInput(
                "subagent parent segment is not the current running segment".into(),
            ));
        }
        let parent_lease =
            workspace_lease_in_transaction(&transaction, request.parent_workspace_lease_id)?;
        if parent_lease.task_id != request.parent_task_id
            || parent_lease.actor_id != request.parent_actor_id
            || parent_lease.state != TaskWorkspaceLeaseState::Active
        {
            return Err(TaskStoreError::InvalidInput(
                "subagent parent workspace lease does not belong to the active parent actor".into(),
            ));
        }
        let child = SubagentProjection {
            child_task_id: request.child_task_id,
            actor_id: request.actor_id,
            segment_id: request.segment_id,
            parent_task_id: request.parent_task_id,
            parent_segment_id: request.parent_segment_id,
            delegation_id: request.delegation_id,
            context_cursor: request.context_cursor,
            workspace_lease_id: None,
            state: SubagentActorState::Starting,
            detached: false,
            summary: None,
            started_at: request.started_at,
            ended_at: None,
        };
        let parent = SubagentParentProjection {
            parent_task_id: child.parent_task_id,
            parent_segment_id: child.parent_segment_id,
            delegation_id: child.delegation_id,
        };
        let supervisor = Self::supervisor_authority();
        let mut child_events = append_in_transaction(
            &transaction,
            child.child_task_id,
            0,
            supervisor.source(),
            vec![NewTaskEvent::task_created(request.title)],
        )?;
        child_events.extend(append_in_transaction(
            &transaction,
            child.child_task_id,
            1,
            Self::subagent_authority(child.actor_id).source(),
            vec![NewTaskEvent::subagent_linked(
                child.actor_id,
                child.context_cursor,
                parent,
            )],
        )?);
        child_events.extend(append_in_transaction(
            &transaction,
            child.child_task_id,
            2,
            supervisor.source(),
            vec![NewTaskEvent::run_started(
                child.segment_id,
                child.started_at,
            )],
        )?);
        let parent_events = append_in_transaction(
            &transaction,
            child.parent_task_id,
            parent_projection.stream_version,
            supervisor.source(),
            vec![NewTaskEvent::subagent_actor_spawned(child)],
        )?;
        for event in child_events.iter().chain(&parent_events) {
            self.projector.apply(&transaction, event)?;
        }
        roll_forward_subagent_checkpoint_if_running(
            &transaction,
            child_events[0].task_id,
            &child_events,
        )?;
        roll_forward_subagent_checkpoint_if_running(
            &transaction,
            parent_events[0].task_id,
            &parent_events,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn apply_subagent_lifecycle(
        &self,
        command: SubagentLifecycleCommand,
    ) -> Result<SubagentProjection, TaskStoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let parent_projection = load_task_projection(&transaction, command.parent_task_id)?
            .ok_or_else(|| {
                TaskStoreError::InvalidInput("subagent parent task is missing".into())
            })?;
        let existing = parent_projection
            .subagents
            .iter()
            .find(|child| child.child_task_id == command.child_task_id)
            .ok_or_else(|| {
                TaskStoreError::InvalidInput("subagent child is not linked to the parent".into())
            })?;
        if existing.actor_id != command.actor_id {
            return Err(TaskStoreError::InvalidInput(
                "subagent lifecycle actor does not match durable topology".into(),
            ));
        }
        let child_projection = load_task_projection(&transaction, command.child_task_id)?
            .ok_or_else(|| TaskStoreError::InvalidInput("subagent child task is missing".into()))?;
        let expected_parent = SubagentParentProjection {
            parent_task_id: existing.parent_task_id,
            parent_segment_id: existing.parent_segment_id,
            delegation_id: existing.delegation_id,
        };
        if child_projection.actor_id != Some(existing.actor_id)
            || child_projection.parent.as_ref() != Some(&expected_parent)
        {
            return Err(TaskStoreError::ProjectionIntegrity(
                "subagent parent and child topology disagree".into(),
            ));
        }
        let source = subagent_lifecycle_authority(command.authority, existing.actor_id)?;
        let mut child = existing.clone();
        validate_subagent_lifecycle_authority(command.authority, &command.transition)?;
        let child_updates = match command.transition {
            SubagentLifecycleTransition::Running {
                workspace_lease_id,
                context_cursor,
            } => {
                if !matches!(
                    child.state,
                    SubagentActorState::Starting | SubagentActorState::Yielding
                ) {
                    return Err(TaskStoreError::InvalidInput(format!(
                        "subagent workspace start expected Starting or Yielding, found {:?}",
                        child.state
                    )));
                }
                let pending_yield = child.state == SubagentActorState::Yielding;
                let lease = workspace_lease_in_transaction(&transaction, workspace_lease_id)?;
                if lease.task_id != child.child_task_id
                    || lease.actor_id != child.actor_id
                    || lease.mode != WorkspaceMode::ManagedWorktree
                    || lease.state != TaskWorkspaceLeaseState::Active
                {
                    return Err(TaskStoreError::InvalidInput(
                        "subagent workspace lease does not belong to the active child actor".into(),
                    ));
                }
                child.workspace_lease_id = Some(workspace_lease_id);
                child.context_cursor = context_cursor;
                if !pending_yield {
                    child.state = SubagentActorState::Running;
                }
                vec![NewTaskEvent::subagent_state_changed(child.clone())]
            }
            SubagentLifecycleTransition::Yielding => {
                require_subagent_state(child.state, SubagentActorState::Running)?;
                child.state = SubagentActorState::Yielding;
                vec![NewTaskEvent::subagent_state_changed(child.clone())]
            }
            SubagentLifecycleTransition::Background => {
                if !matches!(
                    child.state,
                    SubagentActorState::Running | SubagentActorState::Yielding
                ) {
                    return Err(TaskStoreError::InvalidInput(
                        "only a running or yielding subagent can continue in background".into(),
                    ));
                }
                child.detached = true;
                child.state = SubagentActorState::Background;
                vec![NewTaskEvent::subagent_backgrounded(child.clone())]
            }
            SubagentLifecycleTransition::Completed { summary, ended_at } => {
                require_nonterminal_subagent_state(child.state)?;
                validate_subagent_end_time(&child, ended_at)?;
                let mut summarized = child.clone();
                summarized.summary = Some(summary.into_inner());
                child = summarized.clone();
                child.state = SubagentActorState::Completed;
                child.ended_at = Some(ended_at);
                vec![
                    NewTaskEvent::subagent_summary_updated(summarized),
                    NewTaskEvent::subagent_terminal(child.clone()),
                ]
            }
            SubagentLifecycleTransition::Cancelled { ended_at } => {
                require_nonterminal_subagent_state(child.state)?;
                validate_subagent_end_time(&child, ended_at)?;
                child.state = SubagentActorState::Cancelled;
                child.ended_at = Some(ended_at);
                vec![NewTaskEvent::subagent_terminal(child.clone())]
            }
            SubagentLifecycleTransition::Failed { ended_at } => {
                require_nonterminal_subagent_state(child.state)?;
                validate_subagent_end_time(&child, ended_at)?;
                child.state = SubagentActorState::Failed;
                child.ended_at = Some(ended_at);
                vec![NewTaskEvent::subagent_terminal(child.clone())]
            }
        };
        let parent_updates = child_updates.clone();
        let run_completion_update = subagent_run_completed(&child);
        let cleanup_update = if matches!(
            child.state,
            SubagentActorState::Completed
                | SubagentActorState::Cancelled
                | SubagentActorState::Failed
        ) {
            mark_subagent_workspace_cleanup_pending_in_transaction(&transaction, &child)?
        } else {
            None
        };
        let child_version = child_projection.stream_version;
        let mut child_events = append_in_transaction(
            &transaction,
            child.child_task_id,
            child_version,
            source.source(),
            child_updates,
        )?;
        if let Some(run_completion_update) = run_completion_update {
            child_events.extend(append_in_transaction(
                &transaction,
                child.child_task_id,
                child_version + child_events.len() as u64,
                Self::supervisor_authority().source(),
                vec![run_completion_update],
            )?);
        }
        if let Some(cleanup_update) = cleanup_update {
            child_events.extend(append_in_transaction(
                &transaction,
                child.child_task_id,
                child_version + child_events.len() as u64,
                Self::supervisor_authority().source(),
                vec![cleanup_update],
            )?);
        }
        let parent_events = append_in_transaction(
            &transaction,
            child.parent_task_id,
            parent_projection.stream_version,
            source.source(),
            parent_updates,
        )?;
        for envelope in child_events.iter().chain(&parent_events) {
            self.projector.apply(&transaction, envelope)?;
        }
        roll_forward_subagent_checkpoint_if_running(
            &transaction,
            child.child_task_id,
            &child_events,
        )?;
        roll_forward_subagent_checkpoint_if_running(
            &transaction,
            child.parent_task_id,
            &parent_events,
        )?;
        transaction.commit()?;
        Ok(child)
    }

    pub fn apply_parent_subagent_stop(
        &self,
        parent_task_id: TaskId,
        expected_children: &[ExpectedParentStopSubagent],
        mode: ParentSubagentStopMode,
    ) -> Result<Vec<SubagentProjection>, TaskStoreError> {
        let mut identities = HashSet::with_capacity(expected_children.len());
        if expected_children
            .iter()
            .any(|child| !identities.insert((child.child_task_id, child.actor_id)))
        {
            return Err(TaskStoreError::InvalidInput(
                "parent subagent stop contains a duplicate child".into(),
            ));
        }

        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let parent_projection =
            load_task_projection(&transaction, parent_task_id)?.ok_or_else(|| {
                TaskStoreError::InvalidInput("subagent parent task is missing".into())
            })?;
        let mut validated = Vec::with_capacity(expected_children.len());
        for expected in expected_children {
            let existing = parent_projection
                .subagents
                .iter()
                .find(|child| child.child_task_id == expected.child_task_id)
                .ok_or_else(|| {
                    TaskStoreError::InvalidInput(
                        "parent stop child is not linked to the parent".into(),
                    )
                })?;
            if existing.actor_id != expected.actor_id {
                return Err(TaskStoreError::InvalidInput(
                    "parent stop child actor is stale".into(),
                ));
            }
            if existing.detached || !is_nonterminal_subagent_state(existing.state) {
                continue;
            }
            require_subagent_state(existing.state, expected.expected_state)?;
            let child_projection = load_task_projection(&transaction, existing.child_task_id)?
                .ok_or_else(|| {
                    TaskStoreError::InvalidInput("subagent child task is missing".into())
                })?;
            let expected_parent = SubagentParentProjection {
                parent_task_id: existing.parent_task_id,
                parent_segment_id: existing.parent_segment_id,
                delegation_id: existing.delegation_id,
            };
            if child_projection.actor_id != Some(existing.actor_id)
                || child_projection.parent.as_ref() != Some(&expected_parent)
            {
                return Err(TaskStoreError::ProjectionIntegrity(
                    "subagent parent and child topology disagree".into(),
                ));
            }
            let mut updated = existing.clone();
            let event = match mode {
                ParentSubagentStopMode::SafePoint => {
                    if !matches!(
                        updated.state,
                        SubagentActorState::Starting | SubagentActorState::Running
                    ) {
                        return Err(TaskStoreError::InvalidInput(format!(
                            "safe parent stop expected Starting or Running, found {:?}",
                            updated.state
                        )));
                    }
                    updated.state = SubagentActorState::Yielding;
                    NewTaskEvent::subagent_state_changed(updated.clone())
                }
                ParentSubagentStopMode::Force { ended_at } => {
                    require_nonterminal_subagent_state(updated.state)?;
                    validate_subagent_end_time(&updated, ended_at)?;
                    updated.state = SubagentActorState::Cancelled;
                    updated.ended_at = Some(ended_at);
                    NewTaskEvent::subagent_terminal(updated.clone())
                }
            };
            let mut child_events = vec![event.clone()];
            if matches!(mode, ParentSubagentStopMode::Force { .. }) {
                if let Some(run_completed) = subagent_run_completed(&updated) {
                    child_events.push(run_completed);
                }
                if let Some(cleanup_update) =
                    mark_subagent_workspace_cleanup_pending_in_transaction(&transaction, &updated)?
                {
                    child_events.push(cleanup_update);
                }
            }
            validated.push((
                child_projection.stream_version,
                updated,
                child_events,
                event,
            ));
        }

        let source = Self::supervisor_authority();
        let mut committed_child_events = Vec::with_capacity(validated.len());
        let mut parent_updates = Vec::with_capacity(validated.len());
        let mut updated_children = Vec::with_capacity(validated.len());
        for (child_version, child, child_updates, parent_update) in validated {
            let child_events = append_in_transaction(
                &transaction,
                child.child_task_id,
                child_version,
                source.source(),
                child_updates,
            )?;
            committed_child_events.push((child.child_task_id, child_events));
            parent_updates.push(parent_update);
            updated_children.push(child);
        }
        let parent_events = append_in_transaction(
            &transaction,
            parent_task_id,
            parent_projection.stream_version,
            source.source(),
            parent_updates,
        )?;
        for (_, events) in &committed_child_events {
            for event in events {
                self.projector.apply(&transaction, event)?;
            }
        }
        for event in &parent_events {
            self.projector.apply(&transaction, event)?;
        }
        for (child_task_id, events) in &committed_child_events {
            roll_forward_subagent_checkpoint_if_running(&transaction, *child_task_id, events)?;
        }
        if !parent_events.is_empty() {
            roll_forward_subagent_checkpoint_if_running(
                &transaction,
                parent_task_id,
                &parent_events,
            )?;
        }
        transaction.commit()?;
        Ok(updated_children)
    }

    pub fn nonterminal_subagent_actors(&self) -> Result<Vec<SubagentProjection>, TaskStoreError> {
        Ok(self
            .task_projections()?
            .into_iter()
            .flat_map(|projection| projection.subagents)
            .filter(|child| is_nonterminal_subagent_state(child.state))
            .collect())
    }

    pub fn recover_subagent_actor(
        &self,
        child_task_id: TaskId,
        actor_id: ActorId,
        ended_at: DateTime<Utc>,
    ) -> Result<SubagentProjection, TaskStoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let child_projection = load_task_projection(&transaction, child_task_id)?
            .ok_or_else(|| TaskStoreError::InvalidInput("subagent child task is missing".into()))?;
        if child_projection.actor_id != Some(actor_id) {
            return Err(TaskStoreError::InvalidInput(
                "recovery actor does not match the durable child actor".into(),
            ));
        }
        let parent = child_projection.parent.as_ref().ok_or_else(|| {
            TaskStoreError::ProjectionIntegrity("subagent child has no durable parent".into())
        })?;
        let parent_projection = load_task_projection(&transaction, parent.parent_task_id)?
            .ok_or_else(|| {
                TaskStoreError::InvalidInput("subagent parent task is missing".into())
            })?;
        let existing = parent_projection
            .subagents
            .iter()
            .find(|child| child.child_task_id == child_task_id)
            .ok_or_else(|| {
                TaskStoreError::ProjectionIntegrity(
                    "subagent child is not linked from its durable parent".into(),
                )
            })?;
        if existing.actor_id != actor_id {
            return Err(TaskStoreError::ProjectionIntegrity(
                "subagent recovery topology actor mismatch".into(),
            ));
        }
        if existing.state == SubagentActorState::Failed {
            return Ok(existing.clone());
        }
        require_nonterminal_subagent_state(existing.state)?;
        validate_subagent_end_time(existing, ended_at)?;

        let mut recovered = existing.clone();
        recovered.state = SubagentActorState::Failed;
        recovered.ended_at = Some(ended_at);
        let mut child_updates = vec![NewTaskEvent::subagent_terminal(recovered.clone())];
        if let Some(run_completed) = subagent_run_completed(&recovered) {
            child_updates.push(run_completed);
        }
        if let Some(lease_id) = recovered.workspace_lease_id {
            let mut lease = workspace_lease_in_transaction(&transaction, lease_id)?;
            if lease.task_id != child_task_id
                || lease.actor_id != actor_id
                || lease.mode != WorkspaceMode::ManagedWorktree
            {
                return Err(TaskStoreError::ProjectionIntegrity(
                    "recovered subagent workspace lease ownership mismatch".into(),
                ));
            }
            if lease.state != TaskWorkspaceLeaseState::CleanupPending {
                if !matches!(
                    lease.state,
                    TaskWorkspaceLeaseState::Active
                        | TaskWorkspaceLeaseState::Preparing
                        | TaskWorkspaceLeaseState::Expired
                        | TaskWorkspaceLeaseState::CleanupBlocked
                ) {
                    return Err(TaskStoreError::InvalidInput(format!(
                        "workspace lease {lease_id} cannot enter recovery cleanup"
                    )));
                }
                lease.state = TaskWorkspaceLeaseState::CleanupPending;
                update_workspace_lease_in_transaction(&transaction, &lease)?;
                child_updates.push(NewTaskEvent::workspace_cleanup_pending(
                    workspace_lease_projection(&lease),
                ));
            }
        }

        let recovery = Self::recovery_authority();
        let child_events = append_in_transaction(
            &transaction,
            child_task_id,
            child_projection.stream_version,
            recovery.source(),
            child_updates,
        )?;
        let parent_events = append_in_transaction(
            &transaction,
            parent_projection.task_id,
            parent_projection.stream_version,
            recovery.source(),
            vec![NewTaskEvent::subagent_terminal(recovered.clone())],
        )?;
        for event in child_events.iter().chain(&parent_events) {
            self.projector.apply(&transaction, event)?;
        }
        roll_forward_subagent_checkpoint_if_running(&transaction, child_task_id, &child_events)?;
        roll_forward_subagent_checkpoint_if_running(
            &transaction,
            parent_projection.task_id,
            &parent_events,
        )?;
        transaction.commit()?;
        Ok(recovered)
    }

    pub(crate) fn open_with_projector(
        path: impl AsRef<Path>,
        projector: Arc<dyn TaskProjector>,
    ) -> Result<Self, TaskStoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let path = crate::app_controlled_path(path)?;
        let connection = Connection::open(&path)?;
        initialize_task_schema(&connection)?;
        #[cfg(feature = "blob-file")]
        let database_identity = load_or_create_blob_store_identity(&connection)?;
        Ok(Self {
            database_path: path,
            connection: Mutex::new(connection),
            projector,
            workspace_dispatches: Mutex::new(HashMap::new()),
            #[cfg(feature = "blob-file")]
            blob_operation_locks: std::array::from_fn(|_| Mutex::new(())),
            #[cfg(feature = "blob-file")]
            database_identity,
            #[cfg(feature = "blob-file")]
            blob_root_lock: Mutex::new(None),
        })
    }

    #[must_use]
    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    #[must_use]
    pub fn user_authority(client_id: ClientId) -> EventAuthority {
        EventAuthority {
            source: EventSource {
                kind: EventSourceKind::User,
                actor_id: None,
                client_id: Some(client_id),
            },
            principal_id: format!("user:{client_id}"),
        }
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn supervisor_authority() -> EventAuthority {
        EventAuthority {
            source: EventSource {
                kind: EventSourceKind::Supervisor,
                actor_id: None,
                client_id: None,
            },
            principal_id: "system:supervisor".into(),
        }
    }

    #[must_use]
    pub fn recovery_authority() -> EventAuthority {
        EventAuthority {
            source: EventSource {
                kind: EventSourceKind::Recovery,
                actor_id: None,
                client_id: None,
            },
            principal_id: "system:recovery".into(),
        }
    }

    #[must_use]
    pub(crate) fn subagent_authority(actor_id: ActorId) -> EventAuthority {
        EventAuthority {
            source: EventSource {
                kind: EventSourceKind::Subagent,
                actor_id: Some(actor_id),
                client_id: None,
            },
            principal_id: format!("subagent:{actor_id}"),
        }
    }

    #[must_use]
    pub fn supervisor_command_authority(authority: &EventAuthority) -> EventAuthority {
        EventAuthority {
            source: EventSource {
                kind: EventSourceKind::Supervisor,
                actor_id: None,
                client_id: authority.source.client_id,
            },
            principal_id: authority.principal_id.clone(),
        }
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn permission_broker_authority() -> EventAuthority {
        EventAuthority {
            source: EventSource {
                kind: EventSourceKind::PermissionBroker,
                actor_id: None,
                client_id: None,
            },
            principal_id: "system:permission_broker".into(),
        }
    }

    #[must_use]
    pub fn permission_broker_command_authority(authority: &EventAuthority) -> EventAuthority {
        EventAuthority {
            source: EventSource {
                kind: EventSourceKind::PermissionBroker,
                actor_id: None,
                client_id: authority.source.client_id,
            },
            principal_id: authority.principal_id.clone(),
        }
    }

    #[must_use]
    pub(crate) fn engine_authority() -> EventAuthority {
        EventAuthority {
            source: EventSource {
                kind: EventSourceKind::Engine,
                actor_id: None,
                client_id: None,
            },
            principal_id: "system:engine".into(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn append(
        &self,
        task_id: TaskId,
        expected_version: u64,
        authority: &EventAuthority,
        events: Vec<NewTaskEvent>,
    ) -> Result<Vec<TaskEventEnvelope>, TaskStoreError> {
        validate_events(authority.source(), &events)?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_blob_references_in_transaction(&transaction, task_id, &events)?;
        let committed = append_in_transaction(
            &transaction,
            task_id,
            expected_version,
            authority.source(),
            events,
        )?;
        for event in &committed {
            self.projector.apply(&transaction, event)?;
        }
        roll_forward_safe_checkpoint_in_transaction(&transaction, task_id, &committed)?;
        transaction.commit()?;
        Ok(committed)
    }

    pub fn transact_command<F>(
        &self,
        command: AcceptedCommand,
        decide: F,
    ) -> Result<CommandOutcome, TaskStoreError>
    where
        F: FnOnce(&TaskProjection) -> Result<Vec<NewTaskEvent>, CommandRejection>,
    {
        validate_command(&command)?;
        let command_hash = command_hash(&command)?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(outcome) = stored_command_outcome(&transaction, &command, &command_hash)? {
            return Ok(outcome);
        }

        transaction.execute(
            "INSERT INTO command_inbox (
                command_id, task_id, principal_id, idempotency_key, command_hash,
                expected_stream_version, status, accepted_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'processing', ?7)",
            params![
                command.command_id.to_string(),
                command.task_id.to_string(),
                command.authority.principal_id(),
                command.idempotency_key,
                command_hash,
                sqlite_integer(command.expected_stream_version)?,
                now().to_rfc3339(),
            ],
        )?;

        let actual = stream_version_in_transaction(&transaction, command.task_id)?;
        let projection = projection_for_decision(&transaction, command.task_id, actual)?;
        if actual != command.expected_stream_version {
            let outcome = CommandOutcome::Rejected {
                command_id: command.command_id,
                task_id: command.task_id,
                rejection: CommandRejection::WrongExpectedVersion {
                    expected: command.expected_stream_version,
                    actual,
                },
            };
            finish_command(
                &transaction,
                command.command_id,
                "rejected",
                &outcome,
                actual,
                latest_global_offset_in_transaction(&transaction)?,
                0,
            )?;
            transaction.commit()?;
            return Ok(outcome);
        }

        let events = match decide(&projection) {
            Ok(events) => events,
            Err(rejection) => {
                let outcome = CommandOutcome::Rejected {
                    command_id: command.command_id,
                    task_id: command.task_id,
                    rejection,
                };
                finish_command(
                    &transaction,
                    command.command_id,
                    "rejected",
                    &outcome,
                    actual,
                    latest_global_offset_in_transaction(&transaction)?,
                    0,
                )?;
                transaction.commit()?;
                return Ok(outcome);
            }
        };
        if events.is_empty() {
            let outcome = CommandOutcome::Rejected {
                command_id: command.command_id,
                task_id: command.task_id,
                rejection: CommandRejection::InvalidCommand {
                    message: "accepted commands must emit at least one task event".into(),
                },
            };
            finish_command(
                &transaction,
                command.command_id,
                "rejected",
                &outcome,
                actual,
                latest_global_offset_in_transaction(&transaction)?,
                0,
            )?;
            transaction.commit()?;
            return Ok(outcome);
        }
        validate_queue_consumption_events(&events)?;
        validate_events(command.authority.source(), &events)?;
        if let Err(error) = prepare_command_blob_references(&transaction, command.task_id, &events)
        {
            let Some(rejection) = blob_command_rejection(&error) else {
                return Err(error);
            };
            let outcome = CommandOutcome::Rejected {
                command_id: command.command_id,
                task_id: command.task_id,
                rejection,
            };
            finish_command(
                &transaction,
                command.command_id,
                "rejected",
                &outcome,
                actual,
                latest_global_offset_in_transaction(&transaction)?,
                0,
            )?;
            transaction.commit()?;
            return Ok(outcome);
        }
        let committed = append_in_transaction(
            &transaction,
            command.task_id,
            command.expected_stream_version,
            command.authority.source(),
            events,
        )?;
        enqueue_segment_start_in_transaction(
            &transaction,
            command.task_id,
            &command.payload,
            &committed,
        )?;
        for event in &committed {
            self.projector.apply(&transaction, event)?;
        }
        roll_forward_safe_checkpoint_in_transaction(&transaction, command.task_id, &committed)?;
        let stream_version = committed
            .last()
            .map_or(command.expected_stream_version, |event| {
                event.stream_sequence
            });
        let committed_offset = committed
            .last()
            .map_or(latest_global_offset_in_transaction(&transaction), |event| {
                Ok(event.global_offset)
            })?;
        let outcome = CommandOutcome::Accepted {
            command_id: command.command_id,
            task_id: command.task_id,
            stream_version,
            committed_offset,
        };
        finish_command(
            &transaction,
            command.command_id,
            "accepted",
            &outcome,
            stream_version,
            committed_offset,
            committed.len(),
        )?;
        transaction.commit()?;
        Ok(outcome)
    }

    pub fn stream_version(&self, task_id: TaskId) -> Result<u64, TaskStoreError> {
        let connection = self.lock()?;
        let version: i64 = connection.query_row(
            "SELECT COALESCE(MAX(stream_sequence), 0) FROM event_log WHERE task_id = ?1",
            [task_id.to_string()],
            |row| row.get(0),
        )?;
        nonnegative_integer(version)
    }

    pub fn acquire_workspace_lease(
        &self,
        request: AcquireTaskWorkspaceLease,
    ) -> Result<TaskWorkspaceAcquireOutcome, TaskStoreError> {
        if request.canonical_root.trim().is_empty() {
            return Err(TaskStoreError::InvalidInput(
                "workspace canonical root must not be empty".into(),
            ));
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let actual = stream_version_in_transaction(&transaction, request.task_id)?;
        if actual == 0 {
            return Err(TaskStoreError::InvalidInput(format!(
                "workspace lease task {} does not exist",
                request.task_id
            )));
        }

        let has_conflict = if request.mode == WorkspaceMode::Current {
            transaction.query_row(
                "SELECT
                    EXISTS(
                        SELECT 1 FROM workspace_leases
                        WHERE canonical_root = ?1
                          AND mode = 'current'
                          AND state = 'active'
                          AND (?2 = 1 OR writable = 1)
                    )
                    OR (
                        ?2 = 0 AND EXISTS(
                            SELECT 1 FROM workspace_leases
                            WHERE canonical_root = ?1
                              AND mode = 'current'
                              AND state = 'waiting'
                              AND writable = 1
                        )
                    )",
                params![request.canonical_root, i64::from(request.writable)],
                |row| row.get::<_, i64>(0),
            )? != 0
        } else {
            false
        };
        let state = if has_conflict {
            TaskWorkspaceLeaseState::Waiting
        } else {
            TaskWorkspaceLeaseState::Active
        };
        let acquired_at =
            (state == TaskWorkspaceLeaseState::Active).then_some(request.requested_at);
        let lease = TaskWorkspaceLease {
            lease_id: request.lease_id,
            task_id: request.task_id,
            actor_id: request.actor_id,
            mode: request.mode,
            canonical_root: request.canonical_root,
            worktree_path: request.worktree_path,
            branch: request.branch,
            writable: request.writable,
            state,
            requested_at: request.requested_at,
            acquired_at,
            expires_at: request.expires_at,
            baseline_commit: request.baseline_commit,
            baseline_status: request.baseline_status,
            patch_path: None,
        };
        let lease_json = serde_json::to_string(&lease)?;
        transaction.execute(
            "INSERT INTO workspace_leases (
                workspace_lease_id, task_id, canonical_root, mode, writable, state,
                acquired_at, expires_at, lease_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                lease.lease_id.to_string(),
                lease.task_id.to_string(),
                lease.canonical_root,
                workspace_mode_wire(&lease.mode),
                i64::from(lease.writable),
                workspace_lease_state_wire(lease.state),
                lease.acquired_at.map(|value| value.to_rfc3339()),
                lease.expires_at.map(|value| value.to_rfc3339()),
                lease_json,
            ],
        )?;

        let projection = workspace_lease_projection(&lease);
        let event = if state == TaskWorkspaceLeaseState::Active {
            NewTaskEvent::workspace_acquired(projection)
        } else {
            NewTaskEvent::workspace_waiting(projection)
        };
        let committed = append_in_transaction(
            &transaction,
            lease.task_id,
            actual,
            TaskStore::supervisor_authority().source(),
            vec![event],
        )?;
        for event in &committed {
            self.projector.apply(&transaction, event)?;
        }
        transaction.commit()?;
        Ok(if state == TaskWorkspaceLeaseState::Active {
            TaskWorkspaceAcquireOutcome::Acquired(lease)
        } else {
            TaskWorkspaceAcquireOutcome::Waiting(lease)
        })
    }

    pub fn prepare_managed_workspace_lease(
        &self,
        request: AcquireTaskWorkspaceLease,
    ) -> Result<TaskWorkspaceLease, TaskStoreError> {
        if request.mode != WorkspaceMode::ManagedWorktree {
            return Err(TaskStoreError::InvalidInput(
                "only managed worktrees use the preparing transition".into(),
            ));
        }
        if request.canonical_root.trim().is_empty() || request.worktree_path.is_none() {
            return Err(TaskStoreError::InvalidInput(
                "managed workspace preparation requires root and worktree path".into(),
            ));
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if stream_version_in_transaction(&transaction, request.task_id)? == 0 {
            return Err(TaskStoreError::InvalidInput(format!(
                "workspace lease task {} does not exist",
                request.task_id
            )));
        }
        let lease = TaskWorkspaceLease {
            lease_id: request.lease_id,
            task_id: request.task_id,
            actor_id: request.actor_id,
            mode: request.mode,
            canonical_root: request.canonical_root,
            worktree_path: request.worktree_path,
            branch: request.branch,
            writable: request.writable,
            state: TaskWorkspaceLeaseState::Preparing,
            requested_at: request.requested_at,
            acquired_at: None,
            expires_at: request.expires_at,
            baseline_commit: request.baseline_commit,
            baseline_status: request.baseline_status,
            patch_path: None,
        };
        transaction.execute(
            "INSERT INTO workspace_leases (
                workspace_lease_id, task_id, canonical_root, mode, writable, state,
                acquired_at, expires_at, lease_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8)",
            params![
                lease.lease_id.to_string(),
                lease.task_id.to_string(),
                lease.canonical_root,
                workspace_mode_wire(&lease.mode),
                i64::from(lease.writable),
                workspace_lease_state_wire(lease.state),
                lease.expires_at.map(|value| value.to_rfc3339()),
                serde_json::to_string(&lease)?,
            ],
        )?;
        append_and_project_workspace_events(
            &transaction,
            self.projector.as_ref(),
            lease.task_id,
            vec![NewTaskEvent::workspace_preparing(
                workspace_lease_projection(&lease),
            )],
        )?;
        transaction.commit()?;
        Ok(lease)
    }

    pub fn activate_managed_workspace_lease(
        &self,
        lease_id: WorkspaceLeaseId,
    ) -> Result<TaskWorkspaceLease, TaskStoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut lease = workspace_lease_in_transaction(&transaction, lease_id)?;
        if lease.state == TaskWorkspaceLeaseState::Active {
            return Ok(lease);
        }
        if lease.state != TaskWorkspaceLeaseState::Preparing {
            return Err(TaskStoreError::InvalidInput(format!(
                "workspace lease {lease_id} is not preparing"
            )));
        }
        lease.state = TaskWorkspaceLeaseState::Active;
        lease.acquired_at = Some(now());
        update_workspace_lease_in_transaction(&transaction, &lease)?;
        append_and_project_workspace_events(
            &transaction,
            self.projector.as_ref(),
            lease.task_id,
            vec![NewTaskEvent::workspace_acquired(
                workspace_lease_projection(&lease),
            )],
        )?;
        transaction.commit()?;
        Ok(lease)
    }

    pub fn mark_workspace_cleanup_pending(
        &self,
        lease_id: WorkspaceLeaseId,
    ) -> Result<TaskWorkspaceLease, TaskStoreError> {
        let dispatches = self
            .workspace_dispatches
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)?;
        if dispatches.get(&lease_id).copied().unwrap_or_default() > 0 {
            return Err(TaskStoreError::InvalidInput(format!(
                "workspace lease {lease_id} has an in-flight dispatch"
            )));
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut lease = workspace_lease_in_transaction(&transaction, lease_id)?;
        if lease.state == TaskWorkspaceLeaseState::CleanupPending {
            return Ok(lease);
        }
        if !matches!(
            lease.state,
            TaskWorkspaceLeaseState::Active
                | TaskWorkspaceLeaseState::Preparing
                | TaskWorkspaceLeaseState::Expired
                | TaskWorkspaceLeaseState::CleanupBlocked
        ) || lease.mode != WorkspaceMode::ManagedWorktree
        {
            return Err(TaskStoreError::InvalidInput(format!(
                "workspace lease {lease_id} cannot enter cleanup pending"
            )));
        }
        lease.state = TaskWorkspaceLeaseState::CleanupPending;
        update_workspace_lease_in_transaction(&transaction, &lease)?;
        append_and_project_workspace_events(
            &transaction,
            self.projector.as_ref(),
            lease.task_id,
            vec![NewTaskEvent::workspace_cleanup_pending(
                workspace_lease_projection(&lease),
            )],
        )?;
        transaction.commit()?;
        drop(dispatches);
        Ok(lease)
    }

    pub fn recoverable_managed_workspace_leases(
        &self,
    ) -> Result<Vec<TaskWorkspaceLease>, TaskStoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT lease_json FROM workspace_leases
             WHERE mode = 'managed_worktree'
               AND state IN ('preparing', 'expired', 'cleanup_pending')
             ORDER BY rowid ASC",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let json = row?;
            serde_json::from_str(&json).map_err(TaskStoreError::from)
        })
        .collect()
    }

    pub fn nonterminal_workspace_leases_for_task(
        &self,
        task_id: TaskId,
    ) -> Result<Vec<TaskWorkspaceLease>, TaskStoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT lease_json FROM workspace_leases
             WHERE task_id = ?1
               AND state IN ('preparing', 'waiting', 'active', 'cleanup_pending', 'cleanup_blocked')
             ORDER BY rowid ASC",
        )?;
        let rows = statement.query_map([task_id.to_string()], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let json = row?;
            serde_json::from_str(&json).map_err(TaskStoreError::from)
        })
        .collect()
    }

    pub fn active_workspace_leases(
        &self,
        canonical_root: &str,
    ) -> Result<Vec<TaskWorkspaceLease>, TaskStoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT lease_json FROM workspace_leases
             WHERE canonical_root = ?1 AND state = 'active'
             ORDER BY acquired_at ASC, workspace_lease_id ASC",
        )?;
        let rows = statement.query_map([canonical_root], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let json = row?;
            serde_json::from_str(&json).map_err(TaskStoreError::from)
        })
        .collect()
    }

    pub fn workspace_lease(
        &self,
        lease_id: WorkspaceLeaseId,
    ) -> Result<Option<TaskWorkspaceLease>, TaskStoreError> {
        let connection = self.lock()?;
        let lease_json = connection
            .query_row(
                "SELECT lease_json FROM workspace_leases WHERE workspace_lease_id = ?1",
                [lease_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        lease_json
            .map(|json| serde_json::from_str(&json).map_err(TaskStoreError::from))
            .transpose()
    }

    pub fn begin_workspace_dispatch(
        &self,
        lease_id: WorkspaceLeaseId,
    ) -> Result<WorkspaceDispatchGuard<'_>, TaskStoreError> {
        let mut dispatches = self
            .workspace_dispatches
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)?;
        let lease = self.workspace_lease(lease_id)?.ok_or_else(|| {
            TaskStoreError::InvalidInput(format!("workspace lease {lease_id} does not exist"))
        })?;
        if lease.state != TaskWorkspaceLeaseState::Active {
            return Err(TaskStoreError::InvalidInput(format!(
                "workspace lease {lease_id} is not active"
            )));
        }
        *dispatches.entry(lease_id).or_default() += 1;
        Ok(WorkspaceDispatchGuard {
            store: self,
            lease_id,
        })
    }

    pub fn mark_workspace_cleanup_blocked(
        &self,
        lease_id: WorkspaceLeaseId,
        patch_path: &str,
    ) -> Result<TaskWorkspaceLease, TaskStoreError> {
        if patch_path.trim().is_empty() {
            return Err(TaskStoreError::InvalidInput(
                "workspace cleanup patch path must not be empty".into(),
            ));
        }
        let dispatches = self
            .workspace_dispatches
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)?;
        if dispatches.get(&lease_id).copied().unwrap_or_default() > 0 {
            return Err(TaskStoreError::InvalidInput(format!(
                "workspace lease {lease_id} has an in-flight dispatch"
            )));
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let lease_json = transaction
            .query_row(
                "SELECT lease_json FROM workspace_leases WHERE workspace_lease_id = ?1",
                [lease_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| {
                TaskStoreError::InvalidInput(format!("workspace lease {lease_id} does not exist"))
            })?;
        let mut lease: TaskWorkspaceLease = serde_json::from_str(&lease_json)?;
        if lease.state == TaskWorkspaceLeaseState::CleanupBlocked
            && lease.mode == WorkspaceMode::ManagedWorktree
        {
            return Ok(lease);
        }
        if !matches!(
            lease.state,
            TaskWorkspaceLeaseState::Active | TaskWorkspaceLeaseState::CleanupPending
        ) || lease.mode != WorkspaceMode::ManagedWorktree
        {
            return Err(TaskStoreError::InvalidInput(format!(
                "workspace lease {lease_id} is not an active managed worktree"
            )));
        }
        lease.state = TaskWorkspaceLeaseState::CleanupBlocked;
        lease.patch_path = Some(patch_path.to_owned());
        update_workspace_lease_in_transaction(&transaction, &lease)?;
        append_and_project_workspace_events(
            &transaction,
            self.projector.as_ref(),
            lease.task_id,
            vec![NewTaskEvent::workspace_cleanup_blocked(
                workspace_lease_projection(&lease),
                now(),
            )],
        )?;
        transaction.commit()?;
        drop(dispatches);
        Ok(lease)
    }

    pub fn release_workspace_lease(
        &self,
        lease_id: WorkspaceLeaseId,
        reason: &str,
    ) -> Result<ReleaseTaskWorkspaceLeaseOutcome, TaskStoreError> {
        self.transition_workspace_lease(lease_id, reason, TaskWorkspaceLeaseState::Released)
    }

    pub fn expire_workspace_leases(
        &self,
        at: DateTime<Utc>,
    ) -> Result<Vec<ReleaseTaskWorkspaceLeaseOutcome>, TaskStoreError> {
        let expired_ids = {
            let connection = self.lock()?;
            let mut statement = connection.prepare(
                "SELECT lease_json FROM workspace_leases
                 WHERE state IN ('active', 'waiting', 'preparing') AND expires_at IS NOT NULL
                 ORDER BY expires_at ASC, rowid ASC",
            )?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            let mut expired_ids = Vec::new();
            for row in rows {
                let lease: TaskWorkspaceLease = serde_json::from_str(&row?)?;
                if lease.expires_at.is_some_and(|expires_at| expires_at <= at) {
                    expired_ids.push(lease.lease_id);
                }
            }
            expired_ids
        };
        let mut outcomes = Vec::new();
        for lease_id in expired_ids {
            let Some(lease) = self.workspace_lease(lease_id)? else {
                continue;
            };
            if matches!(
                lease.state,
                TaskWorkspaceLeaseState::Active
                    | TaskWorkspaceLeaseState::Waiting
                    | TaskWorkspaceLeaseState::Preparing
            ) && lease.expires_at.is_some_and(|expires_at| expires_at <= at)
            {
                match self.transition_workspace_lease(
                    lease_id,
                    "owner_expired",
                    TaskWorkspaceLeaseState::Expired,
                ) {
                    Ok(outcome) => outcomes.push(outcome),
                    Err(TaskStoreError::InvalidInput(message))
                        if message.contains("in-flight dispatch") => {}
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(outcomes)
    }

    fn transition_workspace_lease(
        &self,
        lease_id: WorkspaceLeaseId,
        reason: &str,
        terminal_state: TaskWorkspaceLeaseState,
    ) -> Result<ReleaseTaskWorkspaceLeaseOutcome, TaskStoreError> {
        if reason.trim().is_empty() {
            return Err(TaskStoreError::InvalidInput(
                "workspace release reason must not be empty".into(),
            ));
        }
        let dispatches = self
            .workspace_dispatches
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)?;
        if dispatches.get(&lease_id).copied().unwrap_or_default() > 0 {
            return Err(TaskStoreError::InvalidInput(format!(
                "workspace lease {lease_id} has an in-flight dispatch"
            )));
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let lease_json = transaction
            .query_row(
                "SELECT lease_json FROM workspace_leases WHERE workspace_lease_id = ?1",
                [lease_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| {
                TaskStoreError::InvalidInput(format!("workspace lease {lease_id} does not exist"))
            })?;
        let mut released: TaskWorkspaceLease = serde_json::from_str(&lease_json)?;
        if !matches!(
            released.state,
            TaskWorkspaceLeaseState::Active
                | TaskWorkspaceLeaseState::Waiting
                | TaskWorkspaceLeaseState::Preparing
                | TaskWorkspaceLeaseState::CleanupPending
        ) {
            return Err(TaskStoreError::InvalidInput(format!(
                "workspace lease {lease_id} is not active"
            )));
        }
        if !matches!(
            terminal_state,
            TaskWorkspaceLeaseState::Released | TaskWorkspaceLeaseState::Expired
        ) {
            return Err(TaskStoreError::InvalidInput(
                "workspace terminal transition must release or expire the lease".into(),
            ));
        }
        released.state = terminal_state;
        update_workspace_lease_in_transaction(&transaction, &released)?;
        let released_at = now();
        append_and_project_workspace_events(
            &transaction,
            self.projector.as_ref(),
            released.task_id,
            vec![NewTaskEvent::workspace_released(
                workspace_lease_projection(&released),
                reason,
                released_at,
            )],
        )?;

        let mut waiters = transaction.prepare(
            "SELECT lease_json FROM workspace_leases
             WHERE canonical_root = ?1 AND mode = 'current' AND state = 'waiting'
             ORDER BY rowid ASC",
        )?;
        let waiting_json = waiters
            .query_map([released.canonical_root.as_str()], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(waiters);

        let mut acquired = Vec::new();
        for json in waiting_json {
            let mut waiting: TaskWorkspaceLease = serde_json::from_str(&json)?;
            if waiting
                .expires_at
                .is_some_and(|expires_at| expires_at <= released_at)
            {
                waiting.state = TaskWorkspaceLeaseState::Expired;
                update_workspace_lease_in_transaction(&transaction, &waiting)?;
                append_and_project_workspace_events(
                    &transaction,
                    self.projector.as_ref(),
                    waiting.task_id,
                    vec![NewTaskEvent::workspace_released(
                        workspace_lease_projection(&waiting),
                        "owner_expired",
                        released_at,
                    )],
                )?;
                continue;
            }
            let active =
                active_workspace_leases_in_transaction(&transaction, &waiting.canonical_root)?;
            let blocked = if waiting.writable {
                !active.is_empty()
            } else {
                active.iter().any(|lease| lease.writable)
            };
            if blocked {
                break;
            }
            waiting.state = TaskWorkspaceLeaseState::Active;
            waiting.acquired_at = Some(released_at);
            update_workspace_lease_in_transaction(&transaction, &waiting)?;
            append_and_project_workspace_events(
                &transaction,
                self.projector.as_ref(),
                waiting.task_id,
                vec![NewTaskEvent::workspace_acquired(
                    workspace_lease_projection(&waiting),
                )],
            )?;
            acquired.push(waiting.clone());
            if waiting.writable {
                break;
            }
        }
        transaction.commit()?;
        drop(dispatches);
        Ok(ReleaseTaskWorkspaceLeaseOutcome { released, acquired })
    }

    pub fn pending_segment_start(
        &self,
        task_id: TaskId,
        segment_id: RunSegmentId,
    ) -> Result<Option<PendingSegmentStart>, TaskStoreError> {
        let connection = self.lock()?;
        let event_payload = connection
            .query_row(
                "SELECT payload_json
                 FROM event_log
                 WHERE task_id = ?1
                   AND event_type = 'run.started'
                   AND json_extract(payload_json, '$.segmentId') = ?2",
                params![task_id.to_string(), segment_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| {
                TaskStoreError::ProjectionIntegrity(
                    "segment start delivery has no canonical run start".into(),
                )
            })?;
        let event_payload: Value = serde_json::from_str(&event_payload).map_err(|error| {
            TaskStoreError::ProjectionIntegrity(format!(
                "canonical run start payload is invalid: {error}"
            ))
        })?;
        if event_payload.get("recoveryStart").and_then(Value::as_bool) != Some(true) {
            return Ok(None);
        }
        let canonical_decisions = serde_json::from_value::<Vec<IndeterminateToolDecision>>(
            event_payload
                .get("indeterminateTools")
                .cloned()
                .unwrap_or_else(|| json!([])),
        )
        .map_err(|error| {
            TaskStoreError::ProjectionIntegrity(format!(
                "canonical run recovery decisions are invalid: {error}"
            ))
        })?;
        let (body, delivered_at) = connection
            .query_row(
                "SELECT request_json, delivered_at
                 FROM segment_start_outbox
                 WHERE task_id = ?1 AND run_segment_id = ?2",
                params![task_id.to_string(), segment_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .optional()?
            .ok_or_else(|| {
                TaskStoreError::ProjectionIntegrity(
                    "canonical recovery run start has no delivery outbox entry".into(),
                )
            })?;
        if body.len() > MAX_COMMAND_PAYLOAD_BYTES {
            return Err(TaskStoreError::ProjectionIntegrity(
                "pending segment start exceeds 1 MiB".into(),
            ));
        }
        let request: PendingSegmentStart = serde_json::from_str(&body).map_err(|error| {
            TaskStoreError::ProjectionIntegrity(format!(
                "pending segment start payload is invalid: {error}"
            ))
        })?;
        if request.task_id != task_id || request.segment_id != segment_id {
            return Err(TaskStoreError::ProjectionIntegrity(
                "pending segment start columns disagree with its payload".into(),
            ));
        }
        if request.indeterminate_tools != canonical_decisions {
            return Err(TaskStoreError::ProjectionIntegrity(
                "pending segment start disagrees with the canonical run start".into(),
            ));
        }
        Ok(delivered_at.is_none().then_some(request))
    }

    pub fn mark_segment_start_delivered(
        &self,
        task_id: TaskId,
        segment_id: RunSegmentId,
    ) -> Result<(), TaskStoreError> {
        let connection = self.lock()?;
        let changed = connection.execute(
            "UPDATE segment_start_outbox
             SET delivered_at = ?3
             WHERE task_id = ?1 AND run_segment_id = ?2 AND delivered_at IS NULL",
            params![
                task_id.to_string(),
                segment_id.to_string(),
                now().to_rfc3339()
            ],
        )?;
        if changed != 1 {
            return Err(TaskStoreError::ProjectionIntegrity(
                "pending segment start is missing or already delivered".into(),
            ));
        }
        Ok(())
    }

    pub fn events_after(
        &self,
        after_global_offset: u64,
        limit: usize,
    ) -> Result<Vec<TaskEventEnvelope>, TaskStoreError> {
        let after = i64::try_from(after_global_offset).unwrap_or(i64::MAX);
        let limit = i64::try_from(limit.clamp(1, MAX_READ_PAGE_SIZE))
            .map_err(|_| TaskStoreError::IntegerOutOfRange)?;
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT global_offset, task_id, stream_sequence, event_id, event_type,
                    schema_version, recorded_at, source_json, payload_json
             FROM event_log
             WHERE global_offset > ?1
             ORDER BY global_offset ASC
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![after, limit], |row| {
            Ok(StoredTaskEvent {
                global_offset: row.get(0)?,
                task_id: row.get(1)?,
                stream_sequence: row.get(2)?,
                event_id: row.get(3)?,
                event_type: row.get(4)?,
                schema_version: row.get(5)?,
                recorded_at: row.get(6)?,
                source_json: row.get(7)?,
                payload_json: row.get(8)?,
            })
        })?;

        rows.map(|row| {
            row.map_err(TaskStoreError::from)
                .and_then(StoredTaskEvent::decode)
        })
        .collect()
    }

    pub fn task_events_after(
        &self,
        task_id: TaskId,
        after_stream_sequence: u64,
        limit: usize,
    ) -> Result<Vec<TaskEventEnvelope>, TaskStoreError> {
        let after = i64::try_from(after_stream_sequence).unwrap_or(i64::MAX);
        let limit = i64::try_from(limit.clamp(1, MAX_READ_PAGE_SIZE))
            .map_err(|_| TaskStoreError::IntegerOutOfRange)?;
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT global_offset, task_id, stream_sequence, event_id, event_type,
                    schema_version, recorded_at, source_json, payload_json
             FROM event_log
             WHERE task_id = ?1 AND stream_sequence > ?2
             ORDER BY stream_sequence ASC
             LIMIT ?3",
        )?;
        let rows = statement.query_map(params![task_id.to_string(), after, limit], |row| {
            Ok(StoredTaskEvent {
                global_offset: row.get(0)?,
                task_id: row.get(1)?,
                stream_sequence: row.get(2)?,
                event_id: row.get(3)?,
                event_type: row.get(4)?,
                schema_version: row.get(5)?,
                recorded_at: row.get(6)?,
                source_json: row.get(7)?,
                payload_json: row.get(8)?,
            })
        })?;

        rows.map(|row| {
            row.map_err(TaskStoreError::from)
                .and_then(StoredTaskEvent::decode)
        })
        .collect()
    }

    pub fn task_events_after_global_offset(
        &self,
        task_id: TaskId,
        after_global_offset: u64,
        limit: usize,
    ) -> Result<Vec<TaskEventEnvelope>, TaskStoreError> {
        let after = i64::try_from(after_global_offset).unwrap_or(i64::MAX);
        let limit = i64::try_from(limit.clamp(1, MAX_READ_PAGE_SIZE))
            .map_err(|_| TaskStoreError::IntegerOutOfRange)?;
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT global_offset, task_id, stream_sequence, event_id, event_type,
                    schema_version, recorded_at, source_json, payload_json
             FROM event_log
             WHERE task_id = ?1 AND global_offset > ?2
             ORDER BY global_offset ASC
             LIMIT ?3",
        )?;
        let rows = statement.query_map(params![task_id.to_string(), after, limit], |row| {
            Ok(StoredTaskEvent {
                global_offset: row.get(0)?,
                task_id: row.get(1)?,
                stream_sequence: row.get(2)?,
                event_id: row.get(3)?,
                event_type: row.get(4)?,
                schema_version: row.get(5)?,
                recorded_at: row.get(6)?,
                source_json: row.get(7)?,
                payload_json: row.get(8)?,
            })
        })?;
        rows.map(|row| {
            row.map_err(TaskStoreError::from)
                .and_then(StoredTaskEvent::decode)
        })
        .collect()
    }

    pub fn run_started_global_offset(
        &self,
        task_id: TaskId,
        segment_id: RunSegmentId,
    ) -> Result<Option<u64>, TaskStoreError> {
        let connection = self.lock()?;
        connection
            .query_row(
                "SELECT global_offset
                 FROM event_log
                 WHERE task_id = ?1
                   AND event_type = 'run.started'
                   AND json_extract(payload_json, '$.segmentId') = ?2
                 ORDER BY global_offset DESC
                 LIMIT 1",
                params![task_id.to_string(), segment_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(nonnegative_integer)
            .transpose()
    }

    pub fn latest_global_offset(&self) -> Result<u64, TaskStoreError> {
        let connection = self.lock()?;
        let offset: i64 = connection.query_row(
            "SELECT COALESCE(MAX(global_offset), 0) FROM event_log",
            [],
            |row| row.get(0),
        )?;
        nonnegative_integer(offset)
    }

    pub fn save_checkpoint(&self, checkpoint: &TaskCheckpoint) -> Result<(), TaskStoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_checkpoint_in_transaction(&transaction, checkpoint)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn latest_checkpoint(
        &self,
        task_id: TaskId,
    ) -> Result<Option<TaskCheckpoint>, TaskStoreError> {
        let connection = self.lock()?;
        load_latest_checkpoint(&connection, task_id)
    }

    pub fn activate_context_summary(&self, summary: &ContextSummary) -> Result<(), TaskStoreError> {
        if summary.source_start_global_offset == 0
            || summary.source_end_global_offset < summary.source_start_global_offset
        {
            return Err(TaskStoreError::InvalidInput(
                "context summary source range is invalid".into(),
            ));
        }
        let body = serde_json::to_string(summary)?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task_offset = transaction
            .query_row(
                "SELECT last_global_offset FROM task_projection WHERE task_id = ?1",
                [summary.task_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or_else(|| {
                TaskStoreError::InvalidInput("context summary task does not exist".into())
            })?;
        if summary.source_end_global_offset > nonnegative_integer(task_offset)? {
            return Err(TaskStoreError::InvalidInput(
                "context summary source range exceeds the task's committed events".into(),
            ));
        }
        let boundary_count = transaction.query_row(
            "SELECT COUNT(*) FROM event_log
             WHERE task_id = ?1 AND global_offset IN (?2, ?3)",
            params![
                summary.task_id.to_string(),
                sqlite_integer(summary.source_start_global_offset)?,
                sqlite_integer(summary.source_end_global_offset)?,
            ],
            |row| row.get::<_, i64>(0),
        )?;
        let expected_boundaries =
            if summary.source_start_global_offset == summary.source_end_global_offset {
                1
            } else {
                2
            };
        if boundary_count != expected_boundaries {
            return Err(TaskStoreError::InvalidInput(
                "context summary source boundaries do not belong to the task".into(),
            ));
        }
        promote_blob_id_in_transaction(&transaction, summary.task_id, summary.blob_id)?;
        validate_blob_reference_in_transaction(
            &transaction,
            summary.task_id,
            &TaskBlobReference {
                blob_id: summary.blob_id,
                expected: None,
            },
        )?;
        let media_type = transaction.query_row(
            "SELECT media_type FROM blob_ownership WHERE task_id = ?1 AND blob_id = ?2",
            params![summary.task_id.to_string(), summary.blob_id.to_string()],
            |row| row.get::<_, String>(0),
        )?;
        if media_type != CONTEXT_SUMMARY_MEDIA_TYPE {
            return Err(TaskStoreError::InvalidInput(format!(
                "context summary blob must use {CONTEXT_SUMMARY_MEDIA_TYPE}"
            )));
        }
        transaction.execute(
            "UPDATE context_summaries SET active = 0 WHERE task_id = ?1 AND active = 1",
            [summary.task_id.to_string()],
        )?;
        transaction.execute(
            "INSERT INTO context_summaries (
                summary_id, task_id, source_start_global_offset, source_end_global_offset,
                blob_id, active, summary_json, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7)",
            params![
                summary.summary_id.to_string(),
                summary.task_id.to_string(),
                sqlite_integer(summary.source_start_global_offset)?,
                sqlite_integer(summary.source_end_global_offset)?,
                summary.blob_id.to_string(),
                body,
                summary.created_at.to_rfc3339(),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn active_context_summary(
        &self,
        task_id: TaskId,
    ) -> Result<Option<ContextSummary>, TaskStoreError> {
        let connection = self.lock()?;
        let row = connection
            .query_row(
                "SELECT summary_id, source_start_global_offset, source_end_global_offset,
                        blob_id, summary_json
                 FROM context_summaries
                 WHERE task_id = ?1 AND active = 1",
                [task_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(stored_id, stored_start, stored_end, stored_blob_id, body)| {
                let summary: ContextSummary = serde_json::from_str(&body)?;
                if summary.task_id != task_id
                    || summary.summary_id.to_string() != stored_id
                    || summary.source_start_global_offset != nonnegative_integer(stored_start)?
                    || summary.source_end_global_offset != nonnegative_integer(stored_end)?
                    || summary.blob_id.to_string() != stored_blob_id
                {
                    return Err(TaskStoreError::ProjectionIntegrity(
                        "context summary columns disagree with summary payload".into(),
                    ));
                }
                Ok(summary)
            },
        )
        .transpose()
    }

    pub fn task_projection(
        &self,
        task_id: TaskId,
    ) -> Result<Option<TaskProjection>, TaskStoreError> {
        Ok(self
            .task_projection_snapshot(task_id)?
            .map(|(projection, _, _)| projection))
    }

    pub fn task_projection_snapshot(
        &self,
        task_id: TaskId,
    ) -> Result<Option<(TaskProjection, u64, Vec<TimelineItemProjection>)>, TaskStoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let actual = stream_version_in_transaction(&transaction, task_id)?;
        let projection = projection_for_decision(&transaction, task_id, actual)?;
        let snapshot_offset = latest_global_offset_in_transaction(&transaction)?;
        let timeline = {
            let mut statement = transaction.prepare(
                "SELECT global_offset, projection_json
                 FROM timeline_projection
                 WHERE task_id = ?1
                 ORDER BY global_offset ASC",
            )?;
            let rows = statement.query_map(params![task_id.to_string()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.map(|row| {
                let (stored_offset, body) = row?;
                let item: TimelineItemProjection = serde_json::from_str(&body)?;
                if item.global_offset != nonnegative_integer(stored_offset)? {
                    return Err(TaskStoreError::ProjectionIntegrity(
                        "timeline projection offset disagrees with its row".into(),
                    ));
                }
                Ok(item)
            })
            .collect::<Result<Vec<_>, TaskStoreError>>()?
        };
        transaction.commit()?;
        Ok((actual > 0).then_some((projection, snapshot_offset, timeline)))
    }

    pub fn task_projections(&self) -> Result<Vec<TaskProjection>, TaskStoreError> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare("SELECT task_id, projection_json FROM task_projection ORDER BY task_id ASC")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (stored_task_id, body) = row?;
            let projection: TaskProjection = serde_json::from_str(&body)?;
            if projection.task_id.to_string() != stored_task_id {
                return Err(TaskStoreError::ProjectionIntegrity(format!(
                    "task projection {stored_task_id} has another identity"
                )));
            }
            Ok(projection)
        })
        .collect()
    }

    pub fn nonterminal_task_projections_after(
        &self,
        after_task_id: Option<TaskId>,
        limit: usize,
    ) -> Result<Vec<TaskProjection>, TaskStoreError> {
        let after_task_id = after_task_id.map(|task_id| task_id.to_string());
        let limit = i64::try_from(limit.clamp(1, MAX_READ_PAGE_SIZE))
            .map_err(|_| TaskStoreError::IntegerOutOfRange)?;
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT task_id, projection_json
             FROM task_projection
             WHERE (?1 IS NULL OR task_id > ?1)
               AND json_extract(projection_json, '$.state') IN (
                   'running', 'waiting_permission', 'yielding', 'interrupted'
               )
             ORDER BY task_id ASC
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![after_task_id, limit], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (stored_task_id, body) = row?;
            let projection: TaskProjection = serde_json::from_str(&body)?;
            if projection.task_id.to_string() != stored_task_id {
                return Err(TaskStoreError::ProjectionIntegrity(format!(
                    "task projection {stored_task_id} has another identity"
                )));
            }
            Ok(projection)
        })
        .collect()
    }

    pub fn queue_item_projection(
        &self,
        task_id: TaskId,
        queue_item_id: harness_contracts::QueueItemId,
    ) -> Result<Option<QueueItemProjection>, TaskStoreError> {
        let connection = self.lock()?;
        let body = connection
            .query_row(
                "SELECT projection_json FROM queue_projection
                 WHERE task_id = ?1 AND queue_item_id = ?2",
                params![task_id.to_string(), queue_item_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        body.map(|body| {
            let projection: QueueItemProjection = serde_json::from_str(&body)?;
            if projection.queue_item_id != queue_item_id {
                return Err(TaskStoreError::ProjectionIntegrity(format!(
                    "queue item {queue_item_id} projection has another identity"
                )));
            }
            Ok(projection)
        })
        .transpose()
    }

    pub fn queue_item_for_segment(
        &self,
        task_id: TaskId,
        segment_id: RunSegmentId,
    ) -> Result<Option<SegmentRunInput>, TaskStoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT queue_item_id, projection_json
             FROM queue_projection
             WHERE task_id = ?1
               AND json_extract(projection_json, '$.consumedBy') = ?2
             ORDER BY queue_item_id ASC
             LIMIT 2",
        )?;
        let mut rows = statement.query(params![task_id.to_string(), segment_id.to_string()])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let stored_queue_item_id = row.get::<_, String>(0)?;
        let projection_json = row.get::<_, String>(1)?;
        if rows.next()?.is_some() {
            return Err(TaskStoreError::ProjectionIntegrity(format!(
                "run segment {segment_id} consumed more than one queue item"
            )));
        }
        let projection: QueueItemProjection = serde_json::from_str(&projection_json)?;
        if projection.queue_item_id.to_string() != stored_queue_item_id
            || projection.consumed_by != Some(segment_id)
        {
            return Err(TaskStoreError::ProjectionIntegrity(format!(
                "run segment {segment_id} queue projection identity is inconsistent"
            )));
        }
        let payload_json = connection
            .query_row(
                "SELECT payload_json
                 FROM event_log
                 WHERE task_id = ?1
                   AND event_type = 'message.queued'
                   AND json_extract(payload_json, '$.queueItemId') = ?2
                 ORDER BY stream_sequence ASC
                 LIMIT 1",
                params![task_id.to_string(), stored_queue_item_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| {
                TaskStoreError::ProjectionIntegrity(format!(
                    "queue item {} has no canonical message.queued event",
                    projection.queue_item_id
                ))
            })?;
        let payload: Value = serde_json::from_str(&payload_json)?;
        let model_config_id = payload
            .get("modelConfigId")
            .filter(|value| !value.is_null())
            .map(|value| serde_json::from_value::<String>(value.clone()))
            .transpose()?;
        let permission_mode = payload
            .get("permissionMode")
            .cloned()
            .map(serde_json::from_value::<PermissionMode>)
            .transpose()?
            .unwrap_or_default();
        Ok(Some(SegmentRunInput {
            queue_item_id: projection.queue_item_id,
            content: projection.content,
            attachments: projection.attachments,
            context_references: projection.context_references,
            model_config_id,
            permission_mode,
        }))
    }

    pub fn latest_queue_revision(&self, task_id: TaskId) -> Result<u64, TaskStoreError> {
        let connection = self.lock()?;
        let revision = connection.query_row(
            "SELECT COALESCE(MAX(CAST(json_extract(projection_json, '$.revision') AS INTEGER)), 0)
             FROM queue_projection WHERE task_id = ?1",
            [task_id.to_string()],
            |row| row.get::<_, i64>(0),
        )?;
        nonnegative_integer(revision)
    }

    pub fn projection_counts(&self) -> Result<ProjectionCounts, TaskStoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let counts = projection_counts(&transaction)?;
        transaction.commit()?;
        Ok(counts)
    }

    pub fn rebuild_projections(&self) -> Result<(), TaskStoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let before = projection_snapshot(&transaction)?;
        for table in PROJECTION_TABLES {
            transaction.execute(&format!("DELETE FROM {table}"), [])?;
        }
        let mut after_offset = 0;
        let mut previous_global_offset = None;
        let mut task_versions = HashMap::<TaskId, u64>::new();
        loop {
            let events = load_events_page(&transaction, after_offset, REBUILD_PAGE_SIZE)?;
            if events.is_empty() {
                break;
            }
            for event in &events {
                if previous_global_offset.is_some_and(|previous| event.global_offset <= previous) {
                    return Err(TaskStoreError::ProjectionIntegrity(
                        "event log global offsets are not strictly increasing".into(),
                    ));
                }
                let expected = task_versions.get(&event.task_id).copied().unwrap_or(0) + 1;
                if event.stream_sequence != expected {
                    return Err(TaskStoreError::ProjectionIntegrity(format!(
                        "task {} stream sequence is {}, expected {expected}",
                        event.task_id, event.stream_sequence
                    )));
                }
                self.projector.apply(&transaction, event)?;
                task_versions.insert(event.task_id, event.stream_sequence);
                previous_global_offset = Some(event.global_offset);
                after_offset = event.global_offset;
            }
        }
        let _repaired = before != projection_snapshot(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn append_engine_events(
        &self,
        task_id: TaskId,
        tenant_id: TenantId,
        session_id: SessionId,
        metadata: crate::AppendMetadata,
        expected_next_offset: Option<u64>,
        events: &[Event],
    ) -> Result<u64, TaskStoreError> {
        let authority = Self::engine_authority();
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let next_engine_offset: i64 = transaction.query_row(
            "SELECT COALESCE(
                MAX(CAST(json_extract(payload_json, '$.journalOffset') AS INTEGER)), -1
             ) + 1
             FROM event_log
             WHERE task_id = ?1
               AND event_type GLOB 'engine.*'
               AND json_extract(payload_json, '$.tenantId') = ?2
               AND json_extract(payload_json, '$.sessionId') = ?3",
            params![
                task_id.to_string(),
                tenant_id.to_string(),
                session_id.to_string()
            ],
            |row| row.get(0),
        )?;
        let next_engine_offset = nonnegative_integer(next_engine_offset)?;
        if let Some(expected) = expected_next_offset {
            if expected != next_engine_offset {
                return Err(TaskStoreError::EngineOffsetMismatch {
                    expected,
                    actual: next_engine_offset,
                });
            }
        }
        if events.is_empty() {
            transaction.commit()?;
            return Ok(next_engine_offset.saturating_sub(1));
        }

        let events = events
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, event)| {
                let index = u64::try_from(index).map_err(|_| TaskStoreError::IntegerOutOfRange)?;
                let journal_offset = next_engine_offset
                    .checked_add(index)
                    .ok_or(TaskStoreError::IntegerOutOfRange)?;
                NewTaskEvent::engine(
                    tenant_id,
                    session_id,
                    journal_offset,
                    metadata.run_id,
                    metadata.correlation_id,
                    metadata.causation_id,
                    event,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        validate_events(authority.source(), &events)?;
        promote_blob_references_in_transaction(&transaction, task_id, &events)?;
        validate_blob_references_in_transaction(&transaction, task_id, &events)?;

        let stream_version = stream_version_in_transaction(&transaction, task_id)?;
        let committed = append_in_transaction(
            &transaction,
            task_id,
            stream_version,
            authority.source(),
            events,
        )?;
        for event in &committed {
            self.projector.apply(&transaction, event)?;
        }
        roll_forward_safe_checkpoint_in_transaction(&transaction, task_id, &committed)?;
        let committed_count =
            u64::try_from(committed.len()).map_err(|_| TaskStoreError::IntegerOutOfRange)?;
        let last_offset = next_engine_offset
            .checked_add(committed_count)
            .and_then(|count| count.checked_sub(1))
            .ok_or(TaskStoreError::IntegerOutOfRange)?;
        transaction.commit()?;
        Ok(last_offset)
    }

    #[cfg(any(test, feature = "blob-file"))]
    pub(crate) fn stage_blob(
        &self,
        task_id: TaskId,
        blob_id: BlobId,
        media_type: &str,
        byte_size: u64,
        content_hash: [u8; 32],
        relative_path: &str,
    ) -> Result<(), TaskStoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if stream_version_in_transaction(&transaction, task_id)? == 0 {
            return Err(TaskStoreError::InvalidInput(format!(
                "task {task_id} must exist before staging blobs"
            )));
        }
        let content_hash = blake3::Hash::from_bytes(content_hash).to_hex().to_string();
        let existing_metadata = blob_metadata_in_transaction(&transaction, blob_id)?;
        if let Some(existing) = &existing_metadata {
            validate_staged_blob_identity(
                blob_id,
                existing.byte_size,
                &existing.content_hash,
                &existing.relative_path,
                byte_size,
                &content_hash,
                relative_path,
            )?;
        }
        let owned_media_type = transaction
            .query_row(
                "SELECT media_type FROM blob_ownership WHERE task_id = ?1 AND blob_id = ?2",
                params![task_id.to_string(), blob_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(owned_media_type) = owned_media_type {
            if owned_media_type != media_type {
                return Err(TaskStoreError::BlobIntegrity(format!(
                    "blob {blob_id} is already owned with another media type"
                )));
            }
            transaction.commit()?;
            return Ok(());
        }
        let existing_stage = staged_blob_for_task(&transaction, task_id, blob_id)?;
        if let Some(existing) = &existing_stage {
            validate_staged_blob_identity(
                blob_id,
                existing.byte_size,
                &existing.content_hash,
                &existing.relative_path,
                byte_size,
                &content_hash,
                relative_path,
            )?;
            if existing.media_type != media_type {
                return Err(TaskStoreError::BlobIntegrity(format!(
                    "blob {blob_id} is already staged with another media type"
                )));
            }
        }
        transaction.execute(
            "INSERT INTO blob_staging (
                task_id, blob_id, media_type, byte_size, content_hash, relative_path, staged_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(task_id, blob_id) DO UPDATE SET staged_at = excluded.staged_at",
            params![
                task_id.to_string(),
                blob_id.to_string(),
                media_type,
                sqlite_integer(byte_size)?,
                content_hash,
                relative_path,
                now().to_rfc3339(),
            ],
        )?;
        enforce_blob_quotas(&transaction, task_id)?;
        transaction.commit()?;
        Ok(())
    }

    #[cfg(feature = "blob-file")]
    pub(crate) fn discard_staged_blob_with<F>(
        &self,
        task_id: TaskId,
        blob_id: BlobId,
        cleanup_file: F,
    ) -> Result<(), TaskStoreError>
    where
        F: FnOnce() -> Result<(), TaskStoreError>,
    {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "DELETE FROM blob_staging WHERE task_id = ?1 AND blob_id = ?2",
            params![task_id.to_string(), blob_id.to_string()],
        )?;
        let referenced: i64 = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM blob_metadata WHERE blob_id = ?1)
                 OR EXISTS(SELECT 1 FROM blob_staging WHERE blob_id = ?1)",
            [blob_id.to_string()],
            |row| row.get(0),
        )?;
        if referenced == 0 {
            cleanup_file()?;
        }
        transaction.commit()?;
        Ok(())
    }

    #[cfg(feature = "blob-file")]
    pub(crate) fn cleanup_blob_if_unreferenced<F>(
        &self,
        blob_id: BlobId,
        byte_size: u64,
        content_hash: [u8; 32],
        relative_path: &str,
        cleanup_file: F,
    ) -> Result<(), TaskStoreError>
    where
        F: FnOnce() -> Result<(), TaskStoreError>,
    {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let referenced: i64 = transaction.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM blob_metadata
                WHERE blob_id = ?1 AND byte_size = ?2 AND content_hash = ?3 AND relative_path = ?4
             ) OR EXISTS(
                SELECT 1 FROM blob_staging
                WHERE blob_id = ?1 AND byte_size = ?2 AND content_hash = ?3 AND relative_path = ?4
             )",
            params![
                blob_id.to_string(),
                sqlite_integer(byte_size)?,
                blake3::Hash::from_bytes(content_hash).to_hex().to_string(),
                relative_path,
            ],
            |row| row.get(0),
        )?;
        if referenced == 0 {
            cleanup_file()?;
        }
        transaction.commit()?;
        Ok(())
    }

    #[cfg(feature = "blob-file")]
    pub(crate) fn bind_blob_root(&self, root: &Path) -> Result<(), TaskStoreError> {
        let root_text = root.to_str().ok_or_else(|| {
            TaskStoreError::InvalidInput("task blob root is not valid UTF-8".into())
        })?;
        let mut root_lock = self
            .blob_root_lock
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)?;
        let new_lock = if root_lock.is_none() {
            let lock_path = root.join(BLOB_ROOT_LOCK_FILE);
            let file = open_blob_root_lock(&lock_path)?;
            harness_fs::set_owner_only_file_if_unix(&file)?;
            file.try_lock_exclusive().map_err(|_| {
                TaskStoreError::BlobIntegrity(
                    "task blob root is already open by another store instance".into(),
                )
            })?;
            Some(file)
        } else {
            None
        };
        let mut claim = None;
        let result = (|| {
            let mut connection = self.lock()?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let existing = transaction.query_row(
                "SELECT canonical_root FROM blob_store_config WHERE singleton = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )?;
            if let Some(existing) = existing.as_deref() {
                if existing != root_text {
                    return Err(TaskStoreError::BlobIntegrity(format!(
                        "task database is already bound to blob root {existing}"
                    )));
                }
            }
            claim = Some(claim_blob_root(root, &self.database_identity)?);
            if existing.is_none() {
                transaction.execute(
                    "UPDATE blob_store_config SET canonical_root = ?1 WHERE singleton = 1",
                    [root_text],
                )?;
            }
            if new_lock.is_some() {
                reconcile_blob_root_in_transaction(&transaction, root)?;
            }
            transaction.commit()?;
            Ok(())
        })();
        if let Err(error) = result {
            if let Some(claim) = claim {
                claim.rollback();
            }
            return Err(error);
        }
        if let Some(file) = new_lock {
            *root_lock = Some(file);
        }
        Ok(())
    }

    #[cfg(feature = "blob-file")]
    pub fn blob_owner_task(&self, blob_id: BlobId) -> Result<Option<TaskId>, TaskStoreError> {
        let connection = self.lock()?;
        let task_id = connection
            .query_row(
                "SELECT task_id
                 FROM blob_ownership
                 WHERE blob_id = ?1
                 ORDER BY task_id ASC
                 LIMIT 1",
                [blob_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        task_id
            .map(|value| TaskId::parse(&value).map_err(TaskStoreError::from))
            .transpose()
    }

    #[cfg(feature = "blob-file")]
    pub(crate) fn blob_metadata_for_task(
        &self,
        task_id: TaskId,
        blob_id: BlobId,
    ) -> Result<StoredBlobMetadata, TaskStoreError> {
        let connection = self.lock()?;
        let metadata = connection
            .query_row(
                "SELECT ownership.media_type, metadata.byte_size,
                        metadata.content_hash, metadata.relative_path
                 FROM blob_metadata AS metadata
                 JOIN blob_ownership AS ownership ON ownership.blob_id = metadata.blob_id
                 WHERE ownership.task_id = ?1 AND metadata.blob_id = ?2",
                params![task_id.to_string(), blob_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        let metadata = match metadata {
            Some(metadata) => metadata,
            None => {
                let exists: i64 = connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM blob_metadata WHERE blob_id = ?1)",
                    [blob_id.to_string()],
                    |row| row.get(0),
                )?;
                return if exists == 1 {
                    Err(TaskStoreError::BlobOwnershipDenied { blob_id, task_id })
                } else {
                    Err(TaskStoreError::BlobNotFound { blob_id })
                };
            }
        };
        let hash = blake3::Hash::from_hex(&metadata.2).map_err(|error| {
            TaskStoreError::BlobIntegrity(format!(
                "blob {blob_id} has invalid hash metadata: {error}"
            ))
        })?;
        Ok(StoredBlobMetadata {
            media_type: metadata.0,
            byte_size: nonnegative_integer(metadata.1)?,
            content_hash: *hash.as_bytes(),
            relative_path: metadata.3,
        })
    }

    #[cfg(feature = "blob-file")]
    pub(crate) fn lock_blob_operation(
        &self,
        blob_id: BlobId,
    ) -> Result<std::sync::MutexGuard<'_, ()>, TaskStoreError> {
        let mut prefix = [0_u8; 8];
        prefix.copy_from_slice(&blob_id.as_bytes()[..8]);
        let index = usize::try_from(u64::from_be_bytes(prefix) % BLOB_OPERATION_LOCK_COUNT as u64)
            .map_err(|_| TaskStoreError::IntegerOutOfRange)?;
        self.blob_operation_locks[index]
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, TaskStoreError> {
        self.connection
            .lock()
            .map_err(|_| TaskStoreError::LockPoisoned)
    }
}

#[cfg(feature = "blob-file")]
fn load_or_create_blob_store_identity(connection: &Connection) -> Result<String, TaskStoreError> {
    let candidate = TaskId::new().to_string();
    connection.execute(
        "INSERT INTO blob_store_config (singleton, store_id, canonical_root)
         VALUES (1, ?1, NULL)
         ON CONFLICT(singleton) DO NOTHING",
        [candidate],
    )?;
    connection
        .query_row(
            "SELECT store_id FROM blob_store_config WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(TaskStoreError::from)
}

#[cfg(feature = "blob-file")]
fn open_blob_root_lock(path: &Path) -> Result<File, TaskStoreError> {
    #[cfg(unix)]
    {
        let parent = harness_fs::open_parent_dir_no_symlink_for_read(path)?
            .ok_or_else(|| TaskStoreError::BlobIntegrity("task blob root does not exist".into()))?;
        let file = parent.open_or_create_read_write_file(parent.file_name())?;
        if !file.metadata()?.is_file() {
            return Err(TaskStoreError::BlobIntegrity(
                "task blob root lock is not a regular file".into(),
            ));
        }
        parent.sync_all()?;
        return Ok(file);
    }

    #[cfg(not(unix))]
    {
        harness_fs::ensure_no_symlink_components(path)?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        if !file.metadata()?.is_file() {
            return Err(TaskStoreError::BlobIntegrity(
                "task blob root lock is not a regular file".into(),
            ));
        }
        Ok(file)
    }
}

#[cfg(feature = "blob-file")]
struct BlobRootClaim {
    directory: PathBuf,
    owner: PathBuf,
    owner_created: bool,
}

#[cfg(feature = "blob-file")]
impl BlobRootClaim {
    fn rollback(self) {
        if !self.owner_created {
            return;
        }
        let _ = std::fs::remove_dir(self.owner);
        let _ = std::fs::remove_dir(self.directory);
    }
}

#[cfg(feature = "blob-file")]
fn claim_blob_root(root: &Path, database_identity: &str) -> Result<BlobRootClaim, TaskStoreError> {
    let claim_directory = root.join(BLOB_ROOT_CLAIM_DIRECTORY);
    let owner = claim_directory.join(database_identity);
    let claim_directory_created = match std::fs::create_dir(&claim_directory) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(error) => return Err(error.into()),
    };
    let mut owner_created = false;
    let result = (|| {
        harness_fs::ensure_owner_only_app_dir(&claim_directory)?;
        owner_created = match std::fs::create_dir(&owner) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(error) => return Err(TaskStoreError::from(error)),
        };
        harness_fs::ensure_owner_only_app_dir(&owner)?;
        let mut entries = std::fs::read_dir(&claim_directory)?;
        let Some(entry) = entries.next().transpose()? else {
            return Err(TaskStoreError::BlobIntegrity(
                "task blob root has an incomplete database claim".into(),
            ));
        };
        if entry.path() != owner || entries.next().transpose()?.is_some() {
            return Err(TaskStoreError::BlobIntegrity(
                "task blob root is already claimed by another database".into(),
            ));
        }
        Ok(())
    })();
    if let Err(error) = result {
        BlobRootClaim {
            directory: claim_directory.clone(),
            owner,
            owner_created,
        }
        .rollback();
        if claim_directory_created {
            let _ = std::fs::remove_dir(&claim_directory);
        }
        return Err(error);
    }
    Ok(BlobRootClaim {
        directory: claim_directory,
        owner,
        owner_created,
    })
}

#[cfg(feature = "blob-file")]
fn reconcile_blob_root_in_transaction(
    transaction: &Transaction<'_>,
    root: &Path,
) -> Result<(), TaskStoreError> {
    let mut statement = transaction.prepare(
        "SELECT relative_path FROM blob_metadata
         UNION
         SELECT relative_path FROM blob_staging",
    )?;
    let referenced = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<HashSet<_>, _>>()?;
    drop(statement);

    for prefix_entry in std::fs::read_dir(root)? {
        let prefix_entry = prefix_entry?;
        if prefix_entry.file_name() == BLOB_ROOT_CLAIM_DIRECTORY {
            continue;
        }
        let prefix_path = prefix_entry.path();
        let prefix_metadata = std::fs::symlink_metadata(&prefix_path)?;
        if prefix_metadata.file_type().is_symlink() {
            return Err(TaskStoreError::BlobIntegrity(
                "task blob root contains a symlink".into(),
            ));
        }
        let prefix = prefix_entry.file_name();
        if !prefix_metadata.is_dir() || prefix.as_encoded_bytes().len() != 2 {
            continue;
        }
        for body_entry in std::fs::read_dir(&prefix_path)? {
            let body_entry = body_entry?;
            let body_path = body_entry.path();
            let body_metadata = std::fs::symlink_metadata(&body_path)?;
            if body_metadata.file_type().is_symlink() || !body_metadata.is_file() {
                return Err(TaskStoreError::BlobIntegrity(
                    "task blob directory contains a non-regular file".into(),
                ));
            }
            let relative_path = body_path
                .strip_prefix(root)
                .map_err(|_| {
                    TaskStoreError::BlobIntegrity(
                        "task blob file is outside its configured root".into(),
                    )
                })?
                .to_str()
                .ok_or_else(|| {
                    TaskStoreError::BlobIntegrity(
                        "task blob relative path is not valid UTF-8".into(),
                    )
                })?;
            if !referenced.contains(relative_path) {
                harness_fs::remove_file_no_follow(&body_path)?;
            }
        }
    }
    Ok(())
}

fn append_in_transaction(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    expected_version: u64,
    source: &EventSource,
    events: Vec<NewTaskEvent>,
) -> Result<Vec<TaskEventEnvelope>, TaskStoreError> {
    let actual = stream_version_in_transaction(transaction, task_id)?;
    if actual != expected_version {
        return Err(TaskStoreError::WrongExpectedVersion {
            expected: expected_version,
            actual,
        });
    }

    let source_json = serde_json::to_string(source)?;
    let mut committed = Vec::with_capacity(events.len());
    for (index, event) in events.into_iter().enumerate() {
        let index = u64::try_from(index).map_err(|_| TaskStoreError::IntegerOutOfRange)?;
        let stream_sequence = expected_version
            .checked_add(index + 1)
            .ok_or(TaskStoreError::IntegerOutOfRange)?;
        let event_id = EventId::new();
        let recorded_at = now();
        let (event_type, schema_version, payload) = event.encode()?;
        let payload_json = serde_json::to_string(&payload)?;
        let inserted = transaction.execute(
            "INSERT INTO event_log (
                task_id, stream_sequence, event_id, event_type, schema_version,
                recorded_at, source_json, payload_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                task_id.to_string(),
                sqlite_integer(stream_sequence)?,
                event_id.to_string(),
                event_type,
                i64::from(schema_version),
                recorded_at.to_rfc3339(),
                source_json,
                payload_json,
            ],
        );
        if let Err(error) = inserted {
            if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                return Err(TaskStoreError::Projector(
                    "task event violates an identity or stream uniqueness constraint".into(),
                ));
            }
            return Err(error.into());
        }
        let global_offset = nonnegative_integer(transaction.last_insert_rowid())?;
        committed.push(TaskEventEnvelope {
            global_offset,
            task_id,
            stream_sequence,
            event_id,
            event_type: event_type.into(),
            schema_version,
            recorded_at,
            source: source.clone(),
            payload,
        });
    }
    Ok(committed)
}

fn enqueue_segment_start_in_transaction(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    command_payload: &Value,
    committed: &[TaskEventEnvelope],
) -> Result<(), TaskStoreError> {
    if command_payload.get("type").and_then(Value::as_str) != Some("continue_task") {
        return Ok(());
    }
    let payload: ContinueTaskCommandPayload = serde_json::from_value(command_payload.clone())?;
    if payload.command_type != "continue_task"
        || payload.indeterminate_tools.len() > MAX_EVENTS_PER_TRANSACTION
    {
        return Err(TaskStoreError::InvalidInput(
            "continue_task recovery decisions are invalid".into(),
        ));
    }
    let mut tool_ids = HashSet::new();
    for decision in &payload.indeterminate_tools {
        let tool_use_id = ToolUseId::parse(&decision.tool_use_id)?;
        if !tool_ids.insert(tool_use_id) {
            return Err(TaskStoreError::InvalidInput(
                "continue_task resolves a tool more than once".into(),
            ));
        }
    }
    let declared_decisions = serde_json::to_value(&payload.indeterminate_tools)?;
    let starts_matching_segment = committed
        .iter()
        .filter(|event| {
            event.event_type == "run.started"
                && event.payload.get("segmentId")
                    == Some(&Value::String(payload.segment_id.to_string()))
                && event.payload.get("recoveryStart") == Some(&Value::Bool(true))
                && event.payload.get("indeterminateTools") == Some(&declared_decisions)
        })
        .count();
    if starts_matching_segment != 1 {
        return Err(TaskStoreError::InvalidInput(
            "continue_task must commit its declared run segment".into(),
        ));
    }
    let request = PendingSegmentStart {
        task_id,
        segment_id: payload.segment_id,
        indeterminate_tools: payload.indeterminate_tools,
    };
    let body = serde_json::to_string(&request)?;
    if body.len() > MAX_COMMAND_PAYLOAD_BYTES {
        return Err(TaskStoreError::InvalidInput(
            "pending segment start exceeds 1 MiB".into(),
        ));
    }
    transaction.execute(
        "INSERT INTO segment_start_outbox (
            task_id, run_segment_id, request_json, created_at, delivered_at
         ) VALUES (?1, ?2, ?3, ?4, NULL)",
        params![
            task_id.to_string(),
            request.segment_id.to_string(),
            body,
            now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn validate_blob_references_in_transaction(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    events: &[NewTaskEvent],
) -> Result<(), TaskStoreError> {
    let mut references = Vec::new();
    for event in events {
        references.extend(event.blob_references()?);
    }
    let references = merge_blob_references(references)?;
    for reference in references {
        validate_blob_reference_in_transaction(transaction, task_id, &reference)?;
    }
    Ok(())
}

fn merge_blob_references(
    references: impl IntoIterator<Item = TaskBlobReference>,
) -> Result<Vec<TaskBlobReference>, TaskStoreError> {
    let mut merged = HashMap::<BlobId, TaskBlobReference>::new();
    for reference in references {
        match merged.get_mut(&reference.blob_id) {
            Some(existing) => match (&existing.expected, reference.expected) {
                (Some(current), Some(incoming)) if current != &incoming => {
                    return Err(TaskStoreError::BlobIntegrity(format!(
                        "blob {} has conflicting references in one event batch",
                        reference.blob_id
                    )));
                }
                (None, Some(incoming)) => existing.expected = Some(incoming),
                _ => {}
            },
            None => {
                merged.insert(reference.blob_id, reference);
            }
        }
    }
    Ok(merged.into_values().collect())
}

fn validate_blob_reference_in_transaction(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    reference: &TaskBlobReference,
) -> Result<(), TaskStoreError> {
    let blob_id = reference.blob_id;
    let owned = transaction
        .query_row(
            "SELECT ownership.media_type, metadata.byte_size, metadata.content_hash
             FROM blob_ownership AS ownership
             JOIN blob_metadata AS metadata ON metadata.blob_id = ownership.blob_id
             WHERE ownership.task_id = ?1 AND ownership.blob_id = ?2",
            params![task_id.to_string(), blob_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((media_type, byte_size, content_hash)) = owned else {
        return if blob_metadata_in_transaction(transaction, blob_id)?.is_some() {
            Err(TaskStoreError::BlobOwnershipDenied { blob_id, task_id })
        } else {
            Err(TaskStoreError::BlobNotFound { blob_id })
        };
    };
    if let Some(expected) = &reference.expected {
        let expected_media_type = expected.content_type.as_deref();
        if expected.id != blob_id
            || expected.size != nonnegative_integer(byte_size)?
            || blake3::Hash::from_bytes(expected.content_hash)
                .to_hex()
                .as_str()
                != content_hash
            || expected_media_type != Some(media_type.as_str())
        {
            return Err(TaskStoreError::BlobIntegrity(format!(
                "blob reference {blob_id} does not match task-owned metadata"
            )));
        }
    }
    Ok(())
}

fn promote_blob_references_in_transaction(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    events: &[NewTaskEvent],
) -> Result<(), TaskStoreError> {
    let mut checked = HashSet::new();
    let mut references = Vec::new();
    for event in events {
        references.extend(event.blob_references()?);
    }
    for blob_id in references.into_iter().map(|reference| reference.blob_id) {
        if !checked.insert(blob_id) {
            continue;
        }
        promote_blob_id_in_transaction(transaction, task_id, blob_id)?;
    }
    Ok(())
}

fn promote_blob_id_in_transaction(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    blob_id: BlobId,
) -> Result<(), TaskStoreError> {
    let owned: i64 = transaction.query_row(
        "SELECT EXISTS(
                SELECT 1 FROM blob_ownership WHERE task_id = ?1 AND blob_id = ?2
             )",
        params![task_id.to_string(), blob_id.to_string()],
        |row| row.get(0),
    )?;
    if owned == 1 {
        return Ok(());
    }
    let staged = staged_blob_for_task(transaction, task_id, blob_id)?;
    let Some(staged) = staged else {
        return if blob_metadata_in_transaction(transaction, blob_id)?.is_some() {
            Err(TaskStoreError::BlobOwnershipDenied { blob_id, task_id })
        } else {
            Err(TaskStoreError::BlobNotFound { blob_id })
        };
    };
    if let Some(existing) = blob_metadata_in_transaction(transaction, blob_id)? {
        validate_staged_blob_identity(
            blob_id,
            existing.byte_size,
            &existing.content_hash,
            &existing.relative_path,
            staged.byte_size,
            &staged.content_hash,
            &staged.relative_path,
        )?;
    } else {
        transaction.execute(
            "INSERT INTO blob_metadata (
                    blob_id, media_type, byte_size, content_hash, relative_path, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                blob_id.to_string(),
                staged.media_type,
                sqlite_integer(staged.byte_size)?,
                staged.content_hash,
                staged.relative_path,
                now().to_rfc3339(),
            ],
        )?;
    }
    transaction.execute(
        "INSERT INTO blob_ownership (task_id, blob_id, media_type, created_at)
             VALUES (?1, ?2, ?3, ?4)",
        params![
            task_id.to_string(),
            blob_id.to_string(),
            staged.media_type,
            now().to_rfc3339(),
        ],
    )?;
    transaction.execute(
        "DELETE FROM blob_staging WHERE task_id = ?1 AND blob_id = ?2",
        params![task_id.to_string(), blob_id.to_string()],
    )?;
    Ok(())
}

fn prepare_command_blob_references(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    events: &[NewTaskEvent],
) -> Result<(), TaskStoreError> {
    transaction.execute_batch("SAVEPOINT command_blob_references")?;
    let result = promote_blob_references_in_transaction(transaction, task_id, events)
        .and_then(|()| validate_blob_references_in_transaction(transaction, task_id, events));
    match result {
        Ok(()) => {
            transaction.execute_batch("RELEASE command_blob_references")?;
            Ok(())
        }
        Err(error) => {
            transaction.execute_batch(
                "ROLLBACK TO command_blob_references; RELEASE command_blob_references",
            )?;
            Err(error)
        }
    }
}

fn blob_command_rejection(error: &TaskStoreError) -> Option<CommandRejection> {
    match error {
        TaskStoreError::BlobNotFound { .. }
        | TaskStoreError::BlobOwnershipDenied { .. }
        | TaskStoreError::BlobIntegrity(_) => Some(CommandRejection::InvalidCommand {
            message: "command references an unavailable or invalid blob".into(),
        }),
        _ => None,
    }
}

fn blob_metadata_in_transaction(
    transaction: &Transaction<'_>,
    blob_id: BlobId,
) -> Result<Option<StagedBlobMetadata>, TaskStoreError> {
    transaction
        .query_row(
            "SELECT media_type, byte_size, content_hash, relative_path
             FROM blob_metadata WHERE blob_id = ?1",
            [blob_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
        .map(|(media_type, byte_size, content_hash, relative_path)| {
            Ok(StagedBlobMetadata {
                media_type,
                byte_size: nonnegative_integer(byte_size)?,
                content_hash,
                relative_path,
            })
        })
        .transpose()
}

fn staged_blob_for_task(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    blob_id: BlobId,
) -> Result<Option<StagedBlobMetadata>, TaskStoreError> {
    transaction
        .query_row(
            "SELECT media_type, byte_size, content_hash, relative_path
             FROM blob_staging WHERE task_id = ?1 AND blob_id = ?2",
            params![task_id.to_string(), blob_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
        .map(|(media_type, byte_size, content_hash, relative_path)| {
            Ok(StagedBlobMetadata {
                media_type,
                byte_size: nonnegative_integer(byte_size)?,
                content_hash,
                relative_path,
            })
        })
        .transpose()
}

#[allow(clippy::too_many_arguments)]
fn validate_staged_blob_identity(
    blob_id: BlobId,
    existing_size: u64,
    existing_hash: &str,
    existing_path: &str,
    staged_size: u64,
    staged_hash: &str,
    staged_path: &str,
) -> Result<(), TaskStoreError> {
    if existing_size != staged_size || existing_hash != staged_hash || existing_path != staged_path
    {
        return Err(TaskStoreError::BlobIntegrity(format!(
            "blob {blob_id} metadata conflicts with its content-addressed identity"
        )));
    }
    Ok(())
}

#[cfg(any(test, feature = "blob-file"))]
fn enforce_blob_quotas(
    transaction: &Transaction<'_>,
    task_id: TaskId,
) -> Result<(), TaskStoreError> {
    let task_bytes: i64 = transaction.query_row(
        "SELECT COALESCE(SUM(byte_size), 0) FROM (
            SELECT metadata.blob_id, metadata.byte_size
            FROM blob_metadata AS metadata
            JOIN blob_ownership AS ownership ON ownership.blob_id = metadata.blob_id
            WHERE ownership.task_id = ?1
            UNION
            SELECT blob_id, byte_size FROM blob_staging WHERE task_id = ?1
         )",
        [task_id.to_string()],
        |row| row.get(0),
    )?;
    if nonnegative_integer(task_bytes)? > MAX_TASK_BLOB_BYTES {
        return Err(TaskStoreError::InvalidInput(format!(
            "task blob quota exceeds {MAX_TASK_BLOB_BYTES} bytes"
        )));
    }
    let global_bytes: i64 = transaction.query_row(
        "SELECT COALESCE(SUM(byte_size), 0) FROM (
            SELECT blob_id, byte_size FROM blob_metadata
            UNION
            SELECT blob_id, byte_size FROM blob_staging
         )",
        [],
        |row| row.get(0),
    )?;
    if nonnegative_integer(global_bytes)? > MAX_GLOBAL_BLOB_BYTES {
        return Err(TaskStoreError::InvalidInput(format!(
            "global blob quota exceeds {MAX_GLOBAL_BLOB_BYTES} bytes"
        )));
    }
    Ok(())
}

fn stream_version_in_transaction(
    transaction: &Transaction<'_>,
    task_id: TaskId,
) -> Result<u64, TaskStoreError> {
    let version: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(stream_sequence), 0) FROM event_log WHERE task_id = ?1",
        [task_id.to_string()],
        |row| row.get(0),
    )?;
    nonnegative_integer(version)
}

fn projection_for_decision(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    actual_stream_version: u64,
) -> Result<TaskProjection, TaskStoreError> {
    let canonical_row = load_task_projection_row(transaction, task_id)?;
    if actual_stream_version == 0 {
        if canonical_row.is_some() {
            return Err(TaskStoreError::ProjectionIntegrity(format!(
                "task {task_id} has a projection without an event stream"
            )));
        }
        return Ok(empty_task_projection(task_id));
    }
    let (_, projection) = canonical_row.ok_or_else(|| {
        TaskStoreError::ProjectionIntegrity(format!(
            "task {task_id} has events but no canonical projection"
        ))
    })?;
    if projection.stream_version != actual_stream_version {
        return Err(TaskStoreError::ProjectionIntegrity(format!(
            "task {task_id} canonical version is {}, event version is {actual_stream_version}",
            projection.stream_version
        )));
    }
    let event_global_offset: i64 = transaction.query_row(
        "SELECT global_offset FROM event_log
         WHERE task_id = ?1
         ORDER BY stream_sequence DESC
         LIMIT 1",
        [task_id.to_string()],
        |row| row.get(0),
    )?;
    let event_global_offset = nonnegative_integer(event_global_offset)?;
    if projection.last_global_offset != event_global_offset {
        return Err(TaskStoreError::ProjectionIntegrity(format!(
            "task {task_id} projection offset is {}, event offset is {event_global_offset}",
            projection.last_global_offset
        )));
    }
    Ok(projection)
}

fn latest_global_offset_in_transaction(
    transaction: &Transaction<'_>,
) -> Result<u64, TaskStoreError> {
    let offset: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(global_offset), 0) FROM event_log",
        [],
        |row| row.get(0),
    )?;
    nonnegative_integer(offset)
}

fn stored_command_outcome(
    transaction: &Transaction<'_>,
    command: &AcceptedCommand,
    command_hash: &str,
) -> Result<Option<CommandOutcome>, TaskStoreError> {
    let mut statement = transaction.prepare(
        "SELECT command_id, task_id, principal_id, idempotency_key, command_hash,
                expected_stream_version, status, completed_at, outcome_json,
                result_stream_version, committed_offset, event_count
         FROM command_inbox
         WHERE command_id = ?1
            OR (task_id = ?2 AND principal_id = ?3 AND idempotency_key = ?4)",
    )?;
    let rows = statement.query_map(
        params![
            command.command_id.to_string(),
            command.task_id.to_string(),
            command.authority.principal_id(),
            command.idempotency_key
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<i64>>(9)?,
                row.get::<_, Option<i64>>(10)?,
                row.get::<_, Option<i64>>(11)?,
            ))
        },
    )?;
    let rows = rows.collect::<Result<Vec<_>, _>>()?;
    if rows.is_empty() {
        return Ok(None);
    }
    if rows.len() != 1 {
        return Err(TaskStoreError::CommandConflict {
            command_id: command.command_id,
        });
    }
    let (
        stored_command_id,
        stored_task_id,
        stored_principal_id,
        stored_idempotency_key,
        stored_hash,
        stored_expected_version,
        status,
        completed_at,
        outcome_json,
        result_stream_version,
        committed_offset,
        event_count,
    ) = &rows[0];
    let same_command_id = stored_command_id == &command.command_id.to_string();
    let same_idempotency_key = stored_task_id == &command.task_id.to_string()
        && stored_principal_id == command.authority.principal_id()
        && stored_idempotency_key == &command.idempotency_key;
    let command_id_replay = same_command_id && same_idempotency_key;
    let idempotency_replay = !same_command_id && same_idempotency_key;
    if (!command_id_replay && !idempotency_replay)
        || stored_hash != command_hash
        || stored_task_id != &command.task_id.to_string()
        || stored_principal_id != command.authority.principal_id()
        || nonnegative_integer(*stored_expected_version)? != command.expected_stream_version
    {
        return Err(TaskStoreError::CommandConflict {
            command_id: command.command_id,
        });
    }
    if !matches!(status.as_str(), "accepted" | "rejected") || completed_at.is_none() {
        return Err(TaskStoreError::IncompleteCommand {
            command_id: command.command_id,
        });
    }
    let outcome_json = outcome_json
        .as_ref()
        .ok_or(TaskStoreError::IncompleteCommand {
            command_id: command.command_id,
        })?;
    let outcome: CommandOutcome = serde_json::from_str(outcome_json)?;
    let result_stream_version = required_nonnegative_integer(
        *result_stream_version,
        stored_command_id,
        "result stream version",
    )?;
    let committed_offset =
        required_nonnegative_integer(*committed_offset, stored_command_id, "committed offset")?;
    let event_count = required_nonnegative_integer(*event_count, stored_command_id, "event count")?;
    let (outcome_command_id, outcome_task_id, outcome_status) = match &outcome {
        CommandOutcome::Accepted {
            command_id,
            task_id,
            ..
        } => (*command_id, *task_id, "accepted"),
        CommandOutcome::Rejected {
            command_id,
            task_id,
            ..
        } => (*command_id, *task_id, "rejected"),
    };
    if outcome_command_id.to_string() != *stored_command_id
        || outcome_task_id.to_string() != *stored_task_id
        || outcome_status != status
    {
        return Err(TaskStoreError::ProjectionIntegrity(format!(
            "command {stored_command_id} inbox outcome is inconsistent"
        )));
    }
    validate_stored_command_facts(
        transaction,
        &outcome,
        command.expected_stream_version,
        result_stream_version,
        committed_offset,
        event_count,
    )?;
    Ok(Some(outcome))
}

fn finish_command(
    transaction: &Transaction<'_>,
    command_id: CommandId,
    status: &str,
    outcome: &CommandOutcome,
    result_stream_version: u64,
    committed_offset: u64,
    event_count: usize,
) -> Result<(), TaskStoreError> {
    let changed = transaction.execute(
        "UPDATE command_inbox
         SET status = ?2, completed_at = ?3, outcome_json = ?4,
             result_stream_version = ?5, committed_offset = ?6, event_count = ?7
         WHERE command_id = ?1",
        params![
            command_id.to_string(),
            status,
            now().to_rfc3339(),
            serde_json::to_string(outcome)?,
            sqlite_integer(result_stream_version)?,
            sqlite_integer(committed_offset)?,
            i64::try_from(event_count).map_err(|_| TaskStoreError::IntegerOutOfRange)?,
        ],
    )?;
    if changed != 1 {
        return Err(TaskStoreError::IncompleteCommand { command_id });
    }
    Ok(())
}

fn required_nonnegative_integer(
    value: Option<i64>,
    command_id: &str,
    field: &str,
) -> Result<u64, TaskStoreError> {
    value
        .ok_or_else(|| {
            TaskStoreError::ProjectionIntegrity(format!("command {command_id} is missing {field}"))
        })
        .and_then(nonnegative_integer)
}

fn validate_stored_command_facts(
    transaction: &Transaction<'_>,
    outcome: &CommandOutcome,
    expected_stream_version: u64,
    result_stream_version: u64,
    committed_offset: u64,
    event_count: u64,
) -> Result<(), TaskStoreError> {
    match outcome {
        CommandOutcome::Accepted {
            task_id,
            stream_version,
            committed_offset: outcome_offset,
            ..
        } => {
            if event_count == 0
                || *stream_version != result_stream_version
                || *outcome_offset != committed_offset
                || result_stream_version
                    != expected_stream_version
                        .checked_add(event_count)
                        .ok_or(TaskStoreError::IntegerOutOfRange)?
            {
                return Err(TaskStoreError::ProjectionIntegrity(
                    "accepted command outcome does not match persisted commit facts".into(),
                ));
            }
            if event_count > 0 {
                let (count, max_offset): (i64, Option<i64>) = transaction.query_row(
                    "SELECT COUNT(*), MAX(global_offset)
                     FROM event_log
                     WHERE task_id = ?1 AND stream_sequence > ?2 AND stream_sequence <= ?3",
                    params![
                        task_id.to_string(),
                        sqlite_integer(expected_stream_version)?,
                        sqlite_integer(result_stream_version)?,
                    ],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                if nonnegative_integer(count)? != event_count
                    || max_offset.map(nonnegative_integer).transpose()? != Some(committed_offset)
                {
                    return Err(TaskStoreError::ProjectionIntegrity(
                        "accepted command commit facts do not match the event log".into(),
                    ));
                }
            }
        }
        CommandOutcome::Rejected { rejection, .. } => {
            if event_count != 0 {
                return Err(TaskStoreError::ProjectionIntegrity(
                    "rejected command persisted a non-zero event count".into(),
                ));
            }
            match rejection {
                CommandRejection::WrongExpectedVersion { expected, actual }
                    if *expected == expected_stream_version && *actual == result_stream_version => {
                }
                CommandRejection::StaleQueueRevision { .. }
                | CommandRejection::InvalidCommand { .. }
                    if result_stream_version == expected_stream_version => {}
                _ => {
                    return Err(TaskStoreError::ProjectionIntegrity(
                        "rejected command outcome does not match persisted commit facts".into(),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn command_hash(command: &AcceptedCommand) -> Result<String, TaskStoreError> {
    let canonical = canonicalize_json(json!({
        "taskId": command.task_id,
        "expectedStreamVersion": command.expected_stream_version,
        "principalId": command.authority.principal_id(),
        "source": command.authority.source(),
        "payload": command.payload,
    }));
    Ok(blake3::hash(&serde_json::to_vec(&canonical)?)
        .to_hex()
        .to_string())
}

fn validate_command(command: &AcceptedCommand) -> Result<(), TaskStoreError> {
    if i64::try_from(command.expected_stream_version).is_err() {
        return Err(TaskStoreError::InvalidInput(
            "expected stream version exceeds SQLite's supported range".into(),
        ));
    }
    if command.idempotency_key.is_empty()
        || command.idempotency_key.len() > MAX_IDEMPOTENCY_KEY_BYTES
    {
        return Err(TaskStoreError::InvalidInput(
            "idempotency key must contain 1 to 256 bytes".into(),
        ));
    }
    if serde_json::to_vec(&command.payload)?.len() > MAX_COMMAND_PAYLOAD_BYTES {
        return Err(TaskStoreError::InvalidInput(
            "command payload exceeds 1 MiB".into(),
        ));
    }
    Ok(())
}

fn validate_events(source: &EventSource, events: &[NewTaskEvent]) -> Result<(), TaskStoreError> {
    if events.len() > MAX_EVENTS_PER_TRANSACTION {
        return Err(TaskStoreError::InvalidInput(format!(
            "a transaction may contain at most {MAX_EVENTS_PER_TRANSACTION} events"
        )));
    }
    let source_bytes = serde_json::to_vec(source)?.len();
    let mut total_bytes = 0_usize;
    for event in events {
        event.validate_source(source)?;
        let (event_type, _, payload) = event.encode()?;
        let payload_bytes = serde_json::to_vec(&payload)?.len();
        let event_bytes = source_bytes
            .checked_add(event_type.len())
            .and_then(|bytes| bytes.checked_add(payload_bytes))
            .ok_or(TaskStoreError::IntegerOutOfRange)?;
        total_bytes = total_bytes
            .checked_add(event_bytes)
            .ok_or(TaskStoreError::IntegerOutOfRange)?;
        if total_bytes > MAX_TOTAL_EVENT_BYTES_PER_TRANSACTION {
            return Err(TaskStoreError::InvalidInput(
                "task event transaction exceeds 8 MiB".into(),
            ));
        }
    }
    Ok(())
}

fn validate_queue_consumption_events(events: &[NewTaskEvent]) -> Result<(), TaskStoreError> {
    let started_segments = events
        .iter()
        .filter_map(|event| match &event.event {
            TaskEvent::RunStarted { segment_id, .. } => Some(*segment_id),
            _ => None,
        })
        .collect::<HashSet<_>>();
    for event in events {
        let TaskEvent::MessageConsumed { run_segment_id, .. } = &event.event else {
            continue;
        };
        if !started_segments.contains(run_segment_id) {
            return Err(TaskStoreError::InvalidInput(
                "message consumption must start its run in the same command".into(),
            ));
        }
    }
    Ok(())
}

fn canonicalize_json(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_json).collect()),
        Value::Object(values) => {
            let sorted = values
                .into_iter()
                .map(|(key, value)| (key, canonicalize_json(value)))
                .collect::<std::collections::BTreeMap<_, _>>();
            Value::Object(Map::from_iter(sorted))
        }
        scalar => scalar,
    }
}

fn load_events_page(
    transaction: &Transaction<'_>,
    after_offset: u64,
    limit: usize,
) -> Result<Vec<TaskEventEnvelope>, TaskStoreError> {
    let mut statement = transaction.prepare(
        "SELECT global_offset, task_id, stream_sequence, event_id, event_type,
                schema_version, recorded_at, source_json, payload_json
         FROM event_log
         WHERE global_offset > ?1
         ORDER BY global_offset ASC
         LIMIT ?2",
    )?;
    let rows = statement.query_map(
        params![
            sqlite_integer(after_offset)?,
            i64::try_from(limit).map_err(|_| TaskStoreError::IntegerOutOfRange)?
        ],
        |row| {
            Ok(StoredTaskEvent {
                global_offset: row.get(0)?,
                task_id: row.get(1)?,
                stream_sequence: row.get(2)?,
                event_id: row.get(3)?,
                event_type: row.get(4)?,
                schema_version: row.get(5)?,
                recorded_at: row.get(6)?,
                source_json: row.get(7)?,
                payload_json: row.get(8)?,
            })
        },
    )?;
    rows.map(|row| {
        row.map_err(TaskStoreError::from)
            .and_then(StoredTaskEvent::decode)
    })
    .collect()
}

struct StoredTaskEvent {
    global_offset: i64,
    task_id: String,
    stream_sequence: i64,
    event_id: String,
    event_type: String,
    schema_version: i64,
    recorded_at: String,
    source_json: String,
    payload_json: String,
}

impl StoredTaskEvent {
    fn decode(self) -> Result<TaskEventEnvelope, TaskStoreError> {
        if self.event_type.len() > MAX_EVENT_TYPE_BYTES
            || self.source_json.len() > MAX_SOURCE_JSON_BYTES
            || self.payload_json.len() > MAX_EVENT_PAYLOAD_BYTES
        {
            return Err(TaskStoreError::InvalidInput(
                "persisted task event exceeds its encoded size limit".into(),
            ));
        }
        Ok(TaskEventEnvelope {
            global_offset: nonnegative_integer(self.global_offset)?,
            task_id: TaskId::parse(&self.task_id)?,
            stream_sequence: nonnegative_integer(self.stream_sequence)?,
            event_id: EventId::parse(&self.event_id)?,
            event_type: self.event_type,
            schema_version: u16::try_from(self.schema_version)
                .map_err(|_| TaskStoreError::IntegerOutOfRange)?,
            recorded_at: DateTime::parse_from_rfc3339(&self.recorded_at)
                .map_err(|error| TaskStoreError::InvalidTimestamp(error.to_string()))?
                .with_timezone(&Utc),
            source: serde_json::from_str(&self.source_json)?,
            payload: serde_json::from_str(&self.payload_json)?,
        })
    }
}

fn sqlite_integer(value: u64) -> Result<i64, TaskStoreError> {
    i64::try_from(value).map_err(|_| TaskStoreError::IntegerOutOfRange)
}

fn nonnegative_integer(value: i64) -> Result<u64, TaskStoreError> {
    u64::try_from(value).map_err(|_| TaskStoreError::IntegerOutOfRange)
}

fn validate_checkpoint_recovery_state(
    checkpoint: &TaskCheckpoint,
    persisted: bool,
) -> Result<(), TaskStoreError> {
    let unique_tools = checkpoint
        .incomplete_tool_use_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let unique_child_actors = checkpoint
        .child_actor_refs
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    if checkpoint
        .workspace_baseline
        .as_ref()
        .is_some_and(|baseline| baseline.revision.is_empty() || baseline.revision.len() > 4096)
        || checkpoint.incomplete_tool_use_ids.len() > MAX_EVENTS_PER_TRANSACTION
        || checkpoint.child_actor_refs.len() > MAX_EVENTS_PER_TRANSACTION
        || unique_tools.len() != checkpoint.incomplete_tool_use_ids.len()
        || unique_child_actors.len() != checkpoint.child_actor_refs.len()
    {
        return Err(if persisted {
            TaskStoreError::ProjectionIntegrity(
                "persisted checkpoint recovery state exceeds its size limits".into(),
            )
        } else {
            TaskStoreError::InvalidInput("checkpoint recovery state exceeds its size limits".into())
        });
    }
    Ok(())
}

fn encode_checkpoint(checkpoint: &TaskCheckpoint) -> Result<String, TaskStoreError> {
    validate_checkpoint_recovery_state(checkpoint, false)?;
    let body = serde_json::to_string(checkpoint)?;
    if body.len() > MAX_EVENT_PAYLOAD_BYTES {
        return Err(TaskStoreError::InvalidInput(
            "checkpoint payload exceeds 1 MiB".into(),
        ));
    }
    Ok(body)
}

fn validate_checkpoint_references(
    connection: &Connection,
    checkpoint: &TaskCheckpoint,
    require_current_offset: bool,
    persisted: bool,
) -> Result<(), TaskStoreError> {
    let projection_offset = connection
        .query_row(
            "SELECT last_global_offset FROM task_projection WHERE task_id = ?1",
            [checkpoint.task_id.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or_else(|| {
            if persisted {
                TaskStoreError::ProjectionIntegrity(
                    "persisted checkpoint task does not exist".into(),
                )
            } else {
                TaskStoreError::InvalidInput("checkpoint task does not exist".into())
            }
        })?;
    let projection_offset = nonnegative_integer(projection_offset)?;
    let invalid_offset = if require_current_offset {
        projection_offset != checkpoint.committed_global_offset
    } else {
        projection_offset < checkpoint.committed_global_offset
    };
    if invalid_offset {
        return Err(if persisted {
            TaskStoreError::ProjectionIntegrity(
                "persisted checkpoint offset is ahead of the task projection".into(),
            )
        } else {
            TaskStoreError::InvalidInput(
                "checkpoint offset is not the task's committed projection offset".into(),
            )
        });
    }
    let offset_belongs_to_task = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM event_log WHERE task_id = ?1 AND global_offset = ?2
         )",
        params![
            checkpoint.task_id.to_string(),
            sqlite_integer(checkpoint.committed_global_offset)?
        ],
        |row| row.get::<_, bool>(0),
    )?;
    if !offset_belongs_to_task {
        return Err(if persisted {
            TaskStoreError::ProjectionIntegrity(
                "persisted checkpoint offset does not belong to its task".into(),
            )
        } else {
            TaskStoreError::InvalidInput("checkpoint offset does not belong to its task".into())
        });
    }
    let run_start_offset = connection
        .query_row(
            "SELECT global_offset
             FROM event_log
             WHERE task_id = ?1
               AND event_type = 'run.started'
               AND json_extract(payload_json, '$.segmentId') = ?2",
            params![
                checkpoint.task_id.to_string(),
                checkpoint.run_segment_id.to_string()
            ],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(run_start_offset) = run_start_offset else {
        return Err(if persisted {
            TaskStoreError::ProjectionIntegrity(
                "persisted checkpoint run segment does not belong to its task".into(),
            )
        } else {
            TaskStoreError::InvalidInput(
                "checkpoint run segment does not belong to the task".into(),
            )
        });
    };
    let next_run_start_offset = connection.query_row(
        "SELECT MIN(global_offset)
             FROM event_log
             WHERE task_id = ?1
               AND event_type = 'run.started'
               AND global_offset > ?2",
        params![checkpoint.task_id.to_string(), run_start_offset,],
        |row| row.get::<_, Option<i64>>(0),
    )?;
    let checkpoint_offset = sqlite_integer(checkpoint.committed_global_offset)?;
    if checkpoint_offset < run_start_offset
        || next_run_start_offset.is_some_and(|next| checkpoint_offset >= next)
    {
        return Err(if persisted {
            TaskStoreError::ProjectionIntegrity(
                "persisted checkpoint offset is outside its run segment".into(),
            )
        } else {
            TaskStoreError::InvalidInput("checkpoint offset is outside its run segment".into())
        });
    }
    if let Some(blob_id) = checkpoint.context_blob_id {
        let owns_blob = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM blob_ownership WHERE task_id = ?1 AND blob_id = ?2
             )",
            params![checkpoint.task_id.to_string(), blob_id.to_string()],
            |row| row.get::<_, bool>(0),
        )?;
        if !owns_blob {
            return Err(TaskStoreError::BlobOwnershipDenied {
                blob_id,
                task_id: checkpoint.task_id,
            });
        }
    }
    Ok(())
}

fn validate_checkpoint_tools(
    connection: &Connection,
    checkpoint: &TaskCheckpoint,
    persisted: bool,
) -> Result<(), TaskStoreError> {
    let canonical = incomplete_tools_for_segment(
        connection,
        checkpoint.task_id,
        checkpoint.run_segment_id,
        checkpoint.committed_global_offset,
        None,
    )
    .map_err(|error| {
        if persisted {
            TaskStoreError::ProjectionIntegrity(format!(
                "persisted checkpoint tool history is invalid: {error}"
            ))
        } else {
            error
        }
    })?;
    let mut recorded = checkpoint.incomplete_tool_use_ids.clone();
    recorded.sort_by_key(ToString::to_string);
    if recorded != canonical {
        return Err(if persisted {
            TaskStoreError::ProjectionIntegrity(
                "persisted checkpoint tools disagree with the canonical event stream".into(),
            )
        } else {
            TaskStoreError::InvalidInput(
                "checkpoint tools disagree with the canonical event stream".into(),
            )
        });
    }
    Ok(())
}

fn load_latest_checkpoint(
    connection: &Connection,
    task_id: TaskId,
) -> Result<Option<TaskCheckpoint>, TaskStoreError> {
    let row = connection
        .query_row(
            "SELECT checkpoint_id, run_segment_id, committed_global_offset, checkpoint_json
             FROM checkpoints
             WHERE task_id = ?1
             ORDER BY committed_global_offset DESC, created_at DESC, checkpoint_id DESC
             LIMIT 1",
            [task_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    row.map(
        |(stored_checkpoint_id, stored_segment_id, stored_offset, body)| {
            if body.len() > MAX_EVENT_PAYLOAD_BYTES {
                return Err(TaskStoreError::ProjectionIntegrity(
                    "persisted checkpoint payload exceeds 1 MiB".into(),
                ));
            }
            let checkpoint: TaskCheckpoint = serde_json::from_str(&body)?;
            if checkpoint.task_id != task_id
                || checkpoint.checkpoint_id.to_string() != stored_checkpoint_id
                || checkpoint.run_segment_id.to_string() != stored_segment_id
                || checkpoint.committed_global_offset != nonnegative_integer(stored_offset)?
            {
                return Err(TaskStoreError::ProjectionIntegrity(
                    "checkpoint columns disagree with checkpoint payload".into(),
                ));
            }
            validate_checkpoint_recovery_state(&checkpoint, true)?;
            validate_checkpoint_references(connection, &checkpoint, false, true)?;
            validate_checkpoint_tools(connection, &checkpoint, true)?;
            Ok(checkpoint)
        },
    )
    .transpose()
}

fn insert_checkpoint_in_transaction(
    transaction: &Transaction<'_>,
    checkpoint: &TaskCheckpoint,
) -> Result<(), TaskStoreError> {
    let body = encode_checkpoint(checkpoint)?;
    validate_checkpoint_references(transaction, checkpoint, true, false)?;
    validate_checkpoint_tools(transaction, checkpoint, false)?;
    transaction.execute(
        "INSERT INTO checkpoints (
            checkpoint_id, task_id, run_segment_id, committed_global_offset,
            checkpoint_json, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            checkpoint.checkpoint_id.to_string(),
            checkpoint.task_id.to_string(),
            checkpoint.run_segment_id.to_string(),
            sqlite_integer(checkpoint.committed_global_offset)?,
            body,
            checkpoint.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn roll_forward_safe_checkpoint_in_transaction(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    committed: &[TaskEventEnvelope],
) -> Result<(), TaskStoreError> {
    if !committed.iter().any(is_safe_checkpoint_boundary) {
        return Ok(());
    }
    let projection = load_task_projection(transaction, task_id)?.ok_or_else(|| {
        TaskStoreError::ProjectionIntegrity("checkpoint boundary has no task projection".into())
    })?;
    let run = projection.current_run.as_ref().ok_or_else(|| {
        TaskStoreError::ProjectionIntegrity("checkpoint boundary has no run segment".into())
    })?;
    let prior = load_latest_checkpoint(transaction, task_id)?;
    let mut child_actor_refs = Vec::new();
    let mut statement = transaction.prepare(
        "SELECT actor_id
         FROM subagent_projection
         WHERE task_id = ?1
         ORDER BY last_global_offset ASC, actor_id ASC",
    )?;
    let rows = statement.query_map([task_id.to_string()], |row| row.get::<_, String>(0))?;
    for row in rows {
        child_actor_refs.push(ActorId::parse(&row?)?);
    }
    drop(statement);
    let checkpoint = TaskCheckpoint {
        checkpoint_id: CheckpointId::new(),
        task_id,
        run_segment_id: run.segment_id,
        committed_global_offset: projection.last_global_offset,
        context_cursor: prior.as_ref().map_or(0, |value| value.context_cursor),
        queue_revision: projection
            .queue
            .iter()
            .map(|item| item.revision)
            .max()
            .unwrap_or(0),
        workspace_baseline: prior
            .as_ref()
            .and_then(|value| value.workspace_baseline.clone()),
        incomplete_tool_use_ids: incomplete_tools_for_segment(
            transaction,
            task_id,
            run.segment_id,
            projection.last_global_offset,
            prior.as_ref(),
        )?,
        child_actor_refs,
        context_blob_id: prior.as_ref().and_then(|value| value.context_blob_id),
        created_at: now(),
    };
    insert_checkpoint_in_transaction(transaction, &checkpoint)?;
    Ok(())
}

fn roll_forward_subagent_checkpoint_if_running(
    transaction: &Transaction<'_>,
    task_id: TaskId,
    committed: &[TaskEventEnvelope],
) -> Result<(), TaskStoreError> {
    let running = load_task_projection(transaction, task_id)?
        .and_then(|projection| projection.current_run)
        .is_some_and(|run| {
            matches!(
                run.state,
                harness_contracts::RunState::Running
                    | harness_contracts::RunState::WaitingPermission
                    | harness_contracts::RunState::Yielding
            )
        });
    if running {
        roll_forward_safe_checkpoint_in_transaction(transaction, task_id, committed)?;
    }
    Ok(())
}

fn is_safe_checkpoint_boundary(event: &TaskEventEnvelope) -> bool {
    matches!(
        event.event_type.as_str(),
        "run.started"
            | "message.consumed"
            | "engine.tool_use_completed"
            | "engine.tool_use_failed"
            | "engine.tool_use_denied"
            | "permission.requested"
            | "permission.resolved"
            | "permission.invalidated"
            | "subagent.spawned"
            | "run.yield_requested"
            | "run.safe_point_reached"
            | "run.force_stop_timed_out"
            | "run.completed"
            | "tool.indeterminate"
    ) || event.event_type.starts_with("subagent.")
}

fn incomplete_tools_for_segment(
    connection: &Connection,
    task_id: TaskId,
    segment_id: RunSegmentId,
    committed_global_offset: u64,
    prior: Option<&TaskCheckpoint>,
) -> Result<Vec<ToolUseId>, TaskStoreError> {
    let (after_offset, mut incomplete) = if let Some(prior) = prior.filter(|checkpoint| {
        checkpoint.run_segment_id == segment_id
            && checkpoint.committed_global_offset <= committed_global_offset
    }) {
        (
            sqlite_integer(prior.committed_global_offset)?,
            prior
                .incomplete_tool_use_ids
                .iter()
                .copied()
                .collect::<HashSet<_>>(),
        )
    } else {
        let run_start_offset = connection.query_row(
            "SELECT global_offset
             FROM event_log
             WHERE task_id = ?1
               AND event_type = 'run.started'
               AND json_extract(payload_json, '$.segmentId') = ?2",
            params![task_id.to_string(), segment_id.to_string()],
            |row| row.get(0),
        )?;
        (run_start_offset, HashSet::new())
    };
    let mut statement = connection.prepare(
        "SELECT event_type, schema_version, payload_json
         FROM event_log
         WHERE task_id = ?1
           AND global_offset > ?2
           AND global_offset <= ?3
           AND (event_type GLOB 'engine.tool_use_*' OR event_type = 'tool.indeterminate')
         ORDER BY global_offset ASC",
    )?;
    let rows = statement.query_map(
        params![
            task_id.to_string(),
            after_offset,
            sqlite_integer(committed_global_offset)?
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u16>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    for row in rows {
        let (event_type, schema_version, payload_json) = row?;
        let event = TaskEvent::decode(
            &event_type,
            schema_version,
            serde_json::from_str(&payload_json)?,
        )?;
        match event {
            TaskEvent::Engine { payload, .. } => match payload.event {
                Event::ToolUseStarted(event) => {
                    incomplete.insert(event.tool_use_id);
                }
                Event::ToolUseCompleted(event) => {
                    incomplete.remove(&event.tool_use_id);
                }
                Event::ToolUseFailed(event) => {
                    incomplete.remove(&event.tool_use_id);
                }
                Event::ToolUseDenied(event) => {
                    incomplete.remove(&event.tool_use_id);
                }
                _ => {}
            },
            TaskEvent::ToolIndeterminate { tool_use_id, .. } => {
                incomplete.insert(tool_use_id);
            }
            _ => {}
        }
    }
    let mut incomplete = incomplete.into_iter().collect::<Vec<_>>();
    incomplete.sort_by_key(ToString::to_string);
    Ok(incomplete)
}

#[derive(Debug, Error)]
pub enum TaskStoreError {
    #[error("wrong expected task stream version: expected {expected}, actual {actual}")]
    WrongExpectedVersion { expected: u64, actual: u64 },
    #[error("wrong expected engine event offset: expected {expected}, actual {actual}")]
    EngineOffsetMismatch { expected: u64, actual: u64 },
    #[error("command {command_id} conflicts with an existing inbox entry")]
    CommandConflict { command_id: CommandId },
    #[error("command {command_id} has no durable outcome")]
    IncompleteCommand { command_id: CommandId },
    #[error("task projector failed: {0}")]
    Projector(String),
    #[error("task projection integrity check failed: {0}")]
    ProjectionIntegrity(String),
    #[error("rebuilt task projections differ from the committed projections")]
    ProjectionMismatch,
    #[error("unsupported task event {event_type} schema version {schema_version}")]
    UnsupportedEvent {
        event_type: String,
        schema_version: u16,
    },
    #[error("invalid task store input: {0}")]
    InvalidInput(String),
    #[error("task blob not found: {blob_id}")]
    BlobNotFound { blob_id: BlobId },
    #[error("task {task_id} does not own blob {blob_id}")]
    BlobOwnershipDenied { blob_id: BlobId, task_id: TaskId },
    #[error("task blob integrity check failed: {0}")]
    BlobIntegrity(String),
    #[error("task store integer is outside SQLite's signed range")]
    IntegerOutOfRange,
    #[error("task store connection lock was poisoned")]
    LockPoisoned,
    #[error("invalid task timestamp: {0}")]
    InvalidTimestamp(String),
    #[error(transparent)]
    InvalidId(#[from] IdParseError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Filesystem(#[from] harness_fs::FsError),
}

fn workspace_mode_wire(mode: &WorkspaceMode) -> &'static str {
    match mode {
        WorkspaceMode::Current => "current",
        WorkspaceMode::ManagedWorktree => "managed_worktree",
    }
}

fn workspace_lease_state_wire(state: TaskWorkspaceLeaseState) -> &'static str {
    match state {
        TaskWorkspaceLeaseState::Preparing => "preparing",
        TaskWorkspaceLeaseState::Waiting => "waiting",
        TaskWorkspaceLeaseState::Active => "active",
        TaskWorkspaceLeaseState::CleanupPending => "cleanup_pending",
        TaskWorkspaceLeaseState::CleanupBlocked => "cleanup_blocked",
        TaskWorkspaceLeaseState::Released => "released",
        TaskWorkspaceLeaseState::Expired => "expired",
    }
}

fn workspace_lease_projection(lease: &TaskWorkspaceLease) -> WorkspaceLeaseProjection {
    WorkspaceLeaseProjection {
        lease_id: lease.lease_id,
        task_id: lease.task_id,
        actor_id: lease.actor_id,
        mode: lease.mode.clone(),
        canonical_root: lease.canonical_root.clone(),
        worktree_path: lease.worktree_path.clone(),
        branch: lease.branch.clone(),
        writable: lease.writable,
        state: lease.state,
        requested_at: lease.requested_at,
        acquired_at: lease.acquired_at,
        expires_at: lease.expires_at,
        baseline_commit: lease.baseline_commit.clone(),
        baseline_status: lease.baseline_status.clone(),
        patch_path: lease.patch_path.clone(),
    }
}

fn update_workspace_lease_in_transaction(
    transaction: &Transaction<'_>,
    lease: &TaskWorkspaceLease,
) -> Result<(), TaskStoreError> {
    let changed = transaction.execute(
        "UPDATE workspace_leases
         SET state = ?2, acquired_at = ?3, expires_at = ?4, lease_json = ?5
         WHERE workspace_lease_id = ?1",
        params![
            lease.lease_id.to_string(),
            workspace_lease_state_wire(lease.state),
            lease.acquired_at.map(|value| value.to_rfc3339()),
            lease.expires_at.map(|value| value.to_rfc3339()),
            serde_json::to_string(lease)?,
        ],
    )?;
    if changed != 1 {
        return Err(TaskStoreError::ProjectionIntegrity(format!(
            "workspace lease {} disappeared during transition",
            lease.lease_id
        )));
    }
    Ok(())
}

fn workspace_lease_in_transaction(
    transaction: &Transaction<'_>,
    lease_id: WorkspaceLeaseId,
) -> Result<TaskWorkspaceLease, TaskStoreError> {
    let json = transaction
        .query_row(
            "SELECT lease_json FROM workspace_leases WHERE workspace_lease_id = ?1",
            [lease_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| {
            TaskStoreError::InvalidInput(format!("workspace lease {lease_id} does not exist"))
        })?;
    Ok(serde_json::from_str(&json)?)
}

fn active_workspace_leases_in_transaction(
    transaction: &Transaction<'_>,
    canonical_root: &str,
) -> Result<Vec<TaskWorkspaceLease>, TaskStoreError> {
    let mut statement = transaction.prepare(
        "SELECT lease_json FROM workspace_leases
         WHERE canonical_root = ?1 AND state = 'active'
         ORDER BY acquired_at ASC, workspace_lease_id ASC",
    )?;
    let rows = statement.query_map([canonical_root], |row| row.get::<_, String>(0))?;
    rows.map(|row| {
        let json = row?;
        serde_json::from_str(&json).map_err(TaskStoreError::from)
    })
    .collect()
}

fn append_and_project_workspace_events(
    transaction: &Transaction<'_>,
    projector: &dyn TaskProjector,
    task_id: TaskId,
    events: Vec<NewTaskEvent>,
) -> Result<Vec<TaskEventEnvelope>, TaskStoreError> {
    let expected_version = stream_version_in_transaction(transaction, task_id)?;
    let authority = TaskStore::supervisor_authority();
    let committed = append_in_transaction(
        transaction,
        task_id,
        expected_version,
        authority.source(),
        events,
    )?;
    for event in &committed {
        projector.apply(transaction, event)?;
    }
    roll_forward_safe_checkpoint_in_transaction(transaction, task_id, &committed)?;
    Ok(committed)
}

#[cfg(test)]
mod blob_reference_merge_tests {
    use harness_contracts::{BlobId, BlobRef};

    use super::*;

    #[test]
    fn repeated_blob_references_are_merged_and_conflicts_are_rejected() {
        let blob = BlobRef {
            id: BlobId::new(),
            size: 1,
            content_hash: [1; 32],
            content_type: Some("text/plain".into()),
        };
        let merged = merge_blob_references(vec![
            TaskBlobReference {
                blob_id: blob.id,
                expected: None,
            },
            TaskBlobReference {
                blob_id: blob.id,
                expected: Some(blob.clone()),
            },
            TaskBlobReference {
                blob_id: blob.id,
                expected: Some(blob.clone()),
            },
        ])
        .unwrap();
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].expected, Some(blob.clone()));

        let mut conflicting = blob.clone();
        conflicting.size += 1;
        assert!(matches!(
            merge_blob_references(vec![
                TaskBlobReference {
                    blob_id: blob.id,
                    expected: Some(blob),
                },
                TaskBlobReference {
                    blob_id: conflicting.id,
                    expected: Some(conflicting),
                },
            ]),
            Err(TaskStoreError::BlobIntegrity(_))
        ));
    }
}

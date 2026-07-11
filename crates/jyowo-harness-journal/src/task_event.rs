//! Typed events accepted by the unified task store.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use harness_contracts::{
    ActorId, BlobId, BlobRef, CausationId, CommandId, ConversationAttachmentReference,
    CorrelationId, Event, EventSource, EventSourceKind, IndeterminateToolDecision, MessageContent,
    MessagePart, PermissionProjection, QueueItemId, ReferenceKind, RequestId, RunId, RunSegmentId,
    RunTerminalReason, SessionId, SubagentParentProjection, SubagentProjection, TenantId,
    ToolResult, ToolResultPart, ToolUseId, WorkspaceLeaseId, WorkspaceLeaseProjection,
    WorkspaceLeaseState, WorkspaceSelection,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::TaskStoreError;

pub(crate) const MAX_EVENT_PAYLOAD_BYTES: usize = 1024 * 1024;
const MAX_TITLE_CHARS: usize = 4096;
const MAX_MESSAGE_CONTENT_BYTES: usize = 64 * 1024;
const MAX_MESSAGE_ATTACHMENTS: usize = 64;
const MAX_CONTEXT_REFERENCES: usize = 64;
const MAX_CONTEXT_REFERENCE_BYTES: usize = 4096;
const MAX_SIDE_EFFECTS: usize = 256;
const MAX_WORKSPACE_ROOT_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub struct NewTaskEvent {
    pub(crate) event: TaskEvent,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TaskEvent {
    TaskCreated {
        title: String,
        workspace: Option<WorkspaceSelection>,
    },
    TaskTitleChanged {
        title: String,
    },
    TaskArchived {
        archived: bool,
    },
    TaskActorFailed {
        segment_id: Option<RunSegmentId>,
        failed_at: DateTime<Utc>,
    },
    RunStarted {
        segment_id: RunSegmentId,
        started_at: DateTime<Utc>,
        recovery_start: bool,
        indeterminate_tools: Vec<IndeterminateToolDecision>,
    },
    RunCompleted {
        segment_id: RunSegmentId,
        ended_at: DateTime<Utc>,
        terminal_reason: RunTerminalReason,
        incomplete_output: bool,
    },
    RunYieldRequested {
        segment_id: RunSegmentId,
        force: bool,
        requested_at: DateTime<Utc>,
    },
    RunSafePointReached {
        segment_id: RunSegmentId,
        forced: bool,
        incomplete_output: bool,
        non_revertible_tool_use_ids: Vec<ToolUseId>,
        reached_at: DateTime<Utc>,
    },
    RunForceStopTimedOut {
        segment_id: RunSegmentId,
        indeterminate_tool_use_ids: Vec<ToolUseId>,
        timed_out_at: DateTime<Utc>,
    },
    ToolIndeterminate {
        run_segment_id: RunSegmentId,
        tool_use_id: ToolUseId,
        detected_at: DateTime<Utc>,
    },
    MessageQueued {
        queue_item_id: QueueItemId,
        content: String,
        attachments: Vec<BlobId>,
        context_references: Vec<String>,
        model_config_id: Option<String>,
        permission_mode: harness_contracts::PermissionMode,
        created_at: DateTime<Utc>,
    },
    MessageEdited {
        queue_item_id: QueueItemId,
        revision: u64,
        content: String,
        attachments: Vec<BlobId>,
        context_references: Vec<String>,
    },
    MessagePromoted {
        queue_item_id: QueueItemId,
        revision: u64,
    },
    MessageDeleted {
        queue_item_id: QueueItemId,
        revision: u64,
    },
    MessageRecovered {
        queue_item_id: QueueItemId,
        revision: u64,
    },
    MessageConsumed {
        queue_item_id: QueueItemId,
        revision: u64,
        run_segment_id: RunSegmentId,
    },
    PermissionRequested {
        permission: PermissionProjection,
    },
    PermissionResolved {
        request_id: RequestId,
        revision: u64,
        option_id: Option<String>,
    },
    PermissionInvalidated {
        request_id: RequestId,
        revision: u64,
        reason: String,
    },
    SubagentSpawned {
        actor_id: ActorId,
        started_at: DateTime<Utc>,
        child: Option<SubagentProjection>,
    },
    SubagentLinked {
        actor_id: ActorId,
        context_cursor: u64,
        parent: SubagentParentProjection,
    },
    SubagentStateChanged {
        child: SubagentProjection,
    },
    SubagentSummaryUpdated {
        child: SubagentProjection,
    },
    SubagentBackgrounded {
        child: SubagentProjection,
    },
    SubagentTerminal {
        child: SubagentProjection,
    },
    WorkspacePreparing {
        lease: WorkspaceLeaseProjection,
    },
    WorkspaceAcquired {
        lease: WorkspaceLeaseProjection,
    },
    WorkspaceWaiting {
        lease: WorkspaceLeaseProjection,
    },
    WorkspaceReleased {
        lease: WorkspaceLeaseProjection,
        reason: String,
        released_at: DateTime<Utc>,
    },
    WorkspaceCleanupBlocked {
        lease: WorkspaceLeaseProjection,
        blocked_at: DateTime<Utc>,
    },
    WorkspaceCleanupPending {
        lease: WorkspaceLeaseProjection,
    },
    WorkspaceOverrideApplied {
        command_id: CommandId,
        lease_id: WorkspaceLeaseId,
        canonical_path: String,
        reason: String,
        applied_at: DateTime<Utc>,
    },
    Engine {
        event_type: String,
        payload: EngineEventPayload,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EngineEventPayload {
    pub(crate) tenant_id: TenantId,
    pub(crate) session_id: SessionId,
    pub(crate) journal_offset: u64,
    pub(crate) run_id: Option<RunId>,
    pub(crate) correlation_id: CorrelationId,
    pub(crate) causation_id: Option<CausationId>,
    pub(crate) event: Event,
}

#[derive(Debug, Clone)]
pub(crate) struct TaskBlobReference {
    pub(crate) blob_id: BlobId,
    pub(crate) expected: Option<BlobRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TitlePayload {
    title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskCreatedPayload {
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace: Option<WorkspaceSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArchivedPayload {
    archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskActorFailedPayload {
    segment_id: Option<RunSegmentId>,
    failed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunStartedPayload {
    segment_id: RunSegmentId,
    started_at: DateTime<Utc>,
    #[serde(default)]
    recovery_start: bool,
    #[serde(default)]
    indeterminate_tools: Vec<IndeterminateToolDecision>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunCompletedPayload {
    segment_id: RunSegmentId,
    ended_at: DateTime<Utc>,
    terminal_reason: RunTerminalReason,
    incomplete_output: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunYieldRequestedPayload {
    segment_id: RunSegmentId,
    force: bool,
    requested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunSafePointReachedPayload {
    segment_id: RunSegmentId,
    forced: bool,
    incomplete_output: bool,
    non_revertible_tool_use_ids: Vec<ToolUseId>,
    reached_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunForceStopTimedOutPayload {
    segment_id: RunSegmentId,
    indeterminate_tool_use_ids: Vec<ToolUseId>,
    timed_out_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ToolIndeterminatePayload {
    run_segment_id: RunSegmentId,
    tool_use_id: ToolUseId,
    detected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MessageQueuedPayload {
    queue_item_id: QueueItemId,
    content: String,
    attachments: Vec<BlobId>,
    context_references: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model_config_id: Option<String>,
    #[serde(default)]
    permission_mode: harness_contracts::PermissionMode,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MessageEditedPayload {
    queue_item_id: QueueItemId,
    revision: u64,
    content: String,
    attachments: Vec<BlobId>,
    context_references: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MessageConsumedPayload {
    queue_item_id: QueueItemId,
    revision: u64,
    run_segment_id: RunSegmentId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueueStateChangedPayload {
    queue_item_id: QueueItemId,
    revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PermissionResolvedPayload {
    request_id: RequestId,
    revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    option_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PermissionInvalidatedPayload {
    request_id: RequestId,
    revision: u64,
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubagentSpawnedPayload {
    actor_id: ActorId,
    started_at: DateTime<Utc>,
    #[serde(default)]
    child: Option<SubagentProjection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubagentLinkedPayload {
    actor_id: ActorId,
    context_cursor: u64,
    parent: SubagentParentProjection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkspaceReleasedPayload {
    lease: WorkspaceLeaseProjection,
    reason: String,
    released_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkspaceCleanupBlockedPayload {
    lease: WorkspaceLeaseProjection,
    blocked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkspaceOverrideAppliedPayload {
    command_id: CommandId,
    lease_id: WorkspaceLeaseId,
    canonical_path: String,
    reason: String,
    applied_at: DateTime<Utc>,
}

impl NewTaskEvent {
    #[must_use]
    pub fn task_created(title: impl Into<String>) -> Self {
        Self {
            event: TaskEvent::TaskCreated {
                title: title.into(),
                workspace: None,
            },
        }
    }

    #[must_use]
    pub fn task_created_in_workspace(
        title: impl Into<String>,
        workspace: WorkspaceSelection,
    ) -> Self {
        Self {
            event: TaskEvent::TaskCreated {
                title: title.into(),
                workspace: Some(workspace),
            },
        }
    }

    #[must_use]
    pub fn task_title_changed(title: impl Into<String>) -> Self {
        Self {
            event: TaskEvent::TaskTitleChanged {
                title: title.into(),
            },
        }
    }

    #[must_use]
    pub const fn task_archived(archived: bool) -> Self {
        Self {
            event: TaskEvent::TaskArchived { archived },
        }
    }

    #[must_use]
    pub const fn task_actor_failed(
        segment_id: Option<RunSegmentId>,
        failed_at: DateTime<Utc>,
    ) -> Self {
        Self {
            event: TaskEvent::TaskActorFailed {
                segment_id,
                failed_at,
            },
        }
    }

    #[must_use]
    pub const fn run_started(segment_id: RunSegmentId, started_at: DateTime<Utc>) -> Self {
        Self {
            event: TaskEvent::RunStarted {
                segment_id,
                started_at,
                recovery_start: false,
                indeterminate_tools: Vec::new(),
            },
        }
    }

    #[must_use]
    pub fn run_started_with_recovery(
        segment_id: RunSegmentId,
        started_at: DateTime<Utc>,
        indeterminate_tools: Vec<IndeterminateToolDecision>,
    ) -> Self {
        Self {
            event: TaskEvent::RunStarted {
                segment_id,
                started_at,
                recovery_start: true,
                indeterminate_tools,
            },
        }
    }

    #[must_use]
    pub const fn run_completed(
        segment_id: RunSegmentId,
        ended_at: DateTime<Utc>,
        terminal_reason: RunTerminalReason,
        incomplete_output: bool,
    ) -> Self {
        Self {
            event: TaskEvent::RunCompleted {
                segment_id,
                ended_at,
                terminal_reason,
                incomplete_output,
            },
        }
    }

    #[must_use]
    pub const fn run_yield_requested(
        segment_id: RunSegmentId,
        force: bool,
        requested_at: DateTime<Utc>,
    ) -> Self {
        Self {
            event: TaskEvent::RunYieldRequested {
                segment_id,
                force,
                requested_at,
            },
        }
    }

    #[must_use]
    pub fn run_safe_point_reached(
        segment_id: RunSegmentId,
        forced: bool,
        incomplete_output: bool,
        non_revertible_tool_use_ids: Vec<ToolUseId>,
        reached_at: DateTime<Utc>,
    ) -> Self {
        Self {
            event: TaskEvent::RunSafePointReached {
                segment_id,
                forced,
                incomplete_output,
                non_revertible_tool_use_ids,
                reached_at,
            },
        }
    }

    #[must_use]
    pub fn run_force_stop_timed_out(
        segment_id: RunSegmentId,
        indeterminate_tool_use_ids: Vec<ToolUseId>,
        timed_out_at: DateTime<Utc>,
    ) -> Self {
        Self {
            event: TaskEvent::RunForceStopTimedOut {
                segment_id,
                indeterminate_tool_use_ids,
                timed_out_at,
            },
        }
    }

    #[must_use]
    pub const fn tool_indeterminate(
        run_segment_id: RunSegmentId,
        tool_use_id: ToolUseId,
        detected_at: DateTime<Utc>,
    ) -> Self {
        Self {
            event: TaskEvent::ToolIndeterminate {
                run_segment_id,
                tool_use_id,
                detected_at,
            },
        }
    }

    #[must_use]
    pub fn message_queued(
        queue_item_id: QueueItemId,
        content: impl Into<String>,
        attachments: Vec<BlobId>,
        context_references: Vec<String>,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self::message_queued_with_runtime(
            queue_item_id,
            content,
            attachments,
            context_references,
            None,
            harness_contracts::PermissionMode::Default,
            created_at,
        )
    }

    #[must_use]
    pub fn message_queued_with_runtime(
        queue_item_id: QueueItemId,
        content: impl Into<String>,
        attachments: Vec<BlobId>,
        context_references: Vec<String>,
        model_config_id: Option<String>,
        permission_mode: harness_contracts::PermissionMode,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            event: TaskEvent::MessageQueued {
                queue_item_id,
                content: content.into(),
                attachments,
                context_references,
                model_config_id,
                permission_mode,
                created_at,
            },
        }
    }

    #[must_use]
    pub fn message_edited(
        queue_item_id: QueueItemId,
        revision: u64,
        content: impl Into<String>,
        attachments: Vec<BlobId>,
        context_references: Vec<String>,
    ) -> Self {
        Self {
            event: TaskEvent::MessageEdited {
                queue_item_id,
                revision,
                content: content.into(),
                attachments,
                context_references,
            },
        }
    }

    #[must_use]
    pub const fn message_promoted(queue_item_id: QueueItemId, revision: u64) -> Self {
        Self {
            event: TaskEvent::MessagePromoted {
                queue_item_id,
                revision,
            },
        }
    }

    #[must_use]
    pub const fn message_deleted(queue_item_id: QueueItemId, revision: u64) -> Self {
        Self {
            event: TaskEvent::MessageDeleted {
                queue_item_id,
                revision,
            },
        }
    }

    #[must_use]
    pub const fn message_recovered(queue_item_id: QueueItemId, revision: u64) -> Self {
        Self {
            event: TaskEvent::MessageRecovered {
                queue_item_id,
                revision,
            },
        }
    }

    #[must_use]
    pub const fn message_consumed(
        queue_item_id: QueueItemId,
        revision: u64,
        run_segment_id: RunSegmentId,
    ) -> Self {
        Self {
            event: TaskEvent::MessageConsumed {
                queue_item_id,
                revision,
                run_segment_id,
            },
        }
    }

    #[must_use]
    pub const fn permission_requested(permission: PermissionProjection) -> Self {
        Self {
            event: TaskEvent::PermissionRequested { permission },
        }
    }

    #[must_use]
    pub const fn permission_resolved(request_id: RequestId, revision: u64) -> Self {
        Self {
            event: TaskEvent::PermissionResolved {
                request_id,
                revision,
                option_id: None,
            },
        }
    }

    #[must_use]
    pub fn permission_resolved_with_option(
        request_id: RequestId,
        revision: u64,
        option_id: impl Into<String>,
    ) -> Self {
        Self {
            event: TaskEvent::PermissionResolved {
                request_id,
                revision,
                option_id: Some(option_id.into()),
            },
        }
    }

    #[must_use]
    pub fn permission_invalidated(
        request_id: RequestId,
        revision: u64,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            event: TaskEvent::PermissionInvalidated {
                request_id,
                revision,
                reason: reason.into(),
            },
        }
    }

    #[must_use]
    pub const fn subagent_spawned(actor_id: ActorId, started_at: DateTime<Utc>) -> Self {
        Self {
            event: TaskEvent::SubagentSpawned {
                actor_id,
                started_at,
                child: None,
            },
        }
    }

    #[must_use]
    pub fn subagent_actor_spawned(child: SubagentProjection) -> Self {
        Self {
            event: TaskEvent::SubagentSpawned {
                actor_id: child.actor_id,
                started_at: child.started_at,
                child: Some(child),
            },
        }
    }

    #[must_use]
    pub const fn subagent_linked(
        actor_id: ActorId,
        context_cursor: u64,
        parent: SubagentParentProjection,
    ) -> Self {
        Self {
            event: TaskEvent::SubagentLinked {
                actor_id,
                context_cursor,
                parent,
            },
        }
    }

    #[must_use]
    pub const fn subagent_state_changed(child: SubagentProjection) -> Self {
        Self {
            event: TaskEvent::SubagentStateChanged { child },
        }
    }

    #[must_use]
    pub const fn subagent_summary_updated(child: SubagentProjection) -> Self {
        Self {
            event: TaskEvent::SubagentSummaryUpdated { child },
        }
    }

    #[must_use]
    pub const fn subagent_backgrounded(child: SubagentProjection) -> Self {
        Self {
            event: TaskEvent::SubagentBackgrounded { child },
        }
    }

    #[must_use]
    pub const fn subagent_terminal(child: SubagentProjection) -> Self {
        Self {
            event: TaskEvent::SubagentTerminal { child },
        }
    }

    #[must_use]
    pub const fn workspace_acquired(lease: WorkspaceLeaseProjection) -> Self {
        Self {
            event: TaskEvent::WorkspaceAcquired { lease },
        }
    }

    #[must_use]
    pub const fn workspace_preparing(lease: WorkspaceLeaseProjection) -> Self {
        Self {
            event: TaskEvent::WorkspacePreparing { lease },
        }
    }

    #[must_use]
    pub const fn workspace_waiting(lease: WorkspaceLeaseProjection) -> Self {
        Self {
            event: TaskEvent::WorkspaceWaiting { lease },
        }
    }

    #[must_use]
    pub fn workspace_released(
        lease: WorkspaceLeaseProjection,
        reason: impl Into<String>,
        released_at: DateTime<Utc>,
    ) -> Self {
        Self {
            event: TaskEvent::WorkspaceReleased {
                lease,
                reason: reason.into(),
                released_at,
            },
        }
    }

    #[must_use]
    pub fn workspace_cleanup_blocked(
        lease: WorkspaceLeaseProjection,
        blocked_at: DateTime<Utc>,
    ) -> Self {
        Self {
            event: TaskEvent::WorkspaceCleanupBlocked { lease, blocked_at },
        }
    }

    #[must_use]
    pub const fn workspace_cleanup_pending(lease: WorkspaceLeaseProjection) -> Self {
        Self {
            event: TaskEvent::WorkspaceCleanupPending { lease },
        }
    }

    #[must_use]
    pub fn workspace_override_applied(
        command_id: CommandId,
        lease_id: WorkspaceLeaseId,
        canonical_path: impl Into<String>,
        reason: impl Into<String>,
        applied_at: DateTime<Utc>,
    ) -> Self {
        Self {
            event: TaskEvent::WorkspaceOverrideApplied {
                command_id,
                lease_id,
                canonical_path: canonical_path.into(),
                reason: reason.into(),
                applied_at,
            },
        }
    }

    pub(crate) fn engine(
        tenant_id: TenantId,
        session_id: SessionId,
        journal_offset: u64,
        run_id: Option<RunId>,
        correlation_id: CorrelationId,
        causation_id: Option<CausationId>,
        event: Event,
    ) -> Result<Self, TaskStoreError> {
        let event_type = engine_event_type(&event)?;
        Ok(Self {
            event: TaskEvent::Engine {
                event_type,
                payload: EngineEventPayload {
                    tenant_id,
                    session_id,
                    journal_offset,
                    run_id,
                    correlation_id,
                    causation_id,
                    event,
                },
            },
        })
    }

    pub fn from_parts(
        event_type: &str,
        schema_version: u16,
        payload: Value,
    ) -> Result<Self, TaskStoreError> {
        if serde_json::to_vec(&payload)?.len() > MAX_EVENT_PAYLOAD_BYTES {
            return Err(TaskStoreError::InvalidInput(
                "task event payload exceeds 1 MiB".into(),
            ));
        }
        Ok(Self {
            event: TaskEvent::decode(event_type, schema_version, payload)?,
        })
    }

    pub(crate) fn encode(&self) -> Result<(String, u16, Value), TaskStoreError> {
        self.event.validate_shape()?;
        let payload = self.event.payload()?;
        if serde_json::to_vec(&payload)?.len() > MAX_EVENT_PAYLOAD_BYTES {
            return Err(TaskStoreError::InvalidInput(
                "task event payload exceeds 1 MiB".into(),
            ));
        }
        Ok((self.event.event_type().to_owned(), 1, payload))
    }

    pub(crate) fn validate_source(&self, source: &EventSource) -> Result<(), TaskStoreError> {
        self.event.validate_source(source)
    }

    pub(crate) fn blob_references(&self) -> Result<Vec<TaskBlobReference>, TaskStoreError> {
        match &self.event {
            TaskEvent::MessageQueued { attachments, .. }
            | TaskEvent::MessageEdited { attachments, .. } => Ok(attachments
                .iter()
                .copied()
                .map(|blob_id| TaskBlobReference {
                    blob_id,
                    expected: None,
                })
                .collect()),
            TaskEvent::Engine { payload, .. } => Ok(engine_blob_references(&payload.event)?
                .into_iter()
                .map(|blob| TaskBlobReference {
                    blob_id: blob.id,
                    expected: Some(blob),
                })
                .collect()),
            _ => Ok(Vec::new()),
        }
    }
}

fn engine_blob_references(event: &Event) -> Result<Vec<BlobRef>, TaskStoreError> {
    let mut references = Vec::new();
    match event {
        Event::RunStarted(event) => {
            collect_message_parts_blob_references(&event.input.message.parts, &mut references);
            if let Some(attachments) = event.input.metadata.get("attachments") {
                let attachments = serde_json::from_value::<Vec<ConversationAttachmentReference>>(
                    attachments.clone(),
                )
                .map_err(|_| {
                    TaskStoreError::InvalidInput(
                        "engine run input attachments metadata is invalid".into(),
                    )
                })?;
                references.extend(
                    attachments
                        .into_iter()
                        .map(|attachment| attachment.blob_ref),
                );
            }
        }
        Event::UserMessageAppended(event) => {
            references.extend(
                event
                    .attachments
                    .iter()
                    .map(|attachment| attachment.blob_ref.clone()),
            );
            collect_message_content_blob_references(&event.content, &mut references);
        }
        Event::AssistantMessageCompleted(event) => {
            collect_message_content_blob_references(&event.content, &mut references);
        }
        Event::ArtifactCreated(event) => {
            references.extend(event.blob_ref.iter().cloned());
        }
        Event::ArtifactUpdated(event) => {
            references.extend(event.blob_ref.iter().cloned());
        }
        Event::ToolUseCompleted(event) => {
            collect_tool_result_blob_references(&event.result, &mut references);
        }
        Event::ToolResultOffloaded(event) => references.push(event.blob_ref.clone()),
        Event::HookReturnedAdditionalContext(event) => {
            references.extend(event.context_blob.iter().cloned());
        }
        Event::CompactionApplied(event) => {
            references.push(event.summary_ref.clone());
            if let Some(handoff) = &event.handoff {
                references.push(handoff.active_task_ref.clone());
            }
        }
        Event::TeamMemberJoined(event) => references.push(event.spec_snapshot_id.clone()),
        Event::SubagentAnnounced(event) => {
            references.extend(
                event
                    .transcript_ref
                    .iter()
                    .map(|transcript| transcript.blob.clone()),
            );
        }
        Event::TeamTurnCompleted(event) => {
            references.extend(
                event
                    .transcript_ref
                    .iter()
                    .map(|transcript| transcript.blob.clone()),
            );
        }
        Event::SandboxExecutionCompleted(event) => {
            references.extend(
                event
                    .overflow
                    .iter()
                    .flat_map(|overflow| overflow.blob_ref.iter())
                    .cloned(),
            );
        }
        Event::SandboxOutputSpilled(event) => references.push(event.blob_ref.clone()),
        Event::ExecuteCodeStepInvoked(event) => {
            references.extend(
                event
                    .overflow
                    .iter()
                    .map(|overflow| overflow.blob_ref.clone()),
            );
        }
        Event::SteeringMessageQueued(event) => {
            references.extend(event.body_blob.iter().cloned());
        }
        _ => {}
    }
    Ok(references)
}

fn collect_message_content_blob_references(
    content: &MessageContent,
    references: &mut Vec<BlobRef>,
) {
    if let MessageContent::Multimodal(parts) = content {
        collect_message_parts_blob_references(parts, references);
    }
}

fn collect_message_parts_blob_references(parts: &[MessagePart], references: &mut Vec<BlobRef>) {
    for part in parts {
        match part {
            MessagePart::Image { blob_ref, .. }
            | MessagePart::Video { blob_ref, .. }
            | MessagePart::File { blob_ref, .. } => references.push(blob_ref.clone()),
            MessagePart::ToolResult { content, .. } => {
                collect_tool_result_blob_references(content, references);
            }
            _ => {}
        }
    }
}

fn collect_tool_result_blob_references(result: &ToolResult, references: &mut Vec<BlobRef>) {
    match result {
        ToolResult::Blob { blob_ref, .. } => references.push(blob_ref.clone()),
        ToolResult::Mixed(parts) => {
            for part in parts {
                match part {
                    ToolResultPart::Blob { blob_ref, .. }
                    | ToolResultPart::Artifact { blob_ref, .. } => {
                        references.push(blob_ref.clone());
                    }
                    ToolResultPart::Reference {
                        reference_kind: ReferenceKind::Transcript(transcript),
                        ..
                    } => references.push(transcript.blob.clone()),
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

impl TaskEvent {
    pub(crate) fn decode(
        event_type: &str,
        schema_version: u16,
        payload: Value,
    ) -> Result<Self, TaskStoreError> {
        if schema_version != 1 {
            return Err(TaskStoreError::UnsupportedEvent {
                event_type: event_type.into(),
                schema_version,
            });
        }
        let event: Result<Self, TaskStoreError> = match event_type {
            "task.created" => {
                let value: TaskCreatedPayload = serde_json::from_value(payload)?;
                Ok(Self::TaskCreated {
                    title: value.title,
                    workspace: value.workspace,
                })
            }
            "task.title_changed" => {
                let value: TitlePayload = serde_json::from_value(payload)?;
                Ok(Self::TaskTitleChanged { title: value.title })
            }
            "task.archived" => {
                let value: ArchivedPayload = serde_json::from_value(payload)?;
                Ok(Self::TaskArchived {
                    archived: value.archived,
                })
            }
            "task.actor_failed" => {
                let value: TaskActorFailedPayload = serde_json::from_value(payload)?;
                Ok(Self::TaskActorFailed {
                    segment_id: value.segment_id,
                    failed_at: value.failed_at,
                })
            }
            "run.started" => {
                let value: RunStartedPayload = serde_json::from_value(payload)?;
                Ok(Self::RunStarted {
                    segment_id: value.segment_id,
                    started_at: value.started_at,
                    recovery_start: value.recovery_start,
                    indeterminate_tools: value.indeterminate_tools,
                })
            }
            "run.completed" => {
                let value: RunCompletedPayload = serde_json::from_value(payload)?;
                Ok(Self::RunCompleted {
                    segment_id: value.segment_id,
                    ended_at: value.ended_at,
                    terminal_reason: value.terminal_reason,
                    incomplete_output: value.incomplete_output,
                })
            }
            "run.yield_requested" => {
                let value: RunYieldRequestedPayload = serde_json::from_value(payload)?;
                Ok(Self::RunYieldRequested {
                    segment_id: value.segment_id,
                    force: value.force,
                    requested_at: value.requested_at,
                })
            }
            "run.safe_point_reached" => {
                let value: RunSafePointReachedPayload = serde_json::from_value(payload)?;
                Ok(Self::RunSafePointReached {
                    segment_id: value.segment_id,
                    forced: value.forced,
                    incomplete_output: value.incomplete_output,
                    non_revertible_tool_use_ids: value.non_revertible_tool_use_ids,
                    reached_at: value.reached_at,
                })
            }
            "run.force_stop_timed_out" => {
                let value: RunForceStopTimedOutPayload = serde_json::from_value(payload)?;
                Ok(Self::RunForceStopTimedOut {
                    segment_id: value.segment_id,
                    indeterminate_tool_use_ids: value.indeterminate_tool_use_ids,
                    timed_out_at: value.timed_out_at,
                })
            }
            "tool.indeterminate" => {
                let value: ToolIndeterminatePayload = serde_json::from_value(payload)?;
                Ok(Self::ToolIndeterminate {
                    run_segment_id: value.run_segment_id,
                    tool_use_id: value.tool_use_id,
                    detected_at: value.detected_at,
                })
            }
            "message.queued" => {
                let value: MessageQueuedPayload = serde_json::from_value(payload)?;
                Ok(Self::MessageQueued {
                    queue_item_id: value.queue_item_id,
                    content: value.content,
                    attachments: value.attachments,
                    context_references: value.context_references,
                    model_config_id: value.model_config_id,
                    permission_mode: value.permission_mode,
                    created_at: value.created_at,
                })
            }
            "message.edited" => {
                let value: MessageEditedPayload = serde_json::from_value(payload)?;
                Ok(Self::MessageEdited {
                    queue_item_id: value.queue_item_id,
                    revision: value.revision,
                    content: value.content,
                    attachments: value.attachments,
                    context_references: value.context_references,
                })
            }
            "message.promoted" | "message.deleted" | "message.recovered" => {
                let value: QueueStateChangedPayload = serde_json::from_value(payload)?;
                Ok(match event_type {
                    "message.promoted" => Self::MessagePromoted {
                        queue_item_id: value.queue_item_id,
                        revision: value.revision,
                    },
                    "message.deleted" => Self::MessageDeleted {
                        queue_item_id: value.queue_item_id,
                        revision: value.revision,
                    },
                    "message.recovered" => Self::MessageRecovered {
                        queue_item_id: value.queue_item_id,
                        revision: value.revision,
                    },
                    _ => unreachable!(),
                })
            }
            "message.consumed" => {
                let value: MessageConsumedPayload = serde_json::from_value(payload)?;
                Ok(Self::MessageConsumed {
                    queue_item_id: value.queue_item_id,
                    revision: value.revision,
                    run_segment_id: value.run_segment_id,
                })
            }
            "permission.requested" => Ok(Self::PermissionRequested {
                permission: serde_json::from_value(payload)?,
            }),
            "permission.resolved" => {
                let value: PermissionResolvedPayload = serde_json::from_value(payload)?;
                Ok(Self::PermissionResolved {
                    request_id: value.request_id,
                    revision: value.revision,
                    option_id: value.option_id,
                })
            }
            "permission.invalidated" => {
                let value: PermissionInvalidatedPayload = serde_json::from_value(payload)?;
                Ok(Self::PermissionInvalidated {
                    request_id: value.request_id,
                    revision: value.revision,
                    reason: value.reason,
                })
            }
            "subagent.spawned" => {
                let value: SubagentSpawnedPayload = serde_json::from_value(payload)?;
                Ok(Self::SubagentSpawned {
                    actor_id: value.actor_id,
                    started_at: value.started_at,
                    child: value.child,
                })
            }
            "subagent.linked" => {
                let value: SubagentLinkedPayload = serde_json::from_value(payload)?;
                Ok(Self::SubagentLinked {
                    actor_id: value.actor_id,
                    context_cursor: value.context_cursor,
                    parent: value.parent,
                })
            }
            "subagent.state_changed" => Ok(Self::SubagentStateChanged {
                child: serde_json::from_value(payload)?,
            }),
            "subagent.summary_updated" => Ok(Self::SubagentSummaryUpdated {
                child: serde_json::from_value(payload)?,
            }),
            "subagent.backgrounded" => Ok(Self::SubagentBackgrounded {
                child: serde_json::from_value(payload)?,
            }),
            "subagent.terminal" => Ok(Self::SubagentTerminal {
                child: serde_json::from_value(payload)?,
            }),
            "workspace.preparing" => Ok(Self::WorkspacePreparing {
                lease: serde_json::from_value(payload)?,
            }),
            "workspace.acquired" => Ok(Self::WorkspaceAcquired {
                lease: serde_json::from_value(payload)?,
            }),
            "workspace.waiting" => Ok(Self::WorkspaceWaiting {
                lease: serde_json::from_value(payload)?,
            }),
            "workspace.released" => {
                let value: WorkspaceReleasedPayload = serde_json::from_value(payload)?;
                Ok(Self::WorkspaceReleased {
                    lease: value.lease,
                    reason: value.reason,
                    released_at: value.released_at,
                })
            }
            "workspace.cleanup_blocked" => {
                let value: WorkspaceCleanupBlockedPayload = serde_json::from_value(payload)?;
                Ok(Self::WorkspaceCleanupBlocked {
                    lease: value.lease,
                    blocked_at: value.blocked_at,
                })
            }
            "workspace.cleanup_pending" => Ok(Self::WorkspaceCleanupPending {
                lease: serde_json::from_value(payload)?,
            }),
            "workspace.override_applied" => {
                let value: WorkspaceOverrideAppliedPayload = serde_json::from_value(payload)?;
                Ok(Self::WorkspaceOverrideApplied {
                    command_id: value.command_id,
                    lease_id: value.lease_id,
                    canonical_path: value.canonical_path,
                    reason: value.reason,
                    applied_at: value.applied_at,
                })
            }
            event_type if event_type.starts_with("engine.") => {
                let value: EngineEventPayload = serde_json::from_value(payload)?;
                let actual = engine_event_type(&value.event)?;
                if actual != event_type {
                    return Err(TaskStoreError::InvalidInput(format!(
                        "engine event payload type {actual} does not match {event_type}"
                    )));
                }
                Ok(Self::Engine {
                    event_type: event_type.to_owned(),
                    payload: value,
                })
            }
            _ => {
                return Err(TaskStoreError::UnsupportedEvent {
                    event_type: event_type.into(),
                    schema_version,
                });
            }
        };
        let event = event?;
        event.validate_shape()?;
        Ok(event)
    }

    pub(crate) fn event_type(&self) -> &str {
        match self {
            Self::TaskCreated { .. } => "task.created",
            Self::TaskTitleChanged { .. } => "task.title_changed",
            Self::TaskArchived { .. } => "task.archived",
            Self::TaskActorFailed { .. } => "task.actor_failed",
            Self::RunStarted { .. } => "run.started",
            Self::RunCompleted { .. } => "run.completed",
            Self::RunYieldRequested { .. } => "run.yield_requested",
            Self::RunSafePointReached { .. } => "run.safe_point_reached",
            Self::RunForceStopTimedOut { .. } => "run.force_stop_timed_out",
            Self::ToolIndeterminate { .. } => "tool.indeterminate",
            Self::MessageQueued { .. } => "message.queued",
            Self::MessageEdited { .. } => "message.edited",
            Self::MessagePromoted { .. } => "message.promoted",
            Self::MessageDeleted { .. } => "message.deleted",
            Self::MessageRecovered { .. } => "message.recovered",
            Self::MessageConsumed { .. } => "message.consumed",
            Self::PermissionRequested { .. } => "permission.requested",
            Self::PermissionResolved { .. } => "permission.resolved",
            Self::PermissionInvalidated { .. } => "permission.invalidated",
            Self::SubagentSpawned { .. } => "subagent.spawned",
            Self::SubagentLinked { .. } => "subagent.linked",
            Self::SubagentStateChanged { .. } => "subagent.state_changed",
            Self::SubagentSummaryUpdated { .. } => "subagent.summary_updated",
            Self::SubagentBackgrounded { .. } => "subagent.backgrounded",
            Self::SubagentTerminal { .. } => "subagent.terminal",
            Self::WorkspacePreparing { .. } => "workspace.preparing",
            Self::WorkspaceAcquired { .. } => "workspace.acquired",
            Self::WorkspaceWaiting { .. } => "workspace.waiting",
            Self::WorkspaceReleased { .. } => "workspace.released",
            Self::WorkspaceCleanupBlocked { .. } => "workspace.cleanup_blocked",
            Self::WorkspaceCleanupPending { .. } => "workspace.cleanup_pending",
            Self::WorkspaceOverrideApplied { .. } => "workspace.override_applied",
            Self::Engine { event_type, .. } => event_type,
        }
    }

    fn payload(&self) -> Result<Value, TaskStoreError> {
        Ok(match self {
            Self::TaskCreated { title, workspace } => serde_json::to_value(TaskCreatedPayload {
                title: title.clone(),
                workspace: workspace.clone(),
            })?,
            Self::TaskTitleChanged { title } => serde_json::to_value(TitlePayload {
                title: title.clone(),
            })?,
            Self::TaskArchived { archived } => serde_json::to_value(ArchivedPayload {
                archived: *archived,
            })?,
            Self::TaskActorFailed {
                segment_id,
                failed_at,
            } => serde_json::to_value(TaskActorFailedPayload {
                segment_id: *segment_id,
                failed_at: *failed_at,
            })?,
            Self::RunStarted {
                segment_id,
                started_at,
                recovery_start,
                indeterminate_tools,
            } => serde_json::to_value(RunStartedPayload {
                segment_id: *segment_id,
                started_at: *started_at,
                recovery_start: *recovery_start,
                indeterminate_tools: indeterminate_tools.clone(),
            })?,
            Self::RunCompleted {
                segment_id,
                ended_at,
                terminal_reason,
                incomplete_output,
            } => serde_json::to_value(RunCompletedPayload {
                segment_id: *segment_id,
                ended_at: *ended_at,
                terminal_reason: terminal_reason.clone(),
                incomplete_output: *incomplete_output,
            })?,
            Self::RunYieldRequested {
                segment_id,
                force,
                requested_at,
            } => serde_json::to_value(RunYieldRequestedPayload {
                segment_id: *segment_id,
                force: *force,
                requested_at: *requested_at,
            })?,
            Self::RunSafePointReached {
                segment_id,
                forced,
                incomplete_output,
                non_revertible_tool_use_ids,
                reached_at,
            } => serde_json::to_value(RunSafePointReachedPayload {
                segment_id: *segment_id,
                forced: *forced,
                incomplete_output: *incomplete_output,
                non_revertible_tool_use_ids: non_revertible_tool_use_ids.clone(),
                reached_at: *reached_at,
            })?,
            Self::RunForceStopTimedOut {
                segment_id,
                indeterminate_tool_use_ids,
                timed_out_at,
            } => serde_json::to_value(RunForceStopTimedOutPayload {
                segment_id: *segment_id,
                indeterminate_tool_use_ids: indeterminate_tool_use_ids.clone(),
                timed_out_at: *timed_out_at,
            })?,
            Self::ToolIndeterminate {
                run_segment_id,
                tool_use_id,
                detected_at,
            } => serde_json::to_value(ToolIndeterminatePayload {
                run_segment_id: *run_segment_id,
                tool_use_id: *tool_use_id,
                detected_at: *detected_at,
            })?,
            Self::MessageQueued {
                queue_item_id,
                content,
                attachments,
                context_references,
                model_config_id,
                permission_mode,
                created_at,
            } => serde_json::to_value(MessageQueuedPayload {
                queue_item_id: *queue_item_id,
                content: content.clone(),
                attachments: attachments.clone(),
                context_references: context_references.clone(),
                model_config_id: model_config_id.clone(),
                permission_mode: *permission_mode,
                created_at: *created_at,
            })?,
            Self::MessageEdited {
                queue_item_id,
                revision,
                content,
                attachments,
                context_references,
            } => serde_json::to_value(MessageEditedPayload {
                queue_item_id: *queue_item_id,
                revision: *revision,
                content: content.clone(),
                attachments: attachments.clone(),
                context_references: context_references.clone(),
            })?,
            Self::MessagePromoted {
                queue_item_id,
                revision,
            }
            | Self::MessageDeleted {
                queue_item_id,
                revision,
            }
            | Self::MessageRecovered {
                queue_item_id,
                revision,
            } => serde_json::to_value(QueueStateChangedPayload {
                queue_item_id: *queue_item_id,
                revision: *revision,
            })?,
            Self::MessageConsumed {
                queue_item_id,
                revision,
                run_segment_id,
            } => serde_json::to_value(MessageConsumedPayload {
                queue_item_id: *queue_item_id,
                revision: *revision,
                run_segment_id: *run_segment_id,
            })?,
            Self::PermissionRequested { permission } => serde_json::to_value(permission)?,
            Self::PermissionResolved {
                request_id,
                revision,
                option_id,
            } => serde_json::to_value(PermissionResolvedPayload {
                request_id: *request_id,
                revision: *revision,
                option_id: option_id.clone(),
            })?,
            Self::PermissionInvalidated {
                request_id,
                revision,
                reason,
            } => serde_json::to_value(PermissionInvalidatedPayload {
                request_id: *request_id,
                revision: *revision,
                reason: reason.clone(),
            })?,
            Self::SubagentSpawned {
                actor_id,
                started_at,
                child,
            } => serde_json::to_value(SubagentSpawnedPayload {
                actor_id: *actor_id,
                started_at: *started_at,
                child: child.clone(),
            })?,
            Self::SubagentLinked {
                actor_id,
                context_cursor,
                parent,
            } => serde_json::to_value(SubagentLinkedPayload {
                actor_id: *actor_id,
                context_cursor: *context_cursor,
                parent: parent.clone(),
            })?,
            Self::SubagentStateChanged { child }
            | Self::SubagentSummaryUpdated { child }
            | Self::SubagentBackgrounded { child }
            | Self::SubagentTerminal { child } => serde_json::to_value(child)?,
            Self::WorkspacePreparing { lease } => serde_json::to_value(lease)?,
            Self::WorkspaceAcquired { lease } => serde_json::to_value(lease)?,
            Self::WorkspaceWaiting { lease } => serde_json::to_value(lease)?,
            Self::WorkspaceReleased {
                lease,
                reason,
                released_at,
            } => serde_json::to_value(WorkspaceReleasedPayload {
                lease: lease.clone(),
                reason: reason.clone(),
                released_at: *released_at,
            })?,
            Self::WorkspaceCleanupBlocked { lease, blocked_at } => {
                serde_json::to_value(WorkspaceCleanupBlockedPayload {
                    lease: lease.clone(),
                    blocked_at: *blocked_at,
                })?
            }
            Self::WorkspaceCleanupPending { lease } => serde_json::to_value(lease)?,
            Self::WorkspaceOverrideApplied {
                command_id,
                lease_id,
                canonical_path,
                reason,
                applied_at,
            } => serde_json::to_value(WorkspaceOverrideAppliedPayload {
                command_id: *command_id,
                lease_id: *lease_id,
                canonical_path: canonical_path.clone(),
                reason: reason.clone(),
                applied_at: *applied_at,
            })?,
            Self::Engine { payload, .. } => serde_json::to_value(payload)?,
        })
    }

    pub(crate) fn validate_source(&self, source: &EventSource) -> Result<(), TaskStoreError> {
        if source.kind == EventSourceKind::User && source.client_id.is_none() {
            return Err(TaskStoreError::InvalidInput(
                "user task events require a client id".into(),
            ));
        }
        if source.kind == EventSourceKind::Subagent && source.actor_id.is_none() {
            return Err(TaskStoreError::InvalidInput(
                "subagent task events require an actor id".into(),
            ));
        }
        let allowed = match self {
            Self::TaskCreated { .. }
            | Self::TaskTitleChanged { .. }
            | Self::TaskArchived { .. }
            | Self::MessageQueued { .. }
            | Self::MessageEdited { .. }
            | Self::MessagePromoted { .. }
            | Self::MessageDeleted { .. } => matches!(
                source.kind,
                EventSourceKind::User | EventSourceKind::Supervisor | EventSourceKind::Recovery
            ),
            Self::MessageRecovered { .. } => source.kind == EventSourceKind::Recovery,
            Self::MessageConsumed { .. } => matches!(
                source.kind,
                EventSourceKind::Supervisor | EventSourceKind::Recovery
            ),
            Self::RunStarted { .. }
            | Self::RunCompleted { .. }
            | Self::RunYieldRequested { .. }
            | Self::RunSafePointReached { .. } => matches!(
                source.kind,
                EventSourceKind::Engine | EventSourceKind::Supervisor | EventSourceKind::Recovery
            ),
            Self::RunForceStopTimedOut { .. } => matches!(
                source.kind,
                EventSourceKind::Supervisor | EventSourceKind::Recovery
            ),
            Self::ToolIndeterminate { .. } => source.kind == EventSourceKind::Recovery,
            Self::TaskActorFailed { .. } => source.kind == EventSourceKind::Supervisor,
            Self::PermissionRequested { .. } | Self::PermissionResolved { .. } => {
                source.kind == EventSourceKind::PermissionBroker
            }
            Self::PermissionInvalidated { .. } => matches!(
                source.kind,
                EventSourceKind::PermissionBroker
                    | EventSourceKind::Supervisor
                    | EventSourceKind::Recovery
            ),
            Self::SubagentSpawned { .. }
            | Self::WorkspacePreparing { .. }
            | Self::WorkspaceAcquired { .. }
            | Self::WorkspaceWaiting { .. }
            | Self::WorkspaceReleased { .. }
            | Self::WorkspaceCleanupBlocked { .. }
            | Self::WorkspaceCleanupPending { .. } => matches!(
                source.kind,
                EventSourceKind::Supervisor | EventSourceKind::Recovery
            ),
            Self::SubagentLinked { .. }
            | Self::SubagentStateChanged { .. }
            | Self::SubagentSummaryUpdated { .. }
            | Self::SubagentBackgrounded { .. }
            | Self::SubagentTerminal { .. } => matches!(
                source.kind,
                EventSourceKind::Subagent | EventSourceKind::Supervisor | EventSourceKind::Recovery
            ),
            Self::WorkspaceOverrideApplied { .. } => source.kind == EventSourceKind::User,
            Self::Engine { .. } => source.kind == EventSourceKind::Engine,
        };
        if !allowed {
            return Err(TaskStoreError::InvalidInput(format!(
                "source {:?} cannot emit {}",
                source.kind,
                self.event_type()
            )));
        }
        Ok(())
    }

    fn validate_shape(&self) -> Result<(), TaskStoreError> {
        match self {
            Self::RunStarted {
                indeterminate_tools,
                ..
            } => {
                let unique = indeterminate_tools
                    .iter()
                    .map(|decision| ToolUseId::parse(&decision.tool_use_id))
                    .collect::<Result<HashSet<_>, _>>()?;
                if indeterminate_tools.len() > MAX_SIDE_EFFECTS
                    || unique.len() != indeterminate_tools.len()
                {
                    return Err(TaskStoreError::InvalidInput(
                        "run recovery decisions exceed their count limit or contain duplicates"
                            .into(),
                    ));
                }
            }
            Self::TaskCreated { title, workspace } => {
                if title.chars().count() > MAX_TITLE_CHARS {
                    return Err(TaskStoreError::InvalidInput(format!(
                        "task title may contain at most {MAX_TITLE_CHARS} characters"
                    )));
                }
                if workspace.as_ref().is_some_and(|selection| {
                    selection.root.trim().is_empty()
                        || selection.root.len() > MAX_WORKSPACE_ROOT_BYTES
                }) {
                    return Err(TaskStoreError::InvalidInput(
                        "task workspace root must be non-empty and bounded".into(),
                    ));
                }
            }
            Self::TaskTitleChanged { title } => {
                if title.chars().count() > MAX_TITLE_CHARS {
                    return Err(TaskStoreError::InvalidInput(format!(
                        "task title may contain at most {MAX_TITLE_CHARS} characters"
                    )));
                }
            }
            Self::MessageQueued {
                content,
                attachments,
                context_references,
                ..
            }
            | Self::MessageEdited {
                content,
                attachments,
                context_references,
                ..
            } => {
                if content.len() > MAX_MESSAGE_CONTENT_BYTES {
                    return Err(TaskStoreError::InvalidInput(
                        "message content exceeds 64 KiB".into(),
                    ));
                }
                if attachments.len() > MAX_MESSAGE_ATTACHMENTS {
                    return Err(TaskStoreError::InvalidInput(format!(
                        "a message may contain at most {MAX_MESSAGE_ATTACHMENTS} attachments"
                    )));
                }
                if context_references.len() > MAX_CONTEXT_REFERENCES
                    || context_references
                        .iter()
                        .any(|reference| reference.len() > MAX_CONTEXT_REFERENCE_BYTES)
                {
                    return Err(TaskStoreError::InvalidInput(
                        "message context references exceed their count or size limit".into(),
                    ));
                }
            }
            Self::Engine {
                event_type,
                payload,
            } => {
                let actual = engine_event_type(&payload.event)?;
                if &actual != event_type {
                    return Err(TaskStoreError::InvalidInput(format!(
                        "engine event payload type {actual} does not match {event_type}"
                    )));
                }
            }
            Self::RunSafePointReached {
                non_revertible_tool_use_ids,
                ..
            } => {
                if non_revertible_tool_use_ids.len() > MAX_SIDE_EFFECTS {
                    return Err(TaskStoreError::InvalidInput(
                        "run side effects exceed their count limit".into(),
                    ));
                }
            }
            Self::RunForceStopTimedOut {
                indeterminate_tool_use_ids,
                ..
            } => {
                if indeterminate_tool_use_ids.is_empty()
                    || indeterminate_tool_use_ids.len() > MAX_SIDE_EFFECTS
                {
                    return Err(TaskStoreError::InvalidInput(
                        "indeterminate tools must contain 1 to 256 identifiers".into(),
                    ));
                }
            }
            Self::PermissionInvalidated { reason, .. } if reason.len() > 256 => {
                return Err(TaskStoreError::InvalidInput(
                    "permission invalidation reason exceeds 256 bytes".into(),
                ));
            }
            Self::WorkspacePreparing { lease } if lease.state != WorkspaceLeaseState::Preparing => {
                return Err(TaskStoreError::InvalidInput(
                    "workspace.preparing requires preparing state".into(),
                ));
            }
            Self::WorkspaceWaiting { lease } if lease.state != WorkspaceLeaseState::Waiting => {
                return Err(TaskStoreError::InvalidInput(
                    "workspace.waiting requires waiting state".into(),
                ));
            }
            Self::WorkspaceAcquired { lease } if lease.state != WorkspaceLeaseState::Active => {
                return Err(TaskStoreError::InvalidInput(
                    "workspace.acquired requires active state".into(),
                ));
            }
            Self::WorkspaceCleanupPending { lease }
                if lease.state != WorkspaceLeaseState::CleanupPending =>
            {
                return Err(TaskStoreError::InvalidInput(
                    "workspace.cleanup_pending requires cleanup_pending state".into(),
                ));
            }
            Self::WorkspaceCleanupBlocked { lease, .. }
                if lease.state != WorkspaceLeaseState::CleanupBlocked =>
            {
                return Err(TaskStoreError::InvalidInput(
                    "workspace.cleanup_blocked requires cleanup_blocked state".into(),
                ));
            }
            Self::WorkspaceReleased { lease, .. }
                if !matches!(
                    lease.state,
                    WorkspaceLeaseState::Released | WorkspaceLeaseState::Expired
                ) =>
            {
                return Err(TaskStoreError::InvalidInput(
                    "workspace.released requires released or expired state".into(),
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

fn engine_event_type(event: &Event) -> Result<String, TaskStoreError> {
    let value = serde_json::to_value(event)?;
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| TaskStoreError::InvalidInput("engine event type is missing".into()))?;
    Ok(format!("engine.{event_type}"))
}

#[cfg(test)]
mod blob_reference_tests {
    use harness_contracts::{
        now, BlobId, BlobRef, JournalOffset, ReferenceKind, SessionId, SubagentAnnouncedEvent,
        SubagentId, SubagentStatus, ToolResult, ToolResultPart, ToolUseCompletedEvent, ToolUseId,
        TranscriptRef, UsageSnapshot,
    };

    use super::*;

    #[test]
    fn engine_blob_visitor_collects_subagent_and_tool_result_transcripts() {
        let subagent_blob = blob_ref(1);
        let tool_blob = blob_ref(2);
        let subagent = Event::SubagentAnnounced(SubagentAnnouncedEvent {
            subagent_id: SubagentId::new(),
            parent_session_id: SessionId::new(),
            status: SubagentStatus::Completed,
            summary: "done".into(),
            result: None,
            usage: UsageSnapshot::default(),
            transcript_ref: Some(transcript(subagent_blob.clone())),
            context_report: None,
            renderer_id: "default".into(),
            at: now(),
        });
        let tool_result = Event::ToolUseCompleted(ToolUseCompletedEvent {
            tool_use_id: ToolUseId::new(),
            result: ToolResult::Mixed(vec![ToolResultPart::Reference {
                reference_kind: ReferenceKind::Transcript(transcript(tool_blob.clone())),
                title: None,
                summary: None,
            }]),
            usage: None,
            duration_ms: 1,
            at: now(),
        });

        assert_eq!(
            engine_blob_references(&subagent).unwrap(),
            vec![subagent_blob]
        );
        assert_eq!(
            engine_blob_references(&tool_result).unwrap(),
            vec![tool_blob]
        );
    }

    fn blob_ref(seed: u8) -> BlobRef {
        BlobRef {
            id: BlobId::new(),
            size: 1,
            content_hash: [seed; 32],
            content_type: Some("text/plain".into()),
        }
    }

    fn transcript(blob: BlobRef) -> TranscriptRef {
        TranscriptRef {
            blob,
            from_offset: JournalOffset(0),
            to_offset: JournalOffset(0),
        }
    }
}

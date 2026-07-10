//! Typed events accepted by the unified task store.

use chrono::{DateTime, Utc};
use harness_contracts::{
    ActorId, BlobId, EventSource, EventSourceKind, PermissionProjection, QueueItemId, RequestId,
    RunSegmentId, RunTerminalReason, WorkspaceLeaseProjection,
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

#[derive(Debug, Clone, PartialEq)]
pub struct NewTaskEvent {
    pub(crate) event: TaskEvent,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TaskEvent {
    TaskCreated {
        title: String,
    },
    TaskTitleChanged {
        title: String,
    },
    TaskArchived {
        archived: bool,
    },
    RunStarted {
        segment_id: RunSegmentId,
        started_at: DateTime<Utc>,
    },
    RunCompleted {
        segment_id: RunSegmentId,
        ended_at: DateTime<Utc>,
        terminal_reason: RunTerminalReason,
        incomplete_output: bool,
    },
    MessageQueued {
        queue_item_id: QueueItemId,
        content: String,
        attachments: Vec<BlobId>,
        context_references: Vec<String>,
        created_at: DateTime<Utc>,
    },
    MessageEdited {
        queue_item_id: QueueItemId,
        revision: u64,
        content: String,
        attachments: Vec<BlobId>,
        context_references: Vec<String>,
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
    },
    SubagentSpawned {
        actor_id: ActorId,
        started_at: DateTime<Utc>,
    },
    WorkspaceAcquired {
        lease: WorkspaceLeaseProjection,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TitlePayload {
    title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArchivedPayload {
    archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RunStartedPayload {
    segment_id: RunSegmentId,
    started_at: DateTime<Utc>,
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
struct MessageQueuedPayload {
    queue_item_id: QueueItemId,
    content: String,
    attachments: Vec<BlobId>,
    context_references: Vec<String>,
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
struct PermissionResolvedPayload {
    request_id: RequestId,
    revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubagentSpawnedPayload {
    actor_id: ActorId,
    started_at: DateTime<Utc>,
}

impl NewTaskEvent {
    #[must_use]
    pub fn task_created(title: impl Into<String>) -> Self {
        Self {
            event: TaskEvent::TaskCreated {
                title: title.into(),
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
    pub const fn run_started(segment_id: RunSegmentId, started_at: DateTime<Utc>) -> Self {
        Self {
            event: TaskEvent::RunStarted {
                segment_id,
                started_at,
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
    pub fn message_queued(
        queue_item_id: QueueItemId,
        content: impl Into<String>,
        attachments: Vec<BlobId>,
        context_references: Vec<String>,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            event: TaskEvent::MessageQueued {
                queue_item_id,
                content: content.into(),
                attachments,
                context_references,
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
            },
        }
    }

    #[must_use]
    pub const fn subagent_spawned(actor_id: ActorId, started_at: DateTime<Utc>) -> Self {
        Self {
            event: TaskEvent::SubagentSpawned {
                actor_id,
                started_at,
            },
        }
    }

    #[must_use]
    pub const fn workspace_acquired(lease: WorkspaceLeaseProjection) -> Self {
        Self {
            event: TaskEvent::WorkspaceAcquired { lease },
        }
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

    pub(crate) fn encode(&self) -> Result<(&'static str, u16, Value), TaskStoreError> {
        self.event.validate_shape()?;
        let payload = self.event.payload()?;
        if serde_json::to_vec(&payload)?.len() > MAX_EVENT_PAYLOAD_BYTES {
            return Err(TaskStoreError::InvalidInput(
                "task event payload exceeds 1 MiB".into(),
            ));
        }
        Ok((self.event.event_type(), 1, payload))
    }

    pub(crate) fn validate_source(&self, source: &EventSource) -> Result<(), TaskStoreError> {
        self.event.validate_source(source)
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
                let value: TitlePayload = serde_json::from_value(payload)?;
                Ok(Self::TaskCreated { title: value.title })
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
            "run.started" => {
                let value: RunStartedPayload = serde_json::from_value(payload)?;
                Ok(Self::RunStarted {
                    segment_id: value.segment_id,
                    started_at: value.started_at,
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
            "message.queued" => {
                let value: MessageQueuedPayload = serde_json::from_value(payload)?;
                Ok(Self::MessageQueued {
                    queue_item_id: value.queue_item_id,
                    content: value.content,
                    attachments: value.attachments,
                    context_references: value.context_references,
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
                })
            }
            "subagent.spawned" => {
                let value: SubagentSpawnedPayload = serde_json::from_value(payload)?;
                Ok(Self::SubagentSpawned {
                    actor_id: value.actor_id,
                    started_at: value.started_at,
                })
            }
            "workspace.acquired" => Ok(Self::WorkspaceAcquired {
                lease: serde_json::from_value(payload)?,
            }),
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

    pub(crate) const fn event_type(&self) -> &'static str {
        match self {
            Self::TaskCreated { .. } => "task.created",
            Self::TaskTitleChanged { .. } => "task.title_changed",
            Self::TaskArchived { .. } => "task.archived",
            Self::RunStarted { .. } => "run.started",
            Self::RunCompleted { .. } => "run.completed",
            Self::MessageQueued { .. } => "message.queued",
            Self::MessageEdited { .. } => "message.edited",
            Self::MessageConsumed { .. } => "message.consumed",
            Self::PermissionRequested { .. } => "permission.requested",
            Self::PermissionResolved { .. } => "permission.resolved",
            Self::SubagentSpawned { .. } => "subagent.spawned",
            Self::WorkspaceAcquired { .. } => "workspace.acquired",
        }
    }

    fn payload(&self) -> Result<Value, TaskStoreError> {
        Ok(match self {
            Self::TaskCreated { title } | Self::TaskTitleChanged { title } => {
                serde_json::to_value(TitlePayload {
                    title: title.clone(),
                })?
            }
            Self::TaskArchived { archived } => serde_json::to_value(ArchivedPayload {
                archived: *archived,
            })?,
            Self::RunStarted {
                segment_id,
                started_at,
            } => serde_json::to_value(RunStartedPayload {
                segment_id: *segment_id,
                started_at: *started_at,
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
            Self::MessageQueued {
                queue_item_id,
                content,
                attachments,
                context_references,
                created_at,
            } => serde_json::to_value(MessageQueuedPayload {
                queue_item_id: *queue_item_id,
                content: content.clone(),
                attachments: attachments.clone(),
                context_references: context_references.clone(),
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
            } => serde_json::to_value(PermissionResolvedPayload {
                request_id: *request_id,
                revision: *revision,
            })?,
            Self::SubagentSpawned {
                actor_id,
                started_at,
            } => serde_json::to_value(SubagentSpawnedPayload {
                actor_id: *actor_id,
                started_at: *started_at,
            })?,
            Self::WorkspaceAcquired { lease } => serde_json::to_value(lease)?,
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
            | Self::MessageConsumed { .. } => matches!(
                source.kind,
                EventSourceKind::User | EventSourceKind::Supervisor | EventSourceKind::Recovery
            ),
            Self::RunStarted { .. } | Self::RunCompleted { .. } => matches!(
                source.kind,
                EventSourceKind::Engine | EventSourceKind::Supervisor | EventSourceKind::Recovery
            ),
            Self::PermissionRequested { .. } => matches!(
                source.kind,
                EventSourceKind::PermissionBroker
                    | EventSourceKind::Engine
                    | EventSourceKind::Supervisor
            ),
            Self::PermissionResolved { .. } => matches!(
                source.kind,
                EventSourceKind::User
                    | EventSourceKind::PermissionBroker
                    | EventSourceKind::Supervisor
            ),
            Self::SubagentSpawned { .. } | Self::WorkspaceAcquired { .. } => matches!(
                source.kind,
                EventSourceKind::Supervisor | EventSourceKind::Recovery
            ),
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
            Self::TaskCreated { title } | Self::TaskTitleChanged { title } => {
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
            _ => {}
        }
        Ok(())
    }
}

use chrono::{DateTime, Utc};
use harness_contracts::{
    BlobId, PermissionMode, PromotionMode, QueueItemId, QueueItemProjection, QueueItemState,
    RunSegmentId, RunState, TaskProjection,
};
use harness_journal::{CommandRejection, NewTaskEvent};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueCommand {
    Submit {
        queue_item_id: QueueItemId,
        content: String,
        attachments: Vec<BlobId>,
        context_references: Vec<String>,
        created_at: DateTime<Utc>,
    },
    SubmitWithRuntime {
        queue_item_id: QueueItemId,
        content: String,
        attachments: Vec<BlobId>,
        context_references: Vec<String>,
        model_config_id: Option<String>,
        permission_mode: PermissionMode,
        created_at: DateTime<Utc>,
    },
    Edit {
        expected_revision: u64,
        content: String,
        attachments: Vec<BlobId>,
        context_references: Vec<String>,
    },
    Delete {
        expected_revision: u64,
    },
    Promote {
        expected_revision: u64,
        mode: PromotionMode,
    },
    Consume {
        expected_revision: u64,
        run_segment_id: RunSegmentId,
    },
    Recover,
}

impl QueueCommand {
    pub(crate) fn canonical_payload(&self, queue_item_id: QueueItemId) -> Value {
        match self {
            Self::Submit {
                content,
                attachments,
                context_references,
                created_at: _,
                ..
            } => json!({
                "type": "queue_submit",
                "content": content,
                "attachments": attachments,
                "contextReferences": context_references,
            }),
            Self::SubmitWithRuntime {
                content,
                attachments,
                context_references,
                model_config_id,
                permission_mode,
                created_at: _,
                ..
            } => json!({
                "type": "queue_submit",
                "content": content,
                "attachments": attachments,
                "contextReferences": context_references,
                "modelConfigId": model_config_id,
                "permissionMode": permission_mode,
            }),
            Self::Edit {
                expected_revision,
                content,
                attachments,
                context_references,
            } => json!({
                "type": "queue_edit",
                "queueItemId": queue_item_id,
                "expectedRevision": expected_revision,
                "content": content,
                "attachments": attachments,
                "contextReferences": context_references,
            }),
            Self::Delete { expected_revision } => json!({
                "type": "queue_delete",
                "queueItemId": queue_item_id,
                "expectedRevision": expected_revision,
            }),
            Self::Promote {
                expected_revision,
                mode,
            } => json!({
                "type": "queue_promote",
                "queueItemId": queue_item_id,
                "expectedRevision": expected_revision,
                "mode": mode,
            }),
            Self::Consume {
                expected_revision,
                run_segment_id,
            } => json!({
                "type": "queue_consume",
                "queueItemId": queue_item_id,
                "expectedRevision": expected_revision,
                "runSegmentId": run_segment_id,
            }),
            Self::Recover => json!({
                "type": "queue_recover",
                "queueItemId": queue_item_id,
            }),
        }
    }
}

pub fn decide_queue(
    current: Option<&QueueItemProjection>,
    command: QueueCommand,
) -> Result<Vec<NewTaskEvent>, CommandRejection> {
    match (current, command) {
        (
            None,
            QueueCommand::Submit {
                queue_item_id,
                content,
                attachments,
                context_references,
                created_at,
            },
        ) => Ok(vec![NewTaskEvent::message_queued(
            queue_item_id,
            content,
            attachments,
            context_references,
            created_at,
        )]),
        (
            None,
            QueueCommand::SubmitWithRuntime {
                queue_item_id,
                content,
                attachments,
                context_references,
                model_config_id,
                permission_mode,
                created_at,
            },
        ) => Ok(vec![NewTaskEvent::message_queued_with_runtime(
            queue_item_id,
            content,
            attachments,
            context_references,
            model_config_id,
            permission_mode,
            created_at,
        )]),
        (
            Some(item),
            QueueCommand::Edit {
                expected_revision,
                content,
                attachments,
                context_references,
            },
        ) if item.state == QueueItemState::Queued => {
            require_revision(item, expected_revision)?;
            let revision =
                item.revision
                    .checked_add(1)
                    .ok_or_else(|| CommandRejection::InvalidCommand {
                        message: "queue revision overflow".into(),
                    })?;
            Ok(vec![NewTaskEvent::message_edited(
                item.queue_item_id,
                revision,
                content,
                attachments,
                context_references,
            )])
        }
        (Some(item), QueueCommand::Delete { expected_revision })
            if item.state == QueueItemState::Queued =>
        {
            require_revision(item, expected_revision)?;
            Ok(vec![NewTaskEvent::message_deleted(
                item.queue_item_id,
                item.revision,
            )])
        }
        (
            Some(item),
            QueueCommand::Promote {
                expected_revision,
                mode: _,
            },
        ) if item.state == QueueItemState::Queued => {
            require_revision(item, expected_revision)?;
            Ok(vec![NewTaskEvent::message_promoted(
                item.queue_item_id,
                item.revision,
            )])
        }
        (
            Some(item),
            QueueCommand::Consume {
                expected_revision,
                run_segment_id,
            },
        ) if item.state == QueueItemState::Promoting => {
            require_revision(item, expected_revision)?;
            Ok(vec![NewTaskEvent::message_consumed(
                item.queue_item_id,
                item.revision,
                run_segment_id,
            )])
        }
        (Some(item), QueueCommand::Recover) if item.state == QueueItemState::Promoting => {
            Ok(vec![NewTaskEvent::message_recovered(
                item.queue_item_id,
                item.revision,
            )])
        }
        _ => Err(CommandRejection::InvalidCommand {
            message: "queue command is not allowed in the current state".into(),
        }),
    }
}

pub fn decide_consume_next(
    projection: &TaskProjection,
    segment_id: RunSegmentId,
    started_at: DateTime<Utc>,
) -> Result<Vec<NewTaskEvent>, CommandRejection> {
    if projection.current_run.as_ref().is_some_and(|run| {
        matches!(
            run.state,
            RunState::Running | RunState::WaitingPermission | RunState::Yielding
        )
    }) {
        return Err(CommandRejection::InvalidCommand {
            message: "normal queue consumption requires an idle task".into(),
        });
    }
    let item = projection
        .queue
        .iter()
        .filter(|item| item.state == QueueItemState::Queued)
        .min_by_key(|item| (item.created_global_offset, item.queue_item_id.to_string()))
        .ok_or_else(|| CommandRejection::InvalidCommand {
            message: "the task has no queued message to consume".into(),
        })?;
    Ok(vec![
        NewTaskEvent::run_started(segment_id, started_at),
        NewTaskEvent::message_consumed(item.queue_item_id, item.revision, segment_id),
    ])
}

fn require_revision(
    item: &QueueItemProjection,
    expected_revision: u64,
) -> Result<(), CommandRejection> {
    if item.revision != expected_revision {
        return Err(CommandRejection::StaleQueueRevision {
            latest: Box::new(item.clone()),
        });
    }
    Ok(())
}

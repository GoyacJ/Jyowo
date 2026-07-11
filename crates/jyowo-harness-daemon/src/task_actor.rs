use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use harness_contracts::{
    CommandId, IndeterminateToolDecision, PromotionMode, QueueItemId, QueueItemState, RunSegmentId,
    RunState, TaskId, ToolUseId,
};
use harness_engine::{RunControl, RunControlHandle};
use harness_journal::{
    AcceptedCommand, CommandOutcome, CommandRejection, NewTaskEvent, TaskCheckpoint, TaskStore,
    TaskStoreError, MAX_ACTIVE_QUEUE_ITEMS,
};
use serde_json::json;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, OwnedSemaphorePermit, Semaphore};

use crate::{
    decide_consume_next, decide_queue, CheckpointService, CheckpointState, QueueCommand,
    RunCoordinatorEvent, StartSegmentRequest, SubagentStopMode,
    WorkspaceBoundRunCoordinatorFactory,
};

#[derive(Debug)]
pub enum TaskActorMessage {
    Command(Box<ValidatedTaskCommand>, oneshot::Sender<CommandOutcome>),
    RunEvent(RunCoordinatorEvent),
    StartNextQueued(OwnedSemaphorePermit),
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum ValidatedTaskCommand {
    StartSegment {
        command: AcceptedCommand,
        segment_id: RunSegmentId,
        started_at: DateTime<Utc>,
    },
    ContinueTask {
        command: AcceptedCommand,
        segment_id: RunSegmentId,
        started_at: DateTime<Utc>,
        indeterminate_tools: Vec<IndeterminateToolDecision>,
    },
    Queue {
        command: AcceptedCommand,
        queue_item_id: QueueItemId,
        queue_command: QueueCommand,
    },
}

#[derive(Debug, Error)]
pub(crate) enum TaskActorError {
    #[error("task does not exist")]
    TaskNotFound,
    #[error("task store failed: {0}")]
    Store(#[from] TaskStoreError),
    #[error("task actor runtime state lock was poisoned")]
    RuntimeStatePoisoned,
    #[error("segment start delivery is no longer pending")]
    SegmentStartDeliveryNotPending,
    #[error("subagent stop propagation failed: {0}")]
    SubagentStop(#[source] harness_subagent::SubagentError),
}

impl ValidatedTaskCommand {
    pub(crate) fn rejected(&self, message: impl Into<String>) -> CommandOutcome {
        let command = match self {
            Self::StartSegment { command, .. }
            | Self::ContinueTask { command, .. }
            | Self::Queue { command, .. } => command,
        };
        CommandOutcome::Rejected {
            command_id: command.command_id,
            task_id: command.task_id,
            rejection: CommandRejection::InvalidCommand {
                message: message.into(),
            },
        }
    }
}

struct ActiveSegment {
    segment_id: RunSegmentId,
    control: RunControlHandle,
    permit: OwnedSemaphorePermit,
}

pub(crate) async fn run_task_actor(
    task_id: TaskId,
    store: Arc<TaskStore>,
    factory: Arc<WorkspaceBoundRunCoordinatorFactory>,
    foreground_runs: Arc<Semaphore>,
    active_segment_state: Arc<Mutex<Option<RunSegmentId>>>,
    pending_start_retry_segment: Option<RunSegmentId>,
    mailbox: mpsc::Sender<TaskActorMessage>,
    mut messages: mpsc::Receiver<TaskActorMessage>,
) -> Result<(), TaskActorError> {
    if store.task_projection(task_id)?.is_none() {
        return Err(TaskActorError::TaskNotFound);
    }
    let mut active = None::<ActiveSegment>;
    let mut retried_pending_start = false;
    if let Some(run) = store
        .task_projection(task_id)?
        .and_then(|projection| projection.current_run)
        .filter(|run| {
            matches!(
                run.state,
                RunState::Running | RunState::WaitingPermission | RunState::Yielding
            )
        })
    {
        if let Some(pending) = store.pending_segment_start(task_id, run.segment_id)? {
            retried_pending_start = true;
            let permit = Arc::clone(&foreground_runs)
                .acquire_owned()
                .await
                .map_err(|_| TaskActorError::RuntimeStatePoisoned)?;
            *active_segment_state
                .lock()
                .map_err(|_| TaskActorError::RuntimeStatePoisoned)? = Some(run.segment_id);
            let running = factory.spawn_idempotent(StartSegmentRequest {
                task_id: pending.task_id,
                segment_id: pending.segment_id,
                indeterminate_tools: pending.indeterminate_tools,
            });
            store.mark_segment_start_delivered(task_id, run.segment_id)?;
            let control = running.control();
            active = Some(ActiveSegment {
                segment_id: run.segment_id,
                control,
                permit,
            });
            forward_run_events(run.segment_id, mailbox.clone(), running.into_events());
        }
    }
    if pending_start_retry_segment.is_some() && !retried_pending_start {
        return Err(TaskActorError::SegmentStartDeliveryNotPending);
    }

    while let Some(message) = messages.recv().await {
        match message {
            TaskActorMessage::Command(command, reply) => {
                handle_command(
                    task_id,
                    &store,
                    &factory,
                    &foreground_runs,
                    &active_segment_state,
                    &mailbox,
                    &mut active,
                    *command,
                    reply,
                )
                .await?;
            }
            TaskActorMessage::RunEvent(event) => {
                if handle_run_event(
                    task_id,
                    &store,
                    &factory,
                    &active_segment_state,
                    &mailbox,
                    &mut active,
                    event,
                )? {
                    schedule_consume_next(Arc::clone(&foreground_runs), mailbox.clone());
                }
            }
            TaskActorMessage::StartNextQueued(permit) => {
                handle_start_next_queued(
                    task_id,
                    &store,
                    &factory,
                    &active_segment_state,
                    &mailbox,
                    &mut active,
                    permit,
                )?;
            }
            TaskActorMessage::Shutdown => break,
        }
    }
    Ok(())
}

async fn handle_command(
    task_id: TaskId,
    store: &TaskStore,
    factory: &Arc<WorkspaceBoundRunCoordinatorFactory>,
    foreground_runs: &Arc<Semaphore>,
    active_segment_state: &Mutex<Option<RunSegmentId>>,
    mailbox: &mpsc::Sender<TaskActorMessage>,
    active: &mut Option<ActiveSegment>,
    command: ValidatedTaskCommand,
    reply: oneshot::Sender<CommandOutcome>,
) -> Result<(), TaskActorError> {
    match command {
        ValidatedTaskCommand::StartSegment {
            command,
            segment_id,
            started_at,
        } => {
            handle_start_segment(
                task_id,
                store,
                factory,
                foreground_runs,
                active_segment_state,
                mailbox,
                active,
                command,
                segment_id,
                started_at,
                None,
                reply,
            )?;
        }
        ValidatedTaskCommand::ContinueTask {
            mut command,
            segment_id,
            started_at,
            indeterminate_tools,
        } => {
            command.payload = json!({
                "type": "continue_task",
                "segmentId": segment_id,
                "indeterminateTools": indeterminate_tools,
            });
            handle_start_segment(
                task_id,
                store,
                factory,
                foreground_runs,
                active_segment_state,
                mailbox,
                active,
                command,
                segment_id,
                started_at,
                Some(indeterminate_tools),
                reply,
            )?;
        }
        ValidatedTaskCommand::Queue {
            mut command,
            queue_item_id,
            queue_command,
        } => {
            if command.task_id != task_id {
                let _ = reply.send(CommandOutcome::Rejected {
                    command_id: command.command_id,
                    task_id: command.task_id,
                    rejection: CommandRejection::InvalidCommand {
                        message: "command task does not match actor task".into(),
                    },
                });
                return Ok(());
            }
            if matches!(
                &queue_command,
                QueueCommand::Submit {
                    queue_item_id: submitted_item_id,
                    ..
                } if *submitted_item_id != queue_item_id
            ) {
                let _ = reply.send(CommandOutcome::Rejected {
                    command_id: command.command_id,
                    task_id: command.task_id,
                    rejection: CommandRejection::InvalidCommand {
                        message: "queue command item identity does not match its address".into(),
                    },
                });
                return Ok(());
            }
            if matches!(
                queue_command,
                QueueCommand::Consume { .. } | QueueCommand::Recover
            ) {
                let _ = reply.send(CommandOutcome::Rejected {
                    command_id: command.command_id,
                    task_id: command.task_id,
                    rejection: CommandRejection::InvalidCommand {
                        message: "queue consume and recovery are daemon-internal commands".into(),
                    },
                });
                return Ok(());
            }
            command.payload = queue_command.canonical_payload(queue_item_id);
            let command_id = command.command_id;
            let command_task_id = command.task_id;
            let is_submit = matches!(&queue_command, QueueCommand::Submit { .. });
            let promotion_mode = match &queue_command {
                QueueCommand::Promote { mode, .. } => Some(mode.clone()),
                _ => None,
            };
            if promotion_mode.is_some() {
                command.authority = TaskStore::supervisor_command_authority(&command.authority);
            }
            let mut steering_target = None;
            let durable_queue_item = store.queue_item_projection(task_id, queue_item_id)?;
            let outcome = match store.transact_command(command, |projection| {
                if is_submit
                    && !projection.current_run.as_ref().is_some_and(|run| {
                        matches!(
                            run.state,
                            RunState::Running | RunState::WaitingPermission | RunState::Yielding
                        )
                    })
                {
                    return Err(CommandRejection::InvalidCommand {
                        message: "messages enter the queue only while a run is active".into(),
                    });
                }
                if is_submit && projection.queue.len() >= MAX_ACTIVE_QUEUE_ITEMS {
                    return Err(CommandRejection::InvalidCommand {
                        message: format!(
                            "a task may contain at most {MAX_ACTIVE_QUEUE_ITEMS} active queue items"
                        ),
                    });
                }
                if promotion_mode.is_some()
                    && (!projection.current_run.as_ref().is_some_and(|run| {
                        matches!(run.state, RunState::Running | RunState::WaitingPermission)
                    }) || projection
                        .queue
                        .iter()
                        .any(|item| item.state == QueueItemState::Promoting))
                {
                    return Err(CommandRejection::InvalidCommand {
                        message: "promotion requires one running segment and no pending promotion"
                            .into(),
                    });
                }
                let current = projection
                    .queue
                    .iter()
                    .find(|item| item.queue_item_id == queue_item_id)
                    .or(durable_queue_item.as_ref());
                let mut events = Vec::new();
                if promotion_mode.is_some() {
                    if let Some(permission) = projection.pending_permission.as_ref() {
                        events.push(NewTaskEvent::permission_invalidated(
                            permission.request_id,
                            permission.revision,
                            "superseded by steering",
                        ));
                    }
                }
                events.extend(decide_queue(current, queue_command)?);
                if let Some(mode) = promotion_mode.as_ref() {
                    let segment_id = projection
                        .current_run
                        .as_ref()
                        .expect("promotion precondition checked above")
                        .segment_id;
                    steering_target = Some(segment_id);
                    events.push(NewTaskEvent::run_yield_requested(
                        segment_id,
                        matches!(mode, PromotionMode::ForceStop),
                        Utc::now(),
                    ));
                }
                Ok(events)
            }) {
                Ok(outcome) => outcome,
                Err(error) => command_store_error(error, command_id, command_task_id)?,
            };
            let accepted = matches!(outcome, CommandOutcome::Accepted { .. });
            reply_before_control_propagation(reply, outcome, || {
                if accepted {
                    if let (Some(mode), Some(target), Some(active)) =
                        (promotion_mode, steering_target, active.as_ref())
                    {
                        if active.segment_id != target {
                            return Ok(());
                        }
                        retry_control_propagation(|| {
                            factory.request_parent_stop(
                                task_id,
                                match mode {
                                    PromotionMode::SafePoint => SubagentStopMode::SafePoint,
                                    PromotionMode::ForceStop => SubagentStopMode::Force,
                                },
                            )
                        })?;
                        active.control.request(match mode {
                            PromotionMode::SafePoint => RunControl::YieldAfterAtomicOperation,
                            PromotionMode::ForceStop => RunControl::ForceStop,
                        });
                    }
                }
                Ok(())
            })
            .map_err(TaskActorError::SubagentStop)?;
        }
    }
    Ok(())
}

fn reply_before_control_propagation<T, E>(
    reply: oneshot::Sender<T>,
    outcome: T,
    propagate: impl FnOnce() -> Result<(), E>,
) -> Result<(), E> {
    let _ = reply.send(outcome);
    propagate()
}

fn retry_control_propagation<E>(mut propagate: impl FnMut() -> Result<(), E>) -> Result<(), E> {
    for attempt in 0..3 {
        match propagate() {
            Ok(()) => return Ok(()),
            Err(error) if attempt == 2 => return Err(error),
            Err(_) => {}
        }
    }
    unreachable!("the retry loop always returns on its final attempt")
}

#[allow(clippy::too_many_arguments)]
fn handle_start_segment(
    task_id: TaskId,
    store: &TaskStore,
    factory: &Arc<WorkspaceBoundRunCoordinatorFactory>,
    foreground_runs: &Arc<Semaphore>,
    active_segment_state: &Mutex<Option<RunSegmentId>>,
    mailbox: &mpsc::Sender<TaskActorMessage>,
    active: &mut Option<ActiveSegment>,
    mut command: AcceptedCommand,
    segment_id: RunSegmentId,
    started_at: DateTime<Utc>,
    indeterminate_tools: Option<Vec<IndeterminateToolDecision>>,
    reply: oneshot::Sender<CommandOutcome>,
) -> Result<(), TaskActorError> {
    if command.task_id != task_id {
        let outcome = CommandOutcome::Rejected {
            command_id: command.command_id,
            task_id: command.task_id,
            rejection: CommandRejection::InvalidCommand {
                message: "command task does not match actor task".into(),
            },
        };
        let _ = reply.send(outcome);
        return Ok(());
    }
    let durable_delivery = indeterminate_tools.is_some();
    let checkpoint = if durable_delivery {
        store.latest_checkpoint(task_id)?
    } else {
        None
    };
    let command_id = command.command_id;
    let command_task_id = command.task_id;
    command.authority = TaskStore::supervisor_command_authority(&command.authority);
    let mut acquired_permit = None;
    let outcome = match store.transact_command(command, |projection| {
        if projection.current_run.as_ref().is_some_and(|run| {
            matches!(
                run.state,
                RunState::Running | RunState::WaitingPermission | RunState::Yielding
            )
        }) {
            return Err(CommandRejection::InvalidCommand {
                message: "task already has a foreground run".into(),
            });
        }
        if indeterminate_tools.is_none()
            && projection.current_run.as_ref().is_some_and(|run| {
                run.terminal_reason
                    == Some(harness_contracts::RunTerminalReason::InterruptedByRestart)
            })
        {
            return Err(CommandRejection::InvalidCommand {
                message: "an interrupted restart must use continue_task".into(),
            });
        }
        if let Some(decisions) = indeterminate_tools.as_ref() {
            validate_continue_task(projection, checkpoint.as_ref(), segment_id, decisions)?;
        }
        let Ok(permit) = Arc::clone(foreground_runs).try_acquire_owned() else {
            return Err(CommandRejection::InvalidCommand {
                message: "global foreground-run quota is exhausted".into(),
            });
        };
        acquired_permit = Some(permit);
        Ok(vec![indeterminate_tools.as_ref().map_or_else(
            || NewTaskEvent::run_started(segment_id, started_at),
            |decisions| {
                NewTaskEvent::run_started_with_recovery(segment_id, started_at, decisions.clone())
            },
        )])
    }) {
        Ok(outcome) => outcome,
        Err(error) => command_store_error(error, command_id, command_task_id)?,
    };
    let accepted = matches!(outcome, CommandOutcome::Accepted { .. });
    let _ = reply.send(outcome);
    if !accepted {
        return Ok(());
    }
    let Some(permit) = acquired_permit else {
        return Ok(());
    };
    *active_segment_state
        .lock()
        .map_err(|_| TaskActorError::RuntimeStatePoisoned)? = Some(segment_id);
    let running = factory.spawn_idempotent(StartSegmentRequest {
        task_id,
        segment_id,
        indeterminate_tools: indeterminate_tools.unwrap_or_default(),
    });
    if durable_delivery {
        store.mark_segment_start_delivered(task_id, segment_id)?;
    }
    let control = running.control();
    *active = Some(ActiveSegment {
        segment_id,
        control,
        permit,
    });
    forward_run_events(segment_id, mailbox.clone(), running.into_events());
    Ok(())
}

fn validate_continue_task(
    projection: &harness_contracts::TaskProjection,
    checkpoint: Option<&TaskCheckpoint>,
    new_segment_id: RunSegmentId,
    decisions: &[IndeterminateToolDecision],
) -> Result<(), CommandRejection> {
    let Some(run) = projection.current_run.as_ref() else {
        return Err(CommandRejection::InvalidCommand {
            message: "continue_task requires a previous run".into(),
        });
    };
    if run.state != RunState::Interrupted || run.segment_id == new_segment_id {
        return Err(CommandRejection::InvalidCommand {
            message: "continue_task requires an interrupted run and a new segment id".into(),
        });
    }
    let Some(checkpoint) = checkpoint.filter(|value| value.run_segment_id == run.segment_id) else {
        return Err(CommandRejection::InvalidCommand {
            message: "continue_task requires the interrupted segment checkpoint".into(),
        });
    };
    let expected = checkpoint
        .incomplete_tool_use_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    if expected.len() != checkpoint.incomplete_tool_use_ids.len() {
        return Err(CommandRejection::InvalidCommand {
            message: "interrupted segment checkpoint contains duplicate tools".into(),
        });
    }
    let mut resolved = HashSet::new();
    for decision in decisions {
        let tool_use_id = ToolUseId::parse(&decision.tool_use_id).map_err(|_| {
            CommandRejection::InvalidCommand {
                message: "continue_task contains an invalid tool id".into(),
            }
        })?;
        if !resolved.insert(tool_use_id) {
            return Err(CommandRejection::InvalidCommand {
                message: "continue_task resolves a tool more than once".into(),
            });
        }
    }
    if resolved != expected {
        return Err(CommandRejection::InvalidCommand {
            message: "continue_task must resolve every indeterminate tool exactly once".into(),
        });
    }
    Ok(())
}

fn command_store_error(
    error: TaskStoreError,
    command_id: CommandId,
    task_id: TaskId,
) -> Result<CommandOutcome, TaskActorError> {
    match error {
        TaskStoreError::CommandConflict { .. }
        | TaskStoreError::InvalidInput(_)
        | TaskStoreError::BlobNotFound { .. }
        | TaskStoreError::BlobOwnershipDenied { .. }
        | TaskStoreError::BlobIntegrity(_) => Ok(CommandOutcome::Rejected {
            command_id,
            task_id,
            rejection: CommandRejection::InvalidCommand {
                message: "command conflicts with a durable command or is invalid".into(),
            },
        }),
        error => Err(TaskActorError::Store(error)),
    }
}

#[cfg(test)]
mod accepted_reply_tests {
    use std::cell::Cell;

    use tokio::sync::oneshot;

    use super::{reply_before_control_propagation, retry_control_propagation};

    #[tokio::test]
    async fn accepted_reply_is_delivered_when_control_propagation_fails() {
        let (reply, response) = oneshot::channel();

        let error =
            reply_before_control_propagation(reply, "accepted", || Err::<(), _>("control failed"));

        assert_eq!(response.await.unwrap(), "accepted");
        assert_eq!(error, Err("control failed"));
    }

    #[test]
    fn control_propagation_retries_before_returning_failure() {
        let attempts = Cell::new(0);
        let error = retry_control_propagation(|| {
            attempts.set(attempts.get() + 1);
            Err::<(), _>("durable stop failed")
        });

        assert_eq!(attempts.get(), 3);
        assert_eq!(error, Err("durable stop failed"));
    }
}

fn handle_run_event(
    task_id: TaskId,
    store: &TaskStore,
    factory: &Arc<WorkspaceBoundRunCoordinatorFactory>,
    active_segment_state: &Mutex<Option<RunSegmentId>>,
    mailbox: &mpsc::Sender<TaskActorMessage>,
    active: &mut Option<ActiveSegment>,
    event: RunCoordinatorEvent,
) -> Result<bool, TaskActorError> {
    match event {
        RunCoordinatorEvent::Completed {
            segment_id,
            terminal_reason,
            incomplete_output,
            ended_at,
        } => {
            if active.as_ref().map(|run| run.segment_id) != Some(segment_id) {
                return Ok(false);
            }
            let projection = store
                .task_projection(task_id)?
                .ok_or(TaskActorError::TaskNotFound)?;
            if projection
                .current_run
                .as_ref()
                .is_some_and(|run| run.state == RunState::Yielding)
            {
                if let Some(promoted) = projection
                    .queue
                    .iter()
                    .find(|item| item.state == QueueItemState::Promoting)
                {
                    let next_segment_id = RunSegmentId::new();
                    let command = AcceptedCommand {
                        command_id: CommandId::new(),
                        task_id,
                        idempotency_key: format!("steer-terminal-transition-{}", CommandId::new()),
                        expected_stream_version: projection.stream_version,
                        authority: TaskStore::supervisor_authority(),
                        payload: json!({
                            "type": "steer_terminal_transition",
                            "oldSegmentId": segment_id,
                            "newSegmentId": next_segment_id,
                            "queueItemId": promoted.queue_item_id,
                            "terminalReason": terminal_reason,
                        }),
                    };
                    let outcome = store.transact_command(command, |_| {
                        Ok(vec![
                            NewTaskEvent::run_completed(
                                segment_id,
                                ended_at,
                                terminal_reason,
                                incomplete_output,
                            ),
                            NewTaskEvent::run_started(next_segment_id, ended_at),
                            NewTaskEvent::message_consumed(
                                promoted.queue_item_id,
                                promoted.revision,
                                next_segment_id,
                            ),
                        ])
                    })?;
                    if matches!(outcome, CommandOutcome::Accepted { .. }) {
                        persist_checkpoint_boundary(store, task_id, next_segment_id, Vec::new())?;
                        let previous = active.take().expect("active segment checked above");
                        *active_segment_state
                            .lock()
                            .map_err(|_| TaskActorError::RuntimeStatePoisoned)? =
                            Some(next_segment_id);
                        let running = factory.spawn_idempotent(StartSegmentRequest {
                            task_id,
                            segment_id: next_segment_id,
                            indeterminate_tools: Vec::new(),
                        });
                        let control = running.control();
                        *active = Some(ActiveSegment {
                            segment_id: next_segment_id,
                            control,
                            permit: previous.permit,
                        });
                        forward_run_events(next_segment_id, mailbox.clone(), running.into_events());
                    }
                    return Ok(false);
                }
            }
            let outcome = commit_run_terminal(
                store,
                task_id,
                projection.stream_version,
                segment_id,
                terminal_reason,
                incomplete_output,
                ended_at,
            )?;
            if matches!(outcome, CommandOutcome::Accepted { .. }) {
                persist_checkpoint_boundary(store, task_id, segment_id, Vec::new())?;
                *active = None;
                *active_segment_state
                    .lock()
                    .map_err(|_| TaskActorError::RuntimeStatePoisoned)? = None;
                return Ok(true);
            }
        }
        RunCoordinatorEvent::SafePointReached {
            segment_id,
            forced,
            incomplete_output,
            non_revertible_tool_use_ids,
            reached_at,
        } => {
            if active.as_ref().map(|run| run.segment_id) != Some(segment_id) {
                return Ok(false);
            }
            let projection = store
                .task_projection(task_id)?
                .ok_or(TaskActorError::TaskNotFound)?;
            let expected_mode = if forced {
                PromotionMode::ForceStop
            } else {
                PromotionMode::SafePoint
            };
            if projection
                .current_run
                .as_ref()
                .and_then(|run| run.promotion_mode.as_ref())
                != Some(&expected_mode)
            {
                return Ok(false);
            }
            let Some(promoted) = projection
                .queue
                .iter()
                .find(|item| item.state == QueueItemState::Promoting)
            else {
                return Ok(false);
            };
            let next_segment_id = RunSegmentId::new();
            let command = AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: format!("steer-transition-{}", CommandId::new()),
                expected_stream_version: projection.stream_version,
                authority: TaskStore::supervisor_authority(),
                payload: json!({
                    "type": "steer_transition",
                    "oldSegmentId": segment_id,
                    "newSegmentId": next_segment_id,
                    "queueItemId": promoted.queue_item_id,
                    "forced": forced,
                    "incompleteOutput": incomplete_output,
                    "nonRevertibleToolUseIds": non_revertible_tool_use_ids,
                }),
            };
            let promoted_item_id = promoted.queue_item_id;
            let promoted_revision = promoted.revision;
            let terminal_reason = if forced {
                harness_contracts::RunTerminalReason::ForcedInterruption
            } else {
                harness_contracts::RunTerminalReason::Superseded
            };
            let outcome = store.transact_command(command, |_| {
                Ok(vec![
                    NewTaskEvent::run_safe_point_reached(
                        segment_id,
                        forced,
                        incomplete_output,
                        non_revertible_tool_use_ids,
                        reached_at,
                    ),
                    NewTaskEvent::run_completed(
                        segment_id,
                        reached_at,
                        terminal_reason,
                        incomplete_output,
                    ),
                    NewTaskEvent::run_started(next_segment_id, reached_at),
                    NewTaskEvent::message_consumed(
                        promoted_item_id,
                        promoted_revision,
                        next_segment_id,
                    ),
                ])
            })?;
            if !matches!(outcome, CommandOutcome::Accepted { .. }) {
                return Ok(false);
            }
            persist_checkpoint_boundary(store, task_id, next_segment_id, Vec::new())?;

            let previous = active.take().expect("active segment checked above");
            *active_segment_state
                .lock()
                .map_err(|_| TaskActorError::RuntimeStatePoisoned)? = Some(next_segment_id);
            let running = factory.spawn_idempotent(StartSegmentRequest {
                task_id,
                segment_id: next_segment_id,
                indeterminate_tools: Vec::new(),
            });
            let control = running.control();
            *active = Some(ActiveSegment {
                segment_id: next_segment_id,
                control,
                permit: previous.permit,
            });
            forward_run_events(next_segment_id, mailbox.clone(), running.into_events());
        }
        RunCoordinatorEvent::ForceStopTimedOut {
            segment_id,
            indeterminate_tool_use_ids,
            timed_out_at,
        } => {
            if active.as_ref().map(|run| run.segment_id) != Some(segment_id) {
                return Ok(false);
            }
            let projection = store
                .task_projection(task_id)?
                .ok_or(TaskActorError::TaskNotFound)?;
            if projection
                .current_run
                .as_ref()
                .and_then(|run| run.promotion_mode.as_ref())
                != Some(&PromotionMode::ForceStop)
            {
                return Ok(false);
            }
            let Some(promoted) = projection
                .queue
                .iter()
                .find(|item| item.state == QueueItemState::Promoting)
            else {
                return Ok(false);
            };
            let checkpoint_tool_use_ids = indeterminate_tool_use_ids.clone();
            let command = AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: format!("force-stop-timeout-{}", CommandId::new()),
                expected_stream_version: projection.stream_version,
                authority: TaskStore::recovery_authority(),
                payload: json!({
                    "type": "force_stop_timed_out",
                    "segmentId": segment_id,
                    "queueItemId": promoted.queue_item_id,
                    "indeterminateToolUseIds": indeterminate_tool_use_ids,
                }),
            };
            let outcome = store.transact_command(command, |_| {
                Ok(vec![
                    NewTaskEvent::run_force_stop_timed_out(
                        segment_id,
                        indeterminate_tool_use_ids,
                        timed_out_at,
                    ),
                    NewTaskEvent::run_completed(
                        segment_id,
                        timed_out_at,
                        harness_contracts::RunTerminalReason::Failed,
                        true,
                    ),
                    NewTaskEvent::message_recovered(promoted.queue_item_id, promoted.revision),
                ])
            })?;
            if matches!(outcome, CommandOutcome::Accepted { .. }) {
                persist_checkpoint_boundary(store, task_id, segment_id, checkpoint_tool_use_ids)?;
                *active = None;
                *active_segment_state
                    .lock()
                    .map_err(|_| TaskActorError::RuntimeStatePoisoned)? = None;
            }
        }
    }
    Ok(false)
}

fn schedule_consume_next(foreground_runs: Arc<Semaphore>, mailbox: mpsc::Sender<TaskActorMessage>) {
    tokio::spawn(async move {
        let Ok(mailbox_permit) = mailbox.reserve_owned().await else {
            return;
        };
        let Ok(run_permit) = foreground_runs.acquire_owned().await else {
            return;
        };
        mailbox_permit.send(TaskActorMessage::StartNextQueued(run_permit));
    });
}

fn handle_start_next_queued(
    task_id: TaskId,
    store: &TaskStore,
    factory: &Arc<WorkspaceBoundRunCoordinatorFactory>,
    active_segment_state: &Mutex<Option<RunSegmentId>>,
    mailbox: &mpsc::Sender<TaskActorMessage>,
    active: &mut Option<ActiveSegment>,
    permit: OwnedSemaphorePermit,
) -> Result<(), TaskActorError> {
    if active.is_some() {
        return Ok(());
    }
    let projection = store
        .task_projection(task_id)?
        .ok_or(TaskActorError::TaskNotFound)?;
    let Some(next) = projection
        .queue
        .iter()
        .filter(|item| item.state == harness_contracts::QueueItemState::Queued)
        .min_by_key(|item| (item.created_global_offset, item.queue_item_id.to_string()))
    else {
        return Ok(());
    };
    let segment_id = RunSegmentId::new();
    let started_at = Utc::now();
    let command = AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("consume-next-{}", CommandId::new()),
        expected_stream_version: projection.stream_version,
        authority: TaskStore::supervisor_authority(),
        payload: json!({
            "type": "consume_next",
            "queueItemId": next.queue_item_id,
            "expectedRevision": next.revision,
            "runSegmentId": segment_id,
            "startedAt": started_at,
        }),
    };
    let outcome = store.transact_command(command, |projection| {
        decide_consume_next(projection, segment_id, started_at)
    })?;
    if !matches!(outcome, CommandOutcome::Accepted { .. }) {
        return Ok(());
    }
    persist_checkpoint_boundary(store, task_id, segment_id, Vec::new())?;
    *active_segment_state
        .lock()
        .map_err(|_| TaskActorError::RuntimeStatePoisoned)? = Some(segment_id);
    let running = factory.spawn_idempotent(StartSegmentRequest {
        task_id,
        segment_id,
        indeterminate_tools: Vec::new(),
    });
    let control = running.control();
    *active = Some(ActiveSegment {
        segment_id,
        control,
        permit,
    });
    forward_run_events(segment_id, mailbox.clone(), running.into_events());
    Ok(())
}

fn persist_checkpoint_boundary(
    store: &TaskStore,
    task_id: TaskId,
    segment_id: RunSegmentId,
    incomplete_tool_use_ids: Vec<ToolUseId>,
) -> Result<(), TaskActorError> {
    let prior = store.latest_checkpoint(task_id)?;
    CheckpointService::persist_current(
        store,
        task_id,
        segment_id,
        CheckpointState {
            context_cursor: prior.as_ref().map_or(0, |value| value.context_cursor),
            workspace_baseline: prior
                .as_ref()
                .and_then(|value| value.workspace_baseline.clone()),
            incomplete_tool_use_ids,
            child_actor_refs: prior
                .as_ref()
                .map_or_else(Vec::new, |value| value.child_actor_refs.clone()),
            context_blob_id: prior.as_ref().and_then(|value| value.context_blob_id),
        },
    )?;
    Ok(())
}

fn commit_run_terminal(
    store: &TaskStore,
    task_id: TaskId,
    initial_stream_version: u64,
    segment_id: RunSegmentId,
    terminal_reason: harness_contracts::RunTerminalReason,
    incomplete_output: bool,
    ended_at: DateTime<Utc>,
) -> Result<CommandOutcome, TaskActorError> {
    let mut expected_stream_version = initial_stream_version;
    for attempt in 0..3 {
        let command = AcceptedCommand {
            command_id: CommandId::new(),
            task_id,
            idempotency_key: format!("run-terminal-{segment_id}-{expected_stream_version}"),
            expected_stream_version,
            authority: TaskStore::supervisor_authority(),
            payload: json!({
                "type": "run_completed",
                "segmentId": segment_id,
                "terminalReason": terminal_reason,
                "incompleteOutput": incomplete_output,
            }),
        };
        let outcome = store.transact_command(command, |_| {
            Ok(vec![NewTaskEvent::run_completed(
                segment_id,
                ended_at,
                terminal_reason.clone(),
                incomplete_output,
            )])
        })?;
        match outcome {
            CommandOutcome::Rejected {
                rejection: CommandRejection::WrongExpectedVersion { expected, actual },
                ..
            } => {
                if attempt == 2 {
                    return Err(TaskStoreError::WrongExpectedVersion { expected, actual }.into());
                }
                expected_stream_version = actual;
            }
            outcome => return Ok(outcome),
        }
    }
    unreachable!("terminal command retry loop always returns on its final attempt")
}

fn forward_run_events(
    segment_id: RunSegmentId,
    mailbox: mpsc::Sender<TaskActorMessage>,
    mut events: mpsc::UnboundedReceiver<RunCoordinatorEvent>,
) {
    tokio::spawn(async move {
        if let Some(event) = events.recv().await {
            let _ = mailbox.send(TaskActorMessage::RunEvent(event)).await;
        } else {
            let _ = mailbox
                .send(TaskActorMessage::RunEvent(RunCoordinatorEvent::Completed {
                    segment_id,
                    terminal_reason: harness_contracts::RunTerminalReason::Failed,
                    incomplete_output: true,
                    ended_at: Utc::now(),
                }))
                .await;
        }
    });
}

#[cfg(test)]
mod tests {
    use harness_contracts::{ClientId, RunTerminalReason, TaskState};
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn queued_start_waits_for_mailbox_capacity_before_taking_a_run_permit() {
        let foreground_runs = Arc::new(Semaphore::new(1));
        let (mailbox, mut messages) = mpsc::channel(1);
        mailbox.send(TaskActorMessage::Shutdown).await.unwrap();

        schedule_consume_next(Arc::clone(&foreground_runs), mailbox);
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }

        assert_eq!(foreground_runs.available_permits(), 1);
        assert!(matches!(
            messages.recv().await,
            Some(TaskActorMessage::Shutdown)
        ));
        let queued_start = tokio::time::timeout(std::time::Duration::from_secs(1), messages.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(queued_start, TaskActorMessage::StartNextQueued(_)));
        assert_eq!(foreground_runs.available_permits(), 0);
        drop(queued_start);
        assert_eq!(foreground_runs.available_permits(), 1);
    }

    #[test]
    fn terminal_commit_retries_after_the_stream_version_advances() {
        let root = tempfile::tempdir().unwrap();
        let store = TaskStore::open(root.path().join("tasks.sqlite")).unwrap();
        let task_id = TaskId::new();
        let segment_id = RunSegmentId::new();

        assert!(matches!(
            store
                .transact_command(
                    command(
                        task_id,
                        0,
                        TaskStore::user_authority(ClientId::new()),
                        json!({ "type": "create" }),
                    ),
                    |_| Ok(vec![NewTaskEvent::task_created("retry terminal")]),
                )
                .unwrap(),
            CommandOutcome::Accepted { .. }
        ));
        assert!(matches!(
            store
                .transact_command(
                    command(
                        task_id,
                        1,
                        TaskStore::supervisor_authority(),
                        json!({ "type": "start", "segmentId": segment_id }),
                    ),
                    |_| Ok(vec![NewTaskEvent::run_started(segment_id, Utc::now())]),
                )
                .unwrap(),
            CommandOutcome::Accepted { .. }
        ));
        let stale_version = store.stream_version(task_id).unwrap();
        assert!(matches!(
            store
                .transact_command(
                    command(
                        task_id,
                        stale_version,
                        TaskStore::user_authority(ClientId::new()),
                        json!({ "type": "rename" }),
                    ),
                    |_| Ok(vec![NewTaskEvent::task_title_changed("renamed")]),
                )
                .unwrap(),
            CommandOutcome::Accepted { .. }
        ));

        let outcome = commit_run_terminal(
            &store,
            task_id,
            stale_version,
            segment_id,
            RunTerminalReason::Completed,
            false,
            Utc::now(),
        )
        .unwrap();

        assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
        assert_eq!(
            store.task_projection(task_id).unwrap().unwrap().state,
            TaskState::Completed
        );
    }

    fn command(
        task_id: TaskId,
        expected_stream_version: u64,
        authority: harness_journal::EventAuthority,
        payload: serde_json::Value,
    ) -> AcceptedCommand {
        AcceptedCommand {
            command_id: CommandId::new(),
            task_id,
            idempotency_key: format!("test-{}", CommandId::new()),
            expected_stream_version,
            authority,
            payload,
        }
    }
}

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use harness_contracts::{CommandId, RunSegmentId, RunState, TaskId};
use harness_journal::{
    AcceptedCommand, CommandOutcome, CommandRejection, NewTaskEvent, TaskStore, TaskStoreError,
};
use serde_json::json;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, OwnedSemaphorePermit, Semaphore};

use crate::{RunCoordinatorEvent, RunCoordinatorFactory, StartSegmentRequest};

#[derive(Debug)]
pub enum TaskActorMessage {
    Command(Box<ValidatedTaskCommand>, oneshot::Sender<CommandOutcome>),
    RunEvent(RunCoordinatorEvent),
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum ValidatedTaskCommand {
    StartSegment {
        command: AcceptedCommand,
        segment_id: RunSegmentId,
        started_at: DateTime<Utc>,
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
}

impl ValidatedTaskCommand {
    pub(crate) fn rejected(&self, message: impl Into<String>) -> CommandOutcome {
        let Self::StartSegment { command, .. } = self;
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
    _permit: OwnedSemaphorePermit,
}

pub(crate) async fn run_task_actor(
    task_id: TaskId,
    store: Arc<TaskStore>,
    factory: Arc<dyn RunCoordinatorFactory>,
    foreground_runs: Arc<Semaphore>,
    active_segment_state: Arc<Mutex<Option<RunSegmentId>>>,
    mailbox: mpsc::UnboundedSender<TaskActorMessage>,
    mut messages: mpsc::UnboundedReceiver<TaskActorMessage>,
) -> Result<(), TaskActorError> {
    if store.task_projection(task_id)?.is_none() {
        return Err(TaskActorError::TaskNotFound);
    }
    let mut active = None::<ActiveSegment>;

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
                handle_run_event(task_id, &store, &active_segment_state, &mut active, event)?;
            }
            TaskActorMessage::Shutdown => break,
        }
    }
    Ok(())
}

async fn handle_command(
    task_id: TaskId,
    store: &TaskStore,
    factory: &Arc<dyn RunCoordinatorFactory>,
    foreground_runs: &Arc<Semaphore>,
    active_segment_state: &Mutex<Option<RunSegmentId>>,
    mailbox: &mpsc::UnboundedSender<TaskActorMessage>,
    active: &mut Option<ActiveSegment>,
    command: ValidatedTaskCommand,
    reply: oneshot::Sender<CommandOutcome>,
) -> Result<(), TaskActorError> {
    match command {
        ValidatedTaskCommand::StartSegment {
            mut command,
            segment_id,
            started_at,
        } => {
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
                let Ok(permit) = Arc::clone(foreground_runs).try_acquire_owned() else {
                    return Err(CommandRejection::InvalidCommand {
                        message: "global foreground-run quota is exhausted".into(),
                    });
                };
                acquired_permit = Some(permit);
                Ok(vec![NewTaskEvent::run_started(segment_id, started_at)])
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
            let running = factory.spawn(StartSegmentRequest {
                task_id,
                segment_id,
            });
            *active = Some(ActiveSegment {
                segment_id,
                _permit: permit,
            });
            forward_run_events(segment_id, mailbox.clone(), running.into_events());
        }
    }
    Ok(())
}

fn command_store_error(
    error: TaskStoreError,
    command_id: CommandId,
    task_id: TaskId,
) -> Result<CommandOutcome, TaskActorError> {
    match error {
        TaskStoreError::CommandConflict { .. } | TaskStoreError::InvalidInput(_) => {
            Ok(CommandOutcome::Rejected {
                command_id,
                task_id,
                rejection: CommandRejection::InvalidCommand {
                    message: "command conflicts with a durable command or is invalid".into(),
                },
            })
        }
        error => Err(TaskActorError::Store(error)),
    }
}

fn handle_run_event(
    task_id: TaskId,
    store: &TaskStore,
    active_segment_state: &Mutex<Option<RunSegmentId>>,
    active: &mut Option<ActiveSegment>,
    event: RunCoordinatorEvent,
) -> Result<(), TaskActorError> {
    match event {
        RunCoordinatorEvent::Completed {
            segment_id,
            terminal_reason,
            incomplete_output,
            ended_at,
        } => {
            if active.as_ref().map(|run| run.segment_id) != Some(segment_id) {
                return Ok(());
            }
            let projection = store
                .task_projection(task_id)?
                .ok_or(TaskActorError::TaskNotFound)?;
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
                *active = None;
                *active_segment_state
                    .lock()
                    .map_err(|_| TaskActorError::RuntimeStatePoisoned)? = None;
            }
        }
    }
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
    mailbox: mpsc::UnboundedSender<TaskActorMessage>,
    mut events: mpsc::UnboundedReceiver<RunCoordinatorEvent>,
) {
    tokio::spawn(async move {
        if let Some(event) = events.recv().await {
            let _ = mailbox.send(TaskActorMessage::RunEvent(event));
        } else {
            let _ = mailbox.send(TaskActorMessage::RunEvent(RunCoordinatorEvent::Completed {
                segment_id,
                terminal_reason: harness_contracts::RunTerminalReason::Failed,
                incomplete_output: true,
                ended_at: Utc::now(),
            }));
        }
    });
}

#[cfg(test)]
mod tests {
    use harness_contracts::{ClientId, RunTerminalReason, TaskState};
    use serde_json::json;

    use super::*;

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

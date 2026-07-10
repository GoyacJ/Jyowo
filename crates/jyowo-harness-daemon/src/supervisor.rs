use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use futures::FutureExt;
use harness_contracts::{CommandId, RunState, TaskId};
use harness_journal::{
    AcceptedCommand, CommandOutcome, CommandRejection, NewTaskEvent, TaskStore, TaskStoreError,
};
use serde_json::json;
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, oneshot, OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};

use crate::task_actor::{run_task_actor, TaskActorError};
use crate::{RunCoordinatorFactory, TaskActorMessage, ValidatedTaskCommand};

const SUPERVISOR_REQUEST_CAPACITY: usize = 64;
const TASK_ACTOR_MAILBOX_CAPACITY: usize = 64;

fn bounded_command_channel<T>(capacity: usize) -> (mpsc::Sender<T>, mpsc::Receiver<T>) {
    mpsc::channel(capacity)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupervisorQuotas {
    pub foreground_runs: usize,
    pub subagents: usize,
}

impl SupervisorQuotas {
    #[must_use]
    pub const fn new(foreground_runs: usize, subagents: usize) -> Self {
        Self {
            foreground_runs,
            subagents,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorEvent {
    ActorFailed { task_id: TaskId },
}

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("supervisor stopped")]
    Stopped,
    #[error("task actor stopped")]
    ActorStopped,
    #[error("supervisor quotas must be greater than zero")]
    InvalidQuota,
    #[error("subagent quota was closed")]
    SubagentQuotaClosed,
}

pub struct Supervisor {
    requests: mpsc::Sender<SupervisorRequest>,
    events: broadcast::Sender<SupervisorEvent>,
    subagent_quota: Arc<Semaphore>,
    task: JoinHandle<()>,
}

impl Supervisor {
    pub fn start(
        store: Arc<TaskStore>,
        factory: Arc<dyn RunCoordinatorFactory>,
        quotas: SupervisorQuotas,
    ) -> Result<Self, SupervisorError> {
        if quotas.foreground_runs == 0 || quotas.subagents == 0 {
            return Err(SupervisorError::InvalidQuota);
        }
        let (requests, receiver) = bounded_command_channel(SUPERVISOR_REQUEST_CAPACITY);
        let (events, _) = broadcast::channel(64);
        let subagent_quota = Arc::new(Semaphore::new(quotas.subagents));
        let task = tokio::spawn(run_supervisor(
            store,
            factory,
            Arc::new(Semaphore::new(quotas.foreground_runs)),
            receiver,
            events.clone(),
        ));
        Ok(Self {
            requests,
            events,
            subagent_quota,
            task,
        })
    }

    pub async fn dispatch(
        &self,
        task_id: TaskId,
        command: ValidatedTaskCommand,
    ) -> Result<CommandOutcome, SupervisorError> {
        let (route_reply, route_response) = oneshot::channel();
        self.requests
            .send(SupervisorRequest::Route {
                task_id,
                reply: route_reply,
            })
            .await
            .map_err(|_| SupervisorError::Stopped)?;
        let route = route_response
            .await
            .map_err(|_| SupervisorError::ActorStopped)?;
        let ActorRoute::Mailbox(mailbox) = route else {
            return Ok(command.rejected("task does not exist"));
        };
        let (reply, response) = oneshot::channel();
        mailbox
            .send(TaskActorMessage::Command(Box::new(command), reply))
            .await
            .map_err(|_| SupervisorError::ActorStopped)?;
        response.await.map_err(|_| SupervisorError::ActorStopped)
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<SupervisorEvent> {
        self.events.subscribe()
    }

    pub async fn acquire_subagent_permit(&self) -> Result<OwnedSemaphorePermit, SupervisorError> {
        Arc::clone(&self.subagent_quota)
            .acquire_owned()
            .await
            .map_err(|_| SupervisorError::SubagentQuotaClosed)
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.task.abort();
    }
}

enum SupervisorRequest {
    Route {
        task_id: TaskId,
        reply: oneshot::Sender<ActorRoute>,
    },
}

enum ActorRoute {
    Mailbox(mpsc::Sender<TaskActorMessage>),
    TaskNotFound,
}

struct ActorExit {
    task_id: TaskId,
    generation: u64,
    failed: bool,
    active_segment: Option<harness_contracts::RunSegmentId>,
}

struct ActorSlot {
    generation: u64,
    mailbox: mpsc::Sender<TaskActorMessage>,
}

async fn run_supervisor(
    store: Arc<TaskStore>,
    factory: Arc<dyn RunCoordinatorFactory>,
    foreground_runs: Arc<Semaphore>,
    mut requests: mpsc::Receiver<SupervisorRequest>,
    events: broadcast::Sender<SupervisorEvent>,
) {
    let mut actors = HashMap::<TaskId, ActorSlot>::new();
    let mut actor_tasks = JoinSet::<ActorExit>::new();
    let mut next_generation = 1_u64;

    loop {
        tokio::select! {
            request = requests.recv() => {
                let Some(request) = request else { break };
                match request {
                    SupervisorRequest::Route { task_id, reply } => {
                        match store.task_projection(task_id) {
                            Ok(None) => {
                                let _ = reply.send(ActorRoute::TaskNotFound);
                                continue;
                            }
                            Err(_) => {
                                drop(reply);
                                continue;
                            }
                            Ok(Some(_)) => {}
                        }
                        let slot = actors.entry(task_id).or_insert_with(|| {
                            let generation = next_generation;
                            next_generation = next_generation.saturating_add(1);
                            spawn_actor(
                                &mut actor_tasks,
                                task_id,
                                generation,
                                Arc::clone(&store),
                                Arc::clone(&factory),
                                Arc::clone(&foreground_runs),
                            )
                        });
                        let _ = reply.send(ActorRoute::Mailbox(slot.mailbox.clone()));
                    }
                }
            }
            exit = actor_tasks.join_next(), if !actor_tasks.is_empty() => {
                let Some(Ok(exit)) = exit else { continue };
                let exited_current_generation =
                    remove_actor_generation(&mut actors, exit.task_id, exit.generation);
                if exit.failed {
                    let failure_committed = persist_actor_failure(
                        &store,
                        exit.task_id,
                        exit.active_segment,
                    )
                    .unwrap_or(false);
                    if failure_committed {
                        let _ = events.send(SupervisorEvent::ActorFailed { task_id: exit.task_id });
                    }
                    if failure_committed && exited_current_generation {
                        let generation = next_generation;
                        next_generation = next_generation.saturating_add(1);
                        actors.insert(
                            exit.task_id,
                            spawn_actor(
                                &mut actor_tasks,
                                exit.task_id,
                                generation,
                                Arc::clone(&store),
                                Arc::clone(&factory),
                                Arc::clone(&foreground_runs),
                            ),
                        );
                    }
                }
            }
        }
    }

    for (_, slot) in actors {
        let _ = slot.mailbox.send(TaskActorMessage::Shutdown).await;
    }
    while actor_tasks.join_next().await.is_some() {}
}

fn spawn_actor(
    actor_tasks: &mut JoinSet<ActorExit>,
    task_id: TaskId,
    generation: u64,
    store: Arc<TaskStore>,
    factory: Arc<dyn RunCoordinatorFactory>,
    foreground_runs: Arc<Semaphore>,
) -> ActorSlot {
    let (mailbox, messages) = bounded_command_channel(TASK_ACTOR_MAILBOX_CAPACITY);
    let actor_mailbox = mailbox.clone();
    let active_segment_state = Arc::new(Mutex::new(None));
    let exit_segment_state = Arc::clone(&active_segment_state);
    actor_tasks.spawn(async move {
        let result = AssertUnwindSafe(run_task_actor(
            task_id,
            store,
            factory,
            foreground_runs,
            active_segment_state,
            actor_mailbox,
            messages,
        ))
        .catch_unwind()
        .await;
        let failed = match result {
            Ok(Ok(())) | Ok(Err(TaskActorError::TaskNotFound)) => false,
            Ok(Err(TaskActorError::Store(_) | TaskActorError::RuntimeStatePoisoned)) | Err(_) => {
                true
            }
        };
        let active_segment = exit_segment_state.lock().ok().and_then(|segment| *segment);
        ActorExit {
            task_id,
            generation,
            failed,
            active_segment,
        }
    });
    ActorSlot {
        generation,
        mailbox,
    }
}

fn remove_actor_generation(
    actors: &mut HashMap<TaskId, ActorSlot>,
    task_id: TaskId,
    generation: u64,
) -> bool {
    if actors
        .get(&task_id)
        .is_some_and(|slot| slot.generation == generation)
    {
        actors.remove(&task_id);
        true
    } else {
        false
    }
}

fn persist_actor_failure(
    store: &TaskStore,
    task_id: TaskId,
    active_segment: Option<harness_contracts::RunSegmentId>,
) -> Result<bool, TaskStoreError> {
    for _ in 0..3 {
        let projection = match store.task_projection(task_id)? {
            Some(projection) => projection,
            None => return Ok(false),
        };
        let projected_active_segment = projection.current_run.as_ref().and_then(|run| {
            matches!(
                run.state,
                RunState::Running | RunState::WaitingPermission | RunState::Yielding
            )
            .then_some(run.segment_id)
        });
        if projected_active_segment != active_segment {
            return Ok(false);
        }
        let command = AcceptedCommand {
            command_id: CommandId::new(),
            task_id,
            idempotency_key: format!("actor-failed-{}", CommandId::new()),
            expected_stream_version: projection.stream_version,
            authority: TaskStore::supervisor_authority(),
            payload: json!({ "type": "actor_failed", "segmentId": active_segment }),
        };
        let outcome = store.transact_command(command, |_| {
            Ok(vec![NewTaskEvent::task_actor_failed(
                active_segment,
                Utc::now(),
            )])
        })?;
        match require_committed_failure(outcome) {
            Ok(()) => return Ok(true),
            Err(TaskStoreError::WrongExpectedVersion { .. }) => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

fn require_committed_failure(outcome: CommandOutcome) -> Result<(), TaskStoreError> {
    match outcome {
        CommandOutcome::Accepted { .. } => Ok(()),
        CommandOutcome::Rejected {
            rejection: CommandRejection::WrongExpectedVersion { expected, actual },
            ..
        } => Err(TaskStoreError::WrongExpectedVersion { expected, actual }),
        CommandOutcome::Rejected { rejection, .. } => Err(TaskStoreError::InvalidInput(format!(
            "task.actor_failed command was rejected: {rejection:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn bounded_command_channel_waits_for_capacity_instead_of_dropping() {
        let (sender, mut receiver) = bounded_command_channel(1);
        sender.send(1_u8).await.unwrap();

        assert!(
            tokio::time::timeout(Duration::from_millis(25), sender.send(2_u8))
                .await
                .is_err()
        );
        assert_eq!(receiver.recv().await, Some(1));
        sender.send(2).await.unwrap();
        assert_eq!(receiver.recv().await, Some(2));
    }

    #[test]
    fn stale_actor_generation_cannot_remove_a_replacement() {
        let task_id = TaskId::new();
        let (mailbox, _) = bounded_command_channel(TASK_ACTOR_MAILBOX_CAPACITY);
        let mut actors = HashMap::from([(
            task_id,
            ActorSlot {
                generation: 2,
                mailbox,
            },
        )]);

        assert!(!remove_actor_generation(&mut actors, task_id, 1));
        assert_eq!(actors.get(&task_id).unwrap().generation, 2);
        assert!(remove_actor_generation(&mut actors, task_id, 2));
        assert!(!actors.contains_key(&task_id));
    }

    #[test]
    fn rejected_failure_command_is_not_a_committed_failure() {
        let task_id = TaskId::new();
        let command_id = CommandId::new();
        let outcome = CommandOutcome::Rejected {
            command_id,
            task_id,
            rejection: CommandRejection::WrongExpectedVersion {
                expected: 1,
                actual: 2,
            },
        };

        assert!(require_committed_failure(outcome).is_err());
    }
}

use std::collections::{HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use futures::FutureExt;
use harness_contracts::{
    ActorId, CommandId, NoopRedactor, Redactor, RunState, TaskId, WorkspaceLeaseState,
};
use harness_journal::{
    AcceptedCommand, CommandOutcome, CommandRejection, NewTaskEvent, PendingSegmentStart,
    TaskStore, TaskStoreError,
};
use serde_json::json;
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, oneshot, OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};

use crate::task_actor::{run_task_actor, TaskActorError};
use crate::{
    PermissionBroker, PermissionBrokerError, PermissionDecisionInput, RecoveryService,
    RunCoordinatorFactory, SubagentStopMode, SubagentSupervisor, TaskActorMessage,
    ValidatedTaskCommand, WorkspaceAccess, WorkspaceAcquireOutcome,
    WorkspaceBoundRunCoordinatorFactory, WorkspaceCoordinator, WorkspaceCoordinatorError,
    WorkspaceExecutionKind, WorkspaceLeaseRequest, WorkspaceSubagentRunnerFactory,
    WorkspaceToolDispatcher,
};

const SUPERVISOR_REQUEST_CAPACITY: usize = 64;
const TASK_ACTOR_MAILBOX_CAPACITY: usize = 64;
const PENDING_START_RETRY_BASE_DELAY: Duration = Duration::from_millis(25);
const PENDING_START_RETRY_MAX_DELAY: Duration = Duration::from_secs(1);
const WORKSPACE_EXPIRY_INTERVAL: Duration = Duration::from_secs(1);

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
    #[error("startup recovery failed: {0}")]
    StartupRecovery(#[from] TaskStoreError),
    #[error("workspace coordination failed: {0}")]
    Workspace(#[from] WorkspaceCoordinatorError),
    #[error("permission routing failed: {0}")]
    Permission(#[from] PermissionBrokerError),
}

pub struct Supervisor {
    requests: mpsc::Sender<SupervisorRequest>,
    events: broadcast::Sender<SupervisorEvent>,
    store: Arc<TaskStore>,
    workspace: Arc<WorkspaceCoordinator>,
    subagents: Arc<SubagentSupervisor>,
    permissions: Arc<PermissionBroker>,
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
        let disabled_factory: Arc<dyn WorkspaceSubagentRunnerFactory> =
            Arc::new(|_context: crate::WorkspaceSubagentRunContext| {
                Err(harness_subagent::SubagentError::Engine(
                    "no daemon subagent runner factory is configured".into(),
                ))
            });
        Self::start_with_subagents(
            store,
            factory,
            quotas,
            disabled_factory,
            Arc::new(NoopRedactor),
            8,
        )
    }

    pub fn start_with_subagents(
        store: Arc<TaskStore>,
        factory: Arc<dyn RunCoordinatorFactory>,
        quotas: SupervisorQuotas,
        runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory>,
        redactor: Arc<dyn Redactor>,
        max_depth: u8,
    ) -> Result<Self, SupervisorError> {
        if quotas.foreground_runs == 0 || quotas.subagents == 0 {
            return Err(SupervisorError::InvalidQuota);
        }
        let permissions = Arc::new(PermissionBroker::new(
            Arc::clone(&store),
            Arc::clone(&redactor),
        ));
        Self::start_with_runtime_components(
            store,
            factory,
            quotas,
            runner_factory,
            redactor,
            max_depth,
            permissions,
        )
    }

    pub fn start_with_runtime_components(
        store: Arc<TaskStore>,
        factory: Arc<dyn RunCoordinatorFactory>,
        quotas: SupervisorQuotas,
        runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory>,
        redactor: Arc<dyn Redactor>,
        max_depth: u8,
        permissions: Arc<PermissionBroker>,
    ) -> Result<Self, SupervisorError> {
        if quotas.foreground_runs == 0 || quotas.subagents == 0 {
            return Err(SupervisorError::InvalidQuota);
        }
        let managed_root = store
            .database_path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("managed-worktrees");
        let workspace = Arc::new(WorkspaceCoordinator::new(Arc::clone(&store), managed_root)?);
        let subagents = Arc::new(SubagentSupervisor::new(
            Arc::clone(&store),
            Arc::clone(&workspace),
            runner_factory,
            redactor,
            max_depth,
            quotas.subagents,
        ));
        Self::start_inner(store, factory, quotas, workspace, subagents, permissions)
    }

    fn start_inner(
        store: Arc<TaskStore>,
        factory: Arc<dyn RunCoordinatorFactory>,
        quotas: SupervisorQuotas,
        workspace: Arc<WorkspaceCoordinator>,
        subagents: Arc<SubagentSupervisor>,
        permissions: Arc<PermissionBroker>,
    ) -> Result<Self, SupervisorError> {
        recover_unreconnectable_subagents(&store, &workspace)?;
        let (requests, receiver) = bounded_command_channel(SUPERVISOR_REQUEST_CAPACITY);
        let (events, _) = broadcast::channel(64);
        let factory = Arc::new(WorkspaceBoundRunCoordinatorFactory::new(
            factory,
            WorkspaceToolDispatcher::new(Arc::clone(&workspace)),
            Arc::clone(&store),
            Arc::clone(&subagents),
        ));
        let task = tokio::spawn(run_supervisor(
            Arc::clone(&store),
            factory,
            Arc::clone(&workspace),
            Arc::clone(&permissions),
            Arc::new(Semaphore::new(quotas.foreground_runs)),
            receiver,
            events.clone(),
        ));
        Ok(Self {
            requests,
            events,
            store,
            workspace,
            subagents,
            permissions,
            task,
        })
    }

    pub async fn dispatch(
        &self,
        task_id: TaskId,
        mut command: ValidatedTaskCommand,
    ) -> Result<CommandOutcome, SupervisorError> {
        if let Err(message) =
            ensure_foreground_workspace(&self.store, &self.workspace, task_id, &mut command)
        {
            return Ok(command.rejected(message));
        }
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

    pub fn resolve_permission(
        &self,
        command: AcceptedCommand,
        input: PermissionDecisionInput,
    ) -> Result<CommandOutcome, SupervisorError> {
        Ok(self.permissions.resolve_client_command(command, input)?)
    }

    #[must_use]
    pub fn permission_broker(&self) -> Arc<PermissionBroker> {
        Arc::clone(&self.permissions)
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<SupervisorEvent> {
        self.events.subscribe()
    }

    pub async fn acquire_subagent_permit(&self) -> Result<OwnedSemaphorePermit, SupervisorError> {
        self.subagents
            .reserve_permit()
            .await
            .map_err(|_| SupervisorError::SubagentQuotaClosed)
    }

    #[cfg(test)]
    async fn resident_actor_count(&self) -> usize {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(SupervisorRequest::ResidentActorCount { reply })
            .await
            .expect("supervisor is running");
        response.await.expect("supervisor returns actor count")
    }
}

fn ensure_foreground_workspace(
    store: &TaskStore,
    workspace: &WorkspaceCoordinator,
    task_id: TaskId,
    command: &mut ValidatedTaskCommand,
) -> Result<(), String> {
    let Some(projection) = store
        .task_projection(task_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    if projection.removed {
        return Ok(());
    }
    let segment_command = matches!(
        command,
        ValidatedTaskCommand::SubmitMessage { .. }
            | ValidatedTaskCommand::StartSegment { .. }
            | ValidatedTaskCommand::ContinueTask { .. }
    );
    if !segment_command {
        return Ok(());
    }
    let active_run = projection.current_run.as_ref().is_some_and(|run| {
        matches!(
            run.state,
            RunState::Running | RunState::WaitingPermission | RunState::Yielding
        )
    });

    let leases = store
        .nonterminal_workspace_leases_for_task(task_id)
        .map_err(|error| error.to_string())?;
    if leases
        .iter()
        .any(|lease| lease.state == WorkspaceLeaseState::Active)
    {
        let expected = command.accepted_command().expected_stream_version;
        if expected < projection.stream_version {
            let next_event = store
                .task_events_after(task_id, expected, 1)
                .map_err(|error| error.to_string())?
                .into_iter()
                .next();
            if next_event
                .as_ref()
                .is_some_and(|event| event.event_type == "workspace.acquired")
            {
                command.accepted_command_mut().expected_stream_version = expected + 1;
            }
        }
        return Ok(());
    }
    if active_run {
        return Ok(());
    }
    if command.accepted_command().expected_stream_version != projection.stream_version {
        return Ok(());
    }
    if leases
        .iter()
        .any(|lease| lease.state == WorkspaceLeaseState::Waiting)
    {
        return Err("workspace is busy".into());
    }

    let selection = projection
        .workspace
        .ok_or_else(|| "task workspace selection is missing".to_owned())?;
    let actor_id = projection
        .actor_id
        .unwrap_or_else(|| ActorId::from_u128(u128::from_be_bytes(task_id.as_bytes())));
    match workspace
        .acquire(WorkspaceLeaseRequest {
            task_id,
            actor_id,
            root: selection.root.into(),
            mode: Some(selection.mode),
            access: WorkspaceAccess::Write,
            execution_kind: WorkspaceExecutionKind::Foreground,
            expires_at: None,
        })
        .map_err(|error| error.to_string())?
    {
        WorkspaceAcquireOutcome::Acquired(_) => {
            command.accepted_command_mut().expected_stream_version = store
                .task_projection(task_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "task does not exist".to_owned())?
                .stream_version;
            Ok(())
        }
        WorkspaceAcquireOutcome::Waiting(_) => Err("workspace is busy".into()),
    }
}

fn recover_unreconnectable_subagents(
    store: &TaskStore,
    workspace: &WorkspaceCoordinator,
) -> Result<(), SupervisorError> {
    for child in store.nonterminal_subagent_actors()? {
        let recovered =
            store.recover_subagent_actor(child.child_task_id, child.actor_id, Utc::now())?;
        if let Some(lease_id) = recovered.workspace_lease_id {
            match workspace.release(lease_id) {
                Ok(_) | Err(WorkspaceCoordinatorError::CleanupBlocked { .. }) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
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
    #[cfg(test)]
    ResidentActorCount { reply: oneshot::Sender<usize> },
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
    factory: Arc<WorkspaceBoundRunCoordinatorFactory>,
    workspace: Arc<WorkspaceCoordinator>,
    permissions: Arc<PermissionBroker>,
    foreground_runs: Arc<Semaphore>,
    mut requests: mpsc::Receiver<SupervisorRequest>,
    events: broadcast::Sender<SupervisorEvent>,
) {
    let mut actors = HashMap::<TaskId, ActorSlot>::new();
    let mut actor_tasks = JoinSet::<ActorExit>::new();
    let mut pending_start_retries =
        HashMap::<TaskId, (harness_contracts::RunSegmentId, u32)>::new();
    let mut pending_parent_stop_compensations = HashSet::<TaskId>::new();
    let mut next_generation = 1_u64;
    let mut workspace_expiry = tokio::time::interval(WORKSPACE_EXPIRY_INTERVAL);
    workspace_expiry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = workspace_expiry.tick() => {
                let _ = workspace.expire_stale(Utc::now());
                retry_parent_stop_compensations(
                    &mut pending_parent_stop_compensations,
                    |task_id| factory.request_parent_stop(task_id, SubagentStopMode::Force),
                );
            }
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
                        if !actors.contains_key(&task_id)
                            && RecoveryService::new(Arc::clone(&store))
                                .recover_task(task_id)
                                .is_err()
                        {
                            drop(reply);
                            continue;
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
                                Arc::clone(&permissions),
                                Arc::clone(&foreground_runs),
                                None,
                                Duration::ZERO,
                            )
                        });
                        let _ = reply.send(ActorRoute::Mailbox(slot.mailbox.clone()));
                    }
                    #[cfg(test)]
                    SupervisorRequest::ResidentActorCount { reply } => {
                        let _ = reply.send(actors.len());
                    }
                }
            }
            exit = actor_tasks.join_next(), if !actor_tasks.is_empty() => {
                let Some(Ok(exit)) = exit else { continue };
                let exited_current_generation =
                    remove_actor_generation(&mut actors, exit.task_id, exit.generation);
                if exit.failed {
                    record_parent_stop_compensation(
                        &mut pending_parent_stop_compensations,
                        exit.task_id,
                        |task_id| factory.request_parent_stop(task_id, SubagentStopMode::Force),
                    );
                    if exited_current_generation {
                        let _ = workspace.release_task_leases(exit.task_id);
                    }
                    let pending_start_retry_segment = exit.active_segment.filter(|segment_id| {
                        pending_segment_start_requires_retry(
                            store.pending_segment_start(exit.task_id, *segment_id),
                        )
                    });
                    if let Some(segment_id) = pending_start_retry_segment {
                        if exited_current_generation {
                            let retry = pending_start_retries.entry(exit.task_id).or_insert((segment_id, 0));
                            if retry.0 != segment_id {
                                *retry = (segment_id, 0);
                            }
                            retry.1 = retry.1.saturating_add(1);
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
                                    Arc::clone(&permissions),
                                    Arc::clone(&foreground_runs),
                                    Some(segment_id),
                                    pending_segment_start_retry_delay(retry.1),
                                ),
                            );
                        }
                    } else {
                        pending_start_retries.remove(&exit.task_id);
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
                                    Arc::clone(&permissions),
                                    Arc::clone(&foreground_runs),
                                    None,
                                    Duration::ZERO,
                                ),
                            );
                        }
                    }
                } else {
                    pending_start_retries.remove(&exit.task_id);
                }
            }
        }
    }

    for (_, slot) in actors {
        let _ = slot.mailbox.send(TaskActorMessage::Shutdown).await;
    }
    while actor_tasks.join_next().await.is_some() {}
}

fn record_parent_stop_compensation<E>(
    pending: &mut HashSet<TaskId>,
    task_id: TaskId,
    mut request_stop: impl FnMut(TaskId) -> Result<(), E>,
) {
    if request_stop(task_id).is_err() {
        pending.insert(task_id);
    } else {
        pending.remove(&task_id);
    }
}

fn retry_parent_stop_compensations<E>(
    pending: &mut HashSet<TaskId>,
    mut request_stop: impl FnMut(TaskId) -> Result<(), E>,
) {
    for task_id in pending.iter().copied().collect::<Vec<_>>() {
        record_parent_stop_compensation(pending, task_id, &mut request_stop);
    }
}

fn spawn_actor(
    actor_tasks: &mut JoinSet<ActorExit>,
    task_id: TaskId,
    generation: u64,
    store: Arc<TaskStore>,
    factory: Arc<WorkspaceBoundRunCoordinatorFactory>,
    permissions: Arc<PermissionBroker>,
    foreground_runs: Arc<Semaphore>,
    pending_start_retry_segment: Option<harness_contracts::RunSegmentId>,
    startup_delay: Duration,
) -> ActorSlot {
    let (mailbox, messages) = bounded_command_channel(TASK_ACTOR_MAILBOX_CAPACITY);
    let actor_mailbox = mailbox.clone();
    let active_segment_state = Arc::new(Mutex::new(pending_start_retry_segment));
    let exit_segment_state = Arc::clone(&active_segment_state);
    actor_tasks.spawn(async move {
        tokio::time::sleep(startup_delay).await;
        let result = AssertUnwindSafe(run_task_actor(
            task_id,
            store,
            factory,
            permissions,
            foreground_runs,
            active_segment_state,
            pending_start_retry_segment,
            actor_mailbox,
            messages,
        ))
        .catch_unwind()
        .await;
        let failed = match result {
            Ok(Ok(())) | Ok(Err(TaskActorError::TaskNotFound)) => false,
            Ok(Err(
                TaskActorError::Store(_)
                | TaskActorError::RuntimeStatePoisoned
                | TaskActorError::SegmentStartDeliveryNotPending
                | TaskActorError::SubagentStop(_)
                | TaskActorError::Permission(_)
                | TaskActorError::Workspace(_),
            ))
            | Err(_) => true,
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

fn pending_segment_start_requires_retry(
    pending: Result<Option<PendingSegmentStart>, TaskStoreError>,
) -> bool {
    !matches!(pending, Ok(None))
}

fn pending_segment_start_retry_delay(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(6);
    (PENDING_START_RETRY_BASE_DELAY * (1_u32 << exponent)).min(PENDING_START_RETRY_MAX_DELAY)
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
    use harness_contracts::{
        ActorId, EventSourceKind, QueueItemId, QueueItemState, RunSegmentId, RunTerminalReason,
        SubagentActorState, SubagentId, WorkspaceLeaseId, WorkspaceMode,
    };
    use harness_journal::{
        AcquireTaskWorkspaceLease, CreateSubagentActorRequest, SubagentLifecycleAuthority,
        SubagentLifecycleCommand, SubagentLifecycleTransition, TaskWorkspaceLeaseState,
    };
    use std::time::Duration;

    struct IdleFactory;

    impl RunCoordinatorFactory for IdleFactory {
        fn spawn_idempotent(
            &self,
            _request: crate::StartSegmentRequest,
            _workspace_tools: crate::WorkspaceToolDispatcher,
            _subagent_runner: Arc<dyn harness_subagent::SubagentRunner>,
            _agent_starters: crate::AgentStarterCapabilities,
        ) -> crate::RunningSegment {
            let (_sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            crate::RunningSegment::new(receiver)
        }
    }

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

    #[tokio::test]
    async fn supervisor_keeps_recovered_tasks_lazy_until_they_are_routed() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        for index in 0..33 {
            let task_id = TaskId::new();
            let segment_id = RunSegmentId::new();
            let command = AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: format!("interrupted-{index}"),
                expected_stream_version: 0,
                authority: TaskStore::supervisor_authority(),
                payload: json!({ "type": "interrupted_fixture" }),
            };
            store
                .transact_command(command, |_| {
                    Ok(vec![
                        NewTaskEvent::task_created(format!("interrupted {index}")),
                        NewTaskEvent::run_started(segment_id, Utc::now()),
                        NewTaskEvent::run_completed(
                            segment_id,
                            Utc::now(),
                            RunTerminalReason::InterruptedByRestart,
                            true,
                        ),
                    ])
                })
                .unwrap();
        }
        let active_task_id = TaskId::new();
        let active_segment_id = RunSegmentId::new();
        store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id: active_task_id,
                    idempotency_key: "active-fixture".into(),
                    expected_stream_version: 0,
                    authority: TaskStore::supervisor_authority(),
                    payload: json!({ "type": "active_fixture" }),
                },
                |_| {
                    Ok(vec![
                        NewTaskEvent::task_created("active"),
                        NewTaskEvent::run_started(active_segment_id, Utc::now()),
                    ])
                },
            )
            .unwrap();
        store
            .mark_segment_start_delivered(active_task_id, active_segment_id)
            .unwrap();

        let supervisor = Supervisor::start(
            Arc::clone(&store),
            Arc::new(IdleFactory),
            SupervisorQuotas::new(1, 1),
        )
        .unwrap();

        assert_eq!(supervisor.resident_actor_count().await, 0);

        let queue_item_id = QueueItemId::new();
        let outcome = supervisor
            .dispatch(
                active_task_id,
                ValidatedTaskCommand::Queue {
                    command: AcceptedCommand {
                        command_id: CommandId::new(),
                        task_id: active_task_id,
                        idempotency_key: "route-active-fixture".into(),
                        expected_stream_version: store.stream_version(active_task_id).unwrap(),
                        authority: TaskStore::user_authority(harness_contracts::ClientId::new()),
                        payload: json!({ "type": "queue" }),
                    },
                    queue_item_id,
                    queue_command: crate::QueueCommand::Submit {
                        queue_item_id,
                        content: "route".into(),
                        attachments: Vec::new(),
                        context_references: Vec::new(),
                        created_at: Utc::now(),
                    },
                },
            )
            .await
            .unwrap();
        assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
        assert_eq!(supervisor.resident_actor_count().await, 1);
    }

    #[tokio::test]
    async fn supervisor_start_recovers_detached_children_and_releases_their_lease() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        let parent_task_id = TaskId::new();
        let parent_segment_id = RunSegmentId::new();
        store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id: parent_task_id,
                    idempotency_key: "recovery-parent".into(),
                    expected_stream_version: 0,
                    authority: TaskStore::supervisor_authority(),
                    payload: json!({ "type": "recovery_parent" }),
                },
                |_| {
                    Ok(vec![
                        NewTaskEvent::task_created("recovery parent"),
                        NewTaskEvent::run_started(parent_segment_id, Utc::now()),
                    ])
                },
            )
            .unwrap();
        let parent_actor_id = store
            .task_projection(parent_task_id)
            .unwrap()
            .unwrap()
            .actor_id
            .unwrap();
        let child_task_id = TaskId::new();
        let child_actor_id = ActorId::new();
        let parent_workspace_lease_id = WorkspaceLeaseId::new();
        let parent_lease = store
            .acquire_workspace_lease(AcquireTaskWorkspaceLease {
                lease_id: parent_workspace_lease_id,
                task_id: parent_task_id,
                actor_id: parent_actor_id,
                mode: WorkspaceMode::Current,
                canonical_root: root.path().to_string_lossy().into_owned(),
                worktree_path: None,
                branch: None,
                writable: true,
                requested_at: Utc::now(),
                expires_at: None,
                baseline_commit: None,
                baseline_status: String::new(),
            })
            .unwrap();
        assert!(matches!(
            parent_lease,
            harness_journal::TaskWorkspaceAcquireOutcome::Acquired(_)
        ));
        store
            .create_subagent_actor_checked(CreateSubagentActorRequest {
                child_task_id,
                actor_id: child_actor_id,
                segment_id: RunSegmentId::new(),
                parent_task_id,
                parent_segment_id,
                parent_actor_id,
                parent_workspace_lease_id,
                delegation_id: SubagentId::new(),
                context_cursor: 0,
                title: "recovery child".into(),
                started_at: Utc::now(),
            })
            .unwrap();
        let lease_id = WorkspaceLeaseId::new();
        let managed_root = root.path().join("managed-worktrees");
        let outcome = store
            .acquire_workspace_lease(AcquireTaskWorkspaceLease {
                lease_id,
                task_id: child_task_id,
                actor_id: child_actor_id,
                mode: WorkspaceMode::ManagedWorktree,
                canonical_root: root.path().to_string_lossy().into_owned(),
                worktree_path: Some(
                    managed_root
                        .join(lease_id.to_string())
                        .to_string_lossy()
                        .into_owned(),
                ),
                branch: Some(format!("jyowo/task-{lease_id}")),
                writable: true,
                requested_at: Utc::now(),
                expires_at: None,
                baseline_commit: Some("0123456789abcdef".into()),
                baseline_status: String::new(),
            })
            .unwrap();
        assert!(matches!(
            outcome,
            harness_journal::TaskWorkspaceAcquireOutcome::Acquired(_)
        ));
        store
            .apply_subagent_lifecycle(SubagentLifecycleCommand {
                parent_task_id,
                child_task_id,
                actor_id: child_actor_id,
                authority: SubagentLifecycleAuthority::Supervisor,
                transition: SubagentLifecycleTransition::Running {
                    workspace_lease_id: lease_id,
                    context_cursor: 0,
                },
            })
            .unwrap();
        store
            .apply_subagent_lifecycle(SubagentLifecycleCommand {
                parent_task_id,
                child_task_id,
                actor_id: child_actor_id,
                authority: SubagentLifecycleAuthority::Supervisor,
                transition: SubagentLifecycleTransition::Background,
            })
            .unwrap();
        let detached = &store
            .task_projection(parent_task_id)
            .unwrap()
            .unwrap()
            .subagents[0];
        assert_eq!(detached.state, SubagentActorState::Background);
        assert!(detached.detached);

        let _supervisor = Supervisor::start(
            Arc::clone(&store),
            Arc::new(IdleFactory),
            SupervisorQuotas::new(1, 1),
        )
        .unwrap();

        let child = &store
            .task_projection(parent_task_id)
            .unwrap()
            .unwrap()
            .subagents[0];
        assert_eq!(child.state, SubagentActorState::Failed);
        assert!(child.detached);
        assert_eq!(
            store.workspace_lease(lease_id).unwrap().unwrap().state,
            TaskWorkspaceLeaseState::Released
        );
        let terminals = store
            .task_events_after(parent_task_id, 0, 64)
            .unwrap()
            .into_iter()
            .filter(|event| event.event_type == "subagent.terminal")
            .collect::<Vec<_>>();
        assert_eq!(terminals.len(), 1);
        assert_eq!(terminals[0].source.kind, EventSourceKind::Recovery);
    }

    #[tokio::test]
    async fn configured_subagent_supervisor_uses_the_supervisors_store_workspace_and_quota() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        let runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory> =
            Arc::new(|_context: crate::WorkspaceSubagentRunContext| {
                Err(harness_subagent::SubagentError::Engine(
                    "runner is not used by this test".into(),
                ))
            });
        let supervisor = Supervisor::start_with_subagents(
            Arc::clone(&store),
            Arc::new(IdleFactory),
            SupervisorQuotas::new(1, 1),
            runner_factory,
            Arc::new(NoopRedactor),
            4,
        )
        .unwrap();

        let permit = supervisor.acquire_subagent_permit().await.unwrap();
        assert!(tokio::time::timeout(
            Duration::from_millis(25),
            supervisor.acquire_subagent_permit()
        )
        .await
        .is_err());
        drop(permit);
        let _permit = supervisor.acquire_subagent_permit().await.unwrap();
    }

    #[tokio::test]
    async fn production_components_share_the_supplied_permission_broker() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        let redactor: Arc<dyn Redactor> = Arc::new(NoopRedactor);
        let permissions = Arc::new(PermissionBroker::new(
            Arc::clone(&store),
            Arc::clone(&redactor),
        ));
        let runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory> =
            Arc::new(|_context: crate::WorkspaceSubagentRunContext| {
                Err(harness_subagent::SubagentError::Engine(
                    "runner is not used by this test".into(),
                ))
            });

        let supervisor = Supervisor::start_with_runtime_components(
            Arc::clone(&store),
            Arc::new(IdleFactory),
            SupervisorQuotas::new(1, 1),
            runner_factory,
            redactor,
            4,
            Arc::clone(&permissions),
        )
        .unwrap();

        assert!(Arc::ptr_eq(&permissions, &supervisor.permission_broker()));
    }

    #[tokio::test]
    async fn supervisor_expires_stale_workspace_leases_without_ui_traffic() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        let task_id = TaskId::new();
        store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: "workspace-expiry-task".into(),
                    expected_stream_version: 0,
                    authority: TaskStore::supervisor_authority(),
                    payload: json!({ "type": "fixture" }),
                },
                |_| Ok(vec![NewTaskEvent::task_created("workspace expiry")]),
            )
            .unwrap();
        let lease_id = WorkspaceLeaseId::new();
        store
            .acquire_workspace_lease(AcquireTaskWorkspaceLease {
                lease_id,
                task_id,
                actor_id: ActorId::new(),
                mode: WorkspaceMode::Current,
                canonical_root: root.path().to_str().unwrap().to_owned(),
                worktree_path: None,
                branch: None,
                writable: true,
                requested_at: Utc::now(),
                expires_at: Some(Utc::now() - chrono::Duration::seconds(1)),
                baseline_commit: None,
                baseline_status: String::new(),
            })
            .unwrap();
        let _supervisor = Supervisor::start(
            Arc::clone(&store),
            Arc::new(IdleFactory),
            SupervisorQuotas::new(1, 1),
        )
        .unwrap();

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if store.workspace_lease(lease_id).unwrap().unwrap().state
                    == TaskWorkspaceLeaseState::Expired
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
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
    fn failed_parent_stop_compensation_remains_queued_until_a_retry_succeeds() {
        let task_id = TaskId::new();
        let mut pending = std::collections::HashSet::new();

        record_parent_stop_compensation(&mut pending, task_id, |_| Err::<(), _>("store down"));
        assert!(pending.contains(&task_id));

        retry_parent_stop_compensations(&mut pending, |_| Err::<(), _>("still down"));
        assert!(pending.contains(&task_id));

        retry_parent_stop_compensations(&mut pending, |_| Ok::<(), &str>(()));
        assert!(!pending.contains(&task_id));
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

    #[test]
    fn outbox_read_errors_remain_retryable() {
        let result = Err::<Option<harness_journal::PendingSegmentStart>, _>(
            TaskStoreError::ProjectionIntegrity("temporary read failure".into()),
        );

        assert!(pending_segment_start_requires_retry(result));
        assert!(!pending_segment_start_requires_retry(Ok(None)));
    }

    #[test]
    fn pending_start_retry_delay_is_exponential_and_capped() {
        assert_eq!(
            pending_segment_start_retry_delay(1),
            Duration::from_millis(25)
        );
        assert_eq!(
            pending_segment_start_retry_delay(2),
            Duration::from_millis(50)
        );
        assert_eq!(pending_segment_start_retry_delay(7), Duration::from_secs(1));
        assert_eq!(
            pending_segment_start_retry_delay(64),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn actor_failure_recovers_the_message_promoted_for_its_yielding_run() {
        let root = tempfile::tempdir().unwrap();
        let store = TaskStore::open(root.path().join("tasks.sqlite")).unwrap();
        let task_id = TaskId::new();
        let segment_id = harness_contracts::RunSegmentId::new();
        let queue_item_id = QueueItemId::new();
        let prepared_at = Utc::now();
        let prepare = AcceptedCommand {
            command_id: CommandId::new(),
            task_id,
            idempotency_key: format!("prepare-yielding-{}", CommandId::new()),
            expected_stream_version: 0,
            authority: TaskStore::supervisor_authority(),
            payload: json!({ "type": "prepare_yielding" }),
        };
        let outcome = store
            .transact_command(prepare, |_| {
                Ok(vec![
                    NewTaskEvent::task_created("yielding actor"),
                    NewTaskEvent::run_started(segment_id, prepared_at),
                    NewTaskEvent::message_queued(
                        queue_item_id,
                        "promoted message",
                        Vec::new(),
                        Vec::new(),
                        prepared_at,
                    ),
                    NewTaskEvent::message_promoted(queue_item_id, 1),
                    NewTaskEvent::run_yield_requested(segment_id, false, prepared_at),
                ])
            })
            .unwrap();
        assert!(matches!(outcome, CommandOutcome::Accepted { .. }));

        assert!(persist_actor_failure(&store, task_id, Some(segment_id)).unwrap());

        let projection = store.task_projection(task_id).unwrap().unwrap();
        assert_eq!(projection.current_run.unwrap().state, RunState::Failed);
        assert_eq!(projection.queue[0].state, QueueItemState::Queued);
        assert_eq!(
            store
                .queue_item_projection(task_id, queue_item_id)
                .unwrap()
                .unwrap()
                .state,
            QueueItemState::Queued
        );
    }
}

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use harness_contracts::{
    ClientId, CommandId, RunSegmentId, RunState, RunTerminalReason, TaskId, TaskState,
};
use harness_daemon::{
    RunCoordinatorEvent, RunCoordinatorFactory, RunningSegment, StartSegmentRequest, Supervisor,
    SupervisorEvent, SupervisorQuotas, ValidatedTaskCommand,
};
use harness_journal::{AcceptedCommand, CommandOutcome, NewTaskEvent, TaskStore};
use serde_json::json;
use tokio::sync::mpsc;

#[derive(Clone, Default)]
struct ControlledFactory {
    state: Arc<Mutex<FactoryState>>,
}

#[derive(Default)]
struct FactoryState {
    active: HashMap<TaskId, usize>,
    maximum_active: HashMap<TaskId, usize>,
    starts: HashMap<TaskId, Vec<RunSegmentId>>,
    controls: HashMap<RunSegmentId, mpsc::UnboundedSender<RunCoordinatorEvent>>,
    panic_next: Option<TaskId>,
}

impl ControlledFactory {
    fn start_count(&self, task_id: TaskId) -> usize {
        self.state
            .lock()
            .unwrap()
            .starts
            .get(&task_id)
            .map_or(0, Vec::len)
    }

    fn maximum_active(&self, task_id: TaskId) -> usize {
        self.state
            .lock()
            .unwrap()
            .maximum_active
            .get(&task_id)
            .copied()
            .unwrap_or(0)
    }

    fn complete(&self, task_id: TaskId, segment_id: RunSegmentId) {
        let sender = {
            let mut state = self.state.lock().unwrap();
            let active = state.active.get_mut(&task_id).unwrap();
            *active -= 1;
            state.controls.remove(&segment_id).unwrap()
        };
        sender
            .send(RunCoordinatorEvent::Completed {
                segment_id,
                terminal_reason: RunTerminalReason::Completed,
                incomplete_output: false,
                ended_at: Utc::now(),
            })
            .unwrap();
    }

    fn close_without_terminal_event(&self, task_id: TaskId, segment_id: RunSegmentId) {
        let mut state = self.state.lock().unwrap();
        let active = state.active.get_mut(&task_id).unwrap();
        *active -= 1;
        state.controls.remove(&segment_id).unwrap();
    }

    fn panic_on_next_start(&self, task_id: TaskId) {
        self.state.lock().unwrap().panic_next = Some(task_id);
    }
}

impl RunCoordinatorFactory for ControlledFactory {
    fn spawn(&self, request: StartSegmentRequest) -> RunningSegment {
        let mut state = self.state.lock().unwrap();
        if state.panic_next == Some(request.task_id) {
            state.panic_next = None;
            drop(state);
            panic!("coordinator factory panic");
        }
        let active = state.active.entry(request.task_id).or_default();
        *active += 1;
        let active_now = *active;
        let maximum = state.maximum_active.entry(request.task_id).or_default();
        *maximum = (*maximum).max(active_now);
        state
            .starts
            .entry(request.task_id)
            .or_default()
            .push(request.segment_id);
        let (sender, receiver) = mpsc::unbounded_channel();
        state.controls.insert(request.segment_id, sender);
        RunningSegment::new(receiver)
    }
}

#[tokio::test]
async fn one_task_has_one_foreground_run_while_another_task_runs_in_parallel() {
    let (store, _root) = test_store();
    let task_a = create_task(&store, "task A");
    let task_b = create_task(&store, "task B");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(2, 4),
    )
    .unwrap();

    let segment_a = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_a, start_command(&store, task_a, segment_a))
            .await
            .unwrap()
    ));
    let rejected = supervisor
        .dispatch(task_a, start_command(&store, task_a, RunSegmentId::new()))
        .await
        .unwrap();
    assert!(matches!(rejected, CommandOutcome::Rejected { .. }));

    let segment_b = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_b, start_command(&store, task_b, segment_b))
            .await
            .unwrap()
    ));

    assert_eq!(factory.start_count(task_a), 1);
    assert_eq!(factory.start_count(task_b), 1);
    assert_eq!(factory.maximum_active(task_a), 1);
    assert_eq!(factory.maximum_active(task_b), 1);

    factory.complete(task_a, segment_a);
    factory.complete(task_b, segment_b);
    wait_for_state(&store, task_a, TaskState::Completed).await;
    wait_for_state(&store, task_b, TaskState::Completed).await;
}

#[tokio::test]
async fn accepted_start_command_replay_does_not_spawn_a_second_coordinator() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "idempotent start");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(2, 1),
    )
    .unwrap();

    let segment_id = RunSegmentId::new();
    let start = start_command(&store, task_id, segment_id);
    assert!(accepted(
        supervisor.dispatch(task_id, start.clone()).await.unwrap()
    ));
    assert!(accepted(supervisor.dispatch(task_id, start).await.unwrap()));

    assert_eq!(factory.start_count(task_id), 1);
    assert_eq!(factory.maximum_active(task_id), 1);
}

#[tokio::test]
async fn a_completed_task_accepts_a_new_message_as_a_new_segment() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "repeatable");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    let first = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, first))
            .await
            .unwrap()
    ));
    factory.complete(task_id, first);
    wait_for_state(&store, task_id, TaskState::Completed).await;

    let second = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, second))
            .await
            .unwrap()
    ));
    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(projection.current_run.unwrap().segment_id, second);
    assert_eq!(factory.start_count(task_id), 2);
    assert_eq!(factory.maximum_active(task_id), 1);
}

#[tokio::test]
async fn actor_panic_is_persisted_and_does_not_stop_another_task() {
    let (store, _root) = test_store();
    let task_a = create_task(&store, "crashing");
    let task_b = create_task(&store, "survivor");
    let factory = Arc::new(ControlledFactory::default());
    factory.panic_on_next_start(task_a);
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(2, 2),
    )
    .unwrap();
    let mut events = supervisor.subscribe();

    let failed_segment = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_a, start_command(&store, task_a, failed_segment),)
            .await
            .unwrap()
    ));

    let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(event, SupervisorEvent::ActorFailed { task_id } if task_id == task_a));
    let failed = store.task_projection(task_a).unwrap().unwrap();
    assert_eq!(failed.state, TaskState::Failed);
    let failed_run = failed.current_run.unwrap();
    assert_eq!(failed_run.segment_id, failed_segment);
    assert_eq!(failed_run.state, RunState::Failed);

    let survivor_segment = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_b, start_command(&store, task_b, survivor_segment),)
            .await
            .unwrap()
    ));
    assert_eq!(factory.start_count(task_b), 1);
}

#[tokio::test]
async fn exhausted_global_quota_does_not_publish_a_run_that_never_started() {
    let (store, _root) = test_store();
    let task_a = create_task(&store, "occupies quota");
    let task_b = create_task(&store, "cannot start");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    let segment_a = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_a, start_command(&store, task_a, segment_a))
            .await
            .unwrap()
    ));
    let outcome = supervisor
        .dispatch(task_b, start_command(&store, task_b, RunSegmentId::new()))
        .await
        .unwrap();

    assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    assert_eq!(
        store.task_projection(task_b).unwrap().unwrap().state,
        TaskState::Idle
    );
    assert_eq!(factory.start_count(task_b), 0);
}

#[tokio::test]
async fn coordinator_channel_close_fails_the_run_and_releases_its_slot() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "coordinator closes");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    let first = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, first))
            .await
            .unwrap()
    ));
    factory.close_without_terminal_event(task_id, first);
    wait_for_state(&store, task_id, TaskState::Failed).await;

    let second = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, second))
            .await
            .unwrap()
    ));
    assert_eq!(factory.start_count(task_id), 2);
}

#[tokio::test]
async fn conflicting_client_command_is_rejected_without_killing_the_actor() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "command conflict");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();
    let mut events = supervisor.subscribe();

    let first_segment = RunSegmentId::new();
    let first_command = start_command(&store, task_id, first_segment);
    assert!(accepted(
        supervisor
            .dispatch(task_id, first_command.clone())
            .await
            .unwrap()
    ));
    let mut conflicting = first_command;
    let ValidatedTaskCommand::StartSegment {
        command,
        segment_id,
        ..
    } = &mut conflicting;
    *segment_id = RunSegmentId::new();
    command.payload = json!({ "segmentId": segment_id });

    let outcome = supervisor.dispatch(task_id, conflicting).await.unwrap();
    assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), events.recv())
            .await
            .is_err()
    );
    assert_eq!(
        store.task_projection(task_id).unwrap().unwrap().state,
        TaskState::Running
    );

    factory.complete(task_id, first_segment);
    wait_for_state(&store, task_id, TaskState::Completed).await;
}

#[tokio::test]
async fn unknown_task_is_rejected_without_publishing_actor_failure() {
    let (store, _root) = test_store();
    let factory = Arc::new(ControlledFactory::default());
    let supervisor =
        Supervisor::start(Arc::clone(&store), factory, SupervisorQuotas::new(1, 1)).unwrap();
    let mut events = supervisor.subscribe();
    let task_id = TaskId::new();
    let command = ValidatedTaskCommand::StartSegment {
        command: command(task_id, 0, json!({ "missing": true })),
        segment_id: RunSegmentId::new(),
        started_at: Utc::now(),
    };

    let outcome = supervisor.dispatch(task_id, command).await.unwrap();
    assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), events.recv())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn subagent_quota_is_owned_by_the_supervisor_and_applies_backpressure() {
    let (store, _root) = test_store();
    let supervisor = Supervisor::start(
        store,
        Arc::new(ControlledFactory::default()),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    let first = supervisor.acquire_subagent_permit().await.unwrap();
    assert!(tokio::time::timeout(
        Duration::from_millis(50),
        supervisor.acquire_subagent_permit()
    )
    .await
    .is_err());
    drop(first);
    let _second =
        tokio::time::timeout(Duration::from_secs(1), supervisor.acquire_subagent_permit())
            .await
            .unwrap()
            .unwrap();
}

#[test]
fn zero_supervisor_quota_is_rejected() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _guard = runtime.enter();
    let (store, _root) = test_store();
    assert!(matches!(
        Supervisor::start(
            store,
            Arc::new(ControlledFactory::default()),
            SupervisorQuotas::new(0, 1),
        ),
        Err(harness_daemon::SupervisorError::InvalidQuota)
    ));
}

fn test_store() -> (Arc<TaskStore>, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    (store, root)
}

fn create_task(store: &TaskStore, title: &str) -> TaskId {
    let task_id = TaskId::new();
    let outcome = store
        .transact_command(command(task_id, 0, json!({ "create": title })), |_| {
            Ok(vec![NewTaskEvent::task_created(title)])
        })
        .unwrap();
    assert!(accepted(outcome));
    task_id
}

fn start_command(
    store: &TaskStore,
    task_id: TaskId,
    segment_id: RunSegmentId,
) -> ValidatedTaskCommand {
    ValidatedTaskCommand::StartSegment {
        command: command(
            task_id,
            store.stream_version(task_id).unwrap(),
            json!({ "segmentId": segment_id }),
        ),
        segment_id,
        started_at: Utc::now(),
    }
}

fn command(
    task_id: TaskId,
    expected_stream_version: u64,
    payload: serde_json::Value,
) -> AcceptedCommand {
    AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("daemon-{}", CommandId::new()),
        expected_stream_version,
        authority: TaskStore::user_authority(ClientId::new()),
        payload,
    }
}

fn accepted(outcome: CommandOutcome) -> bool {
    matches!(outcome, CommandOutcome::Accepted { .. })
}

async fn wait_for_state(store: &TaskStore, task_id: TaskId, state: TaskState) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if store
                .task_projection(task_id)
                .unwrap()
                .is_some_and(|projection| projection.state == state)
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

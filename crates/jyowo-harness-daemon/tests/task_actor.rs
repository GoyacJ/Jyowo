use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use chrono::Utc;
use harness_contracts::{
    BlobId, ClientId, CommandId, QueueItemId, QueueItemState, RequestId, RunSegmentId, RunState,
    RunTerminalReason, TaskId, TaskState,
};
use harness_daemon::{
    DaemonPermissionKind, PermissionOption, PermissionRequestDraft, QueueCommand,
    RunCoordinatorEvent, RunCoordinatorFactory, RunningSegment, StartSegmentRequest, Supervisor,
    SupervisorQuotas, ValidatedTaskCommand,
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
    requests: HashMap<TaskId, Vec<StartSegmentRequest>>,
    controls: HashMap<RunSegmentId, mpsc::UnboundedSender<RunCoordinatorEvent>>,
    panic_next: Option<TaskId>,
    block_next: HashMap<TaskId, Arc<StartGate>>,
}

#[derive(Default)]
struct StartGate {
    entered: AtomicBool,
    released: Mutex<bool>,
    released_changed: Condvar,
}

impl StartGate {
    fn wait(&self) {
        self.entered.store(true, Ordering::Release);
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.released_changed.wait(released).unwrap();
        }
    }

    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.released_changed.notify_all();
    }
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

    fn requests(&self, task_id: TaskId) -> Vec<StartSegmentRequest> {
        self.state
            .lock()
            .unwrap()
            .requests
            .get(&task_id)
            .cloned()
            .unwrap_or_default()
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

    fn block_next_start(&self, task_id: TaskId) -> Arc<StartGate> {
        let gate = Arc::new(StartGate::default());
        self.state
            .lock()
            .unwrap()
            .block_next
            .insert(task_id, Arc::clone(&gate));
        gate
    }
}

impl RunCoordinatorFactory for ControlledFactory {
    fn spawn_idempotent(
        &self,
        request: StartSegmentRequest,
        _workspace_tools: harness_daemon::WorkspaceToolDispatcher,
        _subagent_runner: Arc<dyn harness_subagent::SubagentRunner>,
    ) -> RunningSegment {
        let gate = self
            .state
            .lock()
            .unwrap()
            .block_next
            .remove(&request.task_id);
        if let Some(gate) = gate {
            gate.wait();
        }
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
        state
            .requests
            .entry(request.task_id)
            .or_default()
            .push(request.clone());
        let (sender, receiver) = mpsc::unbounded_channel();
        state.controls.insert(request.segment_id, sender);
        RunningSegment::new(receiver)
    }
}

#[path = "task_actor/task_actor_cases.rs"]
mod task_actor_cases;

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

async fn wait_for_start_count(factory: &ControlledFactory, task_id: TaskId, expected: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if factory.start_count(task_id) == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

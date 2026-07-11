use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use futures::future::join_all;
use harness_contracts::{
    ActorId, ClientId, CommandId, Message, MessageId, MessagePart, MessageRole, NoopRedactor,
    RunSegmentId, RunTerminalReason, SubagentActorState, SubagentId, SubagentStatus, TaskId,
    TaskState, TurnInput, UsageSnapshot, WorkspaceMode,
};
use harness_daemon::{
    RunCoordinatorEvent, RunCoordinatorFactory, RunningSegment, StartSegmentRequest,
    SubagentParentBinding, SubagentSupervisor, Supervisor, SupervisorQuotas, ValidatedTaskCommand,
    WorkspaceAccess, WorkspaceCoordinator, WorkspaceExecutionKind, WorkspaceLeaseRequest,
    WorkspaceSubagentRunContext, WorkspaceSubagentRunnerFactory,
};
use harness_journal::{
    AcceptedCommand, CommandOutcome, CommandRejection, NewTaskEvent, TaskStore, TaskStoreError,
};
use harness_subagent::{
    ParentContext, SubagentAnnouncement, SubagentError, SubagentHandle, SubagentRunner,
    SubagentSpec,
};
use serde_json::json;
use tokio::sync::{mpsc, Notify, Semaphore};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_scoped_idempotency_and_stale_versions_remain_consistent_under_race() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = create_task(&store, "original");
    let shared_key = "same-key-from-two-clients";
    let client_a = ClientId::new();
    let client_b = ClientId::new();
    let command_a = rename_command(task_id, client_a, shared_key, "client A");
    let command_b = rename_command(task_id, client_b, shared_key, "client B");
    let barrier = Arc::new(Barrier::new(2));

    let first = race_rename(
        Arc::clone(&store),
        Arc::clone(&barrier),
        command_a.clone(),
        "client A",
    );
    let second = race_rename(
        Arc::clone(&store),
        Arc::clone(&barrier),
        command_b.clone(),
        "client B",
    );
    let (outcome_a, outcome_b) = tokio::join!(first, second);
    let outcome_a = outcome_a.unwrap().unwrap();
    let outcome_b = outcome_b.unwrap().unwrap();

    let outcomes = [&outcome_a, &outcome_b];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, CommandOutcome::Accepted { .. }))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(
                outcome,
                CommandOutcome::Rejected {
                    rejection: CommandRejection::WrongExpectedVersion {
                        expected: 1,
                        actual: 2,
                    },
                    ..
                }
            ))
            .count(),
        1
    );

    let replay_a = store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                ..command_a
            },
            |_| panic!("client A idempotency replay must not execute its decision"),
        )
        .unwrap();
    let replay_b = store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                ..command_b
            },
            |_| panic!("client B idempotency replay must not execute its decision"),
        )
        .unwrap();
    assert_eq!(replay_a, outcome_a);
    assert_eq!(replay_b, outcome_b);

    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(projection.stream_version, 2);
    assert!(matches!(projection.title.as_str(), "client A" | "client B"));
    let events = store.task_events_after(task_id, 0, 16).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, "task.created");
    assert_eq!(events[1].event_type, "task.title_changed");
    assert_eq!(events[1].stream_sequence, projection.stream_version);
    assert_eq!(events[1].global_offset, projection.last_global_offset);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn twenty_task_actors_share_one_global_foreground_quota() {
    const FOREGROUND_QUOTA: usize = 20;
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_ids = (0..=FOREGROUND_QUOTA)
        .map(|index| create_task(&store, &format!("task {index}")))
        .collect::<Vec<_>>();
    let factory = Arc::new(QuotaRunFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(FOREGROUND_QUOTA, 8),
    )
    .unwrap();

    let dispatches = task_ids.iter().copied().map(|task_id| {
        supervisor.dispatch(task_id, start_command(&store, task_id, RunSegmentId::new()))
    });
    let results = join_all(dispatches)
        .await
        .into_iter()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    let accepted_task_ids = task_ids
        .iter()
        .copied()
        .zip(&results)
        .filter_map(|(task_id, outcome)| {
            matches!(outcome, CommandOutcome::Accepted { .. }).then_some(task_id)
        })
        .collect::<HashSet<_>>();
    let rejected_task_ids = task_ids
        .iter()
        .copied()
        .zip(&results)
        .filter_map(|(task_id, outcome)| {
            matches!(outcome, CommandOutcome::Rejected { .. }).then_some(task_id)
        })
        .collect::<Vec<_>>();

    assert_eq!(accepted_task_ids.len(), FOREGROUND_QUOTA);
    assert_eq!(rejected_task_ids.len(), 1);
    assert!(
        results
            .iter()
            .filter(|outcome| matches!(
                outcome,
                CommandOutcome::Rejected {
                    rejection: CommandRejection::InvalidCommand { message },
                    ..
                } if message == "global foreground-run quota is exhausted"
            ))
            .count()
            == 1
    );
    wait_for_active_runs(&factory, FOREGROUND_QUOTA).await;
    let factory_snapshot = factory.snapshot();
    assert_eq!(factory_snapshot.active, FOREGROUND_QUOTA);
    assert_eq!(factory_snapshot.maximum_active, FOREGROUND_QUOTA);
    assert_eq!(factory_snapshot.started_task_ids, accepted_task_ids);
    assert_eq!(
        store.latest_global_offset().unwrap(),
        (task_ids.len() + FOREGROUND_QUOTA) as u64
    );
    assert!(accepted_task_ids.iter().all(|task_id| {
        store
            .task_projection(*task_id)
            .unwrap()
            .is_some_and(|task| task.state == TaskState::Running && task.stream_version == 2)
    }));
    assert_eq!(
        store
            .task_projection(rejected_task_ids[0])
            .unwrap()
            .unwrap()
            .state,
        TaskState::Idle
    );

    factory.complete_all();
    join_all(
        accepted_task_ids
            .iter()
            .copied()
            .map(|task_id| wait_for_state(Arc::clone(&store), task_id, TaskState::Completed)),
    )
    .await;
    assert_eq!(factory.snapshot().active, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn one_task_runs_eight_subagents_and_rejects_the_ninth_without_partial_state() {
    const SUBAGENT_QUOTA: usize = 8;
    let root = tempfile::tempdir().unwrap();
    let workspace_root = root.path().join("workspace");
    std::fs::create_dir(&workspace_root).unwrap();
    init_git_repo(&workspace_root);
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let workspace = Arc::new(
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap(),
    );
    let delegate = Arc::new(ConcurrentSubagentRunner::default());
    let runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory> = Arc::new(SharedRunnerFactory {
        delegate: Arc::clone(&delegate),
    });
    let subagents = Arc::new(SubagentSupervisor::new(
        Arc::clone(&store),
        Arc::clone(&workspace),
        runner_factory,
        Arc::new(NoopRedactor),
        2,
        SUBAGENT_QUOTA,
    ));
    let (parent_task_id, parent_actor_id, parent_segment_id) =
        create_running_parent(&store, "concurrent parent");
    acquire_parent_workspace(&workspace, &workspace_root, parent_task_id, parent_actor_id);
    let runner = subagents.bind(SubagentParentBinding {
        parent_task_id,
        parent_segment_id,
        parent_actor_id,
        depth: 0,
    });

    let children = (0..SUBAGENT_QUOTA)
        .map(|index| {
            let runner = Arc::clone(&runner);
            tokio::spawn(async move {
                runner
                    .spawn(
                        SubagentSpec::minimal(format!("child {index}"), "concurrency gate"),
                        turn_input(&format!("child {index}")),
                        ParentContext::for_test(0),
                    )
                    .await
            })
        })
        .collect::<Vec<_>>();
    delegate.wait_until_started(SUBAGENT_QUOTA).await;

    let running_projection = store.task_projection(parent_task_id).unwrap().unwrap();
    assert_eq!(running_projection.subagents.len(), SUBAGENT_QUOTA);
    assert!(running_projection
        .subagents
        .iter()
        .all(|child| child.state == SubagentActorState::Running));
    assert_eq!(
        running_projection
            .subagents
            .iter()
            .map(|child| child.child_task_id)
            .collect::<HashSet<_>>()
            .len(),
        SUBAGENT_QUOTA
    );
    assert_eq!(delegate.active.load(Ordering::Acquire), SUBAGENT_QUOTA);
    assert_eq!(
        delegate.maximum_active.load(Ordering::Acquire),
        SUBAGENT_QUOTA
    );

    let ninth = runner
        .spawn(
            SubagentSpec::minimal("ninth", "must be rejected"),
            turn_input("ninth"),
            ParentContext::for_test(0),
        )
        .await;
    assert!(matches!(ninth, Err(SubagentError::ConcurrentLimitExceeded)));
    assert_eq!(
        store
            .task_projection(parent_task_id)
            .unwrap()
            .unwrap()
            .subagents
            .len(),
        SUBAGENT_QUOTA
    );

    delegate.release(SUBAGENT_QUOTA);
    for child in children {
        child.await.unwrap().unwrap().wait().await.unwrap();
    }
    let completed = store.task_projection(parent_task_id).unwrap().unwrap();
    assert_eq!(completed.subagents.len(), SUBAGENT_QUOTA);
    assert!(completed
        .subagents
        .iter()
        .all(|child| child.state == SubagentActorState::Completed));
    assert_eq!(delegate.active.load(Ordering::Acquire), 0);
}

fn race_rename(
    store: Arc<TaskStore>,
    barrier: Arc<Barrier>,
    command: AcceptedCommand,
    title: &'static str,
) -> tokio::task::JoinHandle<Result<CommandOutcome, TaskStoreError>> {
    tokio::task::spawn_blocking(move || {
        barrier.wait();
        store.transact_command(command, |_| {
            Ok(vec![NewTaskEvent::task_title_changed(title)])
        })
    })
}

fn create_task(store: &TaskStore, title: &str) -> TaskId {
    let task_id = TaskId::new();
    let outcome = store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: format!("create-{task_id}"),
                expected_stream_version: 0,
                authority: TaskStore::user_authority(ClientId::new()),
                payload: json!({ "type": "create_task", "title": title }),
            },
            |_| Ok(vec![NewTaskEvent::task_created(title)]),
        )
        .unwrap();
    assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
    task_id
}

fn rename_command(
    task_id: TaskId,
    client_id: ClientId,
    idempotency_key: &str,
    title: &str,
) -> AcceptedCommand {
    AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: idempotency_key.into(),
        expected_stream_version: 1,
        authority: TaskStore::user_authority(client_id),
        payload: json!({ "type": "rename_task", "title": title }),
    }
}

fn start_command(
    store: &TaskStore,
    task_id: TaskId,
    segment_id: RunSegmentId,
) -> ValidatedTaskCommand {
    ValidatedTaskCommand::StartSegment {
        command: AcceptedCommand {
            command_id: CommandId::new(),
            task_id,
            idempotency_key: format!("start-{task_id}"),
            expected_stream_version: store.stream_version(task_id).unwrap(),
            authority: TaskStore::user_authority(ClientId::new()),
            payload: json!({ "type": "start_segment", "segmentId": segment_id }),
        },
        segment_id,
        started_at: Utc::now(),
    }
}

async fn wait_for_state(store: Arc<TaskStore>, task_id: TaskId, expected: TaskState) {
    tokio::time::timeout(Duration::from_secs(5), async move {
        loop {
            if store
                .task_projection(task_id)
                .unwrap()
                .is_some_and(|task| task.state == expected)
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("task did not reach expected terminal state");
}

async fn wait_for_active_runs(factory: &QuotaRunFactory, expected: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if factory.snapshot().active == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("run coordinators did not reach the expected concurrency");
}

#[derive(Default)]
struct QuotaRunFactory {
    state: Mutex<QuotaRunState>,
}

#[derive(Default)]
struct QuotaRunState {
    active: usize,
    maximum_active: usize,
    segments: HashMap<TaskId, (RunSegmentId, mpsc::UnboundedSender<RunCoordinatorEvent>)>,
}

struct QuotaRunSnapshot {
    active: usize,
    maximum_active: usize,
    started_task_ids: HashSet<TaskId>,
}

impl QuotaRunFactory {
    fn snapshot(&self) -> QuotaRunSnapshot {
        let state = self.state.lock().unwrap();
        QuotaRunSnapshot {
            active: state.active,
            maximum_active: state.maximum_active,
            started_task_ids: state.segments.keys().copied().collect(),
        }
    }

    fn complete_all(&self) {
        let completions = {
            let mut state = self.state.lock().unwrap();
            state.active = 0;
            state.segments.drain().collect::<Vec<_>>()
        };
        for (_, (segment_id, sender)) in completions {
            sender
                .send(RunCoordinatorEvent::Completed {
                    segment_id,
                    terminal_reason: RunTerminalReason::Completed,
                    incomplete_output: false,
                    ended_at: Utc::now(),
                })
                .unwrap();
        }
    }
}

impl RunCoordinatorFactory for QuotaRunFactory {
    fn spawn_idempotent(
        &self,
        request: StartSegmentRequest,
        _workspace_tools: harness_daemon::WorkspaceToolDispatcher,
        _subagent_runner: Arc<dyn SubagentRunner>,
    ) -> RunningSegment {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut state = self.state.lock().unwrap();
        state.active += 1;
        state.maximum_active = state.maximum_active.max(state.active);
        assert!(state
            .segments
            .insert(request.task_id, (request.segment_id, sender))
            .is_none());
        RunningSegment::new(receiver)
    }
}

struct ConcurrentSubagentRunner {
    started: AtomicUsize,
    active: AtomicUsize,
    maximum_active: AtomicUsize,
    changed: Notify,
    releases: Semaphore,
}

impl Default for ConcurrentSubagentRunner {
    fn default() -> Self {
        Self {
            started: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            maximum_active: AtomicUsize::new(0),
            changed: Notify::new(),
            releases: Semaphore::new(0),
        }
    }
}

impl ConcurrentSubagentRunner {
    async fn wait_until_started(&self, expected: usize) {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let changed = self.changed.notified();
                if self.started.load(Ordering::Acquire) >= expected {
                    return;
                }
                changed.await;
            }
        })
        .await
        .expect("subagents did not start before the timeout");
    }

    fn release(&self, count: usize) {
        self.releases.add_permits(count);
    }
}

#[async_trait]
impl SubagentRunner for ConcurrentSubagentRunner {
    async fn spawn(
        &self,
        _spec: SubagentSpec,
        _input: TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, SubagentError> {
        let active = self.active.fetch_add(1, Ordering::AcqRel) + 1;
        self.maximum_active.fetch_max(active, Ordering::AcqRel);
        self.started.fetch_add(1, Ordering::AcqRel);
        self.changed.notify_waiters();
        self.releases.acquire().await.unwrap().forget();
        self.active.fetch_sub(1, Ordering::AcqRel);
        Ok(SubagentHandle::ready(SubagentAnnouncement {
            subagent_id: SubagentId::new(),
            parent_session_id: parent_ctx.parent_session_id,
            status: SubagentStatus::Completed,
            summary: "completed".into(),
            result: None,
            usage: UsageSnapshot::default(),
            transcript_ref: None,
            context_report: None,
        }))
    }
}

struct SharedRunnerFactory {
    delegate: Arc<ConcurrentSubagentRunner>,
}

impl WorkspaceSubagentRunnerFactory for SharedRunnerFactory {
    fn create(
        &self,
        _context: WorkspaceSubagentRunContext,
    ) -> Result<Arc<dyn SubagentRunner>, SubagentError> {
        Ok(self.delegate.clone())
    }
}

fn create_running_parent(store: &TaskStore, title: &str) -> (TaskId, ActorId, RunSegmentId) {
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let outcome = store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: format!("create-running-{task_id}"),
                expected_stream_version: 0,
                authority: TaskStore::supervisor_authority(),
                payload: json!({ "type": "create_running_task", "title": title }),
            },
            |_| {
                Ok(vec![
                    NewTaskEvent::task_created(title),
                    NewTaskEvent::run_started(segment_id, Utc::now()),
                ])
            },
        )
        .unwrap();
    assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
    let actor_id = store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .actor_id
        .unwrap();
    (task_id, actor_id, segment_id)
}

fn acquire_parent_workspace(
    workspace: &WorkspaceCoordinator,
    workspace_root: &Path,
    task_id: TaskId,
    actor_id: ActorId,
) {
    let acquired = workspace
        .acquire(WorkspaceLeaseRequest {
            task_id,
            actor_id,
            root: workspace_root.to_path_buf(),
            mode: Some(WorkspaceMode::Current),
            access: WorkspaceAccess::Write,
            execution_kind: WorkspaceExecutionKind::Foreground,
            expires_at: None,
        })
        .unwrap();
    assert!(matches!(
        acquired,
        harness_daemon::WorkspaceAcquireOutcome::Acquired(_)
    ));
}

fn turn_input(text: &str) -> TurnInput {
    TurnInput {
        message: Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(text.into())],
            created_at: Utc::now(),
        },
        metadata: serde_json::Value::Null,
    }
}

fn init_git_repo(path: &Path) {
    git(path, ["init"]);
    git(path, ["config", "user.email", "test@example.com"]);
    git(path, ["config", "user.name", "Test User"]);
    std::fs::write(path.join("README.md"), "baseline\n").unwrap();
    git(path, ["add", "README.md"]);
    git(path, ["commit", "-m", "init"]);
}

fn git<const N: usize>(path: &Path, args: [&str; N]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

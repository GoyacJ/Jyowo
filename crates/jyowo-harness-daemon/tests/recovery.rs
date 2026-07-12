use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use chrono::Utc;
use harness_contracts::{
    ActorId, ClientId, CommandId, DeferPolicy, Event, EventId, IndeterminateToolDecision,
    IndeterminateToolResolution, NoopRedactor, PermissionMode, PermissionProjection,
    PermissionRoute, QueueItemId, QueueItemState, RequestId, RunId, RunSegmentId, RunState,
    RunTerminalReason, SessionId, TaskId, TenantId, ToolProperties, ToolResult,
    ToolUseCompletedEvent, ToolUseId, ToolUseRequestedEvent, ToolUseStartedEvent, WorkspaceMode,
};
use harness_daemon::{
    CheckpointService, CheckpointState, RecoveryService, RunCoordinatorFactory, RunningSegment,
    StartSegmentRequest, Supervisor, SupervisorError, SupervisorQuotas, ValidatedTaskCommand,
    WorkspaceBaseline,
};
use harness_journal::{
    AcceptedCommand, CommandOutcome, EventStore, NewTaskEvent, TaskEventStoreAdapter, TaskStore,
};
use serde_json::json;

#[path = "recovery/recovery_early.rs"]
mod recovery_early;
#[path = "recovery/recovery_late.rs"]
mod recovery_late;

#[derive(Default)]
struct RecordingFactory {
    requests: Mutex<Vec<StartSegmentRequest>>,
    senders: Mutex<Vec<tokio::sync::mpsc::UnboundedSender<harness_daemon::RunCoordinatorEvent>>>,
}

impl RunCoordinatorFactory for RecordingFactory {
    fn spawn_idempotent(
        &self,
        request: StartSegmentRequest,
        _workspace_tools: harness_daemon::WorkspaceToolDispatcher,
        _subagent_runner: Arc<dyn harness_subagent::SubagentRunner>,
        _agent_starters: harness_daemon::AgentStarterCapabilities,
    ) -> RunningSegment {
        self.requests.lock().unwrap().push(request);
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        self.senders.lock().unwrap().push(sender);
        RunningSegment::new(receiver)
    }
}

#[derive(Default)]
struct PanicThreeTimesFactory {
    attempts: AtomicUsize,
    first_attempt: tokio::sync::Notify,
    senders: Mutex<Vec<tokio::sync::mpsc::UnboundedSender<harness_daemon::RunCoordinatorEvent>>>,
}

impl RunCoordinatorFactory for PanicThreeTimesFactory {
    fn spawn_idempotent(
        &self,
        _request: StartSegmentRequest,
        _workspace_tools: harness_daemon::WorkspaceToolDispatcher,
        _subagent_runner: Arc<dyn harness_subagent::SubagentRunner>,
        _agent_starters: harness_daemon::AgentStarterCapabilities,
    ) -> RunningSegment {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
        if attempt == 0 {
            self.first_attempt.notify_one();
        }
        if attempt < 3 {
            panic!("simulated crash before outbox acknowledgement");
        }
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        self.senders.lock().unwrap().push(sender);
        RunningSegment::new(receiver)
    }
}

fn continue_command(
    store: &TaskStore,
    task_id: TaskId,
    segment_id: RunSegmentId,
    indeterminate_tools: Vec<IndeterminateToolDecision>,
) -> ValidatedTaskCommand {
    ValidatedTaskCommand::ContinueTask {
        command: AcceptedCommand {
            command_id: CommandId::new(),
            task_id,
            idempotency_key: format!("continue-{}", CommandId::new()),
            expected_stream_version: store.stream_version(task_id).unwrap(),
            authority: TaskStore::user_authority(ClientId::new()),
            payload: json!({ "type": "continue_task" }),
        },
        segment_id,
        started_at: Utc::now(),
        indeterminate_tools,
    }
}

fn append(store: &TaskStore, task_id: TaskId, version: u64, events: Vec<NewTaskEvent>) {
    append_with_authority(
        store,
        task_id,
        version,
        events,
        TaskStore::supervisor_authority(),
    );
}

fn append_permission(store: &TaskStore, task_id: TaskId, version: u64, events: Vec<NewTaskEvent>) {
    append_with_authority(
        store,
        task_id,
        version,
        events,
        TaskStore::permission_broker_authority(),
    );
}

fn append_with_authority(
    store: &TaskStore,
    task_id: TaskId,
    version: u64,
    events: Vec<NewTaskEvent>,
    authority: harness_journal::EventAuthority,
) {
    let previous_segment_id = store
        .task_projection(task_id)
        .unwrap()
        .and_then(|projection| projection.current_run)
        .map(|run| run.segment_id);
    let command = AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("test-{}", CommandId::new()),
        expected_stream_version: version,
        authority,
        payload: json!({ "type": "test_setup" }),
    };
    assert!(matches!(
        store.transact_command(command, |_| Ok(events)).unwrap(),
        CommandOutcome::Accepted { .. }
    ));
    let current_segment_id = store
        .task_projection(task_id)
        .unwrap()
        .and_then(|projection| projection.current_run)
        .map(|run| run.segment_id);
    if current_segment_id != previous_segment_id {
        if let Some(segment_id) = current_segment_id.filter(|segment_id| {
            store
                .pending_segment_start(task_id, *segment_id)
                .unwrap()
                .is_some()
        }) {
            store
                .mark_segment_start_delivered(task_id, segment_id)
                .unwrap();
        }
    }
}

fn test_store() -> (Arc<TaskStore>, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    (store, root)
}

fn task_created_in_test_workspace(
    root: &std::path::Path,
    task_id: TaskId,
    title: &str,
) -> NewTaskEvent {
    let workspace_root = root.join("workspaces").join(task_id.to_string());
    std::fs::create_dir_all(&workspace_root).unwrap();
    NewTaskEvent::task_created_in_workspace(
        title,
        harness_contracts::WorkspaceSelection {
            mode: WorkspaceMode::Current,
            root: workspace_root.to_string_lossy().into_owned(),
        },
    )
}

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    ActorId, BlobId, BlobRef, ContentHash, CorrelationId, EventSourceKind, JournalOffset, Message,
    MessageId, MessagePart, MessageRole, RunId, RunSegmentId, SessionId, SubagentActorState,
    SubagentContextReport, SubagentStatus, TaskId, TaskState, TenantId, TranscriptRef, TurnInput,
    UnexpectedErrorEvent, UsageSnapshot, WorkspaceMode,
};
use harness_daemon::{
    SubagentParentBinding, SubagentStopMode, SubagentSupervisor, WorkspaceAccess,
    WorkspaceCoordinator, WorkspaceExecutionKind, WorkspaceLeaseRequest,
    WorkspaceSubagentRunContext, WorkspaceSubagentRunnerFactory,
};
use harness_journal::{
    AcceptedCommand, CommandOutcome, EventStore, NewTaskEvent, SubagentLifecycleAuthority,
    SubagentLifecycleCommand, SubagentLifecycleTransition, TaskStore, TaskWorkspaceLeaseState,
};
use harness_subagent::{
    ChildRunOutcome, ChildRunRequest, DefaultSubagentRunner, DelegationPolicy, ParentContext,
    SubagentAnnouncement, SubagentEngineFactory, SubagentError, SubagentHandle, SubagentRunner,
    SubagentSpec,
};
use tokio::sync::{Mutex, Notify, Semaphore};

#[path = "subagent_actor/subagent_actor_cases.rs"]
mod subagent_actor_cases;

struct Fixture {
    _root: tempfile::TempDir,
    workspace_root: std::path::PathBuf,
    store: Arc<TaskStore>,
    workspace: Arc<WorkspaceCoordinator>,
    delegate: Arc<ControlledRunner>,
    subagents: Arc<SubagentSupervisor>,
}

impl Fixture {
    fn new(max_depth: u8, max_global: usize) -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace_root = root.path().join("workspace");
        std::fs::create_dir(&workspace_root).unwrap();
        init_git_repo(&workspace_root);
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        let workspace = Arc::new(
            WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
                .unwrap(),
        );
        let delegate = Arc::new(ControlledRunner::default());
        let runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory> =
            Arc::new(RecordingRunnerFactory {
                delegate: Arc::clone(&delegate),
            });
        let subagents = Arc::new(SubagentSupervisor::new(
            Arc::clone(&store),
            Arc::clone(&workspace),
            runner_factory,
            Arc::new(TokenRedactor),
            max_depth,
            max_global,
        ));
        Self {
            _root: root,
            workspace_root,
            store,
            workspace,
            delegate,
            subagents,
        }
    }

    fn fail_running_lifecycle_writes(&self) {
        let connection = rusqlite::Connection::open(self.store.database_path()).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER fail_running_subagent_lifecycle
                 BEFORE INSERT ON event_log
                 WHEN NEW.event_type = 'subagent.state_changed'
                  AND json_extract(NEW.payload_json, '$.state') = 'running'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected running lifecycle failure');
                 END;",
            )
            .unwrap();
    }

    fn fail_terminal_lifecycle_writes(&self) {
        let connection = rusqlite::Connection::open(self.store.database_path()).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER fail_terminal_subagent_lifecycle
                 BEFORE INSERT ON event_log
                 WHEN NEW.event_type = 'subagent.terminal'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected terminal lifecycle failure');
                 END;",
            )
            .unwrap();
    }
}

struct ControlledRunner {
    started: Semaphore,
    completed: Semaphore,
    summary: Mutex<String>,
    panic: Mutex<bool>,
    changed: Notify,
    workspace: Mutex<Option<std::path::PathBuf>>,
    yield_requested: Semaphore,
    return_artifacts: Mutex<bool>,
    status: Mutex<SubagentStatus>,
    failure: Mutex<Option<SubagentError>>,
}

impl Default for ControlledRunner {
    fn default() -> Self {
        Self {
            started: Semaphore::new(0),
            completed: Semaphore::new(0),
            summary: Mutex::new(String::new()),
            panic: Mutex::new(false),
            changed: Notify::new(),
            workspace: Mutex::new(None),
            yield_requested: Semaphore::new(0),
            return_artifacts: Mutex::new(false),
            status: Mutex::new(SubagentStatus::Completed),
            failure: Mutex::new(None),
        }
    }
}

impl ControlledRunner {
    async fn wait_started(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.started.acquire())
            .await
            .expect("child delegate did not start")
            .unwrap()
            .forget();
    }

    fn complete_one(&self) {
        self.completed.add_permits(1);
        self.changed.notify_waiters();
    }

    async fn set_summary(&self, summary: &str) {
        *self.summary.lock().await = summary.to_owned();
    }

    async fn panic_next(&self) {
        *self.panic.lock().await = true;
    }

    async fn workspace(&self) -> Option<std::path::PathBuf> {
        self.workspace.lock().await.clone()
    }

    async fn wait_yield_requested(&self) {
        self.yield_requested.acquire().await.unwrap().forget();
    }

    async fn return_child_artifacts(&self) {
        *self.return_artifacts.lock().await = true;
    }

    async fn set_status(&self, status: SubagentStatus) {
        *self.status.lock().await = status;
    }

    async fn fail_with(&self, error: SubagentError) {
        self.failure.lock().await.replace(error);
    }

    async fn run(
        &self,
        parent_ctx: ParentContext,
        control: Option<harness_subagent::SubagentCancellationToken>,
    ) -> Result<SubagentHandle, SubagentError> {
        self.started.add_permits(1);
        if std::mem::take(&mut *self.panic.lock().await) {
            panic!("child actor panic");
        }
        if let Some(control) = control {
            tokio::select! {
                permit = self.completed.acquire() => permit.unwrap().forget(),
                () = control.yield_requested() => {
                    self.yield_requested.add_permits(1);
                }
                () = control.cancelled() => return Err(SubagentError::Cancelled),
            }
        } else {
            self.completed.acquire().await.unwrap().forget();
        }
        if let Some(error) = self.failure.lock().await.take() {
            return Err(error);
        }
        let return_artifacts = *self.return_artifacts.lock().await;
        Ok(SubagentHandle::ready(SubagentAnnouncement {
            subagent_id: harness_contracts::SubagentId::new(),
            parent_session_id: parent_ctx.parent_session_id,
            status: self.status.lock().await.clone(),
            summary: self.summary.lock().await.clone(),
            result: return_artifacts.then(|| serde_json::json!({ "secret": "TOKEN result" })),
            usage: UsageSnapshot::default(),
            transcript_ref: return_artifacts.then(|| TranscriptRef {
                blob: BlobRef {
                    id: BlobId::new(),
                    size: 6,
                    content_hash: [7; 32],
                    content_type: Some("application/json".into()),
                },
                from_offset: JournalOffset(1),
                to_offset: JournalOffset(2),
            }),
            context_report: return_artifacts.then(|| SubagentContextReport {
                parent_system_hash: Some(ContentHash([1; 32])),
                child_system_hash: ContentHash([2; 32]),
                shared_system_prefix_hash: None,
                prompt_cache_prefix_reused: false,
                bootstrap_files_inherited: vec!["TOKEN/AGENTS.md".into()],
                system_header_extra_applied: false,
            }),
        }))
    }
}

struct RecordingRunnerFactory {
    delegate: Arc<ControlledRunner>,
}

struct NestedDelegatingRunnerFactory {
    contexts: Arc<std::sync::Mutex<Vec<(TaskId, std::path::PathBuf)>>>,
}

impl WorkspaceSubagentRunnerFactory for NestedDelegatingRunnerFactory {
    fn create(
        &self,
        context: WorkspaceSubagentRunContext,
    ) -> Result<Arc<dyn SubagentRunner>, SubagentError> {
        let mut contexts = self.contexts.lock().unwrap();
        contexts.push((context.child_task_id, context.workspace_root.clone()));
        let delegates = contexts.len() == 1;
        drop(contexts);
        Ok(Arc::new(NestedDelegatingRunner {
            nested: context.subagent_runner,
            delegates,
        }))
    }
}

struct NestedDelegatingRunner {
    nested: Arc<dyn SubagentRunner>,
    delegates: bool,
}

#[async_trait]
impl SubagentRunner for NestedDelegatingRunner {
    async fn spawn(
        &self,
        _spec: SubagentSpec,
        _input: TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, SubagentError> {
        if self.delegates {
            self.nested
                .spawn(
                    SubagentSpec::minimal("grandchild", "nested work"),
                    input("nested work"),
                    parent_context(parent_ctx.depth.saturating_add(1)),
                )
                .await?
                .wait()
                .await?;
        }
        Ok(ready_subagent_handle(parent_ctx, "nested complete"))
    }
}

struct BlockingRunnerFactory {
    entered: Arc<std::sync::Barrier>,
    release: Arc<std::sync::Barrier>,
}

struct PanickingRunnerFactory;

impl WorkspaceSubagentRunnerFactory for PanickingRunnerFactory {
    fn create(
        &self,
        _context: WorkspaceSubagentRunContext,
    ) -> Result<Arc<dyn SubagentRunner>, SubagentError> {
        panic!("injected runner factory panic")
    }
}

impl WorkspaceSubagentRunnerFactory for BlockingRunnerFactory {
    fn create(
        &self,
        _context: WorkspaceSubagentRunContext,
    ) -> Result<Arc<dyn SubagentRunner>, SubagentError> {
        self.entered.wait();
        self.release.wait();
        Ok(Arc::new(InstantRunner))
    }
}

struct InstantRunner;

#[async_trait]
impl SubagentRunner for InstantRunner {
    async fn spawn(
        &self,
        _spec: SubagentSpec,
        _input: TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, SubagentError> {
        Ok(ready_subagent_handle(parent_ctx, "complete"))
    }
}

fn ready_subagent_handle(parent_ctx: ParentContext, summary: &str) -> SubagentHandle {
    SubagentHandle::ready(SubagentAnnouncement {
        subagent_id: harness_contracts::SubagentId::new(),
        parent_session_id: parent_ctx.parent_session_id,
        status: SubagentStatus::Completed,
        summary: summary.to_owned(),
        result: None,
        usage: UsageSnapshot::default(),
        transcript_ref: None,
        context_report: None,
    })
}

struct DaemonDefaultRunnerFactory {
    engine: Arc<DaemonRecordingEngineFactory>,
}

impl WorkspaceSubagentRunnerFactory for DaemonDefaultRunnerFactory {
    fn create(
        &self,
        context: WorkspaceSubagentRunContext,
    ) -> Result<Arc<dyn SubagentRunner>, SubagentError> {
        self.engine
            .expected_session
            .lock()
            .unwrap()
            .replace(context.session_id);
        Ok(Arc::new(
            DefaultSubagentRunner::new_with_engine_factory(
                self.engine.clone(),
                context.event_store,
                context.workspace_root,
                DelegationPolicy::default(),
            )
            .with_child_session_id(context.session_id)
            .with_external_lifecycle_owner(),
        ))
    }
}

#[derive(Default)]
struct DaemonRecordingEngineFactory {
    expected_session: std::sync::Mutex<Option<SessionId>>,
    request: Mutex<Option<ChildRunRequest>>,
}

#[async_trait]
impl SubagentEngineFactory for DaemonRecordingEngineFactory {
    async fn run_child_engine(
        &self,
        request: ChildRunRequest,
    ) -> Result<ChildRunOutcome, SubagentError> {
        assert_eq!(
            request.child_session_id,
            self.expected_session.lock().unwrap().unwrap()
        );
        self.request.lock().await.replace(request);
        Ok(ChildRunOutcome {
            status: SubagentStatus::Completed,
            summary: "default runner completed".to_owned(),
            result: None,
            usage: UsageSnapshot::default(),
            transcript_ref: None,
            context_report: None,
        })
    }
}

impl WorkspaceSubagentRunnerFactory for RecordingRunnerFactory {
    fn create(
        &self,
        context: WorkspaceSubagentRunContext,
    ) -> Result<Arc<dyn SubagentRunner>, SubagentError> {
        Ok(Arc::new(WorkspaceRecordingRunner {
            delegate: Arc::clone(&self.delegate),
            workspace_root: context.workspace_root,
            event_store: context.event_store,
            tenant_id: context.tenant_id,
            session_id: context.session_id,
        }))
    }
}

struct WorkspaceRecordingRunner {
    delegate: Arc<ControlledRunner>,
    workspace_root: std::path::PathBuf,
    event_store: Arc<dyn EventStore>,
    tenant_id: TenantId,
    session_id: harness_contracts::SessionId,
}

#[async_trait]
impl SubagentRunner for WorkspaceRecordingRunner {
    async fn spawn(
        &self,
        spec: SubagentSpec,
        input: TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, SubagentError> {
        *self.delegate.workspace.lock().await = Some(self.workspace_root.clone());
        self.delegate.spawn(spec, input, parent_ctx).await
    }

    async fn spawn_controlled(
        &self,
        spec: SubagentSpec,
        input: TurnInput,
        parent_ctx: ParentContext,
        control: harness_subagent::SubagentCancellationToken,
    ) -> Result<SubagentHandle, SubagentError> {
        *self.delegate.workspace.lock().await = Some(self.workspace_root.clone());
        self.event_store
            .append(
                self.tenant_id,
                self.session_id,
                &[harness_contracts::Event::UnexpectedError(
                    UnexpectedErrorEvent {
                        session_id: Some(self.session_id),
                        run_id: None,
                        error: "child engine marker".into(),
                        at: harness_contracts::now(),
                    },
                )],
            )
            .await
            .map_err(|error| SubagentError::Engine(error.to_string()))?;
        self.delegate
            .spawn_controlled(spec, input, parent_ctx, control)
            .await
    }
}

#[async_trait]
impl SubagentRunner for ControlledRunner {
    async fn spawn(
        &self,
        _spec: SubagentSpec,
        _input: TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, SubagentError> {
        self.run(parent_ctx, None).await
    }

    async fn spawn_controlled(
        &self,
        _spec: SubagentSpec,
        _input: TurnInput,
        parent_ctx: ParentContext,
        control: harness_subagent::SubagentCancellationToken,
    ) -> Result<SubagentHandle, SubagentError> {
        self.run(parent_ctx, Some(control)).await
    }
}

struct TokenRedactor;

impl harness_contracts::Redactor for TokenRedactor {
    fn redact(&self, input: &str, _rules: &harness_contracts::RedactRules) -> String {
        input.replace("TOKEN", "[REDACTED]")
    }
}

fn acquire_parent_workspace(fixture: &Fixture, task_id: TaskId, actor_id: ActorId) {
    acquire_parent_workspace_at(
        &fixture.store,
        &fixture.workspace,
        &fixture.workspace_root,
        task_id,
        actor_id,
    );
}

fn acquire_parent_workspace_at(
    _store: &TaskStore,
    workspace: &WorkspaceCoordinator,
    workspace_root: &Path,
    task_id: TaskId,
    actor_id: ActorId,
) {
    let lease = workspace
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
        lease,
        harness_daemon::WorkspaceAcquireOutcome::Acquired(_)
    ));
}

fn child_state(
    store: &TaskStore,
    parent_task_id: TaskId,
    child_task_id: TaskId,
) -> harness_contracts::SubagentActorState {
    store
        .task_projection(parent_task_id)
        .unwrap()
        .unwrap()
        .subagents
        .into_iter()
        .find(|child| child.child_task_id == child_task_id)
        .unwrap()
        .state
}

fn create_running_parent(store: &TaskStore, title: &str) -> (TaskId, ActorId, RunSegmentId) {
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let outcome = store
        .transact_command(
            AcceptedCommand {
                command_id: harness_contracts::CommandId::new(),
                task_id,
                idempotency_key: format!("create-{task_id}"),
                expected_stream_version: 0,
                authority: TaskStore::supervisor_authority(),
                payload: serde_json::json!({ "type": "create_task", "title": title }),
            },
            |_| {
                Ok(vec![
                    NewTaskEvent::task_created(title),
                    NewTaskEvent::run_started(segment_id, harness_contracts::now()),
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

fn parent_context(depth: u8) -> ParentContext {
    ParentContext {
        tenant_id: TenantId::SINGLE,
        parent_session_id: harness_contracts::SessionId::new(),
        parent_run_id: RunId::new(),
        depth,
        sibling_count: 0,
        trigger_tool_use_id: None,
        correlation_id: CorrelationId::new(),
        team_id: None,
        team_member_profile_id: None,
    }
}

fn input(text: &str) -> TurnInput {
    TurnInput {
        message: Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(text.to_owned())],
            created_at: harness_contracts::now(),
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

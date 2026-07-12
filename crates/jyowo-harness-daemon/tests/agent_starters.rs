use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::{
    AgentTeamRunConfig, AgentTeamSharedMemoryPolicy, AgentTeamStarterCap,
    AgentTeamToolSessionSnapshot, AgentTeamToolStartRequest, AgentTeamTopology, AgentToolPolicy,
    AgentUsePolicy, AgentWorkspaceIsolationMode, BackgroundAgentStarterCap,
    BackgroundAgentToolSessionSnapshot, BackgroundAgentToolStartRequest, Event, InteractivityLevel,
    PermissionMode, RunId, RunSegmentId, SessionId, SubagentActorState, TaskId, TenantId,
    ToolProfile, ToolSearchMode, ToolUseId, TurnInput, WorkspaceMode,
};
use harness_daemon::{
    AgentStarterCapabilities, DaemonAgentStarter, RunCoordinatorFactory, RunningSegment,
    StartSegmentRequest, SubagentParentBinding, SubagentSupervisor, Supervisor, SupervisorQuotas,
    WorkspaceAccess, WorkspaceCoordinator, WorkspaceExecutionKind, WorkspaceLeaseRequest,
    WorkspaceSubagentRunContext, WorkspaceSubagentRunnerFactory, WorkspaceToolDispatcher,
};
use harness_journal::{
    AcceptedCommand, CommandOutcome, EventStore, NewTaskEvent, ReplayCursor, TaskEventStoreAdapter,
    TaskStore,
};
use harness_subagent::{
    ParentContext, SubagentAnnouncement, SubagentError, SubagentHandle, SubagentRunner,
    SubagentSpec,
};
use tokio::sync::Semaphore;

struct BlockingRunner {
    started: Semaphore,
    release: Semaphore,
}

struct IdleFactory;

impl RunCoordinatorFactory for IdleFactory {
    fn spawn_idempotent(
        &self,
        _request: StartSegmentRequest,
        _workspace_tools: WorkspaceToolDispatcher,
        _subagent_runner: Arc<dyn SubagentRunner>,
        _agent_starters: AgentStarterCapabilities,
    ) -> RunningSegment {
        let (_sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        RunningSegment::new(receiver)
    }
}

impl BlockingRunner {
    fn new() -> Self {
        Self {
            started: Semaphore::new(0),
            release: Semaphore::new(0),
        }
    }

    async fn wait_started(&self, count: u32) {
        for _ in 0..count {
            self.started.acquire().await.unwrap().forget();
        }
    }
}

#[async_trait]
impl SubagentRunner for BlockingRunner {
    async fn spawn(
        &self,
        spec: SubagentSpec,
        _input: TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, SubagentError> {
        self.started.add_permits(1);
        self.release.acquire().await.unwrap().forget();
        Ok(SubagentHandle::ready(SubagentAnnouncement {
            subagent_id: harness_contracts::SubagentId::new(),
            parent_session_id: parent_ctx.parent_session_id,
            status: harness_contracts::SubagentStatus::Completed,
            summary: spec.task,
            result: None,
            usage: harness_contracts::UsageSnapshot::default(),
            transcript_ref: None,
            context_report: None,
        }))
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    store: Arc<TaskStore>,
    runner: Arc<BlockingRunner>,
    starter: DaemonAgentStarter,
    parent_task_id: TaskId,
    session_id: SessionId,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace_root = root.path().join("workspace");
        std::fs::create_dir(&workspace_root).unwrap();
        init_git_repo(&workspace_root);
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        let workspace = Arc::new(
            WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
                .unwrap(),
        );
        let runner = Arc::new(BlockingRunner::new());
        let runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory> = {
            let runner = Arc::clone(&runner);
            Arc::new(move |_context: WorkspaceSubagentRunContext| {
                Ok(Arc::clone(&runner) as Arc<dyn SubagentRunner>)
            })
        };
        let supervisor = Arc::new(SubagentSupervisor::new(
            Arc::clone(&store),
            Arc::clone(&workspace),
            runner_factory,
            Arc::new(harness_contracts::NoopRedactor),
            4,
            8,
        ));
        let (parent_task_id, parent_actor_id, parent_segment_id) =
            create_running_parent(&store, "parent");
        workspace
            .acquire(WorkspaceLeaseRequest {
                task_id: parent_task_id,
                actor_id: parent_actor_id,
                root: workspace_root,
                mode: Some(WorkspaceMode::Current),
                access: WorkspaceAccess::Write,
                execution_kind: WorkspaceExecutionKind::Foreground,
                expires_at: None,
            })
            .unwrap();
        let session_id = SessionId::new();
        let starter = DaemonAgentStarter::new(
            Arc::clone(&store),
            supervisor,
            SubagentParentBinding {
                parent_task_id,
                parent_segment_id,
                parent_actor_id,
                depth: 0,
            },
            root.path().join("blobs"),
        );
        Self {
            _root: root,
            store,
            runner,
            starter,
            parent_task_id,
            session_id,
        }
    }
}

#[tokio::test]
async fn background_starter_returns_the_durable_detached_child_task_id() {
    let fixture = Fixture::new();
    let request = background_request(fixture.session_id);
    let response = fixture
        .starter
        .start_background_agent(request.clone())
        .await
        .expect("background child should start");
    let child_task_id = response
        .background_agent_id
        .parse::<TaskId>()
        .expect("background id should be a task id");
    let child = fixture
        .store
        .task_projection(fixture.parent_task_id)
        .unwrap()
        .unwrap()
        .subagents
        .into_iter()
        .find(|child| child.child_task_id == child_task_id)
        .expect("durable child projection");

    assert_eq!(child.state, SubagentActorState::Background);
    assert!(child.detached);
    assert_eq!(response.status, "background");
    fixture.runner.wait_started(1).await;
    fixture.runner.release.add_permits(1);
}

#[tokio::test]
async fn team_starter_validates_before_side_effects_and_enforces_one_active_team() {
    let fixture = Fixture::new();
    let mut invalid = team_request(fixture.session_id);
    invalid
        .agent_tool_policy
        .team_config
        .as_mut()
        .unwrap()
        .lead_profile_id = "missing".to_owned();
    assert!(fixture.starter.start_agent_team(invalid).await.is_err());
    assert!(fixture
        .store
        .task_projection(fixture.parent_task_id)
        .unwrap()
        .unwrap()
        .subagents
        .is_empty());

    let request = team_request(fixture.session_id);
    let response = fixture
        .starter
        .start_agent_team(request.clone())
        .await
        .expect("team should start");
    let parent = fixture
        .store
        .task_projection(fixture.parent_task_id)
        .unwrap()
        .unwrap();
    assert_eq!(parent.subagents.len(), 2);
    assert!(parent
        .subagents
        .iter()
        .all(|child| child.state == SubagentActorState::Background && child.detached));

    let event_store = TaskEventStoreAdapter::new(
        Arc::clone(&fixture.store),
        fixture.parent_task_id,
        TenantId::SINGLE,
        fixture.session_id,
        Arc::new(harness_contracts::NoopRedactor),
    );
    let events = event_store
        .read_envelopes(
            TenantId::SINGLE,
            fixture.session_id,
            ReplayCursor::FromStart,
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(&event.payload, Event::TeamCreated(created) if created.team_id == response.team_id))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(&event.payload, Event::TeamMemberJoined(joined) if joined.team_id == response.team_id))
            .count(),
        2
    );

    assert!(fixture.starter.start_agent_team(request).await.is_err());
    assert_eq!(
        fixture
            .store
            .task_projection(fixture.parent_task_id)
            .unwrap()
            .unwrap()
            .subagents
            .len(),
        2
    );
    fixture.runner.wait_started(2).await;
    fixture.runner.release.add_permits(2);
}

#[tokio::test]
async fn daemon_restart_recovers_background_and_team_child_projections() {
    let background = Fixture::new();
    let response = background
        .starter
        .start_background_agent(background_request(background.session_id))
        .await
        .expect("background child should start");
    background.runner.wait_started(1).await;
    let child_task_id = response.background_agent_id.parse::<TaskId>().unwrap();
    let _restarted = Supervisor::start(
        Arc::clone(&background.store),
        Arc::new(IdleFactory),
        SupervisorQuotas::new(1, 8),
    )
    .expect("daemon restart should recover detached children");
    let child = background
        .store
        .task_projection(background.parent_task_id)
        .unwrap()
        .unwrap()
        .subagents
        .into_iter()
        .find(|child| child.child_task_id == child_task_id)
        .unwrap();
    assert_eq!(child.state, SubagentActorState::Failed);
    assert!(child.detached);
    background.runner.release.add_permits(1);

    let team = Fixture::new();
    let response = team
        .starter
        .start_agent_team(team_request(team.session_id))
        .await
        .expect("team should start");
    team.runner.wait_started(2).await;
    let _restarted = Supervisor::start(
        Arc::clone(&team.store),
        Arc::new(IdleFactory),
        SupervisorQuotas::new(1, 8),
    )
    .expect("daemon restart should recover team members");
    let parent = team
        .store
        .task_projection(team.parent_task_id)
        .unwrap()
        .unwrap();
    assert_eq!(parent.subagents.len(), 2);
    assert!(parent
        .subagents
        .iter()
        .all(|child| child.state == SubagentActorState::Failed && child.detached));

    let event_store = TaskEventStoreAdapter::new(
        Arc::clone(&team.store),
        team.parent_task_id,
        TenantId::SINGLE,
        team.session_id,
        Arc::new(harness_contracts::NoopRedactor),
    );
    let events = event_store
        .read_envelopes(TenantId::SINGLE, team.session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    assert!(events.iter().any(
        |event| matches!(&event.payload, Event::TeamCreated(created) if created.team_id == response.team_id)
    ));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(&event.payload, Event::TeamMemberJoined(joined) if joined.team_id == response.team_id))
            .count(),
        2
    );
    team.runner.release.add_permits(2);
}

fn background_request(session_id: SessionId) -> BackgroundAgentToolStartRequest {
    BackgroundAgentToolStartRequest {
        tenant_id: TenantId::SINGLE,
        conversation_id: session_id,
        parent_run_id: RunId::new(),
        tool_use_id: ToolUseId::new(),
        goal: "inspect in background".to_owned(),
        title: "background review".to_owned(),
        model_config_id: None,
        permission_mode: PermissionMode::Default,
        agent_tool_policy: agent_policy(false),
        session: BackgroundAgentToolSessionSnapshot {
            tenant_id: TenantId::SINGLE,
            session_id,
            tool_search: ToolSearchMode::Disabled,
            tool_profile: ToolProfile::Full,
            permission_mode: PermissionMode::Default,
            interactivity: InteractivityLevel::FullyInteractive,
            team_id: None,
            max_iterations: 16,
            context_compression_trigger_ratio: 0.8,
        },
    }
}

fn team_request(session_id: SessionId) -> AgentTeamToolStartRequest {
    AgentTeamToolStartRequest {
        tenant_id: TenantId::SINGLE,
        conversation_id: session_id,
        parent_run_id: RunId::new(),
        tool_use_id: ToolUseId::new(),
        goal: "review as a team".to_owned(),
        topology: AgentTeamTopology::CoordinatorWorker,
        max_turns_per_goal: 3,
        agent_tool_policy: agent_policy(true),
        session: AgentTeamToolSessionSnapshot {
            tenant_id: TenantId::SINGLE,
            session_id,
            tool_search: ToolSearchMode::Disabled,
            tool_profile: ToolProfile::Full,
            permission_mode: PermissionMode::Default,
            interactivity: InteractivityLevel::FullyInteractive,
            team_id: None,
            max_iterations: 16,
            context_compression_trigger_ratio: 0.8,
        },
    }
}

fn agent_policy(team: bool) -> AgentToolPolicy {
    AgentToolPolicy {
        subagents: AgentUsePolicy::Allowed,
        agent_team: if team {
            AgentUsePolicy::Allowed
        } else {
            AgentUsePolicy::Off
        },
        background_agents: if team {
            AgentUsePolicy::Off
        } else {
            AgentUsePolicy::Allowed
        },
        team_config: team.then(|| AgentTeamRunConfig {
            topology: AgentTeamTopology::CoordinatorWorker,
            lead_profile_id: "reviewer".to_owned(),
            member_profile_ids: vec!["worker".to_owned()],
            max_turns_per_goal: 3,
            shared_memory_policy: AgentTeamSharedMemoryPolicy::SummariesOnly,
        }),
        workspace_isolation: AgentWorkspaceIsolationMode::GitWorktree,
        max_depth: 4,
        max_concurrent_subagents: 8,
        max_team_members: 2,
    }
}

fn create_running_parent(
    store: &TaskStore,
    title: &str,
) -> (TaskId, harness_contracts::ActorId, RunSegmentId) {
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

fn init_git_repo(path: &Path) {
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test User"],
    ] {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .status()
            .unwrap()
            .success());
    }
    std::fs::write(path.join("README.md"), "baseline\n").unwrap();
    for args in [
        vec!["add", "README.md"],
        vec!["commit", "-q", "-m", "initial"],
    ] {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .status()
            .unwrap()
            .success());
    }
}

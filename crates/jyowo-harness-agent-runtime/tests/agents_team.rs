#![cfg(feature = "agents-team")]

use harness_agent_runtime::{
    build_team_spec, mark_team_task_active, persist_team_before_dispatch, prepare_run_scoped_team,
    AgentRuntimeStore, RunScopedTeamCoordinator, RunScopedTeamCoordinatorRequest,
    RunScopedTeamCreateRequest, RunScopedTeamHost, TeamRuntimeError,
};
use harness_contracts::{
    AgentProfile, AgentProfileContextMode, AgentProfileMemoryScope, AgentProfileSandboxInheritance,
    AgentProfileScope, AgentTeamRunConfig, AgentTeamSharedMemoryPolicy, AgentTeamTopology,
    AgentToolPolicy, AgentUsePolicy, AgentWorkspaceIsolationMode, SessionId, TeamId,
};
use harness_team::{SharedMemorySpec, SharedWritePolicy, Topology};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

fn profile(id: &str, role: &str) -> AgentProfile {
    AgentProfile {
        id: id.to_owned(),
        scope: AgentProfileScope::User,
        role: role.to_owned(),
        description: format!("{role} profile"),
        model_config_override: None,
        tool_allowlist: None,
        tool_blocklist: vec![],
        sandbox_inheritance: AgentProfileSandboxInheritance::InheritParent,
        memory_scope: AgentProfileMemoryScope::ReadOnly,
        context_mode: AgentProfileContextMode::Focused,
        max_turns: 6,
        max_depth: 1,
        default_workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
    }
}

fn profiles() -> Vec<AgentProfile> {
    vec![
        profile("lead", "lead"),
        profile("worker", "worker"),
        profile("reviewer", "reviewer"),
    ]
}

fn options(topology: AgentTeamTopology) -> AgentToolPolicy {
    AgentToolPolicy {
        subagents: AgentUsePolicy::Allowed,
        agent_team: AgentUsePolicy::Allowed,
        team_config: Some(AgentTeamRunConfig {
            topology,
            lead_profile_id: "lead".to_owned(),
            member_profile_ids: vec!["worker".to_owned(), "reviewer".to_owned()],
            max_turns_per_goal: 3,
            shared_memory_policy: AgentTeamSharedMemoryPolicy::SummariesOnly,
        }),
        background_agents: AgentUsePolicy::Off,
        workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
        max_depth: 2,
        max_concurrent_subagents: 2,
        max_team_members: 3,
    }
}

#[test]
fn agents_team_builds_supported_topology_specs() {
    for (topology, expected) in [
        (
            AgentTeamTopology::CoordinatorWorker,
            Topology::CoordinatorWorker,
        ),
        (AgentTeamTopology::PeerToPeer, Topology::PeerToPeer),
        (AgentTeamTopology::RoleRouted, Topology::RoleRouted),
    ] {
        let prepared = prepare_run_scoped_team(
            &options(topology),
            &profiles(),
            "run-1",
            "conversation-1",
            "inspect the repository",
        )
        .expect("team should prepare");

        let spec = build_team_spec(&prepared);

        assert_eq!(spec.topology, expected);
        assert_eq!(spec.team_id, prepared.team_id);
        assert_eq!(spec.max_turns_per_goal, 3);
        assert!(spec.single_process_only);
        assert_eq!(spec.members.len(), 3);
    }
}

#[test]
fn agents_team_builds_topology_specific_routing() {
    let coordinator = prepare_run_scoped_team(
        &options(AgentTeamTopology::CoordinatorWorker),
        &profiles(),
        "run-1",
        "conversation-1",
        "inspect",
    )
    .expect("coordinator team");
    let coordinator_spec = build_team_spec(&coordinator);
    assert_eq!(
        coordinator_spec.topology_config.coordinator,
        Some(coordinator.lead.agent_id)
    );
    assert_eq!(coordinator_spec.topology_config.workers.len(), 2);

    let peer = prepare_run_scoped_team(
        &options(AgentTeamTopology::PeerToPeer),
        &profiles(),
        "run-2",
        "conversation-1",
        "inspect",
    )
    .expect("peer team");
    let peer_spec = build_team_spec(&peer);
    assert_eq!(peer_spec.topology_config.coordinator, None);
    assert_eq!(peer_spec.topology_config.workers.len(), 3);

    let role = prepare_run_scoped_team(
        &options(AgentTeamTopology::RoleRouted),
        &profiles(),
        "run-3",
        "conversation-1",
        "inspect",
    )
    .expect("role team");
    let role_spec = build_team_spec(&role);
    assert_eq!(
        role_spec.topology_config.coordinator,
        Some(role.lead.agent_id)
    );
    assert_eq!(role_spec.topology_config.role_routes.len(), 2);
    assert!(role_spec
        .topology_config
        .role_routes
        .iter()
        .any(|route| route.role == "worker"));
}

#[test]
fn agents_team_persists_task_and_mailbox_before_dispatch() {
    let workspace = tempdir().expect("tempdir");
    let store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let prepared = prepare_run_scoped_team(
        &options(AgentTeamTopology::CoordinatorWorker),
        &profiles(),
        "run-1",
        "conversation-1",
        "inspect the repository",
    )
    .expect("team should prepare");

    persist_team_before_dispatch(&store, &prepared).expect("team state persists");
    let queued_tasks = store
        .list_agent_team_tasks_for_team(&prepared.team_id.to_string())
        .expect("tasks load");
    let mailbox = store
        .list_agent_team_mailbox_for_team(&prepared.team_id.to_string())
        .expect("mailbox loads");

    assert_eq!(queued_tasks.len(), 1);
    assert_eq!(queued_tasks[0].status, "queued");
    assert_eq!(queued_tasks[0].assignee_profile_id.as_deref(), Some("lead"));
    assert_eq!(mailbox.len(), 1);
    assert_eq!(mailbox[0].summary, "Team run queued");

    mark_team_task_active(&store, &prepared).expect("task activates");
    let active_tasks = store
        .list_agent_team_tasks_for_team(&prepared.team_id.to_string())
        .expect("tasks reload");
    assert_eq!(active_tasks[0].status, "active");
}

#[tokio::test]
async fn agents_team_coordinator_owns_prepare_persist_build_start_register_dispatch_sequence() {
    struct RecordingHost {
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    #[async_trait::async_trait]
    impl RunScopedTeamHost for RecordingHost {
        type Team = TeamId;

        async fn create_run_scoped_team(
            &self,
            request: RunScopedTeamCreateRequest,
        ) -> Result<Self::Team, String> {
            self.calls.lock().unwrap().push("create");
            assert_eq!(request.spec.topology, Topology::CoordinatorWorker);
            assert_eq!(
                request
                    .member_profile_ids
                    .get(&request.spec.members[0].agent_id)
                    .map(String::as_str),
                Some("lead")
            );
            Ok(request.spec.team_id)
        }

        async fn register_active_run_team(
            &self,
            _run_id: harness_contracts::RunId,
            _team: Self::Team,
        ) -> Result<(), String> {
            self.calls.lock().unwrap().push("register");
            Ok(())
        }

        async fn emit_team_task_updated(
            &self,
            _session_id: SessionId,
            _prepared: &harness_agent_runtime::PreparedRunScopedTeam,
            status: &str,
        ) -> Result<(), String> {
            self.calls.lock().unwrap().push("emit");
            assert_eq!(status, "active");
            Ok(())
        }

        async fn dispatch_run_scoped_team_goal(
            &self,
            _run_id: harness_contracts::RunId,
            _team: Self::Team,
            prepared: harness_agent_runtime::PreparedRunScopedTeam,
            goal: String,
        ) -> Result<(), String> {
            self.calls.lock().unwrap().push("dispatch");
            assert_eq!(prepared.lead.profile_id, "lead");
            assert_eq!(goal, "inspect the repository");
            Ok(())
        }
    }

    let workspace = tempdir().expect("tempdir");
    let store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let run_id = harness_contracts::RunId::new();
    let session_id = SessionId::new();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let host = RecordingHost {
        calls: Arc::clone(&calls),
    };

    let team_id = RunScopedTeamCoordinator::new(&store)
        .start(
            &host,
            RunScopedTeamCoordinatorRequest {
                agent_tool_policy: options(AgentTeamTopology::CoordinatorWorker),
                profiles: profiles(),
                run_id,
                conversation_session_id: session_id,
                goal: "inspect the repository".to_owned(),
                workspace_root: workspace.path().to_path_buf(),
            },
        )
        .await
        .expect("team starts through coordinator");

    assert_eq!(
        &*calls.lock().unwrap(),
        &["create", "register", "emit", "dispatch"]
    );
    let tasks = store
        .list_agent_team_tasks_for_team(&team_id.to_string())
        .expect("tasks load");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].status, "active");
}

#[test]
fn agents_team_rejects_invalid_profiles_members_and_turns() {
    let mut missing_lead = options(AgentTeamTopology::CoordinatorWorker);
    missing_lead
        .team_config
        .as_mut()
        .expect("team config")
        .lead_profile_id = "missing".to_owned();
    assert!(matches!(
        prepare_run_scoped_team(&missing_lead, &profiles(), "run-1", "conversation-1", "goal"),
        Err(TeamRuntimeError::UnknownProfile(id)) if id == "missing"
    ));

    let mut missing_member = options(AgentTeamTopology::CoordinatorWorker);
    missing_member
        .team_config
        .as_mut()
        .expect("team config")
        .member_profile_ids
        .push("missing-member".to_owned());
    assert!(matches!(
        prepare_run_scoped_team(&missing_member, &profiles(), "run-1", "conversation-1", "goal"),
        Err(TeamRuntimeError::UnknownProfile(id)) if id == "missing-member"
    ));

    let mut empty_members = options(AgentTeamTopology::CoordinatorWorker);
    empty_members
        .team_config
        .as_mut()
        .expect("team config")
        .member_profile_ids
        .clear();
    let error = prepare_run_scoped_team(
        &empty_members,
        &profiles(),
        "run-1",
        "conversation-1",
        "goal",
    )
    .expect_err("empty members should fail");
    assert!(error.to_string().contains("memberProfileIds"));

    let mut too_many = options(AgentTeamTopology::CoordinatorWorker);
    too_many.max_team_members = 2;
    assert!(matches!(
        prepare_run_scoped_team(&too_many, &profiles(), "run-1", "conversation-1", "goal"),
        Err(TeamRuntimeError::TooManyMembers { actual: 3, max: 2 })
    ));

    let mut invalid_turns = options(AgentTeamTopology::CoordinatorWorker);
    invalid_turns
        .team_config
        .as_mut()
        .expect("team config")
        .max_turns_per_goal = 0;
    let error = prepare_run_scoped_team(
        &invalid_turns,
        &profiles(),
        "run-1",
        "conversation-1",
        "goal",
    )
    .expect_err("zero turns should fail");
    assert!(error.to_string().contains("maxTurnsPerGoal"));
}

#[test]
fn agents_team_redacted_mailbox_does_not_grant_role_writes_by_default() {
    let mut run_options = options(AgentTeamTopology::CoordinatorWorker);
    run_options
        .team_config
        .as_mut()
        .expect("team config")
        .shared_memory_policy = AgentTeamSharedMemoryPolicy::RedactedMailbox;
    let prepared = prepare_run_scoped_team(
        &run_options,
        &profiles(),
        "run-1",
        "conversation-1",
        "inspect",
    )
    .expect("team should prepare");

    let spec = build_team_spec(&prepared);

    assert!(matches!(
        spec.shared_memory,
        SharedMemorySpec::Enabled {
            write_policy: SharedWritePolicy::RoleGated { ref allowed_roles },
            ..
        } if allowed_roles.is_empty()
    ));
}

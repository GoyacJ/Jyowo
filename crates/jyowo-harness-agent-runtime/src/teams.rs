use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{
    AgentId, AgentProfile, AgentTeamSharedMemoryPolicy, AgentTeamTopology, AgentToolPolicy,
    AgentUsePolicy, RunId, SessionId, TeamId,
};
use harness_team::{
    ContextVisibility, RoleRoute, SharedMemorySpec, SharedWritePolicy, TeamBuilder,
    TeamMemberEngineConfig, TeamResourceQuota, TeamSandboxPolicy, TeamSpec, TeamToolsetSelector,
    Topology,
};
use thiserror::Error;

use crate::store::{
    AgentRuntimeStore, AgentRuntimeStoreError, AgentTeamMailboxRecord, AgentTeamTaskRecord,
};

#[derive(Debug, Error)]
pub enum TeamRuntimeError {
    #[error("agent runtime store error: {0}")]
    Store(#[from] AgentRuntimeStoreError),
    #[error("team runtime validation error: {0}")]
    Validation(String),
    #[error("unknown profile id: {0}")]
    UnknownProfile(String),
    #[error("team member count {actual} exceeds max_team_members {max}")]
    TooManyMembers { actual: usize, max: u32 },
    #[error("team host error: {0}")]
    Host(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedTeamMember {
    pub profile_id: String,
    pub profile: AgentProfile,
    pub agent_id: AgentId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRunScopedTeam {
    pub run_id: String,
    pub conversation_id: String,
    pub goal: String,
    pub team_name: String,
    pub topology: AgentTeamTopology,
    pub shared_memory_policy: AgentTeamSharedMemoryPolicy,
    pub max_turns_per_goal: u32,
    pub lead: PreparedTeamMember,
    pub members: Vec<PreparedTeamMember>,
    pub initial_task_id: String,
    pub team_id: TeamId,
}

#[derive(Debug, Clone)]
pub struct RunScopedTeamCoordinatorRequest {
    pub agent_tool_policy: AgentToolPolicy,
    pub profiles: Vec<AgentProfile>,
    pub run_id: RunId,
    pub conversation_session_id: SessionId,
    pub goal: String,
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RunScopedTeamCreateRequest {
    pub spec: TeamSpec,
    pub conversation_session_id: SessionId,
    pub run_id: RunId,
    pub agent_tool_policy: AgentToolPolicy,
    pub member_profile_ids: HashMap<AgentId, String>,
    pub workspace_root: PathBuf,
}

#[async_trait]
pub trait RunScopedTeamHost {
    type Team: Clone + Send + Sync + 'static;

    async fn create_run_scoped_team(
        &self,
        request: RunScopedTeamCreateRequest,
    ) -> Result<Self::Team, String>;

    async fn register_active_run_team(&self, run_id: RunId, team: Self::Team)
        -> Result<(), String>;

    async fn emit_team_task_updated(
        &self,
        session_id: SessionId,
        prepared: &PreparedRunScopedTeam,
        status: &str,
    ) -> Result<(), String>;

    async fn dispatch_run_scoped_team_goal(
        &self,
        run_id: RunId,
        team: Self::Team,
        prepared: PreparedRunScopedTeam,
        goal: String,
    ) -> Result<(), String>;
}

pub struct RunScopedTeamCoordinator<'store> {
    store: &'store AgentRuntimeStore,
}

impl<'store> RunScopedTeamCoordinator<'store> {
    #[must_use]
    pub fn new(store: &'store AgentRuntimeStore) -> Self {
        Self { store }
    }

    pub async fn start<H>(
        &self,
        host: &H,
        request: RunScopedTeamCoordinatorRequest,
    ) -> Result<H::Team, TeamRuntimeError>
    where
        H: RunScopedTeamHost + Sync,
    {
        let prepared = prepare_run_scoped_team(
            &request.agent_tool_policy,
            &request.profiles,
            &request.run_id.to_string(),
            &request.conversation_session_id.to_string(),
            &request.goal,
        )?;
        persist_team_before_dispatch(self.store, &prepared)?;
        let spec = build_team_spec(&prepared);
        let member_profile_ids = member_profile_id_map(&prepared);
        let team = host
            .create_run_scoped_team(RunScopedTeamCreateRequest {
                spec,
                conversation_session_id: request.conversation_session_id,
                run_id: request.run_id,
                agent_tool_policy: request.agent_tool_policy,
                member_profile_ids,
                workspace_root: request.workspace_root,
            })
            .await
            .map_err(TeamRuntimeError::Host)?;
        host.register_active_run_team(request.run_id, team.clone())
            .await
            .map_err(TeamRuntimeError::Host)?;
        mark_team_task_active(self.store, &prepared)?;
        host.emit_team_task_updated(request.conversation_session_id, &prepared, "active")
            .await
            .map_err(TeamRuntimeError::Host)?;
        host.dispatch_run_scoped_team_goal(request.run_id, team.clone(), prepared, request.goal)
            .await
            .map_err(TeamRuntimeError::Host)?;
        Ok(team)
    }
}

#[must_use]
pub fn should_start_run_scoped_team(options: &AgentToolPolicy) -> bool {
    options.agent_team == AgentUsePolicy::Allowed && options.team_config.is_some()
}

pub fn prepare_run_scoped_team(
    options: &AgentToolPolicy,
    profiles: &[AgentProfile],
    run_id: &str,
    conversation_id: &str,
    goal: &str,
) -> Result<PreparedRunScopedTeam, TeamRuntimeError> {
    let team_config = options
        .team_config
        .as_ref()
        .ok_or_else(|| TeamRuntimeError::Validation("teamConfig is required".to_owned()))?;

    if team_config.member_profile_ids.is_empty() {
        return Err(TeamRuntimeError::Validation(
            "teamConfig.memberProfileIds must not be empty".to_owned(),
        ));
    }

    let lead_profile = find_profile(&team_config.lead_profile_id, profiles)?;
    let members = team_config
        .member_profile_ids
        .iter()
        .map(|member_id| {
            let profile = find_profile(member_id, profiles)?;
            Ok(PreparedTeamMember {
                profile_id: member_id.clone(),
                profile: profile.clone(),
                agent_id: AgentId::new(),
            })
        })
        .collect::<Result<Vec<_>, TeamRuntimeError>>()?;

    let total_members = 1 + members.len();
    if total_members as u32 > options.max_team_members {
        return Err(TeamRuntimeError::TooManyMembers {
            actual: total_members,
            max: options.max_team_members,
        });
    }

    if team_config.max_turns_per_goal == 0 {
        return Err(TeamRuntimeError::Validation(
            "maxTurnsPerGoal must be greater than zero".to_owned(),
        ));
    }

    Ok(PreparedRunScopedTeam {
        run_id: run_id.to_owned(),
        conversation_id: conversation_id.to_owned(),
        goal: goal.to_owned(),
        team_name: format!("run-{run_id}"),
        topology: team_config.topology,
        shared_memory_policy: team_config.shared_memory_policy,
        max_turns_per_goal: team_config.max_turns_per_goal,
        lead: PreparedTeamMember {
            profile_id: team_config.lead_profile_id.clone(),
            profile: lead_profile.clone(),
            agent_id: AgentId::new(),
        },
        members,
        initial_task_id: RunId::new().to_string(),
        team_id: TeamId::new(),
    })
}

pub fn persist_team_before_dispatch(
    store: &AgentRuntimeStore,
    prepared: &PreparedRunScopedTeam,
) -> Result<(), TeamRuntimeError> {
    let now = Utc::now().to_rfc3339();
    let task = AgentTeamTaskRecord {
        task_id: prepared.initial_task_id.clone(),
        team_id: prepared.team_id.to_string(),
        run_id: prepared.run_id.clone(),
        title: truncate_goal(&prepared.goal),
        status: "queued".to_owned(),
        assignee_profile_id: Some(prepared.lead.profile_id.clone()),
        created_at: now.clone(),
        updated_at: now.clone(),
        payload_json: serde_json::json!({
            "conversationId": prepared.conversation_id,
            "goal": prepared.goal,
        })
        .to_string(),
    };
    store.insert_agent_team_task(&task)?;

    let mailbox = AgentTeamMailboxRecord {
        message_id: RunId::new().to_string(),
        team_id: prepared.team_id.to_string(),
        sender_profile_id: prepared.lead.profile_id.clone(),
        recipient_profile_id: None,
        created_at: now,
        summary: "Team run queued".to_owned(),
        payload_json: serde_json::json!({ "kind": "system", "status": "queued" }).to_string(),
    };
    store.insert_agent_team_mailbox_message(&mailbox)?;
    Ok(())
}

pub fn mark_team_task_active(
    store: &AgentRuntimeStore,
    prepared: &PreparedRunScopedTeam,
) -> Result<(), TeamRuntimeError> {
    let now = Utc::now().to_rfc3339();
    store.update_agent_team_task_status(&prepared.initial_task_id, "active", &now)?;
    Ok(())
}

#[must_use]
pub fn build_team_spec(prepared: &PreparedRunScopedTeam) -> TeamSpec {
    let topology = map_topology(prepared.topology);
    let mut builder = TeamBuilder::new(&prepared.team_name, topology);
    builder = builder.member_with_engine_config(
        prepared.lead.agent_id,
        &prepared.lead.profile.role,
        visibility_for_profile(&prepared.lead.profile),
        engine_config_from_profile(&prepared.lead.profile),
    );
    for member in &prepared.members {
        builder = builder.member_with_engine_config(
            member.agent_id,
            &member.profile.role,
            visibility_for_profile(&member.profile),
            engine_config_from_profile(&member.profile),
        );
    }

    let mut spec = builder.build();
    spec.team_id = prepared.team_id;
    spec.max_turns_per_goal = prepared.max_turns_per_goal;
    spec.shared_memory = map_shared_memory(prepared.shared_memory_policy, prepared.lead.agent_id);
    spec.quota = TeamResourceQuota {
        max_members: Some(1 + prepared.members.len() as u32),
        ..TeamResourceQuota::default()
    };
    spec.single_process_only = true;

    match prepared.topology {
        AgentTeamTopology::CoordinatorWorker => {
            let worker_ids: Vec<_> = prepared
                .members
                .iter()
                .map(|member| member.agent_id)
                .collect();
            spec.topology_config.coordinator = Some(prepared.lead.agent_id);
            spec.topology_config.workers = worker_ids;
        }
        AgentTeamTopology::PeerToPeer => {
            spec.topology_config.coordinator = None;
            spec.topology_config.workers = std::iter::once(prepared.lead.agent_id)
                .chain(prepared.members.iter().map(|member| member.agent_id))
                .collect();
        }
        AgentTeamTopology::RoleRouted => {
            spec.topology_config.coordinator = Some(prepared.lead.agent_id);
            spec.topology_config.role_routes = prepared
                .members
                .iter()
                .map(|member| RoleRoute {
                    role: member.profile.role.clone(),
                    targets: vec![member.agent_id],
                })
                .collect();
        }
    }

    spec
}

pub fn open_runtime_store(workspace_root: &Path) -> Result<AgentRuntimeStore, TeamRuntimeError> {
    Ok(AgentRuntimeStore::open(workspace_root)?)
}

fn member_profile_id_map(prepared: &PreparedRunScopedTeam) -> HashMap<AgentId, String> {
    std::iter::once((prepared.lead.agent_id, prepared.lead.profile_id.clone()))
        .chain(
            prepared
                .members
                .iter()
                .map(|member| (member.agent_id, member.profile_id.clone())),
        )
        .collect()
}

fn find_profile<'profiles>(
    profile_id: &str,
    profiles: &'profiles [AgentProfile],
) -> Result<&'profiles AgentProfile, TeamRuntimeError> {
    profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| TeamRuntimeError::UnknownProfile(profile_id.to_owned()))
}

fn truncate_goal(goal: &str) -> String {
    const MAX_TITLE_LEN: usize = 240;
    let trimmed = goal.trim();
    if trimmed.chars().count() <= MAX_TITLE_LEN {
        return trimmed.to_owned();
    }
    trimmed
        .chars()
        .take(MAX_TITLE_LEN)
        .chain(std::iter::once('…'))
        .collect()
}

fn map_topology(topology: AgentTeamTopology) -> Topology {
    match topology {
        AgentTeamTopology::CoordinatorWorker => Topology::CoordinatorWorker,
        AgentTeamTopology::PeerToPeer => Topology::PeerToPeer,
        AgentTeamTopology::RoleRouted => Topology::RoleRouted,
    }
}

fn map_shared_memory(
    policy: AgentTeamSharedMemoryPolicy,
    coordinator: AgentId,
) -> SharedMemorySpec {
    match policy {
        AgentTeamSharedMemoryPolicy::None => SharedMemorySpec::Disabled,
        AgentTeamSharedMemoryPolicy::SummariesOnly => SharedMemorySpec::Enabled {
            provider_id: "team-summaries".to_owned(),
            write_policy: SharedWritePolicy::CoordinatorOnly { coordinator },
        },
        AgentTeamSharedMemoryPolicy::RedactedMailbox => SharedMemorySpec::Enabled {
            provider_id: "team-mailbox".to_owned(),
            write_policy: SharedWritePolicy::RoleGated {
                allowed_roles: Vec::new(),
            },
        },
    }
}

fn visibility_for_profile(profile: &AgentProfile) -> ContextVisibility {
    match profile.context_mode {
        harness_contracts::AgentProfileContextMode::Minimal => ContextVisibility::Private,
        harness_contracts::AgentProfileContextMode::Focused => ContextVisibility::Private,
        harness_contracts::AgentProfileContextMode::FullWorkspace => ContextVisibility::All,
    }
}

fn engine_config_from_profile(profile: &AgentProfile) -> TeamMemberEngineConfig {
    let mut blocklist = profile
        .tool_blocklist
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let toolset = if let Some(allowlist) = &profile.tool_allowlist {
        TeamToolsetSelector::Custom(allowlist.clone())
    } else if blocklist.is_empty() {
        TeamToolsetSelector::InheritAll
    } else {
        TeamToolsetSelector::InheritWithBlocklist(blocklist.clone())
    };

    if let Some(allowlist) = &profile.tool_allowlist {
        for tool in allowlist {
            blocklist.remove(tool);
        }
    }

    TeamMemberEngineConfig {
        model_ref: profile.model_config_override.as_ref().and_then(|model| {
            let model_id = model.model_id.clone()?;
            Some(harness_contracts::ModelRef {
                provider_id: model
                    .provider_config_id
                    .clone()
                    .unwrap_or_else(|| "default".to_owned()),
                model_id,
            })
        }),
        toolset,
        tool_blocklist: blocklist,
        permission_mode: harness_contracts::PermissionMode::Default,
        interactivity: harness_contracts::InteractivityLevel::NoInteractive,
        sandbox_policy: TeamSandboxPolicy::Inherit,
        max_iterations: profile.max_turns,
        system_prompt_addendum: Some(format!(
            "You are team member `{}` ({})",
            profile.id, profile.role
        )),
        quota: None,
        token_budget: harness_team::TokenBudget::default(),
    }
}

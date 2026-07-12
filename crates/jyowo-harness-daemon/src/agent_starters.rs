use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use futures::{future::BoxFuture, StreamExt};
use harness_contracts::{
    AgentId, AgentProfile, AgentProfileMemoryScope, AgentTeamSharedMemoryPolicy,
    AgentTeamStarterCap, AgentTeamToolStartRequest, AgentTeamToolStartResponse, AgentTeamTopology,
    AgentUsePolicy, BackgroundAgentStarterCap, BackgroundAgentToolStartRequest,
    BackgroundAgentToolStartResponse, ContextVisibility, Event, Message, MessageId, MessagePart,
    MessageRole, TeamCreatedEvent, TeamId, TeamMemberJoinedEvent, TeamTerminatedEvent,
    TeamTerminationReason, TenantId, ToolError, TopologyKind, TurnInput,
};
use harness_journal::{
    AppendMetadata, EventStore, ReplayCursor, TaskBlobStore, TaskEventStoreAdapter, TaskStore,
};
use harness_subagent::{
    AnnounceMode, InteractivityLevel, ParentContext, SubagentInputStrategy, SubagentMemoryScope,
    SubagentSpec, ToolsetSelector,
};

use crate::{DetachedChild, SubagentParentBinding, SubagentSupervisor};

#[derive(Clone)]
pub struct DaemonAgentStarter {
    store: Arc<TaskStore>,
    subagents: Arc<SubagentSupervisor>,
    binding: SubagentParentBinding,
    blob_root: PathBuf,
    team_lock: Arc<tokio::sync::Mutex<()>>,
}

impl DaemonAgentStarter {
    #[must_use]
    pub fn new(
        store: Arc<TaskStore>,
        subagents: Arc<SubagentSupervisor>,
        binding: SubagentParentBinding,
        blob_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            store,
            subagents,
            binding,
            blob_root: blob_root.into(),
            team_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    async fn start_background(
        &self,
        request: BackgroundAgentToolStartRequest,
    ) -> Result<BackgroundAgentToolStartResponse, ToolError> {
        validate_background_request(&request)?;
        self.validate_parent_scope(request.tenant_id)?;
        let mut spec = SubagentSpec::minimal(request.title.clone(), request.goal.clone());
        apply_session_to_spec(
            &mut spec,
            request.session.permission_mode,
            request.session.interactivity,
            request.session.max_iterations,
            request.agent_tool_policy.max_depth,
        );
        let child = self
            .subagents
            .start_detached_child(
                self.binding,
                spec,
                turn_input(&request.goal),
                self.parent_context(
                    request.tenant_id,
                    request.conversation_id,
                    request.parent_run_id,
                    request.tool_use_id,
                    request.session.team_id,
                    None,
                )?,
            )
            .await
            .map_err(subagent_tool_error)?;
        Ok(BackgroundAgentToolStartResponse {
            background_agent_id: child.child_task_id.to_string(),
            conversation_id: request.conversation_id,
            parent_run_id: request.parent_run_id,
            title: request.title,
            status: "background".to_owned(),
        })
    }

    async fn start_team(
        &self,
        request: AgentTeamToolStartRequest,
    ) -> Result<AgentTeamToolStartResponse, ToolError> {
        let _guard = self.team_lock.lock().await;
        self.validate_parent_scope(request.tenant_id)?;
        let profiles = validate_team_request(&request)?;
        let event_store = TaskEventStoreAdapter::new(
            Arc::clone(&self.store),
            self.binding.parent_task_id,
            request.tenant_id,
            request.conversation_id,
            Arc::new(harness_contracts::NoopRedactor),
        );
        if has_active_team(&event_store, &request).await? {
            return Err(ToolError::Validation(
                "an agent team is already active for this run".to_owned(),
            ));
        }

        let team_id = TeamId::new();
        let prepared = prepare_member_snapshots(
            &self.store,
            self.binding.parent_task_id,
            &self.blob_root,
            team_id,
            &profiles,
            &request,
        )?;
        let member_specs = serde_json::to_vec(&prepared)
            .map_err(|error| ToolError::Internal(error.to_string()))?;
        event_store
            .append_with_metadata(
                request.tenant_id,
                request.conversation_id,
                AppendMetadata {
                    run_id: Some(request.parent_run_id),
                    ..AppendMetadata::default()
                },
                &[Event::TeamCreated(TeamCreatedEvent {
                    team_id,
                    tenant_id: request.tenant_id,
                    name: format!("agent-team-{team_id}"),
                    topology_kind: topology_kind(request.topology),
                    member_specs_hash: *blake3::hash(&member_specs).as_bytes(),
                    created_at: harness_contracts::now(),
                })],
            )
            .await
            .map_err(|error| ToolError::Internal(error.to_string()))?;

        let mut children = Vec::with_capacity(prepared.len());
        for member in prepared {
            let result = self
                .start_team_member(&event_store, &request, team_id, member)
                .await;
            match result {
                Ok(child) => children.push(child),
                Err(error) => {
                    self.rollback_team(&event_store, &request, team_id, &children)
                        .await;
                    return Err(error);
                }
            }
        }

        Ok(AgentTeamToolStartResponse {
            team_id,
            conversation_id: request.conversation_id,
            parent_run_id: request.parent_run_id,
            status: "started".to_owned(),
        })
    }

    async fn start_team_member(
        &self,
        event_store: &TaskEventStoreAdapter,
        request: &AgentTeamToolStartRequest,
        team_id: TeamId,
        member: PreparedTeamMember,
    ) -> Result<DetachedChild, ToolError> {
        let mut spec = spec_from_profile(&member.profile, &request.goal);
        apply_session_to_spec(
            &mut spec,
            request.session.permission_mode,
            request.session.interactivity,
            request.max_turns_per_goal.min(member.profile.max_turns),
            request.agent_tool_policy.max_depth,
        );
        let child = self
            .subagents
            .start_detached_child(
                self.binding,
                spec,
                turn_input(&request.goal),
                self.parent_context(
                    request.tenant_id,
                    request.conversation_id,
                    request.parent_run_id,
                    request.tool_use_id,
                    Some(team_id),
                    Some(member.profile.id.clone()),
                )?,
            )
            .await
            .map_err(subagent_tool_error)?;
        let agent_id = agent_id_from_actor(child.actor_id);
        let joined = event_store
            .append_with_metadata(
                request.tenant_id,
                request.conversation_id,
                AppendMetadata {
                    run_id: Some(request.parent_run_id),
                    ..AppendMetadata::default()
                },
                &[Event::TeamMemberJoined(TeamMemberJoinedEvent {
                    team_id,
                    agent_id,
                    role: member.profile.role,
                    session_id: child.session_id,
                    visibility: ContextVisibility::All,
                    spec_snapshot_id: member.snapshot,
                    spec_hash: member.spec_hash,
                    joined_at: harness_contracts::now(),
                })],
            )
            .await;
        if let Err(error) = joined {
            let _ = self
                .subagents
                .cancel_child(self.binding.parent_task_id, child.child_task_id);
            return Err(ToolError::Internal(error.to_string()));
        }
        Ok(child)
    }

    async fn rollback_team(
        &self,
        event_store: &TaskEventStoreAdapter,
        request: &AgentTeamToolStartRequest,
        team_id: TeamId,
        children: &[DetachedChild],
    ) {
        for child in children {
            let _ = self
                .subagents
                .cancel_child(self.binding.parent_task_id, child.child_task_id);
        }
        let _ = event_store
            .append_with_metadata(
                request.tenant_id,
                request.conversation_id,
                AppendMetadata {
                    run_id: Some(request.parent_run_id),
                    ..AppendMetadata::default()
                },
                &[Event::TeamTerminated(TeamTerminatedEvent {
                    team_id,
                    reason: TeamTerminationReason::Cancelled,
                    at: harness_contracts::now(),
                })],
            )
            .await;
    }

    fn validate_parent_scope(&self, tenant_id: TenantId) -> Result<(), ToolError> {
        if tenant_id != TenantId::SINGLE {
            return Err(ToolError::Validation(
                "agent starter tenant does not match the local daemon".to_owned(),
            ));
        }
        let parent = self
            .store
            .task_projection(self.binding.parent_task_id)
            .map_err(|error| ToolError::Internal(error.to_string()))?
            .ok_or_else(|| ToolError::Internal("parent task is missing".to_owned()))?;
        if parent
            .current_run
            .as_ref()
            .is_none_or(|run| run.segment_id != self.binding.parent_segment_id)
        {
            return Err(ToolError::Validation(
                "agent starter is not bound to the active parent segment".to_owned(),
            ));
        }
        Ok(())
    }

    fn parent_context(
        &self,
        tenant_id: TenantId,
        session_id: harness_contracts::SessionId,
        run_id: harness_contracts::RunId,
        tool_use_id: harness_contracts::ToolUseId,
        team_id: Option<TeamId>,
        team_member_profile_id: Option<String>,
    ) -> Result<ParentContext, ToolError> {
        let sibling_count = self
            .store
            .task_projection(self.binding.parent_task_id)
            .map_err(|error| ToolError::Internal(error.to_string()))?
            .map(|parent| parent.subagents.len())
            .unwrap_or_default()
            .try_into()
            .map_err(|_| ToolError::Validation("too many sibling agents".to_owned()))?;
        Ok(ParentContext {
            tenant_id,
            parent_session_id: session_id,
            parent_run_id: run_id,
            depth: self.binding.depth,
            sibling_count,
            trigger_tool_use_id: Some(tool_use_id),
            correlation_id: harness_contracts::CorrelationId::new(),
            team_id,
            team_member_profile_id,
        })
    }
}

impl BackgroundAgentStarterCap for DaemonAgentStarter {
    fn start_background_agent(
        &self,
        request: BackgroundAgentToolStartRequest,
    ) -> BoxFuture<'static, Result<BackgroundAgentToolStartResponse, ToolError>> {
        let starter = self.clone();
        Box::pin(async move { starter.start_background(request).await })
    }
}

impl AgentTeamStarterCap for DaemonAgentStarter {
    fn start_agent_team(
        &self,
        request: AgentTeamToolStartRequest,
    ) -> BoxFuture<'static, Result<AgentTeamToolStartResponse, ToolError>> {
        let starter = self.clone();
        Box::pin(async move { starter.start_team(request).await })
    }
}

#[derive(serde::Serialize)]
struct PreparedTeamMember {
    profile: AgentProfile,
    snapshot: harness_contracts::BlobRef,
    spec_hash: [u8; 32],
}

fn prepare_member_snapshots(
    store: &Arc<TaskStore>,
    task_id: harness_contracts::TaskId,
    blob_root: &PathBuf,
    team_id: TeamId,
    profiles: &[AgentProfile],
    request: &AgentTeamToolStartRequest,
) -> Result<Vec<PreparedTeamMember>, ToolError> {
    let blobs = TaskBlobStore::open(Arc::clone(store), task_id, blob_root)
        .map_err(|error| ToolError::Internal(error.to_string()))?;
    profiles
        .iter()
        .map(|profile| {
            let snapshot = serde_json::to_vec(&serde_json::json!({
                "teamId": team_id,
                "profile": profile,
                "goal": request.goal,
                "topology": request.topology,
                "maxTurnsPerGoal": request.max_turns_per_goal,
            }))
            .map_err(|error| ToolError::Internal(error.to_string()))?;
            let spec_hash = *blake3::hash(&snapshot).as_bytes();
            let snapshot = blobs
                .put("application/json", &snapshot)
                .map_err(|error| ToolError::Internal(error.to_string()))?;
            Ok(PreparedTeamMember {
                profile: profile.clone(),
                snapshot,
                spec_hash,
            })
        })
        .collect()
}

async fn has_active_team(
    event_store: &TaskEventStoreAdapter,
    request: &AgentTeamToolStartRequest,
) -> Result<bool, ToolError> {
    let events = event_store
        .read_envelopes(
            request.tenant_id,
            request.conversation_id,
            ReplayCursor::FromStart,
        )
        .await
        .map_err(|error| ToolError::Internal(error.to_string()))?
        .collect::<Vec<_>>()
        .await;
    let mut active = HashSet::new();
    for envelope in events
        .into_iter()
        .filter(|event| event.run_id == Some(request.parent_run_id))
    {
        match envelope.payload {
            Event::TeamCreated(event) => {
                active.insert(event.team_id);
            }
            Event::TeamTerminated(event) => {
                active.remove(&event.team_id);
            }
            _ => {}
        }
    }
    Ok(!active.is_empty())
}

fn validate_background_request(request: &BackgroundAgentToolStartRequest) -> Result<(), ToolError> {
    if request.goal.trim().is_empty() || request.title.trim().is_empty() {
        return Err(ToolError::Validation(
            "background agent goal and title are required".to_owned(),
        ));
    }
    if request.tenant_id != request.session.tenant_id
        || request.conversation_id != request.session.session_id
        || request.permission_mode != request.session.permission_mode
    {
        return Err(ToolError::Validation(
            "background agent session snapshot does not match the request".to_owned(),
        ));
    }
    if request.agent_tool_policy.subagents != AgentUsePolicy::Allowed
        || request.agent_tool_policy.background_agents != AgentUsePolicy::Allowed
    {
        return Err(ToolError::Validation(
            "background agents require an allowed subagent policy".to_owned(),
        ));
    }
    Ok(())
}

fn validate_team_request(
    request: &AgentTeamToolStartRequest,
) -> Result<Vec<AgentProfile>, ToolError> {
    if request.goal.trim().is_empty() || request.max_turns_per_goal == 0 {
        return Err(ToolError::Validation(
            "team goal and max turns are required".to_owned(),
        ));
    }
    if request.tenant_id != request.session.tenant_id
        || request.conversation_id != request.session.session_id
    {
        return Err(ToolError::Validation(
            "agent team session snapshot does not match the request".to_owned(),
        ));
    }
    harness_contracts::validate_agent_tool_policy(&request.agent_tool_policy)
        .map_err(|error| ToolError::Validation(error.to_string()))?;
    if request.agent_tool_policy.subagents != AgentUsePolicy::Allowed
        || request.agent_tool_policy.agent_team != AgentUsePolicy::Allowed
    {
        return Err(ToolError::Validation(
            "agent teams require an allowed subagent policy".to_owned(),
        ));
    }
    let config = request
        .agent_tool_policy
        .team_config
        .as_ref()
        .ok_or_else(|| ToolError::Validation("agent team config is required".to_owned()))?;
    if config.shared_memory_policy != AgentTeamSharedMemoryPolicy::SummariesOnly {
        return Err(ToolError::Validation(
            "daemon agent teams require summaries-only memory".to_owned(),
        ));
    }
    let requested_ids = std::iter::once(&config.lead_profile_id)
        .chain(config.member_profile_ids.iter())
        .collect::<Vec<_>>();
    if requested_ids.len() > request.agent_tool_policy.max_team_members as usize {
        return Err(ToolError::Validation(
            "agent team exceeds maxTeamMembers".to_owned(),
        ));
    }
    let builtins = harness_agent_runtime::builtin_agent_profiles();
    let mut profiles = Vec::with_capacity(requested_ids.len());
    for id in requested_ids {
        let profile = builtins
            .iter()
            .find(|profile| &profile.id == id)
            .cloned()
            .ok_or_else(|| ToolError::Validation(format!("unknown builtin agent profile: {id}")))?;
        harness_contracts::validate_agent_profile(&profile)
            .map_err(|error| ToolError::Validation(error.to_string()))?;
        profiles.push(profile);
    }
    Ok(profiles)
}

fn spec_from_profile(profile: &AgentProfile, goal: &str) -> SubagentSpec {
    let mut spec = SubagentSpec::minimal(profile.id.clone(), goal.to_owned());
    if let Some(allowlist) = &profile.tool_allowlist {
        spec.toolset = ToolsetSelector::Custom(allowlist.clone());
    }
    spec.tool_blocklist = profile.tool_blocklist.iter().cloned().collect();
    spec.memory_scope = match profile.memory_scope {
        AgentProfileMemoryScope::None => SubagentMemoryScope::Empty,
        AgentProfileMemoryScope::ReadOnly => SubagentMemoryScope::ReadOnly,
        AgentProfileMemoryScope::ReadWrite => SubagentMemoryScope::ReadWrite,
    };
    spec.input_strategy = SubagentInputStrategy::LatestUserOnly;
    spec.announce_mode = AnnounceMode::StructuredOnly;
    spec
}

fn apply_session_to_spec(
    spec: &mut SubagentSpec,
    permission_mode: harness_contracts::PermissionMode,
    interactivity: harness_contracts::InteractivityLevel,
    max_turns: u32,
    max_depth: u8,
) {
    spec.permission_mode = permission_mode;
    spec.interactivity = match interactivity {
        harness_contracts::InteractivityLevel::FullyInteractive => {
            InteractivityLevel::FullyInteractive
        }
        harness_contracts::InteractivityLevel::DeferredInteractive => {
            InteractivityLevel::DeferredInteractive
        }
        harness_contracts::InteractivityLevel::NoInteractive => InteractivityLevel::NoInteractive,
        _ => InteractivityLevel::NoInteractive,
    };
    spec.max_turns = max_turns.max(1);
    spec.max_depth = max_depth;
}

fn topology_kind(topology: AgentTeamTopology) -> TopologyKind {
    match topology {
        AgentTeamTopology::CoordinatorWorker => TopologyKind::CoordinatorWorker,
        AgentTeamTopology::PeerToPeer => TopologyKind::PeerToPeer,
        AgentTeamTopology::RoleRouted => TopologyKind::RoleRouted,
    }
}

fn agent_id_from_actor(actor_id: harness_contracts::ActorId) -> AgentId {
    AgentId::from_u128(u128::from_be_bytes(actor_id.as_bytes()))
}

fn turn_input(text: &str) -> TurnInput {
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

fn subagent_tool_error(error: harness_subagent::SubagentError) -> ToolError {
    ToolError::Internal(error.to_string())
}

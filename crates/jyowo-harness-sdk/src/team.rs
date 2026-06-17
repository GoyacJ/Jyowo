#[cfg(feature = "agents-team")]
pub use harness_contracts::Recipient;
#[cfg(feature = "agents-team")]
pub use harness_contracts::{MemberLeaveReason, TeamTerminationReason};
#[cfg(feature = "agents-team")]
pub use harness_team::{
    AgentMessage, ContextVisibility, CoordinatorWorkerRuntime, MessageBus, MessagePayload,
    PeerToPeerRuntime, RoleRoutedRuntime, TeamBuilder, TeamMember, TeamMemberEngineConfig,
    TeamMemberRunOutcome, TeamMemberRunRequest, TeamMemberRunner, TeamReport, TeamSandboxPolicy,
    TeamSpec, TeamToolsetSelector, Topology,
};

#[cfg(feature = "agents-team")]
#[derive(Clone)]
pub struct Team {
    runtime: std::sync::Arc<harness_team::Team>,
    execution: TeamExecutionRuntime,
    spec: TeamSpec,
    tenant_id: harness_contracts::TenantId,
    journal_session_id: harness_contracts::SessionId,
}

#[cfg(feature = "agents-team")]
#[derive(Clone)]
pub(crate) enum TeamExecutionRuntime {
    CoordinatorWorker(std::sync::Arc<CoordinatorWorkerRuntime>),
    PeerToPeer(std::sync::Arc<PeerToPeerRuntime>),
    RoleRouted(std::sync::Arc<RoleRoutedRuntime>),
    MessageOnly,
}

#[cfg(feature = "agents-team")]
impl Team {
    #[must_use]
    pub fn new(
        runtime: harness_team::Team,
        spec: TeamSpec,
        tenant_id: harness_contracts::TenantId,
        journal_session_id: harness_contracts::SessionId,
    ) -> Self {
        Self {
            runtime: std::sync::Arc::new(runtime),
            execution: TeamExecutionRuntime::MessageOnly,
            spec,
            tenant_id,
            journal_session_id,
        }
    }

    #[must_use]
    pub(crate) fn from_runtime(
        runtime: harness_team::Team,
        execution: TeamExecutionRuntime,
        spec: TeamSpec,
        tenant_id: harness_contracts::TenantId,
        journal_session_id: harness_contracts::SessionId,
    ) -> Self {
        Self {
            runtime: std::sync::Arc::new(runtime),
            execution,
            spec,
            tenant_id,
            journal_session_id,
        }
    }

    #[must_use]
    pub fn spec(&self) -> &TeamSpec {
        &self.spec
    }

    #[must_use]
    pub fn tenant_id(&self) -> harness_contracts::TenantId {
        self.tenant_id
    }

    #[must_use]
    pub fn journal_session_id(&self) -> harness_contracts::SessionId {
        self.journal_session_id
    }

    pub async fn dispatch(
        &self,
        from: harness_contracts::AgentId,
        to: harness_contracts::Recipient,
        goal: impl Into<String>,
    ) -> Result<AgentMessage, harness_team::TeamError> {
        self.runtime.dispatch(from, to, goal).await
    }

    pub async fn dispatch_goal(
        &self,
        goal: impl AsRef<str>,
    ) -> Result<TeamReport, harness_team::TeamError> {
        match &self.execution {
            TeamExecutionRuntime::CoordinatorWorker(runtime) => {
                runtime.dispatch_goal(goal.as_ref()).await
            }
            _ => Err(harness_team::TeamError::InvalidSpec(
                "dispatch_goal requires coordinator_worker topology".to_owned(),
            )),
        }
    }

    pub async fn dispatch_goal_from(
        &self,
        from: harness_contracts::AgentId,
        goal: impl AsRef<str>,
    ) -> Result<TeamReport, harness_team::TeamError> {
        match &self.execution {
            TeamExecutionRuntime::PeerToPeer(runtime) => {
                runtime.dispatch_goal(from, goal.as_ref()).await
            }
            _ => Err(harness_team::TeamError::InvalidSpec(
                "dispatch_goal_from requires peer_to_peer topology".to_owned(),
            )),
        }
    }

    pub async fn dispatch_goal_to(
        &self,
        from: harness_contracts::AgentId,
        recipient: harness_contracts::Recipient,
        goal: impl AsRef<str>,
    ) -> Result<TeamReport, harness_team::TeamError> {
        match &self.execution {
            TeamExecutionRuntime::RoleRouted(runtime) => {
                runtime.dispatch_goal(from, recipient, goal.as_ref()).await
            }
            _ => Err(harness_team::TeamError::InvalidSpec(
                "dispatch_goal_to requires role_routed topology".to_owned(),
            )),
        }
    }

    pub async fn post(
        &self,
        message: AgentMessage,
    ) -> Result<AgentMessage, harness_team::TeamError> {
        self.runtime.post(message).await
    }

    pub fn pause(&self) {
        self.runtime.pause();
    }

    pub fn resume(&self) {
        self.runtime.resume();
    }

    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.runtime.is_paused()
    }

    pub async fn pause_member(&self, agent_id: harness_contracts::AgentId) {
        self.runtime.pause_member(agent_id).await;
    }

    pub async fn resume_member(&self, agent_id: harness_contracts::AgentId) {
        self.runtime.resume_member(agent_id).await;
    }

    pub async fn is_member_paused(&self, agent_id: harness_contracts::AgentId) -> bool {
        self.runtime.is_member_paused(agent_id).await
    }

    pub async fn add_member(&self, member: TeamMember) -> Result<(), harness_team::TeamError> {
        self.runtime.add_member(member).await
    }

    pub async fn remove_member(
        &self,
        agent_id: harness_contracts::AgentId,
    ) -> Result<(), harness_team::TeamError> {
        self.runtime.remove_member(agent_id).await
    }

    pub async fn remove_member_with_reason(
        &self,
        agent_id: harness_contracts::AgentId,
        reason: MemberLeaveReason,
    ) -> Result<(), harness_team::TeamError> {
        self.runtime
            .remove_member_with_reason(agent_id, reason)
            .await
    }

    pub async fn terminate(
        &self,
        reason: TeamTerminationReason,
    ) -> Result<TeamReport, harness_team::TeamError> {
        self.runtime.terminate(reason).await
    }

    pub async fn members(&self) -> Vec<TeamMember> {
        self.runtime.members().await
    }

    #[must_use]
    pub fn control_handle(&self) -> harness_team::TeamControlHandle {
        match &self.execution {
            TeamExecutionRuntime::CoordinatorWorker(runtime) => runtime.control_handle(),
            TeamExecutionRuntime::PeerToPeer(runtime) => runtime.control_handle(),
            TeamExecutionRuntime::RoleRouted(runtime) => runtime.control_handle(),
            TeamExecutionRuntime::MessageOnly => self.runtime.control_handle(),
        }
    }
}

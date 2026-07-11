#[cfg(feature = "recall-memory")]
use harness_contracts::MemoryThreadSettings;
use harness_contracts::{
    ConfigHash, CorrelationId, InteractivityLevel, Message, PermissionActorSource, PermissionMode,
    RunId, RunModelSnapshot, SessionId, SnapshotId, TeamId, TenantId, TurnInput,
};
use std::time::Duration;

use crate::{CancellationToken, RunControlHandle};

pub type EngineError = harness_contracts::EngineError;
pub type EventStream = harness_journal::EventStream;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct EngineId(String);

impl EngineId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EngineId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct SessionHandle {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
}

#[derive(Clone)]
pub struct RunContext {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub user_id: Option<String>,
    pub team_id: Option<TeamId>,
    pub run_id: RunId,
    pub parent_run_id: Option<RunId>,
    pub correlation_id: CorrelationId,
    pub subagent_depth: u8,
    pub permission_mode: PermissionMode,
    pub permission_actor_source: PermissionActorSource,
    pub interactivity: InteractivityLevel,
    pub budget_limits: Option<RunBudgetLimits>,
    pub cancellation: CancellationToken,
    pub run_control: Option<RunControlHandle>,
    pub config_snapshot_id: SnapshotId,
    pub effective_config_hash: ConfigHash,
    pub started_from_scope_set: Vec<String>,
    pub context_seed: Vec<Message>,
    #[cfg(feature = "recall-memory")]
    pub memory_thread_settings: Option<MemoryThreadSettings>,
    pub model: Option<RunModelSnapshot>,
    pub model_config_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct RunBudgetLimits {
    pub max_tokens: Option<u64>,
    pub max_tool_calls: Option<u64>,
    pub max_duration: Option<Duration>,
    pub max_cost_micros: Option<u64>,
}

impl RunContext {
    #[must_use]
    pub fn new(tenant_id: TenantId, session_id: SessionId, run_id: RunId) -> Self {
        Self {
            tenant_id,
            session_id,
            user_id: None,
            team_id: None,
            run_id,
            parent_run_id: None,
            correlation_id: CorrelationId::new(),
            subagent_depth: 0,
            permission_mode: PermissionMode::Default,
            permission_actor_source: PermissionActorSource::ParentRun,
            interactivity: InteractivityLevel::NoInteractive,
            budget_limits: None,
            cancellation: CancellationToken::new(),
            run_control: None,
            config_snapshot_id: SnapshotId::from_u128(0),
            effective_config_hash: ConfigHash([0; 32]),
            started_from_scope_set: Vec::new(),
            context_seed: Vec::new(),
            #[cfg(feature = "recall-memory")]
            memory_thread_settings: None,
            model: None,
            model_config_id: None,
        }
    }

    #[must_use]
    pub fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancellation = cancellation;
        self
    }

    #[must_use]
    pub fn with_run_control(mut self, run_control: RunControlHandle) -> Self {
        self.run_control = Some(run_control);
        self
    }

    #[must_use]
    pub fn with_parent_run_id(mut self, parent_run_id: Option<RunId>) -> Self {
        self.parent_run_id = parent_run_id;
        self
    }

    #[must_use]
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    #[must_use]
    pub fn with_optional_user_id(mut self, user_id: Option<String>) -> Self {
        self.user_id = user_id;
        self
    }

    #[must_use]
    pub fn with_team_id(mut self, team_id: TeamId) -> Self {
        self.team_id = Some(team_id);
        self
    }

    #[must_use]
    pub fn with_optional_team_id(mut self, team_id: Option<TeamId>) -> Self {
        self.team_id = team_id;
        self
    }

    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: CorrelationId) -> Self {
        self.correlation_id = correlation_id;
        self
    }

    #[must_use]
    pub fn with_subagent_depth(mut self, depth: u8) -> Self {
        self.subagent_depth = depth;
        self
    }

    #[must_use]
    pub fn with_permission_mode(mut self, permission_mode: PermissionMode) -> Self {
        self.permission_mode = permission_mode;
        self
    }

    #[must_use]
    pub fn with_permission_actor_source(
        mut self,
        permission_actor_source: PermissionActorSource,
    ) -> Self {
        self.permission_actor_source = permission_actor_source;
        self
    }

    #[must_use]
    pub fn with_interactivity(mut self, interactivity: InteractivityLevel) -> Self {
        self.interactivity = interactivity;
        self
    }

    #[must_use]
    pub fn with_budget_limits(mut self, budget_limits: Option<RunBudgetLimits>) -> Self {
        self.budget_limits = budget_limits;
        self
    }

    #[must_use]
    pub fn with_config_snapshot(
        mut self,
        config_snapshot_id: SnapshotId,
        effective_config_hash: ConfigHash,
        started_from_scope_set: Vec<String>,
    ) -> Self {
        self.config_snapshot_id = config_snapshot_id;
        self.effective_config_hash = effective_config_hash;
        self.started_from_scope_set = started_from_scope_set;
        self
    }

    #[must_use]
    pub fn with_context_seed(mut self, context_seed: Vec<Message>) -> Self {
        self.context_seed = context_seed;
        self
    }

    #[cfg(feature = "recall-memory")]
    #[must_use]
    pub fn with_memory_thread_settings(mut self, settings: Option<MemoryThreadSettings>) -> Self {
        self.memory_thread_settings = settings;
        self
    }

    #[must_use]
    pub fn with_model_snapshot(mut self, model: RunModelSnapshot) -> Self {
        self.model_config_id = model.model_config_id.clone();
        self.model = Some(model);
        self
    }
}

#[async_trait::async_trait]
pub trait EngineRunner: Send + Sync + 'static {
    async fn run(
        &self,
        session: SessionHandle,
        input: TurnInput,
        ctx: RunContext,
    ) -> Result<EventStream, EngineError>;

    fn engine_id(&self) -> EngineId;
}

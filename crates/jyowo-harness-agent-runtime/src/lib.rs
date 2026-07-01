//! Cross-domain agent orchestration runtime (L3).
//!
//! Owns durable agent runtime storage, profile registry validation, and
//! run-scoped orchestration adapters. Policy authority remains in contracts
//! and permission layers; this crate owns persistence and assembly helpers.

#![forbid(unsafe_code)]

mod background;
mod isolation;
mod migrations;
mod policy;
mod profiles;
mod store;
#[cfg(feature = "agents-subagent")]
mod subagents;
#[cfg(feature = "agents-team")]
mod teams;

pub use background::{
    BackgroundAgentManager, BackgroundAgentRecord, BackgroundAgentStartRequest,
    BackgroundAgentTransitionError,
};
pub use isolation::{
    CreateWorkspaceIsolationLeaseRequest, GitDiscovery, WorkspaceIsolationCleanupResult,
    WorkspaceIsolationError, WorkspaceIsolationManager, AGENT_WORKTREES_DIR_NAME,
};
pub use migrations::CURRENT_SCHEMA_VERSION;
pub use policy::{
    default_agent_capability_environment, AgentCapabilitiesInput, AgentCapabilityEnvironment,
    AgentCapabilityResolver, AgentRuntimePolicyError, AgentRuntimePolicyResolver,
    ExecutionSettingsAgentInput, ResolvedAgentCapabilityPolicy, ResolvedAgentRuntimePolicy,
    DEFAULT_MAX_CONCURRENT_SUBAGENTS, DEFAULT_MAX_DEPTH, DEFAULT_MAX_TEAM_MEMBERS,
    MAX_ALLOWED_CONCURRENT_SUBAGENTS, MAX_ALLOWED_DEPTH, MAX_ALLOWED_TEAM_MEMBERS,
};
pub use profiles::{
    builtin_agent_profiles, quarantine_invalid_profile_file, AgentProfileRegistry,
    AgentProfileRegistryError, AgentProfilesFile,
};
pub use store::{
    AgentRuntimeStore, AgentRuntimeStoreError, AgentTeamMailboxRecord, AgentTeamTaskRecord,
    BackgroundAgentAttemptRecord, BackgroundAgentStoreRecord, WorkspaceIsolationLease,
    AGENT_RUNTIME_DB_FILENAME,
};
#[cfg(feature = "agents-subagent")]
pub use subagents::{
    assemble_subagent_runner, delegation_policy_from_run_options,
    install_subagent_runner_capability, should_install_subagent_runner,
    SubagentRunnerAssemblyInput, SubagentTeamAttribution,
};
#[cfg(feature = "agents-team")]
pub use teams::{
    build_team_spec, mark_team_task_active, open_runtime_store as open_team_runtime_store,
    persist_team_before_dispatch, prepare_run_scoped_team, should_start_run_scoped_team,
    PreparedRunScopedTeam, PreparedTeamMember, RunScopedTeamCoordinator,
    RunScopedTeamCoordinatorRequest, RunScopedTeamCreateRequest, RunScopedTeamHost,
    TeamRuntimeError,
};

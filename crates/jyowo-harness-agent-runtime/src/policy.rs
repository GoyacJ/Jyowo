use harness_contracts::{
    validate_agent_tool_policy, AgentCapabilityKind, AgentCapabilityUnavailableReason,
    AgentOrchestrationValidationError, AgentToolPolicy, AgentUsePolicy,
    AgentWorkspaceIsolationMode,
};
use std::path::Path;

use crate::isolation::{WorkspaceIsolationError, WorkspaceIsolationManager};
use crate::profiles::AgentProfileRegistry;
use crate::store::AgentRuntimeStore;

pub const DEFAULT_MAX_DEPTH: u8 = 2;
pub const DEFAULT_MAX_CONCURRENT_SUBAGENTS: u32 = 2;
pub const DEFAULT_MAX_TEAM_MEMBERS: u32 = 4;
pub const MAX_ALLOWED_DEPTH: u8 = 8;
pub const MAX_ALLOWED_CONCURRENT_SUBAGENTS: u32 = 8;
pub const MAX_ALLOWED_TEAM_MEMBERS: u32 = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCapabilityEnvironment {
    pub subagents_compiled: bool,
    pub agent_teams_compiled: bool,
    pub background_agents_compiled: bool,
    pub stream_permission_runtime_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAgentCapabilityPolicy {
    pub subagents_available: bool,
    pub agent_teams_available: bool,
    pub background_agents_available: bool,
    pub unavailable_reasons: Vec<AgentCapabilityUnavailableReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionSettingsAgentInput {
    pub subagents_enabled: bool,
    pub agent_teams_enabled: bool,
    pub background_agents_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCapabilitiesInput {
    pub subagents_available: bool,
    pub agent_teams_available: bool,
    pub background_agents_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAgentToolPolicy {
    pub options: AgentToolPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRuntimePolicyError {
    Validation(AgentOrchestrationValidationError),
    CapabilityDisabled { field: &'static str },
    CapabilityUnavailable { field: &'static str },
    UnknownProfileId { id: String },
    InvalidTeamGoalTurns { value: u32 },
    WorkspaceIsolationUnavailable { message: String },
    NonGitWorkspace,
    DirtyWorkspaceForGitWorktree,
}

impl std::fmt::Display for AgentRuntimePolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(error) => write!(f, "{error}"),
            Self::CapabilityDisabled { field } => {
                write!(f, "{field} is disabled in execution settings")
            }
            Self::CapabilityUnavailable { field } => {
                write!(f, "{field} is unavailable in this desktop build")
            }
            Self::UnknownProfileId { id } => write!(f, "unknown agent profile id: {id}"),
            Self::InvalidTeamGoalTurns { value } => {
                write!(f, "maxTurnsPerGoal must be at least 1 (got {value})")
            }
            Self::WorkspaceIsolationUnavailable { message } => write!(f, "{message}"),
            Self::NonGitWorkspace => {
                write!(f, "workspace is not a git repository")
            }
            Self::DirtyWorkspaceForGitWorktree => write!(
                f,
                "workspace worktree is dirty; git worktree isolation requires a clean base commit"
            ),
        }
    }
}

impl std::error::Error for AgentRuntimePolicyError {}

pub struct AgentCapabilityResolver;

impl AgentCapabilityResolver {
    pub fn resolve(
        workspace_root: &Path,
        environment: AgentCapabilityEnvironment,
    ) -> ResolvedAgentCapabilityPolicy {
        let mut unavailable_reasons = Vec::new();

        let execution_settings_store_open = execution_settings_store_open(workspace_root);
        if !execution_settings_store_open {
            let runtime_store_available = execution_settings_store_open;
            for capability in [
                AgentCapabilityKind::Subagents,
                AgentCapabilityKind::AgentTeams,
                AgentCapabilityKind::BackgroundAgents,
            ] {
                unavailable_reasons.push(
                    AgentCapabilityUnavailableReason::RuntimeStoreUnavailable {
                        capability,
                        message: "execution settings store is unavailable".to_owned(),
                    },
                );
            }
            return ResolvedAgentCapabilityPolicy {
                subagents_available: runtime_store_available,
                agent_teams_available: runtime_store_available,
                background_agents_available: runtime_store_available,
                unavailable_reasons,
            };
        }

        let profile_registry_status = profile_registry_status(workspace_root);
        let background_registry_open = background_registry_open(workspace_root);
        let restart_recovery_ok = restart_recovery_ok(workspace_root);
        let write_isolation_status = write_isolation_status(workspace_root);

        let subagents_available = environment.subagents_compiled
            && execution_settings_store_open
            && profile_registry_status.valid
            && environment.stream_permission_runtime_available;

        if !environment.subagents_compiled {
            unavailable_reasons.push(AgentCapabilityUnavailableReason::NotCompiled {
                capability: AgentCapabilityKind::Subagents,
            });
        }
        if !profile_registry_status.valid {
            unavailable_reasons.push(AgentCapabilityUnavailableReason::InvalidAgentProfiles {
                capability: AgentCapabilityKind::Subagents,
                message: profile_registry_status
                    .message
                    .clone()
                    .unwrap_or_else(|| "agent profiles are invalid".to_owned()),
            });
        }
        if !environment.stream_permission_runtime_available {
            unavailable_reasons.push(
                AgentCapabilityUnavailableReason::PermissionRuntimeUnavailable {
                    capability: AgentCapabilityKind::Subagents,
                },
            );
        }
        if !write_isolation_status.valid {
            unavailable_reasons.push(
                AgentCapabilityUnavailableReason::WorkspaceIsolationUnavailable {
                    capability: AgentCapabilityKind::Subagents,
                    message: write_isolation_status.message.clone().unwrap_or_else(|| {
                        "workspace isolation is unavailable for write mode".to_owned()
                    }),
                },
            );
        }

        let agent_teams_available = subagents_available
            && environment.agent_teams_compiled
            && team_runtime_policy_available();

        if subagents_available && !environment.agent_teams_compiled {
            unavailable_reasons.push(AgentCapabilityUnavailableReason::NotCompiled {
                capability: AgentCapabilityKind::AgentTeams,
            });
        }
        if !write_isolation_status.valid {
            unavailable_reasons.push(
                AgentCapabilityUnavailableReason::WorkspaceIsolationUnavailable {
                    capability: AgentCapabilityKind::AgentTeams,
                    message: write_isolation_status.message.clone().unwrap_or_else(|| {
                        "workspace isolation is unavailable for write mode".to_owned()
                    }),
                },
            );
        }

        let background_agents_available = environment.background_agents_compiled
            && background_registry_open
            && restart_recovery_ok
            && environment.stream_permission_runtime_available;

        if !environment.background_agents_compiled {
            unavailable_reasons.push(AgentCapabilityUnavailableReason::NotCompiled {
                capability: AgentCapabilityKind::BackgroundAgents,
            });
        }
        if background_registry_open && !environment.stream_permission_runtime_available {
            unavailable_reasons.push(
                AgentCapabilityUnavailableReason::PermissionRuntimeUnavailable {
                    capability: AgentCapabilityKind::BackgroundAgents,
                },
            );
        }
        if !write_isolation_status.valid {
            unavailable_reasons.push(
                AgentCapabilityUnavailableReason::WorkspaceIsolationUnavailable {
                    capability: AgentCapabilityKind::BackgroundAgents,
                    message: write_isolation_status.message.unwrap_or_else(|| {
                        "workspace isolation is unavailable for write mode".to_owned()
                    }),
                },
            );
        }

        ResolvedAgentCapabilityPolicy {
            subagents_available,
            agent_teams_available,
            background_agents_available,
            unavailable_reasons,
        }
    }
}

pub struct AgentRuntimePolicyResolver;

impl AgentRuntimePolicyResolver {
    pub fn merge(
        settings: &ExecutionSettingsAgentInput,
        agent_tool_policy: Option<&AgentToolPolicy>,
        capabilities: &AgentCapabilitiesInput,
        known_profile_ids: &[String],
        conversation_id: &str,
        workspace_root: &Path,
    ) -> Result<ResolvedAgentToolPolicy, AgentRuntimePolicyError> {
        if conversation_id.trim().is_empty() {
            return Err(AgentRuntimePolicyError::Validation(
                AgentOrchestrationValidationError::InvalidProfileId { id: String::new() },
            ));
        }

        let options = match agent_tool_policy {
            Some(options) => options.clone(),
            None => default_agent_tool_policy_from_settings(settings),
        };

        validate_agent_tool_policy(&options).map_err(AgentRuntimePolicyError::Validation)?;
        validate_numeric_limits(&options)?;
        validate_team_config_profiles(&options, known_profile_ids)?;
        validate_write_capable_isolation(&options, workspace_root)?;

        ensure_use_policy(
            options.subagents,
            settings.subagents_enabled,
            capabilities.subagents_available,
            "subagents",
        )?;
        ensure_use_policy(
            options.agent_team,
            settings.agent_teams_enabled,
            capabilities.agent_teams_available,
            "agentTeam",
        )?;
        ensure_use_policy(
            options.background_agents,
            settings.background_agents_enabled,
            capabilities.background_agents_available,
            "backgroundAgents",
        )?;

        Ok(ResolvedAgentToolPolicy { options })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProfileRegistryStatus {
    valid: bool,
    message: Option<String>,
}

struct WorkspaceIsolationStatus {
    valid: bool,
    message: Option<String>,
}

pub fn default_agent_capability_environment() -> AgentCapabilityEnvironment {
    AgentCapabilityEnvironment {
        subagents_compiled: cfg!(feature = "agents-subagent"),
        agent_teams_compiled: cfg!(feature = "agents-team"),
        background_agents_compiled: false,
        stream_permission_runtime_available: false,
    }
}

fn default_agent_tool_policy_from_settings(
    settings: &ExecutionSettingsAgentInput,
) -> AgentToolPolicy {
    AgentToolPolicy {
        subagents: if settings.subagents_enabled {
            AgentUsePolicy::Allowed
        } else {
            AgentUsePolicy::Off
        },
        agent_team: if settings.agent_teams_enabled {
            AgentUsePolicy::Allowed
        } else {
            AgentUsePolicy::Off
        },
        background_agents: if settings.background_agents_enabled {
            AgentUsePolicy::Allowed
        } else {
            AgentUsePolicy::Off
        },
        team_config: None,
        workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
        max_depth: DEFAULT_MAX_DEPTH,
        max_concurrent_subagents: DEFAULT_MAX_CONCURRENT_SUBAGENTS,
        max_team_members: DEFAULT_MAX_TEAM_MEMBERS,
    }
}

fn ensure_use_policy(
    policy: AgentUsePolicy,
    enabled: bool,
    available: bool,
    field: &'static str,
) -> Result<(), AgentRuntimePolicyError> {
    if policy == AgentUsePolicy::Allowed {
        if !enabled {
            return Err(AgentRuntimePolicyError::CapabilityDisabled { field });
        }
        if !available {
            return Err(AgentRuntimePolicyError::CapabilityUnavailable { field });
        }
    }
    Ok(())
}

fn validate_numeric_limits(options: &AgentToolPolicy) -> Result<(), AgentRuntimePolicyError> {
    if options.max_depth > MAX_ALLOWED_DEPTH {
        return Err(AgentRuntimePolicyError::Validation(
            AgentOrchestrationValidationError::InvalidConcurrency {
                field: "maxDepth",
                value: options.max_depth as u32,
            },
        ));
    }
    if options.max_concurrent_subagents > MAX_ALLOWED_CONCURRENT_SUBAGENTS {
        return Err(AgentRuntimePolicyError::Validation(
            AgentOrchestrationValidationError::InvalidConcurrency {
                field: "maxConcurrentSubagents",
                value: options.max_concurrent_subagents,
            },
        ));
    }
    if options.max_team_members > MAX_ALLOWED_TEAM_MEMBERS {
        return Err(AgentRuntimePolicyError::Validation(
            AgentOrchestrationValidationError::InvalidConcurrency {
                field: "maxTeamMembers",
                value: options.max_team_members,
            },
        ));
    }
    Ok(())
}

fn validate_team_config_profiles(
    options: &AgentToolPolicy,
    known_profile_ids: &[String],
) -> Result<(), AgentRuntimePolicyError> {
    let Some(team_config) = options.team_config.as_ref() else {
        return Ok(());
    };

    if team_config.max_turns_per_goal == 0 {
        return Err(AgentRuntimePolicyError::InvalidTeamGoalTurns {
            value: team_config.max_turns_per_goal,
        });
    }

    ensure_profile_known(&team_config.lead_profile_id, known_profile_ids)?;
    for member_id in &team_config.member_profile_ids {
        ensure_profile_known(member_id, known_profile_ids)?;
    }
    Ok(())
}

fn ensure_profile_known(
    profile_id: &str,
    known_profile_ids: &[String],
) -> Result<(), AgentRuntimePolicyError> {
    if known_profile_ids.iter().any(|id| id == profile_id) {
        Ok(())
    } else {
        Err(AgentRuntimePolicyError::UnknownProfileId {
            id: profile_id.to_owned(),
        })
    }
}

fn execution_settings_store_open(workspace_root: &Path) -> bool {
    std::fs::create_dir_all(workspace_root.join(".jyowo/runtime")).is_ok()
}

fn runtime_dir(workspace_root: &Path) -> std::path::PathBuf {
    workspace_root.join(".jyowo").join("runtime")
}

fn profile_registry_status(workspace_root: &Path) -> ProfileRegistryStatus {
    match AgentRuntimeStore::open_runtime_dir(runtime_dir(workspace_root)) {
        Ok(store) => match AgentProfileRegistry::new(&store).list() {
            Ok(_) => ProfileRegistryStatus {
                valid: true,
                message: None,
            },
            Err(error) => ProfileRegistryStatus {
                valid: false,
                message: Some(error.to_string()),
            },
        },
        Err(error) => ProfileRegistryStatus {
            valid: false,
            message: Some(error.to_string()),
        },
    }
}

fn background_registry_open(workspace_root: &Path) -> bool {
    AgentRuntimeStore::open_runtime_dir(runtime_dir(workspace_root)).is_ok()
}

fn write_isolation_status(workspace_root: &Path) -> WorkspaceIsolationStatus {
    match WorkspaceIsolationManager::open(workspace_root)
        .and_then(|manager| manager.validate_write_mode(AgentWorkspaceIsolationMode::PatchOnly))
    {
        Ok(()) => WorkspaceIsolationStatus {
            valid: true,
            message: None,
        },
        Err(error) => WorkspaceIsolationStatus {
            valid: false,
            message: Some(error.to_string()),
        },
    }
}

fn restart_recovery_ok(workspace_root: &Path) -> bool {
    match AgentRuntimeStore::open_runtime_dir(runtime_dir(workspace_root)) {
        Ok(store) => store
            .table_exists("restart_recovery_markers")
            .unwrap_or(false),
        Err(_) => false,
    }
}

fn team_runtime_policy_available() -> bool {
    cfg!(feature = "agents-team")
}

fn agent_modes_request_isolation(options: &AgentToolPolicy) -> bool {
    options.subagents == AgentUsePolicy::Allowed || options.agent_team == AgentUsePolicy::Allowed
}

fn is_write_capable_isolation(mode: AgentWorkspaceIsolationMode) -> bool {
    matches!(
        mode,
        AgentWorkspaceIsolationMode::PatchOnly | AgentWorkspaceIsolationMode::GitWorktree
    )
}

fn validate_write_capable_isolation(
    options: &AgentToolPolicy,
    workspace_root: &Path,
) -> Result<(), AgentRuntimePolicyError> {
    if !agent_modes_request_isolation(options)
        || !is_write_capable_isolation(options.workspace_isolation)
    {
        return Ok(());
    }

    let manager = WorkspaceIsolationManager::open(workspace_root).map_err(|error| {
        AgentRuntimePolicyError::WorkspaceIsolationUnavailable {
            message: error.to_string(),
        }
    })?;

    manager
        .validate_write_mode(options.workspace_isolation)
        .map_err(map_workspace_isolation_error)
}

fn map_workspace_isolation_error(error: WorkspaceIsolationError) -> AgentRuntimePolicyError {
    match error {
        WorkspaceIsolationError::NonGitWorkspace => AgentRuntimePolicyError::NonGitWorkspace,
        WorkspaceIsolationError::DirtyWorkspace => {
            AgentRuntimePolicyError::DirtyWorkspaceForGitWorktree
        }
        WorkspaceIsolationError::Unavailable { message } => {
            AgentRuntimePolicyError::WorkspaceIsolationUnavailable { message }
        }
        other => AgentRuntimePolicyError::WorkspaceIsolationUnavailable {
            message: other.to_string(),
        },
    }
}

use std::fs;
use std::io::Write;

use chrono::Utc;
use harness_agent_runtime::{
    AgentCapabilitiesInput, AgentCapabilityEnvironment, AgentCapabilityResolver,
    AgentRuntimePolicyError, AgentRuntimePolicyResolver, AgentRuntimeStore,
    ExecutionSettingsAgentInput,
};
use harness_contracts::{
    AgentCapabilityKind, AgentCapabilityUnavailableReason, AgentOrchestrationValidationError,
    AgentRunOptions, AgentTeamRunConfig, AgentTeamSharedMemoryPolicy, AgentTeamTopology,
    AgentUsePolicy, AgentWorkspaceIsolationMode, BackgroundRunPolicy,
};
use tempfile::{tempdir, NamedTempFile};

fn compiled_environment(stream_permission_runtime_available: bool) -> AgentCapabilityEnvironment {
    AgentCapabilityEnvironment {
        subagents_compiled: true,
        agent_teams_compiled: true,
        stream_permission_runtime_available,
    }
}

#[test]
fn subagents_unavailable_when_not_compiled() {
    let workspace = tempdir().expect("tempdir");
    let policy = AgentCapabilityResolver::resolve(
        workspace.path(),
        AgentCapabilityEnvironment {
            subagents_compiled: false,
            agent_teams_compiled: false,
            stream_permission_runtime_available: true,
        },
    );

    assert!(!policy.subagents_available);
    assert!(policy.unavailable_reasons.iter().any(|reason| matches!(
        reason,
        AgentCapabilityUnavailableReason::NotCompiled {
            capability: AgentCapabilityKind::Subagents,
        }
    )));
}

#[test]
fn agent_teams_unavailable_when_not_compiled() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let policy = AgentCapabilityResolver::resolve(
        workspace.path(),
        AgentCapabilityEnvironment {
            subagents_compiled: true,
            agent_teams_compiled: false,
            stream_permission_runtime_available: true,
        },
    );

    assert!(!policy.agent_teams_available);
    assert!(policy.unavailable_reasons.iter().any(|reason| matches!(
        reason,
        AgentCapabilityUnavailableReason::NotCompiled {
            capability: AgentCapabilityKind::AgentTeams,
        }
    )));
}

#[test]
fn subagents_unavailable_without_stream_permission_runtime() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let policy = AgentCapabilityResolver::resolve(workspace.path(), compiled_environment(false));

    assert!(!policy.subagents_available);
    assert!(policy.unavailable_reasons.iter().any(|reason| matches!(
        reason,
        AgentCapabilityUnavailableReason::PermissionRuntimeUnavailable {
            capability: AgentCapabilityKind::Subagents,
        }
    )));
}

#[test]
fn invalid_agent_profiles_mark_subagents_unavailable() {
    let workspace = tempdir().expect("tempdir");
    let store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    fs::write(store.profiles_file_path(), b"{not-json").expect("invalid profiles should write");

    let policy = AgentCapabilityResolver::resolve(workspace.path(), compiled_environment(true));

    assert!(!policy.subagents_available);
    assert!(policy.unavailable_reasons.iter().any(|reason| matches!(
        reason,
        AgentCapabilityUnavailableReason::InvalidAgentProfiles {
            capability: AgentCapabilityKind::Subagents,
            ..
        }
    )));
}

#[test]
fn runtime_store_unavailable_marks_all_capabilities_unavailable() {
    let file = NamedTempFile::new().expect("temp file");
    write!(file.as_file(), "blocked").expect("write temp file");

    let policy = AgentCapabilityResolver::resolve(file.path(), compiled_environment(true));

    assert!(!policy.subagents_available);
    assert!(!policy.agent_teams_available);
    assert!(!policy.background_agents_available);
    assert_eq!(policy.unavailable_reasons.len(), 3);
    assert!(policy.unavailable_reasons.iter().all(|reason| matches!(
        reason,
        AgentCapabilityUnavailableReason::RuntimeStoreUnavailable { .. }
    )));
}

#[test]
fn background_agents_unavailable_when_supervisor_missing() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let policy = AgentCapabilityResolver::resolve(workspace.path(), compiled_environment(true));

    assert!(!policy.background_agents_available);
    assert!(policy.unavailable_reasons.iter().any(|reason| matches!(
        reason,
        AgentCapabilityUnavailableReason::BackgroundSupervisorUnavailable { .. }
    )));
}

#[test]
fn background_agents_unavailable_when_fresh_supervisor_lock_has_no_live_control_channel() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    write_test_supervisor_token_and_lock(workspace.path());

    let policy = AgentCapabilityResolver::resolve(workspace.path(), compiled_environment(true));

    assert!(!policy.background_agents_available);
    assert!(policy.unavailable_reasons.iter().any(|reason| matches!(
        reason,
        AgentCapabilityUnavailableReason::BackgroundSupervisorUnavailable { .. }
    )));
}

#[test]
fn write_isolation_unavailable_is_reported_without_hiding_read_only_agent_capabilities() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let worktrees_dir = workspace
        .path()
        .join(".jyowo")
        .join("runtime")
        .join("agent-worktrees");
    fs::write(&worktrees_dir, "not a directory").expect("block isolation worktrees dir");

    let policy = AgentCapabilityResolver::resolve(workspace.path(), compiled_environment(true));

    assert!(policy.subagents_available);
    #[cfg(feature = "agents-team")]
    assert!(policy.agent_teams_available);
    #[cfg(not(feature = "agents-team"))]
    assert!(!policy.agent_teams_available);
    assert!(policy.unavailable_reasons.iter().any(|reason| matches!(
        reason,
        AgentCapabilityUnavailableReason::WorkspaceIsolationUnavailable {
            capability: AgentCapabilityKind::Subagents,
            ..
        }
    )));
    assert!(policy.unavailable_reasons.iter().any(|reason| matches!(
        reason,
        AgentCapabilityUnavailableReason::WorkspaceIsolationUnavailable {
            capability: AgentCapabilityKind::AgentTeams,
            ..
        }
    )));
}

#[test]
fn merge_rejects_write_capable_isolation_when_write_isolation_store_cannot_open() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let worktrees_dir = workspace
        .path()
        .join(".jyowo")
        .join("runtime")
        .join("agent-worktrees");
    fs::write(&worktrees_dir, "not a directory").expect("block isolation worktrees dir");
    let mut options = sample_subagent_options();
    options.workspace_isolation = AgentWorkspaceIsolationMode::PatchOnly;

    let error = AgentRuntimePolicyResolver::merge(
        &enabled_settings(),
        Some(&options),
        &all_available_capabilities(),
        &["reviewer".to_owned(), "worker".to_owned()],
        "conversation-1",
        workspace.path(),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        AgentRuntimePolicyError::WorkspaceIsolationUnavailable { .. }
    ));
}

#[test]
fn subagents_and_teams_available_with_compiled_runtime_and_stream_permission() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let policy = AgentCapabilityResolver::resolve(workspace.path(), compiled_environment(true));

    assert!(policy.subagents_available);
    #[cfg(feature = "agents-team")]
    assert!(policy.agent_teams_available);
    #[cfg(not(feature = "agents-team"))]
    assert!(!policy.agent_teams_available);
    assert!(!policy.background_agents_available);
}

fn enabled_settings() -> ExecutionSettingsAgentInput {
    ExecutionSettingsAgentInput {
        subagents_enabled: true,
        agent_teams_enabled: true,
        background_agents_enabled: true,
    }
}

fn all_available_capabilities() -> AgentCapabilitiesInput {
    AgentCapabilitiesInput {
        subagents_available: true,
        agent_teams_available: true,
        background_agents_available: true,
    }
}

fn sample_subagent_options() -> AgentRunOptions {
    AgentRunOptions {
        subagents: AgentUsePolicy::Allowed,
        agent_team: AgentUsePolicy::Off,
        team_config: None,
        background: BackgroundRunPolicy::Foreground,
        workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
        max_depth: 2,
        max_concurrent_subagents: 2,
        max_team_members: 4,
    }
}

fn sample_team_options() -> AgentRunOptions {
    AgentRunOptions {
        subagents: AgentUsePolicy::Allowed,
        agent_team: AgentUsePolicy::Allowed,
        team_config: Some(AgentTeamRunConfig {
            topology: AgentTeamTopology::CoordinatorWorker,
            lead_profile_id: "reviewer".to_owned(),
            member_profile_ids: vec!["worker".to_owned()],
            max_turns_per_goal: 4,
            shared_memory_policy: AgentTeamSharedMemoryPolicy::SummariesOnly,
        }),
        background: BackgroundRunPolicy::Foreground,
        workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
        max_depth: 2,
        max_concurrent_subagents: 2,
        max_team_members: 4,
    }
}

#[test]
fn merge_defaults_agent_capabilities_from_settings_when_agent_options_omitted() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let policy = AgentRuntimePolicyResolver::merge(
        &enabled_settings(),
        None,
        &all_available_capabilities(),
        &["reviewer".to_owned(), "worker".to_owned()],
        "conversation-1",
        workspace.path(),
    )
    .expect("defaults should merge");

    assert_eq!(policy.options.subagents, AgentUsePolicy::Allowed);
    assert_eq!(policy.options.agent_team, AgentUsePolicy::Allowed);
    assert!(policy.options.team_config.is_none());
    assert_eq!(policy.options.background, BackgroundRunPolicy::Background);
    assert!(policy.background_agent_id.is_some());
}

#[test]
fn merge_rejects_subagents_allowed_when_settings_disabled() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let error = AgentRuntimePolicyResolver::merge(
        &ExecutionSettingsAgentInput {
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        },
        Some(&sample_subagent_options()),
        &all_available_capabilities(),
        &["reviewer".to_owned(), "worker".to_owned()],
        "conversation-1",
        workspace.path(),
    )
    .unwrap_err();

    assert_eq!(
        error,
        AgentRuntimePolicyError::CapabilityDisabled { field: "subagents" }
    );
}

#[test]
fn merge_rejects_subagents_allowed_when_runtime_unavailable() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let error = AgentRuntimePolicyResolver::merge(
        &enabled_settings(),
        Some(&sample_subagent_options()),
        &AgentCapabilitiesInput {
            subagents_available: false,
            agent_teams_available: false,
            background_agents_available: false,
        },
        &["reviewer".to_owned(), "worker".to_owned()],
        "conversation-1",
        workspace.path(),
    )
    .unwrap_err();

    assert_eq!(
        error,
        AgentRuntimePolicyError::CapabilityUnavailable { field: "subagents" }
    );
}

#[test]
fn merge_rejects_invalid_numeric_limits() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let mut options = sample_subagent_options();
    options.max_concurrent_subagents = 0;

    let error = AgentRuntimePolicyResolver::merge(
        &enabled_settings(),
        Some(&options),
        &all_available_capabilities(),
        &["reviewer".to_owned(), "worker".to_owned()],
        "conversation-1",
        workspace.path(),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        AgentRuntimePolicyError::Validation(
            AgentOrchestrationValidationError::InvalidConcurrency { .. }
        )
    ));
}

#[test]
fn merge_allows_agent_team_without_team_config_for_model_visible_team_tool() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let mut options = sample_subagent_options();
    options.agent_team = AgentUsePolicy::Allowed;

    let policy = AgentRuntimePolicyResolver::merge(
        &enabled_settings(),
        Some(&options),
        &all_available_capabilities(),
        &["reviewer".to_owned(), "worker".to_owned()],
        "conversation-1",
        workspace.path(),
    )
    .expect("team tool availability should not require eager team config");

    assert_eq!(policy.options.agent_team, AgentUsePolicy::Allowed);
    assert!(policy.options.team_config.is_none());
}

#[test]
fn merge_rejects_team_config_when_team_use_is_off() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let mut options = sample_team_options();
    options.agent_team = AgentUsePolicy::Off;

    let error = AgentRuntimePolicyResolver::merge(
        &enabled_settings(),
        Some(&options),
        &all_available_capabilities(),
        &["reviewer".to_owned(), "worker".to_owned()],
        "conversation-1",
        workspace.path(),
    )
    .unwrap_err();

    assert_eq!(
        error,
        AgentRuntimePolicyError::Validation(
            AgentOrchestrationValidationError::UnexpectedTeamConfig
        )
    );
}

#[test]
fn merge_rejects_unknown_lead_profile_id() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let mut options = sample_team_options();
    options.team_config.as_mut().unwrap().lead_profile_id = "missing".to_owned();

    let error = AgentRuntimePolicyResolver::merge(
        &enabled_settings(),
        Some(&options),
        &all_available_capabilities(),
        &["reviewer".to_owned(), "worker".to_owned()],
        "conversation-1",
        workspace.path(),
    )
    .unwrap_err();

    assert_eq!(
        error,
        AgentRuntimePolicyError::UnknownProfileId {
            id: "missing".to_owned()
        }
    );
}

#[test]
fn merge_rejects_empty_member_profile_list() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let mut options = sample_team_options();
    options
        .team_config
        .as_mut()
        .unwrap()
        .member_profile_ids
        .clear();

    let error = AgentRuntimePolicyResolver::merge(
        &enabled_settings(),
        Some(&options),
        &all_available_capabilities(),
        &["reviewer".to_owned(), "worker".to_owned()],
        "conversation-1",
        workspace.path(),
    )
    .unwrap_err();

    assert_eq!(
        error,
        AgentRuntimePolicyError::Validation(AgentOrchestrationValidationError::EmptyTeamMemberList)
    );
}

#[test]
fn merge_enqueues_background_agent_id_when_background_requested() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    write_test_supervisor_token_and_lock(workspace.path());

    let mut options = sample_subagent_options();
    options.background = BackgroundRunPolicy::Background;

    let policy = AgentRuntimePolicyResolver::merge(
        &enabled_settings(),
        Some(&options),
        &all_available_capabilities(),
        &["reviewer".to_owned(), "worker".to_owned()],
        "conversation-1",
        workspace.path(),
    )
    .expect("background merge should succeed");

    assert!(policy.background_agent_id.is_some());
}

fn write_test_supervisor_token_and_lock(workspace: &std::path::Path) {
    let runtime_dir = workspace.join(".jyowo/runtime");
    std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
    let token = "test-background-supervisor-token";
    let token_hash = blake3::hash(token.as_bytes()).to_hex().to_string();
    let workspace_id = blake3::hash(workspace.display().to_string().as_bytes())
        .to_hex()
        .to_string();
    std::fs::write(
        runtime_dir.join("agent-supervisor.token"),
        serde_json::json!({
            "token": token,
            "tokenHash": token_hash,
            "tokenEpoch": 1,
            "workspaceId": workspace_id,
            "createdAt": Utc::now(),
        })
        .to_string(),
    )
    .expect("supervisor token");
    std::fs::write(
        runtime_dir.join("agent-supervisor.lock"),
        serde_json::json!({
            "status": "running",
            "workspaceId": workspace_id,
            "tokenHash": token_hash,
            "tokenEpoch": 1,
            "pid": 1,
            "controlAddr": "127.0.0.1:9",
            "startedAt": Utc::now(),
            "heartbeatAt": Utc::now(),
        })
        .to_string(),
    )
    .expect("supervisor lock");
}

#[test]
fn merge_rejects_git_worktree_in_non_git_workspace() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let mut options = sample_subagent_options();
    options.workspace_isolation = AgentWorkspaceIsolationMode::GitWorktree;

    let error = AgentRuntimePolicyResolver::merge(
        &enabled_settings(),
        Some(&options),
        &all_available_capabilities(),
        &["reviewer".to_owned(), "worker".to_owned()],
        "conversation-1",
        workspace.path(),
    )
    .unwrap_err();

    assert_eq!(error, AgentRuntimePolicyError::NonGitWorkspace);
}

#[test]
fn merge_allows_patch_only_without_git_repository() {
    let workspace = tempdir().expect("tempdir");
    let _store = AgentRuntimeStore::open(workspace.path()).expect("store opens");
    let mut options = sample_subagent_options();
    options.workspace_isolation = AgentWorkspaceIsolationMode::PatchOnly;

    AgentRuntimePolicyResolver::merge(
        &enabled_settings(),
        Some(&options),
        &all_available_capabilities(),
        &["reviewer".to_owned(), "worker".to_owned()],
        "conversation-1",
        workspace.path(),
    )
    .expect("patch-only isolation should not require git");
}

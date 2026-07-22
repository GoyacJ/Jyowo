use harness_contracts::{
    validate_agent_profile, validate_agent_tool_policy, validate_execution_defaults_dependencies,
    AgentCapabilitiesPayload, AgentCapabilityKind, AgentCapabilityUnavailableReason,
    AgentOrchestrationValidationError, AgentProfile, AgentProfileContextMode,
    AgentProfileMemoryScope, AgentProfileModelOverride, AgentProfileSandboxInheritance,
    AgentProfileScope, AgentTeamRunConfig, AgentTeamSharedMemoryPolicy, AgentTeamTopology,
    AgentToolPolicy, AgentUsePolicy, AgentWorkspaceIsolationMode, BackgroundAgentArchivedEvent,
    BackgroundAgentCancelledEvent, BackgroundAgentCompletedEvent, BackgroundAgentDeletedEvent,
    BackgroundAgentFailedEvent, BackgroundAgentId, BackgroundAgentInputRequestedEvent,
    BackgroundAgentInputSubmittedEvent, BackgroundAgentInterruptedEvent,
    BackgroundAgentPermissionRequestedEvent, BackgroundAgentPermissionResolvedEvent,
    BackgroundAgentStartedEvent, BackgroundAgentState, BackgroundAgentStateChangedEvent, Decision,
    Event, RequestId, RunId, SessionId, TenantId, UiSafeText,
};
use serde_json::json;

fn ui(value: &str) -> UiSafeText {
    UiSafeText::from_trusted_redacted(value)
}

#[test]
fn execution_defaults_require_subagents_for_dependent_capabilities() {
    for record in [
        harness_contracts::ExecutionDefaultsRecord {
            subagents_enabled: false,
            agent_teams_enabled: true,
            ..Default::default()
        },
        harness_contracts::ExecutionDefaultsRecord {
            subagents_enabled: false,
            background_agents_enabled: true,
            ..Default::default()
        },
    ] {
        assert!(validate_execution_defaults_dependencies(&record).is_err());
    }

    validate_execution_defaults_dependencies(&harness_contracts::ExecutionDefaultsRecord {
        subagents_enabled: true,
        agent_teams_enabled: true,
        background_agents_enabled: true,
        ..Default::default()
    })
    .expect("subagent-backed capabilities should validate");
}

#[test]
fn agent_team_starter_contract_carries_immutable_run_snapshot() {
    let run_id = harness_contracts::RunId::new();
    let session_id = harness_contracts::SessionId::new();
    let tool_use_id = harness_contracts::ToolUseId::new();
    let policy = harness_contracts::AgentToolPolicy {
        subagents: harness_contracts::AgentUsePolicy::Allowed,
        agent_team: harness_contracts::AgentUsePolicy::Allowed,
        team_config: Some(harness_contracts::AgentTeamRunConfig {
            topology: harness_contracts::AgentTeamTopology::CoordinatorWorker,
            lead_profile_id: "reviewer".to_owned(),
            member_profile_ids: vec!["worker".to_owned()],
            max_turns_per_goal: 3,
            shared_memory_policy: harness_contracts::AgentTeamSharedMemoryPolicy::SummariesOnly,
        }),
        background_agents: harness_contracts::AgentUsePolicy::Off,
        workspace_isolation: harness_contracts::AgentWorkspaceIsolationMode::ReadOnly,
        max_depth: 2,
        max_concurrent_subagents: 4,
        max_team_members: 4,
    };
    let request = harness_contracts::AgentTeamToolStartRequest {
        tenant_id: harness_contracts::TenantId::SINGLE,
        conversation_id: session_id,
        parent_run_id: run_id,
        tool_use_id,
        goal: "review the change".to_owned(),
        topology: harness_contracts::AgentTeamTopology::CoordinatorWorker,
        max_turns_per_goal: 3,
        agent_tool_policy: policy.clone(),
        session: harness_contracts::AgentTeamToolSessionSnapshot {
            tenant_id: harness_contracts::TenantId::SINGLE,
            session_id,
            tool_search: harness_contracts::ToolSearchMode::Disabled,
            tool_profile: harness_contracts::ToolProfile::Full,
            permission_mode: harness_contracts::PermissionMode::Default,
            interactivity: harness_contracts::InteractivityLevel::FullyInteractive,
            team_id: None,
            max_iterations: 16,
            context_compression_trigger_ratio: 0.8,
        },
    };

    assert_eq!(request.parent_run_id, run_id);
    assert_eq!(request.tool_use_id, tool_use_id);
    assert_eq!(request.agent_tool_policy, policy);
    assert_eq!(request.session.session_id, session_id);
}

#[test]
fn capability_unavailable_daemon_roundtrips() {
    let reason = AgentCapabilityUnavailableReason::DaemonUnavailable {
        capability: AgentCapabilityKind::Subagents,
        message: "task daemon is unavailable".to_owned(),
    };
    let value = serde_json::to_value(&reason).unwrap();
    assert_eq!(value["type"], "daemonUnavailable");
    assert_eq!(value["capability"], "subagents");

    let parsed: AgentCapabilityUnavailableReason = serde_json::from_value(value).unwrap();
    assert_eq!(parsed, reason);
}

#[test]
fn builtin_profile_with_read_only_scope_roundtrips() {
    let profile = AgentProfile {
        id: "reviewer".to_owned(),
        scope: AgentProfileScope::Builtin,
        role: "Reviewer".to_owned(),
        description: "Read-only review subagent".to_owned(),
        model_config_override: None,
        tool_allowlist: None,
        tool_blocklist: vec!["bash".to_owned()],
        sandbox_inheritance: AgentProfileSandboxInheritance::InheritParent,
        memory_scope: AgentProfileMemoryScope::ReadOnly,
        context_mode: AgentProfileContextMode::Minimal,
        max_turns: 8,
        max_depth: 1,
        default_workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
    };

    validate_agent_profile(&profile).expect("profile validates");
    let value = serde_json::to_value(&profile).unwrap();
    assert_eq!(value["scope"], "builtin");
    assert_eq!(value["memoryScope"], "read_only");

    let parsed: AgentProfile = serde_json::from_value(value).unwrap();
    assert_eq!(parsed, profile);
}

#[test]
fn user_profile_with_overrides_roundtrips() {
    let profile = AgentProfile {
        id: "custom_worker".to_owned(),
        scope: AgentProfileScope::User,
        role: "Worker".to_owned(),
        description: "User-defined worker".to_owned(),
        model_config_override: Some(AgentProfileModelOverride {
            provider_config_id: Some("openai-default".to_owned()),
            model_id: Some("gpt-test".to_owned()),
        }),
        tool_allowlist: Some(vec!["read".to_owned(), "grep".to_owned()]),
        tool_blocklist: vec![],
        sandbox_inheritance: AgentProfileSandboxInheritance::NarrowOnly,
        memory_scope: AgentProfileMemoryScope::ReadWrite,
        context_mode: AgentProfileContextMode::Focused,
        max_turns: 12,
        max_depth: 2,
        default_workspace_isolation: AgentWorkspaceIsolationMode::GitWorktree,
    };

    validate_agent_profile(&profile).expect("profile validates");
    let parsed: AgentProfile =
        serde_json::from_str(&serde_json::to_string(&profile).unwrap()).unwrap();
    assert_eq!(parsed, profile);
}

#[test]
fn tool_policy_subagents_allowed_team_off_background_agents_off_roundtrips() {
    let options = AgentToolPolicy {
        subagents: AgentUsePolicy::Allowed,
        agent_team: AgentUsePolicy::Off,
        team_config: None,
        background_agents: AgentUsePolicy::Off,
        workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
        max_depth: 2,
        max_concurrent_subagents: 2,
        max_team_members: 4,
    };

    validate_agent_tool_policy(&options).expect("options validate");
    let value = serde_json::to_value(&options).unwrap();
    assert_eq!(value["subagents"], "allowed");
    assert_eq!(value["agentTeam"], "off");
    assert_eq!(value["backgroundAgents"], "off");
}

#[test]
fn run_options_team_allowed_with_team_config_roundtrips() {
    let options = AgentToolPolicy {
        subagents: AgentUsePolicy::Allowed,
        agent_team: AgentUsePolicy::Allowed,
        team_config: Some(AgentTeamRunConfig {
            topology: AgentTeamTopology::CoordinatorWorker,
            lead_profile_id: "lead".to_owned(),
            member_profile_ids: vec!["worker_a".to_owned(), "worker_b".to_owned()],
            max_turns_per_goal: 6,
            shared_memory_policy: AgentTeamSharedMemoryPolicy::SummariesOnly,
        }),
        background_agents: AgentUsePolicy::Off,
        workspace_isolation: AgentWorkspaceIsolationMode::PatchOnly,
        max_depth: 2,
        max_concurrent_subagents: 2,
        max_team_members: 4,
    };

    validate_agent_tool_policy(&options).expect("options validate");
    let parsed: AgentToolPolicy =
        serde_json::from_str(&serde_json::to_string(&options).unwrap()).unwrap();
    assert_eq!(parsed, options);
}

#[test]
fn run_options_team_allowed_without_team_config_validates_for_model_visible_tool() {
    let options = AgentToolPolicy {
        subagents: AgentUsePolicy::Allowed,
        agent_team: AgentUsePolicy::Allowed,
        team_config: None,
        background_agents: AgentUsePolicy::Off,
        workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
        max_depth: 1,
        max_concurrent_subagents: 1,
        max_team_members: 2,
    };

    validate_agent_tool_policy(&options).expect("team tool availability validates");
}

#[test]
fn run_options_team_off_with_team_config_fails_validation() {
    let options = AgentToolPolicy {
        subagents: AgentUsePolicy::Off,
        agent_team: AgentUsePolicy::Off,
        team_config: Some(AgentTeamRunConfig {
            topology: AgentTeamTopology::PeerToPeer,
            lead_profile_id: "lead".to_owned(),
            member_profile_ids: vec!["peer".to_owned()],
            max_turns_per_goal: 3,
            shared_memory_policy: AgentTeamSharedMemoryPolicy::None,
        }),
        background_agents: AgentUsePolicy::Off,
        workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
        max_depth: 1,
        max_concurrent_subagents: 1,
        max_team_members: 2,
    };

    assert_eq!(
        validate_agent_tool_policy(&options).unwrap_err(),
        AgentOrchestrationValidationError::UnexpectedTeamConfig
    );
}

#[test]
fn tool_policy_background_agents_git_worktree_roundtrips() {
    let options = AgentToolPolicy {
        subagents: AgentUsePolicy::Allowed,
        agent_team: AgentUsePolicy::Off,
        team_config: None,
        background_agents: AgentUsePolicy::Allowed,
        workspace_isolation: AgentWorkspaceIsolationMode::GitWorktree,
        max_depth: 2,
        max_concurrent_subagents: 2,
        max_team_members: 4,
    };

    validate_agent_tool_policy(&options).expect("options validate");
    let value = serde_json::to_value(&options).unwrap();
    assert_eq!(value["backgroundAgents"], "allowed");
    assert_eq!(value["workspaceIsolation"], "git_worktree");
}

#[test]
fn agent_capabilities_payload_roundtrips() {
    let payload = AgentCapabilitiesPayload {
        subagents_enabled: true,
        agent_teams_enabled: false,
        background_agents_enabled: false,
        subagents_available: true,
        agent_teams_available: false,
        background_agents_available: false,
        unavailable_reasons: vec![AgentCapabilityUnavailableReason::DaemonUnavailable {
            capability: AgentCapabilityKind::BackgroundAgents,
            message: "task daemon is unavailable".to_owned(),
        }],
    };

    let parsed: AgentCapabilitiesPayload =
        serde_json::from_str(&serde_json::to_string(&payload).unwrap()).unwrap();
    assert_eq!(parsed, payload);
}

#[test]
fn invalid_profile_id_rejected() {
    let profile = AgentProfile {
        id: "Invalid-ID".to_owned(),
        scope: AgentProfileScope::User,
        role: "Worker".to_owned(),
        description: "bad id".to_owned(),
        model_config_override: None,
        tool_allowlist: None,
        tool_blocklist: vec![],
        sandbox_inheritance: AgentProfileSandboxInheritance::InheritParent,
        memory_scope: AgentProfileMemoryScope::None,
        context_mode: AgentProfileContextMode::Minimal,
        max_turns: 1,
        max_depth: 1,
        default_workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
    };

    assert!(validate_agent_profile(&profile).is_err());
}

#[test]
fn schema_export_includes_agent_orchestration_types() {
    let schemas = harness_contracts::export_all_schemas();
    for key in [
        "agent_profile",
        "agent_tool_policy",
        "agent_team_run_config",
        "agent_capabilities_payload",
        "agent_workspace_isolation_mode",
        "background_agent_state",
        "background_agent_started",
        "background_agent_state_changed",
        "background_agent_input_requested",
        "background_agent_input_submitted",
        "background_agent_permission_requested",
        "background_agent_permission_resolved",
        "background_agent_cancelled",
        "background_agent_completed",
        "background_agent_failed",
        "background_agent_interrupted",
        "background_agent_archived",
        "background_agent_deleted",
    ] {
        assert!(schemas.contains_key(key), "missing schema export: {key}");
    }
}

#[test]
fn background_agent_events_roundtrip_with_stable_tags() {
    let background_agent_id = BackgroundAgentId::new();
    let conversation_id = SessionId::new();
    let attempt_id = RunId::new();
    let request_id = RequestId::new();
    let at = chrono::Utc::now();
    let events = vec![
        Event::BackgroundAgentStarted(BackgroundAgentStartedEvent {
            background_agent_id,
            conversation_id,
            attempt_id,
            title: ui("Nightly work"),
            at,
        }),
        Event::BackgroundAgentStateChanged(BackgroundAgentStateChangedEvent {
            background_agent_id,
            from: BackgroundAgentState::Queued,
            to: BackgroundAgentState::Running,
            attempt_id: Some(attempt_id),
            reason: None,
            at,
        }),
        Event::BackgroundAgentInputRequested(BackgroundAgentInputRequestedEvent {
            background_agent_id,
            request_id,
            prompt: ui("Need input"),
            at,
        }),
        Event::BackgroundAgentInputSubmitted(BackgroundAgentInputSubmittedEvent {
            background_agent_id,
            request_id,
            input: ui("safe answer"),
            at,
        }),
        Event::BackgroundAgentPermissionRequested(BackgroundAgentPermissionRequestedEvent {
            background_agent_id,
            tenant_id: TenantId::SINGLE,
            conversation_id,
            request_id,
            attempt_id: Some(attempt_id),
            reason: ui("permission required"),
            at,
        }),
        Event::BackgroundAgentPermissionResolved(BackgroundAgentPermissionResolvedEvent {
            background_agent_id,
            tenant_id: TenantId::SINGLE,
            conversation_id,
            request_id,
            attempt_id: Some(attempt_id),
            decision: Decision::AllowOnce,
            at,
        }),
        Event::BackgroundAgentCancelled(BackgroundAgentCancelledEvent {
            background_agent_id,
            reason: Some(ui("user cancelled")),
            at,
        }),
        Event::BackgroundAgentCompleted(BackgroundAgentCompletedEvent {
            background_agent_id,
            summary: Some(ui("done")),
            at,
        }),
        Event::BackgroundAgentFailed(BackgroundAgentFailedEvent {
            background_agent_id,
            error: ui("failed safely"),
            at,
        }),
        Event::BackgroundAgentInterrupted(BackgroundAgentInterruptedEvent {
            background_agent_id,
            reason: ui("restart"),
            at,
        }),
        Event::BackgroundAgentArchived(BackgroundAgentArchivedEvent {
            background_agent_id,
            at,
        }),
        Event::BackgroundAgentDeleted(BackgroundAgentDeletedEvent {
            background_agent_id,
            at,
        }),
    ];

    let expected_tags = [
        "background_agent_started",
        "background_agent_state_changed",
        "background_agent_input_requested",
        "background_agent_input_submitted",
        "background_agent_permission_requested",
        "background_agent_permission_resolved",
        "background_agent_cancelled",
        "background_agent_completed",
        "background_agent_failed",
        "background_agent_interrupted",
        "background_agent_archived",
        "background_agent_deleted",
    ];

    for (event, expected_tag) in events.into_iter().zip(expected_tags) {
        let value = serde_json::to_value(&event).expect("serialize event");
        assert_eq!(value["type"], expected_tag);
        let roundtrip: Event = serde_json::from_value(value).expect("deserialize event");
        assert_eq!(roundtrip, event);
    }
}

#[test]
fn unknown_capability_reason_type_deserializes_as_error() {
    let value = json!({
        "type": "unknownReason",
        "capability": "subagents"
    });
    let parsed = serde_json::from_value::<AgentCapabilityUnavailableReason>(value);
    assert!(parsed.is_err());
}

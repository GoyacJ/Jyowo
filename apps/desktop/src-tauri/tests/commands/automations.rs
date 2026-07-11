use super::*;

fn global_automations_path(workspace: &std::path::Path) -> std::path::PathBuf {
    test_storage_layout_for_workspace(workspace)
        .global_config_root()
        .join("automations.json")
}

fn global_execution_settings_store(workspace: &std::path::Path) -> DesktopExecutionSettingsStore {
    DesktopExecutionSettingsStore::global_only_with_layout(test_storage_layout_for_workspace(
        workspace,
    ))
}

fn global_execution_settings_path(workspace: &std::path::Path) -> std::path::PathBuf {
    test_storage_layout_for_workspace(workspace).global_execution_defaults_file()
}

#[tokio::test]
async fn automation_store_missing_files_loads_empty_state() {
    let workspace = unique_workspace("automation-empty-state");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");

    let automations = list_automations_with_runtime_state(&state)
        .await
        .expect("missing automation file should load as empty");
    let runs = list_automation_runs_with_runtime_state(None, &state)
        .await
        .expect("missing automation run ledger should load as empty");

    assert!(automations.automations.is_empty());
    assert!(runs.runs.is_empty());
}

#[tokio::test]
async fn automation_store_blank_config_loads_empty_state() {
    let workspace = unique_workspace("automation-blank-state");
    let automations_path = global_automations_path(&workspace);
    std::fs::create_dir_all(automations_path.parent().unwrap())
        .expect("global config directory should exist");
    std::fs::write(&automations_path, b"  \n\t").expect("blank automation file should be seeded");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");

    let automations = list_automations_with_runtime_state(&state)
        .await
        .expect("blank automation file should load as empty");

    assert!(automations.automations.is_empty());
}

#[tokio::test]
async fn save_automation_writes_runtime_file_and_defaults_to_disabled() {
    let workspace = unique_workspace("automation-save-file");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    let automation = automation_spec("checks", false, MissedRunPolicy::Skip);

    let payload = save_automation_with_runtime_state(
        SaveAutomationRequest {
            automation: automation.clone(),
        },
        &state,
    )
    .await
    .expect("automation should save");

    assert_eq!(payload.automation.id, "checks");
    assert!(!payload.automation.enabled);
    let automations_path = global_automations_path(state.workspace_root());
    let saved = std::fs::read_to_string(&automations_path).expect("automation file should exist");
    assert!(saved.contains("\"id\": \"checks\""));
    assert!(
        std::fs::read_dir(automations_path.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp")),
        "successful atomic write should not leave temp files"
    );
}

#[tokio::test]
async fn save_automation_rejects_secret_like_prompt() {
    let workspace = unique_workspace("automation-secret-prompt");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    let mut automation = automation_spec("checks", false, MissedRunPolicy::Skip);
    automation.prompt = "Use token=sk-test-secret-value".to_owned();

    let error = save_automation_with_runtime_state(SaveAutomationRequest { automation }, &state)
        .await
        .expect_err("secret-like automation prompts should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(list_automations_with_runtime_state(&state)
        .await
        .unwrap()
        .automations
        .is_empty());
}

#[tokio::test]
async fn save_automation_rejects_non_read_only_workspace_snapshot() {
    let workspace = unique_workspace("automation-rejects-write-scope");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    let mut automation = automation_spec("checks", false, MissedRunPolicy::Skip);
    automation.workspace_access = WorkspaceAccess::ReadWrite {
        allowed_writable_subpaths: Vec::new(),
    };

    let error = save_automation_with_runtime_state(SaveAutomationRequest { automation }, &state)
        .await
        .expect_err("automation MVP should reject writable workspace snapshots");

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn automation_delete_and_set_enabled_update_saved_state() {
    let workspace = unique_workspace("automation-delete-enable");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    save_automation_with_runtime_state(
        SaveAutomationRequest {
            automation: automation_spec("checks", false, MissedRunPolicy::Skip),
        },
        &state,
    )
    .await
    .expect("automation should save");

    let enabled = jyowo_desktop_shell::commands::set_automation_enabled_with_runtime_state(
        SetAutomationEnabledRequest {
            enabled: true,
            id: "checks".to_owned(),
        },
        &state,
    )
    .await
    .expect("automation should enable");
    assert!(enabled.automation.enabled);

    let deleted = delete_automation_with_runtime_state("checks".to_owned(), &state)
        .await
        .expect("automation should delete");
    assert_eq!(deleted.status, "deleted");
    assert!(list_automations_with_runtime_state(&state)
        .await
        .unwrap()
        .automations
        .is_empty());
}

#[test]
fn execution_settings_save_default_without_changing_session_options() {
    let workspace = unique_workspace("execution-settings-session-options");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::BypassPermissions,
            tool_profile: ToolProfile::Coding,
            context_compression_trigger_ratio: 0.72,
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        },
        &global_execution_settings_store(state.workspace_root()),
        None,
    )
    .expect("execution settings should save");

    let options = state
        .settings_session_options(SessionId::new())
        .expect("session options");

    assert_eq!(options.permission_mode, PermissionMode::Default);
    assert_eq!(options.tool_profile, ToolProfile::Coding);
    assert_eq!(options.context_compression_trigger_ratio, 0.72);
}

#[test]
fn get_execution_settings_defaults_to_standard_mode() {
    let workspace = unique_workspace("execution-settings-default");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    let settings = get_execution_settings_with_store(
        &global_execution_settings_store(state.workspace_root()),
        None,
    )
    .expect("execution settings should load");

    assert_eq!(settings.permission_mode, PermissionMode::Default);
    assert_eq!(settings.tool_profile, ToolProfile::Full);
    assert_eq!(settings.context_compression_trigger_ratio, 0.8);
    assert_eq!(settings.auto_mode_available, cfg!(feature = "auto-mode"));
    assert!(!settings.agent_capabilities.subagents_enabled);
    assert!(!settings.agent_capabilities.agent_teams_enabled);
    assert!(!settings.agent_capabilities.background_agents_enabled);
    assert!(!settings.agent_capabilities.subagents_available);
    assert!(!settings.agent_capabilities.agent_teams_available);
    assert!(!settings.agent_capabilities.background_agents_available);
    assert!(
        settings.agent_capabilities.unavailable_reasons.len() >= 2,
        "expected resolver unavailable reasons, got {:?}",
        settings.agent_capabilities.unavailable_reasons
    );
}

#[test]
fn get_execution_settings_ignores_runtime_record() {
    let workspace = unique_workspace("execution-settings-ignore-runtime-record");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    let workspace = state.workspace_root().to_path_buf();
    let settings_path = workspace
        .join(".jyowo")
        .join("runtime")
        .join("execution-settings.json");
    std::fs::create_dir_all(settings_path.parent().unwrap())
        .expect("settings directory should exist");
    std::fs::write(&settings_path, r#"{"permission_mode":"auto"}"#)
        .expect("runtime execution settings should write");

    let settings =
        get_execution_settings_with_store(&global_execution_settings_store(&workspace), None)
            .expect("execution settings should load");

    assert_eq!(settings.permission_mode, PermissionMode::Default);
    assert!(!settings.agent_capabilities.subagents_enabled);
    assert!(!settings.agent_capabilities.agent_teams_enabled);
    assert!(!settings.agent_capabilities.background_agents_enabled);
}

#[test]
fn get_execution_settings_normalizes_unavailable_auto_default() {
    let workspace = unique_workspace("execution-settings-stale-auto");
    let settings_path = global_execution_settings_path(&workspace);
    std::fs::create_dir_all(settings_path.parent().unwrap())
        .expect("settings directory should exist");
    std::fs::write(&settings_path, br#"{"permissionMode":"auto"}"#)
        .expect("stale settings file should be written");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");

    let settings = get_execution_settings_with_store(
        &global_execution_settings_store(state.workspace_root()),
        None,
    )
    .expect("execution settings should load");

    let expected_permission_mode = if cfg!(feature = "auto-mode") {
        PermissionMode::Auto
    } else {
        PermissionMode::Default
    };
    assert_eq!(settings.permission_mode, expected_permission_mode);
    assert_eq!(settings.auto_mode_available, cfg!(feature = "auto-mode"));
}

#[test]
fn get_execution_settings_for_request_uses_global_defaults_for_registered_workspace() {
    let _lock = HOME_ENV_LOCK.lock().unwrap();
    let home = unique_workspace("execution-settings-project-registry-home");
    let active_workspace = unique_workspace("execution-settings-active-workspace");
    let requested_workspace = unique_workspace("execution-settings-requested-workspace");
    let unregistered_workspace = unique_workspace("execution-settings-unregistered-workspace");
    std::fs::create_dir_all(&home).expect("home directory should exist");
    std::fs::create_dir_all(&active_workspace).expect("active workspace should exist");
    std::fs::create_dir_all(&requested_workspace).expect("requested workspace should exist");
    std::fs::create_dir_all(&unregistered_workspace).expect("unregistered workspace should exist");
    let canonical_home = home.canonicalize().unwrap();
    let _home = EnvVarGuard::set(HOME_ENV, canonical_home.as_os_str());
    let active_workspace = active_workspace.canonicalize().unwrap();
    let requested_workspace = requested_workspace.canonicalize().unwrap();
    let unregistered_workspace = unregistered_workspace.canonicalize().unwrap();
    let registry = ProjectRegistry::load().expect("project registry should load from test HOME");
    registry
        .upsert_and_activate(&requested_workspace)
        .expect("requested workspace should be registered");
    registry
        .upsert_and_activate(&active_workspace)
        .expect("active workspace should be registered");
    let active_store = DesktopExecutionSettingsStore::global_only();
    set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::BypassPermissions,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        },
        &active_store,
        None,
    )
    .expect("active workspace settings should save");

    let active_settings = get_execution_settings_for_request(
        GetExecutionSettingsRequest {
            workspace_path: None,
        },
        &active_store,
        &registry,
        None,
    )
    .expect("active workspace settings should load");
    let requested_settings = get_execution_settings_for_request(
        GetExecutionSettingsRequest {
            workspace_path: Some(requested_workspace.to_string_lossy().into_owned()),
        },
        &active_store,
        &registry,
        None,
    )
    .expect("requested workspace settings should load");
    let unregistered_error = get_execution_settings_for_request(
        GetExecutionSettingsRequest {
            workspace_path: Some(unregistered_workspace.to_string_lossy().into_owned()),
        },
        &active_store,
        &registry,
        None,
    )
    .expect_err("unregistered workspace should be rejected");

    assert_eq!(
        active_settings.permission_mode,
        PermissionMode::BypassPermissions
    );
    assert_eq!(
        requested_settings.permission_mode,
        PermissionMode::BypassPermissions
    );
    assert_eq!(unregistered_error.code, "INVALID_PAYLOAD");
    assert!(unregistered_error.message.contains("not registered"));
    assert!(
        !unregistered_error
            .message
            .contains(&unregistered_workspace.to_string_lossy().to_string()),
        "unregistered workspace errors must not echo local paths"
    );
}

#[test]
fn set_execution_settings_rejects_unavailable_agent_capabilities() {
    let workspace = unique_workspace("execution-settings-unavailable-agent-capabilities");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let store = global_execution_settings_store(&workspace);

    for request in [
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: true,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        },
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: false,
            agent_teams_enabled: true,
            background_agents_enabled: false,
        },
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: true,
        },
    ] {
        let error = set_execution_settings_with_store(request, &store, None)
            .expect_err("unavailable capability should be rejected");
        assert_eq!(error.code, "INVALID_PAYLOAD");
    }
}

#[test]
fn set_execution_settings_serializes_agent_capability_fields() {
    let workspace = unique_workspace("execution-settings-agent-capability-serialization");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    let workspace = state.workspace_root().to_path_buf();
    let store = global_execution_settings_store(&workspace);

    let response = set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Coding,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        },
        &store,
        None,
    )
    .expect("disabled execution settings should save");

    assert!(!response.agent_capabilities.subagents_enabled);
    let settings_path = global_execution_settings_path(&workspace);
    let saved = std::fs::read_to_string(settings_path).expect("settings file should exist");
    let saved: Value = serde_json::from_str(&saved).expect("settings file should be json");
    assert_eq!(
        saved,
        json!({
            "permissionMode": "default",
            "toolProfile": "coding",
            "contextCompressionTriggerRatio": 0.8,
            "subagentsEnabled": false,
            "agentTeamsEnabled": false,
            "backgroundAgentsEnabled": false
        })
    );
}

#[test]
fn set_execution_settings_rejects_invalid_context_compression_trigger_ratio() {
    let workspace = unique_workspace("execution-settings-invalid-context-ratio");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let store = global_execution_settings_store(&workspace);

    let low_error = set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.49,
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        },
        &store,
        None,
    )
    .unwrap_err();
    assert_eq!(low_error.code, "INVALID_PAYLOAD");

    let high_error = set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.96,
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        },
        &store,
        None,
    )
    .unwrap_err();
    assert_eq!(high_error.code, "INVALID_PAYLOAD");
}

#[test]
fn invalid_execution_settings_file_resets_agent_capabilities() {
    let workspace = unique_workspace("execution-settings-invalid-reset");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    let workspace = state.workspace_root().to_path_buf();
    let settings_path = workspace
        .join(".jyowo")
        .join("runtime")
        .join("execution-settings.json");
    std::fs::create_dir_all(settings_path.parent().unwrap())
        .expect("settings directory should exist");
    std::fs::write(
        &settings_path,
        r#"{"permission_mode":"invalid","subagents_enabled":true}"#,
    )
    .expect("invalid execution settings should write");

    let settings =
        get_execution_settings_with_store(&global_execution_settings_store(&workspace), None)
            .expect("invalid execution settings should reset");

    assert_eq!(settings.permission_mode, PermissionMode::Default);
    assert!(!settings.agent_capabilities.subagents_enabled);
    assert!(!settings.agent_capabilities.agent_teams_enabled);
    assert!(!settings.agent_capabilities.background_agents_enabled);
    assert!(
        settings_path.exists(),
        "production execution settings load must not read or delete old runtime file"
    );
}

#[test]
fn set_execution_settings_rejects_auto_without_runtime_support() {
    let workspace = unique_workspace("execution-settings-auto-unavailable");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");

    let error = set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Auto,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        },
        &global_execution_settings_store(state.workspace_root()),
        None,
    )
    .expect_err("auto mode should be rejected without runtime support");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("unavailable"));
}

#[tokio::test]
async fn execution_settings_agent_capabilities_reflect_resolver_with_stream_permission() {
    let workspace = unique_workspace("execution-settings-agent-capabilities");
    let state = runtime_state_with_settings_runtime_for_workspace(workspace).await;
    let context = AgentCapabilityResolutionContext {
        stream_permission_runtime_available: true,
    };
    let store = global_execution_settings_store(state.workspace_root());

    let settings =
        get_execution_settings_with_store(&store, Some(&context)).expect("settings should load");

    assert!(settings.agent_capabilities.subagents_available);
    assert!(settings.agent_capabilities.agent_teams_available);
    assert!(!settings.agent_capabilities.background_agents_available);

    let saved = set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: true,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        },
        &store,
        Some(&context),
    )
    .expect("subagents should save when resolver reports availability");

    assert!(saved.agent_capabilities.subagents_enabled);
    assert!(saved.agent_capabilities.subagents_available);

    let background_error = set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: true,
        },
        &store,
        Some(&context),
    )
    .expect_err("background agents remain unavailable before supervisor wiring");

    assert_eq!(background_error.code, "INVALID_PAYLOAD");
}

#![allow(unused_imports)]

use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

fn global_execution_settings_store(workspace: &std::path::Path) -> DesktopExecutionSettingsStore {
    DesktopExecutionSettingsStore::global_only_with_layout(test_storage_layout_for_workspace(
        workspace,
    ))
}

fn global_execution_settings_path(workspace: &std::path::Path) -> std::path::PathBuf {
    test_storage_layout_for_workspace(workspace).global_execution_defaults_file()
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
    assert_eq!(settings.agent_capabilities.unavailable_reasons.len(), 3);
}

#[test]
fn get_execution_settings_reports_only_capabilities_missing_from_daemon() {
    let workspace = unique_workspace("execution-settings-partial-daemon-capabilities");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    let capabilities = harness_contracts::AgentCapabilities {
        subagents: true,
        agent_teams: false,
        background_agents: true,
    };

    let settings = get_execution_settings_with_store(
        &global_execution_settings_store(state.workspace_root()),
        Some(&capabilities),
    )
    .expect("execution settings should load");

    assert!(settings.agent_capabilities.subagents_available);
    assert!(!settings.agent_capabilities.agent_teams_available);
    assert!(settings.agent_capabilities.background_agents_available);
    assert_eq!(settings.agent_capabilities.unavailable_reasons.len(), 1);
    assert!(matches!(
        settings.agent_capabilities.unavailable_reasons[0],
        harness_contracts::AgentCapabilityUnavailableReason::DaemonUnavailable {
            capability: harness_contracts::AgentCapabilityKind::AgentTeams,
            ..
        }
    ));
}

#[test]
fn get_execution_settings_ignores_old_runtime_record() {
    let workspace = unique_workspace("execution-settings-ignore-old-runtime");
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
        .expect("old execution settings should write");

    let settings =
        get_execution_settings_with_store(&global_execution_settings_store(&workspace), None)
            .expect("execution settings should load");

    assert_eq!(settings.permission_mode, PermissionMode::Default);
    assert!(!settings.agent_capabilities.subagents_enabled);
    assert!(!settings.agent_capabilities.agent_teams_enabled);
    assert!(!settings.agent_capabilities.background_agents_enabled);
    assert!(settings_path.exists());
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
fn set_execution_settings_rejects_capabilities_without_subagents_before_persistence() {
    let capabilities = harness_contracts::AgentCapabilities::daemon_native();

    for (name, agent_teams_enabled, background_agents_enabled) in [
        ("execution-settings-teams-require-subagents", true, false),
        (
            "execution-settings-background-requires-subagents",
            false,
            true,
        ),
    ] {
        let workspace = unique_workspace(name);
        std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
        let store = global_execution_settings_store(&workspace);
        let settings_path = global_execution_settings_path(&workspace);

        let error = set_execution_settings_with_store(
            SetExecutionSettingsRequest {
                permission_mode: PermissionMode::Default,
                tool_profile: ToolProfile::Full,
                context_compression_trigger_ratio: 0.8,
                subagents_enabled: false,
                agent_teams_enabled,
                background_agents_enabled,
            },
            &store,
            Some(&capabilities),
        )
        .expect_err("dependent capabilities should require subagents");

        assert_eq!(error.code, "INVALID_PAYLOAD");
        assert!(
            !settings_path.exists(),
            "invalid settings must not be persisted"
        );
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
fn invalid_execution_settings_file_is_rejected() {
    let workspace = unique_workspace("execution-settings-invalid-reset");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    let workspace = state.workspace_root().to_path_buf();
    let settings_path = global_execution_settings_path(&workspace);
    std::fs::create_dir_all(settings_path.parent().unwrap())
        .expect("settings directory should exist");
    std::fs::write(
        &settings_path,
        r#"{"permission_mode":"invalid","subagents_enabled":true}"#,
    )
    .expect("invalid execution settings should write");

    let error =
        get_execution_settings_with_store(&global_execution_settings_store(&workspace), None)
            .expect_err("old snake_case settings should be rejected");

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert!(error.message.contains("execution settings"));
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

// ── Overlay precedence ──────────────────────────────────────────────

#[test]
fn resolve_effective_execution_settings_applies_global_defaults() {
    use harness_contracts::{ExecutionDefaultsRecord, ExecutionOverridesRecord};
    use jyowo_desktop_shell::commands::stores::GlobalConfigStore;
    use jyowo_desktop_shell::storage_layout::{JyowoHome, StorageLayout};

    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("canonical");
    let home = root.join(".jyowo");
    let layout = StorageLayout::new(JyowoHome::new(&home));
    let global = GlobalConfigStore::new(layout);

    global
        .save_execution_defaults(&ExecutionDefaultsRecord {
            permission_mode: PermissionMode::Auto,
            tool_profile: ToolProfile::Minimal,
            context_compression_trigger_ratio: 0.85,
            subagents_enabled: true,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        })
        .expect("save global");

    let effective = resolve_effective_execution_settings(
        Some(&global),
        None,
        None, // no run param
        None,
    )
    .expect("resolve");

    assert_eq!(effective.permission_mode, PermissionMode::Auto);
    assert_eq!(effective.tool_profile, ToolProfile::Minimal);
    assert!((effective.context_compression_trigger_ratio - 0.85).abs() < f32::EPSILON);
    assert!(effective.subagents_enabled);
}

#[test]
fn resolve_effective_execution_settings_ignores_project_overrides() {
    use harness_contracts::ExecutionDefaultsRecord;
    use jyowo_desktop_shell::commands::stores::{GlobalConfigStore, ProjectConfigStore};
    use jyowo_desktop_shell::storage_layout::{JyowoHome, StorageLayout};

    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("canonical");
    let home = root.join(".jyowo");
    let layout = StorageLayout::new(JyowoHome::new(&home));
    let global = GlobalConfigStore::new(layout.clone());
    let workspace = root.join("workspace");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    let project = ProjectConfigStore::new(layout, workspace);

    // Global: permission_mode=Auto
    global
        .save_execution_defaults(&ExecutionDefaultsRecord {
            permission_mode: PermissionMode::Auto,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        })
        .expect("save global");

    project
        .save_execution_overrides(
            &ExecutionDefaultsRecord {
                permission_mode: PermissionMode::BypassPermissions,
                tool_profile: ToolProfile::Coding,
                context_compression_trigger_ratio: 0.75,
                subagents_enabled: false,
                agent_teams_enabled: false,
                background_agents_enabled: false,
            }
            .into(),
        )
        .expect("save project");

    let effective = resolve_effective_execution_settings(Some(&global), Some(&project), None, None)
        .expect("resolve");

    assert_eq!(effective.permission_mode, PermissionMode::Auto);
    assert_eq!(effective.tool_profile, ToolProfile::Full);
    assert!((effective.context_compression_trigger_ratio - 0.8).abs() < f32::EPSILON);
}

#[test]
fn resolve_effective_execution_settings_ignores_partial_project_override() {
    use harness_contracts::{ExecutionDefaultsRecord, ExecutionOverridesRecord};
    use jyowo_desktop_shell::commands::stores::{GlobalConfigStore, ProjectConfigStore};
    use jyowo_desktop_shell::storage_layout::{JyowoHome, StorageLayout};

    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("canonical");
    let home = root.join(".jyowo");
    let layout = StorageLayout::new(JyowoHome::new(&home));
    let global = GlobalConfigStore::new(layout.clone());
    let workspace = root.join("workspace");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    let project = ProjectConfigStore::new(layout.clone(), workspace.clone());

    global
        .save_execution_defaults(&ExecutionDefaultsRecord {
            permission_mode: PermissionMode::BypassPermissions,
            tool_profile: ToolProfile::Minimal,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: true,
            agent_teams_enabled: true,
            background_agents_enabled: false,
        })
        .expect("save global");

    let override_path = layout.project_execution_overrides_file(&workspace);
    std::fs::create_dir_all(override_path.parent().expect("config parent"))
        .expect("create project config");
    std::fs::write(&override_path, r#"{"contextCompressionTriggerRatio":0.7}"#)
        .expect("write partial override");

    let effective = resolve_effective_execution_settings(Some(&global), Some(&project), None, None)
        .expect("resolve");

    assert_eq!(effective.permission_mode, PermissionMode::BypassPermissions);
    assert_eq!(effective.tool_profile, ToolProfile::Minimal);
    assert!((effective.context_compression_trigger_ratio - 0.8).abs() < f32::EPSILON);
    assert!(effective.subagents_enabled);
    assert!(effective.agent_teams_enabled);
    assert!(!effective.background_agents_enabled);
}

#[test]
fn resolve_effective_execution_settings_run_params_override_all() {
    use harness_contracts::ExecutionDefaultsRecord;
    use jyowo_desktop_shell::commands::stores::{GlobalConfigStore, ProjectConfigStore};
    use jyowo_desktop_shell::storage_layout::{JyowoHome, StorageLayout};

    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("canonical");
    let home = root.join(".jyowo");
    let layout = StorageLayout::new(JyowoHome::new(&home));
    let global = GlobalConfigStore::new(layout.clone());
    let workspace = root.join("workspace");
    std::fs::create_dir_all(&workspace).expect("create workspace");
    let project = ProjectConfigStore::new(layout, workspace);

    global
        .save_execution_defaults(&ExecutionDefaultsRecord {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        })
        .expect("save global");

    project
        .save_execution_overrides(
            &ExecutionDefaultsRecord {
                permission_mode: PermissionMode::Auto,
                tool_profile: ToolProfile::Coding,
                context_compression_trigger_ratio: 0.8,
                subagents_enabled: false,
                agent_teams_enabled: false,
                background_agents_enabled: false,
            }
            .into(),
        )
        .expect("save project");

    // Run explicitly requests BypassPermissions
    let effective = resolve_effective_execution_settings(
        Some(&global),
        Some(&project),
        Some(PermissionMode::BypassPermissions), // run param
        Some(ToolProfile::Minimal),              // run param
    )
    .expect("resolve");

    assert_eq!(effective.permission_mode, PermissionMode::BypassPermissions);
    assert_eq!(effective.tool_profile, ToolProfile::Minimal);
}

#[test]
fn resolve_effective_execution_settings_missing_project_falls_back_to_global() {
    use harness_contracts::ExecutionDefaultsRecord;
    use jyowo_desktop_shell::commands::stores::GlobalConfigStore;
    use jyowo_desktop_shell::storage_layout::{JyowoHome, StorageLayout};

    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("canonical");
    let home = root.join(".jyowo");
    let layout = StorageLayout::new(JyowoHome::new(&home));
    let global = GlobalConfigStore::new(layout);

    global
        .save_execution_defaults(&ExecutionDefaultsRecord {
            permission_mode: PermissionMode::BypassPermissions,
            tool_profile: ToolProfile::Minimal,
            context_compression_trigger_ratio: 0.6,
            subagents_enabled: true,
            agent_teams_enabled: true,
            background_agents_enabled: false,
        })
        .expect("save global");

    let effective = resolve_effective_execution_settings(
        Some(&global),
        None, // no project — falls back to global
        None,
        None,
    )
    .expect("resolve");

    assert_eq!(effective.permission_mode, PermissionMode::BypassPermissions);
    assert!(effective.subagents_enabled);
    assert!(effective.agent_teams_enabled);
}

#[test]
fn resolve_effective_execution_settings_missing_everything_falls_back_to_contract_defaults() {
    let effective = resolve_effective_execution_settings(
        None, // no global
        None, // no project
        None, // no run param
        None,
    )
    .expect("resolve");

    assert_eq!(effective.permission_mode, PermissionMode::Default);
    assert_eq!(effective.tool_profile, ToolProfile::Full);
    assert!(!effective.subagents_enabled);
}

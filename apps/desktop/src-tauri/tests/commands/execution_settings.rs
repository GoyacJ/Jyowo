#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

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
        &DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf()),
        None,
    )
    .expect("execution settings should save");

    let options = state
        .conversation_session_options(SessionId::new())
        .expect("session options");

    assert_eq!(options.permission_mode, PermissionMode::Default);
    assert_eq!(options.tool_profile, ToolProfile::Coding);
    assert_eq!(options.context_compression_trigger_ratio, 0.72);
}

#[tokio::test]
async fn active_conversation_runtime_applies_saved_tool_profile() {
    let workspace = unique_workspace("execution-settings-active-runtime-tool-profile");
    let state = runtime_state_with_harness_for_workspace(workspace).await;
    set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Coding,
            context_compression_trigger_ratio: 0.72,
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        },
        &DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf()),
        None,
    )
    .expect("execution settings should save");

    let (_, options) = state
        .active_conversation_runtime(SessionId::new())
        .expect("active runtime lookup")
        .expect("active runtime should be present");

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
        &DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf()),
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
fn get_execution_settings_ignores_legacy_runtime_record() {
    let workspace = unique_workspace("execution-settings-ignore-legacy-runtime");
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
        .expect("legacy execution settings should write");

    let settings = get_execution_settings_with_store(
        &DesktopExecutionSettingsStore::new(workspace.to_path_buf()),
        None,
    )
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
    let settings_dir = workspace.join(".jyowo").join("config");
    std::fs::create_dir_all(&settings_dir).expect("settings directory should exist");
    std::fs::write(
        settings_dir.join("execution-overrides.json"),
        br#"{"permissionMode":"auto"}"#,
    )
    .expect("stale settings file should be written");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");

    let settings = get_execution_settings_with_store(
        &DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf()),
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
fn get_execution_settings_for_request_reads_registered_workspace_instead_of_active_store() {
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
    let active_store = DesktopExecutionSettingsStore::new(active_workspace);
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
    assert_eq!(requested_settings.permission_mode, PermissionMode::Default);
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
    let store = DesktopExecutionSettingsStore::new(workspace);

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
    let store = DesktopExecutionSettingsStore::new(workspace.clone());

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
    let settings_path = workspace
        .join(".jyowo")
        .join("config")
        .join("execution-overrides.json");
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
    let store = DesktopExecutionSettingsStore::new(workspace);

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
    let settings_path = workspace
        .join(".jyowo")
        .join("config")
        .join("execution-overrides.json");
    std::fs::create_dir_all(settings_path.parent().unwrap())
        .expect("settings directory should exist");
    std::fs::write(
        &settings_path,
        r#"{"permission_mode":"invalid","subagents_enabled":true}"#,
    )
    .expect("invalid execution settings should write");

    let error = get_execution_settings_with_store(
        &DesktopExecutionSettingsStore::new(workspace.to_path_buf()),
        None,
    )
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
        &DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf()),
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
        None, // no project overrides
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
fn resolve_effective_execution_settings_project_overrides_global() {
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

    // Project overrides: permission_mode=BypassPermissions
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

    // Project overrides win
    assert_eq!(effective.permission_mode, PermissionMode::BypassPermissions);
    assert_eq!(effective.tool_profile, ToolProfile::Coding);
    assert!((effective.context_compression_trigger_ratio - 0.75).abs() < f32::EPSILON);
}

#[test]
fn resolve_effective_execution_settings_partial_project_override_preserves_global_fields() {
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
    assert!((effective.context_compression_trigger_ratio - 0.7).abs() < f32::EPSILON);
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

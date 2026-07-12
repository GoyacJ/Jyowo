#![allow(unused_imports)]

use super::support::*;
use super::*;

#[tokio::test]
async fn runtime_execution_status_returns_the_settings_runtime_payload() {
    let workspace = unique_workspace("runtime-status");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace)
        .await
        .expect("runtime state should initialize");

    let status = get_runtime_execution_status_with_runtime_state(&state)
        .expect("runtime status should load");

    assert!(!status.process_sandbox.backend_id.is_empty());
    assert!(!status.process_sandbox.available_network_policies.is_empty());
    assert!(!status.tools.is_empty());
    assert!(status.http_broker.available);
}

#[tokio::test]
async fn desktop_settings_runtime_does_not_start_the_legacy_memory_owner() {
    let workspace = unique_workspace("settings-runtime-without-memory-owner");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace)
        .await
        .expect("runtime state should initialize");
    let legacy_memory_database = state.runtime_root().join("memory/memory.sqlite3");

    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(
        !legacy_memory_database.exists(),
        "desktop settings construction must not create the legacy memory database"
    );
    let settings_runtime = state
        .settings_runtime()
        .expect("settings runtime should initialize");
    let plugin_registry = settings_runtime
        .plugin_registry()
        .expect("settings plugin registry");
    assert!(
        !plugin_registry.memory_provider_capability_enabled(),
        "desktop settings plugins must not receive memory provider registration capability"
    );
    assert!(
        plugin_registry.registered_memory_providers().is_empty(),
        "desktop settings runtime must not retain plugin memory providers"
    );
}

#[test]
fn runtime_execution_status_rejects_an_uninitialized_settings_runtime() {
    let workspace = unique_workspace("runtime-status-not-ready");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = DesktopRuntimeState::with_workspace_for_test(workspace).unwrap();

    let error = get_runtime_execution_status_with_runtime_state(&state)
        .expect_err("missing settings runtime must fail");

    assert_eq!(error.code, "RUNTIME_NOT_READY");
}

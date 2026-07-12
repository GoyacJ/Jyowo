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

#[test]
fn runtime_execution_status_rejects_an_uninitialized_settings_runtime() {
    let workspace = unique_workspace("runtime-status-not-ready");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = DesktopRuntimeState::with_workspace_for_test(workspace).unwrap();

    let error = get_runtime_execution_status_with_runtime_state(&state)
        .expect_err("missing settings runtime must fail");

    assert_eq!(error.code, "RUNTIME_NOT_READY");
}

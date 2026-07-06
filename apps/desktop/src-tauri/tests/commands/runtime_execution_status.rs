#![allow(unused_imports)]

use super::support::*;
use super::*;

#[tokio::test]
async fn runtime_execution_status_command_returns_backend_payload() {
    let state = runtime_state_with_harness().await;
    let harness = state.harness().expect("harness should be initialized");

    let status = harness.runtime_execution_status();

    // Verify the payload has the expected shape.
    assert!(
        !status.process_sandbox.backend_id.is_empty(),
        "backend_id must be set"
    );
    assert!(
        !status.process_sandbox.available_network_policies.is_empty(),
        "at least one network policy must be available"
    );
    assert!(
        !status.tools.is_empty(),
        "tool status list must not be empty"
    );

    // The HTTP broker should be available since the desktop runtime wires it.
    assert!(
        status.http_broker.available,
        "HTTP broker should be available when registered"
    );

    // Each tool must have a name and explicit availability.
    for tool in &status.tools {
        assert!(!tool.tool_name.is_empty(), "tool name must be set");
        if !tool.available {
            assert!(
                tool.unavailable_reason.is_some(),
                "unavailable tools must have a reason"
            );
        }
    }
}

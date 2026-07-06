//! Execution preflight registry.
//!
//! Owned by `jyowo-harness-execution` and injected into `AuthorizationService`.
//! Routes each `ToolExecutionChannel` to the component that can enforce it.

use std::sync::Arc;

use harness_contracts::CapabilityRegistry;
use harness_sandbox::SandboxBackend;
use harness_tool::ToolNetworkBrokerPreflightCap;

/// Typed registry that binds each enforcement channel to its preflight component.
///
/// `AuthorizationService::new` MUST receive this struct. None of the fields
/// silently disable checks — missing broker or missing capability must fail
/// closed with a channel-specific reason.
#[derive(Clone)]
pub struct ExecutionPreflightRegistry {
    pub sandbox_backend: Arc<dyn SandboxBackend>,
    pub network_broker: Option<Arc<dyn ToolNetworkBrokerPreflightCap>>,
    pub capabilities: Arc<CapabilityRegistry>,
}

impl ExecutionPreflightRegistry {
    pub fn new(
        sandbox_backend: Arc<dyn SandboxBackend>,
        network_broker: Option<Arc<dyn ToolNetworkBrokerPreflightCap>>,
        capabilities: Arc<CapabilityRegistry>,
    ) -> Self {
        Self {
            sandbox_backend,
            network_broker,
            capabilities,
        }
    }
}

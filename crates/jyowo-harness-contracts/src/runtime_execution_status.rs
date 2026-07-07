//! Backend-authored runtime execution capability status.
//!
//! These types are computed by the Rust runtime and rendered read-only by the
//! frontend. React must not infer availability from local constants — it must
//! only display what the backend reports here.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Top-level runtime execution capability status, computed at desktop startup
/// and whenever the runtime is reconfigured.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeExecutionStatus {
    pub process_sandbox: ProcessSandboxStatus,
    pub http_broker: BrokerStatus,
    pub tools: Vec<ToolRuntimeStatus>,
}

/// Describes which process sandbox backend is active and what policies it can
/// enforce.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSandboxStatus {
    /// The `backend_id()` of the selected routing backend (e.g. `"routing"`).
    pub backend_id: String,
    /// Candidate backend ids in selection order.
    pub candidate_ids: Vec<String>,
    /// Network policies the sandbox can enforce.
    pub available_network_policies: Vec<String>,
    /// Workspace policies the sandbox can enforce.
    pub available_workspace_policies: Vec<String>,
    /// Human-readable reasons why certain policies are unavailable.
    pub unavailable_reasons: Vec<String>,
}

/// Describes whether the authorized HTTP broker is available.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrokerStatus {
    pub available: bool,
    /// Human-readable reasons why the broker is unavailable.
    pub denied_reasons: Vec<String>,
}

/// Per-tool runtime availability status. Computed by the runtime based on
/// channel-specific enforcement availability.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolRuntimeStatus {
    pub tool_name: String,
    pub available: bool,
    /// Backend-authored reason when unavailable.
    pub unavailable_reason: Option<String>,
}

impl RuntimeExecutionStatus {
    /// Returns a status payload for the current runtime. The `tools` list is
    /// computed from built-in tool names that this runtime knows about.
    pub fn compute(
        process_sandbox_backend_id: &str,
        sandbox_candidate_ids: Vec<String>,
        available_network_policies: Vec<String>,
        available_workspace_policies: Vec<String>,
        sandbox_unavailable_reasons: Vec<String>,
        broker_available: bool,
        broker_denied_reasons: Vec<String>,
        tools: Vec<ToolRuntimeStatus>,
    ) -> Self {
        Self {
            process_sandbox: ProcessSandboxStatus {
                backend_id: process_sandbox_backend_id.to_owned(),
                candidate_ids: sandbox_candidate_ids,
                available_network_policies,
                available_workspace_policies,
                unavailable_reasons: sandbox_unavailable_reasons,
            },
            http_broker: BrokerStatus {
                available: broker_available,
                denied_reasons: broker_denied_reasons,
            },
            tools,
        }
    }
}

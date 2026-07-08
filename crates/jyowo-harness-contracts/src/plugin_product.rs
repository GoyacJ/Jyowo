//! Desktop plugin product contracts.
//!

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ManifestOriginRef, PluginId, PluginLifecycleStateDiscriminant, RejectionReason, SemverString,
    TrustLevel,
};

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginSourceKind {
    User,
    Workspace,
    Project,
    CargoExtension,
    Inline,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginProductState {
    Discovered,
    Validated,
    Disabled {
        #[serde(skip_serializing_if = "Option::is_none")]
        last_state: Option<PluginLifecycleStateDiscriminant>,
    },
    Activating,
    Activated,
    Rejected,
    Failed,
    Deactivated,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginRuntimeCapabilityKind {
    Tool,
    Hook,
    McpServer,
    Skill,
    Steering,
    MemoryProvider,
    Coordinator,
    CustomToolset,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginRecentEvent {
    Loaded,
    Failed,
    Rejected,
    Deactivated,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginRuntimeCapability {
    pub kind: PluginRuntimeCapabilityKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive: Option<bool>,
    pub registered: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginSummary {
    pub id: PluginId,
    pub name: String,
    pub version: SemverString,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub source: PluginSourceKind,
    pub trust_level: TrustLevel,
    pub enabled: bool,
    pub state: PluginProductState,
    pub capabilities: Vec<PluginRuntimeCapability>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginDetail {
    pub summary: PluginSummary,
    pub manifest_origin: ManifestOriginRef,
    pub manifest_hash: [u8; 32],
    pub manifest: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration_schema: Option<Value>,
    pub config: Value,
    pub registered_capabilities: Vec<PluginRuntimeCapability>,
    pub recent_events: Vec<PluginRecentEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<RejectionReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginInstallReport {
    pub source_path: String,
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<PluginSummary>,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginOperationStatus {
    Rejected,
    Installed,
    Enabled,
    Disabled,
    Configured,
    Uninstalled,
    Reloaded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginOperationResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<PluginId>,
    pub status: PluginOperationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<PluginSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<PluginInstallReport>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PluginConfigUpdate {
    pub plugin_id: PluginId,
    pub values: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginRuntimeRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginRuntimeRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<PluginRuntimeRpcError>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginRuntimeRpcError {
    pub code: i64,
    pub message: String,
}

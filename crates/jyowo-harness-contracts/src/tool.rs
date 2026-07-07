//! Tool descriptor contracts shared across model and tool crates.
//!
//! SPEC: docs/architecture/harness/crates/harness-contracts.md §3.4

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

use crate::{
    ActionPlanHash, ActionPlanId, CapabilityRouteKind, DecisionScope, ExecFingerprint,
    ManifestOriginRef, ModelModality, NetworkAccess, PermissionActorSource, PermissionSubject,
    ProviderRestriction, ResultBudget, SandboxPolicy, SandboxPolicyHash, SemverString, Severity,
    ToolCapability, ToolExecutionChannel, ToolGroup, ToolName, ToolOrigin, ToolProperties,
    ToolUseId, TrustLevel, WorkspaceAccess,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ToolServiceBinding {
    pub provider_id: String,
    pub operation_id: String,
    pub route_kind: CapabilityRouteKind,
    pub output_artifact: ModelModality,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderServiceAdapterAvailability {
    pub bindings: Vec<ToolServiceBinding>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolDescriptor {
    pub name: ToolName,
    pub display_name: String,
    pub description: String,
    pub category: String,
    pub group: ToolGroup,
    pub version: SemverString,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    pub dynamic_schema: bool,
    pub properties: ToolProperties,
    pub trust_level: TrustLevel,
    pub required_capabilities: Vec<ToolCapability>,
    pub budget: ResultBudget,
    pub provider_restriction: ProviderRestriction,
    pub origin: ToolOrigin,
    pub search_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_binding: Option<ToolServiceBinding>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum McpResourceOperation {
    List,
    Read { uri: String },
    Subscribe { uri: String },
    Unsubscribe { uri: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum McpPromptOperation {
    List,
    Get { name: String },
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct McpTransportTarget {
    pub transport: String,
    pub endpoint_label: String,
    pub endpoint_fingerprint: String,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum PermissionConfirmation {
    None,
    ExplicitButton { label: String },
    TypeToConfirm { expected: String },
}

impl Default for PermissionConfirmation {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReviewDetail {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub redacted: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReview {
    pub summary: String,
    pub details: Vec<PermissionReviewDetail>,
    pub confirmation: PermissionConfirmation,
    #[serde(default)]
    pub redacted: bool,
}

impl Default for PermissionReview {
    fn default() -> Self {
        Self {
            summary: "Permission review unavailable.".to_owned(),
            details: Vec::new(),
            confirmation: PermissionConfirmation::None,
            redacted: true,
        }
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ActionResource {
    FileRead {
        path: PathBuf,
    },
    FileWrite {
        path: PathBuf,
        content_hash: String,
    },
    FileDelete {
        path: PathBuf,
    },
    Command {
        command: String,
        argv: Vec<String>,
        cwd: Option<PathBuf>,
        fingerprint: ExecFingerprint,
    },
    Network {
        host: String,
        port: Option<u16>,
    },
    McpTool {
        server_id: String,
        origin: ManifestOriginRef,
        tool_name: String,
    },
    McpSampling {
        server_id: String,
        origin: ManifestOriginRef,
    },
    McpResource {
        server_id: String,
        origin: ManifestOriginRef,
        operation: McpResourceOperation,
    },
    McpPrompt {
        server_id: String,
        origin: ManifestOriginRef,
        operation: McpPromptOperation,
    },
    McpTransport {
        server_id: String,
        origin: ManifestOriginRef,
        target: McpTransportTarget,
    },
    Sandbox {
        backend_id: String,
        policy_hash: SandboxPolicyHash,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolActionPlan {
    pub plan_id: ActionPlanId,
    pub tool_use_id: ToolUseId,
    pub tool_name: String,
    pub actor_source: PermissionActorSource,
    pub subject: PermissionSubject,
    pub scope: DecisionScope,
    pub severity: Severity,
    pub resources: Vec<ActionResource>,
    pub sandbox_policy: SandboxPolicy,
    pub workspace_access: WorkspaceAccess,
    pub network_access: NetworkAccess,
    pub execution_channel: ToolExecutionChannel,
    pub review: PermissionReview,
    pub plan_hash: ActionPlanHash,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

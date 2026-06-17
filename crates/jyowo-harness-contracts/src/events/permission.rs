use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PermissionRequestedEvent {
    pub request_id: RequestId,
    pub run_id: RunId,
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub tool_use_id: ToolUseId,
    pub tool_name: String,
    pub subject: PermissionSubject,
    pub severity: Severity,
    pub scope_hint: DecisionScope,
    pub fingerprint: Option<ExecFingerprint>,
    pub presented_options: Vec<Decision>,
    pub interactivity: InteractivityLevel,
    pub causation_id: EventId,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PermissionResolvedEvent {
    pub request_id: RequestId,
    pub decision: Decision,
    pub decided_by: DecidedBy,
    pub scope: DecisionScope,
    pub fingerprint: Option<ExecFingerprint>,
    pub rationale: Option<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PermissionPersistenceTamperedEvent {
    pub tenant_id: TenantId,
    pub file_path_hash: [u8; 32],
    pub fingerprint: Option<ExecFingerprint>,
    pub reason: PersistenceTamperReason,
    pub key_id: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PermissionRequestSuppressedEvent {
    pub request_id: RequestId,
    pub run_id: RunId,
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub tool_use_id: ToolUseId,
    pub tool_name: String,
    pub subject: PermissionSubject,
    pub severity: Severity,
    pub scope_hint: DecisionScope,
    pub original_request_id: RequestId,
    pub original_decision_id: Option<DecisionId>,
    pub reused_decision: Option<Decision>,
    pub reason: SuppressionReason,
    pub causation_id: EventId,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PermissionAwaitingHeartbeatEvent {
    pub request_id: RequestId,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CredentialPoolSharedAcrossTenantsEvent {
    pub tenant_id: TenantId,
    pub provider_id: String,
    pub credential_key_hash: [u8; 32],
    pub at: DateTime<Utc>,
}

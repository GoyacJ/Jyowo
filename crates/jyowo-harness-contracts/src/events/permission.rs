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
    #[serde(default)]
    pub auto_resolved: bool,
    #[serde(default, skip_serializing_if = "PermissionActorSource::is_parent_run")]
    pub actor_source: PermissionActorSource,
    #[serde(default)]
    pub action_plan_hash: ActionPlanHash,
    #[serde(default)]
    pub review: PermissionReview,
    #[serde(default)]
    pub effective_mode: PermissionMode,
    #[serde(default)]
    pub sandbox_policy: SandboxPolicySummary,
    pub causation_id: EventId,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum PermissionActorSource {
    ParentRun,
    Subagent {
        subagent_id: SubagentId,
        parent_session_id: SessionId,
        parent_run_id: RunId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team_id: Option<TeamId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        team_member_profile_id: Option<String>,
    },
    TeamMember {
        team_id: TeamId,
        agent_id: AgentId,
        role: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_run_id: Option<RunId>,
    },
    BackgroundAgent {
        background_agent_id: BackgroundAgentId,
        conversation_id: SessionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attempt_id: Option<RunId>,
    },
    Automation {
        automation_id: String,
        conversation_id: SessionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<RunId>,
    },
    McpServer {
        server_id: McpServerId,
        origin: ManifestOriginRef,
        scope: McpServerScope,
    },
}

impl Default for PermissionActorSource {
    fn default() -> Self {
        Self::ParentRun
    }
}

impl PermissionActorSource {
    #[must_use]
    pub fn is_parent_run(&self) -> bool {
        matches!(self, Self::ParentRun)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PermissionResolvedEvent {
    pub request_id: RequestId,
    pub decision: Decision,
    pub decided_by: DecidedBy,
    pub scope: DecisionScope,
    pub fingerprint: Option<ExecFingerprint>,
    pub rationale: Option<String>,
    #[serde(default)]
    pub action_plan_hash: ActionPlanHash,
    #[serde(default)]
    pub decision_id: DecisionId,
    #[serde(default)]
    pub auto_resolved: bool,
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

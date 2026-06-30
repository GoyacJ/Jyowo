use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{PermissionMode, SandboxMode, ToolProfile, WorkspaceAccess};

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AutomationSchedule {
    pub interval_minutes: u32,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MissedRunPolicy {
    Skip,
    RunOnce,
}

impl Default for MissedRunPolicy {
    fn default() -> Self {
        Self::Skip
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutomationWorkspaceScope {
    CurrentWorkspace,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AutomationSpec {
    pub id: String,
    #[serde(default)]
    pub enabled: bool,
    pub prompt: String,
    pub schedule: AutomationSchedule,
    pub tool_profile: ToolProfile,
    pub permission_mode: PermissionMode,
    pub sandbox_mode: SandboxMode,
    pub workspace_scope: AutomationWorkspaceScope,
    pub workspace_access: WorkspaceAccess,
    #[serde(default)]
    pub missed_run_policy: MissedRunPolicy,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunStatus {
    Started,
    Rejected,
    Failed,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AutomationRunRecord {
    pub automation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub started_at: DateTime<Utc>,
    pub status: AutomationRunStatus,
}

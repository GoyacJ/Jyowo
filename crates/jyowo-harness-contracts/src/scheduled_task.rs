use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::PermissionMode;

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScheduledTaskSchedule {
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

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScheduledTaskSpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    pub prompt: String,
    pub schedule: ScheduledTaskSchedule,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    pub permission_mode: PermissionMode,
    #[serde(default)]
    pub missed_run_policy: MissedRunPolicy,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledTaskRunStatus {
    Started,
    Succeeded,
    Failed,
    Cancelled,
    Rejected,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScheduledTaskRunRecord {
    pub scheduled_task_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub started_at: DateTime<Utc>,
    pub status: ScheduledTaskRunStatus,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScheduledTasksResponse {
    pub scheduled_tasks: Vec<ScheduledTaskSpec>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScheduledTaskSavedResponse {
    pub scheduled_task: ScheduledTaskSpec,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScheduledTaskEnabledResponse {
    pub scheduled_task: ScheduledTaskSpec,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScheduledTaskDeletedResponse {
    pub scheduled_task_id: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScheduledTaskRunResponse {
    pub run: ScheduledTaskRunRecord,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScheduledTaskRunsResponse {
    pub runs: Vec<ScheduledTaskRunRecord>,
}

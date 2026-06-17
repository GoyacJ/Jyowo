use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubagentSpawnedEvent {
    pub subagent_id: SubagentId,
    pub parent_session_id: SessionId,
    pub parent_run_id: RunId,
    pub agent_ref: AgentRef,
    pub spec_snapshot_id: SnapshotId,
    pub spec_hash: [u8; 32],
    pub depth: u8,
    pub trigger_tool_use_id: Option<ToolUseId>,
    pub trigger_tool_name: Option<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubagentAnnouncedEvent {
    pub subagent_id: SubagentId,
    pub parent_session_id: SessionId,
    pub status: SubagentStatus,
    pub summary: String,
    pub result: Option<Value>,
    pub usage: UsageSnapshot,
    pub transcript_ref: Option<TranscriptRef>,
    pub context_report: Option<SubagentContextReport>,
    pub renderer_id: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubagentContextReport {
    pub parent_system_hash: Option<ContentHash>,
    pub child_system_hash: ContentHash,
    pub shared_system_prefix_hash: Option<ContentHash>,
    pub prompt_cache_prefix_reused: bool,
    pub bootstrap_files_inherited: Vec<String>,
    pub system_header_extra_applied: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubagentTerminatedEvent {
    pub subagent_id: SubagentId,
    pub parent_session_id: SessionId,
    pub reason: SubagentTerminationReason,
    pub final_usage: UsageSnapshot,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubagentStalledEvent {
    pub subagent_id: SubagentId,
    pub parent_session_id: SessionId,
    pub parent_run_id: RunId,
    pub last_activity_at: DateTime<Utc>,
    pub stalled_for: Duration,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubagentSpawnPausedEvent {
    pub tenant_id: TenantId,
    pub paused: bool,
    pub by: String,
    pub reason: Option<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubagentPermissionForwardedEvent {
    pub parent_session_id: SessionId,
    pub subagent_id: SubagentId,
    pub original_request_id: PermissionRequestId,
    pub subject: PermissionSubject,
    pub presented_options: Vec<Decision>,
    pub timeout_policy: Option<TimeoutPolicy>,
    pub forwarded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubagentPermissionResolvedEvent {
    pub parent_session_id: SessionId,
    pub subagent_id: SubagentId,
    pub original_request_id: PermissionRequestId,
    pub decision: Decision,
    pub decided_by: DecidedBy,
    pub at: DateTime<Utc>,
}

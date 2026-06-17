use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpToolInjectedEvent {
    pub session_id: SessionId,
    pub server_id: McpServerId,
    pub tool_name: String,
    pub upstream_name: String,
    pub defer_policy: DeferPolicy,
    pub filtered_out: bool,
    pub filter_reason: Option<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpConnectionLostEvent {
    pub session_id: Option<SessionId>,
    pub server_id: McpServerId,
    pub server_source: McpServerSource,
    pub reason: McpConnectionLostReason,
    pub attempts_so_far: u32,
    pub terminal: bool,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpConnectionRecoveredEvent {
    pub session_id: Option<SessionId>,
    pub server_id: McpServerId,
    pub server_source: McpServerSource,
    pub was_first: bool,
    pub total_downtime_ms: u64,
    pub attempts_used: u32,
    pub schema_changed: bool,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpOAuthRefreshEvent {
    pub server_id: McpServerId,
    pub transport: String,
    pub phase: McpOAuthRefreshPhase,
    pub outcome: McpOAuthRefreshOutcome,
    pub reason: Option<String>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpOAuthRefreshPhase {
    Started,
    Completed,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpOAuthRefreshOutcome {
    Started,
    Success,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpElicitationRequestedEvent {
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub server_id: McpServerId,
    pub request_id: RequestId,
    pub subject: String,
    pub schema_summary: ElicitationSchemaSummary,
    pub timeout: Option<Duration>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpElicitationResolvedEvent {
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub server_id: McpServerId,
    pub request_id: RequestId,
    pub outcome: ElicitationOutcome,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpToolsListChangedEvent {
    pub session_id: Option<SessionId>,
    pub server_id: McpServerId,
    pub received_at: DateTime<Utc>,
    pub pending_since: Option<DateTime<Utc>>,
    pub added_count: u32,
    pub removed_count: u32,
    pub disposition: ToolsListChangedDisposition,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpResourceUpdatedEvent {
    pub session_id: Option<SessionId>,
    pub server_id: McpServerId,
    pub kind: McpResourceUpdateKind,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpSamplingRequestedEvent {
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub server_id: McpServerId,
    pub request_id: RequestId,
    pub model_id: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub latency_ms: u64,
    pub outcome: SamplingOutcome,
    pub prompt_cache_namespace: String,
    pub at: DateTime<Utc>,
}

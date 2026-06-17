use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolUseRequestedEvent {
    pub run_id: RunId,
    pub tool_use_id: ToolUseId,
    pub tool_name: String,
    pub input: Value,
    pub properties: ToolProperties,
    pub causation_id: EventId,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolUseApprovedEvent {
    pub tool_use_id: ToolUseId,
    pub decision_id: DecisionId,
    pub scope: DecisionScope,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolUseDeniedEvent {
    pub tool_use_id: ToolUseId,
    pub reason: DenyReason,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolUseCompletedEvent {
    pub tool_use_id: ToolUseId,
    pub result: ToolResult,
    pub usage: Option<UsageSnapshot>,
    pub duration_ms: u64,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolUseFailedEvent {
    pub tool_use_id: ToolUseId,
    pub error: ToolErrorPayload,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolUseHeartbeatEvent {
    pub tool_use_id: ToolUseId,
    pub run_id: RunId,
    pub message: String,
    pub fraction: Option<f32>,
    pub silent_for_ms: u64,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolResultOffloadedEvent {
    pub tool_use_id: ToolUseId,
    pub run_id: RunId,
    pub blob_ref: BlobRef,
    pub original_metric: BudgetMetric,
    pub original_size: u64,
    pub effective_limit: u64,
    pub head_chars: u32,
    pub tail_chars: u32,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolRegistrationShadowedEvent {
    pub tool_name: String,
    pub kept: ToolOrigin,
    pub rejected: ToolOrigin,
    pub reason: ShadowReason,
    pub causation_id: Option<EventId>,
    pub at: DateTime<Utc>,
}

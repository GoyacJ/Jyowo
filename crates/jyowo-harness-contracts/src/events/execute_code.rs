use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExecuteCodeStepInvokedEvent {
    pub parent_tool_use_id: ToolUseId,
    pub run_id: RunId,
    pub session_id: SessionId,
    pub embedded_tool: String,
    pub args_hash: [u8; 32],
    pub step_seq: u32,
    pub duration_ms: u64,
    pub overflow: Option<OverflowMetadata>,
    pub refused_reason: Option<EmbeddedRefusedReason>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExecuteCodeWhitelistExtendedEvent {
    pub session_id: SessionId,
    pub added: Vec<String>,
    pub source: String,
    pub at: DateTime<Utc>,
}

use crate::*;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UserMessageAppendedEvent {
    pub run_id: RunId,
    pub message_id: MessageId,
    pub content: MessageContent,
    pub metadata: MessageMetadata,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AssistantDeltaProducedEvent {
    pub run_id: RunId,
    pub message_id: MessageId,
    pub delta: DeltaChunk,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AssistantMessageCompletedEvent {
    pub run_id: RunId,
    pub message_id: MessageId,
    pub content: MessageContent,
    pub tool_uses: Vec<ToolUseSummary>,
    pub usage: UsageSnapshot,
    pub pricing_snapshot_id: Option<PricingSnapshotId>,
    pub stop_reason: StopReason,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MessageMetadata {
    pub source: Option<String>,
    pub labels: std::collections::BTreeMap<String, String>,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeltaChunk {
    Text(String),
    Thought(ThoughtChunk),
    ToolUseStart {
        tool_use_id: ToolUseId,
        tool_name: String,
    },
    ToolUseInputDelta {
        tool_use_id: ToolUseId,
        delta: String,
    },
    ToolUseEnd {
        tool_use_id: ToolUseId,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ThoughtChunk {
    pub text: Option<String>,
    pub provider_id: String,
    pub provider_native: Option<serde_json::Value>,
    pub signature: Option<String>,
}

impl From<String> for ThoughtChunk {
    fn from(text: String) -> Self {
        Self {
            text: Some(text),
            provider_id: "legacy".to_owned(),
            provider_native: None,
            signature: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolUseSummary {
    pub tool_use_id: ToolUseId,
    pub tool_name: String,
}

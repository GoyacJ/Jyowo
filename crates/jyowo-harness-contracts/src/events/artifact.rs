use crate::*;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type ArtifactId = String;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactCreatedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub artifact_id: ArtifactId,
    pub title: String,
    pub kind: String,
    pub status: ArtifactStatus,
    pub source: ArtifactSource,
    pub source_message_id: Option<MessageId>,
    pub source_tool_use_id: Option<ToolUseId>,
    pub blob_ref: Option<BlobRef>,
    pub preview: Option<String>,
    pub content_hash: Option<Vec<u8>>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactUpdatedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub artifact_id: ArtifactId,
    pub title: Option<String>,
    pub kind: Option<String>,
    pub status: Option<ArtifactStatus>,
    pub source: ArtifactSource,
    pub source_message_id: Option<MessageId>,
    pub source_tool_use_id: Option<ToolUseId>,
    pub blob_ref: Option<BlobRef>,
    pub preview: Option<String>,
    pub content_hash: Option<Vec<u8>>,
    pub at: DateTime<Utc>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStatus {
    Pending,
    Running,
    Ready,
    Failed,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactSource {
    Assistant,
    Tool,
    File,
    ModelService,
}

//! Message, turn input, and tool result contracts.
//!
//! SPEC: docs/architecture/harness/crates/harness-contracts.md §3.5

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    BlobRef, JournalOffset, MemoryId, MessageId, ModelModality, ToolUseId, TranscriptRef,
    UsageSnapshot,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TurnInput {
    pub message: Message,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConversationContextReference {
    WorkspaceFile { path: String, label: String },
    Artifact { id: String, label: String },
    Conversation { id: String, label: String },
    Memory {
        id: String,
        label: String,
        /// Hydrated content, if resolved. Mutually exclusive with `label`-only rendering.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved_content: Option<String>,
    },
    Skill { id: String, label: String },
    Tool { id: String, label: String },
    McpServer { id: String, label: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConversationAttachmentReference {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub blob_ref: BlobRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConversationTurnInput {
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_message_id: Option<String>,
    #[serde(default)]
    pub context_references: Vec<ConversationContextReference>,
    #[serde(default)]
    pub attachments: Vec<ConversationAttachmentReference>,
}

impl ConversationTurnInput {
    #[must_use]
    pub fn ask(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            client_message_id: None,
            context_references: Vec::new(),
            attachments: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Message {
    pub id: MessageId,
    pub role: MessageRole,
    pub parts: Vec<MessagePart>,
    pub created_at: DateTime<Utc>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessageContent {
    Text(String),
    Structured(Value),
    Multimodal(Vec<MessagePart>),
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessagePart {
    Text(String),
    Image {
        mime_type: String,
        blob_ref: BlobRef,
    },
    Video {
        mime_type: String,
        blob_ref: BlobRef,
    },
    File {
        mime_type: String,
        blob_ref: BlobRef,
    },
    ToolUse {
        id: ToolUseId,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: ToolUseId,
        content: ToolResult,
    },
    Thinking(ThinkingBlock),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ThinkingBlock {
    pub text: Option<String>,
    pub provider_id: String,
    pub provider_native: Option<Value>,
    pub signature: Option<String>,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolResult {
    Text(String),
    Structured(Value),
    Blob {
        content_type: String,
        blob_ref: BlobRef,
    },
    Mixed(Vec<ToolResultPart>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolResultEnvelope {
    pub result: ToolResult,
    pub usage: Option<UsageSnapshot>,
    pub is_error: bool,
    pub overflow: Option<crate::OverflowMetadata>,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolResultPart {
    Text {
        text: String,
    },
    Structured {
        value: Value,
        schema_ref: Option<String>,
    },
    Blob {
        content_type: String,
        blob_ref: BlobRef,
        summary: Option<String>,
    },
    Code {
        language: String,
        text: String,
    },
    Reference {
        reference_kind: ReferenceKind,
        title: Option<String>,
        summary: Option<String>,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<Value>>,
        caption: Option<String>,
    },
    Progress {
        stage: String,
        ratio: Option<f32>,
        detail: Option<String>,
    },
    Error {
        code: String,
        message: String,
        retriable: bool,
    },
    Artifact {
        artifact_kind: ModelModality,
        content_type: String,
        blob_ref: BlobRef,
        title: String,
        preview: Option<String>,
    },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "ref_kind", rename_all = "snake_case")]
pub enum ReferenceKind {
    Url {
        url: String,
    },
    File {
        path: PathBuf,
        line_range: Option<(u32, u32)>,
    },
    Transcript(TranscriptRef),
    ToolUse {
        tool_use_id: ToolUseId,
    },
    Memory {
        memory_id: MemoryId,
    },
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct TranscriptRange {
    pub from_offset: JournalOffset,
    pub to_offset: JournalOffset,
}

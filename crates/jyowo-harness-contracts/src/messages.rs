//! Message, turn input, and tool result contracts.
//!

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::{
    BlobRef, JournalOffset, MemoryId, MessageId, ModelModality, SkillId, SkillSourceKind,
    ToolUseId, TranscriptRef, UsageSnapshot,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TurnInput {
    pub message: Message,
    pub metadata: Value,
}

pub const CURRENT_CONTEXT_REFERENCE_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[schemars(deny_unknown_fields)]
pub enum ConversationContextReference {
    WorkspaceFile {
        path: String,
        label: String,
    },
    Artifact {
        id: String,
        label: String,
    },
    Conversation {
        id: String,
        label: String,
    },
    Memory {
        id: String,
        label: String,
        /// Hydrated content, if resolved. Mutually exclusive with `label`-only rendering.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved_content: Option<String>,
    },
    Skill {
        #[serde(default = "current_context_reference_version")]
        #[schemars(extend("const" = CURRENT_CONTEXT_REFERENCE_VERSION))]
        version: u16,
        #[serde(rename = "skillId", alias = "id", alias = "skill_id")]
        skill_id: SkillId,
        label: String,
        #[serde(default)]
        parameters: BTreeMap<String, Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<SkillSourceKind>,
    },
    Tool {
        id: String,
        label: String,
    },
    McpServer {
        id: String,
        label: String,
    },
}

const fn current_context_reference_version() -> u16 {
    CURRENT_CONTEXT_REFERENCE_VERSION
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum TypedConversationContextReference {
    WorkspaceFile {
        path: String,
        label: String,
    },
    Artifact {
        id: String,
        label: String,
    },
    Conversation {
        id: String,
        label: String,
    },
    Memory {
        id: String,
        label: String,
        #[serde(default)]
        resolved_content: Option<String>,
    },
    Skill {
        #[serde(default = "current_context_reference_version")]
        version: u16,
        #[serde(rename = "skillId", alias = "id", alias = "skill_id")]
        skill_id: SkillId,
        label: String,
        #[serde(default)]
        parameters: BTreeMap<String, Value>,
        #[serde(default)]
        source: Option<SkillSourceKind>,
    },
    Tool {
        id: String,
        label: String,
    },
    McpServer {
        id: String,
        label: String,
    },
}

impl From<TypedConversationContextReference> for ConversationContextReference {
    fn from(reference: TypedConversationContextReference) -> Self {
        match reference {
            TypedConversationContextReference::WorkspaceFile { path, label } => {
                Self::WorkspaceFile { path, label }
            }
            TypedConversationContextReference::Artifact { id, label } => {
                Self::Artifact { id, label }
            }
            TypedConversationContextReference::Conversation { id, label } => {
                Self::Conversation { id, label }
            }
            TypedConversationContextReference::Memory {
                id,
                label,
                resolved_content,
            } => Self::Memory {
                id,
                label,
                resolved_content,
            },
            TypedConversationContextReference::Skill {
                version,
                skill_id,
                label,
                parameters,
                source,
            } => Self::Skill {
                version,
                skill_id,
                label,
                parameters,
                source,
            },
            TypedConversationContextReference::Tool { id, label } => Self::Tool { id, label },
            TypedConversationContextReference::McpServer { id, label } => {
                Self::McpServer { id, label }
            }
        }
    }
}

impl<'de> Deserialize<'de> for ConversationContextReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum CompatibleReference {
            LegacyWorkspaceFile(String),
            Typed(TypedConversationContextReference),
        }

        match CompatibleReference::deserialize(deserializer)? {
            CompatibleReference::LegacyWorkspaceFile(path) => Ok(Self::WorkspaceFile {
                label: path.clone(),
                path,
            }),
            CompatibleReference::Typed(TypedConversationContextReference::Skill {
                version,
                ..
            }) if version != CURRENT_CONTEXT_REFERENCE_VERSION => Err(D::Error::custom(format!(
                "unsupported skill context reference version {version}"
            ))),
            CompatibleReference::Typed(reference) => Ok(reference.into()),
        }
    }
}

impl From<String> for ConversationContextReference {
    fn from(path: String) -> Self {
        Self::WorkspaceFile {
            label: path.clone(),
            path,
        }
    }
}

impl From<&str> for ConversationContextReference {
    fn from(path: &str) -> Self {
        path.to_owned().into()
    }
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
    ProviderFileReference {
        provider_id: String,
        file_id: String,
        mime_type: String,
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

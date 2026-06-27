//! Conversation read model contracts.
//!
//! These types are the stable UI-facing query surface. They are projections of
//! redacted journal events, not replacements for the durable journal facts.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ArtifactSource, ArtifactStatus, EventId, RedactRules, Redactor};

const REDACTED: &str = "[REDACTED]";

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct UiSafeText(String);

impl UiSafeText {
    #[must_use]
    pub fn from_trusted_redacted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn from_redacted_display(value: impl AsRef<str>, redactor: &dyn Redactor) -> Self {
        let redacted = redactor.redact(value.as_ref(), &RedactRules::default());
        if contains_private_absolute_path(&redacted) || contains_obvious_secret(&redacted) {
            return Self(REDACTED.to_owned());
        }
        Self(redacted)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<UiSafeText> for String {
    fn from(value: UiSafeText) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationCursor {
    pub event_id: EventId,
    pub conversation_sequence: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummary {
    pub id: String,
    pub is_empty: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message_preview: Option<UiSafeText>,
    pub title: UiSafeText,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_config_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ConversationCursor>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConversationMessageAuthor {
    User,
    Assistant,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    pub author: ConversationMessageAuthor,
    pub body: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_message_id: Option<String>,
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub conversation_sequence: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSnapshot {
    pub id: String,
    pub messages: Vec<ConversationMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_config_id: Option<String>,
    pub title: UiSafeText,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ConversationCursor>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTimelineEvent {
    pub id: String,
    pub cursor: ConversationCursor,
    pub payload: Value,
    pub run_id: String,
    pub sequence: u64,
    pub source: String,
    pub timestamp: DateTime<Utc>,
    #[serde(rename = "type")]
    pub event_type: String,
    pub visibility: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTimelinePage {
    pub events: Vec<ConversationTimelineEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<ConversationCursor>,
    pub gap: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationWorktreePage {
    pub turns: Vec<ConversationTurn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_cursor: Option<ConversationTurnCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_cursor: Option<ConversationCursor>,
    pub has_more_before: bool,
    pub has_more_after: bool,
    pub gap: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurnCursor {
    pub turn_id: String,
    pub position: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurn {
    pub id: String,
    pub conversation_id: String,
    pub position: u64,
    pub user: ConversationTurnUserMessage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant: Option<AssistantWork>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurnUserMessage {
    pub id: String,
    pub message_id: String,
    pub body: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_message_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssistantWork {
    pub id: String,
    pub run_id: String,
    pub status: AssistantWorkStatus,
    pub segments: Vec<AssistantSegment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AssistantWorkStatus {
    Running,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AssistantSegment {
    Process(ProcessSegment),
    Thinking(ThinkingSegment),
    Text(TextSegment),
    ToolGroup(ToolGroupSegment),
    Artifact(ArtifactSegment),
    ReviewRequest(ReviewRequestSegment),
    ClarificationRequest(ClarificationRequestSegment),
    Notice(NoticeSegment),
    Error(ErrorSegment),
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSegment {
    pub id: String,
    pub order: u32,
    pub status: ProcessSegmentStatus,
    pub summary: UiSafeText,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<ProcessStep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ProcessSegmentStatus {
    Running,
    Complete,
    Failed,
    Cancelled,
    Withheld,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessStep {
    pub id: String,
    pub order: u32,
    pub kind: ProcessStepKind,
    pub status: ProcessStepStatus,
    pub title: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<UiSafeText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<ProcessStepDetail>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ProcessStepKind {
    Reasoning,
    Activity,
    Command,
    FileRead,
    FileSearch,
    FileEdit,
    Diff,
    Tool,
    Artifact,
    Synthesis,
    Withheld,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ProcessStepStatus {
    Running,
    Complete,
    Failed,
    Withheld,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProcessStepDetail {
    Activity {
        summary: UiSafeText,
        #[serde(rename = "itemCount", default, skip_serializing_if = "Option::is_none")]
        item_count: Option<u32>,
    },
    Command {
        command: UiSafeText,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<UiSafeText>,
        #[serde(rename = "exitCode", default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(
            rename = "durationMs",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        duration_ms: Option<u64>,
    },
    Diff {
        files: Vec<ProcessDiffFile>,
    },
    Tool {
        #[serde(rename = "toolName")]
        tool_name: UiSafeText,
        #[serde(
            rename = "outputSummary",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        output_summary: Option<UiSafeText>,
        #[serde(
            rename = "durationMs",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        duration_ms: Option<u64>,
    },
    Artifact {
        #[serde(rename = "artifactId")]
        artifact_id: String,
        media: ArtifactMediaPreview,
    },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessDiffFile {
    pub path: UiSafeText,
    pub added_lines: u32,
    pub removed_lines: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<UiSafeText>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingSegment {
    pub id: String,
    pub order: u32,
    pub status: ThinkingSegmentStatus,
    pub summary: ThinkingSummary,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<ThinkingStep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ThinkingSegmentStatus {
    Running,
    Complete,
    Withheld,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingSummary {
    pub text: UiSafeText,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingStep {
    pub id: String,
    pub order: u32,
    pub kind: ThinkingStepKind,
    pub status: ThinkingStepStatus,
    pub title: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<UiSafeText>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ThinkingStepKind {
    Status,
    ReasoningSummary,
    ToolPlanning,
    ToolResult,
    Synthesis,
    Withheld,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ThinkingStepStatus {
    Running,
    Complete,
    Failed,
    Withheld,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextSegment {
    pub id: String,
    pub order: u32,
    pub message_id: String,
    pub body: UiSafeText,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolGroupSegment {
    pub id: String,
    pub order: u32,
    pub attempts: Vec<ToolAttempt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolAttempt {
    pub id: String,
    pub order: u32,
    pub tool_use_id: String,
    pub tool_name: UiSafeText,
    pub status: ToolAttemptStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<ToolPermissionState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_summary: Option<UiSafeText>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ToolAttemptStatus {
    Queued,
    WaitingPermission,
    Running,
    Completed,
    Failed,
    Denied,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolPermissionState {
    pub id: String,
    pub request_id: String,
    pub tool_use_id: String,
    pub status: ToolPermissionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<UiSafeText>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ToolPermissionStatus {
    Pending,
    Submitting,
    Approved,
    Denied,
    Failed,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSegment {
    pub id: String,
    pub order: u32,
    pub artifact_id: String,
    #[serde(rename = "artifactKind", default = "default_artifact_kind")]
    pub kind: String,
    #[serde(default = "default_artifact_segment_status")]
    pub status: ArtifactStatus,
    #[serde(default = "default_artifact_segment_source")]
    pub source: ArtifactSource,
    pub title: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<UiSafeText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<ArtifactMediaPreview>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactMediaPreview {
    pub kind: ArtifactMediaKind,
    pub mime_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactMediaKind {
    Image,
    Video,
    Audio,
    File,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRequestSegment {
    pub id: String,
    pub order: u32,
    pub request_id: String,
    pub title: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<UiSafeText>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClarificationRequestSegment {
    pub id: String,
    pub order: u32,
    pub request_id: String,
    pub prompt: UiSafeText,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoticeSegment {
    pub id: String,
    pub order: u32,
    pub body: UiSafeText,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorSegment {
    pub id: String,
    pub order: u32,
    pub body: UiSafeText,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationEventRef {
    pub event_id: String,
    pub cursor: ConversationCursor,
}

fn default_artifact_kind() -> String {
    "file".to_owned()
}

fn default_artifact_segment_status() -> ArtifactStatus {
    ArtifactStatus::Ready
}

fn default_artifact_segment_source() -> ArtifactSource {
    ArtifactSource::Assistant
}

fn contains_private_absolute_path(value: &str) -> bool {
    value.contains("/Users/")
        || value.contains("/home/")
        || value.contains("/private/var/")
        || contains_windows_absolute_path(value)
}

fn contains_windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.windows(3).any(|window| {
        window[0].is_ascii_alphabetic() && window[1] == b':' && matches!(window[2], b'\\' | b'/')
    })
}

fn contains_obvious_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("authorization: bearer ")
        || lower.contains("authorization bearer ")
        || lower.contains("authorization: basic ")
        || contains_auth_token(&lower, "bearer")
        || contains_auth_token(&lower, "basic")
        || contains_jwt_like_token(value)
        || contains_secret_assignment(&lower)
        || lower.contains("api_key")
        || lower.contains("api-key")
        || lower.contains("token=")
        || lower.contains("secret=")
        || lower.contains("password=")
        || lower.contains("sk-")
        || lower.contains("ghp_")
        || lower.contains("gho_")
        || lower.contains("ghu_")
        || lower.contains("ghs_")
        || lower.contains("ghr_")
        || lower.contains("github_pat_")
        || lower.contains("xoxb-")
        || lower.contains("xoxp-")
        || lower.contains("xoxa-")
        || lower.contains("xoxr-")
        || lower.contains("xoxs-")
        || lower.contains("npm_")
        || lower.contains("lin_api_")
        || lower.contains("secret_")
        || lower.contains("sk_live_")
        || lower.contains("rk_live_")
        || contains_database_url(&lower)
        || lower.contains("-----begin ")
        || value.contains("AKIA")
        || contains_aws_key_like(value)
        || value.contains("AIza")
}

fn contains_auth_token(value: &str, scheme: &str) -> bool {
    value.split_ascii_whitespace().any(|token| token == scheme)
        && value
            .split_ascii_whitespace()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair[0] == scheme && pair[1].len() >= 12)
}

fn contains_jwt_like_token(value: &str) -> bool {
    value.split_ascii_whitespace().any(|token| {
        let token = trim_secret_token_wrapper(token);
        if !token.starts_with("eyJ") {
            return false;
        }
        let parts = token.split('.').collect::<Vec<_>>();
        parts.len() >= 3 && parts[0].len() >= 8 && parts[1].len() >= 8 && parts[2].len() >= 8
    })
}

fn trim_secret_token_wrapper(value: &str) -> &str {
    value.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | ';' | ':' | ')' | '(' | '[' | ']' | '<' | '>' | '"' | '\''
        )
    })
}

fn contains_database_url(value: &str) -> bool {
    [
        "postgres://",
        "postgresql://",
        "mysql://",
        "mongodb://",
        "mongodb+srv://",
        "redis://",
        "amqp://",
        "amqps://",
    ]
    .iter()
    .any(|scheme| {
        value.find(scheme).is_some_and(|start| {
            let url = value[start..]
                .split_ascii_whitespace()
                .next()
                .unwrap_or_default();
            url.contains('@')
        })
    })
}

fn contains_aws_key_like(value: &str) -> bool {
    value.split_ascii_whitespace().any(|token| {
        let token = trim_secret_token_wrapper(token);
        (token.starts_with("ASIA") || token.starts_with("A3T"))
            && token.len() >= 20
            && token
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
    })
}

fn contains_secret_assignment(value: &str) -> bool {
    [
        "password",
        "passwd",
        "pwd",
        "token",
        "secret",
        "client_secret",
        "refresh_token",
        "access_token",
        "oauth_code",
        "code",
    ]
    .iter()
    .any(|name| contains_secret_assignment_name(value, name))
}

fn contains_secret_assignment_name(value: &str, name: &str) -> bool {
    let mut cursor = 0;
    while cursor < value.len() {
        let Some(relative_start) = value[cursor..].find(name) else {
            return false;
        };
        let start = cursor + relative_start;
        let mut offset = start + name.len();
        offset += ascii_whitespace_prefix_len(&value[offset..]);
        let Some(delimiter) = value[offset..].chars().next() else {
            return false;
        };
        if !matches!(delimiter, ':' | '=') {
            cursor = offset + delimiter.len_utf8();
            continue;
        }
        offset += delimiter.len_utf8();
        offset += ascii_whitespace_prefix_len(&value[offset..]);
        if let Some(quote) = value[offset..]
            .chars()
            .next()
            .filter(|ch| matches!(*ch, '"' | '\''))
        {
            offset += quote.len_utf8();
            let quoted_value_len = value[offset..].find(quote).unwrap_or(value[offset..].len());
            if quoted_value_len >= 8 {
                return true;
            }
            cursor = offset + quoted_value_len;
            continue;
        }
        let value_len = value[offset..]
            .char_indices()
            .take_while(|(_, ch)| !ch.is_ascii_whitespace() && !matches!(*ch, '"' | '\''))
            .last()
            .map_or(0, |(index, ch)| index + ch.len_utf8());
        if value_len >= 8 {
            return true;
        }
        cursor = offset + value_len;
    }
    false
}

fn ascii_whitespace_prefix_len(value: &str) -> usize {
    value
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_whitespace())
        .last()
        .map_or(0, |(index, ch)| index + ch.len_utf8())
}

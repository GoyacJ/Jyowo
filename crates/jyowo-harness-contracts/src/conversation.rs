//! Conversation read model contracts.
//!
//! These types are the stable UI-facing query surface. They are projections of
//! redacted journal events, not replacements for the durable journal facts.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{EventId, RedactRules, Redactor};

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

fn contains_private_absolute_path(value: &str) -> bool {
    value.contains("/Users/")
        || value.contains("/home/")
        || value.contains("/private/var/")
        || contains_windows_absolute_path(value)
}

fn contains_windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes
        .windows(3)
        .any(|window| window[0].is_ascii_alphabetic() && window[1] == b':' && window[2] == b'\\')
}

fn contains_obvious_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("authorization: bearer ")
        || lower.contains("authorization bearer ")
        || lower.contains("authorization: basic ")
        || lower.contains("api_key")
        || lower.contains("api-key")
        || lower.contains("token=")
        || lower.contains("secret=")
        || lower.contains("password=")
        || lower.contains("sk-")
        || value.contains("AKIA")
}

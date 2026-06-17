use std::collections::BTreeSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use harness_contracts::{
    MemoryActor, MemoryId, MemoryKind, MemorySource, MemoryVisibility, SessionId, TenantId,
};
use serde::{Deserialize, Serialize};

pub const MEMORY_CONTENT_PREVIEW_MAX_CHARS: usize = 160;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryQuery {
    pub text: String,
    pub kind_filter: Option<MemoryKindFilter>,
    pub visibility_filter: MemoryVisibilityFilter,
    pub max_records: u32,
    pub min_similarity: f32,
    pub tenant_id: TenantId,
    pub session_id: Option<SessionId>,
    pub deadline: Option<Duration>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKindFilter {
    Any,
    OnlyKinds(BTreeSet<MemoryKind>),
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryVisibilityFilter {
    EffectiveFor(MemoryActor),
    Exact(MemoryVisibility),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: MemoryId,
    pub tenant_id: TenantId,
    pub kind: MemoryKind,
    pub visibility: MemoryVisibility,
    pub content: String,
    pub metadata: MemoryMetadata,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemorySummary {
    pub id: MemoryId,
    pub kind: MemoryKind,
    pub visibility: MemoryVisibility,
    pub content_preview: String,
    pub metadata: MemoryMetadata,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryListScope {
    All,
    ByKind(MemoryKind),
    ByVisibility(MemoryVisibility),
    ForActor(MemoryActor),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryMetadata {
    pub tags: Vec<String>,
    pub source: MemorySource,
    pub confidence: f32,
    pub access_count: u32,
    pub last_accessed_at: Option<DateTime<Utc>>,
    pub recall_score: f32,
    pub ttl: Option<Duration>,
    pub redacted_segments: u32,
}

#[must_use]
pub fn content_preview(content: &str) -> String {
    let char_count = content.chars().count();
    if char_count <= MEMORY_CONTENT_PREVIEW_MAX_CHARS {
        return content.to_owned();
    }

    let keep = MEMORY_CONTENT_PREVIEW_MAX_CHARS.saturating_sub(3);
    let mut preview = content.chars().take(keep).collect::<String>();
    preview.push_str("...");
    preview
}

pub fn visibility_matches(visibility: &MemoryVisibility, actor: &MemoryActor) -> bool {
    match visibility {
        MemoryVisibility::Private { session_id } => actor.session_id.as_ref() == Some(session_id),
        MemoryVisibility::User { user_id } => actor.user_id.as_deref() == Some(user_id),
        MemoryVisibility::Team { team_id } => actor.team_id.as_ref() == Some(team_id),
        MemoryVisibility::Tenant => true,
        _ => false,
    }
}

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryUpsertedEvent {
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub memory_id: MemoryId,
    pub kind: MemoryKind,
    pub visibility: MemoryVisibility,
    pub action: MemoryWriteAction,
    pub provider_id: String,
    pub source: MemorySource,
    pub content_hash: ContentHash,
    pub bytes_written: u64,
    pub takes_effect: TakesEffect,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRecalledEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub turn: u32,
    pub provider_id: String,
    pub query_text_hash: ContentHash,
    pub returned_count: u32,
    pub kept_count: u32,
    pub injected_chars: u32,
    pub deadline_used_ms: u32,
    pub min_similarity: f32,
    pub kinds_returned: Vec<MemoryKind>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRecallDegradedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub turn: u32,
    pub provider_id: String,
    pub reason: MemoryRecallDegradedReason,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRecallSkippedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub turn: u32,
    pub reason: RecallSkipReason,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryThreatDetectedEvent {
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub pattern_id: String,
    pub category: ThreatCategory,
    pub severity: Severity,
    pub action: ThreatAction,
    pub direction: ThreatDirection,
    pub provider_id: Option<String>,
    pub content_hash: ContentHash,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemdirOverflowEvent {
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub file: MemdirFileTag,
    pub current_chars: u64,
    pub threshold: u64,
    pub strategy_applied: OverflowStrategy,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryConsolidationRanEvent {
    pub session_id: SessionId,
    pub hook_id: String,
    pub promoted: Vec<MemoryId>,
    pub demoted: Vec<MemoryId>,
    pub draft_dreams_chars: u32,
    pub duration_ms: u32,
    pub at: DateTime<Utc>,
}

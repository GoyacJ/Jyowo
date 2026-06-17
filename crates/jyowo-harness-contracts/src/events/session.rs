use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SessionCreatedEvent {
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub options_hash: [u8; 32],
    pub snapshot_id: SnapshotId,
    pub effective_config_hash: ConfigHash,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SessionForkedEvent {
    pub parent_session_id: SessionId,
    pub child_session_id: SessionId,
    pub tenant_id: TenantId,
    pub fork_reason: ForkReason,
    pub from_offset: JournalOffset,
    pub config_delta_hash: Option<DeltaHash>,
    pub cache_impact: CacheImpact,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SessionEndedEvent {
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub reason: EndReason,
    pub final_usage: UsageSnapshot,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SessionReloadRequestedEvent {
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub reason: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SessionReloadAppliedEvent {
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub new_snapshot_id: SnapshotId,
    pub effective_config_hash: ConfigHash,
    pub cache_impact: CacheImpact,
    pub at: DateTime<Utc>,
}

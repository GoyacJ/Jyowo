use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunStartedEvent {
    pub run_id: RunId,
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub parent_run_id: Option<RunId>,
    pub input: TurnInput,
    pub snapshot_id: SnapshotId,
    pub effective_config_hash: ConfigHash,
    pub started_at: DateTime<Utc>,
    pub correlation_id: CorrelationId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunEndedEvent {
    pub run_id: RunId,
    pub reason: EndReason,
    pub usage: Option<UsageSnapshot>,
    pub ended_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GraceCallTriggeredEvent {
    pub run_id: RunId,
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub current_iteration: u32,
    pub max_iterations: u32,
    pub usage_snapshot: UsageSnapshot,
    pub at: DateTime<Utc>,
    pub correlation_id: CorrelationId,
}

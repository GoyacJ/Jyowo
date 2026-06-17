use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UsageAccumulatedEvent {
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub delta: UsageSnapshot,
    pub model_ref: Option<ModelRef>,
    pub pricing_snapshot_id: Option<PricingSnapshotId>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TraceSpanCompletedEvent {
    pub trace_id: String,
    pub span_id: String,
    pub name: String,
    pub duration_ms: u64,
    pub at: DateTime<Utc>,
}

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CompactionAppliedEvent {
    pub session_id: SessionId,
    pub strategy: CompactStrategyId,
    pub trigger: CompactTrigger,
    pub outcome: CompactOutcome,
    pub before_tokens: u64,
    pub after_tokens: u64,
    pub summary_ref: BlobRef,
    pub child_session_id: Option<SessionId>,
    pub handoff: Option<CompactionHandoff>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContextStageTransitionedEvent {
    pub session_id: SessionId,
    pub stage: ContextStageId,
    pub provider_id: String,
    pub outcome: ContextStageOutcome,
    pub before_tokens: u64,
    pub after_tokens: u64,
    pub bytes_saved: u64,
    pub duration_ms: u32,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContextBudgetExceededEvent {
    pub session_id: SessionId,
    pub budget_kind: BudgetKind,
    pub source: BudgetExceedanceSource,
    pub requested: u64,
    pub max: u64,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContextPatchAppliedEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub source: ContextPatchSource,
    pub lifecycle: ContextPatchLifecycle,
    pub body_bytes: u64,
    pub at: DateTime<Utc>,
}

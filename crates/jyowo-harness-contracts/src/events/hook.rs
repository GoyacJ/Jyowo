use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HookTriggeredEvent {
    pub hook_event_kind: HookEventKind,
    pub handler_id: HandlerId,
    pub outcome_summary: HookOutcomeSummary,
    pub duration_ms: u64,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HookRewroteInputEvent {
    pub tool_use_id: ToolUseId,
    pub before_hash: [u8; 32],
    pub after_hash: [u8; 32],
    pub causation_id: EventId,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HookContextPatchEvent {
    pub hook_event_kind: HookEventKind,
    pub handler_id: HandlerId,
    pub context_blob: Option<BlobRef>,
    pub byte_size: u64,
    pub causation_id: EventId,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HookFailedEvent {
    pub hook_event_kind: HookEventKind,
    pub handler_id: HandlerId,
    pub failure_mode: HookFailureMode,
    pub cause_kind: HookFailureCauseKind,
    pub cause_detail: String,
    pub duration_ms: u64,
    pub fail_closed_denied: Option<EventId>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HookReturnedUnsupportedEvent {
    pub hook_event_kind: HookEventKind,
    pub handler_id: HandlerId,
    pub returned_kind: HookOutcomeDiscriminant,
    pub causation_id: EventId,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HookOutcomeInconsistentEvent {
    pub hook_event_kind: HookEventKind,
    pub handler_id: HandlerId,
    pub reason: InconsistentReason,
    pub causation_id: EventId,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HookPanickedEvent {
    pub hook_event_kind: HookEventKind,
    pub handler_id: HandlerId,
    pub message_snippet: String,
    pub causation_id: EventId,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HookPermissionConflictEvent {
    pub hook_event_kind: HookEventKind,
    pub priority: i32,
    pub participants: Vec<HookPermissionConflictParticipant>,
    pub winner: HookPermissionConflictParticipant,
    pub resolved_event_id: EventId,
    pub at: DateTime<Utc>,
}

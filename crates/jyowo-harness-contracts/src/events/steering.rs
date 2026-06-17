use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SteeringMessageQueuedEvent {
    pub id: SteeringId,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub kind: SteeringKind,
    pub priority: SteeringPriority,
    pub source: SteeringSource,
    pub body_hash: [u8; 32],
    pub body_size: u32,
    pub body_blob: Option<BlobRef>,
    pub correlation_id: Option<CorrelationId>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SteeringMessageAppliedEvent {
    pub ids: Vec<SteeringId>,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub merged_into_message_id: Option<MessageId>,
    pub kind_distribution: BTreeMap<SteeringKind, u32>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SteeringMessageDroppedEvent {
    pub id: SteeringId,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub reason: SteeringDropReason,
    pub at: DateTime<Utc>,
}

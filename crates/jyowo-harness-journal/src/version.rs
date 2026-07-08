use chrono::{DateTime, Utc};
use harness_contracts::{
    CausationId, CorrelationId, Event, EventId, JournalOffset, RunId, SessionId, TenantId,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventEnvelope {
    pub offset: JournalOffset,
    pub event_id: EventId,
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub run_id: Option<RunId>,
    pub correlation_id: CorrelationId,
    pub causation_id: Option<CausationId>,
    pub recorded_at: DateTime<Utc>,
    pub payload: Event,
}

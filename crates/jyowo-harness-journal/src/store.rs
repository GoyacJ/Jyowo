//! `EventStore` trait and redaction adapter.
//!

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::stream::BoxStream;
use futures::StreamExt;
use harness_contracts::{
    CausationId, CorrelationId, EndReason, Event, EventId, ForkReason, JournalError, JournalOffset,
    RedactRules, Redactor, RunId, SessionId, SnapshotId, TenantId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{EventEnvelope, PrunePolicy, PruneReport, SessionSnapshot};

pub type EventStream = BoxStream<'static, Event>;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct AppendMetadata {
    pub run_id: Option<RunId>,
    pub correlation_id: CorrelationId,
    pub causation_id: Option<CausationId>,
}

impl Default for AppendMetadata {
    fn default() -> Self {
        Self {
            run_id: None,
            correlation_id: CorrelationId::new(),
            causation_id: None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayCursor {
    FromStart,
    FromOffset(JournalOffset),
    FromSnapshot(SnapshotId),
    FromTimestamp(DateTime<Utc>),
    Tail { since: Duration },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionFilter {
    pub since: Option<DateTime<Utc>>,
    pub end_reason: Option<EndReason>,
    pub project_compression_tips: bool,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: SessionId,
    pub created_at: DateTime<Utc>,
    pub last_event_at: DateTime<Utc>,
    pub event_count: u64,
    pub end_reason: Option<EndReason>,
    pub root_session: Option<SessionId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelopePage {
    pub envelopes: Vec<EventEnvelope>,
    pub next_event_id: Option<EventId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionLineage {
    pub parent_session: SessionId,
    pub child_session: SessionId,
    pub reason: ForkReason,
    pub linked_at: DateTime<Utc>,
}

#[async_trait]
pub trait EventStore: Send + Sync + 'static {
    async fn append(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError>;

    async fn append_with_metadata(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        metadata: AppendMetadata,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        let _ = metadata;
        self.append(tenant, session_id, events).await
    }

    async fn read(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, Event>, JournalError> {
        Ok(Box::pin(
            self.read_envelopes(tenant, session_id, cursor)
                .await?
                .map(|envelope| envelope.payload),
        ))
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError>;

    async fn page_session_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        after_event_id: Option<EventId>,
        limit: usize,
    ) -> Result<EventEnvelopePage, JournalError> {
        let mut envelopes = self
            .read_envelopes(tenant, session_id, ReplayCursor::FromStart)
            .await?
            .collect::<Vec<_>>()
            .await;
        apply_event_id_cursor(&mut envelopes, after_event_id)?;
        let limit = limit.clamp(1, 200);
        envelopes.truncate(limit);
        let next_event_id = envelopes.last().map(|envelope| envelope.event_id);
        Ok(EventEnvelopePage {
            envelopes,
            next_event_id,
        })
    }

    async fn query_after(
        &self,
        tenant: TenantId,
        after: Option<EventId>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError>;

    async fn snapshot(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, JournalError>;

    async fn save_snapshot(
        &self,
        tenant: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<(), JournalError>;

    async fn compact_link(
        &self,
        parent: SessionId,
        child: SessionId,
        reason: ForkReason,
    ) -> Result<(), JournalError>;

    async fn delete_session(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<bool, JournalError>;

    async fn list_sessions(
        &self,
        tenant: TenantId,
        filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, JournalError>;

    async fn prune(
        &self,
        tenant: TenantId,
        policy: PrunePolicy,
    ) -> Result<PruneReport, JournalError>;

    async fn prune_sessions(
        &self,
        tenant: TenantId,
        session_ids: &[SessionId],
        keep_snapshots: bool,
    ) -> Result<PruneReport, JournalError> {
        let _ = (tenant, session_ids, keep_snapshots);
        Err(journal_error(
            "exact-session prune is not supported by this event store",
        ))
    }
}

#[async_trait]
impl<T> EventStore for Arc<T>
where
    T: EventStore + ?Sized,
{
    async fn append(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        self.as_ref().append(tenant, session_id, events).await
    }

    async fn append_with_metadata(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        metadata: AppendMetadata,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        self.as_ref()
            .append_with_metadata(tenant, session_id, metadata, events)
            .await
    }

    async fn read(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, Event>, JournalError> {
        self.as_ref().read(tenant, session_id, cursor).await
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError> {
        self.as_ref()
            .read_envelopes(tenant, session_id, cursor)
            .await
    }

    async fn page_session_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        after_event_id: Option<EventId>,
        limit: usize,
    ) -> Result<EventEnvelopePage, JournalError> {
        self.as_ref()
            .page_session_envelopes(tenant, session_id, after_event_id, limit)
            .await
    }

    async fn query_after(
        &self,
        tenant: TenantId,
        after: Option<EventId>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        self.as_ref().query_after(tenant, after, limit).await
    }

    async fn snapshot(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, JournalError> {
        self.as_ref().snapshot(tenant, session_id).await
    }

    async fn save_snapshot(
        &self,
        tenant: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<(), JournalError> {
        self.as_ref().save_snapshot(tenant, snapshot).await
    }

    async fn compact_link(
        &self,
        parent: SessionId,
        child: SessionId,
        reason: ForkReason,
    ) -> Result<(), JournalError> {
        self.as_ref().compact_link(parent, child, reason).await
    }

    async fn delete_session(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<bool, JournalError> {
        self.as_ref().delete_session(tenant, session_id).await
    }

    async fn list_sessions(
        &self,
        tenant: TenantId,
        filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, JournalError> {
        self.as_ref().list_sessions(tenant, filter).await
    }

    async fn prune(
        &self,
        tenant: TenantId,
        policy: PrunePolicy,
    ) -> Result<PruneReport, JournalError> {
        self.as_ref().prune(tenant, policy).await
    }

    async fn prune_sessions(
        &self,
        tenant: TenantId,
        session_ids: &[SessionId],
        keep_snapshots: bool,
    ) -> Result<PruneReport, JournalError> {
        self.as_ref()
            .prune_sessions(tenant, session_ids, keep_snapshots)
            .await
    }
}

#[derive(Clone)]
pub struct JournalRedaction {
    redactor: Arc<dyn Redactor>,
}

impl JournalRedaction {
    pub fn new(redactor: Arc<dyn Redactor>) -> Self {
        Self { redactor }
    }

    pub fn redact_event_field(&self, value: &str) -> String {
        self.redactor.redact(value, &RedactRules::default())
    }

    pub fn redact_event(&self, event: &Event) -> Result<Event, JournalError> {
        let mut value = serde_json::to_value(event).map_err(journal_error)?;
        redact_value(self, &mut value);
        serde_json::from_value(value).map_err(journal_error)
    }

    pub fn redactor(&self) -> &Arc<dyn Redactor> {
        &self.redactor
    }
}

pub(crate) fn journal_error(error: impl std::fmt::Display) -> JournalError {
    JournalError::Message(error.to_string())
}

#[allow(dead_code)]
pub(crate) fn event_type(event: &Event) -> Result<String, JournalError> {
    let value = serde_json::to_value(event).map_err(journal_error)?;
    value
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| journal_error("event type missing"))
}

#[allow(dead_code)]
pub(crate) fn session_end_reason(event: &Event) -> Option<(SessionId, EndReason)> {
    match event {
        Event::SessionEnded(event) => Some((event.session_id, event.reason.clone())),
        _ => None,
    }
}

fn redact_value(redaction: &JournalRedaction, value: &mut Value) {
    match value {
        Value::String(text) => *text = redaction.redact_event_field(text),
        Value::Array(items) => {
            for item in items {
                redact_value(redaction, item);
            }
        }
        Value::Object(fields) => {
            for item in fields.values_mut() {
                redact_value(redaction, item);
            }
        }
        _ => {}
    }
}

#[allow(dead_code)]
pub(crate) fn apply_cursor(events: &mut Vec<EventEnvelope>, cursor: ReplayCursor) {
    match cursor {
        ReplayCursor::FromStart | ReplayCursor::FromSnapshot(_) => {}
        ReplayCursor::FromOffset(offset) => events.retain(|event| event.offset.0 > offset.0),
        ReplayCursor::FromTimestamp(timestamp) => {
            events.retain(|event| event.recorded_at >= timestamp);
        }
        ReplayCursor::Tail { since } => {
            let cutoff = harness_contracts::now()
                - chrono::Duration::from_std(since).unwrap_or_else(|_| chrono::Duration::zero());
            events.retain(|event| event.recorded_at >= cutoff);
        }
    }
}

pub(crate) fn apply_event_id_cursor(
    events: &mut Vec<EventEnvelope>,
    after_event_id: Option<EventId>,
) -> Result<(), JournalError> {
    let Some(after_event_id) = after_event_id else {
        return Ok(());
    };
    let Some(position) = events
        .iter()
        .position(|envelope| envelope.event_id == after_event_id)
    else {
        return Err(journal_error("conversation cursor is unknown"));
    };
    events.drain(0..=position).for_each(drop);
    Ok(())
}

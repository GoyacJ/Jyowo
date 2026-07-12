//! Legacy engine `EventStore` adapter backed by the unified task log.

use std::io::{self, Write};
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use harness_contracts::{
    Event, EventId, ForkReason, JournalError, JournalOffset, Redactor, RunSegmentId, SessionId,
    TaskEventEnvelope, TaskId, TenantId,
};

use crate::task_event::TaskEvent;
use crate::{
    apply_cursor, apply_event_id_cursor, journal_error, AppendMetadata, EventEnvelope, EventStore,
    JournalRedaction, PrunePolicy, PruneReport, SessionFilter, SessionSnapshot, SessionSummary,
    TaskStore, MAX_EVENTS_PER_TRANSACTION, MAX_TOTAL_EVENT_BYTES_PER_TRANSACTION,
};

pub struct TaskEventStoreAdapter {
    store: Arc<TaskStore>,
    task_id: TaskId,
    tenant_id: TenantId,
    session_id: SessionId,
    run_segment_id: Option<RunSegmentId>,
    redaction: JournalRedaction,
}

impl TaskEventStoreAdapter {
    pub fn new(
        store: Arc<TaskStore>,
        task_id: TaskId,
        tenant_id: TenantId,
        session_id: SessionId,
        redactor: Arc<dyn Redactor>,
    ) -> Self {
        Self {
            store,
            task_id,
            tenant_id,
            session_id,
            run_segment_id: None,
            redaction: JournalRedaction::new(redactor),
        }
    }

    #[must_use]
    pub fn with_run_segment_id(mut self, run_segment_id: RunSegmentId) -> Self {
        self.run_segment_id = Some(run_segment_id);
        self
    }

    fn validate_scope(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<(), JournalError> {
        if tenant_id != self.tenant_id || session_id != self.session_id {
            return Err(journal_error(
                "engine event scope does not match the task adapter binding",
            ));
        }
        Ok(())
    }

    async fn append_checked(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        metadata: AppendMetadata,
        expected_next_offset: Option<JournalOffset>,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        self.validate_scope(tenant_id, session_id)?;
        let encoded_events = encode_event_batch(events)?;
        let store = Arc::clone(&self.store);
        let task_id = self.task_id;
        let run_segment_id = self.run_segment_id;
        let redaction = self.redaction.clone();
        let result = tokio::task::spawn_blocking(move || {
            let events: Vec<Event> =
                serde_json::from_slice(&encoded_events).map_err(journal_error)?;
            let events = events
                .iter()
                .map(|event| redaction.redact_event(event))
                .collect::<Result<Vec<_>, _>>()?;
            store
                .append_engine_events(
                    task_id,
                    tenant_id,
                    session_id,
                    run_segment_id,
                    metadata,
                    expected_next_offset.map(|offset| offset.0),
                    &events,
                )
                .map_err(journal_error)
        })
        .await
        .map_err(journal_error)??;
        Ok(JournalOffset(result))
    }

    fn unsupported(operation: &str) -> JournalError {
        journal_error(format!(
            "{operation} is not supported by the append-only task engine adapter"
        ))
    }

    async fn load_envelopes(&self) -> Result<Vec<EventEnvelope>, JournalError> {
        let store = Arc::clone(&self.store);
        let task_id = self.task_id;
        let tenant_id = self.tenant_id;
        let session_id = self.session_id;
        tokio::task::spawn_blocking(move || {
            let mut after_stream_sequence = 0;
            let mut envelopes = Vec::new();
            loop {
                let page = store
                    .task_events_after(task_id, after_stream_sequence, usize::MAX)
                    .map_err(journal_error)?;
                let Some(last) = page.last() else {
                    break;
                };
                after_stream_sequence = last.stream_sequence;
                for envelope in page {
                    if let Some(envelope) = decode_engine_envelope(envelope, tenant_id, session_id)?
                    {
                        envelopes.push(envelope);
                    }
                }
            }
            Ok(envelopes)
        })
        .await
        .map_err(journal_error)?
    }
}

fn decode_engine_envelope(
    envelope: TaskEventEnvelope,
    tenant_id: TenantId,
    session_id: SessionId,
) -> Result<Option<EventEnvelope>, JournalError> {
    if !envelope.event_type.starts_with("engine.") {
        return Ok(None);
    }
    let event_id = envelope.event_id;
    let recorded_at = envelope.recorded_at;
    let event = TaskEvent::decode(
        &envelope.event_type,
        envelope.schema_version,
        envelope.payload,
    )
    .map_err(journal_error)?;
    let TaskEvent::Engine { payload, .. } = event else {
        return Ok(None);
    };
    if payload.tenant_id != tenant_id || payload.session_id != session_id {
        return Ok(None);
    }
    Ok(Some(EventEnvelope {
        offset: JournalOffset(payload.journal_offset),
        event_id,
        session_id: payload.session_id,
        tenant_id: payload.tenant_id,
        run_id: payload.run_id,
        correlation_id: payload.correlation_id,
        causation_id: payload.causation_id,
        recorded_at,
        payload: payload.event,
    }))
}

fn encode_event_batch(events: &[Event]) -> Result<Vec<u8>, JournalError> {
    if events.len() > MAX_EVENTS_PER_TRANSACTION {
        return Err(journal_error(format!(
            "an engine event batch may contain at most {MAX_EVENTS_PER_TRANSACTION} events"
        )));
    }
    let mut writer = BoundedWriter::new(MAX_TOTAL_EVENT_BYTES_PER_TRANSACTION);
    serde_json::to_writer(&mut writer, events).map_err(journal_error)?;
    Ok(writer.into_inner())
}

struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
}

impl BoundedWriter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next_len = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("engine event batch size overflow"))?;
        if next_len > self.limit {
            return Err(io::Error::other("engine event batch exceeds 8 MiB"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl EventStore for TaskEventStoreAdapter {
    async fn append(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        self.append_checked(tenant, session_id, AppendMetadata::default(), None, events)
            .await
    }

    async fn append_with_metadata(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        metadata: AppendMetadata,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        self.append_checked(tenant, session_id, metadata, None, events)
            .await
    }

    async fn append_with_metadata_expect_next_offset(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        metadata: AppendMetadata,
        expected_next_offset: JournalOffset,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        self.append_checked(
            tenant,
            session_id,
            metadata,
            Some(expected_next_offset),
            events,
        )
        .await
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: crate::ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError> {
        self.validate_scope(tenant, session_id)?;
        let mut envelopes = self.load_envelopes().await?;
        apply_cursor(&mut envelopes, cursor);
        Ok(Box::pin(stream::iter(envelopes)))
    }

    async fn query_after(
        &self,
        tenant: TenantId,
        after: Option<EventId>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        self.validate_scope(tenant, self.session_id)?;
        let mut envelopes = self.load_envelopes().await?;
        apply_event_id_cursor(&mut envelopes, after)?;
        envelopes.truncate(limit);
        Ok(envelopes)
    }

    async fn snapshot(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, JournalError> {
        Err(Self::unsupported("snapshot"))
    }

    async fn save_snapshot(
        &self,
        _tenant: TenantId,
        _snapshot: SessionSnapshot,
    ) -> Result<(), JournalError> {
        Err(Self::unsupported("save_snapshot"))
    }

    async fn compact_link(
        &self,
        _parent: SessionId,
        _child: SessionId,
        _reason: ForkReason,
    ) -> Result<(), JournalError> {
        Err(Self::unsupported("compact_link"))
    }

    async fn delete_session(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
    ) -> Result<bool, JournalError> {
        Err(Self::unsupported("delete_session"))
    }

    async fn list_sessions(
        &self,
        _tenant: TenantId,
        _filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, JournalError> {
        Err(Self::unsupported("list_sessions"))
    }

    async fn prune(
        &self,
        _tenant: TenantId,
        _policy: PrunePolicy,
    ) -> Result<PruneReport, JournalError> {
        Err(Self::unsupported("prune"))
    }
}

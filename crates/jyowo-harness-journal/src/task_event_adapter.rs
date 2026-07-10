//! Legacy engine `EventStore` adapter backed by the unified task log.

use std::io::{self, Write};
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use harness_contracts::{
    Event, EventId, ForkReason, JournalError, JournalOffset, Redactor, SessionId, TaskId, TenantId,
};

use crate::{
    journal_error, AppendMetadata, EventEnvelope, EventStore, JournalRedaction, PrunePolicy,
    PruneReport, SessionFilter, SessionSnapshot, SessionSummary, TaskStore,
    MAX_EVENTS_PER_TRANSACTION, MAX_TOTAL_EVENT_BYTES_PER_TRANSACTION,
};

pub struct TaskEventStoreAdapter {
    store: Arc<TaskStore>,
    task_id: TaskId,
    tenant_id: TenantId,
    session_id: SessionId,
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
            redaction: JournalRedaction::new(redactor),
        }
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
        _tenant: TenantId,
        _session_id: SessionId,
        _cursor: crate::ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError> {
        Err(Self::unsupported("read_envelopes"))
    }

    async fn query_after(
        &self,
        _tenant: TenantId,
        _after: Option<EventId>,
        _limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        Err(Self::unsupported("query_after"))
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

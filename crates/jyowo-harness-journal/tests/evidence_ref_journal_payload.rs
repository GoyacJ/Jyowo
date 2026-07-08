//! Journal-backed evidence payload read guards.

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use harness_contracts::*;
use harness_journal::evidence::{
    EvidenceReadWindow, EvidenceRefRecord, EvidenceRefSource, EvidenceRefStore,
    InMemoryEvidenceRefRegistry, RedactionProvenance,
};
use harness_journal::{
    EventEnvelope, EventStore, InMemoryBlobStore, SessionFilter, SessionSnapshot,
};

#[tokio::test]
async fn journal_payload_evidence_reads_source_event_and_checks_hash() {
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let session_id = SessionId::new();
    let event = Event::AssistantNotice(AssistantNoticeEvent {
        run_id: RunId::new(),
        notice_id: RequestId::new(),
        body: UiSafeText::from_trusted_redacted("journal backed output"),
        code: None,
        at: chrono::Utc::now(),
    });
    let envelope = test_envelope(session_id, event.clone());
    let event_store = Arc::new(StaticEventStore::new(vec![envelope.clone()]));
    let store = EvidenceRefStore::new_with_event_store(registry, blob_store, event_store);
    let expected = serde_json::to_value(&event)
        .expect("event serializes")
        .pointer("/body")
        .and_then(serde_json::Value::as_str)
        .expect("body is string")
        .as_bytes()
        .to_vec();
    let hash = blake3::hash(&expected);
    let record = journal_record(
        "ref-journal",
        session_id,
        &envelope,
        "/body",
        expected.len() as u64,
        hash.as_bytes(),
    );

    let ref_id = store
        .store_journal_evidence(TenantId::SINGLE, record)
        .await
        .expect("journal ref stores");
    let read = store
        .read_evidence(
            TenantId::SINGLE,
            &session_id.to_string(),
            &ref_id,
            EvidenceRefKind::CommandOutput,
        )
        .await
        .expect("journal evidence reads");

    assert_eq!(read.bytes, expected);
    assert_eq!(read.content_hash, hash_hex_for_test(hash.as_bytes()));
}

#[tokio::test]
async fn journal_payload_evidence_reads_typed_tool_result_pointer() {
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let session_id = SessionId::new();
    let expected = "typed stdout\nline 2";
    let event = Event::ToolUseCompleted(ToolUseCompletedEvent {
        tool_use_id: ToolUseId::new(),
        result: ToolResult::Structured(serde_json::json!({
            "exitCode": 0,
            "stdout": expected,
            "stderr": "",
        })),
        usage: None,
        duration_ms: 13,
        at: chrono::Utc::now(),
    });
    let envelope = test_envelope(session_id, event);
    let event_store = Arc::new(StaticEventStore::new(vec![envelope.clone()]));
    let store = EvidenceRefStore::new_with_event_store(registry, blob_store, event_store);
    let hash = blake3::hash(expected.as_bytes());
    let record = journal_record(
        "ref-typed-tool-result",
        session_id,
        &envelope,
        "/result/structured/stdout",
        expected.len() as u64,
        hash.as_bytes(),
    );

    let ref_id = store
        .store_journal_evidence(TenantId::SINGLE, record)
        .await
        .expect("typed journal ref stores");
    let read = store
        .read_evidence(
            TenantId::SINGLE,
            &session_id.to_string(),
            &ref_id,
            EvidenceRefKind::CommandOutput,
        )
        .await
        .expect("typed journal evidence reads");

    assert_eq!(read.bytes, expected.as_bytes());
    assert_eq!(read.content_hash, hash_hex_for_test(hash.as_bytes()));
}

#[tokio::test]
async fn journal_payload_window_reads_bounded_slice() {
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let session_id = SessionId::new();
    let event = Event::AssistantNotice(AssistantNoticeEvent {
        run_id: RunId::new(),
        notice_id: RequestId::new(),
        body: UiSafeText::from_trusted_redacted("abcdefghijklmnopqrstuvwxyz"),
        code: None,
        at: chrono::Utc::now(),
    });
    let envelope = test_envelope(session_id, event.clone());
    let event_store = Arc::new(StaticEventStore::new(vec![envelope.clone()]));
    let store = EvidenceRefStore::new_with_event_store(registry, blob_store, event_store);
    let expected = serde_json::to_value(&event)
        .expect("event serializes")
        .pointer("/body")
        .and_then(serde_json::Value::as_str)
        .expect("body is string")
        .as_bytes()
        .to_vec();
    let hash = blake3::hash(&expected);
    let record = journal_record(
        "ref-journal-window",
        session_id,
        &envelope,
        "/body",
        expected.len() as u64,
        hash.as_bytes(),
    );

    let ref_id = store
        .store_journal_evidence(TenantId::SINGLE, record)
        .await
        .expect("journal ref stores");
    let page = store
        .read_evidence_window(
            TenantId::SINGLE,
            &session_id.to_string(),
            &ref_id,
            EvidenceRefKind::CommandOutput,
            EvidenceReadWindow {
                cursor: Some("8".to_owned()),
                max_bytes: 4,
            },
        )
        .await
        .expect("journal window reads");

    assert_eq!(page.bytes, b"ijkl");
    assert_eq!(page.content_bytes, 26);
    assert_eq!(page.returned_bytes, 4);
    assert_eq!(page.offset_bytes, 8);
    assert_eq!(page.next_cursor.as_deref(), Some("12"));
}

fn journal_record(
    id: &str,
    session_id: SessionId,
    envelope: &EventEnvelope,
    json_pointer: &str,
    byte_length: u64,
    content_hash: &[u8; 32],
) -> EvidenceRefRecord {
    EvidenceRefRecord {
        id: EvidenceRefId::new(id),
        kind: EvidenceRefKind::CommandOutput,
        conversation_id: session_id.to_string(),
        run_id: "run-1".to_owned(),
        source_event_refs: vec![],
        artifact_id: None,
        revision_id: None,
        content_type: "text/plain".to_owned(),
        byte_length,
        content_hash: content_hash.to_vec(),
        redaction_state: EvidenceRedactionState::Clean,
        redaction_provenance: RedactionProvenance {
            redactor_version: "event-redacted-v1".to_owned(),
        },
        retention: BlobRetention::TenantScoped,
        source: EvidenceRefSource::JournalPayload {
            event_id: envelope.event_id.to_string(),
            json_pointer: json_pointer.to_owned(),
        },
    }
}

fn hash_hex_for_test(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn test_envelope(session_id: SessionId, payload: Event) -> EventEnvelope {
    EventEnvelope {
        offset: JournalOffset(1),
        event_id: EventId::new(),
        session_id,
        tenant_id: TenantId::SINGLE,
        run_id: None,
        correlation_id: CorrelationId::new(),
        causation_id: None,
        recorded_at: chrono::Utc::now(),
        payload,
    }
}

struct StaticEventStore {
    envelopes: Vec<EventEnvelope>,
}

impl StaticEventStore {
    fn new(envelopes: Vec<EventEnvelope>) -> Self {
        Self { envelopes }
    }
}

#[async_trait]
impl EventStore for StaticEventStore {
    async fn append(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
        _events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        Err(JournalError::Message(
            "static event store is read-only".to_owned(),
        ))
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        _cursor: harness_journal::ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, JournalError> {
        let envelopes = self
            .envelopes
            .iter()
            .filter(|envelope| envelope.tenant_id == tenant && envelope.session_id == session_id)
            .cloned()
            .collect::<Vec<_>>();
        Ok(Box::pin(futures::stream::iter(envelopes)))
    }

    async fn query_after(
        &self,
        tenant: TenantId,
        _after: Option<EventId>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        Ok(self
            .envelopes
            .iter()
            .filter(|envelope| envelope.tenant_id == tenant)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn snapshot(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, JournalError> {
        Ok(None)
    }

    async fn save_snapshot(
        &self,
        _tenant: TenantId,
        _snapshot: SessionSnapshot,
    ) -> Result<(), JournalError> {
        Ok(())
    }

    async fn compact_link(
        &self,
        _parent: SessionId,
        _child: SessionId,
        _reason: ForkReason,
    ) -> Result<(), JournalError> {
        Ok(())
    }

    async fn delete_session(
        &self,
        _tenant: TenantId,
        _session_id: SessionId,
    ) -> Result<bool, JournalError> {
        Ok(false)
    }

    async fn list_sessions(
        &self,
        _tenant: TenantId,
        _filter: SessionFilter,
    ) -> Result<Vec<harness_journal::SessionSummary>, JournalError> {
        Ok(Vec::new())
    }

    async fn prune(
        &self,
        _tenant: TenantId,
        _policy: harness_journal::PrunePolicy,
    ) -> Result<harness_journal::PruneReport, JournalError> {
        Ok(harness_journal::PruneReport {
            events_removed: 0,
            snapshots_removed: 0,
            bytes_freed: 0,
        })
    }
}

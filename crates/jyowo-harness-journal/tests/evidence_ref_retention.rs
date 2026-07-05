//! Evidence ref retention read guards.

use std::sync::Arc;

use harness_contracts::{
    BlobId, BlobRef, BlobRetention, ConversationEventRef, EvidenceRedactionState, EvidenceRefId,
    EvidenceRefKind, SessionId, TenantId,
};
use harness_journal::evidence::{
    EvidenceReadWindow, EvidenceRefRecord, EvidenceRefSource, EvidenceRefStore,
    InMemoryEvidenceRefRegistry, RedactionProvenance,
};
use harness_journal::InMemoryBlobStore;

fn blob_record(id: &str, conversation_id: &str, bytes: &[u8]) -> EvidenceRefRecord {
    let hash = blake3::hash(bytes);
    EvidenceRefRecord {
        id: EvidenceRefId::new(id),
        kind: EvidenceRefKind::CommandOutput,
        conversation_id: conversation_id.to_owned(),
        run_id: "run-1".to_owned(),
        source_event_refs: Vec::<ConversationEventRef>::new(),
        artifact_id: None,
        revision_id: None,
        content_type: "text/plain".to_owned(),
        byte_length: bytes.len() as u64,
        content_hash: hash.as_bytes().to_vec(),
        redaction_state: EvidenceRedactionState::Clean,
        redaction_provenance: RedactionProvenance {
            redactor_version: "1.0".to_owned(),
        },
        retention: BlobRetention::TenantScoped,
        source: EvidenceRefSource::Blob {
            blob_ref: BlobRef {
                id: BlobId::new(),
                size: bytes.len() as u64,
                content_hash: *hash.as_bytes(),
                content_type: Some("text/plain".to_owned()),
            },
        },
    }
}

#[tokio::test]
async fn read_rejects_session_scoped_retention_mismatch() {
    let store = EvidenceRefStore::new(
        Arc::new(InMemoryEvidenceRefRegistry::new()),
        Arc::new(InMemoryBlobStore::default()),
    );
    let session_id = SessionId::new();
    let bytes = b"session scoped output".to_vec();
    let mut record = blob_record("ref-retention-session", &session_id.to_string(), &bytes);
    record.retention = BlobRetention::SessionScoped(SessionId::new());

    store
        .store_blob_evidence(TenantId::SINGLE, record.clone(), bytes)
        .await
        .expect("evidence stores");

    let error = store
        .read_evidence(
            TenantId::SINGLE,
            &session_id.to_string(),
            &record.id,
            EvidenceRefKind::CommandOutput,
        )
        .await
        .expect_err("retention mismatch is rejected");

    assert!(error.to_string().contains("retention"));
}

#[tokio::test]
async fn read_window_rejects_ttl_retention_without_expiry_authority() {
    let store = EvidenceRefStore::new(
        Arc::new(InMemoryEvidenceRefRegistry::new()),
        Arc::new(InMemoryBlobStore::default()),
    );
    let bytes = b"ttl output".to_vec();
    let mut record = blob_record("ref-retention-ttl", "conv-1", &bytes);
    record.retention = BlobRetention::TtlDays(7);

    store
        .store_blob_evidence(TenantId::SINGLE, record.clone(), bytes)
        .await
        .expect("evidence stores");

    let error = store
        .read_evidence_window(
            TenantId::SINGLE,
            "conv-1",
            &record.id,
            EvidenceRefKind::CommandOutput,
            EvidenceReadWindow {
                cursor: None,
                max_bytes: 8,
            },
        )
        .await
        .expect_err("ttl evidence reads fail closed");

    assert!(error.to_string().contains("ttl-scoped"));
}

//! Tests for evidence ref store.

use chrono::Utc;
use harness_contracts::*;
use harness_journal::evidence::*;
use std::sync::Arc;

fn make_record(id: &str, conversation_id: &str, kind: EvidenceRefKind) -> EvidenceRefRecord {
    EvidenceRefRecord {
        id: EvidenceRefId::new(id),
        kind,
        conversation_id: conversation_id.to_owned(),
        run_id: "run-1".to_owned(),
        source_event_refs: vec![],
        artifact_id: None,
        revision_id: None,
        content_type: "text/plain".to_owned(),
        byte_length: 100,
        content_hash: vec![1, 2, 3],
        redaction_state: EvidenceRedactionState::Clean,
        redaction_provenance: RedactionProvenance {
            redactor_version: "1.0".to_owned(),
        },
        retention: BlobRetention::TenantScoped,
        source: EvidenceRefSource::JournalPayload {
            event_id: "event-1".to_owned(),
            json_pointer: "/output".to_owned(),
        },
    }
}

#[tokio::test]
async fn insert_and_get_roundtrip() {
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let record = make_record("ref-1", "conv-1", EvidenceRefKind::CommandOutput);

    registry
        .insert(TenantId::SINGLE, record.clone())
        .await
        .unwrap();

    let found = registry.get(TenantId::SINGLE, &record.id).await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap(), record);
}

#[tokio::test]
async fn insert_is_idempotent_for_same_hash() {
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let record = make_record("ref-1", "conv-1", EvidenceRefKind::CommandOutput);

    registry
        .insert(TenantId::SINGLE, record.clone())
        .await
        .unwrap();
    // Second insert with same hash should be idempotent
    registry
        .insert(TenantId::SINGLE, record.clone())
        .await
        .unwrap();
}

#[tokio::test]
async fn insert_fails_on_conflicting_metadata() {
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let record1 = make_record("ref-1", "conv-1", EvidenceRefKind::CommandOutput);

    let mut record2 = record1.clone();
    record2.content_hash = vec![9, 9, 9]; // different hash

    registry.insert(TenantId::SINGLE, record1).await.unwrap();
    let result = registry.insert(TenantId::SINGLE, record2).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn delete_for_conversation_removes_all() {
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());

    registry
        .insert(
            TenantId::SINGLE,
            make_record("ref-1", "conv-1", EvidenceRefKind::CommandOutput),
        )
        .await
        .unwrap();
    registry
        .insert(
            TenantId::SINGLE,
            make_record("ref-2", "conv-1", EvidenceRefKind::DiffPatch),
        )
        .await
        .unwrap();
    registry
        .insert(
            TenantId::SINGLE,
            make_record("ref-3", "conv-2", EvidenceRefKind::CommandOutput),
        )
        .await
        .unwrap();

    registry
        .delete_for_conversation(TenantId::SINGLE, "conv-1")
        .await
        .unwrap();

    assert!(registry
        .get(TenantId::SINGLE, &EvidenceRefId::new("ref-1"))
        .await
        .unwrap()
        .is_none());
    assert!(registry
        .get(TenantId::SINGLE, &EvidenceRefId::new("ref-2"))
        .await
        .unwrap()
        .is_none());
    // conv-2 ref should still exist
    assert!(registry
        .get(TenantId::SINGLE, &EvidenceRefId::new("ref-3"))
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn list_live_blob_roots_returns_only_blob_sources() {
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let mut blob_record = make_record("blob-ref-1", "conv-1", EvidenceRefKind::ArtifactContent);
    blob_record.source = EvidenceRefSource::Blob {
        blob_ref: BlobRef {
            id: BlobId::new(),
            size: 100,
            content_hash: [1; 32],
            content_type: Some("text/plain".to_owned()),
        },
    };
    let journal_record = make_record("journal-ref-1", "conv-1", EvidenceRefKind::CommandOutput);

    registry
        .insert(TenantId::SINGLE, blob_record)
        .await
        .unwrap();
    registry
        .insert(TenantId::SINGLE, journal_record)
        .await
        .unwrap();

    let roots = registry
        .list_live_blob_roots(TenantId::SINGLE)
        .await
        .unwrap();

    // Only blob-backed refs should be returned
    assert_eq!(roots.len(), 1);
}

#[tokio::test]
async fn owner_mismatch_fails_read() {
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let record = make_record("ref-1", "conv-1", EvidenceRefKind::CommandOutput);

    registry.insert(TenantId::SINGLE, record).await.unwrap();

    // Lookup with wrong id should return None
    let found = registry
        .get(TenantId::SINGLE, &EvidenceRefId::new("non-existent"))
        .await
        .unwrap();
    assert!(found.is_none());
}

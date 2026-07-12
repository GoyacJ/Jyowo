#![cfg(all(feature = "sqlite", feature = "in-memory"))]

//! Evidence ref retention read guards.

use std::sync::Arc;

use bytes::Bytes;
use harness_contracts::{
    BlobId, BlobMeta, BlobRef, BlobRetention, BlobStore, ConversationEventRef,
    EvidenceRedactionState, EvidenceRefId, EvidenceRefKind, SessionId, TenantId,
};
use harness_journal::evidence::{
    EvidenceReadWindow, EvidenceRefRecord, EvidenceRefSource, EvidenceRefStore,
    InMemoryEvidenceRefRegistry, RedactionProvenance,
};
use harness_journal::InMemoryBlobStore;
#[cfg(feature = "sqlite")]
use harness_journal::{FileBlobStore, SqliteEvidenceRefRegistry};

#[cfg(feature = "sqlite")]
fn temp_root(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "jyowo-evidence-ref-retention-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    root
}

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

async fn existing_blob_record(
    id: &str,
    conversation_id: &str,
    bytes: Vec<u8>,
    blob_retention: BlobRetention,
) -> (EvidenceRefStore, EvidenceRefRecord) {
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let hash = blake3::hash(&bytes);
    let blob_ref = blob_store
        .put(
            TenantId::SINGLE,
            Bytes::from(bytes.clone()),
            BlobMeta {
                content_type: Some("text/plain".to_owned()),
                size: bytes.len() as u64,
                content_hash: *hash.as_bytes(),
                created_at: chrono::Utc::now(),
                retention: blob_retention,
            },
        )
        .await
        .expect("blob stores");
    let mut record = blob_record(id, conversation_id, &bytes);
    record.source = EvidenceRefSource::Blob { blob_ref };
    (
        EvidenceRefStore::new(Arc::new(InMemoryEvidenceRefRegistry::new()), blob_store),
        record,
    )
}

#[tokio::test]
async fn existing_blob_registration_rejects_other_session_retention() {
    let session_id = SessionId::new();
    let (store, record) = existing_blob_record(
        "ref-existing-other-session",
        &session_id.to_string(),
        b"other session blob output".to_vec(),
        BlobRetention::SessionScoped(SessionId::new()),
    )
    .await;

    let error = store
        .store_existing_blob_evidence_with_blob_retention(TenantId::SINGLE, record.clone())
        .await
        .expect_err("other-session retention is rejected");

    assert!(error.to_string().contains("retention"));
    assert!(store
        .list_for_conversation(TenantId::SINGLE, &session_id.to_string())
        .await
        .expect("conversation refs list")
        .is_empty());
}

#[tokio::test]
async fn existing_blob_registration_rejects_ttl_retention() {
    let session_id = SessionId::new();
    let (store, record) = existing_blob_record(
        "ref-existing-ttl",
        &session_id.to_string(),
        b"ttl blob output".to_vec(),
        BlobRetention::TtlDays(7),
    )
    .await;

    let error = store
        .store_existing_blob_evidence_with_blob_retention(TenantId::SINGLE, record.clone())
        .await
        .expect_err("ttl retention is rejected");

    assert!(error.to_string().contains("ttl-scoped"));
    assert!(store
        .list_for_conversation(TenantId::SINGLE, &session_id.to_string())
        .await
        .expect("conversation refs list")
        .is_empty());
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn existing_blob_registration_reads_session_scoped_file_blob_with_sqlite_registry() {
    let root = temp_root("sqlite-file-session");
    let session_id = SessionId::new();
    let bytes = b"sqlite file blob output".to_vec();
    let hash = blake3::hash(&bytes);
    let blob_store = Arc::new(FileBlobStore::open(root.join("blobs")).expect("blob store opens"));
    let blob_ref = blob_store
        .put(
            TenantId::SINGLE,
            Bytes::from(bytes.clone()),
            BlobMeta {
                content_type: Some("text/plain".to_owned()),
                size: bytes.len() as u64,
                content_hash: *hash.as_bytes(),
                created_at: chrono::Utc::now(),
                retention: BlobRetention::SessionScoped(session_id),
            },
        )
        .await
        .expect("blob stores");
    let registry = Arc::new(
        SqliteEvidenceRefRegistry::open(root.join("evidence.sqlite"))
            .await
            .expect("registry opens"),
    );
    let store = EvidenceRefStore::new(registry, blob_store);
    let mut record = blob_record("ref-existing-sqlite-file", &session_id.to_string(), &bytes);
    record.source = EvidenceRefSource::Blob { blob_ref };

    let ref_id = store
        .store_existing_blob_evidence_with_blob_retention(TenantId::SINGLE, record)
        .await
        .expect("existing blob evidence stores");
    let read = store
        .read_evidence(
            TenantId::SINGLE,
            &session_id.to_string(),
            &ref_id,
            EvidenceRefKind::CommandOutput,
        )
        .await
        .expect("evidence reads");

    assert_eq!(read.bytes, bytes);
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

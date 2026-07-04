//! Tests for evidence ref store.

use harness_contracts::*;
use harness_journal::evidence::*;
use harness_journal::{FileBlobStore, InMemoryBlobStore, RetentionEnforcer};
use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::BoxStream;

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

fn blob_record(id: &str, conversation_id: &str, bytes: &[u8]) -> EvidenceRefRecord {
    let hash = blake3::hash(bytes);
    let mut content_hash = [0u8; 32];
    content_hash.copy_from_slice(hash.as_bytes());
    let mut record = make_record(id, conversation_id, EvidenceRefKind::CommandOutput);
    record.byte_length = bytes.len() as u64;
    record.content_hash = content_hash.to_vec();
    record.source = EvidenceRefSource::Blob {
        blob_ref: BlobRef {
            id: BlobId::new(),
            size: record.byte_length,
            content_hash,
            content_type: Some(record.content_type.clone()),
        },
    };
    record
}

fn temp_root(name: &str) -> std::path::PathBuf {
    let unique = format!(
        "{}-{}-{}",
        name,
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    std::env::temp_dir()
        .join("jyowo-evidence-ref-tests")
        .join(unique)
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

#[tokio::test]
async fn read_rejects_owner_mismatch() {
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let store = EvidenceRefStore::new(registry, blob_store);
    let bytes = b"owned output".to_vec();
    let record = blob_record("ref-owner", "conv-1", &bytes);

    store
        .store_blob_evidence(TenantId::SINGLE, record.clone(), bytes)
        .await
        .expect("evidence stores");

    let error = store
        .read_evidence(
            TenantId::SINGLE,
            "conv-2",
            &record.id,
            EvidenceRefKind::CommandOutput,
        )
        .await
        .expect_err("owner mismatch is rejected");

    assert!(error.to_string().contains("conversation"));
}

#[tokio::test]
async fn read_rejects_kind_mismatch() {
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let store = EvidenceRefStore::new(registry, blob_store);
    let bytes = b"diff content".to_vec();
    let record = blob_record("ref-kind", "conv-1", &bytes);

    store
        .store_blob_evidence(TenantId::SINGLE, record.clone(), bytes)
        .await
        .expect("evidence stores");

    let error = store
        .read_evidence(
            TenantId::SINGLE,
            "conv-1",
            &record.id,
            EvidenceRefKind::DiffPatch,
        )
        .await
        .expect_err("kind mismatch is rejected");

    assert!(error.to_string().contains("kind mismatch"));
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_registry_survives_restart_and_reads_blob_evidence() {
    let root = temp_root("sqlite-restart");
    let registry_path = root.join("evidence.sqlite");
    let blob_root = root.join("blobs");
    let bytes = b"durable command output".to_vec();
    let record = blob_record("ref-durable", "conv-1", &bytes);

    let registry = Arc::new(
        harness_journal::SqliteEvidenceRefRegistry::open(&registry_path)
            .await
            .expect("registry opens"),
    );
    let blob_store = Arc::new(FileBlobStore::open(&blob_root).expect("blob store opens"));
    let store = EvidenceRefStore::new(registry, blob_store);

    let stored_id = store
        .store_blob_evidence(TenantId::SINGLE, record.clone(), bytes.clone())
        .await
        .expect("evidence stores");
    assert_eq!(stored_id, record.id);

    drop(store);

    let restarted_registry = Arc::new(
        harness_journal::SqliteEvidenceRefRegistry::open(&registry_path)
            .await
            .expect("registry reopens"),
    );
    let restarted_blob_store =
        Arc::new(FileBlobStore::open(&blob_root).expect("blob store reopens"));
    let restarted = EvidenceRefStore::new(restarted_registry, restarted_blob_store);

    let read = restarted
        .read_evidence(
            TenantId::SINGLE,
            "conv-1",
            &record.id,
            EvidenceRefKind::CommandOutput,
        )
        .await
        .expect("evidence reads after restart");

    assert_eq!(read.bytes, bytes);
    assert_eq!(read.byte_length, bytes.len() as u64);
    assert_eq!(read.content_type, "text/plain");
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn file_blob_store_blob_evidence_write_is_idempotent_for_same_ref() {
    let root = temp_root("file-blob-idempotent");
    let registry = Arc::new(
        harness_journal::SqliteEvidenceRefRegistry::open(root.join("evidence.sqlite"))
            .await
            .expect("registry opens"),
    );
    let blob_store = Arc::new(FileBlobStore::open(root.join("blobs")).expect("blob store opens"));
    let store = EvidenceRefStore::new(registry.clone(), blob_store);
    let bytes = b"repeatable command output".to_vec();
    let record = blob_record("ref-repeatable", "conv-1", &bytes);

    store
        .store_blob_evidence(TenantId::SINGLE, record.clone(), bytes.clone())
        .await
        .expect("first evidence write stores");
    store
        .store_blob_evidence(TenantId::SINGLE, record.clone(), bytes)
        .await
        .expect("second evidence write is idempotent");

    let records = registry
        .list_for_conversation(TenantId::SINGLE, "conv-1")
        .await
        .expect("registry lists");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, record.id);
}

#[tokio::test]
async fn read_rejects_hash_mismatch_from_blob_store() {
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let store = EvidenceRefStore::new(registry.clone(), blob_store.clone());
    let bytes = b"registry hash source".to_vec();
    let mut record = blob_record("ref-hash", "conv-1", &bytes);

    let wrong_bytes = b"different blob bytes";
    let wrong_hash = blake3::hash(wrong_bytes);
    let mut wrong_content_hash = [0u8; 32];
    wrong_content_hash.copy_from_slice(wrong_hash.as_bytes());
    let blob_ref = blob_store
        .put(
            TenantId::SINGLE,
            bytes::Bytes::from(wrong_bytes.to_vec()),
            BlobMeta {
                content_type: Some("text/plain".to_owned()),
                size: wrong_bytes.len() as u64,
                content_hash: wrong_content_hash,
                created_at: chrono::Utc::now(),
                retention: BlobRetention::TenantScoped,
            },
        )
        .await
        .expect("blob stores");
    record.source = EvidenceRefSource::Blob { blob_ref };

    registry
        .insert(TenantId::SINGLE, record.clone())
        .await
        .expect("registry stores");

    let error = store
        .read_evidence(
            TenantId::SINGLE,
            "conv-1",
            &record.id,
            EvidenceRefKind::CommandOutput,
        )
        .await
        .expect_err("hash mismatch is rejected");

    assert!(error.to_string().contains("hash"));
}

#[tokio::test]
async fn withheld_blob_evidence_does_not_return_content() {
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let store = EvidenceRefStore::new(registry, blob_store);
    let bytes = b"secret output".to_vec();
    let mut record = blob_record("ref-withheld", "conv-1", &bytes);
    record.redaction_state = EvidenceRedactionState::Withheld;

    store
        .store_blob_evidence(TenantId::SINGLE, record.clone(), bytes)
        .await
        .expect("evidence stores");

    let error = store
        .read_evidence(
            TenantId::SINGLE,
            "conv-1",
            &record.id,
            EvidenceRefKind::CommandOutput,
        )
        .await
        .expect_err("withheld evidence is not returned");

    assert!(error.to_string().contains("withheld"));
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn delete_for_conversation_removes_registry_row_and_blob() {
    let root = temp_root("delete");
    let registry = Arc::new(
        harness_journal::SqliteEvidenceRefRegistry::open(root.join("evidence.sqlite"))
            .await
            .expect("registry opens"),
    );
    let blob_store = Arc::new(FileBlobStore::open(root.join("blobs")).expect("blob store opens"));
    let store = EvidenceRefStore::new(registry.clone(), blob_store.clone());
    let bytes = b"temporary output".to_vec();
    let record = blob_record("ref-delete", "conv-1", &bytes);

    store
        .store_blob_evidence(TenantId::SINGLE, record.clone(), bytes)
        .await
        .expect("evidence stores");

    let stored = registry
        .get(TenantId::SINGLE, &record.id)
        .await
        .expect("registry reads")
        .expect("record exists");
    let EvidenceRefSource::Blob { blob_ref } = stored.source else {
        panic!("record must be blob-backed");
    };
    assert!(blob_store
        .head(TenantId::SINGLE, &blob_ref)
        .await
        .expect("blob head reads")
        .is_some());

    store
        .delete_for_conversation(TenantId::SINGLE, "conv-1")
        .await
        .expect("conversation evidence deletes");

    assert!(registry
        .get(TenantId::SINGLE, &record.id)
        .await
        .expect("registry reads")
        .is_none());
    assert!(blob_store
        .head(TenantId::SINGLE, &blob_ref)
        .await
        .expect("blob head reads")
        .is_none());
}

#[tokio::test]
async fn registry_insert_failure_removes_orphan_blob() {
    let root = temp_root("orphan");
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let blob_store = Arc::new(FileBlobStore::open(root.join("blobs")).expect("blob store opens"));
    let store = EvidenceRefStore::new(registry, blob_store.clone());
    let bytes = b"first output".to_vec();
    let record = blob_record("ref-orphan", "conv-1", &bytes);

    store
        .store_blob_evidence(TenantId::SINGLE, record.clone(), bytes)
        .await
        .expect("initial evidence stores");
    assert_eq!(
        blob_store
            .inventory(TenantId::SINGLE)
            .expect("inventory reads")
            .len(),
        1
    );

    let conflicting_bytes = b"conflicting output".to_vec();
    let mut conflicting = blob_record("ref-orphan", "conv-1", &conflicting_bytes);
    conflicting.run_id = "run-2".to_owned();

    store
        .store_blob_evidence(TenantId::SINGLE, conflicting, conflicting_bytes)
        .await
        .expect_err("conflicting registry insert is rejected");

    assert_eq!(
        blob_store
            .inventory(TenantId::SINGLE)
            .expect("inventory reads")
            .len(),
        1
    );
}

#[tokio::test]
async fn delete_for_conversation_keeps_registry_row_when_blob_delete_fails() {
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let blob_store = Arc::new(DeleteFailingBlobStore::default());
    let store = EvidenceRefStore::new(registry.clone(), blob_store);
    let bytes = b"undeletable output".to_vec();
    let record = blob_record("ref-delete-fail", "conv-1", &bytes);

    store
        .store_blob_evidence(TenantId::SINGLE, record.clone(), bytes)
        .await
        .expect("evidence stores before delete failure");

    store
        .delete_for_conversation(TenantId::SINGLE, "conv-1")
        .await
        .expect_err("blob delete failure rejects conversation evidence delete");

    assert!(registry
        .get(TenantId::SINGLE, &record.id)
        .await
        .expect("registry reads after failed delete")
        .is_some());
}

#[tokio::test]
async fn gc_keeps_live_evidence_blobs_and_deletes_dead_blobs() {
    let root = temp_root("gc");
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let blob_store = Arc::new(FileBlobStore::open(root.join("blobs")).expect("blob store opens"));
    let store = EvidenceRefStore::new(registry, blob_store.clone());
    let live_bytes = b"live output".to_vec();
    let mut live_record = blob_record("ref-live", "conv-1", &live_bytes);
    live_record.retention = BlobRetention::SessionScoped(SessionId::new());
    store
        .store_blob_evidence(TenantId::SINGLE, live_record, live_bytes)
        .await
        .expect("live evidence stores");
    let live_blob = store
        .list_live_blob_roots(TenantId::SINGLE)
        .await
        .expect("live roots list")
        .into_iter()
        .next()
        .expect("live blob exists");

    let dead_bytes = b"dead output".to_vec();
    let dead_hash = blake3::hash(&dead_bytes);
    let mut dead_content_hash = [0u8; 32];
    dead_content_hash.copy_from_slice(dead_hash.as_bytes());
    let dead_blob = blob_store
        .put(
            TenantId::SINGLE,
            bytes::Bytes::from(dead_bytes.clone()),
            BlobMeta {
                content_type: Some("text/plain".to_owned()),
                size: dead_bytes.len() as u64,
                content_hash: dead_content_hash,
                created_at: chrono::Utc::now(),
                retention: BlobRetention::SessionScoped(SessionId::new()),
            },
        )
        .await
        .expect("dead blob stores");

    let live_ids: HashSet<_> = store
        .list_live_blob_roots(TenantId::SINGLE)
        .await
        .expect("live roots list")
        .into_iter()
        .map(|blob| blob.id)
        .collect();
    let report = RetentionEnforcer::default()
        .collect_garbage(TenantId::SINGLE, &blob_store, &live_ids)
        .await
        .expect("gc runs");

    assert_eq!(report.deleted, 1);
    assert!(blob_store
        .head(TenantId::SINGLE, &live_blob)
        .await
        .expect("live head reads")
        .is_some());
    assert!(blob_store
        .head(TenantId::SINGLE, &dead_blob)
        .await
        .expect("dead head reads")
        .is_none());
}

#[derive(Default)]
struct DeleteFailingBlobStore {
    inner: InMemoryBlobStore,
}

#[async_trait]
impl BlobStore for DeleteFailingBlobStore {
    fn store_id(&self) -> &str {
        "delete-failing-memory"
    }

    async fn put(
        &self,
        tenant: TenantId,
        bytes: Bytes,
        meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        self.inner.put(tenant, bytes, meta).await
    }

    async fn get(
        &self,
        tenant: TenantId,
        blob: &BlobRef,
    ) -> Result<BoxStream<'static, Bytes>, BlobError> {
        self.inner.get(tenant, blob).await
    }

    async fn head(&self, tenant: TenantId, blob: &BlobRef) -> Result<Option<BlobMeta>, BlobError> {
        self.inner.head(tenant, blob).await
    }

    async fn delete(&self, _tenant: TenantId, _blob: &BlobRef) -> Result<(), BlobError> {
        Err(BlobError::Backend("forced delete failure".to_owned()))
    }
}

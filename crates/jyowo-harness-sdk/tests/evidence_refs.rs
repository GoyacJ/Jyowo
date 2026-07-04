#![cfg(feature = "testing")]

use std::sync::Arc;

use harness_contracts::{
    BlobId, BlobRef, BlobRetention, ConversationEventRef, EvidenceRedactionState, EvidenceRefId,
    EvidenceRefKind, NoopRedactor, SessionId, TenantId,
};
use harness_journal::evidence::{
    EvidenceRefRecord, EvidenceRefSource, EvidenceRefStore, InMemoryEvidenceRefRegistry,
    RedactionProvenance,
};
use harness_journal::InMemoryEventStore;
use jyowo_harness_sdk::{testing::*, Harness, SessionOptions};

fn record(
    id: &str,
    conversation_id: &str,
    kind: EvidenceRefKind,
    bytes: &[u8],
) -> EvidenceRefRecord {
    let hash = blake3::hash(bytes);
    let mut content_hash = [0u8; 32];
    content_hash.copy_from_slice(hash.as_bytes());
    EvidenceRefRecord {
        id: EvidenceRefId::new(id),
        kind,
        conversation_id: conversation_id.to_owned(),
        run_id: "run-1".to_owned(),
        source_event_refs: Vec::<ConversationEventRef>::new(),
        artifact_id: None,
        revision_id: None,
        content_type: "text/plain".to_owned(),
        byte_length: bytes.len() as u64,
        content_hash: content_hash.to_vec(),
        redaction_state: EvidenceRedactionState::Clean,
        redaction_provenance: RedactionProvenance {
            redactor_version: "1.0".to_owned(),
        },
        retention: BlobRetention::TenantScoped,
        source: EvidenceRefSource::Blob {
            blob_ref: BlobRef {
                id: BlobId::new(),
                size: bytes.len() as u64,
                content_hash,
                content_type: Some("text/plain".to_owned()),
            },
        },
    }
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "{}-{}-{}",
        name,
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ))
}

async fn harness_with_store(
    evidence_store: Option<Arc<EvidenceRefStore>>,
) -> jyowo_harness_sdk::Harness {
    let mut builder = Harness::builder()
        .with_model(TestModelProvider::default())
        .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
        .with_sandbox(NoopSandbox::new());
    if let Some(store) = evidence_store {
        builder = builder.with_evidence_ref_store_arc(store);
    }
    builder.build().await.expect("harness builds")
}

#[tokio::test]
async fn typed_command_output_fetch_reads_configured_evidence_store() {
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let evidence_store = Arc::new(EvidenceRefStore::new(registry, blob_store));
    let bytes = b"complete command output".to_vec();
    let evidence_record = record(
        "ref-command-output",
        "conv-1",
        EvidenceRefKind::CommandOutput,
        &bytes,
    );
    evidence_store
        .store_blob_evidence(TenantId::SINGLE, evidence_record.clone(), bytes.clone())
        .await
        .expect("evidence stores");

    let harness = harness_with_store(Some(evidence_store)).await;
    let result = harness
        .read_command_output_evidence(TenantId::SINGLE, "conv-1", &evidence_record.id)
        .await
        .expect("typed evidence reads");

    assert_eq!(result.bytes, bytes);
    assert_eq!(result.byte_length, bytes.len() as u64);
}

#[tokio::test]
async fn typed_fetch_fails_closed_when_evidence_store_is_missing() {
    let harness = harness_with_store(None).await;

    let error = harness
        .read_diff_patch_evidence(
            TenantId::SINGLE,
            "conv-1",
            &EvidenceRefId::new("missing-ref"),
        )
        .await
        .expect_err("missing store is rejected");

    assert!(error
        .to_string()
        .contains("evidence ref store not available"));
}

#[tokio::test]
async fn delete_conversation_session_removes_configured_evidence_refs() {
    let workspace = unique_workspace("sdk-evidence-delete");
    std::fs::create_dir_all(&workspace).expect("workspace exists");
    let session_id = SessionId::new();
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let evidence_store = Arc::new(EvidenceRefStore::new(registry, blob_store));
    let bytes = b"conversation scoped output".to_vec();
    let evidence_record = record(
        "ref-delete-session",
        &session_id.to_string(),
        EvidenceRefKind::CommandOutput,
        &bytes,
    );
    evidence_store
        .store_blob_evidence(TenantId::SINGLE, evidence_record.clone(), bytes)
        .await
        .expect("evidence stores");

    let harness = harness_with_store(Some(evidence_store.clone())).await;
    let options = SessionOptions::new(&workspace).with_session_id(session_id);
    harness
        .open_or_create_conversation_session(options.clone())
        .await
        .expect("session opens");
    assert_eq!(
        harness
            .list_live_evidence_blob_roots(TenantId::SINGLE)
            .await
            .expect("harness lists live roots")
            .len(),
        1
    );

    let deleted = harness
        .delete_conversation_session(options)
        .await
        .expect("conversation deletes");

    assert!(deleted);
    assert!(evidence_store
        .read_evidence(
            TenantId::SINGLE,
            &session_id.to_string(),
            &evidence_record.id,
            EvidenceRefKind::CommandOutput,
        )
        .await
        .is_err());
    assert!(evidence_store
        .list_live_blob_roots(TenantId::SINGLE)
        .await
        .expect("live roots list")
        .is_empty());
    assert!(harness
        .list_live_evidence_blob_roots(TenantId::SINGLE)
        .await
        .expect("harness lists live roots after delete")
        .is_empty());
}

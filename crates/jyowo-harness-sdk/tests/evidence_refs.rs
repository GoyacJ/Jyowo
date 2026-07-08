#![cfg(feature = "testing")]

use std::sync::Arc;

use bytes::Bytes;
use harness_contracts::{
    ArtifactCreatedEvent, ArtifactRevisionId, ArtifactSource, ArtifactStatus, BlobId, BlobMeta,
    BlobRef, BlobRetention, BlobStore, ConversationEventRef, ConversationInspectorItem,
    ConversationInspectorSelection, Event, EvidenceRedactionState, EvidenceRefId, EvidenceRefKind,
    RunId, SessionId, TenantId,
};
use harness_journal::evidence::{
    EvidenceRefRecord, EvidenceRefSource, EvidenceRefStore, InMemoryEvidenceRefRegistry,
    RedactionProvenance,
};
use harness_journal::{EventStore, InMemoryEventStore};
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
    harness_with_store_for_workspace(evidence_store, None).await
}

async fn harness_with_store_for_workspace(
    evidence_store: Option<Arc<EvidenceRefStore>>,
    workspace: Option<&std::path::Path>,
) -> jyowo_harness_sdk::Harness {
    let mut builder = Harness::builder()
        .with_model(TestModelProvider::default())
        .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
        .with_sandbox(NoopSandbox::new());
    if let Some(workspace) = workspace {
        builder = builder.with_workspace_root(workspace);
    }
    if let Some(store) = evidence_store {
        builder = builder.with_evidence_ref_store_arc(store);
    }
    builder.build().await.expect("harness builds")
}

async fn harness_with_event_blob_and_evidence_stores(
    event_store: Arc<InMemoryEventStore>,
    blob_store: Arc<InMemoryBlobStore>,
    evidence_store: Arc<EvidenceRefStore>,
) -> jyowo_harness_sdk::Harness {
    Harness::builder()
        .with_model(TestModelProvider::default())
        .with_store(event_store)
        .with_sandbox(NoopSandbox::new())
        .with_blob_store_arc(blob_store)
        .with_evidence_ref_store_arc(evidence_store)
        .build()
        .await
        .expect("harness builds")
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

    let harness =
        harness_with_store_for_workspace(Some(evidence_store.clone()), Some(&workspace)).await;
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

#[tokio::test]
async fn direct_event_store_delete_session_removes_configured_evidence_refs() {
    let workspace = unique_workspace("sdk-evidence-direct-delete");
    std::fs::create_dir_all(&workspace).expect("workspace exists");
    let session_id = SessionId::new();
    let registry = Arc::new(InMemoryEvidenceRefRegistry::new());
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let evidence_store = Arc::new(EvidenceRefStore::new(registry, blob_store));
    let bytes = b"direct event store delete output".to_vec();
    let evidence_record = record(
        "ref-direct-delete-session",
        &session_id.to_string(),
        EvidenceRefKind::CommandOutput,
        &bytes,
    );
    evidence_store
        .store_blob_evidence(TenantId::SINGLE, evidence_record.clone(), bytes)
        .await
        .expect("evidence stores");

    let harness =
        harness_with_store_for_workspace(Some(evidence_store.clone()), Some(&workspace)).await;
    let options = SessionOptions::new(&workspace).with_session_id(session_id);
    harness
        .open_or_create_conversation_session(options)
        .await
        .expect("session opens");

    let deleted = harness
        .event_store()
        .delete_session(TenantId::SINGLE, session_id)
        .await
        .expect("direct event-store delete succeeds");

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
}

#[tokio::test]
async fn artifact_content_refs_ignore_events_with_mismatched_session_id() {
    let requested_session_id = SessionId::new();
    let embedded_session_id = SessionId::new();
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let evidence_store = Arc::new(EvidenceRefStore::new(
        Arc::new(InMemoryEvidenceRefRegistry::new()),
        blob_store.clone(),
    ));
    let bytes = Bytes::from_static(b"wrong session artifact body");
    let content_hash = *blake3::hash(&bytes).as_bytes();
    let blob_ref = blob_store
        .put(
            TenantId::SINGLE,
            bytes.clone(),
            BlobMeta {
                content_type: Some("text/plain".to_owned()),
                size: bytes.len() as u64,
                content_hash,
                created_at: chrono::Utc::now(),
                retention: BlobRetention::SessionScoped(embedded_session_id),
            },
        )
        .await
        .expect("blob writes");
    event_store
        .append(
            TenantId::SINGLE,
            requested_session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                revision_id: ArtifactRevisionId::new(),
                artifact_id: "wrong-session-artifact".to_owned(),
                at: chrono::Utc::now(),
                blob_ref: Some(blob_ref),
                content_hash: Some(content_hash.to_vec()),
                kind: "text".to_owned(),
                preview: Some("Wrong session".to_owned()),
                run_id: RunId::new(),
                session_id: embedded_session_id,
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                status: ArtifactStatus::Ready,
                title: "Wrong session artifact".to_owned(),
            })],
        )
        .await
        .expect("event appends");
    let harness =
        harness_with_event_blob_and_evidence_stores(event_store, blob_store, evidence_store).await;

    let refs = harness
        .list_artifact_content_evidence_refs(TenantId::SINGLE, &requested_session_id.to_string())
        .await
        .expect("artifact evidence refs list");

    assert!(refs.is_empty());
}

#[tokio::test]
async fn artifact_inspector_registers_content_ref_without_prior_worktree_read() {
    let workspace = unique_workspace("sdk-evidence-inspector-artifact");
    std::fs::create_dir_all(&workspace).expect("workspace exists");
    let session_id = SessionId::new();
    let revision_id = ArtifactRevisionId::new();
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let blob_store = Arc::new(InMemoryBlobStore::default());
    let evidence_store = Arc::new(EvidenceRefStore::new(
        Arc::new(InMemoryEvidenceRefRegistry::new()),
        blob_store.clone(),
    ));
    let bytes = Bytes::from_static(b"inspector artifact body");
    let content_hash = *blake3::hash(&bytes).as_bytes();
    let blob_ref = blob_store
        .put(
            TenantId::SINGLE,
            bytes.clone(),
            BlobMeta {
                content_type: Some("text/plain".to_owned()),
                size: bytes.len() as u64,
                content_hash,
                created_at: chrono::Utc::now(),
                retention: BlobRetention::SessionScoped(session_id),
            },
        )
        .await
        .expect("blob writes");
    let mut options = jyowo_harness_sdk::HarnessOptions::default();
    options.workspace_root = workspace.clone();
    options.model_id = "test-model".to_owned();
    let harness = Harness::builder()
        .with_options(options)
        .with_model(TestModelProvider::default())
        .with_store(event_store.clone())
        .with_sandbox(NoopSandbox::new())
        .with_blob_store_arc(blob_store)
        .with_evidence_ref_store_arc(evidence_store)
        .build()
        .await
        .expect("harness builds");
    harness
        .open_or_create_conversation_session(
            SessionOptions::new(workspace).with_session_id(session_id),
        )
        .await
        .expect("session opens");
    event_store
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                revision_id: revision_id.clone(),
                artifact_id: "artifact-inspector".to_owned(),
                at: chrono::Utc::now(),
                blob_ref: Some(blob_ref),
                content_hash: Some(content_hash.to_vec()),
                kind: "document".to_owned(),
                preview: Some("Inspector artifact".to_owned()),
                run_id: RunId::new(),
                session_id,
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                status: ArtifactStatus::Ready,
                title: "Inspector artifact".to_owned(),
            })],
        )
        .await
        .expect("event appends");

    let inspector = harness
        .get_conversation_inspector_item(
            &session_id.to_string(),
            ConversationInspectorSelection::ArtifactRevision {
                artifact_id: Some("artifact-inspector".to_owned()),
                revision_id: revision_id.to_string(),
            },
        )
        .await
        .expect("inspector loads");

    let content_ref = match inspector.item {
        ConversationInspectorItem::Artifact { segment } => segment
            .revision
            .content_ref
            .expect("artifact inspector revision should expose content ref"),
        other => panic!("expected artifact inspector item, got {other:?}"),
    };
    let content = harness
        .read_artifact_revision_content(TenantId::SINGLE, &session_id.to_string(), &content_ref)
        .await
        .expect("artifact content evidence reads");

    assert_eq!(content.bytes, bytes);
}

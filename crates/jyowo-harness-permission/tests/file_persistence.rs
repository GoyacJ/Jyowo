#![cfg(feature = "integrity")]

use std::fs;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    DecisionId, DecisionScope, PermissionPersistenceTamperedEvent, PersistenceTamperReason,
    RuleSource, TenantId,
};
use harness_permission::{
    DecisionPersistence, FileDecisionPersistence, IntegrityAlgorithm, PermissionTamperEventSink,
    PersistedDecision, StaticSignerStore,
};
use parking_lot::Mutex;

#[tokio::test]
async fn file_persistence_round_trips_signed_decisions() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("permissions.json");
    let persistence = FileDecisionPersistence::new(TenantId::SINGLE, &path, signer());
    let decision = persisted_decision();

    persistence.persist(decision.clone()).await.unwrap();

    assert!(persistence.supports_integrity());
    assert_eq!(persistence.load_decisions().await.unwrap(), vec![decision]);
}

#[tokio::test]
async fn file_persistence_rejects_tamper_and_emits_event() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("permissions.json");
    let sink = Arc::new(RecordingTamperSink::default());
    let persistence =
        FileDecisionPersistence::with_tamper_sink(TenantId::SINGLE, &path, signer(), sink.clone());

    persistence.persist(persisted_decision()).await.unwrap();
    let mut body = fs::read_to_string(&path).unwrap();
    body = body.replace("\"workspace\"", "\"project\"");
    fs::write(&path, body).unwrap();

    let error = persistence.load_decisions().await.unwrap_err();

    assert!(error.to_string().contains("integrity verification"));
    assert!(!path.exists());
    assert!(fs::read_dir(temp.path()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("tampered")));
    let events = sink.events.lock();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].tenant_id, TenantId::SINGLE);
    assert_eq!(events[0].reason, PersistenceTamperReason::SignatureMismatch);
}

#[derive(Default)]
struct RecordingTamperSink {
    events: Mutex<Vec<PermissionPersistenceTamperedEvent>>,
}

#[async_trait]
impl PermissionTamperEventSink for RecordingTamperSink {
    async fn emit(&self, event: PermissionPersistenceTamperedEvent) {
        self.events.lock().push(event);
    }
}

fn signer() -> Arc<dyn harness_permission::IntegritySigner> {
    StaticSignerStore::from_key(
        "test-key",
        b"test-key-material-with-enough-entropy".to_vec(),
        IntegrityAlgorithm::HmacSha256,
    )
    .unwrap()
}

fn persisted_decision() -> PersistedDecision {
    PersistedDecision {
        decision_id: DecisionId::new(),
        scope: DecisionScope::ToolName("read_blob".to_owned()),
        source: RuleSource::Workspace,
        fingerprint: None,
    }
}

#![cfg(feature = "integrity")]

use std::fs;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    Decision, DecisionId, DecisionScope, ExecFingerprint, PermissionMode,
    PermissionPersistenceTamperedEvent, PermissionSubject, PersistenceTamperReason, RuleSource,
    SessionId, TenantId,
};
use harness_permission::{
    DecisionHistory, DecisionLookup, DecisionPersistence, DecisionStore, FileDecisionPersistence,
    IntegrityAlgorithm, PermissionTamperEventSink, PersistedDecision, StaticSignerStore,
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

#[tokio::test]
async fn file_persistence_lookup_works_through_decision_store_trait_object() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("permissions.json");
    let persistence = Arc::new(FileDecisionPersistence::new(
        TenantId::SINGLE,
        &path,
        signer(),
    ));
    let decision = persisted_decision();
    persistence.persist(decision.clone()).await.unwrap();

    let store: Arc<dyn DecisionStore> = persistence;
    let found = store.find_scoped_decision(lookup()).await.unwrap();

    assert_eq!(found, Some(decision));
}

#[tokio::test]
async fn file_persistence_does_not_reuse_session_allow_across_sessions() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("permissions.json");
    let persistence = Arc::new(FileDecisionPersistence::new(
        TenantId::SINGLE,
        &path,
        signer(),
    ));
    let mut decision = persisted_decision();
    let session_id = SessionId::new();
    decision.decision = Decision::AllowSession;
    decision.session_id = Some(session_id);
    persistence.persist(decision.clone()).await.unwrap();

    let mut same_session_lookup = lookup();
    same_session_lookup.session_id = session_id;
    assert_eq!(
        persistence
            .find_scoped_decision(same_session_lookup)
            .await
            .unwrap(),
        Some(decision)
    );

    let mut other_session_lookup = lookup();
    other_session_lookup.session_id = SessionId::new();
    assert_eq!(
        persistence
            .find_scoped_decision(other_session_lookup)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn file_persistence_lookup_fails_closed_on_tamper() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("permissions.json");
    let sink = Arc::new(RecordingTamperSink::default());
    let persistence =
        FileDecisionPersistence::with_tamper_sink(TenantId::SINGLE, &path, signer(), sink.clone());

    persistence.persist(persisted_decision()).await.unwrap();
    let body = fs::read_to_string(&path)
        .unwrap()
        .replace("\"workspace\"", "\"project\"");
    fs::write(&path, body).unwrap();

    let error = persistence
        .find_scoped_decision(lookup())
        .await
        .unwrap_err();

    assert!(error.to_string().contains("integrity verification"));
    assert!(sink.events.lock().len() == 1);
}

#[tokio::test]
async fn file_persistence_unreadable_store_fails_closed_and_emits_event() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("permissions.json");
    fs::create_dir(&path).unwrap();
    let sink = Arc::new(RecordingTamperSink::default());
    let persistence =
        FileDecisionPersistence::with_tamper_sink(TenantId::SINGLE, &path, signer(), sink.clone());

    let error = persistence
        .find_scoped_decision(lookup())
        .await
        .unwrap_err();

    assert!(error.to_string().contains("read permission file"));
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
        decision: Decision::AllowPermanent,
        scope: DecisionScope::ToolName("read_blob".to_owned()),
        source: RuleSource::Workspace,
        session_id: None,
        fingerprint: Some(lookup_fingerprint()),
    }
}

fn lookup() -> DecisionLookup {
    DecisionLookup {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        requested_scope: DecisionScope::ToolName("read_blob".to_owned()),
        subject: PermissionSubject::ToolInvocation {
            tool: "read_blob".to_owned(),
            input: serde_json::json!({ "path": "README.md" }),
        },
        fingerprint: lookup_fingerprint(),
        decision_source: RuleSource::Workspace,
        permission_mode: PermissionMode::Default,
        looked_up_at: harness_contracts::now(),
    }
}

fn lookup_fingerprint() -> ExecFingerprint {
    ExecFingerprint([7; 32])
}

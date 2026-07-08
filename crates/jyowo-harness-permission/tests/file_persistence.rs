#![cfg(feature = "integrity")]

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    Decision, DecisionId, DecisionScope, ExecFingerprint, PermissionMode,
    PermissionPersistenceTamperedEvent, PermissionSubject, PersistenceTamperReason, RuleSource,
    SessionId, TenantId,
};
use harness_permission::{
    canonical_bytes, migrate_legacy_no_workspace_permission_decisions, DecisionHistory,
    DecisionLookup, DecisionPersistence, DecisionStore, FileDecisionPersistence,
    IntegrityAlgorithm, PermissionTamperEventSink, PersistedDecision, StaticSignerStore,
};
use parking_lot::Mutex;

#[tokio::test]
async fn file_persistence_round_trips_signed_decisions() {
    let temp = tempfile::tempdir().unwrap();
    let path = canonical_temp_root(&temp).join("permissions.json");
    let persistence = FileDecisionPersistence::new(TenantId::SINGLE, &path, signer());
    let decision = persisted_decision();

    persistence.persist(decision.clone()).await.unwrap();

    assert!(persistence.supports_integrity());
    assert_eq!(persistence.load_decisions().await.unwrap(), vec![decision]);
}

#[tokio::test]
async fn file_persistence_rejects_tamper_and_emits_event() {
    let temp = tempfile::tempdir().unwrap();
    let temp_root = canonical_temp_root(&temp);
    let path = temp_root.join("permissions.json");
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
    assert!(fs::read_dir(temp_root).unwrap().any(|entry| entry
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
async fn file_persistence_rejects_signed_decision_from_other_tenant() {
    let temp = tempfile::tempdir().unwrap();
    let path = canonical_temp_root(&temp).join("permissions.json");
    let sink = Arc::new(RecordingTamperSink::default());
    let persistence =
        FileDecisionPersistence::with_tamper_sink(TenantId::SINGLE, &path, signer(), sink.clone());
    persistence.persist(persisted_decision()).await.unwrap();

    let other_tenant =
        FileDecisionPersistence::with_tamper_sink(TenantId::SHARED, &path, signer(), sink.clone());
    let error = other_tenant.load_decisions().await.unwrap_err();

    assert!(error.to_string().contains("integrity verification"));
    assert!(!path.exists());
    let events = sink.events.lock();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].tenant_id, TenantId::SHARED);
    assert_eq!(events[0].reason, PersistenceTamperReason::SignatureMismatch);
}

#[tokio::test]
async fn file_persistence_loads_legacy_single_tenant_signed_decision() {
    let temp = tempfile::tempdir().unwrap();
    let path = canonical_temp_root(&temp).join("permissions.json");
    let decision = persisted_decision();
    write_legacy_signed_decision(&path, &decision).await;
    let persistence = FileDecisionPersistence::new(TenantId::SINGLE, &path, signer());

    let loaded = persistence.load_decisions().await.unwrap();

    assert_eq!(loaded, vec![decision]);
}

#[tokio::test]
async fn migrates_legacy_no_workspace_decisions_to_conversation_scope() {
    let temp = tempfile::tempdir().unwrap();
    let path = canonical_temp_root(&temp).join("permission-decisions.json");
    let conversation_id = SessionId::new();
    let mut decision = persisted_decision();
    decision.session_id = Some(conversation_id);
    write_legacy_signed_decision(&path, &decision).await;

    migrate_legacy_no_workspace_permission_decisions(&path, TenantId::SINGLE, signer())
        .await
        .unwrap();

    let scoped = FileDecisionPersistence::new(TenantId::SINGLE, &path, signer())
        .with_no_workspace_conversation_scope(conversation_id);
    assert_eq!(scoped.load_decisions().await.unwrap(), vec![decision]);
    let unscoped = FileDecisionPersistence::new(TenantId::SINGLE, &path, signer());
    assert!(unscoped.load_decisions().await.unwrap().is_empty());
}

#[tokio::test]
async fn quarantines_legacy_no_workspace_decision_without_session_id() {
    let temp = tempfile::tempdir().unwrap();
    let path = canonical_temp_root(&temp).join("permission-decisions.json");
    write_legacy_signed_decision(&path, &persisted_decision()).await;

    let migrated =
        migrate_legacy_no_workspace_permission_decisions(&path, TenantId::SINGLE, signer())
            .await
            .unwrap();

    assert!(migrated);
    assert!(
        FileDecisionPersistence::new(TenantId::SINGLE, &path, signer())
            .load_decisions()
            .await
            .unwrap()
            .is_empty()
    );
    assert!(path
        .with_file_name("permission-decisions.json.unscoped-legacy.json")
        .exists());
}

#[tokio::test]
async fn file_persistence_rejects_legacy_signed_decision_with_injected_runtime_scope() {
    let temp = tempfile::tempdir().unwrap();
    let path = canonical_temp_root(&temp)
        .join("global-conversations")
        .join("permission-decisions.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let conversation_id = SessionId::new();
    let decision = persisted_decision();
    write_legacy_signed_decision(&path, &decision).await;
    let mut records: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    records[0]["runtime_scope"] = serde_json::json!({
        "kind": "no_workspace_conversation",
        "conversation_id": conversation_id,
    });
    fs::write(&path, serde_json::to_vec_pretty(&records).unwrap()).unwrap();
    let persistence = FileDecisionPersistence::new(TenantId::SINGLE, &path, signer())
        .with_no_workspace_conversation_scope(conversation_id);

    let error = persistence.load_decisions().await.unwrap_err();

    assert!(error.to_string().contains("integrity verification"));
    assert!(!path.exists());
}

#[tokio::test]
async fn file_persistence_rejects_legacy_signed_decision_for_shared_tenant() {
    let temp = tempfile::tempdir().unwrap();
    let path = canonical_temp_root(&temp).join("permissions.json");
    write_legacy_signed_decision(&path, &persisted_decision()).await;
    let persistence = FileDecisionPersistence::new(TenantId::SHARED, &path, signer());

    let error = persistence.load_decisions().await.unwrap_err();

    assert!(error.to_string().contains("integrity verification"));
    assert!(!path.exists());
}

#[tokio::test]
async fn file_persistence_lookup_works_through_decision_store_trait_object() {
    let temp = tempfile::tempdir().unwrap();
    let path = canonical_temp_root(&temp).join("permissions.json");
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
    let path = canonical_temp_root(&temp).join("permissions.json");
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
async fn file_persistence_no_workspace_permanent_allow_is_conversation_scoped() {
    let temp = tempfile::tempdir().unwrap();
    let path = canonical_temp_root(&temp)
        .join("global-conversations")
        .join("permission-decisions.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let first_conversation = SessionId::new();
    let second_conversation = SessionId::new();
    let first_persistence = Arc::new(
        FileDecisionPersistence::new(TenantId::SINGLE, &path, signer())
            .with_no_workspace_conversation_scope(first_conversation),
    );
    let second_persistence = Arc::new(
        FileDecisionPersistence::new(TenantId::SINGLE, &path, signer())
            .with_no_workspace_conversation_scope(second_conversation),
    );
    let mut decision = persisted_decision();
    decision.source = RuleSource::User;

    first_persistence.persist(decision.clone()).await.unwrap();
    let records: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("decision file should exist")).unwrap();
    assert_eq!(
        records[0]["runtime_scope"],
        serde_json::json!({
            "kind": "no_workspace_conversation",
            "conversation_id": first_conversation,
        })
    );

    let mut first_lookup = lookup();
    first_lookup.decision_source = RuleSource::User;
    first_lookup.session_id = first_conversation;
    assert_eq!(
        first_persistence
            .find_scoped_decision(first_lookup)
            .await
            .unwrap(),
        Some(decision)
    );

    let mut second_lookup = lookup();
    second_lookup.decision_source = RuleSource::User;
    second_lookup.session_id = second_conversation;
    assert_eq!(
        second_persistence
            .find_scoped_decision(second_lookup)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn file_persistence_removes_no_workspace_conversation_scope() {
    let temp = tempfile::tempdir().unwrap();
    let path = canonical_temp_root(&temp)
        .join("global-conversations")
        .join("permission-decisions.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let first_conversation = SessionId::new();
    let second_conversation = SessionId::new();
    let first_persistence = FileDecisionPersistence::new(TenantId::SINGLE, &path, signer())
        .with_no_workspace_conversation_scope(first_conversation);
    let second_persistence = FileDecisionPersistence::new(TenantId::SINGLE, &path, signer())
        .with_no_workspace_conversation_scope(second_conversation);

    let mut first_decision = persisted_decision();
    first_decision.session_id = Some(first_conversation);
    let mut second_decision = persisted_decision();
    second_decision.decision_id = DecisionId::new();
    second_decision.session_id = Some(second_conversation);

    first_persistence
        .persist(first_decision.clone())
        .await
        .unwrap();
    second_persistence
        .persist(second_decision.clone())
        .await
        .unwrap();

    FileDecisionPersistence::new(TenantId::SINGLE, &path, signer())
        .remove_no_workspace_conversation_scope(first_conversation)
        .await
        .unwrap();

    assert_eq!(
        first_persistence.load_decisions().await.unwrap(),
        Vec::new()
    );
    assert_eq!(
        second_persistence.load_decisions().await.unwrap(),
        vec![second_decision]
    );
}

#[tokio::test]
async fn file_persistence_no_workspace_ignores_legacy_record_without_conversation_scope() {
    let temp = tempfile::tempdir().unwrap();
    let path = canonical_temp_root(&temp)
        .join("global-conversations")
        .join("permission-decisions.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let conversation_id = SessionId::new();
    let unscoped_persistence = FileDecisionPersistence::new(TenantId::SINGLE, &path, signer());
    let scoped_persistence = FileDecisionPersistence::new(TenantId::SINGLE, &path, signer())
        .with_no_workspace_conversation_scope(conversation_id);
    let mut decision = persisted_decision();
    decision.source = RuleSource::User;
    unscoped_persistence.persist(decision).await.unwrap();

    let mut scoped_lookup = lookup();
    scoped_lookup.decision_source = RuleSource::User;
    scoped_lookup.session_id = conversation_id;
    assert_eq!(
        scoped_persistence
            .find_scoped_decision(scoped_lookup)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn file_persistence_concurrent_persists_do_not_drop_decisions() {
    let temp = tempfile::tempdir().unwrap();
    let path = canonical_temp_root(&temp).join("permissions.json");
    let persistence = Arc::new(FileDecisionPersistence::new(
        TenantId::SINGLE,
        &path,
        signer(),
    ));
    let mut tasks = tokio::task::JoinSet::new();

    for _ in 0..20 {
        let persistence = Arc::clone(&persistence);
        tasks.spawn(async move {
            persistence.persist(persisted_decision()).await.unwrap();
        });
    }

    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }

    let loaded = persistence.load_decisions().await.unwrap();
    assert_eq!(loaded.len(), 20);
}

#[tokio::test]
async fn file_persistence_lookup_fails_closed_on_tamper() {
    let temp = tempfile::tempdir().unwrap();
    let path = canonical_temp_root(&temp).join("permissions.json");
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
    let path = canonical_temp_root(&temp).join("permissions.json");
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

#[cfg(unix)]
#[tokio::test]
async fn file_persistence_rejects_symlink_parent_without_writing_target() {
    let temp = tempfile::tempdir().unwrap();
    let temp_root = canonical_temp_root(&temp);
    let external = tempfile::tempdir().unwrap();
    let symlinked_parent = temp_root.join("permissions");
    std::os::unix::fs::symlink(external.path(), &symlinked_parent).unwrap();
    let path = symlinked_parent.join("permission-decisions.json");
    let persistence = FileDecisionPersistence::new(TenantId::SINGLE, &path, signer());

    let error = persistence.persist(persisted_decision()).await.unwrap_err();

    assert!(error.to_string().contains("symlink"));
    assert!(!external.path().join("permission-decisions.json").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn file_persistence_rejects_symlink_decision_file_without_reading_target() {
    let temp = tempfile::tempdir().unwrap();
    let temp_root = canonical_temp_root(&temp);
    let external = tempfile::NamedTempFile::new().unwrap();
    fs::write(external.path(), b"[]").unwrap();
    let path = temp_root.join("permission-decisions.json");
    std::os::unix::fs::symlink(external.path(), &path).unwrap();
    let sink = Arc::new(RecordingTamperSink::default());
    let persistence =
        FileDecisionPersistence::with_tamper_sink(TenantId::SINGLE, &path, signer(), sink.clone());

    let error = persistence.load_decisions().await.unwrap_err();

    assert!(error.to_string().contains("symlink"));
    assert!(external.path().exists());
    assert!(std::fs::symlink_metadata(&path)
        .expect("link metadata")
        .file_type()
        .is_symlink());
    assert!(!fs::read_dir(temp_root).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("tampered")));
    let events = sink.events.lock();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].tenant_id, TenantId::SINGLE);
    assert_eq!(events[0].reason, PersistenceTamperReason::SignatureMismatch);
}

#[cfg(unix)]
#[tokio::test]
async fn file_persistence_creates_owner_only_decision_file() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let path = canonical_temp_root(&temp).join("permissions.json");
    let persistence = FileDecisionPersistence::new(TenantId::SINGLE, &path, signer());

    persistence.persist(persisted_decision()).await.unwrap();

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[cfg(unix)]
#[tokio::test]
async fn file_persistence_load_tightens_existing_decision_file() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let path = canonical_temp_root(&temp).join("permissions.json");
    let persistence = FileDecisionPersistence::new(TenantId::SINGLE, &path, signer());
    persistence.persist(persisted_decision()).await.unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

    persistence.load_decisions().await.unwrap();

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[derive(Default)]
struct RecordingTamperSink {
    events: Mutex<Vec<PermissionPersistenceTamperedEvent>>,
}

fn canonical_temp_root(temp: &tempfile::TempDir) -> PathBuf {
    temp.path().canonicalize().expect("canonical tempdir")
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

async fn write_legacy_signed_decision(path: &std::path::Path, decision: &PersistedDecision) {
    let signer = signer();
    let recorded_at = harness_contracts::now();
    let unsigned = serde_json::json!({
        "decision_id": decision.decision_id,
        "decision": decision.decision,
        "scope": decision.scope,
        "source": decision.source,
        "session_id": decision.session_id,
        "fingerprint": decision.fingerprint,
        "recorded_at": recorded_at,
    });
    let payload = canonical_bytes(&unsigned).unwrap();
    let signature = signer.sign(&payload).await.unwrap();
    let algorithm = match signature.algorithm {
        IntegrityAlgorithm::HmacSha256 => "hmac_sha256",
        IntegrityAlgorithm::HmacSha512 => "hmac_sha512",
    };
    let record = serde_json::json!([{
        "decision_id": decision.decision_id,
        "decision": decision.decision,
        "scope": decision.scope,
        "source": decision.source,
        "session_id": decision.session_id,
        "fingerprint": decision.fingerprint,
        "recorded_at": recorded_at,
        "signature": {
            "algorithm": algorithm,
            "key_id": signature.key_id,
            "mac_hex": hex_bytes(&signature.mac),
            "signed_at": signature.signed_at,
        }
    }]);
    fs::write(path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

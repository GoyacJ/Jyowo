use std::fmt::Write as _;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fs2::FileExt;
use harness_contracts::{
    Decision, DecisionId, DecisionScope, ExecFingerprint, PermissionError,
    PermissionPersistenceTamperedEvent, PersistenceTamperReason, RuleSource, SessionId, TenantId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    canonical_bytes, policy_scope_matches_request, DecisionHistory, DecisionLookup,
    DecisionPersistence, IntegrityAlgorithm, IntegrityError, IntegritySignature, IntegritySigner,
    PersistedDecision,
};

// ── Error conversion ────────────────────────────────────────────────

fn fs_err(error: harness_fs::FsError) -> PermissionError {
    PermissionError::Message(error.to_string())
}

// ── FileDecisionPersistence ─────────────────────────────────────────

pub struct FileDecisionPersistence {
    tenant_id: TenantId,
    runtime_scope: Option<DecisionRuntimeScope>,
    lock_path: PathBuf,
    path: PathBuf,
    signer: Arc<dyn IntegritySigner>,
    tamper_sink: Arc<dyn PermissionTamperEventSink>,
    lock: tokio::sync::Mutex<()>,
}

#[async_trait]
pub trait PermissionTamperEventSink: Send + Sync + 'static {
    async fn emit(&self, event: PermissionPersistenceTamperedEvent);
}

#[derive(Debug, Default)]
pub struct NoopPermissionTamperEventSink;

#[async_trait]
impl PermissionTamperEventSink for NoopPermissionTamperEventSink {
    async fn emit(&self, _event: PermissionPersistenceTamperedEvent) {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SignedDecisionRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tenant_id: Option<TenantId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runtime_scope: Option<DecisionRuntimeScope>,
    decision_id: DecisionId,
    decision: Decision,
    scope: DecisionScope,
    source: RuleSource,
    session_id: Option<harness_contracts::SessionId>,
    fingerprint: Option<ExecFingerprint>,
    recorded_at: DateTime<Utc>,
    signature: StoredSignature,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DecisionRuntimeScope {
    NoWorkspaceConversation { conversation_id: SessionId },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSignature {
    algorithm: String,
    key_id: String,
    mac_hex: String,
    signed_at: DateTime<Utc>,
}

impl FileDecisionPersistence {
    #[must_use]
    pub fn new(
        tenant_id: TenantId,
        path: impl Into<PathBuf>,
        signer: Arc<dyn IntegritySigner>,
    ) -> Self {
        Self::with_tamper_sink(
            tenant_id,
            path,
            signer,
            Arc::new(NoopPermissionTamperEventSink),
        )
    }

    #[must_use]
    pub fn with_tamper_sink(
        tenant_id: TenantId,
        path: impl Into<PathBuf>,
        signer: Arc<dyn IntegritySigner>,
        tamper_sink: Arc<dyn PermissionTamperEventSink>,
    ) -> Self {
        let path = path.into();
        Self {
            tenant_id,
            runtime_scope: None,
            lock_path: lock_path_for(&path),
            path,
            signer,
            tamper_sink,
            lock: tokio::sync::Mutex::new(()),
        }
    }

    #[must_use]
    pub fn with_no_workspace_conversation_scope(mut self, conversation_id: SessionId) -> Self {
        self.runtime_scope =
            Some(DecisionRuntimeScope::NoWorkspaceConversation { conversation_id });
        self
    }

    pub async fn remove_no_workspace_conversation_scope(
        &self,
        conversation_id: SessionId,
    ) -> Result<(), PermissionError> {
        let target_scope = DecisionRuntimeScope::NoWorkspaceConversation { conversation_id };
        let _guard = self.lock.lock().await;
        let lock_file = self.open_lock_file()?;
        lock_file.lock_exclusive().map_err(|err| {
            PermissionError::Message(format!("lock permission decision file: {err}"))
        })?;
        let mut records = match self.load_records().await {
            Ok(records) => records,
            Err(error) => {
                let _ = lock_file.unlock();
                return Err(error);
            }
        };
        let original_len = records.len();
        records.retain(|record| record.runtime_scope.as_ref() != Some(&target_scope));
        let result = if records.len() == original_len {
            Ok(())
        } else {
            harness_fs::write_json_file_atomic(&self.path, &records, true).map_err(fs_err)
        };
        let unlock_result = lock_file.unlock().map_err(|err| {
            PermissionError::Message(format!("unlock permission decision file: {err}"))
        });
        match (result, unlock_result) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    pub async fn load_decisions(&self) -> Result<Vec<PersistedDecision>, PermissionError> {
        let records = self.load_records().await?;
        Ok(records
            .into_iter()
            .filter(|record| self.runtime_scope_matches(record.runtime_scope.as_ref()))
            .map(record_to_persisted_decision)
            .collect())
    }

    async fn load_records(&self) -> Result<Vec<SignedDecisionRecord>, PermissionError> {
        if let Err(err) = harness_fs::ensure_no_symlink_components(&self.path) {
            self.report_tamper(None, PersistenceTamperReason::SignatureMismatch)
                .await;
            return Err(fs_err(err));
        }
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                self.report_tamper(None, PersistenceTamperReason::SignatureMismatch)
                    .await;
                return Err(PermissionError::Message(
                    "read permission file: permission file path must not use symlinks".to_owned(),
                ));
            }
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => {
                self.report_tamper(None, PersistenceTamperReason::SignatureMismatch)
                    .await;
                self.rename_tampered_file()?;
                return Err(PermissionError::Message(
                    "read permission file: permission file path is not a file".to_owned(),
                ));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => {
                self.report_tamper(None, PersistenceTamperReason::SignatureMismatch)
                    .await;
                self.rename_tampered_file()?;
                return Err(PermissionError::Message(format!(
                    "read permission file metadata: {err}"
                )));
            }
        };
        let mut file = match open_no_follow_for_read(&self.path) {
            Ok(file) => file,
            Err(err) => {
                self.report_tamper(None, PersistenceTamperReason::SignatureMismatch)
                    .await;
                self.rename_tampered_file()?;
                return Err(PermissionError::Message(format!(
                    "read permission file: {err}"
                )));
            }
        };
        if let Err(err) = harness_fs::set_owner_only_file_if_unix(&file) {
            self.report_tamper(None, PersistenceTamperReason::SignatureMismatch)
                .await;
            self.rename_tampered_file()?;
            return Err(PermissionError::Message(format!(
                "read permission file: {err}"
            )));
        }
        let mut bytes = Vec::new();
        if let Err(err) = file.read_to_end(&mut bytes) {
            self.report_tamper(None, PersistenceTamperReason::SignatureMismatch)
                .await;
            self.rename_tampered_file()?;
            return Err(PermissionError::Message(format!(
                "read permission file: {err}"
            )));
        }
        let records: Vec<SignedDecisionRecord> = match serde_json::from_slice(&bytes) {
            Ok(records) => records,
            Err(err) => {
                self.report_tamper(None, PersistenceTamperReason::SignatureMismatch)
                    .await;
                self.rename_tampered_file()?;
                return Err(PermissionError::Message(format!(
                    "permission decision file is not valid JSON: {err}"
                )));
            }
        };

        for record in &records {
            if let Err(err) = self.verify_record(record).await {
                let reason = tamper_reason(&err);
                self.report_tamper(record.fingerprint, reason).await;
                self.rename_tampered_file()?;
                return Err(PermissionError::Message(format!(
                    "permission decision file failed integrity verification: {err}"
                )));
            }
        }

        Ok(records)
    }

    async fn sign_record(
        &self,
        decision: PersistedDecision,
    ) -> Result<SignedDecisionRecord, PermissionError> {
        let recorded_at = Utc::now();
        let unsigned = unsigned_record_value(
            self.tenant_id,
            decision.decision_id,
            &decision.decision,
            &decision.scope,
            decision.source,
            decision.session_id,
            decision.fingerprint,
            self.runtime_scope.as_ref(),
            recorded_at,
        );
        let payload = canonical_bytes(&unsigned)
            .map_err(|err| PermissionError::Message(format!("canonicalize decision: {err}")))?;
        let signature = self.signer.sign(&payload).await?;

        Ok(SignedDecisionRecord {
            tenant_id: Some(self.tenant_id),
            runtime_scope: self.runtime_scope.clone(),
            decision_id: decision.decision_id,
            decision: decision.decision,
            scope: decision.scope,
            source: decision.source,
            session_id: decision.session_id,
            fingerprint: decision.fingerprint,
            recorded_at,
            signature: StoredSignature::from_signature(signature),
        })
    }

    async fn verify_record(&self, record: &SignedDecisionRecord) -> Result<(), IntegrityError> {
        let signature = record.signature.to_signature()?;
        let unsigned = match record.tenant_id {
            Some(tenant_id) if tenant_id == self.tenant_id => unsigned_record_value(
                tenant_id,
                record.decision_id,
                &record.decision,
                &record.scope,
                record.source,
                record.session_id,
                record.fingerprint,
                record.runtime_scope.as_ref(),
                record.recorded_at,
            ),
            Some(_) => return Err(IntegrityError::Mismatch),
            None if self.tenant_id == TenantId::SINGLE && record.runtime_scope.is_none() => {
                legacy_unsigned_record_value(
                    record.decision_id,
                    &record.decision,
                    &record.scope,
                    record.source,
                    record.session_id,
                    record.fingerprint,
                    record.recorded_at,
                )
            }
            None => return Err(IntegrityError::Mismatch),
        };
        let payload = canonical_bytes(&unsigned)?;
        self.signer.verify(&payload, &signature).await
    }

    fn runtime_scope_matches(&self, runtime_scope: Option<&DecisionRuntimeScope>) -> bool {
        match self.runtime_scope.as_ref() {
            Some(expected) => runtime_scope == Some(expected),
            None => runtime_scope.is_none(),
        }
    }

    async fn report_tamper(
        &self,
        fingerprint: Option<ExecFingerprint>,
        reason: PersistenceTamperReason,
    ) {
        self.tamper_sink
            .emit(PermissionPersistenceTamperedEvent {
                tenant_id: self.tenant_id,
                file_path_hash: path_hash(&self.path),
                fingerprint,
                reason,
                key_id: self.signer.key_id().to_owned(),
                at: Utc::now(),
            })
            .await;
    }

    fn rename_tampered_file(&self) -> Result<(), PermissionError> {
        #[cfg(unix)]
        {
            let parent =
                harness_fs::open_parent_dir_no_symlink_for_write(&self.path).map_err(fs_err)?;
            if parent
                .try_open_existing_file(parent.file_name())
                .map_err(fs_err)?
                .is_none()
            {
                return Ok(());
            }
            let tampered_path = tampered_path(&self.path);
            let tampered_name = tampered_path.file_name().ok_or_else(|| {
                PermissionError::Message("tampered permission path has no file name".to_owned())
            })?;
            parent
                .rename_file(parent.file_name(), tampered_name)
                .map_err(fs_err)?;
            parent.sync_all().map_err(fs_err)?;
            return Ok(());
        }

        #[cfg(not(unix))]
        {
            if !self.path.exists() {
                return Ok(());
            }

            let tampered_path = tampered_path(&self.path);
            fs::rename(&self.path, &tampered_path).map_err(|err| {
                PermissionError::Message(format!(
                    "rename tampered permission file `{}`: {err}",
                    self.path.display()
                ))
            })
        }
    }

    fn open_lock_file(&self) -> Result<fs::File, PermissionError> {
        #[cfg(unix)]
        {
            let parent = harness_fs::open_parent_dir_no_symlink_for_write(&self.lock_path)
                .map_err(fs_err)?;
            let lock_file = parent
                .open_or_create_read_write_file(parent.file_name())
                .map_err(fs_err)?;
            harness_fs::set_owner_only_file_if_unix(&lock_file).map_err(fs_err)?;
            parent.sync_all().map_err(fs_err)?;
            return Ok(lock_file);
        }

        #[cfg(not(unix))]
        {
            let parent = self.path.parent().ok_or_else(|| {
                PermissionError::Message("permission file path has no parent".to_owned())
            })?;
            harness_fs::ensure_app_dir_no_symlink(parent).map_err(fs_err)?;
            harness_fs::ensure_no_symlink_components(&self.lock_path).map_err(fs_err)?;
            let mut open_options = fs::OpenOptions::new();
            open_options.create(true).read(true).write(true);
            let lock_file = open_options.open(&self.lock_path).map_err(|err| {
                PermissionError::Message(format!("open permission lock file: {err}"))
            })?;
            harness_fs::set_owner_only_file_if_unix(&lock_file).map_err(fs_err)?;
            harness_fs::sync_directory(parent).map_err(fs_err)?;
            Ok(lock_file)
        }
    }
}

#[async_trait]
impl DecisionPersistence for FileDecisionPersistence {
    fn supports_integrity(&self) -> bool {
        true
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        let signed = self.sign_record(decision).await?;
        let _guard = self.lock.lock().await;
        let lock_file = self.open_lock_file()?;
        lock_file.lock_exclusive().map_err(|err| {
            PermissionError::Message(format!("lock permission decision file: {err}"))
        })?;
        let mut records = match self.load_records().await {
            Ok(records) => records,
            Err(error) => {
                let _ = lock_file.unlock();
                return Err(error);
            }
        };
        records.push(signed);
        let result = harness_fs::write_json_file_atomic(&self.path, &records, true);
        let unlock_result = lock_file.unlock().map_err(|err| {
            PermissionError::Message(format!("unlock permission decision file: {err}"))
        });
        match (result, unlock_result) {
            (Err(error), _) => Err(fs_err(error)),
            (Ok(_), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }
}

#[async_trait]
impl DecisionHistory for FileDecisionPersistence {
    async fn find_scoped_decision(
        &self,
        lookup: DecisionLookup,
    ) -> Result<Option<PersistedDecision>, PermissionError> {
        if lookup.tenant_id != self.tenant_id {
            return Ok(None);
        }

        let decisions = self.load_decisions().await?;
        Ok(decisions.into_iter().find(|decision| {
            decision.source == lookup.decision_source
                && session_scope_matches(decision, &lookup)
                && policy_scope_matches_request(&decision.scope, &lookup.requested_scope)
                && fingerprint_matches(decision.fingerprint, lookup.fingerprint)
        }))
    }
}

fn record_to_persisted_decision(record: SignedDecisionRecord) -> PersistedDecision {
    PersistedDecision {
        decision_id: record.decision_id,
        decision: record.decision,
        scope: record.scope,
        source: record.source,
        session_id: record.session_id,
        fingerprint: record.fingerprint,
    }
}

fn fingerprint_matches(
    decision_fingerprint: Option<ExecFingerprint>,
    lookup_fingerprint: ExecFingerprint,
) -> bool {
    match decision_fingerprint {
        Some(fingerprint) => fingerprint == lookup_fingerprint,
        None => false,
    }
}

fn session_scope_matches(decision: &PersistedDecision, lookup: &DecisionLookup) -> bool {
    match decision.decision {
        Decision::AllowSession => decision.session_id == Some(lookup.session_id),
        _ => true,
    }
}

impl StoredSignature {
    fn from_signature(signature: IntegritySignature) -> Self {
        Self {
            algorithm: algorithm_name(signature.algorithm).to_owned(),
            key_id: signature.key_id,
            mac_hex: to_hex(&signature.mac),
            signed_at: signature.signed_at,
        }
    }

    fn to_signature(&self) -> Result<IntegritySignature, IntegrityError> {
        Ok(IntegritySignature {
            algorithm: parse_algorithm(&self.algorithm)?,
            key_id: self.key_id.clone(),
            mac: bytes::Bytes::from(
                from_hex(&self.mac_hex).map_err(|()| IntegrityError::Mismatch)?,
            ),
            signed_at: self.signed_at,
        })
    }
}

fn unsigned_record_value(
    tenant_id: TenantId,
    decision_id: DecisionId,
    decision: &Decision,
    scope: &DecisionScope,
    source: RuleSource,
    session_id: Option<harness_contracts::SessionId>,
    fingerprint: Option<ExecFingerprint>,
    runtime_scope: Option<&DecisionRuntimeScope>,
    recorded_at: DateTime<Utc>,
) -> Value {
    let mut value = json!({
        "tenant_id": tenant_id,
        "decision_id": decision_id,
        "decision": decision,
        "scope": scope,
        "source": source,
        "session_id": session_id,
        "fingerprint": fingerprint,
        "recorded_at": recorded_at,
    });
    if let Some(runtime_scope) = runtime_scope {
        value
            .as_object_mut()
            .expect("unsigned decision record is an object")
            .insert("runtime_scope".to_owned(), json!(runtime_scope));
    }
    value
}

fn legacy_unsigned_record_value(
    decision_id: DecisionId,
    decision: &Decision,
    scope: &DecisionScope,
    source: RuleSource,
    session_id: Option<harness_contracts::SessionId>,
    fingerprint: Option<ExecFingerprint>,
    recorded_at: DateTime<Utc>,
) -> Value {
    json!({
        "decision_id": decision_id,
        "decision": decision,
        "scope": scope,
        "source": source,
        "session_id": session_id,
        "fingerprint": fingerprint,
        "recorded_at": recorded_at,
    })
}

fn tamper_reason(err: &IntegrityError) -> PersistenceTamperReason {
    match err {
        IntegrityError::Mismatch => PersistenceTamperReason::SignatureMismatch,
        IntegrityError::UnknownKeyId(_) => PersistenceTamperReason::UnknownKeyId,
        IntegrityError::AlgorithmDowngrade { .. } => PersistenceTamperReason::AlgorithmDowngrade,
        IntegrityError::Missing => PersistenceTamperReason::MissingSignature,
    }
}

fn parse_algorithm(value: &str) -> Result<IntegrityAlgorithm, IntegrityError> {
    match value {
        "hmac_sha256" => Ok(IntegrityAlgorithm::HmacSha256),
        "hmac_sha512" => Ok(IntegrityAlgorithm::HmacSha512),
        _ => Err(IntegrityError::Mismatch),
    }
}

fn algorithm_name(algorithm: IntegrityAlgorithm) -> &'static str {
    match algorithm {
        IntegrityAlgorithm::HmacSha256 => "hmac_sha256",
        IntegrityAlgorithm::HmacSha512 => "hmac_sha512",
    }
}

fn path_hash(path: &Path) -> [u8; 32] {
    *blake3::hash(path.to_string_lossy().as_bytes()).as_bytes()
}

fn tampered_path(path: &Path) -> PathBuf {
    let suffix = Utc::now().format("%Y%m%d%H%M%S%f").to_string();
    let mut file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "permission-decisions".to_owned());
    file_name.push_str(".tampered.");
    file_name.push_str(&suffix);
    path.with_file_name(file_name)
}

fn lock_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("permission-decisions.json");
    path.with_file_name(format!("{file_name}.lock"))
}

// ── Remaining local helpers ─────────────────────────────────────────

#[cfg(unix)]
fn open_no_follow_for_read(path: &Path) -> Result<fs::File, PermissionError> {
    let Some(parent) = harness_fs::open_parent_dir_no_symlink_for_read(path).map_err(fs_err)?
    else {
        return Err(PermissionError::Message(
            "open permission file: not found".to_owned(),
        ));
    };
    Ok(parent
        .open_existing_file(parent.file_name())
        .map_err(fs_err)?)
}

#[cfg(not(unix))]
fn open_no_follow_for_read(path: &Path) -> Result<fs::File, PermissionError> {
    fs::File::open(path)
        .map_err(|err| PermissionError::Message(format!("open permission file: {err}")))
}

fn to_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("write to string");
    }
    output
}

fn from_hex(value: &str) -> Result<Vec<u8>, ()> {
    if value.len() % 2 != 0 {
        return Err(());
    }

    let mut bytes = Vec::with_capacity(value.len() / 2);
    for index in (0..value.len()).step_by(2) {
        let byte = u8::from_str_radix(&value[index..index + 2], 16).map_err(|_| ())?;
        bytes.push(byte);
    }
    Ok(bytes)
}

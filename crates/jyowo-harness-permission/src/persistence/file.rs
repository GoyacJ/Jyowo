use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use harness_contracts::{
    DecisionId, DecisionScope, ExecFingerprint, PermissionError,
    PermissionPersistenceTamperedEvent, PersistenceTamperReason, RuleSource, TenantId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    canonical_bytes, DecisionPersistence, IntegrityAlgorithm, IntegrityError, IntegritySignature,
    IntegritySigner, PersistedDecision,
};

pub struct FileDecisionPersistence {
    tenant_id: TenantId,
    path: PathBuf,
    signer: Arc<dyn IntegritySigner>,
    tamper_sink: Arc<dyn PermissionTamperEventSink>,
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
    decision_id: DecisionId,
    scope: DecisionScope,
    source: RuleSource,
    fingerprint: Option<ExecFingerprint>,
    recorded_at: DateTime<Utc>,
    signature: StoredSignature,
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
        Self {
            tenant_id,
            path: path.into(),
            signer,
            tamper_sink,
        }
    }

    pub async fn load_decisions(&self) -> Result<Vec<PersistedDecision>, PermissionError> {
        let records = self.load_records().await?;
        Ok(records
            .into_iter()
            .map(|record| PersistedDecision {
                decision_id: record.decision_id,
                scope: record.scope,
                source: record.source,
                fingerprint: record.fingerprint,
            })
            .collect())
    }

    async fn load_records(&self) -> Result<Vec<SignedDecisionRecord>, PermissionError> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let bytes = fs::read(&self.path)
            .map_err(|err| PermissionError::Message(format!("read permission file: {err}")))?;
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
            decision.decision_id,
            &decision.scope,
            decision.source,
            decision.fingerprint,
            recorded_at,
        );
        let payload = canonical_bytes(&unsigned)
            .map_err(|err| PermissionError::Message(format!("canonicalize decision: {err}")))?;
        let signature = self.signer.sign(&payload).await?;

        Ok(SignedDecisionRecord {
            decision_id: decision.decision_id,
            scope: decision.scope,
            source: decision.source,
            fingerprint: decision.fingerprint,
            recorded_at,
            signature: StoredSignature::from_signature(signature),
        })
    }

    async fn verify_record(&self, record: &SignedDecisionRecord) -> Result<(), IntegrityError> {
        let signature = record.signature.to_signature()?;
        let unsigned = unsigned_record_value(
            record.decision_id,
            &record.scope,
            record.source,
            record.fingerprint,
            record.recorded_at,
        );
        let payload = canonical_bytes(&unsigned)?;
        self.signer.verify(&payload, &signature).await
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

#[async_trait]
impl DecisionPersistence for FileDecisionPersistence {
    fn supports_integrity(&self) -> bool {
        true
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        let mut records = self.load_records().await?;
        records.push(self.sign_record(decision).await?);

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                PermissionError::Message(format!("create permission directory: {err}"))
            })?;
        }

        let bytes = serde_json::to_vec_pretty(&records)
            .map_err(|err| PermissionError::Message(format!("encode permission file: {err}")))?;
        fs::write(&self.path, bytes)
            .map_err(|err| PermissionError::Message(format!("write permission file: {err}")))
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
    decision_id: DecisionId,
    scope: &DecisionScope,
    source: RuleSource,
    fingerprint: Option<ExecFingerprint>,
    recorded_at: DateTime<Utc>,
) -> Value {
    json!({
        "decision_id": decision_id,
        "scope": scope,
        "source": source,
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

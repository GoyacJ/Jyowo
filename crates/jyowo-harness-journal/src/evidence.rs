//! Evidence ref store — durable, conversation-scoped backend references for
//! large command output, diff patches, and artifact content.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use harness_contracts::{
    BlobMeta, BlobRef, BlobRetention, BlobStore, ConversationEventRef, EvidenceRedactionState,
    EvidenceRefId, EvidenceRefKind, JournalError, TenantId,
};
use serde::{Deserialize, Serialize};
#[cfg(feature = "sqlite")]
use tokio::sync::Mutex;

/// A durable registry record for an evidence ref.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EvidenceRefRecord {
    pub id: EvidenceRefId,
    pub kind: EvidenceRefKind,
    pub conversation_id: String,
    pub run_id: String,
    pub source_event_refs: Vec<ConversationEventRef>,
    pub artifact_id: Option<String>,
    pub revision_id: Option<String>,
    pub content_type: String,
    pub byte_length: u64,
    pub content_hash: Vec<u8>,
    pub redaction_state: EvidenceRedactionState,
    pub redaction_provenance: RedactionProvenance,
    pub retention: BlobRetention,
    pub source: EvidenceRefSource,
}

/// Where the evidence content lives.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum EvidenceRefSource {
    Blob {
        blob_ref: BlobRef,
    },
    JournalPayload {
        event_id: String,
        json_pointer: String,
    },
}

/// Provenance of redaction for an evidence ref.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct RedactionProvenance {
    pub redactor_version: String,
}

/// The registry trait for evidence ref persistence.
#[async_trait]
pub trait EvidenceRefRegistry: Send + Sync + 'static {
    async fn insert(&self, tenant: TenantId, record: EvidenceRefRecord)
        -> Result<(), JournalError>;

    async fn get(
        &self,
        tenant: TenantId,
        id: &EvidenceRefId,
    ) -> Result<Option<EvidenceRefRecord>, JournalError>;

    async fn delete_for_conversation(
        &self,
        tenant: TenantId,
        conversation_id: &str,
    ) -> Result<(), JournalError>;

    async fn list_for_conversation(
        &self,
        tenant: TenantId,
        conversation_id: &str,
    ) -> Result<Vec<EvidenceRefRecord>, JournalError>;

    async fn list_live_blob_roots(&self, tenant: TenantId) -> Result<Vec<BlobRef>, JournalError>;
}

/// Top-level evidence ref store that orchestrates the registry and blob store.
pub struct EvidenceRefStore {
    registry: Arc<dyn EvidenceRefRegistry>,
    blob_store: Arc<dyn BlobStore>,
}

impl EvidenceRefStore {
    #[must_use]
    pub fn new(registry: Arc<dyn EvidenceRefRegistry>, blob_store: Arc<dyn BlobStore>) -> Self {
        Self {
            registry,
            blob_store,
        }
    }

    /// Store blob-backed evidence content and register the ref.
    pub async fn store_blob_evidence(
        &self,
        tenant: TenantId,
        record: EvidenceRefRecord,
        bytes: Vec<u8>,
    ) -> Result<EvidenceRefId, JournalError> {
        let mut record = record;
        let retention = record.retention.clone();
        let hash = blake3::hash(&bytes);
        let mut content_hash = [0u8; 32];
        content_hash.copy_from_slice(hash.as_bytes());

        if record.byte_length != bytes.len() as u64 {
            return Err(JournalError::Message(format!(
                "evidence byte length mismatch: expected {}, got {}",
                record.byte_length,
                bytes.len()
            )));
        }
        if record.content_hash != content_hash {
            return Err(JournalError::Message(
                "evidence content hash does not match bytes".to_owned(),
            ));
        }
        validate_redaction_provenance(&record.redaction_provenance)?;

        let meta = BlobMeta {
            content_type: Some(record.content_type.clone()),
            size: bytes.len() as u64,
            content_hash,
            created_at: chrono::Utc::now(),
            retention,
        };
        let id = record.id.clone();

        if let Some(existing) = self.registry.get(tenant, &id).await? {
            self.validate_existing_blob_evidence(tenant, &record, &existing)
                .await?;
            return Ok(id);
        }

        // Write blob first
        let stored_ref = self
            .blob_store
            .put(tenant, Bytes::from(bytes), meta)
            .await
            .map_err(|e| JournalError::Message(format!("blob write failed: {e}")))?;

        record.source = EvidenceRefSource::Blob {
            blob_ref: stored_ref.clone(),
        };

        // Then write registry row
        match self.registry.insert(tenant, record.clone()).await {
            Ok(()) => Ok(id),
            Err(registry_error) => {
                if let Err(delete_error) = self.blob_store.delete(tenant, &stored_ref).await {
                    return Err(JournalError::Message(format!(
                        "evidence registry insert failed: {registry_error}; orphan blob cleanup failed: {delete_error}"
                    )));
                }
                if let Some(existing) = self.registry.get(tenant, &id).await? {
                    self.validate_existing_blob_evidence(tenant, &record, &existing)
                        .await?;
                    return Ok(id);
                }
                Err(registry_error)
            }
        }
    }

    async fn validate_existing_blob_evidence(
        &self,
        tenant: TenantId,
        record: &EvidenceRefRecord,
        existing: &EvidenceRefRecord,
    ) -> Result<(), JournalError> {
        if !same_stable_evidence_metadata(record, existing) {
            return Err(JournalError::Message(format!(
                "conflicting evidence ref metadata for id: {}",
                record.id
            )));
        }
        let EvidenceRefSource::Blob { blob_ref } = &existing.source else {
            return Err(JournalError::Message(format!(
                "conflicting evidence ref source for id: {}",
                record.id
            )));
        };
        let expected_hash = record_hash_array(record)?;
        if blob_ref.size != record.byte_length || blob_ref.content_hash != expected_hash {
            return Err(JournalError::Message(format!(
                "existing evidence blob metadata mismatch for id: {}",
                record.id
            )));
        }
        let meta = self
            .blob_store
            .head(tenant, blob_ref)
            .await
            .map_err(|e| JournalError::Message(format!("blob head failed: {e}")))?
            .ok_or_else(|| {
                JournalError::Message(format!(
                    "existing evidence blob not found for id: {}",
                    record.id
                ))
            })?;
        if meta.size != record.byte_length || meta.content_hash != expected_hash {
            return Err(JournalError::Message(format!(
                "existing evidence blob content metadata mismatch for id: {}",
                record.id
            )));
        }
        Ok(())
    }

    /// Store journal-backed evidence by registering a source event pointer.
    pub async fn store_journal_evidence(
        &self,
        tenant: TenantId,
        record: EvidenceRefRecord,
    ) -> Result<EvidenceRefId, JournalError> {
        validate_redaction_provenance(&record.redaction_provenance)?;
        let id = record.id.clone();
        self.registry.insert(tenant, record).await?;
        Ok(id)
    }

    /// Read evidence content, validating ownership, kind, and redaction.
    pub async fn read_evidence(
        &self,
        tenant: TenantId,
        conversation_id: &str,
        ref_id: &EvidenceRefId,
        expected_kind: EvidenceRefKind,
    ) -> Result<EvidenceReadResult, JournalError> {
        let record =
            self.registry.get(tenant, ref_id).await?.ok_or_else(|| {
                JournalError::Message(format!("evidence ref not found: {ref_id}"))
            })?;

        if record.conversation_id != conversation_id {
            return Err(JournalError::Message(
                "evidence ref does not belong to conversation".to_owned(),
            ));
        }

        if record.kind != expected_kind {
            return Err(JournalError::Message(format!(
                "evidence ref kind mismatch: expected {expected_kind:?}, got {:?}",
                record.kind
            )));
        }

        if matches!(record.redaction_state, EvidenceRedactionState::Withheld) {
            return Err(JournalError::Message(
                "evidence content is withheld".to_owned(),
            ));
        }
        validate_redaction_provenance(&record.redaction_provenance)?;

        let bytes = match &record.source {
            EvidenceRefSource::Blob { blob_ref } => {
                if blob_ref.size != record.byte_length {
                    return Err(JournalError::Message(
                        "evidence blob length metadata mismatch".to_owned(),
                    ));
                }
                let expected_hash = record_hash_array(&record)?;
                if blob_ref.content_hash != expected_hash {
                    return Err(JournalError::Message(
                        "evidence blob hash metadata mismatch".to_owned(),
                    ));
                }
                let meta = self
                    .blob_store
                    .head(tenant, blob_ref)
                    .await
                    .map_err(|e| JournalError::Message(format!("blob head failed: {e}")))?
                    .ok_or_else(|| {
                        JournalError::Message(format!("evidence blob not found: {}", blob_ref.id))
                    })?;
                if meta.size != record.byte_length {
                    return Err(JournalError::Message(
                        "evidence blob length mismatch".to_owned(),
                    ));
                }
                if meta.content_hash != expected_hash {
                    return Err(JournalError::Message(
                        "evidence blob hash mismatch".to_owned(),
                    ));
                }
                let mut stream = self
                    .blob_store
                    .get(tenant, blob_ref)
                    .await
                    .map_err(|e| JournalError::Message(format!("blob read failed: {e}")))?;
                let mut buf = Vec::new();
                while let Some(chunk) = stream.next().await {
                    buf.extend_from_slice(&chunk);
                }
                if buf.len() as u64 != record.byte_length {
                    return Err(JournalError::Message(
                        "evidence content length mismatch".to_owned(),
                    ));
                }
                let actual_hash = blake3::hash(&buf);
                if actual_hash.as_bytes() != &expected_hash {
                    return Err(JournalError::Message(
                        "evidence content hash mismatch".to_owned(),
                    ));
                }
                buf
            }
            EvidenceRefSource::JournalPayload { .. } => {
                return Err(JournalError::Message(
                    "journal-backed evidence reads not yet implemented".to_owned(),
                ));
            }
        };

        Ok(EvidenceReadResult {
            content_type: record.content_type,
            byte_length: bytes.len() as u64,
            redaction_state: record.redaction_state,
            bytes,
        })
    }

    /// Delete all refs for a conversation.
    pub async fn delete_for_conversation(
        &self,
        tenant: TenantId,
        conversation_id: &str,
    ) -> Result<(), JournalError> {
        let records = self
            .registry
            .list_for_conversation(tenant, conversation_id)
            .await?;
        for record in records {
            if let EvidenceRefSource::Blob { blob_ref } = record.source {
                self.blob_store
                    .delete(tenant, &blob_ref)
                    .await
                    .map_err(|e| JournalError::Message(format!("blob delete failed: {e}")))?;
            }
        }
        self.registry
            .delete_for_conversation(tenant, conversation_id)
            .await?;
        Ok(())
    }

    /// List live blob roots for GC.
    pub async fn list_live_blob_roots(
        &self,
        tenant: TenantId,
    ) -> Result<Vec<BlobRef>, JournalError> {
        self.registry.list_live_blob_roots(tenant).await
    }
}

/// Result of reading evidence content.
#[derive(Debug, Clone)]
pub struct EvidenceReadResult {
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub byte_length: u64,
    pub redaction_state: EvidenceRedactionState,
}

fn validate_redaction_provenance(provenance: &RedactionProvenance) -> Result<(), JournalError> {
    if provenance.redactor_version.trim().is_empty() {
        return Err(JournalError::Message(
            "evidence redaction provenance is missing".to_owned(),
        ));
    }
    Ok(())
}

fn same_stable_evidence_metadata(left: &EvidenceRefRecord, right: &EvidenceRefRecord) -> bool {
    left.id == right.id
        && left.kind == right.kind
        && left.conversation_id == right.conversation_id
        && left.run_id == right.run_id
        && left.source_event_refs == right.source_event_refs
        && left.artifact_id == right.artifact_id
        && left.revision_id == right.revision_id
        && left.content_type == right.content_type
        && left.byte_length == right.byte_length
        && left.content_hash == right.content_hash
        && left.redaction_state == right.redaction_state
        && left.redaction_provenance == right.redaction_provenance
        && left.retention == right.retention
}

fn record_hash_array(record: &EvidenceRefRecord) -> Result<[u8; 32], JournalError> {
    record.content_hash.clone().try_into().map_err(|_| {
        JournalError::Message(format!(
            "invalid evidence content hash length for id: {}",
            record.id
        ))
    })
}

// ── In-memory registry for tests ──

/// An in-memory evidence ref registry for testing purposes only.
#[derive(Debug, Default)]
pub struct InMemoryEvidenceRefRegistry {
    records: std::sync::Mutex<Vec<EvidenceRefRecord>>,
}

impl InMemoryEvidenceRefRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl EvidenceRefRegistry for InMemoryEvidenceRefRegistry {
    async fn insert(
        &self,
        _tenant: TenantId,
        record: EvidenceRefRecord,
    ) -> Result<(), JournalError> {
        let mut records = self
            .records
            .lock()
            .map_err(|e| JournalError::Message(format!("lock error: {e}")))?;
        if let Some(existing) = records.iter().find(|r| r.id == record.id) {
            if existing == &record {
                return Ok(());
            }
            return Err(JournalError::Message(format!(
                "conflicting evidence ref metadata for id: {}",
                record.id
            )));
        }
        records.push(record);
        Ok(())
    }

    async fn get(
        &self,
        _tenant: TenantId,
        id: &EvidenceRefId,
    ) -> Result<Option<EvidenceRefRecord>, JournalError> {
        let records = self
            .records
            .lock()
            .map_err(|e| JournalError::Message(format!("lock error: {e}")))?;
        Ok(records.iter().find(|r| &r.id == id).cloned())
    }

    async fn delete_for_conversation(
        &self,
        _tenant: TenantId,
        conversation_id: &str,
    ) -> Result<(), JournalError> {
        let mut records = self
            .records
            .lock()
            .map_err(|e| JournalError::Message(format!("lock error: {e}")))?;
        records.retain(|r| r.conversation_id != conversation_id);
        Ok(())
    }

    async fn list_for_conversation(
        &self,
        _tenant: TenantId,
        conversation_id: &str,
    ) -> Result<Vec<EvidenceRefRecord>, JournalError> {
        let records = self
            .records
            .lock()
            .map_err(|e| JournalError::Message(format!("lock error: {e}")))?;
        Ok(records
            .iter()
            .filter(|record| record.conversation_id == conversation_id)
            .cloned()
            .collect())
    }

    async fn list_live_blob_roots(&self, _tenant: TenantId) -> Result<Vec<BlobRef>, JournalError> {
        let records = self
            .records
            .lock()
            .map_err(|e| JournalError::Message(format!("lock error: {e}")))?;
        Ok(records
            .iter()
            .filter_map(|r| match &r.source {
                EvidenceRefSource::Blob { blob_ref } => Some(blob_ref.clone()),
                _ => None,
            })
            .collect())
    }
}

// ── SQLite registry ──

#[cfg(feature = "sqlite")]
pub struct SqliteEvidenceRefRegistry {
    connection: Mutex<rusqlite::Connection>,
}

#[cfg(feature = "sqlite")]
impl SqliteEvidenceRefRegistry {
    pub async fn open(path: impl AsRef<std::path::Path>) -> Result<Self, JournalError> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| JournalError::Message(format!("create evidence registry dir: {e}")))?;
        }
        let connection = rusqlite::Connection::open(path).map_err(journal_sqlite_error)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA busy_timeout = 5000;
                 CREATE TABLE IF NOT EXISTS evidence_refs (
                    tenant_id TEXT NOT NULL,
                    evidence_ref_id TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    conversation_id TEXT NOT NULL,
                    run_id TEXT NOT NULL,
                    source_event_refs TEXT NOT NULL,
                    artifact_id TEXT,
                    revision_id TEXT,
                    content_type TEXT NOT NULL,
                    byte_length INTEGER NOT NULL,
                    content_hash BLOB NOT NULL,
                    redaction_state TEXT NOT NULL,
                    redaction_provenance TEXT NOT NULL,
                    retention TEXT NOT NULL,
                    source TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    PRIMARY KEY (tenant_id, evidence_ref_id)
                 ) STRICT;
                 CREATE INDEX IF NOT EXISTS idx_evidence_refs_conversation
                    ON evidence_refs(tenant_id, conversation_id);
                 CREATE INDEX IF NOT EXISTS idx_evidence_refs_kind
                    ON evidence_refs(tenant_id, kind);",
            )
            .map_err(journal_sqlite_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<EvidenceRefRecord> {
        let id: String = row.get("evidence_ref_id")?;
        let kind: String = row.get("kind")?;
        let source_event_refs: String = row.get("source_event_refs")?;
        let redaction_state: String = row.get("redaction_state")?;
        let redaction_provenance: String = row.get("redaction_provenance")?;
        let retention: String = row.get("retention")?;
        let source: String = row.get("source")?;
        let byte_length: i64 = row.get("byte_length")?;
        Ok(EvidenceRefRecord {
            id: EvidenceRefId::new(id),
            kind: serde_json::from_str(&kind).map_err(sqlite_from_serde)?,
            conversation_id: row.get("conversation_id")?,
            run_id: row.get("run_id")?,
            source_event_refs: serde_json::from_str(&source_event_refs)
                .map_err(sqlite_from_serde)?,
            artifact_id: row.get("artifact_id")?,
            revision_id: row.get("revision_id")?,
            content_type: row.get("content_type")?,
            byte_length: byte_length as u64,
            content_hash: row.get("content_hash")?,
            redaction_state: serde_json::from_str(&redaction_state).map_err(sqlite_from_serde)?,
            redaction_provenance: serde_json::from_str(&redaction_provenance)
                .map_err(sqlite_from_serde)?,
            retention: serde_json::from_str(&retention).map_err(sqlite_from_serde)?,
            source: serde_json::from_str(&source).map_err(sqlite_from_serde)?,
        })
    }
}

#[cfg(feature = "sqlite")]
#[async_trait]
impl EvidenceRefRegistry for SqliteEvidenceRefRegistry {
    async fn insert(
        &self,
        tenant: TenantId,
        record: EvidenceRefRecord,
    ) -> Result<(), JournalError> {
        validate_redaction_provenance(&record.redaction_provenance)?;
        let connection = self.connection.lock().await;
        if let Some(existing) = get_sqlite_record(&connection, tenant, &record.id)? {
            if existing == record {
                return Ok(());
            }
            return Err(JournalError::Message(format!(
                "conflicting evidence ref metadata for id: {}",
                record.id
            )));
        }
        connection
            .execute(
                "INSERT INTO evidence_refs (
                    tenant_id, evidence_ref_id, kind, conversation_id, run_id,
                    source_event_refs, artifact_id, revision_id, content_type,
                    byte_length, content_hash, redaction_state, redaction_provenance,
                    retention, source, created_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                rusqlite::params![
                    tenant.to_string(),
                    record.id.to_string(),
                    serde_json::to_string(&record.kind).map_err(journal_sqlite_error)?,
                    record.conversation_id,
                    record.run_id,
                    serde_json::to_string(&record.source_event_refs)
                        .map_err(journal_sqlite_error)?,
                    record.artifact_id,
                    record.revision_id,
                    record.content_type,
                    record.byte_length as i64,
                    record.content_hash,
                    serde_json::to_string(&record.redaction_state).map_err(journal_sqlite_error)?,
                    serde_json::to_string(&record.redaction_provenance)
                        .map_err(journal_sqlite_error)?,
                    serde_json::to_string(&record.retention).map_err(journal_sqlite_error)?,
                    serde_json::to_string(&record.source).map_err(journal_sqlite_error)?,
                    chrono::Utc::now().to_rfc3339()
                ],
            )
            .map_err(journal_sqlite_error)?;
        Ok(())
    }

    async fn get(
        &self,
        tenant: TenantId,
        id: &EvidenceRefId,
    ) -> Result<Option<EvidenceRefRecord>, JournalError> {
        let connection = self.connection.lock().await;
        get_sqlite_record(&connection, tenant, id)
    }

    async fn delete_for_conversation(
        &self,
        tenant: TenantId,
        conversation_id: &str,
    ) -> Result<(), JournalError> {
        self.connection
            .lock()
            .await
            .execute(
                "DELETE FROM evidence_refs
                 WHERE tenant_id = ?1 AND conversation_id = ?2",
                rusqlite::params![tenant.to_string(), conversation_id],
            )
            .map_err(journal_sqlite_error)?;
        Ok(())
    }

    async fn list_for_conversation(
        &self,
        tenant: TenantId,
        conversation_id: &str,
    ) -> Result<Vec<EvidenceRefRecord>, JournalError> {
        let connection = self.connection.lock().await;
        let mut statement = connection
            .prepare(
                "SELECT * FROM evidence_refs
                 WHERE tenant_id = ?1 AND conversation_id = ?2
                 ORDER BY created_at ASC, evidence_ref_id ASC",
            )
            .map_err(journal_sqlite_error)?;
        let rows = statement
            .query_map(
                rusqlite::params![tenant.to_string(), conversation_id],
                Self::row_to_record,
            )
            .map_err(journal_sqlite_error)?;
        collect_sqlite_records(rows)
    }

    async fn list_live_blob_roots(&self, tenant: TenantId) -> Result<Vec<BlobRef>, JournalError> {
        let connection = self.connection.lock().await;
        let mut statement = connection
            .prepare("SELECT * FROM evidence_refs WHERE tenant_id = ?1")
            .map_err(journal_sqlite_error)?;
        let rows = statement
            .query_map(rusqlite::params![tenant.to_string()], Self::row_to_record)
            .map_err(journal_sqlite_error)?;
        Ok(collect_sqlite_records(rows)?
            .into_iter()
            .filter_map(|record| match record.source {
                EvidenceRefSource::Blob { blob_ref } => Some(blob_ref),
                EvidenceRefSource::JournalPayload { .. } => None,
            })
            .collect())
    }
}

#[cfg(feature = "sqlite")]
fn get_sqlite_record(
    connection: &rusqlite::Connection,
    tenant: TenantId,
    id: &EvidenceRefId,
) -> Result<Option<EvidenceRefRecord>, JournalError> {
    let result = connection.query_row(
        "SELECT * FROM evidence_refs
         WHERE tenant_id = ?1 AND evidence_ref_id = ?2",
        rusqlite::params![tenant.to_string(), id.to_string()],
        SqliteEvidenceRefRegistry::row_to_record,
    );
    match result {
        Ok(record) => Ok(Some(record)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(journal_sqlite_error(error)),
    }
}

#[cfg(feature = "sqlite")]
fn collect_sqlite_records<F>(
    rows: rusqlite::MappedRows<'_, F>,
) -> Result<Vec<EvidenceRefRecord>, JournalError>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<EvidenceRefRecord>,
{
    let mut records = Vec::new();
    for row in rows {
        records.push(row.map_err(journal_sqlite_error)?);
    }
    Ok(records)
}

#[cfg(feature = "sqlite")]
fn sqlite_from_serde(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

#[cfg(feature = "sqlite")]
fn journal_sqlite_error(error: impl std::fmt::Display) -> JournalError {
    JournalError::Message(error.to_string())
}

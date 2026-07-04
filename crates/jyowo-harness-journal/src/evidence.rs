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
#[derive(Debug, Clone, Eq, PartialEq)]
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
#[derive(Debug, Clone, Eq, PartialEq)]
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
        let retention = record.retention.clone();

        let meta = BlobMeta {
            content_type: Some(record.content_type.clone()),
            size: bytes.len() as u64,
            content_hash: {
                let hash = blake3::hash(&bytes);
                let mut arr = [0u8; 32];
                arr.copy_from_slice(hash.as_bytes());
                arr
            },
            created_at: chrono::Utc::now(),
            retention,
        };

        // Write blob first
        let stored_ref = self
            .blob_store
            .put(tenant, Bytes::from(bytes), meta)
            .await
            .map_err(|e| JournalError::Message(format!("blob write failed: {e}")))?;

        // Then write registry row
        match self.registry.insert(tenant, record).await {
            Ok(()) => Ok(EvidenceRefId::new(stored_ref.id.to_string())),
            Err(e) => {
                let _ = self.blob_store.delete(tenant, &stored_ref).await;
                Err(e)
            }
        }
    }

    /// Store journal-backed evidence by registering a source event pointer.
    pub async fn store_journal_evidence(
        &self,
        tenant: TenantId,
        record: EvidenceRefRecord,
    ) -> Result<EvidenceRefId, JournalError> {
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

        let bytes = match &record.source {
            EvidenceRefSource::Blob { blob_ref } => {
                let mut stream = self
                    .blob_store
                    .get(tenant, blob_ref)
                    .await
                    .map_err(|e| JournalError::Message(format!("blob read failed: {e}")))?;
                let mut buf = Vec::new();
                while let Some(chunk) = stream.next().await {
                    buf.extend_from_slice(&chunk);
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
        self.registry
            .delete_for_conversation(tenant, conversation_id)
            .await
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
            if existing.content_hash == record.content_hash {
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

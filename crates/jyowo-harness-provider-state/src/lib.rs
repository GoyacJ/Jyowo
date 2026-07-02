//! `jyowo-harness-provider-state`
//!
//! Private provider continuation persistence for harness runtimes.

#![forbid(unsafe_code)]

use std::{
    collections::{HashMap, HashSet},
    fmt,
    fs::{self, File, OpenOptions},
    hash::Hash,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use harness_contracts::{MessageId, ModelProtocol, RunId, SessionId, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::Mutex;

const STORE_LABEL: &str = "provider-continuations.jsonl";
const RUNTIME_DIR: &str = ".jyowo/runtime";
const PRUNE_TEMP_LABEL: &str = "provider-continuations.jsonl.prune.tmp";

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderContinuationRecord {
    pub provider_id: String,
    pub model_config_id: Option<String>,
    pub protocol: ModelProtocol,
    pub dialect: String,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub producing_run_id: RunId,
    pub message_id: MessageId,
    pub scope: ProviderContinuationScope,
    pub kind: ProviderContinuationKind,
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

impl fmt::Debug for ProviderContinuationRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderContinuationRecord")
            .field("provider_id", &self.provider_id)
            .field("model_config_id", &self.model_config_id)
            .field("protocol", &self.protocol)
            .field("dialect", &self.dialect)
            .field("tenant_id", &self.tenant_id)
            .field("session_id", &self.session_id)
            .field("producing_run_id", &self.producing_run_id)
            .field("message_id", &self.message_id)
            .field("scope", &self.scope)
            .field("kind", &self.kind)
            .field("payload", &"<redacted>")
            .field("created_at", &self.created_at)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderContinuationScope {
    Conversation,
    Run,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderContinuationKind {
    ReasoningReplay,
    ToolReplay,
    CacheReplay,
    ProviderNative(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderContinuationQuery {
    pub provider_id: String,
    pub model_config_id: Option<String>,
    pub protocol: ModelProtocol,
    pub dialect: String,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub message_ids: Vec<MessageId>,
    pub kinds: Vec<ProviderContinuationKind>,
}

#[derive(Debug, Error)]
pub enum ProviderContinuationStoreError {
    #[error("provider continuation store provider-continuations.jsonl I/O failed")]
    Io {
        #[source]
        source: io::Error,
    },
    #[error("provider continuation store provider-continuations.jsonl contains a corrupt record at line {line}")]
    CorruptRecord { line: usize, details: String },
    #[error("provider continuation record payload must not be null")]
    NullPayload,
    #[error("provider continuation record serialization failed")]
    Serialization,
}

#[async_trait]
pub trait ProviderContinuationStore: Send + Sync + 'static {
    async fn load_for_messages(
        &self,
        query: ProviderContinuationQuery,
    ) -> Result<Vec<ProviderContinuationRecord>, ProviderContinuationStoreError>;

    async fn append_batch(
        &self,
        records: Vec<ProviderContinuationRecord>,
    ) -> Result<(), ProviderContinuationStoreError>;

    async fn prune_session(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<(), ProviderContinuationStoreError>;
}

#[derive(Debug)]
pub struct FileProviderContinuationStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileProviderContinuationStore {
    pub fn open(workspace_root: impl AsRef<Path>) -> Result<Self, ProviderContinuationStoreError> {
        let runtime_dir = workspace_root.as_ref().join(RUNTIME_DIR);
        fs::create_dir_all(&runtime_dir).map_err(io_error)?;
        Ok(Self {
            path: runtime_dir.join(STORE_LABEL),
            lock: Mutex::new(()),
        })
    }

    fn runtime_dir(&self) -> Result<&Path, ProviderContinuationStoreError> {
        self.path.parent().ok_or_else(|| {
            io_error(io::Error::new(
                io::ErrorKind::InvalidInput,
                "provider continuation store path has no parent",
            ))
        })
    }

    fn read_records(
        &self,
    ) -> Result<Vec<ProviderContinuationRecord>, ProviderContinuationStoreError> {
        match File::open(&self.path) {
            Ok(file) => read_records_from(file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(io_error(error)),
        }
    }
}

#[async_trait]
impl ProviderContinuationStore for FileProviderContinuationStore {
    async fn load_for_messages(
        &self,
        query: ProviderContinuationQuery,
    ) -> Result<Vec<ProviderContinuationRecord>, ProviderContinuationStoreError> {
        let _guard = self.lock.lock().await;
        let message_ids: HashSet<MessageId> = query.message_ids.into_iter().collect();
        let kinds: HashSet<ProviderContinuationKind> = query.kinds.into_iter().collect();
        let mut newest: HashMap<(MessageId, ProviderContinuationKind), ProviderContinuationRecord> =
            HashMap::new();

        for record in self.read_records()? {
            if record.provider_id != query.provider_id
                || record.model_config_id != query.model_config_id
                || record.protocol != query.protocol
                || record.dialect != query.dialect
                || record.tenant_id != query.tenant_id
                || record.session_id != query.session_id
                || !message_ids.contains(&record.message_id)
                || !kinds.contains(&record.kind)
            {
                continue;
            }

            let key = (record.message_id, record.kind.clone());
            match newest.get(&key) {
                Some(existing) if existing.created_at >= record.created_at => {}
                _ => {
                    newest.insert(key, record);
                }
            }
        }

        Ok(newest.into_values().collect())
    }

    async fn append_batch(
        &self,
        records: Vec<ProviderContinuationRecord>,
    ) -> Result<(), ProviderContinuationStoreError> {
        let _guard = self.lock.lock().await;
        let mut buffer = Vec::new();

        for record in records {
            if record.payload.is_null() {
                return Err(ProviderContinuationStoreError::NullPayload);
            }
            serde_json::to_writer(&mut buffer, &record)
                .map_err(|_| ProviderContinuationStoreError::Serialization)?;
            buffer.push(b'\n');
        }

        if buffer.is_empty() {
            return Ok(());
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(io_error)?;
        file.write_all(&buffer).map_err(io_error)?;
        file.sync_data().map_err(io_error)?;
        Ok(())
    }

    async fn prune_session(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<(), ProviderContinuationStoreError> {
        let _guard = self.lock.lock().await;
        let retained: Vec<_> = self
            .read_records()?
            .into_iter()
            .filter(|record| record.tenant_id != tenant_id || record.session_id != session_id)
            .collect();
        let runtime_dir = self.runtime_dir()?;
        let temp_path = runtime_dir.join(PRUNE_TEMP_LABEL);
        let mut buffer = Vec::new();

        for record in retained {
            serde_json::to_writer(&mut buffer, &record)
                .map_err(|_| ProviderContinuationStoreError::Serialization)?;
            buffer.push(b'\n');
        }

        let mut temp = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_path)
            .map_err(io_error)?;
        temp.write_all(&buffer).map_err(io_error)?;
        temp.sync_data().map_err(io_error)?;
        drop(temp);

        fs::rename(&temp_path, &self.path).map_err(io_error)?;
        sync_directory(runtime_dir)?;
        Ok(())
    }
}

fn read_records_from(
    file: File,
) -> Result<Vec<ProviderContinuationRecord>, ProviderContinuationStoreError> {
    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for (index, line) in reader.lines().enumerate() {
        let line = line.map_err(io_error)?;
        if line.trim().is_empty() {
            continue;
        }
        let record =
            serde_json::from_str::<ProviderContinuationRecord>(&line).map_err(|error| {
                ProviderContinuationStoreError::CorruptRecord {
                    line: index + 1,
                    details: error.to_string(),
                }
            })?;
        if record.payload.is_null() {
            return Err(ProviderContinuationStoreError::CorruptRecord {
                line: index + 1,
                details: "null payload".to_owned(),
            });
        }
        records.push(record);
    }

    Ok(records)
}

fn sync_directory(path: &Path) -> Result<(), ProviderContinuationStoreError> {
    let directory = File::open(path).map_err(io_error)?;
    directory.sync_all().map_err(io_error)
}

fn io_error(source: io::Error) -> ProviderContinuationStoreError {
    ProviderContinuationStoreError::Io { source }
}

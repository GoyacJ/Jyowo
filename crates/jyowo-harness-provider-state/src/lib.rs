//! `jyowo-harness-provider-state`
//!
//! Private provider continuation persistence for harness runtimes.

#![forbid(unsafe_code)]

use std::{
    collections::{HashMap, HashSet},
    fmt,
    fs::{File, OpenOptions},
    hash::Hash,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fs2::FileExt;
use harness_contracts::{MessageId, ModelProtocol, RunId, SessionId, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::Mutex;

const STORE_LABEL: &str = "provider-continuations.jsonl";
const RUNTIME_DIR: &str = ".jyowo/runtime";

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

#[derive(Error)]
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

impl fmt::Debug for ProviderContinuationStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { source } => formatter
                .debug_struct("ProviderContinuationStoreError::Io")
                .field("store", &STORE_LABEL)
                .field("source_kind", &source.kind())
                .finish(),
            Self::CorruptRecord { line, .. } => formatter
                .debug_struct("ProviderContinuationStoreError::CorruptRecord")
                .field("store", &STORE_LABEL)
                .field("line", line)
                .field("details", &"<redacted>")
                .finish(),
            Self::NullPayload => formatter
                .debug_struct("ProviderContinuationStoreError::NullPayload")
                .field("store", &STORE_LABEL)
                .finish(),
            Self::Serialization => formatter
                .debug_struct("ProviderContinuationStoreError::Serialization")
                .field("store", &STORE_LABEL)
                .finish(),
        }
    }
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
    lock_path: PathBuf,
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileProviderContinuationStore {
    /// Open the continuation store at `runtime_root`, using the standard
    /// `provider-continuations.jsonl` under that directory.
    ///
    /// Prefer this over `open` when you already have a resolved runtime root
    /// (e.g. from `RuntimeLayout`).
    pub fn open_runtime_dir(
        runtime_root: impl AsRef<Path>,
    ) -> Result<Self, ProviderContinuationStoreError> {
        let runtime_dir = runtime_root.as_ref().to_path_buf();
        // Resolve any benign OS-level symlinks (e.g. /tmp on macOS) before
        // running the strict no-symlink directory check.
        let runtime_dir = harness_fs::resolve_canonical_prefix(&runtime_dir).map_err(fs_error)?;
        harness_fs::ensure_app_dir_no_symlink(&runtime_dir).map_err(fs_error)?;
        let path = runtime_dir.join(STORE_LABEL);
        Ok(Self {
            lock_path: lock_path_for(&path),
            path,
            lock: Mutex::new(()),
        })
    }

    /// Open the continuation store from a workspace root, appending
    /// `.jyowo/runtime` internally.
    ///
    /// This is a compatibility wrapper. Prefer `open_runtime_dir` when a
    /// resolved runtime root is available from `RuntimeLayout`.
    pub fn open(workspace_root: impl AsRef<Path>) -> Result<Self, ProviderContinuationStoreError> {
        let runtime_dir = workspace_root.as_ref().join(RUNTIME_DIR);
        Self::open_runtime_dir(runtime_dir)
    }

    #[cfg(not(unix))]
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
        harness_fs::ensure_no_symlink_components(&self.path).map_err(fs_error)?;
        match open_existing_file_no_follow(&self.path)? {
            Some(file) => {
                harness_fs::set_owner_only_file_if_unix(&file).map_err(fs_error)?;
                read_records_from(file)
            }
            None => Ok(Vec::new()),
        }
    }

    fn with_file_lock<T>(
        &self,
        action: impl FnOnce() -> Result<T, ProviderContinuationStoreError>,
    ) -> Result<T, ProviderContinuationStoreError> {
        #[cfg(unix)]
        {
            let parent = harness_fs::open_parent_dir_no_symlink_for_write(&self.lock_path)
                .map_err(fs_error)?;
            let lock_file = parent
                .open_or_create_read_write_file(parent.file_name())
                .map_err(fs_error)?;
            harness_fs::set_owner_only_file_if_unix(&lock_file).map_err(fs_error)?;
            parent.sync_all().map_err(fs_error)?;
            lock_file.lock_exclusive().map_err(io_error)?;
            let result = action();
            let unlock_result = lock_file.unlock().map_err(io_error);
            return match (result, unlock_result) {
                (Err(error), _) => Err(error),
                (Ok(_), Err(error)) => Err(error),
                (Ok(value), Ok(())) => Ok(value),
            };
        }

        #[cfg(not(unix))]
        {
            let runtime_dir = self.runtime_dir()?;
            harness_fs::ensure_app_dir_no_symlink(runtime_dir).map_err(fs_error)?;
            harness_fs::ensure_no_symlink_components(&self.lock_path).map_err(fs_error)?;
            let mut open_options = OpenOptions::new();
            open_options.create(true).read(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;

                open_options.custom_flags(libc::O_NOFOLLOW);
                open_options.mode(0o600);
            }
            let lock_file = open_options.open(&self.lock_path).map_err(io_error)?;
            harness_fs::set_owner_only_file_if_unix(&lock_file).map_err(fs_error)?;
            harness_fs::sync_directory(runtime_dir).map_err(fs_error)?;
            lock_file.lock_exclusive().map_err(io_error)?;
            let result = action();
            let unlock_result = lock_file.unlock().map_err(io_error);
            match (result, unlock_result) {
                (Err(error), _) => Err(error),
                (Ok(_), Err(error)) => Err(error),
                (Ok(value), Ok(())) => Ok(value),
            }
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

        for record in self.with_file_lock(|| self.read_records())? {
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

        self.with_file_lock(|| {
            #[cfg(unix)]
            {
                let parent = harness_fs::open_parent_dir_no_symlink_for_write(&self.path)
                    .map_err(fs_error)?;
                let mut file = parent
                    .open_or_create_append_file(parent.file_name())
                    .map_err(fs_error)?;
                harness_fs::set_owner_only_file_if_unix(&file).map_err(fs_error)?;
                file.write_all(&buffer).map_err(io_error)?;
                file.sync_all().map_err(io_error)?;
                parent.sync_all().map_err(fs_error)?;
                return Ok(());
            }

            #[cfg(not(unix))]
            {
                harness_fs::ensure_no_symlink_components(&self.path).map_err(fs_error)?;
                harness_fs::set_owner_only_if_exists_unix(&self.path).map_err(fs_error)?;
                let parent = self.runtime_dir()?;
                let mut open_options = OpenOptions::new();
                open_options.create(true).append(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;

                    open_options.custom_flags(libc::O_NOFOLLOW);
                    open_options.mode(0o600);
                }
                let mut file = open_options.open(&self.path).map_err(io_error)?;
                harness_fs::set_owner_only_file_if_unix(&file).map_err(fs_error)?;
                file.write_all(&buffer).map_err(io_error)?;
                file.sync_all().map_err(io_error)?;
                harness_fs::sync_directory(parent).map_err(fs_error)?;
                Ok(())
            }
        })
    }

    async fn prune_session(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<(), ProviderContinuationStoreError> {
        let _guard = self.lock.lock().await;
        self.with_file_lock(|| {
            let retained: Vec<_> = self
                .read_records()?
                .into_iter()
                .filter(|record| record.tenant_id != tenant_id || record.session_id != session_id)
                .collect();
            let mut buffer = Vec::new();

            for record in retained {
                serde_json::to_writer(&mut buffer, &record)
                    .map_err(|_| ProviderContinuationStoreError::Serialization)?;
                buffer.push(b'\n');
            }

            #[cfg(unix)]
            {
                let parent = harness_fs::open_parent_dir_no_symlink_for_write(&self.path)
                    .map_err(fs_error)?;
                let temp_name = std::ffi::OsString::from(format!(
                    "{}.{}.{}.prune.tmp",
                    STORE_LABEL,
                    std::process::id(),
                    Utc::now().timestamp_nanos_opt().unwrap_or_default()
                ));
                let mut temp = parent.create_new_file(&temp_name, true).map_err(fs_error)?;
                harness_fs::set_owner_only_file_if_unix(&temp).map_err(fs_error)?;
                if let Err(error) = temp.write_all(&buffer) {
                    parent.unlink_file_if_exists(&temp_name);
                    return Err(io_error(error));
                }
                if let Err(error) = temp.sync_all() {
                    parent.unlink_file_if_exists(&temp_name);
                    return Err(io_error(error));
                }
                drop(temp);
                parent
                    .rename_file(&temp_name, parent.file_name())
                    .map_err(fs_error)?;
                let file = parent
                    .open_existing_file(parent.file_name())
                    .map_err(fs_error)?;
                harness_fs::set_owner_only_file_if_unix(&file).map_err(fs_error)?;
                parent.sync_all().map_err(fs_error)?;
                return Ok(());
            }

            #[cfg(not(unix))]
            {
                let runtime_dir = self.runtime_dir()?;
                let temp_path = runtime_dir.join(format!(
                    "{}.{}.{}.prune.tmp",
                    STORE_LABEL,
                    std::process::id(),
                    Utc::now().timestamp_nanos_opt().unwrap_or_default()
                ));
                harness_fs::ensure_no_symlink_components(&temp_path).map_err(fs_error)?;
                let mut open_options = OpenOptions::new();
                open_options.create_new(true).write(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;

                    open_options.custom_flags(libc::O_NOFOLLOW);
                    open_options.mode(0o600);
                }
                let mut temp = open_options.open(&temp_path).map_err(io_error)?;
                harness_fs::set_owner_only_file_if_unix(&temp).map_err(fs_error)?;
                temp.write_all(&buffer).map_err(io_error)?;
                temp.sync_all().map_err(io_error)?;
                drop(temp);

                harness_fs::ensure_no_symlink_components(&self.path).map_err(fs_error)?;
                fs::rename(&temp_path, &self.path).map_err(io_error)?;
                harness_fs::set_owner_only_if_exists_unix(&self.path).map_err(fs_error)?;
                harness_fs::sync_directory(runtime_dir).map_err(fs_error)?;
                Ok(())
            }
        })
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

fn lock_path_for(path: &Path) -> PathBuf {
    path.with_file_name(format!("{STORE_LABEL}.lock"))
}

fn open_existing_file_no_follow(
    path: &Path,
) -> Result<Option<File>, ProviderContinuationStoreError> {
    let mut open_options = OpenOptions::new();
    open_options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        open_options.custom_flags(libc::O_NOFOLLOW);
    }

    match open_options.open(path) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(error)),
    }
}

fn fs_error(error: harness_fs::FsError) -> ProviderContinuationStoreError {
    match error {
        harness_fs::FsError::Io(source) => ProviderContinuationStoreError::Io { source },
        other => io_error(io::Error::other(other.to_string())),
    }
}

fn io_error(source: io::Error) -> ProviderContinuationStoreError {
    ProviderContinuationStoreError::Io { source }
}

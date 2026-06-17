//! Blob storage contracts and builtin local implementations.
//!
//! SPEC: docs/architecture/harness/crates/harness-contracts.md §3.7

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::{self, BoxStream};
pub use harness_contracts::{
    BlobError, BlobId, BlobMeta, BlobRef, BlobRetention, BlobStore, TenantId, TranscriptRef,
};
use tokio::sync::Mutex;

pub const DEFAULT_INLINE_BLOB_THRESHOLD_BYTES: u64 = 1024 * 1024;

#[derive(Debug)]
pub struct FileBlobStore {
    root: PathBuf,
    store_id: String,
    content_addressed: bool,
}

impl FileBlobStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, BlobError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            store_id: "file".to_owned(),
            content_addressed: false,
        })
    }

    pub fn open_content_addressed(root: impl AsRef<Path>) -> Result<Self, BlobError> {
        let mut store = Self::open(root)?;
        store.content_addressed = true;
        Ok(store)
    }

    fn paths(&self, tenant: TenantId, id: BlobId) -> (PathBuf, PathBuf) {
        let id_text = id.to_string();
        let dir = self.root.join(tenant.to_string()).join(&id_text[..2]);
        (
            dir.join(format!("{id}.bin")),
            dir.join(format!("{id}.meta.json")),
        )
    }

    fn find_existing(
        &self,
        tenant: TenantId,
        content_hash: [u8; 32],
    ) -> Result<Option<(BlobId, BlobMeta)>, BlobError> {
        let tenant_dir = self.root.join(tenant.to_string());
        let Ok(prefixes) = fs::read_dir(tenant_dir) else {
            return Ok(None);
        };
        for prefix in prefixes {
            let prefix = prefix?.path();
            if !prefix.is_dir() {
                continue;
            }
            for entry in fs::read_dir(prefix)? {
                let path = entry?.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let Some(id) = name.strip_suffix(".meta.json") else {
                    continue;
                };
                let meta: BlobMeta =
                    serde_json::from_slice(&fs::read(&path)?).map_err(blob_error)?;
                if meta.content_hash == content_hash {
                    return Ok(Some((id.parse().map_err(blob_error)?, meta)));
                }
            }
        }
        Ok(None)
    }

    pub fn inventory(&self, tenant: TenantId) -> Result<Vec<(BlobRef, BlobMeta)>, BlobError> {
        let tenant_dir = self.root.join(tenant.to_string());
        let Ok(prefixes) = fs::read_dir(tenant_dir) else {
            return Ok(Vec::new());
        };
        let mut blobs = Vec::new();
        for prefix in prefixes {
            let prefix = prefix?.path();
            if !prefix.is_dir() {
                continue;
            }
            for entry in fs::read_dir(prefix)? {
                let path = entry?.path();
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let Some(id) = name.strip_suffix(".meta.json") else {
                    continue;
                };
                let id: BlobId = id.parse().map_err(blob_error)?;
                let meta: BlobMeta =
                    serde_json::from_slice(&fs::read(&path)?).map_err(blob_error)?;
                blobs.push((
                    BlobRef {
                        id,
                        size: meta.size,
                        content_hash: meta.content_hash,
                        content_type: meta.content_type.clone(),
                    },
                    meta,
                ));
            }
        }
        Ok(blobs)
    }
}

#[async_trait]
impl BlobStore for FileBlobStore {
    fn store_id(&self) -> &str {
        &self.store_id
    }

    async fn put(
        &self,
        tenant: TenantId,
        bytes: Bytes,
        meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        validate_blob_meta(&bytes, &meta)?;
        if self.content_addressed {
            if let Some((id, existing)) = self.find_existing(tenant, meta.content_hash)? {
                return Ok(BlobRef {
                    id,
                    size: existing.size,
                    content_hash: existing.content_hash,
                    content_type: existing.content_type,
                });
            }
        }
        let id = BlobId::new();
        let (body, meta_path) = self.paths(tenant, id);
        if let Some(dir) = body.parent() {
            fs::create_dir_all(dir)?;
        }
        let body_temp = body.with_extension("bin.tmp");
        let meta_temp = meta_path.with_extension("json.tmp");
        fs::write(&body_temp, &bytes)?;
        fs::rename(&body_temp, &body)?;
        fs::write(&meta_temp, serde_json::to_vec(&meta).map_err(blob_error)?)?;
        fs::rename(&meta_temp, &meta_path)?;
        Ok(BlobRef {
            id,
            size: meta.size,
            content_hash: meta.content_hash,
            content_type: meta.content_type,
        })
    }

    async fn get(
        &self,
        tenant: TenantId,
        blob: &BlobRef,
    ) -> Result<BoxStream<'static, Bytes>, BlobError> {
        let (body, _) = self.paths(tenant, blob.id);
        let bytes = fs::read(body)
            .map(Bytes::from)
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => BlobError::NotFound(blob.id),
                _ => BlobError::from(error),
            })?;
        Ok(Box::pin(stream::once(async move { bytes })))
    }

    async fn head(&self, tenant: TenantId, blob: &BlobRef) -> Result<Option<BlobMeta>, BlobError> {
        let (_, meta_path) = self.paths(tenant, blob.id);
        match fs::read(meta_path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(blob_error),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    async fn delete(&self, tenant: TenantId, blob: &BlobRef) -> Result<(), BlobError> {
        let (body, meta_path) = self.paths(tenant, blob.id);
        for path in [body, meta_path] {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct InMemoryBlobStore {
    blobs: Mutex<HashMap<(TenantId, BlobId), (BlobMeta, Bytes)>>,
}

#[async_trait]
impl BlobStore for InMemoryBlobStore {
    fn store_id(&self) -> &'static str {
        "memory"
    }

    async fn put(
        &self,
        tenant: TenantId,
        bytes: Bytes,
        meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        validate_blob_meta(&bytes, &meta)?;
        let id = BlobId::new();
        self.blobs
            .lock()
            .await
            .insert((tenant, id), (meta.clone(), bytes));
        Ok(BlobRef {
            id,
            size: meta.size,
            content_hash: meta.content_hash,
            content_type: meta.content_type,
        })
    }

    async fn get(
        &self,
        tenant: TenantId,
        blob: &BlobRef,
    ) -> Result<BoxStream<'static, Bytes>, BlobError> {
        let bytes = self
            .blobs
            .lock()
            .await
            .get(&(tenant, blob.id))
            .map(|(_, bytes)| bytes.clone())
            .ok_or(BlobError::NotFound(blob.id))?;
        Ok(Box::pin(stream::once(async move { bytes })))
    }

    async fn head(&self, tenant: TenantId, blob: &BlobRef) -> Result<Option<BlobMeta>, BlobError> {
        Ok(self
            .blobs
            .lock()
            .await
            .get(&(tenant, blob.id))
            .map(|(meta, _)| meta.clone()))
    }

    async fn delete(&self, tenant: TenantId, blob: &BlobRef) -> Result<(), BlobError> {
        self.blobs.lock().await.remove(&(tenant, blob.id));
        Ok(())
    }
}

#[cfg(feature = "sqlite")]
pub struct SqliteBlobStore {
    connection: Mutex<rusqlite::Connection>,
}

#[cfg(feature = "sqlite")]
impl SqliteBlobStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BlobError> {
        let connection = rusqlite::Connection::open(path).map_err(blob_error)?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS blobs (
                    tenant_id TEXT NOT NULL,
                    blob_id TEXT NOT NULL,
                    size INTEGER NOT NULL,
                    content_hash BLOB NOT NULL,
                    content_type TEXT,
                    retention TEXT NOT NULL,
                    body BLOB NOT NULL,
                    created_at TEXT NOT NULL,
                    PRIMARY KEY (tenant_id, blob_id)
                ) STRICT;",
            )
            .map_err(blob_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
}

#[cfg(feature = "sqlite")]
#[async_trait]
impl BlobStore for SqliteBlobStore {
    fn store_id(&self) -> &'static str {
        "sqlite"
    }

    async fn put(
        &self,
        tenant: TenantId,
        bytes: Bytes,
        meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        validate_blob_meta(&bytes, &meta)?;
        let id = BlobId::new();
        self.connection
            .lock()
            .await
            .execute(
                "INSERT INTO blobs (
                    tenant_id, blob_id, size, content_hash, content_type, retention, body, created_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    tenant.to_string(),
                    id.to_string(),
                    meta.size as i64,
                    meta.content_hash.as_slice(),
                    meta.content_type,
                    serde_json::to_string(&meta.retention).map_err(blob_error)?,
                    bytes.to_vec(),
                    meta.created_at.to_rfc3339()
                ],
            )
            .map_err(blob_error)?;
        Ok(BlobRef {
            id,
            size: meta.size,
            content_hash: meta.content_hash,
            content_type: meta.content_type,
        })
    }

    async fn get(
        &self,
        tenant: TenantId,
        blob: &BlobRef,
    ) -> Result<BoxStream<'static, Bytes>, BlobError> {
        let body: Vec<u8> = self
            .connection
            .lock()
            .await
            .query_row(
                "SELECT body FROM blobs WHERE tenant_id = ?1 AND blob_id = ?2",
                rusqlite::params![tenant.to_string(), blob.id.to_string()],
                |row| row.get(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => BlobError::NotFound(blob.id),
                _ => blob_error(error),
            })?;
        Ok(Box::pin(stream::once(async move { Bytes::from(body) })))
    }

    async fn head(&self, tenant: TenantId, blob: &BlobRef) -> Result<Option<BlobMeta>, BlobError> {
        let result: Result<(i64, Vec<u8>, Option<String>, String, String), rusqlite::Error> =
            self.connection.lock().await.query_row(
                "SELECT size, content_hash, content_type, retention, created_at
             FROM blobs WHERE tenant_id = ?1 AND blob_id = ?2",
                rusqlite::params![tenant.to_string(), blob.id.to_string()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            );
        match result {
            Ok((size, content_hash, content_type, retention, created_at)) => {
                let content_hash: [u8; 32] = content_hash
                    .try_into()
                    .map_err(|_| BlobError::Backend("invalid blob hash length".to_owned()))?;
                Ok(Some(BlobMeta {
                    content_type,
                    size: size as u64,
                    content_hash,
                    created_at: chrono::DateTime::parse_from_rfc3339(&created_at)
                        .map_err(blob_error)?
                        .with_timezone(&chrono::Utc),
                    retention: serde_json::from_str(&retention).map_err(blob_error)?,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(blob_error(error)),
        }
    }

    async fn delete(&self, tenant: TenantId, blob: &BlobRef) -> Result<(), BlobError> {
        self.connection
            .lock()
            .await
            .execute(
                "DELETE FROM blobs WHERE tenant_id = ?1 AND blob_id = ?2",
                rusqlite::params![tenant.to_string(), blob.id.to_string()],
            )
            .map_err(blob_error)?;
        Ok(())
    }
}

#[cfg(feature = "sqlite")]
pub struct HybridBlobStore {
    sqlite: SqliteBlobStore,
    file: FileBlobStore,
    inline_threshold: u64,
}

#[cfg(feature = "sqlite")]
impl HybridBlobStore {
    pub fn new(sqlite: SqliteBlobStore, file: FileBlobStore, inline_threshold: u64) -> Self {
        Self {
            sqlite,
            file,
            inline_threshold,
        }
    }

    pub fn with_default_threshold(sqlite: SqliteBlobStore, file: FileBlobStore) -> Self {
        Self::new(sqlite, file, DEFAULT_INLINE_BLOB_THRESHOLD_BYTES)
    }

    fn use_sqlite(&self, size: u64) -> bool {
        size <= self.inline_threshold
    }
}

#[cfg(feature = "sqlite")]
#[async_trait]
impl BlobStore for HybridBlobStore {
    fn store_id(&self) -> &'static str {
        "hybrid"
    }

    async fn put(
        &self,
        tenant: TenantId,
        bytes: Bytes,
        meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        if self.use_sqlite(meta.size) {
            self.sqlite.put(tenant, bytes, meta).await
        } else {
            self.file.put(tenant, bytes, meta).await
        }
    }

    async fn get(
        &self,
        tenant: TenantId,
        blob: &BlobRef,
    ) -> Result<BoxStream<'static, Bytes>, BlobError> {
        if self.use_sqlite(blob.size) {
            self.sqlite.get(tenant, blob).await
        } else {
            self.file.get(tenant, blob).await
        }
    }

    async fn head(&self, tenant: TenantId, blob: &BlobRef) -> Result<Option<BlobMeta>, BlobError> {
        if self.use_sqlite(blob.size) {
            self.sqlite.head(tenant, blob).await
        } else {
            self.file.head(tenant, blob).await
        }
    }

    async fn delete(&self, tenant: TenantId, blob: &BlobRef) -> Result<(), BlobError> {
        if self.use_sqlite(blob.size) {
            self.sqlite.delete(tenant, blob).await
        } else {
            self.file.delete(tenant, blob).await
        }
    }
}

fn blob_error(error: impl std::fmt::Display) -> BlobError {
    BlobError::Backend(error.to_string())
}

fn validate_blob_meta(bytes: &[u8], meta: &BlobMeta) -> Result<(), BlobError> {
    if meta.size != bytes.len() as u64 {
        return Err(BlobError::Backend("blob size metadata mismatch".to_owned()));
    }
    let actual = *blake3::hash(bytes).as_bytes();
    if meta.content_hash != actual {
        return Err(BlobError::HashMismatch {
            expected: hex(&meta.content_hash),
            actual: hex(&actual),
        });
    }
    Ok(())
}

fn hex(bytes: &[u8; 32]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(TABLE[(byte >> 4) as usize] as char);
        out.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    out
}

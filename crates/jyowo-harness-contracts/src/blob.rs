//! Blob references and storage contracts.
//!

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::{stream, stream::BoxStream, StreamExt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{BlobId, SessionId, TenantId};

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct BlobRef {
    pub id: BlobId,
    pub size: u64,
    pub content_hash: [u8; 32],
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct TranscriptRef {
    pub blob: BlobRef,
    pub from_offset: crate::JournalOffset,
    pub to_offset: crate::JournalOffset,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct BlobMeta {
    pub content_type: Option<String>,
    pub size: u64,
    pub content_hash: [u8; 32],
    pub created_at: DateTime<Utc>,
    pub retention: BlobRetention,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlobRetention {
    SessionScoped(SessionId),
    TenantScoped,
    RetainForever,
    TtlDays(u32),
}

#[non_exhaustive]
#[derive(
    Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema, thiserror::Error,
)]
#[serde(rename_all = "snake_case")]
pub enum BlobError {
    #[error("blob not found: {0:?}")]
    NotFound(BlobId),
    #[error("content hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("size exceeds limit: {size} > {limit}")]
    TooLarge { size: u64, limit: u64 },
    #[error("tenant denied: {0:?}")]
    TenantDenied(TenantId),
    #[error("io: {0}")]
    Io(String),
    #[error("backend: {0}")]
    Backend(String),
}

impl From<std::io::Error> for BlobError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

#[async_trait::async_trait]
pub trait BlobStore: Send + Sync + 'static {
    fn store_id(&self) -> &str;

    async fn put(
        &self,
        tenant: TenantId,
        bytes: Bytes,
        meta: BlobMeta,
    ) -> Result<BlobRef, BlobError>;

    async fn get(
        &self,
        tenant: TenantId,
        blob: &BlobRef,
    ) -> Result<BoxStream<'static, Bytes>, BlobError>;

    async fn get_range(
        &self,
        tenant: TenantId,
        blob: &BlobRef,
        offset: u64,
        limit: u64,
    ) -> Result<BoxStream<'static, Bytes>, BlobError> {
        if limit == 0 {
            return Ok(Box::pin(stream::empty()));
        }

        let mut stream = self.get(tenant, blob).await?;
        let mut position = 0_u64;
        let end = offset.saturating_add(limit);
        let mut page = Vec::with_capacity(limit.min(8192) as usize);
        while let Some(chunk) = stream.next().await {
            let chunk_len = chunk.len() as u64;
            let chunk_start = position;
            let chunk_end = position.saturating_add(chunk_len);

            if chunk_end > offset && chunk_start < end {
                let slice_start = offset.saturating_sub(chunk_start) as usize;
                let slice_end = end.min(chunk_end).saturating_sub(chunk_start) as usize;
                page.extend_from_slice(&chunk[slice_start..slice_end]);
            }

            position = chunk_end;
            if position >= end {
                break;
            }
        }

        Ok(Box::pin(stream::once(async move { Bytes::from(page) })))
    }

    async fn head(&self, tenant: TenantId, blob: &BlobRef) -> Result<Option<BlobMeta>, BlobError>;

    async fn delete(&self, tenant: TenantId, blob: &BlobRef) -> Result<(), BlobError>;
}

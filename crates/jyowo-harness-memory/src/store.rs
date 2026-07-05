use async_trait::async_trait;
use harness_contracts::{MemoryError, MemoryId};

use crate::{MemoryListScope, MemoryQuery, MemoryRecord, MemorySummary};

#[async_trait]
pub trait MemoryStore: Send + Sync + 'static {
    fn provider_id(&self) -> &str;

    async fn recall(&self, query: MemoryQuery) -> Result<Vec<MemoryRecord>, MemoryError>;

    async fn get(&self, id: MemoryId) -> Result<MemoryRecord, MemoryError> {
        Err(MemoryError::NotFound(id))
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError>;

    async fn forget(&self, id: MemoryId) -> Result<(), MemoryError>;

    async fn rollback_uncommitted_upsert(&self, id: MemoryId) -> Result<(), MemoryError> {
        self.forget(id).await
    }

    async fn rollback_uncommitted_forget(&self, record: MemoryRecord) -> Result<(), MemoryError> {
        self.upsert(record).await.map(|_| ())
    }

    async fn list(&self, scope: MemoryListScope) -> Result<Vec<MemorySummary>, MemoryError>;
}

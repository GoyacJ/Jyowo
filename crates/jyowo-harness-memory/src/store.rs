use async_trait::async_trait;
use harness_contracts::{MemoryError, MemoryId};

use crate::{MemoryListScope, MemoryQuery, MemoryRecord, MemorySummary};

#[async_trait]
pub trait MemoryStore: Send + Sync + 'static {
    fn provider_id(&self) -> &str;

    async fn recall(&self, query: MemoryQuery) -> Result<Vec<MemoryRecord>, MemoryError>;

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError>;

    async fn forget(&self, id: MemoryId) -> Result<(), MemoryError>;

    async fn list(&self, scope: MemoryListScope) -> Result<Vec<MemorySummary>, MemoryError>;
}

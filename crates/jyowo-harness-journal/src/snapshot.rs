//! Snapshot types and storage abstraction.
//!
//! SPEC: docs/architecture/harness/crates/harness-journal.md §2.4

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use harness_contracts::{JournalError, JournalOffset, SessionId, SnapshotId, TenantId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub offset: JournalOffset,
    pub taken_at: DateTime<Utc>,
    pub body: SnapshotBody,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotBody {
    Full(Vec<u8>),
    Delta { base: SnapshotId, patch: Vec<u8> },
}

#[async_trait]
pub trait SnapshotStore: Send + Sync + 'static {
    async fn load_snapshot(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, JournalError>;

    async fn save_snapshot(
        &self,
        tenant: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<(), JournalError>;
}

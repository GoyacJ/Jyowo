//! Retention policy and prune report types.
//!
//! SPEC: docs/architecture/harness/crates/harness-journal.md §7

use std::collections::HashSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{BlobId, BlobRetention, BlobStore, FileBlobStore, TenantId};

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct PrunePolicy {
    pub older_than: Duration,
    pub keep_snapshots: bool,
    pub keep_latest_n_sessions: Option<u32>,
    pub target_size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PruneReport {
    pub events_removed: u64,
    pub snapshots_removed: u64,
    pub bytes_freed: u64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RetentionEnforcer {
    policy: PrunePolicy,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum RetentionStrategy {
    Ttl,
    SessionScoped,
    TenantScoped,
    RetainForever,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlobGcReport {
    pub scanned: u64,
    pub deleted: u64,
    pub freed_bytes: u64,
}

impl Default for RetentionEnforcer {
    fn default() -> Self {
        Self {
            policy: PrunePolicy {
                older_than: Duration::ZERO,
                keep_snapshots: true,
                keep_latest_n_sessions: None,
                target_size_bytes: None,
            },
        }
    }
}

impl RetentionEnforcer {
    pub fn new(policy: PrunePolicy) -> Self {
        Self { policy }
    }

    pub fn policy(&self) -> &PrunePolicy {
        &self.policy
    }

    pub async fn collect_garbage(
        &self,
        tenant: TenantId,
        store: &FileBlobStore,
        live_refs: &HashSet<BlobId>,
    ) -> Result<BlobGcReport, crate::BlobError> {
        let mut report = BlobGcReport::default();
        for (blob, meta) in store.inventory(tenant)? {
            report.scanned += 1;
            if live_refs.contains(&blob.id) || !is_collectable(&meta.retention, meta.created_at) {
                continue;
            }
            store.delete(tenant, &blob).await?;
            report.deleted += 1;
            report.freed_bytes += meta.size;
        }
        Ok(report)
    }
}

fn is_collectable(retention: &BlobRetention, created_at: chrono::DateTime<chrono::Utc>) -> bool {
    match retention {
        BlobRetention::TtlDays(days) => {
            let expires_at = created_at + chrono::Duration::days(i64::from(*days));
            expires_at <= chrono::Utc::now()
        }
        BlobRetention::SessionScoped(_) => true,
        _ => false,
    }
}

use async_trait::async_trait;
#[cfg(feature = "consolidation")]
use harness_contracts::MemoryId;
use harness_contracts::{
    ContentHash, Event, MemdirFileTag, MemoryError, MemoryKind, MemorySessionCtx, MemoryVisibility,
    MemoryWriteAction, MemoryWriteTarget, MessageView, SessionId, SessionSummaryView, ThreatAction,
    ThreatCategory, UserMessageView,
};

use crate::MemoryStore;

#[async_trait]
pub trait MemoryEventSink: Send + Sync + 'static {
    async fn emit(&self, event: Event);

    async fn emit_required(&self, event: Event) -> Result<(), MemoryError> {
        let _ = event;
        Err(MemoryError::Provider {
            provider: "audit".to_owned(),
            source_message: "required audit sink is not implemented".to_owned(),
        })
    }
}

pub trait MemoryMetricsSink: Send + Sync + 'static {
    fn record(&self, metric: MemoryMetric);
}

#[derive(Debug, Clone, PartialEq)]
pub enum MemoryMetric {
    Recall {
        provider_id: Option<String>,
        outcome: MemoryRecallMetricOutcome,
        duration_ms: u32,
        returned_count: u32,
    },
    RecallDegraded {
        provider_id: Option<String>,
        reason: String,
    },
    RecallHitRateSample {
        provider_id: Option<String>,
        hit: bool,
    },
    ThreatDetected {
        category: ThreatCategory,
        action: ThreatAction,
    },
    MemdirWrite {
        file: MemdirFileTag,
        action: MemoryWriteAction,
        bytes_written: u64,
    },
    MemdirBytes {
        file: MemdirFileTag,
        bytes: u64,
    },
    MemdirOverflow {
        file: MemdirFileTag,
        current_chars: u64,
        threshold: u64,
    },
    MemdirLockWait {
        file: MemdirFileTag,
        waited_ms: u32,
    },
    MemdirLockFailed {
        file: MemdirFileTag,
        retries: u32,
    },
    #[cfg(feature = "consolidation")]
    ConsolidationRan {
        hook_id: String,
        promoted: u32,
        demoted: u32,
    },
    ExternalProviderConfigured {
        configured: bool,
    },
    Upsert {
        kind: MemoryKind,
        visibility: MemoryVisibility,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum MemoryRecallMetricOutcome {
    Recalled,
    Empty,
    Skipped,
    Degraded,
}

#[cfg(feature = "consolidation")]
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct ConsolidationOutcome {
    pub promoted: Vec<MemoryId>,
    pub demoted: Vec<MemoryId>,
    pub inbox_candidates_created: u32,
}

#[cfg(feature = "consolidation")]
#[async_trait]
pub trait ConsolidationHook: Send + Sync + 'static {
    fn hook_id(&self) -> &str;

    async fn on_session_end(
        &self,
        ctx: &MemorySessionCtx<'_>,
        summary: &SessionSummaryView<'_>,
    ) -> Result<ConsolidationOutcome, MemoryError>;
}

#[async_trait]
pub trait MemoryLifecycle: Send + Sync + 'static {
    async fn initialize(&self, ctx: &MemorySessionCtx<'_>) -> Result<(), MemoryError> {
        let _ = ctx;
        Ok(())
    }

    async fn on_turn_start(
        &self,
        turn: u32,
        message: &UserMessageView<'_>,
    ) -> Result<(), MemoryError> {
        let _ = (turn, message);
        Ok(())
    }

    async fn on_pre_compress(
        &self,
        messages: &[MessageView<'_>],
    ) -> Result<Option<String>, MemoryError> {
        let _ = messages;
        Ok(None)
    }

    async fn on_memory_write(
        &self,
        action: MemoryWriteAction,
        target: &MemoryWriteTarget,
        content_hash: ContentHash,
    ) -> Result<(), MemoryError> {
        let _ = (action, target, content_hash);
        Ok(())
    }

    async fn on_delegation(
        &self,
        task: &str,
        result: &str,
        child_session: SessionId,
    ) -> Result<(), MemoryError> {
        let _ = (task, result, child_session);
        Ok(())
    }

    async fn on_session_end(
        &self,
        ctx: &MemorySessionCtx<'_>,
        summary: &SessionSummaryView<'_>,
    ) -> Result<(), MemoryError> {
        let _ = (ctx, summary);
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), MemoryError> {
        Ok(())
    }
}

pub trait MemoryProvider: MemoryStore + MemoryLifecycle {
    fn descriptor(&self) -> MemoryProviderDescriptor {
        MemoryProviderDescriptor {
            provider_id: self.provider_id().to_owned(),
            provider_kind: MemoryProviderKind::Local,
            priority: 0,
            trust_level: MemoryProviderTrust::BuiltIn,
            tenant_scope: None,
            workspace_scope: None,
            durability: MemoryProviderDurability::Durable,
            readable: true,
            writable: true,
            allowed_visibility: vec![
                MemoryVisibilityClass::Private,
                MemoryVisibilityClass::User,
                MemoryVisibilityClass::Team,
                MemoryVisibilityClass::Tenant,
            ],
            supports_evidence: true,
            supports_raw_content_export: false,
            timeout_ms: 5000,
            max_records_per_recall: 50,
            max_chars_per_recall: 100_000,
            max_bytes_per_record: 1024 * 1024,
        }
    }
}

use harness_contracts::{
    MemoryProviderDurability, MemoryProviderKind, MemoryProviderTrust, MemoryVisibilityClass,
    TenantId, WorkspaceId,
};

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryProviderDescriptor {
    pub provider_id: String,
    pub provider_kind: MemoryProviderKind,
    pub priority: i32,
    pub trust_level: MemoryProviderTrust,
    pub tenant_scope: Option<TenantId>,
    pub workspace_scope: Option<WorkspaceId>,
    pub durability: MemoryProviderDurability,
    pub readable: bool,
    pub writable: bool,
    pub allowed_visibility: Vec<MemoryVisibilityClass>,
    pub supports_evidence: bool,
    pub supports_raw_content_export: bool,
    pub timeout_ms: u32,
    pub max_records_per_recall: u32,
    pub max_chars_per_recall: u32,
    pub max_bytes_per_record: u64,
}

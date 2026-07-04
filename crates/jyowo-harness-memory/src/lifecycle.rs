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
        self.emit(event).await;
        Ok(())
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

pub trait MemoryProvider: MemoryStore + MemoryLifecycle {}

impl<T: MemoryStore + MemoryLifecycle> MemoryProvider for T {}

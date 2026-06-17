#![cfg(feature = "external-slot")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    ContentHash, EndReason, MemoryError, MemoryId, MemoryKind, MemorySessionCtx, MemoryVisibility,
    MemoryWriteAction, MemoryWriteTarget, MessageView, SessionId, SessionSummaryView,
    UsageSnapshot, UserMessageView, WriteDestination,
};
use harness_memory::{
    MemoryLifecycle, MemoryListScope, MemoryManager, MemoryQuery, MemoryRecord, MemoryStore,
    MemorySummary,
};

#[tokio::test]
async fn memory_manager_forwards_lifecycle_hooks() {
    let manager = MemoryManager::new();
    let provider = Arc::new(RecordingProvider::default());
    manager.set_external(provider.clone()).unwrap();
    let ctx = MemorySessionCtx {
        tenant_id: harness_contracts::TenantId::SINGLE,
        session_id: SessionId::new(),
        workspace_id: None,
        user_id: Some("user"),
        team_id: None,
    };
    let message = UserMessageView {
        text: "remember this",
        turn: 1,
        at: chrono::Utc::now(),
    };
    let target = MemoryWriteTarget {
        kind: MemoryKind::UserPreference,
        visibility: MemoryVisibility::Tenant,
        destination: WriteDestination::External {
            provider_id: "recording".to_owned(),
        },
    };
    let summary = SessionSummaryView {
        end_reason: EndReason::Completed,
        turn_count: 1,
        tool_use_count: 0,
        usage: UsageSnapshot::default(),
        final_assistant_text: Some("done"),
    };

    manager.initialize_session(&ctx).await.unwrap();
    manager.on_turn_start(1, &message).await.unwrap();
    manager
        .on_pre_compress(&[MessageView {
            role: harness_contracts::MessageRole::User,
            text_snippet: "remember this",
            tool_use_id: None,
        }])
        .await
        .unwrap();
    manager
        .on_memory_write(MemoryWriteAction::Upsert, &target, ContentHash([1; 32]))
        .await
        .unwrap();
    manager
        .on_delegation("task", "result", SessionId::new())
        .await
        .unwrap();
    manager.on_session_end(&ctx, &summary).await.unwrap();

    assert_eq!(provider.initialize.load(Ordering::SeqCst), 1);
    assert_eq!(provider.turn_start.load(Ordering::SeqCst), 1);
    assert_eq!(provider.pre_compress.load(Ordering::SeqCst), 1);
    assert_eq!(provider.memory_write.load(Ordering::SeqCst), 1);
    assert_eq!(provider.delegation.load(Ordering::SeqCst), 1);
    assert_eq!(provider.session_end.load(Ordering::SeqCst), 1);
    assert_eq!(provider.shutdown.load(Ordering::SeqCst), 1);
}

#[derive(Default)]
struct RecordingProvider {
    initialize: AtomicUsize,
    turn_start: AtomicUsize,
    pre_compress: AtomicUsize,
    memory_write: AtomicUsize,
    delegation: AtomicUsize,
    session_end: AtomicUsize,
    shutdown: AtomicUsize,
}

#[async_trait]
impl MemoryStore for RecordingProvider {
    fn provider_id(&self) -> &'static str {
        "recording"
    }

    async fn recall(&self, _query: MemoryQuery) -> Result<Vec<MemoryRecord>, MemoryError> {
        Ok(Vec::new())
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _id: MemoryId) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn list(&self, _scope: MemoryListScope) -> Result<Vec<MemorySummary>, MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl MemoryLifecycle for RecordingProvider {
    async fn initialize(&self, _ctx: &MemorySessionCtx<'_>) -> Result<(), MemoryError> {
        self.initialize.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn on_turn_start(
        &self,
        _turn: u32,
        _message: &UserMessageView<'_>,
    ) -> Result<(), MemoryError> {
        self.turn_start.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn on_pre_compress(
        &self,
        _messages: &[MessageView<'_>],
    ) -> Result<Option<String>, MemoryError> {
        self.pre_compress.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }

    async fn on_memory_write(
        &self,
        _action: MemoryWriteAction,
        _target: &MemoryWriteTarget,
        _content_hash: ContentHash,
    ) -> Result<(), MemoryError> {
        self.memory_write.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn on_delegation(
        &self,
        _task: &str,
        _result: &str,
        _child_session: SessionId,
    ) -> Result<(), MemoryError> {
        self.delegation.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn on_session_end(
        &self,
        _ctx: &MemorySessionCtx<'_>,
        _summary: &SessionSummaryView<'_>,
    ) -> Result<(), MemoryError> {
        self.session_end.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), MemoryError> {
        self.shutdown.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

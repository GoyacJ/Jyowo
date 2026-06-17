use std::collections::BTreeSet;
use std::time::Duration;

use harness_contracts::{
    MemoryActor, MemoryError, MemoryId, MemoryKind, MemorySessionCtx, MemoryVisibility,
    MessageView, SessionId, TenantId,
};
use harness_memory::*;

struct NoopMemory;

#[async_trait::async_trait]
impl MemoryStore for NoopMemory {
    fn provider_id(&self) -> &'static str {
        "noop"
    }

    async fn recall(&self, _: MemoryQuery) -> Result<Vec<MemoryRecord>, MemoryError> {
        Ok(Vec::new())
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _: MemoryId) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn list(&self, _: MemoryListScope) -> Result<Vec<MemorySummary>, MemoryError> {
        Ok(Vec::new())
    }
}

impl MemoryLifecycle for NoopMemory {}

fn dyn_safe(_: &dyn MemoryStore, _: &dyn MemoryLifecycle, _: &dyn MemoryProvider) {}

#[tokio::test]
async fn memory_store_and_lifecycle_traits_are_object_safe() {
    let memory = NoopMemory;
    dyn_safe(&memory, &memory, &memory);

    assert_eq!(memory.provider_id(), "noop");
    assert!(memory.recall(query(None)).await.unwrap().is_empty());
    memory.initialize(&ctx()).await.unwrap();
    assert_eq!(
        memory
            .on_pre_compress(&[] as &[MessageView<'_>])
            .await
            .unwrap(),
        None
    );
    memory.shutdown().await.unwrap();
}

#[test]
fn memory_types_preserve_tenant_and_visibility_boundaries() {
    let session_id = SessionId::new();
    let private = MemoryVisibility::Private { session_id };

    assert_eq!(query(Some(session_id)).tenant_id, TenantId::SINGLE);
    assert!(visibility_matches(&private, &actor(Some(session_id))));
    assert!(!visibility_matches(
        &private,
        &actor(Some(SessionId::new()))
    ));
}

fn query(session_id: Option<SessionId>) -> MemoryQuery {
    MemoryQuery {
        text: "preference".to_owned(),
        kind_filter: Some(MemoryKindFilter::OnlyKinds(BTreeSet::from([
            MemoryKind::UserPreference,
        ]))),
        visibility_filter: MemoryVisibilityFilter::EffectiveFor(actor(session_id)),
        max_records: 3,
        min_similarity: 0.75,
        tenant_id: TenantId::SINGLE,
        session_id,
        deadline: Some(Duration::from_secs(1)),
    }
}

fn actor(session_id: Option<SessionId>) -> MemoryActor {
    MemoryActor {
        tenant_id: TenantId::SINGLE,
        user_id: Some("user-1".to_owned()),
        team_id: None,
        session_id,
    }
}

fn ctx() -> MemorySessionCtx<'static> {
    MemorySessionCtx {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        workspace_id: None,
        user_id: Some("user-1"),
        team_id: None,
    }
}

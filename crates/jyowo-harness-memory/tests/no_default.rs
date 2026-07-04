#![cfg(not(any(
    feature = "builtin",
    feature = "external-slot",
    feature = "threat-scanner"
)))]

use std::collections::BTreeSet;
use std::time::Duration;

use async_trait::async_trait;
use harness_contracts::{
    MemoryActorContext, MemoryError, MemoryId, MemoryKind, MemorySessionCtx, MemoryVisibility,
    MessageView, SessionId, TeamId, TenantId,
};
use harness_memory::{
    content_preview, visibility_matches, MemoryKindFilter, MemoryLifecycle, MemoryListScope,
    MemoryProvider, MemoryQuery, MemoryRecord, MemoryStore, MemorySummary, MemoryVisibilityFilter,
    MEMORY_CONTENT_PREVIEW_MAX_CHARS,
};

struct MinimalProvider;

#[async_trait]
impl MemoryStore for MinimalProvider {
    fn provider_id(&self) -> &str {
        "minimal"
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

impl MemoryLifecycle for MinimalProvider {}

fn assert_provider(_: &dyn MemoryProvider) {}

#[tokio::test]
async fn no_default_core_provider_contract_stays_available() {
    let provider = MinimalProvider;
    assert_provider(&provider);

    provider.initialize(&ctx()).await.unwrap();
    let facts = provider
        .on_pre_compress(&[] as &[MessageView<'_>])
        .await
        .unwrap();

    assert_eq!(provider.provider_id(), "minimal");
    assert!(facts.is_none());
    assert!(provider.recall(query()).await.unwrap().is_empty());
}

#[test]
fn no_default_core_types_keep_visibility_and_preview_contracts() {
    let session_id = SessionId::new();
    let team_id = TeamId::new();
    let actor = MemoryActorContext {
        tenant_id: TenantId::SINGLE,
        user_id: Some("user-1".to_owned()),
        team_id: Some(team_id),
        session_id: Some(session_id),
    };

    assert!(visibility_matches(
        &MemoryVisibility::User {
            user_id: "user-1".to_owned()
        },
        &actor
    ));
    assert!(visibility_matches(
        &MemoryVisibility::Team { team_id },
        &actor
    ));
    assert!(visibility_matches(
        &MemoryVisibility::Private { session_id },
        &actor
    ));

    let preview = content_preview(&"x".repeat(MEMORY_CONTENT_PREVIEW_MAX_CHARS + 20));
    assert_eq!(preview.chars().count(), MEMORY_CONTENT_PREVIEW_MAX_CHARS);
    assert!(preview.ends_with("..."));
}

fn query() -> MemoryQuery {
    MemoryQuery {
        text: "preference".to_owned(),
        kind_filter: Some(MemoryKindFilter::OnlyKinds(BTreeSet::from([
            MemoryKind::UserPreference,
        ]))),
        visibility_filter: MemoryVisibilityFilter::EffectiveFor(MemoryActorContext {
            tenant_id: TenantId::SINGLE,
            user_id: Some("user-1".to_owned()),
            team_id: None,
            session_id: None,
        }),
        max_records: 3,
        min_similarity: 0.75,
        tenant_id: TenantId::SINGLE,
        session_id: None,
        deadline: Some(Duration::from_secs(1)),
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

#![cfg(feature = "external-slot")]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use harness_contracts::{
    MemoryActorContext, MemoryError, MemoryId, MemoryKind, MemorySource, MemoryVisibility, SessionId,
    TenantId,
};
use harness_memory::{
    InMemoryMemoryProvider, MemoryKindFilter, MemoryListScope, MemoryManager, MemoryMetadata,
    MemoryQuery, MemoryRecord, MemoryStore, MemoryVisibilityFilter,
};
use tokio::sync::Barrier;

#[test]
fn memory_manager_accepts_only_one_external_provider() {
    let manager = MemoryManager::new();

    assert!(!manager.has_external());
    manager
        .set_external(Arc::new(InMemoryMemoryProvider::new("first")))
        .unwrap();

    assert!(manager.has_external());
    assert_eq!(manager.external().unwrap().provider_id(), "first");

    let error = manager
        .set_external(Arc::new(InMemoryMemoryProvider::new("second")))
        .unwrap_err();
    assert!(matches!(error, MemoryError::ExternalSlotOccupied));
    assert_eq!(manager.external().unwrap().provider_id(), "first");
}

#[tokio::test]
async fn test_memory_provider_is_tenant_scoped_and_supports_forget() {
    let provider = InMemoryMemoryProvider::new("test");
    let session_id = SessionId::new();
    let kept = record(
        TenantId::SINGLE,
        MemoryVisibility::Private { session_id },
        "single private preference",
    );
    let leaked = record(
        TenantId::SHARED,
        MemoryVisibility::Tenant,
        "shared tenant fact",
    );

    provider.upsert(kept.clone()).await.unwrap();
    provider.upsert(leaked).await.unwrap();

    let recalled = provider
        .recall(query(TenantId::SINGLE, session_id, 5))
        .await
        .unwrap();
    assert_recalled_record(&recalled, &kept);

    assert_eq!(
        provider
            .list(MemoryListScope::ForActor(actor(
                TenantId::SINGLE,
                session_id
            )))
            .await
            .unwrap()
            .len(),
        1
    );

    provider.forget(kept.id).await.unwrap();
    assert!(provider
        .list(MemoryListScope::ForActor(actor(
            TenantId::SINGLE,
            session_id
        )))
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_memory_provider_applies_query_limits_and_filters() {
    let provider = InMemoryMemoryProvider::new("test");
    let session_id = SessionId::new();
    let private = record(
        TenantId::SINGLE,
        MemoryVisibility::Private { session_id },
        "private note",
    );
    let tenant = record(TenantId::SINGLE, MemoryVisibility::Tenant, "tenant note");
    let other_session = record(
        TenantId::SINGLE,
        MemoryVisibility::Private {
            session_id: SessionId::new(),
        },
        "other session note",
    );

    provider.upsert(private.clone()).await.unwrap();
    provider.upsert(tenant.clone()).await.unwrap();
    provider.upsert(other_session).await.unwrap();

    let recalled = provider
        .recall(query(TenantId::SINGLE, session_id, 1))
        .await
        .unwrap();
    assert_recalled_record(&recalled, &private);

    let summaries = provider
        .list(MemoryListScope::ByVisibility(MemoryVisibility::Tenant))
        .await
        .unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].content_preview, tenant.content);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn in_memory_provider_handles_1000_concurrent_recall_and_1000_concurrent_upsert() {
    let provider = Arc::new(InMemoryMemoryProvider::new("test"));
    let session_id = SessionId::new();

    for index in 0..10 {
        provider
            .upsert(record(
                TenantId::SINGLE,
                MemoryVisibility::Tenant,
                &format!("seed-{index:04}"),
            ))
            .await
            .unwrap();
    }

    let barrier = Arc::new(Barrier::new(2001));
    let mut tasks = Vec::with_capacity(2000);

    for index in 0..1000 {
        let recall_provider = Arc::clone(&provider);
        let recall_barrier = Arc::clone(&barrier);
        tasks.push(tokio::spawn(async move {
            recall_barrier.wait().await;
            recall_provider
                .recall(query(TenantId::SINGLE, session_id, 8))
                .await
                .map(|_| ())
        }));

        let upsert_provider = Arc::clone(&provider);
        let upsert_barrier = Arc::clone(&barrier);
        tasks.push(tokio::spawn(async move {
            upsert_barrier.wait().await;
            upsert_provider
                .upsert(record(
                    TenantId::SINGLE,
                    MemoryVisibility::Tenant,
                    &format!("concurrent-{index:04}"),
                ))
                .await
                .map(|_| ())
        }));
    }

    barrier.wait().await;
    for task in tasks {
        task.await.unwrap().unwrap();
    }

    let summaries = provider
        .list(MemoryListScope::ForActor(actor(
            TenantId::SINGLE,
            session_id,
        )))
        .await
        .unwrap();
    assert_eq!(summaries.len(), 1010);
}

fn query(tenant_id: TenantId, session_id: SessionId, max_records: u32) -> MemoryQuery {
    MemoryQuery {
        text: "memory".to_owned(),
        kind_filter: Some(MemoryKindFilter::OnlyKinds(BTreeSet::from([
            MemoryKind::UserPreference,
        ]))),
        visibility_filter: MemoryVisibilityFilter::EffectiveFor(actor(tenant_id, session_id)),
        max_records,
        min_similarity: 0.0,
        tenant_id,
        session_id: Some(session_id),
        deadline: Some(Duration::from_millis(200)),
    }
}

fn actor(tenant_id: TenantId, session_id: SessionId) -> MemoryActorContext {
    MemoryActorContext {
        tenant_id,
        user_id: Some("user-1".to_owned()),
        team_id: None,
        session_id: Some(session_id),
    }
}

fn record(tenant_id: TenantId, visibility: MemoryVisibility, content: &str) -> MemoryRecord {
    let now = Utc::now();
    MemoryRecord {
        id: MemoryId::new(),
        tenant_id,
        kind: MemoryKind::UserPreference,
        visibility,
        content: content.to_owned(),
        metadata: MemoryMetadata {
            tags: Vec::new(),
            source: MemorySource::UserInput,
            confidence: 1.0,
            access_count: 0,
            last_accessed_at: None,
            recall_score: 1.0,
            ttl: None,
            redacted_segments: 0,
        },
        created_at: now,
        updated_at: now,
    }
}

fn assert_recalled_record(recalled: &[MemoryRecord], expected: &MemoryRecord) {
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].id, expected.id);
    assert_eq!(recalled[0].tenant_id, expected.tenant_id);
    assert_eq!(recalled[0].kind, expected.kind);
    assert_eq!(recalled[0].visibility, expected.visibility);
    assert_eq!(recalled[0].content, expected.content);
    assert_eq!(recalled[0].metadata.access_count, 1);
    assert!(recalled[0].metadata.last_accessed_at.is_some());
}

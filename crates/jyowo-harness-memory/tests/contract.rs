#![cfg(any(feature = "builtin", feature = "provider-registry"))]

#[cfg(feature = "provider-registry")]
use std::collections::BTreeSet;
#[cfg(all(feature = "builtin", feature = "provider-registry"))]
use std::sync::Arc;
#[cfg(feature = "provider-registry")]
use std::time::Duration;

#[cfg(feature = "provider-registry")]
use chrono::Utc;
#[cfg(feature = "provider-registry")]
use harness_contracts::{
    MemoryActorContext, MemoryId, MemoryKind, MemorySource, MemoryVisibility, SessionId, TenantId,
};
#[cfg(feature = "provider-registry")]
use harness_memory::{
    MemoryKindFilter, MemoryListScope, MemoryMetadata, MemoryQuery, MemoryRecord, MemoryStore,
    MemoryVisibilityFilter,
};

#[cfg(feature = "provider-registry")]
use harness_memory::InMemoryMemoryProvider;
#[cfg(all(feature = "builtin", feature = "provider-registry"))]
use harness_memory::MemoryManager;
#[cfg(all(feature = "builtin", feature = "provider-registry"))]
use harness_memory::{BuiltinMemory, MemdirFile};

#[cfg(feature = "provider-registry")]
#[tokio::test]
async fn in_memory_provider_contract_upserts_lists_and_forgets() {
    let provider = InMemoryMemoryProvider::new("test-contract");
    let session_id = SessionId::new();
    let kept = record(
        TenantId::SINGLE,
        MemoryVisibility::Private { session_id },
        "tenant scoped memory",
    );
    let other_tenant = record(TenantId::SHARED, MemoryVisibility::Tenant, "other tenant");

    assert_eq!(provider.upsert(kept.clone()).await.unwrap(), kept.id);
    provider.upsert(other_tenant).await.unwrap();

    let recalled = provider
        .recall(query(TenantId::SINGLE, session_id, 8))
        .await
        .unwrap();
    assert_recalled_record(&recalled, &kept);

    let summaries = provider
        .list(MemoryListScope::ForActor(actor(
            TenantId::SINGLE,
            session_id,
        )))
        .await
        .unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, kept.id);

    provider.forget(kept.id).await.unwrap();
    assert!(provider
        .recall(query(TenantId::SINGLE, session_id, 8))
        .await
        .unwrap()
        .is_empty());
}

#[cfg(feature = "provider-registry")]
#[tokio::test]
async fn in_memory_provider_contract_enforces_tenant_isolation() {
    let provider = InMemoryMemoryProvider::new("test-contract");
    let session_id = SessionId::new();
    let kept = record(TenantId::SINGLE, MemoryVisibility::Tenant, "single tenant");
    let leaked = record(TenantId::SHARED, MemoryVisibility::Tenant, "shared tenant");

    provider.upsert(kept.clone()).await.unwrap();
    provider.upsert(leaked).await.unwrap();

    let recalled = provider
        .recall(query(TenantId::SINGLE, session_id, 8))
        .await
        .unwrap();

    assert_recalled_record(&recalled, &kept);
}

#[cfg(feature = "provider-registry")]
#[tokio::test]
async fn in_memory_provider_contract_enforces_private_visibility() {
    let provider = InMemoryMemoryProvider::new("test-contract");
    let session_id = SessionId::new();
    let visible = record(
        TenantId::SINGLE,
        MemoryVisibility::Private { session_id },
        "same session",
    );
    let hidden = record(
        TenantId::SINGLE,
        MemoryVisibility::Private {
            session_id: SessionId::new(),
        },
        "other session",
    );

    provider.upsert(visible.clone()).await.unwrap();
    provider.upsert(hidden).await.unwrap();

    let recalled = provider
        .recall(query(TenantId::SINGLE, session_id, 8))
        .await
        .unwrap();

    assert_recalled_record(&recalled, &visible);
}

#[cfg(feature = "provider-registry")]
#[tokio::test]
async fn in_memory_provider_recall_updates_access_metadata_internally() {
    let provider = InMemoryMemoryProvider::new("test-contract");
    let session_id = SessionId::new();
    let kept = record(
        TenantId::SINGLE,
        MemoryVisibility::Private { session_id },
        "accessed memory",
    );
    let id = kept.id;
    provider.upsert(kept).await.unwrap();

    let recalled = provider
        .recall(query(TenantId::SINGLE, session_id, 8))
        .await
        .unwrap();
    let listed = provider
        .list(MemoryListScope::ForActor(actor(
            TenantId::SINGLE,
            session_id,
        )))
        .await
        .unwrap();

    assert_eq!(recalled.len(), 1);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);
    assert_eq!(listed[0].metadata.access_count, 1);
    assert!(listed[0].metadata.last_accessed_at.is_some());
}

#[cfg(feature = "provider-registry")]
#[tokio::test]
async fn in_memory_provider_list_returns_bounded_content_preview() {
    let provider = InMemoryMemoryProvider::new("test-contract");
    let session_id = SessionId::new();
    let long = record(
        TenantId::SINGLE,
        MemoryVisibility::Private { session_id },
        &"x".repeat(512),
    );
    provider.upsert(long).await.unwrap();

    let summaries = provider
        .list(MemoryListScope::ForActor(actor(
            TenantId::SINGLE,
            session_id,
        )))
        .await
        .unwrap();

    assert_eq!(summaries.len(), 1);
    assert!(summaries[0].content_preview.chars().count() <= 160);
    assert_ne!(summaries[0].content_preview, "x".repeat(512));
}

#[cfg(all(feature = "builtin", feature = "provider-registry"))]
#[tokio::test]
async fn builtin_memdir_does_not_participate_in_external_recall() {
    let root = tempfile::tempdir().unwrap();
    let builtin = BuiltinMemory::at(root.path(), TenantId::SINGLE);
    builtin
        .append_section(MemdirFile::Memory, "profile", "prefers concise answers")
        .await
        .unwrap();

    let manager = MemoryManager::new();
    let session_id = SessionId::new();

    assert!(manager
        .recall(query(TenantId::SINGLE, session_id, 8))
        .await
        .unwrap()
        .is_empty());

    let external = Arc::new(InMemoryMemoryProvider::new("test-contract"));
    external
        .upsert(record(
            TenantId::SINGLE,
            MemoryVisibility::Private { session_id },
            "external preference",
        ))
        .await
        .unwrap();
    manager.register_provider(external).unwrap();

    let recalled = manager
        .recall(query(TenantId::SINGLE, session_id, 8))
        .await
        .unwrap();
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].content, "external preference");
}

#[cfg(all(feature = "builtin", feature = "provider-registry"))]
#[tokio::test]
async fn memory_manager_holds_builtin_memory_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let builtin = BuiltinMemory::at(root.path(), TenantId::SINGLE);
    builtin
        .append_section(MemdirFile::Memory, "profile", "manager-owned fact")
        .await
        .unwrap();

    let manager = MemoryManager::new().with_builtin_memory(builtin);

    assert!(manager.has_builtin());
    let snapshot = manager
        .builtin()
        .expect("builtin memory should be installed")
        .read_all()
        .await
        .unwrap();
    assert!(snapshot.memory.contains("manager-owned fact"));
}

#[cfg(feature = "provider-registry")]
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

#[cfg(feature = "provider-registry")]
fn actor(tenant_id: TenantId, session_id: SessionId) -> MemoryActorContext {
    MemoryActorContext {
        tenant_id,
        user_id: Some("user-1".to_owned()),
        team_id: None,
        session_id: Some(session_id),
    }
}

#[cfg(feature = "provider-registry")]
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

#[cfg(feature = "provider-registry")]
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

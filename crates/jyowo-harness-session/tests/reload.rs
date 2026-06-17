#![cfg(feature = "hot-reload-fork")]

use std::sync::Arc;

use harness_contracts::{
    DeferPolicy, Event, EventId, NoopRedactor, RunId, SessionId, TenantId, ToolProperties,
    ToolUseId, ToolUseRequestedEvent,
};
use harness_journal::{EventStore, InMemoryEventStore};
use harness_session::{
    AddedMcpServer, AddedTool, CacheImpact, CacheInvalidationReason, ConfigDelta, ReloadEffect,
    ReloadMode, Session, SessionOptions,
};
use harness_skill::{SkillPlatform, SkillRegistration, SkillSource};
use serde_json::json;

#[tokio::test]
async fn permission_rule_patch_applies_in_place_without_cache_invalidation() {
    let session = test_session(TenantId::SINGLE).await;

    let outcome = session
        .reload_with(ConfigDelta::for_tenant(TenantId::SINGLE).with_permission_rule_patch())
        .await
        .unwrap();

    assert_eq!(outcome.mode, ReloadMode::AppliedInPlace);
    assert_eq!(outcome.cache_impact, CacheImpact::NoInvalidation);
    assert_eq!(outcome.effective_from, ReloadEffect::Immediate);
}

#[tokio::test]
async fn additive_delta_classifies_cache_impact() {
    let session = test_session(TenantId::SINGLE).await;

    let deferred = session
        .reload_with(
            ConfigDelta::for_tenant(TenantId::SINGLE).add_tool("grep", DeferPolicy::AutoDefer),
        )
        .await
        .unwrap();
    assert_eq!(deferred.cache_impact, CacheImpact::NoInvalidation);

    let always = session
        .reload_with(
            ConfigDelta::for_tenant(TenantId::SINGLE).add_tool("list_dir", DeferPolicy::AlwaysLoad),
        )
        .await
        .unwrap();
    assert!(matches!(
        always.cache_impact,
        CacheImpact::OneShotInvalidation {
            reason: CacheInvalidationReason::ToolsetAppended,
            ..
        }
    ));

    let combined = session_with_skill_reload_cap(TenantId::SINGLE)
        .await
        .reload_with(
            ConfigDelta::for_tenant(TenantId::SINGLE)
                .add_skill(skill_registration("summarizer"))
                .add_mcp_server(AddedMcpServer {
                    id: "fs".to_owned(),
                    tools: vec![AddedTool {
                        name: "stat".to_owned(),
                        defer_policy: DeferPolicy::AlwaysLoad,
                    }],
                }),
        )
        .await
        .unwrap();
    assert_eq!(combined.mode, ReloadMode::AppliedInPlace);
    assert!(matches!(
        combined.cache_impact,
        CacheImpact::OneShotInvalidation { .. }
    ));
}

#[tokio::test]
async fn add_skills_without_reload_cap_is_rejected() {
    let session = test_session(TenantId::SINGLE).await;

    let outcome = session
        .reload_with(ConfigDelta::for_tenant(TenantId::SINGLE).add_skill(skill_registration("x")))
        .await
        .unwrap();

    assert!(matches!(outcome.mode, ReloadMode::Rejected { .. }));
    assert_eq!(outcome.cache_impact, CacheImpact::NoInvalidation);
}

#[tokio::test]
async fn destructive_delta_forks_new_session_with_full_reset() {
    let session = test_session(TenantId::SINGLE).await;

    let outcome = session
        .reload_with(ConfigDelta::for_tenant(TenantId::SINGLE).with_model_ref("gpt-test"))
        .await
        .unwrap();

    let ReloadMode::ForkedNewSession { parent, child } = outcome.mode else {
        panic!("expected fork");
    };
    assert_ne!(parent, child);
    assert_eq!(outcome.cache_impact, CacheImpact::FullReset);
    let child_session = outcome.new_session.unwrap();
    assert_eq!(child_session.projection().await.session_id, child);
}

#[tokio::test]
async fn reload_memdir_delta_forks_new_session_with_full_reset() {
    let session = test_session(TenantId::SINGLE).await;

    let outcome = session
        .reload_with(ConfigDelta::for_tenant(TenantId::SINGLE).reload_memdir())
        .await
        .unwrap();

    let ReloadMode::ForkedNewSession { parent, child } = outcome.mode else {
        panic!("expected fork");
    };
    assert_ne!(parent, child);
    assert_eq!(outcome.cache_impact, CacheImpact::FullReset);
    assert_eq!(outcome.effective_from, ReloadEffect::NextTurn);
    assert!(outcome.new_session.is_some());
}

#[tokio::test]
async fn rejected_deltas_do_not_apply() {
    let session = test_session(TenantId::SINGLE).await;

    let cross_tenant = session
        .reload_with(ConfigDelta::for_tenant(TenantId::SHARED).add_skill(skill_registration("x")))
        .await
        .unwrap();
    assert!(matches!(cross_tenant.mode, ReloadMode::Rejected { .. }));

    let tool_search = session
        .reload_with(ConfigDelta::for_tenant(TenantId::SINGLE).with_tool_search_mode("always"))
        .await
        .unwrap();
    assert!(matches!(tool_search.mode, ReloadMode::Rejected { .. }));
}

#[tokio::test]
async fn removing_running_referenced_tool_is_rejected() {
    let tenant = TenantId::SINGLE;
    let session_id = SessionId::new();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let session = Session::builder()
        .with_options(
            SessionOptions::new(tempfile::tempdir().unwrap().path())
                .with_tenant_id(tenant)
                .with_session_id(session_id),
        )
        .with_event_store(store.clone())
        .build()
        .await
        .unwrap();
    store
        .append(
            tenant,
            session_id,
            &[Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id: RunId::new(),
                tool_use_id: ToolUseId::new(),
                tool_name: "list_dir".to_owned(),
                input: json!({}),
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: true,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                causation_id: EventId::new(),
                at: harness_contracts::now(),
            })],
        )
        .await
        .unwrap();

    let outcome = session
        .reload_with(ConfigDelta::for_tenant(tenant).remove_tool("list_dir"))
        .await
        .unwrap();

    assert!(matches!(outcome.mode, ReloadMode::Rejected { .. }));
}

async fn test_session(tenant: TenantId) -> Session {
    Session::builder()
        .with_options(
            SessionOptions::new(tempfile::tempdir().unwrap().path()).with_tenant_id(tenant),
        )
        .with_event_store(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .build()
        .await
        .unwrap()
}

async fn session_with_skill_reload_cap(tenant: TenantId) -> Session {
    Session::builder()
        .with_options(
            SessionOptions::new(tempfile::tempdir().unwrap().path()).with_tenant_id(tenant),
        )
        .with_event_store(Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))))
        .with_skill_reload_cap(Arc::new(AcceptingSkillReloadCap))
        .build()
        .await
        .unwrap()
}

struct AcceptingSkillReloadCap;

#[async_trait::async_trait]
impl harness_session::SkillReloadCap for AcceptingSkillReloadCap {
    async fn reload_skills(&self, _registrations: &[SkillRegistration]) -> Result<(), String> {
        Ok(())
    }
}

fn skill_registration(name: &str) -> SkillRegistration {
    let skill = harness_skill::parse_skill_markdown(
        &format!(
            r"---
name: {name}
description: Test skill
---
Body
"
        ),
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("skill should parse");
    SkillRegistration {
        skill,
        force_allowlist: None,
    }
}

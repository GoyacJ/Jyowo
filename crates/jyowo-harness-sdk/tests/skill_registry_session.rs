#![cfg(feature = "testing")]

use std::sync::Arc;

use async_trait::async_trait;
use futures::{executor::block_on, StreamExt};
use harness_contracts::{AgentId, Decision, Event, HookEventKind, TenantId, ToolUseId};
use harness_journal::{EventStore, ReplayCursor};
use harness_model::{ContentDelta, ModelStreamEvent};
use harness_skill::{
    parse_skill_markdown, BundledSkillRecord, ConfigResolveError, SkillConfigResolver, SkillLoader,
    SkillPlatform, SkillRegistry, SkillRegistryService, SkillRenderer, SkillSource,
    SkillSourceConfig,
};
use jyowo_harness_sdk::ext::{BuiltinToolset, ToolRegistry};
use jyowo_harness_sdk::{prelude::*, testing::*};
use secrecy::SecretString;
use serde_json::json;
use serde_json::Value;

#[test]
fn skill_registry_session_can_use_skill_tools_without_full_builtin_toolset() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Skills)
        .build()
        .expect("Skill builtin toolset should build");
    let mut names = registry
        .snapshot()
        .as_descriptors()
        .into_iter()
        .map(|descriptor| descriptor.name.clone())
        .collect::<Vec<_>>();
    names.sort();

    assert_eq!(names, ["skills_invoke", "skills_list", "skills_view"]);
    for forbidden in ["bash", "file_read", "file_write", "web_fetch", "web_search"] {
        assert!(
            !names.iter().any(|name| name == forbidden),
            "{forbidden} must require the full builtin-toolset"
        );
    }
}

#[test]
fn skill_registry_session_two_sessions_using_same_loader_do_not_fail_duplicate_skill_registration()
{
    block_on(async {
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let loader = SkillLoader::default().with_source(SkillSourceConfig::BundledRecords {
            records: vec![BundledSkillRecord {
                name: "brief".to_owned(),
                description: "Write brief output.".to_owned(),
                body: "Keep the answer short.".to_owned(),
            }],
        });
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store_arc(store)
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .with_skill_loader(loader)
            .build()
            .await
            .expect("harness should build");

        let first_workspace = unique_workspace("sdk-skill-duplicate-first");
        let second_workspace = unique_workspace("sdk-skill-duplicate-second");
        std::fs::create_dir_all(&first_workspace).unwrap();
        std::fs::create_dir_all(&second_workspace).unwrap();

        harness
            .create_session(SessionOptions::new(&first_workspace))
            .await
            .expect("first session should be created");
        harness
            .create_session(SessionOptions::new(&second_workspace))
            .await
            .expect("second session should not fail on duplicate skill registration");
    });
}

#[test]
fn skill_registry_session_skill_list_uses_turn_snapshot_after_registry_reload() {
    block_on(async {
        let workspace = unique_workspace("sdk-skill-snapshot-list");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let tool_use_id = ToolUseId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(ScriptedProvider::new(vec![
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::ToolUseComplete {
                        id: tool_use_id,
                        name: "skills_list".to_owned(),
                        input: json!({}),
                    },
                },
                ModelStreamEvent::MessageStop,
            ]),
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("done".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]),
        ]));
        let loader = SkillLoader::default().with_source(SkillSourceConfig::BundledRecords {
            records: vec![BundledSkillRecord {
                name: "brief".to_owned(),
                description: "Write brief output.".to_owned(),
                body: "Keep the answer short.".to_owned(),
            }],
        });
        let harness = Harness::builder()
            .with_model_arc(model)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .with_skill_loader(loader)
            .build()
            .await
            .expect("harness should build");
        let session = harness
            .create_session(
                SessionOptions::new(&workspace)
                    .with_session_id(session_id)
                    .with_permission_mode(PermissionMode::BypassPermissions),
            )
            .await
            .expect("session should be created");

        session
            .run_turn("list skills")
            .await
            .expect("skills_list should use the session snapshot");

        let events: Vec<_> = store
            .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(
            events.iter().any(|event| {
                matches!(
                    event,
                    Event::ToolUseCompleted(completed)
                        if completed.tool_use_id == tool_use_id
                            && format!("{:?}", completed.result).contains("brief")
                )
            }),
            "skills_list should complete with brief skill; events: {events:#?}"
        );
    });
}

#[test]
fn skill_registry_session_render_uses_turn_snapshot_for_visibility() {
    block_on(async {
        let denied_in_current = AgentId::from_u128(2);
        let registry = SkillRegistry::builder()
            .with_skill(skill_markdown(
                r#"---
name: brief
description: Public snapshot skill.
---
snapshot body
"#,
            ))
            .build();
        let turn_snapshot = registry.snapshot();
        let replacement = SkillRegistry::builder()
            .with_skill(skill_markdown(&format!(
                r#"---
name: brief
description: Restricted current skill.
allowlist_agents: ["{}"]
---
current body
"#,
                AgentId::from_u128(1)
            )))
            .build()
            .snapshot();
        registry.commit_snapshot((*replacement).clone());

        let service = SkillRegistryService::new(
            registry,
            SkillRenderer::new(Arc::new(TestSkillConfigResolver)),
        )
        .with_snapshot(turn_snapshot);

        let rendered = service
            .render(&denied_in_current, "brief", json!({}))
            .await
            .expect("render visibility should use the turn snapshot");

        assert_eq!(rendered.content, "snapshot body\n");
    });
}

#[test]
fn skill_registry_session_hook_registration_is_idempotent_across_sessions() {
    block_on(async {
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let workspace = unique_workspace("sdk-skill-hook-idempotent");
        let skill_dir = workspace.join("skills");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("audit.md"),
            r#"---
name: audit
description: Audited skill.
hooks:
  - id: start
    events: [SessionStart]
    transport:
      type: builtin
      kind: AuditLog
---
unused body
"#,
        )
        .unwrap();

        let loader = SkillLoader::default().with_source(SkillSourceConfig::Directory {
            path: skill_dir,
            source_kind: harness_skill::DirectorySourceKind::Workspace,
        });
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .with_skill_loader(loader)
            .build()
            .await
            .expect("harness should build");

        let first_session_id = SessionId::new();
        let second_session_id = SessionId::new();
        harness
            .create_session(SessionOptions::new(&workspace).with_session_id(first_session_id))
            .await
            .expect("first session should register the skill hook");
        harness
            .create_session(SessionOptions::new(&workspace).with_session_id(second_session_id))
            .await
            .expect("second session should reuse the existing skill hook handler");

        assert_eq!(
            session_start_hook_count(&store, first_session_id).await,
            1,
            "first session should trigger one SessionStart hook"
        );
        assert_eq!(
            session_start_hook_count(&store, second_session_id).await,
            1,
            "second session should trigger one SessionStart hook"
        );
    });
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-{name}-{}-{}",
        std::process::id(),
        harness_contracts::SessionId::new()
    ))
}

async fn session_start_hook_count(store: &InMemoryEventStore, session_id: SessionId) -> usize {
    store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("events should be readable")
        .filter(|event| {
            let matches = matches!(
                event,
                Event::HookTriggered(triggered)
                    if triggered.handler_id == "skill:audit:start"
                        && triggered.hook_event_kind == HookEventKind::SessionStart
            );
            async move { matches }
        })
        .count()
        .await
}

fn skill_markdown(markdown: &str) -> harness_skill::Skill {
    parse_skill_markdown(
        markdown,
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("skill markdown should parse")
}

struct TestSkillConfigResolver;

#[async_trait]
impl SkillConfigResolver for TestSkillConfigResolver {
    async fn resolve(&self, key: &str) -> Result<Value, ConfigResolveError> {
        Err(ConfigResolveError::UnknownKey(key.to_owned()))
    }

    async fn resolve_secret(&self, key: &str) -> Result<SecretString, ConfigResolveError> {
        Err(ConfigResolveError::UnknownKey(key.to_owned()))
    }
}

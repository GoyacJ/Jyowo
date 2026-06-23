#![cfg(feature = "testing")]

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_contracts::{Decision, Event, ModelError, TenantId, ToolUseId};
use harness_journal::{EventStore, ReplayCursor};
use harness_model::{
    ContentDelta, InferContext, ModelDescriptor, ModelProvider, ModelRequest, ModelStream,
    ModelStreamEvent,
};
use harness_session::{ConfigDelta, ReloadMode};
use harness_skill::{
    parse_skill_markdown, BundledSkillRecord, SkillLoader, SkillPlatform, SkillRegistration,
    SkillSource, SkillSourceConfig,
};
use jyowo_harness_sdk::{prelude::*, testing::*};
use serde_json::json;

#[tokio::test]
async fn sdk_workspace_flow_binds_session_loads_bootstrap_and_merges_defaults() {
    let model = Arc::new(MockProvider::default());
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let harness = Harness::builder()
        .with_model_arc(model.clone())
        .with_store_arc(store)
        .with_sandbox(NoopSandbox::new())
        .with_default_session_options(
            SessionOptions::default().with_system_prompt_addendum("harness default"),
        )
        .build()
        .await
        .expect("harness should build");
    let root = unique_workspace("sdk-workspace-flow");
    std::fs::write(root.join("AGENTS.md"), "workspace bootstrap").unwrap();

    let workspace = harness
        .create_workspace(
            WorkspaceSpec::new(&root, "Production Workspace")
                .with_bootstrap_files(vec![BootstrapFileSpec::required("AGENTS.md")])
                .with_default_session_options(
                    SessionOptions::default()
                        .with_model_id("mock-model")
                        .with_model_extra(json!({ "source": "workspace" }))
                        .with_system_prompt_addendum("workspace default"),
                ),
        )
        .await
        .expect("workspace should be created");

    let session = harness
        .create_session(
            SessionOptions::default()
                .with_workspace(workspace.id)
                .with_system_prompt_addendum("session addendum"),
        )
        .await
        .expect("workspace-bound session should be created");
    session.run_turn("use workspace context").await.unwrap();

    let request = model
        .requests()
        .await
        .into_iter()
        .next()
        .expect("model should receive one request");
    assert_eq!(request.model_id, "mock-model");
    assert_eq!(request.extra["source"], json!("workspace"));
    assert!(request.extra["relay_logical_call_key"]
        .as_str()
        .is_some_and(|value| value.starts_with("engine_turn:")));
    let system = request.system.as_deref().unwrap_or_default();
    assert!(system.contains("workspace bootstrap"));
    assert!(system.contains("session addendum"));
    assert!(!system.contains("harness default"));
}

#[tokio::test]
async fn sdk_skill_flow_uses_turn_snapshot_then_next_turn_sees_reload() {
    let workspace = unique_workspace("sdk-skill-flow");
    let session_id = SessionId::new();
    let first_tool_use = ToolUseId::new();
    let second_tool_use = ToolUseId::new();
    let model = Arc::new(SkillListTwiceProvider::new(first_tool_use, second_tool_use));
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let loader = SkillLoader::default().with_source(SkillSourceConfig::BundledRecords {
        records: vec![BundledSkillRecord {
            name: "brief".to_owned(),
            description: "Write a short answer.".to_owned(),
            body: "Keep it short.".to_owned(),
        }],
    });

    let harness = Harness::builder()
        .with_model_arc(model)
        .with_store_arc(store.clone())
        .with_sandbox(NoopSandbox::new())
        .with_permission_broker(MockBroker::new(vec![
            Decision::AllowOnce,
            Decision::AllowOnce,
        ]))
        .with_skill_loader(loader)
        .build()
        .await
        .expect("harness should build");
    let session = harness
        .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
        .await
        .expect("session should be created");

    session.run_turn("list skills before reload").await.unwrap();
    let outcome = session
        .reload_with(
            ConfigDelta::for_tenant(TenantId::SINGLE).add_skill(skill_registration_from(
                r"---
name: expanded
description: Expanded skill.
---
Use expanded reasoning.
",
                SkillSource::Workspace("data/skills".into()),
            )),
        )
        .await
        .expect("skill reload should return outcome");
    assert_eq!(outcome.mode, ReloadMode::AppliedInPlace);
    session.run_turn("list skills after reload").await.unwrap();

    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("journal should be readable")
        .collect()
        .await;
    let first_result = tool_result_text(&events, first_tool_use);
    let second_result = tool_result_text(&events, second_tool_use);
    assert!(first_result.contains("brief"));
    assert!(!first_result.contains("expanded"));
    assert!(second_result.contains("brief"));
    assert!(second_result.contains("expanded"));
}

fn tool_result_text(events: &[Event], tool_use_id: ToolUseId) -> String {
    events
        .iter()
        .find_map(|event| match event {
            Event::ToolUseCompleted(completed) if completed.tool_use_id == tool_use_id => {
                Some(format!("{:?}", completed.result))
            }
            _ => None,
        })
        .expect("tool use should complete")
}

fn skill_registration_from(markdown: &str, source: SkillSource) -> SkillRegistration {
    SkillRegistration {
        skill: parse_skill_markdown(markdown, source, None, SkillPlatform::Macos)
            .expect("skill should parse"),
        force_allowlist: None,
    }
}

struct SkillListTwiceProvider {
    first_tool_use: ToolUseId,
    second_tool_use: ToolUseId,
    calls: AtomicUsize,
}

impl SkillListTwiceProvider {
    fn new(first_tool_use: ToolUseId, second_tool_use: ToolUseId) -> Self {
        Self {
            first_tool_use,
            second_tool_use,
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl ModelProvider for SkillListTwiceProvider {
    fn provider_id(&self) -> &str {
        "mock"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        MockProvider::default().supported_models()
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let events = match call {
            0 => vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::ToolUseComplete {
                        id: self.first_tool_use,
                        name: "skills_list".to_owned(),
                        input: json!({}),
                    },
                },
                ModelStreamEvent::MessageStop,
            ],
            2 => vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::ToolUseComplete {
                        id: self.second_tool_use,
                        name: "skills_list".to_owned(),
                        input: json!({}),
                    },
                },
                ModelStreamEvent::MessageStop,
            ],
            _ => vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("done".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ],
        };
        Ok(Box::pin(stream::iter(events)))
    }
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("{name}-{}", SessionId::new()));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
}

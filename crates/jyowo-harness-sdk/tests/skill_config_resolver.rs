#![cfg(feature = "testing")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use futures::{executor::block_on, stream, StreamExt};
use harness_contracts::{
    BudgetMetric, Decision, DeferPolicy, Event, ModelError, OverflowAction, ProviderRestriction,
    ResultBudget, SkillInjectionId, SkillInvocationReceipt, SkillRegistryCap, TenantId,
    ToolCapability, ToolDescriptor, ToolError, ToolGroup, ToolOrigin, ToolProperties, ToolResult,
    ToolUseId, TrustLevel,
};
use harness_journal::EventStore;
use harness_model::{
    ContentDelta, InferContext, ModelDescriptor, ModelProvider, ModelRequest, ModelStream,
    ModelStreamEvent,
};
use harness_permission::PermissionCheck;
use harness_skill::{ConfigResolveError, SkillConfigResolver, SkillLoader, SkillSourceConfig};
use harness_tool::{Tool, ToolContext, ToolEvent, ToolStream, ValidationError};
use jyowo_harness_sdk::skill_config::{SkillConfigSnapshot, SkillConfigSnapshotResolver};
use jyowo_harness_sdk::{prelude::*, testing::*};
use serde_json::json;

#[test]
fn skill_config_resolver_missing_required_secret_blocks_session_before_model_invocation() {
    block_on(async {
        let workspace = unique_workspace("sdk-skill-config-missing");
        let skill_dir = workspace.join("skills");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("configured.md"), configured_skill()).unwrap();
        let model = Arc::new(CountingProvider::default());
        let loader = SkillLoader::default().with_source(SkillSourceConfig::Directory {
            path: skill_dir,
            source_kind: harness_skill::DirectorySourceKind::Workspace,
        });

        let harness = Harness::builder()
            .with_model_arc(model.clone())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .with_skill_loader(loader)
            .build()
            .await
            .expect("harness should build");

        let error = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .expect_err("missing required config should fail before runtime starts");

        assert!(
            format!("{error}").contains("missing required skill config `github.org`"),
            "unexpected error: {error}"
        );
        assert_eq!(model.calls(), 0, "model must not be invoked");
    });
}

#[test]
fn skill_config_resolver_secret_values_do_not_enter_tool_receipts_or_events() {
    block_on(async {
        let workspace = unique_workspace("sdk-skill-config-secret-redaction");
        let skill_dir = workspace.join("skills");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("configured.md"), configured_skill()).unwrap();
        let session_id = SessionId::new();
        let tool_use_id = ToolUseId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(ScriptedProvider::new(vec![
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::ToolUseComplete {
                        id: tool_use_id,
                        name: "invoke_configured_skill".to_owned(),
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
        let loader = SkillLoader::default().with_source(SkillSourceConfig::Directory {
            path: skill_dir,
            source_kind: harness_skill::DirectorySourceKind::Workspace,
        });
        let config = SkillConfigSnapshot::new()
            .with_value("github.org", json!("jyowo"))
            .with_secret("github.token", "super-secret-token");

        let harness = Harness::builder()
            .with_model_arc(model)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .with_tool_registry(
                ToolRegistry::builder()
                    .with_tool(Box::new(InvokeConfiguredSkillTool::new()))
                    .build()
                    .expect("tool registry should build"),
            )
            .with_skill_loader(loader)
            .with_skill_config_snapshot(config)
            .build()
            .await
            .expect("harness should build");
        let session = harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should start with configured skill config");

        session
            .run_turn("invoke configured skill")
            .await
            .expect("skills_invoke should render configured skill");

        let events: Vec<_> = store
            .read(
                TenantId::SINGLE,
                session_id,
                harness_journal::ReplayCursor::FromStart,
            )
            .await
            .expect("events should be readable")
            .collect()
            .await;
        assert!(events.iter().any(|event| {
            matches!(
                event,
                Event::ToolUseCompleted(completed)
                    if completed.tool_use_id == tool_use_id
                        && format!("{:?}", completed.result).contains("github.token")
                        && !format!("{:?}", completed.result).contains("super-secret-token")
            )
        }));
        assert!(
            !format!("{events:?}").contains("super-secret-token"),
            "secret must not be persisted in events"
        );
    });
}

#[test]
fn skill_config_resolver_reads_only_approved_snapshot_refs() {
    block_on(async {
        let snapshot = SkillConfigSnapshot::new()
            .with_value("approved", json!("ok"))
            .with_secret("extra.secret", "leak");
        let resolver =
            SkillConfigSnapshotResolver::new(snapshot, ["approved".to_owned()].into_iter());

        assert_eq!(resolver.resolve("approved").await.unwrap(), json!("ok"));
        let error = resolver
            .resolve_secret("extra.secret")
            .await
            .expect_err("unapproved snapshot key should be blocked");

        assert!(matches!(error, ConfigResolveError::UnknownKey(key) if key == "extra.secret"));
    });
}

fn configured_skill() -> &'static str {
    r#"---
name: configured
description: Configured skill.
config:
  - key: github.org
    type: string
    required: true
  - key: github.token
    type: string
    secret: true
    required: true
---
Org ${config.github.org}
Token ${config.github.token:secret}
"#
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-{name}-{}-{}",
        std::process::id(),
        harness_contracts::SessionId::new()
    ))
}

#[derive(Default)]
struct CountingProvider {
    calls: AtomicUsize,
}

impl CountingProvider {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ModelProvider for CountingProvider {
    fn provider_id(&self) -> &str {
        "counting"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        TestModelProvider::default().supported_models()
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Box::pin(stream::iter([ModelStreamEvent::MessageStop])))
    }
}

struct InvokeConfiguredSkillTool {
    descriptor: ToolDescriptor,
}

impl InvokeConfiguredSkillTool {
    fn new() -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: "invoke_configured_skill".to_owned(),
                display_name: "Invoke configured skill".to_owned(),
                description: "Test tool that renders the configured skill.".to_owned(),
                category: "testing".to_owned(),
                group: ToolGroup::Meta,
                version: "0.1.0".to_owned(),
                input_schema: json!({"type": "object"}),
                output_schema: None,
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: true,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                trust_level: TrustLevel::AdminTrusted,
                required_capabilities: vec![ToolCapability::SkillRegistry],
                budget: ResultBudget {
                    metric: BudgetMetric::Chars,
                    limit: 8_192,
                    on_overflow: OverflowAction::Truncate,
                    preview_head_chars: 1_024,
                    preview_tail_chars: 1_024,
                },
                provider_restriction: ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
                search_hint: None,
                service_binding: None,
            },
        }
    }
}

#[async_trait]
impl Tool for InvokeConfiguredSkillTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(
        &self,
        _input: &serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn check_permission(
        &self,
        _input: &serde_json::Value,
        _ctx: &ToolContext,
    ) -> PermissionCheck {
        PermissionCheck::Allowed
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let registry = ctx.capability::<dyn SkillRegistryCap>(ToolCapability::SkillRegistry)?;
        let rendered = registry
            .render(&ctx.agent_id, "configured".to_owned(), json!({}))
            .await?;
        let receipt = SkillInvocationReceipt {
            skill_name: rendered.skill_name,
            injection_id: SkillInjectionId(format!("skill:configured:{}", ctx.tool_use_id)),
            bytes_injected: rendered.content.len() as u64,
            consumed_config_keys: rendered.consumed_config_keys,
        };
        let result = serde_json::to_value(receipt)
            .map_err(|error| ToolError::Internal(error.to_string()))?;
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(result),
        )])))
    }
}

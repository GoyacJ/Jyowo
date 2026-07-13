#![cfg(feature = "testing")]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::{executor::block_on, stream, StreamExt};
use harness_contracts::{
    BudgetMetric, Decision, DeferPolicy, Event, NetworkAccess, OverflowAction, ProviderRestriction,
    ResultBudget, SkillInjectionId, SkillInvocationReceipt, SkillRegistryCap, TenantId,
    ToolActionPlan, ToolCapability, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup,
    ToolOrigin, ToolProperties, ToolResult, ToolUseId, TrustLevel, WorkspaceAccess,
};
use harness_journal::EventStore;
use harness_model::{ContentDelta, ModelStreamEvent};
use harness_permission::PermissionCheck;
use harness_skill::{ConfigResolveError, SkillConfigResolver, SkillLoader, SkillSourceConfig};
use harness_tool::{
    action_plan_from_permission_check, AuthorizedToolInput, Tool, ToolContext, ToolEvent,
    ToolStream, ValidationError,
};
use jyowo_harness_sdk::skill_config::{
    SecretString, SkillConfigSnapshot, SkillConfigSnapshotResolver, SkillConfigStoreError,
    SkillSecretStore,
};
use jyowo_harness_sdk::{prelude::*, testing::*, KeyringSkillSecretStore};
use serde_json::json;

#[test]
fn missing_required_config_disables_only_its_skill_without_blocking_session() {
    block_on(async {
        let workspace = unique_workspace("sdk-skill-config-missing");
        let skill_dir = workspace.join("skills");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("configured.md"), configured_skill()).unwrap();
        std::fs::write(skill_dir.join("ready.md"), ready_skill()).unwrap();
        let model = Arc::new(TestModelProvider::default());
        let loader = SkillLoader::default().with_source(SkillSourceConfig::Directory {
            path: skill_dir,
            source_kind: harness_skill::DirectorySourceKind::Workspace,
        });

        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_arc(model.clone())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .with_skill_loader(loader)
            .build()
            .await
            .expect("harness should build");

        let _session = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .expect("missing config must not block session creation");

        let configured = harness
            .view_runtime_skill("configured", false)
            .expect("configured skill");
        let ready = harness
            .view_runtime_skill("ready", false)
            .expect("ready skill");
        assert_eq!(configured.summary.id, "workspace:configured");
        assert!(configured
            .config
            .iter()
            .any(|declaration| declaration.key == "github.token" && declaration.secret));
        assert!(matches!(
            configured.summary.status,
            harness_contracts::SkillStatus::PrerequisiteMissing { ref config_keys, .. }
                if config_keys == &["github.org".to_owned(), "github.token".to_owned()]
        ));
        assert_eq!(ready.summary.status, harness_contracts::SkillStatus::Ready);
    });
}

#[test]
fn required_secret_metadata_without_a_secret_store_entry_remains_missing() {
    block_on(async {
        let workspace = unique_workspace("sdk-skill-config-missing-secret-entry");
        let skill_dir = workspace.join("skills");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("configured.md"), configured_skill()).unwrap();
        let loader = SkillLoader::default().with_source(SkillSourceConfig::Directory {
            path: skill_dir,
            source_kind: harness_skill::DirectorySourceKind::Workspace,
        });
        let config = SkillConfigSnapshot::new()
            .with_skill_value("workspace:configured", "github.org", json!("jyowo"))
            .with_skill_secret_presence("workspace:configured", "github.token");
        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .with_skill_loader(loader)
            .with_skill_config_snapshot(config)
            .build()
            .await
            .unwrap();
        let _session = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .unwrap();

        let configured = harness.view_runtime_skill("configured", false).unwrap();
        assert!(matches!(
            configured.summary.status,
            harness_contracts::SkillStatus::PrerequisiteMissing { ref config_keys, .. }
                if config_keys == &["github.token".to_owned()]
        ));
    });
}

#[test]
fn skill_config_resolver_rejects_secret_interpolation_without_leaking_to_events() {
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
        let secret_store = Arc::new(MemorySecretStore::default());
        secret_store
            .set(
                "workspace:configured",
                "github.token",
                SecretString::from("super-secret-token".to_owned()),
            )
            .unwrap();
        let config = SkillConfigSnapshot::new()
            .with_skill_value("workspace:configured", "github.org", json!("jyowo"))
            .with_skill_secret_presence("workspace:configured", "github.token")
            .with_secret_store(secret_store);

        let harness = Harness::builder()
            .with_workspace_root(&workspace)
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
            .create_session(
                SessionOptions::new(&workspace)
                    .with_session_id(session_id)
                    .with_permission_mode(PermissionMode::BypassPermissions),
            )
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
        assert!(
            events.iter().any(|event| {
                matches!(
                    event,
                    Event::ToolUseFailed(failed)
                        if failed.tool_use_id == tool_use_id
                            && failed.error.message.contains("cannot be interpolated")
                            && !failed.error.message.contains("super-secret-token")
                )
            }),
            "configured skill tool should reject secret interpolation; events: {events:#?}"
        );
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
            .with_skill_value("workspace:configured", "approved", json!("ok"))
            .with_skill_secret_presence("workspace:configured", "extra.secret");
        let resolver = SkillConfigSnapshotResolver::for_skill(
            "workspace:configured",
            snapshot,
            [harness_skill::SkillConfigDecl {
                key: "approved".to_owned(),
                value_type: harness_skill::SkillParamType::String,
                secret: false,
                required: true,
                default: None,
                description: None,
            }],
        );

        assert_eq!(resolver.resolve("approved").await.unwrap(), json!("ok"));
        let error = resolver
            .resolve_secret("extra.secret")
            .await
            .expect_err("unapproved snapshot key should be blocked");

        assert!(matches!(error, ConfigResolveError::UnknownKey(key) if key == "extra.secret"));
    });
}

#[test]
fn secret_template_interpolation_is_forbidden_without_loading_secret_plaintext() {
    block_on(async {
        let snapshot = SkillConfigSnapshot::new()
            .with_skill_secret_presence("workspace:configured", "github.token");
        let resolver = SkillConfigSnapshotResolver::for_skill(
            "workspace:configured",
            snapshot,
            [harness_skill::SkillConfigDecl {
                key: "github.token".to_owned(),
                value_type: harness_skill::SkillParamType::String,
                secret: true,
                required: true,
                default: None,
                description: None,
            }],
        );

        let error = resolver
            .resolve_secret("github.token")
            .await
            .expect_err("ordinary rendering must not resolve secrets");

        assert!(matches!(
            error,
            ConfigResolveError::SecretInterpolationForbidden { ref key, .. }
                if key == "github.token"
        ));
        assert!(!format!("{error:?} {error}").contains("super-secret-token"));
    });
}

#[test]
fn resolver_rejects_config_values_with_the_wrong_declared_type() {
    block_on(async {
        let snapshot = SkillConfigSnapshot::new().with_skill_value(
            "workspace:configured",
            "github.org",
            json!(42),
        );
        let resolver = SkillConfigSnapshotResolver::for_skill(
            "workspace:configured",
            snapshot,
            [harness_skill::SkillConfigDecl {
                key: "github.org".to_owned(),
                value_type: harness_skill::SkillParamType::String,
                secret: false,
                required: false,
                default: None,
                description: None,
            }],
        );

        let error = resolver
            .resolve("github.org")
            .await
            .expect_err("wrong config types must not reach the renderer");
        assert!(matches!(
            error,
            ConfigResolveError::InvalidType {
                ref skill_id,
                ref key,
                expected: "string",
            } if skill_id == "workspace:configured" && key == "github.org"
        ));
    });
}

#[test]
fn keychain_accounts_are_unambiguous_for_slashes_in_both_components() {
    assert_eq!(
        KeyringSkillSecretStore::account_name("user:example", "apiToken"),
        "user:example/apiToken"
    );
    assert_ne!(
        KeyringSkillSecretStore::account_name("user:a/b", "c"),
        KeyringSkillSecretStore::account_name("user:a", "b/c")
    );
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

fn ready_skill() -> &'static str {
    r#"---
name: ready
description: Ready skill.
---
Ready
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
struct MemorySecretStore {
    secrets: Mutex<BTreeMap<(String, String), SecretString>>,
}

impl SkillSecretStore for MemorySecretStore {
    fn get(
        &self,
        skill_id: &str,
        key: &str,
    ) -> Result<Option<SecretString>, SkillConfigStoreError> {
        Ok(self
            .secrets
            .lock()
            .unwrap()
            .get(&(skill_id.to_owned(), key.to_owned()))
            .cloned())
    }

    fn set(
        &self,
        skill_id: &str,
        key: &str,
        value: SecretString,
    ) -> Result<(), SkillConfigStoreError> {
        self.secrets
            .lock()
            .unwrap()
            .insert((skill_id.to_owned(), key.to_owned()), value);
        Ok(())
    }

    fn delete(&self, skill_id: &str, key: &str) -> Result<(), SkillConfigStoreError> {
        self.secrets
            .lock()
            .unwrap()
            .remove(&(skill_id.to_owned(), key.to_owned()));
        Ok(())
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
                metadata: Default::default(),
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

    async fn plan(
        &self,
        input: &serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            PermissionCheck::Allowed,
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: AuthorizedToolInput,
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

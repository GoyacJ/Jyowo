#![cfg(feature = "testing")]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::{executor::block_on, stream, StreamExt};
use harness_contracts::{
    AgentId, BudgetMetric, Decision, DeferPolicy, Event, NetworkAccess, OverflowAction,
    ProviderRestriction, ResultBudget, SkillId, SkillInjectionId, SkillInvocationReceipt,
    SkillRegistryCap, TenantId, ToolActionPlan, ToolCapability, ToolDescriptor, ToolError,
    ToolExecutionChannel, ToolGroup, ToolOrigin, ToolProperties, ToolResult, ToolUseId, TrustLevel,
    WorkspaceAccess,
};
use harness_journal::EventStore;
use harness_model::{ContentDelta, ModelStreamEvent};
use harness_permission::PermissionCheck;
use harness_skill::{
    parse_skill_markdown, ConfigResolveError, SkillConfigResolver, SkillLoader, SkillPlatform,
    SkillRegistry, SkillRegistryService, SkillSource, SkillSourceConfig,
};
use harness_tool::{
    action_plan_from_permission_check, AuthorizedToolInput, Tool, ToolContext, ToolEvent,
    ToolStream, ValidationError,
};
use jyowo_harness_sdk::skill_config::{
    apply_skill_config_statuses, SecretString, SkillConfigSnapshot, SkillConfigSnapshotResolver,
    SkillConfigStoreError, SkillSecretStore,
};
use jyowo_harness_sdk::{prelude::*, testing::*, KeyringSkillSecretStore};
use secrecy::ExposeSecret;
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
            .unwrap()
            .expect("configured skill");
        let ready = harness
            .view_runtime_skill("ready", false)
            .unwrap()
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
            .with_skill_secret_presence("workspace:configured", "github.token")
            .with_secret_store(Arc::new(MemorySecretStore::default()));
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

        let configured = harness
            .view_runtime_skill("configured", false)
            .unwrap()
            .unwrap();
        assert!(matches!(
            configured.summary.status,
            harness_contracts::SkillStatus::PrerequisiteMissing { ref config_keys, .. }
                if config_keys == &["github.token".to_owned()]
        ));
    });
}

#[test]
fn required_secret_store_entry_without_metadata_is_ready() {
    block_on(async {
        let workspace = unique_workspace("sdk-skill-config-secret-store-truth");
        let skill_dir = workspace.join("skills");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("configured.md"), configured_skill()).unwrap();
        let loader = SkillLoader::default().with_source(SkillSourceConfig::Directory {
            path: skill_dir,
            source_kind: harness_skill::DirectorySourceKind::Workspace,
        });
        let secret_store = Arc::new(MemorySecretStore::default());
        secret_store
            .set(
                "workspace:configured",
                "github.token",
                SecretString::from("store-only-secret".to_owned()),
            )
            .unwrap();
        let config = SkillConfigSnapshot::new()
            .with_skill_value("workspace:configured", "github.org", json!("jyowo"))
            .with_secret_store(secret_store);
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

        assert_eq!(
            harness
                .view_runtime_skill("configured", false)
                .unwrap()
                .unwrap()
                .summary
                .status,
            harness_contracts::SkillStatus::Ready
        );
    });
}

#[test]
fn script_secret_lookup_uses_only_the_injected_store_as_runtime_truth() {
    let populated_store = Arc::new(MemorySecretStore::default());
    populated_store
        .set(
            "workspace:configured",
            "github.token",
            SecretString::from("store-only-secret".to_owned()),
        )
        .unwrap();
    let without_metadata = SkillConfigSnapshot::new().with_secret_store(populated_store.clone());

    assert!(without_metadata
        .secret_is_available_for("workspace:configured", "github.token")
        .unwrap());
    assert_eq!(
        without_metadata
            .secret_for_script("workspace:configured", "github.token")
            .unwrap()
            .unwrap()
            .expose_secret(),
        "store-only-secret"
    );

    let empty_store = Arc::new(MemorySecretStore::default());
    let stale_metadata = SkillConfigSnapshot::new()
        .with_skill_secret_presence("workspace:configured", "github.token")
        .with_secret_store(empty_store);
    assert!(!stale_metadata
        .secret_is_available_for("workspace:configured", "github.token")
        .unwrap());
    assert!(stale_metadata
        .secret_for_script("workspace:configured", "github.token")
        .unwrap()
        .is_none());
}

#[test]
fn secret_store_failure_is_not_reported_as_a_missing_secret() {
    let snapshot = SkillConfigSnapshot::new().with_secret_store(Arc::new(UnavailableSecretStore));

    let presence = snapshot
        .secret_is_available_for("workspace:configured", "github.token")
        .expect_err("store failure must remain distinguishable from a missing entry");
    let script = snapshot
        .secret_for_script("workspace:configured", "github.token")
        .expect_err("script lookup must propagate the same store failure");

    assert_eq!(presence, SkillConfigStoreError::SecretStoreUnavailable);
    assert_eq!(script, SkillConfigStoreError::SecretStoreUnavailable);
    let diagnostic = format!("{presence:?} {presence} {script:?} {script}");
    assert!(!diagnostic.contains("must-not-leak-secret"));
}

#[test]
fn sdk_status_and_session_assembly_propagate_secret_store_failure() {
    block_on(async {
        let workspace = unique_workspace("sdk-skill-config-store-failure");
        let skill_dir = workspace.join("skills");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("configured.md"), configured_skill()).unwrap();
        let loader = SkillLoader::default().with_source(SkillSourceConfig::Directory {
            path: skill_dir,
            source_kind: harness_skill::DirectorySourceKind::Workspace,
        });
        let loaded = loader.clone().load_all().await.unwrap().loaded;
        let config = SkillConfigSnapshot::new()
            .with_skill_value("workspace:configured", "github.org", json!("jyowo"))
            .with_secret_store(Arc::new(UnavailableSecretStore));
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
        harness.skill_registry().register_batch(loaded).unwrap();

        let status_error = harness
            .list_runtime_skills()
            .expect_err("status assembly must return the store error");
        assert_eq!(status_error, SkillConfigStoreError::SecretStoreUnavailable);
        let session_error = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .expect_err("session assembly must fail instead of publishing missing status");
        let diagnostic =
            format!("{status_error:?} {status_error} {session_error:?} {session_error}");
        assert!(!diagnostic.contains("must-not-leak-secret"));
    });
}

#[test]
fn required_public_config_with_the_wrong_type_is_missing_before_rendering() {
    block_on(async {
        let skill = parse_skill_markdown(
            "---\nname: typed\ndescription: Typed config\nconfig:\n  - key: region\n    type: string\n    required: true\n---\nBody without interpolation.\n",
            SkillSource::Workspace("test/skills".into()),
            None,
            SkillPlatform::Macos,
        )
        .unwrap();
        let registry = SkillRegistry::builder().with_skill(skill).build();
        let mut registry_snapshot = (*registry.snapshot()).clone();
        let document = serde_json::from_value(json!({
            "version": 1,
            "skills": {
                "workspace:typed": {
                    "values": { "region": 42 },
                    "secrets": {}
                }
            }
        }))
        .unwrap();
        let config_snapshot =
            SkillConfigSnapshot::from_document(document, Arc::new(MemorySecretStore::default()))
                .unwrap();
        apply_skill_config_statuses(&mut registry_snapshot, &config_snapshot).unwrap();

        assert!(matches!(
            registry_snapshot.status.get(&SkillId("workspace:typed".to_owned())),
            Some(harness_contracts::SkillStatus::PrerequisiteMissing { config_keys, .. })
                if config_keys == &["region".to_owned()]
        ));

        let renderer_snapshot = config_snapshot.clone();
        let renderer = harness_skill::SkillRenderer::new_with_config_resolver_factory(Arc::new(
            move |skill: &harness_skill::Skill| -> Arc<dyn SkillConfigResolver> {
                Arc::new(SkillConfigSnapshotResolver::for_skill(
                    skill.id.0.clone(),
                    renderer_snapshot.clone(),
                    skill.frontmatter.config.clone(),
                ))
            },
        ));
        let service = SkillRegistryService::new(registry, renderer)
            .with_snapshot(Arc::new(registry_snapshot));
        let error = service
            .render(&AgentId::from_u128(1), "typed", json!({}))
            .await
            .expect_err("invalid required config must fail even when the body does not use it");
        assert!(matches!(
            error,
            harness_skill::RenderError::MissingConfig { ref skill_id, ref config_keys }
                if skill_id == "workspace:typed" && config_keys == &["region".to_owned()]
        ));
    });
}

#[test]
fn status_assembly_does_not_read_optional_secrets() {
    let skill = parse_skill_markdown(
        "---\nname: optional-secret\ndescription: Optional secret\nconfig:\n  - key: token\n    type: string\n    secret: true\n---\nBody.\n",
        SkillSource::Workspace("test/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .unwrap();
    let registry = SkillRegistry::builder().with_skill(skill).build();
    let mut registry_snapshot = (*registry.snapshot()).clone();
    let config_snapshot =
        SkillConfigSnapshot::new().with_secret_store(Arc::new(UnavailableSecretStore));

    apply_skill_config_statuses(&mut registry_snapshot, &config_snapshot)
        .expect("optional secrets are not status prerequisites");

    assert_eq!(
        registry_snapshot
            .status
            .get(&SkillId("workspace:optional-secret".to_owned())),
        Some(&harness_contracts::SkillStatus::Ready)
    );
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
fn script_only_secret_resolution_does_not_open_template_interpolation() {
    block_on(async {
        let store = Arc::new(MemorySecretStore::default());
        store
            .set(
                "workspace:configured",
                "github.token",
                SecretString::from("script-only-secret".to_owned()),
            )
            .unwrap();
        let snapshot = SkillConfigSnapshot::new().with_secret_store(store);
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

        let template_error = resolver
            .resolve_secret_for(&SkillId("workspace:configured".to_owned()), "github.token")
            .await
            .expect_err("ordinary rendering must remain unable to read secrets");
        assert!(matches!(
            template_error,
            ConfigResolveError::SecretInterpolationForbidden { .. }
        ));

        let script_secret = resolver
            .resolve_secret_for_script(&SkillId("workspace:configured".to_owned()), "github.token")
            .await
            .expect("declared script resolution may read the secret");
        assert_eq!(script_secret.expose_secret(), "script-only-secret");
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
fn per_skill_renderer_factory_never_resolves_another_skills_same_named_key() {
    block_on(async {
        let skill_a = parse_skill_markdown(
            "---\nname: skill-a\ndescription: Skill A\nconfig:\n  - key: region\n    type: string\n---\nRegion ${config.region}.\n",
            SkillSource::Workspace("test/skills".into()),
            None,
            SkillPlatform::Macos,
        )
        .unwrap();
        let skill_b = parse_skill_markdown(
            "---\nname: skill-b\ndescription: Skill B\nconfig:\n  - key: region\n    type: string\n---\nRegion ${config.region}.\n",
            SkillSource::Workspace("test/skills".into()),
            None,
            SkillPlatform::Macos,
        )
        .unwrap();
        let snapshot = SkillConfigSnapshot::new()
            .with_skill_value("workspace:skill-a", "region", json!("a-only"))
            .with_skill_value("workspace:skill-b", "region", json!("b-only"));
        let factory_snapshot = snapshot.clone();
        let renderer = harness_skill::SkillRenderer::new_with_config_resolver_factory(Arc::new(
            move |skill: &harness_skill::Skill| -> Arc<dyn SkillConfigResolver> {
                Arc::new(SkillConfigSnapshotResolver::for_skill(
                    skill.id.0.clone(),
                    factory_snapshot.clone(),
                    skill.frontmatter.config.clone(),
                ))
            },
        ));

        assert_eq!(
            renderer.render(&skill_a, json!({})).await.unwrap().content,
            "Region a-only.\n"
        );
        assert_eq!(
            renderer.render(&skill_b, json!({})).await.unwrap().content,
            "Region b-only.\n"
        );

        let resolver_a = SkillConfigSnapshotResolver::for_skill(
            skill_a.id.0.clone(),
            snapshot,
            skill_a.frontmatter.config.clone(),
        );
        let error = resolver_a
            .resolve_for(&SkillId(skill_b.id.0.clone()), "region")
            .await
            .expect_err("an A-bound resolver must reject B even when the key name matches");
        assert!(matches!(
            error,
            ConfigResolveError::SkillIdentityMismatch {
                ref expected_skill_id,
                ref actual_skill_id,
            } if expected_skill_id == "workspace:skill-a"
                && actual_skill_id == "workspace:skill-b"
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

struct UnavailableSecretStore;

impl SkillSecretStore for UnavailableSecretStore {
    fn get(
        &self,
        _skill_id: &str,
        _key: &str,
    ) -> Result<Option<SecretString>, SkillConfigStoreError> {
        Err(SkillConfigStoreError::SecretStoreUnavailable)
    }

    fn set(
        &self,
        _skill_id: &str,
        _key: &str,
        _value: SecretString,
    ) -> Result<(), SkillConfigStoreError> {
        Err(SkillConfigStoreError::SecretStoreUnavailable)
    }

    fn delete(&self, _skill_id: &str, _key: &str) -> Result<(), SkillConfigStoreError> {
        Err(SkillConfigStoreError::SecretStoreUnavailable)
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

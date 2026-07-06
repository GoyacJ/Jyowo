use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use harness_contracts::{
    DeferPolicy, Event, HookFailureMode, ManifestOriginRef, McpServerId, McpServerSource,
    MemoryError, MemoryId, NetworkAccess, PluginId, PluginRecentEvent, ProviderRestriction,
    RejectionReason, SteeringBody, SteeringId, SteeringKind, SteeringPriority, SteeringRequest,
    SteeringSource, ToolActionPlan, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup,
    ToolOrigin, ToolProperties, TrustLevel, WorkspaceAccess,
};
use harness_hook::{
    HookContext, HookEvent, HookHandler, HookOrigin, HookOutcome, HookRegistrationKind,
    HookRegistry,
};
use harness_mcp::{
    McpConnection, McpError, McpRegistry, McpServerSpec, McpToolDescriptor, McpToolResult,
    TransportChoice,
};
use harness_memory::{
    MemoryLifecycle, MemoryListScope, MemoryQuery, MemoryRecord, MemoryStore, MemorySummary,
};
use harness_plugin::{
    CapabilitySlot, CoordinatorStrategy, CoordinatorStrategyManifestEntry,
    CustomToolsetManifestEntry, DiscoverySource, ManifestLoaderError, ManifestOrigin,
    ManifestRecord, ManifestSignature, ManifestSigner, Plugin, PluginActivationContext,
    PluginActivationResult, PluginAdmissionPolicy, PluginCapabilities, PluginCapabilityRegistries,
    PluginConfig, PluginError, PluginLifecycleState, PluginManifest, PluginManifestLoader,
    PluginMetricsSink, PluginName, PluginRegistry, PluginRuntimeLoader, RegistrationError,
    RuntimeLoaderError, SignatureAlgorithm, SteeringRegistration, StrictPluginOnlyPolicy,
};
use harness_skill::{Skill, SkillFrontmatter, SkillPrerequisites, SkillRegistry, SkillSource};
use harness_tool::{
    action_plan_from_permission_check, default_result_budget, AuthorizedToolInput, PermissionCheck,
    SchemaResolverContext, Tool, ToolContext, ToolRegistry, ValidationError,
};
use parking_lot::Mutex;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{json, Value};

#[tokio::test]
async fn discover_keeps_runtime_loader_idle() {
    let runtime = Arc::new(CountingRuntimeLoader::new(Arc::new(NoopPlugin::new(record(
        "manifest-only",
        PluginCapabilities::default(),
    ))) as Arc<dyn Plugin>));
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            allow_project_plugins: true,
            ..PluginConfig::default()
        })
        .with_source(DiscoverySource::Project("/workspace".into()))
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![record(
            "manifest-only",
            PluginCapabilities::default(),
        )])))
        .with_runtime_loader(runtime.clone())
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert_eq!(discovered.len(), 1);
    assert_eq!(runtime.load_count(), 0);
    assert_eq!(
        registry.state(&plugin_id("manifest-only")).unwrap(),
        harness_plugin::PluginLifecycleState::Validated
    );
}

#[tokio::test]
async fn activate_injects_only_declared_capability_handles() {
    let manifest = record(
        "tool-only",
        PluginCapabilities {
            tools: vec![harness_plugin::ToolManifestEntry {
                name: "declared-tool".to_owned(),
                destructive: false,
                input_schema: serde_json::json!({ "type": "object" }),
            }],
            ..PluginCapabilities::default()
        },
    );
    let plugin = Arc::new(CapturingPlugin::new(manifest.clone()));
    let runtime_plugin: Arc<dyn Plugin> = plugin.clone();
    let runtime = Arc::new(CountingRuntimeLoader::new(runtime_plugin));
    let registry = registry_for(manifest, runtime.clone());

    registry.discover().await.unwrap();
    registry.activate(&plugin_id("tool-only")).await.unwrap();

    assert_eq!(runtime.load_count(), 1);
    let ctx = plugin.captured_context().unwrap();
    assert!(ctx.tools.is_some());
    assert!(ctx.hooks.is_none());
    assert!(ctx.mcp.is_none());
    assert!(ctx.skills.is_none());
    assert!(ctx.memory.is_none());
    assert!(ctx.coordinator.is_none());
    assert_eq!(
        registry.state(&plugin_id("tool-only")).unwrap(),
        harness_plugin::PluginLifecycleState::Activated
    );
}

#[tokio::test]
async fn config_disabled_skips_manifest_and_runtime_loaders() {
    let manifest = record("disabled-plugin", PluginCapabilities::default());
    let plugin: Arc<dyn Plugin> = Arc::new(NoopPlugin::new(manifest.clone()));
    let manifest_loader = Arc::new(CountingManifestLoader::new(vec![manifest]));
    let runtime = Arc::new(CountingRuntimeLoader::new(plugin));
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            enabled: false,
            ..PluginConfig::default()
        })
        .with_manifest_loader(manifest_loader.clone())
        .with_runtime_loader(runtime.clone())
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert!(discovered.is_empty());
    assert_eq!(manifest_loader.enumerate_count(), 0);
    assert_eq!(runtime.load_count(), 0);
}

#[tokio::test]
async fn disabled_plugin_is_discovered_but_not_activated() {
    let manifest = record("disabled-entry", PluginCapabilities::default());
    let plugin: Arc<dyn Plugin> = Arc::new(NoopPlugin::new(manifest.clone()));
    let runtime = Arc::new(CountingRuntimeLoader::new(plugin));
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            disabled_plugins: BTreeSet::from([PluginName::new("disabled-entry").unwrap()]),
            ..PluginConfig::default()
        })
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest.clone()])))
        .with_runtime_loader(runtime.clone())
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();
    let error = registry
        .activate(&plugin_id("disabled-entry"))
        .await
        .unwrap_err();
    let products = registry.product_snapshot();

    assert_eq!(discovered.len(), 1);
    assert_eq!(
        registry.state(&plugin_id("disabled-entry")).unwrap(),
        harness_plugin::PluginLifecycleState::Deactivated
    );
    assert!(matches!(error, PluginError::AdmissionDenied { .. }));
    assert_eq!(runtime.load_count(), 0);
    assert_eq!(products.len(), 1);
    assert!(!products[0].enabled);
    assert!(matches!(
        products[0].state,
        harness_contracts::PluginProductState::Disabled { .. }
    ));
}

#[tokio::test]
async fn project_sources_are_skipped_without_explicit_allow_gate() {
    let manifest_loader = Arc::new(CountingManifestLoader::new(vec![record(
        "project-plugin",
        PluginCapabilities::default(),
    )]));
    let registry = PluginRegistry::builder()
        .with_source(DiscoverySource::Project("/workspace".into()))
        .with_manifest_loader(manifest_loader.clone())
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert!(discovered.is_empty());
    assert_eq!(manifest_loader.enumerate_count(), 0);
}

#[tokio::test]
async fn project_sources_are_discovered_when_allow_gate_is_enabled() {
    let manifest_loader = Arc::new(CountingManifestLoader::new(vec![record(
        "project-plugin",
        PluginCapabilities::default(),
    )]));
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            allow_project_plugins: true,
            ..PluginConfig::default()
        })
        .with_source(DiscoverySource::Project("/workspace".into()))
        .with_manifest_loader(manifest_loader.clone())
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert_eq!(discovered.len(), 1);
    assert_eq!(manifest_loader.enumerate_count(), 1);
}

#[tokio::test]
async fn product_snapshot_exposes_declared_and_registered_capabilities() {
    let manifest = record(
        "product-plugin",
        PluginCapabilities {
            tools: vec![harness_plugin::ToolManifestEntry {
                name: "registered-tool".to_owned(),
                destructive: false,
                input_schema: serde_json::json!({ "type": "object" }),
            }],
            hooks: vec![harness_plugin::HookManifestEntry {
                name: "registered-hook".to_owned(),
                events: Vec::new(),
            }],
            mcp_servers: vec![harness_plugin::McpManifestEntry {
                name: "registered-mcp".to_owned(),
            }],
            skills: vec![harness_plugin::SkillManifestEntry {
                name: "registered-skill".to_owned(),
            }],
            memory_provider: Some(harness_plugin::MemoryProviderManifestEntry {
                name: "memory".to_owned(),
            }),
            ..PluginCapabilities::default()
        },
    );
    let plugin: Arc<dyn Plugin> = Arc::new(RegisteringPlugin::new(manifest.clone()));
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            allow_project_plugins: true,
            ..PluginConfig::default()
        })
        .with_source(DiscoverySource::Project("/workspace".into()))
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest.clone()])))
        .with_runtime_loader(Arc::new(CountingRuntimeLoader::new(plugin)))
        .with_capability_registries(
            PluginCapabilityRegistries::default()
                .with_tool_registry(ToolRegistry::builder().build().unwrap())
                .with_hook_registry(HookRegistry::builder().build().unwrap())
                .with_mcp_registry(McpRegistry::new())
                .with_skill_registry(SkillRegistry::builder().build()),
        )
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry
        .activate(&plugin_id("product-plugin"))
        .await
        .unwrap();

    let snapshot = registry.product_snapshot();
    let detail = registry
        .product_detail(&plugin_id("product-plugin"))
        .expect("product detail exists");

    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].id, plugin_id("product-plugin"));
    assert_eq!(
        snapshot[0].state,
        harness_contracts::PluginProductState::Activated
    );
    assert!(snapshot[0].capabilities.iter().any(|capability| {
        capability.kind == harness_contracts::PluginRuntimeCapabilityKind::Tool
            && capability.name.as_deref() == Some("registered-tool")
            && capability.registered
    }));
    assert_eq!(detail.summary, snapshot[0]);
    assert_eq!(detail.registered_capabilities.len(), 5);
    assert_eq!(detail.manifest_hash, [3; 32]);
    assert_eq!(
        detail.manifest_origin,
        ManifestOriginRef::File {
            path: "<local-plugin>".to_owned()
        }
    );
    assert!(detail.rejection_reason.is_none());
    assert_eq!(detail.recent_events, vec![PluginRecentEvent::Loaded]);
}

#[tokio::test]
async fn admission_policy_rejects_plugins_during_discovery() {
    let denied = record("denied-plugin", PluginCapabilities::default());
    let allowed = record("allowed-plugin", PluginCapabilities::default());
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            enabled: true,
            allow_project_plugins: false,
            allowed_user_plugins: None,
            disabled_plugins: BTreeSet::new(),
            policy: PluginAdmissionPolicy::Allow(BTreeSet::from([PluginName::new(
                "allowed-plugin",
            )
            .unwrap()])),
            entries: BTreeMap::new(),
            workspace_root: None,
            strict_plugin_only_customization: StrictPluginOnlyPolicy::default(),
        })
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![
            denied.clone(),
            allowed.clone(),
        ])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert_eq!(discovered.len(), 1);
    assert_eq!(
        discovered[0].record.manifest.plugin_id(),
        allowed.manifest.plugin_id()
    );
    assert!(matches!(
        registry.state(&denied.manifest.plugin_id()),
        Some(harness_plugin::PluginLifecycleState::Rejected(_))
    ));
}

#[tokio::test]
async fn deny_admission_policy_rejects_plugins_during_discovery() {
    let denied = record("denylisted-plugin", PluginCapabilities::default());
    let allowed = record("non-denied-plugin", PluginCapabilities::default());
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            enabled: true,
            allow_project_plugins: false,
            allowed_user_plugins: None,
            disabled_plugins: BTreeSet::new(),
            policy: PluginAdmissionPolicy::Deny(BTreeSet::from([PluginName::new(
                "denylisted-plugin",
            )
            .unwrap()])),
            entries: BTreeMap::new(),
            workspace_root: None,
            strict_plugin_only_customization: StrictPluginOnlyPolicy::default(),
        })
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![
            denied.clone(),
            allowed.clone(),
        ])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert_eq!(discovered.len(), 1);
    assert_eq!(
        discovered[0].record.manifest.plugin_id(),
        allowed.manifest.plugin_id()
    );
    assert!(matches!(
        registry.state(&denied.manifest.plugin_id()),
        Some(harness_plugin::PluginLifecycleState::Rejected(_))
    ));
}

#[tokio::test]
async fn strict_plugin_only_policy_blocks_user_controlled_tool_manifests() {
    let plugin = record(
        "user-tool-plugin",
        PluginCapabilities {
            tools: vec![harness_plugin::ToolManifestEntry {
                name: "user-tool".to_owned(),
                destructive: false,
                input_schema: serde_json::json!({ "type": "object" }),
            }],
            ..PluginCapabilities::default()
        },
    );
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            strict_plugin_only_customization: StrictPluginOnlyPolicy { enabled: true },
            ..PluginConfig::default()
        })
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![plugin.clone()])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert!(discovered.is_empty());
    assert!(matches!(
        registry.state(&plugin.manifest.plugin_id()),
        Some(harness_plugin::PluginLifecycleState::Rejected(_))
    ));
    assert!(matches!(
        registry
            .state_detail(&plugin.manifest.plugin_id())
            .and_then(|detail| detail.rejection_reason),
        Some(RejectionReason::AdmissionDenied { policy })
            if policy == "strict_plugin_only:user-tool-plugin"
    ));
}

#[tokio::test]
async fn registry_rejects_min_harness_version_mismatch_with_typed_reason() {
    let mut incompatible = record("future-plugin", PluginCapabilities::default());
    incompatible.manifest.min_harness_version = semver::VersionReq::parse(">=999.0.0").unwrap();
    let sink = Arc::new(CollectingSink::default());
    let registry = PluginRegistry::builder()
        .with_event_sink(sink.clone())
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![
            incompatible.clone()
        ])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert!(discovered.is_empty());
    assert!(matches!(
        registry.state(&incompatible.manifest.plugin_id()),
        Some(harness_plugin::PluginLifecycleState::Rejected(_))
    ));
    assert!(sink.events().iter().any(|event| matches!(
        event,
        Event::PluginRejected(rejected)
            if rejected.plugin_id == incompatible.manifest.plugin_id()
                && matches!(rejected.reason, RejectionReason::HarnessVersionIncompatible { .. })
    )));
    assert!(matches!(
        registry
            .state_detail(&incompatible.manifest.plugin_id())
            .and_then(|detail| detail.rejection_reason),
        Some(RejectionReason::HarnessVersionIncompatible { .. })
    ));
}

#[tokio::test]
async fn registry_rejects_reserved_prefix_for_user_controlled_plugins() {
    let reserved = record("jyowo-user-plugin", PluginCapabilities::default());
    let registry = PluginRegistry::builder()
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![reserved.clone()])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert!(discovered.is_empty());
    assert!(matches!(
        registry.state(&reserved.manifest.plugin_id()),
        Some(harness_plugin::PluginLifecycleState::Rejected(_))
    ));
}

#[tokio::test]
async fn source_priority_keeps_highest_priority_plugin_name() {
    let project = record("priority-plugin", PluginCapabilities::default());
    let mut user = record("priority-plugin", PluginCapabilities::default());
    user.origin = ManifestOrigin::File {
        path: "/user/priority-plugin/plugin.json".into(),
    };
    let sink = Arc::new(CollectingSink::default());
    let registry = PluginRegistry::builder()
        .with_event_sink(sink.clone())
        .with_config(PluginConfig {
            allow_project_plugins: true,
            ..PluginConfig::default()
        })
        .with_source(DiscoverySource::Project("/project".into()))
        .with_source(DiscoverySource::User("/user".into()))
        .with_manifest_loader(Arc::new(SourceAwareManifestLoader {
            project: project.clone(),
            user: user.clone(),
        }))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].source, DiscoverySource::User("/user".into()));
    assert_eq!(discovered[0].record.origin, user.origin);
    assert!(sink.events().iter().any(|event| matches!(
        event,
        Event::PluginRejected(rejected)
            if rejected.plugin_id == project.manifest.plugin_id()
                && matches!(rejected.reason, RejectionReason::NamespaceConflict { .. })
    )));
}

#[tokio::test]
async fn source_priority_rejects_multiple_versions_from_same_source() {
    let first = record_with_version("version-conflict", "0.1.0", PluginCapabilities::default());
    let second = record_with_version("version-conflict", "0.2.0", PluginCapabilities::default());
    let sink = Arc::new(CollectingSink::default());
    let registry = PluginRegistry::builder()
        .with_event_sink(sink.clone())
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![
            first.clone(),
            second.clone(),
        ])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert!(discovered.is_empty());
    assert!(matches!(
        registry.state(&first.manifest.plugin_id()),
        Some(harness_plugin::PluginLifecycleState::Rejected(_))
    ));
    assert!(matches!(
        registry.state(&second.manifest.plugin_id()),
        Some(harness_plugin::PluginLifecycleState::Rejected(_))
    ));
    let rejected = sink
        .events()
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                Event::PluginRejected(rejected)
                    if rejected.plugin_name == "version-conflict"
                        && matches!(rejected.reason, RejectionReason::NamespaceConflict { .. })
            )
        })
        .count();
    assert_eq!(rejected, 2);
}

#[tokio::test]
async fn configuration_schema_rejects_invalid_plugin_config_entry() {
    let mut manifest = record("schema-plugin", PluginCapabilities::default());
    manifest.manifest.capabilities.configuration_schema = Some(json!({
        "type": "object",
        "required": ["mode"],
        "properties": {
            "mode": { "type": "string" }
        }
    }));
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            enabled: true,
            allow_project_plugins: false,
            allowed_user_plugins: None,
            disabled_plugins: BTreeSet::new(),
            policy: PluginAdmissionPolicy::AllowAll,
            entries: BTreeMap::from([(
                PluginName::new("schema-plugin").unwrap(),
                json!({ "mode": 1 }),
            )]),
            workspace_root: None,
            strict_plugin_only_customization: StrictPluginOnlyPolicy::default(),
        })
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest.clone()])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert!(discovered.is_empty());
    assert!(matches!(
        registry.state(&manifest.manifest.plugin_id()),
        Some(harness_plugin::PluginLifecycleState::Rejected(_))
    ));
}

#[tokio::test]
async fn configuration_schema_validates_product_config_updates() {
    let mut manifest = record("configurable-plugin", PluginCapabilities::default());
    manifest.manifest.capabilities.configuration_schema = Some(json!({
        "type": "object",
        "required": ["mode"],
        "properties": {
            "mode": { "type": "string" },
            "limit": { "type": "number" }
        },
        "additionalProperties": false
    }));
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            entries: BTreeMap::from([(
                PluginName::new("configurable-plugin").unwrap(),
                json!({ "mode": "default" }),
            )]),
            ..PluginConfig::default()
        })
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest.clone()])))
        .build()
        .unwrap();

    registry.discover().await.unwrap();

    registry
        .validate_config_update(&harness_contracts::PluginConfigUpdate {
            plugin_id: plugin_id("configurable-plugin"),
            values: json!({ "mode": "strict", "limit": 8 }),
        })
        .unwrap();
    let error = registry
        .validate_config_update(&harness_contracts::PluginConfigUpdate {
            plugin_id: plugin_id("configurable-plugin"),
            values: json!({ "mode": 1 }),
        })
        .unwrap_err();

    assert!(matches!(error, PluginError::AdmissionDenied { .. }));

    let error = registry
        .validate_config_update(&harness_contracts::PluginConfigUpdate {
            plugin_id: plugin_id("configurable-plugin"),
            values: json!({ "mode": "strict", "unknown": "ignored" }),
        })
        .unwrap_err();

    assert!(matches!(error, PluginError::AdmissionDenied { .. }));
}

#[tokio::test]
async fn configuration_schema_allows_required_secret_to_be_managed_outside_public_config() {
    let mut manifest = record("secret-plugin", PluginCapabilities::default());
    manifest.manifest.capabilities.configuration_schema = Some(json!({
        "type": "object",
        "required": ["apiToken"],
        "properties": {
            "apiToken": { "type": "string", "secret": true },
            "lineWidth": { "type": "number" }
        },
        "additionalProperties": false
    }));
    let registry = PluginRegistry::builder()
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest.clone()])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert_eq!(discovered.len(), 1);
    registry
        .validate_config_update(&harness_contracts::PluginConfigUpdate {
            plugin_id: plugin_id("secret-plugin"),
            values: json!({ "lineWidth": 120 }),
        })
        .unwrap();
    let error = registry
        .validate_config_update(&harness_contracts::PluginConfigUpdate {
            plugin_id: plugin_id("secret-plugin"),
            values: json!({ "apiToken": "sk-unsafe-secret" }),
        })
        .unwrap_err();

    assert!(matches!(error, PluginError::AdmissionDenied { .. }));
}

#[tokio::test]
async fn product_detail_redacts_nested_secret_config_schema_and_values() {
    let mut manifest = record("nested-secret-plugin", PluginCapabilities::default());
    manifest.manifest.capabilities.configuration_schema = Some(json!({
        "type": "object",
        "properties": {
            "service": {
                "type": "object",
                "required": ["endpoint", "apiToken"],
                "properties": {
                    "endpoint": { "type": "string" },
                    "apiToken": { "type": "string", "secret": true }
                },
                "additionalProperties": false
            }
        },
        "additionalProperties": false
    }));
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            entries: BTreeMap::from([(
                PluginName::new("nested-secret-plugin").unwrap(),
                json!({
                    "service": {
                        "endpoint": "https://example.invalid",
                        "apiToken": "sk-unsafe-secret"
                    }
                }),
            )]),
            ..PluginConfig::default()
        })
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest.clone()])))
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    let detail = registry
        .product_detail(&manifest.manifest.plugin_id())
        .expect("plugin detail exists");

    assert_eq!(
        detail.config,
        json!({ "service": { "endpoint": "https://example.invalid" } })
    );
    let schema = detail.configuration_schema.expect("schema is public");
    assert!(schema["properties"]["service"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item != "apiToken"));
    assert!(schema["properties"]["service"]["properties"]
        .get("apiToken")
        .is_none());
    assert!(!detail.config.to_string().contains("sk-unsafe-secret"));
    assert!(!detail.manifest.to_string().contains("apiToken"));
    assert!(!schema.to_string().contains("apiToken"));
}

#[tokio::test]
async fn product_detail_redacts_secret_schema_maps_and_dynamic_config_values() {
    let mut manifest = record("dynamic-secret-plugin", PluginCapabilities::default());
    manifest.manifest.capabilities.configuration_schema = Some(json!({
        "type": "object",
        "properties": {
            "safeMode": { "type": "boolean" }
        },
        "patternProperties": {
            "^token": { "type": "string", "secret": true }
        },
        "additionalProperties": { "type": "string", "secret": true },
        "$defs": {
            "apiToken": { "type": "string", "secret": true },
            "safeCount": { "type": "number" }
        }
    }));
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            entries: BTreeMap::from([(
                PluginName::new("dynamic-secret-plugin").unwrap(),
                json!({
                    "safeMode": true,
                    "tokenValue": "sk-pattern-secret",
                    "extraToken": "sk-additional-secret"
                }),
            )]),
            ..PluginConfig::default()
        })
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest.clone()])))
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    let detail = registry
        .product_detail(&manifest.manifest.plugin_id())
        .expect("plugin detail exists");
    let schema = detail.configuration_schema.expect("schema is public");
    let encoded_schema = schema.to_string();
    let encoded_manifest = detail.manifest.to_string();
    let encoded_config = detail.config.to_string();

    assert_eq!(detail.config, json!({ "safeMode": true }));
    assert!(!encoded_config.contains("sk-pattern-secret"));
    assert!(!encoded_config.contains("sk-additional-secret"));
    assert!(!encoded_schema.contains("apiToken"));
    assert!(!encoded_schema.contains("\"secret\""));
    assert!(!encoded_manifest.contains("apiToken"));
    assert!(!encoded_manifest.contains("\"secret\""));
}

#[tokio::test]
async fn product_detail_redacts_pattern_property_secrets_using_json_schema_regex() {
    let mut manifest = record("regex-secret-plugin", PluginCapabilities::default());
    manifest.manifest.capabilities.configuration_schema = Some(json!({
        "type": "object",
        "patternProperties": {
            "^api(Token|Key)$": { "type": "string", "secret": true }
        },
        "additionalProperties": { "type": "string" }
    }));
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            entries: BTreeMap::from([(
                PluginName::new("regex-secret-plugin").unwrap(),
                json!({
                    "apiToken": "sk-pattern-secret",
                    "displayName": "Formatter"
                }),
            )]),
            ..PluginConfig::default()
        })
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest.clone()])))
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    let detail = registry
        .product_detail(&manifest.manifest.plugin_id())
        .expect("plugin detail exists");

    assert_eq!(detail.config, json!({ "displayName": "Formatter" }));
    assert!(!detail.config.to_string().contains("sk-pattern-secret"));
}

#[tokio::test]
async fn activation_context_receives_config_and_workspace_root() {
    let manifest = record("configured-plugin", PluginCapabilities::default());
    let plugin = Arc::new(CapturingPlugin::new(manifest.clone()));
    let runtime_plugin: Arc<dyn Plugin> = plugin.clone();
    let workspace_root = PathBuf::from("/tmp/jyowo-workspace");
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            enabled: true,
            allow_project_plugins: false,
            allowed_user_plugins: None,
            disabled_plugins: BTreeSet::new(),
            policy: PluginAdmissionPolicy::AllowAll,
            entries: BTreeMap::from([(
                PluginName::new("configured-plugin").unwrap(),
                json!({ "mode": "strict" }),
            )]),
            workspace_root: Some(workspace_root.clone()),
            strict_plugin_only_customization: StrictPluginOnlyPolicy::default(),
        })
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest])))
        .with_runtime_loader(Arc::new(CountingRuntimeLoader::new(runtime_plugin)))
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry
        .activate(&plugin_id("configured-plugin"))
        .await
        .unwrap();

    let ctx = plugin.captured_context().unwrap();
    assert_eq!(ctx.config, json!({ "mode": "strict" }));
    assert_eq!(ctx.workspace_root, Some(workspace_root));
}

#[tokio::test]
async fn activation_records_declared_capability_warnings() {
    let manifest = record(
        "partial-plugin",
        PluginCapabilities {
            tools: vec![harness_plugin::ToolManifestEntry {
                name: "implemented-tool".to_owned(),
                destructive: false,
                input_schema: serde_json::json!({ "type": "object" }),
            }],
            hooks: vec![harness_plugin::HookManifestEntry {
                name: "missing-hook".to_owned(),
                events: Vec::new(),
            }],
            ..PluginCapabilities::default()
        },
    );
    let plugin: Arc<dyn Plugin> = Arc::new(PartialRegisteringPlugin::new(manifest.clone()));
    let registry = PluginRegistry::builder()
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest.clone()])))
        .with_runtime_loader(Arc::new(CountingRuntimeLoader::new(plugin)))
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry
        .activate(&manifest.manifest.plugin_id())
        .await
        .unwrap();

    assert_eq!(
        registry
            .snapshot()
            .warnings
            .get(&manifest.manifest.plugin_id()),
        Some(&vec![
            harness_plugin::PluginWarning::DeclaredCapabilityUnregistered {
                kind: "hook",
                name: "missing-hook".to_owned(),
            }
        ])
    );
}

#[tokio::test]
async fn activate_injects_declared_coordinator_handle() {
    let manifest = record(
        "coordinator-plugin",
        PluginCapabilities {
            coordinator_strategy: Some(CoordinatorStrategyManifestEntry {
                name: "coordinator".to_owned(),
            }),
            ..PluginCapabilities::default()
        },
    );
    let plugin = Arc::new(CapturingPlugin::new(manifest.clone()));
    let runtime_plugin: Arc<dyn Plugin> = plugin.clone();
    let registry = registry_for(
        manifest,
        Arc::new(CountingRuntimeLoader::new(runtime_plugin)),
    );

    registry.discover().await.unwrap();
    registry
        .activate(&plugin_id("coordinator-plugin"))
        .await
        .unwrap();

    let ctx = plugin.captured_context().unwrap();
    assert!(ctx.coordinator.is_some());
    assert!(ctx.memory.is_none());
}

#[tokio::test]
async fn activate_injects_declared_steering_handle_for_admin_trusted_plugin() {
    let mut manifest = record(
        "steering-plugin",
        PluginCapabilities {
            steering: true,
            ..PluginCapabilities::default()
        },
    );
    manifest.manifest.trust_level = TrustLevel::AdminTrusted;
    let registry = PluginRegistry::builder()
        .with_capability_registries(
            PluginCapabilityRegistries::default()
                .with_steering_registration(Arc::new(FakeSteeringRegistration::default())),
        )
        .build()
        .unwrap();
    let ctx = registry.activation_context_for_test(&manifest.manifest);

    assert!(ctx.steering.is_some());
    assert!(ctx.tools.is_none());
}

#[tokio::test]
async fn steering_capability_requires_downstream_registration() {
    let mut manifest = record(
        "steering-plugin",
        PluginCapabilities {
            steering: true,
            ..PluginCapabilities::default()
        },
    );
    let keypair = test_keypair();
    manifest.manifest = signed_admin_manifest(manifest.manifest, &keypair);
    let plugin = Arc::new(CapturingPlugin::new(manifest.clone()));
    let runtime_plugin: Arc<dyn Plugin> = plugin.clone();
    let runtime = Arc::new(CountingRuntimeLoader::new(runtime_plugin));
    let registry = PluginRegistry::builder()
        .with_source(DiscoverySource::Inline)
        .with_trusted_signer(keypair.public_key().as_ref().to_vec())
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest])))
        .with_runtime_loader(runtime.clone())
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    let error = registry
        .activate(&plugin_id("steering-plugin"))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        PluginError::Registration(RegistrationError::OwnerRegistry {
            kind: "steering",
            ..
        })
    ));
    assert_eq!(runtime.load_count(), 0);
    assert!(plugin.captured_context().is_none());
}

#[tokio::test]
async fn steering_handle_forces_plugin_source_and_normal_priority() {
    let mut manifest = record(
        "steering-plugin",
        PluginCapabilities {
            steering: true,
            ..PluginCapabilities::default()
        },
    );
    manifest.manifest.trust_level = TrustLevel::AdminTrusted;
    let downstream = Arc::new(FakeSteeringRegistration::default());
    let registry = PluginRegistry::builder()
        .with_capability_registries(
            PluginCapabilityRegistries::default().with_steering_registration(downstream.clone()),
        )
        .build()
        .unwrap();
    let ctx = registry.activation_context_for_test(&manifest.manifest);

    let id = ctx
        .steering
        .expect("steering handle")
        .push(SteeringRequest {
            kind: SteeringKind::Append,
            body: SteeringBody::Text("ignored".to_owned()),
            priority: Some(SteeringPriority::High),
            correlation_id: None,
            source: SteeringSource::User,
        })
        .await
        .unwrap();

    assert_eq!(id, downstream.id());
    let request = downstream.request().unwrap();
    assert_eq!(
        request.source,
        SteeringSource::Plugin {
            plugin_id: plugin_id("steering-plugin")
        }
    );
    assert_eq!(request.priority, Some(SteeringPriority::Normal));
}

#[tokio::test]
async fn user_controlled_plugin_cannot_declare_steering_capability() {
    let mut manifest = record(
        "user-steering-plugin",
        PluginCapabilities {
            steering: true,
            ..PluginCapabilities::default()
        },
    );
    manifest.manifest.trust_level = TrustLevel::UserControlled;
    let registry = PluginRegistry::builder()
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest])))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert!(discovered.is_empty());
    assert!(matches!(
        registry
            .state_detail(&plugin_id("user-steering-plugin"))
            .unwrap()
            .state,
        PluginLifecycleState::Rejected(_)
    ));
}

#[tokio::test]
async fn capability_handles_reject_undeclared_registrations() {
    let manifest = record(
        "declared-tool",
        PluginCapabilities {
            tools: vec![harness_plugin::ToolManifestEntry {
                name: "allowed".to_owned(),
                destructive: false,
                input_schema: serde_json::json!({ "type": "object" }),
            }],
            hooks: vec![harness_plugin::HookManifestEntry {
                name: "allowed-hook".to_owned(),
                events: Vec::new(),
            }],
            mcp_servers: vec![harness_plugin::McpManifestEntry {
                name: "allowed-mcp".to_owned(),
            }],
            skills: vec![harness_plugin::SkillManifestEntry {
                name: "allowed-skill".to_owned(),
            }],
            ..PluginCapabilities::default()
        },
    );
    let registry = PluginRegistry::builder().build().unwrap();
    let ctx = registry.activation_context_for_test(&manifest.manifest);

    let tool_error = ctx
        .tools
        .unwrap()
        .register(Box::new(FakeTool::new("not-declared")))
        .await
        .unwrap_err();
    assert_eq!(
        tool_error,
        RegistrationError::UndeclaredTool {
            name: "not-declared".to_owned()
        }
    );

    let hook_error = ctx
        .hooks
        .unwrap()
        .register(Box::new(FakeHook::new("not-declared-hook")))
        .await
        .unwrap_err();
    assert_eq!(
        hook_error,
        RegistrationError::UndeclaredHook {
            name: "not-declared-hook".to_owned()
        }
    );

    let mcp_error = ctx
        .mcp
        .unwrap()
        .register(mcp_spec("not-declared-mcp"))
        .await
        .unwrap_err();
    assert_eq!(
        mcp_error,
        RegistrationError::UndeclaredMcp {
            name: "not-declared-mcp".to_owned()
        }
    );

    let skill_error = ctx
        .skills
        .unwrap()
        .register(fake_skill("not-declared-skill"))
        .await
        .unwrap_err();
    assert_eq!(
        skill_error,
        RegistrationError::UndeclaredSkill {
            name: "not-declared-skill".to_owned()
        }
    );
}

#[tokio::test]
async fn plugin_metrics_records_undeclared_capability_registration_rejections() {
    let manifest = record(
        "declared-capabilities",
        PluginCapabilities {
            tools: vec![harness_plugin::ToolManifestEntry {
                name: "allowed".to_owned(),
                destructive: false,
                input_schema: serde_json::json!({ "type": "object" }),
            }],
            hooks: vec![harness_plugin::HookManifestEntry {
                name: "allowed-hook".to_owned(),
                events: Vec::new(),
            }],
            mcp_servers: vec![harness_plugin::McpManifestEntry {
                name: "allowed-mcp".to_owned(),
            }],
            skills: vec![harness_plugin::SkillManifestEntry {
                name: "allowed-skill".to_owned(),
            }],
            ..PluginCapabilities::default()
        },
    );
    let metrics = Arc::new(CapabilityMetrics::default());
    let registry = PluginRegistry::builder()
        .with_metrics_sink(metrics.clone())
        .build()
        .unwrap();
    let ctx = registry.activation_context_for_test(&manifest.manifest);

    let _ = ctx
        .tools
        .unwrap()
        .register(Box::new(FakeTool::new("not-declared")))
        .await;
    let _ = ctx
        .hooks
        .unwrap()
        .register(Box::new(FakeHook::new("not-declared-hook")))
        .await;
    let _ = ctx
        .mcp
        .unwrap()
        .register(mcp_spec("not-declared-mcp"))
        .await;
    let _ = ctx
        .skills
        .unwrap()
        .register(fake_skill("not-declared-skill"))
        .await;

    assert_eq!(
        metrics.records(),
        vec![
            ("tool".to_owned(), "undeclared_tool".to_owned()),
            ("hook".to_owned(), "undeclared_hook".to_owned()),
            ("mcp".to_owned(), "undeclared_mcp".to_owned()),
            ("skill".to_owned(), "undeclared_skill".to_owned()),
        ]
    );
}

#[tokio::test]
async fn activation_records_missing_custom_toolset_warning() {
    let manifest = record(
        "toolset-warning",
        PluginCapabilities {
            custom_toolsets: vec![CustomToolsetManifestEntry {
                name: "missing-toolset".to_owned(),
            }],
            ..PluginCapabilities::default()
        },
    );
    let plugin: Arc<dyn Plugin> = Arc::new(NoopPlugin::new(manifest.clone()));
    let registry = PluginRegistry::builder()
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest.clone()])))
        .with_runtime_loader(Arc::new(CountingRuntimeLoader::new(plugin)))
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry
        .activate(&plugin_id("toolset-warning"))
        .await
        .unwrap();

    assert_eq!(
        registry
            .snapshot()
            .warnings
            .get(&plugin_id("toolset-warning")),
        Some(&vec![
            harness_plugin::PluginWarning::DeclaredCapabilityUnregistered {
                kind: "custom_toolset",
                name: "missing-toolset".to_owned(),
            }
        ])
    );
}

#[tokio::test]
async fn plugin_registered_capabilities_are_written_to_owning_registries() {
    let manifest = record(
        "registering-plugin",
        PluginCapabilities {
            tools: vec![harness_plugin::ToolManifestEntry {
                name: "registered-tool".to_owned(),
                destructive: false,
                input_schema: serde_json::json!({ "type": "object" }),
            }],
            hooks: vec![harness_plugin::HookManifestEntry {
                name: "registered-hook".to_owned(),
                events: Vec::new(),
            }],
            mcp_servers: vec![harness_plugin::McpManifestEntry {
                name: "registered-mcp".to_owned(),
            }],
            skills: vec![harness_plugin::SkillManifestEntry {
                name: "registered-skill".to_owned(),
            }],
            memory_provider: Some(harness_plugin::MemoryProviderManifestEntry {
                name: "memory".to_owned(),
            }),
            ..PluginCapabilities::default()
        },
    );
    let plugin_id = plugin_id("registering-plugin");
    let tool_registry = ToolRegistry::builder().build().unwrap();
    let hook_registry = HookRegistry::builder().build().unwrap();
    let mcp_registry = McpRegistry::new();
    let skill_registry = SkillRegistry::builder().build();
    let plugin: Arc<dyn Plugin> = Arc::new(RegisteringPlugin::new(manifest.clone()));
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            allow_project_plugins: true,
            ..PluginConfig::default()
        })
        .with_source(DiscoverySource::Project("/workspace".into()))
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest])))
        .with_runtime_loader(Arc::new(CountingRuntimeLoader::new(plugin)))
        .with_capability_registries(
            PluginCapabilityRegistries::default()
                .with_tool_registry(tool_registry.clone())
                .with_hook_registry(hook_registry.clone())
                .with_mcp_registry(mcp_registry.clone())
                .with_skill_registry(skill_registry.clone()),
        )
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry.activate(&plugin_id).await.unwrap();

    let tool_descriptor = tool_registry
        .snapshot()
        .descriptor("registered-tool")
        .expect("tool should be registered")
        .as_ref()
        .clone();
    assert_eq!(
        tool_descriptor.origin,
        ToolOrigin::Plugin {
            plugin_id: plugin_id.clone(),
            trust: TrustLevel::UserControlled,
        }
    );
    assert_eq!(
        hook_registry.origin_for("registered-hook"),
        Some(HookOrigin::Plugin {
            plugin_id: plugin_id.clone(),
            trust: TrustLevel::UserControlled,
        })
    );
    assert_eq!(
        mcp_registry
            .server_spec(&McpServerId("registered-mcp".to_owned()))
            .await
            .expect("mcp server should be registered")
            .source,
        McpServerSource::Plugin(plugin_id.clone())
    );
    assert_eq!(
        skill_registry
            .get("registered-skill")
            .expect("skill should be registered")
            .source,
        SkillSource::Plugin {
            plugin_id: plugin_id.clone(),
            trust: TrustLevel::UserControlled,
        }
    );
    assert_eq!(
        registry
            .registered_memory_provider()
            .expect("memory provider should be registered")
            .provider_id(),
        "registered-memory"
    );
}

#[tokio::test]
async fn deactivate_failure_still_unregisters_plugin_capabilities() {
    let manifest = record(
        "deactivate-fails",
        PluginCapabilities {
            tools: vec![harness_plugin::ToolManifestEntry {
                name: "cleanup-tool".to_owned(),
                destructive: false,
                input_schema: serde_json::json!({ "type": "object" }),
            }],
            ..PluginCapabilities::default()
        },
    );
    let plugin_id = plugin_id("deactivate-fails");
    let tool_registry = ToolRegistry::builder().build().unwrap();
    let plugin: Arc<dyn Plugin> = Arc::new(DeactivateFailingPlugin::new(manifest.clone()));
    let registry = PluginRegistry::builder()
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest])))
        .with_runtime_loader(Arc::new(CountingRuntimeLoader::new(plugin)))
        .with_capability_registries(
            PluginCapabilityRegistries::default().with_tool_registry(tool_registry.clone()),
        )
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry.activate(&plugin_id).await.unwrap();
    assert!(tool_registry.get("cleanup-tool").is_some());

    let error = registry.deactivate(&plugin_id).await.unwrap_err();

    assert!(matches!(error, PluginError::DeactivateFailed(_)));
    assert!(tool_registry.get("cleanup-tool").is_none());
    assert!(matches!(
        registry.state(&plugin_id),
        Some(harness_plugin::PluginLifecycleState::Failed(_))
    ));
}

#[tokio::test]
async fn deactivate_unregisters_host_capabilities_before_plugin_deactivate_callback() {
    let manifest = record(
        "ordered-deactivate",
        PluginCapabilities {
            tools: vec![harness_plugin::ToolManifestEntry {
                name: "ordered-tool".to_owned(),
                destructive: false,
                input_schema: serde_json::json!({ "type": "object" }),
            }],
            ..PluginCapabilities::default()
        },
    );
    let plugin_id = plugin_id("ordered-deactivate");
    let tool_registry = ToolRegistry::builder().build().unwrap();
    let plugin: Arc<dyn Plugin> = Arc::new(DeactivateObservingPlugin::new(
        manifest.clone(),
        tool_registry.clone(),
    ));
    let registry = PluginRegistry::builder()
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest])))
        .with_runtime_loader(Arc::new(CountingRuntimeLoader::new(plugin)))
        .with_capability_registries(
            PluginCapabilityRegistries::default().with_tool_registry(tool_registry.clone()),
        )
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry.activate(&plugin_id).await.unwrap();
    assert!(tool_registry.get("ordered-tool").is_some());

    registry.deactivate(&plugin_id).await.unwrap();
}

#[tokio::test]
async fn deactivate_does_not_unregister_shadowed_builtin_tool() {
    let manifest = record(
        "shadowed-tool-plugin",
        PluginCapabilities {
            tools: vec![harness_plugin::ToolManifestEntry {
                name: "shared-tool".to_owned(),
                destructive: false,
                input_schema: serde_json::json!({ "type": "object" }),
            }],
            ..PluginCapabilities::default()
        },
    );
    let plugin_id = plugin_id("shadowed-tool-plugin");
    let tool_registry = ToolRegistry::builder().build().unwrap();
    tool_registry
        .register(Box::new(FakeTool::builtin("shared-tool")))
        .unwrap();
    let plugin: Arc<dyn Plugin> = Arc::new(SpecificToolPlugin::new(
        manifest.clone(),
        "shared-tool".to_owned(),
    ));
    let registry = PluginRegistry::builder()
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest])))
        .with_runtime_loader(Arc::new(CountingRuntimeLoader::new(plugin)))
        .with_capability_registries(
            PluginCapabilityRegistries::default().with_tool_registry(tool_registry.clone()),
        )
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry.activate(&plugin_id).await.unwrap();
    registry.deactivate(&plugin_id).await.unwrap();

    let snapshot = tool_registry.snapshot();
    let descriptor = snapshot
        .descriptor("shared-tool")
        .expect("builtin tool must remain after shadowing plugin deactivates");
    assert_eq!(descriptor.origin, ToolOrigin::Builtin);
}

#[tokio::test]
async fn deactivate_unregisters_plugin_mcp_injected_tools() {
    let manifest = record(
        "plugin-mcp",
        PluginCapabilities {
            mcp_servers: vec![harness_plugin::McpManifestEntry {
                name: "registered-mcp".to_owned(),
            }],
            ..PluginCapabilities::default()
        },
    );
    let plugin_id = plugin_id("plugin-mcp");
    let tool_registry = ToolRegistry::builder().build().unwrap();
    let mcp_registry = McpRegistry::new();
    let plugin: Arc<dyn Plugin> = Arc::new(ReadyMcpPlugin::new(manifest.clone()));
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            allow_project_plugins: true,
            ..PluginConfig::default()
        })
        .with_source(DiscoverySource::Project("/workspace".into()))
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest])))
        .with_runtime_loader(Arc::new(CountingRuntimeLoader::new(plugin)))
        .with_capability_registries(
            PluginCapabilityRegistries::default()
                .with_tool_registry(tool_registry.clone())
                .with_mcp_registry(mcp_registry.clone()),
        )
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry.activate(&plugin_id).await.unwrap();
    mcp_registry
        .inject_tools_into(&tool_registry, &McpServerId("registered-mcp".to_owned()))
        .await
        .unwrap();
    assert!(tool_registry.get("mcp__registered-mcp__lookup").is_some());

    registry.deactivate(&plugin_id).await.unwrap();

    assert!(tool_registry.get("mcp__registered-mcp__lookup").is_none());
    assert!(mcp_registry
        .server_spec(&McpServerId("registered-mcp".to_owned()))
        .await
        .is_none());
}

#[tokio::test]
async fn user_controlled_destructive_tool_registration_is_rejected() {
    let manifest = record(
        "dangerous-plugin",
        PluginCapabilities {
            tools: vec![harness_plugin::ToolManifestEntry {
                name: "dangerous-tool".to_owned(),
                destructive: true,
                input_schema: serde_json::json!({ "type": "object" }),
            }],
            ..PluginCapabilities::default()
        },
    );
    let registry = PluginRegistry::builder().build().unwrap();
    let ctx = registry.activation_context_for_test(&manifest.manifest);

    let error = ctx
        .tools
        .unwrap()
        .register(Box::new(FakeTool::destructive("dangerous-tool")))
        .await
        .unwrap_err();

    assert!(matches!(error, RegistrationError::TrustViolation { .. }));
}

#[tokio::test]
async fn tool_descriptor_destructive_flag_must_match_manifest() {
    let manifest = record(
        "mismatch-plugin",
        PluginCapabilities {
            tools: vec![harness_plugin::ToolManifestEntry {
                name: "mismatch-tool".to_owned(),
                destructive: false,
                input_schema: serde_json::json!({ "type": "object" }),
            }],
            ..PluginCapabilities::default()
        },
    );
    let registry = PluginRegistry::builder().build().unwrap();
    let ctx = registry.activation_context_for_test(&manifest.manifest);

    let error = ctx
        .tools
        .unwrap()
        .register(Box::new(FakeTool::destructive("mismatch-tool")))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        RegistrationError::DescriptorMismatch {
            name,
            declared_destructive: false,
            actual_destructive: true,
        } if name == "mismatch-tool"
    ));
}

#[tokio::test]
async fn user_controlled_fail_closed_hook_registration_is_rejected() {
    let manifest = record(
        "hook-plugin",
        PluginCapabilities {
            hooks: vec![harness_plugin::HookManifestEntry {
                name: "fail-closed-hook".to_owned(),
                events: Vec::new(),
            }],
            ..PluginCapabilities::default()
        },
    );
    let registry = PluginRegistry::builder().build().unwrap();
    let ctx = registry.activation_context_for_test(&manifest.manifest);

    let error = ctx
        .hooks
        .unwrap()
        .register(Box::new(FailClosedHook::new("fail-closed-hook")))
        .await
        .unwrap_err();

    assert!(matches!(error, RegistrationError::TrustViolation { .. }));
}

#[tokio::test]
async fn user_controlled_exec_hook_registration_is_rejected() {
    let manifest = record(
        "exec-hook-plugin",
        PluginCapabilities {
            hooks: vec![harness_plugin::HookManifestEntry {
                name: "exec-hook".to_owned(),
                events: Vec::new(),
            }],
            ..PluginCapabilities::default()
        },
    );
    let registry = PluginRegistry::builder().build().unwrap();
    let ctx = registry.activation_context_for_test(&manifest.manifest);

    let error = ctx
        .hooks
        .unwrap()
        .register(Box::new(ExecKindHook::new("exec-hook")))
        .await
        .unwrap_err();

    assert!(matches!(error, RegistrationError::TrustViolation { .. }));
}

#[tokio::test]
async fn user_controlled_http_hook_without_security_posture_is_rejected() {
    let manifest = record(
        "http-hook-plugin",
        PluginCapabilities {
            hooks: vec![harness_plugin::HookManifestEntry {
                name: "http-hook".to_owned(),
                events: Vec::new(),
            }],
            ..PluginCapabilities::default()
        },
    );
    let registry = PluginRegistry::builder().build().unwrap();
    let ctx = registry.activation_context_for_test(&manifest.manifest);

    let error = ctx
        .hooks
        .unwrap()
        .register(Box::new(HttpKindHook::new("http-hook")))
        .await
        .unwrap_err();

    assert!(matches!(error, RegistrationError::TrustViolation { .. }));
}

#[tokio::test]
async fn hook_declared_trust_must_match_plugin_trust() {
    let manifest = record(
        "trust-hook-plugin",
        PluginCapabilities {
            hooks: vec![harness_plugin::HookManifestEntry {
                name: "trust-hook".to_owned(),
                events: Vec::new(),
            }],
            ..PluginCapabilities::default()
        },
    );
    let registry = PluginRegistry::builder().build().unwrap();
    let ctx = registry.activation_context_for_test(&manifest.manifest);

    let error = ctx
        .hooks
        .unwrap()
        .register(Box::new(TrustDeclaringHook::new(
            "trust-hook",
            TrustLevel::AdminTrusted,
        )))
        .await
        .unwrap_err();

    assert!(matches!(error, RegistrationError::TrustViolation { .. }));
}

#[tokio::test]
async fn plugin_mcp_registration_preserves_admin_trust() {
    let mut manifest = record(
        "admin-mcp-plugin",
        PluginCapabilities {
            mcp_servers: vec![harness_plugin::McpManifestEntry {
                name: "admin-mcp".to_owned(),
            }],
            ..PluginCapabilities::default()
        },
    );
    manifest.manifest.trust_level = TrustLevel::AdminTrusted;
    let mcp_registry = McpRegistry::new();
    let registry = PluginRegistry::builder()
        .with_capability_registries(
            PluginCapabilityRegistries::default().with_mcp_registry(mcp_registry.clone()),
        )
        .build()
        .unwrap();
    let ctx = registry.activation_context_for_test(&manifest.manifest);

    ctx.mcp
        .unwrap()
        .register(mcp_spec("admin-mcp"))
        .await
        .unwrap();

    assert_eq!(
        mcp_registry
            .server_spec(&McpServerId("admin-mcp".to_owned()))
            .await
            .unwrap()
            .trust,
        TrustLevel::AdminTrusted
    );
}

#[tokio::test]
async fn user_controlled_remote_mcp_registration_is_rejected() {
    let manifest = record(
        "remote-mcp-plugin",
        PluginCapabilities {
            mcp_servers: vec![harness_plugin::McpManifestEntry {
                name: "remote-mcp".to_owned(),
            }],
            ..PluginCapabilities::default()
        },
    );
    let registry = PluginRegistry::builder()
        .with_capability_registries(
            PluginCapabilityRegistries::default().with_mcp_registry(McpRegistry::new()),
        )
        .build()
        .unwrap();
    let ctx = registry.activation_context_for_test(&manifest.manifest);
    let remote = McpServerSpec::new(
        McpServerId("remote-mcp".to_owned()),
        "remote-mcp",
        TransportChoice::Http {
            url: "https://example.com/mcp".to_owned(),
            headers: BTreeMap::new(),
        },
        McpServerSource::Plugin(plugin_id("remote-mcp-plugin")),
    );

    let error = ctx.mcp.unwrap().register(remote).await.unwrap_err();

    assert!(matches!(error, RegistrationError::TrustViolation { .. }));
}

#[tokio::test]
async fn activate_rejects_result_registrations_outside_manifest() {
    let manifest = record("bad-result", PluginCapabilities::default());
    let plugin: Arc<dyn Plugin> = Arc::new(ResultPlugin::new(
        manifest.clone(),
        PluginActivationResult {
            registered_tools: vec!["extra-tool".to_owned()],
            ..PluginActivationResult::default()
        },
    ));
    let registry = registry_for(manifest, Arc::new(CountingRuntimeLoader::new(plugin)));

    registry.discover().await.unwrap();
    let error = registry
        .activate(&plugin_id("bad-result"))
        .await
        .unwrap_err();

    assert!(matches!(error, PluginError::Registration(_)));
    assert!(matches!(
        registry.state(&plugin_id("bad-result")).unwrap(),
        harness_plugin::PluginLifecycleState::Failed(_)
    ));
}

#[tokio::test]
async fn failed_activation_can_be_retried_from_validated_manifest() {
    let manifest = record("retryable", PluginCapabilities::default());
    let plugin = Arc::new(RetryPlugin::new(manifest.clone()));
    let runtime_plugin: Arc<dyn Plugin> = plugin.clone();
    let sink = Arc::new(CollectingSink::default());
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            allow_project_plugins: true,
            ..PluginConfig::default()
        })
        .with_source(DiscoverySource::Project("/workspace".into()))
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest])))
        .with_runtime_loader(Arc::new(CountingRuntimeLoader::new(runtime_plugin)))
        .with_event_sink(sink.clone())
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    let first = registry
        .activate(&plugin_id("retryable"))
        .await
        .unwrap_err();
    assert!(matches!(first, PluginError::ActivateFailed(_)));
    assert!(matches!(
        registry.state(&plugin_id("retryable")).unwrap(),
        harness_plugin::PluginLifecycleState::Failed(_)
    ));
    let failed_detail = registry
        .product_detail(&plugin_id("retryable"))
        .expect("product detail exists");
    assert_eq!(failed_detail.recent_events, vec![PluginRecentEvent::Failed]);
    assert!(sink.events().iter().any(|event| matches!(
        event,
        Event::PluginFailed(failed)
            if failed.plugin_id == plugin_id("retryable")
                && failed.failure == "Plugin failure details withheld."
    )));
    assert!(!sink.events().iter().any(|event| matches!(
        event,
        Event::PluginRejected(rejected)
            if rejected.plugin_id == plugin_id("retryable")
    )));

    registry.discover().await.unwrap();
    let rediscovered_detail = registry
        .product_detail(&plugin_id("retryable"))
        .expect("product detail exists");
    assert!(matches!(
        rediscovered_detail.summary.state,
        harness_contracts::PluginProductState::Failed
    ));
    assert_eq!(
        rediscovered_detail.recent_events,
        vec![PluginRecentEvent::Failed]
    );

    registry.activate(&plugin_id("retryable")).await.unwrap();
    assert_eq!(
        registry.state(&plugin_id("retryable")).unwrap(),
        harness_plugin::PluginLifecycleState::Activated
    );
    let activated_detail = registry
        .product_detail(&plugin_id("retryable"))
        .expect("product detail exists");
    assert_eq!(
        activated_detail.recent_events,
        vec![PluginRecentEvent::Failed, PluginRecentEvent::Loaded]
    );
}

#[tokio::test]
async fn activation_rejects_slots_not_declared_by_manifest() {
    let manifest = record("undeclared-slot", PluginCapabilities::default());
    let plugin: Arc<dyn Plugin> = Arc::new(ResultPlugin::new(
        manifest.clone(),
        PluginActivationResult {
            occupied_slots: vec![CapabilitySlot::MemoryProvider],
            ..PluginActivationResult::default()
        },
    ));
    let registry = registry_for(manifest, Arc::new(CountingRuntimeLoader::new(plugin)));

    registry.discover().await.unwrap();
    let error = registry
        .activate(&plugin_id("undeclared-slot"))
        .await
        .unwrap_err();

    assert!(matches!(error, PluginError::Registration(_)));
    assert!(matches!(
        registry.state(&plugin_id("undeclared-slot")).unwrap(),
        harness_plugin::PluginLifecycleState::Failed(_)
    ));
}

#[tokio::test]
async fn memory_provider_deactivate_releases_registration_and_is_idempotent() {
    let first = memory_plugin("memory-one");
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            allow_project_plugins: true,
            ..PluginConfig::default()
        })
        .with_source(DiscoverySource::Project("/workspace".into()))
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![first.clone()])))
        .with_runtime_loader(Arc::new(CountingRuntimeLoader::new(Arc::new(
            MemoryRegisteringPlugin::new(first, "memory-one-provider"),
        ))))
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry.activate(&plugin_id("memory-one")).await.unwrap();
    assert_eq!(
        registry
            .registered_memory_provider()
            .expect("memory provider")
            .provider_id(),
        "memory-one-provider"
    );

    registry.deactivate(&plugin_id("memory-one")).await.unwrap();
    assert!(registry.registered_memory_provider().is_none());
    registry.deactivate(&plugin_id("memory-one")).await.unwrap();

    assert_eq!(
        registry.state(&plugin_id("memory-one")).unwrap(),
        harness_plugin::PluginLifecycleState::Deactivated
    );
}

#[tokio::test]
async fn multiple_memory_provider_plugins_can_register_together() {
    let first = memory_plugin("memory-one");
    let second = memory_plugin("memory-two");
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            allow_project_plugins: true,
            ..PluginConfig::default()
        })
        .with_source(DiscoverySource::Project("/workspace".into()))
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![
            first.clone(),
            second.clone(),
        ])))
        .with_runtime_loader(Arc::new(MultiRuntimeLoader::new(vec![
            Arc::new(MemoryRegisteringPlugin::new(first, "memory-one-provider")),
            Arc::new(MemoryRegisteringPlugin::new(second, "memory-two-provider")),
        ])))
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry.activate(&plugin_id("memory-one")).await.unwrap();
    registry.activate(&plugin_id("memory-two")).await.unwrap();

    let mut provider_ids = registry
        .registered_memory_providers()
        .into_iter()
        .map(|provider| provider.provider_id().to_owned())
        .collect::<Vec<_>>();
    provider_ids.sort();
    assert_eq!(
        provider_ids,
        vec!["memory-one-provider", "memory-two-provider"]
    );

    registry.deactivate(&plugin_id("memory-one")).await.unwrap();
    assert_eq!(
        registry
            .registered_memory_providers()
            .into_iter()
            .map(|provider| provider.provider_id().to_owned())
            .collect::<Vec<_>>(),
        vec!["memory-two-provider"]
    );
}

#[tokio::test]
async fn coordinator_slot_conflicts_reject_second_activation() {
    let first = coordinator_plugin("coordinator-one");
    let second = coordinator_plugin("coordinator-two");
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            allow_project_plugins: true,
            ..PluginConfig::default()
        })
        .with_source(DiscoverySource::Project("/workspace".into()))
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![
            first.clone(),
            second.clone(),
        ])))
        .with_runtime_loader(Arc::new(MultiRuntimeLoader::new(vec![
            Arc::new(ResultPlugin::new(
                first.clone(),
                PluginActivationResult {
                    occupied_slots: vec![CapabilitySlot::CoordinatorStrategy],
                    ..PluginActivationResult::default()
                },
            )),
            Arc::new(ResultPlugin::new(
                second.clone(),
                PluginActivationResult {
                    occupied_slots: vec![CapabilitySlot::CoordinatorStrategy],
                    ..PluginActivationResult::default()
                },
            )),
        ])))
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry
        .activate(&plugin_id("coordinator-one"))
        .await
        .unwrap();
    let error = registry
        .activate(&plugin_id("coordinator-two"))
        .await
        .unwrap_err();
    assert!(matches!(error, PluginError::SlotOccupied { .. }));

    registry
        .deactivate(&plugin_id("coordinator-one"))
        .await
        .unwrap();
    registry
        .activate(&plugin_id("coordinator-two"))
        .await
        .unwrap();
}

#[tokio::test]
async fn custom_toolset_slot_requires_manifest_declaration() {
    let manifest = record(
        "implicit-toolset",
        PluginCapabilities {
            tools: vec![harness_plugin::ToolManifestEntry {
                name: "bundle".to_owned(),
                destructive: false,
                input_schema: serde_json::json!({ "type": "object" }),
            }],
            ..PluginCapabilities::default()
        },
    );
    let plugin = Arc::new(ResultPlugin::new(
        manifest.clone(),
        PluginActivationResult {
            occupied_slots: vec![CapabilitySlot::CustomToolset("bundle".to_owned())],
            ..PluginActivationResult::default()
        },
    ));
    let registry = PluginRegistry::builder()
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest.clone()])))
        .with_runtime_loader(Arc::new(CountingRuntimeLoader::new(plugin)))
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    let error = registry
        .activate(&plugin_id("implicit-toolset"))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        PluginError::Registration(RegistrationError::UndeclaredResult { kind: "slot", .. })
    ));
}

#[tokio::test]
async fn declared_custom_toolset_slot_can_be_occupied() {
    let manifest = record(
        "declared-toolset",
        PluginCapabilities {
            custom_toolsets: vec![CustomToolsetManifestEntry {
                name: "bundle".to_owned(),
            }],
            ..PluginCapabilities::default()
        },
    );
    let plugin = Arc::new(ResultPlugin::new(
        manifest.clone(),
        PluginActivationResult {
            occupied_slots: vec![CapabilitySlot::CustomToolset("bundle".to_owned())],
            ..PluginActivationResult::default()
        },
    ));
    let registry = PluginRegistry::builder()
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![manifest.clone()])))
        .with_runtime_loader(Arc::new(CountingRuntimeLoader::new(plugin)))
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry
        .activate(&plugin_id("declared-toolset"))
        .await
        .unwrap();

    assert_eq!(
        registry
            .snapshot()
            .occupied_slots
            .get(&CapabilitySlot::CustomToolset("bundle".to_owned())),
        Some(&plugin_id("declared-toolset"))
    );
}

fn registry_for(record: ManifestRecord, runtime: Arc<CountingRuntimeLoader>) -> PluginRegistry {
    PluginRegistry::builder()
        .with_config(PluginConfig {
            allow_project_plugins: true,
            ..PluginConfig::default()
        })
        .with_source(DiscoverySource::Project("/workspace".into()))
        .with_manifest_loader(Arc::new(StaticManifestLoader::new(vec![record])))
        .with_runtime_loader(runtime)
        .build()
        .unwrap()
}

fn memory_plugin(name: &str) -> ManifestRecord {
    record(
        name,
        PluginCapabilities {
            memory_provider: Some(harness_plugin::MemoryProviderManifestEntry {
                name: "memory".to_owned(),
            }),
            ..PluginCapabilities::default()
        },
    )
}

fn coordinator_plugin(name: &str) -> ManifestRecord {
    record(
        name,
        PluginCapabilities {
            coordinator_strategy: Some(CoordinatorStrategyManifestEntry {
                name: "coordinator".to_owned(),
            }),
            ..PluginCapabilities::default()
        },
    )
}

fn record(name: &str, capabilities: PluginCapabilities) -> ManifestRecord {
    record_with_version(name, "0.1.0", capabilities)
}

fn record_with_version(
    name: &str,
    version: &str,
    capabilities: PluginCapabilities,
) -> ManifestRecord {
    ManifestRecord::new(
        PluginManifest {
            manifest_schema_version: 1,
            name: PluginName::new(name).unwrap(),
            version: semver::Version::parse(version).unwrap(),
            trust_level: TrustLevel::UserControlled,
            description: None,
            authors: Vec::new(),
            repository: None,
            signature: None,
            capabilities,
            dependencies: Vec::new(),
            min_harness_version: semver::VersionReq::parse(">=0.0.0").unwrap(),
        },
        ManifestOrigin::File {
            path: format!("/plugins/{name}/plugin.json").into(),
        },
        [3; 32],
    )
    .unwrap()
}

fn plugin_id(name: &str) -> PluginId {
    PluginId(format!("{name}@0.1.0"))
}

fn test_keypair() -> Ed25519KeyPair {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()
}

fn signed_admin_manifest(mut manifest: PluginManifest, keypair: &Ed25519KeyPair) -> PluginManifest {
    manifest.trust_level = TrustLevel::AdminTrusted;
    let payload = ManifestSigner::canonical_payload(&manifest).unwrap();
    manifest.signature = Some(ManifestSignature {
        algorithm: SignatureAlgorithm::Ed25519,
        signer: "user-injected-0".to_owned(),
        signature: keypair.sign(&payload).as_ref().to_vec(),
        timestamp: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH.to_rfc3339(),
    });
    manifest
}

struct StaticManifestLoader {
    records: Vec<ManifestRecord>,
}

impl StaticManifestLoader {
    fn new(records: Vec<ManifestRecord>) -> Self {
        Self { records }
    }
}

#[async_trait]
impl PluginManifestLoader for StaticManifestLoader {
    async fn enumerate(
        &self,
        _source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        Ok(self.records.clone())
    }
}

struct SourceAwareManifestLoader {
    project: ManifestRecord,
    user: ManifestRecord,
}

#[async_trait]
impl PluginManifestLoader for SourceAwareManifestLoader {
    async fn enumerate(
        &self,
        source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        match source {
            DiscoverySource::Project(_) => Ok(vec![self.project.clone()]),
            DiscoverySource::User(_) => Ok(vec![self.user.clone()]),
            _ => Ok(Vec::new()),
        }
    }
}

#[derive(Default)]
struct CollectingSink {
    events: Mutex<Vec<Event>>,
}

impl CollectingSink {
    fn events(&self) -> Vec<Event> {
        self.events.lock().clone()
    }
}

impl harness_plugin::PluginEventSink for CollectingSink {
    fn emit(&self, event: Event) {
        self.events.lock().push(event);
    }
}

struct CountingManifestLoader {
    records: Vec<ManifestRecord>,
    enumerate_count: AtomicUsize,
}

impl CountingManifestLoader {
    fn new(records: Vec<ManifestRecord>) -> Self {
        Self {
            records,
            enumerate_count: AtomicUsize::new(0),
        }
    }

    fn enumerate_count(&self) -> usize {
        self.enumerate_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl PluginManifestLoader for CountingManifestLoader {
    async fn enumerate(
        &self,
        _source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        self.enumerate_count.fetch_add(1, Ordering::SeqCst);
        Ok(self.records.clone())
    }
}

struct CountingRuntimeLoader {
    plugin: Arc<dyn Plugin>,
    load_count: AtomicUsize,
}

impl CountingRuntimeLoader {
    fn new(plugin: Arc<dyn Plugin>) -> Self {
        Self {
            plugin,
            load_count: AtomicUsize::new(0),
        }
    }

    fn load_count(&self) -> usize {
        self.load_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl PluginRuntimeLoader for CountingRuntimeLoader {
    fn can_load(&self, manifest: &PluginManifest, _origin: &ManifestOrigin) -> bool {
        self.plugin.manifest().plugin_id() == manifest.plugin_id()
    }

    async fn load(
        &self,
        manifest: &PluginManifest,
        origin: &ManifestOrigin,
    ) -> Result<Arc<dyn Plugin>, RuntimeLoaderError> {
        if !self.can_load(manifest, origin) {
            return Err(RuntimeLoaderError::UnsupportedOrigin(origin.to_string()));
        }
        self.load_count.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::clone(&self.plugin))
    }
}

struct MultiRuntimeLoader {
    plugins: Vec<Arc<dyn Plugin>>,
}

impl MultiRuntimeLoader {
    fn new(plugins: Vec<Arc<dyn Plugin>>) -> Self {
        Self { plugins }
    }
}

#[async_trait]
impl PluginRuntimeLoader for MultiRuntimeLoader {
    fn can_load(&self, manifest: &PluginManifest, _origin: &ManifestOrigin) -> bool {
        self.plugins
            .iter()
            .any(|plugin| plugin.manifest().plugin_id() == manifest.plugin_id())
    }

    async fn load(
        &self,
        manifest: &PluginManifest,
        origin: &ManifestOrigin,
    ) -> Result<Arc<dyn Plugin>, RuntimeLoaderError> {
        self.plugins
            .iter()
            .find(|plugin| plugin.manifest().plugin_id() == manifest.plugin_id())
            .cloned()
            .ok_or_else(|| RuntimeLoaderError::UnsupportedOrigin(origin.to_string()))
    }
}

struct NoopPlugin {
    manifest: PluginManifest,
}

impl NoopPlugin {
    fn new(record: ManifestRecord) -> Self {
        Self {
            manifest: record.manifest,
        }
    }
}

#[async_trait]
impl Plugin for NoopPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        _ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        Ok(PluginActivationResult::default())
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

struct CapturingPlugin {
    manifest: PluginManifest,
    captured: tokio::sync::Mutex<Option<PluginActivationContext>>,
}

impl CapturingPlugin {
    fn new(record: ManifestRecord) -> Self {
        Self {
            manifest: record.manifest,
            captured: tokio::sync::Mutex::new(None),
        }
    }

    fn captured_context(&self) -> Option<PluginActivationContext> {
        self.captured.try_lock().ok()?.clone()
    }
}

#[async_trait]
impl Plugin for CapturingPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        let coordinator = ctx.coordinator.clone();
        let has_tools = ctx.tools.is_some();
        *self.captured.lock().await = Some(ctx);
        if let Some(coordinator) = coordinator {
            coordinator
                .register(Arc::new(FakeCoordinatorStrategy))
                .await?;
            return Ok(PluginActivationResult {
                occupied_slots: vec![CapabilitySlot::CoordinatorStrategy],
                ..PluginActivationResult::default()
            });
        }
        if !has_tools {
            return Ok(PluginActivationResult::default());
        }
        Ok(PluginActivationResult {
            registered_tools: vec!["declared-tool".to_owned()],
            ..PluginActivationResult::default()
        })
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

struct FakeCoordinatorStrategy;

impl CoordinatorStrategy for FakeCoordinatorStrategy {}

struct FakeSteeringRegistration {
    id: SteeringId,
    request: Mutex<Option<SteeringRequest>>,
}

impl Default for FakeSteeringRegistration {
    fn default() -> Self {
        Self {
            id: SteeringId::new(),
            request: Mutex::new(None),
        }
    }
}

impl FakeSteeringRegistration {
    fn id(&self) -> SteeringId {
        self.id
    }

    fn request(&self) -> Option<SteeringRequest> {
        self.request.lock().clone()
    }
}

#[async_trait]
impl SteeringRegistration for FakeSteeringRegistration {
    async fn push(&self, request: SteeringRequest) -> Result<SteeringId, RegistrationError> {
        *self.request.lock() = Some(request);
        Ok(self.id())
    }
}

struct ResultPlugin {
    manifest: PluginManifest,
    result: PluginActivationResult,
}

impl ResultPlugin {
    fn new(record: ManifestRecord, result: PluginActivationResult) -> Self {
        Self {
            manifest: record.manifest,
            result,
        }
    }
}

struct RegisteringPlugin {
    manifest: PluginManifest,
}

struct MemoryRegisteringPlugin {
    manifest: PluginManifest,
    provider_id: String,
}

impl MemoryRegisteringPlugin {
    fn new(record: ManifestRecord, provider_id: &str) -> Self {
        Self {
            manifest: record.manifest,
            provider_id: provider_id.to_owned(),
        }
    }
}

impl RegisteringPlugin {
    fn new(record: ManifestRecord) -> Self {
        Self {
            manifest: record.manifest,
        }
    }
}

#[async_trait]
impl Plugin for RegisteringPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        ctx.tools
            .as_ref()
            .expect("tool handle")
            .register(Box::new(FakeTool::new("registered-tool")))
            .await?;
        ctx.hooks
            .as_ref()
            .expect("hook handle")
            .register(Box::new(FakeHook::new("registered-hook")))
            .await?;
        let mcp_id = ctx
            .mcp
            .as_ref()
            .expect("mcp handle")
            .register(mcp_spec("registered-mcp"))
            .await?;
        ctx.skills
            .as_ref()
            .expect("skill handle")
            .register(fake_skill("registered-skill"))
            .await?;
        ctx.memory
            .as_ref()
            .expect("memory handle")
            .register(Arc::new(FakeMemoryProvider::new("registered-memory")))
            .await?;

        Ok(PluginActivationResult {
            registered_tools: vec!["registered-tool".to_owned()],
            registered_hooks: vec!["registered-hook".to_owned()],
            registered_skills: vec!["registered-skill".to_owned()],
            registered_mcp: vec![mcp_id],
            occupied_slots: vec![CapabilitySlot::MemoryProvider],
        })
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

#[async_trait]
impl Plugin for MemoryRegisteringPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        ctx.memory
            .as_ref()
            .expect("memory handle")
            .register(Arc::new(FakeMemoryProvider::new(&self.provider_id)))
            .await?;

        Ok(PluginActivationResult {
            occupied_slots: vec![CapabilitySlot::MemoryProvider],
            ..PluginActivationResult::default()
        })
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

struct ReadyMcpPlugin {
    manifest: PluginManifest,
}

impl ReadyMcpPlugin {
    fn new(record: ManifestRecord) -> Self {
        Self {
            manifest: record.manifest,
        }
    }
}

#[async_trait]
impl Plugin for ReadyMcpPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        let mcp_id = ctx
            .mcp
            .as_ref()
            .expect("mcp handle")
            .register_ready(
                mcp_spec("registered-mcp"),
                Arc::new(StaticMcpConnection {
                    tools: vec![mcp_tool_descriptor("lookup")],
                }),
            )
            .await?;

        Ok(PluginActivationResult {
            registered_mcp: vec![mcp_id],
            ..PluginActivationResult::default()
        })
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

struct PartialRegisteringPlugin {
    manifest: PluginManifest,
}

impl PartialRegisteringPlugin {
    fn new(record: ManifestRecord) -> Self {
        Self {
            manifest: record.manifest,
        }
    }
}

#[async_trait]
impl Plugin for PartialRegisteringPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        ctx.tools
            .as_ref()
            .expect("tool handle")
            .register(Box::new(FakeTool::new("implemented-tool")))
            .await?;
        Ok(PluginActivationResult {
            registered_tools: vec!["implemented-tool".to_owned()],
            ..PluginActivationResult::default()
        })
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

struct DeactivateFailingPlugin {
    manifest: PluginManifest,
}

impl DeactivateFailingPlugin {
    fn new(record: ManifestRecord) -> Self {
        Self {
            manifest: record.manifest,
        }
    }
}

#[async_trait]
impl Plugin for DeactivateFailingPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        ctx.tools
            .as_ref()
            .expect("tool handle")
            .register(Box::new(FakeTool::new("cleanup-tool")))
            .await?;
        Ok(PluginActivationResult {
            registered_tools: vec!["cleanup-tool".to_owned()],
            ..PluginActivationResult::default()
        })
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Err(PluginError::DeactivateFailed("plugin-owned".to_owned()))
    }
}

struct DeactivateObservingPlugin {
    manifest: PluginManifest,
    tool_registry: ToolRegistry,
}

impl DeactivateObservingPlugin {
    fn new(record: ManifestRecord, tool_registry: ToolRegistry) -> Self {
        Self {
            manifest: record.manifest,
            tool_registry,
        }
    }
}

#[async_trait]
impl Plugin for DeactivateObservingPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        ctx.tools
            .as_ref()
            .expect("tool handle")
            .register(Box::new(FakeTool::new("ordered-tool")))
            .await?;
        Ok(PluginActivationResult {
            registered_tools: vec!["ordered-tool".to_owned()],
            ..PluginActivationResult::default()
        })
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        assert!(
            self.tool_registry.get("ordered-tool").is_none(),
            "registry-owned tool should be removed before plugin deactivate callback"
        );
        Ok(())
    }
}

struct RetryPlugin {
    manifest: PluginManifest,
    attempts: AtomicUsize,
}

impl RetryPlugin {
    fn new(record: ManifestRecord) -> Self {
        Self {
            manifest: record.manifest,
            attempts: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl Plugin for RetryPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        _ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(PluginError::ActivateFailed("first attempt".to_owned()));
        }
        Ok(PluginActivationResult::default())
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

struct SpecificToolPlugin {
    manifest: PluginManifest,
    tool_name: String,
}

impl SpecificToolPlugin {
    fn new(record: ManifestRecord, tool_name: String) -> Self {
        Self {
            manifest: record.manifest,
            tool_name,
        }
    }
}

#[async_trait]
impl Plugin for SpecificToolPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        ctx.tools
            .as_ref()
            .expect("tool handle")
            .register(Box::new(FakeTool::new(&self.tool_name)))
            .await?;
        Ok(PluginActivationResult {
            registered_tools: vec![self.tool_name.clone()],
            ..PluginActivationResult::default()
        })
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

#[async_trait]
impl Plugin for ResultPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        _ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        Ok(self.result.clone())
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

struct FakeTool {
    descriptor: ToolDescriptor,
}

impl FakeTool {
    fn new(name: &str) -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: name.to_owned(),
                display_name: name.to_owned(),
                description: "fake".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::Custom("test".to_owned()),
                version: "0.1.0".to_owned(),
                input_schema: json!({ "type": "object" }),
                output_schema: None,
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: true,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                trust_level: TrustLevel::UserControlled,
                required_capabilities: Vec::new(),
                budget: default_result_budget(),
                provider_restriction: ProviderRestriction::All,
                origin: ToolOrigin::Plugin {
                    plugin_id: plugin_id("declared-tool"),
                    trust: TrustLevel::UserControlled,
                },
                search_hint: None,
                service_binding: None,
            },
        }
    }

    fn destructive(name: &str) -> Self {
        let mut tool = Self::new(name);
        tool.descriptor.properties.is_read_only = false;
        tool.descriptor.properties.is_destructive = true;
        tool
    }

    fn builtin(name: &str) -> Self {
        let mut tool = Self::new(name);
        tool.descriptor.origin = ToolOrigin::Builtin;
        tool.descriptor.trust_level = TrustLevel::AdminTrusted;
        tool
    }
}

#[async_trait]
impl Tool for FakeTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn resolve_schema(
        &self,
        _ctx: &SchemaResolverContext,
    ) -> Result<Value, harness_contracts::ToolError> {
        Ok(self.descriptor.input_schema.clone())
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
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
        _ctx: ToolContext,
    ) -> Result<harness_tool::ToolStream, ToolError> {
        Ok(Box::pin(futures::stream::empty()))
    }
}

struct FakeHook {
    id: String,
}

impl FakeHook {
    fn new(id: &str) -> Self {
        Self { id: id.to_owned() }
    }
}

#[async_trait]
impl HookHandler for FakeHook {
    fn handler_id(&self) -> &str {
        &self.id
    }

    fn interested_events(&self) -> &[harness_contracts::HookEventKind] {
        &[harness_contracts::HookEventKind::UserPromptSubmit]
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        Ok(HookOutcome::Continue)
    }
}

struct FailClosedHook {
    id: String,
}

impl FailClosedHook {
    fn new(id: &str) -> Self {
        Self { id: id.to_owned() }
    }
}

#[async_trait]
impl HookHandler for FailClosedHook {
    fn handler_id(&self) -> &str {
        &self.id
    }

    fn interested_events(&self) -> &[harness_contracts::HookEventKind] {
        &[harness_contracts::HookEventKind::UserPromptSubmit]
    }

    fn failure_mode(&self) -> HookFailureMode {
        HookFailureMode::FailClosed
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        Ok(HookOutcome::Continue)
    }
}

struct ExecKindHook {
    id: String,
}

impl ExecKindHook {
    fn new(id: &str) -> Self {
        Self { id: id.to_owned() }
    }
}

#[async_trait]
impl HookHandler for ExecKindHook {
    fn handler_id(&self) -> &str {
        &self.id
    }

    fn interested_events(&self) -> &[harness_contracts::HookEventKind] {
        &[harness_contracts::HookEventKind::UserPromptSubmit]
    }

    fn registration_kind(&self) -> HookRegistrationKind {
        HookRegistrationKind::Exec
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        Ok(HookOutcome::Continue)
    }
}

struct HttpKindHook {
    id: String,
}

impl HttpKindHook {
    fn new(id: &str) -> Self {
        Self { id: id.to_owned() }
    }
}

#[async_trait]
impl HookHandler for HttpKindHook {
    fn handler_id(&self) -> &str {
        &self.id
    }

    fn interested_events(&self) -> &[harness_contracts::HookEventKind] {
        &[harness_contracts::HookEventKind::UserPromptSubmit]
    }

    fn registration_kind(&self) -> HookRegistrationKind {
        HookRegistrationKind::Http
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        Ok(HookOutcome::Continue)
    }
}

struct TrustDeclaringHook {
    id: String,
    trust: TrustLevel,
}

impl TrustDeclaringHook {
    fn new(id: &str, trust: TrustLevel) -> Self {
        Self {
            id: id.to_owned(),
            trust,
        }
    }
}

#[async_trait]
impl HookHandler for TrustDeclaringHook {
    fn handler_id(&self) -> &str {
        &self.id
    }

    fn interested_events(&self) -> &[harness_contracts::HookEventKind] {
        &[harness_contracts::HookEventKind::UserPromptSubmit]
    }

    fn declared_trust(&self) -> Option<TrustLevel> {
        Some(self.trust)
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        Ok(HookOutcome::Continue)
    }
}

struct FakeMemoryProvider {
    id: String,
}

impl FakeMemoryProvider {
    fn new(id: &str) -> Self {
        Self { id: id.to_owned() }
    }
}

#[derive(Default)]
struct CapabilityMetrics {
    records: Mutex<Vec<(String, String)>>,
}

impl CapabilityMetrics {
    fn records(&self) -> Vec<(String, String)> {
        self.records.lock().clone()
    }
}

impl PluginMetricsSink for CapabilityMetrics {
    fn plugin_capability_registration_rejected(&self, kind: &str, reason: &str) {
        self.records
            .lock()
            .push((kind.to_owned(), reason.to_owned()));
    }
}

#[async_trait]
impl MemoryStore for FakeMemoryProvider {
    fn provider_id(&self) -> &str {
        &self.id
    }

    async fn recall(&self, _query: MemoryQuery) -> Result<Vec<MemoryRecord>, MemoryError> {
        Ok(Vec::new())
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _id: MemoryId) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn list(&self, _scope: MemoryListScope) -> Result<Vec<MemorySummary>, MemoryError> {
        Ok(Vec::new())
    }
}

impl MemoryLifecycle for FakeMemoryProvider {}

impl harness_memory::MemoryProvider for FakeMemoryProvider {}

fn mcp_spec(id: &str) -> McpServerSpec {
    McpServerSpec::new(
        McpServerId(id.to_owned()),
        id,
        TransportChoice::InProcess,
        McpServerSource::Plugin(plugin_id("declared-tool")),
    )
}

fn mcp_tool_descriptor(name: &str) -> McpToolDescriptor {
    McpToolDescriptor {
        name: name.to_owned(),
        description: Some(format!("{name} tool")),
        input_schema: json!({ "type": "object" }),
        output_schema: None,
        annotations: None,
        meta: BTreeMap::new(),
    }
}

struct StaticMcpConnection {
    tools: Vec<McpToolDescriptor>,
}

#[async_trait]
impl McpConnection for StaticMcpConnection {
    fn connection_id(&self) -> &str {
        "static-plugin-mcp"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        Ok(self.tools.clone())
    }

    async fn call_tool(&self, _name: &str, _args: Value) -> Result<McpToolResult, McpError> {
        Ok(McpToolResult::text("ok"))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        Ok(())
    }
}

fn fake_skill(name: &str) -> Skill {
    Skill {
        id: harness_contracts::SkillId(format!("skill:{name}")),
        name: name.to_owned(),
        description: "fake skill".to_owned(),
        source: SkillSource::Plugin {
            plugin_id: plugin_id("declared-tool"),
            trust: TrustLevel::UserControlled,
        },
        frontmatter: SkillFrontmatter {
            name: name.to_owned(),
            description: "fake skill".to_owned(),
            allowlist_agents: None,
            parameters: Vec::new(),
            config: Vec::new(),
            platforms: Vec::new(),
            prerequisites: SkillPrerequisites::default(),
            hooks: Vec::new(),
            tags: Vec::new(),
            category: None,
            metadata: HashMap::default(),
        },
        body: String::new(),
        raw_path: None,
    }
}

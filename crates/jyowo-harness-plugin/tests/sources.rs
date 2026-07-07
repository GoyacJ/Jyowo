use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{
    AuthorizationTicketId, CapabilityRegistry, CausationId, CorrelationId, InteractivityLevel,
    ManifestValidationFailure as EventManifestValidationFailure, McpServerId, McpServerSource,
    PermissionMode, PluginId, RedactRules, RejectionReason, RunId, SessionId, TenantId,
    ToolActionPlan, ToolError, ToolResult, ToolUseId, TrustLevel,
};
use harness_hook::{
    HookContext, HookDispatcher, HookEvent, HookMessageView, HookOutcome, HookRegistry,
    HookSessionView, ReplayMode, ToolDescriptorView,
};
use harness_mcp::McpRegistry;
use harness_plugin::{
    CargoExtensionManifestLoader, CargoExtensionRuntimeLoader, DiscoverySource, FileManifestLoader,
    ManifestLoaderError, ManifestOrigin, ManifestSigner, Plugin, PluginActivationContext,
    PluginActivationResult, PluginConfig, PluginError, PluginManifest, PluginManifestLoader,
    PluginRegistry, PluginRuntimeLoader, StaticLinkRuntimeLoader,
};
use harness_skill::{SkillRegistry, SkillSource};
use harness_tool::{
    AuthorizedTicketSummary, AuthorizedToolInput, InterruptToken, Tool, ToolContext, ToolEvent,
    ToolRegistry, ToolStream,
};
use ring::digest;

#[tokio::test]
async fn workspace_source_scans_admin_plugin_json() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(
        &canonical_temp_root(&root).join("admin-a/plugin.json"),
        manifest_json("admin-a", TrustLevel::AdminTrusted),
    );

    let records = FileManifestLoader
        .enumerate(&DiscoverySource::Workspace(canonical_temp_root(&root)))
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].manifest.plugin_id().0, "admin-a@0.1.0");
    assert_eq!(records[0].manifest.trust_level, TrustLevel::AdminTrusted);
    assert!(matches!(records[0].origin, ManifestOrigin::File { .. }));
}

#[tokio::test]
async fn user_source_scans_user_controlled_plugin_json() {
    let home = tempfile::tempdir().unwrap();
    write_manifest(
        &canonical_temp_root(&home).join("user-a/plugin.json"),
        manifest_json("user-a", TrustLevel::UserControlled),
    );

    let records = FileManifestLoader
        .enumerate(&DiscoverySource::User(canonical_temp_root(&home)))
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].manifest.plugin_id().0, "user-a@0.1.0");
    assert_eq!(records[0].manifest.trust_level, TrustLevel::UserControlled);
}

#[tokio::test]
async fn project_source_scans_project_plugin_json() {
    let project = tempfile::tempdir().unwrap();
    write_manifest(
        &canonical_temp_root(&project).join("project-a/plugin.json"),
        manifest_json("project-a", TrustLevel::UserControlled),
    );

    let records = FileManifestLoader
        .enumerate(&DiscoverySource::Project(canonical_temp_root(&project)))
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].manifest.plugin_id().0, "project-a@0.1.0");
    assert_eq!(records[0].manifest.trust_level, TrustLevel::UserControlled);
}

#[tokio::test]
async fn yaml_manifest_is_parsed_through_file_loader() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(
        &canonical_temp_root(&root).join("admin-yaml/plugin.yaml"),
        r#"
manifest_schema_version: 1
name: admin-yaml
version: 0.1.0
trust_level: admin_trusted
min_harness_version: ">=0.0.0"
capabilities:
  tools:
    - name: yaml-tool
      destructive: false
"#,
    );

    let records = FileManifestLoader
        .enumerate(&DiscoverySource::Workspace(canonical_temp_root(&root)))
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].manifest.plugin_id().0, "admin-yaml@0.1.0");
    assert_eq!(records[0].manifest.capabilities.tools[0].name, "yaml-tool");
}

#[tokio::test]
async fn source_trust_mismatch_is_rejected() {
    let home = tempfile::tempdir().unwrap();
    write_manifest(
        &canonical_temp_root(&home).join("bad-trust/plugin.json"),
        manifest_json("bad-trust", TrustLevel::AdminTrusted),
    );

    let records = FileManifestLoader
        .enumerate(&DiscoverySource::User(canonical_temp_root(&home)))
        .await
        .unwrap();

    assert_eq!(records.len(), 1);

    let registry = PluginRegistry::builder()
        .with_source(DiscoverySource::User(canonical_temp_root(&home)))
        .build()
        .unwrap();
    let discovered = registry.discover().await.unwrap();

    assert!(discovered.is_empty());
    assert!(matches!(
        registry
            .state_detail(&PluginId("bad-trust@0.1.0".to_owned()))
            .and_then(|detail| detail.rejection_reason),
        Some(RejectionReason::TrustMismatch { .. })
    ));
}

#[tokio::test]
async fn file_loader_records_canonical_manifest_hash() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(
        &canonical_temp_root(&root).join("data/plugins/hash-a/plugin.json"),
        manifest_json("hash-a", TrustLevel::AdminTrusted),
    );

    let records = FileManifestLoader
        .enumerate(&DiscoverySource::Workspace(canonical_temp_root(&root)))
        .await
        .unwrap();
    let canonical = ManifestSigner::canonical_payload(&records[0].manifest).unwrap();
    let expected = digest::digest(&digest::SHA256, &canonical);
    let expected_hash: [u8; 32] = expected.as_ref().try_into().unwrap();

    assert_eq!(records[0].manifest_hash, expected_hash);
}

#[cfg(unix)]
#[tokio::test]
async fn file_loader_rejects_symlink_plugin_directory() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write_manifest(
        &canonical_temp_root(&outside).join("outside-plugin/plugin.json"),
        manifest_json("outside-plugin", TrustLevel::AdminTrusted),
    );
    std::os::unix::fs::symlink(
        canonical_temp_root(&outside).join("outside-plugin"),
        canonical_temp_root(&root).join("linked-plugin"),
    )
    .unwrap();

    let error = FileManifestLoader
        .enumerate(&DiscoverySource::Workspace(canonical_temp_root(&root)))
        .await
        .unwrap_err();

    assert!(matches!(error, ManifestLoaderError::Io(message) if message.contains("symlink")));
}

#[cfg(unix)]
#[tokio::test]
async fn file_loader_rejects_symlink_manifest_file() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write_manifest(
        &canonical_temp_root(&outside).join("plugin.json"),
        manifest_json("linked-manifest", TrustLevel::AdminTrusted),
    );
    std::fs::create_dir_all(canonical_temp_root(&root).join("linked-manifest")).unwrap();
    std::os::unix::fs::symlink(
        canonical_temp_root(&outside).join("plugin.json"),
        canonical_temp_root(&root).join("linked-manifest/plugin.json"),
    )
    .unwrap();

    let error = FileManifestLoader
        .enumerate(&DiscoverySource::Workspace(canonical_temp_root(&root)))
        .await
        .unwrap_err();

    assert!(matches!(error, ManifestLoaderError::Io(message) if message.contains("symlink")));
}

#[cfg(unix)]
#[tokio::test]
async fn file_loader_rejects_world_writable_plugin_directory_ancestor() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let parent = canonical_temp_root(&root).join("writable-parent");
    let plugin = parent.join("plugin");
    write_manifest(
        &plugin.join("plugin.json"),
        manifest_json("world-writable-ancestor", TrustLevel::AdminTrusted),
    );
    let mut permissions = std::fs::metadata(&parent).unwrap().permissions();
    permissions.set_mode(0o777);
    std::fs::set_permissions(&parent, permissions).unwrap();

    let error = FileManifestLoader
        .load_package_report(&plugin)
        .await
        .unwrap_err();

    assert!(
        matches!(error, ManifestLoaderError::Io(message) if message.contains("world-writable"))
    );
}

#[tokio::test]
async fn malformed_manifest_returns_validation_error() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(
        &canonical_temp_root(&root).join("bad/plugin.json"),
        "{ this is not json",
    );

    let error = FileManifestLoader
        .enumerate(&DiscoverySource::Workspace(canonical_temp_root(&root)))
        .await
        .unwrap_err();

    let ManifestLoaderError::Validation(failure) = error else {
        panic!("expected manifest validation failure");
    };
    assert!(matches!(
        failure.failure,
        EventManifestValidationFailure::SyntaxError { .. }
    ));
}

#[tokio::test]
async fn unknown_manifest_fields_are_rejected_as_schema_violations() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(
        &canonical_temp_root(&root).join("unknown-field/plugin.json"),
        r#"{
  "manifest_schema_version": 1,
  "name": "unknown-field",
  "version": "0.1.0",
  "trust_level": "admin_trusted",
  "min_harness_version": ">=0.0.0",
  "capabilities": {},
  "extra": true
}"#,
    );

    let error = FileManifestLoader
        .enumerate(&DiscoverySource::Workspace(canonical_temp_root(&root)))
        .await
        .unwrap_err();

    let ManifestLoaderError::Validation(failure) = error else {
        panic!("expected manifest validation failure");
    };
    assert!(matches!(
        failure.failure,
        EventManifestValidationFailure::SchemaViolation { .. }
    ));
}

#[tokio::test]
async fn unsupported_manifest_schema_version_is_typed() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(
        &canonical_temp_root(&root).join("future-schema/plugin.json"),
        r#"{
  "manifest_schema_version": 99,
  "name": "future-schema",
  "version": "0.1.0",
  "trust_level": "admin_trusted",
  "min_harness_version": ">=0.0.0",
  "capabilities": {}
}"#,
    );

    let error = FileManifestLoader
        .enumerate(&DiscoverySource::Workspace(canonical_temp_root(&root)))
        .await
        .unwrap_err();

    let ManifestLoaderError::Validation(failure) = error else {
        panic!("expected manifest validation failure");
    };
    assert!(matches!(
        failure.failure,
        EventManifestValidationFailure::UnsupportedSchemaVersion { found: 99, .. }
    ));
}

#[tokio::test]
async fn default_builder_discovers_file_manifests_without_custom_loader() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(
        &canonical_temp_root(&root).join("user-default/plugin.json"),
        manifest_json("user-default", TrustLevel::UserControlled),
    );
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            allow_project_plugins: true,
            ..PluginConfig::default()
        })
        .with_source(DiscoverySource::Project(canonical_temp_root(&root)))
        .build()
        .unwrap();

    let discovered = registry.discover().await.unwrap();

    assert_eq!(discovered.len(), 1);
    assert_eq!(
        discovered[0].record.manifest.plugin_id().0,
        "user-default@0.1.0"
    );
}

#[tokio::test]
async fn cargo_extension_manifest_loader_discovers_metadata_from_path_binary() {
    let root = tempfile::tempdir().unwrap();
    let binary = canonical_temp_root(&root).join("jyowo-plugin-cargo-a");
    let metadata = serde_json::json!({
        "manifest": serde_json::from_str::<serde_json::Value>(&manifest_json("cargo-a", TrustLevel::AdminTrusted)).unwrap(),
        "package_metadata": { "package": "cargo-a" }
    });
    write_executable(
        &binary,
        &format!(
            r#"#!/bin/sh
if [ "$1" = "--harness-manifest" ]; then
printf '%s' '{}'
exit 0
fi
exit 2
"#,
            metadata
        ),
    );

    let records = CargoExtensionManifestLoader::new()
        .with_search_paths(vec![canonical_temp_root(&root).as_path().to_path_buf()])
        .with_timeout(Duration::from_secs(15))
        .enumerate(&DiscoverySource::CargoExtension)
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].manifest.plugin_id().0, "cargo-a@0.1.0");
    let canonical_binary = binary.canonicalize().unwrap();
    assert!(
        matches!(&records[0].origin, ManifestOrigin::CargoExtension { binary: found, package_metadata } if found == &canonical_binary && package_metadata.contains_key("package"))
    );
}

#[tokio::test]
async fn cargo_extension_manifest_loader_supports_runtime_metadata_rpc() {
    let root = tempfile::tempdir().unwrap();
    let binary = canonical_temp_root(&root).join("jyowo-plugin-cargo-rpc");
    let metadata = serde_json::json!({
        "manifest": serde_json::from_str::<serde_json::Value>(&manifest_json("cargo-rpc", TrustLevel::AdminTrusted)).unwrap(),
        "package_metadata": { "package": "cargo-rpc" }
    });
    write_executable(
        &binary,
        &format!(
            r#"#!/bin/sh
if [ "$1" = "--harness-runtime" ]; then
request=$(cat)
case "$request" in
  *\"method\":\"metadata\"*)
    printf '%s' '{{"jsonrpc":"2.0","id":1,"result":{metadata}}}'
    exit 0
    ;;
esac
fi
exit 2
"#,
        ),
    );

    let records = CargoExtensionManifestLoader::new()
        .with_search_paths(vec![canonical_temp_root(&root).as_path().to_path_buf()])
        .with_timeout(Duration::from_secs(15))
        .enumerate(&DiscoverySource::CargoExtension)
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].manifest.plugin_id().0, "cargo-rpc@0.1.0");
    let canonical_binary = binary.canonicalize().unwrap();
    assert!(
        matches!(&records[0].origin, ManifestOrigin::CargoExtension { binary: found, package_metadata } if found == &canonical_binary && package_metadata.contains_key("package"))
    );
}

#[tokio::test]
async fn cargo_extension_manifest_loader_ignores_non_cargo_sources() {
    let records = CargoExtensionManifestLoader::new()
        .enumerate(&DiscoverySource::Inline)
        .await
        .unwrap();

    assert!(records.is_empty());
}

#[tokio::test]
async fn cargo_extension_manifest_loader_reports_malformed_metadata() {
    let root = tempfile::tempdir().unwrap();
    write_executable(
        &canonical_temp_root(&root).join("jyowo-plugin-bad"),
        r#"#!/bin/sh
if [ "$1" = "--harness-manifest" ]; then
printf 'not-json'
exit 0
fi
exit 2
"#,
    );

    let report = CargoExtensionManifestLoader::new()
        .with_search_paths(vec![canonical_temp_root(&root).as_path().to_path_buf()])
        .load_report(&DiscoverySource::CargoExtension)
        .await
        .unwrap();

    assert!(report.records.is_empty());
    assert_eq!(report.failures.len(), 1);
    assert!(matches!(
        report.failures[0].failure,
        EventManifestValidationFailure::CargoExtensionMetadataMalformed { .. }
    ));
}

#[tokio::test]
async fn cargo_extension_runtime_loader_proxies_activation_and_deactivation() {
    let root = tempfile::tempdir().unwrap();
    let binary = canonical_temp_root(&root).join("jyowo-plugin-cargo-a");
    write_executable(
        &binary,
        r#"#!/bin/sh
if [ "$1" = "--harness-runtime" ]; then
IFS= read -r _request
printf '{"jsonrpc":"2.0","id":1,"result":{"registered_tools":["cargo-tool"],"registered_hooks":[],"registered_skills":[],"registered_mcp":[],"occupied_slots":[]}}'
exit 0
fi
exit 2
"#,
    );
    let manifest = manifest("cargo-a", TrustLevel::AdminTrusted);
    let origin = ManifestOrigin::CargoExtension {
        binary,
        package_metadata: BTreeMap::new(),
    };

    assert!(CargoExtensionRuntimeLoader::new().can_load(&manifest, &origin));

    let plugin = CargoExtensionRuntimeLoader::new()
        .with_timeout(Duration::from_secs(15))
        .load(&manifest, &origin)
        .await
        .unwrap();
    let result = plugin
        .activate(PluginActivationContext::manifest_only(&manifest))
        .await
        .unwrap();
    plugin.deactivate().await.unwrap();

    assert_eq!(result.registered_tools, vec!["cargo-tool".to_owned()]);
}

#[tokio::test]
async fn cargo_extension_runtime_registers_manifest_tool_proxy_and_executes_it() {
    let root = tempfile::tempdir().unwrap();
    let binary = canonical_temp_root(&root).join("jyowo-plugin-tool-proxy");
    let manifest = manifest_with_tool(
        "tool-proxy",
        "sidecar-tool",
        false,
        TrustLevel::UserControlled,
    );
    let metadata = serde_json::json!({
        "manifest": serde_json::to_value(&manifest).unwrap(),
        "package_metadata": { "package": "tool-proxy" }
    });
    write_manifest(&binary.with_extension("metadata"), metadata.to_string());
    write_executable(
        &binary,
        r#"#!/bin/sh
if [ "$1" = "--harness-manifest" ]; then
cat "$0.metadata"
exit 0
fi
if [ "$1" = "--harness-runtime" ]; then
request=$(cat)
case "$request" in
  *\"method\":\"activate\"*)
    printf '{"jsonrpc":"2.0","id":1,"result":{"registered_tools":[],"registered_hooks":[],"registered_skills":[],"registered_mcp":[],"occupied_slots":[]}}'
    exit 0
    ;;
  *\"method\":\"tool.execute\"*)
    printf '{"jsonrpc":"2.0","id":1,"result":{"text":"sidecar ok"}}'
    exit 0
    ;;
  *\"method\":\"deactivate\"*)
    printf '{"jsonrpc":"2.0","id":1,"result":null}'
    exit 0
    ;;
esac
fi
exit 2
"#,
    );
    let tools = ToolRegistry::builder().build().unwrap();
    let registry = PluginRegistry::builder()
        .with_source(DiscoverySource::CargoExtension)
        .with_manifest_loader(Arc::new(
            CargoExtensionManifestLoader::new()
                .with_search_paths(vec![canonical_temp_root(&root).as_path().to_path_buf()])
                .with_timeout(Duration::from_secs(15)),
        ))
        .with_runtime_loader(Arc::new(
            CargoExtensionRuntimeLoader::new().with_timeout(Duration::from_secs(15)),
        ))
        .with_capability_registries(
            harness_plugin::PluginCapabilityRegistries::default().with_tool_registry(tools.clone()),
        )
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry.activate(&manifest.plugin_id()).await.unwrap();

    let tool = tools.get("sidecar-tool").expect("tool proxy registered");
    let input = serde_json::json!({"input": "hello"});
    let ctx = tool_ctx();
    let plan = tool.plan(&input, &ctx).await.unwrap();
    assert!(matches!(
        plan.subject,
        harness_contracts::PermissionSubject::ToolInvocation { ref tool, .. }
            if tool == "sidecar-tool"
    ));
    assert!(matches!(
        plan.scope,
        harness_contracts::DecisionScope::ToolName(ref scope) if scope == "sidecar-tool"
    ));
    tool.validate(&serde_json::json!("not an object"), &tool_ctx())
        .await
        .expect_err("sidecar proxy must enforce the manifest input schema");
    let mut stream = execute_authorized_tool(tool.as_ref(), input, ctx)
        .await
        .unwrap();
    let Some(ToolEvent::Final(ToolResult::Text(text))) = stream.next().await else {
        panic!("expected final text result from sidecar proxy");
    };
    assert_eq!(text, "sidecar ok");

    registry.deactivate(&manifest.plugin_id()).await.unwrap();
    assert!(tools.get("sidecar-tool").is_none());
}

#[tokio::test]
async fn cargo_extension_tool_proxy_uses_manifest_input_schema() {
    let root = tempfile::tempdir().unwrap();
    let binary = canonical_temp_root(&root).join("jyowo-plugin-tool-schema");
    let manifest = manifest_with_tool_schema(
        "tool-schema",
        "sidecar-tool",
        serde_json::json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": { "type": "string" }
            },
            "additionalProperties": false
        }),
    );
    let metadata = serde_json::json!({
        "manifest": serde_json::to_value(&manifest).unwrap(),
        "package_metadata": { "package": "tool-schema" }
    });
    write_manifest(&binary.with_extension("metadata"), metadata.to_string());
    write_executable(
        &binary,
        r#"#!/bin/sh
if [ "$1" = "--harness-manifest" ]; then
cat "$0.metadata"
exit 0
fi
if [ "$1" = "--harness-runtime" ]; then
request=$(cat)
case "$request" in
  *\"method\":\"activate\"*)
    printf '{"jsonrpc":"2.0","id":1,"result":{"registered_tools":[],"registered_hooks":[],"registered_skills":[],"registered_mcp":[],"occupied_slots":[]}}'
    exit 0
    ;;
  *\"method\":\"deactivate\"*)
    printf '{"jsonrpc":"2.0","id":1,"result":null}'
    exit 0
    ;;
esac
fi
exit 2
"#,
    );
    let tools = ToolRegistry::builder().build().unwrap();
    let registry = PluginRegistry::builder()
        .with_source(DiscoverySource::CargoExtension)
        .with_manifest_loader(Arc::new(
            CargoExtensionManifestLoader::new()
                .with_search_paths(vec![canonical_temp_root(&root).as_path().to_path_buf()])
                .with_timeout(Duration::from_secs(15)),
        ))
        .with_runtime_loader(Arc::new(
            CargoExtensionRuntimeLoader::new().with_timeout(Duration::from_secs(15)),
        ))
        .with_capability_registries(
            harness_plugin::PluginCapabilityRegistries::default().with_tool_registry(tools.clone()),
        )
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry.activate(&manifest.plugin_id()).await.unwrap();

    let tool = tools.get("sidecar-tool").expect("tool proxy registered");
    tool.validate(&serde_json::json!({}), &tool_ctx())
        .await
        .expect_err("manifest schema should require path");
    tool.validate(&serde_json::json!({"path": "README.md"}), &tool_ctx())
        .await
        .expect("manifest schema should accept a valid payload");
}

#[tokio::test]
async fn cargo_extension_runtime_registers_hook_skill_and_mcp_proxies() {
    let root = tempfile::tempdir().unwrap();
    let binary = canonical_temp_root(&root).join("jyowo-plugin-capability-proxy");
    let manifest = manifest_with_proxy_capabilities("capability-proxy", TrustLevel::UserControlled);
    let metadata = serde_json::json!({
        "manifest": serde_json::to_value(&manifest).unwrap(),
        "package_metadata": { "package": "capability-proxy" }
    });
    write_manifest(&binary.with_extension("metadata"), metadata.to_string());
    write_executable(
        &binary,
        r#"#!/bin/sh
if [ "$1" = "--harness-manifest" ]; then
cat "$0.metadata"
exit 0
fi
if [ "$1" = "--harness-runtime" ]; then
request=$(cat)
case "$request" in
  *\"method\":\"activate\"*)
    printf '{"jsonrpc":"2.0","id":1,"result":{"registered_tools":[],"registered_hooks":[],"registered_skills":[],"registered_mcp":[],"occupied_slots":[]}}'
    exit 0
    ;;
  *\"method\":\"skill.read\"*)
    printf '%s' '{"jsonrpc":"2.0","id":1,"result":{"markdown":"---\nname: sidecar-skill\ndescription: Sidecar skill\n---\nUse sidecar skill."}}'
    exit 0
    ;;
  *\"method\":\"hook.handle\"*)
    printf '{"jsonrpc":"2.0","id":1,"result":{"type":"continue"}}'
    exit 0
    ;;
  *\"method\":\"mcp.list_tools\"*)
    printf '{"jsonrpc":"2.0","id":1,"result":[{"name":"echo","description":"Echo","inputSchema":{"type":"object"},"outputSchema":null}]}'
    exit 0
    ;;
  *\"method\":\"mcp.tool.call\"*)
    printf '{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"mcp ok"}],"isError":false}}'
    exit 0
    ;;
  *\"method\":\"deactivate\"*)
    printf '{"jsonrpc":"2.0","id":1,"result":null}'
    exit 0
    ;;
esac
fi
exit 2
"#,
    );
    let hooks = HookRegistry::builder().build().unwrap();
    let skills = SkillRegistry::builder().build();
    let mcp = McpRegistry::new();
    let tools = ToolRegistry::builder().build().unwrap();
    let registry = PluginRegistry::builder()
        .with_source(DiscoverySource::CargoExtension)
        .with_manifest_loader(Arc::new(
            CargoExtensionManifestLoader::new()
                .with_search_paths(vec![canonical_temp_root(&root).as_path().to_path_buf()])
                .with_timeout(Duration::from_secs(15)),
        ))
        .with_runtime_loader(Arc::new(
            CargoExtensionRuntimeLoader::new().with_timeout(Duration::from_secs(15)),
        ))
        .with_capability_registries(
            harness_plugin::PluginCapabilityRegistries::default()
                .with_hook_registry(hooks.clone())
                .with_skill_registry(skills.clone())
                .with_mcp_registry(mcp.clone()),
        )
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    registry.activate(&manifest.plugin_id()).await.unwrap();

    let unrelated_hook_result = HookDispatcher::new(hooks.snapshot())
        .dispatch(
            HookEvent::PreToolUse {
                tool_use_id: ToolUseId::new(),
                tool_name: "bash".to_owned(),
                input: serde_json::json!({}),
            },
            hook_ctx(),
        )
        .await
        .unwrap();
    assert!(
        unrelated_hook_result.trail.is_empty(),
        "sidecar hook must not receive undeclared hook events"
    );

    let hook_result = HookDispatcher::new(hooks.snapshot())
        .dispatch(
            HookEvent::SessionStart {
                session_id: SessionId::new(),
            },
            hook_ctx(),
        )
        .await
        .unwrap();
    assert_eq!(hook_result.trail.len(), 1);
    assert_eq!(hook_result.trail[0].handler_id, "sidecar-hook");
    assert_eq!(hook_result.final_outcome, HookOutcome::Continue);

    let skill = skills
        .get("sidecar-skill")
        .expect("sidecar skill proxy registered");
    assert!(matches!(
        &skill.source,
        SkillSource::Plugin { plugin_id, trust }
            if plugin_id == &manifest.plugin_id() && *trust == TrustLevel::UserControlled
    ));

    let server_id = McpServerId("sidecar-mcp".to_owned());
    let spec = mcp
        .server_spec(&server_id)
        .await
        .expect("sidecar MCP proxy registered");
    assert!(matches!(
        spec.source,
        McpServerSource::Plugin(ref plugin_id) if plugin_id == &manifest.plugin_id()
    ));
    let injected = mcp.inject_tools_into(&tools, &server_id).await.unwrap();
    assert_eq!(injected.len(), 1);
    let mcp_tool = tools.get(&injected[0]).expect("MCP tool was injected");
    let mut stream = execute_authorized_tool(mcp_tool.as_ref(), serde_json::json!({}), tool_ctx())
        .await
        .unwrap();
    let Some(ToolEvent::Final(ToolResult::Text(text))) = stream.next().await else {
        panic!("expected final text result from sidecar MCP proxy");
    };
    assert_eq!(text, "mcp ok");

    registry.deactivate(&manifest.plugin_id()).await.unwrap();
    assert!(hooks.origin_for("sidecar-hook").is_none());
    assert!(skills.get("sidecar-skill").is_none());
    assert!(mcp.server_spec(&server_id).await.is_none());
}

#[tokio::test]
async fn cargo_extension_runtime_activate_failure_marks_plugin_failed() {
    let root = tempfile::tempdir().unwrap();
    let binary = canonical_temp_root(&root).join("jyowo-plugin-failing");
    let manifest = manifest("failing", TrustLevel::UserControlled);
    let metadata = serde_json::json!({
        "manifest": serde_json::to_value(&manifest).unwrap(),
        "package_metadata": { "package": "failing" }
    });
    write_manifest(&binary.with_extension("metadata"), metadata.to_string());
    write_executable(
        &binary,
        r#"#!/bin/sh
if [ "$1" = "--harness-manifest" ]; then
cat "$0.metadata"
exit 0
fi
if [ "$1" = "--harness-runtime" ]; then
request=$(cat)
case "$request" in
  *\"method\":\"activate\"*)
    printf '{"jsonrpc":"2.0","id":1,"error":{"code":500,"message":"activate exploded with token=plugin-secret-token at /Users/goya/private"}}'
    exit 0
    ;;
esac
fi
exit 2
"#,
    );
    let registry = PluginRegistry::builder()
        .with_source(DiscoverySource::CargoExtension)
        .with_manifest_loader(Arc::new(
            CargoExtensionManifestLoader::new()
                .with_search_paths(vec![canonical_temp_root(&root).as_path().to_path_buf()])
                .with_timeout(Duration::from_secs(15)),
        ))
        .with_runtime_loader(Arc::new(
            CargoExtensionRuntimeLoader::new().with_timeout(Duration::from_secs(15)),
        ))
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    let error = registry.activate(&manifest.plugin_id()).await.unwrap_err();

    assert!(
        matches!(error, PluginError::ActivateFailed(message) if message.contains("activate exploded"))
    );
    assert!(matches!(
        registry.state(&manifest.plugin_id()),
        Some(harness_plugin::PluginLifecycleState::Failed(message)) if message.contains("activate exploded")
    ));
    let detail = registry
        .product_detail(&manifest.plugin_id())
        .expect("failed plugin detail exists");
    assert_eq!(
        detail.manifest_origin,
        harness_contracts::ManifestOriginRef::CargoExtension {
            binary: "<cargo-extension>".to_owned()
        }
    );
    let failure = detail.failure.expect("product failure summary exists");
    assert_eq!(failure, "Plugin failure details withheld.");
    assert!(!failure.contains("plugin-secret-token"));
    assert!(!failure.contains("/Users/goya"));
    assert!(!failure.contains("activate exploded"));
}

#[tokio::test]
async fn cargo_extension_runtime_loader_rejects_file_origin() {
    let manifest = manifest("cargo-a", TrustLevel::AdminTrusted);

    assert!(!CargoExtensionRuntimeLoader::new().can_load(
        &manifest,
        &ManifestOrigin::File {
            path: "/tmp/plugin.json".into()
        }
    ));
}

#[tokio::test]
async fn static_link_runtime_loader_loads_only_during_activate() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(
        &canonical_temp_root(&root).join("static-a/plugin.json"),
        manifest_json("static-a", TrustLevel::UserControlled),
    );
    let load_count = Arc::new(AtomicUsize::new(0));
    let manifest = manifest("static-a", TrustLevel::UserControlled);
    let plugin: Arc<dyn Plugin> = Arc::new(NoopPlugin { manifest });
    let runtime = StaticLinkRuntimeLoader::default().with_factory(
        PluginId("static-a@0.1.0".to_owned()),
        counting_factory(Arc::clone(&load_count), plugin),
    );
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            allow_project_plugins: true,
            ..PluginConfig::default()
        })
        .with_source(DiscoverySource::Project(canonical_temp_root(&root)))
        .with_runtime_loader(Arc::new(runtime))
        .build()
        .unwrap();

    registry.discover().await.unwrap();
    assert_eq!(load_count.load(Ordering::SeqCst), 0);

    registry
        .activate(&PluginId("static-a@0.1.0".to_owned()))
        .await
        .unwrap();
    assert_eq!(load_count.load(Ordering::SeqCst), 1);
}

#[cfg(feature = "dynamic-load")]
#[tokio::test]
async fn dylib_runtime_loader_is_explicitly_unsupported_without_unsafe() {
    use harness_plugin::{DylibRuntimeLoader, PluginRuntimeLoader, RuntimeLoaderError};

    let manifest = manifest("dylib-a", TrustLevel::AdminTrusted);
    let result = DylibRuntimeLoader
        .load(
            &manifest,
            &ManifestOrigin::File {
                path: "/tmp/plugin.dylib".into(),
            },
        )
        .await;
    let Err(error) = result else {
        panic!("dylib loading must be unsupported");
    };

    assert!(
        matches!(error, RuntimeLoaderError::LoadFailed(message) if message.contains("unsupported"))
    );
}

#[cfg(feature = "wasm-runtime")]
#[tokio::test]
async fn wasm_runtime_loader_is_explicitly_unsupported_without_runtime() {
    use harness_plugin::{PluginRuntimeLoader, RuntimeLoaderError, WasmRuntimeLoader};

    let manifest = manifest("wasm-a", TrustLevel::AdminTrusted);
    let result = WasmRuntimeLoader
        .load(
            &manifest,
            &ManifestOrigin::File {
                path: "/tmp/plugin.wasm".into(),
            },
        )
        .await;
    let Err(error) = result else {
        panic!("wasm loading must be unsupported");
    };

    assert!(
        matches!(error, RuntimeLoaderError::LoadFailed(message) if message.contains("unsupported"))
    );
}

#[tokio::test]
async fn manifest_schema_accepts_explicit_custom_toolsets() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(
        &canonical_temp_root(&root).join("toolset/plugin.json"),
        r#"{
  "manifest_schema_version": 1,
  "name": "toolset",
  "version": "0.1.0",
  "trust_level": "admin_trusted",
  "min_harness_version": ">=0.0.0",
  "capabilities": {
    "custom_toolsets": [
      { "name": "default" }
    ]
  }
}"#,
    );

    let records = FileManifestLoader
        .enumerate(&DiscoverySource::Workspace(canonical_temp_root(&root)))
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].manifest.capabilities.custom_toolsets[0].name,
        "default"
    );
}

#[tokio::test]
async fn file_manifest_hash_uses_canonical_payload_not_raw_bytes() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(
        &canonical_temp_root(&root).join("hash-a/plugin.json"),
        r#"{
  "manifest_schema_version": 1,
  "name": "hash-plugin",
  "version": "0.1.0",
  "trust_level": "admin_trusted",
  "min_harness_version": ">=0.0.0",
  "capabilities": {}
}"#,
    );
    write_manifest(
        &canonical_temp_root(&root).join("hash-b/plugin.json"),
        r#"{"capabilities":{},"min_harness_version":">=0.0.0","trust_level":"admin_trusted","version":"0.1.0","name":"hash-plugin","manifest_schema_version":1}"#,
    );

    let records = FileManifestLoader
        .enumerate(&DiscoverySource::Workspace(canonical_temp_root(&root)))
        .await
        .unwrap();

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].manifest_hash, records[1].manifest_hash);
}

fn write_manifest(path: &Path, content: impl AsRef<str>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content.as_ref()).unwrap();
}

fn write_executable(path: &Path, content: impl AsRef<str>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content.as_ref()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }
}

fn manifest_json(name: &str, trust_level: TrustLevel) -> String {
    let trust_level = match trust_level {
        TrustLevel::AdminTrusted => "admin_trusted",
        TrustLevel::UserControlled => "user_controlled",
        _ => unreachable!("test only uses known trust levels"),
    };
    format!(
        r#"{{
  "manifest_schema_version": 1,
  "name": "{name}",
  "version": "0.1.0",
  "trust_level": "{trust_level}",
  "min_harness_version": ">=0.0.0",
  "capabilities": {{}}
}}"#
    )
}

fn manifest(name: &str, trust_level: TrustLevel) -> PluginManifest {
    serde_json::from_str(&manifest_json(name, trust_level)).unwrap()
}

fn manifest_with_tool(
    name: &str,
    tool_name: &str,
    destructive: bool,
    trust_level: TrustLevel,
) -> PluginManifest {
    let trust_level = match trust_level {
        TrustLevel::AdminTrusted => "admin_trusted",
        TrustLevel::UserControlled => "user_controlled",
        _ => unreachable!("test only uses known trust levels"),
    };
    serde_json::from_str(&format!(
        r#"{{
  "manifest_schema_version": 1,
  "name": "{name}",
  "version": "0.1.0",
  "trust_level": "{trust_level}",
  "min_harness_version": ">=0.0.0",
  "capabilities": {{
    "tools": [
      {{ "name": "{tool_name}", "destructive": {destructive} }}
    ]
  }}
}}"#
    ))
    .unwrap()
}

fn manifest_with_tool_schema(
    name: &str,
    tool_name: &str,
    input_schema: serde_json::Value,
) -> PluginManifest {
    let mut manifest = serde_json::json!({
        "manifest_schema_version": 1,
        "name": name,
        "version": "0.1.0",
        "trust_level": "user_controlled",
        "min_harness_version": ">=0.0.0",
        "capabilities": {
            "tools": [
                {
                    "name": tool_name,
                    "destructive": false,
                    "input_schema": input_schema
                }
            ]
        }
    });
    serde_json::from_value(manifest.take()).unwrap()
}

fn manifest_with_proxy_capabilities(name: &str, trust_level: TrustLevel) -> PluginManifest {
    let trust_level = match trust_level {
        TrustLevel::AdminTrusted => "admin_trusted",
        TrustLevel::UserControlled => "user_controlled",
        _ => unreachable!("test only uses known trust levels"),
    };
    serde_json::from_str(&format!(
        r#"{{
  "manifest_schema_version": 1,
  "name": "{name}",
  "version": "0.1.0",
  "trust_level": "{trust_level}",
  "min_harness_version": ">=0.0.0",
  "capabilities": {{
    "hooks": [
      {{ "name": "sidecar-hook", "events": ["session_start"] }}
    ],
    "skills": [
      {{ "name": "sidecar-skill" }}
    ],
    "mcp_servers": [
      {{ "name": "sidecar-mcp" }}
    ]
  }}
}}"#
    ))
    .unwrap()
}

fn tool_ctx() -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: RunId::new(),
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: std::env::temp_dir(),
        project_workspace_root: None,
        sandbox: None,
        cap_registry: Arc::new(CapabilityRegistry::default()),
        redactor: Arc::new(TestRedactor),
        interrupt: InterruptToken::default(),
        parent_run: None,
        model: None,
        model_config_id: None,
        memory_thread_settings: None,
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
    }
}

async fn execute_authorized_tool(
    tool: &dyn Tool,
    input: serde_json::Value,
    ctx: ToolContext,
) -> Result<ToolStream, ToolError> {
    tool.validate(&input, &ctx)
        .await
        .expect("test input validates");
    let plan = tool.plan(&input, &ctx).await?;
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan))?;
    tool.execute_authorized(authorized, ctx).await
}

fn ticket_for(plan: &ToolActionPlan) -> AuthorizedTicketSummary {
    AuthorizedTicketSummary {
        ticket_id: AuthorizationTicketId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        run_id: RunId::new(),
        tool_use_id: plan.tool_use_id,
        tool_name: plan.tool_name.clone(),
        action_plan_hash: plan.plan_hash.clone(),
        consumed_at: Utc::now(),
    }
}

fn hook_ctx() -> HookContext {
    HookContext {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        run_id: Some(RunId::new()),
        turn_index: Some(1),
        correlation_id: CorrelationId::new(),
        causation_id: CausationId::new(),
        trust_level: TrustLevel::UserControlled,
        permission_mode: PermissionMode::Default,
        interactivity: InteractivityLevel::FullyInteractive,
        at: Utc::now(),
        view: Arc::new(TestSessionView {
            redactor: TestRedactor,
        }),
        upstream_outcome: None,
        replay_mode: ReplayMode::Live,
    }
}

struct TestRedactor;

impl harness_contracts::Redactor for TestRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.replace("secret", "[REDACTED]")
    }
}

struct TestSessionView {
    redactor: TestRedactor,
}

impl HookSessionView for TestSessionView {
    fn workspace_root(&self) -> Option<&Path> {
        None
    }

    fn recent_messages(&self, _limit: usize) -> Vec<HookMessageView> {
        Vec::new()
    }

    fn permission_mode(&self) -> PermissionMode {
        PermissionMode::Default
    }

    fn redacted(&self) -> &dyn harness_contracts::Redactor {
        &self.redactor
    }

    fn current_tool_descriptor(&self) -> Option<ToolDescriptorView> {
        None
    }
}

fn counting_factory(
    load_count: Arc<AtomicUsize>,
    plugin: Arc<dyn Plugin>,
) -> impl Fn() -> Arc<dyn Plugin> + Send + Sync + 'static {
    move || {
        load_count.fetch_add(1, Ordering::SeqCst);
        Arc::clone(&plugin)
    }
}

struct NoopPlugin {
    manifest: PluginManifest,
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

fn canonical_temp_root(temp: &tempfile::TempDir) -> PathBuf {
    temp.path().canonicalize().expect("canonical tempdir")
}

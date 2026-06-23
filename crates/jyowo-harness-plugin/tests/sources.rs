use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use harness_contracts::{
    ManifestValidationFailure as EventManifestValidationFailure, PluginId, RejectionReason,
    TrustLevel,
};
use harness_plugin::{
    CargoExtensionManifestLoader, CargoExtensionRuntimeLoader, DiscoverySource, FileManifestLoader,
    ManifestLoaderError, ManifestOrigin, ManifestSigner, Plugin, PluginActivationContext,
    PluginActivationResult, PluginConfig, PluginError, PluginManifest, PluginManifestLoader,
    PluginRegistry, PluginRuntimeLoader, StaticLinkRuntimeLoader,
};
use ring::digest;

#[tokio::test]
async fn workspace_source_scans_admin_plugin_json() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(
        &root.path().join("admin-a/plugin.json"),
        manifest_json("admin-a", TrustLevel::AdminTrusted),
    );

    let records = FileManifestLoader
        .enumerate(&DiscoverySource::Workspace(root.path().into()))
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
        &home.path().join("user-a/plugin.json"),
        manifest_json("user-a", TrustLevel::UserControlled),
    );

    let records = FileManifestLoader
        .enumerate(&DiscoverySource::User(home.path().into()))
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
        &project.path().join("project-a/plugin.json"),
        manifest_json("project-a", TrustLevel::UserControlled),
    );

    let records = FileManifestLoader
        .enumerate(&DiscoverySource::Project(project.path().into()))
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
        &root.path().join("admin-yaml/plugin.yaml"),
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
        .enumerate(&DiscoverySource::Workspace(root.path().into()))
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
        &home.path().join("bad-trust/plugin.json"),
        manifest_json("bad-trust", TrustLevel::AdminTrusted),
    );

    let records = FileManifestLoader
        .enumerate(&DiscoverySource::User(home.path().into()))
        .await
        .unwrap();

    assert_eq!(records.len(), 1);

    let registry = PluginRegistry::builder()
        .with_source(DiscoverySource::User(home.path().into()))
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
        &root.path().join("data/plugins/hash-a/plugin.json"),
        manifest_json("hash-a", TrustLevel::AdminTrusted),
    );

    let records = FileManifestLoader
        .enumerate(&DiscoverySource::Workspace(root.path().into()))
        .await
        .unwrap();
    let canonical = ManifestSigner::canonical_payload(&records[0].manifest).unwrap();
    let expected = digest::digest(&digest::SHA256, &canonical);
    let expected_hash: [u8; 32] = expected.as_ref().try_into().unwrap();

    assert_eq!(records[0].manifest_hash, expected_hash);
}

#[tokio::test]
async fn malformed_manifest_returns_validation_error() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(&root.path().join("bad/plugin.json"), "{ this is not json");

    let error = FileManifestLoader
        .enumerate(&DiscoverySource::Workspace(root.path().into()))
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
        &root.path().join("unknown-field/plugin.json"),
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
        .enumerate(&DiscoverySource::Workspace(root.path().into()))
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
        &root.path().join("future-schema/plugin.json"),
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
        .enumerate(&DiscoverySource::Workspace(root.path().into()))
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
        &root.path().join("user-default/plugin.json"),
        manifest_json("user-default", TrustLevel::UserControlled),
    );
    let registry = PluginRegistry::builder()
        .with_config(PluginConfig {
            allow_project_plugins: true,
            ..PluginConfig::default()
        })
        .with_source(DiscoverySource::Project(root.path().into()))
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
    let binary = root.path().join("jyowo-plugin-cargo-a");
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
        .with_search_paths(vec![root.path().to_path_buf()])
        .with_timeout(Duration::from_secs(15))
        .enumerate(&DiscoverySource::CargoExtension)
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].manifest.plugin_id().0, "cargo-a@0.1.0");
    assert!(
        matches!(&records[0].origin, ManifestOrigin::CargoExtension { binary: found, package_metadata } if found == &binary && package_metadata.contains_key("package"))
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
        &root.path().join("jyowo-plugin-bad"),
        r#"#!/bin/sh
if [ "$1" = "--harness-manifest" ]; then
printf 'not-json'
exit 0
fi
exit 2
"#,
    );

    let report = CargoExtensionManifestLoader::new()
        .with_search_paths(vec![root.path().to_path_buf()])
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
    let binary = root.path().join("jyowo-plugin-cargo-a");
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
        &root.path().join("static-a/plugin.json"),
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
        .with_source(DiscoverySource::Project(root.path().into()))
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
        &root.path().join("toolset/plugin.json"),
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
        .enumerate(&DiscoverySource::Workspace(root.path().into()))
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
        &root.path().join("hash-a/plugin.json"),
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
        &root.path().join("hash-b/plugin.json"),
        r#"{"capabilities":{},"min_harness_version":">=0.0.0","trust_level":"admin_trusted","version":"0.1.0","name":"hash-plugin","manifest_schema_version":1}"#,
    );

    let records = FileManifestLoader
        .enumerate(&DiscoverySource::Workspace(root.path().into()))
        .await
        .unwrap();

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].manifest_hash, records[1].manifest_hash);
}

fn write_manifest(path: &std::path::Path, content: impl AsRef<str>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content.as_ref()).unwrap();
}

fn write_executable(path: &std::path::Path, content: impl AsRef<str>) {
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

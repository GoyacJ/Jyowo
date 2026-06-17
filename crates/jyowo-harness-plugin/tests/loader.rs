use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::TrustLevel;
use harness_plugin::{
    DiscoverySource, ManifestLoaderError, ManifestOrigin, ManifestRecord, Plugin, PluginError,
    PluginManifest, PluginManifestLoader, PluginName, PluginRuntimeLoader, RuntimeLoaderError,
};

#[test]
fn manifest_name_rejects_non_canonical_names() {
    assert!(PluginName::new("invoice-tools").is_ok());
    assert!(PluginName::new("InvoiceTools").is_err());
    assert!(PluginName::new("invoice_tools").is_err());
    assert!(PluginName::new("invoice-").is_err());
    assert!(PluginName::new("1invoice").is_err());
}

#[test]
fn manifest_derives_stable_plugin_id_from_name_and_version() {
    let manifest = manifest("invoice-tools", "1.2.3");

    assert_eq!(manifest.plugin_id().0, "invoice-tools@1.2.3");
    manifest.validate_basic().unwrap();
}

#[tokio::test]
async fn manifest_loader_returns_records_without_instantiating_plugins() {
    let loader = StaticManifestLoader {
        record: record("manifest-only", "0.1.0"),
    };

    let records = loader
        .enumerate(&DiscoverySource::Workspace("/workspace".into()))
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].manifest.plugin_id().0, "manifest-only@0.1.0");
}

#[tokio::test]
async fn runtime_loader_instantiates_plugin_only_for_supported_origin() {
    let record = record("runtime-plugin", "0.2.0");
    let loader = StaticRuntimeLoader {
        plugin: Arc::new(NoopPlugin {
            manifest: record.manifest.clone(),
        }),
    };

    assert!(loader.can_load(&record.manifest, &record.origin));
    let plugin = loader.load(&record.manifest, &record.origin).await.unwrap();

    assert_eq!(plugin.manifest().plugin_id().0, "runtime-plugin@0.2.0");
}

struct StaticManifestLoader {
    record: ManifestRecord,
}

#[async_trait]
impl PluginManifestLoader for StaticManifestLoader {
    async fn enumerate(
        &self,
        _source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        Ok(vec![self.record.clone()])
    }
}

struct StaticRuntimeLoader {
    plugin: Arc<dyn Plugin>,
}

#[async_trait]
impl PluginRuntimeLoader for StaticRuntimeLoader {
    fn can_load(&self, _manifest: &PluginManifest, origin: &ManifestOrigin) -> bool {
        matches!(origin, ManifestOrigin::File { .. })
    }

    async fn load(
        &self,
        _manifest: &PluginManifest,
        origin: &ManifestOrigin,
    ) -> Result<Arc<dyn Plugin>, RuntimeLoaderError> {
        if self.can_load(self.plugin.manifest(), origin) {
            Ok(Arc::clone(&self.plugin))
        } else {
            Err(RuntimeLoaderError::UnsupportedOrigin(origin.to_string()))
        }
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
        _ctx: harness_plugin::PluginActivationContext,
    ) -> Result<harness_plugin::PluginActivationResult, PluginError> {
        Ok(harness_plugin::PluginActivationResult::default())
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

fn record(name: &str, version: &str) -> ManifestRecord {
    ManifestRecord::new(
        manifest(name, version),
        ManifestOrigin::File {
            path: format!("/plugins/{name}/plugin.json").into(),
        },
        [7; 32],
    )
    .unwrap()
}

fn manifest(name: &str, version: &str) -> PluginManifest {
    PluginManifest {
        manifest_schema_version: 1,
        name: PluginName::new(name).unwrap(),
        version: semver::Version::parse(version).unwrap(),
        trust_level: TrustLevel::UserControlled,
        description: None,
        authors: Vec::new(),
        repository: None,
        signature: None,
        capabilities: harness_plugin::PluginCapabilities::default(),
        dependencies: Vec::new(),
        min_harness_version: semver::VersionReq::parse(">=0.0.0").unwrap(),
    }
}

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use harness_contracts::{PluginId, TrustLevel};
use harness_plugin::{
    DiscoverySource, ManifestLoaderError, ManifestOrigin, ManifestRecord, Plugin,
    PluginActivationContext, PluginActivationResult, PluginCapabilities, PluginError,
    PluginManifest, PluginManifestLoader, PluginName, PluginRegistry, PluginRuntimeLoader,
    RuntimeLoaderError,
};

#[tokio::test]
async fn contract_rejects_runtime_plugin_manifest_mismatch() {
    let discovered = record("declared-plugin");
    let returned = Arc::new(NoopPlugin::new(manifest("wrong-plugin"))) as Arc<dyn Plugin>;
    let registry = registry_with(
        vec![discovered.clone()],
        vec![Arc::new(FixedRuntimeLoader::new(true, returned))],
    );

    registry.discover().await.unwrap();
    let error = registry
        .activate(&plugin_id("declared-plugin"))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        PluginError::RuntimeLoader(RuntimeLoaderError::LoadFailed(message))
            if message.contains("manifest mismatch")
    ));
}

#[tokio::test]
async fn contract_selects_first_runtime_loader_that_can_load() {
    let discovered = record("matching-plugin");
    let skipped = Arc::new(FixedRuntimeLoader::new(
        false,
        Arc::new(NoopPlugin::new(discovered.manifest.clone())),
    ));
    let selected = Arc::new(FixedRuntimeLoader::new(
        true,
        Arc::new(NoopPlugin::new(discovered.manifest.clone())),
    ));
    let registry = registry_with(
        vec![discovered],
        vec![
            skipped.clone() as Arc<dyn PluginRuntimeLoader>,
            selected.clone() as Arc<dyn PluginRuntimeLoader>,
        ],
    );

    registry.discover().await.unwrap();
    registry
        .activate(&plugin_id("matching-plugin"))
        .await
        .unwrap();

    assert_eq!(skipped.can_load_count(), 1);
    assert_eq!(skipped.load_count(), 0);
    assert_eq!(selected.can_load_count(), 1);
    assert_eq!(selected.load_count(), 1);
}

#[tokio::test]
async fn contract_manifest_loader_validation_stops_before_runtime_load() {
    let runtime = Arc::new(FixedRuntimeLoader::new(
        true,
        Arc::new(NoopPlugin::new(manifest("unused-plugin"))),
    ));
    let registry = PluginRegistry::builder()
        .with_source(DiscoverySource::Inline)
        .with_manifest_loader(Arc::new(FailingManifestLoader))
        .with_runtime_loader(runtime.clone())
        .build()
        .unwrap();

    let error = registry.discover().await.unwrap_err();

    assert!(matches!(error, PluginError::ManifestLoader(_)));
    assert_eq!(runtime.can_load_count(), 0);
    assert_eq!(runtime.load_count(), 0);
}

fn registry_with(
    records: Vec<ManifestRecord>,
    runtime_loaders: Vec<Arc<dyn PluginRuntimeLoader>>,
) -> PluginRegistry {
    let mut builder = PluginRegistry::builder()
        .with_source(DiscoverySource::Inline)
        .with_manifest_loader(Arc::new(StaticManifestLoader { records }));
    for loader in runtime_loaders {
        builder = builder.with_runtime_loader(loader);
    }
    builder.build().unwrap()
}

fn record(name: &str) -> ManifestRecord {
    ManifestRecord::new(
        manifest(name),
        ManifestOrigin::File {
            path: format!("/plugins/{name}/plugin.json").into(),
        },
        [9; 32],
    )
    .unwrap()
}

fn manifest(name: &str) -> PluginManifest {
    PluginManifest {
        manifest_schema_version: 1,
        name: PluginName::new(name).unwrap(),
        version: semver::Version::parse("0.1.0").unwrap(),
        trust_level: TrustLevel::UserControlled,
        description: None,
        authors: Vec::new(),
        repository: None,
        signature: None,
        capabilities: PluginCapabilities::default(),
        dependencies: Vec::new(),
        min_harness_version: semver::VersionReq::parse(">=0.0.0").unwrap(),
    }
}

fn plugin_id(name: &str) -> PluginId {
    PluginId(format!("{name}@0.1.0"))
}

struct StaticManifestLoader {
    records: Vec<ManifestRecord>,
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

struct FailingManifestLoader;

#[async_trait]
impl PluginManifestLoader for FailingManifestLoader {
    async fn enumerate(
        &self,
        _source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        Err(ManifestLoaderError::Io("contract failure".to_owned()))
    }
}

struct FixedRuntimeLoader {
    can_load: bool,
    plugin: Arc<dyn Plugin>,
    can_load_count: AtomicUsize,
    load_count: AtomicUsize,
}

impl FixedRuntimeLoader {
    fn new(can_load: bool, plugin: Arc<dyn Plugin>) -> Self {
        Self {
            can_load,
            plugin,
            can_load_count: AtomicUsize::new(0),
            load_count: AtomicUsize::new(0),
        }
    }

    fn can_load_count(&self) -> usize {
        self.can_load_count.load(Ordering::SeqCst)
    }

    fn load_count(&self) -> usize {
        self.load_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl PluginRuntimeLoader for FixedRuntimeLoader {
    fn can_load(&self, _manifest: &PluginManifest, _origin: &ManifestOrigin) -> bool {
        self.can_load_count.fetch_add(1, Ordering::SeqCst);
        self.can_load
    }

    async fn load(
        &self,
        _manifest: &PluginManifest,
        _origin: &ManifestOrigin,
    ) -> Result<Arc<dyn Plugin>, RuntimeLoaderError> {
        self.load_count.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::clone(&self.plugin))
    }
}

struct NoopPlugin {
    manifest: PluginManifest,
}

impl NoopPlugin {
    fn new(manifest: PluginManifest) -> Self {
        Self { manifest }
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

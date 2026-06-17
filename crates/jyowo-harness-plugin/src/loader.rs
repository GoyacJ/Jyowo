use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    ManifestLoaderError, ManifestOrigin, ManifestRecord, ManifestValidationFailure, Plugin,
    PluginManifest, RuntimeLoaderError,
};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ManifestLoadReport {
    pub records: Vec<ManifestRecord>,
    pub failures: Vec<ManifestValidationFailure>,
}

#[async_trait]
pub trait PluginManifestLoader: Send + Sync + 'static {
    async fn enumerate(
        &self,
        source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError>;

    async fn load_report(
        &self,
        source: &DiscoverySource,
    ) -> Result<ManifestLoadReport, ManifestLoaderError> {
        self.enumerate(source)
            .await
            .map(|records| ManifestLoadReport {
                records,
                failures: Vec::new(),
            })
    }
}

#[async_trait]
pub trait PluginRuntimeLoader: Send + Sync + 'static {
    fn can_load(&self, manifest: &PluginManifest, origin: &ManifestOrigin) -> bool;

    async fn load(
        &self,
        manifest: &PluginManifest,
        origin: &ManifestOrigin,
    ) -> Result<Arc<dyn Plugin>, RuntimeLoaderError>;
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DiscoverySource {
    Workspace(std::path::PathBuf),
    User(std::path::PathBuf),
    Project(std::path::PathBuf),
    CargoExtension,
    Inline,
}

impl std::fmt::Display for DiscoverySource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Workspace(path) => write!(formatter, "workspace:{}", path.display()),
            Self::User(path) => write!(formatter, "user:{}", path.display()),
            Self::Project(path) => write!(formatter, "project:{}", path.display()),
            Self::CargoExtension => formatter.write_str("cargo_extension"),
            Self::Inline => formatter.write_str("inline"),
        }
    }
}

type StaticPluginFactory = dyn Fn() -> Arc<dyn Plugin> + Send + Sync + 'static;

#[derive(Default, Clone)]
pub struct StaticLinkRuntimeLoader {
    factories: BTreeMap<harness_contracts::PluginId, Arc<StaticPluginFactory>>,
}

impl std::fmt::Debug for StaticLinkRuntimeLoader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StaticLinkRuntimeLoader")
            .field("plugins", &self.factories.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl StaticLinkRuntimeLoader {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_plugin(mut self, id: harness_contracts::PluginId, plugin: Arc<dyn Plugin>) -> Self {
        self.factories
            .insert(id, Arc::new(move || Arc::clone(&plugin)));
        self
    }

    #[must_use]
    pub fn with_factory(
        mut self,
        id: harness_contracts::PluginId,
        factory: impl Fn() -> Arc<dyn Plugin> + Send + Sync + 'static,
    ) -> Self {
        self.factories.insert(id, Arc::new(factory));
        self
    }
}

#[async_trait]
impl PluginRuntimeLoader for StaticLinkRuntimeLoader {
    fn can_load(&self, manifest: &PluginManifest, _origin: &ManifestOrigin) -> bool {
        self.factories.contains_key(&manifest.plugin_id())
    }

    async fn load(
        &self,
        manifest: &PluginManifest,
        _origin: &ManifestOrigin,
    ) -> Result<Arc<dyn Plugin>, RuntimeLoaderError> {
        self.factories
            .get(&manifest.plugin_id())
            .map(|factory| factory())
            .ok_or_else(|| RuntimeLoaderError::PluginNotFound(manifest.name.clone()))
    }
}

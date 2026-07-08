use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use harness_contracts::{PluginId, TrustLevel};
use harness_plugin::{
    DiscoverySource, ManifestLoaderError, ManifestOrigin, ManifestRecord, Plugin,
    PluginActivationContext, PluginActivationResult, PluginCapabilities, PluginDependency,
    PluginDependencyKind, PluginError, PluginManifest, PluginManifestLoader, PluginName,
    PluginRegistry, PluginRuntimeLoader, PluginWarning, RuntimeLoaderError,
};
use parking_lot::Mutex;

#[tokio::test]
async fn activates_required_dependencies_before_root() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let dependency = record("dependency", vec![]);
    let root = record(
        "root",
        vec![PluginDependency {
            name: PluginName::new("dependency").unwrap(),
            version_req: semver::VersionReq::parse(">=0.1.0").unwrap(),
            kind: PluginDependencyKind::Required,
        }],
    );
    let registry = registry_with(vec![dependency.clone(), root.clone()], order.clone());
    registry.discover().await.expect("discover");

    registry
        .activate(&plugin_id("root"))
        .await
        .expect("activate");

    assert_eq!(
        order.lock().clone(),
        vec![plugin_id("dependency"), plugin_id("root")]
    );
}

#[tokio::test]
async fn rejects_missing_required_dependency() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let root = record(
        "root",
        vec![PluginDependency {
            name: PluginName::new("missing").unwrap(),
            version_req: semver::VersionReq::parse(">=1.0.0").unwrap(),
            kind: PluginDependencyKind::Required,
        }],
    );
    let registry = registry_with(vec![root], order);
    registry.discover().await.expect("discover");

    let error = registry
        .activate(&plugin_id("root"))
        .await
        .expect_err("missing dependency rejected");

    assert!(matches!(
        error,
        PluginError::DependencyUnsatisfied {
            dependency,
            requirement
        } if dependency == "missing" && requirement == ">=1.0.0"
    ));
    assert!(matches!(
        registry.state(&plugin_id("root")),
        Some(harness_plugin::PluginLifecycleState::Rejected(_))
    ));
}

#[tokio::test]
async fn detects_required_dependency_cycle() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let alpha = record(
        "alpha",
        vec![PluginDependency {
            name: PluginName::new("beta").unwrap(),
            version_req: semver::VersionReq::parse(">=0.1.0").unwrap(),
            kind: PluginDependencyKind::Required,
        }],
    );
    let beta = record(
        "beta",
        vec![PluginDependency {
            name: PluginName::new("alpha").unwrap(),
            version_req: semver::VersionReq::parse(">=0.1.0").unwrap(),
            kind: PluginDependencyKind::Required,
        }],
    );
    let registry = registry_with(vec![alpha, beta], order);
    registry.discover().await.expect("discover");

    let error = registry
        .activate(&plugin_id("alpha"))
        .await
        .expect_err("cycle rejected");

    assert!(matches!(error, PluginError::DependencyCycle(_)));
    assert!(matches!(
        registry.state(&plugin_id("alpha")),
        Some(harness_plugin::PluginLifecycleState::Rejected(_))
    ));
}

#[tokio::test]
async fn semver_ranges_select_highest_satisfying_dependency() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let low = record_with_version("dependency", "1.1.0", vec![]);
    let high = record_with_version("dependency", "1.4.0", vec![]);
    let out_of_range = record_with_version("dependency", "2.0.0", vec![]);
    let root = record_with_version(
        "root",
        "0.1.0",
        vec![PluginDependency {
            name: PluginName::new("dependency").unwrap(),
            version_req: semver::VersionReq::parse("^1.0").unwrap(),
            kind: PluginDependencyKind::Required,
        }],
    );
    let registry = registry_with(vec![low, high, out_of_range, root], order.clone());
    registry.discover().await.expect("discover");

    registry
        .activate(&plugin_id("root"))
        .await
        .expect("activate");

    assert_eq!(
        order.lock().clone(),
        vec![PluginId("dependency@1.4.0".to_owned()), plugin_id("root")]
    );
}

#[tokio::test]
async fn compound_semver_ranges_select_highest_satisfying_dependency() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let low = record_with_version("dependency", "1.1.0", vec![]);
    let high = record_with_version("dependency", "1.9.0", vec![]);
    let out_of_range = record_with_version("dependency", "2.0.0", vec![]);
    let root = record_with_version(
        "root",
        "0.1.0",
        vec![PluginDependency {
            name: PluginName::new("dependency").unwrap(),
            version_req: semver::VersionReq::parse(">=1.0.0, <2.0.0").unwrap(),
            kind: PluginDependencyKind::Required,
        }],
    );
    let registry = registry_with(vec![low, high, out_of_range, root], order.clone());
    registry.discover().await.expect("discover");

    registry
        .activate(&plugin_id("root"))
        .await
        .expect("activate");

    assert_eq!(
        order.lock().clone(),
        vec![PluginId("dependency@1.9.0".to_owned()), plugin_id("root")]
    );
}

#[tokio::test]
async fn activation_graph_rejects_second_version_for_selected_dependency_name() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let shared_v1 = record_with_version("shared", "1.5.0", vec![]);
    let shared_v2 = record_with_version("shared", "2.1.0", vec![]);
    let peer = record_with_version(
        "peer",
        "0.1.0",
        vec![PluginDependency {
            name: PluginName::new("shared").unwrap(),
            version_req: semver::VersionReq::parse(">=2.0.0, <3.0.0").unwrap(),
            kind: PluginDependencyKind::Required,
        }],
    );
    let root = record_with_version(
        "root",
        "0.1.0",
        vec![
            PluginDependency {
                name: PluginName::new("shared").unwrap(),
                version_req: semver::VersionReq::parse(">=1.0.0, <2.0.0").unwrap(),
                kind: PluginDependencyKind::Required,
            },
            PluginDependency {
                name: PluginName::new("peer").unwrap(),
                version_req: semver::VersionReq::parse(">=0.1.0").unwrap(),
                kind: PluginDependencyKind::Required,
            },
        ],
    );
    let registry = registry_with(vec![shared_v1, shared_v2, peer, root], order);
    registry.discover().await.expect("discover");

    let error = registry
        .activate(&plugin_id("root"))
        .await
        .expect_err("conflicting selected dependency version rejected");

    assert!(matches!(
        error,
        PluginError::DependencyUnsatisfied {
            dependency,
            requirement
        } if dependency == "shared" && requirement == ">=2.0.0, <3.0.0"
    ));
}

#[tokio::test]
async fn optional_dependency_missing_is_recorded_as_warning() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let root = record(
        "root",
        vec![PluginDependency {
            name: PluginName::new("optional-peer").unwrap(),
            version_req: semver::VersionReq::parse("^1.0").unwrap(),
            kind: PluginDependencyKind::Optional,
        }],
    );
    let registry = registry_with(vec![root], order);
    registry.discover().await.expect("discover");

    let warnings = registry.snapshot().warnings;

    assert_eq!(
        warnings.get(&plugin_id("root")),
        Some(&vec![PluginWarning::OptionalDependencyMissing {
            dependency: "optional-peer".to_owned(),
            requirement: "^1.0".to_owned(),
        }])
    );
}

#[tokio::test]
async fn deactivate_rejects_active_dependents_and_cascade_deactivates_them() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let dependency = record("dependency", vec![]);
    let root = record(
        "root",
        vec![PluginDependency {
            name: PluginName::new("dependency").unwrap(),
            version_req: semver::VersionReq::parse(">=0.1.0").unwrap(),
            kind: PluginDependencyKind::Required,
        }],
    );
    let registry = registry_with(vec![dependency, root], order);
    registry.discover().await.expect("discover");
    registry
        .activate(&plugin_id("root"))
        .await
        .expect("activate");

    let error = registry
        .deactivate(&plugin_id("dependency"))
        .await
        .expect_err("active dependent rejected");
    assert!(
        matches!(error, PluginError::ActiveDependents(dependents) if dependents == vec![plugin_id("root")])
    );

    registry
        .deactivate_cascade(&plugin_id("dependency"))
        .await
        .expect("cascade");

    assert_eq!(
        registry.state(&plugin_id("root")),
        Some(harness_plugin::PluginLifecycleState::Deactivated)
    );
    assert_eq!(
        registry.state(&plugin_id("dependency")),
        Some(harness_plugin::PluginLifecycleState::Deactivated)
    );
}

fn registry_with(records: Vec<ManifestRecord>, order: Arc<Mutex<Vec<PluginId>>>) -> PluginRegistry {
    let plugins = records
        .iter()
        .cloned()
        .map(|record| Arc::new(OrderedPlugin::new(record, order.clone())) as Arc<dyn Plugin>)
        .collect::<Vec<_>>();
    PluginRegistry::builder()
        .with_manifest_loader(Arc::new(StaticManifestLoader {
            records: records.clone(),
        }))
        .with_runtime_loader(Arc::new(StaticRuntimeLoader { plugins }))
        .build()
        .expect("registry")
}

fn record(name: &str, dependencies: Vec<PluginDependency>) -> ManifestRecord {
    record_with_version(name, "0.1.0", dependencies)
}

fn record_with_version(
    name: &str,
    version: &str,
    dependencies: Vec<PluginDependency>,
) -> ManifestRecord {
    ManifestRecord::new(
        PluginManifest {
            name: PluginName::new(name).unwrap(),
            version: semver::Version::parse(version).unwrap(),
            trust_level: TrustLevel::UserControlled,
            description: None,
            authors: Vec::new(),
            repository: None,
            signature: None,
            capabilities: PluginCapabilities::default(),
            dependencies,
            min_harness_version: semver::VersionReq::parse(">=0.0.0").unwrap(),
        },
        ManifestOrigin::File {
            path: format!("/plugins/{name}/plugin.json").into(),
        },
        [8; 32],
    )
    .unwrap()
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

struct StaticRuntimeLoader {
    plugins: Vec<Arc<dyn Plugin>>,
}

#[async_trait]
impl PluginRuntimeLoader for StaticRuntimeLoader {
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

struct OrderedPlugin {
    manifest: PluginManifest,
    order: Arc<Mutex<Vec<PluginId>>>,
    activations: AtomicUsize,
}

impl OrderedPlugin {
    fn new(record: ManifestRecord, order: Arc<Mutex<Vec<PluginId>>>) -> Self {
        Self {
            manifest: record.manifest,
            order,
            activations: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl Plugin for OrderedPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        _ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        self.activations.fetch_add(1, Ordering::SeqCst);
        self.order.lock().push(self.manifest.plugin_id());
        Ok(PluginActivationResult::default())
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

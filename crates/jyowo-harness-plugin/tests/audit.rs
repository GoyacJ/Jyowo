use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    Event, ManifestValidationFailure as EventManifestValidationFailure, PluginId, SteeringId,
    SteeringRequest, TenantId, TrustLevel,
};
use harness_plugin::{
    DiscoverySource, ManifestLoaderError, ManifestOrigin, ManifestRecord, ManifestSignature,
    ManifestSigner, Plugin, PluginActivationContext, PluginActivationResult, PluginCapabilities,
    PluginCapabilityRegistries, PluginError, PluginEventSink, PluginManifest, PluginManifestLoader,
    PluginMetricsSink, PluginName, PluginRegistry, PluginRuntimeLoader, RegistrationError,
    RuntimeLoaderError, SignatureAlgorithm, SteeringRegistration,
};
use parking_lot::Mutex;
use ring::signature::{Ed25519KeyPair, KeyPair};

#[tokio::test]
async fn registry_emits_plugin_loaded_event() {
    let sink = Arc::new(CollectingSink::default());
    let mut record = record("audit-plugin");
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).expect("pkcs8");
    let keypair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("keypair");
    record.manifest.trust_level = TrustLevel::AdminTrusted;
    record.manifest.capabilities.steering = true;
    record.manifest = signed_manifest(record.manifest, &keypair);
    let plugin = Arc::new(NoopPlugin {
        manifest: record.manifest.clone(),
    });
    let registry = PluginRegistry::builder()
        .with_tenant_id(TenantId::from_u128(77))
        .with_event_sink(sink.clone())
        .with_trusted_signer(keypair.public_key().as_ref().to_vec())
        .with_capability_registries(
            PluginCapabilityRegistries::default()
                .with_steering_registration(Arc::new(NoopSteeringRegistration)),
        )
        .with_manifest_loader(Arc::new(StaticManifestLoader {
            record: record.clone(),
        }))
        .with_runtime_loader(Arc::new(StaticRuntimeLoader { plugin }))
        .build()
        .expect("registry");

    registry.discover().await.expect("discover");
    registry
        .activate(&plugin_id("audit-plugin"))
        .await
        .expect("activate");

    let loaded = sink
        .events()
        .into_iter()
        .find_map(|event| match event {
            Event::PluginLoaded(loaded) => Some(loaded),
            _ => None,
        })
        .expect("loaded event");
    assert_eq!(loaded.tenant_id, TenantId::from_u128(77));
    assert_eq!(loaded.plugin_id, plugin_id("audit-plugin"));
    assert_eq!(loaded.manifest_hash, [6; 32]);

    let capabilities = serde_json::to_value(&loaded.capabilities).expect("capabilities json");
    assert_eq!(capabilities["steering"], true);
}

#[tokio::test]
async fn registry_emits_manifest_validation_failed_event() {
    let sink = Arc::new(CollectingSink::default());
    let registry = PluginRegistry::builder()
        .with_event_sink(sink.clone())
        .with_manifest_loader(Arc::new(FailingManifestLoader))
        .build()
        .expect("registry");

    let error = registry.discover().await.expect_err("discovery fails");
    assert!(matches!(error, PluginError::ManifestLoader(_)));
    assert!(sink.events().iter().any(|event| matches!(
        event,
        Event::ManifestValidationFailed(failed)
            if failed.partial_name.as_deref() == Some("partial")
                && failed.partial_version.as_deref() == Some("0.2.0")
                && failed.raw_bytes_hash == [7; 32]
                && matches!(
                    failed.failure,
                    EventManifestValidationFailure::SyntaxError { ref details }
                        if details == "withheld"
                )
    )));
    let encoded = serde_json::to_string(&sink.events()).expect("events json");
    assert!(!encoded.contains("/plugins/partial"));
    assert!(!encoded.contains("invalid"));
}

#[tokio::test]
async fn registry_emits_plugin_rejected_event_without_raw_details() {
    let sink = Arc::new(CollectingSink::default());
    let record = record("jyowo-audit-rejected");
    let registry = PluginRegistry::builder()
        .with_event_sink(sink.clone())
        .with_manifest_loader(Arc::new(StaticManifestLoader {
            record: record.clone(),
        }))
        .build()
        .expect("registry");

    let discovered = registry.discover().await.expect("discover");

    assert!(discovered.is_empty());
    assert!(sink.events().iter().any(|event| matches!(
        event,
        Event::PluginRejected(rejected)
            if rejected.plugin_id == record.manifest.plugin_id()
                && matches!(
                    rejected.reason,
                    harness_contracts::RejectionReason::NamespaceConflict { ref details }
                        if details == "withheld"
                )
    )));
    let encoded = serde_json::to_string(&sink.events()).expect("events json");
    assert!(!encoded.contains("/plugins/jyowo-audit-rejected"));
    assert!(!encoded.contains("reserved plugin prefix requires AdminTrusted source"));
}

#[tokio::test]
async fn plugin_metrics_records_loaded_by_source_and_trust_level() {
    let metrics = Arc::new(CollectingMetrics::default());
    let record = record("metric-loaded");
    let plugin = Arc::new(NoopPlugin {
        manifest: record.manifest.clone(),
    });
    let registry = PluginRegistry::builder()
        .with_metrics_sink(metrics.clone())
        .with_manifest_loader(Arc::new(StaticManifestLoader {
            record: record.clone(),
        }))
        .with_runtime_loader(Arc::new(StaticRuntimeLoader { plugin }))
        .build()
        .expect("registry");

    registry.discover().await.expect("discover");
    registry
        .activate(&plugin_id("metric-loaded"))
        .await
        .expect("activate");

    let records = metrics.records();
    assert!(records.contains(&MetricRecord::Discovered {
        source: "file".to_owned(),
        trust_level: TrustLevel::UserControlled,
    }));
    assert!(records.contains(&MetricRecord::Loaded {
        source: "file".to_owned(),
        trust_level: TrustLevel::UserControlled,
    }));
    assert!(records.contains(&MetricRecord::Activated {
        source: "file".to_owned(),
        trust_level: TrustLevel::UserControlled,
    }));
    assert!(records.contains(&MetricRecord::ActiveTotal { active: 1 }));
    assert!(records.iter().any(|record| matches!(
        record,
        MetricRecord::SignatureValidation { source } if source == "file"
    )));
    assert!(records
        .iter()
        .any(|record| matches!(record, MetricRecord::DependencyResolution)));
}

#[tokio::test]
async fn plugin_metrics_records_capability_registration_rejection() {
    let metrics = Arc::new(CollectingMetrics::default());
    let record = record("metric-capability-rejected");
    let plugin = Arc::new(ResultPlugin {
        manifest: record.manifest.clone(),
        result: PluginActivationResult {
            registered_tools: vec!["undeclared-tool".to_owned()],
            ..PluginActivationResult::default()
        },
    });
    let registry = PluginRegistry::builder()
        .with_metrics_sink(metrics.clone())
        .with_manifest_loader(Arc::new(StaticManifestLoader {
            record: record.clone(),
        }))
        .with_runtime_loader(Arc::new(StaticRuntimeLoader { plugin }))
        .build()
        .expect("registry");

    registry.discover().await.expect("discover");
    let error = registry
        .activate(&plugin_id("metric-capability-rejected"))
        .await
        .expect_err("activation should reject undeclared capability result");
    assert!(matches!(error, PluginError::Registration(_)));

    assert!(metrics
        .records()
        .contains(&MetricRecord::CapabilityRegistrationRejected {
            kind: "tool".to_owned(),
            reason: "registration".to_owned(),
        }));
}

#[tokio::test]
async fn plugin_metrics_records_signer_totals() {
    let metrics = Arc::new(CollectingMetrics::default());
    let registry = PluginRegistry::builder()
        .with_metrics_sink(metrics.clone())
        .with_manifest_loader(Arc::new(StaticManifestLoader {
            record: record("metric-signer-total"),
        }))
        .build()
        .expect("registry");

    registry.discover().await.expect("discover");

    assert!(metrics
        .records()
        .iter()
        .any(|record| matches!(record, MetricRecord::SignerTotals { .. })));
}

#[tokio::test]
async fn plugin_metrics_records_loaded_by_source_and_trust_level_legacy_hook() {
    let metrics = Arc::new(CollectingMetrics::default());
    let record = record("metric-loaded-legacy");
    let plugin = Arc::new(NoopPlugin {
        manifest: record.manifest.clone(),
    });
    let registry = PluginRegistry::builder()
        .with_metrics_sink(metrics.clone())
        .with_manifest_loader(Arc::new(StaticManifestLoader {
            record: record.clone(),
        }))
        .with_runtime_loader(Arc::new(StaticRuntimeLoader { plugin }))
        .build()
        .expect("registry");

    registry.discover().await.expect("discover");
    registry
        .activate(&plugin_id("metric-loaded-legacy"))
        .await
        .expect("activate");

    assert!(metrics.records().contains(&MetricRecord::Loaded {
        source: "file".to_owned(),
        trust_level: TrustLevel::UserControlled,
    }));
}

#[tokio::test]
async fn plugin_metrics_records_rejection_by_source_trust_and_reason() {
    let metrics = Arc::new(CollectingMetrics::default());
    let mut record = record("jyowo-reserved");
    record.manifest.trust_level = TrustLevel::UserControlled;
    let registry = PluginRegistry::builder()
        .with_metrics_sink(metrics.clone())
        .with_manifest_loader(Arc::new(StaticManifestLoader { record }))
        .build()
        .expect("registry");

    registry.discover().await.expect("discover");

    assert!(metrics.records().contains(&MetricRecord::Rejected {
        source: "file".to_owned(),
        trust_level: TrustLevel::UserControlled,
        reason: "namespace_conflict".to_owned(),
    }));
}

#[tokio::test]
async fn plugin_metrics_records_manifest_validation_failure_by_source_and_failure() {
    let metrics = Arc::new(CollectingMetrics::default());
    let registry = PluginRegistry::builder()
        .with_metrics_sink(metrics.clone())
        .with_manifest_loader(Arc::new(FailingManifestLoader))
        .build()
        .expect("registry");

    let error = registry.discover().await.expect_err("discovery fails");
    assert!(matches!(error, PluginError::ManifestLoader(_)));
    assert!(metrics
        .records()
        .contains(&MetricRecord::ManifestValidationFailed {
            source: "file".to_owned(),
            failure: "syntax_error".to_owned(),
        }));
}

fn record(name: &str) -> ManifestRecord {
    ManifestRecord::new(
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
        },
        ManifestOrigin::File {
            path: format!("/plugins/{name}/plugin.json").into(),
        },
        [6; 32],
    )
    .unwrap()
}

fn plugin_id(name: &str) -> PluginId {
    PluginId(format!("{name}@0.1.0"))
}

fn signed_manifest(mut manifest: PluginManifest, keypair: &Ed25519KeyPair) -> PluginManifest {
    let payload = ManifestSigner::canonical_payload(&manifest).expect("canonical payload");
    manifest.signature = Some(ManifestSignature {
        algorithm: SignatureAlgorithm::Ed25519,
        signer: "user-injected-0".to_owned(),
        signature: keypair.sign(&payload).as_ref().to_vec(),
        timestamp: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH.to_rfc3339(),
    });
    manifest
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

impl PluginEventSink for CollectingSink {
    fn emit(&self, event: Event) {
        self.events.lock().push(event);
    }
}

struct NoopSteeringRegistration;

#[async_trait]
impl SteeringRegistration for NoopSteeringRegistration {
    async fn push(&self, _request: SteeringRequest) -> Result<SteeringId, RegistrationError> {
        Ok(SteeringId::new())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MetricRecord {
    Discovered {
        source: String,
        trust_level: TrustLevel,
    },
    Loaded {
        source: String,
        trust_level: TrustLevel,
    },
    Activated {
        source: String,
        trust_level: TrustLevel,
    },
    Rejected {
        source: String,
        trust_level: TrustLevel,
        reason: String,
    },
    ManifestValidationFailed {
        source: String,
        failure: String,
    },
    SignatureValidation {
        source: String,
    },
    SignerTotals {
        active: usize,
        revoked: usize,
    },
    DependencyResolution,
    ActiveTotal {
        active: usize,
    },
    CapabilityRegistrationRejected {
        kind: String,
        reason: String,
    },
}

#[derive(Default)]
struct CollectingMetrics {
    records: Mutex<Vec<MetricRecord>>,
}

impl CollectingMetrics {
    fn records(&self) -> Vec<MetricRecord> {
        self.records.lock().clone()
    }
}

impl PluginMetricsSink for CollectingMetrics {
    fn plugin_discovered(&self, source: &str, trust_level: TrustLevel) {
        self.records.lock().push(MetricRecord::Discovered {
            source: source.to_owned(),
            trust_level,
        });
    }

    fn plugin_loaded(&self, source: &str, trust_level: TrustLevel) {
        self.records.lock().push(MetricRecord::Loaded {
            source: source.to_owned(),
            trust_level,
        });
    }

    fn plugin_activated(&self, source: &str, trust_level: TrustLevel) {
        self.records.lock().push(MetricRecord::Activated {
            source: source.to_owned(),
            trust_level,
        });
    }

    fn plugin_rejected(&self, source: &str, trust_level: TrustLevel, reason: &str) {
        self.records.lock().push(MetricRecord::Rejected {
            source: source.to_owned(),
            trust_level,
            reason: reason.to_owned(),
        });
    }

    fn plugin_manifest_validation_failed(&self, source: &str, failure: &str) {
        self.records
            .lock()
            .push(MetricRecord::ManifestValidationFailed {
                source: source.to_owned(),
                failure: failure.to_owned(),
            });
    }

    fn plugin_signature_validation_duration_ms(&self, source: &str, _duration_ms: u64) {
        self.records.lock().push(MetricRecord::SignatureValidation {
            source: source.to_owned(),
        });
    }

    fn plugin_signer_totals(&self, active: usize, revoked: usize) {
        self.records
            .lock()
            .push(MetricRecord::SignerTotals { active, revoked });
    }

    fn plugin_dependency_resolution_duration_ms(&self, _duration_ms: u64) {
        self.records.lock().push(MetricRecord::DependencyResolution);
    }

    fn plugin_active_total(&self, active: usize) {
        self.records
            .lock()
            .push(MetricRecord::ActiveTotal { active });
    }

    fn plugin_capability_registration_rejected(&self, kind: &str, reason: &str) {
        self.records
            .lock()
            .push(MetricRecord::CapabilityRegistrationRejected {
                kind: kind.to_owned(),
                reason: reason.to_owned(),
            });
    }
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

struct FailingManifestLoader;

#[async_trait]
impl PluginManifestLoader for FailingManifestLoader {
    async fn enumerate(
        &self,
        _source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        Err(ManifestLoaderError::Validation(
            harness_plugin::ManifestValidationFailure {
                origin: Some(ManifestOrigin::File {
                    path: "/plugins/partial/plugin.json".into(),
                }),
                partial_name: Some("partial".to_owned()),
                partial_version: Some("0.2.0".to_owned()),
                raw_bytes_hash: [7; 32],
                failure: EventManifestValidationFailure::SyntaxError {
                    details: "invalid".to_owned(),
                },
                details: "invalid".to_owned(),
            },
        ))
    }
}

struct StaticRuntimeLoader {
    plugin: Arc<dyn Plugin>,
}

#[async_trait]
impl PluginRuntimeLoader for StaticRuntimeLoader {
    fn can_load(&self, manifest: &PluginManifest, _origin: &ManifestOrigin) -> bool {
        self.plugin.manifest().plugin_id() == manifest.plugin_id()
    }

    async fn load(
        &self,
        _manifest: &PluginManifest,
        _origin: &ManifestOrigin,
    ) -> Result<Arc<dyn Plugin>, RuntimeLoaderError> {
        Ok(self.plugin.clone())
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

struct ResultPlugin {
    manifest: PluginManifest,
    result: PluginActivationResult,
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

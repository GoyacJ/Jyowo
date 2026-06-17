use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use harness_contracts::{
    now, Event, ManifestOriginRef, ManifestValidationFailedEvent, PluginCapabilitiesSummary,
    PluginId, PluginLifecycleStateDiscriminant, PluginLoadedEvent, PluginRejectedEvent,
    RejectionReason, TenantId, TrustLevel,
};
use parking_lot::RwLock;
use serde_json::Value;

use crate::{
    CapabilityRegistrationState, CapabilitySlot, DiscoverySource, ManifestLoaderError,
    ManifestRecord, ManifestSigner, Plugin, PluginActivationContext, PluginActivationResult,
    PluginCapabilities, PluginCapabilityRegistries, PluginDependencyKind, PluginError,
    PluginManifest, PluginManifestLoader, PluginName, PluginRuntimeLoader, RegistrationError,
    RuntimeLoaderError, ScopedCoordinatorStrategyRegistration, ScopedHookRegistration,
    ScopedMcpRegistration, ScopedMemoryProviderRegistration, ScopedSkillRegistration,
    ScopedSteeringRegistration, ScopedToolRegistration, SignatureAlgorithm, SignerProvenance,
    StaticLinkRuntimeLoader, StaticTrustedSignerStore, TrustedSigner, TrustedSignerStore,
};

#[derive(Debug, Clone, PartialEq)]
pub struct PluginConfig {
    pub enabled: bool,
    pub allow_project_plugins: bool,
    pub policy: PluginAdmissionPolicy,
    pub entries: BTreeMap<PluginName, Value>,
    pub workspace_root: Option<PathBuf>,
    pub strict_plugin_only_customization: StrictPluginOnlyPolicy,
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_project_plugins: false,
            policy: PluginAdmissionPolicy::AllowAll,
            entries: BTreeMap::new(),
            workspace_root: None,
            strict_plugin_only_customization: StrictPluginOnlyPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PluginAdmissionPolicy {
    AllowAll,
    Allow(BTreeSet<PluginName>),
    Deny(BTreeSet<PluginName>),
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct StrictPluginOnlyPolicy {
    pub enabled: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PluginWarning {
    OptionalDependencyMissing {
        dependency: String,
        requirement: String,
    },
    DeclaredCapabilityUnregistered {
        kind: &'static str,
        name: String,
    },
}

#[derive(Clone)]
pub struct PluginRegistry {
    inner: Arc<RwLock<PluginRegistryInner>>,
    manifest_loaders: Arc<Vec<Arc<dyn PluginManifestLoader>>>,
    runtime_loaders: Arc<Vec<Arc<dyn PluginRuntimeLoader>>>,
    discovery_sources: Arc<Vec<DiscoverySource>>,
    manifest_signer: ManifestSigner,
    config: PluginConfig,
    capability_registries: Arc<RwLock<PluginCapabilityRegistries>>,
    event_sink: Option<Arc<dyn PluginEventSink>>,
    metrics_sink: Option<Arc<dyn PluginMetricsSink>>,
    tenant_id: TenantId,
}

impl std::fmt::Debug for PluginRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginRegistry")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

#[derive(Default)]
struct PluginRegistryInner {
    discovered: BTreeMap<PluginId, DiscoveredPlugin>,
    activated: BTreeMap<PluginId, ActivatedPlugin>,
    state: BTreeMap<PluginId, PluginLifecycleState>,
    state_detail: BTreeMap<PluginId, PluginLifecycleDetail>,
    warnings: BTreeMap<PluginId, Vec<PluginWarning>>,
    slots: CapabilitySlotManager,
    memory_provider: Option<Arc<dyn harness_memory::MemoryProvider>>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum PluginLifecycleState {
    Validated,
    Activating,
    Activated,
    Deactivating,
    Deactivated,
    Rejected(RejectionReason),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginLifecycleDetail {
    pub state: PluginLifecycleState,
    pub rejection_reason: Option<RejectionReason>,
    pub failure: Option<String>,
}

#[derive(Clone)]
struct ActivatedPlugin {
    plugin: Arc<dyn Plugin>,
    slots: Vec<CapabilitySlot>,
    registrations: CapabilityRegistrations,
    memory_provider: Option<Arc<dyn harness_memory::MemoryProvider>>,
}

#[derive(Debug, Clone, Default)]
struct CapabilityRegistrations {
    tools: Vec<String>,
    hooks: Vec<String>,
    mcp: Vec<String>,
    skills: Vec<String>,
}

impl CapabilityRegistrations {
    fn from_state(state: &CapabilityRegistrationState) -> Self {
        Self {
            tools: state.registered_tools(),
            hooks: state.registered_hooks(),
            mcp: state.registered_mcp(),
            skills: state.registered_skills(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredPlugin {
    pub record: ManifestRecord,
    pub source: DiscoverySource,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginRegistrySnapshot {
    pub discovered: Vec<PluginId>,
    pub activated: Vec<PluginId>,
    pub states: BTreeMap<PluginId, PluginLifecycleState>,
    pub occupied_slots: HashMap<CapabilitySlot, PluginId>,
    pub warnings: BTreeMap<PluginId, Vec<PluginWarning>>,
}

pub struct PluginRegistryBuilder {
    manifest_loaders: Vec<Arc<dyn PluginManifestLoader>>,
    runtime_loaders: Vec<Arc<dyn PluginRuntimeLoader>>,
    discovery_sources: Vec<DiscoverySource>,
    signer_store: Option<Arc<dyn TrustedSignerStore>>,
    trusted_signers: Vec<Vec<u8>>,
    capability_registries: PluginCapabilityRegistries,
    config: PluginConfig,
    event_sink: Option<Arc<dyn PluginEventSink>>,
    metrics_sink: Option<Arc<dyn PluginMetricsSink>>,
    tenant_id: Option<TenantId>,
}

pub trait PluginEventSink: Send + Sync + 'static {
    fn emit(&self, event: Event);
}

pub trait PluginMetricsSink: Send + Sync + 'static {
    fn plugin_discovered(&self, _source: &str, _trust_level: TrustLevel) {}
    fn plugin_loaded(&self, _source: &str, _trust_level: TrustLevel) {}
    fn plugin_activated(&self, _source: &str, _trust_level: TrustLevel) {}
    fn plugin_rejected(&self, _source: &str, _trust_level: TrustLevel, _reason: &str) {}
    fn plugin_manifest_validation_failed(&self, _source: &str, _failure: &str) {}
    fn plugin_signature_validation_duration_ms(&self, _source: &str, _duration_ms: u64) {}
    fn plugin_signer_totals(&self, _active: usize, _revoked: usize) {}
    fn plugin_dependency_resolution_duration_ms(&self, _duration_ms: u64) {}
    fn plugin_active_total(&self, _active: usize) {}
    fn plugin_capability_registration_rejected(&self, _kind: &str, _reason: &str) {}
}

impl PluginRegistry {
    pub fn builder() -> PluginRegistryBuilder {
        PluginRegistryBuilder::default()
    }

    pub async fn discover(&self) -> Result<Vec<DiscoveredPlugin>, PluginError> {
        if !self.config.enabled {
            return Ok(Vec::new());
        }

        let mut candidates = Vec::new();
        self.emit_signer_totals().await;

        for loader in self.manifest_loaders.iter() {
            for source in self.discovery_sources.iter() {
                if !self.source_enabled(source) {
                    continue;
                }

                let report = match loader.load_report(source).await {
                    Ok(report) => report,
                    Err(error) => {
                        self.emit_manifest_validation_failed(&error);
                        return Err(error.into());
                    }
                };

                for failure in report.failures {
                    self.emit_manifest_validation_failure(&failure);
                }

                for record in report.records {
                    let plugin_id = record.manifest.plugin_id();
                    if let Err(error) = self.validate_manifest_record(&record, source) {
                        self.mark_rejected_with(&plugin_id, &error);
                        self.emit_plugin_rejected(&record, &error);
                        continue;
                    }

                    candidates.push(DiscoveredPlugin {
                        record,
                        source: source.clone(),
                    });
                }
            }
        }

        let candidates = self.filter_source_priority(candidates);
        let mut discovered = Vec::new();

        for plugin in candidates {
            let plugin_id = plugin.record.manifest.plugin_id();
            let signature_started = Instant::now();
            if let Err(error) = self
                .manifest_signer
                .verify_manifest(&plugin.record.manifest)
                .await
            {
                self.emit_signature_validation_duration(
                    &plugin.record,
                    signature_started.elapsed().as_millis() as u64,
                );
                self.mark_rejected_with(&plugin_id, &error);
                self.emit_plugin_rejected(&plugin.record, &error);
                if is_manifest_signature_rejection(&error) {
                    continue;
                }
                return Err(error);
            }
            self.emit_signature_validation_duration(
                &plugin.record,
                signature_started.elapsed().as_millis() as u64,
            );

            let mut inner = self.inner.write();
            if !inner.activated.contains_key(&plugin_id) {
                inner
                    .state
                    .insert(plugin_id.clone(), PluginLifecycleState::Validated);
                inner.state_detail.insert(
                    plugin_id.clone(),
                    PluginLifecycleDetail {
                        state: PluginLifecycleState::Validated,
                        rejection_reason: None,
                        failure: None,
                    },
                );
            }
            inner.discovered.insert(plugin_id, plugin.clone());
            drop(inner);
            self.emit_plugin_discovered(&plugin.record);
            discovered.push(plugin);
        }

        self.record_optional_dependency_warnings();
        Ok(discovered)
    }

    fn source_enabled(&self, source: &DiscoverySource) -> bool {
        !matches!(source, DiscoverySource::Project(_)) || self.config.allow_project_plugins
    }

    fn filter_source_priority(&self, candidates: Vec<DiscoveredPlugin>) -> Vec<DiscoveredPlugin> {
        let mut max_priority_by_name: BTreeMap<PluginName, u8> = BTreeMap::new();
        for candidate in &candidates {
            let priority = source_priority(&candidate.source);
            max_priority_by_name
                .entry(candidate.record.manifest.name.clone())
                .and_modify(|current| *current = (*current).max(priority))
                .or_insert(priority);
        }

        let dependency_names = candidates
            .iter()
            .flat_map(|candidate| candidate.record.manifest.dependencies.iter())
            .map(|dependency| dependency.name.clone())
            .collect::<BTreeSet<_>>();
        let mut highest_priority_versions = BTreeMap::<PluginName, BTreeSet<String>>::new();
        for candidate in &candidates {
            let priority = source_priority(&candidate.source);
            if max_priority_by_name
                .get(&candidate.record.manifest.name)
                .is_some_and(|max_priority| priority == *max_priority)
            {
                highest_priority_versions
                    .entry(candidate.record.manifest.name.clone())
                    .or_default()
                    .insert(candidate.record.manifest.version.to_string());
            }
        }

        candidates
            .into_iter()
            .filter_map(|candidate| {
                let plugin_id = candidate.record.manifest.plugin_id();
                let source_priority = source_priority(&candidate.source);
                let max_priority = max_priority_by_name
                    .get(&candidate.record.manifest.name)
                    .copied()
                    .unwrap_or(source_priority);
                if source_priority < max_priority {
                    let error = PluginError::NamespaceConflict {
                        details: format!(
                            "plugin {} from {} shadowed by higher priority source",
                            candidate.record.manifest.name, candidate.source
                        ),
                    };
                    self.mark_rejected_with(&plugin_id, &error);
                    self.emit_plugin_rejected(&candidate.record, &error);
                    None
                } else if highest_priority_versions
                    .get(&candidate.record.manifest.name)
                    .is_some_and(|versions| versions.len() > 1)
                    && !dependency_names.contains(&candidate.record.manifest.name)
                {
                    let error = PluginError::NamespaceConflict {
                        details: format!(
                            "plugin {} has multiple versions at source priority {}",
                            candidate.record.manifest.name, source_priority
                        ),
                    };
                    self.mark_rejected_with(&plugin_id, &error);
                    self.emit_plugin_rejected(&candidate.record, &error);
                    None
                } else {
                    Some(candidate)
                }
            })
            .collect()
    }

    pub async fn activate(&self, id: &PluginId) -> Result<(), PluginError> {
        let dependency_started = Instant::now();
        let activation_order = match self.resolve_activation_order(id) {
            Ok(order) => order,
            Err(error @ PluginError::DependencyUnsatisfied { .. })
            | Err(error @ PluginError::DependencyCycle(_)) => {
                self.emit_dependency_resolution_duration(
                    dependency_started.elapsed().as_millis() as u64
                );
                self.mark_rejected_with(id, &error);
                if let Some(record) = self.discovered_record(id) {
                    self.emit_plugin_rejected(&record, &error);
                }
                return Err(error);
            }
            Err(error) => {
                self.emit_dependency_resolution_duration(
                    dependency_started.elapsed().as_millis() as u64
                );
                return Err(error);
            }
        };
        self.emit_dependency_resolution_duration(dependency_started.elapsed().as_millis() as u64);

        for plugin_id in activation_order {
            if let Err(error) = self.activate_one(&plugin_id).await {
                return Err(error);
            }
        }
        Ok(())
    }

    async fn activate_one(&self, id: &PluginId) -> Result<(), PluginError> {
        let discovered = {
            let mut inner = self.inner.write();
            if inner.activated.contains_key(id) {
                return Ok(());
            }
            let discovered = inner.discovered.get(id).cloned().ok_or_else(|| {
                PluginError::ActivateFailed(format!("plugin not discovered: {}", id.0))
            })?;
            inner
                .state
                .insert(id.clone(), PluginLifecycleState::Activating);
            inner.state_detail.insert(
                id.clone(),
                PluginLifecycleDetail {
                    state: PluginLifecycleState::Activating,
                    rejection_reason: None,
                    failure: None,
                },
            );
            discovered
        };

        if let Err(error) = self.validate_activation_requirements(&discovered.record.manifest) {
            let error = PluginError::Registration(error);
            self.mark_failed_with(id, error.to_string());
            self.emit_plugin_rejected(&discovered.record, &error);
            return Err(error);
        }

        let plugin = match self.load_plugin(&discovered.record).await {
            Ok(plugin) => plugin,
            Err(error) => {
                self.mark_failed_with(id, error.to_string());
                self.emit_plugin_rejected(&discovered.record, &error);
                return Err(error);
            }
        };

        let activation = Arc::new(CapabilityRegistrationState::default());
        let ctx = self.activation_context(&discovered.record.manifest, Arc::clone(&activation));
        let result = match plugin.activate(ctx).await {
            Ok(result) => result,
            Err(error) => {
                self.rollback_activation(id, &activation).await;
                self.mark_failed_with(id, error.to_string());
                self.emit_plugin_rejected(&discovered.record, &error);
                return Err(error);
            }
        };

        if let Err(error) =
            validate_activation_result(&discovered.record.manifest, &result, &activation)
        {
            self.rollback_activation(id, &activation).await;
            let error = PluginError::Registration(error);
            self.mark_failed_with(id, error.to_string());
            self.emit_capability_registration_rejected(&error);
            self.emit_plugin_rejected(&discovered.record, &error);
            return Err(error);
        }

        if let Err(error) = self.occupy_slots(id, &result.occupied_slots) {
            self.rollback_activation(id, &activation).await;
            self.mark_failed_with(id, error.to_string());
            self.emit_capability_registration_rejected(&error);
            self.emit_plugin_rejected(&discovered.record, &error);
            return Err(error);
        }

        let memory_provider = activation.memory_provider();
        let registrations = CapabilityRegistrations::from_state(&activation);
        let activation_warnings = capability_warnings(
            &discovered.record.manifest.capabilities,
            &activation,
            &result.occupied_slots,
        );
        let mut inner = self.inner.write();
        if memory_provider.is_some() {
            inner.memory_provider = memory_provider.clone();
        }
        if !activation_warnings.is_empty() {
            inner
                .warnings
                .entry(id.clone())
                .or_default()
                .extend(activation_warnings);
        }
        inner.activated.insert(
            id.clone(),
            ActivatedPlugin {
                plugin,
                slots: result.occupied_slots,
                registrations,
                memory_provider,
            },
        );
        inner
            .state
            .insert(id.clone(), PluginLifecycleState::Activated);
        inner.state_detail.insert(
            id.clone(),
            PluginLifecycleDetail {
                state: PluginLifecycleState::Activated,
                rejection_reason: None,
                failure: None,
            },
        );
        drop(inner);
        self.emit_plugin_loaded(
            &discovered.record,
            PluginLifecycleStateDiscriminant::Validated,
        );
        self.emit_plugin_activated(&discovered.record);
        self.emit_active_total();
        Ok(())
    }

    pub async fn deactivate(&self, id: &PluginId) -> Result<(), PluginError> {
        let dependents = self.active_dependents(id);
        if !dependents.is_empty() {
            return Err(PluginError::ActiveDependents(dependents));
        }
        self.deactivate_one(id).await
    }

    pub async fn deactivate_cascade(&self, id: &PluginId) -> Result<(), PluginError> {
        let mut order = Vec::new();
        let mut visited = BTreeSet::new();
        self.collect_deactivation_order(id, &mut visited, &mut order);
        for plugin_id in order {
            self.deactivate_one(&plugin_id).await?;
        }
        Ok(())
    }

    async fn deactivate_one(&self, id: &PluginId) -> Result<(), PluginError> {
        let activated = {
            let mut inner = self.inner.write();
            let Some(activated) = inner.activated.remove(id) else {
                if inner.state.contains_key(id) {
                    inner
                        .state
                        .insert(id.clone(), PluginLifecycleState::Deactivated);
                    inner.state_detail.insert(
                        id.clone(),
                        PluginLifecycleDetail {
                            state: PluginLifecycleState::Deactivated,
                            rejection_reason: None,
                            failure: None,
                        },
                    );
                }
                return Ok(());
            };
            inner
                .state
                .insert(id.clone(), PluginLifecycleState::Deactivating);
            inner.state_detail.insert(
                id.clone(),
                PluginLifecycleDetail {
                    state: PluginLifecycleState::Deactivating,
                    rejection_reason: None,
                    failure: None,
                },
            );
            activated
        };

        self.unregister_capabilities(id, &activated.registrations)
            .await;

        {
            let mut inner = self.inner.write();
            for slot in &activated.slots {
                inner.slots.release(slot, id);
            }
            if activated.memory_provider.is_some() {
                inner.memory_provider = None;
            }
        }

        let deactivate_error = activated.plugin.deactivate().await.err();

        let mut inner = self.inner.write();
        if let Some(error) = deactivate_error {
            let details = error.to_string();
            inner
                .state
                .insert(id.clone(), PluginLifecycleState::Failed(details.clone()));
            inner.state_detail.insert(
                id.clone(),
                PluginLifecycleDetail {
                    state: PluginLifecycleState::Failed(details.clone()),
                    rejection_reason: None,
                    failure: Some(details.clone()),
                },
            );
            return Err(PluginError::DeactivateFailed(details));
        }
        inner
            .state
            .insert(id.clone(), PluginLifecycleState::Deactivated);
        inner.state_detail.insert(
            id.clone(),
            PluginLifecycleDetail {
                state: PluginLifecycleState::Deactivated,
                rejection_reason: None,
                failure: None,
            },
        );
        drop(inner);
        self.emit_active_total();
        Ok(())
    }

    fn active_dependents(&self, id: &PluginId) -> Vec<PluginId> {
        let inner = self.inner.read();
        active_dependents_in(&inner.discovered, &inner.activated, id)
    }

    fn collect_deactivation_order(
        &self,
        id: &PluginId,
        visited: &mut BTreeSet<PluginId>,
        order: &mut Vec<PluginId>,
    ) {
        if !visited.insert(id.clone()) {
            return;
        }
        for dependent in self.active_dependents(id) {
            self.collect_deactivation_order(&dependent, visited, order);
        }
        order.push(id.clone());
    }

    pub fn list_activated(&self) -> Vec<PluginManifest> {
        self.inner
            .read()
            .activated
            .values()
            .map(|activated| activated.plugin.manifest().clone())
            .collect()
    }

    pub fn snapshot(&self) -> PluginRegistrySnapshot {
        let inner = self.inner.read();
        PluginRegistrySnapshot {
            discovered: inner.discovered.keys().cloned().collect(),
            activated: inner.activated.keys().cloned().collect(),
            states: inner.state.clone(),
            occupied_slots: inner.slots.occupied.clone(),
            warnings: inner.warnings.clone(),
        }
    }

    pub fn state(&self, id: &PluginId) -> Option<PluginLifecycleState> {
        self.inner.read().state.get(id).cloned()
    }

    pub fn state_detail(&self, id: &PluginId) -> Option<PluginLifecycleDetail> {
        self.inner.read().state_detail.get(id).cloned()
    }

    pub fn set_capability_registries(&self, registries: PluginCapabilityRegistries) {
        *self.capability_registries.write() = registries;
    }

    pub fn registered_memory_provider(&self) -> Option<Arc<dyn harness_memory::MemoryProvider>> {
        self.inner.read().memory_provider.clone()
    }

    pub fn activation_context_for_test(
        &self,
        manifest: &PluginManifest,
    ) -> PluginActivationContext {
        self.activation_context(manifest, Arc::new(CapabilityRegistrationState::default()))
    }

    fn activation_context(
        &self,
        manifest: &PluginManifest,
        activation: Arc<CapabilityRegistrationState>,
    ) -> PluginActivationContext {
        let registries = self.capability_registries.read().clone();
        let config = self
            .config
            .entries
            .get(&manifest.name)
            .cloned()
            .unwrap_or(Value::Null);
        PluginActivationContext {
            trust_level: manifest.trust_level,
            plugin_id: manifest.plugin_id(),
            config,
            workspace_root: self.config.workspace_root.clone(),
            tools: (!manifest.capabilities.tools.is_empty()).then(|| {
                Arc::new(ScopedToolRegistration::new(
                    manifest,
                    registries.tools.clone(),
                    Arc::clone(&activation),
                    self.metrics_sink.clone(),
                )) as Arc<_>
            }),
            hooks: (!manifest.capabilities.hooks.is_empty()).then(|| {
                Arc::new(ScopedHookRegistration::new(
                    manifest,
                    registries.hooks.clone(),
                    Arc::clone(&activation),
                    self.metrics_sink.clone(),
                )) as Arc<_>
            }),
            mcp: (!manifest.capabilities.mcp_servers.is_empty()).then(|| {
                Arc::new(ScopedMcpRegistration::new(
                    manifest,
                    registries.mcp.clone(),
                    Arc::clone(&activation),
                    self.metrics_sink.clone(),
                )) as Arc<_>
            }),
            skills: (!manifest.capabilities.skills.is_empty()).then(|| {
                Arc::new(ScopedSkillRegistration::new(
                    manifest,
                    registries.skills.clone(),
                    Arc::clone(&activation),
                    self.metrics_sink.clone(),
                )) as Arc<_>
            }),
            memory: manifest.capabilities.memory_provider.is_some().then(|| {
                Arc::new(ScopedMemoryProviderRegistration::new(Arc::clone(
                    &activation,
                ))) as Arc<_>
            }),
            coordinator: manifest
                .capabilities
                .coordinator_strategy
                .is_some()
                .then(|| {
                    Arc::new(ScopedCoordinatorStrategyRegistration::new(Arc::clone(
                        &activation,
                    ))) as Arc<_>
                }),
            steering: manifest.capabilities.steering.then(|| {
                Arc::new(ScopedSteeringRegistration::new(
                    manifest.plugin_id(),
                    registries.steering.clone(),
                )) as Arc<_>
            }),
        }
    }

    fn validate_activation_requirements(
        &self,
        manifest: &PluginManifest,
    ) -> Result<(), RegistrationError> {
        let registries = self.capability_registries.read();
        if manifest.capabilities.steering && registries.steering.is_none() {
            return Err(RegistrationError::OwnerRegistry {
                kind: "steering",
                details: "steering registration is not wired".to_owned(),
            });
        }
        Ok(())
    }

    fn validate_manifest_record(
        &self,
        record: &ManifestRecord,
        source: &DiscoverySource,
    ) -> Result<(), PluginError> {
        if let Some(expected) = source_expected_trust(source) {
            if record.manifest.trust_level != expected {
                return Err(PluginError::TrustMismatch {
                    declared: record.manifest.trust_level,
                    source_label: source.to_string(),
                });
            }
        }

        if is_reserved_prefix(&record.manifest.name)
            && record.manifest.trust_level != harness_contracts::TrustLevel::AdminTrusted
        {
            return Err(PluginError::NamespaceConflict {
                details: format!(
                    "reserved plugin prefix requires AdminTrusted source: {}",
                    record.manifest.name
                ),
            });
        }

        if record.manifest.capabilities.steering
            && record.manifest.trust_level != harness_contracts::TrustLevel::AdminTrusted
        {
            return Err(RegistrationError::TrustViolation {
                capability: "steering",
                details: "UserControlled plugins cannot declare steering capability".to_owned(),
            }
            .into());
        }

        validate_semver_manifest(&record.manifest)?;
        validate_admission(&self.config.policy, &record.manifest)?;
        validate_strict_plugin_only(
            &self.config.strict_plugin_only_customization,
            &record.manifest,
        )?;
        validate_plugin_config_entry(&self.config, &record.manifest)?;
        Ok(())
    }

    fn record_optional_dependency_warnings(&self) {
        let discovered = self.inner.read().discovered.clone();
        let mut warnings = BTreeMap::<PluginId, Vec<PluginWarning>>::new();
        for (plugin_id, plugin) in &discovered {
            for dependency in &plugin.record.manifest.dependencies {
                if dependency.kind != PluginDependencyKind::Optional {
                    continue;
                }
                if find_dependency_candidate(&discovered, &dependency.name, &dependency.version_req)
                    .is_none()
                {
                    warnings.entry(plugin_id.clone()).or_default().push(
                        PluginWarning::OptionalDependencyMissing {
                            dependency: dependency.name.to_string(),
                            requirement: dependency.version_req.to_string(),
                        },
                    );
                }
            }
        }
        self.inner.write().warnings.extend(warnings);
    }

    async fn load_plugin(&self, record: &ManifestRecord) -> Result<Arc<dyn Plugin>, PluginError> {
        for loader in self.runtime_loaders.iter() {
            if loader.can_load(&record.manifest, &record.origin) {
                let plugin = loader.load(&record.manifest, &record.origin).await?;
                if plugin.manifest() != &record.manifest {
                    return Err(RuntimeLoaderError::LoadFailed(format!(
                        "manifest mismatch: expected {}, got {}",
                        record.manifest.plugin_id().0,
                        plugin.manifest().plugin_id().0
                    ))
                    .into());
                }
                return Ok(plugin);
            }
        }

        Err(PluginError::ActivateFailed(format!(
            "no runtime loader can handle origin: {}",
            record.origin
        )))
    }

    fn occupy_slots(&self, id: &PluginId, slots: &[CapabilitySlot]) -> Result<(), PluginError> {
        let mut inner = self.inner.write();
        for slot in slots {
            if let Err(error) = inner.slots.try_occupy(slot.clone(), id) {
                for occupied in slots {
                    inner.slots.release(occupied, id);
                    if occupied == slot {
                        break;
                    }
                }
                return Err(error);
            }
        }
        Ok(())
    }

    fn mark_failed_with(&self, id: &PluginId, failure: impl Into<String>) {
        let failure = failure.into();
        let mut inner = self.inner.write();
        inner
            .state
            .insert(id.clone(), PluginLifecycleState::Failed(failure.clone()));
        inner.state_detail.insert(
            id.clone(),
            PluginLifecycleDetail {
                state: PluginLifecycleState::Failed(failure.clone()),
                rejection_reason: None,
                failure: Some(failure),
            },
        );
    }

    fn mark_rejected_with(&self, id: &PluginId, error: &PluginError) {
        let reason = rejection_reason(error);
        let mut inner = self.inner.write();
        inner
            .state
            .insert(id.clone(), PluginLifecycleState::Rejected(reason.clone()));
        inner.state_detail.insert(
            id.clone(),
            PluginLifecycleDetail {
                state: PluginLifecycleState::Rejected(reason.clone()),
                rejection_reason: Some(reason),
                failure: None,
            },
        );
    }

    fn discovered_record(&self, id: &PluginId) -> Option<ManifestRecord> {
        self.inner
            .read()
            .discovered
            .get(id)
            .map(|plugin| plugin.record.clone())
    }

    async fn rollback_activation(
        &self,
        plugin_id: &PluginId,
        activation: &CapabilityRegistrationState,
    ) {
        let registrations = CapabilityRegistrations::from_state(activation);
        self.unregister_capabilities(plugin_id, &registrations)
            .await;
    }

    async fn unregister_capabilities(
        &self,
        plugin_id: &PluginId,
        registrations: &CapabilityRegistrations,
    ) {
        let registries = self.capability_registries.read().clone();
        if let Some(registry) = &registries.tools {
            for name in &registrations.tools {
                let _ = registry.deregister(name);
            }
        }
        if let Some(registry) = &registries.hooks {
            for name in &registrations.hooks {
                registry.deregister(name);
            }
        }
        if let Some(registry) = &registries.mcp {
            for name in &registrations.mcp {
                let _ = registry
                    .remove_server(&harness_contracts::McpServerId(name.clone()))
                    .await;
            }
        }
        if let Some(registry) = &registries.skills {
            for name in &registrations.skills {
                let handler_ids = registry.deregister_from_plugin(plugin_id, name);
                if let Some(hooks) = &registries.hooks {
                    for handler_id in handler_ids {
                        hooks.deregister(&handler_id);
                    }
                }
            }
        }
    }

    fn emit_plugin_loaded(
        &self,
        record: &ManifestRecord,
        from_state: PluginLifecycleStateDiscriminant,
    ) {
        let manifest = &record.manifest;
        if let Some(metrics) = &self.metrics_sink {
            metrics.plugin_loaded(
                manifest_origin_metric_label(&record.origin),
                manifest.trust_level,
            );
        }
        let Some(sink) = &self.event_sink else {
            return;
        };
        sink.emit(Event::PluginLoaded(PluginLoadedEvent {
            tenant_id: self.tenant_id,
            plugin_id: manifest.plugin_id(),
            plugin_name: manifest.name.to_string(),
            plugin_version: manifest.version.to_string(),
            trust_level: manifest.trust_level,
            capabilities: plugin_capabilities_summary(manifest),
            manifest_origin: manifest_origin_ref(&record.origin),
            manifest_hash: record.manifest_hash,
            from_state,
            at: now(),
        }));
    }

    fn emit_plugin_discovered(&self, record: &ManifestRecord) {
        if let Some(metrics) = &self.metrics_sink {
            metrics.plugin_discovered(
                manifest_origin_metric_label(&record.origin),
                record.manifest.trust_level,
            );
        }
    }

    fn emit_plugin_activated(&self, record: &ManifestRecord) {
        if let Some(metrics) = &self.metrics_sink {
            metrics.plugin_activated(
                manifest_origin_metric_label(&record.origin),
                record.manifest.trust_level,
            );
        }
    }

    fn emit_signature_validation_duration(&self, record: &ManifestRecord, duration_ms: u64) {
        if let Some(metrics) = &self.metrics_sink {
            metrics.plugin_signature_validation_duration_ms(
                manifest_origin_metric_label(&record.origin),
                duration_ms,
            );
        }
    }

    async fn emit_signer_totals(&self) {
        let Some(metrics) = &self.metrics_sink else {
            return;
        };
        if let Ok((active, revoked)) = self.manifest_signer.signer_counts().await {
            metrics.plugin_signer_totals(active, revoked);
        }
    }

    fn emit_dependency_resolution_duration(&self, duration_ms: u64) {
        if let Some(metrics) = &self.metrics_sink {
            metrics.plugin_dependency_resolution_duration_ms(duration_ms);
        }
    }

    fn emit_active_total(&self) {
        if let Some(metrics) = &self.metrics_sink {
            metrics.plugin_active_total(self.inner.read().activated.len());
        }
    }

    fn emit_capability_registration_rejected(&self, error: &PluginError) {
        let Some(metrics) = &self.metrics_sink else {
            return;
        };
        match error {
            PluginError::Registration(registration) => metrics
                .plugin_capability_registration_rejected(
                    registration_metric_kind(registration),
                    "registration",
                ),
            PluginError::SlotOccupied { .. } => {
                metrics.plugin_capability_registration_rejected("slot", "slot_occupied");
            }
            _ => {}
        }
    }

    fn emit_plugin_rejected(&self, record: &ManifestRecord, error: &PluginError) {
        let manifest = &record.manifest;
        let reason = rejection_reason(error);
        if let Some(metrics) = &self.metrics_sink {
            metrics.plugin_rejected(
                manifest_origin_metric_label(&record.origin),
                manifest.trust_level,
                rejection_reason_metric_label(&reason),
            );
        }
        let Some(sink) = &self.event_sink else {
            return;
        };
        sink.emit(Event::PluginRejected(PluginRejectedEvent {
            tenant_id: self.tenant_id,
            plugin_id: manifest.plugin_id(),
            plugin_name: manifest.name.to_string(),
            plugin_version: manifest.version.to_string(),
            trust_level: manifest.trust_level,
            manifest_origin: manifest_origin_ref(&record.origin),
            manifest_hash: record.manifest_hash,
            reason,
            at: now(),
        }));
    }

    fn emit_manifest_validation_failed(&self, error: &ManifestLoaderError) {
        let ManifestLoaderError::Validation(failure) = error else {
            return;
        };
        self.emit_manifest_validation_failure(failure);
    }

    fn emit_manifest_validation_failure(&self, failure: &crate::ManifestValidationFailure) {
        if let Some(metrics) = &self.metrics_sink {
            metrics.plugin_manifest_validation_failed(
                failure
                    .origin
                    .as_ref()
                    .map(manifest_origin_metric_label)
                    .unwrap_or("unknown"),
                manifest_failure_metric_label(&failure.failure),
            );
        }
        let Some(sink) = &self.event_sink else {
            return;
        };
        sink.emit(Event::ManifestValidationFailed(
            ManifestValidationFailedEvent {
                tenant_id: self.tenant_id,
                manifest_origin: failure
                    .origin
                    .as_ref()
                    .map(manifest_origin_ref)
                    .unwrap_or_else(|| ManifestOriginRef::File {
                        path: "<unknown>".to_owned(),
                    }),
                partial_name: failure.partial_name.clone(),
                partial_version: failure.partial_version.clone(),
                raw_bytes_hash: failure.raw_bytes_hash,
                failure: failure.failure.clone(),
                at: now(),
            },
        ));
    }

    fn resolve_activation_order(&self, root: &PluginId) -> Result<Vec<PluginId>, PluginError> {
        let inner = self.inner.read();
        if !inner.discovered.contains_key(root) {
            return Err(PluginError::ActivateFailed(format!(
                "plugin not discovered: {}",
                root.0
            )));
        }

        let mut order = Vec::new();
        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        let mut selected = BTreeMap::new();
        resolve_plugin_dependencies(
            root,
            &inner.discovered,
            &mut visiting,
            &mut visited,
            &mut selected,
            &mut order,
        )?;
        Ok(order)
    }
}

fn resolve_plugin_dependencies(
    id: &PluginId,
    discovered: &BTreeMap<PluginId, DiscoveredPlugin>,
    visiting: &mut BTreeSet<PluginId>,
    visited: &mut BTreeSet<PluginId>,
    selected: &mut BTreeMap<PluginName, PluginId>,
    order: &mut Vec<PluginId>,
) -> Result<(), PluginError> {
    if visited.contains(id) {
        return Ok(());
    }
    if !visiting.insert(id.clone()) {
        return Err(PluginError::DependencyCycle(vec![id.0.clone()]));
    }

    let plugin = discovered
        .get(id)
        .ok_or_else(|| PluginError::ActivateFailed(format!("plugin not discovered: {}", id.0)))?;
    for dependency in &plugin.record.manifest.dependencies {
        if dependency.kind != PluginDependencyKind::Required {
            continue;
        }
        if let Some(selected_id) = selected.get(&dependency.name).cloned() {
            let selected_plugin = discovered.get(&selected_id).ok_or_else(|| {
                PluginError::ActivateFailed(format!("plugin not discovered: {}", selected_id.0))
            })?;
            if !version_satisfies(
                &selected_plugin.record.manifest.version,
                &dependency.version_req,
            ) {
                return Err(PluginError::DependencyUnsatisfied {
                    dependency: dependency.name.to_string(),
                    requirement: dependency.version_req.to_string(),
                });
            }
            resolve_plugin_dependencies(
                &selected_id,
                discovered,
                visiting,
                visited,
                selected,
                order,
            )?;
            continue;
        }
        let Some(dependency_id) =
            find_dependency_candidate(discovered, &dependency.name, &dependency.version_req)
        else {
            return Err(PluginError::DependencyUnsatisfied {
                dependency: dependency.name.to_string(),
                requirement: dependency.version_req.to_string(),
            });
        };
        if visiting.contains(&dependency_id) {
            return Err(PluginError::DependencyCycle(vec![
                id.0.clone(),
                dependency_id.0,
            ]));
        }
        selected.insert(dependency.name.clone(), dependency_id.clone());
        resolve_plugin_dependencies(
            &dependency_id,
            discovered,
            visiting,
            visited,
            selected,
            order,
        )?;
    }

    visiting.remove(id);
    visited.insert(id.clone());
    order.push(id.clone());
    Ok(())
}

fn find_dependency_candidate(
    discovered: &BTreeMap<PluginId, DiscoveredPlugin>,
    name: &crate::PluginName,
    version_req: &semver::VersionReq,
) -> Option<PluginId> {
    discovered
        .iter()
        .filter_map(|(id, plugin)| {
            if &plugin.record.manifest.name != name {
                return None;
            }
            let version = plugin.record.manifest.version.clone();
            version_req
                .matches(&version)
                .then_some((version, id.clone()))
        })
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, id)| id)
}

fn version_satisfies(version: &semver::Version, requirement: &semver::VersionReq) -> bool {
    requirement.matches(&version)
}

fn active_dependents_in(
    discovered: &BTreeMap<PluginId, DiscoveredPlugin>,
    activated: &BTreeMap<PluginId, ActivatedPlugin>,
    id: &PluginId,
) -> Vec<PluginId> {
    let Some(target) = discovered.get(id) else {
        return Vec::new();
    };
    let mut dependents = activated
        .keys()
        .filter(|candidate_id| *candidate_id != id)
        .filter_map(|candidate_id| {
            let candidate = discovered.get(candidate_id)?;
            candidate
                .record
                .manifest
                .dependencies
                .iter()
                .any(|dependency| {
                    dependency.kind == PluginDependencyKind::Required
                        && dependency.name == target.record.manifest.name
                        && version_satisfies(
                            &target.record.manifest.version,
                            &dependency.version_req,
                        )
                })
                .then_some(candidate_id.clone())
        })
        .collect::<Vec<_>>();
    dependents.sort();
    dependents
}

fn validate_semver_manifest(manifest: &PluginManifest) -> Result<(), PluginError> {
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(|error| {
        PluginError::InvalidManifest(format!("invalid harness package version: {error}"))
    })?;
    if !manifest.min_harness_version.matches(&current) {
        return Err(PluginError::HarnessVersionIncompatible {
            required: manifest.min_harness_version.to_string(),
            actual: current.to_string(),
        });
    }
    Ok(())
}

fn validate_admission(
    policy: &PluginAdmissionPolicy,
    manifest: &PluginManifest,
) -> Result<(), PluginError> {
    match policy {
        PluginAdmissionPolicy::AllowAll => Ok(()),
        PluginAdmissionPolicy::Allow(allowed) if allowed.contains(&manifest.name) => Ok(()),
        PluginAdmissionPolicy::Allow(_) => Err(PluginError::AdmissionDenied {
            policy: format!("allowlist:{}", manifest.name),
        }),
        PluginAdmissionPolicy::Deny(denied) if denied.contains(&manifest.name) => {
            Err(PluginError::AdmissionDenied {
                policy: format!("denylist:{}", manifest.name),
            })
        }
        PluginAdmissionPolicy::Deny(_) => Ok(()),
    }
}

fn validate_strict_plugin_only(
    policy: &StrictPluginOnlyPolicy,
    manifest: &PluginManifest,
) -> Result<(), PluginError> {
    if policy.enabled
        && manifest.trust_level == harness_contracts::TrustLevel::UserControlled
        && !manifest.capabilities.tools.is_empty()
    {
        return Err(PluginError::AdmissionDenied {
            policy: format!("strict_plugin_only:{}", manifest.name),
        });
    }
    Ok(())
}

fn validate_plugin_config_entry(
    config: &PluginConfig,
    manifest: &PluginManifest,
) -> Result<(), PluginError> {
    let Some(schema) = &manifest.capabilities.configuration_schema else {
        return Ok(());
    };
    let validator = jsonschema::validator_for(schema).map_err(|error| {
        PluginError::InvalidManifest(format!(
            "configuration_schema cannot compile for {}: {error}",
            manifest.plugin_id().0
        ))
    })?;
    let entry = config.entries.get(&manifest.name).unwrap_or(&Value::Null);
    if validator.is_valid(entry) {
        return Ok(());
    }
    let details = validator.iter_errors(entry).next().map_or_else(
        || "configuration entry does not match schema".to_owned(),
        |error| error.to_string(),
    );
    Err(PluginError::AdmissionDenied {
        policy: format!("config_schema:{}:{details}", manifest.name),
    })
}

fn source_expected_trust(source: &DiscoverySource) -> Option<harness_contracts::TrustLevel> {
    match source {
        DiscoverySource::Workspace(_) => Some(harness_contracts::TrustLevel::AdminTrusted),
        DiscoverySource::User(_) | DiscoverySource::Project(_) => {
            Some(harness_contracts::TrustLevel::UserControlled)
        }
        DiscoverySource::CargoExtension | DiscoverySource::Inline => None,
    }
}

fn source_priority(source: &DiscoverySource) -> u8 {
    match source {
        DiscoverySource::Workspace(_) => 5,
        DiscoverySource::CargoExtension => 4,
        DiscoverySource::User(_) => 3,
        DiscoverySource::Project(_) => 2,
        DiscoverySource::Inline => 1,
    }
}

fn is_reserved_prefix(name: &PluginName) -> bool {
    let name = name.as_str();
    name.starts_with("jyowo-") || name.starts_with("harness-") || name.starts_with("mcp-")
}

fn capability_warnings(
    capabilities: &PluginCapabilities,
    activation: &CapabilityRegistrationState,
    occupied_slots: &[CapabilitySlot],
) -> Vec<PluginWarning> {
    let registered_tools = activation
        .registered_tools()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let registered_hooks = activation
        .registered_hooks()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let registered_mcp = activation
        .registered_mcp()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let registered_skills = activation
        .registered_skills()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let occupied_custom_toolsets = occupied_slots
        .iter()
        .filter_map(|slot| match slot {
            CapabilitySlot::CustomToolset(name) => Some(name.clone()),
            CapabilitySlot::MemoryProvider | CapabilitySlot::CoordinatorStrategy => None,
        })
        .collect::<BTreeSet<_>>();
    let mut warnings = Vec::new();
    for tool in &capabilities.tools {
        if !registered_tools.contains(&tool.name) {
            warnings.push(PluginWarning::DeclaredCapabilityUnregistered {
                kind: "tool",
                name: tool.name.clone(),
            });
        }
    }
    for hook in &capabilities.hooks {
        if !registered_hooks.contains(&hook.name) {
            warnings.push(PluginWarning::DeclaredCapabilityUnregistered {
                kind: "hook",
                name: hook.name.clone(),
            });
        }
    }
    for server in &capabilities.mcp_servers {
        if !registered_mcp.contains(&server.name) {
            warnings.push(PluginWarning::DeclaredCapabilityUnregistered {
                kind: "mcp",
                name: server.name.clone(),
            });
        }
    }
    for skill in &capabilities.skills {
        if !registered_skills.contains(&skill.name) {
            warnings.push(PluginWarning::DeclaredCapabilityUnregistered {
                kind: "skill",
                name: skill.name.clone(),
            });
        }
    }
    for toolset in &capabilities.custom_toolsets {
        if !occupied_custom_toolsets.contains(&toolset.name) {
            warnings.push(PluginWarning::DeclaredCapabilityUnregistered {
                kind: "custom_toolset",
                name: toolset.name.clone(),
            });
        }
    }
    if capabilities.memory_provider.is_some() && !activation.memory_registered() {
        warnings.push(PluginWarning::DeclaredCapabilityUnregistered {
            kind: "memory",
            name: capabilities
                .memory_provider
                .as_ref()
                .map_or_else(|| "memory".to_owned(), |entry| entry.name.clone()),
        });
    }
    if capabilities.coordinator_strategy.is_some() && !activation.coordinator_registered() {
        warnings.push(PluginWarning::DeclaredCapabilityUnregistered {
            kind: "coordinator",
            name: capabilities
                .coordinator_strategy
                .as_ref()
                .map_or_else(|| "coordinator".to_owned(), |entry| entry.name.clone()),
        });
    }
    warnings
}

fn plugin_capabilities_summary(manifest: &PluginManifest) -> PluginCapabilitiesSummary {
    PluginCapabilitiesSummary {
        tools: manifest
            .capabilities
            .tools
            .len()
            .try_into()
            .unwrap_or(u16::MAX),
        hooks: manifest
            .capabilities
            .hooks
            .len()
            .try_into()
            .unwrap_or(u16::MAX),
        mcp_servers: manifest
            .capabilities
            .mcp_servers
            .len()
            .try_into()
            .unwrap_or(u16::MAX),
        skills: manifest
            .capabilities
            .skills
            .len()
            .try_into()
            .unwrap_or(u16::MAX),
        steering: manifest.capabilities.steering,
        memory_provider: manifest.capabilities.memory_provider.is_some(),
        coordinator: manifest.capabilities.coordinator_strategy.is_some(),
    }
}

fn manifest_origin_ref(origin: &crate::ManifestOrigin) -> ManifestOriginRef {
    match origin {
        crate::ManifestOrigin::File { path } => ManifestOriginRef::File {
            path: path.display().to_string(),
        },
        crate::ManifestOrigin::CargoExtension { binary, .. } => ManifestOriginRef::CargoExtension {
            binary: binary.display().to_string(),
        },
        crate::ManifestOrigin::RemoteRegistry { endpoint, .. } => {
            ManifestOriginRef::RemoteRegistry {
                endpoint: endpoint.clone(),
            }
        }
    }
}

fn manifest_origin_metric_label(origin: &crate::ManifestOrigin) -> &'static str {
    match origin {
        crate::ManifestOrigin::File { .. } => "file",
        crate::ManifestOrigin::CargoExtension { .. } => "cargo_extension",
        crate::ManifestOrigin::RemoteRegistry { .. } => "remote_registry",
    }
}

fn rejection_reason(error: &PluginError) -> RejectionReason {
    match error {
        PluginError::SignatureInvalid { details } => RejectionReason::SignatureInvalid {
            details: details.clone(),
        },
        PluginError::UnknownSigner(signer) => RejectionReason::UnknownSigner {
            signer: signer.clone(),
        },
        PluginError::SignerRevoked { signer, revoked_at } => RejectionReason::SignerRevoked {
            signer: signer.clone(),
            revoked_at: *revoked_at,
        },
        PluginError::SlotOccupied { slot, occupant } => RejectionReason::SlotOccupied {
            slot: format!("{slot:?}"),
            occupant: occupant.0.clone(),
        },
        PluginError::DependencyUnsatisfied {
            dependency,
            requirement,
        } => RejectionReason::DependencyUnsatisfied {
            dependency: dependency.clone(),
            requirement: requirement.clone(),
        },
        PluginError::DependencyCycle(cycle) => RejectionReason::DependencyCycle {
            cycle: cycle.clone(),
        },
        PluginError::AdmissionDenied { policy } => RejectionReason::AdmissionDenied {
            policy: policy.clone(),
        },
        PluginError::NamespaceConflict { details } => RejectionReason::NamespaceConflict {
            details: details.clone(),
        },
        PluginError::TrustMismatch {
            declared,
            source_label,
        } => RejectionReason::TrustMismatch {
            declared: *declared,
            source: source_label.clone(),
        },
        PluginError::HarnessVersionIncompatible { required, actual } => {
            RejectionReason::HarnessVersionIncompatible {
                required: required.clone(),
                actual: actual.clone(),
            }
        }
        PluginError::ActiveDependents(dependents) => RejectionReason::AdmissionDenied {
            policy: format!(
                "active_dependents:{}",
                dependents
                    .iter()
                    .map(|dependent| dependent.0.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        },
        PluginError::InvalidManifest(details) => RejectionReason::AdmissionDenied {
            policy: format!("invalid_manifest:{details}"),
        },
        PluginError::Registration(RegistrationError::TrustViolation { details, .. }) => {
            RejectionReason::AdmissionDenied {
                policy: format!("trust_violation:{details}"),
            }
        }
        PluginError::Registration(RegistrationError::DescriptorMismatch { name, .. }) => {
            RejectionReason::AdmissionDenied {
                policy: format!("descriptor_mismatch:{name}"),
            }
        }
        PluginError::Registration(error) => RejectionReason::AdmissionDenied {
            policy: error.to_string(),
        },
        PluginError::ActivateFailed(details)
        | PluginError::DeactivateFailed(details)
        | PluginError::Builder(details) => RejectionReason::AdmissionDenied {
            policy: details.clone(),
        },
        PluginError::SignerStore(error) => RejectionReason::AdmissionDenied {
            policy: error.to_string(),
        },
        PluginError::ManifestLoader(ManifestLoaderError::Io(error))
        | PluginError::RuntimeLoader(RuntimeLoaderError::LoadFailed(error))
        | PluginError::RuntimeLoader(RuntimeLoaderError::UnsupportedOrigin(error)) => {
            RejectionReason::AdmissionDenied {
                policy: error.clone(),
            }
        }
        PluginError::ManifestLoader(ManifestLoaderError::UnsupportedSource(source)) => {
            RejectionReason::AdmissionDenied {
                policy: source.clone(),
            }
        }
        PluginError::ManifestLoader(ManifestLoaderError::Validation(failure)) => {
            RejectionReason::AdmissionDenied {
                policy: failure.details.clone(),
            }
        }
        PluginError::RuntimeLoader(RuntimeLoaderError::PluginNotFound(name)) => {
            RejectionReason::DependencyUnsatisfied {
                dependency: name.to_string(),
                requirement: "static runtime factory".to_owned(),
            }
        }
    }
}

fn rejection_reason_metric_label(reason: &RejectionReason) -> &'static str {
    match reason {
        RejectionReason::SignatureInvalid { .. } => "signature_invalid",
        RejectionReason::UnknownSigner { .. } => "unknown_signer",
        RejectionReason::SignerRevoked { .. } => "signer_revoked",
        RejectionReason::TrustMismatch { .. } => "trust_mismatch",
        RejectionReason::NamespaceConflict { .. } => "namespace_conflict",
        RejectionReason::DependencyUnsatisfied { .. } => "dependency_unsatisfied",
        RejectionReason::DependencyCycle { .. } => "dependency_cycle",
        RejectionReason::HarnessVersionIncompatible { .. } => "harness_version_incompatible",
        RejectionReason::SlotOccupied { .. } => "slot_occupied",
        RejectionReason::AdmissionDenied { .. } => "admission_denied",
        _ => "other",
    }
}

fn registration_metric_kind(error: &RegistrationError) -> &'static str {
    match error {
        RegistrationError::UndeclaredTool { .. }
        | RegistrationError::DescriptorMismatch { .. }
        | RegistrationError::TrustViolation {
            capability: "tool", ..
        } => "tool",
        RegistrationError::UndeclaredHook { .. }
        | RegistrationError::TrustViolation {
            capability: "hook", ..
        } => "hook",
        RegistrationError::UndeclaredMcp { .. }
        | RegistrationError::TrustViolation {
            capability: "mcp", ..
        } => "mcp",
        RegistrationError::UndeclaredSkill { .. } => "skill",
        RegistrationError::UndeclaredResult { kind, .. }
        | RegistrationError::OwnerRegistry { kind, .. } => kind,
        RegistrationError::DuplicateSlot { .. } => "slot",
        RegistrationError::TrustViolation { capability, .. } => capability,
    }
}

fn manifest_failure_metric_label(
    failure: &harness_contracts::ManifestValidationFailure,
) -> &'static str {
    match failure {
        harness_contracts::ManifestValidationFailure::SyntaxError { .. } => "syntax_error",
        harness_contracts::ManifestValidationFailure::SchemaViolation { .. } => "schema_violation",
        harness_contracts::ManifestValidationFailure::UnsupportedSchemaVersion { .. } => {
            "unsupported_schema_version"
        }
        harness_contracts::ManifestValidationFailure::CargoExtensionMetadataMalformed {
            ..
        } => "cargo_extension_metadata_malformed",
        harness_contracts::ManifestValidationFailure::RemoteIntegrityMismatch { .. } => {
            "remote_integrity_mismatch"
        }
        _ => "other",
    }
}

fn is_manifest_signature_rejection(error: &PluginError) -> bool {
    matches!(
        error,
        PluginError::SignatureInvalid { .. }
            | PluginError::UnknownSigner(_)
            | PluginError::SignerRevoked { .. }
    )
}

impl PluginRegistryBuilder {
    #[must_use]
    pub fn with_manifest_loader(mut self, loader: Arc<dyn PluginManifestLoader>) -> Self {
        self.manifest_loaders.push(loader);
        self
    }

    #[must_use]
    pub fn with_runtime_loader(mut self, loader: Arc<dyn PluginRuntimeLoader>) -> Self {
        self.runtime_loaders.push(loader);
        self
    }

    #[must_use]
    pub fn with_source(mut self, source: DiscoverySource) -> Self {
        self.discovery_sources.push(source);
        self
    }

    #[must_use]
    pub fn with_signer_store(mut self, store: Arc<dyn TrustedSignerStore>) -> Self {
        self.signer_store = Some(store);
        self
    }

    #[must_use]
    pub fn with_trusted_signer(mut self, public_key: impl Into<Vec<u8>>) -> Self {
        self.trusted_signers.push(public_key.into());
        self
    }

    pub fn build(self) -> Result<PluginRegistry, PluginError> {
        let Self {
            manifest_loaders,
            runtime_loaders,
            discovery_sources,
            signer_store,
            trusted_signers,
            capability_registries,
            config,
            event_sink,
            metrics_sink,
            tenant_id,
        } = self;

        if signer_store.is_some() && !trusted_signers.is_empty() {
            return Err(PluginError::Builder(
                "with_signer_store and with_trusted_signer are mutually exclusive".to_owned(),
            ));
        }

        let signer_store = match signer_store {
            Some(store) => store,
            None => Arc::new(StaticTrustedSignerStore::new(builder_trusted_signers(
                &trusted_signers,
            ))?),
        };

        Ok(PluginRegistry {
            inner: Arc::new(RwLock::new(PluginRegistryInner::default())),
            manifest_loaders: Arc::new(default_manifest_loaders(manifest_loaders)),
            runtime_loaders: Arc::new(default_runtime_loaders(runtime_loaders)),
            discovery_sources: Arc::new(if discovery_sources.is_empty() {
                vec![DiscoverySource::Inline]
            } else {
                discovery_sources
            }),
            manifest_signer: ManifestSigner::new(signer_store),
            config,
            capability_registries: Arc::new(RwLock::new(capability_registries)),
            event_sink,
            metrics_sink,
            tenant_id: tenant_id.unwrap_or(TenantId::SINGLE),
        })
    }

    #[must_use]
    pub fn with_config(mut self, config: PluginConfig) -> Self {
        self.config = config;
        self
    }

    #[must_use]
    pub fn with_capability_registries(mut self, registries: PluginCapabilityRegistries) -> Self {
        self.capability_registries = registries;
        self
    }

    #[must_use]
    pub fn with_event_sink(mut self, sink: Arc<dyn PluginEventSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    #[must_use]
    pub fn with_metrics_sink(mut self, sink: Arc<dyn PluginMetricsSink>) -> Self {
        self.metrics_sink = Some(sink);
        self
    }

    #[must_use]
    pub fn with_tenant_id(mut self, tenant_id: TenantId) -> Self {
        self.tenant_id = Some(tenant_id);
        self
    }
}

impl Default for PluginRegistryBuilder {
    fn default() -> Self {
        Self {
            manifest_loaders: Vec::new(),
            runtime_loaders: Vec::new(),
            discovery_sources: Vec::new(),
            signer_store: None,
            trusted_signers: Vec::new(),
            capability_registries: PluginCapabilityRegistries::default(),
            config: PluginConfig::default(),
            event_sink: None,
            metrics_sink: None,
            tenant_id: None,
        }
    }
}

fn default_manifest_loaders(
    manifest_loaders: Vec<Arc<dyn PluginManifestLoader>>,
) -> Vec<Arc<dyn PluginManifestLoader>> {
    if manifest_loaders.is_empty() {
        vec![Arc::new(crate::FileManifestLoader)]
    } else {
        manifest_loaders
    }
}

fn default_runtime_loaders(
    runtime_loaders: Vec<Arc<dyn PluginRuntimeLoader>>,
) -> Vec<Arc<dyn PluginRuntimeLoader>> {
    if runtime_loaders.is_empty() {
        vec![Arc::new(StaticLinkRuntimeLoader::default())]
    } else {
        runtime_loaders
    }
}

fn builder_trusted_signers(public_keys: &[Vec<u8>]) -> Vec<TrustedSigner> {
    public_keys
        .iter()
        .enumerate()
        .map(|(index, public_key)| TrustedSigner {
            id: crate::SignerId::new(format!("user-injected-{index}"))
                .expect("generated signer id is valid"),
            algorithm: SignatureAlgorithm::Ed25519,
            public_key: public_key.clone(),
            activated_at: chrono::DateTime::UNIX_EPOCH,
            retired_at: None,
            revoked_at: None,
            provenance: SignerProvenance::BuilderInjected,
        })
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct CapabilitySlotManager {
    occupied: HashMap<CapabilitySlot, PluginId>,
}

impl CapabilitySlotManager {
    pub fn try_occupy(
        &mut self,
        slot: CapabilitySlot,
        plugin_id: &PluginId,
    ) -> Result<(), PluginError> {
        if let Some(occupant) = self.occupied.get(&slot) {
            if occupant != plugin_id {
                return Err(PluginError::SlotOccupied {
                    slot,
                    occupant: occupant.clone(),
                });
            }
        }
        self.occupied.insert(slot, plugin_id.clone());
        Ok(())
    }

    pub fn release(&mut self, slot: &CapabilitySlot, plugin_id: &PluginId) {
        if self.occupied.get(slot) == Some(plugin_id) {
            self.occupied.remove(slot);
        }
    }
}

fn validate_activation_result(
    manifest: &PluginManifest,
    result: &PluginActivationResult,
    activation: &CapabilityRegistrationState,
) -> Result<(), RegistrationError> {
    validate_subset(
        "tool",
        result.registered_tools.iter().cloned(),
        manifest
            .capabilities
            .tools
            .iter()
            .map(|entry| entry.name.clone()),
    )?;
    validate_subset(
        "hook",
        result.registered_hooks.iter().cloned(),
        manifest
            .capabilities
            .hooks
            .iter()
            .map(|entry| entry.name.clone()),
    )?;
    validate_subset(
        "skill",
        result.registered_skills.iter().cloned(),
        manifest
            .capabilities
            .skills
            .iter()
            .map(|entry| entry.name.clone()),
    )?;
    validate_subset(
        "mcp",
        result.registered_mcp.iter().map(|id| id.0.clone()),
        manifest
            .capabilities
            .mcp_servers
            .iter()
            .map(|entry| entry.name.clone()),
    )?;
    validate_subset(
        "tool",
        activation.registered_tools(),
        manifest
            .capabilities
            .tools
            .iter()
            .map(|entry| entry.name.clone()),
    )?;
    validate_subset(
        "hook",
        activation.registered_hooks(),
        manifest
            .capabilities
            .hooks
            .iter()
            .map(|entry| entry.name.clone()),
    )?;
    validate_subset(
        "skill",
        activation.registered_skills(),
        manifest
            .capabilities
            .skills
            .iter()
            .map(|entry| entry.name.clone()),
    )?;
    validate_subset(
        "mcp",
        activation.registered_mcp(),
        manifest
            .capabilities
            .mcp_servers
            .iter()
            .map(|entry| entry.name.clone()),
    )?;
    for slot in &result.occupied_slots {
        if !slot_declared(manifest, slot) {
            return Err(RegistrationError::UndeclaredResult {
                kind: "slot",
                name: format!("{slot:?}"),
            });
        }
    }
    if activation.coordinator_registered()
        && !result
            .occupied_slots
            .contains(&CapabilitySlot::CoordinatorStrategy)
    {
        return Err(RegistrationError::UndeclaredResult {
            kind: "slot",
            name: "CoordinatorStrategy registration missing occupied slot".to_owned(),
        });
    }
    if activation.memory_registered()
        && !result
            .occupied_slots
            .contains(&CapabilitySlot::MemoryProvider)
    {
        return Err(RegistrationError::UndeclaredResult {
            kind: "slot",
            name: "MemoryProvider registration missing occupied slot".to_owned(),
        });
    }
    Ok(())
}

fn slot_declared(manifest: &PluginManifest, slot: &CapabilitySlot) -> bool {
    match slot {
        CapabilitySlot::MemoryProvider => manifest.capabilities.memory_provider.is_some(),
        CapabilitySlot::CustomToolset(name) => manifest
            .capabilities
            .custom_toolsets
            .iter()
            .any(|entry| &entry.name == name),
        CapabilitySlot::CoordinatorStrategy => manifest.capabilities.coordinator_strategy.is_some(),
    }
}

fn validate_subset(
    kind: &'static str,
    registered: impl IntoIterator<Item = String>,
    declared: impl IntoIterator<Item = String>,
) -> Result<(), RegistrationError> {
    let declared = declared
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    for name in registered {
        if !declared.contains(&name) {
            return Err(RegistrationError::UndeclaredResult { kind, name });
        }
    }
    Ok(())
}

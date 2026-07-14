use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{HookFailureMode, PluginId, SteeringId, SteeringRequest, TrustLevel};
use harness_hook::HookRegistry;
use harness_hook::{HookHandler, HookRegistrationKind};
use harness_mcp::{McpConnection, McpRegistry, McpServerSpec, TransportChoice};
use harness_skill::{Skill, SkillRegistry, SkillRegistrySnapshot};
use harness_tool::Tool;
use harness_tool::ToolRegistry;
use parking_lot::Mutex;

use crate::{CapabilitySlot, PluginManifest, PluginMetricsSink, RegistrationError};

#[derive(Clone, Default)]
pub struct PluginCapabilityRegistries {
    pub tools: Option<ToolRegistry>,
    pub hooks: Option<HookRegistry>,
    pub mcp: Option<McpRegistry>,
    pub skills: Option<SkillRegistry>,
    pub skill_reconciler: Option<Arc<dyn SkillRegistryReconciler>>,
    pub steering: Option<Arc<dyn SteeringRegistration>>,
}

pub trait SkillRegistryReconciler: Send + Sync {
    fn reconcile(
        &self,
        current: &SkillRegistrySnapshot,
        candidate: &SkillRegistrySnapshot,
    ) -> Result<(), String>;
}

impl PluginCapabilityRegistries {
    #[must_use]
    pub fn with_tool_registry(mut self, registry: ToolRegistry) -> Self {
        self.tools = Some(registry);
        self
    }

    #[must_use]
    pub fn with_hook_registry(mut self, registry: HookRegistry) -> Self {
        self.hooks = Some(registry);
        self
    }

    #[must_use]
    pub fn with_mcp_registry(mut self, registry: McpRegistry) -> Self {
        self.mcp = Some(registry);
        self
    }

    #[must_use]
    pub fn with_skill_registry(mut self, registry: SkillRegistry) -> Self {
        self.skills = Some(registry);
        self
    }

    #[must_use]
    pub fn with_skill_reconciler(mut self, reconciler: Arc<dyn SkillRegistryReconciler>) -> Self {
        self.skill_reconciler = Some(reconciler);
        self
    }

    #[must_use]
    pub fn with_steering_registration(
        mut self,
        registration: Arc<dyn SteeringRegistration>,
    ) -> Self {
        self.steering = Some(registration);
        self
    }
}

#[async_trait]
pub trait ToolRegistration: Send + Sync {
    async fn register(&self, tool: Box<dyn Tool>) -> Result<(), RegistrationError>;
    fn pending_declared(&self) -> Vec<String>;
}

#[async_trait]
pub trait HookRegistration: Send + Sync {
    async fn register(&self, handler: Box<dyn HookHandler>) -> Result<(), RegistrationError>;
    fn pending_declared(&self) -> Vec<String>;
}

#[async_trait]
pub trait McpRegistration: Send + Sync {
    async fn register(
        &self,
        server: McpServerSpec,
    ) -> Result<harness_contracts::McpServerId, RegistrationError>;

    async fn register_ready(
        &self,
        server: McpServerSpec,
        connection: Arc<dyn McpConnection>,
    ) -> Result<harness_contracts::McpServerId, RegistrationError> {
        let _ = connection;
        self.register(server).await
    }

    fn pending_declared(&self) -> Vec<String>;
}

#[async_trait]
pub trait SkillRegistration: Send + Sync {
    async fn register(&self, skill: Skill) -> Result<(), RegistrationError>;
    fn pending_declared(&self) -> Vec<String>;
}

#[async_trait]
pub trait MemoryProviderRegistration: Send + Sync {
    async fn register(
        &self,
        provider: Arc<dyn harness_memory::MemoryProvider>,
    ) -> Result<(), RegistrationError>;
}

#[async_trait]
pub trait CoordinatorStrategy: Send + Sync + 'static {}

#[async_trait]
pub trait CoordinatorStrategyRegistration: Send + Sync {
    async fn register(
        &self,
        strategy: Arc<dyn CoordinatorStrategy>,
    ) -> Result<(), RegistrationError>;
}

#[async_trait]
pub trait SteeringRegistration: Send + Sync {
    async fn push(&self, request: SteeringRequest) -> Result<SteeringId, RegistrationError>;
}

#[derive(Default)]
pub(crate) struct CapabilityRegistrationState {
    tools: Mutex<BTreeSet<String>>,
    hooks: Mutex<BTreeSet<String>>,
    mcp: Mutex<BTreeSet<String>>,
    skills: Mutex<BTreeSet<String>>,
    memory_providers: Mutex<BTreeMap<String, Arc<dyn harness_memory::MemoryProvider>>>,
    coordinator_registered: Mutex<bool>,
}

impl CapabilityRegistrationState {
    pub(crate) fn registered_tools(&self) -> Vec<String> {
        sorted_strings(&self.tools)
    }

    pub(crate) fn registered_hooks(&self) -> Vec<String> {
        sorted_strings(&self.hooks)
    }

    pub(crate) fn registered_mcp(&self) -> Vec<String> {
        sorted_strings(&self.mcp)
    }

    pub(crate) fn registered_skills(&self) -> Vec<String> {
        sorted_strings(&self.skills)
    }

    pub(crate) fn memory_providers(&self) -> Vec<Arc<dyn harness_memory::MemoryProvider>> {
        self.memory_providers.lock().values().cloned().collect()
    }

    pub(crate) fn memory_registered(&self) -> bool {
        !self.memory_providers.lock().is_empty()
    }

    pub(crate) fn coordinator_registered(&self) -> bool {
        *self.coordinator_registered.lock()
    }
}

fn sorted_strings(values: &Mutex<BTreeSet<String>>) -> Vec<String> {
    values.lock().iter().cloned().collect()
}

pub(crate) struct ScopedToolRegistration {
    declared: BTreeMap<String, bool>,
    plugin_id: PluginId,
    trust_level: TrustLevel,
    registry: Option<ToolRegistry>,
    state: Arc<CapabilityRegistrationState>,
    metrics: Option<Arc<dyn PluginMetricsSink>>,
}

impl ScopedToolRegistration {
    pub(crate) fn new(
        manifest: &PluginManifest,
        registry: Option<ToolRegistry>,
        state: Arc<CapabilityRegistrationState>,
        metrics: Option<Arc<dyn PluginMetricsSink>>,
    ) -> Self {
        Self {
            declared: manifest
                .capabilities
                .tools
                .iter()
                .map(|entry| (entry.name.clone(), entry.destructive))
                .collect(),
            plugin_id: manifest.plugin_id(),
            trust_level: manifest.trust_level,
            registry,
            state,
            metrics,
        }
    }
}

#[async_trait]
impl ToolRegistration for ScopedToolRegistration {
    async fn register(&self, tool: Box<dyn Tool>) -> Result<(), RegistrationError> {
        let name = tool.descriptor().name.clone();
        let Some(declared_destructive) = self.declared.get(&name).copied() else {
            record_capability_rejection(self.metrics.as_ref(), "tool", "undeclared_tool");
            return Err(RegistrationError::UndeclaredTool { name });
        };
        let actual_destructive = tool.descriptor().properties.is_destructive;
        if actual_destructive != declared_destructive {
            return Err(RegistrationError::DescriptorMismatch {
                name,
                declared_destructive,
                actual_destructive,
            });
        }
        if self.trust_level == TrustLevel::UserControlled && actual_destructive {
            return Err(RegistrationError::TrustViolation {
                capability: "tool",
                details: "UserControlled plugins cannot register destructive tools".to_owned(),
            });
        }
        let registered = if let Some(registry) = &self.registry {
            registry
                .register_from_plugin(self.plugin_id.clone(), self.trust_level, tool)
                .map_err(|error| RegistrationError::OwnerRegistry {
                    kind: "tool",
                    details: error.to_string(),
                })?
        } else {
            true
        };
        if registered {
            self.state.tools.lock().insert(name);
        }
        Ok(())
    }

    fn pending_declared(&self) -> Vec<String> {
        pending_map_keys(&self.declared, &self.state.tools)
    }
}

pub(crate) struct ScopedHookRegistration {
    declared: BTreeSet<String>,
    plugin_id: PluginId,
    trust_level: TrustLevel,
    registry: Option<HookRegistry>,
    state: Arc<CapabilityRegistrationState>,
    metrics: Option<Arc<dyn PluginMetricsSink>>,
}

impl ScopedHookRegistration {
    pub(crate) fn new(
        manifest: &PluginManifest,
        registry: Option<HookRegistry>,
        state: Arc<CapabilityRegistrationState>,
        metrics: Option<Arc<dyn PluginMetricsSink>>,
    ) -> Self {
        Self {
            declared: manifest
                .capabilities
                .hooks
                .iter()
                .map(|entry| entry.name.clone())
                .collect(),
            plugin_id: manifest.plugin_id(),
            trust_level: manifest.trust_level,
            registry,
            state,
            metrics,
        }
    }
}

#[async_trait]
impl HookRegistration for ScopedHookRegistration {
    async fn register(&self, handler: Box<dyn HookHandler>) -> Result<(), RegistrationError> {
        let name = handler.handler_id().to_owned();
        if !self.declared.contains(&name) {
            record_capability_rejection(self.metrics.as_ref(), "hook", "undeclared_hook");
            return Err(RegistrationError::UndeclaredHook { name });
        }
        if self.trust_level == TrustLevel::UserControlled
            && handler.failure_mode() != HookFailureMode::FailOpen
        {
            return Err(RegistrationError::TrustViolation {
                capability: "hook",
                details: "UserControlled plugin hooks must be fail-open".to_owned(),
            });
        }
        if self.trust_level == TrustLevel::UserControlled
            && handler.registration_kind() == HookRegistrationKind::Exec
        {
            return Err(RegistrationError::TrustViolation {
                capability: "hook",
                details: "UserControlled plugins cannot register exec hooks".to_owned(),
            });
        }
        if self.trust_level == TrustLevel::UserControlled
            && handler.registration_kind() == HookRegistrationKind::Http
        {
            let Some(posture) = handler.http_security_posture() else {
                return Err(RegistrationError::TrustViolation {
                    capability: "hook",
                    details: "UserControlled HTTP hooks must expose security posture".to_owned(),
                });
            };
            if !posture.allowlist_non_empty || !posture.ssrf_guard_strict {
                return Err(RegistrationError::TrustViolation {
                    capability: "hook",
                    details: "UserControlled HTTP hooks require allowlist and strict SSRF guard"
                        .to_owned(),
                });
            }
        }
        if let Some(declared_trust) = handler.declared_trust() {
            if declared_trust != self.trust_level {
                return Err(RegistrationError::TrustViolation {
                    capability: "hook",
                    details: format!(
                        "hook declared trust {declared_trust:?} does not match plugin trust {:?}",
                        self.trust_level
                    ),
                });
            }
        }
        if let Some(registry) = &self.registry {
            registry
                .register_from_plugin(self.plugin_id.clone(), self.trust_level, handler)
                .map_err(|error| RegistrationError::OwnerRegistry {
                    kind: "hook",
                    details: error.to_string(),
                })?;
        }
        self.state.hooks.lock().insert(name);
        Ok(())
    }

    fn pending_declared(&self) -> Vec<String> {
        pending(&self.declared, &self.state.hooks)
    }
}

pub(crate) struct ScopedMcpRegistration {
    declared: BTreeSet<String>,
    plugin_id: PluginId,
    trust_level: TrustLevel,
    registry: Option<McpRegistry>,
    state: Arc<CapabilityRegistrationState>,
    metrics: Option<Arc<dyn PluginMetricsSink>>,
}

impl ScopedMcpRegistration {
    pub(crate) fn new(
        manifest: &PluginManifest,
        registry: Option<McpRegistry>,
        state: Arc<CapabilityRegistrationState>,
        metrics: Option<Arc<dyn PluginMetricsSink>>,
    ) -> Self {
        Self {
            declared: manifest
                .capabilities
                .mcp_servers
                .iter()
                .map(|entry| entry.name.clone())
                .collect(),
            plugin_id: manifest.plugin_id(),
            trust_level: manifest.trust_level,
            registry,
            state,
            metrics,
        }
    }
}

#[async_trait]
impl McpRegistration for ScopedMcpRegistration {
    async fn register(
        &self,
        server: McpServerSpec,
    ) -> Result<harness_contracts::McpServerId, RegistrationError> {
        self.validate_server(&server)?;
        if let Some(registry) = &self.registry {
            registry
                .add_plugin_server(self.plugin_id.clone(), self.trust_level, server.clone())
                .await
                .map_err(|error| RegistrationError::OwnerRegistry {
                    kind: "mcp",
                    details: error.to_string(),
                })?;
        }
        self.state.mcp.lock().insert(server.server_id.0.clone());
        Ok(server.server_id)
    }

    async fn register_ready(
        &self,
        server: McpServerSpec,
        connection: Arc<dyn McpConnection>,
    ) -> Result<harness_contracts::McpServerId, RegistrationError> {
        self.validate_server(&server)?;
        if let Some(registry) = &self.registry {
            registry
                .add_ready_plugin_server(
                    self.plugin_id.clone(),
                    self.trust_level,
                    server.clone(),
                    connection,
                )
                .await
                .map_err(|error| RegistrationError::OwnerRegistry {
                    kind: "mcp",
                    details: error.to_string(),
                })?;
        }
        self.state.mcp.lock().insert(server.server_id.0.clone());
        Ok(server.server_id)
    }

    fn pending_declared(&self) -> Vec<String> {
        pending(&self.declared, &self.state.mcp)
    }
}

impl ScopedMcpRegistration {
    fn validate_server(&self, server: &McpServerSpec) -> Result<(), RegistrationError> {
        let name = server.server_id.0.clone();
        if !self.declared.contains(&name) {
            record_capability_rejection(self.metrics.as_ref(), "mcp", "undeclared_mcp");
            return Err(RegistrationError::UndeclaredMcp { name });
        }
        if self.trust_level == TrustLevel::UserControlled
            && matches!(
                server.transport,
                TransportChoice::Http { .. }
                    | TransportChoice::WebSocket { .. }
                    | TransportChoice::Sse { .. }
            )
        {
            return Err(RegistrationError::TrustViolation {
                capability: "mcp",
                details: "UserControlled plugins cannot register remote MCP transports".to_owned(),
            });
        }
        Ok(())
    }
}

pub(crate) struct ScopedSkillRegistration {
    declared: BTreeSet<String>,
    plugin_id: PluginId,
    trust_level: TrustLevel,
    registry: Option<SkillRegistry>,
    reconciler: Option<Arc<dyn SkillRegistryReconciler>>,
    state: Arc<CapabilityRegistrationState>,
    metrics: Option<Arc<dyn PluginMetricsSink>>,
}

impl ScopedSkillRegistration {
    pub(crate) fn new(
        manifest: &PluginManifest,
        registry: Option<SkillRegistry>,
        reconciler: Option<Arc<dyn SkillRegistryReconciler>>,
        state: Arc<CapabilityRegistrationState>,
        metrics: Option<Arc<dyn PluginMetricsSink>>,
    ) -> Self {
        Self {
            declared: manifest
                .capabilities
                .skills
                .iter()
                .map(|entry| entry.name.clone())
                .collect(),
            plugin_id: manifest.plugin_id(),
            trust_level: manifest.trust_level,
            registry,
            reconciler,
            state,
            metrics,
        }
    }
}

#[async_trait]
impl SkillRegistration for ScopedSkillRegistration {
    async fn register(&self, skill: Skill) -> Result<(), RegistrationError> {
        let name = skill.name.clone();
        if !self.declared.contains(&name) {
            record_capability_rejection(self.metrics.as_ref(), "skill", "undeclared_skill");
            return Err(RegistrationError::UndeclaredSkill { name });
        }
        if let Some(registry) = &self.registry {
            if let Some(reconciler) = &self.reconciler {
                registry
                    .try_register_from_plugin(
                        self.plugin_id.clone(),
                        self.trust_level,
                        skill.clone(),
                        |current, candidate| reconciler.reconcile(current, candidate),
                    )
                    .map_err(|error| RegistrationError::OwnerRegistry {
                        kind: "skill",
                        details: match error {
                            harness_skill::SkillRegistryUpdateError::Registry(error) => {
                                error.to_string()
                            }
                            harness_skill::SkillRegistryUpdateError::Reconcile(error) => error,
                        },
                    })?;
            } else {
                registry
                    .register_from_plugin(self.plugin_id.clone(), self.trust_level, skill.clone())
                    .map_err(|error| RegistrationError::OwnerRegistry {
                        kind: "skill",
                        details: error.to_string(),
                    })?;
            }
        }
        self.state.skills.lock().insert(name);
        Ok(())
    }

    fn pending_declared(&self) -> Vec<String> {
        pending(&self.declared, &self.state.skills)
    }
}

pub(crate) struct ScopedMemoryProviderRegistration {
    state: Arc<CapabilityRegistrationState>,
}

impl ScopedMemoryProviderRegistration {
    pub(crate) fn new(state: Arc<CapabilityRegistrationState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl MemoryProviderRegistration for ScopedMemoryProviderRegistration {
    async fn register(
        &self,
        provider: Arc<dyn harness_memory::MemoryProvider>,
    ) -> Result<(), RegistrationError> {
        let mut registered = self.state.memory_providers.lock();
        let provider_id = provider.provider_id().to_owned();
        if registered.contains_key(&provider_id) {
            return Err(RegistrationError::DuplicateSlot {
                slot: CapabilitySlot::MemoryProvider,
            });
        }
        registered.insert(provider_id, provider);
        Ok(())
    }
}

pub struct ScopedCoordinatorStrategyRegistration {
    state: Arc<CapabilityRegistrationState>,
}

impl ScopedCoordinatorStrategyRegistration {
    pub(crate) fn new(state: Arc<CapabilityRegistrationState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl CoordinatorStrategyRegistration for ScopedCoordinatorStrategyRegistration {
    async fn register(
        &self,
        _strategy: Arc<dyn CoordinatorStrategy>,
    ) -> Result<(), RegistrationError> {
        let mut registered = self.state.coordinator_registered.lock();
        if *registered {
            return Err(RegistrationError::DuplicateSlot {
                slot: CapabilitySlot::CoordinatorStrategy,
            });
        }
        *registered = true;
        Ok(())
    }
}

pub struct ScopedSteeringRegistration {
    plugin_id: PluginId,
    downstream: Option<Arc<dyn SteeringRegistration>>,
}

impl ScopedSteeringRegistration {
    pub(crate) fn new(
        plugin_id: PluginId,
        downstream: Option<Arc<dyn SteeringRegistration>>,
    ) -> Self {
        Self {
            plugin_id,
            downstream,
        }
    }
}

#[async_trait]
impl SteeringRegistration for ScopedSteeringRegistration {
    async fn push(&self, mut request: SteeringRequest) -> Result<SteeringId, RegistrationError> {
        request.source = harness_contracts::SteeringSource::Plugin {
            plugin_id: self.plugin_id.clone(),
        };
        request.priority = Some(harness_contracts::SteeringPriority::Normal);

        let Some(downstream) = &self.downstream else {
            return Err(RegistrationError::OwnerRegistry {
                kind: "steering",
                details: "steering registration is not wired".to_owned(),
            });
        };
        downstream.push(request).await
    }
}

fn pending(declared: &BTreeSet<String>, registered: &Mutex<BTreeSet<String>>) -> Vec<String> {
    let registered = registered.lock();
    declared.difference(&registered).cloned().collect()
}

fn record_capability_rejection(
    metrics: Option<&Arc<dyn PluginMetricsSink>>,
    kind: &str,
    reason: &str,
) {
    if let Some(metrics) = metrics {
        metrics.plugin_capability_registration_rejected(kind, reason);
    }
}

fn pending_map_keys(
    declared: &BTreeMap<String, bool>,
    registered: &Mutex<BTreeSet<String>>,
) -> Vec<String> {
    let registered = registered.lock();
    declared
        .keys()
        .filter(|name| !registered.contains(*name))
        .cloned()
        .collect()
}

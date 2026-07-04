use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use harness_contracts::{
    canonical_mcp_tool_name, parse_canonical_mcp_tool_name, validate_tool_name, McpServerId,
    McpServerSource, ProviderServiceAdapterAvailability, ShadowReason, ToolActionPlan,
    ToolCapability, ToolDescriptor, ToolError, ToolGroup, ToolOrigin, ToolServiceBinding,
    TrustLevel,
};
use parking_lot::RwLock;
use serde_json::Value;

use crate::{
    AuthorizedToolInput, SchemaResolverContext, Tool, ToolContext, ToolJournalAuthority,
    ToolRegistryBuilder, ToolStream, ValidationError,
};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RegistrationError {
    #[error("duplicate tool name: {0}")]
    Duplicate(String),
    #[error("trust violation: required {required:?}, got {provided:?}")]
    TrustViolation {
        required: TrustLevel,
        provided: TrustLevel,
    },
    #[error("capability not permitted for trust level {trust:?}: {cap}")]
    CapabilityNotPermitted {
        trust: TrustLevel,
        cap: ToolCapability,
    },
    #[error("invalid descriptor: {0}")]
    InvalidDescriptor(String),
    #[error("tool not found: {0}")]
    NotFound(String),
}

#[derive(Clone)]
pub struct ToolRegistry {
    inner: Arc<RwLock<ToolRegistryInner>>,
}

#[derive(Default)]
struct ToolRegistryInner {
    tools: BTreeMap<String, RegisteredTool>,
    shadowed: Vec<ShadowedRegistration>,
    generation: u64,
}

#[derive(Clone)]
struct RegisteredTool {
    tool: Arc<dyn Tool>,
    descriptor: Arc<ToolDescriptor>,
    origin: ToolOrigin,
    trust_level: TrustLevel,
    journal_authority: ToolJournalAuthority,
}

impl ToolRegistry {
    pub fn builder() -> ToolRegistryBuilder {
        ToolRegistryBuilder::new()
    }

    pub(crate) fn empty() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ToolRegistryInner::default())),
        }
    }

    pub fn register(&self, tool: Box<dyn Tool>) -> Result<(), RegistrationError> {
        self.register_with_journal_authority(tool, ToolJournalAuthority::None)
    }

    pub(crate) fn register_with_journal_authority(
        &self,
        tool: Box<dyn Tool>,
        journal_authority: ToolJournalAuthority,
    ) -> Result<(), RegistrationError> {
        self.register_with_journal_authority_inner(tool, journal_authority)
            .map(|_| ())
    }

    fn register_with_journal_authority_inner(
        &self,
        tool: Box<dyn Tool>,
        journal_authority: ToolJournalAuthority,
    ) -> Result<bool, RegistrationError> {
        let descriptor = tool.descriptor().clone();
        validate_descriptor(&descriptor)?;
        validate_capabilities(&descriptor)?;

        let name = descriptor.name.clone();
        let origin = descriptor.origin.clone();
        let trust_level = descriptor.trust_level;
        let registered = RegisteredTool {
            tool: tool.into(),
            descriptor: Arc::new(descriptor),
            origin,
            trust_level,
            journal_authority,
        };

        let mut inner = self.inner.write();
        if let Some(existing) = inner.tools.get(&name).cloned() {
            match resolve_shadow(&existing, &registered) {
                RegistrationDecision::KeepExisting(reason) => {
                    inner.shadowed.push(ShadowedRegistration {
                        name,
                        kept: existing.origin,
                        rejected: registered.origin,
                        reason,
                        at: Utc::now(),
                    });
                    inner.generation += 1;
                    return Ok(false);
                }
                RegistrationDecision::ReplaceExisting(reason) => {
                    inner.shadowed.push(ShadowedRegistration {
                        name: name.clone(),
                        kept: registered.origin.clone(),
                        rejected: existing.origin,
                        reason,
                        at: Utc::now(),
                    });
                    inner.tools.insert(name, registered);
                    inner.generation += 1;
                    return Ok(true);
                }
            }
        }

        inner.tools.insert(name, registered);
        inner.generation += 1;
        Ok(true)
    }

    pub fn register_from_plugin(
        &self,
        plugin_id: harness_contracts::PluginId,
        trust: TrustLevel,
        tool: Box<dyn Tool>,
    ) -> Result<bool, RegistrationError> {
        if trust == TrustLevel::UserControlled && tool.descriptor().properties.is_destructive {
            return Err(RegistrationError::TrustViolation {
                required: TrustLevel::AdminTrusted,
                provided: trust,
            });
        }
        self.register_with_journal_authority_inner(
            Box::new(PluginOriginTool::new(plugin_id, trust, tool)),
            ToolJournalAuthority::None,
        )
    }

    pub fn deregister(&self, name: &str) -> Result<(), RegistrationError> {
        let mut inner = self.inner.write();
        if inner.tools.remove(name).is_none() {
            return Err(RegistrationError::NotFound(name.to_owned()));
        }
        inner.generation += 1;
        Ok(())
    }

    pub fn deregister_from_plugin(
        &self,
        plugin_id: &harness_contracts::PluginId,
        name: &str,
    ) -> Result<bool, RegistrationError> {
        let mut inner = self.inner.write();
        let Some(existing) = inner.tools.get(name) else {
            return Ok(false);
        };
        if !matches!(&existing.origin, ToolOrigin::Plugin { plugin_id: owner, .. } if owner == plugin_id)
        {
            return Ok(false);
        }
        inner.tools.remove(name);
        inner.generation += 1;
        Ok(true)
    }

    pub fn deregister_mcp_tool(
        &self,
        server_id: &McpServerId,
        server_source: &McpServerSource,
        name: &str,
    ) -> Result<bool, RegistrationError> {
        let mut inner = self.inner.write();
        let Some(existing) = inner.tools.get(name) else {
            return Ok(false);
        };
        if !matches!(
            &existing.origin,
            ToolOrigin::Mcp(origin)
                if &origin.server_id == server_id && &origin.server_source == server_source
        ) {
            return Ok(false);
        }
        inner.tools.remove(name);
        inner.generation += 1;
        Ok(true)
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.inner
            .read()
            .tools
            .get(name)
            .map(|tool| Arc::clone(&tool.tool))
    }

    pub fn snapshot(&self) -> ToolRegistrySnapshot {
        let inner = self.inner.read();
        ToolRegistrySnapshot {
            tools: Arc::new(
                inner
                    .tools
                    .iter()
                    .map(|(name, tool)| (name.clone(), Arc::clone(&tool.tool)))
                    .collect(),
            ),
            descriptors: Arc::new(
                inner
                    .tools
                    .iter()
                    .map(|(name, tool)| (name.clone(), Arc::clone(&tool.descriptor)))
                    .collect(),
            ),
            journal_authorities: Arc::new(
                inner
                    .tools
                    .iter()
                    .map(|(name, tool)| (name.clone(), tool.journal_authority))
                    .collect(),
            ),
            generation: inner.generation,
        }
    }

    pub fn shadowed(&self) -> Vec<ShadowedRegistration> {
        self.inner.read().shadowed.clone()
    }

    pub fn provider_service_adapter_availability(&self) -> ProviderServiceAdapterAvailability {
        provider_service_adapter_availability_from_snapshot(&self.snapshot())
    }
}

struct PluginOriginTool {
    inner: Arc<dyn Tool>,
    descriptor: ToolDescriptor,
}

impl PluginOriginTool {
    fn new(plugin_id: harness_contracts::PluginId, trust: TrustLevel, tool: Box<dyn Tool>) -> Self {
        let mut descriptor = tool.descriptor().clone();
        descriptor.origin = ToolOrigin::Plugin { plugin_id, trust };
        descriptor.trust_level = trust;
        Self {
            inner: tool.into(),
            descriptor,
        }
    }
}

#[async_trait::async_trait]
impl Tool for PluginOriginTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn resolve_schema(
        &self,
        ctx: &SchemaResolverContext,
    ) -> Result<Value, harness_contracts::ToolError> {
        self.inner.resolve_schema(ctx).await
    }

    async fn validate(&self, input: &Value, ctx: &ToolContext) -> Result<(), ValidationError> {
        self.inner.validate(input, ctx).await
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        self.inner.plan(input, ctx).await
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        self.inner.execute_authorized(authorized, ctx).await
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShadowedRegistration {
    pub name: String,
    pub kept: ToolOrigin,
    pub rejected: ToolOrigin,
    pub reason: ShadowReason,
    pub at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct ToolRegistrySnapshot {
    tools: Arc<BTreeMap<String, Arc<dyn Tool>>>,
    descriptors: Arc<BTreeMap<String, Arc<ToolDescriptor>>>,
    journal_authorities: Arc<BTreeMap<String, ToolJournalAuthority>>,
    generation: u64,
}

impl ToolRegistrySnapshot {
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    pub fn descriptor(&self, name: &str) -> Option<&Arc<ToolDescriptor>> {
        self.descriptors.get(name)
    }

    pub fn journal_authority(&self, name: &str) -> ToolJournalAuthority {
        self.journal_authorities
            .get(name)
            .copied()
            .unwrap_or_default()
    }

    pub fn iter_sorted(&self) -> impl Iterator<Item = (&String, &Arc<dyn Tool>)> {
        self.tools.iter()
    }

    pub fn as_descriptors(&self) -> Vec<&ToolDescriptor> {
        self.descriptors
            .values()
            .map(std::convert::AsRef::as_ref)
            .collect()
    }

    pub fn by_group(&self, group: &ToolGroup) -> Vec<&Arc<dyn Tool>> {
        self.tools
            .iter()
            .filter_map(|(name, tool)| {
                (self.descriptors.get(name)?.group == *group).then_some(tool)
            })
            .collect()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

pub fn tool_service_bindings_from_snapshot(
    snapshot: &ToolRegistrySnapshot,
) -> Vec<ToolServiceBinding> {
    let mut bindings = Vec::new();

    for descriptor in snapshot.as_descriptors() {
        let Some(binding) = descriptor.service_binding.as_ref() else {
            continue;
        };
        if bindings.iter().any(|existing: &ToolServiceBinding| {
            existing.provider_id == binding.provider_id
                && existing.operation_id == binding.operation_id
                && existing.route_kind == binding.route_kind
        }) {
            continue;
        }
        bindings.push(binding.clone());
    }

    bindings
}

pub fn provider_service_adapter_availability_from_snapshot(
    snapshot: &ToolRegistrySnapshot,
) -> ProviderServiceAdapterAvailability {
    ProviderServiceAdapterAvailability {
        bindings: tool_service_bindings_from_snapshot(snapshot),
    }
}

enum RegistrationDecision {
    KeepExisting(ShadowReason),
    ReplaceExisting(ShadowReason),
}

fn resolve_shadow(existing: &RegisteredTool, incoming: &RegisteredTool) -> RegistrationDecision {
    match (&existing.origin, &incoming.origin) {
        (ToolOrigin::Builtin, ToolOrigin::Builtin) => {
            RegistrationDecision::KeepExisting(ShadowReason::Duplicate)
        }
        (ToolOrigin::Builtin, _) => RegistrationDecision::KeepExisting(ShadowReason::BuiltinWins),
        (_, ToolOrigin::Builtin) => {
            RegistrationDecision::ReplaceExisting(ShadowReason::BuiltinWins)
        }
        _ if trust_rank(incoming.trust_level) > trust_rank(existing.trust_level) => {
            RegistrationDecision::ReplaceExisting(ShadowReason::HigherTrust)
        }
        _ if trust_rank(incoming.trust_level) < trust_rank(existing.trust_level) => {
            RegistrationDecision::KeepExisting(ShadowReason::HigherTrust)
        }
        _ => RegistrationDecision::KeepExisting(ShadowReason::Duplicate),
    }
}

fn trust_rank(trust: TrustLevel) -> u8 {
    match trust {
        TrustLevel::AdminTrusted => 1,
        _ => 0,
    }
}

fn validate_descriptor(descriptor: &ToolDescriptor) -> Result<(), RegistrationError> {
    if let Some((server, tool)) = parse_canonical_mcp_tool_name(&descriptor.name) {
        let canonical = canonical_mcp_tool_name(server, tool)
            .map_err(|error| RegistrationError::InvalidDescriptor(error.to_string()))?;
        if canonical == descriptor.name {
            return Ok(());
        }
    }
    validate_tool_name(&descriptor.name)
        .map_err(|error| RegistrationError::InvalidDescriptor(error.to_string()))?;
    Ok(())
}

fn validate_capabilities(descriptor: &ToolDescriptor) -> Result<(), RegistrationError> {
    let policy = CapabilityPolicy::default();
    for cap in &descriptor.required_capabilities {
        if !policy.allows(descriptor.trust_level, cap) {
            return Err(RegistrationError::CapabilityNotPermitted {
                trust: descriptor.trust_level,
                cap: cap.clone(),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CapabilityPolicy {
    any_trust: Vec<ToolCapability>,
}

impl Default for CapabilityPolicy {
    fn default() -> Self {
        Self {
            any_trust: vec![ToolCapability::BlobReader, ToolCapability::TodoStore],
        }
    }
}

impl CapabilityPolicy {
    #[must_use]
    pub fn allows(&self, trust: TrustLevel, cap: &ToolCapability) -> bool {
        self.any_trust.contains(cap) || trust == TrustLevel::AdminTrusted
    }

    #[must_use]
    pub fn describe(&self) -> Vec<CapabilityPolicyEntry> {
        let mut entries = self
            .any_trust
            .iter()
            .cloned()
            .map(|capability| CapabilityPolicyEntry {
                capability,
                rule: CapabilityPolicyRule::AnyTrust,
            })
            .collect::<Vec<_>>();
        entries.push(CapabilityPolicyEntry {
            capability: ToolCapability::Custom("*".to_owned()),
            rule: CapabilityPolicyRule::AdminTrustedOnly,
        });
        entries
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CapabilityPolicyEntry {
    pub capability: ToolCapability,
    pub rule: CapabilityPolicyRule,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CapabilityPolicyRule {
    AnyTrust,
    AdminTrustedOnly,
    Deny,
}

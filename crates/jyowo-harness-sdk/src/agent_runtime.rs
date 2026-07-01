use std::path::Path;

use harness_agent_runtime::{
    default_agent_capability_environment, AgentCapabilityEnvironment, AgentCapabilityResolver,
    AgentProfileRegistry, AgentProfileRegistryError, AgentRuntimePolicyResolver, AgentRuntimeStore,
    ResolvedAgentCapabilityPolicy,
};
use harness_contracts::{AgentProfile, AgentRunOptions};

pub use harness_agent_runtime::{
    AgentCapabilitiesInput, AgentRuntimePolicyError, ExecutionSettingsAgentInput,
    ResolvedAgentRuntimePolicy,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgentCapabilityResolutionContext {
    pub stream_permission_runtime_available: bool,
}

#[derive(Debug)]
pub enum AgentRuntimeFacadeError {
    Store(harness_agent_runtime::AgentRuntimeStoreError),
    Profiles(AgentProfileRegistryError),
}

impl std::fmt::Display for AgentRuntimeFacadeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(f, "{error}"),
            Self::Profiles(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for AgentRuntimeFacadeError {}

impl From<harness_agent_runtime::AgentRuntimeStoreError> for AgentRuntimeFacadeError {
    fn from(error: harness_agent_runtime::AgentRuntimeStoreError) -> Self {
        Self::Store(error)
    }
}

impl From<AgentProfileRegistryError> for AgentRuntimeFacadeError {
    fn from(error: AgentProfileRegistryError) -> Self {
        Self::Profiles(error)
    }
}

pub fn list_agent_profiles(
    workspace_root: impl AsRef<Path>,
) -> Result<Vec<AgentProfile>, AgentRuntimeFacadeError> {
    let store = AgentRuntimeStore::open(workspace_root)?;
    Ok(AgentProfileRegistry::new(&store).list()?)
}

pub fn save_agent_profile(
    workspace_root: impl AsRef<Path>,
    profile: AgentProfile,
) -> Result<AgentProfile, AgentRuntimeFacadeError> {
    let store = AgentRuntimeStore::open(workspace_root)?;
    Ok(AgentProfileRegistry::new(&store).save(profile)?)
}

pub fn delete_agent_profile(
    workspace_root: impl AsRef<Path>,
    profile_id: &str,
) -> Result<(), AgentRuntimeFacadeError> {
    let store = AgentRuntimeStore::open(workspace_root)?;
    AgentProfileRegistry::new(&store).delete(profile_id)?;
    Ok(())
}

#[must_use]
pub fn resolve_agent_capabilities(
    workspace_root: impl AsRef<Path>,
    environment: AgentCapabilityEnvironment,
) -> ResolvedAgentCapabilityPolicy {
    AgentCapabilityResolver::resolve(workspace_root.as_ref(), environment)
}

#[must_use]
pub fn resolve_agent_capabilities_with_context(
    workspace_root: impl AsRef<Path>,
    context: AgentCapabilityResolutionContext,
) -> ResolvedAgentCapabilityPolicy {
    let mut environment = default_agent_capability_environment();
    environment.stream_permission_runtime_available = context.stream_permission_runtime_available;
    resolve_agent_capabilities(workspace_root, environment)
}

pub fn resolve_agent_runtime_policy(
    workspace_root: impl AsRef<Path>,
    settings: &ExecutionSettingsAgentInput,
    agent_options: Option<&AgentRunOptions>,
    capabilities: &AgentCapabilitiesInput,
    known_profile_ids: &[String],
    conversation_id: &str,
) -> Result<ResolvedAgentRuntimePolicy, AgentRuntimePolicyError> {
    AgentRuntimePolicyResolver::merge(
        settings,
        agent_options,
        capabilities,
        known_profile_ids,
        conversation_id,
        workspace_root.as_ref(),
    )
}

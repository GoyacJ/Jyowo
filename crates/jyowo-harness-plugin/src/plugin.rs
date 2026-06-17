use std::path::PathBuf;

use async_trait::async_trait;
use harness_contracts::{PluginId, TrustLevel};
use serde_json::Value;

use crate::{
    CoordinatorStrategyRegistration, HookRegistration, McpRegistration, MemoryProviderRegistration,
    PluginError, PluginManifest, SkillRegistration, SteeringRegistration, ToolRegistration,
};

#[async_trait]
pub trait Plugin: Send + Sync + 'static {
    fn manifest(&self) -> &PluginManifest;

    async fn activate(
        &self,
        ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError>;

    async fn deactivate(&self) -> Result<(), PluginError>;
}

#[derive(Clone)]
pub struct PluginActivationContext {
    pub trust_level: TrustLevel,
    pub plugin_id: PluginId,
    pub config: Value,
    pub workspace_root: Option<PathBuf>,
    pub tools: Option<std::sync::Arc<dyn ToolRegistration>>,
    pub hooks: Option<std::sync::Arc<dyn HookRegistration>>,
    pub mcp: Option<std::sync::Arc<dyn McpRegistration>>,
    pub skills: Option<std::sync::Arc<dyn SkillRegistration>>,
    pub memory: Option<std::sync::Arc<dyn MemoryProviderRegistration>>,
    pub coordinator: Option<std::sync::Arc<dyn CoordinatorStrategyRegistration>>,
    pub steering: Option<std::sync::Arc<dyn SteeringRegistration>>,
}

impl PluginActivationContext {
    pub fn manifest_only(manifest: &PluginManifest) -> Self {
        Self {
            trust_level: manifest.trust_level,
            plugin_id: manifest.plugin_id(),
            config: Value::Null,
            workspace_root: None,
            tools: None,
            hooks: None,
            mcp: None,
            skills: None,
            memory: None,
            coordinator: None,
            steering: None,
        }
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginActivationResult {
    pub registered_tools: Vec<String>,
    pub registered_hooks: Vec<String>,
    pub registered_skills: Vec<String>,
    pub registered_mcp: Vec<harness_contracts::McpServerId>,
    pub occupied_slots: Vec<CapabilitySlot>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySlot {
    MemoryProvider,
    CustomToolset(String),
    CoordinatorStrategy,
}

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    CacheImpact, Event, ToolDescriptor, ToolError, ToolLoadingBackendName, ToolName,
    ToolSearchQueryKind, ToolUseId,
};
use harness_model::ModelCapabilities;
use harness_tool::ToolContext;

use crate::ReloadHandle;

pub const TOOL_SEARCH_RUNTIME_CAPABILITY: &str = "tool_search_runtime";

#[async_trait]
pub trait ToolSearchRuntimeCap: Send + Sync + 'static {
    async fn snapshot(&self) -> Result<ToolSearchRuntimeSnapshot, ToolError>;

    async fn emit_event(&self, _event: Event) -> Result<(), ToolError> {
        Ok(())
    }

    async fn dispatch_pre_tool_search_hook(
        &self,
        _ctx: &ToolContext,
        _tool_use_id: ToolUseId,
        _query: &str,
        _query_kind: ToolSearchQueryKind,
    ) -> Result<ToolSearchPreHookOutcome, ToolError> {
        Ok(ToolSearchPreHookOutcome::Continue)
    }

    async fn dispatch_post_tool_search_hook(
        &self,
        _ctx: &ToolContext,
        _tool_use_id: ToolUseId,
        _materialized: Vec<ToolName>,
        _backend: ToolLoadingBackendName,
        _cache_impact: CacheImpact,
    ) -> Result<(), ToolError> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolSearchPreHookOutcome {
    Continue,
    Block { reason: String },
    RewriteInput(serde_json::Value),
}

impl ToolSearchPreHookOutcome {
    pub fn continue_if_unsupported(
        result: Result<Self, ToolError>,
        _ctx: &ToolContext,
    ) -> Result<Self, ToolError> {
        result
    }
}

#[derive(Clone)]
pub struct ToolSearchRuntimeSnapshot {
    pub deferred_tools: Vec<ToolDescriptor>,
    pub loaded_tool_names: BTreeSet<ToolName>,
    pub discovered_tool_names: BTreeSet<ToolName>,
    pub pending_mcp_servers: Vec<String>,
    pub model_caps: Arc<ModelCapabilities>,
    pub reload_handle: Option<Arc<dyn ReloadHandle>>,
}

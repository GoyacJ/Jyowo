use std::sync::Arc;

use async_trait::async_trait;
pub use harness_contracts::ToolLoadingBackendName;
use harness_contracts::{CacheImpact, HarnessError, RunId, SessionId, ToolName};
use harness_model::ModelCapabilities;

use crate::ToolLoadingError;

#[async_trait]
pub trait ToolLoadingBackend: Send + Sync + 'static {
    fn backend_name(&self) -> ToolLoadingBackendName;

    async fn materialize(
        &self,
        ctx: &ToolLoadingContext,
        requested: &[ToolName],
    ) -> Result<MaterializeOutcome, ToolLoadingError>;
}

#[async_trait]
pub trait ToolLoadingBackendSelector: Send + Sync + 'static {
    async fn select(&self, ctx: &ToolLoadingContext) -> Arc<dyn ToolLoadingBackend>;
}

#[derive(Clone)]
pub struct ToolLoadingContext {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub model_caps: Arc<ModelCapabilities>,
    pub reload_handle: Option<Arc<dyn ReloadHandle>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MaterializeOutcome {
    ToolReferenceEmitted {
        refs: Vec<ToolReference>,
    },
    InlineReinjected {
        tools: Vec<ToolName>,
        cache_impact: CacheImpact,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolReference {
    pub tool_name: String,
}

#[async_trait]
pub trait ReloadHandle: Send + Sync + 'static {
    async fn reload_with_add_tools(
        &self,
        tools: Vec<ToolName>,
    ) -> Result<CacheImpact, HarnessError>;
}

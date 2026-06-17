use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::ToolName;

use crate::{
    MaterializationCoalescer, MaterializeOutcome, ToolLoadingBackend, ToolLoadingBackendName,
    ToolLoadingContext, ToolLoadingError,
};

pub struct InlineReinjectionBackend {
    coalescer: Arc<MaterializationCoalescer>,
}

impl InlineReinjectionBackend {
    pub const NAME: &'static str = "inline_reinjection";

    #[must_use]
    pub fn new(coalescer: Arc<MaterializationCoalescer>) -> Self {
        Self { coalescer }
    }
}

#[async_trait]
impl ToolLoadingBackend for InlineReinjectionBackend {
    fn backend_name(&self) -> ToolLoadingBackendName {
        Self::NAME.to_owned()
    }

    async fn materialize(
        &self,
        ctx: &ToolLoadingContext,
        requested: &[ToolName],
    ) -> Result<MaterializeOutcome, ToolLoadingError> {
        let handle = ctx
            .reload_handle
            .as_ref()
            .ok_or(ToolLoadingError::ReloadHandleMissing)?
            .clone();
        let cache_impact = self
            .coalescer
            .submit(ctx.session_id, ctx.run_id, requested.to_vec(), handle)
            .await
            .map_err(|error| ToolLoadingError::ReloadRejected(error.to_string()))?;
        Ok(MaterializeOutcome::InlineReinjected {
            tools: requested.to_vec(),
            cache_impact,
        })
    }
}

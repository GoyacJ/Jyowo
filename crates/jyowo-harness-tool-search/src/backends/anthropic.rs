use async_trait::async_trait;
use harness_contracts::ToolName;

use crate::{
    MaterializeOutcome, ToolLoadingBackend, ToolLoadingBackendName, ToolLoadingContext,
    ToolLoadingError, ToolReference,
};

#[derive(Debug, Default)]
pub struct AnthropicToolReferenceBackend;

impl AnthropicToolReferenceBackend {
    pub const NAME: &'static str = "anthropic_tool_reference";
}

#[async_trait]
impl ToolLoadingBackend for AnthropicToolReferenceBackend {
    fn backend_name(&self) -> ToolLoadingBackendName {
        Self::NAME.to_owned()
    }

    async fn materialize(
        &self,
        _ctx: &ToolLoadingContext,
        requested: &[ToolName],
    ) -> Result<MaterializeOutcome, ToolLoadingError> {
        Ok(MaterializeOutcome::ToolReferenceEmitted {
            refs: requested
                .iter()
                .map(|tool_name| ToolReference {
                    tool_name: tool_name.clone(),
                })
                .collect(),
        })
    }
}

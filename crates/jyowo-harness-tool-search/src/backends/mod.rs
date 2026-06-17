mod anthropic;
mod inline;

use std::sync::Arc;

use async_trait::async_trait;

pub use self::anthropic::AnthropicToolReferenceBackend;
pub use self::inline::InlineReinjectionBackend;

use crate::{ToolLoadingBackend, ToolLoadingBackendSelector, ToolLoadingContext};

pub struct DefaultBackendSelector {
    anthropic: Arc<AnthropicToolReferenceBackend>,
    inline: Arc<InlineReinjectionBackend>,
}

impl DefaultBackendSelector {
    #[must_use]
    pub fn new(
        anthropic: Arc<AnthropicToolReferenceBackend>,
        inline: Arc<InlineReinjectionBackend>,
    ) -> Self {
        Self { anthropic, inline }
    }
}

#[async_trait]
impl ToolLoadingBackendSelector for DefaultBackendSelector {
    async fn select(&self, ctx: &ToolLoadingContext) -> Arc<dyn ToolLoadingBackend> {
        if ctx.model_caps.supports_tool_reference {
            self.anthropic.clone()
        } else {
            self.inline.clone()
        }
    }
}

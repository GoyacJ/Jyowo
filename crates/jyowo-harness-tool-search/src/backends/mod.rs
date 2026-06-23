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
        if ctx.reload_handle.is_some() {
            self.inline.clone()
        } else if ctx.model_caps.tool_calling {
            self.anthropic.clone()
        } else {
            self.inline.clone()
        }
    }
}

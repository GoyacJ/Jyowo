use harness_contracts::Message;

use crate::{AnthropicCounter, ImageMeta, TokenCounter};

#[derive(Debug, Clone, Copy, Default)]
pub struct AnthropicTokenCounter;

impl TokenCounter for AnthropicTokenCounter {
    fn count_tokens(&self, text: &str, model: &str) -> usize {
        AnthropicCounter.count_tokens(text, model)
    }

    fn count_messages(&self, messages: &[Message], model: &str) -> usize {
        AnthropicCounter.count_messages(messages, model)
    }

    fn count_image(&self, image: &ImageMeta, model: &str) -> Option<usize> {
        AnthropicCounter.count_image(image, model)
    }
}

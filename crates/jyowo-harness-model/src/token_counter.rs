use harness_contracts::{Message, MessagePart, ToolResult, ToolResultPart};

pub trait TokenCounter: Send + Sync + 'static {
    fn count_tokens(&self, text: &str, model: &str) -> usize;
    fn count_messages(&self, messages: &[Message], model: &str) -> usize;

    fn count_image(&self, _image: &ImageMeta, _model: &str) -> Option<usize> {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    pub mime: String,
    pub detail: ImageDetail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageDetail {
    Low,
    High,
    Auto,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ApproximateCounter;

impl TokenCounter for ApproximateCounter {
    fn count_tokens(&self, text: &str, _model: &str) -> usize {
        approximate_text_tokens(text)
    }

    fn count_messages(&self, messages: &[Message], model: &str) -> usize {
        count_messages_with(messages, model, self, 4, 2)
    }

    fn count_image(&self, image: &ImageMeta, model: &str) -> Option<usize> {
        if is_gemini_model(model) {
            return Some(gemini_image_tokens(image));
        }
        None
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TiktokenCounter;

impl TokenCounter for TiktokenCounter {
    fn count_tokens(&self, text: &str, _model: &str) -> usize {
        approximate_text_tokens(text)
    }

    fn count_messages(&self, messages: &[Message], model: &str) -> usize {
        count_messages_with(messages, model, self, 3, 3)
    }

    fn count_image(&self, image: &ImageMeta, model: &str) -> Option<usize> {
        if is_openai_vision_model(model) {
            return Some(openai_image_tokens(image));
        }
        None
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AnthropicCounter;

impl TokenCounter for AnthropicCounter {
    fn count_tokens(&self, text: &str, _model: &str) -> usize {
        approximate_text_tokens(text)
    }

    fn count_messages(&self, messages: &[Message], model: &str) -> usize {
        count_messages_with(messages, model, self, 4, 2)
    }

    fn count_image(&self, image: &ImageMeta, model: &str) -> Option<usize> {
        if is_anthropic_model(model) {
            return Some(anthropic_image_tokens(image));
        }
        None
    }
}

pub(crate) fn approximate_text_tokens(text: &str) -> usize {
    let chars = text.chars().count();
    if chars == 0 {
        0
    } else {
        chars.div_ceil(4)
    }
}

pub(crate) fn count_messages_with(
    messages: &[Message],
    model: &str,
    counter: &dyn TokenCounter,
    per_message_overhead: usize,
    reply_overhead: usize,
) -> usize {
    messages
        .iter()
        .map(|message| {
            per_message_overhead
                + message
                    .parts
                    .iter()
                    .map(|part| count_part(part, model, counter))
                    .sum::<usize>()
        })
        .sum::<usize>()
        + reply_overhead
}

fn count_part(part: &MessagePart, model: &str, counter: &dyn TokenCounter) -> usize {
    match part {
        MessagePart::Text(text) => counter.count_tokens(text, model),
        MessagePart::Image { mime_type, .. } => counter
            .count_image(
                &ImageMeta {
                    width: 0,
                    height: 0,
                    mime: mime_type.clone(),
                    detail: ImageDetail::Auto,
                },
                model,
            )
            .unwrap_or(0),
        MessagePart::Video { .. } | MessagePart::File { .. } => 0,
        MessagePart::ToolUse { name, input, .. } => {
            counter.count_tokens(name, model) + counter.count_tokens(&input.to_string(), model)
        }
        MessagePart::ToolResult { content, .. } => count_tool_result(content, model, counter),
        MessagePart::Thinking(thinking) => thinking
            .text
            .as_deref()
            .map_or(0, |text| counter.count_tokens(text, model)),
        _ => 0,
    }
}

fn count_tool_result(result: &ToolResult, model: &str, counter: &dyn TokenCounter) -> usize {
    match result {
        ToolResult::Text(text) => counter.count_tokens(text, model),
        ToolResult::Structured(value) => counter.count_tokens(&value.to_string(), model),
        ToolResult::Blob { content_type, .. } => counter.count_tokens(content_type, model),
        ToolResult::Mixed(parts) => parts
            .iter()
            .map(|part| match part {
                ToolResultPart::Text { text } | ToolResultPart::Code { text, .. } => {
                    counter.count_tokens(text, model)
                }
                ToolResultPart::Structured { value, .. } => {
                    counter.count_tokens(&value.to_string(), model)
                }
                ToolResultPart::Blob {
                    content_type,
                    summary,
                    ..
                } => {
                    counter.count_tokens(content_type, model)
                        + summary
                            .as_deref()
                            .map_or(0, |summary| counter.count_tokens(summary, model))
                }
                ToolResultPart::Reference { title, summary, .. } => {
                    title
                        .as_deref()
                        .map_or(0, |title| counter.count_tokens(title, model))
                        + summary
                            .as_deref()
                            .map_or(0, |summary| counter.count_tokens(summary, model))
                }
                ToolResultPart::Table {
                    headers,
                    rows,
                    caption,
                } => {
                    headers
                        .iter()
                        .map(|header| counter.count_tokens(header, model))
                        .sum::<usize>()
                        + rows
                            .iter()
                            .flatten()
                            .map(|value| counter.count_tokens(&value.to_string(), model))
                            .sum::<usize>()
                        + caption
                            .as_deref()
                            .map_or(0, |caption| counter.count_tokens(caption, model))
                }
                ToolResultPart::Progress { stage, detail, .. } => {
                    counter.count_tokens(stage, model)
                        + detail
                            .as_deref()
                            .map_or(0, |detail| counter.count_tokens(detail, model))
                }
                ToolResultPart::Error { code, message, .. } => {
                    counter.count_tokens(code, model) + counter.count_tokens(message, model)
                }
                _ => 0,
            })
            .sum(),
        _ => 0,
    }
}

fn anthropic_image_tokens(image: &ImageMeta) -> usize {
    image_area(image).div_ceil(750).max(1)
}

fn openai_image_tokens(image: &ImageMeta) -> usize {
    if matches!(image.detail, ImageDetail::Low) {
        return 85;
    }
    let tiles =
        image.width.max(1).div_ceil(512) as usize * image.height.max(1).div_ceil(512) as usize;
    85 + 170 * tiles
}

fn gemini_image_tokens(image: &ImageMeta) -> usize {
    let tiles =
        image.width.max(1).div_ceil(768) as usize * image.height.max(1).div_ceil(768) as usize;
    258 * tiles.max(1)
}

fn image_area(image: &ImageMeta) -> usize {
    image.width.max(1) as usize * image.height.max(1) as usize
}

fn is_openai_vision_model(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    model.contains("gpt-4o")
        || model.contains("gpt-4.1")
        || model.contains("o3")
        || model.contains("o4")
}

fn is_anthropic_model(model: &str) -> bool {
    model.to_ascii_lowercase().contains("claude")
}

fn is_gemini_model(model: &str) -> bool {
    model.to_ascii_lowercase().contains("gemini")
}

use futures::stream;
use harness_contracts::{ModelError, StopReason, UsageSnapshot};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    ContentDelta, ContentType, InferContext, ModelRequest, ModelStream, ModelStreamEvent,
    ThinkingDelta,
};

use super::continuation;
use super::dialect::OpenAiChatDialect;
use super::request::{
    chat_message_for_dialect, merge_extra_object, openai_tool, DEFAULT_MAX_TOKENS,
};

pub(super) async fn chat_request_body(
    req: &ModelRequest,
    max_tokens_field: &str,
    dialect: OpenAiChatDialect,
    ctx: &InferContext,
) -> Result<Value, ModelError> {
    let mut messages = Vec::new();
    if let Some(system) = &req.system {
        messages.push(json!({
            "role": "system",
            "content": system,
        }));
    }
    for message in &req.messages {
        let mut encoded = chat_message_for_dialect(message, ctx, dialect).await?;
        continuation::apply_chat_message_continuation(
            &mut encoded,
            message,
            dialect,
            &req.provider_context.continuations,
        )?;
        messages.push(encoded);
    }

    let mut body = json!({
        "model": req.model_id,
        "messages": messages,
        "stream": req.stream,
    });
    body[max_tokens_field] = json!(req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS));

    if dialect == OpenAiChatDialect::MiniMax && req.extra.get("reasoning_split").is_none() {
        body["reasoning_split"] = json!(true);
    }

    if req.stream {
        body["stream_options"] = json!({ "include_usage": true });
    }
    if let Some(temperature) = req.temperature {
        body["temperature"] = json!(temperature);
    }
    merge_extra_object(&mut body, &req.extra)?;
    if let Some(tools) = &req.tools {
        let local_tools = tools.iter().map(openai_tool);
        match body.get_mut("tools") {
            Some(Value::Array(existing)) => existing.extend(local_tools),
            Some(_) => {
                return Err(ModelError::InvalidRequest(
                    "Chat Completions tools extra must be an array".to_owned(),
                ));
            }
            None => body["tools"] = Value::Array(local_tools.collect()),
        }
    }
    if matches!(dialect, OpenAiChatDialect::DeepSeek) && deepseek_thinking_enabled(&body) {
        remove_deepseek_sampling_parameters(&mut body);
    }

    Ok(body)
}

pub(super) fn chat_response_to_stream(
    response: Value,
    dialect: OpenAiChatDialect,
) -> Result<ModelStream, ModelError> {
    let continuation_event = continuation::chat_response_continuation_event(dialect, &response);
    let response: ChatCompletionResponse = serde_json::from_value(response).map_err(|error| {
        ModelError::UnexpectedResponse(format!("invalid OpenAI protocol response: {error}"))
    })?;
    let usage = usage_for_dialect(response.usage.as_ref(), dialect);
    let choice = response.choices.into_iter().next().ok_or_else(|| {
        ModelError::UnexpectedResponse("OpenAI protocol response had no choices".to_owned())
    })?;
    let mut events = vec![ModelStreamEvent::MessageStart {
        message_id: response.id,
        usage: usage.clone(),
    }];
    if let Some(event) = continuation_event {
        events.push(event);
    }
    let mut index = 0;

    if dialect == OpenAiChatDialect::Qwen {
        if let Some(reasoning_content) = choice.message.reasoning_content {
            if !reasoning_content.is_empty() {
                events.push(ModelStreamEvent::ContentBlockStart {
                    index,
                    content_type: ContentType::Thinking,
                });
                events.push(ModelStreamEvent::ContentBlockDelta {
                    index,
                    delta: ContentDelta::Thinking(ThinkingDelta {
                        text: Some(reasoning_content),
                        provider_native: None,
                        signature: None,
                    }),
                });
                events.push(ModelStreamEvent::ContentBlockStop { index });
                index += 1;
            }
        }
    }

    if let Some(content) = choice.message.content {
        if !content.is_empty() {
            events.push(ModelStreamEvent::ContentBlockStart {
                index,
                content_type: ContentType::Text,
            });
            events.push(ModelStreamEvent::ContentBlockDelta {
                index,
                delta: ContentDelta::Text(content),
            });
            events.push(ModelStreamEvent::ContentBlockStop { index });
            index += 1;
        }
    }

    for tool_call in choice.message.tool_calls {
        events.push(ModelStreamEvent::ContentBlockStart {
            index,
            content_type: ContentType::ToolUse,
        });
        events.push(ModelStreamEvent::ContentBlockDelta {
            index,
            delta: ContentDelta::ToolUseStart {
                id: tool_call.id,
                name: tool_call.function.name,
            },
        });
        if !tool_call.function.arguments.is_empty() {
            events.push(ModelStreamEvent::ContentBlockDelta {
                index,
                delta: ContentDelta::ToolUseInputJson(tool_call.function.arguments),
            });
        }
        events.push(ModelStreamEvent::ContentBlockStop { index });
        index += 1;
    }

    events.push(ModelStreamEvent::MessageDelta {
        stop_reason: choice.finish_reason.as_deref().map(stop_reason),
        usage_delta: usage,
    });
    events.push(ModelStreamEvent::MessageStop);
    Ok(Box::pin(stream::iter(events)))
}

pub(crate) fn usage_for_dialect(
    value: Option<&OpenAiUsage>,
    dialect: OpenAiChatDialect,
) -> UsageSnapshot {
    match dialect {
        OpenAiChatDialect::DeepSeek => deepseek_usage(value),
        _ => openai_compatible_usage(value),
    }
}

fn deepseek_usage(value: Option<&OpenAiUsage>) -> UsageSnapshot {
    if let Some(usage) = value {
        if usage.prompt_cache_miss_tokens.is_some() || usage.prompt_cache_hit_tokens.is_some() {
            return UsageSnapshot {
                input_tokens: usage.prompt_cache_miss_tokens.unwrap_or_default(),
                output_tokens: usage.completion_tokens.unwrap_or_default(),
                cache_read_tokens: usage.prompt_cache_hit_tokens.unwrap_or_default(),
                cache_write_tokens: 0,
                cost_micros: 0,
                tool_calls: 0,
            };
        }
    }
    openai_compatible_usage(value)
}

fn openai_compatible_usage(value: Option<&OpenAiUsage>) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens: value
            .and_then(|usage| usage.prompt_tokens)
            .unwrap_or_default(),
        output_tokens: value
            .and_then(|usage| usage.completion_tokens)
            .unwrap_or_default(),
        cache_read_tokens: value
            .and_then(|usage| usage.prompt_tokens_details.as_ref())
            .and_then(|details| details.cached_tokens)
            .unwrap_or_default(),
        cache_write_tokens: 0,
        cost_micros: 0,
        tool_calls: 0,
    }
}

fn deepseek_thinking_enabled(body: &Value) -> bool {
    body.pointer("/thinking/disabled").and_then(Value::as_bool) != Some(true)
}

fn remove_deepseek_sampling_parameters(body: &mut Value) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    for key in [
        "temperature",
        "top_p",
        "presence_penalty",
        "frequency_penalty",
    ] {
        object.remove(key);
    }
}

pub(crate) fn stop_reason(reason: &str) -> StopReason {
    match reason {
        "stop" => StopReason::EndTurn,
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "length" => StopReason::MaxIterations,
        "content_filter" => StopReason::ContentFiltered,
        "insufficient_system_resource" => StopReason::ProviderResourceExhausted,
        _ => StopReason::Error(reason.to_owned()),
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    id: String,
    #[serde(default)]
    choices: Vec<ChatCompletionChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChoice {
    message: ChatMessageResponse,
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ChatMessageResponse {
    content: Option<String>,
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ChatToolCall>,
}

#[derive(Debug, Deserialize)]
struct ChatToolCall {
    id: String,
    function: ChatToolCallFunction,
}

#[derive(Debug, Deserialize)]
struct ChatToolCallFunction {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    prompt_tokens_details: Option<PromptTokensDetails>,
    prompt_cache_miss_tokens: Option<u64>,
    prompt_cache_hit_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PromptTokensDetails {
    cached_tokens: Option<u64>,
}

use std::collections::BTreeMap;

use futures::stream;
use harness_contracts::{Message, MessagePart, MessageRole, ModelError, StopReason, UsageSnapshot};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{
    ContentDelta, ContentType, InferContext, ModelRequest, ModelStream, ModelStreamEvent,
    ThinkingDelta,
};

use super::continuation;
use super::dialect::OpenAiChatDialect;
use super::request::{chat_message, merge_extra_object, openai_tool, DEFAULT_MAX_TOKENS};

pub(crate) struct EncodedChatMessages {
    pub(crate) messages: Vec<Value>,
    pub(crate) replayed_provider_reasoning: bool,
}

pub(crate) async fn chat_messages_for_request(
    req: &ModelRequest,
    dialect: OpenAiChatDialect,
    ctx: &InferContext,
) -> Result<EncodedChatMessages, ModelError> {
    let kimi_web_search = dialect == OpenAiChatDialect::Kimi && kimi_has_web_search(req)?;
    let continuation_required = match dialect {
        OpenAiChatDialect::DeepSeek | OpenAiChatDialect::Zhipu => true,
        OpenAiChatDialect::Kimi => kimi_reasoning_continuation_required(req, kimi_web_search)?,
        _ => false,
    };
    let mut messages = Vec::new();
    let mut tool_call_names = BTreeMap::new();
    let mut replayed_provider_reasoning = false;

    if let Some(system) = &req.system {
        messages.push(json!({
            "role": "system",
            "content": system,
        }));
    }
    for message in &req.messages {
        let mut encoded = chat_message(message, dialect, ctx, &tool_call_names).await?;
        replayed_provider_reasoning |= continuation::apply_chat_message_continuation(
            &mut encoded,
            message,
            dialect,
            &req.provider_context.continuations,
            continuation_required,
        )?;
        messages.push(encoded);
        remember_assistant_tool_names(message, &mut tool_call_names);
    }

    Ok(EncodedChatMessages {
        messages,
        replayed_provider_reasoning,
    })
}

pub(super) async fn chat_request_body(
    req: &ModelRequest,
    max_tokens_field: &str,
    dialect: OpenAiChatDialect,
    ctx: &InferContext,
) -> Result<Value, ModelError> {
    let encoded = chat_messages_for_request(req, dialect, ctx).await?;
    let mut body = json!({
        "model": req.model_id,
        "messages": encoded.messages,
        "stream": req.stream,
    });
    body[max_tokens_field] = json!(req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS));

    if dialect == OpenAiChatDialect::MiniMax && req.extra.get("reasoning_split").is_none() {
        body["reasoning_split"] = json!(true);
    }

    if req.stream {
        body["stream_options"] = json!({ "include_usage": true });
    }

    if dialect == OpenAiChatDialect::Kimi {
        apply_kimi_request_options(&mut body, req, encoded.replayed_provider_reasoning)?;
    } else {
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

    if matches!(dialect, OpenAiChatDialect::Qwen | OpenAiChatDialect::Doubao) {
        if let Some(reasoning_content) = choice.message.reasoning_content {
            if !reasoning_content.is_empty() {
                let provider_native = choice
                    .message
                    .encrypted_content
                    .filter(|value| !value.is_empty())
                    .map(|encrypted_content| {
                        json!({
                            "encrypted_content": encrypted_content,
                        })
                    });
                events.push(ModelStreamEvent::ContentBlockStart {
                    index,
                    content_type: ContentType::Thinking,
                });
                events.push(ModelStreamEvent::ContentBlockDelta {
                    index,
                    delta: ContentDelta::Thinking(ThinkingDelta {
                        text: Some(reasoning_content),
                        provider_native,
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

fn remember_assistant_tool_names(
    message: &Message,
    tool_call_names: &mut BTreeMap<String, String>,
) {
    if message.role != MessageRole::Assistant {
        return;
    }
    for part in &message.parts {
        if let MessagePart::ToolUse { id, name, .. } = part {
            tool_call_names.insert(id.to_string(), name.clone());
        }
    }
}

fn apply_kimi_request_options(
    body: &mut Value,
    req: &ModelRequest,
    replayed_provider_reasoning: bool,
) -> Result<(), ModelError> {
    let mut extra = extra_object(&req.extra)?.cloned().unwrap_or_default();
    let extra_tools = extra.remove("tools");
    let extra_thinking = extra.remove("thinking");

    if extra.remove("max_tokens").is_some() {
        return Err(ModelError::InvalidRequest(
            "Kimi Chat Completions uses max_completion_tokens instead of max_tokens".to_owned(),
        ));
    }

    let is_k2 = is_kimi_k2_model(&req.model_id);
    if is_k2 {
        if req.temperature.is_some() {
            return Err(ModelError::InvalidRequest(
                "Kimi K2 models do not accept temperature through the typed request field"
                    .to_owned(),
            ));
        }
    } else if let Some(temperature) = req.temperature {
        body["temperature"] = json!(temperature);
    }

    let mut tools = Vec::new();
    if let Some(request_tools) = &req.tools {
        tools.extend(request_tools.iter().map(openai_tool));
    }
    if let Some(extra_tools) = extra_tools {
        let extra_tools = extra_tools.as_array().ok_or_else(|| {
            ModelError::InvalidRequest("Kimi extra.tools must be an array".to_owned())
        })?;
        tools.extend(extra_tools.iter().cloned());
    }
    if !tools.is_empty() {
        validate_kimi_tools(&tools)?;
        body["tools"] = Value::Array(tools.clone());
    }
    let has_web_search = kimi_tools_contain_web_search(&tools);

    let thinking = kimi_thinking_value(
        &req.model_id,
        extra_thinking,
        has_web_search,
        replayed_provider_reasoning,
    )?;

    if is_k2 {
        let thinking_disabled = thinking
            .as_ref()
            .and_then(|thinking| thinking.get("type"))
            .and_then(Value::as_str)
            == Some("disabled");
        for key in [
            "temperature",
            "top_p",
            "n",
            "presence_penalty",
            "frequency_penalty",
        ] {
            if let Some(value) = extra.get(key) {
                validate_kimi_k2_extra_parameter(key, value, thinking_disabled)?;
            }
        }
    }

    for (key, value) in extra {
        body[&key] = value;
    }

    if let Some(thinking) = thinking {
        body["thinking"] = thinking;
    }

    Ok(())
}

fn kimi_has_web_search(req: &ModelRequest) -> Result<bool, ModelError> {
    let Some(extra) = extra_object(&req.extra)? else {
        return Ok(false);
    };
    let Some(tools) = extra.get("tools") else {
        return Ok(false);
    };
    let tools = tools.as_array().ok_or_else(|| {
        ModelError::InvalidRequest("Kimi extra.tools must be an array".to_owned())
    })?;
    Ok(kimi_tools_contain_web_search(tools))
}

fn kimi_reasoning_continuation_required(
    req: &ModelRequest,
    has_web_search: bool,
) -> Result<bool, ModelError> {
    if !is_kimi_k2_model(&req.model_id) {
        return Ok(false);
    }
    if is_kimi_k27_model(&req.model_id) {
        return Ok(true);
    }
    if has_web_search || kimi_extra_thinking_disabled(&req.extra)? {
        return Ok(false);
    }
    Ok(true)
}

fn extra_object(value: &Value) -> Result<Option<&Map<String, Value>>, ModelError> {
    if value.is_null() {
        return Ok(None);
    }
    value.as_object().map(Some).ok_or_else(|| {
        ModelError::InvalidRequest("model request extra must be an object".to_owned())
    })
}

fn validate_kimi_k2_extra_parameter(
    key: &str,
    value: &Value,
    thinking_disabled: bool,
) -> Result<(), ModelError> {
    let invalid =
        || ModelError::InvalidRequest(format!("Kimi K2 extra.{key} has an invalid value"));
    match key {
        "temperature" => number_equals(value, if thinking_disabled { 0.6 } else { 1.0 })
            .then_some(())
            .ok_or_else(invalid),
        "top_p" => number_equals(value, 0.95).then_some(()).ok_or_else(invalid),
        "presence_penalty" | "frequency_penalty" => {
            number_equals(value, 0.0).then_some(()).ok_or_else(invalid)
        }
        "n" => value
            .as_u64()
            .filter(|count| *count == 1)
            .map(|_| ())
            .ok_or_else(invalid),
        _ => Ok(()),
    }
}

fn number_equals(value: &Value, expected: f64) -> bool {
    value
        .as_f64()
        .is_some_and(|number| number.is_finite() && (number - expected).abs() < 0.000_001)
}

fn validate_kimi_tools(tools: &[Value]) -> Result<(), ModelError> {
    if tools.len() > 128 {
        return Err(ModelError::InvalidRequest(
            "Kimi Chat Completions accepts at most 128 tools".to_owned(),
        ));
    }
    for tool in tools {
        let tool_type = tool.get("type").and_then(Value::as_str).ok_or_else(|| {
            ModelError::InvalidRequest("Kimi tools must include a type".to_owned())
        })?;
        let name = tool
            .get("function")
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ModelError::InvalidRequest("Kimi tools must include function.name".to_owned())
            })?;
        match tool_type {
            "function" => {
                if !kimi_function_name_is_valid(name) {
                    return Err(ModelError::InvalidRequest(format!(
                        "Kimi function tool name does not match ^[a-zA-Z_][a-zA-Z0-9-_]{{2,63}}$: {name}"
                    )));
                }
            }
            "builtin_function" if name == "$web_search" => {}
            "builtin_function" => {
                return Err(ModelError::InvalidRequest(format!(
                    "unsupported Kimi builtin tool: {name}"
                )));
            }
            _ => {
                return Err(ModelError::InvalidRequest(format!(
                    "unsupported Kimi tool type: {tool_type}"
                )));
            }
        }
    }
    Ok(())
}

fn kimi_function_name_is_valid(name: &str) -> bool {
    let bytes = name.as_bytes();
    if !(3..=64).contains(&bytes.len()) {
        return false;
    }
    let first = bytes[0];
    if !first.is_ascii_alphabetic() && first != b'_' {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn kimi_tools_contain_web_search(tools: &[Value]) -> bool {
    tools.iter().any(|tool| {
        tool.get("type").and_then(Value::as_str) == Some("builtin_function")
            && tool
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                == Some("$web_search")
    })
}

fn kimi_extra_thinking_disabled(extra: &Value) -> Result<bool, ModelError> {
    let Some(extra) = extra_object(extra)? else {
        return Ok(false);
    };
    Ok(extra
        .get("thinking")
        .and_then(|thinking| thinking.get("type"))
        .and_then(Value::as_str)
        == Some("disabled"))
}

fn kimi_thinking_value(
    model_id: &str,
    thinking: Option<Value>,
    has_web_search: bool,
    replayed_provider_reasoning: bool,
) -> Result<Option<Value>, ModelError> {
    if !is_kimi_k2_model(model_id) {
        return Ok(thinking);
    }
    let had_thinking = thinking.is_some();
    let mut thinking = match thinking {
        Some(Value::Object(map)) => map,
        Some(_) => {
            return Err(ModelError::InvalidRequest(
                "Kimi thinking must be an object".to_owned(),
            ));
        }
        None => Map::new(),
    };

    if is_kimi_k27_model(model_id) {
        if has_web_search {
            return Err(ModelError::InvalidRequest(
                "Kimi K2.7 does not support $web_search because thinking cannot be disabled"
                    .to_owned(),
            ));
        }
        validate_kimi_thinking_type(&thinking, true)?;
        validate_kimi_thinking_keep(&thinking, true)?;
        if replayed_provider_reasoning {
            thinking.insert("keep".to_owned(), Value::String("all".to_owned()));
        }
        return (!thinking.is_empty())
            .then_some(Value::Object(thinking))
            .transpose_ok();
    }

    if is_kimi_k26_model(model_id) {
        validate_kimi_thinking_type(&thinking, false)?;
        validate_kimi_thinking_keep(&thinking, true)?;
        if has_web_search {
            thinking.insert("type".to_owned(), Value::String("disabled".to_owned()));
            thinking.remove("keep");
            return Ok(Some(Value::Object(thinking)));
        }
        if replayed_provider_reasoning {
            if thinking.get("type").and_then(Value::as_str) == Some("disabled") {
                return Err(ModelError::InvalidRequest(
                    "Kimi K2.6 reasoning replay conflicts with thinking.type=disabled".to_owned(),
                ));
            }
            if let Some(keep) = thinking.get("keep") {
                if keep.as_str() != Some("all") {
                    return Err(ModelError::InvalidRequest(
                        "Kimi K2.6 reasoning replay requires thinking.keep=all".to_owned(),
                    ));
                }
            }
            thinking.insert("keep".to_owned(), Value::String("all".to_owned()));
        }
        return (had_thinking || replayed_provider_reasoning)
            .then_some(Value::Object(thinking))
            .transpose_ok();
    }

    validate_kimi_thinking_type(&thinking, false)?;
    if thinking.contains_key("keep") {
        return Err(ModelError::InvalidRequest(
            "Kimi K2.5 does not support thinking.keep".to_owned(),
        ));
    }
    if has_web_search {
        thinking.insert("type".to_owned(), Value::String("disabled".to_owned()));
        return Ok(Some(Value::Object(thinking)));
    }
    if replayed_provider_reasoning
        && thinking.get("type").and_then(Value::as_str) == Some("disabled")
    {
        return Err(ModelError::InvalidRequest(
            "Kimi K2.5 reasoning replay conflicts with thinking.type=disabled".to_owned(),
        ));
    }
    Ok(had_thinking.then_some(Value::Object(thinking)))
}

fn validate_kimi_thinking_type(
    thinking: &Map<String, Value>,
    enabled_only: bool,
) -> Result<(), ModelError> {
    let Some(value) = thinking.get("type") else {
        return Ok(());
    };
    let Some(value) = value.as_str() else {
        return Err(ModelError::InvalidRequest(
            "Kimi thinking.type must be a string".to_owned(),
        ));
    };
    match value {
        "enabled" => Ok(()),
        "disabled" if !enabled_only => Ok(()),
        "disabled" => Err(ModelError::InvalidRequest(
            "Kimi K2.7 does not allow thinking.type=disabled".to_owned(),
        )),
        _ => Err(ModelError::InvalidRequest(
            "Kimi thinking.type must be enabled or disabled".to_owned(),
        )),
    }
}

fn validate_kimi_thinking_keep(
    thinking: &Map<String, Value>,
    allow_all: bool,
) -> Result<(), ModelError> {
    let Some(value) = thinking.get("keep") else {
        return Ok(());
    };
    if value.is_null() {
        return Ok(());
    }
    if allow_all && value.as_str() == Some("all") {
        return Ok(());
    }
    Err(ModelError::InvalidRequest(
        "Kimi thinking.keep must be null or all".to_owned(),
    ))
}

fn is_kimi_k2_model(model_id: &str) -> bool {
    model_id.starts_with("kimi-k2.")
}

fn is_kimi_k26_model(model_id: &str) -> bool {
    model_id == "kimi-k2.6"
}

fn is_kimi_k27_model(model_id: &str) -> bool {
    matches!(model_id, "kimi-k2.7-code" | "kimi-k2.7-code-highspeed")
}

trait OptionValueExt {
    fn transpose_ok(self) -> Result<Option<Value>, ModelError>;
}

impl OptionValueExt for Option<Value> {
    fn transpose_ok(self) -> Result<Option<Value>, ModelError> {
        Ok(self)
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
    encrypted_content: Option<String>,
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

use std::collections::BTreeMap;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use futures::StreamExt;
use harness_contracts::{
    BlobRef, BlobStore, Message, MessagePart, MessageRole, ModelError, OpenAiResponsesOptions,
    OpenAiTextFormat, TenantId, ToolDescriptor, ToolResult, ToolResultPart,
};
use serde_json::{json, Map, Value};

use crate::InferContext;

use super::dialect::OpenAiChatDialect;

pub(super) const DEFAULT_MAX_TOKENS: u32 = 1024;

pub(super) fn merge_extra_object(body: &mut Value, extra: &Value) -> Result<(), ModelError> {
    merge_extra_object_protecting(body, extra, &[])
}

pub(super) fn merge_extra_object_protecting(
    body: &mut Value,
    extra: &Value,
    protected_keys: &[&str],
) -> Result<(), ModelError> {
    if extra.is_null() {
        return Ok(());
    }
    let extra = extra.as_object().ok_or_else(|| {
        ModelError::InvalidRequest("model request extra must be an object".to_owned())
    })?;
    for (key, value) in extra {
        if protected_keys.iter().any(|protected| protected == key) {
            return Err(ModelError::InvalidRequest(format!(
                "model request extra conflicts with typed OpenAI Responses option: {key}"
            )));
        }
        body[key] = value.clone();
    }
    Ok(())
}

pub(super) async fn chat_message(
    message: &Message,
    dialect: OpenAiChatDialect,
    ctx: &InferContext,
    tool_call_names: &BTreeMap<String, String>,
) -> Result<Value, ModelError> {
    match message.role {
        MessageRole::System => Ok(json!({
            "role": "system",
            "content": text_content(&message.parts, ctx, dialect).await?,
        })),
        MessageRole::User => Ok(json!({
            "role": "user",
            "content": text_content(&message.parts, ctx, dialect).await?,
        })),
        MessageRole::Assistant => assistant_message(&message.parts),
        MessageRole::Tool => tool_message(&message.parts, dialect, ctx, tool_call_names).await,
        _ => Err(ModelError::InvalidRequest(
            "unknown message role is not supported by OpenAI protocol providers".to_owned(),
        )),
    }
}

pub(super) async fn responses_message(
    message: &Message,
    ctx: &InferContext,
) -> Result<Vec<Value>, ModelError> {
    match message.role {
        MessageRole::System => Ok(vec![json!({
            "role": "system",
            "content": responses_input_content(&message.parts, ctx).await?,
        })]),
        MessageRole::User => Ok(vec![json!({
            "role": "user",
            "content": responses_input_content(&message.parts, ctx).await?,
        })]),
        MessageRole::Assistant => responses_assistant_items(&message.parts),
        MessageRole::Tool => Ok(vec![responses_tool_output_item(&message.parts)?]),
        _ => Err(ModelError::InvalidRequest(
            "unknown message role is not supported by OpenAI Responses providers".to_owned(),
        )),
    }
}

fn assistant_message(parts: &[MessagePart]) -> Result<Value, ModelError> {
    let mut text = Vec::new();
    let mut tool_calls = Vec::new();

    for part in parts {
        match part {
            MessagePart::Text(value) => text.push(value.clone()),
            MessagePart::ToolUse { id, name, input } => tool_calls.push(json!({
                "id": id.to_string(),
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": input.to_string(),
                },
            })),
            MessagePart::Image { .. }
            | MessagePart::Video { .. }
            | MessagePart::File { .. }
            | MessagePart::ProviderFileReference { .. }
            | MessagePart::Thinking(_)
            | MessagePart::ToolResult { .. } => {
                return Err(ModelError::InvalidRequest(
                    "assistant messages only support text and tool use parts for OpenAI protocol providers"
                        .to_owned(),
                ));
            }
            _ => {
                return Err(ModelError::InvalidRequest(
                    "unsupported assistant message part for OpenAI protocol providers".to_owned(),
                ));
            }
        }
    }

    let mut message = json!({
        "role": "assistant",
        "content": if text.is_empty() {
            Value::Null
        } else {
            Value::String(text.join(""))
        },
    });
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    Ok(message)
}

async fn tool_message(
    parts: &[MessagePart],
    dialect: OpenAiChatDialect,
    ctx: &InferContext,
    tool_call_names: &BTreeMap<String, String>,
) -> Result<Value, ModelError> {
    let [MessagePart::ToolResult {
        tool_use_id,
        content,
    }] = parts
    else {
        return Err(ModelError::InvalidRequest(
            "tool messages must contain exactly one tool result part for OpenAI protocol providers"
                .to_owned(),
        ));
    };

    let kimi_tool_name = if dialect == OpenAiChatDialect::Kimi {
        Some(
            tool_call_names
                .get(&tool_use_id.to_string())
                .ok_or_else(|| {
                    ModelError::InvalidRequest(
                        "Kimi tool messages require a preceding assistant tool call name"
                            .to_owned(),
                    )
                })?
                .clone(),
        )
    } else {
        None
    };

    let mut message = json!({
        "role": "tool",
        "tool_call_id": tool_use_id.to_string(),
        "content": if dialect == OpenAiChatDialect::Kimi {
            if kimi_tool_result_needs_content_parts(content) {
                kimi_tool_result_content(content, ctx).await?
            } else {
                Value::String(kimi_tool_result_text_content(content)?)
            }
        } else {
            Value::String(tool_result_content(content)?)
        },
    });

    if let Some(name) = kimi_tool_name {
        message["name"] = Value::String(name);
    }

    Ok(message)
}

fn responses_assistant_items(parts: &[MessagePart]) -> Result<Vec<Value>, ModelError> {
    let mut text = String::new();
    let mut items = Vec::new();

    for part in parts {
        match part {
            MessagePart::Text(value) => text.push_str(value),
            MessagePart::ToolUse { id, name, input } => items.push(json!({
                "type": "function_call",
                "call_id": id.to_string(),
                "name": name,
                "arguments": input.to_string(),
            })),
            MessagePart::Image { .. }
            | MessagePart::Video { .. }
            | MessagePart::File { .. }
            | MessagePart::ProviderFileReference { .. }
            | MessagePart::Thinking(_)
            | MessagePart::ToolResult { .. } => {
                return Err(ModelError::InvalidRequest(
                    "assistant messages only support text and tool use parts for OpenAI Responses providers"
                        .to_owned(),
                ));
            }
            _ => {
                return Err(ModelError::InvalidRequest(
                    "unsupported assistant message part for OpenAI Responses providers".to_owned(),
                ));
            }
        }
    }

    if !text.is_empty() {
        items.insert(
            0,
            json!({
                "role": "assistant",
                "content": [{"type": "output_text", "text": text}],
            }),
        );
    }
    Ok(items)
}

fn responses_tool_output_item(parts: &[MessagePart]) -> Result<Value, ModelError> {
    let [MessagePart::ToolResult {
        tool_use_id,
        content,
    }] = parts
    else {
        return Err(ModelError::InvalidRequest(
            "tool messages must contain exactly one tool result part for OpenAI Responses providers"
                .to_owned(),
        ));
    };

    Ok(json!({
        "type": "function_call_output",
        "call_id": tool_use_id.to_string(),
        "output": tool_result_content(content)?,
    }))
}

async fn text_content(
    parts: &[MessagePart],
    ctx: &InferContext,
    dialect: OpenAiChatDialect,
) -> Result<Value, ModelError> {
    if parts
        .iter()
        .all(|part| matches!(part, MessagePart::Text(_)))
    {
        let mut text = String::new();
        for part in parts {
            if let MessagePart::Text(value) = part {
                text.push_str(value);
            }
        }
        return Ok(Value::String(text));
    }
    content_parts(parts, ctx, dialect).await
}

async fn content_parts(
    parts: &[MessagePart],
    ctx: &InferContext,
    dialect: OpenAiChatDialect,
) -> Result<Value, ModelError> {
    let mut content = Vec::new();
    for part in parts {
        match part {
            MessagePart::Text(value) => content.push(json!({
                "type": "text",
                "text": value,
            })),
            MessagePart::Image {
                mime_type,
                blob_ref,
            } => content.push(json!({
                "type": "image_url",
                "image_url": {
                    "url": blob_data_url(ctx, mime_type, blob_ref).await?
                },
            })),
            MessagePart::Video {
                mime_type,
                blob_ref,
            } => content.push(json!({
                "type": "video_url",
                "video_url": {
                    "url": blob_data_url(ctx, mime_type, blob_ref).await?
                },
            })),
            MessagePart::File { .. } => {
                return Err(ModelError::InvalidRequest(
                    "file message parts require provider-specific upload support for OpenAI protocol providers"
                        .to_owned(),
                ));
            }
            MessagePart::ProviderFileReference {
                provider_id,
                file_id,
                mime_type,
            } => {
                if dialect == OpenAiChatDialect::Kimi && provider_id == "km" {
                    if mime_type.starts_with("image/") {
                        content.push(json!({
                            "type": "image_url",
                            "image_url": {
                                "url": format!("ms://{file_id}")
                            },
                        }));
                        continue;
                    }
                    if mime_type.starts_with("video/") {
                        content.push(json!({
                            "type": "video_url",
                            "video_url": {
                                "url": format!("ms://{file_id}")
                            },
                        }));
                        continue;
                    }
                    return Err(ModelError::InvalidRequest(
                        "Kimi provider file references only support Kimi image and video files"
                            .to_owned(),
                    ));
                }
                if dialect == OpenAiChatDialect::MiniMax
                    && provider_id == "minimax"
                    && mime_type.starts_with("video/")
                {
                    content.push(json!({
                        "type": "video_url",
                        "video_url": {
                            "url": format!("mm_file://{file_id}")
                        },
                    }));
                    continue;
                }
                return Err(ModelError::InvalidRequest(
                    "provider file reference is not supported by this OpenAI protocol provider"
                        .to_owned(),
                ));
            }
            MessagePart::Thinking(_) => {
                return Err(ModelError::InvalidRequest(
                    "thinking message parts are not supported by OpenAI protocol providers"
                        .to_owned(),
                ));
            }
            MessagePart::ToolUse { .. } | MessagePart::ToolResult { .. } => {
                return Err(ModelError::InvalidRequest(
                    "tool message parts must use assistant/tool roles for OpenAI protocol providers"
                        .to_owned(),
                ));
            }
            _ => {
                return Err(ModelError::InvalidRequest(
                    "unsupported message part for OpenAI protocol providers".to_owned(),
                ));
            }
        }
    }
    Ok(Value::Array(content))
}

async fn responses_input_content(
    parts: &[MessagePart],
    ctx: &InferContext,
) -> Result<Value, ModelError> {
    let mut content = Vec::new();
    for part in parts {
        match part {
            MessagePart::Text(value) => content.push(json!({
                "type": "input_text",
                "text": value,
            })),
            MessagePart::Image {
                mime_type,
                blob_ref,
            } => content.push(json!({
                "type": "input_image",
                "image_url": blob_data_url(ctx, mime_type, blob_ref).await?,
            })),
            MessagePart::File {
                mime_type,
                blob_ref,
            } => content.push(json!({
                "type": "input_file",
                "file_data": blob_data_url(ctx, mime_type, blob_ref).await?,
                "filename": blob_ref.id.to_string(),
            })),
            MessagePart::Video { .. } => {
                return Err(ModelError::InvalidRequest(
                    "video message parts are not supported by OpenAI Responses providers"
                        .to_owned(),
                ));
            }
            MessagePart::Thinking(_) => {
                return Err(ModelError::InvalidRequest(
                    "thinking message parts are not supported by OpenAI Responses providers"
                        .to_owned(),
                ));
            }
            MessagePart::ToolUse { .. } | MessagePart::ToolResult { .. } => {
                return Err(ModelError::InvalidRequest(
                    "tool message parts must use assistant/tool roles for OpenAI Responses providers"
                        .to_owned(),
                ));
            }
            _ => {
                return Err(ModelError::InvalidRequest(
                    "unsupported message part for OpenAI Responses providers".to_owned(),
                ));
            }
        }
    }
    Ok(Value::Array(content))
}

async fn blob_data_url(
    ctx: &InferContext,
    mime_type: &str,
    blob_ref: &BlobRef,
) -> Result<String, ModelError> {
    let store = ctx.blob_store.as_ref().ok_or_else(|| {
        ModelError::InvalidRequest("blob store is required for multimodal model input".to_owned())
    })?;
    let bytes = read_blob_bytes(store.as_ref(), ctx.tenant_id, blob_ref).await?;
    Ok(format!(
        "data:{};base64,{}",
        mime_type,
        BASE64_STANDARD.encode(bytes)
    ))
}

async fn read_blob_bytes(
    store: &dyn BlobStore,
    tenant_id: TenantId,
    blob_ref: &BlobRef,
) -> Result<Vec<u8>, ModelError> {
    let mut stream = store.get(tenant_id, blob_ref).await.map_err(|_| {
        ModelError::InvalidRequest("failed to read multimodal input blob".to_owned())
    })?;
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn tool_result_content(content: &ToolResult) -> Result<String, ModelError> {
    match content {
        ToolResult::Text(text) => Ok(text.clone()),
        ToolResult::Structured(value) => Ok(value.to_string()),
        ToolResult::Blob { .. } => Err(ModelError::InvalidRequest(
            "blob tool results are not supported by OpenAI protocol providers".to_owned(),
        )),
        ToolResult::Mixed(parts) => parts
            .iter()
            .map(tool_result_part_content)
            .collect::<Result<Vec<_>, _>>()
            .map(|parts| parts.join("")),
        _ => Err(ModelError::InvalidRequest(
            "unsupported tool result for OpenAI protocol providers".to_owned(),
        )),
    }
}

fn tool_result_part_content(part: &ToolResultPart) -> Result<String, ModelError> {
    match part {
        ToolResultPart::Structured { value, .. } => Ok(value.to_string()),
        ToolResultPart::Text { text } | ToolResultPart::Code { text, .. } => Ok(text.clone()),
        ToolResultPart::Reference { summary, .. } => Ok(summary.clone().unwrap_or_default()),
        ToolResultPart::Blob { .. } => Err(ModelError::InvalidRequest(
            "blob tool result parts are not supported by OpenAI protocol providers".to_owned(),
        )),
        ToolResultPart::Artifact { .. } => Err(ModelError::InvalidRequest(
            "artifact tool result parts are not supported by OpenAI protocol providers".to_owned(),
        )),
        _ => Err(ModelError::InvalidRequest(
            "unsupported tool result part for OpenAI protocol providers".to_owned(),
        )),
    }
}

fn kimi_tool_result_needs_content_parts(content: &ToolResult) -> bool {
    match content {
        ToolResult::Blob { .. } => true,
        ToolResult::Mixed(parts) => parts
            .iter()
            .any(|part| matches!(part, ToolResultPart::Blob { .. })),
        _ => false,
    }
}

fn kimi_tool_result_text_content(content: &ToolResult) -> Result<String, ModelError> {
    match content {
        ToolResult::Text(text) => Ok(text.clone()),
        ToolResult::Structured(value) => Ok(value.to_string()),
        ToolResult::Mixed(parts) => parts
            .iter()
            .map(kimi_tool_result_part_text_content)
            .collect::<Result<Vec<_>, _>>()
            .map(|parts| parts.join("")),
        ToolResult::Blob { .. } => Err(ModelError::InvalidRequest(
            "blob tool results require Kimi multimodal content parts".to_owned(),
        )),
        _ => Err(ModelError::InvalidRequest(
            "unsupported Kimi tool result for OpenAI protocol providers".to_owned(),
        )),
    }
}

fn kimi_tool_result_part_text_content(part: &ToolResultPart) -> Result<String, ModelError> {
    match part {
        ToolResultPart::Structured { value, .. } => Ok(value.to_string()),
        ToolResultPart::Text { text } | ToolResultPart::Code { text, .. } => Ok(text.clone()),
        ToolResultPart::Reference { title, summary, .. } => Ok(summary
            .as_deref()
            .or(title.as_deref())
            .unwrap_or_default()
            .to_owned()),
        ToolResultPart::Artifact { title, preview, .. } => {
            Ok(preview.as_deref().unwrap_or(title).to_owned())
        }
        ToolResultPart::Table { .. }
        | ToolResultPart::Progress { .. }
        | ToolResultPart::Error { .. } => serde_json::to_string(part).map_err(|error| {
            ModelError::InvalidRequest(format!("failed to encode Kimi tool result: {error}"))
        }),
        ToolResultPart::Blob { .. } => Err(ModelError::InvalidRequest(
            "blob tool result parts require Kimi multimodal content parts".to_owned(),
        )),
        _ => Err(ModelError::InvalidRequest(
            "unsupported Kimi tool result part for OpenAI protocol providers".to_owned(),
        )),
    }
}

async fn kimi_tool_result_content(
    content: &ToolResult,
    ctx: &InferContext,
) -> Result<Value, ModelError> {
    let blocks = match content {
        ToolResult::Text(text) => vec![text_block(text)],
        ToolResult::Structured(value) => vec![text_block(&value.to_string())],
        ToolResult::Blob {
            content_type,
            blob_ref,
        } => vec![kimi_blob_block(ctx, content_type, blob_ref).await?],
        ToolResult::Mixed(parts) => {
            let mut blocks = Vec::new();
            for part in parts {
                blocks.push(kimi_tool_result_part_block(part, ctx).await?);
            }
            blocks
        }
        _ => {
            return Err(ModelError::InvalidRequest(
                "unsupported Kimi tool result for OpenAI protocol providers".to_owned(),
            ));
        }
    };
    Ok(Value::Array(blocks))
}

async fn kimi_tool_result_part_block(
    part: &ToolResultPart,
    ctx: &InferContext,
) -> Result<Value, ModelError> {
    match part {
        ToolResultPart::Structured { value, .. } => Ok(text_block(&value.to_string())),
        ToolResultPart::Text { text } | ToolResultPart::Code { text, .. } => Ok(text_block(text)),
        ToolResultPart::Reference { title, summary, .. } => Ok(text_block(
            summary.as_deref().or(title.as_deref()).unwrap_or_default(),
        )),
        ToolResultPart::Artifact { title, preview, .. } => {
            Ok(text_block(preview.as_deref().unwrap_or(title)))
        }
        ToolResultPart::Blob {
            content_type,
            blob_ref,
            ..
        } => kimi_blob_block(ctx, content_type, blob_ref).await,
        ToolResultPart::Table { .. }
        | ToolResultPart::Progress { .. }
        | ToolResultPart::Error { .. } => Ok(text_block(&serde_json::to_string(part).map_err(
            |error| {
                ModelError::InvalidRequest(format!("failed to encode Kimi tool result: {error}"))
            },
        )?)),
        _ => Err(ModelError::InvalidRequest(
            "unsupported Kimi tool result part for OpenAI protocol providers".to_owned(),
        )),
    }
}

async fn kimi_blob_block(
    ctx: &InferContext,
    content_type: &str,
    blob_ref: &BlobRef,
) -> Result<Value, ModelError> {
    let url = blob_data_url(ctx, content_type, blob_ref).await?;
    if content_type.starts_with("image/") {
        return Ok(json!({
            "type": "image_url",
            "image_url": { "url": url },
        }));
    }
    if content_type.starts_with("video/") {
        return Ok(json!({
            "type": "video_url",
            "video_url": { "url": url },
        }));
    }
    Err(ModelError::InvalidRequest(
        "Kimi tool result blobs must be image or video content".to_owned(),
    ))
}

fn text_block(text: &str) -> Value {
    json!({
        "type": "text",
        "text": text,
    })
}

pub(super) fn openai_tool(tool: &ToolDescriptor) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema,
        },
    })
}

pub(super) fn responses_tool(tool: &ToolDescriptor, strict: bool) -> Value {
    let mut tool_value = json!({
        "type": "function",
        "name": tool.name,
        "description": tool.description,
        "parameters": tool.input_schema,
    });
    if strict {
        tool_value["strict"] = Value::Bool(true);
    }
    tool_value
}

pub(super) fn apply_openai_responses_options(
    body: &mut Value,
    options: &OpenAiResponsesOptions,
) -> Vec<&'static str> {
    let mut keys = Vec::new();
    if let Some(background) = options.background {
        body["background"] = Value::Bool(background);
        keys.push("background");
    }
    if let Some(conversation) = &options.conversation {
        body["conversation"] = conversation.clone();
        keys.push("conversation");
    }
    if !options.include.is_empty() {
        body["include"] = json!(options.include);
        keys.push("include");
    }
    if let Some(instructions) = &options.instructions {
        body["instructions"] = json!(instructions);
        keys.push("instructions");
    }
    if let Some(max_tool_calls) = options.max_tool_calls {
        body["max_tool_calls"] = json!(max_tool_calls);
        keys.push("max_tool_calls");
    }
    if let Some(prompt) = &options.prompt {
        body["prompt"] = prompt.clone();
        keys.push("prompt");
    }
    if let Some(prompt_cache_key) = &options.prompt_cache_key {
        body["prompt_cache_key"] = json!(prompt_cache_key);
        keys.push("prompt_cache_key");
    }
    if let Some(prompt_cache_retention) = &options.prompt_cache_retention {
        body["prompt_cache_retention"] = json!(prompt_cache_retention);
        keys.push("prompt_cache_retention");
    }
    if let Some(reasoning) = &options.reasoning {
        let mut value = Map::new();
        if let Some(effort) = &reasoning.effort {
            value.insert("effort".to_owned(), json!(effort));
        }
        if let Some(summary) = &reasoning.summary {
            value.insert("summary".to_owned(), json!(summary));
        }
        if let Some(context) = &reasoning.context {
            value.insert("context".to_owned(), json!(context));
        }
        body["reasoning"] = Value::Object(value);
        keys.push("reasoning");
    }
    if let Some(safety_identifier) = &options.safety_identifier {
        body["safety_identifier"] = json!(safety_identifier);
        keys.push("safety_identifier");
    }
    if let Some(service_tier) = &options.service_tier {
        body["service_tier"] = json!(service_tier);
        keys.push("service_tier");
    }
    if let Some(text) = &options.text {
        let mut value = Map::new();
        if let Some(verbosity) = &text.verbosity {
            value.insert("verbosity".to_owned(), json!(verbosity));
        }
        if let Some(format) = &text.format {
            value.insert("format".to_owned(), openai_text_format(format));
        }
        body["text"] = Value::Object(value);
        keys.push("text");
    }
    if let Some(top_logprobs) = options.top_logprobs {
        body["top_logprobs"] = json!(top_logprobs);
        keys.push("top_logprobs");
    }
    if let Some(top_p) = &options.top_p {
        body["top_p"] = top_p.clone();
        keys.push("top_p");
    }
    if let Some(tool_choice) = &options.tool_choice {
        body["tool_choice"] = tool_choice.clone();
        keys.push("tool_choice");
    }
    if let Some(parallel_tool_calls) = options.parallel_tool_calls {
        body["parallel_tool_calls"] = Value::Bool(parallel_tool_calls);
        keys.push("parallel_tool_calls");
    }
    if let Some(truncation) = &options.truncation {
        body["truncation"] = json!(truncation);
        keys.push("truncation");
    }
    if let Some(store) = options.store {
        body["store"] = Value::Bool(store);
        keys.push("store");
    }
    if let Some(metadata) = &options.metadata {
        body["metadata"] = json!(metadata);
        keys.push("metadata");
    }
    if let Some(user) = &options.user {
        body["user"] = json!(user);
        keys.push("user");
    }
    keys
}

fn openai_text_format(format: &OpenAiTextFormat) -> Value {
    match format {
        OpenAiTextFormat::JsonSchema {
            name,
            schema,
            strict,
        } => {
            let mut value = json!({
                "type": "json_schema",
                "name": name,
                "schema": schema,
            });
            if let Some(strict) = strict {
                value["strict"] = Value::Bool(*strict);
            }
            value
        }
    }
}

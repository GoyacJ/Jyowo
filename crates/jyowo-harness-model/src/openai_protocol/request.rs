use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use futures::StreamExt;
use harness_contracts::{
    BlobRef, BlobStore, Message, MessagePart, MessageRole, ModelError, OpenAiResponsesOptions,
    OpenAiTextFormat, TenantId, ToolDescriptor, ToolResult, ToolResultPart,
};
use serde_json::{json, Map, Value};

use crate::InferContext;

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
    ctx: &InferContext,
) -> Result<Value, ModelError> {
    match message.role {
        MessageRole::System => Ok(json!({
            "role": "system",
            "content": text_content(&message.parts, ctx).await?,
        })),
        MessageRole::User => Ok(json!({
            "role": "user",
            "content": text_content(&message.parts, ctx).await?,
        })),
        MessageRole::Assistant => assistant_message(&message.parts),
        MessageRole::Tool => tool_message(&message.parts),
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

fn tool_message(parts: &[MessagePart]) -> Result<Value, ModelError> {
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

    Ok(json!({
        "role": "tool",
        "tool_call_id": tool_use_id.to_string(),
        "content": tool_result_content(content)?,
    }))
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

async fn text_content(parts: &[MessagePart], ctx: &InferContext) -> Result<Value, ModelError> {
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
    content_parts(parts, ctx).await
}

async fn content_parts(parts: &[MessagePart], ctx: &InferContext) -> Result<Value, ModelError> {
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
    if let Some(reasoning) = &options.reasoning {
        let mut value = Map::new();
        if let Some(effort) = &reasoning.effort {
            value.insert("effort".to_owned(), json!(effort));
        }
        if let Some(summary) = &reasoning.summary {
            value.insert("summary".to_owned(), json!(summary));
        }
        body["reasoning"] = Value::Object(value);
        keys.push("reasoning");
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

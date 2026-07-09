use harness_contracts::{Message, MessageRole, ModelError};
use harness_provider_state::{ProviderContinuationKind, ProviderContinuationRecord};
use serde_json::{json, Value};

use crate::ModelStreamEvent;

use super::dialect::OpenAiChatDialect;

const DEEPSEEK_REASONING_FIELD: &str = "reasoning_content";
const DEEPSEEK_REASONING_PAYLOAD_FORMAT: &str = "deepseek.reasoning_content.v1";
const MINIMAX_REASONING_CONTENT_FIELD: &str = "reasoning_content";
const MINIMAX_REASONING_DETAILS_FIELD: &str = "reasoning_details";
const MINIMAX_REASONING_PAYLOAD_FORMAT: &str = "minimax.reasoning_details.v1";

pub(super) fn deepseek_stream_reasoning_delta(
    dialect: OpenAiChatDialect,
    chunk: &Value,
) -> Option<String> {
    if dialect != OpenAiChatDialect::DeepSeek {
        return None;
    }
    let reasoning = chunk
        .get("choices")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|choice| {
            choice
                .get("delta")
                .and_then(|delta| delta.get(DEEPSEEK_REASONING_FIELD))
                .and_then(Value::as_str)
        })
        .filter(|value| !value.is_empty())
        .collect::<String>();
    (!reasoning.is_empty()).then_some(reasoning)
}

pub(super) fn stream_reasoning_continuation_payload(
    dialect: OpenAiChatDialect,
    chunk: &Value,
) -> Option<Value> {
    match dialect {
        OpenAiChatDialect::DeepSeek => {
            deepseek_stream_reasoning_delta(dialect, chunk).map(|reasoning| {
                json!({
                    "format": DEEPSEEK_REASONING_PAYLOAD_FORMAT,
                    "reasoningContent": reasoning,
                })
            })
        }
        OpenAiChatDialect::MiniMax => minimax_stream_reasoning_payload(chunk),
        _ => None,
    }
}

pub(super) fn deepseek_stream_continuation_event(
    dialect: OpenAiChatDialect,
    reasoning: &str,
) -> Option<ModelStreamEvent> {
    if dialect != OpenAiChatDialect::DeepSeek || reasoning.is_empty() {
        return None;
    }
    Some(reasoning_continuation_event(reasoning))
}

pub(super) fn stream_continuation_event(
    dialect: OpenAiChatDialect,
    payloads: &[Value],
) -> Option<ModelStreamEvent> {
    if payloads.is_empty() {
        return None;
    }

    match dialect {
        OpenAiChatDialect::DeepSeek => {
            let reasoning = payloads
                .iter()
                .filter_map(|payload| payload.get("reasoningContent").and_then(Value::as_str))
                .collect::<String>();
            deepseek_stream_continuation_event(dialect, &reasoning)
        }
        OpenAiChatDialect::MiniMax => Some(ModelStreamEvent::ProviderContinuationDelta {
            kind: ProviderContinuationKind::ReasoningReplay,
            payload: minimax_payload_from_chunks(payloads),
        }),
        _ => None,
    }
}

pub(super) fn chat_response_continuation_event(
    dialect: OpenAiChatDialect,
    response: &Value,
) -> Option<ModelStreamEvent> {
    match dialect {
        OpenAiChatDialect::DeepSeek => deepseek_chat_response_continuation_event(response),
        OpenAiChatDialect::MiniMax => minimax_chat_response_continuation_event(response),
        _ => None,
    }
}

fn deepseek_chat_response_continuation_event(response: &Value) -> Option<ModelStreamEvent> {
    let reasoning = response
        .get("choices")
        .and_then(Value::as_array)?
        .first()
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get(DEEPSEEK_REASONING_FIELD))
        .and_then(Value::as_str)?;
    (!reasoning.is_empty()).then(|| reasoning_continuation_event(reasoning))
}

pub(super) fn apply_chat_message_continuation(
    encoded: &mut Value,
    source: &Message,
    dialect: OpenAiChatDialect,
    continuations: &[ProviderContinuationRecord],
) -> Result<(), ModelError> {
    if source.role != MessageRole::Assistant {
        return Ok(());
    }
    let has_tool_calls = encoded
        .get("tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|tool_calls| !tool_calls.is_empty());
    if !has_tool_calls {
        return Ok(());
    }

    if dialect == OpenAiChatDialect::MiniMax {
        let Some(record) = continuations.iter().find(|record| {
            record.message_id == source.id
                && record.kind == ProviderContinuationKind::ReasoningReplay
        }) else {
            return Ok(());
        };
        apply_minimax_payload(encoded, &record.payload)?;
        return Ok(());
    }

    if dialect != OpenAiChatDialect::DeepSeek {
        return Ok(());
    }

    let matching_message = continuations
        .iter()
        .filter(|record| record.message_id == source.id)
        .collect::<Vec<_>>();
    if matching_message.is_empty() {
        return Err(missing_provider_continuation());
    }
    let record = matching_message
        .iter()
        .find(|record| record.kind == ProviderContinuationKind::ReasoningReplay)
        .ok_or_else(invalid_provider_continuation)?;
    let reasoning = reasoning_content_from_payload(&record.payload)?;
    encoded[DEEPSEEK_REASONING_FIELD] = json!(reasoning);
    Ok(())
}

fn minimax_chat_response_continuation_event(response: &Value) -> Option<ModelStreamEvent> {
    let message = response
        .get("choices")
        .and_then(Value::as_array)?
        .first()?
        .get("message")?;
    let payload = minimax_message_payload(message)?;
    Some(ModelStreamEvent::ProviderContinuationDelta {
        kind: ProviderContinuationKind::ReasoningReplay,
        payload,
    })
}

fn minimax_stream_reasoning_payload(chunk: &Value) -> Option<Value> {
    let payloads = chunk
        .get("choices")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|choice| choice.get("delta"))
        .filter_map(minimax_message_payload)
        .collect::<Vec<_>>();
    (!payloads.is_empty()).then(|| minimax_payload_from_chunks(&payloads))
}

fn minimax_message_payload(message: &Value) -> Option<Value> {
    let reasoning_content = message
        .get(MINIMAX_REASONING_CONTENT_FIELD)
        .filter(|value| !value.is_null());
    let reasoning_details = message
        .get(MINIMAX_REASONING_DETAILS_FIELD)
        .filter(|value| !value.is_null());
    if reasoning_content.is_none() && reasoning_details.is_none() {
        return None;
    }

    let mut payload = json!({ "format": MINIMAX_REASONING_PAYLOAD_FORMAT });
    if let Some(value) = reasoning_content {
        payload["reasoningContent"] = value.clone();
    }
    if let Some(value) = reasoning_details {
        payload["reasoningDetails"] = value.clone();
    }
    Some(payload)
}

fn minimax_payload_from_chunks(payloads: &[Value]) -> Value {
    let mut reasoning_content = String::new();
    let mut reasoning_details = Vec::new();
    for payload in payloads {
        if let Some(content) = payload.get("reasoningContent").and_then(Value::as_str) {
            reasoning_content.push_str(content);
        }
        if let Some(details) = payload.get("reasoningDetails") {
            match details {
                Value::Array(items) => reasoning_details.extend(items.iter().cloned()),
                value => reasoning_details.push(value.clone()),
            }
        }
    }

    let mut payload = json!({ "format": MINIMAX_REASONING_PAYLOAD_FORMAT });
    if !reasoning_content.is_empty() {
        payload["reasoningContent"] = json!(reasoning_content);
    }
    if !reasoning_details.is_empty() {
        payload["reasoningDetails"] = Value::Array(reasoning_details);
    }
    payload
}

fn apply_minimax_payload(encoded: &mut Value, payload: &Value) -> Result<(), ModelError> {
    let format_matches = payload
        .get("format")
        .and_then(Value::as_str)
        .is_some_and(|format| format == MINIMAX_REASONING_PAYLOAD_FORMAT);
    if !format_matches {
        return Ok(());
    }
    if let Some(value) = payload.get("reasoningContent") {
        encoded[MINIMAX_REASONING_CONTENT_FIELD] = value.clone();
    }
    if let Some(value) = payload.get("reasoningDetails") {
        encoded[MINIMAX_REASONING_DETAILS_FIELD] = value.clone();
    }
    Ok(())
}

fn reasoning_continuation_event(reasoning: &str) -> ModelStreamEvent {
    ModelStreamEvent::ProviderContinuationDelta {
        kind: ProviderContinuationKind::ReasoningReplay,
        payload: json!({
            "format": DEEPSEEK_REASONING_PAYLOAD_FORMAT,
            "reasoningContent": reasoning,
        }),
    }
}

fn reasoning_content_from_payload(payload: &Value) -> Result<&str, ModelError> {
    let format_matches = payload
        .get("format")
        .and_then(Value::as_str)
        .is_some_and(|format| format == DEEPSEEK_REASONING_PAYLOAD_FORMAT);
    if !format_matches {
        return Err(invalid_provider_continuation());
    }
    payload
        .get("reasoningContent")
        .and_then(Value::as_str)
        .filter(|reasoning| !reasoning.is_empty())
        .ok_or_else(invalid_provider_continuation)
}

fn missing_provider_continuation() -> ModelError {
    ModelError::InvalidRequest("missing provider continuation for assistant tool replay".to_owned())
}

fn invalid_provider_continuation() -> ModelError {
    ModelError::InvalidRequest("invalid provider continuation for assistant tool replay".to_owned())
}

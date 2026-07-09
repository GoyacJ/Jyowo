use harness_contracts::{Message, MessageRole, ModelError};
use harness_provider_state::{ProviderContinuationKind, ProviderContinuationRecord};
use serde_json::{json, Value};

use crate::ModelStreamEvent;

use super::dialect::OpenAiChatDialect;

const REASONING_FIELD: &str = "reasoning_content";
const DEEPSEEK_REASONING_PAYLOAD_FORMAT: &str = "deepseek.reasoning_content.v1";
const KIMI_REASONING_PAYLOAD_FORMAT: &str = "kimi.reasoning_content.v1";

pub(super) fn stream_reasoning_delta(dialect: OpenAiChatDialect, chunk: &Value) -> Option<String> {
    reasoning_payload_format(dialect)?;
    let reasoning = chunk
        .get("choices")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|choice| {
            choice
                .get("delta")
                .and_then(|delta| delta.get(REASONING_FIELD))
                .and_then(Value::as_str)
        })
        .filter(|value| !value.is_empty())
        .collect::<String>();
    (!reasoning.is_empty()).then_some(reasoning)
}

pub(super) fn stream_continuation_event(
    dialect: OpenAiChatDialect,
    reasoning: &str,
) -> Option<ModelStreamEvent> {
    let format = reasoning_payload_format(dialect)?;
    (!reasoning.is_empty()).then(|| reasoning_continuation_event(format, reasoning))
}

pub(super) fn chat_response_continuation_event(
    dialect: OpenAiChatDialect,
    response: &Value,
) -> Option<ModelStreamEvent> {
    let format = reasoning_payload_format(dialect)?;
    let reasoning = response
        .get("choices")
        .and_then(Value::as_array)?
        .first()
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get(REASONING_FIELD))
        .and_then(Value::as_str)?;
    (!reasoning.is_empty()).then(|| reasoning_continuation_event(format, reasoning))
}

pub(super) fn apply_chat_message_continuation(
    encoded: &mut Value,
    source: &Message,
    dialect: OpenAiChatDialect,
    continuations: &[ProviderContinuationRecord],
    required: bool,
) -> Result<bool, ModelError> {
    if !matches!(
        dialect,
        OpenAiChatDialect::DeepSeek | OpenAiChatDialect::Kimi
    ) || source.role != MessageRole::Assistant
    {
        return Ok(false);
    }
    let has_tool_calls = encoded
        .get("tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|tool_calls| !tool_calls.is_empty());
    if !has_tool_calls {
        return Ok(false);
    }
    if !required {
        return Ok(false);
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
    let reasoning = reasoning_content_from_payload(dialect, &record.payload)?;
    encoded[REASONING_FIELD] = json!(reasoning);
    Ok(true)
}

fn reasoning_continuation_event(format: &'static str, reasoning: &str) -> ModelStreamEvent {
    ModelStreamEvent::ProviderContinuationDelta {
        kind: ProviderContinuationKind::ReasoningReplay,
        payload: json!({
            "format": format,
            "reasoningContent": reasoning,
        }),
    }
}

fn reasoning_content_from_payload<'a>(
    dialect: OpenAiChatDialect,
    payload: &'a Value,
) -> Result<&'a str, ModelError> {
    let expected_format =
        reasoning_payload_format(dialect).ok_or_else(invalid_provider_continuation)?;
    let format_matches = payload
        .get("format")
        .and_then(Value::as_str)
        .is_some_and(|format| format == expected_format);
    if !format_matches {
        return Err(invalid_provider_continuation());
    }
    payload
        .get("reasoningContent")
        .and_then(Value::as_str)
        .filter(|reasoning| !reasoning.is_empty())
        .ok_or_else(invalid_provider_continuation)
}

fn reasoning_payload_format(dialect: OpenAiChatDialect) -> Option<&'static str> {
    match dialect {
        OpenAiChatDialect::DeepSeek => Some(DEEPSEEK_REASONING_PAYLOAD_FORMAT),
        OpenAiChatDialect::Kimi => Some(KIMI_REASONING_PAYLOAD_FORMAT),
        _ => None,
    }
}

fn missing_provider_continuation() -> ModelError {
    ModelError::InvalidRequest("missing provider continuation for assistant tool replay".to_owned())
}

fn invalid_provider_continuation() -> ModelError {
    ModelError::InvalidRequest("invalid provider continuation for assistant tool replay".to_owned())
}

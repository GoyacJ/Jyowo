use harness_contracts::{Message, MessageRole, ModelError};
use harness_provider_state::{ProviderContinuationKind, ProviderContinuationRecord};
use serde_json::{json, Value};

use crate::ModelStreamEvent;

use super::dialect::OpenAiChatDialect;

const REASONING_CONTENT_FIELD: &str = "reasoning_content";
const DEEPSEEK_REASONING_PAYLOAD_FORMAT: &str = "deepseek.reasoning_content.v1";
const ZHIPU_REASONING_PAYLOAD_FORMAT: &str = "zhipu.reasoning_content.v1";

#[derive(Clone, Copy)]
struct ReasoningReplayConfig {
    payload_format: &'static str,
}

pub(super) fn stream_reasoning_delta(dialect: OpenAiChatDialect, chunk: &Value) -> Option<String> {
    reasoning_replay_config(dialect)?;
    let reasoning = chunk
        .get("choices")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|choice| {
            choice
                .get("delta")
                .and_then(|delta| delta.get(REASONING_CONTENT_FIELD))
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
    let config = reasoning_replay_config(dialect)?;
    (!reasoning.is_empty()).then(|| reasoning_continuation_event(config, reasoning))
}

pub(super) fn chat_response_continuation_event(
    dialect: OpenAiChatDialect,
    response: &Value,
) -> Option<ModelStreamEvent> {
    let config = reasoning_replay_config(dialect)?;
    let reasoning = response
        .get("choices")
        .and_then(Value::as_array)?
        .first()
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get(REASONING_CONTENT_FIELD))
        .and_then(Value::as_str)?;
    (!reasoning.is_empty()).then(|| reasoning_continuation_event(config, reasoning))
}

pub(super) fn apply_chat_message_continuation(
    encoded: &mut Value,
    source: &Message,
    dialect: OpenAiChatDialect,
    continuations: &[ProviderContinuationRecord],
) -> Result<(), ModelError> {
    let Some(config) = reasoning_replay_config(dialect) else {
        return Ok(());
    };
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
    let reasoning = reasoning_content_from_payload(&record.payload, config)?;
    encoded[REASONING_CONTENT_FIELD] = json!(reasoning);
    Ok(())
}

fn reasoning_replay_config(dialect: OpenAiChatDialect) -> Option<ReasoningReplayConfig> {
    match dialect {
        OpenAiChatDialect::DeepSeek => Some(ReasoningReplayConfig {
            payload_format: DEEPSEEK_REASONING_PAYLOAD_FORMAT,
        }),
        OpenAiChatDialect::Zhipu => Some(ReasoningReplayConfig {
            payload_format: ZHIPU_REASONING_PAYLOAD_FORMAT,
        }),
        _ => None,
    }
}

fn reasoning_continuation_event(
    config: ReasoningReplayConfig,
    reasoning: &str,
) -> ModelStreamEvent {
    ModelStreamEvent::ProviderContinuationDelta {
        kind: ProviderContinuationKind::ReasoningReplay,
        payload: json!({
            "format": config.payload_format,
            "reasoningContent": reasoning,
        }),
    }
}

fn reasoning_content_from_payload<'a>(
    payload: &'a Value,
    config: ReasoningReplayConfig,
) -> Result<&'a str, ModelError> {
    let format_matches = payload
        .get("format")
        .and_then(Value::as_str)
        .is_some_and(|format| format == config.payload_format);
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

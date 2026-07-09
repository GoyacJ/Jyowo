use harness_contracts::{Message, MessageId, MessageRole, ModelError};
use harness_provider_state::{ProviderContinuationKind, ProviderContinuationRecord};
use serde_json::{json, Value};

use crate::ModelStreamEvent;

use super::dialect::OpenAiChatDialect;

const REASONING_CONTENT_FIELD: &str = "reasoning_content";
const DEEPSEEK_REASONING_PAYLOAD_FORMAT: &str = "deepseek.reasoning_content.v1";
const ZHIPU_REASONING_PAYLOAD_FORMAT: &str = "zhipu.reasoning_content.v1";
const KIMI_REASONING_PAYLOAD_FORMAT: &str = "kimi.reasoning_content.v1";
const MINIMAX_REASONING_CONTENT_FIELD: &str = "reasoning_content";
const MINIMAX_REASONING_DETAILS_FIELD: &str = "reasoning_details";
const MINIMAX_REASONING_PAYLOAD_FORMAT: &str = "minimax.reasoning_details.v1";
pub(super) const OPENAI_RESPONSES_PREVIOUS_RESPONSE_KIND: &str =
    "openai.responses.previous_response";
const OPENAI_RESPONSES_PREVIOUS_RESPONSE_FORMAT: &str = "openai.responses.previous_response.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OpenAiResponsesContinuationCapture {
    pub model_id: String,
    pub setup_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OpenAiResponsesPreviousResponse {
    pub response_id: String,
    pub after_message_id: MessageId,
}

#[derive(Clone, Copy)]
struct ReasoningReplayConfig {
    payload_format: &'static str,
}

pub(super) fn stream_reasoning_continuation_payload(
    dialect: OpenAiChatDialect,
    chunk: &Value,
) -> Option<Value> {
    match dialect {
        OpenAiChatDialect::DeepSeek | OpenAiChatDialect::Zhipu | OpenAiChatDialect::Kimi => {
            let config = reasoning_replay_config(dialect)?;
            stream_reasoning_delta(dialect, chunk)
                .map(|reasoning| reasoning_payload(config, &reasoning))
        }
        OpenAiChatDialect::MiniMax => minimax_stream_reasoning_payload(chunk),
        _ => None,
    }
}

pub(super) fn stream_continuation_event(
    dialect: OpenAiChatDialect,
    payloads: &[Value],
) -> Option<ModelStreamEvent> {
    if payloads.is_empty() {
        return None;
    }

    match dialect {
        OpenAiChatDialect::DeepSeek | OpenAiChatDialect::Zhipu | OpenAiChatDialect::Kimi => {
            let config = reasoning_replay_config(dialect)?;
            let reasoning = payloads
                .iter()
                .filter(|payload| payload_format_matches(payload, config))
                .filter_map(|payload| payload.get("reasoningContent").and_then(Value::as_str))
                .collect::<String>();
            (!reasoning.is_empty()).then(|| reasoning_continuation_event(config, &reasoning))
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
        OpenAiChatDialect::DeepSeek | OpenAiChatDialect::Zhipu | OpenAiChatDialect::Kimi => {
            reasoning_chat_response_continuation_event(dialect, response)
        }
        OpenAiChatDialect::MiniMax => minimax_chat_response_continuation_event(response),
        _ => None,
    }
}

pub(super) fn openai_responses_previous_response_event(
    response_id: &str,
    capture: &OpenAiResponsesContinuationCapture,
) -> Option<ModelStreamEvent> {
    if response_id.is_empty() {
        return None;
    }
    Some(ModelStreamEvent::ProviderContinuationDelta {
        kind: ProviderContinuationKind::ProviderNative(
            OPENAI_RESPONSES_PREVIOUS_RESPONSE_KIND.to_owned(),
        ),
        payload: json!({
            "format": OPENAI_RESPONSES_PREVIOUS_RESPONSE_FORMAT,
            "responseId": response_id,
            "modelId": capture.model_id,
            "setupFingerprint": capture.setup_fingerprint,
        }),
    })
}

pub(super) fn find_openai_responses_previous_response(
    continuations: &[ProviderContinuationRecord],
    model_id: &str,
    setup_fingerprint: Option<&str>,
) -> Option<OpenAiResponsesPreviousResponse> {
    continuations
        .iter()
        .filter(|record| {
            record.kind
                == ProviderContinuationKind::ProviderNative(
                    OPENAI_RESPONSES_PREVIOUS_RESPONSE_KIND.to_owned(),
                )
        })
        .filter_map(|record| {
            let payload = &record.payload;
            let format_matches = payload
                .get("format")
                .and_then(Value::as_str)
                .is_some_and(|format| format == OPENAI_RESPONSES_PREVIOUS_RESPONSE_FORMAT);
            if !format_matches {
                return None;
            }
            let payload_model_id = payload.get("modelId").and_then(Value::as_str)?;
            if payload_model_id != model_id {
                return None;
            }
            let payload_setup = payload.get("setupFingerprint").and_then(Value::as_str);
            if payload_setup != setup_fingerprint {
                return None;
            }
            let response_id = payload.get("responseId").and_then(Value::as_str)?;
            if response_id.is_empty() {
                return None;
            }
            Some((
                record.created_at,
                OpenAiResponsesPreviousResponse {
                    response_id: response_id.to_owned(),
                    after_message_id: record.message_id,
                },
            ))
        })
        .max_by_key(|(created_at, _)| *created_at)
        .map(|(_, continuation)| continuation)
}

pub(super) fn apply_chat_message_continuation(
    encoded: &mut Value,
    source: &Message,
    dialect: OpenAiChatDialect,
    continuations: &[ProviderContinuationRecord],
    required: bool,
) -> Result<bool, ModelError> {
    if source.role != MessageRole::Assistant {
        return Ok(false);
    }
    let has_tool_calls = encoded
        .get("tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|tool_calls| !tool_calls.is_empty());
    if !has_tool_calls {
        return Ok(false);
    }

    if dialect == OpenAiChatDialect::MiniMax {
        let Some(record) = continuations.iter().find(|record| {
            record.message_id == source.id
                && record.kind == ProviderContinuationKind::ReasoningReplay
        }) else {
            return Ok(false);
        };
        apply_minimax_payload(encoded, &record.payload)?;
        return Ok(true);
    }

    let Some(config) = reasoning_replay_config(dialect) else {
        return Ok(false);
    };
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
    let reasoning = reasoning_content_from_payload(&record.payload, config)?;
    encoded[REASONING_CONTENT_FIELD] = json!(reasoning);
    Ok(true)
}

fn stream_reasoning_delta(dialect: OpenAiChatDialect, chunk: &Value) -> Option<String> {
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

fn reasoning_chat_response_continuation_event(
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

fn reasoning_replay_config(dialect: OpenAiChatDialect) -> Option<ReasoningReplayConfig> {
    match dialect {
        OpenAiChatDialect::DeepSeek => Some(ReasoningReplayConfig {
            payload_format: DEEPSEEK_REASONING_PAYLOAD_FORMAT,
        }),
        OpenAiChatDialect::Zhipu => Some(ReasoningReplayConfig {
            payload_format: ZHIPU_REASONING_PAYLOAD_FORMAT,
        }),
        OpenAiChatDialect::Kimi => Some(ReasoningReplayConfig {
            payload_format: KIMI_REASONING_PAYLOAD_FORMAT,
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
        payload: reasoning_payload(config, reasoning),
    }
}

fn reasoning_payload(config: ReasoningReplayConfig, reasoning: &str) -> Value {
    json!({
        "format": config.payload_format,
        "reasoningContent": reasoning,
    })
}

fn reasoning_content_from_payload<'a>(
    payload: &'a Value,
    config: ReasoningReplayConfig,
) -> Result<&'a str, ModelError> {
    if !payload_format_matches(payload, config) {
        return Err(invalid_provider_continuation());
    }
    payload
        .get("reasoningContent")
        .and_then(Value::as_str)
        .filter(|reasoning| !reasoning.is_empty())
        .ok_or_else(invalid_provider_continuation)
}

fn payload_format_matches(payload: &Value, config: ReasoningReplayConfig) -> bool {
    payload
        .get("format")
        .and_then(Value::as_str)
        .is_some_and(|format| format == config.payload_format)
}

fn missing_provider_continuation() -> ModelError {
    ModelError::InvalidRequest("missing provider continuation for assistant tool replay".to_owned())
}

fn invalid_provider_continuation() -> ModelError {
    ModelError::InvalidRequest("invalid provider continuation for assistant tool replay".to_owned())
}

use harness_contracts::{Message, MessageId, MessageRole, ModelError};
use harness_provider_state::{ProviderContinuationKind, ProviderContinuationRecord};
use serde_json::{json, Value};

use crate::ModelStreamEvent;

use super::dialect::OpenAiChatDialect;

const DEEPSEEK_REASONING_FIELD: &str = "reasoning_content";
const DEEPSEEK_REASONING_PAYLOAD_FORMAT: &str = "deepseek.reasoning_content.v1";
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

pub(super) fn deepseek_stream_continuation_event(
    dialect: OpenAiChatDialect,
    reasoning: &str,
) -> Option<ModelStreamEvent> {
    if dialect != OpenAiChatDialect::DeepSeek || reasoning.is_empty() {
        return None;
    }
    Some(reasoning_continuation_event(reasoning))
}

pub(super) fn deepseek_chat_response_continuation_event(
    dialect: OpenAiChatDialect,
    response: &Value,
) -> Option<ModelStreamEvent> {
    if dialect != OpenAiChatDialect::DeepSeek {
        return None;
    }
    let reasoning = response
        .get("choices")
        .and_then(Value::as_array)?
        .first()
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get(DEEPSEEK_REASONING_FIELD))
        .and_then(Value::as_str)?;
    (!reasoning.is_empty()).then(|| reasoning_continuation_event(reasoning))
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
) -> Result<(), ModelError> {
    if dialect != OpenAiChatDialect::DeepSeek || source.role != MessageRole::Assistant {
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
    let reasoning = reasoning_content_from_payload(&record.payload)?;
    encoded[DEEPSEEK_REASONING_FIELD] = json!(reasoning);
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

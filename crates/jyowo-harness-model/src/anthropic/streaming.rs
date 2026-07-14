use async_stream::stream;
use futures::StreamExt;
use harness_contracts::{ModelError, StopReason, UsageSnapshot};
use serde_json::Value;

use crate::sse::IncrementalSseDecoder;
use crate::{
    ContentDelta, ContentType, ErrorClass, ErrorHints, ModelStream, ModelStreamEvent, ThinkingDelta,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SseEvent {
    event: Option<String>,
    data: String,
}

#[derive(Debug, Default)]
pub(super) struct IncrementalSseParser {
    decoder: IncrementalSseDecoder,
}

impl IncrementalSseParser {
    pub(super) fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, ModelError> {
        Ok(self
            .decoder
            .push(chunk)?
            .into_iter()
            .filter_map(|frame| parse_frame(&frame))
            .collect())
    }

    pub(super) fn finish(&mut self) -> Result<Vec<SseEvent>, ModelError> {
        Ok(self
            .decoder
            .finish()?
            .into_iter()
            .filter_map(|frame| parse_frame(&frame))
            .collect())
    }
}

pub(super) fn response_to_stream(response: reqwest::Response) -> ModelStream {
    let mut bytes = response.bytes_stream();
    Box::pin(stream! {
        let mut parser = IncrementalSseParser::default();
        while let Some(chunk) = bytes.next().await {
            match chunk {
                Ok(chunk) => match parser.push(&chunk) {
                    Ok(events) => {
                        for event in events {
                            for mapped in map_sse_event(event) {
                                yield mapped;
                            }
                        }
                    }
                    Err(error) => {
                        yield stream_error(error, ErrorClass::Fatal, None);
                        return;
                    }
                },
                Err(error) => {
                    yield stream_error(
                        ModelError::ProviderUnavailable(error.to_string()),
                        ErrorClass::Transient,
                        None,
                    );
                    return;
                }
            }
        }

        match parser.finish() {
            Ok(events) => {
                for event in events {
                    for mapped in map_sse_event(event) {
                        yield mapped;
                    }
                }
            }
            Err(error) => yield stream_error(error, ErrorClass::Fatal, None),
        }
    })
}

fn parse_frame(frame: &str) -> Option<SseEvent> {
    let mut event = None;
    let mut data_lines = Vec::new();

    for raw_line in frame.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim().to_owned());
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_owned());
        }
    }

    if event.is_none() && data_lines.is_empty() {
        return None;
    }

    Some(SseEvent {
        event,
        data: data_lines.join("\n"),
    })
}

fn map_sse_event(event: SseEvent) -> Vec<ModelStreamEvent> {
    let event_name = event.event.as_deref().unwrap_or_default();
    if event_name == "ping" || event.data == "[DONE]" {
        return Vec::new();
    }

    let payload = match serde_json::from_str::<Value>(&event.data) {
        Ok(payload) => payload,
        Err(error) => {
            return vec![stream_error(
                ModelError::UnexpectedResponse(format!("invalid Anthropic SSE JSON: {error}")),
                ErrorClass::Fatal,
                Some(event_name),
            )];
        }
    };

    let event_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or(event_name);

    match event_type {
        "message_start" => vec![ModelStreamEvent::MessageStart {
            message_id: payload
                .pointer("/message/id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            usage: usage(payload.pointer("/message/usage")),
        }],
        "content_block_start" => content_block_start(&payload),
        "content_block_delta" => content_block_delta(&payload),
        "content_block_stop" => value_as_u32(payload.get("index"))
            .map(|index| vec![ModelStreamEvent::ContentBlockStop { index }])
            .unwrap_or_default(),
        "message_delta" => vec![ModelStreamEvent::MessageDelta {
            stop_reason: payload
                .pointer("/delta/stop_reason")
                .and_then(Value::as_str)
                .map(stop_reason),
            usage_delta: usage(payload.get("usage")),
        }],
        "message_stop" => vec![ModelStreamEvent::MessageStop],
        "error" => vec![anthropic_error(&payload)],
        _ => Vec::new(),
    }
}

fn content_block_start(payload: &Value) -> Vec<ModelStreamEvent> {
    let Some(index) = value_as_u32(payload.get("index")) else {
        return Vec::new();
    };
    let block = &payload["content_block"];
    match block.get("type").and_then(Value::as_str) {
        Some("text") => vec![ModelStreamEvent::ContentBlockStart {
            index,
            content_type: ContentType::Text,
        }],
        Some("tool_use") => vec![
            ModelStreamEvent::ContentBlockStart {
                index,
                content_type: ContentType::ToolUse,
            },
            ModelStreamEvent::ContentBlockDelta {
                index,
                delta: ContentDelta::ToolUseStart {
                    id: block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                },
            },
        ],
        Some("thinking" | "redacted_thinking") => vec![ModelStreamEvent::ContentBlockStart {
            index,
            content_type: ContentType::Thinking,
        }],
        _ => Vec::new(),
    }
}

fn content_block_delta(payload: &Value) -> Vec<ModelStreamEvent> {
    let Some(index) = value_as_u32(payload.get("index")) else {
        return Vec::new();
    };
    let delta = &payload["delta"];
    let Some(delta_type) = delta.get("type").and_then(Value::as_str) else {
        return Vec::new();
    };

    let mapped = match delta_type {
        "text_delta" => delta
            .get("text")
            .and_then(Value::as_str)
            .map(|text| ContentDelta::Text(text.to_owned())),
        "input_json_delta" => delta
            .get("partial_json")
            .and_then(Value::as_str)
            .map(|partial| ContentDelta::ToolUseInputJson(partial.to_owned())),
        "thinking_delta" => Some(ContentDelta::Thinking(ThinkingDelta {
            text: delta
                .get("thinking")
                .and_then(Value::as_str)
                .map(str::to_owned),
            provider_native: Some(delta.clone()),
            signature: None,
        })),
        "signature_delta" => Some(ContentDelta::Thinking(ThinkingDelta {
            text: None,
            provider_native: Some(delta.clone()),
            signature: delta
                .get("signature")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })),
        "redacted_thinking_delta" => Some(ContentDelta::Thinking(ThinkingDelta {
            text: None,
            provider_native: Some(delta.clone()),
            signature: None,
        })),
        _ => None,
    };

    mapped
        .map(|delta| vec![ModelStreamEvent::ContentBlockDelta { index, delta }])
        .unwrap_or_default()
}

pub(super) fn usage(value: Option<&Value>) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens: value
            .and_then(|usage| usage.get("input_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        output_tokens: value
            .and_then(|usage| usage.get("output_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        cache_read_tokens: value
            .and_then(|usage| usage.get("cache_read_input_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        cache_write_tokens: value
            .and_then(|usage| usage.get("cache_creation_input_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        cost_micros: 0,
        tool_calls: 0,
    }
}

pub(super) fn stop_reason(reason: &str) -> StopReason {
    match reason {
        "end_turn" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxIterations,
        "stop_sequence" | "pause_turn" | "refusal" | "model_context_window_exceeded" => {
            StopReason::Error(reason.to_owned())
        }
        _ => StopReason::Error(reason.to_owned()),
    }
}

fn anthropic_error(payload: &Value) -> ModelStreamEvent {
    let error = &payload["error"];
    let error_type = error
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown_error");
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or(error_type)
        .to_owned();

    match error_type {
        "overloaded_error" => stream_error(
            ModelError::ProviderUnavailable(message),
            ErrorClass::Transient,
            Some(error_type),
        ),
        "rate_limit_error" => stream_error(
            ModelError::RateLimited(message),
            ErrorClass::RateLimited { retry_after: None },
            Some(error_type),
        ),
        "authentication_error" | "permission_error" => stream_error(
            ModelError::AuthExpired(message),
            ErrorClass::AuthExpired,
            Some(error_type),
        ),
        "invalid_request_error" | "request_too_large" => stream_error(
            ModelError::InvalidRequest(message),
            ErrorClass::Fatal,
            Some(error_type),
        ),
        _ => stream_error(
            ModelError::UnexpectedResponse(message),
            ErrorClass::Fatal,
            Some(error_type),
        ),
    }
}

fn stream_error(
    error: ModelError,
    class: ErrorClass,
    provider_error_code: Option<&str>,
) -> ModelStreamEvent {
    ModelStreamEvent::StreamError {
        error,
        class,
        hints: ErrorHints {
            raw_headers: None,
            provider_error_code: provider_error_code.map(str::to_owned),
            request_id: None,
        },
    }
}

fn value_as_u32(value: Option<&Value>) -> Option<u32> {
    value.and_then(Value::as_u64).map(|value| value as u32)
}

#[cfg(test)]
mod tests {
    use super::{IncrementalSseParser, SseEvent};

    #[test]
    fn parses_split_crlf_comments_and_multiline_data() {
        let mut parser = IncrementalSseParser::default();
        assert!(parser
            .push(b": comment\r\nevent: content_block_delta\r\ndata: {\"a\":\"")
            .expect("partial frame should buffer")
            .is_empty());

        let events = parser
            .push(b"b\"}\r\ndata: {\"c\":1}\r\n\r\n")
            .expect("completed frame should parse");

        assert_eq!(
            events,
            vec![SseEvent {
                event: Some("content_block_delta".to_owned()),
                data: "{\"a\":\"b\"}\n{\"c\":1}".to_owned(),
            }]
        );
    }

    #[test]
    fn finish_flushes_partial_frame() {
        let mut parser = IncrementalSseParser::default();
        parser
            .push(b"event: message_stop\ndata: {\"type\":\"message_stop\"}")
            .expect("partial frame should buffer");

        assert_eq!(
            parser.finish().expect("partial frame should flush"),
            vec![SseEvent {
                event: Some("message_stop".to_owned()),
                data: "{\"type\":\"message_stop\"}".to_owned(),
            }]
        );
    }
}

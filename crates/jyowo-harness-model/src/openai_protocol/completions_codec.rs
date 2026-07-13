use async_stream::stream;
use futures::StreamExt;
use harness_contracts::{ModelError, StopReason, UsageSnapshot};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    ContentDelta, ContentType, ErrorClass, ErrorHints, ModelRequest, ModelStream, ModelStreamEvent,
};

use super::request::{merge_extra_object, DEFAULT_MAX_TOKENS};
use super::streaming::IncrementalSseParser;

const DEEPSEEK_FIM_MAX_TOKENS: u32 = 4096;

pub(super) fn completions_request_body(req: &ModelRequest) -> Result<Value, ModelError> {
    let prompt = req
        .extra
        .get("prompt")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ModelError::InvalidRequest(
                "Completions protocol requires extra.prompt for DeepSeek FIM".to_owned(),
            )
        })?;
    let max_tokens = req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
    if max_tokens > DEEPSEEK_FIM_MAX_TOKENS {
        return Err(ModelError::InvalidRequest(format!(
            "DeepSeek FIM max_tokens must be <= {DEEPSEEK_FIM_MAX_TOKENS}"
        )));
    }

    let mut body = json!({
        "model": req.model_id,
        "prompt": prompt,
        "max_tokens": max_tokens,
        "stream": req.stream,
    });
    let mut extra = req.extra.clone();
    if let Some(object) = extra.as_object_mut() {
        object.remove("prompt");
        object.remove("model");
        object.remove("max_tokens");
        object.remove("stream");
    }
    merge_extra_object(&mut body, &extra)?;
    Ok(body)
}

pub(super) fn response_to_stream(response: reqwest::Response) -> ModelStream {
    let mut bytes = response.bytes_stream();
    Box::pin(stream! {
        let mut parser = IncrementalSseParser::default();
        let mut state = CompletionsStreamState::default();
        while let Some(chunk) = bytes.next().await {
            match chunk {
                Ok(chunk) => match parser.push(&chunk) {
                    Ok(events) => {
                        for event in events {
                            for mapped in state.map_event(&event.data) {
                                yield mapped;
                            }
                            if state.stopped {
                                return;
                            }
                        }
                    }
                    Err(error) => yield stream_error(error, ErrorClass::Fatal),
                },
                Err(error) => {
                    yield stream_error(
                        ModelError::ProviderUnavailable(error.to_string()),
                        ErrorClass::Transient,
                    );
                    return;
                }
            }
        }

        for event in parser.finish() {
            for mapped in state.map_event(&event.data) {
                yield mapped;
            }
        }
    })
}

pub(super) fn json_response_to_stream(value: Value) -> Result<ModelStream, ModelError> {
    let response: CompletionResponse = serde_json::from_value(value).map_err(|error| {
        ModelError::UnexpectedResponse(format!("invalid Completions API JSON: {error}"))
    })?;
    let usage = usage(response.usage.as_ref());
    let choice = response.choices.into_iter().next().ok_or_else(|| {
        ModelError::UnexpectedResponse("Completions response had no choices".to_owned())
    })?;
    let mut events = vec![ModelStreamEvent::MessageStart {
        message_id: response.id,
        usage: UsageSnapshot::default(),
    }];
    if !choice.text.is_empty() {
        events.push(ModelStreamEvent::ContentBlockStart {
            index: 0,
            content_type: ContentType::Text,
        });
        events.push(ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text(choice.text),
        });
        events.push(ModelStreamEvent::ContentBlockStop { index: 0 });
    }
    events.extend(message_end(choice.finish_reason.as_deref(), usage));
    Ok(Box::pin(futures::stream::iter(events)))
}

#[derive(Default)]
struct CompletionsStreamState {
    message_id: String,
    started: bool,
    text_started: bool,
    stopped: bool,
}

impl CompletionsStreamState {
    fn map_event(&mut self, data: &str) -> Vec<ModelStreamEvent> {
        if data == "[DONE]" {
            return self.finish(None, UsageSnapshot::default());
        }
        let payload: CompletionResponse = match serde_json::from_str(data) {
            Ok(payload) => payload,
            Err(error) => {
                return vec![stream_error(
                    ModelError::UnexpectedResponse(format!(
                        "invalid Completions API SSE JSON: {error}"
                    )),
                    ErrorClass::Fatal,
                )];
            }
        };
        let usage = usage(payload.usage.as_ref());
        let mut events = Vec::new();
        if !self.started {
            self.started = true;
            self.message_id = payload.id;
            events.push(ModelStreamEvent::MessageStart {
                message_id: self.message_id.clone(),
                usage: UsageSnapshot::default(),
            });
        }
        for choice in payload.choices {
            if !choice.text.is_empty() {
                if !self.text_started {
                    self.text_started = true;
                    events.push(ModelStreamEvent::ContentBlockStart {
                        index: 0,
                        content_type: ContentType::Text,
                    });
                }
                events.push(ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text(choice.text),
                });
            }
            if choice.finish_reason.is_some() {
                events.extend(self.finish(choice.finish_reason.as_deref(), usage.clone()));
            }
        }
        events
    }

    fn finish(
        &mut self,
        finish_reason: Option<&str>,
        usage: UsageSnapshot,
    ) -> Vec<ModelStreamEvent> {
        if self.stopped || !self.started {
            return Vec::new();
        }
        self.stopped = true;
        let mut events = Vec::new();
        if self.text_started {
            events.push(ModelStreamEvent::ContentBlockStop { index: 0 });
        }
        events.extend(message_end(finish_reason, usage));
        events
    }
}

fn message_end(finish_reason: Option<&str>, usage: UsageSnapshot) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageDelta {
            stop_reason: finish_reason.map(stop_reason),
            usage_delta: usage,
        },
        ModelStreamEvent::MessageStop,
    ]
}

fn usage(value: Option<&CompletionUsage>) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens: value
            .and_then(|usage| usage.prompt_tokens)
            .unwrap_or_default(),
        output_tokens: value
            .and_then(|usage| usage.completion_tokens)
            .unwrap_or_default(),
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        cost_micros: 0,
        tool_calls: 0,
    }
}

fn stop_reason(reason: &str) -> StopReason {
    match reason {
        "stop" => StopReason::EndTurn,
        "length" => StopReason::MaxIterations,
        "content_filter" => StopReason::ContentFiltered,
        _ => StopReason::Error(reason.to_owned()),
    }
}

fn stream_error(error: ModelError, class: ErrorClass) -> ModelStreamEvent {
    ModelStreamEvent::StreamError {
        error,
        class,
        hints: ErrorHints {
            raw_headers: None,
            provider_error_code: None,
            request_id: None,
        },
    }
}

#[derive(Debug, Deserialize)]
struct CompletionResponse {
    id: String,
    #[serde(default)]
    choices: Vec<CompletionChoice>,
    usage: Option<CompletionUsage>,
}

#[derive(Debug, Deserialize)]
struct CompletionChoice {
    #[serde(default)]
    text: String,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CompletionUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

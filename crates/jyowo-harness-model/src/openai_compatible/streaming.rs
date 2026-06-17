use std::collections::BTreeMap;

use async_stream::stream;
use futures::StreamExt;
use harness_contracts::ModelError;
use serde::Deserialize;

use crate::{ContentDelta, ContentType, ErrorClass, ErrorHints, ModelStream, ModelStreamEvent};

use super::{stop_reason, usage, OpenAiUsage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SseEvent {
    data: String,
}

#[derive(Debug, Default)]
pub(super) struct IncrementalSseParser {
    buffer: String,
}

impl IncrementalSseParser {
    pub(super) fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, ModelError> {
        let decoded = std::str::from_utf8(chunk)
            .map_err(|_| ModelError::UnexpectedResponse("invalid UTF-8 in SSE stream".to_owned()))?
            .replace("\r\n", "\n");
        self.buffer.push_str(&decoded);
        Ok(self.drain_complete_frames())
    }

    pub(super) fn finish(&mut self) -> Vec<SseEvent> {
        let mut events = self.drain_complete_frames();
        if !self.buffer.trim().is_empty() {
            let frame = std::mem::take(&mut self.buffer);
            if let Some(event) = parse_frame(&frame) {
                events.push(event);
            }
        }
        events
    }

    fn drain_complete_frames(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        while let Some(end) = self.buffer.find("\n\n") {
            let frame = self.buffer[..end].to_owned();
            self.buffer.drain(..end + 2);
            if let Some(event) = parse_frame(&frame) {
                events.push(event);
            }
        }
        events
    }
}

pub(super) fn response_to_stream(response: reqwest::Response) -> ModelStream {
    let mut bytes = response.bytes_stream();
    Box::pin(stream! {
        let mut parser = IncrementalSseParser::default();
        let mut state = OpenAiStreamState::default();
        while let Some(chunk) = bytes.next().await {
            match chunk {
                Ok(chunk) => match parser.push(&chunk) {
                    Ok(events) => {
                        for event in events {
                            for mapped in state.map_event(event) {
                                yield mapped;
                            }
                        }
                    }
                    Err(error) => {
                        yield stream_error(error, ErrorClass::Fatal);
                    }
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
            for mapped in state.map_event(event) {
                yield mapped;
            }
        }

        if state.started && !state.stopped {
            yield ModelStreamEvent::MessageStop;
        }
    })
}

fn parse_frame(frame: &str) -> Option<SseEvent> {
    let mut data_lines = Vec::new();

    for raw_line in frame.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_owned());
        }
    }

    if data_lines.is_empty() {
        return None;
    }

    Some(SseEvent {
        data: data_lines.join("\n"),
    })
}

#[derive(Default)]
struct OpenAiStreamState {
    started: bool,
    stopped: bool,
    text_started: bool,
    text_stopped: bool,
    next_block_index: u32,
    tool_calls: BTreeMap<u32, ToolCallState>,
}

impl OpenAiStreamState {
    fn map_event(&mut self, event: SseEvent) -> Vec<ModelStreamEvent> {
        if event.data == "[DONE]" {
            return Vec::new();
        }

        let payload = match serde_json::from_str::<ChatCompletionChunk>(&event.data) {
            Ok(payload) => payload,
            Err(error) => {
                return vec![stream_error(
                    ModelError::UnexpectedResponse(format!(
                        "invalid OpenAI-compatible SSE JSON: {error}"
                    )),
                    ErrorClass::Fatal,
                )];
            }
        };

        let mut events = Vec::new();
        if !self.started {
            self.started = true;
            events.push(ModelStreamEvent::MessageStart {
                message_id: payload.id.clone().unwrap_or_default(),
                usage: usage(None),
            });
        }

        for choice in payload.choices {
            if let Some(content) = choice.delta.content {
                if !content.is_empty() {
                    if !self.text_started {
                        self.text_started = true;
                        self.next_block_index = self.next_block_index.max(1);
                        events.push(ModelStreamEvent::ContentBlockStart {
                            index: 0,
                            content_type: ContentType::Text,
                        });
                    }
                    events.push(ModelStreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: ContentDelta::Text(content),
                    });
                }
            }

            for tool_call in choice.delta.tool_calls {
                events.extend(self.map_tool_call(tool_call));
            }

            if let Some(reason) = choice.finish_reason {
                if self.text_started && !self.text_stopped {
                    self.text_stopped = true;
                    events.push(ModelStreamEvent::ContentBlockStop { index: 0 });
                }
                for state in self.tool_calls.values_mut() {
                    if state.started && !state.stopped {
                        state.stopped = true;
                        events.push(ModelStreamEvent::ContentBlockStop {
                            index: state.block_index,
                        });
                    }
                }
                events.push(ModelStreamEvent::MessageDelta {
                    stop_reason: Some(stop_reason(&reason)),
                    usage_delta: usage(payload.usage.as_ref()),
                });
                events.push(ModelStreamEvent::MessageStop);
                self.stopped = true;
            }
        }

        if let Some(usage_value) = payload.usage.as_ref() {
            if !self.stopped {
                events.push(ModelStreamEvent::MessageDelta {
                    stop_reason: None,
                    usage_delta: usage(Some(usage_value)),
                });
            }
        }

        events
    }

    fn map_tool_call(&mut self, delta: StreamToolCallDelta) -> Vec<ModelStreamEvent> {
        let index = delta.index.unwrap_or_default();
        let state = self.tool_calls.entry(index).or_insert_with(|| {
            let block_index = self.next_block_index.max(1);
            self.next_block_index = block_index + 1;
            ToolCallState {
                block_index,
                ..ToolCallState::default()
            }
        });

        if let Some(id) = delta.id {
            state.id = Some(id);
        }
        if let Some(function) = delta.function {
            if let Some(name) = function.name {
                state.name = Some(name);
            }
            if let Some(arguments) = function.arguments {
                state.pending_arguments.push(arguments);
            }
        }

        let mut events = Vec::new();
        if !state.started {
            if let (Some(id), Some(name)) = (&state.id, &state.name) {
                state.started = true;
                events.push(ModelStreamEvent::ContentBlockStart {
                    index: state.block_index,
                    content_type: ContentType::ToolUse,
                });
                events.push(ModelStreamEvent::ContentBlockDelta {
                    index: state.block_index,
                    delta: ContentDelta::ToolUseStart {
                        id: id.clone(),
                        name: name.clone(),
                    },
                });
            }
        }

        if state.started {
            for arguments in state.pending_arguments.drain(..) {
                if !arguments.is_empty() {
                    events.push(ModelStreamEvent::ContentBlockDelta {
                        index: state.block_index,
                        delta: ContentDelta::ToolUseInputJson(arguments),
                    });
                }
            }
        }

        events
    }
}

#[derive(Default)]
struct ToolCallState {
    block_index: u32,
    id: Option<String>,
    name: Option<String>,
    started: bool,
    stopped: bool,
    pending_arguments: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunk {
    id: Option<String>,
    #[serde(default)]
    choices: Vec<StreamChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: StreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct StreamDelta {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<StreamToolCallDelta>,
}

#[derive(Debug, Deserialize)]
struct StreamToolCallDelta {
    index: Option<u32>,
    id: Option<String>,
    function: Option<StreamFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct StreamFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::{IncrementalSseParser, OpenAiStreamState, SseEvent};
    use crate::{ContentDelta, ModelStreamEvent};

    #[test]
    fn parses_split_crlf_comments_multiline_data_and_done() {
        let mut parser = IncrementalSseParser::default();
        assert!(parser
            .push(b": comment\r\ndata: {\"a\":\"")
            .expect("partial frame should buffer")
            .is_empty());

        let events = parser
            .push(b"b\"}\r\ndata: {\"c\":1}\r\n\r\n")
            .expect("completed frame should parse");

        assert_eq!(
            events,
            vec![SseEvent {
                data: "{\"a\":\"b\"}\n{\"c\":1}".to_owned(),
            }]
        );

        assert_eq!(
            parser
                .push(b"data: [DONE]\n\n")
                .expect("done frame should parse"),
            vec![SseEvent {
                data: "[DONE]".to_owned(),
            }]
        );
    }

    #[test]
    fn tool_arguments_wait_until_id_and_name_are_known() {
        let mut state = OpenAiStreamState::default();

        let first = state.map_event(SseEvent {
            data: "{\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"q\\\":\"}}]},\"finish_reason\":null}]}".to_owned(),
        });
        assert!(!first.iter().any(|event| matches!(
            event,
            ModelStreamEvent::ContentBlockDelta {
                delta: ContentDelta::ToolUseInputJson(_),
                ..
            }
        )));

        let second = state.map_event(SseEvent {
            data: "{\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"search\",\"arguments\":\"\\\"docs\\\"}\"}}]},\"finish_reason\":null}]}".to_owned(),
        });

        assert!(second.contains(&ModelStreamEvent::ContentBlockDelta {
            index: 1,
            delta: ContentDelta::ToolUseStart {
                id: "call_1".to_owned(),
                name: "search".to_owned(),
            },
        }));
        assert!(second.contains(&ModelStreamEvent::ContentBlockDelta {
            index: 1,
            delta: ContentDelta::ToolUseInputJson("{\"q\":".to_owned()),
        }));
        assert!(second.contains(&ModelStreamEvent::ContentBlockDelta {
            index: 1,
            delta: ContentDelta::ToolUseInputJson("\"docs\"}".to_owned()),
        }));
    }
}

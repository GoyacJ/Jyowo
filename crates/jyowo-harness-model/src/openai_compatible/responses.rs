use async_stream::stream;
use futures::StreamExt;
use harness_contracts::{ModelError, StopReason, UsageSnapshot};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    ContentDelta, ContentType, ErrorClass, ErrorHints, ModelStream, ModelStreamEvent,
    ReasoningSummaryDelta, ThinkingDelta,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SseEvent {
    event: Option<String>,
    data: String,
}

#[derive(Debug, Default)]
struct IncrementalSseParser {
    buffer: String,
}

impl IncrementalSseParser {
    fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, ModelError> {
        let decoded = std::str::from_utf8(chunk)
            .map_err(|_| ModelError::UnexpectedResponse("invalid UTF-8 in SSE stream".to_owned()))?
            .replace("\r\n", "\n");
        self.buffer.push_str(&decoded);
        Ok(self.drain_complete_frames())
    }

    fn finish(&mut self) -> Vec<SseEvent> {
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
        let mut state = ResponsesStreamState::default();
        while let Some(chunk) = bytes.next().await {
            match chunk {
                Ok(chunk) => match parser.push(&chunk) {
                    Ok(events) => {
                        for event in events {
                            for mapped in state.map_event(event) {
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
            for mapped in state.map_event(event) {
                yield mapped;
            }
        }

        if state.started && !state.stopped {
            yield ModelStreamEvent::MessageStop;
        }
    })
}

pub(super) fn json_response_to_stream(value: Value) -> Result<ModelStream, ModelError> {
    let response: ResponsesJson = serde_json::from_value(value).map_err(|error| {
        ModelError::UnexpectedResponse(format!("invalid Responses API JSON: {error}"))
    })?;
    let usage = usage(response.usage.as_ref());
    let mut events = vec![ModelStreamEvent::MessageStart {
        message_id: response.id,
        usage: usage.clone(),
    }];
    let mut next_index = 0;

    for item in response.output {
        if item.kind == "message" {
            for content in item.content {
                if content.kind == "output_text" {
                    let index = next_index;
                    next_index += 1;
                    events.push(ModelStreamEvent::ContentBlockStart {
                        index,
                        content_type: ContentType::Text,
                    });
                    events.push(ModelStreamEvent::ContentBlockDelta {
                        index,
                        delta: ContentDelta::Text(content.text.unwrap_or_default()),
                    });
                    events.push(ModelStreamEvent::ContentBlockStop { index });
                }
            }
        } else if item.kind == "function_call" {
            let index = next_index;
            next_index += 1;
            events.push(ModelStreamEvent::ContentBlockStart {
                index,
                content_type: ContentType::ToolUse,
            });
            events.push(ModelStreamEvent::ContentBlockDelta {
                index,
                delta: ContentDelta::ToolUseStart {
                    id: item.call_id.unwrap_or_else(|| item.id.clone()),
                    name: item.name.unwrap_or_default(),
                },
            });
            if let Some(arguments) = item.arguments {
                events.push(ModelStreamEvent::ContentBlockDelta {
                    index,
                    delta: ContentDelta::ToolUseInputJson(arguments),
                });
            }
            events.push(ModelStreamEvent::ContentBlockStop { index });
        } else if item.kind == "reasoning" {
            let index = next_index;
            next_index += 1;
            events.push(ModelStreamEvent::ContentBlockStart {
                index,
                content_type: ContentType::Thinking,
            });
            events.push(ModelStreamEvent::ContentBlockDelta {
                index,
                delta: ContentDelta::ReasoningSummary(ReasoningSummaryDelta {
                    text: item.summary.unwrap_or_default(),
                    provider_native: Some(item.raw),
                }),
            });
            events.push(ModelStreamEvent::ContentBlockStop { index });
        }
    }

    events.push(ModelStreamEvent::MessageDelta {
        stop_reason: Some(StopReason::EndTurn),
        usage_delta: usage,
    });
    events.push(ModelStreamEvent::MessageStop);
    Ok(Box::pin(futures::stream::iter(events)))
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
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_owned());
        }
    }

    if data_lines.is_empty() {
        return None;
    }

    Some(SseEvent {
        event,
        data: data_lines.join("\n"),
    })
}

#[derive(Default)]
struct ResponsesStreamState {
    started: bool,
    stopped: bool,
    text_index: Option<u32>,
    thinking_index: Option<u32>,
    tool_index: Option<u32>,
    next_index: u32,
}

impl ResponsesStreamState {
    fn map_event(&mut self, event: SseEvent) -> Vec<ModelStreamEvent> {
        let event_name = event.event.as_deref().unwrap_or_default();
        let data = match serde_json::from_str::<Value>(&event.data) {
            Ok(data) => data,
            Err(error) => {
                return vec![stream_error(
                    ModelError::UnexpectedResponse(format!(
                        "invalid Responses API SSE JSON: {error}"
                    )),
                    ErrorClass::Fatal,
                )];
            }
        };

        let mut events = Vec::new();
        if !self.started {
            self.started = true;
            events.push(ModelStreamEvent::MessageStart {
                message_id: data
                    .get("response")
                    .and_then(|response| response.get("id"))
                    .or_else(|| data.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                usage: UsageSnapshot::default(),
            });
        }

        match event_name {
            "response.output_text.delta" => {
                let delta = data
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !delta.is_empty() {
                    let index = self.ensure_text_block(&mut events);
                    events.push(ModelStreamEvent::ContentBlockDelta {
                        index,
                        delta: ContentDelta::Text(delta.to_owned()),
                    });
                }
            }
            "response.reasoning_text.delta" => {
                let delta = data
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !delta.is_empty() {
                    let index = self.ensure_thinking_block(&mut events);
                    events.push(ModelStreamEvent::ContentBlockDelta {
                        index,
                        delta: ContentDelta::Thinking(ThinkingDelta {
                            text: Some(delta.to_owned()),
                            provider_native: Some(data),
                            signature: None,
                        }),
                    });
                }
            }
            "response.reasoning_summary_text.delta" => {
                let delta = data
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !delta.is_empty() {
                    let index = self.ensure_thinking_block(&mut events);
                    events.push(ModelStreamEvent::ContentBlockDelta {
                        index,
                        delta: ContentDelta::ReasoningSummary(ReasoningSummaryDelta {
                            text: delta.to_owned(),
                            provider_native: Some(data),
                        }),
                    });
                }
            }
            "response.output_item.added" => {
                let item = data.get("item").unwrap_or(&data);
                if item.get("type").and_then(Value::as_str) == Some("function_call") {
                    let index = self.ensure_tool_block(&mut events);
                    events.push(ModelStreamEvent::ContentBlockDelta {
                        index,
                        delta: ContentDelta::ToolUseStart {
                            id: item
                                .get("call_id")
                                .or_else(|| item.get("id"))
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            name: item
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                        },
                    });
                }
            }
            "response.function_call_arguments.delta" => {
                let delta = data
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !delta.is_empty() {
                    let index = self.ensure_tool_block(&mut events);
                    events.push(ModelStreamEvent::ContentBlockDelta {
                        index,
                        delta: ContentDelta::ToolUseInputJson(delta.to_owned()),
                    });
                }
            }
            "response.completed" => {
                self.close_blocks(&mut events);
                events.push(ModelStreamEvent::MessageDelta {
                    stop_reason: Some(StopReason::EndTurn),
                    usage_delta: usage(
                        data.get("response")
                            .and_then(|response| response.get("usage")),
                    ),
                });
                events.push(ModelStreamEvent::MessageStop);
                self.stopped = true;
            }
            "response.failed" => {
                events.push(stream_error(
                    ModelError::ProviderUnavailable(
                        data.get("error")
                            .and_then(|error| error.get("message"))
                            .and_then(Value::as_str)
                            .unwrap_or("Responses API stream failed")
                            .to_owned(),
                    ),
                    ErrorClass::Fatal,
                ));
                self.stopped = true;
            }
            _ => {}
        }

        events
    }

    fn ensure_text_block(&mut self, events: &mut Vec<ModelStreamEvent>) -> u32 {
        self.ensure_block(events, ContentType::Text, BlockKind::Text)
    }

    fn ensure_thinking_block(&mut self, events: &mut Vec<ModelStreamEvent>) -> u32 {
        self.ensure_block(events, ContentType::Thinking, BlockKind::Thinking)
    }

    fn ensure_tool_block(&mut self, events: &mut Vec<ModelStreamEvent>) -> u32 {
        self.ensure_block(events, ContentType::ToolUse, BlockKind::Tool)
    }

    fn ensure_block(
        &mut self,
        events: &mut Vec<ModelStreamEvent>,
        content_type: ContentType,
        kind: BlockKind,
    ) -> u32 {
        let slot = match kind {
            BlockKind::Text => &mut self.text_index,
            BlockKind::Thinking => &mut self.thinking_index,
            BlockKind::Tool => &mut self.tool_index,
        };
        if let Some(index) = *slot {
            return index;
        }
        let index = self.next_index;
        self.next_index += 1;
        *slot = Some(index);
        events.push(ModelStreamEvent::ContentBlockStart {
            index,
            content_type,
        });
        index
    }

    fn close_blocks(&mut self, events: &mut Vec<ModelStreamEvent>) {
        for index in [self.text_index, self.thinking_index, self.tool_index]
            .into_iter()
            .flatten()
        {
            events.push(ModelStreamEvent::ContentBlockStop { index });
        }
    }
}

enum BlockKind {
    Text,
    Thinking,
    Tool,
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

fn usage(value: Option<&Value>) -> UsageSnapshot {
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
            .and_then(|usage| usage.get("input_tokens_details"))
            .and_then(|details| details.get("cached_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        cache_write_tokens: 0,
        cost_micros: 0,
        tool_calls: 0,
    }
}

#[derive(Debug, Deserialize)]
struct ResponsesJson {
    id: String,
    #[serde(default)]
    output: Vec<ResponseOutputItem>,
    usage: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ResponseOutputItem {
    #[serde(rename = "type")]
    kind: String,
    id: String,
    call_id: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
    summary: Option<String>,
    #[serde(default)]
    content: Vec<ResponseContent>,
    #[serde(flatten)]
    raw: Value,
}

#[derive(Debug, Deserialize)]
struct ResponseContent {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::future;
    use std::time::Duration;

    use futures::StreamExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::{response_to_stream, ResponsesStreamState, SseEvent};
    use crate::ModelStreamEvent;

    #[test]
    fn completed_marks_stream_stopped_after_message_stop() {
        let mut state = ResponsesStreamState::default();

        let events = state.map_event(SseEvent {
            event: Some("response.completed".to_owned()),
            data: "{\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}".to_owned(),
        });

        assert!(events.contains(&ModelStreamEvent::MessageStop));
        assert!(state.stopped);
    }

    #[tokio::test]
    async fn completed_terminates_response_stream_without_waiting_for_eof() {
        let response = pending_sse_response(
            "event: response.completed\ndata: {\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n",
        )
        .await;
        let mut stream = response_to_stream(response);

        let mut events = Vec::new();
        while let Some(event) = tokio::time::timeout(Duration::from_millis(200), stream.next())
            .await
            .expect("stream should terminate after response.completed without EOF")
        {
            events.push(event);
        }

        assert!(events.contains(&ModelStreamEvent::MessageStop));
    }

    async fn pending_sse_response(frame: &'static str) -> reqwest::Response {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await.unwrap();
            let header =
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: keep-alive\r\n\r\n";
            socket.write_all(header.as_bytes()).await.unwrap();
            let chunk = format!("{:x}\r\n{}\r\n", frame.len(), frame);
            socket.write_all(chunk.as_bytes()).await.unwrap();
            socket.flush().await.unwrap();
            future::pending::<()>().await;
        });

        reqwest::get(url).await.unwrap()
    }
}

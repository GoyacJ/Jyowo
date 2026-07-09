use std::collections::BTreeMap;
use std::time::Duration;

use async_stream::stream;
use futures::StreamExt;
use harness_contracts::ModelError;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;

use crate::{
    ContentDelta, ContentType, ErrorClass, ErrorHints, ModelStream, ModelStreamEvent, ThinkingDelta,
};

use super::chat_codec::{stop_reason, usage_for_dialect, OpenAiUsage};
use super::continuation;
use super::dialect::OpenAiChatDialect;

const POST_FINISH_USAGE_GRACE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SseEvent {
    pub(super) data: String,
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

pub(super) fn response_to_stream(
    response: reqwest::Response,
    dialect: OpenAiChatDialect,
) -> ModelStream {
    let mut bytes = response.bytes_stream();
    Box::pin(stream! {
        let mut parser = IncrementalSseParser::default();
        let mut state = OpenAiStreamState::new(dialect);
        let mut terminal_pending_deadline = None;
        loop {
            let chunk = if state.terminal_pending {
                let deadline = *terminal_pending_deadline
                    .get_or_insert_with(|| tokio::time::Instant::now() + POST_FINISH_USAGE_GRACE);
                match tokio::time::timeout_at(deadline, bytes.next()).await {
                    Ok(chunk) => chunk,
                    Err(_) => {
                        for event in state.finish_message() {
                            yield event;
                        }
                        return;
                    }
                }
            } else {
                terminal_pending_deadline = None;
                bytes.next().await
            };

            let Some(chunk) = chunk else {
                break;
            };

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
            for event in state.finish_message() {
                yield event;
            }
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
    dialect: OpenAiChatDialect,
    started: bool,
    stopped: bool,
    terminal_pending: bool,
    text_started: bool,
    text_stopped: bool,
    text_block_index: Option<u32>,
    thinking_started: bool,
    thinking_stopped: bool,
    thinking_block_index: Option<u32>,
    next_block_index: u32,
    reasoning_replay_payloads: Vec<Value>,
    continuation_emitted: bool,
    tool_calls: BTreeMap<u32, ToolCallState>,
}

impl OpenAiStreamState {
    fn new(dialect: OpenAiChatDialect) -> Self {
        Self {
            dialect,
            ..Self::default()
        }
    }

    fn map_event(&mut self, event: SseEvent) -> Vec<ModelStreamEvent> {
        if event.data == "[DONE]" {
            self.terminal_pending = false;
            if !self.started {
                self.stopped = true;
                return Vec::new();
            }
            return self.finish_message();
        }

        let payload_value = match parse_sse_json::<Value>(&event.data) {
            Ok(payload) => payload,
            Err(error) => return vec![error],
        };
        if let Some(payload) =
            continuation::stream_reasoning_continuation_payload(self.dialect, &payload_value)
        {
            self.reasoning_replay_payloads.push(payload);
        }
        let payload = match serde_json::from_value::<ChatCompletionChunk>(payload_value) {
            Ok(payload) => payload,
            Err(error) => return vec![invalid_sse_json(error)],
        };

        let mut events = Vec::new();
        let mut finish_reason_seen = false;
        if !self.started {
            self.started = true;
            events.push(ModelStreamEvent::MessageStart {
                message_id: payload.id.clone().unwrap_or_default(),
                usage: usage_for_dialect(None, self.dialect),
            });
        }

        for choice in payload.choices {
            if self.dialect == OpenAiChatDialect::Qwen {
                if let Some(reasoning_content) = choice.delta.reasoning_content {
                    if !reasoning_content.is_empty() {
                        let index = self.ensure_thinking_block(&mut events);
                        events.push(ModelStreamEvent::ContentBlockDelta {
                            index,
                            delta: ContentDelta::Thinking(ThinkingDelta {
                                text: Some(reasoning_content),
                                provider_native: None,
                                signature: None,
                            }),
                        });
                    }
                }
            }

            if let Some(content) = choice.delta.content {
                if !content.is_empty() {
                    let index = self.ensure_text_block(&mut events);
                    events.push(ModelStreamEvent::ContentBlockDelta {
                        index,
                        delta: ContentDelta::Text(content),
                    });
                }
            }

            for tool_call in choice.delta.tool_calls {
                events.extend(self.map_tool_call(tool_call));
            }

            if let Some(reason) = choice.finish_reason {
                finish_reason_seen = true;
                if self.text_started && !self.text_stopped {
                    self.text_stopped = true;
                    if let Some(index) = self.text_block_index {
                        events.push(ModelStreamEvent::ContentBlockStop { index });
                    }
                }
                if self.thinking_started && !self.thinking_stopped {
                    self.thinking_stopped = true;
                    if let Some(index) = self.thinking_block_index {
                        events.push(ModelStreamEvent::ContentBlockStop { index });
                    }
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
                    usage_delta: usage_for_dialect(payload.usage.as_ref(), self.dialect),
                });
                self.terminal_pending = true;
            }
        }

        if let Some(usage_value) = payload.usage.as_ref() {
            if !self.stopped && !finish_reason_seen {
                events.push(ModelStreamEvent::MessageDelta {
                    stop_reason: None,
                    usage_delta: usage_for_dialect(Some(usage_value), self.dialect),
                });
            }
            events.extend(self.finish_message());
        }

        events
    }

    fn finish_message(&mut self) -> Vec<ModelStreamEvent> {
        if self.stopped || !self.started {
            return Vec::new();
        }
        let mut events = Vec::new();
        if self.thinking_started && !self.thinking_stopped {
            self.thinking_stopped = true;
            if let Some(index) = self.thinking_block_index {
                events.push(ModelStreamEvent::ContentBlockStop { index });
            }
        }
        if !self.continuation_emitted {
            if let Some(event) = continuation::stream_continuation_event(
                self.dialect,
                &self.reasoning_replay_payloads,
            ) {
                events.push(event);
            }
            self.continuation_emitted = true;
        }
        self.stopped = true;
        self.terminal_pending = false;
        events.push(ModelStreamEvent::MessageStop);
        events
    }

    fn ensure_text_block(&mut self, events: &mut Vec<ModelStreamEvent>) -> u32 {
        if let Some(index) = self.text_block_index {
            return index;
        }
        let index = self.next_block_index;
        self.next_block_index = index + 1;
        self.text_started = true;
        self.text_block_index = Some(index);
        events.push(ModelStreamEvent::ContentBlockStart {
            index,
            content_type: ContentType::Text,
        });
        index
    }

    fn ensure_thinking_block(&mut self, events: &mut Vec<ModelStreamEvent>) -> u32 {
        if let Some(index) = self.thinking_block_index {
            return index;
        }
        let index = self.next_block_index;
        self.next_block_index = index + 1;
        self.thinking_started = true;
        self.thinking_block_index = Some(index);
        events.push(ModelStreamEvent::ContentBlockStart {
            index,
            content_type: ContentType::Thinking,
        });
        index
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
    reasoning_content: Option<String>,
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

fn parse_sse_json<T: DeserializeOwned>(data: &str) -> Result<T, ModelStreamEvent> {
    serde_json::from_str(data).map_err(invalid_sse_json)
}

fn invalid_sse_json(error: serde_json::Error) -> ModelStreamEvent {
    stream_error(
        ModelError::UnexpectedResponse(format!("invalid OpenAI protocol SSE JSON: {error}")),
        ErrorClass::Fatal,
    )
}

#[cfg(test)]
mod tests {
    use std::future;
    use std::time::Duration;

    use futures::StreamExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::{response_to_stream, IncrementalSseParser, OpenAiStreamState, SseEvent};
    use crate::openai_protocol::OpenAiChatDialect;
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

    #[test]
    fn done_marks_stream_stopped_without_error() {
        let mut state = OpenAiStreamState::default();

        let events = state.map_event(SseEvent {
            data: "[DONE]".to_owned(),
        });

        assert!(events.is_empty());
        assert!(state.stopped);
    }

    #[test]
    fn finish_reason_marks_terminal_pending_until_usage_or_done() {
        let mut state = OpenAiStreamState::default();

        let events = state.map_event(SseEvent {
            data: "{\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}".to_owned(),
        });

        assert!(!events.contains(&ModelStreamEvent::MessageStop));
        assert!(state.terminal_pending);
        assert!(!state.stopped);
    }

    #[tokio::test]
    async fn done_terminates_response_stream_without_waiting_for_eof() {
        let response = pending_sse_response("data: [DONE]\n\n").await;
        let mut stream = response_to_stream(response, OpenAiChatDialect::Plain);

        let next = tokio::time::timeout(Duration::from_millis(200), stream.next())
            .await
            .expect("stream should terminate after [DONE] without EOF");

        assert!(next.is_none());
    }

    #[tokio::test]
    async fn finish_reason_terminates_response_stream_without_waiting_for_eof() {
        let response = pending_sse_response(
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\n",
        )
        .await;
        let mut stream = response_to_stream(response, OpenAiChatDialect::Plain);

        let mut events = Vec::new();
        while let Some(event) = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("stream should terminate after finish_reason without EOF")
        {
            events.push(event);
        }

        assert!(events.contains(&ModelStreamEvent::MessageStop));
    }

    #[tokio::test]
    async fn finish_reason_preserves_following_usage_chunk_before_message_stop() {
        let response = pending_sse_response(
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: {\"id\":\"chatcmpl_1\",\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n",
        )
        .await;
        let mut stream = response_to_stream(response, OpenAiChatDialect::Plain);

        let mut events = Vec::new();
        while let Some(event) = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("stream should terminate after preserving usage chunk")
        {
            events.push(event);
        }

        let usage_index = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    ModelStreamEvent::MessageDelta {
                        usage_delta,
                        ..
                    } if usage_delta.input_tokens == 3 && usage_delta.output_tokens == 2
                )
            })
            .expect("post-finish usage chunk should be emitted");
        let stop_index = events
            .iter()
            .position(|event| matches!(event, ModelStreamEvent::MessageStop))
            .expect("message stop should be emitted");
        assert!(usage_index < stop_index);
    }

    #[tokio::test]
    async fn finish_reason_terminates_even_when_provider_keeps_sending_comments() {
        let response = pending_sse_response_with_comments(
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\n",
        )
        .await;
        let mut stream = response_to_stream(response, OpenAiChatDialect::Plain);

        let mut events = Vec::new();
        while let Some(event) = tokio::time::timeout(Duration::from_millis(700), stream.next())
            .await
            .expect("stream should terminate after finish_reason grace despite comment frames")
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

    async fn pending_sse_response_with_comments(frame: &'static str) -> reqwest::Response {
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

            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
                let comment = ": keepalive\n\n";
                let chunk = format!("{:x}\r\n{}\r\n", comment.len(), comment);
                if socket.write_all(chunk.as_bytes()).await.is_err() {
                    break;
                }
                if socket.flush().await.is_err() {
                    break;
                }
            }
        });

        reqwest::get(url).await.unwrap()
    }
}

use std::collections::{HashMap, HashSet};

use async_stream::stream;
use futures::StreamExt;
use harness_contracts::{ModelError, StopReason, UsageSnapshot};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    ContentDelta, ContentType, ErrorClass, ErrorHints, InferContext, ModelRequest, ModelStream,
    ModelStreamEvent, ReasoningSummaryDelta, ThinkingDelta,
};

use super::continuation::{
    find_openai_responses_previous_response, openai_responses_previous_response_event,
    OpenAiResponsesContinuationCapture,
};
use super::request::{
    apply_openai_responses_options, merge_extra_object_protecting, responses_message,
    responses_tool, DEFAULT_MAX_TOKENS,
};

pub(super) async fn responses_request_body(
    req: &ModelRequest,
    ctx: &InferContext,
) -> Result<Value, ModelError> {
    let setup_fingerprint = req.provider_context.setup_fingerprint.as_deref();
    let previous = find_openai_responses_previous_response(
        &req.provider_context.continuations,
        &req.model_id,
        setup_fingerprint,
    );
    let previous_position = previous.as_ref().and_then(|previous| {
        req.messages
            .iter()
            .position(|message| message.id == previous.after_message_id)
    });

    let mut input = Vec::new();
    if previous_position.is_none() {
        if let Some(system) = &req.system {
            input.push(json!({
                "role": "system",
                "content": [{"type": "input_text", "text": system}],
            }));
        }
    }
    let message_start = previous_position.map_or(0, |position| position + 1);
    for message in req.messages.iter().skip(message_start) {
        input.extend(responses_message(message, ctx).await?);
    }

    let mut body = json!({
        "model": req.model_id,
        "input": input,
        "max_output_tokens": req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "stream": req.stream,
    });
    if let (Some(previous), Some(_)) = (previous, previous_position) {
        body["previous_response_id"] = json!(previous.response_id);
    }

    if let Some(temperature) = req.temperature {
        body["temperature"] = json!(temperature);
    }
    let response_options = req.options.openai_responses.as_ref();
    if let Some(tools) = &req.tools {
        let strict = response_options.is_some_and(|options| options.strict_tool_schemas);
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(|tool| responses_tool(tool, strict))
                .collect(),
        );
    }
    let mut protected_keys = Vec::new();
    if let Some(options) = response_options {
        protected_keys.extend(apply_openai_responses_options(&mut body, options));
    }
    merge_extra_object_protecting(&mut body, &req.extra, &protected_keys)?;

    Ok(body)
}

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

pub(super) fn response_to_stream(
    response: reqwest::Response,
    continuation: OpenAiResponsesContinuationCapture,
) -> ModelStream {
    let mut bytes = response.bytes_stream();
    Box::pin(stream! {
        let mut parser = IncrementalSseParser::default();
        let mut state = ResponsesStreamState::new(continuation);
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

pub(super) fn json_response_to_stream(
    value: Value,
    continuation: OpenAiResponsesContinuationCapture,
) -> Result<ModelStream, ModelError> {
    let response: ResponsesJson = serde_json::from_value(value).map_err(|error| {
        ModelError::UnexpectedResponse(format!("invalid Responses API JSON: {error}"))
    })?;
    let response_id = response.id.clone();
    let failed_message = response
        .error
        .as_ref()
        .and_then(|error| error.message.clone());
    let stop_reason = non_stream_stop_reason(&response);
    let usage = usage(response.usage.as_ref());
    let mut events = vec![ModelStreamEvent::MessageStart {
        message_id: response_id.clone(),
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
        } else if is_builtin_tool_output(&item.kind) {
            let index = next_index;
            next_index += 1;
            let status = item.raw.get("status").and_then(Value::as_str);
            events.push(ModelStreamEvent::ContentBlockStart {
                index,
                content_type: ContentType::Thinking,
            });
            events.push(ModelStreamEvent::ContentBlockDelta {
                index,
                delta: ContentDelta::Thinking(ThinkingDelta {
                    text: Some(builtin_tool_status_text(&item.kind, status)),
                    provider_native: Some(item.provider_native()),
                    signature: None,
                }),
            });
            events.push(ModelStreamEvent::ContentBlockStop { index });
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
            let summary = item
                .summary
                .iter()
                .filter_map(|summary| summary.text.as_deref())
                .collect::<Vec<_>>()
                .join("");
            let index = next_index;
            next_index += 1;
            events.push(ModelStreamEvent::ContentBlockStart {
                index,
                content_type: ContentType::Thinking,
            });
            events.push(ModelStreamEvent::ContentBlockDelta {
                index,
                delta: ContentDelta::ReasoningSummary(ReasoningSummaryDelta {
                    text: summary,
                    provider_native: Some(item.raw),
                }),
            });
            events.push(ModelStreamEvent::ContentBlockStop { index });
        }
    }

    if matches!(stop_reason, StopReason::Error(_)) && failed_message.is_some() {
        events.push(stream_error(
            ModelError::ProviderUnavailable(
                failed_message.unwrap_or_else(|| "Responses API request failed".to_owned()),
            ),
            ErrorClass::Fatal,
        ));
        return Ok(Box::pin(futures::stream::iter(events)));
    }
    if let Some(event) = openai_responses_previous_response_event(&response_id, &continuation) {
        events.push(event);
    }
    events.push(ModelStreamEvent::MessageDelta {
        stop_reason: Some(stop_reason),
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ResponseBlockKey {
    item_id: String,
    output_index: Option<u64>,
    content_index: Option<u64>,
}

impl ResponseBlockKey {
    fn is_empty(&self) -> bool {
        self.item_id.is_empty() && self.output_index.is_none() && self.content_index.is_none()
    }
}

#[derive(Default)]
struct ToolBlockState {
    index: u32,
    saw_arguments_delta: bool,
}

struct ResponsesStreamState {
    started: bool,
    stopped: bool,
    response_id: Option<String>,
    text_indices: HashMap<ResponseBlockKey, u32>,
    refusal_indices: HashMap<ResponseBlockKey, u32>,
    thinking_indices: HashMap<ResponseBlockKey, u32>,
    tool_indices: HashMap<ResponseBlockKey, ToolBlockState>,
    closed: HashSet<u32>,
    next_index: u32,
    continuation: OpenAiResponsesContinuationCapture,
}

impl ResponsesStreamState {
    fn new(continuation: OpenAiResponsesContinuationCapture) -> Self {
        Self {
            started: false,
            stopped: false,
            response_id: None,
            text_indices: HashMap::new(),
            refusal_indices: HashMap::new(),
            thinking_indices: HashMap::new(),
            tool_indices: HashMap::new(),
            closed: HashSet::new(),
            next_index: 0,
            continuation,
        }
    }

    fn map_event(&mut self, event: SseEvent) -> Vec<ModelStreamEvent> {
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
        let event_name = event
            .event
            .as_deref()
            .or_else(|| data.get("type").and_then(Value::as_str))
            .unwrap_or_default();

        let mut events = Vec::new();
        if !self.started {
            self.started = true;
            self.response_id = response_id_from_event(&data).map(ToOwned::to_owned);
            events.push(ModelStreamEvent::MessageStart {
                message_id: self.response_id.clone().unwrap_or_default(),
                usage: UsageSnapshot::default(),
            });
        } else if self.response_id.is_none() {
            self.response_id = response_id_from_event(&data).map(ToOwned::to_owned);
        }

        match event_name {
            "response.output_text.delta" => {
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        let key = block_key(&data);
                        let index = self.ensure_text_block(key, &mut events);
                        events.push(ModelStreamEvent::ContentBlockDelta {
                            index,
                            delta: ContentDelta::Text(delta.to_owned()),
                        });
                    }
                }
            }
            "response.output_text.done" => {
                let key = block_key(&data);
                if !self.text_indices.contains_key(&key) {
                    if let Some(text) = data.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            let index = self.ensure_text_block(key.clone(), &mut events);
                            events.push(ModelStreamEvent::ContentBlockDelta {
                                index,
                                delta: ContentDelta::Text(text.to_owned()),
                            });
                        }
                    }
                }
                self.close_keyed_text_block(&key, &mut events);
            }
            "response.refusal.delta" => {
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        let key = block_key(&data);
                        let index = self.ensure_refusal_block(key, &mut events);
                        events.push(ModelStreamEvent::ContentBlockDelta {
                            index,
                            delta: ContentDelta::Text(delta.to_owned()),
                        });
                    }
                }
            }
            "response.refusal.done" => {
                let key = block_key(&data);
                if !self.refusal_indices.contains_key(&key) {
                    if let Some(refusal) = data.get("refusal").and_then(Value::as_str) {
                        if !refusal.is_empty() {
                            let index = self.ensure_refusal_block(key.clone(), &mut events);
                            events.push(ModelStreamEvent::ContentBlockDelta {
                                index,
                                delta: ContentDelta::Text(refusal.to_owned()),
                            });
                        }
                    }
                }
                self.close_keyed_refusal_block(&key, &mut events);
            }
            "response.reasoning_text.delta" => {
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        let key = block_key(&data);
                        let index = self.ensure_thinking_block(key, &mut events);
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
            }
            "response.reasoning_summary_text.delta" => {
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        let key = block_key(&data);
                        let index = self.ensure_thinking_block(key, &mut events);
                        events.push(ModelStreamEvent::ContentBlockDelta {
                            index,
                            delta: ContentDelta::ReasoningSummary(ReasoningSummaryDelta {
                                text: delta.to_owned(),
                                provider_native: Some(data),
                            }),
                        });
                    }
                }
            }
            "response.output_item.added" => {
                let item = data.get("item").unwrap_or(&data);
                let item_type = item.get("type").and_then(Value::as_str);
                if item_type == Some("function_call") {
                    let key = item_block_key(&data, item);
                    let index = self.ensure_tool_block(key, &mut events);
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
                } else if let Some(item_type) =
                    item_type.filter(|kind| is_builtin_tool_output(kind))
                {
                    let status = item.get("status").and_then(Value::as_str).or(Some("added"));
                    self.push_builtin_tool_event(
                        &mut events,
                        item_type,
                        status,
                        provider_native_with_type(item, item_type),
                    );
                }
            }
            "response.output_item.done" => {
                let item = data.get("item").unwrap_or(&data);
                let item_type = item.get("type").and_then(Value::as_str);
                if item_type == Some("function_call") {
                    let key = item_block_key(&data, item);
                    self.close_keyed_tool_block(&key, &mut events);
                } else if let Some(item_type) =
                    item_type.filter(|kind| is_builtin_tool_output(kind))
                {
                    self.push_builtin_tool_event(
                        &mut events,
                        item_type,
                        Some("done"),
                        provider_native_with_type(item, item_type),
                    );
                }
            }
            "response.function_call_arguments.delta" => {
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        let key = self.tool_key_for_event(&data);
                        let index = self.ensure_tool_block(key.clone(), &mut events);
                        if let Some(tool) = self.tool_indices.get_mut(&key) {
                            tool.saw_arguments_delta = true;
                        }
                        events.push(ModelStreamEvent::ContentBlockDelta {
                            index,
                            delta: ContentDelta::ToolUseInputJson(delta.to_owned()),
                        });
                    }
                }
            }
            "response.function_call_arguments.done" => {
                let key = self.tool_key_for_event(&data);
                let index = self.ensure_tool_block(key.clone(), &mut events);
                let saw_arguments_delta = self
                    .tool_indices
                    .get(&key)
                    .is_some_and(|tool| tool.saw_arguments_delta);
                if !saw_arguments_delta {
                    if let Some(arguments) = data.get("arguments").and_then(Value::as_str) {
                        if !arguments.is_empty() {
                            events.push(ModelStreamEvent::ContentBlockDelta {
                                index,
                                delta: ContentDelta::ToolUseInputJson(arguments.to_owned()),
                            });
                        }
                    }
                }
                self.close_keyed_tool_block(&key, &mut events);
            }
            event_name if builtin_tool_stream_event(event_name).is_some() => {
                let (item_type, status) = builtin_tool_stream_event(event_name).unwrap();
                self.push_builtin_tool_event(
                    &mut events,
                    item_type,
                    Some(status),
                    provider_native_with_type(&data, event_name),
                );
            }
            "response.completed" => {
                self.close_blocks(&mut events);
                let response = data.get("response");
                if let Some(response_id) = response
                    .and_then(|response| response.get("id"))
                    .and_then(Value::as_str)
                    .or_else(|| self.response_id.as_deref())
                {
                    if let Some(event) =
                        openai_responses_previous_response_event(response_id, &self.continuation)
                    {
                        events.push(event);
                    }
                }
                events.push(ModelStreamEvent::MessageDelta {
                    stop_reason: Some(StopReason::EndTurn),
                    usage_delta: usage(response.and_then(|response| response.get("usage"))),
                });
                events.push(ModelStreamEvent::MessageStop);
                self.stopped = true;
            }
            "response.incomplete" => {
                self.close_blocks(&mut events);
                events.push(ModelStreamEvent::MessageDelta {
                    stop_reason: Some(incomplete_stop_reason(
                        data.get("response").unwrap_or(&data),
                    )),
                    usage_delta: usage(
                        data.get("response")
                            .and_then(|response| response.get("usage")),
                    ),
                });
                events.push(ModelStreamEvent::MessageStop);
                self.stopped = true;
            }
            "response.failed" | "error" | "response.error" => {
                events.push(stream_error(
                    ModelError::ProviderUnavailable(stream_error_message(&data)),
                    ErrorClass::Fatal,
                ));
                self.stopped = true;
            }
            _ => {}
        }

        events
    }

    fn ensure_text_block(
        &mut self,
        key: ResponseBlockKey,
        events: &mut Vec<ModelStreamEvent>,
    ) -> u32 {
        if let Some(index) = self.text_indices.get(&key) {
            return *index;
        }
        let index = self.start_block(events, ContentType::Text);
        self.text_indices.insert(key, index);
        index
    }

    fn ensure_refusal_block(
        &mut self,
        key: ResponseBlockKey,
        events: &mut Vec<ModelStreamEvent>,
    ) -> u32 {
        if let Some(index) = self.refusal_indices.get(&key) {
            return *index;
        }
        let index = self.start_block(events, ContentType::Text);
        self.refusal_indices.insert(key, index);
        index
    }

    fn ensure_thinking_block(
        &mut self,
        key: ResponseBlockKey,
        events: &mut Vec<ModelStreamEvent>,
    ) -> u32 {
        if let Some(index) = self.thinking_indices.get(&key) {
            return *index;
        }
        let index = self.start_block(events, ContentType::Thinking);
        self.thinking_indices.insert(key, index);
        index
    }

    fn push_builtin_tool_event(
        &mut self,
        events: &mut Vec<ModelStreamEvent>,
        item_type: &str,
        status: Option<&str>,
        provider_native: Value,
    ) {
        let key = block_key(&provider_native);
        let index = self.ensure_thinking_block(key, events);
        events.push(ModelStreamEvent::ContentBlockDelta {
            index,
            delta: ContentDelta::Thinking(ThinkingDelta {
                text: Some(builtin_tool_status_text(item_type, status)),
                provider_native: Some(provider_native),
                signature: None,
            }),
        });
    }

    fn ensure_tool_block(
        &mut self,
        key: ResponseBlockKey,
        events: &mut Vec<ModelStreamEvent>,
    ) -> u32 {
        if let Some(tool) = self.tool_indices.get(&key) {
            return tool.index;
        }
        let index = self.start_block(events, ContentType::ToolUse);
        self.tool_indices.insert(
            key,
            ToolBlockState {
                index,
                saw_arguments_delta: false,
            },
        );
        index
    }

    fn tool_key_for_event(&self, data: &Value) -> ResponseBlockKey {
        let key = block_key(data);
        if key.is_empty() && self.tool_indices.len() == 1 {
            return self
                .tool_indices
                .keys()
                .next()
                .expect("single tool key should exist")
                .clone();
        }
        key
    }

    fn start_block(
        &mut self,
        events: &mut Vec<ModelStreamEvent>,
        content_type: ContentType,
    ) -> u32 {
        let index = self.next_index;
        self.next_index += 1;
        events.push(ModelStreamEvent::ContentBlockStart {
            index,
            content_type,
        });
        index
    }

    fn close_keyed_text_block(
        &mut self,
        key: &ResponseBlockKey,
        events: &mut Vec<ModelStreamEvent>,
    ) {
        if let Some(index) = self.text_indices.remove(key) {
            self.close_index(index, events);
        }
    }

    fn close_keyed_refusal_block(
        &mut self,
        key: &ResponseBlockKey,
        events: &mut Vec<ModelStreamEvent>,
    ) {
        if let Some(index) = self.refusal_indices.remove(key) {
            self.close_index(index, events);
        }
    }

    fn close_keyed_tool_block(
        &mut self,
        key: &ResponseBlockKey,
        events: &mut Vec<ModelStreamEvent>,
    ) {
        if let Some(tool) = self.tool_indices.remove(key) {
            self.close_index(tool.index, events);
        }
    }

    fn close_blocks(&mut self, events: &mut Vec<ModelStreamEvent>) {
        let text_indices = self
            .text_indices
            .drain()
            .map(|(_, index)| index)
            .collect::<Vec<_>>();
        let refusal_indices = self
            .refusal_indices
            .drain()
            .map(|(_, index)| index)
            .collect::<Vec<_>>();
        let thinking_indices = self
            .thinking_indices
            .drain()
            .map(|(_, index)| index)
            .collect::<Vec<_>>();
        let tool_indices = self
            .tool_indices
            .drain()
            .map(|(_, tool)| tool.index)
            .collect::<Vec<_>>();
        for index in text_indices
            .into_iter()
            .chain(refusal_indices)
            .chain(thinking_indices)
            .chain(tool_indices)
        {
            self.close_index(index, events);
        }
    }

    fn close_index(&mut self, index: u32, events: &mut Vec<ModelStreamEvent>) {
        if self.closed.insert(index) {
            events.push(ModelStreamEvent::ContentBlockStop { index });
        }
    }
}

impl Default for ResponsesStreamState {
    fn default() -> Self {
        Self::new(OpenAiResponsesContinuationCapture {
            model_id: String::new(),
            setup_fingerprint: None,
        })
    }
}

fn response_id_from_event(data: &Value) -> Option<&str> {
    data.get("response")
        .and_then(|response| response.get("id"))
        .or_else(|| data.get("response_id"))
        .or_else(|| data.get("id"))
        .and_then(Value::as_str)
}

fn block_key(data: &Value) -> ResponseBlockKey {
    ResponseBlockKey {
        item_id: data
            .get("item_id")
            .and_then(Value::as_str)
            .or_else(|| data.get("id").and_then(Value::as_str))
            .unwrap_or_default()
            .to_owned(),
        output_index: data.get("output_index").and_then(Value::as_u64),
        content_index: data.get("content_index").and_then(Value::as_u64),
    }
}

fn item_block_key(data: &Value, item: &Value) -> ResponseBlockKey {
    ResponseBlockKey {
        item_id: item
            .get("id")
            .and_then(Value::as_str)
            .or_else(|| data.get("item_id").and_then(Value::as_str))
            .unwrap_or_default()
            .to_owned(),
        output_index: data.get("output_index").and_then(Value::as_u64),
        content_index: data.get("content_index").and_then(Value::as_u64),
    }
}

fn non_stream_stop_reason(response: &ResponsesJson) -> StopReason {
    match response.status.as_deref() {
        Some("incomplete") => incomplete_stop_reason(
            response
                .raw
                .get("incomplete_details")
                .unwrap_or(&response.raw),
        ),
        Some("failed") => StopReason::Error(
            response
                .error
                .as_ref()
                .and_then(|error| error.message.clone())
                .unwrap_or_else(|| "Responses API request failed".to_owned()),
        ),
        _ => StopReason::EndTurn,
    }
}

fn incomplete_stop_reason(response: &Value) -> StopReason {
    let reason = response
        .get("incomplete_details")
        .and_then(|details| details.get("reason"))
        .or_else(|| response.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or("incomplete");
    if reason == "max_output_tokens" {
        StopReason::MaxIterations
    } else {
        StopReason::Error(format!("Responses API response incomplete: {reason}"))
    }
}

fn stream_error_message(data: &Value) -> String {
    data.get("error")
        .and_then(|error| error.get("message").or_else(|| error.get("error")))
        .and_then(Value::as_str)
        .or_else(|| data.get("message").and_then(Value::as_str))
        .unwrap_or("Responses API stream failed")
        .to_owned()
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
    status: Option<String>,
    #[serde(default)]
    output: Vec<ResponseOutputItem>,
    usage: Option<Value>,
    error: Option<ResponseErrorJson>,
    #[serde(flatten)]
    raw: Value,
}

#[derive(Debug, Deserialize)]
struct ResponseErrorJson {
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponseOutputItem {
    #[serde(rename = "type")]
    kind: String,
    id: String,
    call_id: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
    #[serde(default)]
    summary: Vec<ResponseReasoningSummary>,
    #[serde(default)]
    content: Vec<ResponseContent>,
    #[serde(flatten)]
    raw: Value,
}

impl ResponseOutputItem {
    fn provider_native(&self) -> Value {
        let mut value = provider_native_with_type(&self.raw, &self.kind);
        if let Value::Object(object) = &mut value {
            object.insert("id".to_owned(), Value::String(self.id.clone()));
            if let Some(call_id) = &self.call_id {
                object.insert("call_id".to_owned(), Value::String(call_id.clone()));
            }
            if let Some(name) = &self.name {
                object.insert("name".to_owned(), Value::String(name.clone()));
            }
            if let Some(arguments) = &self.arguments {
                object.insert("arguments".to_owned(), Value::String(arguments.clone()));
            }
            if !self.summary.is_empty() {
                object.insert(
                    "summary".to_owned(),
                    Value::Array(
                        self.summary
                            .iter()
                            .map(|summary| {
                                json!({
                                    "type": "summary_text",
                                    "text": summary.text.clone().unwrap_or_default(),
                                })
                            })
                            .collect(),
                    ),
                );
            }
        }
        value
    }
}

fn is_builtin_tool_output(kind: &str) -> bool {
    matches!(
        kind,
        "web_search_call" | "web_extractor_call" | "code_interpreter_call"
    )
}

fn builtin_tool_stream_event(event_name: &str) -> Option<(&str, &str)> {
    let rest = event_name.strip_prefix("response.")?;
    let (item_type, status) = rest.rsplit_once('.')?;
    is_builtin_tool_output(item_type).then_some((item_type, status))
}

fn builtin_tool_status_text(item_type: &str, status: Option<&str>) -> String {
    status.map_or_else(
        || item_type.to_owned(),
        |status| format!("{item_type} {status}"),
    )
}

fn provider_native_with_type(value: &Value, kind: &str) -> Value {
    let mut native = value.clone();
    match &mut native {
        Value::Object(object) => {
            object.insert("type".to_owned(), Value::String(kind.to_owned()));
        }
        _ => {
            native = json!({ "type": kind, "value": value });
        }
    }
    native
}

#[derive(Debug, Deserialize)]
struct ResponseReasoningSummary {
    text: Option<String>,
    #[serde(flatten)]
    _raw: Value,
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

    use serde_json::json;

    use super::super::continuation::OpenAiResponsesContinuationCapture;
    use super::{json_response_to_stream, response_to_stream, ResponsesStreamState, SseEvent};
    use crate::{ContentDelta, ContentType, ModelStreamEvent};

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
    async fn json_response_maps_builtin_tool_output_as_provider_native_thinking() {
        let stream = json_response_to_stream(
            json!({
                "id": "resp_1",
                "output": [
                    {
                        "id": "ws_1",
                        "type": "web_search_call",
                        "status": "completed",
                        "action": {
                            "type": "search",
                            "query": "qwen responses"
                        }
                    }
                ],
                "usage": {
                    "input_tokens": 1,
                    "output_tokens": 2
                }
            }),
            OpenAiResponsesContinuationCapture {
                model_id: "gpt-test".to_owned(),
                setup_fingerprint: None,
            },
        )
        .expect("Responses JSON should parse");

        let events = stream.collect::<Vec<_>>().await;

        assert!(events.contains(&ModelStreamEvent::ContentBlockStart {
            index: 0,
            content_type: ContentType::Thinking,
        }));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Thinking(thinking),
            } if thinking.text.as_deref() == Some("web_search_call completed")
                && thinking.provider_native.as_ref().and_then(|value| value.get("type")).and_then(serde_json::Value::as_str) == Some("web_search_call")
        )));
        assert!(!events.iter().any(|event| matches!(
            event,
            ModelStreamEvent::ContentBlockDelta {
                delta: ContentDelta::ToolUseStart { .. },
                ..
            }
        )));
    }

    #[test]
    fn sse_builtin_tool_status_is_provider_native_thinking() {
        let mut state = ResponsesStreamState::default();

        let events = state.map_event(SseEvent {
            event: Some("response.web_search_call.searching".to_owned()),
            data: r#"{"response":{"id":"resp_1"},"type":"response.web_search_call.searching","item_id":"ws_1"}"#.to_owned(),
        });

        assert!(events.contains(&ModelStreamEvent::ContentBlockStart {
            index: 0,
            content_type: ContentType::Thinking,
        }));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Thinking(thinking),
            } if thinking.text.as_deref() == Some("web_search_call searching")
                && thinking.provider_native.as_ref().and_then(|value| value.get("type")).and_then(serde_json::Value::as_str) == Some("response.web_search_call.searching")
        )));
        assert!(!events.iter().any(|event| matches!(
            event,
            ModelStreamEvent::ContentBlockDelta {
                delta: ContentDelta::ToolUseStart { .. },
                ..
            }
        )));
    }

    #[tokio::test]
    async fn completed_terminates_response_stream_without_waiting_for_eof() {
        let response = pending_sse_response(
            "event: response.completed\ndata: {\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n",
        )
        .await;
        let mut stream = response_to_stream(
            response,
            OpenAiResponsesContinuationCapture {
                model_id: "gpt-5.4-mini".to_owned(),
                setup_fingerprint: None,
            },
        );

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

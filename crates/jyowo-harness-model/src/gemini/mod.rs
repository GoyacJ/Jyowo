use std::time::{Duration, Instant};

use async_stream::stream;
use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::{
    Message, MessagePart, MessageRole, ModelError, StopReason, ToolDescriptor, ToolResult,
    ToolResultPart, UsageSnapshot,
};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{json, Value};

use crate::{
    apply_response_headers_middlewares, wrap_stream_with_cancel_deadline, ApiMode, ContentDelta,
    ContentType, ErrorClass, ErrorHints, GeminiCacheMode, InferContext, ModelCapabilities,
    ModelDescriptor, ModelProvider, ModelRequest, ModelStream, ModelStreamEvent, PromptCacheStyle,
};

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";
const API_VERSION: &str = "v1beta";
const DEFAULT_MAX_TOKENS: u32 = 1024;
const PROVIDER_ID: &str = "gemini";
pub const GEMINI_API_KEY_ENV: &str = "GEMINI_API_KEY";

#[derive(Clone)]
pub struct GeminiProvider {
    http: reqwest::Client,
    api_key: SecretString,
    base_url: String,
}

impl GeminiProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: SecretString::new(api_key.into().into_boxed_str()),
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    async fn send_once(&self, req: &ModelRequest) -> Result<reqwest::Response, ModelError> {
        let method = if req.stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        let mut url = format!(
            "{}/{}/models/{}:{}",
            self.base_url.trim_end_matches('/'),
            API_VERSION,
            req.model_id,
            method
        );
        if req.stream {
            url.push_str("?alt=sse");
        }
        let request = self
            .http
            .post(url)
            .headers(self.headers()?)
            .json(&request_body(req)?);
        let response = request
            .send()
            .await
            .map_err(|error| ModelError::ProviderUnavailable(error.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(match status.as_u16() {
                401 | 403 => ModelError::AuthExpired(body),
                429 => ModelError::RateLimited(body),
                400 => ModelError::InvalidRequest(body),
                _ => ModelError::ProviderUnavailable(body),
            });
        }
        Ok(response)
    }

    fn headers(&self) -> Result<HeaderMap, ModelError> {
        let mut headers = HeaderMap::new();
        let key = HeaderValue::from_str(self.api_key.expose_secret())
            .map_err(|error| ModelError::AuthExpired(error.to_string()))?;
        headers.insert("x-goog-api-key", key);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }
}

#[async_trait]
impl ModelProvider for GeminiProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![
            descriptor("gemini-2.5-pro", "Gemini 2.5 Pro", 32_000),
            descriptor("gemini-2.5-flash", "Gemini 2.5 Flash", 16_384),
            descriptor("gemini-2.5-flash-lite", "Gemini 2.5 Flash Lite", 8192),
        ]
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        validate_request(&req, &ctx)?;
        let response = self.send_once(&req).await?;
        let headers = response.headers().clone();
        apply_response_headers_middlewares(&headers, &ctx).await?;
        if req.stream {
            Ok(wrap_stream_with_cancel_deadline(
                response_to_stream(response),
                &ctx,
            ))
        } else {
            let value = response
                .json::<Value>()
                .await
                .map_err(|error| ModelError::UnexpectedResponse(error.to_string()))?;
            json_to_stream(value)
        }
    }

    fn prompt_cache_style(&self) -> PromptCacheStyle {
        PromptCacheStyle::Gemini {
            mode: GeminiCacheMode::ExternalReferenceOnly,
        }
    }

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_vision(&self) -> bool {
        true
    }
}

fn validate_request(req: &ModelRequest, ctx: &InferContext) -> Result<(), ModelError> {
    if req.api_mode != ApiMode::GenerateContent {
        return Err(ModelError::InvalidRequest(
            "GeminiProvider only supports ApiMode::GenerateContent".to_owned(),
        ));
    }
    if !req.cache_breakpoints.is_empty() {
        return Err(ModelError::InvalidRequest(
            "GeminiProvider accepts only external cachedContent references".to_owned(),
        ));
    }
    if ctx.cancel.is_cancelled() {
        return Err(ModelError::Cancelled);
    }
    if let Some(deadline) = ctx.deadline {
        if Instant::now() >= deadline {
            return Err(ModelError::DeadlineExceeded(Duration::ZERO));
        }
    }
    Ok(())
}

fn request_body(req: &ModelRequest) -> Result<Value, ModelError> {
    let mut contents = Vec::new();
    for message in &req.messages {
        contents.push(content(message)?);
    }

    let mut body = json!({
        "contents": contents,
        "generationConfig": {
            "maxOutputTokens": req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        },
    });
    if let Some(system) = &req.system {
        body["systemInstruction"] = json!({
            "parts": [{ "text": system }],
        });
    }
    if let Some(temperature) = req.temperature {
        body["generationConfig"]["temperature"] = json!(temperature);
    }
    if let Some(tools) = &req.tools {
        body["tools"] = json!([{ "functionDeclarations": tools.iter().map(function_declaration).collect::<Vec<_>>() }]);
    }
    if let Some(cached_content) = req.extra.get("cached_content").and_then(Value::as_str) {
        body["cachedContent"] = json!(cached_content);
    }
    Ok(body)
}

fn content(message: &Message) -> Result<Value, ModelError> {
    let role = match message.role {
        MessageRole::Assistant => "model",
        MessageRole::Tool => "function",
        MessageRole::User | MessageRole::System => "user",
        _ => {
            return Err(ModelError::InvalidRequest(
                "unsupported Gemini message role".to_owned(),
            ));
        }
    };
    Ok(json!({
        "role": role,
        "parts": message.parts.iter().map(part).collect::<Result<Vec<_>, _>>()?,
    }))
}

fn part(part: &MessagePart) -> Result<Value, ModelError> {
    match part {
        MessagePart::Text(text) => Ok(json!({ "text": text })),
        MessagePart::ToolUse { name, input, .. } => Ok(json!({
            "functionCall": {
                "name": name,
                "args": input,
            },
        })),
        MessagePart::ToolResult {
            tool_use_id,
            content,
        } => Ok(json!({
            "functionResponse": {
                "name": tool_use_id.to_string(),
                "response": tool_result(content)?,
            },
        })),
        MessagePart::Image { .. } | MessagePart::Thinking(_) => Err(ModelError::InvalidRequest(
            "GeminiProvider only supports text and tool parts in M2-T04.7".to_owned(),
        )),
        _ => Err(ModelError::InvalidRequest(
            "unsupported Gemini message part".to_owned(),
        )),
    }
}

fn tool_result(content: &ToolResult) -> Result<Value, ModelError> {
    match content {
        ToolResult::Text(text) => Ok(json!({ "content": text })),
        ToolResult::Structured(value) => Ok(value.clone()),
        ToolResult::Mixed(parts) => Ok(json!({
            "content": parts.iter().map(tool_result_part).collect::<Result<Vec<_>, _>>()?,
        })),
        ToolResult::Blob { .. } => Err(ModelError::InvalidRequest(
            "GeminiProvider does not inline blob tool results in M2-T04.7".to_owned(),
        )),
        _ => Err(ModelError::InvalidRequest(
            "unsupported Gemini tool result".to_owned(),
        )),
    }
}

fn tool_result_part(part: &ToolResultPart) -> Result<Value, ModelError> {
    match part {
        ToolResultPart::Structured { value, .. } => Ok(value.clone()),
        ToolResultPart::Text { text } | ToolResultPart::Code { text, .. } => {
            Ok(json!({ "text": text }))
        }
        ToolResultPart::Reference { summary, .. } => Ok(json!({ "text": summary })),
        ToolResultPart::Blob { .. } => Err(ModelError::InvalidRequest(
            "GeminiProvider does not inline blob tool result parts in M2-T04.7".to_owned(),
        )),
        _ => Err(ModelError::InvalidRequest(
            "unsupported Gemini tool result part".to_owned(),
        )),
    }
}

fn function_declaration(tool: &ToolDescriptor) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "parameters": tool.input_schema,
    })
}

fn response_to_stream(response: reqwest::Response) -> ModelStream {
    let mut bytes = response.bytes_stream();
    Box::pin(stream! {
        let mut parser = IncrementalSseParser::default();
        let mut state = GeminiStreamState::default();
        while let Some(chunk) = bytes.next().await {
            match chunk {
                Ok(chunk) => match parser.push(&chunk) {
                    Ok(events) => {
                        for event in events {
                            for mapped in state.map_chunk(&event.data) {
                                yield mapped;
                            }
                        }
                    }
                    Err(error) => yield stream_error(error, ErrorClass::Fatal),
                },
                Err(error) => {
                    yield stream_error(ModelError::ProviderUnavailable(error.to_string()), ErrorClass::Transient);
                    return;
                }
            }
        }
        for event in parser.finish() {
            for mapped in state.map_chunk(&event.data) {
                yield mapped;
            }
        }
        if state.started && !state.stopped {
            yield ModelStreamEvent::MessageStop;
        }
    })
}

fn json_to_stream(value: Value) -> Result<ModelStream, ModelError> {
    let mut state = GeminiStreamState::default();
    Ok(Box::pin(futures::stream::iter(state.map_value(value)?)))
}

#[derive(Default)]
struct GeminiStreamState {
    started: bool,
    stopped: bool,
    text_started: bool,
    text_stopped: bool,
    next_index: u32,
}

impl GeminiStreamState {
    fn map_chunk(&mut self, data: &str) -> Vec<ModelStreamEvent> {
        match serde_json::from_str::<Value>(data) {
            Ok(value) => match self.map_value(value) {
                Ok(events) => events,
                Err(error) => vec![stream_error(error, ErrorClass::Fatal)],
            },
            Err(error) => vec![stream_error(
                ModelError::UnexpectedResponse(format!("invalid Gemini SSE JSON: {error}")),
                ErrorClass::Fatal,
            )],
        }
    }

    fn map_value(&mut self, value: Value) -> Result<Vec<ModelStreamEvent>, ModelError> {
        if let Some(error) = value.get("error") {
            return Ok(vec![stream_error(
                ModelError::ProviderUnavailable(
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("Gemini stream error")
                        .to_owned(),
                ),
                ErrorClass::Fatal,
            )]);
        }

        let mut events = Vec::new();
        if !self.started {
            self.started = true;
            events.push(ModelStreamEvent::MessageStart {
                message_id: value
                    .get("responseId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                usage: usage(value.get("usageMetadata")),
            });
        }

        for candidate in value
            .get("candidates")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for part in candidate
                .get("content")
                .and_then(|content| content.get("parts"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        if !self.text_started {
                            self.text_started = true;
                            self.next_index = self.next_index.max(1);
                            events.push(ModelStreamEvent::ContentBlockStart {
                                index: 0,
                                content_type: ContentType::Text,
                            });
                        }
                        events.push(ModelStreamEvent::ContentBlockDelta {
                            index: 0,
                            delta: ContentDelta::Text(text.to_owned()),
                        });
                    }
                }
                if let Some(function_call) = part.get("functionCall") {
                    let index = self.next_index.max(1);
                    self.next_index = index + 1;
                    let name = function_call
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    events.push(ModelStreamEvent::ContentBlockStart {
                        index,
                        content_type: ContentType::ToolUse,
                    });
                    events.push(ModelStreamEvent::ContentBlockDelta {
                        index,
                        delta: ContentDelta::ToolUseStart {
                            id: format!("gemini-{index}-{name}"),
                            name,
                        },
                    });
                    events.push(ModelStreamEvent::ContentBlockDelta {
                        index,
                        delta: ContentDelta::ToolUseInputJson(
                            function_call
                                .get("args")
                                .cloned()
                                .unwrap_or(Value::Object(serde_json::Map::default()))
                                .to_string(),
                        ),
                    });
                    events.push(ModelStreamEvent::ContentBlockStop { index });
                }
            }

            if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
                if self.text_started && !self.text_stopped {
                    self.text_stopped = true;
                    events.push(ModelStreamEvent::ContentBlockStop { index: 0 });
                }
                events.push(ModelStreamEvent::MessageDelta {
                    stop_reason: Some(stop_reason(reason)),
                    usage_delta: usage(value.get("usageMetadata")),
                });
                events.push(ModelStreamEvent::MessageStop);
                self.stopped = true;
            }
        }

        Ok(events)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SseEvent {
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
    (!data_lines.is_empty()).then(|| SseEvent {
        data: data_lines.join("\n"),
    })
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
            .and_then(|usage| usage.get("promptTokenCount"))
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        output_tokens: value
            .and_then(|usage| usage.get("candidatesTokenCount"))
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        cache_read_tokens: value
            .and_then(|usage| usage.get("cachedContentTokenCount"))
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        cache_write_tokens: 0,
        cost_micros: 0,
        tool_calls: 0,
    }
}

fn stop_reason(reason: &str) -> StopReason {
    match reason {
        "STOP" => StopReason::EndTurn,
        "MAX_TOKENS" => StopReason::MaxIterations,
        "MALFORMED_FUNCTION_CALL" => StopReason::ToolUse,
        other => StopReason::Error(other.to_owned()),
    }
}

fn descriptor(model_id: &str, display_name: &str, max_output_tokens: u32) -> ModelDescriptor {
    ModelDescriptor {
        provider_id: "gemini".to_owned(),
        model_id: model_id.to_owned(),
        display_name: display_name.to_owned(),
        context_window: 1_000_000,
        max_output_tokens,
        capabilities: ModelCapabilities {
            supports_tools: true,
            supports_vision: true,
            supports_thinking: false,
            supports_prompt_cache: true,
            supports_tool_reference: false,
            tool_reference_beta_header: None,
        },
        pricing: None,
    }
}

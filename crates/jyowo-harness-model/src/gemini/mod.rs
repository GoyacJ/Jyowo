use std::time::{Duration, Instant};

use async_stream::stream;
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use futures::StreamExt;
use harness_contracts::{
    BlobRef, BlobStore, Message, MessagePart, MessageRole, ModelError, StopReason, TenantId,
    ToolDescriptor, ToolResult, ToolResultPart, UsageSnapshot,
};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{json, Value};

use crate::{
    apply_response_headers_middlewares, wrap_stream_with_cancel_deadline, ContentDelta,
    ContentType, ErrorClass, ErrorHints, GeminiCacheMode, InferContext, ModelDescriptor,
    ModelProtocol, ModelProvider, ModelRequest, ModelStream, ModelStreamEvent, PromptCacheStyle,
};

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";
const API_VERSION: &str = "v1beta";
const DEFAULT_MAX_TOKENS: u32 = 1024;
const MAX_GEMINI_ERROR_BODY_BYTES: u64 = 64 * 1024;
const MAX_GEMINI_INLINE_BLOB_BYTES: u64 = 20 * 1024 * 1024;
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
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("Gemini HTTP client should build"),
            api_key: SecretString::new(api_key.into().into_boxed_str()),
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    async fn send_once(
        &self,
        req: &ModelRequest,
        ctx: &InferContext,
    ) -> Result<reqwest::Response, ModelError> {
        let method = if req.stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        let base_url = normalized_gemini_base_url(&self.base_url)?;
        let mut url = format!(
            "{}/{}/models/{}:{}",
            base_url, API_VERSION, req.model_id, method
        );
        if req.stream {
            url.push_str("?alt=sse");
        }
        let request = self
            .http
            .post(url)
            .headers(self.headers()?)
            .json(&request_body(req, ctx).await?);
        let response = request
            .send()
            .await
            .map_err(|error| ModelError::ProviderUnavailable(error.to_string()))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = read_error_body(response, self.api_key.expose_secret()).await;
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
        crate::catalog::provider_model_descriptors(PROVIDER_ID)
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        validate_request(&req, &ctx)?;
        let response = self.send_once(&req, &ctx).await?;
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

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::GenerateContent
    }

    fn prompt_cache_style(&self) -> PromptCacheStyle {
        PromptCacheStyle::Gemini {
            mode: GeminiCacheMode::ExternalReferenceOnly,
        }
    }
}

fn validate_request(req: &ModelRequest, ctx: &InferContext) -> Result<(), ModelError> {
    if req.protocol != ModelProtocol::GenerateContent {
        return Err(ModelError::InvalidRequest(
            "GeminiProvider only supports ModelProtocol::GenerateContent".to_owned(),
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

async fn request_body(req: &ModelRequest, ctx: &InferContext) -> Result<Value, ModelError> {
    let mut contents = Vec::new();
    for message in &req.messages {
        contents.push(content(message, ctx).await?);
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
    merge_gemini_extra(&mut body, &req.extra)?;
    Ok(body)
}

fn merge_gemini_extra(body: &mut Value, extra: &Value) -> Result<(), ModelError> {
    if extra.is_null() {
        return Ok(());
    }
    let extra = extra.as_object().ok_or_else(|| {
        ModelError::InvalidRequest("model request extra must be an object".to_owned())
    })?;
    for (key, value) in extra {
        match key.as_str() {
            "thinkingConfig" | "stopSequences" | "topP" | "topK" | "seed" | "responseMimeType"
            | "responseSchema" | "responseJsonSchema" => {
                body["generationConfig"][key] = value.clone();
            }
            "toolConfig" | "safetySettings" | "cachedContent" | "serviceTier" | "store" => {
                body[key] = value.clone();
            }
            "cached_content" => {
                if body.get("cachedContent").is_none() {
                    body["cachedContent"] = value.clone();
                }
            }
            _ => {
                body[key] = value.clone();
            }
        }
    }
    Ok(())
}

async fn content(message: &Message, ctx: &InferContext) -> Result<Value, ModelError> {
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
    let mut parts = Vec::new();
    for part in &message.parts {
        parts.push(message_part(part, ctx).await?);
    }
    Ok(json!({
        "role": role,
        "parts": parts,
    }))
}

async fn message_part(part: &MessagePart, ctx: &InferContext) -> Result<Value, ModelError> {
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
        MessagePart::Image {
            mime_type,
            blob_ref,
        }
        | MessagePart::Video {
            mime_type,
            blob_ref,
        }
        | MessagePart::File {
            mime_type,
            blob_ref,
        } => Ok(json!({
            "inlineData": {
                "mimeType": mime_type,
                "data": BASE64_STANDARD.encode(read_blob_bytes(ctx, blob_ref, mime_type).await?),
            },
        })),
        MessagePart::ProviderFileReference {
            provider_id,
            file_id,
            mime_type,
        } => {
            if provider_id != PROVIDER_ID {
                return Err(ModelError::InvalidRequest(
                    "Gemini provider file references must use provider_id gemini".to_owned(),
                ));
            }
            Ok(json!({
                "fileData": {
                    "mimeType": mime_type,
                    "fileUri": file_id,
                },
            }))
        }
        MessagePart::Thinking(_) => Err(ModelError::InvalidRequest(
            "GeminiProvider does not replay thinking message parts".to_owned(),
        )),
        _ => Err(ModelError::InvalidRequest(
            "unsupported Gemini message part".to_owned(),
        )),
    }
}

async fn read_blob_bytes(
    ctx: &InferContext,
    blob_ref: &BlobRef,
    mime_type: &str,
) -> Result<Vec<u8>, ModelError> {
    validate_inline_blob(blob_ref, mime_type)?;
    let store = ctx.blob_store.as_ref().ok_or_else(|| {
        ModelError::InvalidRequest("blob store is required for multimodal model input".to_owned())
    })?;
    read_blob_bytes_from_store(store.as_ref(), ctx.tenant_id, blob_ref).await
}

async fn read_blob_bytes_from_store(
    store: &dyn BlobStore,
    tenant_id: TenantId,
    blob_ref: &BlobRef,
) -> Result<Vec<u8>, ModelError> {
    let mut stream = store.get(tenant_id, blob_ref).await.map_err(|_| {
        ModelError::InvalidRequest("failed to read multimodal input blob".to_owned())
    })?;
    let initial_capacity =
        usize::try_from(blob_ref.size.min(MAX_GEMINI_INLINE_BLOB_BYTES)).unwrap_or(usize::MAX);
    let mut bytes = Vec::with_capacity(initial_capacity);
    while let Some(chunk) = stream.next().await {
        let next_len = u64::try_from(bytes.len())
            .unwrap_or(u64::MAX)
            .saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        if next_len > MAX_GEMINI_INLINE_BLOB_BYTES {
            return Err(ModelError::InvalidRequest(
                "Gemini inline multimodal input exceeds 20 MiB; upload it with Gemini Files API and reference fileData".to_owned(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn validate_inline_blob(blob_ref: &BlobRef, mime_type: &str) -> Result<(), ModelError> {
    if blob_ref.size > MAX_GEMINI_INLINE_BLOB_BYTES {
        return Err(ModelError::InvalidRequest(
            "Gemini inline multimodal input exceeds 20 MiB; upload it with Gemini Files API and reference fileData".to_owned(),
        ));
    }
    let mime_type = mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .to_ascii_lowercase();
    let allowed = matches!(
        mime_type.as_str(),
        "image/png"
            | "image/jpeg"
            | "image/webp"
            | "image/heic"
            | "image/heif"
            | "video/mp4"
            | "video/mpeg"
            | "video/mov"
            | "video/avi"
            | "video/x-flv"
            | "video/mpg"
            | "video/webm"
            | "video/wmv"
            | "video/3gpp"
            | "audio/wav"
            | "audio/mp3"
            | "audio/aiff"
            | "audio/aac"
            | "audio/ogg"
            | "audio/flac"
            | "application/pdf"
            | "text/plain"
            | "text/csv"
            | "text/html"
            | "application/json"
    );
    if !allowed {
        return Err(ModelError::InvalidRequest(
            "Gemini inline multimodal input MIME type is not supported".to_owned(),
        ));
    }
    Ok(())
}

fn normalized_gemini_base_url(value: &str) -> Result<String, ModelError> {
    let value = value.trim().trim_end_matches('/');
    let url = reqwest::Url::parse(value)
        .map_err(|_| ModelError::InvalidRequest("Gemini base URL is invalid".to_owned()))?;
    if url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ModelError::InvalidRequest(
            "Gemini base URL is invalid".to_owned(),
        ));
    }
    if !matches!(url.scheme(), "https" | "http") {
        return Err(ModelError::InvalidRequest(
            "Gemini base URL must use http(s)".to_owned(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| ModelError::InvalidRequest("Gemini base URL is invalid".to_owned()))?;
    let is_allowed_host = host.eq_ignore_ascii_case("generativelanguage.googleapis.com");
    #[cfg(debug_assertions)]
    let is_allowed_host = is_allowed_host
        || host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    if !is_allowed_host {
        return Err(ModelError::InvalidRequest(
            "Gemini base URL host is not allowed".to_owned(),
        ));
    }
    if url.scheme() == "http" {
        #[cfg(debug_assertions)]
        {
            let loopback = host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback());
            if loopback {
                return Ok(value.to_owned());
            }
        }
        return Err(ModelError::InvalidRequest(
            "Gemini base URL must use https".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

async fn read_error_body(mut response: reqwest::Response, secret: &str) -> String {
    let mut bytes = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        let next_len = u64::try_from(bytes.len())
            .unwrap_or(u64::MAX)
            .saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        if next_len > MAX_GEMINI_ERROR_BODY_BYTES {
            bytes.extend_from_slice(
                &chunk[..usize::try_from(
                    MAX_GEMINI_ERROR_BODY_BYTES.saturating_sub(bytes.len() as u64),
                )
                .unwrap_or(0)
                .min(chunk.len())],
            );
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    let body = String::from_utf8_lossy(&bytes).into_owned();
    let body = if secret.is_empty() {
        body
    } else {
        body.replace(secret, "[REDACTED]")
    };
    if body.trim().is_empty() {
        "Gemini API request failed".to_owned()
    } else {
        body
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
            "GeminiProvider does not inline blob tool results".to_owned(),
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
            "GeminiProvider does not inline blob tool result parts".to_owned(),
        )),
        ToolResultPart::Artifact { title, preview, .. } => Ok(json!({
            "text": preview
                .as_deref()
                .filter(|text| !text.is_empty())
                .unwrap_or(title.as_str()),
        })),
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

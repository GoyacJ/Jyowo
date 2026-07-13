use std::collections::BTreeMap;
use std::sync::Arc;

use async_stream::stream;
use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::{
    Message, MessagePart, MessageRole, ModelError, StopReason, ToolResult, UsageSnapshot,
};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::anthropic::{AnthropicAuthHeader, AnthropicClient};
use crate::openai_protocol::{OpenAiChatDialect, OpenAiProtocolClient, OpenAiProtocolProviderExt};
use crate::{
    apply_response_headers_middlewares, wrap_stream_with_cancel_deadline, ContentDelta,
    ContentType, CredentialValue, ErrorClass, ErrorHints, InferContext, ModelCredentialPickContext,
    ModelCredentialResolver, ModelDescriptor, ModelProtocol, ModelProvider, ModelRequest,
    ModelStream, ModelStreamEvent, PickedCredential, ThinkingDelta,
};

pub const DEFAULT_BASE_URL: &str = "https://dashscope-us.aliyuncs.com/compatible-mode/v1";
pub const LEGACY_BASE_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode";
pub const LEGACY_BASE_URL_V1: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";
const PROVIDER_ID: &str = "qwen";
pub const DASHSCOPE_API_KEY_ENV: &str = "DASHSCOPE_API_KEY";
pub const QWEN_API_KEY_ENV: &str = "QWEN_API_KEY";

#[derive(Clone)]
pub struct QwenProvider {
    chat_client: OpenAiProtocolClient,
    responses_client: OpenAiProtocolClient,
    messages_client: AnthropicClient,
    dashscope_client: DashScopeClient,
}

impl QwenProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        let api_key = api_key.into();
        Self {
            chat_client: qwen_chat_client(api_key.clone(), DEFAULT_BASE_URL),
            responses_client: qwen_responses_client(api_key.clone(), DEFAULT_BASE_URL),
            messages_client: qwen_messages_client(api_key.clone(), DEFAULT_BASE_URL),
            dashscope_client: DashScopeClient::from_api_key(api_key, DEFAULT_BASE_URL),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let base_url = base_url.into();
        self.chat_client = self
            .chat_client
            .with_base_url(qwen_openai_base_url(&base_url));
        self.responses_client = self
            .responses_client
            .with_base_url(qwen_openai_base_url(&base_url));
        self.messages_client = self
            .messages_client
            .with_base_url(qwen_messages_base_url(&base_url));
        self.dashscope_client = self
            .dashscope_client
            .with_base_url(qwen_dashscope_base_url(&base_url));
        self
    }

    #[must_use]
    pub fn with_credential_resolver(mut self, resolver: Arc<dyn ModelCredentialResolver>) -> Self {
        self.chat_client = self
            .chat_client
            .with_credential_resolver(Arc::clone(&resolver));
        self.responses_client = self
            .responses_client
            .with_credential_resolver(Arc::clone(&resolver));
        self.messages_client = self
            .messages_client
            .with_credential_resolver(Arc::clone(&resolver));
        self.dashscope_client = self.dashscope_client.with_credential_resolver(resolver);
        self
    }

    #[must_use]
    pub fn with_default_headers(mut self, headers: BTreeMap<String, String>) -> Self {
        self.chat_client = self.chat_client.with_extra_headers(headers.clone());
        self.responses_client = self.responses_client.with_extra_headers(headers.clone());
        self.messages_client = self.messages_client.with_extra_headers(headers.clone());
        self.dashscope_client = self.dashscope_client.with_extra_headers(headers);
        self
    }
}

impl OpenAiProtocolProviderExt for QwenProvider {
    fn client(&self) -> &OpenAiProtocolClient {
        &self.chat_client
    }
}

#[async_trait]
impl ModelProvider for QwenProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        crate::catalog::provider_model_descriptors(PROVIDER_ID)
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        match req.protocol {
            ModelProtocol::ChatCompletions => self.chat_client.infer(req, ctx).await,
            ModelProtocol::Responses => self.responses_client.infer(req, ctx).await,
            ModelProtocol::Messages => self.messages_client.infer(req, ctx).await,
            ModelProtocol::Dashscope => self.dashscope_client.infer(req, ctx).await,
            protocol => Err(ModelError::InvalidRequest(format!(
                "QwenProvider only supports chat_completions, responses, messages and dashscope, got {protocol:?}"
            ))),
        }
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::Responses
    }
}

fn qwen_chat_client(api_key: String, base_url: &str) -> OpenAiProtocolClient {
    OpenAiProtocolClient::from_api_key(api_key, base_url)
        .with_provider_id(PROVIDER_ID)
        .with_chat_dialect(OpenAiChatDialect::Qwen)
        .with_chat_completions_path("/chat/completions")
        .with_max_tokens_field("max_completion_tokens")
}

fn qwen_responses_client(api_key: String, base_url: &str) -> OpenAiProtocolClient {
    OpenAiProtocolClient::from_api_key(api_key, base_url)
        .with_provider_id(PROVIDER_ID)
        .with_responses_path("/responses")
}

fn qwen_messages_client(api_key: String, base_url: &str) -> AnthropicClient {
    AnthropicClient::from_api_key(api_key)
        .with_provider_id(PROVIDER_ID)
        .with_base_url(qwen_messages_base_url(base_url))
        .with_messages_path("/v1/messages")
        .with_auth_header(AnthropicAuthHeader::Bearer)
}

pub fn normalize_qwen_base_url(base_url: impl Into<String>) -> String {
    qwen_openai_base_url(&base_url.into())
}

fn qwen_openai_base_url(base_url: &str) -> String {
    let base_url = base_url.trim_end_matches('/').to_owned();
    if base_url == LEGACY_BASE_URL {
        LEGACY_BASE_URL_V1.to_owned()
    } else {
        base_url
    }
}

fn qwen_messages_base_url(base_url: &str) -> String {
    let base_url = qwen_openai_base_url(base_url);
    for suffix in [
        "/compatible-mode/v1",
        "/compatible-mode",
        "/api/v1",
        "/api",
        "/apps/anthropic/v1",
    ] {
        if let Some(root) = base_url.strip_suffix(suffix) {
            return format!("{root}/apps/anthropic");
        }
    }
    if base_url.ends_with("/apps/anthropic") {
        base_url
    } else {
        format!("{base_url}/apps/anthropic")
    }
}

fn qwen_dashscope_base_url(base_url: &str) -> String {
    let base_url = qwen_openai_base_url(base_url);
    for suffix in [
        "/compatible-mode/v1",
        "/compatible-mode",
        "/apps/anthropic/v1",
        "/apps/anthropic",
        "/api/v1",
        "/api",
    ] {
        if let Some(root) = base_url.strip_suffix(suffix) {
            return root.to_owned();
        }
    }
    base_url
}

#[derive(Clone)]
struct DashScopeClient {
    http: reqwest::Client,
    api_key: SecretString,
    credential_resolver: Option<Arc<dyn ModelCredentialResolver>>,
    base_url: String,
    extra_headers: BTreeMap<String, String>,
}

impl DashScopeClient {
    fn from_api_key(api_key: impl Into<String>, base_url: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: SecretString::new(api_key.into().into_boxed_str()),
            credential_resolver: None,
            base_url: qwen_dashscope_base_url(base_url),
            extra_headers: BTreeMap::new(),
        }
    }

    #[must_use]
    fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    #[must_use]
    fn with_credential_resolver(mut self, resolver: Arc<dyn ModelCredentialResolver>) -> Self {
        self.credential_resolver = Some(resolver);
        self
    }

    #[must_use]
    fn with_extra_headers(mut self, headers: BTreeMap<String, String>) -> Self {
        self.extra_headers = headers;
        self
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        validate_dashscope_request(&req)?;
        let body = dashscope_request_body(&req)?;
        if ctx.cancel.is_cancelled() {
            return Err(ModelError::Cancelled);
        }
        let credential = self.pick_credential(&req, &ctx).await?;
        let response = self
            .http
            .post(format!(
                "{}{}",
                self.base_url.trim_end_matches('/'),
                dashscope_path(&req.model_id)
            ))
            .headers(self.headers(credential.as_ref().map(|picked| &picked.value))?)
            .json(&body)
            .send()
            .await
            .map_err(|error| ModelError::ProviderUnavailable(error.to_string()))?;
        if !response.status().is_success() {
            return Err(ModelError::ProviderUnavailable(format!(
                "Qwen DashScope request failed with status {}",
                response.status()
            )));
        }
        let headers = response.headers().clone();
        apply_response_headers_middlewares(&headers, &ctx).await?;
        if req.stream {
            return Ok(wrap_stream_with_cancel_deadline(
                dashscope_response_to_stream(response),
                &ctx,
            ));
        }
        let value = response
            .json()
            .await
            .map_err(|error| ModelError::UnexpectedResponse(error.to_string()))?;
        dashscope_json_to_stream(value)
    }

    async fn pick_credential(
        &self,
        req: &ModelRequest,
        ctx: &InferContext,
    ) -> Result<Option<PickedCredential>, ModelError> {
        let Some(resolver) = &self.credential_resolver else {
            return Ok(None);
        };
        resolver
            .pick(ModelCredentialPickContext {
                tenant_id: ctx.tenant_id,
                provider_id: PROVIDER_ID.to_owned(),
                model_id: req.model_id.clone(),
            })
            .await
            .map(Some)
            .map_err(|error| error.into_model_error())
    }

    fn headers(&self, credential: Option<&CredentialValue>) -> Result<HeaderMap, ModelError> {
        let mut headers = HeaderMap::new();
        let api_key = credential
            .map(|credential| &credential.secret)
            .unwrap_or(&self.api_key);
        let value = format!("Bearer {}", api_key.expose_secret());
        let auth = HeaderValue::from_str(&value)
            .map_err(|error| ModelError::AuthExpired(error.to_string()))?;
        headers.insert(AUTHORIZATION, auth);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        for (name, value) in &self.extra_headers {
            let name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                ModelError::InvalidRequest(format!("invalid provider header name: {error}"))
            })?;
            let value = HeaderValue::from_str(value).map_err(|error| {
                ModelError::InvalidRequest(format!("invalid provider header value: {error}"))
            })?;
            headers.insert(name, value);
        }
        Ok(headers)
    }
}

fn validate_dashscope_request(req: &ModelRequest) -> Result<(), ModelError> {
    if req.protocol != ModelProtocol::Dashscope {
        return Err(ModelError::InvalidRequest(format!(
            "Qwen DashScope expected dashscope protocol, got {:?}",
            req.protocol
        )));
    }
    if !req.cache_breakpoints.is_empty() {
        return Err(ModelError::InvalidRequest(
            "Qwen DashScope does not accept explicit cache breakpoints".to_owned(),
        ));
    }
    Ok(())
}

fn dashscope_path(model_id: &str) -> &'static str {
    if is_qwen_multimodal_model(model_id) {
        "/api/v1/services/aigc/multimodal-generation/generation"
    } else {
        "/api/v1/services/aigc/text-generation/generation"
    }
}

fn is_qwen_multimodal_model(model_id: &str) -> bool {
    model_id.contains("-vl") || model_id.contains("omni")
}

fn dashscope_request_body(req: &ModelRequest) -> Result<Value, ModelError> {
    let messages = req
        .messages
        .iter()
        .map(dashscope_message)
        .collect::<Result<Vec<_>, _>>()?;
    let mut parameters = Map::new();
    parameters.insert("result_format".to_owned(), json!("message"));
    if req.stream {
        parameters.insert("stream".to_owned(), json!(true));
        parameters.insert("incremental_output".to_owned(), json!(true));
    }
    if let Some(max_tokens) = req.max_tokens {
        parameters.insert("max_tokens".to_owned(), json!(max_tokens));
    }
    if let Some(temperature) = req.temperature {
        parameters.insert("temperature".to_owned(), json!(temperature));
    }
    merge_dashscope_extra(&mut parameters, &req.extra)?;
    Ok(json!({
        "model": req.model_id,
        "input": {
            "messages": messages,
        },
        "parameters": parameters,
    }))
}

fn merge_dashscope_extra(
    parameters: &mut Map<String, Value>,
    extra: &Value,
) -> Result<(), ModelError> {
    if extra.is_null() {
        return Ok(());
    }
    let object = extra.as_object().ok_or_else(|| {
        ModelError::InvalidRequest("model request extra must be an object".to_owned())
    })?;
    for (key, value) in object {
        if key == "parameters" {
            let nested = value.as_object().ok_or_else(|| {
                ModelError::InvalidRequest(
                    "DashScope parameters extra must be an object".to_owned(),
                )
            })?;
            for (nested_key, nested_value) in nested {
                parameters.insert(nested_key.clone(), nested_value.clone());
            }
        } else {
            parameters.insert(key.clone(), value.clone());
        }
    }
    Ok(())
}

fn dashscope_message(message: &Message) -> Result<Value, ModelError> {
    let role = match message.role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
        MessageRole::Tool => "tool",
        _ => {
            return Err(ModelError::InvalidRequest(
                "unknown message role is not supported by Qwen DashScope".to_owned(),
            ));
        }
    };
    let mut text = String::new();
    for part in &message.parts {
        match part {
            MessagePart::Text(value) => text.push_str(value),
            MessagePart::ToolResult { content, .. } => {
                text.push_str(&tool_result_text(content));
            }
            MessagePart::ToolUse { name, input, .. } => {
                text.push_str(&json!({ "tool": name, "input": input }).to_string());
            }
            MessagePart::Thinking(_) => {}
            MessagePart::Image { .. }
            | MessagePart::Video { .. }
            | MessagePart::File { .. }
            | MessagePart::ProviderFileReference { .. } => {
                return Err(ModelError::InvalidRequest(
                    "Qwen DashScope does not inline local media message parts yet".to_owned(),
                ));
            }
            _ => {
                return Err(ModelError::InvalidRequest(
                    "unsupported Qwen DashScope message part".to_owned(),
                ));
            }
        }
    }
    Ok(json!({
        "role": role,
        "content": text,
    }))
}

fn tool_result_text(content: &ToolResult) -> String {
    match content {
        ToolResult::Text(text) => text.clone(),
        ToolResult::Structured(value) => value.to_string(),
        ToolResult::Mixed(parts) => parts
            .iter()
            .map(|part| format!("{part:?}"))
            .collect::<Vec<_>>()
            .join("\n"),
        ToolResult::Blob { .. } => "blob tool result".to_owned(),
        _ => "tool result".to_owned(),
    }
}

fn dashscope_response_to_stream(response: reqwest::Response) -> ModelStream {
    let mut bytes = response.bytes_stream();
    Box::pin(stream! {
        let mut parser = SseParser::default();
        let mut state = DashScopeStreamState::default();
        while let Some(chunk) = bytes.next().await {
            match chunk {
                Ok(chunk) => match parser.push(&chunk) {
                    Ok(events) => {
                        for event in events {
                            for mapped in state.map_event(&event) {
                                yield mapped;
                            }
                        }
                    }
                    Err(error) => {
                        yield stream_error(error, ErrorClass::Fatal);
                        return;
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
            for mapped in state.map_event(&event) {
                yield mapped;
            }
        }
        if state.terminal_seen {
            for event in state.finish() {
                yield event;
            }
        }
    })
}

fn dashscope_json_to_stream(value: Value) -> Result<ModelStream, ModelError> {
    let mut state = DashScopeStreamState::default();
    let mut events = state.map_payload(value)?;
    events.extend(state.finish());
    Ok(Box::pin(futures::stream::iter(events)))
}

#[derive(Default)]
struct SseParser {
    buffer: String,
}

impl SseParser {
    fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, ModelError> {
        let decoded = std::str::from_utf8(chunk)
            .map_err(|_| ModelError::UnexpectedResponse("invalid UTF-8 in SSE stream".to_owned()))?
            .replace("\r\n", "\n");
        self.buffer.push_str(&decoded);
        Ok(self.drain())
    }

    fn finish(&mut self) -> Vec<String> {
        let mut events = self.drain();
        if !self.buffer.trim().is_empty() {
            let frame = std::mem::take(&mut self.buffer);
            if let Some(data) = parse_sse_frame(&frame) {
                events.push(data);
            }
        }
        events
    }

    fn drain(&mut self) -> Vec<String> {
        let mut events = Vec::new();
        while let Some(end) = self.buffer.find("\n\n") {
            let frame = self.buffer[..end].to_owned();
            self.buffer.drain(..end + 2);
            if let Some(data) = parse_sse_frame(&frame) {
                events.push(data);
            }
        }
        events
    }
}

fn parse_sse_frame(frame: &str) -> Option<String> {
    let data = frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
        .collect::<Vec<_>>();
    (!data.is_empty()).then(|| data.join("\n"))
}

#[derive(Default)]
struct DashScopeStreamState {
    started: bool,
    stopped: bool,
    terminal_seen: bool,
    text_started: bool,
    thinking_started: bool,
}

impl DashScopeStreamState {
    fn map_event(&mut self, data: &str) -> Vec<ModelStreamEvent> {
        if data == "[DONE]" {
            return self.finish();
        }
        match serde_json::from_str::<Value>(data).map_err(|error| {
            ModelError::UnexpectedResponse(format!("invalid Qwen DashScope SSE payload: {error}"))
        }) {
            Ok(value) => self
                .map_payload(value)
                .unwrap_or_else(|error| vec![stream_error(error, ErrorClass::Fatal)]),
            Err(error) => vec![stream_error(error, ErrorClass::Fatal)],
        }
    }

    fn map_payload(&mut self, value: Value) -> Result<Vec<ModelStreamEvent>, ModelError> {
        let chunk: DashScopeChunk = serde_json::from_value(value)
            .map_err(|error| ModelError::UnexpectedResponse(error.to_string()))?;
        let mut events = Vec::new();
        if !self.started {
            self.started = true;
            events.push(ModelStreamEvent::MessageStart {
                message_id: String::new(),
                usage: UsageSnapshot::default(),
            });
        }
        for choice in chunk.output.choices {
            if let Some(content) = choice.message.content {
                if !content.is_empty() {
                    if !self.text_started {
                        self.text_started = true;
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
            if let Some(reasoning) = choice.message.reasoning_content {
                if !reasoning.is_empty() {
                    if !self.thinking_started {
                        self.thinking_started = true;
                        events.push(ModelStreamEvent::ContentBlockStart {
                            index: 1,
                            content_type: ContentType::Thinking,
                        });
                    }
                    events.push(ModelStreamEvent::ContentBlockDelta {
                        index: 1,
                        delta: ContentDelta::Thinking(ThinkingDelta {
                            text: Some(reasoning),
                            provider_native: None,
                            signature: None,
                        }),
                    });
                }
            }
            if let Some(reason) = choice.finish_reason {
                self.terminal_seen = true;
                events.push(ModelStreamEvent::MessageDelta {
                    stop_reason: Some(dashscope_stop_reason(&reason)),
                    usage_delta: usage_snapshot(chunk.usage.as_ref()),
                });
            }
        }
        Ok(events)
    }

    fn finish(&mut self) -> Vec<ModelStreamEvent> {
        if self.stopped || !self.started {
            return Vec::new();
        }
        self.stopped = true;
        let mut events = Vec::new();
        if self.text_started {
            events.push(ModelStreamEvent::ContentBlockStop { index: 0 });
        }
        if self.thinking_started {
            events.push(ModelStreamEvent::ContentBlockStop { index: 1 });
        }
        events.push(ModelStreamEvent::MessageStop);
        events
    }
}

fn stream_error(error: ModelError, class: ErrorClass) -> ModelStreamEvent {
    ModelStreamEvent::StreamError {
        error,
        class,
        hints: ErrorHints::default(),
    }
}

fn dashscope_stop_reason(reason: &str) -> StopReason {
    match reason {
        "stop" => StopReason::EndTurn,
        "tool_calls" => StopReason::ToolUse,
        "length" => StopReason::MaxIterations,
        value => StopReason::Error(value.to_owned()),
    }
}

fn usage_snapshot(usage: Option<&DashScopeUsage>) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens: usage
            .and_then(|usage| usage.input_tokens)
            .unwrap_or_default(),
        output_tokens: usage
            .and_then(|usage| usage.output_tokens)
            .unwrap_or_default(),
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        cost_micros: 0,
        tool_calls: 0,
    }
}

#[derive(Debug, Deserialize)]
struct DashScopeChunk {
    output: DashScopeOutput,
    usage: Option<DashScopeUsage>,
}

#[derive(Debug, Deserialize)]
struct DashScopeOutput {
    #[serde(default)]
    choices: Vec<DashScopeChoice>,
}

#[derive(Debug, Deserialize)]
struct DashScopeChoice {
    #[serde(default)]
    message: DashScopeMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct DashScopeMessage {
    content: Option<String>,
    reasoning_content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DashScopeUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

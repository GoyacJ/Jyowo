use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use futures::stream;
use futures::StreamExt;
use harness_contracts::{
    BlobRef, BlobStore, Message, MessagePart, MessageRole, ModelError, StopReason, TenantId,
    ThinkingBlock, ToolDescriptor, ToolResult, ToolResultPart, UsageSnapshot,
};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::{
    apply_response_headers_middlewares, wrap_stream_with_cancel_deadline, AnthropicCacheMode,
    Backoff, ContentDelta, ContentType, CredentialValue, ErrorClass, InferContext,
    ModelCredentialPickContext, ModelCredentialResolver, ModelDescriptor, ModelProtocol,
    ModelProvider, ModelRequest, ModelStream, ModelStreamEvent, PickedCredential, PromptCacheStyle,
};

use super::error::{map_response_error, map_transport_error, AnthropicError};
use super::{cache::apply_prompt_cache, streaming};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 1024;
const DEFAULT_CREDENTIAL_RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(60);
const PROVIDER_ID: &str = "anthropic";
const DEFAULT_MESSAGES_PATH: &str = "/v1/messages";

#[derive(Clone)]
pub struct AnthropicClient {
    http: reqwest::Client,
    api_key: SecretString,
    credential_resolver: Option<Arc<dyn ModelCredentialResolver>>,
    provider_id: String,
    base_url: String,
    messages_path: String,
    cooldown_until: Arc<Mutex<Option<Instant>>>,
}

impl AnthropicClient {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: SecretString::new(api_key.into().into_boxed_str()),
            credential_resolver: None,
            provider_id: PROVIDER_ID.to_owned(),
            base_url: DEFAULT_BASE_URL.to_owned(),
            messages_path: DEFAULT_MESSAGES_PATH.to_owned(),
            cooldown_until: Arc::new(Mutex::new(None)),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    #[must_use]
    pub fn with_credential_resolver(mut self, resolver: Arc<dyn ModelCredentialResolver>) -> Self {
        self.credential_resolver = Some(resolver);
        self
    }

    #[must_use]
    pub(crate) fn with_provider_id(mut self, provider_id: impl Into<String>) -> Self {
        self.provider_id = provider_id.into();
        self
    }

    #[must_use]
    pub(crate) fn with_messages_path(mut self, path: impl Into<String>) -> Self {
        self.messages_path = path.into();
        self
    }

    pub(crate) async fn infer(
        &self,
        req: ModelRequest,
        ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        validate_request(&req)?;
        let body = request_body(&req, &ctx).await?;
        let max_attempts = ctx.retry_policy.max_attempts.max(1);
        let mut attempt = 0;

        loop {
            if ctx.cancel.is_cancelled() {
                return Err(ModelError::Cancelled);
            }
            if let Some(deadline) = ctx.deadline {
                if Instant::now() >= deadline {
                    return Err(ModelError::DeadlineExceeded(Duration::ZERO));
                }
            }
            self.wait_for_cooldown().await;

            let credential = self.pick_credential(&req, &ctx).await?;
            let result = self
                .send_once(&body, credential.as_ref().map(|picked| &picked.value))
                .await;
            match result {
                Ok(response) => {
                    let headers = response.headers().clone();
                    apply_response_headers_middlewares(&headers, &ctx).await?;
                    if req.stream {
                        let stream = streaming::response_to_stream(response);
                        return Ok(wrap_stream_with_cancel_deadline(stream, &ctx));
                    }
                    let response = response
                        .json()
                        .await
                        .map_err(map_transport_error)
                        .map_err(|error| error.error)?;
                    return response_to_stream(response);
                }
                Err(err) => {
                    let is_rate_limited = matches!(err.class, ErrorClass::RateLimited { .. });
                    if is_rate_limited {
                        let cooldown = err
                            .retry_after
                            .unwrap_or(DEFAULT_CREDENTIAL_RATE_LIMIT_COOLDOWN);
                        if let (Some(resolver), Some(picked)) =
                            (self.credential_resolver.as_ref(), credential.as_ref())
                        {
                            resolver.mark_rate_limited(&picked.key, cooldown);
                        } else if let Some(retry_after) = err.retry_after {
                            self.set_cooldown(retry_after).await;
                        }
                    } else if let Some(retry_after) = err.retry_after {
                        self.set_cooldown(retry_after).await;
                    }
                    if matches!(err.class, ErrorClass::AuthExpired) {
                        if let (Some(resolver), Some(picked)) =
                            (self.credential_resolver.as_ref(), credential.as_ref())
                        {
                            resolver.mark_banned(&picked.key);
                        }
                        return Err(err.error);
                    }

                    attempt += 1;
                    let can_retry =
                        attempt < max_attempts && (ctx.retry_policy.retry_on)(&err.class);
                    if !can_retry {
                        return Err(err.error);
                    }

                    let delay = err
                        .retry_after
                        .unwrap_or_else(|| retry_delay(&ctx.retry_policy.backoff, attempt));
                    let credential_rate_limit_retried = is_rate_limited
                        && self.credential_resolver.is_some()
                        && credential.is_some();
                    if !delay.is_zero() && !credential_rate_limit_retried {
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }
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
                provider_id: self.provider_id.clone(),
                model_id: req.model_id.clone(),
            })
            .await
            .map(Some)
            .map_err(|error| error.into_model_error())
    }

    async fn send_once(
        &self,
        body: &Value,
        credential: Option<&CredentialValue>,
    ) -> Result<reqwest::Response, AnthropicError> {
        let response = self
            .http
            .post(format!(
                "{}{}",
                self.base_url.trim_end_matches('/'),
                normalized_path(&self.messages_path)
            ))
            .headers(self.headers(credential)?)
            .json(body)
            .send()
            .await
            .map_err(map_transport_error)?;

        if !response.status().is_success() {
            return Err(map_response_error(response).await);
        }

        Ok(response)
    }

    fn headers(&self, credential: Option<&CredentialValue>) -> Result<HeaderMap, AnthropicError> {
        let mut headers = HeaderMap::new();
        let api_key = credential
            .map(|credential| &credential.secret)
            .unwrap_or(&self.api_key);
        let api_key =
            HeaderValue::from_str(api_key.expose_secret()).map_err(|error| AnthropicError {
                error: ModelError::AuthExpired(error.to_string()),
                class: ErrorClass::AuthExpired,
                retry_after: None,
            })?;
        headers.insert("x-api-key", api_key);
        headers.insert("anthropic-version", HeaderValue::from_static(API_VERSION));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }

    async fn wait_for_cooldown(&self) {
        let cooldown_until = *self.cooldown_until.lock().await;
        let delay = cooldown_until.and_then(|until| until.checked_duration_since(Instant::now()));
        if let Some(delay) = delay {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
        }
    }

    async fn set_cooldown(&self, delay: Duration) {
        *self.cooldown_until.lock().await = Some(Instant::now() + delay);
    }
}

#[derive(Clone)]
pub struct AnthropicProvider {
    client: AnthropicClient,
}

impl AnthropicProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: AnthropicClient::from_api_key(api_key),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.client = self.client.with_base_url(base_url);
        self
    }

    #[must_use]
    pub fn with_credential_resolver(mut self, resolver: Arc<dyn ModelCredentialResolver>) -> Self {
        self.client = self.client.with_credential_resolver(resolver);
        self
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        crate::catalog::provider_model_descriptors(PROVIDER_ID)
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        self.client.infer(req, ctx).await
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::Messages
    }

    fn prompt_cache_style(&self) -> PromptCacheStyle {
        PromptCacheStyle::Anthropic {
            mode: AnthropicCacheMode::SystemAnd3,
        }
    }
}

fn validate_request(req: &ModelRequest) -> Result<(), ModelError> {
    if req.protocol != ModelProtocol::Messages {
        return Err(ModelError::InvalidRequest(
            "Anthropic protocol client only supports ModelProtocol::Messages".to_owned(),
        ));
    }
    Ok(())
}

fn normalized_path(path: &str) -> String {
    let path = path.trim();
    if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    }
}

async fn request_body(req: &ModelRequest, ctx: &InferContext) -> Result<Value, ModelError> {
    let messages = req
        .messages
        .iter()
        .map(|message| anthropic_message(message, ctx))
        .collect::<futures::stream::FuturesOrdered<_>>()
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let tools = req
        .tools
        .as_ref()
        .map(|tools| tools.iter().map(anthropic_tool).collect::<Vec<_>>());

    let mut body = json!({
        "model": req.model_id,
        "messages": messages,
        "max_tokens": req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "stream": req.stream,
    });

    if let Some(system) = &req.system {
        body["system"] = Value::String(system.clone());
    }
    if let Some(temperature) = req.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(tools) = tools {
        body["tools"] = Value::Array(tools);
    }

    apply_prompt_cache(&mut body, req)?;
    merge_extra_object(&mut body, &req.extra)?;
    Ok(body)
}

fn merge_extra_object(body: &mut Value, extra: &Value) -> Result<(), ModelError> {
    if extra.is_null() {
        return Ok(());
    }
    let extra = extra.as_object().ok_or_else(|| {
        ModelError::InvalidRequest("model request extra must be an object".to_owned())
    })?;
    for (key, value) in extra {
        body[key] = value.clone();
    }
    Ok(())
}

async fn anthropic_message(message: &Message, ctx: &InferContext) -> Result<Value, ModelError> {
    let role = match message.role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => {
            return Err(ModelError::InvalidRequest(
                "system messages must use ModelRequest.system for Anthropic".to_owned(),
            ));
        }
        MessageRole::Tool => "user",
        _ => {
            return Err(ModelError::InvalidRequest(
                "unknown message role is not supported by Anthropic".to_owned(),
            ));
        }
    };
    let content = if message.role == MessageRole::Tool {
        message
            .parts
            .iter()
            .map(|part| anthropic_tool_result_part(part, ctx))
            .collect::<futures::stream::FuturesOrdered<_>>()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?
    } else {
        message
            .parts
            .iter()
            .map(|part| anthropic_part(part, ctx))
            .collect::<futures::stream::FuturesOrdered<_>>()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(json!({
        "role": role,
        "content": content,
    }))
}

async fn anthropic_part(part: &MessagePart, ctx: &InferContext) -> Result<Value, ModelError> {
    match part {
        MessagePart::Text(text) => Ok(json!({ "type": "text", "text": text })),
        MessagePart::ToolUse { id, name, input } => Ok(json!({
            "type": "tool_use",
            "id": id.to_string(),
            "name": name,
            "input": input,
        })),
        MessagePart::Thinking(thinking) => anthropic_thinking_part(thinking),
        MessagePart::ToolResult { .. } => Err(ModelError::InvalidRequest(
            "Anthropic tool_result blocks must use MessageRole::Tool".to_owned(),
        )),
        MessagePart::Image {
            mime_type,
            blob_ref,
        } => anthropic_blob_part(ctx, "image", mime_type, blob_ref).await,
        MessagePart::File {
            mime_type,
            blob_ref,
        } => anthropic_blob_part(ctx, "document", mime_type, blob_ref).await,
        MessagePart::ProviderFileReference {
            provider_id,
            file_id,
            mime_type,
        } => anthropic_provider_file_part(provider_id, file_id, mime_type),
        MessagePart::Video { .. } => Err(ModelError::InvalidRequest(
            "AnthropicProvider does not support video message parts".to_owned(),
        )),
        _ => Err(ModelError::InvalidRequest(
            "unsupported Anthropic message part".to_owned(),
        )),
    }
}

async fn anthropic_tool_result_part(
    part: &MessagePart,
    ctx: &InferContext,
) -> Result<Value, ModelError> {
    match part {
        MessagePart::ToolResult {
            tool_use_id,
            content,
        } => Ok(json!({
            "type": "tool_result",
            "tool_use_id": tool_use_id.to_string(),
            "content": anthropic_tool_result_content(content, ctx).await?,
        })),
        _ => Err(ModelError::InvalidRequest(
            "Anthropic tool messages may only contain tool_result parts".to_owned(),
        )),
    }
}

fn anthropic_thinking_part(thinking: &ThinkingBlock) -> Result<Value, ModelError> {
    if thinking.provider_id != PROVIDER_ID {
        return Err(ModelError::InvalidRequest(
            "AnthropicProvider can only replay Anthropic thinking blocks".to_owned(),
        ));
    }
    if let Some(native) = &thinking.provider_native {
        if native
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "thinking" || kind == "redacted_thinking")
        {
            return Ok(native.clone());
        }
    }
    let text = thinking.text.as_deref().ok_or_else(|| {
        ModelError::InvalidRequest(
            "Anthropic thinking block requires text or provider_native".to_owned(),
        )
    })?;
    let mut value = json!({
        "type": "thinking",
        "thinking": text,
    });
    if let Some(signature) = &thinking.signature {
        value["signature"] = Value::String(signature.clone());
    }
    Ok(value)
}

async fn anthropic_blob_part(
    ctx: &InferContext,
    kind: &str,
    mime_type: &str,
    blob_ref: &BlobRef,
) -> Result<Value, ModelError> {
    let bytes = read_blob_bytes(ctx, blob_ref).await?;
    let source = json!({
        "type": "base64",
        "media_type": mime_type,
        "data": BASE64_STANDARD.encode(bytes),
    });
    Ok(json!({
        "type": kind,
        "source": source,
    }))
}

fn anthropic_provider_file_part(
    provider_id: &str,
    file_id: &str,
    mime_type: &str,
) -> Result<Value, ModelError> {
    if provider_id != PROVIDER_ID {
        return Err(ModelError::InvalidRequest(format!(
            "AnthropicProvider cannot use {provider_id} provider file references"
        )));
    }
    let block_type = if mime_type.starts_with("image/") {
        "image"
    } else if mime_type == "application/x-container-upload" {
        "container_upload"
    } else {
        "document"
    };
    if block_type == "container_upload" {
        return Ok(json!({
            "type": "container_upload",
            "file_id": file_id,
        }));
    }
    Ok(json!({
        "type": block_type,
        "source": {
            "type": "file",
            "file_id": file_id,
        },
    }))
}

async fn read_blob_bytes(ctx: &InferContext, blob_ref: &BlobRef) -> Result<Vec<u8>, ModelError> {
    let store = ctx.blob_store.as_ref().ok_or_else(|| {
        ModelError::InvalidRequest(
            "blob store is required for Anthropic multimodal input".to_owned(),
        )
    })?;
    read_blob_bytes_from_store(store.as_ref(), ctx.tenant_id, blob_ref).await
}

async fn read_blob_bytes_from_store(
    store: &dyn BlobStore,
    tenant_id: TenantId,
    blob_ref: &BlobRef,
) -> Result<Vec<u8>, ModelError> {
    let mut stream = store.get(tenant_id, blob_ref).await.map_err(|error| {
        ModelError::InvalidRequest(format!("failed to read Anthropic input blob: {error}"))
    })?;
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn anthropic_tool_result_content(
    content: &ToolResult,
    ctx: &InferContext,
) -> Result<Value, ModelError> {
    match content {
        ToolResult::Text(text) => Ok(json!([{ "type": "text", "text": text }])),
        ToolResult::Structured(value) => Ok(json!([{ "type": "text", "text": value.to_string() }])),
        ToolResult::Mixed(parts) => Ok(Value::Array(
            futures::stream::iter(parts)
                .then(|part| anthropic_tool_result_content_part(part, ctx))
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?,
        )),
        ToolResult::Blob {
            content_type,
            blob_ref,
        } => Ok(json!([anthropic_blob_part(
            ctx,
            "document",
            content_type,
            blob_ref
        )
        .await?])),
        _ => Err(ModelError::InvalidRequest(
            "unsupported Anthropic tool result".to_owned(),
        )),
    }
}

async fn anthropic_tool_result_content_part(
    part: &ToolResultPart,
    ctx: &InferContext,
) -> Result<Value, ModelError> {
    match part {
        ToolResultPart::Text { text } => Ok(json!({ "type": "text", "text": text })),
        ToolResultPart::Structured { value, .. } => {
            Ok(json!({ "type": "text", "text": value.to_string() }))
        }
        ToolResultPart::Code { language, text } => Ok(json!({
            "type": "text",
            "text": format!("```{language}\n{text}\n```"),
        })),
        ToolResultPart::Reference { title, summary, .. } => Ok(json!({
            "type": "text",
            "text": summary
                .as_deref()
                .or(title.as_deref())
                .unwrap_or("reference"),
        })),
        ToolResultPart::Table {
            headers,
            rows,
            caption,
        } => Ok(json!({
            "type": "text",
            "text": json!({
                "caption": caption,
                "headers": headers,
                "rows": rows,
            })
            .to_string(),
        })),
        ToolResultPart::Progress {
            stage,
            ratio,
            detail,
        } => Ok(json!({
            "type": "text",
            "text": json!({
                "stage": stage,
                "ratio": ratio,
                "detail": detail,
            })
            .to_string(),
        })),
        ToolResultPart::Error {
            code,
            message,
            retriable,
        } => Ok(json!({
            "type": "text",
            "text": json!({
                "code": code,
                "message": message,
                "retriable": retriable,
            })
            .to_string(),
        })),
        ToolResultPart::Blob {
            content_type,
            blob_ref,
            ..
        } => anthropic_blob_part(ctx, "document", content_type, blob_ref).await,
        ToolResultPart::Artifact { title, preview, .. } => Ok(json!({
            "type": "text",
            "text": preview
                .as_deref()
                .filter(|text| !text.is_empty())
                .unwrap_or(title.as_str()),
        })),
        _ => Err(ModelError::InvalidRequest(
            "unsupported Anthropic tool result part".to_owned(),
        )),
    }
}

fn anthropic_tool(tool: &ToolDescriptor) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "input_schema": tool.input_schema,
    })
}

fn response_to_stream(response: AnthropicResponse) -> Result<ModelStream, ModelError> {
    let usage = usage(response.usage);
    let mut events = vec![ModelStreamEvent::MessageStart {
        message_id: response.id,
        usage: usage.clone(),
    }];

    for (index, part) in response.content.into_iter().enumerate() {
        match part {
            AnthropicContent::Text { text } => {
                let index = index as u32;
                events.push(ModelStreamEvent::ContentBlockStart {
                    index,
                    content_type: ContentType::Text,
                });
                events.push(ModelStreamEvent::ContentBlockDelta {
                    index,
                    delta: ContentDelta::Text(text),
                });
                events.push(ModelStreamEvent::ContentBlockStop { index });
            }
            AnthropicContent::ToolUse { id, name, input } => {
                let index = index as u32;
                events.push(ModelStreamEvent::ContentBlockStart {
                    index,
                    content_type: ContentType::ToolUse,
                });
                events.push(ModelStreamEvent::ContentBlockDelta {
                    index,
                    delta: ContentDelta::ToolUseStart { id, name },
                });
                events.push(ModelStreamEvent::ContentBlockDelta {
                    index,
                    delta: ContentDelta::ToolUseInputJson(input.to_string()),
                });
                events.push(ModelStreamEvent::ContentBlockStop { index });
            }
            AnthropicContent::Thinking {
                thinking,
                signature,
            } => {
                let index = index as u32;
                let provider_native = json!({
                    "type": "thinking",
                    "thinking": thinking,
                    "signature": signature,
                });
                events.push(ModelStreamEvent::ContentBlockStart {
                    index,
                    content_type: ContentType::Thinking,
                });
                events.push(ModelStreamEvent::ContentBlockDelta {
                    index,
                    delta: ContentDelta::Thinking(crate::ThinkingDelta {
                        text: Some(thinking),
                        provider_native: Some(provider_native),
                        signature,
                    }),
                });
                events.push(ModelStreamEvent::ContentBlockStop { index });
            }
            AnthropicContent::RedactedThinking { data } => {
                let index = index as u32;
                events.push(ModelStreamEvent::ContentBlockStart {
                    index,
                    content_type: ContentType::Thinking,
                });
                events.push(ModelStreamEvent::ContentBlockDelta {
                    index,
                    delta: ContentDelta::Thinking(crate::ThinkingDelta {
                        text: None,
                        provider_native: Some(json!({
                            "type": "redacted_thinking",
                            "data": data,
                        })),
                        signature: None,
                    }),
                });
                events.push(ModelStreamEvent::ContentBlockStop { index });
            }
            AnthropicContent::Other => {}
        }
    }

    events.push(ModelStreamEvent::MessageDelta {
        stop_reason: response.stop_reason.as_deref().map(stop_reason),
        usage_delta: usage,
    });
    events.push(ModelStreamEvent::MessageStop);
    Ok(Box::pin(stream::iter(events)))
}

fn usage(usage: AnthropicUsage) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens: usage.input.unwrap_or_default(),
        output_tokens: usage.output.unwrap_or_default(),
        cache_read_tokens: usage.cache_read_input.unwrap_or_default(),
        cache_write_tokens: usage.cache_creation_input.unwrap_or_default(),
        cost_micros: 0,
        tool_calls: 0,
    }
}

fn stop_reason(reason: &str) -> StopReason {
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

fn retry_delay(backoff: &Backoff, attempt: u32) -> Duration {
    match backoff {
        Backoff::Fixed(delay) => *delay,
        Backoff::Exponential {
            initial,
            factor,
            cap,
        } => {
            let multiplier = factor.powi(attempt.saturating_sub(1) as i32);
            initial.mul_f32(multiplier).min(*cap)
        }
    }
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    id: String,
    #[serde(default)]
    content: Vec<AnthropicContent>,
    stop_reason: Option<String>,
    #[serde(default)]
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContent {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: Value,
    },
    Thinking {
        thinking: String,
        signature: Option<String>,
    },
    RedactedThinking {
        data: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Default, Deserialize)]
struct AnthropicUsage {
    #[serde(rename = "input_tokens")]
    input: Option<u64>,
    #[serde(rename = "output_tokens")]
    output: Option<u64>,
    #[serde(rename = "cache_creation_input_tokens")]
    cache_creation_input: Option<u64>,
    #[serde(rename = "cache_read_input_tokens")]
    cache_read_input: Option<u64>,
}

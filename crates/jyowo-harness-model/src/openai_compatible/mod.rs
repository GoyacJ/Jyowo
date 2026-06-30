mod error;
mod responses;
mod streaming;

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use futures::{stream, StreamExt};
use harness_contracts::{
    BlobRef, BlobStore, Message, MessagePart, MessageRole, ModelError, StopReason, TenantId,
    ToolDescriptor, ToolResult, ToolResultPart, UsageSnapshot,
};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{Mutex, Semaphore};

use crate::{
    apply_response_headers_middlewares, wrap_stream_with_cancel_deadline, Backoff, ContentDelta,
    ContentType, CredentialValue, ErrorClass, InferContext, ModelCredentialPickContext,
    ModelCredentialResolver, ModelProtocol, ModelRequest, ModelStream, ModelStreamEvent,
    PickedCredential,
};

use self::error::{map_response_error, map_transport_error, OpenAiCompatibleError};

const DEFAULT_MAX_TOKENS: u32 = 1024;
const DEFAULT_CREDENTIAL_RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub(crate) struct OpenAiCompatibleClient {
    http: reqwest::Client,
    api_key: Option<SecretString>,
    credential_resolver: Option<Arc<dyn ModelCredentialResolver>>,
    provider_id: String,
    base_url: String,
    path: String,
    protocol: ModelProtocol,
    max_tokens_field: &'static str,
    cooldown_until: Arc<Mutex<Option<Instant>>>,
    concurrency: Option<Arc<Semaphore>>,
}

#[allow(dead_code)]
impl OpenAiCompatibleClient {
    pub(crate) fn from_api_key(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self::new(
            Some(api_key.into()),
            base_url,
            ModelProtocol::ChatCompletions,
            "/v1/chat/completions",
        )
    }

    pub(crate) fn without_api_key(base_url: impl Into<String>) -> Self {
        Self::new(
            None,
            base_url,
            ModelProtocol::ChatCompletions,
            "/v1/chat/completions",
        )
    }

    fn new(
        api_key: Option<String>,
        base_url: impl Into<String>,
        protocol: ModelProtocol,
        path: impl Into<String>,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: api_key.map(|api_key| SecretString::new(api_key.into_boxed_str())),
            credential_resolver: None,
            provider_id: "openai-compatible".to_owned(),
            base_url: base_url.into(),
            path: path.into(),
            protocol,
            max_tokens_field: "max_tokens",
            cooldown_until: Arc::new(Mutex::new(None)),
            concurrency: None,
        }
    }

    #[must_use]
    pub(crate) fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    #[must_use]
    pub(crate) fn with_chat_completions_path(mut self, path: impl Into<String>) -> Self {
        self.protocol = ModelProtocol::ChatCompletions;
        self.path = path.into();
        self
    }

    #[must_use]
    pub(crate) fn with_responses_path(mut self, path: impl Into<String>) -> Self {
        self.protocol = ModelProtocol::Responses;
        self.path = path.into();
        self
    }

    #[must_use]
    pub(crate) fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(SecretString::new(api_key.into().into_boxed_str()));
        self
    }

    #[must_use]
    pub(crate) fn with_provider_id(mut self, provider_id: impl Into<String>) -> Self {
        self.provider_id = provider_id.into();
        self
    }

    #[must_use]
    pub(crate) fn with_max_tokens_field(mut self, field: &'static str) -> Self {
        self.max_tokens_field = field;
        self
    }

    #[must_use]
    pub(crate) fn with_credential_resolver(
        mut self,
        resolver: Arc<dyn ModelCredentialResolver>,
    ) -> Self {
        self.credential_resolver = Some(resolver);
        self
    }

    #[must_use]
    pub(crate) fn with_timeout(mut self, timeout: Duration) -> Self {
        self.http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        self
    }

    #[must_use]
    pub(crate) fn with_max_concurrency(mut self, max_concurrency: usize) -> Self {
        self.concurrency = (max_concurrency > 0).then(|| Arc::new(Semaphore::new(max_concurrency)));
        self
    }

    pub(crate) async fn infer(
        &self,
        req: ModelRequest,
        ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.validate_request(&req)?;
        let body = self.request_body(&req, &ctx).await?;
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
                        let stream = match self.protocol {
                            ModelProtocol::ChatCompletions => {
                                streaming::response_to_stream(response)
                            }
                            ModelProtocol::Responses => responses::response_to_stream(response),
                            _ => unreachable!("validated OpenAI-compatible API mode"),
                        };
                        return Ok(wrap_stream_with_cancel_deadline(stream, &ctx));
                    }
                    let response = response
                        .json()
                        .await
                        .map_err(map_transport_error)
                        .map_err(|error| error.error)?;
                    return match self.protocol {
                        ModelProtocol::ChatCompletions => chat_response_to_stream(response),
                        ModelProtocol::Responses => responses::json_response_to_stream(response),
                        _ => unreachable!("validated OpenAI-compatible API mode"),
                    };
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
    ) -> Result<reqwest::Response, OpenAiCompatibleError> {
        let _permit = match &self.concurrency {
            Some(semaphore) => Some(semaphore.clone().acquire_owned().await.map_err(|error| {
                OpenAiCompatibleError {
                    error: ModelError::ProviderUnavailable(error.to_string()),
                    class: ErrorClass::Transient,
                    retry_after: None,
                }
            })?),
            None => None,
        };
        let response = self
            .http
            .post(format!(
                "{}{}",
                self.base_url.trim_end_matches('/'),
                normalize_path(&self.path)
            ))
            .headers(self.headers(credential)?)
            .json(body)
            .send()
            .await
            .map_err(map_transport_error)?;

        if !response.status().is_success() {
            let credential_secret = credential
                .map(|credential| credential.secret.expose_secret())
                .or_else(|| self.api_key.as_ref().map(|api_key| api_key.expose_secret()));
            return Err(map_response_error(response, credential_secret).await);
        }

        Ok(response)
    }

    fn headers(
        &self,
        credential: Option<&CredentialValue>,
    ) -> Result<HeaderMap, OpenAiCompatibleError> {
        let mut headers = HeaderMap::new();
        let api_key = credential
            .map(|credential| &credential.secret)
            .or(self.api_key.as_ref());
        if let Some(api_key) = api_key {
            let value = format!("Bearer {}", api_key.expose_secret());
            let auth = HeaderValue::from_str(&value).map_err(|error| OpenAiCompatibleError {
                error: ModelError::AuthExpired(error.to_string()),
                class: ErrorClass::AuthExpired,
                retry_after: None,
            })?;
            headers.insert(AUTHORIZATION, auth);
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }

    fn validate_request(&self, req: &ModelRequest) -> Result<(), ModelError> {
        if req.protocol != self.protocol {
            return Err(ModelError::InvalidRequest(format!(
                "OpenAI-compatible provider expected {:?}, got {:?}",
                self.protocol, req.protocol
            )));
        }
        if !req.cache_breakpoints.is_empty() {
            return Err(ModelError::InvalidRequest(
                "OpenAI-compatible providers do not accept explicit cache breakpoints".to_owned(),
            ));
        }
        Ok(())
    }

    async fn request_body(
        &self,
        req: &ModelRequest,
        ctx: &InferContext,
    ) -> Result<Value, ModelError> {
        match self.protocol {
            ModelProtocol::ChatCompletions => {
                chat_request_body(req, self.max_tokens_field, ctx).await
            }
            ModelProtocol::Responses => responses_request_body(req, ctx).await,
            _ => Err(ModelError::InvalidRequest(
                "unsupported OpenAI-compatible API mode".to_owned(),
            )),
        }
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

fn normalize_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    }
}

#[async_trait]
pub(crate) trait OpenAiCompatibleProviderExt: Send + Sync + 'static {
    fn client(&self) -> &OpenAiCompatibleClient;

    async fn infer_openai_compatible(
        &self,
        req: ModelRequest,
        ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.client().infer(req, ctx).await
    }
}

async fn chat_request_body(
    req: &ModelRequest,
    max_tokens_field: &str,
    ctx: &InferContext,
) -> Result<Value, ModelError> {
    let mut messages = Vec::new();
    if let Some(system) = &req.system {
        messages.push(json!({
            "role": "system",
            "content": system,
        }));
    }
    for message in &req.messages {
        messages.push(chat_message(message, ctx).await?);
    }

    let mut body = json!({
        "model": req.model_id,
        "messages": messages,
        "stream": req.stream,
    });
    body[max_tokens_field] = json!(req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS));

    if req.stream {
        body["stream_options"] = json!({ "include_usage": true });
    }
    if let Some(temperature) = req.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(tools) = &req.tools {
        body["tools"] = Value::Array(tools.iter().map(openai_tool).collect());
    }
    merge_extra_object(&mut body, &req.extra)?;

    Ok(body)
}

async fn responses_request_body(
    req: &ModelRequest,
    ctx: &InferContext,
) -> Result<Value, ModelError> {
    let mut input = Vec::new();
    if let Some(system) = &req.system {
        input.push(json!({
            "role": "system",
            "content": system,
        }));
    }
    for message in &req.messages {
        input.push(chat_message(message, ctx).await?);
    }

    let mut body = json!({
        "model": req.model_id,
        "input": input,
        "max_output_tokens": req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "stream": req.stream,
    });

    if let Some(temperature) = req.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(tools) = &req.tools {
        body["tools"] = Value::Array(tools.iter().map(responses_tool).collect());
    }
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

async fn chat_message(message: &Message, ctx: &InferContext) -> Result<Value, ModelError> {
    match message.role {
        MessageRole::System => Ok(json!({
            "role": "system",
            "content": text_content(&message.parts, ctx).await?,
        })),
        MessageRole::User => Ok(json!({
            "role": "user",
            "content": text_content(&message.parts, ctx).await?,
        })),
        MessageRole::Assistant => assistant_message(&message.parts),
        MessageRole::Tool => tool_message(&message.parts),
        _ => Err(ModelError::InvalidRequest(
            "unknown message role is not supported by OpenAI-compatible providers".to_owned(),
        )),
    }
}

fn assistant_message(parts: &[MessagePart]) -> Result<Value, ModelError> {
    let mut text = Vec::new();
    let mut tool_calls = Vec::new();

    for part in parts {
        match part {
            MessagePart::Text(value) => text.push(value.clone()),
            MessagePart::ToolUse { id, name, input } => tool_calls.push(json!({
                "id": id.to_string(),
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": input.to_string(),
                },
            })),
            MessagePart::Image { .. }
            | MessagePart::Video { .. }
            | MessagePart::File { .. }
            | MessagePart::Thinking(_)
            | MessagePart::ToolResult { .. } => {
                return Err(ModelError::InvalidRequest(
                    "assistant messages only support text and tool use parts for OpenAI-compatible providers"
                        .to_owned(),
                ));
            }
            _ => {
                return Err(ModelError::InvalidRequest(
                    "unsupported assistant message part for OpenAI-compatible providers".to_owned(),
                ));
            }
        }
    }

    let mut message = json!({
        "role": "assistant",
        "content": if text.is_empty() {
            Value::Null
        } else {
            Value::String(text.join(""))
        },
    });
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    Ok(message)
}

fn tool_message(parts: &[MessagePart]) -> Result<Value, ModelError> {
    let [MessagePart::ToolResult {
        tool_use_id,
        content,
    }] = parts
    else {
        return Err(ModelError::InvalidRequest(
            "tool messages must contain exactly one tool result part for OpenAI-compatible providers"
                .to_owned(),
        ));
    };

    Ok(json!({
        "role": "tool",
        "tool_call_id": tool_use_id.to_string(),
        "content": tool_result_content(content)?,
    }))
}

async fn text_content(parts: &[MessagePart], ctx: &InferContext) -> Result<Value, ModelError> {
    if parts
        .iter()
        .all(|part| matches!(part, MessagePart::Text(_)))
    {
        let mut text = String::new();
        for part in parts {
            if let MessagePart::Text(value) = part {
                text.push_str(value);
            }
        }
        return Ok(Value::String(text));
    }
    content_parts(parts, ctx).await
}

async fn content_parts(parts: &[MessagePart], ctx: &InferContext) -> Result<Value, ModelError> {
    let mut content = Vec::new();
    for part in parts {
        match part {
            MessagePart::Text(value) => content.push(json!({
                "type": "text",
                "text": value,
            })),
            MessagePart::Image {
                mime_type,
                blob_ref,
            } => content.push(json!({
                "type": "image_url",
                "image_url": {
                    "url": blob_data_url(ctx, mime_type, blob_ref).await?
                },
            })),
            MessagePart::Video {
                mime_type,
                blob_ref,
            } => content.push(json!({
                "type": "video_url",
                "video_url": {
                    "url": blob_data_url(ctx, mime_type, blob_ref).await?
                },
            })),
            MessagePart::File { .. } => {
                return Err(ModelError::InvalidRequest(
                    "file message parts require provider-specific upload support for OpenAI-compatible providers"
                        .to_owned(),
                ));
            }
            MessagePart::Thinking(_) => {
                return Err(ModelError::InvalidRequest(
                    "thinking message parts are not supported by OpenAI-compatible providers"
                        .to_owned(),
                ));
            }
            MessagePart::ToolUse { .. } | MessagePart::ToolResult { .. } => {
                return Err(ModelError::InvalidRequest(
                    "tool message parts must use assistant/tool roles for OpenAI-compatible providers"
                        .to_owned(),
                ));
            }
            _ => {
                return Err(ModelError::InvalidRequest(
                    "unsupported message part for OpenAI-compatible providers".to_owned(),
                ));
            }
        }
    }
    Ok(Value::Array(content))
}

async fn blob_data_url(
    ctx: &InferContext,
    mime_type: &str,
    blob_ref: &BlobRef,
) -> Result<String, ModelError> {
    let store = ctx.blob_store.as_ref().ok_or_else(|| {
        ModelError::InvalidRequest("blob store is required for multimodal model input".to_owned())
    })?;
    let bytes = read_blob_bytes(store.as_ref(), ctx.tenant_id, blob_ref).await?;
    Ok(format!(
        "data:{};base64,{}",
        mime_type,
        BASE64_STANDARD.encode(bytes)
    ))
}

async fn read_blob_bytes(
    store: &dyn BlobStore,
    tenant_id: TenantId,
    blob_ref: &BlobRef,
) -> Result<Vec<u8>, ModelError> {
    let mut stream = store.get(tenant_id, blob_ref).await.map_err(|_| {
        ModelError::InvalidRequest("failed to read multimodal input blob".to_owned())
    })?;
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn tool_result_content(content: &ToolResult) -> Result<String, ModelError> {
    match content {
        ToolResult::Text(text) => Ok(text.clone()),
        ToolResult::Structured(value) => Ok(value.to_string()),
        ToolResult::Blob { .. } => Err(ModelError::InvalidRequest(
            "blob tool results are not supported by OpenAI-compatible providers".to_owned(),
        )),
        ToolResult::Mixed(parts) => parts
            .iter()
            .map(tool_result_part_content)
            .collect::<Result<Vec<_>, _>>()
            .map(|parts| parts.join("")),
        _ => Err(ModelError::InvalidRequest(
            "unsupported tool result for OpenAI-compatible providers".to_owned(),
        )),
    }
}

fn tool_result_part_content(part: &ToolResultPart) -> Result<String, ModelError> {
    match part {
        ToolResultPart::Structured { value, .. } => Ok(value.to_string()),
        ToolResultPart::Text { text } | ToolResultPart::Code { text, .. } => Ok(text.clone()),
        ToolResultPart::Reference { summary, .. } => Ok(summary.clone().unwrap_or_default()),
        ToolResultPart::Blob { .. } => Err(ModelError::InvalidRequest(
            "blob tool result parts are not supported by OpenAI-compatible providers".to_owned(),
        )),
        ToolResultPart::Artifact { .. } => Err(ModelError::InvalidRequest(
            "artifact tool result parts are not supported by OpenAI-compatible providers"
                .to_owned(),
        )),
        _ => Err(ModelError::InvalidRequest(
            "unsupported tool result part for OpenAI-compatible providers".to_owned(),
        )),
    }
}

fn openai_tool(tool: &ToolDescriptor) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema,
        },
    })
}

fn responses_tool(tool: &ToolDescriptor) -> Value {
    json!({
        "type": "function",
        "name": tool.name,
        "description": tool.description,
        "parameters": tool.input_schema,
    })
}

fn chat_response_to_stream(response: Value) -> Result<ModelStream, ModelError> {
    let response: ChatCompletionResponse = serde_json::from_value(response).map_err(|error| {
        ModelError::UnexpectedResponse(format!("invalid OpenAI-compatible response: {error}"))
    })?;
    let usage = usage(response.usage.as_ref());
    let choice = response.choices.into_iter().next().ok_or_else(|| {
        ModelError::UnexpectedResponse("OpenAI-compatible response had no choices".to_owned())
    })?;
    let mut events = vec![ModelStreamEvent::MessageStart {
        message_id: response.id,
        usage: usage.clone(),
    }];
    let mut index = 0;

    if let Some(content) = choice.message.content {
        if !content.is_empty() {
            events.push(ModelStreamEvent::ContentBlockStart {
                index,
                content_type: ContentType::Text,
            });
            events.push(ModelStreamEvent::ContentBlockDelta {
                index,
                delta: ContentDelta::Text(content),
            });
            events.push(ModelStreamEvent::ContentBlockStop { index });
            index += 1;
        }
    }

    for tool_call in choice.message.tool_calls {
        events.push(ModelStreamEvent::ContentBlockStart {
            index,
            content_type: ContentType::ToolUse,
        });
        events.push(ModelStreamEvent::ContentBlockDelta {
            index,
            delta: ContentDelta::ToolUseStart {
                id: tool_call.id,
                name: tool_call.function.name,
            },
        });
        if !tool_call.function.arguments.is_empty() {
            events.push(ModelStreamEvent::ContentBlockDelta {
                index,
                delta: ContentDelta::ToolUseInputJson(tool_call.function.arguments),
            });
        }
        events.push(ModelStreamEvent::ContentBlockStop { index });
        index += 1;
    }

    events.push(ModelStreamEvent::MessageDelta {
        stop_reason: choice.finish_reason.as_deref().map(stop_reason),
        usage_delta: usage,
    });
    events.push(ModelStreamEvent::MessageStop);
    Ok(Box::pin(stream::iter(events)))
}

pub(crate) fn usage(value: Option<&OpenAiUsage>) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens: value
            .and_then(|usage| usage.prompt_tokens)
            .unwrap_or_default(),
        output_tokens: value
            .and_then(|usage| usage.completion_tokens)
            .unwrap_or_default(),
        cache_read_tokens: value
            .and_then(|usage| usage.prompt_tokens_details.as_ref())
            .and_then(|details| details.cached_tokens)
            .unwrap_or_default(),
        cache_write_tokens: 0,
        cost_micros: 0,
        tool_calls: 0,
    }
}

pub(crate) fn stop_reason(reason: &str) -> StopReason {
    match reason {
        "stop" => StopReason::EndTurn,
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "length" => StopReason::MaxIterations,
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

#[cfg(test)]
mod credential_pool_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use chrono::Utc;
    use futures::StreamExt;
    use harness_contracts::{Message, MessageId, MessagePart, MessageRole, TenantId};
    use parking_lot::Mutex;
    use secrecy::SecretString;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, Request, ResponseTemplate,
    };

    use super::*;
    use crate::{
        CredentialError, CredentialKey, CredentialMetadata, CredentialPool, CredentialPoolResolver,
        CredentialSource, CredentialValue, PoolStrategy, RetryPolicy,
    };

    #[derive(Default)]
    struct Source {
        seen: Mutex<Vec<CredentialKey>>,
    }

    #[async_trait]
    impl CredentialSource for Source {
        async fn fetch(&self, key: CredentialKey) -> Result<CredentialValue, CredentialError> {
            self.seen.lock().push(key.clone());
            Ok(CredentialValue {
                secret: SecretString::new(key.key_label.clone().into_boxed_str()),
                metadata: CredentialMetadata::default(),
            })
        }

        async fn rotate(&self, _key: CredentialKey) -> Result<(), CredentialError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn credential_resolver_uses_tenant_provider_and_model_context() {
        let server = ok_server(Arc::new(Mutex::new(Vec::new()))).await;
        let source = Arc::new(Source::default());
        let resolver = resolver(
            PoolStrategy::FillFirst,
            source.clone(),
            ["default"],
            |resolver| resolver.with_model_labels("gpt-test", ["model-key"]),
        );
        let mut ctx = test_context();
        ctx.tenant_id = TenantId::from_u128(77);

        client(&server, resolver)
            .infer(request(), ctx)
            .await
            .expect("request should use pool credential")
            .collect::<Vec<_>>()
            .await;

        let seen = source.seen.lock();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].tenant_id, TenantId::from_u128(77));
        assert_eq!(seen[0].provider_id, "openai");
        assert_eq!(seen[0].key_label, "model-key");
    }

    #[tokio::test]
    async fn credential_resolver_round_robins_between_keys() {
        let auth_headers = Arc::new(Mutex::new(Vec::new()));
        let server = ok_server(auth_headers.clone()).await;
        let source = Arc::new(Source::default());
        let resolver = resolver(
            PoolStrategy::RoundRobin,
            source,
            ["primary", "backup"],
            |r| r,
        );
        let client = client_with_provider(&server, resolver, "openai-round-robin");
        let mut ctx = test_context();
        ctx.retry_policy.max_attempts = 1;

        client
            .infer(request(), ctx.clone())
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;
        client
            .infer(request(), ctx)
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;

        assert_eq!(
            auth_headers.lock().as_slice(),
            ["Bearer primary", "Bearer backup"]
        );
    }

    #[tokio::test]
    async fn rate_limit_cools_only_the_selected_credential_key() {
        let auth_headers = Arc::new(Mutex::new(Vec::new()));
        let seen_headers = auth_headers.clone();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(move |req: &Request| {
                let auth = authorization(req);
                seen_headers.lock().push(auth.clone());
                if auth == "Bearer primary" {
                    ResponseTemplate::new(429)
                        .set_body_json(json!({ "error": { "message": "rate limited" } }))
                } else {
                    ok_response()
                }
            })
            .mount(&server)
            .await;
        let source = Arc::new(Source::default());
        let resolver = resolver(
            PoolStrategy::RoundRobin,
            source,
            ["primary", "backup"],
            |r| r,
        );
        let client = client(&server, resolver);
        let mut ctx = test_context();
        ctx.retry_policy = RetryPolicy {
            backoff: Backoff::Fixed(Duration::ZERO),
            ..RetryPolicy::default()
        };

        client
            .infer(request(), ctx.clone())
            .await
            .expect("backup key should satisfy retry")
            .collect::<Vec<_>>()
            .await;
        client
            .infer(request(), ctx)
            .await
            .expect("primary should still be cooling")
            .collect::<Vec<_>>()
            .await;

        assert_eq!(
            auth_headers.lock().as_slice(),
            ["Bearer primary", "Bearer backup", "Bearer backup"]
        );
    }

    #[tokio::test]
    async fn auth_failure_bans_key_without_retrying_current_request() {
        let auth_headers = Arc::new(Mutex::new(Vec::new()));
        let seen_headers = auth_headers.clone();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(move |req: &Request| {
                let auth = authorization(req);
                seen_headers.lock().push(auth.clone());
                if auth == "Bearer primary" {
                    ResponseTemplate::new(401)
                        .set_body_json(json!({ "error": { "message": "bad key" } }))
                } else {
                    ok_response()
                }
            })
            .mount(&server)
            .await;
        let source = Arc::new(Source::default());
        let resolver = resolver(
            PoolStrategy::FillFirst,
            source,
            ["primary", "backup"],
            |r| r,
        );
        let client = client(&server, resolver);

        let error = match client.infer(request(), test_context()).await {
            Ok(_) => panic!("auth failure should not retry to backup in the same request"),
            Err(error) => error,
        };
        assert!(matches!(error, ModelError::AuthExpired(_)));

        client
            .infer(request(), test_context())
            .await
            .expect("next request should skip banned primary")
            .collect::<Vec<_>>()
            .await;

        assert_eq!(
            auth_headers.lock().as_slice(),
            ["Bearer primary", "Bearer backup"]
        );
    }

    #[tokio::test]
    async fn provider_error_redacts_static_api_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": { "message": "bad key custom-provider-token" }
            })))
            .mount(&server)
            .await;
        let client = OpenAiCompatibleClient::from_api_key("custom-provider-token", server.uri());

        let error = match client.infer(request(), test_context()).await {
            Ok(_) => panic!("auth failure should be returned"),
            Err(error) => error,
        };

        assert!(
            matches!(error, ModelError::AuthExpired(message) if !message.contains("custom-provider-token") && message.contains("[REDACTED]"))
        );
    }

    #[tokio::test]
    async fn provider_error_redacts_resolved_credential() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": { "message": "bad key tenant-provider-token" }
            })))
            .mount(&server)
            .await;
        let source = Arc::new(Source::default());
        let resolver = resolver(
            PoolStrategy::FillFirst,
            source,
            ["tenant-provider-token"],
            |r| r,
        );
        let client = client(&server, resolver);

        let error = match client.infer(request(), test_context()).await {
            Ok(_) => panic!("auth failure should be returned"),
            Err(error) => error,
        };

        assert!(
            matches!(error, ModelError::AuthExpired(message) if !message.contains("tenant-provider-token") && message.contains("[REDACTED]"))
        );
    }

    fn resolver<I, S, F>(
        strategy: PoolStrategy,
        source: Arc<Source>,
        labels: I,
        configure: F,
    ) -> Arc<dyn ModelCredentialResolver>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
        F: FnOnce(CredentialPoolResolver) -> CredentialPoolResolver,
    {
        let pool = Arc::new(
            CredentialPool::builder()
                .strategy(strategy)
                .add_source(source)
                .build(),
        );
        Arc::new(configure(CredentialPoolResolver::new(pool, labels)))
    }

    fn client(
        server: &MockServer,
        resolver: Arc<dyn ModelCredentialResolver>,
    ) -> OpenAiCompatibleClient {
        client_with_provider(server, resolver, "openai")
    }

    fn client_with_provider(
        server: &MockServer,
        resolver: Arc<dyn ModelCredentialResolver>,
        provider_id: &'static str,
    ) -> OpenAiCompatibleClient {
        OpenAiCompatibleClient::from_api_key("unused", server.uri())
            .with_provider_id(provider_id)
            .with_credential_resolver(resolver)
    }

    async fn ok_server(auth_headers: Arc<Mutex<Vec<String>>>) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(move |req: &Request| {
                auth_headers.lock().push(authorization(req));
                ok_response()
            })
            .mount(&server)
            .await;
        server
    }

    fn authorization(req: &Request) -> String {
        req.headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    }

    fn ok_response() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl_1",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "ok"
                },
                "finish_reason": "stop"
            }],
            "usage": {}
        }))
    }

    fn request() -> ModelRequest {
        ModelRequest {
            model_id: "gpt-test".to_owned(),
            messages: vec![Message {
                id: MessageId::new(),
                role: MessageRole::User,
                parts: vec![MessagePart::Text("hello".to_owned())],
                created_at: Utc::now(),
            }],
            tools: None,
            system: None,
            temperature: None,
            max_tokens: Some(32),
            stream: false,
            cache_breakpoints: Vec::new(),
            protocol: ModelProtocol::ChatCompletions,
            extra: Value::Null,
        }
    }

    fn test_context() -> InferContext {
        InferContext::for_test()
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    id: String,
    #[serde(default)]
    choices: Vec<ChatCompletionChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChoice {
    message: ChatMessageResponse,
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ChatMessageResponse {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ChatToolCall>,
}

#[derive(Debug, Deserialize)]
struct ChatToolCall {
    id: String,
    function: ChatToolCallFunction,
}

#[derive(Debug, Deserialize)]
struct ChatToolCallFunction {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PromptTokensDetails {
    cached_tokens: Option<u64>,
}

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    Message, MessagePart, MessageRole, ModelError, StopReason, ToolDescriptor, ToolResult,
    ToolResultPart, UsageSnapshot,
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

#[derive(Clone)]
pub struct AnthropicClient {
    http: reqwest::Client,
    api_key: SecretString,
    credential_resolver: Option<Arc<dyn ModelCredentialResolver>>,
    base_url: String,
    cooldown_until: Arc<Mutex<Option<Instant>>>,
}

impl AnthropicClient {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: SecretString::new(api_key.into().into_boxed_str()),
            credential_resolver: None,
            base_url: DEFAULT_BASE_URL.to_owned(),
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

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        validate_request(&req)?;
        let body = request_body(&req)?;
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
                provider_id: PROVIDER_ID.to_owned(),
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
                "{}/v1/messages",
                self.base_url.trim_end_matches('/')
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
            "AnthropicProvider only supports ModelProtocol::Messages".to_owned(),
        ));
    }
    Ok(())
}

fn request_body(req: &ModelRequest) -> Result<Value, ModelError> {
    let messages = req
        .messages
        .iter()
        .map(anthropic_message)
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

fn anthropic_message(message: &Message) -> Result<Value, ModelError> {
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
            .map(anthropic_tool_result_part)
            .collect::<Result<Vec<_>, _>>()?
    } else {
        message
            .parts
            .iter()
            .map(anthropic_part)
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(json!({
        "role": role,
        "content": content,
    }))
}

fn anthropic_part(part: &MessagePart) -> Result<Value, ModelError> {
    match part {
        MessagePart::Text(text) => Ok(json!({ "type": "text", "text": text })),
        MessagePart::ToolUse { id, name, input } => Ok(json!({
            "type": "tool_use",
            "id": id.to_string(),
            "name": name,
            "input": input,
        })),
        MessagePart::Thinking(_) => Err(ModelError::InvalidRequest(
            "Anthropic thinking replay requires an explicit provider-native contract".to_owned(),
        )),
        MessagePart::ToolResult { .. } => Err(ModelError::InvalidRequest(
            "Anthropic tool_result blocks must use MessageRole::Tool".to_owned(),
        )),
        MessagePart::Image { .. } => Err(ModelError::InvalidRequest(
            "AnthropicProvider does not inline image message parts yet".to_owned(),
        )),
        MessagePart::Video { .. } | MessagePart::File { .. } => Err(ModelError::InvalidRequest(
            "AnthropicProvider does not inline file or video message parts yet".to_owned(),
        )),
        _ => Err(ModelError::InvalidRequest(
            "unsupported Anthropic message part".to_owned(),
        )),
    }
}

fn anthropic_tool_result_part(part: &MessagePart) -> Result<Value, ModelError> {
    match part {
        MessagePart::ToolResult {
            tool_use_id,
            content,
        } => Ok(json!({
            "type": "tool_result",
            "tool_use_id": tool_use_id.to_string(),
            "content": anthropic_tool_result_content(content)?,
        })),
        _ => Err(ModelError::InvalidRequest(
            "Anthropic tool messages may only contain tool_result parts".to_owned(),
        )),
    }
}

fn anthropic_tool_result_content(content: &ToolResult) -> Result<Value, ModelError> {
    match content {
        ToolResult::Text(text) => Ok(json!([{ "type": "text", "text": text }])),
        ToolResult::Structured(value) => Ok(json!([{ "type": "text", "text": value.to_string() }])),
        ToolResult::Mixed(parts) => Ok(Value::Array(
            parts
                .iter()
                .map(anthropic_tool_result_content_part)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        ToolResult::Blob { .. } => Err(ModelError::InvalidRequest(
            "AnthropicProvider does not inline blob tool results".to_owned(),
        )),
        _ => Err(ModelError::InvalidRequest(
            "unsupported Anthropic tool result".to_owned(),
        )),
    }
}

fn anthropic_tool_result_content_part(part: &ToolResultPart) -> Result<Value, ModelError> {
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
        ToolResultPart::Blob { .. } => Err(ModelError::InvalidRequest(
            "AnthropicProvider does not inline blob tool result parts".to_owned(),
        )),
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

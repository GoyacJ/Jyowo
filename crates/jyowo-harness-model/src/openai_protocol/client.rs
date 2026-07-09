use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use harness_contracts::ModelError;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use tokio::sync::{Mutex, Semaphore};

use crate::{
    apply_response_headers_middlewares, wrap_stream_with_cancel_deadline, Backoff, CredentialValue,
    ErrorClass, InferContext, ModelCredentialPickContext, ModelCredentialResolver, ModelProtocol,
    ModelRequest, ModelStream, PickedCredential,
};

use super::chat_codec;
use super::dialect::OpenAiChatDialect;
use super::error::{map_response_error, map_transport_error, OpenAiProtocolError};
use super::{responses_codec, streaming};

const DEFAULT_CREDENTIAL_RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub(crate) struct OpenAiProtocolClient {
    http: reqwest::Client,
    api_key: Option<SecretString>,
    credential_resolver: Option<Arc<dyn ModelCredentialResolver>>,
    provider_id: String,
    base_url: String,
    path: String,
    protocol: ModelProtocol,
    max_tokens_field: &'static str,
    dialect: OpenAiChatDialect,
    extra_headers: BTreeMap<String, String>,
    cooldown_until: Arc<Mutex<Option<Instant>>>,
    concurrency: Option<Arc<Semaphore>>,
}

#[allow(dead_code)]
impl OpenAiProtocolClient {
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
            provider_id: "openai-protocol".to_owned(),
            base_url: base_url.into(),
            path: path.into(),
            protocol,
            max_tokens_field: "max_tokens",
            dialect: OpenAiChatDialect::Plain,
            extra_headers: BTreeMap::new(),
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
    pub(crate) fn with_chat_dialect(mut self, dialect: OpenAiChatDialect) -> Self {
        self.dialect = dialect;
        self
    }

    pub(crate) fn chat_dialect(&self) -> OpenAiChatDialect {
        self.dialect
    }

    #[must_use]
    pub(crate) fn with_extra_headers(mut self, headers: BTreeMap<String, String>) -> Self {
        self.extra_headers = headers;
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
                                streaming::response_to_stream(response, self.dialect)
                            }
                            ModelProtocol::Responses => {
                                responses_codec::response_to_stream(response)
                            }
                            _ => unreachable!("validated OpenAI protocol API mode"),
                        };
                        return Ok(wrap_stream_with_cancel_deadline(stream, &ctx));
                    }
                    let response = response
                        .json()
                        .await
                        .map_err(map_transport_error)
                        .map_err(|error| error.error)?;
                    return match self.protocol {
                        ModelProtocol::ChatCompletions => {
                            chat_codec::chat_response_to_stream(response, self.dialect)
                        }
                        ModelProtocol::Responses => {
                            responses_codec::json_response_to_stream(response)
                        }
                        _ => unreachable!("validated OpenAI protocol API mode"),
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
    ) -> Result<reqwest::Response, OpenAiProtocolError> {
        let _permit = match &self.concurrency {
            Some(semaphore) => Some(semaphore.clone().acquire_owned().await.map_err(|error| {
                OpenAiProtocolError {
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
    ) -> Result<HeaderMap, OpenAiProtocolError> {
        let mut headers = HeaderMap::new();
        let api_key = credential
            .map(|credential| &credential.secret)
            .or(self.api_key.as_ref());
        if let Some(api_key) = api_key {
            let value = format!("Bearer {}", api_key.expose_secret());
            let auth = HeaderValue::from_str(&value).map_err(|error| OpenAiProtocolError {
                error: ModelError::AuthExpired(error.to_string()),
                class: ErrorClass::AuthExpired,
                retry_after: None,
            })?;
            headers.insert(AUTHORIZATION, auth);
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        for (name, value) in &self.extra_headers {
            let name =
                HeaderName::from_bytes(name.as_bytes()).map_err(|error| OpenAiProtocolError {
                    error: ModelError::InvalidRequest(format!(
                        "invalid provider header name: {error}"
                    )),
                    class: ErrorClass::Fatal,
                    retry_after: None,
                })?;
            let value = HeaderValue::from_str(value).map_err(|error| OpenAiProtocolError {
                error: ModelError::InvalidRequest(format!(
                    "invalid provider header value: {error}"
                )),
                class: ErrorClass::Fatal,
                retry_after: None,
            })?;
            headers.insert(name, value);
        }
        Ok(headers)
    }

    fn validate_request(&self, req: &ModelRequest) -> Result<(), ModelError> {
        if req.protocol != self.protocol {
            return Err(ModelError::InvalidRequest(format!(
                "OpenAI protocol provider expected {:?}, got {:?}",
                self.protocol, req.protocol
            )));
        }
        if !req.cache_breakpoints.is_empty() {
            return Err(ModelError::InvalidRequest(
                "OpenAI protocol providers do not accept explicit cache breakpoints".to_owned(),
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
                chat_codec::chat_request_body(req, self.max_tokens_field, self.dialect, ctx).await
            }
            ModelProtocol::Responses => responses_codec::responses_request_body(req, ctx).await,
            _ => Err(ModelError::InvalidRequest(
                "unsupported OpenAI protocol API mode".to_owned(),
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
pub(crate) trait OpenAiProtocolProviderExt: Send + Sync + 'static {
    fn client(&self) -> &OpenAiProtocolClient;

    async fn infer_openai_protocol(
        &self,
        req: ModelRequest,
        ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.client().infer(req, ctx).await
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
    use harness_contracts::TenantId;
    use parking_lot::Mutex;
    use secrecy::SecretString;
    use serde_json::json;
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
        let client = test_client_from_api_key("custom-provider-token", server.uri());

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
    ) -> OpenAiProtocolClient {
        client_with_provider(server, resolver, "openai")
    }

    fn client_with_provider(
        server: &MockServer,
        resolver: Arc<dyn ModelCredentialResolver>,
        provider_id: &'static str,
    ) -> OpenAiProtocolClient {
        test_client_from_api_key("unused", server.uri())
            .with_provider_id(provider_id)
            .with_credential_resolver(resolver)
    }

    fn test_client_from_api_key(
        api_key: impl Into<String>,
        base_url: impl Into<String>,
    ) -> OpenAiProtocolClient {
        let mut client = OpenAiProtocolClient::from_api_key(api_key, base_url);
        client.http = reqwest::Client::builder()
            .no_proxy()
            .pool_max_idle_per_host(0)
            .build()
            .expect("test http client should build");
        client
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
            messages: vec![harness_contracts::Message {
                id: harness_contracts::MessageId::new(),
                role: harness_contracts::MessageRole::User,
                parts: vec![harness_contracts::MessagePart::Text("hello".to_owned())],
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
            provider_context: crate::ProviderRequestContext::default(),
        }
    }

    fn test_context() -> InferContext {
        InferContext::for_test()
    }
}

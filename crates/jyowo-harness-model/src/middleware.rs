use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::{ModelError, RedactRules, Redactor, UsageSnapshot};
use http::HeaderMap;

use crate::{
    ContentDelta, ErrorClass, ErrorHints, InferContext, ModelRequest, ModelStream,
    ModelStreamEvent, TraceContext,
};

#[async_trait]
pub trait InferMiddleware: Send + Sync + 'static {
    fn middleware_id(&self) -> &str;

    async fn before_request(
        &self,
        _req: &mut ModelRequest,
        _ctx: &mut InferContext,
    ) -> Result<(), ModelError> {
        Ok(())
    }

    async fn on_response_headers(
        &self,
        _headers: &HeaderMap,
        _ctx: &InferContext,
    ) -> Result<(), ModelError> {
        Ok(())
    }

    fn wrap_stream(&self, stream: ModelStream, _ctx: &InferContext) -> ModelStream {
        stream
    }

    async fn on_request_end(
        &self,
        _usage: &UsageSnapshot,
        _ctx: &InferContext,
    ) -> Result<(), ModelError> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitObservation {
    pub remaining_requests: Option<u64>,
    pub remaining_tokens: Option<u64>,
    pub retry_after: Option<Duration>,
}

pub trait RateLimitObserver: Send + Sync + 'static {
    fn observe_rate_limit(&self, observation: RateLimitObservation, ctx: &InferContext);
}

#[derive(Clone)]
pub struct RateLimitObserverMiddleware {
    observer: Arc<dyn RateLimitObserver>,
}

impl RateLimitObserverMiddleware {
    #[must_use]
    pub fn new(observer: Arc<dyn RateLimitObserver>) -> Self {
        Self { observer }
    }
}

#[async_trait]
impl InferMiddleware for RateLimitObserverMiddleware {
    fn middleware_id(&self) -> &str {
        "rate-limit-observer"
    }

    async fn on_response_headers(
        &self,
        headers: &HeaderMap,
        ctx: &InferContext,
    ) -> Result<(), ModelError> {
        let observation = RateLimitObservation {
            remaining_requests: header_u64(headers, "x-ratelimit-remaining-requests"),
            remaining_tokens: header_u64(headers, "x-ratelimit-remaining-tokens"),
            retry_after: header_u64(headers, "retry-after").map(Duration::from_secs),
        };
        if observation.remaining_requests.is_some()
            || observation.remaining_tokens.is_some()
            || observation.retry_after.is_some()
        {
            self.observer.observe_rate_limit(observation, ctx);
        }
        Ok(())
    }
}

#[async_trait]
pub trait OAuthRefreshHandler: Send + Sync + 'static {
    async fn refresh(&self, ctx: &InferContext) -> Result<(), ModelError>;
}

#[derive(Clone)]
pub struct OAuthAutoRefreshMiddleware {
    handler: Arc<dyn OAuthRefreshHandler>,
}

impl OAuthAutoRefreshMiddleware {
    #[must_use]
    pub fn new(handler: Arc<dyn OAuthRefreshHandler>) -> Self {
        Self { handler }
    }
}

#[async_trait]
impl InferMiddleware for OAuthAutoRefreshMiddleware {
    fn middleware_id(&self) -> &str {
        "oauth-auto-refresh"
    }

    fn wrap_stream(&self, stream: ModelStream, ctx: &InferContext) -> ModelStream {
        let handler = Arc::clone(&self.handler);
        let ctx = ctx.clone();
        Box::pin(stream.then(move |event| {
            let handler = Arc::clone(&handler);
            let ctx = ctx.clone();
            async move {
                if matches!(
                    &event,
                    ModelStreamEvent::StreamError {
                        class: ErrorClass::AuthExpired,
                        ..
                    }
                ) {
                    let _ = handler.refresh(&ctx).await;
                }
                event
            }
        }))
    }
}

#[derive(Clone)]
pub struct RedactStreamMiddleware {
    redactor: Arc<dyn Redactor>,
    rules: RedactRules,
}

impl RedactStreamMiddleware {
    #[must_use]
    pub fn new(redactor: Arc<dyn Redactor>, rules: RedactRules) -> Self {
        Self { redactor, rules }
    }
}

#[async_trait]
impl InferMiddleware for RedactStreamMiddleware {
    fn middleware_id(&self) -> &str {
        "redact-stream"
    }

    fn wrap_stream(&self, stream: ModelStream, _ctx: &InferContext) -> ModelStream {
        let redactor = Arc::clone(&self.redactor);
        let rules = self.rules.clone();
        Box::pin(stream.map(move |event| match event {
            ModelStreamEvent::ContentBlockDelta {
                index,
                delta: ContentDelta::Text(text),
            } => ModelStreamEvent::ContentBlockDelta {
                index,
                delta: ContentDelta::Text(redactor.redact(&text, &rules)),
            },
            event => event,
        }))
    }
}

pub trait TraceSpanObserver: Send + Sync + 'static {
    fn model_infer_span_started(&self, ctx: &InferContext);
    fn model_infer_span_finished(&self, usage: &UsageSnapshot, ctx: &InferContext);
}

#[derive(Clone, Default)]
pub struct TraceSpanMiddleware {
    observer: Option<Arc<dyn TraceSpanObserver>>,
}

impl TraceSpanMiddleware {
    #[must_use]
    pub fn new(observer: Arc<dyn TraceSpanObserver>) -> Self {
        Self {
            observer: Some(observer),
        }
    }

    #[must_use]
    pub fn noop() -> Self {
        Self::default()
    }
}

#[async_trait]
impl InferMiddleware for TraceSpanMiddleware {
    fn middleware_id(&self) -> &str {
        "trace-span"
    }

    async fn before_request(
        &self,
        _req: &mut ModelRequest,
        ctx: &mut InferContext,
    ) -> Result<(), ModelError> {
        if ctx.tracing.is_none() {
            ctx.tracing = Some(TraceContext {
                trace_id: Some(ctx.request_id.to_string()),
                span_id: Some("harness.model.infer".to_owned()),
            });
        }
        if let Some(observer) = &self.observer {
            observer.model_infer_span_started(ctx);
        }
        Ok(())
    }

    async fn on_request_end(
        &self,
        usage: &UsageSnapshot,
        ctx: &InferContext,
    ) -> Result<(), ModelError> {
        if let Some(observer) = &self.observer {
            observer.model_infer_span_finished(usage, ctx);
        }
        Ok(())
    }
}

pub async fn apply_before_request_middlewares(
    req: &mut ModelRequest,
    ctx: &mut InferContext,
) -> Result<(), ModelError> {
    let middlewares = ctx.middlewares.clone();
    for middleware in middlewares {
        middleware.before_request(req, ctx).await?;
    }
    Ok(())
}

pub async fn apply_response_headers_middlewares(
    headers: &HeaderMap,
    ctx: &InferContext,
) -> Result<(), ModelError> {
    for middleware in &ctx.middlewares {
        middleware.on_response_headers(headers, ctx).await?;
    }
    Ok(())
}

pub fn wrap_stream_with_middlewares(mut stream: ModelStream, ctx: &InferContext) -> ModelStream {
    for middleware in ctx.middlewares.iter().rev() {
        stream = middleware.wrap_stream(stream, ctx);
    }
    stream
}

pub fn wrap_stream_with_cancel_deadline(
    mut stream: ModelStream,
    ctx: &InferContext,
) -> ModelStream {
    let cancel = ctx.cancel.clone();
    let deadline = ctx.deadline;
    Box::pin(async_stream::stream! {
        loop {
            if cancel.is_cancelled() {
                yield control_stream_error(ModelError::Cancelled);
                return;
            }
            let Some(deadline) = deadline else {
                tokio::select! {
                    () = cancel.cancelled() => {
                        yield control_stream_error(ModelError::Cancelled);
                        return;
                    },
                    next = futures::StreamExt::next(&mut stream) => {
                        match next {
                            Some(event) => yield event,
                            None => return,
                        }
                    },
                }
                continue;
            };
            let now = std::time::Instant::now();
            if now >= deadline {
                yield control_stream_error(ModelError::DeadlineExceeded(now.duration_since(deadline)));
                return;
            }
            let sleep = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline));
            tokio::pin!(sleep);
            tokio::select! {
                () = cancel.cancelled() => {
                    yield control_stream_error(ModelError::Cancelled);
                    return;
                },
                () = &mut sleep => {
                    yield control_stream_error(ModelError::DeadlineExceeded(std::time::Duration::ZERO));
                    return;
                },
                next = futures::StreamExt::next(&mut stream) => {
                    match next {
                        Some(event) => yield event,
                        None => return,
                    }
                },
            }
        }
    })
}

fn control_stream_error(error: ModelError) -> ModelStreamEvent {
    ModelStreamEvent::StreamError {
        error,
        class: ErrorClass::Transient,
        hints: ErrorHints::default(),
    }
}

fn header_u64(headers: &HeaderMap, name: &'static str) -> Option<u64> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
}

pub async fn apply_request_end_middlewares(
    usage: &UsageSnapshot,
    ctx: &InferContext,
) -> Result<(), ModelError> {
    for middleware in &ctx.middlewares {
        middleware.on_request_end(usage, ctx).await?;
    }
    Ok(())
}

use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_contracts::{ModelError, RedactRules, Redactor, UsageSnapshot};
use harness_model::{
    apply_before_request_middlewares, apply_request_end_middlewares,
    apply_response_headers_middlewares, wrap_stream_with_middlewares, ContentDelta, ErrorClass,
    ErrorHints, InferContext, InferMiddleware, ModelRequest, ModelStreamEvent,
    OAuthAutoRefreshMiddleware, OAuthRefreshHandler, RateLimitObservation, RateLimitObserver,
    RateLimitObserverMiddleware, RedactStreamMiddleware, TraceSpanMiddleware, TraceSpanObserver,
};
use http::HeaderMap;
use parking_lot::Mutex;
use serde_json::json;

fn request() -> ModelRequest {
    ModelRequest {
        model_id: "test-model".to_owned(),
        messages: Vec::new(),
        tools: None,
        system: None,
        temperature: None,
        max_tokens: None,
        stream: true,
        cache_breakpoints: Vec::new(),
        protocol: harness_model::ModelProtocol::Messages,
        extra: json!({}),
    }
}

#[derive(Clone)]
struct OrderMiddleware {
    id: &'static str,
    calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl InferMiddleware for OrderMiddleware {
    fn middleware_id(&self) -> &str {
        self.id
    }

    async fn before_request(
        &self,
        _req: &mut ModelRequest,
        _ctx: &mut InferContext,
    ) -> Result<(), ModelError> {
        self.calls.lock().push(format!("before:{}", self.id));
        Ok(())
    }

    fn wrap_stream(
        &self,
        stream: harness_model::ModelStream,
        _ctx: &InferContext,
    ) -> harness_model::ModelStream {
        let id = self.id;
        let calls = Arc::clone(&self.calls);
        Box::pin(stream.map(move |event| {
            calls.lock().push(format!("wrap:{id}"));
            event
        }))
    }
}

#[tokio::test]
async fn middleware_order_matches_contract() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut ctx = InferContext::for_test();
    ctx.middlewares = vec![
        Arc::new(OrderMiddleware {
            id: "first",
            calls: Arc::clone(&calls),
        }),
        Arc::new(OrderMiddleware {
            id: "second",
            calls: Arc::clone(&calls),
        }),
    ];
    let mut req = request();

    apply_before_request_middlewares(&mut req, &mut ctx)
        .await
        .unwrap();
    let stream = Box::pin(stream::iter(vec![ModelStreamEvent::MessageStop]));
    let mut stream = wrap_stream_with_middlewares(stream, &ctx);
    while stream.next().await.is_some() {}

    assert_eq!(
        *calls.lock(),
        vec!["before:first", "before:second", "wrap:second", "wrap:first"]
    );
}

#[derive(Default)]
struct RecordingRateLimitObserver {
    observations: Mutex<Vec<RateLimitObservation>>,
}

impl RateLimitObserver for RecordingRateLimitObserver {
    fn observe_rate_limit(&self, observation: RateLimitObservation, _ctx: &InferContext) {
        self.observations.lock().push(observation);
    }
}

#[tokio::test]
async fn rate_limit_observer_reads_response_headers() {
    let observer = Arc::new(RecordingRateLimitObserver::default());
    let mut ctx = InferContext::for_test();
    ctx.middlewares = vec![Arc::new(RateLimitObserverMiddleware::new(observer.clone()))];
    let mut headers = HeaderMap::new();
    headers.insert("x-ratelimit-remaining-requests", "7".parse().unwrap());
    headers.insert("x-ratelimit-remaining-tokens", "99".parse().unwrap());
    headers.insert("retry-after", "5".parse().unwrap());

    apply_response_headers_middlewares(&headers, &ctx)
        .await
        .unwrap();

    assert_eq!(observer.observations.lock().len(), 1);
    assert_eq!(
        observer.observations.lock()[0],
        RateLimitObservation {
            remaining_requests: Some(7),
            remaining_tokens: Some(99),
            retry_after: Some(std::time::Duration::from_secs(5)),
        }
    );
}

struct ReplaceRedactor;

impl Redactor for ReplaceRedactor {
    fn redact(&self, input: &str, rules: &RedactRules) -> String {
        input.replace("secret", &rules.replacement)
    }
}

#[tokio::test]
async fn redact_stream_only_rewrites_text_delta() {
    let mut ctx = InferContext::for_test();
    ctx.middlewares = vec![Arc::new(RedactStreamMiddleware::new(
        Arc::new(ReplaceRedactor),
        RedactRules::default(),
    ))];
    let stream = Box::pin(stream::iter(vec![ModelStreamEvent::ContentBlockDelta {
        index: 0,
        delta: ContentDelta::Text("secret value".to_owned()),
    }]));
    let events = wrap_stream_with_middlewares(stream, &ctx)
        .collect::<Vec<_>>()
        .await;

    assert_eq!(
        events,
        vec![ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("[REDACTED] value".to_owned()),
        }]
    );
}

#[derive(Default)]
struct RefreshRecorder {
    calls: Mutex<u32>,
}

#[async_trait]
impl OAuthRefreshHandler for RefreshRecorder {
    async fn refresh(&self, _ctx: &InferContext) -> Result<(), ModelError> {
        *self.calls.lock() += 1;
        Ok(())
    }
}

#[tokio::test]
async fn oauth_auto_refresh_observes_auth_expired_stream_error() {
    let handler = Arc::new(RefreshRecorder::default());
    let mut ctx = InferContext::for_test();
    ctx.middlewares = vec![Arc::new(OAuthAutoRefreshMiddleware::new(handler.clone()))];
    let stream = Box::pin(stream::iter(vec![ModelStreamEvent::StreamError {
        error: ModelError::AuthExpired("expired".to_owned()),
        class: ErrorClass::AuthExpired,
        hints: ErrorHints::default(),
    }]));

    let _ = wrap_stream_with_middlewares(stream, &ctx)
        .collect::<Vec<_>>()
        .await;

    assert_eq!(*handler.calls.lock(), 1);
}

#[derive(Default)]
struct TraceRecorder {
    calls: Mutex<Vec<&'static str>>,
}

impl TraceSpanObserver for TraceRecorder {
    fn model_infer_span_started(&self, _ctx: &InferContext) {
        self.calls.lock().push("start");
    }

    fn model_infer_span_finished(&self, _usage: &UsageSnapshot, _ctx: &InferContext) {
        self.calls.lock().push("finish");
    }
}

#[tokio::test]
async fn trace_span_middleware_sets_context_and_finishes() {
    let recorder = Arc::new(TraceRecorder::default());
    let mut ctx = InferContext::for_test();
    ctx.middlewares = vec![Arc::new(TraceSpanMiddleware::new(recorder.clone()))];
    let mut req = request();

    apply_before_request_middlewares(&mut req, &mut ctx)
        .await
        .unwrap();
    apply_request_end_middlewares(&UsageSnapshot::default(), &ctx)
        .await
        .unwrap();

    assert!(ctx.tracing.is_some());
    assert_eq!(*recorder.calls.lock(), vec!["start", "finish"]);
}

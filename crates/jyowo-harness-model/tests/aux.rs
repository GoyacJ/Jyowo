use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{Message, MessagePart, MessageRole, ModelError};
use harness_model::{
    AuxExecutor, AuxModelProvider, AuxOptions, AuxTask, BasicAuxProvider, ContentDelta,
    ConversationModelCapability, InferContext, ModelDescriptor, ModelMetricsSink, ModelProtocol,
    ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use parking_lot::Mutex as ParkingMutex;
use tokio::sync::Mutex;

#[tokio::test]
async fn basic_aux_provider_collects_model_text() {
    let inner = Arc::new(TextModelProvider::new("summary"));
    let provider = BasicAuxProvider::new(inner);
    let output = provider
        .call_aux(AuxTask::Compact, request("compact this"))
        .await
        .unwrap();

    assert_eq!(output, "summary");
}

#[tokio::test]
async fn aux_executor_times_out_fail_closed() {
    let provider = Arc::new(SlowAuxProvider::new(
        AuxOptions {
            per_task_timeout: Duration::from_millis(10),
            fail_open: false,
            ..AuxOptions::default()
        },
        Duration::from_millis(100),
    ));
    let executor = AuxExecutor::new(provider);

    let error = executor
        .call(AuxTask::Summarize, request("slow"))
        .await
        .unwrap_err();

    assert!(matches!(error, ModelError::DeadlineExceeded(_)));
}

#[tokio::test]
async fn aux_executor_fails_open_when_configured() {
    let provider = Arc::new(ErrorAuxProvider {
        options: AuxOptions {
            fail_open: true,
            ..AuxOptions::default()
        },
    });
    let executor = AuxExecutor::new(provider);

    let output = executor
        .call(AuxTask::Classify, request("classify"))
        .await
        .unwrap();

    assert_eq!(output, None);
}

#[tokio::test]
async fn aux_executor_limits_concurrency() {
    let provider = Arc::new(ConcurrentAuxProvider::new(AuxOptions {
        max_concurrency: 1,
        per_task_timeout: Duration::from_secs(1),
        fail_open: false,
    }));
    let executor = AuxExecutor::new(provider.clone());

    let first = executor.call(AuxTask::Compact, request("first"));
    let second = executor.call(AuxTask::Compact, request("second"));
    let (first, second) = tokio::join!(first, second);

    assert_eq!(first.unwrap(), Some("ok".to_owned()));
    assert_eq!(second.unwrap(), Some("ok".to_owned()));
    assert_eq!(provider.max_seen.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn aux_executor_records_queue_wait_metric() {
    let provider = Arc::new(ConcurrentAuxProvider::new(AuxOptions {
        max_concurrency: 1,
        per_task_timeout: Duration::from_secs(1),
        fail_open: false,
    }));
    let metrics = Arc::new(RecordingModelMetricsSink::default());
    let executor = AuxExecutor::new(provider).with_metrics_sink(metrics.clone());

    let first = executor.call(AuxTask::Compact, request("first"));
    let second = executor.call(AuxTask::Compact, request("second"));
    let (first, second) = tokio::join!(first, second);

    assert_eq!(first.unwrap(), Some("ok".to_owned()));
    assert_eq!(second.unwrap(), Some("ok".to_owned()));
    let waits = metrics.aux_waits.lock();
    assert_eq!(waits.len(), 2);
    assert!(waits.iter().all(|(model, _)| model == "test"));
    assert!(waits.iter().any(|(_, wait)| *wait > Duration::ZERO));
}

struct TextModelProvider {
    text: String,
}

impl TextModelProvider {
    fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
        }
    }
}

#[async_trait]
impl ModelProvider for TextModelProvider {
    fn provider_id(&self) -> &str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![descriptor()]
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(stream::iter([
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text(self.text.clone()),
            },
            ModelStreamEvent::MessageStop,
        ])))
    }
}

struct SlowAuxProvider {
    options: AuxOptions,
    delay: Duration,
}

impl SlowAuxProvider {
    fn new(options: AuxOptions, delay: Duration) -> Self {
        Self { options, delay }
    }
}

#[async_trait]
impl AuxModelProvider for SlowAuxProvider {
    fn inner(&self) -> Arc<dyn ModelProvider> {
        Arc::new(TextModelProvider::new("unused"))
    }

    fn aux_options(&self) -> AuxOptions {
        self.options.clone()
    }

    async fn call_aux(&self, _task: AuxTask, _req: ModelRequest) -> Result<String, ModelError> {
        tokio::time::sleep(self.delay).await;
        Ok("late".to_owned())
    }
}

struct ErrorAuxProvider {
    options: AuxOptions,
}

#[async_trait]
impl AuxModelProvider for ErrorAuxProvider {
    fn inner(&self) -> Arc<dyn ModelProvider> {
        Arc::new(TextModelProvider::new("unused"))
    }

    fn aux_options(&self) -> AuxOptions {
        self.options.clone()
    }

    async fn call_aux(&self, _task: AuxTask, _req: ModelRequest) -> Result<String, ModelError> {
        Err(ModelError::ProviderUnavailable("down".to_owned()))
    }
}

#[derive(Default)]
struct RecordingModelMetricsSink {
    aux_waits: ParkingMutex<Vec<(String, Duration)>>,
}

impl ModelMetricsSink for RecordingModelMetricsSink {
    fn record_credential_pool_cooldown(&self, _model_id: &str) {}

    fn record_aux_queue_wait(&self, model_id: &str, duration: Duration) {
        self.aux_waits.lock().push((model_id.to_owned(), duration));
    }
}

struct ConcurrentAuxProvider {
    options: AuxOptions,
    active: AtomicUsize,
    max_seen: AtomicUsize,
    calls: Mutex<usize>,
}

impl ConcurrentAuxProvider {
    fn new(options: AuxOptions) -> Self {
        Self {
            options,
            active: AtomicUsize::new(0),
            max_seen: AtomicUsize::new(0),
            calls: Mutex::new(0),
        }
    }
}

#[async_trait]
impl AuxModelProvider for ConcurrentAuxProvider {
    fn inner(&self) -> Arc<dyn ModelProvider> {
        Arc::new(TextModelProvider::new("unused"))
    }

    fn aux_options(&self) -> AuxOptions {
        self.options.clone()
    }

    async fn call_aux(&self, _task: AuxTask, _req: ModelRequest) -> Result<String, ModelError> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_seen.fetch_max(active, Ordering::SeqCst);
        let mut calls = self.calls.lock().await;
        *calls += 1;
        let call_index = *calls;
        drop(calls);

        if call_index == 1 {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok("ok".to_owned())
    }
}

fn request(text: &str) -> ModelRequest {
    ModelRequest {
        model_id: "test".to_owned(),
        messages: vec![Message {
            id: harness_contracts::MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(text.to_owned())],
            created_at: harness_contracts::now(),
        }],
        tools: None,
        system: None,
        temperature: None,
        max_tokens: None,
        stream: false,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::Responses,
        extra: serde_json::Value::Null,
        options: harness_contracts::ModelRequestOptions::default(),
        provider_context: harness_model::ProviderRequestContext::default(),
    }
}

fn descriptor() -> ModelDescriptor {
    ModelDescriptor {
        protocol: harness_model::ModelProtocol::Messages,
        lifecycle: harness_model::ModelLifecycle::Stable,
        provider_id: "test".to_owned(),
        model_id: "test".to_owned(),
        display_name: "Test".to_owned(),
        context_window: 1_000,
        max_output_tokens: 100,
        provider_declared_capability: ConversationModelCapability::default(),
        conversation_capability: ConversationModelCapability::default(),
        runtime_semantics: harness_model::ModelRuntimeSemantics::messages_default(
            harness_model::ModelProtocol::Messages,
        ),
        pricing: None,
    }
}

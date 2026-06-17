use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use harness_contracts::ModelError;
use tokio::sync::Semaphore;

use crate::{
    InferContext, ModelMetricsSink, ModelProvider, ModelRequest, NoopModelMetricsSink,
    StreamAggregate, StreamAggregator,
};

#[async_trait]
pub trait AuxModelProvider: Send + Sync + 'static {
    fn inner(&self) -> Arc<dyn ModelProvider>;
    fn aux_options(&self) -> AuxOptions;

    async fn call_aux(&self, task: AuxTask, req: ModelRequest) -> Result<String, ModelError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuxTask {
    Compact,
    Summarize,
    Classify,
    PermissionAdvisory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuxOptions {
    pub max_concurrency: usize,
    pub per_task_timeout: Duration,
    pub fail_open: bool,
}

impl Default for AuxOptions {
    fn default() -> Self {
        Self {
            max_concurrency: 4,
            per_task_timeout: Duration::from_secs(30),
            fail_open: true,
        }
    }
}

#[derive(Clone)]
pub struct AuxExecutor {
    provider: Arc<dyn AuxModelProvider>,
    options: AuxOptions,
    semaphore: Arc<Semaphore>,
    metrics_sink: Arc<dyn ModelMetricsSink>,
}

impl AuxExecutor {
    #[must_use]
    pub fn new(provider: Arc<dyn AuxModelProvider>) -> Self {
        let options = provider.aux_options();
        Self::with_options(provider, options)
    }

    #[must_use]
    pub fn with_options(provider: Arc<dyn AuxModelProvider>, options: AuxOptions) -> Self {
        let max_concurrency = options.max_concurrency.max(1);
        Self {
            provider,
            options,
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
            metrics_sink: Arc::new(NoopModelMetricsSink),
        }
    }

    #[must_use]
    pub fn options(&self) -> &AuxOptions {
        &self.options
    }

    #[must_use]
    pub fn with_metrics_sink(mut self, metrics_sink: Arc<dyn ModelMetricsSink>) -> Self {
        self.metrics_sink = metrics_sink;
        self
    }

    #[must_use]
    pub fn provider(&self) -> Arc<dyn AuxModelProvider> {
        Arc::clone(&self.provider)
    }

    pub async fn call(
        &self,
        task: AuxTask,
        req: ModelRequest,
    ) -> Result<Option<String>, ModelError> {
        let wait_started = Instant::now();
        let Ok(_permit) = self.semaphore.acquire().await else {
            return self.handle_error(ModelError::ProviderUnavailable(
                "aux executor closed".to_owned(),
            ));
        };
        self.metrics_sink
            .record_aux_queue_wait(&req.model_id, wait_started.elapsed());

        match tokio::time::timeout(
            self.options.per_task_timeout,
            self.provider.call_aux(task, req),
        )
        .await
        {
            Ok(Ok(output)) => Ok(Some(output)),
            Ok(Err(error)) => self.handle_error(error),
            Err(_elapsed) => {
                self.handle_error(ModelError::DeadlineExceeded(self.options.per_task_timeout))
            }
        }
    }

    fn handle_error(&self, error: ModelError) -> Result<Option<String>, ModelError> {
        if self.options.fail_open {
            Ok(None)
        } else {
            Err(error)
        }
    }
}

#[derive(Clone)]
pub struct BasicAuxProvider {
    inner: Arc<dyn ModelProvider>,
    options: AuxOptions,
}

impl BasicAuxProvider {
    #[must_use]
    pub fn new(inner: Arc<dyn ModelProvider>) -> Self {
        Self {
            inner,
            options: AuxOptions::default(),
        }
    }

    #[must_use]
    pub fn with_options(mut self, options: AuxOptions) -> Self {
        self.options = options;
        self
    }
}

#[async_trait]
impl AuxModelProvider for BasicAuxProvider {
    fn inner(&self) -> Arc<dyn ModelProvider> {
        Arc::clone(&self.inner)
    }

    fn aux_options(&self) -> AuxOptions {
        self.options.clone()
    }

    async fn call_aux(&self, _task: AuxTask, mut req: ModelRequest) -> Result<String, ModelError> {
        req.stream = false;
        let mut stream = self.inner.infer(req, InferContext::for_test()).await?;
        let mut aggregator = StreamAggregator::default();
        let mut output = String::new();

        while let Some(event) = futures::StreamExt::next(&mut stream).await {
            for aggregate in aggregator.push(event) {
                match aggregate {
                    StreamAggregate::TextChunk { text } => output.push_str(&text),
                    StreamAggregate::StreamError { error, .. } => return Err(error),
                    StreamAggregate::MessageDone => return Ok(output),
                    _ => {}
                }
            }
        }

        Ok(output)
    }
}

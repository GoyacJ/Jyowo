use std::{net::SocketAddr, sync::Arc};

#[cfg(not(feature = "redactor"))]
use harness_contracts::NoopRedactor;
use harness_contracts::Redactor;

#[cfg(feature = "prometheus")]
use crate::PrometheusExporter;
#[cfg(feature = "replay")]
use crate::ReplayEngine;
use crate::{
    ModelMetricsAccumulator, NoopTracer, ObservabilityError, Span, SpanAttributes, TraceCarrier,
    TraceContext, Tracer, UsageAccumulator,
};

#[derive(Clone)]
pub struct Observer {
    pub tracer: Arc<dyn Tracer>,
    pub usage: Arc<UsageAccumulator>,
    pub model_metrics: Arc<ModelMetricsAccumulator>,
    pub redactor: Arc<dyn Redactor>,
    #[cfg(feature = "replay")]
    pub replay: Option<Arc<ReplayEngine>>,
    #[cfg(feature = "prometheus")]
    pub prometheus: Option<Arc<PrometheusExporter>>,
    #[allow(dead_code)]
    prometheus_bind: Option<SocketAddr>,
}

impl Observer {
    #[must_use]
    pub fn builder() -> ObserverBuilder {
        ObserverBuilder::default()
    }
}

impl Tracer for Observer {
    fn start_span(&self, name: &str, attrs: SpanAttributes) -> Box<dyn Span> {
        self.tracer.start_span(name, attrs)
    }

    fn inject_context(&self, carrier: &mut dyn TraceCarrier) {
        self.tracer.inject_context(carrier);
    }

    fn extract_context(&self, carrier: &dyn TraceCarrier) -> Option<TraceContext> {
        self.tracer.extract_context(carrier)
    }
}

#[derive(Clone, Default)]
pub struct ObserverBuilder {
    tracer: Option<Arc<dyn Tracer>>,
    usage: Option<Arc<UsageAccumulator>>,
    model_metrics: Option<Arc<ModelMetricsAccumulator>>,
    redactor: Option<Arc<dyn Redactor>>,
    #[cfg(feature = "replay")]
    replay: Option<Arc<ReplayEngine>>,
    prometheus_bind: Option<SocketAddr>,
    #[cfg(feature = "otel")]
    otel_endpoint: Option<String>,
    service_name: Option<String>,
}

impl ObserverBuilder {
    #[must_use]
    pub fn with_tracer(mut self, tracer: Arc<dyn Tracer>) -> Self {
        self.tracer = Some(tracer);
        self
    }

    #[must_use]
    pub fn with_usage_accumulator(mut self, usage: Arc<UsageAccumulator>) -> Self {
        self.usage = Some(usage);
        self
    }

    #[must_use]
    pub fn with_model_metrics(mut self, model_metrics: Arc<ModelMetricsAccumulator>) -> Self {
        self.model_metrics = Some(model_metrics);
        self
    }

    #[must_use]
    pub fn with_redactor(mut self, redactor: Arc<dyn Redactor>) -> Self {
        self.redactor = Some(redactor);
        self
    }

    #[cfg(feature = "replay")]
    #[must_use]
    pub fn with_replay_engine(mut self, replay: Arc<ReplayEngine>) -> Self {
        self.replay = Some(replay);
        self
    }

    #[must_use]
    pub fn with_prometheus(mut self, bind: SocketAddr) -> Self {
        self.prometheus_bind = Some(bind);
        self
    }

    #[cfg(feature = "otel")]
    #[must_use]
    pub fn with_otel_endpoint(mut self, endpoint: impl AsRef<str>) -> Self {
        self.otel_endpoint = Some(endpoint.as_ref().to_owned());
        self
    }

    #[must_use]
    pub fn with_service_name(mut self, name: impl AsRef<str>) -> Self {
        self.service_name = Some(name.as_ref().to_owned());
        self
    }

    pub fn build(self) -> Result<Observer, ObservabilityError> {
        #[cfg(feature = "otel")]
        let tracer = match (self.tracer, self.otel_endpoint) {
            (Some(tracer), _) => tracer,
            (None, Some(endpoint)) => Arc::new(crate::OtelTracer::new(
                endpoint,
                self.service_name.as_deref().unwrap_or("jyowo"),
            )?),
            (None, None) => Arc::new(NoopTracer),
        };
        #[cfg(not(feature = "otel"))]
        let tracer = self.tracer.unwrap_or_else(|| Arc::new(NoopTracer));

        let usage = self
            .usage
            .unwrap_or_else(|| Arc::new(UsageAccumulator::default()));
        let model_metrics = self
            .model_metrics
            .unwrap_or_else(|| Arc::new(ModelMetricsAccumulator::default()));
        #[cfg(feature = "prometheus")]
        let prometheus = self.prometheus_bind.map(|bind| {
            Arc::new(PrometheusExporter::new(
                bind,
                Arc::clone(&usage),
                Arc::clone(&model_metrics),
            ))
        });

        Ok(Observer {
            tracer,
            usage,
            model_metrics,
            redactor: self.redactor.unwrap_or_else(default_redactor),
            #[cfg(feature = "replay")]
            replay: self.replay,
            #[cfg(feature = "prometheus")]
            prometheus,
            prometheus_bind: self.prometheus_bind,
        })
    }
}

#[cfg(feature = "redactor")]
fn default_redactor() -> Arc<dyn Redactor> {
    Arc::new(crate::DefaultRedactor::default())
}

#[cfg(not(feature = "redactor"))]
fn default_redactor() -> Arc<dyn Redactor> {
    Arc::new(NoopRedactor)
}

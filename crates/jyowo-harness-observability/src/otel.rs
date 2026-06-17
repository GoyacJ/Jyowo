use opentelemetry::{
    trace::{Span as OtelSpanTrait, Tracer as OtelTracerTrait, TracerProvider as _},
    KeyValue,
};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::TracerProvider;

use crate::{
    AttributeValue, Span, SpanAttributes, SpanStatus, TraceCarrier, TraceContext, TraceId, Tracer,
};

#[derive(Debug)]
pub struct OtelTracer {
    endpoint: String,
    service_name: String,
    _provider: TracerProvider,
    tracer: opentelemetry_sdk::trace::Tracer,
}

impl OtelTracer {
    pub fn new(
        endpoint: impl Into<String>,
        service_name: impl Into<String>,
    ) -> Result<Self, crate::ObservabilityError> {
        let endpoint = endpoint.into();
        let service_name = service_name.into();
        if endpoint.trim().is_empty() {
            return Err(crate::ObservabilityError::TracerInit(
                "otel endpoint is empty".to_owned(),
            ));
        }
        if service_name.trim().is_empty() {
            return Err(crate::ObservabilityError::TracerInit(
                "otel service name is empty".to_owned(),
            ));
        }
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint.clone())
            .build()
            .map_err(|error| crate::ObservabilityError::TracerInit(error.to_string()))?;
        let provider = TracerProvider::builder()
            .with_simple_exporter(exporter)
            .build();
        let tracer = provider.tracer(service_name.clone());
        Ok(Self {
            endpoint,
            service_name,
            _provider: provider,
            tracer,
        })
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    #[must_use]
    pub fn service_name(&self) -> &str {
        &self.service_name
    }
}

impl Tracer for OtelTracer {
    fn start_span(&self, name: &str, attrs: SpanAttributes) -> Box<dyn Span> {
        let mut attrs = attrs;
        attrs.attrs.insert(
            "otel.endpoint".to_owned(),
            AttributeValue::String(self.endpoint.clone()),
        );
        attrs.attrs.insert(
            "service.name".to_owned(),
            AttributeValue::String(self.service_name.clone()),
        );
        let mut span = self.tracer.start(name.to_owned());
        for (key, value) in attrs.attrs {
            span.set_attribute(to_key_value(key, value));
        }
        Box::new(OtelSpan::new(name, span))
    }

    fn inject_context(&self, carrier: &mut dyn TraceCarrier) {
        let mut span = self.tracer.start("harness.trace_context".to_owned());
        OtelSpan::trace_context_from_span(&span).inject(carrier);
        span.end();
    }

    fn extract_context(&self, carrier: &dyn TraceCarrier) -> Option<TraceContext> {
        TraceContext::extract(carrier)
    }
}

struct OtelSpan {
    context: TraceContext,
    inner: opentelemetry_sdk::trace::Span,
}

impl OtelSpan {
    fn new(_name: &str, inner: opentelemetry_sdk::trace::Span) -> Self {
        let context = Self::trace_context_from_span(&inner);
        Self { context, inner }
    }

    fn trace_context_from_span(span: &opentelemetry_sdk::trace::Span) -> TraceContext {
        let context = span.span_context();
        TraceContext::new(
            TraceId::new(context.trace_id().to_string()),
            crate::SpanId::new(context.span_id().to_string()),
            None,
        )
    }
}

impl Span for OtelSpan {
    fn context(&self) -> &TraceContext {
        &self.context
    }

    fn set_attribute(&mut self, key: &str, value: AttributeValue) {
        self.inner
            .set_attribute(to_key_value(key.to_owned(), value));
    }

    fn add_event(&mut self, name: &str, attrs: SpanAttributes) {
        self.inner.add_event(
            name.to_owned(),
            attrs
                .attrs
                .into_iter()
                .map(|(key, value)| to_key_value(key, value))
                .collect(),
        );
    }

    fn set_status(&mut self, status: SpanStatus) {
        let status = match status {
            SpanStatus::Unset | SpanStatus::Ok => opentelemetry::trace::Status::Ok,
            SpanStatus::Error(message) => opentelemetry::trace::Status::error(message),
        };
        self.inner.set_status(status);
    }

    fn end(mut self: Box<Self>) {
        self.inner.end();
    }
}

fn to_key_value(key: String, value: AttributeValue) -> KeyValue {
    match value {
        AttributeValue::String(value) => KeyValue::new(key, value),
        AttributeValue::Int(value) => KeyValue::new(key, value),
        AttributeValue::Float(value) => KeyValue::new(key, value),
        AttributeValue::Bool(value) => KeyValue::new(key, value),
        AttributeValue::Bytes(value) => KeyValue::new(key, format!("{value:?}")),
    }
}

use std::collections::HashMap;

use harness_observability::{
    AttributeValue, NoopTracer, SpanAttributes, SpanStatus, TraceContext, TraceId, Tracer,
};

#[test]
fn tracer_trait_is_object_safe_and_starts_spans() {
    let tracer: Box<dyn Tracer> = Box::new(NoopTracer);
    let mut span = tracer.start_span(
        "harness.session.run",
        SpanAttributes::new().with("tenant.id", AttributeValue::String("tenant-a".to_owned())),
    );

    assert_eq!(span.context().trace_id.as_str().len(), 32);
    assert_eq!(span.context().span_id.as_str().len(), 16);

    span.set_attribute("run.iteration", AttributeValue::Int(1));
    span.add_event(
        "assistant.delta",
        SpanAttributes::new().with("bytes", AttributeValue::Int(42)),
    );
    span.set_status(SpanStatus::Ok);
    span.end();
}

#[test]
fn trace_context_round_trips_through_w3c_traceparent_carrier() {
    let context = TraceContext::new(
        TraceId::new("0123456789abcdef0123456789abcdef"),
        harness_observability::SpanId::new("0123456789abcdef"),
        None,
    );
    let mut carrier = HashMap::new();

    context.inject(&mut carrier);

    let extracted = TraceContext::extract(&carrier).expect("trace context");
    assert_eq!(extracted, context);
}

#[test]
fn malformed_traceparent_is_ignored() {
    let mut carrier = HashMap::new();
    carrier.insert("traceparent".to_owned(), "00-short-bad-01".to_owned());

    assert!(TraceContext::extract(&carrier).is_none());
}

#[cfg(feature = "otel")]
#[tokio::test]
async fn otel_tracer_validates_endpoint_and_service_name() {
    assert!(harness_observability::OtelTracer::new("", "jyowo").is_err());
    assert!(harness_observability::OtelTracer::new("http://localhost:4317", "").is_err());

    let tracer = harness_observability::OtelTracer::new("http://localhost:4317", "jyowo").unwrap();
    assert_eq!(tracer.endpoint(), "http://localhost:4317");
    assert_eq!(tracer.service_name(), "jyowo");
}

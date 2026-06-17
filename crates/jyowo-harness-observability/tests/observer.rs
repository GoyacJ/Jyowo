use std::sync::Arc;

use harness_contracts::{NoopRedactor, Redactor};
use harness_observability::{
    NoopTracer, Observer, SpanAttributes, Tracer, UsageAccumulator, UsageScope,
};

#[test]
fn observer_builder_assembles_tracer_usage_and_redactor() {
    let tracer = Arc::new(NoopTracer);
    let usage = Arc::new(UsageAccumulator::default());
    let redactor: Arc<dyn Redactor> = Arc::new(NoopRedactor);

    let observer = Observer::builder()
        .with_tracer(tracer.clone())
        .with_usage_accumulator(usage.clone())
        .with_redactor(redactor.clone())
        .build()
        .unwrap();

    assert!(Arc::ptr_eq(&observer.usage, &usage));
    assert_eq!(
        observer
            .redactor
            .redact("plain", &harness_contracts::RedactRules::default()),
        "plain"
    );

    let span = observer
        .tracer
        .start_span("harness.session.run", SpanAttributes::default());
    span.end();

    observer.usage.record(
        UsageScope::Global,
        None,
        harness_contracts::UsageSnapshot {
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_micros: 0,
            tool_calls: 0,
        },
    );
    assert_eq!(observer.usage.snapshot(UsageScope::Global).output_tokens, 2);
}

#[test]
fn observer_is_a_tracer_facade_for_existing_runtime_paths() {
    let observer = Observer::builder().build().unwrap();
    let tracer: &dyn Tracer = &observer;

    let span = tracer.start_span("harness.model.infer", SpanAttributes::default());

    assert_eq!(span.context().trace_id.as_str().len(), 32);
}

#[cfg(feature = "redactor")]
#[test]
fn observer_default_redactor_masks_secrets() {
    let observer = Observer::builder().build().unwrap();

    let redacted = observer.redactor.redact(
        "token sk-abcdefghijklmnopqrstuvwxyz",
        &harness_contracts::RedactRules::default(),
    );

    assert!(!redacted.contains("sk-abcdefghijklmnopqrstuvwxyz"));
}

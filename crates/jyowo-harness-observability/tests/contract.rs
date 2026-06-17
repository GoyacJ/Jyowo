use harness_observability::{NoopTracer, SpanAttributes, SpanStatus, Tracer};

#[test]
fn tracer_contract_is_object_safe() {
    let tracer: Box<dyn Tracer> = Box::new(NoopTracer);
    let mut span = tracer.start_span("harness.contract", SpanAttributes::default());

    span.set_status(SpanStatus::Ok);
    span.end();
}

#[cfg(feature = "redactor")]
#[test]
fn redactor_contract_is_object_safe_and_idempotent() {
    use harness_contracts::{RedactRules, RedactScope, Redactor};
    use harness_observability::{DefaultRedactor, RedactorContractTest};

    let redactor: Box<dyn Redactor> = Box::new(DefaultRedactor::default());
    let rules = RedactRules {
        scope: RedactScope::All,
        ..RedactRules::default()
    };

    RedactorContractTest::assert_idempotent(
        redactor.as_ref(),
        "token sk-abcdefghijklmnopqrstuvwxyz",
        &rules,
    );
}

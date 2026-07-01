use super::*;

#[derive(Default)]
pub struct RecordingTracer {
    pub started: AtomicUsize,
}

impl Tracer for RecordingTracer {
    fn start_span(&self, name: &str, attrs: SpanAttributes) -> Box<dyn Span> {
        assert_eq!(name, "engine.run_turn");
        self.started.fetch_add(1, Ordering::SeqCst);
        Box::new(InMemorySpan::new(name, attrs))
    }

    fn inject_context(&self, _carrier: &mut dyn TraceCarrier) {}

    fn extract_context(&self, carrier: &dyn TraceCarrier) -> Option<TraceContext> {
        TraceContext::extract(carrier)
    }
}

#[derive(Default)]
pub struct RecordingAnyTracer {
    pub spans: Mutex<Vec<RecordedSpan>>,
}

#[derive(Clone)]
pub struct RecordedSpan {
    pub name: String,
    pub attrs: SpanAttributes,
}

impl RecordingAnyTracer {
    pub fn spans(&self) -> Vec<RecordedSpan> {
        self.spans.lock().unwrap().clone()
    }
}

impl Tracer for RecordingAnyTracer {
    fn start_span(&self, name: &str, attrs: SpanAttributes) -> Box<dyn Span> {
        self.spans.lock().unwrap().push(RecordedSpan {
            name: name.to_owned(),
            attrs: attrs.clone(),
        });
        Box::new(InMemorySpan::new(name, attrs))
    }

    fn inject_context(&self, _carrier: &mut dyn TraceCarrier) {}

    fn extract_context(&self, carrier: &dyn TraceCarrier) -> Option<TraceContext> {
        TraceContext::extract(carrier)
    }
}

pub struct ErrorMemoryProvider;

#[async_trait]
impl MemoryStore for ErrorMemoryProvider {
    fn provider_id(&self) -> &str {
        "error-memory"
    }

    async fn recall(
        &self,
        _query: harness_memory::MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        Err(MemoryError::Message(format!(
            "provider failed with secret-token {}",
            "x".repeat(240)
        )))
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        Ok(record.id)
    }

    async fn forget(&self, _id: MemoryId) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn list(
        &self,
        _scope: harness_memory::MemoryListScope,
    ) -> Result<Vec<harness_memory::MemorySummary>, MemoryError> {
        Ok(Vec::new())
    }
}

impl MemoryLifecycle for ErrorMemoryProvider {}

pub struct TestRedactor;

impl Redactor for TestRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.replace("secret-token", "[REDACTED]")
    }
}

pub fn string_attr<'a>(attrs: &'a SpanAttributes, key: &str) -> Option<&'a str> {
    match attrs.attrs.get(key) {
        Some(AttributeValue::String(value)) => Some(value.as_str()),
        _ => None,
    }
}

pub fn int_attr(attrs: &SpanAttributes, key: &str) -> Option<i64> {
    match attrs.attrs.get(key) {
        Some(AttributeValue::Int(value)) => Some(*value),
        _ => None,
    }
}

pub fn bool_attr(attrs: &SpanAttributes, key: &str) -> Option<bool> {
    match attrs.attrs.get(key) {
        Some(AttributeValue::Bool(value)) => Some(*value),
        _ => None,
    }
}

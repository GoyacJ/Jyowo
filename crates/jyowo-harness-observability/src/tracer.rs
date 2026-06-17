use std::{
    collections::HashMap,
    hash::BuildHasher,
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct TraceId(String);

impl TraceId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TraceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct SpanId(String);

impl SpanId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SpanId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct TraceContext {
    pub trace_id: TraceId,
    pub span_id: SpanId,
    pub parent_span_id: Option<SpanId>,
}

impl TraceContext {
    const TRACEPARENT_KEY: &'static str = "traceparent";

    #[must_use]
    pub fn new(trace_id: TraceId, span_id: SpanId, parent_span_id: Option<SpanId>) -> Self {
        Self {
            trace_id,
            span_id,
            parent_span_id,
        }
    }

    pub fn inject(&self, carrier: &mut dyn TraceCarrier) {
        carrier.set(
            Self::TRACEPARENT_KEY,
            format!("00-{}-{}-01", self.trace_id, self.span_id),
        );
    }

    #[must_use]
    pub fn extract(carrier: &dyn TraceCarrier) -> Option<Self> {
        let value = carrier.get(Self::TRACEPARENT_KEY)?;
        let mut parts = value.split('-');
        let version = parts.next()?;
        let trace_id = parts.next()?;
        let span_id = parts.next()?;
        let _flags = parts.next()?;
        if parts.next().is_some()
            || version.len() != 2
            || trace_id.len() != 32
            || span_id.len() != 16
        {
            return None;
        }
        Some(Self::new(
            TraceId::new(trace_id),
            SpanId::new(span_id),
            None,
        ))
    }
}

pub trait TraceCarrier {
    fn set(&mut self, key: &str, value: String);
    fn get(&self, key: &str) -> Option<&str>;
}

impl<S: BuildHasher> TraceCarrier for HashMap<String, String, S> {
    fn set(&mut self, key: &str, value: String) {
        self.insert(key.to_owned(), value);
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.get(key).map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttributeValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpanAttributes {
    pub attrs: HashMap<String, AttributeValue>,
}

impl SpanAttributes {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: AttributeValue) -> Self {
        self.attrs.insert(key.into(), value);
        self
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum SpanStatus {
    #[default]
    Unset,
    Ok,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpanEvent {
    pub name: String,
    pub attrs: SpanAttributes,
}

pub trait Span: Send {
    fn context(&self) -> &TraceContext;
    fn set_attribute(&mut self, key: &str, value: AttributeValue);
    fn add_event(&mut self, name: &str, attrs: SpanAttributes);
    fn set_status(&mut self, status: SpanStatus);
    fn end(self: Box<Self>);
}

pub trait Tracer: Send + Sync + 'static {
    fn start_span(&self, name: &str, attrs: SpanAttributes) -> Box<dyn Span>;
    fn inject_context(&self, carrier: &mut dyn TraceCarrier);
    fn extract_context(&self, carrier: &dyn TraceCarrier) -> Option<TraceContext>;
}

#[derive(Debug, Default)]
pub struct NoopTracer;

impl Tracer for NoopTracer {
    fn start_span(&self, name: &str, attrs: SpanAttributes) -> Box<dyn Span> {
        Box::new(InMemorySpan::new(name, attrs))
    }

    fn inject_context(&self, _carrier: &mut dyn TraceCarrier) {}

    fn extract_context(&self, carrier: &dyn TraceCarrier) -> Option<TraceContext> {
        TraceContext::extract(carrier)
    }
}

#[derive(Debug, Default)]
pub struct ConsoleTracer;

impl Tracer for ConsoleTracer {
    fn start_span(&self, name: &str, attrs: SpanAttributes) -> Box<dyn Span> {
        tracing::debug!(span = name, attrs = ?attrs.attrs, "harness span started");
        Box::new(InMemorySpan::new(name, attrs))
    }

    fn inject_context(&self, _carrier: &mut dyn TraceCarrier) {}

    fn extract_context(&self, carrier: &dyn TraceCarrier) -> Option<TraceContext> {
        TraceContext::extract(carrier)
    }
}

#[derive(Debug, Clone)]
pub struct InMemorySpan {
    name: String,
    context: TraceContext,
    attrs: SpanAttributes,
    events: Vec<SpanEvent>,
    status: SpanStatus,
}

impl InMemorySpan {
    #[must_use]
    pub fn new(name: impl Into<String>, attrs: SpanAttributes) -> Self {
        let sequence = next_span_sequence();
        Self {
            name: name.into(),
            context: TraceContext::new(
                TraceId::new(format!("{sequence:032x}")),
                SpanId::new(format!("{sequence:016x}")),
                None,
            ),
            attrs,
            events: Vec::new(),
            status: SpanStatus::Unset,
        }
    }
}

impl Span for InMemorySpan {
    fn context(&self) -> &TraceContext {
        &self.context
    }

    fn set_attribute(&mut self, key: &str, value: AttributeValue) {
        self.attrs.attrs.insert(key.to_owned(), value);
    }

    fn add_event(&mut self, name: &str, attrs: SpanAttributes) {
        self.events.push(SpanEvent {
            name: name.to_owned(),
            attrs,
        });
    }

    fn set_status(&mut self, status: SpanStatus) {
        self.status = status;
    }

    fn end(self: Box<Self>) {
        tracing::debug!(
            span = self.name,
            trace_id = %self.context.trace_id,
            span_id = %self.context.span_id,
            status = ?self.status,
            events = self.events.len(),
            "harness span ended"
        );
    }
}

fn next_span_sequence() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

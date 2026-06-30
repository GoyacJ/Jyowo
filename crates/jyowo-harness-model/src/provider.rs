use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use futures::stream::BoxStream;
use harness_contracts::{
    BlobStore, ConversationModelCapability, Message, MessageId, ModelError, ModelProtocol,
    ModelRef, PricingId, PricingSnapshotId, RequestId, RunId, SessionId, StopReason, TenantId,
    ToolDescriptor, ToolUseId, UsageSnapshot,
};
use http::HeaderMap;
use rust_decimal::Decimal;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::InferMiddleware;

pub type ModelStream = BoxStream<'static, ModelStreamEvent>;
pub type RetryClassifier = Arc<dyn Fn(&ErrorClass) -> bool + Send + Sync>;

#[async_trait]
pub trait ModelProvider: Send + Sync + 'static {
    fn provider_id(&self) -> &str;
    fn supported_models(&self) -> Vec<ModelDescriptor>;

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError>;

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::Messages
    }

    fn snapshot_for_model(&self, model_id: &str) -> Result<ModelRuntimeSnapshot, ModelError> {
        let provider_id = self.provider_id().to_owned();
        self.supported_models()
            .into_iter()
            .find(|descriptor| {
                descriptor.provider_id == provider_id && descriptor.model_id == model_id
            })
            .map(ModelRuntimeSnapshot::from_descriptor)
            .ok_or_else(|| {
                ModelError::InvalidRequest(format!(
                    "unsupported model id for provider {provider_id}: {model_id}"
                ))
            })
    }

    fn prompt_cache_style(&self) -> PromptCacheStyle {
        PromptCacheStyle::None
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

#[derive(Clone)]
pub struct InferContext {
    pub request_id: RequestId,
    pub tenant_id: TenantId,
    pub session_id: Option<SessionId>,
    pub run_id: Option<RunId>,
    pub cancel: CancellationToken,
    pub deadline: Option<Instant>,
    pub retry_policy: RetryPolicy,
    pub tracing: Option<TraceContext>,
    pub middlewares: Vec<Arc<dyn InferMiddleware>>,
    pub blob_store: Option<Arc<dyn BlobStore>>,
    pub suppress_usage_accounting: bool,
}

impl InferContext {
    pub fn for_test() -> Self {
        Self {
            request_id: RequestId::new(),
            tenant_id: TenantId::SINGLE,
            session_id: None,
            run_id: None,
            cancel: CancellationToken::new(),
            deadline: None,
            retry_policy: RetryPolicy::default(),
            tracing: None,
            middlewares: Vec::new(),
            blob_store: None,
            suppress_usage_accounting: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricingSnapshotResolveContext {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub model_ref: ModelRef,
}

#[async_trait]
pub trait PricingSnapshotResolver: Send + Sync + 'static {
    async fn resolve(&self, context: PricingSnapshotResolveContext) -> Option<PricingSnapshotId>;

    async fn record_miss(&self, _context: PricingSnapshotResolveContext) {}
}

#[derive(Clone)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff: Backoff,
    pub retry_on: RetryClassifier,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff: Backoff::Exponential {
                initial: Duration::from_millis(200),
                factor: 2.0,
                cap: Duration::from_secs(5),
            },
            retry_on: Arc::new(|class| {
                matches!(
                    class,
                    ErrorClass::Transient | ErrorClass::RateLimited { .. }
                )
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Backoff {
    Fixed(Duration),
    Exponential {
        initial: Duration,
        factor: f32,
        cap: Duration,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraceContext {
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelDescriptor {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub protocol: ModelProtocol,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub conversation_capability: ConversationModelCapability,
    pub lifecycle: ModelLifecycle,
    pub pricing: Option<ModelPricing>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRuntimeSnapshot {
    pub provider_id: String,
    pub model_id: String,
    pub protocol: ModelProtocol,
    pub context_window: u32,
    pub conversation_capability: ConversationModelCapability,
    pub lifecycle: ModelLifecycle,
    pub pricing: Option<ModelPricing>,
}

impl ModelRuntimeSnapshot {
    #[must_use]
    pub fn from_descriptor(descriptor: ModelDescriptor) -> Self {
        Self {
            provider_id: descriptor.provider_id,
            model_id: descriptor.model_id,
            protocol: descriptor.protocol,
            context_window: descriptor.context_window,
            conversation_capability: descriptor.conversation_capability,
            lifecycle: descriptor.lifecycle,
            pricing: descriptor.pricing,
        }
    }

    #[must_use]
    pub fn pricing_snapshot_id(&self) -> Option<harness_contracts::PricingSnapshotId> {
        self.pricing
            .as_ref()
            .map(|pricing| harness_contracts::PricingSnapshotId {
                pricing_id: pricing.pricing_id.clone(),
                version: pricing.pricing_version,
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelLifecycle {
    Stable,
    Preview,
    Deprecated { retirement_date: NaiveDate },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelRuntimeStatus {
    Runnable,
    Unsupported { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelInventoryEntry {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub protocol: ModelProtocol,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub conversation_capability: ConversationModelCapability,
    pub lifecycle: ModelLifecycle,
    pub pricing: Option<ModelPricing>,
    pub runtime_status: ModelRuntimeStatus,
}

impl ModelInventoryEntry {
    #[must_use]
    pub fn runnable(descriptor: ModelDescriptor) -> Self {
        Self {
            provider_id: descriptor.provider_id,
            model_id: descriptor.model_id,
            display_name: descriptor.display_name,
            protocol: descriptor.protocol,
            context_window: descriptor.context_window,
            max_output_tokens: descriptor.max_output_tokens,
            conversation_capability: descriptor.conversation_capability,
            lifecycle: descriptor.lifecycle,
            pricing: descriptor.pricing,
            runtime_status: ModelRuntimeStatus::Runnable,
        }
    }

    #[must_use]
    pub fn unsupported(
        provider_id: impl Into<String>,
        model_id: impl Into<String>,
        display_name: impl Into<String>,
        protocol: ModelProtocol,
        conversation_capability: ConversationModelCapability,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            display_name: display_name.into(),
            protocol,
            context_window: 0,
            max_output_tokens: 0,
            conversation_capability,
            lifecycle: ModelLifecycle::Stable,
            pricing: None,
            runtime_status: ModelRuntimeStatus::Unsupported {
                reason: reason.into(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelPricing {
    pub pricing_id: PricingId,
    pub pricing_version: u32,
    pub currency: Currency,
    pub input_per_million: Decimal,
    pub output_per_million: Decimal,
    pub cache_creation_per_million: Option<Decimal>,
    pub cache_read_per_million: Option<Decimal>,
    pub image_per_image: Option<Decimal>,
    pub last_updated: DateTime<Utc>,
    pub source: PricingSource,
    pub billing_mode: BillingMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Currency {
    Usd,
    Cny,
    Eur,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PricingSource {
    Hardcoded,
    ProviderApi,
    ManualOverride,
    BusinessProvided,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BillingMode {
    Standard,
    Cached { cache_read_discount: Ratio },
    Batched { discount: Ratio },
    Tiered { thresholds: Vec<(u64, Decimal)> },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ratio(pub f32);

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRequest {
    pub model_id: String,
    pub messages: Vec<Message>,
    pub tools: Option<Vec<ToolDescriptor>>,
    pub system: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
    pub cache_breakpoints: Vec<CacheBreakpoint>,
    pub protocol: ModelProtocol,
    pub extra: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModelStreamEvent {
    MessageStart {
        message_id: String,
        usage: UsageSnapshot,
    },
    ContentBlockStart {
        index: u32,
        content_type: ContentType,
    },
    ContentBlockDelta {
        index: u32,
        delta: ContentDelta,
    },
    ContentBlockStop {
        index: u32,
    },
    MessageDelta {
        stop_reason: Option<StopReason>,
        usage_delta: UsageSnapshot,
    },
    MessageStop,
    StreamError {
        error: ModelError,
        class: ErrorClass,
        hints: ErrorHints,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Text,
    Thinking,
    ToolUse,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorClass {
    Transient,
    RateLimited { retry_after: Option<Duration> },
    ContextOverflow,
    AuthExpired,
    Fatal,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ErrorHints {
    pub raw_headers: Option<HeaderMap>,
    pub provider_error_code: Option<String>,
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContentDelta {
    Text(String),
    Thinking(ThinkingDelta),
    ReasoningSummary(ReasoningSummaryDelta),
    ToolUseStart {
        id: String,
        name: String,
    },
    ToolUseInputJson(String),
    ToolUseComplete {
        id: ToolUseId,
        name: String,
        input: Value,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThinkingDelta {
    pub text: Option<String>,
    pub provider_native: Option<Value>,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReasoningSummaryDelta {
    pub text: String,
    pub provider_native: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheBreakpoint {
    pub after_message_id: MessageId,
    pub reason: BreakpointReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakpointReason {
    System,
    RecentMessage,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptCacheStyle {
    Anthropic { mode: AnthropicCacheMode },
    OpenAi { auto: bool },
    Gemini { mode: GeminiCacheMode },
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnthropicCacheMode {
    None,
    SystemAnd3,
    Custom(Vec<CacheBreakpoint>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeminiCacheMode {
    None,
    ExternalReferenceOnly,
    Explicit { ttl: Duration, min_tokens: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded { reason: String },
    Unhealthy { reason: String },
}

pub trait ModelErrorExt {
    fn classify(&self) -> ErrorClass;
}

impl ModelErrorExt for ModelError {
    fn classify(&self) -> ErrorClass {
        match self {
            Self::RateLimited(_) => ErrorClass::RateLimited { retry_after: None },
            Self::ContextTooLong { .. } => ErrorClass::ContextOverflow,
            Self::AuthExpired(_) => ErrorClass::AuthExpired,
            Self::ProviderUnavailable(_) | Self::Io(_) => ErrorClass::Transient,
            _ => ErrorClass::Fatal,
        }
    }
}

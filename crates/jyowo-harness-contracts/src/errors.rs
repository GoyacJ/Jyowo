//! Error contracts.
//!
//! SPEC: docs/architecture/harness/crates/harness-contracts.md §3.8

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    BudgetKind, BudgetMetric, HookOutcomeDiscriminant, InconsistentReason, MemoryActor, MemoryId,
    MemoryVisibility, TenantId, ThreatAction, ThreatCategory, ToolCapability,
};

pub type Result<T, E = HarnessError> = std::result::Result<T, E>;

macro_rules! define_error_family {
    ($($name:ident),+ $(,)?) => {
        $(
            #[non_exhaustive]
            #[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema, thiserror::Error)]
            #[serde(rename_all = "snake_case")]
            pub enum $name {
                #[error("{0}")]
                Message(String),
            }
        )+
    };
}

#[non_exhaustive]
#[derive(
    Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema, thiserror::Error,
)]
#[serde(rename_all = "snake_case")]
pub enum ModelError {
    #[error("{0}")]
    Message(String),
    #[error("rate limited: {0}")]
    RateLimited(String),
    #[error("context too long: tokens={tokens}, max={max}")]
    ContextTooLong { tokens: usize, max: usize },
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("all credentials banned")]
    AllCredentialsBanned,
    #[error("aux model not configured")]
    AuxModelNotConfigured,
    #[error("auth expired: {0}")]
    AuthExpired(String),
    #[error("provider unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("unexpected response: {0}")]
    UnexpectedResponse(String),
    #[error("cancelled by caller")]
    Cancelled,
    #[error("deadline exceeded after {0:?}")]
    DeadlineExceeded(std::time::Duration),
    #[error("io: {0}")]
    Io(String),
}

define_error_family! {
    JournalError,
    PermissionError,
    SessionError,
    EngineError,
    PluginError,
    McpError,
}

#[non_exhaustive]
#[derive(
    Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema, thiserror::Error,
)]
#[serde(rename_all = "snake_case")]
pub enum MemoryError {
    #[error("{0}")]
    Message(String),
    #[error("external memory provider slot lock busy")]
    ExternalSlotLockBusy,
    #[error("external memory provider slot occupied")]
    ExternalSlotOccupied,
    #[error("external memory provider is not configured")]
    ExternalProviderNotConfigured,
    #[error("threat detected: pattern={pattern_id} category={category:?} action={action:?}")]
    ThreatDetected {
        pattern_id: String,
        category: ThreatCategory,
        action: ThreatAction,
    },
    #[error("memory not found: {0:?}")]
    NotFound(MemoryId),
    #[error("too large: {bytes} bytes (max {max})")]
    TooLarge { bytes: u64, max: u64 },
    #[error("memdir overflow: {chars} > {threshold}")]
    MemdirOverflow { chars: u64, threshold: u64 },
    #[error("memory recall deadline exceeded: provider={provider}")]
    RecallDeadlineExceeded { provider: String },
    #[error("concurrent write lock failed after {retries} retries")]
    ConcurrentWriteLockFailed { retries: u32 },
    #[error("visibility violation: {actor:?} cannot access {visibility:?}")]
    VisibilityViolation {
        actor: MemoryActor,
        visibility: MemoryVisibility,
    },
    #[error("unsupported memory kind: {0}")]
    UnsupportedKind(String),
    #[error("provider error: {provider}: {source_message}")]
    Provider {
        provider: String,
        source_message: String,
    },
    #[error("io: {0}")]
    Io(String),
}

#[non_exhaustive]
#[derive(
    Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema, thiserror::Error,
)]
#[serde(rename_all = "snake_case")]
pub enum SandboxError {
    #[error("{0}")]
    Message(String),
    #[error("sandbox backend unavailable: {backend}: {detail}")]
    Unavailable { backend: String, detail: String },
    #[error("sandbox capability mismatch: {capability}: {detail}")]
    CapabilityMismatch { capability: String, detail: String },
    #[error("sandbox timeout: {detail}")]
    Timeout { detail: String },
    #[error("sandbox inactivity timeout: {detail}")]
    InactivityTimeout { detail: String },
    #[error("sandbox output budget exceeded: limit={limit}")]
    OutputBudgetExceeded { limit: u64 },
    #[error("sandbox host path denied: {path}")]
    HostPathDenied { path: String },
    #[error("sandbox resource limit exceeded: {limit}: {detail}")]
    ResourceLimitExceeded { limit: String, detail: String },
    #[error("sandbox snapshot unsupported: {kind}")]
    SnapshotUnsupported { kind: String },
    #[error("sandbox container lifecycle error: {detail}")]
    ContainerLifecycleError { detail: String },
    #[error("sandbox workspace sync failed: direction={direction} program={program}: {detail}")]
    WorkspaceSyncFailed {
        direction: String,
        program: String,
        detail: String,
    },
    #[error("code runtime: {detail}")]
    CodeRuntime { detail: String },
}

#[non_exhaustive]
#[derive(
    Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema, thiserror::Error,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextError {
    #[error("{0}")]
    Message(String),
    #[error("offload failed: {0}")]
    OffloadFailed(String),
    #[error("internal: {0}")]
    Internal(String),
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransportFailureKind {
    SsrfBlocked,
    AllowlistMiss,
    ProtocolVersionMismatch,
    BodyTooLarge,
    NetworkError,
    NonZeroExit { code: i32 },
    UnsupportedLimit { limit: String },
}

#[non_exhaustive]
#[derive(
    Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema, thiserror::Error,
)]
#[serde(rename_all = "snake_case")]
pub enum HookError {
    #[error("{0}")]
    Message(String),
    #[error("handler timeout: {handler_id}")]
    Timeout { handler_id: String },
    #[error("handler error: {handler_id}: {cause}")]
    HandlerError { handler_id: String, cause: String },
    #[error("handler panicked: {handler_id}")]
    Panicked { handler_id: String, snippet: String },
    #[error("outcome inconsistent: {handler_id}: {reason:?}")]
    Inconsistent {
        handler_id: String,
        reason: InconsistentReason,
    },
    #[error("outcome unsupported: {handler_id}: {kind:?}")]
    Unsupported {
        handler_id: String,
        kind: HookOutcomeDiscriminant,
    },
    #[error("protocol parse: {0}")]
    ProtocolParse(String),
    #[error("transport: {kind:?}: {detail}")]
    Transport {
        kind: TransportFailureKind,
        detail: String,
    },
    #[error("unauthorized: {0}")]
    Unauthorized(String),
}

#[non_exhaustive]
#[derive(
    Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema, thiserror::Error,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolError {
    #[error("{0}")]
    Message(String),
    #[error("validation: {0}")]
    Validation(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("sandbox: {0}")]
    Sandbox(SandboxError),
    #[error("timeout")]
    Timeout,
    #[error("interrupted")]
    Interrupted,
    #[error("result too large: {original} {metric:?} > {limit} {metric:?}")]
    ResultTooLarge {
        original: u64,
        limit: u64,
        metric: BudgetMetric,
    },
    #[error("offload failed: {0}")]
    OffloadFailed(String),
    #[error("required capability missing: {0}")]
    CapabilityMissing(ToolCapability),
    #[error("dynamic schema resolution failed: {0}")]
    SchemaResolution(String),
    #[error("tool deferral required but tool search is disabled: {tool}")]
    DeferralRequired { tool: String },
    #[error("internal: {0}")]
    Internal(String),
}

#[non_exhaustive]
#[derive(
    Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema, thiserror::Error,
)]
#[serde(rename_all = "snake_case")]
pub enum HarnessError {
    #[error("prompt cache locked for running session")]
    PromptCacheLocked,
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("tool not found: {0}")]
    ToolNotFound(String),
    #[error("invalid tenant: {0:?}")]
    InvalidTenant(TenantId),
    #[error("budget exhausted: {0:?}")]
    BudgetExhausted(BudgetKind),
    #[error("interrupted by user")]
    Interrupted,
    #[error("model: {0}")]
    Model(ModelError),
    #[error("journal: {0}")]
    Journal(JournalError),
    #[error("sandbox: {0}")]
    Sandbox(SandboxError),
    #[error("permission: {0}")]
    Permission(PermissionError),
    #[error("memory: {0}")]
    Memory(MemoryError),
    #[error("tool: {0}")]
    Tool(ToolError),
    #[error("session: {0}")]
    Session(SessionError),
    #[error("engine: {0}")]
    Engine(EngineError),
    #[error("plugin: {0}")]
    Plugin(PluginError),
    #[error("mcp: {0}")]
    Mcp(McpError),
    #[error("hook: {0}")]
    Hook(HookError),
    #[error("context: {0}")]
    Context(ContextError),
    #[error("tenant mismatch")]
    TenantMismatch,
    #[error("internal error: {0}")]
    Internal(String),
    #[error("other: {0}")]
    Other(String),
}

macro_rules! impl_from_family {
    ($($variant:ident($name:ident)),+ $(,)?) => {
        $(
            impl From<$name> for HarnessError {
                fn from(value: $name) -> Self {
                    Self::$variant(value)
                }
            }
        )+
    };
}

impl_from_family! {
    Model(ModelError),
    Journal(JournalError),
    Sandbox(SandboxError),
    Permission(PermissionError),
    Memory(MemoryError),
    Tool(ToolError),
    Session(SessionError),
    Engine(EngineError),
    Plugin(PluginError),
    Mcp(McpError),
    Hook(HookError),
    Context(ContextError),
}

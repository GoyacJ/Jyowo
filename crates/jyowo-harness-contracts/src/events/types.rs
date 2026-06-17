use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::*;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UsageSnapshot {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost_micros: u64,
    #[serde(default)]
    pub tool_calls: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventPayload {
    #[serde(default)]
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CacheImpact {
    pub prompt_cache_invalidated: bool,
    pub reason: Option<String>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ForkReason {
    UserRequested,
    Compaction,
    HotReload,
    Isolation,
    RetryFromCheckpoint(JournalOffset),
}

pub type DeltaHash = [u8; 32];
pub type HandlerId = String;
pub type SchemaId = String;
pub type CompactStrategyId = String;
pub type PermissionRequestId = RequestId;
pub type PricingId = String;

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct PricingSnapshotId {
    pub pricing_id: PricingId,
    pub version: u32,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ModelRef {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct AgentRef {
    pub id: AgentId,
    pub name: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ContentHash(pub [u8; 32]);

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct MemoryActor {
    pub tenant_id: TenantId,
    pub user_id: Option<String>,
    pub team_id: Option<TeamId>,
    pub session_id: Option<SessionId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct UserMessageView<'a> {
    pub text: &'a str,
    pub turn: u32,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct MessageView<'a> {
    pub role: MessageRole,
    pub text_snippet: &'a str,
    pub tool_use_id: Option<ToolUseId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct SessionSummaryView<'a> {
    pub end_reason: EndReason,
    pub turn_count: u32,
    pub tool_use_count: u32,
    pub usage: UsageSnapshot,
    pub final_assistant_text: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct MemorySessionCtx<'a> {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub workspace_id: Option<WorkspaceId>,
    pub user_id: Option<&'a str>,
    pub team_id: Option<TeamId>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct MemoryWriteTarget {
    pub kind: MemoryKind,
    pub visibility: MemoryVisibility,
    pub destination: WriteDestination,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WriteDestination {
    Memdir(MemdirFileTag),
    External { provider_id: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemdirFileTag {
    Memory,
    User,
    Dreams,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubagentTerminationReason {
    NaturalCompletion,
    ParentCancelled,
    AdminInterrupted { admin_id: String },
    Stalled { silent_for_ms: u64 },
    BridgeBroken,
    Failed { detail: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    Completed,
    Cancelled,
    Failed,
    Stalled,
    MaxIterationsReached,
    MaxBudget(BudgetKind),
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TopologyKind {
    CoordinatorWorker,
    PeerToPeer,
    RoleRouted,
    Custom(String),
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamTerminationReason {
    Completed,
    Cancelled,
    Error(String),
    MemberFailed,
    IdleTimeout,
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CompactionHandoff {
    pub active_task_ref: BlobRef,
    pub remaining_budget: RemainingBudget,
    pub pending_tool_uses: Vec<ToolUseId>,
    pub outstanding_permissions: Vec<PermissionRequestId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RemainingBudget {
    pub iterations_remaining: u32,
    pub tokens_remaining_in_session: u64,
    pub wall_clock_deadline: Option<DateTime<Utc>>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompactTrigger {
    SoftBudget,
    HardBudget,
    ProviderReport { reported_tokens: u64 },
    UserCommand,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompactOutcome {
    Succeeded,
    DegradedNoAuxProvider,
    DegradedAuxFailure { failure_count: u32 },
    ReactiveAttemptFailed,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContextStageId {
    ToolResultBudget,
    Snip,
    Microcompact,
    Collapse,
    Autocompact,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContextStageOutcome {
    NoChange,
    Modified,
    Forked { child: SessionId },
    SkippedNoAuxProvider,
    SkippedAuxCooldown { until_turn: u32 },
    Failed { reason: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BudgetExceedanceSource {
    LocalEstimate,
    ProviderReport { reported_tokens: u64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxPolicySummary {
    pub mode: SandboxMode,
    pub scope: SandboxScope,
    pub network: NetworkAccess,
    pub resource_limits: ResourceLimits,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SandboxExitStatus {
    Code(i32),
    Signal(i32),
    Timeout,
    InactivityTimeout,
    OutputBudgetExceeded,
    Cancelled,
    BackendError,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SandboxOutputStream {
    Stdout,
    Stderr,
    Combined,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxOverflowSummary {
    pub stream: SandboxOutputStream,
    pub original_bytes: u64,
    pub effective_limit: u64,
    pub blob_ref: Option<BlobRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ContainerRef {
    pub backend_kind: String,
    pub container_id: String,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContainerLifecycleState {
    Provisioning,
    Ready,
    InUse,
    Idle,
    Stopping,
    Stopped,
    Failed,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContainerLifecycleReason {
    SessionAttached,
    SessionDetached,
    PoolReused,
    PoolEvicted,
    HealthCheckFailed,
    SnapshotRestore,
    Manual,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HookEventKind {
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    PermissionRequest,
    SessionStart,
    Setup,
    SessionEnd,
    SubagentStart,
    SubagentStop,
    Notification,
    PreLlmCall,
    PostLlmCall,
    PreApiRequest,
    PostApiRequest,
    TransformToolResult,
    TransformTerminalOutput,
    Elicitation,
    PreToolSearch,
    PostToolSearchMaterialize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HookOutcomeSummary {
    pub continued: bool,
    pub blocked_reason: Option<String>,
    pub rewrote_input: bool,
    pub overrode_permission: Option<Decision>,
    pub added_context_bytes: Option<u64>,
    pub transformed: bool,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HookFailureMode {
    FailOpen,
    FailClosed,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HookFailureCauseKind {
    Unsupported,
    Inconsistent,
    Panicked,
    Timeout,
    Transport,
    Unauthorized,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HookOutcomeDiscriminant {
    Continue,
    Block,
    PreToolUse,
    RewriteInput,
    OverridePermission,
    AddContext,
    Transform,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InconsistentReason {
    PreToolUseBlockExclusive,
    PromptCacheViolation,
    SchemaInvalid {
        schema_id: SchemaId,
        message: String,
    },
    ContextPatchTooLarge {
        limit_bytes: u64,
        actual_bytes: u64,
    },
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct HookPermissionConflictParticipant {
    pub handler_id: HandlerId,
    pub decision: Decision,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceTamperReason {
    SignatureMismatch,
    AlgorithmDowngrade,
    UnknownKeyId,
    MissingSignature,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SuppressionReason {
    JoinedInFlight,
    RecentlyAllowed,
    RecentlyDenied,
    RecentlyTimedOut,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddedRefusedReason {
    NotWhitelisted,
    SelfReentrant,
    CapabilityDenied,
    PropertyViolation,
    PermissionDenied,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SteeringDropReason {
    Capacity,
    TtlExpired,
    DedupHit,
    RunEnded,
    SessionEnded,
    PluginDenied,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpConnectionLostReason {
    Network(String),
    AuthFailure(String),
    HandshakeMismatch(String),
    StdioProcessExited {
        exit_code: Option<i32>,
        signal: Option<i32>,
    },
    Shutdown,
    Other(String),
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ElicitationSchemaSummary {
    pub field_count: u16,
    pub required_count: u16,
    pub has_secret_field: bool,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ElicitationOutcome {
    Provided { value_hash: [u8; 32] },
    UserDeclined,
    Timeout,
    Invalid { reason: String },
    NoHandlerRegistered,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolsListChangedDisposition {
    DeferredApplied,
    PendingForReload,
    NoChange,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpResourceUpdateKind {
    ListChanged { added: u32, removed: u32 },
    ResourceUpdated { uri: String },
    PromptsListChanged { added: u32, removed: u32 },
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SamplingOutcome {
    Completed,
    Denied { reason: SamplingDenyReason },
    BudgetExceeded { dimension: SamplingBudgetDimension },
    RateLimited,
    UpstreamError { code: i32, message: String },
    Cancelled,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SamplingDenyReason {
    PolicyDenied,
    ApprovalDenied,
    ModelNotAllowed,
    PermissionModeBlocked,
    InlineUserSourceRefused,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SamplingBudgetDimension {
    PerRequestInputTokens,
    PerRequestOutputTokens,
    PerRequestTimeout,
    PerRequestToolRounds,
    PerServerSessionInput,
    PerServerSessionOutput,
    PerSessionInput,
    PerSessionOutput,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginRejectedReason {
    TrustPolicy,
    ManifestInvalid,
    CapabilityDenied,
    Duplicate,
    Other(String),
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct PluginCapabilitiesSummary {
    pub tools: u16,
    pub hooks: u16,
    pub mcp_servers: u16,
    pub skills: u16,
    #[serde(default)]
    pub steering: bool,
    pub memory_provider: bool,
    pub coordinator: bool,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ManifestOriginRef {
    File { path: String },
    CargoExtension { binary: String },
    RemoteRegistry { endpoint: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PluginLifecycleStateDiscriminant {
    Validated,
    Activating,
    Activated,
    Deactivating,
    Deactivated,
    Rejected,
    Failed,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RejectionReason {
    SignatureInvalid {
        details: String,
    },
    UnknownSigner {
        signer: String,
    },
    SignerRevoked {
        signer: String,
        revoked_at: DateTime<Utc>,
    },
    TrustMismatch {
        declared: TrustLevel,
        source: String,
    },
    NamespaceConflict {
        details: String,
    },
    DependencyUnsatisfied {
        dependency: String,
        requirement: String,
    },
    DependencyCycle {
        cycle: Vec<String>,
    },
    HarnessVersionIncompatible {
        required: String,
        actual: String,
    },
    SlotOccupied {
        slot: String,
        occupant: String,
    },
    AdmissionDenied {
        policy: String,
    },
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContextVisibility {
    All,
    Allowlist(Vec<AgentId>),
    AllowlistQuote(Vec<AgentId>),
    Private,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemberLeaveReason {
    GoalAchieved,
    QuotaExceeded,
    Interrupted,
    Error(String),
    Removed,
    StalledRemoved,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StalledAction {
    Reported,
    Interrupted,
    Removed,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Recipient {
    Agent(AgentId),
    Role(String),
    Broadcast,
    Coordinator,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessagePayload {
    Text(String),
    Structured(Value),
    Request { reply_to: MessageId },
    Response { in_reply_to: MessageId, body: Value },
    Handoff { to: AgentId, summary: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoutingPolicyKind {
    Direct,
    Role,
    Broadcast,
    Coordinator,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DeferredToolHint {
    pub name: ToolName,
    pub hint: Option<String>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolPoolChangeSource {
    InitialClassification,
    McpListChanged { server_id: McpServerId },
    PluginRegistration { plugin_id: String },
    SkillHotReload { skill_id: String },
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct SchemaVersionRange {
    pub min: u32,
    pub max: u32,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ManifestValidationFailure {
    SyntaxError {
        details: String,
    },
    SchemaViolation {
        json_pointer: String,
        details: String,
    },
    UnsupportedSchemaVersion {
        found: u32,
        supported: SchemaVersionRange,
    },
    CargoExtensionMetadataMalformed {
        details: String,
    },
    RemoteIntegrityMismatch {
        expected_etag: String,
        got_etag: Option<String>,
    },
}

pub fn now() -> DateTime<Utc> {
    Utc::now()
}

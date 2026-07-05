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
pub struct MemoryActorContext {
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

impl Default for SandboxPolicySummary {
    fn default() -> Self {
        Self {
            mode: SandboxMode::None,
            scope: SandboxScope::WorkspaceOnly,
            network: NetworkAccess::None,
            resource_limits: ResourceLimits {
                max_memory_bytes: None,
                max_cpu_cores: None,
                max_pids: None,
                max_wall_clock_ms: None,
                max_open_files: None,
            },
        }
    }
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

// ── Memory Platform Type Aliases ──

pub type MemoryProviderId = String;
pub type MemoryOriginName = String;
pub type MemoryOriginLabel = String;
pub type MemoryPageCursor = String;

// ── Memory Platform Structs ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<std::time::Duration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default = "default_source_trust")]
    pub source_trust: f64,
}

fn default_source_trust() -> f64 {
    0.5
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRecord {
    pub id: MemoryId,
    pub tenant_id: TenantId,
    pub kind: MemoryKind,
    pub visibility: MemoryVisibility,
    pub content: String,
    pub metadata: MemoryMetadata,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRecordDraft {
    pub kind: MemoryKind,
    pub visibility: MemoryVisibility,
    pub content: String,
    pub metadata: MemoryMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryToolVisibility {
    User,
    Tenant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryToolDraft {
    pub kind: MemoryKind,
    pub visibility: MemoryToolVisibility,
    pub content: String,
    #[serde(default = "default_memory_metadata")]
    pub metadata: MemoryMetadata,
}

fn default_memory_metadata() -> MemoryMetadata {
    MemoryMetadata {
        ttl: None,
        tags: Vec::new(),
        source_trust: default_source_trust(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryEvidence {
    pub source: MemorySource,
    pub origin: MemoryEvidenceOrigin,
    pub content_hash: ContentHash,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<MessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<ToolUseId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryCandidate {
    pub id: MemoryCandidateId,
    pub tenant_id: TenantId,
    pub state: MemoryCandidateState,
    #[serde(default = "default_memory_candidate_operation")]
    pub operation: MemoryCandidateOperation,
    pub proposed_record: MemoryRecordDraft,
    pub evidence: MemoryEvidence,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCandidateOperation {
    Create,
    Update { memory_id: MemoryId },
    Delete { memory_id: MemoryId },
}

fn default_memory_candidate_operation() -> MemoryCandidateOperation {
    MemoryCandidateOperation::Create
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryScoreBreakdown {
    pub lexical_score: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector_score: Option<f32>,
    pub confidence_score: f32,
    pub recency_score: f32,
    pub access_score: f32,
    pub source_trust_score: f32,
    pub explicit_selection_boost: f32,
    pub final_score: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryProviderTrace {
    pub provider_id: MemoryProviderId,
    pub trust_level: MemoryProviderTrust,
    pub readable: bool,
    pub writable: bool,
    pub requested_count: u32,
    pub returned_count: u32,
    pub timed_out: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    pub latency_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryCandidateTrace {
    pub memory_id: MemoryId,
    pub provider_id: MemoryProviderId,
    pub content_hash: ContentHash,
    pub score: MemoryScoreBreakdown,
    pub policy_decision: MemoryPolicyDecision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryInjectedTrace {
    pub memory_id: MemoryId,
    pub provider_id: MemoryProviderId,
    pub content_hash: ContentHash,
    pub injected_chars: u32,
    pub fence_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryDroppedTrace {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<MemoryId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<MemoryProviderId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<ContentHash>,
    pub reason: MemoryDropReason,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRecallTrace {
    pub trace_id: MemoryTraceId,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub turn: u32,
    pub query_text_hash: ContentHash,
    pub provider_results: Vec<MemoryProviderTrace>,
    pub candidates: Vec<MemoryCandidateTrace>,
    pub injected: Vec<MemoryInjectedTrace>,
    pub dropped: Vec<MemoryDroppedTrace>,
    pub redacted_count: u32,
    pub injected_chars: u32,
    pub deadline_used_ms: u32,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryGlobalSettings {
    pub use_memories: bool,
    pub generate_memories: bool,
    pub disable_generation_when_external_context_used: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_days: Option<u32>,
    pub max_memory_bytes: u64,
    pub max_recall_records_per_turn: u32,
    pub max_recall_chars_per_turn: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryThreadSettings {
    pub session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_memories: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generate_memories: Option<bool>,
    pub memory_mode: MemoryThreadMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryPermissionContext {
    pub explicit_user_instruction: bool,
    #[serde(default)]
    pub include_raw_content: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_plan_id: Option<ActionPlanId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_ticket_id: Option<AuthorizationTicketId>,
    pub non_interactive_policy_grant: bool,
}

// ── Memory Tool Structs ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryToolArgs {
    #[serde(flatten)]
    pub action: MemoryToolAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryToolRuntimeContext {
    pub actor: MemoryActor,
    pub permission_context: MemoryPermissionContext,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub provider_policy: MemoryProviderSelectionPolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryToolRequest {
    pub args: MemoryToolArgs,
    pub runtime: MemoryToolRuntimeContext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemorySearchRequest {
    pub query: String,
    pub max_records: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<MemoryToolVisibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<MemoryPageCursor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryReadRequest {
    pub memory_id: MemoryId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryToolCreateArgs {
    pub draft: MemoryToolDraft,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryToolUpdateArgs {
    pub memory_id: MemoryId,
    pub draft: MemoryToolDraft,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryDeleteRequest {
    pub memory_id: MemoryId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryListRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<MemoryToolVisibility>,
    #[serde(default)]
    pub include_expired: bool,
    #[serde(default)]
    pub include_deleted: bool,
    pub limit: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<MemoryPageCursor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryToolProposeArgs {
    pub draft: MemoryToolDraft,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryToolResponse {
    pub action: String,
    pub state: MemoryToolState,
    pub memory_ids: Vec<MemoryId>,
    pub candidate_ids: Vec<MemoryCandidateId>,
    pub records: Vec<MemoryToolRecordView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<MemoryPageCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_plan_id: Option<ActionPlanId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denial: Option<MemoryToolDenial>,
    pub redaction: MemoryRedactionSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<MemoryTraceId>,
    pub takes_effect: MemoryTakesEffect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryToolRecordView {
    pub memory_id: MemoryId,
    pub provider_id: MemoryProviderId,
    pub kind: MemoryKind,
    pub visibility: MemoryVisibility,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redacted_content: Option<String>,
    pub content_hash: ContentHash,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<MemoryScoreBreakdown>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryToolDenial {
    pub reason: MemoryPolicyDenyReason,
    pub safe_message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_plan_id: Option<ActionPlanId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRedactionSummary {
    pub redacted_count: u32,
    pub dropped_count: u32,
}

// ── Memory Provider Descriptor ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryProviderDescriptor {
    pub provider_id: MemoryProviderId,
    pub provider_kind: MemoryProviderKind,
    pub priority: i32,
    pub trust_level: MemoryProviderTrust,
    pub tenant_scope: Option<TenantId>,
    pub workspace_scope: Option<WorkspaceId>,
    pub durability: MemoryProviderDurability,
    pub readable: bool,
    pub writable: bool,
    pub allowed_visibility: Vec<MemoryVisibilityClass>,
    pub supports_evidence: bool,
    pub supports_raw_content_export: bool,
    pub timeout_ms: u32,
    pub max_records_per_recall: u32,
    pub max_chars_per_recall: u32,
    pub max_bytes_per_record: u64,
}

// ── IPC Contracts ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GetMemorySettingsRequest {
    pub tenant_id: TenantId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GetMemorySettingsResponse {
    pub settings: MemoryGlobalSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UpdateMemorySettingsRequest {
    pub tenant_id: TenantId,
    pub settings: MemoryGlobalSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UpdateMemorySettingsResponse {
    pub settings: MemoryGlobalSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GetThreadMemorySettingsRequest {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GetThreadMemorySettingsResponse {
    pub settings: MemoryThreadSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UpdateThreadMemorySettingsRequest {
    pub tenant_id: TenantId,
    pub settings: MemoryThreadSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UpdateThreadMemorySettingsResponse {
    pub settings: MemoryThreadSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ListMemoryCandidatesRequest {
    pub tenant_id: TenantId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<MemoryCandidateState>,
    pub limit: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<MemoryPageCursor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ListMemoryCandidatesResponse {
    pub candidates: Vec<MemoryCandidateListItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<MemoryPageCursor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryCandidateListItem {
    pub id: MemoryCandidateId,
    pub state: MemoryCandidateState,
    #[serde(default = "default_memory_candidate_operation")]
    pub operation: MemoryCandidateOperation,
    pub proposed_record: MemoryRecordDraft,
    pub evidence: MemoryEvidence,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ApproveMemoryCandidateRequest {
    pub tenant_id: TenantId,
    pub candidate_id: MemoryCandidateId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_plan_id: Option<ActionPlanId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ApproveMemoryCandidateResponse {
    pub candidate: MemoryCandidate,
    pub memory_id: MemoryId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RejectMemoryCandidateRequest {
    pub tenant_id: TenantId,
    pub candidate_id: MemoryCandidateId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RejectMemoryCandidateResponse {
    pub candidate: MemoryCandidate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MergeMemoryCandidateRequest {
    pub tenant_id: TenantId,
    pub candidate_ids: Vec<MemoryCandidateId>,
    pub merged_record: MemoryRecordDraft,
    pub evidence: MemoryEvidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_plan_id: Option<ActionPlanId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MergeMemoryCandidateResponse {
    pub candidate_ids: Vec<MemoryCandidateId>,
    pub memory_id: MemoryId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ListMemoryRecallTracesRequest {
    pub tenant_id: TenantId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    pub limit: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<MemoryPageCursor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ListMemoryRecallTracesResponse {
    pub traces: Vec<MemoryRecallTraceSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<MemoryPageCursor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRecallTraceSummary {
    pub trace_id: MemoryTraceId,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub injected_count: u32,
    pub dropped_count: u32,
    pub redacted_count: u32,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GetMemoryRecallTraceRequest {
    pub tenant_id: TenantId,
    pub trace_id: MemoryTraceId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GetMemoryRecallTraceResponse {
    pub trace: MemoryRecallTrace,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GetModelRequestPreviewRequest {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<MemoryTraceId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GetModelRequestPreviewResponse {
    pub preview: MemoryModelRequestPreview,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryModelRequestPreview {
    pub session_id: SessionId,
    pub run_id: RunId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<MemoryTraceId>,
    pub sections: Vec<MemoryModelRequestPreviewSection>,
    pub redacted_count: u32,
    pub token_estimate: u64,
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub policy_decisions: Vec<String>,
    pub content_hash: ContentHash,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryModelRequestPreviewSection {
    pub source: MemorySource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<MemoryProviderId>,
    pub memory_ids: Vec<MemoryId>,
    pub redacted_content: String,
}

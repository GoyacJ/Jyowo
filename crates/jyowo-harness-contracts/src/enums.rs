use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, Utc};
use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::events::types::{
    ContentHash, MemoryDeleteRequest, MemoryListRequest, MemoryReadRequest, MemorySearchRequest,
    MemoryToolCreateArgs, MemoryToolProposeArgs, MemoryToolUpdateArgs,
};
use crate::ids::*;

#[non_exhaustive]
#[derive(
    Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema, strum::EnumDiscriminants,
)]
#[strum_discriminants(
    name(DecisionDiscriminant),
    derive(Hash, Serialize, Deserialize, JsonSchema)
)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    AllowOnce,
    AllowSession,
    AllowPermanent,
    DenyOnce,
    DenyPermanent,
    Escalate,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Default,
    Plan,
    AcceptEdits,
    BypassPermissions,
    DontAsk,
    Auto,
}

impl Default for PermissionMode {
    fn default() -> Self {
        Self::Default
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    AdminTrusted,
    UserControlled,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Completed,
    MaxIterationsReached,
    TokenBudgetExhausted,
    BudgetExhausted(BudgetKind),
    Interrupted,
    Cancelled { initiator: CancelInitiator },
    Error(String),
    Compacted,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CancelInitiator {
    User,
    Parent,
    System { reason: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxIterations,
    Interrupt,
    ContentFiltered,
    ProviderResourceExhausted,
    Error(String),
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeferPolicy {
    AlwaysLoad,
    AutoDefer,
    ForceDefer,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolSearchMode {
    Always,
    Auto {
        ratio: f32,
        min_absolute_tokens: u32,
    },
    Disabled,
}

impl ToolSearchMode {
    #[must_use]
    pub fn min_absolute_tokens(&self) -> u32 {
        match self {
            Self::Auto {
                min_absolute_tokens,
                ..
            } => *min_absolute_tokens,
            Self::Always | Self::Disabled => 0,
        }
    }
}

impl Default for ToolSearchMode {
    fn default() -> Self {
        Self::Auto {
            ratio: 0.10,
            min_absolute_tokens: 4_000,
        }
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolSearchQueryKind {
    Select,
    Keyword,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolGroup {
    FileSystem,
    Search,
    Network,
    Shell,
    Git,
    Worktree,
    Session,
    Artifact,
    Browser,
    Computer,
    Image,
    Notebook,
    Lsp,
    Automation,
    Workflow,
    Agent,
    Coordinator,
    Memory,
    Clarification,
    Meta,
    Custom(String),
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecisionScope {
    ExactCommand {
        command: String,
        cwd: Option<PathBuf>,
    },
    ExactArgs(Value),
    ToolName(String),
    Category(String),
    PathPrefix(PathBuf),
    GlobPattern(String),
    ExecuteCodeScript {
        script_hash: [u8; 32],
    },
    Any,
}

#[non_exhaustive]
#[derive(
    Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema, strum::EnumDiscriminants,
)]
#[strum_discriminants(
    name(DecidedByDiscriminant),
    derive(Hash, Serialize, Deserialize, JsonSchema)
)]
#[serde(rename_all = "snake_case")]
pub enum DecidedBy {
    User,
    Rule {
        rule_id: String,
    },
    DefaultMode,
    Broker {
        broker_id: String,
    },
    Hook {
        handler_id: String,
    },
    Timeout {
        default: Decision,
    },
    ParentForwarded {
        parent_session_id: SessionId,
        original_decided_by: Box<DecidedBy>,
    },
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolCapability {
    SubagentRunner,
    TodoStore,
    RunCanceller,
    ClarifyChannel,
    UserMessenger,
    BlobReader,
    BlobWriter,
    OffloadedBlobAuthorizer,
    HookEmitter,
    SkillRegistry,
    ContextPatchSink,
    EmbeddedToolDispatcher,
    CodeRuntime,
    ProviderCredentialResolver,
    NetworkBroker,
    Custom(String),
}

impl fmt::Display for ToolCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SubagentRunner => f.write_str("subagent_runner"),
            Self::TodoStore => f.write_str("todo_store"),
            Self::RunCanceller => f.write_str("run_canceller"),
            Self::ClarifyChannel => f.write_str("clarify_channel"),
            Self::UserMessenger => f.write_str("user_messenger"),
            Self::BlobReader => f.write_str("blob_reader"),
            Self::BlobWriter => f.write_str("blob_writer"),
            Self::OffloadedBlobAuthorizer => f.write_str("offloaded_blob_authorizer"),
            Self::HookEmitter => f.write_str("hook_emitter"),
            Self::SkillRegistry => f.write_str("skill_registry"),
            Self::ContextPatchSink => f.write_str("context_patch_sink"),
            Self::EmbeddedToolDispatcher => f.write_str("embedded_tool_dispatcher"),
            Self::CodeRuntime => f.write_str("code_runtime"),
            Self::ProviderCredentialResolver => f.write_str("provider_credential_resolver"),
            Self::NetworkBroker => f.write_str("network_broker"),
            Self::Custom(value) => write!(f, "custom:{value}"),
        }
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ToolExecutionChannel {
    DirectAuthorizedRust,
    ProcessSandbox,
    HttpBroker,
    ExternalCapability { capability: ToolCapability },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolOrigin {
    Builtin,
    Plugin {
        plugin_id: PluginId,
        trust: TrustLevel,
    },
    Mcp(McpOrigin),
    Skill(SkillOrigin),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct McpOrigin {
    pub server_id: McpServerId,
    pub upstream_name: String,
    pub server_meta: BTreeMap<String, Value>,
    pub server_source: McpServerSource,
    pub server_trust: TrustLevel,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpServerSource {
    Workspace,
    User,
    Project,
    Policy,
    Plugin(PluginId),
    Dynamic { registered_by: String },
    Managed { registry_url: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpServerScope {
    Global,
    Session(SessionId),
    Agent(AgentId),
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct SkillOrigin {
    pub skill_id: SkillId,
    pub skill_name: String,
    pub source_kind: SkillSourceKind,
    pub trust: TrustLevel,
}

#[derive(
    Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct SkillId(pub String);

#[derive(
    Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct PluginId(pub String);

#[derive(
    Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct McpServerId(pub String);

pub type ToolName = String;
pub type SemverString = String;
pub type ToolLoadingBackendName = String;

#[derive(
    Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct ModelProvider(pub String);

#[derive(
    Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct UlidString(pub String);

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkillSourceKind {
    Bundled,
    Workspace,
    User,
    Plugin(PluginId),
    Mcp(McpServerId),
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShadowReason {
    BuiltinWins,
    HigherTrust,
    Duplicate,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRestriction {
    All,
    Allowlist(BTreeSet<ModelProvider>),
    Denylist(BTreeSet<ModelProvider>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResultBudget {
    pub metric: BudgetMetric,
    pub limit: u64,
    pub on_overflow: OverflowAction,
    pub preview_head_chars: u32,
    pub preview_tail_chars: u32,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BudgetMetric {
    Chars,
    Bytes,
    Lines,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OverflowAction {
    Truncate,
    Offload,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OverflowMetadata {
    pub blob_ref: crate::BlobRef,
    pub head_chars: u32,
    pub tail_chars: u32,
    pub original_size: u64,
    pub original_metric: BudgetMetric,
    pub effective_limit: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolProperties {
    pub is_concurrency_safe: bool,
    pub is_read_only: bool,
    pub is_destructive: bool,
    pub long_running: Option<LongRunningPolicy>,
    pub defer_policy: DeferPolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LongRunningPolicy {
    pub stall_threshold: Duration,
    pub hard_timeout: Duration,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DenyReason {
    UserDenied,
    RuleDenied,
    DefaultModeDenied,
    HookBlocked { handler_id: String },
    SubagentBlocked,
    PolicyDenied,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolErrorPayload {
    pub code: String,
    pub message: String,
    pub retriable: bool,
}

#[non_exhaustive]
#[derive(
    Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    UserPreference,
    Feedback,
    ProjectFact,
    Reference,
    AgentSelfNote,
    Custom(String),
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryVisibility {
    Private { session_id: SessionId },
    User { user_id: String },
    Team { team_id: TeamId },
    Tenant,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryWriteAction {
    AppendSection { section: String },
    ReplaceSection { section: String },
    DeleteSection { section: String },
    Upsert,
    Forget,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    UserInput,
    AgentDerived,
    SubagentDerived { child_session: SessionId },
    ToolOutput,
    McpToolOutput,
    PluginOutput,
    WebRetrieval,
    WorkspaceFile,
    ExternalRetrieval,
    Imported,
    Consolidated { from: Vec<MemoryId> },
}

#[non_exhaustive]
#[derive(
    Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ThreatCategory {
    PromptInjection,
    Exfiltration,
    Backdoor,
    Credential,
    Malicious,
    SpecialToken,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThreatAction {
    Warn,
    Redact,
    Block,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThreatDirection {
    OnWrite,
    OnRecall,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRecallDegradedReason {
    Timeout,
    ProviderError(String),
    RecordTooLarge,
    VisibilityViolation,
    ScannerBlocked,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecallSkipReason {
    NoExternalProvider,
    PolicyDecidedSkip,
    DeadlineZero,
    Cancelled,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TakesEffect {
    CurrentSession,
    NextSession,
    AfterReloadWith { session_id: SessionId },
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OverflowStrategy {
    SectionTruncated {
        kept_sections: u32,
        dropped_sections: u32,
    },
    HeadOnly {
        kept_chars: u32,
    },
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SandboxMode {
    None,
    OsLevel(LocalIsolationTag),
    Container,
    Remote,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LocalIsolationTag {
    None,
    Bubblewrap,
    Seatbelt,
    JobObject,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SandboxScope {
    WorkspaceOnly,
    WorkspacePlus(Vec<PathBuf>),
    Unrestricted,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NetworkAccess {
    None,
    LoopbackOnly,
    AllowList(Vec<HostRule>),
    Unrestricted,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct HostRule {
    pub pattern: String,
    pub ports: Option<Vec<u16>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResourceLimits {
    pub max_memory_bytes: Option<u64>,
    pub max_cpu_cores: Option<f32>,
    pub max_pids: Option<u32>,
    pub max_wall_clock_ms: Option<u64>,
    pub max_open_files: Option<u32>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAccess {
    None,
    ReadOnly,
    ReadWrite {
        allowed_writable_subpaths: Vec<PathBuf>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxPolicy {
    pub mode: SandboxMode,
    pub scope: SandboxScope,
    pub network: NetworkAccess,
    pub resource_limits: ResourceLimits,
    pub denied_host_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ExecFingerprint(pub [u8; 32]);

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ActionPlanHash([u8; 32]);

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct SandboxPolicyHash([u8; 32]);

impl Default for ActionPlanHash {
    fn default() -> Self {
        Self([0; 32])
    }
}

impl Default for SandboxPolicyHash {
    fn default() -> Self {
        Self([0; 32])
    }
}

fn decode_hex_digest(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 {
        return Err("digest must be 64 lowercase hex characters".to_owned());
    }

    let mut bytes = [0_u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(chunk[0])?;
        let low = hex_nibble(chunk[1])?;
        bytes[index] = (high << 4) | low;
    }
    Ok(bytes)
}

fn hex_nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err("digest must use lowercase hex".to_owned()),
    }
}

fn encode_hex_digest(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

macro_rules! impl_hash_digest {
    ($ty:ident) => {
        impl $ty {
            pub fn from_bytes(bytes: [u8; 32]) -> Self {
                Self(bytes)
            }

            pub fn from_hex(value: &str) -> Result<Self, String> {
                decode_hex_digest(value).map(Self)
            }

            #[must_use]
            pub fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }

            #[must_use]
            pub fn to_hex(&self) -> String {
                encode_hex_digest(&self.0)
            }
        }

        impl fmt::Display for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.to_hex())
            }
        }

        impl FromStr for $ty {
            type Err = String;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::from_hex(value)
            }
        }

        impl Serialize for $ty {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.to_hex())
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::from_hex(&value).map_err(D::Error::custom)
            }
        }

        impl JsonSchema for $ty {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($ty).into()
            }

            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!({
                    "type": "string",
                    "pattern": "^[0-9a-f]{64}$"
                })
            }
        }
    };
}

impl_hash_digest!(ActionPlanHash);
impl_hash_digest!(SandboxPolicyHash);

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KillScope {
    Process,
    ProcessGroup,
    SessionLeader,
}

#[non_exhaustive]
#[derive(
    Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SessionSnapshotKind {
    FilesystemImage,
    ShellState,
    ContainerImage,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShellKind {
    System,
    Bash(PathBuf),
    Zsh(PathBuf),
    PowerShell,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleSource {
    User,
    Workspace,
    Project,
    Local,
    Flag,
    Policy,
    CliArg,
    Command,
    Session,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, strum::EnumDiscriminants)]
#[strum_discriminants(
    name(PermissionSubjectDiscriminant),
    derive(Hash, Serialize, Deserialize, JsonSchema)
)]
#[serde(rename_all = "snake_case")]
pub enum PermissionSubject {
    ToolInvocation {
        tool: String,
        input: Value,
    },
    CommandExec {
        command: String,
        argv: Vec<String>,
        cwd: Option<PathBuf>,
        fingerprint: Option<ExecFingerprint>,
    },
    FileWrite {
        path: PathBuf,
        bytes_preview: Vec<u8>,
    },
    FileDelete {
        path: PathBuf,
    },
    NetworkAccess {
        host: String,
        port: Option<u16>,
    },
    DangerousCommand {
        command: String,
        pattern_id: String,
        severity: Severity,
    },
    McpToolCall {
        server: String,
        tool: String,
        input: Value,
    },
    Custom {
        kind: String,
        payload: Value,
    },
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InteractivityLevel {
    FullyInteractive,
    NoInteractive,
    DeferredInteractive,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FallbackPolicy {
    AskUser,
    DenyAll,
    AllowReadOnly,
    ClosestMatchingRule,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TimeoutPolicy {
    pub deadline_ms: u64,
    pub default_on_timeout: Decision,
    pub heartbeat_interval_ms: Option<u64>,
}

pub const TOOL_NAME_PATTERN: &str = r"^[a-zA-Z0-9_-]{1,64}$";

#[derive(Debug, thiserror::Error)]
pub enum ToolNameError {
    #[error("tool name `{0}` violates `{TOOL_NAME_PATTERN}`")]
    Invalid(String),
    #[error("mcp namespace separator `__` is reserved; got `{0}`")]
    ReservedSeparator(String),
}

pub fn validate_tool_name(name: &str) -> Result<(), ToolNameError> {
    let valid_chars = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if name.is_empty() || name.len() > 64 || !valid_chars {
        return Err(ToolNameError::Invalid(name.to_owned()));
    }
    if name.contains("__") {
        return Err(ToolNameError::ReservedSeparator(name.to_owned()));
    }
    Ok(())
}

pub fn canonical_mcp_tool_name(server: &str, tool: &str) -> Result<String, ToolNameError> {
    validate_tool_name(server)?;
    validate_tool_name(tool)?;
    Ok(format!("mcp__{server}__{tool}"))
}

pub fn parse_canonical_mcp_tool_name(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix("mcp__")?;
    rest.split_once("__")
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SteeringMessage {
    pub id: SteeringId,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub kind: SteeringKind,
    pub priority: SteeringPriority,
    pub body: SteeringBody,
    pub queued_at: DateTime<Utc>,
    pub correlation_id: Option<CorrelationId>,
    pub source: SteeringSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SteeringRequest {
    pub kind: SteeringKind,
    pub body: SteeringBody,
    pub priority: Option<SteeringPriority>,
    pub correlation_id: Option<CorrelationId>,
    pub source: SteeringSource,
}

#[non_exhaustive]
#[derive(
    Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SteeringKind {
    Append,
    Replace,
    NudgeOnly,
}

#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SteeringBody {
    Text(String),
    Structured {
        instruction: String,
        addenda: BTreeMap<String, Value>,
    },
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SteeringPriority {
    Normal,
    High,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SteeringSource {
    User,
    Plugin { plugin_id: PluginId },
    AutoMonitor { rule_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SteeringPolicy {
    pub capacity: usize,
    pub ttl_ms: u64,
    pub overflow: SteeringOverflow,
    pub dedup_window_ms: u64,
}

impl Default for SteeringPolicy {
    fn default() -> Self {
        Self {
            capacity: 8,
            ttl_ms: 60_000,
            overflow: SteeringOverflow::DropOldest,
            dedup_window_ms: 1_500,
        }
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SteeringOverflow {
    DropOldest,
    DropNewest,
    BackPressure,
}

// ── Memory Platform Contracts ──

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEvidenceOrigin {
    UserMessage {
        session_id: SessionId,
        run_id: RunId,
        message_id: MessageId,
    },
    AssistantMessage {
        session_id: SessionId,
        run_id: RunId,
        message_id: MessageId,
    },
    SubagentOutput {
        parent_session_id: SessionId,
        child_session_id: SessionId,
        run_id: RunId,
        agent_id: Option<AgentId>,
    },
    BuiltinToolOutput {
        tool_name: String,
        tool_use_id: ToolUseId,
    },
    McpToolOutput {
        server_id: String,
        tool_name: String,
        tool_use_id: ToolUseId,
    },
    PluginOutput {
        plugin_id: String,
        tool_name: Option<String>,
        tool_use_id: Option<ToolUseId>,
    },
    WebRetrieval {
        url_hash: ContentHash,
        fetch_tool_use_id: Option<ToolUseId>,
    },
    WorkspaceFile {
        workspace_id: WorkspaceId,
        path_hash: ContentHash,
        snapshot_id: Option<SnapshotId>,
    },
    Imported {
        importer: String,
        import_id: String,
    },
    Consolidated {
        from: Vec<MemoryId>,
    },
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCandidateState {
    Proposed,
    Approved,
    Rejected,
    Promoted,
    Merged,
    Expired,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryThreadMode {
    Off,
    ReadOnly,
    ReadWrite,
    CandidateOnly,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryDropReason {
    Expired,
    Deleted,
    VisibilityDenied,
    PolicyDenied,
    ThreatBlocked,
    BudgetExceeded,
    Duplicate,
    ProviderTimeout,
    ProviderError,
    ScoreBelowThreshold,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPolicyDecision {
    Allow,
    Deny { reason: MemoryPolicyDenyReason },
    CandidateOnly { reason: MemoryPolicyDenyReason },
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPolicyDenyReason {
    GlobalUseDisabled,
    ThreadUseDisabled,
    GlobalGenerationDisabled,
    ThreadGenerationDisabled,
    ExternalContextGenerationDisabled,
    MissingPolicy,
    VisibilityEscalationDenied,
    ProviderNotWritable,
    TenantMismatch,
    TombstoneMatched,
    PermissionRequired,
    ThreatBlocked,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryProviderTrust {
    BuiltIn,
    Workspace,
    Team,
    Plugin,
    External,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryProviderKind {
    Local,
    Team,
    Subagent,
    Plugin,
    External,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryProviderDurability {
    Durable,
    Ephemeral,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryVisibilityClass {
    Private,
    User,
    Team,
    Tenant,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryActor {
    User {
        user_label: Option<String>,
    },
    Model,
    System,
    Subagent {
        child_session_id: SessionId,
        agent_id: Option<AgentId>,
    },
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryProviderSelectionPolicy {
    PolicySelected,
    RequireProvider { provider_id: String },
    DenyModelSelectedProvider,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MemoryToolAction {
    Search(MemorySearchRequest),
    Read(MemoryReadRequest),
    Create(MemoryToolCreateArgs),
    Update(MemoryToolUpdateArgs),
    Delete(MemoryDeleteRequest),
    List(MemoryListRequest),
    Propose(MemoryToolProposeArgs),
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryToolState {
    Completed,
    CandidateCreated,
    PermissionRequired { action_plan_id: ActionPlanId },
    Denied { reason: MemoryPolicyDenyReason },
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTakesEffect {
    CurrentTurn,
    NextTurn,
    NextSession,
    Never,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BudgetKind {
    SoftBudget,
    HardBudget,
    PerTurnTokens,
    PerSessionTokens,
    PerToolMaxChars { tool_name: String },
    Tokens,
    ToolCalls,
    WallClock,
    Cost,
}

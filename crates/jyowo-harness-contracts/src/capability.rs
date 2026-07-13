//! Tool capability marker traits.
//!

use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet, HashMap},
    path::PathBuf,
    sync::Arc,
};

use futures::future::BoxFuture;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use serde_json::Value;

use bytes::Bytes;
use futures::stream::BoxStream;

use crate::{
    AgentId, BlobMeta, BlobRef, BlobStore, CapabilityRouteKind, CorrelationId,
    DiagnosticsRawOutput, DiagnosticsRunRequest, Event, HookEventKind, InteractivityLevel,
    MemoryId, NetworkAccess, OverflowMetadata, PermissionMode, RunId, SessionId, SkillId,
    SkillSourceKind, SubagentId, TeamId, TenantId, ToolCapability, ToolError, ToolProfile,
    ToolSearchMode, ToolUseId, TranscriptRef, TurnInput, UsageSnapshot,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AgentCapabilityKind {
    Subagents,
    AgentTeams,
    BackgroundAgents,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentCapabilityUnavailableReason {
    DaemonUnavailable {
        capability: AgentCapabilityKind,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilitiesPayload {
    pub subagents_enabled: bool,
    pub agent_teams_enabled: bool,
    pub background_agents_enabled: bool,
    pub subagents_available: bool,
    pub agent_teams_available: bool,
    pub background_agents_available: bool,
    pub unavailable_reasons: Vec<AgentCapabilityUnavailableReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    pub id: String,
    pub scope: AgentProfileScope,
    pub role: String,
    pub description: String,
    pub model_config_override: Option<AgentProfileModelOverride>,
    pub tool_allowlist: Option<Vec<String>>,
    pub tool_blocklist: Vec<String>,
    pub sandbox_inheritance: AgentProfileSandboxInheritance,
    pub memory_scope: AgentProfileMemoryScope,
    pub context_mode: AgentProfileContextMode,
    pub max_turns: u32,
    pub max_depth: u8,
    pub default_workspace_isolation: AgentWorkspaceIsolationMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileModelOverride {
    pub provider_config_id: Option<String>,
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentProfileScope {
    Builtin,
    User,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentProfileSandboxInheritance {
    InheritParent,
    NarrowOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentProfileMemoryScope {
    None,
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentProfileContextMode {
    Minimal,
    Focused,
    FullWorkspace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentToolPolicy {
    pub subagents: AgentUsePolicy,
    pub agent_team: AgentUsePolicy,
    pub background_agents: AgentUsePolicy,
    pub team_config: Option<AgentTeamRunConfig>,
    pub workspace_isolation: AgentWorkspaceIsolationMode,
    pub max_depth: u8,
    pub max_concurrent_subagents: u32,
    pub max_team_members: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentTeamRunConfig {
    pub topology: AgentTeamTopology,
    pub lead_profile_id: String,
    pub member_profile_ids: Vec<String>,
    pub max_turns_per_goal: u32,
    pub shared_memory_policy: AgentTeamSharedMemoryPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentTeamTopology {
    CoordinatorWorker,
    PeerToPeer,
    RoleRouted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentTeamSharedMemoryPolicy {
    None,
    SummariesOnly,
    RedactedMailbox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentUsePolicy {
    Off,
    Allowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspaceIsolationMode {
    ReadOnly,
    PatchOnly,
    GitWorktree,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentOrchestrationValidationError {
    InvalidProfileId { id: String },
    EmptyTeamMemberList,
    UnexpectedTeamConfig,
    InvalidConcurrency { field: &'static str, value: u32 },
}

impl std::fmt::Display for AgentOrchestrationValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidProfileId { id } => write!(f, "invalid agent profile id: {id}"),
            Self::EmptyTeamMemberList => write!(f, "agent team member list must not be empty"),
            Self::UnexpectedTeamConfig => {
                write!(f, "agent team off but teamConfig is present")
            }
            Self::InvalidConcurrency { field, value } => {
                write!(f, "invalid {field}: {value}")
            }
        }
    }
}

impl std::error::Error for AgentOrchestrationValidationError {}

pub fn validate_agent_profile_id(id: &str) -> Result<(), AgentOrchestrationValidationError> {
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
    {
        return Err(AgentOrchestrationValidationError::InvalidProfileId { id: id.to_owned() });
    }
    Ok(())
}

pub fn validate_agent_profile(
    profile: &AgentProfile,
) -> Result<(), AgentOrchestrationValidationError> {
    validate_agent_profile_id(&profile.id)?;
    if profile.role.trim().is_empty() {
        return Err(AgentOrchestrationValidationError::InvalidProfileId {
            id: profile.id.clone(),
        });
    }
    Ok(())
}

pub fn validate_agent_tool_policy(
    options: &AgentToolPolicy,
) -> Result<(), AgentOrchestrationValidationError> {
    match (options.agent_team, options.team_config.as_ref()) {
        (AgentUsePolicy::Off, Some(_)) => {
            return Err(AgentOrchestrationValidationError::UnexpectedTeamConfig);
        }
        _ => {}
    }

    if let Some(team_config) = options.team_config.as_ref() {
        validate_agent_profile_id(&team_config.lead_profile_id)?;
        if team_config.member_profile_ids.is_empty() {
            return Err(AgentOrchestrationValidationError::EmptyTeamMemberList);
        }
        for member_id in &team_config.member_profile_ids {
            validate_agent_profile_id(member_id)?;
        }
    }

    if options.max_concurrent_subagents == 0 {
        return Err(AgentOrchestrationValidationError::InvalidConcurrency {
            field: "maxConcurrentSubagents",
            value: options.max_concurrent_subagents,
        });
    }

    if options.max_team_members == 0 {
        return Err(AgentOrchestrationValidationError::InvalidConcurrency {
            field: "maxTeamMembers",
            value: options.max_team_members,
        });
    }

    Ok(())
}

pub trait SubagentRunnerCap: Send + Sync + 'static {
    fn spawn(
        &self,
        spec: Value,
        parent: SubagentParentContext,
    ) -> BoxFuture<'static, Result<SubagentSpawnHandle, ToolError>>;
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct SubagentParentContext {
    pub tenant_id: TenantId,
    pub parent_session_id: SessionId,
    pub parent_run_id: RunId,
    pub depth: u8,
    pub sibling_count: u32,
    pub trigger_tool_use_id: Option<ToolUseId>,
    pub correlation_id: CorrelationId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubagentSpawnHandle {
    pub subagent_id: SubagentId,
    pub input: TurnInput,
    pub announcement: SubagentCapAnnouncement,
}

impl SubagentSpawnHandle {
    pub fn wait(self) -> BoxFuture<'static, Result<SubagentCapAnnouncement, ToolError>> {
        Box::pin(async move { Ok(self.announcement) })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubagentCapAnnouncement {
    pub subagent_id: SubagentId,
    pub status: crate::SubagentStatus,
    pub summary: String,
    pub result: Option<Value>,
    pub usage: UsageSnapshot,
    pub transcript_ref: Option<TranscriptRef>,
}

pub const BACKGROUND_AGENT_STARTER_CAPABILITY: &str = "jyowo.background_agent.starter";

pub trait BackgroundAgentStarterCap: Send + Sync + 'static {
    fn start_background_agent(
        &self,
        request: BackgroundAgentToolStartRequest,
    ) -> BoxFuture<'static, Result<BackgroundAgentToolStartResponse, ToolError>>;
}

pub const AGENT_TEAM_STARTER_CAPABILITY: &str = "jyowo.agent_team.starter";

pub trait AgentTeamStarterCap: Send + Sync + 'static {
    fn start_agent_team(
        &self,
        request: AgentTeamToolStartRequest,
    ) -> BoxFuture<'static, Result<AgentTeamToolStartResponse, ToolError>>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentTeamToolStartRequest {
    pub tenant_id: TenantId,
    pub conversation_id: SessionId,
    pub parent_run_id: RunId,
    pub tool_use_id: ToolUseId,
    pub goal: String,
    pub topology: AgentTeamTopology,
    pub max_turns_per_goal: u32,
    pub agent_tool_policy: AgentToolPolicy,
    pub session: AgentTeamToolSessionSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentTeamToolSessionSnapshot {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub tool_search: ToolSearchMode,
    pub tool_profile: ToolProfile,
    pub permission_mode: PermissionMode,
    pub interactivity: InteractivityLevel,
    pub team_id: Option<TeamId>,
    pub max_iterations: u32,
    pub context_compression_trigger_ratio: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentTeamToolStartResponse {
    pub team_id: TeamId,
    pub conversation_id: SessionId,
    pub parent_run_id: RunId,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundAgentToolStartRequest {
    pub tenant_id: TenantId,
    pub conversation_id: SessionId,
    pub parent_run_id: RunId,
    pub tool_use_id: ToolUseId,
    pub goal: String,
    pub title: String,
    pub model_config_id: Option<String>,
    pub permission_mode: PermissionMode,
    pub agent_tool_policy: AgentToolPolicy,
    pub session: BackgroundAgentToolSessionSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundAgentToolSessionSnapshot {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub tool_search: ToolSearchMode,
    pub tool_profile: ToolProfile,
    pub permission_mode: PermissionMode,
    pub interactivity: InteractivityLevel,
    pub team_id: Option<TeamId>,
    pub max_iterations: u32,
    pub context_compression_trigger_ratio: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundAgentToolStartResponse {
    pub background_agent_id: String,
    pub conversation_id: SessionId,
    pub parent_run_id: RunId,
    pub title: String,
    pub status: String,
}

pub trait TodoStoreCap: Send + Sync + 'static {
    fn replace_todos(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
        items: Vec<TodoItem>,
    ) -> BoxFuture<'_, Result<(), ToolError>>;
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct TodoItem {
    pub content: String,
    pub status: String,
}

pub trait RunCancellerCap: Send + Sync + 'static {
    fn request_stop(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
        reason: String,
    ) -> BoxFuture<'_, Result<(), ToolError>>;
}

pub trait DiagnosticsRunnerCap: Send + Sync + 'static {
    fn run_diagnostics(
        &self,
        request: DiagnosticsRunRequest,
    ) -> BoxFuture<'_, Result<DiagnosticsRawOutput, ToolError>>;
}

pub trait ClarifyChannelCap: Send + Sync + 'static {
    fn ask(&self, prompt: ClarifyPrompt) -> BoxFuture<'static, Result<ClarifyAnswer, ToolError>>;
}

pub trait UserMessengerCap: Send + Sync + 'static {
    fn send(
        &self,
        message: OutboundUserMessage,
    ) -> BoxFuture<'static, Result<UserMessageDelivery, ToolError>>;
}

pub trait ProviderCredentialResolverCap: Send + Sync + 'static {
    fn resolve_provider_credential(
        &self,
        context: ProviderCredentialResolveContext,
    ) -> BoxFuture<'_, Result<ProviderCredential, ToolError>>;
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ProviderCredentialResolveContext {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub provider_id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "modelConfigId"
    )]
    pub model_config_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "operationId"
    )]
    pub operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "routeKind")]
    pub route_kind: Option<CapabilityRouteKind>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ProviderCredential {
    pub provider_id: String,
    pub config_id: String,
    pub api_key: String,
    pub base_url: Option<String>,
}

pub trait BlobReaderCap: Send + Sync + 'static {
    fn read_blob(
        &self,
        tenant_id: TenantId,
        blob: BlobRef,
    ) -> BoxFuture<'_, Result<BoxStream<'static, Bytes>, ToolError>>;
}

pub trait BlobWriterCap: Send + Sync + 'static {
    fn write_blob(
        &self,
        tenant_id: TenantId,
        bytes: Bytes,
        meta: BlobMeta,
    ) -> BoxFuture<'_, Result<BlobRef, ToolError>>;
}

pub trait OffloadedBlobAuthorizerCap: Send + Sync + 'static {
    fn authorize_offloaded_blob(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
        blob: BlobRef,
    ) -> BoxFuture<'_, Result<(), ToolError>>;
}

impl<T> BlobReaderCap for T
where
    T: BlobStore + ?Sized,
{
    fn read_blob(
        &self,
        tenant_id: TenantId,
        blob: BlobRef,
    ) -> BoxFuture<'_, Result<BoxStream<'static, Bytes>, ToolError>> {
        Box::pin(async move {
            self.get(tenant_id, &blob)
                .await
                .map_err(|error| ToolError::Message(error.to_string()))
        })
    }
}

impl<T> BlobWriterCap for T
where
    T: BlobStore + ?Sized,
{
    fn write_blob(
        &self,
        tenant_id: TenantId,
        bytes: Bytes,
        meta: BlobMeta,
    ) -> BoxFuture<'_, Result<BlobRef, ToolError>> {
        Box::pin(async move {
            self.put(tenant_id, bytes, meta)
                .await
                .map_err(|error| ToolError::Message(error.to_string()))
        })
    }
}

#[derive(Clone)]
pub struct BlobReaderCapAdapter {
    inner: Arc<dyn BlobStore>,
}

impl BlobReaderCapAdapter {
    #[must_use]
    pub fn new(inner: Arc<dyn BlobStore>) -> Self {
        Self { inner }
    }
}

impl BlobReaderCap for BlobReaderCapAdapter {
    fn read_blob(
        &self,
        tenant_id: TenantId,
        blob: BlobRef,
    ) -> BoxFuture<'_, Result<BoxStream<'static, Bytes>, ToolError>> {
        Box::pin(async move {
            self.inner
                .get(tenant_id, &blob)
                .await
                .map_err(|error| ToolError::Message(error.to_string()))
        })
    }
}

#[derive(Clone)]
pub struct BlobWriterCapAdapter {
    inner: Arc<dyn BlobStore>,
}

impl BlobWriterCapAdapter {
    #[must_use]
    pub fn new(inner: Arc<dyn BlobStore>) -> Self {
        Self { inner }
    }
}

impl BlobWriterCap for BlobWriterCapAdapter {
    fn write_blob(
        &self,
        tenant_id: TenantId,
        bytes: Bytes,
        meta: BlobMeta,
    ) -> BoxFuture<'_, Result<BlobRef, ToolError>> {
        Box::pin(async move {
            self.inner
                .put(tenant_id, bytes, meta)
                .await
                .map_err(|error| ToolError::Message(error.to_string()))
        })
    }
}
pub trait HookEmitterCap: Send + Sync + 'static {}
pub trait SkillRegistryCap: Send + Sync + 'static {
    fn list_summaries(&self, agent: &AgentId, filter: SkillFilter) -> Vec<SkillSummary>;

    fn view(&self, agent: &AgentId, name: &str, full: bool) -> Option<SkillView>;

    fn render(
        &self,
        agent: &AgentId,
        name: String,
        params: Value,
    ) -> BoxFuture<'static, Result<RenderedSkill, ToolError>>;

    fn prepare_script(
        &self,
        _agent: &AgentId,
        _name: String,
        _script_id: String,
        _arguments: Value,
    ) -> BoxFuture<'static, Result<SkillScriptRunPreparation, ToolError>> {
        Box::pin(async {
            Err(ToolError::Validation(
                "skill script execution is unavailable".to_owned(),
            ))
        })
    }

    fn prepare_script_authorized(
        &self,
        _agent: &AgentId,
        _name: String,
        _script_id: String,
        _arguments: Value,
    ) -> BoxFuture<'static, Result<SkillScriptRunPreparation, ToolError>> {
        Box::pin(async {
            Err(ToolError::PermissionDenied(
                "authorized skill script preparation is unavailable".to_owned(),
            ))
        })
    }
}

#[derive(Clone, PartialEq)]
pub struct SkillScriptRunPreparation {
    pub skill_id: SkillId,
    pub skill_name: String,
    pub script_id: String,
    pub package_hash: String,
    pub arguments: Value,
    pub declaration: SkillScriptRunDeclaration,
    pub files: Vec<SkillScriptRunFile>,
    pub env: BTreeMap<String, String>,
}

impl std::fmt::Debug for SkillScriptRunPreparation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SkillScriptRunPreparation")
            .field("skill_id", &self.skill_id)
            .field("skill_name", &self.skill_name)
            .field("script_id", &self.script_id)
            .field("package_hash", &self.package_hash)
            .field("arguments", &self.arguments)
            .field("declaration", &self.declaration)
            .field("files", &self.files)
            .field("env_keys", &self.env.keys().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillScriptRunDeclaration {
    pub path: PathBuf,
    pub timeout_seconds: u64,
    pub max_stdout_bytes: u64,
    pub max_stderr_bytes: u64,
    pub max_output_bytes: u64,
    pub max_artifact_count: u64,
    pub max_artifact_bytes: u64,
    pub network_access: NetworkAccess,
    pub env_config_keys: BTreeMap<String, String>,
    pub secret_env_keys: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillScriptRunFile {
    pub path: String,
    pub content: String,
}
pub trait ContextPatchSinkCap: Send + Sync + 'static {
    fn push_patch(&self, request: ContextPatchRequest)
        -> BoxFuture<'static, Result<(), ToolError>>;
}
pub trait EmbeddedToolDispatcherCap: Send + Sync + 'static {
    fn dispatch_embedded(
        &self,
        request: EmbeddedToolDispatchRequest,
    ) -> BoxFuture<'static, Result<EmbeddedToolDispatchResponse, ToolError>>;
}

pub trait CodeRuntimeCap: Send + Sync + 'static {
    fn run_code(
        &self,
        request: CodeRunRequest,
        dispatcher: Arc<dyn EmbeddedToolDispatcherCap>,
    ) -> BoxFuture<'static, Result<CodeRunResult, CodeRunError>>;
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CodeLanguage {
    MiniLua,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CodeRunRequest {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: ToolUseId,
    pub language: CodeLanguage,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CodeRunResult {
    pub value: Value,
    pub stats: CodeRunStats,
    pub embedded_steps: Vec<EmbeddedToolDispatchResponse>,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodeRunError {
    pub error: ToolError,
    pub events: Vec<Event>,
}

impl From<ToolError> for CodeRunError {
    fn from(error: ToolError) -> Self {
        Self {
            error,
            events: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct CodeRunStats {
    pub instructions: u64,
    pub embedded_call_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EmbeddedToolDispatchRequest {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub parent_tool_use_id: ToolUseId,
    pub tool_name: String,
    pub input: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EmbeddedToolDispatchResponse {
    pub tool_use_id: ToolUseId,
    pub tool_name: String,
    pub output: Value,
    pub duration_ms: u64,
    pub overflow: Option<OverflowMetadata>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContextPatchRequest {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub source: ContextPatchSource,
    pub body: String,
    pub lifecycle: ContextPatchLifecycle,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContextPatchSource {
    MemoryRecall {
        provider_id: String,
        turn: u32,
    },
    MemoryReference {
        provider_id: String,
        memory_ids: Vec<MemoryId>,
    },
    KnowledgeRetrieval {
        provider_id: String,
        knowledge_base_ids: Vec<String>,
        reference_chunk_count: u32,
    },
    SkillInjection {
        skill_id: SkillId,
        skill_name: String,
        injection_id: SkillInjectionId,
        tool_use_id: ToolUseId,
        consumed_config_keys: Vec<String>,
    },
    HookAddContext {
        handler_id: String,
        hook_event_kind: HookEventKind,
    },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContextPatchLifecycle {
    Transient,
    Persistent { ttl_turns: Option<u32> },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkillStatus {
    Ready,
    PrerequisiteMissing {
        #[serde(default)]
        env_vars: Vec<String>,
        #[serde(default)]
        config_keys: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub source: SkillSourceKind,
    pub status: SkillStatus,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillFilter {
    pub tag: Option<String>,
    pub category: Option<String>,
    pub include_prerequisite_missing: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillView {
    pub summary: SkillSummary,
    pub parameters: Vec<SkillParameterInfo>,
    pub config_keys: Vec<String>,
    pub body_preview: String,
    pub body_full: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillParameterInfo {
    pub name: String,
    pub param_type: String,
    pub required: bool,
    pub default: Option<Value>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct SkillInjectionId(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillInvocationReceipt {
    pub skill_name: String,
    pub injection_id: SkillInjectionId,
    pub bytes_injected: u64,
    pub consumed_config_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RenderedSkill {
    pub skill_id: SkillId,
    pub skill_name: String,
    pub content: String,
    pub shell_invocations: Vec<SkillShellInvocation>,
    pub consumed_config_keys: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkillShellInvocation {
    pub command: String,
    pub stdout_truncated: bool,
    pub exit_code: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ClarifyPrompt {
    pub prompt: String,
    pub choices: Vec<ClarifyChoice>,
    pub multiple: bool,
    pub timeout_seconds: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ClarifyChoice {
    pub id: String,
    pub label: String,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ClarifyAnswer {
    pub answer: String,
    pub chosen_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OutboundUserMessage {
    pub channel: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct UserMessageDelivery {
    pub message_id: String,
    pub delivered: bool,
}

#[derive(Clone, Default)]
pub struct CapabilityRegistry {
    inner: HashMap<ToolCapability, Arc<dyn Any + Send + Sync>>,
}

impl CapabilityRegistry {
    pub fn install<T>(&mut self, capability: ToolCapability, implementation: Arc<T>)
    where
        T: ?Sized + Send + Sync + 'static,
    {
        self.inner.insert(capability, Arc::new(implementation));
    }

    #[must_use]
    pub fn contains(&self, capability: &ToolCapability) -> bool {
        self.inner.contains_key(capability)
    }

    pub fn overlay_from(&mut self, other: &Self) {
        self.inner.extend(
            other.inner.iter().map(|(capability, implementation)| {
                (capability.clone(), Arc::clone(implementation))
            }),
        );
    }

    pub fn get<T>(&self, capability: &ToolCapability) -> Option<Arc<T>>
    where
        T: ?Sized + Send + Sync + 'static,
    {
        let erased = Arc::clone(self.inner.get(capability)?);
        erased
            .downcast::<Arc<T>>()
            .ok()
            .map(|typed| Arc::clone(typed.as_ref()))
    }
}

#[cfg(feature = "testing")]
#[derive(Clone, Default)]
pub struct TestCapabilityRegistry {
    inner: CapabilityRegistry,
}

#[cfg(feature = "testing")]
impl TestCapabilityRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_capability<T>(mut self, capability: ToolCapability, implementation: Arc<T>) -> Self
    where
        T: ?Sized + Send + Sync + 'static,
    {
        self.inner.install(capability, implementation);
        self
    }

    pub fn install<T>(&mut self, capability: ToolCapability, implementation: Arc<T>)
    where
        T: ?Sized + Send + Sync + 'static,
    {
        self.inner.install(capability, implementation);
    }

    #[must_use]
    pub fn into_registry(self) -> CapabilityRegistry {
        self.inner
    }
}

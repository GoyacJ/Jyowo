//! Tool capability marker traits.
//!
//! SPEC: docs/architecture/harness/crates/harness-contracts.md §3.4

use std::{any::Any, collections::HashMap, sync::Arc};

use futures::future::BoxFuture;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use serde_json::Value;

use bytes::Bytes;
use futures::stream::BoxStream;

use crate::{
    AgentId, BlobMeta, BlobRef, BlobStore, CapabilityRouteKind, CorrelationId,
    DiagnosticsRawOutput, DiagnosticsRunRequest, Event, HookEventKind, OverflowMetadata, RunId,
    SessionId, SkillId, SkillSourceKind, SubagentId, TenantId, ToolCapability, ToolError,
    ToolUseId, TranscriptRef, TurnInput, UsageSnapshot,
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
    NotCompiled { capability: AgentCapabilityKind },
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
    PrerequisiteMissing { env_vars: Vec<String> },
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

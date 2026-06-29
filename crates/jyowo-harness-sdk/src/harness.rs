#[cfg(feature = "tool-search")]
use std::collections::BTreeSet;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
#[cfg(feature = "stream-permission")]
use std::thread;
use std::time::Duration;

use async_trait::async_trait;
#[cfg(feature = "mcp-server-adapter")]
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
#[cfg(feature = "agents-team")]
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::stream::BoxStream;
use futures::StreamExt;
use harness_context::{ContextEngine, TokenBudget};
#[cfg(feature = "mcp-server-adapter")]
use harness_contracts::BlobRef;
#[cfg(feature = "tool-search")]
use harness_contracts::CacheImpact;
#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
use harness_contracts::MemdirFileTag;
#[cfg(not(feature = "observability-redactor"))]
use harness_contracts::RedactPatternKind;
#[cfg(feature = "stream-permission")]
use harness_contracts::RequestId;
#[cfg(feature = "agents-team")]
use harness_contracts::{
    BlobMeta, BlobRetention, TeamCreatedEvent, TeamMemberJoinedEvent, TopologyKind,
};
use harness_contracts::{
    BlobReaderCapAdapter, BlobStore, BlobWriterCapAdapter, CapabilityRegistry, ContextPatchRequest,
    ContextPatchSinkCap, ConversationAttachmentReference, ConversationContextReference,
    ConversationTurnInput, Decision, Event, EventId, HarnessError, HookEventKind,
    InteractivityLevel, JournalOffset, ManifestOriginRef, ManifestValidationFailedEvent,
    McpServerId, Message, MessageContent, MessageId, MessagePart, MessageRole, ModelModality,
    PermissionError, PermissionMode, PluginCapabilitiesSummary, PluginLifecycleStateDiscriminant,
    PluginLoadedEvent, PluginRejectedEvent, ProviderCapabilityRouteSettings, RedactPatternSet,
    RedactRules, RedactScope, Redactor, RejectionReason, RunId, SessionError, SessionId, TenantId,
    ToolCapability, ToolSearchMode, TrustLevel, TurnInput,
};
#[cfg(feature = "sqlite-store")]
use harness_contracts::{
    ConversationCursor, ConversationSnapshot, ConversationSummary, ConversationTimelinePage,
    ConversationTurnCursor, ConversationWorktreePage,
};
#[cfg(feature = "memory-builtin")]
use harness_contracts::{MemdirOverflowEvent, OverflowStrategy};
use harness_engine::{
    CancellationToken, Engine, EngineRunner, InterruptCause, RunContext, SessionHandle,
};
#[cfg(feature = "steering-queue")]
use harness_engine::{SteeringDrain, SteeringMerge};
use harness_hook::{
    DispatchResult, ExecHookTransport, HookContext, HookDispatcher, HookEvent,
    HookExecResourceLimits, HookExecSignalPolicy, HookExecSpec, HookFailureCause, HookHandler,
    HookHttpAuth, HookHttpSecurityPolicy, HookHttpSpec, HookMessageView, HookOutcome,
    HookProtocolVersion, HookRegistry, HookSessionView, HostAllowlist, HttpHookTransport,
    NotificationKind, ReplayMode, SsrfGuardPolicy, SubagentSpecView, ToolDescriptorView,
    WorkingDir,
};
#[cfg(feature = "sqlite-store")]
use harness_journal::ConversationTurnPageDirection;
#[cfg(feature = "sqlite-store")]
use harness_journal::SqliteConversationReadModelStore;
use harness_journal::{
    AppendMetadata, AuditPage, AuditQuery, AuditStore, EventEnvelope, EventStore, EventStoreAudit,
    EventStoreOffloadedBlobAuthorizer, PrunePolicy, PruneReport, ReplayCursor, SessionFilter,
    SessionSnapshot, SessionSummary,
};
use harness_mcp::{
    ElicitationHandler, McpEventSink, McpMetric, McpMetricConnectionState, McpMetricsSink,
    McpRegistry, SamplingProvider, SamplingRequest, SamplingResponse, StreamElicitationHandler,
};
#[cfg(feature = "mcp-server-adapter")]
use harness_mcp::{ExposedCapability, HarnessMcpBackend, McpServerError, McpServerRequestContext};
#[cfg(feature = "memory-consolidation")]
use harness_memory::ConsolidationHook;
use harness_memory::MemoryProvider;
use harness_model::{provider_catalog_entries, ModelRuntimeSnapshot};
use harness_model::{
    AuxModelProvider, ContentDelta, InferContext, InferMiddleware, ModelMetricsSink, ModelProtocol,
    ModelProvider, ModelRequest, ModelStreamEvent,
};
#[cfg(feature = "observability-redactor")]
use harness_observability::DefaultRedactor;
use harness_observability::{AttributeValue, Observer, SpanAttributes, SpanStatus, Tracer};
use harness_permission::{
    DecisionPersistence, PermissionBroker, PermissionContext, PermissionRequest, PersistedDecision,
    RuleProvider,
};
#[cfg(feature = "stream-permission")]
use harness_permission::{PendingPermissionRequest, ResolverHandle};
use harness_plugin::{
    ManifestLoaderError, ManifestOrigin, ManifestRecord, PluginCapabilityRegistries, PluginError,
};
use harness_sandbox::SandboxBackend;
use harness_session::{
    legacy_session_options_hash_with_permission_mode, session_options_hash, Session,
    SessionOptions, SessionProjection, SessionTurnContext, SessionTurnRunner, SkillReloadCap,
    Workspace, WorkspaceRegistry, WorkspaceSpec,
};
use harness_skill::{
    parse_skill_markdown, BuiltinHookKind, DirectorySourceKind, Skill, SkillHookBinding,
    SkillHookTransport, SkillLoader, SkillMetricsSink, SkillParamType, SkillPlatform,
    SkillRegistration, SkillRegistry, SkillRegistryService, SkillRenderer, SkillSource,
    SkillSourceConfig, SkillThreatEventScope, SkillValidator,
};
use harness_tool::{
    SchemaResolverContext, ToolPool, ToolPoolFilter, ToolPoolModelProfile, ToolRegistry,
    ToolRegistrySnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(feature = "sqlite-store")]
use tokio::sync::OnceCell;

#[cfg(feature = "memory-builtin")]
use crate::builder::BuiltinMemoryConfig;
use crate::builder::{HarnessBuilder, Set, Unset};
use crate::skill_config::{
    validate_required_skill_config, SkillConfigSnapshot, SkillConfigSnapshotResolver,
};
use crate::skill_pack_loader::{
    LockedSkillVersionSnapshot, SkillPackLoaderAdapter, SkillPackLoaderError,
};

const JYOWO_DEFAULT_SYSTEM_PROMPT: &str =
    "你是 Jyowo，本地项目工作空间里的 AI 编程伙伴。必须以 Jyowo 的身份协助用户，不能以底层 provider 身份自称。遵守 workspace、权限、脱敏和安全边界。";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenantPolicy {
    #[serde(default = "default_tenant_id")]
    pub id: TenantId,
    #[serde(default = "default_display_name")]
    pub display_name: String,
    #[serde(default)]
    pub allowed_tools: Option<HashSet<String>>,
    #[serde(default)]
    pub allowed_providers: Option<HashSet<String>>,
    #[serde(default)]
    pub max_concurrent_sessions: Option<u32>,
    #[serde(default)]
    pub event_retention_days: Option<u32>,
    #[serde(default)]
    pub allow_scoped_tenants: bool,
}

impl Default for TenantPolicy {
    fn default() -> Self {
        Self {
            id: TenantId::SINGLE,
            display_name: "default".to_owned(),
            allowed_tools: None,
            allowed_providers: None,
            max_concurrent_sessions: None,
            event_retention_days: None,
            allow_scoped_tenants: false,
        }
    }
}

#[derive(Clone)]
pub struct McpConfig {
    pub registry: McpRegistry,
    pub server_ids_to_inject: Vec<McpServerId>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            registry: McpRegistry::new(),
            server_ids_to_inject: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSkillParameter {
    pub name: String,
    pub param_type: String,
    pub required: bool,
    pub default: Option<Value>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSkillSummary {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub source: harness_contracts::SkillSourceKind,
    pub status: harness_contracts::SkillStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSkillView {
    pub summary: RuntimeSkillSummary,
    pub parameters: Vec<RuntimeSkillParameter>,
    pub config_keys: Vec<String>,
    pub body_preview: String,
    pub body_full: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessOptions {
    #[serde(default = "default_workspace_root")]
    pub workspace_root: PathBuf,
    #[serde(default = "default_model_id")]
    pub model_id: String,
    #[serde(default = "default_tool_search_enabled")]
    pub tool_search_enabled: bool,
    #[serde(default)]
    pub tenant_policy: TenantPolicy,
    #[serde(default)]
    pub default_session_options: SessionOptions,
    #[serde(default)]
    pub concurrent_sessions: Option<u32>,
    #[serde(default)]
    pub enable_replay: bool,
}

impl Default for HarnessOptions {
    fn default() -> Self {
        let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            workspace_root: workspace_root.clone(),
            model_id: "default".to_owned(),
            tool_search_enabled: true,
            tenant_policy: TenantPolicy::default(),
            default_session_options: SessionOptions::new(workspace_root),
            concurrent_sessions: None,
            enable_replay: false,
        }
    }
}

fn default_tenant_id() -> TenantId {
    TenantId::SINGLE
}

fn default_display_name() -> String {
    "default".to_owned()
}

fn default_workspace_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn default_model_id() -> String {
    "default".to_owned()
}

fn default_tool_search_enabled() -> bool {
    true
}

#[derive(Clone)]
pub struct Harness {
    inner: Arc<HarnessInner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationSession {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub message_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationSessionSummary {
    pub session_id: SessionId,
    pub created_at: DateTime<Utc>,
    pub last_event_at: DateTime<Utc>,
    pub event_count: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversationTurnRequest {
    pub options: SessionOptions,
    pub input: ConversationTurnInput,
    pub permission_mode_override: Option<PermissionMode>,
}

impl ConversationTurnRequest {
    #[must_use]
    pub fn from_prompt(options: SessionOptions, prompt: impl Into<String>) -> Self {
        Self {
            options,
            input: ConversationTurnInput::ask(prompt),
            permission_mode_override: None,
        }
    }
}

fn render_conversation_turn_prompt(
    input: &ConversationTurnInput,
    supported_modalities: &[ModelModality],
) -> String {
    let text_attachments = input
        .attachments
        .iter()
        .filter(|attachment| {
            !is_image_attachment(attachment)
                && !is_video_attachment(attachment)
                && !(supports_file_input(supported_modalities) && is_file_attachment(attachment))
        })
        .collect::<Vec<_>>();
    if input.context_references.is_empty() && text_attachments.is_empty() {
        return input.prompt.clone();
    }

    let mut lines = vec!["<conversation-context>".to_owned()];

    if !input.context_references.is_empty() {
        lines.push("references:".to_owned());
        lines.extend(
            input
                .context_references
                .iter()
                .map(render_context_reference),
        );
    }

    if !text_attachments.is_empty() {
        lines.push("attachments:".to_owned());
        lines.extend(
            text_attachments
                .into_iter()
                .map(render_attachment_reference),
        );
    }

    lines.push("</conversation-context>".to_owned());
    lines.push(input.prompt.clone());
    lines.join("\n")
}

fn conversation_turn_parts(
    input: &ConversationTurnInput,
    supported_modalities: &[ModelModality],
) -> Vec<MessagePart> {
    let mut parts = vec![MessagePart::Text(render_conversation_turn_prompt(
        input,
        supported_modalities,
    ))];
    parts.extend(input.attachments.iter().filter_map(|attachment| {
        if is_image_attachment(attachment) {
            Some(MessagePart::Image {
                mime_type: attachment.mime_type.clone(),
                blob_ref: attachment.blob_ref.clone(),
            })
        } else if is_video_attachment(attachment) {
            Some(MessagePart::Video {
                mime_type: attachment.mime_type.clone(),
                blob_ref: attachment.blob_ref.clone(),
            })
        } else if supports_file_input(supported_modalities) && is_file_attachment(attachment) {
            Some(MessagePart::File {
                mime_type: attachment.mime_type.clone(),
                blob_ref: attachment.blob_ref.clone(),
            })
        } else {
            None
        }
    }));
    parts
}

fn is_image_attachment(attachment: &ConversationAttachmentReference) -> bool {
    attachment.mime_type.starts_with("image/")
}

fn is_video_attachment(attachment: &ConversationAttachmentReference) -> bool {
    attachment.mime_type.starts_with("video/")
}

fn is_file_attachment(attachment: &ConversationAttachmentReference) -> bool {
    !is_image_attachment(attachment) && !is_video_attachment(attachment)
}

fn supports_file_input(supported_modalities: &[ModelModality]) -> bool {
    supported_modalities.contains(&ModelModality::File)
}

fn render_context_reference(reference: &ConversationContextReference) -> String {
    match reference {
        ConversationContextReference::WorkspaceFile { path, label } => {
            format!(
                "- workspace_file: {} ({})",
                sanitize_context_line(label),
                sanitize_context_line(path)
            )
        }
        ConversationContextReference::Artifact { id, label } => {
            format!(
                "- artifact: {} ({})",
                sanitize_context_line(label),
                sanitize_context_line(id)
            )
        }
        ConversationContextReference::Conversation { id, label } => {
            format!(
                "- conversation: {} ({})",
                sanitize_context_line(label),
                sanitize_context_line(id)
            )
        }
        ConversationContextReference::Memory { id, label } => {
            format!(
                "- memory: {} ({})",
                sanitize_context_line(label),
                sanitize_context_line(id)
            )
        }
        ConversationContextReference::Skill { id, label } => {
            format!(
                "- skill: {} ({})",
                sanitize_context_line(label),
                sanitize_context_line(id)
            )
        }
        ConversationContextReference::Tool { id, label } => {
            format!(
                "- tool: {} ({})",
                sanitize_context_line(label),
                sanitize_context_line(id)
            )
        }
        ConversationContextReference::McpServer { id, label } => {
            format!(
                "- mcp_server: {} ({})",
                sanitize_context_line(label),
                sanitize_context_line(id)
            )
        }
    }
}

fn render_attachment_reference(attachment: &ConversationAttachmentReference) -> String {
    format!(
        "- attachment: {} {} {} bytes {}",
        sanitize_context_line(&attachment.name),
        sanitize_context_line(&attachment.mime_type),
        attachment.size_bytes,
        sanitize_context_line(&attachment.id)
    )
}

fn sanitize_context_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationTurnReceipt {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub message_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversationEventsPageRequest {
    pub options: SessionOptions,
    pub after_event_id: Option<EventId>,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversationEventsPage {
    pub events: Vec<EventEnvelope>,
    pub next_event_id: Option<EventId>,
}

#[cfg(feature = "stream-permission")]
pub struct StreamPermissionRuntime {
    permission_broker: Arc<dyn PermissionBroker>,
    resolver: ResolverHandle,
}

#[cfg(feature = "stream-permission")]
impl StreamPermissionRuntime {
    #[must_use]
    pub fn new(config: harness_permission::StreamBrokerConfig) -> Self {
        let (broker, mut receiver, resolver) = harness_permission::StreamBasedBroker::new(config);

        thread::spawn(move || while receiver.blocking_recv().is_some() {});

        Self {
            permission_broker: Arc::new(broker),
            resolver,
        }
    }

    #[must_use]
    pub fn broker(&self) -> Arc<dyn PermissionBroker> {
        Arc::clone(&self.permission_broker)
    }

    #[must_use]
    pub fn resolver_handle(&self) -> ResolverHandle {
        self.resolver.clone()
    }

    #[must_use]
    pub fn pending_requests(&self) -> Vec<PermissionRequest> {
        self.resolver.pending_requests()
    }

    #[must_use]
    pub fn pending_permission_requests(&self) -> Vec<PendingPermissionRequest> {
        self.resolver.pending_permission_requests()
    }

    pub async fn resolve_permission(
        &self,
        request_id: RequestId,
        decision: Decision,
    ) -> Result<(), HarnessError> {
        self.resolver
            .resolve(request_id, decision)
            .await
            .map_err(HarnessError::Permission)
    }
}

#[cfg(feature = "stream-permission")]
impl Default for StreamPermissionRuntime {
    fn default() -> Self {
        Self::new(harness_permission::StreamBrokerConfig {
            default_timeout: Some(Duration::from_secs(300)),
            heartbeat_interval: None,
            max_pending: 1024,
        })
    }
}

struct HarnessInner {
    options: HarnessOptions,
    model: Arc<dyn ModelProvider>,
    event_store: Arc<dyn EventStore>,
    #[cfg(feature = "sqlite-store")]
    conversation_read_model: OnceCell<Arc<SqliteConversationReadModelStore>>,
    sandbox: Arc<dyn SandboxBackend>,
    permission_broker: Arc<dyn PermissionBroker>,
    #[cfg(feature = "stream-permission")]
    permission_resolver: Option<ResolverHandle>,
    tool_registry: ToolRegistry,
    hook_registry: HookRegistry,
    memory_provider: Option<Arc<dyn MemoryProvider>>,
    #[cfg(feature = "memory-consolidation")]
    consolidation_hook: Option<Arc<dyn ConsolidationHook>>,
    #[cfg(feature = "memory-builtin")]
    builtin_memory: Option<BuiltinMemoryConfig>,
    blob_store: Option<Arc<dyn BlobStore>>,
    skill_loader: Option<SkillLoader>,
    skill_config_snapshot: SkillConfigSnapshot,
    skill_registry: SkillRegistry,
    mcp_config: Option<McpConfig>,
    elicitation_handler: Option<Arc<dyn ElicitationHandler>>,
    stream_elicitation_handler: Option<StreamElicitationHandler>,
    plugin_registry: Option<harness_plugin::PluginRegistry>,
    tracer: Option<Arc<dyn Tracer>>,
    observer: Option<Arc<Observer>>,
    aux_model: Option<Arc<dyn AuxModelProvider>>,
    model_middlewares: Vec<Arc<dyn InferMiddleware>>,
    rule_providers: Vec<Arc<dyn RuleProvider>>,
    cap_registry: Arc<CapabilityRegistry>,
    #[cfg(feature = "tool-search")]
    tool_search_scorer: Option<Arc<dyn harness_tool_search::ToolSearchScorer>>,
    enabled_features: HashSet<String>,
    session_limits: Arc<SessionLimitState>,
    workspace_registry: Arc<WorkspaceRegistry>,
    active_conversation_runs: Arc<parking_lot::Mutex<HashMap<RunId, ActiveConversationRun>>>,
    deleted_conversation_sessions: Arc<parking_lot::Mutex<HashSet<(TenantId, SessionId)>>>,
    provider_capability_routes: Arc<parking_lot::RwLock<ProviderCapabilityRouteSettings>>,
}

#[derive(Clone)]
struct ActiveConversationRun {
    tenant_id: TenantId,
    session_id: SessionId,
    cancellation: CancellationToken,
}

struct SdkSessionState {
    projection: SessionProjection,
}

fn sdk_session_not_found(session_id: SessionId) -> HarnessError {
    HarnessError::Session(SessionError::Message(format!(
        "session not found: {session_id}"
    )))
}

#[cfg(feature = "sqlite-store")]
fn parse_conversation_session_id(conversation_id: &str) -> Result<SessionId, HarnessError> {
    SessionId::parse(conversation_id).map_err(|error| {
        HarnessError::Session(SessionError::Message(format!(
            "invalid conversation id: {error}"
        )))
    })
}

pub trait WorkspaceCreateRequest {
    type Output;

    fn create_workspace(self, harness: &Harness) -> Result<Self::Output, HarnessError>;
}

#[derive(Clone)]
pub struct HarnessSamplingProvider {
    model: Arc<dyn ModelProvider>,
    default_model_id: String,
    tenant_id: TenantId,
    session_id: Option<harness_contracts::SessionId>,
    run_id: Option<RunId>,
}

impl HarnessSamplingProvider {
    #[must_use]
    pub fn new(
        model: Arc<dyn ModelProvider>,
        default_model_id: impl Into<String>,
        tenant_id: TenantId,
        session_id: Option<harness_contracts::SessionId>,
        run_id: Option<RunId>,
    ) -> Self {
        Self {
            model,
            default_model_id: default_model_id.into(),
            tenant_id,
            session_id,
            run_id,
        }
    }
}

#[async_trait]
impl SamplingProvider for HarnessSamplingProvider {
    async fn create_message(
        &self,
        request: SamplingRequest,
    ) -> Result<SamplingResponse, harness_mcp::McpError> {
        let model_id = request
            .model_id
            .clone()
            .unwrap_or_else(|| self.default_model_id.clone());
        let model_snapshot = snapshot_for_supported_model(self.model.as_ref(), &model_id)
            .map_err(|error| harness_mcp::McpError::Protocol(error.to_string()))?;
        let messages = sampling_messages_from_params(&request.params)?;
        let model_request = ModelRequest {
            model_id: model_id.clone(),
            messages,
            tools: None,
            system: sampling_string_param(&request.params, "systemPrompt")
                .or_else(|| sampling_string_param(&request.params, "system")),
            temperature: request
                .params
                .get("temperature")
                .and_then(Value::as_f64)
                .map(|value| value as f32),
            max_tokens: (request.max_output_tokens > 0)
                .then(|| request.max_output_tokens.min(u64::from(u32::MAX)) as u32),
            stream: true,
            cache_breakpoints: Vec::new(),
            protocol: model_snapshot.protocol,
            extra: json!({
                "source": "mcp_sampling",
                "server_id": request.server_id.0,
                "request_id": request.request_id,
                "prompt_cache_namespace": request.prompt_cache_namespace,
            }),
        };
        let mut context = InferContext::for_test();
        context.tenant_id = self.tenant_id;
        context.session_id = self.session_id.or(Some(request.session_id));
        context.run_id = self.run_id.or(request.run_id);
        let mut stream = self
            .model
            .infer(model_request, context)
            .await
            .map_err(|error| harness_mcp::McpError::Protocol(error.to_string()))?;
        let mut text = String::new();
        let mut usage = harness_contracts::UsageSnapshot::default();
        while let Some(event) = stream.next().await {
            match event {
                ModelStreamEvent::MessageStart {
                    usage: start_usage, ..
                } => add_usage(&mut usage, &start_usage),
                ModelStreamEvent::ContentBlockDelta { delta, .. } => match delta {
                    ContentDelta::Text(delta) => text.push_str(&delta),
                    ContentDelta::Thinking(thinking) => {
                        if let Some(delta) = thinking.text {
                            text.push_str(&delta);
                        }
                    }
                    ContentDelta::ReasoningSummary(_) => {}
                    ContentDelta::ToolUseStart { .. }
                    | ContentDelta::ToolUseInputJson(_)
                    | ContentDelta::ToolUseComplete { .. } => {}
                },
                ModelStreamEvent::MessageDelta { usage_delta, .. } => {
                    add_usage(&mut usage, &usage_delta);
                }
                ModelStreamEvent::StreamError { error, .. } => {
                    return Err(harness_mcp::McpError::Protocol(error.to_string()));
                }
                ModelStreamEvent::MessageStop
                | ModelStreamEvent::ContentBlockStart { .. }
                | ModelStreamEvent::ContentBlockStop { .. } => {}
            }
        }
        Ok(SamplingResponse {
            model_id,
            content: json!({ "type": "text", "text": text }),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
        })
    }
}

fn sampling_messages_from_params(params: &Value) -> Result<Vec<Message>, harness_mcp::McpError> {
    let Some(messages) = params.get("messages").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    messages
        .iter()
        .map(|message| {
            let role = match message.get("role").and_then(Value::as_str) {
                Some("assistant") => MessageRole::Assistant,
                Some("system") => MessageRole::System,
                Some("tool") => MessageRole::Tool,
                Some("user") | None => MessageRole::User,
                Some(other) => {
                    return Err(harness_mcp::McpError::Protocol(format!(
                        "unsupported sampling message role: {other}"
                    )))
                }
            };
            let content = message.get("content").unwrap_or(&Value::Null);
            Ok(Message {
                id: MessageId::new(),
                role,
                parts: vec![MessagePart::Text(sampling_content_text(content))],
                created_at: harness_contracts::now(),
            })
        })
        .collect()
}

fn sampling_content_text(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    if let Some(text) = content.get("text").and_then(Value::as_str) {
        return text.to_owned();
    }
    if let Some(parts) = content.as_array() {
        return parts
            .iter()
            .filter_map(|part| {
                part.as_str().map(ToOwned::to_owned).or_else(|| {
                    part.get("text")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
            })
            .collect::<Vec<_>>()
            .join("");
    }
    String::new()
}

fn sampling_string_param(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn add_usage(
    total: &mut harness_contracts::UsageSnapshot,
    delta: &harness_contracts::UsageSnapshot,
) {
    total.input_tokens = total.input_tokens.saturating_add(delta.input_tokens);
    total.output_tokens = total.output_tokens.saturating_add(delta.output_tokens);
    total.cache_read_tokens = total
        .cache_read_tokens
        .saturating_add(delta.cache_read_tokens);
    total.cache_write_tokens = total
        .cache_write_tokens
        .saturating_add(delta.cache_write_tokens);
    total.cost_micros = total.cost_micros.saturating_add(delta.cost_micros);
    total.tool_calls = total.tool_calls.saturating_add(delta.tool_calls);
}

struct SessionLimitState {
    max: Option<u32>,
    active: AtomicU32,
}

struct SessionLimitPermit {
    state: Arc<SessionLimitState>,
    armed: bool,
}

impl SessionLimitState {
    fn new(max: Option<u32>) -> Self {
        Self {
            max,
            active: AtomicU32::new(0),
        }
    }

    fn try_acquire(self: &Arc<Self>) -> Result<SessionLimitPermit, HarnessError> {
        let Some(max) = self.max else {
            return Ok(SessionLimitPermit {
                state: Arc::clone(self),
                armed: false,
            });
        };

        loop {
            let active = self.active.load(Ordering::Acquire);
            if active >= max {
                return Err(HarnessError::PermissionDenied(format!(
                    "tenant session limit exceeded: active={active}, max={max}"
                )));
            }
            if self
                .active
                .compare_exchange(active, active + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(SessionLimitPermit {
                    state: Arc::clone(self),
                    armed: true,
                });
            }
        }
    }

    fn release(&self) {
        if self.max.is_some() {
            let _ = self
                .active
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                    active.checked_sub(1)
                });
        }
    }
}

impl SessionLimitPermit {
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for SessionLimitPermit {
    fn drop(&mut self) {
        if self.armed {
            self.state.release();
        }
    }
}

impl Harness {
    #[must_use]
    pub fn builder() -> HarnessBuilder<Unset, Unset, Unset> {
        HarnessBuilder::new()
    }

    #[cfg(feature = "sqlite-store")]
    async fn conversation_read_model(
        &self,
    ) -> Result<Arc<SqliteConversationReadModelStore>, HarnessError> {
        let path = self
            .inner
            .options
            .workspace_root
            .join(".jyowo/runtime/conversation-read-model.sqlite");
        let store = self
            .inner
            .conversation_read_model
            .get_or_try_init(|| async move {
                SqliteConversationReadModelStore::open(path)
                    .await
                    .map(Arc::new)
                    .map_err(HarnessError::Journal)
            })
            .await?;
        Ok(Arc::clone(store))
    }

    pub(crate) async fn from_builder(
        builder: HarnessBuilder<
            Set<Arc<dyn ModelProvider>>,
            Set<Arc<dyn EventStore>>,
            Set<Arc<dyn SandboxBackend>>,
        >,
    ) -> Result<Self, HarnessError> {
        let mut extras = builder.extras;
        let tool_registry = match extras.tool_registry.take() {
            Some(registry) => registry,
            None => ToolRegistry::builder().build().map_err(|error| {
                HarnessError::Tool(harness_contracts::ToolError::Message(error.to_string()))
            })?,
        };
        let hook_registry = match extras.hook_registry.take() {
            Some(registry) => registry,
            None => HookRegistry::builder().build().map_err(|error| {
                HarnessError::Hook(harness_contracts::HookError::Message(error.to_string()))
            })?,
        };
        let permission_broker = match extras.permission_broker.take() {
            Some(broker) => {
                policy_gated_permission_broker(&builder.options, broker, &extras.rule_providers)
                    .await?
            }
            None => {
                default_permission_broker(
                    &builder.options,
                    &extras.rule_providers,
                    extras.decision_persistence.take(),
                )
                .await?
            }
        };
        let skill_registry = SkillRegistry::builder().build();
        let mut mcp_config = extras.mcp_config.take();
        let plugin_registry = extras.plugin_registry.take();
        if let Some(registry) = &plugin_registry {
            let mut capability_registries = PluginCapabilityRegistries::default()
                .with_tool_registry(tool_registry.clone())
                .with_hook_registry(hook_registry.clone())
                .with_skill_registry(skill_registry.clone());
            if let Some(config) = &mcp_config {
                capability_registries =
                    capability_registries.with_mcp_registry(config.registry.clone());
            }
            registry.set_capability_registries(capability_registries);
        }

        let observer = match extras.observer.take() {
            Some(observer) => Some(observer),
            None => Some(Arc::new(
                Observer::builder()
                    .build()
                    .map_err(|error| HarnessError::Other(error.to_string()))?,
            )),
        };
        let tracer = extras.tracer.take().or_else(|| {
            observer
                .as_ref()
                .map(|observer| Arc::clone(observer) as Arc<dyn Tracer>)
        });
        if let (Some(config), Some(observer)) = (&mut mcp_config, observer.as_ref()) {
            config.registry =
                config
                    .registry
                    .clone_with_metrics_sink(Arc::new(SdkMcpMetricsSink {
                        observer: Arc::clone(observer),
                    }));
        }
        let session_limits = Arc::new(SessionLimitState::new(
            builder
                .options
                .tenant_policy
                .max_concurrent_sessions
                .or(builder.options.concurrent_sessions),
        ));
        let (elicitation_handler, stream_elicitation_handler) =
            match extras.stream_elicitation_handler.take() {
                Some(handler) => (
                    extras
                        .elicitation_handler
                        .take()
                        .or_else(|| Some(Arc::new(handler.clone()) as Arc<dyn ElicitationHandler>)),
                    Some(handler),
                ),
                None if extras.elicitation_handler.is_some() => {
                    (extras.elicitation_handler.take(), None)
                }
                None => {
                    let session_id = harness_contracts::SessionId::default();
                    let handler = StreamElicitationHandler::new(
                        session_id,
                        None,
                        Arc::new(SdkMcpEventSink {
                            event_store: Arc::clone(&builder.store.0),
                            tenant_id: builder.options.tenant_policy.id,
                            session_id,
                        }),
                    );
                    (
                        Some(Arc::new(handler.clone()) as Arc<dyn ElicitationHandler>),
                        Some(handler),
                    )
                }
            };

        Ok(Self {
            inner: Arc::new(HarnessInner {
                options: builder.options,
                model: builder.model.0,
                event_store: builder.store.0,
                #[cfg(feature = "sqlite-store")]
                conversation_read_model: OnceCell::new(),
                sandbox: builder.sandbox.0,
                permission_broker,
                #[cfg(feature = "stream-permission")]
                permission_resolver: extras.permission_resolver.take(),
                tool_registry,
                hook_registry,
                memory_provider: extras.memory_provider.take(),
                #[cfg(feature = "memory-consolidation")]
                consolidation_hook: extras.consolidation_hook.take(),
                #[cfg(feature = "memory-builtin")]
                builtin_memory: extras.builtin_memory.take(),
                blob_store: extras.blob_store.take(),
                skill_loader: extras.skill_loader.take(),
                skill_config_snapshot: extras.skill_config_snapshot.take().unwrap_or_default(),
                skill_registry,
                mcp_config,
                elicitation_handler,
                stream_elicitation_handler,
                plugin_registry,
                tracer,
                observer,
                aux_model: extras.aux_model.take(),
                model_middlewares: extras.model_middlewares,
                rule_providers: extras.rule_providers,
                cap_registry: Arc::new(extras.cap_registry.take().unwrap_or_default()),
                #[cfg(feature = "tool-search")]
                tool_search_scorer: extras.tool_search_scorer.take(),
                enabled_features: Self::enabled_feature_set(),
                session_limits,
                workspace_registry: Arc::new(WorkspaceRegistry::new()),
                active_conversation_runs: Arc::new(parking_lot::Mutex::new(HashMap::new())),
                deleted_conversation_sessions: Arc::new(parking_lot::Mutex::new(HashSet::new())),
                provider_capability_routes: extras
                    .provider_capability_routes
                    .take()
                    .unwrap_or_else(|| {
                        Arc::new(parking_lot::RwLock::new(ProviderCapabilityRouteSettings {
                            version: 1,
                            routes: Vec::new(),
                        }))
                    }),
            }),
        })
    }

    pub async fn create_session(&self, options: SessionOptions) -> Result<Session, HarnessError> {
        let mut options = self.effective_session_options(options)?;
        if !self.inner.options.tool_search_enabled {
            options.tool_search = ToolSearchMode::Disabled;
        }
        self.enforce_tenant(&options)?;
        let limit_permit = self.inner.session_limits.try_acquire()?;
        #[cfg(feature = "memory-external-slot")]
        self.activate_plugins(&options).await?;
        #[cfg(feature = "memory-external-slot")]
        let memory_manager = self.memory_manager_for_session(&options).await?;
        let pending_session_events = Arc::new(PendingSessionEvents::default());
        #[cfg(feature = "memory-external-slot")]
        let engine = self
            .engine_for_session(
                &options,
                memory_manager.clone(),
                Some(Arc::clone(&pending_session_events)),
            )
            .await?;
        #[cfg(not(feature = "memory-external-slot"))]
        let engine = self
            .engine_for_session(&options, Some(Arc::clone(&pending_session_events)))
            .await?;
        let tenant_id = options.tenant_id;
        let session_id = options.session_id;
        let event_store: Arc<dyn EventStore> = Arc::new(LifecycleHookEventStore {
            inner: Arc::clone(&self.inner.event_store),
            hooks: HookDispatcher::new(self.inner.hook_registry.snapshot()),
            tenant_id: options.tenant_id,
            session_id: options.session_id,
            #[cfg(feature = "memory-external-slot")]
            user_id: options.user_id.clone(),
            #[cfg(feature = "memory-external-slot")]
            team_id: options.team_id,
            workspace_root: options.workspace_root.clone(),
            redactor: self.hook_redactor(),
            session_limits: Arc::clone(&self.inner.session_limits),
            deleted_conversation_sessions: Arc::clone(&self.inner.deleted_conversation_sessions),
            summary_state: parking_lot::Mutex::new(MemorySessionSummaryState::default()),
            #[cfg(feature = "memory-external-slot")]
            memory_manager,
        });

        let session = Session::builder()
            .with_options(options)
            .with_event_store(event_store)
            .with_turn_runner(Arc::new(EngineSessionTurnRunner {
                engine,
                active_conversation_runs: Arc::clone(&self.inner.active_conversation_runs),
                skill_registry: Some(self.inner.skill_registry.clone()),
                skill_metrics_sink: self.skill_metrics_sink(),
                skill_config_snapshot: self.inner.skill_config_snapshot.clone(),
            }))
            .with_skill_reload_cap(Arc::new(SdkSkillReloadCap {
                inner: Arc::clone(&self.inner),
            }))
            .build()
            .await
            .map_err(HarnessError::from)?;
        let pending_events = pending_session_events.drain();
        if !pending_events.is_empty() {
            self.inner
                .event_store
                .append(tenant_id, session_id, &pending_events)
                .await
                .map_err(HarnessError::Journal)?;
        }
        limit_permit.disarm();
        Ok(session)
    }

    pub async fn open_or_create_conversation_session(
        &self,
        options: SessionOptions,
    ) -> Result<ConversationSession, HarnessError> {
        let effective = self.effective_sdk_session_options(options.clone())?;
        self.ensure_conversation_session_not_deleted(effective.tenant_id, effective.session_id)?;
        match self.read_sdk_session_state(&effective).await? {
            Some(state) => Ok(ConversationSession {
                tenant_id: state.projection.tenant_id,
                session_id: state.projection.session_id,
                message_count: state.projection.messages.len(),
            }),
            None => {
                let session = self.create_session(options).await?;
                let projection = session.projection().await;
                Ok(ConversationSession {
                    tenant_id: projection.tenant_id,
                    session_id: projection.session_id,
                    message_count: projection.messages.len(),
                })
            }
        }
    }

    pub async fn list_conversation_sessions(
        &self,
        tenant_id: TenantId,
        limit: u32,
    ) -> Result<Vec<ConversationSessionSummary>, HarnessError> {
        let sessions = self
            .inner
            .event_store
            .list_sessions(
                tenant_id,
                SessionFilter {
                    since: None,
                    end_reason: None,
                    project_compression_tips: false,
                    limit,
                },
            )
            .await
            .map_err(HarnessError::Journal)?;

        let mut conversation_sessions = Vec::new();
        for session in sessions {
            if session.end_reason.is_some() {
                continue;
            }
            if self
                .is_conversation_session_stream(tenant_id, session.session_id)
                .await?
            {
                conversation_sessions.push(session);
            }
        }
        conversation_sessions.sort_by_key(|session| session.last_event_at);
        conversation_sessions.reverse();

        Ok(conversation_sessions
            .into_iter()
            .map(|session| ConversationSessionSummary {
                session_id: session.session_id,
                created_at: session.created_at,
                last_event_at: session.last_event_at,
                event_count: session.event_count,
            })
            .collect())
    }

    #[cfg(feature = "sqlite-store")]
    pub async fn list_conversation_summaries(
        &self,
        tenant_id: TenantId,
        limit: usize,
    ) -> Result<Vec<ConversationSummary>, HarnessError> {
        let bounded_limit = limit.clamp(1, 200);
        let sessions = self
            .inner
            .event_store
            .list_sessions(
                tenant_id,
                SessionFilter {
                    since: None,
                    end_reason: None,
                    project_compression_tips: false,
                    limit: u32::MAX,
                },
            )
            .await
            .map_err(HarnessError::Journal)?;
        let mut live_conversation_session_ids = HashSet::new();
        for session in sessions {
            if self
                .is_conversation_session_stream_page(tenant_id, session.session_id)
                .await?
            {
                live_conversation_session_ids.insert(session.session_id);
                self.catch_up_conversation_projection(tenant_id, session.session_id)
                    .await?;
            }
        }
        let read_model = self.conversation_read_model().await?;
        loop {
            let summaries = read_model
                .list_summaries(tenant_id, 200)
                .await
                .map_err(HarnessError::Journal)?;
            let mut visible_summaries = Vec::new();
            let mut removed_stale_summary = false;
            for summary in summaries {
                let Ok(session_id) = SessionId::parse(&summary.id) else {
                    continue;
                };
                if live_conversation_session_ids.contains(&session_id) {
                    visible_summaries.push(summary);
                } else {
                    read_model
                        .reset_session(tenant_id, session_id)
                        .await
                        .map_err(HarnessError::Journal)?;
                    removed_stale_summary = true;
                }
                if visible_summaries.len() >= bounded_limit {
                    break;
                }
            }
            if visible_summaries.len() >= bounded_limit || !removed_stale_summary {
                return Ok(visible_summaries);
            }
        }
    }

    #[cfg(feature = "sqlite-store")]
    pub async fn get_conversation_snapshot(
        &self,
        conversation_id: &str,
        message_limit: usize,
    ) -> Result<Option<ConversationSnapshot>, HarnessError> {
        let tenant_id = self.inner.options.tenant_policy.id;
        let session_id = parse_conversation_session_id(conversation_id)?;
        let read_model = self.conversation_read_model().await?;
        let existing_empty_summary = read_model
            .summary(tenant_id, session_id)
            .await
            .map_err(HarnessError::Journal)?
            .filter(|summary| summary.is_empty);
        self.catch_up_conversation_projection(tenant_id, session_id)
            .await?;
        let snapshot = read_model
            .snapshot(tenant_id, session_id, message_limit)
            .await
            .map_err(HarnessError::Journal)?;
        if snapshot.is_some() {
            return Ok(snapshot);
        }
        if let Some(existing_empty_summary) = existing_empty_summary {
            read_model
                .seed_empty_conversation(
                    tenant_id,
                    session_id,
                    existing_empty_summary.updated_at,
                    existing_empty_summary.model_config_id.as_deref(),
                )
                .await
                .map_err(HarnessError::Journal)?;
            return read_model
                .snapshot(tenant_id, session_id, message_limit)
                .await
                .map_err(HarnessError::Journal);
        }
        let Some(summary) = self
            .conversation_session_summary(tenant_id, session_id)
            .await?
        else {
            return Ok(None);
        };
        read_model
            .seed_empty_summary(tenant_id, &summary, None)
            .await
            .map_err(HarnessError::Journal)?;
        read_model
            .snapshot(tenant_id, session_id, message_limit)
            .await
            .map_err(HarnessError::Journal)
    }

    #[cfg(feature = "sqlite-store")]
    pub async fn page_conversation_timeline(
        &self,
        conversation_id: &str,
        after_cursor: Option<ConversationCursor>,
        limit: usize,
    ) -> Result<ConversationTimelinePage, HarnessError> {
        let tenant_id = self.inner.options.tenant_policy.id;
        let session_id = parse_conversation_session_id(conversation_id)?;
        self.catch_up_conversation_projection(tenant_id, session_id)
            .await?;
        self.conversation_read_model()
            .await?
            .page_timeline(tenant_id, session_id, after_cursor, limit)
            .await
            .map_err(HarnessError::Journal)
    }

    #[cfg(feature = "sqlite-store")]
    pub async fn page_conversation_worktree(
        &self,
        conversation_id: &str,
        page_cursor: Option<ConversationTurnCursor>,
        direction: ConversationTurnPageDirection,
        limit_turns: usize,
    ) -> Result<ConversationWorktreePage, HarnessError> {
        let tenant_id = self.inner.options.tenant_policy.id;
        let session_id = parse_conversation_session_id(conversation_id)?;
        self.catch_up_conversation_projection(tenant_id, session_id)
            .await?;
        self.conversation_read_model()
            .await?
            .page_worktree(tenant_id, session_id, page_cursor, direction, limit_turns)
            .await
            .map_err(HarnessError::Journal)
    }

    #[cfg(feature = "sqlite-store")]
    pub async fn catch_up_conversation_projection(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<(), HarnessError> {
        let read_model = self.conversation_read_model().await?;
        let mut after_event_id = read_model
            .projection_cursor(tenant_id, session_id)
            .await
            .map_err(HarnessError::Journal)?
            .map(|cursor| cursor.event_id);
        let mut reset_stale_projection = false;
        loop {
            let page = match self
                .inner
                .event_store
                .page_session_envelopes(tenant_id, session_id, after_event_id, 200)
                .await
            {
                Ok(page) => page,
                Err(error)
                    if after_event_id.is_some()
                        && !reset_stale_projection
                        && error.to_string().contains("conversation cursor is unknown") =>
                {
                    read_model
                        .reset_session(tenant_id, session_id)
                        .await
                        .map_err(HarnessError::Journal)?;
                    after_event_id = None;
                    reset_stale_projection = true;
                    continue;
                }
                Err(error) => return Err(HarnessError::Journal(error)),
            };
            if page.envelopes.is_empty() {
                return Ok(());
            }
            read_model
                .apply_envelopes(tenant_id, session_id, &page.envelopes, None)
                .await
                .map_err(HarnessError::Journal)?;
            after_event_id = page.next_event_id;
        }
    }

    #[cfg(feature = "sqlite-store")]
    pub async fn conversation_session_exists(
        &self,
        options: SessionOptions,
    ) -> Result<bool, HarnessError> {
        let options = self.effective_sdk_session_options(options)?;
        if self
            .inner
            .deleted_conversation_sessions
            .lock()
            .contains(&(options.tenant_id, options.session_id))
        {
            return Ok(false);
        }
        if self
            .conversation_read_model()
            .await?
            .snapshot(options.tenant_id, options.session_id, 1)
            .await
            .map_err(HarnessError::Journal)?
            .is_some()
        {
            return Ok(true);
        }
        Ok(self
            .conversation_session_summary(options.tenant_id, options.session_id)
            .await?
            .is_some())
    }

    #[cfg(feature = "sqlite-store")]
    async fn conversation_session_summary(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<Option<harness_journal::SessionSummary>, HarnessError> {
        let summaries = self
            .inner
            .event_store
            .list_sessions(
                tenant_id,
                SessionFilter {
                    since: None,
                    end_reason: None,
                    project_compression_tips: false,
                    limit: 200,
                },
            )
            .await
            .map_err(HarnessError::Journal)?;
        Ok(summaries
            .into_iter()
            .find(|summary| summary.session_id == session_id))
    }

    pub async fn delete_conversation_session(
        &self,
        options: SessionOptions,
    ) -> Result<bool, HarnessError> {
        let options = self.effective_sdk_session_options(options)?;
        #[cfg_attr(not(feature = "sqlite-store"), allow(unused_mut))]
        let mut deleted = self
            .inner
            .event_store
            .delete_session(options.tenant_id, options.session_id)
            .await
            .map_err(HarnessError::Journal)?;
        #[cfg(feature = "sqlite-store")]
        {
            let read_model = self.conversation_read_model().await?;
            if deleted {
                read_model
                    .reset_session(options.tenant_id, options.session_id)
                    .await
                    .map_err(HarnessError::Journal)?;
            } else if read_model
                .summary(options.tenant_id, options.session_id)
                .await
                .map_err(HarnessError::Journal)?
                .is_some()
            {
                read_model
                    .reset_session(options.tenant_id, options.session_id)
                    .await
                    .map_err(HarnessError::Journal)?;
                deleted = true;
            }
        }
        if !deleted {
            return Ok(false);
        }
        self.inner
            .deleted_conversation_sessions
            .lock()
            .insert((options.tenant_id, options.session_id));
        self.cancel_conversation_session_runs(options.tenant_id, options.session_id);
        Ok(true)
    }

    pub async fn submit_conversation_turn(
        &self,
        request: ConversationTurnRequest,
    ) -> Result<ConversationTurnReceipt, HarnessError> {
        if request.input.prompt.trim().is_empty() {
            return Err(HarnessError::Session(SessionError::Message(
                "prompt must not be empty".to_owned(),
            )));
        }

        let options = self.effective_sdk_session_options(request.options)?;
        self.ensure_conversation_session_not_deleted(options.tenant_id, options.session_id)?;
        let state = self
            .read_sdk_session_state(&options)
            .await?
            .ok_or_else(|| sdk_session_not_found(options.session_id))?;
        let projection = state.projection;
        if projection.end_reason.is_some() {
            return Err(HarnessError::Session(SessionError::Message(
                "cannot submit turn to ended session".to_owned(),
            )));
        }
        let last_offset = projection.last_offset;
        let model_id = options
            .model_id
            .clone()
            .unwrap_or_else(|| self.inner.options.model_id.clone());
        let model_snapshot = snapshot_for_supported_model(self.inner.model.as_ref(), &model_id)?;
        let parts = conversation_turn_parts(
            &request.input,
            &model_snapshot.conversation_capability.input_modalities,
        );
        let session = self
            .resume_sdk_session_from_projection(options.clone(), projection)
            .await?;
        session
            .run_turn_parts_with_client_message_id_attachments_and_permission_mode(
                parts,
                request.input.client_message_id.clone(),
                request.input.attachments.clone(),
                request.permission_mode_override,
            )
            .await?;
        let new_events = self
            .inner
            .event_store
            .read_envelopes(
                options.tenant_id,
                options.session_id,
                ReplayCursor::FromOffset(last_offset),
            )
            .await
            .map_err(HarnessError::Journal)?
            .collect::<Vec<_>>()
            .await;
        let run_id = new_events
            .iter()
            .find_map(|envelope| match &envelope.payload {
                Event::RunStarted(started) => Some(started.run_id),
                _ => None,
            })
            .ok_or_else(|| {
                HarnessError::Session(SessionError::Message(
                    "run did not emit RunStarted".to_owned(),
                ))
            })?;
        let projection = session.projection().await;
        Ok(ConversationTurnReceipt {
            tenant_id: options.tenant_id,
            session_id: options.session_id,
            run_id,
            message_count: projection.messages.len(),
        })
    }

    pub async fn page_conversation_events(
        &self,
        request: ConversationEventsPageRequest,
    ) -> Result<ConversationEventsPage, HarnessError> {
        let options = self.effective_sdk_session_options(request.options)?;
        let limit = request.limit.clamp(1, 200);
        let page = self
            .inner
            .event_store
            .page_session_envelopes(
                options.tenant_id,
                options.session_id,
                request.after_event_id,
                limit,
            )
            .await
            .map_err(HarnessError::Journal)?;
        let mut envelopes = page.envelopes;
        if request.after_event_id.is_none() {
            self.enforce_sdk_session_options_hash(&options, &envelopes)?;
        } else {
            let header = self
                .inner
                .event_store
                .page_session_envelopes(options.tenant_id, options.session_id, None, 1)
                .await
                .map_err(HarnessError::Journal)?;
            self.enforce_sdk_session_options_hash(&options, &header.envelopes)?;
        }
        let redactor = self.hook_redactor();
        for envelope in &mut envelopes {
            envelope.payload =
                redact_business_event_for_display(envelope.payload.clone(), redactor.as_ref());
        }
        Ok(ConversationEventsPage {
            events: envelopes,
            next_event_id: page.next_event_id,
        })
    }

    pub async fn cancel_conversation_run(&self, run_id: RunId) -> Result<(), HarnessError> {
        let active_run = self
            .inner
            .active_conversation_runs
            .lock()
            .get(&run_id)
            .cloned();
        let Some(active_run) = active_run else {
            return Err(HarnessError::Session(SessionError::Message(format!(
                "run is not active or cannot be cancelled through this facade: {run_id}"
            ))));
        };

        active_run.cancellation.cancel(InterruptCause::User);
        Ok(())
    }

    fn cancel_conversation_session_runs(&self, tenant_id: TenantId, session_id: SessionId) {
        let active_runs: Vec<_> = self
            .inner
            .active_conversation_runs
            .lock()
            .values()
            .filter(|active_run| {
                active_run.tenant_id == tenant_id && active_run.session_id == session_id
            })
            .cloned()
            .collect();

        for active_run in active_runs {
            active_run.cancellation.cancel(InterruptCause::System {
                reason: "conversation deleted".to_owned(),
            });
        }
    }

    fn ensure_conversation_session_not_deleted(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<(), HarnessError> {
        if self
            .inner
            .deleted_conversation_sessions
            .lock()
            .contains(&(tenant_id, session_id))
        {
            return Err(sdk_session_not_found(session_id));
        }

        Ok(())
    }

    fn conversation_deletion_guarded_event_store(&self) -> Arc<dyn EventStore> {
        Arc::new(ConversationDeletionGuardEventStore {
            inner: Arc::clone(&self.inner.event_store),
            deleted_conversation_sessions: Arc::clone(&self.inner.deleted_conversation_sessions),
        })
    }

    fn effective_sdk_session_options(
        &self,
        options: SessionOptions,
    ) -> Result<SessionOptions, HarnessError> {
        let mut options = self.effective_session_options(options)?;
        if !self.inner.options.tool_search_enabled {
            options.tool_search = ToolSearchMode::Disabled;
        }
        self.enforce_tenant(&options)?;
        Ok(options)
    }

    async fn read_sdk_session_state(
        &self,
        options: &SessionOptions,
    ) -> Result<Option<SdkSessionState>, HarnessError> {
        let envelopes = self
            .inner
            .event_store
            .read_envelopes(
                options.tenant_id,
                options.session_id,
                ReplayCursor::FromStart,
            )
            .await
            .map_err(HarnessError::Journal)?
            .collect::<Vec<_>>()
            .await;
        if envelopes.is_empty() {
            return Ok(None);
        }
        self.enforce_sdk_session_options_hash(options, &envelopes)?;
        let projection =
            SessionProjection::replay(envelopes.clone()).map_err(HarnessError::Session)?;
        Ok(Some(SdkSessionState { projection }))
    }

    async fn is_conversation_session_stream(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<bool, HarnessError> {
        let envelopes = self
            .inner
            .event_store
            .read_envelopes(tenant_id, session_id, ReplayCursor::FromStart)
            .await
            .map_err(HarnessError::Journal)?
            .take(1)
            .collect::<Vec<_>>()
            .await;
        let Some(envelope) = envelopes.first() else {
            return Ok(false);
        };
        let Event::SessionCreated(created) = &envelope.payload else {
            return Ok(false);
        };
        Ok(created.tenant_id == tenant_id && created.session_id == session_id)
    }

    #[cfg(feature = "sqlite-store")]
    async fn is_conversation_session_stream_page(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
    ) -> Result<bool, HarnessError> {
        let page = self
            .inner
            .event_store
            .page_session_envelopes(tenant_id, session_id, None, 1)
            .await
            .map_err(HarnessError::Journal)?;
        let Some(envelope) = page.envelopes.first() else {
            return Ok(false);
        };
        let Event::SessionCreated(created) = &envelope.payload else {
            return Ok(false);
        };
        Ok(created.tenant_id == tenant_id && created.session_id == session_id)
    }

    fn enforce_sdk_session_options_hash(
        &self,
        options: &SessionOptions,
        envelopes: &[EventEnvelope],
    ) -> Result<(), HarnessError> {
        let Some(Event::SessionCreated(created)) =
            envelopes.first().map(|envelope| &envelope.payload)
        else {
            return Err(HarnessError::Session(SessionError::Message(
                "session event stream does not start with SessionCreated".to_owned(),
            )));
        };
        let mut canonical = options.clone();
        canonical.workspace_root = canonical.workspace_root.canonicalize().map_err(|error| {
            HarnessError::Session(SessionError::Message(format!(
                "workspace_root invalid: {error}"
            )))
        })?;
        let expected = session_options_hash(&canonical);
        let matches_expected = created.options_hash == expected
            || self.session_options_hash_matches_model_runtime_variant(
                &canonical,
                created.options_hash,
            );
        if !matches_expected {
            return Err(HarnessError::PermissionDenied(
                "conversation session options do not match the existing session".to_owned(),
            ));
        }
        let session_created_options_hash = created.options_hash;
        for envelope in envelopes.iter().skip(1) {
            let Event::SessionCreated(created) = &envelope.payload else {
                continue;
            };
            if created.tenant_id != options.tenant_id
                || created.session_id != options.session_id
                || created.options_hash != session_created_options_hash
            {
                return Err(HarnessError::PermissionDenied(
                    "conversation session stream contains a mismatched SessionCreated event"
                        .to_owned(),
                ));
            }
        }
        Ok(())
    }

    fn session_options_hash_matches_model_runtime_variant(
        &self,
        options: &SessionOptions,
        actual: [u8; 32],
    ) -> bool {
        let mut model_ids = vec![None, options.model_id.clone()];
        let mut protocols = vec![
            None,
            options.protocol,
            Some(ModelProtocol::ChatCompletions),
            Some(ModelProtocol::Responses),
            Some(ModelProtocol::Messages),
            Some(ModelProtocol::GenerateContent),
        ];
        let mut permission_modes = vec![
            options.permission_mode,
            PermissionMode::Default,
            PermissionMode::Auto,
            PermissionMode::BypassPermissions,
            PermissionMode::DontAsk,
            PermissionMode::AcceptEdits,
            PermissionMode::Plan,
        ];

        for descriptor in self.inner.model.supported_models() {
            model_ids.push(Some(descriptor.model_id));
            protocols.push(Some(descriptor.protocol));
        }
        for entry in provider_catalog_entries() {
            for descriptor in entry.models {
                model_ids.push(Some(descriptor.model_id));
                protocols.push(Some(descriptor.protocol));
            }
        }
        model_ids.sort();
        model_ids.dedup();
        let mut deduped_protocols = Vec::new();
        for protocol in protocols {
            if !deduped_protocols.contains(&protocol) {
                deduped_protocols.push(protocol);
            }
        }
        permission_modes.sort_by_key(|mode| format!("{mode:?}"));
        permission_modes.dedup();

        for model_id in model_ids {
            for protocol in &deduped_protocols {
                for permission_mode in &permission_modes {
                    let mut variant = options.clone();
                    variant.model_id = model_id.clone();
                    variant.protocol = *protocol;
                    variant.permission_mode = *permission_mode;
                    if session_options_hash(&variant) == actual
                        || legacy_session_options_hash_with_permission_mode(&variant) == actual
                    {
                        return true;
                    }
                }
            }
        }

        false
    }

    async fn resume_sdk_session_from_projection(
        &self,
        options: SessionOptions,
        projection: SessionProjection,
    ) -> Result<Session, HarnessError> {
        let limit_permit = self.inner.session_limits.try_acquire()?;
        #[cfg(feature = "memory-external-slot")]
        let memory_manager = self.memory_manager_for_session(&options).await?;
        #[cfg(feature = "memory-external-slot")]
        let engine = self
            .engine_for_session(&options, memory_manager.clone(), None)
            .await?;
        #[cfg(not(feature = "memory-external-slot"))]
        let engine = self.engine_for_session(&options, None).await?;
        let event_store: Arc<dyn EventStore> = Arc::new(LifecycleHookEventStore {
            inner: Arc::clone(&self.inner.event_store),
            hooks: HookDispatcher::new(self.inner.hook_registry.snapshot()),
            tenant_id: options.tenant_id,
            session_id: options.session_id,
            #[cfg(feature = "memory-external-slot")]
            user_id: options.user_id.clone(),
            #[cfg(feature = "memory-external-slot")]
            team_id: options.team_id,
            workspace_root: options.workspace_root.clone(),
            redactor: self.hook_redactor(),
            session_limits: Arc::clone(&self.inner.session_limits),
            deleted_conversation_sessions: Arc::clone(&self.inner.deleted_conversation_sessions),
            summary_state: parking_lot::Mutex::new(MemorySessionSummaryState::default()),
            #[cfg(feature = "memory-external-slot")]
            memory_manager,
        });
        let session = Session::builder()
            .with_options(options)
            .with_event_store(event_store)
            .with_turn_runner(Arc::new(EngineSessionTurnRunner {
                engine,
                active_conversation_runs: Arc::clone(&self.inner.active_conversation_runs),
                skill_registry: Some(self.inner.skill_registry.clone()),
                skill_metrics_sink: self.skill_metrics_sink(),
                skill_config_snapshot: self.inner.skill_config_snapshot.clone(),
            }))
            .with_skill_reload_cap(Arc::new(SdkSkillReloadCap {
                inner: Arc::clone(&self.inner),
            }))
            .with_projection(projection)
            .build()
            .await
            .map_err(HarnessError::from)?;
        limit_permit.disarm();
        Ok(session)
    }

    fn effective_session_options(
        &self,
        explicit: SessionOptions,
    ) -> Result<SessionOptions, HarnessError> {
        let mut options = self.inner.options.default_session_options.clone();
        options.session_id = explicit.session_id;

        if let Some(workspace_id) = explicit.workspace_ref {
            let workspace = self
                .inner
                .workspace_registry
                .get(workspace_id)
                .ok_or_else(|| {
                    HarnessError::Other(format!("workspace not found: {workspace_id}"))
                })?;
            if explicit.tenant_id != TenantId::SINGLE && explicit.tenant_id != workspace.tenant_id {
                return Err(HarnessError::TenantMismatch);
            }
            options.workspace_ref = Some(workspace.id);
            options.tenant_id = workspace.tenant_id;
            options.workspace_root = workspace.root_path.clone();
            options.workspace_bootstrap = Some(workspace.bootstrap());
            if let Some(defaults) = &workspace.default_session_options {
                apply_non_default_session_options(&mut options, defaults);
            }
        }

        apply_explicit_session_options(&mut options, &explicit);
        self.load_workspace_bootstrap(&mut options)?;
        Ok(options)
    }

    fn load_workspace_bootstrap(&self, options: &mut SessionOptions) -> Result<(), HarnessError> {
        let Some(bootstrap) = options.workspace_bootstrap.clone() else {
            return Ok(());
        };
        let mut sections = Vec::new();
        for file in &bootstrap.files {
            let path = resolve_bootstrap_path(&bootstrap.workspace_root, &file.relative_path)?;
            match std::fs::read_to_string(&path) {
                Ok(content) if !content.trim().is_empty() => sections.push(content),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && !file.required => {}
                Err(error) => {
                    return Err(HarnessError::Other(format!(
                        "load workspace bootstrap `{}` failed: {error}",
                        path.display()
                    )));
                }
            }
        }
        if let Some(addendum) = bootstrap.system_prompt_addendum {
            if !addendum.trim().is_empty() {
                sections.push(addendum);
            }
        }
        if sections.is_empty() {
            return Ok(());
        }
        let bootstrap_prompt = sections.join("\n\n");
        options.system_prompt_addendum = Some(match options.system_prompt_addendum.take() {
            Some(existing) if !existing.trim().is_empty() => {
                format!("{bootstrap_prompt}\n\n{existing}")
            }
            _ => bootstrap_prompt,
        });
        Ok(())
    }

    #[cfg(feature = "memory-external-slot")]
    async fn engine_for_session(
        &self,
        options: &SessionOptions,
        memory_manager: Option<Arc<harness_memory::MemoryManager>>,
        pending_session_events: Option<Arc<PendingSessionEvents>>,
    ) -> Result<Engine, HarnessError> {
        self.activate_plugins(options).await?;
        let context = self.context_engine(options, memory_manager).await?;
        self.engine_for_session_with_context(options, context, pending_session_events)
            .await
    }

    #[cfg(not(feature = "memory-external-slot"))]
    async fn engine_for_session(
        &self,
        options: &SessionOptions,
        pending_session_events: Option<Arc<PendingSessionEvents>>,
    ) -> Result<Engine, HarnessError> {
        self.activate_plugins(options).await?;
        let context = self.context_engine(options).await?;
        self.engine_for_session_with_context(options, context, pending_session_events)
            .await
    }

    async fn engine_for_session_with_context(
        &self,
        options: &SessionOptions,
        context: ContextEngine,
        pending_session_events: Option<Arc<PendingSessionEvents>>,
    ) -> Result<Engine, HarnessError> {
        let mut cap_registry = (*self.inner.cap_registry).clone();
        if let Some(blob_store) = &self.inner.blob_store {
            cap_registry.install::<dyn harness_contracts::BlobReaderCap>(
                ToolCapability::BlobReader,
                Arc::new(BlobReaderCapAdapter::new(Arc::clone(blob_store))),
            );
            cap_registry.install::<dyn harness_contracts::BlobWriterCap>(
                ToolCapability::BlobWriter,
                Arc::new(BlobWriterCapAdapter::new(Arc::clone(blob_store))),
            );
            cap_registry.install::<dyn harness_contracts::OffloadedBlobAuthorizerCap>(
                ToolCapability::OffloadedBlobAuthorizer,
                Arc::new(EventStoreOffloadedBlobAuthorizer::new(Arc::clone(
                    &self.inner.event_store,
                ))),
            );
        }
        if let Some(skill_registry) = self
            .skill_registry_service(options, pending_session_events)
            .await?
        {
            cap_registry.install::<dyn harness_contracts::SkillRegistryCap>(
                ToolCapability::SkillRegistry,
                Arc::new(skill_registry),
            );
        }
        self.inject_mcp_tools().await?;
        let model_id = options
            .model_id
            .clone()
            .unwrap_or_else(|| self.inner.options.model_id.clone());
        let model_snapshot = snapshot_for_supported_model(self.inner.model.as_ref(), &model_id)?;
        let protocol = options.protocol.unwrap_or(model_snapshot.protocol);
        self.enforce_provider_allowed(&model_snapshot.provider_id)?;
        let context = context.clone_with_budget(context_budget_for_model(
            &model_snapshot,
            options.context_compression_trigger_ratio,
        ));
        if !cap_registry.contains(&ToolCapability::ContextPatchSink) {
            cap_registry.install::<dyn ContextPatchSinkCap>(
                ToolCapability::ContextPatchSink,
                Arc::new(context.clone()),
            );
        }
        let model_profile = ToolPoolModelProfile {
            provider: harness_contracts::ModelProvider(model_snapshot.provider_id.clone()),
            max_context_tokens: (model_snapshot.context_window > 0)
                .then_some(model_snapshot.context_window),
        };
        let tool_registry_snapshot = self.inner.tool_registry.snapshot();
        let mut tool_filter = filter_unavailable_tools(&tool_registry_snapshot, &cap_registry);
        filter_unrouted_service_tools(
            &mut tool_filter,
            &tool_registry_snapshot,
            &*self.inner.provider_capability_routes.read(),
        );
        apply_tenant_tool_filter(&mut tool_filter, &self.inner.options.tenant_policy);
        let schema_context = SchemaResolverContext {
            run_id: RunId::new(),
            session_id: options.session_id,
            tenant_id: options.tenant_id,
        };
        let tools = ToolPool::assemble(
            &tool_registry_snapshot,
            &tool_filter,
            &options.tool_search,
            &model_profile,
            &schema_context,
        )
        .await
        .map_err(HarnessError::Tool)?;
        #[cfg(feature = "tool-search")]
        let mut tools = tools;
        #[cfg(feature = "tool-search")]
        self.install_tool_search_runtime(options, &mut tools, &mut cap_registry, &model_snapshot);

        let mut builder = Engine::builder()
            .with_event_store(self.conversation_deletion_guarded_event_store())
            .with_context(context)
            .with_hooks(HookDispatcher::new(self.inner.hook_registry.snapshot()))
            .with_model(Arc::clone(&self.inner.model))
            .with_tools(tools)
            .with_permission_broker(Arc::clone(&self.inner.permission_broker))
            .with_workspace_root(&options.workspace_root)
            .with_model_id(model_id)
            .with_model_snapshot(model_snapshot)
            .with_model_extra(options.model_extra.clone())
            .with_protocol(protocol)
            .with_system_prompt(self.session_system_prompt(options).await?)
            .with_sandbox(Arc::clone(&self.inner.sandbox))
            .with_cap_registry(Arc::new(cap_registry));
        if options.max_iterations > 0 {
            builder = builder.with_max_iterations(options.max_iterations);
        }
        #[cfg(feature = "agents-subagent")]
        if self
            .inner
            .cap_registry
            .contains(&ToolCapability::SubagentRunner)
        {
            builder = builder.with_subagent_tool();
        }
        if let Some(blob_store) = &self.inner.blob_store {
            builder = builder.with_blob_store(Arc::clone(blob_store));
        }
        if let Some(tracer) = &self.inner.tracer {
            builder = builder.with_tracer(Arc::clone(tracer));
        }
        if let Some(observer) = &self.inner.observer {
            builder = builder.with_observer(Arc::clone(observer));
        }
        builder = builder.with_model_middlewares(self.inner.model_middlewares.clone());
        builder.build().map_err(Into::into)
    }

    async fn skill_registry_service(
        &self,
        options: &SessionOptions,
        pending_session_events: Option<Arc<PendingSessionEvents>>,
    ) -> Result<Option<SkillRegistryService>, HarnessError> {
        let registry = self.inner.skill_registry.clone();
        let metrics_sink = self.skill_metrics_sink();
        if let Some(loader) = &self.inner.skill_loader {
            let event_sink: Arc<dyn harness_skill::SkillEventSink> =
                if let Some(pending_session_events) = pending_session_events {
                    Arc::new(BufferedSkillEventSink {
                        pending_session_events,
                    })
                } else {
                    Arc::new(SdkSkillEventSink {
                        event_store: Arc::clone(&self.inner.event_store),
                        tenant_id: options.tenant_id,
                        session_id: options.session_id,
                    })
                };
            let mut loader = loader.clone().with_event_sink(event_sink).with_event_scope(
                SkillThreatEventScope {
                    session_id: Some(options.session_id),
                    run_id: None,
                },
            );
            if let Some(metrics_sink) = &metrics_sink {
                loader = loader.with_metrics_sink(Arc::clone(metrics_sink));
            }
            let report = loader
                .load_all()
                .await
                .map_err(|error| HarnessError::Other(format!("load skills failed: {error}")))?;
            let snapshot = registry.snapshot();
            let new_skills = report
                .loaded
                .into_iter()
                .filter(|skill| {
                    !snapshot
                        .entries
                        .get(&skill.name)
                        .is_some_and(|existing| existing.source == skill.source)
                })
                .collect::<Vec<_>>();
            if !new_skills.is_empty() {
                registry.register_batch(new_skills).map_err(|error| {
                    HarnessError::Other(format!("register skill failed: {error}"))
                })?;
            }
        }
        self.register_skill_hooks(&registry)?;
        let snapshot = registry.snapshot();
        validate_required_skill_config(&snapshot, &self.inner.skill_config_snapshot)
            .map_err(|error| HarnessError::Other(error.to_string()))?;
        let mut renderer = SkillRenderer::new(Arc::new(
            SkillConfigSnapshotResolver::from_registry_snapshot(
                &snapshot,
                self.inner.skill_config_snapshot.clone(),
            ),
        ));
        if let Some(metrics_sink) = &metrics_sink {
            renderer = renderer.with_metrics_sink(Arc::clone(metrics_sink));
        }
        let mut service = SkillRegistryService::new(registry, renderer);
        if let Some(metrics_sink) = metrics_sink {
            service = service.with_metrics_sink(metrics_sink);
        }
        Ok(Some(service))
    }

    fn skill_metrics_sink(&self) -> Option<Arc<dyn SkillMetricsSink>> {
        self.inner.observer.as_ref().map(|observer| {
            Arc::new(SdkSkillMetricsSink {
                observer: Arc::clone(observer),
            }) as Arc<dyn SkillMetricsSink>
        })
    }

    fn model_metrics_sink(&self) -> Option<Arc<dyn ModelMetricsSink>> {
        self.inner.observer.as_ref().map(|observer| {
            Arc::new(SdkModelMetricsSink {
                observer: Arc::clone(observer),
            }) as Arc<dyn ModelMetricsSink>
        })
    }

    #[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
    fn memory_metrics_sink(&self) -> Option<Arc<dyn harness_memory::MemoryMetricsSink>> {
        self.inner.observer.as_ref().map(|observer| {
            Arc::new(SdkMemoryMetricsSink {
                observer: Arc::clone(observer),
            }) as Arc<dyn harness_memory::MemoryMetricsSink>
        })
    }

    fn register_skill_hooks(&self, registry: &SkillRegistry) -> Result<(), HarnessError> {
        for binding in registry.hook_bindings() {
            if self
                .inner
                .hook_registry
                .origin_for(&binding.handler_id)
                .is_some()
            {
                continue;
            }
            self.inner
                .hook_registry
                .register(skill_hook_handler(binding)?)
                .map_err(|error| {
                    HarnessError::Hook(harness_contracts::HookError::Message(error.to_string()))
                })?;
        }
        Ok(())
    }

    async fn activate_plugins(&self, options: &SessionOptions) -> Result<(), HarnessError> {
        let Some(registry) = &self.inner.plugin_registry else {
            return Ok(());
        };
        let discovered = match registry.discover().await {
            Ok(discovered) => discovered,
            Err(error) => {
                self.emit_plugin_discovery_error(options, &error).await?;
                return Err(HarnessError::Other(error.to_string()));
            }
        };
        for plugin in discovered {
            let plugin_id = plugin.record.manifest.plugin_id();
            if matches!(
                registry.state(&plugin_id),
                Some(harness_plugin::PluginLifecycleState::Activated)
            ) {
                continue;
            }
            let from_state = registry
                .state(&plugin_id)
                .map(plugin_state_discriminant)
                .unwrap_or(PluginLifecycleStateDiscriminant::Validated);
            match registry.activate(&plugin_id).await {
                Ok(()) => {
                    self.emit_plugin_loaded(options, &plugin.record, from_state)
                        .await?;
                }
                Err(error) => {
                    self.emit_plugin_rejected(options, &plugin.record, &error)
                        .await?;
                    return Err(HarnessError::Other(error.to_string()));
                }
            }
        }
        Ok(())
    }

    async fn emit_plugin_loaded(
        &self,
        options: &SessionOptions,
        record: &ManifestRecord,
        from_state: PluginLifecycleStateDiscriminant,
    ) -> Result<(), HarnessError> {
        let manifest = &record.manifest;
        self.inner
            .event_store
            .append(
                options.tenant_id,
                options.session_id,
                &[Event::PluginLoaded(PluginLoadedEvent {
                    tenant_id: options.tenant_id,
                    plugin_id: manifest.plugin_id(),
                    plugin_name: manifest.name.to_string(),
                    plugin_version: manifest.version.to_string(),
                    trust_level: manifest.trust_level,
                    capabilities: plugin_capabilities_summary(manifest),
                    manifest_origin: manifest_origin_ref(&record.origin),
                    manifest_hash: record.manifest_hash,
                    from_state,
                    at: harness_contracts::now(),
                })],
            )
            .await
            .map_err(HarnessError::Journal)?;
        Ok(())
    }

    async fn emit_plugin_rejected(
        &self,
        options: &SessionOptions,
        record: &ManifestRecord,
        error: &PluginError,
    ) -> Result<(), HarnessError> {
        let manifest = &record.manifest;
        self.inner
            .event_store
            .append(
                options.tenant_id,
                options.session_id,
                &[Event::PluginRejected(PluginRejectedEvent {
                    tenant_id: options.tenant_id,
                    plugin_id: manifest.plugin_id(),
                    plugin_name: manifest.name.to_string(),
                    plugin_version: manifest.version.to_string(),
                    trust_level: manifest.trust_level,
                    manifest_origin: manifest_origin_ref(&record.origin),
                    manifest_hash: record.manifest_hash,
                    reason: rejection_reason(error),
                    at: harness_contracts::now(),
                })],
            )
            .await
            .map_err(HarnessError::Journal)?;
        Ok(())
    }

    async fn emit_plugin_discovery_error(
        &self,
        options: &SessionOptions,
        error: &PluginError,
    ) -> Result<(), HarnessError> {
        if let PluginError::ManifestLoader(ManifestLoaderError::Validation(failure)) = error {
            self.inner
                .event_store
                .append(
                    options.tenant_id,
                    options.session_id,
                    &[Event::ManifestValidationFailed(
                        ManifestValidationFailedEvent {
                            tenant_id: options.tenant_id,
                            manifest_origin: failure
                                .origin
                                .as_ref()
                                .map(manifest_origin_ref)
                                .unwrap_or_else(|| ManifestOriginRef::File {
                                    path: "<unknown>".to_owned(),
                                }),
                            partial_name: failure.partial_name.clone(),
                            partial_version: failure.partial_version.clone(),
                            raw_bytes_hash: failure.raw_bytes_hash,
                            failure: failure.failure.clone(),
                            at: harness_contracts::now(),
                        },
                    )],
                )
                .await
                .map_err(HarnessError::Journal)?;
        }
        Ok(())
    }

    async fn inject_mcp_tools(&self) -> Result<(), HarnessError> {
        let Some(config) = &self.inner.mcp_config else {
            return Ok(());
        };
        for server_id in &config.server_ids_to_inject {
            config
                .registry
                .inject_tools_into(&self.inner.tool_registry, server_id)
                .await
                .map_err(|error| HarnessError::Other(error.to_string()))?;
        }
        Ok(())
    }

    #[cfg(feature = "tool-search")]
    fn install_tool_search_runtime(
        &self,
        options: &SessionOptions,
        tools: &mut ToolPool,
        cap_registry: &mut CapabilityRegistry,
        model_snapshot: &ModelRuntimeSnapshot,
    ) {
        if matches!(options.tool_search, ToolSearchMode::Disabled) {
            return;
        }
        if let Some(allowed_tools) = &self.inner.options.tenant_policy.allowed_tools {
            if !allowed_tools.contains("tool_search") {
                return;
            }
        }
        let runtime = SdkToolSearchRuntime {
            tools: tools.clone(),
            model_caps: Arc::new(model_snapshot.conversation_capability.clone()),
            mcp_config: self.inner.mcp_config.clone(),
            event_store: Arc::clone(&self.inner.event_store),
            hooks: HookDispatcher::new(self.inner.hook_registry.snapshot()),
            tenant_id: options.tenant_id,
            session_id: options.session_id,
            redactor: self.hook_redactor(),
        };
        let runtime: Arc<dyn harness_tool_search::ToolSearchRuntimeCap> = Arc::new(runtime);
        cap_registry.install::<dyn harness_tool_search::ToolSearchRuntimeCap>(
            ToolCapability::Custom(harness_tool_search::TOOL_SEARCH_RUNTIME_CAPABILITY.to_owned()),
            runtime,
        );
        tools.append_runtime_tool(Arc::new({
            let mut builder =
                harness_tool_search::ToolSearchTool::builder().with_coalesce_window(Duration::ZERO);
            if let Some(scorer) = &self.inner.tool_search_scorer {
                builder = builder.with_scorer(Arc::clone(scorer));
            }
            builder.build()
        }));
    }

    async fn context_engine(
        &self,
        _options: &SessionOptions,
        #[cfg(feature = "memory-external-slot")] memory_manager: Option<
            Arc<harness_memory::MemoryManager>,
        >,
    ) -> Result<ContextEngine, HarnessError> {
        let mut builder =
            ContextEngine::builder().with_default_compaction(self.inner.blob_store.clone());
        if let Some(aux_model) = &self.inner.aux_model {
            builder = builder.with_aux_provider(Arc::clone(aux_model));
        }
        if let Some(metrics_sink) = self.model_metrics_sink() {
            builder = builder.with_model_metrics_sink(metrics_sink);
        }
        #[cfg(feature = "memory-external-slot")]
        if let Some(memory_manager) = memory_manager {
            builder = builder.with_memory_manager(memory_manager);
        }
        builder.build().map_err(HarnessError::Context)
    }

    #[cfg(feature = "memory-external-slot")]
    async fn memory_manager_for_session(
        &self,
        options: &SessionOptions,
    ) -> Result<Option<Arc<harness_memory::MemoryManager>>, HarnessError> {
        let provider = self.effective_memory_provider();
        #[cfg(feature = "memory-consolidation")]
        let has_consolidation_hook = self.inner.consolidation_hook.is_some();
        #[cfg(not(feature = "memory-consolidation"))]
        let has_consolidation_hook = false;
        if provider.is_none() && !has_consolidation_hook {
            return Ok(None);
        }

        let mut manager = harness_memory::MemoryManager::new()
            .with_event_sink(Arc::new(SdkMemoryEventSink {
                event_store: Arc::clone(&self.inner.event_store),
                tenant_id: options.tenant_id,
                session_id: options.session_id,
            }))
            .with_threat_scanner(Arc::new(harness_memory::MemoryThreatScanner::default()));
        if let Some(metrics_sink) = self.memory_metrics_sink() {
            manager = manager.with_metrics_sink(metrics_sink);
        }
        #[cfg(feature = "memory-consolidation")]
        if let Some(hook) = &self.inner.consolidation_hook {
            manager = manager.with_consolidation_hook(Arc::clone(hook));
        }
        if let Some(provider) = provider {
            manager
                .set_external(provider)
                .map_err(HarnessError::Memory)?;
        }
        manager
            .initialize_session(&harness_contracts::MemorySessionCtx {
                tenant_id: options.tenant_id,
                session_id: options.session_id,
                workspace_id: None,
                user_id: options.user_id.as_deref(),
                team_id: options.team_id,
            })
            .await
            .map_err(HarnessError::Memory)?;
        Ok(Some(Arc::new(manager)))
    }

    #[cfg(feature = "memory-builtin")]
    async fn builtin_system_prompt(
        &self,
        options: &SessionOptions,
    ) -> Result<Option<String>, HarnessError> {
        let Some(config) = &self.inner.builtin_memory else {
            return Ok(None);
        };
        let mut memory = config.for_session(options);
        if let Some(metrics_sink) = self.memory_metrics_sink() {
            memory = memory.with_metrics_sink(metrics_sink);
        }
        let snapshot = memory.read_all().await.map_err(HarnessError::Memory)?;
        let rendered =
            render_builtin_memory_system_prompt(&snapshot, options.tenant_id, options.session_id);
        if !rendered.overflows.is_empty() {
            let events = rendered
                .overflows
                .iter()
                .cloned()
                .map(Event::MemdirOverflow)
                .collect::<Vec<_>>();
            let _ = self
                .inner
                .event_store
                .append(options.tenant_id, options.session_id, &events)
                .await;
            if let Some(metrics_sink) = self.memory_metrics_sink() {
                for overflow in &rendered.overflows {
                    metrics_sink.record(harness_memory::MemoryMetric::MemdirOverflow {
                        file: overflow.file,
                        current_chars: overflow.current_chars,
                        threshold: overflow.threshold,
                    });
                }
            }
        }
        Ok(rendered.prompt)
    }

    async fn session_system_prompt(
        &self,
        options: &SessionOptions,
    ) -> Result<Option<String>, HarnessError> {
        let base = append_system_prompt_addendum(
            Some(JYOWO_DEFAULT_SYSTEM_PROMPT.to_owned()),
            self.builtin_system_prompt(options).await?,
        );
        Ok(append_system_prompt_addendum(
            base,
            options.system_prompt_addendum.clone(),
        ))
    }

    #[cfg(not(feature = "memory-builtin"))]
    async fn builtin_system_prompt(
        &self,
        _options: &SessionOptions,
    ) -> Result<Option<String>, HarnessError> {
        Ok(None)
    }

    pub async fn create_workspace<R>(&self, request: R) -> Result<R::Output, HarnessError>
    where
        R: WorkspaceCreateRequest,
    {
        request.create_workspace(self)
    }

    pub async fn list_workspaces(&self, tenant: TenantId) -> Result<Vec<Workspace>, HarnessError> {
        if tenant != self.inner.options.tenant_policy.id
            && !self.inner.options.tenant_policy.allow_scoped_tenants
        {
            return Err(HarnessError::InvalidTenant(tenant));
        }
        Ok(self.inner.workspace_registry.list(tenant))
    }

    pub async fn get_workspace(
        &self,
        id: harness_contracts::WorkspaceId,
    ) -> Result<Option<Workspace>, HarnessError> {
        let workspace = self.inner.workspace_registry.get(id);
        if let Some(workspace) = &workspace {
            if workspace.tenant_id != self.inner.options.tenant_policy.id
                && !self.inner.options.tenant_policy.allow_scoped_tenants
            {
                return Err(HarnessError::InvalidTenant(workspace.tenant_id));
            }
        }
        Ok(workspace)
    }

    fn create_workspace_path(&self, root: &Path) -> Result<PathBuf, HarnessError> {
        std::fs::create_dir_all(root)
            .map_err(|error| HarnessError::Other(format!("create workspace failed: {error}")))?;
        for relative in GOVERNED_WORKSPACE_DIRS {
            std::fs::create_dir_all(root.join(relative)).map_err(|error| {
                HarnessError::Other(format!("create workspace path {relative} failed: {error}"))
            })?;
        }
        root.canonicalize()
            .map_err(|error| HarnessError::Other(format!("canonicalize workspace failed: {error}")))
    }

    fn create_workspace_record(&self, mut spec: WorkspaceSpec) -> Result<Workspace, HarnessError> {
        if spec.tenant_id != self.inner.options.tenant_policy.id
            && !self.inner.options.tenant_policy.allow_scoped_tenants
        {
            return Err(HarnessError::InvalidTenant(spec.tenant_id));
        }
        spec.root_path = self.create_workspace_path(&spec.root_path)?;
        Ok(self.inner.workspace_registry.create(spec))
    }

    pub async fn audit_query(
        &self,
        tenant: TenantId,
        query: AuditQuery,
        caller_trust: TrustLevel,
    ) -> Result<AuditPage, HarnessError> {
        if caller_trust != TrustLevel::AdminTrusted {
            return Err(HarnessError::PermissionDenied(
                "audit query requires admin-trusted caller".to_owned(),
            ));
        }

        EventStoreAudit::new(Arc::clone(&self.inner.event_store))
            .query(tenant, query)
            .await
            .map_err(HarnessError::Journal)
    }

    #[cfg(feature = "agents-team")]
    pub async fn create_team(
        &self,
        builder: harness_team::TeamBuilder,
    ) -> Result<crate::team::Team, HarnessError> {
        let spec = builder.build();
        spec.validate()
            .map_err(|error| HarnessError::Other(error.to_string()))?;
        if spec.topology == harness_team::Topology::Custom {
            return Err(HarnessError::Other(
                "custom team topology is not executable through the SDK facade".to_owned(),
            ));
        }
        let tenant_id = self.inner.options.tenant_policy.id;
        let journal_session_id = harness_contracts::SessionId::new();
        let journal = harness_team::TeamJournalContext {
            tenant_id,
            session_id: journal_session_id,
        };
        let event_store = Arc::clone(&self.inner.event_store);
        let blob_store: Arc<dyn BlobStore> = self.inner.blob_store.as_ref().map_or_else(
            || Arc::new(harness_journal::InMemoryBlobStore::default()) as Arc<dyn BlobStore>,
            Arc::clone,
        );
        self.emit_team_created(&spec, journal, Arc::clone(&blob_store))
            .await?;
        let bus = harness_team::MessageBus::journaled(
            spec.team_id,
            spec.message_bus.buffer_size,
            journal,
            Arc::clone(&event_store),
        );
        let runtime = harness_team::Team::new(spec.clone(), bus, journal, event_store, blob_store);
        let execution = self
            .team_execution_runtime(runtime.clone(), &spec, tenant_id, journal_session_id)
            .await?;
        Ok(crate::team::Team::from_runtime(
            runtime,
            execution,
            spec,
            tenant_id,
            journal_session_id,
        ))
    }

    #[cfg(feature = "agents-team")]
    async fn team_execution_runtime(
        &self,
        runtime: harness_team::Team,
        spec: &harness_team::TeamSpec,
        tenant_id: TenantId,
        journal_session_id: harness_contracts::SessionId,
    ) -> Result<crate::team::TeamExecutionRuntime, HarnessError> {
        match spec.topology {
            harness_team::Topology::CoordinatorWorker => {
                let mut execution = harness_team::CoordinatorWorkerRuntime::from_team(runtime);
                for member in &spec.members {
                    execution = execution.with_member_runner(
                        member.agent_id,
                        self.team_member_runner(
                            member,
                            spec.team_id,
                            tenant_id,
                            journal_session_id,
                        )
                        .await?,
                    );
                }
                Ok(crate::team::TeamExecutionRuntime::CoordinatorWorker(
                    Arc::new(execution),
                ))
            }
            harness_team::Topology::PeerToPeer => {
                let mut execution = harness_team::PeerToPeerRuntime::from_team(runtime);
                for member in &spec.members {
                    execution = execution.with_member_runner(
                        member.agent_id,
                        self.team_member_runner(
                            member,
                            spec.team_id,
                            tenant_id,
                            journal_session_id,
                        )
                        .await?,
                    );
                }
                Ok(crate::team::TeamExecutionRuntime::PeerToPeer(Arc::new(
                    execution,
                )))
            }
            harness_team::Topology::RoleRouted => {
                let mut execution = harness_team::RoleRoutedRuntime::from_team(runtime);
                for member in &spec.members {
                    execution = execution.with_member_runner(
                        member.agent_id,
                        self.team_member_runner(
                            member,
                            spec.team_id,
                            tenant_id,
                            journal_session_id,
                        )
                        .await?,
                    );
                }
                Ok(crate::team::TeamExecutionRuntime::RoleRouted(Arc::new(
                    execution,
                )))
            }
            harness_team::Topology::Custom => Err(HarnessError::Other(
                "custom team topology is not executable through the SDK facade".to_owned(),
            )),
        }
    }

    #[cfg(feature = "agents-team")]
    async fn team_member_runner(
        &self,
        member: &harness_team::TeamMember,
        team_id: harness_contracts::TeamId,
        tenant_id: TenantId,
        session_id: harness_contracts::SessionId,
    ) -> Result<Arc<dyn harness_team::TeamMemberRunner>, HarnessError> {
        let mut options = SessionOptions::new(self.inner.options.workspace_root.clone())
            .with_tenant_id(tenant_id)
            .with_session_id(session_id)
            .with_team_id(team_id)
            .with_permission_mode(member.engine_config.permission_mode)
            .with_interactivity(member.engine_config.interactivity)
            .with_max_iterations(member.engine_config.max_iterations);
        if let Some(model_ref) = &member.engine_config.model_ref {
            options = options.with_model_id(model_ref.model_id.clone());
        }
        let mut options = self.effective_session_options(options)?;
        if !self.inner.options.tool_search_enabled {
            options.tool_search = ToolSearchMode::Disabled;
        }
        self.enforce_tenant(&options)?;
        #[cfg(feature = "memory-external-slot")]
        let memory_manager = self.memory_manager_for_session(&options).await?;
        #[cfg(feature = "memory-external-slot")]
        let engine = self.engine_for_session(&options, memory_manager).await?;
        #[cfg(not(feature = "memory-external-slot"))]
        let engine = self.engine_for_session(&options).await?;
        Ok(Arc::new(crate::agents_team::EngineTeamMemberRunner::new(
            Arc::new(engine),
        )))
    }

    #[cfg(feature = "agents-team")]
    async fn emit_team_created(
        &self,
        spec: &harness_team::TeamSpec,
        journal: harness_team::TeamJournalContext,
        blob_store: Arc<dyn BlobStore>,
    ) -> Result<(), HarnessError> {
        let member_specs = serde_json::to_vec(&spec.members)
            .map_err(|error| HarnessError::Other(error.to_string()))?;
        let member_specs_hash = *blake3::hash(&member_specs).as_bytes();
        let mut events = vec![Event::TeamCreated(TeamCreatedEvent {
            team_id: spec.team_id,
            tenant_id: journal.tenant_id,
            name: spec.name.clone(),
            topology_kind: topology_kind(spec.topology),
            member_specs_hash,
            created_at: chrono::Utc::now(),
        })];

        for member in &spec.members {
            let session_id = harness_contracts::SessionId::new();
            Session::builder()
                .with_options(
                    SessionOptions::new(PathBuf::from("."))
                        .with_tenant_id(journal.tenant_id)
                        .with_session_id(session_id),
                )
                .with_event_store(Arc::clone(&self.inner.event_store))
                .build()
                .await
                .map_err(HarnessError::Session)?;

            let member_bytes = serde_json::to_vec(member)
                .map_err(|error| HarnessError::Other(error.to_string()))?;
            let member_size = member_bytes.len() as u64;
            let spec_hash = *blake3::hash(&member_bytes).as_bytes();
            let spec_snapshot_id = blob_store
                .put(
                    journal.tenant_id,
                    Bytes::from(member_bytes),
                    BlobMeta {
                        content_type: Some("application/json".to_owned()),
                        size: member_size,
                        content_hash: spec_hash,
                        created_at: chrono::Utc::now(),
                        retention: BlobRetention::SessionScoped(session_id),
                    },
                )
                .await
                .map_err(|error| HarnessError::Other(error.to_string()))?;
            events.push(Event::TeamMemberJoined(TeamMemberJoinedEvent {
                team_id: spec.team_id,
                agent_id: member.agent_id,
                role: member.role.clone(),
                session_id,
                visibility: member.visibility.clone(),
                spec_snapshot_id,
                spec_hash,
                joined_at: chrono::Utc::now(),
            }));
        }

        self.inner
            .event_store
            .append_with_metadata(
                journal.tenant_id,
                journal.session_id,
                AppendMetadata::default(),
                &events,
            )
            .await
            .map(|_| ())
            .map_err(HarnessError::Journal)
    }

    pub async fn resolve_permission(
        &self,
        request_id: harness_contracts::RequestId,
        decision: Decision,
    ) -> Result<(), HarnessError> {
        #[cfg(feature = "stream-permission")]
        {
            if let Some(resolver) = &self.inner.permission_resolver {
                return resolver
                    .resolve(request_id, decision)
                    .await
                    .map_err(HarnessError::Permission);
            }
        }

        let _ = (&request_id, &decision);
        Err(HarnessError::Other(
            "permission resolver is not configured".to_owned(),
        ))
    }

    pub async fn resolve_elicitation(
        &self,
        request_id: harness_contracts::RequestId,
        response: serde_json::Value,
    ) -> Result<(), HarnessError> {
        let Some(handler) = &self.inner.stream_elicitation_handler else {
            return Err(HarnessError::Other(
                "elicitation resolver is not configured".to_owned(),
            ));
        };

        handler
            .resolve_elicitation(request_id, response)
            .await
            .map_err(|error| HarnessError::Other(error.to_string()))
    }

    #[must_use]
    pub fn options(&self) -> &HarnessOptions {
        &self.inner.options
    }

    #[must_use]
    pub fn model_provider(&self) -> Arc<dyn ModelProvider> {
        Arc::clone(&self.inner.model)
    }

    #[must_use]
    pub fn mcp_sampling_provider(
        &self,
        tenant_id: TenantId,
        session_id: Option<harness_contracts::SessionId>,
        run_id: Option<RunId>,
    ) -> HarnessSamplingProvider {
        HarnessSamplingProvider::new(
            Arc::clone(&self.inner.model),
            self.inner.options.model_id.clone(),
            tenant_id,
            session_id,
            run_id,
        )
    }

    #[must_use]
    pub fn event_store(&self) -> Arc<dyn EventStore> {
        Arc::clone(&self.inner.event_store)
    }

    pub async fn event_stream(
        &self,
        tenant_id: TenantId,
        session_id: harness_contracts::SessionId,
        cursor: ReplayCursor,
    ) -> Result<harness_journal::EventStream, HarnessError> {
        let redactor = self.hook_redactor();
        let stream = self
            .inner
            .event_store
            .read(tenant_id, session_id, cursor)
            .await
            .map_err(HarnessError::Journal)?
            .map(move |event| redact_business_event_for_display(event, redactor.as_ref()));
        Ok(Box::pin(stream))
    }

    #[must_use]
    pub fn sandbox(&self) -> Arc<dyn SandboxBackend> {
        Arc::clone(&self.inner.sandbox)
    }

    #[must_use]
    pub fn permission_broker(&self) -> Option<Arc<dyn PermissionBroker>> {
        Some(Arc::clone(&self.inner.permission_broker))
    }

    #[must_use]
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.inner.tool_registry
    }

    #[must_use]
    pub fn provider_capability_routes(
        &self,
    ) -> Arc<parking_lot::RwLock<ProviderCapabilityRouteSettings>> {
        Arc::clone(&self.inner.provider_capability_routes)
    }

    #[must_use]
    pub fn hook_dispatcher(&self) -> HookDispatcher {
        HookDispatcher::new(self.inner.hook_registry.snapshot())
    }

    #[must_use]
    pub fn memory_provider(&self) -> Option<Arc<dyn MemoryProvider>> {
        self.effective_memory_provider()
    }

    #[cfg(feature = "memory-external-slot")]
    pub async fn list_memory_items(
        &self,
        options: SessionOptions,
    ) -> Result<Vec<harness_memory::MemorySummary>, HarnessError> {
        self.enforce_tenant(&options)?;
        let manager = self.memory_manager_for_browser(&options).await?;
        manager
            .list_for_actor(memory_actor_from_options(&options))
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-external-slot")]
    pub async fn get_memory_item(
        &self,
        options: SessionOptions,
        id: harness_contracts::MemoryId,
    ) -> Result<harness_memory::MemoryRecord, HarnessError> {
        self.enforce_tenant(&options)?;
        let manager = self.memory_manager_for_browser(&options).await?;
        manager
            .get_for_actor(id, memory_actor_from_options(&options))
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-external-slot")]
    pub async fn update_memory_item_content(
        &self,
        options: SessionOptions,
        id: harness_contracts::MemoryId,
        content: impl Into<String>,
    ) -> Result<harness_memory::MemoryRecord, HarnessError> {
        self.enforce_tenant(&options)?;
        let manager = self.memory_manager_for_browser(&options).await?;
        manager
            .update_content_for_actor(id, memory_actor_from_options(&options), content, None)
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-external-slot")]
    pub async fn delete_memory_item(
        &self,
        options: SessionOptions,
        id: harness_contracts::MemoryId,
    ) -> Result<(), HarnessError> {
        self.enforce_tenant(&options)?;
        let manager = self.memory_manager_for_browser(&options).await?;
        manager
            .forget_for_actor(id, memory_actor_from_options(&options), None)
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-external-slot")]
    pub async fn export_memory_items(
        &self,
        options: SessionOptions,
    ) -> Result<Vec<harness_memory::MemoryRecord>, HarnessError> {
        self.enforce_tenant(&options)?;
        let manager = self.memory_manager_for_browser(&options).await?;
        manager
            .export_for_actor(memory_actor_from_options(&options))
            .await
            .map_err(HarnessError::Memory)
    }

    #[cfg(feature = "memory-external-slot")]
    async fn memory_manager_for_browser(
        &self,
        options: &SessionOptions,
    ) -> Result<Arc<harness_memory::MemoryManager>, HarnessError> {
        self.memory_manager_for_session(options)
            .await?
            .ok_or_else(|| {
                HarnessError::Memory(harness_contracts::MemoryError::ExternalProviderNotConfigured)
            })
    }

    fn effective_memory_provider(&self) -> Option<Arc<dyn MemoryProvider>> {
        self.inner
            .memory_provider
            .as_ref()
            .map(Arc::clone)
            .or_else(|| {
                self.inner
                    .plugin_registry
                    .as_ref()
                    .and_then(harness_plugin::PluginRegistry::registered_memory_provider)
            })
    }

    fn hook_redactor(&self) -> Arc<dyn Redactor> {
        self.inner
            .observer
            .as_ref()
            .map(|observer| Arc::clone(&observer.redactor))
            .unwrap_or_else(default_hook_redactor)
    }

    fn enforce_tenant(&self, options: &SessionOptions) -> Result<(), HarnessError> {
        if options.tenant_id != self.inner.options.tenant_policy.id
            && !self.inner.options.tenant_policy.allow_scoped_tenants
        {
            return Err(HarnessError::InvalidTenant(options.tenant_id));
        }
        Ok(())
    }

    fn enforce_provider_allowed(&self, provider_id: &str) -> Result<(), HarnessError> {
        if let Some(allowed) = &self.inner.options.tenant_policy.allowed_providers {
            if !allowed.contains(provider_id) {
                return Err(HarnessError::PermissionDenied(format!(
                    "provider `{provider_id}` is not allowed by tenant policy"
                )));
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn blob_store(&self) -> Option<Arc<dyn BlobStore>> {
        self.inner.blob_store.as_ref().map(Arc::clone)
    }

    #[must_use]
    pub fn skill_loader(&self) -> Option<&SkillLoader> {
        self.inner.skill_loader.as_ref()
    }

    #[must_use]
    pub fn skill_registry(&self) -> &SkillRegistry {
        &self.inner.skill_registry
    }

    pub async fn validate_workspace_skill_markdown(
        &self,
        markdown: &str,
        source_path: Option<PathBuf>,
    ) -> Result<RuntimeSkillView, HarnessError> {
        let source = SkillSource::Workspace(PathBuf::new());
        let skill =
            parse_skill_markdown(markdown, source, source_path, sdk_current_skill_platform())
                .map_err(|error| HarnessError::Other(format!("parse skill failed: {error}")))?;
        let validator = self
            .inner
            .skill_loader
            .as_ref()
            .map(SkillLoader::validator)
            .unwrap_or_default();
        let skill = validator
            .validate_skill(skill)
            .await
            .map_err(|error| HarnessError::Other(format!("validate skill failed: {error}")))?;
        Ok(runtime_skill_view(
            &skill,
            harness_contracts::SkillStatus::Ready,
            true,
        ))
    }

    pub async fn reload_workspace_managed_skills(
        &self,
        enabled_dir: impl AsRef<Path>,
    ) -> Result<(), HarnessError> {
        let enabled_dir = enabled_dir.as_ref().to_path_buf();
        let source = SkillSource::Workspace(enabled_dir.clone());
        let loader = SkillLoader::default().with_source(SkillSourceConfig::DirectoryPackages {
            path: enabled_dir,
            source_kind: DirectorySourceKind::Workspace,
        });
        let report = loader.load_all().await.map_err(|error| {
            HarnessError::Other(format!("load workspace skills failed: {error}"))
        })?;
        self.replace_workspace_managed_skills(source, report.loaded)
    }

    pub fn list_runtime_skills(&self) -> Vec<RuntimeSkillSummary> {
        let snapshot = self.inner.skill_registry.snapshot();
        snapshot
            .entries
            .values()
            .map(|skill| {
                let status = snapshot
                    .status
                    .get(&skill.id)
                    .cloned()
                    .unwrap_or(harness_contracts::SkillStatus::Ready);
                runtime_skill_summary(skill, status)
            })
            .collect()
    }

    pub fn view_runtime_skill(&self, name: &str, full: bool) -> Option<RuntimeSkillView> {
        let snapshot = self.inner.skill_registry.snapshot();
        let skill = snapshot.entries.get(name)?;
        let status = snapshot
            .status
            .get(&skill.id)
            .cloned()
            .unwrap_or(harness_contracts::SkillStatus::Ready);
        Some(runtime_skill_view(skill, status, full))
    }

    fn replace_workspace_managed_skills(
        &self,
        source: SkillSource,
        skills: Vec<Skill>,
    ) -> Result<(), HarnessError> {
        let current = self.inner.skill_registry.snapshot();
        let old_bindings = self
            .inner
            .skill_registry
            .hook_bindings_in_snapshot(&current);
        let mut next_skills = current
            .entries
            .values()
            .filter(|skill| skill.source != source)
            .map(|skill| skill.as_ref().clone())
            .collect::<Vec<_>>();
        next_skills.extend(skills);

        let replacement = SkillRegistry::builder().with_skills(next_skills).build();
        let mut candidate = replacement.snapshot().as_ref().clone();
        if candidate.entries != current.entries {
            candidate.generation = current.generation.saturating_add(1);
        } else {
            candidate.generation = current.generation;
        }
        let next_bindings = replacement.hook_bindings_in_snapshot(&candidate);
        let next_handler_ids = next_bindings
            .iter()
            .map(|binding| binding.handler_id.clone())
            .collect::<HashSet<_>>();

        let mut registered = Vec::<String>::new();
        for binding in next_bindings {
            if self
                .inner
                .hook_registry
                .origin_for(&binding.handler_id)
                .is_some()
            {
                continue;
            }
            let handler_id = binding.handler_id.clone();
            let handler = skill_hook_handler(binding)?;
            if let Err(error) = self.inner.hook_registry.register(handler) {
                for registered_id in registered {
                    self.inner.hook_registry.deregister(&registered_id);
                }
                return Err(HarnessError::Hook(harness_contracts::HookError::Message(
                    error.to_string(),
                )));
            }
            registered.push(handler_id);
        }

        for binding in old_bindings {
            if !next_handler_ids.contains(&binding.handler_id) {
                self.inner.hook_registry.deregister(&binding.handler_id);
            }
        }
        self.inner.skill_registry.commit_snapshot(candidate);
        Ok(())
    }

    pub fn register_locked_skill_versions(
        &self,
        snapshots: &[LockedSkillVersionSnapshot],
    ) -> Result<(), SkillPackLoaderError> {
        let skills = SkillPackLoaderAdapter::default().load_skills(snapshots)?;
        let skill_count = skills.len();
        self.inner
            .skill_registry
            .register_batch(skills)
            .map_err(|error| SkillPackLoaderError::Registry(error.to_string()))?;
        if let Some(observer) = &self.inner.observer {
            let mut span = observer.start_span(
                "skill.runtime_injection",
                SpanAttributes::new().with(
                    "skill_count",
                    AttributeValue::Int(skill_count.min(i64::MAX as usize) as i64),
                ),
            );
            span.set_status(SpanStatus::Ok);
            span.end();
        }
        Ok(())
    }

    #[must_use]
    pub fn mcp_config(&self) -> Option<&McpConfig> {
        self.inner.mcp_config.as_ref()
    }

    #[must_use]
    pub fn elicitation_handler(&self) -> Option<Arc<dyn ElicitationHandler>> {
        self.inner.elicitation_handler.as_ref().map(Arc::clone)
    }

    #[must_use]
    pub fn plugin_registry(&self) -> Option<&harness_plugin::PluginRegistry> {
        self.inner.plugin_registry.as_ref()
    }

    #[must_use]
    pub fn tracer(&self) -> Option<Arc<dyn Tracer>> {
        self.inner.tracer.as_ref().map(Arc::clone)
    }

    #[must_use]
    pub fn observer(&self) -> Option<Arc<Observer>> {
        self.inner.observer.as_ref().map(Arc::clone)
    }

    #[must_use]
    pub fn aux_model(&self) -> Option<Arc<dyn AuxModelProvider>> {
        self.inner.aux_model.as_ref().map(Arc::clone)
    }

    #[must_use]
    pub fn rule_providers(&self) -> &[Arc<dyn RuleProvider>] {
        &self.inner.rule_providers
    }

    #[must_use]
    pub fn enabled_features(&self) -> &HashSet<String> {
        &self.inner.enabled_features
    }

    #[must_use]
    pub fn enabled_feature_set() -> HashSet<String> {
        let mut features = HashSet::new();
        for feature in compiled_features() {
            features.insert(feature.to_owned());
        }
        features
    }
}

#[cfg(feature = "mcp-server-adapter")]
#[async_trait]
impl HarnessMcpBackend for Harness {
    async fn call_harness_tool(
        &self,
        context: &McpServerRequestContext,
        capability: ExposedCapability,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        match capability {
            ExposedCapability::SessionsList => self.mcp_sessions_list(context, arguments).await,
            ExposedCapability::SessionGet => self.mcp_session_get(context, arguments).await,
            ExposedCapability::MessagesRead => self.mcp_messages_read(context, arguments).await,
            ExposedCapability::MessagesSend => self.mcp_messages_send(context, arguments).await,
            ExposedCapability::AttachmentsFetch => {
                self.mcp_attachments_fetch(context, arguments).await
            }
            ExposedCapability::EventsPoll => self.mcp_events_poll(context, arguments).await,
            ExposedCapability::EventsWait => self.mcp_events_wait(context, arguments).await,
            ExposedCapability::PermissionsListOpen => {
                self.mcp_permissions_list_open(context, arguments).await
            }
            ExposedCapability::PermissionsRespond => {
                self.mcp_permissions_respond(context, arguments).await
            }
            ExposedCapability::ChannelsList => Ok(json!({ "count": 0, "channels": [] })),
            _ => Err(McpServerError::InvalidParams(
                "unsupported harness MCP capability".to_owned(),
            )),
        }
    }
}

#[cfg(feature = "mcp-server-adapter")]
impl Harness {
    async fn mcp_sessions_list(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: SessionsListArgs = mcp_args(arguments)?;
        let mut sessions = self
            .inner
            .event_store
            .list_sessions(
                context.tenant_id,
                SessionFilter {
                    since: args.since,
                    end_reason: None,
                    project_compression_tips: false,
                    limit: args.limit(),
                },
            )
            .await
            .map_err(mcp_journal_error)?;
        if !args.include_ended {
            sessions.retain(|session| session.end_reason.is_none());
        }
        Ok(json!({
            "count": sessions.len(),
            "sessions": sessions,
        }))
    }

    async fn mcp_session_get(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: SessionGetArgs = mcp_args(arguments)?;
        let session_id = parse_session_id(&args.session_id)?;
        let projection = self
            .read_session_projection(context.tenant_id, session_id)
            .await?;
        Ok(json!({
            "session": {
                "session_id": projection.session_id,
                "tenant_id": projection.tenant_id,
                "message_count": projection.messages.len(),
                "permission_count": projection.permission_log.len(),
                "tool_use_count": projection.tool_uses.len(),
                "end_reason": projection.end_reason,
                "last_offset": projection.last_offset,
                "snapshot_id": projection.snapshot_id,
                "usage": projection.usage,
            }
        }))
    }

    async fn mcp_messages_read(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: MessagesReadArgs = mcp_args(arguments)?;
        let session_id = parse_session_id(&args.session_id)?;
        let projection = self
            .read_session_projection(context.tenant_id, session_id)
            .await?;
        let offset = args.offset.unwrap_or(0);
        let limit = args.limit();
        let messages = projection
            .messages
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>();
        Ok(json!({
            "session_id": session_id,
            "offset": offset,
            "limit": limit,
            "count": messages.len(),
            "messages": messages,
        }))
    }

    async fn mcp_messages_send(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: MessagesSendArgs = mcp_args(arguments)?;
        let session_id = parse_session_id(&args.session_id)?;
        let projection = self
            .read_session_projection(context.tenant_id, session_id)
            .await?;
        if projection.end_reason.is_some() {
            return Err(McpServerError::InvalidParams(
                "cannot send message to ended session".to_owned(),
            ));
        }
        let session = self
            .resume_session_from_projection(context.tenant_id, session_id, projection)
            .await?;
        session
            .run_turn(args.message)
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        let projection = session.projection().await;
        Ok(json!({
            "session_id": projection.session_id,
            "message_count": projection.messages.len(),
            "last_offset": projection.last_offset,
            "snapshot_id": projection.snapshot_id,
        }))
    }

    async fn mcp_attachments_fetch(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: AttachmentsFetchArgs = mcp_args(arguments)?;
        let Some(blob_store) = &self.inner.blob_store else {
            return Err(McpServerError::InvalidParams(
                "blob store is not configured".to_owned(),
            ));
        };
        const MAX_ATTACHMENT_BYTES: usize = 8 * 1024 * 1024;
        let meta = blob_store
            .head(context.tenant_id, &args.blob_ref)
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?
            .ok_or_else(|| McpServerError::InvalidParams("blob not found".to_owned()))?;
        if meta.size as usize > MAX_ATTACHMENT_BYTES {
            return Err(McpServerError::InvalidParams(format!(
                "blob exceeds MCP attachment fetch limit: {} > {}",
                meta.size, MAX_ATTACHMENT_BYTES
            )));
        }
        let mut bytes = Vec::new();
        let chunks = blob_store
            .get(context.tenant_id, &args.blob_ref)
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?
            .collect::<Vec<_>>()
            .await;
        for chunk in chunks {
            if bytes.len() + chunk.len() > MAX_ATTACHMENT_BYTES {
                return Err(McpServerError::InvalidParams(format!(
                    "blob exceeds MCP attachment fetch limit: > {MAX_ATTACHMENT_BYTES}"
                )));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(json!({
            "blob_ref": args.blob_ref,
            "meta": meta,
            "content_base64": BASE64_STANDARD.encode(bytes),
        }))
    }

    async fn mcp_events_poll(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: EventsPollArgs = mcp_args(arguments)?;
        self.poll_events(context.tenant_id, args).await
    }

    async fn mcp_events_wait(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: EventsWaitArgs = mcp_args(arguments)?;
        let timeout = Duration::from_millis(args.timeout_ms.unwrap_or(30_000).min(300_000));
        let started = std::time::Instant::now();
        loop {
            let result = self
                .poll_events(
                    context.tenant_id,
                    EventsPollArgs {
                        after_event_id: args.after_event_id.clone(),
                        session_id: args.session_id.clone(),
                        limit: args.limit,
                    },
                )
                .await?;
            if result["count"].as_u64().unwrap_or(0) > 0 || started.elapsed() >= timeout {
                return Ok(result);
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Ok(result);
            }
            tokio::time::sleep(Duration::from_millis(200).min(remaining)).await;
        }
    }

    #[cfg(feature = "stream-permission")]
    async fn mcp_permissions_list_open(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: PermissionsListOpenArgs = mcp_args(arguments)?;
        let limit = args.limit();
        let mut permissions = Vec::new();
        let session_id_filter = args
            .session_id
            .as_deref()
            .map(parse_session_id)
            .transpose()?;
        append_pending_stream_permissions(
            &mut permissions,
            self.inner.permission_resolver.as_ref(),
            context.tenant_id,
            session_id_filter,
            limit,
        );
        if permissions.len() < limit {
            if let Some(session_id) = session_id_filter {
                let projection = self
                    .read_session_projection(context.tenant_id, session_id)
                    .await?;
                permissions.extend(open_permissions(projection, limit - permissions.len()));
            } else {
                let sessions = self
                    .inner
                    .event_store
                    .list_sessions(
                        context.tenant_id,
                        SessionFilter {
                            since: None,
                            end_reason: None,
                            project_compression_tips: false,
                            limit: limit as u32,
                        },
                    )
                    .await
                    .map_err(mcp_journal_error)?;
                for summary in sessions {
                    if permissions.len() >= limit {
                        break;
                    }
                    let projection = self
                        .read_session_projection(context.tenant_id, summary.session_id)
                        .await?;
                    permissions.extend(open_permissions(projection, limit - permissions.len()));
                }
            }
        }
        Ok(json!({
            "count": permissions.len(),
            "permissions": permissions,
        }))
    }

    #[cfg(not(feature = "stream-permission"))]
    async fn mcp_permissions_list_open(
        &self,
        context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: PermissionsListOpenArgs = mcp_args(arguments)?;
        let limit = args.limit();
        let mut permissions = Vec::new();
        if let Some(session_id) = args.session_id {
            let projection = self
                .read_session_projection(context.tenant_id, parse_session_id(&session_id)?)
                .await?;
            permissions.extend(open_permissions(projection, limit));
        } else {
            let sessions = self
                .inner
                .event_store
                .list_sessions(
                    context.tenant_id,
                    SessionFilter {
                        since: None,
                        end_reason: None,
                        project_compression_tips: false,
                        limit: limit as u32,
                    },
                )
                .await
                .map_err(mcp_journal_error)?;
            for summary in sessions {
                if permissions.len() >= limit {
                    break;
                }
                let projection = self
                    .read_session_projection(context.tenant_id, summary.session_id)
                    .await?;
                permissions.extend(open_permissions(projection, limit - permissions.len()));
            }
        }
        Ok(json!({
            "count": permissions.len(),
            "permissions": permissions,
        }))
    }

    async fn mcp_permissions_respond(
        &self,
        _context: &McpServerRequestContext,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        let args: PermissionsRespondArgs = mcp_args(arguments)?;
        self.resolve_permission(parse_request_id(&args.request_id)?, args.decision)
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        Ok(json!({ "resolved": true }))
    }

    async fn poll_events(
        &self,
        tenant_id: TenantId,
        args: EventsPollArgs,
    ) -> Result<Value, McpServerError> {
        let limit = args.limit();
        let after_event_id = args
            .after_event_id
            .as_deref()
            .map(parse_event_id)
            .transpose()?;
        let envelopes = if let Some(session_id) = args.session_id {
            let session_id = parse_session_id(&session_id)?;
            let mut envelopes = self
                .inner
                .event_store
                .read_envelopes(tenant_id, session_id, ReplayCursor::FromStart)
                .await
                .map_err(mcp_journal_error)?
                .collect::<Vec<_>>()
                .await;
            if let Some(after) = after_event_id {
                match envelopes
                    .iter()
                    .position(|envelope| envelope.event_id == after)
                {
                    Some(position) => envelopes.drain(0..=position).for_each(drop),
                    None => envelopes.clear(),
                }
            }
            envelopes.truncate(limit);
            envelopes
        } else {
            self.inner
                .event_store
                .query_after(tenant_id, after_event_id, limit)
                .await
                .map_err(mcp_journal_error)?
        };
        let next_event_id = envelopes
            .last()
            .map(|envelope| envelope.event_id.to_string());
        Ok(json!({
            "count": envelopes.len(),
            "next_event_id": next_event_id,
            "events": envelopes,
        }))
    }

    async fn read_session_projection(
        &self,
        tenant_id: TenantId,
        session_id: harness_contracts::SessionId,
    ) -> Result<SessionProjection, McpServerError> {
        let envelopes = self
            .inner
            .event_store
            .read_envelopes(tenant_id, session_id, ReplayCursor::FromStart)
            .await
            .map_err(mcp_journal_error)?
            .collect::<Vec<_>>()
            .await;
        if envelopes.is_empty() {
            return Err(McpServerError::InvalidParams(format!(
                "session not found: {session_id}"
            )));
        }
        SessionProjection::replay(envelopes)
            .map_err(|error| McpServerError::Internal(error.to_string()))
    }

    async fn resume_session_from_projection(
        &self,
        tenant_id: TenantId,
        session_id: harness_contracts::SessionId,
        projection: SessionProjection,
    ) -> Result<Session, McpServerError> {
        let mut options = self.inner.options.default_session_options.clone();
        options.workspace_root = self.inner.options.workspace_root.clone();
        options.tenant_id = tenant_id;
        options.session_id = session_id;
        self.enforce_tenant(&options)
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        let limit_permit = self
            .inner
            .session_limits
            .try_acquire()
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        #[cfg(feature = "memory-external-slot")]
        let memory_manager = self
            .memory_manager_for_session(&options)
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        #[cfg(feature = "memory-external-slot")]
        let engine = self
            .engine_for_session(&options, memory_manager.clone(), None)
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        #[cfg(not(feature = "memory-external-slot"))]
        let engine = self
            .engine_for_session(&options, None)
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        let event_store: Arc<dyn EventStore> = Arc::new(LifecycleHookEventStore {
            inner: Arc::clone(&self.inner.event_store),
            hooks: HookDispatcher::new(self.inner.hook_registry.snapshot()),
            tenant_id: options.tenant_id,
            session_id: options.session_id,
            #[cfg(feature = "memory-external-slot")]
            user_id: options.user_id.clone(),
            #[cfg(feature = "memory-external-slot")]
            team_id: options.team_id,
            workspace_root: options.workspace_root.clone(),
            redactor: self.hook_redactor(),
            session_limits: Arc::clone(&self.inner.session_limits),
            deleted_conversation_sessions: Arc::clone(&self.inner.deleted_conversation_sessions),
            summary_state: parking_lot::Mutex::new(MemorySessionSummaryState::default()),
            #[cfg(feature = "memory-external-slot")]
            memory_manager,
        });
        let session = Session::builder()
            .with_options(options)
            .with_event_store(event_store)
            .with_turn_runner(Arc::new(EngineSessionTurnRunner {
                engine,
                active_conversation_runs: Arc::clone(&self.inner.active_conversation_runs),
                skill_registry: Some(self.inner.skill_registry.clone()),
                skill_metrics_sink: self.skill_metrics_sink(),
                skill_config_snapshot: self.inner.skill_config_snapshot.clone(),
            }))
            .with_skill_reload_cap(Arc::new(SdkSkillReloadCap {
                inner: Arc::clone(&self.inner),
            }))
            .with_projection(projection)
            .build()
            .await
            .map_err(|error| McpServerError::Internal(error.to_string()))?;
        limit_permit.disarm();
        Ok(session)
    }
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionsListArgs {
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    since: Option<DateTime<chrono::Utc>>,
    #[serde(default = "default_true")]
    include_ended: bool,
}

#[cfg(feature = "mcp-server-adapter")]
impl SessionsListArgs {
    fn limit(&self) -> u32 {
        self.limit.unwrap_or(50).clamp(1, 200)
    }
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionGetArgs {
    session_id: String,
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MessagesReadArgs {
    session_id: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

#[cfg(feature = "mcp-server-adapter")]
impl MessagesReadArgs {
    fn limit(&self) -> usize {
        self.limit.unwrap_or(50).clamp(1, 200)
    }
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MessagesSendArgs {
    session_id: String,
    message: String,
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachmentsFetchArgs {
    blob_ref: BlobRef,
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsPollArgs {
    #[serde(default)]
    after_event_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[cfg(feature = "mcp-server-adapter")]
impl EventsPollArgs {
    fn limit(&self) -> usize {
        self.limit.unwrap_or(20).clamp(1, 500)
    }
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsWaitArgs {
    #[serde(default)]
    after_event_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PermissionsListOpenArgs {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[cfg(feature = "mcp-server-adapter")]
impl PermissionsListOpenArgs {
    fn limit(&self) -> usize {
        self.limit.unwrap_or(50).clamp(1, 200)
    }
}

#[cfg(feature = "mcp-server-adapter")]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PermissionsRespondArgs {
    request_id: String,
    decision: Decision,
}

#[cfg(feature = "mcp-server-adapter")]
fn default_true() -> bool {
    true
}

#[cfg(feature = "mcp-server-adapter")]
fn mcp_args<T>(arguments: Value) -> Result<T, McpServerError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(arguments)
        .map_err(|error| McpServerError::InvalidParams(error.to_string()))
}

#[cfg(feature = "mcp-server-adapter")]
fn parse_session_id(value: &str) -> Result<harness_contracts::SessionId, McpServerError> {
    value
        .parse()
        .map_err(|error| McpServerError::InvalidParams(format!("invalid session_id: {error}")))
}

#[cfg(feature = "mcp-server-adapter")]
fn parse_event_id(value: &str) -> Result<EventId, McpServerError> {
    value
        .parse()
        .map_err(|error| McpServerError::InvalidParams(format!("invalid event_id: {error}")))
}

#[cfg(feature = "mcp-server-adapter")]
fn parse_request_id(value: &str) -> Result<RequestId, McpServerError> {
    value
        .parse()
        .map_err(|error| McpServerError::InvalidParams(format!("invalid request_id: {error}")))
}

#[cfg(feature = "mcp-server-adapter")]
fn mcp_journal_error(error: harness_contracts::JournalError) -> McpServerError {
    McpServerError::Internal(error.to_string())
}

#[cfg(all(feature = "mcp-server-adapter", feature = "stream-permission"))]
fn append_pending_stream_permissions(
    permissions: &mut Vec<harness_session::PermissionRecord>,
    resolver: Option<&ResolverHandle>,
    tenant_id: TenantId,
    session_id: Option<harness_contracts::SessionId>,
    limit: usize,
) {
    let Some(resolver) = resolver else {
        return;
    };

    let remaining = limit.saturating_sub(permissions.len());
    if remaining == 0 {
        return;
    }

    permissions.extend(
        resolver
            .pending_requests()
            .into_iter()
            .filter(|request| request.tenant_id == tenant_id)
            .filter(|request| {
                session_id
                    .map(|session_id| request.session_id == session_id)
                    .unwrap_or(true)
            })
            .take(remaining)
            .map(permission_request_to_record),
    );
}

#[cfg(all(feature = "mcp-server-adapter", feature = "stream-permission"))]
fn permission_request_to_record(request: PermissionRequest) -> harness_session::PermissionRecord {
    harness_session::PermissionRecord {
        request_id: request.request_id,
        tool_use_id: request.tool_use_id,
        tool_name: request.tool_name,
        subject: request.subject,
        decision: None,
        scope: request.scope_hint,
    }
}

#[cfg(feature = "mcp-server-adapter")]
fn open_permissions(
    projection: SessionProjection,
    limit: usize,
) -> Vec<harness_session::PermissionRecord> {
    projection
        .permission_log
        .into_iter()
        .filter(|record| record.decision.is_none())
        .take(limit)
        .collect()
}

impl WorkspaceCreateRequest for WorkspaceSpec {
    type Output = Workspace;

    fn create_workspace(self, harness: &Harness) -> Result<Self::Output, HarnessError> {
        harness.create_workspace_record(self)
    }
}

impl WorkspaceCreateRequest for PathBuf {
    type Output = PathBuf;

    fn create_workspace(self, harness: &Harness) -> Result<Self::Output, HarnessError> {
        harness.create_workspace_path(&self)
    }
}

impl WorkspaceCreateRequest for &PathBuf {
    type Output = PathBuf;

    fn create_workspace(self, harness: &Harness) -> Result<Self::Output, HarnessError> {
        harness.create_workspace_path(self)
    }
}

impl WorkspaceCreateRequest for &Path {
    type Output = PathBuf;

    fn create_workspace(self, harness: &Harness) -> Result<Self::Output, HarnessError> {
        harness.create_workspace_path(self)
    }
}

fn snapshot_for_supported_model(
    model: &dyn ModelProvider,
    model_id: &str,
) -> Result<ModelRuntimeSnapshot, HarnessError> {
    let provider_id = model.provider_id().to_owned();
    let descriptor = model
        .supported_models()
        .into_iter()
        .find(|descriptor| descriptor.provider_id == provider_id && descriptor.model_id == model_id)
        .ok_or_else(|| {
            HarnessError::Engine(harness_contracts::EngineError::Message(format!(
                "unsupported model id for provider {provider_id}: {model_id}"
            )))
        })?;
    Ok(ModelRuntimeSnapshot {
        provider_id: descriptor.provider_id,
        model_id: descriptor.model_id,
        protocol: descriptor.protocol,
        context_window: descriptor.context_window,
        conversation_capability: descriptor.conversation_capability,
        lifecycle: descriptor.lifecycle,
        pricing: descriptor.pricing,
    })
}

fn context_budget_for_model(
    model_snapshot: &ModelRuntimeSnapshot,
    context_compression_trigger_ratio: f32,
) -> TokenBudget {
    let mut budget = TokenBudget::default();
    let context_window = u64::from(model_snapshot.context_window);
    budget.soft_budget_ratio = context_compression_trigger_ratio.clamp(0.5, 0.95);
    budget.hard_budget_ratio = 0.95;

    if context_window == 0 {
        return budget;
    }

    let declared_output_tokens =
        u64::from(model_snapshot.conversation_capability.max_output_tokens);
    let reserved_output_tokens = if declared_output_tokens > 0 {
        declared_output_tokens.min(context_window / 2)
    } else {
        4_096_u64.min(context_window / 4)
    };
    budget.max_tokens_per_turn = context_window.saturating_sub(reserved_output_tokens).max(1);
    budget
}

fn apply_non_default_session_options(options: &mut SessionOptions, defaults: &SessionOptions) {
    if defaults.workspace_root != PathBuf::from(".") {
        options.workspace_root = defaults.workspace_root.clone();
    }
    if defaults.workspace_bootstrap.is_some() {
        options.workspace_bootstrap = defaults.workspace_bootstrap.clone();
    }
    if defaults.tool_search != ToolSearchMode::default() {
        options.tool_search = defaults.tool_search.clone();
    }
    if defaults.model_id.is_some() {
        options.model_id = defaults.model_id.clone();
    }
    if defaults.protocol.is_some() {
        options.protocol = defaults.protocol;
    }
    if defaults.model_extra != Value::Null {
        options.model_extra = defaults.model_extra.clone();
    }
    if defaults.permission_mode != PermissionMode::Default {
        options.permission_mode = defaults.permission_mode;
    }
    if defaults.interactivity != InteractivityLevel::NoInteractive {
        options.interactivity = defaults.interactivity;
    }
    if defaults.user_id.is_some() {
        options.user_id = defaults.user_id.clone();
    }
    if defaults.team_id.is_some() {
        options.team_id = defaults.team_id;
    }
    if defaults.system_prompt_addendum.is_some() {
        options.system_prompt_addendum = defaults.system_prompt_addendum.clone();
    }
    if defaults.max_iterations > 0 {
        options.max_iterations = defaults.max_iterations;
    }
    if defaults.context_compression_trigger_ratio != 0.8 {
        options.context_compression_trigger_ratio = defaults.context_compression_trigger_ratio;
    }
}

#[cfg(test)]
mod tests {
    use harness_contracts::{BlobId, BlobRef, ModelModality};

    use super::*;

    #[test]
    fn conversation_turn_parts_promotes_file_attachment_when_model_accepts_file_input() {
        let input = ConversationTurnInput {
            client_message_id: None,
            prompt: "Summarize this".to_owned(),
            context_references: Vec::new(),
            attachments: vec![attachment("notes.pdf", "application/pdf")],
        };

        let parts = conversation_turn_parts(&input, &[ModelModality::Text, ModelModality::File]);

        assert!(parts.iter().any(|part| {
            matches!(
                part,
                MessagePart::File {
                    mime_type,
                    blob_ref
                } if mime_type == "application/pdf" && blob_ref.content_type.as_deref() == Some("application/pdf")
            )
        }));
        assert!(
            !parts
                .iter()
                .filter_map(|part| match part {
                    MessagePart::Text(text) => Some(text),
                    _ => None,
                })
                .any(|text| text.contains("notes.pdf")),
            "file-capable models should receive files as model input, not text context"
        );
    }

    #[test]
    fn conversation_turn_parts_keeps_file_attachment_as_text_when_model_lacks_file_input() {
        let input = ConversationTurnInput {
            client_message_id: None,
            prompt: "Summarize this".to_owned(),
            context_references: Vec::new(),
            attachments: vec![attachment("notes.pdf", "application/pdf")],
        };

        let parts = conversation_turn_parts(&input, &[ModelModality::Text]);

        assert!(parts
            .iter()
            .any(|part| { matches!(part, MessagePart::Text(text) if text.contains("notes.pdf")) }));
        assert!(!parts
            .iter()
            .any(|part| matches!(part, MessagePart::File { .. })));
    }

    #[test]
    #[cfg(not(feature = "observability-redactor"))]
    fn default_hook_redactor_redacts_without_observability_redactor_feature() {
        let redactor = default_hook_redactor();
        let redacted = redactor.redact(
            "token sk-abcdefghijklmnopqrstuvwxyz and ghp_abcdefghijklmnopqrstuvwxyz \
             bearer synthetic-token Basic synthetic-basic \
             jwt eyJabcdefgh.eyJijklmnop.eyJqrstuvwx \
             bearer\twhitespace-token \
             db postgres://user:password@example.com/app \
             paths /Users/goya/.ssh/config C:/Users/goya/.ssh/config \
             password=supersecret client_secret: verysecretvalue \
             google AIzaabcdefghijklmnopqrstuvwxyz123456789 \
             stripe rk_live_abcdefghijklmnop \
             -----BEGIN OPENSSH PRIVATE KEY-----truncated",
            &RedactRules::default(),
        );

        assert!(!redacted.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(!redacted.contains("ghp_abcdefghijklmnopqrstuvwxyz"));
        assert!(!redacted.contains("synthetic-token"));
        assert!(!redacted.contains("synthetic-basic"));
        assert!(!redacted.contains("whitespace-token"));
        assert!(!redacted.contains("eyJabcdefgh.eyJijklmnop.eyJqrstuvwx"));
        assert!(!redacted.contains("postgres://user:password@example.com/app"));
        assert!(!redacted.contains("/Users/goya/.ssh/config"));
        assert!(!redacted.contains("C:/Users/goya/.ssh/config"));
        assert!(!redacted.contains("supersecret"));
        assert!(!redacted.contains("verysecretvalue"));
        assert!(!redacted.contains("AIzaabcdefghijklmnopqrstuvwxyz123456789"));
        assert!(!redacted.contains("rk_live_abcdefghijklmnop"));
        assert!(!redacted.contains("BEGIN OPENSSH PRIVATE KEY"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    #[cfg(not(feature = "observability-redactor"))]
    fn default_hook_redactor_honors_rules_without_observability_redactor_feature() {
        let redactor = default_hook_redactor();
        let only_database_url = redactor.redact(
            "token sk-abcdefghijklmnopqrstuvwxyz db postgres://user:password@example.com/app",
            &RedactRules {
                pattern_set: RedactPatternSet::Only(vec![RedactPatternKind::DatabaseUrl]),
                ..RedactRules::default()
            },
        );

        assert!(only_database_url.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(!only_database_url.contains("postgres://user:password@example.com/app"));

        let event_body = redactor.redact(
            "email user@example.com ip 10.1.2.3",
            &RedactRules::default(),
        );
        assert!(event_body.contains("user@example.com"));
        assert!(event_body.contains("10.1.2.3"));

        let log_only = redactor.redact(
            "email <user@example.com> ip [10.1.2.3]",
            &RedactRules {
                scope: RedactScope::LogOnly,
                ..RedactRules::default()
            },
        );
        assert!(!log_only.contains("user@example.com"));
        assert!(log_only.contains("10.1.2.3"));

        let trace_only = redactor.redact(
            "email \"user@example.com\" ip [10.1.2.3]",
            &RedactRules {
                scope: RedactScope::TraceOnly,
                ..RedactRules::default()
            },
        );
        assert!(trace_only.contains("user@example.com"));
        assert!(!trace_only.contains("10.1.2.3"));
    }

    fn attachment(name: &str, mime_type: &str) -> ConversationAttachmentReference {
        ConversationAttachmentReference {
            id: "attachment-test".to_owned(),
            name: name.to_owned(),
            mime_type: mime_type.to_owned(),
            size_bytes: 42,
            blob_ref: BlobRef {
                id: BlobId::from_u128(42),
                size: 42,
                content_hash: [7; 32],
                content_type: Some(mime_type.to_owned()),
            },
        }
    }
}

fn apply_explicit_session_options(options: &mut SessionOptions, explicit: &SessionOptions) {
    if explicit.workspace_ref.is_some() {
        options.workspace_ref = explicit.workspace_ref;
    }
    if explicit.workspace_root != PathBuf::from(".") {
        options.workspace_root = explicit.workspace_root.clone();
    }
    if explicit.workspace_bootstrap.is_some() {
        options.workspace_bootstrap = explicit.workspace_bootstrap.clone();
    }
    if explicit.tenant_id != TenantId::SINGLE {
        options.tenant_id = explicit.tenant_id;
    }
    if explicit.tool_search != ToolSearchMode::default() {
        options.tool_search = explicit.tool_search.clone();
    }
    if explicit.model_id.is_some() {
        options.model_id = explicit.model_id.clone();
    }
    if explicit.protocol.is_some() {
        options.protocol = explicit.protocol;
    }
    if explicit.model_extra != Value::Null {
        options.model_extra = explicit.model_extra.clone();
    }
    if explicit.permission_mode != PermissionMode::Default {
        options.permission_mode = explicit.permission_mode;
    }
    if explicit.interactivity != InteractivityLevel::NoInteractive {
        options.interactivity = explicit.interactivity;
    }
    if explicit.user_id.is_some() {
        options.user_id = explicit.user_id.clone();
    }
    if explicit.team_id.is_some() {
        options.team_id = explicit.team_id;
    }
    if explicit.system_prompt_addendum.is_some() {
        options.system_prompt_addendum = explicit.system_prompt_addendum.clone();
    }
    if explicit.max_iterations > 0 {
        options.max_iterations = explicit.max_iterations;
    }
    if explicit.context_compression_trigger_ratio != 0.8 {
        options.context_compression_trigger_ratio = explicit.context_compression_trigger_ratio;
    }
    options.session_id = explicit.session_id;
}

fn append_system_prompt_addendum(base: Option<String>, addendum: Option<String>) -> Option<String> {
    match (base, addendum) {
        (Some(base), Some(addendum)) if !base.trim().is_empty() && !addendum.trim().is_empty() => {
            Some(format!("{base}\n\n{addendum}"))
        }
        (Some(base), _) if !base.trim().is_empty() => Some(base),
        (_, Some(addendum)) if !addendum.trim().is_empty() => Some(addendum),
        _ => None,
    }
}

fn resolve_bootstrap_path(root: &Path, relative_path: &Path) -> Result<PathBuf, HarnessError> {
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(HarnessError::PermissionDenied(format!(
            "workspace bootstrap path must stay inside workspace: {}",
            relative_path.display()
        )));
    }
    Ok(root.join(relative_path))
}

#[cfg(feature = "agents-team")]
fn topology_kind(topology: harness_team::Topology) -> TopologyKind {
    match topology {
        harness_team::Topology::CoordinatorWorker => TopologyKind::CoordinatorWorker,
        harness_team::Topology::PeerToPeer => TopologyKind::PeerToPeer,
        harness_team::Topology::RoleRouted => TopologyKind::RoleRouted,
        harness_team::Topology::Custom => TopologyKind::Custom("sdk".to_owned()),
    }
}

fn redact_business_event(event: Event, redactor: &dyn Redactor) -> Event {
    let Ok(mut value) = serde_json::to_value(&event) else {
        return event;
    };
    redact_json_strings(&mut value, redactor);
    serde_json::from_value(value).unwrap_or(event)
}

fn redact_business_event_for_display(event: Event, redactor: &dyn Redactor) -> Event {
    let event = redact_business_event(event, redactor);
    let default_redactor = default_hook_redactor();
    redact_business_event(event, default_redactor.as_ref())
}

fn redact_json_strings(value: &mut Value, redactor: &dyn Redactor) {
    match value {
        Value::String(text) => {
            *text = redactor.redact(text, &business_event_redact_rules());
        }
        Value::Array(items) => {
            for item in items {
                redact_json_strings(item, redactor);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                redact_json_strings(item, redactor);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn business_event_redact_rules() -> RedactRules {
    RedactRules {
        scope: RedactScope::EventBody,
        replacement: "[REDACTED]".to_owned(),
        pattern_set: RedactPatternSet::Default,
    }
}

#[cfg(feature = "observability-redactor")]
fn default_hook_redactor() -> Arc<dyn Redactor> {
    Arc::new(DefaultRedactor::default())
}

#[cfg(not(feature = "observability-redactor"))]
fn default_hook_redactor() -> Arc<dyn Redactor> {
    Arc::new(MinimalHookRedactor)
}

#[cfg(not(feature = "observability-redactor"))]
struct MinimalHookRedactor;

#[cfg(not(feature = "observability-redactor"))]
impl Redactor for MinimalHookRedactor {
    fn redact(&self, input: &str, rules: &RedactRules) -> String {
        if matches!(rules.pattern_set, RedactPatternSet::None) {
            return input.to_owned();
        }
        let mut output = input.to_owned();
        if minimal_pattern_enabled(RedactScope::All, RedactPatternKind::PrivateKey, rules) {
            output = redact_private_key_blocks(&output, &rules.replacement);
        }
        if minimal_pattern_enabled(RedactScope::All, RedactPatternKind::ApiKey, rules) {
            for prefix in [
                "sk-ant-",
                "sk-",
                "ghp_",
                "gho_",
                "ghu_",
                "ghs_",
                "ghr_",
                "xoxb-",
                "xoxp_",
                "xoxp-",
                "xoxa-",
                "xoxr-",
                "xoxs-",
                "github_pat_",
                "npm_",
                "lin_api_",
                "secret_",
                "sk_live_",
                "rk_live_",
            ] {
                output = redact_prefixed_tokens(&output, prefix, &rules.replacement);
            }
            for (prefix, min_len) in [("AKIA", 16), ("ASIA", 16), ("A3T", 17), ("AIza", 35)] {
                output = redact_prefixed_tokens_min(&output, prefix, min_len, &rules.replacement);
            }
            output = redact_secret_assignments(
                &output,
                &["password", "passwd", "pwd", "secret", "client_secret"],
                &rules.replacement,
            );
        }
        if minimal_pattern_enabled(RedactScope::All, RedactPatternKind::BearerToken, rules) {
            output = redact_auth_scheme_tokens(&output, "Bearer", &rules.replacement);
            output = redact_auth_scheme_tokens(&output, "Basic", &rules.replacement);
            output = redact_jwt_like_tokens(&output, &rules.replacement);
        }
        if minimal_pattern_enabled(RedactScope::All, RedactPatternKind::OAuthCode, rules) {
            output = redact_secret_assignments(
                &output,
                &["code", "oauth_code", "refresh_token", "access_token"],
                &rules.replacement,
            );
        }
        if minimal_pattern_enabled(RedactScope::All, RedactPatternKind::DatabaseUrl, rules) {
            output = redact_database_urls(&output, &rules.replacement);
        }
        if minimal_pattern_enabled(RedactScope::TraceOnly, RedactPatternKind::PrivateIp, rules) {
            output = redact_private_ip_addresses(&output, &rules.replacement);
        }
        if minimal_pattern_enabled(RedactScope::LogOnly, RedactPatternKind::Email, rules) {
            output = redact_email_addresses(&output, &rules.replacement);
        }
        if minimal_default_event_body_patterns_enabled(rules) {
            output = redact_private_absolute_paths(&output, &rules.replacement);
        }
        output
    }
}

#[cfg(not(feature = "observability-redactor"))]
fn minimal_default_event_body_patterns_enabled(rules: &RedactRules) -> bool {
    let scope_matches = matches!(rules.scope, RedactScope::All | RedactScope::EventBody);
    let pattern_matches = matches!(
        rules.pattern_set,
        RedactPatternSet::Default | RedactPatternSet::AllBuiltins
    );
    scope_matches && pattern_matches
}

#[cfg(not(feature = "observability-redactor"))]
fn minimal_pattern_enabled(
    pattern_scope: RedactScope,
    kind: RedactPatternKind,
    rules: &RedactRules,
) -> bool {
    let scope_matches = matches!(rules.scope, RedactScope::All)
        || matches!(pattern_scope, RedactScope::All)
        || pattern_scope == rules.scope;
    if !scope_matches {
        return false;
    }
    match &rules.pattern_set {
        RedactPatternSet::Default | RedactPatternSet::AllBuiltins => true,
        RedactPatternSet::Only(kinds) => kinds.iter().any(|candidate| candidate == &kind),
        RedactPatternSet::None => false,
        _ => false,
    }
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_prefixed_tokens(input: &str, prefix: &str, replacement: &str) -> String {
    redact_prefixed_tokens_min(input, prefix, 1, replacement)
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_auth_scheme_tokens(input: &str, scheme: &str, replacement: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        let remaining = &input[cursor..];
        if remaining
            .get(..scheme.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(scheme))
        {
            let mut offset = scheme.len();
            let whitespace_len = ascii_whitespace_prefix_len(&remaining[offset..]);
            if whitespace_len > 0 {
                offset += whitespace_len;
                let token_len = remaining[offset..]
                    .char_indices()
                    .take_while(|(_, ch)| {
                        ch.is_ascii_alphanumeric()
                            || matches!(*ch, '_' | '-' | '.' | '~' | '+' | '/' | '=')
                    })
                    .last()
                    .map_or(0, |(index, ch)| index + ch.len_utf8());
                if token_len > 0 {
                    output.push_str(replacement);
                    cursor += offset + token_len;
                    continue;
                }
            }
        }
        let ch = remaining
            .chars()
            .next()
            .expect("cursor should point to char boundary");
        output.push(ch);
        cursor += ch.len_utf8();
    }
    output
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_prefixed_tokens_min(
    input: &str,
    prefix: &str,
    min_token_len: usize,
    replacement: &str,
) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        let remaining = &input[cursor..];
        if remaining
            .get(..prefix.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
        {
            let token_len = remaining[prefix.len()..]
                .char_indices()
                .take_while(|(_, ch)| {
                    ch.is_ascii_alphanumeric()
                        || matches!(*ch, '_' | '-' | '.' | '~' | '+' | '/' | '=')
                })
                .last()
                .map_or(0, |(index, ch)| index + ch.len_utf8());
            if token_len >= min_token_len {
                output.push_str(replacement);
                cursor += prefix.len() + token_len;
                continue;
            }
        }
        let ch = remaining
            .chars()
            .next()
            .expect("cursor should point to char boundary");
        output.push(ch);
        cursor += ch.len_utf8();
    }
    output
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_private_key_blocks(input: &str, replacement: &str) -> String {
    let mut output = input.to_owned();
    for (begin, end) in [
        (
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            "-----END OPENSSH PRIVATE KEY-----",
        ),
        (
            "-----BEGIN RSA PRIVATE KEY-----",
            "-----END RSA PRIVATE KEY-----",
        ),
        (
            "-----BEGIN EC PRIVATE KEY-----",
            "-----END EC PRIVATE KEY-----",
        ),
        ("-----BEGIN PRIVATE KEY-----", "-----END PRIVATE KEY-----"),
    ] {
        while let Some(start) = output.find(begin) {
            let end_index = output[start..]
                .find(end)
                .map_or(output.len(), |relative_end| {
                    start + relative_end + end.len()
                });
            output.replace_range(start..end_index, replacement);
        }
    }
    output
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_jwt_like_tokens(input: &str, replacement: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < input.len() {
        let remaining = &input[cursor..];
        if remaining.starts_with("eyJ") {
            let token_len = remaining
                .char_indices()
                .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || matches!(*ch, '_' | '-' | '.'))
                .last()
                .map_or(0, |(index, ch)| index + ch.len_utf8());
            let token = &remaining[..token_len];
            let parts = token.split('.').collect::<Vec<_>>();
            if parts.len() >= 3 && parts[0].len() >= 8 && parts[1].len() >= 8 && parts[2].len() >= 8
            {
                output.push_str(replacement);
                cursor += token_len;
                continue;
            }
        }
        let ch = remaining
            .chars()
            .next()
            .expect("cursor should point to char boundary");
        output.push(ch);
        cursor += ch.len_utf8();
    }
    output
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_secret_assignments(input: &str, names: &[&str], replacement: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    'scan: while cursor < input.len() {
        let remaining = &input[cursor..];
        for name in names.iter().copied() {
            if let Some(match_len) = assignment_match_len(remaining, name) {
                output.push_str(replacement);
                cursor += match_len;
                continue 'scan;
            }
        }
        let ch = remaining
            .chars()
            .next()
            .expect("cursor should point to char boundary");
        output.push(ch);
        cursor += ch.len_utf8();
    }
    output
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_private_ip_addresses(input: &str, replacement: &str) -> String {
    replace_matching_tokens(input, replacement, is_private_ipv4)
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_email_addresses(input: &str, replacement: &str) -> String {
    replace_matching_tokens(input, replacement, is_email_like)
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_private_absolute_paths(input: &str, replacement: &str) -> String {
    replace_matching_tokens(input, replacement, is_private_absolute_path_like)
}

#[cfg(not(feature = "observability-redactor"))]
fn replace_matching_tokens(
    input: &str,
    replacement: &str,
    matches_token: impl Fn(&str) -> bool,
) -> String {
    let mut output = String::with_capacity(input.len());
    let mut token_start: Option<usize> = None;
    for (index, ch) in input.char_indices() {
        if ch.is_ascii_whitespace() {
            if let Some(start) = token_start.take() {
                let token = &input[start..index];
                if matches_token(token) {
                    output.push_str(replacement);
                } else {
                    output.push_str(token);
                }
            }
            output.push(ch);
        } else if token_start.is_none() {
            token_start = Some(index);
        }
    }
    if let Some(start) = token_start {
        let token = &input[start..];
        if matches_token(token) {
            output.push_str(replacement);
        } else {
            output.push_str(token);
        }
    }
    output
}

#[cfg(not(feature = "observability-redactor"))]
fn is_private_ipv4(token: &str) -> bool {
    let token = token.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | ';' | ':' | ')' | '(' | '.' | '[' | ']' | '<' | '>' | '"' | '\''
        )
    });
    let octets = token
        .split('.')
        .map(str::parse::<u8>)
        .collect::<Result<Vec<_>, _>>();
    let Ok(octets) = octets else {
        return false;
    };
    if octets.len() != 4 {
        return false;
    }
    matches!(
        octets.as_slice(),
        [10, _, _, _] | [172, 16..=31, _, _] | [192, 168, _, _] | [127, _, _, _]
    )
}

#[cfg(not(feature = "observability-redactor"))]
fn is_email_like(token: &str) -> bool {
    let token = token.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | ';' | ':' | ')' | '(' | '.' | '[' | ']' | '<' | '>' | '"' | '\''
        )
    });
    let Some((local, domain)) = token.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain
            .rsplit_once('.')
            .is_some_and(|(name, tld)| !name.is_empty() && tld.len() >= 2)
}

#[cfg(not(feature = "observability-redactor"))]
fn is_private_absolute_path_like(token: &str) -> bool {
    let token = token.trim_matches(|ch: char| {
        matches!(
            ch,
            ',' | ';' | ':' | ')' | '(' | '[' | ']' | '<' | '>' | '"' | '\''
        )
    });
    token.contains("/Users/")
        || token.contains("/home/")
        || token.contains("/private/var/")
        || token.as_bytes().windows(3).any(|window| {
            window[0].is_ascii_alphabetic()
                && window[1] == b':'
                && matches!(window[2], b'\\' | b'/')
        })
}

#[cfg(not(feature = "observability-redactor"))]
fn assignment_match_len(input: &str, name: &str) -> Option<usize> {
    if !input
        .get(..name.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
    {
        return None;
    }

    let mut offset = name.len();
    offset += ascii_whitespace_prefix_len(&input[offset..]);
    let delimiter = input[offset..].chars().next()?;
    if !matches!(delimiter, ':' | '=') {
        return None;
    }
    offset += delimiter.len_utf8();
    offset += ascii_whitespace_prefix_len(&input[offset..]);

    let quote = input[offset..]
        .chars()
        .next()
        .filter(|ch| matches!(*ch, '"' | '\''));
    if let Some(quote) = quote {
        offset += quote.len_utf8();
    }

    let value_len = input[offset..]
        .char_indices()
        .take_while(|(_, ch)| match quote {
            Some(quote) => *ch != quote,
            None => !ch.is_ascii_whitespace() && !matches!(*ch, '"' | '\''),
        })
        .last()
        .map_or(0, |(index, ch)| index + ch.len_utf8());
    if value_len < 8 {
        return None;
    }
    Some(offset + value_len)
}

#[cfg(not(feature = "observability-redactor"))]
fn ascii_whitespace_prefix_len(input: &str) -> usize {
    input
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_whitespace())
        .last()
        .map_or(0, |(index, ch)| index + ch.len_utf8())
}

#[cfg(not(feature = "observability-redactor"))]
fn redact_database_urls(input: &str, replacement: &str) -> String {
    let schemes = [
        "postgres://",
        "postgresql://",
        "mysql://",
        "mongodb://",
        "mongodb+srv://",
        "redis://",
        "amqp://",
        "amqps://",
    ];
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    'scan: while cursor < input.len() {
        let remaining = &input[cursor..];
        for scheme in schemes {
            if remaining
                .get(..scheme.len())
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(scheme))
            {
                let url_len = remaining
                    .char_indices()
                    .take_while(|(_, ch)| !ch.is_ascii_whitespace())
                    .last()
                    .map_or(0, |(index, ch)| index + ch.len_utf8());
                let url = &remaining[..url_len];
                if url.contains('@') {
                    output.push_str(replacement);
                    cursor += url_len;
                    continue 'scan;
                }
            }
        }
        let ch = remaining
            .chars()
            .next()
            .expect("cursor should point to char boundary");
        output.push(ch);
        cursor += ch.len_utf8();
    }
    output
}

const GOVERNED_WORKSPACE_DIRS: &[&str] = &[
    "config",
    "data",
    "runtime/events",
    "runtime/sessions",
    "logs",
    "tmp",
];

fn plugin_capabilities_summary(
    manifest: &harness_plugin::PluginManifest,
) -> PluginCapabilitiesSummary {
    PluginCapabilitiesSummary {
        tools: manifest
            .capabilities
            .tools
            .len()
            .try_into()
            .unwrap_or(u16::MAX),
        hooks: manifest
            .capabilities
            .hooks
            .len()
            .try_into()
            .unwrap_or(u16::MAX),
        mcp_servers: manifest
            .capabilities
            .mcp_servers
            .len()
            .try_into()
            .unwrap_or(u16::MAX),
        skills: manifest
            .capabilities
            .skills
            .len()
            .try_into()
            .unwrap_or(u16::MAX),
        steering: manifest.capabilities.steering,
        memory_provider: manifest.capabilities.memory_provider.is_some(),
        coordinator: manifest.capabilities.coordinator_strategy.is_some(),
    }
}

fn manifest_origin_ref(origin: &ManifestOrigin) -> ManifestOriginRef {
    match origin {
        ManifestOrigin::File { path } => ManifestOriginRef::File {
            path: path.display().to_string(),
        },
        ManifestOrigin::CargoExtension { binary, .. } => ManifestOriginRef::CargoExtension {
            binary: binary.display().to_string(),
        },
        ManifestOrigin::RemoteRegistry { endpoint, .. } => ManifestOriginRef::RemoteRegistry {
            endpoint: endpoint.clone(),
        },
        _ => ManifestOriginRef::File {
            path: origin.to_string(),
        },
    }
}

fn plugin_state_discriminant(
    state: harness_plugin::PluginLifecycleState,
) -> PluginLifecycleStateDiscriminant {
    match state {
        harness_plugin::PluginLifecycleState::Validated => {
            PluginLifecycleStateDiscriminant::Validated
        }
        harness_plugin::PluginLifecycleState::Activating => {
            PluginLifecycleStateDiscriminant::Activating
        }
        harness_plugin::PluginLifecycleState::Activated => {
            PluginLifecycleStateDiscriminant::Activated
        }
        harness_plugin::PluginLifecycleState::Deactivating => {
            PluginLifecycleStateDiscriminant::Deactivating
        }
        harness_plugin::PluginLifecycleState::Deactivated => {
            PluginLifecycleStateDiscriminant::Deactivated
        }
        harness_plugin::PluginLifecycleState::Rejected(_) => {
            PluginLifecycleStateDiscriminant::Rejected
        }
        harness_plugin::PluginLifecycleState::Failed(_) => PluginLifecycleStateDiscriminant::Failed,
        _ => PluginLifecycleStateDiscriminant::Failed,
    }
}

fn rejection_reason(error: &PluginError) -> RejectionReason {
    match error {
        PluginError::SignatureInvalid { details } => RejectionReason::SignatureInvalid {
            details: details.clone(),
        },
        PluginError::UnknownSigner(signer) => RejectionReason::UnknownSigner {
            signer: signer.clone(),
        },
        PluginError::SignerRevoked { signer, revoked_at } => RejectionReason::SignerRevoked {
            signer: signer.clone(),
            revoked_at: *revoked_at,
        },
        PluginError::SlotOccupied { slot, occupant } => RejectionReason::SlotOccupied {
            slot: format!("{slot:?}"),
            occupant: occupant.0.clone(),
        },
        PluginError::DependencyUnsatisfied {
            dependency,
            requirement,
        } => RejectionReason::DependencyUnsatisfied {
            dependency: dependency.clone(),
            requirement: requirement.clone(),
        },
        PluginError::DependencyCycle(cycle) => RejectionReason::DependencyCycle {
            cycle: cycle.clone(),
        },
        PluginError::AdmissionDenied { policy } => RejectionReason::AdmissionDenied {
            policy: policy.clone(),
        },
        PluginError::NamespaceConflict { details } => RejectionReason::NamespaceConflict {
            details: details.clone(),
        },
        PluginError::TrustMismatch {
            declared,
            source_label,
        } => RejectionReason::AdmissionDenied {
            policy: format!("trust mismatch: declared {declared:?}, source {source_label}"),
        },
        PluginError::HarnessVersionIncompatible { required, actual } => {
            RejectionReason::AdmissionDenied {
                policy: format!(
                    "harness version incompatible: required {required}, actual {actual}"
                ),
            }
        }
        PluginError::ActiveDependents(dependents) => RejectionReason::AdmissionDenied {
            policy: format!("active dependents: {dependents:?}"),
        },
        PluginError::InvalidManifest(details) => RejectionReason::NamespaceConflict {
            details: details.clone(),
        },
        PluginError::Registration(error) => RejectionReason::AdmissionDenied {
            policy: error.to_string(),
        },
        PluginError::ActivateFailed(details)
        | PluginError::DeactivateFailed(details)
        | PluginError::Builder(details) => RejectionReason::AdmissionDenied {
            policy: details.clone(),
        },
        PluginError::SignerStore(error) => RejectionReason::AdmissionDenied {
            policy: error.to_string(),
        },
        PluginError::ManifestLoader(ManifestLoaderError::Io(error))
        | PluginError::RuntimeLoader(harness_plugin::RuntimeLoaderError::LoadFailed(error))
        | PluginError::RuntimeLoader(harness_plugin::RuntimeLoaderError::UnsupportedOrigin(
            error,
        )) => RejectionReason::AdmissionDenied {
            policy: error.clone(),
        },
        PluginError::ManifestLoader(ManifestLoaderError::UnsupportedSource(source)) => {
            RejectionReason::AdmissionDenied {
                policy: source.clone(),
            }
        }
        PluginError::ManifestLoader(ManifestLoaderError::Validation(failure)) => {
            RejectionReason::AdmissionDenied {
                policy: failure.details.clone(),
            }
        }
        PluginError::RuntimeLoader(harness_plugin::RuntimeLoaderError::PluginNotFound(name)) => {
            RejectionReason::DependencyUnsatisfied {
                dependency: name.to_string(),
                requirement: "static runtime factory".to_owned(),
            }
        }
    }
}

struct EngineSessionTurnRunner {
    engine: Engine,
    active_conversation_runs: Arc<parking_lot::Mutex<HashMap<RunId, ActiveConversationRun>>>,
    skill_registry: Option<SkillRegistry>,
    skill_metrics_sink: Option<Arc<dyn SkillMetricsSink>>,
    skill_config_snapshot: SkillConfigSnapshot,
}

struct ActiveConversationRunGuard {
    active_conversation_runs: Arc<parking_lot::Mutex<HashMap<RunId, ActiveConversationRun>>>,
    run_id: RunId,
}

impl ActiveConversationRunGuard {
    fn register(
        active_conversation_runs: Arc<parking_lot::Mutex<HashMap<RunId, ActiveConversationRun>>>,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
        cancellation: CancellationToken,
    ) -> Self {
        active_conversation_runs.lock().insert(
            run_id,
            ActiveConversationRun {
                tenant_id,
                session_id,
                cancellation,
            },
        );
        Self {
            active_conversation_runs,
            run_id,
        }
    }
}

impl Drop for ActiveConversationRunGuard {
    fn drop(&mut self) {
        self.active_conversation_runs.lock().remove(&self.run_id);
    }
}

#[async_trait]
impl SessionTurnRunner for EngineSessionTurnRunner {
    async fn run_turn(
        &self,
        ctx: SessionTurnContext,
        prompt: String,
    ) -> Result<Vec<Event>, SessionError> {
        let input = TurnInput {
            message: Message {
                id: ctx.message_id,
                role: MessageRole::User,
                parts: vec![MessagePart::Text(prompt)],
                created_at: harness_contracts::now(),
            },
            metadata: conversation_turn_metadata(
                ctx.turn_index,
                ctx.client_message_id.clone(),
                ctx.attachments.clone(),
            ),
        };
        let cancellation = CancellationToken::new();
        let _active_run = ActiveConversationRunGuard::register(
            Arc::clone(&self.active_conversation_runs),
            ctx.tenant_id,
            ctx.session_id,
            ctx.run_id,
            cancellation.clone(),
        );
        let run_ctx = RunContext::new(ctx.tenant_id, ctx.session_id, ctx.run_id)
            .with_cancellation(cancellation)
            .with_optional_user_id(ctx.user_id.clone())
            .with_optional_team_id(ctx.team_id)
            .with_permission_mode(ctx.permission_mode)
            .with_interactivity(ctx.interactivity)
            .with_config_snapshot(
                ctx.config_snapshot_id,
                ctx.effective_config_hash,
                ctx.started_from_scope_set,
            )
            .with_context_seed(ctx.context_seed.clone());
        let engine = self.engine_with_turn_skill_snapshot()?;
        #[cfg(feature = "steering-queue")]
        let mut engine = engine;
        if let Some(delta) = ctx.pending_deferred_tools_delta.clone() {
            engine
                .context_engine()
                .push_deferred_tools_delta(ctx.tenant_id, ctx.session_id, delta)
                .map_err(|error| SessionError::Message(error.to_string()))?;
        }
        #[cfg(feature = "steering-queue")]
        if let Some(merge) = ctx.steering_merge.clone() {
            engine = engine
                .into_builder()
                .with_steering_drain(Arc::new(PreDrainedSteeringDrain::new(merge)))
                .build()
                .map_err(|error| SessionError::Message(error.to_string()))?;
        }
        let stream = engine
            .run(
                SessionHandle {
                    tenant_id: ctx.tenant_id,
                    session_id: ctx.session_id,
                },
                input,
                run_ctx,
            )
            .await
            .map_err(|error| SessionError::Message(error.to_string()))?;
        Ok(stream.collect().await)
    }

    async fn push_context_patch(&self, request: ContextPatchRequest) -> Result<(), SessionError> {
        self.engine
            .context_engine()
            .push_patch(request)
            .await
            .map_err(|error| SessionError::Message(error.to_string()))
    }
}

fn conversation_turn_metadata(
    turn_index: usize,
    client_message_id: Option<String>,
    attachments: Vec<ConversationAttachmentReference>,
) -> serde_json::Value {
    let mut metadata = json!({ "turn": turn_index });
    if let Some(client_message_id) = client_message_id.filter(|value| is_uuid_v4_like(value)) {
        metadata["clientMessageId"] = json!(client_message_id);
    }
    if !attachments.is_empty() {
        metadata["attachments"] = json!(attachments);
    }
    metadata
}

fn is_uuid_v4_like(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }

    for index in [8, 13, 18, 23] {
        if bytes[index] != b'-' {
            return false;
        }
    }
    if bytes[14] != b'4' || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B') {
        return false;
    }

    bytes
        .iter()
        .enumerate()
        .filter(|(index, _)| !matches!(index, 8 | 13 | 18 | 23))
        .all(|(_, byte)| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod conversation_metadata_tests {
    use super::conversation_turn_metadata;

    #[test]
    fn conversation_turn_metadata_keeps_only_uuid_v4_client_message_ids() {
        let uuid_v4 = "00000000-0000-4000-8000-000000000001";
        let uuid_v1 = "00000000-0000-1000-8000-000000000001";

        assert_eq!(
            conversation_turn_metadata(1, Some(uuid_v4.to_owned()), Vec::new())["clientMessageId"],
            uuid_v4
        );
        assert!(
            conversation_turn_metadata(1, Some(uuid_v1.to_owned()), Vec::new())
                .get("clientMessageId")
                .is_none()
        );
    }
}

impl EngineSessionTurnRunner {
    fn engine_with_turn_skill_snapshot(&self) -> Result<Engine, SessionError> {
        let engine = self.engine.clone();
        let Some(registry) = &self.skill_registry else {
            return Ok(engine);
        };

        let snapshot = registry.snapshot();
        let mut cap_registry = engine.cap_registry().as_ref().clone();
        validate_required_skill_config(&snapshot, &self.skill_config_snapshot)
            .map_err(|error| SessionError::Message(error.to_string()))?;
        let mut renderer = SkillRenderer::new(Arc::new(
            SkillConfigSnapshotResolver::from_registry_snapshot(
                &snapshot,
                self.skill_config_snapshot.clone(),
            ),
        ));
        if let Some(metrics_sink) = &self.skill_metrics_sink {
            renderer = renderer.with_metrics_sink(Arc::clone(metrics_sink));
        }
        let mut service =
            SkillRegistryService::new(registry.clone(), renderer).with_snapshot(snapshot);
        if let Some(metrics_sink) = &self.skill_metrics_sink {
            service = service.with_metrics_sink(Arc::clone(metrics_sink));
        }
        cap_registry.install::<dyn harness_contracts::SkillRegistryCap>(
            ToolCapability::SkillRegistry,
            Arc::new(service),
        );

        engine
            .into_builder()
            .with_cap_registry(Arc::new(cap_registry))
            .build()
            .map_err(|error| SessionError::Message(error.to_string()))
    }
}

#[cfg(feature = "steering-queue")]
struct PreDrainedSteeringDrain {
    merge: parking_lot::Mutex<Option<harness_session::SynthesizedUserMessage>>,
}

#[cfg(feature = "steering-queue")]
impl PreDrainedSteeringDrain {
    fn new(merge: harness_session::SynthesizedUserMessage) -> Self {
        Self {
            merge: parking_lot::Mutex::new(Some(merge)),
        }
    }
}

#[cfg(feature = "steering-queue")]
#[async_trait]
impl SteeringDrain for PreDrainedSteeringDrain {
    async fn drain_and_merge(
        &self,
        _session: &SessionHandle,
        _run_id: RunId,
        _merged_into_message_id: MessageId,
    ) -> Result<Option<SteeringMerge>, harness_contracts::EngineError> {
        let merge = self.merge.lock().take();
        Ok(merge.map(|message| SteeringMerge {
            body: message.body,
            applied_event: message.applied_event,
            already_persisted: true,
        }))
    }
}

fn filter_unavailable_tools(
    snapshot: &ToolRegistrySnapshot,
    cap_registry: &CapabilityRegistry,
) -> ToolPoolFilter {
    let mut filter = ToolPoolFilter::default();
    for descriptor in snapshot.as_descriptors() {
        if descriptor
            .required_capabilities
            .iter()
            .any(|capability| !cap_registry.contains(capability))
        {
            filter.denylist.insert(descriptor.name.clone());
        }
    }
    filter
}

pub fn filter_unrouted_service_tools(
    filter: &mut ToolPoolFilter,
    snapshot: &ToolRegistrySnapshot,
    routes: &ProviderCapabilityRouteSettings,
) {
    for descriptor in snapshot.as_descriptors() {
        let Some(binding) = descriptor.service_binding.as_ref() else {
            continue;
        };
        let routed = routes.routes.iter().any(|route| {
            route.enabled
                && route.kind == binding.route_kind
                && route.provider_id == binding.provider_id
                && route
                    .operation_ids
                    .iter()
                    .any(|operation_id| operation_id == &binding.operation_id)
        });
        if !routed {
            filter.denylist.insert(descriptor.name.clone());
        }
    }
}

fn apply_tenant_tool_filter(filter: &mut ToolPoolFilter, policy: &TenantPolicy) {
    if let Some(allowed_tools) = &policy.allowed_tools {
        filter.allowlist = Some(match filter.allowlist.take() {
            Some(existing) => existing
                .intersection(allowed_tools)
                .cloned()
                .collect::<HashSet<_>>(),
            None => allowed_tools.clone(),
        });
    }
}

#[cfg(feature = "memory-external-slot")]
fn memory_actor_from_options(options: &SessionOptions) -> harness_contracts::MemoryActor {
    harness_contracts::MemoryActor {
        tenant_id: options.tenant_id,
        user_id: options.user_id.clone(),
        team_id: options.team_id,
        session_id: Some(options.session_id),
    }
}

#[cfg(feature = "memory-builtin")]
const BUILTIN_MEMORY_PROMPT_MEMORY_THRESHOLD: usize = 16_000;
#[cfg(feature = "memory-builtin")]
const BUILTIN_MEMORY_PROMPT_USER_THRESHOLD: usize = 8_000;
#[cfg(feature = "memory-builtin")]
const BUILTIN_MEMORY_PROMPT_TOTAL_THRESHOLD: usize =
    BUILTIN_MEMORY_PROMPT_MEMORY_THRESHOLD + BUILTIN_MEMORY_PROMPT_USER_THRESHOLD;
#[cfg(feature = "memory-builtin")]
const BUILTIN_MEMORY_PROMPT_OVERFLOW_THRESHOLD: usize =
    BUILTIN_MEMORY_PROMPT_TOTAL_THRESHOLD + (BUILTIN_MEMORY_PROMPT_TOTAL_THRESHOLD / 2);
#[cfg(feature = "memory-builtin")]
const BUILTIN_MEMORY_PROMPT_HEAD_ONLY_CHARS: usize = 1_024;

#[cfg(feature = "memory-builtin")]
struct RenderedBuiltinMemory {
    prompt: Option<String>,
    overflows: Vec<MemdirOverflowEvent>,
}

#[cfg(feature = "memory-builtin")]
fn render_builtin_memory_system_prompt(
    snapshot: &harness_memory::MemdirSnapshot,
    tenant_id: TenantId,
    session_id: harness_contracts::SessionId,
) -> RenderedBuiltinMemory {
    let mut sections = Vec::new();
    let mut overflows = Vec::new();
    let memory = snapshot.memory.trim();
    let user = snapshot.user.trim();
    let total_chars = memory.chars().count() + user.chars().count();
    let mode = if total_chars > BUILTIN_MEMORY_PROMPT_OVERFLOW_THRESHOLD {
        MemdirPromptTruncationMode::HeadOnly
    } else if total_chars > BUILTIN_MEMORY_PROMPT_TOTAL_THRESHOLD {
        MemdirPromptTruncationMode::LatestSections
    } else {
        MemdirPromptTruncationMode::Full
    };
    if !memory.is_empty() {
        let truncated = truncate_memdir_prompt_file(
            memory,
            BUILTIN_MEMORY_PROMPT_MEMORY_THRESHOLD,
            MemdirFileTag::Memory,
            tenant_id,
            session_id,
            total_chars,
            mode,
        );
        if let Some(event) = truncated.overflow {
            overflows.push(event);
        }
        sections.push(format!("<MEMORY.md>\n{}\n</MEMORY.md>", truncated.content));
    }
    if !user.is_empty() {
        let truncated = truncate_memdir_prompt_file(
            user,
            BUILTIN_MEMORY_PROMPT_USER_THRESHOLD,
            MemdirFileTag::User,
            tenant_id,
            session_id,
            total_chars,
            mode,
        );
        if let Some(event) = truncated.overflow {
            overflows.push(event);
        }
        sections.push(format!("<USER.md>\n{}\n</USER.md>", truncated.content));
    }

    let prompt = if sections.is_empty() {
        None
    } else {
        Some(format!(
            "<builtin-memory>\n{}\n</builtin-memory>",
            sections.join("\n\n")
        ))
    };

    RenderedBuiltinMemory { prompt, overflows }
}

#[cfg(feature = "memory-builtin")]
struct TruncatedMemdirPromptFile {
    content: String,
    overflow: Option<MemdirOverflowEvent>,
}

#[cfg(feature = "memory-builtin")]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum MemdirPromptTruncationMode {
    Full,
    LatestSections,
    HeadOnly,
}

#[cfg(feature = "memory-builtin")]
fn truncate_memdir_prompt_file(
    content: &str,
    threshold: usize,
    file: MemdirFileTag,
    tenant_id: TenantId,
    session_id: harness_contracts::SessionId,
    total_chars: usize,
    mode: MemdirPromptTruncationMode,
) -> TruncatedMemdirPromptFile {
    match mode {
        MemdirPromptTruncationMode::Full => TruncatedMemdirPromptFile {
            content: content.to_owned(),
            overflow: None,
        },
        MemdirPromptTruncationMode::LatestSections => TruncatedMemdirPromptFile {
            content: truncate_by_latest_memdir_sections(content, threshold),
            overflow: None,
        },
        MemdirPromptTruncationMode::HeadOnly => {
            let content = content
                .chars()
                .take(BUILTIN_MEMORY_PROMPT_HEAD_ONLY_CHARS)
                .collect::<String>();
            TruncatedMemdirPromptFile {
                content,
                overflow: Some(MemdirOverflowEvent {
                    session_id,
                    tenant_id,
                    file,
                    current_chars: total_chars as u64,
                    threshold: BUILTIN_MEMORY_PROMPT_OVERFLOW_THRESHOLD as u64,
                    strategy_applied: OverflowStrategy::HeadOnly {
                        kept_chars: BUILTIN_MEMORY_PROMPT_HEAD_ONLY_CHARS as u32,
                    },
                    at: Utc::now(),
                }),
            }
        }
    }
}

#[cfg(feature = "memory-builtin")]
fn truncate_by_latest_memdir_sections(content: &str, threshold: usize) -> String {
    let sections = split_memdir_sections(content);
    if sections.len() <= 1 {
        return content.chars().take(threshold).collect::<String>();
    }

    let mut kept = Vec::new();
    let mut kept_chars = 0_usize;
    for section in sections.iter().rev() {
        let section_chars = section.chars().count();
        let next_len = kept_chars + section_chars;
        if next_len > threshold {
            break;
        }
        kept.push(*section);
        kept_chars = next_len;
    }

    if kept.is_empty() {
        return sections
            .last()
            .copied()
            .unwrap_or(content)
            .chars()
            .take(threshold)
            .collect::<String>();
    }

    kept.reverse();
    let dropped_sections = sections.len().saturating_sub(kept.len());
    format!(
        "[{dropped_sections} sections truncated]\n{}",
        kept.join("").trim()
    )
}

#[cfg(feature = "memory-builtin")]
fn split_memdir_sections(content: &str) -> Vec<&str> {
    let mut starts = content
        .char_indices()
        .filter_map(|(index, ch)| (ch == '§').then_some(index))
        .collect::<Vec<_>>();
    if starts.is_empty() {
        return vec![content];
    }
    if starts[0] != 0 {
        starts.insert(0, 0);
    }

    starts
        .iter()
        .enumerate()
        .map(|(position, start)| {
            let end = starts.get(position + 1).copied().unwrap_or(content.len());
            &content[*start..end]
        })
        .collect()
}

struct LifecycleHookEventStore {
    inner: Arc<dyn EventStore>,
    hooks: HookDispatcher,
    tenant_id: TenantId,
    session_id: harness_contracts::SessionId,
    #[cfg(feature = "memory-external-slot")]
    user_id: Option<String>,
    #[cfg(feature = "memory-external-slot")]
    team_id: Option<harness_contracts::TeamId>,
    workspace_root: PathBuf,
    redactor: Arc<dyn Redactor>,
    session_limits: Arc<SessionLimitState>,
    deleted_conversation_sessions: Arc<parking_lot::Mutex<HashSet<(TenantId, SessionId)>>>,
    summary_state: parking_lot::Mutex<MemorySessionSummaryState>,
    #[cfg(feature = "memory-external-slot")]
    memory_manager: Option<Arc<harness_memory::MemoryManager>>,
}

#[derive(Debug, Default, Clone)]
struct MemorySessionSummaryState {
    turn_count: u32,
    tool_use_count: u32,
    final_assistant_text: Option<String>,
}

struct ConversationDeletionGuardEventStore {
    inner: Arc<dyn EventStore>,
    deleted_conversation_sessions: Arc<parking_lot::Mutex<HashSet<(TenantId, SessionId)>>>,
}

impl ConversationDeletionGuardEventStore {
    fn ensure_not_deleted(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<(), harness_contracts::JournalError> {
        if self
            .deleted_conversation_sessions
            .lock()
            .contains(&(tenant, session_id))
        {
            return Err(harness_contracts::JournalError::Message(format!(
                "conversation session was deleted: {session_id}"
            )));
        }

        Ok(())
    }
}

#[async_trait]
impl EventStore for ConversationDeletionGuardEventStore {
    async fn append(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        events: &[Event],
    ) -> Result<JournalOffset, harness_contracts::JournalError> {
        self.ensure_not_deleted(tenant, session_id)?;
        self.inner.append(tenant, session_id, events).await
    }

    async fn append_with_metadata(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        metadata: AppendMetadata,
        events: &[Event],
    ) -> Result<JournalOffset, harness_contracts::JournalError> {
        self.ensure_not_deleted(tenant, session_id)?;
        self.inner
            .append_with_metadata(tenant, session_id, metadata, events)
            .await
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, harness_contracts::JournalError> {
        self.inner.read_envelopes(tenant, session_id, cursor).await
    }

    async fn query_after(
        &self,
        tenant: TenantId,
        after: Option<harness_contracts::EventId>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, harness_contracts::JournalError> {
        self.inner.query_after(tenant, after, limit).await
    }

    async fn snapshot(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, harness_contracts::JournalError> {
        self.inner.snapshot(tenant, session_id).await
    }

    async fn save_snapshot(
        &self,
        tenant: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<(), harness_contracts::JournalError> {
        self.ensure_not_deleted(tenant, snapshot.session_id)?;
        self.inner.save_snapshot(tenant, snapshot).await
    }

    async fn compact_link(
        &self,
        parent: SessionId,
        child: SessionId,
        reason: harness_contracts::ForkReason,
    ) -> Result<(), harness_contracts::JournalError> {
        self.inner.compact_link(parent, child, reason).await
    }

    async fn delete_session(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<bool, harness_contracts::JournalError> {
        self.inner.delete_session(tenant, session_id).await
    }

    async fn list_sessions(
        &self,
        tenant: TenantId,
        filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, harness_contracts::JournalError> {
        self.inner.list_sessions(tenant, filter).await
    }

    async fn prune(
        &self,
        tenant: TenantId,
        policy: PrunePolicy,
    ) -> Result<PruneReport, harness_contracts::JournalError> {
        self.inner.prune(tenant, policy).await
    }
}

#[async_trait]
impl EventStore for LifecycleHookEventStore {
    async fn append(
        &self,
        tenant: TenantId,
        session_id: harness_contracts::SessionId,
        events: &[Event],
    ) -> Result<JournalOffset, harness_contracts::JournalError> {
        if self
            .deleted_conversation_sessions
            .lock()
            .contains(&(tenant, session_id))
        {
            return Err(harness_contracts::JournalError::Message(format!(
                "conversation session was deleted: {session_id}"
            )));
        }
        let mut combined = events.to_vec();
        combined.extend(self.lifecycle_hook_events(events).await?);
        let result = self.inner.append(tenant, session_id, &combined).await;
        if result.is_ok()
            && events
                .iter()
                .any(|event| matches!(event, Event::SessionEnded(_)))
        {
            self.session_limits.release();
        }
        result
    }

    async fn append_with_metadata(
        &self,
        tenant: TenantId,
        session_id: harness_contracts::SessionId,
        metadata: AppendMetadata,
        events: &[Event],
    ) -> Result<JournalOffset, harness_contracts::JournalError> {
        if self
            .deleted_conversation_sessions
            .lock()
            .contains(&(tenant, session_id))
        {
            return Err(harness_contracts::JournalError::Message(format!(
                "conversation session was deleted: {session_id}"
            )));
        }
        let mut combined = events.to_vec();
        combined.extend(self.lifecycle_hook_events(events).await?);
        let result = self
            .inner
            .append_with_metadata(tenant, session_id, metadata, &combined)
            .await;
        if result.is_ok()
            && events
                .iter()
                .any(|event| matches!(event, Event::SessionEnded(_)))
        {
            self.session_limits.release();
        }
        result
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: harness_contracts::SessionId,
        cursor: ReplayCursor,
    ) -> Result<BoxStream<'static, EventEnvelope>, harness_contracts::JournalError> {
        self.inner.read_envelopes(tenant, session_id, cursor).await
    }

    async fn query_after(
        &self,
        tenant: TenantId,
        after: Option<harness_contracts::EventId>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, harness_contracts::JournalError> {
        self.inner.query_after(tenant, after, limit).await
    }

    async fn snapshot(
        &self,
        tenant: TenantId,
        session_id: harness_contracts::SessionId,
    ) -> Result<Option<SessionSnapshot>, harness_contracts::JournalError> {
        self.inner.snapshot(tenant, session_id).await
    }

    async fn save_snapshot(
        &self,
        tenant: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<(), harness_contracts::JournalError> {
        self.inner.save_snapshot(tenant, snapshot).await
    }

    async fn compact_link(
        &self,
        parent: harness_contracts::SessionId,
        child: harness_contracts::SessionId,
        reason: harness_contracts::ForkReason,
    ) -> Result<(), harness_contracts::JournalError> {
        self.inner.compact_link(parent, child, reason).await
    }

    async fn delete_session(
        &self,
        tenant: TenantId,
        session_id: harness_contracts::SessionId,
    ) -> Result<bool, harness_contracts::JournalError> {
        self.inner.delete_session(tenant, session_id).await
    }

    async fn list_sessions(
        &self,
        tenant: TenantId,
        filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, harness_contracts::JournalError> {
        self.inner.list_sessions(tenant, filter).await
    }

    async fn prune(
        &self,
        tenant: TenantId,
        policy: PrunePolicy,
    ) -> Result<PruneReport, harness_contracts::JournalError> {
        self.inner.prune(tenant, policy).await
    }
}

impl LifecycleHookEventStore {
    async fn lifecycle_hook_events(
        &self,
        events: &[Event],
    ) -> Result<Vec<Event>, harness_contracts::JournalError> {
        let mut output = Vec::new();
        for event in events {
            self.record_memory_summary_event(event);
            match event {
                Event::SessionCreated(created) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::Setup {
                            workspace_root: Some(self.workspace_root.clone()),
                        })
                        .await?,
                    );
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::SessionStart {
                            session_id: created.session_id,
                        })
                        .await?,
                    );
                }
                Event::SessionEnded(ended) => {
                    self.call_memory_session_end(ended).await;
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::SessionEnd {
                            session_id: ended.session_id,
                            reason: ended.reason.clone(),
                        })
                        .await?,
                    );
                }
                Event::SubagentSpawned(spawned) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::SubagentStart {
                            subagent_id: spawned.subagent_id,
                            spec: SubagentSpecView {
                                name: spawned.agent_ref.name.clone(),
                                description: spawned.trigger_tool_name.clone(),
                            },
                        })
                        .await?,
                    );
                }
                Event::SubagentTerminated(terminated) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::SubagentStop {
                            subagent_id: terminated.subagent_id,
                            status: subagent_status_from_reason(&terminated.reason),
                        })
                        .await?,
                    );
                }
                Event::McpElicitationRequested(requested) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::Elicitation {
                            mcp_server_id: requested.server_id.clone(),
                            schema: json!({
                                "subject": &requested.subject,
                                "summary": &requested.schema_summary,
                            }),
                        })
                        .await?,
                    );
                }
                Event::McpConnectionLost(lost) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::Notification {
                            kind: NotificationKind::Warning,
                            body: json!({
                                "kind": "mcp_connection_lost",
                                "server_id": &lost.server_id,
                                "terminal": lost.terminal,
                            }),
                        })
                        .await?,
                    );
                }
                Event::McpConnectionRecovered(recovered) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::Notification {
                            kind: NotificationKind::Info,
                            body: json!({
                                "kind": "mcp_connection_recovered",
                                "server_id": &recovered.server_id,
                                "schema_changed": recovered.schema_changed,
                            }),
                        })
                        .await?,
                    );
                }
                Event::McpToolsListChanged(changed) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::Notification {
                            kind: NotificationKind::Info,
                            body: json!({
                                "kind": "mcp_tools_list_changed",
                                "server_id": &changed.server_id,
                                "added_count": changed.added_count,
                                "removed_count": changed.removed_count,
                            }),
                        })
                        .await?,
                    );
                }
                Event::McpResourceUpdated(updated) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::Notification {
                            kind: NotificationKind::Info,
                            body: json!({
                                "kind": "mcp_resource_updated",
                                "server_id": &updated.server_id,
                                "resource_kind": &updated.kind,
                            }),
                        })
                        .await?,
                    );
                }
                Event::McpSamplingRequested(requested) => {
                    output.extend(
                        self.dispatch_lifecycle_hook(HookEvent::Notification {
                            kind: NotificationKind::Info,
                            body: json!({
                                "kind": "mcp_sampling_requested",
                                "server_id": &requested.server_id,
                                "request_id": requested.request_id,
                                "outcome": &requested.outcome,
                            }),
                        })
                        .await?,
                    );
                }
                _ => {}
            }
        }
        Ok(output)
    }

    async fn dispatch_lifecycle_hook(
        &self,
        event: HookEvent,
    ) -> Result<Vec<Event>, harness_contracts::JournalError> {
        let kind = event.kind();
        let result = self
            .hooks
            .dispatch(event, self.hook_context())
            .await
            .map_err(|error| harness_contracts::JournalError::Message(error.to_string()))?;
        Ok(sdk_hook_events(kind, &result, None))
    }

    fn hook_context(&self) -> HookContext {
        HookContext {
            tenant_id: self.tenant_id,
            session_id: self.session_id,
            run_id: None,
            turn_index: None,
            correlation_id: harness_contracts::CorrelationId::new(),
            causation_id: harness_contracts::CausationId::new(),
            trust_level: TrustLevel::AdminTrusted,
            permission_mode: PermissionMode::Default,
            interactivity: InteractivityLevel::NoInteractive,
            at: harness_contracts::now(),
            view: Arc::new(SdkHookView {
                workspace_root: self.workspace_root.clone(),
                redactor: Arc::clone(&self.redactor),
            }),
            upstream_outcome: None,
            replay_mode: ReplayMode::Live,
        }
    }

    fn record_memory_summary_event(&self, event: &Event) {
        let mut state = self.summary_state.lock();
        record_memory_summary_event(&mut state, event);
    }

    #[cfg(feature = "memory-external-slot")]
    async fn memory_summary_state(&self) -> MemorySessionSummaryState {
        let fallback = self.summary_state.lock().clone();
        let Ok(mut stream) = self
            .inner
            .read_envelopes(self.tenant_id, self.session_id, ReplayCursor::FromStart)
            .await
        else {
            return fallback;
        };
        let mut state = MemorySessionSummaryState::default();
        while let Some(envelope) = stream.next().await {
            record_memory_summary_event(&mut state, &envelope.payload);
        }
        state
    }

    #[cfg(feature = "memory-external-slot")]
    async fn call_memory_session_end(&self, ended: &harness_contracts::SessionEndedEvent) {
        let Some(memory) = &self.memory_manager else {
            return;
        };
        let summary_state = self.memory_summary_state().await;
        let ctx = harness_contracts::MemorySessionCtx {
            tenant_id: ended.tenant_id,
            session_id: ended.session_id,
            workspace_id: None,
            user_id: self.user_id.as_deref(),
            team_id: self.team_id,
        };
        let summary = harness_contracts::SessionSummaryView {
            end_reason: ended.reason.clone(),
            turn_count: summary_state.turn_count,
            tool_use_count: summary_state.tool_use_count,
            usage: ended.final_usage.clone(),
            final_assistant_text: summary_state.final_assistant_text.as_deref(),
        };
        let _ = memory.on_session_end(&ctx, &summary).await;
    }

    #[cfg(not(feature = "memory-external-slot"))]
    async fn call_memory_session_end(&self, _ended: &harness_contracts::SessionEndedEvent) {}
}

fn record_memory_summary_event(state: &mut MemorySessionSummaryState, event: &Event) {
    match event {
        Event::UserMessageAppended(_) => {
            state.turn_count = state.turn_count.saturating_add(1);
        }
        Event::AssistantMessageCompleted(completed) => {
            state.final_assistant_text = message_content_text(&completed.content);
        }
        Event::ToolUseCompleted(_) | Event::ToolUseFailed(_) => {
            state.tool_use_count = state.tool_use_count.saturating_add(1);
        }
        _ => {}
    }
}

fn message_content_text(content: &MessageContent) -> Option<String> {
    match content {
        MessageContent::Text(text) => Some(text.clone()),
        MessageContent::Structured(value) => Some(value.to_string()),
        MessageContent::Multimodal(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| match part {
                    MessagePart::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
    }
}

#[cfg(feature = "memory-external-slot")]
struct SdkMemoryEventSink {
    event_store: Arc<dyn EventStore>,
    tenant_id: TenantId,
    session_id: harness_contracts::SessionId,
}

#[cfg(feature = "memory-external-slot")]
#[async_trait]
impl harness_memory::MemoryEventSink for SdkMemoryEventSink {
    async fn emit(&self, event: Event) {
        let _ = self
            .event_store
            .append(self.tenant_id, self.session_id, &[event])
            .await;
    }

    async fn emit_required(&self, event: Event) -> Result<(), harness_contracts::MemoryError> {
        self.event_store
            .append(self.tenant_id, self.session_id, &[event])
            .await
            .map(|_| ())
            .map_err(|error| harness_contracts::MemoryError::Provider {
                provider: "journal".to_owned(),
                source_message: error.to_string(),
            })
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
struct SdkMemoryMetricsSink {
    observer: Arc<Observer>,
}

struct SdkModelMetricsSink {
    observer: Arc<Observer>,
}

impl ModelMetricsSink for SdkModelMetricsSink {
    fn record_credential_pool_cooldown(&self, model_id: &str) {
        self.observer
            .model_metrics
            .record_credential_pool_cooldown(model_id);
    }

    fn record_aux_queue_wait(&self, model_id: &str, duration: Duration) {
        self.observer
            .model_metrics
            .record_aux_queue_wait(model_id, duration);
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
impl harness_memory::MemoryMetricsSink for SdkMemoryMetricsSink {
    fn record(&self, metric: harness_memory::MemoryMetric) {
        let (name, attrs) = self.attributes(metric);
        let mut span = self.observer.start_span(name, attrs);
        span.set_status(SpanStatus::Ok);
        span.end();
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
impl SdkMemoryMetricsSink {
    fn attributes(&self, metric: harness_memory::MemoryMetric) -> (&'static str, SpanAttributes) {
        match metric {
            harness_memory::MemoryMetric::Recall {
                provider_id,
                outcome,
                duration_ms,
                returned_count,
            } => {
                let mut attrs = SpanAttributes::new()
                    .with(
                        "outcome",
                        AttributeValue::String(memory_recall_outcome(outcome).to_owned()),
                    )
                    .with(
                        "duration_ms",
                        AttributeValue::Int(u64_to_i64(duration_ms.into())),
                    )
                    .with(
                        "returned_count",
                        AttributeValue::Int(u64_to_i64(returned_count.into())),
                    );
                if let Some(provider_id) = provider_id {
                    attrs = attrs.with("provider_id", AttributeValue::String(provider_id));
                }
                ("memory.recall", attrs)
            }
            harness_memory::MemoryMetric::RecallDegraded {
                provider_id,
                reason,
            } => {
                let mut attrs = SpanAttributes::new().with(
                    "reason",
                    AttributeValue::String(self.redact_reason(&reason)),
                );
                if let Some(provider_id) = provider_id {
                    attrs = attrs.with("provider_id", AttributeValue::String(provider_id));
                }
                ("memory.recall.degraded", attrs)
            }
            harness_memory::MemoryMetric::RecallHitRateSample { provider_id, hit } => {
                let mut attrs = SpanAttributes::new().with("hit", AttributeValue::Bool(hit));
                if let Some(provider_id) = provider_id {
                    attrs = attrs.with("provider_id", AttributeValue::String(provider_id));
                }
                ("memory.recall.hit_rate", attrs)
            }
            harness_memory::MemoryMetric::ThreatDetected { category, action } => (
                "memory.threat.detected",
                SpanAttributes::new()
                    .with(
                        "category",
                        AttributeValue::String(threat_category(category).to_owned()),
                    )
                    .with(
                        "action",
                        AttributeValue::String(threat_action(action).to_owned()),
                    ),
            ),
            harness_memory::MemoryMetric::MemdirWrite {
                file,
                action,
                bytes_written,
            } => (
                "memory.memdir.write",
                SpanAttributes::new()
                    .with("file", AttributeValue::String(memdir_file(file).to_owned()))
                    .with(
                        "action",
                        AttributeValue::String(memory_write_action(&action).to_owned()),
                    )
                    .with(
                        "bytes_written",
                        AttributeValue::Int(u64_to_i64(bytes_written)),
                    ),
            ),
            harness_memory::MemoryMetric::MemdirBytes { file, bytes } => (
                "memory.memdir.bytes",
                SpanAttributes::new()
                    .with("file", AttributeValue::String(memdir_file(file).to_owned()))
                    .with("bytes", AttributeValue::Int(u64_to_i64(bytes))),
            ),
            harness_memory::MemoryMetric::MemdirOverflow {
                file,
                current_chars,
                threshold,
            } => (
                "memory.memdir.overflow",
                SpanAttributes::new()
                    .with("file", AttributeValue::String(memdir_file(file).to_owned()))
                    .with(
                        "current_chars",
                        AttributeValue::Int(u64_to_i64(current_chars)),
                    )
                    .with("threshold", AttributeValue::Int(u64_to_i64(threshold))),
            ),
            harness_memory::MemoryMetric::MemdirLockWait { file, waited_ms } => (
                "memory.memdir.lock_wait",
                SpanAttributes::new()
                    .with("file", AttributeValue::String(memdir_file(file).to_owned()))
                    .with(
                        "waited_ms",
                        AttributeValue::Int(u64_to_i64(waited_ms.into())),
                    ),
            ),
            harness_memory::MemoryMetric::MemdirLockFailed { file, retries } => (
                "memory.memdir.lock_failed",
                SpanAttributes::new()
                    .with("file", AttributeValue::String(memdir_file(file).to_owned()))
                    .with("retries", AttributeValue::Int(u64_to_i64(retries.into()))),
            ),
            #[cfg(feature = "memory-consolidation")]
            harness_memory::MemoryMetric::ConsolidationRan {
                hook_id,
                promoted,
                demoted,
            } => (
                "memory.consolidation.ran",
                SpanAttributes::new()
                    .with("hook_id", AttributeValue::String(hook_id))
                    .with("promoted", AttributeValue::Int(u64_to_i64(promoted.into())))
                    .with("demoted", AttributeValue::Int(u64_to_i64(demoted.into()))),
            ),
            harness_memory::MemoryMetric::ExternalProviderConfigured { configured } => (
                "memory.external.configured",
                SpanAttributes::new().with("configured", AttributeValue::Bool(configured)),
            ),
            harness_memory::MemoryMetric::Upsert { kind, visibility } => (
                "memory.upsert",
                SpanAttributes::new()
                    .with(
                        "kind",
                        AttributeValue::String(memory_kind(&kind).to_owned()),
                    )
                    .with(
                        "visibility",
                        AttributeValue::String(memory_visibility(&visibility).to_owned()),
                    ),
            ),
        }
    }

    fn redact_reason(&self, reason: &str) -> String {
        let redacted = self.observer.redactor.redact(
            reason,
            &RedactRules {
                scope: RedactScope::TraceOnly,
                replacement: "[REDACTED]".to_owned(),
                pattern_set: RedactPatternSet::Default,
            },
        );
        truncate_chars(&redacted, 160)
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn memory_recall_outcome(outcome: harness_memory::MemoryRecallMetricOutcome) -> &'static str {
    match outcome {
        harness_memory::MemoryRecallMetricOutcome::Recalled => "recalled",
        harness_memory::MemoryRecallMetricOutcome::Empty => "empty",
        harness_memory::MemoryRecallMetricOutcome::Skipped => "skipped",
        harness_memory::MemoryRecallMetricOutcome::Degraded => "degraded",
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn memdir_file(file: MemdirFileTag) -> &'static str {
    match file {
        MemdirFileTag::Memory => "memory",
        MemdirFileTag::User => "user",
        MemdirFileTag::Dreams => "dreams",
        _ => "unknown",
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn memory_write_action(action: &harness_contracts::MemoryWriteAction) -> &'static str {
    match action {
        harness_contracts::MemoryWriteAction::AppendSection { .. } => "append_section",
        harness_contracts::MemoryWriteAction::ReplaceSection { .. } => "replace_section",
        harness_contracts::MemoryWriteAction::DeleteSection { .. } => "delete_section",
        harness_contracts::MemoryWriteAction::Upsert => "upsert",
        harness_contracts::MemoryWriteAction::Forget => "forget",
        _ => "unknown",
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn memory_kind(kind: &harness_contracts::MemoryKind) -> &'static str {
    match kind {
        harness_contracts::MemoryKind::UserPreference => "user_preference",
        harness_contracts::MemoryKind::Feedback => "feedback",
        harness_contracts::MemoryKind::ProjectFact => "project_fact",
        harness_contracts::MemoryKind::Reference => "reference",
        harness_contracts::MemoryKind::AgentSelfNote => "agent_self_note",
        harness_contracts::MemoryKind::Custom(_) => "custom",
        _ => "unknown",
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn memory_visibility(visibility: &harness_contracts::MemoryVisibility) -> &'static str {
    match visibility {
        harness_contracts::MemoryVisibility::Private { .. } => "private",
        harness_contracts::MemoryVisibility::User { .. } => "user",
        harness_contracts::MemoryVisibility::Team { .. } => "team",
        harness_contracts::MemoryVisibility::Tenant => "tenant",
        _ => "unknown",
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn threat_category(category: harness_contracts::ThreatCategory) -> &'static str {
    match category {
        harness_contracts::ThreatCategory::PromptInjection => "prompt_injection",
        harness_contracts::ThreatCategory::Exfiltration => "exfiltration",
        harness_contracts::ThreatCategory::Backdoor => "backdoor",
        harness_contracts::ThreatCategory::Credential => "credential",
        harness_contracts::ThreatCategory::Malicious => "malicious",
        harness_contracts::ThreatCategory::SpecialToken => "special_token",
        _ => "unknown",
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn threat_action(action: harness_contracts::ThreatAction) -> &'static str {
    match action {
        harness_contracts::ThreatAction::Warn => "warn",
        harness_contracts::ThreatAction::Redact => "redact",
        harness_contracts::ThreatAction::Block => "block",
        _ => "unknown",
    }
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn u64_to_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[derive(Default)]
struct PendingSessionEvents {
    events: parking_lot::Mutex<Vec<Event>>,
}

impl PendingSessionEvents {
    fn push(&self, event: Event) {
        self.events.lock().push(event);
    }

    fn drain(&self) -> Vec<Event> {
        self.events.lock().drain(..).collect()
    }
}

struct BufferedSkillEventSink {
    pending_session_events: Arc<PendingSessionEvents>,
}

#[async_trait]
impl harness_skill::SkillEventSink for BufferedSkillEventSink {
    async fn emit(&self, event: Event) {
        self.pending_session_events.push(event);
    }
}

struct SdkSkillEventSink {
    event_store: Arc<dyn EventStore>,
    tenant_id: TenantId,
    session_id: harness_contracts::SessionId,
}

#[async_trait]
impl harness_skill::SkillEventSink for SdkSkillEventSink {
    async fn emit(&self, event: Event) {
        let _ = self
            .event_store
            .append(self.tenant_id, self.session_id, &[event])
            .await;
    }
}

struct SdkMcpEventSink {
    event_store: Arc<dyn EventStore>,
    tenant_id: TenantId,
    session_id: harness_contracts::SessionId,
}

impl McpEventSink for SdkMcpEventSink {
    fn emit(&self, event: Event) {
        let event_store = Arc::clone(&self.event_store);
        let tenant_id = self.tenant_id;
        let session_id = self.session_id;
        std::thread::spawn(move || {
            futures::executor::block_on(async move {
                let _ = event_store.append(tenant_id, session_id, &[event]).await;
            });
        });
    }
}

struct SdkMcpMetricsSink {
    observer: Arc<Observer>,
}

impl McpMetricsSink for SdkMcpMetricsSink {
    fn record(&self, metric: McpMetric) {
        let (name, attrs) = mcp_metric_attributes(metric);
        let mut span = self.observer.start_span(name, attrs);
        span.set_status(SpanStatus::Ok);
        span.end();
    }
}

fn mcp_metric_attributes(metric: McpMetric) -> (&'static str, SpanAttributes) {
    match metric {
        McpMetric::OAuthRefresh { outcome } => (
            "mcp.oauth.refresh",
            SpanAttributes::new().with("outcome", string_attr_value(outcome.as_str())),
        ),
        McpMetric::ConnectionTotal {
            server_id,
            transport,
            outcome,
        } => (
            "mcp.connection.total",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with("transport", string_attr_value(transport))
                .with("outcome", string_attr_value(outcome.as_str())),
        ),
        McpMetric::ConnectionState { server_id, state } => (
            "mcp.connection.state",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with("state", string_attr_value(mcp_state_label(state))),
        ),
        McpMetric::ReconnectAttempt {
            server_id,
            attempt,
            outcome,
        } => (
            "mcp.reconnect.attempt",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with("attempt", AttributeValue::Int(i64::from(attempt)))
                .with("outcome", string_attr_value(outcome.as_str())),
        ),
        McpMetric::ToolInvocation { server_id, outcome } => (
            "mcp.tool.invocation",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with("outcome", string_attr_value(outcome.as_str())),
        ),
        McpMetric::ToolFilterSkipped { server_id, reason } => (
            "mcp.tool_filter.skipped",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with("reason", string_attr_value(reason)),
        ),
        McpMetric::ListChanged {
            server_id,
            disposition,
        } => (
            "mcp.list.changed",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with("disposition", string_attr_value(format!("{disposition:?}"))),
        ),
        McpMetric::ResourceUpdated { server_id, kind } => (
            "mcp.resource.updated",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with("kind", string_attr_value(format!("{kind:?}"))),
        ),
        McpMetric::SamplingRequested { outcome } => (
            "mcp.sampling.requested",
            SpanAttributes::new().with("outcome", string_attr_value(outcome.as_str())),
        ),
        McpMetric::SamplingInputTokens { server_id, amount } => (
            "mcp.sampling.input_tokens",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with(
                    "amount",
                    AttributeValue::Int(amount.try_into().unwrap_or(i64::MAX)),
                ),
        ),
        McpMetric::SamplingOutputTokens { server_id, amount } => (
            "mcp.sampling.output_tokens",
            SpanAttributes::new()
                .with("server_id", string_attr_value(server_id.0))
                .with(
                    "amount",
                    AttributeValue::Int(amount.try_into().unwrap_or(i64::MAX)),
                ),
        ),
        McpMetric::ServerRequest { method, outcome } => (
            "mcp.server.request",
            SpanAttributes::new()
                .with("method", string_attr_value(method))
                .with("outcome", string_attr_value(outcome.as_str())),
        ),
        McpMetric::ServerThrottled { capability } => (
            "mcp.server.throttled",
            SpanAttributes::new().with("capability", string_attr_value(capability)),
        ),
        McpMetric::ServerTenantIsolationRejected => (
            "mcp.server.tenant_isolation.rejected",
            SpanAttributes::new(),
        ),
    }
}

fn string_attr_value(value: impl Into<String>) -> AttributeValue {
    AttributeValue::String(value.into())
}

fn mcp_state_label(state: McpMetricConnectionState) -> &'static str {
    match state {
        McpMetricConnectionState::Connecting => "connecting",
        McpMetricConnectionState::Ready => "ready",
        McpMetricConnectionState::Reconnecting => "reconnecting",
        McpMetricConnectionState::Failed => "failed",
        McpMetricConnectionState::Closed => "closed",
    }
}

struct SdkSkillMetricsSink {
    observer: Arc<Observer>,
}

impl SkillMetricsSink for SdkSkillMetricsSink {
    fn skill_loaded(&self, source: &str) {
        self.record("skill.loaded", "source", source);
    }

    fn skill_rejected(&self, reason: &str) {
        self.record("skill.rejected", "reason", reason);
    }

    fn skill_render_duration_ms(&self, duration_ms: u64) {
        let mut span = self.observer.start_span(
            "skill.render",
            SpanAttributes::new().with(
                "duration_ms",
                AttributeValue::Int(duration_ms.min(i64::MAX as u64) as i64),
            ),
        );
        span.set_status(SpanStatus::Ok);
        span.end();
    }

    fn skill_invocation(&self, skill_name: &str) {
        self.record(
            "skill.invocation",
            "skill_ref",
            &safe_skill_metric_label(skill_name),
        );
    }

    fn skill_view(&self, skill_name: &str) {
        self.record(
            "skill.view",
            "skill_ref",
            &safe_skill_metric_label(skill_name),
        );
    }

    fn skill_shell_invocation(&self, command: &str) {
        self.record(
            "skill.shell.invocation",
            "command_kind",
            &safe_skill_metric_label(command),
        );
    }

    fn skill_shell_blocked(&self, command: &str) {
        self.record(
            "skill.shell.blocked",
            "command_kind",
            &safe_skill_metric_label(command),
        );
    }

    fn skill_threat_detected(&self, category: &str) {
        self.record("skill.threat.detected", "category", category);
    }

    fn skill_prerequisite_missing(&self, skill_name: &str) {
        self.record(
            "skill.prerequisite.missing",
            "skill_ref",
            &safe_skill_metric_label(skill_name),
        );
    }

    fn skill_prerequisite_advisory(&self, skill_name: &str) {
        self.record(
            "skill.prerequisite.advisory",
            "skill_ref",
            &safe_skill_metric_label(skill_name),
        );
    }
}

impl SdkSkillMetricsSink {
    fn record(&self, name: &str, key: &str, value: &str) {
        let mut span = self.observer.start_span(
            name,
            SpanAttributes::new().with(key, AttributeValue::String(value.to_owned())),
        );
        span.set_status(SpanStatus::Ok);
        span.end();
    }
}

fn safe_skill_metric_label(value: &str) -> String {
    let mut label = value
        .chars()
        .map(|character| match character {
            'a'..='z' | '0'..='9' => character,
            'A'..='Z' => character.to_ascii_lowercase(),
            '-' | '.' | '/' | ':' | ' ' => '_',
            '_' => '_',
            _ => '_',
        })
        .take(48)
        .collect::<String>();
    while label.contains("__") {
        label = label.replace("__", "_");
    }
    let label = label.trim_matches('_').to_owned();
    if label.is_empty() {
        "unknown".to_owned()
    } else {
        label
    }
}

struct SdkSkillReloadCap {
    inner: Arc<HarnessInner>,
}

fn runtime_skill_summary(
    skill: &Skill,
    status: harness_contracts::SkillStatus,
) -> RuntimeSkillSummary {
    RuntimeSkillSummary {
        name: skill.name.clone(),
        description: skill.description.clone(),
        tags: skill.frontmatter.tags.clone(),
        category: skill.frontmatter.category.clone(),
        source: skill.source.to_kind(),
        status,
    }
}

fn runtime_skill_view(
    skill: &Skill,
    status: harness_contracts::SkillStatus,
    full: bool,
) -> RuntimeSkillView {
    RuntimeSkillView {
        summary: runtime_skill_summary(skill, status),
        parameters: skill
            .frontmatter
            .parameters
            .iter()
            .map(|parameter| RuntimeSkillParameter {
                name: parameter.name.clone(),
                param_type: skill_param_type_name(parameter.param_type).to_owned(),
                required: parameter.required,
                default: parameter.default.clone(),
                description: parameter.description.clone(),
            })
            .collect(),
        config_keys: skill
            .frontmatter
            .config
            .iter()
            .map(|config| config.key.clone())
            .collect(),
        body_preview: skill.body.chars().take(1024).collect(),
        body_full: full.then(|| skill.body.clone()),
    }
}

fn skill_param_type_name(param_type: SkillParamType) -> &'static str {
    match param_type {
        SkillParamType::String => "string",
        SkillParamType::Number => "number",
        SkillParamType::Boolean => "boolean",
        SkillParamType::Path => "path",
        SkillParamType::Url => "url",
    }
}

fn sdk_current_skill_platform() -> SkillPlatform {
    #[cfg(target_os = "macos")]
    {
        SkillPlatform::Macos
    }
    #[cfg(target_os = "linux")]
    {
        SkillPlatform::Linux
    }
    #[cfg(target_os = "windows")]
    {
        SkillPlatform::Windows
    }
}

#[async_trait]
impl SkillReloadCap for SdkSkillReloadCap {
    async fn reload_skills(&self, registrations: &[SkillRegistration]) -> Result<(), String> {
        let validator = self.skill_validator();
        let mut validated = Vec::with_capacity(registrations.len());
        for registration in registrations {
            let skill = validator
                .validate_registration(registration)
                .await
                .map_err(|error| error.to_string())?;
            validated.push(SkillRegistration {
                skill,
                force_allowlist: registration.force_allowlist.clone(),
            });
        }

        let candidate = self
            .inner
            .skill_registry
            .candidate_snapshot(&validated)
            .map_err(|error| error.to_string())?;
        let bindings = self
            .inner
            .skill_registry
            .hook_bindings_in_snapshot(&candidate);
        let mut registered = Vec::<String>::new();

        for binding in bindings {
            if self
                .inner
                .hook_registry
                .origin_for(&binding.handler_id)
                .is_some()
            {
                continue;
            }
            let handler_id = binding.handler_id.clone();
            let handler = match skill_hook_handler(binding) {
                Ok(handler) => handler,
                Err(error) => {
                    for registered_id in registered {
                        self.inner.hook_registry.deregister(&registered_id);
                    }
                    return Err(error.to_string());
                }
            };
            if let Err(error) = self.inner.hook_registry.register(handler) {
                for registered_id in registered {
                    self.inner.hook_registry.deregister(&registered_id);
                }
                return Err(error.to_string());
            }
            registered.push(handler_id);
        }

        self.inner.skill_registry.commit_snapshot(candidate);
        Ok(())
    }
}

impl SdkSkillReloadCap {
    fn skill_validator(&self) -> SkillValidator {
        let mut validator = self
            .inner
            .skill_loader
            .as_ref()
            .map(SkillLoader::validator)
            .unwrap_or_default();
        if let Some(observer) = &self.inner.observer {
            validator = validator.with_metrics_sink(Arc::new(SdkSkillMetricsSink {
                observer: Arc::clone(observer),
            }));
        }
        validator
    }
}

struct SkillDeclaredHookHandler {
    handler_id: String,
    events: Vec<HookEventKind>,
}

#[async_trait]
impl HookHandler for SkillDeclaredHookHandler {
    fn handler_id(&self) -> &str {
        &self.handler_id
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &self.events
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        Ok(HookOutcome::Continue)
    }
}

fn skill_hook_handler(binding: SkillHookBinding) -> Result<Box<dyn HookHandler>, HarnessError> {
    validate_skill_hook_binding(&binding)?;
    match binding.transport {
        SkillHookTransport::Builtin(BuiltinHookKind::AuditLog) => {
            Ok(Box::new(SkillDeclaredHookHandler {
                handler_id: binding.handler_id,
                events: binding.events,
            }))
        }
        SkillHookTransport::Exec(spec) => {
            let handler = ExecHookTransport::new(HookExecSpec {
                handler_id: binding.handler_id,
                interested_events: binding.events,
                failure_mode: spec.failure_mode,
                command: spec.command,
                args: spec.args,
                env: Default::default(),
                working_dir: WorkingDir::SessionWorkspace,
                timeout: Duration::from_millis(spec.timeout_ms),
                resource_limits: HookExecResourceLimits::default(),
                signal_policy: HookExecSignalPolicy::default(),
                protocol_version: HookProtocolVersion::V1,
                trust: binding.source.trust_level(),
            })
            .map_err(HarnessError::Hook)?;
            Ok(Box::new(handler))
        }
        SkillHookTransport::Http(spec) => {
            let url = spec
                .url
                .parse()
                .map_err(|error| harness_contracts::HookError::Message(format!("{error}")))?;
            let handler = HttpHookTransport::new(HookHttpSpec {
                handler_id: binding.handler_id,
                interested_events: binding.events,
                failure_mode: spec.failure_mode,
                url,
                auth: HookHttpAuth::None,
                timeout: Duration::from_millis(spec.timeout_ms),
                security: HookHttpSecurityPolicy {
                    allowlist: HostAllowlist::from_hosts(spec.security.allowlist),
                    ssrf_guard: skill_ssrf_guard_policy(spec.security.ssrf_guard),
                    max_redirects: spec.security.max_redirects,
                    max_body_bytes: spec.security.max_body_bytes,
                    mtls: None,
                },
                protocol_version: HookProtocolVersion::V1,
                trust: binding.source.trust_level(),
            })
            .map_err(HarnessError::Hook)?;
            Ok(Box::new(handler))
        }
    }
}

fn skill_ssrf_guard_policy(enabled: bool) -> SsrfGuardPolicy {
    if enabled {
        return SsrfGuardPolicy::default();
    }
    SsrfGuardPolicy {
        deny_loopback: false,
        deny_private: false,
        deny_link_local: false,
        deny_metadata: false,
    }
}

fn validate_skill_hook_binding(binding: &SkillHookBinding) -> Result<(), HarnessError> {
    let denied = match (&binding.source, &binding.transport) {
        (SkillSource::Mcp(_), _) => true,
        (_, SkillHookTransport::Builtin(_)) => false,
        (SkillSource::Bundled, SkillHookTransport::Exec(_) | SkillHookTransport::Http(_)) => false,
        (
            SkillSource::Plugin {
                trust: TrustLevel::AdminTrusted,
                ..
            },
            SkillHookTransport::Exec(_) | SkillHookTransport::Http(_),
        ) => false,
        (_, SkillHookTransport::Exec(_) | SkillHookTransport::Http(_)) => true,
    };
    if denied {
        return Err(HarnessError::Hook(
            harness_contracts::HookError::Unauthorized(format!(
                "skill hook transport not permitted for trust={:?}",
                binding.source.trust_level()
            )),
        ));
    }
    Ok(())
}

fn subagent_status_from_reason(
    reason: &harness_contracts::SubagentTerminationReason,
) -> harness_contracts::SubagentStatus {
    match reason {
        harness_contracts::SubagentTerminationReason::NaturalCompletion => {
            harness_contracts::SubagentStatus::Completed
        }
        harness_contracts::SubagentTerminationReason::ParentCancelled
        | harness_contracts::SubagentTerminationReason::AdminInterrupted { .. } => {
            harness_contracts::SubagentStatus::Cancelled
        }
        harness_contracts::SubagentTerminationReason::Stalled { .. } => {
            harness_contracts::SubagentStatus::Stalled
        }
        harness_contracts::SubagentTerminationReason::BridgeBroken
        | harness_contracts::SubagentTerminationReason::Failed { .. } => {
            harness_contracts::SubagentStatus::Failed
        }
        _ => harness_contracts::SubagentStatus::Failed,
    }
}

struct SdkHookView {
    workspace_root: PathBuf,
    redactor: Arc<dyn Redactor>,
}

impl HookSessionView for SdkHookView {
    fn workspace_root(&self) -> Option<&Path> {
        Some(&self.workspace_root)
    }

    fn recent_messages(&self, _limit: usize) -> Vec<HookMessageView> {
        Vec::new()
    }

    fn permission_mode(&self) -> PermissionMode {
        PermissionMode::Default
    }

    fn redacted(&self) -> &dyn Redactor {
        self.redactor.as_ref()
    }

    fn current_tool_descriptor(&self) -> Option<ToolDescriptorView> {
        None
    }
}

#[cfg(feature = "tool-search")]
#[derive(Clone)]
struct SdkToolSearchRuntime {
    tools: ToolPool,
    model_caps: Arc<harness_model::ConversationModelCapability>,
    mcp_config: Option<McpConfig>,
    event_store: Arc<dyn EventStore>,
    hooks: HookDispatcher,
    tenant_id: TenantId,
    session_id: harness_contracts::SessionId,
    redactor: Arc<dyn Redactor>,
}

#[cfg(feature = "tool-search")]
impl SdkToolSearchRuntime {
    async fn emit_hook_events(
        &self,
        kind: harness_contracts::HookEventKind,
        result: &DispatchResult,
    ) -> Result<(), harness_contracts::ToolError> {
        for event in sdk_hook_events(kind, result, None) {
            self.event_store
                .append(self.tenant_id, self.session_id, &[event])
                .await
                .map_err(|error| harness_contracts::ToolError::Internal(error.to_string()))?;
        }
        Ok(())
    }
}

#[cfg(feature = "tool-search")]
#[async_trait]
impl harness_tool_search::ToolSearchRuntimeCap for SdkToolSearchRuntime {
    async fn snapshot(
        &self,
    ) -> Result<harness_tool_search::ToolSearchRuntimeSnapshot, harness_contracts::ToolError> {
        let loaded_tool_names = loaded_tool_names(&self.tools);
        let pending_mcp_servers = match &self.mcp_config {
            Some(config) => config
                .registry
                .pending_mcp_servers_for_tool_search(&config.server_ids_to_inject)
                .await
                .into_iter()
                .map(|server_id| server_id.0)
                .collect(),
            None => Vec::new(),
        };
        Ok(harness_tool_search::ToolSearchRuntimeSnapshot {
            deferred_tools: self
                .tools
                .deferred()
                .iter()
                .filter(|tool| !loaded_tool_names.contains(&tool.descriptor().name))
                .map(|tool| tool.descriptor().clone())
                .collect(),
            loaded_tool_names,
            discovered_tool_names: BTreeSet::new(),
            pending_mcp_servers,
            model_caps: Arc::clone(&self.model_caps),
            reload_handle: Some(Arc::new(SdkToolSearchReloadHandle {
                tools: self.tools.clone(),
            })),
        })
    }

    async fn emit_event(&self, event: Event) -> Result<(), harness_contracts::ToolError> {
        self.event_store
            .append(self.tenant_id, self.session_id, &[event])
            .await
            .map(|_| ())
            .map_err(|error| harness_contracts::ToolError::Internal(error.to_string()))
    }

    async fn dispatch_pre_tool_search_hook(
        &self,
        ctx: &harness_tool::ToolContext,
        tool_use_id: harness_contracts::ToolUseId,
        query: &str,
        query_kind: harness_contracts::ToolSearchQueryKind,
    ) -> Result<harness_tool_search::ToolSearchPreHookOutcome, harness_contracts::ToolError> {
        let result = self
            .hooks
            .dispatch(
                HookEvent::PreToolSearch {
                    tool_use_id,
                    query: query.to_owned(),
                    query_kind,
                },
                tool_search_hook_context(ctx, Arc::clone(&self.redactor)),
            )
            .await
            .map_err(|error| harness_contracts::ToolError::Internal(error.to_string()))?;
        self.emit_hook_events(harness_contracts::HookEventKind::PreToolSearch, &result)
            .await?;
        match result.final_outcome {
            HookOutcome::Continue => Ok(harness_tool_search::ToolSearchPreHookOutcome::Continue),
            HookOutcome::Block { reason } => {
                Ok(harness_tool_search::ToolSearchPreHookOutcome::Block { reason })
            }
            HookOutcome::RewriteInput(value) => Ok(
                harness_tool_search::ToolSearchPreHookOutcome::RewriteInput(value),
            ),
            _ => Ok(harness_tool_search::ToolSearchPreHookOutcome::Continue),
        }
    }

    async fn dispatch_post_tool_search_hook(
        &self,
        ctx: &harness_tool::ToolContext,
        tool_use_id: harness_contracts::ToolUseId,
        materialized: Vec<harness_contracts::ToolName>,
        backend: harness_contracts::ToolLoadingBackendName,
        cache_impact: harness_contracts::CacheImpact,
    ) -> Result<(), harness_contracts::ToolError> {
        let result = self
            .hooks
            .dispatch(
                HookEvent::PostToolSearchMaterialize {
                    tool_use_id,
                    materialized,
                    backend,
                    cache_impact,
                },
                tool_search_hook_context(ctx, Arc::clone(&self.redactor)),
            )
            .await
            .map_err(|error| harness_contracts::ToolError::Internal(error.to_string()))?;
        self.emit_hook_events(
            harness_contracts::HookEventKind::PostToolSearchMaterialize,
            &result,
        )
        .await
    }
}

#[cfg(feature = "tool-search")]
struct SdkToolSearchReloadHandle {
    tools: ToolPool,
}

#[cfg(feature = "tool-search")]
#[async_trait]
impl harness_tool_search::ReloadHandle for SdkToolSearchReloadHandle {
    async fn reload_with_add_tools(
        &self,
        tools: Vec<harness_contracts::ToolName>,
    ) -> Result<CacheImpact, HarnessError> {
        let materialized = self.tools.materialize_deferred_tools(&tools);
        if materialized.len() != tools.len() {
            let missing = tools
                .into_iter()
                .find(|tool| !materialized.contains(tool))
                .unwrap_or_else(|| "unknown".to_owned());
            return Err(HarnessError::ToolNotFound(missing));
        }
        Ok(CacheImpact {
            prompt_cache_invalidated: true,
            reason: Some("tool_search_inline_reinjection".to_owned()),
        })
    }
}

#[cfg(feature = "tool-search")]
fn tool_search_hook_context(
    ctx: &harness_tool::ToolContext,
    redactor: Arc<dyn Redactor>,
) -> HookContext {
    HookContext {
        tenant_id: ctx.tenant_id,
        session_id: ctx.session_id,
        run_id: Some(ctx.run_id),
        turn_index: None,
        correlation_id: ctx.correlation_id,
        causation_id: harness_contracts::CausationId::new(),
        trust_level: TrustLevel::AdminTrusted,
        permission_mode: PermissionMode::Default,
        interactivity: InteractivityLevel::NoInteractive,
        at: harness_contracts::now(),
        view: Arc::new(ToolSearchHookView {
            workspace_root: ctx.workspace_root.clone(),
            redactor,
        }),
        upstream_outcome: None,
        replay_mode: ReplayMode::Live,
    }
}

#[cfg(feature = "tool-search")]
struct ToolSearchHookView {
    workspace_root: PathBuf,
    redactor: Arc<dyn Redactor>,
}

#[cfg(feature = "tool-search")]
impl HookSessionView for ToolSearchHookView {
    fn workspace_root(&self) -> Option<&Path> {
        Some(&self.workspace_root)
    }

    fn recent_messages(&self, _limit: usize) -> Vec<HookMessageView> {
        Vec::new()
    }

    fn permission_mode(&self) -> PermissionMode {
        PermissionMode::Default
    }

    fn redacted(&self) -> &dyn Redactor {
        self.redactor.as_ref()
    }

    fn current_tool_descriptor(&self) -> Option<ToolDescriptorView> {
        None
    }
}

fn sdk_hook_events(
    kind: harness_contracts::HookEventKind,
    result: &DispatchResult,
    fail_closed_denied: Option<harness_contracts::EventId>,
) -> Vec<Event> {
    let mut events = Vec::with_capacity(result.trail.len() + result.failures.len());
    for record in &result.trail {
        events.push(Event::HookTriggered(
            harness_contracts::HookTriggeredEvent {
                hook_event_kind: kind.clone(),
                handler_id: record.handler_id.clone(),
                outcome_summary: hook_outcome_summary(&record.outcome),
                duration_ms: hook_duration_ms(record.duration),
                at: harness_contracts::now(),
            },
        ));
    }
    for failure in &result.failures {
        let causation_id = harness_contracts::EventId::new();
        events.push(Event::HookFailed(harness_contracts::HookFailedEvent {
            hook_event_kind: kind.clone(),
            handler_id: failure.handler_id.clone(),
            failure_mode: failure.mode,
            cause_kind: failure.cause_kind,
            cause_detail: hook_failure_detail(&failure.cause),
            duration_ms: hook_duration_ms(failure.duration),
            fail_closed_denied,
            at: harness_contracts::now(),
        }));
        match &failure.cause {
            HookFailureCause::Unsupported {
                kind: returned_kind,
            } => events.push(Event::HookReturnedUnsupported(
                harness_contracts::HookReturnedUnsupportedEvent {
                    hook_event_kind: kind.clone(),
                    handler_id: failure.handler_id.clone(),
                    returned_kind: returned_kind.clone(),
                    causation_id,
                    at: harness_contracts::now(),
                },
            )),
            HookFailureCause::Inconsistent { reason } => {
                events.push(Event::HookOutcomeInconsistent(
                    harness_contracts::HookOutcomeInconsistentEvent {
                        hook_event_kind: kind.clone(),
                        handler_id: failure.handler_id.clone(),
                        reason: reason.clone(),
                        causation_id,
                        at: harness_contracts::now(),
                    },
                ));
            }
            _ => {}
        }
    }
    events
}

fn hook_outcome_summary(outcome: &HookOutcome) -> harness_contracts::HookOutcomeSummary {
    match outcome {
        HookOutcome::Continue => harness_contracts::HookOutcomeSummary {
            continued: true,
            blocked_reason: None,
            rewrote_input: false,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: false,
        },
        HookOutcome::Block { reason } => harness_contracts::HookOutcomeSummary {
            continued: false,
            blocked_reason: Some(reason.clone()),
            rewrote_input: false,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: false,
        },
        HookOutcome::PreToolUse(outcome) => harness_contracts::HookOutcomeSummary {
            continued: outcome.is_continue(),
            blocked_reason: outcome.block.clone(),
            rewrote_input: outcome.rewrite_input.is_some(),
            overrode_permission: outcome.override_permission.clone(),
            added_context_bytes: outcome
                .additional_context
                .as_ref()
                .map(|context| context.content.len() as u64),
            transformed: false,
        },
        HookOutcome::RewriteInput(_) => harness_contracts::HookOutcomeSummary {
            continued: false,
            blocked_reason: None,
            rewrote_input: true,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: false,
        },
        HookOutcome::OverridePermission(decision) => harness_contracts::HookOutcomeSummary {
            continued: false,
            blocked_reason: None,
            rewrote_input: false,
            overrode_permission: Some(decision.clone()),
            added_context_bytes: None,
            transformed: false,
        },
        HookOutcome::AddContext(context) => harness_contracts::HookOutcomeSummary {
            continued: false,
            blocked_reason: None,
            rewrote_input: false,
            overrode_permission: None,
            added_context_bytes: Some(context.content.len() as u64),
            transformed: false,
        },
        HookOutcome::Transform(_) => harness_contracts::HookOutcomeSummary {
            continued: false,
            blocked_reason: None,
            rewrote_input: false,
            overrode_permission: None,
            added_context_bytes: None,
            transformed: true,
        },
    }
}

fn hook_failure_detail(cause: &HookFailureCause) -> String {
    match cause {
        HookFailureCause::Unsupported { kind } => format!("unsupported outcome: {kind:?}"),
        HookFailureCause::Inconsistent { reason } => format!("inconsistent outcome: {reason:?}"),
        HookFailureCause::Panicked { snippet } => snippet.clone(),
        HookFailureCause::Timeout => "timeout".to_owned(),
        HookFailureCause::Transport { kind, detail } => format!("{kind:?}: {detail}"),
        HookFailureCause::Unauthorized { capability } => format!("unauthorized: {capability}"),
    }
}

fn hook_duration_ms(duration: std::time::Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(feature = "tool-search")]
fn loaded_tool_names(tools: &ToolPool) -> BTreeSet<String> {
    tools
        .prompt_visible_descriptors()
        .into_iter()
        .map(|descriptor| descriptor.name)
        .collect()
}

async fn default_permission_broker(
    options: &HarnessOptions,
    rule_providers: &[Arc<dyn RuleProvider>],
    decision_persistence: Option<Arc<dyn DecisionPersistence>>,
) -> Result<Arc<dyn PermissionBroker>, HarnessError> {
    #[cfg(feature = "rule-engine-permission")]
    {
        if !rule_providers.is_empty() {
            let mut builder = harness_permission::RuleEngineBroker::builder()
                .with_tenant(options.tenant_policy.id);
            for provider in rule_providers {
                builder = builder.with_rule_provider(Arc::clone(provider));
            }
            if let Some(persistence) = decision_persistence {
                builder = builder.with_persistence(persistence);
            }
            return builder
                .build()
                .await
                .map(|broker| Arc::new(broker) as Arc<dyn PermissionBroker>)
                .map_err(HarnessError::Permission);
        }
    }

    #[cfg(not(feature = "rule-engine-permission"))]
    {
        if !rule_providers.is_empty() {
            return Err(HarnessError::PermissionDenied(
                "rule providers require the `rule-engine-permission` feature".to_owned(),
            ));
        }
    }

    let _ = (options, rule_providers, decision_persistence);
    Ok(Arc::new(DenyAllPermissionBroker))
}

async fn policy_gated_permission_broker(
    options: &HarnessOptions,
    broker: Arc<dyn PermissionBroker>,
    rule_providers: &[Arc<dyn RuleProvider>],
) -> Result<Arc<dyn PermissionBroker>, HarnessError> {
    #[cfg(feature = "rule-engine-permission")]
    {
        if !rule_providers.is_empty() {
            let mut builder = harness_permission::RuleEngineBroker::builder()
                .with_tenant(options.tenant_policy.id)
                .policy_deny_only();
            for provider in rule_providers {
                builder = builder.with_rule_provider(Arc::clone(provider));
            }
            let policy_gate = builder.build().await.map_err(HarnessError::Permission)?;
            return Ok(Arc::new(PolicyGatedPermissionBroker {
                policy_gate: Arc::new(policy_gate),
                inner: broker,
            }));
        }
    }

    #[cfg(not(feature = "rule-engine-permission"))]
    {
        if !rule_providers.is_empty() {
            return Err(HarnessError::PermissionDenied(
                "rule providers require the `rule-engine-permission` feature".to_owned(),
            ));
        }
    }

    let _ = (options, rule_providers);
    Ok(broker)
}

#[cfg(feature = "rule-engine-permission")]
struct PolicyGatedPermissionBroker {
    policy_gate: Arc<dyn PermissionBroker>,
    inner: Arc<dyn PermissionBroker>,
}

#[cfg(feature = "rule-engine-permission")]
#[async_trait]
impl PermissionBroker for PolicyGatedPermissionBroker {
    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        if self.hard_policy_denies(&request, &ctx).await {
            return Decision::DenyOnce;
        }

        match self.policy_gate.decide(request.clone(), ctx.clone()).await {
            Decision::Escalate => self.inner.decide(request, ctx).await,
            decision => decision,
        }
    }

    async fn hard_policy_denies(
        &self,
        request: &PermissionRequest,
        ctx: &PermissionContext,
    ) -> bool {
        self.policy_gate.hard_policy_denies(request, ctx).await
            || self.inner.hard_policy_denies(request, ctx).await
            || harness_permission::hard_policy_denies_from_context(request, ctx)
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.inner.persist(decision).await
    }
}

struct DenyAllPermissionBroker;

#[async_trait]
impl PermissionBroker for DenyAllPermissionBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::DenyOnce
    }

    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Ok(())
    }
}

fn compiled_features() -> Vec<&'static str> {
    let mut features = Vec::new();
    push_feature(
        &mut features,
        "sqlite-store",
        cfg!(feature = "sqlite-store"),
    );
    push_feature(&mut features, "jsonl-store", cfg!(feature = "jsonl-store"));
    push_feature(
        &mut features,
        "in-memory-store",
        cfg!(feature = "in-memory-store"),
    );
    push_feature(&mut features, "blob-file", cfg!(feature = "blob-file"));
    push_feature(&mut features, "blob-sqlite", cfg!(feature = "blob-sqlite"));
    push_feature(
        &mut features,
        "provider-openai",
        cfg!(feature = "provider-openai"),
    );
    push_feature(
        &mut features,
        "provider-anthropic",
        cfg!(feature = "provider-anthropic"),
    );
    push_feature(
        &mut features,
        "provider-gemini",
        cfg!(feature = "provider-gemini"),
    );
    push_feature(
        &mut features,
        "provider-openrouter",
        cfg!(feature = "provider-openrouter"),
    );
    push_feature(
        &mut features,
        "provider-bedrock",
        cfg!(feature = "provider-bedrock"),
    );
    push_feature(
        &mut features,
        "provider-codex",
        cfg!(feature = "provider-codex"),
    );
    push_feature(
        &mut features,
        "provider-local-llama",
        cfg!(feature = "provider-local-llama"),
    );
    push_feature(
        &mut features,
        "provider-deepseek",
        cfg!(feature = "provider-deepseek"),
    );
    push_feature(
        &mut features,
        "provider-minimax",
        cfg!(feature = "provider-minimax"),
    );
    push_feature(
        &mut features,
        "provider-qwen",
        cfg!(feature = "provider-qwen"),
    );
    push_feature(
        &mut features,
        "provider-doubao",
        cfg!(feature = "provider-doubao"),
    );
    push_feature(
        &mut features,
        "provider-zhipu",
        cfg!(feature = "provider-zhipu"),
    );
    push_feature(&mut features, "provider-km", cfg!(feature = "provider-km"));
    push_feature(
        &mut features,
        "local-sandbox",
        cfg!(feature = "local-sandbox"),
    );
    push_feature(
        &mut features,
        "docker-sandbox",
        cfg!(feature = "docker-sandbox"),
    );
    push_feature(&mut features, "ssh-sandbox", cfg!(feature = "ssh-sandbox"));
    push_feature(
        &mut features,
        "noop-sandbox",
        cfg!(feature = "noop-sandbox"),
    );
    push_feature(&mut features, "mcp-stdio", cfg!(feature = "mcp-stdio"));
    push_feature(&mut features, "mcp-http", cfg!(feature = "mcp-http"));
    push_feature(
        &mut features,
        "mcp-websocket",
        cfg!(feature = "mcp-websocket"),
    );
    push_feature(&mut features, "mcp-sse", cfg!(feature = "mcp-sse"));
    push_feature(
        &mut features,
        "mcp-in-process",
        cfg!(feature = "mcp-in-process"),
    );
    push_feature(
        &mut features,
        "mcp-server-adapter",
        cfg!(feature = "mcp-server-adapter"),
    );
    push_feature(
        &mut features,
        "interactive-permission",
        cfg!(feature = "interactive-permission"),
    );
    push_feature(
        &mut features,
        "stream-permission",
        cfg!(feature = "stream-permission"),
    );
    push_feature(
        &mut features,
        "rule-engine-permission",
        cfg!(feature = "rule-engine-permission"),
    );
    push_feature(
        &mut features,
        "memory-builtin",
        cfg!(feature = "memory-builtin"),
    );
    push_feature(
        &mut features,
        "memory-external-slot",
        cfg!(feature = "memory-external-slot"),
    );
    push_feature(
        &mut features,
        "agents-subagent",
        cfg!(feature = "agents-subagent"),
    );
    push_feature(&mut features, "agents-team", cfg!(feature = "agents-team"));
    push_feature(
        &mut features,
        "observability-replay",
        cfg!(feature = "observability-replay"),
    );
    push_feature(
        &mut features,
        "observability-otel",
        cfg!(feature = "observability-otel"),
    );
    push_feature(
        &mut features,
        "observability-prometheus",
        cfg!(feature = "observability-prometheus"),
    );
    push_feature(
        &mut features,
        "observability-redactor",
        cfg!(feature = "observability-redactor"),
    );
    push_feature(
        &mut features,
        "plugin-dynamic-load",
        cfg!(feature = "plugin-dynamic-load"),
    );
    push_feature(
        &mut features,
        "plugin-manifest-sign",
        cfg!(feature = "plugin-manifest-sign"),
    );
    push_feature(
        &mut features,
        "builtin-toolset",
        cfg!(feature = "builtin-toolset"),
    );
    push_feature(&mut features, "tool-search", cfg!(feature = "tool-search"));
    push_feature(
        &mut features,
        "tool-loading-anthropic",
        cfg!(feature = "tool-loading-anthropic"),
    );
    push_feature(
        &mut features,
        "tool-loading-inline",
        cfg!(feature = "tool-loading-inline"),
    );
    push_feature(
        &mut features,
        "tool-search-default-scorer",
        cfg!(feature = "tool-search-default-scorer"),
    );
    push_feature(
        &mut features,
        "programmatic-tool-calling",
        cfg!(feature = "programmatic-tool-calling"),
    );
    push_feature(
        &mut features,
        "steering-queue",
        cfg!(feature = "steering-queue"),
    );
    push_feature(&mut features, "testing", cfg!(feature = "testing"));
    features
}

fn push_feature(features: &mut Vec<&'static str>, name: &'static str, enabled: bool) {
    if enabled {
        features.push(name);
    }
}

#[cfg(all(test, not(feature = "rule-engine-permission")))]
mod no_rule_engine_permission_tests {
    use super::*;

    struct NoopRuleProvider;

    #[async_trait]
    impl RuleProvider for NoopRuleProvider {
        fn provider_id(&self) -> &str {
            "noop-rule-provider"
        }

        fn source(&self) -> harness_contracts::RuleSource {
            harness_contracts::RuleSource::Workspace
        }

        async fn resolve_rules(
            &self,
            _tenant: TenantId,
        ) -> Result<Vec<harness_permission::PermissionRule>, PermissionError> {
            Ok(Vec::new())
        }

        fn watch(&self) -> Option<BoxStream<'static, harness_permission::RulesUpdated>> {
            None
        }
    }

    struct AllowPermissionBroker;

    #[async_trait]
    impl PermissionBroker for AllowPermissionBroker {
        async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
            Decision::AllowOnce
        }

        async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn default_permission_broker_rejects_rule_providers_without_rule_engine_feature() {
        let providers: Vec<Arc<dyn RuleProvider>> = vec![Arc::new(NoopRuleProvider)];
        let result = default_permission_broker(&HarnessOptions::default(), &providers, None).await;

        match result {
            Err(HarnessError::PermissionDenied(message)) => {
                assert!(message.contains("rule-engine-permission"));
            }
            Err(error) => panic!("expected permission denied, got {error}"),
            Ok(_) => panic!("rule providers should fail closed without rule-engine-permission"),
        }
    }

    #[tokio::test]
    async fn policy_gated_permission_broker_rejects_rule_providers_without_rule_engine_feature() {
        let providers: Vec<Arc<dyn RuleProvider>> = vec![Arc::new(NoopRuleProvider)];
        let broker: Arc<dyn PermissionBroker> = Arc::new(AllowPermissionBroker);
        let result =
            policy_gated_permission_broker(&HarnessOptions::default(), broker, &providers).await;

        match result {
            Err(HarnessError::PermissionDenied(message)) => {
                assert!(message.contains("rule-engine-permission"));
            }
            Err(error) => panic!("expected permission denied, got {error}"),
            Ok(_) => panic!("rule providers should fail closed without rule-engine-permission"),
        }
    }
}

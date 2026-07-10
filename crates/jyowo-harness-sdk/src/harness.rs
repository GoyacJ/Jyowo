#[cfg(feature = "tool-search")]
use std::collections::BTreeSet;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
#[cfg(feature = "memory-provider-registry")]
use std::sync::atomic::AtomicBool;
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
#[cfg(feature = "sqlite-store")]
use harness_contracts::BlobError;
#[cfg(feature = "tool-search")]
use harness_contracts::CacheImpact;
#[cfg(any(feature = "memory-builtin", feature = "memory-provider-registry"))]
use harness_contracts::MemdirFileTag;
#[cfg(not(feature = "observability-redactor"))]
use harness_contracts::RedactPatternKind;
#[cfg(feature = "agents-team")]
use harness_contracts::{
    AgentId, BlobMeta, Recipient, TeamCreatedEvent, TeamMemberJoinedEvent, TeamTaskUpdatedEvent,
    TeamTerminationReason, TopologyKind,
};
use harness_contracts::{
    BlobReaderCapAdapter, BlobRef, BlobRetention, BlobStore, BlobWriterCapAdapter,
    CapabilityRegistry, ContextPatchRequest, ContextPatchSinkCap, ConversationAttachmentReference,
    ConversationContextReference, ConversationCursor, ConversationEventRef, ConversationTurnInput,
    Decision, Event, EventId, EvidenceRedactionState, EvidenceRefId, EvidenceRefKind, HarnessError,
    HookEventKind, InteractivityLevel, JournalOffset, ManifestOriginRef,
    ManifestValidationFailedEvent, McpServerId, MemoryId, Message, MessageContent, MessageId,
    MessagePart, MessageRole, ModelModality, ModelProtocol, PermissionError, PermissionMode,
    PluginCapabilitiesSummary, PluginFailedEvent, PluginLifecycleStateDiscriminant,
    PluginLoadedEvent, PluginRejectedEvent, ProviderCapabilityRouteSettings, RedactPatternSet,
    RedactRules, RedactScope, Redactor, RejectionReason, RunId, RunModelSnapshot,
    RunScopedProcessRegistryCap, RuntimeExecutionStatus, SessionError, SessionId, TenantId,
    ToolCapability, ToolProfile, ToolRuntimeStatus, ToolSearchMode, TrustLevel, TurnInput,
    WorkspaceAccess, RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY,
};
#[cfg(feature = "sqlite-store")]
use harness_contracts::{
    ConversationSnapshot, ConversationSummary, ConversationTimelinePage, ConversationTurnCursor,
    ConversationWorktreePage,
};
#[cfg(feature = "stream-permission")]
use harness_contracts::{PermissionOptionId, RequestId};
#[cfg(any(feature = "agents-team", feature = "agents-subagent"))]
use harness_contracts::{
    ToolDescriptor, ToolError, ToolGroup, ToolOrigin, ToolProperties, ToolResult,
};
use harness_engine::{
    CancellationToken, Engine, EngineRunner, InterruptCause, RunContext, SessionHandle,
};
#[cfg(feature = "steering-queue")]
use harness_engine::{SteeringDrain, SteeringMerge};
use harness_execution::{AuthorizationEventSink, ExecutionError};
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
    EventStoreOffloadedBlobAuthorizer, EvidenceRefRecord, EvidenceRefSource, PrunePolicy,
    PruneReport, RedactionProvenance, ReplayCursor, SessionFilter, SessionSnapshot, SessionSummary,
};
use harness_mcp::{
    ElicitationHandler, McpEventSink, McpMetric, McpMetricConnectionState, McpMetricsSink,
    McpRegistry, SamplingProvider, SamplingRequest, SamplingResponse, StreamElicitationHandler,
};
#[cfg(feature = "mcp-server-adapter")]
use harness_mcp::{ExposedCapability, HarnessMcpBackend, McpServerError, McpServerRequestContext};
#[cfg(feature = "memory-provider-registry")]
use harness_memory::MemoryExtractor;
use harness_memory::MemoryProvider;
use harness_model::ModelRuntimeSnapshot;
#[cfg(feature = "memory-provider-registry")]
use harness_model::ProviderRequestContext;
use harness_model::{
    AuxModelProvider, ContentDelta, InferContext, InferMiddleware, ModelMetricsSink, ModelProvider,
    ModelRequest, ModelStreamEvent,
};
#[cfg(feature = "observability-redactor")]
use harness_observability::DefaultRedactor;
use harness_observability::{AttributeValue, Observer, SpanAttributes, SpanStatus, Tracer};
use harness_permission::{
    DecisionPersistence, DecisionStore, PermissionBroker, PermissionContext, PermissionRequest,
    PersistedDecision, RuleProvider,
};
#[cfg(feature = "stream-permission")]
use harness_permission::{PendingPermissionRequest, ResolverHandle};
use harness_plugin::{
    ManifestLoaderError, ManifestOrigin, ManifestRecord, PluginCapabilityRegistries, PluginError,
    PluginEventSink,
};
use harness_provider_state::ProviderContinuationStore;
use harness_sandbox::{ExecSpec, SandboxBackend, StdioSpec};
#[cfg(feature = "agents-team")]
use harness_session::WorkspaceBootstrap;
use harness_session::{
    run_effective_config_hash, session_options_hash, Session, SessionOptions, SessionProjection,
    SessionTurnContext, SessionTurnRunner, SkillReloadCap, Workspace, WorkspaceRegistry,
    WorkspaceSpec,
};
use harness_skill::{
    parse_skill_markdown, BuiltinHookKind, DirectorySourceKind, Skill, SkillHookBinding,
    SkillHookTransport, SkillLoader, SkillMetricsSink, SkillParamType, SkillPlatform,
    SkillRegistration, SkillRegistry, SkillRegistryService, SkillRenderer, SkillSource,
    SkillSourceConfig, SkillThreatEventScope, SkillValidator,
};
use harness_tool::{
    DefaultRunScopedProcessRegistry, SchemaResolverContext, ToolPool, ToolPoolFilter,
    ToolPoolModelProfile, ToolRegistry, ToolRegistrySnapshot,
};
#[cfg(any(feature = "agents-team", feature = "agents-subagent"))]
use harness_tool::{PermissionCheck, Tool, ToolContext, ToolEvent, ToolStream, ValidationError};
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
#[cfg(feature = "memory-builtin")]
use crate::system_prompt::render_builtin_memory_system_prompt;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ArtifactContentEvidenceRef {
    pub artifact_id: String,
    pub revision_id: String,
    pub content_ref: EvidenceRefId,
}
use crate::system_prompt::{
    build_runtime_prompt_context, effective_prompt_inputs_hash, runtime_prompt_context_hash,
    workspace_instruction_section, EffectiveSystemPromptInputs, RuntimePromptContext,
    SystemPromptBuilder,
};

mod accessors;
mod conversation;
mod events;
mod limits;
#[cfg(feature = "mcp-server-adapter")]
mod mcp_server;
mod memory;
#[cfg(feature = "memory-provider-registry")]
mod memory_preview;
mod metrics;
mod permissions;
mod plugins;
mod read_model;
mod redaction;
mod run_state;
mod sampling;
mod session_runtime;
mod skills;
#[cfg(feature = "agents-team")]
mod team_runtime;
mod tool_pool;
mod types;
mod workspace;

#[cfg(feature = "stream-permission")]
pub use self::permissions::StreamPermissionRuntime;
pub use self::sampling::HarnessSamplingProvider;
pub use self::tool_pool::filter_unrouted_service_tools;
pub use self::types::{
    ConversationEventsPage, ConversationEventsPageRequest, ConversationRunOptions,
    ConversationSession, ConversationSessionSummary, ConversationTurnReceipt,
    ConversationTurnRequest, HarnessOptions, McpConfig, RuntimeSkillParameter, RuntimeSkillSummary,
    RuntimeSkillView, TenantPolicy,
};
pub use self::workspace::WorkspaceCreateRequest;
pub use crate::agent_runtime::AgentCapabilityResolutionContext;

#[cfg(feature = "tool-search")]
use self::events::sdk_hook_events;
use self::events::{
    ConversationDeletionGuardEventStore, LifecycleHookEventStore, MemorySessionSummaryState,
    PendingSessionEvents,
};
use self::limits::SessionLimitState;
use self::memory::record_memory_summary_event;
use self::metrics::{SdkMcpEventSink, SdkMcpMetricsSink};
use self::permissions::{permission_authority_runtime, PermissionAuthorityBroker};
#[cfg(feature = "memory-provider-registry")]
use self::redaction::default_hook_redactor;
use self::redaction::redact_business_event_for_display;
use self::run_state::{
    ActiveConversationRun, ActiveConversationRunGuard, ActiveConversationSessionGuard,
    EngineSessionTurnRunner,
};
use self::session_runtime::{sdk_session_not_found, snapshot_for_supported_model};
use self::skills::SdkSkillReloadCap;
use self::tool_pool::{apply_tenant_tool_filter, filter_unavailable_tools};
#[derive(Clone)]
pub struct Harness {
    inner: Arc<HarnessInner>,
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
    permission_authority: Arc<harness_permission::PermissionAuthority>,
    authorization_service: Arc<harness_execution::AuthorizationService>,
    tool_registry: ToolRegistry,
    hook_registry: HookRegistry,
    memory_providers: Vec<Arc<dyn MemoryProvider>>,
    #[cfg(feature = "memory-provider-registry")]
    memory_database_path: PathBuf,
    #[cfg(feature = "memory-provider-registry")]
    _memory_extraction_runtime: Option<MemoryExtractionRuntime>,
    #[cfg(feature = "memory-builtin")]
    builtin_memory: Option<BuiltinMemoryConfig>,
    blob_store: Option<Arc<dyn BlobStore>>,
    evidence_ref_store: Option<Arc<harness_journal::EvidenceRefStore>>,
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
    active_conversation_sessions: Arc<parking_lot::Mutex<HashMap<(TenantId, SessionId), RunId>>>,
    #[cfg(feature = "agents-team")]
    active_run_teams: Arc<parking_lot::Mutex<HashMap<RunId, Arc<crate::team::Team>>>>,
    deleted_conversation_sessions: Arc<parking_lot::Mutex<HashSet<(TenantId, SessionId)>>>,
    provider_capability_routes: Arc<parking_lot::RwLock<ProviderCapabilityRouteSettings>>,
    provider_continuation_store: Option<Arc<dyn ProviderContinuationStore>>,
}

struct SdkAuthorizationEventSink {
    event_store: Arc<dyn EventStore>,
    redactor: Arc<dyn Redactor>,
}

#[cfg(feature = "sqlite-store")]
fn artifact_content_blob_from_event(
    event: &Event,
    expected_session_id: SessionId,
) -> Option<(String, String, RunId, BlobRef)> {
    match event {
        Event::ArtifactCreated(event) if event.session_id == expected_session_id => Some((
            event.artifact_id.clone(),
            event.revision_id.to_string(),
            event.run_id,
            event.blob_ref.clone()?,
        )),
        Event::ArtifactUpdated(event) if event.session_id == expected_session_id => Some((
            event.artifact_id.clone(),
            event.revision_id.to_string(),
            event.run_id,
            event.blob_ref.clone()?,
        )),
        _ => None,
    }
}

#[cfg(feature = "sqlite-store")]
fn artifact_content_evidence_record(
    conversation_id: &str,
    envelope: &EventEnvelope,
    conversation_sequence: u64,
    artifact_id: String,
    revision_id: String,
    run_id: RunId,
    content_type: String,
    bytes: &[u8],
    redaction_state: EvidenceRedactionState,
) -> EvidenceRefRecord {
    let hash = blake3::hash(bytes);
    EvidenceRefRecord {
        id: sanitized_artifact_content_evidence_ref_id(envelope.event_id, hash.as_bytes()),
        kind: EvidenceRefKind::ArtifactContent,
        conversation_id: conversation_id.to_owned(),
        run_id: run_id.to_string(),
        source_event_refs: vec![ConversationEventRef {
            event_id: envelope.event_id.to_string(),
            cursor: ConversationCursor {
                event_id: envelope.event_id,
                conversation_sequence,
            },
        }],
        artifact_id: Some(artifact_id),
        revision_id: Some(revision_id),
        content_type,
        byte_length: bytes.len() as u64,
        content_hash: hash.as_bytes().to_vec(),
        redaction_state,
        redaction_provenance: RedactionProvenance {
            redactor_version: "event-redacted-v1".to_owned(),
        },
        retention: BlobRetention::TenantScoped,
        source: EvidenceRefSource::JournalPayload {
            event_id: envelope.event_id.to_string(),
            json_pointer: String::new(),
        },
    }
}

#[cfg(feature = "sqlite-store")]
fn sanitized_artifact_content_evidence_ref_id(event_id: EventId, hash: &[u8; 32]) -> EvidenceRefId {
    let digest = blake3::hash(
        format!(
            "artifact-content-redacted:{event_id}:{}",
            hash.iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
        .as_bytes(),
    );
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    EvidenceRefId::new(EventId::from_u128(u128::from_be_bytes(bytes)).to_string())
}

#[cfg(feature = "sqlite-store")]
fn artifact_content_type(blob_ref: &BlobRef) -> String {
    blob_ref
        .content_type
        .clone()
        .unwrap_or_else(|| "application/octet-stream".to_owned())
}

#[cfg(feature = "sqlite-store")]
fn redaction_state_for_text(original: &str, redacted: &str) -> EvidenceRedactionState {
    if original == redacted {
        EvidenceRedactionState::Clean
    } else {
        EvidenceRedactionState::Redacted
    }
}

#[cfg(feature = "sqlite-store")]
async fn redacted_artifact_content_bytes(
    blob_store: &dyn BlobStore,
    redactor: &dyn Redactor,
    tenant: TenantId,
    blob_ref: &BlobRef,
    expected_session_id: SessionId,
) -> Result<Option<(Vec<u8>, EvidenceRedactionState)>, HarnessError> {
    let meta = match blob_store.head(tenant, blob_ref).await {
        Ok(Some(meta)) => meta,
        Ok(None) | Err(BlobError::NotFound(_)) => return Ok(None),
        Err(error) => {
            return Err(HarnessError::Other(format!(
                "artifact content blob head failed: {error}"
            )))
        }
    };
    if meta.size != blob_ref.size || meta.content_hash != blob_ref.content_hash {
        return Err(HarnessError::Other(
            "artifact content blob metadata mismatch".to_owned(),
        ));
    }
    match meta.retention {
        BlobRetention::TenantScoped => {}
        BlobRetention::SessionScoped(session_id) if session_id == expected_session_id => {}
        _ => return Ok(None),
    }
    let mut stream = match blob_store.get(tenant, blob_ref).await {
        Ok(stream) => stream,
        Err(BlobError::NotFound(_)) => return Ok(None),
        Err(error) => {
            return Err(HarnessError::Other(format!(
                "artifact content blob read failed: {error}"
            )))
        }
    };
    let mut bytes = Vec::with_capacity(blob_ref.size as usize);
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk);
    }
    if bytes.len() as u64 != blob_ref.size {
        return Err(HarnessError::Other(
            "artifact content blob length mismatch".to_owned(),
        ));
    }
    match String::from_utf8(bytes) {
        Ok(content) => {
            let redacted = redactor.redact(&content, &RedactRules::default());
            let redaction_state = redaction_state_for_text(&content, &redacted);
            Ok(Some((redacted.into_bytes(), redaction_state)))
        }
        Err(error) => Ok(Some((error.into_bytes(), EvidenceRedactionState::Clean))),
    }
}

#[cfg(feature = "sqlite-store")]
fn event_projects_to_conversation_timeline(event: &Event) -> bool {
    matches!(
        event,
        Event::RunStarted(_)
            | Event::RunEnded(_)
            | Event::UserMessageAppended(_)
            | Event::AssistantDeltaProduced(_)
            | Event::AssistantMessageCompleted(_)
            | Event::AssistantReviewRequested(_)
            | Event::AssistantClarificationRequested(_)
            | Event::AssistantNotice(_)
            | Event::ArtifactCreated(_)
            | Event::ArtifactUpdated(_)
            | Event::ToolUseRequested(_)
            | Event::ToolUseApproved(_)
            | Event::ToolUseStarted(_)
            | Event::ToolUseDenied(_)
            | Event::ToolUseCompleted(_)
            | Event::ToolUseFailed(_)
            | Event::PermissionRequested(_)
            | Event::PermissionResolved(_)
            | Event::ContextBudgetExceeded(_)
            | Event::ContextStageTransitioned(_)
            | Event::ContextPatchApplied(_)
            | Event::SubagentSpawned(_)
            | Event::SubagentAnnounced(_)
            | Event::SubagentTerminated(_)
            | Event::SubagentStalled(_)
            | Event::TeamCreated(_)
            | Event::TeamMemberJoined(_)
            | Event::TeamMemberLeft(_)
            | Event::TeamMemberStalled(_)
            | Event::AgentMessageSent(_)
            | Event::AgentMessageRouted(_)
            | Event::TeamTurnCompleted(_)
            | Event::TeamTaskUpdated(_)
            | Event::TeamTerminated(_)
            | Event::BackgroundAgentStarted(_)
            | Event::BackgroundAgentStateChanged(_)
            | Event::BackgroundAgentInputRequested(_)
            | Event::BackgroundAgentInputSubmitted(_)
            | Event::BackgroundAgentPermissionRequested(_)
            | Event::BackgroundAgentPermissionResolved(_)
            | Event::BackgroundAgentCancelled(_)
            | Event::BackgroundAgentCompleted(_)
            | Event::BackgroundAgentFailed(_)
            | Event::BackgroundAgentInterrupted(_)
            | Event::BackgroundAgentArchived(_)
            | Event::BackgroundAgentDeleted(_)
            | Event::EngineFailed(_)
    )
}

#[cfg(feature = "memory-provider-registry")]
struct MemoryExtractionRuntime {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[cfg(feature = "memory-provider-registry")]
struct ModelBackedMemoryExtractor {
    model: Arc<dyn ModelProvider>,
    model_id: String,
    protocol: ModelProtocol,
    redactor: Arc<dyn Redactor>,
}

#[cfg(feature = "memory-provider-registry")]
impl ModelBackedMemoryExtractor {
    fn new(
        model: Arc<dyn ModelProvider>,
        model_id: String,
        protocol: ModelProtocol,
        redactor: Arc<dyn Redactor>,
    ) -> Self {
        Self {
            model,
            model_id,
            protocol,
            redactor,
        }
    }
}

#[cfg(feature = "memory-provider-registry")]
impl MemoryExtractor for ModelBackedMemoryExtractor {
    fn extract(
        &self,
        job: &harness_memory::ExtractionJob,
    ) -> Result<harness_memory::ExtractionOutput, String> {
        let Some(excerpt) = job
            .source_excerpt
            .as_deref()
            .filter(|excerpt| !excerpt.trim().is_empty())
        else {
            return Ok(harness_memory::ExtractionOutput::default());
        };

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("build extraction runtime: {error}"))?;
        let excerpt = redact_memory_extraction_excerpt(excerpt, self.redactor.as_ref());
        if excerpt.trim().is_empty() {
            return Ok(harness_memory::ExtractionOutput::default());
        }

        let raw = runtime.block_on(infer_memory_extraction(
            Arc::clone(&self.model),
            self.model_id.clone(),
            self.protocol,
            job,
            &excerpt,
        ))?;
        parse_extraction_output(&raw)
    }
}

#[cfg(feature = "memory-provider-registry")]
fn redact_memory_extraction_excerpt(excerpt: &str, redactor: &dyn Redactor) -> String {
    let rules = memory_extraction_redact_rules();
    let redacted = redactor.redact(excerpt, &rules);
    default_hook_redactor().redact(&redacted, &rules)
}

#[cfg(feature = "memory-provider-registry")]
fn memory_extraction_redact_rules() -> RedactRules {
    RedactRules {
        scope: RedactScope::All,
        replacement: "[REDACTED]".to_owned(),
        pattern_set: RedactPatternSet::Default,
    }
}

#[cfg(feature = "memory-provider-registry")]
async fn infer_memory_extraction(
    model: Arc<dyn ModelProvider>,
    model_id: String,
    protocol: ModelProtocol,
    job: &harness_memory::ExtractionJob,
    excerpt: &str,
) -> Result<String, String> {
    let request = ModelRequest {
        model_id,
        messages: vec![Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(format!(
                "Extract durable memory candidates from this completed session excerpt.\n\
Return only JSON matching this shape:\n\
{{\"candidates\":[{{\"kind\":\"project_fact|user_preference|reference|feedback|agent_self_note\",\"visibility\":\"tenant|user\",\"content\":\"...\",\"confidence\":0.0}}],\"consolidations\":[],\"summary\":null}}\n\
Use only facts supported by the excerpt. Do not include secrets. If nothing should be remembered, return {{\"candidates\":[],\"consolidations\":[],\"summary\":null}}.\n\n\
Session: {}\nRun: {}\nExcerpt:\n{}",
                job.session_id, job.run_id, excerpt
            ))],
            created_at: Utc::now(),
        }],
        tools: None,
        system: Some(
            "You extract long-term memory candidates for Jyowo. Output strict JSON only."
                .to_owned(),
        ),
        temperature: Some(0.0),
        max_tokens: Some(1200),
        stream: true,
        cache_breakpoints: Vec::new(),
        protocol,
        extra: json!({ "source": "memory_extraction" }),
        options: harness_contracts::ModelRequestOptions::default(),
        provider_context: ProviderRequestContext::default(),
    };
    let mut context = InferContext::for_test();
    context.tenant_id = job.tenant_id;
    context.session_id = Some(job.session_id);
    context.run_id = Some(job.run_id);
    context.suppress_usage_accounting = true;

    let mut stream = model
        .infer(request, context)
        .await
        .map_err(|error| format!("memory extraction infer failed: {error}"))?;
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ModelStreamEvent::ContentBlockDelta {
                delta: ContentDelta::Text(delta),
                ..
            } => text.push_str(&delta),
            ModelStreamEvent::StreamError { error, .. } => {
                return Err(format!("memory extraction stream failed: {error}"));
            }
            _ => {}
        }
    }
    Ok(text)
}

#[cfg(feature = "memory-provider-registry")]
fn parse_extraction_output(raw: &str) -> Result<harness_memory::ExtractionOutput, String> {
    if let Ok(output) = serde_json::from_str::<harness_memory::ExtractionOutput>(raw) {
        return Ok(output);
    }

    let trimmed = raw.trim();
    let fenced = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim);
    if let Some(json) = fenced {
        if let Ok(output) = serde_json::from_str::<harness_memory::ExtractionOutput>(json) {
            return Ok(output);
        }
    }

    let Some(start) = trimmed.find('{') else {
        return Err("memory extraction output did not contain JSON".to_owned());
    };
    let Some(end) = trimmed.rfind('}') else {
        return Err("memory extraction output did not contain a complete JSON object".to_owned());
    };
    serde_json::from_str::<harness_memory::ExtractionOutput>(&trimmed[start..=end])
        .map_err(|error| format!("parse memory extraction output: {error}"))
}

#[cfg(feature = "memory-provider-registry")]
impl MemoryExtractionRuntime {
    fn spawn(
        memory_database_path: PathBuf,
        tenant_id: TenantId,
        extractor: Arc<dyn MemoryExtractor>,
        observer: Option<Arc<Observer>>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            let db_path = memory_database_path.to_string_lossy().to_string();
            while !worker_stop.load(Ordering::SeqCst) {
                if let Err(error) =
                    poll_memory_extraction_once(&db_path, tenant_id, Arc::clone(&extractor))
                {
                    record_memory_extraction_poll_error(observer.as_deref(), &error);
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

#[cfg(feature = "memory-provider-registry")]
impl Drop for MemoryExtractionRuntime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(feature = "memory-provider-registry")]
fn poll_memory_extraction_once(
    db_path: &str,
    tenant_id: TenantId,
    extractor: Arc<dyn MemoryExtractor>,
) -> Result<(), String> {
    let settings = harness_memory::settings::MemorySettingsStore::open(db_path)?;
    let global = settings.get_global(tenant_id)?;
    let inbox = harness_memory::MemoryInbox::open(db_path, tenant_id)?;
    let worker = harness_memory::ExtractionWorker::open(
        db_path,
        harness_memory::ExtractionWorkerConfig::default(),
        harness_memory::MemoryPolicyEngine::new(global),
        inbox,
        extractor,
    )?;
    worker
        .poll_and_process("sdk-memory-extraction-worker", true, u64::MAX, false)
        .map(|_| ())
}

#[cfg(feature = "memory-provider-registry")]
fn record_memory_extraction_poll_error(observer: Option<&Observer>, error: &str) {
    let Some(observer) = observer else {
        return;
    };
    let rules = memory_extraction_redact_rules();
    let redacted = observer.redactor.redact(error, &rules);
    let redacted = default_hook_redactor().redact(&redacted, &rules);
    let attrs = SpanAttributes::new()
        .with(
            "component",
            AttributeValue::String("memory_extraction_runtime".to_owned()),
        )
        .with("outcome", AttributeValue::String("error".to_owned()));
    let mut span = observer.start_span("memory.extraction.poll", attrs);
    span.add_event(
        "memory.extraction.poll.error",
        SpanAttributes::new().with("error", AttributeValue::String(redacted.clone())),
    );
    span.set_status(SpanStatus::Error(redacted));
    span.end();
}

#[async_trait]
impl AuthorizationEventSink for SdkAuthorizationEventSink {
    async fn emit_batch(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        events: Vec<Event>,
    ) -> Result<(), ExecutionError> {
        let events = events
            .into_iter()
            .map(|event| redact_business_event_for_display(event, self.redactor.as_ref()))
            .collect::<Vec<_>>();
        self.event_store
            .append(tenant_id, session_id, &events)
            .await
            .map(|_| ())
            .map_err(|error| ExecutionError::EventSinkFailed {
                reason: error.to_string(),
            })
    }
}

fn process_tool_status_spec() -> ExecSpec {
    ExecSpec {
        command: "true".to_owned(),
        stdin: StdioSpec::Null,
        stdout: StdioSpec::Null,
        stderr: StdioSpec::Null,
        workspace_access: WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: Vec::new(),
        },
        ..ExecSpec::default()
    }
}

impl Harness {
    #[must_use]
    pub fn builder() -> HarnessBuilder<Unset, Unset, Unset> {
        HarnessBuilder::new()
    }

    /// Returns the capability registry.
    #[must_use]
    pub fn capability_registry(&self) -> Arc<CapabilityRegistry> {
        Arc::clone(&self.inner.cap_registry)
    }

    /// Computes a backend-authored runtime execution capability status.
    ///
    /// The returned payload describes which sandbox backend is active, whether
    /// the HTTP broker is available, and per-tool availability with reasons.
    /// The frontend must render this payload read-only; it must not infer
    /// availability from local constants.
    #[must_use]
    pub fn runtime_execution_status(&self) -> RuntimeExecutionStatus {
        let sandbox = &self.inner.sandbox;
        let caps = sandbox.capabilities();

        // Process sandbox
        let backend_id = sandbox.backend_id().to_owned();
        let candidate_ids = sandbox.candidate_backend_ids();

        let mut available_network_policies = Vec::new();
        if caps.network.none {
            available_network_policies.push("none".to_owned());
        }
        if caps.network.loopback_only {
            available_network_policies.push("loopback_only".to_owned());
        }
        if caps.network.allowlist {
            available_network_policies.push("allowlist".to_owned());
        }
        if caps.network.unrestricted {
            available_network_policies.push("unrestricted".to_owned());
        }

        let mut available_workspace_policies = Vec::new();
        if caps.workspace.read_write_all {
            available_workspace_policies.push("read_write_all".to_owned());
        }
        if caps.workspace.read_only {
            available_workspace_policies.push("read_only".to_owned());
        }
        if caps.workspace.writable_subpaths {
            available_workspace_policies.push("writable_subpaths".to_owned());
        }

        let mut unavailable_reasons = Vec::new();
        if !caps.network.none {
            unavailable_reasons
                .push("network policy `none` is not supported by the current sandbox".to_owned());
        }
        if !caps.network.allowlist {
            unavailable_reasons.push(
                "network policy `allowlist` is not supported by the current sandbox".to_owned(),
            );
        }
        if !caps.workspace.read_only {
            unavailable_reasons.push(
                "workspace policy `read_only` is not supported by the current sandbox".to_owned(),
            );
        }
        if !caps.workspace.writable_subpaths {
            unavailable_reasons.push(
                "workspace policy `writable_subpaths` is not supported by the current sandbox"
                    .to_owned(),
            );
        }

        // HTTP broker
        let broker_available = self
            .inner
            .cap_registry
            .get::<dyn harness_tool::ToolNetworkBrokerCap>(&ToolCapability::NetworkBroker)
            .is_some();
        let broker_denied_reasons: Vec<String> = if broker_available {
            Vec::new()
        } else {
            vec!["network broker is not registered in the capability registry".to_owned()]
        };
        let process_unavailable_reason = sandbox
            .preflight_execute(&process_tool_status_spec())
            .err()
            .map(|error| format!("process sandbox preflight failed: {error}"));
        let diagnostics_runner_available = self
            .inner
            .cap_registry
            .contains(&ToolCapability::Custom("diagnostics_runner".to_owned()));
        let web_search_backend_available = self
            .inner
            .cap_registry
            .contains(&ToolCapability::Custom("web_search_backend".to_owned()));
        let user_messenger_available = self
            .inner
            .cap_registry
            .contains(&ToolCapability::UserMessenger);

        // Per-tool status — report key tools this runtime knows about.
        let tools = vec![
            ToolRuntimeStatus {
                tool_name: "Bash".to_owned(),
                available: process_unavailable_reason.is_none(),
                unavailable_reason: process_unavailable_reason.clone(),
            },
            ToolRuntimeStatus {
                tool_name: "Diagnostics".to_owned(),
                available: diagnostics_runner_available && process_unavailable_reason.is_none(),
                unavailable_reason: if !diagnostics_runner_available {
                    Some("Diagnostics runner capability is not registered".to_owned())
                } else {
                    process_unavailable_reason.clone()
                },
            },
            ToolRuntimeStatus {
                tool_name: "WebFetch".to_owned(),
                available: broker_available,
                unavailable_reason: if broker_available {
                    None
                } else {
                    Some("HTTP broker is not registered".to_owned())
                },
            },
            ToolRuntimeStatus {
                tool_name: "WebSearch".to_owned(),
                available: web_search_backend_available,
                unavailable_reason: if web_search_backend_available {
                    None
                } else {
                    Some("web search backend capability is not registered".to_owned())
                },
            },
            ToolRuntimeStatus {
                tool_name: "MiniMaxTextToImage".to_owned(),
                available: broker_available,
                unavailable_reason: if broker_available {
                    None
                } else {
                    Some("HTTP broker is not registered".to_owned())
                },
            },
            ToolRuntimeStatus {
                tool_name: "SeedanceTextToVideo".to_owned(),
                available: broker_available,
                unavailable_reason: if broker_available {
                    None
                } else {
                    Some("HTTP broker is not registered".to_owned())
                },
            },
            ToolRuntimeStatus {
                tool_name: "SendMessage".to_owned(),
                available: user_messenger_available,
                unavailable_reason: if user_messenger_available {
                    None
                } else {
                    Some("UserMessenger capability is not registered".to_owned())
                },
            },
        ];

        RuntimeExecutionStatus::compute(
            &backend_id,
            candidate_ids,
            available_network_policies,
            available_workspace_policies,
            unavailable_reasons,
            broker_available,
            broker_denied_reasons,
            tools,
        )
    }

    fn evidence_ref_store(&self) -> Result<Arc<harness_journal::EvidenceRefStore>, HarnessError> {
        self.inner.evidence_ref_store.clone().ok_or_else(|| {
            HarnessError::Journal(harness_contracts::JournalError::Message(
                "evidence ref store not available".to_owned(),
            ))
        })
    }

    pub async fn read_command_output_evidence(
        &self,
        tenant: TenantId,
        conversation_id: &str,
        ref_id: &EvidenceRefId,
    ) -> Result<harness_journal::EvidenceReadResult, HarnessError> {
        self.read_typed_evidence(
            tenant,
            conversation_id,
            ref_id,
            EvidenceRefKind::CommandOutput,
        )
        .await
    }

    pub async fn read_command_output_evidence_window(
        &self,
        tenant: TenantId,
        conversation_id: &str,
        ref_id: &EvidenceRefId,
        cursor: Option<String>,
        max_bytes: usize,
    ) -> Result<harness_journal::EvidenceReadResult, HarnessError> {
        self.read_typed_evidence_window(
            tenant,
            conversation_id,
            ref_id,
            EvidenceRefKind::CommandOutput,
            cursor,
            max_bytes,
        )
        .await
    }

    pub async fn read_diff_patch_evidence(
        &self,
        tenant: TenantId,
        conversation_id: &str,
        ref_id: &EvidenceRefId,
    ) -> Result<harness_journal::EvidenceReadResult, HarnessError> {
        self.read_typed_evidence(tenant, conversation_id, ref_id, EvidenceRefKind::DiffPatch)
            .await
    }

    pub async fn read_diff_patch_evidence_window(
        &self,
        tenant: TenantId,
        conversation_id: &str,
        ref_id: &EvidenceRefId,
        cursor: Option<String>,
        max_bytes: usize,
    ) -> Result<harness_journal::EvidenceReadResult, HarnessError> {
        self.read_typed_evidence_window(
            tenant,
            conversation_id,
            ref_id,
            EvidenceRefKind::DiffPatch,
            cursor,
            max_bytes,
        )
        .await
    }

    pub async fn read_artifact_revision_content(
        &self,
        tenant: TenantId,
        conversation_id: &str,
        ref_id: &EvidenceRefId,
    ) -> Result<harness_journal::EvidenceReadResult, HarnessError> {
        self.read_typed_evidence(
            tenant,
            conversation_id,
            ref_id,
            EvidenceRefKind::ArtifactContent,
        )
        .await
    }

    pub async fn read_artifact_revision_content_window(
        &self,
        tenant: TenantId,
        conversation_id: &str,
        ref_id: &EvidenceRefId,
        cursor: Option<String>,
        max_bytes: usize,
    ) -> Result<harness_journal::EvidenceReadResult, HarnessError> {
        self.read_typed_evidence_window(
            tenant,
            conversation_id,
            ref_id,
            EvidenceRefKind::ArtifactContent,
            cursor,
            max_bytes,
        )
        .await
    }

    pub async fn list_live_evidence_blob_roots(
        &self,
        tenant: TenantId,
    ) -> Result<Vec<harness_contracts::BlobRef>, HarnessError> {
        self.evidence_ref_store()?
            .list_live_blob_roots(tenant)
            .await
            .map_err(HarnessError::Journal)
    }

    #[cfg(feature = "sqlite-store")]
    pub async fn list_artifact_content_evidence_refs(
        &self,
        tenant: TenantId,
        conversation_id: &str,
    ) -> Result<Vec<ArtifactContentEvidenceRef>, HarnessError> {
        self.register_artifact_content_evidence_refs(tenant, conversation_id)
            .await?;
        let refs = self
            .evidence_ref_store()?
            .list_for_conversation(tenant, conversation_id)
            .await
            .map_err(HarnessError::Journal)?;
        Ok(refs
            .into_iter()
            .filter(|record| record.kind == EvidenceRefKind::ArtifactContent)
            .filter_map(|record| {
                Some(ArtifactContentEvidenceRef {
                    artifact_id: record.artifact_id?,
                    revision_id: record.revision_id?,
                    content_ref: record.id,
                })
            })
            .collect())
    }

    #[cfg(feature = "sqlite-store")]
    async fn register_artifact_content_evidence_refs(
        &self,
        tenant: TenantId,
        conversation_id: &str,
    ) -> Result<(), HarnessError> {
        let session_id = SessionId::parse(conversation_id)
            .map_err(|error| HarnessError::Session(SessionError::Message(error.to_string())))?;
        let evidence_store = self.evidence_ref_store()?;
        let Some(blob_store) = self.inner.blob_store.as_ref() else {
            return Ok(());
        };
        let redactor = self
            .inner
            .observer
            .as_ref()
            .ok_or_else(|| HarnessError::Other("artifact content redactor is missing".to_owned()))?
            .redactor
            .clone();
        let mut envelopes = self
            .inner
            .event_store
            .read_envelopes(tenant, session_id, ReplayCursor::FromStart)
            .await
            .map_err(HarnessError::Journal)?
            .collect::<Vec<_>>()
            .await;
        envelopes.sort_by_key(|envelope| envelope.offset);

        let mut conversation_sequence = 0_u64;
        for envelope in envelopes {
            if let Some((artifact_id, revision_id, run_id, blob_ref)) =
                artifact_content_blob_from_event(&envelope.payload, session_id)
            {
                conversation_sequence = conversation_sequence.saturating_add(1);
                if let Some((bytes, redaction_state)) = redacted_artifact_content_bytes(
                    blob_store.as_ref(),
                    redactor.as_ref(),
                    tenant,
                    &blob_ref,
                    session_id,
                )
                .await?
                {
                    let record = artifact_content_evidence_record(
                        conversation_id,
                        &envelope,
                        conversation_sequence,
                        artifact_id,
                        revision_id,
                        run_id,
                        artifact_content_type(&blob_ref),
                        &bytes,
                        redaction_state,
                    );
                    evidence_store
                        .store_blob_evidence(tenant, record, bytes)
                        .await
                        .map_err(HarnessError::Journal)?;
                }
                continue;
            }

            if event_projects_to_conversation_timeline(&envelope.payload) {
                conversation_sequence = conversation_sequence.saturating_add(1);
            }
        }
        Ok(())
    }

    async fn read_typed_evidence(
        &self,
        tenant: TenantId,
        conversation_id: &str,
        ref_id: &EvidenceRefId,
        kind: EvidenceRefKind,
    ) -> Result<harness_journal::EvidenceReadResult, HarnessError> {
        self.evidence_ref_store()?
            .read_evidence(tenant, conversation_id, ref_id, kind)
            .await
            .map_err(HarnessError::Journal)
    }

    async fn read_typed_evidence_window(
        &self,
        tenant: TenantId,
        conversation_id: &str,
        ref_id: &EvidenceRefId,
        kind: EvidenceRefKind,
        cursor: Option<String>,
        max_bytes: usize,
    ) -> Result<harness_journal::EvidenceReadResult, HarnessError> {
        self.evidence_ref_store()?
            .read_evidence_window(
                tenant,
                conversation_id,
                ref_id,
                kind,
                harness_journal::EvidenceReadWindow { cursor, max_bytes },
            )
            .await
            .map_err(HarnessError::Journal)
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
        let observer = match extras.observer.take() {
            Some(observer) => Some(observer),
            None => Some(Arc::new(
                Observer::builder()
                    .build()
                    .map_err(|error| HarnessError::Other(error.to_string()))?,
            )),
        };
        let authorization_service = extras.authorization_service.take();
        let permission_authority =
            match (extras.permission_authority.take(), &authorization_service) {
                (Some(authority), Some(service)) => {
                    let service_authority = service.permission_authority();
                    if !Arc::ptr_eq(&authority, &service_authority) {
                        return Err(HarnessError::PermissionDenied(
                        "authorization service and permission authority must use the same authority"
                            .to_owned(),
                    ));
                    }
                    authority
                }
                (Some(authority), None) => authority,
                (None, Some(service)) => service.permission_authority(),
                (None, None) => {
                    let runtime = permission_authority_runtime(
                        &builder.options,
                        extras.permission_broker.take(),
                        &extras.rule_providers,
                        extras.decision_store.take(),
                    )
                    .await?;
                    runtime.permission_authority
                }
            };
        let permission_broker = Arc::new(PermissionAuthorityBroker {
            authority: Arc::clone(&permission_authority),
            policy_broker: permission_authority.policy_broker(),
            decision_store: permission_authority.decision_store(),
        }) as Arc<dyn PermissionBroker>;
        let mut cap_registry = extras.cap_registry.take().unwrap_or_default();
        let process_registry_capability =
            ToolCapability::Custom(RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY.to_owned());
        if !cap_registry.contains(&process_registry_capability) {
            cap_registry.install::<dyn RunScopedProcessRegistryCap>(
                process_registry_capability,
                Arc::new(DefaultRunScopedProcessRegistry::new(Arc::clone(
                    &builder.sandbox.0,
                ))),
            );
        }
        let cap_registry = Arc::new(cap_registry);
        let authorization_service = if let Some(service) = authorization_service {
            service
        } else {
            let network_broker = cap_registry
                .get::<dyn harness_tool::ToolNetworkBrokerCap>(&ToolCapability::NetworkBroker)
                .map(|broker| broker as Arc<dyn harness_tool::ToolNetworkBrokerPreflightCap>);
            let registry = harness_execution::ExecutionPreflightRegistry::new(
                Arc::clone(&builder.sandbox.0),
                network_broker,
                Arc::clone(&cap_registry),
            );
            Arc::new(harness_execution::AuthorizationService::new(
                Arc::clone(&permission_authority),
                registry,
                Arc::new(SdkAuthorizationEventSink {
                    event_store: Arc::clone(&builder.store.0),
                    redactor: observer
                        .as_ref()
                        .expect("SDK observer is always initialized")
                        .redactor
                        .clone(),
                }),
                Arc::new(harness_execution::TicketLedger::default()),
            ))
        };
        let skill_registry = SkillRegistry::builder().build();
        let mut mcp_config = extras.mcp_config.take();
        let plugin_registry = extras.plugin_registry.take();
        if plugin_registry.is_some() && mcp_config.is_none() {
            mcp_config = Some(McpConfig::default());
        }
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

        #[cfg(feature = "memory-provider-registry")]
        let memory_extractor = extras.memory_extractor.take().unwrap_or_else(|| {
            let extraction_redactor = observer
                .as_ref()
                .map(|observer| Arc::clone(&observer.redactor))
                .unwrap_or_else(default_hook_redactor);
            Arc::new(ModelBackedMemoryExtractor::new(
                Arc::clone(&builder.model.0),
                builder.options.model_id.clone(),
                builder.model.0.default_protocol(),
                extraction_redactor,
            ))
        });
        #[cfg(feature = "memory-provider-registry")]
        let memory_database_path = extras.memory_database_path.take().unwrap_or_else(|| {
            builder
                .options
                .default_session_options
                .agent_runtime_root
                .clone()
                .unwrap_or_else(|| {
                    builder
                        .options
                        .workspace_root
                        .join(".jyowo")
                        .join("runtime")
                })
                .join("memory")
                .join("memory.sqlite3")
        });
        #[cfg(feature = "memory-provider-registry")]
        let memory_extraction_runtime = Some({
            MemoryExtractionRuntime::spawn(
                memory_database_path.clone(),
                builder.options.tenant_policy.id,
                Arc::clone(&memory_extractor),
                observer.as_ref().map(Arc::clone),
            )
        });

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
                permission_authority,
                authorization_service,
                tool_registry,
                hook_registry,
                memory_providers: extras.memory_providers,
                #[cfg(feature = "memory-provider-registry")]
                memory_database_path,
                #[cfg(feature = "memory-provider-registry")]
                _memory_extraction_runtime: memory_extraction_runtime,
                #[cfg(feature = "memory-builtin")]
                builtin_memory: extras.builtin_memory.take(),
                blob_store: extras.blob_store.take(),
                evidence_ref_store: extras.evidence_ref_store.take(),
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
                cap_registry,
                #[cfg(feature = "tool-search")]
                tool_search_scorer: extras.tool_search_scorer.take(),
                enabled_features: Self::enabled_feature_set(),
                session_limits,
                workspace_registry: Arc::new(WorkspaceRegistry::new()),
                active_conversation_runs: Arc::new(parking_lot::Mutex::new(HashMap::new())),
                active_conversation_sessions: Arc::new(parking_lot::Mutex::new(HashMap::new())),
                #[cfg(feature = "agents-team")]
                active_run_teams: Arc::new(parking_lot::Mutex::new(HashMap::new())),
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
                provider_continuation_store: extras.provider_continuation_store.take(),
            }),
        })
    }
}

#[cfg(all(test, feature = "memory-provider-registry"))]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use harness_observability::{Span, SpanEvent, TraceCarrier, TraceContext, TraceId, Tracer};

    #[test]
    fn memory_extraction_excerpt_uses_default_redactor_after_noop_redactor() {
        let redacted = redact_memory_extraction_excerpt(
            "token sk-abcdefghijklmnopqrstuvwxyz should not reach memory extraction",
            &harness_contracts::NoopRedactor,
        );

        assert!(!redacted.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn memory_extraction_poll_error_records_redacted_telemetry() {
        let tracer = Arc::new(RecordingTracer::default());
        let observer = Observer::builder()
            .with_tracer(tracer.clone())
            .with_redactor(Arc::new(harness_contracts::NoopRedactor))
            .build()
            .expect("observer");

        record_memory_extraction_poll_error(
            Some(&observer),
            "open queue failed with token sk-abcdefghijklmnopqrstuvwxyz",
        );

        let spans = tracer.spans.lock().expect("spans");
        assert_eq!(spans.len(), 1);
        let span = &spans[0];
        assert_eq!(span.name, "memory.extraction.poll");
        assert!(matches!(span.status, SpanStatus::Error(_)));
        assert!(!span.status_text().contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(span.status_text().contains("[REDACTED]"));
        let error_event = span
            .events
            .iter()
            .find(|event| event.name == "memory.extraction.poll.error")
            .expect("poll error event");
        let error = match error_event.attrs.attrs.get("error") {
            Some(AttributeValue::String(error)) => error,
            _ => panic!("error attribute"),
        };
        assert!(!error.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(error.contains("[REDACTED]"));
    }

    struct RecordingTracer {
        spans: Arc<Mutex<Vec<RecordedSpan>>>,
    }

    impl Default for RecordingTracer {
        fn default() -> Self {
            Self {
                spans: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl Tracer for RecordingTracer {
        fn start_span(&self, name: &str, attrs: SpanAttributes) -> Box<dyn Span> {
            Box::new(RecordingSpan {
                target: Arc::clone(&self.spans),
                name: name.to_owned(),
                attrs,
                events: Vec::new(),
                status: SpanStatus::Unset,
                context: TraceContext::new(
                    TraceId::new("00000000000000000000000000000001"),
                    harness_observability::SpanId::new("0000000000000001"),
                    None,
                ),
            })
        }

        fn inject_context(&self, _carrier: &mut dyn TraceCarrier) {}

        fn extract_context(&self, carrier: &dyn TraceCarrier) -> Option<TraceContext> {
            TraceContext::extract(carrier)
        }
    }

    struct RecordingSpan {
        target: Arc<Mutex<Vec<RecordedSpan>>>,
        name: String,
        attrs: SpanAttributes,
        events: Vec<SpanEvent>,
        status: SpanStatus,
        context: TraceContext,
    }

    impl Span for RecordingSpan {
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
            self.target.lock().expect("spans").push(RecordedSpan {
                name: self.name,
                attrs: self.attrs,
                events: self.events,
                status: self.status,
            });
        }
    }

    struct RecordedSpan {
        name: String,
        #[allow(dead_code)]
        attrs: SpanAttributes,
        events: Vec<SpanEvent>,
        status: SpanStatus,
    }

    impl RecordedSpan {
        fn status_text(&self) -> &str {
            match &self.status {
                SpanStatus::Error(error) => error,
                SpanStatus::Unset | SpanStatus::Ok => "",
            }
        }
    }
}

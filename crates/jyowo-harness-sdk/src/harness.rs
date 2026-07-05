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
#[cfg(feature = "sqlite-store")]
use harness_contracts::BlobError;
#[cfg(feature = "tool-search")]
use harness_contracts::CacheImpact;
#[cfg(any(feature = "memory-builtin", feature = "memory-external-slot"))]
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
    ManifestValidationFailedEvent, McpServerId, Message, MessageContent, MessageId, MessagePart,
    MessageRole, ModelModality, ModelProtocol, PermissionError, PermissionMode,
    PluginCapabilitiesSummary, PluginFailedEvent, PluginLifecycleStateDiscriminant,
    PluginLoadedEvent, PluginRejectedEvent, ProviderCapabilityRouteSettings, RedactPatternSet,
    RedactRules, RedactScope, Redactor, RejectionReason, RunId, RunModelSnapshot,
    RunScopedProcessRegistryCap, SessionError, SessionId, TenantId, ToolCapability, ToolProfile,
    ToolSearchMode, TrustLevel, TurnInput, RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY,
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
#[cfg(feature = "memory-consolidation")]
use harness_memory::ConsolidationHook;
use harness_memory::MemoryProvider;
use harness_model::ModelRuntimeSnapshot;
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
use harness_sandbox::SandboxBackend;
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
use self::redaction::redact_business_event_for_display;
use self::run_state::{ActiveConversationRun, ActiveConversationRunGuard, EngineSessionTurnRunner};
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
    memory_provider: Option<Arc<dyn MemoryProvider>>,
    #[cfg(feature = "memory-consolidation")]
    consolidation_hook: Option<Arc<dyn ConsolidationHook>>,
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

impl Harness {
    #[must_use]
    pub fn builder() -> HarnessBuilder<Unset, Unset, Unset> {
        HarnessBuilder::new()
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
        let authorization_service = if let Some(service) = authorization_service {
            service
        } else {
            Arc::new(harness_execution::AuthorizationService::new(
                Arc::clone(&permission_authority),
                Arc::clone(&builder.sandbox.0),
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
                permission_authority,
                authorization_service,
                tool_registry,
                hook_registry,
                memory_provider: extras.memory_provider.take(),
                #[cfg(feature = "memory-consolidation")]
                consolidation_hook: extras.consolidation_hook.take(),
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
                cap_registry: Arc::new(cap_registry),
                #[cfg(feature = "tool-search")]
                tool_search_scorer: extras.tool_search_scorer.take(),
                enabled_features: Self::enabled_feature_set(),
                session_limits,
                workspace_registry: Arc::new(WorkspaceRegistry::new()),
                active_conversation_runs: Arc::new(parking_lot::Mutex::new(HashMap::new())),
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

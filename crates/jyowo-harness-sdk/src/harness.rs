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
#[cfg(any(feature = "memory-builtin", feature = "memory-provider-registry"))]
use harness_contracts::MemdirFileTag;
#[cfg(not(feature = "observability-redactor"))]
use harness_contracts::RedactPatternKind;
#[cfg(feature = "stream-permission")]
use harness_contracts::RequestId;
#[cfg(feature = "agents-team")]
use harness_contracts::{
    AgentId, BlobMeta, BlobRetention, Recipient, TeamCreatedEvent, TeamMemberJoinedEvent,
    TeamTaskUpdatedEvent, TeamTerminationReason, TopologyKind,
};
use harness_contracts::{
    BlobReaderCapAdapter, BlobStore, BlobWriterCapAdapter, CapabilityRegistry, ContextPatchRequest,
    ContextPatchSinkCap, ConversationAttachmentReference, ConversationContextReference,
    ConversationTurnInput, Decision, Event, EventId, HarnessError, HookEventKind,
    InteractivityLevel, JournalOffset, ManifestOriginRef, ManifestValidationFailedEvent,
    McpServerId, Message, MessageContent, MessageId, MessagePart, MessageRole, ModelModality,
    ModelProtocol, PermissionError, PermissionMode, PluginCapabilitiesSummary, PluginFailedEvent,
    PluginLifecycleStateDiscriminant, PluginLoadedEvent, PluginRejectedEvent,
    ProviderCapabilityRouteSettings, RedactPatternSet, RedactRules, RedactScope, Redactor,
    RejectionReason, RunId, RunModelSnapshot, RunScopedProcessRegistryCap, SessionError, SessionId,
    TenantId, ToolCapability, ToolProfile, ToolSearchMode, TrustLevel, TurnInput,
    RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY,
};
#[cfg(feature = "sqlite-store")]
use harness_contracts::{
    ConversationCursor, ConversationSnapshot, ConversationSummary, ConversationTimelinePage,
    ConversationTurnCursor, ConversationWorktreePage,
};
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

use std::{
    collections::{hash_map::Entry, BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
    time::Duration,
};

use async_trait::async_trait;
use chrono::Utc;
use futures::{future::BoxFuture, stream, FutureExt, Stream, StreamExt};
use harness_contracts::{
    now, ActionResource, AgentId, AgentTeamRunConfig, AgentTeamSharedMemoryPolicy,
    AgentTeamTopology, AgentToolPolicy, AgentUsePolicy, AgentWorkspaceIsolationMode,
    CapabilityRegistry, ConversationAttachmentReference, ConversationContextReference,
    ConversationTurnInput, Event, ExecutionDefaultsRecord, FallbackPolicy,
    IndeterminateToolResolution, InteractivityLevel, McpServerId, McpServerScope, ModelError,
    PermissionMode, PromotionMode, QueueItemState, Redactor, RunId, RunSegmentId,
    RunTerminalReason, StopReason, TaskId, TenantId, ToolActionPlan, ToolCapability,
    ToolDescriptor, ToolError, ToolErrorPayload, ToolResult, ToolUseFailedEvent, ToolUseId,
    UnexpectedErrorEvent, UsageSnapshot, WorkspaceAccess as ToolWorkspaceAccess, WorkspaceLeaseId,
    WorkspaceLeaseState, WorkspaceMode,
};
use harness_engine::{EngineBoundSubagentFactory, RunControlHandle, TurnOutcome};
use harness_execution::{
    AuthorizationEventSink, AuthorizationService, ExecutionPreflightRegistry,
    ReqwestToolNetworkBroker, TicketLedger,
};
use harness_journal::{
    AppendMetadata, EventStore, ReplayCursor, SegmentExecutionClaim, SegmentExecutionTerminal,
    TaskBlobStore, TaskEventStoreAdapter, TaskStore,
};
use harness_mcp::{
    HttpTransport, McpAuthorizationContext, McpConnectContext, McpEventSink, McpRegistry,
    McpServerSpec, McpTransport, StdioEnv, StdioPolicy, StdioTransport, TransportChoice,
};
use harness_permission::{NoopDecisionPersistence, PermissionAuthority};
use harness_sandbox::{LocalIsolation, LocalSandbox, SandboxBackend};
use harness_subagent::{
    ChildRunOutcome, ChildRunRequest, DefaultSubagentRunner, DelegationPolicy,
    SubagentEngineFactory, SubagentError, SubagentRunner,
};
use harness_tool::{ToolNetworkBrokerCap, ToolNetworkBrokerPreflightCap};
use jyowo_harness_sdk::{
    ext::{
        AuthorizedToolInput, ContentDelta, HealthStatus, InferContext, ModelDescriptor,
        ModelProvider, ModelRequest, ModelStream, ModelStreamEvent, SchemaResolverContext, Tool,
        ToolContext, ToolRegistry, ToolStream, ValidationError,
    },
    ConversationRunOptions, ConversationTurnRequest, Harness, HarnessBuilder, McpConfig,
    SessionOptions,
};
use serde_json::json;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, watch};

use crate::{
    AgentStarterCapabilities, HarnessPermissionBroker, PermissionBroker,
    PermissionRuntimeAuthority, RunCoordinatorEvent, RunCoordinatorFactory, RunningSegment,
    RuntimeConfigResolver, RuntimeConfigSnapshot, RuntimeMcpServerConfig, StartSegmentRequest,
    WorkspaceSubagentRunContext, WorkspaceSubagentRunnerFactory, WorkspaceToolAction,
    WorkspaceToolDispatcher,
};

struct DaemonAuthorizationEventSink {
    event_store: Arc<dyn EventStore>,
}

#[async_trait]
impl AuthorizationEventSink for DaemonAuthorizationEventSink {
    async fn emit_batch(
        &self,
        tenant_id: TenantId,
        session_id: harness_contracts::SessionId,
        events: Vec<Event>,
    ) -> Result<(), harness_execution::ExecutionError> {
        self.event_store
            .append(tenant_id, session_id, &events)
            .await
            .map(|_| ())
            .map_err(|error| harness_execution::ExecutionError::EventSinkFailed {
                reason: format!("journal append failed: {error}"),
            })
    }
}

struct DaemonAuthorizationRuntime {
    authority: Arc<PermissionAuthority>,
    service: Arc<AuthorizationService>,
    network_broker: Arc<dyn ToolNetworkBrokerCap>,
}

struct DaemonPolicyBroker {
    hard_policy_broker: Arc<dyn harness_permission::PermissionBroker>,
}

#[async_trait]
impl harness_permission::PermissionBroker for DaemonPolicyBroker {
    async fn decide(
        &self,
        _request: harness_permission::PermissionRequest,
        _ctx: harness_permission::PermissionContext,
    ) -> harness_contracts::Decision {
        harness_contracts::Decision::Escalate
    }

    async fn hard_policy_denies(
        &self,
        request: &harness_permission::PermissionRequest,
        ctx: &harness_permission::PermissionContext,
    ) -> bool {
        self.hard_policy_broker
            .hard_policy_denies(request, ctx)
            .await
    }

    async fn persist(
        &self,
        decision: harness_permission::PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        self.hard_policy_broker.persist(decision).await
    }
}

fn daemon_authorization_runtime(
    permission_broker: HarnessPermissionBroker,
    sandbox: Arc<dyn SandboxBackend>,
    event_store: Arc<dyn EventStore>,
    redactor: Arc<dyn Redactor>,
    provider_credential_resolver: Arc<dyn harness_contracts::ProviderCredentialResolverCap>,
) -> Result<DaemonAuthorizationRuntime, SdkRunFactoryError> {
    let interactive_broker: Arc<dyn harness_permission::PermissionBroker> =
        Arc::new(permission_broker);
    let policy_broker: Arc<dyn harness_permission::PermissionBroker> =
        Arc::new(DaemonPolicyBroker {
            hard_policy_broker: Arc::clone(&interactive_broker),
        });
    let authority = Arc::new(
        PermissionAuthority::builder()
            .with_policy_broker(policy_broker)
            .with_interactive_broker(interactive_broker)
            .with_transient_decision_store(Arc::new(NoopDecisionPersistence))
            .build()
            .map_err(|error| SdkRunFactoryError::Sdk(error.to_string()))?,
    );
    let ticket_ledger = Arc::new(TicketLedger::default());
    let concrete_network_broker = Arc::new(
        ReqwestToolNetworkBroker::new_with_ticket_authority(
            Duration::from_secs(120),
            10 * 1024 * 1024,
            redactor,
            ticket_ledger.authority_key(),
        )
        .map_err(|error| SdkRunFactoryError::Sdk(error.to_string()))?,
    );
    let network_broker: Arc<dyn ToolNetworkBrokerCap> = concrete_network_broker.clone();
    let network_preflight: Arc<dyn ToolNetworkBrokerPreflightCap> = concrete_network_broker;
    let mut capabilities = CapabilityRegistry::default();
    capabilities.install(ToolCapability::NetworkBroker, Arc::clone(&network_broker));
    capabilities.install(
        ToolCapability::ProviderCredentialResolver,
        provider_credential_resolver,
    );
    let preflight =
        ExecutionPreflightRegistry::new(sandbox, Some(network_preflight), Arc::new(capabilities));
    let service = Arc::new(AuthorizationService::new(
        Arc::clone(&authority),
        preflight,
        Arc::new(DaemonAuthorizationEventSink { event_store }),
        ticket_ledger,
    ));
    Ok(DaemonAuthorizationRuntime {
        authority,
        service,
        network_broker,
    })
}

fn apply_runtime_snapshot<M, S, SB>(
    builder: HarnessBuilder<M, S, SB>,
    snapshot: &RuntimeConfigSnapshot,
    mcp_config: McpConfig,
) -> Result<HarnessBuilder<M, S, SB>, harness_plugin::PluginError> {
    Ok(builder
        .with_mcp_config(mcp_config)
        .with_plugin_registry(snapshot.materialize_plugin_registry()?)
        .with_skill_loader(snapshot.skill_loader.clone())
        .with_skill_config_snapshot(snapshot.skill_config.clone())
        .with_provider_capability_routes(snapshot.provider_routes.clone())
        .with_capability::<dyn harness_contracts::ProviderCredentialResolverCap>(
            harness_contracts::ToolCapability::ProviderCredentialResolver,
            Arc::clone(&snapshot.provider_credential_resolver),
        ))
}

async fn mcp_config_from_runtime_snapshot(
    snapshot: &RuntimeConfigSnapshot,
    authorization_service: Arc<AuthorizationService>,
    event_store: Arc<dyn EventStore>,
    execution_root: &Path,
    session_id: harness_contracts::SessionId,
    run_id: RunId,
    permission_mode: PermissionMode,
) -> Result<DaemonMcpRuntimeGuard, SdkRunFactoryError> {
    let registry = McpRegistry::new();
    let (daemon_event_sink, event_receiver) = DaemonMcpEventSink::channel_with_context(
        DAEMON_MCP_EVENT_CHANNEL_CAPACITY,
        session_id,
        run_id,
    );
    let event_writer = spawn_daemon_mcp_event_writer(event_receiver, event_store, session_id);
    let event_sink: Arc<dyn McpEventSink> = daemon_event_sink.clone();
    let build_result = async {
        let mut server_ids_to_inject = Vec::new();
        let agent_id = AgentId::new();
        for record in snapshot
            .mcp_servers
            .iter()
            .filter(|record| should_activate_mcp_server(record))
        {
            let (spec, transport) = mcp_server_runtime(record, execution_root)?;
            let scope = match record.scope.as_str() {
                "global" => McpServerScope::Global,
                "session" => McpServerScope::Session(session_id),
                "agent" => McpServerScope::Agent(agent_id),
                _ => {
                    return Err(SdkRunFactoryError::RuntimeConfig(
                        "invalid persisted MCP server scope".to_owned(),
                    ));
                }
            };
            let authorization = McpAuthorizationContext {
                authorization_service: Arc::clone(&authorization_service),
                tenant_id: TenantId::SINGLE,
                scope: scope.clone(),
                session_id,
                run_id,
                permission_mode,
                interactivity: InteractivityLevel::NoInteractive,
                fallback_policy: FallbackPolicy::AskUser,
                workspace_root: execution_root.to_owned(),
            };
            if let Some(server_id) = activate_daemon_mcp_server(
                &registry,
                spec,
                scope,
                transport,
                Arc::clone(&event_sink),
                McpConnectContext::default()
                    .with_permission_mode(permission_mode)
                    .with_authorization(authorization),
            )
            .await?
            {
                server_ids_to_inject.push(server_id);
            }
        }
        Ok(server_ids_to_inject)
    }
    .await;
    match build_result {
        Ok(server_ids_to_inject) => Ok(DaemonMcpRuntimeGuard {
            config: McpConfig {
                registry,
                server_ids_to_inject,
                event_sink,
            },
            event_sink: daemon_event_sink,
            event_writer: Some(event_writer),
            shutdown_complete: false,
        }),
        Err(error) => {
            let _ = registry.shutdown_all().await;
            daemon_event_sink.close();
            let _ = event_writer.await;
            Err(error)
        }
    }
}

fn should_activate_mcp_server(record: &RuntimeMcpServerConfig) -> bool {
    record.enabled
}

async fn activate_daemon_mcp_server(
    registry: &McpRegistry,
    spec: McpServerSpec,
    scope: McpServerScope,
    transport: Arc<dyn McpTransport>,
    event_sink: Arc<dyn McpEventSink>,
    context: McpConnectContext,
) -> Result<Option<McpServerId>, SdkRunFactoryError> {
    let session_id = context
        .authorization
        .as_ref()
        .map(|authorization| authorization.session_id);
    let run_id = context
        .authorization
        .as_ref()
        .map(|authorization| authorization.run_id);
    let server_id = spec.server_id.clone();
    let required = spec.required;
    if registry
        .add_managed_server_with_context(
            spec.clone(),
            scope.clone(),
            transport,
            Arc::clone(&event_sink),
            context,
        )
        .await
        .is_ok()
    {
        return Ok(Some(server_id));
    }

    let reason = "MCP server activation failed".to_owned();
    event_sink.emit(Event::UnexpectedError(UnexpectedErrorEvent {
        session_id,
        run_id,
        error: reason.clone(),
        at: now(),
    }));
    registry
        .add_failed_server(spec, scope, reason)
        .await
        .map_err(|_| {
            SdkRunFactoryError::RuntimeConfig(
                "persisted MCP server state could not be registered".to_owned(),
            )
        })?;

    if required {
        let _ = registry.shutdown_all().await;
        return Err(SdkRunFactoryError::RuntimeConfig(
            "required MCP server failed during activation".to_owned(),
        ));
    }
    Ok(None)
}

struct DaemonMcpRuntimeGuard {
    config: McpConfig,
    event_sink: Arc<DaemonMcpEventSink>,
    event_writer: Option<tokio::task::JoinHandle<Result<(), SdkRunFactoryError>>>,
    shutdown_complete: bool,
}

impl DaemonMcpRuntimeGuard {
    fn config(&self) -> McpConfig {
        self.config.clone()
    }

    async fn shutdown(mut self) -> Result<(), SdkRunFactoryError> {
        let registry_result = self.config.registry.shutdown_all().await;
        self.event_sink.close();
        let writer_result = match self.event_writer.take() {
            Some(writer) => writer.await.map_err(|_| {
                SdkRunFactoryError::RuntimeConfig(
                    "MCP diagnostic writer did not shut down cleanly".to_owned(),
                )
            })?,
            None => Ok(()),
        };
        self.shutdown_complete = true;
        registry_result
            .map_err(|_| {
                SdkRunFactoryError::RuntimeConfig("failed to shut down MCP runtime".to_owned())
            })
            .and(writer_result)
    }
}

impl Drop for DaemonMcpRuntimeGuard {
    fn drop(&mut self) {
        if self.shutdown_complete {
            return;
        }
        let registry = self.config.registry.clone();
        self.event_sink.close();
        let writer = self.event_writer.take();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = registry.shutdown_all().await;
                if let Some(writer) = writer {
                    let _ = writer.await;
                }
            });
        }
    }
}

const DAEMON_MCP_EVENT_CHANNEL_CAPACITY: usize = 128;

struct DaemonMcpEventSink {
    sender: Mutex<Option<mpsc::Sender<Event>>>,
    dropped_events: AtomicU64,
    default_session_id: Option<harness_contracts::SessionId>,
    default_run_id: Option<RunId>,
}

impl DaemonMcpEventSink {
    #[cfg(test)]
    fn channel(capacity: usize) -> (Arc<Self>, mpsc::Receiver<Event>) {
        Self::channel_with_optional_context(capacity, None, None)
    }

    fn channel_with_context(
        capacity: usize,
        session_id: harness_contracts::SessionId,
        run_id: RunId,
    ) -> (Arc<Self>, mpsc::Receiver<Event>) {
        Self::channel_with_optional_context(capacity, Some(session_id), Some(run_id))
    }

    fn channel_with_optional_context(
        capacity: usize,
        default_session_id: Option<harness_contracts::SessionId>,
        default_run_id: Option<RunId>,
    ) -> (Arc<Self>, mpsc::Receiver<Event>) {
        let (sender, receiver) = mpsc::channel(capacity);
        (
            Arc::new(Self {
                sender: Mutex::new(Some(sender)),
                dropped_events: AtomicU64::new(0),
                default_session_id,
                default_run_id,
            }),
            receiver,
        )
    }

    fn close(&self) {
        self.sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }

    #[cfg(test)]
    fn dropped_events(&self) -> u64 {
        self.dropped_events.load(Ordering::Relaxed)
    }
}

impl McpEventSink for DaemonMcpEventSink {
    fn emit(&self, mut event: Event) {
        if let Event::UnexpectedError(diagnostic) = &mut event {
            diagnostic.session_id = diagnostic.session_id.or(self.default_session_id);
            diagnostic.run_id = diagnostic.run_id.or(self.default_run_id);
        }
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .cloned();
        if sender.is_none_or(|sender| sender.try_send(event).is_err()) {
            self.dropped_events.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn spawn_daemon_mcp_event_writer(
    mut receiver: mpsc::Receiver<Event>,
    event_store: Arc<dyn EventStore>,
    session_id: harness_contracts::SessionId,
) -> tokio::task::JoinHandle<Result<(), SdkRunFactoryError>> {
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            event_store
                .append(TenantId::SINGLE, session_id, &[event])
                .await
                .map_err(|_| {
                    SdkRunFactoryError::RuntimeConfig(
                        "MCP diagnostic could not be written to the task journal".to_owned(),
                    )
                })?;
        }
        Ok(())
    })
}

fn mcp_server_runtime(
    record: &RuntimeMcpServerConfig,
    workspace_root: &Path,
) -> Result<(McpServerSpec, Arc<dyn McpTransport>), SdkRunFactoryError> {
    let (transport_choice, transport): (TransportChoice, Arc<dyn McpTransport>) =
        match &record.transport {
            harness_contracts::McpServerTransportConfig::Stdio {
                command,
                args,
                env,
                inherit_env,
                working_dir,
            } => {
                if command.trim().is_empty() {
                    return Err(SdkRunFactoryError::RuntimeConfig(
                        "persisted MCP stdio command is invalid".to_owned(),
                    ));
                }
                let mut policy = StdioPolicy::default();
                policy.working_dir = Some(mcp_stdio_working_dir(
                    working_dir.as_deref(),
                    workspace_root,
                )?);
                let extra = env
                    .iter()
                    .map(|entry| (entry.key.clone(), entry.value.clone()))
                    .collect::<BTreeMap<_, _>>();
                let inherit = effective_stdio_inherit_env(command, inherit_env)
                    .into_iter()
                    .collect::<BTreeSet<_>>();
                let env = if inherit.is_empty() {
                    StdioEnv::Empty { extra }
                } else {
                    StdioEnv::Allowlist { inherit, extra }
                };
                (
                    TransportChoice::Stdio {
                        command: command.clone(),
                        args: args.clone(),
                        env,
                        policy,
                    },
                    Arc::new(StdioTransport::new()),
                )
            }
            harness_contracts::McpServerTransportConfig::Http {
                url,
                bearer_token_env_var,
                headers,
                headers_from_env,
            } => {
                if url.trim().is_empty() {
                    return Err(SdkRunFactoryError::RuntimeConfig(
                        "persisted MCP HTTP URL is invalid".to_owned(),
                    ));
                }
                let mut resolved_headers = headers
                    .iter()
                    .map(|entry| (entry.key.trim().to_owned(), entry.value.clone()))
                    .collect::<BTreeMap<_, _>>();
                for header in headers_from_env {
                    let value = std::env::var(&header.env_var).map_err(|_| {
                        SdkRunFactoryError::RuntimeConfig(
                            "configured MCP header environment variable is unavailable".to_owned(),
                        )
                    })?;
                    resolved_headers.insert(header.key.trim().to_owned(), value);
                }
                if let Some(env_var) = bearer_token_env_var {
                    let token = std::env::var(env_var).map_err(|_| {
                        SdkRunFactoryError::RuntimeConfig(
                            "configured MCP bearer environment variable is unavailable".to_owned(),
                        )
                    })?;
                    resolved_headers.insert("Authorization".to_owned(), format!("Bearer {token}"));
                }
                (
                    TransportChoice::Http {
                        url: url.clone(),
                        headers: resolved_headers,
                    },
                    Arc::new(HttpTransport::new()),
                )
            }
            harness_contracts::McpServerTransportConfig::InProcess => {
                return Err(SdkRunFactoryError::RuntimeConfig(
                    "persisted MCP transport is unsupported by the daemon".to_owned(),
                ))
            }
        };
    let mut spec = McpServerSpec::new(
        McpServerId(record.id.clone()),
        record.display_name.clone(),
        transport_choice,
        record.source.clone(),
    );
    spec.required = record.required;
    Ok((spec, transport))
}

fn effective_stdio_inherit_env(command: &str, configured: &[String]) -> Vec<String> {
    if !configured.is_empty() {
        return configured.to_vec();
    }
    let command_name = Path::new(command)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(command);
    if matches!(command_name, "npx" | "npm" | "pnpm" | "yarn" | "bun") {
        ["PATH", "HOME", "USER", "TMPDIR"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    } else {
        Vec::new()
    }
}

fn mcp_stdio_working_dir(
    configured: Option<&str>,
    workspace_root: &Path,
) -> Result<PathBuf, SdkRunFactoryError> {
    let Some(configured) = configured else {
        return Ok(workspace_root.to_owned());
    };
    if configured.trim().is_empty() {
        return Err(SdkRunFactoryError::RuntimeConfig(
            "persisted MCP working directory is invalid".to_owned(),
        ));
    }
    let path = PathBuf::from(configured);
    let candidate = if path.is_absolute() {
        path
    } else {
        workspace_root.join(path)
    };
    let canonical = candidate.canonicalize().map_err(|_| {
        SdkRunFactoryError::RuntimeConfig(
            "persisted MCP working directory is unavailable".to_owned(),
        )
    })?;
    if !canonical.starts_with(workspace_root) {
        return Err(SdkRunFactoryError::RuntimeConfig(
            "persisted MCP working directory escapes the workspace".to_owned(),
        ));
    }
    Ok(canonical)
}

#[derive(Clone)]
struct SharedSegment {
    control: RunControlHandle,
    terminal: watch::Receiver<Option<RunCoordinatorEvent>>,
}

/// Production daemon adapter that executes task segments through the public SDK facade.
pub struct SdkRunCoordinatorFactory {
    store: Arc<TaskStore>,
    runtime_configs: RuntimeConfigResolver,
    blob_root: PathBuf,
    permissions: Arc<PermissionBroker>,
    redactor: Arc<dyn Redactor>,
    subagent_engines: Arc<SdkSubagentEngineRegistry>,
    segments: Arc<Mutex<HashMap<(TaskId, RunSegmentId), SharedSegment>>>,
}

#[derive(Default)]
pub struct SdkSubagentEngineRegistry {
    runtimes: Mutex<HashMap<RunSegmentId, Arc<SdkSubagentRuntimeTemplate>>>,
}

impl SdkSubagentEngineRegistry {
    fn bind(
        self: &Arc<Self>,
        segment_id: RunSegmentId,
        runtime: Arc<SdkSubagentRuntimeTemplate>,
    ) -> SdkSubagentEngineBinding {
        self.runtimes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(segment_id, Arc::clone(&runtime));
        SdkSubagentEngineBinding {
            registry: Arc::clone(self),
            segment_id,
            runtime,
        }
    }

    fn get(&self, segment_id: RunSegmentId) -> Option<Arc<SdkSubagentRuntimeTemplate>> {
        self.runtimes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&segment_id)
            .cloned()
    }
}

struct SdkSubagentEngineBinding {
    registry: Arc<SdkSubagentEngineRegistry>,
    segment_id: RunSegmentId,
    runtime: Arc<SdkSubagentRuntimeTemplate>,
}

impl Drop for SdkSubagentEngineBinding {
    fn drop(&mut self) {
        let mut runtimes = self
            .registry
            .runtimes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if runtimes
            .get(&self.segment_id)
            .is_some_and(|runtime| Arc::ptr_eq(runtime, &self.runtime))
        {
            runtimes.remove(&self.segment_id);
        }
    }
}

struct SdkSubagentRuntimeTemplate {
    store: Arc<TaskStore>,
    runtime_config: RuntimeConfigSnapshot,
    permissions: Arc<PermissionBroker>,
    redactor: Arc<dyn Redactor>,
    workspace_tools: WorkspaceToolDispatcher,
    agent_tool_policy: AgentToolPolicy,
}

pub struct SdkWorkspaceSubagentRunnerFactory {
    engines: Arc<SdkSubagentEngineRegistry>,
}

impl SdkWorkspaceSubagentRunnerFactory {
    #[must_use]
    pub fn new(engines: Arc<SdkSubagentEngineRegistry>) -> Self {
        Self { engines }
    }
}

impl WorkspaceSubagentRunnerFactory for SdkWorkspaceSubagentRunnerFactory {
    fn create(
        &self,
        context: WorkspaceSubagentRunContext,
    ) -> Result<Arc<dyn SubagentRunner>, SubagentError> {
        let runtime = self.engines.get(context.parent_segment_id).ok_or_else(|| {
            SubagentError::Engine(
                "parent SDK runtime is no longer available for the subagent".into(),
            )
        })?;
        let event_store = Arc::clone(&context.event_store);
        let workspace_root = context.workspace_root.clone();
        let child_session_id = context.session_id;
        let policy = DelegationPolicy {
            max_depth: runtime.agent_tool_policy.max_depth,
            depth_cap: runtime.agent_tool_policy.max_depth,
            max_concurrent_children: runtime.agent_tool_policy.max_concurrent_subagents as usize,
            max_global_children: runtime.agent_tool_policy.max_concurrent_subagents as usize,
            ..DelegationPolicy::default()
        };
        Ok(Arc::new(
            DefaultSubagentRunner::new_with_engine_factory(
                Arc::new(SdkIsolatedSubagentEngineFactory { runtime, context }),
                event_store,
                workspace_root,
                policy,
            )
            .with_child_session_id(child_session_id)
            .with_external_lifecycle_owner(),
        ))
    }
}

struct SdkIsolatedSubagentEngineFactory {
    runtime: Arc<SdkSubagentRuntimeTemplate>,
    context: WorkspaceSubagentRunContext,
}

#[async_trait]
impl SubagentEngineFactory for SdkIsolatedSubagentEngineFactory {
    async fn run_child_engine(
        &self,
        request: ChildRunRequest,
    ) -> Result<ChildRunOutcome, SubagentError> {
        if request.tenant_id != self.context.tenant_id
            || request.child_session_id != self.context.session_id
        {
            return Err(SubagentError::Engine(
                "child engine request does not match the durable daemon scope".into(),
            ));
        }
        let lease = self
            .runtime
            .store
            .workspace_lease(self.context.workspace_lease_id)
            .map_err(|error| SubagentError::Engine(error.to_string()))?
            .ok_or_else(|| SubagentError::Engine("child workspace lease is missing".into()))?;
        if lease.task_id != self.context.child_task_id
            || lease.actor_id != self.context.actor_id
            || lease.state != WorkspaceLeaseState::Active
        {
            return Err(SubagentError::Engine(
                "child workspace lease no longer matches the daemon scope".into(),
            ));
        }
        let workspace_root =
            execution_root(&lease).map_err(|error| SubagentError::Engine(error.to_string()))?;
        if workspace_root != self.context.workspace_root {
            return Err(SubagentError::Engine(
                "child workspace root no longer matches the daemon scope".into(),
            ));
        }
        let isolation = LocalIsolation::for_current_platform();
        validate_daemon_segment_isolation(isolation)
            .map_err(|error| SubagentError::Engine(error.to_string()))?;
        let tool_registry = workspace_tool_registry(
            self.runtime.workspace_tools.clone(),
            lease.lease_id,
            workspace_root.clone(),
            isolation,
        )
        .map_err(|error| SubagentError::Engine(error.to_string()))?;
        let permission_broker = HarnessPermissionBroker::new(
            Arc::clone(&self.runtime.permissions),
            self.context.child_task_id,
            self.context.segment_id,
            PermissionRuntimeAuthority {
                workspace_lease_id: lease.lease_id,
                actor_id: lease.actor_id,
                execution_root: workspace_root.to_string_lossy().into_owned(),
                writable: lease.writable,
                sandbox_policy_hash: sandbox_policy_hash(
                    isolation,
                    lease.lease_id,
                    lease.actor_id,
                    &workspace_root,
                    lease.writable,
                ),
            },
        );
        let sandbox: Arc<dyn SandboxBackend> =
            Arc::new(LocalSandbox::new(&workspace_root).with_isolation(isolation));
        let authorization = daemon_authorization_runtime(
            permission_broker,
            Arc::clone(&sandbox),
            Arc::clone(&self.context.event_store),
            Arc::clone(&self.runtime.redactor),
            Arc::clone(&self.runtime.runtime_config.provider_credential_resolver),
        )
        .map_err(|error| SubagentError::Engine(error.to_string()))?;
        let execution_defaults = &self.runtime.runtime_config.execution_defaults;
        let permission_mode = effective_runtime_permission_mode(
            request.spec.permission_mode,
            execution_defaults.permission_mode,
        );
        let mcp_runtime = mcp_config_from_runtime_snapshot(
            &self.runtime.runtime_config,
            Arc::clone(&authorization.service),
            Arc::clone(&self.context.event_store),
            &workspace_root,
            self.context.session_id,
            request.child_run_id,
            permission_mode,
        )
        .await
        .map_err(|error| SubagentError::Engine(error.to_string()))?;
        let primary_result = async {
            let provider = &self.runtime.runtime_config.provider;
            let engine_factory = Arc::new(EngineBoundSubagentFactory::default());
            let harness_builder = Harness::builder()
                .with_workspace_root(&workspace_root)
                .with_model_arc(Arc::clone(&provider.provider))
                .with_store_arc(Arc::clone(&self.context.event_store))
                .with_sandbox_arc(sandbox)
                .with_tool_registry(tool_registry)
                .with_model_id(&provider.model_id)
                .with_permission_authority_arc(authorization.authority)
                .with_authorization_service_arc(authorization.service)
                .with_capability::<dyn ToolNetworkBrokerCap>(
                    ToolCapability::NetworkBroker,
                    authorization.network_broker,
                )
                .with_memory_database_path(&self.runtime.runtime_config.memory_database_path)
                .with_subagent_runner(Arc::clone(&self.context.subagent_runner))
                .with_subagent_engine_factory(Arc::clone(&engine_factory));
            let harness = apply_runtime_snapshot(
                harness_builder,
                &self.runtime.runtime_config,
                mcp_runtime.config(),
            )
            .map_err(|error| SubagentError::Engine(error.to_string()))?
            .build()
            .await
            .map_err(|error| SubagentError::Engine(error.to_string()))?;
            let options = SessionOptions::new(&workspace_root)
                .with_project_workspace_root(&self.runtime.runtime_config.workspace_root)
                .with_tenant_id(self.context.tenant_id)
                .with_session_id(self.context.session_id)
                .with_tool_profile(execution_defaults.tool_profile.clone())
                .with_model_id(&provider.model_id)
                .with_protocol(provider.protocol)
                .with_model_options(provider.model_options.clone())
                .with_agent_profiles(self.runtime.runtime_config.agent_profiles.clone())
                .with_context_compression_trigger_ratio(
                    execution_defaults.context_compression_trigger_ratio,
                )
                .with_permission_mode(permission_mode);
            let mut run_options = ConversationRunOptions::from_session_options(&options)
                .with_model_config_id(&provider.config_id)
                .with_model_id(&provider.model_id)
                .with_protocol(provider.protocol)
                .with_permission_mode(permission_mode)
                .with_model_options(provider.model_options.clone());
            run_options.agent_tool_policy = Some(self.runtime.agent_tool_policy.clone());
            harness
                .prepare_external_subagent_engine(options, run_options)
                .await
                .map_err(|error| SubagentError::Engine(error.to_string()))?;
            engine_factory.run_child_engine(request).await
        }
        .await;
        let shutdown_result = mcp_runtime
            .shutdown()
            .await
            .map_err(|error| SubagentError::Engine(error.to_string()));
        complete_after_mcp_shutdown(primary_result, shutdown_result)
    }
}

impl SdkRunCoordinatorFactory {
    #[must_use]
    pub fn new(
        store: Arc<TaskStore>,
        runtime_configs: RuntimeConfigResolver,
        blob_root: impl Into<PathBuf>,
        permissions: Arc<PermissionBroker>,
        redactor: Arc<dyn Redactor>,
    ) -> Self {
        Self::new_with_subagent_engines(
            store,
            runtime_configs,
            blob_root,
            permissions,
            redactor,
            Arc::new(SdkSubagentEngineRegistry::default()),
        )
    }

    #[must_use]
    pub fn new_with_subagent_engines(
        store: Arc<TaskStore>,
        runtime_configs: RuntimeConfigResolver,
        blob_root: impl Into<PathBuf>,
        permissions: Arc<PermissionBroker>,
        redactor: Arc<dyn Redactor>,
        subagent_engines: Arc<SdkSubagentEngineRegistry>,
    ) -> Self {
        Self {
            store,
            runtime_configs,
            blob_root: blob_root.into(),
            permissions,
            redactor,
            subagent_engines,
            segments: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn running_segment(segment_id: RunSegmentId, shared: SharedSegment) -> RunningSegment {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut terminal = shared.terminal;
        tokio::spawn(async move {
            if let Some(event) = terminal.borrow().clone() {
                let _ = sender.send(event);
                return;
            }
            while terminal.changed().await.is_ok() {
                if let Some(event) = terminal.borrow().clone() {
                    let _ = sender.send(event);
                    return;
                }
            }
        });
        RunningSegment::with_control(segment_id, receiver, shared.control)
    }

    async fn execute_segment(
        store: Arc<TaskStore>,
        runtime_configs: RuntimeConfigResolver,
        blob_root: PathBuf,
        permissions: Arc<PermissionBroker>,
        redactor: Arc<dyn Redactor>,
        request: StartSegmentRequest,
        workspace_tools: WorkspaceToolDispatcher,
        subagent_runner: Arc<dyn SubagentRunner>,
        agent_starters: AgentStarterCapabilities,
        subagent_engines: Arc<SdkSubagentEngineRegistry>,
        control: RunControlHandle,
    ) -> Result<(), SdkRunFactoryError> {
        let lease_id = request
            .input
            .workspace_lease_id
            .ok_or(SdkRunFactoryError::WorkspaceLeaseMissing)?;
        let lease = store
            .workspace_lease(lease_id)
            .map_err(|error| SdkRunFactoryError::Workspace(error.to_string()))?
            .ok_or(SdkRunFactoryError::WorkspaceLeaseNotFound)?;
        if lease.task_id != request.task_id {
            return Err(SdkRunFactoryError::WorkspaceLeaseTaskMismatch);
        }
        if lease.state != WorkspaceLeaseState::Active {
            return Err(SdkRunFactoryError::WorkspaceLeaseInactive);
        }
        let workspace_root = execution_root(&lease)?;
        let event_store: Arc<dyn EventStore> = Arc::new(
            TaskEventStoreAdapter::new(
                Arc::clone(&store),
                request.task_id,
                TenantId::SINGLE,
                request.input.session_id,
                Arc::clone(&redactor),
            )
            .with_run_segment_id(request.segment_id),
        );
        let replay_calls =
            apply_indeterminate_tool_decisions(event_store.as_ref(), &request).await?;
        let runtime_config = runtime_configs
            .resolve(&workspace_root, request.input.model_config_id.as_deref())
            .map_err(|error| SdkRunFactoryError::RuntimeConfig(error.to_string()))?;
        let provider = &runtime_config.provider;
        let execution_defaults = &runtime_config.execution_defaults;
        let permission_mode = effective_runtime_permission_mode(
            request.input.permission_mode,
            execution_defaults.permission_mode,
        );
        let model: Arc<dyn ModelProvider> = if replay_calls.is_empty() {
            Arc::clone(&provider.provider)
        } else {
            Arc::new(ReplayFirstModelProvider::new(
                Arc::clone(&provider.provider),
                replay_calls,
            ))
        };
        let isolation = LocalIsolation::for_current_platform();
        validate_daemon_segment_isolation(isolation)?;
        let tool_registry = workspace_tool_registry(
            workspace_tools.clone(),
            lease_id,
            workspace_root.clone(),
            isolation,
        )
        .map_err(|error| SdkRunFactoryError::Sdk(error.to_string()))?;
        let permission_broker = HarnessPermissionBroker::new(
            Arc::clone(&permissions),
            request.task_id,
            request.segment_id,
            PermissionRuntimeAuthority {
                workspace_lease_id: lease.lease_id,
                actor_id: lease.actor_id,
                execution_root: workspace_root.to_string_lossy().into_owned(),
                writable: lease.writable,
                sandbox_policy_hash: sandbox_policy_hash(
                    isolation,
                    lease.lease_id,
                    lease.actor_id,
                    &workspace_root,
                    lease.writable,
                ),
            },
        );
        let sandbox: Arc<dyn SandboxBackend> =
            Arc::new(LocalSandbox::new(&workspace_root).with_isolation(isolation));
        let authorization = daemon_authorization_runtime(
            permission_broker,
            Arc::clone(&sandbox),
            Arc::clone(&event_store),
            Arc::clone(&redactor),
            Arc::clone(&runtime_config.provider_credential_resolver),
        )?;
        let mcp_runtime = mcp_config_from_runtime_snapshot(
            &runtime_config,
            Arc::clone(&authorization.service),
            Arc::clone(&event_store),
            &workspace_root,
            request.input.session_id,
            request.input.run_id,
            permission_mode,
        )
        .await?;
        let primary_result = async {
            let agent_tool_policy = daemon_agent_tool_policy(&execution_defaults)?;
            let subagents_enabled = agent_tool_policy.subagents == AgentUsePolicy::Allowed;
            let memory_database_path = runtime_config.memory_database_path.clone();
            runtime_config
                .ensure_memory_parent()
                .map_err(|error| SdkRunFactoryError::Sdk(error.to_string()))?;
            let _runtime_binding = subagents_enabled.then(|| {
                subagent_engines.bind(
                    request.segment_id,
                    Arc::new(SdkSubagentRuntimeTemplate {
                        store: Arc::clone(&store),
                        runtime_config: runtime_config.clone(),
                        permissions: Arc::clone(&permissions),
                        redactor: Arc::clone(&redactor),
                        workspace_tools: workspace_tools.clone(),
                        agent_tool_policy: agent_tool_policy.clone(),
                    }),
                )
            });
            let harness_builder = Harness::builder()
                .with_workspace_root(&workspace_root)
                .with_model_arc(model)
                .with_store_arc(Arc::clone(&event_store))
                .with_sandbox_arc(sandbox)
                .with_tool_registry(tool_registry)
                .with_model_id(&provider.model_id)
                .with_permission_authority_arc(authorization.authority)
                .with_authorization_service_arc(authorization.service)
                .with_capability::<dyn ToolNetworkBrokerCap>(
                    ToolCapability::NetworkBroker,
                    authorization.network_broker,
                )
                .with_memory_database_path(memory_database_path);
            let harness_builder = if subagents_enabled {
                harness_builder.with_subagent_runner(subagent_runner)
            } else {
                harness_builder
            };
            let harness_builder = if agent_tool_policy.background_agents == AgentUsePolicy::Allowed
            {
                harness_builder.with_capability::<dyn harness_contracts::BackgroundAgentStarterCap>(
                    harness_contracts::ToolCapability::Custom(
                        harness_contracts::BACKGROUND_AGENT_STARTER_CAPABILITY.to_owned(),
                    ),
                    agent_starters.background,
                )
            } else {
                harness_builder
            };
            let harness_builder = if agent_tool_policy.agent_team == AgentUsePolicy::Allowed {
                harness_builder.with_capability::<dyn harness_contracts::AgentTeamStarterCap>(
                    harness_contracts::ToolCapability::Custom(
                        harness_contracts::AGENT_TEAM_STARTER_CAPABILITY.to_owned(),
                    ),
                    agent_starters.team,
                )
            } else {
                harness_builder
            };
            let harness =
                apply_runtime_snapshot(harness_builder, &runtime_config, mcp_runtime.config())
                    .map_err(|error| SdkRunFactoryError::Sdk(error.to_string()))?
                    .build()
                    .await
                    .map_err(|error| SdkRunFactoryError::Sdk(error.to_string()))?;

            let session_options = SessionOptions::new(&workspace_root)
                .with_project_workspace_root(&runtime_config.workspace_root)
                .with_tenant_id(TenantId::SINGLE)
                .with_session_id(request.input.session_id)
                .with_tool_profile(execution_defaults.tool_profile.clone())
                .with_model_id(&provider.model_id)
                .with_protocol(provider.protocol)
                .with_model_options(provider.model_options.clone())
                .with_agent_profiles(runtime_config.agent_profiles.clone())
                .with_context_compression_trigger_ratio(
                    execution_defaults.context_compression_trigger_ratio,
                )
                .with_permission_mode(permission_mode);
            harness
                .open_or_create_conversation_session(session_options.clone())
                .await
                .map_err(|error| SdkRunFactoryError::Sdk(error.to_string()))?;

            let mut input = ConversationTurnInput::ask(request.input.content);
            input.client_message_id = Some(request.segment_id.to_string());
            input.context_references = request
                .input
                .context_references
                .into_iter()
                .map(|path| ConversationContextReference::WorkspaceFile {
                    label: path.clone(),
                    path,
                })
                .collect();
            input.attachments = load_attachments(
                &store,
                request.task_id,
                &blob_root,
                &request.input.attachments,
            )?;
            let mut run_options = ConversationRunOptions::from_session_options(&session_options)
                .with_model_config_id(provider.config_id.clone())
                .with_model_id(provider.model_id.clone())
                .with_protocol(provider.protocol)
                .with_permission_mode(permission_mode)
                .with_model_options(provider.model_options.clone());
            run_options.agent_tool_policy = Some(agent_tool_policy);
            harness
                .submit_conversation_turn_with_run_control(
                    ConversationTurnRequest {
                        options: session_options,
                        run_options,
                        input,
                        permission_actor_source: None,
                    },
                    request.input.run_id,
                    control,
                )
                .await
                .map_err(|error| SdkRunFactoryError::Sdk(error.to_string()))
        }
        .await;
        let shutdown_result = mcp_runtime.shutdown().await;
        complete_after_mcp_shutdown(primary_result.map(|_| ()), shutdown_result)
    }
}

fn complete_after_mcp_shutdown<T, E>(
    primary: Result<T, E>,
    shutdown: Result<(), E>,
) -> Result<T, E> {
    match primary {
        Err(error) => Err(error),
        Ok(value) => shutdown.map(|()| value),
    }
}

struct WorkspaceDispatchedTool {
    inner: Arc<dyn Tool>,
    workspace_tools: WorkspaceToolDispatcher,
    lease_id: WorkspaceLeaseId,
    workspace_root: PathBuf,
    isolation: LocalIsolation,
}

struct InterruptOnDrop {
    interrupt: jyowo_harness_sdk::ext::InterruptToken,
    armed: bool,
}

impl InterruptOnDrop {
    fn new(interrupt: jyowo_harness_sdk::ext::InterruptToken) -> Self {
        Self {
            interrupt,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for InterruptOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.interrupt.interrupt();
        }
    }
}

struct WorkspaceToolEventStream {
    receiver: mpsc::UnboundedReceiver<jyowo_harness_sdk::ext::ToolEvent>,
    interrupt: jyowo_harness_sdk::ext::InterruptToken,
    completed: bool,
}

impl Stream for WorkspaceToolEventStream {
    type Item = jyowo_harness_sdk::ext::ToolEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.receiver).poll_recv(cx) {
            Poll::Ready(Some(event))
                if matches!(
                    event,
                    jyowo_harness_sdk::ext::ToolEvent::Final(_)
                        | jyowo_harness_sdk::ext::ToolEvent::Error(_)
                ) =>
            {
                self.completed = true;
                Poll::Ready(Some(event))
            }
            Poll::Ready(None) => {
                self.completed = true;
                Poll::Ready(None)
            }
            other => other,
        }
    }
}

impl Drop for WorkspaceToolEventStream {
    fn drop(&mut self) {
        if !self.completed {
            self.interrupt.interrupt();
        }
    }
}

#[async_trait::async_trait]
impl Tool for WorkspaceDispatchedTool {
    fn descriptor(&self) -> &ToolDescriptor {
        self.inner.descriptor()
    }

    async fn resolve_schema(
        &self,
        ctx: &SchemaResolverContext,
    ) -> Result<serde_json::Value, ToolError> {
        self.inner.resolve_schema(ctx).await
    }

    async fn validate(
        &self,
        input: &serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<(), ValidationError> {
        self.inner.validate(input, ctx).await
    }

    async fn plan(
        &self,
        input: &serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolActionPlan, ToolError> {
        self.inner.plan(input, ctx).await
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let actions = workspace_actions(authorized.action_plan(), &self.workspace_root);
        if actions.is_empty() {
            return self.inner.execute_authorized(authorized, ctx).await;
        }
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        let (ready_sender, ready_receiver) = oneshot::channel();
        let ready_sender = Arc::new(Mutex::new(Some(ready_sender)));
        let task_ready_sender = Arc::clone(&ready_sender);
        let workspace_tools = self.workspace_tools.clone();
        let lease_id = self.lease_id;
        let inner = Arc::clone(&self.inner);
        let isolation = self.isolation;
        let interrupt = ctx.interrupt.clone();
        let mut interrupt_on_drop = InterruptOnDrop::new(interrupt.clone());
        tokio::spawn(async move {
            let result = dispatch_tool_to_channel(
                workspace_tools,
                lease_id,
                actions,
                inner,
                authorized,
                ctx,
                event_sender.clone(),
                Arc::clone(&task_ready_sender),
                isolation,
                Vec::new(),
            )
            .await;
            if let Err(error) = result {
                if let Some(sender) = task_ready_sender
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                {
                    let _ = sender.send(Err(error));
                } else {
                    let _ = event_sender.send(jyowo_harness_sdk::ext::ToolEvent::Error(error));
                }
            }
        });
        let ready = ready_receiver.await;
        interrupt_on_drop.disarm();
        match ready {
            Ok(Ok(())) => Ok(Box::pin(WorkspaceToolEventStream {
                receiver: event_receiver,
                interrupt,
                completed: false,
            })),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(ToolError::Message(
                "workspace-dispatched tool stopped before execution".into(),
            )),
        }
    }
}

fn workspace_tool_registry(
    workspace_tools: WorkspaceToolDispatcher,
    lease_id: WorkspaceLeaseId,
    workspace_root: PathBuf,
    isolation: LocalIsolation,
) -> Result<ToolRegistry, jyowo_harness_sdk::ext::RegistrationError> {
    let registry = ToolRegistry::builder().build()?;
    registry.wrap_tools(|inner| {
        Arc::new(WorkspaceDispatchedTool {
            inner,
            workspace_tools: workspace_tools.clone(),
            lease_id,
            workspace_root: workspace_root.clone(),
            isolation,
        })
    })?;
    Ok(registry)
}

fn workspace_actions(plan: &ToolActionPlan, workspace_root: &Path) -> Vec<WorkspaceToolAction> {
    let mut actions = Vec::new();
    let command_requires_write =
        matches!(plan.workspace_access, ToolWorkspaceAccess::ReadWrite { .. });
    for resource in &plan.resources {
        let action = match resource {
            ActionResource::FileRead { path } => WorkspaceToolAction::ReadPath(path.clone()),
            ActionResource::FileWrite { path, .. } | ActionResource::FileDelete { path } => {
                WorkspaceToolAction::WritePath(path.clone())
            }
            ActionResource::Command { cwd, .. } => WorkspaceToolAction::Command {
                cwd: cwd.clone().unwrap_or_else(|| workspace_root.to_path_buf()),
                requires_write: command_requires_write,
            },
            _ => continue,
        };
        if !actions.contains(&action) {
            actions.push(action);
        }
    }
    actions
}

#[allow(clippy::too_many_arguments)]
fn dispatch_tool_to_channel(
    workspace_tools: WorkspaceToolDispatcher,
    lease_id: WorkspaceLeaseId,
    mut actions: Vec<WorkspaceToolAction>,
    inner: Arc<dyn Tool>,
    authorized: AuthorizedToolInput,
    ctx: ToolContext,
    event_sender: mpsc::UnboundedSender<jyowo_harness_sdk::ext::ToolEvent>,
    ready_sender: Arc<Mutex<Option<oneshot::Sender<Result<(), ToolError>>>>>,
    isolation: LocalIsolation,
    authorizations: Vec<(WorkspaceToolAction, crate::WorkspaceToolAuthorization)>,
) -> BoxFuture<'static, Result<(), ToolError>> {
    async move {
        if actions.is_empty() {
            let has_filesystem_authorization = authorizations.iter().any(|(action, _)| {
                matches!(
                    action,
                    WorkspaceToolAction::ReadPath(_) | WorkspaceToolAction::WritePath(_)
                )
            });
            let mut events = if has_filesystem_authorization {
                execute_workspace_file_tool(
                    &authorized.action_plan().tool_name,
                    &authorized,
                    authorizations,
                )?
            } else {
                inner.execute_authorized(authorized, ctx).await?
            };
            if let Some(sender) = ready_sender
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                let _ = sender.send(Ok(()));
            }
            while let Some(event) = events.next().await {
                let _ = event_sender.send(event);
            }
            return Ok(());
        }
        let action = actions.remove(0);
        match action {
            WorkspaceToolAction::Command {
                cwd,
                requires_write,
            } => {
                let nested_workspace_tools = workspace_tools.clone();
                let execute = move |_| {
                    dispatch_tool_to_channel(
                        nested_workspace_tools,
                        lease_id,
                        actions,
                        inner,
                        authorized,
                        ctx,
                        event_sender,
                        ready_sender,
                        isolation,
                        authorizations,
                    )
                };
                workspace_tools
                    .dispatch_sandboxed_command(lease_id, cwd, requires_write, isolation, execute)
                    .await
            }
            action => {
                let nested_workspace_tools = workspace_tools.clone();
                let dispatched_action = action.clone();
                let execute = move |authorization| {
                    let mut authorizations = authorizations;
                    authorizations.push((dispatched_action, authorization));
                    dispatch_tool_to_channel(
                        nested_workspace_tools,
                        lease_id,
                        actions,
                        inner,
                        authorized,
                        ctx,
                        event_sender,
                        ready_sender,
                        isolation,
                        authorizations,
                    )
                };
                workspace_tools.dispatch(lease_id, action, execute).await
            }
        }
        .map_err(|error| ToolError::PermissionDenied(error.to_string()))?
    }
    .boxed()
}

fn execute_workspace_file_tool(
    tool_name: &str,
    authorized: &AuthorizedToolInput,
    authorizations: Vec<(WorkspaceToolAction, crate::WorkspaceToolAuthorization)>,
) -> Result<ToolStream, ToolError> {
    let mut filesystem_authorizations = authorizations.into_iter().filter(|(action, _)| {
        matches!(
            action,
            WorkspaceToolAction::ReadPath(_) | WorkspaceToolAction::WritePath(_)
        )
    });
    let Some((action, authorization)) = filesystem_authorizations.next() else {
        return Err(ToolError::PermissionDenied(
            "workspace filesystem authorization missing".into(),
        ));
    };
    if filesystem_authorizations.next().is_some() {
        return Err(ToolError::PermissionDenied(
            "workspace filesystem tool requested multiple paths without a secure adapter".into(),
        ));
    }
    let input = authorized.raw_input();
    let final_result = match (tool_name, action) {
        ("FileRead", WorkspaceToolAction::ReadPath(_)) => {
            let bytes = authorization.read_bytes().map_err(workspace_tool_error)?;
            let content =
                String::from_utf8(bytes).map_err(|error| ToolError::Message(error.to_string()))?;
            let start_line = positive_line_number(input, "start_line")?.unwrap_or(1);
            let end_line = positive_line_number(input, "end_line")?
                .unwrap_or(u64::MAX)
                .max(start_line);
            let content = content
                .lines()
                .enumerate()
                .filter_map(|(index, line)| {
                    let line_number = index as u64 + 1;
                    (line_number >= start_line && line_number <= end_line).then_some(line)
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            ToolResult::Text(content)
        }
        ("FileWrite", WorkspaceToolAction::WritePath(_)) => {
            let content = required_input_string(input, "content")?;
            verify_authorized_write_hash(authorized, content.as_bytes())?;
            authorization
                .write_bytes(content.as_bytes())
                .map_err(workspace_tool_error)?;
            ToolResult::Structured(serde_json::json!({
                "path": authorized_filesystem_path(authorized, true)?,
                "bytes": content.len(),
            }))
        }
        ("FileEdit", WorkspaceToolAction::WritePath(_)) => {
            verify_authorized_edit_hash(authorized, input)?;
            let old = required_input_string(input, "old")?;
            let new = required_input_string(input, "new")?;
            let replace_all = input
                .get("replace_all")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let replacements = authorization
                .edit_bytes(|bytes| {
                    let content = std::str::from_utf8(bytes).map_err(|error| {
                        std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                    })?;
                    let replacements = if replace_all {
                        content.matches(old).count()
                    } else {
                        usize::from(content.contains(old))
                    };
                    let edited = if replace_all {
                        content.replace(old, new)
                    } else {
                        content.replacen(old, new, 1)
                    };
                    Ok((edited.into_bytes(), replacements))
                })
                .map_err(workspace_tool_error)?;
            ToolResult::Structured(serde_json::json!({
                "path": authorized_filesystem_path(authorized, true)?,
                "replacements": replacements,
            }))
        }
        _ => {
            return Err(ToolError::PermissionDenied(format!(
                "tool {tool_name} has no secure workspace filesystem adapter"
            )))
        }
    };
    Ok(Box::pin(stream::iter([
        jyowo_harness_sdk::ext::ToolEvent::Final(final_result),
    ])))
}

fn positive_line_number(input: &serde_json::Value, field: &str) -> Result<Option<u64>, ToolError> {
    let Some(value) = input.get(field) else {
        return Ok(None);
    };
    let value = value
        .as_u64()
        .filter(|value| *value > 0)
        .ok_or_else(|| ToolError::Validation(format!("{field} must be a positive integer")))?;
    Ok(Some(value))
}

fn required_input_string<'a>(
    input: &'a serde_json::Value,
    field: &str,
) -> Result<&'a str, ToolError> {
    input
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ToolError::Validation(format!("{field} is required")))
}

fn authorized_filesystem_path(
    authorized: &AuthorizedToolInput,
    write: bool,
) -> Result<PathBuf, ToolError> {
    authorized
        .action_plan()
        .resources
        .iter()
        .find_map(|resource| match resource {
            ActionResource::FileWrite { path, .. } if write => Some(path.clone()),
            ActionResource::FileRead { path } if !write => Some(path.clone()),
            _ => None,
        })
        .ok_or_else(|| ToolError::PermissionDenied("authorized filesystem path missing".into()))
}

fn verify_authorized_write_hash(
    authorized: &AuthorizedToolInput,
    authorized_bytes: &[u8],
) -> Result<(), ToolError> {
    let expected = authorized
        .action_plan()
        .resources
        .iter()
        .find_map(|resource| match resource {
            ActionResource::FileWrite { content_hash, .. } => Some(content_hash.as_str()),
            _ => None,
        })
        .ok_or_else(|| ToolError::PermissionDenied("authorized content hash missing".into()))?;
    let actual = blake3::hash(authorized_bytes).to_hex();
    if actual.as_str() != expected {
        return Err(ToolError::PermissionDenied(
            "authorized content hash does not match tool input".into(),
        ));
    }
    Ok(())
}

fn verify_authorized_edit_hash(
    authorized: &AuthorizedToolInput,
    input: &serde_json::Value,
) -> Result<(), ToolError> {
    let encoded =
        serde_json::to_vec(input).map_err(|error| ToolError::Message(error.to_string()))?;
    let expected = authorized
        .action_plan()
        .resources
        .iter()
        .find_map(|resource| match resource {
            ActionResource::FileWrite { content_hash, .. } => Some(content_hash.as_str()),
            _ => None,
        })
        .ok_or_else(|| ToolError::PermissionDenied("authorized edit hash missing".into()))?;
    if blake3::hash(&encoded).to_hex().as_str() != expected {
        return Err(ToolError::PermissionDenied(
            "authorized edit hash does not match tool input".into(),
        ));
    }
    Ok(())
}

fn workspace_tool_error(error: crate::WorkspaceCoordinatorError) -> ToolError {
    ToolError::PermissionDenied(error.to_string())
}

fn sandbox_policy_hash(
    isolation: LocalIsolation,
    lease_id: harness_contracts::WorkspaceLeaseId,
    actor_id: harness_contracts::ActorId,
    execution_root: &Path,
    writable: bool,
) -> String {
    let isolation = match isolation {
        LocalIsolation::None => "none",
        LocalIsolation::Bubblewrap => "bubblewrap",
        LocalIsolation::Seatbelt => "seatbelt",
        LocalIsolation::JobObject => "job_object",
    };
    let policy = format!(
        "local-sandbox-v1\0{isolation}\0{lease_id}\0{actor_id}\0{}\0{writable}",
        execution_root.to_string_lossy()
    );
    blake3::hash(policy.as_bytes()).to_hex().to_string()
}

fn validate_daemon_segment_isolation(isolation: LocalIsolation) -> Result<(), SdkRunFactoryError> {
    if isolation == LocalIsolation::None {
        Err(SdkRunFactoryError::WorkspaceSandboxUnavailable)
    } else {
        Ok(())
    }
}

fn daemon_agent_tool_policy(
    defaults: &ExecutionDefaultsRecord,
) -> Result<AgentToolPolicy, SdkRunFactoryError> {
    harness_contracts::validate_execution_defaults_dependencies(defaults)
        .map_err(|error| SdkRunFactoryError::ExecutionDefaults(error.to_string()))?;
    let teams_enabled = defaults.agent_teams_enabled;
    Ok(AgentToolPolicy {
        subagents: if defaults.subagents_enabled {
            AgentUsePolicy::Allowed
        } else {
            AgentUsePolicy::Off
        },
        agent_team: if teams_enabled {
            AgentUsePolicy::Allowed
        } else {
            AgentUsePolicy::Off
        },
        background_agents: if defaults.background_agents_enabled {
            AgentUsePolicy::Allowed
        } else {
            AgentUsePolicy::Off
        },
        team_config: teams_enabled.then(|| AgentTeamRunConfig {
            topology: AgentTeamTopology::CoordinatorWorker,
            lead_profile_id: "reviewer".to_owned(),
            member_profile_ids: vec!["worker".to_owned()],
            max_turns_per_goal: 8,
            shared_memory_policy: AgentTeamSharedMemoryPolicy::SummariesOnly,
        }),
        workspace_isolation: AgentWorkspaceIsolationMode::GitWorktree,
        max_depth: 4,
        max_concurrent_subagents: 8,
        max_team_members: if teams_enabled { 2 } else { 0 },
    })
}

fn effective_runtime_permission_mode(
    requested: PermissionMode,
    configured: PermissionMode,
) -> PermissionMode {
    if requested == PermissionMode::Default {
        configured
    } else {
        requested
    }
}

impl RunCoordinatorFactory for SdkRunCoordinatorFactory {
    fn spawn_idempotent(
        &self,
        request: StartSegmentRequest,
        workspace_tools: WorkspaceToolDispatcher,
        subagent_runner: Arc<dyn SubagentRunner>,
        agent_starters: AgentStarterCapabilities,
    ) -> RunningSegment {
        let key = (request.task_id, request.segment_id);
        let request_digest = segment_request_digest(&request);
        let (shared, start) = {
            let mut segments = self
                .segments
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match segments.entry(key) {
                Entry::Occupied(entry) => (entry.get().clone(), None),
                Entry::Vacant(entry) => {
                    let claim = self.store.claim_segment_execution(
                        request.task_id,
                        request.segment_id,
                        &request_digest,
                    );
                    match claim {
                        Ok(SegmentExecutionClaim::Completed(terminal)) => {
                            return completed_running_segment(request.segment_id, terminal);
                        }
                        Ok(SegmentExecutionClaim::InProgress) => {
                            let terminal = SegmentExecutionTerminal {
                                terminal_reason: RunTerminalReason::Failed,
                                incomplete_output: true,
                                ended_at: Utc::now(),
                            };
                            if let Err(error) = self.store.complete_segment_execution(
                                request.task_id,
                                request.segment_id,
                                &request_digest,
                                &terminal,
                            ) {
                                tracing::error!(
                                    task_id = %request.task_id,
                                    segment_id = %request.segment_id,
                                    error = %error,
                                    "recovered SDK segment completion failed"
                                );
                                return closed_running_segment(request.segment_id);
                            }
                            return completed_running_segment(request.segment_id, terminal);
                        }
                        Err(error) => {
                            tracing::error!(
                                task_id = %request.task_id,
                                segment_id = %request.segment_id,
                                error = %error,
                                "durable SDK segment claim failed"
                            );
                            return closed_running_segment(request.segment_id);
                        }
                        Ok(SegmentExecutionClaim::Claimed) => {}
                    }
                    let control = RunControlHandle::new();
                    let (terminal_sender, terminal) = watch::channel(None);
                    let shared = SharedSegment {
                        control: control.clone(),
                        terminal,
                    };
                    entry.insert(shared.clone());
                    (shared, Some((control, terminal_sender)))
                }
            }
        };
        if let Some((control, terminal_sender)) = start {
            let store = Arc::clone(&self.store);
            let runtime_configs = self.runtime_configs.clone();
            let blob_root = self.blob_root.clone();
            let permissions = Arc::clone(&self.permissions);
            let redactor = Arc::clone(&self.redactor);
            let subagent_engines = Arc::clone(&self.subagent_engines);
            let segments = Arc::clone(&self.segments);
            let request_digest = request_digest.clone();
            tokio::spawn(async move {
                let task_id = request.task_id;
                let segment_id = request.segment_id;
                let execution_control = control.clone();
                let result = Self::execute_segment(
                    Arc::clone(&store),
                    runtime_configs,
                    blob_root,
                    permissions,
                    redactor,
                    request,
                    workspace_tools,
                    subagent_runner,
                    agent_starters,
                    subagent_engines,
                    control,
                )
                .await;
                let execution_failed = if let Err(error) = result {
                    tracing::error!(
                        %task_id,
                        %segment_id,
                        error_kind = error.diagnostic_kind(),
                        "SDK segment failed"
                    );
                    true
                } else {
                    false
                };
                let terminal_reason = match segment_terminal_reason(
                    &store,
                    task_id,
                    segment_id,
                    execution_control.finished_outcome(),
                    execution_failed,
                ) {
                    Ok(reason) => reason,
                    Err(error) => {
                        tracing::error!(%task_id, %segment_id, error = %error, "durable SDK terminal classification failed");
                        RunTerminalReason::Failed
                    }
                };
                let terminal = SegmentExecutionTerminal {
                    incomplete_output: terminal_reason != RunTerminalReason::Completed,
                    terminal_reason,
                    ended_at: Utc::now(),
                };
                let completion = store.complete_segment_execution(
                    task_id,
                    segment_id,
                    &request_digest,
                    &terminal,
                );
                if let Err(error) = &completion {
                    tracing::error!(%task_id, %segment_id, error = %error, "durable SDK segment completion failed");
                }
                if completion.is_ok() {
                    let _ = terminal_sender.send(Some(RunCoordinatorEvent::Completed {
                        segment_id,
                        terminal_reason: terminal.terminal_reason,
                        incomplete_output: terminal.incomplete_output,
                        ended_at: terminal.ended_at,
                    }));
                }
                segments
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&(task_id, segment_id));
            });
        }
        Self::running_segment(key.1, shared)
    }
}

fn run_terminal_reason(
    outcome: Option<TurnOutcome>,
    superseded: bool,
    execution_failed: bool,
) -> RunTerminalReason {
    if execution_failed {
        return RunTerminalReason::Failed;
    }
    match outcome {
        None => RunTerminalReason::Completed,
        Some(TurnOutcome::YieldedAtSafePoint) if superseded => RunTerminalReason::Superseded,
        Some(TurnOutcome::YieldedAtSafePoint) => RunTerminalReason::Cancelled,
        Some(TurnOutcome::ForceStopped { .. }) => RunTerminalReason::ForcedInterruption,
        Some(TurnOutcome::ForceStopTimedOut { .. }) => RunTerminalReason::Failed,
    }
}

fn segment_terminal_reason(
    store: &TaskStore,
    task_id: TaskId,
    segment_id: RunSegmentId,
    outcome: Option<TurnOutcome>,
    execution_failed: bool,
) -> Result<RunTerminalReason, SdkRunFactoryError> {
    let projection = store
        .task_projection(task_id)
        .map_err(|error| SdkRunFactoryError::DurableTerminal(error.to_string()))?;
    let projected_superseded = projection.is_some_and(|projection| {
        projection.current_run.as_ref().is_some_and(|run| {
            run.segment_id == segment_id && run.promotion_mode == Some(PromotionMode::SafePoint)
        }) && projection
            .queue
            .iter()
            .any(|item| item.state == QueueItemState::Promoting)
    });
    if let Some(reason) = durable_run_terminal_reason(store, task_id, segment_id)? {
        return Ok(reason);
    }
    Ok(run_terminal_reason(
        outcome,
        projected_superseded,
        execution_failed,
    ))
}

fn durable_run_terminal_reason(
    store: &TaskStore,
    task_id: TaskId,
    segment_id: RunSegmentId,
) -> Result<Option<RunTerminalReason>, SdkRunFactoryError> {
    store
        .run_terminal_reason(task_id, segment_id)
        .map_err(|error| SdkRunFactoryError::DurableTerminal(error.to_string()))
}

fn segment_request_digest(request: &StartSegmentRequest) -> String {
    let body = serde_json::to_vec(&json!({
        "taskId": request.task_id,
        "segmentId": request.segment_id,
        "input": request.input,
        "indeterminateTools": request.indeterminate_tools,
    }))
    .expect("segment request contracts serialize");
    blake3::hash(&body).to_hex().to_string()
}

fn completed_running_segment(
    segment_id: RunSegmentId,
    terminal: SegmentExecutionTerminal,
) -> RunningSegment {
    let (sender, receiver) = mpsc::unbounded_channel();
    let _ = sender.send(RunCoordinatorEvent::Completed {
        segment_id,
        terminal_reason: terminal.terminal_reason,
        incomplete_output: terminal.incomplete_output,
        ended_at: terminal.ended_at,
    });
    RunningSegment::new(receiver)
}

fn closed_running_segment(_segment_id: RunSegmentId) -> RunningSegment {
    let (sender, receiver) = mpsc::unbounded_channel();
    drop(sender);
    RunningSegment::new(receiver)
}

fn execution_root(
    lease: &harness_journal::TaskWorkspaceLease,
) -> Result<PathBuf, SdkRunFactoryError> {
    let root = match lease.mode {
        WorkspaceMode::Current => Path::new(&lease.canonical_root),
        WorkspaceMode::ManagedWorktree => lease
            .worktree_path
            .as_deref()
            .map(Path::new)
            .ok_or(SdkRunFactoryError::ManagedWorkspacePathMissing)?,
    };
    Ok(root.to_path_buf())
}

fn load_attachments(
    store: &Arc<TaskStore>,
    task_id: TaskId,
    blob_root: &Path,
    blob_ids: &[harness_contracts::BlobId],
) -> Result<Vec<ConversationAttachmentReference>, SdkRunFactoryError> {
    if blob_ids.is_empty() {
        return Ok(Vec::new());
    }
    let blobs = TaskBlobStore::open(Arc::clone(store), task_id, blob_root)
        .map_err(|error| SdkRunFactoryError::Attachment(error.to_string()))?;
    blob_ids
        .iter()
        .map(|blob_id| {
            let blob = match blobs
                .read(blob_id)
                .map_err(|error| SdkRunFactoryError::Attachment(error.to_string()))?
            {
                harness_journal::BlobRead::Available { blob, .. } => blob,
                harness_journal::BlobRead::Missing { .. } => {
                    return Err(SdkRunFactoryError::AttachmentMissing)
                }
            };
            let mime_type = blob
                .content_type
                .clone()
                .unwrap_or_else(|| "application/octet-stream".to_owned());
            Ok(ConversationAttachmentReference {
                id: blob_id.to_string(),
                name: blob_id.to_string(),
                mime_type,
                size_bytes: blob.size,
                blob_ref: blob,
            })
        })
        .collect()
}

async fn apply_indeterminate_tool_decisions(
    event_store: &dyn EventStore,
    request: &StartSegmentRequest,
) -> Result<Vec<ReplayToolCall>, SdkRunFactoryError> {
    let mut failures = Vec::new();
    let mut replay_tool_use_ids = Vec::new();
    for decision in &request.indeterminate_tools {
        let tool_use_id = ToolUseId::parse(&decision.tool_use_id)
            .map_err(|error| SdkRunFactoryError::RecoveryDecision(error.to_string()))?;
        match decision.resolution {
            IndeterminateToolResolution::TreatAsFailed => {
                failures.push(Event::ToolUseFailed(ToolUseFailedEvent {
                    tool_use_id,
                    error: ToolErrorPayload {
                        code: "indeterminate_treated_as_failed".into(),
                        message: "tool outcome was indeterminate after daemon recovery".into(),
                        retriable: false,
                    },
                    at: Utc::now(),
                }));
            }
            IndeterminateToolResolution::ExecuteAgain => replay_tool_use_ids.push(tool_use_id),
        }
    }
    if !failures.is_empty() {
        event_store
            .append_with_metadata(
                TenantId::SINGLE,
                request.input.session_id,
                AppendMetadata {
                    run_id: Some(request.input.run_id),
                    ..AppendMetadata::default()
                },
                &failures,
            )
            .await
            .map_err(|error| SdkRunFactoryError::RecoveryDecision(error.to_string()))?;
    }
    if replay_tool_use_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut requested_calls = HashMap::new();
    let mut events = event_store
        .read(
            TenantId::SINGLE,
            request.input.session_id,
            ReplayCursor::FromStart,
        )
        .await
        .map_err(|error| SdkRunFactoryError::RecoveryDecision(error.to_string()))?;
    while let Some(event) = events.next().await {
        if let Event::ToolUseRequested(requested) = event {
            requested_calls
                .entry(requested.tool_use_id)
                .or_insert(ReplayToolCall {
                    tool_use_id: requested.tool_use_id,
                    tool_name: requested.tool_name,
                    input: requested.input,
                });
        }
    }
    replay_tool_use_ids
        .into_iter()
        .map(|tool_use_id| {
            requested_calls.remove(&tool_use_id).ok_or_else(|| {
                SdkRunFactoryError::RecoveryDecision(format!(
                    "original tool request {tool_use_id} is missing"
                ))
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
struct ReplayToolCall {
    tool_use_id: ToolUseId,
    tool_name: String,
    input: serde_json::Value,
}

struct ReplayFirstModelProvider {
    inner: Arc<dyn ModelProvider>,
    replay_calls: Vec<ReplayToolCall>,
    replay_pending: AtomicBool,
}

impl ReplayFirstModelProvider {
    fn new(inner: Arc<dyn ModelProvider>, replay_calls: Vec<ReplayToolCall>) -> Self {
        Self {
            inner,
            replay_pending: AtomicBool::new(!replay_calls.is_empty()),
            replay_calls,
        }
    }

    fn replay_events(&self) -> Vec<ModelStreamEvent> {
        let mut events = Vec::with_capacity(self.replay_calls.len() + 3);
        events.push(ModelStreamEvent::MessageStart {
            message_id: format!(
                "indeterminate-tool-replay-{}",
                self.replay_calls[0].tool_use_id
            ),
            usage: UsageSnapshot::default(),
        });
        events.extend(self.replay_calls.iter().enumerate().map(|(index, call)| {
            ModelStreamEvent::ContentBlockDelta {
                index: index as u32,
                delta: ContentDelta::ToolUseComplete {
                    id: call.tool_use_id,
                    name: call.tool_name.clone(),
                    input: call.input.clone(),
                },
            }
        }));
        events.push(ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::ToolUse),
            usage_delta: UsageSnapshot::default(),
        });
        events.push(ModelStreamEvent::MessageStop);
        events
    }
}

#[async_trait]
impl ModelProvider for ReplayFirstModelProvider {
    fn provider_id(&self) -> &str {
        self.inner.provider_id()
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        self.inner.supported_models()
    }

    async fn infer(
        &self,
        request: ModelRequest,
        context: InferContext,
    ) -> Result<ModelStream, ModelError> {
        if context.cancel.is_cancelled() {
            return Err(ModelError::Cancelled);
        }
        if context
            .deadline
            .is_some_and(|deadline| std::time::Instant::now() >= deadline)
        {
            return Err(ModelError::DeadlineExceeded(std::time::Duration::ZERO));
        }
        if self.replay_pending.swap(false, Ordering::AcqRel) {
            return Ok(Box::pin(stream::iter(self.replay_events())));
        }
        self.inner.infer(request, context).await
    }

    fn default_protocol(&self) -> harness_contracts::ModelProtocol {
        self.inner.default_protocol()
    }

    fn prompt_cache_style(&self) -> harness_model::PromptCacheStyle {
        self.inner.prompt_cache_style()
    }

    async fn health(&self) -> HealthStatus {
        self.inner.health().await
    }
}

#[derive(Debug, Error)]
enum SdkRunFactoryError {
    #[error("workspace lease is missing from the immutable segment input")]
    WorkspaceLeaseMissing,
    #[error("workspace lease does not exist")]
    WorkspaceLeaseNotFound,
    #[error("workspace lease belongs to another task")]
    WorkspaceLeaseTaskMismatch,
    #[error("workspace lease is not active")]
    WorkspaceLeaseInactive,
    #[error("the current platform has no filesystem-enforcing local sandbox")]
    WorkspaceSandboxUnavailable,
    #[error("managed workspace lease has no worktree path")]
    ManagedWorkspacePathMissing,
    #[error("workspace validation failed: {0}")]
    Workspace(String),
    #[error("runtime configuration failed: {0}")]
    RuntimeConfig(String),
    #[error("execution defaults failed: {0}")]
    ExecutionDefaults(String),
    #[error("attachment could not be loaded: {0}")]
    Attachment(String),
    #[error("attachment body is missing")]
    AttachmentMissing,
    #[error("indeterminate tool recovery decision failed: {0}")]
    RecoveryDecision(String),
    #[error("SDK execution failed: {0}")]
    Sdk(String),
    #[error("durable segment terminal lookup failed: {0}")]
    DurableTerminal(String),
}

impl SdkRunFactoryError {
    fn diagnostic_kind(&self) -> &'static str {
        match self {
            Self::WorkspaceLeaseMissing
            | Self::WorkspaceLeaseNotFound
            | Self::WorkspaceLeaseTaskMismatch
            | Self::WorkspaceLeaseInactive
            | Self::WorkspaceSandboxUnavailable
            | Self::ManagedWorkspacePathMissing
            | Self::Workspace(_) => "workspace",
            Self::RuntimeConfig(_) | Self::ExecutionDefaults(_) => "runtime_config",
            Self::Attachment(_) | Self::AttachmentMissing => "attachment",
            Self::RecoveryDecision(_) => "recovery",
            Self::Sdk(_) => "sdk",
            Self::DurableTerminal(_) => "durable_terminal",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    };

    use async_trait::async_trait;
    use harness_contracts::{
        ClientId, CommandId, DeferPolicy, Event, EventId, ExecutionDefaultsRecord,
        ExecutionOverridesRecord, IndeterminateToolDecision, IndeterminateToolResolution,
        ModelError, ModelProtocol, NoopRedactor, PermissionMode,
        ProviderProfileConversationCapability, ProviderProfileDefinition,
        ProviderProfileModelDescriptor, ProviderProfileModelLifecycle, ProviderSecretEntry,
        ProviderSecretsRecord, ProviderSelectionRecord, QueueItemId, RunId, RunSegmentId,
        RunTerminalReason, SessionId, StopReason, TaskId, TenantId, ToolProfile, ToolProperties,
        ToolUseId, ToolUseRequestedEvent, ToolUseStartedEvent, TrustLevel, UsageSnapshot,
        WorkspaceMode,
    };
    use harness_engine::{RunControl, TurnOutcome};
    use harness_journal::{
        AcceptedCommand, CommandOutcome, EventStore, NewTaskEvent, ReplayCursor, SegmentRunInput,
        TaskEventStoreAdapter, TaskStore,
    };
    use harness_mcp::{
        McpConnectContext, McpConnection, McpError, McpEventSink, McpRegistry, McpServerScope,
        McpServerSpec, McpToolDescriptor, McpToolResult, McpTransport, NoopMcpEventSink,
        TransportChoice,
    };
    use harness_model::TestModelProvider;
    use harness_plugin::{
        PluginCapabilities, PluginLifecycleState, PluginManifest, PluginName, ToolManifestEntry,
    };
    use harness_sandbox::LocalIsolation;
    use harness_subagent::{
        ParentContext, SubagentError, SubagentHandle, SubagentRunner, SubagentSpec,
    };
    use jyowo_harness_sdk::ext::{
        ContentDelta, InferContext, ModelProvider, ModelRequest, ModelStreamEvent,
    };
    use jyowo_harness_sdk::testing::{InMemoryEventStore, NoopSandbox, TestTool};
    use serde_json::json;

    use crate::{
        AgentStarterCapabilities, PermissionBroker, RunCoordinatorEvent, RunCoordinatorFactory,
        RuntimeConfigResolver, SdkRunCoordinatorFactory, SdkSubagentEngineRegistry,
        SdkWorkspaceSubagentRunnerFactory, StartSegmentRequest, SubagentParentBinding,
        SubagentSupervisor, WorkspaceAccess, WorkspaceAcquireOutcome, WorkspaceCoordinator,
        WorkspaceExecutionKind, WorkspaceLeaseRequest, WorkspaceSubagentRunnerFactory,
        WorkspaceToolDispatcher,
    };

    struct UnusedAgentStarter;

    impl harness_contracts::BackgroundAgentStarterCap for UnusedAgentStarter {
        fn start_background_agent(
            &self,
            _request: harness_contracts::BackgroundAgentToolStartRequest,
        ) -> futures::future::BoxFuture<
            'static,
            Result<
                harness_contracts::BackgroundAgentToolStartResponse,
                harness_contracts::ToolError,
            >,
        > {
            Box::pin(async {
                Err(harness_contracts::ToolError::Internal(
                    "unexpected background starter execution".to_owned(),
                ))
            })
        }
    }

    impl harness_contracts::AgentTeamStarterCap for UnusedAgentStarter {
        fn start_agent_team(
            &self,
            _request: harness_contracts::AgentTeamToolStartRequest,
        ) -> futures::future::BoxFuture<
            'static,
            Result<harness_contracts::AgentTeamToolStartResponse, harness_contracts::ToolError>,
        > {
            Box::pin(async {
                Err(harness_contracts::ToolError::Internal(
                    "unexpected team starter execution".to_owned(),
                ))
            })
        }
    }

    fn unused_agent_starters() -> AgentStarterCapabilities {
        AgentStarterCapabilities {
            background: Arc::new(UnusedAgentStarter),
            team: Arc::new(UnusedAgentStarter),
        }
    }

    #[derive(Default)]
    struct ShutdownTrackingMcpConnection {
        shutdown: AtomicBool,
    }

    struct FailingMcpTransport;

    #[async_trait]
    impl McpTransport for FailingMcpTransport {
        fn transport_id(&self) -> &str {
            "failing"
        }

        async fn connect(&self, _spec: McpServerSpec) -> Result<Arc<dyn McpConnection>, McpError> {
            Err(McpError::Connection("fixture failure".to_owned()))
        }
    }

    #[tokio::test]
    async fn optional_mcp_connect_failure_is_registered_without_injection() {
        let registry = McpRegistry::new();
        let spec = McpServerSpec::new(
            harness_contracts::McpServerId("optional-server".to_owned()),
            "optional server",
            TransportChoice::InProcess,
            harness_contracts::McpServerSource::User,
        );

        let outcome = super::activate_daemon_mcp_server(
            &registry,
            spec.clone(),
            McpServerScope::Global,
            Arc::new(FailingMcpTransport),
            Arc::new(NoopMcpEventSink),
            McpConnectContext::default(),
        )
        .await
        .expect("optional failure must not abort activation");

        assert_eq!(outcome, None);
        assert!(matches!(
            registry.connection_state(&spec.server_id).await,
            Some(harness_mcp::McpConnectionState::Failed { .. })
        ));
    }

    #[tokio::test]
    async fn required_mcp_connect_failure_closes_previously_registered_servers() {
        let registry = McpRegistry::new();
        let existing = Arc::new(ShutdownTrackingMcpConnection::default());
        registry
            .add_ready_server(
                McpServerSpec::new(
                    harness_contracts::McpServerId("existing".to_owned()),
                    "existing",
                    TransportChoice::InProcess,
                    harness_contracts::McpServerSource::User,
                ),
                McpServerScope::Global,
                existing.clone(),
            )
            .await
            .expect("register existing server");
        let mut required = McpServerSpec::new(
            harness_contracts::McpServerId("sk-private-server-id".to_owned()),
            "private server",
            TransportChoice::InProcess,
            harness_contracts::McpServerSource::User,
        );
        required.required = true;

        let error = super::activate_daemon_mcp_server(
            &registry,
            required,
            McpServerScope::Global,
            Arc::new(FailingMcpTransport),
            Arc::new(NoopMcpEventSink),
            McpConnectContext::default(),
        )
        .await
        .expect_err("required failure must abort activation");

        assert!(!error.to_string().contains("private-server-id"));
        assert!(existing.shutdown.load(Ordering::SeqCst));
        assert!(registry.server_ids().await.is_empty());
    }

    #[tokio::test]
    async fn daemon_mcp_event_sink_is_bounded() {
        let (sink, mut receiver) = super::DaemonMcpEventSink::channel(1);
        let event = Event::UnexpectedError(harness_contracts::UnexpectedErrorEvent {
            session_id: None,
            run_id: None,
            error: "fixture".to_owned(),
            at: chrono::Utc::now(),
        });

        sink.emit(event.clone());
        sink.emit(event);

        assert_eq!(sink.dropped_events(), 1);
        assert!(receiver.recv().await.is_some());
    }

    #[tokio::test]
    async fn daemon_mcp_event_writer_flushes_task_run_and_segment_context() {
        let fixture = Fixture::new();
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let segment_id = RunSegmentId::new();
        let event_store: Arc<dyn EventStore> = Arc::new(
            TaskEventStoreAdapter::new(
                Arc::clone(&fixture.store),
                fixture.task_id,
                TenantId::SINGLE,
                session_id,
                Arc::new(NoopRedactor),
            )
            .with_run_segment_id(segment_id),
        );
        let (sink, receiver) =
            super::DaemonMcpEventSink::channel_with_context(8, session_id, run_id);
        let writer = super::spawn_daemon_mcp_event_writer(receiver, event_store, session_id);

        sink.emit(Event::UnexpectedError(
            harness_contracts::UnexpectedErrorEvent {
                session_id: None,
                run_id: None,
                error: "MCP server activation failed".to_owned(),
                at: chrono::Utc::now(),
            },
        ));
        sink.close();
        writer
            .await
            .expect("event writer task")
            .expect("flush MCP event");

        let events = fixture
            .store
            .task_events_after(fixture.task_id, 0, 64)
            .expect("read task journal");
        let diagnostic = events
            .iter()
            .find(|event| event.event_type == "engine.unexpected_error")
            .expect("flushed MCP diagnostic");
        let payload = diagnostic.payload.to_string();
        assert!(payload.contains(&session_id.to_string()));
        assert!(payload.contains(&run_id.to_string()));
        assert!(payload.contains(&segment_id.to_string()));
    }

    #[test]
    fn daemon_mcp_cleanup_preserves_the_primary_error() {
        let result = super::complete_after_mcp_shutdown::<(), _>(
            Err("primary failure"),
            Err("shutdown failure"),
        );

        assert_eq!(result, Err("primary failure"));
    }

    #[async_trait]
    impl McpConnection for ShutdownTrackingMcpConnection {
        fn connection_id(&self) -> &'static str {
            "daemon-shutdown-tracking"
        }

        async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
            Ok(Vec::new())
        }

        async fn call_tool(
            &self,
            _name: &str,
            _args: serde_json::Value,
        ) -> Result<McpToolResult, McpError> {
            Ok(McpToolResult::text("ok"))
        }

        async fn shutdown(&self) -> Result<(), McpError> {
            self.shutdown.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn daemon_mcp_runtime_shutdown_closes_and_clears_registry() {
        let registry = McpRegistry::new();
        let connection = Arc::new(ShutdownTrackingMcpConnection::default());
        let server_id = harness_contracts::McpServerId("daemon-fixture".to_owned());
        registry
            .add_ready_server(
                McpServerSpec::new(
                    server_id.clone(),
                    "daemon fixture",
                    harness_mcp::TransportChoice::InProcess,
                    harness_contracts::McpServerSource::Workspace,
                ),
                harness_contracts::McpServerScope::Global,
                connection.clone(),
            )
            .await
            .expect("register daemon MCP fixture");
        let session_id = SessionId::new();
        let (event_sink, receiver) = super::DaemonMcpEventSink::channel(8);
        let event_store: Arc<dyn EventStore> =
            Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let event_writer = super::spawn_daemon_mcp_event_writer(receiver, event_store, session_id);
        let guard = super::DaemonMcpRuntimeGuard {
            config: jyowo_harness_sdk::McpConfig {
                registry: registry.clone(),
                server_ids_to_inject: vec![server_id],
                event_sink: event_sink.clone(),
            },
            event_sink,
            event_writer: Some(event_writer),
            shutdown_complete: false,
        };

        guard
            .shutdown()
            .await
            .expect("shut down daemon MCP runtime");

        assert!(connection.shutdown.load(Ordering::SeqCst));
        assert!(registry.server_ids().await.is_empty());
    }

    #[test]
    fn runtime_mcp_failures_do_not_expose_persisted_ids_or_environment_names() {
        let record = serde_json::from_value::<crate::RuntimeMcpServerConfig>(json!({
            "enabled": true,
            "displayName": "secret display",
            "id": "sk-diagnostic-secret",
            "scope": "session",
            "transport": {
                "kind": "http",
                "url": "https://example.com",
                "headers_from_env": [{
                    "key": "X-Test",
                    "envVar": "HEADER_DIAGNOSTIC_SECRET"
                }]
            }
        }))
        .expect("runtime MCP fixture");

        let error = match super::mcp_server_runtime(&record, Path::new("/tmp")) {
            Ok(_) => panic!("missing environment must fail"),
            Err(error) => error,
        };
        let message = error.to_string();

        assert!(!message.contains("diagnostic-secret"));
        assert!(!message.contains("DIAGNOSTIC_SECRET"));
        assert!(message.len() <= 256);
    }

    #[test]
    fn runtime_mcp_spec_carries_required_policy_and_user_trust() {
        let record = serde_json::from_value::<crate::RuntimeMcpServerConfig>(json!({
            "enabled": true,
            "required": true,
            "displayName": "global server",
            "id": "global-server",
            "scope": "global",
            "transport": {
                "kind": "stdio",
                "command": "node"
            }
        }))
        .expect("runtime MCP fixture");

        let (spec, _) = super::mcp_server_runtime(&record, Path::new("/tmp"))
            .expect("build runtime MCP server");

        assert!(spec.required);
        assert_eq!(spec.source, harness_contracts::McpServerSource::User);
        assert_eq!(spec.trust, harness_contracts::TrustLevel::UserControlled);
    }

    #[test]
    fn disabled_required_mcp_server_is_not_activated() {
        let record = serde_json::from_value::<crate::RuntimeMcpServerConfig>(json!({
            "enabled": false,
            "required": true,
            "displayName": "disabled required server",
            "id": "disabled-required",
            "scope": "global",
            "transport": {
                "kind": "stdio",
                "command": "missing-command"
            }
        }))
        .expect("runtime MCP fixture");

        assert!(!super::should_activate_mcp_server(&record));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn foreground_and_subagent_harnesses_receive_the_same_runtime_snapshot() {
        let fixture = Fixture::new();
        fixture.write_provider_config();
        let manifest = fixture.write_sidecar_plugin("snapshot-plugin", "snapshot-tool");
        let plugin_id = manifest.plugin_id();
        let snapshot = crate::RuntimeConfigResolver::new(fixture._root.path().join("config"))
            .resolve(&fixture.workspace_root, None)
            .expect("runtime snapshot");
        let build_harness = || {
            super::apply_runtime_snapshot(
                jyowo_harness_sdk::Harness::builder()
                    .with_workspace_root(&fixture.workspace_root)
                    .with_model(TestModelProvider::default())
                    .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
                    .with_sandbox(NoopSandbox::new()),
                &snapshot,
                jyowo_harness_sdk::McpConfig::default(),
            )
            .expect("apply runtime snapshot")
        };

        let foreground = build_harness().build().await.expect("foreground harness");
        let subagent = build_harness().build().await.expect("subagent harness");

        for harness in [&foreground, &subagent] {
            assert!(harness.mcp_config().is_some());
            assert!(harness.plugin_registry().is_some());
            assert_eq!(
                *harness.provider_capability_routes().read(),
                snapshot.provider_routes
            );
        }

        let foreground_mcp = foreground.mcp_config().expect("foreground MCP config");
        let subagent_mcp = subagent.mcp_config().expect("subagent MCP config");
        foreground_mcp
            .registry
            .add_failed_server(
                McpServerSpec::new(
                    harness_contracts::McpServerId("foreground-only".to_owned()),
                    "foreground only",
                    TransportChoice::InProcess,
                    harness_contracts::McpServerSource::User,
                ),
                McpServerScope::Global,
                "fixture".to_owned(),
            )
            .await
            .expect("register foreground-only MCP server");
        assert_eq!(foreground_mcp.registry.server_ids().await.len(), 1);
        assert!(subagent_mcp.registry.server_ids().await.is_empty());

        let foreground_registry = foreground.plugin_registry().expect("foreground registry");
        let subagent_registry = subagent.plugin_registry().expect("subagent registry");
        foreground_registry
            .discover()
            .await
            .expect("discover foreground");
        foreground_registry
            .activate(&plugin_id)
            .await
            .expect("activate foreground");
        assert_eq!(
            foreground_registry.state(&plugin_id),
            Some(PluginLifecycleState::Activated)
        );
        assert!(foreground.tool_registry().get("snapshot-tool").is_some());
        assert_eq!(subagent_registry.state(&plugin_id), None);
        assert!(subagent.tool_registry().get("snapshot-tool").is_none());

        subagent_registry
            .discover()
            .await
            .expect("discover subagent");
        subagent_registry
            .activate(&plugin_id)
            .await
            .expect("activate subagent");
        assert_eq!(
            subagent_registry.state(&plugin_id),
            Some(PluginLifecycleState::Activated)
        );
        assert!(subagent.tool_registry().get("snapshot-tool").is_some());
    }

    #[tokio::test]
    async fn production_subagent_factory_executes_the_child_only_in_its_task_scope() {
        use harness_contracts::{AgentToolPolicy, AgentUsePolicy, AgentWorkspaceIsolationMode};

        let fixture = Fixture::new();
        fixture.write_provider_config();
        initialize_git_repository(&fixture.workspace_root);
        let parent_segment_id = RunSegmentId::new();
        let parent_actor_id = fixture
            .store
            .workspace_lease(fixture.lease_id)
            .unwrap()
            .unwrap()
            .actor_id;
        let expected_stream_version = fixture.store.stream_version(fixture.task_id).unwrap();
        fixture
            .store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id: fixture.task_id,
                    idempotency_key: format!("start-{parent_segment_id}"),
                    expected_stream_version,
                    authority: TaskStore::supervisor_authority(),
                    payload: json!({ "type": "test_start" }),
                },
                |_| {
                    Ok(vec![NewTaskEvent::run_started(
                        parent_segment_id,
                        chrono::Utc::now(),
                    )])
                },
            )
            .unwrap();
        let provider: Arc<dyn ModelProvider> =
            Arc::new(TestModelProvider::default().with_events(vec![
                ModelStreamEvent::MessageStart {
                    message_id: "child-response".into(),
                    usage: UsageSnapshot::default(),
                },
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("child complete".into()),
                },
                ModelStreamEvent::MessageDelta {
                    stop_reason: Some(StopReason::EndTurn),
                    usage_delta: UsageSnapshot::default(),
                },
                ModelStreamEvent::MessageStop,
            ]));
        let mut runtime_config = RuntimeConfigResolver::new(fixture._root.path().join("config"))
            .resolve(&fixture.workspace_root, None)
            .expect("runtime config");
        runtime_config.provider.provider = provider;
        runtime_config.provider.config_id = "test".into();
        runtime_config.provider.model_id = "test-model".into();
        runtime_config.provider.protocol = ModelProtocol::Messages;
        let registry = Arc::new(SdkSubagentEngineRegistry::default());
        let _binding = registry.bind(
            parent_segment_id,
            Arc::new(super::SdkSubagentRuntimeTemplate {
                store: Arc::clone(&fixture.store),
                runtime_config,
                permissions: Arc::clone(&fixture.factory.permissions),
                redactor: Arc::new(NoopRedactor),
                workspace_tools: fixture.workspace_tools.clone(),
                agent_tool_policy: AgentToolPolicy {
                    subagents: AgentUsePolicy::Allowed,
                    agent_team: AgentUsePolicy::Off,
                    background_agents: AgentUsePolicy::Off,
                    team_config: None,
                    workspace_isolation: AgentWorkspaceIsolationMode::GitWorktree,
                    max_depth: 4,
                    max_concurrent_subagents: 8,
                    max_team_members: 0,
                },
            }),
        );
        let runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory> = Arc::new(
            SdkWorkspaceSubagentRunnerFactory::new(Arc::clone(&registry)),
        );
        let subagents = Arc::new(SubagentSupervisor::new(
            Arc::clone(&fixture.store),
            Arc::clone(&fixture.coordinator),
            runner_factory,
            Arc::new(NoopRedactor),
            4,
            8,
        ));

        let spawn_result = subagents
            .bind(SubagentParentBinding {
                parent_task_id: fixture.task_id,
                parent_segment_id,
                parent_actor_id,
                depth: 0,
            })
            .spawn(
                SubagentSpec::minimal("reviewer", "inspect child workspace"),
                harness_contracts::TurnInput {
                    message: harness_contracts::Message {
                        id: harness_contracts::MessageId::new(),
                        role: harness_contracts::MessageRole::User,
                        parts: vec![harness_contracts::MessagePart::Text("inspect".into())],
                        created_at: chrono::Utc::now(),
                    },
                    metadata: Default::default(),
                },
                ParentContext::for_test(0),
            )
            .await;

        let projections = fixture.store.task_projections().unwrap();
        if let Err(error) = &spawn_result {
            panic!(
                "child failed before handle creation: {error}; projections={:?}",
                projections
                    .iter()
                    .map(|projection| (
                        projection.task_id,
                        projection.state.clone(),
                        projection
                            .parent
                            .as_ref()
                            .map(|parent| (parent.parent_task_id, parent.parent_segment_id,)),
                    ))
                    .collect::<Vec<_>>()
            );
        }
        let child_task_id = projections
            .into_iter()
            .into_iter()
            .find(|projection| {
                projection.parent.as_ref().is_some_and(|parent| {
                    parent.parent_task_id == fixture.task_id
                        && parent.parent_segment_id == parent_segment_id
                })
            })
            .expect("child task projection should be persisted even when execution fails")
            .task_id;
        let child_events = fixture
            .store
            .task_events_after(child_task_id, 0, 128)
            .unwrap();
        let handle = spawn_result.unwrap_or_else(|error| {
            panic!(
                "child failed: {error}; events={:?}",
                child_events
                    .iter()
                    .map(|event| (&event.event_type, &event.payload))
                    .collect::<Vec<_>>()
            )
        });
        let announcement = handle.wait().await.unwrap();

        assert_eq!(
            announcement.status,
            harness_contracts::SubagentStatus::Completed
        );
        assert!(child_events
            .iter()
            .any(|event| event.event_type == "engine.run_started"));
        assert!(!fixture
            .store
            .task_events_after(fixture.task_id, 0, 128)
            .unwrap()
            .iter()
            .any(|event| event.event_type == "engine.run_started"));
    }

    #[test]
    fn job_object_supports_model_segments_without_authorizing_workspace_commands() {
        assert!(super::validate_daemon_segment_isolation(LocalIsolation::JobObject).is_ok());
        assert!(!crate::workspace::workspace_command_isolation_enforced(
            LocalIsolation::JobObject
        ));
        assert!(super::validate_daemon_segment_isolation(LocalIsolation::None).is_err());
    }

    #[test]
    fn execution_defaults_control_the_immutable_agent_policy() {
        use harness_contracts::{
            AgentTeamSharedMemoryPolicy, AgentTeamTopology, AgentUsePolicy, ExecutionDefaultsRecord,
        };

        for (defaults, subagents, teams, background) in [
            (
                ExecutionDefaultsRecord::default(),
                AgentUsePolicy::Off,
                AgentUsePolicy::Off,
                AgentUsePolicy::Off,
            ),
            (
                ExecutionDefaultsRecord {
                    subagents_enabled: true,
                    ..Default::default()
                },
                AgentUsePolicy::Allowed,
                AgentUsePolicy::Off,
                AgentUsePolicy::Off,
            ),
            (
                ExecutionDefaultsRecord {
                    subagents_enabled: true,
                    agent_teams_enabled: true,
                    ..Default::default()
                },
                AgentUsePolicy::Allowed,
                AgentUsePolicy::Allowed,
                AgentUsePolicy::Off,
            ),
            (
                ExecutionDefaultsRecord {
                    subagents_enabled: true,
                    background_agents_enabled: true,
                    ..Default::default()
                },
                AgentUsePolicy::Allowed,
                AgentUsePolicy::Off,
                AgentUsePolicy::Allowed,
            ),
            (
                ExecutionDefaultsRecord {
                    subagents_enabled: true,
                    agent_teams_enabled: true,
                    background_agents_enabled: true,
                    ..Default::default()
                },
                AgentUsePolicy::Allowed,
                AgentUsePolicy::Allowed,
                AgentUsePolicy::Allowed,
            ),
        ] {
            let policy = super::daemon_agent_tool_policy(&defaults)
                .expect("valid execution defaults should produce a policy");
            assert_eq!(policy.subagents, subagents);
            assert_eq!(policy.agent_team, teams);
            assert_eq!(policy.background_agents, background);
            if teams == AgentUsePolicy::Allowed {
                let team = policy.team_config.expect("enabled teams require config");
                assert_eq!(team.topology, AgentTeamTopology::CoordinatorWorker);
                assert_eq!(team.lead_profile_id, "reviewer");
                assert_eq!(team.member_profile_ids, ["worker"]);
                assert_eq!(
                    team.shared_memory_policy,
                    AgentTeamSharedMemoryPolicy::SummariesOnly
                );
                assert!(team.max_turns_per_goal > 0);
                assert_eq!(policy.max_team_members, 2);
            } else {
                assert!(policy.team_config.is_none());
                assert_eq!(policy.max_team_members, 0);
            }
        }

        for defaults in [
            ExecutionDefaultsRecord {
                agent_teams_enabled: true,
                ..Default::default()
            },
            ExecutionDefaultsRecord {
                background_agents_enabled: true,
                ..Default::default()
            },
            ExecutionDefaultsRecord {
                agent_teams_enabled: true,
                background_agents_enabled: true,
                ..Default::default()
            },
        ] {
            assert!(super::daemon_agent_tool_policy(&defaults).is_err());
        }
    }

    #[test]
    fn runtime_permission_precedence_uses_explicit_then_project_then_global() {
        assert_eq!(
            super::effective_runtime_permission_mode(PermissionMode::Auto, PermissionMode::Plan),
            PermissionMode::Auto
        );
        assert_eq!(
            super::effective_runtime_permission_mode(PermissionMode::Default, PermissionMode::Plan),
            PermissionMode::Plan
        );
        assert_eq!(
            super::effective_runtime_permission_mode(
                PermissionMode::Default,
                PermissionMode::DontAsk,
            ),
            PermissionMode::DontAsk
        );
    }

    #[tokio::test]
    async fn foreground_runtime_applies_global_project_and_explicit_permission_precedence() {
        for (project, requested, expected) in [
            (None, PermissionMode::Default, PermissionMode::Plan),
            (
                Some(PermissionMode::AcceptEdits),
                PermissionMode::Default,
                PermissionMode::AcceptEdits,
            ),
            (
                Some(PermissionMode::AcceptEdits),
                PermissionMode::DontAsk,
                PermissionMode::DontAsk,
            ),
        ] {
            let fixture = Fixture::new();
            fixture.write_provider_config();
            fixture.write_permission_config(PermissionMode::Plan, project);
            let mut request = fixture.request(Some("selected"));
            request.input.permission_mode = requested;
            let running = fixture.factory.spawn_idempotent(
                request,
                fixture.workspace_tools.clone(),
                Arc::new(UnusedSubagentRunner),
                unused_agent_starters(),
            );
            running.control().request(RunControl::ForceStop);
            let mut events = running.into_events();
            tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
                .await
                .expect("permission precedence run should terminate");

            let task_events = fixture
                .store
                .task_events_after(fixture.task_id, 0, 256)
                .expect("read task events");
            let run_started = task_events
                .iter()
                .find(|event| event.event_type == "engine.run_started")
                .expect("effective permission must be journaled on run start");
            let encoded = serde_json::to_string(&run_started.payload).expect("encode run start");
            let expected = serde_json::to_string(&expected).expect("encode permission mode");
            assert!(
                encoded.contains(&format!("\"permission_mode\":{expected}")),
                "requested={requested:?}, project={project:?}, payload={encoded}"
            );
        }
    }

    #[tokio::test]
    async fn missing_provider_configuration_finishes_the_segment_as_failed() {
        let fixture = Fixture::new();
        let request = fixture.request(Some("missing"));
        let running = fixture.factory.spawn_idempotent(
            request,
            fixture.workspace_tools.clone(),
            Arc::new(UnusedSubagentRunner),
            unused_agent_starters(),
        );

        assert!(matches!(
            running.into_events().recv().await,
            Some(RunCoordinatorEvent::Completed {
                terminal_reason: RunTerminalReason::Failed,
                ..
            })
        ));
    }

    #[test]
    fn controlled_run_outcomes_map_to_durable_terminal_reasons() {
        assert_eq!(
            super::run_terminal_reason(None, false, false),
            RunTerminalReason::Completed
        );
        assert_eq!(
            super::run_terminal_reason(Some(TurnOutcome::YieldedAtSafePoint), false, false),
            RunTerminalReason::Cancelled
        );
        assert_eq!(
            super::run_terminal_reason(Some(TurnOutcome::YieldedAtSafePoint), true, false),
            RunTerminalReason::Superseded
        );
        assert_eq!(
            super::run_terminal_reason(
                Some(TurnOutcome::ForceStopped {
                    non_revertible_tool_use_ids: Vec::new(),
                }),
                false,
                false,
            ),
            RunTerminalReason::ForcedInterruption
        );
        assert_eq!(
            super::run_terminal_reason(
                Some(TurnOutcome::ForceStopTimedOut {
                    indeterminate_tool_use_ids: Vec::new(),
                }),
                false,
                false,
            ),
            RunTerminalReason::Failed
        );
        assert_eq!(
            super::run_terminal_reason(None, false, true),
            RunTerminalReason::Failed
        );
    }

    #[test]
    fn superseded_terminal_survives_the_projection_advancing_to_the_next_segment() {
        let fixture = Fixture::new();
        let old_segment = RunSegmentId::new();
        let next_segment = RunSegmentId::new();
        let queue_item_id = QueueItemId::new();
        let now = chrono::Utc::now();
        let expected_stream_version = fixture.store.stream_version(fixture.task_id).unwrap();
        fixture
            .store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id: fixture.task_id,
                    idempotency_key: "advance-after-safe-promotion".into(),
                    expected_stream_version,
                    authority: TaskStore::supervisor_authority(),
                    payload: json!({ "type": "test_safe_promotion" }),
                },
                |_| {
                    Ok(vec![
                        NewTaskEvent::run_started(old_segment, now),
                        NewTaskEvent::message_queued_with_runtime(
                            queue_item_id,
                            "next",
                            Vec::new(),
                            Vec::new(),
                            None,
                            PermissionMode::BypassPermissions,
                            now,
                        ),
                        NewTaskEvent::message_promoted(queue_item_id, 1),
                        NewTaskEvent::run_yield_requested(old_segment, false, now),
                        NewTaskEvent::run_safe_point_reached(
                            old_segment,
                            false,
                            true,
                            Vec::new(),
                            now,
                        ),
                        NewTaskEvent::run_completed(
                            old_segment,
                            now,
                            RunTerminalReason::Superseded,
                            true,
                        ),
                        NewTaskEvent::run_started(next_segment, now),
                        NewTaskEvent::message_consumed(queue_item_id, 1, next_segment),
                    ])
                },
            )
            .unwrap();

        assert_eq!(
            fixture
                .store
                .task_projection(fixture.task_id)
                .unwrap()
                .unwrap()
                .current_run
                .unwrap()
                .segment_id,
            next_segment
        );
        assert_eq!(
            super::segment_terminal_reason(
                &fixture.store,
                fixture.task_id,
                old_segment,
                Some(TurnOutcome::YieldedAtSafePoint),
                false,
            )
            .unwrap(),
            RunTerminalReason::Superseded
        );
    }

    #[tokio::test]
    async fn duplicate_spawn_reuses_one_control_and_one_terminal_result() {
        let fixture = Fixture::new();
        let request = fixture.request(Some("missing"));
        let first = fixture.factory.spawn_idempotent(
            request.clone(),
            fixture.workspace_tools.clone(),
            Arc::new(UnusedSubagentRunner),
            unused_agent_starters(),
        );
        let second = fixture.factory.spawn_idempotent(
            request,
            fixture.workspace_tools.clone(),
            Arc::new(UnusedSubagentRunner),
            unused_agent_starters(),
        );
        let first_control = first.control();
        let second_control = second.control();
        first_control.finish(harness_engine::TurnOutcome::ForceStopped {
            non_revertible_tool_use_ids: Vec::new(),
        });
        assert_eq!(
            second_control.outcome().await,
            harness_engine::TurnOutcome::ForceStopped {
                non_revertible_tool_use_ids: Vec::new(),
            }
        );
    }

    #[tokio::test]
    async fn terminal_segments_are_removed_from_the_in_process_registry() {
        let fixture = Fixture::new();
        let request = fixture.request(Some("missing"));
        let key = (request.task_id, request.segment_id);
        let running = fixture.factory.spawn_idempotent(
            request,
            fixture.workspace_tools.clone(),
            Arc::new(UnusedSubagentRunner),
            unused_agent_starters(),
        );

        assert!(running.into_events().recv().await.is_some());
        tokio::task::yield_now().await;

        assert!(!fixture.factory.segments.lock().unwrap().contains_key(&key));
    }

    #[tokio::test]
    async fn durable_completion_failure_is_not_published_as_a_terminal_event() {
        let fixture = Fixture::new();
        let request = fixture.request(Some("missing"));
        let request_digest = super::segment_request_digest(&request);
        rusqlite::Connection::open(fixture.store.database_path())
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER inject_segment_completion_failure
                 BEFORE UPDATE OF status ON segment_execution
                 WHEN NEW.status = 'completed'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected segment completion failure');
                 END;",
            )
            .unwrap();
        let mut events = fixture
            .factory
            .spawn_idempotent(
                request.clone(),
                fixture.workspace_tools.clone(),
                Arc::new(UnusedSubagentRunner),
                unused_agent_starters(),
            )
            .into_events();

        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
                .await
                .expect("failed completion closes the in-process event stream")
                .is_none()
        );
        assert_eq!(
            fixture
                .store
                .claim_segment_execution(request.task_id, request.segment_id, &request_digest)
                .unwrap(),
            harness_journal::SegmentExecutionClaim::InProgress
        );
    }

    #[tokio::test]
    async fn treat_as_failed_indeterminate_decision_is_consumed_once_before_the_model_request() {
        let fixture = Fixture::new();
        let mut request = fixture.request(Some("missing"));
        let tool_use_id = ToolUseId::new();
        let expected_stream_version = fixture.store.stream_version(fixture.task_id).unwrap();
        fixture
            .store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id: fixture.task_id,
                    idempotency_key: format!("start-{}", request.segment_id),
                    expected_stream_version,
                    authority: TaskStore::supervisor_authority(),
                    payload: json!({ "type": "test_start" }),
                },
                |_| {
                    Ok(vec![NewTaskEvent::run_started(
                        request.segment_id,
                        chrono::Utc::now(),
                    )])
                },
            )
            .unwrap();
        let event_store = TaskEventStoreAdapter::new(
            Arc::clone(&fixture.store),
            request.task_id,
            harness_contracts::TenantId::SINGLE,
            request.input.session_id,
            Arc::new(NoopRedactor),
        );
        event_store
            .append(
                harness_contracts::TenantId::SINGLE,
                request.input.session_id,
                &[
                    Event::ToolUseRequested(ToolUseRequestedEvent {
                        run_id: request.input.run_id,
                        tool_use_id,
                        tool_name: "Bash".into(),
                        input: json!({ "command": "echo side-effect" }),
                        properties: ToolProperties {
                            is_concurrency_safe: false,
                            is_read_only: false,
                            is_destructive: true,
                            long_running: None,
                            defer_policy: DeferPolicy::AlwaysLoad,
                        },
                        causation_id: EventId::new(),
                        at: chrono::Utc::now(),
                    }),
                    Event::ToolUseStarted(ToolUseStartedEvent {
                        run_id: request.input.run_id,
                        tool_use_id,
                        at: chrono::Utc::now(),
                    }),
                ],
            )
            .await
            .unwrap();
        request.indeterminate_tools = vec![IndeterminateToolDecision {
            tool_use_id: tool_use_id.to_string(),
            resolution: IndeterminateToolResolution::TreatAsFailed,
        }];

        let first = fixture.factory.spawn_idempotent(
            request.clone(),
            fixture.workspace_tools.clone(),
            Arc::new(UnusedSubagentRunner),
            unused_agent_starters(),
        );
        assert!(first.into_events().recv().await.is_some());
        let replay = fixture.factory.spawn_idempotent(
            request,
            fixture.workspace_tools.clone(),
            Arc::new(UnusedSubagentRunner),
            unused_agent_starters(),
        );
        assert!(replay.into_events().recv().await.is_some());

        let task_events = fixture
            .store
            .task_events_after(fixture.task_id, 0, 256)
            .unwrap();
        let failures = task_events
            .iter()
            .filter(|event| {
                event.event_type == "engine.tool_use_failed"
                    && event.payload.to_string().contains(&tool_use_id.to_string())
            })
            .count();
        let event_types = task_events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>();
        assert_eq!(failures, 1, "event_types={event_types:?}");
    }

    #[tokio::test]
    async fn execute_again_recovers_the_original_tool_request_for_explicit_replay() {
        let fixture = Fixture::new();
        let mut request = fixture.request(Some("missing"));
        let tool_use_id = ToolUseId::new();
        let expected_stream_version = fixture.store.stream_version(fixture.task_id).unwrap();
        fixture
            .store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id: fixture.task_id,
                    idempotency_key: format!("start-{}", request.segment_id),
                    expected_stream_version,
                    authority: TaskStore::supervisor_authority(),
                    payload: json!({ "type": "test_start" }),
                },
                |_| {
                    Ok(vec![NewTaskEvent::run_started(
                        request.segment_id,
                        chrono::Utc::now(),
                    )])
                },
            )
            .unwrap();
        let event_store = TaskEventStoreAdapter::new(
            Arc::clone(&fixture.store),
            request.task_id,
            harness_contracts::TenantId::SINGLE,
            request.input.session_id,
            Arc::new(NoopRedactor),
        );
        event_store
            .append(
                harness_contracts::TenantId::SINGLE,
                request.input.session_id,
                &[Event::ToolUseRequested(ToolUseRequestedEvent {
                    run_id: request.input.run_id,
                    tool_use_id,
                    tool_name: "Bash".into(),
                    input: json!({ "command": "echo side-effect" }),
                    properties: ToolProperties {
                        is_concurrency_safe: false,
                        is_read_only: false,
                        is_destructive: true,
                        long_running: None,
                        defer_policy: DeferPolicy::AlwaysLoad,
                    },
                    causation_id: EventId::new(),
                    at: chrono::Utc::now(),
                })],
            )
            .await
            .unwrap();
        request.indeterminate_tools = vec![IndeterminateToolDecision {
            tool_use_id: tool_use_id.to_string(),
            resolution: IndeterminateToolResolution::ExecuteAgain,
        }];

        let replay_calls = super::apply_indeterminate_tool_decisions(&event_store, &request)
            .await
            .unwrap();

        assert_eq!(replay_calls.len(), 1);
        assert_eq!(replay_calls[0].tool_use_id, tool_use_id);
        assert_eq!(replay_calls[0].tool_name, "Bash");
        assert_eq!(
            replay_calls[0].input,
            json!({ "command": "echo side-effect" })
        );
    }

    #[tokio::test]
    async fn replay_provider_synthesizes_once_before_delegating_to_the_real_provider() {
        use futures::StreamExt;

        let tool_use_id = ToolUseId::new();
        let inner = Arc::new(TestModelProvider::default());
        let provider = super::ReplayFirstModelProvider::new(
            inner.clone(),
            vec![super::ReplayToolCall {
                tool_use_id,
                tool_name: "Bash".into(),
                input: json!({ "command": "echo side-effect" }),
            }],
        );

        let first = provider
            .infer(model_request(), InferContext::for_test())
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;

        assert!(inner.requests().await.is_empty());
        assert!(first.iter().any(|event| {
            matches!(
                event,
                ModelStreamEvent::ContentBlockDelta {
                    delta: ContentDelta::ToolUseComplete { id, name, input },
                    ..
                } if *id == tool_use_id
                    && name == "Bash"
                    && *input == json!({ "command": "echo side-effect" })
            )
        }));
        assert!(first.iter().any(|event| {
            matches!(
                event,
                ModelStreamEvent::MessageDelta {
                    stop_reason: Some(StopReason::ToolUse),
                    usage_delta,
                } if *usage_delta == UsageSnapshot::default()
            )
        }));

        let _second = provider
            .infer(model_request(), InferContext::for_test())
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;
        assert_eq!(inner.requests().await.len(), 1);
    }

    #[tokio::test]
    async fn replay_provider_honors_cancellation_before_synthesizing_a_tool_call() {
        let inner = Arc::new(TestModelProvider::default());
        let provider = super::ReplayFirstModelProvider::new(
            inner.clone(),
            vec![super::ReplayToolCall {
                tool_use_id: ToolUseId::new(),
                tool_name: "replay_tool".into(),
                input: json!({}),
            }],
        );
        let context = InferContext::for_test();
        context.cancel.cancel();

        assert!(matches!(
            provider.infer(model_request(), context).await,
            Err(ModelError::Cancelled)
        ));
        assert!(inner.requests().await.is_empty());
    }

    #[tokio::test]
    async fn replay_provider_honors_an_expired_deadline_before_synthesizing_a_tool_call() {
        let inner = Arc::new(TestModelProvider::default());
        let provider = super::ReplayFirstModelProvider::new(
            inner.clone(),
            vec![super::ReplayToolCall {
                tool_use_id: ToolUseId::new(),
                tool_name: "replay_tool".into(),
                input: json!({}),
            }],
        );
        let mut context = InferContext::for_test();
        context.deadline = Some(std::time::Instant::now());

        assert!(matches!(
            provider.infer(model_request(), context).await,
            Err(ModelError::DeadlineExceeded(_))
        ));
        assert!(inner.requests().await.is_empty());
    }

    #[tokio::test]
    async fn explicit_replay_executes_the_original_tool_once_through_the_engine() {
        use futures::StreamExt;

        let workspace = tempfile::tempdir().unwrap();
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let tool_use_id = ToolUseId::new();
        let inner = Arc::new(TestModelProvider::default());
        let model: Arc<dyn ModelProvider> = Arc::new(super::ReplayFirstModelProvider::new(
            inner.clone(),
            vec![super::ReplayToolCall {
                tool_use_id,
                tool_name: "replay_tool".into(),
                input: json!({ "value": "original" }),
            }],
        ));
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let event_store: Arc<dyn EventStore> = store.clone();
        let tool_registry = jyowo_harness_sdk::ext::ToolRegistry::builder()
            .with_tool(Box::new(TestTool::new("replay_tool")))
            .build()
            .unwrap();
        let harness = jyowo_harness_sdk::Harness::builder()
            .with_workspace_root(workspace.path())
            .with_model_arc(model)
            .with_store_arc(event_store)
            .with_sandbox(NoopSandbox::new())
            .with_tool_registry(tool_registry)
            .build()
            .await
            .unwrap();
        let options = jyowo_harness_sdk::SessionOptions::new(workspace.path())
            .with_session_id(session_id)
            .with_model_id("test-model")
            .with_permission_mode(PermissionMode::BypassPermissions);
        harness
            .open_or_create_conversation_session(options.clone())
            .await
            .unwrap();
        let run_options = jyowo_harness_sdk::ConversationRunOptions::from_session_options(&options)
            .with_permission_mode(PermissionMode::BypassPermissions);

        harness
            .submit_conversation_turn_with_run_control(
                jyowo_harness_sdk::ConversationTurnRequest {
                    options,
                    run_options,
                    input: harness_contracts::ConversationTurnInput::ask("resume after recovery"),
                    permission_actor_source: None,
                },
                run_id,
                harness_engine::RunControlHandle::new(),
            )
            .await
            .unwrap();

        let events = store
            .read(
                harness_contracts::TenantId::SINGLE,
                session_id,
                ReplayCursor::FromStart,
            )
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;
        let completed = events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    Event::ToolUseCompleted(completed)
                        if completed.tool_use_id == tool_use_id
                )
            })
            .count();
        assert_eq!(completed, 1);
        assert_eq!(inner.requests().await.len(), 1);
    }

    fn model_request() -> ModelRequest {
        ModelRequest {
            model_id: "test-model".into(),
            messages: Vec::new(),
            tools: None,
            system: None,
            temperature: None,
            max_tokens: None,
            stream: true,
            cache_breakpoints: Vec::new(),
            protocol: ModelProtocol::Messages,
            extra: serde_json::Value::Null,
            options: Default::default(),
            provider_context: harness_model::ProviderRequestContext::default(),
        }
    }

    fn initialize_git_repository(path: &Path) {
        for arguments in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            assert!(std::process::Command::new("git")
                .args(arguments)
                .current_dir(path)
                .status()
                .unwrap()
                .success());
        }
        std::fs::write(path.join("README.md"), "fixture\n").unwrap();
        assert!(std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(path)
            .status()
            .unwrap()
            .success());
        assert!(std::process::Command::new("git")
            .args(["commit", "-q", "-m", "fixture"])
            .current_dir(path)
            .status()
            .unwrap()
            .success());
    }

    #[tokio::test]
    async fn inactive_workspace_lease_finishes_as_failed_before_provider_resolution() {
        let fixture = Fixture::new();
        fixture
            .coordinator
            .release(fixture.lease_id)
            .expect("release fixture lease");
        let running = fixture.factory.spawn_idempotent(
            fixture.request(Some("missing")),
            fixture.workspace_tools.clone(),
            Arc::new(UnusedSubagentRunner),
            unused_agent_starters(),
        );

        assert!(matches!(
            running.into_events().recv().await,
            Some(RunCoordinatorEvent::Completed {
                terminal_reason: RunTerminalReason::Failed,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn controlled_sdk_turn_uses_stable_session_and_run_ids_in_the_task_log() {
        let fixture = Fixture::new();
        fixture.write_provider_config();
        let request = fixture.request(Some("selected"));
        let session_id = request.input.session_id;
        let run_id = request.input.run_id;
        let running = fixture.factory.spawn_idempotent(
            request,
            fixture.workspace_tools.clone(),
            Arc::new(UnusedSubagentRunner),
            unused_agent_starters(),
        );
        running.control().request(RunControl::ForceStop);
        let mut events = running.into_events();
        let terminal = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
            .await
            .expect("controlled SDK turn should terminate");

        let task_events = fixture
            .store
            .task_events_after(fixture.task_id, 0, 256)
            .unwrap();
        let event_types = task_events
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>();
        assert!(
            task_events
                .iter()
                .any(|event| event.event_type == "engine.session_created"),
            "terminal={terminal:?}, event_types={event_types:?}"
        );
        let run_started = task_events
            .iter()
            .find(|event| event.event_type == "engine.run_started")
            .unwrap_or_else(|| {
                panic!(
                    "controlled run should be written through TaskEventStoreAdapter; terminal={terminal:?}, event_types={event_types:?}"
                )
            });
        let encoded = serde_json::to_string(&run_started.payload).unwrap();
        assert!(encoded.contains(&session_id.to_string()));
        assert!(encoded.contains(&run_id.to_string()));
    }

    #[tokio::test]
    async fn real_file_and_command_tools_revalidate_the_workspace_lease_at_execution() {
        use harness_contracts::{
            AgentId, CapabilityRegistry, CorrelationId, PermissionActorSource,
        };
        use jyowo_harness_sdk::ext::{
            AuthorizationTicketClaims, AuthorizedToolInput, InterruptToken, TicketLedger,
            ToolContext, ToolJournalAuthority,
        };

        let fixture = Fixture::new();
        let input_path = fixture.workspace_root.join("input.txt");
        std::fs::write(&input_path, "before\n").unwrap();
        let registry = super::workspace_tool_registry(
            fixture.workspace_tools.clone(),
            fixture.lease_id,
            fixture.workspace_root.clone(),
            LocalIsolation::for_current_platform(),
        )
        .unwrap();
        assert_eq!(
            registry.snapshot().journal_authority("Bash"),
            ToolJournalAuthority::Sandbox
        );
        let sandbox = Arc::new(harness_sandbox::LocalSandbox::new(&fixture.workspace_root));
        let cases = [
            ("FileRead", json!({ "path": input_path })),
            (
                "FileWrite",
                json!({ "path": fixture.workspace_root.join("write.txt"), "content": "written" }),
            ),
            (
                "FileEdit",
                json!({ "path": input_path, "old": "before", "new": "after" }),
            ),
            ("Bash", json!({ "command": "pwd" })),
        ];
        let mut executions = Vec::new();
        for (name, input) in cases {
            let tool = Arc::clone(registry.snapshot().get(name).unwrap());
            let ctx = ToolContext {
                tool_use_id: harness_contracts::ToolUseId::new(),
                run_id: RunId::new(),
                session_id: SessionId::new(),
                tenant_id: harness_contracts::TenantId::SINGLE,
                model: None,
                model_config_id: None,
                memory_thread_settings: None,
                correlation_id: CorrelationId::new(),
                agent_id: AgentId::from_u128(1),
                subagent_depth: 0,
                workspace_root: fixture.workspace_root.clone(),
                project_workspace_root: None,
                sandbox: Some(sandbox.clone()),
                cap_registry: Arc::new(CapabilityRegistry::default()),
                redactor: Arc::new(NoopRedactor),
                interrupt: InterruptToken::default(),
                parent_run: None,
                actor_source: PermissionActorSource::ParentRun,
            };
            tool.validate(&input, &ctx).await.unwrap();
            let plan = tool.plan(&input, &ctx).await.unwrap();
            let ledger = TicketLedger::default();
            let claims = AuthorizationTicketClaims {
                tenant_id: ctx.tenant_id,
                session_id: ctx.session_id,
                run_id: ctx.run_id,
                tool_use_id: plan.tool_use_id,
                tool_name: plan.tool_name.clone(),
                action_plan_hash: plan.plan_hash.clone(),
            };
            let ticket = ledger.mint(claims.clone(), chrono::Utc::now()).unwrap();
            let ticket = ledger
                .consume(ticket.id, &claims, chrono::Utc::now())
                .unwrap();
            executions.push((
                name,
                tool,
                AuthorizedToolInput::new(input, plan, ticket).unwrap(),
                ctx,
            ));
        }

        fixture
            .coordinator
            .release(fixture.lease_id)
            .expect("release fixture lease");
        for (name, tool, authorized, ctx) in executions {
            assert!(
                tool.execute_authorized(authorized, ctx).await.is_err(),
                "{name} bypassed workspace lease revalidation"
            );
        }
    }

    #[tokio::test]
    async fn command_tool_holds_the_workspace_dispatch_for_its_full_stream() {
        use futures::StreamExt;
        use harness_contracts::{
            AgentId, CapabilityRegistry, CorrelationId, PermissionActorSource,
        };
        use jyowo_harness_sdk::ext::{
            AuthorizationTicketClaims, AuthorizedToolInput, InterruptToken, TicketLedger,
            ToolContext,
        };

        let fixture = Fixture::new();
        let registry = super::workspace_tool_registry(
            fixture.workspace_tools.clone(),
            fixture.lease_id,
            fixture.workspace_root.clone(),
            LocalIsolation::for_current_platform(),
        )
        .unwrap();
        let tool = Arc::clone(registry.snapshot().get("Bash").unwrap());
        let input = json!({ "command": "sleep 0.2" });
        let ctx = ToolContext {
            tool_use_id: harness_contracts::ToolUseId::new(),
            run_id: RunId::new(),
            session_id: SessionId::new(),
            tenant_id: harness_contracts::TenantId::SINGLE,
            model: None,
            model_config_id: None,
            memory_thread_settings: None,
            correlation_id: CorrelationId::new(),
            agent_id: AgentId::from_u128(1),
            subagent_depth: 0,
            workspace_root: fixture.workspace_root.clone(),
            project_workspace_root: None,
            sandbox: Some(Arc::new(
                harness_sandbox::LocalSandbox::new(&fixture.workspace_root)
                    .with_isolation(LocalIsolation::for_current_platform()),
            )),
            cap_registry: Arc::new(CapabilityRegistry::default()),
            redactor: Arc::new(NoopRedactor),
            interrupt: InterruptToken::default(),
            parent_run: None,
            actor_source: PermissionActorSource::ParentRun,
        };
        tool.validate(&input, &ctx).await.unwrap();
        let plan = tool.plan(&input, &ctx).await.unwrap();
        let ledger = TicketLedger::default();
        let claims = AuthorizationTicketClaims {
            tenant_id: ctx.tenant_id,
            session_id: ctx.session_id,
            run_id: ctx.run_id,
            tool_use_id: plan.tool_use_id,
            tool_name: plan.tool_name.clone(),
            action_plan_hash: plan.plan_hash.clone(),
        };
        let ticket = ledger.mint(claims.clone(), chrono::Utc::now()).unwrap();
        let ticket = ledger
            .consume(ticket.id, &claims, chrono::Utc::now())
            .unwrap();
        let authorized = AuthorizedToolInput::new(input, plan, ticket).unwrap();

        let mut events = tool.execute_authorized(authorized, ctx).await.unwrap();
        assert!(fixture.coordinator.release(fixture.lease_id).is_err());
        while events.next().await.is_some() {}
        fixture.coordinator.release(fixture.lease_id).unwrap();
    }

    #[tokio::test]
    async fn cancelling_before_tool_stream_ready_interrupts_the_worker_and_releases_dispatch() {
        let fixture = Fixture::new();
        let started = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let tool = blocking_workspace_command_tool(
            &fixture,
            BlockingCommandMode::BeforeReady,
            Arc::clone(&started),
            Arc::clone(&finished),
        );
        let ctx = workspace_tool_test_context(&fixture.workspace_root);
        let authorized = authorize_test_tool(&tool, json!({ "command": "true" }), &ctx).await;
        let execution = tokio::spawn(async move { tool.execute_authorized(authorized, ctx).await });
        wait_for_flag(&started).await;

        execution.abort();
        let _ = execution.await;

        wait_for_flag(&finished).await;
        fixture.coordinator.release(fixture.lease_id).unwrap();
    }

    #[tokio::test]
    async fn dropping_tool_stream_interrupts_the_worker_before_releasing_dispatch() {
        let fixture = Fixture::new();
        let started = Arc::new(AtomicBool::new(false));
        let finished = Arc::new(AtomicBool::new(false));
        let tool = blocking_workspace_command_tool(
            &fixture,
            BlockingCommandMode::Stream,
            Arc::clone(&started),
            Arc::clone(&finished),
        );
        let ctx = workspace_tool_test_context(&fixture.workspace_root);
        let authorized = authorize_test_tool(&tool, json!({ "command": "true" }), &ctx).await;
        let events = tool.execute_authorized(authorized, ctx).await.unwrap();
        wait_for_flag(&started).await;
        assert!(fixture.coordinator.release(fixture.lease_id).is_err());

        drop(events);

        wait_for_flag(&finished).await;
        fixture.coordinator.release(fixture.lease_id).unwrap();
    }

    #[tokio::test]
    async fn dropping_after_a_terminal_event_does_not_interrupt_later_tools() {
        use futures::StreamExt;

        let fixture = Fixture::new();
        let registry = super::workspace_tool_registry(
            fixture.workspace_tools.clone(),
            fixture.lease_id,
            fixture.workspace_root.clone(),
            LocalIsolation::for_current_platform(),
        )
        .unwrap();
        let tool = Arc::clone(registry.snapshot().get("FileWrite").unwrap());
        let ctx = workspace_tool_test_context(&fixture.workspace_root);
        let interrupt = ctx.interrupt.clone();
        let authorized = authorize_test_tool(
            &tool,
            json!({
                "path": fixture.workspace_root.join("terminal.txt"),
                "content": "done",
            }),
            &ctx,
        )
        .await;

        let mut events = tool.execute_authorized(authorized, ctx).await.unwrap();
        assert!(matches!(
            events.next().await,
            Some(jyowo_harness_sdk::ext::ToolEvent::Final(_))
        ));
        drop(events);

        assert!(!interrupt.is_interrupted());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn workspace_file_adapter_rejects_a_symlink_swap_after_authorization() {
        use harness_contracts::{
            AgentId, CapabilityRegistry, CorrelationId, PermissionActorSource,
        };
        use jyowo_harness_sdk::ext::{
            AuthorizationTicketClaims, AuthorizedToolInput, InterruptToken, TicketLedger,
            ToolContext, ToolRegistry,
        };

        use crate::WorkspaceToolAction;

        let fixture = Fixture::new();
        let input_path = fixture.workspace_root.join("input.txt");
        let outside_path = fixture._root.path().join("outside.txt");
        std::fs::write(&input_path, "inside\n").unwrap();
        std::fs::write(&outside_path, "outside secret\n").unwrap();
        let registry = ToolRegistry::builder().build().unwrap();
        let tool = Arc::clone(registry.snapshot().get("FileRead").unwrap());
        let input = json!({ "path": input_path });
        let ctx = ToolContext {
            tool_use_id: harness_contracts::ToolUseId::new(),
            run_id: RunId::new(),
            session_id: SessionId::new(),
            tenant_id: harness_contracts::TenantId::SINGLE,
            model: None,
            model_config_id: None,
            memory_thread_settings: None,
            correlation_id: CorrelationId::new(),
            agent_id: AgentId::from_u128(1),
            subagent_depth: 0,
            workspace_root: fixture.workspace_root.clone(),
            project_workspace_root: None,
            sandbox: None,
            cap_registry: Arc::new(CapabilityRegistry::default()),
            redactor: Arc::new(NoopRedactor),
            interrupt: InterruptToken::default(),
            parent_run: None,
            actor_source: PermissionActorSource::ParentRun,
        };
        let plan = tool.plan(&input, &ctx).await.unwrap();
        let ledger = TicketLedger::default();
        let claims = AuthorizationTicketClaims {
            tenant_id: ctx.tenant_id,
            session_id: ctx.session_id,
            run_id: ctx.run_id,
            tool_use_id: plan.tool_use_id,
            tool_name: plan.tool_name.clone(),
            action_plan_hash: plan.plan_hash.clone(),
        };
        let ticket = ledger.mint(claims.clone(), chrono::Utc::now()).unwrap();
        let ticket = ledger
            .consume(ticket.id, &claims, chrono::Utc::now())
            .unwrap();
        let authorized = AuthorizedToolInput::new(input, plan, ticket).unwrap();
        let action = WorkspaceToolAction::ReadPath(input_path.clone());

        let result = fixture
            .workspace_tools
            .dispatch(fixture.lease_id, action.clone(), move |authorization| {
                let action = action.clone();
                async move {
                    std::fs::remove_file(&input_path).unwrap();
                    std::os::unix::fs::symlink(&outside_path, &input_path).unwrap();
                    super::execute_workspace_file_tool(
                        "FileRead",
                        &authorized,
                        vec![(action, authorization)],
                    )
                }
            })
            .await
            .unwrap();

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn secure_workspace_file_adapters_preserve_builtin_semantics_and_fail_closed_other_tools()
    {
        use futures::StreamExt;

        let fixture = Fixture::new();
        let input_path = fixture.workspace_root.join("input.txt");
        std::fs::write(&input_path, "alpha\nbeta\n").unwrap();
        let registry = super::workspace_tool_registry(
            fixture.workspace_tools.clone(),
            fixture.lease_id,
            fixture.workspace_root.clone(),
            LocalIsolation::for_current_platform(),
        )
        .unwrap();

        let cases = [
            (
                "FileRead",
                json!({ "path": input_path, "start_line": 2, "end_line": 2 }),
            ),
            (
                "FileWrite",
                json!({ "path": fixture.workspace_root.join("written.txt"), "content": "written" }),
            ),
            (
                "FileEdit",
                json!({ "path": input_path, "old": "beta", "new": "gamma" }),
            ),
        ];
        for (name, input) in cases {
            let tool = Arc::clone(registry.snapshot().get(name).unwrap());
            let ctx = workspace_tool_test_context(&fixture.workspace_root);
            let authorized = authorize_test_tool(&tool, input, &ctx).await;
            let mut events = tool.execute_authorized(authorized, ctx).await.unwrap();
            while events.next().await.is_some() {}
        }
        assert_eq!(
            std::fs::read_to_string(fixture.workspace_root.join("written.txt")).unwrap(),
            "written"
        );
        assert_eq!(
            std::fs::read_to_string(&input_path).unwrap(),
            "alpha\ngamma\n"
        );

        let list_dir = Arc::clone(registry.snapshot().get("ListDir").unwrap());
        let ctx = workspace_tool_test_context(&fixture.workspace_root);
        let authorized =
            authorize_test_tool(&list_dir, json!({ "path": fixture.workspace_root }), &ctx).await;
        assert!(matches!(
            list_dir.execute_authorized(authorized, ctx).await,
            Err(harness_contracts::ToolError::PermissionDenied(message))
                if message.contains("no secure workspace filesystem adapter")
        ));
    }

    #[tokio::test]
    async fn workspace_file_adapter_rejects_content_not_bound_to_the_authorized_write_plan() {
        let fixture = Fixture::new();
        let output_path = fixture.workspace_root.join("output.txt");
        let registry = super::workspace_tool_registry(
            fixture.workspace_tools.clone(),
            fixture.lease_id,
            fixture.workspace_root.clone(),
            LocalIsolation::for_current_platform(),
        )
        .unwrap();
        let tool = Arc::clone(registry.snapshot().get("FileWrite").unwrap());
        let ctx = workspace_tool_test_context(&fixture.workspace_root);
        let planned_input = json!({ "path": output_path, "content": "authorized" });
        let plan = tool.plan(&planned_input, &ctx).await.unwrap();
        let ticket = consumed_test_ticket(&plan, &ctx);
        let authorized = jyowo_harness_sdk::ext::AuthorizedToolInput::new(
            json!({ "path": output_path, "content": "mutated" }),
            plan,
            ticket,
        )
        .unwrap();

        assert!(matches!(
            tool.execute_authorized(authorized, ctx).await,
            Err(harness_contracts::ToolError::PermissionDenied(message))
                if message.contains("content hash")
        ));
        assert!(!output_path.exists());
    }

    #[tokio::test]
    async fn workspace_file_adapter_rejects_an_edit_not_bound_to_the_authorized_plan() {
        let fixture = Fixture::new();
        let output_path = fixture.workspace_root.join("output.txt");
        std::fs::write(&output_path, "alpha beta\n").unwrap();
        let registry = super::workspace_tool_registry(
            fixture.workspace_tools.clone(),
            fixture.lease_id,
            fixture.workspace_root.clone(),
            LocalIsolation::for_current_platform(),
        )
        .unwrap();
        let tool = Arc::clone(registry.snapshot().get("FileEdit").unwrap());
        let ctx = workspace_tool_test_context(&fixture.workspace_root);
        let planned_input = json!({
            "path": output_path,
            "old": "beta",
            "new": "gamma",
            "replace_all": false,
        });
        let plan = tool.plan(&planned_input, &ctx).await.unwrap();
        let ticket = consumed_test_ticket(&plan, &ctx);
        let authorized = jyowo_harness_sdk::ext::AuthorizedToolInput::new(
            json!({
                "path": output_path,
                "old": "alpha",
                "new": "gamma",
                "replace_all": false,
            }),
            plan,
            ticket,
        )
        .unwrap();

        assert!(matches!(
            tool.execute_authorized(authorized, ctx).await,
            Err(harness_contracts::ToolError::PermissionDenied(message))
                if message.contains("edit hash")
        ));
        assert_eq!(
            std::fs::read_to_string(output_path).unwrap(),
            "alpha beta\n"
        );
    }

    fn workspace_tool_test_context(root: &Path) -> jyowo_harness_sdk::ext::ToolContext {
        jyowo_harness_sdk::ext::ToolContext {
            tool_use_id: harness_contracts::ToolUseId::new(),
            run_id: RunId::new(),
            session_id: SessionId::new(),
            tenant_id: harness_contracts::TenantId::SINGLE,
            model: None,
            model_config_id: None,
            memory_thread_settings: None,
            correlation_id: harness_contracts::CorrelationId::new(),
            agent_id: harness_contracts::AgentId::from_u128(1),
            subagent_depth: 0,
            workspace_root: root.to_path_buf(),
            project_workspace_root: None,
            sandbox: None,
            cap_registry: Arc::new(harness_contracts::CapabilityRegistry::default()),
            redactor: Arc::new(NoopRedactor),
            interrupt: jyowo_harness_sdk::ext::InterruptToken::default(),
            parent_run: None,
            actor_source: harness_contracts::PermissionActorSource::ParentRun,
        }
    }

    async fn authorize_test_tool(
        tool: &Arc<dyn jyowo_harness_sdk::ext::Tool>,
        input: serde_json::Value,
        ctx: &jyowo_harness_sdk::ext::ToolContext,
    ) -> jyowo_harness_sdk::ext::AuthorizedToolInput {
        tool.validate(&input, ctx).await.unwrap();
        let plan = tool.plan(&input, ctx).await.unwrap();
        let ticket = consumed_test_ticket(&plan, ctx);
        jyowo_harness_sdk::ext::AuthorizedToolInput::new(input, plan, ticket).unwrap()
    }

    fn consumed_test_ticket(
        plan: &harness_contracts::ToolActionPlan,
        ctx: &jyowo_harness_sdk::ext::ToolContext,
    ) -> jyowo_harness_sdk::ext::AuthorizedTicketSummary {
        let ledger = jyowo_harness_sdk::ext::TicketLedger::default();
        let claims = jyowo_harness_sdk::ext::AuthorizationTicketClaims {
            tenant_id: ctx.tenant_id,
            session_id: ctx.session_id,
            run_id: ctx.run_id,
            tool_use_id: plan.tool_use_id,
            tool_name: plan.tool_name.clone(),
            action_plan_hash: plan.plan_hash.clone(),
        };
        let ticket = ledger.mint(claims.clone(), chrono::Utc::now()).unwrap();
        let ticket = ledger
            .consume(ticket.id, &claims, chrono::Utc::now())
            .unwrap();
        ticket
    }

    #[derive(Clone, Copy)]
    enum BlockingCommandMode {
        BeforeReady,
        Stream,
    }

    struct BlockingCommandTool {
        delegate: Arc<dyn jyowo_harness_sdk::ext::Tool>,
        mode: BlockingCommandMode,
        started: Arc<AtomicBool>,
        finished: Arc<AtomicBool>,
    }

    #[async_trait]
    impl jyowo_harness_sdk::ext::Tool for BlockingCommandTool {
        fn descriptor(&self) -> &harness_contracts::ToolDescriptor {
            self.delegate.descriptor()
        }

        async fn validate(
            &self,
            input: &serde_json::Value,
            ctx: &jyowo_harness_sdk::ext::ToolContext,
        ) -> Result<(), jyowo_harness_sdk::ext::ValidationError> {
            self.delegate.validate(input, ctx).await
        }

        async fn plan(
            &self,
            input: &serde_json::Value,
            ctx: &jyowo_harness_sdk::ext::ToolContext,
        ) -> Result<harness_contracts::ToolActionPlan, harness_contracts::ToolError> {
            self.delegate.plan(input, ctx).await
        }

        async fn execute_authorized(
            &self,
            _authorized: jyowo_harness_sdk::ext::AuthorizedToolInput,
            ctx: jyowo_harness_sdk::ext::ToolContext,
        ) -> Result<jyowo_harness_sdk::ext::ToolStream, harness_contracts::ToolError> {
            self.started.store(true, Ordering::SeqCst);
            match self.mode {
                BlockingCommandMode::BeforeReady => {
                    while !ctx.interrupt.is_interrupted() {
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    }
                    self.finished.store(true, Ordering::SeqCst);
                    Err(harness_contracts::ToolError::Message("interrupted".into()))
                }
                BlockingCommandMode::Stream => {
                    let interrupt = ctx.interrupt;
                    let finished = Arc::clone(&self.finished);
                    Ok(Box::pin(futures::stream::once(async move {
                        while !interrupt.is_interrupted() {
                            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                        }
                        finished.store(true, Ordering::SeqCst);
                        jyowo_harness_sdk::ext::ToolEvent::Error(
                            harness_contracts::ToolError::Message("interrupted".into()),
                        )
                    })))
                }
            }
        }
    }

    fn blocking_workspace_command_tool(
        fixture: &Fixture,
        mode: BlockingCommandMode,
        started: Arc<AtomicBool>,
        finished: Arc<AtomicBool>,
    ) -> Arc<dyn jyowo_harness_sdk::ext::Tool> {
        let registry = jyowo_harness_sdk::ext::ToolRegistry::builder()
            .build()
            .unwrap();
        let delegate = Arc::clone(registry.snapshot().get("Bash").unwrap());
        Arc::new(super::WorkspaceDispatchedTool {
            inner: Arc::new(BlockingCommandTool {
                delegate,
                mode,
                started,
                finished,
            }),
            workspace_tools: fixture.workspace_tools.clone(),
            lease_id: fixture.lease_id,
            workspace_root: fixture.workspace_root.clone(),
            isolation: LocalIsolation::for_current_platform(),
        })
    }

    async fn wait_for_flag(flag: &AtomicBool) {
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !flag.load(Ordering::SeqCst) {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("worker lifecycle flag should be observed");
    }

    struct Fixture {
        _root: tempfile::TempDir,
        task_id: TaskId,
        lease_id: harness_contracts::WorkspaceLeaseId,
        store: Arc<TaskStore>,
        coordinator: Arc<WorkspaceCoordinator>,
        workspace_tools: WorkspaceToolDispatcher,
        workspace_root: std::path::PathBuf,
        factory: SdkRunCoordinatorFactory,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("workspace");
            std::fs::create_dir(&workspace).unwrap();
            let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
            let task_id = create_task(&store);
            let actor_id = store
                .task_projection(task_id)
                .unwrap()
                .unwrap()
                .actor_id
                .unwrap();
            let coordinator = Arc::new(
                WorkspaceCoordinator::new(
                    Arc::clone(&store),
                    root.path().join("managed-worktrees"),
                )
                .unwrap(),
            );
            let lease = match coordinator
                .acquire(WorkspaceLeaseRequest {
                    task_id,
                    actor_id,
                    root: workspace,
                    mode: Some(WorkspaceMode::Current),
                    access: WorkspaceAccess::Write,
                    execution_kind: WorkspaceExecutionKind::Foreground,
                    expires_at: None,
                })
                .unwrap()
            {
                WorkspaceAcquireOutcome::Acquired(lease) => lease,
                WorkspaceAcquireOutcome::Waiting(_) => panic!("fixture lease must be active"),
            };
            let redactor = Arc::new(NoopRedactor);
            let permissions = Arc::new(PermissionBroker::new(Arc::clone(&store), redactor.clone()));
            let factory = SdkRunCoordinatorFactory::new(
                Arc::clone(&store),
                RuntimeConfigResolver::new(root.path().join("config")),
                root.path().join("blobs"),
                permissions,
                redactor,
            );
            Self {
                _root: root,
                task_id,
                lease_id: lease.lease_id,
                store,
                coordinator: Arc::clone(&coordinator),
                workspace_tools: WorkspaceToolDispatcher::new(coordinator),
                workspace_root: lease.canonical_root.into(),
                factory,
            }
        }

        fn write_provider_config(&self) {
            let config = self._root.path().join("config");
            std::fs::create_dir(&config).unwrap();
            write_json(
                &config.join("provider-profiles.json"),
                &[profile("selected", "local-llama", "llama3.1")],
            );
            write_json(
                &config.join("provider-secrets.json"),
                &ProviderSecretsRecord {
                    entries: vec![ProviderSecretEntry {
                        config_id: "selected".into(),
                        api_key: "test-key".into(),
                        official_quota_api_key: None,
                    }],
                },
            );
            write_json(
                &config.join("provider-selection.json"),
                &ProviderSelectionRecord {
                    default_config_id: Some("selected".into()),
                },
            );
        }

        fn write_permission_config(&self, global: PermissionMode, project: Option<PermissionMode>) {
            let config = self._root.path().join("config");
            std::fs::create_dir_all(&config).expect("global config directory");
            write_json(
                &config.join("execution-defaults.json"),
                &ExecutionDefaultsRecord {
                    permission_mode: global,
                    tool_profile: ToolProfile::Full,
                    ..ExecutionDefaultsRecord::default()
                },
            );
            if let Some(permission_mode) = project {
                let project_config = self.workspace_root.join(".jyowo/config");
                std::fs::create_dir_all(&project_config).expect("project config directory");
                write_json(
                    &project_config.join("execution-overrides.json"),
                    &ExecutionOverridesRecord {
                        permission_mode: Some(permission_mode),
                        ..ExecutionOverridesRecord::default()
                    },
                );
            }
        }

        fn write_sidecar_plugin(&self, name: &str, tool_name: &str) -> PluginManifest {
            let package = self
                .workspace_root
                .join(".jyowo/plugins/packages")
                .join(name);
            std::fs::create_dir_all(&package).expect("plugin package");
            let manifest = PluginManifest {
                name: PluginName::new(name).expect("plugin name"),
                version: semver::Version::parse("0.1.0").expect("plugin version"),
                trust_level: TrustLevel::UserControlled,
                description: Some("snapshot plugin".to_owned()),
                authors: Vec::new(),
                repository: None,
                signature: None,
                capabilities: PluginCapabilities {
                    tools: vec![ToolManifestEntry {
                        name: tool_name.to_owned(),
                        destructive: false,
                        input_schema: serde_json::json!({ "type": "object" }),
                    }],
                    ..PluginCapabilities::default()
                },
                dependencies: Vec::new(),
                min_harness_version: semver::VersionReq::parse(">=0.0.0")
                    .expect("version requirement"),
            };
            write_json(&package.join("plugin.json"), &manifest);
            let binary = package.join(format!("jyowo-plugin-{name}"));
            std::fs::write(
                &binary,
                r#"#!/bin/sh
if [ "$1" = "--harness-runtime" ]; then
request=$(cat)
case "$request" in
  *\"method\":\"activate\"*)
    printf '{"jsonrpc":"2.0","id":1,"result":{"registered_tools":[],"registered_hooks":[],"registered_skills":[],"registered_mcp":[],"occupied_slots":[]}}'
    exit 0
    ;;
  *\"method\":\"deactivate\"*)
    printf '{"jsonrpc":"2.0","id":1,"result":null}'
    exit 0
    ;;
esac
fi
exit 2
"#,
            )
            .expect("plugin sidecar");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                let mut permissions = std::fs::metadata(&binary)
                    .expect("plugin sidecar metadata")
                    .permissions();
                permissions.set_mode(0o755);
                std::fs::set_permissions(&binary, permissions).expect("plugin sidecar executable");
            }
            write_json(
                &self.workspace_root.join(".jyowo/plugins/index.json"),
                &serde_json::json!({
                    "allowProjectPlugins": true,
                    "records": [{
                        "pluginId": manifest.plugin_id().0,
                        "name": name,
                        "version": "0.1.0",
                        "enabled": true,
                        "packageDir": name,
                        "sourcePath": "fixture",
                        "contentHash": "fixture",
                        "importedAt": "2026-01-01T00:00:00Z",
                        "updatedAt": "2026-01-01T00:00:00Z",
                        "config": {}
                    }]
                }),
            );
            std::fs::create_dir_all(self.workspace_root.join(".jyowo/config"))
                .expect("project config directory");
            write_json(
                &self.workspace_root.join(".jyowo/config/plugins.json"),
                &harness_contracts::PluginSelectionRecord {
                    allow_project_plugins: true,
                    enabled: vec![manifest.plugin_id().0.clone()],
                },
            );
            manifest
        }

        fn request(&self, model_config_id: Option<&str>) -> StartSegmentRequest {
            StartSegmentRequest {
                task_id: self.task_id,
                segment_id: RunSegmentId::new(),
                input: SegmentRunInput {
                    queue_item_id: None,
                    content: "hello".into(),
                    attachments: Vec::new(),
                    context_references: Vec::new(),
                    model_config_id: model_config_id.map(ToOwned::to_owned),
                    permission_mode: PermissionMode::BypassPermissions,
                    workspace: None,
                    session_id: SessionId::new(),
                    run_id: RunId::new(),
                    workspace_lease_id: Some(self.lease_id),
                },
                indeterminate_tools: Vec::new(),
            }
        }
    }

    fn profile(config_id: &str, provider_id: &str, model_id: &str) -> ProviderProfileDefinition {
        ProviderProfileDefinition {
            id: config_id.into(),
            display_name: config_id.into(),
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            protocol: ModelProtocol::ChatCompletions,
            model_options: Default::default(),
            base_url: Some("http://127.0.0.1:9/v1".into()),
            provider_defaults: None,
            model_descriptor: ProviderProfileModelDescriptor {
                protocol: ModelProtocol::ChatCompletions,
                context_window: 32_000,
                display_name: model_id.into(),
                lifecycle: ProviderProfileModelLifecycle::Stable,
                max_output_tokens: 4_096,
                model_id: model_id.into(),
                provider_id: provider_id.into(),
                conversation_capability: ProviderProfileConversationCapability {
                    input_modalities: vec!["text".into()],
                    output_modalities: vec!["text".into()],
                    context_window: 32_000,
                    max_output_tokens: 4_096,
                    streaming: true,
                    tool_calling: true,
                    reasoning: false,
                    prompt_cache: false,
                    structured_output: false,
                },
                runtime_semantics: None,
            },
        }
    }

    fn write_json(path: &Path, value: &(impl serde::Serialize + ?Sized)) {
        std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }

    fn create_task(store: &TaskStore) -> TaskId {
        let task_id = TaskId::new();
        let outcome = store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: format!("create-{task_id}"),
                    expected_stream_version: 0,
                    authority: TaskStore::user_authority(ClientId::new()),
                    payload: json!({ "type": "create_task" }),
                },
                |_| Ok(vec![NewTaskEvent::task_created("factory test")]),
            )
            .unwrap();
        assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
        task_id
    }

    struct UnusedSubagentRunner;

    #[async_trait]
    impl SubagentRunner for UnusedSubagentRunner {
        async fn spawn(
            &self,
            _spec: SubagentSpec,
            _input: harness_contracts::TurnInput,
            _parent_ctx: ParentContext,
        ) -> Result<SubagentHandle, SubagentError> {
            Err(SubagentError::Engine("unused".into()))
        }
    }
}

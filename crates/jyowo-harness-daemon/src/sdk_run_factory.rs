use std::{
    collections::{hash_map::Entry, HashMap},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
};

use async_trait::async_trait;
use chrono::Utc;
use futures::{future::BoxFuture, stream, FutureExt, Stream, StreamExt};
use harness_contracts::{
    ActionResource, AgentToolPolicy, AgentUsePolicy, AgentWorkspaceIsolationMode,
    ConversationAttachmentReference, ConversationContextReference, ConversationTurnInput, Event,
    ExecutionDefaultsRecord, IndeterminateToolResolution, ModelError, PromotionMode,
    QueueItemState, Redactor, RunSegmentId, RunTerminalReason, StopReason, TaskId, TenantId,
    ToolActionPlan, ToolDescriptor, ToolError, ToolErrorPayload, ToolResult, ToolUseFailedEvent,
    ToolUseId, UsageSnapshot, WorkspaceAccess as ToolWorkspaceAccess, WorkspaceLeaseId,
    WorkspaceLeaseState, WorkspaceMode,
};
use harness_engine::{EngineBoundSubagentFactory, RunControlHandle, TurnOutcome};
use harness_journal::{
    AppendMetadata, EventStore, ReplayCursor, SegmentExecutionClaim, SegmentExecutionTerminal,
    TaskBlobStore, TaskEventStoreAdapter, TaskStore,
};
use harness_sandbox::{LocalIsolation, LocalSandbox};
use harness_subagent::{
    ChildRunOutcome, ChildRunRequest, DefaultSubagentRunner, DelegationPolicy,
    SubagentEngineFactory, SubagentError, SubagentRunner,
};
use jyowo_harness_sdk::{
    ext::{
        AuthorizedToolInput, ContentDelta, HealthStatus, InferContext, ModelDescriptor,
        ModelProvider, ModelRequest, ModelStream, ModelStreamEvent, SchemaResolverContext, Tool,
        ToolContext, ToolRegistry, ToolStream, ValidationError,
    },
    ConversationRunOptions, ConversationTurnRequest, Harness, SessionOptions,
};
use serde_json::json;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, watch};

use crate::{
    HarnessPermissionBroker, PermissionBroker, PermissionRuntimeAuthority, ProviderConfigResolver,
    RunCoordinatorEvent, RunCoordinatorFactory, RunningSegment, StartSegmentRequest,
    WorkspaceSubagentRunContext, WorkspaceSubagentRunnerFactory, WorkspaceToolAction,
    WorkspaceToolDispatcher,
};

#[derive(Clone)]
struct SharedSegment {
    control: RunControlHandle,
    terminal: watch::Receiver<Option<RunCoordinatorEvent>>,
}

/// Production daemon adapter that executes task segments through the public SDK facade.
pub struct SdkRunCoordinatorFactory {
    store: Arc<TaskStore>,
    provider_configs: ProviderConfigResolver,
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
    provider: Arc<dyn ModelProvider>,
    config_id: String,
    model_id: String,
    protocol: harness_contracts::ModelProtocol,
    model_options: harness_contracts::ModelRequestOptions,
    permissions: Arc<PermissionBroker>,
    memory_database_path: PathBuf,
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
        let engine_factory = Arc::new(EngineBoundSubagentFactory::default());
        let harness = Harness::builder()
            .with_workspace_root(&workspace_root)
            .with_model_arc(Arc::clone(&self.runtime.provider))
            .with_store_arc(Arc::clone(&self.context.event_store))
            .with_sandbox(LocalSandbox::new(&workspace_root).with_isolation(isolation))
            .with_tool_registry(tool_registry)
            .with_model_id(&self.runtime.model_id)
            .with_permission_broker(permission_broker)
            .with_memory_database_path(&self.runtime.memory_database_path)
            .with_subagent_runner(Arc::clone(&self.context.subagent_runner))
            .with_subagent_engine_factory(Arc::clone(&engine_factory))
            .build()
            .await
            .map_err(|error| SubagentError::Engine(error.to_string()))?;
        let options = SessionOptions::new(&workspace_root)
            .with_tenant_id(self.context.tenant_id)
            .with_session_id(self.context.session_id)
            .with_model_id(&self.runtime.model_id)
            .with_protocol(self.runtime.protocol)
            .with_model_options(self.runtime.model_options.clone())
            .with_permission_mode(request.spec.permission_mode);
        let mut run_options = ConversationRunOptions::from_session_options(&options)
            .with_model_config_id(&self.runtime.config_id)
            .with_model_id(&self.runtime.model_id)
            .with_protocol(self.runtime.protocol)
            .with_permission_mode(request.spec.permission_mode)
            .with_model_options(self.runtime.model_options.clone());
        run_options.agent_tool_policy = Some(self.runtime.agent_tool_policy.clone());
        harness
            .prepare_external_subagent_engine(options, run_options)
            .await
            .map_err(|error| SubagentError::Engine(error.to_string()))?;
        engine_factory.run_child_engine(request).await
    }
}

impl SdkRunCoordinatorFactory {
    #[must_use]
    pub fn new(
        store: Arc<TaskStore>,
        provider_configs: ProviderConfigResolver,
        blob_root: impl Into<PathBuf>,
        permissions: Arc<PermissionBroker>,
        redactor: Arc<dyn Redactor>,
    ) -> Self {
        Self::new_with_subagent_engines(
            store,
            provider_configs,
            blob_root,
            permissions,
            redactor,
            Arc::new(SdkSubagentEngineRegistry::default()),
        )
    }

    #[must_use]
    pub fn new_with_subagent_engines(
        store: Arc<TaskStore>,
        provider_configs: ProviderConfigResolver,
        blob_root: impl Into<PathBuf>,
        permissions: Arc<PermissionBroker>,
        redactor: Arc<dyn Redactor>,
        subagent_engines: Arc<SdkSubagentEngineRegistry>,
    ) -> Self {
        Self {
            store,
            provider_configs,
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
        provider_configs: ProviderConfigResolver,
        blob_root: PathBuf,
        permissions: Arc<PermissionBroker>,
        redactor: Arc<dyn Redactor>,
        request: StartSegmentRequest,
        workspace_tools: WorkspaceToolDispatcher,
        subagent_runner: Arc<dyn SubagentRunner>,
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
        let event_store: Arc<dyn EventStore> = Arc::new(TaskEventStoreAdapter::new(
            Arc::clone(&store),
            request.task_id,
            TenantId::SINGLE,
            request.input.session_id,
            Arc::clone(&redactor),
        ));
        let replay_calls =
            apply_indeterminate_tool_decisions(event_store.as_ref(), &request).await?;
        let provider = provider_configs
            .resolve(request.input.model_config_id.as_deref())
            .map_err(|error| SdkRunFactoryError::Provider(error.to_string()))?;
        let execution_defaults = provider_configs
            .resolve_execution_defaults()
            .map_err(|error| SdkRunFactoryError::ExecutionDefaults(error.to_string()))?;
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
        let agent_tool_policy = daemon_agent_tool_policy(&execution_defaults);
        let subagents_enabled = agent_tool_policy.subagents == AgentUsePolicy::Allowed;
        let memory_database_path = blob_root.join("runtime").join("memory.sqlite3");
        std::fs::create_dir_all(
            memory_database_path
                .parent()
                .expect("daemon memory database path has a parent"),
        )
        .map_err(|error| SdkRunFactoryError::Sdk(error.to_string()))?;
        let _runtime_binding = subagents_enabled.then(|| {
            subagent_engines.bind(
                request.segment_id,
                Arc::new(SdkSubagentRuntimeTemplate {
                    store: Arc::clone(&store),
                    provider: Arc::clone(&provider.provider),
                    config_id: provider.config_id.clone(),
                    model_id: provider.model_id.clone(),
                    protocol: provider.protocol,
                    model_options: provider.model_options.clone(),
                    permissions: Arc::clone(&permissions),
                    memory_database_path: memory_database_path.clone(),
                    workspace_tools: workspace_tools.clone(),
                    agent_tool_policy: agent_tool_policy.clone(),
                }),
            )
        });
        let harness_builder = Harness::builder()
            .with_workspace_root(&workspace_root)
            .with_model_arc(model)
            .with_store_arc(event_store)
            .with_sandbox(LocalSandbox::new(&workspace_root).with_isolation(isolation))
            .with_tool_registry(tool_registry)
            .with_model_id(&provider.model_id)
            .with_permission_broker(permission_broker)
            .with_memory_database_path(memory_database_path);
        let harness_builder = if subagents_enabled {
            harness_builder.with_subagent_runner(subagent_runner)
        } else {
            harness_builder
        };
        let harness = harness_builder
            .build()
            .await
            .map_err(|error| SdkRunFactoryError::Sdk(error.to_string()))?;

        let session_options = SessionOptions::new(&workspace_root)
            .with_tenant_id(TenantId::SINGLE)
            .with_session_id(request.input.session_id)
            .with_model_id(&provider.model_id)
            .with_protocol(provider.protocol)
            .with_model_options(provider.model_options.clone())
            .with_permission_mode(request.input.permission_mode);
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
            .with_model_config_id(provider.config_id)
            .with_model_id(provider.model_id)
            .with_protocol(provider.protocol)
            .with_permission_mode(request.input.permission_mode)
            .with_model_options(provider.model_options);
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
            .map_err(|error| SdkRunFactoryError::Sdk(error.to_string()))?;
        Ok(())
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

fn daemon_agent_tool_policy(defaults: &ExecutionDefaultsRecord) -> AgentToolPolicy {
    AgentToolPolicy {
        subagents: if defaults.subagents_enabled {
            AgentUsePolicy::Allowed
        } else {
            AgentUsePolicy::Off
        },
        agent_team: AgentUsePolicy::Off,
        background_agents: AgentUsePolicy::Off,
        team_config: None,
        workspace_isolation: AgentWorkspaceIsolationMode::GitWorktree,
        max_depth: 4,
        max_concurrent_subagents: 8,
        max_team_members: 0,
    }
}

impl RunCoordinatorFactory for SdkRunCoordinatorFactory {
    fn spawn_idempotent(
        &self,
        request: StartSegmentRequest,
        workspace_tools: WorkspaceToolDispatcher,
        subagent_runner: Arc<dyn SubagentRunner>,
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
            let provider_configs = self.provider_configs.clone();
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
                    provider_configs,
                    blob_root,
                    permissions,
                    redactor,
                    request,
                    workspace_tools,
                    subagent_runner,
                    subagent_engines,
                    control,
                )
                .await;
                let execution_failed = if let Err(error) = result {
                    tracing::error!(%task_id, %segment_id, error = %error, "SDK segment failed");
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
    #[error("provider configuration failed: {0}")]
    Provider(String),
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
        ClientId, CommandId, DeferPolicy, Event, EventId, IndeterminateToolDecision,
        IndeterminateToolResolution, ModelError, ModelProtocol, NoopRedactor, PermissionMode,
        ProviderProfileConversationCapability, ProviderProfileDefinition,
        ProviderProfileModelDescriptor, ProviderProfileModelLifecycle, ProviderSecretEntry,
        ProviderSecretsRecord, ProviderSelectionRecord, QueueItemId, RunId, RunSegmentId,
        RunTerminalReason, SessionId, StopReason, TaskId, ToolProperties, ToolUseId,
        ToolUseRequestedEvent, ToolUseStartedEvent, UsageSnapshot, WorkspaceMode,
    };
    use harness_engine::{RunControl, TurnOutcome};
    use harness_journal::{
        AcceptedCommand, CommandOutcome, EventStore, NewTaskEvent, ReplayCursor, SegmentRunInput,
        TaskEventStoreAdapter, TaskStore,
    };
    use harness_model::TestModelProvider;
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
        PermissionBroker, ProviderConfigResolver, RunCoordinatorEvent, RunCoordinatorFactory,
        SdkRunCoordinatorFactory, SdkSubagentEngineRegistry, SdkWorkspaceSubagentRunnerFactory,
        StartSegmentRequest, SubagentParentBinding, SubagentSupervisor, WorkspaceAccess,
        WorkspaceAcquireOutcome, WorkspaceCoordinator, WorkspaceExecutionKind,
        WorkspaceLeaseRequest, WorkspaceSubagentRunnerFactory, WorkspaceToolDispatcher,
    };

    #[tokio::test]
    async fn production_subagent_factory_executes_the_child_only_in_its_task_scope() {
        use harness_contracts::{AgentToolPolicy, AgentUsePolicy, AgentWorkspaceIsolationMode};

        let fixture = Fixture::new();
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
        let registry = Arc::new(SdkSubagentEngineRegistry::default());
        let _binding = registry.bind(
            parent_segment_id,
            Arc::new(super::SdkSubagentRuntimeTemplate {
                store: Arc::clone(&fixture.store),
                provider,
                config_id: "test".into(),
                model_id: "test-model".into(),
                protocol: ModelProtocol::Messages,
                model_options: Default::default(),
                permissions: Arc::clone(&fixture.factory.permissions),
                memory_database_path: fixture._root.path().join("memory.sqlite3"),
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
    fn execution_defaults_control_the_immutable_subagent_policy() {
        let disabled =
            super::daemon_agent_tool_policy(&harness_contracts::ExecutionDefaultsRecord::default());
        let enabled =
            super::daemon_agent_tool_policy(&harness_contracts::ExecutionDefaultsRecord {
                subagents_enabled: true,
                ..Default::default()
            });

        assert_eq!(disabled.subagents, harness_contracts::AgentUsePolicy::Off);
        assert_eq!(
            enabled.subagents,
            harness_contracts::AgentUsePolicy::Allowed
        );
        assert_eq!(disabled.agent_team, harness_contracts::AgentUsePolicy::Off);
        assert_eq!(
            disabled.background_agents,
            harness_contracts::AgentUsePolicy::Off
        );
    }

    #[tokio::test]
    async fn missing_provider_configuration_finishes_the_segment_as_failed() {
        let fixture = Fixture::new();
        let request = fixture.request(Some("missing"));
        let running = fixture.factory.spawn_idempotent(
            request,
            fixture.workspace_tools.clone(),
            Arc::new(UnusedSubagentRunner),
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
        );
        let second = fixture.factory.spawn_idempotent(
            request,
            fixture.workspace_tools.clone(),
            Arc::new(UnusedSubagentRunner),
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
        );
        assert!(first.into_events().recv().await.is_some());
        let replay = fixture.factory.spawn_idempotent(
            request,
            fixture.workspace_tools.clone(),
            Arc::new(UnusedSubagentRunner),
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
                ProviderConfigResolver::new(root.path().join("config")),
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

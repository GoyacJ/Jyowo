use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(feature = "programmatic-tool-calling")]
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Mutex,
};
#[cfg(feature = "subagent-tool")]
use std::time::Duration;
#[cfg(feature = "subagent-tool")]
use std::{ffi::OsStr, fs};

#[cfg(feature = "subagent-tool")]
use bytes::Bytes;
#[cfg(feature = "subagent-tool")]
use chrono::Utc;
#[cfg(feature = "programmatic-tool-calling")]
use chrono::Utc;
use harness_context::ContextEngine;
#[cfg(feature = "subagent-tool")]
use harness_contracts::{
    BlobMeta, BlobRetention, BudgetKind, ContentHash, EndReason, JournalOffset, MessageContent,
    PermissionActorSource, SubagentContextReport, TranscriptRef, UsageSnapshot,
};
use harness_contracts::{
    BlobStore, CapabilityRegistry, Event, MessageId, ModelRef, RunId, ToolCapability,
};
#[cfg(feature = "programmatic-tool-calling")]
use harness_contracts::{
    CodeLanguage, CodeRunRequest, CodeRunResult, CodeRunStats, EmbeddedRefusedReason,
    EmbeddedToolDispatchRequest, EmbeddedToolDispatchResponse, ExecuteCodeStepInvokedEvent,
    FallbackPolicy, InteractivityLevel, PermissionMode, Redactor, SessionId, TenantId, ToolError,
    ToolResult, ToolUseId,
};
#[cfg(feature = "subagent-tool")]
use harness_contracts::{NetworkAccess, SandboxPolicy, SandboxScope};
#[cfg(feature = "programmatic-tool-calling")]
use harness_execution::{AuthorizationContext, AuthorizationService, ExecutionError, TicketLedger};
use harness_hook::HookDispatcher;
use harness_journal::EventStore;
#[cfg(feature = "subagent-tool")]
use harness_mcp::{McpRegistry, McpServerPattern, McpServerRef, RequiredEvaluation};
use harness_model::{
    InferMiddleware, ModelProtocol, ModelProvider, ModelRuntimeSnapshot, PricingSnapshotResolver,
};
#[cfg(feature = "programmatic-tool-calling")]
use harness_observability::DefaultRedactor;
use harness_observability::{Observer, Tracer};
use harness_permission::PermissionBroker;
#[cfg(feature = "programmatic-tool-calling")]
use harness_permission::{NoopDecisionPersistence, PermissionAuthority};
#[cfg(feature = "programmatic-tool-calling")]
use harness_sandbox::CodeSandbox;
use harness_sandbox::SandboxBackend;
use harness_tool::ToolPool;
#[cfg(feature = "subagent-tool")]
use harness_tool::ToolPoolFilter;
#[cfg(feature = "programmatic-tool-calling")]
use harness_tool::{
    AuthorizedToolCall, AuthorizedToolInput, NoopToolEventEmitter, OrchestratorContext,
    ToolOrchestrator,
};
use serde_json::Value;
#[cfg(feature = "subagent-tool")]
use std::collections::HashSet;

use crate::{EngineError, EngineId, EngineRunner, EventStream, RunContext, SessionHandle};

#[cfg(feature = "subagent-tool")]
use futures::StreamExt;

#[derive(Debug, Clone)]
pub struct SteeringMerge {
    pub body: String,
    pub applied_event: Event,
    pub already_persisted: bool,
}

#[async_trait::async_trait]
pub trait SteeringDrain: Send + Sync + 'static {
    async fn drain_and_merge(
        &self,
        session: &SessionHandle,
        run_id: RunId,
        merged_into_message_id: MessageId,
    ) -> Result<Option<SteeringMerge>, EngineError>;
}

#[derive(Clone)]
pub struct Engine {
    id: EngineId,
    pub(crate) event_store: Arc<dyn EventStore>,
    pub(crate) context: ContextEngine,
    pub(crate) hooks: HookDispatcher,
    pub(crate) model: Arc<dyn ModelProvider>,
    pub(crate) model_snapshot: ModelRuntimeSnapshot,
    pub(crate) model_middlewares: Vec<Arc<dyn InferMiddleware>>,
    pub(crate) pricing_snapshot_resolver: Option<Arc<dyn PricingSnapshotResolver>>,
    pub(crate) tools: ToolPool,
    pub(crate) permission_broker: Arc<dyn PermissionBroker>,
    pub(crate) workspace_root: PathBuf,
    pub(crate) model_id: String,
    pub(crate) model_extra: Value,
    pub(crate) protocol: ModelProtocol,
    pub(crate) system_prompt: Option<String>,
    pub(crate) sandbox: Option<Arc<dyn SandboxBackend>>,
    #[cfg(feature = "programmatic-tool-calling")]
    pub(crate) code_sandbox: Option<Arc<dyn CodeSandbox>>,
    pub(crate) cap_registry: Arc<CapabilityRegistry>,
    pub(crate) blob_store: Option<Arc<dyn BlobStore>>,
    pub(crate) tracer: Option<Arc<dyn Tracer>>,
    pub(crate) observer: Option<Arc<Observer>>,
    pub(crate) max_iterations: u32,
    pub(crate) steering_drain: Option<Arc<dyn SteeringDrain>>,
    #[cfg(feature = "subagent-tool")]
    pub(crate) mcp_registry: Option<McpRegistry>,
}

#[derive(Clone)]
pub struct EngineBuilder {
    id: EngineId,
    event_store: Option<Arc<dyn EventStore>>,
    context: Option<ContextEngine>,
    hooks: Option<HookDispatcher>,
    model: Option<Arc<dyn ModelProvider>>,
    model_snapshot: Option<ModelRuntimeSnapshot>,
    model_middlewares: Vec<Arc<dyn InferMiddleware>>,
    pricing_snapshot_resolver: Option<Arc<dyn PricingSnapshotResolver>>,
    tools: Option<ToolPool>,
    permission_broker: Option<Arc<dyn PermissionBroker>>,
    workspace_root: Option<PathBuf>,
    model_id: Option<String>,
    model_extra: Value,
    protocol: ModelProtocol,
    system_prompt: Option<String>,
    sandbox: Option<Arc<dyn SandboxBackend>>,
    #[cfg(feature = "programmatic-tool-calling")]
    code_sandbox: Option<Arc<dyn CodeSandbox>>,
    cap_registry: Option<Arc<CapabilityRegistry>>,
    cap_overrides: CapabilityRegistry,
    blob_store: Option<Arc<dyn BlobStore>>,
    tracer: Option<Arc<dyn Tracer>>,
    observer: Option<Arc<Observer>>,
    max_iterations: u32,
    steering_drain: Option<Arc<dyn SteeringDrain>>,
    #[cfg(feature = "subagent-tool")]
    subagent_tool_enabled: bool,
    #[cfg(feature = "subagent-tool")]
    subagent_watchdog_interval: Duration,
    #[cfg(feature = "subagent-tool")]
    mcp_registry: Option<McpRegistry>,
}

impl Engine {
    #[must_use]
    pub fn builder() -> EngineBuilder {
        EngineBuilder::default()
    }

    #[must_use]
    pub fn engine_id(&self) -> EngineId {
        self.id.clone()
    }

    #[must_use]
    pub fn into_builder(self) -> EngineBuilder {
        EngineBuilder {
            id: self.id,
            event_store: Some(self.event_store),
            context: Some(self.context),
            hooks: Some(self.hooks),
            model: Some(self.model),
            model_snapshot: Some(self.model_snapshot),
            model_middlewares: self.model_middlewares,
            pricing_snapshot_resolver: self.pricing_snapshot_resolver,
            tools: Some(self.tools),
            permission_broker: Some(self.permission_broker),
            workspace_root: Some(self.workspace_root),
            model_id: Some(self.model_id),
            model_extra: self.model_extra,
            protocol: self.protocol,
            system_prompt: self.system_prompt,
            sandbox: self.sandbox,
            #[cfg(feature = "programmatic-tool-calling")]
            code_sandbox: self.code_sandbox,
            cap_registry: Some(self.cap_registry),
            cap_overrides: CapabilityRegistry::default(),
            blob_store: self.blob_store,
            tracer: self.tracer,
            observer: self.observer,
            max_iterations: self.max_iterations,
            steering_drain: self.steering_drain,
            #[cfg(feature = "subagent-tool")]
            subagent_tool_enabled: false,
            #[cfg(feature = "subagent-tool")]
            subagent_watchdog_interval: Duration::from_secs(30),
            #[cfg(feature = "subagent-tool")]
            mcp_registry: self.mcp_registry,
        }
    }
}

impl Default for EngineBuilder {
    fn default() -> Self {
        Self {
            id: EngineId::new("default-engine"),
            event_store: None,
            context: None,
            hooks: None,
            model: None,
            model_snapshot: None,
            model_middlewares: Vec::new(),
            pricing_snapshot_resolver: None,
            tools: None,
            permission_broker: None,
            workspace_root: None,
            model_id: None,
            model_extra: Value::Null,
            protocol: ModelProtocol::Messages,
            system_prompt: None,
            sandbox: None,
            #[cfg(feature = "programmatic-tool-calling")]
            code_sandbox: None,
            cap_registry: None,
            cap_overrides: CapabilityRegistry::default(),
            blob_store: None,
            tracer: None,
            observer: None,
            max_iterations: 25,
            steering_drain: None,
            #[cfg(feature = "subagent-tool")]
            subagent_tool_enabled: false,
            #[cfg(feature = "subagent-tool")]
            subagent_watchdog_interval: Duration::from_secs(30),
            #[cfg(feature = "subagent-tool")]
            mcp_registry: None,
        }
    }
}

impl EngineBuilder {
    #[must_use]
    pub fn with_engine_id(mut self, id: EngineId) -> Self {
        self.id = id;
        self
    }

    #[must_use]
    pub fn with_event_store(mut self, event_store: Arc<dyn EventStore>) -> Self {
        self.event_store = Some(event_store);
        self
    }

    #[must_use]
    pub fn with_context(mut self, context: ContextEngine) -> Self {
        self.context = Some(context);
        self
    }

    #[must_use]
    pub fn with_hooks(mut self, hooks: HookDispatcher) -> Self {
        self.hooks = Some(hooks);
        self
    }

    #[must_use]
    pub fn with_model(mut self, model: Arc<dyn ModelProvider>) -> Self {
        self.model = Some(model);
        self
    }

    #[must_use]
    pub fn with_model_snapshot(mut self, model_snapshot: ModelRuntimeSnapshot) -> Self {
        self.model_snapshot = Some(model_snapshot);
        self
    }

    #[must_use]
    pub fn with_model_middleware(mut self, middleware: Arc<dyn InferMiddleware>) -> Self {
        self.model_middlewares.push(middleware);
        self
    }

    #[must_use]
    pub fn with_model_middlewares<I>(mut self, middlewares: I) -> Self
    where
        I: IntoIterator<Item = Arc<dyn InferMiddleware>>,
    {
        self.model_middlewares.extend(middlewares);
        self
    }

    #[must_use]
    pub fn with_pricing_snapshot_resolver(
        mut self,
        resolver: Arc<dyn PricingSnapshotResolver>,
    ) -> Self {
        self.pricing_snapshot_resolver = Some(resolver);
        self
    }

    #[must_use]
    pub fn with_tools(mut self, tools: ToolPool) -> Self {
        self.tools = Some(tools);
        self
    }

    #[must_use]
    pub fn with_permission_broker(mut self, permission_broker: Arc<dyn PermissionBroker>) -> Self {
        self.permission_broker = Some(permission_broker);
        self
    }

    #[must_use]
    pub fn with_workspace_root(mut self, workspace_root: impl AsRef<Path>) -> Self {
        self.workspace_root = Some(workspace_root.as_ref().to_path_buf());
        self
    }

    #[must_use]
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = Some(model_id.into());
        self
    }

    #[must_use]
    pub fn with_model_extra(mut self, model_extra: Value) -> Self {
        self.model_extra = model_extra;
        self
    }

    #[must_use]
    pub fn with_protocol(mut self, protocol: ModelProtocol) -> Self {
        self.protocol = protocol;
        self
    }

    #[must_use]
    pub fn with_system_prompt(mut self, system_prompt: Option<impl Into<String>>) -> Self {
        self.system_prompt = system_prompt.map(Into::into);
        self
    }

    #[must_use]
    pub fn with_sandbox(mut self, sandbox: Arc<dyn SandboxBackend>) -> Self {
        self.sandbox = Some(sandbox);
        self
    }

    #[must_use]
    pub fn without_sandbox(mut self) -> Self {
        self.sandbox = None;
        self
    }

    #[cfg(feature = "programmatic-tool-calling")]
    #[must_use]
    pub fn with_code_sandbox(mut self, sandbox: Arc<dyn CodeSandbox>) -> Self {
        self.code_sandbox = Some(sandbox);
        self
    }

    #[must_use]
    pub fn with_cap_registry(mut self, cap_registry: Arc<CapabilityRegistry>) -> Self {
        self.cap_registry = Some(cap_registry);
        self
    }

    #[must_use]
    pub fn with_capability<T>(mut self, capability: ToolCapability, implementation: Arc<T>) -> Self
    where
        T: ?Sized + Send + Sync + 'static,
    {
        self.cap_overrides.install(capability, implementation);
        self
    }

    #[must_use]
    pub fn with_blob_store(mut self, blob_store: Arc<dyn BlobStore>) -> Self {
        self.blob_store = Some(blob_store);
        self
    }

    #[must_use]
    pub fn with_tracer(mut self, tracer: Arc<dyn Tracer>) -> Self {
        self.tracer = Some(tracer);
        self
    }

    #[must_use]
    pub fn with_observer(mut self, observer: Arc<Observer>) -> Self {
        self.observer = Some(observer);
        self
    }

    #[must_use]
    pub fn with_max_iterations(mut self, max_iterations: u32) -> Self {
        self.max_iterations = max_iterations.max(1);
        self
    }

    #[must_use]
    pub fn with_steering_drain(mut self, steering_drain: Arc<dyn SteeringDrain>) -> Self {
        self.steering_drain = Some(steering_drain);
        self
    }

    #[cfg(feature = "subagent-tool")]
    #[must_use]
    pub fn with_subagent_tool(mut self) -> Self {
        self.subagent_tool_enabled = true;
        self
    }

    #[cfg(feature = "subagent-tool")]
    #[must_use]
    pub fn with_subagent_watchdog_interval(mut self, interval: Duration) -> Self {
        self.subagent_watchdog_interval = interval;
        self
    }

    #[cfg(feature = "subagent-tool")]
    #[must_use]
    pub fn with_mcp_registry(mut self, registry: McpRegistry) -> Self {
        self.mcp_registry = Some(registry);
        self
    }

    pub fn build(self) -> Result<Engine, harness_contracts::EngineError> {
        let event_store = self.event_store.ok_or_else(|| {
            harness_contracts::EngineError::Message("event store missing".to_owned())
        })?;
        let context = self.context.ok_or_else(|| {
            harness_contracts::EngineError::Message("context engine missing".to_owned())
        })?;
        let hooks = self.hooks.ok_or_else(|| {
            harness_contracts::EngineError::Message("hook dispatcher missing".to_owned())
        })?;
        let model = self.model.ok_or_else(|| {
            harness_contracts::EngineError::Message("model provider missing".to_owned())
        })?;
        let tools = self.tools.ok_or_else(|| {
            harness_contracts::EngineError::Message("tool pool missing".to_owned())
        })?;
        let permission_broker = self.permission_broker.ok_or_else(|| {
            harness_contracts::EngineError::Message("permission broker missing".to_owned())
        })?;
        let workspace_root = self.workspace_root.ok_or_else(|| {
            harness_contracts::EngineError::Message("workspace root missing".to_owned())
        })?;
        let model_id = self.model_id.ok_or_else(|| {
            harness_contracts::EngineError::Message("model id missing".to_owned())
        })?;
        let model_snapshot = match self.model_snapshot {
            Some(model_snapshot) => model_snapshot,
            None => model
                .snapshot_for_model(&model_id)
                .map_err(|error| harness_contracts::EngineError::Message(error.to_string()))?,
        };
        let assembled_cap_registry = crate::capability_assembly::assemble_capability_registry(
            self.cap_registry.as_ref(),
            &event_store,
            self.blob_store.as_ref(),
            &self.cap_overrides,
        );
        let mut cap_registry_value = assembled_cap_registry.as_ref().clone();
        if !cap_registry_value.contains(&ToolCapability::ContextPatchSink) {
            cap_registry_value.install::<dyn harness_contracts::ContextPatchSinkCap>(
                ToolCapability::ContextPatchSink,
                Arc::new(context.clone()),
            );
        }
        #[cfg(feature = "programmatic-tool-calling")]
        if let Some(code_sandbox) = &self.code_sandbox {
            cap_registry_value.install::<dyn harness_contracts::CodeRuntimeCap>(
                ToolCapability::CodeRuntime,
                Arc::new(EngineCodeRuntimeCap {
                    sandbox: Arc::clone(code_sandbox),
                }),
            );
            if !cap_registry_value.contains(&ToolCapability::EmbeddedToolDispatcher) {
                cap_registry_value.install::<dyn harness_contracts::EmbeddedToolDispatcherCap>(
                    ToolCapability::EmbeddedToolDispatcher,
                    Arc::new(EngineEmbeddedToolDispatcher {
                        tools: tools.clone(),
                        workspace_root: workspace_root.clone(),
                        sandbox: self.sandbox.clone(),
                        permission_broker: Arc::clone(&permission_broker),
                        event_store: Arc::clone(&event_store),
                        cap_registry: Arc::new(cap_registry_value.clone()),
                        redactor: self
                            .observer
                            .as_ref()
                            .map(|observer| Arc::clone(&observer.redactor))
                            .unwrap_or_else(|| Arc::new(DefaultRedactor::default())),
                        blob_store: self.blob_store.clone(),
                    }),
                );
            }
        }
        #[cfg(feature = "subagent-tool")]
        let mut tools = tools;
        #[cfg(feature = "subagent-tool")]
        let mut self_child_runner = None;
        #[cfg(feature = "subagent-tool")]
        if self.subagent_tool_enabled {
            if !cap_registry_value.contains(&ToolCapability::SubagentRunner) {
                let child_runner = Arc::new(EngineBoundSubagentFactory::default());
                let runner = Arc::new(
                    harness_subagent::DefaultSubagentRunner::new_with_engine_factory(
                        Arc::clone(&child_runner)
                            as Arc<dyn harness_subagent::SubagentEngineFactory>,
                        Arc::clone(&event_store),
                        workspace_root.clone(),
                        harness_subagent::DelegationPolicy::default(),
                    )
                    .with_watchdog_interval(self.subagent_watchdog_interval),
                );
                cap_registry_value.install::<dyn harness_contracts::SubagentRunnerCap>(
                    ToolCapability::SubagentRunner,
                    harness_subagent::SubagentRunnerCapAdapter::from_runner(runner),
                );
                self_child_runner = Some(child_runner);
            }
            tools.append_runtime_tool(Arc::new(harness_subagent::AgentTool::default()));
        }
        let cap_registry = Arc::new(cap_registry_value);
        validate_tool_capabilities(&tools, &cap_registry)?;

        let tracer = self.tracer.or_else(|| {
            self.observer
                .as_ref()
                .map(|observer| Arc::clone(observer) as Arc<dyn Tracer>)
        });
        let engine = Engine {
            id: self.id,
            event_store,
            context,
            hooks,
            model,
            model_snapshot,
            model_middlewares: self.model_middlewares,
            pricing_snapshot_resolver: self.pricing_snapshot_resolver,
            tools,
            permission_broker,
            workspace_root,
            model_id,
            model_extra: self.model_extra,
            protocol: self.protocol,
            system_prompt: self.system_prompt,
            sandbox: self.sandbox,
            #[cfg(feature = "programmatic-tool-calling")]
            code_sandbox: self.code_sandbox,
            cap_registry,
            blob_store: self.blob_store,
            tracer,
            observer: self.observer,
            max_iterations: self.max_iterations,
            steering_drain: self.steering_drain,
            #[cfg(feature = "subagent-tool")]
            mcp_registry: self.mcp_registry,
        };
        #[cfg(feature = "subagent-tool")]
        if let Some(child_runner) = self_child_runner {
            child_runner.bind_engine(engine.clone()).map_err(|()| {
                harness_contracts::EngineError::Message(
                    "engine self runner already initialized".to_owned(),
                )
            })?;
        }
        Ok(engine)
    }
}

impl Engine {
    #[must_use]
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.get(name).is_some()
    }

    #[must_use]
    pub fn model_ref(&self) -> ModelRef {
        ModelRef {
            provider_id: self.model_snapshot.provider_id.clone(),
            model_id: self.model_snapshot.model_id.clone(),
        }
    }

    #[must_use]
    pub fn tool_pool(&self) -> &ToolPool {
        &self.tools
    }

    #[must_use]
    pub fn context_engine(&self) -> ContextEngine {
        self.context.clone()
    }

    #[must_use]
    pub fn cap_registry(&self) -> Arc<CapabilityRegistry> {
        Arc::clone(&self.cap_registry)
    }

    #[must_use]
    pub fn sandbox_backend_id(&self) -> Option<String> {
        self.sandbox
            .as_ref()
            .map(|sandbox| sandbox.backend_id().to_owned())
    }

    #[must_use]
    pub fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    #[must_use]
    pub fn has_capability(&self, capability: &ToolCapability) -> bool {
        self.cap_registry.contains(capability)
    }
}

#[cfg(feature = "programmatic-tool-calling")]
struct EngineCodeRuntimeCap {
    sandbox: Arc<dyn CodeSandbox>,
}

#[cfg(feature = "programmatic-tool-calling")]
impl harness_contracts::CodeRuntimeCap for EngineCodeRuntimeCap {
    fn run_code(
        &self,
        request: CodeRunRequest,
        dispatcher: Arc<dyn harness_contracts::EmbeddedToolDispatcherCap>,
    ) -> futures::future::BoxFuture<'static, Result<CodeRunResult, harness_contracts::CodeRunError>>
    {
        let sandbox = Arc::clone(&self.sandbox);
        Box::pin(async move {
            let source_hash = blake3::hash(request.source.as_bytes());
            let script = harness_sandbox::CompiledScript {
                language: match request.language {
                    CodeLanguage::MiniLua => harness_sandbox::ScriptLanguage::MiniLua,
                },
                source_hash: *source_hash.as_bytes(),
                bytecode: request.source.into_bytes(),
            };
            let events = Arc::new(Mutex::new(Vec::new()));
            let usage = Arc::new(EngineCodeUsageMeter::default());
            let result = sandbox
                .run(
                    &script,
                    harness_sandbox::CodeSandboxRunContext {
                        session_id: request.session_id,
                        run_id: request.run_id,
                        parent_tool_use_id: request.tool_use_id,
                        embedded_dispatcher: Arc::new(SandboxEmbeddedDispatcher {
                            inner: dispatcher,
                            tenant_id: request.tenant_id,
                            session_id: request.session_id,
                            run_id: request.run_id,
                            parent_tool_use_id: request.tool_use_id,
                            step_seq: AtomicU32::new(1),
                            events: Arc::clone(&events),
                        }),
                        usage_meter: usage.clone(),
                        event_sink: Arc::new(EngineCodeEventSink {
                            events: Arc::clone(&events),
                        }),
                    },
                )
                .await
                .map_err(|error| harness_contracts::CodeRunError {
                    error: ToolError::Sandbox(error),
                    events: events
                        .lock()
                        .map(|events| events.clone())
                        .unwrap_or_default(),
                })?;

            Ok(CodeRunResult {
                value: lua_value_to_json(result.value),
                stats: CodeRunStats {
                    instructions: result.stats.instructions,
                    embedded_call_count: result.stats.embedded_call_count,
                },
                embedded_steps: result
                    .embedded_steps
                    .into_iter()
                    .map(|step| EmbeddedToolDispatchResponse {
                        tool_use_id: step.tool_use_id,
                        tool_name: step.tool_name,
                        output: serde_json::from_str(&step.output_json)
                            .unwrap_or(serde_json::Value::String(step.output_json)),
                        duration_ms: step.duration_ms,
                        overflow: step.overflow,
                    })
                    .collect(),
                events: events
                    .lock()
                    .map(|events| events.clone())
                    .unwrap_or_default(),
            })
        })
    }
}

#[cfg(feature = "programmatic-tool-calling")]
struct SandboxEmbeddedDispatcher {
    inner: Arc<dyn harness_contracts::EmbeddedToolDispatcherCap>,
    tenant_id: harness_contracts::TenantId,
    session_id: harness_contracts::SessionId,
    run_id: RunId,
    parent_tool_use_id: ToolUseId,
    step_seq: AtomicU32,
    events: Arc<Mutex<Vec<Event>>>,
}

#[cfg(feature = "programmatic-tool-calling")]
#[async_trait::async_trait]
impl harness_sandbox::EmbeddedToolDispatcherCap for SandboxEmbeddedDispatcher {
    async fn dispatch(
        &self,
        request: harness_sandbox::EmbeddedToolCall,
    ) -> Result<harness_sandbox::EmbeddedStepSummary, harness_contracts::SandboxError> {
        let started = std::time::Instant::now();
        let args_hash = blake3::hash(request.input_json.as_bytes());
        let input = serde_json::from_str(&request.input_json)
            .map_err(|error| harness_contracts::SandboxError::Message(error.to_string()))?;
        let response = match self
            .inner
            .dispatch_embedded(EmbeddedToolDispatchRequest {
                tenant_id: self.tenant_id,
                session_id: self.session_id,
                run_id: self.run_id,
                parent_tool_use_id: self.parent_tool_use_id,
                tool_name: request.name.clone(),
                input,
            })
            .await
        {
            Ok(response) => response,
            Err(error) => {
                let duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                let refused_reason = match &error {
                    ToolError::PermissionDenied(_) => Some(EmbeddedRefusedReason::PermissionDenied),
                    ToolError::CapabilityMissing(_) => {
                        Some(EmbeddedRefusedReason::CapabilityDenied)
                    }
                    _ => None,
                };
                if let Some(refused_reason) = refused_reason {
                    if let Ok(mut events) = self.events.lock() {
                        events.push(Event::ExecuteCodeStepInvoked(ExecuteCodeStepInvokedEvent {
                            parent_tool_use_id: self.parent_tool_use_id,
                            run_id: self.run_id,
                            session_id: self.session_id,
                            embedded_tool: request.name,
                            args_hash: *args_hash.as_bytes(),
                            step_seq: self.step_seq.fetch_add(1, Ordering::SeqCst),
                            duration_ms,
                            overflow: None,
                            refused_reason: Some(refused_reason),
                            at: harness_contracts::now(),
                        }));
                    }
                }
                return Err(harness_contracts::SandboxError::Message(error.to_string()));
            }
        };
        let duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        if let Ok(mut events) = self.events.lock() {
            events.push(Event::ExecuteCodeStepInvoked(ExecuteCodeStepInvokedEvent {
                parent_tool_use_id: self.parent_tool_use_id,
                run_id: self.run_id,
                session_id: self.session_id,
                embedded_tool: request.name,
                args_hash: *args_hash.as_bytes(),
                step_seq: self.step_seq.fetch_add(1, Ordering::SeqCst),
                duration_ms,
                overflow: response.overflow.clone(),
                refused_reason: None,
                at: harness_contracts::now(),
            }));
        }
        Ok(harness_sandbox::EmbeddedStepSummary {
            tool_use_id: response.tool_use_id,
            tool_name: response.tool_name,
            output_json: serde_json::to_string(&response.output)
                .unwrap_or_else(|_| "null".to_owned()),
            duration_ms: response.duration_ms,
            overflow: response.overflow,
        })
    }
}

#[cfg(feature = "programmatic-tool-calling")]
#[derive(Default)]
struct EngineCodeUsageMeter {
    instructions: std::sync::atomic::AtomicU64,
}

#[cfg(feature = "programmatic-tool-calling")]
impl harness_sandbox::UsageMeter for EngineCodeUsageMeter {
    fn record_instructions(&self, count: u64) {
        self.instructions.fetch_add(count, Ordering::SeqCst);
    }

    fn record_event(&self, _event: Event) {}
}

#[cfg(feature = "programmatic-tool-calling")]
struct EngineCodeEventSink {
    events: Arc<Mutex<Vec<Event>>>,
}

#[cfg(feature = "programmatic-tool-calling")]
impl harness_sandbox::EventSink for EngineCodeEventSink {
    fn emit(&self, event: Event) -> Result<(), harness_contracts::SandboxError> {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
        Ok(())
    }
}

#[cfg(feature = "programmatic-tool-calling")]
struct EngineEmbeddedToolDispatcher {
    tools: ToolPool,
    workspace_root: PathBuf,
    sandbox: Option<Arc<dyn SandboxBackend>>,
    permission_broker: Arc<dyn PermissionBroker>,
    event_store: Arc<dyn EventStore>,
    cap_registry: Arc<CapabilityRegistry>,
    redactor: Arc<dyn Redactor>,
    blob_store: Option<Arc<dyn BlobStore>>,
}

#[cfg(feature = "programmatic-tool-calling")]
impl harness_contracts::EmbeddedToolDispatcherCap for EngineEmbeddedToolDispatcher {
    fn dispatch_embedded(
        &self,
        request: EmbeddedToolDispatchRequest,
    ) -> futures::future::BoxFuture<'static, Result<EmbeddedToolDispatchResponse, ToolError>> {
        let this = self.clone_for_dispatch();
        Box::pin(async move { this.dispatch_embedded_inner(request).await })
    }
}

#[cfg(feature = "programmatic-tool-calling")]
impl EngineEmbeddedToolDispatcher {
    fn clone_for_dispatch(&self) -> Self {
        Self {
            tools: self.tools.clone(),
            workspace_root: self.workspace_root.clone(),
            sandbox: self.sandbox.clone(),
            permission_broker: Arc::clone(&self.permission_broker),
            event_store: Arc::clone(&self.event_store),
            cap_registry: Arc::clone(&self.cap_registry),
            redactor: Arc::clone(&self.redactor),
            blob_store: self.blob_store.clone(),
        }
    }

    async fn dispatch_embedded_inner(
        self,
        request: EmbeddedToolDispatchRequest,
    ) -> Result<EmbeddedToolDispatchResponse, ToolError> {
        let tool_use_id = ToolUseId::new();
        let tool_ctx = harness_tool::ToolContext {
            tool_use_id,
            run_id: request.run_id,
            session_id: request.session_id,
            tenant_id: request.tenant_id,
            correlation_id: harness_contracts::CorrelationId::new(),
            agent_id: harness_contracts::AgentId::from_u128(1),
            subagent_depth: 0,
            workspace_root: self.workspace_root.clone(),
            sandbox: self.sandbox.clone(),
            cap_registry: Arc::clone(&self.cap_registry),
            redactor: Arc::clone(&self.redactor),
            interrupt: harness_tool::InterruptToken::default(),
            parent_run: None,
            model: None,
            model_config_id: None,
        };

        let tool = self.tools.get(&request.tool_name).ok_or_else(|| {
            ToolError::Internal(format!("embedded tool not found: {}", request.tool_name))
        })?;
        tool.validate(&request.input, &tool_ctx)
            .await
            .map_err(|error| ToolError::Validation(error.to_string()))?;
        let plan = tool.plan(&request.input, &tool_ctx).await?;

        let authority = PermissionAuthority::builder()
            .with_policy_broker(Arc::clone(&self.permission_broker))
            .with_transient_decision_store(Arc::new(NoopDecisionPersistence))
            .build()
            .map_err(|error| ToolError::Internal(error.to_string()))?;
        let ticket_ledger = Arc::new(TicketLedger::default());
        let sandbox = self.sandbox.clone().ok_or_else(|| {
            ToolError::PermissionDenied(
                "sandbox required for embedded tool authorization".to_owned(),
            )
        })?;
        let auth_service = AuthorizationService::new(
            Arc::new(authority),
            sandbox,
            Arc::new(JournalAuthorizationEventSink::new(Arc::clone(
                &self.event_store,
            ))),
            ticket_ledger.clone(),
        );
        let auth_context = AuthorizationContext {
            tenant_id: request.tenant_id,
            session_id: request.session_id,
            run_id: request.run_id,
            permission_mode: PermissionMode::Default,
            interactivity: InteractivityLevel::FullyInteractive,
            fallback_policy: FallbackPolicy::DenyAll,
            workspace_root: self.workspace_root,
        };
        let outcome = auth_service
            .authorize_plan(auth_context, plan.clone())
            .await
            .map_err(|error| match error {
                ExecutionError::PermissionDenied { decision, .. } => {
                    ToolError::PermissionDenied(format!("embedded tool denied: {decision:?}"))
                }
                other => ToolError::Internal(other.to_string()),
            })?;
        let consumed = ticket_ledger
            .consume(outcome.ticket.id, &outcome.ticket.claims, Utc::now())
            .map_err(|error| ToolError::Internal(error.to_string()))?;
        let authorized_input = AuthorizedToolInput::new(
            request.input,
            plan,
            harness_tool::AuthorizedTicketSummary {
                ticket_id: consumed.id,
                tenant_id: consumed.claims.tenant_id,
                session_id: consumed.claims.session_id,
                run_id: consumed.claims.run_id,
                tool_use_id: consumed.claims.tool_use_id,
                tool_name: consumed.claims.tool_name,
                action_plan_hash: consumed.claims.action_plan_hash,
                consumed_at: Utc::now(),
            },
        )?;
        let results = ToolOrchestrator::new(1)
            .dispatch(
                vec![AuthorizedToolCall {
                    tool_use_id,
                    tool_name: request.tool_name.clone(),
                    input: authorized_input,
                }],
                OrchestratorContext {
                    pool: self.tools,
                    tool_context: tool_ctx,
                    blob_store: self.blob_store,
                    event_emitter: Arc::new(NoopToolEventEmitter),
                },
            )
            .await;
        let result = results
            .into_iter()
            .next()
            .ok_or_else(|| ToolError::Internal("embedded tool returned no result".to_owned()))?;
        let output = match result.result? {
            ToolResult::Structured(value) => value,
            ToolResult::Text(text) => serde_json::Value::String(text),
            other => serde_json::to_value(other)
                .map_err(|error| ToolError::Internal(error.to_string()))?,
        };
        Ok(EmbeddedToolDispatchResponse {
            tool_use_id,
            tool_name: request.tool_name,
            output,
            duration_ms: result.duration.as_millis().min(u128::from(u64::MAX)) as u64,
            overflow: result.overflow,
        })
    }
}

#[cfg(feature = "programmatic-tool-calling")]
struct JournalAuthorizationEventSink {
    event_store: Arc<dyn EventStore>,
}

#[cfg(feature = "programmatic-tool-calling")]
impl JournalAuthorizationEventSink {
    fn new(event_store: Arc<dyn EventStore>) -> Self {
        Self { event_store }
    }
}

#[cfg(feature = "programmatic-tool-calling")]
#[async_trait::async_trait]
impl harness_execution::AuthorizationEventSink for JournalAuthorizationEventSink {
    async fn emit_batch(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        events: Vec<Event>,
    ) -> Result<(), ExecutionError> {
        self.event_store
            .append(tenant_id, session_id, &events)
            .await
            .map(|_| ())
            .map_err(|error| ExecutionError::EventSinkFailed {
                reason: error.to_string(),
            })
    }
}

#[cfg(feature = "programmatic-tool-calling")]
fn lua_value_to_json(value: harness_sandbox::LuaValue) -> serde_json::Value {
    match value {
        harness_sandbox::LuaValue::Nil => serde_json::Value::Null,
        harness_sandbox::LuaValue::Bool(value) => serde_json::Value::Bool(value),
        harness_sandbox::LuaValue::Number(value) => json_number(value),
        harness_sandbox::LuaValue::String(value) => serde_json::Value::String(value),
    }
}

#[cfg(feature = "programmatic-tool-calling")]
fn json_number(value: f64) -> serde_json::Value {
    serde_json::Number::from_f64(value)
        .map(serde_json::Value::Number)
        .unwrap_or(serde_json::Value::Null)
}

#[cfg(feature = "subagent-tool")]
#[derive(Default)]
pub struct EngineBoundSubagentFactory {
    engine: tokio::sync::OnceCell<Engine>,
}

#[cfg(feature = "subagent-tool")]
impl EngineBoundSubagentFactory {
    pub fn bind_engine(&self, engine: Engine) -> Result<(), ()> {
        self.engine.set(engine).map_err(|_| ())
    }
}

#[cfg(feature = "subagent-tool")]
struct ChildCancellationBridge {
    handle: tokio::task::JoinHandle<()>,
}

#[cfg(feature = "subagent-tool")]
impl ChildCancellationBridge {
    fn spawn(
        child_cancellation: harness_subagent::SubagentCancellationToken,
        engine_cancellation: crate::CancellationToken,
    ) -> Self {
        let handle = tokio::spawn(async move {
            child_cancellation.cancelled().await;
            engine_cancellation.cancel(crate::InterruptCause::Parent);
        });
        Self { handle }
    }
}

#[cfg(feature = "subagent-tool")]
impl Drop for ChildCancellationBridge {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[cfg(feature = "subagent-tool")]
#[async_trait::async_trait]
impl harness_subagent::SubagentEngineFactory for EngineBoundSubagentFactory {
    async fn run_child_engine(
        &self,
        request: harness_subagent::ChildRunRequest,
    ) -> Result<harness_subagent::ChildRunOutcome, harness_subagent::SubagentError> {
        let engine = self.engine.get().ok_or_else(|| {
            harness_subagent::SubagentError::Engine("engine self runner missing".to_owned())
        })?;
        if request.cancellation.is_cancelled() {
            return Err(harness_subagent::SubagentError::Cancelled);
        }
        let full_transcript = matches!(
            request.spec.announce_mode,
            harness_subagent::AnnounceMode::FullTranscript
        );
        if full_transcript && engine.blob_store.is_none() {
            return Err(harness_subagent::SubagentError::Engine(
                "subagent full transcript requires blob store".to_owned(),
            ));
        }
        let tenant_id = request.tenant_id;
        let child_session_id = request.child_session_id;
        let (child_engine, context_report) = scoped_child_engine(engine, &request).await?;
        let cancellation = crate::CancellationToken::new();
        let _cancellation_bridge =
            ChildCancellationBridge::spawn(request.cancellation.clone(), cancellation.clone());

        let stream = child_engine
            .run(
                SessionHandle {
                    tenant_id: request.tenant_id,
                    session_id: request.child_session_id,
                },
                request.input,
                RunContext::new(
                    request.tenant_id,
                    request.child_session_id,
                    request.child_run_id,
                )
                .with_parent_run_id(Some(request.parent_run_id))
                .with_correlation_id(request.correlation_id)
                .with_subagent_depth(request.child_depth)
                .with_permission_actor_source(PermissionActorSource::Subagent {
                    subagent_id: request.subagent_id,
                    parent_session_id: request.parent_session_id,
                    parent_run_id: request.parent_run_id,
                    team_id: None,
                    team_member_profile_id: None,
                })
                .with_permission_mode(request.spec.permission_mode)
                .with_interactivity(interactivity_level(request.spec.interactivity.clone()))
                .with_budget_limits(subagent_budget_limits(&request.spec))
                .with_context_seed(request.context_seed.clone())
                .with_cancellation(cancellation),
            )
            .await
            .map_err(|error| harness_subagent::SubagentError::Engine(error.to_string()))?;
        let events: Vec<_> = stream.collect().await;
        let transcript_ref = if full_transcript {
            Some(write_child_transcript_ref(&child_engine, tenant_id, child_session_id).await?)
        } else {
            None
        };
        Ok(child_run_outcome(
            &events,
            transcript_ref,
            Some(context_report),
        ))
    }
}

#[cfg(feature = "subagent-tool")]
async fn write_child_transcript_ref(
    engine: &Engine,
    tenant_id: harness_contracts::TenantId,
    child_session_id: harness_contracts::SessionId,
) -> Result<TranscriptRef, harness_subagent::SubagentError> {
    let blob_store = engine.blob_store.as_ref().ok_or_else(|| {
        harness_subagent::SubagentError::Engine(
            "subagent full transcript requires blob store".to_owned(),
        )
    })?;
    let envelopes: Vec<_> = engine
        .event_store
        .read_envelopes(
            tenant_id,
            child_session_id,
            harness_journal::ReplayCursor::FromStart,
        )
        .await
        .map_err(|error| harness_subagent::SubagentError::Engine(error.to_string()))?
        .collect()
        .await;
    let from_offset = envelopes
        .first()
        .map(|envelope| envelope.offset)
        .unwrap_or(JournalOffset(0));
    let to_offset = envelopes
        .last()
        .map(|envelope| envelope.offset)
        .unwrap_or(from_offset);
    let body = Bytes::from(
        serde_json::to_vec(&envelopes)
            .map_err(|error| harness_subagent::SubagentError::Engine(error.to_string()))?,
    );
    let meta = BlobMeta {
        content_type: Some("application/json".to_owned()),
        size: body.len() as u64,
        content_hash: *blake3::hash(&body).as_bytes(),
        created_at: Utc::now(),
        retention: BlobRetention::SessionScoped(child_session_id),
    };
    let blob = blob_store
        .put(tenant_id, body, meta)
        .await
        .map_err(|error| harness_subagent::SubagentError::Engine(error.to_string()))?;
    Ok(TranscriptRef {
        blob,
        from_offset,
        to_offset,
    })
}

#[cfg(feature = "subagent-tool")]
async fn scoped_child_engine(
    engine: &Engine,
    request: &harness_subagent::ChildRunRequest,
) -> Result<(Engine, SubagentContextReport), harness_subagent::SubagentError> {
    if let harness_subagent::ToolsetSelector::Preset(preset) = &request.spec.toolset {
        return Err(harness_subagent::SubagentError::Engine(format!(
            "subagent toolset preset is not configured: {preset}"
        )));
    }
    let bootstrap_files =
        resolve_bootstrap_files(&engine.workspace_root, &request.spec.bootstrap_filter)?;
    let missing_mcp =
        missing_required_mcp_servers(&engine.tools, engine.mcp_registry.as_ref(), &request.spec)
            .await;
    if !missing_mcp.is_empty() {
        return Err(harness_subagent::SubagentError::McpRequirementUnsatisfied(
            missing_mcp,
        ));
    }
    let untrusted_mcp = requested_mcp_trust_violations(&engine.tools, &request.spec);
    if !untrusted_mcp.is_empty() {
        return Err(harness_subagent::SubagentError::McpRequirementUnsatisfied(
            untrusted_mcp,
        ));
    }

    let tools = engine
        .tools
        .filtered(&child_tool_filter(&engine.tools, &request.spec));
    let mut builder = engine
        .clone()
        .into_builder()
        .with_tools(tools)
        .with_max_iterations(request.spec.max_turns);
    builder = match &request.spec.sandbox_policy {
        harness_subagent::SandboxInheritance::Inherit => builder,
        harness_subagent::SandboxInheritance::Empty => builder.without_sandbox(),
        harness_subagent::SandboxInheritance::Require(required) => {
            let sandbox = engine.sandbox.as_ref().ok_or_else(|| {
                harness_subagent::SubagentError::Engine(
                    "required sandbox is not available".to_owned(),
                )
            })?;
            let missing = missing_required_sandbox_capabilities(
                sandbox.backend_id(),
                &sandbox.capabilities(),
                required,
            );
            if !missing.is_empty() {
                return Err(harness_subagent::SubagentError::Engine(format!(
                    "sandbox capability mismatch: {}",
                    missing.join(", ")
                )));
            }
            builder
        }
        harness_subagent::SandboxInheritance::Override(policy) => {
            let sandbox = engine.sandbox.as_ref().ok_or_else(|| {
                harness_subagent::SubagentError::Engine(
                    "sandbox override requires parent sandbox".to_owned(),
                )
            })?;
            let required = required_capabilities_for_policy(policy);
            let missing = missing_required_sandbox_capabilities(
                sandbox.backend_id(),
                &sandbox.capabilities(),
                &required,
            );
            if !missing.is_empty() {
                return Err(harness_subagent::SubagentError::Engine(format!(
                    "sandbox capability mismatch: {}",
                    missing.join(", ")
                )));
            }
            builder.with_sandbox(Arc::new(PolicyOverrideSandbox::new(
                Arc::clone(sandbox),
                policy.clone(),
            )))
        }
    };
    if matches!(
        request.spec.memory_scope,
        harness_subagent::SubagentMemoryScope::Empty
    ) {
        builder = builder.with_context(
            harness_context::ContextEngine::builder()
                .build()
                .map_err(|error| harness_subagent::SubagentError::Engine(error.to_string()))?,
        );
    } else if matches!(
        request.spec.memory_scope,
        harness_subagent::SubagentMemoryScope::Subset { .. }
    ) && !request.memory_scope_resolved
    {
        return Err(harness_subagent::SubagentError::Engine(
            "subagent memory_scope subset resolver is not configured".to_owned(),
        ));
    }
    let bootstrap_segment = bootstrap_system_segment(&bootstrap_files);
    let extra = request
        .spec
        .system_header_extra
        .as_deref()
        .filter(|extra| !extra.is_empty());
    let child_system = child_system_prompt(
        engine.system_prompt.as_deref(),
        bootstrap_segment.as_deref(),
        extra,
    );
    if !child_system.is_empty() {
        builder = builder.with_system_prompt(Some(child_system.clone()));
    }
    let context_report = subagent_context_report(
        engine.system_prompt.as_deref(),
        &child_system,
        bootstrap_files
            .iter()
            .map(|(filename, _)| filename.clone())
            .collect(),
        extra.is_some(),
    );
    let child_engine = builder
        .build()
        .map_err(|error| harness_subagent::SubagentError::Engine(error.to_string()))?;
    Ok((child_engine, context_report))
}

#[cfg(feature = "subagent-tool")]
fn missing_required_sandbox_capabilities(
    backend_id: &str,
    capabilities: &harness_sandbox::SandboxCapabilities,
    required: &harness_subagent::RequiredSandboxCapabilities,
) -> Vec<String> {
    let mut missing = Vec::new();
    if let Some(required_backend) = &required.backend_id {
        if backend_id != required_backend {
            missing.push(format!("backend_id={required_backend}"));
        }
    }
    if required.supports_streaming && !capabilities.supports_streaming {
        missing.push("supports_streaming".to_owned());
    }
    if required.supports_stdin && !capabilities.supports_stdin {
        missing.push("supports_stdin".to_owned());
    }
    if required.supports_cwd_tracking && !capabilities.supports_cwd_tracking {
        missing.push("supports_cwd_tracking".to_owned());
    }
    if required.supports_activity_heartbeat && !capabilities.supports_activity_heartbeat {
        missing.push("supports_activity_heartbeat".to_owned());
    }
    if required.supports_interactive_shell && !capabilities.supports_interactive_shell {
        missing.push("supports_interactive_shell".to_owned());
    }
    if required.supports_network && !capabilities.supports_network {
        missing.push("supports_network".to_owned());
    }
    if required.supports_filesystem_write && !capabilities.supports_filesystem_write {
        missing.push("supports_filesystem_write".to_owned());
    }
    if required.supports_gpu && !capabilities.supports_gpu {
        missing.push("supports_gpu".to_owned());
    }
    if required.supports_pty && !capabilities.supports_pty {
        missing.push("supports_pty".to_owned());
    }
    if required.supports_detach && !capabilities.supports_detach {
        missing.push("supports_detach".to_owned());
    }
    if required.supports_workspace_sync && !capabilities.supports_workspace_sync {
        missing.push("supports_workspace_sync".to_owned());
    }
    if required.supports_session_snapshot && !capabilities.supports_session_snapshot {
        missing.push("supports_session_snapshot".to_owned());
    }
    if let Some(min_concurrent_execs) = required.min_concurrent_execs {
        if capabilities.max_concurrent_execs < min_concurrent_execs {
            missing.push(format!("max_concurrent_execs>={min_concurrent_execs}"));
        }
    }
    for scope in &required.kill_scopes {
        if !capabilities.supports_kill_scope.contains(scope) {
            missing.push(format!("kill_scope={scope:?}"));
        }
    }
    for kind in &required.snapshot_kinds {
        if !capabilities.snapshot_kinds.contains(kind) {
            missing.push(format!("snapshot_kind={kind:?}"));
        }
    }
    missing
}

#[cfg(feature = "subagent-tool")]
const STANDARD_BOOTSTRAP_FILES: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    "GEMINI.md",
    "IDENTITY.md",
    "SOUL.md",
];

#[cfg(feature = "subagent-tool")]
fn resolve_bootstrap_files(
    workspace_root: &Path,
    filter: &harness_subagent::BootstrapFilter,
) -> Result<Vec<(String, String)>, harness_subagent::SubagentError> {
    let filenames: Vec<String> = match filter {
        harness_subagent::BootstrapFilter::ExcludeAll => return Ok(Vec::new()),
        harness_subagent::BootstrapFilter::Allow(filenames) => filenames.clone(),
        harness_subagent::BootstrapFilter::InheritAll => STANDARD_BOOTSTRAP_FILES
            .iter()
            .map(|filename| (*filename).to_owned())
            .collect(),
    };
    let mut files = Vec::new();
    for filename in filenames {
        validate_bootstrap_filename(&filename)?;
        let path = workspace_root.join(&filename);
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let content = fs::read_to_string(&path).map_err(|error| {
            harness_subagent::SubagentError::Engine(format!(
                "failed to read bootstrap file {filename}: {error}"
            ))
        })?;
        files.push((filename, content));
    }
    Ok(files)
}

#[cfg(feature = "subagent-tool")]
fn validate_bootstrap_filename(filename: &str) -> Result<(), harness_subagent::SubagentError> {
    let path = Path::new(filename);
    if filename.is_empty()
        || filename.contains('/')
        || filename.contains('\\')
        || path.is_absolute()
        || path.components().count() != 1
        || path.file_name() != Some(OsStr::new(filename))
    {
        return Err(harness_subagent::SubagentError::Engine(format!(
            "invalid bootstrap_filter filename: {filename}"
        )));
    }
    Ok(())
}

#[cfg(feature = "subagent-tool")]
fn escape_prompt_section_content(content: &str) -> String {
    content
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(feature = "subagent-tool")]
fn escape_prompt_source_attribute(source: &str) -> String {
    source
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(feature = "subagent-tool")]
pub(crate) fn wrap_workspace_instruction(filename: &str, content: &str) -> Option<String> {
    if content.trim().is_empty() {
        return None;
    }
    Some(format!(
        "<workspace-instructions source=\"{}\">\n{}\n</workspace-instructions>",
        escape_prompt_source_attribute(filename),
        escape_prompt_section_content(content)
    ))
}

#[cfg(feature = "subagent-tool")]
pub(crate) fn wrap_subagent_addendum(content: &str) -> Option<String> {
    if content.trim().is_empty() {
        return None;
    }
    Some(format!(
        "<subagent-addendum>\n{}\n</subagent-addendum>",
        escape_prompt_section_content(content)
    ))
}

#[cfg(feature = "subagent-tool")]
fn bootstrap_system_segment(files: &[(String, String)]) -> Option<String> {
    if files.is_empty() {
        return None;
    }
    let sections: Vec<_> = files
        .iter()
        .filter_map(|(filename, content)| wrap_workspace_instruction(filename, content))
        .collect();
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

#[cfg(feature = "subagent-tool")]
fn child_system_prompt(
    base: Option<&str>,
    bootstrap_segment: Option<&str>,
    extra: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if let Some(base) = base.filter(|base| !base.is_empty()) {
        parts.push(base.to_owned());
    }
    if let Some(bootstrap_segment) =
        bootstrap_segment.filter(|bootstrap_segment| !bootstrap_segment.is_empty())
    {
        parts.push(bootstrap_segment.to_owned());
    }
    if let Some(extra) = extra.filter(|extra| !extra.is_empty()) {
        if let Some(wrapped) = wrap_subagent_addendum(extra) {
            parts.push(wrapped);
        }
    }
    parts.join("\n\n")
}

#[cfg(feature = "subagent-tool")]
fn subagent_context_report(
    parent_system: Option<&str>,
    child_system: &str,
    bootstrap_files_inherited: Vec<String>,
    system_header_extra_applied: bool,
) -> SubagentContextReport {
    let parent_system_hash = parent_system.map(content_hash);
    let shared_system_prefix_hash = parent_system.and_then(|parent| {
        child_system
            .starts_with(parent)
            .then(|| content_hash(parent))
    });
    let prompt_cache_prefix_reused = parent_system
        .map(|parent| child_system.starts_with(parent))
        .unwrap_or(true);
    SubagentContextReport {
        parent_system_hash,
        child_system_hash: content_hash(child_system),
        shared_system_prefix_hash,
        prompt_cache_prefix_reused,
        bootstrap_files_inherited,
        system_header_extra_applied,
    }
}

#[cfg(feature = "subagent-tool")]
fn content_hash(value: &str) -> ContentHash {
    ContentHash(*blake3::hash(value.as_bytes()).as_bytes())
}

#[cfg(feature = "subagent-tool")]
fn subagent_budget_limits(spec: &harness_subagent::SubagentSpec) -> Option<crate::RunBudgetLimits> {
    let quota = spec.quota.as_ref()?;
    Some(crate::RunBudgetLimits {
        max_tokens: quota.max_tokens,
        max_tool_calls: quota.max_tool_calls,
        max_duration: quota.max_duration,
        max_cost_micros: quota
            .max_cost_cents
            .map(|cents| cents.saturating_mul(10_000)),
    })
}

#[cfg(feature = "subagent-tool")]
fn child_tool_filter(tools: &ToolPool, spec: &harness_subagent::SubagentSpec) -> ToolPoolFilter {
    let mut denylist = harness_subagent::DelegationBlocklist::default()
        .tools()
        .clone();
    denylist.extend(spec.tool_blocklist.iter().cloned());
    let allowlist = match &spec.toolset {
        harness_subagent::ToolsetSelector::Custom(tools) => {
            Some(tools.iter().cloned().collect::<HashSet<_>>())
        }
        harness_subagent::ToolsetSelector::InheritWithBlocklist(blocklist) => {
            denylist.extend(blocklist.iter().cloned());
            None
        }
        harness_subagent::ToolsetSelector::InheritAll
        | harness_subagent::ToolsetSelector::Preset(_) => None,
    };

    if spec.mcp_servers.is_empty() {
        for tool in tools.iter() {
            let descriptor = tool.descriptor();
            match &descriptor.origin {
                harness_contracts::ToolOrigin::Mcp(origin) => {
                    if !is_subagent_trusted_mcp_origin(origin) {
                        denylist.insert(descriptor.name.clone());
                    }
                }
                _ if descriptor.name.starts_with("mcp__") => {
                    denylist.insert(descriptor.name.clone());
                }
                _ => {}
            }
        }
    } else {
        let allowed_servers: HashSet<_> = spec
            .mcp_servers
            .iter()
            .map(|server| server.server_id().to_owned())
            .collect();
        let allowed_mcp_tool_names: HashSet<_> = tools
            .iter()
            .filter_map(|tool| match &tool.descriptor().origin {
                harness_contracts::ToolOrigin::Mcp(origin)
                    if allowed_servers.contains(origin.server_id.0.as_str()) =>
                {
                    Some(tool.descriptor().name.clone())
                }
                _ => None,
            })
            .collect();
        for tool in tools.iter() {
            let descriptor = tool.descriptor();
            match &descriptor.origin {
                harness_contracts::ToolOrigin::Mcp(origin) => {
                    if !is_subagent_trusted_mcp_origin(origin)
                        || !allowed_mcp_tool_names.contains(&descriptor.name)
                    {
                        denylist.insert(descriptor.name.clone());
                    }
                }
                _ if descriptor.name.starts_with("mcp__") => {
                    denylist.insert(descriptor.name.clone());
                }
                _ => {}
            }
        }
    }

    ToolPoolFilter {
        allowlist,
        denylist,
        mcp_included: true,
        plugin_included: true,
        group_allowlist: None,
        group_denylist: HashSet::new(),
    }
}

#[cfg(feature = "subagent-tool")]
fn child_run_outcome(
    events: &[Event],
    transcript_ref: Option<TranscriptRef>,
    context_report: Option<SubagentContextReport>,
) -> harness_subagent::ChildRunOutcome {
    let mut summary = None;
    let mut result = None;
    let mut assistant_usage = None;
    for event in events {
        if let Event::AssistantMessageCompleted(message) = event {
            let (content_summary, content_result) = announcement_content(&message.content);
            summary = Some(content_summary);
            result = Some(content_result);
            assistant_usage = Some(message.usage.clone());
        }
    }

    let mut run_usage = None;
    let status = events
        .iter()
        .rev()
        .find_map(|event| match event {
            Event::RunEnded(run) => {
                run_usage = run.usage.clone();
                Some(match &run.reason {
                    EndReason::Completed => harness_subagent::SubagentStatus::Completed,
                    EndReason::MaxIterationsReached => {
                        harness_subagent::SubagentStatus::MaxIterationsReached
                    }
                    EndReason::BudgetExhausted(kind) => {
                        harness_subagent::SubagentStatus::MaxBudget(kind.clone())
                    }
                    EndReason::TokenBudgetExhausted => {
                        harness_subagent::SubagentStatus::MaxBudget(BudgetKind::Tokens)
                    }
                    EndReason::Interrupted | EndReason::Cancelled { .. } => {
                        harness_subagent::SubagentStatus::Cancelled
                    }
                    _ => harness_subagent::SubagentStatus::Failed,
                })
            }
            _ => None,
        })
        .unwrap_or(harness_subagent::SubagentStatus::Failed);

    harness_subagent::ChildRunOutcome {
        status,
        summary: summary.unwrap_or_else(|| "subagent run completed".to_owned()),
        result,
        usage: run_usage
            .or(assistant_usage)
            .unwrap_or_else(UsageSnapshot::default),
        transcript_ref,
        context_report,
    }
}

#[cfg(feature = "subagent-tool")]
fn announcement_content(content: &MessageContent) -> (String, serde_json::Value) {
    match content {
        MessageContent::Text(text) => (
            text.clone(),
            serde_json::json!({
                "text": text
            }),
        ),
        MessageContent::Structured(value) => (
            value
                .get("summary")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
            value.clone(),
        ),
        MessageContent::Multimodal(parts) => {
            let value = serde_json::to_value(parts).unwrap_or(serde_json::Value::Null);
            (value.to_string(), value)
        }
    }
}

#[cfg(feature = "subagent-tool")]
fn required_capabilities_for_policy(
    policy: &SandboxPolicy,
) -> harness_subagent::RequiredSandboxCapabilities {
    harness_subagent::RequiredSandboxCapabilities {
        supports_network: !matches!(policy.network, NetworkAccess::Unrestricted),
        supports_filesystem_write: matches!(
            policy.scope,
            SandboxScope::WorkspaceOnly | SandboxScope::WorkspacePlus(_)
        ),
        ..Default::default()
    }
}

#[cfg(feature = "subagent-tool")]
struct PolicyOverrideSandbox {
    inner: Arc<dyn SandboxBackend>,
    policy: SandboxPolicy,
}

#[cfg(feature = "subagent-tool")]
impl PolicyOverrideSandbox {
    fn new(inner: Arc<dyn SandboxBackend>, policy: SandboxPolicy) -> Self {
        Self { inner, policy }
    }
}

#[cfg(feature = "subagent-tool")]
#[async_trait::async_trait]
impl SandboxBackend for PolicyOverrideSandbox {
    fn backend_id(&self) -> &str {
        self.inner.backend_id()
    }

    fn capabilities(&self) -> harness_sandbox::SandboxCapabilities {
        self.inner.capabilities()
    }

    fn base_config(&self) -> harness_sandbox::SandboxBaseConfig {
        self.inner.base_config()
    }

    async fn before_execute(
        &self,
        spec: &harness_sandbox::ExecSpec,
        ctx: &harness_sandbox::ExecContext,
    ) -> Result<(), harness_contracts::SandboxError> {
        self.inner.before_execute(spec, ctx).await
    }

    async fn execute(
        &self,
        mut spec: harness_sandbox::ExecSpec,
        ctx: harness_sandbox::ExecContext,
    ) -> Result<harness_sandbox::ProcessHandle, harness_contracts::SandboxError> {
        spec.policy = self.policy.clone();
        self.inner.execute(spec, ctx).await
    }

    async fn after_execute(
        &self,
        outcome: &harness_sandbox::ExecOutcome,
        ctx: &harness_sandbox::ExecContext,
    ) -> Result<(), harness_contracts::SandboxError> {
        self.inner.after_execute(outcome, ctx).await
    }

    async fn snapshot_session(
        &self,
        spec: &harness_sandbox::SnapshotSpec,
    ) -> Result<harness_sandbox::SessionSnapshotFile, harness_contracts::SandboxError> {
        self.inner.snapshot_session(spec).await
    }

    async fn restore_session(
        &self,
        snapshot: &harness_sandbox::SessionSnapshotFile,
    ) -> Result<(), harness_contracts::SandboxError> {
        self.inner.restore_session(snapshot).await
    }

    async fn shutdown(&self) -> Result<(), harness_contracts::SandboxError> {
        self.inner.shutdown().await
    }
}

#[cfg(feature = "subagent-tool")]
fn requested_mcp_trust_violations(
    tools: &ToolPool,
    spec: &harness_subagent::SubagentSpec,
) -> Vec<String> {
    let requested_servers = spec
        .mcp_servers
        .iter()
        .chain(spec.required_mcp_servers.iter())
        .map(|server| server.server_id().to_owned())
        .collect::<HashSet<_>>();
    if requested_servers.is_empty() {
        return Vec::new();
    }

    let mut violations = HashSet::new();
    for tool in tools.iter() {
        if let harness_contracts::ToolOrigin::Mcp(origin) = &tool.descriptor().origin {
            if requested_servers.contains(origin.server_id.0.as_str()) {
                if let Some(reason) = subagent_mcp_trust_violation_reason(origin) {
                    violations.insert(reason);
                }
            }
        }
    }
    let mut violations = violations.into_iter().collect::<Vec<_>>();
    violations.sort();
    violations
}

#[cfg(feature = "subagent-tool")]
fn is_subagent_trusted_mcp_origin(origin: &harness_contracts::McpOrigin) -> bool {
    subagent_mcp_trust_violation_reason(origin).is_none()
}

#[cfg(feature = "subagent-tool")]
fn subagent_mcp_trust_violation_reason(origin: &harness_contracts::McpOrigin) -> Option<String> {
    if origin.server_trust != harness_contracts::TrustLevel::AdminTrusted {
        return Some(format!(
            "{} user-controlled MCP server is disallowed for subagents",
            origin.server_id.0
        ));
    }
    if !matches!(
        origin.server_source,
        harness_contracts::McpServerSource::Workspace
            | harness_contracts::McpServerSource::Policy
            | harness_contracts::McpServerSource::Managed { .. }
    ) {
        return Some(format!(
            "{} MCP server source {:?} is disallowed for subagents",
            origin.server_id.0, origin.server_source
        ));
    }
    None
}

#[cfg(feature = "subagent-tool")]
async fn missing_required_mcp_servers(
    tools: &ToolPool,
    registry: Option<&McpRegistry>,
    spec: &harness_subagent::SubagentSpec,
) -> Vec<String> {
    if let Some(registry) = registry {
        let refs = spec
            .mcp_servers
            .iter()
            .map(|server| {
                McpServerRef::Shared(harness_contracts::McpServerId(
                    server.server_id().to_owned(),
                ))
            })
            .collect::<Vec<_>>();
        let required = spec
            .required_mcp_servers
            .iter()
            .map(|server| {
                McpServerPattern::exact(harness_contracts::McpServerId(
                    server.server_id().to_owned(),
                ))
            })
            .collect::<Vec<_>>();
        return registry
            .evaluate_required(&refs, &required)
            .await
            .into_iter()
            .filter_map(required_evaluation_reason)
            .collect();
    }

    let available: HashSet<_> = tools
        .iter()
        .filter_map(|tool| match &tool.descriptor().origin {
            harness_contracts::ToolOrigin::Mcp(origin) => Some(origin.server_id.0.clone()),
            _ => None,
        })
        .collect();
    spec.required_mcp_servers
        .iter()
        .filter(|server| !available.contains(server.server_id()))
        .map(|server| server.server_id().to_owned())
        .collect()
}

#[cfg(feature = "subagent-tool")]
fn required_evaluation_reason(evaluation: RequiredEvaluation) -> Option<String> {
    match evaluation {
        RequiredEvaluation::Satisfied => None,
        RequiredEvaluation::Missing { pattern } => Some(pattern),
        RequiredEvaluation::NotReady { server_id, state } => {
            Some(format!("{} not ready: {state:?}", server_id.0))
        }
        RequiredEvaluation::InlineDisallowed { pattern, server_id } => Some(format!(
            "{pattern} inline MCP server {} is disallowed",
            server_id.0
        )),
        _ => Some("unsupported required MCP evaluation".to_owned()),
    }
}

#[cfg(all(test, feature = "subagent-tool"))]
mod subagent_tool_tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use harness_contracts::{
        AssistantMessageCompletedEvent, BudgetKind, DeferPolicy, EndReason, Event, McpOrigin,
        McpServerId, McpServerSource, MessageContent, MessageId, NetworkAccess,
        ProviderRestriction, ResultBudget, StopReason, ToolActionPlan, ToolDescriptor, ToolError,
        ToolGroup, ToolOrigin, ToolProperties, ToolResult, TrustLevel, UsageSnapshot,
        WorkspaceAccess,
    };
    use harness_mcp::{
        ListChangedEvent, McpConnection, McpConnectionState, McpError, McpRegistry, McpServerScope,
        McpServerSpec, McpToolDescriptor, McpToolResult, TransportChoice,
    };
    use harness_permission::PermissionCheck;
    use harness_subagent::{SubagentStatus, ToolsetSelector};
    use harness_tool::{
        action_plan_from_permission_check, AuthorizedToolInput, Tool, ToolContext, ToolEvent,
        ToolPool, ToolStream, ValidationError,
    };

    use super::{
        announcement_content, child_run_outcome, child_system_prompt, child_tool_filter,
        missing_required_mcp_servers, wrap_subagent_addendum, wrap_workspace_instruction,
    };

    #[test]
    fn custom_toolset_mcp_servers_do_not_expand_allowlist() {
        let mut tools = ToolPool::default();
        tools.append_runtime_tool(Arc::new(TestTool::new("explicit", ToolOrigin::Builtin)));
        tools.append_runtime_tool(Arc::new(TestTool::new("mcp_extra", mcp_origin("srv-a"))));
        tools.append_runtime_tool(Arc::new(TestTool::new("mcp_other", mcp_origin("srv-b"))));

        let mut spec = harness_subagent::SubagentSpec::minimal("worker", "task");
        spec.toolset = ToolsetSelector::Custom(vec!["explicit".to_owned()]);
        spec.mcp_servers = vec!["srv-a".into()];

        let filtered = tools.filtered(&child_tool_filter(&tools, &spec));

        assert!(filtered.get("explicit").is_some());
        assert!(filtered.get("mcp_extra").is_none());
        assert!(filtered.get("mcp_other").is_none());
    }

    #[test]
    fn custom_toolset_allows_explicit_mcp_tool_from_allowed_server() {
        let mut tools = ToolPool::default();
        tools.append_runtime_tool(Arc::new(TestTool::new("mcp_allowed", mcp_origin("srv-a"))));
        tools.append_runtime_tool(Arc::new(TestTool::new("mcp_blocked", mcp_origin("srv-b"))));

        let mut spec = harness_subagent::SubagentSpec::minimal("worker", "task");
        spec.toolset = ToolsetSelector::Custom(vec!["mcp_allowed".to_owned()]);
        spec.mcp_servers = vec!["srv-a".into()];

        let filtered = tools.filtered(&child_tool_filter(&tools, &spec));

        assert!(filtered.get("mcp_allowed").is_some());
        assert!(filtered.get("mcp_blocked").is_none());
    }

    #[test]
    fn child_tool_filter_rejects_forged_mcp_builtin_name() {
        let mut tools = ToolPool::default();
        tools.append_runtime_tool(Arc::new(TestTool::new(
            "mcp__srv_a__forged",
            ToolOrigin::Builtin,
        )));
        tools.append_runtime_tool(Arc::new(TestTool::new("file_read", ToolOrigin::Builtin)));

        let mut spec = harness_subagent::SubagentSpec::minimal("worker", "task");
        spec.toolset = ToolsetSelector::Custom(vec![
            "mcp__srv_a__forged".to_owned(),
            "file_read".to_owned(),
        ]);
        spec.mcp_servers = vec!["srv-a".into()];

        let filtered = tools.filtered(&child_tool_filter(&tools, &spec));

        assert!(filtered.get("mcp__srv_a__forged").is_none());
        assert!(filtered.get("file_read").is_some());
    }

    #[tokio::test]
    async fn missing_required_mcp_server_is_reported() {
        let mut tools = ToolPool::default();
        tools.append_runtime_tool(Arc::new(TestTool::new("mcp_allowed", mcp_origin("srv-a"))));

        let mut spec = harness_subagent::SubagentSpec::minimal("worker", "task");
        spec.required_mcp_servers = vec!["srv-missing".into()];

        assert_eq!(
            missing_required_mcp_servers(&tools, None, &spec).await,
            vec!["srv-missing".to_owned()]
        );
    }

    #[tokio::test]
    async fn required_mcp_server_uses_registry_state_when_available() {
        let mut tools = ToolPool::default();
        tools.append_runtime_tool(Arc::new(TestTool::new("mcp_allowed", mcp_origin("srv-a"))));
        let registry = McpRegistry::new();
        registry
            .add_ready_server(
                mcp_spec("srv-a"),
                McpServerScope::Global,
                Arc::new(NoopMcpConnection),
            )
            .await
            .expect("server");
        registry
            .set_connection_state(
                &McpServerId("srv-a".into()),
                McpConnectionState::Reconnecting {
                    attempt: 1,
                    last_error: "transport reset".to_owned(),
                },
            )
            .await
            .expect("state");

        let mut spec = harness_subagent::SubagentSpec::minimal("worker", "task");
        spec.required_mcp_servers = vec!["srv-a".into()];

        assert_eq!(
            missing_required_mcp_servers(&tools, Some(&registry), &spec).await,
            vec![
                "srv-a not ready: Reconnecting { attempt: 1, last_error: \"transport reset\" }"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn child_outcome_uses_last_assistant_output_and_run_usage() {
        let run_id = harness_contracts::RunId::new();
        let assistant_usage = UsageSnapshot {
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_micros: 3,
            tool_calls: 0,
        };
        let run_usage = UsageSnapshot {
            input_tokens: 4,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_micros: 6,
            tool_calls: 0,
        };
        let events = vec![
            Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
                run_id,
                message_id: MessageId::new(),
                content: MessageContent::Text("child answer".to_owned()),
                tool_uses: Vec::new(),
                usage: assistant_usage,
                pricing_snapshot_id: None,
                stop_reason: StopReason::EndTurn,
                at: harness_contracts::now(),
            }),
            Event::RunEnded(harness_contracts::RunEndedEvent {
                run_id,
                reason: EndReason::Completed,
                usage: Some(run_usage.clone()),
                ended_at: harness_contracts::now(),
            }),
        ];

        let outcome = child_run_outcome(&events, None, None);

        assert_eq!(outcome.status, SubagentStatus::Completed);
        assert_eq!(outcome.summary, "child answer");
        assert_eq!(
            outcome.result,
            Some(serde_json::json!({ "text": "child answer" }))
        );
        assert_eq!(outcome.usage, run_usage);
    }

    #[test]
    fn structured_child_output_uses_summary_field() {
        let content = MessageContent::Structured(serde_json::json!({
            "summary": "structured summary",
            "value": 42
        }));

        let (summary, result) = announcement_content(&content);

        assert_eq!(summary, "structured summary");
        assert_eq!(
            result,
            serde_json::json!({
                "summary": "structured summary",
                "value": 42
            })
        );
    }

    #[test]
    fn child_outcome_maps_max_iterations_status() {
        let events = vec![Event::RunEnded(harness_contracts::RunEndedEvent {
            run_id: harness_contracts::RunId::new(),
            reason: EndReason::MaxIterationsReached,
            usage: Some(UsageSnapshot::default()),
            ended_at: harness_contracts::now(),
        })];

        let outcome = child_run_outcome(&events, None, None);

        assert_eq!(outcome.status, SubagentStatus::MaxIterationsReached);
    }

    #[test]
    fn child_outcome_maps_budget_status() {
        let events = vec![Event::RunEnded(harness_contracts::RunEndedEvent {
            run_id: harness_contracts::RunId::new(),
            reason: EndReason::BudgetExhausted(BudgetKind::Tokens),
            usage: Some(UsageSnapshot::default()),
            ended_at: harness_contracts::now(),
        })];

        let outcome = child_run_outcome(&events, None, None);

        assert_eq!(
            outcome.status,
            SubagentStatus::MaxBudget(BudgetKind::Tokens)
        );
    }

    #[test]
    fn wrap_workspace_instruction_renders_source_wrapped_bootstrap() {
        let rendered = wrap_workspace_instruction("AGENTS.md", "Root workspace rule.")
            .expect("non-empty content renders");
        assert!(rendered.contains(r#"<workspace-instructions source="AGENTS.md">"#));
        assert!(rendered.contains("Root workspace rule."));
        assert!(!rendered.contains("AI 编程伙伴"));
    }

    #[test]
    fn wrap_workspace_instruction_escapes_xml_and_quotes_in_source() {
        let rendered = wrap_workspace_instruction("AGENTS\"<>.md", "rule & value <tag>")
            .expect("content renders");
        assert!(rendered.contains(r#"source="AGENTS&quot;&lt;&gt;.md""#));
        assert!(rendered.contains("rule &amp; value &lt;tag&gt;"));
    }

    #[test]
    fn wrap_subagent_addendum_renders_bounded_child_section() {
        let rendered = wrap_subagent_addendum("Child-only constraint.").unwrap();
        assert_eq!(
            rendered,
            "<subagent-addendum>\nChild-only constraint.\n</subagent-addendum>"
        );
    }

    #[test]
    fn child_system_prompt_preserves_parent_prefix_for_cache_reuse() {
        let child = child_system_prompt(
            Some("parent-system"),
            Some(
                r#"<workspace-instructions source="AGENTS.md">
agent rules
</workspace-instructions>"#,
            ),
            Some("child-only-system"),
        );
        assert_eq!(&child.as_bytes()[.."parent-system".len()], b"parent-system");
        assert!(child.contains("<subagent-addendum>"));
        assert!(child.contains("child-only-system"));
        assert!(child.contains("<workspace-instructions"));
    }

    #[test]
    fn empty_addendum_and_bootstrap_helpers_return_none() {
        assert!(wrap_workspace_instruction("AGENTS.md", "   ").is_none());
        assert!(wrap_subagent_addendum("   ").is_none());
    }

    #[tokio::test]
    async fn child_cancellation_bridge_aborts_on_drop() {
        let parent_cancellation = harness_subagent::SubagentCancellationToken::new();
        let engine_cancellation = crate::CancellationToken::new();
        let bridge = super::ChildCancellationBridge::spawn(
            parent_cancellation.clone(),
            engine_cancellation.clone(),
        );

        drop(bridge);
        parent_cancellation.cancel();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        assert!(!engine_cancellation.is_cancelled());
    }

    fn mcp_origin(server_id: &str) -> ToolOrigin {
        ToolOrigin::Mcp(McpOrigin {
            server_id: McpServerId(server_id.to_owned()),
            upstream_name: "upstream".to_owned(),
            server_meta: Default::default(),
            server_source: McpServerSource::Workspace,
            server_trust: TrustLevel::AdminTrusted,
        })
    }

    fn mcp_spec(server_id: &str) -> McpServerSpec {
        McpServerSpec::new(
            McpServerId(server_id.to_owned()),
            server_id,
            TransportChoice::InProcess,
            McpServerSource::Workspace,
        )
    }

    struct NoopMcpConnection;

    #[async_trait]
    impl McpConnection for NoopMcpConnection {
        fn connection_id(&self) -> &str {
            "noop"
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

        async fn subscribe_changes(&self) -> Result<ListChangedEvent, McpError> {
            Ok(Box::pin(futures::stream::empty()))
        }

        async fn shutdown(&self) -> Result<(), McpError> {
            Ok(())
        }
    }

    struct TestTool {
        descriptor: ToolDescriptor,
    }

    impl TestTool {
        fn new(name: &str, origin: ToolOrigin) -> Self {
            Self {
                descriptor: ToolDescriptor {
                    name: name.to_owned(),
                    display_name: name.to_owned(),
                    description: "test tool".to_owned(),
                    category: "test".to_owned(),
                    group: ToolGroup::Custom("test".to_owned()),
                    version: "0.0.0".to_owned(),
                    input_schema: serde_json::json!({ "type": "object" }),
                    output_schema: None,
                    dynamic_schema: false,
                    properties: ToolProperties {
                        is_concurrency_safe: true,
                        is_read_only: true,
                        is_destructive: false,
                        long_running: None,
                        defer_policy: DeferPolicy::AlwaysLoad,
                    },
                    trust_level: TrustLevel::UserControlled,
                    required_capabilities: Vec::new(),
                    budget: ResultBudget {
                        metric: harness_contracts::BudgetMetric::Chars,
                        limit: 1024,
                        on_overflow: harness_contracts::OverflowAction::Truncate,
                        preview_head_chars: 128,
                        preview_tail_chars: 128,
                    },
                    provider_restriction: ProviderRestriction::All,
                    origin,
                    search_hint: None,
                    service_binding: None,
                },
            }
        }
    }

    #[async_trait]
    impl Tool for TestTool {
        fn descriptor(&self) -> &ToolDescriptor {
            &self.descriptor
        }

        async fn validate(
            &self,
            _input: &serde_json::Value,
            _ctx: &ToolContext,
        ) -> Result<(), ValidationError> {
            Ok(())
        }

        async fn plan(
            &self,
            input: &serde_json::Value,
            ctx: &ToolContext,
        ) -> Result<ToolActionPlan, ToolError> {
            action_plan_from_permission_check(
                self.descriptor(),
                input,
                ctx,
                PermissionCheck::Allowed,
                Vec::new(),
                WorkspaceAccess::None,
                NetworkAccess::None,
            )
        }

        async fn execute_authorized(
            &self,
            _authorized: AuthorizedToolInput,
            _ctx: ToolContext,
        ) -> Result<ToolStream, ToolError> {
            Ok(Box::pin(futures::stream::iter([ToolEvent::Final(
                ToolResult::Text("ok".to_owned()),
            )])))
        }
    }
}

#[cfg(feature = "subagent-tool")]
fn interactivity_level(
    level: harness_subagent::InteractivityLevel,
) -> harness_contracts::InteractivityLevel {
    match level {
        harness_subagent::InteractivityLevel::FullyInteractive => {
            harness_contracts::InteractivityLevel::FullyInteractive
        }
        harness_subagent::InteractivityLevel::DeferredInteractive => {
            harness_contracts::InteractivityLevel::DeferredInteractive
        }
        harness_subagent::InteractivityLevel::NoInteractive => {
            harness_contracts::InteractivityLevel::NoInteractive
        }
    }
}

#[cfg(feature = "steering")]
#[async_trait::async_trait]
impl SteeringDrain for harness_session::Session {
    async fn drain_and_merge(
        &self,
        _session: &SessionHandle,
        run_id: RunId,
        merged_into_message_id: MessageId,
    ) -> Result<Option<SteeringMerge>, EngineError> {
        Ok(self
            .drain_and_merge_into(run_id, Some(merged_into_message_id))
            .await
            .map_err(|error| EngineError::Message(error.to_string()))?
            .map(|message| SteeringMerge {
                body: message.body,
                applied_event: message.applied_event,
                already_persisted: true,
            }))
    }
}

fn validate_tool_capabilities(
    tools: &ToolPool,
    cap_registry: &CapabilityRegistry,
) -> Result<(), harness_contracts::EngineError> {
    for tool in tools.iter() {
        let descriptor = tool.descriptor();
        for capability in &descriptor.required_capabilities {
            if !cap_registry.contains(capability) {
                return Err(harness_contracts::EngineError::Message(format!(
                    "missing required capability {capability} for tool {}",
                    descriptor.name
                )));
            }
        }
    }

    Ok(())
}

#[async_trait::async_trait]
impl EngineRunner for Engine {
    async fn run(
        &self,
        session: SessionHandle,
        input: harness_contracts::TurnInput,
        ctx: RunContext,
    ) -> Result<EventStream, harness_contracts::EngineError> {
        crate::turn::run_turn(self, session, input, ctx).await
    }

    fn engine_id(&self) -> EngineId {
        self.engine_id()
    }
}

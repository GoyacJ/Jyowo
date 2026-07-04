use std::path::PathBuf;
use std::sync::Arc;

#[cfg(feature = "agents-subagent")]
use harness_contracts::SubagentRunnerCap;
use harness_contracts::{
    BlobStore, CapabilityRegistry, HarnessError, ProviderCapabilityRouteSettings, ToolCapability,
};
use harness_hook::HookRegistry;
use harness_journal::{EventStore, EvidenceRefStore};
use harness_mcp::{ElicitationHandler, StreamElicitationHandler};
#[cfg(feature = "memory-consolidation")]
use harness_memory::ConsolidationHook;
use harness_memory::MemoryProvider;
use harness_model::{AuxModelProvider, InferMiddleware, ModelProvider};
use harness_observability::{Observer, Tracer};
#[cfg(feature = "stream-permission")]
use harness_permission::ResolverHandle;
use harness_permission::{DecisionStore, PermissionAuthority, PermissionBroker, RuleProvider};
use harness_plugin::PluginRegistry;
use harness_provider_state::ProviderContinuationStore;
use harness_sandbox::SandboxBackend;
use harness_skill::SkillLoader;
use harness_tool::ToolRegistry;

use crate::skill_config::SkillConfigSnapshot;
use crate::{Harness, HarnessOptions, McpConfig, TenantPolicy};

#[derive(Debug, Clone, Copy, Default)]
pub struct Unset;

#[derive(Debug, Clone)]
pub struct Set<T>(pub T);

#[derive(Default)]
pub(crate) struct BuilderExtras {
    pub(crate) permission_broker: Option<Arc<dyn PermissionBroker>>,
    #[cfg(feature = "stream-permission")]
    pub(crate) permission_resolver: Option<ResolverHandle>,
    pub(crate) tool_registry: Option<ToolRegistry>,
    pub(crate) hook_registry: Option<HookRegistry>,
    pub(crate) memory_provider: Option<Arc<dyn MemoryProvider>>,
    #[cfg(feature = "memory-consolidation")]
    pub(crate) consolidation_hook: Option<Arc<dyn ConsolidationHook>>,
    #[cfg(feature = "memory-builtin")]
    pub(crate) builtin_memory: Option<BuiltinMemoryConfig>,
    pub(crate) blob_store: Option<Arc<dyn BlobStore>>,
    pub(crate) evidence_ref_store: Option<Arc<EvidenceRefStore>>,
    pub(crate) skill_loader: Option<SkillLoader>,
    pub(crate) skill_config_snapshot: Option<SkillConfigSnapshot>,
    pub(crate) mcp_config: Option<McpConfig>,
    pub(crate) elicitation_handler: Option<Arc<dyn ElicitationHandler>>,
    pub(crate) stream_elicitation_handler: Option<StreamElicitationHandler>,
    pub(crate) plugin_registry: Option<PluginRegistry>,
    pub(crate) tracer: Option<Arc<dyn Tracer>>,
    pub(crate) observer: Option<Arc<Observer>>,
    pub(crate) aux_model: Option<Arc<dyn AuxModelProvider>>,
    pub(crate) model_middlewares: Vec<Arc<dyn InferMiddleware>>,
    pub(crate) rule_providers: Vec<Arc<dyn RuleProvider>>,
    pub(crate) decision_store: Option<Arc<dyn DecisionStore>>,
    pub(crate) permission_authority: Option<Arc<PermissionAuthority>>,
    pub(crate) authorization_service: Option<Arc<harness_execution::AuthorizationService>>,
    pub(crate) cap_registry: Option<CapabilityRegistry>,
    pub(crate) provider_capability_routes:
        Option<Arc<parking_lot::RwLock<ProviderCapabilityRouteSettings>>>,
    pub(crate) provider_continuation_store: Option<Arc<dyn ProviderContinuationStore>>,
    #[cfg(feature = "tool-search")]
    pub(crate) tool_search_scorer: Option<Arc<dyn harness_tool_search::ToolSearchScorer>>,
}

#[cfg(feature = "memory-builtin")]
#[derive(Clone)]
pub(crate) enum BuiltinMemoryConfig {
    Fixed(harness_memory::BuiltinMemory),
    Root(PathBuf),
}

#[cfg(feature = "memory-builtin")]
impl BuiltinMemoryConfig {
    pub(crate) fn for_session(
        &self,
        options: &harness_session::SessionOptions,
    ) -> harness_memory::BuiltinMemory {
        match self {
            Self::Fixed(memory) => memory.clone(),
            Self::Root(root) => harness_memory::BuiltinMemory::at(root, options.tenant_id),
        }
    }
}

pub struct HarnessBuilder<ModelState = Unset, StoreState = Unset, SandboxState = Unset> {
    pub(crate) model: ModelState,
    pub(crate) store: StoreState,
    pub(crate) sandbox: SandboxState,
    pub(crate) options: HarnessOptions,
    pub(crate) model_id_explicit: bool,
    pub(crate) extras: BuilderExtras,
}

impl HarnessBuilder<Unset, Unset, Unset> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            model: Unset,
            store: Unset,
            sandbox: Unset,
            options: HarnessOptions::default(),
            model_id_explicit: false,
            extras: BuilderExtras::default(),
        }
    }
}

impl Default for HarnessBuilder<Unset, Unset, Unset> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M, S, SB> HarnessBuilder<M, S, SB> {
    #[must_use]
    pub fn with_options(mut self, options: HarnessOptions) -> Self {
        self.options = options;
        self.model_id_explicit = true;
        self
    }

    #[must_use]
    pub fn with_workspace_root(mut self, workspace_root: impl Into<PathBuf>) -> Self {
        let workspace_root = workspace_root.into();
        self.options.default_session_options.workspace_root = workspace_root.clone();
        self.options.workspace_root = workspace_root;
        self
    }

    #[must_use]
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.options.model_id = model_id.into();
        self.model_id_explicit = true;
        self
    }

    #[must_use]
    pub fn with_tenant_policy(mut self, policy: TenantPolicy) -> Self {
        self.options.tenant_policy = policy;
        self
    }

    #[must_use]
    pub fn with_default_session_options(
        mut self,
        options: harness_session::SessionOptions,
    ) -> Self {
        self.options.default_session_options = options;
        self
    }

    #[must_use]
    pub fn disable_tool_search(mut self) -> Self {
        self.options.tool_search_enabled = false;
        self
    }

    #[cfg(feature = "tool-search")]
    #[must_use]
    pub fn with_tool_search_scorer<T>(self, scorer: T) -> Self
    where
        T: harness_tool_search::ToolSearchScorer,
    {
        self.with_tool_search_scorer_arc(Arc::new(scorer))
    }

    #[cfg(feature = "tool-search")]
    #[must_use]
    pub fn with_tool_search_scorer_arc(
        mut self,
        scorer: Arc<dyn harness_tool_search::ToolSearchScorer>,
    ) -> Self {
        self.extras.tool_search_scorer = Some(scorer);
        self
    }

    #[must_use]
    pub fn with_permission_broker<B>(mut self, broker: B) -> Self
    where
        B: PermissionBroker,
    {
        self.extras.permission_broker = Some(Arc::new(broker));
        #[cfg(feature = "stream-permission")]
        {
            self.extras.permission_resolver = None;
        }
        self
    }

    #[must_use]
    pub fn with_permission_broker_arc(mut self, broker: Arc<dyn PermissionBroker>) -> Self {
        self.extras.permission_broker = Some(broker);
        #[cfg(feature = "stream-permission")]
        {
            self.extras.permission_resolver = None;
        }
        self
    }

    #[cfg(feature = "stream-permission")]
    #[must_use]
    pub fn with_stream_permission_broker<B>(mut self, broker: B, resolver: ResolverHandle) -> Self
    where
        B: PermissionBroker,
    {
        self.extras.permission_broker = Some(Arc::new(broker));
        self.extras.permission_resolver = Some(resolver);
        self
    }

    #[cfg(feature = "stream-permission")]
    #[must_use]
    pub fn with_stream_permission_broker_arc(
        mut self,
        broker: Arc<dyn PermissionBroker>,
        resolver: ResolverHandle,
    ) -> Self {
        self.extras.permission_broker = Some(broker);
        self.extras.permission_resolver = Some(resolver);
        self
    }

    #[must_use]
    pub fn with_tool_registry(mut self, registry: ToolRegistry) -> Self {
        self.extras.tool_registry = Some(registry);
        self
    }

    #[must_use]
    pub fn with_hook_registry(mut self, registry: HookRegistry) -> Self {
        self.extras.hook_registry = Some(registry);
        self
    }

    #[must_use]
    pub fn with_memory_provider<P>(self, provider: P) -> Self
    where
        P: MemoryProvider,
    {
        self.with_memory_provider_arc(Arc::new(provider))
    }

    #[must_use]
    pub fn with_memory_provider_arc(mut self, provider: Arc<dyn MemoryProvider>) -> Self {
        self.extras.memory_provider = Some(provider);
        self
    }

    #[cfg(feature = "memory-consolidation")]
    #[must_use]
    pub fn with_memory_consolidation_hook<H>(self, hook: H) -> Self
    where
        H: ConsolidationHook,
    {
        self.with_memory_consolidation_hook_arc(Arc::new(hook))
    }

    #[cfg(feature = "memory-consolidation")]
    #[must_use]
    pub fn with_memory_consolidation_hook_arc(mut self, hook: Arc<dyn ConsolidationHook>) -> Self {
        self.extras.consolidation_hook = Some(hook);
        self
    }

    #[cfg(feature = "memory-builtin")]
    #[must_use]
    pub fn with_builtin_memory(mut self, memory: harness_memory::BuiltinMemory) -> Self {
        self.extras.builtin_memory = Some(BuiltinMemoryConfig::Fixed(memory));
        self
    }

    #[cfg(feature = "memory-builtin")]
    #[must_use]
    pub fn with_builtin_memory_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.extras.builtin_memory = Some(BuiltinMemoryConfig::Root(root.into()));
        self
    }

    #[must_use]
    pub fn with_blob_store<B>(self, store: B) -> Self
    where
        B: BlobStore,
    {
        self.with_blob_store_arc(Arc::new(store))
    }

    #[must_use]
    pub fn with_blob_store_arc(mut self, store: Arc<dyn BlobStore>) -> Self {
        self.extras.blob_store = Some(store);
        self
    }

    #[must_use]
    pub fn with_evidence_ref_store_arc(mut self, store: Arc<EvidenceRefStore>) -> Self {
        self.extras.evidence_ref_store = Some(store);
        self
    }

    #[must_use]
    pub fn with_capability<T>(mut self, capability: ToolCapability, implementation: Arc<T>) -> Self
    where
        T: ?Sized + Send + Sync + 'static,
    {
        let mut registry = self.extras.cap_registry.take().unwrap_or_default();
        registry.install::<T>(capability, implementation);
        self.extras.cap_registry = Some(registry);
        self
    }

    #[must_use]
    pub fn with_skill_loader(mut self, loader: SkillLoader) -> Self {
        self.extras.skill_loader = Some(loader);
        self
    }

    #[must_use]
    pub fn with_skill_config_snapshot(mut self, snapshot: SkillConfigSnapshot) -> Self {
        self.extras.skill_config_snapshot = Some(snapshot);
        self
    }

    #[must_use]
    pub fn with_mcp_config(mut self, config: McpConfig) -> Self {
        self.extras.mcp_config = Some(config);
        self
    }

    #[must_use]
    pub fn with_elicitation_handler<H>(self, handler: H) -> Self
    where
        H: ElicitationHandler,
    {
        self.with_elicitation_handler_arc(Arc::new(handler))
    }

    #[must_use]
    pub fn with_elicitation_handler_arc(mut self, handler: Arc<dyn ElicitationHandler>) -> Self {
        self.extras.elicitation_handler = Some(handler);
        self
    }

    #[must_use]
    pub fn with_stream_elicitation_handler(mut self, handler: StreamElicitationHandler) -> Self {
        self.extras.stream_elicitation_handler = Some(handler.clone());
        self.extras.elicitation_handler = Some(Arc::new(handler));
        self
    }

    #[must_use]
    pub fn with_plugin_registry(mut self, registry: PluginRegistry) -> Self {
        self.extras.plugin_registry = Some(registry);
        self
    }

    #[must_use]
    pub fn with_observability(mut self, tracer: Arc<dyn Tracer>) -> Self {
        self.extras.tracer = Some(tracer);
        self
    }

    #[must_use]
    pub fn with_observer(mut self, observer: Arc<Observer>) -> Self {
        self.extras.observer = Some(observer);
        self
    }

    #[must_use]
    pub fn with_aux_model<P>(self, provider: P) -> Self
    where
        P: AuxModelProvider,
    {
        self.with_aux_model_arc(Arc::new(provider))
    }

    #[must_use]
    pub fn with_aux_model_arc(mut self, provider: Arc<dyn AuxModelProvider>) -> Self {
        self.extras.aux_model = Some(provider);
        self
    }

    #[must_use]
    pub fn with_model_middleware(mut self, middleware: Arc<dyn InferMiddleware>) -> Self {
        self.extras.model_middlewares.push(middleware);
        self
    }

    #[must_use]
    pub fn with_model_middlewares<I>(mut self, middlewares: I) -> Self
    where
        I: IntoIterator<Item = Arc<dyn InferMiddleware>>,
    {
        self.extras.model_middlewares.extend(middlewares);
        self
    }

    #[must_use]
    pub fn with_rule_provider(mut self, provider: Arc<dyn RuleProvider>) -> Self {
        self.extras.rule_providers.push(provider);
        self
    }

    #[must_use]
    pub fn with_shared_provider_capability_routes(
        mut self,
        routes: Arc<parking_lot::RwLock<ProviderCapabilityRouteSettings>>,
    ) -> Self {
        self.extras.provider_capability_routes = Some(routes);
        self
    }

    #[must_use]
    pub fn with_provider_capability_routes(self, routes: ProviderCapabilityRouteSettings) -> Self {
        self.with_shared_provider_capability_routes(Arc::new(parking_lot::RwLock::new(routes)))
    }

    #[must_use]
    pub fn with_provider_continuation_store<PCS>(self, store: PCS) -> Self
    where
        PCS: ProviderContinuationStore,
    {
        self.with_provider_continuation_store_arc(Arc::new(store))
    }

    #[must_use]
    pub fn with_provider_continuation_store_arc(
        mut self,
        store: Arc<dyn ProviderContinuationStore>,
    ) -> Self {
        self.extras.provider_continuation_store = Some(store);
        self
    }

    #[must_use]
    pub fn with_decision_persistence(mut self, persistence: Arc<dyn DecisionStore>) -> Self {
        self.extras.decision_store = Some(persistence);
        self
    }

    #[must_use]
    pub fn with_permission_authority(mut self, authority: PermissionAuthority) -> Self {
        self.extras.permission_authority = Some(Arc::new(authority));
        self
    }

    #[must_use]
    pub fn with_permission_authority_arc(mut self, authority: Arc<PermissionAuthority>) -> Self {
        self.extras.permission_authority = Some(authority);
        self
    }

    #[must_use]
    pub fn with_authorization_service(
        mut self,
        service: harness_execution::AuthorizationService,
    ) -> Self {
        self.extras.authorization_service = Some(Arc::new(service));
        self
    }

    #[must_use]
    pub fn with_authorization_service_arc(
        mut self,
        service: Arc<harness_execution::AuthorizationService>,
    ) -> Self {
        self.extras.authorization_service = Some(service);
        self
    }

    #[cfg(feature = "agents-subagent")]
    #[must_use]
    pub fn with_subagent_runner(
        mut self,
        runner: Arc<dyn harness_subagent::SubagentRunner>,
    ) -> Self {
        let mut registry = self.extras.cap_registry.take().unwrap_or_default();
        let runner_cap = harness_subagent::SubagentRunnerCapAdapter::from_runner(runner);
        registry.install::<dyn SubagentRunnerCap>(ToolCapability::SubagentRunner, runner_cap);
        self.extras.cap_registry = Some(registry);
        self
    }
}

impl<S, SB> HarnessBuilder<Unset, S, SB> {
    #[must_use]
    pub fn with_model<M>(self, model: M) -> HarnessBuilder<Set<Arc<dyn ModelProvider>>, S, SB>
    where
        M: ModelProvider,
    {
        self.with_model_arc(Arc::new(model))
    }

    #[must_use]
    pub fn with_model_arc(
        self,
        model: Arc<dyn ModelProvider>,
    ) -> HarnessBuilder<Set<Arc<dyn ModelProvider>>, S, SB> {
        HarnessBuilder {
            model: Set(model),
            store: self.store,
            sandbox: self.sandbox,
            options: self.options,
            model_id_explicit: self.model_id_explicit,
            extras: self.extras,
        }
    }
}

impl<S, SB> HarnessBuilder<Set<Arc<dyn ModelProvider>>, S, SB> {
    #[must_use]
    pub fn with_model<M>(self, model: M) -> HarnessBuilder<Set<Arc<dyn ModelProvider>>, S, SB>
    where
        M: ModelProvider,
    {
        self.with_model_arc(Arc::new(model))
    }

    #[must_use]
    pub fn with_model_arc(
        self,
        model: Arc<dyn ModelProvider>,
    ) -> HarnessBuilder<Set<Arc<dyn ModelProvider>>, S, SB> {
        HarnessBuilder {
            model: Set(model),
            store: self.store,
            sandbox: self.sandbox,
            options: self.options,
            model_id_explicit: self.model_id_explicit,
            extras: self.extras,
        }
    }
}

impl<M, SB> HarnessBuilder<M, Unset, SB> {
    #[must_use]
    pub fn with_store<S>(self, store: S) -> HarnessBuilder<M, Set<Arc<dyn EventStore>>, SB>
    where
        S: EventStore,
    {
        self.with_store_arc(Arc::new(store))
    }

    #[must_use]
    pub fn with_store_arc(
        self,
        store: Arc<dyn EventStore>,
    ) -> HarnessBuilder<M, Set<Arc<dyn EventStore>>, SB> {
        HarnessBuilder {
            model: self.model,
            store: Set(store),
            sandbox: self.sandbox,
            options: self.options,
            model_id_explicit: self.model_id_explicit,
            extras: self.extras,
        }
    }
}

impl<M, SB> HarnessBuilder<M, Set<Arc<dyn EventStore>>, SB> {
    #[must_use]
    pub fn with_store<S>(self, store: S) -> HarnessBuilder<M, Set<Arc<dyn EventStore>>, SB>
    where
        S: EventStore,
    {
        self.with_store_arc(Arc::new(store))
    }

    #[must_use]
    pub fn with_store_arc(
        self,
        store: Arc<dyn EventStore>,
    ) -> HarnessBuilder<M, Set<Arc<dyn EventStore>>, SB> {
        HarnessBuilder {
            model: self.model,
            store: Set(store),
            sandbox: self.sandbox,
            options: self.options,
            model_id_explicit: self.model_id_explicit,
            extras: self.extras,
        }
    }
}

impl<M, S> HarnessBuilder<M, S, Unset> {
    #[must_use]
    pub fn with_sandbox<SB>(self, sandbox: SB) -> HarnessBuilder<M, S, Set<Arc<dyn SandboxBackend>>>
    where
        SB: SandboxBackend,
    {
        self.with_sandbox_arc(Arc::new(sandbox))
    }

    #[must_use]
    pub fn with_sandbox_arc(
        self,
        sandbox: Arc<dyn SandboxBackend>,
    ) -> HarnessBuilder<M, S, Set<Arc<dyn SandboxBackend>>> {
        HarnessBuilder {
            model: self.model,
            store: self.store,
            sandbox: Set(sandbox),
            options: self.options,
            model_id_explicit: self.model_id_explicit,
            extras: self.extras,
        }
    }
}

impl<M, S> HarnessBuilder<M, S, Set<Arc<dyn SandboxBackend>>> {
    #[must_use]
    pub fn with_sandbox<SB>(self, sandbox: SB) -> HarnessBuilder<M, S, Set<Arc<dyn SandboxBackend>>>
    where
        SB: SandboxBackend,
    {
        self.with_sandbox_arc(Arc::new(sandbox))
    }

    #[must_use]
    pub fn with_sandbox_arc(
        self,
        sandbox: Arc<dyn SandboxBackend>,
    ) -> HarnessBuilder<M, S, Set<Arc<dyn SandboxBackend>>> {
        HarnessBuilder {
            model: self.model,
            store: self.store,
            sandbox: Set(sandbox),
            options: self.options,
            model_id_explicit: self.model_id_explicit,
            extras: self.extras,
        }
    }
}

impl
    HarnessBuilder<
        Set<Arc<dyn ModelProvider>>,
        Set<Arc<dyn EventStore>>,
        Set<Arc<dyn SandboxBackend>>,
    >
{
    pub async fn build(self) -> Result<Harness, HarnessError> {
        let mut builder = self;
        if !builder.model_id_explicit {
            let model = &builder.model.0;
            let provider_id = model.provider_id();
            let supported_models = model.supported_models();
            if !supported_models.iter().any(|descriptor| {
                descriptor.provider_id == provider_id
                    && descriptor.model_id == builder.options.model_id
            }) {
                if let Some(descriptor) = supported_models
                    .iter()
                    .find(|descriptor| descriptor.provider_id == provider_id)
                {
                    builder.options.model_id = descriptor.model_id.clone();
                }
            }
        }
        Harness::from_builder(builder).await
    }
}

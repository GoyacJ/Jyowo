#[allow(unused_imports)]
use super::app::*;
#[allow(unused_imports)]
use super::artifacts::*;
#[allow(unused_imports)]
use super::automations::*;
#[allow(unused_imports)]
use super::constants::*;
#[allow(unused_imports)]
use super::contracts::*;
#[allow(unused_imports)]
#[allow(unused_imports)]
use super::error::*;
#[allow(unused_imports)]
use super::evals::*;
#[allow(unused_imports)]
use super::mcp::*;
#[allow(unused_imports)]
#[allow(unused_imports)]
use super::model_settings::*;
#[allow(unused_imports)]
use super::plugins::*;
#[allow(unused_imports)]
use super::providers::*;
#[allow(unused_imports)]
use super::skills::*;
#[allow(unused_imports)]
use super::stores::*;
#[allow(unused_imports)]
use super::validation::*;
use super::*;
use crate::storage_layout::{ConfigScope, JyowoHome, RuntimeLayout, RuntimeScope, StorageLayout};
use async_trait::async_trait;
use harness_contracts::CapabilityRegistry;
use harness_contracts::{KillScope, Redactor, SandboxError, SandboxExitStatus};
use harness_execution::{
    AuthorizationEventSink, AuthorizationService, ExecutionPreflightRegistry,
    ReqwestToolNetworkBroker, TicketLedger,
};
use harness_journal::InMemoryEventStore;
use harness_model::{default_account_usage_registry, ProviderAccountUsageRegistry};
use harness_permission::{NoopDecisionPersistence, PermissionAuthority};
use harness_sandbox::{ContainerLifecycle, DockerSandbox, RoutingSandboxBackend, VolumeMount};
use harness_tool::ToolNetworkBrokerCap;

#[derive(Clone)]
pub(crate) struct ProviderConfigRevealTokenRecord {
    pub(crate) api_key_fingerprint: [u8; 32],
    pub(crate) config_id: String,
    pub(crate) expires_at: Instant,
}

#[derive(Clone)]
pub(crate) struct DesktopActiveRuntime {
    default_model_config_id: Option<String>,
    default_model_id: String,
    default_protocol: ModelProtocol,
    default_model_options: harness_contracts::ModelRequestOptions,
    provider_config_fingerprint: Option<[u8; 32]>,
    settings_runtime: Option<Arc<DesktopSettingsRuntime>>,
}

pub(crate) struct McpDiagnosticSubscriptionHandle {
    pub(crate) task: JoinHandle<()>,
    pub(crate) window_label: String,
}

fn active_runtime_provider_binding(
    project_workspace_root: Option<&Path>,
    default_model_id: &str,
    default_protocol: ModelProtocol,
) -> Result<Option<(String, [u8; 32], harness_contracts::ModelRequestOptions)>, CommandErrorPayload>
{
    let store = project_workspace_root.map_or_else(
        DesktopProviderSettingsStore::global_only,
        |workspace_root| DesktopProviderSettingsStore::new(workspace_root.to_path_buf()),
    );
    let Some(record) = store.load_record()? else {
        return Ok(None);
    };
    let Some(config_id) = record.default_config_id.as_deref() else {
        return Ok(None);
    };
    let config = provider_config_by_id(&record, config_id)?;
    if config.model_id != default_model_id || config.protocol != default_protocol {
        return Ok(None);
    }
    Ok(Some((
        config.id.clone(),
        provider_config_runtime_fingerprint(config)?,
        config.model_options.clone(),
    )))
}

impl DesktopRuntimeState {
    pub fn with_workspace_for_test(workspace_root: PathBuf) -> Result<Self, CommandErrorPayload> {
        let workspace_root = canonical_workspace_root(workspace_root, "workspace root".to_owned())?;
        let storage_layout = test_storage_layout_for_workspace(&workspace_root);
        let runtime_layout = storage_layout.runtime_layout_for_project(&workspace_root);

        Ok(Self {
            active_runtime: Arc::new(RwLock::new(DesktopActiveRuntime {
                default_model_config_id: None,
                default_model_id: "llama3.1".to_owned(),
                default_protocol: ModelProtocol::ChatCompletions,
                default_model_options: harness_contracts::ModelRequestOptions::default(),
                provider_config_fingerprint: None,
                settings_runtime: None,
            })),
            default_conversation_id: SessionId::new(),
            mcp_diagnostic_store: Arc::new(DesktopMcpDiagnosticStore::new_runtime_root(
                runtime_layout.runtime_root.clone(),
            )),
            mcp_diagnostic_subscriptions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            mcp_server_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_server_store: Arc::new(DesktopMcpServerStore::global(storage_layout.clone())),
            project_mcp_server_store: Some(Arc::new(DesktopMcpServerStore::new(
                storage_layout.clone(),
                workspace_root.clone(),
            ))),
            provider_api_key_reveal_tokens: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            plugin_store: Arc::new(DesktopPluginStore::global(storage_layout.clone())),
            plugin_store_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_settings_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_settings_store: Arc::new(
                DesktopProviderSettingsStore::global_only_with_layout(storage_layout.clone()),
            ),
            provider_diagnostics_store: Arc::new(
                DesktopProviderDiagnosticsStore::new_runtime_root(
                    runtime_layout.runtime_root.clone(),
                ),
            ),
            provider_probe_flights: new_provider_probe_flights(),
            provider_quota_cache_store: Arc::new(DesktopProviderQuotaCacheStore::new_runtime_root(
                runtime_layout.runtime_root.clone(),
            )),
            provider_catalog_snapshot_store: Arc::new(
                DesktopProviderCatalogSnapshotStore::new_runtime_root(
                    runtime_layout.runtime_root.clone(),
                ),
            ),
            model_usage_rollup_store: Arc::new(DesktopModelUsageRollupStore::new_runtime_root(
                runtime_layout.runtime_root.clone(),
            )),
            official_quota_flights: new_official_quota_flights(),
            account_usage_registry: Arc::new(default_account_usage_registry()),
            provider_capability_route_store: Arc::new(
                DesktopProviderCapabilityRouteStore::global_only_with_layout(
                    storage_layout.clone(),
                ),
            ),
            provider_capability_routes: Arc::new(ParkingRwLock::new(
                empty_provider_capability_route_settings(),
            )),
            execution_settings_lock: Arc::new(tokio::sync::Mutex::new(())),
            execution_settings_store: Arc::new(
                DesktopExecutionSettingsStore::global_only_with_layout(storage_layout.clone()),
            ),
            skill_catalog_install_tasks: Arc::new(RwLock::new(HashMap::new())),
            skill_store: Arc::new(DesktopSkillStore::global(storage_layout.clone())),
            skill_store_lock: Arc::new(tokio::sync::Mutex::new(())),
            settings_reload_lock: Arc::new(tokio::sync::Mutex::new(())),
            global_config_store: Some(GlobalConfigStore::new(storage_layout.clone())),
            project_config_store: Some(ProjectConfigStore::new(
                storage_layout,
                workspace_root.clone(),
            )),
            runtime_layout,
            workspace_root,
        })
    }

    pub fn with_workspace_and_account_usage_registry_for_test(
        workspace_root: PathBuf,
        account_usage_registry: Arc<ProviderAccountUsageRegistry>,
    ) -> Result<Self, CommandErrorPayload> {
        let mut state = Self::with_workspace_for_test(workspace_root)?;
        state.account_usage_registry = account_usage_registry;
        Ok(state)
    }

    pub fn with_settings_runtime(
        settings_runtime: Arc<DesktopSettingsRuntime>,
    ) -> Result<Self, CommandErrorPayload> {
        Self::with_settings_runtime_for_workspace(
            current_process_workspace_root()?,
            settings_runtime,
        )
    }

    pub fn with_settings_runtime_for_workspace(
        workspace_root: PathBuf,
        settings_runtime: Arc<DesktopSettingsRuntime>,
    ) -> Result<Self, CommandErrorPayload> {
        Self::with_settings_runtime_for_canonical_workspace(
            canonical_workspace_root(workspace_root, "workspace root".to_owned())?,
            settings_runtime,
        )
    }

    #[doc(hidden)]
    pub fn with_settings_runtime_for_global_conversation(
        runtime_root: PathBuf,
        conversation_id: SessionId,
        settings_runtime: Arc<DesktopSettingsRuntime>,
    ) -> Result<Self, CommandErrorPayload> {
        let provider = settings_runtime.model_provider();
        let mut default_model_id = settings_runtime.options().model_id.clone();
        let supported_models = provider.supported_models();
        if !supported_models
            .iter()
            .any(|model| model.model_id == default_model_id)
        {
            if let Some(model) = supported_models.first() {
                default_model_id = model.model_id.clone();
            }
        }
        let default_protocol = provider
            .snapshot_for_model(&default_model_id)
            .map_err(|error| runtime_init_failed(error.to_string()))?
            .protocol;
        let default_model_options = settings_runtime
            .options()
            .default_session_options
            .model_options
            .clone();
        let runtime_layout =
            global_conversation_runtime_layout_with_runtime_root(conversation_id, runtime_root);
        Self::with_settings_runtime_and_model_for_layout(
            runtime_layout,
            settings_runtime,
            default_model_id,
            default_protocol,
            default_model_options,
        )
    }

    fn with_settings_runtime_for_canonical_workspace(
        workspace_root: PathBuf,
        settings_runtime: Arc<DesktopSettingsRuntime>,
    ) -> Result<Self, CommandErrorPayload> {
        let provider = settings_runtime.model_provider();
        let mut default_model_id = settings_runtime.options().model_id.clone();
        let supported_models = provider.supported_models();
        if !supported_models
            .iter()
            .any(|model| model.model_id == default_model_id)
        {
            if let Some(model) = supported_models.first() {
                default_model_id = model.model_id.clone();
            }
        }
        let default_protocol = provider
            .snapshot_for_model(&default_model_id)
            .map_err(|error| runtime_init_failed(error.to_string()))?
            .protocol;
        let default_model_options = settings_runtime
            .options()
            .default_session_options
            .model_options
            .clone();
        Self::with_settings_runtime_and_model_for_workspace(
            workspace_root,
            settings_runtime,
            default_model_id,
            default_protocol,
            default_model_options,
        )
    }

    fn with_settings_runtime_and_model_for_workspace(
        workspace_root: PathBuf,
        settings_runtime: Arc<DesktopSettingsRuntime>,
        default_model_id: String,
        default_protocol: ModelProtocol,
        default_model_options: harness_contracts::ModelRequestOptions,
    ) -> Result<Self, CommandErrorPayload> {
        let runtime_layout = project_runtime_layout(&workspace_root);
        Self::with_settings_runtime_and_model_for_layout(
            runtime_layout,
            settings_runtime,
            default_model_id,
            default_protocol,
            default_model_options,
        )
    }

    fn with_settings_runtime_and_model_for_layout(
        runtime_layout: RuntimeLayout,
        settings_runtime: Arc<DesktopSettingsRuntime>,
        default_model_id: String,
        default_protocol: ModelProtocol,
        default_model_options: harness_contracts::ModelRequestOptions,
    ) -> Result<Self, CommandErrorPayload> {
        let provider_capability_routes = settings_runtime.provider_capability_routes();
        let active_runtime_binding = active_runtime_provider_binding(
            runtime_layout.workspace_root.as_deref(),
            &default_model_id,
            default_protocol,
        )
        .ok()
        .flatten();
        let state_workspace_root = runtime_layout.conversation_cwd.clone();
        let state = Self {
            active_runtime: Arc::new(RwLock::new(DesktopActiveRuntime {
                default_model_config_id: active_runtime_binding
                    .as_ref()
                    .map(|binding| binding.0.clone()),
                default_model_id,
                default_protocol,
                default_model_options: active_runtime_binding
                    .as_ref()
                    .map(|binding| binding.2.clone())
                    .unwrap_or(default_model_options),
                provider_config_fingerprint: active_runtime_binding.map(|binding| binding.1),
                settings_runtime: Some(settings_runtime),
            })),
            default_conversation_id: SessionId::new(),
            mcp_diagnostic_store: Arc::new(DesktopMcpDiagnosticStore::new_runtime_root(
                runtime_layout.runtime_root.clone(),
            )),
            mcp_diagnostic_subscriptions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            mcp_server_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_server_store: mcp_server_store_for_layout(&runtime_layout),
            project_mcp_server_store: runtime_layout.workspace_root.as_ref().map(
                |workspace_root| {
                    Arc::new(DesktopMcpServerStore::new(
                        storage_layout_for_home(),
                        workspace_root.clone(),
                    )) as Arc<dyn McpServerStore>
                },
            ),
            provider_api_key_reveal_tokens: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            plugin_store: Arc::new(DesktopPluginStore::global(storage_layout_for_home())),
            plugin_store_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_settings_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_settings_store: Arc::new(DesktopProviderSettingsStore::from_runtime_layout(
                &runtime_layout,
            )),
            provider_diagnostics_store: Arc::new(
                DesktopProviderDiagnosticsStore::new_runtime_root(
                    runtime_layout.runtime_root.clone(),
                ),
            ),
            provider_probe_flights: new_provider_probe_flights(),
            provider_quota_cache_store: Arc::new(DesktopProviderQuotaCacheStore::new_runtime_root(
                runtime_layout.runtime_root.clone(),
            )),
            provider_catalog_snapshot_store: Arc::new(
                DesktopProviderCatalogSnapshotStore::new_runtime_root(
                    runtime_layout.runtime_root.clone(),
                ),
            ),
            model_usage_rollup_store: Arc::new(DesktopModelUsageRollupStore::new_runtime_root(
                runtime_layout.runtime_root.clone(),
            )),
            official_quota_flights: new_official_quota_flights(),
            account_usage_registry: Arc::new(default_account_usage_registry()),
            provider_capability_route_store: provider_capability_route_store_for_layout(
                &runtime_layout,
            ),
            provider_capability_routes,
            execution_settings_lock: Arc::new(tokio::sync::Mutex::new(())),
            execution_settings_store: Arc::new(DesktopExecutionSettingsStore::from_runtime_layout(
                &runtime_layout,
            )),
            skill_catalog_install_tasks: Arc::new(RwLock::new(HashMap::new())),
            skill_store: Arc::new(DesktopSkillStore::global(storage_layout_for_home())),
            skill_store_lock: Arc::new(tokio::sync::Mutex::new(())),
            settings_reload_lock: Arc::new(tokio::sync::Mutex::new(())),
            global_config_store: Some(global_config_store_for_home()),
            project_config_store: runtime_layout
                .workspace_root
                .as_deref()
                .map(project_config_store_for_workspace),
            runtime_layout,
            workspace_root: state_workspace_root,
        };
        Ok(state)
    }

    #[doc(hidden)]
    pub fn set_provider_settings_store_for_test(
        &mut self,
        provider_settings_store: Arc<dyn ProviderSettingsStore>,
    ) {
        self.provider_settings_store = provider_settings_store;
    }

    #[doc(hidden)]
    pub fn set_mcp_server_store_for_test(&mut self, mcp_server_store: Arc<dyn McpServerStore>) {
        self.mcp_server_store = mcp_server_store;
    }

    #[doc(hidden)]
    pub fn set_project_mcp_server_store_for_test(
        &mut self,
        mcp_server_store: Option<Arc<dyn McpServerStore>>,
    ) {
        self.project_mcp_server_store = mcp_server_store;
    }

    #[doc(hidden)]
    pub fn set_skill_store_for_test(&mut self, skill_store: Arc<dyn SkillStore>) {
        self.skill_store = skill_store;
    }

    #[doc(hidden)]
    pub fn set_config_stores_for_test(
        &mut self,
        global_config_store: GlobalConfigStore,
        project_config_store: Option<ProjectConfigStore>,
    ) {
        self.global_config_store = Some(global_config_store);
        self.project_config_store = project_config_store;
    }

    #[doc(hidden)]
    pub fn set_active_runtime_provider_config_for_test(
        &self,
        config: &ProviderConfigRecord,
    ) -> Result<(), CommandErrorPayload> {
        let fingerprint = provider_config_runtime_fingerprint(config)?;
        let mut active_runtime = self
            .active_runtime
            .write()
            .expect("desktop active runtime lock should not be poisoned");
        active_runtime.default_model_config_id = Some(config.id.clone());
        active_runtime.default_model_id = config.model_id.clone();
        active_runtime.default_protocol = config.protocol;
        active_runtime.provider_config_fingerprint = Some(fingerprint);
        Ok(())
    }

    #[must_use]
    pub fn settings_runtime(&self) -> Option<Arc<DesktopSettingsRuntime>> {
        self.active_runtime
            .read()
            .expect("desktop active runtime lock should not be poisoned")
            .settings_runtime
            .as_ref()
            .map(Arc::clone)
    }

    pub fn replace_settings_runtime(
        &self,
        settings_runtime: Arc<DesktopSettingsRuntime>,
        default_model_id: String,
        default_protocol: ModelProtocol,
        default_model_options: harness_contracts::ModelRequestOptions,
    ) {
        let active_runtime_binding = active_runtime_provider_binding(
            self.runtime_layout.workspace_root.as_deref(),
            &default_model_id,
            default_protocol,
        )
        .ok()
        .flatten();
        *self
            .active_runtime
            .write()
            .expect("desktop active runtime lock should not be poisoned") = DesktopActiveRuntime {
            default_model_config_id: active_runtime_binding
                .as_ref()
                .map(|binding| binding.0.clone()),
            default_model_id,
            default_protocol,
            default_model_options: active_runtime_binding
                .as_ref()
                .map(|binding| binding.2.clone())
                .unwrap_or(default_model_options),
            provider_config_fingerprint: active_runtime_binding.map(|binding| binding.1),
            settings_runtime: Some(settings_runtime),
        };
    }

    pub fn effective_execution_settings(
        &self,
        run_permission_mode: Option<PermissionMode>,
    ) -> Result<harness_contracts::ExecutionDefaultsRecord, CommandErrorPayload> {
        resolve_effective_execution_settings(
            self.global_config_store.as_ref(),
            None,
            run_permission_mode,
            None,
        )
    }

    pub fn settings_session_options(
        &self,
        session_id: SessionId,
    ) -> Result<SessionOptions, CommandErrorPayload> {
        let active_runtime = self
            .active_runtime
            .read()
            .expect("desktop active runtime lock should not be poisoned");
        self.settings_session_options_for_model(
            session_id,
            active_runtime.default_model_id.clone(),
            active_runtime.default_protocol,
            active_runtime.default_model_options.clone(),
        )
    }

    pub fn settings_session_options_for_model(
        &self,
        session_id: SessionId,
        model_id: String,
        protocol: ModelProtocol,
        model_options: harness_contracts::ModelRequestOptions,
    ) -> Result<SessionOptions, CommandErrorPayload> {
        let execution_settings = self.effective_execution_settings(None)?;
        let mut options = SessionOptions::new(self.conversation_cwd_for_session(session_id))
            .with_tenant_id(TenantId::SINGLE)
            .with_session_id(session_id)
            .with_agent_runtime_root(self.runtime_layout.runtime_root.clone())
            .with_interactivity(InteractivityLevel::FullyInteractive)
            .with_model_id(model_id)
            .with_protocol(protocol)
            .with_model_options(model_options)
            .with_tool_profile(execution_settings.tool_profile)
            .with_context_compression_trigger_ratio(
                execution_settings.context_compression_trigger_ratio,
            );
        if let Some(global_config) = &self.global_config_store {
            let mut profiles = jyowo_harness_sdk::builtin_agent_profiles();
            profiles.extend(global_config.load_global_agent_profiles()?);
            options = options.with_agent_profiles(profiles);
        }
        if let Some(project_workspace_root) = self.runtime_layout.workspace_root.as_ref() {
            options = options.with_project_workspace_root(project_workspace_root.clone());
        }
        Ok(options)
    }

    fn conversation_cwd_for_session(&self, session_id: SessionId) -> PathBuf {
        if self.runtime_layout.workspace_root.is_some() {
            return self.runtime_layout.conversation_cwd.clone();
        }
        self.runtime_layout
            .runtime_root
            .join("workdir")
            .join(session_id.to_string())
    }

    #[must_use]
    pub fn default_conversation_id(&self) -> SessionId {
        self.default_conversation_id
    }

    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    #[must_use]
    pub fn project_workspace_root(&self) -> Option<&Path> {
        self.runtime_layout.workspace_root.as_deref()
    }

    #[must_use]
    pub fn runtime_layout(&self) -> &RuntimeLayout {
        &self.runtime_layout
    }

    #[must_use]
    pub fn runtime_root(&self) -> &Path {
        &self.runtime_layout.runtime_root
    }

    #[must_use]
    pub fn conversation_cwd(&self) -> &Path {
        &self.runtime_layout.conversation_cwd
    }
}

pub type ManagedDesktopRuntime = Arc<AsyncRwLock<DesktopRuntimeState>>;

#[must_use]
pub fn managed_runtime_state() -> ManagedDesktopRuntime {
    Arc::new(AsyncRwLock::new(initial_managed_runtime_state()))
}

pub(crate) fn initial_managed_runtime_state() -> DesktopRuntimeState {
    if let Ok(registry) = ProjectRegistry::load() {
        if let Some(active_path) = registry.active_path() {
            if let Ok(state) = tauri::async_runtime::block_on(runtime_state_for_workspace(
                PathBuf::from(active_path),
            )) {
                return state;
            }
        }
    }

    unconfigured_runtime_state()
}

pub(crate) fn unconfigured_runtime_state() -> DesktopRuntimeState {
    tauri::async_runtime::block_on(runtime_state_for_no_workspace())
        .expect("desktop no-workspace runtime state should initialize")
}

#[must_use]
pub fn runtime_state() -> DesktopRuntimeState {
    tauri::async_runtime::block_on(runtime_state_async())
        .expect("desktop runtime state should initialize")
}

pub async fn runtime_state_async() -> Result<DesktopRuntimeState, CommandErrorPayload> {
    runtime_state_for_workspace(current_workspace_root()?).await
}

pub(crate) async fn runtime_state_for_no_workspace(
) -> Result<DesktopRuntimeState, CommandErrorPayload> {
    runtime_state_for_global_conversation(SessionId::new()).await
}

pub async fn runtime_state_for_workspace(
    workspace_root: PathBuf,
) -> Result<DesktopRuntimeState, CommandErrorPayload> {
    runtime_state_from_settings_store(workspace_root, None).await
}

pub(crate) async fn runtime_state_for_global_conversation(
    conversation_id: SessionId,
) -> Result<DesktopRuntimeState, CommandErrorPayload> {
    let layout = global_conversation_runtime_layout(conversation_id);
    runtime_state_for_global_conversation_layout(layout).await
}

async fn runtime_state_for_global_conversation_layout(
    layout: RuntimeLayout,
) -> Result<DesktopRuntimeState, CommandErrorPayload> {
    let provider_capability_routes = Arc::new(ParkingRwLock::new(
        empty_provider_capability_route_settings(),
    ));
    let (settings_runtime, model_id, protocol, model_options) = build_desktop_settings_runtime(
        &layout,
        None,
        Arc::clone(&provider_capability_routes),
        None,
    )
    .await?;

    DesktopRuntimeState::with_settings_runtime_and_model_for_layout(
        layout,
        Arc::new(settings_runtime),
        model_id,
        protocol,
        model_options,
    )
}

#[doc(hidden)]
pub async fn runtime_state_with_provider_settings_store_for_test(
    workspace_root: PathBuf,
    provider_settings_store: Arc<dyn ProviderSettingsStore>,
) -> Result<DesktopRuntimeState, CommandErrorPayload> {
    runtime_state_from_settings_store(workspace_root, Some(provider_settings_store)).await
}

async fn runtime_state_from_settings_store(
    workspace_root: PathBuf,
    provider_settings_store_override: Option<Arc<dyn ProviderSettingsStore>>,
) -> Result<DesktopRuntimeState, CommandErrorPayload> {
    let workspace_root = canonical_workspace_root(workspace_root, "workspace root".to_owned())?;
    let route_store = DesktopProviderCapabilityRouteStore::new(workspace_root.clone());
    let provider_capability_routes = shared_provider_capability_routes_from_store(&route_store)?;
    let layout = project_runtime_layout(&workspace_root);
    let (settings_runtime, model_id, protocol, model_options) = build_desktop_settings_runtime(
        &layout,
        None,
        Arc::clone(&provider_capability_routes),
        provider_settings_store_override.clone(),
    )
    .await?;

    let mut state = DesktopRuntimeState::with_settings_runtime_and_model_for_workspace(
        workspace_root,
        Arc::new(settings_runtime),
        model_id,
        protocol,
        model_options,
    )?;
    if let Some(provider_settings_store) = provider_settings_store_override {
        state.set_provider_settings_store_for_test(provider_settings_store);
    }
    Ok(state)
}

pub(crate) fn project_runtime_layout(workspace_root: &Path) -> RuntimeLayout {
    let home = jyowo_home_dir();
    let storage =
        crate::storage_layout::StorageLayout::new(crate::storage_layout::JyowoHome::new(home));
    storage.runtime_layout_for_project(workspace_root)
}

pub(crate) fn global_conversation_runtime_layout(conversation_id: SessionId) -> RuntimeLayout {
    let home = jyowo_home_dir();
    let storage =
        crate::storage_layout::StorageLayout::new(crate::storage_layout::JyowoHome::new(home));
    storage.runtime_layout_for_global_conversation(conversation_id)
}

pub(crate) fn global_conversation_runtime_layout_with_runtime_root(
    conversation_id: SessionId,
    runtime_root: PathBuf,
) -> RuntimeLayout {
    RuntimeLayout {
        scope: RuntimeScope::GlobalConversation { conversation_id },
        workspace_root: None,
        conversation_cwd: runtime_root
            .join("workdir")
            .join(conversation_id.to_string()),
        runtime_root,
        config_scope: ConfigScope::GlobalOnly,
    }
}

fn storage_layout_for_home() -> crate::storage_layout::StorageLayout {
    let home = jyowo_home_dir();
    crate::storage_layout::StorageLayout::new(crate::storage_layout::JyowoHome::new(home))
}

fn test_storage_layout_for_workspace(workspace_root: &Path) -> StorageLayout {
    StorageLayout::new(JyowoHome::new(
        workspace_root.join(".jyowo-test-home").join(".jyowo"),
    ))
}

pub(crate) fn global_config_store_for_home() -> GlobalConfigStore {
    GlobalConfigStore::new(storage_layout_for_home())
}

pub(crate) fn project_config_store_for_workspace(workspace_root: &Path) -> ProjectConfigStore {
    ProjectConfigStore::new(storage_layout_for_home(), workspace_root.to_path_buf())
}

fn mcp_server_store_for_layout(layout: &RuntimeLayout) -> Arc<dyn McpServerStore> {
    let _ = layout;
    Arc::new(DesktopMcpServerStore::global(storage_layout_for_home()))
}

fn provider_capability_route_store_for_layout(
    layout: &RuntimeLayout,
) -> Arc<dyn ProviderCapabilityRouteStore> {
    let _ = layout;
    Arc::new(DesktopProviderCapabilityRouteStore::global_only())
}

fn jyowo_home_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .unwrap_or_else(|| std::ffi::OsString::from("."));
    PathBuf::from(home).join(".jyowo")
}

/// Builds the desktop process sandbox as a routing backend.
///
/// Selection order (first backend whose preflight and before_execute succeed wins):
/// 1. OS-level isolation (`Seatbelt` on macOS, `Bubblewrap` on Linux) when the
///    platform binary is present.
/// 2. Docker ephemeral per-exec with the workspace mounted at `/workspace`.
/// 3. `LocalIsolation::None` — only selected by the router for unrestricted
///    network and read_write_all workspace policies.
///
/// Docker fallback is only registered for `read_write_all` workspace policy
/// because `VolumeMount::workspace` is a read-write mount. The router will skip
/// Docker for read-only or writable-subpath policies until those mounts are
/// implemented and tested.
pub(crate) async fn build_desktop_process_sandbox(
    workspace_root: &Path,
) -> Result<Arc<dyn SandboxBackend>, CommandErrorPayload> {
    let mut backends: Vec<Arc<dyn SandboxBackend>> = Vec::new();

    // 1. OS-level isolation.
    let platform_isolation = LocalIsolation::for_current_platform();
    if platform_isolation.is_os_level() && isolation_binary_available(platform_isolation) {
        let local = LocalSandbox::new(workspace_root).with_isolation(platform_isolation);
        backends.push(Arc::new(local));
    }

    // 2. Docker ephemeral fallback.
    match build_docker_fallback(workspace_root).await {
        Ok(docker) => backends.push(docker),
        Err(reason) => {
            log::info!("desktop sandbox: docker fallback unavailable: {reason}");
        }
    }

    // 3. LocalIsolation::None as last resort.
    // Only supports unrestricted network and read_write_all workspace, so the
    // router will not select it for restricted policies.
    let no_isolation = LocalSandbox::new(workspace_root);
    backends.push(Arc::new(no_isolation));

    let router = RoutingSandboxBackend::new(backends).map_err(|error| {
        runtime_init_failed(format!("sandbox routing initialization failed: {error}"))
    })?;
    Ok(Arc::new(router))
}

/// Returns `true` when the platform isolation binary is present on the host.
fn isolation_binary_available(isolation: LocalIsolation) -> bool {
    match isolation {
        LocalIsolation::Bubblewrap => binary_in_path("bwrap"),
        LocalIsolation::Seatbelt => binary_in_path("sandbox-exec"),
        LocalIsolation::JobObject => {
            // JobObject on Windows is not yet verified per plan — do not claim support.
            false
        }
        LocalIsolation::None => false,
    }
}

fn binary_in_path(name: &str) -> bool {
    std::env::var_os("PATH").map_or(false, |path| {
        std::env::split_paths(&path).any(|dir| dir.join(name).is_file())
    })
}

/// Builds a Docker ephemeral sandbox with the workspace mounted at `/workspace`.
///
/// Fails with a backend-authored reason when Docker is unreachable or the
/// image cannot be verified. The availability check is bounded to avoid
/// blocking startup when the Docker daemon is not running.
async fn build_docker_fallback(workspace_root: &Path) -> Result<Arc<DockerSandbox>, SandboxError> {
    // Fast-fail when the docker binary is not on PATH.
    if !binary_in_path("docker") {
        return Err(SandboxError::Unavailable {
            backend: "docker".to_owned(),
            detail: "docker binary not found".to_owned(),
        });
    }

    build_docker_fallback_with_binary(workspace_root, PathBuf::from("docker")).await
}

async fn build_docker_fallback_with_binary(
    workspace_root: &Path,
    docker_binary: PathBuf,
) -> Result<Arc<DockerSandbox>, SandboxError> {
    let docker = DockerSandbox::builder()
        .docker_binary(docker_binary)
        .mount(VolumeMount::workspace(workspace_root, "/workspace"))
        .lifecycle(ContainerLifecycle::EphemeralPerExec)
        .build()?;

    // Bound the daemon check so a non-responsive daemon doesn't block startup.
    tokio::time::timeout(Duration::from_secs(5), docker.ensure_available())
        .await
        .map_err(|_| SandboxError::Unavailable {
            backend: "docker".to_owned(),
            detail: "docker daemon check timed out".to_owned(),
        })??;

    verify_docker_workspace_mount(&docker, workspace_root).await?;

    Ok(Arc::new(docker))
}

async fn verify_docker_workspace_mount(
    docker: &DockerSandbox,
    workspace_root: &Path,
) -> Result<(), SandboxError> {
    let probe = ExecSpec {
        command: "sh".to_owned(),
        args: vec![
            "-c".to_owned(),
            r#"test "$(pwd)" = "/workspace" && test -d /workspace"#.to_owned(),
        ],
        cwd: Some(workspace_root.to_path_buf()),
        stdin: StdioSpec::Null,
        stdout: StdioSpec::Null,
        stderr: StdioSpec::Null,
        timeout: Some(Duration::from_secs(5)),
        workspace_access: WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: Vec::new(),
        },
        ..ExecSpec::default()
    };
    let mut ctx = ExecContext::new(Arc::new(RecordingSandboxEventSink::default()));
    ctx.workspace_root = workspace_root.to_path_buf();

    let handle = tokio::time::timeout(Duration::from_secs(5), docker.execute(probe, ctx))
        .await
        .map_err(|_| SandboxError::Unavailable {
            backend: "docker".to_owned(),
            detail: "docker workspace probe timed out before spawn".to_owned(),
        })?
        .map_err(|error| SandboxError::Unavailable {
            backend: "docker".to_owned(),
            detail: format!("docker workspace probe failed to start: {error}"),
        })?;

    let outcome = match tokio::time::timeout(Duration::from_secs(5), handle.activity.wait()).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => {
            return Err(SandboxError::Unavailable {
                backend: "docker".to_owned(),
                detail: format!("docker workspace probe failed: {error}"),
            });
        }
        Err(_) => {
            let _ = handle.activity.kill(9, KillScope::Process).await;
            return Err(SandboxError::Unavailable {
                backend: "docker".to_owned(),
                detail: "docker workspace probe timed out".to_owned(),
            });
        }
    };

    if outcome.exit_status == SandboxExitStatus::Code(0) {
        return Ok(());
    }

    Err(SandboxError::Unavailable {
        backend: "docker".to_owned(),
        detail: format!(
            "docker workspace probe exited with {:?}",
            outcome.exit_status
        ),
    })
}

pub(crate) async fn build_desktop_settings_runtime(
    layout: &RuntimeLayout,
    model_config_id: Option<&str>,
    provider_capability_routes: Arc<ParkingRwLock<ProviderCapabilityRouteSettings>>,
    provider_settings_store_override: Option<Arc<dyn ProviderSettingsStore>>,
) -> Result<
    (
        DesktopSettingsRuntime,
        String,
        ModelProtocol,
        harness_contracts::ModelRequestOptions,
    ),
    CommandErrorPayload,
> {
    let project_workspace_root = layout.workspace_root.as_deref();
    let execution_cwd = layout.conversation_cwd.as_path();
    let runtime_root = &layout.runtime_root;

    let event_store: Arc<dyn EventStore> = Arc::new(InMemoryEventStore::new(Arc::new(
        DefaultRedactor::default(),
    )));
    let global_mcp_server_records =
        DesktopMcpServerStore::global(storage_layout_for_home()).load_records()?;
    let project_mcp_server_records = project_workspace_root
        .map(|workspace_root| {
            DesktopMcpServerStore::new(storage_layout_for_home(), workspace_root.to_path_buf())
                .load_records()
        })
        .transpose()?
        .unwrap_or_default();
    let mcp_server_records =
        effective_mcp_records(global_mcp_server_records, project_mcp_server_records);
    let mcp_diagnostic_store: Arc<dyn McpDiagnosticStore> = Arc::new(
        DesktopMcpDiagnosticStore::new_runtime_root(runtime_root.clone()),
    );
    let default_provider_settings_store =
        Arc::new(DesktopProviderSettingsStore::from_runtime_layout(layout))
            as Arc<dyn ProviderSettingsStore>;
    let provider_settings_store =
        provider_settings_store_override.unwrap_or(default_provider_settings_store);
    let provider_settings_record = provider_settings_store.load_record()?;
    let provider_defaults_extra = provider_settings_record
        .as_ref()
        .and_then(|record| {
            let config_id = model_config_id.or(record.default_config_id.as_deref())?;
            record
                .configs
                .iter()
                .find(|config| config.id == config_id)
                .map(provider_defaults_body)
        })
        .unwrap_or(Value::Null);
    let (model_provider, model_id, protocol, model_options) =
        model_from_provider_settings(provider_settings_store.as_ref(), model_config_id)?
            .unwrap_or_else(|| {
                (
                    Arc::new(LocalLlamaProvider::default()) as Arc<dyn ModelProvider>,
                    "llama3.1".to_owned(),
                    ModelProtocol::ChatCompletions,
                    harness_contracts::ModelRequestOptions::default(),
                )
            });
    let storage_layout = storage_layout_for_home();
    let global_config_store = global_config_store_for_home();
    let global_skill_selection = global_config_store
        .load_global_skill_selection_if_present()?
        .map(|selection| selection.enabled.into_iter().collect());
    let global_skill_store = DesktopSkillStore::global(storage_layout);
    let skill_loader = SkillLoader::default().with_source(SkillSourceConfig::DirectoryPackages {
        path: global_skill_store.enabled_dir(),
        source_kind: DirectorySourceKind::User,
        allowed_package_ids: global_skill_selection,
    });
    let provider_credential_resolver: Arc<dyn ProviderCredentialResolverCap> =
        Arc::new(DesktopProviderCredentialResolver::new(
            Arc::clone(&provider_settings_store),
            Arc::clone(&provider_capability_routes),
        ));
    let plugin_store: Arc<dyn PluginStore> =
        Arc::new(DesktopPluginStore::global(storage_layout_for_home()));
    let plugin_registry = build_plugin_registry(execution_cwd, None, plugin_store.as_ref(), None)?;

    let sandbox = build_desktop_process_sandbox(execution_cwd).await?;

    let rule_broker: Arc<dyn harness_permission::PermissionBroker> = Arc::new(
        harness_permission::RuleEngineBroker::builder()
            .with_tenant(TenantId::SINGLE)
            .build()
            .await
            .map_err(|error| {
                runtime_init_failed(format!("rule engine broker initialization failed: {error}"))
            })?,
    );

    let permission_authority = PermissionAuthority::builder()
        .with_policy_broker(Arc::clone(&rule_broker))
        .with_transient_decision_store(Arc::new(NoopDecisionPersistence))
        .build()
        .map_err(|error| {
            runtime_init_failed(format!(
                "permission authority initialization failed: {error}"
            ))
        })?;

    let event_sink: Arc<dyn AuthorizationEventSink> = Arc::new(DesktopAuthorizationEventSink {
        event_store: Arc::clone(&event_store),
    });

    // ── HTTP broker: same Arc instance injected into both authorization preflight
    // and capability registry, per Task 6 design. ──
    let ticket_ledger = Arc::new(TicketLedger::default());
    let broker_redactor: Arc<dyn Redactor> = Arc::new(DefaultRedactor::default());
    let network_broker: Arc<dyn ToolNetworkBrokerCap> = Arc::new(
        ReqwestToolNetworkBroker::new_with_ticket_authority(
            Duration::from_secs(120),
            10 * 1024 * 1024, // 10 MiB max response
            Arc::clone(&broker_redactor),
            ticket_ledger.authority_key(),
        )
        .map_err(|error| {
            runtime_init_failed(format!("network broker initialization failed: {error}"))
        })?,
    );

    let diagnostics_runner: Arc<dyn DiagnosticsRunnerCap> =
        Arc::new(DesktopDiagnosticsRunner::new(Arc::clone(&sandbox)));
    let plugin_sidecar_capability = Arc::new(());

    let mut capabilities = CapabilityRegistry::default();
    capabilities.install(ToolCapability::NetworkBroker, Arc::clone(&network_broker));
    capabilities.install(
        ToolCapability::ProviderCredentialResolver,
        Arc::clone(&provider_credential_resolver),
    );
    capabilities.install(
        ToolCapability::Custom("diagnostics_runner".to_owned()),
        Arc::clone(&diagnostics_runner),
    );
    capabilities.install(
        ToolCapability::Custom("plugin_sidecar".to_owned()),
        Arc::clone(&plugin_sidecar_capability),
    );
    let preflight_capabilities = Arc::new(capabilities);

    let registry = ExecutionPreflightRegistry::new(
        Arc::clone(&sandbox),
        Some(network_broker.clone()),
        preflight_capabilities,
    );
    let authorization_service = Arc::new(AuthorizationService::new(
        Arc::new(permission_authority),
        registry,
        event_sink,
        ticket_ledger,
    ));
    let mcp_config = mcp_config_from_layered_records(
        mcp_server_records,
        SessionId::new(),
        AgentId::new(),
        Arc::clone(&mcp_diagnostic_store),
        Arc::clone(&authorization_service),
        execution_cwd,
    )
    .await?;

    let mut default_session_options = SessionOptions::new(execution_cwd)
        .with_agent_runtime_root(runtime_root)
        .with_model_id(model_id.clone())
        .with_protocol(protocol)
        .with_model_options(model_options.clone());
    if provider_defaults_extra != Value::Null {
        default_session_options = default_session_options.with_model_extra(provider_defaults_extra);
    }
    if let Some(project_workspace_root) = project_workspace_root {
        default_session_options =
            default_session_options.with_project_workspace_root(project_workspace_root);
    }
    let settings_harness_builder = DesktopSettingsRuntime::builder()
        .with_workspace_root(execution_cwd)
        .with_model_arc(model_provider)
        .with_model_id(model_id.clone())
        .with_shared_provider_capability_routes(provider_capability_routes)
        .with_default_session_options(default_session_options)
        .with_store_arc(event_store)
        .with_sandbox_arc(sandbox)
        .with_capability(
            ToolCapability::ProviderCredentialResolver,
            provider_credential_resolver,
        )
        .with_capability(
            ToolCapability::Custom("diagnostics_runner".to_owned()),
            diagnostics_runner,
        )
        .with_capability(
            ToolCapability::Custom("plugin_sidecar".to_owned()),
            plugin_sidecar_capability,
        )
        .with_capability(ToolCapability::NetworkBroker, network_broker)
        .with_mcp_config(mcp_config)
        .with_plugin_registry(plugin_registry)
        .with_skill_loader(skill_loader)
        .with_permission_authority_arc(authorization_service.permission_authority())
        .with_authorization_service_arc(authorization_service);
    let settings_harness =
        super::provider_continuation_runtime::with_file_provider_continuation_store(
            settings_harness_builder,
            runtime_root,
        )
        .map_err(|error| {
            runtime_init_failed(format!(
                "provider continuation store initialization failed: {error}"
            ))
        })?
        .build()
        .await
        .map_err(|error| {
            runtime_init_failed(format!(
                "desktop settings runtime initialization failed: {error}"
            ))
        })?;
    let settings_runtime = DesktopSettingsRuntime::try_from(settings_harness).map_err(|error| {
        runtime_init_failed(format!(
            "desktop settings runtime ownership validation failed: {error}"
        ))
    })?;

    Ok((settings_runtime, model_id, protocol, model_options))
}

struct DesktopAuthorizationEventSink {
    event_store: Arc<dyn EventStore>,
}

#[async_trait]
impl AuthorizationEventSink for DesktopAuthorizationEventSink {
    async fn emit_batch(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
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

pub(crate) struct DesktopDiagnosticsRunner {
    sandbox: Arc<dyn SandboxBackend>,
}

impl DesktopDiagnosticsRunner {
    fn new(sandbox: Arc<dyn SandboxBackend>) -> Self {
        Self { sandbox }
    }
}

impl DiagnosticsRunnerCap for DesktopDiagnosticsRunner {
    fn run_diagnostics(
        &self,
        request: DiagnosticsRunRequest,
    ) -> BoxFuture<'_, Result<DiagnosticsRawOutput, ToolError>> {
        Box::pin(async move {
            let spec =
                diagnostics_exec_spec(request.runner, &request.workspace_root, request.run_id);
            let event_sink = Arc::new(RecordingSandboxEventSink::default());
            let ctx = ExecContext {
                session_id: request.session_id,
                run_id: request.run_id,
                tool_use_id: None,
                tenant_id: request.tenant_id,
                workspace_root: request.workspace_root.clone(),
                correlation_id: harness_contracts::CorrelationId::new(),
                event_sink: event_sink.clone(),
                redactor: Arc::new(DefaultRedactor::default()),
                blob_store: None,
                execution_id: 0,
            };
            let mut handle = execute_with_lifecycle(Arc::clone(&self.sandbox), spec, ctx)
                .await
                .map_err(ToolError::Sandbox)?;
            let stdout_stream = handle.stdout.take();
            let stderr_stream = handle.stderr.take();
            let (stdout, stderr, outcome) = tokio::join!(
                collect_diagnostics_output(stdout_stream),
                collect_diagnostics_output(stderr_stream),
                handle.activity.wait()
            );
            outcome.map_err(ToolError::Sandbox)?;
            Ok(DiagnosticsRawOutput {
                runner: request.runner,
                stdout,
                stderr,
                sandbox_events: event_sink.events(),
            })
        })
    }
}

pub(crate) fn diagnostics_exec_spec(
    runner: DiagnosticsRunnerKind,
    workspace_root: &Path,
    run_id: RunId,
) -> ExecSpec {
    let (command, args) = match runner {
        DiagnosticsRunnerKind::Rust => (
            "cargo".to_owned(),
            vec![
                "check".to_owned(),
                "--message-format=json".to_owned(),
                "--target-dir".to_owned(),
                std::env::temp_dir()
                    .join(format!("jyowo-diagnostics-target-{run_id}"))
                    .display()
                    .to_string(),
            ],
        ),
        DiagnosticsRunnerKind::DesktopTs => (
            "pnpm".to_owned(),
            vec![
                "--dir".to_owned(),
                "apps/desktop".to_owned(),
                "exec".to_owned(),
                "tsc".to_owned(),
                "--noEmit".to_owned(),
                "--pretty".to_owned(),
                "false".to_owned(),
            ],
        ),
        _ => ("true".to_owned(), Vec::new()),
    };
    ExecSpec {
        command,
        args,
        cwd: Some(workspace_root.to_path_buf()),
        stdin: StdioSpec::Null,
        stdout: StdioSpec::Piped,
        stderr: StdioSpec::Piped,
        timeout: Some(Duration::from_secs(180)),
        workspace_access: WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: Vec::new(),
        },
        ..ExecSpec::default()
    }
}

pub(crate) async fn collect_diagnostics_output(
    stream: Option<BoxStream<'static, Bytes>>,
) -> String {
    const MAX_DIAGNOSTICS_OUTPUT_BYTES: usize = 1_048_576;
    let mut stream = match stream {
        Some(stream) => stream,
        None => return String::new(),
    };
    let mut output = Vec::new();
    while let Some(chunk) = stream.next().await {
        if output.len() >= MAX_DIAGNOSTICS_OUTPUT_BYTES {
            break;
        }
        let remaining = MAX_DIAGNOSTICS_OUTPUT_BYTES - output.len();
        output.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }
    String::from_utf8_lossy(&output).into_owned()
}

#[derive(Default)]
pub(crate) struct RecordingSandboxEventSink {
    events: ParkingMutex<Vec<Event>>,
}

impl RecordingSandboxEventSink {
    fn events(&self) -> Vec<Event> {
        self.events.lock().clone()
    }
}

impl EventSink for RecordingSandboxEventSink {
    fn emit(&self, event: Event) -> Result<(), SandboxError> {
        self.events.lock().push(event);
        Ok(())
    }
}

pub(crate) fn build_plugin_registry(
    execution_cwd: &Path,
    project_workspace_root: Option<&Path>,
    plugin_store: &dyn PluginStore,
    global_plugin_store: Option<&DesktopPluginStore>,
) -> Result<PluginRegistry, CommandErrorPayload> {
    let settings = plugin_store.load_record()?;
    let global_settings = global_plugin_store
        .map(PluginStore::load_record)
        .transpose()?;
    let project_enabled_plugin_ids: BTreeSet<String> = settings
        .records
        .iter()
        .filter(|record| record.enabled)
        .map(|record| record.plugin_id.0.clone())
        .collect();
    let global_enabled_plugin_ids: BTreeSet<String> = global_settings
        .as_ref()
        .map(|settings| {
            settings
                .records
                .iter()
                .filter(|record| record.enabled)
                .map(|record| record.plugin_id.0.clone())
                .collect()
        })
        .unwrap_or_default();
    let global_plugin_ids: BTreeSet<String> = global_settings
        .as_ref()
        .map(|settings| {
            settings
                .records
                .iter()
                .map(|record| record.plugin_id.0.clone())
                .collect()
        })
        .unwrap_or_default();
    let allow_project_plugins = settings.allow_project_plugins;
    let (sidecar_sandbox, sidecar_sandbox_mode) = desktop_plugin_sidecar_sandbox(execution_cwd);
    let mut entries = BTreeMap::new();
    let mut plugin_enabled_by_name = BTreeMap::<PluginName, bool>::new();
    if let (Some(global_store), Some(global_settings)) = (global_plugin_store, &global_settings) {
        collect_plugin_registry_records(
            &global_settings.records,
            global_store,
            &global_enabled_plugin_ids,
            false,
            true,
            &BTreeSet::new(),
            &mut entries,
            &mut plugin_enabled_by_name,
        )?;
    }
    collect_plugin_registry_records(
        &settings.records,
        plugin_store,
        &project_enabled_plugin_ids,
        false,
        false,
        &global_plugin_ids,
        &mut entries,
        &mut plugin_enabled_by_name,
    )?;
    let disabled_plugins = plugin_enabled_by_name
        .into_iter()
        .filter_map(|(name, enabled)| (!enabled).then_some(name))
        .collect();

    let mut builder = PluginRegistry::builder()
        .without_memory_provider_capability()
        .with_config(plugin_config_from_parts(
            allow_project_plugins,
            disabled_plugins,
            entries,
            project_workspace_root,
        ))
        .with_source(DiscoverySource::User(plugin_store.package_root()));

    if project_workspace_root.is_some() {
        builder = builder.with_source(DiscoverySource::Workspace(
            plugin_store.workspace_plugin_root(),
        ));
    }

    if let Some(global_store) = global_plugin_store {
        builder = builder.with_source(DiscoverySource::User(global_store.package_root()));
    }

    let mut builder = builder
        .with_source(DiscoverySource::CargoExtension)
        .with_manifest_loader(Arc::new(FileManifestLoader))
        .with_manifest_loader(Arc::new(
            CargoExtensionManifestLoader::new()
                .with_timeout(Duration::from_secs(5))
                .with_search_paths(desktop_cargo_extension_search_paths(plugin_store))
                .with_sandbox(
                    sidecar_sandbox.clone(),
                    sidecar_sandbox_mode.clone(),
                    execution_cwd.to_path_buf(),
                ),
        ))
        .with_runtime_loader(Arc::new(CargoExtensionRuntimeLoader::new().with_sandbox(
            sidecar_sandbox,
            sidecar_sandbox_mode,
            execution_cwd.to_path_buf(),
        )));

    if let Some(project_workspace_root) = project_workspace_root {
        if allow_project_plugins {
            builder = builder.with_source(DiscoverySource::Project(
                project_workspace_root.to_path_buf(),
            ));
        }
    }

    builder.build().map_err(|error| {
        runtime_init_failed(format!("plugin registry initialization failed: {error}"))
    })
}

fn collect_plugin_registry_records(
    records: &[PluginStoreRecord],
    store: &dyn PluginStore,
    enabled_plugin_ids: &BTreeSet<String>,
    use_selection: bool,
    require_record_enabled: bool,
    record_enabled_required_ids: &BTreeSet<String>,
    entries: &mut BTreeMap<PluginName, Value>,
    plugin_enabled_by_name: &mut BTreeMap<PluginName, bool>,
) -> Result<(), CommandErrorPayload> {
    for record in records {
        let mut enabled = if use_selection {
            enabled_plugin_ids.contains(&record.plugin_id.0)
        } else {
            record.enabled
        };
        if require_record_enabled {
            enabled &= record.enabled;
        }
        if record_enabled_required_ids.contains(&record.plugin_id.0) {
            enabled &= record.enabled;
        }
        if enabled {
            verify_installed_plugin_content_hash(record, store)?;
        }
        let name = PluginName::new(record.name.clone())
            .map_err(|error| runtime_init_failed(format!("plugin record invalid: {error}")))?;
        entries.insert(name.clone(), record.config.clone());
        plugin_enabled_by_name
            .entry(name)
            .and_modify(|current| *current &= enabled)
            .or_insert(enabled);
    }
    Ok(())
}

pub(crate) fn desktop_plugin_sidecar_sandbox(
    workspace_root: &Path,
) -> (Arc<dyn SandboxBackend>, SandboxMode) {
    let isolation = LocalIsolation::for_current_platform();
    let mode = SandboxMode::OsLevel(local_isolation_tag(isolation));
    let sandbox = LocalSandbox::new(workspace_root).with_isolation(isolation);
    (Arc::new(sandbox), mode)
}

pub(crate) fn local_isolation_tag(isolation: LocalIsolation) -> LocalIsolationTag {
    match isolation {
        LocalIsolation::None => LocalIsolationTag::None,
        LocalIsolation::Bubblewrap => LocalIsolationTag::Bubblewrap,
        LocalIsolation::Seatbelt => LocalIsolationTag::Seatbelt,
        LocalIsolation::JobObject => LocalIsolationTag::JobObject,
    }
}

pub(crate) fn plugin_config_from_parts(
    allow_project_plugins: bool,
    disabled_plugins: BTreeSet<PluginName>,
    entries: BTreeMap<PluginName, Value>,
    project_workspace_root: Option<&Path>,
) -> PluginConfig {
    let allowed_user_plugins = entries.keys().cloned().collect();
    PluginConfig {
        allow_project_plugins,
        allowed_user_plugins: Some(allowed_user_plugins),
        disabled_plugins,
        entries,
        workspace_root: project_workspace_root.map(Path::to_path_buf),
        ..PluginConfig::default()
    }
}

pub(crate) fn desktop_cargo_extension_search_paths(plugin_store: &dyn PluginStore) -> Vec<PathBuf> {
    vec![plugin_store.cargo_extension_root()]
}

pub(crate) fn verify_installed_plugin_content_hash(
    record: &PluginStoreRecord,
    plugin_store: &dyn PluginStore,
) -> Result<(), CommandErrorPayload> {
    ensure_plugin_package_dir_name(&record.package_dir)?;
    let package_path = plugin_store.package_root().join(&record.package_dir);
    let current_hash = hash_plugin_package(&package_path)?;
    if current_hash == record.content_hash {
        return Ok(());
    }
    Err(runtime_operation_failed(format!(
        "plugin package content hash mismatch: {}",
        record.plugin_id.0
    )))
}

pub(crate) async fn reload_desktop_settings_runtime_after_plugin_change_locked(
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    if state.settings_runtime().is_none() {
        return Ok(());
    }
    let layout = state.runtime_layout().clone();
    let (settings_runtime, model_id, protocol, model_options) = build_desktop_settings_runtime(
        &layout,
        None,
        Arc::clone(&state.provider_capability_routes),
        Some(Arc::clone(&state.provider_settings_store)),
    )
    .await?;
    if let Some(old_settings_runtime) = state.settings_runtime() {
        if let Some(registry) = old_settings_runtime.plugin_registry() {
            for manifest in registry.list_activated() {
                let _ = registry.deactivate_cascade(&manifest.plugin_id()).await;
            }
        }
    }
    state.replace_settings_runtime(
        Arc::new(settings_runtime),
        model_id,
        protocol,
        model_options,
    );
    Ok(())
}

pub(crate) fn current_workspace_root() -> Result<PathBuf, CommandErrorPayload> {
    if let Some(value) = std::env::var_os(WORKSPACE_ROOT_ENV) {
        if value.is_empty() {
            return Err(runtime_init_failed(format!(
                "{WORKSPACE_ROOT_ENV} is empty"
            )));
        }

        return canonical_workspace_root(
            PathBuf::from(value),
            format!("{WORKSPACE_ROOT_ENV} workspace root"),
        );
    }

    let current_dir = std::env::current_dir()
        .map_err(|error| runtime_init_failed(format!("workspace root unavailable: {error}")))?;
    canonical_workspace_root(current_dir, "current workspace root".to_owned())
}

pub(crate) fn current_process_workspace_root() -> Result<PathBuf, CommandErrorPayload> {
    let current_dir = std::env::current_dir()
        .map_err(|error| runtime_init_failed(format!("workspace root unavailable: {error}")))?;
    canonical_workspace_root(current_dir, "current workspace root".to_owned())
}

pub(crate) fn canonical_workspace_root(
    workspace_root: PathBuf,
    source: String,
) -> Result<PathBuf, CommandErrorPayload> {
    workspace_root.canonicalize().map_err(|error| {
        runtime_init_failed(format!(
            "{source} unavailable at {}: {error}",
            workspace_root.display()
        ))
    })
}

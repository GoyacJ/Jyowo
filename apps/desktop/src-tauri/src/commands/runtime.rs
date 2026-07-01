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
use super::conversations::*;
#[allow(unused_imports)]
use super::error::*;
#[allow(unused_imports)]
use super::evals::*;
#[allow(unused_imports)]
use super::mcp::*;
#[allow(unused_imports)]
use super::memory::*;
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
use harness_model::{default_account_usage_registry, ProviderAccountUsageRegistry};

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
    provider_config_fingerprint: Option<[u8; 32]>,
    harness: Option<Arc<Harness>>,
}

pub(crate) struct ConversationSubscriptionHandle {
    pub(crate) conversation_id: String,
    pub(crate) task: JoinHandle<()>,
    pub(crate) window_label: String,
}

pub(crate) struct McpDiagnosticSubscriptionHandle {
    pub(crate) task: JoinHandle<()>,
    pub(crate) window_label: String,
}

fn active_runtime_provider_binding(
    workspace_root: &Path,
    default_model_id: &str,
    default_protocol: ModelProtocol,
) -> Result<Option<(String, [u8; 32])>, CommandErrorPayload> {
    let store = DesktopProviderSettingsStore::new(workspace_root.to_path_buf());
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
    )))
}

impl DesktopRuntimeState {
    #[must_use]
    pub(crate) fn agent_capability_resolution_context(&self) -> AgentCapabilityResolutionContext {
        AgentCapabilityResolutionContext {
            stream_permission_runtime_available: self.stream_permission_runtime.is_some(),
        }
    }

    pub fn with_workspace_for_test(workspace_root: PathBuf) -> Result<Self, CommandErrorPayload> {
        let workspace_root = canonical_workspace_root(workspace_root, "workspace root".to_owned())?;

        Ok(Self {
            active_runtime: Arc::new(RwLock::new(DesktopActiveRuntime {
                default_model_config_id: None,
                default_model_id: "llama3.1".to_owned(),
                default_protocol: ModelProtocol::ChatCompletions,
                provider_config_fingerprint: None,
                harness: None,
            })),
            automation_lock: Arc::new(tokio::sync::Mutex::new(())),
            automation_store: Arc::new(DesktopAutomationStore::new(workspace_root.clone())),
            conversation_metadata_lock: Arc::new(tokio::sync::Mutex::new(())),
            conversation_metadata_store: Arc::new(DesktopConversationMetadataStore::new(
                workspace_root.clone(),
            )),
            conversation_event_subscriptions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            default_conversation_id: SessionId::new(),
            deleted_conversation_ids: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            memory_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_diagnostic_store: Arc::new(DesktopMcpDiagnosticStore::new(workspace_root.clone())),
            mcp_diagnostic_subscriptions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            mcp_server_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_server_store: Arc::new(DesktopMcpServerStore::new(workspace_root.clone())),
            permission_resolver: None,
            provider_api_key_reveal_tokens: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            plugin_store: Arc::new(DesktopPluginStore::new(workspace_root.clone())),
            plugin_store_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_settings_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_settings_store: Arc::new(DesktopProviderSettingsStore::new(
                workspace_root.clone(),
            )),
            provider_diagnostics_store: Arc::new(DesktopProviderDiagnosticsStore::new(
                workspace_root.clone(),
            )),
            provider_probe_flights: new_provider_probe_flights(),
            provider_quota_cache_store: Arc::new(DesktopProviderQuotaCacheStore::new(
                workspace_root.clone(),
            )),
            official_quota_flights: new_official_quota_flights(),
            account_usage_registry: Arc::new(default_account_usage_registry()),
            provider_capability_route_store: Arc::new(DesktopProviderCapabilityRouteStore::new(
                workspace_root.clone(),
            )),
            provider_capability_routes: Arc::new(ParkingRwLock::new(
                empty_provider_capability_route_settings(),
            )),
            execution_settings_lock: Arc::new(tokio::sync::Mutex::new(())),
            execution_settings_store: Arc::new(DesktopExecutionSettingsStore::new(
                workspace_root.clone(),
            )),
            skill_catalog_install_tasks: Arc::new(RwLock::new(HashMap::new())),
            skill_store: Arc::new(DesktopSkillStore::new(workspace_root.clone())),
            skill_store_lock: Arc::new(tokio::sync::Mutex::new(())),
            start_run_lock: Arc::new(tokio::sync::Mutex::new(())),
            stream_permission_runtime: None,
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

    pub fn with_harness_and_stream_permission_runtime(
        harness: Arc<Harness>,
        stream_permission_runtime: Arc<StreamPermissionRuntime>,
    ) -> Result<Self, CommandErrorPayload> {
        Self::with_harness_stream_permission_runtime_for_workspace(
            current_process_workspace_root()?,
            harness,
            stream_permission_runtime,
        )
    }

    pub fn with_harness_and_stream_permission_runtime_for_workspace(
        workspace_root: PathBuf,
        harness: Arc<Harness>,
        stream_permission_runtime: Arc<StreamPermissionRuntime>,
    ) -> Result<Self, CommandErrorPayload> {
        Self::with_harness_stream_permission_runtime_for_workspace(
            canonical_workspace_root(workspace_root, "workspace root".to_owned())?,
            harness,
            stream_permission_runtime,
        )
    }

    fn with_harness_stream_permission_runtime_for_workspace(
        workspace_root: PathBuf,
        harness: Arc<Harness>,
        stream_permission_runtime: Arc<StreamPermissionRuntime>,
    ) -> Result<Self, CommandErrorPayload> {
        let provider = harness.model_provider();
        let mut default_model_id = harness.options().model_id.clone();
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
        Self::with_harness_stream_permission_runtime_and_model_for_workspace(
            workspace_root,
            harness,
            stream_permission_runtime,
            default_model_id,
            default_protocol,
        )
    }

    fn with_harness_stream_permission_runtime_and_model_for_workspace(
        workspace_root: PathBuf,
        harness: Arc<Harness>,
        stream_permission_runtime: Arc<StreamPermissionRuntime>,
        default_model_id: String,
        default_protocol: ModelProtocol,
    ) -> Result<Self, CommandErrorPayload> {
        let Some(permission_broker) = harness.permission_broker() else {
            return Err(runtime_unavailable(
                "Permission decisions require a Harness PermissionBroker.",
            ));
        };
        if !Arc::ptr_eq(&permission_broker, &stream_permission_runtime.broker()) {
            return Err(runtime_unavailable(
                "Harness PermissionBroker must come from the stream permission runtime.",
            ));
        }
        let permission_resolver: Arc<dyn PermissionResolver> = stream_permission_runtime.clone();

        let provider_capability_routes = harness.provider_capability_routes();
        let active_runtime_binding =
            active_runtime_provider_binding(&workspace_root, &default_model_id, default_protocol)?;
        Ok(Self {
            active_runtime: Arc::new(RwLock::new(DesktopActiveRuntime {
                default_model_config_id: active_runtime_binding
                    .as_ref()
                    .map(|binding| binding.0.clone()),
                default_model_id,
                default_protocol,
                provider_config_fingerprint: active_runtime_binding.map(|binding| binding.1),
                harness: Some(harness),
            })),
            automation_lock: Arc::new(tokio::sync::Mutex::new(())),
            automation_store: Arc::new(DesktopAutomationStore::new(workspace_root.clone())),
            conversation_metadata_lock: Arc::new(tokio::sync::Mutex::new(())),
            conversation_metadata_store: Arc::new(DesktopConversationMetadataStore::new(
                workspace_root.clone(),
            )),
            conversation_event_subscriptions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            default_conversation_id: SessionId::new(),
            deleted_conversation_ids: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            memory_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_diagnostic_store: Arc::new(DesktopMcpDiagnosticStore::new(workspace_root.clone())),
            mcp_diagnostic_subscriptions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            mcp_server_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_server_store: Arc::new(DesktopMcpServerStore::new(workspace_root.clone())),
            permission_resolver: Some(permission_resolver),
            provider_api_key_reveal_tokens: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            plugin_store: Arc::new(DesktopPluginStore::new(workspace_root.clone())),
            plugin_store_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_settings_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_settings_store: Arc::new(DesktopProviderSettingsStore::new(
                workspace_root.clone(),
            )),
            provider_diagnostics_store: Arc::new(DesktopProviderDiagnosticsStore::new(
                workspace_root.clone(),
            )),
            provider_probe_flights: new_provider_probe_flights(),
            provider_quota_cache_store: Arc::new(DesktopProviderQuotaCacheStore::new(
                workspace_root.clone(),
            )),
            official_quota_flights: new_official_quota_flights(),
            account_usage_registry: Arc::new(default_account_usage_registry()),
            provider_capability_route_store: Arc::new(DesktopProviderCapabilityRouteStore::new(
                workspace_root.clone(),
            )),
            provider_capability_routes,
            execution_settings_lock: Arc::new(tokio::sync::Mutex::new(())),
            execution_settings_store: Arc::new(DesktopExecutionSettingsStore::new(
                workspace_root.clone(),
            )),
            skill_catalog_install_tasks: Arc::new(RwLock::new(HashMap::new())),
            skill_store: Arc::new(DesktopSkillStore::new(workspace_root.clone())),
            skill_store_lock: Arc::new(tokio::sync::Mutex::new(())),
            start_run_lock: Arc::new(tokio::sync::Mutex::new(())),
            stream_permission_runtime: Some(stream_permission_runtime),
            workspace_root,
        })
    }

    #[must_use]
    pub fn harness(&self) -> Option<Arc<Harness>> {
        self.active_runtime
            .read()
            .expect("desktop active runtime lock should not be poisoned")
            .harness
            .as_ref()
            .map(Arc::clone)
    }

    pub fn replace_harness(
        &self,
        harness: Arc<Harness>,
        default_model_id: String,
        default_protocol: ModelProtocol,
    ) {
        let active_runtime_binding = active_runtime_provider_binding(
            &self.workspace_root,
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
            provider_config_fingerprint: active_runtime_binding.map(|binding| binding.1),
            harness: Some(harness),
        };
    }

    #[must_use]
    pub fn active_conversation_runtime_for_model_config(
        &self,
        session_id: SessionId,
        model_config_id: &str,
        provider_config_fingerprint: [u8; 32],
    ) -> Option<(Arc<Harness>, SessionOptions)> {
        let active_runtime = self
            .active_runtime
            .read()
            .expect("desktop active runtime lock should not be poisoned");
        if active_runtime.default_model_config_id.as_deref() != Some(model_config_id)
            || active_runtime.provider_config_fingerprint != Some(provider_config_fingerprint)
        {
            return None;
        }
        let harness = active_runtime.harness.as_ref().map(Arc::clone)?;
        let options = self.conversation_session_options_for_model(
            session_id,
            active_runtime.default_model_id.clone(),
            active_runtime.default_protocol,
        );
        Some((harness, options))
    }

    #[must_use]
    pub fn active_conversation_runtime(
        &self,
        session_id: SessionId,
    ) -> Option<(Arc<Harness>, SessionOptions)> {
        let active_runtime = self
            .active_runtime
            .read()
            .expect("desktop active runtime lock should not be poisoned");
        let harness = active_runtime.harness.as_ref().map(Arc::clone)?;
        let options = self.conversation_session_options_for_model(
            session_id,
            active_runtime.default_model_id.clone(),
            active_runtime.default_protocol,
        );
        Some((harness, options))
    }

    #[must_use]
    pub fn pending_permission_requests(&self) -> Vec<PendingPermissionRequest> {
        self.stream_permission_runtime
            .as_ref()
            .map_or_else(Vec::new, |runtime| runtime.pending_permission_requests())
    }

    #[must_use]
    pub fn conversation_session_options(&self, session_id: SessionId) -> SessionOptions {
        let active_runtime = self
            .active_runtime
            .read()
            .expect("desktop active runtime lock should not be poisoned");
        self.conversation_session_options_for_model(
            session_id,
            active_runtime.default_model_id.clone(),
            active_runtime.default_protocol,
        )
    }

    #[must_use]
    pub fn conversation_session_options_for_model(
        &self,
        session_id: SessionId,
        model_id: String,
        protocol: ModelProtocol,
    ) -> SessionOptions {
        let execution_settings =
            self.execution_settings_store
                .load_record()
                .unwrap_or(ExecutionSettingsRecord {
                    permission_mode: PermissionMode::Default,
                    tool_profile: ToolProfile::Full,
                    context_compression_trigger_ratio: default_context_compression_trigger_ratio(),
                    subagents_enabled: false,
                    agent_teams_enabled: false,
                    background_agents_enabled: false,
                });
        SessionOptions::new(&self.workspace_root)
            .with_tenant_id(TenantId::SINGLE)
            .with_session_id(session_id)
            .with_interactivity(InteractivityLevel::FullyInteractive)
            .with_model_id(model_id)
            .with_protocol(protocol)
            .with_tool_profile(execution_settings.tool_profile)
            .with_context_compression_trigger_ratio(
                execution_settings.context_compression_trigger_ratio,
            )
    }

    #[must_use]
    pub fn default_conversation_id(&self) -> SessionId {
        self.default_conversation_id
    }

    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }
}

pub type ManagedDesktopRuntime = Arc<AsyncRwLock<DesktopRuntimeState>>;

#[must_use]
pub fn managed_runtime_state() -> ManagedDesktopRuntime {
    Arc::new(AsyncRwLock::new(initial_managed_runtime_state()))
}

pub fn spawn_automation_scheduler(runtime: ManagedDesktopRuntime) -> JoinHandle<()> {
    tokio::spawn(run_automation_scheduler(runtime))
}

pub fn spawn_automation_scheduler_on_tauri_runtime(
    runtime: ManagedDesktopRuntime,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(run_automation_scheduler(runtime))
}

pub(crate) async fn run_automation_scheduler(runtime: ManagedDesktopRuntime) {
    let mut interval = tokio::time::interval(AUTOMATION_SCHEDULER_INTERVAL);
    loop {
        interval.tick().await;
        let state = runtime.read().await.clone();
        let _ = run_due_automations_once_with_runtime_state(Utc::now(), &state).await;
    }
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
    let workspace_root = crate::project_registry::unconfigured_workspace_root();
    let _ = std::fs::create_dir_all(&workspace_root);
    DesktopRuntimeState::with_workspace_for_test(workspace_root).unwrap_or_else(|_| {
        tauri::async_runtime::block_on(runtime_state_async())
            .expect("desktop runtime state should initialize")
    })
}

#[must_use]
pub fn runtime_state() -> DesktopRuntimeState {
    tauri::async_runtime::block_on(runtime_state_async())
        .expect("desktop runtime state should initialize")
}

pub async fn runtime_state_async() -> Result<DesktopRuntimeState, CommandErrorPayload> {
    runtime_state_for_workspace(current_workspace_root()?).await
}

pub async fn runtime_state_for_workspace(
    workspace_root: PathBuf,
) -> Result<DesktopRuntimeState, CommandErrorPayload> {
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::default());
    runtime_state_from_stream_permission_runtime(workspace_root, stream_permission_runtime).await
}

pub(crate) async fn runtime_state_from_stream_permission_runtime(
    workspace_root: PathBuf,
    stream_permission_runtime: Arc<StreamPermissionRuntime>,
) -> Result<DesktopRuntimeState, CommandErrorPayload> {
    let workspace_root = canonical_workspace_root(workspace_root, "workspace root".to_owned())?;
    let route_store = DesktopProviderCapabilityRouteStore::new(workspace_root.clone());
    let provider_capability_routes = shared_provider_capability_routes_from_store(&route_store)?;
    let (harness, model_id, protocol) = build_desktop_harness(
        &workspace_root,
        Arc::clone(&stream_permission_runtime),
        None,
        Arc::clone(&provider_capability_routes),
    )
    .await?;

    let state =
        DesktopRuntimeState::with_harness_stream_permission_runtime_and_model_for_workspace(
            workspace_root,
            Arc::new(harness),
            stream_permission_runtime,
            model_id,
            protocol,
        )?;
    Ok(state)
}

pub(crate) async fn ensure_agent_supervisor_sidecar_for_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    crate::agent_supervisor::launch_agent_supervisor_sidecar(app, state.workspace_root.clone())
        .await
        .map_err(|error| {
            runtime_init_failed(format!("agent supervisor sidecar startup failed: {error}"))
        })
}

pub fn agent_supervisor_sidecar_startup_result_for_project_command(
    result: Result<(), crate::agent_supervisor::AgentSupervisorError>,
) -> Result<(), CommandErrorPayload> {
    if let Err(error) = result {
        // Project switching is not the policy authority for background agents. Keep this
        // command usable and let the capability resolver surface supervisor unavailability.
        log::warn!("agent supervisor sidecar startup failed after project command: {error}");
    }
    Ok(())
}

pub(crate) async fn build_desktop_harness(
    workspace_root: &Path,
    stream_permission_runtime: Arc<StreamPermissionRuntime>,
    model_config_id: Option<&str>,
    provider_capability_routes: Arc<ParkingRwLock<ProviderCapabilityRouteSettings>>,
) -> Result<(Harness, String, ModelProtocol), CommandErrorPayload> {
    let event_store = JsonlEventStore::open(
        workspace_root.join(".jyowo").join("runtime").join("events"),
        Arc::new(DefaultRedactor::default()),
    )
    .await
    .map_err(|error| runtime_init_failed(format!("event store initialization failed: {error}")))?;
    let mcp_server_store = DesktopMcpServerStore::new(workspace_root.to_path_buf());
    let mcp_diagnostic_store: Arc<dyn McpDiagnosticStore> =
        Arc::new(DesktopMcpDiagnosticStore::new(workspace_root.to_path_buf()));
    let mcp_config = mcp_config_from_records(
        mcp_server_store.load_records()?,
        SessionId::new(),
        AgentId::new(),
        Arc::clone(&mcp_diagnostic_store),
        workspace_root,
    )
    .await?;
    let provider_settings_store = DesktopProviderSettingsStore::new(workspace_root.to_path_buf());
    let conversation_metadata_store =
        DesktopConversationMetadataStore::new(workspace_root.to_path_buf());
    let (model_provider, model_id, protocol) =
        model_from_provider_settings(&provider_settings_store, model_config_id)?.unwrap_or_else(
            || {
                (
                    Arc::new(LocalLlamaProvider::default()) as Arc<dyn ModelProvider>,
                    "llama3.1".to_owned(),
                    ModelProtocol::ChatCompletions,
                )
            },
        );
    let skill_store = DesktopSkillStore::new(workspace_root.to_path_buf());
    let skill_loader = SkillLoader::default().with_source(SkillSourceConfig::DirectoryPackages {
        path: skill_store.enabled_dir(),
        source_kind: DirectorySourceKind::Workspace,
    });
    let blob_store = FileBlobStore::open(
        workspace_root.join(".jyowo").join("runtime").join("blobs"),
    )
    .map_err(|error| runtime_init_failed(format!("blob store initialization failed: {error}")))?;
    let provider_credential_resolver: Arc<dyn ProviderCredentialResolverCap> =
        Arc::new(DesktopProviderCredentialResolver::new(
            Arc::new(conversation_metadata_store),
            Arc::new(provider_settings_store.clone()),
            Arc::clone(&provider_capability_routes),
        ));
    let plugin_store: Arc<dyn PluginStore> =
        Arc::new(DesktopPluginStore::new(workspace_root.to_path_buf()));
    let plugin_registry = build_plugin_registry(workspace_root, plugin_store.as_ref())?;

    let sandbox = Arc::new(LocalSandbox::new(workspace_root)) as Arc<dyn SandboxBackend>;
    let diagnostics_runner: Arc<dyn DiagnosticsRunnerCap> =
        Arc::new(DesktopDiagnosticsRunner::new(Arc::clone(&sandbox)));
    let harness = Harness::builder()
        .with_workspace_root(workspace_root)
        .with_model_arc(model_provider)
        .with_model_id(model_id.clone())
        .with_shared_provider_capability_routes(provider_capability_routes)
        .with_default_session_options(
            SessionOptions::new(workspace_root)
                .with_model_id(model_id.clone())
                .with_protocol(protocol),
        )
        .with_store(event_store)
        .with_sandbox_arc(sandbox)
        .with_blob_store(blob_store)
        .with_capability(
            ToolCapability::ProviderCredentialResolver,
            provider_credential_resolver,
        )
        .with_capability(
            ToolCapability::Custom("diagnostics_runner".to_owned()),
            diagnostics_runner,
        )
        .with_mcp_config(mcp_config)
        .with_plugin_registry(plugin_registry)
        .with_memory_provider(InMemoryMemoryProvider::new("desktop-memory"))
        .with_skill_loader(skill_loader)
        .with_stream_permission_broker_arc(
            stream_permission_runtime.broker(),
            stream_permission_runtime.resolver_handle(),
        )
        .build()
        .await
        .map_err(|error| runtime_init_failed(format!("harness initialization failed: {error}")))?;

    Ok((harness, model_id, protocol))
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
    workspace_root: &Path,
    plugin_store: &dyn PluginStore,
) -> Result<PluginRegistry, CommandErrorPayload> {
    let settings = plugin_store.load_record()?;
    let (sidecar_sandbox, sidecar_sandbox_mode) = desktop_plugin_sidecar_sandbox(workspace_root);
    let mut entries = BTreeMap::new();
    let mut disabled_plugins = BTreeSet::new();
    for record in &settings.records {
        if record.enabled {
            verify_installed_plugin_content_hash(record, plugin_store)?;
        }
        let name = PluginName::new(record.name.clone())
            .map_err(|error| runtime_init_failed(format!("plugin record invalid: {error}")))?;
        entries.insert(name.clone(), record.config.clone());
        if !record.enabled {
            disabled_plugins.insert(name);
        }
    }

    let mut builder = PluginRegistry::builder()
        .with_config(plugin_config_from_settings(
            &settings,
            disabled_plugins,
            entries,
        ))
        .with_source(DiscoverySource::User(plugin_store.package_root()))
        .with_source(DiscoverySource::Workspace(
            plugin_store.workspace_plugin_root(),
        ))
        .with_source(DiscoverySource::CargoExtension)
        .with_manifest_loader(Arc::new(FileManifestLoader))
        .with_manifest_loader(Arc::new(
            CargoExtensionManifestLoader::new()
                .with_timeout(Duration::from_secs(5))
                .with_search_paths(desktop_cargo_extension_search_paths(plugin_store))
                .with_sandbox(
                    sidecar_sandbox.clone(),
                    sidecar_sandbox_mode.clone(),
                    workspace_root.to_path_buf(),
                ),
        ))
        .with_runtime_loader(Arc::new(CargoExtensionRuntimeLoader::new().with_sandbox(
            sidecar_sandbox,
            sidecar_sandbox_mode,
            workspace_root.to_path_buf(),
        )));

    if settings.allow_project_plugins {
        builder = builder.with_source(DiscoverySource::Project(workspace_root.to_path_buf()));
    }

    builder.build().map_err(|error| {
        runtime_init_failed(format!("plugin registry initialization failed: {error}"))
    })
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

pub(crate) fn plugin_config_from_settings(
    settings: &PluginSettingsRecord,
    disabled_plugins: BTreeSet<PluginName>,
    entries: BTreeMap<PluginName, Value>,
) -> PluginConfig {
    let allowed_user_plugins = entries.keys().cloned().collect();
    PluginConfig {
        allow_project_plugins: settings.allow_project_plugins,
        allowed_user_plugins: Some(allowed_user_plugins),
        disabled_plugins,
        entries,
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

pub(crate) async fn reload_desktop_harness_after_plugin_change_locked(
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let Some(stream_permission_runtime) = state.stream_permission_runtime.as_ref() else {
        return Ok(());
    };
    let (harness, model_id, protocol) = build_desktop_harness(
        &state.workspace_root,
        Arc::clone(stream_permission_runtime),
        None,
        Arc::clone(&state.provider_capability_routes),
    )
    .await?;
    if let Some(old_harness) = state.harness() {
        if let Some(registry) = old_harness.plugin_registry() {
            for manifest in registry.list_activated() {
                let _ = registry.deactivate_cascade(&manifest.plugin_id()).await;
            }
        }
    }
    state.replace_harness(Arc::new(harness), model_id, protocol);
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

impl PermissionResolver for StreamPermissionRuntime {
    fn resolve_permission<'a>(
        &'a self,
        request_id: RequestId,
        decision: Decision,
    ) -> Pin<Box<dyn Future<Output = Result<(), CommandErrorPayload>> + Send + 'a>> {
        Box::pin(async move {
            self.resolve_permission(request_id, decision)
                .await
                .map_err(|error| CommandErrorPayload {
                    code: "PERMISSION_RESOLVE_FAILED",
                    message: error.to_string(),
                })
        })
    }
}

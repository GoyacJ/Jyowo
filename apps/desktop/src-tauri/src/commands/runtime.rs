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
use crate::agent_supervisor::BackgroundSupervisorSession;
use crate::storage_layout::RuntimeLayout;
use async_trait::async_trait;
use harness_execution::{AuthorizationEventSink, AuthorizationService, TicketLedger};
use harness_model::{default_account_usage_registry, ProviderAccountUsageRegistry};
use harness_permission::{FileDecisionPersistence, IntegrityAlgorithm, PermissionAuthority};
use harness_provider_state::FileProviderContinuationStore;

const PROVIDER_CONTINUATION_RUNTIME_VERSION: &str = "1";

#[derive(Clone)]
struct DesktopBackgroundAgentStarter {
    workspace_root: PathBuf,
    event_store: Arc<dyn EventStore>,
}

impl BackgroundAgentStarterCap for DesktopBackgroundAgentStarter {
    fn start_background_agent(
        &self,
        request: BackgroundAgentToolStartRequest,
    ) -> futures::future::BoxFuture<'static, Result<BackgroundAgentToolStartResponse, ToolError>>
    {
        let workspace_root = self.workspace_root.clone();
        let event_store = Arc::clone(&self.event_store);
        Box::pin(async move {
            let settings_store = DesktopExecutionSettingsStore::new(workspace_root.clone());
            let settings = settings_store
                .load_record()
                .map_err(|error| ToolError::Internal(error.message))?;
            let capabilities_payload = agent_capabilities_payload(
                &settings,
                &workspace_root,
                Some(&AgentCapabilityResolutionContext {
                    stream_permission_runtime_available: true,
                }),
            );
            let capabilities = AgentCapabilitiesInput {
                subagents_available: capabilities_payload.subagents_available,
                agent_teams_available: capabilities_payload.agent_teams_available,
                background_agents_available: capabilities_payload.background_agents_available,
            };
            let settings_input = ExecutionSettingsAgentInput {
                subagents_enabled: settings.subagents_enabled,
                agent_teams_enabled: settings.agent_teams_enabled,
                background_agents_enabled: settings.background_agents_enabled,
            };
            let profiles = jyowo_harness_sdk::list_agent_profiles(&workspace_root)
                .map_err(|error| ToolError::Internal(error.to_string()))?;
            let profile_ids: Vec<String> = profiles.into_iter().map(|profile| profile.id).collect();
            let _resolved_policy = resolve_agent_runtime_policy(
                &workspace_root,
                &settings_input,
                Some(&request.agent_tool_policy),
                &capabilities,
                &profile_ids,
                &request.conversation_id.to_string(),
            )
            .map_err(|error| ToolError::Validation(error.to_string()))?;

            let store = Arc::new(
                AgentRuntimeStore::open(&workspace_root)
                    .map_err(|error| ToolError::Internal(error.to_string()))?,
            );
            let redactor = Arc::new(DefaultRedactor::default());
            let manager = BackgroundAgentManager::new(
                store,
                event_store,
                request.tenant_id,
                request.conversation_id,
                redactor.clone(),
            );
            let safe_input = safe_background_supervisor_input(
                &ConversationTurnInput::ask(request.goal.clone()),
                redactor.as_ref(),
            );
            let model_config_id = request.model_config_id.clone().ok_or_else(|| {
                ToolError::Validation("background_agent requires a model config id".to_owned())
            })?;
            if request.session.tenant_id != request.tenant_id
                || request.session.session_id != request.conversation_id
            {
                return Err(ToolError::Validation(
                    "background_agent session snapshot does not match tool context".to_owned(),
                ));
            }
            let supervisor_session =
                BackgroundSupervisorSession::from_tool_session_snapshot(request.session.clone());
            let mut agent_tool_policy = request.agent_tool_policy.clone();
            agent_tool_policy.background_agents = AgentUsePolicy::Off;
            let record = manager
                .start(BackgroundAgentStartRequest {
                    background_agent_id: None,
                    conversation_id: request.conversation_id,
                    title: request.title.clone(),
                    payload_json: json!({
                        "conversationId": request.conversation_id.to_string(),
                        "parentRunId": request.parent_run_id.to_string(),
                        "toolUseId": request.tool_use_id.to_string(),
                        "source": "background_agent_tool",
                        "supervisorExecution": {
                            "status": "queued",
                            "session": supervisor_session,
                            "input": safe_input,
                            "modelConfigId": model_config_id,
                            "permissionMode": request.permission_mode,
                            "agentToolPolicy": agent_tool_policy,
                        },
                    })
                    .to_string(),
                })
                .await
                .map_err(|error| ToolError::Internal(error.to_string()))?;
            let _ = crate::agent_supervisor::wake_agent_supervisor(&workspace_root).await;
            Ok(BackgroundAgentToolStartResponse {
                background_agent_id: record.background_agent_id,
                conversation_id: request.conversation_id,
                parent_run_id: request.parent_run_id,
                title: record.title,
                status: "started".to_owned(),
            })
        })
    }
}

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
            global_config_store: None,
            project_config_store: None,
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
        let _ = permission_broker;
        let Some(permission_resolver) = harness.permission_resolver_handle() else {
            return Err(runtime_unavailable(
                "Permission decisions require a Harness permission resolver.",
            ));
        };
        if !permission_resolver.same_origin_as(&stream_permission_runtime.resolver_handle()) {
            return Err(runtime_unavailable(
                "Harness permission resolver must come from the stream permission runtime.",
            ));
        }
        let permission_resolver: Arc<dyn PermissionResolver> = stream_permission_runtime.clone();

        let provider_capability_routes = harness.provider_capability_routes();
        let active_runtime_binding =
            active_runtime_provider_binding(&workspace_root, &default_model_id, default_protocol)?;
        let state = Self {
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
            global_config_store: Some(global_config_store_for_home()),
            project_config_store: Some(project_config_store_for_workspace(&workspace_root)),
            workspace_root,
        };
        // Migrate old execution-settings.json to project config overrides.
        // Format handled by ExecutionDefaultsRecord serde aliases (snake_case → camelCase).
        let _ = crate::commands::providers::migrate_execution_settings(&state.workspace_root);
        // Migrate old provider-capability-routes.json from runtime to project config.
        let _ =
            crate::commands::providers::migrate_provider_capability_routes(&state.workspace_root);

        Ok(state)
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
        let execution_settings = self.execution_settings_store.load_record().unwrap_or(
            harness_contracts::ExecutionDefaultsRecord {
                permission_mode: PermissionMode::Default,
                tool_profile: ToolProfile::Full,
                context_compression_trigger_ratio: default_context_compression_trigger_ratio(),
                subagents_enabled: false,
                agent_teams_enabled: false,
                background_agents_enabled: false,
            },
        );
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
    let layout = project_runtime_layout(&workspace_root);
    let (harness, model_id, protocol) = build_desktop_harness(
        &layout,
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

pub(crate) fn project_runtime_layout(workspace_root: &Path) -> RuntimeLayout {
    let home = jyowo_home_dir();
    let storage =
        crate::storage_layout::StorageLayout::new(crate::storage_layout::JyowoHome::new(home));
    storage.runtime_layout_for_project(workspace_root)
}

fn storage_layout_for_home() -> crate::storage_layout::StorageLayout {
    let home = jyowo_home_dir();
    crate::storage_layout::StorageLayout::new(crate::storage_layout::JyowoHome::new(home))
}

fn global_config_store_for_home() -> GlobalConfigStore {
    GlobalConfigStore::new(storage_layout_for_home())
}

fn project_config_store_for_workspace(workspace_root: &Path) -> ProjectConfigStore {
    ProjectConfigStore::new(storage_layout_for_home(), workspace_root.to_path_buf())
}

fn jyowo_home_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .unwrap_or_else(|| std::ffi::OsString::from("."));
    PathBuf::from(home).join(".jyowo")
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
    layout: &RuntimeLayout,
    stream_permission_runtime: Arc<StreamPermissionRuntime>,
    model_config_id: Option<&str>,
    provider_capability_routes: Arc<ParkingRwLock<ProviderCapabilityRouteSettings>>,
) -> Result<(Harness, String, ModelProtocol), CommandErrorPayload> {
    let workspace_root = layout.workspace_root.as_deref().ok_or_else(|| {
        runtime_init_failed("build_desktop_harness requires a project runtime layout".to_owned())
    })?;
    let runtime_root = &layout.runtime_root;

    reset_legacy_conversation_runtime_for_provider_continuations(workspace_root)?;
    let provider_continuation_store = Arc::new(
        FileProviderContinuationStore::open_runtime_dir(runtime_root).map_err(|error| {
            runtime_init_failed(format!(
                "provider continuation store initialization failed: {error}"
            ))
        })?,
    );
    let event_store: Arc<dyn EventStore> = Arc::new(
        JsonlEventStore::open(
            runtime_root.join("events"),
            Arc::new(DefaultRedactor::default()),
        )
        .await
        .map_err(|error| {
            runtime_init_failed(format!("event store initialization failed: {error}"))
        })?,
    );
    let mcp_server_store = DesktopMcpServerStore::new(workspace_root.to_path_buf());
    let mcp_diagnostic_store: Arc<dyn McpDiagnosticStore> =
        Arc::new(DesktopMcpDiagnosticStore::new(workspace_root.to_path_buf()));
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
    let blob_store: Arc<dyn harness_contracts::BlobStore> = Arc::new(
        FileBlobStore::open(runtime_root.join("blobs")).map_err(|error| {
            runtime_init_failed(format!("blob store initialization failed: {error}"))
        })?,
    );
    let evidence_registry = Arc::new(
        jyowo_harness_sdk::SqliteEvidenceRefRegistry::open(
            runtime_root.join("conversation-read-model.sqlite"),
        )
        .await
        .map_err(|error| {
            runtime_init_failed(format!("evidence registry initialization failed: {error}"))
        })?,
    );
    let evidence_ref_store = Arc::new(jyowo_harness_sdk::EvidenceRefStore::new_with_event_store(
        evidence_registry,
        Arc::clone(&blob_store),
        Arc::clone(&event_store),
    ));
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

    // Build the production PermissionAuthority with signed file persistence.
    let signer = desktop_integrity_signer(workspace_root)?;
    let decision_path = runtime_root.join("permission-decisions.json");
    let file_persistence: Arc<dyn harness_permission::DecisionStore> = Arc::new(
        FileDecisionPersistence::new(TenantId::SINGLE, decision_path, signer),
    );

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
        .with_interactive_broker(stream_permission_runtime.broker())
        .with_decision_store(Arc::clone(&file_persistence))
        .build()
        .map_err(|error| {
            runtime_init_failed(format!(
                "permission authority initialization failed: {error}"
            ))
        })?;

    let event_sink: Arc<dyn AuthorizationEventSink> = Arc::new(DesktopAuthorizationEventSink {
        event_store: Arc::clone(&event_store),
    });
    let authorization_service = Arc::new(AuthorizationService::new(
        Arc::new(permission_authority),
        Arc::clone(&sandbox),
        event_sink,
        Arc::new(TicketLedger::default()),
    ));
    let mcp_config = mcp_config_from_records(
        mcp_server_store.load_records()?,
        SessionId::new(),
        AgentId::new(),
        Arc::clone(&mcp_diagnostic_store),
        Arc::clone(&authorization_service),
        workspace_root,
    )
    .await?;

    let diagnostics_runner: Arc<dyn DiagnosticsRunnerCap> =
        Arc::new(DesktopDiagnosticsRunner::new(Arc::clone(&sandbox)));
    let background_agent_starter: Arc<dyn BackgroundAgentStarterCap> =
        Arc::new(DesktopBackgroundAgentStarter {
            workspace_root: workspace_root.to_path_buf(),
            event_store: Arc::clone(&event_store),
        });
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
        .with_store_arc(event_store)
        .with_sandbox_arc(sandbox)
        .with_blob_store_arc(blob_store)
        .with_evidence_ref_store_arc(evidence_ref_store)
        .with_provider_continuation_store_arc(provider_continuation_store)
        .with_capability(
            ToolCapability::ProviderCredentialResolver,
            provider_credential_resolver,
        )
        .with_capability(
            ToolCapability::Custom("diagnostics_runner".to_owned()),
            diagnostics_runner,
        )
        .with_capability(
            ToolCapability::Custom("jyowo.background_agent.starter".to_owned()),
            background_agent_starter,
        )
        .with_mcp_config(mcp_config)
        .with_plugin_registry(plugin_registry)
        .with_memory_provider(
            harness_memory::local::LocalMemoryProvider::open(
                &runtime_root
                    .join("memory")
                    .join("memory.sqlite3")
                    .to_string_lossy(),
                TenantId::SINGLE,
            )
            .map_err(|e| {
                runtime_init_failed(format!("memory provider initialization failed: {e}"))
            })?,
        )
        .with_skill_loader(skill_loader)
        .with_permission_authority_arc(authorization_service.permission_authority())
        .with_authorization_service_arc(authorization_service)
        .with_stream_permission_broker_arc(
            stream_permission_runtime.broker(),
            stream_permission_runtime.resolver_handle(),
        )
        .build()
        .await
        .map_err(|error| runtime_init_failed(format!("harness initialization failed: {error}")))?;

    Ok((harness, model_id, protocol))
}

pub fn reset_legacy_conversation_runtime_for_provider_continuations(
    workspace_root: &Path,
) -> Result<(), CommandErrorPayload> {
    let runtime_dir = workspace_root.join(".jyowo").join("runtime");
    let marker_path = runtime_dir.join("provider-continuation-runtime.version");

    super::stores::ensure_no_symlink_components(&runtime_dir, "provider continuation runtime")?;
    super::stores::ensure_no_symlink_components(
        &marker_path,
        "provider continuation runtime marker",
    )?;

    if std::fs::read_to_string(&marker_path)
        .map(|value| value.trim() == PROVIDER_CONTINUATION_RUNTIME_VERSION)
        .unwrap_or(false)
    {
        return Ok(());
    }

    for target in provider_continuation_dev_reset_targets(&runtime_dir) {
        remove_provider_continuation_dev_reset_target(&target)?;
    }

    super::stores::ensure_app_dir_no_symlink(
        &runtime_dir.join("events"),
        "provider continuation runtime events directory",
    )?;
    super::stores::ensure_app_dir_no_symlink(
        &runtime_dir.join("sessions"),
        "provider continuation runtime sessions directory",
    )?;

    super::stores::write_atomic_runtime_file(
        &marker_path,
        "provider-continuation-runtime.version",
        "provider continuation runtime marker",
        format!("{PROVIDER_CONTINUATION_RUNTIME_VERSION}\n").as_bytes(),
    )?;

    Ok(())
}

fn provider_continuation_dev_reset_targets(runtime_dir: &Path) -> Vec<PathBuf> {
    vec![
        runtime_dir.join("events"),
        runtime_dir.join("sessions"),
        runtime_dir.join("conversation-read-model.sqlite"),
        runtime_dir.join("conversation-read-model.sqlite-shm"),
        runtime_dir.join("conversation-read-model.sqlite-wal"),
        runtime_dir.join("conversation-metadata.json"),
        runtime_dir.join("provider-continuations.jsonl"),
    ]
}

fn remove_provider_continuation_dev_reset_target(target: &Path) -> Result<(), CommandErrorPayload> {
    super::stores::ensure_no_symlink_components(target, "provider continuation reset target")?;
    let metadata = match std::fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(runtime_init_failed(format!(
                "provider continuation reset target metadata failed: {error}"
            )));
        }
    };

    if metadata.is_dir() {
        std::fs::remove_dir_all(target).map_err(|error| {
            runtime_init_failed(format!(
                "provider continuation reset directory removal failed: {error}"
            ))
        })?;
    } else {
        std::fs::remove_file(target).map_err(|error| {
            runtime_init_failed(format!(
                "provider continuation reset file removal failed: {error}"
            ))
        })?;
    }

    Ok(())
}

fn desktop_integrity_signer(
    workspace_root: &Path,
) -> Result<Arc<dyn harness_permission::IntegritySigner>, CommandErrorPayload> {
    let key = desktop_integrity_key(workspace_root)?;
    harness_permission::StaticSignerStore::from_key(
        "desktop-integrity",
        key,
        IntegrityAlgorithm::HmacSha256,
    )
    .map_err(|error| {
        runtime_init_failed(format!("integrity signer initialization failed: {error}"))
    })
}

fn desktop_integrity_key(workspace_root: &Path) -> Result<Vec<u8>, CommandErrorPayload> {
    let path = workspace_root
        .join(".jyowo")
        .join("runtime")
        .join("permission-integrity.key");
    ensure_no_symlink_components(&path, "integrity key file")?;
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => {
            let raw = read_owner_only_file(&path, "integrity key")?;
            let key = general_purpose::STANDARD
                .decode(raw.trim())
                .map_err(|error| {
                    runtime_init_failed(format!("integrity key decode failed: {error}"))
                })?;
            if key.len() == 32 {
                return Ok(key);
            }
            return Err(runtime_init_failed(
                "integrity key has invalid length".to_owned(),
            ));
        }
        Ok(_) => {
            return Err(runtime_init_failed(
                "integrity key path is not a file".to_owned(),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(runtime_init_failed(format!(
                "integrity key metadata failed: {error}"
            )));
        }
    }

    let mut key = Vec::with_capacity(32);
    key.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    key.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    super::stores::write_atomic_runtime_file(
        &path,
        "permission-integrity.key",
        "integrity key",
        general_purpose::STANDARD.encode(&key).as_bytes(),
    )?;
    Ok(key)
}

#[cfg(unix)]
fn set_owner_only_open_runtime_file(
    file: &std::fs::File,
    label: &str,
) -> Result<(), CommandErrorPayload> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|error| runtime_init_failed(format!("{label} permission update failed: {error}")))
}

#[cfg(not(unix))]
fn set_owner_only_open_runtime_file(
    _file: &std::fs::File,
    _label: &str,
) -> Result<(), CommandErrorPayload> {
    Ok(())
}

#[cfg(unix)]
fn open_runtime_file_no_follow(
    path: &Path,
    label: &str,
) -> Result<std::fs::File, CommandErrorPayload> {
    let mut components = Vec::new();
    let mut absolute = false;
    for component in path.components() {
        match component {
            std::path::Component::Prefix(_) => {
                return Err(runtime_init_failed(format!(
                    "{label} has unsupported path prefix"
                )));
            }
            std::path::Component::RootDir => absolute = true,
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                return Err(runtime_init_failed(format!(
                    "{label} must not use parent directory components"
                )));
            }
            std::path::Component::Normal(value) => components.push(value.to_os_string()),
        }
    }
    let file_name = components
        .pop()
        .ok_or_else(|| runtime_init_failed(format!("{label} path has no file name")))?;
    let mut directory = if absolute {
        std::fs::File::open(Path::new("/"))
            .map_err(|error| runtime_init_failed(format!("{label} root open failed: {error}")))?
    } else {
        std::fs::File::open(Path::new(".")).map_err(|error| {
            runtime_init_failed(format!("{label} current directory open failed: {error}"))
        })?
    };

    for component in components {
        let fd = rustix::fs::openat(
            &directory,
            Path::new(&component),
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_raw_mode(0),
        )
        .map_err(|error| {
            if error == rustix::io::Errno::LOOP || error == rustix::io::Errno::NOTDIR {
                runtime_init_failed(format!("{label} must not use symlinks"))
            } else {
                runtime_init_failed(format!("{label} directory open failed: {error}"))
            }
        })?;
        directory = std::fs::File::from(fd);
    }

    match rustix::fs::openat(
        &directory,
        Path::new(&file_name),
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::from_raw_mode(0),
    ) {
        Ok(fd) => Ok(std::fs::File::from(fd)),
        Err(rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) => Err(runtime_init_failed(
            format!("{label} must not use symlinks"),
        )),
        Err(error) => Err(runtime_init_failed(format!("{label} open failed: {error}"))),
    }
}

#[cfg(not(unix))]
fn open_runtime_file_no_follow(
    path: &Path,
    label: &str,
) -> Result<std::fs::File, CommandErrorPayload> {
    std::fs::File::open(path)
        .map_err(|error| runtime_init_failed(format!("{label} open failed: {error}")))
}

fn read_owner_only_file(path: &Path, label: &str) -> Result<String, CommandErrorPayload> {
    let mut file = open_runtime_file_no_follow(path, label)?;
    set_owner_only_open_runtime_file(&file, label)?;
    let mut raw = String::new();
    std::io::Read::read_to_string(&mut file, &mut raw)
        .map_err(|error| runtime_init_failed(format!("{label} read failed: {error}")))?;
    Ok(raw)
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
    let layout = project_runtime_layout(&state.workspace_root);
    let (harness, model_id, protocol) = build_desktop_harness(
        &layout,
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

#[cfg(test)]
mod tests {
    use super::*;
    use harness_contracts::{
        AgentToolPolicy, AgentWorkspaceIsolationMode, BackgroundAgentToolSessionSnapshot,
        ToolSearchMode,
    };
    use jyowo_harness_sdk::testing::{InMemoryEventStore, NoopRedactor};

    #[cfg(unix)]
    #[test]
    fn desktop_integrity_key_is_created_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let workspace = tempfile::tempdir().expect("temp workspace");
        let workspace_root = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");
        let key = desktop_integrity_key(&workspace_root).expect("integrity key");

        assert_eq!(key.len(), 32);
        let key_path = workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("permission-integrity.key");
        let mode = std::fs::metadata(key_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn desktop_integrity_key_rejects_symlink_parent() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let workspace_root = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");
        let external = tempfile::tempdir().expect("external tempdir");
        std::os::unix::fs::symlink(external.path(), workspace_root.join(".jyowo"))
            .expect("symlink");

        let error = desktop_integrity_key(&workspace_root).expect_err("symlink should fail");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(error.message.contains("symlink"));
        assert!(!external.path().join("runtime").exists());
    }

    #[cfg(unix)]
    #[test]
    fn desktop_integrity_key_rejects_symlink_key_file() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let workspace_root = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");
        let external = tempfile::NamedTempFile::new().expect("external file");
        let runtime_dir = workspace_root.join(".jyowo").join("runtime");
        std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
        std::os::unix::fs::symlink(
            external.path(),
            runtime_dir.join("permission-integrity.key"),
        )
        .expect("symlink");

        let error = desktop_integrity_key(&workspace_root).expect_err("symlink should fail");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(error.message.contains("symlink"));
    }

    #[cfg(unix)]
    #[test]
    fn desktop_integrity_key_tightens_existing_owner_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let workspace = tempfile::tempdir().expect("temp workspace");
        let workspace_root = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");
        let runtime_dir = workspace_root.join(".jyowo").join("runtime");
        std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
        let key_path = runtime_dir.join("permission-integrity.key");
        let key = [7_u8; 32];
        std::fs::write(&key_path, general_purpose::STANDARD.encode(key)).expect("write key");
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644))
            .expect("widen key mode");

        let loaded = desktop_integrity_key(&workspace_root).expect("integrity key");

        assert_eq!(loaded, key);
        let mode = std::fs::metadata(key_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn provider_continuation_runtime_marker_is_created_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let workspace = tempfile::tempdir().expect("temp workspace");
        let workspace_root = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");

        reset_legacy_conversation_runtime_for_provider_continuations(&workspace_root)
            .expect("reset should succeed");

        let marker_path = workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("provider-continuation-runtime.version");
        assert_eq!(
            std::fs::read_to_string(&marker_path).unwrap().trim(),
            PROVIDER_CONTINUATION_RUNTIME_VERSION
        );
        let mode = std::fs::metadata(marker_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn provider_continuation_runtime_rejects_symlink_runtime_parent() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let workspace_root = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");
        let external = tempfile::tempdir().expect("external tempdir");
        std::os::unix::fs::symlink(external.path(), workspace_root.join(".jyowo"))
            .expect("symlink");

        let error = reset_legacy_conversation_runtime_for_provider_continuations(&workspace_root)
            .expect_err("symlink runtime parent should fail");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(error.message.contains("symlink"));
        assert!(!external.path().join("runtime").exists());
    }

    #[cfg(unix)]
    #[test]
    fn provider_continuation_runtime_rejects_symlink_marker_file() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let workspace_root = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");
        let external = tempfile::NamedTempFile::new().expect("external file");
        let runtime_dir = workspace_root.join(".jyowo").join("runtime");
        std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
        std::os::unix::fs::symlink(
            external.path(),
            runtime_dir.join("provider-continuation-runtime.version"),
        )
        .expect("symlink");

        let error = reset_legacy_conversation_runtime_for_provider_continuations(&workspace_root)
            .expect_err("symlink marker should fail");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(error.message.contains("symlink"));
        assert!(external.path().exists());
    }

    #[tokio::test]
    async fn background_agent_starter_rejects_when_settings_disable_capability() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let workspace_root = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");
        let starter = DesktopBackgroundAgentStarter {
            workspace_root: workspace_root.clone(),
            event_store: Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))),
        };
        let conversation_id = SessionId::new();

        let error = starter
            .start_background_agent(BackgroundAgentToolStartRequest {
                tenant_id: TenantId::SINGLE,
                conversation_id,
                parent_run_id: RunId::new(),
                tool_use_id: ToolUseId::new(),
                goal: "start hidden background work".to_owned(),
                title: "hidden background work".to_owned(),
                model_config_id: Some("test-model-config".to_owned()),
                permission_mode: PermissionMode::Default,
                agent_tool_policy: AgentToolPolicy {
                    subagents: AgentUsePolicy::Off,
                    agent_team: AgentUsePolicy::Off,
                    background_agents: AgentUsePolicy::Allowed,
                    team_config: None,
                    workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
                    max_depth: 1,
                    max_concurrent_subagents: 1,
                    max_team_members: 1,
                },
                session: BackgroundAgentToolSessionSnapshot {
                    tenant_id: TenantId::SINGLE,
                    session_id: conversation_id,
                    tool_search: ToolSearchMode::Disabled,
                    tool_profile: ToolProfile::Minimal,
                    permission_mode: PermissionMode::Default,
                    interactivity: InteractivityLevel::NoInteractive,
                    team_id: None,
                    max_iterations: 0,
                    context_compression_trigger_ratio: 0.8,
                },
            })
            .await
            .expect_err("settings off must reject direct starter capability use");

        assert!(
            matches!(error, ToolError::Validation(ref message) if message.contains("backgroundAgents")),
            "unexpected error: {error:?}"
        );
        let store = AgentRuntimeStore::open(&workspace_root).expect("runtime store opens");
        assert!(
            store
                .list_background_agents(true)
                .expect("background records list")
                .is_empty(),
            "policy rejection must happen before creating a background record"
        );
    }
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
    fn resolve_permission_option<'a>(
        &'a self,
        request_id: RequestId,
        tenant_id: TenantId,
        session_id: SessionId,
        option_id: PermissionOptionId,
        submitted_decision: Decision,
        confirmation_text: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<Decision, CommandErrorPayload>> + Send + 'a>> {
        Box::pin(async move {
            self.resolve_permission_option(
                request_id,
                tenant_id,
                session_id,
                option_id,
                submitted_decision,
                confirmation_text,
            )
            .await
            .map_err(|error| CommandErrorPayload {
                code: "PERMISSION_RESOLVE_FAILED",
                message: error.to_string(),
            })
        })
    }
}

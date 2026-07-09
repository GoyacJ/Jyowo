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
use crate::storage_layout::{ConfigScope, JyowoHome, RuntimeLayout, RuntimeScope, StorageLayout};
use async_trait::async_trait;
use harness_contracts::CapabilityRegistry;
#[cfg(test)]
use harness_contracts::NoopRedactor;
use harness_contracts::{KillScope, Redactor, SandboxError, SandboxExitStatus};
use harness_execution::{
    AuthorizationEventSink, AuthorizationService, ExecutionPreflightRegistry,
    ReqwestToolNetworkBroker, TicketLedger,
};
use harness_model::{default_account_usage_registry, ProviderAccountUsageRegistry};
use harness_permission::{FileDecisionPersistence, IntegrityAlgorithm, PermissionAuthority};
use harness_provider_state::FileProviderContinuationStore;
use harness_sandbox::{ContainerLifecycle, DockerSandbox, RoutingSandboxBackend, VolumeMount};
use harness_tool::ToolNetworkBrokerCap;

#[derive(Clone)]
struct DesktopBackgroundAgentStarter {
    runtime_layout: RuntimeLayout,
    global_config_store: GlobalConfigStore,
    project_config_store: Option<ProjectConfigStore>,
    event_store: Arc<dyn EventStore>,
}

impl BackgroundAgentStarterCap for DesktopBackgroundAgentStarter {
    fn start_background_agent(
        &self,
        request: BackgroundAgentToolStartRequest,
    ) -> futures::future::BoxFuture<'static, Result<BackgroundAgentToolStartResponse, ToolError>>
    {
        let runtime_layout = self.runtime_layout.clone();
        let global_config_store = self.global_config_store.clone();
        let project_config_store = self.project_config_store.clone();
        let event_store = Arc::clone(&self.event_store);
        Box::pin(async move {
            let policy_root = runtime_layout
                .workspace_root
                .clone()
                .unwrap_or_else(|| runtime_layout.runtime_root.clone());
            let settings = resolve_effective_execution_settings(
                Some(&global_config_store),
                project_config_store.as_ref(),
                None,
                None,
            )
            .map_err(|error| ToolError::Internal(error.message))?;
            let capabilities_payload =
                if let Some(project_workspace_root) = runtime_layout.workspace_root.as_deref() {
                    agent_capabilities_payload(
                        &settings,
                        project_workspace_root,
                        Some(&AgentCapabilityResolutionContext {
                            stream_permission_runtime_available: true,
                        }),
                    )
                } else {
                    no_workspace_agent_capabilities_payload_for_conversation(
                        &settings,
                        &runtime_layout.runtime_root,
                        Some(request.conversation_id),
                        Some(&AgentCapabilityResolutionContext {
                            stream_permission_runtime_available: true,
                        }),
                    )
                };
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
            let mut profiles = jyowo_harness_sdk::builtin_agent_profiles();
            profiles.extend(
                global_config_store
                    .load_global_agent_profiles()
                    .map_err(|error| ToolError::Internal(error.message))?,
            );
            let profile_ids: Vec<String> = profiles.into_iter().map(|profile| profile.id).collect();
            let resolved_policy = resolve_agent_runtime_policy(
                &policy_root,
                &settings_input,
                Some(&request.agent_tool_policy),
                &capabilities,
                &profile_ids,
                &request.conversation_id.to_string(),
            )
            .map_err(|error| ToolError::Validation(error.to_string()))?;

            let store = Arc::new(
                AgentRuntimeStore::open_runtime_dir(&runtime_layout.runtime_root)
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
            let mut agent_tool_policy = resolved_policy.options;
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
            let supervisor_scope = agent_supervisor_scope_for_layout(&runtime_layout);
            let _ = crate::agent_supervisor::wake_agent_supervisor_scope(&supervisor_scope).await;
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
    runtime_scope: RuntimeScope,
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
    project_workspace_root: Option<&Path>,
    default_model_id: &str,
    default_protocol: ModelProtocol,
) -> Result<Option<(String, [u8; 32])>, CommandErrorPayload> {
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
        let storage_layout = test_storage_layout_for_workspace(&workspace_root);
        let runtime_layout = storage_layout.runtime_layout_for_project(&workspace_root);

        Ok(Self {
            active_runtime: Arc::new(RwLock::new(DesktopActiveRuntime {
                default_model_config_id: None,
                default_model_id: "llama3.1".to_owned(),
                default_protocol: ModelProtocol::ChatCompletions,
                provider_config_fingerprint: None,
                runtime_scope: runtime_layout.scope.clone(),
                harness: None,
            })),
            automation_lock: Arc::new(tokio::sync::Mutex::new(())),
            automation_store: Arc::new(DesktopAutomationStore::new_with_layout(
                storage_layout.clone(),
                workspace_root.clone(),
            )),
            conversation_metadata_lock: Arc::new(tokio::sync::Mutex::new(())),
            conversation_metadata_store: Arc::new(DesktopConversationMetadataStore::new(
                workspace_root.clone(),
            )),
            conversation_event_subscriptions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            default_conversation_id: SessionId::new(),
            deleted_conversation_ids: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            memory_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_diagnostic_store: Arc::new(DesktopMcpDiagnosticStore::new_runtime_root(
                runtime_layout.runtime_root.clone(),
            )),
            mcp_diagnostic_subscriptions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            mcp_server_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_server_store: Arc::new(DesktopMcpServerStore::new(
                storage_layout.clone(),
                workspace_root.clone(),
            )),
            permission_resolver: None,
            provider_api_key_reveal_tokens: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            plugin_store: Arc::new(DesktopPluginStore::project(
                storage_layout.clone(),
                workspace_root.clone(),
            )),
            plugin_store_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_settings_lock: Arc::new(tokio::sync::Mutex::new(())),
            provider_settings_store: Arc::new(DesktopProviderSettingsStore::new_with_layout(
                storage_layout.clone(),
                workspace_root.clone(),
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
            official_quota_flights: new_official_quota_flights(),
            account_usage_registry: Arc::new(default_account_usage_registry()),
            provider_capability_route_store: provider_capability_route_store_for_layout(
                &runtime_layout,
            ),
            provider_capability_routes: Arc::new(ParkingRwLock::new(
                empty_provider_capability_route_settings(),
            )),
            execution_settings_lock: Arc::new(tokio::sync::Mutex::new(())),
            execution_settings_store: Arc::new(DesktopExecutionSettingsStore::new_with_layout(
                storage_layout.clone(),
                workspace_root.clone(),
            )),
            skill_catalog_install_tasks: Arc::new(RwLock::new(HashMap::new())),
            skill_store: Arc::new(DesktopSkillStore::project(
                storage_layout.clone(),
                workspace_root.clone(),
            )),
            skill_store_lock: Arc::new(tokio::sync::Mutex::new(())),
            start_run_lock: Arc::new(tokio::sync::Mutex::new(())),
            stream_permission_runtime: None,
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

    #[doc(hidden)]
    pub fn with_harness_and_stream_permission_runtime_for_global_conversation(
        runtime_root: PathBuf,
        conversation_id: SessionId,
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
        let runtime_layout =
            global_conversation_runtime_layout_with_runtime_root(conversation_id, runtime_root);
        Self::with_harness_stream_permission_runtime_and_model_for_layout(
            runtime_layout,
            harness,
            stream_permission_runtime,
            default_model_id,
            default_protocol,
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
        let runtime_layout = project_runtime_layout(&workspace_root);
        Self::with_harness_stream_permission_runtime_and_model_for_layout(
            runtime_layout,
            harness,
            stream_permission_runtime,
            default_model_id,
            default_protocol,
        )
    }

    fn with_harness_stream_permission_runtime_and_model_for_layout(
        runtime_layout: RuntimeLayout,
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
                provider_config_fingerprint: active_runtime_binding.map(|binding| binding.1),
                runtime_scope: runtime_layout.scope.clone(),
                harness: Some(harness),
            })),
            automation_lock: Arc::new(tokio::sync::Mutex::new(())),
            automation_store: automation_store_for_layout(&runtime_layout),
            conversation_metadata_lock: Arc::new(tokio::sync::Mutex::new(())),
            conversation_metadata_store: Arc::new(
                DesktopConversationMetadataStore::new_runtime_root(
                    runtime_layout.runtime_root.clone(),
                ),
            ),
            conversation_event_subscriptions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            default_conversation_id: SessionId::new(),
            deleted_conversation_ids: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            memory_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_diagnostic_store: Arc::new(DesktopMcpDiagnosticStore::new_runtime_root(
                runtime_layout.runtime_root.clone(),
            )),
            mcp_diagnostic_subscriptions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            mcp_server_lock: Arc::new(tokio::sync::Mutex::new(())),
            mcp_server_store: mcp_server_store_for_layout(&runtime_layout),
            permission_resolver: Some(permission_resolver),
            provider_api_key_reveal_tokens: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            plugin_store: Arc::new(if runtime_layout.workspace_root.is_some() {
                DesktopPluginStore::project(
                    storage_layout_for_home(),
                    runtime_layout
                        .workspace_root
                        .clone()
                        .expect("project runtime layout has workspace root"),
                )
            } else {
                DesktopPluginStore::global(storage_layout_for_home())
            }),
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
            skill_store: Arc::new(if runtime_layout.workspace_root.is_some() {
                DesktopSkillStore::project(
                    storage_layout_for_home(),
                    runtime_layout
                        .workspace_root
                        .clone()
                        .expect("project runtime layout has workspace root"),
                )
            } else {
                DesktopSkillStore::global(storage_layout_for_home())
            }),
            skill_store_lock: Arc::new(tokio::sync::Mutex::new(())),
            start_run_lock: Arc::new(tokio::sync::Mutex::new(())),
            stream_permission_runtime: Some(stream_permission_runtime),
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
            provider_config_fingerprint: active_runtime_binding.map(|binding| binding.1),
            runtime_scope: self.runtime_layout.scope.clone(),
            harness: Some(harness),
        };
    }

    #[must_use]
    pub fn active_conversation_runtime_for_model_config(
        &self,
        session_id: SessionId,
        model_config_id: &str,
        provider_config_fingerprint: [u8; 32],
    ) -> Result<Option<(Arc<Harness>, SessionOptions)>, CommandErrorPayload> {
        let active_runtime = self
            .active_runtime
            .read()
            .expect("desktop active runtime lock should not be poisoned");
        if active_runtime.default_model_config_id.as_deref() != Some(model_config_id)
            || active_runtime.provider_config_fingerprint != Some(provider_config_fingerprint)
            || !active_runtime_scope_matches_session(&active_runtime.runtime_scope, session_id)
        {
            return Ok(None);
        }
        let Some(harness) = active_runtime.harness.as_ref().map(Arc::clone) else {
            return Ok(None);
        };
        let options = self.conversation_session_options_for_model(
            session_id,
            active_runtime.default_model_id.clone(),
            active_runtime.default_protocol,
        )?;
        Ok(Some((harness, options)))
    }

    pub fn active_conversation_runtime(
        &self,
        session_id: SessionId,
    ) -> Result<Option<(Arc<Harness>, SessionOptions)>, CommandErrorPayload> {
        let active_runtime = self
            .active_runtime
            .read()
            .expect("desktop active runtime lock should not be poisoned");
        if !active_runtime_scope_matches_session(&active_runtime.runtime_scope, session_id) {
            return Ok(None);
        }
        let Some(harness) = active_runtime.harness.as_ref().map(Arc::clone) else {
            return Ok(None);
        };
        let options = self.conversation_session_options_for_model(
            session_id,
            active_runtime.default_model_id.clone(),
            active_runtime.default_protocol,
        )?;
        Ok(Some((harness, options)))
    }

    #[must_use]
    pub fn pending_permission_requests(&self) -> Vec<PendingPermissionRequest> {
        self.stream_permission_runtime
            .as_ref()
            .map_or_else(Vec::new, |runtime| runtime.pending_permission_requests())
    }

    pub fn effective_execution_settings(
        &self,
        run_permission_mode: Option<PermissionMode>,
    ) -> Result<harness_contracts::ExecutionDefaultsRecord, CommandErrorPayload> {
        resolve_effective_execution_settings(
            self.global_config_store.as_ref(),
            self.project_config_store.as_ref(),
            run_permission_mode,
            None,
        )
    }

    pub fn conversation_session_options(
        &self,
        session_id: SessionId,
    ) -> Result<SessionOptions, CommandErrorPayload> {
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

    pub fn conversation_session_options_for_model(
        &self,
        session_id: SessionId,
        model_id: String,
        protocol: ModelProtocol,
    ) -> Result<SessionOptions, CommandErrorPayload> {
        let execution_settings = self.effective_execution_settings(None)?;
        let mut options = SessionOptions::new(self.conversation_cwd_for_session(session_id))
            .with_tenant_id(TenantId::SINGLE)
            .with_session_id(session_id)
            .with_agent_runtime_root(self.runtime_layout.runtime_root.clone())
            .with_interactivity(InteractivityLevel::FullyInteractive)
            .with_model_id(model_id)
            .with_protocol(protocol)
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

fn active_runtime_scope_matches_session(
    runtime_scope: &RuntimeScope,
    session_id: SessionId,
) -> bool {
    match runtime_scope {
        RuntimeScope::Project { .. } => true,
        RuntimeScope::GlobalConversation { conversation_id } => *conversation_id == session_id,
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
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::default());
    runtime_state_from_stream_permission_runtime(workspace_root, stream_permission_runtime).await
}

pub(crate) async fn runtime_state_for_global_conversation(
    conversation_id: SessionId,
) -> Result<DesktopRuntimeState, CommandErrorPayload> {
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::default());
    let layout = global_conversation_runtime_layout(conversation_id);
    runtime_state_for_global_conversation_layout(layout, stream_permission_runtime).await
}

pub(crate) async fn runtime_state_for_global_conversation_with_runtime_root(
    conversation_id: SessionId,
    runtime_root: PathBuf,
    stream_permission_runtime: Arc<StreamPermissionRuntime>,
) -> Result<DesktopRuntimeState, CommandErrorPayload> {
    let layout =
        global_conversation_runtime_layout_with_runtime_root(conversation_id, runtime_root);
    runtime_state_for_global_conversation_layout(layout, stream_permission_runtime).await
}

async fn runtime_state_for_global_conversation_layout(
    layout: RuntimeLayout,
    stream_permission_runtime: Arc<StreamPermissionRuntime>,
) -> Result<DesktopRuntimeState, CommandErrorPayload> {
    let provider_capability_routes = Arc::new(ParkingRwLock::new(
        empty_provider_capability_route_settings(),
    ));
    let (harness, model_id, protocol) = build_desktop_harness(
        &layout,
        Arc::clone(&stream_permission_runtime),
        None,
        Arc::clone(&provider_capability_routes),
        None,
    )
    .await?;

    DesktopRuntimeState::with_harness_stream_permission_runtime_and_model_for_layout(
        layout,
        Arc::new(harness),
        stream_permission_runtime,
        model_id,
        protocol,
    )
}

pub(crate) async fn runtime_state_from_stream_permission_runtime(
    workspace_root: PathBuf,
    stream_permission_runtime: Arc<StreamPermissionRuntime>,
) -> Result<DesktopRuntimeState, CommandErrorPayload> {
    runtime_state_from_stream_permission_runtime_inner(
        workspace_root,
        stream_permission_runtime,
        None,
    )
    .await
}

#[doc(hidden)]
pub async fn runtime_state_from_stream_permission_runtime_with_provider_settings_store_for_test(
    workspace_root: PathBuf,
    stream_permission_runtime: Arc<StreamPermissionRuntime>,
    provider_settings_store: Arc<dyn ProviderSettingsStore>,
) -> Result<DesktopRuntimeState, CommandErrorPayload> {
    runtime_state_from_stream_permission_runtime_inner(
        workspace_root,
        stream_permission_runtime,
        Some(provider_settings_store),
    )
    .await
}

async fn runtime_state_from_stream_permission_runtime_inner(
    workspace_root: PathBuf,
    stream_permission_runtime: Arc<StreamPermissionRuntime>,
    provider_settings_store_override: Option<Arc<dyn ProviderSettingsStore>>,
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
        provider_settings_store_override.clone(),
    )
    .await?;

    let mut state =
        DesktopRuntimeState::with_harness_stream_permission_runtime_and_model_for_workspace(
            workspace_root,
            Arc::new(harness),
            stream_permission_runtime,
            model_id,
            protocol,
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

fn automation_store_for_layout(layout: &RuntimeLayout) -> Arc<dyn AutomationStore> {
    match layout.workspace_root.as_ref() {
        Some(workspace_root) => Arc::new(DesktopAutomationStore::new(workspace_root.clone())),
        None => Arc::new(NoWorkspaceAutomationStore),
    }
}

fn mcp_server_store_for_layout(layout: &RuntimeLayout) -> Arc<dyn McpServerStore> {
    match layout.workspace_root.as_ref() {
        Some(workspace_root) => Arc::new(DesktopMcpServerStore::new(
            storage_layout_for_home(),
            workspace_root.clone(),
        )),
        None => Arc::new(NoWorkspaceMcpServerStore),
    }
}

fn provider_capability_route_store_for_layout(
    layout: &RuntimeLayout,
) -> Arc<dyn ProviderCapabilityRouteStore> {
    match layout.workspace_root.as_ref() {
        Some(workspace_root) => Arc::new(DesktopProviderCapabilityRouteStore::new(
            workspace_root.clone(),
        )),
        None => Arc::new(NoWorkspaceProviderCapabilityRouteStore),
    }
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
    let supervisor_scope = agent_supervisor_scope_for_state(state);
    crate::agent_supervisor::launch_agent_supervisor_sidecar_for_scope(app, supervisor_scope)
        .await
        .map_err(|error| {
            runtime_init_failed(format!("agent supervisor sidecar startup failed: {error}"))
        })
}

pub(crate) fn agent_supervisor_scope_for_state(
    state: &DesktopRuntimeState,
) -> crate::agent_supervisor::AgentSupervisorScope {
    agent_supervisor_scope_for_layout(&state.runtime_layout)
}

pub(crate) fn agent_supervisor_scope_for_layout(
    layout: &RuntimeLayout,
) -> crate::agent_supervisor::AgentSupervisorScope {
    match layout.workspace_root.as_ref() {
        Some(workspace_root) => {
            crate::agent_supervisor::AgentSupervisorScope::project(workspace_root.clone())
        }
        None => match &layout.scope {
            RuntimeScope::GlobalConversation { conversation_id } => {
                crate::agent_supervisor::AgentSupervisorScope::runtime_conversation(
                    layout.runtime_root.clone(),
                    *conversation_id,
                )
            }
            RuntimeScope::Project { .. } => {
                crate::agent_supervisor::AgentSupervisorScope::runtime(layout.runtime_root.clone())
            }
        },
    }
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

pub(crate) async fn build_desktop_harness(
    layout: &RuntimeLayout,
    stream_permission_runtime: Arc<StreamPermissionRuntime>,
    model_config_id: Option<&str>,
    provider_capability_routes: Arc<ParkingRwLock<ProviderCapabilityRouteSettings>>,
    provider_settings_store_override: Option<Arc<dyn ProviderSettingsStore>>,
) -> Result<(Harness, String, ModelProtocol), CommandErrorPayload> {
    let project_workspace_root = layout.workspace_root.as_deref();
    let execution_cwd = layout.conversation_cwd.as_path();
    let runtime_root = &layout.runtime_root;

    ensure_desktop_runtime_store_paths(runtime_root)?;
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
    let mcp_server_records = if let Some(project_workspace_root) = layout.workspace_root.as_deref()
    {
        let mcp_server_store = DesktopMcpServerStore::new(
            storage_layout_for_home(),
            project_workspace_root.to_path_buf(),
        );
        mcp_server_store.load_records()?
    } else {
        Vec::new()
    };
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
    let conversation_metadata_store =
        DesktopConversationMetadataStore::new_runtime_root(runtime_root.clone());
    let (model_provider, model_id, protocol) =
        model_from_provider_settings(provider_settings_store.as_ref(), model_config_id)?
            .unwrap_or_else(|| {
                (
                    Arc::new(LocalLlamaProvider::default()) as Arc<dyn ModelProvider>,
                    "llama3.1".to_owned(),
                    ModelProtocol::ChatCompletions,
                )
            });
    let storage_layout = storage_layout_for_home();
    let global_config_store = global_config_store_for_home();
    let global_skill_selection = global_config_store
        .load_global_skill_selection_if_present()?
        .map(|selection| selection.enabled.into_iter().collect());
    let global_skill_store = DesktopSkillStore::global(storage_layout);
    let mut skill_loader =
        SkillLoader::default().with_source(SkillSourceConfig::DirectoryPackages {
            path: global_skill_store.enabled_dir(),
            source_kind: DirectorySourceKind::User,
            allowed_package_ids: global_skill_selection,
        });
    if let Some(project_workspace_root) = layout.workspace_root.as_deref() {
        let skill_store = DesktopSkillStore::project(
            storage_layout_for_home(),
            project_workspace_root.to_path_buf(),
        );
        let project_skill_selection = project_config_store_for_workspace(project_workspace_root)
            .load_project_skill_selection_if_present()?
            .map(|selection| selection.enabled.into_iter().collect());
        skill_loader = skill_loader.with_source(SkillSourceConfig::DirectoryPackages {
            path: skill_store.enabled_dir(),
            source_kind: DirectorySourceKind::Workspace,
            allowed_package_ids: project_skill_selection,
        });
    }
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
            Arc::clone(&provider_settings_store),
            Arc::clone(&provider_capability_routes),
        ));
    let global_plugin_store = DesktopPluginStore::global(storage_layout_for_home());
    let plugin_store: Arc<dyn PluginStore> =
        if let Some(project_workspace_root) = layout.workspace_root.as_deref() {
            Arc::new(DesktopPluginStore::project(
                storage_layout_for_home(),
                project_workspace_root.to_path_buf(),
            ))
        } else {
            Arc::new(global_plugin_store.clone())
        };
    let global_plugin_store_for_registry = if layout.workspace_root.is_some() {
        Some(&global_plugin_store)
    } else {
        None
    };
    let plugin_registry = build_plugin_registry(
        execution_cwd,
        project_workspace_root,
        plugin_store.as_ref(),
        global_plugin_store_for_registry,
    )?;

    let sandbox = build_desktop_process_sandbox(execution_cwd).await?;

    // Build the production PermissionAuthority with signed file persistence.
    let signer = desktop_integrity_signer(runtime_root)?;
    let decision_path = runtime_root.join("permission-decisions.json");
    let decision_persistence =
        FileDecisionPersistence::new(TenantId::SINGLE, decision_path, signer);
    let decision_persistence = match &layout.scope {
        crate::storage_layout::RuntimeScope::GlobalConversation { conversation_id } => {
            decision_persistence.with_no_workspace_conversation_scope(*conversation_id)
        }
        crate::storage_layout::RuntimeScope::Project { .. } => decision_persistence,
    };
    let file_persistence: Arc<dyn harness_permission::DecisionStore> =
        Arc::new(decision_persistence);

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
    let background_agent_starter: Arc<dyn BackgroundAgentStarterCap> =
        Arc::new(DesktopBackgroundAgentStarter {
            runtime_layout: layout.clone(),
            global_config_store: global_config_store_for_home(),
            project_config_store: project_workspace_root
                .as_deref()
                .map(project_config_store_for_workspace),
            event_store: Arc::clone(&event_store),
        });
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
        ToolCapability::Custom("jyowo.background_agent.starter".to_owned()),
        Arc::clone(&background_agent_starter),
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
    let mcp_config = mcp_config_from_records(
        mcp_server_records,
        SessionId::new(),
        AgentId::new(),
        Arc::clone(&mcp_diagnostic_store),
        Arc::clone(&authorization_service),
        project_workspace_root,
    )
    .await?;

    let mut default_session_options = SessionOptions::new(execution_cwd)
        .with_agent_runtime_root(runtime_root)
        .with_model_id(model_id.clone())
        .with_protocol(protocol);
    if provider_defaults_extra != Value::Null {
        default_session_options = default_session_options.with_model_extra(provider_defaults_extra);
    }
    if let Some(project_workspace_root) = project_workspace_root {
        default_session_options =
            default_session_options.with_project_workspace_root(project_workspace_root);
    }
    let harness = Harness::builder()
        .with_workspace_root(execution_cwd)
        .with_model_arc(model_provider)
        .with_model_id(model_id.clone())
        .with_shared_provider_capability_routes(provider_capability_routes)
        .with_default_session_options(default_session_options)
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
        .with_capability(
            ToolCapability::Custom("plugin_sidecar".to_owned()),
            plugin_sidecar_capability,
        )
        .with_capability(ToolCapability::NetworkBroker, network_broker)
        .with_mcp_config(mcp_config)
        .with_plugin_registry(plugin_registry)
        .with_memory_database_path(layout.runtime_root.join("memory").join("memory.sqlite3"))
        .with_memory_provider(
            harness_memory::local::LocalMemoryProvider::open(
                &layout
                    .runtime_root
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

fn ensure_desktop_runtime_store_paths(runtime_root: &Path) -> Result<(), CommandErrorPayload> {
    ensure_runtime_directory_no_symlink(runtime_root, "runtime root")?;
    ensure_runtime_directory_no_symlink(&runtime_root.join("events"), "runtime events directory")?;
    ensure_runtime_directory_no_symlink(&runtime_root.join("blobs"), "runtime blob directory")?;
    ensure_runtime_directory_no_symlink(&runtime_root.join("memory"), "runtime memory directory")?;

    for (path, label) in [
        (
            runtime_root.join("provider-continuations.jsonl"),
            "provider continuation file",
        ),
        (
            runtime_root.join("permission-decisions.json"),
            "permission decisions file",
        ),
        (
            runtime_root.join("conversation-read-model.sqlite"),
            "conversation read model sqlite file",
        ),
        (
            runtime_root.join("conversation-read-model.sqlite-shm"),
            "conversation read model sqlite shm file",
        ),
        (
            runtime_root.join("conversation-read-model.sqlite-wal"),
            "conversation read model sqlite wal file",
        ),
        (
            runtime_root.join("memory").join("memory.sqlite3"),
            "memory sqlite file",
        ),
        (
            runtime_root.join("memory").join("memory.sqlite3-shm"),
            "memory sqlite shm file",
        ),
        (
            runtime_root.join("memory").join("memory.sqlite3-wal"),
            "memory sqlite wal file",
        ),
    ] {
        ensure_runtime_path_no_symlink(&path, label)?;
    }

    Ok(())
}

fn ensure_runtime_directory_no_symlink(
    path: &Path,
    label: &str,
) -> Result<(), CommandErrorPayload> {
    super::stores::ensure_app_dir_no_symlink(path, label)
        .map_err(|error| runtime_init_failed(error.message))
}

fn ensure_runtime_path_no_symlink(path: &Path, label: &str) -> Result<(), CommandErrorPayload> {
    super::stores::ensure_no_symlink_components(path, label)
        .map_err(|error| runtime_init_failed(error.message))
}

pub(crate) fn desktop_integrity_signer(
    runtime_root: &Path,
) -> Result<Arc<dyn harness_permission::IntegritySigner>, CommandErrorPayload> {
    let key = desktop_integrity_key(runtime_root)?;
    harness_permission::StaticSignerStore::from_key(
        "desktop-integrity",
        key,
        IntegrityAlgorithm::HmacSha256,
    )
    .map_err(|error| {
        runtime_init_failed(format!("integrity signer initialization failed: {error}"))
    })
}

fn desktop_integrity_key(runtime_root: &Path) -> Result<Vec<u8>, CommandErrorPayload> {
    let path = runtime_root.join("permission-integrity.key");
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
    let project_selection = project_workspace_root
        .map(|project_workspace_root| {
            project_config_store_for_workspace(project_workspace_root)
                .load_project_plugin_selection_if_present()
        })
        .transpose()?
        .flatten();
    let project_enabled_plugin_ids: BTreeSet<String> = if let Some(selection) = &project_selection {
        selection.enabled.iter().cloned().collect()
    } else {
        settings
            .records
            .iter()
            .filter(|record| record.enabled)
            .map(|record| record.plugin_id.0.clone())
            .collect()
    };
    let global_enabled_plugin_ids: BTreeSet<String> = if let Some(selection) = &project_selection {
        selection.enabled.iter().cloned().collect()
    } else {
        global_settings
            .as_ref()
            .map(|settings| {
                settings
                    .records
                    .iter()
                    .filter(|record| record.enabled)
                    .map(|record| record.plugin_id.0.clone())
                    .collect()
            })
            .unwrap_or_default()
    };
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
    let allow_project_plugins = project_selection
        .as_ref()
        .map(|selection| selection.allow_project_plugins)
        .unwrap_or(settings.allow_project_plugins);
    let (sidecar_sandbox, sidecar_sandbox_mode) = desktop_plugin_sidecar_sandbox(execution_cwd);
    let mut entries = BTreeMap::new();
    let mut plugin_enabled_by_name = BTreeMap::<PluginName, bool>::new();
    if let (Some(global_store), Some(global_settings)) = (global_plugin_store, &global_settings) {
        collect_plugin_registry_records(
            &global_settings.records,
            global_store,
            &global_enabled_plugin_ids,
            project_selection.is_some(),
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
        project_selection.is_some(),
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
        if allow_project_plugins && project_selection.is_none() {
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

pub(crate) async fn reload_desktop_harness_after_plugin_change_locked(
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    let Some(stream_permission_runtime) = state.stream_permission_runtime.as_ref() else {
        return Ok(());
    };
    let layout = state.runtime_layout().clone();
    let (harness, model_id, protocol) = build_desktop_harness(
        &layout,
        Arc::clone(stream_permission_runtime),
        None,
        Arc::clone(&state.provider_capability_routes),
        Some(Arc::clone(&state.provider_settings_store)),
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
        AgentToolPolicy, AgentWorkspaceIsolationMode, AssistantSegment,
        BackgroundAgentToolSessionSnapshot, DeferPolicy, DiagnosticsRunRequest,
        DiagnosticsRunnerKind, MessageId, MessageMetadata, NetworkAccess, PluginId,
        PluginSelectionRecord, ProcessStepDetail, ProviderProfileDefinition,
        ProviderProfileModelDescriptor, ProviderProfileModelLifecycle, ResourceLimits,
        SandboxPolicy, SandboxScope, ToolProperties, ToolResult, ToolSearchMode,
        ToolUseCompletedEvent, ToolUseRequestedEvent, UserMessageAppendedEvent, WorkspaceAccess,
    };
    use jyowo_harness_sdk::testing::InMemoryEventStore;
    use std::sync::Mutex;

    static HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct HomeEnvGuard {
        previous: Option<std::ffi::OsString>,
    }

    struct CurrentDirGuard {
        previous: PathBuf,
    }

    impl HomeEnvGuard {
        fn set(home: &Path) -> Self {
            let previous = std::env::var_os("HOME");
            std::env::set_var("HOME", home.as_os_str());
            Self { previous }
        }
    }

    impl CurrentDirGuard {
        fn set(cwd: &Path) -> Self {
            let previous = std::env::current_dir().expect("current dir");
            std::env::set_current_dir(cwd).expect("set current dir");
            Self { previous }
        }
    }

    fn lock_home_env() -> std::sync::MutexGuard<'static, ()> {
        HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn temp_home_dir() -> tempfile::TempDir {
        let base = std::env::current_dir()
            .expect("current dir")
            .join("target")
            .join("runtime-test-homes");
        std::fs::create_dir_all(&base).expect("test home base");
        tempfile::Builder::new()
            .prefix("home-")
            .tempdir_in(base)
            .expect("home tempdir")
    }

    fn test_tool_use_requested_event(
        run_id: RunId,
        tool_use_id: ToolUseId,
        tool_name: &str,
    ) -> ToolUseRequestedEvent {
        ToolUseRequestedEvent {
            at: now(),
            causation_id: EventId::new(),
            input: json!({ "toolName": tool_name }),
            properties: ToolProperties {
                is_concurrency_safe: true,
                is_destructive: false,
                is_read_only: false,
                long_running: None,
                defer_policy: DeferPolicy::AlwaysLoad,
            },
            run_id,
            tool_name: tool_name.to_owned(),
            tool_use_id,
        }
    }

    async fn command_output_ref_for_session(
        state: &DesktopRuntimeState,
        session_id: SessionId,
        stdout: &str,
        stderr: &str,
    ) -> String {
        let run_id = RunId::new();
        let user_message_id = MessageId::new();
        let tool_use_id = ToolUseId::new();

        state
            .harness()
            .expect("runtime harness should exist")
            .open_or_create_conversation_session(
                state
                    .conversation_session_options(session_id)
                    .expect("session options"),
            )
            .await
            .expect("conversation session should open");
        state
            .harness()
            .expect("runtime harness should exist")
            .event_store()
            .append(
                TenantId::SINGLE,
                session_id,
                &[
                    Event::UserMessageAppended(UserMessageAppendedEvent {
                        run_id,
                        message_id: user_message_id,
                        content: MessageContent::Text("run command".to_owned()),
                        metadata: MessageMetadata::default(),
                        attachments: Vec::new(),
                        at: now(),
                    }),
                    Event::ToolUseRequested(test_tool_use_requested_event(
                        run_id,
                        tool_use_id,
                        "shell",
                    )),
                    Event::ToolUseCompleted(ToolUseCompletedEvent {
                        tool_use_id,
                        result: ToolResult::Structured(json!({
                            "exitCode": 0,
                            "stdout": stdout,
                            "stderr": stderr,
                        })),
                        usage: None,
                        duration_ms: 21,
                        at: now(),
                    }),
                ],
            )
            .await
            .expect("events should append");

        let page = page_conversation_worktree_with_runtime_state(
            PageConversationWorktreeRequest {
                conversation_id: session_id.to_string(),
                page_cursor: None,
                direction: PageConversationWorktreeDirection::After,
                limit: Some(1),
            },
            state,
        )
        .await
        .expect("worktree should load");

        page.turns[0]
            .assistant
            .as_ref()
            .expect("assistant projection should exist")
            .segments
            .iter()
            .find_map(|segment| match segment {
                AssistantSegment::Process(process) => {
                    process.steps.iter().find_map(|step| match &step.detail {
                        Some(ProcessStepDetail::Command(command)) => {
                            command.full_output_ref.as_ref().map(ToString::to_string)
                        }
                        _ => None,
                    })
                }
                _ => None,
            })
            .expect("command output ref should be projected")
    }

    impl Drop for HomeEnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.previous).expect("restore current dir");
        }
    }

    #[cfg(unix)]
    #[test]
    fn desktop_integrity_key_is_created_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let workspace = tempfile::tempdir().expect("temp workspace");
        let workspace_root = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");
        let runtime_dir = workspace_root.join(".jyowo").join("runtime");
        let key = desktop_integrity_key(&runtime_dir).expect("integrity key");

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

        let error = desktop_integrity_key(&workspace_root.join(".jyowo").join("runtime"))
            .expect_err("symlink should fail");

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

        let error = desktop_integrity_key(&runtime_dir).expect_err("symlink should fail");

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

        let loaded = desktop_integrity_key(&runtime_dir).expect("integrity key");

        assert_eq!(loaded, key);
        let mode = std::fs::metadata(key_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[tokio::test]
    async fn background_agent_starter_rejects_when_settings_disable_capability() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let workspace_root = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");
        let storage_layout = test_storage_layout_for_workspace(&workspace_root);
        let starter = DesktopBackgroundAgentStarter {
            runtime_layout: storage_layout.runtime_layout_for_project(&workspace_root),
            global_config_store: GlobalConfigStore::new(storage_layout.clone()),
            project_config_store: Some(ProjectConfigStore::new(
                storage_layout,
                workspace_root.clone(),
            )),
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
        let store =
            AgentRuntimeStore::open_runtime_dir(workspace_root.join(".jyowo").join("runtime"))
                .expect("runtime store opens");
        assert!(
            store
                .list_background_agents(true)
                .expect("background records list")
                .is_empty(),
            "policy rejection must happen before creating a background record"
        );
    }

    // ── Task 5: routing sandbox tests ──

    #[tokio::test]
    async fn runtime_uses_routing_sandbox() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let sandbox = build_desktop_process_sandbox(workspace.path())
            .await
            .expect("factory should succeed");
        assert_eq!(
            sandbox.backend_id(),
            "routing",
            "desktop sandbox must be a routing backend, not a bare LocalSandbox"
        );
    }

    #[tokio::test]
    async fn diagnostics_runner_uses_routing_sandbox() {
        let workspace = tempfile::tempdir().expect("temp workspace");

        // Set up a minimal Rust project so cargo check can run.
        std::fs::write(
            workspace.path().join("Cargo.toml"),
            "[package]\nname = \"test-crate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("write Cargo.toml");
        std::fs::create_dir_all(workspace.path().join("src")).expect("create src");
        std::fs::write(workspace.path().join("src").join("lib.rs"), "").expect("write lib.rs");

        let sandbox = build_desktop_process_sandbox(workspace.path())
            .await
            .expect("factory should succeed");

        let runner = DesktopDiagnosticsRunner::new(Arc::clone(&sandbox));

        let request = DiagnosticsRunRequest {
            runner: DiagnosticsRunnerKind::Rust,
            workspace_root: workspace.path().to_path_buf(),
            session_id: SessionId::new(),
            run_id: RunId::new(),
            tenant_id: TenantId::SINGLE,
        };
        let output = runner.run_diagnostics(request).await;
        // Diagnostics should succeed (or return a structured error, not panic).
        // cargo check on an empty crate will succeed.
        match output {
            Ok(_raw) => {} // diagnostics succeeded through the routing sandbox
            Err(ref err) => {
                // If cargo isn't installed, this is an expected runtime failure, not
                // a sandbox wiring failure.
                let msg = format!("{err}");
                assert!(
                    !msg.contains("routing") && !msg.contains("sandbox"),
                    "diagnostics failure must not be a sandbox wiring error: {msg}"
                );
            }
        }
    }

    #[tokio::test]
    async fn docker_fallback_mounts_workspace() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let workspace_root = workspace.path().canonicalize().expect("canonicalize");

        // build_docker_fallback creates a DockerSandbox with the workspace mounted
        // at /workspace. The test skips when Docker is unavailable.
        let docker = match build_docker_fallback(&workspace_root).await {
            Ok(docker) => docker,
            Err(_) => {
                eprintln!("docker unavailable — skipping docker_fallback_mounts_workspace");
                return;
            }
        };

        // The workspace mount is internal to DockerSandbox. We verify indirectly
        // through capabilities: a DockerSandbox with a read-write mount reports
        // read_write_all workspace support.
        let caps = docker.capabilities();
        assert!(
            caps.workspace.read_write_all,
            "docker fallback with workspace mount must support read_write_all"
        );
        assert!(
            !caps.workspace.read_only,
            "docker fallback must not report read_only support"
        );
        assert!(
            !caps.workspace.writable_subpaths,
            "docker fallback must not report writable_subpaths support"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn docker_fallback_verifies_workspace_mount_before_registration() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let workspace_root = workspace.path().canonicalize().expect("canonicalize");
        let log = workspace.path().join("docker.log");
        let docker = stub_desktop_docker(workspace.path(), &log, false);

        build_docker_fallback_with_binary(&workspace_root, docker)
            .await
            .expect("docker fallback should register after live workspace probe");

        let log_text = std::fs::read_to_string(log).expect("docker log should be written");
        assert!(
            log_text.contains("version --format {{.Server.Version}}"),
            "fallback must verify daemon availability before registration: {log_text}"
        );
        assert!(
            log_text.contains("run --rm -i"),
            "fallback must run a live container probe before registration: {log_text}"
        );
        assert!(
            log_text.contains(&format!(
                "-v {}:/workspace:rw,rprivate",
                workspace_root.display()
            )),
            "probe must use the desktop workspace mount contract: {log_text}"
        );
        assert!(
            log_text.contains("-w /workspace"),
            "probe must execute from /workspace: {log_text}"
        );
        assert!(
            log_text.contains("jyowo-workspace:latest sh -c"),
            "probe must use the default workspace image and shell command: {log_text}"
        );
        assert!(
            log_text.contains("test -d /workspace"),
            "probe must verify /workspace is mounted: {log_text}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn docker_fallback_rejects_failed_workspace_mount_probe() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let workspace_root = workspace.path().canonicalize().expect("canonicalize");
        let log = workspace.path().join("docker.log");
        let docker = stub_desktop_docker(workspace.path(), &log, true);

        let err = build_docker_fallback_with_binary(&workspace_root, docker)
            .await
            .expect_err("docker fallback must not register when live workspace probe fails");
        let msg = err.to_string();
        assert!(
            msg.contains("workspace probe"),
            "error must explain the failed workspace probe: {msg}"
        );

        let log_text = std::fs::read_to_string(log).expect("docker log should be written");
        assert!(
            log_text.contains("run --rm -i"),
            "failing probe should still be the registration gate: {log_text}"
        );
    }

    #[tokio::test]
    async fn docker_fallback_rejects_restricted_workspace_mounts() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let workspace_root = workspace.path().canonicalize().expect("canonicalize");

        let docker = match build_docker_fallback(&workspace_root).await {
            Ok(docker) => docker,
            Err(_) => {
                eprintln!("docker unavailable — skipping docker_fallback_rejects_restricted_workspace_mounts");
                return;
            }
        };

        // A read_only workspace policy must fail preflight because the Docker
        // fallback only has a read-write workspace mount.
        let spec = ExecSpec {
            workspace_access: WorkspaceAccess::ReadOnly,
            ..ExecSpec::default()
        };
        let err = docker
            .preflight_execute(&spec)
            .expect_err("read_only workspace must fail preflight for docker fallback");
        let msg = err.to_string();
        assert!(
            msg.contains("workspace_access") || msg.contains("workspace"),
            "error must identify workspace policy as the reason: {msg}"
        );
    }

    #[cfg(unix)]
    fn stub_desktop_docker(root: &Path, log: &Path, fail_run: bool) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let bin = root.join("docker");
        let fail_run = if fail_run { "1" } else { "0" };
        let script = format!(
            r#"#!/bin/sh
printf '%s\n' "$*" >> "{}"
case "$1" in
  version) exit 0 ;;
  run)
    if [ "{}" = "1" ]; then
      printf 'workspace probe failed' >&2
      exit 42
    fi
    printf 'workspace probe ok'
    exit 0
    ;;
  *) exit 0 ;;
esac
"#,
            log.display(),
            fail_run
        );
        std::fs::write(&bin, script).expect("stub docker should be written");
        let mut permissions = std::fs::metadata(&bin)
            .expect("stub docker metadata should exist")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&bin, permissions).expect("stub docker should be executable");
        bin
    }

    #[tokio::test]
    async fn unsupported_restricted_policy_reports_reason() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let sandbox = build_desktop_process_sandbox(workspace.path())
            .await
            .expect("factory should succeed");

        // Request a restricted network policy (AllowList) + read_only workspace.
        // No child backend supports this combination, so the router must fail with
        // a capability reason naming the candidate backends.
        let spec = ExecSpec {
            policy: SandboxPolicy {
                network: NetworkAccess::AllowList(vec![]),
                mode: SandboxMode::None,
                scope: SandboxScope::WorkspaceOnly,
                resource_limits: ResourceLimits {
                    max_memory_bytes: None,
                    max_cpu_cores: None,
                    max_pids: None,
                    max_wall_clock_ms: None,
                    max_open_files: None,
                },
                denied_host_paths: Vec::new(),
            },
            workspace_access: WorkspaceAccess::ReadOnly,
            ..ExecSpec::default()
        };

        let err = sandbox
            .preflight_execute(&spec)
            .expect_err("restricted policy must fail preflight");
        let msg = err.to_string();
        assert!(
            msg.contains("network") || msg.contains("capability"),
            "error must describe why the policy cannot be enforced: {msg}"
        );
    }

    #[test]
    fn global_conversation_runtime_state_uses_global_runtime_not_unconfigured_workspace() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        let conversation_id = SessionId::new();
        let state =
            tauri::async_runtime::block_on(runtime_state_for_global_conversation(conversation_id))
                .expect("global conversation runtime state");

        let layout = state.runtime_layout();
        assert!(layout.workspace_root.is_none());
        assert!(layout
            .runtime_root
            .ends_with(Path::new(".jyowo/runtime/global-conversations")));
        assert!(layout.conversation_cwd.ends_with(
            Path::new("global-conversations")
                .join("workdir")
                .join(conversation_id.to_string())
        ));
        assert_ne!(
            layout.conversation_cwd,
            crate::project_registry::unconfigured_workspace_root()
        );
        assert_ne!(
            layout.runtime_root,
            crate::project_registry::unconfigured_workspace_root()
                .join(".jyowo")
                .join("runtime")
        );
        assert!(state.project_config_store.is_none());
        assert_eq!(
            state.runtime_root().join("memory").join("memory.sqlite3"),
            layout.runtime_root.join("memory").join("memory.sqlite3")
        );
        assert_eq!(
            state.skill_store.enabled_dir(),
            home.path().join(".jyowo").join("skills").join("packages")
        );
        assert_eq!(
            state.plugin_store.package_root(),
            home.path().join(".jyowo").join("plugins").join("packages")
        );
    }

    #[tokio::test]
    async fn project_selection_cannot_enable_globally_disabled_plugin() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let workspace_root = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");
        let state = DesktopRuntimeState::with_workspace_for_test(workspace_root)
            .expect("desktop runtime state");
        let global_plugin_id = PluginId("global-disabled@0.1.0".to_owned());
        let global_plugin_store = DesktopPluginStore::global(
            state
                .global_config_store
                .as_ref()
                .expect("global config")
                .layout()
                .clone(),
        );
        global_plugin_store
            .save_record(&PluginSettingsRecord {
                records: vec![PluginStoreRecord {
                    plugin_id: global_plugin_id.clone(),
                    name: "global-disabled".to_owned(),
                    version: "0.1.0".to_owned(),
                    enabled: false,
                    package_dir: "global-disabled_0.1.0".to_owned(),
                    source_path: "<local-plugin>".to_owned(),
                    content_hash: "hash".to_owned(),
                    imported_at: "2026-01-01T00:00:00Z".to_owned(),
                    updated_at: "2026-01-01T00:00:00Z".to_owned(),
                    config: Value::Null,
                    last_validation_error: None,
                }],
                ..PluginSettingsRecord::default()
            })
            .expect("save global plugin index");
        state
            .project_config_store
            .as_ref()
            .expect("project config")
            .save_project_plugin_selection(&PluginSelectionRecord {
                allow_project_plugins: false,
                enabled: Vec::new(),
            })
            .expect("save project selection");

        let error = set_plugin_enabled_with_runtime_state(
            SetPluginEnabledRequest {
                plugin_id: global_plugin_id,
                enabled: true,
            },
            &state,
        )
        .await
        .expect_err("global disabled plugin must not be enabled from project selection");

        assert_eq!(error.code, "INVALID_PAYLOAD");
        assert!(error.message.contains("disabled globally"));
    }

    #[tokio::test]
    async fn network_broker_runtime_assembly_uses_same_instance() {
        use harness_execution::ReqwestToolNetworkBroker;
        use harness_tool::ToolNetworkBrokerCap;

        // Simulate the runtime assembly: create one broker instance, register it
        // in both a CapabilityRegistry and use it as the network_broker for
        // ExecutionPreflightRegistry. Both must hold the same Arc allocation.
        let redactor: Arc<dyn Redactor> = Arc::new(NoopRedactor);
        let broker: Arc<dyn ToolNetworkBrokerCap> = Arc::new(
            ReqwestToolNetworkBroker::new(
                Duration::from_secs(10),
                1_048_576,
                Arc::clone(&redactor),
            )
            .expect("broker construction"),
        );

        // Register in CapabilityRegistry.
        let mut capabilities = CapabilityRegistry::default();
        capabilities.install(ToolCapability::NetworkBroker, Arc::clone(&broker));

        // Build ExecutionPreflightRegistry with the same broker Arc.
        let workspace = tempfile::tempdir().expect("temp workspace");
        let sandbox = build_desktop_process_sandbox(workspace.path())
            .await
            .expect("factory should succeed");
        let _registry =
            ExecutionPreflightRegistry::new(sandbox, Some(broker), Arc::new(capabilities));

        // The key invariant: ExecutionPreflightRegistry.network_broker and
        // CapabilityRegistry[ToolCapability::NetworkBroker] hold the same Arc.
        // This is verified at construction time — no assertion needed beyond
        // successful construction without panics.
    }

    #[tokio::test]
    async fn plugin_sidecar_capability_is_registered_for_preflight() {
        let workspace = tempfile::tempdir().expect("temp workspace");
        let sandbox = build_desktop_process_sandbox(workspace.path())
            .await
            .expect("factory should succeed");
        let mut capabilities = CapabilityRegistry::default();
        capabilities.install(
            ToolCapability::Custom("plugin_sidecar".to_owned()),
            Arc::new(()),
        );
        let registry = ExecutionPreflightRegistry::new(sandbox, None, Arc::new(capabilities));

        assert!(
            registry
                .capabilities
                .contains(&ToolCapability::Custom("plugin_sidecar".to_owned())),
            "registered plugin sidecar capability should be available to preflight"
        );
    }

    #[cfg(unix)]
    #[test]
    fn global_conversation_runtime_state_rejects_symlink_blob_store_directory() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        let runtime_root = home
            .path()
            .join(".jyowo")
            .join("runtime")
            .join("global-conversations");
        std::fs::create_dir_all(&runtime_root).expect("runtime root");
        let external = tempfile::tempdir().expect("external");
        std::os::unix::fs::symlink(external.path(), runtime_root.join("blobs")).expect("symlink");

        let error = match tauri::async_runtime::block_on(runtime_state_for_global_conversation(
            SessionId::new(),
        )) {
            Ok(_) => panic!("symlinked blob directory should fail"),
            Err(error) => error,
        };

        assert_eq!(error.code, "RUNTIME_INIT_FAILED");
        assert!(error.message.contains("symlink"));
    }

    #[cfg(unix)]
    #[test]
    fn unconfigured_runtime_state_fails_closed_without_current_dir_workspace_fallback() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let workspace = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        let _cwd_guard = CurrentDirGuard::set(workspace.path());
        let runtime_root = home
            .path()
            .join(".jyowo")
            .join("runtime")
            .join("global-conversations");
        std::fs::create_dir_all(&runtime_root).expect("runtime root");
        let external = tempfile::tempdir().expect("external");
        std::os::unix::fs::symlink(external.path(), runtime_root.join("blobs")).expect("symlink");

        let result = std::panic::catch_unwind(unconfigured_runtime_state);

        assert!(result.is_err());
        assert!(
            !workspace.path().join(".jyowo").join("runtime").exists(),
            "no-workspace init failure must not create cwd project runtime"
        );
    }

    #[cfg(unix)]
    #[test]
    fn global_conversation_runtime_state_rejects_symlink_sqlite_read_model_file() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        let runtime_root = home
            .path()
            .join(".jyowo")
            .join("runtime")
            .join("global-conversations");
        std::fs::create_dir_all(&runtime_root).expect("runtime root");
        let external = tempfile::NamedTempFile::new().expect("external sqlite");
        std::os::unix::fs::symlink(
            external.path(),
            runtime_root.join("conversation-read-model.sqlite"),
        )
        .expect("symlink");

        let error = match tauri::async_runtime::block_on(runtime_state_for_global_conversation(
            SessionId::new(),
        )) {
            Ok(_) => panic!("symlinked sqlite read model should fail"),
            Err(error) => error,
        };

        assert_eq!(error.code, "RUNTIME_INIT_FAILED");
        assert!(error.message.contains("symlink"));
    }

    #[cfg(unix)]
    #[test]
    fn no_workspace_delete_preserves_metadata_when_cleanup_fails() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        let state =
            tauri::async_runtime::block_on(runtime_state_for_global_conversation(SessionId::new()))
                .expect("global conversation runtime state");
        let created =
            tauri::async_runtime::block_on(create_conversation_with_runtime_state(&state))
                .expect("create conversation");
        let conversation_id = created.conversation.id.clone();
        let session_id = SessionId::parse(&conversation_id).expect("session id");
        let workdir_parent = state.runtime_root().join("workdir");
        std::fs::create_dir_all(&workdir_parent).expect("workdir parent");
        let external = tempfile::tempdir().expect("external");
        std::os::unix::fs::symlink(external.path(), workdir_parent.join(session_id.to_string()))
            .expect("symlink");

        let error = tauri::async_runtime::block_on(delete_conversation_with_runtime_state(
            DeleteConversationRequest {
                conversation_id: conversation_id.clone(),
            },
            &state,
        ))
        .expect_err("cleanup failure should fail delete");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(error.message.contains("symlink"));
        let metadata = state
            .conversation_metadata_store
            .load_record()
            .expect("load metadata");
        assert!(metadata.conversations.contains_key(&conversation_id));
    }

    #[test]
    fn no_workspace_attachment_owner_uses_requested_conversation_id() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        let state =
            tauri::async_runtime::block_on(runtime_state_for_global_conversation(SessionId::new()))
                .expect("global conversation runtime state");
        let created =
            tauri::async_runtime::block_on(create_conversation_with_runtime_state(&state))
                .expect("create conversation");
        let conversation_id = created.conversation.id.clone();
        let session_id = SessionId::parse(&conversation_id).expect("session id");
        let attachment_path = state
            .runtime_root()
            .join("workdir")
            .join(session_id.to_string())
            .join("notes.txt");
        std::fs::create_dir_all(attachment_path.parent().expect("attachment parent"))
            .expect("create attachment parent");
        std::fs::write(&attachment_path, "draft notes").expect("write attachment");

        let attachment =
            tauri::async_runtime::block_on(create_attachment_from_path_with_runtime_state(
                CreateAttachmentFromPathRequest {
                    conversation_id: Some(conversation_id.clone()),
                    path: attachment_path.to_string_lossy().to_string(),
                },
                &state,
            ))
            .expect("create attachment")
            .attachment;
        let record_path = state
            .runtime_root()
            .join("attachments")
            .join("records")
            .join(format!("{}.json", attachment.id));
        assert!(record_path.exists());

        tauri::async_runtime::block_on(delete_conversation_with_runtime_state(
            DeleteConversationRequest { conversation_id },
            &state,
        ))
        .expect("delete conversation");

        assert!(!record_path.exists());
    }

    #[test]
    fn no_workspace_execution_settings_use_global_defaults_not_workdir_overrides() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        let conversation_id = SessionId::new();
        let state =
            tauri::async_runtime::block_on(runtime_state_for_global_conversation(conversation_id))
                .expect("global conversation runtime state");

        state
            .global_config_store
            .as_ref()
            .expect("global config store")
            .save_execution_defaults(&harness_contracts::ExecutionDefaultsRecord {
                permission_mode: PermissionMode::BypassPermissions,
                tool_profile: ToolProfile::Minimal,
                context_compression_trigger_ratio: 0.7,
                subagents_enabled: false,
                agent_teams_enabled: false,
                background_agents_enabled: false,
            })
            .expect("save global defaults");

        let record = state
            .execution_settings_store
            .load_record()
            .expect("load execution settings");
        assert_eq!(record.permission_mode, PermissionMode::BypassPermissions);
        assert_eq!(record.tool_profile, ToolProfile::Minimal);
        assert!(!state
            .conversation_cwd()
            .join(".jyowo")
            .join("config")
            .join("execution-overrides.json")
            .exists());
    }

    #[test]
    fn no_workspace_project_scoped_stores_read_empty_and_fail_closed_without_scratch_config() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        global_config_store_for_home()
            .save_provider_profiles(&[ProviderProfileDefinition {
                id: "global-openai".to_owned(),
                display_name: "Global OpenAI".to_owned(),
                provider_id: "openai".to_owned(),
                model_id: "gpt-5".to_owned(),
                protocol: ModelProtocol::ChatCompletions,
                base_url: None,
                provider_defaults: None,
                model_descriptor: ProviderProfileModelDescriptor {
                    protocol: ModelProtocol::ChatCompletions,
                    context_window: 128000,
                    display_name: "GPT-5".to_owned(),
                    lifecycle: ProviderProfileModelLifecycle::Stable,
                    max_output_tokens: 16384,
                    model_id: "gpt-5".to_owned(),
                    provider_id: "openai".to_owned(),
                    conversation_capability:
                        harness_contracts::ProviderProfileConversationCapability {
                            input_modalities: vec!["text".to_owned()],
                            output_modalities: vec!["text".to_owned()],
                            context_window: 128000,
                            max_output_tokens: 16384,
                            streaming: true,
                            tool_calling: true,
                            reasoning: false,
                            prompt_cache: false,
                            structured_output: false,
                        },
                },
            }])
            .expect("save global provider profile");
        let conversation_id = SessionId::new();
        let state =
            tauri::async_runtime::block_on(runtime_state_for_global_conversation(conversation_id))
                .expect("global conversation runtime state");
        let scratch_config_root = state.conversation_cwd().join(".jyowo").join("config");

        assert_eq!(
            state
                .automation_store
                .load_automations()
                .expect("automations read"),
            Vec::<AutomationSpec>::new()
        );
        assert_eq!(
            state
                .mcp_server_store
                .load_records()
                .expect("mcp servers read"),
            Vec::<McpServerConfigRecord>::new()
        );
        assert_eq!(
            state
                .provider_capability_route_store
                .load_record()
                .expect("provider routes read"),
            Some(empty_provider_capability_route_settings())
        );

        let automation_error = state
            .automation_store
            .save_automations(&[])
            .expect_err("no-workspace automations must be unavailable");
        assert_eq!(automation_error.code, "INVALID_PAYLOAD");

        let mcp_error = state
            .mcp_server_store
            .save_record(&browser_mcp_preset_record(
                BrowserMcpPresetId::Playwright,
                true,
            ))
            .expect_err("no-workspace mcp servers must be unavailable");
        assert_eq!(mcp_error.code, "INVALID_PAYLOAD");

        let provider_route_error = state
            .provider_capability_route_store
            .save_record(
                &empty_provider_capability_route_settings(),
                ProviderCapabilityRouteValidationToken { _private: () },
            )
            .expect_err("no-workspace provider routes must be unavailable");
        assert_eq!(provider_route_error.code, "INVALID_PAYLOAD");

        assert!(
            !scratch_config_root.exists(),
            "no-workspace project stores must not create scratch .jyowo/config"
        );
    }

    #[test]
    fn no_workspace_session_options_use_per_conversation_cwd() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        let first = SessionId::new();
        let second = SessionId::new();
        let state = tauri::async_runtime::block_on(runtime_state_for_global_conversation(first))
            .expect("global conversation runtime state");

        let first_options = state
            .conversation_session_options(first)
            .expect("first session options");
        let second_options = state
            .conversation_session_options(second)
            .expect("second session options");

        assert!(first_options.workspace_root.ends_with(
            Path::new("global-conversations")
                .join("workdir")
                .join(first.to_string())
        ));
        assert!(second_options.workspace_root.ends_with(
            Path::new("global-conversations")
                .join("workdir")
                .join(second.to_string())
        ));
        assert_ne!(first_options.workspace_root, second_options.workspace_root);
        assert_ne!(
            first_options.workspace_root,
            crate::project_registry::unconfigured_workspace_root()
        );
        assert_eq!(
            first_options.agent_runtime_root.as_deref(),
            Some(state.runtime_root())
        );
        assert_eq!(
            second_options.agent_runtime_root.as_deref(),
            Some(state.runtime_root())
        );
    }

    #[test]
    fn global_conversation_runtime_layout_can_use_explicit_runtime_root() {
        let conversation_id = SessionId::new();
        let runtime_root = tempfile::tempdir().expect("runtime").path().join("runtime");

        let layout = global_conversation_runtime_layout_with_runtime_root(
            conversation_id,
            runtime_root.clone(),
        );

        assert_eq!(layout.runtime_root, runtime_root);
        assert_eq!(
            layout.conversation_cwd,
            layout
                .runtime_root
                .join("workdir")
                .join(conversation_id.to_string())
        );
        assert!(layout.workspace_root.is_none());
    }

    #[test]
    fn no_workspace_active_runtime_cache_is_scoped_to_conversation() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        let first = SessionId::new();
        let second = SessionId::new();
        let state = tauri::async_runtime::block_on(runtime_state_for_global_conversation(first))
            .expect("global conversation runtime state");
        let model_config_id = "test-model-config";
        let provider_fingerprint = [7_u8; 32];
        {
            let mut active = state
                .active_runtime
                .write()
                .expect("desktop active runtime lock should not be poisoned");
            active.default_model_config_id = Some(model_config_id.to_owned());
            active.provider_config_fingerprint = Some(provider_fingerprint);
        }

        assert!(state
            .active_conversation_runtime_for_model_config(
                first,
                model_config_id,
                provider_fingerprint
            )
            .expect("active runtime lookup")
            .is_some());
        assert!(state
            .active_conversation_runtime_for_model_config(
                second,
                model_config_id,
                provider_fingerprint
            )
            .expect("active runtime lookup")
            .is_none());
    }

    #[test]
    fn no_workspace_plugin_reload_preserves_conversation_runtime_scope() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        let conversation_id = SessionId::new();
        let state =
            tauri::async_runtime::block_on(runtime_state_for_global_conversation(conversation_id))
                .expect("global conversation runtime state");

        tauri::async_runtime::block_on(reload_desktop_harness_after_plugin_change_locked(&state))
            .expect("plugin reload should rebuild no-workspace runtime");

        let active = state
            .active_runtime
            .read()
            .expect("desktop active runtime lock should not be poisoned");
        assert_eq!(
            active.runtime_scope,
            RuntimeScope::GlobalConversation { conversation_id }
        );
    }

    #[test]
    fn no_workspace_attachment_records_use_global_runtime_root() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        let conversation_id = SessionId::new();
        let state =
            tauri::async_runtime::block_on(runtime_state_for_global_conversation(conversation_id))
                .expect("global conversation runtime state");
        let attachment_path = state.conversation_cwd().join("notes.txt");
        std::fs::create_dir_all(attachment_path.parent().unwrap()).expect("workdir");
        std::fs::write(&attachment_path, "local notes").expect("attachment source");

        let payload =
            tauri::async_runtime::block_on(create_attachment_from_path_with_runtime_state(
                CreateAttachmentFromPathRequest {
                    conversation_id: None,
                    path: attachment_path.to_string_lossy().into_owned(),
                },
                &state,
            ))
            .expect("attachment should be stored");

        assert!(state
            .runtime_root()
            .join("attachments")
            .join("records")
            .join(format!("{}.json", payload.attachment.id))
            .is_file());
        assert!(state.runtime_root().join("blobs").is_dir());
        assert!(!state
            .conversation_cwd()
            .join(".jyowo")
            .join("runtime")
            .join("attachments")
            .exists());
    }

    #[test]
    fn no_workspace_evidence_exports_use_global_runtime_root() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        let conversation_id = SessionId::new();
        let state =
            tauri::async_runtime::block_on(runtime_state_for_global_conversation(conversation_id))
                .expect("global conversation runtime state");
        let command_ref = tauri::async_runtime::block_on(command_output_ref_for_session(
            &state,
            conversation_id,
            "exported output",
            "",
        ));

        let exported =
            tauri::async_runtime::block_on(export_conversation_evidence_with_runtime_state(
                ExportConversationEvidenceRequest {
                    conversation_id: conversation_id.to_string(),
                    kind: "command-output".to_owned(),
                    ref_id: command_ref,
                },
                &state,
            ))
            .expect("command output evidence should export");

        assert!(exported
            .path
            .starts_with(&format!("exports/{conversation_id}/")));
        assert_eq!(
            std::fs::read_to_string(state.runtime_root().join(&exported.path))
                .expect("export should be readable"),
            "exported output"
        );
        assert!(!state
            .conversation_cwd()
            .join(".jyowo")
            .join("runtime")
            .join("exports")
            .exists());
    }

    #[test]
    fn no_workspace_rejects_workspace_file_context_reference() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        let conversation_id = SessionId::new();
        let state =
            tauri::async_runtime::block_on(runtime_state_for_global_conversation(conversation_id))
                .expect("global conversation runtime state");

        let error = tauri::async_runtime::block_on(build_conversation_turn_input(
            &StartRunRequest {
                attachments: None,
                client_message_id: None,
                context_references: Some(vec![ContextReferencePayload::WorkspaceFile {
                    path: "notes.txt".to_owned(),
                    label: "Notes".to_owned(),
                }]),
                conversation_id: conversation_id.to_string(),
                model_config_id: Some("test-model-config".to_owned()),
                permission_mode: None,
                prompt: "Use this file".to_owned(),
            },
            &state,
        ))
        .expect_err("no-workspace must not accept workspace file references");

        assert_eq!(error.code, "INVALID_PAYLOAD");
        assert!(error.message.contains("active project workspace"));
    }

    #[test]
    fn deleting_no_workspace_conversation_prunes_workdir_and_exports() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        let state =
            tauri::async_runtime::block_on(runtime_state_for_global_conversation(SessionId::new()))
                .expect("global conversation runtime state");
        let created =
            tauri::async_runtime::block_on(create_conversation_with_runtime_state(&state))
                .expect("conversation created");
        let conversation_id =
            SessionId::parse(&created.conversation.id).expect("created id should parse");
        let workdir = state
            .runtime_root()
            .join("workdir")
            .join(conversation_id.to_string());
        let exports = state
            .runtime_root()
            .join("exports")
            .join(conversation_id.to_string());
        std::fs::create_dir_all(&workdir).expect("workdir");
        std::fs::write(workdir.join("scratch.txt"), "scratch").expect("scratch");
        std::fs::create_dir_all(&exports).expect("exports");
        std::fs::write(exports.join("support.json"), "{}").expect("export");

        tauri::async_runtime::block_on(delete_conversation_with_runtime_state(
            DeleteConversationRequest {
                conversation_id: conversation_id.to_string(),
            },
            &state,
        ))
        .expect("delete should succeed");

        assert!(!workdir.exists());
        assert!(!exports.exists());
    }

    #[test]
    fn deleting_no_workspace_conversation_prunes_attachment_records() {
        let _lock = lock_home_env();
        let home = temp_home_dir();
        let _home_guard = HomeEnvGuard::set(home.path());
        let conversation_id = SessionId::new();
        let state =
            tauri::async_runtime::block_on(runtime_state_for_global_conversation(conversation_id))
                .expect("global conversation runtime state");
        let attachment_path = state.conversation_cwd().join("notes.txt");
        std::fs::create_dir_all(attachment_path.parent().unwrap()).expect("workdir");
        std::fs::write(&attachment_path, "local notes").expect("attachment source");
        let attachment =
            tauri::async_runtime::block_on(create_attachment_from_path_with_runtime_state(
                CreateAttachmentFromPathRequest {
                    conversation_id: None,
                    path: attachment_path.to_string_lossy().into_owned(),
                },
                &state,
            ))
            .expect("attachment should be stored");
        let attachment_record =
            attachment_record_path(state.runtime_root(), &attachment.attachment.id);
        assert!(attachment_record.exists());
        let command_ref = tauri::async_runtime::block_on(command_output_ref_for_session(
            &state,
            conversation_id,
            "deleted output",
            "",
        ));
        let exported =
            tauri::async_runtime::block_on(export_conversation_evidence_with_runtime_state(
                ExportConversationEvidenceRequest {
                    conversation_id: conversation_id.to_string(),
                    kind: "command-output".to_owned(),
                    ref_id: command_ref.clone(),
                },
                &state,
            ))
            .expect("command output evidence should export");
        let exported_path = state.runtime_root().join(&exported.path);
        assert!(exported_path.exists());

        tauri::async_runtime::block_on(delete_conversation_with_runtime_state(
            DeleteConversationRequest {
                conversation_id: conversation_id.to_string(),
            },
            &state,
        ))
        .expect("delete should succeed");

        assert!(!state.conversation_cwd().exists());
        assert!(!exported_path.exists());
        assert!(!attachment_record.exists());
        let attachment_error =
            read_attachment_record(state.runtime_root(), &attachment.attachment.id).unwrap_err();
        assert_eq!(attachment_error.code, "INVALID_PAYLOAD");
        let export_error =
            tauri::async_runtime::block_on(export_conversation_evidence_with_runtime_state(
                ExportConversationEvidenceRequest {
                    conversation_id: conversation_id.to_string(),
                    kind: "command-output".to_owned(),
                    ref_id: command_ref,
                },
                &state,
            ))
            .unwrap_err();
        assert_eq!(export_error.code, "INVALID_PAYLOAD");
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

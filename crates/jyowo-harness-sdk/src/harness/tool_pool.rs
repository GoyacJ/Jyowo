use super::*;

impl Harness {
    pub(super) async fn inject_mcp_tools(&self) -> Result<(), HarnessError> {
        let Some(config) = &self.inner.mcp_config else {
            return Ok(());
        };
        let mut server_ids = config.server_ids_to_inject.clone();
        for server_id in config.registry.ready_plugin_server_ids().await {
            if !server_ids.contains(&server_id) {
                server_ids.push(server_id);
            }
        }
        for server_id in &server_ids {
            config
                .registry
                .inject_tools_into(&self.inner.tool_registry, server_id)
                .await
                .map_err(|error| HarnessError::Other(error.to_string()))?;
        }
        Ok(())
    }

    #[cfg(feature = "tool-search")]
    pub(super) fn install_tool_search_runtime(
        &self,
        options: &SessionOptions,
        tools: &mut ToolPool,
        cap_registry: &mut CapabilityRegistry,
        model_snapshot: &ModelRuntimeSnapshot,
    ) {
        if matches!(options.tool_search, ToolSearchMode::Disabled) {
            return;
        }
        if let Some(allowed_tools) = &self.inner.options.tenant_policy.allowed_tools {
            if !allowed_tools.contains("tool_search") {
                return;
            }
        }
        let runtime = SdkToolSearchRuntime {
            tools: tools.clone(),
            model_caps: Arc::new(model_snapshot.conversation_capability.clone()),
            mcp_config: self.inner.mcp_config.clone(),
            event_store: Arc::clone(&self.inner.event_store),
            hooks: HookDispatcher::new(self.inner.hook_registry.snapshot()),
            tenant_id: options.tenant_id,
            session_id: options.session_id,
            redactor: self.hook_redactor(),
        };
        let runtime: Arc<dyn harness_tool_search::ToolSearchRuntimeCap> = Arc::new(runtime);
        cap_registry.install::<dyn harness_tool_search::ToolSearchRuntimeCap>(
            ToolCapability::Custom(harness_tool_search::TOOL_SEARCH_RUNTIME_CAPABILITY.to_owned()),
            runtime,
        );
        tools.append_runtime_tool(Arc::new({
            let mut builder =
                harness_tool_search::ToolSearchTool::builder().with_coalesce_window(Duration::ZERO);
            if let Some(scorer) = &self.inner.tool_search_scorer {
                builder = builder.with_scorer(Arc::clone(scorer));
            }
            builder.build()
        }));
    }
}

pub(super) fn filter_unavailable_tools(
    snapshot: &ToolRegistrySnapshot,
    cap_registry: &CapabilityRegistry,
) -> ToolPoolFilter {
    let mut filter = ToolPoolFilter::default();
    for descriptor in snapshot.as_descriptors() {
        if descriptor
            .required_capabilities
            .iter()
            .any(|capability| !cap_registry.contains(capability))
        {
            filter.denylist.insert(descriptor.name.clone());
        }
    }
    filter
}

pub fn filter_unrouted_service_tools(
    filter: &mut ToolPoolFilter,
    snapshot: &ToolRegistrySnapshot,
    routes: &ProviderCapabilityRouteSettings,
) {
    for descriptor in snapshot.as_descriptors() {
        let Some(binding) = descriptor.service_binding.as_ref() else {
            continue;
        };
        let routed = routes.routes.iter().any(|route| {
            route.enabled
                && route.kind == binding.route_kind
                && route.provider_id == binding.provider_id
                && route
                    .operation_ids
                    .iter()
                    .any(|operation_id| operation_id == &binding.operation_id)
        });
        if !routed {
            filter.denylist.insert(descriptor.name.clone());
        }
    }
}

pub(super) fn apply_tenant_tool_filter(filter: &mut ToolPoolFilter, policy: &TenantPolicy) {
    if let Some(allowed_tools) = &policy.allowed_tools {
        filter.allowlist = Some(match filter.allowlist.take() {
            Some(existing) => existing
                .intersection(allowed_tools)
                .cloned()
                .collect::<HashSet<_>>(),
            None => allowed_tools.clone(),
        });
    }
}

#[cfg(feature = "agents-subagent")]
pub(super) struct SubagentSessionAssembly {
    pub(super) engine_factory: Arc<harness_engine::EngineBoundSubagentFactory>,
}

#[cfg(feature = "agents-subagent")]
pub(super) fn install_subagent_runner_for_run(
    cap_registry: &mut CapabilityRegistry,
    agent_run_options: &harness_contracts::AgentRunOptions,
    event_store: Arc<dyn EventStore>,
    workspace_root: &Path,
    team_attribution: Option<harness_agent_runtime::SubagentTeamAttribution>,
) -> SubagentSessionAssembly {
    let engine_factory = Arc::new(harness_engine::EngineBoundSubagentFactory::default());
    let runner = harness_agent_runtime::assemble_subagent_runner(
        harness_agent_runtime::SubagentRunnerAssemblyInput {
            agent_run_options: agent_run_options.clone(),
            engine_factory: Arc::clone(&engine_factory)
                as Arc<dyn harness_subagent::SubagentEngineFactory>,
            event_store,
            workspace_root: workspace_root.to_path_buf(),
            team_attribution: team_attribution.clone(),
        },
    );
    harness_agent_runtime::install_subagent_runner_capability(
        cap_registry,
        runner,
        team_attribution,
    );
    SubagentSessionAssembly { engine_factory }
}

#[cfg(feature = "agents-subagent")]
pub(super) fn subagent_tool_should_be_enabled(
    harness_has_runner: bool,
    agent_run_options: Option<&harness_contracts::AgentRunOptions>,
) -> bool {
    match agent_run_options {
        Some(options) => harness_agent_runtime::should_install_subagent_runner(options),
        None => harness_has_runner,
    }
}

#[cfg(feature = "tool-search")]
#[derive(Clone)]
struct SdkToolSearchRuntime {
    tools: ToolPool,
    model_caps: Arc<harness_model::ConversationModelCapability>,
    mcp_config: Option<McpConfig>,
    event_store: Arc<dyn EventStore>,
    hooks: HookDispatcher,
    tenant_id: TenantId,
    session_id: harness_contracts::SessionId,
    redactor: Arc<dyn Redactor>,
}

#[cfg(feature = "tool-search")]
impl SdkToolSearchRuntime {
    async fn emit_hook_events(
        &self,
        kind: harness_contracts::HookEventKind,
        result: &DispatchResult,
    ) -> Result<(), harness_contracts::ToolError> {
        for event in sdk_hook_events(kind, result, None) {
            self.event_store
                .append(self.tenant_id, self.session_id, &[event])
                .await
                .map_err(|error| harness_contracts::ToolError::Internal(error.to_string()))?;
        }
        Ok(())
    }
}

#[cfg(feature = "tool-search")]
#[async_trait]
impl harness_tool_search::ToolSearchRuntimeCap for SdkToolSearchRuntime {
    async fn snapshot(
        &self,
    ) -> Result<harness_tool_search::ToolSearchRuntimeSnapshot, harness_contracts::ToolError> {
        let loaded_tool_names = loaded_tool_names(&self.tools);
        let pending_mcp_servers = match &self.mcp_config {
            Some(config) => config
                .registry
                .pending_mcp_servers_for_tool_search(&config.server_ids_to_inject)
                .await
                .into_iter()
                .map(|server_id| server_id.0)
                .collect(),
            None => Vec::new(),
        };
        Ok(harness_tool_search::ToolSearchRuntimeSnapshot {
            deferred_tools: self
                .tools
                .deferred()
                .iter()
                .filter(|tool| !loaded_tool_names.contains(&tool.descriptor().name))
                .map(|tool| tool.descriptor().clone())
                .collect(),
            loaded_tool_names,
            discovered_tool_names: BTreeSet::new(),
            pending_mcp_servers,
            model_caps: Arc::clone(&self.model_caps),
            reload_handle: Some(Arc::new(SdkToolSearchReloadHandle {
                tools: self.tools.clone(),
            })),
        })
    }

    async fn emit_event(&self, event: Event) -> Result<(), harness_contracts::ToolError> {
        self.event_store
            .append(self.tenant_id, self.session_id, &[event])
            .await
            .map(|_| ())
            .map_err(|error| harness_contracts::ToolError::Internal(error.to_string()))
    }

    async fn dispatch_pre_tool_search_hook(
        &self,
        ctx: &harness_tool::ToolContext,
        tool_use_id: harness_contracts::ToolUseId,
        query: &str,
        query_kind: harness_contracts::ToolSearchQueryKind,
    ) -> Result<harness_tool_search::ToolSearchPreHookOutcome, harness_contracts::ToolError> {
        let result = self
            .hooks
            .dispatch(
                HookEvent::PreToolSearch {
                    tool_use_id,
                    query: query.to_owned(),
                    query_kind,
                },
                tool_search_hook_context(ctx, Arc::clone(&self.redactor)),
            )
            .await
            .map_err(|error| harness_contracts::ToolError::Internal(error.to_string()))?;
        self.emit_hook_events(harness_contracts::HookEventKind::PreToolSearch, &result)
            .await?;
        match result.final_outcome {
            HookOutcome::Continue => Ok(harness_tool_search::ToolSearchPreHookOutcome::Continue),
            HookOutcome::Block { reason } => {
                Ok(harness_tool_search::ToolSearchPreHookOutcome::Block { reason })
            }
            HookOutcome::RewriteInput(value) => Ok(
                harness_tool_search::ToolSearchPreHookOutcome::RewriteInput(value),
            ),
            _ => Ok(harness_tool_search::ToolSearchPreHookOutcome::Continue),
        }
    }

    async fn dispatch_post_tool_search_hook(
        &self,
        ctx: &harness_tool::ToolContext,
        tool_use_id: harness_contracts::ToolUseId,
        materialized: Vec<harness_contracts::ToolName>,
        backend: harness_contracts::ToolLoadingBackendName,
        cache_impact: harness_contracts::CacheImpact,
    ) -> Result<(), harness_contracts::ToolError> {
        let result = self
            .hooks
            .dispatch(
                HookEvent::PostToolSearchMaterialize {
                    tool_use_id,
                    materialized,
                    backend,
                    cache_impact,
                },
                tool_search_hook_context(ctx, Arc::clone(&self.redactor)),
            )
            .await
            .map_err(|error| harness_contracts::ToolError::Internal(error.to_string()))?;
        self.emit_hook_events(
            harness_contracts::HookEventKind::PostToolSearchMaterialize,
            &result,
        )
        .await
    }
}

#[cfg(feature = "tool-search")]
struct SdkToolSearchReloadHandle {
    tools: ToolPool,
}

#[cfg(feature = "tool-search")]
#[async_trait]
impl harness_tool_search::ReloadHandle for SdkToolSearchReloadHandle {
    async fn reload_with_add_tools(
        &self,
        tools: Vec<harness_contracts::ToolName>,
    ) -> Result<CacheImpact, HarnessError> {
        let materialized = self.tools.materialize_deferred_tools(&tools);
        if materialized.len() != tools.len() {
            let missing = tools
                .into_iter()
                .find(|tool| !materialized.contains(tool))
                .unwrap_or_else(|| "unknown".to_owned());
            return Err(HarnessError::ToolNotFound(missing));
        }
        Ok(CacheImpact {
            prompt_cache_invalidated: true,
            reason: Some("tool_search_inline_reinjection".to_owned()),
        })
    }
}

#[cfg(feature = "tool-search")]
fn tool_search_hook_context(
    ctx: &harness_tool::ToolContext,
    redactor: Arc<dyn Redactor>,
) -> HookContext {
    HookContext {
        tenant_id: ctx.tenant_id,
        session_id: ctx.session_id,
        run_id: Some(ctx.run_id),
        turn_index: None,
        correlation_id: ctx.correlation_id,
        causation_id: harness_contracts::CausationId::new(),
        trust_level: TrustLevel::AdminTrusted,
        permission_mode: PermissionMode::Default,
        interactivity: InteractivityLevel::NoInteractive,
        at: harness_contracts::now(),
        view: Arc::new(ToolSearchHookView {
            workspace_root: ctx.workspace_root.clone(),
            redactor,
        }),
        upstream_outcome: None,
        replay_mode: ReplayMode::Live,
    }
}

#[cfg(feature = "tool-search")]
struct ToolSearchHookView {
    workspace_root: PathBuf,
    redactor: Arc<dyn Redactor>,
}

#[cfg(feature = "tool-search")]
impl HookSessionView for ToolSearchHookView {
    fn workspace_root(&self) -> Option<&Path> {
        Some(&self.workspace_root)
    }

    fn recent_messages(&self, _limit: usize) -> Vec<HookMessageView> {
        Vec::new()
    }

    fn permission_mode(&self) -> PermissionMode {
        PermissionMode::Default
    }

    fn redacted(&self) -> &dyn Redactor {
        self.redactor.as_ref()
    }

    fn current_tool_descriptor(&self) -> Option<ToolDescriptorView> {
        None
    }
}

#[cfg(feature = "tool-search")]
pub(super) fn loaded_tool_names(tools: &ToolPool) -> BTreeSet<String> {
    tools
        .prompt_visible_descriptors()
        .into_iter()
        .map(|descriptor| descriptor.name)
        .collect()
}

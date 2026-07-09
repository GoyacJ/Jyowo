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
        tool_search: &ToolSearchMode,
        tools: &mut ToolPool,
        cap_registry: &mut CapabilityRegistry,
        model_snapshot: &ModelRuntimeSnapshot,
    ) {
        if matches!(tool_search, ToolSearchMode::Disabled) {
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
    agent_tool_policy: &harness_contracts::AgentToolPolicy,
    event_store: Arc<dyn EventStore>,
    workspace_root: &Path,
    team_attribution: Option<harness_agent_runtime::SubagentTeamAttribution>,
) -> SubagentSessionAssembly {
    let engine_factory = Arc::new(harness_engine::EngineBoundSubagentFactory::default());
    let runner = harness_agent_runtime::assemble_subagent_runner(
        harness_agent_runtime::SubagentRunnerAssemblyInput {
            agent_tool_policy: agent_tool_policy.clone(),
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
    agent_tool_policy: Option<&harness_contracts::AgentToolPolicy>,
) -> bool {
    match agent_tool_policy {
        Some(options) => harness_agent_runtime::should_install_subagent_runner(options),
        None => harness_has_runner,
    }
}

#[cfg(feature = "agents-team")]
const AGENT_TEAM_RUNNER_CAPABILITY: &str = "jyowo.agent_team.runner";
#[cfg(feature = "agents-team")]
const AGENT_RUNTIME_ROOT_CAPABILITY: &str = "jyowo.agent_runtime.root";

#[cfg(feature = "agents-team")]
trait AgentRuntimeRootCap: Send + Sync + 'static {
    fn agent_runtime_root(&self) -> PathBuf;
}

#[cfg(feature = "agents-team")]
struct FixedAgentRuntimeRootCap {
    root: PathBuf,
}

#[cfg(feature = "agents-team")]
impl AgentRuntimeRootCap for FixedAgentRuntimeRootCap {
    fn agent_runtime_root(&self) -> PathBuf {
        self.root.clone()
    }
}

#[cfg(feature = "agents-team")]
pub(super) fn install_agent_runtime_root_capability(
    cap_registry: &mut CapabilityRegistry,
    options: &SessionOptions,
) {
    let Some(root) = options.agent_runtime_root.clone() else {
        return;
    };
    cap_registry.install::<dyn AgentRuntimeRootCap>(
        ToolCapability::Custom(AGENT_RUNTIME_ROOT_CAPABILITY.to_owned()),
        Arc::new(FixedAgentRuntimeRootCap { root }),
    );
}

#[cfg(feature = "agents-subagent")]
pub(crate) const BACKGROUND_AGENT_STARTER_CAPABILITY: &str = "jyowo.background_agent.starter";

#[cfg(feature = "agents-team")]
#[async_trait]
trait AgentTeamRunnerCap: Send + Sync + 'static {
    async fn start_team(
        &self,
        request: AgentTeamToolStartRequest,
    ) -> Result<harness_contracts::TeamId, ToolError>;
}

#[cfg(feature = "agents-team")]
#[derive(Clone)]
struct AgentTeamToolStartRequest {
    run_id: RunId,
    conversation_session_id: SessionId,
    goal: String,
    topology: harness_contracts::AgentTeamTopology,
    max_turns_per_goal: u32,
    workspace_root: PathBuf,
    project_workspace_root: Option<PathBuf>,
    agent_runtime_root: Option<PathBuf>,
}

#[cfg(feature = "agents-team")]
struct SdkAgentTeamRunner {
    harness: Harness,
    agent_tool_policy: harness_contracts::AgentToolPolicy,
    workspace_bootstrap: Option<WorkspaceBootstrap>,
    agent_profiles: Vec<harness_contracts::AgentProfile>,
}

#[cfg(feature = "agents-team")]
#[async_trait]
impl AgentTeamRunnerCap for SdkAgentTeamRunner {
    async fn start_team(
        &self,
        request: AgentTeamToolStartRequest,
    ) -> Result<harness_contracts::TeamId, ToolError> {
        if self.harness.has_active_run_team(request.run_id) {
            return Err(ToolError::Validation(
                "an agent team is already active for this run".to_owned(),
            ));
        }
        let profiles = if self.agent_profiles.is_empty() {
            match request.agent_runtime_root.as_ref() {
                Some(runtime_root) => crate::list_agent_profiles_from_runtime_dir(runtime_root),
                None => crate::list_agent_profiles(&request.workspace_root),
            }
            .map_err(|error| ToolError::Internal(error.to_string()))?
        } else {
            self.agent_profiles.clone()
        };
        let mut agent_tool_policy = self.agent_tool_policy.clone();
        agent_tool_policy.agent_team = harness_contracts::AgentUsePolicy::Allowed;
        agent_tool_policy.team_config = Some(harness_contracts::AgentTeamRunConfig {
            topology: request.topology,
            lead_profile_id: "reviewer".to_owned(),
            member_profile_ids: vec!["worker".to_owned()],
            max_turns_per_goal: request.max_turns_per_goal,
            shared_memory_policy: harness_contracts::AgentTeamSharedMemoryPolicy::SummariesOnly,
        });
        let team = self
            .harness
            .start_run_scoped_team(super::team_runtime::RunScopedTeamStartupRequest {
                agent_tool_policy,
                profiles,
                run_id: request.run_id,
                conversation_session_id: request.conversation_session_id,
                goal: request.goal,
                workspace_root: request.workspace_root,
                project_workspace_root: request.project_workspace_root,
                agent_runtime_root: request.agent_runtime_root,
                workspace_bootstrap: self.workspace_bootstrap.clone(),
            })
            .await
            .map_err(|error| ToolError::Internal(error.to_string()))?;
        Ok(team.spec().team_id)
    }
}

#[cfg(feature = "agents-team")]
pub(super) fn install_agent_team_tool_for_run(
    harness: Harness,
    cap_registry: &mut CapabilityRegistry,
    tools: &mut ToolPool,
    agent_tool_policy: &harness_contracts::AgentToolPolicy,
    workspace_bootstrap: Option<WorkspaceBootstrap>,
    agent_profiles: Vec<harness_contracts::AgentProfile>,
) {
    cap_registry.install::<dyn AgentTeamRunnerCap>(
        ToolCapability::Custom(AGENT_TEAM_RUNNER_CAPABILITY.to_owned()),
        Arc::new(SdkAgentTeamRunner {
            harness,
            agent_tool_policy: agent_tool_policy.clone(),
            workspace_bootstrap,
            agent_profiles,
        }),
    );
    tools.append_runtime_tool(Arc::new(AgentTeamTool::default()));
}

#[cfg(feature = "agents-subagent")]
pub(super) fn install_background_agent_tool_for_run(
    cap_registry: &CapabilityRegistry,
    tools: &mut ToolPool,
    agent_tool_policy: &harness_contracts::AgentToolPolicy,
    model_config_id: Option<String>,
    permission_mode: harness_contracts::PermissionMode,
    session_snapshot: harness_contracts::BackgroundAgentToolSessionSnapshot,
) {
    if agent_tool_policy.background_agents != harness_contracts::AgentUsePolicy::Allowed {
        return;
    }
    let capability = ToolCapability::Custom(BACKGROUND_AGENT_STARTER_CAPABILITY.to_owned());
    if !cap_registry.contains(&capability) {
        return;
    }
    tools.append_runtime_tool(Arc::new(BackgroundAgentTool::new(
        agent_tool_policy.clone(),
        model_config_id,
        permission_mode,
        session_snapshot,
    )));
}

#[cfg(feature = "agents-subagent")]
struct BackgroundAgentTool {
    descriptor: ToolDescriptor,
    agent_tool_policy: harness_contracts::AgentToolPolicy,
    model_config_id: Option<String>,
    permission_mode: harness_contracts::PermissionMode,
    session_snapshot: harness_contracts::BackgroundAgentToolSessionSnapshot,
}

#[cfg(feature = "agents-subagent")]
impl BackgroundAgentTool {
    fn new(
        agent_tool_policy: harness_contracts::AgentToolPolicy,
        model_config_id: Option<String>,
        permission_mode: harness_contracts::PermissionMode,
        session_snapshot: harness_contracts::BackgroundAgentToolSessionSnapshot,
    ) -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: "background_agent".to_owned(),
                display_name: "Background Agent".to_owned(),
                description: "Start a durable background agent for a follow-up goal.".to_owned(),
                category: "builtin".to_owned(),
                group: ToolGroup::Coordinator,
                version: "0.1.0".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["goal"],
                    "properties": {
                        "goal": { "type": "string", "minLength": 1 },
                        "title": { "type": "string", "minLength": 1 }
                    },
                    "additionalProperties": false
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "required": ["backgroundAgentId", "status", "conversationId", "parentRunId", "title"],
                    "properties": {
                        "backgroundAgentId": { "type": "string" },
                        "status": { "type": "string" },
                        "conversationId": { "type": "string" },
                        "parentRunId": { "type": "string" },
                        "title": { "type": "string" }
                    },
                    "additionalProperties": false
                })),
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: false,
                    is_read_only: false,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: harness_contracts::DeferPolicy::AlwaysLoad,
                },
                trust_level: harness_contracts::TrustLevel::AdminTrusted,
                required_capabilities: vec![ToolCapability::Custom(
                    BACKGROUND_AGENT_STARTER_CAPABILITY.to_owned(),
                )],
                budget: harness_contracts::ResultBudget {
                    metric: harness_contracts::BudgetMetric::Chars,
                    limit: 4_000,
                    on_overflow: harness_contracts::OverflowAction::Offload,
                    preview_head_chars: 1_000,
                    preview_tail_chars: 1_000,
                },
                provider_restriction: harness_contracts::ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
                search_hint: Some("start durable background agent".to_owned()),
                service_binding: None,
                metadata: Default::default(),
            },
            agent_tool_policy,
            model_config_id,
            permission_mode,
            session_snapshot,
        }
    }
}

#[cfg(feature = "agents-subagent")]
#[async_trait]
impl Tool for BackgroundAgentTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        let goal = input
            .get("goal")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if goal.is_empty() {
            return Err(ValidationError::Message("goal is required".to_owned()));
        }
        if let Some(title) = input.get("title") {
            let title = title.as_str().unwrap_or_default().trim();
            if title.is_empty() {
                return Err(ValidationError::Message(
                    "title must be non-empty when provided".to_owned(),
                ));
            }
        }
        Ok(())
    }

    async fn plan(
        &self,
        input: &Value,
        ctx: &ToolContext,
    ) -> Result<harness_contracts::ToolActionPlan, ToolError> {
        harness_tool::action_plan_from_permission_check(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::Allowed,
            vec![harness_contracts::ActionResource::TeamControl {
                action: "background_agent".to_owned(),
                target: tool_goal_target(input),
            }],
            harness_contracts::WorkspaceAccess::None,
            harness_contracts::NetworkAccess::None,
            harness_contracts::ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: harness_tool::AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input().clone();
        let goal = input
            .get("goal")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::Validation("goal is required".to_owned()))?
            .trim()
            .to_owned();
        let title = input
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| goal.lines().next().unwrap_or("Background agent"))
            .to_owned();
        let starter = ctx.capability::<dyn harness_contracts::BackgroundAgentStarterCap>(
            ToolCapability::Custom(BACKGROUND_AGENT_STARTER_CAPABILITY.to_owned()),
        )?;
        let response = starter
            .start_background_agent(harness_contracts::BackgroundAgentToolStartRequest {
                tenant_id: ctx.tenant_id,
                conversation_id: ctx.session_id,
                parent_run_id: ctx.run_id,
                tool_use_id: ctx.tool_use_id,
                goal: goal.clone(),
                title: title.clone(),
                model_config_id: self
                    .model_config_id
                    .clone()
                    .or_else(|| ctx.model_config_id.clone()),
                permission_mode: self.permission_mode,
                agent_tool_policy: self.agent_tool_policy.clone(),
                session: self.session_snapshot.clone(),
            })
            .await?;
        Ok(Box::pin(futures::stream::iter([ToolEvent::Final(
            ToolResult::Structured(json!({
                "backgroundAgentId": response.background_agent_id,
                "status": response.status,
                "conversationId": response.conversation_id.to_string(),
                "parentRunId": response.parent_run_id.to_string(),
                "title": response.title,
            })),
        )])))
    }
}

#[cfg(feature = "agents-team")]
struct AgentTeamTool {
    descriptor: ToolDescriptor,
}

#[cfg(feature = "agents-team")]
impl Default for AgentTeamTool {
    fn default() -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: "agent_team".to_owned(),
                display_name: "Agent Team".to_owned(),
                description: "Start one run-scoped agent team for a coordinated goal.".to_owned(),
                category: "builtin".to_owned(),
                group: ToolGroup::Coordinator,
                version: "0.1.0".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["goal"],
                    "properties": {
                        "goal": { "type": "string", "minLength": 1 },
                        "topology": {
                            "type": "string",
                            "enum": ["coordinator_worker", "peer_to_peer", "role_routed"]
                        },
                        "maxTurnsPerGoal": {
                            "type": "integer",
                            "minimum": 1,
                            "default": 4
                        }
                    },
                    "additionalProperties": false
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "required": ["team_id", "status", "goal", "topology", "leadProfileId", "memberProfileIds", "sharedMemoryPolicy", "maxTurnsPerGoal"],
                    "properties": {
                        "team_id": { "type": "string" },
                        "status": { "type": "string" },
                        "goal": { "type": "string" },
                        "topology": { "type": "string" },
                        "leadProfileId": { "type": "string" },
                        "memberProfileIds": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "sharedMemoryPolicy": { "type": "string" },
                        "maxTurnsPerGoal": { "type": "integer", "minimum": 1 }
                    },
                    "additionalProperties": false
                })),
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: false,
                    is_read_only: false,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: harness_contracts::DeferPolicy::AlwaysLoad,
                },
                trust_level: harness_contracts::TrustLevel::AdminTrusted,
                required_capabilities: vec![ToolCapability::Custom(
                    AGENT_TEAM_RUNNER_CAPABILITY.to_owned(),
                )],
                budget: harness_contracts::ResultBudget {
                    metric: harness_contracts::BudgetMetric::Chars,
                    limit: 4_000,
                    on_overflow: harness_contracts::OverflowAction::Offload,
                    preview_head_chars: 1_000,
                    preview_tail_chars: 1_000,
                },
                provider_restriction: harness_contracts::ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
                search_hint: Some("start coordinated agent team".to_owned()),
                service_binding: None,
                metadata: Default::default(),
            },
        }
    }
}

#[cfg(feature = "agents-team")]
#[async_trait]
impl Tool for AgentTeamTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        let goal = input
            .get("goal")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if goal.is_empty() {
            return Err(ValidationError::Message("goal is required".to_owned()));
        }
        if let Some(max_turns) = input.get("maxTurnsPerGoal").and_then(Value::as_u64) {
            if max_turns == 0 {
                return Err(ValidationError::Message(
                    "maxTurnsPerGoal must be at least 1".to_owned(),
                ));
            }
        }
        Ok(())
    }

    async fn plan(
        &self,
        input: &Value,
        ctx: &ToolContext,
    ) -> Result<harness_contracts::ToolActionPlan, ToolError> {
        harness_tool::action_plan_from_permission_check(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::Allowed,
            vec![harness_contracts::ActionResource::TeamControl {
                action: "agent_team".to_owned(),
                target: tool_goal_target(input),
            }],
            harness_contracts::WorkspaceAccess::None,
            harness_contracts::NetworkAccess::None,
            harness_contracts::ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: harness_tool::AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input().clone();
        let goal = input
            .get("goal")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::Validation("goal is required".to_owned()))?
            .trim()
            .to_owned();
        let topology = match input.get("topology").and_then(Value::as_str) {
            Some("peer_to_peer") => harness_contracts::AgentTeamTopology::PeerToPeer,
            Some("role_routed") => harness_contracts::AgentTeamTopology::RoleRouted,
            Some("coordinator_worker") | None => {
                harness_contracts::AgentTeamTopology::CoordinatorWorker
            }
            Some(other) => {
                return Err(ToolError::Validation(format!(
                    "unsupported team topology: {other}"
                )));
            }
        };
        let max_turns_per_goal = input
            .get("maxTurnsPerGoal")
            .and_then(Value::as_u64)
            .unwrap_or(4)
            .try_into()
            .map_err(|_| ToolError::Validation("maxTurnsPerGoal is too large".to_owned()))?;
        let runner = ctx.capability::<dyn AgentTeamRunnerCap>(ToolCapability::Custom(
            AGENT_TEAM_RUNNER_CAPABILITY.to_owned(),
        ))?;
        let agent_runtime_root = ctx
            .cap_registry
            .get::<dyn AgentRuntimeRootCap>(&ToolCapability::Custom(
                AGENT_RUNTIME_ROOT_CAPABILITY.to_owned(),
            ))
            .map(|capability| capability.agent_runtime_root());
        let team_id = runner
            .start_team(AgentTeamToolStartRequest {
                run_id: ctx.run_id,
                conversation_session_id: ctx.session_id,
                goal: goal.clone(),
                topology,
                max_turns_per_goal,
                workspace_root: ctx.workspace_root,
                project_workspace_root: ctx.project_workspace_root,
                agent_runtime_root,
            })
            .await?;
        Ok(Box::pin(futures::stream::iter([ToolEvent::Final(
            ToolResult::Structured(json!({
                "team_id": team_id.to_string(),
                "status": "started",
                "goal": goal,
                "leadProfileId": "reviewer",
                "memberProfileIds": ["worker"],
                "topology": topology,
                "sharedMemoryPolicy": "summaries_only",
                "maxTurnsPerGoal": max_turns_per_goal
            })),
        )])))
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
            workspace_root: ctx.project_workspace_root.clone(),
            redactor,
        }),
        upstream_outcome: None,
        replay_mode: ReplayMode::Live,
    }
}

#[cfg(any(feature = "agents-subagent", feature = "agents-team"))]
fn tool_goal_target(input: &Value) -> Option<String> {
    input
        .get("goal")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|goal| !goal.is_empty())
        .map(|goal| format!("goal:{goal}"))
}

#[cfg(feature = "tool-search")]
struct ToolSearchHookView {
    workspace_root: Option<PathBuf>,
    redactor: Arc<dyn Redactor>,
}

#[cfg(feature = "tool-search")]
impl HookSessionView for ToolSearchHookView {
    fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
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

#[cfg(all(test, feature = "agents-team"))]
mod tests {
    use super::*;
    use futures::StreamExt;
    use harness_contracts::{
        ActionResource, CorrelationId, NoopRedactor, PermissionActorSource, ToolActionPlan,
        ToolUseId,
    };
    use harness_tool::{
        AuthorizationTicketClaims, AuthorizedTicketSummary, AuthorizedToolInput, InterruptToken,
        TicketLedger,
    };
    use std::sync::Mutex;

    #[test]
    fn agent_team_tool_preserves_no_workspace_project_context() {
        futures::executor::block_on(async {
            let execution_cwd = std::env::temp_dir().join(format!(
                "jyowo-agent-team-no-workspace-{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&execution_cwd).expect("execution cwd");
            let captured = Arc::new(Mutex::new(None));
            let mut cap_registry = CapabilityRegistry::default();
            cap_registry.install::<dyn AgentTeamRunnerCap>(
                ToolCapability::Custom(AGENT_TEAM_RUNNER_CAPABILITY.to_owned()),
                Arc::new(CapturingAgentTeamRunner {
                    captured: Arc::clone(&captured),
                }),
            );
            cap_registry.install::<dyn AgentRuntimeRootCap>(
                ToolCapability::Custom(AGENT_RUNTIME_ROOT_CAPABILITY.to_owned()),
                Arc::new(TestAgentRuntimeRoot {
                    root: execution_cwd.join("runtime"),
                }),
            );
            let ctx = ToolContext {
                tool_use_id: ToolUseId::new(),
                run_id: RunId::new(),
                session_id: SessionId::new(),
                tenant_id: TenantId::SINGLE,
                model: None,
                model_config_id: None,
                memory_thread_settings: None,
                correlation_id: CorrelationId::new(),
                agent_id: AgentId::new(),
                subagent_depth: 0,
                workspace_root: execution_cwd.clone(),
                project_workspace_root: None,
                sandbox: None,
                cap_registry: Arc::new(cap_registry),
                redactor: Arc::new(NoopRedactor),
                interrupt: InterruptToken::new(),
                parent_run: None,
                actor_source: PermissionActorSource::ParentRun,
            };
            let tool = AgentTeamTool::default();
            let input = json!({ "goal": "inspect no-workspace" });
            tool.validate(&input, &ctx).await.expect("validate");
            let plan = tool.plan(&input, &ctx).await.expect("plan");
            assert!(matches!(
                plan.resources.as_slice(),
                [ActionResource::TeamControl { action, target }]
                    if action == "agent_team" && target.as_deref() == Some("goal:inspect no-workspace")
            ));
            let authorized =
                AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).expect("ticket");
            let mut stream = tool
                .execute_authorized(authorized, ctx)
                .await
                .expect("execute");
            let event = stream.next().await.expect("final event");
            let ToolEvent::Final(ToolResult::Structured(output)) = event else {
                panic!("expected structured final event");
            };
            assert_eq!(output["status"], json!("started"));
            assert_eq!(output["goal"], json!("inspect no-workspace"));
            assert_eq!(output["topology"], json!("coordinator_worker"));
            assert_eq!(output["maxTurnsPerGoal"], json!(4));
            assert!(output["team_id"]
                .as_str()
                .is_some_and(|value| !value.is_empty()));

            let request = captured
                .lock()
                .expect("captured lock")
                .clone()
                .expect("captured request");
            assert_eq!(request.workspace_root, execution_cwd);
            assert_eq!(request.project_workspace_root, None);
        });
    }

    #[test]
    fn agent_team_tool_reports_missing_runner_capability() {
        futures::executor::block_on(async {
            let ctx = test_context(std::env::temp_dir());
            let tool = AgentTeamTool::default();
            let input = json!({ "goal": "inspect missing runner" });
            let plan = tool.plan(&input, &ctx).await.expect("plan");
            let authorized =
                AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).expect("ticket");

            let error = match tool.execute_authorized(authorized, ctx).await {
                Ok(_) => panic!("missing runner should fail"),
                Err(error) => error,
            };

            assert!(matches!(
                error,
                ToolError::CapabilityMissing(ToolCapability::Custom(ref capability))
                    if capability == AGENT_TEAM_RUNNER_CAPABILITY
            ));
        });
    }

    #[test]
    fn background_agent_tool_plan_declares_team_control_resource() {
        futures::executor::block_on(async {
            let ctx = test_context(std::env::temp_dir());
            let tool = BackgroundAgentTool::new(
                agent_tool_policy(),
                None,
                harness_contracts::PermissionMode::Default,
                background_session_snapshot(ctx.session_id),
            );
            let input = json!({
                "goal": "summarize traces",
                "title": "Trace summary"
            });

            let plan = tool.plan(&input, &ctx).await.expect("plan");

            assert!(matches!(
                plan.resources.as_slice(),
                [ActionResource::TeamControl { action, target }]
                    if action == "background_agent"
                        && target.as_deref() == Some("goal:summarize traces")
            ));
        });
    }

    #[test]
    fn background_agent_tool_executes_with_policy_snapshot_and_output_contract() {
        futures::executor::block_on(async {
            let captured = Arc::new(Mutex::new(None));
            let mut cap_registry = CapabilityRegistry::default();
            cap_registry.install::<dyn harness_contracts::BackgroundAgentStarterCap>(
                ToolCapability::Custom(BACKGROUND_AGENT_STARTER_CAPABILITY.to_owned()),
                Arc::new(CapturingBackgroundAgentStarter {
                    captured: Arc::clone(&captured),
                }),
            );
            let mut ctx = test_context(std::env::temp_dir());
            ctx.model_config_id = Some("ctx-model".to_owned());
            ctx.cap_registry = Arc::new(cap_registry);
            let session = background_session_snapshot(ctx.session_id);
            let tool = BackgroundAgentTool::new(
                agent_tool_policy(),
                Some("tool-model".to_owned()),
                harness_contracts::PermissionMode::BypassPermissions,
                session.clone(),
            );
            let input = json!({
                "goal": "summarize traces",
                "title": "Trace summary"
            });
            let plan = tool.plan(&input, &ctx).await.expect("plan");
            let authorized =
                AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).expect("ticket");

            let mut stream = tool
                .execute_authorized(authorized, ctx.clone())
                .await
                .expect("execute");
            let event = stream.next().await.expect("final event");
            let ToolEvent::Final(ToolResult::Structured(output)) = event else {
                panic!("expected structured final event");
            };
            assert_eq!(output["backgroundAgentId"], json!("background-1"));
            assert_eq!(output["status"], json!("queued"));
            assert_eq!(output["conversationId"], json!(ctx.session_id.to_string()));
            assert_eq!(output["parentRunId"], json!(ctx.run_id.to_string()));
            assert_eq!(output["title"], json!("Trace summary"));

            let request = captured
                .lock()
                .expect("captured lock")
                .clone()
                .expect("captured request");
            assert_eq!(request.goal, "summarize traces");
            assert_eq!(request.title, "Trace summary");
            assert_eq!(request.model_config_id.as_deref(), Some("tool-model"));
            assert_eq!(
                request.permission_mode,
                harness_contracts::PermissionMode::BypassPermissions
            );
            assert_eq!(request.session, session);
            assert_eq!(
                request.agent_tool_policy.background_agents,
                harness_contracts::AgentUsePolicy::Allowed
            );
        });
    }

    #[test]
    fn background_agent_tool_reports_missing_starter_capability() {
        futures::executor::block_on(async {
            let ctx = test_context(std::env::temp_dir());
            let tool = BackgroundAgentTool::new(
                agent_tool_policy(),
                None,
                harness_contracts::PermissionMode::Default,
                background_session_snapshot(ctx.session_id),
            );
            let input = json!({ "goal": "summarize traces" });
            let plan = tool.plan(&input, &ctx).await.expect("plan");
            let authorized =
                AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).expect("ticket");

            let error = match tool.execute_authorized(authorized, ctx).await {
                Ok(_) => panic!("missing starter should fail"),
                Err(error) => error,
            };

            assert!(matches!(
                error,
                ToolError::CapabilityMissing(ToolCapability::Custom(ref capability))
                    if capability == BACKGROUND_AGENT_STARTER_CAPABILITY
            ));
        });
    }

    #[test]
    fn runtime_agent_tools_declare_strict_input_and_output_schemas() {
        let ctx = test_context(std::env::temp_dir());
        let background = BackgroundAgentTool::new(
            agent_tool_policy(),
            None,
            harness_contracts::PermissionMode::Default,
            background_session_snapshot(ctx.session_id),
        );
        assert_eq!(
            background
                .descriptor()
                .input_schema
                .get("additionalProperties"),
            Some(&serde_json::Value::Bool(false))
        );
        let background_output = background
            .descriptor()
            .output_schema
            .as_ref()
            .expect("background_agent should declare output schema");
        assert_eq!(
            background_output.get("additionalProperties"),
            Some(&serde_json::Value::Bool(false))
        );
        for field in [
            "backgroundAgentId",
            "status",
            "conversationId",
            "parentRunId",
            "title",
        ] {
            assert!(
                background_output
                    .get("required")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|required| required
                        .iter()
                        .any(|value| value.as_str() == Some(field))),
                "background_agent output should require {field}"
            );
        }

        let agent_team = AgentTeamTool::default();
        assert_eq!(
            agent_team
                .descriptor()
                .input_schema
                .get("additionalProperties"),
            Some(&serde_json::Value::Bool(false))
        );
        let agent_team_output = agent_team
            .descriptor()
            .output_schema
            .as_ref()
            .expect("agent_team should declare output schema");
        assert_eq!(
            agent_team_output.get("additionalProperties"),
            Some(&serde_json::Value::Bool(false))
        );
        for field in [
            "team_id",
            "status",
            "goal",
            "topology",
            "leadProfileId",
            "memberProfileIds",
            "sharedMemoryPolicy",
            "maxTurnsPerGoal",
        ] {
            assert!(
                agent_team_output
                    .get("required")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|required| required
                        .iter()
                        .any(|value| value.as_str() == Some(field))),
                "agent_team output should require {field}"
            );
        }
    }

    #[derive(Clone)]
    struct CapturingAgentTeamRunner {
        captured: Arc<Mutex<Option<AgentTeamToolStartRequest>>>,
    }

    #[async_trait]
    impl AgentTeamRunnerCap for CapturingAgentTeamRunner {
        async fn start_team(
            &self,
            request: AgentTeamToolStartRequest,
        ) -> Result<harness_contracts::TeamId, ToolError> {
            *self.captured.lock().expect("captured lock") = Some(request);
            Ok(harness_contracts::TeamId::new())
        }
    }

    #[derive(Clone)]
    struct CapturingBackgroundAgentStarter {
        captured: Arc<Mutex<Option<harness_contracts::BackgroundAgentToolStartRequest>>>,
    }

    impl harness_contracts::BackgroundAgentStarterCap for CapturingBackgroundAgentStarter {
        fn start_background_agent(
            &self,
            request: harness_contracts::BackgroundAgentToolStartRequest,
        ) -> futures::future::BoxFuture<
            'static,
            Result<harness_contracts::BackgroundAgentToolStartResponse, ToolError>,
        > {
            *self.captured.lock().expect("captured lock") = Some(request.clone());
            Box::pin(async move {
                Ok(harness_contracts::BackgroundAgentToolStartResponse {
                    background_agent_id: "background-1".to_owned(),
                    conversation_id: request.conversation_id,
                    parent_run_id: request.parent_run_id,
                    title: request.title,
                    status: "queued".to_owned(),
                })
            })
        }
    }

    #[derive(Clone)]
    struct TestAgentRuntimeRoot {
        root: PathBuf,
    }

    impl AgentRuntimeRootCap for TestAgentRuntimeRoot {
        fn agent_runtime_root(&self) -> PathBuf {
            self.root.clone()
        }
    }

    fn ticket_for(plan: &ToolActionPlan) -> AuthorizedTicketSummary {
        let ledger = TicketLedger::default();
        let claims = AuthorizationTicketClaims {
            tenant_id: TenantId::SINGLE,
            session_id: SessionId::new(),
            run_id: RunId::new(),
            tool_use_id: plan.tool_use_id,
            tool_name: plan.tool_name.clone(),
            action_plan_hash: plan.plan_hash.clone(),
        };
        let ticket = ledger
            .mint(claims.clone(), chrono::Utc::now())
            .expect("test ticket should mint");
        ledger
            .consume(ticket.id, &claims, chrono::Utc::now())
            .expect("test ticket should consume")
    }

    fn test_context(workspace_root: PathBuf) -> ToolContext {
        ToolContext {
            tool_use_id: ToolUseId::new(),
            run_id: RunId::new(),
            session_id: SessionId::new(),
            tenant_id: TenantId::SINGLE,
            model: None,
            model_config_id: None,
            memory_thread_settings: None,
            correlation_id: CorrelationId::new(),
            agent_id: AgentId::new(),
            subagent_depth: 0,
            workspace_root,
            project_workspace_root: None,
            sandbox: None,
            cap_registry: Arc::new(CapabilityRegistry::default()),
            redactor: Arc::new(NoopRedactor),
            interrupt: InterruptToken::new(),
            parent_run: None,
            actor_source: PermissionActorSource::ParentRun,
        }
    }

    fn agent_tool_policy() -> harness_contracts::AgentToolPolicy {
        harness_contracts::AgentToolPolicy {
            subagents: harness_contracts::AgentUsePolicy::Allowed,
            agent_team: harness_contracts::AgentUsePolicy::Allowed,
            background_agents: harness_contracts::AgentUsePolicy::Allowed,
            team_config: None,
            workspace_isolation: harness_contracts::AgentWorkspaceIsolationMode::ReadOnly,
            max_depth: 1,
            max_concurrent_subagents: 1,
            max_team_members: 1,
        }
    }

    fn background_session_snapshot(
        session_id: SessionId,
    ) -> harness_contracts::BackgroundAgentToolSessionSnapshot {
        harness_contracts::BackgroundAgentToolSessionSnapshot {
            tenant_id: TenantId::SINGLE,
            session_id,
            tool_search: harness_contracts::ToolSearchMode::Disabled,
            tool_profile: harness_contracts::ToolProfile::Full,
            permission_mode: harness_contracts::PermissionMode::Default,
            interactivity: harness_contracts::InteractivityLevel::FullyInteractive,
            team_id: None,
            max_iterations: 1,
            context_compression_trigger_ratio: 0.8,
        }
    }
}

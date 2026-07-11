//! `jyowo-harness-sdk`
//!
//! Business-facing facade for the Jyowo Agent Harness SDK.
//!
//! Status: M7 facade.
//!
//! ```compile_fail
//! # async fn demo() {
//! let _ = jyowo_harness_sdk::Harness::builder().build().await;
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod agent_runtime;
pub mod builder;
pub mod builtin;
pub mod error;
pub mod ext;
pub mod harness;
pub mod options;
pub mod prelude;
pub mod session;
mod settings_runtime;
pub mod skill_config;
pub mod skill_pack_loader;
mod system_prompt;
pub mod team;
#[cfg(feature = "testing")]
pub mod testing;

pub use agent_runtime::{
    delete_agent_profile, list_agent_profiles, list_agent_profiles_from_runtime_dir,
    resolve_agent_capabilities, resolve_agent_capabilities_with_context,
    resolve_agent_runtime_policy, save_agent_profile, AgentCapabilitiesInput,
    AgentCapabilityResolutionContext, AgentRuntimeFacadeError, AgentRuntimePolicyError,
    ExecutionSettingsAgentInput, ResolvedAgentToolPolicy,
};
pub use builder::{HarnessBuilder, Set, Unset};
pub use error::HarnessError;
#[cfg(feature = "stream-permission")]
pub use harness::StreamPermissionRuntime;
pub use harness::{
    filter_unrouted_service_tools, ConversationEventsPage, ConversationEventsPageRequest,
    ConversationRunOptions, ConversationSession, ConversationSessionSummary,
    ConversationTurnReceipt, ConversationTurnRequest, Harness, HarnessOptions,
    HarnessSamplingProvider, McpConfig, RuntimeSkillParameter, RuntimeSkillSummary,
    RuntimeSkillView, TenantPolicy, WorkspaceCreateRequest,
};
pub use harness_agent_runtime::{
    builtin_agent_profiles, default_agent_capability_environment, AgentCapabilityEnvironment,
    AgentCapabilityResolver, ResolvedAgentCapabilityPolicy,
};
pub use harness_agent_runtime::{
    AgentProfileRegistryError, AgentProfilesFile, AgentRuntimeStore, AgentRuntimeStoreError,
};
pub use harness_agent_runtime::{
    BackgroundAgentManager, BackgroundAgentRecord, BackgroundAgentStartRequest,
    BackgroundAgentTransitionError,
};
pub use harness_engine::{RunControl, RunControlHandle, TurnOutcome};
#[cfg(feature = "sqlite-store")]
pub use harness_journal::SqliteEvidenceRefRegistry;
pub use harness_journal::{
    AuditFilter, AuditOrder, AuditPage, AuditQuery, AuditRecord, AuditScope, EvidenceRefStore,
};
pub use harness_session::{BootstrapFileSpec, Workspace, WorkspaceBootstrap, WorkspaceSpec};
pub use harness_skill::{parse_skill_markdown, SkillSource};
pub use options::{
    ConfigError, ConfigSource, ConfigWarning, LastKnownGoodConfig, OptionsParseMode,
    ParsedHarnessOptions,
};
pub use session::{EventStream, RunContext, Session, SessionHandle, SessionOptions};
pub use settings_runtime::DesktopSettingsRuntime;
pub use skill_config::{
    validate_required_skill_config, SkillConfigError, SkillConfigSnapshot,
    SkillConfigSnapshotResolver,
};
pub use skill_pack_loader::{
    LockedSkillPackFile, LockedSkillVersionSnapshot, SkillPackLoaderAdapter, SkillPackLoaderError,
};
#[cfg(feature = "agents-team")]
pub use team::{Team, TeamBuilder};

pub use harness_contracts::{
    AgentId, ConversationAttachmentReference, ConversationContextReference, ConversationTurnInput,
    Event, MessageId, RunId, SessionId, TeamId, TenantId, ToolExecutionChannel, ToolUseId,
    TurnInput, WorkspaceId,
};

#[cfg(feature = "agents-team")]
pub mod agents_team {
    use std::collections::HashSet;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::thread;
    use std::time::Duration;

    use async_trait::async_trait;
    use futures::{stream, StreamExt};
    use harness_contracts::{
        ActionResource, AgentId, BudgetMetric, ContextVisibility, DeferPolicy, Event,
        MessageContent, OverflowAction, PermissionActorSource, ProviderRestriction, Recipient,
        ResultBudget, ToolActionPlan, ToolDescriptor, ToolError, ToolGroup, ToolOrigin,
        ToolProperties, ToolResult, TrustLevel, UsageSnapshot,
    };
    use harness_engine::{Engine, EngineRunner, RunContext, SessionHandle};
    use harness_team::{
        TeamControlHandle, TeamError, TeamMember, TeamMemberEngineConfig, TeamMemberRunOutcome,
        TeamMemberRunRequest, TeamMemberRunner, TeamSandboxPolicy, TeamToolsetSelector,
    };
    use harness_tool::{
        action_plan_from_permission_check, AuthorizedToolInput, PermissionCheck, Tool, ToolContext,
        ToolEvent, ToolPool, ToolPoolFilter, ToolStream, ValidationError,
    };

    #[derive(Clone)]
    pub struct EngineTeamMemberRunner {
        engine: Arc<Engine>,
    }

    impl EngineTeamMemberRunner {
        #[must_use]
        pub fn new(engine: Arc<Engine>) -> Self {
            Self { engine }
        }
    }

    #[async_trait]
    impl TeamMemberRunner for EngineTeamMemberRunner {
        async fn run_member(
            &self,
            request: TeamMemberRunRequest,
        ) -> Result<TeamMemberRunOutcome, TeamError> {
            let engine = scoped_member_engine(Arc::clone(&self.engine), &request)?;
            let session = SessionHandle {
                tenant_id: request.tenant_id,
                session_id: request.session_id,
            };
            let engine_cancellation = harness_engine::CancellationToken::new();
            let _cancellation_bridge = TeamMemberCancellationBridge::spawn(
                request.cancellation.clone(),
                engine_cancellation.clone(),
            );
            let ctx = RunContext::new(request.tenant_id, request.session_id, request.run_id)
                .with_parent_run_id(request.parent_run_id)
                .with_team_id(request.team_id)
                .with_permission_actor_source(PermissionActorSource::TeamMember {
                    team_id: request.team_id,
                    agent_id: request.agent_id,
                    role: request.role.clone(),
                    parent_run_id: request.parent_run_id,
                })
                .with_correlation_id(request.correlation_id)
                .with_permission_mode(request.engine_config.permission_mode)
                .with_interactivity(request.engine_config.interactivity)
                .with_budget_limits(team_member_budget_limits(&request.engine_config))
                .with_cancellation(engine_cancellation);
            #[cfg(feature = "memory-provider-registry")]
            let ctx =
                ctx.with_memory_thread_settings(Some(harness_contracts::MemoryThreadSettings {
                    session_id: request.session_id,
                    use_memories: None,
                    generate_memories: None,
                    memory_mode: request.engine_config.memory_mode,
                }));
            let mut stream = self
                .run_engine(&engine, session, request.input, ctx)
                .await?;

            let mut body = String::new();
            let mut usage = UsageSnapshot::default();
            let mut saw_run_end = false;
            while let Some(event) = stream.next().await {
                match event {
                    Event::AssistantMessageCompleted(event) => {
                        body = message_content_text(event.content);
                        usage = event.usage;
                    }
                    Event::RunEnded(event) => {
                        saw_run_end = true;
                        if let Some(run_usage) = event.usage {
                            usage = run_usage;
                        }
                    }
                    _ => {}
                }
            }

            if !saw_run_end {
                return Err(TeamError::Journal(
                    "member engine stream ended without RunEnded".to_owned(),
                ));
            }

            Ok(TeamMemberRunOutcome { body, usage })
        }
    }

    impl EngineTeamMemberRunner {
        async fn run_engine(
            &self,
            engine: &Engine,
            session: SessionHandle,
            input: harness_contracts::TurnInput,
            ctx: RunContext,
        ) -> Result<harness_engine::EventStream, TeamError> {
            engine.run(session, input, ctx).await.map_err(team_error)
        }
    }

    enum TeamMemberCancellationBridge {
        Tokio(tokio::task::JoinHandle<()>),
        Thread {
            stop: Arc<AtomicBool>,
            handle: Option<thread::JoinHandle<()>>,
        },
    }

    impl TeamMemberCancellationBridge {
        fn spawn(
            member_cancellation: harness_team::TeamMemberCancellationToken,
            engine_cancellation: harness_engine::CancellationToken,
        ) -> Self {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                return Self::Tokio(handle.spawn(async move {
                    member_cancellation.cancelled().await;
                    engine_cancellation.cancel(harness_engine::InterruptCause::Parent);
                }));
            }

            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = Arc::clone(&stop);
            let handle = thread::spawn(move || {
                while !worker_stop.load(Ordering::SeqCst) {
                    if member_cancellation.is_cancelled() {
                        engine_cancellation.cancel(harness_engine::InterruptCause::Parent);
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            });
            Self::Thread {
                stop,
                handle: Some(handle),
            }
        }
    }

    impl Drop for TeamMemberCancellationBridge {
        fn drop(&mut self) {
            match self {
                Self::Tokio(handle) => handle.abort(),
                Self::Thread { stop, handle } => {
                    stop.store(true, Ordering::SeqCst);
                    if let Some(handle) = handle.take() {
                        let _ = handle.join();
                    }
                }
            }
        }
    }

    fn scoped_member_engine(
        base: Arc<Engine>,
        request: &TeamMemberRunRequest,
    ) -> Result<Engine, TeamError> {
        if let Some(model_ref) = &request.engine_config.model_ref {
            if model_ref != &base.model_ref() {
                return Err(TeamError::Journal(format!(
                    "team member model_ref {}:{} does not match base engine {}:{}",
                    model_ref.provider_id,
                    model_ref.model_id,
                    base.model_ref().provider_id,
                    base.model_ref().model_id
                )));
            }
        }
        if let TeamToolsetSelector::Preset(preset) = &request.engine_config.toolset {
            return Err(TeamError::Journal(format!(
                "team member toolset preset is not configured: {preset}"
            )));
        }

        let tools = if request.control_tools_enabled {
            let control = request.team_control.clone().ok_or_else(|| {
                TeamError::Journal("coordinator control tools require team_control".to_owned())
            })?;
            coordinator_tool_pool(control, request.agent_id, Arc::clone(&base))
        } else {
            base.tool_pool()
                .filtered(&member_tool_filter(&request.engine_config))
        };
        let context = member_context_engine(&base, request)?;
        let mut builder = base
            .as_ref()
            .clone()
            .into_builder()
            .with_context(context)
            .with_tools(tools)
            .with_max_iterations(request.engine_config.max_iterations);

        builder = match &request.engine_config.sandbox_policy {
            TeamSandboxPolicy::Inherit => builder,
            TeamSandboxPolicy::Empty => builder.without_sandbox(),
            TeamSandboxPolicy::RequireBackend(required) => {
                let actual = base.sandbox_backend_id().ok_or_else(|| {
                    TeamError::Journal(format!("required sandbox is not available: {required}"))
                })?;
                if actual != *required {
                    return Err(TeamError::Journal(format!(
                        "required sandbox {required} does not match base sandbox {actual}"
                    )));
                }
                builder
            }
        };

        if let Some(addendum) = &request.engine_config.system_prompt_addendum {
            let rendered_addendum = crate::system_prompt::render_session_addendum(addendum);
            let system_prompt = match base.system_prompt() {
                Some(base_prompt) if !base_prompt.is_empty() => rendered_addendum
                    .map(|section| format!("{base_prompt}\n\n{section}"))
                    .or_else(|| Some(base_prompt.to_owned())),
                _ => rendered_addendum,
            };
            builder = builder.with_system_prompt(system_prompt);
        }

        builder
            .build()
            .map_err(|error| TeamError::Journal(error.to_string()))
    }

    fn team_member_budget_limits(
        config: &TeamMemberEngineConfig,
    ) -> Option<harness_engine::RunBudgetLimits> {
        let quota = config.quota.as_ref()?;
        Some(harness_engine::RunBudgetLimits {
            max_tokens: quota.max_tokens,
            max_tool_calls: quota.max_tool_calls,
            max_duration: quota.max_duration,
            max_cost_micros: quota
                .max_cost_cents
                .map(|cents| cents.saturating_mul(10_000)),
        })
    }

    fn member_context_engine(
        base: &Engine,
        request: &TeamMemberRunRequest,
    ) -> Result<harness_context::ContextEngine, TeamError> {
        #[cfg(feature = "memory-provider-registry")]
        if let Some(shared_memory) = request.shared_memory.clone() {
            if request.engine_config.memory_mode != harness_contracts::MemoryThreadMode::Off {
                let provider: Arc<dyn harness_memory::MemoryProvider> = Arc::new(shared_memory);
                let manager = base
                    .context_engine()
                    .memory_manager()
                    .unwrap_or_else(|| Arc::new(harness_memory::MemoryManager::new()));
                manager
                    .register_provider(provider)
                    .map_err(|error| TeamError::Journal(error.to_string()))?;
                return Ok(base.context_engine().clone_with_budget_and_memory_manager(
                    request.engine_config.token_budget,
                    Some(manager),
                ));
            }
        }
        Ok(base
            .context_engine()
            .clone_with_budget(request.engine_config.token_budget))
    }

    fn coordinator_tool_pool(
        control: TeamControlHandle,
        coordinator: AgentId,
        base_engine: Arc<Engine>,
    ) -> ToolPool {
        let mut pool = ToolPool::default();
        for kind in [
            TeamControlToolKind::Dispatch,
            TeamControlToolKind::Message,
            TeamControlToolKind::PauseWorker,
            TeamControlToolKind::ResumeWorker,
            TeamControlToolKind::SpawnWorker,
            TeamControlToolKind::StopTeam,
            TeamControlToolKind::TeamStatus,
        ] {
            pool.append_runtime_tool(Arc::new(TeamControlTool::new(
                kind,
                control.clone(),
                coordinator,
                Arc::clone(&base_engine),
            )));
        }
        pool
    }

    #[derive(Debug, Clone, Copy)]
    enum TeamControlToolKind {
        Dispatch,
        Message,
        PauseWorker,
        ResumeWorker,
        SpawnWorker,
        StopTeam,
        TeamStatus,
    }

    impl TeamControlToolKind {
        fn name(self) -> &'static str {
            match self {
                Self::Dispatch => "dispatch",
                Self::Message => "message",
                Self::PauseWorker => "pause_worker",
                Self::ResumeWorker => "resume_worker",
                Self::SpawnWorker => "spawn_worker",
                Self::StopTeam => "stop_team",
                Self::TeamStatus => "team_status",
            }
        }
    }

    struct TeamControlTool {
        kind: TeamControlToolKind,
        descriptor: ToolDescriptor,
        control: TeamControlHandle,
        coordinator: AgentId,
        base_engine: Arc<Engine>,
    }

    impl TeamControlTool {
        fn new(
            kind: TeamControlToolKind,
            control: TeamControlHandle,
            coordinator: AgentId,
            base_engine: Arc<Engine>,
        ) -> Self {
            Self {
                kind,
                descriptor: team_control_descriptor(kind),
                control,
                coordinator,
                base_engine,
            }
        }
    }

    #[async_trait]
    impl Tool for TeamControlTool {
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
                &self.descriptor,
                input,
                ctx,
                PermissionCheck::Allowed,
                vec![ActionResource::TeamControl {
                    action: self.kind.name().to_owned(),
                    target: team_control_target(input),
                }],
                harness_contracts::WorkspaceAccess::None,
                harness_contracts::NetworkAccess::None,
                harness_contracts::ToolExecutionChannel::DirectAuthorizedRust,
            )
        }

        async fn execute_authorized(
            &self,
            authorized: AuthorizedToolInput,
            _ctx: ToolContext,
        ) -> Result<ToolStream, ToolError> {
            let input = authorized.raw_input().clone();
            let result = match self.kind {
                TeamControlToolKind::Dispatch => {
                    let to = parse_recipient(&input)?;
                    let body = input
                        .get("body")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    let message = self
                        .control
                        .dispatch(self.coordinator, to, body)
                        .await
                        .map_err(team_tool_error)?;
                    ToolResult::Structured(serde_json::json!({
                        "message_id": message.message_id.to_string()
                    }))
                }
                TeamControlToolKind::Message => {
                    let to = parse_recipient(&input)?;
                    let body = input
                        .get("body")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    let message = self
                        .control
                        .message(self.coordinator, to, body)
                        .await
                        .map_err(team_tool_error)?;
                    ToolResult::Structured(serde_json::json!({
                        "message_id": message.message_id.to_string()
                    }))
                }
                TeamControlToolKind::PauseWorker => {
                    let worker = parse_agent_id(&input, "agent_id")?;
                    self.control
                        .pause_worker(worker)
                        .await
                        .map_err(team_tool_error)?;
                    ToolResult::Structured(serde_json::json!({ "paused": worker.to_string() }))
                }
                TeamControlToolKind::ResumeWorker => {
                    let worker = parse_agent_id(&input, "agent_id")?;
                    self.control
                        .resume_worker(worker)
                        .await
                        .map_err(team_tool_error)?;
                    ToolResult::Structured(serde_json::json!({ "resumed": worker.to_string() }))
                }
                TeamControlToolKind::SpawnWorker => {
                    let agent_id = input
                        .get("agent_id")
                        .and_then(serde_json::Value::as_str)
                        .map(AgentId::parse)
                        .transpose()
                        .map_err(|error| ToolError::Validation(error.to_string()))?
                        .unwrap_or_else(AgentId::new);
                    let role = input
                        .get("role")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("worker")
                        .to_owned();
                    self.control
                        .spawn_worker_with_runner(
                            TeamMember {
                                agent_id,
                                role,
                                visibility: ContextVisibility::All,
                                engine_config: TeamMemberEngineConfig::default(),
                            },
                            Arc::new(EngineTeamMemberRunner::new(Arc::clone(&self.base_engine))),
                        )
                        .await
                        .map_err(team_tool_error)?;
                    ToolResult::Structured(serde_json::json!({
                        "agent_id": agent_id.to_string()
                    }))
                }
                TeamControlToolKind::StopTeam => {
                    let report = self.control.stop_team().await.map_err(team_tool_error)?;
                    ToolResult::Structured(serde_json::json!({
                        "team_id": report.team_id.to_string(),
                        "message_count": report.message_count
                    }))
                }
                TeamControlToolKind::TeamStatus => {
                    let status = self.control.status().await;
                    ToolResult::Structured(serde_json::json!({
                        "team_id": status.team_id.to_string(),
                        "member_count": status.member_count,
                        "paused_members": status
                            .paused_members
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>(),
                        "terminated": status.terminated.map(|reason| format!("{reason:?}"))
                    }))
                }
            };
            Ok(Box::pin(stream::iter([ToolEvent::Final(result)])))
        }
    }

    fn team_control_descriptor(kind: TeamControlToolKind) -> ToolDescriptor {
        let name = kind.name().to_owned();
        ToolDescriptor {
            name: name.clone(),
            display_name: name.clone(),
            description: "team coordinator control tool".to_owned(),
            category: "team".to_owned(),
            group: ToolGroup::Custom("team_control".to_owned()),
            version: "0.0.0".to_owned(),
            input_schema: team_control_input_schema(kind),
            output_schema: Some(team_control_output_schema(kind)),
            dynamic_schema: false,
            properties: ToolProperties {
                is_concurrency_safe: false,
                is_read_only: matches!(kind, TeamControlToolKind::TeamStatus),
                is_destructive: matches!(kind, TeamControlToolKind::StopTeam),
                long_running: None,
                defer_policy: DeferPolicy::AlwaysLoad,
            },
            trust_level: TrustLevel::UserControlled,
            required_capabilities: Vec::new(),
            budget: ResultBudget {
                metric: BudgetMetric::Chars,
                limit: 4096,
                on_overflow: OverflowAction::Truncate,
                preview_head_chars: 512,
                preview_tail_chars: 512,
            },
            provider_restriction: ProviderRestriction::All,
            origin: ToolOrigin::Builtin,
            search_hint: Some(format!("team coordinator control {}", kind.name())),
            service_binding: None,
            metadata: Default::default(),
        }
    }

    fn team_control_input_schema(kind: TeamControlToolKind) -> serde_json::Value {
        let recipient_properties = serde_json::json!({
            "agent_id": { "type": "string" },
            "role": { "type": "string" },
            "broadcast": { "type": "boolean" },
            "body": { "type": "string" }
        });
        match kind {
            TeamControlToolKind::Dispatch | TeamControlToolKind::Message => {
                team_control_object_schema(&[], recipient_properties)
            }
            TeamControlToolKind::PauseWorker | TeamControlToolKind::ResumeWorker => {
                team_control_object_schema(
                    &["agent_id"],
                    serde_json::json!({ "agent_id": { "type": "string" } }),
                )
            }
            TeamControlToolKind::SpawnWorker => team_control_object_schema(
                &[],
                serde_json::json!({
                    "agent_id": { "type": "string" },
                    "role": { "type": "string" }
                }),
            ),
            TeamControlToolKind::StopTeam | TeamControlToolKind::TeamStatus => {
                team_control_object_schema(&[], serde_json::json!({}))
            }
        }
    }

    fn team_control_output_schema(kind: TeamControlToolKind) -> serde_json::Value {
        match kind {
            TeamControlToolKind::Dispatch | TeamControlToolKind::Message => {
                team_control_object_schema(
                    &["message_id"],
                    serde_json::json!({ "message_id": { "type": "string" } }),
                )
            }
            TeamControlToolKind::PauseWorker => team_control_object_schema(
                &["paused"],
                serde_json::json!({ "paused": { "type": "string" } }),
            ),
            TeamControlToolKind::ResumeWorker => team_control_object_schema(
                &["resumed"],
                serde_json::json!({ "resumed": { "type": "string" } }),
            ),
            TeamControlToolKind::SpawnWorker => team_control_object_schema(
                &["agent_id"],
                serde_json::json!({ "agent_id": { "type": "string" } }),
            ),
            TeamControlToolKind::StopTeam => team_control_object_schema(
                &["team_id", "message_count"],
                serde_json::json!({
                    "team_id": { "type": "string" },
                    "message_count": { "type": "integer", "minimum": 0 }
                }),
            ),
            TeamControlToolKind::TeamStatus => team_control_object_schema(
                &["team_id", "member_count", "paused_members", "terminated"],
                serde_json::json!({
                    "team_id": { "type": "string" },
                    "member_count": { "type": "integer", "minimum": 0 },
                    "paused_members": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "terminated": { "type": ["string", "null"] }
                }),
            ),
        }
    }

    fn team_control_object_schema(
        required: &[&str],
        properties: serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": required,
            "properties": properties,
            "additionalProperties": false
        })
    }

    fn team_control_target(input: &serde_json::Value) -> Option<String> {
        if let Some(agent_id) = input.get("agent_id").and_then(serde_json::Value::as_str) {
            return Some(format!("agent:{agent_id}"));
        }
        if let Some(role) = input.get("role").and_then(serde_json::Value::as_str) {
            return Some(format!("role:{role}"));
        }
        input
            .get("broadcast")
            .and_then(serde_json::Value::as_bool)
            .filter(|broadcast| *broadcast)
            .map(|_| "broadcast".to_owned())
    }

    fn parse_recipient(input: &serde_json::Value) -> Result<Recipient, ToolError> {
        if let Some(agent_id) = input.get("agent_id").and_then(serde_json::Value::as_str) {
            return AgentId::parse(agent_id)
                .map(Recipient::Agent)
                .map_err(|error| ToolError::Validation(error.to_string()));
        }
        if let Some(role) = input.get("role").and_then(serde_json::Value::as_str) {
            return Ok(Recipient::Role(role.to_owned()));
        }
        if input
            .get("broadcast")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(Recipient::Broadcast);
        }
        Ok(Recipient::Coordinator)
    }

    fn parse_agent_id(input: &serde_json::Value, field: &str) -> Result<AgentId, ToolError> {
        let value = input
            .get(field)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::Validation(format!("{field} is required")))?;
        AgentId::parse(value).map_err(|error| ToolError::Validation(error.to_string()))
    }

    fn team_tool_error(error: TeamError) -> ToolError {
        ToolError::Message(error.to_string())
    }

    fn member_tool_filter(config: &harness_team::TeamMemberEngineConfig) -> ToolPoolFilter {
        let mut denylist = config.tool_blocklist.clone();
        let allowlist = match &config.toolset {
            TeamToolsetSelector::Custom(tools) => {
                Some(tools.iter().cloned().collect::<HashSet<_>>())
            }
            TeamToolsetSelector::InheritWithBlocklist(blocklist) => {
                denylist.extend(blocklist.iter().cloned());
                None
            }
            TeamToolsetSelector::InheritAll | TeamToolsetSelector::Preset(_) => None,
        };
        if matches!(config.memory_mode, harness_contracts::MemoryThreadMode::Off) {
            denylist.insert("memory".to_owned());
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

    fn message_content_text(content: MessageContent) -> String {
        match content {
            MessageContent::Text(text) => text,
            MessageContent::Structured(value) => value.to_string(),
            MessageContent::Multimodal(parts) => serde_json::to_string(&parts).unwrap_or_default(),
        }
    }

    fn team_error(error: impl std::fmt::Display) -> TeamError {
        TeamError::Journal(error.to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn candidate_only_team_members_keep_memory_tool_available() {
            let mut config = harness_team::TeamMemberEngineConfig::default();
            config.memory_mode = harness_contracts::MemoryThreadMode::CandidateOnly;

            let filter = member_tool_filter(&config);

            assert!(!filter.denylist.contains("memory"));
        }

        #[test]
        fn read_only_team_members_keep_memory_tool_available() {
            let mut config = harness_team::TeamMemberEngineConfig::default();
            config.memory_mode = harness_contracts::MemoryThreadMode::ReadOnly;

            let filter = member_tool_filter(&config);

            assert!(!filter.denylist.contains("memory"));
        }

        #[test]
        fn team_control_descriptors_declare_strict_io_schemas() {
            for kind in [
                TeamControlToolKind::Dispatch,
                TeamControlToolKind::Message,
                TeamControlToolKind::PauseWorker,
                TeamControlToolKind::ResumeWorker,
                TeamControlToolKind::SpawnWorker,
                TeamControlToolKind::StopTeam,
                TeamControlToolKind::TeamStatus,
            ] {
                let descriptor = team_control_descriptor(kind);
                assert_eq!(
                    descriptor
                        .input_schema
                        .get("additionalProperties")
                        .and_then(serde_json::Value::as_bool),
                    Some(false),
                    "{} input schema should reject unknown fields",
                    kind.name()
                );
                assert!(
                    descriptor.output_schema.is_some(),
                    "{} should declare an output schema",
                    kind.name()
                );
            }
        }
    }
}

#[cfg(feature = "agents-team")]
pub use agents_team::EngineTeamMemberRunner;

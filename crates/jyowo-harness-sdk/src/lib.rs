//! `jyowo-harness-sdk`
//!
//! Business-facing facade for the Jyowo Agent Harness SDK.
//!
//! SPEC: docs/architecture/harness/crates/harness-sdk.md
//! Status: M7 facade.
//!
//! ```compile_fail
//! # async fn demo() {
//! let _ = jyowo_harness_sdk::Harness::builder().build().await;
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod builder;
pub mod builtin;
pub mod error;
pub mod ext;
pub mod harness;
pub mod options;
pub mod prelude;
pub mod session;
pub mod skill_config;
pub mod skill_pack_loader;
pub mod team;
#[cfg(feature = "testing")]
pub mod testing;

pub use builder::{HarnessBuilder, Set, Unset};
pub use error::HarnessError;
#[cfg(feature = "stream-permission")]
pub use harness::StreamPermissionRuntime;
pub use harness::{
    ConversationEventsPage, ConversationEventsPageRequest, ConversationSession,
    ConversationSessionSummary, ConversationTurnReceipt, ConversationTurnRequest, Harness,
    HarnessOptions, HarnessSamplingProvider, McpConfig, TenantPolicy, WorkspaceCreateRequest,
};
pub use harness_journal::{
    AuditFilter, AuditOrder, AuditPage, AuditQuery, AuditRecord, AuditScope,
};
pub use harness_session::{BootstrapFileSpec, Workspace, WorkspaceBootstrap, WorkspaceSpec};
pub use options::{
    ConfigError, ConfigSource, ConfigWarning, LastKnownGoodConfig, OptionsParseMode,
    ParsedHarnessOptions,
};
pub use session::{EventStream, RunContext, Session, SessionHandle, SessionOptions};
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
    AgentId, Event, MessageId, RunId, SessionId, TeamId, TenantId, ToolUseId, TurnInput,
    WorkspaceId,
};

#[cfg(feature = "agents-team")]
pub mod agents_team {
    use std::collections::HashSet;
    use std::sync::Arc;

    use async_trait::async_trait;
    use futures::{stream, StreamExt};
    use harness_contracts::{
        AgentId, BudgetMetric, ContextVisibility, DeferPolicy, Event, MessageContent,
        OverflowAction, ProviderRestriction, Recipient, ResultBudget, ToolDescriptor, ToolError,
        ToolGroup, ToolOrigin, ToolProperties, ToolResult, TrustLevel, UsageSnapshot,
    };
    use harness_engine::{Engine, EngineRunner, RunContext, SessionHandle};
    use harness_team::{
        TeamControlHandle, TeamError, TeamMember, TeamMemberEngineConfig, TeamMemberRunOutcome,
        TeamMemberRunRequest, TeamMemberRunner, TeamSandboxPolicy, TeamToolsetSelector,
    };
    use harness_tool::{
        PermissionCheck, Tool, ToolContext, ToolEvent, ToolPool, ToolPoolFilter, ToolStream,
        ValidationError,
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
            let ctx = RunContext::new(request.tenant_id, request.session_id, request.run_id)
                .with_parent_run_id(request.parent_run_id)
                .with_team_id(request.team_id)
                .with_correlation_id(request.correlation_id)
                .with_permission_mode(request.engine_config.permission_mode)
                .with_interactivity(request.engine_config.interactivity)
                .with_budget_limits(team_member_budget_limits(&request.engine_config));
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
        let context = base
            .context_engine()
            .clone_with_budget(request.engine_config.token_budget);
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
            let system_prompt = match base.system_prompt() {
                Some(base_prompt) if !base_prompt.is_empty() => {
                    Some(format!("{base_prompt}\n\n{addendum}"))
                }
                _ => Some(addendum.clone()),
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

        async fn check_permission(
            &self,
            _input: &serde_json::Value,
            _ctx: &ToolContext,
        ) -> PermissionCheck {
            PermissionCheck::Allowed
        }

        async fn execute(
            &self,
            input: serde_json::Value,
            _ctx: ToolContext,
        ) -> Result<ToolStream, ToolError> {
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
                    self.control.pause_worker(worker).await;
                    ToolResult::Structured(serde_json::json!({ "paused": worker.to_string() }))
                }
                TeamControlToolKind::ResumeWorker => {
                    let worker = parse_agent_id(&input, "agent_id")?;
                    self.control.resume_worker(worker).await;
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
            input_schema: serde_json::json!({ "type": "object" }),
            output_schema: None,
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
            search_hint: None,
        }
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
}

#[cfg(feature = "agents-team")]
pub use agents_team::EngineTeamMemberRunner;

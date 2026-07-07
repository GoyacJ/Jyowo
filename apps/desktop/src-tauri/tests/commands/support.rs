#![allow(dead_code)]
#![allow(unused_imports)]

use super::*;
use harness_contracts::{NetworkAccess, ToolActionPlan};
use harness_tool::{action_plan_from_permission_check, AuthorizedToolInput};

pub(crate) fn permission_request() -> PermissionRequest {
    permission_request_with_subject(PermissionSubject::CommandExec {
        command: "pwd".to_owned(),
        argv: vec!["pwd".to_owned()],
        cwd: None,
        fingerprint: None,
    })
}

pub(crate) struct NeedsPermissionTool {
    pub(crate) descriptor: ToolDescriptor,
}

impl Default for NeedsPermissionTool {
    fn default() -> Self {
        Self::named("NeedsPermission", "NeedsPermission")
    }
}

impl NeedsPermissionTool {
    pub(crate) fn named(name: &str, display_name: &str) -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: name.to_owned(),
                display_name: display_name.to_owned(),
                description: "Requests command permission for desktop tests.".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::Custom("test".to_owned()),
                version: "0.1.0".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" }
                    },
                    "required": ["command"]
                }),
                output_schema: None,
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: false,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                trust_level: TrustLevel::UserControlled,
                required_capabilities: Vec::new(),
                budget: ResultBudget {
                    metric: BudgetMetric::Chars,
                    limit: 30_000,
                    on_overflow: OverflowAction::Offload,
                    preview_head_chars: 2_000,
                    preview_tail_chars: 2_000,
                },
                provider_restriction: ProviderRestriction::All,
                origin: jyowo_harness_sdk::ext::ToolOrigin::Builtin,
                search_hint: None,
                service_binding: None,
            },
        }
    }
}

#[async_trait]
impl Tool for NeedsPermissionTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let command = input
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("needs-permission")
            .to_owned();

        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            PermissionCheck::AskUser {
                subject: PermissionSubject::CommandExec {
                    command: command.clone(),
                    argv: vec![command.clone()],
                    cwd: None,
                    fingerprint: None,
                },
                scope: DecisionScope::ExactCommand { command, cwd: None },
            },
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
        Ok(Box::pin(stream::iter(vec![ToolEvent::Final(
            ToolResult::Text("done".to_owned()),
        )])))
    }
}

struct TestBackgroundAgentStarter {
    workspace_root: PathBuf,
    event_store: Arc<dyn EventStore>,
}

impl harness_contracts::BackgroundAgentStarterCap for TestBackgroundAgentStarter {
    fn start_background_agent(
        &self,
        request: harness_contracts::BackgroundAgentToolStartRequest,
    ) -> futures::future::BoxFuture<
        'static,
        Result<harness_contracts::BackgroundAgentToolStartResponse, ToolError>,
    > {
        let workspace_root = self.workspace_root.clone();
        let event_store = Arc::clone(&self.event_store);
        Box::pin(async move {
            let store = Arc::new(
                jyowo_harness_sdk::AgentRuntimeStore::open(&workspace_root)
                    .map_err(|error| ToolError::Internal(error.to_string()))?,
            );
            let redactor = Arc::new(DefaultRedactor::default());
            let manager = jyowo_harness_sdk::BackgroundAgentManager::new(
                store,
                event_store,
                request.tenant_id,
                request.conversation_id,
                redactor.clone(),
            );
            let mut safe_input =
                harness_contracts::ConversationTurnInput::ask(request.goal.clone());
            safe_input.prompt = redactor.redact(&request.goal, &RedactRules::default());
            let mut agent_tool_policy = request.agent_tool_policy.clone();
            agent_tool_policy.background_agents = AgentUsePolicy::Off;
            let record = manager
                .start(jyowo_harness_sdk::BackgroundAgentStartRequest {
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
                            "session": request.session,
                            "input": safe_input,
                            "modelConfigId": request.model_config_id,
                            "permissionMode": request.permission_mode,
                            "agentToolPolicy": agent_tool_policy,
                        },
                    })
                    .to_string(),
                })
                .await
                .map_err(|error| ToolError::Internal(error.to_string()))?;
            Ok(harness_contracts::BackgroundAgentToolStartResponse {
                background_agent_id: record.background_agent_id,
                conversation_id: request.conversation_id,
                parent_run_id: request.parent_run_id,
                title: record.title,
                status: "started".to_owned(),
            })
        })
    }
}

pub(crate) fn permission_request_with_subject(subject: PermissionSubject) -> PermissionRequest {
    let tenant_id = TenantId::SHARED;
    let session_id = SessionId::new();

    PermissionRequest {
        request_id: RequestId::new(),
        tenant_id,
        session_id,
        tool_use_id: ToolUseId::new(),
        tool_name: "shell".to_owned(),
        subject,
        severity: Severity::Low,
        scope_hint: DecisionScope::ToolName("shell".to_owned()),
        action_plan_hash: harness_contracts::ActionPlanHash::default(),
        decision_options: Vec::new(),
        confirmation_expected: None,
        created_at: now(),
    }
}

pub(crate) fn permission_context() -> PermissionContext {
    permission_context_with_run_id(None)
}

pub(crate) fn permission_context_with_run_id(run_id: Option<RunId>) -> PermissionContext {
    PermissionContext {
        permission_mode: PermissionMode::Default,
        previous_mode: None,
        session_id: SessionId::new(),
        tenant_id: TenantId::SHARED,
        run_id,
        interactivity: InteractivityLevel::FullyInteractive,
        timeout_policy: None,
        fallback_policy: FallbackPolicy::AskUser,
        hook_overrides: Vec::new(),
    }
}

pub(crate) fn permission_context_for_request(
    request: &PermissionRequest,
    run_id: Option<RunId>,
) -> PermissionContext {
    PermissionContext {
        session_id: request.session_id,
        tenant_id: request.tenant_id,
        run_id,
        ..permission_context_with_run_id(run_id)
    }
}

pub(crate) fn test_memory_record(session_id: SessionId, content: &str) -> MemoryRecord {
    MemoryRecord {
        id: MemoryId::new(),
        tenant_id: TenantId::SINGLE,
        kind: MemoryKind::UserPreference,
        visibility: MemoryVisibility::Private { session_id },
        content: content.to_owned(),
        metadata: MemoryMetadata {
            tags: Vec::new(),
            source: MemorySource::UserInput,
            confidence: 1.0,
            evidence: None,
            access_count: 0,
            last_accessed_at: None,
            recall_score: 1.0,
            recall_score_breakdown: None,
            ttl: None,
            redacted_segments: 0,
        },
        created_at: now(),
        updated_at: now(),
    }
}

pub(crate) struct RawExportMemoryProvider {
    inner: Arc<InMemoryMemoryProvider>,
}

impl RawExportMemoryProvider {
    pub(crate) fn new(inner: Arc<InMemoryMemoryProvider>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl MemoryStore for RawExportMemoryProvider {
    fn provider_id(&self) -> &str {
        self.inner.provider_id()
    }

    async fn recall(
        &self,
        query: MemoryQuery,
    ) -> Result<Vec<MemoryRecord>, harness_contracts::MemoryError> {
        self.inner.recall(query).await
    }

    async fn get(&self, id: MemoryId) -> Result<MemoryRecord, harness_contracts::MemoryError> {
        self.inner.get(id).await
    }

    async fn upsert(
        &self,
        record: MemoryRecord,
    ) -> Result<MemoryId, harness_contracts::MemoryError> {
        self.inner.upsert(record).await
    }

    async fn forget(&self, id: MemoryId) -> Result<(), harness_contracts::MemoryError> {
        self.inner.forget(id).await
    }

    async fn list(
        &self,
        scope: MemoryListScope,
    ) -> Result<Vec<MemorySummary>, harness_contracts::MemoryError> {
        self.inner.list(scope).await
    }
}

impl MemoryLifecycle for RawExportMemoryProvider {}

impl MemoryProvider for RawExportMemoryProvider {
    fn descriptor(&self) -> MemoryProviderDescriptor {
        let mut descriptor = self.inner.descriptor();
        descriptor.supports_raw_content_export = true;
        descriptor
    }
}

pub(crate) async fn wait_for_pending_permission(
    state: &DesktopRuntimeState,
    request_id: RequestId,
) -> jyowo_harness_sdk::ext::PendingPermissionRequest {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(pending) = state
            .pending_permission_requests()
            .into_iter()
            .find(|pending| pending.request.request_id == request_id)
        {
            return pending;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("permission request should become pending");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

pub(crate) async fn wait_for_pending_permission_for_session(
    state: &DesktopRuntimeState,
    session_id: SessionId,
) -> jyowo_harness_sdk::ext::PendingPermissionRequest {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(pending) = state
            .pending_permission_requests()
            .into_iter()
            .find(|pending| pending.request.session_id == session_id)
        {
            return pending;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("permission request should become pending for session");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

pub(crate) fn permission_option_id_for_decision(
    pending: &jyowo_harness_sdk::ext::PendingPermissionRequest,
    decision: Decision,
) -> String {
    pending
        .decision_options
        .iter()
        .find(|option| option.decision == decision)
        .map(|option| option.option_id.to_string())
        .expect("pending permission should expose the requested decision option")
}

pub(crate) fn approve_permission_option_id(
    pending: &jyowo_harness_sdk::ext::PendingPermissionRequest,
) -> String {
    permission_option_id_for_decision(pending, Decision::AllowOnce)
}

pub(crate) fn deny_permission_option_id(
    pending: &jyowo_harness_sdk::ext::PendingPermissionRequest,
) -> String {
    permission_option_id_for_decision(pending, Decision::DenyOnce)
}

pub(crate) async fn run_with_mcp_transport_approval<T>(
    state: &DesktopRuntimeState,
    command: impl std::future::Future<Output = Result<T, jyowo_desktop_shell::commands::CommandErrorPayload>>
        + Send
        + 'static,
) -> Result<T, jyowo_desktop_shell::commands::CommandErrorPayload>
where
    T: Send + 'static,
{
    let command_task = tokio::spawn(command);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let pending = loop {
        if let Some(pending) = state
            .pending_permission_requests()
            .into_iter()
            .find(|pending| {
                matches!(
                    &pending.request.subject,
                    PermissionSubject::Custom { kind, .. } if kind == "mcp_transport"
                )
            })
        {
            break pending;
        }

        if command_task.is_finished() {
            return command_task
                .await
                .expect("mcp command task should complete without panicking");
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("mcp transport permission request should become pending");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    };

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: pending.request.session_id.to_string(),
            decision: PermissionDecision::Approve,
            option_id: approve_permission_option_id(&pending),
            request_id: pending.request.request_id.to_string(),
            confirmation_text: None,
        },
        state,
    )
    .await?;

    command_task
        .await
        .expect("mcp command task should complete without panicking")
}

pub(crate) async fn wait_for_pending_mcp_transport_permission(
    state: &DesktopRuntimeState,
) -> jyowo_harness_sdk::ext::PendingPermissionRequest {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(pending) = state
            .pending_permission_requests()
            .into_iter()
            .find(|pending| {
                matches!(
                    &pending.request.subject,
                    PermissionSubject::Custom { kind, .. } if kind == "mcp_transport"
                )
            })
        {
            return pending;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("mcp transport permission request should become pending");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

pub(crate) async fn open_conversation_session(state: &DesktopRuntimeState, session_id: SessionId) {
    state
        .harness()
        .expect("runtime state should retain the configured harness")
        .open_or_create_conversation_session(
            state
                .conversation_session_options(session_id)
                .expect("session options"),
        )
        .await
        .expect("conversation session should open");
}

#[derive(Debug, Default)]
struct AllowExecPreflightSandbox;

#[async_trait]
impl jyowo_harness_sdk::ext::SandboxBackend for AllowExecPreflightSandbox {
    fn backend_id(&self) -> &'static str {
        "allow-exec-preflight"
    }

    fn capabilities(&self) -> jyowo_harness_sdk::ext::SandboxCapabilities {
        jyowo_harness_sdk::ext::SandboxCapabilities {
            supports_network: true,
            supports_filesystem_write: true,
            max_concurrent_execs: 1,
            ..jyowo_harness_sdk::ext::SandboxCapabilities::default()
        }
    }

    fn preflight_execute(
        &self,
        _spec: &jyowo_harness_sdk::ext::ExecSpec,
    ) -> Result<(), harness_contracts::SandboxError> {
        Ok(())
    }

    async fn execute(
        &self,
        _spec: jyowo_harness_sdk::ext::ExecSpec,
        _ctx: jyowo_harness_sdk::ext::ExecContext,
    ) -> Result<jyowo_harness_sdk::ext::ProcessHandle, harness_contracts::SandboxError> {
        Err(harness_contracts::SandboxError::CapabilityMismatch {
            capability: "execute".to_owned(),
            detail: "test sandbox only supports preflight".to_owned(),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &jyowo_harness_sdk::ext::SnapshotSpec,
    ) -> Result<jyowo_harness_sdk::ext::SessionSnapshotFile, harness_contracts::SandboxError> {
        Err(harness_contracts::SandboxError::SnapshotUnsupported {
            kind: "allow_exec_preflight_snapshot".to_owned(),
        })
    }

    async fn restore_session(
        &self,
        _snapshot: &jyowo_harness_sdk::ext::SessionSnapshotFile,
    ) -> Result<(), harness_contracts::SandboxError> {
        Err(harness_contracts::SandboxError::SnapshotUnsupported {
            kind: "allow_exec_preflight_restore".to_owned(),
        })
    }

    async fn shutdown(&self) -> Result<(), harness_contracts::SandboxError> {
        Ok(())
    }
}

pub(crate) fn test_run_started_event(session_id: SessionId, run_id: RunId) -> RunStartedEvent {
    RunStartedEvent {
        correlation_id: CorrelationId::new(),
        effective_config_hash: ConfigHash([0; 32]),
        input: TurnInput {
            message: Message {
                created_at: now(),
                id: MessageId::new(),
                parts: vec![MessagePart::Text("Test run".to_owned())],
                role: MessageRole::User,
            },
            metadata: json!({}),
        },
        parent_run_id: None,
        permission_mode: PermissionMode::Default,
        model: test_run_model_snapshot(),
        run_id,
        session_id,
        snapshot_id: SnapshotId::new(),
        started_at: now(),
        tenant_id: TenantId::SINGLE,
    }
}

pub(crate) fn test_run_model_snapshot() -> RunModelSnapshot {
    RunModelSnapshot {
        model_config_id: None,
        provider_id: "test".to_owned(),
        model_id: "test-model".to_owned(),
        display_name: "Test Model".to_owned(),
        protocol: ModelProtocol::Messages,
        context_window: 128_000,
        max_output_tokens: 8_192,
        conversation_capability: ConversationModelCapability::default(),
    }
}

pub(crate) fn test_tool_use_requested_event(
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

pub(crate) fn test_permission_requested_event(
    session_id: SessionId,
    run_id: RunId,
    tool_use_id: ToolUseId,
    request_id: RequestId,
    tool_name: &str,
) -> PermissionRequestedEvent {
    PermissionRequestedEvent {
        at: now(),
        causation_id: EventId::new(),
        fingerprint: None,
        interactivity: InteractivityLevel::FullyInteractive,
        auto_resolved: false,
        actor_source: PermissionActorSource::ParentRun,
        action_plan_hash: Default::default(),
        review: Default::default(),
        effective_mode: Default::default(),
        sandbox_policy: Default::default(),
        presented_options: vec![PermissionDecisionOption {
            option_id: PermissionOptionId::new(),
            decision: Decision::AllowOnce,
            scope: DecisionScope::Any,
            lifetime: DecisionLifetime::Once,
            matcher_summary: DecisionMatcherSummary {
                kind: DecisionMatcherKind::Any,
                label: "allow once".to_owned(),
            },
            label: "Allow once".to_owned(),
            requires_confirmation: false,
            action_plan_hash: ActionPlanHash::default(),
            fingerprint: None,
        }],
        request_id,
        run_id,
        scope_hint: DecisionScope::ToolName(tool_name.to_owned()),
        session_id,
        severity: Severity::Low,
        subject: PermissionSubject::CommandExec {
            argv: vec![tool_name.to_owned()],
            command: tool_name.to_owned(),
            cwd: None,
            fingerprint: None,
        },
        tenant_id: TenantId::SINGLE,
        tool_name: tool_name.to_owned(),
        tool_use_id,
    }
}

pub(crate) async fn runtime_state_with_harness() -> DesktopRuntimeState {
    runtime_state_with_harness_for_workspace(unique_workspace("harness")).await
}

pub(crate) async fn runtime_state_with_harness_for_workspace(
    workspace: PathBuf,
) -> DesktopRuntimeState {
    std::fs::create_dir_all(&workspace).unwrap();
    write_test_provider_settings(&workspace);
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let blob_store: Arc<dyn BlobStore> = Arc::new(
        FileBlobStore::open(workspace.join(".jyowo").join("runtime").join("blobs"))
            .expect("test blob store should open"),
    );
    let evidence_registry = Arc::new(
        SqliteEvidenceRefRegistry::open(
            workspace
                .join(".jyowo")
                .join("runtime")
                .join("conversation-read-model.sqlite"),
        )
        .await
        .expect("test evidence registry should open"),
    );
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let event_store_for_evidence = Arc::clone(&event_store) as Arc<dyn harness_journal::EventStore>;
    let evidence_ref_store = Arc::new(EvidenceRefStore::new_with_event_store(
        evidence_registry,
        Arc::clone(&blob_store),
        event_store_for_evidence,
    ));
    let harness = Arc::new(
        Harness::builder()
            .with_options(test_harness_options(&workspace))
            .with_model(TestModelProvider::default())
            .with_store(event_store)
            .with_sandbox(NoopSandbox::new())
            .with_blob_store_arc(blob_store)
            .with_evidence_ref_store_arc(evidence_ref_store)
            .with_stream_permission_broker_arc(
                stream_permission_runtime.broker(),
                stream_permission_runtime.resolver_handle(),
            )
            .build()
            .await
            .expect("harness should build with stream permission runtime"),
    );

    let mut state = DesktopRuntimeState::with_harness_and_stream_permission_runtime_for_workspace(
        workspace,
        harness,
        stream_permission_runtime,
    )
    .expect("state should use the harness permission broker");
    let state_workspace = state.workspace_root().to_path_buf();
    use_test_provider_settings_store(&mut state, &state_workspace);
    state
}

pub(crate) async fn runtime_state_with_memory_provider(
    provider: Arc<dyn MemoryProvider>,
) -> DesktopRuntimeState {
    let workspace = unique_workspace("memory-provider");
    std::fs::create_dir_all(&workspace).unwrap();
    write_test_provider_settings(&workspace);
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let harness = Arc::new(
        Harness::builder()
            .with_options(test_harness_options(&workspace))
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_stream_permission_broker_arc(
                stream_permission_runtime.broker(),
                stream_permission_runtime.resolver_handle(),
            )
            .with_memory_provider_arc(provider)
            .build()
            .await
            .expect("harness should build with memory provider"),
    );

    let mut state = DesktopRuntimeState::with_harness_and_stream_permission_runtime_for_workspace(
        workspace,
        harness,
        stream_permission_runtime,
    )
    .expect("state should use the harness permission broker");
    let state_workspace = state.workspace_root().to_path_buf();
    use_test_provider_settings_store(&mut state, &state_workspace);
    state
}

pub(crate) async fn runtime_state_with_mcp_registry(
    registry: McpRegistry,
    server_ids_to_inject: Vec<McpServerId>,
) -> DesktopRuntimeState {
    runtime_state_with_mcp_registry_for_workspace(
        unique_workspace("mcp-registry"),
        registry,
        server_ids_to_inject,
    )
    .await
}

pub(crate) async fn runtime_state_with_mcp_registry_for_workspace(
    workspace: PathBuf,
    registry: McpRegistry,
    server_ids_to_inject: Vec<McpServerId>,
) -> DesktopRuntimeState {
    std::fs::create_dir_all(&workspace).unwrap();
    write_test_provider_settings(&workspace);
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let harness = Arc::new(
        Harness::builder()
            .with_options(test_harness_options(&workspace))
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(AllowExecPreflightSandbox)
            .with_stream_permission_broker_arc(
                stream_permission_runtime.broker(),
                stream_permission_runtime.resolver_handle(),
            )
            .with_mcp_config(McpConfig {
                registry,
                server_ids_to_inject,
            })
            .build()
            .await
            .expect("harness should build with MCP registry"),
    );

    let mut state = DesktopRuntimeState::with_harness_and_stream_permission_runtime_for_workspace(
        workspace,
        harness,
        stream_permission_runtime,
    )
    .expect("state should use the harness permission broker");
    let state_workspace = state.workspace_root().to_path_buf();
    use_test_provider_settings_store(&mut state, &state_workspace);
    state
}

pub(crate) async fn runtime_state_with_scripted_model(
    responses: Vec<ScriptedResponse>,
) -> DesktopRuntimeState {
    runtime_state_with_scripted_model_for_workspace(unique_workspace("scripted-model"), responses)
        .await
}

pub(crate) async fn runtime_state_with_scripted_model_for_workspace(
    workspace: PathBuf,
    responses: Vec<ScriptedResponse>,
) -> DesktopRuntimeState {
    std::fs::create_dir_all(&workspace).unwrap();
    write_test_provider_settings(&workspace);
    let event_store =
        Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))) as Arc<dyn EventStore>;
    let background_agent_starter: Arc<dyn harness_contracts::BackgroundAgentStarterCap> =
        Arc::new(TestBackgroundAgentStarter {
            workspace_root: workspace.clone(),
            event_store: Arc::clone(&event_store),
        });
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let harness = Arc::new(
        Harness::builder()
            .with_options(test_harness_options(&workspace))
            .with_model_arc(Arc::new(ScriptedProvider::new(responses)))
            .with_store_arc(event_store)
            .with_sandbox(NoopSandbox::new())
            .with_capability(
                ToolCapability::Custom("jyowo.background_agent.starter".to_owned()),
                background_agent_starter,
            )
            .with_stream_permission_broker_arc(
                stream_permission_runtime.broker(),
                stream_permission_runtime.resolver_handle(),
            )
            .with_tool_registry(
                ToolRegistry::builder()
                    .with_tool(Box::<NeedsPermissionTool>::default())
                    .build()
                    .expect("test tool registry should build"),
            )
            .build()
            .await
            .expect("harness should build with stream permission runtime"),
    );

    let mut state = DesktopRuntimeState::with_harness_and_stream_permission_runtime_for_workspace(
        workspace,
        harness,
        stream_permission_runtime,
    )
    .expect("state should use the harness permission broker");
    let state_workspace = state.workspace_root().to_path_buf();
    use_test_provider_settings_store(&mut state, &state_workspace);
    state
}

pub(crate) fn test_harness_options(workspace: &Path) -> HarnessOptions {
    let mut options = HarnessOptions::default();
    options.workspace_root = workspace.to_path_buf();
    options.model_id = "test-model".to_owned();
    options
}

pub(crate) fn unique_workspace(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-desktop-{name}-{}-{}",
        std::process::id(),
        SessionId::new()
    ))
}

pub(crate) fn provider_settings_store_for_workspace(
    workspace: &Path,
) -> DesktopProviderSettingsStore {
    let layout = test_storage_layout_for_workspace(workspace);
    DesktopProviderSettingsStore::new_with_layout(layout, workspace.to_path_buf())
}

pub(crate) fn use_test_provider_settings_store(state: &mut DesktopRuntimeState, workspace: &Path) {
    let layout = test_storage_layout_for_workspace(workspace);
    let store = provider_settings_store_for_workspace(workspace);
    let record = store
        .load_record()
        .expect("test provider settings should load")
        .expect("test provider settings should exist");
    let config_id = record
        .default_config_id
        .as_deref()
        .expect("test provider settings should have default config");
    let config = record
        .configs
        .iter()
        .find(|config| config.id == config_id)
        .expect("test provider settings default config should exist");
    state
        .set_active_runtime_provider_config_for_test(config)
        .expect("test active runtime provider binding should update");
    state.set_provider_settings_store_for_test(Arc::new(store));
    state.set_config_stores_for_test(
        jyowo_desktop_shell::commands::stores::GlobalConfigStore::new(layout.clone()),
        Some(
            jyowo_desktop_shell::commands::stores::ProjectConfigStore::new(
                layout,
                workspace.to_path_buf(),
            ),
        ),
    );
}

fn test_storage_layout_for_workspace(
    workspace: &Path,
) -> jyowo_desktop_shell::storage_layout::StorageLayout {
    jyowo_desktop_shell::storage_layout::StorageLayout::new(
        jyowo_desktop_shell::storage_layout::JyowoHome::new(
            workspace.join(".jyowo-test-home").join(".jyowo"),
        ),
    )
}

pub(crate) fn test_attachment_blob_ref(size: u64, content_type: &str) -> AttachmentBlobRefPayload {
    AttachmentBlobRefPayload {
        id: "01J00000000000000000000000".to_owned(),
        size,
        content_hash: [1; 32],
        content_type: Some(content_type.to_owned()),
    }
}

pub(crate) fn skill_markdown(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\nSkill body for {name}.\n")
}

pub(crate) fn write_skill_package(
    root: &std::path::Path,
    package_name: &str,
    skill_name: &str,
    description: &str,
    resource: Option<(&str, &str)>,
) -> PathBuf {
    let package_path = root.join(package_name);
    std::fs::create_dir_all(&package_path).unwrap();
    std::fs::write(
        package_path.join("SKILL.md"),
        skill_markdown(skill_name, description),
    )
    .unwrap();
    if let Some((relative_path, content)) = resource {
        let resource_path = package_path.join(relative_path);
        std::fs::create_dir_all(resource_path.parent().unwrap()).unwrap();
        std::fs::write(resource_path, content).unwrap();
    }
    package_path.canonicalize().unwrap()
}

pub(crate) fn register_test_skill(state: &DesktopRuntimeState, name: &str, description: &str) {
    let harness = state
        .harness()
        .expect("runtime state should include harness");
    let skill = parse_skill_markdown(
        &skill_markdown(name, description),
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("test skill should parse");
    harness
        .skill_registry()
        .register_batch(vec![skill])
        .expect("test skill should register");
}

pub(crate) fn register_test_tool(state: &DesktopRuntimeState, name: &str, display_name: &str) {
    let harness = state
        .harness()
        .expect("runtime state should include harness");
    harness
        .tool_registry()
        .register(Box::new(NeedsPermissionTool::named(name, display_name)))
        .expect("test tool should register");
}

pub(crate) struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    pub(crate) fn set(key: &'static str, value: &std::ffi::OsStr) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

pub(crate) fn stdio_mcp_fixture_script() -> String {
    r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"0.1.0"}}}'
      ;;
    *'"method":"tools/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"Echo input","inputSchema":{"type":"object"}}]}}'
      ;;
  esac
done
"#
    .to_owned()
}

pub(crate) struct StaticMcpConnection {
    pub(crate) tools: Vec<McpToolDescriptor>,
}

#[async_trait]
impl McpConnection for StaticMcpConnection {
    fn connection_id(&self) -> &str {
        "static-test-mcp"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        Ok(self.tools.clone())
    }

    async fn call_tool(&self, _name: &str, _args: Value) -> Result<McpToolResult, McpError> {
        Ok(McpToolResult::text("ok"))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct RecordingProviderSettingsStore {
    pub(crate) fail_record: bool,
    pub(crate) record: Mutex<Option<ProviderSettingsRecord>>,
}

impl ProviderSettingsStore for RecordingProviderSettingsStore {
    fn load_record(
        &self,
    ) -> Result<Option<ProviderSettingsRecord>, jyowo_desktop_shell::commands::CommandErrorPayload>
    {
        Ok(self.record.lock().unwrap().clone())
    }

    fn save_record(
        &self,
        record: &ProviderSettingsRecord,
    ) -> Result<(), jyowo_desktop_shell::commands::CommandErrorPayload> {
        if self.fail_record {
            return Err(jyowo_desktop_shell::commands::CommandErrorPayload {
                code: "RUNTIME_OPERATION_FAILED",
                message: "record write failed".to_owned(),
            });
        }

        *self.record.lock().unwrap() = Some(record.clone());
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct RecordingMcpServerStore {
    pub(crate) record: Mutex<Option<McpServerConfigRecord>>,
}

impl McpServerStore for RecordingMcpServerStore {
    fn load_records(
        &self,
    ) -> Result<Vec<McpServerConfigRecord>, jyowo_desktop_shell::commands::CommandErrorPayload>
    {
        Ok(self.record.lock().unwrap().clone().into_iter().collect())
    }

    fn save_record(
        &self,
        record: &McpServerConfigRecord,
    ) -> Result<(), jyowo_desktop_shell::commands::CommandErrorPayload> {
        *self.record.lock().unwrap() = Some(record.clone());
        Ok(())
    }

    fn delete_record(
        &self,
        id: &str,
    ) -> Result<(), jyowo_desktop_shell::commands::CommandErrorPayload> {
        let mut record = self.record.lock().unwrap();
        if record.as_ref().is_some_and(|record| record.id == id) {
            *record = None;
        }
        Ok(())
    }
}

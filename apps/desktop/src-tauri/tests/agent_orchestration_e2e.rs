use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_contracts::{
    AgentUsePolicy, AgentWorkspaceIsolationMode, BackgroundAgentState, NetworkAccess,
    PermissionActorSource, Redactor, ToolActionPlan, WorkspaceAccess,
};
use harness_tool::{action_plan_from_permission_check, AuthorizedToolInput};
use jyowo_desktop_shell::commands::{
    cancel_background_agent_with_runtime_state, get_background_agent_with_runtime_state,
    list_background_agents_with_runtime_state, page_conversation_worktree_with_runtime_state,
    resolve_permission_with_runtime_state, resolve_start_run_agent_policy,
    set_execution_settings_with_store, start_run_with_runtime_state, BackgroundAgentIdRequest,
    ConversationModelCapabilityRecord, DesktopExecutionSettingsStore, DesktopProviderSettingsStore,
    DesktopRuntimeState, GetBackgroundAgentRequest, ListBackgroundAgentsRequest,
    PageConversationWorktreeDirection, PageConversationWorktreeRequest, ProviderConfigRecord,
    ProviderModelDescriptorRecord, ProviderModelLifecycleRecord, ProviderModelModalityRecord,
    ProviderSettingsRecord, ProviderSettingsStore, ResolvePermissionRequest,
    SetExecutionSettingsRequest, StartRunRequest,
};
use jyowo_harness_sdk::ext::{
    ContentDelta, DecisionScope, DeferPolicy, Event, EventStore, HealthStatus, InferContext,
    ModelDescriptor, ModelError, ModelLifecycle, ModelProtocol, ModelProvider, ModelRequest,
    ModelStream, ModelStreamEvent, OverflowAction, PermissionCheck, PermissionMode,
    PermissionSubject, ProviderRestriction, ReplayCursor, ResultBudget, StreamBrokerConfig,
    TenantId, Tool, ToolCapability, ToolContext, ToolDescriptor, ToolError, ToolEvent, ToolGroup,
    ToolOrigin, ToolProfile, ToolProperties, ToolRegistry, ToolResult, ToolStream, ToolUseId,
    TrustLevel, ValidationError,
};
use jyowo_harness_sdk::testing::{
    InMemoryEventStore, NoopRedactor, NoopSandbox, ScriptedProvider, ScriptedResponse,
};
use jyowo_harness_sdk::{
    AgentCapabilityResolutionContext, Harness, HarnessOptions, StreamPermissionRuntime,
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex as AsyncMutex;

static WORKSPACE_COUNTER: Mutex<u64> = Mutex::new(0);
const TEST_MODEL_CONFIG_ID: &str = "test-model-config";

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
            let redactor = Arc::new(jyowo_harness_sdk::builtin::DefaultRedactor::default());
            let manager = jyowo_harness_sdk::BackgroundAgentManager::new(
                store,
                event_store,
                request.tenant_id,
                request.conversation_id,
                redactor.clone(),
            );
            let mut safe_input =
                harness_contracts::ConversationTurnInput::ask(request.goal.clone());
            safe_input.prompt =
                redactor.redact(&request.goal, &harness_contracts::RedactRules::default());
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

#[tokio::test]
async fn agent_orchestration_e2e_real_subagent_spawn_projects_activity() {
    let workspace = unique_workspace("real-subagent");
    jyowo_harness_sdk::list_agent_profiles(&workspace).expect("agent profiles initialize");
    let agent_tool_use_id = ToolUseId::new();
    let state = runtime_state_with_scripted_model_for_workspace(
        workspace,
        vec![
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::ToolUseComplete {
                        id: agent_tool_use_id,
                        name: "agent".to_owned(),
                        input: json!({
                            "role": "worker",
                            "task": "inspect repository"
                        }),
                    },
                },
                ModelStreamEvent::MessageStop,
            ]),
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("child completed".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]),
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("parent completed".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]),
        ],
    )
    .await;
    enable_agent_execution_settings(&state, false).await;
    let session_id = jyowo_harness_sdk::ext::SessionId::new();

    start_run_with_runtime_state(
        StartRunRequest {
            attachments: None,
            client_message_id: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
            permission_mode: Some(PermissionMode::BypassPermissions),
            prompt: "Delegate inspection".to_owned(),
        },
        &state,
    )
    .await
    .expect("subagent start_run succeeds");

    wait_for_event(&state, session_id, |event| {
        matches!(
            event,
            Event::SubagentSpawned(_) | Event::SubagentAnnounced(_)
        )
    })
    .await;
    let events = read_events(&state, session_id).await;
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::SubagentSpawned(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseRequested(requested) if requested.tool_name == "agent")));

    let page = page_conversation_worktree_with_runtime_state(
        PageConversationWorktreeRequest {
            conversation_id: session_id.to_string(),
            direction: PageConversationWorktreeDirection::After,
            limit: Some(20),
            page_cursor: None,
        },
        &state,
    )
    .await
    .expect("worktree page projects subagent activity");
    let projected = serde_json::to_value(&page).expect("page serializes");
    assert!(projected.to_string().contains("\"kind\":\"agentActivity\""));
    assert!(projected
        .to_string()
        .contains("\"activityKind\":\"subagent\""));
}

#[tokio::test]
async fn agent_orchestration_e2e_real_run_scoped_team_persists_and_projects() {
    let workspace = unique_workspace("real-team");
    jyowo_harness_sdk::list_agent_profiles(&workspace).expect("agent profiles initialize");
    let agent_team_tool_use_id = ToolUseId::new();
    let state = runtime_state_with_scripted_model_for_workspace(
        workspace.clone(),
        vec![
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::ToolUseComplete {
                        id: agent_team_tool_use_id,
                        name: "agent_team".to_owned(),
                        input: json!({
                            "goal": "Run with a scoped team",
                            "maxTurnsPerGoal": 4
                        }),
                    },
                },
                ModelStreamEvent::MessageStop,
            ]),
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("team lead completed".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]),
        ],
    )
    .await;
    enable_agent_execution_settings(&state, false).await;
    let session_id = jyowo_harness_sdk::ext::SessionId::new();

    let started = start_run_with_runtime_state(
        StartRunRequest {
            attachments: None,
            client_message_id: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
            permission_mode: Some(PermissionMode::BypassPermissions),
            prompt: "Run with a scoped team".to_owned(),
        },
        &state,
    )
    .await
    .expect("team start_run succeeds");

    let connection =
        rusqlite::Connection::open(workspace.join(".jyowo/runtime/agent-runtime.sqlite"))
            .expect("runtime sqlite opens");
    let (team_id, task_count): (String, i64) = connection
        .query_row(
            "SELECT team_id, COUNT(*) FROM agent_team_tasks WHERE run_id = ?1 GROUP BY team_id",
            [started.run_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("team task persists");
    assert_eq!(task_count, 1);
    let mailbox_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM agent_team_mailbox WHERE team_id = ?1",
            [team_id.as_str()],
            |row| row.get(0),
        )
        .expect("team mailbox persists");
    assert_eq!(mailbox_count, 1);

    wait_for_event(&state, session_id, |event| {
        matches!(event, Event::TeamCreated(_) | Event::TeamTaskUpdated(_))
    })
    .await;
    let page = page_conversation_worktree_with_runtime_state(
        PageConversationWorktreeRequest {
            conversation_id: session_id.to_string(),
            direction: PageConversationWorktreeDirection::After,
            limit: Some(20),
            page_cursor: None,
        },
        &state,
    )
    .await
    .expect("worktree page projects team activity");
    let projected = serde_json::to_value(&page).expect("page serializes");
    assert!(
        projected.to_string().contains("\"kind\":\"agentActivity\""),
        "projected page: {projected}"
    );
    assert!(projected
        .to_string()
        .contains("\"activityKind\":\"agentTeam\""));
    assert!(projected.to_string().contains("Run with a scoped team"));
}

#[tokio::test]
async fn agent_orchestration_e2e_real_background_agent_commands_and_recovery() {
    let workspace = unique_workspace("real-background-agent-tool");
    jyowo_harness_sdk::list_agent_profiles(&workspace).expect("agent profiles initialize");
    let background_tool_use_id = ToolUseId::new();
    let state = runtime_state_with_scripted_model_for_workspace(
        workspace,
        vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::ToolUseComplete {
                    id: background_tool_use_id,
                    name: "background_agent".to_owned(),
                    input: json!({
                        "goal": "Run as a background agent",
                        "title": "Background investigation"
                    }),
                },
            },
            ModelStreamEvent::MessageStop,
        ])],
    )
    .await;
    let supervisor = write_test_supervisor_lock(state.workspace_root());
    enable_agent_execution_settings(&state, true).await;
    let session_id = jyowo_harness_sdk::ext::SessionId::new();

    start_run_with_runtime_state(
        StartRunRequest {
            attachments: None,
            client_message_id: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
            permission_mode: Some(PermissionMode::BypassPermissions),
            prompt: "Run as a background agent".to_owned(),
        },
        &state,
    )
    .await
    .expect("background agent tool run succeeds");

    wait_for_event(&state, session_id, |event| {
        matches!(
            event,
            Event::ToolUseCompleted(completed)
                if completed.tool_use_id == background_tool_use_id
        )
    })
    .await;

    let listed = list_background_agents_with_runtime_state(
        ListBackgroundAgentsRequest {
            conversation_id: Some(session_id.to_string()),
            include_archived: false,
        },
        &state,
    )
    .await
    .expect("background list succeeds");
    assert_eq!(listed.agents.len(), 1);
    assert_eq!(listed.agents[0].title, "Background investigation");
    let background_agent_id = listed.agents[0].background_agent_id.clone();

    let detail = get_background_agent_with_runtime_state(
        GetBackgroundAgentRequest {
            background_agent_id: background_agent_id.clone(),
            conversation_id: Some(session_id.to_string()),
        },
        &state,
    )
    .await
    .expect("background detail succeeds");
    assert_eq!(detail.agent.state, BackgroundAgentState::Running);

    let cancelled = cancel_background_agent_with_runtime_state(
        BackgroundAgentIdRequest {
            background_agent_id: background_agent_id.clone(),
            conversation_id: Some(session_id.to_string()),
        },
        &state,
    )
    .await
    .expect("background cancel succeeds");
    assert_eq!(cancelled.agent.state, BackgroundAgentState::Cancelled);

    let store = jyowo_harness_sdk::AgentRuntimeStore::open(state.workspace_root())
        .expect("agent runtime store opens");
    let manager = jyowo_harness_sdk::BackgroundAgentManager::new(
        Arc::new(store),
        state.harness().expect("harness exists").event_store(),
        TenantId::SINGLE,
        session_id,
        Arc::new(jyowo_harness_sdk::builtin::DefaultRedactor::default()),
    );
    manager
        .recover_on_startup("test restart")
        .await
        .expect("restart recovery scans durable registry");
    let recovered = get_background_agent_with_runtime_state(
        GetBackgroundAgentRequest {
            background_agent_id,
            conversation_id: Some(session_id.to_string()),
        },
        &state,
    )
    .await
    .expect("background detail survives recovery");
    assert_eq!(recovered.agent.state, BackgroundAgentState::Cancelled);

    supervisor.shutdown().await;
}

#[tokio::test]
async fn agent_orchestration_e2e_negative_policy_and_permission_paths_fail_closed() {
    let session_id = jyowo_harness_sdk::ext::SessionId::new();
    let state = runtime_state_with_session_routed_model(session_id).await;

    set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        },
        &DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf()),
        Some(&AgentCapabilityResolutionContext {
            stream_permission_runtime_available: true,
        }),
    )
    .expect("disabled settings save");
    let disabled_policy = resolve_start_run_agent_policy(
        &StartRunRequest {
            attachments: None,
            client_message_id: None,
            context_references: None,
            conversation_id: jyowo_harness_sdk::ext::SessionId::new().to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
            permission_mode: None,
            prompt: "Run with disabled subagents".to_owned(),
        },
        &state,
    )
    .expect("disabled settings should resolve to foreground policy");
    assert_eq!(disabled_policy.options.subagents, AgentUsePolicy::Off);
    assert_eq!(disabled_policy.options.agent_team, AgentUsePolicy::Off);
    assert_eq!(
        disabled_policy.options.background_agents,
        AgentUsePolicy::Off
    );

    let unavailable_error = set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: true,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        },
        &DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf()),
        Some(&AgentCapabilityResolutionContext {
            stream_permission_runtime_available: false,
        }),
    )
    .expect_err("unavailable runtime disables settings switch");
    assert_eq!(unavailable_error.code, "INVALID_PAYLOAD");

    enable_agent_execution_settings(&state, false).await;
    let enabled_policy = resolve_start_run_agent_policy(
        &StartRunRequest {
            attachments: None,
            client_message_id: None,
            context_references: None,
            conversation_id: jyowo_harness_sdk::ext::SessionId::new().to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
            permission_mode: None,
            prompt: "Run configured child".to_owned(),
        },
        &state,
    )
    .expect("enabled settings should resolve agent policy");
    assert_eq!(enabled_policy.options.subagents, AgentUsePolicy::Allowed);
    assert_eq!(
        enabled_policy.options.workspace_isolation,
        AgentWorkspaceIsolationMode::ReadOnly
    );

    start_run_with_runtime_state(
        StartRunRequest {
            attachments: None,
            client_message_id: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
            permission_mode: None,
            prompt: "Trigger unsafe child action".to_owned(),
        },
        &state,
    )
    .await
    .expect("run starts and waits on permission");
    let agent_pending = wait_for_pending_permission(&state, |pending| {
        pending.request.session_id == session_id && pending.request.tool_name == "agent"
    })
    .await;
    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: jyowo_desktop_shell::commands::PermissionDecision::Approve,
            request_id: agent_pending.request.request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .expect("agent tool permission approval is accepted");
    let pending = wait_for_pending_permission(&state, |pending| {
        pending.request.tool_name == "NeedsPermission"
    })
    .await;
    let child_session_id = pending.request.session_id;
    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: pending.request.session_id.to_string(),
            decision: jyowo_desktop_shell::commands::PermissionDecision::Deny,
            request_id: pending.request.request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .expect("permission denial is accepted");
    wait_for_event(&state, child_session_id, |event| {
        matches!(event, Event::ToolUseDenied(denied) if denied.tool_use_id == pending.request.tool_use_id)
    })
    .await;
    let events = read_events(&state, session_id).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::SubagentSpawned(_))),
        "parent events: {events:#?}"
    );
    assert!(events.iter().any(|event| matches!(
        event,
        Event::SubagentTerminated(terminated)
            if terminated.parent_session_id == session_id
    )));
    let child_events = read_events(&state, child_session_id).await;
    assert!(
        child_events.iter().any(|event| matches!(
            event,
            Event::PermissionRequested(requested)
                if requested.tool_name == "NeedsPermission"
                    && matches!(requested.actor_source, PermissionActorSource::Subagent { parent_session_id, .. } if parent_session_id == session_id)
        )),
        "child events: {child_events:#?}"
    );
    assert!(child_events.iter().any(|event| matches!(
        event,
        Event::PermissionResolved(resolved)
            if resolved.decision == jyowo_harness_sdk::ext::Decision::DenyOnce
    )));
    assert!(
        child_events
            .iter()
            .any(|event| matches!(event, Event::ToolUseDenied(denied) if denied.tool_use_id == pending.request.tool_use_id)),
        "child events: {child_events:#?}"
    );
    assert!(
        !child_events.iter().any(|event| {
            matches!(event, Event::ToolUseCompleted(completed) if completed.tool_use_id == pending.request.tool_use_id)
        }),
        "denied child tool must not complete: {child_events:#?}"
    );
}

async fn runtime_state_with_scripted_model_for_workspace(
    workspace: PathBuf,
    responses: Vec<ScriptedResponse>,
) -> DesktopRuntimeState {
    std::fs::create_dir_all(&workspace).expect("workspace dir");
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
                    .expect("tool registry builds"),
            )
            .build()
            .await
            .expect("harness builds"),
    );

    DesktopRuntimeState::with_harness_and_stream_permission_runtime_for_workspace(
        workspace,
        harness,
        stream_permission_runtime,
    )
    .expect("state uses harness broker")
}

fn write_test_provider_settings(workspace: &Path) {
    let workspace = workspace
        .canonicalize()
        .expect("test workspace should canonicalize");
    DesktopProviderSettingsStore::new(workspace)
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Messages,
                base_url: None,
                display_name: "Test provider".to_owned(),
                id: TEST_MODEL_CONFIG_ID.to_owned(),
                model_id: "test-model".to_owned(),
                official_quota_api_key: None,
                provider_id: "test".to_owned(),
                model_descriptor: ProviderModelDescriptorRecord {
                    protocol: ModelProtocol::Messages,
                    conversation_capability: ConversationModelCapabilityRecord {
                        input_modalities: vec![ProviderModelModalityRecord::Text],
                        output_modalities: vec![ProviderModelModalityRecord::Text],
                        context_window: 128_000,
                        max_output_tokens: 8192,
                        streaming: true,
                        tool_calling: true,
                        reasoning: false,
                        prompt_cache: false,
                        structured_output: true,
                    },
                    context_window: 128_000,
                    display_name: "Test model".to_owned(),
                    lifecycle: ProviderModelLifecycleRecord::Stable,
                    max_output_tokens: 8192,
                    model_id: "test-model".to_owned(),
                    provider_id: "test".to_owned(),
                },
            }],
        })
        .expect("test provider settings save");
}

async fn runtime_state_with_session_routed_model(
    parent_session_id: jyowo_harness_sdk::ext::SessionId,
) -> DesktopRuntimeState {
    let workspace = unique_workspace("session-routed");
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    write_test_provider_settings(&workspace);
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let harness = Arc::new(
        Harness::builder()
            .with_options(test_harness_options(&workspace))
            .with_model_arc(Arc::new(SessionRoutedProvider::new(parent_session_id)))
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_stream_permission_broker_arc(
                stream_permission_runtime.broker(),
                stream_permission_runtime.resolver_handle(),
            )
            .with_tool_registry(
                ToolRegistry::builder()
                    .with_tool(Box::<NeedsPermissionTool>::default())
                    .build()
                    .expect("tool registry builds"),
            )
            .build()
            .await
            .expect("harness builds"),
    );

    DesktopRuntimeState::with_harness_and_stream_permission_runtime_for_workspace(
        workspace,
        harness,
        stream_permission_runtime,
    )
    .expect("state uses harness broker")
}

fn test_harness_options(workspace: &Path) -> HarnessOptions {
    let mut options = HarnessOptions::default();
    options.workspace_root = workspace.to_path_buf();
    options.model_id = "test-model".to_owned();
    options
}

async fn enable_agent_execution_settings(state: &DesktopRuntimeState, background: bool) {
    set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: true,
            agent_teams_enabled: true,
            background_agents_enabled: background,
        },
        &DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf()),
        Some(&AgentCapabilityResolutionContext {
            stream_permission_runtime_available: true,
        }),
    )
    .expect("execution settings save");
}

async fn read_events(
    state: &DesktopRuntimeState,
    session_id: jyowo_harness_sdk::ext::SessionId,
) -> Vec<Event> {
    state
        .harness()
        .expect("harness exists")
        .event_store()
        .read_envelopes(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("events read")
        .map(|envelope| envelope.payload)
        .collect()
        .await
}

async fn wait_for_event(
    state: &DesktopRuntimeState,
    session_id: jyowo_harness_sdk::ext::SessionId,
    predicate: impl Fn(&Event) -> bool,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let events = read_events(state, session_id).await;
        if events.iter().any(&predicate) {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("expected event did not appear: {events:?}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_pending_permission(
    state: &DesktopRuntimeState,
    predicate: impl Fn(&jyowo_harness_sdk::ext::PendingPermissionRequest) -> bool,
) -> jyowo_harness_sdk::ext::PendingPermissionRequest {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(pending) = state
            .pending_permission_requests()
            .into_iter()
            .find(|pending| predicate(pending))
        {
            return pending;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("pending permission did not appear");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

struct NeedsPermissionTool {
    descriptor: ToolDescriptor,
}

impl Default for NeedsPermissionTool {
    fn default() -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: "NeedsPermission".to_owned(),
                display_name: "NeedsPermission".to_owned(),
                description: "Requests command permission for desktop E2E tests.".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::Custom("test".to_owned()),
                version: "0.1.0".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": { "command": { "type": "string" } },
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
                    metric: jyowo_harness_sdk::ext::BudgetMetric::Chars,
                    limit: 30_000,
                    on_overflow: OverflowAction::Offload,
                    preview_head_chars: 2_000,
                    preview_tail_chars: 2_000,
                },
                provider_restriction: ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
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

struct SessionRoutedProvider {
    parent_session_id: jyowo_harness_sdk::ext::SessionId,
    parent_responses: AsyncMutex<VecDeque<ScriptedResponse>>,
    child_responses: AsyncMutex<VecDeque<ScriptedResponse>>,
    requests: AsyncMutex<Vec<ModelRequest>>,
}

impl SessionRoutedProvider {
    fn new(parent_session_id: jyowo_harness_sdk::ext::SessionId) -> Self {
        Self {
            parent_session_id,
            parent_responses: AsyncMutex::new(
                vec![
                    ScriptedResponse::Stream(vec![
                        ModelStreamEvent::ContentBlockDelta {
                            index: 0,
                            delta: ContentDelta::ToolUseComplete {
                                id: ToolUseId::new(),
                                name: "agent".to_owned(),
                                input: json!({
                                    "role": "worker",
                                    "task": "try unsafe write"
                                }),
                            },
                        },
                        ModelStreamEvent::MessageStop,
                    ]),
                    ScriptedResponse::Stream(vec![ModelStreamEvent::MessageStop]),
                ]
                .into(),
            ),
            child_responses: AsyncMutex::new(
                vec![
                    ScriptedResponse::Stream(vec![
                        ModelStreamEvent::ContentBlockDelta {
                            index: 0,
                            delta: ContentDelta::ToolUseComplete {
                                id: ToolUseId::new(),
                                name: "NeedsPermission".to_owned(),
                                input: json!({ "command": "write-file" }),
                            },
                        },
                        ModelStreamEvent::MessageStop,
                    ]),
                    ScriptedResponse::Stream(vec![ModelStreamEvent::MessageStop]),
                ]
                .into(),
            ),
            requests: AsyncMutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ModelProvider for SessionRoutedProvider {
    fn provider_id(&self) -> &str {
        "test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            provider_id: "test".to_owned(),
            model_id: "test-model".to_owned(),
            display_name: "Test model".to_owned(),
            protocol: ModelProtocol::Messages,
            context_window: 128_000,
            max_output_tokens: 8192,
            conversation_capability: Default::default(),
            lifecycle: ModelLifecycle::Stable,
            pricing: None,
        }]
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        self.requests.lock().await.push(req);
        let is_parent = ctx.session_id == Some(self.parent_session_id);
        let response = if is_parent {
            self.parent_responses.lock().await.pop_front()
        } else {
            self.child_responses.lock().await.pop_front()
        }
        .unwrap_or_else(|| ScriptedResponse::Stream(vec![ModelStreamEvent::MessageStop]));

        match response {
            ScriptedResponse::Stream(events) => Ok(Box::pin(stream::iter(events))),
            ScriptedResponse::Error(error) => Err(error),
            ScriptedResponse::WaitForCancel => {
                ctx.cancel.cancelled().await;
                Err(ModelError::Cancelled)
            }
        }
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

fn write_test_supervisor_lock(workspace: &Path) -> TestSupervisorControl {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("control bind");
    listener.set_nonblocking(true).expect("control nonblocking");
    let control_addr = listener.local_addr().expect("control addr");
    let runtime_dir = workspace.join(".jyowo/runtime");
    std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
    let token = "test-background-supervisor-token";
    let token_hash = blake3::hash(token.as_bytes()).to_hex().to_string();
    let workspace_id = blake3::hash(workspace.display().to_string().as_bytes())
        .to_hex()
        .to_string();
    std::fs::write(
        runtime_dir.join("agent-supervisor.token"),
        serde_json::json!({
            "token": token,
            "tokenHash": token_hash,
            "tokenEpoch": 1,
            "workspaceId": workspace_id,
            "createdAt": chrono::Utc::now(),
        })
        .to_string(),
    )
    .expect("token write");
    std::fs::write(
        runtime_dir.join("agent-supervisor.lock"),
        serde_json::json!({
            "status": "running",
            "workspaceId": workspace_id,
            "tokenHash": token_hash,
            "tokenEpoch": 1,
            "pid": std::process::id(),
            "controlAddr": control_addr.to_string(),
            "startedAt": chrono::Utc::now(),
            "heartbeatAt": chrono::Utc::now(),
        })
        .to_string(),
    )
    .expect("lock write");

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let thread = std::thread::spawn(move || {
        while !thread_stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, peer)) => {
                    let mut buffer = [0_u8; 8192];
                    let Ok(read) = stream.read(&mut buffer) else {
                        continue;
                    };
                    let request = serde_json::from_slice::<serde_json::Value>(&buffer[..read])
                        .unwrap_or_else(|_| serde_json::json!({}));
                    let ok = peer.ip().is_loopback()
                        && request.get("token").and_then(Value::as_str) == Some(token)
                        && request.get("request").and_then(Value::as_str).is_some();
                    let response = serde_json::json!({
                        "ok": ok,
                        "status": if ok { "running" } else { "unauthorized" },
                    });
                    let _ = stream.write_all(response.to_string().as_bytes());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });
    TestSupervisorControl {
        stop,
        thread: Some(thread),
    }
}

struct TestSupervisorControl {
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TestSupervisorControl {
    async fn shutdown(mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn unique_workspace(label: &str) -> PathBuf {
    let mut counter = WORKSPACE_COUNTER.lock();
    *counter += 1;
    let path = std::env::temp_dir().join(format!(
        "jyowo-agent-orchestration-e2e-{label}-{}-{}",
        std::process::id(),
        *counter
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("workspace dir");
    path
}

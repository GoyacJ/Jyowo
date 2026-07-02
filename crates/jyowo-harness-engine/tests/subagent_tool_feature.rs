#![cfg(feature = "subagent-tool")]

use std::{
    fs,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::{
    AssistantMessageCompletedEvent, BlobStore, BudgetKind, CapabilityRegistry, ConfigHash,
    Decision, DeferPolicy, EndReason, Event, McpOrigin, McpServerId, McpServerSource, Message,
    MessageContent, MessageId, MessagePart, MessageRole, ModelError, ModelProtocol, NetworkAccess,
    NoopRedactor, PermissionError, PermissionMode, ProviderRestriction, ResourceLimits,
    ResultBudget, RunId, RunModelSnapshot, RunStartedEvent, SandboxMode, SandboxPolicy,
    SandboxScope, SessionId, SnapshotId, StopReason, SubagentTerminationReason, TenantId,
    ToolActionPlan, ToolCapability, ToolDescriptor, ToolError, ToolGroup, ToolOrigin,
    ToolProperties, ToolResult, ToolUseCompletedEvent, ToolUseId, TrustLevel, TurnInput,
    UsageSnapshot, UserMessageAppendedEvent, WorkspaceAccess,
};
use harness_engine::{Engine, EngineId, EngineRunner, RunContext, SessionHandle};
use harness_journal::{EventStore, ReplayCursor};
use harness_model::{
    ContentDelta, ConversationModelCapability, HealthStatus, InferContext, ModelDescriptor,
    ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionRequest};
use harness_sandbox::{
    ExecContext, ExecSpec, ProcessHandle, SandboxBackend, SandboxCapabilities, SessionSnapshotFile,
    SnapshotSpec,
};
use harness_subagent::{
    AnnounceMode, BootstrapFilter, MemorySelector, ParentContext, RequiredSandboxCapabilities,
    ResourceQuota, SandboxInheritance, SubagentAnnouncement, SubagentContextMode, SubagentHandle,
    SubagentInputStrategy, SubagentMemoryScope, SubagentRunner, SubagentRunnerCapAdapter,
    SubagentSpec, SubagentStatus, ToolsetSelector,
};
use harness_tool::{
    action_plan_from_permission_check, AuthorizedToolInput, Tool, ToolContext, ToolEvent, ToolPool,
    ToolStream, ValidationError,
};

#[test]
fn subagent_tool_feature_appends_agent_tool_when_enabled() {
    let workspace = tempfile::tempdir().unwrap();
    let mut registry = CapabilityRegistry::default();
    registry.install::<dyn harness_contracts::SubagentRunnerCap>(
        ToolCapability::SubagentRunner,
        SubagentRunnerCapAdapter::from_runner(Arc::new(ReadyRunner)),
    );

    let engine = Engine::builder()
        .with_engine_id(EngineId::new("subagent-tool-feature"))
        .with_event_store(Arc::new(harness_journal::InMemoryEventStore::new(
            Arc::new(NoopRedactor),
        )))
        .with_context(harness_context::ContextEngine::builder().build().unwrap())
        .with_hooks(harness_hook::HookDispatcher::new(
            harness_hook::HookRegistry::builder()
                .build()
                .unwrap()
                .snapshot(),
        ))
        .with_model(Arc::new(EmptyModel))
        .with_tools(ToolPool::default())
        .with_permission_broker(Arc::new(AllowBroker))
        .with_workspace_root(workspace.path())
        .with_model_id("empty-model")
        .with_cap_registry(Arc::new(registry))
        .with_subagent_tool()
        .build()
        .unwrap();

    assert!(engine.has_tool("agent"));
}

#[test]
fn subagent_tool_feature_installs_default_runner_when_cap_missing() {
    let workspace = tempfile::tempdir().unwrap();
    let engine = Engine::builder()
        .with_engine_id(EngineId::new("subagent-tool-default-runner"))
        .with_event_store(Arc::new(harness_journal::InMemoryEventStore::new(
            Arc::new(NoopRedactor),
        )))
        .with_context(harness_context::ContextEngine::builder().build().unwrap())
        .with_hooks(harness_hook::HookDispatcher::new(
            harness_hook::HookRegistry::builder()
                .build()
                .unwrap()
                .snapshot(),
        ))
        .with_model(Arc::new(EmptyModel))
        .with_tools(ToolPool::default())
        .with_permission_broker(Arc::new(AllowBroker))
        .with_workspace_root(workspace.path())
        .with_model_id("empty-model")
        .with_subagent_tool()
        .build()
        .unwrap();

    assert!(engine.has_tool("agent"));
}

#[tokio::test]
async fn default_runner_scopes_child_tools_and_announces_child_output() {
    let workspace = tempfile::tempdir().unwrap();
    let mut tools = ToolPool::default();
    tools.append_runtime_tool(Arc::new(TestTool::new("explicit", ToolOrigin::Builtin)));
    tools.append_runtime_tool(Arc::new(TestTool::new("mcp_extra", mcp_origin("srv-a"))));
    let mut spec = SubagentSpec::minimal("worker", "summarize");
    spec.toolset = ToolsetSelector::Custom(vec!["explicit".to_owned()]);
    spec.mcp_servers = vec!["srv-a".into()];
    spec.required_mcp_servers = vec!["srv-a".into()];
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = Engine::builder()
        .with_engine_id(EngineId::new("subagent-tool-product-path"))
        .with_event_store(store.clone())
        .with_context(harness_context::ContextEngine::builder().build().unwrap())
        .with_hooks(harness_hook::HookDispatcher::new(
            harness_hook::HookRegistry::builder()
                .build()
                .unwrap()
                .snapshot(),
        ))
        .with_model(model.clone())
        .with_tools(tools)
        .with_permission_broker(Arc::new(AllowBroker))
        .with_workspace_root(workspace.path())
        .with_model_id("test-model")
        .with_subagent_tool()
        .build()
        .unwrap();
    let tenant_id = TenantId::SINGLE;
    let parent_session_id = SessionId::new();
    let parent_run_id = RunId::new();
    let parent_ctx = RunContext::new(tenant_id, parent_session_id, parent_run_id);

    let events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            parent_ctx,
        )
        .await
        .unwrap()
        .collect()
        .await;

    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::RunEnded(run) if run.run_id == parent_run_id && run.reason == EndReason::Completed
        )
    }));
    let requests = model.requests();
    assert_eq!(requests.len(), 3);
    let child_tools: Vec<_> = requests[1]
        .tools
        .as_ref()
        .expect("child tool snapshot should be present")
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert_eq!(child_tools, vec!["explicit"]);

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentAnnounced(announced)
                if announced.summary == "child answer"
                    && announced.result == Some(serde_json::json!({ "text": "child answer" }))
                    && announced.usage.output_tokens == 7
        )
    }));
}

#[tokio::test]
async fn default_runner_removes_default_blocklist_from_child_tool_snapshot() {
    let workspace = tempfile::tempdir().unwrap();
    let mut tools = ToolPool::default();
    tools.append_runtime_tool(Arc::new(TestTool::new("safe_tool", ToolOrigin::Builtin)));
    tools.append_runtime_tool(Arc::new(TestTool::new("execute_code", ToolOrigin::Builtin)));
    tools.append_runtime_tool(Arc::new(TestTool::new(
        "send_user_message",
        ToolOrigin::Builtin,
    )));
    let spec = SubagentSpec::minimal("worker", "use safe tools");
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-default-blocklist",
        store,
        model.clone(),
        tools,
        workspace.path(),
        None,
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let requests = model.requests();
    let child_tools: Vec<_> = requests[1]
        .tools
        .as_ref()
        .expect("child tool snapshot should be present")
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert!(child_tools.contains(&"safe_tool"));
    assert!(!child_tools.contains(&"agent"));
    assert!(!child_tools.contains(&"execute_code"));
    assert!(!child_tools.contains(&"send_user_message"));
}

#[tokio::test]
async fn required_mcp_missing_fails_closed_before_child_model_call() {
    let workspace = tempfile::tempdir().unwrap();
    let mut spec = SubagentSpec::minimal("worker", "needs missing mcp");
    spec.required_mcp_servers = vec!["srv-missing".into()];
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-required-mcp-missing",
        store.clone(),
        model.clone(),
        ToolPool::default(),
        workspace.path(),
        None,
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentTerminated(terminated)
                if matches!(
                    &terminated.reason,
                    harness_contracts::SubagentTerminationReason::Failed { detail }
                        if detail.contains("required mcp servers")
                            && detail.contains("srv-missing")
                )
        )
    }));
    let all_envelopes = store
        .query_after(TenantId::SINGLE, None, 100)
        .await
        .unwrap();
    assert!(!all_envelopes.iter().any(|envelope| {
        matches!(
            &envelope.payload,
            Event::RunStarted(started) if started.parent_run_id.is_some()
        )
    }));
}

#[tokio::test]
async fn user_controlled_mcp_server_fails_closed_before_child_run() {
    let workspace = tempfile::tempdir().unwrap();
    let mut tools = ToolPool::default();
    tools.append_runtime_tool(Arc::new(TestTool::new(
        "mcp_user",
        mcp_origin_with(
            "srv-user",
            McpServerSource::User,
            TrustLevel::UserControlled,
        ),
    )));
    let mut spec = SubagentSpec::minimal("worker", "needs user mcp");
    spec.mcp_servers = vec!["srv-user".into()];
    spec.required_mcp_servers = vec!["srv-user".into()];
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-user-mcp-fail-closed",
        store.clone(),
        model.clone(),
        tools,
        workspace.path(),
        None,
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentTerminated(terminated)
                if matches!(
                    &terminated.reason,
                    harness_contracts::SubagentTerminationReason::Failed { detail }
                        if detail.contains("srv-user")
                            && detail.contains("user-controlled")
                )
        )
    }));
    let all_envelopes = store
        .query_after(TenantId::SINGLE, None, 100)
        .await
        .unwrap();
    assert!(!all_envelopes.iter().any(|envelope| {
        matches!(
            &envelope.payload,
            Event::RunStarted(started) if started.parent_run_id.is_some()
        )
    }));
}

#[tokio::test]
async fn sandbox_inherit_passes_parent_sandbox_to_child_tool_context() {
    let workspace = tempfile::tempdir().unwrap();
    let probe = Arc::new(SandboxProbeTool::new("probe"));
    let mut tools = ToolPool::default();
    tools.append_runtime_tool(probe.clone());
    let mut spec = SubagentSpec::minimal("worker", "probe sandbox");
    spec.toolset = ToolsetSelector::Custom(vec!["probe".to_owned()]);
    spec.sandbox_policy = SandboxInheritance::Inherit;
    let model = Arc::new(DelegatingModel::with_child_events(
        serde_json::to_value(spec).unwrap(),
        tool_events("probe", serde_json::json!({})),
    ));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine_with_sandbox(
        "subagent-sandbox-inherit",
        store,
        model,
        tools,
        workspace.path(),
        None,
        Some(Arc::new(FakeSandbox::new("parent-sandbox"))),
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    assert_eq!(
        probe.seen_sandboxes(),
        vec![Some("parent-sandbox".to_owned())]
    );
}

#[tokio::test]
async fn sandbox_empty_removes_parent_sandbox_from_child_tool_context() {
    let workspace = tempfile::tempdir().unwrap();
    let probe = Arc::new(SandboxProbeTool::new("probe"));
    let mut tools = ToolPool::default();
    tools.append_runtime_tool(probe.clone());
    let mut spec = SubagentSpec::minimal("worker", "probe sandbox");
    spec.toolset = ToolsetSelector::Custom(vec!["probe".to_owned()]);
    spec.sandbox_policy = SandboxInheritance::Empty;
    let model = Arc::new(DelegatingModel::with_child_events(
        serde_json::to_value(spec).unwrap(),
        tool_events("probe", serde_json::json!({})),
    ));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine_with_sandbox(
        "subagent-sandbox-empty",
        store,
        model,
        tools,
        workspace.path(),
        None,
        Some(Arc::new(FakeSandbox::new("parent-sandbox"))),
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    assert_eq!(probe.seen_sandboxes(), vec![None]);
}

#[tokio::test]
async fn sandbox_require_mismatch_fails_closed_before_child_run() {
    let workspace = tempfile::tempdir().unwrap();
    let mut spec = SubagentSpec::minimal("worker", "requires sandbox");
    spec.sandbox_policy = SandboxInheritance::Require(RequiredSandboxCapabilities {
        supports_network: true,
        ..RequiredSandboxCapabilities::default()
    });
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine_with_sandbox(
        "subagent-sandbox-require-mismatch",
        store.clone(),
        model.clone(),
        ToolPool::default(),
        workspace.path(),
        None,
        Some(Arc::new(FakeSandbox::new("actual-sandbox"))),
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentTerminated(terminated)
                if matches!(
                    &terminated.reason,
                    harness_contracts::SubagentTerminationReason::Failed { detail }
                        if detail.contains("sandbox requirements are not satisfied")
                            || detail.contains("sandbox capability mismatch")
                                && detail.contains("supports_network")
                )
        )
    }));
    let all_envelopes = store
        .query_after(TenantId::SINGLE, None, 100)
        .await
        .unwrap();
    assert!(!all_envelopes.iter().any(|envelope| {
        matches!(
            &envelope.payload,
            Event::RunStarted(started) if started.parent_run_id.is_some()
        )
    }));
}

#[tokio::test]
async fn sandbox_override_applies_policy_to_child_sandbox_execution() {
    let workspace = tempfile::tempdir().unwrap();
    let sandbox = Arc::new(FakeSandbox::new("parent-sandbox").with_capabilities(
        SandboxCapabilities {
            supports_network: true,
            supports_filesystem_write: true,
            resource_limit_support: harness_sandbox::ResourceLimitSupport {
                wall_clock: true,
                ..Default::default()
            },
            ..SandboxCapabilities::default()
        },
    ));
    let probe = Arc::new(SandboxProbeTool::new("probe"));
    let mut tools = ToolPool::default();
    tools.append_runtime_tool(probe.clone());
    let override_policy = sandbox_policy(NetworkAccess::LoopbackOnly);
    let mut spec = SubagentSpec::minimal("worker", "override sandbox");
    spec.toolset = ToolsetSelector::Custom(vec!["probe".to_owned()]);
    spec.sandbox_policy = SandboxInheritance::Override(override_policy.clone());
    let model = Arc::new(DelegatingModel::with_child_events(
        serde_json::to_value(spec).unwrap(),
        tool_events("probe", serde_json::json!({})),
    ));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine_with_sandbox(
        "subagent-sandbox-override",
        store,
        model,
        tools,
        workspace.path(),
        None,
        Some(sandbox.clone()),
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    assert_eq!(
        probe.seen_sandboxes(),
        vec![Some("parent-sandbox".to_owned())]
    );
    assert_eq!(sandbox.seen_policies(), vec![override_policy]);
}

#[tokio::test]
async fn sandbox_override_without_parent_sandbox_fails_closed_before_child_run() {
    let workspace = tempfile::tempdir().unwrap();
    let mut spec = SubagentSpec::minimal("worker", "override sandbox");
    spec.sandbox_policy = SandboxInheritance::Override(sandbox_policy(NetworkAccess::None));
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-sandbox-override-missing",
        store.clone(),
        model.clone(),
        ToolPool::default(),
        workspace.path(),
        None,
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentTerminated(terminated)
                if matches!(
                    &terminated.reason,
                    harness_contracts::SubagentTerminationReason::Failed { detail }
                        if detail.contains("sandbox override requires parent sandbox")
                )
        )
    }));
    let all_envelopes = store
        .query_after(TenantId::SINGLE, None, 100)
        .await
        .unwrap();
    assert!(!all_envelopes.iter().any(|envelope| {
        matches!(
            &envelope.payload,
            Event::RunStarted(started) if started.parent_run_id.is_some()
        )
    }));
}

#[tokio::test]
async fn subagent_max_turns_maps_to_max_iterations_status() {
    let workspace = tempfile::tempdir().unwrap();
    let mut spec = SubagentSpec::minimal("worker", "loop once");
    spec.max_turns = 1;
    let model = Arc::new(DelegatingModel::with_child_events(
        serde_json::to_value(spec).unwrap(),
        tool_events("missing_tool", serde_json::json!({})),
    ));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-max-turns",
        store.clone(),
        model,
        ToolPool::default(),
        workspace.path(),
        None,
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentAnnounced(announced)
                if announced.status == SubagentStatus::MaxIterationsReached
        )
    }));
}

#[tokio::test]
async fn subagent_token_quota_maps_to_max_budget_status() {
    let workspace = tempfile::tempdir().unwrap();
    let mut spec = SubagentSpec::minimal("worker", "spend tokens");
    spec.quota = Some(ResourceQuota {
        max_tokens: Some(1),
        max_tool_calls: None,
        max_duration: None,
        max_cost_cents: None,
    });
    let model = Arc::new(DelegatingModel::with_child_events(
        serde_json::to_value(spec).unwrap(),
        text_events("child costly", usage(0, 2, 0)),
    ));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-token-quota",
        store.clone(),
        model,
        ToolPool::default(),
        workspace.path(),
        None,
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentAnnounced(announced)
                if announced.status == SubagentStatus::MaxBudget(BudgetKind::Tokens)
        )
    }));
}

#[tokio::test]
async fn subagent_tool_call_quota_maps_to_max_budget_and_records_child_usage() {
    let workspace = tempfile::tempdir().unwrap();
    let mut tools = ToolPool::default();
    tools.append_runtime_tool(Arc::new(TestTool::new("explicit", ToolOrigin::Builtin)));
    let mut spec = SubagentSpec::minimal("worker", "use one tool");
    spec.toolset = ToolsetSelector::Custom(vec!["explicit".to_owned()]);
    spec.quota = Some(ResourceQuota {
        max_tokens: None,
        max_tool_calls: Some(1),
        max_duration: None,
        max_cost_cents: None,
    });
    let model = Arc::new(DelegatingModel::with_child_events(
        serde_json::to_value(spec).unwrap(),
        tool_events("explicit", serde_json::json!({})),
    ));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-tool-call-quota",
        store.clone(),
        model,
        tools,
        workspace.path(),
        None,
    );
    let parent_session_id = SessionId::new();
    let parent_run_id = RunId::new();
    let parent_ctx = RunContext::new(TenantId::SINGLE, parent_session_id, parent_run_id);
    let parent_correlation_id = parent_ctx.correlation_id;

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            parent_ctx,
        )
        .await
        .unwrap()
        .collect()
        .await;

    let parent_journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(parent_journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentAnnounced(announced)
                if announced.status == SubagentStatus::MaxBudget(BudgetKind::ToolCalls)
                    && announced.usage.tool_calls == 1
        )
    }));

    let all_envelopes = store
        .query_after(TenantId::SINGLE, None, 100)
        .await
        .unwrap();
    let child_run = all_envelopes
        .iter()
        .find_map(|envelope| match &envelope.payload {
            Event::RunStarted(started) if started.parent_run_id == Some(parent_run_id) => {
                assert_eq!(started.correlation_id, parent_correlation_id);
                Some(started.run_id)
            }
            _ => None,
        })
        .expect("child run should be journaled with parent run id");

    assert!(all_envelopes.iter().any(|envelope| {
        matches!(
            &envelope.payload,
            Event::RunEnded(ended)
                if ended.run_id == child_run
                    && ended.reason == EndReason::BudgetExhausted(BudgetKind::ToolCalls)
                    && ended.usage.as_ref().is_some_and(|usage| usage.tool_calls == 1)
        )
    }));
}

#[tokio::test]
async fn isolated_child_request_does_not_include_parent_transcript() {
    let mut spec = SubagentSpec::minimal("worker", "summarize");
    spec.context_mode = SubagentContextMode::Isolated;
    spec.input_strategy = SubagentInputStrategy::LatestUserOnly;

    let requests = run_delegating_with_prior_parent_events(
        "subagent-isolated-context",
        spec,
        parent_transcript_events("prior user", "prior assistant", "prior latest"),
        None,
    )
    .await;
    let child_texts = message_texts(&requests[1].messages);

    assert_eq!(child_texts, vec!["summarize"]);
}

#[tokio::test]
async fn fork_latest_user_seeds_child_prompt_with_latest_parent_user() {
    let mut spec = SubagentSpec::minimal("worker", "summarize");
    spec.context_mode = SubagentContextMode::ForkFromParent {
        include_tool_results: false,
    };
    spec.input_strategy = SubagentInputStrategy::LatestUserOnly;

    let requests = run_delegating_with_prior_parent_events(
        "subagent-fork-latest-user",
        spec,
        parent_transcript_events("prior user", "prior assistant", "prior latest"),
        None,
    )
    .await;
    let child_texts = message_texts(&requests[1].messages);

    assert_eq!(child_texts, vec!["delegate", "summarize"]);
}

#[tokio::test]
async fn fork_inherit_all_excludes_tool_results_when_disabled() {
    let mut spec = SubagentSpec::minimal("worker", "summarize");
    spec.context_mode = SubagentContextMode::ForkFromParent {
        include_tool_results: false,
    };
    spec.input_strategy = SubagentInputStrategy::InheritAll;

    let requests = run_delegating_with_prior_parent_events(
        "subagent-fork-inherit-without-tools",
        spec,
        parent_transcript_events("prior user", "prior assistant", "prior latest"),
        None,
    )
    .await;
    let child_messages = &requests[1].messages;

    assert!(!child_messages
        .iter()
        .any(|message| message.role == MessageRole::Tool));
    let child_texts = message_texts(child_messages);
    assert!(child_texts.contains(&"prior user"));
    assert!(child_texts.contains(&"prior assistant"));
    assert!(child_texts.contains(&"prior latest"));
    assert!(child_texts.contains(&"delegate"));
    assert!(child_texts.contains(&"summarize"));
}

#[tokio::test]
async fn fork_inherit_all_includes_tool_results_when_enabled() {
    let mut spec = SubagentSpec::minimal("worker", "summarize");
    spec.context_mode = SubagentContextMode::ForkFromParent {
        include_tool_results: true,
    };
    spec.input_strategy = SubagentInputStrategy::InheritAll;

    let requests = run_delegating_with_prior_parent_events(
        "subagent-fork-inherit-with-tools",
        spec,
        parent_transcript_events("prior user", "prior assistant", "prior latest"),
        None,
    )
    .await;
    let child_messages = &requests[1].messages;

    assert!(child_messages.iter().any(|message| {
        message.role == MessageRole::Tool
            && matches!(
                message.parts.as_slice(),
                [MessagePart::ToolResult { content: ToolResult::Text(text), .. }]
                    if text == "prior tool output"
            )
    }));
}

#[tokio::test]
async fn subagent_system_header_extra_only_scopes_child_prompt() {
    let workspace = tempfile::tempdir().unwrap();
    let mut spec = SubagentSpec::minimal("worker", "summarize");
    spec.system_header_extra = Some("child-only-system".to_owned());
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-system-extra",
        store,
        model.clone(),
        ToolPool::default(),
        workspace.path(),
        Some("parent-system"),
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let requests = model.requests();
    let parent_system = requests[0].system.as_deref().unwrap();
    let child_system = requests[1].system.as_deref().unwrap();
    assert_eq!(parent_system, "parent-system");
    assert_eq!(
        &child_system.as_bytes()[..parent_system.len()],
        parent_system.as_bytes()
    );
    assert_eq!(
        child_system,
        "parent-system\n\n<subagent-addendum>\nchild-only-system\n</subagent-addendum>"
    );
    assert_eq!(requests[2].system.as_deref(), Some("parent-system"));
}

#[tokio::test]
async fn inherit_all_with_no_bootstrap_files_runs_without_inherited_files() {
    let workspace = tempfile::tempdir().unwrap();
    let mut spec = SubagentSpec::minimal("worker", "inherit bootstrap");
    spec.bootstrap_filter = BootstrapFilter::InheritAll;
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-bootstrap-fail-closed",
        store.clone(),
        model,
        ToolPool::default(),
        workspace.path(),
        None,
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentAnnounced(announced)
                if announced.context_report.as_ref().is_some_and(|report|
                    report.bootstrap_files_inherited.is_empty()
                )
        )
    }));
}

#[tokio::test]
async fn subagent_bootstrap_allow_inherits_only_named_root_files() {
    let workspace = tempfile::tempdir().unwrap();
    fs::write(workspace.path().join("AGENTS.md"), "agent rules").unwrap();
    fs::write(workspace.path().join("CLAUDE.md"), "claude rules").unwrap();
    let mut spec = SubagentSpec::minimal("worker", "inherit selected bootstrap");
    spec.bootstrap_filter = BootstrapFilter::Allow(vec!["AGENTS.md".to_owned()]);
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-bootstrap-allow",
        store.clone(),
        model.clone(),
        ToolPool::default(),
        workspace.path(),
        Some("parent-system"),
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let requests = model.requests();
    let child_system = requests[1]
        .system
        .as_deref()
        .expect("child system should include inherited bootstrap");
    assert!(child_system.contains("parent-system"));
    assert!(child_system.contains(r#"<workspace-instructions source="AGENTS.md">"#));
    assert!(child_system.contains("agent rules"));
    assert!(!child_system.contains("CLAUDE.md"));
    assert!(!child_system.contains("claude rules"));

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentAnnounced(announced)
                if announced.context_report.as_ref().is_some_and(|report|
                    report.bootstrap_files_inherited == vec!["AGENTS.md"]
                        && report.prompt_cache_prefix_reused
                )
        )
    }));
}

#[tokio::test]
async fn subagent_bootstrap_inherit_all_reads_standard_workspace_bootstrap_files() {
    let workspace = tempfile::tempdir().unwrap();
    fs::write(workspace.path().join("AGENTS.md"), "agent rules").unwrap();
    fs::write(workspace.path().join("IDENTITY.md"), "identity rules").unwrap();
    fs::write(workspace.path().join("notes.md"), "not bootstrap").unwrap();
    let mut spec = SubagentSpec::minimal("worker", "inherit all bootstrap");
    spec.bootstrap_filter = BootstrapFilter::InheritAll;
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-bootstrap-inherit-all",
        store.clone(),
        model.clone(),
        ToolPool::default(),
        workspace.path(),
        Some("parent-system"),
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let requests = model.requests();
    let child_system = requests[1].system.as_deref().unwrap();
    assert!(child_system.contains(r#"<workspace-instructions source="AGENTS.md">"#));
    assert!(child_system.contains(r#"<workspace-instructions source="IDENTITY.md">"#));
    assert!(!child_system.contains("notes.md"));
    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentAnnounced(announced)
                if announced.context_report.as_ref().is_some_and(|report|
                    report.bootstrap_files_inherited
                        == vec!["AGENTS.md".to_owned(), "IDENTITY.md".to_owned()]
                )
        )
    }));
}

#[tokio::test]
async fn invalid_subagent_bootstrap_filter_filename_fails_closed() {
    let workspace = tempfile::tempdir().unwrap();
    let mut spec = SubagentSpec::minimal("worker", "invalid bootstrap");
    spec.bootstrap_filter = BootstrapFilter::Allow(vec!["../AGENTS.md".to_owned()]);
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-bootstrap-invalid",
        store.clone(),
        model,
        ToolPool::default(),
        workspace.path(),
        None,
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentTerminated(terminated)
                if matches!(
                    &terminated.reason,
                    SubagentTerminationReason::Failed { detail }
                        if detail.contains("invalid bootstrap_filter filename")
                )
        )
    }));
}

#[tokio::test]
async fn subagent_announced_records_context_report_for_prompt_prefix_and_bootstrap() {
    let workspace = tempfile::tempdir().unwrap();
    fs::write(workspace.path().join("AGENTS.md"), "agent rules").unwrap();
    let mut spec = SubagentSpec::minimal("worker", "context report");
    spec.bootstrap_filter = BootstrapFilter::Allow(vec!["AGENTS.md".to_owned()]);
    spec.system_header_extra = Some("child-only-system".to_owned());
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-context-report",
        store.clone(),
        model,
        ToolPool::default(),
        workspace.path(),
        Some("parent-system"),
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    let report = journal_events
        .iter()
        .find_map(|event| match event {
            Event::SubagentAnnounced(announced) => announced.context_report.as_ref(),
            _ => None,
        })
        .expect("SubagentAnnounced should carry context report");
    assert!(report.parent_system_hash.is_some());
    assert_ne!(
        report.parent_system_hash,
        Some(report.child_system_hash.clone())
    );
    assert!(report.shared_system_prefix_hash.is_some());
    assert!(report.prompt_cache_prefix_reused);
    assert_eq!(report.bootstrap_files_inherited, vec!["AGENTS.md"]);
    assert!(report.system_header_extra_applied);
}

#[tokio::test]
async fn unsupported_custom_input_strategy_fails_closed() {
    let workspace = tempfile::tempdir().unwrap();
    let mut spec = SubagentSpec::minimal("worker", "custom context");
    spec.context_mode = SubagentContextMode::ForkFromParent {
        include_tool_results: false,
    };
    spec.input_strategy = SubagentInputStrategy::Custom {
        selector_id: "missing-selector".to_owned(),
    };
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-input-strategy-fail-closed",
        store.clone(),
        model,
        ToolPool::default(),
        workspace.path(),
        None,
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentTerminated(terminated)
                if matches!(
                    &terminated.reason,
                    harness_contracts::SubagentTerminationReason::Failed { detail }
                        if detail.contains("missing-selector")
                )
        )
    }));
}

#[tokio::test]
async fn unsupported_memory_subset_scope_fails_closed() {
    let workspace = tempfile::tempdir().unwrap();
    let mut spec = SubagentSpec::minimal("worker", "subset memory");
    spec.memory_scope = SubagentMemoryScope::Subset {
        selectors: vec![MemorySelector::Tag("safe".to_owned())],
    };
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-memory-subset-fail-closed",
        store.clone(),
        model,
        ToolPool::default(),
        workspace.path(),
        None,
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentTerminated(terminated)
                if matches!(
                    &terminated.reason,
                    SubagentTerminationReason::Failed { detail }
                        if detail.contains("memory_scope")
                )
        )
    }));
}

#[tokio::test]
async fn full_transcript_announcement_writes_child_transcript_blob() {
    let workspace = tempfile::tempdir().unwrap();
    let mut spec = SubagentSpec::minimal("worker", "capture transcript");
    spec.announce_mode = AnnounceMode::FullTranscript;
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let blob_store = Arc::new(harness_journal::InMemoryBlobStore::default());
    let engine = test_engine_with_blob_store(
        "subagent-full-transcript-blob",
        store.clone(),
        model,
        ToolPool::default(),
        workspace.path(),
        blob_store.clone(),
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    let transcript_ref = journal_events
        .iter()
        .find_map(|event| match event {
            Event::SubagentAnnounced(announced) => announced.transcript_ref.clone(),
            _ => None,
        })
        .expect("full transcript mode should attach a transcript ref");
    assert_eq!(
        transcript_ref.blob.content_type.as_deref(),
        Some("application/json")
    );
    assert!(transcript_ref.from_offset <= transcript_ref.to_offset);

    let chunks = blob_store
        .get(TenantId::SINGLE, &transcript_ref.blob)
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    let mut body = Vec::new();
    for chunk in chunks {
        body.extend_from_slice(&chunk);
    }
    assert_eq!(
        transcript_ref.blob.content_hash,
        *blake3::hash(&body).as_bytes()
    );
    let envelopes: Vec<harness_journal::EventEnvelope> = serde_json::from_slice(&body).unwrap();
    assert!(envelopes.iter().any(|envelope| {
        matches!(
            &envelope.payload,
            Event::AssistantMessageCompleted(message)
                if message.content == MessageContent::Text("child answer".to_owned())
        )
    }));
}

#[tokio::test]
async fn full_transcript_without_blob_store_fails_closed() {
    let workspace = tempfile::tempdir().unwrap();
    let mut spec = SubagentSpec::minimal("worker", "capture transcript");
    spec.announce_mode = AnnounceMode::FullTranscript;
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        "subagent-full-transcript-requires-blob-store",
        store.clone(),
        model,
        ToolPool::default(),
        workspace.path(),
        None,
    );
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentTerminated(terminated)
                if matches!(
                    &terminated.reason,
                    SubagentTerminationReason::Failed { detail }
                        if detail.contains("full transcript requires blob store")
                )
        )
    }));
}

#[tokio::test]
async fn required_sandbox_capabilities_fail_closed_when_parent_backend_is_missing_feature() {
    let workspace = tempfile::tempdir().unwrap();
    let mut spec = SubagentSpec::minimal("worker", "need streaming");
    spec.sandbox_policy = SandboxInheritance::Require(RequiredSandboxCapabilities {
        supports_streaming: true,
        ..RequiredSandboxCapabilities::default()
    });
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = Engine::builder()
        .with_engine_id(EngineId::new("subagent-sandbox-capability-fail-closed"))
        .with_event_store(store.clone())
        .with_context(harness_context::ContextEngine::builder().build().unwrap())
        .with_hooks(harness_hook::HookDispatcher::new(
            harness_hook::HookRegistry::builder()
                .build()
                .unwrap()
                .snapshot(),
        ))
        .with_model(model)
        .with_tools(ToolPool::default())
        .with_permission_broker(Arc::new(AllowBroker))
        .with_workspace_root(workspace.path())
        .with_model_id("test-model")
        .with_sandbox(Arc::new(MissingFeatureSandbox))
        .with_subagent_tool()
        .build()
        .unwrap();
    let parent_session_id = SessionId::new();

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    let journal_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(journal_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentTerminated(terminated)
                if matches!(
                    &terminated.reason,
                    SubagentTerminationReason::Failed { detail }
                        if detail.contains("sandbox capability mismatch")
                            && detail.contains("supports_streaming")
                )
        )
    }));
}

#[derive(Clone)]
struct ReadyRunner;

#[async_trait]
impl SubagentRunner for ReadyRunner {
    async fn spawn(
        &self,
        spec: SubagentSpec,
        _input: harness_contracts::TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, harness_subagent::SubagentError> {
        Ok(SubagentHandle::ready(SubagentAnnouncement {
            subagent_id: harness_contracts::SubagentId::new(),
            parent_session_id: parent_ctx.parent_session_id,
            status: SubagentStatus::Completed,
            summary: spec.task,
            result: None,
            usage: UsageSnapshot::default(),
            transcript_ref: None,
            context_report: None,
        }))
    }
}

struct EmptyModel;

#[async_trait]
impl ModelProvider for EmptyModel {
    fn provider_id(&self) -> &'static str {
        "empty"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "empty".to_owned(),
            model_id: "empty-model".to_owned(),
            display_name: "Empty Model".to_owned(),
            context_window: 8_000,
            max_output_tokens: 1_024,
            conversation_capability: ConversationModelCapability::default(),
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        _req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        Ok(Box::pin(futures::stream::iter([
            ModelStreamEvent::MessageStop,
        ])))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

#[derive(Default)]
struct AllowBroker;

#[async_trait]
impl PermissionBroker for AllowBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::AllowOnce
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), PermissionError> {
        Ok(())
    }
}

fn test_engine(
    engine_id: &str,
    store: Arc<harness_journal::InMemoryEventStore>,
    model: Arc<DelegatingModel>,
    tools: ToolPool,
    workspace_root: &std::path::Path,
    system_prompt: Option<&str>,
) -> Engine {
    test_engine_with_sandbox(
        engine_id,
        store,
        model,
        tools,
        workspace_root,
        system_prompt,
        None,
    )
}

fn test_engine_with_blob_store(
    engine_id: &str,
    store: Arc<harness_journal::InMemoryEventStore>,
    model: Arc<DelegatingModel>,
    tools: ToolPool,
    workspace_root: &std::path::Path,
    blob_store: Arc<dyn BlobStore>,
) -> Engine {
    Engine::builder()
        .with_engine_id(EngineId::new(engine_id))
        .with_event_store(store)
        .with_context(harness_context::ContextEngine::builder().build().unwrap())
        .with_hooks(harness_hook::HookDispatcher::new(
            harness_hook::HookRegistry::builder()
                .build()
                .unwrap()
                .snapshot(),
        ))
        .with_model(model)
        .with_tools(tools)
        .with_permission_broker(Arc::new(AllowBroker))
        .with_workspace_root(workspace_root)
        .with_model_id("test-model")
        .with_blob_store(blob_store)
        .with_subagent_tool()
        .build()
        .unwrap()
}

fn test_engine_with_sandbox(
    engine_id: &str,
    store: Arc<harness_journal::InMemoryEventStore>,
    model: Arc<DelegatingModel>,
    tools: ToolPool,
    workspace_root: &std::path::Path,
    system_prompt: Option<&str>,
    sandbox: Option<Arc<dyn SandboxBackend>>,
) -> Engine {
    let mut builder = Engine::builder()
        .with_engine_id(EngineId::new(engine_id))
        .with_event_store(store)
        .with_context(harness_context::ContextEngine::builder().build().unwrap())
        .with_hooks(harness_hook::HookDispatcher::new(
            harness_hook::HookRegistry::builder()
                .build()
                .unwrap()
                .snapshot(),
        ))
        .with_model(model)
        .with_tools(tools)
        .with_permission_broker(Arc::new(AllowBroker))
        .with_workspace_root(workspace_root)
        .with_model_id("test-model")
        .with_subagent_tool();
    if let Some(system_prompt) = system_prompt {
        builder = builder.with_system_prompt(Some(system_prompt));
    }
    if let Some(sandbox) = sandbox {
        builder = builder.with_sandbox(sandbox);
    }
    builder.build().unwrap()
}

struct DelegatingModel {
    child_spec: serde_json::Value,
    child_events: Vec<ModelStreamEvent>,
    calls: AtomicUsize,
    requests: Mutex<Vec<ModelRequest>>,
}

impl DelegatingModel {
    fn new(child_spec: serde_json::Value) -> Self {
        Self::with_child_events(child_spec, text_events("child answer", usage(3, 7, 11)))
    }

    fn with_child_events(
        child_spec: serde_json::Value,
        child_events: Vec<ModelStreamEvent>,
    ) -> Self {
        Self {
            child_spec,
            child_events,
            calls: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl ModelProvider for DelegatingModel {
    fn provider_id(&self) -> &'static str {
        "delegating"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "delegating".to_owned(),
            model_id: "test-model".to_owned(),
            display_name: "Delegating Model".to_owned(),
            context_window: 8_000,
            max_output_tokens: 1_024,
            conversation_capability: ConversationModelCapability::default(),
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        req: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.requests.lock().unwrap().push(req);
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let events = match call {
            0 => tool_events("agent", self.child_spec.clone()),
            1 => self.child_events.clone(),
            _ => text_events("parent final", usage(5, 13, 17)),
        };
        Ok(Box::pin(futures::stream::iter(events)))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

struct TestTool {
    descriptor: ToolDescriptor,
}

struct MissingFeatureSandbox;

#[async_trait]
impl harness_sandbox::SandboxBackend for MissingFeatureSandbox {
    fn backend_id(&self) -> &'static str {
        "missing-feature"
    }

    fn capabilities(&self) -> harness_sandbox::SandboxCapabilities {
        harness_sandbox::SandboxCapabilities::default()
    }

    async fn execute(
        &self,
        _spec: harness_sandbox::ExecSpec,
        _ctx: harness_sandbox::ExecContext,
    ) -> Result<harness_sandbox::ProcessHandle, harness_contracts::SandboxError> {
        Err(harness_contracts::SandboxError::CapabilityMismatch {
            capability: "execute".to_owned(),
            detail: "test sandbox does not execute".to_owned(),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &harness_sandbox::SnapshotSpec,
    ) -> Result<harness_sandbox::SessionSnapshotFile, harness_contracts::SandboxError> {
        Err(harness_contracts::SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn restore_session(
        &self,
        _snapshot: &harness_sandbox::SessionSnapshotFile,
    ) -> Result<(), harness_contracts::SandboxError> {
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), harness_contracts::SandboxError> {
        Ok(())
    }
}

impl TestTool {
    fn new(name: &str, origin: ToolOrigin) -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: name.to_owned(),
                display_name: name.to_owned(),
                description: "test tool".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::Custom("test".to_owned()),
                version: "0.0.0".to_owned(),
                input_schema: serde_json::json!({ "type": "object" }),
                output_schema: None,
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: true,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                trust_level: TrustLevel::UserControlled,
                required_capabilities: Vec::new(),
                budget: ResultBudget {
                    metric: harness_contracts::BudgetMetric::Chars,
                    limit: 1024,
                    on_overflow: harness_contracts::OverflowAction::Truncate,
                    preview_head_chars: 128,
                    preview_tail_chars: 128,
                },
                provider_restriction: ProviderRestriction::All,
                origin,
                search_hint: None,
                service_binding: None,
            },
        }
    }
}

#[async_trait]
impl Tool for TestTool {
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
            self.descriptor(),
            input,
            ctx,
            harness_permission::PermissionCheck::Allowed,
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
        Ok(Box::pin(futures::stream::iter([ToolEvent::Final(
            ToolResult::Text("ok".to_owned()),
        )])))
    }
}

struct SandboxProbeTool {
    descriptor: ToolDescriptor,
    seen_sandboxes: Mutex<Vec<Option<String>>>,
}

impl SandboxProbeTool {
    fn new(name: &str) -> Self {
        Self {
            descriptor: TestTool::new(name, ToolOrigin::Builtin).descriptor,
            seen_sandboxes: Mutex::new(Vec::new()),
        }
    }

    fn seen_sandboxes(&self) -> Vec<Option<String>> {
        self.seen_sandboxes.lock().unwrap().clone()
    }
}

#[async_trait]
impl Tool for SandboxProbeTool {
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
            self.descriptor(),
            input,
            ctx,
            harness_permission::PermissionCheck::Allowed,
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        if let Some(sandbox) = &ctx.sandbox {
            self.seen_sandboxes
                .lock()
                .unwrap()
                .push(Some(sandbox.backend_id().to_owned()));
            let _ = sandbox
                .execute(
                    ExecSpec::default(),
                    ExecContext::for_test(Arc::new(NoopSandboxEventSink)),
                )
                .await
                .map_err(ToolError::Sandbox)?;
        } else {
            self.seen_sandboxes.lock().unwrap().push(None);
        }
        Ok(Box::pin(futures::stream::iter([ToolEvent::Final(
            ToolResult::Text("ok".to_owned()),
        )])))
    }
}

struct NoopSandboxEventSink;

impl harness_sandbox::EventSink for NoopSandboxEventSink {
    fn emit(&self, _event: Event) -> Result<(), harness_contracts::SandboxError> {
        Ok(())
    }
}

struct FakeSandbox {
    id: String,
    capabilities: SandboxCapabilities,
    seen_policies: Mutex<Vec<SandboxPolicy>>,
}

impl FakeSandbox {
    fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            capabilities: SandboxCapabilities::default(),
            seen_policies: Mutex::new(Vec::new()),
        }
    }

    fn with_capabilities(mut self, capabilities: SandboxCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    fn seen_policies(&self) -> Vec<SandboxPolicy> {
        self.seen_policies.lock().unwrap().clone()
    }
}

#[async_trait]
impl SandboxBackend for FakeSandbox {
    fn backend_id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> SandboxCapabilities {
        self.capabilities.clone()
    }

    async fn execute(
        &self,
        spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, harness_contracts::SandboxError> {
        self.seen_policies.lock().unwrap().push(spec.policy);
        Ok(ProcessHandle {
            pid: None,
            stdout: None,
            stderr: None,
            stdin: None,
            cwd_marker: None,
            activity: Arc::new(FakeActivity),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, harness_contracts::SandboxError> {
        Err(harness_contracts::SandboxError::SnapshotUnsupported {
            kind: "fake".to_owned(),
        })
    }

    async fn restore_session(
        &self,
        _snapshot: &SessionSnapshotFile,
    ) -> Result<(), harness_contracts::SandboxError> {
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), harness_contracts::SandboxError> {
        Ok(())
    }
}

struct FakeActivity;

#[async_trait]
impl harness_sandbox::ActivityHandle for FakeActivity {
    async fn wait(&self) -> Result<harness_sandbox::ExecOutcome, harness_contracts::SandboxError> {
        Ok(harness_sandbox::ExecOutcome::default())
    }

    async fn kill(
        &self,
        _signal: i32,
        _scope: harness_contracts::KillScope,
    ) -> Result<(), harness_contracts::SandboxError> {
        Ok(())
    }

    fn touch(&self) {}

    fn last_activity(&self) -> std::time::Instant {
        std::time::Instant::now()
    }
}

fn sandbox_policy(network: NetworkAccess) -> SandboxPolicy {
    SandboxPolicy {
        mode: SandboxMode::Container,
        scope: SandboxScope::WorkspaceOnly,
        network,
        resource_limits: ResourceLimits {
            max_memory_bytes: None,
            max_cpu_cores: None,
            max_pids: None,
            max_wall_clock_ms: Some(1_000),
            max_open_files: None,
        },
        denied_host_paths: Vec::new(),
    }
}

fn text_events(body: &str, usage: UsageSnapshot) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text(body.to_owned()),
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::EndTurn),
            usage_delta: usage,
        },
        ModelStreamEvent::MessageStop,
    ]
}

fn tool_events(tool_name: &str, input: serde_json::Value) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: tool_name.to_owned(),
                input,
            },
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::ToolUse),
            usage_delta: UsageSnapshot::default(),
        },
        ModelStreamEvent::MessageStop,
    ]
}

fn usage(input_tokens: u64, output_tokens: u64, cost_micros: u64) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens,
        output_tokens,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        cost_micros,
        tool_calls: 0,
    }
}

fn turn_input(text: &str) -> TurnInput {
    TurnInput {
        message: Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(text.to_owned())],
            created_at: harness_contracts::now(),
        },
        metadata: serde_json::Value::Null,
    }
}

async fn run_delegating_with_prior_parent_events(
    engine_id: &str,
    spec: SubagentSpec,
    prior_events: Vec<Event>,
    system_prompt: Option<&str>,
) -> Vec<ModelRequest> {
    let workspace = tempfile::tempdir().unwrap();
    let model = Arc::new(DelegatingModel::new(serde_json::to_value(spec).unwrap()));
    let store = Arc::new(harness_journal::InMemoryEventStore::new(Arc::new(
        NoopRedactor,
    )));
    let engine = test_engine(
        engine_id,
        store.clone(),
        model.clone(),
        ToolPool::default(),
        workspace.path(),
        system_prompt,
    );
    let parent_session_id = SessionId::new();
    if !prior_events.is_empty() {
        store
            .append(TenantId::SINGLE, parent_session_id, &prior_events)
            .await
            .unwrap();
    }

    let _events: Vec<_> = engine
        .run(
            SessionHandle {
                tenant_id: TenantId::SINGLE,
                session_id: parent_session_id,
            },
            turn_input("delegate"),
            RunContext::new(TenantId::SINGLE, parent_session_id, RunId::new()),
        )
        .await
        .unwrap()
        .collect()
        .await;

    model.requests()
}

fn parent_transcript_events(
    first_user: &str,
    assistant_text: &str,
    latest_user: &str,
) -> Vec<Event> {
    let run_id = RunId::new();
    let tool_use_id = ToolUseId::new();
    vec![
        Event::RunStarted(RunStartedEvent {
            run_id,
            session_id: SessionId::new(),
            tenant_id: TenantId::SINGLE,
            parent_run_id: None,
            model: test_run_model_snapshot(),
            input: turn_input(first_user),
            snapshot_id: SnapshotId::from_u128(0),
            effective_config_hash: ConfigHash([0; 32]),
            started_at: harness_contracts::now(),
            correlation_id: harness_contracts::CorrelationId::new(),
            permission_mode: PermissionMode::Default,
        }),
        Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
            run_id,
            message_id: MessageId::new(),
            content: MessageContent::Text(assistant_text.to_owned()),
            tool_uses: Vec::new(),
            usage: UsageSnapshot::default(),
            pricing_snapshot_id: None,
            stop_reason: StopReason::EndTurn,
            at: harness_contracts::now(),
        }),
        Event::ToolUseCompleted(ToolUseCompletedEvent {
            tool_use_id,
            result: ToolResult::Text("prior tool output".to_owned()),
            usage: None,
            duration_ms: 1,
            at: harness_contracts::now(),
        }),
        Event::UserMessageAppended(UserMessageAppendedEvent {
            run_id,
            message_id: MessageId::new(),
            content: MessageContent::Text(latest_user.to_owned()),
            metadata: Default::default(),
            attachments: Vec::new(),
            at: harness_contracts::now(),
        }),
        Event::RunEnded(harness_contracts::RunEndedEvent {
            run_id,
            reason: EndReason::Completed,
            usage: Some(UsageSnapshot::default()),
            ended_at: harness_contracts::now(),
        }),
    ]
}

fn test_run_model_snapshot() -> RunModelSnapshot {
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

fn message_texts(messages: &[Message]) -> Vec<&str> {
    messages
        .iter()
        .filter_map(|message| match message.parts.as_slice() {
            [MessagePart::Text(text)] => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn mcp_origin(server_id: &str) -> ToolOrigin {
    mcp_origin_with(
        server_id,
        McpServerSource::Workspace,
        TrustLevel::AdminTrusted,
    )
}

fn mcp_origin_with(
    server_id: &str,
    server_source: McpServerSource,
    server_trust: TrustLevel,
) -> ToolOrigin {
    ToolOrigin::Mcp(McpOrigin {
        server_id: McpServerId(server_id.to_owned()),
        upstream_name: "upstream".to_owned(),
        server_meta: Default::default(),
        server_source,
        server_trust,
    })
}

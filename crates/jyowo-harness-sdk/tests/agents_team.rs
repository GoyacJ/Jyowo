#![cfg(all(feature = "agents-team", feature = "testing"))]

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use async_trait::async_trait;
use futures::{executor::block_on, stream};
use harness_contracts::{
    AgentId, BudgetKind, CorrelationId, Decision, DecisionScope, DeferPolicy, EndReason, Event,
    EventId, ForkReason, InteractivityLevel, JournalError, JournalOffset, Message, MessageId,
    MessagePart, MessageRole, ModelError, ModelRef, PermissionActorSource, PermissionError,
    PermissionMode, PermissionSubject, ProviderRestriction, Recipient, RunId, SessionId,
    StopReason, TeamId, TenantId, ToolDescriptor, ToolError, ToolGroup, ToolOrigin, ToolProperties,
    ToolResult, ToolUseId, TrustLevel, TurnInput, UsageSnapshot,
};
use harness_engine::{Engine, EngineId};
use harness_hook::{HookDispatcher, HookRegistry};
use harness_journal::{
    AppendMetadata, EventEnvelope, EventStore, InMemoryBlobStore, PrunePolicy, PruneReport,
    ReplayCursor, SchemaVersion, SessionFilter, SessionSnapshot, SessionSummary,
};
use harness_model::{
    ContentDelta, ConversationModelCapability, HealthStatus, InferContext, ModelDescriptor,
    ModelProvider, ModelRequest, ModelStream, ModelStreamEvent,
};
use harness_permission::{PermissionBroker, PermissionCheck, PermissionContext, PermissionRequest};
use harness_team::{
    ContextVisibility, MessageBus, ResourceQuota, RoleRoutedRuntime, Team, TeamBuilder,
    TeamControlHandle, TeamJournalContext, TeamMemberEngineConfig, TeamMemberRunRequest,
    TeamMemberRunner, TeamSandboxPolicy, TeamToolsetSelector, Topology,
};
use harness_tool::{Tool, ToolContext, ToolEvent, ToolPool, ToolStream, ValidationError};
use jyowo_harness_sdk::EngineTeamMemberRunner;

#[test]
fn engine_team_member_runner_uses_member_session_parent_and_correlation() {
    block_on(async {
        let harness = test_harness(Script::Text {
            body: "member answer".to_owned(),
            usage: usage(4, 7, 13),
        });
        let runner = EngineTeamMemberRunner::new(harness.engine.clone());
        let tenant_id = TenantId::SINGLE;
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let parent_run_id = RunId::new();
        let correlation_id = CorrelationId::new();

        let outcome = runner
            .run_member(request_with(
                tenant_id,
                session_id,
                run_id,
                Some(parent_run_id),
                correlation_id,
                TeamMemberEngineConfig::default(),
            ))
            .await
            .expect("member engine run should succeed");

        assert_eq!(outcome.body, "member answer");
        assert_eq!(outcome.usage.output_tokens, 7);

        let events = harness.store.events().await;
        let started = events
            .iter()
            .find_map(|envelope| match &envelope.payload {
                Event::RunStarted(event) => Some(event),
                _ => None,
            })
            .expect("RunStarted should be persisted");
        assert_eq!(started.tenant_id, tenant_id);
        assert_eq!(started.session_id, session_id);
        assert_eq!(started.run_id, run_id);
        assert_eq!(started.parent_run_id, Some(parent_run_id));
        assert_eq!(started.correlation_id, correlation_id);
        assert!(events.iter().any(|envelope| {
            matches!(
                &envelope.payload,
                Event::RunEnded(event)
                    if event.run_id == run_id
                        && matches!(event.reason, EndReason::Completed)
                        && event.usage.as_ref().is_some_and(|usage| usage.output_tokens == 7)
            )
        }));
    });
}

#[test]
fn engine_team_member_runner_marks_permission_requests_with_team_member_source() {
    block_on(async {
        let mut harness = test_harness(Script::ToolThenText {
            tool_name: "ask_user_tool".to_owned(),
            body: "member answer".to_owned(),
            usage: usage(4, 7, 13),
        });
        harness
            .tools
            .append_runtime_tool(Arc::new(TestTool::new("ask_user_tool")));
        harness.rebuild();
        let runner = EngineTeamMemberRunner::new(harness.engine.clone());
        let tenant_id = TenantId::SINGLE;
        let team_id = TeamId::new();
        let agent_id = AgentId::new();
        let role = "researcher sk-abcdefghijklmnopqrstuvwxyz".to_owned();
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let parent_run_id = RunId::new();
        let correlation_id = CorrelationId::new();
        let request = TeamMemberRunRequest::synthetic(
            tenant_id,
            team_id,
            agent_id,
            role.clone(),
            session_id,
            run_id,
            Some(parent_run_id),
            turn_input("dispatch goal"),
            "dispatch goal",
            correlation_id,
            TeamMemberEngineConfig::default(),
        );

        let outcome = runner
            .run_member(request)
            .await
            .expect("member engine run should succeed");

        assert_eq!(outcome.body, "member answer");
        let events = harness.store.events().await;
        let permission_requested = events
            .iter()
            .find_map(|envelope| match &envelope.payload {
                Event::PermissionRequested(event) => Some(event),
                _ => None,
            })
            .expect("PermissionRequested should be persisted");
        assert_eq!(
            permission_requested.actor_source,
            PermissionActorSource::TeamMember {
                team_id,
                agent_id,
                role: "researcher [REDACTED]".to_owned(),
                parent_run_id: Some(parent_run_id),
            }
        );
    });
}

#[tokio::test]
async fn engine_team_member_runner_preserves_correlation_for_spawned_subagent() {
    let harness = test_harness_with_subagent_tool(Script::ToolThenTextWithInput {
        tool_name: "agent".to_owned(),
        input: serde_json::json!({
            "role": "worker",
            "task": "child task"
        }),
        body: "parent final".to_owned(),
        usage: usage(5, 8, 13),
    });
    let runner = EngineTeamMemberRunner::new(harness.engine.clone());
    let tenant_id = TenantId::SINGLE;
    let session_id = SessionId::new();
    let member_run_id = RunId::new();
    let parent_run_id = RunId::new();
    let correlation_id = CorrelationId::new();

    let outcome = runner
        .run_member(request_with(
            tenant_id,
            session_id,
            member_run_id,
            Some(parent_run_id),
            correlation_id,
            TeamMemberEngineConfig::default(),
        ))
        .await
        .expect("member engine run should succeed");

    assert_eq!(outcome.body, "parent final");

    let envelopes = harness.store.events().await;
    let child_started = envelopes
        .iter()
        .find_map(|envelope| match &envelope.payload {
            Event::RunStarted(started) if started.parent_run_id == Some(member_run_id) => {
                Some(started)
            }
            _ => None,
        })
        .expect("member-spawned subagent should start a child run");
    assert_eq!(child_started.correlation_id, correlation_id);

    let subagent_event_correlations: Vec<_> = envelopes
        .iter()
        .filter_map(|envelope| match &envelope.payload {
            Event::SubagentSpawned(_)
            | Event::SubagentAnnounced(_)
            | Event::SubagentTerminated(_) => Some(envelope.correlation_id),
            _ => None,
        })
        .collect();
    assert_eq!(subagent_event_correlations.len(), 3);
    assert!(subagent_event_correlations
        .iter()
        .all(|event_correlation| *event_correlation == correlation_id));
}

#[test]
fn engine_team_member_runner_passes_permission_and_interactivity_to_run_context() {
    block_on(async {
        let mut harness = test_harness(Script::ToolThenText {
            tool_name: "ask_tool".to_owned(),
            body: "after tool".to_owned(),
            usage: usage(2, 3, 5),
        });
        harness
            .tools
            .append_runtime_tool(Arc::new(TestTool::new("ask_tool")));
        harness.rebuild();

        let runner = EngineTeamMemberRunner::new(harness.engine.clone());
        let config = TeamMemberEngineConfig {
            permission_mode: PermissionMode::BypassPermissions,
            interactivity: InteractivityLevel::FullyInteractive,
            ..TeamMemberEngineConfig::default()
        };

        let outcome = runner
            .run_member(request_with(
                TenantId::SINGLE,
                SessionId::new(),
                RunId::new(),
                None,
                CorrelationId::new(),
                config,
            ))
            .await
            .expect("member engine run should succeed");

        assert_eq!(outcome.body, "after tool");
        let contexts = harness.broker.contexts();
        assert!(contexts.iter().any(|ctx| {
            ctx.permission_mode == PermissionMode::BypassPermissions
                && ctx.interactivity == InteractivityLevel::FullyInteractive
        }));
    });
}

#[test]
fn engine_team_member_runner_maps_member_quota_to_run_budget_limits() {
    block_on(async {
        let harness = test_harness(Script::Text {
            body: "over budget".to_owned(),
            usage: usage(2, 3, 5),
        });
        let runner = EngineTeamMemberRunner::new(harness.engine.clone());
        let run_id = RunId::new();
        let config = TeamMemberEngineConfig {
            quota: Some(ResourceQuota {
                max_tokens: Some(1),
                ..ResourceQuota::default()
            }),
            ..TeamMemberEngineConfig::default()
        };

        runner
            .run_member(request_with(
                TenantId::SINGLE,
                SessionId::new(),
                run_id,
                None,
                CorrelationId::new(),
                config,
            ))
            .await
            .expect("member engine run should still stream the terminal event");

        let events = harness.store.events().await;
        assert!(events.iter().any(|envelope| {
            matches!(
                &envelope.payload,
                Event::RunEnded(event)
                    if event.run_id == run_id
                        && event.reason == EndReason::BudgetExhausted(BudgetKind::Tokens)
            )
        }));
    });
}

#[test]
fn engine_team_member_runner_filters_custom_toolset() {
    block_on(async {
        let mut harness = test_harness(Script::Text {
            body: "filtered".to_owned(),
            usage: usage(1, 1, 1),
        });
        harness
            .tools
            .append_runtime_tool(Arc::new(TestTool::new("allowed_tool")));
        harness
            .tools
            .append_runtime_tool(Arc::new(TestTool::new("blocked_tool")));
        harness.rebuild();

        let runner = EngineTeamMemberRunner::new(harness.engine.clone());
        let config = TeamMemberEngineConfig {
            toolset: TeamToolsetSelector::Custom(vec!["allowed_tool".to_owned()]),
            ..TeamMemberEngineConfig::default()
        };

        runner
            .run_member(request_with(
                TenantId::SINGLE,
                SessionId::new(),
                RunId::new(),
                None,
                CorrelationId::new(),
                config,
            ))
            .await
            .expect("member engine run should succeed");

        let requests = harness.model.requests();
        let tool_names: Vec<_> = requests[0]
            .tools
            .as_ref()
            .expect("tool snapshot should be present")
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        assert_eq!(tool_names, vec!["allowed_tool"]);
    });
}

#[test]
fn engine_team_member_runner_fails_closed_for_preset_model_and_sandbox_mismatch() {
    block_on(async {
        let harness = test_harness(Script::Text {
            body: "unused".to_owned(),
            usage: usage(0, 0, 0),
        });
        let runner = EngineTeamMemberRunner::new(harness.engine.clone());

        let preset = TeamMemberEngineConfig {
            toolset: TeamToolsetSelector::Preset("coding".to_owned()),
            ..TeamMemberEngineConfig::default()
        };
        assert!(runner
            .run_member(request_with(
                TenantId::SINGLE,
                SessionId::new(),
                RunId::new(),
                None,
                CorrelationId::new(),
                preset,
            ))
            .await
            .is_err());

        let model_mismatch = TeamMemberEngineConfig {
            model_ref: Some(ModelRef {
                provider_id: "sdk-test".to_owned(),
                model_id: "other-model".to_owned(),
            }),
            ..TeamMemberEngineConfig::default()
        };
        assert!(runner
            .run_member(request_with(
                TenantId::SINGLE,
                SessionId::new(),
                RunId::new(),
                None,
                CorrelationId::new(),
                model_mismatch,
            ))
            .await
            .is_err());

        let sandbox_mismatch = TeamMemberEngineConfig {
            sandbox_policy: TeamSandboxPolicy::RequireBackend("missing-sandbox".to_owned()),
            ..TeamMemberEngineConfig::default()
        };
        assert!(runner
            .run_member(request_with(
                TenantId::SINGLE,
                SessionId::new(),
                RunId::new(),
                None,
                CorrelationId::new(),
                sandbox_mismatch,
            ))
            .await
            .is_err());

        assert!(harness.model.requests().is_empty());
    });
}

#[test]
fn coordinator_member_runner_exposes_only_team_control_tools() {
    block_on(async {
        let mut harness = test_harness(Script::Text {
            body: "coordinator ready".to_owned(),
            usage: usage(1, 1, 1),
        });
        harness
            .tools
            .append_runtime_tool(Arc::new(TestTool::new("ask_tool")));
        harness.rebuild();
        let runner = EngineTeamMemberRunner::new(harness.engine.clone());

        runner
            .run_member(request_with_control(
                TenantId::SINGLE,
                SessionId::new(),
                RunId::new(),
                CorrelationId::new(),
                TeamMemberEngineConfig::default(),
                true,
                test_control_handle(),
            ))
            .await
            .expect("coordinator engine run should succeed");

        let requests = harness.model.requests();
        let tool_names: Vec<_> = requests[0]
            .tools
            .as_ref()
            .expect("coordinator tool snapshot should be present")
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        assert_eq!(
            tool_names,
            vec![
                "dispatch",
                "message",
                "pause_worker",
                "resume_worker",
                "spawn_worker",
                "stop_team",
                "team_status",
            ]
        );
    });
}

#[test]
fn coordinator_team_status_tool_executes_through_engine_loop() {
    block_on(async {
        let harness = test_harness(Script::ToolThenTextWithInput {
            tool_name: "team_status".to_owned(),
            input: serde_json::json!({}),
            body: "status inspected".to_owned(),
            usage: usage(2, 3, 5),
        });
        let runner = EngineTeamMemberRunner::new(harness.engine.clone());

        let outcome = runner
            .run_member(request_with_control(
                TenantId::SINGLE,
                SessionId::new(),
                RunId::new(),
                CorrelationId::new(),
                TeamMemberEngineConfig::default(),
                true,
                test_control_handle(),
            ))
            .await
            .expect("team_status should execute as coordinator control tool");

        assert_eq!(outcome.body, "status inspected");
        assert_eq!(harness.model.requests().len(), 2);
    });
}

#[test]
fn coordinator_spawn_worker_tool_binds_engine_runner_for_new_member() {
    block_on(async {
        let spawned = AgentId::new();
        let harness = test_harness(Script::ToolThenTextWithInput {
            tool_name: "spawn_worker".to_owned(),
            input: serde_json::json!({
                "agent_id": spawned.to_string(),
                "role": "reviewer"
            }),
            body: "worker spawned".to_owned(),
            usage: usage(2, 3, 5),
        });
        let store: Arc<RecordingEventStore> = Arc::new(RecordingEventStore::default());
        let journal = TeamJournalContext {
            tenant_id: TenantId::SINGLE,
            session_id: SessionId::new(),
        };
        let coordinator = AgentId::new();
        let team = TeamBuilder::new("sdk-dynamic-team", Topology::RoleRouted)
            .member(coordinator, "coordinator", ContextVisibility::All)
            .build();
        let runtime = RoleRoutedRuntime::new(
            team.clone(),
            MessageBus::journaled(team.team_id, 16, journal, store.clone()),
            journal,
            store,
            Arc::new(InMemoryBlobStore::default()),
        );
        let runner = EngineTeamMemberRunner::new(harness.engine.clone());

        runner
            .run_member(request_with_control(
                TenantId::SINGLE,
                SessionId::new(),
                RunId::new(),
                CorrelationId::new(),
                TeamMemberEngineConfig::default(),
                true,
                runtime.control_handle(),
            ))
            .await
            .expect("spawn_worker should execute as coordinator control tool");
        let report = runtime
            .dispatch_goal(
                coordinator,
                Recipient::Role("reviewer".to_owned()),
                "review",
            )
            .await
            .expect("spawned worker should have a bound engine runner");

        assert_eq!(report.final_state["responses"], 1);
        assert!(report.members_usage.contains_key(&spawned));
        assert_eq!(harness.model.requests().len(), 3);
    });
}

struct TestHarness {
    store: Arc<RecordingEventStore>,
    model: Arc<ScriptedModel>,
    broker: Arc<RecordingBroker>,
    tools: ToolPool,
    engine: Arc<Engine>,
}

impl TestHarness {
    fn rebuild(&mut self) {
        self.engine = build_engine(
            self.store.clone(),
            self.model.clone(),
            self.broker.clone(),
            self.tools.clone(),
        );
    }
}

fn test_harness(script: Script) -> TestHarness {
    let store = Arc::new(RecordingEventStore::default());
    let model = Arc::new(ScriptedModel::new(script));
    let broker = Arc::new(RecordingBroker::default());
    let tools = ToolPool::default();
    let engine = build_engine(store.clone(), model.clone(), broker.clone(), tools.clone());
    TestHarness {
        store,
        model,
        broker,
        tools,
        engine,
    }
}

fn test_harness_with_subagent_tool(script: Script) -> TestHarness {
    let store = Arc::new(RecordingEventStore::default());
    let model = Arc::new(ScriptedModel::new(script));
    let broker = Arc::new(RecordingBroker::default());
    let tools = ToolPool::default();
    let engine = build_engine_with_subagent_tool(
        store.clone(),
        model.clone(),
        broker.clone(),
        tools.clone(),
        true,
    );
    TestHarness {
        store,
        model,
        broker,
        tools,
        engine,
    }
}

fn build_engine(
    store: Arc<RecordingEventStore>,
    model: Arc<ScriptedModel>,
    broker: Arc<RecordingBroker>,
    tools: ToolPool,
) -> Arc<Engine> {
    build_engine_with_subagent_tool(store, model, broker, tools, false)
}

fn build_engine_with_subagent_tool(
    store: Arc<RecordingEventStore>,
    model: Arc<ScriptedModel>,
    broker: Arc<RecordingBroker>,
    tools: ToolPool,
    enable_subagent_tool: bool,
) -> Arc<Engine> {
    let mut builder = Engine::builder()
        .with_engine_id(EngineId::new("sdk-team-member"))
        .with_event_store(store)
        .with_context(harness_context::ContextEngine::builder().build().unwrap())
        .with_hooks(HookDispatcher::new(
            HookRegistry::builder().build().unwrap().snapshot(),
        ))
        .with_model(model)
        .with_tools(tools)
        .with_permission_broker(broker)
        .with_workspace_root(std::env::temp_dir())
        .with_model_id("base-model");
    if enable_subagent_tool {
        builder = builder.with_subagent_tool();
    }
    Arc::new(builder.build().unwrap())
}

fn request_with(
    tenant_id: TenantId,
    session_id: SessionId,
    run_id: RunId,
    parent_run_id: Option<RunId>,
    correlation_id: CorrelationId,
    engine_config: TeamMemberEngineConfig,
) -> TeamMemberRunRequest {
    TeamMemberRunRequest::synthetic(
        tenant_id,
        TeamId::new(),
        AgentId::new(),
        "researcher",
        session_id,
        run_id,
        parent_run_id,
        turn_input("dispatch goal"),
        "dispatch goal",
        correlation_id,
        engine_config,
    )
}

fn request_with_control(
    tenant_id: TenantId,
    session_id: SessionId,
    run_id: RunId,
    correlation_id: CorrelationId,
    engine_config: TeamMemberEngineConfig,
    control_tools_enabled: bool,
    team_control: TeamControlHandle,
) -> TeamMemberRunRequest {
    let mut request = TeamMemberRunRequest::synthetic(
        tenant_id,
        TeamId::new(),
        AgentId::new(),
        "coordinator",
        session_id,
        run_id,
        None,
        turn_input("coordinate"),
        "coordinate",
        correlation_id,
        engine_config,
    );
    request.team_control = Some(team_control);
    request.control_tools_enabled = control_tools_enabled;
    request
}

fn test_control_handle() -> TeamControlHandle {
    let store = Arc::new(RecordingEventStore::default());
    let journal = TeamJournalContext {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
    };
    let coordinator = AgentId::new();
    let worker = AgentId::new();
    let spec = TeamBuilder::new("sdk-control", Topology::CoordinatorWorker)
        .member(coordinator, "coordinator", ContextVisibility::All)
        .member(worker, "worker", ContextVisibility::All)
        .coordinator_worker(coordinator, vec![worker])
        .build();
    let bus = MessageBus::journaled(spec.team_id, 16, journal, store.clone());
    Team::new(
        spec,
        bus,
        journal,
        store,
        Arc::new(InMemoryBlobStore::default()),
    )
    .control_handle()
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

enum Script {
    Text {
        body: String,
        usage: UsageSnapshot,
    },
    ToolThenText {
        tool_name: String,
        body: String,
        usage: UsageSnapshot,
    },
    ToolThenTextWithInput {
        tool_name: String,
        input: serde_json::Value,
        body: String,
        usage: UsageSnapshot,
    },
}

struct ScriptedModel {
    script: Script,
    calls: AtomicUsize,
    requests: Mutex<Vec<ModelRequest>>,
}

impl ScriptedModel {
    fn new(script: Script) -> Self {
        Self {
            script,
            calls: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl ModelProvider for ScriptedModel {
    fn provider_id(&self) -> &str {
        "sdk-test"
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            protocol: harness_model::ModelProtocol::Messages,
            lifecycle: harness_model::ModelLifecycle::Stable,
            provider_id: "sdk-test".to_owned(),
            model_id: "base-model".to_owned(),
            display_name: "SDK Test".to_owned(),
            context_window: 8_000,
            max_output_tokens: 1_024,
            conversation_capability: ConversationModelCapability::default(),
            runtime_semantics: harness_model::ModelRuntimeSemantics::messages_default(
                harness_model::ModelProtocol::Messages,
            ),
            pricing: None,
        }]
    }

    async fn infer(
        &self,
        request: ModelRequest,
        _ctx: InferContext,
    ) -> Result<ModelStream, ModelError> {
        self.requests.lock().unwrap().push(request);
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let events = match &self.script {
            Script::Text { body, usage } => text_events(body, usage.clone()),
            Script::ToolThenText { tool_name, .. } if call == 0 => tool_events(tool_name),
            Script::ToolThenTextWithInput {
                tool_name, input, ..
            } if call == 0 => tool_events_with_input(tool_name, input.clone()),
            Script::ToolThenText { body, usage, .. } => text_events(body, usage.clone()),
            Script::ToolThenTextWithInput { body, usage, .. } => text_events(body, usage.clone()),
        };
        Ok(Box::pin(stream::iter(events)))
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
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

fn tool_events(tool_name: &str) -> Vec<ModelStreamEvent> {
    tool_events_with_input(tool_name, serde_json::json!({}))
}

fn tool_events_with_input(tool_name: &str, input: serde_json::Value) -> Vec<ModelStreamEvent> {
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

#[derive(Default)]
struct RecordingBroker {
    contexts: Mutex<Vec<PermissionContext>>,
}

impl RecordingBroker {
    fn contexts(&self) -> Vec<PermissionContext> {
        self.contexts.lock().unwrap().clone()
    }
}

#[async_trait]
impl PermissionBroker for RecordingBroker {
    async fn decide(&self, _request: PermissionRequest, ctx: PermissionContext) -> Decision {
        self.contexts.lock().unwrap().push(ctx);
        Decision::AllowOnce
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), PermissionError> {
        Ok(())
    }
}

struct TestTool {
    descriptor: ToolDescriptor,
}

impl TestTool {
    fn new(name: &str) -> Self {
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
                budget: harness_contracts::ResultBudget {
                    metric: harness_contracts::BudgetMetric::Chars,
                    limit: 1024,
                    on_overflow: harness_contracts::OverflowAction::Truncate,
                    preview_head_chars: 128,
                    preview_tail_chars: 128,
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

    async fn check_permission(
        &self,
        input: &serde_json::Value,
        _ctx: &ToolContext,
    ) -> PermissionCheck {
        PermissionCheck::AskUser {
            subject: PermissionSubject::ToolInvocation {
                tool: self.descriptor.name.clone(),
                input: input.clone(),
            },
            scope: DecisionScope::ToolName(self.descriptor.name.clone()),
        }
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Text("ok".to_owned()),
        )])))
    }
}

#[derive(Default)]
struct RecordingEventStore {
    events: futures::lock::Mutex<Vec<EventEnvelope>>,
    snapshots: futures::lock::Mutex<HashMap<(TenantId, SessionId), SessionSnapshot>>,
}

impl RecordingEventStore {
    async fn events(&self) -> Vec<EventEnvelope> {
        self.events.lock().await.clone()
    }
}

#[async_trait]
impl EventStore for RecordingEventStore {
    async fn append(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        self.append_with_metadata(tenant, session_id, AppendMetadata::default(), events)
            .await
    }

    async fn append_with_metadata(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        metadata: AppendMetadata,
        events: &[Event],
    ) -> Result<JournalOffset, JournalError> {
        let mut guard = self.events.lock().await;
        let mut offset = guard.len() as u64;
        for event in events {
            guard.push(EventEnvelope {
                offset: JournalOffset(offset),
                event_id: EventId::new(),
                session_id,
                tenant_id: tenant,
                run_id: metadata.run_id,
                correlation_id: metadata.correlation_id,
                causation_id: metadata.causation_id,
                schema_version: SchemaVersion::CURRENT,
                recorded_at: harness_contracts::now(),
                payload: event.clone(),
            });
            offset += 1;
        }
        Ok(JournalOffset(offset.saturating_sub(1)))
    }

    async fn read_envelopes(
        &self,
        tenant: TenantId,
        session_id: SessionId,
        _cursor: ReplayCursor,
    ) -> Result<futures::stream::BoxStream<'static, EventEnvelope>, JournalError> {
        let events: Vec<_> = self
            .events
            .lock()
            .await
            .iter()
            .filter(|envelope| envelope.tenant_id == tenant && envelope.session_id == session_id)
            .cloned()
            .collect();
        Ok(Box::pin(stream::iter(events)))
    }

    async fn query_after(
        &self,
        tenant: TenantId,
        _after: Option<EventId>,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, JournalError> {
        let mut events: Vec<_> = self
            .events
            .lock()
            .await
            .iter()
            .filter(|envelope| envelope.tenant_id == tenant)
            .take(limit)
            .cloned()
            .collect();
        events.sort_by_key(|envelope| (envelope.recorded_at, envelope.offset));
        Ok(events)
    }

    async fn snapshot(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<Option<SessionSnapshot>, JournalError> {
        Ok(self
            .snapshots
            .lock()
            .await
            .get(&(tenant, session_id))
            .cloned())
    }

    async fn save_snapshot(
        &self,
        tenant: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<(), JournalError> {
        self.snapshots
            .lock()
            .await
            .insert((tenant, snapshot.session_id), snapshot);
        Ok(())
    }

    async fn compact_link(
        &self,
        _parent: SessionId,
        _child: SessionId,
        _reason: ForkReason,
    ) -> Result<(), JournalError> {
        Ok(())
    }

    async fn delete_session(
        &self,
        tenant: TenantId,
        session_id: SessionId,
    ) -> Result<bool, JournalError> {
        let mut events = self.events.lock().await;
        let before = events.len();
        events.retain(|event| !(event.tenant_id == tenant && event.session_id == session_id));
        self.snapshots.lock().await.remove(&(tenant, session_id));
        Ok(events.len() != before)
    }

    async fn list_sessions(
        &self,
        _tenant: TenantId,
        _filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>, JournalError> {
        Ok(Vec::new())
    }

    async fn prune(
        &self,
        _tenant: TenantId,
        _policy: PrunePolicy,
    ) -> Result<PruneReport, JournalError> {
        Ok(PruneReport::default())
    }
}

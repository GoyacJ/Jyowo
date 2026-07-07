use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

use async_trait::async_trait;
use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{
    canonical_mcp_tool_name, AuthorizationTicketId, CapabilityRegistry, DeferPolicy, Event,
    McpServerId, McpServerSource, PluginId, SessionId, TenantId, ToolActionPlan, ToolError,
    ToolResult, ToolUseHeartbeatEvent, ToolUseId, TrustLevel,
};
use harness_mcp::{
    collapse_reserved_separator, trust_level_for_source, FilterConflict, FilterDecision,
    JsonRpcRequest, JsonRpcResponse, ListChangedEvent, McpChange, McpClient, McpConnection,
    McpMetric, McpMetricOutcome, McpMetricsSink, McpRegistry, McpResource, McpResourceContents,
    McpServerScope, McpServerSpec, McpTimeouts, McpToolCallEvent, McpToolDescriptor, McpToolFilter,
    McpToolGlob, McpToolResult, ReconnectPolicy, SamplingPolicy, StdioEnv, TransportChoice,
};
use harness_tool::{
    AuthorizedTicketSummary, AuthorizedToolInput, InterruptToken, Tool, ToolContext, ToolEvent,
    ToolRegistry,
};
use parking_lot::Mutex;
use serde_json::{json, Value};

#[test]
fn jsonrpc_request_response_round_trips() {
    let request = JsonRpcRequest::new(
        json!(7),
        "tools/call",
        Some(json!({ "name": "grep", "arguments": { "pattern": "mcp" } })),
    );

    let value = serde_json::to_value(&request).expect("request serializes");
    let decoded: JsonRpcRequest = serde_json::from_value(value).expect("request deserializes");

    assert_eq!(decoded.jsonrpc, "2.0");
    assert_eq!(decoded.method, "tools/call");
    assert_eq!(
        decoded.params,
        Some(json!({ "name": "grep", "arguments": { "pattern": "mcp" } }))
    );

    let response = JsonRpcResponse::success(json!(7), json!({ "ok": true }));
    let value = serde_json::to_value(&response).expect("response serializes");
    let decoded: JsonRpcResponse = serde_json::from_value(value).expect("response deserializes");

    assert_eq!(decoded.result, Some(json!({ "ok": true })));
    assert!(decoded.error.is_none());
}

#[tokio::test]
async fn transport_and_connection_traits_are_object_safe() {
    let transport: Arc<dyn harness_mcp::McpTransport> =
        Arc::new(TestTransport::new(TestConnection::default()));
    let spec = server_spec("slack", McpServerSource::Workspace);

    let connection = McpClient::new(transport)
        .connect(spec)
        .await
        .expect("test transport connects");

    assert_eq!(connection.connection_id(), "test");
}

#[test]
fn server_source_derives_trust_level() {
    assert_eq!(
        trust_level_for_source(&McpServerSource::Workspace),
        TrustLevel::AdminTrusted
    );
    assert_eq!(
        trust_level_for_source(&McpServerSource::Policy),
        TrustLevel::AdminTrusted
    );
    assert_eq!(
        trust_level_for_source(&McpServerSource::Managed {
            registry_url: "https://registry.example".into()
        }),
        TrustLevel::AdminTrusted
    );
    assert_eq!(
        trust_level_for_source(&McpServerSource::User),
        TrustLevel::UserControlled
    );
    assert_eq!(
        trust_level_for_source(&McpServerSource::Dynamic {
            registered_by: "user".into()
        }),
        TrustLevel::UserControlled
    );
    assert_eq!(
        trust_level_for_source(&McpServerSource::Plugin(PluginId("plugin".into()))),
        TrustLevel::UserControlled,
        "plugin source lacks trust in contracts, so MCP fails closed"
    );
}

#[test]
fn stdio_default_env_denies_common_credentials() {
    let deny = StdioEnv::default_deny_envs();

    for key in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "AWS_SECRET_ACCESS_KEY",
        "GITHUB_TOKEN",
        "KUBECONFIG",
        "NPM_TOKEN",
        "HARNESS_*",
    ] {
        assert!(deny.contains(key), "missing deny env {key}");
    }

    assert!(matches!(
        StdioEnv::default(),
        StdioEnv::InheritWithDeny { .. }
    ));
}

#[test]
fn canonical_mcp_names_reject_or_collapse_reserved_separator() {
    assert_eq!(
        canonical_mcp_tool_name("slack", "post_message").expect("canonical name"),
        "mcp__slack__post_message"
    );
    assert!(canonical_mcp_tool_name("bad__server", "post_message").is_err());

    assert_eq!(
        collapse_reserved_separator(&McpServerId("slack".into()), "bulk__import")
            .expect("collapsed canonical name"),
        "mcp__slack__bulk_import"
    );
}

#[test]
fn tool_filter_applies_allow_deny_and_conflict_policy() {
    let filter = McpToolFilter {
        allow: vec![McpToolGlob("mcp__slack__*".into())],
        deny: vec![McpToolGlob("mcp__slack__delete_*".into())],
        on_conflict: FilterConflict::DenyWins,
    };

    assert_eq!(
        filter.evaluate("mcp__slack__post_message"),
        FilterDecision::Inject
    );
    assert!(matches!(
        filter.evaluate("mcp__slack__delete_channel"),
        FilterDecision::Skip { .. }
    ));
    assert!(matches!(
        filter.evaluate("mcp__github__create_issue"),
        FilterDecision::Skip { .. }
    ));
}

#[tokio::test]
async fn registry_injects_mcp_tool_wrapper_and_executes_test_connection() {
    let connection = TestConnection {
        tools: vec![McpToolDescriptor {
            name: "post_message".into(),
            description: Some("Post a message".into()),
            input_schema: json!({
                "type": "object",
                "properties": { "text": { "type": "string" } }
            }),
            output_schema: None,
            annotations: None,
            meta: BTreeMap::new(),
        }],
        ..Default::default()
    };
    connection
        .results
        .lock()
        .push_back(McpToolResult::text("sent"));

    let metrics = Arc::new(CollectingMetrics::default());
    let mcp_registry = McpRegistry::with_metrics_sink(metrics.clone());
    let server_id = McpServerId("slack".into());
    let spec = server_spec("slack", McpServerSource::Workspace);
    mcp_registry
        .add_ready_server(
            spec,
            McpServerScope::Session(SessionId::new()),
            Arc::new(connection),
        )
        .await
        .expect("server registers");

    let tool_registry = ToolRegistry::builder().build().expect("tool registry");
    let injected = mcp_registry
        .inject_tools_into(&tool_registry, &server_id)
        .await
        .expect("tools inject");

    assert_eq!(injected, vec!["mcp__slack__post_message"]);
    let descriptor = tool_registry
        .snapshot()
        .descriptor("mcp__slack__post_message")
        .expect("descriptor exists")
        .as_ref()
        .clone();
    assert_eq!(descriptor.properties.defer_policy, DeferPolicy::AutoDefer);
    assert_eq!(descriptor.trust_level, TrustLevel::AdminTrusted);

    let tool = tool_registry
        .get("mcp__slack__post_message")
        .expect("tool registered");
    let mut stream = run_authorized(&tool, json!({ "text": "hello" }), tool_context())
        .await
        .expect("tool executes");

    let event = stream.next().await.expect("final event");
    assert_eq!(event, ToolEvent::Final(ToolResult::Text("sent".into())));
    assert!(metrics.metrics().iter().any(|metric| {
        matches!(
            metric,
            McpMetric::ToolInvocation {
                outcome: McpMetricOutcome::Success,
                ..
            }
        )
    }));
}

#[tokio::test]
async fn registry_records_metric_when_tool_filter_skips_tool() {
    let connection = TestConnection {
        tools: vec![
            McpToolDescriptor {
                name: "lookup".into(),
                description: Some("Lookup".into()),
                input_schema: json!({ "type": "object" }),
                output_schema: None,
                annotations: None,
                meta: BTreeMap::new(),
            },
            McpToolDescriptor {
                name: "delete_record".into(),
                description: Some("Delete".into()),
                input_schema: json!({ "type": "object" }),
                output_schema: None,
                annotations: None,
                meta: BTreeMap::new(),
            },
        ],
        ..Default::default()
    };
    let metrics = Arc::new(CollectingMetrics::default());
    let mcp_registry = McpRegistry::with_metrics_sink(metrics.clone());
    let server_id = McpServerId("crm".into());
    let mut spec = server_spec("crm", McpServerSource::Workspace);
    spec.tool_filter = McpToolFilter {
        allow: Vec::new(),
        deny: vec![McpToolGlob("mcp__crm__delete_*".into())],
        on_conflict: FilterConflict::DenyWins,
    };
    mcp_registry
        .add_ready_server(
            spec,
            McpServerScope::Session(SessionId::new()),
            Arc::new(connection),
        )
        .await
        .expect("server registers");
    let tool_registry = ToolRegistry::builder().build().expect("tool registry");

    let injected = mcp_registry
        .inject_tools_into(&tool_registry, &server_id)
        .await
        .expect("tools inject");

    assert_eq!(injected, vec!["mcp__crm__lookup"]);
    assert!(metrics.metrics().iter().any(|metric| {
        matches!(
            metric,
            McpMetric::ToolFilterSkipped { server_id, reason }
                if server_id == &McpServerId("crm".into()) && *reason == "deny_matched"
        )
    }));
}

#[tokio::test]
async fn mcp_tool_wrapper_validates_input_schema_before_upstream_call() {
    let connection = TestConnection {
        tools: vec![McpToolDescriptor {
            name: "post_message".into(),
            description: Some("Post a message".into()),
            input_schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["text"],
                "properties": { "text": { "type": "string" } }
            }),
            output_schema: None,
            annotations: None,
            meta: BTreeMap::new(),
        }],
        ..Default::default()
    };

    let mcp_registry = McpRegistry::new();
    let server_id = McpServerId("slack".into());
    let spec = server_spec("slack", McpServerSource::Workspace);
    mcp_registry
        .add_ready_server(
            spec,
            McpServerScope::Session(SessionId::new()),
            Arc::new(connection),
        )
        .await
        .expect("server registers");

    let tool_registry = ToolRegistry::builder().build().expect("tool registry");
    mcp_registry
        .inject_tools_into(&tool_registry, &server_id)
        .await
        .expect("tools inject");

    let tool = tool_registry
        .get("mcp__slack__post_message")
        .expect("tool registered");
    let error = tool
        .validate(&json!({ "extra": true }), &tool_context())
        .await
        .expect_err("invalid input should fail schema validation");

    assert!(error
        .to_string()
        .contains("mcp tool input schema validation failed"));
}

#[tokio::test]
async fn mcp_tool_wrapper_maps_trusted_mcp_annotations_to_tool_properties() {
    let connection = TestConnection {
        tools: vec![McpToolDescriptor {
            name: "lookup".into(),
            description: Some("Lookup".into()),
            input_schema: json!({ "type": "object" }),
            output_schema: None,
            annotations: Some(harness_mcp::McpToolAnnotations {
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
            }),
            meta: BTreeMap::new(),
        }],
        ..Default::default()
    };

    let mcp_registry = McpRegistry::new();
    let server_id = McpServerId("trusted".into());
    mcp_registry
        .add_ready_server(
            server_spec("trusted", McpServerSource::Workspace),
            McpServerScope::Session(SessionId::new()),
            Arc::new(connection),
        )
        .await
        .expect("server registers");
    let tool_registry = ToolRegistry::builder().build().expect("tool registry");
    mcp_registry
        .inject_tools_into(&tool_registry, &server_id)
        .await
        .expect("tools inject");

    let descriptor = tool_registry
        .snapshot()
        .descriptor("mcp__trusted__lookup")
        .expect("descriptor exists")
        .as_ref()
        .clone();

    assert!(descriptor.properties.is_read_only);
    assert!(!descriptor.properties.is_destructive);
    assert!(descriptor.properties.is_concurrency_safe);
    let harness_contracts::ToolOrigin::Mcp(origin) = descriptor.origin else {
        panic!("expected mcp origin");
    };
    assert_eq!(origin.server_meta.get("openWorldHint"), Some(&json!(false)));
}

#[tokio::test]
async fn mcp_tool_wrapper_keeps_untrusted_annotations_fail_closed() {
    let connection = TestConnection {
        tools: vec![McpToolDescriptor {
            name: "lookup".into(),
            description: Some("Lookup".into()),
            input_schema: json!({ "type": "object" }),
            output_schema: None,
            annotations: Some(harness_mcp::McpToolAnnotations {
                read_only_hint: Some(true),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
            }),
            meta: BTreeMap::new(),
        }],
        ..Default::default()
    };

    let mcp_registry = McpRegistry::new();
    let server_id = McpServerId("user".into());
    mcp_registry
        .add_ready_server(
            server_spec("user", McpServerSource::User),
            McpServerScope::Session(SessionId::new()),
            Arc::new(connection),
        )
        .await
        .expect("server registers");
    let tool_registry = ToolRegistry::builder().build().expect("tool registry");
    mcp_registry
        .inject_tools_into(&tool_registry, &server_id)
        .await
        .expect("tools inject");

    let descriptor = tool_registry
        .snapshot()
        .descriptor("mcp__user__lookup")
        .expect("descriptor exists")
        .as_ref()
        .clone();

    assert!(!descriptor.properties.is_read_only);
    assert!(descriptor.properties.is_destructive);
    assert!(!descriptor.properties.is_concurrency_safe);
}

#[tokio::test]
async fn mcp_tool_wrapper_maps_mcp_progress_to_progress_and_heartbeat_events() {
    let connection = TestConnection {
        tools: vec![McpToolDescriptor {
            name: "post_message".into(),
            description: Some("Post a message".into()),
            input_schema: json!({
                "type": "object",
                "properties": { "text": { "type": "string" } }
            }),
            output_schema: None,
            annotations: None,
            meta: BTreeMap::new(),
        }],
        ..Default::default()
    };
    connection.streams.lock().push_back(vec![
        McpToolCallEvent::Progress {
            progress_token: Some("2".into()),
            progress: Some(1.0),
            total: Some(4.0),
            message: Some("quarter".into()),
        },
        McpToolCallEvent::Final(McpToolResult::text("sent")),
    ]);
    let ctx = tool_context();
    let tool_use_id = ctx.tool_use_id;
    let run_id = ctx.run_id;

    let mcp_registry = McpRegistry::new();
    let server_id = McpServerId("slack".into());
    mcp_registry
        .add_ready_server(
            server_spec("slack", McpServerSource::Workspace),
            McpServerScope::Session(SessionId::new()),
            Arc::new(connection),
        )
        .await
        .expect("server registers");

    let tool_registry = ToolRegistry::builder().build().expect("tool registry");
    mcp_registry
        .inject_tools_into(&tool_registry, &server_id)
        .await
        .expect("tools inject");

    let tool = tool_registry
        .get("mcp__slack__post_message")
        .expect("tool registered");
    let mut stream = run_authorized(&tool, json!({ "text": "hello" }), ctx)
        .await
        .expect("tool executes");

    assert!(matches!(
        stream.next().await,
        Some(ToolEvent::Progress(progress))
            if progress.message == "quarter" && progress.fraction == Some(0.25)
    ));
    assert!(matches!(
        stream.next().await,
        Some(ToolEvent::Journal(Event::ToolUseHeartbeat(ToolUseHeartbeatEvent {
            tool_use_id: actual_tool_use_id,
            run_id: actual_run_id,
            message,
            fraction: Some(0.25),
            silent_for_ms: 0,
            ..
        }))) if actual_tool_use_id == tool_use_id && actual_run_id == run_id && message == "quarter"
    ));
    assert_eq!(
        stream.next().await,
        Some(ToolEvent::Final(ToolResult::Text("sent".into())))
    );
}

#[tokio::test]
async fn mcp_tool_wrapper_maps_mcp_cancelled_to_interrupted_error() {
    let connection = TestConnection {
        tools: vec![McpToolDescriptor {
            name: "post_message".into(),
            description: Some("Post a message".into()),
            input_schema: json!({
                "type": "object",
                "properties": { "text": { "type": "string" } }
            }),
            output_schema: None,
            annotations: None,
            meta: BTreeMap::new(),
        }],
        ..Default::default()
    };
    connection.streams.lock().push_back(vec![
        McpToolCallEvent::Cancelled {
            request_id: Some("2".into()),
            reason: Some("client interrupted".into()),
        },
        McpToolCallEvent::Final(McpToolResult::text("late")),
    ]);

    let mcp_registry = McpRegistry::new();
    let server_id = McpServerId("slack".into());
    mcp_registry
        .add_ready_server(
            server_spec("slack", McpServerSource::Workspace),
            McpServerScope::Session(SessionId::new()),
            Arc::new(connection),
        )
        .await
        .expect("server registers");

    let tool_registry = ToolRegistry::builder().build().expect("tool registry");
    mcp_registry
        .inject_tools_into(&tool_registry, &server_id)
        .await
        .expect("tools inject");

    let tool = tool_registry
        .get("mcp__slack__post_message")
        .expect("tool registered");
    let mut stream = run_authorized(&tool, json!({ "text": "hello" }), tool_context())
        .await
        .expect("tool executes");

    assert_eq!(
        stream.next().await,
        Some(ToolEvent::Error(ToolError::Interrupted))
    );
}

#[tokio::test]
async fn mcp_tool_wrapper_sends_cancel_and_times_out_when_upstream_ignores_ack() {
    let connection = Arc::new(TestConnection {
        tools: vec![McpToolDescriptor {
            name: "post_message".into(),
            description: Some("Post a message".into()),
            input_schema: json!({
                "type": "object",
                "properties": { "text": { "type": "string" } }
            }),
            output_schema: None,
            annotations: None,
            meta: BTreeMap::new(),
        }],
        pending_streams: Mutex::new(1),
        ..Default::default()
    });

    let mcp_registry = McpRegistry::new();
    let server_id = McpServerId("slack".into());
    let mut spec = server_spec("slack", McpServerSource::Workspace);
    spec.timeouts.cancel_ack = std::time::Duration::from_millis(20);
    mcp_registry
        .add_ready_server(
            spec,
            McpServerScope::Session(SessionId::new()),
            connection.clone(),
        )
        .await
        .expect("server registers");

    let tool_registry = ToolRegistry::builder().build().expect("tool registry");
    mcp_registry
        .inject_tools_into(&tool_registry, &server_id)
        .await
        .expect("tools inject");

    let interrupt = InterruptToken::new();
    let mut ctx = tool_context();
    ctx.interrupt = interrupt.clone();
    let tool = tool_registry
        .get("mcp__slack__post_message")
        .expect("tool registered");
    let mut stream = run_authorized(&tool, json!({ "text": "hello" }), ctx)
        .await
        .expect("tool executes");

    interrupt.interrupt();

    assert_eq!(
        stream.next().await,
        Some(ToolEvent::Error(ToolError::Interrupted))
    );
    assert_eq!(connection.cancelled_count(), 1);
}

#[test]
fn policy_defaults_are_fail_closed_or_bounded() {
    assert_eq!(
        McpTimeouts::default().handshake.as_secs(),
        5,
        "handshake timeout should stay bounded"
    );
    assert_eq!(
        ReconnectPolicy::default().max_attempts,
        0,
        "0 means unlimited retries"
    );
    assert!(ReconnectPolicy::default().keep_deferred_during_reconnect);
    assert!(SamplingPolicy::denied().is_denied());
    assert_eq!(McpTimeouts::default().cancel_ack.as_secs(), 5);
}

fn server_spec(id: &str, source: McpServerSource) -> McpServerSpec {
    McpServerSpec::new(
        McpServerId(id.into()),
        format!("{id} server"),
        TransportChoice::InProcess,
        source,
    )
}

fn tool_context() -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: std::path::PathBuf::from("."),
        project_workspace_root: None,
        sandbox: None,
        cap_registry: Arc::new(CapabilityRegistry::default()),
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::new(),
        parent_run: None,
        model: None,
        model_config_id: None,
        memory_thread_settings: None,
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
    }
}

async fn run_authorized(
    tool: &Arc<dyn Tool>,
    input: Value,
    ctx: ToolContext,
) -> Result<harness_tool::ToolStream, ToolError> {
    tool.validate(&input, &ctx)
        .await
        .expect("test input validates");
    let plan = tool.plan(&input, &ctx).await?;
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan))?;
    tool.execute_authorized(authorized, ctx).await
}

fn ticket_for(plan: &ToolActionPlan) -> AuthorizedTicketSummary {
    AuthorizedTicketSummary {
        ticket_id: AuthorizationTicketId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        run_id: harness_contracts::RunId::new(),
        tool_use_id: plan.tool_use_id,
        tool_name: plan.tool_name.clone(),
        action_plan_hash: plan.plan_hash.clone(),
        consumed_at: Utc::now(),
    }
}

#[derive(Default)]
struct TestConnection {
    tools: Vec<McpToolDescriptor>,
    results: Mutex<VecDeque<McpToolResult>>,
    streams: Mutex<VecDeque<Vec<McpToolCallEvent>>>,
    pending_streams: Mutex<usize>,
    cancelled: Mutex<Vec<String>>,
}

impl TestConnection {
    fn cancelled_count(&self) -> usize {
        self.cancelled.lock().len()
    }
}

#[async_trait]
impl McpConnection for TestConnection {
    fn connection_id(&self) -> &'static str {
        "test"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, harness_mcp::McpError> {
        Ok(self.tools.clone())
    }

    async fn call_tool(
        &self,
        _name: &str,
        _args: Value,
    ) -> Result<McpToolResult, harness_mcp::McpError> {
        self.results
            .lock()
            .pop_front()
            .ok_or_else(|| harness_mcp::McpError::Protocol("missing test result".into()))
    }

    async fn call_tool_events(
        &self,
        _name: &str,
        _args: Value,
    ) -> Result<harness_mcp::McpToolCallStream, harness_mcp::McpError> {
        if *self.pending_streams.lock() > 0 {
            *self.pending_streams.lock() -= 1;
            return Ok(Box::pin(futures::stream::pending()));
        }
        let events = if let Some(events) = self.streams.lock().pop_front() {
            events
        } else {
            vec![McpToolCallEvent::Final(
                self.results
                    .lock()
                    .pop_front()
                    .ok_or_else(|| harness_mcp::McpError::Protocol("missing test result".into()))?,
            )]
        };
        Ok(Box::pin(futures::stream::iter(events)))
    }

    async fn cancel_tool_call(
        &self,
        request_id: &str,
        _reason: Option<String>,
    ) -> Result<(), harness_mcp::McpError> {
        self.cancelled.lock().push(request_id.to_owned());
        Ok(())
    }

    async fn list_resources(&self) -> Result<Vec<McpResource>, harness_mcp::McpError> {
        Ok(Vec::new())
    }

    async fn read_resource(
        &self,
        _uri: &str,
    ) -> Result<McpResourceContents, harness_mcp::McpError> {
        Err(harness_mcp::McpError::Protocol("not implemented".into()))
    }

    async fn subscribe_changes(&self) -> Result<ListChangedEvent, harness_mcp::McpError> {
        Ok(Box::pin(futures::stream::empty::<McpChange>()))
    }

    async fn shutdown(&self) -> Result<(), harness_mcp::McpError> {
        Ok(())
    }
}

#[derive(Default)]
struct CollectingMetrics {
    metrics: Mutex<Vec<McpMetric>>,
}

impl CollectingMetrics {
    fn metrics(&self) -> Vec<McpMetric> {
        self.metrics.lock().clone()
    }
}

impl McpMetricsSink for CollectingMetrics {
    fn record(&self, metric: McpMetric) {
        self.metrics.lock().push(metric);
    }
}

struct TestTransport {
    connection: Arc<dyn McpConnection>,
}

impl TestTransport {
    fn new(connection: TestConnection) -> Self {
        Self {
            connection: Arc::new(connection),
        }
    }
}

#[async_trait]
impl harness_mcp::McpTransport for TestTransport {
    fn transport_id(&self) -> &'static str {
        "test"
    }

    async fn connect(
        &self,
        _spec: McpServerSpec,
    ) -> Result<Arc<dyn McpConnection>, harness_mcp::McpError> {
        Ok(Arc::clone(&self.connection))
    }
}

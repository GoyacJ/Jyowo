use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{
    AuthorizationTicketId, CacheImpact, CapabilityRegistry, DeferPolicy, Event, HarnessError,
    ProviderRestriction, RunId, SessionId, TenantId, ToolActionPlan, ToolDescriptor, ToolError,
    ToolGroup, ToolOrigin, ToolProperties, ToolResult, ToolUseId, TrustLevel,
};
use harness_model::ConversationModelCapability;
use harness_tool::{
    AuthorizedTicketSummary, AuthorizedToolInput, InterruptToken, Tool, ToolContext,
};
use harness_tool_search::{
    DefaultScorer, MaterializeOutcome, ReloadHandle, ToolLoadingBackend, ToolLoadingBackendName,
    ToolLoadingContext, ToolSearchRuntimeCap, ToolSearchRuntimeSnapshot, ToolSearchTool,
    TOOL_SEARCH_RUNTIME_CAPABILITY,
};
use serde_json::{json, Value};

#[tokio::test]
async fn descriptor_is_always_loaded_meta_tool() {
    let tool = ToolSearchTool::builder().build();

    let descriptor = tool.descriptor();
    assert_eq!(descriptor.name, "tool_search");
    assert_eq!(descriptor.group, ToolGroup::Meta);
    assert_eq!(descriptor.trust_level, TrustLevel::AdminTrusted);
    assert_eq!(descriptor.properties.defer_policy, DeferPolicy::AlwaysLoad);
    assert!(descriptor.required_capabilities.is_empty());

    let output_schema = descriptor.output_schema.as_ref().unwrap();
    let properties = output_schema["properties"].as_object().unwrap();
    for field in [
        "matches",
        "query",
        "total_deferred_tools",
        "pending_mcp_servers",
        "materialization",
    ] {
        assert!(properties.contains_key(field), "missing {field}");
    }
    let variants = output_schema["properties"]["materialization"]["oneOf"]
        .as_array()
        .unwrap();
    let kinds = variants
        .iter()
        .filter_map(|variant| {
            variant["properties"]["kind"]["const"]
                .as_str()
                .map(str::to_owned)
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        kinds,
        BTreeSet::from([
            "tool_reference".to_owned(),
            "inline_reinjected".to_owned(),
            "no_match".to_owned()
        ])
    );
}

#[tokio::test]
async fn select_query_materializes_only_deferred_matches() {
    let backend = Arc::new(RecordingBackend::default());
    let runtime = Arc::new(FakeRuntime::new(ToolSearchRuntimeSnapshot {
        deferred_tools: vec![descriptor("ReadFile", "Read file contents", None)],
        loaded_tool_names: BTreeSet::from(["AlreadyLoaded".to_owned()]),
        discovered_tool_names: BTreeSet::new(),
        pending_mcp_servers: Vec::new(),
        model_caps: Arc::new(ConversationModelCapability::default()),
        reload_handle: None,
    }));
    let tool = ToolSearchTool::builder()
        .with_scorer(Arc::new(DefaultScorer::default()))
        .with_backend_selector(Arc::new(StaticSelector::new(backend.clone())))
        .build();

    let result = execute(
        &tool,
        runtime.clone(),
        json!({ "query": "select:ReadFile,AlreadyLoaded" }),
    )
    .await;

    assert_eq!(result["matches"], json!(["ReadFile", "AlreadyLoaded"]));
    assert_eq!(
        result["materialization"],
        json!({
            "kind": "tool_reference",
            "tool_names": ["ReadFile"]
        })
    );
    assert_eq!(backend.requested().await, vec!["ReadFile".to_owned()]);
    assert!(runtime
        .events()
        .await
        .iter()
        .any(|event| matches!(event, Event::ToolSearchQueried(_))));
    assert!(runtime
        .events()
        .await
        .iter()
        .any(|event| matches!(event, Event::ToolSchemaMaterialized(_))));
}

#[tokio::test]
async fn keyword_query_scores_and_clamps_results() {
    let backend = Arc::new(RecordingBackend::default());
    let runtime = Arc::new(FakeRuntime::new(ToolSearchRuntimeSnapshot {
        deferred_tools: vec![
            descriptor(
                "mcp__slack__post_message",
                "Post a Slack message",
                Some("slack send"),
            ),
            descriptor(
                "mcp__slack__list_channels",
                "List Slack channels",
                Some("slack list"),
            ),
            descriptor("ReadFile", "Read file contents", Some("file read")),
        ],
        loaded_tool_names: BTreeSet::new(),
        discovered_tool_names: BTreeSet::from(["mcp__slack__list_channels".to_owned()]),
        pending_mcp_servers: vec!["slow-server".to_owned()],
        model_caps: Arc::new(ConversationModelCapability::default()),
        reload_handle: None,
    }));
    let tool = ToolSearchTool::builder()
        .with_scorer(Arc::new(DefaultScorer::default()))
        .with_backend_selector(Arc::new(StaticSelector::new(backend)))
        .build();

    let result = execute(
        &tool,
        runtime,
        json!({ "query": "+slack message", "max_results": 1 }),
    )
    .await;

    assert_eq!(result["matches"], json!(["mcp__slack__post_message"]));
    assert_eq!(result["pending_mcp_servers"], json!(["slow-server"]));
    assert_eq!(result["total_deferred_tools"], json!(3));
}

#[tokio::test]
async fn no_match_does_not_materialize() {
    let backend = Arc::new(RecordingBackend::default());
    let runtime = Arc::new(FakeRuntime::new(ToolSearchRuntimeSnapshot {
        deferred_tools: vec![descriptor("ReadFile", "Read file contents", None)],
        loaded_tool_names: BTreeSet::new(),
        discovered_tool_names: BTreeSet::new(),
        pending_mcp_servers: Vec::new(),
        model_caps: Arc::new(ConversationModelCapability::default()),
        reload_handle: None,
    }));
    let tool = ToolSearchTool::builder()
        .with_scorer(Arc::new(DefaultScorer::default()))
        .with_backend_selector(Arc::new(StaticSelector::new(backend.clone())))
        .build();

    let result = execute(&tool, runtime, json!({ "query": "slack" })).await;

    assert_eq!(result["matches"], json!([]));
    assert_eq!(result["materialization"], json!({ "kind": "no_match" }));
    assert!(backend.requested().await.is_empty());
}

#[tokio::test]
async fn default_backend_uses_inline_reinjection_for_non_tool_reference_models() {
    let reload = Arc::new(RecordingReloadHandle::default());
    let mut model_caps = ConversationModelCapability::default();
    model_caps.tool_calling = false;
    let runtime = Arc::new(FakeRuntime::new(ToolSearchRuntimeSnapshot {
        deferred_tools: vec![descriptor("ReadFile", "Read file contents", None)],
        loaded_tool_names: BTreeSet::new(),
        discovered_tool_names: BTreeSet::new(),
        pending_mcp_servers: Vec::new(),
        model_caps: Arc::new(model_caps),
        reload_handle: Some(reload.clone()),
    }));
    let tool = ToolSearchTool::builder()
        .with_scorer(Arc::new(DefaultScorer::default()))
        .with_coalesce_window(std::time::Duration::ZERO)
        .build();

    let result = execute(
        &tool,
        runtime.clone(),
        json!({ "query": "select:ReadFile" }),
    )
    .await;

    assert_eq!(
        result["materialization"],
        json!({
            "kind": "inline_reinjected",
            "tool_names": ["ReadFile"],
            "cache_impact": {
                "prompt_cache_invalidated": true,
                "reason": "test_reload"
            }
        })
    );
    assert_eq!(reload.requested().await, vec!["ReadFile".to_owned()]);
    assert!(runtime.events().await.iter().any(|event| {
        matches!(
            event,
            Event::ToolSchemaMaterialized(materialized)
                if materialized.backend == "inline_reinjection"
                    && materialized.cache_impact.prompt_cache_invalidated
        )
    }));
}

#[tokio::test]
async fn default_backend_rejects_inline_reinjection_when_reload_handle_is_missing() {
    let mut model_caps = ConversationModelCapability::default();
    model_caps.tool_calling = false;
    let runtime = Arc::new(FakeRuntime::new(ToolSearchRuntimeSnapshot {
        deferred_tools: vec![descriptor("ReadFile", "Read file contents", None)],
        loaded_tool_names: BTreeSet::new(),
        discovered_tool_names: BTreeSet::new(),
        pending_mcp_servers: Vec::new(),
        model_caps: Arc::new(model_caps),
        reload_handle: None,
    }));
    let tool = ToolSearchTool::builder()
        .with_scorer(Arc::new(DefaultScorer::default()))
        .build();

    let error = try_execute(
        &tool,
        runtime.clone(),
        json!({ "query": "select:ReadFile" }),
    )
    .await
    .expect_err("inline reinjection without reload handle must fail closed");

    assert!(matches!(
        error,
        ToolError::Internal(message) if message.contains("reload handle missing")
    ));
    assert!(!runtime.events().await.iter().any(|event| {
        matches!(
            event,
            Event::ToolSchemaMaterialized(materialized)
                if materialized.backend == "anthropic_tool_reference_degraded_no_reload_handle"
        )
    }));
}

async fn execute(tool: &ToolSearchTool, runtime: Arc<FakeRuntime>, input: Value) -> Value {
    try_execute(tool, runtime, input)
        .await
        .expect("tool_search should execute")
}

async fn try_execute(
    tool: &ToolSearchTool,
    runtime: Arc<FakeRuntime>,
    input: Value,
) -> Result<Value, ToolError> {
    let mut caps = CapabilityRegistry::default();
    let cap: Arc<dyn ToolSearchRuntimeCap> = runtime;
    caps.install(
        harness_contracts::ToolCapability::Custom(TOOL_SEARCH_RUNTIME_CAPABILITY.to_owned()),
        cap,
    );
    let ctx = ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: RunId::new(),
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: std::env::temp_dir(),
        project_workspace_root: None,
        sandbox: None,
        cap_registry: Arc::new(caps),
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::new(),
        parent_run: None,
        model: None,
        model_config_id: None,
        memory_thread_settings: None,
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
    };
    tool.validate(&input, &ctx).await.unwrap();
    let plan = tool.plan(&input, &ctx).await?;
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan))?;
    let mut stream = tool.execute_authorized(authorized, ctx).await?;
    match stream.next().await.unwrap() {
        harness_tool::ToolEvent::Final(ToolResult::Structured(value)) => Ok(value),
        other => panic!("unexpected event: {other:?}"),
    }
}

fn ticket_for(plan: &ToolActionPlan) -> AuthorizedTicketSummary {
    AuthorizedTicketSummary {
        ticket_id: AuthorizationTicketId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        run_id: RunId::new(),
        tool_use_id: plan.tool_use_id,
        tool_name: plan.tool_name.clone(),
        action_plan_hash: plan.plan_hash.clone(),
        consumed_at: Utc::now(),
    }
}

#[derive(Default)]
struct RecordingReloadHandle {
    requested: tokio::sync::Mutex<Vec<String>>,
}

impl RecordingReloadHandle {
    async fn requested(&self) -> Vec<String> {
        self.requested.lock().await.clone()
    }
}

#[async_trait]
impl ReloadHandle for RecordingReloadHandle {
    async fn reload_with_add_tools(&self, tools: Vec<String>) -> Result<CacheImpact, HarnessError> {
        self.requested.lock().await.extend(tools);
        Ok(CacheImpact {
            prompt_cache_invalidated: true,
            reason: Some("test_reload".to_owned()),
        })
    }
}

fn descriptor(name: &str, description: &str, search_hint: Option<&str>) -> ToolDescriptor {
    ToolDescriptor {
        name: name.to_owned(),
        display_name: name.to_owned(),
        description: description.to_owned(),
        category: "test".to_owned(),
        group: ToolGroup::Custom("test".to_owned()),
        version: "0.1.0".to_owned(),
        input_schema: json!({ "type": "object" }),
        output_schema: None,
        dynamic_schema: false,
        properties: ToolProperties {
            is_concurrency_safe: true,
            is_read_only: true,
            is_destructive: false,
            long_running: None,
            defer_policy: DeferPolicy::AutoDefer,
        },
        trust_level: TrustLevel::AdminTrusted,
        required_capabilities: Vec::new(),
        budget: harness_tool::default_result_budget(),
        provider_restriction: ProviderRestriction::All,
        origin: ToolOrigin::Builtin,
        search_hint: search_hint.map(str::to_owned),
        service_binding: None,
    }
}

#[derive(Default)]
struct RecordingBackend {
    requested: tokio::sync::Mutex<Vec<String>>,
}

impl RecordingBackend {
    async fn requested(&self) -> Vec<String> {
        self.requested.lock().await.clone()
    }
}

#[async_trait]
impl ToolLoadingBackend for RecordingBackend {
    fn backend_name(&self) -> ToolLoadingBackendName {
        "recording".to_owned()
    }

    async fn materialize(
        &self,
        _ctx: &ToolLoadingContext,
        requested: &[String],
    ) -> Result<MaterializeOutcome, harness_tool_search::ToolLoadingError> {
        self.requested.lock().await.extend_from_slice(requested);
        Ok(MaterializeOutcome::ToolReferenceEmitted {
            refs: requested
                .iter()
                .map(|tool_name| harness_tool_search::ToolReference {
                    tool_name: tool_name.clone(),
                })
                .collect(),
        })
    }
}

struct StaticSelector {
    backend: Arc<dyn ToolLoadingBackend>,
}

impl StaticSelector {
    fn new(backend: Arc<dyn ToolLoadingBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl harness_tool_search::ToolLoadingBackendSelector for StaticSelector {
    async fn select(&self, _ctx: &ToolLoadingContext) -> Arc<dyn ToolLoadingBackend> {
        self.backend.clone()
    }
}

struct FakeRuntime {
    snapshot: ToolSearchRuntimeSnapshot,
    events: tokio::sync::Mutex<Vec<Event>>,
}

impl FakeRuntime {
    fn new(snapshot: ToolSearchRuntimeSnapshot) -> Self {
        Self {
            snapshot,
            events: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    async fn events(&self) -> Vec<Event> {
        self.events.lock().await.clone()
    }
}

#[async_trait]
impl ToolSearchRuntimeCap for FakeRuntime {
    async fn snapshot(&self) -> Result<ToolSearchRuntimeSnapshot, ToolError> {
        Ok(self.snapshot.clone())
    }

    async fn emit_event(&self, event: Event) -> Result<(), ToolError> {
        self.events.lock().await.push(event);
        Ok(())
    }
}

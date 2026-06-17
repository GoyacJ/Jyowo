use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::{
    CapabilityRegistry, Decision, DeferPolicy, Event, HookEventKind, PermissionError,
    ProviderRestriction, RunId, SessionId, TenantId, ToolDescriptor, ToolError, ToolGroup,
    ToolOrigin, ToolProperties, ToolResult, ToolSearchQueryKind, ToolUseId, TrustLevel,
};
use harness_model::ModelCapabilities;
use harness_tool::{
    InterruptToken, PermissionBroker, PermissionContext, PermissionRequest, PersistedDecision,
    Tool, ToolContext,
};
use harness_tool_search::{
    MaterializeOutcome, ToolLoadingBackend, ToolLoadingBackendName, ToolLoadingContext,
    ToolSearchPreHookOutcome, ToolSearchRuntimeCap, ToolSearchRuntimeSnapshot, ToolSearchTool,
    TOOL_SEARCH_RUNTIME_CAPABILITY,
};
use serde_json::{json, Value};
use tokio::sync::Mutex;

#[tokio::test]
async fn pre_search_rewrite_and_post_materialize_hooks_are_emitted() {
    let runtime = Arc::new(FakeRuntime::new(ToolSearchRuntimeSnapshot {
        deferred_tools: vec![descriptor("Rewritten")],
        loaded_tool_names: BTreeSet::new(),
        discovered_tool_names: BTreeSet::new(),
        pending_mcp_servers: Vec::new(),
        model_caps: Arc::new(ModelCapabilities::default()),
        reload_handle: None,
    }));
    let tool = ToolSearchTool::builder()
        .with_backend_selector(Arc::new(StaticSelector))
        .build();

    let result = execute(
        &tool,
        runtime.clone(),
        json!({ "query": "select:Original" }),
    )
    .await;

    assert_eq!(result["query"], json!("select:Rewritten"));
    assert_eq!(result["matches"], json!(["Rewritten"]));
    assert_eq!(
        runtime.hook_kinds().await,
        vec![
            HookEventKind::PreToolSearch,
            HookEventKind::PostToolSearchMaterialize
        ]
    );
    let events = runtime.events().await;
    assert!(events.iter().any(|event| matches!(
        event,
        Event::HookTriggered(triggered)
            if triggered.hook_event_kind == HookEventKind::PreToolSearch
                && triggered.outcome_summary.rewrote_input
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::HookRewroteInput(_))));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::HookTriggered(triggered)
            if triggered.hook_event_kind == HookEventKind::PostToolSearchMaterialize
    )));
    let order = events
        .iter()
        .filter_map(|event| match event {
            Event::HookTriggered(triggered)
                if triggered.hook_event_kind == HookEventKind::PreToolSearch =>
            {
                Some("pre")
            }
            Event::ToolSearchQueried(_) => Some("query"),
            Event::ToolSchemaMaterialized(_) => Some("materialize"),
            Event::HookTriggered(triggered)
                if triggered.hook_event_kind == HookEventKind::PostToolSearchMaterialize =>
            {
                Some("post")
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(order, vec!["pre", "query", "materialize", "post"]);
}

async fn execute(tool: &ToolSearchTool, runtime: Arc<FakeRuntime>, input: Value) -> Value {
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
        sandbox: None,
        permission_broker: Arc::new(AllowBroker),
        cap_registry: Arc::new(caps),
        interrupt: InterruptToken::new(),
        parent_run: None,
    };
    tool.validate(&input, &ctx).await.unwrap();
    let mut stream = tool.execute(input, ctx).await.unwrap();
    match stream.next().await.unwrap() {
        harness_tool::ToolEvent::Final(ToolResult::Structured(value)) => value,
        other => panic!("unexpected event: {other:?}"),
    }
}

fn descriptor(name: &str) -> ToolDescriptor {
    ToolDescriptor {
        name: name.to_owned(),
        display_name: name.to_owned(),
        description: "test tool".to_owned(),
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
        search_hint: None,
    }
}

struct StaticSelector;

#[async_trait]
impl harness_tool_search::ToolLoadingBackendSelector for StaticSelector {
    async fn select(&self, _ctx: &ToolLoadingContext) -> Arc<dyn ToolLoadingBackend> {
        Arc::new(RecordingBackend)
    }
}

struct RecordingBackend;

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

struct FakeRuntime {
    snapshot: ToolSearchRuntimeSnapshot,
    events: Mutex<Vec<Event>>,
    hook_kinds: Mutex<Vec<HookEventKind>>,
}

impl FakeRuntime {
    fn new(snapshot: ToolSearchRuntimeSnapshot) -> Self {
        Self {
            snapshot,
            events: Mutex::new(Vec::new()),
            hook_kinds: Mutex::new(Vec::new()),
        }
    }

    async fn events(&self) -> Vec<Event> {
        self.events.lock().await.clone()
    }

    async fn hook_kinds(&self) -> Vec<HookEventKind> {
        self.hook_kinds.lock().await.clone()
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

    async fn dispatch_pre_tool_search_hook(
        &self,
        _ctx: &ToolContext,
        _tool_use_id: ToolUseId,
        query: &str,
        query_kind: ToolSearchQueryKind,
    ) -> Result<ToolSearchPreHookOutcome, ToolError> {
        assert_eq!(query, "select:Original");
        assert_eq!(query_kind, ToolSearchQueryKind::Select);
        self.hook_kinds
            .lock()
            .await
            .push(HookEventKind::PreToolSearch);
        self.events.lock().await.push(Event::HookTriggered(
            harness_contracts::HookTriggeredEvent {
                hook_event_kind: HookEventKind::PreToolSearch,
                handler_id: "tool-search-hook".to_owned(),
                outcome_summary: harness_contracts::HookOutcomeSummary {
                    continued: false,
                    blocked_reason: None,
                    rewrote_input: true,
                    overrode_permission: None,
                    added_context_bytes: None,
                    transformed: false,
                },
                duration_ms: 1,
                at: harness_contracts::now(),
            },
        ));
        Ok(ToolSearchPreHookOutcome::RewriteInput(json!({
            "query": "select:Rewritten"
        })))
    }

    async fn dispatch_post_tool_search_hook(
        &self,
        _ctx: &ToolContext,
        _tool_use_id: ToolUseId,
        materialized: Vec<harness_contracts::ToolName>,
        _backend: ToolLoadingBackendName,
        _cache_impact: harness_contracts::CacheImpact,
    ) -> Result<(), ToolError> {
        assert_eq!(materialized, vec!["Rewritten".to_owned()]);
        self.hook_kinds
            .lock()
            .await
            .push(HookEventKind::PostToolSearchMaterialize);
        self.events.lock().await.push(Event::HookTriggered(
            harness_contracts::HookTriggeredEvent {
                hook_event_kind: HookEventKind::PostToolSearchMaterialize,
                handler_id: "tool-search-hook".to_owned(),
                outcome_summary: harness_contracts::HookOutcomeSummary {
                    continued: true,
                    blocked_reason: None,
                    rewrote_input: false,
                    overrode_permission: None,
                    added_context_bytes: None,
                    transformed: false,
                },
                duration_ms: 1,
                at: harness_contracts::now(),
            },
        ));
        Ok(())
    }
}

struct AllowBroker;

#[async_trait]
impl PermissionBroker for AllowBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::AllowOnce
    }

    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Ok(())
    }
}

#![cfg(feature = "server-adapter")]

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    BudgetMetric, CapabilityRegistry, DeferPolicy, NetworkAccess, OverflowAction,
    ProviderRestriction, ResultBudget, SemverString, SessionId, TenantId, ToolActionPlan,
    ToolDescriptor, ToolError, ToolGroup, ToolOrigin, ToolProperties, ToolResult, ToolUseId,
    TrustLevel, WorkspaceAccess,
};
use harness_mcp::{
    IsolationMode, JsonRpcRequest, McpServerAdapter, McpServerAuditEvent, McpServerAuditSink,
    McpServerError, McpServerPolicy, McpServerRequestContext, StaticToolContextFactory,
    TenantIsolationPolicy, TenantMapping, TenantResolver,
};
use harness_tool::{
    action_plan_from_permission_check, AuthorizedToolInput, BuiltinToolset, InterruptToken,
    PermissionCheck, Tool, ToolContext, ToolEvent, ToolRegistry, ToolStream, ValidationError,
};
use serde_json::{json, Value};

#[tokio::test]
async fn strict_tenant_rejects_mismatch_before_execution() {
    let executions = Arc::new(AtomicUsize::new(0));
    let audit = Arc::new(RecordingAudit::default());
    let server = adapter_for_tenant(
        TenantId::SINGLE,
        executions.clone(),
        IsolationMode::StrictTenant,
        Some(Arc::clone(&audit)),
    );

    let response = server
        .handle_request_with_context(
            JsonRpcRequest::new(
                json!(1),
                "tools/call",
                Some(json!({ "name": "tenant_echo", "arguments": {} })),
            ),
            McpServerRequestContext::default().with_tenant_id(TenantId::SHARED),
        )
        .await;

    assert_eq!(response.error.expect("tenant error").code, -32603);
    assert_eq!(executions.load(Ordering::SeqCst), 0);
    assert!(audit
        .events
        .lock()
        .expect("events")
        .iter()
        .any(|event| matches!(event, McpServerAuditEvent::TenantIsolationRejected { .. })));
}

#[tokio::test]
async fn strict_tenant_allows_matching_tenant() {
    let executions = Arc::new(AtomicUsize::new(0));
    let server = adapter_for_tenant(
        TenantId::SINGLE,
        executions.clone(),
        IsolationMode::StrictTenant,
        None,
    );

    let response = server
        .handle_request_with_context(
            JsonRpcRequest::new(
                json!(2),
                "tools/call",
                Some(json!({ "name": "tenant_echo", "arguments": {} })),
            ),
            McpServerRequestContext::default().with_tenant_id(TenantId::SINGLE),
        )
        .await;

    assert!(response.error.is_none());
    assert_eq!(executions.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn single_tenant_mode_and_legacy_entrypoint_allow_calls() {
    let executions = Arc::new(AtomicUsize::new(0));
    let server = adapter_for_tenant(
        TenantId::SINGLE,
        executions.clone(),
        IsolationMode::SingleTenant,
        None,
    );

    let response = server
        .handle_request_with_context(
            JsonRpcRequest::new(
                json!(3),
                "tools/call",
                Some(json!({ "name": "tenant_echo", "arguments": {} })),
            ),
            McpServerRequestContext::default().with_tenant_id(TenantId::SHARED),
        )
        .await;
    assert!(response.error.is_none());

    let legacy = server
        .handle_request(JsonRpcRequest::new(
            json!(4),
            "tools/call",
            Some(json!({ "name": "tenant_echo", "arguments": {} })),
        ))
        .await;
    assert!(legacy.error.is_none());
    assert_eq!(executions.load(Ordering::SeqCst), 2);
}

fn adapter_for_tenant(
    tenant_id: TenantId,
    executions: Arc<AtomicUsize>,
    mode: IsolationMode,
    audit: Option<Arc<RecordingAudit>>,
) -> McpServerAdapter {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(TenantTool { executions }))
        .build()
        .expect("registry");
    let mut builder = McpServerAdapter::builder(registry)
        .with_policy(McpServerPolicy {
            tenant_mapping: TenantMapping::Custom(Arc::new(ContextTenantResolver)),
            tenant_isolation: TenantIsolationPolicy {
                mode,
                ..TenantIsolationPolicy::default()
            },
            ..McpServerPolicy::default()
        })
        .with_tool_context_factory(StaticToolContextFactory::new(tool_context(tenant_id)));
    if let Some(audit) = audit {
        builder = builder.with_audit_sink(audit);
    }
    builder.build().expect("server")
}

#[derive(Default)]
struct RecordingAudit {
    events: Mutex<Vec<McpServerAuditEvent>>,
}

impl McpServerAuditSink for RecordingAudit {
    fn record(&self, event: McpServerAuditEvent) {
        self.events.lock().expect("events").push(event);
    }
}

struct ContextTenantResolver;

#[async_trait]
impl TenantResolver for ContextTenantResolver {
    async fn resolve_tenant(
        &self,
        context: &McpServerRequestContext,
    ) -> Result<TenantId, McpServerError> {
        Ok(context.tenant_id)
    }
}

#[derive(Clone)]
struct TenantTool {
    executions: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for TenantTool {
    fn descriptor(&self) -> &ToolDescriptor {
        static DESCRIPTOR: std::sync::OnceLock<ToolDescriptor> = std::sync::OnceLock::new();
        DESCRIPTOR.get_or_init(|| ToolDescriptor {
            name: "tenant_echo".to_owned(),
            display_name: "tenant_echo".to_owned(),
            description: "tenant echo".to_owned(),
            category: "test".to_owned(),
            group: ToolGroup::Custom("test".to_owned()),
            version: SemverString::from("0.1.0"),
            input_schema: json!({ "type": "object" }),
            output_schema: None,
            dynamic_schema: false,
            properties: ToolProperties {
                is_concurrency_safe: true,
                is_read_only: true,
                is_destructive: false,
                long_running: None,
                defer_policy: DeferPolicy::AlwaysLoad,
            },
            trust_level: TrustLevel::AdminTrusted,
            required_capabilities: Vec::new(),
            budget: ResultBudget {
                metric: BudgetMetric::Chars,
                limit: 10_000,
                on_overflow: OverflowAction::Truncate,
                preview_head_chars: 1_000,
                preview_tail_chars: 200,
            },
            provider_restriction: ProviderRestriction::All,
            origin: ToolOrigin::Builtin,
            search_hint: None,
            service_binding: None,
        })
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            PermissionCheck::Allowed,
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
        self.executions.fetch_add(1, Ordering::SeqCst);
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Text("ok".to_owned()),
        )])))
    }
}

fn tool_context(tenant_id: TenantId) -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: SessionId::new(),
        tenant_id,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: std::path::PathBuf::from("."),
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

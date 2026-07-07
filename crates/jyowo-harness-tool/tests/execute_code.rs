#![cfg(feature = "programmatic-tool-calling")]

use std::sync::Arc;

use chrono::Utc;
use futures::{future::BoxFuture, StreamExt};
use harness_contracts::{
    CapabilityRegistry, CodeLanguage, CodeRunRequest, CodeRunResult, CodeRunStats, DecisionScope,
    EmbeddedToolDispatchRequest, EmbeddedToolDispatchResponse, Event, ExecuteCodeStepInvokedEvent,
    TenantId, ToolActionPlan, ToolCapability, ToolError, ToolGroup, ToolResult, ToolUseId,
};
use harness_tool::{
    builtin::ExecuteCodeTool, AuthorizedTicketSummary, AuthorizedToolInput, BuiltinToolset,
    InterruptToken, Tool, ToolContext, ToolEvent, ToolRegistry,
};
use serde_json::json;

#[test]
fn default_toolset_registers_execute_code_with_runtime_capabilities() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    let tool = registry
        .get("execute_code")
        .expect("execute_code should be registered");
    assert_eq!(tool.descriptor().group, ToolGroup::Shell);
    assert!(!tool.descriptor().properties.is_concurrency_safe);
    assert!(!tool.descriptor().properties.is_read_only);
    assert!(tool.descriptor().properties.is_destructive);
    assert_eq!(tool.descriptor().budget.limit, 256_000);
    assert_eq!(
        tool.descriptor().required_capabilities,
        vec![
            ToolCapability::CodeRuntime,
            ToolCapability::EmbeddedToolDispatcher
        ]
    );
}

#[tokio::test]
async fn execute_code_runs_runtime_and_streams_journal_events() {
    let mut caps = CapabilityRegistry::default();
    let runtime: Arc<dyn harness_contracts::CodeRuntimeCap> = Arc::new(FakeCodeRuntime);
    let dispatcher: Arc<dyn harness_contracts::EmbeddedToolDispatcherCap> =
        Arc::new(FakeEmbeddedDispatcher);
    caps.install(ToolCapability::CodeRuntime, runtime);
    caps.install(ToolCapability::EmbeddedToolDispatcher, dispatcher);

    let tool = ExecuteCodeTool::default();
    let input = json!({ "language": "mini_lua", "source": "return 1 + 2" });
    let ctx = tool_ctx(caps);
    let plan = tool.plan(&input, &ctx).await.unwrap();
    assert!(matches!(
        plan.scope,
        DecisionScope::ExecuteCodeScript { .. }
    ));

    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).unwrap();
    let mut stream = tool.execute_authorized(authorized, ctx).await.unwrap();
    assert!(matches!(
        stream.next().await,
        Some(ToolEvent::Journal(Event::ExecuteCodeStepInvoked(_)))
    ));
    let Some(ToolEvent::Final(ToolResult::Structured(value))) = stream.next().await else {
        panic!("expected final structured result");
    };
    assert_eq!(value["value"], json!(3));
    assert_eq!(value["embedded_steps"][0]["tool_name"], "ListDir");
    assert_eq!(value["embedded_steps"][0]["duration_ms"], 1);
}

#[tokio::test]
async fn execute_code_rejects_raw_source_when_authorized_script_hash_differs() {
    let mut caps = CapabilityRegistry::default();
    let runtime: Arc<dyn harness_contracts::CodeRuntimeCap> = Arc::new(FakeCodeRuntime);
    let dispatcher: Arc<dyn harness_contracts::EmbeddedToolDispatcherCap> =
        Arc::new(FakeEmbeddedDispatcher);
    caps.install(ToolCapability::CodeRuntime, runtime);
    caps.install(ToolCapability::EmbeddedToolDispatcher, dispatcher);

    let tool = ExecuteCodeTool::default();
    let planned_input = json!({ "language": "mini_lua", "source": "return 1" });
    let raw_input = json!({ "language": "mini_lua", "source": "return 2" });
    let ctx = tool_ctx(caps);
    let plan = tool.plan(&planned_input, &ctx).await.unwrap();
    let authorized = AuthorizedToolInput::new(raw_input, plan.clone(), ticket_for(&plan)).unwrap();

    let error = match tool.execute_authorized(authorized, ctx).await {
        Ok(_) => panic!("expected authorized execution to fail"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        ToolError::PermissionDenied(ref message)
            if message == "authorized execute_code script hash mismatch"
    ));
}

#[tokio::test]
async fn execute_code_denies_subagent_callers_before_runtime_execution() {
    let tool = ExecuteCodeTool::default();
    let input = json!({ "language": "mini_lua", "source": "return 1 + 2" });
    let mut ctx = tool_ctx(CapabilityRegistry::default());
    ctx.subagent_depth = 1;

    let error = tool.plan(&input, &ctx).await.unwrap_err();

    assert!(matches!(
        error,
        ToolError::PermissionDenied(ref reason)
            if reason == "execute_code is not available from subagents"
    ));
}

struct FakeCodeRuntime;

impl harness_contracts::CodeRuntimeCap for FakeCodeRuntime {
    fn run_code(
        &self,
        request: CodeRunRequest,
        dispatcher: Arc<dyn harness_contracts::EmbeddedToolDispatcherCap>,
    ) -> BoxFuture<'static, Result<CodeRunResult, harness_contracts::CodeRunError>> {
        Box::pin(async move {
            assert_eq!(request.language, CodeLanguage::MiniLua);
            let step = dispatcher
                .dispatch_embedded(EmbeddedToolDispatchRequest {
                    tenant_id: request.tenant_id,
                    session_id: request.session_id,
                    run_id: request.run_id,
                    parent_tool_use_id: request.tool_use_id,
                    tool_name: "ListDir".to_owned(),
                    input: json!({ "path": "." }),
                })
                .await
                .map_err(harness_contracts::CodeRunError::from)?;
            Ok(CodeRunResult {
                value: json!(3),
                stats: CodeRunStats {
                    instructions: 4,
                    embedded_call_count: 1,
                },
                embedded_steps: vec![step.clone()],
                events: vec![Event::ExecuteCodeStepInvoked(ExecuteCodeStepInvokedEvent {
                    parent_tool_use_id: request.tool_use_id,
                    run_id: request.run_id,
                    session_id: request.session_id,
                    embedded_tool: "ListDir".to_owned(),
                    args_hash: [1; 32],
                    step_seq: 1,
                    duration_ms: step.duration_ms,
                    overflow: None,
                    refused_reason: None,
                    at: harness_contracts::now(),
                })],
            })
        })
    }
}

struct FakeEmbeddedDispatcher;

impl harness_contracts::EmbeddedToolDispatcherCap for FakeEmbeddedDispatcher {
    fn dispatch_embedded(
        &self,
        request: EmbeddedToolDispatchRequest,
    ) -> BoxFuture<'static, Result<EmbeddedToolDispatchResponse, harness_contracts::ToolError>>
    {
        Box::pin(async move {
            Ok(EmbeddedToolDispatchResponse {
                tool_use_id: ToolUseId::new(),
                tool_name: request.tool_name,
                output: json!({ "ok": true }),
                duration_ms: 1,
                overflow: None,
            })
        })
    }
}

fn tool_ctx(cap_registry: CapabilityRegistry) -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: harness_contracts::SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: std::env::temp_dir(),
        sandbox: None,
        cap_registry: Arc::new(cap_registry),
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::default(),
        parent_run: None,
        model: None,
        model_config_id: None,
    }
}

fn ticket_for(plan: &ToolActionPlan) -> AuthorizedTicketSummary {
    {
        let ledger = harness_tool::TicketLedger::default();
        let claims = harness_tool::AuthorizationTicketClaims {
            tenant_id: harness_contracts::TenantId::SINGLE,
            session_id: harness_contracts::SessionId::new(),
            run_id: harness_contracts::RunId::new(),
            tool_use_id: plan.tool_use_id,
            tool_name: plan.tool_name.clone(),
            action_plan_hash: plan.plan_hash.clone(),
        };
        let ticket = ledger
            .mint(claims.clone(), chrono::Utc::now())
            .expect("test ticket should mint");
        ledger
            .consume(ticket.id, &claims, chrono::Utc::now())
            .expect("test ticket should consume")
    }
}

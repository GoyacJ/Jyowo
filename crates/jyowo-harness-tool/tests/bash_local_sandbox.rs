#![cfg(all(feature = "builtin-toolset", unix))]

use std::sync::Arc;

use futures::StreamExt;
use harness_contracts::{
    AgentId, CapabilityRegistry, CorrelationId, MessagePart, PermissionActorSource, TenantId,
    ToolResult, ToolUseId,
};
use harness_sandbox::{LocalIsolation, LocalSandbox};
use harness_tool::{
    builtin::BashTool, AuthorizationTicketClaims, InterruptToken, TicketLedger, Tool, ToolContext,
};
use serde_json::json;

#[tokio::test]
async fn bash_executes_compound_script_through_local_sandbox() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("marker.txt"), "present").unwrap();
    let workspace_root = std::fs::canonicalize(workspace.path()).unwrap();
    let isolation = LocalIsolation::for_current_platform();
    let sandbox = Arc::new(LocalSandbox::new(&workspace_root).with_isolation(isolation));
    let tool = BashTool::default();
    let ctx = tool_context(workspace_root.clone(), sandbox);
    let input = json!({
        "command": "printf 'cwd=%s\\n' \"$PWD\" && test -f marker.txt && printf 'found-marker\\n'"
    });
    let plan = tool.plan(&input, &ctx).await.unwrap();
    let ticket = authorized_ticket(&ctx, &plan);
    let authorized = harness_tool::AuthorizedToolInput::new(input, plan, ticket).unwrap();
    let mut stream = tool.execute_authorized(authorized, ctx).await.unwrap();
    let mut output = String::new();
    let mut result = None;

    while let Some(event) = stream.next().await {
        match event {
            harness_tool::ToolEvent::Partial(MessagePart::Text(text)) => output.push_str(&text),
            harness_tool::ToolEvent::Final(final_result) => result = Some(final_result),
            _ => {}
        }
    }

    assert!(output.contains(&format!("cwd={}", workspace_root.display())));
    assert!(output.contains("found-marker"));
    let Some(ToolResult::Structured(result)) = result else {
        panic!("expected structured Bash result");
    };
    assert_eq!(result["success"], true, "result={result}, output={output}");
    assert_eq!(result["exit_status"], json!({ "code": 0 }));
}

fn tool_context(workspace_root: std::path::PathBuf, sandbox: Arc<LocalSandbox>) -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: harness_contracts::SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: CorrelationId::new(),
        agent_id: AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root,
        project_workspace_root: None,
        sandbox: Some(sandbox),
        cap_registry: Arc::new(CapabilityRegistry::default()),
        redactor: Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::default(),
        parent_run: None,
        model: None,
        model_config_id: None,
        memory_thread_settings: None,
        actor_source: PermissionActorSource::ParentRun,
    }
}

fn authorized_ticket(
    ctx: &ToolContext,
    plan: &harness_contracts::ToolActionPlan,
) -> harness_tool::AuthorizedTicketSummary {
    let ledger = TicketLedger::default();
    let claims = AuthorizationTicketClaims {
        tenant_id: ctx.tenant_id,
        session_id: ctx.session_id,
        run_id: ctx.run_id,
        tool_use_id: plan.tool_use_id,
        tool_name: plan.tool_name.clone(),
        action_plan_hash: plan.plan_hash.clone(),
    };
    let ticket = ledger.mint(claims.clone(), chrono::Utc::now()).unwrap();
    ledger
        .consume(ticket.id, &claims, chrono::Utc::now())
        .unwrap()
}

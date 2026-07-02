#![cfg(feature = "builtin-toolset")]

use std::{path::Path, sync::Arc};

use chrono::Utc;
use futures::StreamExt;
use harness_contracts::{
    CapabilityRegistry, DecisionScope, TenantId, ToolActionPlan, ToolCapability, ToolError,
    ToolGroup, ToolResult, ToolUseId,
};
use harness_tool::{
    builtin::{FileEditTool, GlobTool, TaskStopTool, TodoTool, WebFetchTool},
    AuthorizedTicketSummary, AuthorizedToolInput, BuiltinToolset, InterruptToken, Tool,
    ToolContext, ToolRegistry,
};
use serde_json::{json, Value};

#[test]
fn default_builtin_toolset_registers_architecture_m0_tools() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    for name in [
        "FileEdit",
        "Glob",
        "WebFetch",
        "Diagnostics",
        "Todo",
        "TaskStop",
        "FileRead",
        "FileWrite",
        "ListDir",
        "Grep",
        "ReadBlob",
        "WebSearch",
        "ProcessStart",
        "ProcessRead",
        "ProcessStop",
        "Clarify",
        "SendMessage",
    ] {
        assert!(registry.get(name).is_some(), "{name} should be registered");
    }
}

#[tokio::test]
async fn missing_capability_tools_fail_closed() {
    assert!(matches!(
        execute_error(
            &TodoTool::default(),
            json!({ "items": [{"content": "review", "status": "pending"}] }),
            tool_ctx(CapabilityRegistry::default()),
        )
        .await,
        ToolError::CapabilityMissing(ToolCapability::TodoStore)
    ));

    assert!(matches!(
        execute_error(
            &TaskStopTool::default(),
            json!({ "reason": "done" }),
            tool_ctx(CapabilityRegistry::default()),
        )
        .await,
        ToolError::CapabilityMissing(ToolCapability::RunCanceller)
    ));
}

#[tokio::test]
async fn file_edit_replaces_text_and_asks_for_path_permission() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("note.txt");
    std::fs::write(&file, "alpha\nbeta\n").unwrap();
    let tool = FileEditTool::default();

    let input = json!({
        "path": file,
        "old": "beta",
        "new": "gamma"
    });
    let plan = tool
        .plan(&input, &tool_ctx(CapabilityRegistry::default()))
        .await;
    assert!(matches!(plan.unwrap().scope, DecisionScope::PathPrefix(_)));

    let result = execute_final(
        &tool,
        input,
        tool_ctx_at(dir.path(), CapabilityRegistry::default()),
    )
    .await;

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\ngamma\n");
    let ToolResult::Structured(value) = result else {
        panic!("expected structured edit result");
    };
    assert_eq!(value["replacements"], 1);
    assert!(value["path"].as_str().unwrap().ends_with("note.txt"));
}

#[tokio::test]
async fn glob_returns_stable_workspace_matches() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/a.rs"), "").unwrap();
    std::fs::write(dir.path().join("src/b.txt"), "").unwrap();
    std::fs::write(dir.path().join("root.rs"), "").unwrap();
    let tool = GlobTool::default();

    let result = execute_final(
        &tool,
        json!({ "path": dir.path(), "pattern": "**/*.rs" }),
        tool_ctx_at(dir.path(), CapabilityRegistry::default()),
    )
    .await;

    let ToolResult::Structured(value) = result else {
        panic!("expected structured glob result");
    };
    let matches = value.as_array().unwrap();
    assert_eq!(matches.len(), 2);
    assert!(matches[0]["path"].as_str().unwrap().ends_with("root.rs"));
    assert!(matches[1]["path"].as_str().unwrap().ends_with("src/a.rs"));
}

#[cfg(unix)]
#[tokio::test]
async fn file_edit_and_glob_reject_workspace_escape_paths_before_fs_access() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let outside_file = root.path().join("outside.txt");
    let outside_dir = root.path().join("outside_dir");
    std::fs::create_dir(&outside_dir).unwrap();
    std::fs::write(&outside_file, "secret").unwrap();
    std::fs::write(outside_dir.join("secret.rs"), "fn secret() {}").unwrap();
    std::os::unix::fs::symlink(&outside_file, workspace.join("link.txt")).unwrap();
    std::os::unix::fs::symlink(&outside_dir, workspace.join("linked_dir")).unwrap();
    let ctx = || tool_ctx_at(&workspace, CapabilityRegistry::default());

    assert!(matches!(
        execute_error(
            &FileEditTool::default(),
            json!({ "path": "../outside.txt", "old": "secret", "new": "changed" }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
    assert!(matches!(
        execute_error(
            &FileEditTool::default(),
            json!({ "path": "link.txt", "old": "secret", "new": "changed" }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
    assert_eq!(std::fs::read_to_string(&outside_file).unwrap(), "secret");
    assert!(matches!(
        execute_error(
            &GlobTool::default(),
            json!({ "path": outside_dir, "pattern": "**/*.rs" }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
    assert!(matches!(
        execute_error(
            &GlobTool::default(),
            json!({ "path": "linked_dir", "pattern": "**/*.rs" }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
}

#[test]
fn descriptors_match_architecture_groups_and_capabilities() {
    let edit = FileEditTool::default();
    assert_eq!(edit.descriptor().group, ToolGroup::FileSystem);
    assert!(edit.descriptor().properties.is_destructive);

    let glob = GlobTool::default();
    assert_eq!(glob.descriptor().group, ToolGroup::Search);
    assert!(glob.descriptor().properties.is_read_only);

    let fetch = WebFetchTool::default();
    assert_eq!(fetch.descriptor().group, ToolGroup::Network);
    assert!(fetch.descriptor().properties.is_read_only);

    let todo = TodoTool::default();
    assert_eq!(
        todo.descriptor().required_capabilities,
        vec![ToolCapability::TodoStore]
    );

    let stop = TaskStopTool::default();
    assert_eq!(
        stop.descriptor().required_capabilities,
        vec![ToolCapability::RunCanceller]
    );
}

async fn execute_final(tool: &dyn Tool, input: Value, ctx: ToolContext) -> ToolResult {
    tool.validate(&input, &ctx).await.unwrap();
    let plan = tool.plan(&input, &ctx).await.unwrap();
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).unwrap();
    let mut stream = tool.execute_authorized(authorized, ctx).await.unwrap();
    match stream.next().await {
        Some(harness_tool::ToolEvent::Final(result)) => result,
        other => panic!("expected final result, got {other:?}"),
    }
}

async fn execute_error(tool: &dyn Tool, input: Value, ctx: ToolContext) -> ToolError {
    tool.validate(&input, &ctx).await.unwrap();
    let plan = match tool.plan(&input, &ctx).await {
        Ok(plan) => plan,
        Err(error) => return error,
    };
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).unwrap();
    match tool.execute_authorized(authorized, ctx).await {
        Ok(_) => panic!("expected tool error"),
        Err(error) => error,
    }
}

fn tool_ctx(cap_registry: CapabilityRegistry) -> ToolContext {
    tool_ctx_at(std::env::temp_dir(), cap_registry)
}

fn tool_ctx_at(workspace_root: impl AsRef<Path>, cap_registry: CapabilityRegistry) -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: harness_contracts::SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: workspace_root.as_ref().to_path_buf(),
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
    AuthorizedTicketSummary {
        ticket_id: harness_contracts::AuthorizationTicketId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: harness_contracts::SessionId::new(),
        run_id: harness_contracts::RunId::new(),
        tool_use_id: plan.tool_use_id,
        tool_name: plan.tool_name.clone(),
        action_plan_hash: plan.plan_hash.clone(),
        consumed_at: Utc::now(),
    }
}

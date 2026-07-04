//! Tests for the MemoryTool.

#![cfg(feature = "builtin-toolset")]

use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::*;
use harness_tool::{
    builtin::{
        memory_tool_runtime_capability, MemoryTool, MemoryToolRuntimeCap, MemoryToolRuntimeRequest,
    },
    AuthorizedTicketSummary, AuthorizedToolInput, BuiltinToolset, InterruptToken, Tool,
    ToolContext, ToolRegistry,
};
use serde_json::{json, Value};

fn make_tool() -> MemoryTool {
    MemoryTool::default()
}

#[test]
fn tool_descriptor_exposes_memory_tool_args_schema() {
    let tool = make_tool();
    let desc = tool.descriptor();
    assert_eq!(desc.name, "memory");
    assert_eq!(desc.display_name, "Memory");
    // Tool belongs to Memory group
    assert!(matches!(desc.group, ToolGroup::Memory));
}

#[test]
fn tool_descriptor_has_all_seven_actions() {
    let tool = make_tool();
    let desc = tool.descriptor();
    let schema_str = serde_json::to_string(&desc.input_schema).unwrap();

    // All 7 actions should be present
    assert!(schema_str.contains("search"));
    assert!(schema_str.contains("read"));
    assert!(schema_str.contains("create"));
    assert!(schema_str.contains("update"));
    assert!(schema_str.contains("delete"));
    assert!(schema_str.contains("list"));
    assert!(schema_str.contains("propose"));
}

#[test]
fn tool_args_parse_search_action() {
    let input = json!({
        "action": "search",
        "query": "rust programming",
        "max_records": 5
    });
    // Validate the flat model-facing format
    assert_eq!(input["action"], "search");
    assert_eq!(input["query"], "rust programming");
    assert_eq!(input["max_records"], 5);
}

#[test]
fn tool_args_parse_create_action() {
    let input = json!({
        "action": "create",
        "draft": {
            "kind": "project_fact",
            "visibility": "user",
            "content": "Rust is a systems programming language"
        }
    });
    assert_eq!(input["action"], "create");
    assert_eq!(input["draft"]["kind"], "project_fact");
    assert_eq!(
        input["draft"]["content"],
        "Rust is a systems programming language"
    );
}

#[test]
fn tool_args_parse_delete_action() {
    let input = json!({
        "action": "delete",
        "memory_id": "01J00000000000000000000000",
        "reason": "outdated information"
    });
    assert_eq!(input["action"], "delete");
    assert_eq!(input["reason"], "outdated information");
}

#[test]
fn tool_args_parse_propose_action() {
    let input = json!({
        "action": "propose",
        "draft": {
            "kind": "reference",
            "visibility": "tenant",
            "content": "Candidate memory entry"
        }
    });
    assert_eq!(input["action"], "propose");
    assert_eq!(input["draft"]["content"], "Candidate memory entry");
}

#[test]
fn tool_args_parse_list_action() {
    let input = json!({
        "action": "list",
        "limit": 10
    });
    assert_eq!(input["action"], "list");
    assert_eq!(input["limit"], 10);
}

#[test]
fn model_cannot_provide_runtime_context_fields() {
    // The model-visible schema (MemoryToolArgs) should not expose
    // runtime context fields like tenant_id, session_id, run_id, etc.
    let schema = make_tool().descriptor().input_schema.clone();
    let schema_str = serde_json::to_string(&schema).unwrap();

    assert!(!schema_str.contains("tenant_id"));
    assert!(!schema_str.contains("session_id"));
    assert!(!schema_str.contains("run_id"));
    assert!(!schema_str.contains("permission_context"));
    assert!(!schema_str.contains("authorization_ticket"));
    assert!(!schema_str.contains("non_interactive_policy"));
}

#[test]
fn default_toolset_registers_memory_tool_with_runtime_capability() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    let tool = registry.get("memory").expect("memory should be registered");
    assert_eq!(
        tool.descriptor().required_capabilities,
        vec![memory_tool_runtime_capability()]
    );
}

#[tokio::test]
async fn memory_tool_fails_closed_without_runtime_capability() {
    let result = execute_final(
        &MemoryTool::default(),
        json!({ "action": "search", "query": "rust", "max_records": 2 }),
        tool_ctx(CapabilityRegistry::default()),
    )
    .await;

    let ToolResult::Structured(value) = result else {
        panic!("expected structured denial");
    };
    assert_eq!(value["state"], "denied");
    assert!(value["error"]
        .as_str()
        .unwrap()
        .contains("jyowo.memory.tool_runtime"));
}

#[tokio::test]
async fn memory_tool_delegates_read_actions_to_runtime() {
    let runtime = Arc::new(FakeMemoryRuntime::default());
    let ctx = tool_ctx(memory_caps(runtime.clone()));

    let search = execute_structured(
        &MemoryTool::default(),
        json!({ "action": "search", "query": "rust", "max_records": 2 }),
        ctx.clone(),
    )
    .await;
    assert!(search["records"][0].get("content").is_none());
    assert_eq!(
        search["records"][0]["content_preview"],
        "[redacted memory content]"
    );
    assert_eq!(search["memory_ids"], json!(["mem-search"]));

    let read = execute_structured(
        &MemoryTool::default(),
        json!({ "action": "read", "memory_id": "mem-read" }),
        ctx.clone(),
    )
    .await;
    assert_eq!(read["record"]["id"], "mem-read");
    assert!(read["record"].get("content").is_none());
    assert_eq!(
        read["record"]["content_preview"],
        "[redacted memory content]"
    );

    let list = execute_structured(
        &MemoryTool::default(),
        json!({ "action": "list", "limit": 3 }),
        ctx,
    )
    .await;
    assert!(list["records"][0].get("content").is_none());
    assert_eq!(
        list["records"][0]["content_preview"],
        "[redacted memory content]"
    );

    let calls = runtime.calls.lock().unwrap();
    assert!(calls.iter().any(|call| call == "search:rust:2"));
    assert!(calls.iter().any(|call| call == "read:mem-read"));
    assert!(calls.iter().any(|call| call == "list:3"));
}

#[tokio::test]
async fn memory_tool_delegates_write_actions_to_runtime() {
    let runtime = Arc::new(FakeMemoryRuntime::default());
    let ctx = tool_ctx(memory_caps(runtime.clone()));
    let draft = json!({
        "kind": "project_fact",
        "visibility": "tenant",
        "content": "Memory tool uses runtime storage."
    });

    let create = execute_structured(
        &MemoryTool::default(),
        json!({ "action": "create", "draft": draft.clone() }),
        ctx.clone(),
    )
    .await;
    assert_eq!(create["state"], "created");
    assert_eq!(create["memory_id"], "mem-create");

    let update = execute_structured(
        &MemoryTool::default(),
        json!({ "action": "update", "memory_id": "mem-update", "draft": draft.clone() }),
        ctx.clone(),
    )
    .await;
    assert_eq!(update["state"], "updated");
    assert_eq!(update["memory_id"], "mem-update");

    let delete = execute_structured(
        &MemoryTool::default(),
        json!({ "action": "delete", "memory_id": "mem-delete", "reason": "stale" }),
        ctx.clone(),
    )
    .await;
    assert_eq!(delete["state"], "forgotten");
    assert_eq!(delete["memory_id"], "mem-delete");

    let propose = execute_structured(
        &MemoryTool::default(),
        json!({ "action": "propose", "draft": draft }),
        ctx,
    )
    .await;
    assert_eq!(propose["state"], "candidate_created");
    assert_eq!(propose["candidate_id"], "candidate-propose");

    let calls = runtime.calls.lock().unwrap();
    assert!(calls.iter().any(|call| call == "create"));
    assert!(calls.iter().any(|call| call == "update:mem-update"));
    assert!(calls.iter().any(|call| call == "delete:mem-delete:stale"));
    assert!(calls.iter().any(|call| call == "propose"));
}

#[tokio::test]
async fn memory_tool_requires_update_id_and_draft_visibility() {
    let tool = MemoryTool::default();
    let ctx = tool_ctx(CapabilityRegistry::default());

    let missing_id = tool
        .validate(
            &json!({
                "action": "update",
                "draft": {
                    "kind": "project_fact",
                    "visibility": "tenant",
                    "content": "content"
                }
            }),
            &ctx,
        )
        .await
        .unwrap_err();
    assert!(missing_id.to_string().contains("memory_id is required"));

    let missing_visibility = tool
        .validate(
            &json!({
                "action": "propose",
                "draft": {
                    "kind": "project_fact",
                    "content": "content"
                }
            }),
            &ctx,
        )
        .await
        .unwrap_err();
    assert!(missing_visibility
        .to_string()
        .contains("visibility is required"));
}

#[derive(Default)]
struct FakeMemoryRuntime {
    calls: Mutex<Vec<String>>,
}

#[async_trait]
impl MemoryToolRuntimeCap for FakeMemoryRuntime {
    async fn execute(&self, request: MemoryToolRuntimeRequest) -> Result<Value, ToolError> {
        assert_eq!(request.tenant_id, TenantId::SINGLE);
        assert!(request.session_id.to_string().len() > 10);
        assert!(request.run_id.to_string().len() > 10);
        match request.action.as_str() {
            "search" => {
                let query = request.input["query"].as_str().unwrap();
                let max_records = request.input["max_records"].as_u64().unwrap();
                self.calls
                    .lock()
                    .unwrap()
                    .push(format!("search:{query}:{max_records}"));
                Ok(json!({
                    "action": "search",
                    "state": "completed",
                    "query": query,
                    "max_records": max_records,
                    "records": [{"id": "mem-search", "content": "runtime search result"}],
                    "memory_ids": ["mem-search"]
                }))
            }
            "read" => {
                let memory_id = request.input["memory_id"].as_str().unwrap();
                self.calls.lock().unwrap().push(format!("read:{memory_id}"));
                Ok(json!({
                    "action": "read",
                    "state": "completed",
                    "memory_id": memory_id,
                    "record": {"id": memory_id, "content": "runtime read result"}
                }))
            }
            "list" => {
                let limit = request.input["limit"].as_u64().unwrap();
                self.calls.lock().unwrap().push(format!("list:{limit}"));
                Ok(json!({
                    "action": "list",
                    "state": "completed",
                    "limit": limit,
                    "records": [{"id": "mem-list", "content": "runtime list result"}]
                }))
            }
            "create" => {
                self.calls.lock().unwrap().push("create".to_owned());
                Ok(json!({
                    "action": "create",
                    "state": "created",
                    "memory_id": "mem-create"
                }))
            }
            "update" => {
                let memory_id = request.input["memory_id"].as_str().unwrap();
                self.calls
                    .lock()
                    .unwrap()
                    .push(format!("update:{memory_id}"));
                Ok(json!({
                    "action": "update",
                    "state": "updated",
                    "memory_id": memory_id
                }))
            }
            "delete" => {
                let memory_id = request.input["memory_id"].as_str().unwrap();
                let reason = request.input["reason"].as_str().unwrap();
                self.calls
                    .lock()
                    .unwrap()
                    .push(format!("delete:{memory_id}:{reason}"));
                Ok(json!({
                    "action": "delete",
                    "state": "forgotten",
                    "memory_id": memory_id
                }))
            }
            "propose" => {
                self.calls.lock().unwrap().push("propose".to_owned());
                Ok(json!({
                    "action": "propose",
                    "state": "candidate_created",
                    "candidate_id": "candidate-propose"
                }))
            }
            _ => unreachable!(),
        }
    }
}

fn memory_caps(runtime: Arc<FakeMemoryRuntime>) -> CapabilityRegistry {
    let mut caps = CapabilityRegistry::default();
    let runtime: Arc<dyn MemoryToolRuntimeCap> = runtime;
    caps.install(memory_tool_runtime_capability(), runtime);
    caps
}

async fn execute_structured(tool: &dyn Tool, input: Value, ctx: ToolContext) -> Value {
    let ToolResult::Structured(value) = execute_final(tool, input, ctx).await else {
        panic!("expected structured result");
    };
    value
}

async fn execute_final(tool: &dyn Tool, input: Value, ctx: ToolContext) -> ToolResult {
    tool.validate(&input, &ctx).await.unwrap();
    let plan = tool.plan(&input, &ctx).await.unwrap();
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).unwrap();
    let mut stream = tool.execute_authorized(authorized, ctx).await.unwrap();
    while let Some(event) = stream.next().await {
        if let harness_tool::ToolEvent::Final(result) = event {
            return result;
        }
    }
    panic!("expected final result");
}

fn tool_ctx(cap_registry: CapabilityRegistry) -> ToolContext {
    tool_ctx_at(std::env::temp_dir(), cap_registry)
}

fn tool_ctx_at(workspace_root: impl AsRef<Path>, cap_registry: CapabilityRegistry) -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: RunId::new(),
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: CorrelationId::new(),
        agent_id: AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: workspace_root.as_ref().to_path_buf(),
        sandbox: None,
        cap_registry: Arc::new(cap_registry),
        redactor: Arc::new(NoopRedactor),
        interrupt: InterruptToken::default(),
        parent_run: None,
        model: None,
        model_config_id: None,
        actor_source: PermissionActorSource::ParentRun,
    }
}

struct NoopRedactor;

impl Redactor for NoopRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.to_owned()
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
        consumed_at: chrono::Utc::now(),
    }
}

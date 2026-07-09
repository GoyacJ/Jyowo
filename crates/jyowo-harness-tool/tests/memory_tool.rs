//! Tests for the MemoryTool.

#![cfg(feature = "builtin-toolset")]

use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::*;
use harness_tool::{
    builtin::{
        memory_tool_runtime_capability, MemoryTool, MemoryToolRuntimeAction, MemoryToolRuntimeCap,
        MemoryToolRuntimeRequest,
    },
    AuthorizedTicketSummary, AuthorizedToolInput, BuiltinToolset, InterruptToken, Tool,
    ToolContext, ToolRegistry,
};
use serde_json::{json, Value};

const MEM_READ: &str = "01J00000000000000000000000";
const MEM_UPDATE: &str = "01J00000000000000000000001";
const MEM_DELETE: &str = "01J00000000000000000000002";

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
    let mut expected = serde_json::to_value(schemars::schema_for!(MemoryToolArgs)).unwrap();
    expected
        .as_object_mut()
        .unwrap()
        .insert("additionalProperties".to_owned(), json!(false));
    assert_eq!(desc.input_schema, expected);
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
    assert_eq!(value["action"], "search");
    assert_eq!(value["state"]["denied"]["reason"], "missing_policy");
    assert_eq!(value["denial"]["reason"], "missing_policy");
    assert!(value["denial"]["safe_message"]
        .as_str()
        .unwrap()
        .contains("jyowo.memory.tool_runtime"));
    assert_eq!(value["takes_effect"], "never");
    assert_eq!(value["memory_ids"], json!([]));
    assert_eq!(value["candidate_ids"], json!([]));
    assert_eq!(value["records"], json!([]));
}

#[tokio::test]
async fn memory_tool_plans_read_actions_as_policy_auto_and_writes_as_user_review() {
    let tool = MemoryTool::default();
    let ctx = tool_ctx(CapabilityRegistry::default());

    for input in [
        json!({ "action": "search", "query": "rust", "max_records": 2 }),
        json!({ "action": "read", "memory_id": MEM_READ }),
        json!({ "action": "list", "limit": 3 }),
    ] {
        tool.validate(&input, &ctx).await.unwrap();
        let plan = tool.plan(&input, &ctx).await.unwrap();
        assert_eq!(plan.severity, Severity::Info);
    }

    let draft = json!({
        "kind": "project_fact",
        "visibility": "tenant",
        "content": "Memory tool uses runtime storage."
    });
    for input in [
        json!({ "action": "create", "draft": draft.clone() }),
        json!({ "action": "update", "memory_id": MEM_UPDATE, "draft": draft.clone() }),
        json!({ "action": "delete", "memory_id": MEM_DELETE, "reason": "stale" }),
        json!({ "action": "propose", "draft": draft }),
    ] {
        tool.validate(&input, &ctx).await.unwrap();
        let plan = tool.plan(&input, &ctx).await.unwrap();
        assert_eq!(plan.severity, Severity::Medium);
    }
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
        search["records"][0]["redacted_content"],
        "[redacted memory content]"
    );
    assert_eq!(search["memory_ids"].as_array().unwrap().len(), 1);

    let read = execute_structured(
        &MemoryTool::default(),
        json!({ "action": "read", "memory_id": MEM_READ }),
        ctx.clone(),
    )
    .await;
    assert_eq!(read["memory_ids"], json!([MEM_READ]));
    assert!(read["records"][0].get("content").is_none());
    assert_eq!(
        read["records"][0]["redacted_content"],
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
        list["records"][0]["redacted_content"],
        "[redacted memory content]"
    );

    let calls = runtime.calls.lock().unwrap();
    assert!(calls.iter().any(|call| call == "search:rust:2"));
    assert!(calls.iter().any(|call| call == &format!("read:{MEM_READ}")));
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
    assert_eq!(create["state"], "completed");
    assert_eq!(create["memory_ids"].as_array().unwrap().len(), 1);

    let update = execute_structured(
        &MemoryTool::default(),
        json!({ "action": "update", "memory_id": MEM_UPDATE, "draft": draft.clone() }),
        ctx.clone(),
    )
    .await;
    assert_eq!(update["state"], "completed");
    assert_eq!(update["memory_ids"], json!([MEM_UPDATE]));

    let delete = execute_structured(
        &MemoryTool::default(),
        json!({ "action": "delete", "memory_id": MEM_DELETE, "reason": "stale" }),
        ctx.clone(),
    )
    .await;
    assert_eq!(delete["state"], "completed");
    assert_eq!(delete["memory_ids"], json!([MEM_DELETE]));

    let propose = execute_structured(
        &MemoryTool::default(),
        json!({ "action": "propose", "draft": draft }),
        ctx,
    )
    .await;
    assert_eq!(propose["state"], "candidate_created");
    assert_eq!(propose["candidate_ids"].as_array().unwrap().len(), 1);

    let calls = runtime.calls.lock().unwrap();
    assert!(calls.iter().any(|call| call == "create"));
    assert!(calls
        .iter()
        .any(|call| call == &format!("update:{MEM_UPDATE}")));
    assert!(calls
        .iter()
        .any(|call| call == &format!("delete:{MEM_DELETE}:stale")));
    assert!(calls.iter().any(|call| call == "propose"));
}

#[tokio::test]
async fn memory_tool_passes_context_memory_thread_settings_to_runtime() {
    let runtime = Arc::new(FakeMemoryRuntime::default());
    let mut ctx = tool_ctx(memory_caps(runtime.clone()));
    ctx.memory_thread_settings = Some(MemoryThreadSettings {
        session_id: ctx.session_id,
        use_memories: None,
        generate_memories: None,
        memory_mode: MemoryThreadMode::ReadOnly,
    });

    execute_structured(
        &MemoryTool::default(),
        json!({
            "action": "propose",
            "draft": {
                "kind": "project_fact",
                "visibility": "tenant",
                "content": "candidate"
            }
        }),
        ctx,
    )
    .await;

    let modes = runtime.memory_modes.lock().unwrap();
    assert_eq!(modes.as_slice(), &[Some(MemoryThreadMode::ReadOnly)]);
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
    assert!(missing_id.to_string().contains("memory_id"));

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
    assert!(missing_visibility.to_string().contains("visibility"));
}

#[derive(Default)]
struct FakeMemoryRuntime {
    calls: Mutex<Vec<String>>,
    memory_modes: Mutex<Vec<Option<MemoryThreadMode>>>,
}

#[async_trait]
impl MemoryToolRuntimeCap for FakeMemoryRuntime {
    async fn execute(
        &self,
        request: MemoryToolRuntimeRequest,
    ) -> Result<MemoryToolResponse, ToolError> {
        assert_eq!(request.tenant_id, TenantId::SINGLE);
        assert_eq!(
            request.provider_policy,
            harness_contracts::MemoryProviderSelectionPolicy::PolicySelected
        );
        assert!(request.session_id.to_string().len() > 10);
        assert!(request.run_id.to_string().len() > 10);
        self.memory_modes.lock().unwrap().push(
            request
                .memory_thread_settings
                .as_ref()
                .map(|settings| settings.memory_mode.clone()),
        );
        match request.action {
            MemoryToolRuntimeAction::Search {
                query, max_records, ..
            } => {
                self.calls
                    .lock()
                    .unwrap()
                    .push(format!("search:{query}:{max_records}"));
                Ok(fake_response(
                    "search",
                    MemoryToolState::Completed,
                    vec![MemoryId::new()],
                    Vec::new(),
                    vec![fake_record(MemoryId::new())],
                    MemoryTakesEffect::CurrentTurn,
                ))
            }
            MemoryToolRuntimeAction::Read { memory_id } => {
                self.calls.lock().unwrap().push(format!("read:{memory_id}"));
                Ok(fake_response(
                    "read",
                    MemoryToolState::Completed,
                    vec![memory_id],
                    Vec::new(),
                    vec![fake_record(memory_id)],
                    MemoryTakesEffect::CurrentTurn,
                ))
            }
            MemoryToolRuntimeAction::List { limit, .. } => {
                self.calls.lock().unwrap().push(format!("list:{limit}"));
                let memory_id = MemoryId::new();
                Ok(fake_response(
                    "list",
                    MemoryToolState::Completed,
                    vec![memory_id],
                    Vec::new(),
                    vec![fake_record(memory_id)],
                    MemoryTakesEffect::CurrentTurn,
                ))
            }
            MemoryToolRuntimeAction::Create { .. } => {
                assert!(!request.permission_context.explicit_user_instruction);
                self.calls.lock().unwrap().push("create".to_owned());
                Ok(fake_response(
                    "create",
                    MemoryToolState::Completed,
                    vec![MemoryId::new()],
                    Vec::new(),
                    Vec::new(),
                    MemoryTakesEffect::NextTurn,
                ))
            }
            MemoryToolRuntimeAction::Update { memory_id, .. } => {
                assert!(!request.permission_context.explicit_user_instruction);
                self.calls
                    .lock()
                    .unwrap()
                    .push(format!("update:{memory_id}"));
                Ok(fake_response(
                    "update",
                    MemoryToolState::Completed,
                    vec![memory_id],
                    Vec::new(),
                    vec![fake_record(memory_id)],
                    MemoryTakesEffect::NextTurn,
                ))
            }
            MemoryToolRuntimeAction::Delete { memory_id, reason } => {
                assert!(!request.permission_context.explicit_user_instruction);
                self.calls
                    .lock()
                    .unwrap()
                    .push(format!("delete:{memory_id}:{reason}"));
                Ok(fake_response(
                    "delete",
                    MemoryToolState::Completed,
                    vec![memory_id],
                    Vec::new(),
                    Vec::new(),
                    MemoryTakesEffect::NextTurn,
                ))
            }
            MemoryToolRuntimeAction::Propose { .. } => {
                assert!(!request.permission_context.explicit_user_instruction);
                self.calls.lock().unwrap().push("propose".to_owned());
                Ok(fake_response(
                    "propose",
                    MemoryToolState::CandidateCreated,
                    Vec::new(),
                    vec![MemoryCandidateId::new()],
                    Vec::new(),
                    MemoryTakesEffect::Never,
                ))
            }
        }
    }
}

fn fake_response(
    action: &str,
    state: MemoryToolState,
    memory_ids: Vec<MemoryId>,
    candidate_ids: Vec<MemoryCandidateId>,
    records: Vec<MemoryToolRecordView>,
    takes_effect: MemoryTakesEffect,
) -> MemoryToolResponse {
    MemoryToolResponse {
        action: action.to_owned(),
        state,
        memory_ids,
        candidate_ids,
        records,
        next_cursor: None,
        action_plan_id: None,
        denial: None,
        redaction: MemoryRedactionSummary {
            redacted_count: 1,
            dropped_count: 0,
        },
        trace_id: None,
        takes_effect,
    }
}

fn fake_record(memory_id: MemoryId) -> MemoryToolRecordView {
    MemoryToolRecordView {
        memory_id,
        provider_id: "fake".to_owned(),
        kind: MemoryKind::ProjectFact,
        visibility: MemoryVisibility::Tenant,
        redacted_content: Some("[redacted memory content]".to_owned()),
        content_hash: ContentHash([7u8; 32]),
        score: None,
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
        project_workspace_root: None,
        sandbox: None,
        cap_registry: Arc::new(cap_registry),
        redactor: Arc::new(NoopRedactor),
        interrupt: InterruptToken::default(),
        parent_run: None,
        model: None,
        model_config_id: None,
        memory_thread_settings: None,
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

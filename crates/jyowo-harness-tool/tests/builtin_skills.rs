#![cfg(feature = "builtin-toolset")]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use futures::{future::BoxFuture, StreamExt};
use harness_contracts::{
    ActionResource, AgentId, CapabilityRegistry, ContextPatchRequest, ContextPatchSinkCap,
    ContextPatchSource, NetworkAccess, PermissionSubject, RenderedSkill, SkillFilter, SkillId,
    SkillParameterInfo, SkillRegistryCap, SkillScriptRunDeclaration, SkillScriptRunFile,
    SkillScriptRunPreparation, SkillStatus, SkillSummary, SkillView, TenantId, ToolActionPlan,
    ToolCapability, ToolError, ToolExecutionChannel, ToolGroup, ToolOrigin, ToolResult, ToolUseId,
    WorkspaceAccess,
};
use harness_tool::{
    builtin::{SkillsInvokeTool, SkillsListTool, SkillsRunScriptTool, SkillsViewTool},
    AuthorizedTicketSummary, AuthorizedToolInput, BuiltinToolset, InterruptToken, Tool,
    ToolContext, ToolRegistry,
};
use serde_json::{json, Value};

#[test]
fn skill_tools_declare_meta_descriptors_and_defer_policies() {
    let list = SkillsListTool::default();
    let view = SkillsViewTool::default();
    let invoke = SkillsInvokeTool::default();
    let run_script = SkillsRunScriptTool::default();

    assert_eq!(list.descriptor().name, "skills_list");
    assert_eq!(list.descriptor().group, ToolGroup::Meta);
    assert_eq!(list.descriptor().origin, ToolOrigin::Builtin);
    assert_eq!(
        list.descriptor().required_capabilities,
        vec![ToolCapability::SkillRegistry]
    );
    assert_eq!(
        list.descriptor().properties.defer_policy,
        harness_contracts::DeferPolicy::AlwaysLoad
    );

    assert_eq!(view.descriptor().name, "skills_view");
    assert_eq!(view.descriptor().group, ToolGroup::Meta);
    assert_eq!(view.descriptor().origin, ToolOrigin::Builtin);
    assert_eq!(
        view.descriptor().required_capabilities,
        vec![ToolCapability::SkillRegistry]
    );
    assert_eq!(
        view.descriptor().properties.defer_policy,
        harness_contracts::DeferPolicy::AutoDefer
    );

    assert_eq!(invoke.descriptor().name, "skills_invoke");
    assert_eq!(invoke.descriptor().group, ToolGroup::Meta);
    assert_eq!(invoke.descriptor().origin, ToolOrigin::Builtin);
    assert_eq!(
        invoke.descriptor().required_capabilities,
        vec![
            ToolCapability::SkillRegistry,
            ToolCapability::ContextPatchSink
        ]
    );
    assert_eq!(
        invoke.descriptor().properties.defer_policy,
        harness_contracts::DeferPolicy::AutoDefer
    );

    assert_eq!(run_script.descriptor().name, "skills_run_script");
    assert_eq!(run_script.descriptor().group, ToolGroup::Meta);
    assert_eq!(run_script.descriptor().origin, ToolOrigin::Builtin);
    assert_eq!(
        run_script.descriptor().required_capabilities,
        vec![
            ToolCapability::SkillRegistry,
            ToolCapability::ProcessSandbox,
        ]
    );
    assert_eq!(
        run_script.descriptor().properties.defer_policy,
        harness_contracts::DeferPolicy::AutoDefer
    );
}

#[test]
fn skills_run_script_has_an_independent_sandbox_capability() {
    let tool = SkillsRunScriptTool::default();

    assert_eq!(tool.descriptor().name, "skills_run_script");
    assert_eq!(
        tool.descriptor().required_capabilities,
        vec![
            ToolCapability::SkillRegistry,
            ToolCapability::ProcessSandbox,
        ]
    );
}

#[test]
fn default_builtin_toolset_registers_skill_tools() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    for name in [
        "skills_list",
        "skills_view",
        "skills_invoke",
        "skills_run_script",
    ] {
        assert!(registry.get(name).is_some(), "{name} should be registered");
    }
}

#[tokio::test]
async fn skill_tools_plan_declares_skill_resources() {
    let mut caps = CapabilityRegistry::default();
    caps.install::<dyn SkillRegistryCap>(
        ToolCapability::SkillRegistry,
        Arc::new(TestSkillRegistryCap),
    );
    let ctx = tool_ctx(AgentId::from_u128(7), caps);

    let list_plan = SkillsListTool::default()
        .plan(&json!({}), &ctx)
        .await
        .unwrap();
    assert!(matches!(
        list_plan.resources.as_slice(),
        [ActionResource::Skill { action, name }]
            if action == "list" && name.is_none()
    ));

    let view_plan = SkillsViewTool::default()
        .plan(&json!({ "name": "daily" }), &ctx)
        .await
        .unwrap();
    assert!(matches!(
        view_plan.resources.as_slice(),
        [ActionResource::Skill { action, name }]
            if action == "view" && name.as_deref() == Some("daily")
    ));

    let invoke_plan = SkillsInvokeTool::default()
        .plan(&json!({ "name": "daily" }), &ctx)
        .await
        .unwrap();
    assert!(matches!(
        invoke_plan.resources.as_slice(),
        [ActionResource::Skill { action, name }]
            if action == "invoke" && name.as_deref() == Some("daily")
    ));

    let run_plan = SkillsRunScriptTool::default()
        .plan(
            &json!({
                "name": "daily",
                "script_id": "collect",
                "arguments": { "topic": "M4" }
            }),
            &ctx,
        )
        .await
        .unwrap();
    assert!(matches!(
        run_plan.resources.as_slice(),
        [ActionResource::Skill { action, name }]
            if action == "run_script" && name.as_deref() == Some("daily")
    ));
    assert_eq!(
        run_plan.execution_channel,
        ToolExecutionChannel::ProcessSandbox
    );
    assert_eq!(run_plan.workspace_access, WorkspaceAccess::None);
    assert_eq!(run_plan.network_access, NetworkAccess::None);
    assert!(matches!(
        &run_plan.subject,
        PermissionSubject::Custom { kind, payload }
            if kind == "skill_script"
                && payload["skill_id"] == "skill-daily"
                && payload["script_id"] == "collect"
                && payload["package_hash"] == "package-hash"
                && payload["arguments"] == json!({ "topic": "M4" })
                && payload["workspace_access"] == "none"
                && payload["network_access"] == "none"
    ));
}

#[tokio::test]
async fn skills_list_uses_registry_capability_and_agent_filter() {
    let tool = SkillsListTool::default();
    let agent = AgentId::from_u128(42);
    let cap: Arc<dyn SkillRegistryCap> = Arc::new(TestSkillRegistryCap);
    let mut caps = CapabilityRegistry::default();
    caps.install(ToolCapability::SkillRegistry, cap);

    let result = execute_final(
        &tool,
        json!({
            "tag": "briefing",
            "category": "ops",
            "include_prerequisite_missing": true
        }),
        tool_ctx(agent, caps),
    )
    .await;

    assert_eq!(
        result,
        ToolResult::Structured(json!([{
            "name": "daily",
            "description": "Daily skill",
            "tags": ["briefing"],
            "category": "ops",
            "source": "workspace",
            "status": "ready"
        }]))
    );
}

#[tokio::test]
async fn skills_view_defaults_to_preview_and_hides_full_body() {
    let tool = SkillsViewTool::default();
    let cap: Arc<dyn SkillRegistryCap> = Arc::new(TestSkillRegistryCap);
    let mut caps = CapabilityRegistry::default();
    caps.install(ToolCapability::SkillRegistry, cap);

    let result = execute_final(
        &tool,
        json!({ "name": "daily" }),
        tool_ctx(AgentId::from_u128(7), caps),
    )
    .await;

    let ToolResult::Structured(ref value) = result else {
        panic!("expected structured skill view");
    };
    assert_eq!(value["summary"]["name"], "daily");
    assert_eq!(value["body_preview"], "preview");
    assert!(value["body_full"].is_null());
}

#[tokio::test]
async fn skills_invoke_returns_receipt_without_rendered_body() {
    let tool = SkillsInvokeTool::default();
    let cap: Arc<dyn SkillRegistryCap> = Arc::new(TestSkillRegistryCap);
    let patch_sink = Arc::new(RecordingPatchSink::default());
    let mut caps = CapabilityRegistry::default();
    caps.install(ToolCapability::SkillRegistry, cap);
    caps.install::<dyn ContextPatchSinkCap>(ToolCapability::ContextPatchSink, patch_sink.clone());

    let result = execute_final(
        &tool,
        json!({ "name": "daily", "params": { "topic": "M4" } }),
        tool_ctx(AgentId::from_u128(7), caps),
    )
    .await;

    let ToolResult::Structured(ref value) = result else {
        panic!("expected structured receipt");
    };
    assert_eq!(value["skill_name"], "daily");
    assert_eq!(value["bytes_injected"], 16);
    assert_eq!(value["consumed_config_keys"], json!(["github.org"]));
    assert!(value["injection_id"]
        .as_str()
        .unwrap()
        .starts_with("skill:daily:"));
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains("Daily M4"));

    let patches = patch_sink.patches().await;
    assert_eq!(patches.len(), 1);
    assert_eq!(patches[0].body, "Daily M4 content");
    assert!(matches!(
        &patches[0].source,
        ContextPatchSource::SkillInjection { skill_name, .. } if skill_name == "daily"
    ));
}

#[tokio::test]
async fn skill_tools_report_missing_registry_capability() {
    let tool = SkillsListTool::default();

    let error = execute_error(
        &tool,
        json!({}),
        tool_ctx(AgentId::from_u128(7), CapabilityRegistry::default()),
    )
    .await;

    assert!(matches!(
        error,
        ToolError::CapabilityMissing(ToolCapability::SkillRegistry)
    ));
}

struct TestSkillRegistryCap;

impl SkillRegistryCap for TestSkillRegistryCap {
    fn list_summaries(&self, agent: &AgentId, filter: SkillFilter) -> Vec<SkillSummary> {
        assert_eq!(*agent, AgentId::from_u128(42));
        assert_eq!(filter.tag.as_deref(), Some("briefing"));
        assert_eq!(filter.category.as_deref(), Some("ops"));
        assert!(filter.include_prerequisite_missing);
        vec![SkillSummary {
            name: "daily".to_owned(),
            description: "Daily skill".to_owned(),
            tags: vec!["briefing".to_owned()],
            category: Some("ops".to_owned()),
            source: harness_contracts::SkillSourceKind::Workspace,
            status: SkillStatus::Ready,
        }]
    }

    fn view(&self, agent: &AgentId, name: &str, full: bool) -> Option<SkillView> {
        assert_eq!(*agent, AgentId::from_u128(7));
        assert_eq!(name, "daily");
        assert!(!full);
        Some(SkillView {
            summary: SkillSummary {
                name: "daily".to_owned(),
                description: "Daily skill".to_owned(),
                tags: vec!["briefing".to_owned()],
                category: Some("ops".to_owned()),
                source: harness_contracts::SkillSourceKind::Workspace,
                status: SkillStatus::Ready,
            },
            parameters: vec![SkillParameterInfo {
                name: "topic".to_owned(),
                param_type: "string".to_owned(),
                required: true,
                default: None,
                description: None,
            }],
            config_keys: vec!["github.org".to_owned()],
            body_preview: "preview".to_owned(),
            body_full: None,
        })
    }

    fn render(
        &self,
        agent: &AgentId,
        name: String,
        params: Value,
    ) -> BoxFuture<'static, Result<RenderedSkill, ToolError>> {
        assert_eq!(*agent, AgentId::from_u128(7));
        assert_eq!(name, "daily");
        assert_eq!(params, json!({ "topic": "M4" }));
        Box::pin(async {
            Ok(RenderedSkill {
                skill_id: SkillId("skill-daily".to_owned()),
                skill_name: "daily".to_owned(),
                content: "Daily M4 content".to_owned(),
                shell_invocations: Vec::new(),
                consumed_config_keys: vec!["github.org".to_owned()],
            })
        })
    }

    fn prepare_script(
        &self,
        agent: &AgentId,
        name: String,
        script_id: String,
        arguments: Value,
    ) -> BoxFuture<'static, Result<SkillScriptRunPreparation, ToolError>> {
        assert_eq!(*agent, AgentId::from_u128(7));
        assert_eq!(name, "daily");
        assert_eq!(script_id, "collect");
        Box::pin(async move {
            Ok(SkillScriptRunPreparation {
                skill_id: SkillId("skill-daily".to_owned()),
                skill_name: "daily".to_owned(),
                script_id: "collect".to_owned(),
                package_hash: "package-hash".to_owned(),
                arguments,
                declaration: SkillScriptRunDeclaration {
                    path: PathBuf::from("scripts/collect.sh"),
                    timeout_seconds: 30,
                    max_stdout_bytes: 1024,
                    max_stderr_bytes: 1024,
                    max_output_bytes: 2048,
                    max_artifact_count: 4,
                    max_artifact_bytes: 4096,
                    network_access: NetworkAccess::None,
                    env_config_keys: BTreeMap::from([(
                        "API_TOKEN".to_owned(),
                        "apiToken".to_owned(),
                    )]),
                    secret_env_keys: BTreeSet::from(["API_TOKEN".to_owned()]),
                },
                files: vec![SkillScriptRunFile {
                    path: "scripts/collect.sh".to_owned(),
                    content: "#!/bin/sh\n".to_owned(),
                }],
                env: BTreeMap::from([("API_TOKEN".to_owned(), "secret".to_owned())]),
            })
        })
    }
}

#[derive(Default)]
struct RecordingPatchSink {
    patches: Arc<tokio::sync::Mutex<Vec<ContextPatchRequest>>>,
}

impl RecordingPatchSink {
    async fn patches(&self) -> Vec<ContextPatchRequest> {
        self.patches.lock().await.clone()
    }
}

impl ContextPatchSinkCap for RecordingPatchSink {
    fn push_patch(
        &self,
        request: ContextPatchRequest,
    ) -> BoxFuture<'static, Result<(), ToolError>> {
        let patches = self.patches.clone();
        Box::pin(async move {
            patches.lock().await.push(request);
            Ok(())
        })
    }
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

fn tool_ctx(agent_id: AgentId, cap_registry: CapabilityRegistry) -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: harness_contracts::SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id,
        subagent_depth: 0,
        workspace_root: std::env::temp_dir(),
        project_workspace_root: None,
        sandbox: None,
        cap_registry: Arc::new(cap_registry),
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::default(),
        parent_run: None,
        model: None,
        model_config_id: None,
        memory_thread_settings: None,
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
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

#![allow(unused_imports)]

use super::support::*;
use super::*;
use harness_contracts::{
    McpOrigin, McpServerId, McpServerSource, NetworkAccess, PluginId, SkillId, SkillOrigin,
    SkillSourceKind, ToolActionPlan, ToolExecutionChannel, ToolOrigin,
};
use harness_permission::PermissionCheck;
use harness_tool::{action_plan_from_permission_check, AuthorizedToolInput};
use std::collections::BTreeMap;

struct CatalogTestTool {
    descriptor: ToolDescriptor,
}

impl CatalogTestTool {
    fn named(name: &str, display_name: &str) -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: name.to_owned(),
                display_name: display_name.to_owned(),
                description: "Runtime catalog test tool".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::Custom("test".to_owned()),
                version: "0.1.0".to_owned(),
                input_schema: json!({ "type": "object" }),
                output_schema: None,
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: false,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                trust_level: TrustLevel::UserControlled,
                required_capabilities: Vec::new(),
                budget: ResultBudget {
                    metric: BudgetMetric::Chars,
                    limit: 1_024,
                    on_overflow: OverflowAction::Truncate,
                    preview_head_chars: 512,
                    preview_tail_chars: 512,
                },
                provider_restriction: ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
                search_hint: None,
                service_binding: None,
                metadata: Default::default(),
            },
        }
    }
}

#[async_trait]
impl Tool for CatalogTestTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
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
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        Ok(Box::pin(futures::stream::iter(vec![ToolEvent::Final(
            ToolResult::Text("done".to_owned()),
        )])))
    }
}

#[tokio::test]
async fn list_runtime_tools_returns_the_complete_settings_catalog() {
    let _home_lock = HOME_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("runtime-tools");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace)
        .await
        .expect("runtime state should initialize");

    let response = list_runtime_tools_with_runtime_state(&state).expect("tools should list");

    assert!(response.generation > 0);
    assert!(response.tools.len() > 17);
    assert!(response.tools.windows(2).all(|tools| {
        (
            &tools[0].group_label,
            &tools[0].display_name,
            &tools[0].name,
        ) <= (
            &tools[1].group_label,
            &tools[1].display_name,
            &tools[1].name,
        )
    }));

    let by_name = response
        .tools
        .iter()
        .map(|tool| (tool.name.as_str(), tool))
        .collect::<BTreeMap<_, _>>();

    for name in [
        "FileRead",
        "GitStatus",
        "ProcessStart",
        "Diagnostics",
        "memory",
        "skills_invoke",
        "MiniMaxTextToImage",
        "SeedanceTextToVideo",
    ] {
        assert!(by_name.contains_key(name), "{name} should be exposed");
    }

    assert_eq!(by_name["FileRead"].access, "readOnly");
    assert_eq!(by_name["FileWrite"].access, "destructive");
    assert_eq!(by_name["Todo"].access, "mutating");
    assert_eq!(
        by_name["Todo"].required_capabilities,
        vec!["todo_store".to_owned()]
    );
    assert_eq!(by_name["WebFetch"].execution_channel, "httpBroker");
    assert_eq!(by_name["WebSearch"].execution_channel, "externalCapability");
    assert_eq!(
        by_name["MiniMaxTextToImage"]
            .service_binding
            .as_ref()
            .unwrap()
            .provider_id,
        "minimax"
    );
}

#[tokio::test]
async fn list_runtime_tools_includes_dynamic_sources_from_the_same_registry() {
    let _home_lock = HOME_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("runtime-tools-dynamic-origin");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace)
        .await
        .expect("runtime state should initialize");
    let runtime = state
        .settings_runtime()
        .expect("settings runtime should initialize");
    let plugin_id = PluginId("formatter@1.0.0".to_owned());

    runtime
        .tool_registry()
        .register_from_plugin(
            plugin_id.clone(),
            TrustLevel::AdminTrusted,
            Box::new(CatalogTestTool::named("plugin_tool", "Plugin tool")),
        )
        .expect("plugin tool should register");

    let mut mcp_tool = CatalogTestTool::named("mcp_tool", "MCP tool");
    mcp_tool.descriptor.origin = ToolOrigin::Mcp(McpOrigin {
        server_id: McpServerId("workspace-server".to_owned()),
        upstream_name: "tools/mcp_tool".to_owned(),
        server_meta: BTreeMap::new(),
        server_source: McpServerSource::Workspace,
        server_trust: TrustLevel::AdminTrusted,
    });
    runtime
        .tool_registry()
        .register(Box::new(mcp_tool))
        .expect("mcp tool should register");

    let mut skill_tool = CatalogTestTool::named("skill_tool", "Skill tool");
    skill_tool.descriptor.origin = ToolOrigin::Skill(SkillOrigin {
        skill_id: SkillId("summarize".to_owned()),
        skill_name: "Summarize".to_owned(),
        source_kind: SkillSourceKind::User,
        trust: TrustLevel::AdminTrusted,
    });
    runtime
        .tool_registry()
        .register(Box::new(skill_tool))
        .expect("skill tool should register");

    let response = list_runtime_tools_with_runtime_state(&state).expect("tools should list");
    let by_name = response
        .tools
        .iter()
        .map(|tool| (tool.name.as_str(), tool))
        .collect::<BTreeMap<_, _>>();

    assert_eq!(by_name["plugin_tool"].origin_kind, "plugin");
    assert_eq!(by_name["plugin_tool"].origin_id, Some(plugin_id.0));
    assert_eq!(by_name["mcp_tool"].origin_kind, "mcp");
    assert_eq!(
        by_name["mcp_tool"].origin_id,
        Some("workspace-server".to_owned())
    );
    assert_eq!(by_name["skill_tool"].origin_kind, "skill");
    assert_eq!(
        by_name["skill_tool"].origin_id,
        Some("summarize".to_owned())
    );
}

#[test]
fn list_runtime_tools_rejects_an_uninitialized_settings_runtime() {
    let workspace = unique_workspace("runtime-tools-not-ready");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = DesktopRuntimeState::with_workspace_for_test(workspace).unwrap();

    let error = list_runtime_tools_with_runtime_state(&state)
        .expect_err("missing settings runtime must fail");

    assert_eq!(error.code, "RUNTIME_NOT_READY");
}

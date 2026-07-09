#![allow(unused_imports)]

use super::support::*;
use super::*;
use harness_contracts::{
    McpOrigin, McpServerId, McpServerSource, PluginId, SkillId, SkillOrigin, SkillSourceKind,
    ToolOrigin,
};
use std::collections::BTreeMap;

#[tokio::test]
async fn list_runtime_tools_returns_registered_tool_catalog() {
    let workspace = unique_workspace("runtime-tools");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace)
        .await
        .expect("runtime state should initialize");

    let response = list_runtime_tools_with_runtime_state(&state).expect("tools should list");

    assert!(response.generation > 0, "snapshot generation must be set");
    assert!(
        response.tools.len() > 17,
        "runtime catalog should include the full registered tool set"
    );

    let by_name = response
        .tools
        .iter()
        .map(|tool| (tool.name.as_str(), tool))
        .collect::<std::collections::BTreeMap<_, _>>();

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

    let file_read = by_name.get("FileRead").unwrap();
    assert_eq!(file_read.access, "readOnly");
    assert_eq!(file_read.origin_kind, "builtin");
    assert_eq!(file_read.origin_id, None);

    let file_write = by_name.get("FileWrite").unwrap();
    assert_eq!(file_write.access, "destructive");

    let todo = by_name.get("Todo").unwrap();
    assert_eq!(todo.access, "mutating");
    assert_eq!(todo.required_capabilities, vec!["todo_store".to_owned()]);

    let web_fetch = by_name.get("WebFetch").unwrap();
    assert_eq!(web_fetch.execution_channel, "httpBroker");

    let web_search = by_name.get("WebSearch").unwrap();
    assert_eq!(web_search.execution_channel, "externalCapability");

    let send_message = by_name.get("SendMessage").unwrap();
    assert_eq!(send_message.execution_channel, "externalCapability");

    let minimax = by_name.get("MiniMaxTextToImage").unwrap();
    assert_eq!(
        minimax.service_binding.as_ref().unwrap().provider_id,
        "minimax"
    );

    let seedance = by_name.get("SeedanceTextToVideo").unwrap();
    assert_eq!(
        seedance.service_binding.as_ref().unwrap().provider_id,
        "doubao"
    );
}

#[tokio::test]
async fn list_runtime_tools_marks_dynamic_source_tools_as_external_capabilities() {
    let workspace = unique_workspace("runtime-tools-plugin-origin");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace)
        .await
        .expect("runtime state should initialize");
    let plugin_id = PluginId("formatter@1.0.0".to_owned());

    state
        .harness()
        .expect("harness should initialize")
        .tool_registry()
        .register_from_plugin(
            plugin_id.clone(),
            TrustLevel::AdminTrusted,
            Box::new(NeedsPermissionTool::named("plugin_tool", "Plugin tool")),
        )
        .expect("plugin tool should register");
    let mcp_tool = NeedsPermissionTool::named("mcp_tool", "MCP tool");
    let mut mcp_descriptor = mcp_tool.descriptor.clone();
    mcp_descriptor.origin = ToolOrigin::Mcp(McpOrigin {
        server_id: McpServerId("workspace-server".to_owned()),
        upstream_name: "tools/mcp_tool".to_owned(),
        server_meta: BTreeMap::new(),
        server_source: McpServerSource::Workspace,
        server_trust: TrustLevel::AdminTrusted,
    });
    state
        .harness()
        .expect("harness should initialize")
        .tool_registry()
        .register(Box::new(NeedsPermissionTool {
            descriptor: mcp_descriptor,
        }))
        .expect("mcp tool should register");
    let skill_tool = NeedsPermissionTool::named("skill_tool", "Skill tool");
    let mut skill_descriptor = skill_tool.descriptor.clone();
    skill_descriptor.origin = ToolOrigin::Skill(SkillOrigin {
        skill_id: SkillId("summarize".to_owned()),
        skill_name: "Summarize".to_owned(),
        source_kind: SkillSourceKind::User,
        trust: TrustLevel::AdminTrusted,
    });
    state
        .harness()
        .expect("harness should initialize")
        .tool_registry()
        .register(Box::new(NeedsPermissionTool {
            descriptor: skill_descriptor,
        }))
        .expect("skill tool should register");

    let response = list_runtime_tools_with_runtime_state(&state).expect("tools should list");
    let by_name = response
        .tools
        .iter()
        .map(|tool| (tool.name.as_str(), tool))
        .collect::<std::collections::BTreeMap<_, _>>();
    let plugin_tool = by_name
        .get("plugin_tool")
        .expect("plugin tool should be exposed");
    assert_eq!(plugin_tool.origin_kind, "plugin");
    assert_eq!(plugin_tool.origin_id, Some(plugin_id.0));
    assert_eq!(plugin_tool.execution_channel, "externalCapability");

    let mcp_tool = by_name.get("mcp_tool").expect("mcp tool should be exposed");
    assert_eq!(mcp_tool.origin_kind, "mcp");
    assert_eq!(mcp_tool.origin_id, Some("workspace-server".to_owned()));
    assert_eq!(mcp_tool.execution_channel, "externalCapability");

    let skill_tool = by_name
        .get("skill_tool")
        .expect("skill tool should be exposed");
    assert_eq!(skill_tool.origin_kind, "skill");
    assert_eq!(skill_tool.origin_id, Some("summarize".to_owned()));
    assert_eq!(skill_tool.execution_channel, "externalCapability");
}

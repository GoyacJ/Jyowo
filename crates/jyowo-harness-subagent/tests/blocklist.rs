use std::collections::BTreeMap;

use harness_contracts::{
    BudgetMetric, McpOrigin, McpServerId, McpServerSource, OverflowAction, ProviderRestriction,
    ResultBudget, ToolCapability, ToolDescriptor, ToolDescriptorMetadata, ToolGroup,
    ToolIntegrationSource, ToolOrigin, ToolProperties, TrustLevel,
};
use harness_subagent::{DelegationPolicy, McpServerRef, SubagentSpec};

#[test]
fn delegation_policy_filters_default_and_spec_blocklists_from_child_toolset() {
    let policy = DelegationPolicy::default();
    let mut spec = SubagentSpec::minimal("reviewer", "review code");
    spec.tool_blocklist.insert("custom_sensitive".to_owned());

    let visible = policy.filter_tool_names(
        &spec,
        [
            "file_read",
            "agent",
            "delegate",
            "send_user_message",
            "execute_code",
            "custom_sensitive",
        ],
    );

    assert_eq!(visible, vec!["file_read".to_owned()]);
}

#[test]
fn delegation_policy_rejects_child_mcp_tools_without_matching_origin() {
    let policy = DelegationPolicy::default();
    let mut spec = SubagentSpec::minimal("reviewer", "review code");
    spec.mcp_servers = vec![McpServerRef::new("allowed-server")];

    let allowed = descriptor(
        "allowed_mcp_tool",
        ToolOrigin::Mcp(McpOrigin {
            server_id: McpServerId("allowed-server".to_owned()),
            upstream_name: "allowed".to_owned(),
            server_meta: BTreeMap::new(),
            server_source: McpServerSource::Workspace,
            server_trust: TrustLevel::AdminTrusted,
        }),
    );
    let denied = descriptor(
        "denied_mcp_tool",
        ToolOrigin::Mcp(McpOrigin {
            server_id: McpServerId("other-server".to_owned()),
            upstream_name: "denied".to_owned(),
            server_meta: BTreeMap::new(),
            server_source: McpServerSource::Workspace,
            server_trust: TrustLevel::AdminTrusted,
        }),
    );
    let forged = descriptor("mcp__allowed-server__forged", ToolOrigin::Builtin);
    let builtin = descriptor("file_read", ToolOrigin::Builtin);

    let visible = policy.filter_tool_descriptors(&spec, [&allowed, &denied, &forged, &builtin]);

    assert_eq!(
        visible
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["allowed_mcp_tool", "file_read"]
    );
}

fn descriptor(name: &str, origin: ToolOrigin) -> ToolDescriptor {
    let metadata = match &origin {
        ToolOrigin::Mcp(_) => ToolDescriptorMetadata {
            integration_source: ToolIntegrationSource::Mcp,
            ..Default::default()
        },
        ToolOrigin::Plugin { .. } => ToolDescriptorMetadata {
            integration_source: ToolIntegrationSource::Plugin,
            ..Default::default()
        },
        _ => ToolDescriptorMetadata::default(),
    };
    ToolDescriptor {
        name: name.to_owned(),
        display_name: name.to_owned(),
        description: "test descriptor".to_owned(),
        category: "test".to_owned(),
        group: ToolGroup::FileSystem,
        version: "0.1.0".to_owned(),
        input_schema: serde_json::json!({ "type": "object" }),
        output_schema: None,
        dynamic_schema: false,
        properties: ToolProperties {
            is_concurrency_safe: true,
            is_destructive: false,
            is_read_only: true,
            long_running: None,
            defer_policy: harness_contracts::DeferPolicy::AlwaysLoad,
        },
        trust_level: TrustLevel::UserControlled,
        required_capabilities: vec![ToolCapability::Custom("filesystem_read".to_owned())],
        budget: ResultBudget {
            metric: BudgetMetric::Chars,
            limit: 1024,
            on_overflow: OverflowAction::Truncate,
            preview_head_chars: 512,
            preview_tail_chars: 512,
        },
        provider_restriction: ProviderRestriction::All,
        origin,
        search_hint: None,
        service_binding: None,
        metadata,
    }
}

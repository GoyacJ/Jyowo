use harness_subagent::{DelegationPolicy, SubagentSpec};

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

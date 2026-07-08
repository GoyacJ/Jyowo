use std::collections::BTreeSet;

use harness_contracts::{ToolGroup, ToolProfile};
use serde_json::json;

#[test]
fn tool_profile_serializes_builtin_profiles_as_strings() {
    assert_eq!(
        serde_json::to_value(ToolProfile::Minimal).unwrap(),
        json!("minimal")
    );
    assert_eq!(
        serde_json::to_value(ToolProfile::Coding).unwrap(),
        json!("coding")
    );
    assert_eq!(
        serde_json::to_value(ToolProfile::Full).unwrap(),
        json!("full")
    );
}

#[test]
fn tool_profile_serializes_custom_filter_shape() {
    let profile = ToolProfile::Custom {
        allowlist: BTreeSet::from(["read".to_owned()]),
        denylist: BTreeSet::from(["bash".to_owned()]),
        group_allowlist: vec![ToolGroup::FileSystem],
        group_denylist: vec![ToolGroup::Network],
        mcp_included: false,
        plugin_included: false,
    };

    assert_eq!(
        serde_json::to_value(&profile).unwrap(),
        json!({
            "custom": {
                "allowlist": ["read"],
                "denylist": ["bash"],
                "group_allowlist": ["file_system"],
                "group_denylist": ["network"],
                "mcp_included": false,
                "plugin_included": false
            }
        })
    );
    assert_eq!(
        serde_json::from_value::<ToolProfile>(serde_json::to_value(profile).unwrap()).unwrap(),
        ToolProfile::Custom {
            allowlist: BTreeSet::from(["read".to_owned()]),
            denylist: BTreeSet::from(["bash".to_owned()]),
            group_allowlist: vec![ToolGroup::FileSystem],
            group_denylist: vec![ToolGroup::Network],
            mcp_included: false,
            plugin_included: false,
        }
    );
}

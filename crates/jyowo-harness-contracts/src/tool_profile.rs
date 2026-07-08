use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ToolGroup;

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolProfile {
    Minimal,
    Coding,
    Full,
    Custom {
        #[serde(default)]
        allowlist: BTreeSet<String>,
        #[serde(default)]
        denylist: BTreeSet<String>,
        #[serde(default)]
        group_allowlist: Vec<ToolGroup>,
        #[serde(default)]
        group_denylist: Vec<ToolGroup>,
        #[serde(default)]
        mcp_included: bool,
        #[serde(default)]
        plugin_included: bool,
    },
}

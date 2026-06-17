//! Tool descriptor contracts shared across model and tool crates.
//!
//! SPEC: docs/architecture/harness/crates/harness-contracts.md §3.4

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ProviderRestriction, ResultBudget, SemverString, ToolCapability, ToolGroup, ToolName,
    ToolOrigin, ToolProperties, TrustLevel,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolDescriptor {
    pub name: ToolName,
    pub display_name: String,
    pub description: String,
    pub category: String,
    pub group: ToolGroup,
    pub version: SemverString,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    pub dynamic_schema: bool,
    pub properties: ToolProperties,
    pub trust_level: TrustLevel,
    pub required_capabilities: Vec<ToolCapability>,
    pub budget: ResultBudget,
    pub provider_restriction: ProviderRestriction,
    pub origin: ToolOrigin,
    pub search_hint: Option<String>,
}

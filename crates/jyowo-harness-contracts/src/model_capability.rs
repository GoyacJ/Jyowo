//! Model and provider capability contracts.
//!
//! SPEC: docs/architecture/harness/crates/harness-contracts.md

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelProtocol {
    ChatCompletions,
    Responses,
    Messages,
    GenerateContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelModality {
    Text,
    Image,
    Audio,
    Video,
    File,
    Embedding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConversationModelCapability {
    pub input_modalities: Vec<ModelModality>,
    pub output_modalities: Vec<ModelModality>,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub streaming: bool,
    pub tool_calling: bool,
    pub reasoning: bool,
    pub prompt_cache: bool,
    pub structured_output: bool,
}

impl Default for ConversationModelCapability {
    fn default() -> Self {
        Self {
            input_modalities: vec![ModelModality::Text],
            output_modalities: vec![ModelModality::Text],
            context_window: 0,
            max_output_tokens: 0,
            streaming: true,
            tool_calling: true,
            reasoning: false,
            prompt_cache: false,
            structured_output: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderServiceCapability {
    pub operation_id: String,
    pub category: ProviderServiceCategory,
    pub input_modalities: Vec<ModelModality>,
    pub output_artifact: ModelModality,
    pub execution: ProviderServiceExecution,
    pub requires_polling: bool,
    pub permission_subject: String,
    pub cost_risk: ProviderServiceCostRisk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderServiceCategory {
    Conversation,
    Image,
    Video,
    Audio,
    Music,
    File,
    Model,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderServiceExecution {
    Sync,
    AsyncJob,
    Websocket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderServiceCostRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderRuntimeCapability {
    pub auth_scheme: ProviderAuthScheme,
    pub base_url_regions: Vec<ProviderBaseUrlRegion>,
    pub supports_live_validation: bool,
    pub supports_streaming_validation: bool,
    pub secret_reveal_supported: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuthScheme {
    Bearer,
    ApiKey,
    XApiKey,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderBaseUrlRegion {
    pub id: String,
    pub label: String,
    pub base_url: String,
}

//! Model and provider capability contracts.
//!

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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
pub struct RunModelSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_config_id: Option<String>,
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub protocol: ModelProtocol,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub conversation_capability: ConversationModelCapability,
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
pub enum CapabilityRouteKind {
    ImageGeneration,
    VideoGeneration,
    ThreeDGeneration,
    EmbeddingGeneration,
    FileOperation,
    TextToSpeech,
    SpeechToText,
    MusicGeneration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderCapabilityRoute {
    pub kind: CapabilityRouteKind,
    pub config_id: String,
    pub provider_id: String,
    pub operation_ids: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderCapabilityRouteSettings {
    pub version: u32,
    pub routes: Vec<ProviderCapabilityRoute>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProviderCapabilityRouteOption {
    pub kind: CapabilityRouteKind,
    pub config_id: String,
    pub provider_id: String,
    pub operation_id: String,
    pub output_artifact: ModelModality,
    pub execution: ProviderServiceExecution,
    pub cost_risk: ProviderServiceCostRisk,
    pub runtime_supported: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ListProviderCapabilityRouteOptionsResponse {
    pub options: Vec<ProviderCapabilityRouteOption>,
}

pub fn validate_provider_capability_route(route: &ProviderCapabilityRoute) -> Result<(), String> {
    if route.config_id.trim().is_empty() {
        return Err("config_id is required".to_owned());
    }
    if route.provider_id.trim().is_empty() {
        return Err("provider_id is required".to_owned());
    }
    if route.operation_ids.is_empty() {
        return Err("operation_ids must not be empty".to_owned());
    }

    let mut seen = HashSet::new();
    for operation_id in &route.operation_ids {
        let operation_id = operation_id.trim();
        if operation_id.is_empty() {
            return Err("operation_ids must not contain empty values".to_owned());
        }
        if !seen.insert(operation_id.to_owned()) {
            return Err("operation_ids must not contain duplicates".to_owned());
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderServiceCategory {
    Conversation,
    Image,
    Video,
    ThreeD,
    Embedding,
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

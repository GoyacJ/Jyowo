use async_trait::async_trait;
use harness_contracts::ModelError;
use serde::Deserialize;
use std::sync::Arc;

use crate::openai_compatible::{OpenAiCompatibleClient, OpenAiCompatibleProviderExt};
use crate::{
    ConversationModelCapability, InferContext, ModelCredentialResolver, ModelDescriptor,
    ModelInventoryEntry, ModelLifecycle, ModelModality, ModelProtocol, ModelProvider, ModelRequest,
    ModelRuntimeStatus, ModelStream,
};

const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api";
const PROVIDER_ID: &str = "openrouter";
pub const OPENROUTER_API_KEY_ENV: &str = "OPENROUTER_API_KEY";

#[derive(Clone)]
pub struct OpenRouterProvider {
    client: OpenAiCompatibleClient,
    extra_models: Vec<ModelDescriptor>,
}

impl OpenRouterProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiCompatibleClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID),
            extra_models: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.client = self.client.with_base_url(base_url);
        self
    }

    #[must_use]
    pub fn with_credential_resolver(mut self, resolver: Arc<dyn ModelCredentialResolver>) -> Self {
        self.client = self.client.with_credential_resolver(resolver);
        self
    }

    #[must_use]
    pub fn with_model_descriptor(mut self, descriptor: ModelDescriptor) -> Self {
        if descriptor.provider_id == PROVIDER_ID {
            self.extra_models.push(descriptor);
        }
        self
    }
}

impl OpenAiCompatibleProviderExt for OpenRouterProvider {
    fn client(&self) -> &OpenAiCompatibleClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for OpenRouterProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        // Verified 2026-06-21: https://openrouter.ai/api/v1/models
        let mut models = vec![
            descriptor("openai/gpt-5.5", "OpenAI GPT-5.5 via OpenRouter"),
            descriptor("anthropic/claude-fable-5", "Claude Fable 5 via OpenRouter"),
            descriptor(
                "anthropic/claude-sonnet-4.6",
                "Claude Sonnet 4.6 via OpenRouter",
            ),
            descriptor("google/gemini-2.5-pro", "Gemini 2.5 Pro via OpenRouter"),
            descriptor("deepseek/deepseek-v4-pro", "DeepSeek V4 Pro via OpenRouter"),
            descriptor("moonshotai/kimi-k2.7-code", "Kimi K2.7 Code via OpenRouter"),
            descriptor("z-ai/glm-5.2", "GLM 5.2 via OpenRouter"),
            descriptor("minimax/minimax-m3", "MiniMax M3 via OpenRouter"),
        ];
        for descriptor in &self.extra_models {
            if !models
                .iter()
                .any(|model| model.model_id == descriptor.model_id)
            {
                models.push(descriptor.clone());
            }
        }
        models
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        self.infer_openai_compatible(req, ctx).await
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::ChatCompletions
    }
}

fn descriptor(model_id: &str, display_name: &str) -> ModelDescriptor {
    ModelDescriptor {
        provider_id: "openrouter".to_owned(),
        model_id: model_id.to_owned(),
        display_name: display_name.to_owned(),
        protocol: ModelProtocol::ChatCompletions,
        context_window: 128_000,
        max_output_tokens: 8192,
        conversation_capability: ConversationModelCapability {
            context_window: 128_000,
            max_output_tokens: 8192,
            tool_calling: true,
            reasoning: false,
            prompt_cache: false,
            streaming: true,
            structured_output: false,
            input_modalities: vec![ModelModality::Text],
            output_modalities: vec![ModelModality::Text],
        },
        runtime_semantics: crate::ModelRuntimeSemantics::openai_chat_plain(),
        lifecycle: ModelLifecycle::Stable,
        pricing: None,
    }
}

pub fn inventory_from_models_api_json(
    bytes: &[u8],
) -> Result<Vec<ModelInventoryEntry>, ModelError> {
    let response: OpenRouterModelsResponse = serde_json::from_slice(bytes)
        .map_err(|error| ModelError::UnexpectedResponse(error.to_string()))?;
    let mut models = response
        .data
        .into_iter()
        .filter_map(OpenRouterModelData::into_inventory_entry)
        .collect::<Vec<_>>();
    models.sort_by(|left, right| left.model_id.cmp(&right.model_id));
    Ok(models)
}

#[derive(Debug, Deserialize)]
struct OpenRouterModelsResponse {
    data: Vec<OpenRouterModelData>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModelData {
    id: String,
    name: Option<String>,
    context_length: Option<u32>,
    architecture: Option<OpenRouterArchitecture>,
    top_provider: Option<OpenRouterTopProvider>,
    supported_parameters: Option<Vec<String>>,
}

impl OpenRouterModelData {
    fn into_inventory_entry(self) -> Option<ModelInventoryEntry> {
        let model_id = self.id;
        if model_id.trim().is_empty() {
            return None;
        }
        let input_modalities = self
            .architecture
            .as_ref()
            .map(|architecture| modalities(&architecture.input_modalities))
            .unwrap_or_else(|| vec![ModelModality::Text]);
        let output_modalities = self
            .architecture
            .as_ref()
            .map(|architecture| modalities(&architecture.output_modalities))
            .unwrap_or_else(|| vec![ModelModality::Text]);
        let supported_parameters = self.supported_parameters.unwrap_or_default();
        let tool_calling = supported_parameters.iter().any(|value| value == "tools");
        let structured_output = supported_parameters
            .iter()
            .any(|value| value == "structured_outputs");
        let reasoning = supported_parameters
            .iter()
            .any(|value| value == "reasoning" || value == "include_reasoning");
        let is_text_to_text =
            input_modalities == [ModelModality::Text] && output_modalities == [ModelModality::Text];
        let context_window = self
            .top_provider
            .as_ref()
            .and_then(|provider| provider.context_length)
            .or(self.context_length)
            .unwrap_or(0);
        let max_output_tokens = self
            .top_provider
            .as_ref()
            .and_then(|provider| provider.max_completion_tokens)
            .unwrap_or(0);
        let conversation_capability = ConversationModelCapability {
            context_window,
            max_output_tokens,
            tool_calling,
            reasoning,
            prompt_cache: supported_parameters
                .iter()
                .any(|value| value == "cache_control"),
            streaming: is_text_to_text,
            structured_output,
            input_modalities,
            output_modalities,
        };
        let runtime_status = if is_text_to_text {
            ModelRuntimeStatus::Runnable
        } else {
            ModelRuntimeStatus::Unsupported {
                reason: "model modalities are not supported by the current runtime".to_owned(),
            }
        };
        Some(ModelInventoryEntry {
            provider_id: PROVIDER_ID.to_owned(),
            model_id: model_id.clone(),
            display_name: self.name.unwrap_or(model_id),
            protocol: ModelProtocol::ChatCompletions,
            context_window,
            max_output_tokens,
            conversation_capability,
            runtime_semantics: crate::ModelRuntimeSemantics::openai_chat_plain(),
            lifecycle: ModelLifecycle::Stable,
            pricing: None,
            runtime_status,
        })
    }
}

#[derive(Debug, Deserialize)]
struct OpenRouterArchitecture {
    #[serde(default)]
    input_modalities: Vec<String>,
    #[serde(default)]
    output_modalities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterTopProvider {
    context_length: Option<u32>,
    max_completion_tokens: Option<u32>,
}

fn modalities(values: &[String]) -> Vec<ModelModality> {
    let mut modalities = values
        .iter()
        .filter_map(|value| match value.as_str() {
            "text" => Some(ModelModality::Text),
            "image" => Some(ModelModality::Image),
            "audio" => Some(ModelModality::Audio),
            "video" => Some(ModelModality::Video),
            "file" => Some(ModelModality::File),
            "embedding" => Some(ModelModality::Embedding),
            _ => None,
        })
        .collect::<Vec<_>>();
    if modalities.is_empty() {
        modalities.push(ModelModality::Text);
    }
    modalities.sort_by_key(|modality| model_modality_order(modality));
    modalities.dedup();
    modalities
}

fn model_modality_order(modality: &ModelModality) -> u8 {
    match modality {
        ModelModality::Text => 0,
        ModelModality::Image => 1,
        ModelModality::Audio => 2,
        ModelModality::Video => 3,
        ModelModality::File => 4,
        ModelModality::Embedding => 5,
    }
}

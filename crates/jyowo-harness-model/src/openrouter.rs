use async_trait::async_trait;
use harness_contracts::ModelError;
use serde::Deserialize;
use std::sync::Arc;

use crate::openai_protocol::{OpenAiChatDialect, OpenAiProtocolClient, OpenAiProtocolProviderExt};
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
    client: OpenAiProtocolClient,
    extra_models: Vec<ModelDescriptor>,
}

impl OpenRouterProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiProtocolClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
                .with_chat_dialect(OpenAiChatDialect::OpenRouter),
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

impl OpenAiProtocolProviderExt for OpenRouterProvider {
    fn client(&self) -> &OpenAiProtocolClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for OpenRouterProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        let mut models = crate::catalog::provider_model_descriptors(PROVIDER_ID);
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
        self.infer_openai_protocol(req, ctx).await
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::ChatCompletions
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
            provider_declared_capability: conversation_capability.clone(),
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

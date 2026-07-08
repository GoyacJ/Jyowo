use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::ModelError;

use crate::openai_protocol::{OpenAiChatDialect, OpenAiProtocolClient, OpenAiProtocolProviderExt};
use crate::{
    ConversationModelCapability, InferContext, ModelCredentialResolver, ModelDescriptor,
    ModelLifecycle, ModelModality, ModelProtocol, ModelProvider, ModelRequest, ModelStream,
};

const DEFAULT_BASE_URL: &str = "https://api.minimaxi.com";
const PROVIDER_ID: &str = "minimax";
pub const MINIMAX_API_KEY_ENV: &str = "MINIMAX_API_KEY";

#[derive(Clone)]
pub struct MinimaxProvider {
    client: OpenAiProtocolClient,
}

impl MinimaxProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiProtocolClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
                .with_chat_dialect(OpenAiChatDialect::MiniMax)
                .with_chat_completions_path("/v1/chat/completions")
                .with_max_tokens_field("max_completion_tokens"),
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
}

impl OpenAiProtocolProviderExt for MinimaxProvider {
    fn client(&self) -> &OpenAiProtocolClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for MinimaxProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        // Verified 2026-06-22: https://platform.minimax.io/docs/guides/models-intro
        vec![
            descriptor(
                "MiniMax-M3",
                "MiniMax M3",
                1_000_000,
                524_288,
                CapabilityProfile::M3,
            ),
            descriptor(
                "MiniMax-M2.7",
                "MiniMax M2.7",
                204_800,
                204_800,
                CapabilityProfile::Text,
            ),
            descriptor(
                "MiniMax-M2.7-highspeed",
                "MiniMax M2.7 Highspeed",
                204_800,
                204_800,
                CapabilityProfile::Text,
            ),
            descriptor(
                "MiniMax-M2.5",
                "MiniMax M2.5",
                204_800,
                204_800,
                CapabilityProfile::Text,
            ),
            descriptor(
                "MiniMax-M2.5-highspeed",
                "MiniMax M2.5 Highspeed",
                204_800,
                204_800,
                CapabilityProfile::Text,
            ),
            descriptor(
                "MiniMax-M2.1",
                "MiniMax M2.1",
                204_800,
                204_800,
                CapabilityProfile::Text,
            ),
            descriptor(
                "MiniMax-M2.1-highspeed",
                "MiniMax M2.1 Highspeed",
                204_800,
                204_800,
                CapabilityProfile::Text,
            ),
            descriptor(
                "MiniMax-M2",
                "MiniMax M2",
                204_800,
                204_800,
                CapabilityProfile::Text,
            ),
            descriptor(
                "M2-her",
                "MiniMax M2 Her",
                65_536,
                65_536,
                CapabilityProfile::Text,
            ),
        ]
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        self.infer_openai_protocol(req, ctx).await
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::ChatCompletions
    }
}

fn descriptor(
    model_id: &str,
    display_name: &str,
    context_window: u32,
    max_output_tokens: u32,
    profile: CapabilityProfile,
) -> ModelDescriptor {
    let input_modalities = match profile {
        CapabilityProfile::M3 => vec![
            ModelModality::Text,
            ModelModality::Image,
            ModelModality::Video,
        ],
        CapabilityProfile::Text => vec![ModelModality::Text],
    };
    ModelDescriptor {
        provider_id: PROVIDER_ID.to_owned(),
        model_id: model_id.to_owned(),
        display_name: display_name.to_owned(),
        protocol: ModelProtocol::ChatCompletions,
        context_window,
        max_output_tokens,
        conversation_capability: ConversationModelCapability {
            context_window,
            max_output_tokens,
            tool_calling: true,
            reasoning: matches!(profile, CapabilityProfile::M3),
            prompt_cache: true,
            streaming: true,
            structured_output: false,
            input_modalities,
            output_modalities: vec![ModelModality::Text],
        },
        runtime_semantics: crate::ModelRuntimeSemantics::openai_chat_minimax(),
        lifecycle: ModelLifecycle::Stable,
        pricing: None,
    }
}

#[derive(Debug, Clone, Copy)]
enum CapabilityProfile {
    Text,
    M3,
}

use std::sync::Arc;

use async_trait::async_trait;
use chrono::NaiveDate;
use harness_contracts::ModelError;

use crate::openai_compatible::{OpenAiCompatibleClient, OpenAiCompatibleProviderExt};
use crate::{
    ConversationModelCapability, InferContext, ModelCredentialResolver, ModelDescriptor,
    ModelLifecycle, ModelModality, ModelProtocol, ModelProvider, ModelRequest, ModelStream,
};

const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
const PROVIDER_ID: &str = "deepseek";
pub const DEEPSEEK_API_KEY_ENV: &str = "DEEPSEEK_API_KEY";

#[derive(Clone)]
pub struct DeepSeekProvider {
    client: OpenAiCompatibleClient,
}

impl DeepSeekProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiCompatibleClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
                .with_chat_completions_path("/v1/chat/completions"),
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

impl OpenAiCompatibleProviderExt for DeepSeekProvider {
    fn client(&self) -> &OpenAiCompatibleClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for DeepSeekProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        // Verified 2026-06-21: https://api-docs.deepseek.com/quick_start/pricing
        vec![
            descriptor("deepseek-v4-flash", "DeepSeek V4 Flash", 1_000_000, 384_000),
            descriptor("deepseek-v4-pro", "DeepSeek V4 Pro", 1_000_000, 384_000),
            deprecated_descriptor("deepseek-chat", "DeepSeek Chat", 64_000, 8192),
            deprecated_descriptor("deepseek-reasoner", "DeepSeek Reasoner", 64_000, 8192),
        ]
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        self.infer_openai_compatible(req, ctx).await
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
) -> ModelDescriptor {
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
            reasoning: model_id.contains("reasoner"),
            prompt_cache: false,
            streaming: true,
            structured_output: false,
            input_modalities: vec![ModelModality::Text],
            output_modalities: vec![ModelModality::Text],
        },
        lifecycle: ModelLifecycle::Stable,
        pricing: None,
    }
}

fn deprecated_descriptor(
    model_id: &str,
    display_name: &str,
    context_window: u32,
    max_output_tokens: u32,
) -> ModelDescriptor {
    let mut descriptor = descriptor(model_id, display_name, context_window, max_output_tokens);
    descriptor.lifecycle = ModelLifecycle::Deprecated {
        retirement_date: NaiveDate::from_ymd_opt(2026, 7, 24).expect("valid retirement date"),
    };
    descriptor
}

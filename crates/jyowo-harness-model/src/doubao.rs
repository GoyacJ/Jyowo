use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::ModelError;

use crate::openai_compatible::{
    OpenAiChatDialect, OpenAiCompatibleClient, OpenAiCompatibleProviderExt,
};
use crate::{
    ConversationModelCapability, InferContext, ModelCredentialResolver, ModelDescriptor,
    ModelLifecycle, ModelModality, ModelProtocol, ModelProvider, ModelRequest, ModelStream,
};

const DEFAULT_BASE_URL: &str = "https://ark.cn-beijing.volces.com/api/v3";
const PROVIDER_ID: &str = "doubao";
pub const DOUBAO_API_KEY_ENV: &str = "DOUBAO_API_KEY";

#[derive(Clone)]
pub struct DoubaoProvider {
    client: OpenAiCompatibleClient,
}

impl DoubaoProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiCompatibleClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
                .with_chat_dialect(OpenAiChatDialect::Doubao)
                .with_chat_completions_path("/chat/completions"),
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

impl OpenAiCompatibleProviderExt for DoubaoProvider {
    fn client(&self) -> &OpenAiCompatibleClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for DoubaoProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        // Verified 2026-06-21: https://www.volcengine.com/docs/82379/1494384
        vec![
            descriptor(
                "doubao-seed-2-0-mini-260428",
                "Doubao Seed 2.0 Mini",
                256_000,
                64_000,
                false,
            ),
            descriptor(
                "doubao-seed-1-8-260116",
                "Doubao Seed 1.8",
                256_000,
                64_000,
                false,
            ),
            descriptor(
                "doubao-seed-code-251201",
                "Doubao Seed Code",
                256_000,
                64_000,
                false,
            ),
            descriptor("doubao-seed-1.6", "Doubao Seed 1.6", 256_000, 64_000, false),
            descriptor(
                "doubao-seed-1.6-thinking",
                "Doubao Seed 1.6 Thinking",
                256_000,
                64_000,
                true,
            ),
            descriptor(
                "doubao-seed-1.6-flash",
                "Doubao Seed 1.6 Flash",
                256_000,
                64_000,
                false,
            ),
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
    reasoning: bool,
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
            reasoning,
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

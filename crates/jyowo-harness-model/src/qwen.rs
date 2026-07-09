use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::ModelError;

use crate::openai_protocol::{OpenAiChatDialect, OpenAiProtocolClient, OpenAiProtocolProviderExt};
use crate::{
    ConversationModelCapability, InferContext, ModelCredentialResolver, ModelDescriptor,
    ModelLifecycle, ModelModality, ModelProtocol, ModelProvider, ModelRequest, ModelStream,
};

pub const DEFAULT_BASE_URL: &str = "https://dashscope-us.aliyuncs.com/compatible-mode/v1";
pub const LEGACY_BASE_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode";
pub const LEGACY_BASE_URL_V1: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";
const PROVIDER_ID: &str = "qwen";
pub const DASHSCOPE_API_KEY_ENV: &str = "DASHSCOPE_API_KEY";
pub const QWEN_API_KEY_ENV: &str = "QWEN_API_KEY";

#[derive(Clone)]
pub struct QwenProvider {
    chat_client: OpenAiProtocolClient,
    responses_client: OpenAiProtocolClient,
}

impl QwenProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        let api_key = api_key.into();
        Self {
            chat_client: qwen_chat_client(api_key.clone(), DEFAULT_BASE_URL),
            responses_client: qwen_responses_client(api_key, DEFAULT_BASE_URL),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let base_url = normalize_qwen_base_url(base_url.into());
        self.chat_client = self.chat_client.with_base_url(base_url.clone());
        self.responses_client = self.responses_client.with_base_url(base_url);
        self
    }

    #[must_use]
    pub fn with_credential_resolver(mut self, resolver: Arc<dyn ModelCredentialResolver>) -> Self {
        self.chat_client = self
            .chat_client
            .with_credential_resolver(Arc::clone(&resolver));
        self.responses_client = self.responses_client.with_credential_resolver(resolver);
        self
    }

    #[must_use]
    pub fn with_default_headers(mut self, headers: BTreeMap<String, String>) -> Self {
        self.chat_client = self.chat_client.with_extra_headers(headers.clone());
        self.responses_client = self.responses_client.with_extra_headers(headers);
        self
    }
}

impl OpenAiProtocolProviderExt for QwenProvider {
    fn client(&self) -> &OpenAiProtocolClient {
        &self.chat_client
    }
}

#[async_trait]
impl ModelProvider for QwenProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        // Verified 2026-07-09:
        // https://help.aliyun.com/en/model-studio/text-generation-model/
        // https://help.aliyun.com/en/model-studio/vision-model/
        vec![
            descriptor(
                "qwen3-max",
                "Qwen3 Max",
                256_000,
                65_536,
                true,
                true,
                text_modalities(),
            ),
            descriptor(
                "qwen3-max-2026-01-23",
                "Qwen3 Max 2026-01-23",
                256_000,
                65_536,
                true,
                true,
                text_modalities(),
            ),
            descriptor(
                "qwen3.7-max",
                "Qwen3.7 Max",
                1_000_000,
                65_536,
                true,
                false,
                text_modalities(),
            ),
            descriptor(
                "qwen3.7-max-preview",
                "Qwen3.7 Max Preview",
                1_000_000,
                65_536,
                true,
                false,
                text_modalities(),
            ),
            descriptor(
                "qwen3.7-max-2026-06-08",
                "Qwen3.7 Max 2026-06-08",
                1_000_000,
                65_536,
                true,
                false,
                text_modalities(),
            ),
            descriptor(
                "qwen3.7-max-2026-05-20",
                "Qwen3.7 Max 2026-05-20",
                1_000_000,
                65_536,
                true,
                false,
                text_modalities(),
            ),
            descriptor(
                "qwen3.7-max-2026-05-17",
                "Qwen3.7 Max 2026-05-17",
                1_000_000,
                65_536,
                true,
                false,
                text_modalities(),
            ),
            descriptor(
                "qwen3.7-plus",
                "Qwen3.7 Plus",
                1_000_000,
                65_536,
                true,
                true,
                multimodal_modalities(),
            ),
            descriptor(
                "qwen3.7-plus-2026-01-23",
                "Qwen3.7 Plus 2026-01-23",
                1_000_000,
                65_536,
                true,
                true,
                multimodal_modalities(),
            ),
            descriptor(
                "qwen3.6-flash",
                "Qwen3.6 Flash",
                1_000_000,
                65_536,
                true,
                true,
                multimodal_modalities(),
            ),
            descriptor(
                "qwen3.6-plus",
                "Qwen3.6 Plus",
                1_000_000,
                65_536,
                true,
                true,
                multimodal_modalities(),
            ),
            descriptor(
                "qwen3.5-plus",
                "Qwen3.5 Plus",
                1_000_000,
                65_536,
                true,
                true,
                multimodal_modalities(),
            ),
            descriptor(
                "qwen3.5-flash",
                "Qwen3.5 Flash",
                1_000_000,
                65_536,
                true,
                true,
                multimodal_modalities(),
            ),
            descriptor(
                "qwen-plus",
                "Qwen Plus",
                1_000_000,
                65_536,
                true,
                true,
                text_modalities(),
            ),
            descriptor(
                "qwen-flash",
                "Qwen Flash",
                1_000_000,
                65_536,
                true,
                true,
                text_modalities(),
            ),
            descriptor(
                "qwen3-coder-plus",
                "Qwen3 Coder Plus",
                1_000_000,
                65_536,
                true,
                true,
                text_modalities(),
            ),
            descriptor(
                "qwen3-coder-flash",
                "Qwen3 Coder Flash",
                1_000_000,
                65_536,
                true,
                true,
                text_modalities(),
            ),
            descriptor(
                "qwen3-coder-next",
                "Qwen3 Coder Next",
                256_000,
                65_536,
                true,
                true,
                text_modalities(),
            ),
            descriptor(
                "qwen3.6-35b-a3b",
                "Qwen3.6 35B A3B",
                1_000_000,
                65_536,
                true,
                true,
                text_modalities(),
            ),
            descriptor(
                "qwen3.5-397b-a17b",
                "Qwen3.5 397B A17B",
                1_000_000,
                65_536,
                true,
                true,
                text_modalities(),
            ),
            descriptor(
                "qwen3.5-122b-a10b",
                "Qwen3.5 122B A10B",
                1_000_000,
                65_536,
                true,
                true,
                text_modalities(),
            ),
            descriptor(
                "qwen3.5-27b",
                "Qwen3.5 27B",
                1_000_000,
                65_536,
                true,
                true,
                text_modalities(),
            ),
            descriptor(
                "qwen3.5-35b-a3b",
                "Qwen3.5 35B A3B",
                1_000_000,
                65_536,
                true,
                true,
                text_modalities(),
            ),
            descriptor(
                "qwen3-vl-plus",
                "Qwen3 VL Plus",
                128_000,
                8192,
                false,
                true,
                multimodal_modalities(),
            ),
            descriptor(
                "qwen3-vl-flash",
                "Qwen3 VL Flash",
                128_000,
                8192,
                false,
                true,
                multimodal_modalities(),
            ),
        ]
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        match req.protocol {
            ModelProtocol::ChatCompletions => self.chat_client.infer(req, ctx).await,
            ModelProtocol::Responses => self.responses_client.infer(req, ctx).await,
            protocol => Err(ModelError::InvalidRequest(format!(
                "QwenProvider only supports chat_completions and responses, got {protocol:?}"
            ))),
        }
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::Responses
    }
}

fn qwen_chat_client(api_key: String, base_url: &str) -> OpenAiProtocolClient {
    OpenAiProtocolClient::from_api_key(api_key, base_url)
        .with_provider_id(PROVIDER_ID)
        .with_chat_dialect(OpenAiChatDialect::Qwen)
        .with_chat_completions_path("/chat/completions")
        .with_max_tokens_field("max_completion_tokens")
}

fn qwen_responses_client(api_key: String, base_url: &str) -> OpenAiProtocolClient {
    OpenAiProtocolClient::from_api_key(api_key, base_url)
        .with_provider_id(PROVIDER_ID)
        .with_responses_path("/responses")
}

pub fn normalize_qwen_base_url(base_url: impl Into<String>) -> String {
    let base_url = base_url.into().trim_end_matches('/').to_owned();
    if base_url == LEGACY_BASE_URL {
        LEGACY_BASE_URL_V1.to_owned()
    } else {
        base_url
    }
}

fn descriptor(
    model_id: &str,
    display_name: &str,
    context_window: u32,
    max_output_tokens: u32,
    reasoning: bool,
    structured_output: bool,
    input_modalities: Vec<ModelModality>,
) -> ModelDescriptor {
    ModelDescriptor {
        provider_id: PROVIDER_ID.to_owned(),
        model_id: model_id.to_owned(),
        display_name: display_name.to_owned(),
        protocol: ModelProtocol::Responses,
        context_window,
        max_output_tokens,
        conversation_capability: ConversationModelCapability {
            context_window,
            max_output_tokens,
            tool_calling: true,
            reasoning,
            prompt_cache: false,
            streaming: true,
            structured_output,
            input_modalities,
            output_modalities: vec![ModelModality::Text],
        },
        runtime_semantics: crate::ModelRuntimeSemantics::openai_responses_default(),
        lifecycle: ModelLifecycle::Stable,
        pricing: None,
    }
}

fn text_modalities() -> Vec<ModelModality> {
    vec![ModelModality::Text]
}

fn multimodal_modalities() -> Vec<ModelModality> {
    vec![
        ModelModality::Text,
        ModelModality::Image,
        ModelModality::Video,
    ]
}

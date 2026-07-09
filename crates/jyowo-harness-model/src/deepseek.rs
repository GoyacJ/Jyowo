use std::sync::Arc;

use async_stream::stream;
use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::ModelError;
use serde_json::{json, Value};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::openai_protocol::{
    deepseek_chat_prefix_requested, OpenAiChatDialect, OpenAiProtocolClient,
    OpenAiProtocolProviderExt,
};
use crate::{
    AnthropicClient, InferContext, ModelCredentialResolver, ModelDescriptor, ModelProtocol,
    ModelProvider, ModelRequest, ModelStream, PromptCacheStyle,
};

const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
const DEFAULT_BETA_BASE_URL: &str = "https://api.deepseek.com/beta";
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.deepseek.com/anthropic";
const PROVIDER_ID: &str = "deepseek";
const DEFAULT_PRO_CONCURRENCY_LIMIT: usize = 500;
const DEFAULT_FLASH_CONCURRENCY_LIMIT: usize = 2_500;
pub const DEEPSEEK_API_KEY_ENV: &str = "DEEPSEEK_API_KEY";

#[derive(Clone)]
pub struct DeepSeekProvider {
    chat_client: OpenAiProtocolClient,
    completions_client: OpenAiProtocolClient,
    messages_client: AnthropicClient,
    concurrency: DeepSeekModelConcurrency,
}

impl DeepSeekProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        let api_key = api_key.into();
        Self {
            chat_client: OpenAiProtocolClient::from_api_key(api_key.clone(), DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
                .with_chat_dialect(OpenAiChatDialect::DeepSeek)
                .with_chat_completions_path("/chat/completions"),
            completions_client: OpenAiProtocolClient::from_api_key(
                api_key.clone(),
                DEFAULT_BETA_BASE_URL,
            )
            .with_provider_id(PROVIDER_ID)
            .with_completions_path("/completions"),
            messages_client: AnthropicClient::from_api_key(api_key)
                .with_provider_id(PROVIDER_ID)
                .with_base_url(DEFAULT_ANTHROPIC_BASE_URL)
                .with_messages_path("/v1/messages")
                .with_prompt_cache(false),
            concurrency: DeepSeekModelConcurrency::new(
                DEFAULT_PRO_CONCURRENCY_LIMIT,
                DEFAULT_FLASH_CONCURRENCY_LIMIT,
            ),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let base_url = base_url.into();
        self.chat_client = self.chat_client.with_base_url(base_url.clone());
        self.completions_client = self.completions_client.with_base_url(base_url.clone());
        self.messages_client = self.messages_client.with_base_url(base_url);
        self
    }

    #[must_use]
    pub fn with_credential_resolver(mut self, resolver: Arc<dyn ModelCredentialResolver>) -> Self {
        self.chat_client = self.chat_client.with_credential_resolver(resolver.clone());
        self.completions_client = self
            .completions_client
            .with_credential_resolver(resolver.clone());
        self.messages_client = self.messages_client.with_credential_resolver(resolver);
        self
    }

    #[must_use]
    pub fn with_model_concurrency_limits(mut self, pro_limit: usize, flash_limit: usize) -> Self {
        self.concurrency = DeepSeekModelConcurrency::new(pro_limit, flash_limit);
        self
    }
}

impl OpenAiProtocolProviderExt for DeepSeekProvider {
    fn client(&self) -> &OpenAiProtocolClient {
        &self.chat_client
    }
}

#[async_trait]
impl ModelProvider for DeepSeekProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        crate::catalog::provider_model_descriptors(PROVIDER_ID)
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        let req = normalize_deepseek_request(req);
        let permit = self.concurrency.acquire(&req.model_id).await?;
        let stream = match req.protocol {
            ModelProtocol::ChatCompletions => {
                if deepseek_chat_prefix_requested(&req.extra)
                    && !is_deepseek_beta_base_url(self.chat_client.base_url())
                {
                    return Err(ModelError::InvalidRequest(
                        "DeepSeek Chat Prefix requires the beta base URL".to_owned(),
                    ));
                }
                self.chat_client.infer(req, ctx).await?
            }
            ModelProtocol::Completions => self.completions_client.infer(req, ctx).await?,
            ModelProtocol::Messages => self.messages_client.infer(req, ctx).await?,
            _ => {
                return Err(ModelError::InvalidRequest(format!(
                    "DeepSeekProvider only supports chat_completions, completions, and messages, got {:?}",
                    req.protocol
                )));
            }
        };
        Ok(hold_concurrency_permit(stream, permit))
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::ChatCompletions
    }

    fn prompt_cache_style(&self) -> PromptCacheStyle {
        PromptCacheStyle::OpenAi { auto: true }
    }
}

fn is_deepseek_beta_base_url(base_url: &str) -> bool {
    base_url.trim_end_matches('/').ends_with("/beta")
}

fn normalize_deepseek_request(mut req: ModelRequest) -> ModelRequest {
    match req.model_id.as_str() {
        "deepseek-chat" => {
            req.model_id = "deepseek-v4-flash".to_owned();
            if req.extra.pointer("/thinking").is_none() {
                ensure_extra_object(&mut req.extra);
                req.extra["thinking"] = json!({ "type": "disabled" });
            }
        }
        "deepseek-reasoner" => {
            req.model_id = "deepseek-v4-flash".to_owned();
        }
        _ => {}
    }
    req
}

fn ensure_extra_object(value: &mut Value) {
    if !value.is_object() {
        *value = json!({});
    }
}

#[derive(Clone)]
struct DeepSeekModelConcurrency {
    pro: Option<Arc<Semaphore>>,
    flash: Option<Arc<Semaphore>>,
}

impl DeepSeekModelConcurrency {
    fn new(pro_limit: usize, flash_limit: usize) -> Self {
        Self {
            pro: (pro_limit > 0).then(|| Arc::new(Semaphore::new(pro_limit))),
            flash: (flash_limit > 0).then(|| Arc::new(Semaphore::new(flash_limit))),
        }
    }

    async fn acquire(&self, model_id: &str) -> Result<Option<OwnedSemaphorePermit>, ModelError> {
        let Some(semaphore) = self.semaphore_for_model(model_id) else {
            return Ok(None);
        };
        semaphore
            .acquire_owned()
            .await
            .map(Some)
            .map_err(|error| ModelError::ProviderUnavailable(error.to_string()))
    }

    fn semaphore_for_model(&self, model_id: &str) -> Option<Arc<Semaphore>> {
        if model_id.ends_with("-pro") {
            self.pro.clone()
        } else if model_id.ends_with("-flash")
            || matches!(model_id, "deepseek-chat" | "deepseek-reasoner")
        {
            self.flash.clone()
        } else {
            None
        }
    }
}

fn hold_concurrency_permit(
    stream: ModelStream,
    permit: Option<OwnedSemaphorePermit>,
) -> ModelStream {
    let Some(permit) = permit else {
        return stream;
    };

    Box::pin(stream! {
        let _permit = permit;
        let mut stream = stream;
        while let Some(event) = stream.next().await {
            yield event;
        }
    })
}

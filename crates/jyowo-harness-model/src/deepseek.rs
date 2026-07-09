use std::sync::Arc;

use async_stream::stream;
use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::ModelError;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::openai_protocol::{OpenAiChatDialect, OpenAiProtocolClient, OpenAiProtocolProviderExt};
use crate::{
    InferContext, ModelCredentialResolver, ModelDescriptor, ModelProtocol, ModelProvider,
    ModelRequest, ModelStream, PromptCacheStyle,
};

const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
const PROVIDER_ID: &str = "deepseek";
const DEFAULT_PRO_CONCURRENCY_LIMIT: usize = 500;
const DEFAULT_FLASH_CONCURRENCY_LIMIT: usize = 2_500;
pub const DEEPSEEK_API_KEY_ENV: &str = "DEEPSEEK_API_KEY";

#[derive(Clone)]
pub struct DeepSeekProvider {
    client: OpenAiProtocolClient,
    concurrency: DeepSeekModelConcurrency,
}

impl DeepSeekProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiProtocolClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
                .with_chat_dialect(OpenAiChatDialect::DeepSeek)
                .with_chat_completions_path("/chat/completions"),
            concurrency: DeepSeekModelConcurrency::new(
                DEFAULT_PRO_CONCURRENCY_LIMIT,
                DEFAULT_FLASH_CONCURRENCY_LIMIT,
            ),
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
    pub fn with_model_concurrency_limits(mut self, pro_limit: usize, flash_limit: usize) -> Self {
        self.concurrency = DeepSeekModelConcurrency::new(pro_limit, flash_limit);
        self
    }
}

impl OpenAiProtocolProviderExt for DeepSeekProvider {
    fn client(&self) -> &OpenAiProtocolClient {
        &self.client
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
        let permit = self.concurrency.acquire(&req.model_id).await?;
        let stream = self.infer_openai_protocol(req, ctx).await?;
        Ok(hold_concurrency_permit(stream, permit))
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::ChatCompletions
    }

    fn prompt_cache_style(&self) -> PromptCacheStyle {
        PromptCacheStyle::OpenAi { auto: true }
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
        } else if model_id.ends_with("-flash") {
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

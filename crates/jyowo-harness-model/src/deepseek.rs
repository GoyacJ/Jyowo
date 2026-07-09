use std::sync::Arc;

use async_stream::stream;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use harness_contracts::ModelError;
use rust_decimal::Decimal;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::openai_protocol::{OpenAiChatDialect, OpenAiProtocolClient, OpenAiProtocolProviderExt};
use crate::{
    BillingMode, ConversationModelCapability, Currency, InferContext, ModelCredentialResolver,
    ModelDescriptor, ModelLifecycle, ModelModality, ModelPricing, ModelProtocol, ModelProvider,
    ModelRequest, ModelStream, PricingSource, PromptCacheStyle, Ratio,
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
        // Verified 2026-06-21: https://api-docs.deepseek.com/quick_start/pricing
        vec![
            descriptor("deepseek-v4-flash", "DeepSeek V4 Flash", 1_000_000, 384_000),
            descriptor("deepseek-v4-pro", "DeepSeek V4 Pro", 1_000_000, 384_000),
        ]
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
            reasoning: true,
            prompt_cache: true,
            streaming: true,
            structured_output: true,
            input_modalities: vec![ModelModality::Text],
            output_modalities: vec![ModelModality::Text],
        },
        runtime_semantics: crate::ModelRuntimeSemantics::openai_chat_deepseek(),
        lifecycle: ModelLifecycle::Stable,
        pricing: Some(pricing(model_id)),
    }
}

fn pricing(model_id: &str) -> ModelPricing {
    let (input_per_million, cache_read_per_million, output_per_million, discount) =
        if model_id.ends_with("-pro") {
            (
                Decimal::new(435, 3),
                Decimal::new(3_625, 6),
                Decimal::new(87, 2),
                Ratio(0.008_333_334),
            )
        } else {
            (
                Decimal::new(14, 2),
                Decimal::new(28, 4),
                Decimal::new(28, 2),
                Ratio(0.02),
            )
        };

    ModelPricing {
        pricing_id: format!("deepseek-{model_id}-official-2026-06-21"),
        pricing_version: 1,
        currency: Currency::Usd,
        input_per_million,
        output_per_million,
        cache_creation_per_million: None,
        cache_read_per_million: Some(cache_read_per_million),
        image_per_image: None,
        last_updated: pricing_last_updated(),
        source: PricingSource::Hardcoded,
        billing_mode: BillingMode::Cached {
            cache_read_discount: discount,
        },
    }
}

fn pricing_last_updated() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-06-21T00:00:00Z")
        .expect("hardcoded DeepSeek pricing timestamp should parse")
        .with_timezone(&Utc)
}

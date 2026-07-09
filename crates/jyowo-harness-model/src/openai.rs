use async_trait::async_trait;
use chrono::{DateTime, Utc};
use harness_contracts::ModelError;
use rust_decimal::Decimal;
use std::sync::Arc;

use crate::openai_protocol::{OpenAiProtocolClient, OpenAiProtocolProviderExt};
use crate::{
    BillingMode, ConversationModelCapability, Currency, InferContext, ModelCredentialResolver,
    ModelDescriptor, ModelLifecycle, ModelModality, ModelPricing, ModelProtocol, ModelProvider,
    ModelRequest, ModelStream, PricingSource, PromptCacheStyle, Ratio,
};

const DEFAULT_BASE_URL: &str = "https://api.openai.com";
const PROVIDER_ID: &str = "openai";
const PRICING_VERSION: u32 = 20260709;
pub const OPENAI_API_KEY_ENV: &str = "OPENAI_API_KEY";

#[derive(Clone)]
pub struct OpenAiProvider {
    client: OpenAiProtocolClient,
}

impl OpenAiProvider {
    pub fn from_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: OpenAiProtocolClient::from_api_key(api_key, DEFAULT_BASE_URL)
                .with_provider_id(PROVIDER_ID)
                .with_responses_path("/v1/responses"),
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

impl OpenAiProtocolProviderExt for OpenAiProvider {
    fn client(&self) -> &OpenAiProtocolClient {
        &self.client
    }
}

#[async_trait]
impl ModelProvider for OpenAiProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn supported_models(&self) -> Vec<ModelDescriptor> {
        // Verified 2026-07-09: https://developers.openai.com/api/docs/models
        vec![
            descriptor(ModelSpec {
                model_id: "gpt-5.5-pro",
                display_name: "GPT-5.5 Pro",
                context_window: 1_050_000,
                max_output_tokens: 128_000,
                streaming: false,
                structured_output: true,
                input_per_million: "30.00",
                cached_input_per_million: None,
                output_per_million: "180.00",
            }),
            descriptor(ModelSpec {
                model_id: "gpt-5.5",
                display_name: "GPT-5.5",
                context_window: 1_050_000,
                max_output_tokens: 128_000,
                streaming: true,
                structured_output: true,
                input_per_million: "5.00",
                cached_input_per_million: Some("0.50"),
                output_per_million: "30.00",
            }),
            descriptor(ModelSpec {
                model_id: "gpt-5.4-pro",
                display_name: "GPT-5.4 Pro",
                context_window: 1_050_000,
                max_output_tokens: 128_000,
                streaming: true,
                structured_output: false,
                input_per_million: "30.00",
                cached_input_per_million: None,
                output_per_million: "180.00",
            }),
            descriptor(ModelSpec {
                model_id: "gpt-5.4",
                display_name: "GPT-5.4",
                context_window: 1_050_000,
                max_output_tokens: 128_000,
                streaming: true,
                structured_output: true,
                input_per_million: "2.50",
                cached_input_per_million: Some("0.25"),
                output_per_million: "15.00",
            }),
            descriptor(ModelSpec {
                model_id: "gpt-5.4-mini",
                display_name: "GPT-5.4 mini",
                context_window: 400_000,
                max_output_tokens: 128_000,
                streaming: true,
                structured_output: true,
                input_per_million: "0.75",
                cached_input_per_million: Some("0.075"),
                output_per_million: "4.50",
            }),
            descriptor(ModelSpec {
                model_id: "gpt-5.4-nano",
                display_name: "GPT-5.4 nano",
                context_window: 400_000,
                max_output_tokens: 128_000,
                streaming: true,
                structured_output: true,
                input_per_million: "0.20",
                cached_input_per_million: Some("0.02"),
                output_per_million: "1.25",
            }),
        ]
    }

    async fn infer(&self, req: ModelRequest, ctx: InferContext) -> Result<ModelStream, ModelError> {
        self.infer_openai_protocol(req, ctx).await
    }

    fn default_protocol(&self) -> ModelProtocol {
        ModelProtocol::Responses
    }

    fn prompt_cache_style(&self) -> PromptCacheStyle {
        PromptCacheStyle::OpenAi { auto: true }
    }
}

struct ModelSpec {
    model_id: &'static str,
    display_name: &'static str,
    context_window: u32,
    max_output_tokens: u32,
    streaming: bool,
    structured_output: bool,
    input_per_million: &'static str,
    cached_input_per_million: Option<&'static str>,
    output_per_million: &'static str,
}

fn descriptor(spec: ModelSpec) -> ModelDescriptor {
    ModelDescriptor {
        provider_id: PROVIDER_ID.to_owned(),
        model_id: spec.model_id.to_owned(),
        display_name: spec.display_name.to_owned(),
        protocol: ModelProtocol::Responses,
        context_window: spec.context_window,
        max_output_tokens: spec.max_output_tokens,
        conversation_capability: ConversationModelCapability {
            context_window: spec.context_window,
            max_output_tokens: spec.max_output_tokens,
            tool_calling: true,
            reasoning: true,
            prompt_cache: true,
            streaming: spec.streaming,
            structured_output: spec.structured_output,
            input_modalities: vec![
                ModelModality::Text,
                ModelModality::Image,
                ModelModality::File,
            ],
            output_modalities: vec![ModelModality::Text],
        },
        runtime_semantics: crate::ModelRuntimeSemantics::openai_responses_default(),
        lifecycle: ModelLifecycle::Stable,
        pricing: Some(pricing(
            spec.model_id,
            spec.input_per_million,
            spec.cached_input_per_million,
            spec.output_per_million,
        )),
    }
}

fn pricing(
    model_id: &str,
    input_per_million: &str,
    cached_input_per_million: Option<&str>,
    output_per_million: &str,
) -> ModelPricing {
    ModelPricing {
        pricing_id: format!("openai:{model_id}"),
        pricing_version: PRICING_VERSION,
        currency: Currency::Usd,
        input_per_million: decimal(input_per_million),
        output_per_million: decimal(output_per_million),
        cache_creation_per_million: None,
        cache_read_per_million: cached_input_per_million.map(decimal),
        image_per_image: None,
        last_updated: DateTime::parse_from_rfc3339("2026-07-09T00:00:00Z")
            .expect("static OpenAI pricing timestamp must be valid")
            .with_timezone(&Utc),
        source: PricingSource::Hardcoded,
        billing_mode: cached_input_per_million.map_or(BillingMode::Standard, |_| {
            BillingMode::Cached {
                cache_read_discount: Ratio(0.1),
            }
        }),
    }
}

fn decimal(value: &str) -> Decimal {
    value
        .parse()
        .expect("static OpenAI pricing decimal must be valid")
}

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;

use crate::{
    BillingMode, ConversationModelCapability, Currency, ModelDescriptor, ModelLifecycle,
    ModelModality, ModelPricing, ModelProtocol, ModelRuntimeSemantics, PricingSource, Ratio,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ModelCatalogEntry {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub protocol: ModelProtocol,
    pub supported_parameters: Vec<String>,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub provider_declared_capability: ConversationModelCapability,
    pub conversation_capability: ConversationModelCapability,
    pub runtime_semantics: ModelRuntimeSemantics,
    pub lifecycle: ModelLifecycle,
    pub pricing: Option<ModelPricing>,
    pub source_url: String,
    pub verified_at: NaiveDate,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProviderCatalogMetadata {
    pub provider_id: &'static str,
    pub source_url: &'static str,
    pub verified_at: NaiveDate,
}

#[derive(Debug, Clone, Copy)]
struct ModelCatalogSpec {
    provider_id: &'static str,
    model_id: &'static str,
    display_name: &'static str,
    protocol: ModelProtocol,
    context_window: u32,
    max_output_tokens: u32,
    tool_calling: bool,
    reasoning: bool,
    prompt_cache: bool,
    streaming: bool,
    structured_output: bool,
    declared_input_modalities: &'static [ModelModality],
    declared_output_modalities: &'static [ModelModality],
    runtime_input_modalities: &'static [ModelModality],
    runtime_output_modalities: &'static [ModelModality],
    runtime_semantics: RuntimeSemanticsKind,
    lifecycle: ModelLifecycleSpec,
}

#[derive(Debug, Clone, Copy)]
enum RuntimeSemanticsKind {
    AnthropicMessages,
    BedrockConverse,
    Gemini,
    OpenAiChatDeepSeek,
    OpenAiChatKimi,
    OpenAiChatKimiOptionalReplay,
    OpenAiChatKimiPlain,
    OpenAiChatMinimax,
    OpenAiChatPlain,
    OpenAiChatZhipu,
    OpenAiResponses,
}

#[derive(Debug, Clone, Copy)]
enum ModelLifecycleSpec {
    Preview,
    Stable,
    Retiring { retirement_date: NaiveDate },
}

const TEXT: &[ModelModality] = &[ModelModality::Text];
const TEXT_IMAGE: &[ModelModality] = &[ModelModality::Text, ModelModality::Image];
const TEXT_IMAGE_FILE: &[ModelModality] = &[
    ModelModality::Text,
    ModelModality::Image,
    ModelModality::File,
];
const TEXT_IMAGE_VIDEO: &[ModelModality] = &[
    ModelModality::Text,
    ModelModality::Image,
    ModelModality::Video,
];

pub(crate) const PROVIDER_METADATA: &[ProviderCatalogMetadata] = &[
    provider(
        "anthropic",
        "https://docs.anthropic.com/en/docs/about-claude/models/overview",
    ),
    provider(
        "bedrock",
        "https://docs.aws.amazon.com/bedrock/latest/userguide/models-supported.html",
    ),
    provider("codex", "https://developers.openai.com/api/docs/models/all"),
    provider(
        "deepseek",
        "https://api-docs.deepseek.com/quick_start/pricing",
    ),
    provider("doubao", "https://www.volcengine.com/docs/82379/1494384"),
    provider("gemini", "https://ai.google.dev/gemini-api/docs/models"),
    provider_with_date("km", "https://platform.kimi.com/docs", date(2026, 7, 9)),
    provider("local-llama", "https://ollama.com/library"),
    provider(
        "minimax",
        "https://platform.minimax.io/docs/guides/models-intro",
    ),
    provider_with_date(
        "openai",
        "https://developers.openai.com/api/docs/models",
        date(2026, 7, 9),
    ),
    provider("openrouter", "https://openrouter.ai/api/v1/models"),
    provider_with_date(
        "qwen",
        "https://help.aliyun.com/en/model-studio/text-generation-model/",
        date(2026, 7, 9),
    ),
    provider_with_date(
        "zhipu",
        "https://docs.bigmodel.cn/api-reference/模型-api/对话补全",
        date(2026, 7, 9),
    ),
];

const MODEL_SPECS: &[ModelCatalogSpec] = &[
    messages_declared_input_model(
        "anthropic",
        "claude-fable-5",
        "Claude Fable 5",
        1_000_000,
        128_000,
        true,
        true,
        true,
        TEXT_IMAGE,
        RuntimeSemanticsKind::AnthropicMessages,
    ),
    messages_declared_input_model(
        "anthropic",
        "claude-opus-4-8",
        "Claude Opus 4.8",
        1_000_000,
        128_000,
        true,
        true,
        true,
        TEXT_IMAGE,
        RuntimeSemanticsKind::AnthropicMessages,
    ),
    messages_declared_input_model(
        "anthropic",
        "claude-sonnet-4-6",
        "Claude Sonnet 4.6",
        1_000_000,
        64_000,
        true,
        true,
        true,
        TEXT_IMAGE,
        RuntimeSemanticsKind::AnthropicMessages,
    ),
    messages_declared_input_model(
        "anthropic",
        "claude-haiku-4-5",
        "Claude Haiku 4.5",
        200_000,
        64_000,
        true,
        true,
        true,
        TEXT_IMAGE,
        RuntimeSemanticsKind::AnthropicMessages,
    ),
    messages_declared_input_model(
        "bedrock",
        "anthropic.claude-3-5-sonnet-20241022-v2:0",
        "Claude 3.5 Sonnet on Bedrock",
        200_000,
        8192,
        true,
        true,
        false,
        TEXT_IMAGE,
        RuntimeSemanticsKind::BedrockConverse,
    ),
    responses_model(
        "codex",
        "gpt-5.3-codex",
        "GPT-5.3 Codex",
        200_000,
        32_000,
        true,
        true,
    ),
    chat_model(
        "deepseek",
        "deepseek-v4-flash",
        "DeepSeek V4 Flash",
        1_000_000,
        384_000,
        true,
        true,
        true,
        true,
        RuntimeSemanticsKind::OpenAiChatDeepSeek,
    ),
    chat_model(
        "deepseek",
        "deepseek-v4-pro",
        "DeepSeek V4 Pro",
        1_000_000,
        384_000,
        true,
        true,
        true,
        true,
        RuntimeSemanticsKind::OpenAiChatDeepSeek,
    ),
    chat_model(
        "doubao",
        "doubao-seed-2-0-mini-260428",
        "Doubao Seed 2.0 Mini",
        256_000,
        64_000,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "doubao",
        "doubao-seed-1-8-260116",
        "Doubao Seed 1.8",
        256_000,
        64_000,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "doubao",
        "doubao-seed-code-251201",
        "Doubao Seed Code",
        256_000,
        64_000,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "doubao",
        "doubao-seed-1.6",
        "Doubao Seed 1.6",
        256_000,
        64_000,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "doubao",
        "doubao-seed-1.6-thinking",
        "Doubao Seed 1.6 Thinking",
        256_000,
        64_000,
        true,
        true,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "doubao",
        "doubao-seed-1.6-flash",
        "Doubao Seed 1.6 Flash",
        256_000,
        64_000,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    generate_content_declared_input_model(
        "gemini",
        "gemini-2.5-pro",
        "Gemini 2.5 Pro",
        1_000_000,
        32_000,
        TEXT_IMAGE,
    ),
    generate_content_declared_input_model(
        "gemini",
        "gemini-2.5-flash",
        "Gemini 2.5 Flash",
        1_000_000,
        16_384,
        TEXT_IMAGE,
    ),
    generate_content_declared_input_model(
        "gemini",
        "gemini-2.5-flash-lite",
        "Gemini 2.5 Flash Lite",
        1_000_000,
        8192,
        TEXT_IMAGE,
    ),
    kimi_chat_model(
        "kimi-k2.7-code",
        "Kimi K2.7 Code",
        256_000,
        32_768,
        TEXT_IMAGE_VIDEO,
        true,
        true,
        true,
        RuntimeSemanticsKind::OpenAiChatKimi,
        ModelLifecycleSpec::Stable,
    ),
    kimi_chat_model(
        "kimi-k2.7-code-highspeed",
        "Kimi K2.7 Code Highspeed",
        256_000,
        32_768,
        TEXT_IMAGE_VIDEO,
        true,
        true,
        true,
        RuntimeSemanticsKind::OpenAiChatKimi,
        ModelLifecycleSpec::Stable,
    ),
    kimi_chat_model(
        "kimi-k2.6",
        "Kimi K2.6",
        256_000,
        32_768,
        TEXT_IMAGE_VIDEO,
        true,
        true,
        true,
        RuntimeSemanticsKind::OpenAiChatKimiOptionalReplay,
        ModelLifecycleSpec::Stable,
    ),
    kimi_chat_model(
        "kimi-k2.5",
        "Kimi K2.5",
        256_000,
        32_768,
        TEXT_IMAGE_VIDEO,
        true,
        true,
        true,
        RuntimeSemanticsKind::OpenAiChatKimiOptionalReplay,
        ModelLifecycleSpec::Stable,
    ),
    kimi_chat_model(
        "moonshot-v1-8k",
        "Moonshot V1 8K",
        8_192,
        4_096,
        TEXT,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatKimiPlain,
        ModelLifecycleSpec::Stable,
    ),
    kimi_chat_model(
        "moonshot-v1-32k",
        "Moonshot V1 32K",
        32_768,
        4_096,
        TEXT,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatKimiPlain,
        ModelLifecycleSpec::Stable,
    ),
    kimi_chat_model(
        "moonshot-v1-128k",
        "Moonshot V1 128K",
        131_072,
        4_096,
        TEXT,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatKimiPlain,
        ModelLifecycleSpec::Stable,
    ),
    kimi_chat_model(
        "moonshot-v1-8k-vision-preview",
        "Moonshot V1 8K Vision Preview",
        8_192,
        4_096,
        TEXT_IMAGE,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatKimiPlain,
        ModelLifecycleSpec::Preview,
    ),
    kimi_chat_model(
        "moonshot-v1-32k-vision-preview",
        "Moonshot V1 32K Vision Preview",
        32_768,
        4_096,
        TEXT_IMAGE,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatKimiPlain,
        ModelLifecycleSpec::Preview,
    ),
    kimi_chat_model(
        "moonshot-v1-128k-vision-preview",
        "Moonshot V1 128K Vision Preview",
        131_072,
        4_096,
        TEXT_IMAGE,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatKimiPlain,
        ModelLifecycleSpec::Preview,
    ),
    chat_model(
        "local-llama",
        "llama3.1",
        "Local Llama 3.1",
        128_000,
        8192,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "local-llama",
        "llama3.1:8b",
        "Local Llama 3.1 8B",
        128_000,
        8192,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    minimax_m3_model("MiniMax-M3", "MiniMax M3", 1_000_000, 524_288),
    chat_model(
        "minimax",
        "MiniMax-M2.7",
        "MiniMax M2.7",
        204_800,
        204_800,
        true,
        false,
        true,
        false,
        RuntimeSemanticsKind::OpenAiChatMinimax,
    ),
    chat_model(
        "minimax",
        "MiniMax-M2.7-highspeed",
        "MiniMax M2.7 Highspeed",
        204_800,
        204_800,
        true,
        false,
        true,
        false,
        RuntimeSemanticsKind::OpenAiChatMinimax,
    ),
    chat_model(
        "minimax",
        "MiniMax-M2.5",
        "MiniMax M2.5",
        204_800,
        204_800,
        true,
        false,
        true,
        false,
        RuntimeSemanticsKind::OpenAiChatMinimax,
    ),
    chat_model(
        "minimax",
        "MiniMax-M2.5-highspeed",
        "MiniMax M2.5 Highspeed",
        204_800,
        204_800,
        true,
        false,
        true,
        false,
        RuntimeSemanticsKind::OpenAiChatMinimax,
    ),
    chat_model(
        "minimax",
        "MiniMax-M2.1",
        "MiniMax M2.1",
        204_800,
        204_800,
        true,
        false,
        true,
        false,
        RuntimeSemanticsKind::OpenAiChatMinimax,
    ),
    chat_model(
        "minimax",
        "MiniMax-M2.1-highspeed",
        "MiniMax M2.1 Highspeed",
        204_800,
        204_800,
        true,
        false,
        true,
        false,
        RuntimeSemanticsKind::OpenAiChatMinimax,
    ),
    chat_model(
        "minimax",
        "MiniMax-M2",
        "MiniMax M2",
        204_800,
        204_800,
        true,
        false,
        true,
        false,
        RuntimeSemanticsKind::OpenAiChatMinimax,
    ),
    openai_responses_model("openai", "gpt-5.5-pro", "GPT-5.5 Pro", 1_050_000, 128_000),
    openai_responses_model("openai", "gpt-5.5", "GPT-5.5", 1_050_000, 128_000),
    openai_responses_model("openai", "gpt-5.4-pro", "GPT-5.4 Pro", 1_050_000, 128_000),
    openai_responses_model("openai", "gpt-5.4", "GPT-5.4", 1_050_000, 128_000),
    openai_responses_model("openai", "gpt-5.4-mini", "GPT-5.4 mini", 400_000, 128_000),
    openai_responses_model("openai", "gpt-5.4-nano", "GPT-5.4 nano", 400_000, 128_000),
    chat_model(
        "openrouter",
        "openai/gpt-5.5",
        "OpenAI GPT-5.5 via OpenRouter",
        128_000,
        8192,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "openrouter",
        "anthropic/claude-fable-5",
        "Claude Fable 5 via OpenRouter",
        128_000,
        8192,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "openrouter",
        "anthropic/claude-sonnet-4.6",
        "Claude Sonnet 4.6 via OpenRouter",
        128_000,
        8192,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "openrouter",
        "google/gemini-2.5-pro",
        "Gemini 2.5 Pro via OpenRouter",
        128_000,
        8192,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "openrouter",
        "deepseek/deepseek-v4-pro",
        "DeepSeek V4 Pro via OpenRouter",
        128_000,
        8192,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "openrouter",
        "moonshotai/kimi-k2.7-code",
        "Kimi K2.7 Code via OpenRouter",
        128_000,
        8192,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "openrouter",
        "z-ai/glm-5.2",
        "GLM 5.2 via OpenRouter",
        128_000,
        8192,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "openrouter",
        "minimax/minimax-m3",
        "MiniMax M3 via OpenRouter",
        128_000,
        8192,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    qwen_responses_model("qwen3-max", "Qwen3 Max", 256_000, 65_536, true, true, TEXT),
    qwen_responses_model(
        "qwen3-max-2026-01-23",
        "Qwen3 Max 2026-01-23",
        256_000,
        65_536,
        true,
        true,
        TEXT,
    ),
    qwen_responses_model(
        "qwen3.7-max",
        "Qwen3.7 Max",
        1_000_000,
        65_536,
        true,
        false,
        TEXT,
    ),
    qwen_responses_model(
        "qwen3.7-max-preview",
        "Qwen3.7 Max Preview",
        1_000_000,
        65_536,
        true,
        false,
        TEXT,
    ),
    qwen_responses_model(
        "qwen3.7-max-2026-06-08",
        "Qwen3.7 Max 2026-06-08",
        1_000_000,
        65_536,
        true,
        false,
        TEXT,
    ),
    qwen_responses_model(
        "qwen3.7-max-2026-05-20",
        "Qwen3.7 Max 2026-05-20",
        1_000_000,
        65_536,
        true,
        false,
        TEXT,
    ),
    qwen_responses_model(
        "qwen3.7-max-2026-05-17",
        "Qwen3.7 Max 2026-05-17",
        1_000_000,
        65_536,
        true,
        false,
        TEXT,
    ),
    qwen_responses_model(
        "qwen3.7-plus",
        "Qwen3.7 Plus",
        1_000_000,
        65_536,
        true,
        true,
        TEXT_IMAGE_VIDEO,
    ),
    qwen_responses_model(
        "qwen3.7-plus-2026-01-23",
        "Qwen3.7 Plus 2026-01-23",
        1_000_000,
        65_536,
        true,
        true,
        TEXT_IMAGE_VIDEO,
    ),
    qwen_responses_model(
        "qwen3.6-flash",
        "Qwen3.6 Flash",
        1_000_000,
        65_536,
        true,
        true,
        TEXT_IMAGE_VIDEO,
    ),
    qwen_responses_model(
        "qwen3.6-plus",
        "Qwen3.6 Plus",
        1_000_000,
        65_536,
        true,
        true,
        TEXT_IMAGE_VIDEO,
    ),
    qwen_responses_model(
        "qwen3.5-plus",
        "Qwen3.5 Plus",
        1_000_000,
        65_536,
        true,
        true,
        TEXT_IMAGE_VIDEO,
    ),
    qwen_responses_model(
        "qwen3.5-flash",
        "Qwen3.5 Flash",
        1_000_000,
        65_536,
        true,
        true,
        TEXT_IMAGE_VIDEO,
    ),
    qwen_responses_model(
        "qwen-plus",
        "Qwen Plus",
        1_000_000,
        65_536,
        true,
        true,
        TEXT,
    ),
    qwen_responses_model(
        "qwen-flash",
        "Qwen Flash",
        1_000_000,
        65_536,
        true,
        true,
        TEXT,
    ),
    qwen_responses_model(
        "qwen3-coder-plus",
        "Qwen3 Coder Plus",
        1_000_000,
        65_536,
        true,
        true,
        TEXT,
    ),
    qwen_responses_model(
        "qwen3-coder-flash",
        "Qwen3 Coder Flash",
        1_000_000,
        65_536,
        true,
        true,
        TEXT,
    ),
    qwen_responses_model(
        "qwen3-coder-next",
        "Qwen3 Coder Next",
        256_000,
        65_536,
        true,
        true,
        TEXT,
    ),
    qwen_responses_model(
        "qwen3.6-35b-a3b",
        "Qwen3.6 35B A3B",
        1_000_000,
        65_536,
        true,
        true,
        TEXT,
    ),
    qwen_responses_model(
        "qwen3.5-397b-a17b",
        "Qwen3.5 397B A17B",
        1_000_000,
        65_536,
        true,
        true,
        TEXT,
    ),
    qwen_responses_model(
        "qwen3.5-122b-a10b",
        "Qwen3.5 122B A10B",
        1_000_000,
        65_536,
        true,
        true,
        TEXT,
    ),
    qwen_responses_model(
        "qwen3.5-27b",
        "Qwen3.5 27B",
        1_000_000,
        65_536,
        true,
        true,
        TEXT,
    ),
    qwen_responses_model(
        "qwen3.5-35b-a3b",
        "Qwen3.5 35B A3B",
        1_000_000,
        65_536,
        true,
        true,
        TEXT,
    ),
    qwen_responses_model(
        "qwen3-vl-plus",
        "Qwen3 VL Plus",
        128_000,
        8192,
        false,
        true,
        TEXT_IMAGE_VIDEO,
    ),
    qwen_responses_model(
        "qwen3-vl-flash",
        "Qwen3 VL Flash",
        128_000,
        8192,
        false,
        true,
        TEXT_IMAGE_VIDEO,
    ),
    zhipu_chat_model(
        "glm-5.2",
        "GLM-5.2",
        1_000_000,
        131_072,
        true,
        true,
        true,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_chat_model(
        "glm-5.1",
        "GLM-5.1",
        200_000,
        131_072,
        true,
        true,
        true,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_chat_model(
        "glm-5-turbo",
        "GLM-5 Turbo",
        200_000,
        131_072,
        true,
        true,
        true,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_chat_model(
        "glm-5",
        "GLM-5",
        200_000,
        131_072,
        true,
        true,
        true,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_chat_model(
        "glm-4.7",
        "GLM-4.7",
        200_000,
        131_072,
        true,
        true,
        true,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_chat_model(
        "glm-4.7-flash",
        "GLM-4.7 Flash",
        200_000,
        131_072,
        true,
        true,
        true,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_chat_model(
        "glm-4.7-flashx",
        "GLM-4.7 FlashX",
        200_000,
        131_072,
        true,
        true,
        true,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_chat_model(
        "glm-4.6",
        "GLM-4.6",
        200_000,
        131_072,
        true,
        true,
        true,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_chat_model(
        "glm-4.5-air",
        "GLM-4.5 Air",
        128_000,
        98_304,
        true,
        true,
        true,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_chat_model(
        "glm-4.5-airx",
        "GLM-4.5 AirX",
        128_000,
        98_304,
        true,
        true,
        true,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_chat_model(
        "glm-4.5-flash",
        "GLM-4.5 Flash",
        128_000,
        98_304,
        true,
        true,
        true,
        true,
        ModelLifecycleSpec::Retiring {
            retirement_date: date(2026, 1, 30),
        },
    ),
    zhipu_chat_model(
        "glm-4-flash-250414",
        "GLM-4 Flash 250414",
        128_000,
        16_384,
        true,
        false,
        false,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_chat_model(
        "glm-4-flashx-250414",
        "GLM-4 FlashX 250414",
        128_000,
        16_384,
        true,
        false,
        false,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_vision_model(
        "glm-5v-turbo",
        "GLM-5V Turbo",
        200_000,
        131_072,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_vision_model(
        "glm-4.6v",
        "GLM-4.6V",
        128_000,
        32_768,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_vision_model(
        "autoglm-phone",
        "AutoGLM Phone",
        20_000,
        2048,
        false,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_vision_model(
        "glm-4.6v-flash",
        "GLM-4.6V Flash",
        128_000,
        32_768,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_vision_model(
        "glm-4.6v-flashx",
        "GLM-4.6V FlashX",
        128_000,
        32_768,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_vision_model(
        "glm-4v-flash",
        "GLM-4V Flash",
        16_000,
        1024,
        false,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_vision_model(
        "glm-4.1v-thinking-flashx",
        "GLM-4.1V Thinking FlashX",
        64_000,
        16_384,
        true,
        ModelLifecycleSpec::Stable,
    ),
    zhipu_vision_model(
        "glm-4.1v-thinking-flash",
        "GLM-4.1V Thinking Flash",
        64_000,
        16_384,
        true,
        ModelLifecycleSpec::Stable,
    ),
];

#[must_use]
pub fn model_catalog_entries() -> Vec<ModelCatalogEntry> {
    MODEL_SPECS.iter().map(ModelCatalogEntry::from).collect()
}

#[must_use]
#[allow(dead_code)]
pub(crate) fn provider_model_descriptors(provider_id: &str) -> Vec<ModelDescriptor> {
    MODEL_SPECS
        .iter()
        .filter(|spec| spec.provider_id == provider_id)
        .map(ModelDescriptor::from)
        .collect()
}

#[must_use]
pub(crate) fn provider_metadata(provider_id: &str) -> Option<ProviderCatalogMetadata> {
    PROVIDER_METADATA
        .iter()
        .copied()
        .find(|metadata| metadata.provider_id == provider_id)
}

impl From<&ModelCatalogSpec> for ModelCatalogEntry {
    fn from(spec: &ModelCatalogSpec) -> Self {
        let metadata = provider_metadata(spec.provider_id)
            .unwrap_or_else(|| panic!("missing provider metadata for {}", spec.provider_id));
        Self {
            provider_id: spec.provider_id.to_owned(),
            model_id: spec.model_id.to_owned(),
            display_name: spec.display_name.to_owned(),
            protocol: spec.protocol,
            supported_parameters: supported_parameters(spec),
            context_window: spec.context_window,
            max_output_tokens: spec.max_output_tokens,
            provider_declared_capability: declared_capability(spec),
            conversation_capability: runtime_capability(spec),
            runtime_semantics: runtime_semantics(spec.runtime_semantics),
            lifecycle: lifecycle(spec.lifecycle),
            pricing: pricing(spec),
            source_url: metadata.source_url.to_owned(),
            verified_at: metadata.verified_at,
        }
    }
}

impl From<&ModelCatalogSpec> for ModelDescriptor {
    fn from(spec: &ModelCatalogSpec) -> Self {
        Self {
            provider_id: spec.provider_id.to_owned(),
            model_id: spec.model_id.to_owned(),
            display_name: spec.display_name.to_owned(),
            protocol: spec.protocol,
            supported_parameters: supported_parameters(spec),
            context_window: spec.context_window,
            max_output_tokens: spec.max_output_tokens,
            provider_declared_capability: declared_capability(spec),
            conversation_capability: runtime_capability(spec),
            runtime_semantics: runtime_semantics(spec.runtime_semantics),
            lifecycle: lifecycle(spec.lifecycle),
            pricing: pricing(spec),
        }
    }
}

fn supported_parameters(spec: &ModelCatalogSpec) -> Vec<String> {
    let values: &[&str] = match spec.provider_id {
        "anthropic" => &[
            "thinking",
            "output_config",
            "service_tier",
            "stop_sequences",
            "top_k",
            "top_p",
            "tool_choice",
            "metadata",
        ],
        "bedrock" => &[
            "inferenceConfig",
            "additionalModelRequestFields",
            "additionalModelResponseFieldPaths",
            "performanceConfig",
            "requestMetadata",
        ],
        "gemini" => &[
            "thinkingConfig",
            "stopSequences",
            "topP",
            "topK",
            "seed",
            "responseMimeType",
            "responseSchema",
            "toolConfig",
            "safetySettings",
            "cachedContent",
        ],
        "qwen" => &[
            "enable_thinking",
            "reasoning",
            "tools",
            "enable_search",
            "enable_code_interpreter",
            "search_options",
        ],
        "codex" | "openai" => &[
            "reasoning",
            "text",
            "metadata",
            "service_tier",
            "store",
            "truncation",
            "parallel_tool_calls",
            "tool_choice",
            "response_format",
        ],
        "zhipu" => return zhipu_supported_parameters(spec),
        _ if spec.protocol == ModelProtocol::ChatCompletions => &[
            "temperature",
            "top_p",
            "max_tokens",
            "max_completion_tokens",
            "stop",
            "response_format",
            "tool_choice",
            "reasoning",
            "reasoning_effort",
            "service_tier",
            "stream_options",
            "frequency_penalty",
            "presence_penalty",
            "logprobs",
            "top_logprobs",
            "seed",
            "n",
        ],
        _ => &[],
    };
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn zhipu_supported_parameters(spec: &ModelCatalogSpec) -> Vec<String> {
    let mut values = vec![
        "thinking",
        "do_sample",
        "temperature",
        "top_p",
        "max_tokens",
        "tools",
        "tool_choice",
        "stop",
        "user_id",
    ];
    if spec.model_id == "glm-5.2" {
        values.push("reasoning_effort");
    }
    if matches!(
        spec.model_id,
        "glm-5.2"
            | "glm-5.1"
            | "glm-5"
            | "glm-5-turbo"
            | "glm-4.7"
            | "glm-4.7-flash"
            | "glm-4.7-flashx"
            | "glm-4.6"
    ) {
        values.push("tool_stream");
    }
    if spec.declared_input_modalities == TEXT {
        values.push("response_format");
    }
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn pricing(spec: &ModelCatalogSpec) -> Option<ModelPricing> {
    if spec.provider_id == "openai" {
        return openai_pricing(spec);
    }
    if spec.provider_id != "deepseek" {
        return None;
    }

    let (input_per_million, cache_read_per_million, output_per_million, discount) =
        if spec.model_id.ends_with("-pro") {
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

    Some(ModelPricing {
        pricing_id: format!("deepseek-{}-official-2026-06-21", spec.model_id),
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
    })
}

fn openai_pricing(spec: &ModelCatalogSpec) -> Option<ModelPricing> {
    let (input_per_million, cache_read_per_million, output_per_million) = match spec.model_id {
        "gpt-5.5-pro" | "gpt-5.4-pro" => (Decimal::new(30, 0), None, Decimal::new(180, 0)),
        "gpt-5.5" => (
            Decimal::new(5, 0),
            Some(Decimal::new(5, 1)),
            Decimal::new(30, 0),
        ),
        "gpt-5.4" => (
            Decimal::new(25, 1),
            Some(Decimal::new(25, 2)),
            Decimal::new(15, 0),
        ),
        "gpt-5.4-mini" => (
            Decimal::new(75, 2),
            Some(Decimal::new(75, 3)),
            Decimal::new(45, 1),
        ),
        "gpt-5.4-nano" => (
            Decimal::new(20, 2),
            Some(Decimal::new(2, 2)),
            Decimal::new(125, 2),
        ),
        _ => return None,
    };

    Some(ModelPricing {
        pricing_id: format!("openai:{}", spec.model_id),
        pricing_version: 20260709,
        currency: Currency::Usd,
        input_per_million,
        output_per_million,
        cache_creation_per_million: None,
        cache_read_per_million,
        image_per_image: None,
        last_updated: DateTime::parse_from_rfc3339("2026-07-09T00:00:00Z")
            .expect("hardcoded OpenAI pricing timestamp should parse")
            .with_timezone(&Utc),
        source: PricingSource::Hardcoded,
        billing_mode: cache_read_per_million.map_or(BillingMode::Standard, |_| {
            BillingMode::Cached {
                cache_read_discount: Ratio(0.1),
            }
        }),
    })
}

fn pricing_last_updated() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-06-21T00:00:00Z")
        .expect("hardcoded DeepSeek pricing timestamp should parse")
        .with_timezone(&Utc)
}

const fn provider(provider_id: &'static str, source_url: &'static str) -> ProviderCatalogMetadata {
    provider_with_date(provider_id, source_url, verified_at())
}

const fn provider_with_date(
    provider_id: &'static str,
    source_url: &'static str,
    verified_at: NaiveDate,
) -> ProviderCatalogMetadata {
    ProviderCatalogMetadata {
        provider_id,
        source_url,
        verified_at,
    }
}

const fn verified_at() -> NaiveDate {
    date(2026, 6, 21)
}

const fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    match NaiveDate::from_ymd_opt(year, month, day) {
        Some(date) => date,
        None => panic!("valid catalog date"),
    }
}

const fn messages_declared_input_model(
    provider_id: &'static str,
    model_id: &'static str,
    display_name: &'static str,
    context_window: u32,
    max_output_tokens: u32,
    tool_calling: bool,
    reasoning: bool,
    prompt_cache: bool,
    declared_input_modalities: &'static [ModelModality],
    runtime_semantics: RuntimeSemanticsKind,
) -> ModelCatalogSpec {
    model(
        provider_id,
        model_id,
        display_name,
        ModelProtocol::Messages,
        context_window,
        max_output_tokens,
        tool_calling,
        reasoning,
        prompt_cache,
        true,
        false,
        declared_input_modalities,
        TEXT,
        TEXT,
        TEXT,
        runtime_semantics,
    )
}

const fn responses_model(
    provider_id: &'static str,
    model_id: &'static str,
    display_name: &'static str,
    context_window: u32,
    max_output_tokens: u32,
    reasoning: bool,
    prompt_cache: bool,
) -> ModelCatalogSpec {
    model(
        provider_id,
        model_id,
        display_name,
        ModelProtocol::Responses,
        context_window,
        max_output_tokens,
        true,
        reasoning,
        prompt_cache,
        true,
        true,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        RuntimeSemanticsKind::OpenAiResponses,
    )
}

const fn openai_responses_model(
    provider_id: &'static str,
    model_id: &'static str,
    display_name: &'static str,
    context_window: u32,
    max_output_tokens: u32,
) -> ModelCatalogSpec {
    model(
        provider_id,
        model_id,
        display_name,
        ModelProtocol::Responses,
        context_window,
        max_output_tokens,
        true,
        true,
        true,
        true,
        true,
        TEXT_IMAGE_FILE,
        TEXT,
        TEXT_IMAGE_FILE,
        TEXT,
        RuntimeSemanticsKind::OpenAiResponses,
    )
}

#[allow(clippy::fn_params_excessive_bools)]
const fn qwen_responses_model(
    model_id: &'static str,
    display_name: &'static str,
    context_window: u32,
    max_output_tokens: u32,
    reasoning: bool,
    structured_output: bool,
    declared_input_modalities: &'static [ModelModality],
) -> ModelCatalogSpec {
    model(
        "qwen",
        model_id,
        display_name,
        ModelProtocol::Responses,
        context_window,
        max_output_tokens,
        true,
        reasoning,
        false,
        true,
        structured_output,
        declared_input_modalities,
        TEXT,
        declared_input_modalities,
        TEXT,
        RuntimeSemanticsKind::OpenAiResponses,
    )
}

#[allow(clippy::fn_params_excessive_bools)]
const fn chat_model(
    provider_id: &'static str,
    model_id: &'static str,
    display_name: &'static str,
    context_window: u32,
    max_output_tokens: u32,
    tool_calling: bool,
    reasoning: bool,
    prompt_cache: bool,
    structured_output: bool,
    runtime_semantics: RuntimeSemanticsKind,
) -> ModelCatalogSpec {
    model(
        provider_id,
        model_id,
        display_name,
        ModelProtocol::ChatCompletions,
        context_window,
        max_output_tokens,
        tool_calling,
        reasoning,
        prompt_cache,
        true,
        structured_output,
        TEXT,
        TEXT,
        TEXT,
        TEXT,
        runtime_semantics,
    )
}

#[allow(clippy::fn_params_excessive_bools)]
const fn kimi_chat_model(
    model_id: &'static str,
    display_name: &'static str,
    context_window: u32,
    max_output_tokens: u32,
    input_modalities: &'static [ModelModality],
    reasoning: bool,
    prompt_cache: bool,
    structured_output: bool,
    runtime_semantics: RuntimeSemanticsKind,
    lifecycle: ModelLifecycleSpec,
) -> ModelCatalogSpec {
    ModelCatalogSpec {
        provider_id: "km",
        model_id,
        display_name,
        protocol: ModelProtocol::ChatCompletions,
        context_window,
        max_output_tokens,
        tool_calling: true,
        reasoning,
        prompt_cache,
        streaming: true,
        structured_output,
        declared_input_modalities: input_modalities,
        declared_output_modalities: TEXT,
        runtime_input_modalities: input_modalities,
        runtime_output_modalities: TEXT,
        runtime_semantics,
        lifecycle,
    }
}

#[allow(clippy::fn_params_excessive_bools)]
const fn zhipu_chat_model(
    model_id: &'static str,
    display_name: &'static str,
    context_window: u32,
    max_output_tokens: u32,
    tool_calling: bool,
    reasoning: bool,
    prompt_cache: bool,
    structured_output: bool,
    lifecycle: ModelLifecycleSpec,
) -> ModelCatalogSpec {
    ModelCatalogSpec {
        provider_id: "zhipu",
        model_id,
        display_name,
        protocol: ModelProtocol::ChatCompletions,
        context_window,
        max_output_tokens,
        tool_calling,
        reasoning,
        prompt_cache,
        streaming: true,
        structured_output,
        declared_input_modalities: TEXT,
        declared_output_modalities: TEXT,
        runtime_input_modalities: TEXT,
        runtime_output_modalities: TEXT,
        runtime_semantics: RuntimeSemanticsKind::OpenAiChatZhipu,
        lifecycle,
    }
}

#[allow(clippy::fn_params_excessive_bools)]
const fn zhipu_vision_model(
    model_id: &'static str,
    display_name: &'static str,
    context_window: u32,
    max_output_tokens: u32,
    reasoning: bool,
    lifecycle: ModelLifecycleSpec,
) -> ModelCatalogSpec {
    ModelCatalogSpec {
        provider_id: "zhipu",
        model_id,
        display_name,
        protocol: ModelProtocol::ChatCompletions,
        context_window,
        max_output_tokens,
        tool_calling: true,
        reasoning,
        prompt_cache: true,
        streaming: true,
        structured_output: false,
        declared_input_modalities: TEXT_IMAGE,
        declared_output_modalities: TEXT,
        runtime_input_modalities: TEXT_IMAGE,
        runtime_output_modalities: TEXT,
        runtime_semantics: RuntimeSemanticsKind::OpenAiChatZhipu,
        lifecycle,
    }
}

const fn generate_content_declared_input_model(
    provider_id: &'static str,
    model_id: &'static str,
    display_name: &'static str,
    context_window: u32,
    max_output_tokens: u32,
    declared_input_modalities: &'static [ModelModality],
) -> ModelCatalogSpec {
    model(
        provider_id,
        model_id,
        display_name,
        ModelProtocol::GenerateContent,
        context_window,
        max_output_tokens,
        true,
        false,
        true,
        true,
        false,
        declared_input_modalities,
        TEXT,
        TEXT,
        TEXT,
        RuntimeSemanticsKind::Gemini,
    )
}

const fn minimax_m3_model(
    model_id: &'static str,
    display_name: &'static str,
    context_window: u32,
    max_output_tokens: u32,
) -> ModelCatalogSpec {
    model(
        "minimax",
        model_id,
        display_name,
        ModelProtocol::Responses,
        context_window,
        max_output_tokens,
        true,
        true,
        true,
        true,
        false,
        TEXT_IMAGE_VIDEO,
        TEXT,
        TEXT_IMAGE_VIDEO,
        TEXT,
        RuntimeSemanticsKind::OpenAiResponses,
    )
}

#[allow(clippy::fn_params_excessive_bools, clippy::too_many_arguments)]
const fn model(
    provider_id: &'static str,
    model_id: &'static str,
    display_name: &'static str,
    protocol: ModelProtocol,
    context_window: u32,
    max_output_tokens: u32,
    tool_calling: bool,
    reasoning: bool,
    prompt_cache: bool,
    streaming: bool,
    structured_output: bool,
    declared_input_modalities: &'static [ModelModality],
    declared_output_modalities: &'static [ModelModality],
    runtime_input_modalities: &'static [ModelModality],
    runtime_output_modalities: &'static [ModelModality],
    runtime_semantics: RuntimeSemanticsKind,
) -> ModelCatalogSpec {
    ModelCatalogSpec {
        provider_id,
        model_id,
        display_name,
        protocol,
        context_window,
        max_output_tokens,
        tool_calling,
        reasoning,
        prompt_cache,
        streaming,
        structured_output,
        declared_input_modalities,
        declared_output_modalities,
        runtime_input_modalities,
        runtime_output_modalities,
        runtime_semantics,
        lifecycle: ModelLifecycleSpec::Stable,
    }
}

fn declared_capability(spec: &ModelCatalogSpec) -> ConversationModelCapability {
    capability(
        spec,
        spec.declared_input_modalities,
        spec.declared_output_modalities,
    )
}

fn runtime_capability(spec: &ModelCatalogSpec) -> ConversationModelCapability {
    capability(
        spec,
        spec.runtime_input_modalities,
        spec.runtime_output_modalities,
    )
}

fn capability(
    spec: &ModelCatalogSpec,
    input_modalities: &[ModelModality],
    output_modalities: &[ModelModality],
) -> ConversationModelCapability {
    ConversationModelCapability {
        context_window: spec.context_window,
        max_output_tokens: spec.max_output_tokens,
        tool_calling: spec.tool_calling,
        reasoning: spec.reasoning,
        prompt_cache: spec.prompt_cache,
        streaming: spec.streaming,
        structured_output: spec.structured_output,
        input_modalities: input_modalities.to_vec(),
        output_modalities: output_modalities.to_vec(),
    }
}

fn runtime_semantics(kind: RuntimeSemanticsKind) -> ModelRuntimeSemantics {
    match kind {
        RuntimeSemanticsKind::AnthropicMessages => {
            ModelRuntimeSemantics::anthropic_messages_default()
        }
        RuntimeSemanticsKind::BedrockConverse => ModelRuntimeSemantics::bedrock_converse_default(),
        RuntimeSemanticsKind::Gemini => ModelRuntimeSemantics::gemini_default(),
        RuntimeSemanticsKind::OpenAiChatDeepSeek => ModelRuntimeSemantics::openai_chat_deepseek(),
        RuntimeSemanticsKind::OpenAiChatKimi => ModelRuntimeSemantics::openai_chat_kimi(),
        RuntimeSemanticsKind::OpenAiChatKimiOptionalReplay => {
            ModelRuntimeSemantics::openai_chat_kimi_optional_replay()
        }
        RuntimeSemanticsKind::OpenAiChatKimiPlain => {
            ModelRuntimeSemantics::openai_chat_kimi_plain()
        }
        RuntimeSemanticsKind::OpenAiChatMinimax => ModelRuntimeSemantics::openai_chat_minimax(),
        RuntimeSemanticsKind::OpenAiChatPlain => ModelRuntimeSemantics::openai_chat_plain(),
        RuntimeSemanticsKind::OpenAiChatZhipu => ModelRuntimeSemantics::openai_chat_zhipu(),
        RuntimeSemanticsKind::OpenAiResponses => ModelRuntimeSemantics::openai_responses_default(),
    }
}

fn lifecycle(spec: ModelLifecycleSpec) -> ModelLifecycle {
    match spec {
        ModelLifecycleSpec::Preview => ModelLifecycle::Preview,
        ModelLifecycleSpec::Stable => ModelLifecycle::Stable,
        ModelLifecycleSpec::Retiring { retirement_date } => {
            ModelLifecycle::Retiring { retirement_date }
        }
    }
}

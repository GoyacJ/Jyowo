use chrono::NaiveDate;

use crate::{
    ConversationModelCapability, ModelDescriptor, ModelLifecycle, ModelModality, ModelPricing,
    ModelProtocol, ModelRuntimeSemantics,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ModelCatalogEntry {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub protocol: ModelProtocol,
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
    OpenAiChatMinimax,
    OpenAiChatPlain,
    OpenAiResponses,
}

#[derive(Debug, Clone, Copy)]
enum ModelLifecycleSpec {
    Stable,
}

const TEXT: &[ModelModality] = &[ModelModality::Text];
const TEXT_IMAGE: &[ModelModality] = &[ModelModality::Text, ModelModality::Image];
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
    provider("km", "https://platform.moonshot.ai/docs"),
    provider("local-llama", "https://ollama.com/library"),
    provider(
        "minimax",
        "https://platform.minimax.io/docs/guides/models-intro",
    ),
    provider("openai", "https://platform.openai.com/docs/models"),
    provider("openrouter", "https://openrouter.ai/api/v1/models"),
    provider("qwen", "https://help.aliyun.com/zh/model-studio/models"),
    provider(
        "zhipu",
        "https://docs.bigmodel.cn/api-reference/模型-api/对话补全",
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
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatDeepSeek,
    ),
    chat_model(
        "deepseek",
        "deepseek-v4-pro",
        "DeepSeek V4 Pro",
        1_000_000,
        384_000,
        true,
        false,
        false,
        false,
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
    chat_model(
        "km",
        "kimi-k2.7-code",
        "Kimi K2.7 Code",
        200_000,
        16_384,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "km",
        "kimi-k2.7-code-highspeed",
        "Kimi K2.7 Code Highspeed",
        200_000,
        16_384,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "km",
        "kimi-k2.6",
        "Kimi K2.6",
        200_000,
        16_384,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "km",
        "kimi-k2.5",
        "Kimi K2.5",
        200_000,
        16_384,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
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
    responses_model(
        "openai",
        "gpt-5.5-pro",
        "GPT-5.5 Pro",
        1_050_000,
        128_000,
        false,
        true,
    ),
    responses_model(
        "openai", "gpt-5.5", "GPT-5.5", 1_000_000, 128_000, false, true,
    ),
    responses_model(
        "openai",
        "gpt-5.4-pro",
        "GPT-5.4 Pro",
        1_050_000,
        128_000,
        false,
        true,
    ),
    responses_model(
        "openai", "gpt-5.4", "GPT-5.4", 1_000_000, 128_000, false, true,
    ),
    responses_model(
        "openai",
        "gpt-5.4-mini",
        "GPT-5.4 mini",
        400_000,
        128_000,
        false,
        true,
    ),
    responses_model(
        "openai",
        "gpt-5.4-nano",
        "GPT-5.4 nano",
        400_000,
        128_000,
        false,
        true,
    ),
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
    chat_model(
        "qwen",
        "qwen3.7-max",
        "Qwen3.7 Max",
        1_000_000,
        32_768,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "qwen",
        "qwen3.7-max-thinking",
        "Qwen3.7 Max Thinking",
        1_000_000,
        32_768,
        true,
        true,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "qwen",
        "qwen3.7-plus",
        "Qwen3.7 Plus",
        1_000_000,
        32_768,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "qwen",
        "qwen3.6-flash",
        "Qwen3.6 Flash",
        128_000,
        8192,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "qwen",
        "qwen3-coder-plus",
        "Qwen3 Coder Plus",
        128_000,
        8192,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "zhipu",
        "glm-5.2",
        "GLM-5.2",
        1_000_000,
        131_072,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "zhipu",
        "glm-5.1",
        "GLM-5.1",
        1_000_000,
        131_072,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "zhipu",
        "glm-5-turbo",
        "GLM-5 Turbo",
        1_000_000,
        131_072,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "zhipu",
        "glm-5",
        "GLM-5",
        1_000_000,
        131_072,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "zhipu",
        "glm-4.7",
        "GLM-4.7",
        1_000_000,
        131_072,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "zhipu",
        "glm-4.6",
        "GLM-4.6",
        128_000,
        16_384,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
    chat_model(
        "zhipu",
        "glm-4.5-flash",
        "GLM-4.5 Flash",
        128_000,
        16_384,
        true,
        false,
        false,
        false,
        RuntimeSemanticsKind::OpenAiChatPlain,
    ),
];

#[must_use]
pub fn model_catalog_entries() -> Vec<ModelCatalogEntry> {
    MODEL_SPECS.iter().map(ModelCatalogEntry::from).collect()
}

#[must_use]
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
            context_window: spec.context_window,
            max_output_tokens: spec.max_output_tokens,
            provider_declared_capability: declared_capability(spec),
            conversation_capability: runtime_capability(spec),
            runtime_semantics: runtime_semantics(spec.runtime_semantics),
            lifecycle: lifecycle(spec.lifecycle),
            pricing: None,
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
            context_window: spec.context_window,
            max_output_tokens: spec.max_output_tokens,
            provider_declared_capability: declared_capability(spec),
            conversation_capability: runtime_capability(spec),
            runtime_semantics: runtime_semantics(spec.runtime_semantics),
            lifecycle: lifecycle(spec.lifecycle),
            pricing: None,
        }
    }
}

const fn provider(provider_id: &'static str, source_url: &'static str) -> ProviderCatalogMetadata {
    ProviderCatalogMetadata {
        provider_id,
        source_url,
        verified_at: verified_at(),
    }
}

const fn verified_at() -> NaiveDate {
    match NaiveDate::from_ymd_opt(2026, 6, 21) {
        Some(date) => date,
        None => panic!("valid verification date"),
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
        RuntimeSemanticsKind::OpenAiChatMinimax => ModelRuntimeSemantics::openai_chat_minimax(),
        RuntimeSemanticsKind::OpenAiChatPlain => ModelRuntimeSemantics::openai_chat_plain(),
        RuntimeSemanticsKind::OpenAiResponses => ModelRuntimeSemantics::openai_responses_default(),
    }
}

fn lifecycle(spec: ModelLifecycleSpec) -> ModelLifecycle {
    match spec {
        ModelLifecycleSpec::Stable => ModelLifecycle::Stable,
    }
}

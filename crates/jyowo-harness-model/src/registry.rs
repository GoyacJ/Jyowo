use std::collections::BTreeMap;
use std::fmt;

use chrono::NaiveDate;

use crate::{
    ConversationModelCapability, ModelDescriptor, ModelInventoryEntry, ModelModality,
    ModelProtocol, ModelProvider, ModelRuntimeStatus, ProviderAuthScheme, ProviderBaseUrlRegion,
    ProviderRuntimeCapability, ProviderServiceCapability, ProviderServiceCategory,
    ProviderServiceCostRisk, ProviderServiceExecution,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderCatalogEntry {
    pub provider_id: String,
    pub display_name: String,
    pub default_base_url: String,
    pub source_url: String,
    pub verified_date: NaiveDate,
    pub runtime_capability: ProviderRuntimeCapability,
    pub service_capabilities: Vec<ProviderServiceCapability>,
    pub models: Vec<ModelDescriptor>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderInventoryEntry {
    pub provider_id: String,
    pub display_name: String,
    pub default_base_url: String,
    pub source_url: String,
    pub verified_date: NaiveDate,
    pub models: Vec<ModelInventoryEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderBuildConfig {
    pub provider_id: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model_descriptor: Option<ModelDescriptor>,
    pub provider_defaults: Option<ProviderRequestDefaults>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProviderRequestDefaults {
    pub body: serde_json::Value,
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderRegistryError {
    UnsupportedProvider {
        provider_id: String,
    },
    UnsupportedModel {
        provider_id: String,
        model_id: String,
    },
}

impl fmt::Display for ProviderRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedProvider { .. } => {
                formatter.write_str("providerId must be a supported model provider")
            }
            Self::UnsupportedModel { .. } => {
                formatter.write_str("modelId must be supported by the selected provider")
            }
        }
    }
}

impl std::error::Error for ProviderRegistryError {}

#[must_use]
pub fn provider_catalog_entries() -> Vec<ProviderCatalogEntry> {
    #[allow(unused_mut)]
    let mut entries = Vec::new();

    #[cfg(feature = "anthropic")]
    entries.push(entry(
        "anthropic",
        "Anthropic",
        "https://api.anthropic.com",
        crate::AnthropicProvider::from_api_key("").supported_models(),
    ));
    #[cfg(feature = "bedrock")]
    entries.push(entry(
        "bedrock",
        "Bedrock",
        "https://bedrock-runtime.us-east-1.amazonaws.com",
        crate::catalog::provider_model_descriptors("bedrock"),
    ));
    #[cfg(feature = "codex")]
    entries.push(entry(
        "codex",
        "Codex",
        "https://api.openai.com",
        crate::CodexResponsesProvider::from_api_key("").supported_models(),
    ));
    #[cfg(feature = "deepseek")]
    entries.push(entry(
        "deepseek",
        "DeepSeek",
        "https://api.deepseek.com",
        crate::DeepSeekProvider::from_api_key("").supported_models(),
    ));
    #[cfg(feature = "doubao")]
    entries.push(entry(
        "doubao",
        "Doubao",
        "https://ark.cn-beijing.volces.com/api/v3",
        crate::DoubaoProvider::from_api_key("").supported_models(),
    ));
    #[cfg(feature = "gemini")]
    entries.push(entry(
        "gemini",
        "Gemini",
        "https://generativelanguage.googleapis.com",
        crate::GeminiProvider::from_api_key("").supported_models(),
    ));
    #[cfg(feature = "km")]
    entries.push(entry(
        "km",
        "Kimi",
        "https://api.moonshot.cn",
        crate::KmProvider::from_api_key("").supported_models(),
    ));
    #[cfg(feature = "local-llama")]
    entries.push(entry(
        "local-llama",
        "Local Llama",
        "http://127.0.0.1:11434",
        crate::LocalLlamaProvider::default().supported_models(),
    ));
    #[cfg(feature = "minimax")]
    entries.push(entry(
        "minimax",
        "Minimax",
        "https://api.minimaxi.com",
        crate::MinimaxProvider::from_api_key("").supported_models(),
    ));
    #[cfg(feature = "openai")]
    entries.push(entry(
        "openai",
        "OpenAI",
        "https://api.openai.com",
        crate::OpenAiProvider::from_api_key("").supported_models(),
    ));
    #[cfg(feature = "openrouter")]
    entries.push(entry(
        "openrouter",
        "OpenRouter",
        "https://openrouter.ai/api",
        crate::OpenRouterProvider::from_api_key("").supported_models(),
    ));
    #[cfg(feature = "qwen")]
    entries.push(entry(
        "qwen",
        "Qwen",
        crate::qwen::DEFAULT_BASE_URL,
        crate::QwenProvider::from_api_key("").supported_models(),
    ));
    #[cfg(feature = "zhipu")]
    entries.push(entry(
        "zhipu",
        "Zhipu",
        "https://open.bigmodel.cn/api/paas/v4",
        crate::ZhipuProvider::from_api_key("").supported_models(),
    ));

    entries
}

#[must_use]
pub fn provider_inventory_entries() -> Vec<ProviderInventoryEntry> {
    provider_catalog_entries()
        .into_iter()
        .map(|entry| {
            let (source_url, verified_date) = provider_source(&entry.provider_id);
            let mut models = entry
                .models
                .into_iter()
                .map(ModelInventoryEntry::runnable)
                .collect::<Vec<_>>();
            models.extend(inventory_only_models(&entry.provider_id));
            models.sort_by(|left, right| left.model_id.cmp(&right.model_id));
            ProviderInventoryEntry {
                provider_id: entry.provider_id,
                display_name: entry.display_name,
                default_base_url: entry.default_base_url,
                source_url: source_url.to_owned(),
                verified_date,
                models,
            }
        })
        .collect()
}

pub fn resolve_model_descriptor(
    provider_id: &str,
    model_id: &str,
) -> Result<ModelDescriptor, ProviderRegistryError> {
    let Some(entry) = provider_catalog_entries()
        .into_iter()
        .find(|entry| entry.provider_id == provider_id)
    else {
        return Err(ProviderRegistryError::UnsupportedProvider {
            provider_id: provider_id.to_owned(),
        });
    };

    entry
        .models
        .into_iter()
        .find(|model| model.model_id == model_id)
        .ok_or_else(|| ProviderRegistryError::UnsupportedModel {
            provider_id: provider_id.to_owned(),
            model_id: model_id.to_owned(),
        })
}

#[allow(unused_variables)]
pub fn build_provider(
    config: ProviderBuildConfig,
) -> Result<Box<dyn ModelProvider>, ProviderRegistryError> {
    let provider_id = config.provider_id.as_str();
    let api_key = config.api_key;
    let base_url = config.base_url.as_deref();
    let model_descriptor = config.model_descriptor;

    #[cfg(feature = "anthropic")]
    if provider_id == "anthropic" {
        let provider = crate::AnthropicProvider::from_api_key(api_key);
        return Ok(Box::new(match base_url {
            Some(base_url) => provider.with_base_url(base_url),
            None => provider,
        }));
    }
    #[cfg(feature = "bedrock")]
    if provider_id == "bedrock" {
        let provider = crate::BedrockProvider::new();
        return Ok(Box::new(match base_url {
            Some(base_url) => provider.with_base_url(base_url),
            None => provider,
        }));
    }
    #[cfg(feature = "codex")]
    if provider_id == "codex" {
        let provider = crate::CodexResponsesProvider::from_api_key(api_key);
        return Ok(Box::new(match base_url {
            Some(base_url) => provider.with_base_url(base_url),
            None => provider,
        }));
    }
    #[cfg(feature = "deepseek")]
    if provider_id == "deepseek" {
        let provider = crate::DeepSeekProvider::from_api_key(api_key);
        return Ok(Box::new(match base_url {
            Some(base_url) => provider.with_base_url(base_url),
            None => provider,
        }));
    }
    #[cfg(feature = "doubao")]
    if provider_id == "doubao" {
        let provider = crate::DoubaoProvider::from_api_key(api_key);
        return Ok(Box::new(match base_url {
            Some(base_url) => provider.with_base_url(base_url),
            None => provider,
        }));
    }
    #[cfg(feature = "gemini")]
    if provider_id == "gemini" {
        let provider = crate::GeminiProvider::from_api_key(api_key);
        return Ok(Box::new(match base_url {
            Some(base_url) => provider.with_base_url(base_url),
            None => provider,
        }));
    }
    #[cfg(feature = "km")]
    if provider_id == "km" {
        let api_key = if api_key.trim().is_empty() {
            crate::kimi_api_key_from_env().unwrap_or(api_key)
        } else {
            api_key
        };
        let provider = crate::KmProvider::from_api_key(api_key);
        return Ok(Box::new(match base_url {
            Some(base_url) => provider.with_base_url(base_url),
            None => provider,
        }));
    }
    #[cfg(feature = "local-llama")]
    if provider_id == "local-llama" {
        let provider = crate::LocalLlamaProvider::default().with_api_key(api_key);
        return Ok(Box::new(match base_url {
            Some(base_url) => provider.with_base_url(base_url),
            None => provider,
        }));
    }
    #[cfg(feature = "minimax")]
    if provider_id == "minimax" {
        let provider = crate::MinimaxProvider::from_api_key(api_key);
        return Ok(Box::new(match base_url {
            Some(base_url) => provider.with_base_url(base_url),
            None => provider,
        }));
    }
    #[cfg(feature = "openai")]
    if provider_id == "openai" {
        let provider = crate::OpenAiProvider::from_api_key(api_key);
        return Ok(Box::new(match base_url {
            Some(base_url) => provider.with_base_url(base_url),
            None => provider,
        }));
    }
    #[cfg(feature = "openrouter")]
    if provider_id == "openrouter" {
        let mut provider = crate::OpenRouterProvider::from_api_key(api_key);
        if let Some(descriptor) = model_descriptor {
            provider = provider.with_model_descriptor(descriptor);
        }
        return Ok(Box::new(match base_url {
            Some(base_url) => provider.with_base_url(base_url),
            None => provider,
        }));
    }
    #[cfg(feature = "qwen")]
    if provider_id == "qwen" {
        let defaults = config.provider_defaults.unwrap_or_default();
        let provider =
            crate::QwenProvider::from_api_key(api_key).with_default_headers(defaults.headers);
        return Ok(Box::new(match base_url {
            Some(base_url) => provider.with_base_url(base_url),
            None => provider,
        }));
    }
    #[cfg(feature = "zhipu")]
    if provider_id == "zhipu" {
        let provider = crate::ZhipuProvider::from_api_key(api_key);
        return Ok(Box::new(match base_url {
            Some(base_url) => provider.with_base_url(base_url),
            None => provider,
        }));
    }

    Err(ProviderRegistryError::UnsupportedProvider {
        provider_id: config.provider_id,
    })
}

#[allow(dead_code)]
fn entry(
    provider_id: &str,
    display_name: &str,
    default_base_url: &str,
    models: Vec<ModelDescriptor>,
) -> ProviderCatalogEntry {
    let (source_url, verified_date) = provider_source(provider_id);
    ProviderCatalogEntry {
        provider_id: provider_id.to_owned(),
        display_name: display_name.to_owned(),
        default_base_url: default_base_url.to_owned(),
        source_url: source_url.to_owned(),
        verified_date,
        runtime_capability: runtime_capability(provider_id, display_name, default_base_url),
        service_capabilities: service_capabilities(provider_id),
        models,
    }
}

fn runtime_capability(
    provider_id: &str,
    display_name: &str,
    default_base_url: &str,
) -> ProviderRuntimeCapability {
    let base_url_regions = if provider_id == "minimax" {
        vec![
            ProviderBaseUrlRegion {
                id: "cn".to_owned(),
                label: "China".to_owned(),
                base_url: "https://api.minimaxi.com".to_owned(),
            },
            ProviderBaseUrlRegion {
                id: "global".to_owned(),
                label: "Global".to_owned(),
                base_url: "https://api.minimax.io".to_owned(),
            },
        ]
    } else if provider_id == "zhipu" {
        vec![
            ProviderBaseUrlRegion {
                id: "default".to_owned(),
                label: "BigModel Chat".to_owned(),
                base_url: default_base_url.to_owned(),
            },
            ProviderBaseUrlRegion {
                id: "coding-plan".to_owned(),
                label: "BigModel Coding Plan".to_owned(),
                base_url: "https://open.bigmodel.cn/api/coding/paas/v4".to_owned(),
            },
            ProviderBaseUrlRegion {
                id: "zai-coding".to_owned(),
                label: "Z.ai Coding".to_owned(),
                base_url: "https://api.z.ai/api/coding/paas/v4".to_owned(),
            },
        ]
    } else if provider_id == "qwen" {
        qwen_base_url_regions()
    } else {
        vec![ProviderBaseUrlRegion {
            id: "default".to_owned(),
            label: display_name.to_owned(),
            base_url: default_base_url.to_owned(),
        }]
    };

    ProviderRuntimeCapability {
        auth_scheme: provider_auth_scheme(provider_id),
        base_url_regions,
        supports_live_validation: provider_id == "km",
        supports_streaming_validation: true,
        secret_reveal_supported: true,
    }
}

fn qwen_base_url_regions() -> Vec<ProviderBaseUrlRegion> {
    vec![
        ProviderBaseUrlRegion {
            id: "us".to_owned(),
            label: "US Virginia".to_owned(),
            base_url: "https://dashscope-us.aliyuncs.com/compatible-mode/v1".to_owned(),
        },
        ProviderBaseUrlRegion {
            id: "beijing".to_owned(),
            label: "China Beijing".to_owned(),
            base_url: "https://{WorkspaceId}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1"
                .to_owned(),
        },
        ProviderBaseUrlRegion {
            id: "singapore".to_owned(),
            label: "Singapore".to_owned(),
            base_url: "https://{WorkspaceId}.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1"
                .to_owned(),
        },
        ProviderBaseUrlRegion {
            id: "hong-kong".to_owned(),
            label: "Hong Kong".to_owned(),
            base_url: "https://{WorkspaceId}.cn-hongkong.maas.aliyuncs.com/compatible-mode/v1"
                .to_owned(),
        },
        ProviderBaseUrlRegion {
            id: "germany".to_owned(),
            label: "Germany Frankfurt".to_owned(),
            base_url: "https://{WorkspaceId}.eu-central-1.maas.aliyuncs.com/compatible-mode/v1"
                .to_owned(),
        },
        ProviderBaseUrlRegion {
            id: "japan".to_owned(),
            label: "Japan Tokyo".to_owned(),
            base_url: "https://{WorkspaceId}.ap-northeast-1.maas.aliyuncs.com/compatible-mode/v1"
                .to_owned(),
        },
        ProviderBaseUrlRegion {
            id: "legacy".to_owned(),
            label: "Legacy DashScope".to_owned(),
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".to_owned(),
        },
    ]
}

fn provider_auth_scheme(provider_id: &str) -> ProviderAuthScheme {
    match provider_id {
        "anthropic" => ProviderAuthScheme::XApiKey,
        "gemini" => ProviderAuthScheme::ApiKey,
        "local-llama" => ProviderAuthScheme::None,
        "bedrock" => ProviderAuthScheme::None,
        _ => ProviderAuthScheme::Bearer,
    }
}

fn service_capabilities(provider_id: &str) -> Vec<ProviderServiceCapability> {
    match provider_id {
        "km" => kimi_service_capabilities(),
        "minimax" => minimax_service_capabilities(),
        "doubao" => seedance_service_capabilities(),
        _ => Vec::new(),
    }
}

fn kimi_service_capabilities() -> Vec<ProviderServiceCapability> {
    vec![
        kimi_service(
            "kimi.models.list",
            ProviderServiceCategory::Model,
            vec![ModelModality::Text],
            ModelModality::Text,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        kimi_service(
            "kimi.tokenizers.estimate_token_count",
            ProviderServiceCategory::Conversation,
            vec![
                ModelModality::Text,
                ModelModality::Image,
                ModelModality::Video,
            ],
            ModelModality::Text,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        kimi_service(
            "kimi.balance.retrieve",
            ProviderServiceCategory::Model,
            vec![ModelModality::Text],
            ModelModality::Text,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
    ]
}

fn minimax_service_capabilities() -> Vec<ProviderServiceCapability> {
    vec![
        service(
            "minimax.image_generation",
            ProviderServiceCategory::Image,
            vec![ModelModality::Text, ModelModality::Image],
            ModelModality::Image,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::High,
        ),
        service(
            "minimax.video_generation",
            ProviderServiceCategory::Video,
            vec![
                ModelModality::Text,
                ModelModality::Image,
                ModelModality::Video,
            ],
            ModelModality::Video,
            ProviderServiceExecution::AsyncJob,
            true,
            ProviderServiceCostRisk::High,
        ),
        service(
            "minimax.video_generation.query",
            ProviderServiceCategory::Video,
            vec![ModelModality::Text],
            ModelModality::Video,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        service(
            "minimax.video_template",
            ProviderServiceCategory::Video,
            vec![ModelModality::Text, ModelModality::Image],
            ModelModality::Video,
            ProviderServiceExecution::AsyncJob,
            true,
            ProviderServiceCostRisk::High,
        ),
        service(
            "minimax.video_template.query",
            ProviderServiceCategory::Video,
            vec![ModelModality::Text],
            ModelModality::Video,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        service(
            "minimax.text_to_speech.sync",
            ProviderServiceCategory::Audio,
            vec![ModelModality::Text],
            ModelModality::Audio,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Medium,
        ),
        service(
            "minimax.text_to_speech.async",
            ProviderServiceCategory::Audio,
            vec![ModelModality::Text],
            ModelModality::Audio,
            ProviderServiceExecution::AsyncJob,
            true,
            ProviderServiceCostRisk::Medium,
        ),
        service(
            "minimax.text_to_speech.async.query",
            ProviderServiceCategory::Audio,
            vec![ModelModality::Text],
            ModelModality::Audio,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        service(
            "minimax.voice_clone",
            ProviderServiceCategory::Audio,
            vec![ModelModality::Audio],
            ModelModality::Audio,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Medium,
        ),
        service(
            "minimax.voice_design",
            ProviderServiceCategory::Audio,
            vec![ModelModality::Text],
            ModelModality::Audio,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Medium,
        ),
        service(
            "minimax.voice_list",
            ProviderServiceCategory::Audio,
            vec![ModelModality::Text],
            ModelModality::Text,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        service(
            "minimax.voice_delete",
            ProviderServiceCategory::Audio,
            vec![ModelModality::Text],
            ModelModality::Text,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        service(
            "minimax.lyrics_generation",
            ProviderServiceCategory::Music,
            vec![ModelModality::Text],
            ModelModality::Text,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Medium,
        ),
        service(
            "minimax.music_generation",
            ProviderServiceCategory::Music,
            vec![ModelModality::Text, ModelModality::Audio],
            ModelModality::Audio,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::High,
        ),
        service(
            "minimax.music_cover_preprocess",
            ProviderServiceCategory::Music,
            vec![ModelModality::Audio],
            ModelModality::Audio,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Medium,
        ),
        service(
            "minimax.files.upload",
            ProviderServiceCategory::File,
            vec![ModelModality::File],
            ModelModality::File,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        service(
            "minimax.files.list",
            ProviderServiceCategory::File,
            vec![ModelModality::Text],
            ModelModality::File,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        service(
            "minimax.files.retrieve",
            ProviderServiceCategory::File,
            vec![ModelModality::Text],
            ModelModality::File,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        service(
            "minimax.files.delete",
            ProviderServiceCategory::File,
            vec![ModelModality::Text],
            ModelModality::Text,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        service(
            "minimax.models.list",
            ProviderServiceCategory::Model,
            vec![ModelModality::Text],
            ModelModality::Text,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        service(
            "minimax.models.retrieve",
            ProviderServiceCategory::Model,
            vec![ModelModality::Text],
            ModelModality::Text,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        service(
            "minimax.responses",
            ProviderServiceCategory::Conversation,
            vec![ModelModality::Text, ModelModality::Image],
            ModelModality::Text,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Medium,
        ),
        service(
            "minimax.responses.input_tokens",
            ProviderServiceCategory::Conversation,
            vec![ModelModality::Text, ModelModality::Image],
            ModelModality::Text,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        service(
            "minimax.anthropic_protocol",
            ProviderServiceCategory::Conversation,
            vec![ModelModality::Text, ModelModality::Image],
            ModelModality::Text,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Medium,
        ),
        service(
            "minimax.anthropic_protocol.count_tokens",
            ProviderServiceCategory::Conversation,
            vec![ModelModality::Text, ModelModality::Image],
            ModelModality::Text,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        service(
            "minimax.anthropic_protocol.models.list",
            ProviderServiceCategory::Model,
            vec![ModelModality::Text],
            ModelModality::Text,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
        service(
            "minimax.anthropic_protocol.models.retrieve",
            ProviderServiceCategory::Model,
            vec![ModelModality::Text],
            ModelModality::Text,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
    ]
}

fn seedance_service_capabilities() -> Vec<ProviderServiceCapability> {
    vec![
        doubao_service(
            "seedance.video_generation",
            ProviderServiceCategory::Video,
            vec![ModelModality::Text, ModelModality::Image],
            ModelModality::Video,
            ProviderServiceExecution::AsyncJob,
            true,
            ProviderServiceCostRisk::High,
        ),
        doubao_service(
            "seedance.video_generation.query",
            ProviderServiceCategory::Video,
            vec![ModelModality::Text],
            ModelModality::Video,
            ProviderServiceExecution::Sync,
            false,
            ProviderServiceCostRisk::Low,
        ),
    ]
}

fn service(
    operation_id: &str,
    category: ProviderServiceCategory,
    input_modalities: Vec<ModelModality>,
    output_artifact: ModelModality,
    execution: ProviderServiceExecution,
    requires_polling: bool,
    cost_risk: ProviderServiceCostRisk,
) -> ProviderServiceCapability {
    ProviderServiceCapability {
        operation_id: operation_id.to_owned(),
        category,
        input_modalities,
        output_artifact,
        execution,
        requires_polling,
        permission_subject: "network:minimax".to_owned(),
        cost_risk,
    }
}

fn doubao_service(
    operation_id: &str,
    category: ProviderServiceCategory,
    input_modalities: Vec<ModelModality>,
    output_artifact: ModelModality,
    execution: ProviderServiceExecution,
    requires_polling: bool,
    cost_risk: ProviderServiceCostRisk,
) -> ProviderServiceCapability {
    ProviderServiceCapability {
        operation_id: operation_id.to_owned(),
        category,
        input_modalities,
        output_artifact,
        execution,
        requires_polling,
        permission_subject: "network:doubao".to_owned(),
        cost_risk,
    }
}

fn kimi_service(
    operation_id: &str,
    category: ProviderServiceCategory,
    input_modalities: Vec<ModelModality>,
    output_artifact: ModelModality,
    execution: ProviderServiceExecution,
    requires_polling: bool,
    cost_risk: ProviderServiceCostRisk,
) -> ProviderServiceCapability {
    ProviderServiceCapability {
        operation_id: operation_id.to_owned(),
        category,
        input_modalities,
        output_artifact,
        execution,
        requires_polling,
        permission_subject: "network:kimi".to_owned(),
        cost_risk,
    }
}

fn provider_source(provider_id: &str) -> (&'static str, NaiveDate) {
    crate::catalog::provider_metadata(provider_id)
        .map(|metadata| (metadata.source_url, metadata.verified_at))
        .unwrap_or_else(|| {
            (
                "https://jyowo.local/provider-catalog",
                NaiveDate::from_ymd_opt(2026, 6, 21).expect("valid verification date"),
            )
        })
}

fn inventory_only_models(provider_id: &str) -> Vec<ModelInventoryEntry> {
    match provider_id {
        "openai" => vec![
            unsupported_model(
                "openai",
                "gpt-image-1",
                "GPT Image 1",
                vec![ModelModality::Text, ModelModality::Image],
                vec![ModelModality::Image],
                "image generation output is not supported by the current runtime",
            ),
            unsupported_model(
                "openai",
                "gpt-4o-transcribe",
                "GPT-4o Transcribe",
                vec![ModelModality::Audio],
                vec![ModelModality::Text],
                "audio input is not supported by the current runtime",
            ),
            unsupported_model(
                "openai",
                "gpt-4o-mini-tts",
                "GPT-4o mini TTS",
                vec![ModelModality::Text],
                vec![ModelModality::Audio],
                "audio output is not supported by the current runtime",
            ),
            unsupported_model(
                "openai",
                "text-embedding-3-large",
                "text-embedding-3-large",
                vec![ModelModality::Text],
                vec![ModelModality::Embedding],
                "embedding output is not supported by the current runtime",
            ),
        ],
        "gemini" => vec![
            unsupported_model(
                "gemini",
                "gemini-2.5-flash-tts-preview",
                "Gemini 2.5 Flash TTS Preview",
                vec![ModelModality::Text],
                vec![ModelModality::Audio],
                "audio output is not supported by the current runtime",
            ),
            unsupported_model(
                "gemini",
                "gemini-2.5-pro-tts-preview",
                "Gemini 2.5 Pro TTS Preview",
                vec![ModelModality::Text],
                vec![ModelModality::Audio],
                "audio output is not supported by the current runtime",
            ),
        ],
        "qwen" => qwen_unsupported_image_models(),
        _ => Vec::new(),
    }
}

fn qwen_unsupported_image_models() -> Vec<ModelInventoryEntry> {
    [
        ("qwen-image-2.0-pro", "Qwen Image 2.0 Pro"),
        (
            "qwen-image-2.0-pro-2026-03-03",
            "Qwen Image 2.0 Pro 2026-03-03",
        ),
        (
            "qwen-image-2.0-pro-2026-04-22",
            "Qwen Image 2.0 Pro 2026-04-22",
        ),
        ("qwen-image-2.0", "Qwen Image 2.0"),
        ("qwen-image-2.0-2026-03-03", "Qwen Image 2.0 2026-03-03"),
        ("qwen-image-max", "Qwen Image Max"),
        ("qwen-image-max-2025-12-30", "Qwen Image Max 2025-12-30"),
        ("qwen-image-plus", "Qwen Image Plus"),
        ("qwen-image-plus-2026-01-09", "Qwen Image Plus 2026-01-09"),
        ("qwen-image", "Qwen Image"),
        ("qwen-image-edit-max", "Qwen Image Edit Max"),
        (
            "qwen-image-edit-max-2026-01-16",
            "Qwen Image Edit Max 2026-01-16",
        ),
        ("qwen-image-edit-plus", "Qwen Image Edit Plus"),
        (
            "qwen-image-edit-plus-2025-12-15",
            "Qwen Image Edit Plus 2025-12-15",
        ),
        (
            "qwen-image-edit-plus-2025-10-30",
            "Qwen Image Edit Plus 2025-10-30",
        ),
        ("qwen-image-edit", "Qwen Image Edit"),
    ]
    .into_iter()
    .map(|(model_id, display_name)| {
        unsupported_model(
            "qwen",
            model_id,
            display_name,
            vec![ModelModality::Text, ModelModality::Image],
            vec![ModelModality::Image],
            "image output is not supported by the current runtime",
        )
    })
    .collect()
}

fn unsupported_model(
    provider_id: &str,
    model_id: &str,
    display_name: &str,
    input_modalities: Vec<ModelModality>,
    output_modalities: Vec<ModelModality>,
    reason: &str,
) -> ModelInventoryEntry {
    let mut conversation_capability = ConversationModelCapability {
        streaming: false,
        input_modalities,
        output_modalities,
        ..ConversationModelCapability::default()
    };
    conversation_capability.tool_calling = false;
    ModelInventoryEntry::unsupported(
        provider_id,
        model_id,
        display_name,
        ModelProtocol::ChatCompletions,
        conversation_capability,
        reason,
    )
}

#[must_use]
pub fn runnable_inventory_models(models: &[ModelInventoryEntry]) -> Vec<ModelDescriptor> {
    models
        .iter()
        .filter_map(|model| match model.runtime_status {
            ModelRuntimeStatus::Runnable => Some(ModelDescriptor {
                provider_id: model.provider_id.clone(),
                model_id: model.model_id.clone(),
                display_name: model.display_name.clone(),
                protocol: model.protocol,
                supported_parameters: model.supported_parameters.clone(),
                context_window: model.context_window,
                max_output_tokens: model.max_output_tokens,
                provider_declared_capability: model.provider_declared_capability.clone(),
                conversation_capability: model.conversation_capability.clone(),
                runtime_semantics: model.runtime_semantics.clone(),
                lifecycle: model.lifecycle.clone(),
                pricing: model.pricing.clone(),
            }),
            ModelRuntimeStatus::Unsupported { .. } => None,
        })
        .collect()
}

#![allow(dead_code)]
#![allow(unused_imports)]

use super::*;

pub(crate) fn openrouter_descriptor_record(
    model_id: &str,
    input_modalities: Vec<ProviderModelModalityRecord>,
    output_modalities: Vec<ProviderModelModalityRecord>,
    supports_streaming: bool,
) -> ProviderModelDescriptorRecord {
    ProviderModelDescriptorRecord {
        protocol: ModelProtocol::ChatCompletions,
        conversation_capability: ConversationModelCapabilityRecord {
            input_modalities,
            output_modalities,
            context_window: 128_000,
            max_output_tokens: 8_192,
            streaming: supports_streaming,
            tool_calling: true,
            reasoning: false,
            prompt_cache: false,
            structured_output: true,
        },
        context_window: 128_000,
        display_name: "Dynamic OpenRouter model".to_owned(),
        lifecycle: ProviderModelLifecycleRecord::Stable,
        max_output_tokens: 8_192,
        model_id: model_id.to_owned(),
        provider_id: "openrouter".to_owned(),
    }
}

pub(crate) fn openai_descriptor_record(model_id: &str) -> ProviderModelDescriptorRecord {
    ProviderModelDescriptorRecord {
        protocol: ModelProtocol::Responses,
        conversation_capability: ConversationModelCapabilityRecord {
            input_modalities: vec![ProviderModelModalityRecord::Text],
            output_modalities: vec![ProviderModelModalityRecord::Text],
            context_window: 128_000,
            max_output_tokens: 16_384,
            streaming: true,
            tool_calling: true,
            reasoning: false,
            prompt_cache: true,
            structured_output: true,
        },
        context_window: 128_000,
        display_name: "GPT-5.4 mini".to_owned(),
        lifecycle: ProviderModelLifecycleRecord::Stable,
        max_output_tokens: 16_384,
        model_id: model_id.to_owned(),
        provider_id: "openai".to_owned(),
    }
}

pub(crate) fn provider_settings_record_with_minimax_config(
    config_id: &str,
    has_api_key: bool,
) -> ProviderSettingsRecord {
    ProviderSettingsRecord {
        default_config_id: Some(config_id.to_owned()),
        configs: vec![ProviderConfigRecord {
            api_key: if has_api_key {
                "provider-test-token".to_owned()
            } else {
                String::new()
            },
            protocol: ModelProtocol::ChatCompletions,
            base_url: None,
            display_name: "MiniMax service".to_owned(),
            id: config_id.to_owned(),
            model_id: "minimax-text-01".to_owned(),
            official_quota_api_key: None,
            provider_id: "minimax".to_owned(),
            provider_defaults: None,
            model_descriptor: ProviderModelDescriptorRecord {
                protocol: ModelProtocol::ChatCompletions,
                conversation_capability: ConversationModelCapabilityRecord {
                    input_modalities: vec![ProviderModelModalityRecord::Text],
                    output_modalities: vec![ProviderModelModalityRecord::Text],
                    context_window: 1_000_000,
                    max_output_tokens: 8_192,
                    streaming: true,
                    tool_calling: true,
                    reasoning: false,
                    prompt_cache: false,
                    structured_output: true,
                },
                context_window: 1_000_000,
                display_name: "MiniMax text".to_owned(),
                lifecycle: ProviderModelLifecycleRecord::Stable,
                max_output_tokens: 8_192,
                model_id: "minimax-text-01".to_owned(),
                provider_id: "minimax".to_owned(),
            },
        }],
    }
}

pub(crate) fn test_provider_settings_record() -> ProviderSettingsRecord {
    ProviderSettingsRecord {
        default_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
        configs: vec![ProviderConfigRecord {
            api_key: "provider-test-token".to_owned(),
            protocol: ModelProtocol::Messages,
            base_url: None,
            display_name: "Test provider".to_owned(),
            id: TEST_MODEL_CONFIG_ID.to_owned(),
            model_id: "test-model".to_owned(),
            official_quota_api_key: None,
            provider_id: "test".to_owned(),
            provider_defaults: None,
            model_descriptor: ProviderModelDescriptorRecord {
                protocol: ModelProtocol::Messages,
                conversation_capability: ConversationModelCapabilityRecord {
                    input_modalities: vec![ProviderModelModalityRecord::Text],
                    output_modalities: vec![ProviderModelModalityRecord::Text],
                    context_window: 128_000,
                    max_output_tokens: 16_384,
                    streaming: true,
                    tool_calling: true,
                    reasoning: false,
                    prompt_cache: false,
                    structured_output: true,
                },
                context_window: 128_000,
                display_name: "Test model".to_owned(),
                lifecycle: ProviderModelLifecycleRecord::Stable,
                max_output_tokens: 16_384,
                model_id: "test-model".to_owned(),
                provider_id: "test".to_owned(),
            },
        }],
    }
}

pub(crate) fn chat_provider_config_record(
    config_id: &str,
    provider_id: &str,
    model_id: &str,
    display_name: &str,
    base_url: Option<String>,
    api_key: &str,
) -> ProviderConfigRecord {
    ProviderConfigRecord {
        api_key: api_key.to_owned(),
        protocol: ModelProtocol::ChatCompletions,
        base_url,
        display_name: display_name.to_owned(),
        id: config_id.to_owned(),
        model_id: model_id.to_owned(),
        official_quota_api_key: None,
        provider_id: provider_id.to_owned(),
        provider_defaults: None,
        model_descriptor: ProviderModelDescriptorRecord {
            protocol: ModelProtocol::ChatCompletions,
            conversation_capability: ConversationModelCapabilityRecord {
                input_modalities: vec![ProviderModelModalityRecord::Text],
                output_modalities: vec![ProviderModelModalityRecord::Text],
                context_window: 128_000,
                max_output_tokens: 8192,
                streaming: true,
                tool_calling: true,
                reasoning: false,
                prompt_cache: false,
                structured_output: false,
            },
            context_window: 128_000,
            display_name: display_name.to_owned(),
            lifecycle: ProviderModelLifecycleRecord::Stable,
            max_output_tokens: 8192,
            model_id: model_id.to_owned(),
            provider_id: provider_id.to_owned(),
        },
    }
}

pub(crate) fn write_test_provider_settings(workspace: &Path) {
    let workspace = workspace
        .canonicalize()
        .expect("test workspace should canonicalize");
    let store = provider_settings_store_for_workspace(&workspace);
    if store
        .load_record()
        .expect("test provider settings should load")
        .is_some()
    {
        return;
    }
    store
        .save_record(&test_provider_settings_record())
        .expect("test provider settings should save");
}

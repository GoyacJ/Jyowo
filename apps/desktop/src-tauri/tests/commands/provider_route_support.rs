#![allow(dead_code)]
#![allow(unused_imports)]

use super::provider_support::*;
use super::support::*;
use super::*;

pub(crate) fn minimax_image_route(config_id: &str, enabled: bool) -> ProviderCapabilityRoute {
    ProviderCapabilityRoute {
        kind: CapabilityRouteKind::ImageGeneration,
        config_id: config_id.to_owned(),
        provider_id: "minimax".to_owned(),
        operation_ids: vec!["minimax.image_generation".to_owned()],
        enabled,
    }
}

pub(crate) fn minimax_image_adapter_availability() -> ProviderServiceAdapterAvailability {
    ProviderServiceAdapterAvailability {
        bindings: vec![ToolServiceBinding {
            provider_id: "minimax".to_owned(),
            operation_id: "minimax.image_generation".to_owned(),
            route_kind: CapabilityRouteKind::ImageGeneration,
            output_artifact: ModelModality::Image,
        }],
    }
}

pub(crate) fn minimax_image_and_video_adapter_availability() -> ProviderServiceAdapterAvailability {
    ProviderServiceAdapterAvailability {
        bindings: vec![
            ToolServiceBinding {
                provider_id: "minimax".to_owned(),
                operation_id: "minimax.image_generation".to_owned(),
                route_kind: CapabilityRouteKind::ImageGeneration,
                output_artifact: ModelModality::Image,
            },
            ToolServiceBinding {
                provider_id: "minimax".to_owned(),
                operation_id: "minimax.video_generation".to_owned(),
                route_kind: CapabilityRouteKind::VideoGeneration,
                output_artifact: ModelModality::Video,
            },
            ToolServiceBinding {
                provider_id: "minimax".to_owned(),
                operation_id: "minimax.video_generation.query".to_owned(),
                route_kind: CapabilityRouteKind::VideoGeneration,
                output_artifact: ModelModality::Video,
            },
        ],
    }
}

pub(crate) fn doubao_seedance_adapter_availability() -> ProviderServiceAdapterAvailability {
    ProviderServiceAdapterAvailability {
        bindings: vec![
            ToolServiceBinding {
                provider_id: "doubao".to_owned(),
                operation_id: "seedance.video_generation".to_owned(),
                route_kind: CapabilityRouteKind::VideoGeneration,
                output_artifact: ModelModality::Video,
            },
            ToolServiceBinding {
                provider_id: "doubao".to_owned(),
                operation_id: "seedance.video_generation.query".to_owned(),
                route_kind: CapabilityRouteKind::VideoGeneration,
                output_artifact: ModelModality::Video,
            },
        ],
    }
}

pub(crate) fn gemini_service_adapter_availability() -> ProviderServiceAdapterAvailability {
    ProviderServiceAdapterAvailability {
        bindings: vec![
            ToolServiceBinding {
                provider_id: "gemini".to_owned(),
                operation_id: "gemini.image_generation".to_owned(),
                route_kind: CapabilityRouteKind::ImageGeneration,
                output_artifact: ModelModality::Image,
            },
            ToolServiceBinding {
                provider_id: "gemini".to_owned(),
                operation_id: "gemini.video_generation".to_owned(),
                route_kind: CapabilityRouteKind::VideoGeneration,
                output_artifact: ModelModality::Video,
            },
            ToolServiceBinding {
                provider_id: "gemini".to_owned(),
                operation_id: "gemini.video_generation.query".to_owned(),
                route_kind: CapabilityRouteKind::VideoGeneration,
                output_artifact: ModelModality::Video,
            },
            ToolServiceBinding {
                provider_id: "gemini".to_owned(),
                operation_id: "gemini.text_to_speech".to_owned(),
                route_kind: CapabilityRouteKind::TextToSpeech,
                output_artifact: ModelModality::Audio,
            },
        ],
    }
}

pub(crate) fn canonical_unique_workspace(name: &str) -> PathBuf {
    let workspace = unique_workspace(name);
    std::fs::create_dir_all(&workspace).unwrap();
    workspace.canonicalize().unwrap()
}

pub(crate) fn provider_capability_route_store(name: &str) -> DesktopProviderCapabilityRouteStore {
    DesktopProviderCapabilityRouteStore::new(canonical_unique_workspace(name))
}

pub(crate) fn empty_provider_capability_routes(
) -> Arc<ParkingRwLock<ProviderCapabilityRouteSettings>> {
    Arc::new(ParkingRwLock::new(ProviderCapabilityRouteSettings {
        version: 1,
        routes: Vec::new(),
    }))
}

pub(crate) fn provider_settings_with_openai_and_minimax(
    openai_config_id: &str,
    minimax_config_id: &str,
    minimax_api_key: &str,
) -> ProviderSettingsRecord {
    ProviderSettingsRecord {
        default_config_id: Some(openai_config_id.to_owned()),
        configs: vec![
            ProviderConfigRecord {
                api_key: "openai-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI main".to_owned(),
                id: openai_config_id.to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                model_options: harness_contracts::ModelRequestOptions::default(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                provider_defaults: None,
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            },
            ProviderConfigRecord {
                api_key: minimax_api_key.to_owned(),
                protocol: ModelProtocol::ChatCompletions,
                base_url: None,
                display_name: "MiniMax image".to_owned(),
                id: minimax_config_id.to_owned(),
                model_id: "minimax-text-01".to_owned(),
                model_options: harness_contracts::ModelRequestOptions::default(),
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
                    display_name: "MiniMax service".to_owned(),
                    lifecycle: ProviderModelLifecycleRecord::Stable,
                    max_output_tokens: 8_192,
                    model_id: "minimax-text-01".to_owned(),
                    provider_id: "minimax".to_owned(),
                    runtime_semantics: None,
                },
            },
        ],
    }
}

pub(crate) fn provider_settings_with_test_and_minimax(
    minimax_config_id: &str,
    minimax_api_key: &str,
) -> ProviderSettingsRecord {
    let mut settings = test_provider_settings_record();
    let mut minimax_settings = provider_settings_with_openai_and_minimax(
        TEST_MODEL_CONFIG_ID,
        minimax_config_id,
        minimax_api_key,
    );
    settings.configs.push(minimax_settings.configs.remove(1));
    settings
}

pub(crate) fn minimax_video_route(config_id: &str, enabled: bool) -> ProviderCapabilityRoute {
    ProviderCapabilityRoute {
        kind: CapabilityRouteKind::VideoGeneration,
        config_id: config_id.to_owned(),
        provider_id: "minimax".to_owned(),
        operation_ids: vec![
            "minimax.video_generation".to_owned(),
            "minimax.video_generation.query".to_owned(),
        ],
        enabled,
    }
}

pub(crate) fn minimax_tts_route(config_id: &str, enabled: bool) -> ProviderCapabilityRoute {
    ProviderCapabilityRoute {
        kind: CapabilityRouteKind::TextToSpeech,
        config_id: config_id.to_owned(),
        provider_id: "minimax".to_owned(),
        operation_ids: vec!["minimax.text_to_speech.sync".to_owned()],
        enabled,
    }
}

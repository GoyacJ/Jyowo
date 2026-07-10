#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

#[tokio::test]
async fn validate_provider_settings_payload_accepts_supported_provider_metadata() {
    let payload = validate_provider_settings_payload(ValidateProviderSettingsRequest {
        model_id: "gpt-5.4-mini".to_owned(),
        provider_id: "openai".to_owned(),
    })
    .await
    .unwrap();
    let value = serde_json::to_value(payload).unwrap();

    assert_eq!(
        value,
        json!({
            "modelId": "gpt-5.4-mini",
            "providerId": "openai",
            "status": "accepted"
        })
    );
}

#[test]
fn list_model_provider_catalog_payload_exposes_models_and_default_base_urls() {
    let payload = list_model_provider_catalog_payload();
    let value = serde_json::to_value(payload).unwrap();
    let providers = value["providers"].as_array().unwrap();

    let openai = providers
        .iter()
        .find(|provider| provider["providerId"] == "openai")
        .unwrap();
    assert_eq!(openai["displayName"], "OpenAI");
    assert_eq!(openai["defaultBaseUrl"], "https://api.openai.com");
    assert!(openai["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|model| model["modelId"] == "gpt-5.4-mini"));
    assert_eq!(openai["runtimeCapability"]["authScheme"], "bearer");
    assert!(openai["runtimeCapability"].get("auth_scheme").is_none());

    let anthropic = providers
        .iter()
        .find(|provider| provider["providerId"] == "anthropic")
        .unwrap();
    assert_eq!(anthropic["runtimeCapability"]["authScheme"], "x_api_key");
    assert!(anthropic["models"].as_array().unwrap().iter().all(|model| {
        model["supportedParameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|parameter| parameter == "thinking")
    }));

    let bedrock = providers
        .iter()
        .find(|provider| provider["providerId"] == "bedrock")
        .unwrap();
    assert_eq!(bedrock["displayName"], "Bedrock");
    assert_eq!(bedrock["runtimeCapability"]["authScheme"], "none");

    let gemini = providers
        .iter()
        .find(|provider| provider["providerId"] == "gemini")
        .unwrap();
    assert_eq!(gemini["runtimeCapability"]["authScheme"], "api_key");
    assert!(gemini["models"].as_array().unwrap().iter().all(|model| {
        model["supportedParameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|parameter| parameter == "thinkingConfig")
    }));

    let local_llama = providers
        .iter()
        .find(|provider| provider["providerId"] == "local-llama")
        .unwrap();
    assert_eq!(local_llama["runtimeCapability"]["authScheme"], "none");

    let km = providers
        .iter()
        .find(|provider| provider["providerId"] == "km")
        .unwrap();
    assert_eq!(km["displayName"], "Kimi");
    assert_eq!(km["defaultBaseUrl"], "https://api.moonshot.cn");
    assert!(km["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|model| model["modelId"] == "kimi-k2.5"));

    let zhipu = providers
        .iter()
        .find(|provider| provider["providerId"] == "zhipu")
        .unwrap();
    assert_eq!(zhipu["displayName"], "Zhipu");
    assert_eq!(
        zhipu["defaultBaseUrl"],
        "https://open.bigmodel.cn/api/paas/v4"
    );
    assert!(zhipu["models"].as_array().unwrap().iter().any(|model| {
        model["modelId"] == "glm-5" && model["conversationCapability"]["promptCache"] == true
    }));
    let zhipu_glm_5 = zhipu["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["modelId"] == "glm-5")
        .unwrap();
    assert_eq!(
        zhipu_glm_5["conversationCapability"]["inputModalities"],
        json!(["text"])
    );
    assert!(zhipu_glm_5["supportedParameters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|parameter| parameter == "do_sample"));
    assert!(zhipu["models"].as_array().unwrap().iter().any(|model| {
        model["modelId"] == "glm-5v-turbo"
            && model["conversationCapability"]["inputModalities"] == json!(["text", "image"])
    }));
    assert!(zhipu["serviceCapabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|service| service["operationId"] == "zhipu.image_generation"));
    assert!(zhipu["runtimeCapability"]["baseUrlRegions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|region| region["baseUrl"] == "https://open.bigmodel.cn/api/coding/paas/v4"));
    assert!(zhipu["runtimeCapability"]["baseUrlRegions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|region| region["baseUrl"] == "https://api.z.ai/api/coding/paas/v4"));

    let minimax = providers
        .iter()
        .find(|provider| provider["providerId"] == "minimax")
        .unwrap();
    let service = minimax["serviceCapabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["operationId"] == "minimax.image_generation")
        .unwrap();
    assert_eq!(service["requiresPolling"], false);
    assert!(service.get("operation_id").is_none());
    assert!(minimax["serviceCapabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|service| service["operationId"] == "minimax.text_to_speech.websocket"));
    assert!(minimax["serviceCapabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|service| service["execution"] == "websocket"));
}

#[tokio::test]
async fn save_provider_settings_payload_accepts_zhipu_official_provider_defaults() {
    let store = RecordingProviderSettingsStore::default();
    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("zhipu-key".to_owned()),
            base_url: None,
            config_id: Some("zhipu-glm".to_owned()),
            display_name: Some("GLM".to_owned()),
            model_id: "glm-5.2".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "zhipu".to_owned(),
            protocol: None,
            provider_defaults: Some(ProviderDefaultsRecord {
                body: Some(json!({
                    "thinking": {
                        "type": "enabled",
                        "clear_thinking": false
                    },
                    "reasoning_effort": "high",
                    "do_sample": false,
                    "tool_stream": true,
                    "temperature": 0.8,
                    "top_p": 0.7,
                    "max_tokens": 4096,
                    "stop": ["END"],
                    "response_format": { "type": "json_object" },
                    "user_id": "user-1"
                })),
                headers: Default::default(),
            }),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    let body = payload.config.provider_defaults.unwrap().body.unwrap();
    assert_eq!(body["thinking"]["clear_thinking"], false);
    assert_eq!(body["do_sample"], false);
    assert_eq!(body["tool_stream"], true);
}

#[tokio::test]
async fn save_provider_settings_rejects_zhipu_tool_defaults() {
    let store = RecordingProviderSettingsStore::default();

    let error = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("zhipu-key".to_owned()),
            base_url: None,
            config_id: Some("zhipu-glm".to_owned()),
            display_name: Some("GLM".to_owned()),
            model_id: "glm-5.2".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "zhipu".to_owned(),
            protocol: None,
            provider_defaults: Some(ProviderDefaultsRecord {
                body: Some(json!({
                    "tools": [],
                    "tool_choice": "auto"
                })),
                headers: Default::default(),
            }),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("tool"));
}

#[tokio::test]
async fn save_provider_settings_accepts_structured_defaults_for_non_qwen_providers() {
    let store = RecordingProviderSettingsStore::default();

    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: None,
            config_id: None,
            display_name: None,
            model_id: "claude-sonnet-4-6".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "anthropic".to_owned(),
            protocol: None,
            provider_defaults: Some(ProviderDefaultsRecord {
                body: Some(json!({
                    "thinking": { "type": "adaptive" },
                    "output_config": { "effort": "medium" },
                    "service_tier": "auto",
                    "stop_sequences": ["DONE"],
                    "top_p": 0.9
                })),
                headers: Default::default(),
            }),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    assert_eq!(
        payload
            .config
            .provider_defaults
            .as_ref()
            .and_then(|defaults| defaults.body.as_ref())
            .and_then(|body| body.pointer("/thinking/type"))
            .and_then(serde_json::Value::as_str),
        Some("adaptive")
    );
}

#[tokio::test]
async fn save_provider_settings_accepts_deepseek_chat_and_messages_defaults() {
    let store = RecordingProviderSettingsStore::default();

    let messages = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: Some("https://api.deepseek.com/anthropic".to_owned()),
            config_id: None,
            display_name: Some("DeepSeek Anthropic".to_owned()),
            model_id: "deepseek-v4-pro".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "deepseek".to_owned(),
            protocol: Some(ModelProtocol::Messages),
            provider_defaults: Some(ProviderDefaultsRecord {
                body: Some(json!({
                    "thinking": { "type": "enabled" },
                    "output_config": { "effort": "max" },
                    "stop_sequences": ["DONE"]
                })),
                headers: Default::default(),
            }),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    assert_eq!(messages.config.protocol, ModelProtocol::Messages);
    assert_eq!(
        messages
            .config
            .provider_defaults
            .as_ref()
            .and_then(|defaults| defaults.body.as_ref())
            .and_then(|body| body.pointer("/output_config/effort"))
            .and_then(serde_json::Value::as_str),
        Some("max")
    );
    assert_eq!(
        messages
            .config
            .provider_defaults
            .as_ref()
            .and_then(|defaults| defaults.body.as_ref())
            .and_then(|body| body.pointer("/stop_sequences/0"))
            .and_then(serde_json::Value::as_str),
        Some("DONE")
    );

    let chat = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: Some("https://api.deepseek.com".to_owned()),
            config_id: Some(messages.config.id.clone()),
            display_name: Some("DeepSeek Chat".to_owned()),
            model_id: "deepseek-v4-pro".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "deepseek".to_owned(),
            protocol: Some(ModelProtocol::ChatCompletions),
            provider_defaults: Some(ProviderDefaultsRecord {
                body: Some(json!({
                    "thinking": { "type": "disabled" },
                    "reasoning_effort": "high",
                    "stop": ["DONE"]
                })),
                headers: Default::default(),
            }),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    assert_eq!(chat.config.protocol, ModelProtocol::ChatCompletions);
}

#[tokio::test]
async fn save_provider_settings_rejects_deepseek_fim_as_conversation_config() {
    let store = RecordingProviderSettingsStore::default();

    let error = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: Some("https://api.deepseek.com/beta".to_owned()),
            config_id: None,
            display_name: Some("DeepSeek FIM".to_owned()),
            model_id: "deepseek-v4-pro".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "deepseek".to_owned(),
            protocol: Some(ModelProtocol::Completions),
            provider_defaults: None,
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert!(error.message.contains("chat_completions or messages"));
}

#[tokio::test]
async fn save_provider_settings_rejects_deepseek_messages_with_chat_defaults_or_base_url() {
    let store = RecordingProviderSettingsStore::default();

    let wrong_defaults = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: Some("https://api.deepseek.com/anthropic".to_owned()),
            config_id: None,
            display_name: Some("DeepSeek Anthropic".to_owned()),
            model_id: "deepseek-v4-pro".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "deepseek".to_owned(),
            protocol: Some(ModelProtocol::Messages),
            provider_defaults: Some(ProviderDefaultsRecord {
                body: Some(json!({ "reasoning_effort": "max" })),
                headers: Default::default(),
            }),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();
    assert!(wrong_defaults
        .message
        .contains("unsupported field reasoning_effort"));

    let wrong_base_url = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: Some("https://api.deepseek.com".to_owned()),
            config_id: None,
            display_name: Some("DeepSeek Anthropic".to_owned()),
            model_id: "deepseek-v4-pro".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "deepseek".to_owned(),
            protocol: Some(ModelProtocol::Messages),
            provider_defaults: None,
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();
    assert!(wrong_base_url.message.contains("/anthropic"));
}

#[tokio::test]
async fn save_provider_settings_accepts_qwen_messages_and_dashscope_protocols() {
    for protocol in [ModelProtocol::Messages, ModelProtocol::Dashscope] {
        let store = RecordingProviderSettingsStore::default();
        let payload = save_provider_settings_with_store(
            ProviderSettingsRequest {
                api_key: Some("provider-test-token".to_owned()),
                base_url: Some("https://dashscope-us.aliyuncs.com/compatible-mode/v1".to_owned()),
                config_id: Some(format!("qwen-{protocol:?}")),
                display_name: Some("Qwen custom mode".to_owned()),
                model_id: "qwen3.7-max".to_owned(),
                model_options: Some(harness_contracts::ModelRequestOptions::default()),
                official_quota_api_key: None,
                provider_id: "qwen".to_owned(),
                protocol: Some(protocol),
                provider_defaults: Some(ProviderDefaultsRecord {
                    body: Some(json!({
                        "enable_thinking": true,
                        "thinking_budget": 2048,
                        "preserve_thinking": true
                    })),
                    headers: [("x-dashscope-session-cache".to_owned(), "enable".to_owned())]
                        .into_iter()
                        .collect(),
                }),
                set_default: true,
            },
            &store,
        )
        .await
        .unwrap();

        assert_eq!(payload.config.provider_id, "qwen");
        assert_eq!(payload.config.protocol, protocol);
        assert_eq!(payload.config.model_descriptor.protocol, protocol);
    }
}

#[tokio::test]
async fn save_provider_settings_rejects_provider_defaults_core_fields() {
    let store = RecordingProviderSettingsStore::default();

    let error = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: None,
            config_id: None,
            display_name: None,
            model_id: "claude-sonnet-4-6".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "anthropic".to_owned(),
            protocol: None,
            provider_defaults: Some(ProviderDefaultsRecord {
                body: Some(json!({ "messages": [] })),
                headers: Default::default(),
            }),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("messages"));
}

#[tokio::test]
async fn provider_inventory_ipc_payloads_do_not_expose_runtime_semantics_or_continuations() {
    let catalog = serde_json::to_string(&list_model_provider_catalog_payload()).unwrap();
    let store = RecordingProviderSettingsStore::default();
    let saved = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: None,
            config_id: None,
            display_name: Some("OpenAI Mini".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
            protocol: None,
            provider_defaults: None,
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();
    let listed = list_provider_settings_with_store(&store).await.unwrap();
    let payloads = [
        catalog,
        serde_json::to_string(&saved).unwrap(),
        serde_json::to_string(&listed).unwrap(),
    ];

    for payload in payloads {
        for field in [
            "runtimeSemantics",
            "runtime_semantics",
            "ProviderContinuation",
            "providerContinuation",
            "provider_continuation",
            "reasoningContent",
            concat!("reasoning", "_content"),
            "continuationPayload",
            "providerNative",
        ] {
            assert!(
                !payload.contains(field),
                "provider IPC payload unexpectedly exposed {field}: {payload}"
            );
        }
    }
}

#[tokio::test]
async fn save_provider_settings_persists_private_runtime_semantics_in_store_record() {
    let store = RecordingProviderSettingsStore::default();

    save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: None,
            config_id: None,
            display_name: Some("OpenAI Mini".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            protocol: None,
            provider_defaults: None,
            provider_id: "openai".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    let record = store.record.lock().unwrap().clone().unwrap();
    let descriptor = &record.configs[0].model_descriptor;
    let semantics = descriptor.runtime_semantics.as_ref().unwrap();
    assert_eq!(semantics.protocol, ModelProtocol::Responses);
    assert_eq!(semantics.tool_protocol, "openai_responses_tools");
    assert_eq!(semantics.streaming_protocol, "sse");
    assert_eq!(semantics.cache_protocol, "openai_auto");
    assert_eq!(semantics.media_protocol, "openai_content_parts");
}

#[tokio::test]
async fn save_provider_settings_migrates_old_openai_responses_record_runtime_semantics() {
    let store = RecordingProviderSettingsStore::default();
    *store.record.lock().unwrap() = Some(ProviderSettingsRecord {
        default_config_id: Some("openai".to_owned()),
        configs: vec![ProviderConfigRecord {
            api_key: "provider-test-token".to_owned(),
            protocol: ModelProtocol::Responses,
            base_url: None,
            display_name: "OpenAI Mini".to_owned(),
            id: "openai".to_owned(),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: harness_contracts::ModelRequestOptions::default(),
            official_quota_api_key: None,
            provider_defaults: None,
            provider_id: "openai".to_owned(),
            model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
        }],
    });

    save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: None,
            base_url: None,
            config_id: Some("openai".to_owned()),
            display_name: Some("OpenAI Mini".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: None,
            official_quota_api_key: None,
            protocol: None,
            provider_defaults: None,
            provider_id: "openai".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    let record = store.record.lock().unwrap().clone().unwrap();
    let semantics = record.configs[0]
        .model_descriptor
        .runtime_semantics
        .as_ref()
        .unwrap();
    assert_eq!(semantics.tool_protocol, "openai_responses_tools");
    assert_eq!(semantics.cache_protocol, "openai_auto");
}

#[tokio::test]
async fn save_provider_settings_preserves_existing_model_options_when_request_omits_them() {
    let stored_model_options = harness_contracts::ModelRequestOptions {
        kimi_chat: None,
        openai_responses: Some(harness_contracts::OpenAiResponsesOptions {
            reasoning: Some(harness_contracts::OpenAiReasoningOptions {
                context: None,
                effort: Some("minimal".to_owned()),
                summary: Some("auto".to_owned()),
            }),
            ..harness_contracts::OpenAiResponsesOptions::default()
        }),
    };
    let store = RecordingProviderSettingsStore::default();
    *store.record.lock().unwrap() = Some(ProviderSettingsRecord {
        default_config_id: Some("openai".to_owned()),
        configs: vec![ProviderConfigRecord {
            api_key: "provider-test-token".to_owned(),
            protocol: ModelProtocol::Responses,
            base_url: None,
            display_name: "OpenAI Mini".to_owned(),
            id: "openai".to_owned(),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: stored_model_options.clone(),
            official_quota_api_key: None,
            provider_defaults: None,
            provider_id: "openai".to_owned(),
            model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
        }],
    });

    save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: None,
            base_url: None,
            config_id: Some("openai".to_owned()),
            display_name: Some("OpenAI Mini".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: None,
            official_quota_api_key: None,
            protocol: None,
            provider_defaults: None,
            provider_id: "openai".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    let record = store.record.lock().unwrap().clone().unwrap();
    assert_eq!(record.configs[0].model_options, stored_model_options);
}

#[tokio::test]
async fn save_provider_settings_clears_existing_model_options_when_request_sends_empty_options() {
    let stored_model_options = harness_contracts::ModelRequestOptions {
        kimi_chat: None,
        openai_responses: Some(harness_contracts::OpenAiResponsesOptions {
            reasoning: Some(harness_contracts::OpenAiReasoningOptions {
                context: None,
                effort: Some("minimal".to_owned()),
                summary: Some("auto".to_owned()),
            }),
            ..harness_contracts::OpenAiResponsesOptions::default()
        }),
    };
    let store = RecordingProviderSettingsStore::default();
    *store.record.lock().unwrap() = Some(ProviderSettingsRecord {
        default_config_id: Some("openai".to_owned()),
        configs: vec![ProviderConfigRecord {
            api_key: "provider-test-token".to_owned(),
            protocol: ModelProtocol::Responses,
            base_url: None,
            display_name: "OpenAI Mini".to_owned(),
            id: "openai".to_owned(),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: stored_model_options,
            official_quota_api_key: None,
            provider_defaults: None,
            provider_id: "openai".to_owned(),
            model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
        }],
    });

    save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: None,
            base_url: None,
            config_id: Some("openai".to_owned()),
            display_name: Some("OpenAI Mini".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            protocol: None,
            provider_defaults: None,
            provider_id: "openai".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    let record = store.record.lock().unwrap().clone().unwrap();
    assert_eq!(
        record.configs[0].model_options,
        harness_contracts::ModelRequestOptions::default()
    );
}

#[tokio::test]
async fn save_provider_settings_payload_stores_viewable_api_key_but_omits_key_from_list_payload() {
    let raw_key = "provider-test-token";
    let store = RecordingProviderSettingsStore::default();
    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some(raw_key.to_owned()),
            base_url: None,
            config_id: None,
            display_name: Some("OpenAI Mini".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
            protocol: None,
            provider_defaults: None,
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(serialized.contains("\"status\":\"saved\""));
    assert!(serialized.contains("\"displayName\":\"OpenAI Mini\""));
    assert!(serialized.contains("\"isDefault\":true"));
    assert!(serialized.contains("\"hasApiKey\":true"));
    assert!(!serialized.contains(raw_key));
    let record = store.record.lock().unwrap().clone().unwrap();
    assert_eq!(record.default_config_id.as_deref(), Some("openai"));
    assert_eq!(record.configs.len(), 1);
    assert_eq!(record.configs[0].protocol, ModelProtocol::Responses);
    assert!(!record.configs[0].api_key.trim().is_empty());
    assert_eq!(record.configs[0].display_name, "OpenAI Mini");
    assert_eq!(record.configs[0].model_descriptor.model_id, "gpt-5.4-mini");

    let listed = list_provider_settings_with_store(&store).await.unwrap();
    let listed_serialized = serde_json::to_string(&listed).unwrap();
    assert_eq!(listed.default_config_id.as_deref(), Some("openai"));
    assert!(listed_serialized.contains("\"hasApiKey\":true"));
    assert!(!listed_serialized.contains(raw_key));
}

#[tokio::test]
async fn get_provider_config_api_key_with_store_requires_runtime_reveal_token_store() {
    let raw_key = "provider-test-token";
    let store = RecordingProviderSettingsStore {
        record: Mutex::new(Some(ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: raw_key.to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                model_options: harness_contracts::ModelRequestOptions::default(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                provider_defaults: None,
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })),
        ..RecordingProviderSettingsStore::default()
    };

    let error = get_provider_config_api_key_with_store(
        GetProviderConfigApiKeyRequest {
            config_id: "openai".to_owned(),
            reveal_token: "test-reveal-token".to_owned(),
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("runtime state"));
}

#[tokio::test]
async fn provider_config_api_key_reveal_token_is_single_use_and_scoped_to_config() {
    let raw_key = "provider-test-token";
    let workspace = unique_workspace("provider-key-reveal-token");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    provider_settings_store_for_workspace(&workspace)
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai-work".to_owned()),
            configs: vec![
                ProviderConfigRecord {
                    api_key: raw_key.to_owned(),
                    protocol: ModelProtocol::Responses,
                    base_url: None,
                    display_name: "OpenAI Work".to_owned(),
                    id: "openai-work".to_owned(),
                    model_id: "gpt-5.4-mini".to_owned(),
                    model_options: harness_contracts::ModelRequestOptions::default(),
                    official_quota_api_key: None,
                    provider_id: "openai".to_owned(),
                    provider_defaults: None,
                    model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
                },
                ProviderConfigRecord {
                    api_key: "personal-token".to_owned(),
                    protocol: ModelProtocol::Responses,
                    base_url: None,
                    display_name: "OpenAI Personal".to_owned(),
                    id: "openai-personal".to_owned(),
                    model_id: "gpt-5.4-mini".to_owned(),
                    model_options: harness_contracts::ModelRequestOptions::default(),
                    official_quota_api_key: None,
                    provider_id: "openai".to_owned(),
                    provider_defaults: None,
                    model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
                },
            ],
        })
        .unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;

    let reveal = request_provider_config_api_key_reveal_with_runtime_state(
        RequestProviderConfigApiKeyRevealRequest {
            config_id: "openai-work".to_owned(),
        },
        &state,
    )
    .await
    .expect("saved provider config should create a reveal token");
    assert_eq!(reveal.config_id, "openai-work");
    assert_eq!(reveal.expires_in_seconds, 60);
    assert_eq!(reveal.status, "ready");
    assert!(!reveal.reveal_token.trim().is_empty());
    assert!(!serde_json::to_string(&reveal).unwrap().contains(raw_key));

    let mismatch_error = get_provider_config_api_key_with_runtime_state(
        GetProviderConfigApiKeyRequest {
            config_id: "openai-personal".to_owned(),
            reveal_token: reveal.reveal_token,
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(mismatch_error.code, "INVALID_PAYLOAD");

    let reveal = request_provider_config_api_key_reveal_with_runtime_state(
        RequestProviderConfigApiKeyRevealRequest {
            config_id: "openai-work".to_owned(),
        },
        &state,
    )
    .await
    .expect("saved provider config should create another reveal token");
    let token = reveal.reveal_token;
    let payload = get_provider_config_api_key_with_runtime_state(
        GetProviderConfigApiKeyRequest {
            config_id: "openai-work".to_owned(),
            reveal_token: token.clone(),
        },
        &state,
    )
    .await
    .expect("fresh reveal token should return the provider key");
    assert_eq!(payload.config_id, "openai-work");
    assert_eq!(payload.api_key, raw_key);

    let reuse_error = get_provider_config_api_key_with_runtime_state(
        GetProviderConfigApiKeyRequest {
            config_id: "openai-work".to_owned(),
            reveal_token: token,
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(reuse_error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn request_provider_config_api_key_reveal_with_store_requires_runtime_reveal_token_store() {
    let store = RecordingProviderSettingsStore {
        record: Mutex::new(Some(ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                model_options: harness_contracts::ModelRequestOptions::default(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                provider_defaults: None,
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })),
        ..RecordingProviderSettingsStore::default()
    };

    let error = request_provider_config_api_key_reveal_with_store(
        RequestProviderConfigApiKeyRevealRequest {
            config_id: "openai".to_owned(),
        },
        &store,
    )
    .await
    .expect_err("plaintext key reveal should fail closed without runtime state");
    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("runtime state"));
}

#[tokio::test(start_paused = true)]
async fn provider_config_api_key_reveal_token_expires() {
    let workspace = unique_workspace("provider-key-reveal-expired");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    provider_settings_store_for_workspace(&workspace)
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                model_options: harness_contracts::ModelRequestOptions::default(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                provider_defaults: None,
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;

    let reveal = request_provider_config_api_key_reveal_with_runtime_state(
        RequestProviderConfigApiKeyRevealRequest {
            config_id: "openai".to_owned(),
        },
        &state,
    )
    .await
    .expect("saved provider config should create a reveal token");
    tokio::time::advance(Duration::from_secs(61)).await;

    let error = get_provider_config_api_key_with_runtime_state(
        GetProviderConfigApiKeyRequest {
            config_id: "openai".to_owned(),
            reveal_token: reveal.reveal_token,
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("expired"));
}

#[tokio::test]
async fn provider_config_api_key_reveal_token_rejects_config_key_changed_after_issue() {
    let workspace = unique_workspace("provider-key-reveal-key-changed");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let store = provider_settings_store_for_workspace(&workspace);
    store
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "old-provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                model_options: harness_contracts::ModelRequestOptions::default(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                provider_defaults: None,
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;

    let reveal = request_provider_config_api_key_reveal_with_runtime_state(
        RequestProviderConfigApiKeyRevealRequest {
            config_id: "openai".to_owned(),
        },
        &state,
    )
    .await
    .expect("saved provider config should create a reveal token");
    store
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "new-provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                model_options: harness_contracts::ModelRequestOptions::default(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                provider_defaults: None,
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap();

    let error = get_provider_config_api_key_with_runtime_state(
        GetProviderConfigApiKeyRequest {
            config_id: "openai".to_owned(),
            reveal_token: reveal.reveal_token,
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("no longer matches"));
}

#[tokio::test]
async fn save_provider_settings_with_runtime_state_invalidates_pending_reveal_tokens_for_config() {
    let workspace = unique_workspace("provider-key-reveal-save-invalidates");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    provider_settings_store_for_workspace(&workspace)
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "old-provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                model_options: harness_contracts::ModelRequestOptions::default(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                provider_defaults: None,
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;

    let reveal = request_provider_config_api_key_reveal_with_runtime_state(
        RequestProviderConfigApiKeyRevealRequest {
            config_id: "openai".to_owned(),
        },
        &state,
    )
    .await
    .expect("saved provider config should create a reveal token");
    save_provider_settings_with_runtime_state(
        ProviderSettingsRequest {
            api_key: Some("new-provider-test-token".to_owned()),
            base_url: None,
            config_id: Some("openai".to_owned()),
            display_name: Some("OpenAI".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
            protocol: None,
            provider_defaults: None,
            set_default: true,
        },
        &state,
    )
    .await
    .expect("saving provider settings should succeed");

    let error = get_provider_config_api_key_with_runtime_state(
        GetProviderConfigApiKeyRequest {
            config_id: "openai".to_owned(),
            reveal_token: reveal.reveal_token,
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn save_provider_settings_payload_allows_same_provider_model_multiple_keys() {
    let store = RecordingProviderSettingsStore::default();

    let work = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("work-token".to_owned()),
            base_url: None,
            config_id: Some("openai-work".to_owned()),
            display_name: Some("OpenAI Work".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
            protocol: None,
            provider_defaults: None,
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();
    let personal = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("personal-token".to_owned()),
            base_url: None,
            config_id: Some("openai-personal".to_owned()),
            display_name: Some("OpenAI Personal".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
            protocol: None,
            provider_defaults: None,
            set_default: false,
        },
        &store,
    )
    .await
    .unwrap();

    assert!(work.config.is_default);
    assert!(!personal.config.is_default);
    let record = store.record.lock().unwrap().clone().unwrap();
    assert_eq!(record.default_config_id.as_deref(), Some("openai-work"));
    assert_eq!(record.configs.len(), 2);
    assert_eq!(record.configs[0].model_id, record.configs[1].model_id);
    assert_ne!(record.configs[0].api_key, record.configs[1].api_key);
}

#[tokio::test]
async fn list_provider_settings_payload_returns_profiles_without_raw_keys() {
    let store = RecordingProviderSettingsStore {
        record: Mutex::new(Some(ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: Some("https://gateway.example.com".to_owned()),
                display_name: "OpenAI gateway".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                model_options: harness_contracts::ModelRequestOptions::default(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                provider_defaults: None,
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })),
        ..RecordingProviderSettingsStore::default()
    };

    let payload = list_provider_settings_with_store(&store).await.unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(serialized.contains("\"defaultConfigId\":\"openai\""));
    assert!(serialized.contains("\"baseUrl\":\"https://gateway.example.com\""));
    assert!(serialized.contains("\"hasApiKey\":true"));
    assert!(!serialized.contains("provider-test-token"));
}

#[tokio::test]
async fn list_provider_settings_payload_returns_saved_openrouter_dynamic_descriptor() {
    let store = RecordingProviderSettingsStore {
        record: Mutex::new(Some(ProviderSettingsRecord {
            default_config_id: Some("openrouter".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::ChatCompletions,
                base_url: None,
                display_name: "OpenRouter dynamic".to_owned(),
                id: "openrouter".to_owned(),
                model_id: "dynamic/provider-model".to_owned(),
                model_options: harness_contracts::ModelRequestOptions::default(),
                official_quota_api_key: None,
                provider_id: "openrouter".to_owned(),
                provider_defaults: None,
                model_descriptor: openrouter_descriptor_record(
                    "dynamic/provider-model",
                    vec![ProviderModelModalityRecord::Text],
                    vec![ProviderModelModalityRecord::Text],
                    true,
                ),
            }],
        })),
        ..RecordingProviderSettingsStore::default()
    };

    let payload = list_provider_settings_with_store(&store).await.unwrap();

    assert_eq!(payload.configs[0].protocol, ModelProtocol::ChatCompletions);
    let descriptor = &payload.configs[0].model_descriptor;
    assert_eq!(descriptor.model_id, "dynamic/provider-model");
    assert_eq!(descriptor.runtime_status.kind, "runnable");
    assert_eq!(
        descriptor.conversation_capability.input_modalities,
        vec![ProviderModelModalityRecord::Text]
    );
}

#[tokio::test]
async fn list_provider_settings_payload_rejects_openrouter_descriptor_with_unsupported_modalities()
{
    let store = RecordingProviderSettingsStore {
        record: Mutex::new(Some(ProviderSettingsRecord {
            default_config_id: Some("openrouter".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::ChatCompletions,
                base_url: None,
                display_name: "OpenRouter image".to_owned(),
                id: "openrouter".to_owned(),
                model_id: "dynamic/image-model".to_owned(),
                model_options: harness_contracts::ModelRequestOptions::default(),
                official_quota_api_key: None,
                provider_id: "openrouter".to_owned(),
                provider_defaults: None,
                model_descriptor: openrouter_descriptor_record(
                    "dynamic/image-model",
                    vec![
                        ProviderModelModalityRecord::Text,
                        ProviderModelModalityRecord::Image,
                    ],
                    vec![ProviderModelModalityRecord::Text],
                    true,
                ),
            }],
        })),
        ..RecordingProviderSettingsStore::default()
    };

    let error = list_provider_settings_with_store(&store).await.unwrap_err();

    assert_eq!(error.code, "RUNTIME_INIT_FAILED");
}

#[tokio::test]
async fn list_provider_settings_payload_rejects_openrouter_descriptor_with_wrong_protocol() {
    let mut descriptor = openrouter_descriptor_record(
        "dynamic/messages-model",
        vec![ProviderModelModalityRecord::Text],
        vec![ProviderModelModalityRecord::Text],
        true,
    );
    descriptor.protocol = ModelProtocol::Messages;
    let store = RecordingProviderSettingsStore {
        record: Mutex::new(Some(ProviderSettingsRecord {
            default_config_id: Some("openrouter".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Messages,
                base_url: None,
                display_name: "OpenRouter wrong protocol".to_owned(),
                id: "openrouter".to_owned(),
                model_id: "dynamic/messages-model".to_owned(),
                model_options: harness_contracts::ModelRequestOptions::default(),
                official_quota_api_key: None,
                provider_id: "openrouter".to_owned(),
                provider_defaults: None,
                model_descriptor: descriptor,
            }],
        })),
        ..RecordingProviderSettingsStore::default()
    };

    let error = list_provider_settings_with_store(&store).await.unwrap_err();

    assert_eq!(error.code, "RUNTIME_INIT_FAILED");
}

#[test]
fn provider_settings_record_rejects_old_single_provider_shape() {
    let old = json!({
        "modelId": "gpt-5.4-mini",
        "providerId": "openai",
        "secretRef": "provider/workspace-local/openai/default"
    });

    assert!(serde_json::from_value::<ProviderSettingsRecord>(old).is_err());
}

#[test]
fn provider_settings_record_rejects_config_without_new_model_descriptor() {
    let old = json!({
        "defaultConfigId": "openai",
        "configs": [{
            "apiKey": "provider-test-token",
            "baseUrl": "https://gateway.example.com",
            "displayName": "OpenAI gateway",
            "id": "openai",
            "modelId": "gpt-5.4-mini",
            "providerId": "openai"
        }]
    });

    assert!(serde_json::from_value::<ProviderSettingsRecord>(old).is_err());
}

#[test]
fn desktop_provider_settings_store_rejects_invalid_old_provider_settings_file() {
    let workspace = unique_workspace("provider-settings-old-provider-settings");
    let settings_dir = workspace.join(".jyowo").join("runtime");
    std::fs::create_dir_all(&settings_dir).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let settings_dir = workspace.join(".jyowo").join("runtime");
    let settings_path = settings_dir.join("provider-settings.json");
    let mut descriptor = serde_json::to_value(openai_descriptor_record("gpt-5.4-mini")).unwrap();
    let descriptor_object = descriptor.as_object_mut().unwrap();
    let protocol = descriptor_object.remove("protocol").unwrap();
    descriptor_object.insert("apiMode".to_owned(), protocol);
    descriptor_object.remove("conversationCapability").unwrap();
    descriptor_object.insert(
        "capabilities".to_owned(),
        json!({
            "supportsTools": true,
            "supportsVision": true,
            "supportsThinking": false,
            "supportsStreaming": true,
            "supportsStructuredOutput": true,
            "supportsJsonMode": true,
            "supportsParallelToolCalls": true,
            "supportsBuiltinWebSearch": false,
            "supportsBuiltinCodeExecution": false,
            "supportsPromptCache": true,
            "inputModalities": ["text", "image"],
            "outputModalities": ["text"]
        }),
    );
    let record = json!({
        "defaultConfigId": "openai",
        "configs": [{
            "apiKey": "provider-test-token",
            "apiMode": "responses",
            "displayName": "OpenAI",
            "id": "openai",
            "modelId": "gpt-5.4-mini",
            "providerId": "openai",
            "modelDescriptor": descriptor
        }]
    });
    std::fs::write(&settings_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    let store = provider_settings_store_for_workspace(&workspace);

    let loaded = store
        .load_record()
        .expect("production provider settings load must ignore old runtime file");

    assert!(
        loaded.is_none(),
        "old runtime provider settings must not be used as production fallback"
    );
    assert!(settings_path.exists());
}

#[test]
fn provider_settings_record_rejects_config_secret_ref() {
    let record = json!({
        "defaultConfigId": "openai-gateway",
        "configs": [{
            "apiKey": "provider-test-token",
            "protocol": "responses",
            "baseUrl": "https://gateway.example.com",
            "displayName": "OpenAI gateway",
            "id": "openai-gateway",
            "modelId": "gpt-5.4-mini",
            "providerId": "openai",
            "secretRef": "provider/workspace-local/openai/default"
        }]
    });

    assert!(serde_json::from_value::<ProviderSettingsRecord>(record).is_err());
}

#[test]
fn provider_settings_record_rejects_config_without_api_key() {
    let record = json!({
        "defaultConfigId": "openai",
        "configs": [{
            "protocol": "responses",
            "displayName": "OpenAI",
            "id": "openai",
            "modelId": "gpt-5.4-mini",
            "providerId": "openai"
        }]
    });

    assert!(serde_json::from_value::<ProviderSettingsRecord>(record).is_err());
}

#[test]
fn provider_settings_record_rejects_configs_without_default_config_id() {
    let record = json!({
        "configs": [{
            "apiKey": "provider-test-token",
            "protocol": "responses",
            "displayName": "OpenAI",
            "id": "openai",
            "modelId": "gpt-5.4-mini",
            "providerId": "openai"
        }]
    });

    assert!(serde_json::from_value::<ProviderSettingsRecord>(record).is_err());
}

#[test]
fn provider_settings_record_rejects_default_config_id_missing_from_configs() {
    let record = json!({
        "defaultConfigId": "missing",
        "configs": [{
            "apiKey": "provider-test-token",
            "protocol": "responses",
            "displayName": "OpenAI",
            "id": "openai",
            "modelId": "gpt-5.4-mini",
            "providerId": "openai"
        }]
    });

    assert!(serde_json::from_value::<ProviderSettingsRecord>(record).is_err());
}

#[tokio::test]
async fn save_provider_settings_payload_accepts_bedrock_without_api_key() {
    let store = RecordingProviderSettingsStore::default();
    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: None,
            base_url: None,
            config_id: Some("bedrock-claude".to_owned()),
            display_name: Some("Bedrock Claude".to_owned()),
            model_id: "anthropic.claude-3-5-sonnet-20241022-v2:0".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "bedrock".to_owned(),
            protocol: None,
            provider_defaults: Some(ProviderDefaultsRecord {
                body: Some(json!({
                    "inferenceConfig": {
                        "topP": 0.8
                    },
                    "performanceConfig": {
                        "latency": "optimized"
                    }
                })),
                headers: Default::default(),
            }),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    assert_eq!(payload.config.provider_id, "bedrock");
    assert!(!payload.config.has_api_key);
    assert_eq!(
        payload.config.provider_defaults.unwrap().body.unwrap(),
        json!({
            "inferenceConfig": {
                "topP": 0.8
            },
            "performanceConfig": {
                "latency": "optimized"
            }
        })
    );
    assert_eq!(
        store.record.lock().unwrap().as_ref().unwrap().configs[0].api_key,
        ""
    );
}

#[tokio::test]
async fn save_provider_settings_payload_reuses_saved_openrouter_dynamic_descriptor() {
    let store = RecordingProviderSettingsStore::default();
    *store.record.lock().unwrap() = Some(ProviderSettingsRecord {
        default_config_id: Some("openrouter".to_owned()),
        configs: vec![ProviderConfigRecord {
            api_key: "provider-test-token".to_owned(),
            protocol: ModelProtocol::ChatCompletions,
            base_url: Some("https://openrouter.ai/api".to_owned()),
            display_name: "OpenRouter dynamic".to_owned(),
            id: "openrouter".to_owned(),
            model_id: "dynamic/provider-model".to_owned(),
            model_options: harness_contracts::ModelRequestOptions::default(),
            official_quota_api_key: None,
            provider_id: "openrouter".to_owned(),
            provider_defaults: None,
            model_descriptor: openrouter_descriptor_record(
                "dynamic/provider-model",
                vec![ProviderModelModalityRecord::Text],
                vec![ProviderModelModalityRecord::Text],
                true,
            ),
        }],
    });

    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: None,
            base_url: Some("https://openrouter.ai/api".to_owned()),
            config_id: Some("openrouter".to_owned()),
            display_name: Some("OpenRouter dynamic".to_owned()),
            model_id: "dynamic/provider-model".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "openrouter".to_owned(),
            protocol: None,
            provider_defaults: None,
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    assert_eq!(payload.config.model_id, "dynamic/provider-model");
    assert_eq!(payload.config.protocol, ModelProtocol::ChatCompletions);
    assert_eq!(
        payload.config.model_descriptor.model_id,
        "dynamic/provider-model"
    );
}

#[tokio::test]
async fn save_provider_settings_payload_requires_api_key_when_base_url_changes() {
    let store = RecordingProviderSettingsStore::default();
    *store.record.lock().unwrap() = Some(ProviderSettingsRecord {
        default_config_id: Some("openai-gateway".to_owned()),
        configs: vec![ProviderConfigRecord {
            api_key: "provider-test-token".to_owned(),
            protocol: ModelProtocol::Responses,
            base_url: Some("https://gateway.example.com".to_owned()),
            display_name: "OpenAI gateway".to_owned(),
            id: "openai-gateway".to_owned(),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: harness_contracts::ModelRequestOptions::default(),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
            provider_defaults: None,
            model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
        }],
    });

    let error = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: None,
            base_url: Some("https://attacker.example.com".to_owned()),
            config_id: Some("openai-gateway".to_owned()),
            display_name: Some("OpenAI gateway".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
            protocol: None,
            provider_defaults: None,
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("apiKey is required"));
}

#[tokio::test]
async fn save_provider_settings_payload_rejects_http_base_url_with_loopback_prefix_domain() {
    let store = RecordingProviderSettingsStore::default();
    let error = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: Some("http://127.attacker.example".to_owned()),
            config_id: None,
            display_name: Some("OpenAI gateway".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
            protocol: None,
            provider_defaults: None,
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error
        .message
        .contains("baseUrl must use https:// unless it targets localhost"));
}

#[tokio::test]
async fn save_provider_settings_payload_rejects_custom_gemini_base_url() {
    let store = RecordingProviderSettingsStore::default();
    let error = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: Some("https://gateway.example.com".to_owned()),
            config_id: None,
            display_name: Some("Gemini gateway".to_owned()),
            model_id: "gemini-2.5-flash".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "gemini".to_owned(),
            protocol: None,
            provider_defaults: None,
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error
        .message
        .contains("Gemini baseUrl must target generativelanguage.googleapis.com"));
}

#[tokio::test]
async fn save_provider_settings_payload_accepts_http_loopback_base_url() {
    let store = RecordingProviderSettingsStore::default();
    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: Some("http://127.0.0.1:11434/v1".to_owned()),
            config_id: None,
            display_name: Some("OpenAI gateway".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
            protocol: None,
            provider_defaults: None,
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    assert_eq!(
        payload.config.base_url.as_deref(),
        Some("http://127.0.0.1:11434/v1")
    );
}

#[tokio::test]
async fn save_provider_settings_payload_does_not_save_record_when_record_write_fails() {
    let store = RecordingProviderSettingsStore {
        fail_record: true,
        ..RecordingProviderSettingsStore::default()
    };
    let error = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: None,
            config_id: None,
            display_name: None,
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
            protocol: None,
            provider_defaults: None,
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
}

#[tokio::test]
async fn provider_settings_payload_rejects_invalid_provider_model_and_key() {
    let store = RecordingProviderSettingsStore::default();
    let invalid_provider = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: None,
            config_id: None,
            display_name: None,
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "unknown".to_owned(),
            protocol: None,
            provider_defaults: None,
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(invalid_provider.code, "INVALID_PAYLOAD");

    let invalid_model = validate_provider_settings_payload(ValidateProviderSettingsRequest {
        model_id: "not-a-real-model".to_owned(),
        provider_id: "openai".to_owned(),
    })
    .await
    .unwrap_err();

    assert_eq!(invalid_model.code, "INVALID_PAYLOAD");

    let invalid_key = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some(String::new()),
            base_url: None,
            config_id: None,
            display_name: None,
            model_id: "gpt-5.4-mini".to_owned(),
            model_options: Some(harness_contracts::ModelRequestOptions::default()),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
            protocol: None,
            provider_defaults: None,
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(invalid_key.code, "INVALID_PAYLOAD");

    let invalid_metadata = validate_provider_settings_payload(ValidateProviderSettingsRequest {
        model_id: String::new(),
        provider_id: "openai".to_owned(),
    })
    .await
    .unwrap_err();

    assert_eq!(invalid_metadata.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn validate_provider_settings_payload_does_not_require_api_key() {
    let payload = validate_provider_settings_payload(ValidateProviderSettingsRequest {
        model_id: "gpt-5.4-mini".to_owned(),
        provider_id: "openai".to_owned(),
    })
    .await
    .unwrap();

    assert_eq!(payload.status, "accepted");
}

#[test]
fn provider_inventory_runtime_semantics_are_not_serialized_to_public_catalog_payloads() {
    let payload = serde_json::to_string(&list_model_provider_catalog_payload()).unwrap();

    for field in [
        "runtimeSemantics",
        "runtime_semantics",
        "ProviderContinuation",
        "providerContinuation",
        "provider_continuation",
        "reasoningContent",
        concat!("reasoning", "_content"),
        "continuationPayload",
        "providerNative",
    ] {
        assert!(
            !payload.contains(field),
            "provider catalog payload unexpectedly exposed {field}: {payload}"
        );
    }
}

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

    let gemini = providers
        .iter()
        .find(|provider| provider["providerId"] == "gemini")
        .unwrap();
    assert_eq!(gemini["runtimeCapability"]["authScheme"], "api_key");

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
    assert!(!minimax["serviceCapabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|service| service["operationId"] == "minimax.text_to_speech.websocket"));
    assert!(!minimax["serviceCapabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|service| service["execution"] == "websocket"));
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
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
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
async fn save_provider_settings_payload_stores_official_quota_key_but_omits_raw_key_from_payload() {
    let raw_key = "provider-test-token";
    let official_quota_key = "official-quota-admin-token";
    let store = RecordingProviderSettingsStore::default();

    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some(raw_key.to_owned()),
            base_url: None,
            config_id: None,
            display_name: Some("OpenAI Mini".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            official_quota_api_key: Some(official_quota_key.to_owned()),
            provider_id: "openai".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    let serialized = serde_json::to_string(&payload).unwrap();
    assert!(serialized.contains("\"hasOfficialQuotaApiKey\":true"));
    assert!(!serialized.contains(raw_key));
    assert!(!serialized.contains(official_quota_key));

    let record = store.record.lock().unwrap().clone().unwrap();
    assert_eq!(
        record.configs[0].official_quota_api_key.as_deref(),
        Some(official_quota_key)
    );

    let listed = list_provider_settings_with_store(&store).await.unwrap();
    let listed_serialized = serde_json::to_string(&listed).unwrap();
    assert!(listed_serialized.contains("\"hasOfficialQuotaApiKey\":true"));
    assert!(!listed_serialized.contains(raw_key));
    assert!(!listed_serialized.contains(official_quota_key));
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
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
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
    DesktopProviderSettingsStore::new(workspace.clone())
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
                    official_quota_api_key: None,
                    provider_id: "openai".to_owned(),
                    model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
                },
                ProviderConfigRecord {
                    api_key: "personal-token".to_owned(),
                    protocol: ModelProtocol::Responses,
                    base_url: None,
                    display_name: "OpenAI Personal".to_owned(),
                    id: "openai-personal".to_owned(),
                    model_id: "gpt-5.4-mini".to_owned(),
                    official_quota_api_key: None,
                    provider_id: "openai".to_owned(),
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
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
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
    DesktopProviderSettingsStore::new(workspace.clone())
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
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
    let store = DesktopProviderSettingsStore::new(workspace.clone());
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
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
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
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
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
    DesktopProviderSettingsStore::new(workspace.clone())
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "old-provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
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
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
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
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
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
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
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
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
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
                official_quota_api_key: None,
                provider_id: "openrouter".to_owned(),
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
                official_quota_api_key: None,
                provider_id: "openrouter".to_owned(),
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
                official_quota_api_key: None,
                provider_id: "openrouter".to_owned(),
                model_descriptor: descriptor,
            }],
        })),
        ..RecordingProviderSettingsStore::default()
    };

    let error = list_provider_settings_with_store(&store).await.unwrap_err();

    assert_eq!(error.code, "RUNTIME_INIT_FAILED");
}

#[test]
fn provider_settings_record_rejects_legacy_single_provider_shape() {
    let legacy = json!({
        "modelId": "gpt-5.4-mini",
        "providerId": "openai",
        "secretRef": "provider/workspace-local/openai/default"
    });

    assert!(serde_json::from_value::<ProviderSettingsRecord>(legacy).is_err());
}

#[test]
fn provider_settings_record_rejects_config_without_new_model_descriptor() {
    let legacy = json!({
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

    assert!(serde_json::from_value::<ProviderSettingsRecord>(legacy).is_err());
}

#[test]
fn desktop_provider_settings_store_deletes_legacy_provider_settings_file() {
    let workspace = unique_workspace("provider-settings-legacy-provider-settings");
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
    let store = DesktopProviderSettingsStore::new(workspace);

    assert_eq!(store.load_record().unwrap(), None);
    assert!(!settings_path.exists());
}

#[tokio::test]
async fn provider_capability_route_missing_file_returns_empty_settings() {
    let workspace = canonical_unique_workspace("provider-capability-route-missing");
    let store = DesktopProviderCapabilityRouteStore::new(workspace);

    let payload = list_provider_capability_routes_with_store(
        &store,
        &ProviderSettingsRecord::default(),
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .await
    .unwrap();

    assert_eq!(payload.version, 1);
    assert!(payload.routes.is_empty());
}

#[tokio::test]
async fn provider_capability_route_save_writes_runtime_file() {
    let workspace = canonical_unique_workspace("provider-capability-route-save-file");
    let store = DesktopProviderCapabilityRouteStore::new(workspace.clone());
    let provider_settings = provider_settings_record_with_minimax_config("minimax-image", true);
    let catalog = list_model_provider_catalog_payload();
    let availability = minimax_image_adapter_availability();

    save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest {
            route: minimax_image_route("minimax-image", true),
        },
        &store,
        &provider_settings,
        &catalog,
        &availability,
    )
    .await
    .unwrap();

    let route_path = workspace
        .join(".jyowo")
        .join("runtime")
        .join("provider-capability-routes.json");
    let saved: Value = serde_json::from_slice(&std::fs::read(route_path).unwrap()).unwrap();

    assert_eq!(saved["version"], 1);
    assert_eq!(saved["routes"][0]["kind"], "image_generation");
    assert_eq!(saved["routes"][0]["configId"], "minimax-image");
}

#[tokio::test]
async fn provider_capability_route_rejects_missing_config_id() {
    let store = provider_capability_route_store("provider-capability-route-missing-config");
    let provider_settings = provider_settings_record_with_minimax_config("minimax-image", true);
    let error = save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest {
            route: minimax_image_route("missing", true),
        },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &minimax_image_adapter_availability(),
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn provider_capability_route_disabled_route_allows_stale_runtime_references() {
    let store = provider_capability_route_store("provider-capability-route-disabled-stale");
    let mut route = minimax_image_route("missing-minimax", false);
    route.operation_ids = vec!["minimax.retired_image_generation".to_owned()];

    let payload = save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest { route },
        &store,
        &ProviderSettingsRecord::default(),
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .await
    .expect("disabled route should save stale runtime references");

    assert_eq!(payload.routes.len(), 1);
    assert!(!payload.routes[0].enabled);
    assert_eq!(payload.routes[0].config_id, "missing-minimax");
    assert_eq!(
        payload.routes[0].operation_ids,
        vec!["minimax.retired_image_generation".to_owned()]
    );

    let saved = list_provider_capability_routes_with_store(
        &store,
        &ProviderSettingsRecord::default(),
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .await
    .expect("disabled stale route should remain listable");

    assert_eq!(saved.routes.len(), 1);
    assert!(!saved.routes[0].enabled);
}

#[tokio::test]
async fn provider_capability_route_rejects_provider_mismatch() {
    let store = provider_capability_route_store("provider-capability-route-provider-mismatch");
    let provider_settings = ProviderSettingsRecord {
        default_config_id: Some("openai-image".to_owned()),
        configs: vec![ProviderConfigRecord {
            api_key: "provider-test-token".to_owned(),
            protocol: ModelProtocol::Responses,
            base_url: None,
            display_name: "OpenAI".to_owned(),
            id: "openai-image".to_owned(),
            model_id: "gpt-5.4-mini".to_owned(),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
            model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
        }],
    };
    let mut route = minimax_image_route("openai-image", true);
    route.provider_id = "minimax".to_owned();

    let error = save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest { route },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &minimax_image_adapter_availability(),
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn provider_capability_route_rejects_profile_without_api_key() {
    let store = provider_capability_route_store("provider-capability-route-no-api-key");
    let provider_settings = provider_settings_record_with_minimax_config("minimax-image", false);

    let error = save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest {
            route: minimax_image_route("minimax-image", true),
        },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &minimax_image_adapter_availability(),
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn provider_capability_route_rejects_operation_not_in_catalog() {
    let store = provider_capability_route_store("provider-capability-route-unknown-operation");
    let provider_settings = provider_settings_record_with_minimax_config("minimax-image", true);
    let mut route = minimax_image_route("minimax-image", true);
    route.operation_ids = vec!["minimax.unknown".to_owned()];

    let error = save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest { route },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn provider_capability_route_rejects_catalog_operation_without_adapter() {
    let store = provider_capability_route_store("provider-capability-route-no-adapter");
    let provider_settings = provider_settings_record_with_minimax_config("minimax-image", true);

    let error = save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest {
            route: minimax_image_route("minimax-image", true),
        },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn provider_capability_route_rejects_contract_validation_failure() {
    let store = provider_capability_route_store("provider-capability-route-contract-validation");
    let provider_settings = provider_settings_record_with_minimax_config("minimax-image", true);
    let mut route = minimax_image_route("minimax-image", true);
    route.operation_ids.clear();

    let error = save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest { route },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &minimax_image_adapter_availability(),
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn provider_capability_route_rejects_version_mismatch() {
    let store = provider_capability_route_store("provider-capability-route-version-mismatch");

    let error = save_provider_capability_route_settings_with_store(
        harness_contracts::ProviderCapabilityRouteSettings {
            version: 2,
            routes: Vec::new(),
        },
        &store,
        &ProviderSettingsRecord::default(),
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn provider_capability_route_rejects_same_enabled_kind_for_multiple_configs() {
    let store = provider_capability_route_store("provider-capability-route-kind-conflict");
    let mut provider_settings =
        provider_settings_record_with_minimax_config("minimax-image-primary", true);
    let mut secondary = provider_settings.configs[0].clone();
    secondary.id = "minimax-image-secondary".to_owned();
    provider_settings.configs.push(secondary);

    let error = save_provider_capability_route_settings_with_store(
        harness_contracts::ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![
                minimax_image_route("minimax-image-primary", true),
                minimax_image_route("minimax-image-secondary", true),
            ],
        },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &minimax_image_adapter_availability(),
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn provider_capability_route_enabled_save_replaces_same_kind_route() {
    let store = provider_capability_route_store("provider-capability-route-kind-replace");
    let mut provider_settings =
        provider_settings_record_with_minimax_config("minimax-image-primary", true);
    let mut secondary = provider_settings.configs[0].clone();
    secondary.id = "minimax-image-secondary".to_owned();
    provider_settings.configs.push(secondary);

    save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest {
            route: minimax_image_route("minimax-image-primary", true),
        },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &minimax_image_adapter_availability(),
    )
    .await
    .unwrap();

    let payload = save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest {
            route: minimax_image_route("minimax-image-secondary", true),
        },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &minimax_image_adapter_availability(),
    )
    .await
    .unwrap();

    assert_eq!(payload.routes.len(), 1);
    assert_eq!(payload.routes[0].kind, CapabilityRouteKind::ImageGeneration);
    assert_eq!(payload.routes[0].config_id, "minimax-image-secondary");
}

#[tokio::test]
async fn provider_capability_route_enabled_save_keeps_other_kinds() {
    let store = provider_capability_route_store("provider-capability-route-kind-replace-keeps");
    let mut provider_settings =
        provider_settings_record_with_minimax_config("minimax-primary", true);
    let mut secondary = provider_settings.configs[0].clone();
    secondary.id = "minimax-secondary".to_owned();
    provider_settings.configs.push(secondary);
    let availability = minimax_image_and_video_adapter_availability();

    save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest {
            route: minimax_video_route("minimax-primary", true),
        },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &availability,
    )
    .await
    .unwrap();
    save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest {
            route: minimax_image_route("minimax-primary", true),
        },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &availability,
    )
    .await
    .unwrap();

    let payload = save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest {
            route: minimax_image_route("minimax-secondary", true),
        },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &availability,
    )
    .await
    .unwrap();

    assert_eq!(payload.routes.len(), 2);
    assert!(payload.routes.iter().any(|route| {
        route.kind == CapabilityRouteKind::ImageGeneration && route.config_id == "minimax-secondary"
    }));
    assert!(payload.routes.iter().any(|route| {
        route.kind == CapabilityRouteKind::VideoGeneration && route.config_id == "minimax-primary"
    }));
}

#[test]
fn provider_capability_route_options_use_injected_adapter_availability() {
    let provider_settings = provider_settings_record_with_minimax_config("minimax-image", true);
    let store = provider_capability_route_store("provider-capability-route-options-unsupported");

    let unsupported = list_provider_capability_route_options_from_inputs(
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .unwrap();
    let image_option = unsupported
        .options
        .iter()
        .find(|option| option.operation_id == "minimax.image_generation")
        .unwrap();
    assert!(!image_option.runtime_supported);

    let store = provider_capability_route_store("provider-capability-route-options-supported");
    let supported = list_provider_capability_route_options_from_inputs(
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &minimax_image_adapter_availability(),
    )
    .unwrap();
    let image_option = supported
        .options
        .iter()
        .find(|option| option.operation_id == "minimax.image_generation")
        .unwrap();
    assert!(image_option.runtime_supported);
}

#[test]
fn provider_capability_route_options_never_expose_api_keys() {
    let provider_settings = provider_settings_record_with_minimax_config("minimax-image", true);
    let store = provider_capability_route_store("provider-capability-route-options-redaction");

    let payload = list_provider_capability_route_options_from_inputs(
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &minimax_image_adapter_availability(),
    )
    .unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(!serialized.contains("provider-test-token"));
}

#[tokio::test]
async fn provider_capability_route_disabled_route_is_saved() {
    let store = provider_capability_route_store("provider-capability-route-disabled");
    let provider_settings = provider_settings_record_with_minimax_config("minimax-image", true);

    let payload = save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest {
            route: minimax_image_route("minimax-image", false),
        },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .await
    .unwrap();

    assert!(!payload.routes[0].enabled);
    let saved = list_provider_capability_routes_with_store(
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .await
    .unwrap();
    assert_eq!(saved.routes.len(), 1);
}

#[tokio::test]
async fn provider_capability_route_delete_removes_matching_route() {
    let store = provider_capability_route_store("provider-capability-route-delete");
    let provider_settings = provider_settings_record_with_minimax_config("minimax-image", true);
    save_provider_capability_route_with_store(
        SaveProviderCapabilityRouteRequest {
            route: minimax_image_route("minimax-image", true),
        },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &minimax_image_adapter_availability(),
    )
    .await
    .unwrap();

    let payload = delete_provider_capability_route_with_store(
        DeleteProviderCapabilityRouteRequest {
            kind: CapabilityRouteKind::ImageGeneration,
            config_id: "minimax-image".to_owned(),
            provider_id: "minimax".to_owned(),
        },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .await
    .unwrap();

    assert!(payload.routes.is_empty());
    let saved = list_provider_capability_routes_with_store(
        &store,
        &ProviderSettingsRecord::default(),
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .await
    .unwrap();
    assert!(saved.routes.is_empty());
}

#[tokio::test]
async fn provider_capability_route_saving_empty_routes_writes_empty_settings() {
    let workspace = canonical_unique_workspace("provider-capability-route-empty-save");
    let store = DesktopProviderCapabilityRouteStore::new(workspace.clone());

    save_provider_capability_route_settings_with_store(
        harness_contracts::ProviderCapabilityRouteSettings {
            version: 1,
            routes: Vec::new(),
        },
        &store,
        &ProviderSettingsRecord::default(),
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .await
    .unwrap();

    let route_path = workspace
        .join(".jyowo")
        .join("runtime")
        .join("provider-capability-routes.json");
    let saved: Value = serde_json::from_slice(&std::fs::read(route_path).unwrap()).unwrap();

    assert_eq!(saved, json!({ "version": 1, "routes": [] }));
}

#[tokio::test]
async fn provider_capability_route_invalid_file_is_removed_and_returns_empty_settings() {
    let workspace = canonical_unique_workspace("provider-capability-route-invalid-file");
    let route_dir = workspace.join(".jyowo").join("runtime");
    std::fs::create_dir_all(&route_dir).unwrap();
    let route_path = route_dir.join("provider-capability-routes.json");
    std::fs::write(
        &route_path,
        br#"{ "version": 1, "routes": [], "apiKey": "secret" }"#,
    )
    .unwrap();
    let store = DesktopProviderCapabilityRouteStore::new(workspace);

    let payload = list_provider_capability_routes_with_store(
        &store,
        &ProviderSettingsRecord::default(),
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .await
    .unwrap();

    assert_eq!(payload.version, 1);
    assert!(payload.routes.is_empty());
    assert!(!route_path.exists());
}

#[tokio::test]
async fn provider_capability_route_malformed_json_file_is_removed_and_returns_empty_settings() {
    let workspace = canonical_unique_workspace("provider-capability-route-malformed-file");
    let route_dir = workspace.join(".jyowo").join("runtime");
    std::fs::create_dir_all(&route_dir).unwrap();
    let route_path = route_dir.join("provider-capability-routes.json");
    std::fs::write(&route_path, br#"{ "version": 1, "routes": ["#).unwrap();
    let store = DesktopProviderCapabilityRouteStore::new(workspace);

    let payload = list_provider_capability_routes_with_store(
        &store,
        &ProviderSettingsRecord::default(),
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .await
    .unwrap();

    assert_eq!(payload.version, 1);
    assert!(payload.routes.is_empty());
    assert!(!route_path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn desktop_provider_capability_route_store_writes_owner_only_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let workspace = canonical_unique_workspace("provider-capability-route-owner-only");
    let store = DesktopProviderCapabilityRouteStore::new(workspace.clone());
    save_provider_capability_route_settings_with_store(
        harness_contracts::ProviderCapabilityRouteSettings {
            version: 1,
            routes: Vec::new(),
        },
        &store,
        &ProviderSettingsRecord::default(),
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .await
    .unwrap();

    let route_path = workspace
        .join(".jyowo")
        .join("runtime")
        .join("provider-capability-routes.json");
    let mode = std::fs::metadata(route_path).unwrap().permissions().mode() & 0o777;

    assert_eq!(mode, 0o600);
}

#[cfg(unix)]
#[tokio::test]
async fn desktop_provider_capability_route_store_rejects_symlink_settings_file() {
    let workspace = canonical_unique_workspace("provider-capability-route-symlink-file");
    let external = canonical_unique_workspace("provider-capability-route-external-target");
    let route_dir = workspace.join(".jyowo").join("runtime");
    let route_path = route_dir.join("provider-capability-routes.json");
    std::fs::create_dir_all(&route_dir).unwrap();
    std::os::unix::fs::symlink(
        external.join("provider-capability-routes.json"),
        &route_path,
    )
    .unwrap();
    let store = DesktopProviderCapabilityRouteStore::new(workspace);

    let error = store.load_record().unwrap_err();
    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");

    let error = save_provider_capability_route_settings_with_store(
        harness_contracts::ProviderCapabilityRouteSettings {
            version: 1,
            routes: Vec::new(),
        },
        &store,
        &ProviderSettingsRecord::default(),
        &list_model_provider_catalog_payload(),
        &ProviderServiceAdapterAvailability::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert!(route_path.is_symlink());
}

mod capability_route_conversation {
    use super::*;
    use harness_contracts::ConversationModelCapability;

    struct NoToolCallingScriptedProvider {
        responses: Vec<ScriptedResponse>,
        requests: Mutex<Vec<jyowo_harness_sdk::ext::ModelRequest>>,
    }

    impl NoToolCallingScriptedProvider {
        fn new(responses: Vec<ScriptedResponse>) -> Self {
            Self {
                responses,
                requests: Mutex::new(Vec::new()),
            }
        }

        async fn requests(&self) -> Vec<jyowo_harness_sdk::ext::ModelRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl jyowo_harness_sdk::ext::ModelProvider for NoToolCallingScriptedProvider {
        fn provider_id(&self) -> &str {
            "test"
        }

        fn supported_models(&self) -> Vec<jyowo_harness_sdk::ext::ModelDescriptor> {
            vec![jyowo_harness_sdk::ext::ModelDescriptor {
                provider_id: "test".to_owned(),
                model_id: "test-model".to_owned(),
                display_name: "No tool calling".to_owned(),
                protocol: ModelProtocol::Messages,
                context_window: 128_000,
                max_output_tokens: 8_192,
                conversation_capability: ConversationModelCapability {
                    tool_calling: false,
                    ..ConversationModelCapability::default()
                },
                lifecycle: jyowo_harness_sdk::ext::ModelLifecycle::Stable,
                pricing: None,
            }]
        }

        async fn infer(
            &self,
            req: jyowo_harness_sdk::ext::ModelRequest,
            _ctx: jyowo_harness_sdk::ext::InferContext,
        ) -> Result<jyowo_harness_sdk::ext::ModelStream, jyowo_harness_sdk::ext::ModelError>
        {
            self.requests.lock().unwrap().push(req);
            let response = self
                .responses
                .first()
                .cloned()
                .unwrap_or(ScriptedResponse::Stream(vec![
                    ModelStreamEvent::MessageStop,
                ]));
            match response {
                ScriptedResponse::Stream(events) => Ok(Box::pin(futures::stream::iter(events))),
                ScriptedResponse::Error(error) => Err(error),
                ScriptedResponse::WaitForCancel => {
                    std::future::pending::<()>().await;
                    Err(jyowo_harness_sdk::ext::ModelError::Cancelled)
                }
            }
        }

        async fn health(&self) -> jyowo_harness_sdk::ext::HealthStatus {
            jyowo_harness_sdk::ext::HealthStatus::Healthy
        }
    }

    #[tokio::test]
    async fn capability_route_conversation_exposes_minimax_image_tool_with_route() {
        let workspace = unique_workspace("capability-route-conversation-image-with-route");
        let provider = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::MessageStop,
        ])]));
        let state = runtime_state_with_capability_route_harness(
            workspace,
            ProviderCapabilityRouteSettings {
                version: 1,
                routes: vec![minimax_image_route("minimax-image", true)],
            },
            Arc::clone(&provider),
            provider_settings_with_openai_and_minimax(
                "openai-main",
                "minimax-image",
                "route-token",
            ),
        )
        .await;
        let session_id = SessionId::new();
        open_conversation_session(&state, session_id).await;

        start_run_with_runtime_state(
            StartRunRequest {
                client_message_id: None,
                attachments: None,
                agent_options: None,
                context_references: None,
                conversation_id: session_id.to_string(),
                model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
                permission_mode: None,
                prompt: "draw a poster".to_owned(),
            },
            &state,
        )
        .await
        .expect("start_run should start");

        let requests = wait_for_scripted_model_requests(&provider).await;
        let tool_names = model_request_tool_names(&requests[0]);
        assert!(tool_names.contains(&"MiniMaxTextToImage".to_owned()));
    }

    #[tokio::test]
    async fn capability_route_conversation_hides_minimax_image_tool_without_route() {
        let workspace = unique_workspace("capability-route-conversation-image-without-route");
        let provider = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::MessageStop,
        ])]));
        let state = runtime_state_with_capability_route_harness(
            workspace,
            ProviderCapabilityRouteSettings {
                version: 1,
                routes: Vec::new(),
            },
            Arc::clone(&provider),
            provider_settings_with_openai_and_minimax(
                "openai-main",
                "minimax-image",
                "route-token",
            ),
        )
        .await;
        let session_id = SessionId::new();
        open_conversation_session(&state, session_id).await;

        start_run_with_runtime_state(
            StartRunRequest {
                client_message_id: None,
                attachments: None,
                agent_options: None,
                context_references: None,
                conversation_id: session_id.to_string(),
                model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
                permission_mode: None,
                prompt: "draw a poster".to_owned(),
            },
            &state,
        )
        .await
        .expect("start_run should start");

        let requests = wait_for_scripted_model_requests(&provider).await;
        let tool_names = model_request_tool_names(&requests[0]);
        assert!(!tool_names.contains(&"MiniMaxTextToImage".to_owned()));
    }

    #[tokio::test]
    async fn capability_route_conversation_hides_service_tools_without_tool_calling() {
        let workspace = unique_workspace("capability-route-conversation-no-tool-calling");
        std::fs::create_dir_all(&workspace).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let provider = Arc::new(NoToolCallingScriptedProvider::new(vec![
            ScriptedResponse::Stream(vec![ModelStreamEvent::MessageStop]),
        ]));
        let routes = Arc::new(ParkingRwLock::new(ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![minimax_image_route("minimax-image", true)],
        }));
        let resolver = desktop_provider_credential_resolver_with_stores(
            Arc::new(DesktopConversationMetadataStore::new(workspace.clone())),
            Arc::new(DesktopProviderSettingsStore::new(workspace.clone())),
            Arc::clone(&routes),
        );
        DesktopProviderSettingsStore::new(workspace.clone())
            .save_record(&provider_settings_with_openai_and_minimax(
                "openai-main",
                "minimax-image",
                "route-token",
            ))
            .expect("provider settings should save");
        let stream_permission_runtime =
            Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
                default_timeout: Some(Duration::from_secs(5)),
                heartbeat_interval: None,
                max_pending: 16,
            }));
        let registry = ToolRegistry::builder()
            .with_builtin_toolset(BuiltinToolset::Default)
            .build()
            .expect("tool registry should build");
        std::fs::create_dir_all(workspace.join(".jyowo").join("runtime").join("blobs")).unwrap();
        let blob_store =
            FileBlobStore::open(workspace.join(".jyowo").join("runtime").join("blobs"))
                .expect("blob store should open");
        let harness = Arc::new(
            Harness::builder()
                .with_options(test_harness_options(&workspace))
                .with_model_arc(provider.clone())
                .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
                .with_sandbox(NoopSandbox::new())
                .with_blob_store(blob_store)
                .with_stream_permission_broker_arc(
                    stream_permission_runtime.broker(),
                    stream_permission_runtime.resolver_handle(),
                )
                .with_tool_registry(registry)
                .with_shared_provider_capability_routes(routes)
                .with_capability(ToolCapability::ProviderCredentialResolver, resolver)
                .build()
                .await
                .expect("harness should build"),
        );
        let state = DesktopRuntimeState::with_harness_and_stream_permission_runtime_for_workspace(
            workspace,
            harness,
            stream_permission_runtime,
        )
        .expect("state should initialize");
        let session_id = SessionId::new();
        open_conversation_session(&state, session_id).await;

        start_run_with_runtime_state(
            StartRunRequest {
                client_message_id: None,
                attachments: None,
                agent_options: None,
                context_references: None,
                conversation_id: session_id.to_string(),
                model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
                permission_mode: None,
                prompt: "draw a poster".to_owned(),
            },
            &state,
        )
        .await
        .expect("start_run should start");

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let requests = provider.requests().await;
            if !requests.is_empty() {
                assert!(requests[0].tools.is_none());
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("timed out waiting for model requests");
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    #[tokio::test]
    async fn capability_route_conversation_exposes_video_tools_when_video_route_exists() {
        let workspace = unique_workspace("capability-route-conversation-video");
        let provider = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::MessageStop,
        ])]));
        let state = runtime_state_with_capability_route_harness(
            workspace,
            ProviderCapabilityRouteSettings {
                version: 1,
                routes: vec![minimax_video_route("minimax-image", true)],
            },
            Arc::clone(&provider),
            provider_settings_with_openai_and_minimax(
                "openai-main",
                "minimax-image",
                "route-token",
            ),
        )
        .await;
        let session_id = SessionId::new();
        open_conversation_session(&state, session_id).await;

        start_run_with_runtime_state(
            StartRunRequest {
                client_message_id: None,
                attachments: None,
                agent_options: None,
                context_references: None,
                conversation_id: session_id.to_string(),
                model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
                permission_mode: None,
                prompt: "make a clip".to_owned(),
            },
            &state,
        )
        .await
        .expect("start_run should start");

        let requests = wait_for_scripted_model_requests(&provider).await;
        let tool_names = model_request_tool_names(&requests[0]);
        assert!(tool_names.contains(&"MiniMaxTextToVideo".to_owned()));
        assert!(tool_names.contains(&"MiniMaxVideoGenerationQuery".to_owned()));
    }

    #[tokio::test]
    async fn capability_route_conversation_exposes_tts_tools_when_tts_route_exists() {
        let workspace = unique_workspace("capability-route-conversation-tts");
        let provider = Arc::new(ScriptedProvider::new(vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::MessageStop,
        ])]));
        let state = runtime_state_with_capability_route_harness(
            workspace,
            ProviderCapabilityRouteSettings {
                version: 1,
                routes: vec![minimax_tts_route("minimax-image", true)],
            },
            Arc::clone(&provider),
            provider_settings_with_openai_and_minimax(
                "openai-main",
                "minimax-image",
                "route-token",
            ),
        )
        .await;
        let session_id = SessionId::new();
        open_conversation_session(&state, session_id).await;

        start_run_with_runtime_state(
            StartRunRequest {
                client_message_id: None,
                attachments: None,
                agent_options: None,
                context_references: None,
                conversation_id: session_id.to_string(),
                model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
                permission_mode: None,
                prompt: "read this aloud".to_owned(),
            },
            &state,
        )
        .await
        .expect("start_run should start");

        let requests = wait_for_scripted_model_requests(&provider).await;
        let tool_names = model_request_tool_names(&requests[0]);
        assert!(tool_names.contains(&"MiniMaxTextToSpeech".to_owned()));
    }
}

mod provider_credential_route {
    use super::*;

    #[tokio::test]
    async fn provider_credential_route_provider_only_resolution_requires_run_model_config() {
        let workspace = canonical_unique_workspace("provider-credential-route-provider-only");
        let provider_settings = provider_settings_record_with_minimax_config("minimax-main", true);
        let provider_store = DesktopProviderSettingsStore::new(workspace.clone());
        provider_store
            .save_record(&provider_settings)
            .expect("provider settings should save");
        let conversation_store = DesktopConversationMetadataStore::new(workspace);
        let resolver = desktop_provider_credential_resolver_with_stores(
            Arc::new(conversation_store),
            Arc::new(provider_store),
            empty_provider_capability_routes(),
        );
        let session_id = SessionId::new();

        let error = resolver
            .resolve_provider_credential(ProviderCredentialResolveContext {
                tenant_id: TenantId::SINGLE,
                session_id,
                run_id: RunId::new(),
                provider_id: "minimax".to_owned(),
                model_config_id: None,
                operation_id: None,
                route_kind: None,
            })
            .await
            .expect_err("provider-only credential resolution without model config should fail");

        assert!(matches!(error, ToolError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn provider_credential_route_provider_only_resolution_uses_run_model_config() {
        let workspace = canonical_unique_workspace("provider-credential-route-provider-only-run");
        let provider_settings = provider_settings_record_with_minimax_config("minimax-main", true);
        let provider_store = DesktopProviderSettingsStore::new(workspace.clone());
        provider_store
            .save_record(&provider_settings)
            .expect("provider settings should save");
        let resolver = desktop_provider_credential_resolver_with_stores(
            Arc::new(DesktopConversationMetadataStore::new(workspace)),
            Arc::new(provider_store),
            empty_provider_capability_routes(),
        );

        let credential = resolver
            .resolve_provider_credential(ProviderCredentialResolveContext {
                tenant_id: TenantId::SINGLE,
                session_id: SessionId::new(),
                run_id: RunId::new(),
                provider_id: "minimax".to_owned(),
                model_config_id: Some("minimax-main".to_owned()),
                operation_id: None,
                route_kind: None,
            })
            .await
            .expect("provider-only credential resolution should use run model config");

        assert_eq!(credential.provider_id, "minimax");
        assert_eq!(credential.config_id, "minimax-main");
        assert!(!credential.api_key.is_empty());
    }

    #[tokio::test]
    async fn provider_credential_route_routed_service_context_fails_closed_without_route() {
        let workspace = canonical_unique_workspace("provider-credential-route-routed-fail-closed");
        let provider_settings = provider_settings_record_with_minimax_config("minimax-main", true);
        let provider_store = DesktopProviderSettingsStore::new(workspace.clone());
        provider_store
            .save_record(&provider_settings)
            .expect("provider settings should save");
        let conversation_store = DesktopConversationMetadataStore::new(workspace);
        let resolver = desktop_provider_credential_resolver_with_stores(
            Arc::new(conversation_store),
            Arc::new(provider_store),
            empty_provider_capability_routes(),
        );

        let error = resolver
            .resolve_provider_credential(ProviderCredentialResolveContext {
                tenant_id: TenantId::SINGLE,
                session_id: SessionId::new(),
                run_id: RunId::new(),
                provider_id: "minimax".to_owned(),
                model_config_id: None,
                operation_id: Some("minimax.image_generation".to_owned()),
                route_kind: Some(CapabilityRouteKind::ImageGeneration),
            })
            .await
            .expect_err("routed service credential resolution should fail closed");

        assert!(matches!(error, ToolError::PermissionDenied(_)));
        assert!(!error.to_string().contains("provider-test-token"));
    }

    #[tokio::test]
    async fn provider_credential_route_resolves_routed_service_credential() {
        let workspace = canonical_unique_workspace("provider-credential-route-success");
        let provider_store = DesktopProviderSettingsStore::new(workspace.clone());
        provider_store
            .save_record(&provider_settings_with_openai_and_minimax(
                "openai-main",
                "minimax-image",
                "route-specific-token",
            ))
            .expect("provider settings should save");
        let routes = Arc::new(ParkingRwLock::new(ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![minimax_image_route("minimax-image", true)],
        }));
        let resolver = desktop_provider_credential_resolver_with_stores(
            Arc::new(DesktopConversationMetadataStore::new(workspace)),
            Arc::new(provider_store),
            routes,
        );

        let credential = resolver
            .resolve_provider_credential(ProviderCredentialResolveContext {
                tenant_id: TenantId::SINGLE,
                session_id: SessionId::new(),
                run_id: RunId::new(),
                provider_id: "minimax".to_owned(),
                model_config_id: None,
                operation_id: Some("minimax.image_generation".to_owned()),
                route_kind: Some(CapabilityRouteKind::ImageGeneration),
            })
            .await
            .expect("routed service credential resolution should succeed");

        assert_eq!(credential.config_id, "minimax-image");
        assert_eq!(credential.api_key, "route-specific-token");
    }

    #[tokio::test]
    async fn provider_credential_route_wrong_provider_denies_routed_service_credential() {
        let workspace = canonical_unique_workspace("provider-credential-route-wrong-provider");
        let provider_store = DesktopProviderSettingsStore::new(workspace.clone());
        provider_store
            .save_record(&provider_settings_with_openai_and_minimax(
                "openai-main",
                "minimax-image",
                "route-specific-token",
            ))
            .expect("provider settings should save");
        let routes = Arc::new(ParkingRwLock::new(ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![minimax_image_route("minimax-image", true)],
        }));
        let resolver = desktop_provider_credential_resolver_with_stores(
            Arc::new(DesktopConversationMetadataStore::new(workspace)),
            Arc::new(provider_store),
            routes,
        );

        let error = resolver
            .resolve_provider_credential(ProviderCredentialResolveContext {
                tenant_id: TenantId::SINGLE,
                session_id: SessionId::new(),
                run_id: RunId::new(),
                provider_id: "openai".to_owned(),
                model_config_id: None,
                operation_id: Some("minimax.image_generation".to_owned()),
                route_kind: Some(CapabilityRouteKind::ImageGeneration),
            })
            .await
            .expect_err("wrong provider should deny routed credential");

        assert!(matches!(error, ToolError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn provider_credential_route_disabled_route_denies_routed_service_credential() {
        let workspace = canonical_unique_workspace("provider-credential-route-disabled");
        let provider_store = DesktopProviderSettingsStore::new(workspace.clone());
        provider_store
            .save_record(&provider_settings_with_openai_and_minimax(
                "openai-main",
                "minimax-image",
                "route-specific-token",
            ))
            .expect("provider settings should save");
        let routes = Arc::new(ParkingRwLock::new(ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![minimax_image_route("minimax-image", false)],
        }));
        let resolver = desktop_provider_credential_resolver_with_stores(
            Arc::new(DesktopConversationMetadataStore::new(workspace)),
            Arc::new(provider_store),
            routes,
        );

        let error = resolver
            .resolve_provider_credential(ProviderCredentialResolveContext {
                tenant_id: TenantId::SINGLE,
                session_id: SessionId::new(),
                run_id: RunId::new(),
                provider_id: "minimax".to_owned(),
                model_config_id: None,
                operation_id: Some("minimax.image_generation".to_owned()),
                route_kind: Some(CapabilityRouteKind::ImageGeneration),
            })
            .await
            .expect_err("disabled route should deny routed credential");

        assert!(matches!(error, ToolError::PermissionDenied(_)));
    }

    #[tokio::test]
    async fn provider_credential_route_routed_service_never_falls_back_to_default_config() {
        let workspace = canonical_unique_workspace("provider-credential-route-no-fallback");
        let provider_store = DesktopProviderSettingsStore::new(workspace.clone());
        provider_store
            .save_record(&provider_settings_with_openai_and_minimax(
                "openai-main",
                "minimax-image",
                "route-specific-token",
            ))
            .expect("provider settings should save");
        let resolver = desktop_provider_credential_resolver_with_stores(
            Arc::new(DesktopConversationMetadataStore::new(workspace)),
            Arc::new(provider_store),
            empty_provider_capability_routes(),
        );

        let error = resolver
            .resolve_provider_credential(ProviderCredentialResolveContext {
                tenant_id: TenantId::SINGLE,
                session_id: SessionId::new(),
                run_id: RunId::new(),
                provider_id: "minimax".to_owned(),
                model_config_id: None,
                operation_id: Some("minimax.image_generation".to_owned()),
                route_kind: Some(CapabilityRouteKind::ImageGeneration),
            })
            .await
            .expect_err("routed service must not fall back to default provider config");

        assert!(matches!(error, ToolError::PermissionDenied(_)));
        assert!(!error.to_string().contains("openai-test-token"));
    }
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
            official_quota_api_key: None,
            provider_id: "openrouter".to_owned(),
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
            official_quota_api_key: None,
            provider_id: "openrouter".to_owned(),
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
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
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
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
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
async fn save_provider_settings_payload_does_not_preserve_official_quota_key_when_base_url_changes()
{
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
            official_quota_api_key: Some("old-official-admin-key".to_owned()),
            provider_id: "openai".to_owned(),
            model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
        }],
    });

    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("new-provider-test-token".to_owned()),
            base_url: Some("https://new-gateway.example.com".to_owned()),
            config_id: Some("openai-gateway".to_owned()),
            display_name: Some("OpenAI gateway".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    assert!(!payload.config.has_official_quota_api_key);
    let record = store.record.lock().unwrap().clone().unwrap();
    assert_eq!(record.configs[0].official_quota_api_key, None);
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
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
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
async fn save_provider_settings_payload_accepts_http_loopback_base_url() {
    let store = RecordingProviderSettingsStore::default();
    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: Some("http://127.0.0.1:11434/v1".to_owned()),
            config_id: None,
            display_name: Some("OpenAI gateway".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
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
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
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
            official_quota_api_key: None,
            provider_id: "unknown".to_owned(),
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
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
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
fn desktop_provider_settings_store_rejects_config_without_api_key() {
    let workspace = unique_workspace("conversation-model-no-key");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let error = DesktopProviderSettingsStore::new(workspace)
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai-work".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: String::new(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI Work".to_owned(),
                id: "openai-work".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap_err();

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert!(error.message.contains("apiKey is required"));
}

#[cfg(unix)]
#[test]
fn desktop_provider_settings_store_writes_owner_only_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let workspace = unique_workspace("provider-settings-owner-only");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    DesktopProviderSettingsStore::new(workspace.clone())
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai-work".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI Work".to_owned(),
                id: "openai-work".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap();

    let settings_path = workspace
        .join(".jyowo")
        .join("runtime")
        .join("provider-settings.json");
    let mode = std::fs::metadata(settings_path)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn desktop_provider_settings_store_rejects_symlink_settings_file() {
    let workspace = unique_workspace("provider-settings-symlink-file");
    let external = unique_workspace("provider-settings-external-target");
    let settings_dir = workspace.join(".jyowo").join("runtime");
    let settings_path = settings_dir.join("provider-settings.json");
    std::fs::create_dir_all(&settings_dir).unwrap();
    std::fs::create_dir_all(&external).unwrap();
    std::os::unix::fs::symlink(external.join("provider-settings.json"), &settings_path).unwrap();
    let store = DesktopProviderSettingsStore::new(workspace);

    let error = store.load_record().unwrap_err();
    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");

    let error = store
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: String::new(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                official_quota_api_key: None,
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap_err();

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert!(!external.join("provider-settings.json").exists());
}

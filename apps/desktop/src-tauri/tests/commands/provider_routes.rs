#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

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
        .join("config")
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
            model_options: harness_contracts::ModelRequestOptions::default(),
            official_quota_api_key: None,
            provider_id: "openai".to_owned(),
            provider_defaults: None,
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
fn no_workspace_provider_capability_route_options_are_empty() {
    let provider_settings = provider_settings_record_with_minimax_config("minimax-image", true);

    let payload = list_provider_capability_route_options_from_inputs(
        &NoWorkspaceProviderCapabilityRouteStore,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &minimax_image_adapter_availability(),
    )
    .unwrap();

    assert!(payload.options.is_empty());
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
        .join("config")
        .join("provider-capability-routes.json");
    let saved: Value = serde_json::from_slice(&std::fs::read(route_path).unwrap()).unwrap();

    assert_eq!(saved, json!({ "version": 1, "routes": [] }));
}

#[tokio::test]
async fn provider_capability_route_invalid_old_file_is_ignored_and_preserved() {
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
    assert!(route_path.exists());
}

#[tokio::test]
async fn provider_capability_route_malformed_old_file_is_ignored_and_preserved() {
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
    assert!(route_path.exists());
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
        .join("config")
        .join("provider-capability-routes.json");
    let mode = std::fs::metadata(route_path).unwrap().permissions().mode() & 0o777;

    assert_eq!(mode, 0o600);
}

#[cfg(unix)]
#[tokio::test]
async fn desktop_provider_capability_route_store_rejects_symlink_settings_file() {
    let workspace = canonical_unique_workspace("provider-capability-route-symlink-file");
    let external = canonical_unique_workspace("provider-capability-route-external-target");
    let route_dir = workspace.join(".jyowo").join("config");
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

#[tokio::test]
async fn provider_capability_routes_save_ignores_old_runtime_path() {
    let workspace = canonical_unique_workspace("provider-route-save-ignores-runtime");
    let runtime_dir = workspace.join(".jyowo").join("runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();

    let old_path = runtime_dir.join("provider-capability-routes.json");
    std::fs::write(
        &old_path,
        serde_json::to_vec_pretty(&harness_contracts::ProviderCapabilityRouteSettings {
            version: 1,
            routes: Vec::new(),
        })
        .unwrap(),
    )
    .unwrap();

    let store = DesktopProviderCapabilityRouteStore::new(workspace.clone());
    let provider_settings = provider_settings_record_with_minimax_config("minimax-image", true);
    save_provider_capability_route_settings_with_store(
        harness_contracts::ProviderCapabilityRouteSettings {
            version: 1,
            routes: vec![minimax_image_route("minimax-image", true)],
        },
        &store,
        &provider_settings,
        &list_model_provider_catalog_payload(),
        &minimax_image_adapter_availability(),
    )
    .await
    .unwrap();

    let config_path = workspace
        .join(".jyowo")
        .join("config")
        .join("provider-capability-routes.json");
    assert!(config_path.exists(), "should write to config");
    assert!(old_path.exists(), "old runtime file should be ignored");

    let saved: harness_contracts::ProviderCapabilityRouteSettings =
        serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    assert_eq!(saved.routes.len(), 1);
    assert_eq!(saved.routes[0].config_id, "minimax-image");
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
                supported_parameters: Vec::new(),
                context_window: 128_000,
                max_output_tokens: 8_192,
                provider_declared_capability: ConversationModelCapability {
                    tool_calling: false,
                    ..ConversationModelCapability::default()
                },
                conversation_capability: ConversationModelCapability {
                    tool_calling: false,
                    ..ConversationModelCapability::default()
                },
                runtime_semantics: jyowo_harness_sdk::ext::ModelRuntimeSemantics::messages_default(
                    ModelProtocol::Messages,
                ),
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
            provider_settings_with_test_and_minimax("minimax-image", "route-token"),
        )
        .await;
        let session_id = SessionId::new();
        open_conversation_session(&state, session_id).await;

        start_run_with_runtime_state(
            StartRunRequest {
                client_message_id: None,
                attachments: None,
                context_references: None,
                conversation_id: session_id.to_string(),
                model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
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
            provider_settings_with_test_and_minimax("minimax-image", "route-token"),
        )
        .await;
        let session_id = SessionId::new();
        open_conversation_session(&state, session_id).await;

        start_run_with_runtime_state(
            StartRunRequest {
                client_message_id: None,
                attachments: None,
                context_references: None,
                conversation_id: session_id.to_string(),
                model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
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
            Arc::new(provider_settings_store_for_workspace(&workspace)),
            Arc::clone(&routes),
        );
        provider_settings_store_for_workspace(&workspace)
            .save_record(&provider_settings_with_test_and_minimax(
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
        let mut state =
            DesktopRuntimeState::with_harness_and_stream_permission_runtime_for_workspace(
                workspace,
                harness,
                stream_permission_runtime,
            )
            .expect("state should initialize");
        let state_workspace = state.workspace_root().to_path_buf();
        use_test_provider_settings_store(&mut state, &state_workspace);
        let session_id = SessionId::new();
        open_conversation_session(&state, session_id).await;

        start_run_with_runtime_state(
            StartRunRequest {
                client_message_id: None,
                attachments: None,
                context_references: None,
                conversation_id: session_id.to_string(),
                model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
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
            provider_settings_with_test_and_minimax("minimax-image", "route-token"),
        )
        .await;
        let session_id = SessionId::new();
        open_conversation_session(&state, session_id).await;

        start_run_with_runtime_state(
            StartRunRequest {
                client_message_id: None,
                attachments: None,
                context_references: None,
                conversation_id: session_id.to_string(),
                model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
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
            provider_settings_with_test_and_minimax("minimax-image", "route-token"),
        )
        .await;
        let session_id = SessionId::new();
        open_conversation_session(&state, session_id).await;

        start_run_with_runtime_state(
            StartRunRequest {
                client_message_id: None,
                attachments: None,
                context_references: None,
                conversation_id: session_id.to_string(),
                model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
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

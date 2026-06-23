use harness_model::{
    build_provider, provider_inventory_entries, resolve_model_descriptor, ProviderAuthScheme,
    ProviderBuildConfig, ProviderRegistryError, ProviderServiceExecution,
};

#[test]
fn registry_rejects_unknown_provider_fail_closed() {
    let descriptor_error = resolve_model_descriptor("unknown-provider", "model").unwrap_err();
    assert!(matches!(
        descriptor_error,
        ProviderRegistryError::UnsupportedProvider { .. }
    ));
    assert_eq!(
        descriptor_error.to_string(),
        "providerId must be a supported model provider"
    );

    let build_error = build_provider(ProviderBuildConfig {
        provider_id: "unknown-provider".to_owned(),
        api_key: "test-key".to_owned(),
        base_url: None,
        model_descriptor: None,
    })
    .err()
    .unwrap();
    assert!(matches!(
        build_error,
        ProviderRegistryError::UnsupportedProvider { .. }
    ));
}

#[test]
fn provider_inventory_includes_source_metadata() {
    let entries = provider_inventory_entries();

    assert!(entries.iter().all(|entry| !entry.source_url.is_empty()));
    assert!(entries
        .iter()
        .all(|entry| entry.verified_date.to_string() == "2026-06-21"));
}

#[test]
fn provider_catalog_auth_schemes_match_runtime_adapters() {
    let entries = harness_model::provider_catalog_entries();

    #[cfg(feature = "openai")]
    assert_eq!(
        catalog_auth_scheme(&entries, "openai"),
        ProviderAuthScheme::Bearer
    );
    #[cfg(feature = "openrouter")]
    assert_eq!(
        catalog_auth_scheme(&entries, "openrouter"),
        ProviderAuthScheme::Bearer
    );
    #[cfg(feature = "anthropic")]
    assert_eq!(
        catalog_auth_scheme(&entries, "anthropic"),
        ProviderAuthScheme::XApiKey
    );
    #[cfg(feature = "gemini")]
    assert_eq!(
        catalog_auth_scheme(&entries, "gemini"),
        ProviderAuthScheme::ApiKey
    );
    #[cfg(feature = "local-llama")]
    assert_eq!(
        catalog_auth_scheme(&entries, "local-llama"),
        ProviderAuthScheme::None
    );
}

#[cfg(feature = "minimax")]
#[test]
fn minimax_provider_catalog_exposes_runtime_and_service_capabilities() {
    let minimax = harness_model::provider_catalog_entries()
        .into_iter()
        .find(|entry| entry.provider_id == "minimax")
        .expect("minimax catalog should exist");

    assert_eq!(minimax.default_base_url, "https://api.minimaxi.com");
    assert!(minimax
        .runtime_capability
        .base_url_regions
        .iter()
        .any(|region| region.base_url == "https://api.minimaxi.com"));
    assert!(minimax
        .runtime_capability
        .base_url_regions
        .iter()
        .any(|region| region.base_url == "https://api.minimax.io"));
    assert!(minimax
        .service_capabilities
        .iter()
        .any(|capability| capability.operation_id == "minimax.image_generation"));
    assert!(!minimax
        .service_capabilities
        .iter()
        .any(|capability| capability.operation_id == "minimax.text_to_speech.websocket"));
    assert!(!minimax
        .service_capabilities
        .iter()
        .any(|capability| capability.execution == ProviderServiceExecution::Websocket));
}

#[cfg(feature = "openai")]
#[test]
fn unsupported_inventory_models_cannot_be_resolved_for_runtime() {
    let openai = provider_inventory_entries()
        .into_iter()
        .find(|entry| entry.provider_id == "openai")
        .expect("openai inventory should exist");
    let unsupported = openai
        .models
        .iter()
        .find(|model| {
            model.model_id == "gpt-image-1"
                && matches!(
                    model.runtime_status,
                    harness_model::ModelRuntimeStatus::Unsupported { .. }
                )
        })
        .expect("openai image model should be inventory-only");

    let error = resolve_model_descriptor(&openai.provider_id, &unsupported.model_id).unwrap_err();

    assert!(matches!(
        error,
        ProviderRegistryError::UnsupportedModel { .. }
    ));
}

fn catalog_auth_scheme(
    entries: &[harness_model::ProviderCatalogEntry],
    provider_id: &str,
) -> ProviderAuthScheme {
    entries
        .iter()
        .find(|entry| entry.provider_id == provider_id)
        .unwrap_or_else(|| panic!("{provider_id} catalog should exist"))
        .runtime_capability
        .auth_scheme
}

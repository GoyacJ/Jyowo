use harness_model::{
    build_provider, provider_catalog_entries, provider_inventory_entries, resolve_model_descriptor,
    ConversationModelCapability, ModelRuntimeSemantics, ProviderBuildConfig, ProviderRegistryError,
    ReasoningProtocolSemantics,
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
        provider_defaults: None,
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

#[cfg(any(
    feature = "openai",
    feature = "openrouter",
    feature = "anthropic",
    feature = "gemini",
    feature = "local-llama"
))]
#[test]
fn provider_catalog_auth_schemes_match_runtime_adapters() {
    let entries = harness_model::provider_catalog_entries();

    #[cfg(feature = "openai")]
    assert_eq!(
        catalog_auth_scheme(&entries, "openai"),
        harness_model::ProviderAuthScheme::Bearer
    );
    #[cfg(feature = "openrouter")]
    assert_eq!(
        catalog_auth_scheme(&entries, "openrouter"),
        harness_model::ProviderAuthScheme::Bearer
    );
    #[cfg(feature = "anthropic")]
    assert_eq!(
        catalog_auth_scheme(&entries, "anthropic"),
        harness_model::ProviderAuthScheme::XApiKey
    );
    #[cfg(feature = "gemini")]
    assert_eq!(
        catalog_auth_scheme(&entries, "gemini"),
        harness_model::ProviderAuthScheme::ApiKey
    );
    #[cfg(feature = "local-llama")]
    assert_eq!(
        catalog_auth_scheme(&entries, "local-llama"),
        harness_model::ProviderAuthScheme::None
    );
}

#[test]
fn every_runtime_descriptor_has_explicit_semantics() {
    assert!(
        descriptor_blocks_missing_runtime_semantics().is_empty(),
        "all ModelDescriptor construction sites must set runtime_semantics explicitly"
    );

    let entries = provider_catalog_entries();
    for entry in entries {
        for descriptor in entry.models {
            assert_eq!(
                descriptor.runtime_semantics,
                expected_runtime_semantics(&entry.provider_id),
                "{}:{} should use the provider's explicit runtime semantics",
                entry.provider_id,
                descriptor.model_id
            );
            assert_eq!(
                descriptor.runtime_semantics.protocol, descriptor.protocol,
                "{}:{} runtime semantics protocol must match descriptor protocol",
                entry.provider_id, descriptor.model_id
            );
        }
    }
}

#[test]
fn public_capability_projection_does_not_expose_private_replay() {
    let private_semantics = ModelRuntimeSemantics::openai_chat_deepseek();
    assert!(matches!(
        private_semantics.reasoning_protocol,
        ReasoningProtocolSemantics::ProviderPrivateReplay { .. }
    ));

    let serialized = serde_json::to_string(&ConversationModelCapability::default()).unwrap();
    for forbidden in private_runtime_semantics_fields() {
        assert!(
            !serialized.contains(forbidden),
            "public capability unexpectedly exposed {forbidden}: {serialized}"
        );
    }
}

#[test]
fn provider_registry_resolves_deepseek_with_private_replay_semantics() {
    #[cfg(feature = "deepseek")]
    {
        let descriptor = resolve_model_descriptor("deepseek", "deepseek-v4-flash")
            .expect("deepseek descriptor should resolve");

        assert_eq!(
            descriptor.runtime_semantics.reasoning_protocol,
            ReasoningProtocolSemantics::ProviderPrivateReplay {
                continuation_kind:
                    harness_provider_state::ProviderContinuationKind::ReasoningReplay,
                required_for_assistant_tool_replay: true,
            }
        );
    }

    #[cfg(not(feature = "deepseek"))]
    assert_source_contains(
        "deepseek.rs",
        "ModelRuntimeSemantics::openai_chat_deepseek()",
    );
}

#[test]
fn provider_registry_resolves_minimax_without_private_replay_requirement() {
    #[cfg(feature = "minimax")]
    {
        let descriptor = resolve_model_descriptor("minimax", "MiniMax-M3")
            .expect("minimax descriptor should resolve");

        assert_eq!(
            descriptor.runtime_semantics.reasoning_protocol,
            ReasoningProtocolSemantics::None
        );
    }

    #[cfg(not(feature = "minimax"))]
    assert_source_contains("minimax.rs", "ModelRuntimeSemantics::openai_chat_minimax()");
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
    assert!(!minimax.service_capabilities.iter().any(
        |capability| capability.execution == harness_model::ProviderServiceExecution::Websocket
    ));
}

#[cfg(feature = "openai")]
#[test]
fn inventory_only_models_are_not_runtime_resolvable() {
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

#[cfg(any(
    feature = "openai",
    feature = "openrouter",
    feature = "anthropic",
    feature = "gemini",
    feature = "local-llama"
))]
fn catalog_auth_scheme(
    entries: &[harness_model::ProviderCatalogEntry],
    provider_id: &str,
) -> harness_model::ProviderAuthScheme {
    entries
        .iter()
        .find(|entry| entry.provider_id == provider_id)
        .unwrap_or_else(|| panic!("{provider_id} catalog should exist"))
        .runtime_capability
        .auth_scheme
}

fn expected_runtime_semantics(provider_id: &str) -> ModelRuntimeSemantics {
    match provider_id {
        "anthropic" => ModelRuntimeSemantics::anthropic_messages_default(),
        "codex" | "openai" => ModelRuntimeSemantics::openai_responses_default(),
        "deepseek" => ModelRuntimeSemantics::openai_chat_deepseek(),
        "gemini" => ModelRuntimeSemantics::gemini_default(),
        "bedrock" => ModelRuntimeSemantics::bedrock_converse_default(),
        "minimax" => ModelRuntimeSemantics::openai_chat_minimax(),
        "qwen" => ModelRuntimeSemantics::openai_responses_default(),
        "doubao" | "km" | "local-llama" | "openrouter" | "zhipu" => {
            ModelRuntimeSemantics::openai_chat_plain()
        }
        provider_id => {
            panic!("missing explicit runtime semantics expectation for provider {provider_id}")
        }
    }
}

fn descriptor_blocks_missing_runtime_semantics() -> Vec<String> {
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut missing = Vec::new();
    collect_missing_descriptor_blocks(&src_dir, &mut missing);
    missing
}

fn collect_missing_descriptor_blocks(path: &std::path::Path, missing: &mut Vec<String>) {
    if path.is_dir() {
        for entry in std::fs::read_dir(path).expect("model source dir should be readable") {
            collect_missing_descriptor_blocks(
                &entry.expect("model source entry should be readable").path(),
                missing,
            );
        }
        return;
    }
    if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
        return;
    }

    let source = std::fs::read_to_string(path).expect("model source file should be readable");
    let mut search_from = 0;
    while let Some(relative_start) = source[search_from..].find("ModelDescriptor {") {
        let start = search_from + relative_start;
        let end = descriptor_block_end(&source, start)
            .unwrap_or_else(|| panic!("descriptor block should close in {}", path.display()));
        let block = &source[start..end];
        if !block.contains("runtime_semantics:") {
            missing.push(format!(
                "{}:{}",
                path.display(),
                source[..start].lines().count() + 1
            ));
        }
        search_from = end;
    }
}

fn descriptor_block_end(source: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, byte) in source[start..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(start + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(any(not(feature = "deepseek"), not(feature = "minimax")))]
fn assert_source_contains(relative_path: &str, needle: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(relative_path);
    let source = std::fs::read_to_string(path).expect("provider source should be readable");
    assert!(
        source.contains(needle),
        "{relative_path} should contain {needle}"
    );
}

fn private_runtime_semantics_fields() -> [&'static str; 8] {
    [
        "runtimeSemantics",
        "runtime_semantics",
        "ProviderContinuation",
        "providerContinuation",
        "provider_continuation",
        "reasoningContent",
        concat!("reasoning", "_content"),
        "ProviderPrivateReplay",
    ]
}

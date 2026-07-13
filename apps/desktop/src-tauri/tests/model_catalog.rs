use std::fs;
use std::ops::Deref;

use jyowo_desktop_shell::commands::{
    get_model_settings_page_with_runtime_state, list_model_provider_catalog_payload,
    DesktopRuntimeState,
};
use jyowo_harness_sdk::ext::{
    build_provider, inventory_from_models_api_json, resolve_model_descriptor,
    runnable_inventory_models, ProviderBuildConfig,
};
use serde_json::json;
use tempfile::TempDir;

struct CatalogRuntime {
    runtime: DesktopRuntimeState,
    _temp_dir: TempDir,
}

impl Deref for CatalogRuntime {
    type Target = DesktopRuntimeState;

    fn deref(&self) -> &Self::Target {
        &self.runtime
    }
}

fn runtime_with_catalog_snapshot(snapshot: serde_json::Value) -> CatalogRuntime {
    let temp_dir = tempfile::tempdir().unwrap();
    let workspace = temp_dir.path().to_path_buf();
    fs::create_dir_all(workspace.join(".jyowo/runtime")).unwrap();
    fs::write(
        workspace.join(".jyowo/runtime/provider-catalog-snapshot.json"),
        serde_json::to_vec_pretty(&snapshot).unwrap(),
    )
    .unwrap();
    CatalogRuntime {
        runtime: DesktopRuntimeState::with_workspace_for_test(workspace).unwrap(),
        _temp_dir: temp_dir,
    }
}

#[tokio::test]
async fn model_catalog_does_not_make_unknown_anthropic_inventory_runnable() {
    let runtime = runtime_with_catalog_snapshot(json!({
        "openrouterModelsApiJson": { "data": [] },
        "anthropicModelsApiJson": {
            "data": [{
                "id": "claude-unknown-dynamic-model",
                "display_name": "Unknown dynamic Claude"
            }]
        },
        "deepseekModelsApiJson": null,
        "lastSuccessfulRefreshAt": "2026-07-13T00:00:00Z",
        "lastAttemptAt": "2026-07-13T00:00:00Z"
    }));

    let page = get_model_settings_page_with_runtime_state(&runtime)
        .await
        .unwrap();
    let anthropic = page
        .catalog
        .providers
        .iter()
        .find(|provider| provider.provider_id == "anthropic")
        .expect("Anthropic provider must exist");
    let unknown = anthropic
        .models
        .iter()
        .find(|model| model.model_id == "claude-unknown-dynamic-model")
        .expect("unknown Anthropic inventory must remain visible");

    assert_eq!(unknown.runtime_status.kind, "unsupported");
    assert!(unknown
        .runtime_status
        .reason
        .as_deref()
        .is_some_and(|reason| reason.contains("not registered")));
    assert!(unknown.supported_protocols.is_empty());
    assert!(unknown.supported_parameters.is_empty());
    assert_eq!(unknown.context_window, 0);
    assert_eq!(unknown.max_output_tokens, 0);
    assert!(unknown.conversation_capability.input_modalities.is_empty());
    assert!(unknown.conversation_capability.output_modalities.is_empty());
    assert!(!unknown.conversation_capability.streaming);
    assert!(!unknown.conversation_capability.tool_calling);
}

#[tokio::test]
async fn model_catalog_uses_dynamic_anthropic_data_only_as_known_descriptor_metadata() {
    let bundled = list_model_provider_catalog_payload();
    let bundled_model = bundled
        .providers
        .iter()
        .find(|provider| provider.provider_id == "anthropic")
        .and_then(|provider| provider.models.first())
        .cloned()
        .expect("Anthropic must have a registry-backed model");
    let runtime = runtime_with_catalog_snapshot(json!({
        "openrouterModelsApiJson": { "data": [] },
        "anthropicModelsApiJson": {
            "data": [{
                "id": bundled_model.model_id,
                "display_name": "Name from Anthropic inventory",
                "max_input_tokens": 1,
                "max_tokens": 1,
                "capabilities": {
                    "tool_use": false,
                    "thinking": false
                }
            }]
        },
        "deepseekModelsApiJson": null,
        "lastSuccessfulRefreshAt": "2026-07-13T00:00:00Z",
        "lastAttemptAt": "2026-07-13T00:00:00Z"
    }));

    let page = get_model_settings_page_with_runtime_state(&runtime)
        .await
        .unwrap();
    let refreshed = page
        .catalog
        .providers
        .iter()
        .find(|provider| provider.provider_id == "anthropic")
        .and_then(|provider| {
            provider
                .models
                .iter()
                .find(|model| model.model_id == bundled_model.model_id)
        })
        .expect("known Anthropic model must remain visible");

    assert_eq!(refreshed.display_name, "Name from Anthropic inventory");
    assert_eq!(refreshed.protocol, bundled_model.protocol);
    assert_eq!(
        refreshed.supported_protocols,
        bundled_model.supported_protocols
    );
    assert_eq!(
        refreshed.supported_parameters,
        bundled_model.supported_parameters
    );
    assert_eq!(refreshed.context_window, bundled_model.context_window);
    assert_eq!(refreshed.max_output_tokens, bundled_model.max_output_tokens);
    assert_eq!(
        refreshed.conversation_capability,
        bundled_model.conversation_capability
    );
    assert_eq!(refreshed.lifecycle, bundled_model.lifecycle);
    assert_eq!(refreshed.runtime_status, bundled_model.runtime_status);
}

#[tokio::test]
async fn model_catalog_does_not_make_unknown_deepseek_snapshot_inventory_runnable() {
    let runtime = runtime_with_catalog_snapshot(json!({
        "openrouterModelsApiJson": { "data": [] },
        "anthropicModelsApiJson": null,
        "deepseekModelsApiJson": {
            "data": [{ "id": "deepseek-unknown-dynamic-model" }]
        },
        "lastSuccessfulRefreshAt": "2026-07-13T00:00:00Z",
        "lastAttemptAt": "2026-07-13T00:00:00Z"
    }));

    let page = get_model_settings_page_with_runtime_state(&runtime)
        .await
        .unwrap();
    let deepseek = page
        .catalog
        .providers
        .iter()
        .find(|provider| provider.provider_id == "deepseek")
        .expect("DeepSeek provider must exist");

    assert!(deepseek.models.iter().all(|model| {
        model.model_id != "deepseek-unknown-dynamic-model"
            || model.runtime_status.kind != "runnable"
    }));
}

#[tokio::test]
async fn model_catalog_keeps_unsupported_openrouter_inventory_visible() {
    let runtime = runtime_with_catalog_snapshot(json!({
        "openrouterModelsApiJson": {
            "data": [{
                "id": "vendor/dynamic-image-model",
                "name": "Dynamic image model",
                "context_length": 32000,
                "architecture": {
                    "input_modalities": ["text"],
                    "output_modalities": ["image"]
                },
                "top_provider": { "max_completion_tokens": 4096 },
                "supported_parameters": []
            }]
        },
        "anthropicModelsApiJson": null,
        "lastSuccessfulRefreshAt": "2026-07-13T00:00:00Z",
        "lastAttemptAt": "2026-07-13T00:00:00Z"
    }));

    let page = get_model_settings_page_with_runtime_state(&runtime)
        .await
        .unwrap();
    let unsupported = page
        .catalog
        .providers
        .iter()
        .find(|provider| provider.provider_id == "openrouter")
        .and_then(|provider| {
            provider
                .models
                .iter()
                .find(|model| model.model_id == "vendor/dynamic-image-model")
        })
        .expect("unsupported OpenRouter inventory must remain visible");

    assert_eq!(unsupported.runtime_status.kind, "unsupported");
    assert_eq!(
        unsupported.runtime_status.reason.as_deref(),
        Some("model modalities are not supported by the current runtime")
    );
    assert_eq!(
        serde_json::to_value(&unsupported.conversation_capability).unwrap()["outputModalities"],
        json!(["image"])
    );
}

#[tokio::test]
async fn model_catalog_marks_only_descriptor_backed_constructible_models_runnable() {
    let openrouter_json = json!({
        "data": [
            {
                "id": "vendor/dynamic-text-model",
                "name": "Dynamic text model",
                "context_length": 128000,
                "architecture": {
                    "input_modalities": ["text"],
                    "output_modalities": ["text"]
                },
                "top_provider": { "max_completion_tokens": 8192 },
                "supported_parameters": ["tools"]
            },
            {
                "id": "vendor/dynamic-image-model",
                "name": "Dynamic image model",
                "context_length": 32000,
                "architecture": {
                    "input_modalities": ["text"],
                    "output_modalities": ["image"]
                },
                "top_provider": { "max_completion_tokens": 4096 },
                "supported_parameters": []
            }
        ]
    });
    let openrouter_inventory = inventory_from_models_api_json(
        &serde_json::to_vec(&openrouter_json).expect("inventory must serialize"),
    )
    .expect("inventory must parse");
    let openrouter_descriptors = runnable_inventory_models(&openrouter_inventory);
    assert!(openrouter_descriptors
        .iter()
        .all(|descriptor| descriptor.model_id != "vendor/dynamic-image-model"));
    let runtime = runtime_with_catalog_snapshot(json!({
        "openrouterModelsApiJson": openrouter_json,
        "anthropicModelsApiJson": {
            "data": [{ "id": "claude-unknown-dynamic-model" }]
        },
        "deepseekModelsApiJson": {
            "data": [{ "id": "deepseek-unknown-dynamic-model" }]
        },
        "lastSuccessfulRefreshAt": "2026-07-13T00:00:00Z",
        "lastAttemptAt": "2026-07-13T00:00:00Z"
    }));

    let page = get_model_settings_page_with_runtime_state(&runtime)
        .await
        .unwrap();
    let unsupported_anthropic = page
        .catalog
        .providers
        .iter()
        .find(|provider| provider.provider_id == "anthropic")
        .and_then(|provider| {
            provider
                .models
                .iter()
                .find(|model| model.model_id == "claude-unknown-dynamic-model")
        })
        .expect("unsupported Anthropic inventory must remain visible");
    assert_eq!(unsupported_anthropic.runtime_status.kind, "unsupported");
    let unsupported_openrouter = page
        .catalog
        .providers
        .iter()
        .find(|provider| provider.provider_id == "openrouter")
        .and_then(|provider| {
            provider
                .models
                .iter()
                .find(|model| model.model_id == "vendor/dynamic-image-model")
        })
        .expect("unsupported OpenRouter inventory must remain visible");
    assert_eq!(unsupported_openrouter.runtime_status.kind, "unsupported");

    for provider in &page.catalog.providers {
        for model in provider
            .models
            .iter()
            .filter(|model| model.runtime_status.kind == "runnable")
        {
            let descriptor = if provider.provider_id == "openrouter" {
                openrouter_descriptors
                    .iter()
                    .find(|descriptor| descriptor.model_id == model.model_id)
                    .cloned()
                    .expect("runnable OpenRouter model must have an inventory descriptor")
            } else {
                resolve_model_descriptor(&provider.provider_id, &model.model_id)
                    .expect("runnable registry model must resolve")
            };
            build_provider(ProviderBuildConfig {
                provider_id: provider.provider_id.clone(),
                api_key: "catalog-construction-test".to_owned(),
                base_url: None,
                model_descriptor: Some(descriptor),
                provider_defaults: None,
            })
            .unwrap_or_else(|error| {
                panic!(
                    "runnable model {}/{} must construct: {error}",
                    provider.provider_id, model.model_id
                )
            });
        }
    }
}

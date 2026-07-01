#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
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

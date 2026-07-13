#![allow(unused_imports)]

use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

#[tokio::test]
async fn provider_credential_route_provider_only_resolution_requires_run_model_config() {
    let workspace = canonical_unique_workspace("provider-credential-route-provider-only");
    let provider_settings = provider_settings_record_with_minimax_config("minimax-main", true);
    let provider_store = provider_settings_store_for_workspace(&workspace);
    provider_store
        .save_record(&provider_settings)
        .expect("provider settings should save");
    let resolver = desktop_provider_credential_resolver_with_stores(
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
    let provider_store = provider_settings_store_for_workspace(&workspace);
    provider_store
        .save_record(&provider_settings)
        .expect("provider settings should save");
    let resolver = desktop_provider_credential_resolver_with_stores(
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
async fn provider_credential_route_provider_only_resolution_still_works() {
    let workspace = canonical_unique_workspace("provider-credential-route-provider-only-success");
    let provider_settings = provider_settings_record_with_minimax_config("minimax-main", true);
    let provider_store = provider_settings_store_for_workspace(&workspace);
    provider_store
        .save_record(&provider_settings)
        .expect("provider settings should save");
    let resolver = desktop_provider_credential_resolver_with_stores(
        Arc::new(provider_store),
        empty_provider_capability_routes(),
    );
    let session_id = SessionId::new();

    let credential = resolver
        .resolve_provider_credential(ProviderCredentialResolveContext {
            tenant_id: TenantId::SINGLE,
            session_id,
            run_id: RunId::new(),
            provider_id: "minimax".to_owned(),
            model_config_id: Some("minimax-main".to_owned()),
            operation_id: None,
            route_kind: None,
        })
        .await
        .expect("provider-only credential resolution should succeed");

    assert_eq!(credential.provider_id, "minimax");
    assert_eq!(credential.config_id, "minimax-main");
    assert!(!credential.api_key.is_empty());
}

#[tokio::test]
async fn provider_credential_route_routed_service_context_fails_closed_without_route() {
    let workspace = canonical_unique_workspace("provider-credential-route-routed-fail-closed");
    let provider_settings = provider_settings_record_with_minimax_config("minimax-main", true);
    let provider_store = provider_settings_store_for_workspace(&workspace);
    provider_store
        .save_record(&provider_settings)
        .expect("provider settings should save");
    let resolver = desktop_provider_credential_resolver_with_stores(
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
    let provider_store = provider_settings_store_for_workspace(&workspace);
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
    let resolver =
        desktop_provider_credential_resolver_with_stores(Arc::new(provider_store), routes);

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
    let provider_store = provider_settings_store_for_workspace(&workspace);
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
    let resolver =
        desktop_provider_credential_resolver_with_stores(Arc::new(provider_store), routes);

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
    let provider_store = provider_settings_store_for_workspace(&workspace);
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
    let resolver =
        desktop_provider_credential_resolver_with_stores(Arc::new(provider_store), routes);

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
    let provider_store = provider_settings_store_for_workspace(&workspace);
    provider_store
        .save_record(&provider_settings_with_openai_and_minimax(
            "openai-main",
            "minimax-image",
            "route-specific-token",
        ))
        .expect("provider settings should save");
    let resolver = desktop_provider_credential_resolver_with_stores(
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

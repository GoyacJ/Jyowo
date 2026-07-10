use harness_contracts::{
    validate_provider_capability_route, CapabilityRouteKind,
    ListProviderCapabilityRouteOptionsResponse, ModelModality, ProviderCapabilityRoute,
    ProviderCapabilityRouteOption, ProviderCapabilityRouteSettings,
    ProviderCredentialResolveContext, ProviderServiceAdapterAvailability, ProviderServiceCostRisk,
    ProviderServiceExecution, RunId, SessionId, TenantId, ToolServiceBinding,
};
use serde_json::json;

#[test]
fn capability_route_kind_serializes_as_snake_case() {
    assert_eq!(
        serde_json::to_value(CapabilityRouteKind::ImageGeneration).unwrap(),
        json!("image_generation")
    );
    assert_eq!(
        serde_json::to_value(CapabilityRouteKind::ThreeDGeneration).unwrap(),
        json!("three_d_generation")
    );
    assert_eq!(
        serde_json::to_value(CapabilityRouteKind::EmbeddingGeneration).unwrap(),
        json!("embedding_generation")
    );
    assert_eq!(
        serde_json::to_value(CapabilityRouteKind::FileOperation).unwrap(),
        json!("file_operation")
    );
}

#[test]
fn provider_capability_route_serializes_as_camel_case() {
    let route = ProviderCapabilityRoute {
        kind: CapabilityRouteKind::VideoGeneration,
        config_id: "minimax-video".to_owned(),
        provider_id: "minimax".to_owned(),
        operation_ids: vec!["minimax.video_generation".to_owned()],
        enabled: true,
    };

    assert_eq!(
        serde_json::to_value(route).unwrap(),
        json!({
            "kind": "video_generation",
            "configId": "minimax-video",
            "providerId": "minimax",
            "operationIds": ["minimax.video_generation"],
            "enabled": true
        })
    );
}

#[test]
fn provider_capability_route_settings_rejects_unknown_fields() {
    let value = json!({
        "version": 1,
        "routes": [],
        "extra": true
    });

    assert!(serde_json::from_value::<ProviderCapabilityRouteSettings>(value).is_err());
}

#[test]
fn provider_capability_route_option_omits_absent_unavailable_reason() {
    let option = ProviderCapabilityRouteOption {
        kind: CapabilityRouteKind::ImageGeneration,
        config_id: "minimax-image".to_owned(),
        provider_id: "minimax".to_owned(),
        operation_id: "minimax.image_generation".to_owned(),
        output_artifact: ModelModality::Image,
        execution: ProviderServiceExecution::Sync,
        cost_risk: ProviderServiceCostRisk::High,
        runtime_supported: true,
        unavailable_reason: None,
    };

    let value = serde_json::to_value(option).unwrap();

    assert_eq!(value["runtimeSupported"], true);
    assert_eq!(value.get("unavailableReason"), None);
}

#[test]
fn list_provider_capability_route_options_response_serializes_as_camel_case() {
    let response = ListProviderCapabilityRouteOptionsResponse {
        options: vec![ProviderCapabilityRouteOption {
            kind: CapabilityRouteKind::TextToSpeech,
            config_id: "minimax-tts".to_owned(),
            provider_id: "minimax".to_owned(),
            operation_id: "minimax.text_to_speech.sync".to_owned(),
            output_artifact: ModelModality::Audio,
            execution: ProviderServiceExecution::Sync,
            cost_risk: ProviderServiceCostRisk::Medium,
            runtime_supported: false,
            unavailable_reason: Some("No runtime adapter".to_owned()),
        }],
    };

    let value = serde_json::to_value(response).unwrap();

    assert_eq!(
        value,
        json!({
            "options": [{
                "kind": "text_to_speech",
                "configId": "minimax-tts",
                "providerId": "minimax",
                "operationId": "minimax.text_to_speech.sync",
                "outputArtifact": "audio",
                "execution": "sync",
                "costRisk": "medium",
                "runtimeSupported": false,
                "unavailableReason": "No runtime adapter"
            }]
        })
    );
}

#[test]
fn validate_provider_capability_route_rejects_empty_operation_ids() {
    let route = ProviderCapabilityRoute {
        kind: CapabilityRouteKind::ImageGeneration,
        config_id: "minimax-image".to_owned(),
        provider_id: "minimax".to_owned(),
        operation_ids: Vec::new(),
        enabled: true,
    };

    assert!(validate_provider_capability_route(&route).is_err());
}

#[test]
fn validate_provider_capability_route_rejects_duplicate_operation_ids() {
    let route = ProviderCapabilityRoute {
        kind: CapabilityRouteKind::ImageGeneration,
        config_id: "minimax-image".to_owned(),
        provider_id: "minimax".to_owned(),
        operation_ids: vec![
            "minimax.image_generation".to_owned(),
            "minimax.image_generation".to_owned(),
        ],
        enabled: true,
    };

    assert!(validate_provider_capability_route(&route).is_err());
}

#[test]
fn tool_service_binding_serializes_as_camel_case() {
    let binding = ToolServiceBinding {
        provider_id: "minimax".to_owned(),
        operation_id: "minimax.music_generation".to_owned(),
        route_kind: CapabilityRouteKind::MusicGeneration,
        output_artifact: ModelModality::Audio,
    };

    assert_eq!(
        serde_json::to_value(binding).unwrap(),
        json!({
            "providerId": "minimax",
            "operationId": "minimax.music_generation",
            "routeKind": "music_generation",
            "outputArtifact": "audio"
        })
    );
}

#[test]
fn provider_service_adapter_availability_serializes_descriptor_bindings() {
    let availability = ProviderServiceAdapterAvailability {
        bindings: vec![ToolServiceBinding {
            provider_id: "minimax".to_owned(),
            operation_id: "minimax.image_generation".to_owned(),
            route_kind: CapabilityRouteKind::ImageGeneration,
            output_artifact: ModelModality::Image,
        }],
    };

    let value = serde_json::to_value(availability).unwrap();

    assert_eq!(value["bindings"][0]["providerId"], "minimax");
    assert_eq!(
        value["bindings"][0]["operationId"],
        "minimax.image_generation"
    );
    assert_eq!(value.get("providers"), None);
}

#[test]
fn provider_credential_resolve_context_round_trips_with_operation_scope() {
    let context = ProviderCredentialResolveContext {
        tenant_id: TenantId::new(),
        session_id: SessionId::new(),
        run_id: RunId::new(),
        provider_id: "minimax".to_owned(),
        model_config_id: Some("minimax-main".to_owned()),
        operation_id: Some("minimax.image_generation".to_owned()),
        route_kind: Some(CapabilityRouteKind::ImageGeneration),
    };

    let value = serde_json::to_value(&context).unwrap();
    assert_eq!(value["modelConfigId"], "minimax-main");
    assert_eq!(value["operationId"], "minimax.image_generation");
    assert_eq!(value["routeKind"], "image_generation");

    let roundtrip: ProviderCredentialResolveContext = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, context);
}

#[test]
fn provider_credential_resolve_context_keeps_operation_scope_optional() {
    let value = json!({
        "tenant_id": TenantId::new(),
        "session_id": SessionId::new(),
        "run_id": RunId::new(),
        "provider_id": "minimax"
    });

    let context: ProviderCredentialResolveContext = serde_json::from_value(value).unwrap();

    assert_eq!(context.operation_id, None);
    assert_eq!(context.route_kind, None);
    assert_eq!(context.model_config_id, None);
}

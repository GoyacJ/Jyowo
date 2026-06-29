use harness_contracts::{
    export_all_schemas, ManifestOriginRef, PluginConfigUpdate, PluginDetail, PluginId,
    PluginInstallReport, PluginLifecycleStateDiscriminant, PluginOperationResult,
    PluginOperationStatus, PluginProductState, PluginRecentEvent, PluginRuntimeCapability,
    PluginRuntimeCapabilityKind, PluginRuntimeRpcRequest, PluginRuntimeRpcResponse,
    PluginSourceKind, PluginSummary, TrustLevel,
};
use serde_json::json;

#[test]
fn plugin_product_contracts_serialize_camel_case_and_hide_secret_values() {
    let summary = PluginSummary {
        id: PluginId("formatter@1.0.0".to_owned()),
        name: "formatter".to_owned(),
        version: "1.0.0".to_owned(),
        description: Some("Formats workspace files".to_owned()),
        source: PluginSourceKind::User,
        trust_level: TrustLevel::UserControlled,
        enabled: true,
        state: PluginProductState::Activated,
        capabilities: vec![PluginRuntimeCapability {
            kind: PluginRuntimeCapabilityKind::Tool,
            name: Some("format_file".to_owned()),
            destructive: Some(false),
            registered: true,
        }],
        warnings: vec![],
    };

    let value = serde_json::to_value(&summary).unwrap();

    assert_eq!(value["trustLevel"], "user_controlled");
    assert_eq!(value["capabilities"][0]["kind"], "tool");
    assert!(value.get("trust_level").is_none());

    let update = PluginConfigUpdate {
        plugin_id: PluginId("formatter@1.0.0".to_owned()),
        values: json!({ "lineWidth": 100 }),
    };
    let update_value = serde_json::to_value(update).unwrap();

    assert_eq!(update_value["pluginId"], "formatter@1.0.0");
    assert!(!update_value.to_string().contains("secret"));
}

#[test]
fn plugin_product_detail_and_operation_reports_roundtrip() {
    let summary = PluginSummary {
        id: PluginId("formatter@1.0.0".to_owned()),
        name: "formatter".to_owned(),
        version: "1.0.0".to_owned(),
        description: None,
        source: PluginSourceKind::CargoExtension,
        trust_level: TrustLevel::AdminTrusted,
        enabled: false,
        state: PluginProductState::Disabled {
            last_state: Some(PluginLifecycleStateDiscriminant::Validated),
        },
        capabilities: vec![],
        warnings: vec!["optional dependency missing".to_owned()],
    };
    let detail = PluginDetail {
        summary: summary.clone(),
        manifest_origin: ManifestOriginRef::CargoExtension {
            binary: "/bin/jyowo-plugin-formatter".to_owned(),
        },
        manifest_hash: [7; 32],
        manifest: json!({ "name": "formatter", "version": "1.0.0" }),
        configuration_schema: Some(json!({
            "type": "object",
            "properties": {
                "lineWidth": { "type": "number" },
                "apiToken": { "type": "string", "secret": true }
            }
        })),
        config: json!({ "lineWidth": 100 }),
        registered_capabilities: vec![],
        recent_events: vec![PluginRecentEvent::Loaded],
        rejection_reason: None,
        failure: None,
    };

    let encoded = serde_json::to_string(&detail).unwrap();
    let decoded: PluginDetail = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded.summary, summary);
    assert_eq!(decoded.recent_events, vec![PluginRecentEvent::Loaded]);
    assert!(encoded.contains(r#""recentEvents":["loaded"]"#));
    assert!(!encoded.contains("api-token-value"));

    let install_report = PluginInstallReport {
        source_path: "/tmp/formatter".to_owned(),
        valid: true,
        summary: Some(summary.clone()),
        warnings: vec![],
        reason: None,
    };
    let operation = PluginOperationResult {
        plugin_id: Some(summary.id.clone()),
        status: PluginOperationStatus::Installed,
        summary: Some(summary),
        report: Some(install_report),
    };

    let value = serde_json::to_value(operation).unwrap();
    assert_eq!(value["status"], "installed");
    assert_eq!(
        serde_json::from_value::<PluginOperationResult>(value)
            .unwrap()
            .status,
        PluginOperationStatus::Installed
    );
}

#[test]
fn plugin_runtime_rpc_contracts_roundtrip() {
    let request = PluginRuntimeRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: 7,
        method: "tool.execute".to_owned(),
        params: json!({ "tool": "format_file", "input": { "path": "src/lib.rs" } }),
    };
    let response = PluginRuntimeRpcResponse {
        jsonrpc: "2.0".to_owned(),
        id: 7,
        result: Some(json!({ "ok": true })),
        error: None,
    };

    assert_eq!(
        serde_json::from_value::<PluginRuntimeRpcRequest>(serde_json::to_value(&request).unwrap())
            .unwrap(),
        request
    );
    assert_eq!(
        serde_json::from_value::<PluginRuntimeRpcResponse>(
            serde_json::to_value(&response).unwrap()
        )
        .unwrap(),
        response
    );
}

#[test]
fn plugin_product_schemas_are_exported() {
    let schemas = export_all_schemas();

    for key in [
        "plugin_summary",
        "plugin_detail",
        "plugin_install_report",
        "plugin_operation_status",
        "plugin_operation_result",
        "plugin_config_update",
        "plugin_runtime_capability",
        "plugin_runtime_rpc_request",
        "plugin_runtime_rpc_response",
    ] {
        assert!(schemas.contains_key(key), "missing plugin schema: {key}");
    }
}

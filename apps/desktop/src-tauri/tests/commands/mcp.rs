use super::*;

#[tokio::test]
async fn save_mcp_server_payload_rejects_invalid_config_fail_closed() {
    let store = RecordingMcpServerStore::default();
    let error = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            display_name: String::new(),
            id: "bad id".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: String::new(),
                args: Vec::new(),
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(store.record.lock().unwrap().is_none());
}

#[test]
fn mcp_server_config_record_defaults_legacy_stdio_records_to_enabled() {
    let record = serde_json::from_value::<McpServerConfigRecord>(json!({
        "displayName": "Workspace GitHub",
        "id": "github",
        "scope": "global",
        "transport": {
            "kind": "stdio",
            "command": "node",
            "args": ["server.js"]
        }
    }))
    .unwrap();

    assert!(record.enabled);
    assert_eq!(record.display_name, "Workspace GitHub");
}

#[tokio::test]
async fn browser_mcp_presets_are_disabled_until_saved() {
    let store = RecordingMcpServerStore::default();

    let payload = list_browser_mcp_presets_with_store(&store)
        .await
        .expect("browser MCP presets should list");

    assert_eq!(payload.presets.len(), 2);
    assert_eq!(payload.presets[0].id, BrowserMcpPresetId::Playwright);
    assert_eq!(payload.presets[0].server_id, "browser-playwright");
    assert!(!payload.presets[0].enabled);
    assert_eq!(payload.presets[1].id, BrowserMcpPresetId::ChromeDevtools);
    assert_eq!(payload.presets[1].server_id, "browser-chrome-devtools");
    assert!(!payload.presets[1].enabled);
}

#[tokio::test]
async fn browser_mcp_preset_save_writes_disabled_workspace_server_by_default() {
    let store = RecordingMcpServerStore::default();

    let payload = save_browser_mcp_preset_with_store(
        SaveBrowserMcpPresetRequest {
            preset_id: BrowserMcpPresetId::Playwright,
            enabled: false,
        },
        &store,
    )
    .await
    .expect("browser MCP preset should save");
    let stored = store.record.lock().unwrap().clone().unwrap();

    assert_eq!(payload.preset.id, BrowserMcpPresetId::Playwright);
    assert!(!payload.preset.enabled);
    assert_eq!(payload.server.status, "disabled");
    assert!(!stored.enabled);
    assert_eq!(stored.id, "browser-playwright");
    assert!(matches!(
        stored.transport,
        McpServerTransportConfig::Stdio { ref command, ref args, ref env, .. }
            if command == "npx" && args == &vec!["@playwright/mcp@latest".to_owned()] && env.is_empty()
    ));
    assert!(!serde_json::to_string(&stored)
        .unwrap()
        .contains("mcp-secret-token"));
}

#[tokio::test]
async fn browser_mcp_preset_can_be_explicitly_enabled() {
    let store = RecordingMcpServerStore::default();

    let payload = save_browser_mcp_preset_with_store(
        SaveBrowserMcpPresetRequest {
            preset_id: BrowserMcpPresetId::ChromeDevtools,
            enabled: true,
        },
        &store,
    )
    .await
    .expect("browser MCP preset should save");
    let stored = store.record.lock().unwrap().clone().unwrap();

    assert_eq!(payload.preset.id, BrowserMcpPresetId::ChromeDevtools);
    assert!(payload.preset.enabled);
    assert!(payload.server.enabled);
    assert_eq!(stored.id, "browser-chrome-devtools");
    assert!(stored.enabled);
    assert!(matches!(
        stored.transport,
        McpServerTransportConfig::Stdio { ref command, ref args, .. }
            if command == "npx" && args == &vec!["chrome-devtools-mcp@latest".to_owned()]
    ));
}

#[tokio::test]
async fn save_mcp_server_payload_rejects_secret_bearing_stdio_args() {
    let store = RecordingMcpServerStore::default();
    let error = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Workspace GitHub".to_owned(),
            id: "github".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "node".to_owned(),
                args: vec!["--token=mcp-secret-token".to_owned()],
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(store.record.lock().unwrap().is_none());
}

#[tokio::test]
async fn save_mcp_server_payload_rejects_raw_secret_like_stdio_args() {
    let store = RecordingMcpServerStore::default();
    let error = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Workspace GitHub".to_owned(),
            id: "github".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "node".to_owned(),
                args: vec!["ghp_abcdefghijklmnopqrstuvwxyz0123456789".to_owned()],
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(store.record.lock().unwrap().is_none());
}

#[tokio::test]
async fn save_mcp_server_payload_rejects_in_process_workspace_config() {
    let store = RecordingMcpServerStore::default();
    let error = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Internal".to_owned(),
            id: "internal".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::InProcess,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(store.record.lock().unwrap().is_none());
}

#[tokio::test]
async fn save_mcp_server_payload_persists_http_config_without_secret_values() {
    let store = RecordingMcpServerStore::default();
    let payload = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Remote Context".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Http {
                url: "https://mcp.example.com/mcp".to_owned(),
                bearer_token_env_var: Some("MCP_BEARER_TOKEN".to_owned()),
                headers: vec![McpNameValueRecord {
                    key: "X-Workspace".to_owned(),
                    value: "jyowo".to_owned(),
                }],
                headers_from_env: vec![McpHeaderEnvRecord {
                    key: "X-Api-Key".to_owned(),
                    env_var: "MCP_CONTEXT7_TOKEN".to_owned(),
                }],
            },
        },
        &store,
    )
    .await
    .unwrap();
    let stored = store.record.lock().unwrap().clone().unwrap();

    assert!(payload.server.enabled);
    assert!(payload.server.manageable);
    assert_eq!(payload.server.transport, "http");
    assert_eq!(stored.enabled, true);
    assert_eq!(
        serde_json::to_string(&stored).unwrap().contains("secret"),
        false
    );
}

#[tokio::test]
async fn get_mcp_server_config_with_store_returns_workspace_managed_record() {
    let store = RecordingMcpServerStore::default();
    save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Remote Context".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Http {
                url: "https://mcp.example.com/mcp".to_owned(),
                bearer_token_env_var: Some("MCP_BEARER_TOKEN".to_owned()),
                headers: vec![McpNameValueRecord {
                    key: "X-Workspace".to_owned(),
                    value: "jyowo".to_owned(),
                }],
                headers_from_env: vec![McpHeaderEnvRecord {
                    key: "X-Api-Key".to_owned(),
                    env_var: "MCP_CONTEXT7_TOKEN".to_owned(),
                }],
            },
        },
        &store,
    )
    .await
    .unwrap();

    let payload = get_mcp_server_config_with_store(
        GetMcpServerConfigRequest {
            id: "context7".to_owned(),
        },
        &store,
    )
    .await
    .unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();

    assert_eq!(payload.server.display_name, "Remote Context");
    assert_eq!(payload.server.id, "context7");
    assert!(matches!(
        payload.server.transport,
        McpServerTransportConfig::Http { .. }
    ));
    assert!(serialized.contains("MCP_BEARER_TOKEN"));
    assert!(!serialized.contains("mcp-secret-token"));
}

#[tokio::test]
async fn get_mcp_server_config_with_runtime_state_rejects_unmanaged_runtime_server() {
    let server_id = McpServerId("plugin-context".to_owned());
    let mcp_registry = McpRegistry::new();
    mcp_registry
        .add_ready_server(
            McpServerSpec::new(
                server_id.clone(),
                "Plugin Context",
                TransportChoice::InProcess,
                McpServerSource::Plugin(harness_contracts::PluginId("context".to_owned())),
            ),
            McpServerScope::Global,
            Arc::new(StaticMcpConnection { tools: Vec::new() }),
        )
        .await
        .unwrap();
    let state = runtime_state_with_mcp_registry(mcp_registry, vec![server_id]).await;

    let error = get_mcp_server_config_with_runtime_state(
        GetMcpServerConfigRequest {
            id: "plugin-context".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "NOT_FOUND");
}

#[tokio::test]
async fn save_mcp_server_payload_rejects_secret_bearing_http_headers() {
    let store = RecordingMcpServerStore::default();
    let error = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Remote Context".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Http {
                url: "https://mcp.example.com/mcp".to_owned(),
                bearer_token_env_var: None,
                headers: vec![McpNameValueRecord {
                    key: "Authorization".to_owned(),
                    value: "Bearer mcp-secret-token".to_owned(),
                }],
                headers_from_env: Vec::new(),
            },
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(store.record.lock().unwrap().is_none());
}

#[tokio::test]
async fn save_mcp_server_payload_rejects_cookie_headers_from_env() {
    let store = RecordingMcpServerStore::default();
    let error = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Remote Browser".to_owned(),
            id: "remote-browser".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Http {
                url: "https://mcp.example.com/mcp".to_owned(),
                bearer_token_env_var: None,
                headers: Vec::new(),
                headers_from_env: vec![McpHeaderEnvRecord {
                    key: "Cookie".to_owned(),
                    env_var: "BROWSER_COOKIE".to_owned(),
                }],
            },
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(store.record.lock().unwrap().is_none());
}

#[test]
fn save_mcp_server_payload_rejects_unknown_transport_fields() {
    let error = serde_json::from_value::<SaveMcpServerRequest>(json!({
        "enabled": true,
        "displayName": "Workspace GitHub",
        "id": "github",
        "scope": "global",
        "transport": {
            "kind": "stdio",
            "command": "node",
            "args": [],
            "envMap": { "GITHUB_TOKEN": "secret" }
        }
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown field"));
}

#[tokio::test(flavor = "current_thread")]
async fn save_mcp_server_with_runtime_state_registers_and_injects_stdio_tools() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("mcp-save-registers");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;

    let payload = save_mcp_server_with_runtime_state(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Workspace Stdio".to_owned(),
            id: "stdio".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "/bin/sh".to_owned(),
                args: vec!["-c".to_owned(), stdio_mcp_fixture_script()],
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &state,
    )
    .await
    .unwrap();
    let harness = state.harness().unwrap();

    assert_eq!(payload.server.status, "ready");
    assert_eq!(payload.server.exposed_tool_count, 1);
    assert!(harness.tool_registry().get("mcp__stdio__echo").is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_mcp_server_with_runtime_state_does_not_register_or_inject_tools() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("mcp-disabled");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;

    let payload = save_mcp_server_with_runtime_state(
        SaveMcpServerRequest {
            enabled: false,
            display_name: "Workspace Stdio".to_owned(),
            id: "stdio".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "/bin/sh".to_owned(),
                args: vec!["-c".to_owned(), stdio_mcp_fixture_script()],
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &state,
    )
    .await
    .unwrap();
    let harness = state.harness().unwrap();

    assert_eq!(payload.server.status, "disabled");
    assert!(!payload.server.enabled);
    assert!(harness
        .mcp_config()
        .unwrap()
        .registry
        .server_spec(&McpServerId("stdio".to_owned()))
        .await
        .is_none());
    assert!(harness.tool_registry().get("mcp__stdio__echo").is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn set_mcp_server_enabled_registers_and_injects_tools() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("mcp-enable");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;
    save_mcp_server_with_runtime_state(
        SaveMcpServerRequest {
            enabled: false,
            display_name: "Workspace Stdio".to_owned(),
            id: "stdio".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "/bin/sh".to_owned(),
                args: vec!["-c".to_owned(), stdio_mcp_fixture_script()],
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &state,
    )
    .await
    .unwrap();

    let payload = set_mcp_server_enabled_with_runtime_state(
        SetMcpServerEnabledRequest {
            id: "stdio".to_owned(),
            enabled: true,
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(payload.server.status, "ready");
    assert!(payload.server.enabled);
    assert!(state
        .harness()
        .unwrap()
        .tool_registry()
        .get("mcp__stdio__echo")
        .is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn restart_mcp_server_removes_registers_and_injects_tools() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("mcp-restart");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;
    save_mcp_server_with_runtime_state(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Workspace Stdio".to_owned(),
            id: "stdio".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "/bin/sh".to_owned(),
                args: vec!["-c".to_owned(), stdio_mcp_fixture_script()],
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &state,
    )
    .await
    .unwrap();

    let payload = restart_mcp_server_with_runtime_state(
        RestartMcpServerRequest {
            id: "stdio".to_owned(),
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(payload.server.status, "ready");
    assert!(state
        .harness()
        .unwrap()
        .tool_registry()
        .get("mcp__stdio__echo")
        .is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn http_mcp_server_with_runtime_state_registers_as_http_transport() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let _env = EnvVarGuard::set(
        "MCP_TEST_BEARER",
        std::ffi::OsStr::new("not-secret-test-token"),
    );
    let workspace = unique_workspace("mcp-http-register");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;

    let payload = save_mcp_server_with_runtime_state(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Remote Context".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Http {
                url: "http://127.0.0.1:9/mcp".to_owned(),
                bearer_token_env_var: Some("MCP_TEST_BEARER".to_owned()),
                headers: vec![McpNameValueRecord {
                    key: "X-Workspace".to_owned(),
                    value: "jyowo".to_owned(),
                }],
                headers_from_env: Vec::new(),
            },
        },
        &state,
    )
    .await
    .unwrap();
    let spec = state
        .harness()
        .unwrap()
        .mcp_config()
        .unwrap()
        .registry
        .server_spec(&McpServerId("context7".to_owned()))
        .await
        .unwrap();

    assert_eq!(payload.server.transport, "http");
    assert!(matches!(spec.transport, TransportChoice::Http { .. }));
}

#[tokio::test]
async fn delete_mcp_server_payload_is_idempotent_for_missing_server() {
    let store = RecordingMcpServerStore::default();
    let payload = delete_mcp_server_with_store(
        DeleteMcpServerRequest {
            id: "github".to_owned(),
        },
        &store,
    )
    .await
    .unwrap();

    assert_eq!(payload.id, "github");
    assert_eq!(payload.status, "deleted");
}

#[tokio::test(flavor = "current_thread")]
async fn delete_mcp_server_with_runtime_state_removes_registry_server_and_injected_tools() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("mcp-delete-runtime");
    std::fs::create_dir_all(&workspace).unwrap();
    let server_id = McpServerId("github".to_owned());
    let mcp_registry = McpRegistry::new();
    mcp_registry
        .add_ready_server(
            McpServerSpec::new(
                server_id.clone(),
                "Workspace GitHub",
                TransportChoice::InProcess,
                McpServerSource::Workspace,
            ),
            McpServerScope::Global,
            Arc::new(StaticMcpConnection {
                tools: vec![McpToolDescriptor {
                    name: "search".to_owned(),
                    description: Some("Search".to_owned()),
                    input_schema: json!({ "type": "object" }),
                    output_schema: None,
                    annotations: None,
                    meta: Default::default(),
                }],
            }),
        )
        .await
        .unwrap();
    let state = runtime_state_with_mcp_registry_for_workspace(
        workspace,
        mcp_registry,
        vec![server_id.clone()],
    )
    .await;
    let harness = state.harness().unwrap();
    harness
        .mcp_config()
        .unwrap()
        .registry
        .inject_tools_into(harness.tool_registry(), &server_id)
        .await
        .unwrap();
    assert!(harness.tool_registry().get("mcp__github__search").is_some());

    let payload = delete_mcp_server_with_runtime_state(
        DeleteMcpServerRequest {
            id: "github".to_owned(),
        },
        &state,
    )
    .await
    .unwrap();
    let servers = list_mcp_servers_with_runtime_state(&state).await.unwrap();

    assert_eq!(payload.status, "deleted");
    assert!(servers.servers.is_empty());
    assert!(harness.tool_registry().get("mcp__github__search").is_none());
}

#[tokio::test]
async fn list_mcp_servers_with_runtime_state_includes_origin_scope_and_tool_count() {
    let server_id = McpServerId("github".to_owned());
    let mcp_registry = McpRegistry::new();
    mcp_registry
        .add_ready_server(
            McpServerSpec::new(
                server_id.clone(),
                "Workspace GitHub",
                TransportChoice::InProcess,
                McpServerSource::Workspace,
            ),
            McpServerScope::Global,
            Arc::new(StaticMcpConnection {
                tools: vec![
                    McpToolDescriptor {
                        name: "search".to_owned(),
                        description: Some("Search".to_owned()),
                        input_schema: json!({ "type": "object" }),
                        output_schema: None,
                        annotations: None,
                        meta: Default::default(),
                    },
                    McpToolDescriptor {
                        name: "issue".to_owned(),
                        description: Some("Issue".to_owned()),
                        input_schema: json!({ "type": "object" }),
                        output_schema: None,
                        annotations: None,
                        meta: Default::default(),
                    },
                ],
            }),
        )
        .await
        .unwrap();
    let tool_registry = ToolRegistry::builder().build().unwrap();
    mcp_registry
        .inject_tools_into(&tool_registry, &server_id)
        .await
        .unwrap();
    let state = runtime_state_with_mcp_registry(mcp_registry, vec![server_id]).await;
    let payload = list_mcp_servers_with_runtime_state(&state).await.unwrap();
    let value = serde_json::to_value(payload).unwrap();

    assert_eq!(
        value,
        json!({
            "servers": [
                {
                    "displayName": "Workspace GitHub",
                    "enabled": true,
                    "exposedToolCount": 2,
                    "id": "github",
                    "manageable": false,
                    "origin": "workspace",
                    "scope": "global",
                    "status": "ready",
                    "transport": "inProcess"
                }
            ]
        })
    );
}

#[test]
fn mcp_diagnostic_event_summary_does_not_expose_raw_connection_error() {
    let diagnostic =
        mcp_diagnostic_record_from_event(Event::McpConnectionLost(McpConnectionLostEvent {
            session_id: None,
            server_id: McpServerId("github".to_owned()),
            server_source: McpServerSource::Workspace,
            reason: McpConnectionLostReason::Network(
                "Authorization: Bearer mcp-secret-token".to_owned(),
            ),
            attempts_so_far: 1,
            terminal: false,
            at: now(),
        }))
        .unwrap();

    assert_eq!(diagnostic.server_id, "github");
    assert_eq!(diagnostic.severity, McpDiagnosticSeverity::Warning);
    assert_eq!(
        diagnostic.summary,
        "MCP server connection lost; reconnecting."
    );
    assert!(!serde_json::to_string(&diagnostic)
        .unwrap()
        .contains("mcp-secret-token"));
}

#[tokio::test]
async fn mcp_diagnostic_store_retains_recent_records_and_filters_by_server() {
    let workspace = unique_workspace("mcp-diagnostics");
    std::fs::create_dir_all(&workspace).unwrap();
    let store = DesktopMcpDiagnosticStore::new_with_limit(workspace, 3);

    for index in 0..5 {
        store
            .append_record(&McpDiagnosticRecord {
                event_type: "connection_lost".to_owned(),
                id: format!("event-{index}"),
                server_id: if index == 4 { "fetch" } else { "github" }.to_owned(),
                severity: McpDiagnosticSeverity::Warning,
                summary: format!("diagnostic {index}"),
                timestamp: format!("2026-06-17T00:00:0{index}.000Z"),
            })
            .unwrap();
    }

    let all = list_mcp_diagnostics_with_store(None, &store).await.unwrap();
    let github = list_mcp_diagnostics_with_store(Some("github".to_owned()), &store)
        .await
        .unwrap();

    assert_eq!(
        all.events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        vec!["event-2", "event-3", "event-4"]
    );
    assert_eq!(
        github
            .events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        vec!["event-2", "event-3"]
    );
}

use super::*;

#[tokio::test]
async fn save_mcp_server_payload_rejects_invalid_config_fail_closed() {
    let store = RecordingMcpServerStore::default();
    let error = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            required: false,
            display_name: String::new(),
            id: "bad id".to_owned(),
            scope: "global".to_owned(),
            transport: SaveMcpServerTransportConfig::Stdio {
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
fn mcp_server_config_record_rejects_missing_enabled() {
    let error = serde_json::from_value::<McpServerConfigRecord>(json!({
        "displayName": "Workspace GitHub",
        "id": "github",
        "scope": "global",
        "transport": {
            "kind": "stdio",
            "command": "node",
            "args": ["server.js"]
        }
    }))
    .unwrap_err();

    assert!(error.to_string().contains("missing field `enabled`"));
}

#[test]
fn save_mcp_server_request_missing_required_defaults_to_optional() {
    let request = serde_json::from_value::<SaveMcpServerRequest>(json!({
        "enabled": true,
        "displayName": "Workspace GitHub",
        "id": "github",
        "scope": "global",
        "transport": {
            "kind": "stdio",
            "command": "node"
        }
    }))
    .expect("deserialize legacy save request");

    assert!(!request.required);
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
    assert_eq!(payload.presets[0].version, "0.0.78");
    assert!(!payload.presets[0].enabled);
    assert_eq!(payload.presets[1].id, BrowserMcpPresetId::ChromeDevtools);
    assert_eq!(payload.presets[1].server_id, "browser-chrome-devtools");
    assert_eq!(payload.presets[1].version, "1.5.0");
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
    assert_eq!(payload.preset.version, "0.0.78");
    assert!(!payload.preset.enabled);
    assert_eq!(payload.server.status, "disabled");
    assert!(!stored.enabled);
    assert!(!stored.required);
    assert_eq!(stored.id, "browser-playwright");
    assert!(matches!(
        stored.transport,
        McpServerTransportConfig::Stdio { ref command, ref args, ref env, ref inherit_env, .. }
            if command == "npx"
                && args == &vec!["-y".to_owned(), "@playwright/mcp@0.0.78".to_owned()]
                && env.is_empty()
                && inherit_env == &vec![
                    "PATH".to_owned(),
                    "HOME".to_owned(),
                    "USER".to_owned(),
                    "TMPDIR".to_owned()
                ]
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
    assert_eq!(payload.preset.version, "1.5.0");
    assert!(payload.preset.enabled);
    assert!(payload.server.enabled);
    assert_eq!(stored.id, "browser-chrome-devtools");
    assert!(stored.enabled);
    assert!(!stored.required);
    assert!(matches!(
        stored.transport,
        McpServerTransportConfig::Stdio { ref command, ref args, ref inherit_env, .. }
            if command == "npx"
                && args == &vec!["-y".to_owned(), "chrome-devtools-mcp@1.5.0".to_owned()]
                && inherit_env == &vec![
                    "PATH".to_owned(),
                    "HOME".to_owned(),
                    "USER".to_owned(),
                    "TMPDIR".to_owned()
                ]
    ));
}

#[tokio::test]
async fn save_mcp_server_payload_allows_secret_bearing_stdio_args() {
    let store = RecordingMcpServerStore::default();
    let payload = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            required: false,
            display_name: "Workspace GitHub".to_owned(),
            id: "github".to_owned(),
            scope: "global".to_owned(),
            transport: SaveMcpServerTransportConfig::Stdio {
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
    .unwrap();
    let stored = store.record.lock().unwrap().clone().unwrap();

    assert!(payload.server.enabled);
    assert!(matches!(
        stored.transport,
        McpServerTransportConfig::Stdio { ref args, .. }
            if args == &vec!["--token=mcp-secret-token".to_owned()]
    ));
}

#[tokio::test]
async fn save_mcp_server_payload_allows_raw_secret_like_stdio_args() {
    let store = RecordingMcpServerStore::default();
    let payload = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            required: false,
            display_name: "Workspace GitHub".to_owned(),
            id: "github".to_owned(),
            scope: "global".to_owned(),
            transport: SaveMcpServerTransportConfig::Stdio {
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
    .unwrap();
    let stored = store.record.lock().unwrap().clone().unwrap();

    assert!(payload.server.enabled);
    assert!(matches!(
        stored.transport,
        McpServerTransportConfig::Stdio { ref args, .. }
            if args == &vec!["ghp_abcdefghijklmnopqrstuvwxyz0123456789".to_owned()]
    ));
}

#[tokio::test]
async fn save_mcp_server_payload_rejects_in_process_workspace_config() {
    let store = RecordingMcpServerStore::default();
    let error = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            required: false,
            display_name: "Internal".to_owned(),
            id: "internal".to_owned(),
            scope: "global".to_owned(),
            transport: SaveMcpServerTransportConfig::InProcess,
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
            required: false,
            display_name: "Remote Context".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: SaveMcpServerTransportConfig::Http {
                url: "https://mcp.example.com/mcp".to_owned(),
                bearer_token_env_var: Some("MCP_BEARER_TOKEN".to_owned()),
                headers: vec![McpNameValueSaveRecord {
                    key: "X-Workspace".to_owned(),
                    value: Some("jyowo".to_owned()),
                    preserve_existing: false,
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
            required: false,
            display_name: "Remote Context".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: SaveMcpServerTransportConfig::Http {
                url: "https://mcp.example.com/mcp".to_owned(),
                bearer_token_env_var: Some("MCP_BEARER_TOKEN".to_owned()),
                headers: vec![McpNameValueSaveRecord {
                    key: "X-Workspace".to_owned(),
                    value: Some("jyowo".to_owned()),
                    preserve_existing: false,
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
    match payload.server.transport {
        McpServerConfigTransportPayload::Http { headers, .. } => {
            assert_eq!(headers.len(), 1);
            assert_eq!(headers[0].key, "X-Workspace");
            assert!(headers[0].has_value);
            assert_eq!(headers[0].value, None);
        }
        _ => panic!("expected http transport"),
    }
    assert!(serialized.contains("MCP_BEARER_TOKEN"));
    assert!(!serialized.contains("jyowo"));
    assert!(!serialized.contains("mcp-secret-token"));
}

#[tokio::test]
async fn save_mcp_server_with_store_preserves_existing_redacted_http_header_value() {
    let store = RecordingMcpServerStore::default();
    save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            required: false,
            display_name: "Remote Context".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: SaveMcpServerTransportConfig::Http {
                url: "https://mcp.example.com/mcp".to_owned(),
                bearer_token_env_var: None,
                headers: vec![McpNameValueSaveRecord {
                    key: "X-Workspace".to_owned(),
                    value: Some("jyowo".to_owned()),
                    preserve_existing: false,
                }],
                headers_from_env: Vec::new(),
            },
        },
        &store,
    )
    .await
    .unwrap();

    save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            required: false,
            display_name: "Remote Context".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: SaveMcpServerTransportConfig::Http {
                url: "https://mcp.example.com/mcp".to_owned(),
                bearer_token_env_var: None,
                headers: vec![McpNameValueSaveRecord {
                    key: "X-Workspace".to_owned(),
                    value: None,
                    preserve_existing: true,
                }],
                headers_from_env: Vec::new(),
            },
        },
        &store,
    )
    .await
    .unwrap();
    let stored = store.record.lock().unwrap().clone().unwrap();

    match stored.transport {
        McpServerTransportConfig::Http { headers, .. } => {
            assert_eq!(headers.len(), 1);
            assert_eq!(headers[0].key, "X-Workspace");
            assert_eq!(headers[0].value, "jyowo");
        }
        _ => panic!("expected http transport"),
    }
}

#[tokio::test]
async fn save_mcp_server_with_store_rejects_preserve_existing_without_existing_value() {
    let store = RecordingMcpServerStore::default();
    let error = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            required: false,
            display_name: "Remote Context".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: SaveMcpServerTransportConfig::Http {
                url: "https://mcp.example.com/mcp".to_owned(),
                bearer_token_env_var: None,
                headers: vec![McpNameValueSaveRecord {
                    key: "X-Workspace".to_owned(),
                    value: None,
                    preserve_existing: true,
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
            required: false,
            display_name: "Remote Context".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: SaveMcpServerTransportConfig::Http {
                url: "https://mcp.example.com/mcp".to_owned(),
                bearer_token_env_var: None,
                headers: vec![McpNameValueSaveRecord {
                    key: "Authorization".to_owned(),
                    value: Some("Bearer mcp-secret-token".to_owned()),
                    preserve_existing: false,
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
            required: false,
            display_name: "Remote Browser".to_owned(),
            id: "remote-browser".to_owned(),
            scope: "global".to_owned(),
            transport: SaveMcpServerTransportConfig::Http {
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

    let state_for_command = state.clone();
    let payload = run_with_mcp_transport_approval(&state, async move {
        save_mcp_server_with_runtime_state(
            SaveMcpServerRequest {
                enabled: true,
                required: false,
                display_name: "Workspace Stdio".to_owned(),
                id: "stdio".to_owned(),
                scope: "global".to_owned(),
                transport: SaveMcpServerTransportConfig::Stdio {
                    command: "/bin/sh".to_owned(),
                    args: vec!["-c".to_owned(), stdio_mcp_fixture_script()],
                    env: Vec::new(),
                    inherit_env: Vec::new(),
                    working_dir: None,
                },
            },
            &state_for_command,
        )
        .await
    })
    .await
    .unwrap();
    let settings_runtime = state.settings_runtime().unwrap();

    let connection_state = state
        .settings_runtime()
        .unwrap()
        .mcp_config()
        .unwrap()
        .registry
        .connection_state(&McpServerId("stdio".to_owned()))
        .await;
    assert_eq!(
        payload.server.status, "ready",
        "connection state: {:?}",
        connection_state
    );
    assert_eq!(payload.server.exposed_tool_count, 1);
    assert!(settings_runtime
        .tool_registry()
        .get("mcp__stdio__echo")
        .is_some());
}

#[tokio::test]
async fn save_mcp_server_with_runtime_state_runs_npx_without_explicit_inherited_env() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let _home_lock = HOME_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("mcp-save-npx-empty-env");
    let fake_bin = workspace.join("bin");
    std::fs::create_dir_all(&fake_bin).unwrap();
    write_test_executable(&fake_bin.join("npx"), &context7_npx_fixture_script());
    let canonical_home = workspace.canonicalize().unwrap();
    let path_guard = EnvVarGuard::set("PATH", fake_bin.as_os_str());
    let home_guard = EnvVarGuard::set("HOME", canonical_home.as_os_str());
    let state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;

    let state_for_command = state.clone();
    let payload = run_with_mcp_transport_approval(&state, async move {
        save_mcp_server_with_runtime_state(
            SaveMcpServerRequest {
                enabled: true,
                required: false,
                display_name: "Context7".to_owned(),
                id: "context7".to_owned(),
                scope: "global".to_owned(),
                transport: SaveMcpServerTransportConfig::Stdio {
                    command: "npx".to_owned(),
                    args: vec![
                        "-y".to_owned(),
                        "@upstash/context7-mcp".to_owned(),
                        "--api-key".to_owned(),
                        "ctx7sk-test-token".to_owned(),
                    ],
                    env: Vec::new(),
                    inherit_env: Vec::new(),
                    working_dir: None,
                },
            },
            &state_for_command,
        )
        .await
    })
    .await
    .unwrap();

    assert_eq!(payload.server.id, "context7");
    assert_eq!(payload.server.status, "ready");
    assert_eq!(payload.server.exposed_tool_count, 1);
    drop(home_guard);
    drop(path_guard);
}

#[tokio::test]
async fn save_mcp_server_with_runtime_state_accepts_workspace_relative_working_dir() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let _home_lock = HOME_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("mcp-save-relative-working-dir");
    let fake_bin = workspace.join("bin");
    std::fs::create_dir_all(&fake_bin).unwrap();
    write_test_executable(&fake_bin.join("npx"), &context7_npx_fixture_script());
    let canonical_home = workspace.canonicalize().unwrap();
    let path_guard = EnvVarGuard::set("PATH", fake_bin.as_os_str());
    let home_guard = EnvVarGuard::set("HOME", canonical_home.as_os_str());
    let state = runtime_state_with_mcp_registry_for_workspace(
        workspace.clone(),
        McpRegistry::new(),
        Vec::new(),
    )
    .await;

    let state_for_command = state.clone();
    let payload = run_with_mcp_transport_approval(&state, async move {
        save_mcp_server_with_runtime_state(
            SaveMcpServerRequest {
                enabled: true,
                required: false,
                display_name: "Context7".to_owned(),
                id: "context7".to_owned(),
                scope: "global".to_owned(),
                transport: SaveMcpServerTransportConfig::Stdio {
                    command: "npx".to_owned(),
                    args: vec![
                        "-y".to_owned(),
                        "@upstash/context7-mcp".to_owned(),
                        "--api-key".to_owned(),
                        "ctx7sk-test-token".to_owned(),
                    ],
                    env: Vec::new(),
                    inherit_env: Vec::new(),
                    working_dir: Some(".".to_owned()),
                },
            },
            &state_for_command,
        )
        .await
    })
    .await
    .unwrap();

    assert_eq!(payload.server.id, "context7");
    assert_eq!(payload.server.status, "ready");
    assert_eq!(payload.server.exposed_tool_count, 1);
    drop(home_guard);
    drop(path_guard);
}

#[tokio::test(flavor = "current_thread")]
async fn save_mcp_server_with_runtime_state_rejects_invalid_working_dir_without_persisting() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("mcp-save-invalid-working-dir");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_with_mcp_registry_for_workspace(
        workspace.clone(),
        McpRegistry::new(),
        Vec::new(),
    )
    .await;

    let error = save_mcp_server_with_runtime_state(
        SaveMcpServerRequest {
            enabled: true,
            required: false,
            display_name: "Context7".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: SaveMcpServerTransportConfig::Stdio {
                command: "npx".to_owned(),
                args: vec![
                    "-y".to_owned(),
                    "@upstash/context7-mcp".to_owned(),
                    "--api-key".to_owned(),
                    "ctx7sk-test-token".to_owned(),
                ],
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: Some("missing-dir".to_owned()),
            },
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(!workspace.join(".jyowo/runtime/mcp-servers.json").exists());
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
            required: false,
            display_name: "Workspace Stdio".to_owned(),
            id: "stdio".to_owned(),
            scope: "global".to_owned(),
            transport: SaveMcpServerTransportConfig::Stdio {
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
    let settings_runtime = state.settings_runtime().unwrap();

    assert_eq!(payload.server.status, "disabled");
    assert!(!payload.server.enabled);
    assert!(settings_runtime
        .mcp_config()
        .unwrap()
        .registry
        .server_spec(&McpServerId("stdio".to_owned()))
        .await
        .is_none());
    assert!(settings_runtime
        .tool_registry()
        .get("mcp__stdio__echo")
        .is_none());
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
            required: false,
            display_name: "Workspace Stdio".to_owned(),
            id: "stdio".to_owned(),
            scope: "global".to_owned(),
            transport: SaveMcpServerTransportConfig::Stdio {
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

    let state_for_command = state.clone();
    let payload = run_with_mcp_transport_approval(&state, async move {
        set_mcp_server_enabled_with_runtime_state(
            SetMcpServerEnabledRequest {
                id: "stdio".to_owned(),
                enabled: true,
            },
            &state_for_command,
        )
        .await
    })
    .await
    .unwrap();

    let connection_state = state
        .settings_runtime()
        .unwrap()
        .mcp_config()
        .unwrap()
        .registry
        .connection_state(&McpServerId("stdio".to_owned()))
        .await;
    assert_eq!(
        payload.server.status, "ready",
        "connection state: {:?}",
        connection_state
    );
    assert!(payload.server.enabled);
    assert!(state
        .settings_runtime()
        .unwrap()
        .tool_registry()
        .get("mcp__stdio__echo")
        .is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn set_mcp_server_enabled_rejects_missing_bearer_env_without_persisting_enabled() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let _env = EnvVarGuard::remove("MCP_ENABLE_TEST_BEARER");
    let workspace = unique_workspace("mcp-enable-missing-bearer");
    std::fs::create_dir_all(&workspace).unwrap();
    let mut state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;
    let store = Arc::new(RecordingMcpServerStore::default());
    *store.record.lock().unwrap() = Some(McpServerConfigRecord {
        enabled: false,
        required: false,
        display_name: "Remote Context".to_owned(),
        id: "context7".to_owned(),
        scope: "global".to_owned(),
        transport: McpServerTransportConfig::Http {
            url: "http://127.0.0.1:9/mcp".to_owned(),
            bearer_token_env_var: Some("MCP_ENABLE_TEST_BEARER".to_owned()),
            headers: Vec::new(),
            headers_from_env: Vec::new(),
        },
    });
    state.set_mcp_server_store_for_test(store.clone());

    let error = set_mcp_server_enabled_with_runtime_state(
        SetMcpServerEnabledRequest {
            id: "context7".to_owned(),
            enabled: true,
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert!(error
        .message
        .contains("MCP bearer token env var is unavailable: MCP_ENABLE_TEST_BEARER"));
    assert!(!store.record.lock().unwrap().as_ref().unwrap().enabled);
    assert!(state
        .settings_runtime()
        .unwrap()
        .mcp_config()
        .unwrap()
        .registry
        .connection_state(&McpServerId("context7".to_owned()))
        .await
        .is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn restart_mcp_server_removes_registers_and_injects_tools() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("mcp-restart");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;
    let state_for_command = state.clone();
    run_with_mcp_transport_approval(&state, async move {
        save_mcp_server_with_runtime_state(
            SaveMcpServerRequest {
                enabled: true,
                required: false,
                display_name: "Workspace Stdio".to_owned(),
                id: "stdio".to_owned(),
                scope: "global".to_owned(),
                transport: SaveMcpServerTransportConfig::Stdio {
                    command: "/bin/sh".to_owned(),
                    args: vec!["-c".to_owned(), stdio_mcp_fixture_script()],
                    env: Vec::new(),
                    inherit_env: Vec::new(),
                    working_dir: None,
                },
            },
            &state_for_command,
        )
        .await
    })
    .await
    .unwrap();

    let state_for_command = state.clone();
    let payload = run_with_mcp_transport_approval(&state, async move {
        restart_mcp_server_with_runtime_state(
            RestartMcpServerRequest {
                id: "stdio".to_owned(),
            },
            &state_for_command,
        )
        .await
    })
    .await
    .unwrap();

    let connection_state = state
        .settings_runtime()
        .unwrap()
        .mcp_config()
        .unwrap()
        .registry
        .connection_state(&McpServerId("stdio".to_owned()))
        .await;
    assert_eq!(
        payload.server.status, "ready",
        "connection state: {:?}",
        connection_state
    );
    assert!(state
        .settings_runtime()
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
            required: true,
            display_name: "Remote Context".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: SaveMcpServerTransportConfig::Http {
                url: "http://127.0.0.1:9/mcp".to_owned(),
                bearer_token_env_var: Some("MCP_TEST_BEARER".to_owned()),
                headers: vec![McpNameValueSaveRecord {
                    key: "X-Workspace".to_owned(),
                    value: Some("jyowo".to_owned()),
                    preserve_existing: false,
                }],
                headers_from_env: Vec::new(),
            },
        },
        &state,
    )
    .await
    .unwrap();
    let spec = state
        .settings_runtime()
        .unwrap()
        .mcp_config()
        .unwrap()
        .registry
        .server_spec(&McpServerId("context7".to_owned()))
        .await
        .unwrap();

    assert_eq!(payload.server.transport, "http");
    assert!(payload.server.required);
    assert!(spec.required);
    assert_eq!(spec.source, McpServerSource::User);
    assert_eq!(spec.trust, TrustLevel::UserControlled);
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
                    title: None,
                    description: Some("Search".to_owned()),
                    icons: None,
                    input_schema: json!({ "type": "object" }),
                    execution: None,
                    output_schema: None,
                    annotations: None,
                    meta: Default::default(),
                }],
            }),
        )
        .await
        .unwrap();
    let mut state = runtime_state_with_mcp_registry_for_workspace(
        workspace,
        mcp_registry,
        vec![server_id.clone()],
    )
    .await;
    let store = Arc::new(RecordingMcpServerStore::default());
    *store.record.lock().unwrap() = Some(McpServerConfigRecord {
        enabled: true,
        required: false,
        display_name: "Workspace GitHub".to_owned(),
        id: "github".to_owned(),
        scope: "global".to_owned(),
        transport: McpServerTransportConfig::Stdio {
            command: "node".to_owned(),
            args: Vec::new(),
            env: Vec::new(),
            inherit_env: Vec::new(),
            working_dir: None,
        },
    });
    state.set_mcp_server_store_for_test(store);
    let settings_runtime = state.settings_runtime().unwrap();
    settings_runtime
        .mcp_config()
        .unwrap()
        .registry
        .inject_tools_into(settings_runtime.tool_registry(), &server_id)
        .await
        .unwrap();
    assert!(settings_runtime
        .tool_registry()
        .get("mcp__github__search")
        .is_some());

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
    assert!(settings_runtime
        .tool_registry()
        .get("mcp__github__search")
        .is_none());
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
                        title: None,
                        description: Some("Search".to_owned()),
                        icons: None,
                        input_schema: json!({ "type": "object" }),
                        execution: None,
                        output_schema: None,
                        annotations: None,
                        meta: Default::default(),
                    },
                    McpToolDescriptor {
                        name: "issue".to_owned(),
                        title: None,
                        description: Some("Issue".to_owned()),
                        icons: None,
                        input_schema: json!({ "type": "object" }),
                        execution: None,
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
            "configLayer": "global",
            "servers": [
                {
                    "configLayer": "global",
                    "displayName": "Workspace GitHub",
                    "effective": true,
                    "enabled": true,
                    "exposedToolCount": 2,
                    "id": "github",
                    "manageable": false,
                    "origin": "workspace",
                    "overridesGlobal": false,
                    "required": false,
                    "scope": "global",
                    "status": "ready",
                    "statusSource": "settings",
                    "transport": "inProcess"
                }
            ]
        })
    );
}

#[tokio::test]
async fn list_mcp_servers_with_runtime_state_projects_safe_failed_connection_error() {
    let server_id = McpServerId("playwright".to_owned());
    let mcp_registry = McpRegistry::new();
    mcp_registry
        .add_failed_server(
            McpServerSpec::new(
                server_id.clone(),
                "Playwright Browser",
                TransportChoice::InProcess,
                McpServerSource::Workspace,
            ),
            McpServerScope::Global,
            "spawn npx failed at /Users/alice/.npm with token=mcp-secret-token: No such file or directory (os error 2)".to_owned(),
        )
        .await
        .unwrap();
    let state = runtime_state_with_mcp_registry(mcp_registry, vec![server_id]).await;

    let payload = list_mcp_servers_with_runtime_state(&state).await.unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();

    assert_eq!(payload.servers[0].status, "failed");
    assert_eq!(
        payload.servers[0].last_error.as_deref(),
        Some("MCP server command was not found.")
    );
    assert!(!serialized.contains("mcp-secret-token"));
    assert!(!serialized.contains("/Users/alice"));
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
                plane: McpDiagnosticPlane::Settings,
                task_id: None,
                session_id: None,
                run_id: None,
                run_segment_id: None,
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

fn write_test_executable(path: &std::path::Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }
}

fn context7_npx_fixture_script() -> String {
    format!(
        r#"
if [ "$#" -ne 4 ] || [ "$1" != "-y" ] || [ "$2" != "@upstash/context7-mcp" ] || [ "$3" != "--api-key" ] || [ "$4" != "ctx7sk-test-token" ]; then
  printf '%s\n' "unexpected npx args: $*" >&2
  exit 64
fi
{}
"#,
        stdio_mcp_fixture_script()
    )
}

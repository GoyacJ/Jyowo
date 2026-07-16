#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use harness_contracts::{
    now, AgentCapabilities, BlobId, ClientFrame, ClientRequest, CorrelationId, Event, EventId,
    EventSource, EventSourceKind, HandshakeResponse, McpConnectionLostEvent,
    McpConnectionLostReason, McpServerId, McpServerSource, ProtocolError, ProtocolErrorCode, RunId,
    RunSegmentId, ServerFrame, ServerMessage, SessionId, TaskEventEnvelope, TaskEventPage, TaskId,
    TaskProjection, TaskState, TenantId, PROTOCOL_VERSION,
};
use jyowo_desktop_shell::commands::ModelUsageHistorySource;
use jyowo_desktop_shell::commands::{
    clear_mcp_diagnostics_with_runtime_state, list_mcp_diagnostics_with_runtime_state_and_daemon,
    ClearMcpDiagnosticsRequest, DesktopRuntimeState, ListMcpDiagnosticsRequest, McpDiagnosticPlane,
};
use jyowo_desktop_shell::daemon_client::{DaemonClient, DaemonClientConfig, DaemonClientError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

async fn read_frame(stream: &mut UnixStream) -> ClientFrame {
    let mut header = [0_u8; 4];
    stream.read_exact(&mut header).await.unwrap();
    let mut body = vec![0; u32::from_be_bytes(header) as usize];
    stream.read_exact(&mut body).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn write_frame(stream: &mut UnixStream, frame: &ServerFrame) {
    let body = serde_json::to_vec(frame).unwrap();
    stream
        .write_all(&(body.len() as u32).to_be_bytes())
        .await
        .unwrap();
    stream.write_all(&body).await.unwrap();
}

fn write_private_token(path: &Path, token: &str) {
    std::fs::write(path, token).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

async fn serve_connection(listener: &UnixListener, expected_requests: usize) {
    let (mut stream, _) = listener.accept().await.unwrap();
    let handshake = read_frame(&mut stream).await;
    assert!(matches!(handshake.request, ClientRequest::Handshake(_)));
    write_frame(
        &mut stream,
        &ServerFrame {
            request_id: Some(handshake.request_id),
            protocol_version: PROTOCOL_VERSION,
            message: ServerMessage::Handshake(HandshakeResponse {
                daemon_version: "0.1.0".into(),
                user_instance_id: "user-a".into(),
                latest_global_offset: 0,
                agent_capabilities: AgentCapabilities::daemon_native(),
            }),
        },
    )
    .await;
    for _ in 0..expected_requests {
        let request = read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &ServerFrame {
                request_id: Some(request.request_id),
                protocol_version: PROTOCOL_VERSION,
                message: ServerMessage::TaskList { tasks: Vec::new() },
            },
        )
        .await;
    }
}

fn client(socket: &Path, token: &Path) -> DaemonClient {
    DaemonClient::new(DaemonClientConfig {
        endpoint: socket.into(),
        token_path: token.into(),
        user_instance_id: "user-a".into(),
        client_version: "0.1.0".into(),
    })
}

#[tokio::test]
async fn desktop_diagnostic_helper_reads_task_journal_through_daemon_client() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("daemon.sock");
    let token = root.path().join("connection.token");
    write_private_token(&token, "token-a");
    let listener = UnixListener::bind(&socket).unwrap();
    let task_id = TaskId::new();
    let event_id = EventId::new();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let run_segment_id = RunSegmentId::new();
    let at = now();
    let envelope = TaskEventEnvelope {
        global_offset: 12,
        task_id,
        stream_sequence: 2,
        event_id,
        event_type: "engine.mcp_connection_lost".to_owned(),
        schema_version: 1,
        recorded_at: at,
        source: EventSource {
            kind: EventSourceKind::Engine,
            actor_id: None,
            client_id: None,
        },
        payload: serde_json::json!({
            "tenantId": TenantId::SINGLE,
            "sessionId": session_id,
            "journalOffset": 11,
            "runId": run_id,
            "runSegmentId": run_segment_id,
            "correlationId": CorrelationId::new(),
            "causationId": null,
            "event": Event::McpConnectionLost(McpConnectionLostEvent {
                session_id: Some(session_id),
                server_id: McpServerId("github".to_owned()),
                server_source: McpServerSource::Project,
                reason: McpConnectionLostReason::Other("closed".to_owned()),
                attempts_so_far: 1,
                terminal: false,
                at,
            }),
        }),
    };
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let handshake = read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &ServerFrame {
                request_id: Some(handshake.request_id),
                protocol_version: PROTOCOL_VERSION,
                message: ServerMessage::Handshake(HandshakeResponse {
                    daemon_version: "0.1.0".into(),
                    user_instance_id: "user-a".into(),
                    latest_global_offset: 12,
                    agent_capabilities: AgentCapabilities::daemon_native(),
                }),
            },
        )
        .await;
        let list = read_frame(&mut stream).await;
        assert!(matches!(list.request, ClientRequest::ListTasks));
        write_frame(
            &mut stream,
            &ServerFrame {
                request_id: Some(list.request_id),
                protocol_version: PROTOCOL_VERSION,
                message: ServerMessage::TaskList {
                    tasks: vec![TaskProjection {
                        task_id,
                        title: "Task".to_owned(),
                        state: TaskState::Completed,
                        pinned: false,
                        archived: false,
                        removed: false,
                        stream_version: 2,
                        last_global_offset: 12,
                        current_run: None,
                        pending_permission: None,
                        queue: Vec::new(),
                        workspace: None,
                        actor_id: None,
                        context_cursor: 0,
                        parent: None,
                        subagents: Vec::new(),
                    }],
                },
            },
        )
        .await;
        let load = read_frame(&mut stream).await;
        assert!(matches!(
            load.request,
            ClientRequest::LoadTaskEvents { task_id: loaded, .. } if loaded == task_id
        ));
        write_frame(
            &mut stream,
            &ServerFrame {
                request_id: Some(load.request_id),
                protocol_version: PROTOCOL_VERSION,
                message: ServerMessage::TaskEventPage(TaskEventPage {
                    task_id,
                    events: vec![envelope],
                    next_before_offset: None,
                }),
            },
        )
        .await;
    });

    let daemon_client = client(&socket, &token);
    let workspace = tempfile::tempdir().unwrap();
    let state = DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf())
        .expect("desktop runtime state");
    let response = list_mcp_diagnostics_with_runtime_state_and_daemon(
        ListMcpDiagnosticsRequest { server_id: None },
        &state,
        Some(&daemon_client),
    )
    .await
    .expect("list diagnostics");

    assert_eq!(response.events.len(), 1);
    assert_eq!(response.events[0].id, event_id.to_string());
    assert_eq!(response.events[0].plane, McpDiagnosticPlane::Task);
    server.await.unwrap();
}

#[tokio::test]
async fn cleared_task_diagnostics_stay_hidden_after_runtime_recreation() {
    let workspace = tempfile::tempdir().unwrap();
    let state = DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf())
        .expect("desktop runtime state");
    let old_at = now() - chrono::Duration::seconds(1);
    clear_mcp_diagnostics_with_runtime_state(
        ClearMcpDiagnosticsRequest { server_id: None },
        &state,
    )
    .await
    .expect("clear diagnostics without daemon");
    drop(state);
    let new_at = now();

    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("daemon.sock");
    let token = root.path().join("connection.token");
    write_private_token(&token, "token-a");
    let listener = UnixListener::bind(&socket).unwrap();
    let task_id = TaskId::new();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let run_segment_id = RunSegmentId::new();
    let old_id = EventId::new();
    let new_id = EventId::new();
    let envelope = |global_offset, event_id, at| TaskEventEnvelope {
        global_offset,
        task_id,
        stream_sequence: global_offset,
        event_id,
        event_type: "engine.mcp_connection_lost".to_owned(),
        schema_version: 1,
        recorded_at: at,
        source: EventSource {
            kind: EventSourceKind::Engine,
            actor_id: None,
            client_id: None,
        },
        payload: serde_json::json!({
            "tenantId": TenantId::SINGLE,
            "sessionId": session_id,
            "journalOffset": global_offset - 1,
            "runId": run_id,
            "runSegmentId": run_segment_id,
            "correlationId": CorrelationId::new(),
            "causationId": null,
            "event": Event::McpConnectionLost(McpConnectionLostEvent {
                session_id: Some(session_id),
                server_id: McpServerId("github".to_owned()),
                server_source: McpServerSource::Project,
                reason: McpConnectionLostReason::Other("closed".to_owned()),
                attempts_so_far: 1,
                terminal: false,
                at,
            }),
        }),
    };
    let old_envelope = envelope(12, old_id, old_at);
    let new_envelope = envelope(13, new_id, new_at);
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let handshake = read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &ServerFrame {
                request_id: Some(handshake.request_id),
                protocol_version: PROTOCOL_VERSION,
                message: ServerMessage::Handshake(HandshakeResponse {
                    daemon_version: "0.1.0".into(),
                    user_instance_id: "user-a".into(),
                    latest_global_offset: 13,
                    agent_capabilities: AgentCapabilities::daemon_native(),
                }),
            },
        )
        .await;
        let list = read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &ServerFrame {
                request_id: Some(list.request_id),
                protocol_version: PROTOCOL_VERSION,
                message: ServerMessage::TaskList {
                    tasks: vec![TaskProjection {
                        task_id,
                        title: "Task".to_owned(),
                        state: TaskState::Completed,
                        pinned: false,
                        archived: false,
                        removed: false,
                        stream_version: 3,
                        last_global_offset: 13,
                        current_run: None,
                        pending_permission: None,
                        queue: Vec::new(),
                        workspace: None,
                        actor_id: None,
                        context_cursor: 0,
                        parent: None,
                        subagents: Vec::new(),
                    }],
                },
            },
        )
        .await;
        let load = read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &ServerFrame {
                request_id: Some(load.request_id),
                protocol_version: PROTOCOL_VERSION,
                message: ServerMessage::TaskEventPage(TaskEventPage {
                    task_id,
                    events: vec![new_envelope, old_envelope],
                    next_before_offset: None,
                }),
            },
        )
        .await;
    });

    let daemon_client = client(&socket, &token);
    let recreated = DesktopRuntimeState::with_workspace_for_test(workspace.path().to_path_buf())
        .expect("recreated desktop runtime state");
    let response = list_mcp_diagnostics_with_runtime_state_and_daemon(
        ListMcpDiagnosticsRequest { server_id: None },
        &recreated,
        Some(&daemon_client),
    )
    .await
    .expect("list diagnostics after clear");

    assert_eq!(response.events.len(), 1);
    assert_eq!(response.events[0].id, new_id.to_string());
    assert_ne!(response.events[0].id, old_id.to_string());
    server.await.unwrap();
}

#[tokio::test]
async fn bridge_forwards_validated_frames_over_one_persistent_connection() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("daemon.sock");
    let token = root.path().join("connection.token");
    write_private_token(&token, "token-a");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move { serve_connection(&listener, 2).await });
    let client = client(&socket, &token);

    for _ in 0..2 {
        let response = client.request(ClientRequest::ListTasks).await.unwrap();
        assert!(matches!(response.message, ServerMessage::TaskList { .. }));
    }
    assert_eq!(
        client.agent_capabilities(),
        Some(AgentCapabilities::daemon_native())
    );
    server.await.unwrap();
}

#[tokio::test]
async fn bridge_reconnects_after_daemon_restart_and_blob_reads_accept_only_ids() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("daemon.sock");
    let token = root.path().join("connection.token");
    write_private_token(&token, "token-a");
    let first_listener = UnixListener::bind(&socket).unwrap();
    let first = tokio::spawn(async move { serve_connection(&first_listener, 1).await });
    let client = Arc::new(client(&socket, &token));
    client.request(ClientRequest::ListTasks).await.unwrap();
    first.await.unwrap();
    std::fs::remove_file(&socket).unwrap();

    let second_listener = UnixListener::bind(&socket).unwrap();
    let second = tokio::spawn(async move { serve_connection(&second_listener, 1).await });
    let response = client.read_blob(BlobId::new()).await.unwrap();
    assert!(matches!(response.message, ServerMessage::TaskList { .. }));
    second.await.unwrap();
}

#[tokio::test]
async fn requestless_protocol_error_completes_the_current_non_streaming_request() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("daemon.sock");
    let token = root.path().join("connection.token");
    write_private_token(&token, "token-a");
    let listener = UnixListener::bind(&socket).unwrap();
    let (release_server, hold_server) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let handshake = read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &ServerFrame {
                request_id: Some(handshake.request_id),
                protocol_version: PROTOCOL_VERSION,
                message: ServerMessage::Handshake(HandshakeResponse {
                    daemon_version: "0.1.0".into(),
                    user_instance_id: "user-a".into(),
                    latest_global_offset: 0,
                    agent_capabilities: AgentCapabilities::daemon_native(),
                }),
            },
        )
        .await;
        let request = read_frame(&mut stream).await;
        assert_eq!(request.request_id.len(), 129);
        write_frame(
            &mut stream,
            &ServerFrame {
                request_id: None,
                protocol_version: PROTOCOL_VERSION,
                message: ServerMessage::Error(ProtocolError {
                    code: ProtocolErrorCode::InvalidFrame,
                    message: "invalid request ID".into(),
                }),
            },
        )
        .await;
        let _ = hold_server.await;
    });
    let client = client(&socket, &token);
    let response = tokio::time::timeout(
        std::time::Duration::from_millis(250),
        client.send_frame(ClientFrame {
            request_id: "r".repeat(129),
            protocol_version: PROTOCOL_VERSION,
            request: ClientRequest::ListTasks,
        }),
    )
    .await;
    let _ = release_server.send(());
    server.await.unwrap();

    let response = response
        .expect("requestless protocol error must not leave the bridge waiting")
        .unwrap();
    assert!(response.request_id.is_none());
    assert!(matches!(
        response.message,
        ServerMessage::Error(ProtocolError {
            code: ProtocolErrorCode::InvalidFrame,
            ..
        })
    ));
}

#[tokio::test(start_paused = true)]
async fn model_usage_history_request_has_a_finite_timeout() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("daemon.sock");
    let token = root.path().join("connection.token");
    write_private_token(&token, "token-a");
    let listener = UnixListener::bind(&socket).unwrap();
    let (release_server, hold_server) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let handshake = read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &ServerFrame {
                request_id: Some(handshake.request_id),
                protocol_version: PROTOCOL_VERSION,
                message: ServerMessage::Handshake(HandshakeResponse {
                    daemon_version: "0.1.0".into(),
                    user_instance_id: "user-a".into(),
                    latest_global_offset: 0,
                    agent_capabilities: AgentCapabilities::daemon_native(),
                }),
            },
        )
        .await;
        let request = read_frame(&mut stream).await;
        assert!(matches!(request.request, ClientRequest::LoadEvents { .. }));
        let _ = hold_server.await;
    });
    let client = client(&socket, &token);

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(6),
        ModelUsageHistorySource::load_events(&client, 0, 1),
    )
    .await;

    let _ = release_server.send(());
    server.await.unwrap();
    let error = result
        .expect("usage history source must enforce its own timeout")
        .unwrap_err();
    assert!(error.message.contains("timed out"));
}

#[tokio::test]
async fn load_events_preserves_daemon_protocol_errors() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("daemon.sock");
    let token = root.path().join("connection.token");
    write_private_token(&token, "token-a");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let handshake = read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &ServerFrame {
                request_id: Some(handshake.request_id),
                protocol_version: PROTOCOL_VERSION,
                message: ServerMessage::Handshake(HandshakeResponse {
                    daemon_version: "0.1.0".into(),
                    user_instance_id: "user-a".into(),
                    latest_global_offset: 0,
                    agent_capabilities: AgentCapabilities::daemon_native(),
                }),
            },
        )
        .await;
        let request = read_frame(&mut stream).await;
        assert!(matches!(request.request, ClientRequest::LoadEvents { .. }));
        write_frame(
            &mut stream,
            &ServerFrame {
                request_id: Some(request.request_id),
                protocol_version: PROTOCOL_VERSION,
                message: ServerMessage::Error(ProtocolError {
                    code: ProtocolErrorCode::FrameTooLarge,
                    message: "history event exceeds frame budget".into(),
                }),
            },
        )
        .await;
    });
    let client = client(&socket, &token);

    let error = client.load_events(0, 1).await.unwrap_err();
    server.await.unwrap();

    match error {
        DaemonClientError::ProtocolError { code, message } => {
            assert_eq!(code, ProtocolErrorCode::FrameTooLarge);
            assert_eq!(message, "history event exceeds frame budget");
        }
        other => panic!("unexpected client error: {other:?}"),
    }
}

#[tokio::test]
async fn bridge_rejects_symlinked_or_overexposed_connection_tokens() {
    let root = tempfile::tempdir().unwrap();
    let token_target = root.path().join("token-target");
    let token_link = root.path().join("connection.token");
    write_private_token(&token_target, "token-a");
    std::os::unix::fs::symlink(&token_target, &token_link).unwrap();

    let error = client(&root.path().join("missing.sock"), &token_link)
        .request(ClientRequest::ListTasks)
        .await
        .unwrap_err();
    assert!(matches!(error, DaemonClientError::InvalidToken));

    std::fs::remove_file(&token_link).unwrap();
    std::fs::write(&token_link, "token-a").unwrap();
    std::fs::set_permissions(&token_link, std::fs::Permissions::from_mode(0o640)).unwrap();
    let error = client(&root.path().join("missing.sock"), &token_link)
        .request(ClientRequest::ListTasks)
        .await
        .unwrap_err();
    assert!(matches!(error, DaemonClientError::InvalidToken));
}

#[tokio::test]
async fn persistent_request_connection_rejects_streaming_subscriptions() {
    let root = tempfile::tempdir().unwrap();
    let token = root.path().join("connection.token");
    write_private_token(&token, "token-a");
    let error = client(&root.path().join("missing.sock"), &token)
        .request(ClientRequest::SubscribeEvents { after_offset: 0 })
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        DaemonClientError::StreamingRequestNotAllowed
    ));
}

#[test]
fn bridge_source_contains_no_task_authority_or_blob_path_api() {
    let source = std::fs::read_to_string("src/daemon_client.rs").unwrap();
    for forbidden in [
        "TaskStore",
        "Harness",
        "RunCoordinator",
        "blob_path",
        "blobPath",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden bridge authority: {forbidden}"
        );
    }
}

#[test]
fn tauri_exposes_only_thin_daemon_bridge_commands() {
    let source = std::fs::read_to_string("src/commands/daemon.rs").unwrap();
    for command in [
        "daemon_connect",
        "daemon_request",
        "daemon_subscribe",
        "daemon_unsubscribe",
        "daemon_read_blob",
        "daemon_stage_blob_from_path",
        "daemon_list_reference_candidates",
    ] {
        assert!(source.contains(command), "missing command {command}");
    }
    for forbidden in [
        "TaskStore",
        "Harness",
        "RunCoordinator",
        "blob_path",
        "blobPath",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden command authority: {forbidden}"
        );
    }
    assert!(
        source.contains("window.emit(&event_name"),
        "subscription events must be scoped to an invoking webview channel"
    );
    assert!(
        source.contains("format!(\"{DAEMON_EVENT_NAME}/{subscription_id}\")"),
        "concurrent subscriptions must use separate event channels"
    );
    assert!(
        source.contains(".remove_finished_subscription(&cleanup_id, subscription_token)"),
        "finished subscriptions must remove only their own bridge handles"
    );
    assert!(
        source.contains("window.on_window_event")
            && source.contains("tauri::WindowEvent::Destroyed"),
        "destroying an invoking window must actively reclaim its subscriptions"
    );
    assert!(
        source.find("contains_key(&subscription_id)")
            < source.find("register_window(owner_window_instance)"),
        "a rejected duplicate subscription must not register a cleanup callback that can abort the existing subscription"
    );
    assert!(
        source.contains("window.resources_table()")
            && source.contains("WindowInstanceId")
            && source.contains("window_registration.install_handler"),
        "each webview window instance must install at most one daemon subscription lifecycle handler"
    );
    assert!(
        source.contains("owner_window_generation")
            && source.contains("remove_window_subscriptions(window_generation)"),
        "destroy cleanup must be scoped to the destroyed window generation"
    );
    assert!(
        source.contains("subscription.token == token"),
        "natural completion must not remove a replacement subscription with a reused id"
    );
}

#[test]
fn model_usage_commands_release_the_managed_runtime_read_guard_before_history_awaits() {
    let source = std::fs::read_to_string("src/commands/mod.rs").unwrap();

    assert!(source.contains(
        "let model_usage_rollup_store = {\n        let runtime_state = runtime_handle.read().await;\n        Arc::clone(&runtime_state.model_usage_rollup_store)\n    };"
    ));
    assert!(source.contains("let runtime_state = runtime_handle.read().await.clone();"));
    assert!(!source.contains(
        "let runtime_state = runtime_handle.read().await;\n    model_settings::get_model_usage_summary_with_history_source(&*runtime_state, &client).await"
    ));
}

#[test]
fn active_tauri_runtime_manages_and_registers_the_daemon_bridge() {
    let source = std::fs::read_to_string("src/lib.rs").unwrap();
    assert!(
        source.contains("manage(commands::DaemonBridgeState::default())"),
        "daemon bridge state is not managed"
    );
    for command in [
        "commands::daemon_connect",
        "commands::daemon_request",
        "commands::daemon_subscribe",
        "commands::daemon_unsubscribe",
        "commands::daemon_read_blob",
        "commands::daemon_stage_blob_from_path",
        "commands::daemon_list_reference_candidates",
    ] {
        assert!(
            source.contains(command),
            "active handler is missing {command}"
        );
    }
    assert!(
        !source.contains("ensure_agent_supervisor_sidecar_for_state"),
        "legacy supervisor is still launched by the active runtime"
    );
}

#[test]
fn active_tauri_handler_keeps_settings_queries_outside_the_legacy_task_boundary() {
    let source = std::fs::read_to_string("src/lib.rs").unwrap();
    for command in [
        "commands::cancel_run",
        "commands::create_conversation",
        "commands::create_default_conversation",
        "commands::create_project_conversation",
        "commands::delete_conversation",
        "commands::get_conversation",
        "commands::harness_healthcheck",
        "commands::list_conversations",
        "commands::start_run",
    ] {
        assert!(
            !source.contains(command),
            "active handler still registers legacy runtime command {command}"
        );
    }
    for command in [
        "commands::get_runtime_execution_status",
        "commands::list_runtime_tools",
        "commands::reset_runtime_tool_config",
        "commands::reset_runtime_tools",
        "commands::set_runtime_tool_enabled",
        "commands::update_runtime_tool_config",
    ] {
        assert!(
            source.contains(command),
            "active handler is missing settings query {command}"
        );
    }
}

#[test]
fn legacy_supervisor_and_loopback_control_plane_are_absent() {
    for path in [
        "src/agent_supervisor.rs",
        "src/bin/jyowo-agent-supervisor.rs",
    ] {
        assert!(
            !std::path::Path::new(path).exists(),
            "legacy runtime remains: {path}"
        );
    }

    for path in [
        "src/lib.rs",
        "src/commands/mod.rs",
        "src/commands/runtime.rs",
        "src/commands/providers.rs",
        "src/commands/background_agents.rs",
        "src/commands/conversations.rs",
    ] {
        let Ok(source) = std::fs::read_to_string(path) else {
            assert!(
                !std::path::Path::new(path).exists(),
                "failed to read active runtime source: {path}"
            );
            continue;
        };
        for forbidden in [
            "agent_supervisor",
            "AgentSupervisor",
            "jyowo-agent-supervisor",
            "TcpListener",
            "TcpStream",
            "control_addr",
            "page_conversation_timeline",
            "subscribe_conversation_events",
            "unsubscribe_conversation_events",
        ] {
            assert!(
                !source.contains(forbidden),
                "{path} still contains legacy control-plane token {forbidden}"
            );
        }
    }

    for path in ["src/lib.rs", "src/commands/mod.rs"] {
        let source = std::fs::read_to_string(path).unwrap();
        assert!(
            !source.contains("page_conversation_worktree"),
            "{path} still exposes the legacy conversation worktree command"
        );
    }

    let source = std::fs::read_to_string("src/lib.rs").unwrap();
    assert!(
        !source.contains("commands::resolve_permission"),
        "src/lib.rs still registers the legacy conversation permission command"
    );
    let source = std::fs::read_to_string("src/commands/mod.rs").unwrap();
    assert!(
        !source.contains("pub async fn resolve_permission("),
        "src/commands/mod.rs still exposes the legacy conversation permission command"
    );
}

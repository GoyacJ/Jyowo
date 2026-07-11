#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use harness_contracts::{
    BlobId, ClientFrame, ClientRequest, HandshakeResponse, ProtocolError, ProtocolErrorCode,
    ServerFrame, ServerMessage, PROTOCOL_VERSION,
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

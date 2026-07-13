#![cfg(feature = "websocket")]

#[cfg(feature = "oauth")]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use futures::{SinkExt, StreamExt};
use harness_contracts::{McpServerId, McpServerSource, RequestId};
use harness_mcp::{
    DirectElicitationHandler, McpChange, McpClient, McpClientAuth, McpConnectContext, McpError,
    McpServerSpec, McpToolCallEvent, TransportChoice, WebsocketTransport,
    MCP_ELICITATION_REQUIRED_CODE,
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};
#[cfg(feature = "oauth")]
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        handshake::server::{ErrorResponse, Request, Response},
        http::StatusCode,
    },
};
#[cfg(feature = "oauth")]
use wiremock::{
    matchers::{body_partial_json, method},
    Mock, MockServer, ResponseTemplate,
};

mod support;

#[tokio::test]
async fn websocket_upgrade_respects_handshake_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let _stream = stream;
        std::future::pending::<()>().await;
    });
    let mut spec = McpServerSpec::new(
        McpServerId("ws-timeout".into()),
        "websocket timeout fixture",
        TransportChoice::WebSocket {
            url: format!("ws://{addr}"),
            headers: BTreeMap::default(),
        },
        McpServerSource::Workspace,
    );
    spec.timeouts.handshake = Duration::from_millis(50);

    let result = tokio::time::timeout(
        Duration::from_millis(250),
        McpClient::new(Arc::new(WebsocketTransport::new()))
            .connect_with_context(spec, support::authorized_connect_context()),
    )
    .await;
    server.abort();
    let result = result.expect("transport handshake timeout must bound websocket upgrade");
    let error = match result {
        Ok(_) => panic!("stalled websocket upgrade unexpectedly connected"),
        Err(error) => error,
    };
    assert!(matches!(error, McpError::Connection(message) if message.contains("timed out")));
}

#[tokio::test]
async fn websocket_transport_rejects_transport_owned_headers() {
    for owned in [
        "authorization",
        "connection",
        "host",
        "proxy-authorization",
        "sec-websocket-key",
        "sec-websocket-protocol",
        "sec-websocket-version",
        "upgrade",
    ] {
        let mut headers = BTreeMap::new();
        headers.insert(owned.to_owned(), "injected".to_owned());
        let spec = McpServerSpec::new(
            McpServerId(format!("ws-owned-{owned}")),
            "websocket owned header fixture",
            TransportChoice::WebSocket {
                url: "ws://127.0.0.1:9".into(),
                headers,
            },
            McpServerSource::Workspace,
        );

        let error = match McpClient::new(Arc::new(WebsocketTransport::new()))
            .connect_with_context(spec, support::authorized_connect_context())
            .await
        {
            Ok(_) => panic!("transport-owned header {owned} must be rejected"),
            Err(error) => error,
        };
        assert!(
            matches!(error, McpError::Protocol(ref message) if message.contains(owned)),
            "unexpected error for {owned}: {error}"
        );
    }
}

#[tokio::test]
async fn websocket_transport_handles_requests_and_list_changed_notifications() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket accept");
        while let Some(message) = socket.next().await {
            let text = message.expect("message").into_text().expect("text");
            let value: Value = serde_json::from_str(&text).expect("json");
            match value.get("method").and_then(Value::as_str) {
                Some("initialize") => {
                    socket
                        .send(Message::text(
                            json!({
                                "jsonrpc": "2.0",
                                "id": value["id"].clone(),
                                "result": {
                                    "protocolVersion": "2025-11-25",
                                    "capabilities": { "tools": {} },
                                    "serverInfo": { "name": "fixture", "version": "0.1.0" }
                                }
                            })
                            .to_string(),
                        ))
                        .await
                        .expect("send initialize");
                }
                Some("tools/list") => {
                    socket
                        .send(Message::text(json!({
                            "jsonrpc": "2.0",
                            "id": value["id"].clone(),
                            "result": {
                                "tools": [
                                    { "name": "lookup", "description": "Lookup", "inputSchema": { "type": "object" } }
                                ]
                            }
                        }).to_string()))
                        .await
                        .expect("send tools list");
                    socket
                        .send(Message::text(
                            json!({
                                "jsonrpc": "2.0",
                                "method": "notifications/tools/list_changed"
                            })
                            .to_string(),
                        ))
                        .await
                        .expect("send list changed");
                }
                Some("tools/call") => {
                    socket
                        .send(Message::text(
                            json!({
                                "jsonrpc": "2.0",
                                "id": value["id"].clone(),
                                "result": {
                                    "content": [{ "type": "text", "text": "looked up" }],
                                    "isError": false
                                }
                            })
                            .to_string(),
                        ))
                        .await
                        .expect("send tool result");
                }
                _ => {}
            }
        }
    });

    let spec = McpServerSpec::new(
        McpServerId("ws".into()),
        "websocket fixture",
        TransportChoice::WebSocket {
            url: format!("ws://{addr}"),
            headers: BTreeMap::default(),
        },
        McpServerSource::Workspace,
    );

    let connection = McpClient::new(std::sync::Arc::new(WebsocketTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("websocket connects");
    let mut changes = connection.subscribe_changes().await.expect("changes");

    let tools = connection.list_tools().await.expect("tools list");
    assert_eq!(tools[0].name, "lookup");
    assert_eq!(changes.next().await, Some(McpChange::ToolsListChanged));

    let result = connection
        .call_tool("lookup", json!({ "id": 1 }))
        .await
        .expect("tool call");
    assert_eq!(result, harness_mcp::McpToolResult::text("looked up"));
}

#[tokio::test]
async fn websocket_transport_routes_binary_responses_and_replies_to_server_requests() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket accept");

        let initialize = socket
            .next()
            .await
            .expect("initialize frame")
            .expect("initialize message")
            .into_text()
            .expect("initialize text");
        let initialize: Value = serde_json::from_str(&initialize).expect("initialize json");
        assert_eq!(initialize["params"]["protocolVersion"], "2025-11-25");
        socket
            .send(Message::binary(
                json!({
                    "jsonrpc": "2.0",
                    "id": initialize["id"].clone(),
                    "result": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "fixture", "version": "0.1.0" }
                    }
                })
                .to_string()
                .into_bytes(),
            ))
            .await
            .expect("send binary initialize response");

        let initialized = socket
            .next()
            .await
            .expect("initialized frame")
            .expect("initialized message")
            .into_text()
            .expect("initialized text");
        let initialized: Value = serde_json::from_str(&initialized).expect("initialized json");
        assert_eq!(initialized["method"], "notifications/initialized");

        let list = socket
            .next()
            .await
            .expect("list frame")
            .expect("list message")
            .into_text()
            .expect("list text");
        let list: Value = serde_json::from_str(&list).expect("list json");
        assert_eq!(list["method"], "tools/list");

        socket
            .send(Message::text(
                json!({ "jsonrpc": "2.0", "id": "server-ping", "method": "ping" }).to_string(),
            ))
            .await
            .expect("send server ping");
        let ping_response = socket
            .next()
            .await
            .expect("ping response frame")
            .expect("ping response message")
            .into_text()
            .expect("ping response text");
        let ping_response: Value =
            serde_json::from_str(&ping_response).expect("ping response json");
        assert_eq!(ping_response["id"], "server-ping");
        assert_eq!(ping_response["result"], json!({}));

        socket
            .send(Message::binary(
                json!({
                    "jsonrpc": "2.0",
                    "id": list["id"].clone(),
                    "result": {
                        "tools": [
                            { "name": "binary_lookup", "inputSchema": { "type": "object" } }
                        ]
                    }
                })
                .to_string()
                .into_bytes(),
            ))
            .await
            .expect("send binary tools response");
    });

    let spec = McpServerSpec::new(
        McpServerId("ws-binary-peer".into()),
        "websocket binary peer fixture",
        TransportChoice::WebSocket {
            url: format!("ws://{addr}"),
            headers: BTreeMap::default(),
        },
        McpServerSource::Workspace,
    );
    let connection = McpClient::new(Arc::new(WebsocketTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("websocket connects");
    let tools = connection
        .list_tools()
        .await
        .expect("binary tools response");
    assert_eq!(tools[0].name, "binary_lookup");
    connection.shutdown().await.expect("shutdown");
    server.await.expect("server task");
}

#[tokio::test]
async fn websocket_close_frame_wakes_pending_request_without_jsonrpc_shutdown() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket accept");
        while let Some(frame) = socket.next().await {
            let frame = frame.expect("message");
            let value: Value = serde_json::from_slice(frame.into_data().as_ref()).expect("json");
            match value.get("method").and_then(Value::as_str) {
                Some("initialize") => {
                    socket
                        .send(Message::text(
                            json!({
                                "jsonrpc": "2.0",
                                "id": value["id"].clone(),
                                "result": {
                                    "protocolVersion": "2025-11-25",
                                    "capabilities": { "tools": {} },
                                    "serverInfo": { "name": "fixture", "version": "0.1.0" }
                                }
                            })
                            .to_string(),
                        ))
                        .await
                        .expect("initialize response");
                }
                Some("tools/list") => {
                    socket.close(None).await.expect("close frame");
                    break;
                }
                Some("shutdown") => panic!("must not send JSON-RPC shutdown"),
                _ => {}
            }
        }
    });

    let spec = McpServerSpec::new(
        McpServerId("ws-close".into()),
        "websocket close fixture",
        TransportChoice::WebSocket {
            url: format!("ws://{addr}"),
            headers: BTreeMap::default(),
        },
        McpServerSource::Workspace,
    );
    let connection = McpClient::new(Arc::new(WebsocketTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("websocket connects");
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), connection.list_tools())
        .await
        .expect("close wakes pending request");
    assert!(matches!(result, Err(McpError::Connection(_))));
}

#[tokio::test]
async fn websocket_transport_continues_tool_call_after_elicitation_resolution() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket accept");
        let mut call_count = 0usize;
        while let Some(message) = socket.next().await {
            let text = message.expect("message").into_text().expect("text");
            let value: Value = serde_json::from_str(&text).expect("json");
            match value.get("method").and_then(Value::as_str) {
                Some("initialize") => {
                    socket
                        .send(Message::text(
                            json!({
                                "jsonrpc": "2.0",
                                "id": value["id"].clone(),
                                "result": {
                                    "protocolVersion": "2025-11-25",
                                    "capabilities": { "tools": {} },
                                    "serverInfo": { "name": "fixture", "version": "0.1.0" }
                                }
                            })
                            .to_string(),
                        ))
                        .await
                        .expect("send initialize");
                }
                Some("tools/call") => {
                    call_count += 1;
                    if call_count == 1 {
                        socket
                            .send(Message::text(
                                json!({
                                    "jsonrpc": "2.0",
                                    "id": value["id"].clone(),
                                    "error": {
                                        "code": MCP_ELICITATION_REQUIRED_CODE,
                                        "message": "more input required",
                                        "data": {
                                            "server_id": "ws",
                                            "request_id": RequestId::from_u128(42),
                                            "subject": "credentials",
                                            "schema": { "type": "object" }
                                        }
                                    }
                                })
                                .to_string(),
                            ))
                            .await
                            .expect("send elicitation");
                    } else {
                        assert_eq!(value["params"]["arguments"]["token"], "resolved");
                        socket
                            .send(Message::text(
                                json!({
                                    "jsonrpc": "2.0",
                                    "id": value["id"].clone(),
                                    "result": {
                                        "content": [{ "type": "text", "text": "looked up" }],
                                        "isError": false
                                    }
                                })
                                .to_string(),
                            ))
                            .await
                            .expect("send tool result");
                    }
                }
                _ => {}
            }
        }
    });

    let spec = McpServerSpec::new(
        McpServerId("ws".into()),
        "websocket fixture",
        TransportChoice::WebSocket {
            url: format!("ws://{addr}"),
            headers: BTreeMap::default(),
        },
        McpServerSource::Workspace,
    );
    let handler =
        DirectElicitationHandler::new(|_request| async { Ok(json!({ "token": "resolved" })) });

    let connection = McpClient::new(Arc::new(WebsocketTransport::new()))
        .connect_with_context(
            spec,
            support::with_transport_authorization(
                McpConnectContext::default().with_elicitation_handler(Arc::new(handler)),
            ),
        )
        .await
        .expect("websocket connects");

    let result = connection
        .call_tool("lookup", json!({ "id": 1 }))
        .await
        .expect("tool call continues");
    assert_eq!(result, harness_mcp::McpToolResult::text("looked up"));
}

#[tokio::test]
async fn websocket_transport_sends_resource_subscription_requests() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let methods = Arc::new(Mutex::new(Vec::new()));
    let received_methods = methods.clone();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket accept");
        while let Some(message) = socket.next().await {
            let text = message.expect("message").into_text().expect("text");
            let value: Value = serde_json::from_str(&text).expect("json");
            let Some(method) = value.get("method").and_then(Value::as_str) else {
                continue;
            };
            received_methods.lock().push(method.to_owned());
            match method {
                "initialize" => {
                    socket
                        .send(Message::text(
                            json!({
                                "jsonrpc": "2.0",
                                "id": value["id"].clone(),
                                "result": {
                                    "protocolVersion": "2025-11-25",
                                    "capabilities": {
                                        "tools": {},
                                        "resources": { "subscribe": true }
                                    },
                                    "serverInfo": { "name": "fixture", "version": "0.1.0" }
                                }
                            })
                            .to_string(),
                        ))
                        .await
                        .expect("send initialize");
                }
                "resources/subscribe" | "resources/unsubscribe" => {
                    socket
                        .send(Message::text(
                            json!({
                                "jsonrpc": "2.0",
                                "id": value["id"].clone(),
                                "result": {}
                            })
                            .to_string(),
                        ))
                        .await
                        .expect("send resource subscription response");
                }
                _ => {}
            }
        }
    });

    let spec = McpServerSpec::new(
        McpServerId("ws-observers".into()),
        "websocket subscription fixture",
        TransportChoice::WebSocket {
            url: format!("ws://{addr}"),
            headers: BTreeMap::default(),
        },
        McpServerSource::Workspace,
    );

    let connection = McpClient::new(Arc::new(WebsocketTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("websocket connects");
    connection
        .subscribe_resource("jyowo://sessions/1")
        .await
        .expect("subscribe");
    connection
        .unsubscribe_resource("jyowo://sessions/1")
        .await
        .expect("unsubscribe");
    connection.shutdown().await.expect("shutdown");

    assert_eq!(
        methods.lock().as_slice(),
        &[
            "initialize",
            "notifications/initialized",
            "resources/subscribe",
            "resources/unsubscribe",
        ]
    );
}

#[tokio::test]
async fn websocket_tool_call_stream_filters_progress_by_request_id_and_finishes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket accept");
        while let Some(message) = socket.next().await {
            let text = message.expect("message").into_text().expect("text");
            let value: Value = serde_json::from_str(&text).expect("json");
            match value.get("method").and_then(Value::as_str) {
                Some("initialize") => {
                    socket
                        .send(Message::text(
                            json!({
                                "jsonrpc": "2.0",
                                "id": value["id"].clone(),
                                "result": {
                                    "protocolVersion": "2025-11-25",
                                    "capabilities": { "tools": {} },
                                    "serverInfo": { "name": "fixture", "version": "0.1.0" }
                                }
                            })
                            .to_string(),
                        ))
                        .await
                        .expect("send initialize");
                }
                Some("tools/call") => {
                    let id = value["id"].clone();
                    socket
                        .send(Message::text(
                            json!({
                                "jsonrpc": "2.0",
                                "method": "notifications/progress",
                                "params": {
                                    "progressToken": "unrelated",
                                    "progress": 99,
                                    "total": 100,
                                    "message": "wrong call"
                                }
                            })
                            .to_string(),
                        ))
                        .await
                        .expect("send unrelated progress");
                    socket
                        .send(Message::text(
                            json!({
                                "jsonrpc": "2.0",
                                "method": "notifications/progress",
                                "params": {
                                    "progressToken": id.to_string(),
                                    "progress": 1,
                                    "total": 4,
                                    "message": "quarter"
                                }
                            })
                            .to_string(),
                        ))
                        .await
                        .expect("send progress");
                    socket
                        .send(Message::text(
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "content": [{ "type": "text", "text": "done" }],
                                    "isError": false
                                }
                            })
                            .to_string(),
                        ))
                        .await
                        .expect("send final");
                }
                _ => {}
            }
        }
    });

    let spec = McpServerSpec::new(
        McpServerId("ws-progress".into()),
        "websocket progress fixture",
        TransportChoice::WebSocket {
            url: format!("ws://{addr}"),
            headers: BTreeMap::default(),
        },
        McpServerSource::Workspace,
    );
    let connection = McpClient::new(Arc::new(WebsocketTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("websocket connects");

    let mut events = connection
        .call_tool_events("lookup", json!({ "id": 1 }))
        .await
        .expect("tool call stream");

    assert_eq!(
        events.next().await,
        Some(McpToolCallEvent::Progress {
            progress_token: Some("2".into()),
            progress: Some(1.0),
            total: Some(4.0),
            message: Some("quarter".into())
        })
    );
    assert_eq!(
        events.next().await,
        Some(McpToolCallEvent::Final(harness_mcp::McpToolResult::text(
            "done"
        )))
    );
}

#[tokio::test]
#[cfg(feature = "oauth")]
async fn websocket_transport_refreshes_oauth_for_handshake_authorization() {
    let token_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({
            "grant_type": "refresh_token",
            "client_id": "client",
            "client_secret": "secret",
            "refresh_token": "refresh"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "ws-oauth-access",
            "token_type": "Bearer",
            "expires_in": 300,
            "refresh_token": "refresh"
        })))
        .expect(1)
        .mount(&token_server)
        .await;

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let authorization = Arc::new(Mutex::new(None::<String>));
    let seen_authorization = authorization.clone();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_hdr_async(stream, |request: &Request, response: Response| {
            let value = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned);
            *seen_authorization.lock() = value;
            Ok(response)
        })
        .await
        .expect("websocket accept");
        while let Some(message) = socket.next().await {
            let text = message.expect("message").into_text().expect("text");
            let value: Value = serde_json::from_str(&text).expect("json");
            if value.get("method").and_then(Value::as_str) == Some("initialize") {
                socket
                    .send(Message::text(
                        json!({
                            "jsonrpc": "2.0",
                            "id": value["id"].clone(),
                            "result": {
                                "protocolVersion": "2025-11-25",
                                "capabilities": { "tools": {} },
                                "serverInfo": { "name": "fixture", "version": "0.1.0" }
                            }
                        })
                        .to_string(),
                    ))
                    .await
                    .expect("send initialize");
            }
        }
    });

    let mut spec = McpServerSpec::new(
        McpServerId("ws-oauth".into()),
        "websocket oauth fixture",
        TransportChoice::WebSocket {
            url: format!("ws://{addr}"),
            headers: BTreeMap::default(),
        },
        McpServerSource::Workspace,
    );
    spec.auth = McpClientAuth::OAuth {
        authorize_url: "http://authorize.example.test".into(),
        token_url: token_server.uri(),
        client_id: "client".into(),
        client_secret: "secret".into(),
        scopes: vec!["tools".into()],
        refresh_token: Some("refresh".into()),
    };

    McpClient::new(Arc::new(WebsocketTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("websocket oauth connects");

    assert_eq!(
        authorization.lock().as_deref(),
        Some("Bearer ws-oauth-access")
    );
}

#[tokio::test]
#[cfg(feature = "oauth")]
async fn websocket_transport_retries_handshake_after_unauthorized_oauth_refresh() {
    let token_server = MockServer::start().await;
    let refreshes = Arc::new(AtomicUsize::new(0));
    let token_refreshes = refreshes.clone();
    Mock::given(method("POST"))
        .respond_with(move |_: &wiremock::Request| {
            let token_number = token_refreshes.fetch_add(1, Ordering::SeqCst) + 1;
            ResponseTemplate::new(200).set_body_json(json!({
                "access_token": format!("ws-retry-{token_number}"),
                "token_type": "Bearer",
                "expires_in": 300,
                "refresh_token": "refresh"
            }))
        })
        .expect(2)
        .mount(&token_server)
        .await;

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let handshakes = Arc::new(AtomicUsize::new(0));
    let observed_handshakes = handshakes.clone();
    tokio::spawn(async move {
        for _ in 0..2 {
            let (stream, _) = listener.accept().await.expect("accept");
            let attempts = observed_handshakes.clone();
            let accepted =
                accept_hdr_async(stream, move |request: &Request, response: Response| {
                    let attempt = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                    let authorization = request
                        .headers()
                        .get("authorization")
                        .and_then(|value| value.to_str().ok());
                    if attempt == 1 {
                        assert_eq!(authorization, Some("Bearer ws-retry-1"));
                        let mut response = ErrorResponse::new(Some("expired".into()));
                        *response.status_mut() = StatusCode::UNAUTHORIZED;
                        return Err(response);
                    }
                    assert_eq!(authorization, Some("Bearer ws-retry-2"));
                    Ok(response)
                })
                .await;
            let Ok(mut socket) = accepted else {
                continue;
            };
            let mut initialized = false;
            while let Some(message) = socket.next().await {
                let text = message.expect("message").into_text().expect("text");
                let value: Value = serde_json::from_str(&text).expect("json");
                match value.get("method").and_then(Value::as_str) {
                    Some("initialize") => {
                        socket
                            .send(Message::text(
                                json!({
                                    "jsonrpc": "2.0",
                                    "id": value["id"].clone(),
                                    "result": {
                                        "protocolVersion": "2025-11-25",
                                        "capabilities": { "tools": {} },
                                        "serverInfo": { "name": "fixture", "version": "0.1.0" }
                                    }
                                })
                                .to_string(),
                            ))
                            .await
                            .expect("send initialize");
                    }
                    Some("notifications/initialized") => {
                        initialized = true;
                        break;
                    }
                    _ => {}
                }
            }
            assert!(initialized);
            break;
        }
    });

    let mut spec = McpServerSpec::new(
        McpServerId("ws-oauth-retry".into()),
        "websocket oauth retry fixture",
        TransportChoice::WebSocket {
            url: format!("ws://{addr}"),
            headers: BTreeMap::default(),
        },
        McpServerSource::Workspace,
    );
    spec.auth = McpClientAuth::OAuth {
        authorize_url: "http://authorize.example.test".into(),
        token_url: token_server.uri(),
        client_id: "client".into(),
        client_secret: "secret".into(),
        scopes: vec!["tools".into()],
        refresh_token: Some("refresh".into()),
    };

    McpClient::new(Arc::new(WebsocketTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("websocket oauth connects after refresh");

    assert_eq!(refreshes.load(Ordering::SeqCst), 2);
    assert_eq!(handshakes.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn websocket_transport_rejects_xaa_without_request_signer() {
    let mut spec = McpServerSpec::new(
        McpServerId("ws-xaa".into()),
        "websocket xaa fixture",
        TransportChoice::WebSocket {
            url: "ws://127.0.0.1:9".into(),
            headers: BTreeMap::default(),
        },
        McpServerSource::Workspace,
    );
    spec.auth = McpClientAuth::Xaa {
        parent_session: harness_contracts::SessionId::from_u128(7),
        scopes: vec!["tools".into()],
    };

    let error = match McpClient::new(Arc::new(WebsocketTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
    {
        Ok(_) => panic!("xaa has no signer"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        McpError::Unsupported(message) if message.contains("XAA")
    ));
}

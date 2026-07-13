#![cfg(feature = "sse")]

use std::{
    collections::BTreeMap,
    convert::Infallible,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use futures::StreamExt;
use harness_contracts::{McpServerId, McpServerSource, RequestId};
use harness_mcp::{
    DirectElicitationHandler, McpChange, McpClient, McpClientAuth, McpConnectContext,
    McpServerSpec, SseTransport, TransportChoice, MCP_ELICITATION_REQUIRED_CODE,
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot},
};
use tokio_stream::wrappers::UnboundedReceiverStream;
#[cfg(feature = "oauth")]
use wiremock::{
    matchers::{body_partial_json, method},
    Mock, MockServer, ResponseTemplate,
};

mod support;

#[tokio::test]
async fn sse_transport_posts_requests_and_receives_streamed_responses() {
    let (addr, shutdown, _methods) = spawn_sse_fixture().await;
    let mut headers = BTreeMap::new();
    headers.insert("x-mcp-client".to_owned(), "jyowo".to_owned());
    let mut spec = McpServerSpec::new(
        McpServerId("sse".into()),
        "sse fixture",
        TransportChoice::Sse {
            url: format!("http://{addr}/mcp"),
            headers,
        },
        McpServerSource::Workspace,
    );
    spec.auth = McpClientAuth::Bearer("token".into());

    let connection = McpClient::new(std::sync::Arc::new(SseTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("sse connects");
    let mut changes = connection.subscribe_changes().await.expect("changes");

    let tools = connection.list_tools().await.expect("tools list");
    assert_eq!(tools[0].name, "sse_search");
    assert_eq!(changes.next().await, Some(McpChange::ToolsListChanged));

    let result = connection
        .call_tool("sse_search", json!({ "q": "mcp" }))
        .await
        .expect("tool call");
    assert_eq!(result, harness_mcp::McpToolResult::text("sse-found"));

    connection.shutdown().await.expect("shutdown");
    let _ = shutdown.send(());
}

#[tokio::test]
async fn sse_transport_continues_tool_call_after_elicitation_resolution() {
    let (addr, shutdown, _methods) = spawn_sse_elicitation_fixture().await;
    let mut spec = McpServerSpec::new(
        McpServerId("sse".into()),
        "sse fixture",
        TransportChoice::Sse {
            url: format!("http://{addr}/mcp"),
            headers: BTreeMap::new(),
        },
        McpServerSource::Workspace,
    );
    spec.auth = McpClientAuth::Bearer("token".into());
    let handler =
        DirectElicitationHandler::new(|_request| async { Ok(json!({ "token": "resolved" })) });

    let connection = McpClient::new(Arc::new(SseTransport::new()))
        .connect_with_context(
            spec,
            support::with_transport_authorization(
                McpConnectContext::default().with_elicitation_handler(Arc::new(handler)),
            ),
        )
        .await
        .expect("sse connects");

    let result = connection
        .call_tool("sse_search", json!({ "q": "mcp" }))
        .await
        .expect("tool call continues");
    assert_eq!(result, harness_mcp::McpToolResult::text("sse-found"));

    connection.shutdown().await.expect("shutdown");
    let _ = shutdown.send(());
}

#[tokio::test]
async fn sse_transport_posts_resource_subscription_requests() {
    let (addr, shutdown, methods) = spawn_sse_fixture().await;
    let mut headers = BTreeMap::new();
    headers.insert("x-mcp-client".to_owned(), "jyowo".to_owned());
    let mut spec = McpServerSpec::new(
        McpServerId("sse-observers".into()),
        "sse subscription fixture",
        TransportChoice::Sse {
            url: format!("http://{addr}/mcp"),
            headers,
        },
        McpServerSource::Workspace,
    );
    spec.auth = McpClientAuth::Bearer("token".into());

    let connection = McpClient::new(Arc::new(SseTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("sse connects");

    connection
        .subscribe_resource("jyowo://sessions/1")
        .await
        .expect("subscribe");
    connection
        .unsubscribe_resource("jyowo://sessions/1")
        .await
        .expect("unsubscribe");
    connection.shutdown().await.expect("shutdown");
    let _ = shutdown.send(());

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
async fn sse_connect_waits_until_initialized_is_accepted() {
    let (addr, shutdown, initialized) = spawn_sse_fixture_with_options(SseFixtureOptions {
        initialized_delay: Duration::from_millis(150),
        ..SseFixtureOptions::default()
    })
    .await;
    let mut spec = McpServerSpec::new(
        McpServerId("sse-initialized-order".into()),
        "sse initialized ordering fixture",
        TransportChoice::Sse {
            url: format!("http://{addr}/mcp"),
            headers: BTreeMap::new(),
        },
        McpServerSource::Workspace,
    );
    spec.auth = McpClientAuth::Bearer("token".into());

    let connection = McpClient::new(Arc::new(SseTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("sse connects after initialized is accepted");

    assert!(initialized.load(Ordering::SeqCst));
    connection.shutdown().await.expect("shutdown");
    let _ = shutdown.send(());
}

#[tokio::test]
async fn sse_accepts_case_insensitive_event_stream_content_type() {
    let (addr, shutdown, _) = spawn_sse_fixture_with_options(SseFixtureOptions {
        uppercase_content_type: true,
        ..SseFixtureOptions::default()
    })
    .await;
    let mut spec = McpServerSpec::new(
        McpServerId("sse-content-type".into()),
        "sse content type fixture",
        TransportChoice::Sse {
            url: format!("http://{addr}/mcp"),
            headers: BTreeMap::new(),
        },
        McpServerSource::Workspace,
    );
    spec.auth = McpClientAuth::Bearer("token".into());

    let connection = McpClient::new(Arc::new(SseTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("case-insensitive event stream content type");
    connection.shutdown().await.expect("shutdown");
    let _ = shutdown.send(());
}

#[tokio::test]
async fn sse_post_timeout_closes_the_transport() {
    let (addr, shutdown, _) = spawn_sse_fixture_with_options(SseFixtureOptions {
        hang_tools_post: true,
        ..SseFixtureOptions::default()
    })
    .await;
    let mut spec = McpServerSpec::new(
        McpServerId("sse-post-timeout".into()),
        "sse post timeout fixture",
        TransportChoice::Sse {
            url: format!("http://{addr}/mcp"),
            headers: BTreeMap::new(),
        },
        McpServerSource::Workspace,
    );
    spec.timeouts.call_default = Duration::from_millis(50);
    spec.auth = McpClientAuth::Bearer("token".into());

    let connection = McpClient::new(Arc::new(SseTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("sse connects");
    connection
        .list_tools()
        .await
        .expect("streamed response completes before POST headers");
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(connection.subscribe_changes().await.is_err());
    let _ = shutdown.send(());
}

#[tokio::test]
async fn sse_transport_rejects_cross_origin_discovered_endpoint() {
    use axum::{
        response::{sse::Event, Sse},
        routing::get,
        Router,
    };
    use futures::stream;
    use std::convert::Infallible;

    let app = Router::new().route(
        "/configured-stream",
        get(|| async {
            Sse::new(stream::once(async {
                Ok::<_, Infallible>(
                    Event::default()
                        .event("endpoint")
                        .data("https://other.example.test/rpc"),
                )
            }))
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    let spec = McpServerSpec::new(
        McpServerId("sse-cross-origin".into()),
        "cross-origin fixture",
        TransportChoice::Sse {
            url: format!("http://{addr}/configured-stream"),
            headers: BTreeMap::new(),
        },
        McpServerSource::Workspace,
    );

    let error = match McpClient::new(Arc::new(SseTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
    {
        Ok(_) => panic!("cross-origin endpoint must be rejected"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("origin"), "{error}");
}

#[tokio::test]
#[cfg(feature = "oauth")]
async fn sse_transport_refreshes_oauth_for_stream_and_request_clients() {
    let token_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({
            "grant_type": "refresh_token",
            "client_id": "client",
            "client_secret": "secret",
            "refresh_token": "refresh"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "sse-oauth-access",
            "token_type": "Bearer",
            "expires_in": 300,
            "refresh_token": "refresh"
        })))
        .expect(1)
        .mount(&token_server)
        .await;

    let (addr, shutdown, _methods) = spawn_sse_fixture_with_auth("Bearer sse-oauth-access").await;
    let mut spec = McpServerSpec::new(
        McpServerId("sse-oauth".into()),
        "sse oauth fixture",
        TransportChoice::Sse {
            url: format!("http://{addr}/mcp"),
            headers: BTreeMap::new(),
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

    let connection = McpClient::new(Arc::new(SseTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("sse oauth connects");
    connection.shutdown().await.expect("shutdown");
    let _ = shutdown.send(());
}

async fn spawn_sse_fixture() -> (SocketAddr, oneshot::Sender<()>, Arc<Mutex<Vec<String>>>) {
    spawn_sse_fixture_with_auth("Bearer token").await
}

async fn spawn_sse_elicitation_fixture(
) -> (SocketAddr, oneshot::Sender<()>, Arc<Mutex<Vec<String>>>) {
    spawn_sse_fixture_with_auth_and_elicitation("Bearer token", true).await
}

async fn spawn_sse_fixture_with_auth(
    expected_authorization: &'static str,
) -> (SocketAddr, oneshot::Sender<()>, Arc<Mutex<Vec<String>>>) {
    spawn_sse_fixture_with_auth_and_elicitation(expected_authorization, false).await
}

async fn spawn_sse_fixture_with_auth_and_elicitation(
    expected_authorization: &'static str,
    require_elicitation: bool,
) -> (SocketAddr, oneshot::Sender<()>, Arc<Mutex<Vec<String>>>) {
    let (addr, shutdown, initialized) = spawn_sse_fixture_with_options(SseFixtureOptions {
        expected_authorization,
        require_elicitation,
        ..SseFixtureOptions::default()
    })
    .await;
    let methods = initialized.methods.clone();
    (addr, shutdown, methods)
}

#[derive(Clone, Copy)]
struct SseFixtureOptions {
    expected_authorization: &'static str,
    require_elicitation: bool,
    initialized_delay: Duration,
    uppercase_content_type: bool,
    hang_tools_post: bool,
}

impl Default for SseFixtureOptions {
    fn default() -> Self {
        Self {
            expected_authorization: "Bearer token",
            require_elicitation: false,
            initialized_delay: Duration::ZERO,
            uppercase_content_type: false,
            hang_tools_post: false,
        }
    }
}

struct SseFixtureStateHandle {
    initialized: Arc<AtomicBool>,
    methods: Arc<Mutex<Vec<String>>>,
}

impl SseFixtureStateHandle {
    fn load(&self, ordering: Ordering) -> bool {
        self.initialized.load(ordering)
    }
}

async fn spawn_sse_fixture_with_options(
    options: SseFixtureOptions,
) -> (SocketAddr, oneshot::Sender<()>, SseFixtureStateHandle) {
    use axum::{
        body::Bytes,
        extract::State,
        http::{header::CONNECTION, HeaderMap, StatusCode},
        response::IntoResponse,
        response::{sse::Event, Sse},
        routing::{get, post},
        Router,
    };

    #[derive(Clone)]
    struct AppState {
        events: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
        methods: Arc<Mutex<Vec<String>>>,
        tool_calls: Arc<AtomicUsize>,
        expected_authorization: &'static str,
        require_elicitation: bool,
        initialized: Arc<AtomicBool>,
        initialized_delay: Duration,
        uppercase_content_type: bool,
        hang_tools_post: bool,
    }

    fn authorized(headers: &HeaderMap, expected_authorization: &str) -> bool {
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            == Some(expected_authorization)
            && headers
                .get("x-mcp-client")
                .and_then(|value| value.to_str().ok())
                .map_or(true, |value| value == "jyowo")
    }

    async fn send_event(state: &AppState, data: String) {
        for _ in 0..50 {
            let sender = state.events.lock().clone();
            if let Some(sender) = sender {
                if sender.send(data.clone()).is_ok() {
                    return;
                }
                *state.events.lock() = None;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("send sse event");
    }

    async fn rpc(
        State(state): State<AppState>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Result<impl IntoResponse, StatusCode> {
        if !authorized(&headers, state.expected_authorization) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let request: Value = serde_json::from_slice(&body).expect("request json");
        if let Some(method) = request.get("method").and_then(Value::as_str) {
            state.methods.lock().push(method.to_owned());
        }
        let response = match request.get("method").and_then(Value::as_str) {
            Some("initialize") => json!({
                "jsonrpc": "2.0",
                "id": request["id"].clone(),
                "result": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "fixture", "version": "0.1.0" }
                }
            }),
            Some("tools/list") => {
                send_event(
                    &state,
                    json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/tools/list_changed"
                    })
                    .to_string(),
                )
                .await;
                json!({
                    "jsonrpc": "2.0",
                    "id": request["id"].clone(),
                    "result": {
                        "tools": [
                            { "name": "sse_search", "description": "SSE search", "inputSchema": { "type": "object" } }
                        ]
                    }
                })
            }
            Some("tools/call") => {
                let call = state.tool_calls.fetch_add(1, Ordering::SeqCst) + 1;
                if state.require_elicitation && call == 1 {
                    json!({
                        "jsonrpc": "2.0",
                        "id": request["id"].clone(),
                        "error": {
                            "code": MCP_ELICITATION_REQUIRED_CODE,
                            "message": "more input required",
                            "data": {
                                "server_id": "sse",
                                "request_id": RequestId::from_u128(42),
                                "subject": "credentials",
                                "schema": { "type": "object" }
                            }
                        }
                    })
                } else {
                    if state.require_elicitation {
                        assert_eq!(request["params"]["arguments"]["token"], "resolved");
                    }
                    json!({
                        "jsonrpc": "2.0",
                        "id": request["id"].clone(),
                        "result": { "content": [{ "type": "text", "text": "sse-found" }], "isError": false }
                    })
                }
            }
            Some("resources/subscribe") | Some("resources/unsubscribe") => json!({
                "jsonrpc": "2.0",
                "id": request["id"].clone(),
                "result": {}
            }),
            Some("notifications/initialized") => {
                tokio::time::sleep(state.initialized_delay).await;
                state.initialized.store(true, Ordering::SeqCst);
                return Ok(([(CONNECTION, "close")], StatusCode::ACCEPTED));
            }
            other => json!({
                "jsonrpc": "2.0",
                "id": request["id"].clone(),
                "error": { "code": -32601, "message": format!("unknown method: {other:?}") }
            }),
        };
        send_event(&state, response.to_string()).await;
        if state.hang_tools_post
            && request.get("method").and_then(Value::as_str) == Some("tools/list")
        {
            std::future::pending::<()>().await;
        }
        Ok(([(CONNECTION, "close")], StatusCode::ACCEPTED))
    }

    async fn real_stream(
        State(state): State<AppState>,
        headers: HeaderMap,
    ) -> Result<axum::response::Response, StatusCode> {
        if !authorized(&headers, state.expected_authorization) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let (sender, receiver) = mpsc::unbounded_channel();
        *state.events.lock() = Some(sender);
        let endpoint = futures::stream::once(async {
            Ok::<_, Infallible>(Event::default().event("endpoint").data("/rpc"))
        });
        let messages =
            UnboundedReceiverStream::new(receiver).map(|data| Ok(Event::default().data(data)));
        let stream = endpoint.chain(messages);
        let mut response = Sse::new(stream).into_response();
        if state.uppercase_content_type {
            response.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("Text/Event-Stream; charset=utf-8"),
            );
        }
        Ok(response)
    }

    let state = AppState {
        events: Arc::new(Mutex::new(None)),
        methods: Arc::new(Mutex::new(Vec::new())),
        tool_calls: Arc::new(AtomicUsize::new(0)),
        expected_authorization: options.expected_authorization,
        require_elicitation: options.require_elicitation,
        initialized: Arc::new(AtomicBool::new(false)),
        initialized_delay: options.initialized_delay,
        uppercase_content_type: options.uppercase_content_type,
        hang_tools_post: options.hang_tools_post,
    };
    let methods = state.methods.clone();
    let initialized = state.initialized.clone();
    let app = Router::new()
        .route("/mcp", get(real_stream))
        .route("/rpc", post(rpc))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("serve");
    });
    wait_for_listener(addr).await;
    (
        addr,
        shutdown_tx,
        SseFixtureStateHandle {
            initialized,
            methods,
        },
    )
}

async fn wait_for_listener(addr: SocketAddr) {
    for _ in 0..20 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

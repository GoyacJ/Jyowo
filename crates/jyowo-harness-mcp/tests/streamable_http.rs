#![cfg(feature = "http")]

use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use harness_contracts::{McpServerId, McpServerSource};
use harness_mcp::{HttpTransport, McpClient, McpError, McpServerSpec, TransportChoice};
use serde_json::{json, Value};
use wiremock::{
    matchers::{body_partial_json, header, header_exists, method},
    Mock, MockServer, Request, Respond, ResponseTemplate,
};

mod support;

fn spec(server: &MockServer) -> McpServerSpec {
    McpServerSpec::new(
        McpServerId("streamable-http".into()),
        "streamable http fixture",
        TransportChoice::Http {
            url: server.uri(),
            headers: BTreeMap::new(),
        },
        McpServerSource::Workspace,
    )
}

#[tokio::test]
async fn initialization_negotiates_latest_version_and_reuses_session_headers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({
            "method": "initialize",
            "params": { "protocolVersion": "2025-11-25" }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .insert_header("mcp-session-id", "session-one")
                .set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "fixture", "version": "0.1.0" }
                    }
                })),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(header("mcp-session-id", "session-one"))
        .and(header("mcp-protocol-version", "2025-11-25"))
        .and(body_partial_json(json!({
            "method": "notifications/initialized"
        })))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(header("accept", "text/event-stream"))
        .and(header("mcp-session-id", "session-one"))
        .and(header("mcp-protocol-version", "2025-11-25"))
        .respond_with(ResponseTemplate::new(405))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(header("mcp-session-id", "session-one"))
        .and(header("mcp-protocol-version", "2025-11-25"))
        .and(body_partial_json(json!({ "method": "tools/list" })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "result": { "tools": [] }
                })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let connection_result = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await;
    let connection = connection_result.expect("streamable HTTP connects");
    let requests = server.received_requests().await.expect("request log");
    let initialize = requests
        .iter()
        .find(|request| {
            serde_json::from_slice::<serde_json::Value>(&request.body)
                .ok()
                .and_then(|body| body.get("method").cloned())
                == Some(json!("initialize"))
        })
        .expect("initialize request");
    assert_eq!(
        initialize
            .headers
            .get("accept")
            .and_then(|value| value.to_str().ok()),
        Some("application/json, text/event-stream")
    );
    assert!(connection
        .list_tools()
        .await
        .expect("tools list")
        .is_empty());
}

#[tokio::test]
async fn initialize_request_has_no_session_or_protocol_headers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header_exists("accept"))
        .and(body_partial_json(json!({ "method": "initialize" })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "fixture", "version": "0.1.0" }
                    }
                })),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            json!({ "method": "notifications/initialized" }),
        ))
        .respond_with(ResponseTemplate::new(202))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;

    McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("stateless streamable HTTP connects");

    let requests = server.received_requests().await.expect("request log");
    let initialize = requests
        .iter()
        .find(|request| request.method.as_str() == "POST")
        .expect("initialize request");
    assert!(!initialize.headers.contains_key("mcp-session-id"));
    assert!(!initialize.headers.contains_key("mcp-protocol-version"));
}

#[tokio::test]
async fn transport_owned_headers_cannot_be_injected_by_configuration() {
    for owned in [
        "accept",
        "content-type",
        "host",
        "mcp-session-id",
        "mcp-protocol-version",
        "last-event-id",
    ] {
        let server = MockServer::start().await;
        let mut headers = BTreeMap::new();
        headers.insert(owned.to_owned(), "attacker-controlled".to_owned());
        let mut invalid = spec(&server);
        invalid.transport = TransportChoice::Http {
            url: server.uri(),
            headers,
        };

        let error = match McpClient::new(Arc::new(HttpTransport::new()))
            .connect_with_context(invalid, support::authorized_connect_context())
            .await
        {
            Ok(_) => panic!("reserved header {owned} must be rejected"),
            Err(error) => error,
        };
        assert!(matches!(error, McpError::Protocol(_)));
        assert!(server
            .received_requests()
            .await
            .expect("request log")
            .is_empty());
    }
}

#[tokio::test]
async fn redirects_are_disabled_by_default() {
    let destination = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "redirect-target", "version": "0.1.0" }
                    }
                })),
        )
        .mount(&destination)
        .await;

    let origin = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(307).insert_header("location", destination.uri().as_str()),
        )
        .expect(1)
        .mount(&origin)
        .await;

    let error = match McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&origin), support::authorized_connect_context())
        .await
    {
        Ok(_) => panic!("redirected initialize must not connect"),
        Err(error) => error,
    };
    assert!(matches!(error, McpError::Transport(_)));
    assert!(destination
        .received_requests()
        .await
        .expect("destination request log")
        .is_empty());
}

#[tokio::test]
async fn legacy_fallback_status_is_preserved_as_a_typed_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "initialize" })))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;

    let error = match McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
    {
        Ok(_) => panic!("streamable initialization should fail"),
        Err(error) => error,
    };
    assert_eq!(error, McpError::StreamableHttpUnavailable(405));
}

#[tokio::test]
async fn initialized_requires_202_with_an_empty_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "initialize" })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "fixture", "version": "0.1.0" }
                    }
                })),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            json!({ "method": "notifications/initialized" }),
        ))
        .respond_with(ResponseTemplate::new(202).set_body_string("not empty"))
        .mount(&server)
        .await;

    let error = match McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
    {
        Ok(_) => panic!("non-empty 202 must fail initialization"),
        Err(error) => error,
    };
    assert!(matches!(error, McpError::InvalidResponse(_)));
}

#[tokio::test]
async fn json_response_requires_matching_id_and_stable_session() {
    let server = MockServer::start().await;
    mount_initialize(&server, Some("stable-session")).await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "tools/list" })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .insert_header("mcp-session-id", "drifted-session")
                .set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": 99,
                    "result": { "tools": [] }
                })),
        )
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("streamable HTTP connects");
    let error = connection
        .list_tools()
        .await
        .expect_err("session drift must fail");
    assert!(matches!(error, McpError::InvalidResponse(message) if message.contains("changed")));
}

#[tokio::test]
async fn json_response_requires_application_json_and_matching_request_id() {
    let server = MockServer::start().await;
    mount_initialize(&server, None).await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "tools/list" })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": 99,
                    "result": { "tools": [] }
                })),
        )
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("streamable HTTP connects");
    let error = connection
        .list_tools()
        .await
        .expect_err("wrong response id must fail");
    assert!(
        matches!(error, McpError::InvalidResponse(message) if message.contains("does not match"))
    );
}

#[tokio::test]
async fn post_sse_stops_after_routing_its_target_response() {
    let server = MockServer::start().await;
    mount_initialize(&server, Some("sse-session")).await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "tools/list" })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{\"progressToken\":2,\"progress\":0.5}}\n\n",
                        "data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[]}}\n\n",
                        "data: this tail must not be parsed\n\n"
                    ),
                    "text/event-stream",
                ),
        )
        .expect(1)
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("streamable HTTP connects");
    assert!(connection
        .list_tools()
        .await
        .expect("SSE tools list")
        .is_empty());
}

#[tokio::test]
async fn timed_out_posts_release_worker_capacity() {
    let server = MockServer::start().await;
    mount_initialize(&server, None).await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;
    let responder = SlowFirstToolRequests::default();
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "tools/list" })))
        .respond_with(responder.clone())
        .mount(&server)
        .await;

    let mut short_timeout_spec = spec(&server);
    short_timeout_spec.timeouts.call_default = Duration::from_millis(50);
    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(short_timeout_spec, support::authorized_connect_context())
        .await
        .expect("streamable HTTP connects");

    let mut calls = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let connection = Arc::clone(&connection);
        calls.spawn(async move { connection.list_tools().await });
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        while let Some(result) = calls.join_next().await {
            assert!(result.expect("request task completes").is_err());
        }
    })
    .await
    .expect("timed-out requests complete");

    let tools = tokio::time::timeout(Duration::from_secs(1), connection.list_tools())
        .await
        .expect("a worker permit is released")
        .expect("the next request reaches the server");
    assert!(tools.is_empty());
    assert_eq!(responder.requests.load(Ordering::SeqCst), 17);
}

#[derive(Clone, Default)]
struct SlowFirstToolRequests {
    requests: Arc<AtomicUsize>,
}

impl Respond for SlowFirstToolRequests {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let attempt = self.requests.fetch_add(1, Ordering::SeqCst);
        let body: Value = serde_json::from_slice(&request.body).expect("JSON-RPC request");
        let id = body.get("id").cloned().expect("request id");
        let response = ResponseTemplate::new(200)
            .insert_header("content-type", "application/json")
            .set_body_json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": [] }
            }));
        if attempt < 16 {
            response.set_delay(Duration::from_secs(5))
        } else {
            response
        }
    }
}

#[tokio::test]
async fn post_sse_resumes_its_own_stream_with_last_event_id() {
    let server = MockServer::start().await;
    mount_initialize(&server, Some("resume-session")).await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "tools/list" })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    "id: post-stream-1\ndata:\n\nretry: 1\n\n",
                    "text/event-stream",
                ),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(PostResumeResponder)
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("streamable HTTP connects");
    assert!(connection
        .list_tools()
        .await
        .expect("POST stream resumes")
        .is_empty());

    let requests = server.received_requests().await.expect("request log");
    assert!(requests.iter().any(|request| {
        request.method.as_str() == "GET"
            && request
                .headers
                .get("last-event-id")
                .and_then(|value| value.to_str().ok())
                == Some("post-stream-1")
    }));
}

#[derive(Clone, Copy)]
struct PostResumeResponder;

impl Respond for PostResumeResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        if request
            .headers
            .get("last-event-id")
            .and_then(|value| value.to_str().ok())
            == Some("post-stream-1")
        {
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    "id: post-stream-2\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[]}}\n\n",
                    "text/event-stream",
                )
        } else {
            ResponseTemplate::new(405)
        }
    }
}

#[tokio::test]
async fn get_sse_server_request_is_answered_with_independent_post() {
    let server = MockServer::start().await;
    mount_initialize(&server, Some("get-session")).await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            json!({ "id": "server-ping", "result": {} }),
        ))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(header("mcp-session-id", "get-session"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    "data: {\"jsonrpc\":\"2.0\",\"id\":\"server-ping\",\"method\":\"ping\"}\n\n",
                    "text/event-stream",
                ),
        )
        .expect(1)
        .mount(&server)
        .await;

    let _connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("streamable HTTP connects");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}

#[tokio::test]
async fn get_sse_reconnects_with_its_own_last_event_id() {
    let server = MockServer::start().await;
    mount_initialize(&server, Some("get-resume-session")).await;
    let responder = GetResumeResponder::default();
    Mock::given(method("GET"))
        .respond_with(responder.clone())
        .mount(&server)
        .await;

    let _connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("streamable HTTP connects");
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while responder.gets.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("GET stream reconnects");
    assert!(responder.saw_last_event_id.load(Ordering::SeqCst) > 0);
}

#[tokio::test]
async fn get_sse_without_a_checkpoint_closes_the_peer() {
    let server = MockServer::start().await;
    mount_initialize(&server, Some("get-no-checkpoint-session")).await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{}}\n\n",
                    "text/event-stream",
                ),
        )
        .expect(1)
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("streamable HTTP connects");
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(matches!(
        connection.list_tools().await,
        Err(McpError::Connection(_))
    ));
}

#[tokio::test]
async fn stateless_get_failure_closes_the_peer() {
    let server = MockServer::start().await;
    mount_initialize(&server, None).await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("streamable HTTP connects");
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(matches!(
        connection.list_tools().await,
        Err(McpError::Connection(_))
    ));
}

#[tokio::test]
async fn get_sse_reconnect_exhaustion_closes_the_peer() {
    let server = MockServer::start().await;
    mount_initialize(&server, Some("get-exhausted-session")).await;
    let responder = AlwaysResumableGet::default();
    Mock::given(method("GET"))
        .respond_with(responder.clone())
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("streamable HTTP connects");
    tokio::time::timeout(Duration::from_secs(1), async {
        while responder.gets.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("GET reconnect limit is reached");
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(matches!(
        connection.list_tools().await,
        Err(McpError::Connection(_))
    ));
}

#[derive(Clone, Default)]
struct AlwaysResumableGet {
    gets: Arc<AtomicUsize>,
}

impl Respond for AlwaysResumableGet {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        let attempt = self.gets.fetch_add(1, Ordering::SeqCst);
        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_raw(
                format!(
                    "id: checkpoint-{attempt}\nretry: 1\ndata: {{\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{{}}}}\n\n"
                ),
                "text/event-stream",
            )
    }
}

#[tokio::test]
async fn ordinary_get_resumption_rejects_responses_without_request_context() {
    let server = MockServer::start().await;
    mount_initialize(&server, Some("get-context-session")).await;
    Mock::given(method("GET"))
        .respond_with(GetWithoutRequestContextResponder::default())
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("streamable HTTP connects");
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    let error = connection
        .list_tools()
        .await
        .expect_err("context-free GET response closes the peer");
    assert!(matches!(error, McpError::Connection(_)));
}

#[derive(Clone, Default)]
struct GetWithoutRequestContextResponder {
    gets: Arc<AtomicUsize>,
}

impl Respond for GetWithoutRequestContextResponder {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        let attempt = self.gets.fetch_add(1, Ordering::SeqCst);
        let body = if attempt == 0 {
            "id: get-context-1\nretry: 1\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{}}\n\n"
        } else {
            "data: {\"jsonrpc\":\"2.0\",\"id\":999,\"result\":{}}\n\n"
        };
        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_raw(body, "text/event-stream")
    }
}

#[tokio::test]
async fn get_404_starts_one_new_generation_without_waiting_for_a_business_call() {
    let server = MockServer::start().await;
    let post = GetExpiryPostResponder::default();
    let get = GetExpiryResponder::default();
    Mock::given(method("POST"))
        .respond_with(post.clone())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(get)
        .mount(&server)
        .await;

    let _connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("first generation connects");
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while post.initializations.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("GET 404 starts reinitialization");
    assert_eq!(post.initializations.load(Ordering::SeqCst), 2);
}

#[derive(Clone, Default)]
struct GetExpiryPostResponder {
    initializations: Arc<AtomicUsize>,
}

impl Respond for GetExpiryPostResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).expect("JSON-RPC request");
        match body.get("method").and_then(Value::as_str) {
            Some("initialize") => {
                let generation = self.initializations.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .insert_header(
                        "mcp-session-id",
                        if generation == 0 {
                            "get-old"
                        } else {
                            "get-new"
                        },
                    )
                    .set_body_json(json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": {
                            "protocolVersion": "2025-11-25",
                            "capabilities": { "tools": {} },
                            "serverInfo": { "name": "fixture", "version": "0.1.0" }
                        }
                    }))
            }
            Some("notifications/initialized") => ResponseTemplate::new(202),
            method => panic!("unexpected request method: {method:?}"),
        }
    }
}

#[derive(Clone, Default)]
struct GetExpiryResponder {
    gets: Arc<AtomicUsize>,
}

impl Respond for GetExpiryResponder {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        if self.gets.fetch_add(1, Ordering::SeqCst) == 0 {
            ResponseTemplate::new(404)
        } else {
            ResponseTemplate::new(405)
        }
    }
}

#[derive(Clone, Default)]
struct GetResumeResponder {
    gets: Arc<AtomicUsize>,
    saw_last_event_id: Arc<AtomicUsize>,
}

impl Respond for GetResumeResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let attempt = self.gets.fetch_add(1, Ordering::SeqCst);
        if request
            .headers
            .get("last-event-id")
            .and_then(|value| value.to_str().ok())
            == Some("get-stream-1")
        {
            self.saw_last_event_id.fetch_add(1, Ordering::SeqCst);
        }
        let body = if attempt == 0 {
            "id: get-stream-1\nretry: 1\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{}}\n\n"
        } else {
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{}}\n\n"
        };
        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_raw(body, "text/event-stream")
    }
}

#[tokio::test]
async fn shutdown_deletes_stateful_session_and_accepts_405() {
    let server = MockServer::start().await;
    mount_initialize(&server, Some("delete-session")).await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(header("mcp-session-id", "delete-session"))
        .and(header("mcp-protocol-version", "2025-11-25"))
        .respond_with(ResponseTemplate::new(405))
        .expect(1)
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("streamable HTTP connects");
    connection
        .shutdown()
        .await
        .expect("405 cleanup is accepted");
}

#[tokio::test]
async fn session_404_reinitializes_once_and_retries_the_original_request() {
    let server = MockServer::start().await;
    let responder = ExpiringSessionResponder::default();
    Mock::given(method("POST"))
        .respond_with(responder.clone())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("first generation connects");
    assert!(connection
        .list_tools()
        .await
        .expect("request is retried on the new generation")
        .is_empty());
    assert_eq!(responder.initializations.load(Ordering::SeqCst), 2);
    assert_eq!(responder.tool_requests.load(Ordering::SeqCst), 2);

    let requests = server.received_requests().await.expect("request log");
    let tools = requests
        .iter()
        .filter(|request| {
            serde_json::from_slice::<Value>(&request.body)
                .ok()
                .and_then(|body| body.get("method").cloned())
                == Some(json!("tools/list"))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tools[0].headers.get("mcp-session-id").unwrap(),
        "old-session"
    );
    assert_eq!(
        tools[1].headers.get("mcp-session-id").unwrap(),
        "new-session"
    );
}

#[derive(Clone, Default)]
struct ExpiringSessionResponder {
    initializations: Arc<AtomicUsize>,
    tool_requests: Arc<AtomicUsize>,
}

impl Respond for ExpiringSessionResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).expect("JSON-RPC request");
        match body.get("method").and_then(Value::as_str) {
            Some("initialize") => {
                let generation = self.initializations.fetch_add(1, Ordering::SeqCst);
                let session = if generation == 0 {
                    "old-session"
                } else {
                    "new-session"
                };
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .insert_header("mcp-session-id", session)
                    .set_body_json(json!({
                        "jsonrpc": "2.0",
                        "id": 1,
                        "result": {
                            "protocolVersion": "2025-11-25",
                            "capabilities": { "tools": {} },
                            "serverInfo": { "name": "fixture", "version": "0.1.0" }
                        }
                    }))
            }
            Some("notifications/initialized") => ResponseTemplate::new(202),
            Some("tools/list") => {
                let attempt = self.tool_requests.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    ResponseTemplate::new(404)
                } else {
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "application/json")
                        .set_body_json(json!({
                            "jsonrpc": "2.0",
                            "id": 2,
                            "result": { "tools": [] }
                        }))
                }
            }
            method => panic!("unexpected request method: {method:?}"),
        }
    }
}

async fn mount_initialize(server: &MockServer, session: Option<&str>) {
    let mut response = ResponseTemplate::new(200)
        .insert_header("content-type", "application/json")
        .set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2025-11-25",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "fixture", "version": "0.1.0" }
            }
        }));
    if let Some(session) = session {
        response = response.insert_header("mcp-session-id", session);
    }
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "initialize" })))
        .respond_with(response)
        .expect(1)
        .mount(server)
        .await;
    let mut initialized = Mock::given(method("POST")).and(body_partial_json(
        json!({ "method": "notifications/initialized" }),
    ));
    if let Some(session) = session {
        initialized = initialized.and(header("mcp-session-id", session));
    }
    initialized
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(server)
        .await;
}

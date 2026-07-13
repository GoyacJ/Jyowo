#![cfg(feature = "http")]

use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Condvar, Mutex as StdMutex,
    },
    time::{Duration, Instant},
};

use futures::StreamExt;
use harness_contracts::{McpServerId, McpServerSource};
use harness_mcp::{
    HttpTransport, McpChange, McpClient, McpError, McpServerSpec, McpTimeouts, ReconnectPolicy,
    TransportChoice,
};
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

async fn wait_for_count(counter: &AtomicUsize, expected: usize, message: &str) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while counter.load(Ordering::SeqCst) < expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{message}"));
}

#[derive(Clone, Default)]
struct AcceptedCounter {
    requests: Arc<AtomicUsize>,
}

impl Respond for AcceptedCounter {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        self.requests.fetch_add(1, Ordering::SeqCst);
        ResponseTemplate::new(202)
    }
}

#[derive(Clone)]
struct CountedStatus {
    requests: Arc<AtomicUsize>,
    status: u16,
}

impl CountedStatus {
    fn new(status: u16) -> Self {
        Self {
            requests: Arc::new(AtomicUsize::new(0)),
            status,
        }
    }
}

impl Respond for CountedStatus {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        self.requests.fetch_add(1, Ordering::SeqCst);
        ResponseTemplate::new(self.status)
    }
}

#[derive(Clone, Default)]
struct CountedNoCheckpointGet {
    requests: Arc<AtomicUsize>,
}

#[derive(Clone, Default)]
struct SuccessfulToolsResponder;

impl Respond for SuccessfulToolsResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).expect("JSON-RPC request");
        ResponseTemplate::new(200)
            .insert_header("content-type", "application/json")
            .set_body_json(json!({
                "jsonrpc": "2.0",
                "id": body.get("id").cloned().unwrap_or(Value::Null),
                "result": { "tools": [] }
            }))
    }
}

impl Respond for CountedNoCheckpointGet {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        self.requests.fetch_add(1, Ordering::SeqCst);
        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_raw(
                "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{}}\n\n",
                "text/event-stream",
            )
    }
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
    Mock::given(method("POST"))
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
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("stateless streamable HTTP connects");
    assert!(connection
        .list_tools()
        .await
        .expect("list tools")
        .is_empty());

    let requests = server.received_requests().await.expect("request log");
    let initialize = requests
        .iter()
        .find(|request| request.method.as_str() == "POST")
        .expect("initialize request");
    assert!(!initialize.headers.contains_key("mcp-session-id"));
    assert!(!initialize.headers.contains_key("mcp-protocol-version"));
    for request in requests.iter().filter(|request| {
        serde_json::from_slice::<Value>(&request.body)
            .ok()
            .and_then(|body| body.get("method").cloned())
            != Some(json!("initialize"))
            && request.method.as_str() == "POST"
    }) {
        assert_eq!(
            request.headers.get("mcp-protocol-version").unwrap(),
            "2025-11-25"
        );
        assert!(!request.headers.contains_key("mcp-session-id"));
    }
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
async fn transport_errors_do_not_expose_endpoint_query_secrets() {
    let secret = "query-secret-must-not-leak";
    let mut failing = McpServerSpec::new(
        McpServerId("redaction".into()),
        "redaction fixture",
        TransportChoice::Http {
            url: format!("http://localhost:1/mcp?token={secret}"),
            headers: BTreeMap::new(),
        },
        McpServerSource::Workspace,
    );
    failing.timeouts.handshake = Duration::from_millis(250);
    failing.timeouts.call_default = Duration::from_millis(250);

    let error = match McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(failing, support::authorized_connect_context())
        .await
    {
        Ok(_) => panic!("closed endpoint unexpectedly connected"),
        Err(error) => error,
    };
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains(secret));
    assert!(!rendered.contains("token="));
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
async fn response_mime_types_are_case_insensitive_and_allow_parameters() {
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
                .insert_header("content-type", "Application/JSON; Charset=UTF-8")
                .set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "result": { "tools": [] }
                })),
        )
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("connect");
    assert!(connection
        .list_tools()
        .await
        .expect("list tools")
        .is_empty());
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
async fn post_sse_server_request_is_answered_with_independent_post() {
    let server = MockServer::start().await;
    mount_initialize(&server, Some("post-server-request")).await;
    let answer = AcceptedCounter::default();
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            json!({ "id": "post-ping", "result": {} }),
        ))
        .respond_with(answer.clone())
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "tools/list" })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    concat!(
                        "data: {\"jsonrpc\":\"2.0\",\"id\":\"post-ping\",\"method\":\"ping\"}\n\n",
                        "data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[]}}\n\n"
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
        .expect("connect");
    assert!(connection
        .list_tools()
        .await
        .expect("list tools")
        .is_empty());
    wait_for_count(&answer.requests, 1, "POST SSE server request is answered").await;
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

#[tokio::test]
async fn long_post_sse_streams_do_not_starve_short_posts() {
    let server = MockServer::start().await;
    let responder = SaturatedPostSseResponder::default();
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
        .expect("connect");
    let mut streams = Vec::new();
    for _ in 0..16 {
        let connection = Arc::clone(&connection);
        streams.push(tokio::spawn(async move { connection.list_tools().await }));
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        while responder.tool_requests.load(Ordering::SeqCst) < 16 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("all long POST streams start");
    tokio::time::timeout(Duration::from_millis(500), async {
        while responder.response_posts.load(Ordering::SeqCst) < 16 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("server request responses are not starved");

    let tools = tokio::time::timeout(Duration::from_millis(500), connection.list_tools())
        .await
        .expect("short POST is not starved")
        .expect("short POST succeeds");
    assert!(tools.is_empty());

    for stream in streams {
        stream.abort();
    }
}

#[derive(Clone, Default)]
struct SaturatedPostSseResponder {
    tool_requests: Arc<AtomicUsize>,
    response_posts: Arc<AtomicUsize>,
}

impl Respond for SaturatedPostSseResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).expect("JSON-RPC request");
        let id = body.get("id").cloned().unwrap_or(Value::Null);
        match body.get("method").and_then(Value::as_str) {
            Some("initialize") => ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "fixture", "version": "0.1.0" }
                    }
                })),
            Some("notifications/initialized") => ResponseTemplate::new(202),
            Some("tools/list") => {
                let request_index = self.tool_requests.fetch_add(1, Ordering::SeqCst);
                if request_index < 16 {
                    ResponseTemplate::new(200).set_body_raw(
                        format!(
                            concat!(
                                "id: hold-{}\n",
                                "data: {{\"jsonrpc\":\"2.0\",\"id\":\"hold-ping-{}\",\"method\":\"ping\"}}\n\n",
                                "retry: 5000\n\n"
                            ),
                            request_index,
                            request_index,
                        ),
                        "text/event-stream",
                    )
                } else {
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "application/json")
                        .set_body_json(json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": { "tools": [] }
                        }))
                }
            }
            None if body.get("id").is_some() => {
                self.response_posts.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(202)
            }
            method => panic!("unexpected request method: {method:?}"),
        }
    }
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
    let answer = AcceptedCounter::default();
    Mock::given(method("POST"))
        .and(body_partial_json(
            json!({ "id": "server-ping", "result": {} }),
        ))
        .respond_with(answer.clone())
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
    wait_for_count(&answer.requests, 1, "GET SSE server request is answered").await;
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
    let get = CountedNoCheckpointGet::default();
    Mock::given(method("GET"))
        .respond_with(get.clone())
        .expect(1)
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("streamable HTTP connects");
    wait_for_count(&get.requests, 1, "GET SSE response is delivered").await;
    let result = tokio::time::timeout(Duration::from_secs(1), connection.list_tools())
        .await
        .expect("peer closure is observed");
    assert!(matches!(result, Err(McpError::Connection(_))));
}

#[tokio::test]
async fn stateless_get_failure_closes_the_peer() {
    let server = MockServer::start().await;
    mount_initialize(&server, None).await;
    let get = CountedStatus::new(500);
    Mock::given(method("GET"))
        .respond_with(get.clone())
        .expect(1)
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("streamable HTTP connects");
    wait_for_count(&get.requests, 1, "failed GET response is delivered").await;
    let result = tokio::time::timeout(Duration::from_secs(1), connection.list_tools())
        .await
        .expect("peer closure is observed");
    assert!(matches!(result, Err(McpError::Connection(_))));
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
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "tools/list" })))
        .respond_with(SuccessfulToolsResponder)
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
    let error = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match connection.list_tools().await {
                Err(error @ McpError::Connection(_)) => break error,
                Ok(_) => tokio::task::yield_now().await,
                Err(error) => panic!("unexpected error while awaiting peer closure: {error}"),
            }
        }
    })
    .await
    .expect("peer closure is observed");
    assert!(matches!(error, McpError::Connection(_)));
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
    let get = GetWithoutRequestContextResponder::default();
    Mock::given(method("GET"))
        .respond_with(get.clone())
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("streamable HTTP connects");
    wait_for_count(&get.gets, 2, "invalid resumed GET response is delivered").await;
    let error = tokio::time::timeout(Duration::from_secs(1), connection.list_tools())
        .await
        .expect("peer closure is observed")
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

#[tokio::test]
async fn immediate_get_404_on_a_new_generation_starts_the_next_generation() {
    let server = MockServer::start().await;
    let post = GetExpiryPostResponder::default();
    Mock::given(method("POST"))
        .respond_with(post.clone())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(TwoGetExpiries::default())
        .mount(&server)
        .await;

    let mut two_expiry_spec = spec(&server);
    two_expiry_spec.reconnect = ReconnectPolicy {
        max_attempts: 2,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(1),
        backoff_jitter: 0.0,
        ..ReconnectPolicy::default()
    };
    let _connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(two_expiry_spec, support::authorized_connect_context())
        .await
        .expect("first generation connects");
    tokio::time::timeout(Duration::from_secs(1), async {
        while post.initializations.load(Ordering::SeqCst) < 3 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both GET expiries rebuild");
    assert_eq!(post.initializations.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn successful_business_request_resets_the_session_expiry_budget() {
    let server = MockServer::start().await;
    let post = ResettingExpiryPostResponder::default();
    let get = GetExpiryResponder::default();
    Mock::given(method("POST"))
        .respond_with(post.clone())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(get)
        .mount(&server)
        .await;

    let mut reset_spec = spec(&server);
    reset_spec.reconnect = ReconnectPolicy {
        max_attempts: 1,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(1),
        backoff_jitter: 0.0,
        ..ReconnectPolicy::default()
    };
    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(reset_spec, support::authorized_connect_context())
        .await
        .expect("first generation connects");
    tokio::time::timeout(Duration::from_secs(1), async {
        while post.initializations.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("GET expiry rebuilds the first session");

    assert!(connection
        .list_tools()
        .await
        .expect("stable business request succeeds")
        .is_empty());
    assert!(connection
        .list_tools()
        .await
        .expect("a later expiry receives a fresh retry budget")
        .is_empty());
    assert_eq!(post.initializations.load(Ordering::SeqCst), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn change_subscription_survives_a_get_404_rebuild() {
    let server = MockServer::start().await;
    let post = GetExpiryPostResponder::default();
    let get = ExpiryThenListChanged::default();
    Mock::given(method("POST"))
        .respond_with(post.clone())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(get.clone())
        .mount(&server)
        .await;

    let mut rebuild_spec = spec(&server);
    rebuild_spec.reconnect = ReconnectPolicy {
        max_attempts: 1,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(1),
        backoff_jitter: 0.0,
        ..ReconnectPolicy::default()
    };
    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(rebuild_spec, support::authorized_connect_context())
        .await
        .expect("first generation connects");
    let mut changes = connection
        .subscribe_changes()
        .await
        .expect("subscribe before expiry");
    get.release_first_expiry();

    let change = tokio::time::timeout(Duration::from_secs(1), changes.next())
        .await
        .expect("new generation emits a change")
        .expect("change stream remains open");
    assert_eq!(change, McpChange::ToolsListChanged);
    assert_eq!(post.initializations.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn consecutive_get_404s_are_bounded_and_backed_off() {
    let server = MockServer::start().await;
    let post = GetExpiryPostResponder::default();
    let get = AlwaysExpiredGet::default();
    Mock::given(method("POST"))
        .respond_with(post.clone())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(get.clone())
        .mount(&server)
        .await;

    let mut bounded_spec = spec(&server);
    bounded_spec.reconnect = ReconnectPolicy {
        max_attempts: 2,
        initial_backoff: Duration::from_millis(25),
        max_backoff: Duration::from_millis(50),
        backoff_jitter: 0.0,
        ..ReconnectPolicy::default()
    };
    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(bounded_spec, support::authorized_connect_context())
        .await
        .expect("first generation connects");

    tokio::time::timeout(Duration::from_secs(1), async {
        while get.gets.load(Ordering::SeqCst) < 3 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("expiry budget is consumed");
    let first = tokio::time::timeout(Duration::from_secs(1), connection.list_tools())
        .await
        .expect("failed state is published")
        .expect_err("continuous session expiry is terminal");
    let second = connection
        .list_tools()
        .await
        .expect_err("following calls observe the same failed state");

    assert_eq!(first, second);
    assert_eq!(post.initializations.load(Ordering::SeqCst), 3);
    let gets = get.instants.lock().unwrap();
    assert_eq!(gets.len(), 3);
    assert!(gets[1].duration_since(gets[0]) >= Duration::from_millis(25));
    assert!(gets[2].duration_since(gets[1]) >= Duration::from_millis(50));
}

#[tokio::test]
async fn shutdown_interrupts_session_expiry_backoff() {
    let server = MockServer::start().await;
    let post = GetExpiryPostResponder::default();
    let get = AlwaysExpiredGet::default();
    Mock::given(method("POST"))
        .respond_with(post.clone())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(get.clone())
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;

    let mut slow_spec = spec(&server);
    slow_spec.reconnect = ReconnectPolicy {
        max_attempts: 2,
        initial_backoff: Duration::from_secs(5),
        max_backoff: Duration::from_secs(5),
        backoff_jitter: 0.0,
        ..ReconnectPolicy::default()
    };
    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(slow_spec, support::authorized_connect_context())
        .await
        .expect("first generation connects");
    tokio::time::timeout(Duration::from_secs(1), async {
        while get.gets.load(Ordering::SeqCst) < 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("GET session expiry is observed");

    assert_eq!(post.initializations.load(Ordering::SeqCst), 1);
    tokio::time::timeout(Duration::from_millis(250), connection.shutdown())
        .await
        .expect("shutdown interrupts backoff")
        .expect("shutdown succeeds");
    assert_eq!(post.initializations.load(Ordering::SeqCst), 1);
}

#[derive(Clone, Default)]
struct TwoGetExpiries {
    gets: Arc<AtomicUsize>,
}

#[derive(Clone, Default)]
struct AlwaysExpiredGet {
    gets: Arc<AtomicUsize>,
    instants: Arc<StdMutex<Vec<Instant>>>,
}

#[derive(Clone, Default)]
struct ExpiryThenListChanged {
    gets: Arc<AtomicUsize>,
    first_expiry_gate: Arc<(StdMutex<bool>, Condvar)>,
}

impl ExpiryThenListChanged {
    fn release_first_expiry(&self) {
        let (gate, wake) = &*self.first_expiry_gate;
        *gate.lock().unwrap() = true;
        wake.notify_all();
    }
}

impl Respond for ExpiryThenListChanged {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        match self.gets.fetch_add(1, Ordering::SeqCst) {
            0 => {
                let (gate, wake) = &*self.first_expiry_gate;
                let (gate, wait) = wake
                    .wait_timeout_while(gate.lock().unwrap(), Duration::from_secs(1), |open| !*open)
                    .unwrap();
                assert!(!wait.timed_out() && *gate, "subscription did not release GET expiry");
                ResponseTemplate::new(404)
            }
            1 => ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/tools/list_changed\"}\n\n",
                    "text/event-stream",
                ),
            _ => ResponseTemplate::new(405),
        }
    }
}

impl Respond for AlwaysExpiredGet {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        self.gets.fetch_add(1, Ordering::SeqCst);
        self.instants.lock().unwrap().push(Instant::now());
        ResponseTemplate::new(404)
    }
}

impl Respond for TwoGetExpiries {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        if self.gets.fetch_add(1, Ordering::SeqCst) < 2 {
            ResponseTemplate::new(404)
        } else {
            ResponseTemplate::new(405)
        }
    }
}

#[derive(Clone, Default)]
struct GetExpiryPostResponder {
    initializations: Arc<AtomicUsize>,
}

#[derive(Clone, Default)]
struct ResettingExpiryPostResponder {
    initializations: Arc<AtomicUsize>,
    tool_requests: Arc<AtomicUsize>,
}

impl Respond for ResettingExpiryPostResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).expect("JSON-RPC request");
        let id = body.get("id").cloned().unwrap_or(Value::Null);
        match body.get("method").and_then(Value::as_str) {
            Some("initialize") => {
                let generation = self.initializations.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .insert_header("mcp-session-id", format!("reset-{generation}"))
                    .set_body_json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
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
                if attempt == 1 {
                    ResponseTemplate::new(404)
                } else {
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "application/json")
                        .set_body_json(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "tools": [] }
                        }))
                }
            }
            method => panic!("unexpected request method: {method:?}"),
        }
    }
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
            Some("tools/list") => ResponseTemplate::new(404),
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
async fn shutdown_accepts_delete_404_as_idempotent_success() {
    let server = MockServer::start().await;
    mount_initialize(&server, Some("already-deleted-session")).await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(header("mcp-session-id", "already-deleted-session"))
        .and(header("mcp-protocol-version", "2025-11-25"))
        .respond_with(ResponseTemplate::new(404))
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
        .expect("an already absent session is cleaned up");
}

#[tokio::test]
async fn shutdown_reports_delete_status_as_cleanup_error() {
    let server = MockServer::start().await;
    mount_initialize(&server, Some("cleanup-status")).await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("connect");
    assert_eq!(
        connection.shutdown().await,
        Err(McpError::HttpCleanupStatus(500))
    );
}

#[tokio::test]
async fn shutdown_reports_delete_timeout_as_cleanup_error() {
    let server = MockServer::start().await;
    mount_initialize(&server, Some("cleanup-timeout")).await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(204).set_delay(Duration::from_millis(100)))
        .mount(&server)
        .await;

    let mut cleanup_spec = spec(&server);
    cleanup_spec.timeouts = McpTimeouts {
        call_default: Duration::from_millis(10),
        ..McpTimeouts::default()
    };
    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(cleanup_spec, support::authorized_connect_context())
        .await
        .expect("connect");
    assert_eq!(
        connection.shutdown().await,
        Err(McpError::HttpCleanupTimeout)
    );
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

#[tokio::test]
async fn session_404_rebuilds_but_does_not_replay_a_tool_call() {
    let server = MockServer::start().await;
    let responder = ExpiringToolCallResponder::default();
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
    let error = connection
        .call_tool("unsafe", json!({}))
        .await
        .expect_err("a committed tool call must not be replayed");

    assert_eq!(error, McpError::SessionExpired);
    assert_eq!(responder.initializations.load(Ordering::SeqCst), 2);
    assert_eq!(responder.tool_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn calls_arriving_during_rebuild_wait_for_the_same_new_generation() {
    let server = MockServer::start().await;
    let responder = DelayedRebuildResponder::default();
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
    let first_connection = Arc::clone(&connection);
    let first = tokio::spawn(async move { first_connection.list_tools().await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while responder.initializations.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("rebuild starts");

    let second_connection = Arc::clone(&connection);
    let second = tokio::spawn(async move { second_connection.list_tools().await });
    let (first, second) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(first, second)
    })
    .await
    .expect("both calls finish after rebuilding");

    assert!(first.expect("first task").expect("first call").is_empty());
    assert!(second
        .expect("second task")
        .expect("second call")
        .is_empty());
    assert_eq!(responder.initializations.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn read_only_call_with_stale_generation_snapshot_retries_on_the_new_generation() {
    let server = MockServer::start().await;
    let responder = StaleGenerationResponder::new("tools/list");
    Mock::given(method("POST"))
        .respond_with(responder.clone())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;

    let mut stale_spec = spec(&server);
    stale_spec.reconnect = ReconnectPolicy {
        max_attempts: 1,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(1),
        backoff_jitter: 0.0,
        ..ReconnectPolicy::default()
    };
    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(stale_spec, support::authorized_connect_context())
        .await
        .expect("first generation connects");
    let stale_connection = Arc::clone(&connection);
    let stale = tokio::spawn(async move { stale_connection.list_tools().await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while responder.stale_requests.load(Ordering::SeqCst) < 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the stale request reaches the old generation");

    assert!(connection
        .list_tools()
        .await
        .expect("the request that expires the session retries")
        .is_empty());
    assert!(stale
        .await
        .expect("stale request task")
        .expect("stale read-only request retries")
        .is_empty());
    assert_eq!(responder.initializations.load(Ordering::SeqCst), 2);
    assert_eq!(responder.stale_requests.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn non_idempotent_call_with_stale_generation_snapshot_is_not_replayed() {
    let server = MockServer::start().await;
    let responder = StaleGenerationResponder::new("tools/call");
    Mock::given(method("POST"))
        .respond_with(responder.clone())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;

    let mut stale_spec = spec(&server);
    stale_spec.reconnect = ReconnectPolicy {
        max_attempts: 1,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(1),
        backoff_jitter: 0.0,
        ..ReconnectPolicy::default()
    };
    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(stale_spec, support::authorized_connect_context())
        .await
        .expect("first generation connects");
    let stale_connection = Arc::clone(&connection);
    let stale = tokio::spawn(async move { stale_connection.call_tool("unsafe", json!({})).await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while responder.stale_requests.load(Ordering::SeqCst) < 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the tool call reaches the old generation");

    assert!(connection
        .list_tools()
        .await
        .expect("the request that expires the session retries")
        .is_empty());
    assert_eq!(
        stale.await.expect("stale request task"),
        Err(McpError::SessionExpired)
    );
    assert_eq!(responder.initializations.load(Ordering::SeqCst), 2);
    assert_eq!(responder.stale_requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn shutdown_during_rebuild_leaves_the_connection_closed() {
    let server = MockServer::start().await;
    let responder = DelayedRebuildResponder::default();
    Mock::given(method("POST"))
        .respond_with(responder.clone())
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;

    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(spec(&server), support::authorized_connect_context())
        .await
        .expect("first generation connects");
    let first_connection = Arc::clone(&connection);
    let first = tokio::spawn(async move { first_connection.list_tools().await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while responder.initializations.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("rebuild starts");

    connection.shutdown().await.expect("shutdown");
    let error = connection
        .list_tools()
        .await
        .expect_err("shutdown is terminal");
    assert!(matches!(error, McpError::Connection(message) if message.contains("closed")));
    let _ = first.await;
}

#[tokio::test]
async fn failed_rebuild_is_published_to_following_calls() {
    let server = MockServer::start().await;
    let responder = FailedRebuildResponder::default();
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
    let first = connection
        .list_tools()
        .await
        .expect_err("replacement initialize fails");
    let second = connection
        .list_tools()
        .await
        .expect_err("following calls observe the same failed state");

    assert_eq!(first, second);
    assert_eq!(responder.initializations.load(Ordering::SeqCst), 2);
}

#[derive(Clone, Default)]
struct FailedRebuildResponder {
    initializations: Arc<AtomicUsize>,
}

impl Respond for FailedRebuildResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).expect("JSON-RPC request");
        let id = body.get("id").cloned().unwrap_or(Value::Null);
        match body.get("method").and_then(Value::as_str) {
            Some("initialize") => {
                let generation = self.initializations.fetch_add(1, Ordering::SeqCst);
                if generation == 0 {
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "application/json")
                        .insert_header("mcp-session-id", "failed-old")
                        .set_body_json(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "protocolVersion": "2025-11-25",
                                "capabilities": { "tools": {} },
                                "serverInfo": { "name": "fixture", "version": "0.1.0" }
                            }
                        }))
                } else {
                    ResponseTemplate::new(500)
                }
            }
            Some("notifications/initialized") => ResponseTemplate::new(202),
            Some("tools/list") => ResponseTemplate::new(404),
            method => panic!("unexpected request method: {method:?}"),
        }
    }
}

#[derive(Clone, Default)]
struct DelayedRebuildResponder {
    initializations: Arc<AtomicUsize>,
    old_lists: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct StaleGenerationResponder {
    stale_method: &'static str,
    initializations: Arc<AtomicUsize>,
    stale_requests: Arc<AtomicUsize>,
    list_requests: Arc<AtomicUsize>,
}

impl StaleGenerationResponder {
    fn new(stale_method: &'static str) -> Self {
        Self {
            stale_method,
            initializations: Arc::new(AtomicUsize::new(0)),
            stale_requests: Arc::new(AtomicUsize::new(0)),
            list_requests: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Respond for StaleGenerationResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).expect("JSON-RPC request");
        let id = body.get("id").cloned().unwrap_or(Value::Null);
        let method = body.get("method").and_then(Value::as_str);
        match method {
            Some("initialize") => {
                let generation = self.initializations.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .insert_header(
                        "mcp-session-id",
                        if generation == 0 {
                            "stale-old"
                        } else {
                            "stale-new"
                        },
                    )
                    .set_body_json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "protocolVersion": "2025-11-25",
                            "capabilities": { "tools": {} },
                            "serverInfo": { "name": "fixture", "version": "0.1.0" }
                        }
                    }))
            }
            Some("notifications/initialized") => ResponseTemplate::new(202),
            Some("tools/list") if self.stale_method == "tools/list" => {
                let attempt = self.stale_requests.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "application/json")
                        .set_body_json(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "tools": [] }
                        }))
                        .set_delay(Duration::from_millis(250))
                } else if attempt == 1 {
                    ResponseTemplate::new(404)
                } else {
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "application/json")
                        .set_body_json(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "tools": [] }
                        }))
                }
            }
            Some("tools/list") => {
                let attempt = self.list_requests.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    ResponseTemplate::new(404)
                } else {
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "application/json")
                        .set_body_json(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "tools": [] }
                        }))
                }
            }
            Some("tools/call") if self.stale_method == "tools/call" => {
                self.stale_requests.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .set_body_json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{ "type": "text", "text": "must not be observed" }],
                            "isError": false
                        }
                    }))
                    .set_delay(Duration::from_millis(250))
            }
            method => panic!("unexpected request method: {method:?}"),
        }
    }
}

impl Respond for DelayedRebuildResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).expect("JSON-RPC request");
        let id = body.get("id").cloned().unwrap_or(Value::Null);
        match body.get("method").and_then(Value::as_str) {
            Some("initialize") => {
                let generation = self.initializations.fetch_add(1, Ordering::SeqCst);
                let response = ResponseTemplate::new(200)
                    .insert_header("content-type", "application/json")
                    .insert_header(
                        "mcp-session-id",
                        if generation == 0 {
                            "delayed-old"
                        } else {
                            "delayed-new"
                        },
                    )
                    .set_body_json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "protocolVersion": "2025-11-25",
                            "capabilities": { "tools": {} },
                            "serverInfo": { "name": "fixture", "version": "0.1.0" }
                        }
                    }));
                if generation == 0 {
                    response
                } else {
                    response.set_delay(Duration::from_millis(100))
                }
            }
            Some("notifications/initialized") => ResponseTemplate::new(202),
            Some("tools/list") => {
                let old_session = request
                    .headers
                    .get("mcp-session-id")
                    .and_then(|value| value.to_str().ok())
                    == Some("delayed-old");
                if old_session && self.old_lists.fetch_add(1, Ordering::SeqCst) == 0 {
                    ResponseTemplate::new(404)
                } else {
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "application/json")
                        .set_body_json(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "tools": [] }
                        }))
                }
            }
            method => panic!("unexpected request method: {method:?}"),
        }
    }
}

#[derive(Clone, Default)]
struct ExpiringToolCallResponder {
    initializations: Arc<AtomicUsize>,
    tool_calls: Arc<AtomicUsize>,
}

impl Respond for ExpiringToolCallResponder {
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
                            "unsafe-old"
                        } else {
                            "unsafe-new"
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
            Some("tools/call") => {
                let attempt = self.tool_calls.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    ResponseTemplate::new(404)
                } else {
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "application/json")
                        .set_body_json(json!({
                            "jsonrpc": "2.0",
                            "id": 2,
                            "result": {
                                "content": [{ "type": "text", "text": "replayed" }],
                                "isError": false
                            }
                        }))
                }
            }
            method => panic!("unexpected request method: {method:?}"),
        }
    }
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

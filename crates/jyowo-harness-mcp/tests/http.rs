#![cfg(feature = "http")]

#[cfg(feature = "oauth")]
use std::sync::Mutex;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

#[cfg(feature = "oauth")]
use harness_contracts::{Event, McpOAuthRefreshOutcome, McpOAuthRefreshPhase};
use harness_contracts::{McpServerId, McpServerSource};
use harness_mcp::{
    DirectElicitationHandler, HttpTransport, McpClient, McpClientAuth, McpConnectContext, McpError,
    McpServerSpec, TransportChoice, MCP_ELICITATION_REQUIRED_CODE,
};
#[cfg(feature = "oauth")]
use harness_mcp::{McpEventSink, McpMetric, McpMetricOutcome, McpMetricsSink};
use serde_json::json;
#[cfg(feature = "oauth")]
use wiremock::Request;
use wiremock::{
    matchers::{body_partial_json, header, method},
    Mock, MockServer, ResponseTemplate,
};

mod support;

async fn mount_get_unsupported(server: &MockServer) {
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .mount(server)
        .await;
}

#[tokio::test]
async fn http_transport_posts_jsonrpc_with_headers_and_auth() {
    let server = MockServer::start().await;
    mount_get_unsupported(&server).await;
    Mock::given(method("POST"))
        .and(header("x-mcp-client", "jyowo"))
        .and(header("authorization", "Bearer token"))
        .and(body_partial_json(json!({ "method": "initialize" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "fixture", "version": "0.1.0" }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            json!({ "method": "notifications/initialized" }),
        ))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "tools/list" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "tools": [
                    { "name": "search", "description": "Search docs", "inputSchema": { "type": "object" } }
                ]
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "tools/call" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 3,
            "result": { "content": [{ "type": "text", "text": "found" }], "isError": false }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut headers = BTreeMap::new();
    headers.insert("x-mcp-client".to_owned(), "jyowo".to_owned());
    let mut spec = McpServerSpec::new(
        McpServerId("http".into()),
        "http fixture",
        TransportChoice::Http {
            url: server.uri(),
            headers,
        },
        McpServerSource::Workspace,
    );
    spec.auth = McpClientAuth::Bearer("token".into());

    let connection = McpClient::new(std::sync::Arc::new(HttpTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("http connects");

    let tools = connection.list_tools().await.expect("tools list");
    assert_eq!(tools[0].name, "search");

    let result = connection
        .call_tool("search", json!({ "q": "mcp" }))
        .await
        .expect("tool call");
    assert_eq!(result, harness_mcp::McpToolResult::text("found"));
}

#[tokio::test]
async fn http_transport_decodes_url_elicitation_required_error() {
    let server = MockServer::start().await;
    mount_get_unsupported(&server).await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "initialize" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "fixture", "version": "0.1.0" }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            json!({ "method": "notifications/initialized" }),
        ))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "tools/call" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "error": {
                "code": MCP_ELICITATION_REQUIRED_CODE,
                "message": "more input required",
                "data": {
                    "elicitations": [{
                        "mode": "url",
                        "message": "authorize access",
                        "elicitationId": "auth-42",
                        "url": "https://example.com/authorize"
                    }]
                }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let spec = McpServerSpec::new(
        McpServerId("http".into()),
        "http fixture",
        TransportChoice::Http {
            url: server.uri(),
            headers: BTreeMap::new(),
        },
        McpServerSource::Workspace,
    );

    let connection = McpClient::new(std::sync::Arc::new(HttpTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("http connects");
    let error = connection
        .call_tool("search", json!({ "q": "mcp" }))
        .await
        .expect_err("elicitation required");

    let McpError::UrlElicitationRequired(elicitations) = error else {
        panic!("expected structured URL elicitation error");
    };
    assert_eq!(elicitations.len(), 1);
    assert_eq!(elicitations[0].elicitation_id, "auth-42");
    assert_eq!(elicitations[0].url, "https://example.com/authorize");
}

#[tokio::test]
async fn http_transport_does_not_retry_legacy_form_elicitation_errors() {
    let server = MockServer::start().await;
    mount_get_unsupported(&server).await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "initialize" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "fixture", "version": "0.1.0" }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(
            json!({ "method": "notifications/initialized" }),
        ))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "search",
                "arguments": { "q": "mcp" }
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "error": {
                "code": MCP_ELICITATION_REQUIRED_CODE,
                "message": "more input required",
                "data": {
                    "server_id": "http",
                    "request_id": "legacy-42",
                    "subject": "credentials",
                    "schema": {
                        "type": "object",
                        "properties": { "token": { "type": "string" } }
                    }
                }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "search",
                "arguments": { "q": "mcp", "token": "resolved" }
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 3,
            "result": {
                "content": [{ "type": "text", "text": "found" }],
                "isError": false
            }
        })))
        .expect(0)
        .mount(&server)
        .await;

    let spec = McpServerSpec::new(
        McpServerId("http".into()),
        "http fixture",
        TransportChoice::Http {
            url: server.uri(),
            headers: BTreeMap::new(),
        },
        McpServerSource::Workspace,
    );
    let handler_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&handler_calls);
    let handler = DirectElicitationHandler::new(move |_request| {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Ok(json!({ "token": "resolved" })) }
    });
    let connection = McpClient::new(Arc::new(HttpTransport::new()))
        .connect_with_context(
            spec,
            support::with_transport_authorization(
                McpConnectContext::default().with_elicitation_handler(Arc::new(handler)),
            ),
        )
        .await
        .expect("http connects");

    let error = connection
        .call_tool("search", json!({ "q": "mcp" }))
        .await
        .expect_err("legacy form elicitation errors are not retried");
    assert_eq!(handler_calls.load(Ordering::SeqCst), 0);
    assert!(matches!(error, McpError::Protocol(message) if message.contains("-32042")));
}

#[tokio::test]
#[cfg(feature = "oauth")]
async fn http_transport_refreshes_oauth_and_posts_with_access_token() {
    let token_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({
            "grant_type": "refresh_token",
            "client_id": "client",
            "client_secret": "secret",
            "refresh_token": "refresh"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "oauth-access",
            "token_type": "Bearer",
            "expires_in": 300,
            "refresh_token": "refresh"
        })))
        .expect(1)
        .mount(&token_server)
        .await;

    let server = MockServer::start().await;
    mount_get_unsupported(&server).await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer oauth-access"))
        .and(body_partial_json(json!({ "method": "initialize" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "fixture", "version": "0.1.0" }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer oauth-access"))
        .and(body_partial_json(
            json!({ "method": "notifications/initialized" }),
        ))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;

    let mut spec = McpServerSpec::new(
        McpServerId("http-oauth".into()),
        "http oauth fixture",
        TransportChoice::Http {
            url: server.uri(),
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

    McpClient::new(std::sync::Arc::new(HttpTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("http oauth connects");
}

#[tokio::test]
#[cfg(feature = "oauth")]
async fn http_transport_refreshes_oauth_before_short_lived_token_expires() {
    let token_server = MockServer::start().await;
    let refreshes = Arc::new(AtomicUsize::new(0));
    let token_refreshes = refreshes.clone();
    Mock::given(method("POST"))
        .respond_with(move |_: &Request| {
            let token_number = token_refreshes.fetch_add(1, Ordering::SeqCst) + 1;
            ResponseTemplate::new(200).set_body_json(json!({
                "access_token": format!("short-{token_number}"),
                "token_type": "Bearer",
                "expires_in": 0,
                "refresh_token": "refresh"
            }))
        })
        .expect(4)
        .mount(&token_server)
        .await;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer short-1"))
        .and(body_partial_json(json!({ "method": "initialize" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "fixture", "version": "0.1.0" }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer short-2"))
        .and(body_partial_json(
            json!({ "method": "notifications/initialized" }),
        ))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "tools/list" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": { "tools": [] }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(405))
        .expect(1)
        .mount(&server)
        .await;

    let mut spec = McpServerSpec::new(
        McpServerId("http-oauth-expiry".into()),
        "http oauth expiry fixture",
        TransportChoice::Http {
            url: server.uri(),
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

    let metrics = Arc::new(CollectingMetrics::default());
    let connection = McpClient::new(std::sync::Arc::new(HttpTransport::with_metrics_sink(
        metrics.clone(),
    )))
    .connect_with_context(spec, support::authorized_connect_context())
    .await
    .expect("http oauth connects");
    let tools = connection.list_tools().await.expect("tools list");

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while refreshes.load(Ordering::SeqCst) < 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("GET channel authorization refresh completes");

    assert!(tools.is_empty());
    assert_eq!(refreshes.load(Ordering::SeqCst), 4);
    assert_eq!(
        metrics.oauth_refresh_outcomes(),
        vec![
            McpMetricOutcome::Success,
            McpMetricOutcome::Success,
            McpMetricOutcome::Success,
            McpMetricOutcome::Success
        ]
    );
}

#[tokio::test]
#[cfg(feature = "oauth")]
async fn http_transport_retries_once_after_unauthorized_when_oauth_refresh_succeeds() {
    let token_server = MockServer::start().await;
    let refreshes = Arc::new(AtomicUsize::new(0));
    let token_refreshes = refreshes.clone();
    Mock::given(method("POST"))
        .respond_with(move |_: &Request| {
            let token_number = token_refreshes.fetch_add(1, Ordering::SeqCst) + 1;
            ResponseTemplate::new(200).set_body_json(json!({
                "access_token": format!("retry-{token_number}"),
                "token_type": "Bearer",
                "expires_in": 300,
                "refresh_token": "refresh"
            }))
        })
        .expect(2)
        .mount(&token_server)
        .await;

    let server = MockServer::start().await;
    mount_get_unsupported(&server).await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer retry-1"))
        .and(body_partial_json(json!({ "method": "initialize" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "fixture", "version": "0.1.0" }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer retry-1"))
        .and(body_partial_json(
            json!({ "method": "notifications/initialized" }),
        ))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer retry-1"))
        .and(body_partial_json(json!({ "method": "tools/list" })))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer retry-2"))
        .and(body_partial_json(json!({ "method": "tools/list" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "tools": [
                    { "name": "search", "description": "Search docs", "inputSchema": { "type": "object" } }
                ]
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut spec = McpServerSpec::new(
        McpServerId("http-oauth-retry".into()),
        "http oauth retry fixture",
        TransportChoice::Http {
            url: server.uri(),
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

    let metrics = Arc::new(CollectingMetrics::default());
    let connection = McpClient::new(std::sync::Arc::new(HttpTransport::with_metrics_sink(
        metrics.clone(),
    )))
    .connect_with_context(spec, support::authorized_connect_context())
    .await
    .expect("http oauth connects");
    let tools = connection.list_tools().await.expect("tools list");

    assert_eq!(tools[0].name, "search");
    assert_eq!(refreshes.load(Ordering::SeqCst), 2);
}

#[tokio::test]
#[cfg(feature = "oauth")]
async fn http_transport_emits_oauth_refresh_lifecycle_events() {
    let token_server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "fresh",
            "token_type": "Bearer",
            "expires_in": 300,
            "refresh_token": "refresh"
        })))
        .expect(1)
        .mount(&token_server)
        .await;

    let server = MockServer::start().await;
    mount_get_unsupported(&server).await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer fresh"))
        .and(body_partial_json(json!({ "method": "initialize" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "fixture", "version": "0.1.0" }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer fresh"))
        .and(body_partial_json(
            json!({ "method": "notifications/initialized" }),
        ))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;

    let mut spec = McpServerSpec::new(
        McpServerId("http-oauth-events".into()),
        "http oauth event fixture",
        TransportChoice::Http {
            url: server.uri(),
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

    let events = Arc::new(CollectingEvents::default());
    McpClient::new(std::sync::Arc::new(HttpTransport::new()))
        .connect_with_context(
            spec,
            support::with_transport_authorization(
                McpConnectContext::default().with_event_sink(events.clone()),
            ),
        )
        .await
        .expect("http oauth connects");

    let oauth_events = events.oauth_refresh_events();
    assert_eq!(oauth_events.len(), 2);
    assert!(matches!(
        oauth_events.first(),
        Some(event)
            if event.server_id == McpServerId("http-oauth-events".into())
                && event.transport == "http"
                && event.phase == McpOAuthRefreshPhase::Started
                && event.outcome == McpOAuthRefreshOutcome::Started
    ));
    assert!(matches!(
        oauth_events.get(1),
        Some(event)
            if event.server_id == McpServerId("http-oauth-events".into())
                && event.transport == "http"
                && event.phase == McpOAuthRefreshPhase::Completed
                && event.outcome == McpOAuthRefreshOutcome::Success
                && event.reason.is_none()
    ));
}

#[tokio::test]
#[cfg(feature = "oauth")]
async fn http_transport_fails_closed_when_unauthorized_oauth_refresh_fails() {
    let token_server = MockServer::start().await;
    let refreshes = Arc::new(AtomicUsize::new(0));
    let token_refreshes = refreshes.clone();
    Mock::given(method("POST"))
        .respond_with(move |_: &Request| {
            let token_number = token_refreshes.fetch_add(1, Ordering::SeqCst) + 1;
            if token_number == 1 {
                return ResponseTemplate::new(200).set_body_json(json!({
                    "access_token": "stale",
                    "token_type": "Bearer",
                    "expires_in": 300,
                    "refresh_token": "refresh"
                }));
            }
            ResponseTemplate::new(400).set_body_json(json!({
                "error": "invalid_grant"
            }))
        })
        .expect(2)
        .mount(&token_server)
        .await;

    let server = MockServer::start().await;
    mount_get_unsupported(&server).await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer stale"))
        .and(body_partial_json(json!({ "method": "initialize" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "fixture", "version": "0.1.0" }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer stale"))
        .and(body_partial_json(
            json!({ "method": "notifications/initialized" }),
        ))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer stale"))
        .and(body_partial_json(json!({ "method": "tools/list" })))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;

    let mut spec = McpServerSpec::new(
        McpServerId("http-oauth-refresh-failure".into()),
        "http oauth refresh failure fixture",
        TransportChoice::Http {
            url: server.uri(),
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

    let metrics = Arc::new(CollectingMetrics::default());
    let connection = McpClient::new(std::sync::Arc::new(HttpTransport::with_metrics_sink(
        metrics.clone(),
    )))
    .connect_with_context(spec, support::authorized_connect_context())
    .await
    .expect("http oauth connects");
    let error = connection
        .list_tools()
        .await
        .expect_err("refresh failure fails closed");

    assert!(matches!(
        error,
        McpError::OAuth(message) if message == "invalid_grant"
    ));
    assert_eq!(refreshes.load(Ordering::SeqCst), 2);
    assert_eq!(
        metrics.oauth_refresh_outcomes(),
        vec![McpMetricOutcome::Success, McpMetricOutcome::Error]
    );
}

#[derive(Default)]
#[cfg(feature = "oauth")]
struct CollectingMetrics {
    metrics: Mutex<Vec<McpMetric>>,
}

#[cfg(feature = "oauth")]
impl CollectingMetrics {
    fn oauth_refresh_outcomes(&self) -> Vec<McpMetricOutcome> {
        self.metrics
            .lock()
            .expect("metrics lock")
            .iter()
            .filter_map(|metric| match metric {
                McpMetric::OAuthRefresh { outcome } => Some(*outcome),
                _ => None,
            })
            .collect()
    }
}

#[cfg(feature = "oauth")]
impl McpMetricsSink for CollectingMetrics {
    fn record(&self, metric: McpMetric) {
        self.metrics.lock().expect("metrics lock").push(metric);
    }
}

#[derive(Default)]
#[cfg(feature = "oauth")]
struct CollectingEvents {
    events: Mutex<Vec<Event>>,
}

#[cfg(feature = "oauth")]
impl CollectingEvents {
    fn oauth_refresh_events(&self) -> Vec<harness_contracts::McpOAuthRefreshEvent> {
        self.events
            .lock()
            .expect("events lock")
            .iter()
            .filter_map(|event| match event {
                Event::McpOAuthRefresh(event) => Some(event.clone()),
                _ => None,
            })
            .collect()
    }
}

#[cfg(feature = "oauth")]
impl McpEventSink for CollectingEvents {
    fn emit(&self, event: Event) {
        self.events.lock().expect("events lock").push(event);
    }
}

#[tokio::test]
async fn http_transport_rejects_xaa_without_request_signer() {
    let server = MockServer::start().await;
    let mut spec = McpServerSpec::new(
        McpServerId("http-xaa".into()),
        "http xaa fixture",
        TransportChoice::Http {
            url: server.uri(),
            headers: BTreeMap::new(),
        },
        McpServerSource::Workspace,
    );
    spec.auth = McpClientAuth::Xaa {
        parent_session: harness_contracts::SessionId::from_u128(7),
        scopes: vec!["tools".into()],
    };

    let error = match McpClient::new(std::sync::Arc::new(HttpTransport::new()))
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

#[tokio::test]
async fn http_transport_can_disable_redirect_following() {
    let target = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "redirect-target", "version": "0.1.0" }
            }
        })))
        .expect(0)
        .mount(&target)
        .await;

    let redirector = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", target.uri()))
        .expect(1)
        .mount(&redirector)
        .await;

    let spec = McpServerSpec::new(
        McpServerId("http-redirect".into()),
        "http redirect fixture",
        TransportChoice::Http {
            url: redirector.uri(),
            headers: BTreeMap::new(),
        },
        McpServerSource::Workspace,
    );

    let error = match McpClient::new(Arc::new(HttpTransport::new().with_redirects_disabled()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
    {
        Ok(_) => panic!("redirects must fail closed when disabled"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        McpError::Transport(_) | McpError::InvalidResponse(_)
    ));
}

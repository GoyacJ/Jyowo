//! HTTP broker integration tests.
//!
//! Uses local loopback HTTP servers to exercise the production broker code path.
//! No mock broker — the reqwest client and validation logic are tested against
//! real TCP listeners.

use std::io::Write;
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use harness_contracts::{
    HostRule, NetworkAccess, NoopRedactor, RunId, SessionId, TenantId, ToolUseId,
};
use harness_execution::ReqwestToolNetworkBroker;
use harness_tool::{
    AuthorizedNetworkPermit, HttpMethod, NetworkBrokerPreflightRequest, ToolHttpJsonRequest,
    ToolNetworkBrokerCap, ToolNetworkBrokerPreflightCap,
};

/// Starts a tiny HTTP server on localhost that responds 200 OK with "hello".
/// Returns the bound port.
fn start_hello_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                );
            }
        }
    });
    port
}

fn broker() -> ReqwestToolNetworkBroker {
    ReqwestToolNetworkBroker::new(
        Duration::from_secs(10),
        1_048_576, // 1 MiB
        Arc::new(NoopRedactor),
    )
    .expect("broker construction")
}

fn permit_for(host: &str, port: u16) -> AuthorizedNetworkPermit {
    let host_rule = HostRule {
        pattern: host.to_owned(),
        ports: Some(vec![port]),
    };
    AuthorizedNetworkPermit::for_test(
        "test-tool",
        ToolUseId::new(),
        SessionId::new(),
        RunId::new(),
        vec![host_rule],
    )
}

fn json_request(method: HttpMethod, url: &str) -> ToolHttpJsonRequest {
    ToolHttpJsonRequest {
        method,
        url: url.to_owned(),
        timeout: Duration::from_secs(10),
        max_response_bytes: 1_048_576,
        ..ToolHttpJsonRequest::default()
    }
}

// ── Tests ──

#[tokio::test]
async fn approved_host_succeeds_against_loopback_server() {
    let port = start_hello_server();
    let broker = broker();
    let permit = permit_for("127.0.0.1", port);
    let request = json_request(HttpMethod::Get, &format!("http://127.0.0.1:{port}/"));

    let response = broker
        .execute_json(&permit, request)
        .await
        .expect("approved loopback host should succeed");
    assert_eq!(response.status, 200);
}

#[tokio::test]
async fn different_host_fails_before_sending() {
    let port = start_hello_server();
    let broker = broker();
    // Permit approves 127.0.0.1 but the request goes to localhost (different host).
    let permit = permit_for("127.0.0.1", port);
    let request = json_request(HttpMethod::Get, &format!("http://localhost:{port}/"));

    let err = broker
        .execute_json(&permit, request)
        .await
        .expect_err("unapproved host must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("localhost") || msg.contains("allowlist"),
        "error must name the rejected host: {msg}"
    );
}

#[tokio::test]
async fn redirect_to_unapproved_host_fails() {
    // Start a server that redirects.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let redirect_port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: http://evil.example.com:9999/\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                );
            }
        }
    });

    let broker = broker();
    let permit = permit_for("127.0.0.1", redirect_port);
    let request = json_request(
        HttpMethod::Get,
        &format!("http://127.0.0.1:{redirect_port}/"),
    );

    // The broker disables auto-redirect. The request succeeds (302 is returned
    // as-is), and the caller/tool must handle the redirect. The broker must not
    // follow redirects to unapproved hosts.
    let response = broker
        .execute_json(&permit, request)
        .await
        .expect("request should succeed; redirect is returned as 302, not followed");
    assert_eq!(response.status, 302);
}

#[tokio::test]
async fn public_raw_ip_is_denied() {
    let broker = broker();
    // Permit approves "api.example.com" (a hostname), not raw IPs.
    let host_rule = HostRule {
        pattern: "api.example.com".to_owned(),
        ports: None,
    };
    let permit = AuthorizedNetworkPermit::for_test(
        "test-tool",
        ToolUseId::new(),
        SessionId::new(),
        RunId::new(),
        vec![host_rule],
    );

    // Request to a public IP (8.8.8.8).
    let request = json_request(HttpMethod::Get, "http://8.8.8.8:80/");

    let err = broker
        .execute_json(&permit, request)
        .await
        .expect_err("public raw IP must be denied");
    let msg = err.to_string();
    assert!(
        msg.contains("8.8.8.8") || msg.contains("raw IP") || msg.contains("denied"),
        "error must reject raw IP: {msg}"
    );
}

#[tokio::test]
async fn loopback_ip_succeeds_only_when_explicitly_approved() {
    let port = start_hello_server();
    let broker = broker();

    // Permit approves "localhost" but NOT "127.0.0.1".
    let host_rule = HostRule {
        pattern: "localhost".to_owned(),
        ports: Some(vec![port]),
    };
    let permit = AuthorizedNetworkPermit::for_test(
        "test-tool",
        ToolUseId::new(),
        SessionId::new(),
        RunId::new(),
        vec![host_rule],
    );

    // Request to 127.0.0.1 — not explicitly in the allowlist.
    let request = json_request(HttpMethod::Get, &format!("http://127.0.0.1:{port}/"));

    let err = broker
        .execute_json(&permit, request)
        .await
        .expect_err("loopback IP without explicit approval must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("127.0.0.1")
            || msg.contains("loopback")
            || msg.contains("explicitly approved"),
        "error must explain loopback IP is not explicitly approved: {msg}"
    );
}

#[tokio::test]
async fn network_access_unrestricted_fails_preflight() {
    let broker = broker();
    let request = NetworkBrokerPreflightRequest {
        tool_name: "test-tool".to_owned(),
        tool_use_id: ToolUseId::new(),
        network_access: NetworkAccess::Unrestricted,
        action_plan_hash: Default::default(),
    };

    let err = broker
        .preflight_network_request(&request)
        .await
        .expect_err("unrestricted network must fail broker preflight");
    let msg = err.to_string();
    assert!(
        msg.contains("unrestricted") || msg.contains("v1"),
        "error must reject unrestricted network: {msg}"
    );
}

#[tokio::test]
async fn permit_fields_bind_to_correct_identity() {
    let port = start_hello_server();
    let broker = broker();
    let permit = permit_for("127.0.0.1", port);

    assert_eq!(permit.tool_name(), "test-tool");
    assert_eq!(permit.approved_hosts().len(), 1);
    assert_eq!(permit.approved_hosts()[0].pattern, "127.0.0.1");

    let request = json_request(HttpMethod::Get, &format!("http://127.0.0.1:{port}/"));
    let response = broker.execute_json(&permit, request).await.unwrap();
    assert_eq!(response.status, 200);
}

#[tokio::test]
async fn empty_allowlist_fails_preflight() {
    let broker = broker();
    let request = NetworkBrokerPreflightRequest {
        tool_name: "test-tool".to_owned(),
        tool_use_id: ToolUseId::new(),
        network_access: NetworkAccess::AllowList(vec![]),
        action_plan_hash: Default::default(),
    };

    let err = broker
        .preflight_network_request(&request)
        .await
        .expect_err("empty allowlist must fail preflight");
    let msg = err.to_string();
    assert!(
        msg.contains("empty") || msg.contains("allowlist"),
        "error must reject empty allowlist: {msg}"
    );
}

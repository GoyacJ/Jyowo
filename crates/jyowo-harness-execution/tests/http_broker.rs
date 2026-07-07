//! HTTP broker integration tests.
//!
//! Uses local loopback HTTP servers to exercise the production broker code path.
//! No mock broker — the reqwest client and validation logic are tested against
//! real TCP listeners.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use harness_contracts::{
    ActionPlanId, DecisionScope, HostRule, NetworkAccess, NoopRedactor, PermissionActorSource,
    PermissionReview, PermissionSubject, ResourceLimits, RunId, SandboxMode, SandboxPolicy,
    SandboxScope, SessionId, Severity, TenantId, ToolActionPlan, ToolExecutionChannel, ToolUseId,
    WorkspaceAccess,
};
use harness_execution::{AuthorizationTicketClaims, ReqwestToolNetworkBroker, TicketLedger};
use harness_tool::{
    canonical_action_plan_hash, AuthorizedNetworkPermit, AuthorizedToolInput, HttpMethod,
    NetworkBrokerPreflightRequest, ToolHttpJsonRequest, ToolNetworkBrokerCap,
    ToolNetworkBrokerPreflightCap,
};

/// Starts a tiny HTTP server on localhost that responds 200 OK with "hello".
/// Returns the bound port.
fn start_hello_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                );
            }
        }
    });
    port
}

fn start_slow_response_server(delay: Duration) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            thread::sleep(delay);
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    port
}

fn start_streaming_over_cap_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nexceeds");
            let _ = stream.flush();
            thread::sleep(Duration::from_secs(2));
            let _ = stream.write_all(b"-later");
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

fn broker_for_ledger(ledger: &TicketLedger) -> ReqwestToolNetworkBroker {
    ReqwestToolNetworkBroker::new_with_ticket_authority(
        Duration::from_secs(10),
        1_048_576, // 1 MiB
        Arc::new(NoopRedactor),
        ledger.authority_key(),
    )
    .expect("broker construction")
}

fn permit_for(ledger: &TicketLedger, host: &str, port: u16) -> AuthorizedNetworkPermit {
    permit_for_rules(
        ledger,
        vec![HostRule {
            pattern: host.to_owned(),
            ports: Some(vec![port]),
        }],
    )
}

fn permit_for_rules(
    ledger: &TicketLedger,
    approved_hosts: Vec<HostRule>,
) -> AuthorizedNetworkPermit {
    let tool_use_id = ToolUseId::new();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let network_access = NetworkAccess::AllowList(approved_hosts);
    let plan_hash: harness_contracts::ActionPlanHash = Default::default();
    let mut action_plan = ToolActionPlan {
        plan_id: ActionPlanId::new(),
        tool_use_id,
        tool_name: "test-tool".to_owned(),
        actor_source: PermissionActorSource::ParentRun,
        subject: PermissionSubject::ToolInvocation {
            tool: "test-tool".to_owned(),
            input: serde_json::json!({}),
        },
        scope: DecisionScope::ToolName("test-tool".to_owned()),
        severity: Severity::Info,
        resources: Vec::new(),
        sandbox_policy: SandboxPolicy {
            mode: SandboxMode::None,
            scope: SandboxScope::WorkspaceOnly,
            network: network_access.clone(),
            resource_limits: ResourceLimits {
                max_memory_bytes: None,
                max_cpu_cores: None,
                max_pids: None,
                max_wall_clock_ms: None,
                max_open_files: None,
            },
            denied_host_paths: Vec::new(),
        },
        workspace_access: WorkspaceAccess::None,
        network_access,
        execution_channel: ToolExecutionChannel::HttpBroker,
        review: PermissionReview::default(),
        plan_hash: plan_hash.clone(),
        created_at: chrono::Utc::now(),
    };
    action_plan.plan_hash = canonical_action_plan_hash(&action_plan);
    let claims = AuthorizationTicketClaims {
        tenant_id: TenantId::SINGLE,
        session_id,
        run_id,
        tool_use_id,
        tool_name: "test-tool".to_owned(),
        action_plan_hash: action_plan.plan_hash.clone(),
    };
    let ticket = ledger
        .mint(claims.clone(), chrono::Utc::now())
        .and_then(|ticket| ledger.consume(ticket.id, &claims, chrono::Utc::now()))
        .expect("test ticket should mint and consume");
    AuthorizedToolInput::new(serde_json::json!({}), action_plan, ticket)
        .expect("authorized input should be valid")
        .network_permit()
        .expect("allowlist action plan should create permit")
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
    let ledger = TicketLedger::default();
    let broker = broker_for_ledger(&ledger);
    let permit = permit_for(&ledger, "127.0.0.1", port);
    let request = json_request(HttpMethod::Get, &format!("http://127.0.0.1:{port}/"));

    let response = broker
        .execute_json(&permit, request)
        .await
        .expect("approved loopback host should succeed");
    assert_eq!(response.status, 200);
}

#[tokio::test]
async fn broker_rejects_permit_not_minted_by_its_ticket_authority() {
    let port = start_hello_server();
    let broker = broker();
    let foreign_ledger = TicketLedger::default();
    let permit = permit_for(&foreign_ledger, "127.0.0.1", port);
    let request = json_request(HttpMethod::Get, &format!("http://127.0.0.1:{port}/"));

    let err = broker
        .execute_json(&permit, request)
        .await
        .expect_err("broker must reject permits not minted by its ticket authority");

    assert!(
        err.to_string().contains("ticket proof"),
        "error should identify invalid ticket proof: {err}"
    );
}

#[tokio::test]
async fn different_host_fails_before_sending() {
    let port = start_hello_server();
    let ledger = TicketLedger::default();
    let broker = broker_for_ledger(&ledger);
    // Permit approves 127.0.0.1 but the request goes to localhost (different host).
    let permit = permit_for(&ledger, "127.0.0.1", port);
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
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: http://evil.example.com:9999/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        }
    });

    let ledger = TicketLedger::default();
    let broker = broker_for_ledger(&ledger);
    let permit = permit_for(&ledger, "127.0.0.1", redirect_port);
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
    let ledger = TicketLedger::default();
    let broker = broker_for_ledger(&ledger);
    // Permit approves "api.example.com" (a hostname), not raw IPs.
    let host_rule = HostRule {
        pattern: "api.example.com".to_owned(),
        ports: None,
    };
    let permit = permit_for_rules(&ledger, vec![host_rule]);

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
    let ledger = TicketLedger::default();
    let broker = broker_for_ledger(&ledger);

    // Permit approves "localhost" but NOT "127.0.0.1".
    let host_rule = HostRule {
        pattern: "localhost".to_owned(),
        ports: Some(vec![port]),
    };
    let permit = permit_for_rules(&ledger, vec![host_rule]);

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
async fn url_with_userinfo_is_rejected_before_dispatch() {
    let port = start_hello_server();
    let ledger = TicketLedger::default();
    let broker = broker_for_ledger(&ledger);
    let permit = permit_for(&ledger, "127.0.0.1", port);
    let request = json_request(
        HttpMethod::Get,
        &format!("http://user:password@127.0.0.1:{port}/"),
    );

    let err = broker
        .execute_json(&permit, request)
        .await
        .expect_err("broker must reject URL userinfo before dispatch");
    let msg = err.to_string();
    assert!(
        msg.contains("userinfo") || msg.contains("credentials") || msg.contains("not allowed"),
        "error must reject URL credentials: {msg}"
    );
}

#[tokio::test]
async fn loopback_host_only_allowlist_does_not_approve_arbitrary_port() {
    let port = start_hello_server();
    let ledger = TicketLedger::default();
    let broker = broker_for_ledger(&ledger);
    let permit = permit_for_rules(
        &ledger,
        vec![HostRule {
            pattern: "127.0.0.1".to_owned(),
            ports: None,
        }],
    );
    let request = json_request(HttpMethod::Get, &format!("http://127.0.0.1:{port}/"));

    let err = broker
        .execute_json(&permit, request)
        .await
        .expect_err("loopback approval must include the exact port");
    let msg = err.to_string();
    assert!(
        msg.contains("127.0.0.1") && (msg.contains("port") || msg.contains("explicitly approved")),
        "error must require exact loopback host and port: {msg}"
    );
}

#[tokio::test]
async fn request_response_size_cap_is_enforced_below_global_cap() {
    let port = start_hello_server();
    let ledger = TicketLedger::default();
    let broker = broker_for_ledger(&ledger);
    let permit = permit_for(&ledger, "127.0.0.1", port);
    let mut request = json_request(HttpMethod::Get, &format!("http://127.0.0.1:{port}/"));
    request.max_response_bytes = 4;

    let err = broker
        .execute_json(&permit, request)
        .await
        .expect_err("per-request response cap must be enforced");
    assert!(
        matches!(err, harness_contracts::ToolError::ResultTooLarge { .. }),
        "error must report response size cap as ResultTooLarge, got {err:?}"
    );
}

#[tokio::test]
async fn allowlist_rules_without_explicit_ports_fail_preflight() {
    let broker = broker();
    let request = NetworkBrokerPreflightRequest {
        tool_name: "test-tool".to_owned(),
        tool_use_id: ToolUseId::new(),
        network_access: NetworkAccess::AllowList(vec![HostRule {
            pattern: "api.example.test".to_owned(),
            ports: None,
        }]),
        action_plan_hash: Default::default(),
    };

    let err = broker
        .preflight_network_request(&request)
        .await
        .expect_err("HTTP broker preflight must require explicit ports");
    let msg = err.to_string();
    assert!(
        msg.contains("port") || msg.contains("explicit"),
        "error must explain explicit ports are required: {msg}"
    );
}

#[tokio::test]
async fn per_request_timeout_is_applied() {
    let port = start_slow_response_server(Duration::from_millis(500));
    let ledger = TicketLedger::default();
    let broker = broker_for_ledger(&ledger);
    let permit = permit_for(&ledger, "127.0.0.1", port);
    let mut request = json_request(HttpMethod::Get, &format!("http://127.0.0.1:{port}/"));
    request.timeout = Duration::from_millis(25);
    let started = Instant::now();

    let err = broker
        .execute_json(&permit, request)
        .await
        .expect_err("per-request timeout must fail before the slow response arrives");

    assert!(
        started.elapsed() < Duration::from_millis(300),
        "request-specific timeout was not applied quickly enough"
    );
    assert!(
        matches!(err, harness_contracts::ToolError::Internal(_)),
        "request timeout should surface as a broker execution error: {err}"
    );
}

#[tokio::test]
async fn response_body_cap_is_enforced_while_streaming() {
    let port = start_streaming_over_cap_server();
    let ledger = TicketLedger::default();
    let broker = broker_for_ledger(&ledger);
    let permit = permit_for(&ledger, "127.0.0.1", port);
    let mut request = json_request(HttpMethod::Get, &format!("http://127.0.0.1:{port}/"));
    request.max_response_bytes = 4;
    request.timeout = Duration::from_secs(5);

    let result = tokio::time::timeout(
        Duration::from_millis(500),
        broker.execute_json(&permit, request),
    )
    .await
    .expect("body cap must trip before the server closes the connection");
    let err = result.expect_err("body larger than cap must fail");
    assert!(
        matches!(err, harness_contracts::ToolError::ResultTooLarge { .. }),
        "error must report response body cap as ResultTooLarge, got {err:?}"
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
    let ledger = TicketLedger::default();
    let broker = broker_for_ledger(&ledger);
    let permit = permit_for(&ledger, "127.0.0.1", port);

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

#![cfg(feature = "http")]

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use harness_contracts::{
    HookEventKind, HookFailureMode, InteractivityLevel, MessageRole, PermissionMode, RunId,
    TenantId, ToolUseId, TransportFailureKind, TrustLevel,
};
use harness_hook::{
    HookContext, HookDispatcher, HookEvent, HookFailureCause, HookHttpAuth, HookHttpDnsResolver,
    HookHttpSecurityPolicy, HookHttpSpec, HookMessageView, HookOutcome, HookPayload,
    HookProtocolVersion, HookRegistry, HookSessionView, HookTransport, HostAllowlist,
    HttpHookTransport, MtlsConfig, ReplayMode, SsrfGuardPolicy, ToolDescriptorView,
};
use parking_lot::Mutex;
use serde_json::json;

#[tokio::test]
async fn http_transport_posts_payload_and_parses_continue_response() {
    let server = TestHttpServer::spawn(
        200,
        r#"{"protocol_version":"v1","outcome":{"continue":null}}"#,
    );
    let transport = HttpHookTransport::new(http_spec(
        "http-ok",
        server.url(),
        HookHttpSecurityPolicy {
            allowlist: HostAllowlist::from_hosts(["127.0.0.1"]),
            ssrf_guard: SsrfGuardPolicy {
                deny_loopback: false,
                ..SsrfGuardPolicy::default()
            },
            ..HookHttpSecurityPolicy::default()
        },
    ))
    .unwrap();

    let output = transport
        .invoke(HookPayload {
            event: sample_pre_tool_use(),
            ctx: sample_context(),
        })
        .await
        .unwrap();

    assert_eq!(output, HookOutcome::Continue);
    assert_eq!(server.body(), "captured");
}

#[tokio::test]
async fn http_allowlist_miss_is_recorded_as_transport_failure() {
    let server = TestHttpServer::spawn(
        200,
        r#"{"protocol_version":"v1","outcome":{"continue":null}}"#,
    );
    let registry = HookRegistry::builder()
        .with_hook(Box::new(
            HttpHookTransport::new(http_spec(
                "http-allowlist",
                server.url(),
                HookHttpSecurityPolicy {
                    allowlist: HostAllowlist::from_hosts(["example.com"]),
                    ssrf_guard: SsrfGuardPolicy {
                        deny_loopback: false,
                        ..SsrfGuardPolicy::default()
                    },
                    ..HookHttpSecurityPolicy::default()
                },
            ))
            .unwrap(),
        ))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(sample_pre_tool_use(), sample_context())
        .await
        .unwrap();

    assert_eq!(result.final_outcome, HookOutcome::Continue);
    assert!(matches!(
        result.failures[0].cause,
        HookFailureCause::Transport {
            kind: TransportFailureKind::AllowlistMiss,
            ..
        }
    ));
}

#[tokio::test]
async fn http_default_ssrf_guard_rejects_loopback() {
    let server = TestHttpServer::spawn(
        200,
        r#"{"protocol_version":"v1","outcome":{"continue":null}}"#,
    );
    let registry = HookRegistry::builder()
        .with_hook(Box::new(
            HttpHookTransport::new(http_spec(
                "http-ssrf",
                server.url(),
                HookHttpSecurityPolicy {
                    allowlist: HostAllowlist::from_hosts(["127.0.0.1"]),
                    ..HookHttpSecurityPolicy::default()
                },
            ))
            .unwrap(),
        ))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(sample_pre_tool_use(), sample_context())
        .await
        .unwrap();

    assert_eq!(result.final_outcome, HookOutcome::Continue);
    assert!(matches!(
        result.failures[0].cause,
        HookFailureCause::Transport {
            kind: TransportFailureKind::SsrfBlocked,
            ..
        }
    ));
}

#[tokio::test]
async fn http_rejects_protocol_mismatch_and_body_too_large() {
    let mismatch = TestHttpServer::spawn(
        200,
        r#"{"protocol_version":"v2","outcome":{"continue":null}}"#,
    );
    let mismatch_result = HookDispatcher::new(
        HookRegistry::builder()
            .with_hook(Box::new(
                HttpHookTransport::new(http_spec(
                    "http-mismatch",
                    mismatch.url(),
                    local_security(1024),
                ))
                .unwrap(),
            ))
            .build()
            .unwrap()
            .snapshot(),
    )
    .dispatch(sample_pre_tool_use(), sample_context())
    .await
    .unwrap();

    assert!(matches!(
        mismatch_result.failures[0].cause,
        HookFailureCause::Transport {
            kind: TransportFailureKind::ProtocolVersionMismatch,
            ..
        }
    ));

    let too_large = TestHttpServer::spawn(
        200,
        r#"{"protocol_version":"v1","outcome":{"continue":null}}"#,
    );
    let too_large_result = HookDispatcher::new(
        HookRegistry::builder()
            .with_hook(Box::new(
                HttpHookTransport::new(http_spec("http-large", too_large.url(), local_security(8)))
                    .unwrap(),
            ))
            .build()
            .unwrap()
            .snapshot(),
    )
    .dispatch(sample_pre_tool_use(), sample_context())
    .await
    .unwrap();

    assert!(matches!(
        too_large_result.failures[0].cause,
        HookFailureCause::Transport {
            kind: TransportFailureKind::BodyTooLarge,
            ..
        }
    ));
}

#[test]
fn http_rejects_user_controlled_without_required_security_and_invalid_mtls() {
    let url = reqwest::Url::parse("http://example.com/hook").unwrap();
    let mut user_spec = http_spec("user-http", url.clone(), HookHttpSecurityPolicy::default());
    user_spec.trust = TrustLevel::UserControlled;
    assert!(HttpHookTransport::new(user_spec).is_err());

    let mut invalid_mtls = http_spec("invalid-mtls", url, HookHttpSecurityPolicy::default());
    invalid_mtls.security.mtls = Some(MtlsConfig {
        identity_pem: b"not a pem".to_vec(),
    });
    assert!(HttpHookTransport::new(invalid_mtls).is_err());
}

#[tokio::test]
async fn http_strict_ssrf_blocks_hostname_resolving_to_loopback() {
    let transport = HttpHookTransport::with_resolver(
        http_spec(
            "http-dns-loopback",
            reqwest::Url::parse("http://allowed.example/hook").unwrap(),
            strict_security(["allowed.example"]),
        ),
        Arc::new(StaticResolver::new([addr("127.0.0.1", 80)])),
    )
    .unwrap();

    let error = transport
        .invoke(HookPayload {
            event: sample_pre_tool_use(),
            ctx: sample_context(),
        })
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        harness_contracts::HookError::Transport {
            kind: TransportFailureKind::SsrfBlocked,
            ..
        }
    ));
}

#[tokio::test]
async fn http_strict_ssrf_blocks_private_link_local_and_metadata_dns_results() {
    for ip in ["10.0.0.5", "169.254.1.1", "169.254.169.254"] {
        let transport = HttpHookTransport::with_resolver(
            http_spec(
                "http-dns-private",
                reqwest::Url::parse("http://allowed.example/hook").unwrap(),
                strict_security(["allowed.example"]),
            ),
            Arc::new(StaticResolver::new([addr(ip, 80)])),
        )
        .unwrap();

        let error = transport
            .invoke(HookPayload {
                event: sample_pre_tool_use(),
                ctx: sample_context(),
            })
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            harness_contracts::HookError::Transport {
                kind: TransportFailureKind::SsrfBlocked,
                ..
            }
        ));
    }
}

#[tokio::test]
async fn http_allowlisted_public_dns_result_reaches_network_layer() {
    let mut spec = http_spec(
        "http-dns-public",
        reqwest::Url::parse("http://allowed.example/hook").unwrap(),
        strict_security(["allowed.example"]),
    );
    spec.timeout = Duration::from_millis(25);
    let transport = HttpHookTransport::with_resolver(
        spec,
        Arc::new(StaticResolver::new([addr("93.184.216.34", 80)])),
    )
    .unwrap();

    let error = transport
        .invoke(HookPayload {
            event: sample_pre_tool_use(),
            ctx: sample_context(),
        })
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        harness_contracts::HookError::Transport {
            kind: TransportFailureKind::NetworkError,
            ..
        }
    ));
}

#[test]
fn http_strict_ssrf_rejects_redirects_until_per_hop_validation_exists() {
    let mut security = strict_security(["allowed.example"]);
    security.max_redirects = 1;

    let error = match HttpHookTransport::new(http_spec(
        "http-redirect",
        reqwest::Url::parse("http://allowed.example/hook").unwrap(),
        security,
    )) {
        Ok(_) => panic!("strict SSRF must reject redirects"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("redirect"));
}

fn http_spec(
    handler_id: &str,
    url: reqwest::Url,
    security: HookHttpSecurityPolicy,
) -> HookHttpSpec {
    HookHttpSpec {
        handler_id: handler_id.to_owned(),
        interested_events: vec![HookEventKind::PreToolUse],
        failure_mode: HookFailureMode::FailOpen,
        url,
        auth: HookHttpAuth::None,
        timeout: Duration::from_secs(2),
        security,
        protocol_version: HookProtocolVersion::V1,
        trust: TrustLevel::AdminTrusted,
    }
}

fn local_security(max_body_bytes: u64) -> HookHttpSecurityPolicy {
    HookHttpSecurityPolicy {
        allowlist: HostAllowlist::from_hosts(["127.0.0.1"]),
        ssrf_guard: SsrfGuardPolicy {
            deny_loopback: false,
            ..SsrfGuardPolicy::default()
        },
        max_body_bytes,
        ..HookHttpSecurityPolicy::default()
    }
}

fn strict_security<const N: usize>(hosts: [&str; N]) -> HookHttpSecurityPolicy {
    HookHttpSecurityPolicy {
        allowlist: HostAllowlist::from_hosts(hosts),
        ..HookHttpSecurityPolicy::default()
    }
}

fn addr(ip: &str, port: u16) -> SocketAddr {
    SocketAddr::new(ip.parse::<IpAddr>().unwrap(), port)
}

struct StaticResolver {
    addrs: Vec<SocketAddr>,
}

impl StaticResolver {
    fn new<const N: usize>(addrs: [SocketAddr; N]) -> Self {
        Self {
            addrs: addrs.into_iter().collect(),
        }
    }
}

#[async_trait::async_trait]
impl HookHttpDnsResolver for StaticResolver {
    async fn resolve(
        &self,
        _host: &str,
        _port: u16,
    ) -> Result<Vec<SocketAddr>, harness_contracts::HookError> {
        Ok(self.addrs.clone())
    }
}

struct TestHttpServer {
    url: reqwest::Url,
    body_marker: Arc<Mutex<Option<String>>>,
}

impl TestHttpServer {
    fn spawn(status: u16, body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let body_marker = Arc::new(Mutex::new(None));
        let body_marker_thread = Arc::clone(&body_marker);
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            if request.contains("\"event\"") && request.contains("\"context\"") {
                *body_marker_thread.lock() = Some("captured".to_owned());
            }

            let response = format!(
                "HTTP/1.1 {status} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        Self {
            url: reqwest::Url::parse(&format!("http://{addr}/hook")).unwrap(),
            body_marker,
        }
    }

    fn url(&self) -> reqwest::Url {
        self.url.clone()
    }

    fn body(&self) -> String {
        self.body_marker.lock().clone().unwrap_or_default()
    }
}

#[derive(Debug)]
struct TestSessionView;

impl HookSessionView for TestSessionView {
    fn workspace_root(&self) -> Option<&Path> {
        Some(Path::new("/workspace"))
    }

    fn recent_messages(&self, limit: usize) -> Vec<HookMessageView> {
        vec![HookMessageView {
            role: MessageRole::User,
            text_snippet: "hello".to_owned(),
            tool_use_id: None,
        }]
        .into_iter()
        .take(limit)
        .collect()
    }

    fn permission_mode(&self) -> PermissionMode {
        PermissionMode::Default
    }

    fn redacted(&self) -> &dyn harness_contracts::Redactor {
        &harness_contracts::NoopRedactor
    }

    fn current_tool_descriptor(&self) -> Option<ToolDescriptorView> {
        None
    }
}

fn sample_pre_tool_use() -> HookEvent {
    HookEvent::PreToolUse {
        tool_use_id: ToolUseId::new(),
        tool_name: "bash".to_owned(),
        input: json!({ "command": "ls" }),
    }
}

fn sample_context() -> HookContext {
    HookContext {
        tenant_id: TenantId::SINGLE,
        session_id: harness_contracts::SessionId::new(),
        run_id: Some(RunId::new()),
        turn_index: Some(1),
        correlation_id: harness_contracts::CorrelationId::new(),
        causation_id: harness_contracts::CausationId::new(),
        trust_level: TrustLevel::AdminTrusted,
        permission_mode: PermissionMode::Default,
        interactivity: InteractivityLevel::FullyInteractive,
        at: chrono::Utc::now(),
        view: Arc::new(TestSessionView),
        upstream_outcome: None,
        replay_mode: ReplayMode::Live,
    }
}

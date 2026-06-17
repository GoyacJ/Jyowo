#![cfg(all(feature = "in-process", feature = "exec", feature = "http"))]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use async_trait::async_trait;
use harness_contracts::{
    HookError, HookEventKind, HookFailureCauseKind, HookFailureMode, InteractivityLevel,
    MessageRole, PermissionMode, RunId, TenantId, ToolUseId, TransportFailureKind, TrustLevel,
};
use harness_hook::{
    ExecHookTransport, HookContext, HookDispatcher, HookEvent, HookExecResourceLimits,
    HookExecSignalPolicy, HookExecSpec, HookFailureCause, HookHandler, HookHttpAuth,
    HookHttpSecurityPolicy, HookHttpSpec, HookMessageView, HookOutcome, HookProtocolVersion,
    HookRegistry, HookSessionView, HostAllowlist, HttpHookTransport, InProcessHookTransport,
    MtlsConfig, ReplayMode, SsrfGuardPolicy, ToolDescriptorView, WorkingDir,
};
use serde_json::json;

#[tokio::test]
async fn spike_transport_failure_matrix_matches_failure_mode_contract() {
    let open = dispatch(InProcessHookTransport::new(Arc::new(PanicHook::new(
        "panic-open",
        HookFailureMode::FailOpen,
    ))))
    .await;
    assert_continue_failure(&open, "panic-open", HookFailureMode::FailOpen);
    assert_eq!(open.failures[0].cause_kind, HookFailureCauseKind::Panicked);

    let closed = dispatch(InProcessHookTransport::new(Arc::new(PanicHook::new(
        "panic-closed",
        HookFailureMode::FailClosed,
    ))))
    .await;
    assert_eq!(
        closed.final_outcome,
        HookOutcome::Block {
            reason: "hook handler panic-closed failed".to_owned()
        }
    );
    assert_eq!(
        closed.failures[0].cause_kind,
        HookFailureCauseKind::Panicked
    );

    let nonzero = dispatch(
        ExecHookTransport::new(exec_spec(
            "exec-nonzero",
            write_script("nonzero", "#!/bin/sh\nexit 9\n"),
        ))
        .unwrap(),
    )
    .await;
    assert_continue_failure(&nonzero, "exec-nonzero", HookFailureMode::FailOpen);
    assert!(matches!(
        nonzero.failures[0].cause,
        HookFailureCause::Transport {
            kind: TransportFailureKind::NonZeroExit { code: 9 },
            ..
        }
    ));

    let mut timeout_spec = exec_spec(
        "exec-timeout",
        write_script("timeout", "#!/bin/sh\nsleep 2\nprintf '{}'\n"),
    );
    timeout_spec.timeout = Duration::from_millis(20);
    let timeout = dispatch(ExecHookTransport::new(timeout_spec).unwrap()).await;
    assert_continue_failure(&timeout, "exec-timeout", HookFailureMode::FailOpen);
    assert_eq!(
        timeout.failures[0].cause_kind,
        HookFailureCauseKind::Timeout
    );

    let http_5xx = TestHttpServer::spawn(
        500,
        r#"{"protocol_version":"v1","outcome":{"continue":null}}"#,
    );
    let http_5xx = dispatch(
        HttpHookTransport::new(http_spec("http-5xx", http_5xx.url(), local_security())).unwrap(),
    )
    .await;
    assert_continue_failure(&http_5xx, "http-5xx", HookFailureMode::FailOpen);
    assert!(matches!(
        http_5xx.failures[0].cause,
        HookFailureCause::Transport {
            kind: TransportFailureKind::NetworkError,
            ..
        }
    ));

    let ssrf = TestHttpServer::spawn(
        200,
        r#"{"protocol_version":"v1","outcome":{"continue":null}}"#,
    );
    let ssrf = dispatch(
        HttpHookTransport::new(http_spec(
            "http-ssrf",
            ssrf.url(),
            HookHttpSecurityPolicy {
                allowlist: HostAllowlist::from_hosts(["127.0.0.1"]),
                ..HookHttpSecurityPolicy::default()
            },
        ))
        .unwrap(),
    )
    .await;
    assert_continue_failure(&ssrf, "http-ssrf", HookFailureMode::FailOpen);
    assert!(matches!(
        ssrf.failures[0].cause,
        HookFailureCause::Transport {
            kind: TransportFailureKind::SsrfBlocked,
            ..
        }
    ));
}

#[test]
fn spike_http_invalid_mtls_is_rejected_before_registration() {
    let spec = HookHttpSpec {
        security: HookHttpSecurityPolicy {
            mtls: Some(MtlsConfig {
                identity_pem: b"not a pem identity".to_vec(),
            }),
            ..HookHttpSecurityPolicy::default()
        },
        ..http_spec(
            "http-invalid-mtls",
            reqwest::Url::parse("https://example.com/hook").unwrap(),
            HookHttpSecurityPolicy::default(),
        )
    };

    assert!(HttpHookTransport::new(spec).is_err());
}

#[tokio::test]
async fn spike_audit_replay_does_not_retrigger_hook_side_effects() {
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = HookRegistry::builder()
        .with_hook(Box::new(InProcessHookTransport::new(Arc::new(
            CountingHook(Arc::clone(&calls)),
        ))))
        .build()
        .unwrap();
    let dispatcher = HookDispatcher::new(registry.snapshot());

    let live = dispatcher
        .dispatch(sample_pre_tool_use(), context(ReplayMode::Live))
        .await
        .unwrap();
    let audit = dispatcher
        .dispatch(sample_pre_tool_use(), context(ReplayMode::Audit))
        .await
        .unwrap();

    assert_eq!(live.final_outcome, HookOutcome::Continue);
    assert_eq!(audit, harness_hook::DispatchResult::default());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

async fn dispatch(handler: impl HookHandler + 'static) -> harness_hook::DispatchResult {
    let registry = HookRegistry::builder()
        .with_hook(Box::new(handler))
        .build()
        .unwrap();
    HookDispatcher::new(registry.snapshot())
        .dispatch(sample_pre_tool_use(), context(ReplayMode::Live))
        .await
        .unwrap()
}

fn assert_continue_failure(
    result: &harness_hook::DispatchResult,
    handler_id: &str,
    mode: HookFailureMode,
) {
    assert_eq!(result.final_outcome, HookOutcome::Continue);
    assert_eq!(result.failures.len(), 1);
    assert_eq!(result.failures[0].handler_id, handler_id);
    assert_eq!(result.failures[0].mode, mode);
}

struct PanicHook {
    id: String,
    mode: HookFailureMode,
}

impl PanicHook {
    fn new(id: &str, mode: HookFailureMode) -> Self {
        Self {
            id: id.to_owned(),
            mode,
        }
    }
}

#[async_trait]
impl HookHandler for PanicHook {
    fn handler_id(&self) -> &str {
        &self.id
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    fn failure_mode(&self) -> HookFailureMode {
        self.mode
    }

    async fn handle(&self, _event: HookEvent, _ctx: HookContext) -> Result<HookOutcome, HookError> {
        panic!("spike hook panic")
    }
}

struct CountingHook(Arc<AtomicUsize>);

#[async_trait]
impl HookHandler for CountingHook {
    fn handler_id(&self) -> &'static str {
        "counting"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    async fn handle(&self, _event: HookEvent, _ctx: HookContext) -> Result<HookOutcome, HookError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(HookOutcome::Continue)
    }
}

fn exec_spec(handler_id: &str, command: PathBuf) -> HookExecSpec {
    HookExecSpec {
        handler_id: handler_id.to_owned(),
        interested_events: vec![HookEventKind::PreToolUse],
        failure_mode: HookFailureMode::FailOpen,
        command,
        args: Vec::new(),
        env: BTreeMap::new(),
        working_dir: WorkingDir::EphemeralTemp,
        timeout: Duration::from_secs(2),
        resource_limits: HookExecResourceLimits::default(),
        signal_policy: HookExecSignalPolicy::default(),
        protocol_version: HookProtocolVersion::V1,
        trust: TrustLevel::AdminTrusted,
    }
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

fn local_security() -> HookHttpSecurityPolicy {
    HookHttpSecurityPolicy {
        allowlist: HostAllowlist::from_hosts(["127.0.0.1"]),
        ssrf_guard: SsrfGuardPolicy {
            deny_loopback: false,
            ..SsrfGuardPolicy::default()
        },
        ..HookHttpSecurityPolicy::default()
    }
}

fn write_script(name: &str, body: &str) -> PathBuf {
    static NEXT_SCRIPT_ID: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "jyowo-harness-hook-spike-{name}-{}-{}",
        std::process::id(),
        NEXT_SCRIPT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hook.sh");
    std::fs::write(&path, body).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
    }

    path
}

struct TestHttpServer(reqwest::Url);

impl TestHttpServer {
    fn spawn(status: u16, body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            let response = format!(
                "HTTP/1.1 {status} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        Self(reqwest::Url::parse(&format!("http://{addr}/hook")).unwrap())
    }

    fn url(&self) -> reqwest::Url {
        self.0.clone()
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

fn context(replay_mode: ReplayMode) -> HookContext {
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
        replay_mode,
    }
}

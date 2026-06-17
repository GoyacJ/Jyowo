#![cfg(all(feature = "in-process", feature = "exec", feature = "http"))]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use async_trait::async_trait;
use harness_contracts::{
    HookError, HookEventKind, HookFailureMode, InteractivityLevel, MessageRole, PermissionMode,
    RunId, TenantId, ToolUseId, TrustLevel,
};
use harness_hook::{
    ExecHookTransport, HookContext, HookDispatcher, HookEvent, HookExecResourceLimits,
    HookExecSignalPolicy, HookExecSpec, HookHandler, HookHttpAuth, HookHttpSecurityPolicy,
    HookHttpSpec, HookMessageView, HookOutcome, HookPayload, HookProtocolVersion, HookRegistry,
    HookSessionView, HookTransport, HostAllowlist, HttpHookTransport, InProcessHookTransport,
    ReplayMode, SsrfGuardPolicy, ToolDescriptorView, WorkingDir,
};
use parking_lot::Mutex;
use serde_json::json;

#[tokio::test]
async fn contract_transports_share_invoke_contract() {
    let exec = ExecHookTransport::new(exec_spec(
        "exec-block",
        write_script(
            "contract-block",
            r#"#!/bin/sh
printf '{"protocol_version":"v1","outcome":{"block":{"reason":"blocked"}}}'
"#,
        ),
    ))
    .unwrap();
    let http_server = TestHttpServer::spawn(
        200,
        r#"{"protocol_version":"v1","outcome":{"block":{"reason":"blocked"}}}"#,
    );
    let http = HttpHookTransport::new(http_spec("http-block", http_server.url())).unwrap();
    let in_process = InProcessHookTransport::new(Arc::new(StaticHook {
        id: "in-process-block".to_owned(),
        outcome: HookOutcome::Block {
            reason: "blocked".to_owned(),
        },
    }));

    for transport in [
        Box::new(in_process) as Box<dyn HookTransport>,
        Box::new(exec),
        Box::new(http),
    ] {
        let outcome = transport
            .invoke(HookPayload {
                event: sample_pre_tool_use(),
                ctx: sample_context(),
            })
            .await
            .unwrap();
        assert_eq!(
            outcome,
            HookOutcome::Block {
                reason: "blocked".to_owned()
            }
        );
    }
}

#[tokio::test]
async fn contract_transports_register_and_dispatch_in_stable_order() {
    let exec = ExecHookTransport::new(exec_spec(
        "a-exec",
        write_script(
            "contract-continue",
            r#"#!/bin/sh
printf '{"protocol_version":"v1","outcome":{"continue":null}}'
"#,
        ),
    ))
    .unwrap();
    let http_server = TestHttpServer::spawn(
        200,
        r#"{"protocol_version":"v1","outcome":{"continue":null}}"#,
    );
    let http = HttpHookTransport::new(http_spec("b-http", http_server.url())).unwrap();
    let in_process = InProcessHookTransport::new(Arc::new(StaticHook {
        id: "c-in-process".to_owned(),
        outcome: HookOutcome::Continue,
    }));

    let registry = HookRegistry::builder()
        .with_hook(Box::new(http))
        .with_hook(Box::new(in_process))
        .with_hook(Box::new(exec))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(sample_pre_tool_use(), sample_context())
        .await
        .unwrap();

    let trail: Vec<_> = result
        .trail
        .iter()
        .map(|record| record.handler_id.as_str())
        .collect();
    assert_eq!(trail, vec!["a-exec", "b-http", "c-in-process"]);
    assert_eq!(result.final_outcome, HookOutcome::Continue);
    assert!(result.failures.is_empty());
}

struct StaticHook {
    id: String,
    outcome: HookOutcome,
}

#[async_trait]
impl HookHandler for StaticHook {
    fn handler_id(&self) -> &str {
        &self.id
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    async fn handle(&self, _event: HookEvent, _ctx: HookContext) -> Result<HookOutcome, HookError> {
        Ok(self.outcome.clone())
    }
}

fn exec_spec(handler_id: &str, command: PathBuf) -> HookExecSpec {
    let working_dir = WorkingDir::Pinned(command.parent().unwrap().to_path_buf());
    HookExecSpec {
        handler_id: handler_id.to_owned(),
        interested_events: vec![HookEventKind::PreToolUse],
        failure_mode: HookFailureMode::FailOpen,
        command,
        args: Vec::new(),
        env: BTreeMap::new(),
        working_dir,
        timeout: Duration::from_secs(5),
        resource_limits: HookExecResourceLimits::default(),
        signal_policy: HookExecSignalPolicy::default(),
        protocol_version: HookProtocolVersion::V1,
        trust: TrustLevel::AdminTrusted,
    }
}

fn http_spec(handler_id: &str, url: reqwest::Url) -> HookHttpSpec {
    HookHttpSpec {
        handler_id: handler_id.to_owned(),
        interested_events: vec![HookEventKind::PreToolUse],
        failure_mode: HookFailureMode::FailOpen,
        url,
        auth: HookHttpAuth::None,
        timeout: Duration::from_secs(2),
        security: HookHttpSecurityPolicy {
            allowlist: HostAllowlist::from_hosts(["127.0.0.1"]),
            ssrf_guard: SsrfGuardPolicy {
                deny_loopback: false,
                ..SsrfGuardPolicy::default()
            },
            ..HookHttpSecurityPolicy::default()
        },
        protocol_version: HookProtocolVersion::V1,
        trust: TrustLevel::AdminTrusted,
    }
}

fn write_script(name: &str, body: &str) -> PathBuf {
    static NEXT_SCRIPT_ID: AtomicU64 = AtomicU64::new(0);
    let script_id = NEXT_SCRIPT_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "jyowo-harness-hook-contract-{}-{}-{}",
        name,
        std::process::id(),
        script_id
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
}

impl Drop for TestHttpServer {
    fn drop(&mut self) {
        assert_eq!(self.body_marker.lock().as_deref(), Some("captured"));
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

#![cfg(all(feature = "in-process", feature = "exec", feature = "http"))]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_contracts::{
    HookError, HookEventKind, HookFailureMode, InteractivityLevel, MessageRole, PermissionMode,
    RunId, TenantId, ToolUseId, TrustLevel,
};
use harness_hook::{
    DispatchResult, ExecHookTransport, HookContext, HookDispatcher, HookEvent,
    HookExecResourceLimits, HookExecSignalPolicy, HookExecSpec, HookHandler, HookHttpAuth,
    HookHttpSecurityPolicy, HookHttpSpec, HookMessageView, HookOutcome, HookRegistry,
    HookSessionView, HostAllowlist, HttpHookTransport, InProcessHookTransport, ReplayMode,
    ToolDescriptorView, WorkingDir,
};
use serde_json::json;

#[tokio::test]
async fn audit_replay_skips_in_process_handler() {
    let called = Arc::new(AtomicUsize::new(0));
    let registry = HookRegistry::builder()
        .with_hook(Box::new(InProcessHookTransport::new(Arc::new(
            CountingHook {
                id: "counter".to_owned(),
                mode: HookFailureMode::FailOpen,
                called: Arc::clone(&called),
            },
        ))))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(sample_pre_tool_use(), audit_context())
        .await
        .unwrap();

    assert_eq!(result, DispatchResult::default());
    assert_eq!(called.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn audit_replay_skips_exec_and_http_transports_even_when_fail_closed() {
    let mut exec_spec = exec_spec("exec-should-not-run", PathBuf::from("/no/such/hook"));
    exec_spec.failure_mode = HookFailureMode::FailClosed;
    let mut http_spec = http_spec("http-should-not-run");
    http_spec.failure_mode = HookFailureMode::FailClosed;

    let registry = HookRegistry::builder()
        .with_hook(Box::new(ExecHookTransport::new(exec_spec).unwrap()))
        .with_hook(Box::new(HttpHookTransport::new(http_spec).unwrap()))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(sample_pre_tool_use(), audit_context())
        .await
        .unwrap();

    assert_eq!(result, DispatchResult::default());
}

#[tokio::test]
async fn live_then_audit_does_not_replay_side_effects() {
    let called = Arc::new(AtomicUsize::new(0));
    let registry = HookRegistry::builder()
        .with_hook(Box::new(InProcessHookTransport::new(Arc::new(
            CountingHook {
                id: "counter".to_owned(),
                mode: HookFailureMode::FailOpen,
                called: Arc::clone(&called),
            },
        ))))
        .build()
        .unwrap();
    let dispatcher = HookDispatcher::new(registry.snapshot());

    let live = dispatcher
        .dispatch(sample_pre_tool_use(), live_context())
        .await
        .unwrap();
    assert_eq!(live.final_outcome, HookOutcome::Continue);
    assert_eq!(live.trail.len(), 1);
    assert_eq!(called.load(Ordering::SeqCst), 1);

    let audit = dispatcher
        .dispatch(sample_pre_tool_use(), audit_context())
        .await
        .unwrap();
    assert_eq!(audit, DispatchResult::default());
    assert_eq!(called.load(Ordering::SeqCst), 1);
}

struct CountingHook {
    id: String,
    mode: HookFailureMode,
    called: Arc<AtomicUsize>,
}

#[async_trait]
impl HookHandler for CountingHook {
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
        self.called.fetch_add(1, Ordering::SeqCst);
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
        timeout: Duration::from_millis(50),
        resource_limits: HookExecResourceLimits::default(),
        signal_policy: HookExecSignalPolicy::default(),
        protocol_version: harness_hook::HookProtocolVersion::V1,
        trust: TrustLevel::AdminTrusted,
    }
}

fn http_spec(handler_id: &str) -> HookHttpSpec {
    HookHttpSpec {
        handler_id: handler_id.to_owned(),
        interested_events: vec![HookEventKind::PreToolUse],
        failure_mode: HookFailureMode::FailOpen,
        url: reqwest::Url::parse("http://example.com/hook").unwrap(),
        auth: HookHttpAuth::None,
        timeout: Duration::from_millis(50),
        security: HookHttpSecurityPolicy {
            allowlist: HostAllowlist::from_hosts(["example.com"]),
            ..HookHttpSecurityPolicy::default()
        },
        protocol_version: harness_hook::HookProtocolVersion::V1,
        trust: TrustLevel::AdminTrusted,
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

fn live_context() -> HookContext {
    context(ReplayMode::Live)
}

fn audit_context() -> HookContext {
    context(ReplayMode::Audit)
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

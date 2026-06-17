#![cfg(feature = "in-process")]

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    HookError, HookEventKind, HookFailureMode, InteractivityLevel, MessageRole, PermissionMode,
    RunId, TenantId, ToolUseId, TrustLevel,
};
use harness_hook::{
    HookContext, HookDispatcher, HookEvent, HookFailureCause, HookHandler, HookMessageView,
    HookOutcome, HookPayload, HookRegistry, HookSessionView, HookTransport, InProcessHookTransport,
    ReplayMode, ToolDescriptorView,
};
use serde_json::json;

#[tokio::test]
async fn in_process_transport_invokes_wrapped_handler() {
    let called = Arc::new(AtomicUsize::new(0));
    let transport = InProcessHookTransport::new(Arc::new(CountingHook {
        id: "audit".to_owned(),
        events: vec![HookEventKind::PreToolUse],
        priority: 10,
        mode: HookFailureMode::FailClosed,
        called: Arc::clone(&called),
    }));

    assert_eq!(transport.handler_id(), "audit");
    assert_eq!(transport.interested_events(), &[HookEventKind::PreToolUse]);
    assert_eq!(transport.priority(), 10);
    assert_eq!(transport.failure_mode(), HookFailureMode::FailClosed);

    let output = transport
        .invoke(HookPayload {
            event: sample_pre_tool_use(),
            ctx: sample_context(),
        })
        .await
        .unwrap();

    assert_eq!(output, HookOutcome::Continue);
    assert_eq!(called.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn in_process_transport_registers_as_hook_handler() {
    let called = Arc::new(AtomicUsize::new(0));
    let transport = InProcessHookTransport::new(Arc::new(CountingHook {
        id: "registered".to_owned(),
        events: vec![HookEventKind::PreToolUse],
        priority: 0,
        mode: HookFailureMode::FailOpen,
        called: Arc::clone(&called),
    }));
    let registry = HookRegistry::builder()
        .with_hook(Box::new(transport))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(sample_pre_tool_use(), sample_context())
        .await
        .unwrap();

    assert_eq!(called.load(Ordering::SeqCst), 1);
    assert_eq!(result.final_outcome, HookOutcome::Continue);
    assert_eq!(result.trail.len(), 1);
    assert_eq!(result.trail[0].handler_id, "registered");
}

#[tokio::test]
async fn in_process_transport_errors_follow_dispatch_failure_mode() {
    let fail_open = HookRegistry::builder()
        .with_hook(Box::new(InProcessHookTransport::new(Arc::new(ErrorHook {
            id: "open".to_owned(),
            mode: HookFailureMode::FailOpen,
        }))))
        .build()
        .unwrap();
    let open_result = HookDispatcher::new(fail_open.snapshot())
        .dispatch(sample_pre_tool_use(), sample_context())
        .await
        .unwrap();

    assert_eq!(open_result.final_outcome, HookOutcome::Continue);
    assert_eq!(open_result.failures.len(), 1);
    assert!(matches!(
        open_result.failures[0].cause,
        HookFailureCause::Panicked { .. }
    ));

    let fail_closed = HookRegistry::builder()
        .with_hook(Box::new(InProcessHookTransport::new(Arc::new(ErrorHook {
            id: "closed".to_owned(),
            mode: HookFailureMode::FailClosed,
        }))))
        .build()
        .unwrap();
    let closed_result = HookDispatcher::new(fail_closed.snapshot())
        .dispatch(sample_pre_tool_use(), sample_context())
        .await
        .unwrap();

    assert!(matches!(
        closed_result.final_outcome,
        HookOutcome::Block { ref reason } if reason.contains("closed")
    ));
    assert_eq!(closed_result.failures.len(), 1);
}

#[tokio::test]
async fn in_process_transport_panics_are_captured_by_dispatcher() {
    let registry = HookRegistry::builder()
        .with_hook(Box::new(InProcessHookTransport::new(Arc::new(PanicHook {
            id: "panic".to_owned(),
        }))))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(sample_pre_tool_use(), sample_context())
        .await
        .unwrap();

    assert_eq!(result.final_outcome, HookOutcome::Continue);
    assert_eq!(result.failures.len(), 1);
    assert!(matches!(
        result.failures[0].cause,
        HookFailureCause::Panicked { ref snippet } if snippet.contains("in-process panic")
    ));
}

#[test]
fn in_process_transport_keeps_hook_dependency_boundary() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();

    assert!(!manifest.contains("jyowo-harness-tool"));
    assert!(!manifest.contains("jyowo-harness-session"));
    assert!(!manifest.contains("jyowo-harness-journal"));
    assert!(!manifest.contains("jyowo-harness-observability"));
    assert!(!manifest.contains("jyowo-harness-engine"));
}

struct CountingHook {
    id: String,
    events: Vec<HookEventKind>,
    priority: i32,
    mode: HookFailureMode,
    called: Arc<AtomicUsize>,
}

#[async_trait]
impl HookHandler for CountingHook {
    fn handler_id(&self) -> &str {
        &self.id
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &self.events
    }

    fn priority(&self) -> i32 {
        self.priority
    }

    fn failure_mode(&self) -> HookFailureMode {
        self.mode
    }

    async fn handle(&self, _event: HookEvent, _ctx: HookContext) -> Result<HookOutcome, HookError> {
        self.called.fetch_add(1, Ordering::SeqCst);
        Ok(HookOutcome::Continue)
    }
}

struct ErrorHook {
    id: String,
    mode: HookFailureMode,
}

#[async_trait]
impl HookHandler for ErrorHook {
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
        Err(HookError::Message("handler failed".to_owned()))
    }
}

struct PanicHook {
    id: String,
}

#[async_trait]
impl HookHandler for PanicHook {
    fn handler_id(&self) -> &str {
        &self.id
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    async fn handle(&self, _event: HookEvent, _ctx: HookContext) -> Result<HookOutcome, HookError> {
        panic!("in-process panic");
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

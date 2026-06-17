use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    HookError, HookEventKind, HookFailureMode, HookOutcomeDiscriminant, InconsistentReason,
    InteractivityLevel, MessageRole, PermissionMode, RunId, TenantId, ToolUseId, TrustLevel,
};
use harness_hook::{
    ContextPatch, ContextPatchRole, DispatchResult, HookContext, HookDispatcher, HookEvent,
    HookFailureCause, HookHandler, HookMessageView, HookOutcome, HookRegistry, HookSessionView,
    PreToolUseOutcome, RegistrationError, ReplayMode, ToolDescriptorView,
};
use serde_json::{json, Value};
use tokio::sync::Mutex;

#[test]
fn registry_snapshots_are_immutable_and_sorted() {
    let registry = HookRegistry::builder()
        .with_hook(Box::new(RecordingHook::new(
            "b",
            &[HookEventKind::PreToolUse],
            10,
            HookOutcome::Continue,
        )))
        .with_hook(Box::new(RecordingHook::new(
            "a",
            &[HookEventKind::PreToolUse],
            10,
            HookOutcome::Continue,
        )))
        .build()
        .unwrap();

    let snapshot = registry.snapshot();
    registry
        .register(Box::new(RecordingHook::new(
            "c",
            &[HookEventKind::PreToolUse],
            20,
            HookOutcome::Continue,
        )))
        .unwrap();

    let old_ids: Vec<_> = snapshot
        .handlers_for(HookEventKind::PreToolUse)
        .iter()
        .map(|handler| handler.handler_id().to_owned())
        .collect();
    assert_eq!(old_ids, vec!["a", "b"]);

    let new_ids: Vec<_> = registry
        .snapshot()
        .handlers_for(HookEventKind::PreToolUse)
        .iter()
        .map(|handler| handler.handler_id().to_owned())
        .collect();
    assert_eq!(new_ids, vec!["c", "a", "b"]);
}

#[test]
fn registry_rejects_duplicate_ids_and_invalid_handlers() {
    let registry = HookRegistry::builder()
        .with_hook(Box::new(RecordingHook::new(
            "audit",
            &[HookEventKind::PreToolUse],
            0,
            HookOutcome::Continue,
        )))
        .build()
        .unwrap();

    let duplicate = registry.register(Box::new(RecordingHook::new(
        "audit",
        &[HookEventKind::PostToolUse],
        0,
        HookOutcome::Continue,
    )));
    assert!(matches!(duplicate, Err(RegistrationError::Duplicate(_))));

    let empty_id = HookRegistry::builder()
        .with_hook(Box::new(RecordingHook::new(
            "",
            &[HookEventKind::PreToolUse],
            0,
            HookOutcome::Continue,
        )))
        .build();
    assert!(matches!(
        empty_id,
        Err(RegistrationError::InvalidHandler(_))
    ));

    let no_events = HookRegistry::builder()
        .with_hook(Box::new(RecordingHook::new(
            "empty",
            &[],
            0,
            HookOutcome::Continue,
        )))
        .build();
    assert!(matches!(
        no_events,
        Err(RegistrationError::InvalidHandler(_))
    ));
}

#[tokio::test]
async fn dispatcher_routes_by_event_and_skips_handlers_in_audit_replay() {
    let called = Arc::new(AtomicUsize::new(0));
    let registry = HookRegistry::builder()
        .with_hook(Box::new(CountingHook {
            id: "pre".to_owned(),
            events: vec![HookEventKind::PreToolUse],
            called: Arc::clone(&called),
        }))
        .with_hook(Box::new(CountingHook {
            id: "post".to_owned(),
            events: vec![HookEventKind::PostToolUse],
            called: Arc::clone(&called),
        }))
        .build()
        .unwrap();
    let dispatcher = HookDispatcher::new(registry.snapshot());

    let result = dispatcher
        .dispatch(
            sample_pre_tool_use(json!({ "command": "ls" })),
            sample_context(),
        )
        .await
        .unwrap();
    assert_eq!(called.load(Ordering::SeqCst), 1);
    assert_eq!(result.final_outcome, HookOutcome::Continue);
    assert_eq!(result.trail.len(), 1);
    assert_eq!(result.trail[0].handler_id, "pre");

    let mut audit_ctx = sample_context();
    audit_ctx.replay_mode = ReplayMode::Audit;
    let audit = dispatcher
        .dispatch(sample_pre_tool_use(json!({ "command": "ls" })), audit_ctx)
        .await
        .unwrap();
    assert_eq!(called.load(Ordering::SeqCst), 1);
    assert_eq!(audit, DispatchResult::default());
}

#[tokio::test]
async fn dispatcher_merges_pre_tool_use_outcomes_transactionally() {
    let seen_by_second = Arc::new(Mutex::new(Vec::new()));
    let registry = HookRegistry::builder()
        .with_hook(Box::new(RecordingHook::new(
            "rewrite",
            &[HookEventKind::PreToolUse],
            30,
            HookOutcome::PreToolUse(PreToolUseOutcome {
                rewrite_input: Some(json!({ "command": "ls -la" })),
                ..PreToolUseOutcome::default()
            }),
        )))
        .with_hook(Box::new(InspectingHook {
            id: "inspect".to_owned(),
            priority: 20,
            seen_inputs: Arc::clone(&seen_by_second),
            outcome: HookOutcome::PreToolUse(PreToolUseOutcome {
                override_permission: Some(harness_contracts::Decision::AllowOnce),
                additional_context: Some(ContextPatch {
                    role: ContextPatchRole::UserSuffix,
                    content: "used safe listing".to_owned(),
                    apply_to_next_turn_only: true,
                }),
                ..PreToolUseOutcome::default()
            }),
        }))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(
            sample_pre_tool_use(json!({ "command": "ls" })),
            sample_context(),
        )
        .await
        .unwrap();

    assert_eq!(
        seen_by_second.lock().await.as_slice(),
        &[json!({ "command": "ls -la" })]
    );
    assert_eq!(
        result.final_outcome,
        HookOutcome::PreToolUse(PreToolUseOutcome {
            rewrite_input: Some(json!({ "command": "ls -la" })),
            override_permission: Some(harness_contracts::Decision::AllowOnce),
            additional_context: Some(ContextPatch {
                role: ContextPatchRole::UserSuffix,
                content: "used safe listing".to_owned(),
                apply_to_next_turn_only: true,
            }),
            block: None,
        })
    );
    assert!(result.failures.is_empty());
}

#[tokio::test]
async fn dispatcher_rolls_back_pre_tool_use_outputs_on_fail_open_failure() {
    let registry = HookRegistry::builder()
        .with_hook(Box::new(RecordingHook::new(
            "rewrite",
            &[HookEventKind::PreToolUse],
            20,
            HookOutcome::PreToolUse(PreToolUseOutcome {
                rewrite_input: Some(json!({ "command": "safe" })),
                ..PreToolUseOutcome::default()
            }),
        )))
        .with_hook(Box::new(RecordingHook::new(
            "bad",
            &[HookEventKind::PreToolUse],
            10,
            HookOutcome::PreToolUse(PreToolUseOutcome {
                rewrite_input: Some(json!({ "command": "unsafe" })),
                block: Some("invalid".to_owned()),
                ..PreToolUseOutcome::default()
            }),
        )))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(
            sample_pre_tool_use(json!({ "command": "raw" })),
            sample_context(),
        )
        .await
        .unwrap();

    assert_eq!(result.final_outcome, HookOutcome::Continue);
    assert_eq!(result.failures.len(), 1);
    assert!(matches!(
        result.failures[0].cause,
        HookFailureCause::Inconsistent {
            reason: InconsistentReason::PreToolUseBlockExclusive
        }
    ));
}

#[tokio::test]
async fn dispatcher_fail_closed_failure_blocks_and_rolls_back() {
    let registry = HookRegistry::builder()
        .with_hook(Box::new(RecordingHook::new(
            "rewrite",
            &[HookEventKind::PreToolUse],
            20,
            HookOutcome::PreToolUse(PreToolUseOutcome {
                rewrite_input: Some(json!({ "command": "safe" })),
                ..PreToolUseOutcome::default()
            }),
        )))
        .with_hook(Box::new(FailClosedHook {
            inner: RecordingHook::new(
                "deny-on-error",
                &[HookEventKind::PreToolUse],
                10,
                HookOutcome::RewriteInput(json!({ "unsupported": true })),
            ),
        }))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(
            sample_pre_tool_use(json!({ "command": "raw" })),
            sample_context(),
        )
        .await
        .unwrap();

    assert!(matches!(
        result.final_outcome,
        HookOutcome::Block { ref reason } if reason.contains("deny-on-error")
    ));
    assert_eq!(result.failures.len(), 1);
    assert!(matches!(
        result.failures[0].cause,
        HookFailureCause::Unsupported {
            kind: HookOutcomeDiscriminant::RewriteInput
        }
    ));
}

#[tokio::test]
async fn dispatcher_short_circuits_pre_tool_use_block() {
    let called = Arc::new(AtomicUsize::new(0));
    let registry = HookRegistry::builder()
        .with_hook(Box::new(RecordingHook::new(
            "blocker",
            &[HookEventKind::PreToolUse],
            20,
            HookOutcome::PreToolUse(PreToolUseOutcome {
                block: Some("blocked".to_owned()),
                ..PreToolUseOutcome::default()
            }),
        )))
        .with_hook(Box::new(CountingHook {
            id: "late".to_owned(),
            events: vec![HookEventKind::PreToolUse],
            called,
        }))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(
            sample_pre_tool_use(json!({ "command": "rm -rf /" })),
            sample_context(),
        )
        .await
        .unwrap();

    assert_eq!(
        result.final_outcome,
        HookOutcome::Block {
            reason: "blocked".to_owned()
        }
    );
    assert_eq!(result.trail.len(), 1);
}

#[tokio::test]
async fn dispatcher_resolves_same_priority_permission_conflicts_with_deny_winning() {
    let registry = HookRegistry::builder()
        .with_hook(Box::new(RecordingHook::new(
            "allow",
            &[HookEventKind::PreToolUse],
            10,
            HookOutcome::PreToolUse(PreToolUseOutcome {
                override_permission: Some(harness_contracts::Decision::AllowOnce),
                ..PreToolUseOutcome::default()
            }),
        )))
        .with_hook(Box::new(RecordingHook::new(
            "deny",
            &[HookEventKind::PreToolUse],
            10,
            HookOutcome::PreToolUse(PreToolUseOutcome {
                override_permission: Some(harness_contracts::Decision::DenyOnce),
                ..PreToolUseOutcome::default()
            }),
        )))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(
            sample_pre_tool_use(json!({ "command": "ls" })),
            sample_context(),
        )
        .await
        .unwrap();

    assert_eq!(
        result.final_outcome,
        HookOutcome::PreToolUse(PreToolUseOutcome {
            override_permission: Some(harness_contracts::Decision::DenyOnce),
            ..PreToolUseOutcome::default()
        })
    );
}

#[tokio::test]
async fn dispatcher_catches_handler_errors_and_panics() {
    let error_registry = HookRegistry::builder()
        .with_hook(Box::new(ErrorHook {
            id: "err".to_owned(),
            mode: HookFailureMode::FailOpen,
        }))
        .build()
        .unwrap();
    let error_result = HookDispatcher::new(error_registry.snapshot())
        .dispatch(
            sample_pre_tool_use(json!({ "command": "ls" })),
            sample_context(),
        )
        .await
        .unwrap();

    assert_eq!(error_result.final_outcome, HookOutcome::Continue);
    assert_eq!(error_result.failures.len(), 1);
    assert!(matches!(
        error_result.failures[0].cause,
        HookFailureCause::Panicked { .. }
    ));

    let panic_registry = HookRegistry::builder()
        .with_hook(Box::new(PanicHook {
            id: "panic".to_owned(),
            mode: HookFailureMode::FailOpen,
        }))
        .build()
        .unwrap();

    let panic_result = HookDispatcher::new(panic_registry.snapshot())
        .dispatch(
            sample_pre_tool_use(json!({ "command": "ls" })),
            sample_context(),
        )
        .await
        .unwrap();

    assert_eq!(panic_result.final_outcome, HookOutcome::Continue);
    assert_eq!(panic_result.failures.len(), 1);
    assert!(matches!(
        panic_result.failures[0].cause,
        HookFailureCause::Panicked { .. }
    ));
}

#[tokio::test]
async fn fail_open_continues_to_later_handlers() {
    let called = Arc::new(AtomicUsize::new(0));
    let registry = HookRegistry::builder()
        .with_hook(Box::new(ErrorHook {
            id: "err".to_owned(),
            mode: HookFailureMode::FailOpen,
        }))
        .with_hook(Box::new(CountingHook {
            id: "later".to_owned(),
            events: vec![HookEventKind::PreToolUse],
            called: called.clone(),
        }))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(
            sample_pre_tool_use(json!({ "command": "ls" })),
            sample_context(),
        )
        .await
        .unwrap();

    assert_eq!(result.final_outcome, HookOutcome::Continue);
    assert_eq!(result.failures.len(), 1);
    assert_eq!(called.load(Ordering::SeqCst), 1);
    assert_eq!(result.trail.len(), 1);
    assert_eq!(result.trail[0].handler_id, "later");
}

struct RecordingHook {
    id: String,
    events: Vec<HookEventKind>,
    priority: i32,
    outcome: HookOutcome,
}

impl RecordingHook {
    fn new(id: &str, events: &[HookEventKind], priority: i32, outcome: HookOutcome) -> Self {
        Self {
            id: id.to_owned(),
            events: events.to_vec(),
            priority,
            outcome,
        }
    }
}

#[async_trait]
impl HookHandler for RecordingHook {
    fn handler_id(&self) -> &str {
        &self.id
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &self.events
    }

    fn priority(&self) -> i32 {
        self.priority
    }

    async fn handle(&self, _event: HookEvent, _ctx: HookContext) -> Result<HookOutcome, HookError> {
        Ok(self.outcome.clone())
    }
}

struct InspectingHook {
    id: String,
    priority: i32,
    seen_inputs: Arc<Mutex<Vec<Value>>>,
    outcome: HookOutcome,
}

#[async_trait]
impl HookHandler for InspectingHook {
    fn handler_id(&self) -> &str {
        &self.id
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    fn priority(&self) -> i32 {
        self.priority
    }

    async fn handle(&self, event: HookEvent, _ctx: HookContext) -> Result<HookOutcome, HookError> {
        if let HookEvent::PreToolUse { input, .. } = event {
            self.seen_inputs.lock().await.push(input);
        }
        Ok(self.outcome.clone())
    }
}

struct CountingHook {
    id: String,
    events: Vec<HookEventKind>,
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

    async fn handle(&self, _event: HookEvent, _ctx: HookContext) -> Result<HookOutcome, HookError> {
        self.called.fetch_add(1, Ordering::SeqCst);
        Ok(HookOutcome::Continue)
    }
}

struct FailClosedHook {
    inner: RecordingHook,
}

#[async_trait]
impl HookHandler for FailClosedHook {
    fn handler_id(&self) -> &str {
        self.inner.handler_id()
    }

    fn interested_events(&self) -> &[HookEventKind] {
        self.inner.interested_events()
    }

    fn priority(&self) -> i32 {
        self.inner.priority()
    }

    fn failure_mode(&self) -> HookFailureMode {
        HookFailureMode::FailClosed
    }

    async fn handle(&self, event: HookEvent, ctx: HookContext) -> Result<HookOutcome, HookError> {
        self.inner.handle(event, ctx).await
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
        Err(HookError::Message("handler error".to_owned()))
    }
}

struct PanicHook {
    id: String,
    mode: HookFailureMode,
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
        panic!("panic hook");
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

fn sample_pre_tool_use(input: Value) -> HookEvent {
    HookEvent::PreToolUse {
        tool_use_id: ToolUseId::new(),
        tool_name: "bash".to_owned(),
        input,
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

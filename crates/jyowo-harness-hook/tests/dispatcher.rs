use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    HookError, HookEventKind, HookFailureMode, InteractivityLevel, MessageRole, PermissionMode,
    RunId, TenantId, ToolUseId, TrustLevel,
};
use harness_hook::{
    HookContext, HookDispatcher, HookEvent, HookHandler, HookMessageView, HookOutcome,
    HookRegistry, HookSessionView, ReplayMode, ToolDescriptorView,
};
use serde_json::json;

#[tokio::test]
async fn fail_open_continues_to_later_handlers() {
    let registry = HookRegistry::builder()
        .with_hook(Box::new(FailingHook))
        .with_hook(Box::new(SecondHook))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(sample_pre_tool_use(), sample_context())
        .await
        .unwrap();

    assert_eq!(result.final_outcome, HookOutcome::Continue);
    assert_eq!(result.failures.len(), 1);
    assert!(result
        .trail
        .iter()
        .any(|record| record.handler_id == "second"));
}

struct FailingHook;

#[async_trait]
impl HookHandler for FailingHook {
    fn handler_id(&self) -> &str {
        "failing"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    fn failure_mode(&self) -> HookFailureMode {
        HookFailureMode::FailOpen
    }

    async fn handle(&self, _event: HookEvent, _ctx: HookContext) -> Result<HookOutcome, HookError> {
        Err(HookError::Message("boom".to_owned()))
    }
}

struct SecondHook;

#[async_trait]
impl HookHandler for SecondHook {
    fn handler_id(&self) -> &str {
        "second"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    async fn handle(&self, _event: HookEvent, _ctx: HookContext) -> Result<HookOutcome, HookError> {
        Ok(HookOutcome::Continue)
    }
}

#[derive(Debug)]
struct TestSessionView;

impl HookSessionView for TestSessionView {
    fn workspace_root(&self) -> Option<&Path> {
        Some(Path::new("/workspace"))
    }

    fn recent_messages(&self, _limit: usize) -> Vec<HookMessageView> {
        vec![HookMessageView {
            role: MessageRole::User,
            text_snippet: "hello".to_owned(),
            tool_use_id: None,
        }]
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

include!("hook_pipeline.rs");

use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn hook_permission_override_emits_hook_provenance() {
    let harness = TestHarness::new(
        vec![
            tool_call_events("Echo", json!({ "value": "hook allow" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![Box::new(AllowPermissionHook)],
        Arc::new(RecordingBroker::new(Decision::DenyOnce)),
    )
    .await;

    let events = harness.run("hook permission").await.unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::PermissionResolved(resolved)
            if resolved.decision == Decision::DenyOnce
                && matches!(
                    &resolved.decided_by,
                    harness_contracts::DecidedBy::Rule { rule_id }
                        if rule_id == "permission_authority"
                )
    )));
}

#[tokio::test]
async fn hook_permission_deny_beats_broker_allow() {
    let harness = TestHarness::new(
        vec![
            tool_call_events("Echo", json!({ "value": "hook deny" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![Box::new(DenyPermissionHook)],
        Arc::new(RecordingBroker::new(Decision::AllowOnce)),
    )
    .await;

    let events = harness.run("hook permission").await.unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::PermissionResolved(resolved)
            if resolved.decision == Decision::AllowOnce
                && matches!(
                    &resolved.decided_by,
                    harness_contracts::DecidedBy::Rule { rule_id }
                        if rule_id == "permission_authority"
                )
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
}

#[tokio::test]
async fn bypass_permission_mode_keeps_hook_deny_request_pending_for_audit() {
    let harness = TestHarness::new(
        vec![
            tool_call_events("Echo", json!({ "value": "bypass hook deny" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![Box::new(DenyPermissionHook)],
        Arc::new(RecordingBroker::new(Decision::AllowOnce)),
    )
    .await;

    let events = harness
        .run_with_permission_mode("bypass hook deny", PermissionMode::BypassPermissions)
        .await
        .unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::PermissionResolved(resolved)
            if resolved.decision == Decision::AllowOnce
                && matches!(
                    &resolved.decided_by,
                    harness_contracts::DecidedBy::Rule { rule_id }
                        if rule_id == "permission_authority"
                )
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
}

#[tokio::test]
async fn broker_deny_beats_hook_allow_in_bypass_permission_mode() {
    let harness = TestHarness::new(
        vec![
            tool_call_events("Echo", json!({ "value": "policy deny" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![Box::new(AllowPermissionHook)],
        Arc::new(RecordingBroker::new(Decision::DenyOnce)),
    )
    .await;

    let events = harness
        .run_with_permission_mode("hook allow policy deny", PermissionMode::BypassPermissions)
        .await
        .unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::PermissionResolved(resolved)
            if resolved.decision == Decision::DenyOnce
                && matches!(
                    &resolved.decided_by,
                    harness_contracts::DecidedBy::Rule { rule_id }
                        if rule_id == "permission_authority"
                )
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseFailed(_))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
}

#[tokio::test]
async fn hard_policy_deny_beats_hook_allow_in_bypass_permission_mode() {
    let harness = TestHarness::new(
        vec![
            tool_call_events("Echo", json!({ "value": "bypass hard policy deny" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![Box::new(AllowPermissionHook)],
        Arc::new(HardPolicyDenyBroker),
    )
    .await;

    let events = harness
        .run_with_permission_mode(
            "bypass hook allow hard policy deny",
            PermissionMode::BypassPermissions,
        )
        .await
        .unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::PermissionResolved(resolved)
            if resolved.decision == Decision::DenyOnce
                && matches!(
                    &resolved.decided_by,
                    harness_contracts::DecidedBy::Rule { rule_id }
                        if rule_id == "permission_authority"
                )
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseFailed(_))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
}

#[tokio::test]
async fn hard_policy_deny_prevents_reusing_previous_allow() {
    let harness = TestHarness::new(
        vec![
            tool_call_events("Echo", json!({ "value": "repeat" })),
            tool_call_events("Echo", json!({ "value": "repeat" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        Vec::new(),
        Arc::new(StatefulHardPolicyBroker::default()),
    )
    .await;

    let events = harness.run("repeat tool").await.unwrap();

    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::ToolUseCompleted(_)))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::ToolUseFailed(_)))
            .count(),
        1
    );
    assert!(events.iter().any(|event| matches!(
        event,
        Event::PermissionResolved(resolved)
            if resolved.decision == Decision::DenyOnce
                && matches!(
                    &resolved.decided_by,
                    harness_contracts::DecidedBy::Rule { rule_id }
                        if rule_id == "permission_authority"
                )
    )));
}

#[tokio::test]
async fn hard_policy_deny_beats_hook_allow_in_default_permission_mode() {
    let harness = TestHarness::new(
        vec![
            tool_call_events("Echo", json!({ "value": "hard policy deny" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![Box::new(AllowPermissionHook)],
        Arc::new(HardPolicyDenyBroker),
    )
    .await;

    let events = harness.run("hook allow hard policy deny").await.unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::PermissionResolved(resolved)
            if resolved.decision == Decision::DenyOnce
                && matches!(
                    &resolved.decided_by,
                    harness_contracts::DecidedBy::Rule { rule_id }
                        if rule_id == "permission_authority"
                )
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseFailed(_))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
}

#[tokio::test]
async fn bypass_permission_mode_allows_broker_escalation_without_hook_override() {
    let harness = TestHarness::new(
        vec![
            tool_call_events("Echo", json!({ "value": "escalate" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        Vec::new(),
        Arc::new(RecordingBroker::new(Decision::Escalate)),
    )
    .await;

    let events = harness
        .run_with_permission_mode("bypass broker escalate", PermissionMode::BypassPermissions)
        .await
        .unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::PermissionResolved(resolved)
            if resolved.decision == Decision::AllowOnce
                && matches!(
                    &resolved.decided_by,
                    harness_contracts::DecidedBy::DefaultMode
                )
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::ToolUseDenied(_))));
}

#[tokio::test]
async fn bypass_permission_mode_normalizes_hook_escalation_to_allow() {
    let harness = TestHarness::new(
        vec![
            tool_call_events("Echo", json!({ "value": "hook escalate" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![Box::new(EscalatePermissionHook)],
        Arc::new(RecordingBroker::new(Decision::AllowOnce)),
    )
    .await;

    let events = harness
        .run_with_permission_mode("bypass hook escalate", PermissionMode::BypassPermissions)
        .await
        .unwrap();
    let requested = events
        .iter()
        .find_map(|event| match event {
            Event::PermissionRequested(requested) => Some(requested),
            _ => None,
        })
        .expect("bypass mode should journal the permission request context");

    assert!(!requested.auto_resolved);
    assert!(events.iter().any(|event| matches!(
        event,
        Event::PermissionResolved(resolved)
            if resolved.request_id == requested.request_id
                && resolved.decision == Decision::AllowOnce
                && matches!(
                    &resolved.decided_by,
                    harness_contracts::DecidedBy::Rule { rule_id }
                        if rule_id == "permission_authority"
                )
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::ToolUseDenied(_))));
}

#[tokio::test]
async fn conflicting_hook_permission_overrides_emit_conflict_and_deny_wins() {
    let harness = TestHarness::new(
        vec![
            tool_call_events("Echo", json!({ "value": "conflict" })),
            text_events("done"),
        ],
        Box::new(EchoTool::new()),
        vec![Box::new(AllowPermissionHook), Box::new(DenyPermissionHook)],
        Arc::new(RecordingBroker::new(Decision::AllowOnce)),
    )
    .await;

    let events = harness.run("hook permission conflict").await.unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::PermissionResolved(resolved)
            if resolved.decision == Decision::AllowOnce
                && matches!(
                    &resolved.decided_by,
                    harness_contracts::DecidedBy::Rule { rule_id }
                        if rule_id == "permission_authority"
                )
    )));
}

struct AllowPermissionHook;

#[async_trait]
impl HookHandler for AllowPermissionHook {
    fn handler_id(&self) -> &str {
        "allow-permission"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        Ok(HookOutcome::PreToolUse(PreToolUseOutcome {
            rewrite_input: None,
            override_permission: Some(Decision::AllowOnce),
            additional_context: None,
            block: None,
        }))
    }
}

#[derive(Default)]
struct StatefulHardPolicyBroker {
    decide_count: AtomicUsize,
}

#[async_trait]
impl PermissionBroker for StatefulHardPolicyBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        self.decide_count.fetch_add(1, Ordering::SeqCst);
        Decision::AllowOnce
    }

    async fn hard_policy_denies(
        &self,
        _request: &PermissionRequest,
        _ctx: &PermissionContext,
    ) -> bool {
        self.decide_count.load(Ordering::SeqCst) > 0
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), PermissionError> {
        Ok(())
    }
}

struct HardPolicyDenyBroker;

#[async_trait]
impl PermissionBroker for HardPolicyDenyBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::AllowOnce
    }

    async fn hard_policy_denies(
        &self,
        _request: &PermissionRequest,
        _ctx: &PermissionContext,
    ) -> bool {
        true
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), PermissionError> {
        Ok(())
    }
}

struct DenyPermissionHook;

#[async_trait]
impl HookHandler for DenyPermissionHook {
    fn handler_id(&self) -> &str {
        "deny-permission"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        Ok(HookOutcome::PreToolUse(PreToolUseOutcome {
            rewrite_input: None,
            override_permission: Some(Decision::DenyOnce),
            additional_context: None,
            block: None,
        }))
    }
}

struct EscalatePermissionHook;

#[async_trait]
impl HookHandler for EscalatePermissionHook {
    fn handler_id(&self) -> &str {
        "escalate-permission"
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &[HookEventKind::PreToolUse]
    }

    async fn handle(
        &self,
        _event: HookEvent,
        _ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        Ok(HookOutcome::PreToolUse(PreToolUseOutcome {
            rewrite_input: None,
            override_permission: Some(Decision::Escalate),
            additional_context: None,
            block: None,
        }))
    }
}

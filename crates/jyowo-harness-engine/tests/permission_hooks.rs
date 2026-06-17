include!("hook_pipeline.rs");

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
            if resolved.decision == Decision::AllowOnce
                && matches!(
                    &resolved.decided_by,
                    harness_contracts::DecidedBy::Hook { handler_id }
                        if handler_id == "allow-permission"
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
            if resolved.decision == Decision::DenyOnce
                && matches!(
                    &resolved.decided_by,
                    harness_contracts::DecidedBy::Hook { handler_id }
                        if handler_id == "deny-permission"
                )
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ToolUseDenied(_))));
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::ToolUseCompleted(_))));
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
            if resolved.decision == Decision::DenyOnce
                && matches!(
                    &resolved.decided_by,
                    harness_contracts::DecidedBy::Hook { handler_id }
                        if handler_id == "deny-permission"
                )
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::HookPermissionConflict(conflict)
            if conflict.priority == 0
                && conflict.participants.len() == 2
                && conflict.winner.handler_id == "deny-permission"
                && conflict.winner.decision == Decision::DenyOnce
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

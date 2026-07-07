mod authorization_flow_support;
use authorization_flow_support::*;

use std::sync::Arc;

use chrono::Utc;
use harness_contracts::{
    CapabilityRegistry, Decision, DecisionScope, Event, PermissionMode, RuleSource, RunId,
    SessionId, TenantId, ToolCapability,
};
use harness_execution::{
    AuthorizationService, ExecutionError, ExecutionPreflightRegistry, TicketLedger,
};
use harness_permission::RuleAction;

// ── Execution channel preflight tests (Task 3) ──

#[tokio::test]
async fn http_broker_allowlist_passes_without_sandbox_preflight() {
    let sink = Arc::new(RecordingSink::default());
    let broker = Arc::new(RecordingBroker { allow: true });
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::Category("network".to_owned()),
        )
        .await,
        broker_registry(Arc::new(TestSandbox::default()), Some(broker)),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let outcome = service
        .authorize_plan(context(), http_broker_plan())
        .await
        .unwrap();

    assert_eq!(outcome.decision, Decision::AllowOnce);
    let events = sink.events();
    assert!(matches!(events[0], Event::PermissionRequested(_)));
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::AllowOnce
    ));
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightPassed(passed)
            if passed.backend_id == "http_broker"
    ));
}

#[tokio::test]
async fn process_sandbox_with_none_still_invokes_sandbox_preflight() {
    let sink = Arc::new(RecordingSink::default());
    let broker = Arc::new(RecordingBroker { allow: true });
    // Even with a broker registered, ProcessSandbox channel uses sandbox preflight.
    let service = AuthorizationService::new(
        real_authority(RuleSource::Session, RuleAction::Allow, DecisionScope::Any).await,
        broker_registry(Arc::new(TestSandbox::default()), Some(broker)),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let outcome = service
        .authorize_plan(context(), action_plan("safe", DecisionScope::Any))
        .await
        .unwrap();

    assert_eq!(outcome.decision, Decision::AllowOnce);
    let events = sink.events();
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightPassed(passed)
            if passed.backend_id == "test-sandbox"
    ));
}

#[tokio::test]
async fn http_broker_with_none_fails_because_http_requires_network() {
    let sink = Arc::new(RecordingSink::default());
    let broker = Arc::new(RecordingBroker { allow: true });
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::Category("network".to_owned()),
        )
        .await,
        broker_registry(Arc::new(TestSandbox::default()), Some(broker)),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let error = service
        .authorize_plan(context(), http_broker_none_plan())
        .await
        .unwrap_err();

    assert!(
        matches!(error, ExecutionError::SandboxPreflightFailed { ref backend_id, .. } if backend_id == "http_broker"),
        "unexpected error: {error:?}"
    );
    let events = sink.events();
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightFailed(failed)
            if failed.backend_id == "http_broker"
                && failed.reason.contains("[http_broker]")
                && failed.reason.contains("NetworkAccess::None")
    ));
}

#[tokio::test]
async fn http_broker_with_missing_broker_fails_before_ticket_mint() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::Category("network".to_owned()),
        )
        .await,
        // No broker registered
        preflight_registry(Arc::new(TestSandbox::default())),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let error = service
        .authorize_plan(context(), http_broker_plan())
        .await
        .unwrap_err();

    assert!(
        matches!(error, ExecutionError::SandboxPreflightFailed { ref backend_id, .. } if backend_id == "http_broker"),
        "unexpected error: {error:?}"
    );
    let events = sink.events();
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightFailed(failed)
            if failed.backend_id == "http_broker"
                && failed.reason.contains("not registered")
    ));
}

#[tokio::test]
async fn external_capability_missing_capability_fails_before_ticket_mint() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::ToolName("send_message".to_owned()),
        )
        .await,
        preflight_registry(Arc::new(TestSandbox::default())),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let error = service
        .authorize_plan(context(), external_capability_plan())
        .await
        .unwrap_err();

    assert!(
        matches!(error, ExecutionError::SandboxPreflightFailed { ref backend_id, .. } if backend_id.contains("external_capability")),
        "unexpected error: {error:?}"
    );
    let events = sink.events();
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightFailed(failed)
            if failed.backend_id.contains("external_capability")
                && failed.reason.contains("not registered")
    ));
}

// ── Task 8: external capability audit tests ──

#[tokio::test]
async fn external_capability_present_usermessenger_passes_without_sandbox_preflight() {
    let sink = Arc::new(RecordingSink::default());
    let mut caps = CapabilityRegistry::default();
    caps.install(
        ToolCapability::UserMessenger,
        Arc::new(StubUserMessenger) as Arc<dyn harness_contracts::UserMessengerCap>,
    );

    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::ToolName("send_message".to_owned()),
        )
        .await,
        ExecutionPreflightRegistry::new(Arc::new(TestSandbox::default()), None, Arc::new(caps)),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let outcome = service
        .authorize_plan(context(), external_capability_plan())
        .await
        .unwrap();

    assert_eq!(outcome.decision, Decision::AllowOnce);
    let events = sink.events();
    // Permission event order: requested → resolved → preflight passed
    assert!(matches!(&events[2], Event::SandboxPreflightPassed(passed)
        if passed.backend_id.contains("external_capability")
    ));
    // Verify NO SandboxPreflightFailed events were emitted for process sandbox.
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::SandboxPreflightFailed(failed)
            if failed.backend_id == "test_sandbox"
        )));
}

#[tokio::test]
async fn external_capability_passes_without_calling_sandbox_preflight() {
    // Verify the authorization service only checks ExternalCapability
    // against the capability registry, never against the process sandbox.
    let sink = Arc::new(RecordingSink::default());
    let mut caps = CapabilityRegistry::default();
    caps.install(
        ToolCapability::UserMessenger,
        Arc::new(StubUserMessenger) as Arc<dyn harness_contracts::UserMessengerCap>,
    );

    // ExternalCapability channels must never consult the process sandbox.
    // Even with a sandbox present in the registry, the authorization preflight
    // should route through external_capability check only.
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::ToolName("send_message".to_owned()),
        )
        .await,
        ExecutionPreflightRegistry::new(Arc::new(TestSandbox::default()), None, Arc::new(caps)),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let outcome = service
        .authorize_plan(context(), external_capability_plan())
        .await
        .unwrap();

    assert_eq!(outcome.decision, Decision::AllowOnce);
    // The sandbox_backend_id must be external_capability, not test_sandbox.
    assert!(
        outcome.sandbox_backend_id.contains("external_capability"),
        "backend id should indicate external capability, got: {}",
        outcome.sandbox_backend_id
    );
}

// ── Task 11: end-to-end regression tests ──

#[tokio::test]
async fn bypass_permissions_does_not_skip_sandbox_preflight() {
    // BypassPermissions skips interactive permission prompts but must NOT skip
    // process sandbox preflight. A plan with NetworkAccess::None routed through
    // a sandbox that can't enforce it must fail before minting a ticket.
    let sink = Arc::new(RecordingSink::default());
    let ledger = Arc::new(TicketLedger::default());

    // RejectingPreflightSandbox: capabilities pass, but preflight_execute always fails.
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::ToolName("Bash".to_owned()),
        )
        .await,
        preflight_registry(Arc::new(RejectingPreflightSandbox {
            backend_id: "rejecting",
            reason: "sandbox unavailable".to_owned(),
        })),
        sink.clone(),
        ledger.clone(),
    );

    let mut context = context();
    context.permission_mode = PermissionMode::BypassPermissions;
    let plan = process_sandbox_plan(); // ProcessSandbox channel with NetworkAccess::None

    let error = service
        .authorize_plan(context, plan.clone())
        .await
        .unwrap_err();

    // Must be a sandbox preflight failure, NOT a permission denial.
    assert!(
        matches!(error, ExecutionError::SandboxPreflightFailed { .. }),
        "bypass must not skip sandbox preflight: {error:?}"
    );

    // No ticket should have been minted.
    assert!(matches!(
        ledger.consume(
            harness_contracts::AuthorizationTicketId::new(),
            &harness_execution::AuthorizationTicketClaims {
                tenant_id: TenantId::SINGLE,
                session_id: SessionId::new(),
                run_id: RunId::new(),
                tool_use_id: plan.tool_use_id,
                tool_name: plan.tool_name,
                action_plan_hash: plan.plan_hash,
            },
            Utc::now(),
        ),
        Err(ExecutionError::TicketUnknown { .. })
    ));
}

#[tokio::test]
async fn bypass_permissions_does_not_skip_broker_preflight() {
    // BypassPermissions must not skip HTTP broker preflight. A plan with
    // HttpBroker channel routed through a denying broker must fail before
    // minting a ticket.
    let sink = Arc::new(RecordingSink::default());
    let ledger = Arc::new(TicketLedger::default());
    let denying_broker = Arc::new(RecordingBroker { allow: false });

    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::Category("network".to_owned()),
        )
        .await,
        broker_registry(Arc::new(TestSandbox::default()), Some(denying_broker)),
        sink.clone(),
        ledger.clone(),
    );

    let mut context = context();
    context.permission_mode = PermissionMode::BypassPermissions;
    let plan = http_broker_plan();

    let error = service
        .authorize_plan(context, plan.clone())
        .await
        .unwrap_err();

    assert!(
        matches!(error, ExecutionError::SandboxPreflightFailed { .. }),
        "bypass must not skip broker preflight: {error:?}"
    );

    // No ticket should have been minted.
    assert!(matches!(
        ledger.consume(
            harness_contracts::AuthorizationTicketId::new(),
            &harness_execution::AuthorizationTicketClaims {
                tenant_id: TenantId::SINGLE,
                session_id: SessionId::new(),
                run_id: RunId::new(),
                tool_use_id: plan.tool_use_id,
                tool_name: plan.tool_name,
                action_plan_hash: plan.plan_hash,
            },
            Utc::now(),
        ),
        Err(ExecutionError::TicketUnknown { .. })
    ));
}

#[tokio::test]
async fn process_sandbox_network_none_fails_when_backend_cannot_enforce() {
    // Regression: Bash with NetworkAccess::None must fail with a clear
    // backend-authored reason when no candidate backend can enforce it.
    let sink = Arc::new(RecordingSink::default());

    // TestSandbox reports no network policy support → preflight must fail.
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::ToolName("Bash".to_owned()),
        )
        .await,
        preflight_registry(Arc::new(TestSandbox::default())),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let plan = process_sandbox_plan();

    let error = service.authorize_plan(context(), plan).await.unwrap_err();

    let msg = error.to_string();
    assert!(
        msg.contains("sandbox") || msg.contains("network") || msg.contains("capability"),
        "error must identify the sandbox capability reason: {msg}"
    );
}

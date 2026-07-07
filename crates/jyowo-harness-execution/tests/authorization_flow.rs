use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use harness_contracts::{
    Decision, DecisionScope, Event, InteractivityLevel, NetworkAccess, PermissionConfirmation,
    PermissionMode, RuleSource, RunId, SandboxPreflightStatus, SessionId, TenantId,
};
use harness_execution::{AuthorizationService, ExecutionError, TicketLedger};
use harness_permission::{RuleAction, StreamBasedBroker, StreamBrokerConfig};
use harness_sandbox::LocalSandbox;

mod authorization_flow_support;
use authorization_flow_support::*;

#[tokio::test]
async fn authorization_service_denies_hard_policy_without_minting_ticket() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(RuleSource::Policy, RuleAction::Deny, DecisionScope::Any).await,
        preflight_registry(Arc::new(TestSandbox::default())),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let error = service
        .authorize_plan(context(), action_plan("dangerous", DecisionScope::Any))
        .await
        .unwrap_err();

    assert!(matches!(error, ExecutionError::PermissionDenied { .. }));
    let events = sink.events();
    assert!(matches!(events[0], Event::PermissionRequested(_)));
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::DenyOnce
    ));
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn authorization_service_denies_hard_policy_under_bypass_mode_without_minting_ticket() {
    let sink = Arc::new(RecordingSink::default());
    let ledger = Arc::new(TicketLedger::default());
    let service = AuthorizationService::new(
        real_authority(RuleSource::Policy, RuleAction::Deny, DecisionScope::Any).await,
        preflight_registry(Arc::new(TestSandbox::default())),
        sink.clone(),
        ledger.clone(),
    );
    let mut context = context();
    context.permission_mode = PermissionMode::BypassPermissions;
    let plan = action_plan("dangerous", DecisionScope::Any);

    let error = service
        .authorize_plan(context, plan.clone())
        .await
        .unwrap_err();

    assert!(matches!(error, ExecutionError::PermissionDenied { .. }));
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
    let events = sink.events();
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::DenyOnce
    ));
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn authorization_service_denies_dangerous_command_under_bypass_without_minting_ticket() {
    let sink = Arc::new(RecordingSink::default());
    let ledger = Arc::new(TicketLedger::default());
    let service = AuthorizationService::new(
        dangerous_command_authority().await,
        preflight_registry(Arc::new(TestSandbox::default())),
        sink.clone(),
        ledger.clone(),
    );
    let mut context = context();
    context.permission_mode = PermissionMode::BypassPermissions;
    let plan = dangerous_command_plan("rm -rf /");

    let error = service
        .authorize_plan(context, plan.clone())
        .await
        .unwrap_err();

    assert!(matches!(error, ExecutionError::PermissionDenied { .. }));
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
    let events = sink.events();
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::DenyOnce
    ));
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn authorization_service_emits_permission_then_preflight_events_without_journal_dependency() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(RuleSource::Session, RuleAction::Allow, DecisionScope::Any).await,
        preflight_registry(Arc::new(TestSandbox::default())),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let outcome = service
        .authorize_plan(context(), action_plan("safe", DecisionScope::Any))
        .await
        .unwrap();

    assert_eq!(outcome.decision, Decision::AllowOnce);
    assert_eq!(
        outcome.ticket.claims.action_plan_hash,
        outcome.action_plan_hash
    );
    let events = sink.events();
    assert!(matches!(events[0], Event::PermissionRequested(_)));
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::AllowOnce
    ));
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightPassed(preflight)
            if preflight.status == SandboxPreflightStatus::Passed
                && preflight.backend_id == "test-sandbox"
    ));
}

#[tokio::test]
async fn authorization_service_uses_sandbox_authority_for_exec_preflight() {
    let sink = Arc::new(RecordingSink::default());
    let workspace = tempfile::tempdir().unwrap();
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::ToolName("Bash".to_owned()),
        )
        .await,
        preflight_registry(Arc::new(LocalSandbox::new(workspace.path()))),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let error = service
        .authorize_plan(context(), command_plan("printf blocked"))
        .await
        .unwrap_err();

    assert!(
        matches!(error, ExecutionError::SandboxPreflightFailed { .. }),
        "unexpected error: {error:?}"
    );
    let events = sink.events();
    assert!(matches!(events[0], Event::PermissionRequested(_)));
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::AllowOnce
    ));
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightFailed(failed)
            if failed.status == SandboxPreflightStatus::Failed
                && failed.backend_id == "local"
                && failed.reason.contains("cannot enforce network policy")
    ));
}

#[tokio::test]
async fn authorization_service_uses_sandbox_authority_for_network_only_preflight() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::ToolName("mcp_transport".to_owned()),
        )
        .await,
        preflight_registry(Arc::new(NetworkCapablePreflightSandbox)),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let error = service
        .authorize_plan(context(), network_only_plan())
        .await
        .unwrap_err();

    assert!(
        matches!(error, ExecutionError::SandboxPreflightFailed { .. }),
        "unexpected error: {error:?}"
    );
    let events = sink.events();
    assert!(matches!(events[0], Event::PermissionRequested(_)));
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::AllowOnce
    ));
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightFailed(failed)
            if failed.status == SandboxPreflightStatus::Failed
                && failed.backend_id == "network-capable"
                && matches!(failed.policy.network, NetworkAccess::AllowList(_))
                && failed.policy_hash != Default::default()
                && failed.reason.contains("cannot enforce network policy")
    ));
}

#[tokio::test]
async fn authorization_service_mints_ticket_after_sandbox_preflight() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::ToolName("Bash".to_owned()),
        )
        .await,
        preflight_registry(Arc::new(SlowPassingPreflightSandbox)),
        sink.clone(),
        Arc::new(TicketLedger::new(Duration::from_millis(5))),
    );

    let operation = service
        .authorize_operation(context(), command_plan("printf authorized"))
        .await
        .unwrap();

    assert_eq!(operation.sandbox_backend_id, "slow-preflight");
    let events = sink.events();
    assert!(matches!(events[0], Event::PermissionRequested(_)));
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::AllowOnce
    ));
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightPassed(passed)
            if passed.status == SandboxPreflightStatus::Passed
                && passed.backend_id == "slow-preflight"
    ));
}

#[tokio::test]
async fn authorization_service_declared_network_resource_requires_effective_network_policy() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::ToolName("custom_network_tool".to_owned()),
        )
        .await,
        preflight_registry(Arc::new(NetworkCapablePreflightSandbox)),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let error = service
        .authorize_plan(context(), declared_network_resource_plan("network-capable"))
        .await
        .unwrap_err();

    assert!(
        matches!(error, ExecutionError::SandboxPreflightFailed { .. }),
        "unexpected error: {error:?}"
    );
    let events = sink.events();
    assert!(matches!(events[0], Event::PermissionRequested(_)));
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::AllowOnce
    ));
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightFailed(failed)
            if failed.status == SandboxPreflightStatus::Failed
                && failed.backend_id == "network-capable"
                && failed.reason.contains("cannot enforce network policy")
    ));
}

#[tokio::test]
async fn authorization_service_preflights_declared_network_resource_even_without_network_policy() {
    let sink = Arc::new(RecordingSink::default());
    let service = AuthorizationService::new(
        real_authority(
            RuleSource::Session,
            RuleAction::Allow,
            DecisionScope::ToolName("custom_network_tool".to_owned()),
        )
        .await,
        preflight_registry(Arc::new(RejectingPreflightSandbox {
            backend_id: "network-resource-preflight",
            reason: "declared network resource preflight".to_owned(),
        })),
        sink.clone(),
        Arc::new(TicketLedger::default()),
    );

    let error = service
        .authorize_plan(
            context(),
            declared_network_resource_plan("network-resource-preflight"),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(error, ExecutionError::SandboxPreflightFailed { .. }),
        "unexpected error: {error:?}"
    );
    let events = sink.events();
    assert!(matches!(events[0], Event::PermissionRequested(_)));
    assert!(matches!(
        &events[1],
        Event::PermissionResolved(resolved) if resolved.decision == Decision::AllowOnce
    ));
    assert!(matches!(
        &events[2],
        Event::SandboxPreflightFailed(failed)
            if failed.status == SandboxPreflightStatus::Failed
                && failed.backend_id == "network-resource-preflight"
                && matches!(failed.policy.network, NetworkAccess::AllowList(_))
                && failed.policy_hash != Default::default()
                && failed.reason.contains("declared network resource preflight")
    ));
}

#[tokio::test]
async fn authorization_service_carries_type_to_confirm_into_pending_permission() {
    let sink = Arc::new(RecordingSink::default());
    let (stream_broker, _receiver, resolver) = StreamBasedBroker::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    });
    let service = AuthorizationService::new(
        interactive_authority(Arc::new(stream_broker)).await,
        preflight_registry(Arc::new(TestSandbox::default())),
        sink,
        Arc::new(TicketLedger::default()),
    );
    let mut context = context();
    context.interactivity = InteractivityLevel::FullyInteractive;
    let mut plan = action_plan(
        "write_file",
        DecisionScope::ToolName("write_file".to_owned()),
    );
    plan.review.confirmation = PermissionConfirmation::TypeToConfirm {
        expected: "DELETE".to_owned(),
    };
    let request_id = plan.tool_use_id;

    let authorize = tokio::spawn(async move { service.authorize_plan(context, plan).await });

    let pending = wait_for_pending_confirmation(&resolver, request_id).await;
    assert_eq!(pending.as_deref(), Some("DELETE"));

    let pending = resolver
        .pending_permission_requests()
        .into_iter()
        .find(|pending| pending.request.tool_use_id == request_id)
        .expect("permission should still be pending");
    let request_id = pending.request.request_id;
    let tenant_id = pending.request.tenant_id;
    let session_id = pending.request.session_id;
    let option_id = pending
        .decision_options
        .into_iter()
        .find(|option| option.decision == Decision::DenyOnce)
        .expect("deny option should exist")
        .option_id;
    resolver
        .resolve_option_for(
            request_id,
            tenant_id,
            session_id,
            option_id,
            Decision::DenyOnce,
            None,
        )
        .await
        .unwrap();
    assert!(matches!(
        authorize.await.unwrap(),
        Err(ExecutionError::PermissionDenied { .. })
    ));
}

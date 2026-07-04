#![cfg(feature = "rule-engine")]

use chrono::Utc;
use harness_contracts::{
    Decision, DecisionScope, FallbackPolicy, InteractivityLevel, PermissionMode, PermissionSubject,
    RequestId, RuleSource, SessionId, Severity, TenantId, ToolUseId,
};
use harness_permission::{
    DangerousPatternLibrary, PermissionBroker, PermissionContext, PermissionRequest,
    PermissionRule, RuleAction, RuleEngineBroker,
};

#[tokio::test]
async fn dangerous_command_escalates_even_when_allow_rule_matches() {
    let broker = RuleEngineBroker::builder()
        .with_dangerous_library(DangerousPatternLibrary::default_unix())
        .with_rules(vec![allow_shell_rule()])
        .build()
        .await
        .unwrap();

    assert_eq!(
        broker
            .decide(
                dangerous_request("rm -rf /"),
                permission_context(InteractivityLevel::FullyInteractive)
            )
            .await,
        Decision::Escalate
    );
}

#[tokio::test]
async fn dangerous_command_fails_closed_without_interactivity() {
    let broker = RuleEngineBroker::builder()
        .with_dangerous_library(DangerousPatternLibrary::default_unix())
        .with_rules(vec![allow_shell_rule()])
        .build()
        .await
        .unwrap();

    assert_eq!(
        broker
            .decide(
                dangerous_request("rm -rf /"),
                permission_context(InteractivityLevel::NoInteractive)
            )
            .await,
        Decision::DenyOnce
    );
}

#[tokio::test]
async fn policy_deny_still_wins_before_dangerous_escalation() {
    let broker = RuleEngineBroker::builder()
        .with_dangerous_library(DangerousPatternLibrary::default_unix())
        .with_rules(vec![PermissionRule {
            id: "policy-deny-shell".to_owned(),
            priority: 1,
            scope: DecisionScope::ToolName("shell".to_owned()),
            action: RuleAction::Deny,
            source: RuleSource::Policy,
        }])
        .build()
        .await
        .unwrap();

    assert_eq!(
        broker
            .decide(
                dangerous_request("rm -rf /"),
                permission_context(InteractivityLevel::FullyInteractive)
            )
            .await,
        Decision::DenyOnce
    );
}

#[tokio::test]
async fn rule_engine_escalates_dangerous_command_in_bypass_permission_mode() {
    let broker = RuleEngineBroker::builder()
        .with_dangerous_library(DangerousPatternLibrary::default_unix())
        .with_rules(vec![allow_shell_rule()])
        .build()
        .await
        .unwrap();

    assert_eq!(
        broker
            .decide(
                dangerous_request("rm -rf /"),
                permission_context_with_mode(
                    InteractivityLevel::FullyInteractive,
                    PermissionMode::BypassPermissions,
                ),
            )
            .await,
        Decision::Escalate
    );
}

#[tokio::test]
async fn policy_deny_still_wins_in_bypass_permission_mode() {
    let broker = RuleEngineBroker::builder()
        .with_dangerous_library(DangerousPatternLibrary::default_unix())
        .with_rules(vec![PermissionRule {
            id: "policy-deny-shell".to_owned(),
            priority: 1,
            scope: DecisionScope::ToolName("shell".to_owned()),
            action: RuleAction::Deny,
            source: RuleSource::Policy,
        }])
        .build()
        .await
        .unwrap();

    assert_eq!(
        broker
            .decide(
                dangerous_request("rm -rf /"),
                permission_context_with_mode(
                    InteractivityLevel::FullyInteractive,
                    PermissionMode::BypassPermissions,
                ),
            )
            .await,
        Decision::DenyOnce
    );
}

#[tokio::test]
async fn any_policy_deny_still_wins_in_bypass_permission_mode() {
    let broker = RuleEngineBroker::builder()
        .with_dangerous_library(DangerousPatternLibrary::default_unix())
        .with_rules(vec![PermissionRule {
            id: "policy-deny-any".to_owned(),
            priority: 1,
            scope: DecisionScope::Any,
            action: RuleAction::Deny,
            source: RuleSource::Policy,
        }])
        .build()
        .await
        .unwrap();

    assert_eq!(
        broker
            .decide(
                dangerous_request("rm -rf /"),
                permission_context_with_mode(
                    InteractivityLevel::FullyInteractive,
                    PermissionMode::BypassPermissions,
                ),
            )
            .await,
        Decision::DenyOnce
    );
}

fn allow_shell_rule() -> PermissionRule {
    PermissionRule {
        id: "allow-shell".to_owned(),
        priority: 10,
        scope: DecisionScope::ToolName("shell".to_owned()),
        action: RuleAction::Allow,
        source: RuleSource::Workspace,
    }
}

fn dangerous_request(command: &str) -> PermissionRequest {
    let tenant_id = TenantId::SHARED;
    let session_id = SessionId::new();
    PermissionRequest {
        request_id: RequestId::new(),
        tenant_id,
        session_id,
        tool_use_id: ToolUseId::new(),
        tool_name: "shell".to_owned(),
        subject: PermissionSubject::CommandExec {
            command: command.to_owned(),
            argv: command.split_whitespace().map(str::to_owned).collect(),
            cwd: None,
            fingerprint: None,
        },
        severity: Severity::Critical,
        scope_hint: DecisionScope::ToolName("shell".to_owned()),
        action_plan_hash: harness_contracts::ActionPlanHash::default(),
        decision_options: Vec::new(),
        confirmation_expected: None,
        created_at: Utc::now(),
    }
}

fn permission_context(interactivity: InteractivityLevel) -> PermissionContext {
    permission_context_with_mode(interactivity, PermissionMode::Default)
}

fn permission_context_with_mode(
    interactivity: InteractivityLevel,
    permission_mode: PermissionMode,
) -> PermissionContext {
    PermissionContext {
        permission_mode,
        previous_mode: None,
        session_id: SessionId::new(),
        tenant_id: TenantId::SHARED,
        run_id: None,
        interactivity,
        timeout_policy: None,
        fallback_policy: FallbackPolicy::AskUser,
        hook_overrides: Vec::new(),
    }
}

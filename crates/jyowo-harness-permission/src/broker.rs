use async_trait::async_trait;
use chrono::{DateTime, Utc};
use harness_contracts::{
    ActionPlanHash, Decision, DecisionId, DecisionLifetime, DecisionMatcherKind,
    DecisionMatcherSummary, DecisionScope, ExecFingerprint, FallbackPolicy, InteractivityLevel,
    PermissionDecisionOption, PermissionError, PermissionMode, PermissionOptionId,
    PermissionSubject, RequestId, RuleSource, RunId, SessionId, Severity, TenantId, TimeoutPolicy,
    ToolUseId,
};

use crate::rule::OverrideDecision;

#[async_trait]
pub trait PermissionBroker: Send + Sync + 'static {
    fn can_anchor_authority(&self) -> bool {
        true
    }

    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision;

    async fn hard_policy_denies(
        &self,
        _request: &PermissionRequest,
        _ctx: &PermissionContext,
    ) -> bool {
        false
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError>;
}

#[must_use]
pub fn policy_scope_matches_request(
    rule_scope: &DecisionScope,
    request_scope: &DecisionScope,
) -> bool {
    match (rule_scope, request_scope) {
        (DecisionScope::Any, _) => true,
        (DecisionScope::PathPrefix(rule_path), DecisionScope::PathPrefix(request_path)) => {
            request_path.starts_with(rule_path)
        }
        _ => rule_scope == request_scope,
    }
}

#[async_trait]
pub trait DecisionPersistence: Send + Sync + 'static {
    fn supports_integrity(&self) -> bool {
        false
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError>;
}

#[derive(Debug, Default)]
pub struct NoopDecisionPersistence;

#[async_trait]
impl DecisionPersistence for NoopDecisionPersistence {
    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PersistedDecision {
    pub decision_id: DecisionId,
    pub decision: Decision,
    pub scope: DecisionScope,
    pub source: RuleSource,
    pub session_id: Option<SessionId>,
    pub fingerprint: Option<ExecFingerprint>,
}

pub fn canonical_permission_fingerprint(request: &PermissionRequest) -> ExecFingerprint {
    if let PermissionSubject::CommandExec {
        fingerprint: Some(fingerprint),
        ..
    } = &request.subject
    {
        return *fingerprint;
    }

    let mut hasher = blake3::Hasher::new();
    write_hash_field(&mut hasher, b"jyowo.permission_fingerprint.v1");
    write_hash_field(&mut hasher, request.tool_name.as_bytes());
    write_hash_field(&mut hasher, format!("{:?}", request.subject).as_bytes());
    write_hash_field(&mut hasher, format!("{:?}", request.scope_hint).as_bytes());
    ExecFingerprint(*hasher.finalize().as_bytes())
}

#[must_use]
pub fn default_permission_decision_options(
    request: &PermissionRequest,
) -> Vec<PermissionDecisionOption> {
    let fingerprint = Some(canonical_permission_fingerprint(request));
    vec![
        PermissionDecisionOption {
            option_id: PermissionOptionId::new(),
            decision: Decision::AllowOnce,
            scope: request.scope_hint.clone(),
            lifetime: DecisionLifetime::Once,
            matcher_summary: DecisionMatcherSummary {
                kind: matcher_kind_for_scope(&request.scope_hint),
                label: "allow once".to_owned(),
            },
            label: "Allow once".to_owned(),
            requires_confirmation: request.confirmation_expected.is_some(),
            action_plan_hash: request.action_plan_hash.clone(),
            fingerprint,
        },
        PermissionDecisionOption {
            option_id: PermissionOptionId::new(),
            decision: Decision::DenyOnce,
            scope: request.scope_hint.clone(),
            lifetime: DecisionLifetime::Once,
            matcher_summary: DecisionMatcherSummary {
                kind: matcher_kind_for_scope(&request.scope_hint),
                label: "deny once".to_owned(),
            },
            label: "Deny once".to_owned(),
            requires_confirmation: false,
            action_plan_hash: request.action_plan_hash.clone(),
            fingerprint,
        },
    ]
}

fn matcher_kind_for_scope(scope: &DecisionScope) -> DecisionMatcherKind {
    match scope {
        DecisionScope::ExactCommand { .. } => DecisionMatcherKind::ExactCommand,
        DecisionScope::ExactArgs(_) => DecisionMatcherKind::ExactArgs,
        DecisionScope::Any => DecisionMatcherKind::Any,
        DecisionScope::ToolName(_) => DecisionMatcherKind::ToolName,
        DecisionScope::Category(_) => DecisionMatcherKind::Category,
        DecisionScope::PathPrefix(_) => DecisionMatcherKind::PathPrefix,
        DecisionScope::GlobPattern(_) => DecisionMatcherKind::GlobPattern,
        DecisionScope::ExecuteCodeScript { .. } => DecisionMatcherKind::ExecuteCodeScript,
        _ => DecisionMatcherKind::Any,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRequest {
    pub request_id: RequestId,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub tool_use_id: ToolUseId,
    pub tool_name: String,
    pub subject: PermissionSubject,
    pub severity: Severity,
    pub scope_hint: DecisionScope,
    pub action_plan_hash: ActionPlanHash,
    pub decision_options: Vec<PermissionDecisionOption>,
    pub confirmation_expected: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionContext {
    pub permission_mode: PermissionMode,
    pub previous_mode: Option<PermissionMode>,
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub run_id: Option<RunId>,
    pub interactivity: InteractivityLevel,
    pub timeout_policy: Option<TimeoutPolicy>,
    pub fallback_policy: FallbackPolicy,
    pub hook_overrides: Vec<OverrideDecision>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionCheck {
    Allowed,
    Denied {
        reason: String,
    },
    AskUser {
        subject: PermissionSubject,
        scope: DecisionScope,
    },
    DangerousPattern {
        kind: String,
        pattern: String,
        severity: Severity,
        subject: PermissionSubject,
        scope: DecisionScope,
    },
    DangerousCommand {
        command: String,
        pattern: String,
        severity: Severity,
    },
}

fn write_hash_field(hasher: &mut blake3::Hasher, value: &[u8]) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value);
}

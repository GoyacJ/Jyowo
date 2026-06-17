use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use harness_contracts::{
    Decision, DecisionId, DecisionScope, ExecFingerprint, FallbackPolicy, InteractivityLevel,
    PermissionError, PermissionMode, PermissionSubject, RequestId, RuleSource, SessionId, Severity,
    TenantId, TimeoutPolicy, ToolUseId,
};

use crate::rule::{OverrideDecision, RuleSnapshot};

#[async_trait]
pub trait PermissionBroker: Send + Sync + 'static {
    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision;

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError>;
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
    fn supports_integrity(&self) -> bool {
        true
    }

    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PersistedDecision {
    pub decision_id: DecisionId,
    pub scope: DecisionScope,
    pub source: RuleSource,
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
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionContext {
    pub permission_mode: PermissionMode,
    pub previous_mode: Option<PermissionMode>,
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub interactivity: InteractivityLevel,
    pub timeout_policy: Option<TimeoutPolicy>,
    pub fallback_policy: FallbackPolicy,
    pub rule_snapshot: Arc<RuleSnapshot>,
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
        pattern: String,
        severity: Severity,
    },
}

fn write_hash_field(hasher: &mut blake3::Hasher, value: &[u8]) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value);
}

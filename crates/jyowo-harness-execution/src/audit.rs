use harness_contracts::{Decision, SandboxPreflightStatus};
use harness_permission::PermissionAuthorityDecisionSource;

#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizationAudit {
    pub permission_decision: Decision,
    pub permission_source: PermissionAuthorityDecisionSource,
    pub sandbox_preflight: SandboxPreflightStatus,
}

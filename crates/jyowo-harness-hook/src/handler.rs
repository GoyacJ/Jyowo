use async_trait::async_trait;
use harness_contracts::{HookError, HookEventKind, HookFailureMode, TrustLevel};

use crate::{HookContext, HookEvent, HookOutcome};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum HookRegistrationKind {
    InProcess,
    Exec,
    Http,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct HookHttpSecurityPosture {
    pub allowlist_non_empty: bool,
    pub ssrf_guard_strict: bool,
}

#[async_trait]
pub trait HookHandler: Send + Sync + 'static {
    fn handler_id(&self) -> &str;

    fn interested_events(&self) -> &[HookEventKind];

    fn priority(&self) -> i32 {
        0
    }

    fn failure_mode(&self) -> HookFailureMode {
        HookFailureMode::FailOpen
    }

    fn registration_kind(&self) -> HookRegistrationKind {
        HookRegistrationKind::InProcess
    }

    fn http_security_posture(&self) -> Option<HookHttpSecurityPosture> {
        None
    }

    fn declared_trust(&self) -> Option<TrustLevel> {
        None
    }

    async fn handle(&self, event: HookEvent, ctx: HookContext) -> Result<HookOutcome, HookError>;
}

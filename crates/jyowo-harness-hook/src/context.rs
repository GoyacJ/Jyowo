use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use harness_contracts::{
    CausationId, CorrelationId, InteractivityLevel, PermissionMode, Redactor, RunId, SessionId,
    TenantId, TrustLevel,
};

use crate::{HookMessageView, ToolDescriptorView};

#[derive(Clone)]
pub struct HookContext {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub turn_index: Option<u32>,
    pub correlation_id: CorrelationId,
    pub causation_id: CausationId,
    pub trust_level: TrustLevel,
    pub permission_mode: PermissionMode,
    pub interactivity: InteractivityLevel,
    pub at: DateTime<Utc>,
    pub view: Arc<dyn HookSessionView>,
    pub upstream_outcome: Option<UpstreamOutcomeView>,
    pub replay_mode: ReplayMode,
}

pub trait HookSessionView: Send + Sync + 'static {
    fn workspace_root(&self) -> Option<&Path>;

    fn recent_messages(&self, limit: usize) -> Vec<HookMessageView>;

    fn permission_mode(&self) -> PermissionMode;

    fn redacted(&self) -> &dyn Redactor;

    fn current_tool_descriptor(&self) -> Option<ToolDescriptorView>;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct UpstreamOutcomeView {
    pub last_handler_id: String,
    pub rewrote_input: bool,
    pub override_permission_present: bool,
    pub additional_context_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ReplayMode {
    Live,
    Audit,
}

use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use harness_contracts::{
    AgentId, CapabilityRegistry, CorrelationId, RunId, SessionId, TenantId, ToolCapability,
    ToolError, ToolUseId,
};
use harness_permission::PermissionBroker;
use harness_sandbox::SandboxBackend;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaResolverContext {
    pub run_id: RunId,
    pub session_id: SessionId,
    pub tenant_id: TenantId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentRunHandle {
    pub run_id: RunId,
    pub session_id: SessionId,
}

#[derive(Clone)]
pub struct ToolContext {
    pub tool_use_id: ToolUseId,
    pub run_id: RunId,
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub correlation_id: CorrelationId,
    pub agent_id: AgentId,
    pub subagent_depth: u8,
    pub workspace_root: PathBuf,
    pub sandbox: Option<Arc<dyn SandboxBackend>>,
    pub permission_broker: Arc<dyn PermissionBroker>,
    pub cap_registry: Arc<CapabilityRegistry>,
    pub interrupt: InterruptToken,
    pub parent_run: Option<ParentRunHandle>,
}

impl ToolContext {
    pub fn capability<T>(&self, cap: ToolCapability) -> Result<Arc<T>, ToolError>
    where
        T: ?Sized + Send + Sync + 'static,
    {
        self.cap_registry
            .get::<T>(&cap)
            .ok_or(ToolError::CapabilityMissing(cap))
    }
}

#[derive(Debug, Clone, Default)]
pub struct InterruptToken {
    interrupted: Arc<AtomicBool>,
}

impl InterruptToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn interrupt(&self) {
        self.interrupted.store(true, Ordering::SeqCst);
    }

    pub fn is_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }
}

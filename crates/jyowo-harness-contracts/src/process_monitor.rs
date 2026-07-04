use std::path::PathBuf;
use std::sync::Arc;

use futures::future::BoxFuture;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Event, Redactor, RunId, SandboxExitStatus, SandboxPolicy, SessionId, TenantId, ToolError,
    ToolUseId, WorkspaceAccess,
};

pub const RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY: &str = "run_scoped_process_registry";

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessStartRequest {
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer_bytes: Option<u32>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessReadRequest {
    pub process_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<u32>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessStopRequest {
    pub process_id: String,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProcessRuntimeStatus {
    Running,
    Exited,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessStartResult {
    pub process_id: String,
    pub pid: Option<u32>,
    pub status: ProcessRuntimeStatus,
    #[serde(skip)]
    #[schemars(skip)]
    pub sandbox_events: Vec<Event>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessReadResult {
    pub process_id: String,
    pub status: ProcessRuntimeStatus,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_status: Option<SandboxExitStatus>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessStopResult {
    pub process_id: String,
    pub status: ProcessRuntimeStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessStartInvocation {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: ToolUseId,
    pub workspace_root: PathBuf,
    pub request: ProcessStartRequest,
    pub sandbox_policy: SandboxPolicy,
    pub workspace_access: WorkspaceAccess,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessReadInvocation {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub request: ProcessReadRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessStopInvocation {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub request: ProcessStopRequest,
}

pub trait RunScopedProcessRegistryCap: Send + Sync + 'static {
    fn start_process(
        &self,
        invocation: ProcessStartInvocation,
        redactor: Arc<dyn Redactor>,
    ) -> BoxFuture<'_, Result<ProcessStartResult, ToolError>>;

    fn read_process(
        &self,
        invocation: ProcessReadInvocation,
        redactor: Arc<dyn Redactor>,
    ) -> BoxFuture<'_, Result<ProcessReadResult, ToolError>>;

    fn stop_process(
        &self,
        invocation: ProcessStopInvocation,
    ) -> BoxFuture<'_, Result<ProcessStopResult, ToolError>>;

    fn cleanup_run(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
    ) -> BoxFuture<'_, Result<(), ToolError>>;
}

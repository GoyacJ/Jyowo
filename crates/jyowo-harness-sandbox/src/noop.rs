//! Testing sandbox backend that records and rejects exec requests.

use std::time::Duration;

use async_trait::async_trait;
use harness_contracts::SandboxError;
use parking_lot::Mutex;

use crate::{
    ExecContext, ExecSpec, ProcessHandle, SandboxBackend, SandboxCapabilities, SessionSnapshotFile,
    SnapshotSpec,
};

#[derive(Debug, Default)]
pub struct NoopSandbox {
    recorded_execs: Mutex<Vec<ExecSpec>>,
    delay: Duration,
}

impl NoopSandbox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_delay(delay: Duration) -> Self {
        Self {
            recorded_execs: Mutex::new(Vec::new()),
            delay,
        }
    }

    pub fn recorded_execs(&self) -> Vec<ExecSpec> {
        self.recorded_execs.lock().clone()
    }
}

#[async_trait]
impl SandboxBackend for NoopSandbox {
    fn backend_id(&self) -> &'static str {
        "noop"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities::default()
    }

    fn preflight_execute(&self, _spec: &ExecSpec) -> Result<(), SandboxError> {
        Err(SandboxError::CapabilityMismatch {
            capability: "execute".to_owned(),
            detail: "noop sandbox rejects exec".to_owned(),
        })
    }

    async fn execute(
        &self,
        spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        self.recorded_execs.lock().push(spec);
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        Err(SandboxError::CapabilityMismatch {
            capability: "execute".to_owned(),
            detail: "noop sandbox rejects exec".to_owned(),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "noop_snapshot".to_owned(),
        })
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "noop_restore".to_owned(),
        })
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

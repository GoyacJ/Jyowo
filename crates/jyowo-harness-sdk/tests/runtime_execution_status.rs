#![cfg(feature = "testing")]

use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{NoopRedactor, SandboxError, ToolRuntimeStatus};
use harness_sandbox::{
    ExecContext, ExecSpec, NetworkPolicySupport, ProcessHandle, ResourceLimitSupport,
    RoutingSandboxBackend, SandboxBackend, SandboxCapabilities, SessionSnapshotFile, SnapshotSpec,
    WorkspacePolicySupport,
};
use jyowo_harness_sdk::{prelude::*, testing::*};

#[tokio::test]
async fn runtime_execution_status_requires_broker_for_web_fetch() {
    let harness = Harness::builder()
        .with_model_arc(Arc::new(TestModelProvider::default()))
        .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
        .with_sandbox(UnrestrictedProcessSandbox)
        .build()
        .await
        .expect("harness should build");

    let status = harness.runtime_execution_status();

    let web_fetch = tool_status(&status.tools, "WebFetch");
    assert!(
        !web_fetch.available,
        "WebFetch must not be available without HTTP broker"
    );
    assert!(web_fetch
        .unavailable_reason
        .as_deref()
        .unwrap_or_default()
        .contains("HTTP broker"));

    let web_search = tool_status(&status.tools, "WebSearch");
    assert!(
        !web_search.available,
        "WebSearch must not be available without a registered search backend"
    );
    assert!(web_search
        .unavailable_reason
        .as_deref()
        .unwrap_or_default()
        .contains("web search backend"));
}

#[tokio::test]
async fn runtime_execution_status_reports_routing_candidate_backend_ids() {
    let child_a: Arc<dyn SandboxBackend> = Arc::new(NamedStatusSandbox::new("local-process"));
    let child_b: Arc<dyn SandboxBackend> = Arc::new(NamedStatusSandbox::new("docker-process"));
    let routing = RoutingSandboxBackend::new(vec![child_a, child_b]).expect("routing backend");
    let harness = Harness::builder()
        .with_model_arc(Arc::new(TestModelProvider::default()))
        .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
        .with_sandbox(routing)
        .build()
        .await
        .expect("harness should build");

    let status = harness.runtime_execution_status();

    assert_eq!(status.process_sandbox.backend_id, "routing");
    assert_eq!(
        status.process_sandbox.candidate_ids,
        vec!["local-process".to_owned(), "docker-process".to_owned()],
        "runtime status should expose real routing candidates, not only `routing`"
    );
}

fn tool_status<'a>(tools: &'a [ToolRuntimeStatus], name: &str) -> &'a ToolRuntimeStatus {
    tools
        .iter()
        .find(|tool| tool.tool_name == name)
        .expect("tool status should exist")
}

#[derive(Debug)]
struct UnrestrictedProcessSandbox;

#[async_trait]
impl SandboxBackend for UnrestrictedProcessSandbox {
    fn backend_id(&self) -> &'static str {
        "unrestricted-test"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            network: NetworkPolicySupport {
                none: true,
                loopback_only: false,
                allowlist: false,
                unrestricted: true,
            },
            workspace: WorkspacePolicySupport {
                read_write_all: true,
                read_only: false,
                writable_subpaths: false,
            },
            max_concurrent_execs: 1,
            resource_limit_support: ResourceLimitSupport {
                wall_clock: true,
                ..ResourceLimitSupport::default()
            },
            ..SandboxCapabilities::default()
        }
    }

    fn preflight_execute(&self, spec: &ExecSpec) -> Result<(), SandboxError> {
        harness_sandbox::validate_preflight_capabilities(
            self.backend_id(),
            &self.capabilities(),
            spec,
        )
    }

    async fn execute(
        &self,
        _spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        Err(SandboxError::Message(
            "test sandbox does not execute".to_owned(),
        ))
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

#[derive(Debug)]
struct NamedStatusSandbox {
    id: &'static str,
}

impl NamedStatusSandbox {
    fn new(id: &'static str) -> Self {
        Self { id }
    }
}

#[async_trait]
impl SandboxBackend for NamedStatusSandbox {
    fn backend_id(&self) -> &str {
        self.id
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            network: NetworkPolicySupport {
                none: true,
                loopback_only: false,
                allowlist: false,
                unrestricted: true,
            },
            workspace: WorkspacePolicySupport {
                read_write_all: true,
                read_only: false,
                writable_subpaths: false,
            },
            max_concurrent_execs: 1,
            resource_limit_support: ResourceLimitSupport {
                wall_clock: true,
                ..ResourceLimitSupport::default()
            },
            ..SandboxCapabilities::default()
        }
    }

    async fn execute(
        &self,
        _spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        Err(SandboxError::Message(
            "test sandbox does not execute".to_owned(),
        ))
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    CapabilityRegistry, Event, PermissionError, SandboxError, SessionId, TenantId,
};
use harness_execution::{
    AuthorizationEventSink, AuthorizationService, ExecutionError, ExecutionPreflightRegistry,
    TicketLedger,
};
use harness_journal::EventStore;
use harness_permission::{
    DecisionHistory, DecisionLookup, DecisionPersistence, PermissionAuthority, PermissionBroker,
    PersistedDecision,
};
use harness_sandbox::{
    ExecContext, ExecSpec, NetworkPolicySupport, ProcessHandle, ResourceLimitSupport,
    SandboxBackend, SandboxCapabilities, SessionSnapshotFile, SnapshotSpec, WorkspacePolicySupport,
};
use parking_lot::Mutex;

pub fn test_authorization_service(
    broker: Arc<dyn PermissionBroker>,
    event_store: Arc<dyn EventStore>,
) -> Arc<AuthorizationService> {
    let decision_store = Arc::new(TransientDecisionStore::default());
    let authority = Arc::new(
        PermissionAuthority::builder()
            .with_policy_broker(broker)
            .with_transient_decision_store(decision_store)
            .build()
            .expect("test permission authority should build"),
    );
    let registry = ExecutionPreflightRegistry::new(
        Arc::new(TestSandbox),
        None,
        Arc::new(CapabilityRegistry::default()),
    );
    Arc::new(AuthorizationService::new(
        authority,
        registry,
        Arc::new(JournalAuthorizationEventSink { event_store }),
        Arc::new(TicketLedger::default()),
    ))
}

#[derive(Default)]
struct TransientDecisionStore {
    decisions: Mutex<Vec<PersistedDecision>>,
}

#[async_trait]
impl DecisionPersistence for TransientDecisionStore {
    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.decisions.lock().push(decision);
        Ok(())
    }
}

#[async_trait]
impl DecisionHistory for TransientDecisionStore {
    async fn find_scoped_decision(
        &self,
        _lookup: DecisionLookup,
    ) -> Result<Option<PersistedDecision>, PermissionError> {
        Ok(None)
    }
}

struct JournalAuthorizationEventSink {
    event_store: Arc<dyn EventStore>,
}

#[async_trait]
impl AuthorizationEventSink for JournalAuthorizationEventSink {
    async fn emit_batch(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        events: Vec<Event>,
    ) -> Result<(), ExecutionError> {
        self.event_store
            .append(tenant_id, session_id, &events)
            .await
            .map_err(|error| ExecutionError::EventSinkFailed {
                reason: error.to_string(),
            })?;
        Ok(())
    }
}

struct TestSandbox;

#[async_trait]
impl SandboxBackend for TestSandbox {
    fn backend_id(&self) -> &str {
        "test-sandbox"
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
            snapshot_kinds: BTreeSet::new(),
            resource_limit_support: ResourceLimitSupport {
                memory: true,
                cpu: true,
                pids: true,
                wall_clock: true,
                open_files: true,
            },
            ..SandboxCapabilities::default()
        }
    }

    async fn execute(
        &self,
        _spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        Err(SandboxError::CapabilityMismatch {
            capability: "execute".to_owned(),
            detail: "test sandbox does not execute".to_owned(),
        })
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

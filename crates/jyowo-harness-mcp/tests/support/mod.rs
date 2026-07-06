use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use harness_contracts::{
    CapabilityRegistry, Decision, Event, FallbackPolicy, InteractivityLevel, PermissionMode, RunId,
    SandboxError, SessionId, TenantId,
};
use harness_execution::{
    AuthorizationEventSink, AuthorizationService, ExecutionError, ExecutionPreflightRegistry,
    TicketLedger,
};
use harness_mcp::{McpAuthorizationContext, McpConnectContext};
use harness_permission::{
    NoopDecisionPersistence, PermissionAuthority, PermissionBroker, PermissionContext,
    PermissionRequest, PersistedDecision,
};
use harness_sandbox::{
    ExecContext, ExecSpec, NetworkPolicySupport, ProcessHandle, SandboxBackend,
    SandboxCapabilities, SessionSnapshotFile, SnapshotSpec, WorkspacePolicySupport,
};

pub fn authorized_connect_context() -> McpConnectContext {
    with_transport_authorization(McpConnectContext::default())
}

pub fn with_transport_authorization(context: McpConnectContext) -> McpConnectContext {
    context.with_authorization(mcp_authorization_context())
}

pub fn mcp_authorization_context() -> McpAuthorizationContext {
    mcp_authorization_context_with_broker(Arc::new(AllowTransportPermissionBroker))
}

#[allow(dead_code)]
pub fn mcp_authorization_context_allowing_tool(tool_name: &str) -> McpAuthorizationContext {
    mcp_authorization_context_with_broker(Arc::new(AllowListedPermissionBroker {
        tool_name: tool_name.to_owned(),
    }))
}

#[allow(dead_code)]
pub fn mcp_authorization_context_allowing_tool_with_sink(
    tool_name: &str,
    event_sink: Arc<dyn AuthorizationEventSink>,
) -> McpAuthorizationContext {
    mcp_authorization_context_with_broker_and_sink(
        Arc::new(AllowListedPermissionBroker {
            tool_name: tool_name.to_owned(),
        }),
        event_sink,
    )
}

fn mcp_authorization_context_with_broker(
    permission_broker: Arc<dyn PermissionBroker>,
) -> McpAuthorizationContext {
    mcp_authorization_context_with_broker_and_sink(
        permission_broker,
        Arc::new(NoopAuthorizationEventSink),
    )
}

fn mcp_authorization_context_with_broker_and_sink(
    permission_broker: Arc<dyn PermissionBroker>,
    event_sink: Arc<dyn AuthorizationEventSink>,
) -> McpAuthorizationContext {
    let authority = Arc::new(
        PermissionAuthority::builder()
            .with_policy_broker(permission_broker)
            .with_transient_decision_store(Arc::new(NoopDecisionPersistence))
            .build()
            .expect("test permission authority should build"),
    );
    let service = Arc::new(AuthorizationService::new(
        authority,
        ExecutionPreflightRegistry::new(
            Arc::new(AllowTransportPreflightSandbox),
            None,
            Arc::new(CapabilityRegistry::default()),
        ),
        event_sink,
        Arc::new(TicketLedger::default()),
    ));
    let session_id = SessionId::from_u128(1);
    McpAuthorizationContext {
        authorization_service: service,
        tenant_id: TenantId::SINGLE,
        scope: harness_contracts::McpServerScope::Session(session_id),
        session_id,
        run_id: RunId::from_u128(2),
        permission_mode: PermissionMode::Default,
        interactivity: InteractivityLevel::NoInteractive,
        fallback_policy: FallbackPolicy::AskUser,
        workspace_root: std::env::temp_dir(),
    }
}

struct AllowTransportPermissionBroker;

#[async_trait]
impl PermissionBroker for AllowTransportPermissionBroker {
    async fn decide(&self, request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        if matches!(request.tool_name.as_str(), "mcp_transport" | "mcp_sampling") {
            Decision::AllowOnce
        } else {
            Decision::DenyOnce
        }
    }

    async fn persist(
        &self,
        _decision: PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
    }
}

#[allow(dead_code)]
struct AllowListedPermissionBroker {
    tool_name: String,
}

#[async_trait]
impl PermissionBroker for AllowListedPermissionBroker {
    async fn decide(&self, request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        if request.tool_name == self.tool_name {
            Decision::AllowOnce
        } else {
            Decision::DenyOnce
        }
    }

    async fn persist(
        &self,
        _decision: PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
    }
}

#[allow(dead_code)]
#[derive(Default)]
pub struct RecordingAuthorizationEventSink {
    events: Mutex<Vec<Event>>,
}

#[allow(dead_code)]
impl RecordingAuthorizationEventSink {
    pub fn events(&self) -> Vec<Event> {
        self.events.lock().expect("events lock").clone()
    }
}

#[async_trait]
impl AuthorizationEventSink for RecordingAuthorizationEventSink {
    async fn emit_batch(
        &self,
        _tenant_id: TenantId,
        _session_id: SessionId,
        events: Vec<Event>,
    ) -> Result<(), ExecutionError> {
        self.events.lock().expect("events lock").extend(events);
        Ok(())
    }
}

struct NoopAuthorizationEventSink;

#[async_trait]
impl AuthorizationEventSink for NoopAuthorizationEventSink {
    async fn emit_batch(
        &self,
        _tenant_id: TenantId,
        _session_id: SessionId,
        _events: Vec<Event>,
    ) -> Result<(), ExecutionError> {
        Ok(())
    }
}

struct AllowTransportPreflightSandbox;

#[async_trait]
impl SandboxBackend for AllowTransportPreflightSandbox {
    fn backend_id(&self) -> &'static str {
        "allow-transport-preflight"
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
            ..SandboxCapabilities::default()
        }
    }

    fn preflight_execute(&self, _spec: &ExecSpec) -> Result<(), SandboxError> {
        Ok(())
    }

    async fn execute(
        &self,
        _spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        Err(SandboxError::CapabilityMismatch {
            capability: "execute".to_owned(),
            detail: "test sandbox only supports transport preflight".to_owned(),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "allow_transport_preflight_snapshot".to_owned(),
        })
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "allow_transport_preflight_restore".to_owned(),
        })
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

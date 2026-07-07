#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use futures::future::BoxFuture;
use harness_contracts::{
    ActionPlanHash, ActionPlanId, ActionResource, Decision, DecisionScope, Event, FallbackPolicy,
    HostRule, InteractivityLevel, NetworkAccess, OutboundUserMessage, PermissionActorSource,
    PermissionMode, PermissionSubject, ResourceLimits, RuleSource, RunId, SandboxMode,
    SandboxPolicy, SandboxScope, SessionId, Severity, TenantId, ToolActionPlan, ToolCapability,
    ToolError, ToolExecutionChannel, ToolUseId, UserMessageDelivery, UserMessengerCap,
    WorkspaceAccess,
};
use harness_contracts::{CapabilityRegistry, SandboxError};
use harness_execution::{
    AuthorizationContext, AuthorizationEventSink, ExecutionError, ExecutionPreflightRegistry,
};
use harness_permission::{
    DangerousPatternLibrary, NoopDecisionPersistence, PermissionAuthority, PermissionBroker,
    PermissionContext, PermissionRequest, PermissionRule, PersistedDecision, RuleAction,
    RuleEngineBroker,
};
use harness_sandbox::{
    ExecContext, ExecSpec, NetworkPolicySupport, ProcessHandle, SandboxBackend, SandboxBaseConfig,
    SandboxCapabilities, SessionSnapshotFile, SnapshotSpec,
};
use parking_lot::Mutex;

// ── Shared authorization-flow test support ──

pub async fn real_authority(
    source: RuleSource,
    action: RuleAction,
    scope: DecisionScope,
) -> Arc<PermissionAuthority> {
    let broker = RuleEngineBroker::builder()
        .with_tenant(TenantId::SINGLE)
        .with_rules(vec![PermissionRule {
            id: "test-rule".to_owned(),
            priority: 10,
            scope,
            action,
            source,
        }])
        .with_fallback(FallbackPolicy::AskUser)
        .build()
        .await
        .unwrap();

    Arc::new(
        PermissionAuthority::builder()
            .with_policy_broker(Arc::new(broker))
            .with_transient_decision_store(Arc::new(NoopDecisionPersistence))
            .build()
            .unwrap(),
    )
}

pub async fn interactive_authority(
    interactive_broker: Arc<dyn PermissionBroker>,
) -> Arc<PermissionAuthority> {
    Arc::new(
        PermissionAuthority::builder()
            .with_policy_broker(Arc::new(EscalatingPolicyBroker))
            .with_interactive_broker(interactive_broker)
            .with_transient_decision_store(Arc::new(NoopDecisionPersistence))
            .build()
            .unwrap(),
    )
}

pub async fn dangerous_command_authority() -> Arc<PermissionAuthority> {
    let broker = RuleEngineBroker::builder()
        .with_tenant(TenantId::SINGLE)
        .with_dangerous_library(DangerousPatternLibrary::default_unix())
        .with_rules(vec![PermissionRule {
            id: "allow-shell".to_owned(),
            priority: 10,
            scope: DecisionScope::ToolName("Bash".to_owned()),
            action: RuleAction::Allow,
            source: RuleSource::Session,
        }])
        .with_fallback(FallbackPolicy::AskUser)
        .build()
        .await
        .unwrap();

    Arc::new(
        PermissionAuthority::builder()
            .with_policy_broker(Arc::new(broker))
            .with_transient_decision_store(Arc::new(NoopDecisionPersistence))
            .build()
            .unwrap(),
    )
}

pub async fn wait_for_pending_confirmation(
    resolver: &harness_permission::ResolverHandle,
    tool_use_id: ToolUseId,
) -> Option<String> {
    for _ in 0..50 {
        if let Some(pending) = resolver
            .pending_permission_requests()
            .into_iter()
            .find(|pending| pending.request.tool_use_id == tool_use_id)
        {
            return pending.confirmation_expected;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    None
}

pub struct RecordingBroker {
    pub allow: bool,
}

#[async_trait]
impl harness_tool::ToolNetworkBrokerPreflightCap for RecordingBroker {
    async fn preflight_network_request(
        &self,
        _request: &harness_tool::NetworkBrokerPreflightRequest,
    ) -> Result<(), ToolError> {
        if self.allow {
            Ok(())
        } else {
            Err(ToolError::Message("broker denied".to_owned()))
        }
    }
}

pub fn broker_registry(
    sandbox: Arc<dyn SandboxBackend>,
    broker: Option<Arc<dyn harness_tool::ToolNetworkBrokerPreflightCap>>,
) -> ExecutionPreflightRegistry {
    ExecutionPreflightRegistry::new(sandbox, broker, Arc::new(CapabilityRegistry::default()))
}

pub fn http_broker_plan() -> ToolActionPlan {
    let mut plan = action_plan(
        "minimax_image_generation",
        DecisionScope::Category("network".to_owned()),
    );
    plan.execution_channel = ToolExecutionChannel::HttpBroker;
    plan.network_access = NetworkAccess::AllowList(vec![HostRule {
        pattern: "api.minimaxi.com".to_owned(),
        ports: Some(vec![443]),
    }]);
    plan.sandbox_policy.network = plan.network_access.clone();
    plan.resources = vec![ActionResource::Network {
        host: "api.minimaxi.com".to_owned(),
        port: Some(443),
    }];
    plan
}

pub fn http_broker_none_plan() -> ToolActionPlan {
    let mut plan = http_broker_plan();
    plan.network_access = NetworkAccess::None;
    plan.sandbox_policy.network = NetworkAccess::None;
    plan
}

pub fn external_capability_plan() -> ToolActionPlan {
    let mut plan = action_plan(
        "send_message",
        DecisionScope::ToolName("send_message".to_owned()),
    );
    plan.execution_channel = ToolExecutionChannel::ExternalCapability {
        capability: ToolCapability::UserMessenger,
    };
    plan
}

pub struct StubUserMessenger;

impl UserMessengerCap for StubUserMessenger {
    fn send(
        &self,
        _message: OutboundUserMessage,
    ) -> BoxFuture<'static, Result<UserMessageDelivery, ToolError>> {
        Box::pin(async {
            Ok(UserMessageDelivery {
                message_id: "msg-1".to_owned(),
                delivered: true,
            })
        })
    }
}

pub fn process_sandbox_plan() -> ToolActionPlan {
    let mut plan = command_plan("pwd");
    plan.execution_channel = ToolExecutionChannel::ProcessSandbox;
    plan.sandbox_policy.network = NetworkAccess::None;
    plan.network_access = NetworkAccess::None;
    plan
}

// ── Helpers ──

pub fn preflight_registry(sandbox: Arc<dyn SandboxBackend>) -> ExecutionPreflightRegistry {
    ExecutionPreflightRegistry::new(sandbox, None, Arc::new(CapabilityRegistry::default()))
}

pub fn context() -> AuthorizationContext {
    AuthorizationContext {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        run_id: RunId::new(),
        permission_mode: PermissionMode::Default,
        interactivity: InteractivityLevel::NoInteractive,
        fallback_policy: FallbackPolicy::AskUser,
        workspace_root: PathBuf::from("/workspace"),
    }
}

pub fn action_plan(tool_name: &str, scope: DecisionScope) -> ToolActionPlan {
    ToolActionPlan {
        plan_id: ActionPlanId::new(),
        tool_use_id: ToolUseId::new(),
        tool_name: tool_name.to_owned(),
        actor_source: PermissionActorSource::ParentRun,
        subject: PermissionSubject::ToolInvocation {
            tool: tool_name.to_owned(),
            input: serde_json::json!({}),
        },
        scope,
        severity: Severity::Medium,
        resources: vec![ActionResource::Sandbox {
            backend_id: "test-sandbox".to_owned(),
            policy_hash: Default::default(),
        }],
        sandbox_policy: SandboxPolicy {
            mode: SandboxMode::None,
            scope: SandboxScope::WorkspaceOnly,
            network: NetworkAccess::None,
            resource_limits: ResourceLimits {
                max_memory_bytes: None,
                max_cpu_cores: None,
                max_pids: None,
                max_wall_clock_ms: None,
                max_open_files: None,
            },
            denied_host_paths: Vec::new(),
        },
        workspace_access: WorkspaceAccess::None,
        network_access: NetworkAccess::None,
        execution_channel: ToolExecutionChannel::ProcessSandbox,
        review: Default::default(),
        plan_hash: ActionPlanHash::from_bytes([2; 32]),
        created_at: Utc::now(),
    }
}

pub fn dangerous_command_plan(command: &str) -> ToolActionPlan {
    let mut plan = action_plan("Bash", DecisionScope::ToolName("Bash".to_owned()));
    plan.subject = PermissionSubject::DangerousCommand {
        command: command.to_owned(),
        pattern_id: "unix-rm-rf-root".to_owned(),
        severity: Severity::Critical,
    };
    plan.severity = Severity::Critical;
    plan
}

pub fn command_plan(command: &str) -> ToolActionPlan {
    let mut plan = action_plan("Bash", DecisionScope::ToolName("Bash".to_owned()));
    plan.subject = PermissionSubject::CommandExec {
        command: command.to_owned(),
        argv: Vec::new(),
        cwd: None,
        fingerprint: None,
    };
    plan.resources = vec![ActionResource::Command {
        command: command.to_owned(),
        argv: Vec::new(),
        cwd: None,
        fingerprint: harness_contracts::ExecFingerprint([0; 32]),
    }];
    plan
}

pub fn network_only_plan() -> ToolActionPlan {
    let mut plan = action_plan(
        "mcp_transport",
        DecisionScope::ToolName("mcp_transport".to_owned()),
    );
    plan.resources = vec![
        ActionResource::Network {
            host: "api.example.test".to_owned(),
            port: Some(443),
        },
        ActionResource::Sandbox {
            backend_id: "network-capable".to_owned(),
            policy_hash: Default::default(),
        },
    ];
    let network_access = NetworkAccess::AllowList(vec![HostRule {
        pattern: "api.example.test".to_owned(),
        ports: Some(vec![443]),
    }]);
    plan.sandbox_policy.network = network_access.clone();
    plan.network_access = network_access;
    plan
}

pub fn declared_network_resource_plan(backend_id: &str) -> ToolActionPlan {
    let mut plan = action_plan(
        "custom_network_tool",
        DecisionScope::ToolName("custom_network_tool".to_owned()),
    );
    plan.resources = vec![
        ActionResource::Network {
            host: "api.example.test".to_owned(),
            port: Some(443),
        },
        ActionResource::Sandbox {
            backend_id: backend_id.to_owned(),
            policy_hash: Default::default(),
        },
    ];
    plan
}

pub struct SlowPassingPreflightSandbox;

#[async_trait]
impl SandboxBackend for SlowPassingPreflightSandbox {
    fn backend_id(&self) -> &str {
        "slow-preflight"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        }
    }

    fn preflight_execute(&self, _spec: &ExecSpec) -> Result<(), SandboxError> {
        std::thread::sleep(Duration::from_millis(30));
        Ok(())
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

struct EscalatingPolicyBroker;

#[async_trait]
impl PermissionBroker for EscalatingPolicyBroker {
    fn can_anchor_authority(&self) -> bool {
        true
    }

    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::Escalate
    }

    async fn hard_policy_denies(
        &self,
        _request: &PermissionRequest,
        _ctx: &PermissionContext,
    ) -> bool {
        false
    }

    async fn persist(
        &self,
        _decision: PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
    }
}

#[derive(Default)]
pub struct RecordingSink {
    events: Mutex<Vec<Event>>,
}

impl RecordingSink {
    pub fn events(&self) -> Vec<Event> {
        self.events.lock().clone()
    }
}

#[async_trait]
impl AuthorizationEventSink for RecordingSink {
    async fn emit_batch(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        events: Vec<Event>,
    ) -> Result<(), ExecutionError> {
        assert_eq!(tenant_id, TenantId::SINGLE);
        assert!(!session_id.to_string().is_empty());
        self.events.lock().extend(events);
        Ok(())
    }
}

#[derive(Default)]
pub struct TestSandbox;

#[async_trait]
impl SandboxBackend for TestSandbox {
    fn backend_id(&self) -> &str {
        "test-sandbox"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            max_concurrent_execs: 1,
            snapshot_kinds: BTreeSet::new(),
            ..SandboxCapabilities::default()
        }
    }

    fn base_config(&self) -> SandboxBaseConfig {
        SandboxBaseConfig::default()
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

pub struct NetworkCapablePreflightSandbox;

#[async_trait]
impl SandboxBackend for NetworkCapablePreflightSandbox {
    fn backend_id(&self) -> &str {
        "network-capable"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            network: NetworkPolicySupport {
                none: true,
                loopback_only: false,
                allowlist: false,
                unrestricted: true,
            },
            max_concurrent_execs: 1,
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

pub struct RejectingPreflightSandbox {
    pub backend_id: &'static str,
    pub reason: String,
}

#[async_trait]
impl SandboxBackend for RejectingPreflightSandbox {
    fn backend_id(&self) -> &str {
        self.backend_id
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            network: NetworkPolicySupport {
                none: true,
                loopback_only: false,
                allowlist: false,
                unrestricted: true,
            },
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        }
    }

    fn preflight_execute(&self, _spec: &ExecSpec) -> Result<(), SandboxError> {
        Err(SandboxError::CapabilityMismatch {
            capability: "network".to_owned(),
            detail: self.reason.clone(),
        })
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

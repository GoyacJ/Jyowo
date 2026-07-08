#![cfg(feature = "builtin-toolset")]

use std::path::Path;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Instant;

use async_trait::async_trait;
use bytes::Bytes;
use futures::{future::BoxFuture, stream, StreamExt};
use harness_contracts::{
    ActionResource, CapabilityRegistry, CorrelationId, Event, ProcessReadInvocation,
    ProcessReadRequest, ProcessReadResult, ProcessRuntimeStatus, ProcessStartInvocation,
    ProcessStartRequest, ProcessStartResult, ProcessStopInvocation, ProcessStopRequest,
    ProcessStopResult, RedactRules, Redactor, RunScopedProcessRegistryCap, SandboxError, SessionId,
    TenantId, ToolActionPlan, ToolCapability, ToolError, ToolResult, ToolUseHeartbeatEvent,
    ToolUseId, WorkspaceAccess, RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY,
};
use harness_sandbox::{
    ActivityHandle, ExecContext, ExecOutcome, ExecSpec, KillScope, NetworkPolicySupport,
    ProcessHandle, SandboxBackend, SandboxCapabilities, SessionSnapshotFile,
    WorkspacePolicySupport,
};
use harness_tool::{
    builtin::{ProcessReadTool, ProcessStartTool, ProcessStopTool},
    canonical_action_plan_hash, AuthorizedTicketSummary, AuthorizedToolInput,
    DefaultRunScopedProcessRegistry, InterruptToken, Tool, ToolContext, ValidationError,
};
use parking_lot::Mutex;
use serde_json::{json, Value};

#[tokio::test]
async fn process_start_requires_registry_capability() {
    let error = execute_error(
        &ProcessStartTool::default(),
        json!({ "command": "sleep", "args": ["100"] }),
        tool_ctx(CapabilityRegistry::default(), Arc::new(NoopRedactor)),
    )
    .await;

    assert!(matches!(
        error,
        ToolError::CapabilityMissing(ToolCapability::Custom(name))
            if name == RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY
    ));
}

#[tokio::test]
async fn process_tools_pass_scope_and_redactor_to_registry() {
    let registry = Arc::new(FakeProcessRegistry::default());
    let mut caps = CapabilityRegistry::default();
    caps.install::<dyn RunScopedProcessRegistryCap>(
        ToolCapability::Custom(RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY.to_owned()),
        registry.clone(),
    );
    let ctx = tool_ctx_at(
        std::env::temp_dir(),
        caps,
        Arc::new(SecretRedactor),
        SessionId::new(),
        harness_contracts::RunId::new(),
    );

    let start = execute_final(
        &ProcessStartTool::default(),
        json!({ "command": "pnpm", "args": ["dev"], "cwd": "apps/desktop" }),
        ctx.clone(),
    )
    .await;
    assert_eq!(
        start,
        ToolResult::Structured(json!({ "process_id": "proc-1", "pid": 9, "status": "running" }))
    );

    let read = execute_final(
        &ProcessReadTool::default(),
        json!({ "process_id": "proc-1", "max_bytes": 512 }),
        ctx.clone(),
    )
    .await;
    let ToolResult::Structured(read) = read else {
        panic!("expected structured read result");
    };
    assert_eq!(read["stdout"], "token [REDACTED]");
    assert_eq!(
        registry.last_start.lock().as_ref().unwrap().session_id,
        ctx.session_id
    );
    assert_eq!(
        registry.last_start.lock().as_ref().unwrap().request.args,
        ["dev"]
    );
    assert_eq!(
        registry.last_read.lock().as_ref().unwrap().run_id,
        ctx.run_id
    );

    let stop = execute_final(
        &ProcessStopTool::default(),
        json!({ "process_id": "proc-1" }),
        ctx,
    )
    .await;
    assert_eq!(
        stop,
        ToolResult::Structured(json!({ "process_id": "proc-1", "status": "stopped" }))
    );
}

#[tokio::test]
async fn process_start_streams_sandbox_events_before_final_result() {
    let registry = Arc::new(FakeProcessRegistry::default());
    let mut caps = CapabilityRegistry::default();
    caps.install::<dyn RunScopedProcessRegistryCap>(
        ToolCapability::Custom(RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY.to_owned()),
        registry,
    );
    let ctx = tool_ctx(caps, Arc::new(NoopRedactor));

    let events = execute_events(
        &ProcessStartTool::default(),
        json!({ "command": "pnpm", "args": ["dev"] }),
        ctx,
    )
    .await;

    assert!(matches!(
        events.first(),
        Some(harness_tool::ToolEvent::Journal(Event::ToolUseHeartbeat(_)))
    ));
    assert!(matches!(
        events.last(),
        Some(harness_tool::ToolEvent::Final(ToolResult::Structured(value)))
            if value == &json!({ "process_id": "proc-1", "pid": 9, "status": "running" })
    ));
}

#[tokio::test]
async fn process_start_default_registry_streams_sandbox_preflight_before_final_result() {
    let sandbox = Arc::new(FakeSandbox::new(vec![Bytes::from_static(b"ready")]));
    let registry = Arc::new(DefaultRunScopedProcessRegistry::new(sandbox));
    let mut caps = CapabilityRegistry::default();
    caps.install::<dyn RunScopedProcessRegistryCap>(
        ToolCapability::Custom(RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY.to_owned()),
        registry,
    );

    let events = execute_events(
        &ProcessStartTool::default(),
        json!({ "command": "serve" }),
        tool_ctx(caps, Arc::new(NoopRedactor)),
    )
    .await;

    let preflight_index = events
        .iter()
        .position(|event| {
            matches!(
                event,
                harness_tool::ToolEvent::Journal(Event::SandboxPreflightPassed(_))
            )
        })
        .expect("expected sandbox preflight event");
    let final_index = events
        .iter()
        .position(|event| matches!(event, harness_tool::ToolEvent::Final(_)))
        .expect("expected final result");
    assert!(preflight_index < final_index);
}

#[tokio::test]
async fn process_start_default_registry_uses_authorized_sandbox_policy() {
    let sandbox = Arc::new(FakeSandbox::new(vec![Bytes::from_static(b"ready")]));
    let registry = Arc::new(DefaultRunScopedProcessRegistry::new(sandbox.clone()));
    let mut caps = CapabilityRegistry::default();
    caps.install::<dyn RunScopedProcessRegistryCap>(
        ToolCapability::Custom(RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY.to_owned()),
        registry,
    );
    let ctx = tool_ctx(caps, Arc::new(NoopRedactor));
    let tool = ProcessStartTool::default();
    let input = json!({ "command": "serve" });
    let mut plan = tool.plan(&input, &ctx).await.unwrap();
    plan.sandbox_policy
        .denied_host_paths
        .push(std::path::PathBuf::from("/tmp/blocked"));
    plan.plan_hash = canonical_action_plan_hash(&plan);
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).unwrap();

    let mut stream = tool.execute_authorized(authorized, ctx).await.unwrap();
    while stream.next().await.is_some() {}

    let specs = sandbox.recorded_execs();
    assert_eq!(specs.len(), 1);
    assert_eq!(
        specs[0].policy.denied_host_paths,
        [std::path::PathBuf::from("/tmp/blocked")]
    );
}

#[tokio::test]
async fn process_start_uses_authorized_command_resource_not_raw_input() {
    let registry = Arc::new(FakeProcessRegistry::default());
    let mut caps = CapabilityRegistry::default();
    caps.install::<dyn RunScopedProcessRegistryCap>(
        ToolCapability::Custom(RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY.to_owned()),
        registry.clone(),
    );
    let ctx = tool_ctx(caps, Arc::new(NoopRedactor));
    let tool = ProcessStartTool::default();
    let planned_input = json!({ "command": "pnpm", "args": ["dev"], "cwd": "apps/desktop" });
    let raw_input = json!({ "command": "rm", "args": ["-rf", "/"], "cwd": "tmp" });
    let plan = tool.plan(&planned_input, &ctx).await.unwrap();
    let authorized = AuthorizedToolInput::new(raw_input, plan.clone(), ticket_for(&plan)).unwrap();

    let mut stream = tool.execute_authorized(authorized, ctx).await.unwrap();
    while stream.next().await.is_some() {}

    let invocation = registry.last_start.lock().clone().unwrap();
    assert_eq!(invocation.request.command, "pnpm");
    assert_eq!(invocation.request.args, ["dev"]);
    assert_eq!(invocation.request.cwd.as_deref(), Some("apps/desktop"));
}

#[tokio::test]
async fn process_start_rejects_authorized_command_when_fingerprint_no_longer_matches_exec_spec() {
    let registry = Arc::new(FakeProcessRegistry::default());
    let mut caps = CapabilityRegistry::default();
    caps.install::<dyn RunScopedProcessRegistryCap>(
        ToolCapability::Custom(RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY.to_owned()),
        registry.clone(),
    );
    let ctx = tool_ctx(caps, Arc::new(NoopRedactor));
    let tool = ProcessStartTool::default();
    let input = json!({ "command": "pnpm", "args": ["dev"] });
    let mut plan = tool.plan(&input, &ctx).await.unwrap();
    let Some(ActionResource::Command { command, .. }) = plan
        .resources
        .iter_mut()
        .find(|resource| matches!(resource, ActionResource::Command { .. }))
    else {
        panic!("expected command resource");
    };
    *command = "rm".to_owned();
    plan.plan_hash = canonical_action_plan_hash(&plan);
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).unwrap();

    let error = match tool.execute_authorized(authorized, ctx).await {
        Ok(_) => panic!("expected authorized execution to fail"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        ToolError::PermissionDenied(ref message)
            if message == "authorized command fingerprint mismatch"
    ));
    assert!(registry.last_start.lock().is_none());
}

#[tokio::test]
async fn process_start_rejects_ticket_with_mismatched_plan_hash() {
    let registry = Arc::new(FakeProcessRegistry::default());
    let mut caps = CapabilityRegistry::default();
    caps.install::<dyn RunScopedProcessRegistryCap>(
        ToolCapability::Custom(RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY.to_owned()),
        registry.clone(),
    );
    let ctx = tool_ctx(caps, Arc::new(NoopRedactor));
    let tool = ProcessStartTool::default();
    let plan = tool
        .plan(&json!({ "command": "pnpm", "args": ["dev"] }), &ctx)
        .await
        .unwrap();
    let other_plan = tool
        .plan(&json!({ "command": "serve" }), &ctx)
        .await
        .unwrap();
    let ticket_for_other = ticket_for(&other_plan);

    let error = AuthorizedToolInput::new(
        json!({ "command": "pnpm", "args": ["dev"] }),
        plan.clone(),
        ticket_for_other,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        ToolError::PermissionDenied(ref message)
            if message == "authorization ticket action plan hash does not match action plan"
    ));
    assert!(registry.last_start.lock().is_none());
}

#[tokio::test]
async fn process_start_rejects_command_with_inline_args() {
    let error = validate_error(
        &ProcessStartTool::default(),
        json!({ "command": "pnpm dev" }),
        tool_ctx(CapabilityRegistry::default(), Arc::new(NoopRedactor)),
    )
    .await;

    assert!(error
        .to_string()
        .contains("command must be an executable name"));
}

#[tokio::test]
async fn default_registry_buffers_truncates_redacts_and_stops_processes() {
    let sandbox = Arc::new(FakeSandbox::new(vec![Bytes::from_static(
        b"aaaaSECRET123bbbb",
    )]));
    let registry = DefaultRunScopedProcessRegistry::new(sandbox);
    let session_id = SessionId::new();
    let run_id = harness_contracts::RunId::new();
    let workspace = tempfile::tempdir().unwrap();

    let start = registry
        .start_process(
            ProcessStartInvocation {
                tenant_id: TenantId::SINGLE,
                session_id,
                run_id,
                tool_use_id: ToolUseId::new(),
                workspace_root: workspace.path().to_path_buf(),
                request: ProcessStartRequest {
                    command: "serve".to_owned(),
                    args: Vec::new(),
                    cwd: None,
                    buffer_bytes: Some(8),
                },
                sandbox_policy: ExecSpec::default().policy,
                workspace_access: WorkspaceAccess::ReadWrite {
                    allowed_writable_subpaths: Vec::new(),
                },
            },
            Arc::new(SecretRedactor),
        )
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let read = registry
        .read_process(
            ProcessReadInvocation {
                tenant_id: TenantId::SINGLE,
                session_id,
                run_id,
                request: ProcessReadRequest {
                    process_id: start.process_id.clone(),
                    max_bytes: Some(64),
                },
            },
            Arc::new(SecretRedactor),
        )
        .await
        .unwrap();
    assert!(!read.stdout.contains("SECRET"));
    assert!(!read.stdout.contains("123"));
    assert!(read.stdout.ends_with("bbbb"));
    assert!(read.stdout_truncated);

    registry
        .cleanup_run(TenantId::SINGLE, session_id, run_id)
        .await
        .unwrap();
    assert_eq!(registry.active_process_count(), 0);
}

#[tokio::test]
async fn default_registry_read_after_stop_returns_stopped_status() {
    let sandbox = Arc::new(FakeSandbox::new(vec![Bytes::from_static(b"ready")]));
    let kill_count = sandbox.kill_count.clone();
    let registry = DefaultRunScopedProcessRegistry::new(sandbox);
    let session_id = SessionId::new();
    let run_id = harness_contracts::RunId::new();
    let workspace = tempfile::tempdir().unwrap();

    let start = registry
        .start_process(
            ProcessStartInvocation {
                tenant_id: TenantId::SINGLE,
                session_id,
                run_id,
                tool_use_id: ToolUseId::new(),
                workspace_root: workspace.path().to_path_buf(),
                request: ProcessStartRequest {
                    command: "serve".to_owned(),
                    args: Vec::new(),
                    cwd: None,
                    buffer_bytes: None,
                },
                sandbox_policy: ExecSpec::default().policy,
                workspace_access: WorkspaceAccess::ReadWrite {
                    allowed_writable_subpaths: Vec::new(),
                },
            },
            Arc::new(NoopRedactor),
        )
        .await
        .unwrap();
    registry
        .stop_process(ProcessStopInvocation {
            tenant_id: TenantId::SINGLE,
            session_id,
            run_id,
            request: ProcessStopRequest {
                process_id: start.process_id.clone(),
            },
        })
        .await
        .unwrap();

    let read = registry
        .read_process(
            ProcessReadInvocation {
                tenant_id: TenantId::SINGLE,
                session_id,
                run_id,
                request: ProcessReadRequest {
                    process_id: start.process_id,
                    max_bytes: None,
                },
            },
            Arc::new(NoopRedactor),
        )
        .await
        .unwrap();
    assert_eq!(read.status, ProcessRuntimeStatus::Stopped);
    assert!(kill_count.load(Ordering::SeqCst) > 0);
}

#[derive(Default)]
struct FakeProcessRegistry {
    last_start: Mutex<Option<ProcessStartInvocation>>,
    last_read: Mutex<Option<ProcessReadInvocation>>,
}

impl RunScopedProcessRegistryCap for FakeProcessRegistry {
    fn start_process(
        &self,
        invocation: ProcessStartInvocation,
        _redactor: Arc<dyn Redactor>,
    ) -> BoxFuture<'_, Result<ProcessStartResult, ToolError>> {
        *self.last_start.lock() = Some(invocation);
        Box::pin(async {
            Ok(ProcessStartResult {
                process_id: "proc-1".to_owned(),
                pid: Some(9),
                status: ProcessRuntimeStatus::Running,
                sandbox_events: vec![heartbeat_event()],
            })
        })
    }

    fn read_process(
        &self,
        invocation: ProcessReadInvocation,
        redactor: Arc<dyn Redactor>,
    ) -> BoxFuture<'_, Result<ProcessReadResult, ToolError>> {
        *self.last_read.lock() = Some(invocation);
        Box::pin(async move {
            Ok(ProcessReadResult {
                process_id: "proc-1".to_owned(),
                status: ProcessRuntimeStatus::Running,
                stdout: redactor.redact("token SECRET123", &RedactRules::default()),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                exit_status: None,
            })
        })
    }

    fn stop_process(
        &self,
        _invocation: ProcessStopInvocation,
    ) -> BoxFuture<'_, Result<ProcessStopResult, ToolError>> {
        Box::pin(async {
            Ok(ProcessStopResult {
                process_id: "proc-1".to_owned(),
                status: ProcessRuntimeStatus::Stopped,
            })
        })
    }

    fn cleanup_run(
        &self,
        _tenant_id: TenantId,
        _session_id: SessionId,
        _run_id: harness_contracts::RunId,
    ) -> BoxFuture<'_, Result<(), ToolError>> {
        Box::pin(async { Ok(()) })
    }
}

struct SecretRedactor;

impl Redactor for SecretRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.replace("SECRET123", "[REDACTED]")
    }
}

struct NoopRedactor;

impl Redactor for NoopRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.to_owned()
    }
}

struct FakeSandbox {
    stdout: Mutex<Vec<Bytes>>,
    recorded_execs: Mutex<Vec<ExecSpec>>,
    activity: Arc<FakeActivity>,
    kill_count: Arc<AtomicUsize>,
}

impl FakeSandbox {
    fn new(stdout: Vec<Bytes>) -> Self {
        let kill_count = Arc::new(AtomicUsize::new(0));
        Self {
            stdout: Mutex::new(stdout),
            recorded_execs: Mutex::new(Vec::new()),
            activity: Arc::new(FakeActivity {
                kill_count: kill_count.clone(),
                last_activity: Instant::now(),
            }),
            kill_count,
        }
    }

    fn recorded_execs(&self) -> Vec<ExecSpec> {
        self.recorded_execs.lock().clone()
    }
}

#[async_trait]
impl SandboxBackend for FakeSandbox {
    fn backend_id(&self) -> &str {
        "fake"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            supports_streaming: true,
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

    async fn execute(
        &self,
        spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        self.recorded_execs.lock().push(spec);
        let stdout = std::mem::take(&mut *self.stdout.lock());
        Ok(ProcessHandle {
            pid: Some(42),
            stdout: Some(Box::pin(stream::iter(stdout))),
            stderr: Some(Box::pin(stream::empty())),
            stdin: None,
            cwd_marker: None,
            activity: self.activity.clone(),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &harness_sandbox::SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "snapshot".to_owned(),
        })
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "restore".to_owned(),
        })
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

struct FakeActivity {
    kill_count: Arc<AtomicUsize>,
    last_activity: Instant,
}

#[async_trait]
impl ActivityHandle for FakeActivity {
    async fn wait(&self) -> Result<ExecOutcome, SandboxError> {
        futures::future::pending().await
    }

    async fn kill(&self, _signal: i32, _scope: KillScope) -> Result<(), SandboxError> {
        self.kill_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn touch(&self) {}

    fn last_activity(&self) -> Instant {
        self.last_activity
    }
}

async fn execute_final(tool: &dyn Tool, input: Value, ctx: ToolContext) -> ToolResult {
    let events = execute_events(tool, input, ctx).await;
    events
        .into_iter()
        .find_map(|event| match event {
            harness_tool::ToolEvent::Final(result) => Some(result),
            _ => None,
        })
        .expect("expected final result")
}

async fn execute_events(
    tool: &dyn Tool,
    input: Value,
    ctx: ToolContext,
) -> Vec<harness_tool::ToolEvent> {
    tool.validate(&input, &ctx).await.unwrap();
    let plan = tool.plan(&input, &ctx).await.unwrap();
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).unwrap();
    let mut stream = tool.execute_authorized(authorized, ctx).await.unwrap();
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }
    events
}

async fn validate_error(tool: &dyn Tool, input: Value, ctx: ToolContext) -> ValidationError {
    tool.validate(&input, &ctx).await.unwrap_err()
}

async fn execute_error(tool: &dyn Tool, input: Value, ctx: ToolContext) -> ToolError {
    tool.validate(&input, &ctx).await.unwrap();
    let plan = match tool.plan(&input, &ctx).await {
        Ok(plan) => plan,
        Err(error) => return error,
    };
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).unwrap();
    match tool.execute_authorized(authorized, ctx).await {
        Ok(_) => panic!("expected tool error"),
        Err(error) => error,
    }
}

fn tool_ctx(cap_registry: CapabilityRegistry, redactor: Arc<dyn Redactor>) -> ToolContext {
    tool_ctx_at(
        std::env::temp_dir(),
        cap_registry,
        redactor,
        SessionId::new(),
        harness_contracts::RunId::new(),
    )
}

fn tool_ctx_at(
    workspace_root: impl AsRef<Path>,
    cap_registry: CapabilityRegistry,
    redactor: Arc<dyn Redactor>,
    session_id: SessionId,
    run_id: harness_contracts::RunId,
) -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id,
        session_id,
        tenant_id: TenantId::SINGLE,
        correlation_id: CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: workspace_root.as_ref().to_path_buf(),
        project_workspace_root: None,
        sandbox: None,
        cap_registry: Arc::new(cap_registry),
        redactor,
        interrupt: InterruptToken::default(),
        parent_run: None,
        model: None,
        model_config_id: None,
        memory_thread_settings: None,
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
    }
}

fn ticket_for(plan: &ToolActionPlan) -> AuthorizedTicketSummary {
    {
        let ledger = harness_tool::TicketLedger::default();
        let claims = harness_tool::AuthorizationTicketClaims {
            tenant_id: harness_contracts::TenantId::SINGLE,
            session_id: harness_contracts::SessionId::new(),
            run_id: harness_contracts::RunId::new(),
            tool_use_id: plan.tool_use_id,
            tool_name: plan.tool_name.clone(),
            action_plan_hash: plan.plan_hash.clone(),
        };
        let ticket = ledger
            .mint(claims.clone(), chrono::Utc::now())
            .expect("test ticket should mint");
        ledger
            .consume(ticket.id, &claims, chrono::Utc::now())
            .expect("test ticket should consume")
    }
}

fn heartbeat_event() -> Event {
    Event::ToolUseHeartbeat(ToolUseHeartbeatEvent {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        message: "sandbox started".to_owned(),
        fraction: None,
        silent_for_ms: 0,
        at: chrono::Utc::now(),
    })
}

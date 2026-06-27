#![cfg(feature = "builtin-toolset")]

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    AgentId, CapabilityRegistry, CorrelationId, Decision, DecisionScope, FallbackPolicy,
    InteractivityLevel, PermissionMode, PermissionSubject, TenantId, ToolUseId, WorkspaceAccess,
};
use harness_permission::{
    canonical_permission_fingerprint, PermissionBroker, PermissionContext, PermissionRequest,
    RuleSnapshot,
};
use harness_sandbox::{ExecSpec, SandboxBaseConfig, StdioSpec};
use harness_tool::{
    builtin::BashTool, BuiltinToolset, InterruptToken, NoopToolEventEmitter, OrchestratorContext,
    Tool, ToolCall, ToolContext, ToolOrchestrator, ToolPool, ToolPoolFilter, ToolPoolModelProfile,
    ToolRegistry, ToolSearchMode,
};
use parking_lot::Mutex;
use serde_json::json;

#[tokio::test]
async fn builtin_permission_requests_use_canonical_fingerprints() {
    let broker = Arc::new(RecordingBroker::new(vec![
        Decision::DenyOnce,
        Decision::DenyOnce,
        Decision::DenyOnce,
    ]));
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(workspace.path().join("src")).unwrap();
    std::fs::write(workspace.path().join("src/lib.rs"), "").unwrap();
    let ctx = orchestrator_ctx(broker.clone(), workspace.path()).await;
    let orchestrator = ToolOrchestrator::new(1);

    let calls = vec![
        ToolCall {
            tool_use_id: ToolUseId::new(),
            tool_name: "Bash".to_owned(),
            input: json!({ "command": "cargo test", "cwd": "/workspace/./crates/../crates/jyowo" }),
        },
        ToolCall {
            tool_use_id: ToolUseId::new(),
            tool_name: "FileRead".to_owned(),
            input: json!({ "path": "src/lib.rs" }),
        },
        ToolCall {
            tool_use_id: ToolUseId::new(),
            tool_name: "WebFetch".to_owned(),
            input: json!({ "url": "https://example.com:443/docs" }),
        },
    ];

    orchestrator.dispatch(calls, ctx).await;

    let requests = broker.calls();
    assert_eq!(requests.len(), 3);
    assert_command_fingerprint(&requests[0]);
    assert_ne!(canonical_permission_fingerprint(&requests[1]).0, [0; 32]);
    assert_ne!(canonical_permission_fingerprint(&requests[2]).0, [0; 32]);
}

#[tokio::test]
async fn bash_check_permission_generates_canonical_fingerprint() {
    let broker = Arc::new(RecordingBroker::new(vec![Decision::DenyOnce]));
    let workspace = tempfile::tempdir().unwrap();
    let ctx = orchestrator_ctx(broker, workspace.path())
        .await
        .tool_context;
    let tool = BashTool::default();
    let input = json!({ "command": "cargo test", "cwd": "/workspace/./crates/../crates/jyowo" });

    let first = tool.check_permission(&input, &ctx).await;
    let second = tool.check_permission(&input, &ctx).await;
    let changed = tool
        .check_permission(&json!({ "command": "cargo test --all" }), &ctx)
        .await;

    let first_fingerprint = command_fingerprint(first);
    assert_eq!(first_fingerprint, command_fingerprint(second));
    assert_ne!(first_fingerprint, command_fingerprint(changed));
}

fn assert_command_fingerprint(request: &PermissionRequest) {
    let PermissionSubject::CommandExec {
        command,
        cwd,
        fingerprint,
        ..
    } = &request.subject
    else {
        panic!("Bash should request command permission");
    };
    assert_eq!(
        request.scope_hint,
        DecisionScope::ExactCommand {
            command: command.clone(),
            cwd: cwd.clone(),
        }
    );

    let expected = ExecSpec {
        command: command.clone(),
        cwd: cwd.clone(),
        stdin: StdioSpec::Null,
        stdout: StdioSpec::Piped,
        stderr: StdioSpec::Piped,
        workspace_access: WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: Vec::new(),
        },
        ..ExecSpec::default()
    }
    .canonical_fingerprint(&SandboxBaseConfig::default());

    assert_eq!(*fingerprint, Some(expected));
    assert_eq!(canonical_permission_fingerprint(request), expected);
}

fn command_fingerprint(
    check: harness_permission::PermissionCheck,
) -> harness_contracts::ExecFingerprint {
    let harness_permission::PermissionCheck::AskUser {
        subject:
            PermissionSubject::CommandExec {
                command,
                cwd,
                fingerprint: Some(fingerprint),
                ..
            },
        scope,
    } = check
    else {
        panic!("expected command permission check with fingerprint");
    };
    let DecisionScope::ExactCommand {
        command: scope_command,
        cwd: scope_cwd,
    } = scope
    else {
        panic!("expected ExactCommand scope");
    };
    assert_eq!(command, scope_command);
    assert_eq!(cwd, scope_cwd);

    let expected = ExecSpec {
        command,
        cwd,
        stdin: StdioSpec::Null,
        stdout: StdioSpec::Piped,
        stderr: StdioSpec::Piped,
        workspace_access: WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: Vec::new(),
        },
        ..ExecSpec::default()
    }
    .canonical_fingerprint(&SandboxBaseConfig::default());

    assert_eq!(fingerprint, expected);
    fingerprint
}

#[derive(Clone)]
struct RecordingBroker {
    decisions: Arc<Mutex<VecDeque<Decision>>>,
    calls: Arc<Mutex<Vec<PermissionRequest>>>,
}

impl RecordingBroker {
    fn new(decisions: Vec<Decision>) -> Self {
        Self {
            decisions: Arc::new(Mutex::new(decisions.into())),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> Vec<PermissionRequest> {
        self.calls.lock().clone()
    }
}

#[async_trait]
impl PermissionBroker for RecordingBroker {
    async fn decide(&self, request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        self.calls.lock().push(request);
        self.decisions
            .lock()
            .pop_front()
            .unwrap_or(Decision::DenyOnce)
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
    }
}

async fn pool() -> ToolPool {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();
    ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter::default(),
        &ToolSearchMode::Disabled,
        &ToolPoolModelProfile::default(),
        &harness_tool::SchemaResolverContext {
            run_id: harness_contracts::RunId::new(),
            session_id: harness_contracts::SessionId::new(),
            tenant_id: TenantId::SINGLE,
        },
    )
    .await
    .unwrap()
}

async fn orchestrator_ctx(
    broker: Arc<RecordingBroker>,
    workspace_root: &Path,
) -> OrchestratorContext {
    let run_id = harness_contracts::RunId::new();
    let session_id = harness_contracts::SessionId::new();
    OrchestratorContext {
        pool: pool().await,
        tool_context: ToolContext {
            tool_use_id: ToolUseId::new(),
            run_id,
            session_id,
            tenant_id: TenantId::SINGLE,
            correlation_id: CorrelationId::new(),
            agent_id: AgentId::from_u128(1),
            subagent_depth: 0,
            workspace_root: workspace_root.to_path_buf(),
            sandbox: None,
            permission_broker: broker,
            cap_registry: Arc::new(CapabilityRegistry::default()),
            redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
            interrupt: InterruptToken::default(),
            parent_run: None,
        },
        permission_context: PermissionContext {
            permission_mode: PermissionMode::Default,
            previous_mode: None,
            session_id,
            tenant_id: TenantId::SINGLE,
            run_id: None,
            interactivity: InteractivityLevel::FullyInteractive,
            timeout_policy: None,
            fallback_policy: FallbackPolicy::DenyAll,
            rule_snapshot: Arc::new(RuleSnapshot {
                rules: vec![],
                generation: 0,
                built_at: chrono::Utc::now(),
            }),
            hook_overrides: vec![],
        },
        blob_store: None,
        event_emitter: Arc::new(NoopToolEventEmitter),
    }
}

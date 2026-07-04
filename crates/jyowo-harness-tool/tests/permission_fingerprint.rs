#![cfg(feature = "builtin-toolset")]

use std::path::Path;
use std::sync::Arc;

use harness_contracts::{
    AgentId, CapabilityRegistry, CorrelationId, DecisionScope, PermissionSubject, TenantId,
    ToolUseId, WorkspaceAccess,
};
use harness_sandbox::{ExecSpec, SandboxBaseConfig, StdioSpec};
use harness_tool::{
    builtin::{BashTool, FileReadTool, WebFetchTool},
    InterruptToken, Tool, ToolContext,
};
use serde_json::json;

#[tokio::test]
async fn builtin_action_plans_use_stable_nonzero_hashes() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(workspace.path().join("src")).unwrap();
    std::fs::write(workspace.path().join("src/lib.rs"), "").unwrap();
    let ctx = tool_ctx(workspace.path());

    let bash = BashTool::default();
    let bash_input =
        json!({ "command": "cargo test", "cwd": "/workspace/./crates/../crates/jyowo" });
    let first_bash = bash.plan(&bash_input, &ctx).await.unwrap();
    let second_bash = bash.plan(&bash_input, &ctx).await.unwrap();

    assert_ne!(
        first_bash.plan_hash,
        harness_contracts::ActionPlanHash::default()
    );
    assert_eq!(first_bash.plan_hash, second_bash.plan_hash);
    assert_command_fingerprint(&first_bash.subject, &first_bash.scope);

    let file_read = FileReadTool::default()
        .plan(&json!({ "path": "src/lib.rs" }), &ctx)
        .await
        .unwrap();
    let web_fetch = WebFetchTool::default()
        .plan(&json!({ "url": "https://example.com:443/docs" }), &ctx)
        .await
        .unwrap();

    assert_ne!(
        file_read.plan_hash,
        harness_contracts::ActionPlanHash::default()
    );
    assert_ne!(
        web_fetch.plan_hash,
        harness_contracts::ActionPlanHash::default()
    );
}

#[tokio::test]
async fn bash_plan_hash_changes_when_canonical_command_changes() {
    let workspace = tempfile::tempdir().unwrap();
    let ctx = tool_ctx(workspace.path());
    let tool = BashTool::default();
    let input = json!({ "command": "cargo test", "cwd": "/workspace/./crates/../crates/jyowo" });

    let first = tool.plan(&input, &ctx).await.unwrap();
    let second = tool.plan(&input, &ctx).await.unwrap();
    let changed = tool
        .plan(&json!({ "command": "cargo test --all" }), &ctx)
        .await
        .unwrap();

    assert_eq!(first.plan_hash, second.plan_hash);
    assert_ne!(first.plan_hash, changed.plan_hash);
    assert_eq!(
        command_fingerprint(&first.subject, &first.scope),
        command_fingerprint(&second.subject, &second.scope)
    );
    assert_ne!(
        command_fingerprint(&first.subject, &first.scope),
        command_fingerprint(&changed.subject, &changed.scope)
    );
}

fn assert_command_fingerprint(subject: &PermissionSubject, scope: &DecisionScope) {
    let fingerprint = command_fingerprint(subject, scope);
    assert_ne!(fingerprint.0, [0; 32]);
}

fn command_fingerprint(
    subject: &PermissionSubject,
    scope: &DecisionScope,
) -> harness_contracts::ExecFingerprint {
    let PermissionSubject::CommandExec {
        command,
        cwd,
        fingerprint: Some(fingerprint),
        ..
    } = subject
    else {
        panic!("expected command subject with fingerprint");
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

    assert_eq!(*fingerprint, expected);
    fingerprint.clone()
}

fn tool_ctx(workspace_root: &Path) -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: harness_contracts::SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: CorrelationId::new(),
        agent_id: AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: workspace_root.to_path_buf(),
        sandbox: None,
        cap_registry: Arc::new(CapabilityRegistry::default()),
        redactor: Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::default(),
        parent_run: None,
        model: None,
        model_config_id: None,
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
    }
}

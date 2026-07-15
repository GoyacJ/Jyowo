#![cfg(feature = "builtin-toolset")]

use std::{path::Path, sync::Arc};

use futures::{future::BoxFuture, StreamExt};
use harness_contracts::{
    ActionResource, CapabilityRegistry, DecisionScope, NetworkAccess, PermissionSubject, TenantId,
    ToolActionPlan, ToolCapability, ToolError, ToolExecutionChannel, ToolGroup, ToolResult,
    ToolUseId, WorkspaceAccess,
};
use harness_tool::{
    builtin::{
        brokered_platform_runtime_capability, browser_runtime_capability, ArtifactTool,
        BrokeredPlatformRuntimeCap, BrokeredPlatformRuntimeRequest, BrowserDevToolsTool,
        BrowserUseTool, FileEditTool, GitPullTool, GitPushTool, GitStageTool, GitStatusTool,
        GlobTool, GrepTool, ImageGenerationTool, ListDirTool, TaskStopTool, TodoTool, WebFetchTool,
    },
    AuthorizedTicketSummary, AuthorizedToolInput, BuiltinToolset, InterruptToken, Tool,
    ToolContext, ToolRegistry,
};
use serde_json::{json, Value};

#[test]
fn default_builtin_toolset_registers_architecture_m0_tools() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .unwrap();

    for name in [
        "FileEdit",
        "Glob",
        "WebFetch",
        "Diagnostics",
        "Todo",
        "TaskStop",
        "FileRead",
        "FileWrite",
        "ListDir",
        "Grep",
        "ReadBlob",
        "WebSearch",
        "ProcessStart",
        "ProcessRead",
        "ProcessStop",
        "Clarify",
        "SendMessage",
        "GitStatus",
        "GitDiff",
        "GitShow",
        "GitLog",
        "GitStage",
        "GitCommit",
        "GitBranch",
        "GitPull",
        "GitPush",
        "Worktree",
        "Session",
        "Artifact",
        "BrowserUse",
        "BrowserDevTools",
        "ComputerUse",
        "ImageGeneration",
        "NotebookEdit",
        "LSP",
        "Automation",
        "Workflow",
    ] {
        assert!(registry.get(name).is_some(), "{name} should be registered");
    }
}

#[test]
fn git_status_descriptor_exposes_git_metadata() {
    let tool = GitStatusTool::default();
    let descriptor = tool.descriptor();

    assert_eq!(descriptor.group, ToolGroup::Git);
    assert!(descriptor.properties.is_read_only);
    assert_eq!(descriptor.metadata.families, ["git"]);
    assert!(descriptor
        .metadata
        .aliases
        .iter()
        .any(|alias| alias == "git status"));
    assert!(descriptor
        .metadata
        .effects
        .iter()
        .any(|effect| effect == "reads_git"));
}

#[test]
fn brokered_platform_descriptors_expose_searchable_metadata() {
    let artifact = ArtifactTool::default();
    assert_eq!(artifact.descriptor().group, ToolGroup::Artifact);
    assert!(!artifact.descriptor().properties.is_read_only);
    assert_eq!(artifact.descriptor().metadata.families, ["artifact"]);
    assert!(artifact
        .descriptor()
        .metadata
        .aliases
        .iter()
        .any(|alias| alias == "artifact"));

    let browser = BrowserUseTool::default();
    assert_eq!(browser.descriptor().group, ToolGroup::Browser);
    assert!(!browser.descriptor().properties.is_read_only);
    assert!(browser
        .descriptor()
        .metadata
        .platforms
        .iter()
        .any(|platform| platform == "codex"));
    assert!(browser
        .descriptor()
        .metadata
        .effects
        .iter()
        .any(|effect| effect == "external_interaction"));

    let devtools = BrowserDevToolsTool::default();
    assert_eq!(devtools.descriptor().group, ToolGroup::Browser);
    assert_eq!(
        devtools.descriptor().required_capabilities,
        vec![browser_runtime_capability()]
    );
    assert!(devtools
        .descriptor()
        .metadata
        .aliases
        .iter()
        .any(|alias| alias == "chrome devtools"));

    assert_eq!(
        browser.descriptor().required_capabilities,
        vec![browser_runtime_capability()]
    );
}

#[tokio::test]
async fn brokered_platform_tool_fails_closed_without_runtime_capability() {
    assert!(matches!(
        execute_error(
            &ImageGenerationTool::default(),
            json!({ "prompt": "diagram" }),
            tool_ctx(CapabilityRegistry::default()),
        )
        .await,
        ToolError::CapabilityMissing(capability)
            if capability == brokered_platform_runtime_capability()
    ));
}

#[tokio::test]
async fn brokered_platform_tool_dispatches_to_runtime_capability() {
    let mut caps = CapabilityRegistry::default();
    caps.install::<dyn BrokeredPlatformRuntimeCap>(
        brokered_platform_runtime_capability(),
        Arc::new(EchoBrokeredPlatformRuntime),
    );

    let result = execute_final(
        &ImageGenerationTool::default(),
        json!({ "prompt": "diagram" }),
        tool_ctx(caps),
    )
    .await;

    let ToolResult::Structured(value) = result else {
        panic!("expected structured brokered platform result");
    };
    assert_eq!(value["tool"], "ImageGeneration");
    assert_eq!(value["input"]["prompt"], "diagram");
}

#[tokio::test]
async fn brokered_platform_execute_uses_authorized_input_snapshot() {
    let mut caps = CapabilityRegistry::default();
    caps.install::<dyn BrokeredPlatformRuntimeCap>(
        brokered_platform_runtime_capability(),
        Arc::new(EchoBrokeredPlatformRuntime),
    );
    let tool = ImageGenerationTool::default();
    let ctx = tool_ctx(caps);
    let plan = tool
        .plan(&json!({ "prompt": "authorized" }), &ctx)
        .await
        .unwrap();
    let authorized = AuthorizedToolInput::new(
        json!({ "prompt": "mutated" }),
        plan.clone(),
        ticket_for(&plan),
    )
    .unwrap();

    let mut stream = tool.execute_authorized(authorized, ctx).await.unwrap();
    let Some(harness_tool::ToolEvent::Final(ToolResult::Structured(value))) = stream.next().await
    else {
        panic!("expected structured brokered platform result");
    };

    assert_eq!(value["input"]["prompt"], "authorized");
}

#[tokio::test]
async fn git_status_plan_is_read_only_fixed_command() {
    let workspace = tempfile::tempdir().unwrap();
    let tool = GitStatusTool::default();
    let ctx = tool_ctx_at(workspace.path(), CapabilityRegistry::default());

    let plan = tool.plan(&json!({}), &ctx).await.unwrap();

    assert_eq!(plan.tool_name, "GitStatus");
    assert_eq!(plan.scope, DecisionScope::ToolName("GitStatus".to_owned()));
    assert!(matches!(
        plan.subject,
        PermissionSubject::ToolInvocation { ref tool, .. } if tool == "GitStatus"
    ));
    assert_eq!(plan.workspace_access, WorkspaceAccess::ReadOnly);
    assert_eq!(plan.network_access, NetworkAccess::None);
    assert_eq!(plan.execution_channel, ToolExecutionChannel::ProcessSandbox);
    assert!(matches!(
        plan.resources.as_slice(),
        [ActionResource::Command {
            command,
            argv,
            cwd: Some(cwd),
            ..
        }] if command == "git"
            && argv == &vec!["status".to_owned(), "--short".to_owned(), "--branch".to_owned()]
            && cwd == workspace.path()
    ));
}

#[tokio::test]
async fn git_stage_plan_requires_exact_command_permission() {
    let workspace = tempfile::tempdir().unwrap();
    let tool = GitStageTool::default();
    let ctx = tool_ctx_at(workspace.path(), CapabilityRegistry::default());

    let plan = tool
        .plan(&json!({ "paths": ["src/lib.rs", "README.md"] }), &ctx)
        .await
        .unwrap();

    assert_eq!(plan.tool_name, "GitStage");
    assert!(matches!(
        plan.scope,
        DecisionScope::ExactCommand {
            ref command,
            cwd: Some(ref cwd)
        } if command == "git add -- src/lib.rs README.md" && cwd == workspace.path()
    ));
    assert!(matches!(
        plan.subject,
        PermissionSubject::CommandExec {
            ref command,
            ref argv,
            cwd: Some(ref cwd),
            fingerprint: Some(_),
        } if command == "git"
            && argv == &vec![
                "add".to_owned(),
                "--".to_owned(),
                "src/lib.rs".to_owned(),
                "README.md".to_owned()
            ]
            && cwd == workspace.path()
    ));
    assert!(matches!(
        plan.workspace_access,
        WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: ref paths
        } if paths.is_empty()
    ));
    assert_eq!(plan.network_access, NetworkAccess::None);
    assert_eq!(plan.execution_channel, ToolExecutionChannel::ProcessSandbox);
    assert!(matches!(
        plan.resources.as_slice(),
        [ActionResource::Command {
            command,
            argv,
            cwd: Some(cwd),
            ..
        }] if command == "git"
            && argv == &vec![
                "add".to_owned(),
                "--".to_owned(),
                "src/lib.rs".to_owned(),
                "README.md".to_owned()
            ]
            && cwd == workspace.path()
    ));
}

#[tokio::test]
async fn git_remote_plans_allow_network_in_the_process_sandbox() {
    let workspace = tempfile::tempdir().unwrap();
    let ctx = tool_ctx_at(workspace.path(), CapabilityRegistry::default());

    for tool in [
        &GitPullTool::default() as &dyn Tool,
        &GitPushTool::default() as &dyn Tool,
    ] {
        let plan = tool.plan(&json!({}), &ctx).await.unwrap();

        assert_eq!(plan.network_access, NetworkAccess::Unrestricted);
        assert_eq!(plan.sandbox_policy.network, NetworkAccess::Unrestricted);
        assert_eq!(plan.execution_channel, ToolExecutionChannel::ProcessSandbox);
    }
}

#[tokio::test]
async fn git_remote_execution_uses_the_authorized_network_policy_fingerprint() {
    if std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let isolation = harness_sandbox::LocalIsolation::for_current_platform();
    if matches!(
        isolation,
        harness_sandbox::LocalIsolation::None | harness_sandbox::LocalIsolation::JobObject
    ) {
        return;
    }

    let workspace = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .arg("init")
        .current_dir(workspace.path())
        .output()
        .unwrap();
    let canonical_workspace = std::fs::canonicalize(workspace.path()).unwrap();
    let tool = GitPullTool::default();
    let mut ctx = tool_ctx_at(&canonical_workspace, CapabilityRegistry::default());
    ctx.sandbox = Some(Arc::new(
        harness_sandbox::LocalSandbox::new(&canonical_workspace).with_isolation(isolation),
    ));
    let input = json!({});
    let plan = tool.plan(&input, &ctx).await.unwrap();
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).unwrap();

    let mut stream = tool.execute_authorized(authorized, ctx).await.unwrap();
    while stream.next().await.is_some() {}
}

#[tokio::test]
async fn git_execute_uses_authorized_command_resource_not_raw_input() {
    if std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }

    let workspace = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .arg("init")
        .current_dir(workspace.path())
        .output()
        .unwrap();
    std::fs::write(workspace.path().join("a.txt"), "a").unwrap();
    std::fs::write(workspace.path().join("b.txt"), "b").unwrap();
    let canonical_workspace = std::fs::canonicalize(workspace.path()).unwrap();

    let tool = GitStageTool::default();
    let isolation = harness_sandbox::LocalIsolation::for_current_platform();
    if matches!(
        isolation,
        harness_sandbox::LocalIsolation::None | harness_sandbox::LocalIsolation::JobObject
    ) {
        return;
    }
    let mut ctx = tool_ctx_at(&canonical_workspace, CapabilityRegistry::default());
    ctx.sandbox = Some(Arc::new(
        harness_sandbox::LocalSandbox::new(&canonical_workspace).with_isolation(isolation),
    ));
    let plan = tool
        .plan(&json!({ "paths": ["a.txt"] }), &ctx)
        .await
        .unwrap();
    let authorized = AuthorizedToolInput::new(
        json!({ "paths": ["b.txt"] }),
        plan.clone(),
        ticket_for(&plan),
    )
    .unwrap();
    let mut stream = tool.execute_authorized(authorized, ctx).await.unwrap();
    let mut final_result = None;
    while let Some(event) = stream.next().await {
        if let harness_tool::ToolEvent::Final(result) = event {
            final_result = Some(result);
            break;
        }
    }
    let Some(ToolResult::Structured(final_result)) = final_result else {
        panic!("expected structured final git result");
    };
    assert_eq!(final_result["success"], true, "result={final_result}");

    let staged = std::process::Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(workspace.path())
        .output()
        .unwrap();
    assert!(staged.status.success());
    assert_eq!(String::from_utf8_lossy(&staged.stdout), "a.txt\n");
}

#[tokio::test]
async fn git_plan_rejects_cwd_outside_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let tool = GitStatusTool::default();
    let ctx = tool_ctx_at(workspace.path(), CapabilityRegistry::default());

    assert!(matches!(
        tool.plan(&json!({ "cwd": outside.path() }), &ctx)
            .await
            .unwrap_err(),
        ToolError::PermissionDenied(_)
    ));
}

#[tokio::test]
async fn missing_capability_tools_fail_closed() {
    assert!(matches!(
        execute_error(
            &TodoTool::default(),
            json!({ "items": [{"content": "review", "status": "pending"}] }),
            tool_ctx(CapabilityRegistry::default()),
        )
        .await,
        ToolError::CapabilityMissing(ToolCapability::TodoStore)
    ));

    assert!(matches!(
        execute_error(
            &TaskStopTool::default(),
            json!({ "reason": "done" }),
            tool_ctx(CapabilityRegistry::default()),
        )
        .await,
        ToolError::CapabilityMissing(ToolCapability::RunCanceller)
    ));
}

#[tokio::test]
async fn file_edit_replaces_text_and_asks_for_path_permission() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("note.txt");
    std::fs::write(&file, "alpha\nbeta\n").unwrap();
    let tool = FileEditTool::default();

    let input = json!({
        "path": file,
        "old": "beta",
        "new": "gamma"
    });
    let plan = tool
        .plan(&input, &tool_ctx(CapabilityRegistry::default()))
        .await;
    assert!(matches!(plan.unwrap().scope, DecisionScope::PathPrefix(_)));

    let result = execute_final(
        &tool,
        input,
        tool_ctx_at(dir.path(), CapabilityRegistry::default()),
    )
    .await;

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\ngamma\n");
    let ToolResult::Structured(value) = result else {
        panic!("expected structured edit result");
    };
    assert_eq!(value["replacements"], 1);
    assert!(value["path"].as_str().unwrap().ends_with("note.txt"));
}

#[tokio::test]
async fn glob_returns_stable_workspace_matches() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/a.rs"), "").unwrap();
    std::fs::write(dir.path().join("src/b.txt"), "").unwrap();
    std::fs::write(dir.path().join("root.rs"), "").unwrap();
    let tool = GlobTool::default();

    let result = execute_final(
        &tool,
        json!({ "path": dir.path(), "pattern": "**/*.rs" }),
        tool_ctx_at(dir.path(), CapabilityRegistry::default()),
    )
    .await;

    let ToolResult::Structured(value) = result else {
        panic!("expected structured glob result");
    };
    let matches = value.as_array().unwrap();
    assert_eq!(matches.len(), 2);
    assert!(matches[0]["path"].as_str().unwrap().ends_with("root.rs"));
    assert!(matches[1]["path"].as_str().unwrap().ends_with("src/a.rs"));
}

#[cfg(unix)]
#[tokio::test]
async fn file_edit_and_glob_reject_workspace_escape_paths_before_fs_access() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let outside_file = root.path().join("outside.txt");
    let outside_dir = root.path().join("outside_dir");
    let outside_ssh_dir = root.path().join(".ssh");
    let empty_parent = root.path().join("empty");
    let empty_outside_ssh_dir = empty_parent.join(".ssh");
    std::fs::create_dir(&outside_dir).unwrap();
    std::fs::create_dir(&outside_ssh_dir).unwrap();
    std::fs::create_dir(&empty_parent).unwrap();
    std::fs::create_dir(&empty_outside_ssh_dir).unwrap();
    let outside_ssh_key = outside_ssh_dir.join("id_rsa");
    std::fs::write(&outside_file, "secret").unwrap();
    std::fs::write(&outside_ssh_key, "private-key").unwrap();
    std::fs::write(outside_dir.join("secret.rs"), "fn secret() {}").unwrap();
    std::os::unix::fs::symlink(&outside_file, workspace.join("link.txt")).unwrap();
    std::os::unix::fs::symlink(&outside_dir, workspace.join("linked_dir")).unwrap();
    let ctx = || tool_ctx_at(&workspace, CapabilityRegistry::default());

    assert!(matches!(
        execute_error(
            &FileEditTool::default(),
            json!({ "path": "../outside.txt", "old": "secret", "new": "changed" }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
    assert!(matches!(
        execute_error(
            &FileEditTool::default(),
            json!({ "path": "link.txt", "old": "secret", "new": "changed" }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
    assert_eq!(std::fs::read_to_string(&outside_file).unwrap(), "secret");
    assert!(matches!(
        execute_error(
            &FileEditTool::default(),
            json!({ "path": outside_ssh_key, "old": "private-key", "new": "changed" }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
    assert_eq!(
        std::fs::read_to_string(outside_ssh_dir.join("id_rsa")).unwrap(),
        "private-key"
    );
    assert!(matches!(
        execute_error(
            &GrepTool::default(),
            json!({ "path": outside_ssh_key, "pattern": "NO_MATCH" }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
    assert!(matches!(
        execute_error(
            &GlobTool::default(),
            json!({ "path": outside_dir, "pattern": "**/*.rs" }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
    assert!(matches!(
        execute_error(
            &GlobTool::default(),
            json!({ "path": "linked_dir", "pattern": "**/*.rs" }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
    assert!(matches!(
        execute_error(
            &GlobTool::default(),
            json!({ "path": empty_outside_ssh_dir, "pattern": "**/*.rs" }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
    assert!(matches!(
        execute_error(
            &ListDirTool::default(),
            json!({ "path": empty_outside_ssh_dir }),
            ctx()
        )
        .await,
        ToolError::PermissionDenied(_)
    ));
}

#[test]
fn descriptors_match_architecture_groups_and_capabilities() {
    let edit = FileEditTool::default();
    assert_eq!(edit.descriptor().group, ToolGroup::FileSystem);
    assert!(edit.descriptor().properties.is_destructive);

    let glob = GlobTool::default();
    assert_eq!(glob.descriptor().group, ToolGroup::Search);
    assert!(glob.descriptor().properties.is_read_only);

    let fetch = WebFetchTool::default();
    assert_eq!(fetch.descriptor().group, ToolGroup::Network);
    assert!(fetch.descriptor().properties.is_read_only);

    let todo = TodoTool::default();
    assert_eq!(
        todo.descriptor().required_capabilities,
        vec![ToolCapability::TodoStore]
    );

    let stop = TaskStopTool::default();
    assert_eq!(
        stop.descriptor().required_capabilities,
        vec![ToolCapability::RunCanceller]
    );
}

async fn execute_final(tool: &dyn Tool, input: Value, ctx: ToolContext) -> ToolResult {
    tool.validate(&input, &ctx).await.unwrap();
    let plan = tool.plan(&input, &ctx).await.unwrap();
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).unwrap();
    let mut stream = tool.execute_authorized(authorized, ctx).await.unwrap();
    match stream.next().await {
        Some(harness_tool::ToolEvent::Final(result)) => result,
        other => panic!("expected final result, got {other:?}"),
    }
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

fn tool_ctx(cap_registry: CapabilityRegistry) -> ToolContext {
    tool_ctx_at(std::env::temp_dir(), cap_registry)
}

fn tool_ctx_at(workspace_root: impl AsRef<Path>, cap_registry: CapabilityRegistry) -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: harness_contracts::SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: workspace_root.as_ref().to_path_buf(),
        project_workspace_root: None,
        sandbox: None,
        cap_registry: Arc::new(cap_registry),
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
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

struct EchoBrokeredPlatformRuntime;

impl BrokeredPlatformRuntimeCap for EchoBrokeredPlatformRuntime {
    fn execute(
        &self,
        request: BrokeredPlatformRuntimeRequest,
    ) -> BoxFuture<'static, Result<Value, ToolError>> {
        Box::pin(async move {
            Ok(json!({
                "tool": request.tool_name,
                "input": request.input,
            }))
        })
    }
}

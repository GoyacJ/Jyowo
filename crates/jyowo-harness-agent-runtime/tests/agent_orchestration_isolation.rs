use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use harness_agent_runtime::{
    CreateWorkspaceIsolationLeaseRequest, WorkspaceIsolationCleanupResult, WorkspaceIsolationError,
    WorkspaceIsolationManager,
};
use harness_contracts::AgentWorkspaceIsolationMode;
use tempfile::{tempdir, TempDir};

fn init_git_repo(path: &Path) {
    run_git(path, ["init"]);
    run_git(path, ["config", "user.email", "test@example.com"]);
    run_git(path, ["config", "user.name", "Test"]);
    fs::write(path.join("README.md"), "hello").expect("write readme");
    run_git(path, ["add", "."]);
    run_git(path, ["commit", "-m", "init"]);
}

fn run_git(cwd: &Path, args: impl IntoIterator<Item = impl AsRef<str>>) {
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect();
    let status = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("git command should spawn");
    assert!(status.success(), "git command failed in {}", cwd.display());
}

fn sample_request(
    conversation_id: &str,
    run_id: &str,
    agent_id: &str,
    mode: AgentWorkspaceIsolationMode,
) -> CreateWorkspaceIsolationLeaseRequest {
    CreateWorkspaceIsolationLeaseRequest {
        conversation_id: conversation_id.to_owned(),
        run_id: run_id.to_owned(),
        agent_id: agent_id.to_owned(),
        mode,
    }
}

#[test]
fn create_git_worktree_lease_persists_metadata() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    init_git_repo(&workspace_root);

    let manager = WorkspaceIsolationManager::open(&workspace_root).expect("manager opens");
    let lease = manager
        .create_lease(sample_request(
            "conversation-1",
            "run-1",
            "agent-1",
            AgentWorkspaceIsolationMode::GitWorktree,
        ))
        .expect("lease should be created");

    assert_eq!(lease.conversation_id, "conversation-1");
    assert_eq!(lease.run_id, "run-1");
    assert_eq!(lease.agent_id, "agent-1");
    assert_eq!(lease.branch.as_deref(), Some("jyowo/agent-agent-1"));
    assert!(lease.base_commit.is_some());
    assert_eq!(lease.status, "active");
    assert!(Path::new(&lease.path).is_dir());
}

#[test]
fn reject_non_git_workspace_for_git_worktree() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    let manager = WorkspaceIsolationManager::open(&workspace_root).expect("manager opens");

    let error = manager
        .create_lease(sample_request(
            "conversation-1",
            "run-1",
            "agent-1",
            AgentWorkspaceIsolationMode::GitWorktree,
        ))
        .unwrap_err();

    assert!(matches!(error, WorkspaceIsolationError::NonGitWorkspace));
}

#[test]
fn reject_duplicate_branch_lease() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    init_git_repo(&workspace_root);

    let manager = WorkspaceIsolationManager::open(&workspace_root).expect("manager opens");
    manager
        .create_lease(sample_request(
            "conversation-1",
            "run-1",
            "agent-1",
            AgentWorkspaceIsolationMode::GitWorktree,
        ))
        .expect("first lease");

    let error = manager
        .create_lease(sample_request(
            "conversation-2",
            "run-2",
            "agent-1",
            AgentWorkspaceIsolationMode::GitWorktree,
        ))
        .unwrap_err();

    assert!(matches!(
        error,
        WorkspaceIsolationError::DuplicateBranchLease { branch } if branch == "jyowo/agent-agent-1"
    ));
}

#[test]
fn detect_dirty_worktree_on_cleanup() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    init_git_repo(&workspace_root);

    let manager = WorkspaceIsolationManager::open(&workspace_root).expect("manager opens");
    let lease = manager
        .create_lease(sample_request(
            "conversation-1",
            "run-1",
            "agent-1",
            AgentWorkspaceIsolationMode::GitWorktree,
        ))
        .expect("lease should be created");

    fs::write(Path::new(&lease.path).join("dirty.txt"), "change").expect("write dirty file");

    let result = manager
        .cleanup_lease(&lease.lease_id)
        .expect("cleanup should block on dirty worktree");
    assert!(matches!(
        result,
        WorkspaceIsolationCleanupResult::CleanupBlocked { .. }
    ));

    let stored = manager
        .get_lease(&lease.lease_id)
        .expect("lease lookup")
        .expect("lease exists");
    assert_eq!(stored.status, "cleanup_blocked");
}

#[test]
fn resume_lease_metadata_after_reopening_store() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_temp_root(&workspace);
    init_git_repo(&workspace_root);

    let lease_id = {
        let manager = WorkspaceIsolationManager::open(&workspace_root).expect("manager opens");
        manager
            .create_lease(sample_request(
                "conversation-1",
                "run-1",
                "agent-1",
                AgentWorkspaceIsolationMode::GitWorktree,
            ))
            .expect("lease should be created")
            .lease_id
    };

    let reopened = WorkspaceIsolationManager::open(&workspace_root).expect("manager reopens");
    let lease = reopened
        .get_lease(&lease_id)
        .expect("lease lookup")
        .expect("lease exists");
    assert_eq!(lease.conversation_id, "conversation-1");
    assert_eq!(lease.agent_id, "agent-1");
    assert_eq!(lease.status, "active");
}

fn canonical_temp_root(temp: &TempDir) -> PathBuf {
    temp.path().canonicalize().expect("canonical tempdir")
}

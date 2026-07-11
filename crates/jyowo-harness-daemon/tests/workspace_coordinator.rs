use std::sync::Arc;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

use chrono::{Duration, Utc};
use harness_contracts::{ActorId, ClientId, CommandId, TaskId, WorkspaceMode};
use harness_daemon::{
    WorkspaceAccess, WorkspaceAcquireOutcome, WorkspaceCleanupOutcome, WorkspaceCoordinator,
    WorkspaceCoordinatorError, WorkspaceExecutionKind, WorkspaceLeaseRequest,
    WorkspaceOverrideCommand, WorkspaceToolAction,
};
use harness_journal::{AcceptedCommand, CommandOutcome, NewTaskEvent, TaskStore};
use serde_json::json;

#[path = "workspace_coordinator/workspace_dispatch_cases.rs"]
mod workspace_dispatch_cases;
#[path = "workspace_coordinator/workspace_lease_cases.rs"]
mod workspace_lease_cases;

fn acquired(outcome: WorkspaceAcquireOutcome) -> harness_journal::TaskWorkspaceLease {
    match outcome {
        WorkspaceAcquireOutcome::Acquired(lease) => lease,
        WorkspaceAcquireOutcome::Waiting(_) => panic!("expected an acquired workspace lease"),
    }
}

fn waiting(outcome: WorkspaceAcquireOutcome) -> harness_journal::TaskWorkspaceLease {
    match outcome {
        WorkspaceAcquireOutcome::Waiting(lease) => lease,
        WorkspaceAcquireOutcome::Acquired(_) => panic!("expected a waiting workspace lease"),
    }
}

fn request(
    task_id: TaskId,
    root: &std::path::Path,
    mode: Option<WorkspaceMode>,
    access: WorkspaceAccess,
    execution_kind: WorkspaceExecutionKind,
) -> WorkspaceLeaseRequest {
    WorkspaceLeaseRequest {
        task_id,
        actor_id: ActorId::new(),
        root: root.to_path_buf(),
        mode,
        access,
        execution_kind,
        expires_at: None,
    }
}

fn create_task(store: &TaskStore, title: &str) -> TaskId {
    let task_id = TaskId::new();
    let outcome = store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: format!("create-{task_id}"),
                expected_stream_version: 0,
                authority: TaskStore::user_authority(ClientId::new()),
                payload: json!({ "type": "create_task" }),
            },
            |_| Ok(vec![NewTaskEvent::task_created(title)]),
        )
        .unwrap();
    assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
    task_id
}

fn init_git_repo(path: &Path) {
    git(path, ["init"]);
    git(path, ["config", "user.email", "test@example.com"]);
    git(path, ["config", "user.name", "Test"]);
    std::fs::write(path.join("README.md"), "initial\n").unwrap();
    git(path, ["add", "."]);
    git(path, ["commit", "-m", "initial"]);
}

fn git(cwd: &Path, args: impl IntoIterator<Item = impl AsRef<str>>) {
    let output = git_command(cwd, args).output().unwrap();
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(cwd: &Path, args: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    let output = git_command(cwd, args).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn git_command(cwd: &Path, args: impl IntoIterator<Item = impl AsRef<str>>) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(cwd).args(
        args.into_iter()
            .map(|arg| arg.as_ref().to_owned())
            .collect::<Vec<_>>(),
    );
    command.env("GIT_TERMINAL_PROMPT", "0");
    command
}

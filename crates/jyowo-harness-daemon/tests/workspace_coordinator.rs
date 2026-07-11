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

#[test]
fn two_read_only_current_workspace_tasks_may_coexist() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let first_task = create_task(&store, "first reader");
    let second_task = create_task(&store, "second reader");
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();

    let first = coordinator
        .acquire(request(
            first_task,
            &workspace,
            Some(WorkspaceMode::Current),
            WorkspaceAccess::ReadOnly,
            WorkspaceExecutionKind::Foreground,
        ))
        .unwrap();
    let second = coordinator
        .acquire(request(
            second_task,
            &workspace,
            Some(WorkspaceMode::Current),
            WorkspaceAccess::ReadOnly,
            WorkspaceExecutionKind::Foreground,
        ))
        .unwrap();

    assert!(matches!(first, WorkspaceAcquireOutcome::Acquired(_)));
    assert!(matches!(second, WorkspaceAcquireOutcome::Acquired(_)));
    let active = coordinator.active_for_root(&workspace).unwrap();
    assert_eq!(active.len(), 2);
    assert!(active.iter().all(|lease| !lease.writable));
}

#[test]
fn current_workspace_writers_wait_and_acquire_in_fifo_order() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let tasks = [
        create_task(&store, "writer one"),
        create_task(&store, "writer two"),
        create_task(&store, "writer three"),
    ];
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();

    let first = acquired(
        coordinator
            .acquire(request(
                tasks[0],
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    let second = waiting(
        coordinator
            .acquire(request(
                tasks[1],
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    let third = waiting(
        coordinator
            .acquire(request(
                tasks[2],
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );

    let release = coordinator.release(first.lease_id).unwrap();
    assert_eq!(
        release.acquired.first().map(|lease| lease.lease_id),
        Some(second.lease_id)
    );
    let release = coordinator.release(second.lease_id).unwrap();
    assert_eq!(
        release.acquired.first().map(|lease| lease.lease_id),
        Some(third.lease_id)
    );
}

#[test]
fn waiting_writer_blocks_read_upgrade_and_new_reader_leapfrogging() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let reader_task = create_task(&store, "reader");
    let later_reader_task = create_task(&store, "later reader");
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();

    let reader = acquired(
        coordinator
            .acquire(request(
                reader_task,
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::ReadOnly,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    let writer = waiting(
        coordinator
            .acquire(request(
                reader_task,
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    waiting(
        coordinator
            .acquire(request(
                later_reader_task,
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::ReadOnly,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );

    let release = coordinator.release(reader.lease_id).unwrap();
    assert_eq!(
        release.acquired.first().map(|lease| lease.lease_id),
        Some(writer.lease_id)
    );
}

#[test]
fn expired_owner_is_released_visibly_and_next_writer_acquires() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let owner = create_task(&store, "crashed owner");
    let successor = create_task(&store, "successor");
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let expiry = Utc::now() + Duration::seconds(5);
    let mut owner_request = request(
        owner,
        &workspace,
        Some(WorkspaceMode::Current),
        WorkspaceAccess::Write,
        WorkspaceExecutionKind::Foreground,
    );
    owner_request.expires_at = Some(expiry);
    let owner_lease = acquired(coordinator.acquire(owner_request).unwrap());
    let successor_lease = waiting(
        coordinator
            .acquire(request(
                successor,
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );

    let expired = coordinator
        .expire_stale(expiry + Duration::milliseconds(1))
        .unwrap();
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].released.lease_id, owner_lease.lease_id);
    assert_eq!(
        expired[0].released.state,
        harness_journal::TaskWorkspaceLeaseState::Expired
    );
    assert_eq!(
        expired[0].acquired.first().map(|lease| lease.lease_id),
        Some(successor_lease.lease_id)
    );
    let events = store.task_events_after(owner, 0, 16).unwrap();
    assert!(events.iter().any(|event| {
        event.event_type == "workspace.released" && event.payload["reason"] == "owner_expired"
    }));
}

#[test]
fn expired_waiting_writer_does_not_block_later_readers() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let owner = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "owner"),
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::ReadOnly,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    let expiry = Utc::now() + Duration::seconds(1);
    let mut writer_request = request(
        create_task(&store, "expired writer"),
        &workspace,
        Some(WorkspaceMode::Current),
        WorkspaceAccess::Write,
        WorkspaceExecutionKind::Foreground,
    );
    writer_request.expires_at = Some(expiry);
    let writer = waiting(coordinator.acquire(writer_request).unwrap());

    coordinator
        .expire_stale(expiry + Duration::milliseconds(1))
        .unwrap();
    assert_eq!(
        store
            .workspace_lease(writer.lease_id)
            .unwrap()
            .unwrap()
            .state,
        harness_journal::TaskWorkspaceLeaseState::Expired
    );
    assert!(matches!(
        coordinator
            .acquire(request(
                create_task(&store, "later reader"),
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::ReadOnly,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
        WorkspaceAcquireOutcome::Acquired(_)
    ));
    coordinator.release(owner.lease_id).unwrap();
}

#[test]
fn releasing_writer_reports_every_promoted_reader() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let writer = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "writer"),
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    for title in ["reader one", "reader two"] {
        waiting(
            coordinator
                .acquire(request(
                    create_task(&store, title),
                    &workspace,
                    Some(WorkspaceMode::Current),
                    WorkspaceAccess::ReadOnly,
                    WorkspaceExecutionKind::Foreground,
                ))
                .unwrap(),
        );
    }
    assert_eq!(
        coordinator.release(writer.lease_id).unwrap().acquired.len(),
        2
    );
}

#[test]
fn background_and_parallel_children_default_to_distinct_managed_worktrees() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    init_git_repo(&workspace);
    let baseline = git_output(&workspace, ["rev-parse", "HEAD"]);
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let background = create_task(&store, "background");
    let child = create_task(&store, "parallel child");
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();

    let background_lease = acquired(
        coordinator
            .acquire(request(
                background,
                &workspace,
                None,
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Background,
            ))
            .unwrap(),
    );
    let child_lease = acquired(
        coordinator
            .acquire(request(
                child,
                &workspace,
                None,
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::ParallelChild,
            ))
            .unwrap(),
    );

    assert_eq!(background_lease.mode, WorkspaceMode::ManagedWorktree);
    assert_eq!(child_lease.mode, WorkspaceMode::ManagedWorktree);
    assert_ne!(background_lease.worktree_path, child_lease.worktree_path);
    for lease in [&background_lease, &child_lease] {
        assert!(Path::new(lease.worktree_path.as_deref().unwrap()).is_dir());
        assert_eq!(lease.baseline_commit.as_deref(), Some(baseline.as_str()));
        assert_eq!(lease.baseline_status, "");
    }
}

#[test]
fn coordinator_recovers_a_durable_preparing_worktree() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let managed = root.path().join("managed-worktrees");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&managed).unwrap();
    init_git_repo(&workspace);
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task = create_task(&store, "recover preparing");
    let lease_id = harness_contracts::WorkspaceLeaseId::new();
    let path = managed.join(lease_id.to_string());
    store
        .prepare_managed_workspace_lease(harness_journal::AcquireTaskWorkspaceLease {
            lease_id,
            task_id: task,
            actor_id: ActorId::new(),
            mode: WorkspaceMode::ManagedWorktree,
            canonical_root: workspace.to_str().unwrap().to_owned(),
            worktree_path: Some(path.to_str().unwrap().to_owned()),
            branch: Some(format!("jyowo/task-{lease_id}")),
            writable: true,
            requested_at: Utc::now(),
            expires_at: None,
            baseline_commit: Some(git_output(&workspace, ["rev-parse", "HEAD"])),
            baseline_status: String::new(),
        })
        .unwrap();

    WorkspaceCoordinator::new(Arc::clone(&store), managed.clone()).unwrap();

    assert!(path.is_dir());
    assert_eq!(
        store.workspace_lease(lease_id).unwrap().unwrap().state,
        harness_journal::TaskWorkspaceLeaseState::Active
    );
}

#[test]
fn coordinator_replaces_untrusted_directory_before_activating_preparing_lease() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let managed = root.path().join("managed-worktrees");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&managed).unwrap();
    init_git_repo(&workspace);
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let lease_id = harness_contracts::WorkspaceLeaseId::new();
    let path = managed.join(lease_id.to_string());
    store
        .prepare_managed_workspace_lease(harness_journal::AcquireTaskWorkspaceLease {
            lease_id,
            task_id: create_task(&store, "untrusted preparing path"),
            actor_id: ActorId::new(),
            mode: WorkspaceMode::ManagedWorktree,
            canonical_root: workspace.to_str().unwrap().to_owned(),
            worktree_path: Some(path.to_str().unwrap().to_owned()),
            branch: Some(format!("jyowo/task-{lease_id}")),
            writable: true,
            requested_at: Utc::now(),
            expires_at: None,
            baseline_commit: Some(git_output(&workspace, ["rev-parse", "HEAD"])),
            baseline_status: String::new(),
        })
        .unwrap();
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("attacker-marker"), "not a worktree").unwrap();

    WorkspaceCoordinator::new(Arc::clone(&store), managed).unwrap();

    assert!(!path.join("attacker-marker").exists());
    assert_eq!(
        PathBuf::from(git_output(&path, ["rev-parse", "--show-toplevel"]))
            .canonicalize()
            .unwrap(),
        path.canonicalize().unwrap()
    );
    assert_eq!(
        store.workspace_lease(lease_id).unwrap().unwrap().state,
        harness_journal::TaskWorkspaceLeaseState::Active
    );
}

#[test]
fn coordinator_finishes_a_durable_cleanup_pending_transition() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let managed = root.path().join("managed-worktrees");
    std::fs::create_dir(&workspace).unwrap();
    init_git_repo(&workspace);
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator = WorkspaceCoordinator::new(Arc::clone(&store), managed.clone()).unwrap();
    let lease = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "recover cleanup"),
                &workspace,
                Some(WorkspaceMode::ManagedWorktree),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    let path = PathBuf::from(lease.worktree_path.as_deref().unwrap());
    store
        .mark_workspace_cleanup_pending(lease.lease_id)
        .unwrap();
    drop(coordinator);

    WorkspaceCoordinator::new(Arc::clone(&store), managed.clone()).unwrap();

    assert!(!path.exists());
    assert_eq!(
        store
            .workspace_lease(lease.lease_id)
            .unwrap()
            .unwrap()
            .state,
        harness_journal::TaskWorkspaceLeaseState::Released
    );
}

#[test]
fn coordinator_recovers_dirty_cleanup_pending_without_losing_changes() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let managed = root.path().join("managed-worktrees");
    std::fs::create_dir(&workspace).unwrap();
    init_git_repo(&workspace);
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator = WorkspaceCoordinator::new(Arc::clone(&store), managed.clone()).unwrap();
    let lease = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "recover dirty cleanup"),
                &workspace,
                Some(WorkspaceMode::ManagedWorktree),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    let path = PathBuf::from(lease.worktree_path.as_deref().unwrap());
    std::fs::write(path.join("README.md"), "changed before crash\n").unwrap();
    std::fs::write(path.join("untracked.txt"), "retain before crash\n").unwrap();
    store
        .mark_workspace_cleanup_pending(lease.lease_id)
        .unwrap();
    drop(coordinator);

    WorkspaceCoordinator::new(Arc::clone(&store), managed.clone()).unwrap();

    let recovered = store.workspace_lease(lease.lease_id).unwrap().unwrap();
    assert_eq!(
        recovered.state,
        harness_journal::TaskWorkspaceLeaseState::CleanupBlocked
    );
    assert!(path.is_dir());
    let patch_path = PathBuf::from(recovered.patch_path.unwrap());
    let patch = std::fs::read_to_string(patch_path).unwrap();
    assert!(patch.contains("changed before crash"));
    assert!(patch.contains("retain before crash"));
}

#[test]
fn current_git_workspace_records_commit_and_status() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    init_git_repo(&workspace);
    std::fs::write(workspace.join("README.md"), "changed\n").unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let lease = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "current git"),
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::ReadOnly,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    assert_eq!(
        lease.baseline_commit.as_deref(),
        Some(git_output(&workspace, ["rev-parse", "HEAD"]).as_str())
    );
    assert!(lease.baseline_status.contains("README.md"));
}

#[test]
fn non_git_workspace_rejects_managed_mode_but_allows_current_mode() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let managed_task = create_task(&store, "managed");
    let current_task = create_task(&store, "current");
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();

    let error = coordinator
        .acquire(request(
            managed_task,
            &workspace,
            Some(WorkspaceMode::ManagedWorktree),
            WorkspaceAccess::Write,
            WorkspaceExecutionKind::Foreground,
        ))
        .unwrap_err();
    assert!(matches!(
        error,
        WorkspaceCoordinatorError::Isolation(
            harness_agent_runtime::WorkspaceIsolationError::NonGitWorkspace
        )
    ));
    acquired(
        coordinator
            .acquire(request(
                current_task,
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
}

#[test]
fn dirty_managed_worktree_retains_patch_and_emits_cleanup_blocked() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    init_git_repo(&workspace);
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task = create_task(&store, "dirty managed worktree");
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let lease = acquired(
        coordinator
            .acquire(request(
                task,
                &workspace,
                Some(WorkspaceMode::ManagedWorktree),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    let worktree = Path::new(lease.worktree_path.as_deref().unwrap());
    std::fs::write(worktree.join("README.md"), "changed\n").unwrap();
    std::fs::write(worktree.join("untracked.txt"), "retain me\n").unwrap();

    let result = coordinator.cleanup_managed(lease.lease_id).unwrap();
    let WorkspaceCleanupOutcome::CleanupBlocked { lease, patch_path } = result else {
        panic!("dirty worktree must block cleanup");
    };
    assert_eq!(
        lease.state,
        harness_journal::TaskWorkspaceLeaseState::CleanupBlocked
    );
    assert!(worktree.is_dir());
    let patch = std::fs::read_to_string(&patch_path).unwrap();
    assert!(patch.contains("changed"));
    assert!(patch.contains("untracked.txt"));
    assert!(patch.contains("retain me"));
    let events = store.task_events_after(task, 0, 16).unwrap();
    assert!(events
        .iter()
        .any(|event| event.event_type == "workspace.cleanup_blocked"));
}

#[cfg(unix)]
#[test]
fn patch_capture_replaces_symlink_without_overwriting_its_target() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let managed = root.path().join("managed-worktrees");
    std::fs::create_dir(&workspace).unwrap();
    init_git_repo(&workspace);
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator = WorkspaceCoordinator::new(Arc::clone(&store), managed.clone()).unwrap();
    let lease = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "secure patch"),
                &workspace,
                Some(WorkspaceMode::ManagedWorktree),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    let worktree = Path::new(lease.worktree_path.as_deref().unwrap());
    std::fs::write(worktree.join("README.md"), "changed\n").unwrap();
    let outside = root.path().join("outside.txt");
    std::fs::write(&outside, "preserve").unwrap();
    let patch = managed.join(format!("{}.patch", lease.lease_id));
    symlink(&outside, &patch).unwrap();

    coordinator.cleanup_managed(lease.lease_id).unwrap();

    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "preserve");
    assert!(std::fs::symlink_metadata(&patch)
        .unwrap()
        .file_type()
        .is_file());
}

#[test]
fn cleanup_blocked_can_be_retried_after_changes_are_removed() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    init_git_repo(&workspace);
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task = create_task(&store, "retry cleanup");
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let lease = acquired(
        coordinator
            .acquire(request(
                task,
                &workspace,
                Some(WorkspaceMode::ManagedWorktree),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    let worktree = Path::new(lease.worktree_path.as_deref().unwrap());
    std::fs::write(worktree.join("README.md"), "changed\n").unwrap();
    assert!(matches!(
        coordinator.cleanup_managed(lease.lease_id).unwrap(),
        WorkspaceCleanupOutcome::CleanupBlocked { .. }
    ));
    git(worktree, ["reset", "--hard", "HEAD"]);
    assert!(matches!(
        coordinator.cleanup_managed(lease.lease_id).unwrap(),
        WorkspaceCleanupOutcome::Released(_)
    ));
    assert!(!worktree.exists());
}

#[test]
fn ordinary_managed_release_runs_cleanup_instead_of_leaking_worktree() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    init_git_repo(&workspace);
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let lease = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "managed release"),
                &workspace,
                Some(WorkspaceMode::ManagedWorktree),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    let path = PathBuf::from(lease.worktree_path.as_deref().unwrap());

    coordinator.release(lease.lease_id).unwrap();

    assert!(!path.exists());
    assert_eq!(
        store
            .workspace_lease(lease.lease_id)
            .unwrap()
            .unwrap()
            .state,
        harness_journal::TaskWorkspaceLeaseState::Released
    );
}

#[test]
fn expired_dirty_managed_lease_retains_patch() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    init_git_repo(&workspace);
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let expiry = Utc::now() + Duration::seconds(1);
    let mut lease_request = request(
        create_task(&store, "expired dirty managed"),
        &workspace,
        Some(WorkspaceMode::ManagedWorktree),
        WorkspaceAccess::Write,
        WorkspaceExecutionKind::Foreground,
    );
    lease_request.expires_at = Some(expiry);
    let lease = acquired(coordinator.acquire(lease_request).unwrap());
    let path = PathBuf::from(lease.worktree_path.as_deref().unwrap());
    std::fs::write(path.join("README.md"), "changed\n").unwrap();

    coordinator
        .expire_stale(expiry + Duration::milliseconds(1))
        .unwrap();

    let retained = store.workspace_lease(lease.lease_id).unwrap().unwrap();
    assert_eq!(
        retained.state,
        harness_journal::TaskWorkspaceLeaseState::CleanupBlocked
    );
    assert!(Path::new(retained.patch_path.as_deref().unwrap()).is_file());
    assert!(path.exists());
}

#[test]
fn coordinator_recovers_an_expired_dirty_managed_lease() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let managed = root.path().join("managed-worktrees");
    std::fs::create_dir(&workspace).unwrap();
    init_git_repo(&workspace);
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator = WorkspaceCoordinator::new(Arc::clone(&store), managed.clone()).unwrap();
    let expiry = Utc::now() + Duration::seconds(1);
    let mut lease_request = request(
        create_task(&store, "expired before cleanup"),
        &workspace,
        Some(WorkspaceMode::ManagedWorktree),
        WorkspaceAccess::Write,
        WorkspaceExecutionKind::Foreground,
    );
    lease_request.expires_at = Some(expiry);
    let lease = acquired(coordinator.acquire(lease_request).unwrap());
    let path = PathBuf::from(lease.worktree_path.as_deref().unwrap());
    std::fs::write(path.join("README.md"), "changed before restart\n").unwrap();
    store
        .expire_workspace_leases(expiry + Duration::milliseconds(1))
        .unwrap();
    drop(coordinator);

    WorkspaceCoordinator::new(Arc::clone(&store), managed).unwrap();

    let recovered = store.workspace_lease(lease.lease_id).unwrap().unwrap();
    assert_eq!(
        recovered.state,
        harness_journal::TaskWorkspaceLeaseState::CleanupBlocked
    );
    assert!(path.exists());
    assert!(std::fs::read_to_string(recovered.patch_path.unwrap())
        .unwrap()
        .contains("changed before restart"));
}

#[cfg(unix)]
#[tokio::test]
async fn write_dispatch_requires_exclusive_lease_and_rejects_path_escape() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let outside = root.path().join("outside");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(workspace.join("inside.txt"), "inside").unwrap();
    std::fs::write(outside.join("secret.txt"), "outside").unwrap();
    symlink(&outside, workspace.join("escape")).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let reader_task = create_task(&store, "reader");
    let writer_task = create_task(&store, "writer");
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let reader = acquired(
        coordinator
            .acquire(request(
                reader_task,
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::ReadOnly,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    let writer = waiting(
        coordinator
            .acquire(request(
                writer_task,
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );

    coordinator
        .dispatch_tool(
            reader.lease_id,
            WorkspaceToolAction::ReadPath(workspace.join("inside.txt")),
            |_| async {},
        )
        .await
        .unwrap();
    assert!(matches!(
        coordinator
            .dispatch_tool(
                reader.lease_id,
                WorkspaceToolAction::WritePath(workspace.join("inside.txt")),
                |_| async {},
            )
            .await,
        Err(WorkspaceCoordinatorError::ExclusiveWriteLeaseRequired { .. })
    ));
    assert!(matches!(
        coordinator
            .dispatch_tool(
                writer.lease_id,
                WorkspaceToolAction::WritePath(workspace.join("inside.txt")),
                |_| async {},
            )
            .await,
        Err(WorkspaceCoordinatorError::InactiveLease { .. })
    ));

    let promoted = coordinator
        .release(reader.lease_id)
        .unwrap()
        .acquired
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(promoted.lease_id, writer.lease_id);
    coordinator
        .dispatch_tool(
            writer.lease_id,
            WorkspaceToolAction::WritePath(workspace.join("new.txt")),
            |_| async {},
        )
        .await
        .unwrap();
    for escaped in [
        workspace.join("escape/secret.txt"),
        outside.join("secret.txt"),
    ] {
        assert!(matches!(
            coordinator
                .dispatch_tool(
                    writer.lease_id,
                    WorkspaceToolAction::WritePath(escaped),
                    |_| async {},
                )
                .await,
            Err(WorkspaceCoordinatorError::PathEscapesWorkspace { .. })
        ));
    }
}

#[tokio::test]
async fn dispatch_boundary_never_calls_executor_without_an_active_lease() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let target = workspace.join("target.txt");
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator = Arc::new(
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap(),
    );
    let lease = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "writer"),
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    coordinator.release(lease.lease_id).unwrap();
    let called = std::cell::Cell::new(false);
    assert!(coordinator
        .dispatch_tool(
            lease.lease_id,
            WorkspaceToolAction::WritePath(target),
            |_| async { called.set(true) },
        )
        .await
        .is_err());
    assert!(!called.get());
}

#[tokio::test]
async fn command_dispatch_fails_closed_without_an_os_sandbox() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let lease = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "command"),
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    let called = std::cell::Cell::new(false);

    assert!(matches!(
        coordinator
            .dispatch_tool(
                lease.lease_id,
                WorkspaceToolAction::Command {
                    cwd: workspace.clone(),
                },
                |_| async { called.set(true) },
            )
            .await,
        Err(WorkspaceCoordinatorError::SandboxedCommandRequired)
    ));
    assert!(!called.get());
}

#[tokio::test]
async fn in_flight_dispatch_fences_release_until_execution_finishes() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator = Arc::new(
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap(),
    );
    let lease = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "writer"),
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
    let worker = Arc::clone(&coordinator);
    let target = workspace.join("target.txt");
    let handle = tokio::spawn(async move {
        worker
            .dispatch_tool(
                lease.lease_id,
                WorkspaceToolAction::WritePath(target),
                |_| async move {
                    entered_tx.send(()).unwrap();
                    finish_rx.await.unwrap();
                },
            )
            .await
            .unwrap();
    });
    entered_rx.await.unwrap();
    assert!(coordinator.release(lease.lease_id).is_err());
    finish_tx.send(()).unwrap();
    handle.await.unwrap();
    coordinator.release(lease.lease_id).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn dispatched_write_rejects_a_directory_replaced_by_a_symlink() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let nested = workspace.join("nested");
    let moved = workspace.join("moved");
    let outside = root.path().join("outside");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let lease = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "writer"),
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );

    let outside_target = outside.join("target.txt");
    let result = coordinator
        .dispatch_tool(
            lease.lease_id,
            WorkspaceToolAction::WritePath(nested.join("target.txt")),
            |authorization| async move {
                std::fs::rename(&nested, &moved).unwrap();
                symlink(&outside, &nested).unwrap();
                authorization.write_bytes(b"must stay inside")
            },
        )
        .await
        .unwrap();

    assert!(result.is_err());
    assert!(!outside_target.exists());
}

#[tokio::test]
async fn workspace_file_capability_expires_when_dispatch_returns() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let target = workspace.join("target.txt");
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let lease = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "writer"),
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );

    let authorization = coordinator
        .dispatch_tool(
            lease.lease_id,
            WorkspaceToolAction::WritePath(target.clone()),
            |authorization| async move { authorization },
        )
        .await
        .unwrap();

    assert!(authorization.write_bytes(b"after dispatch").is_err());
    assert!(!target.exists());
}

#[tokio::test]
async fn workspace_read_rejects_files_over_the_memory_limit() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let target = workspace.join("large.bin");
    let file = std::fs::File::create(&target).unwrap();
    file.set_len(16 * 1024 * 1024 + 1).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let lease = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "reader"),
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::ReadOnly,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );

    let result = coordinator
        .dispatch_tool(
            lease.lease_id,
            WorkspaceToolAction::ReadPath(target),
            |authorization| async move { authorization.read_bytes() },
        )
        .await
        .unwrap();

    assert!(matches!(
        result,
        Err(WorkspaceCoordinatorError::WorkspaceReadLimitExceeded { .. })
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn replacing_the_leased_root_with_a_symlink_invalidates_authorization() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let moved = root.path().join("moved");
    let outside = root.path().join("outside");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let lease = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "writer"),
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    std::fs::rename(&workspace, &moved).unwrap();
    symlink(&outside, &workspace).unwrap();
    assert!(matches!(
        coordinator
            .dispatch_tool(
                lease.lease_id,
                WorkspaceToolAction::WritePath(workspace.join("escape.txt")),
                |_| async {},
            )
            .await,
        Err(WorkspaceCoordinatorError::PathEscapesWorkspace { .. })
    ));
    assert!(matches!(
        coordinator
            .dispatch_override(
                WorkspaceOverrideCommand {
                    command_id: CommandId::new(),
                    task_id: lease.task_id,
                    expected_stream_version: store.stream_version(lease.task_id).unwrap(),
                    lease_id: lease.lease_id,
                    path: workspace.join("escape.txt"),
                    reason: "explicit test override".into(),
                    authority: TaskStore::user_authority(ClientId::new()),
                },
                |_| async {},
            )
            .await,
        Err(WorkspaceCoordinatorError::PathEscapesWorkspace { .. })
    ));
}

#[tokio::test]
async fn explicit_write_override_is_a_separate_audited_command() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let target = workspace.join("shared.txt");
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task = create_task(&store, "override");
    let other = create_task(&store, "other reader");
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let lease = acquired(
        coordinator
            .acquire(request(
                task,
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::ReadOnly,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    acquired(
        coordinator
            .acquire(request(
                other,
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::ReadOnly,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    assert!(matches!(
        coordinator
            .dispatch_tool(
                lease.lease_id,
                WorkspaceToolAction::WritePath(target.clone()),
                |_| async {},
            )
            .await,
        Err(WorkspaceCoordinatorError::ExclusiveWriteLeaseRequired { .. })
    ));

    let command_id = CommandId::new();
    let relative_path = coordinator
        .dispatch_override(
            WorkspaceOverrideCommand {
                command_id,
                task_id: task,
                expected_stream_version: store.stream_version(task).unwrap(),
                lease_id: lease.lease_id,
                path: target.clone(),
                reason: "user explicitly approved shared write".into(),
                authority: TaskStore::user_authority(ClientId::new()),
            },
            |authorization| async move { authorization.relative_path().to_path_buf() },
        )
        .await
        .unwrap();
    assert_eq!(relative_path, Path::new("shared.txt"));
    let events = store.task_events_after(task, 0, 16).unwrap();
    let event = events
        .iter()
        .find(|event| event.event_type == "workspace.override_applied")
        .unwrap();
    assert_eq!(event.payload["commandId"], command_id.to_string());
    assert_eq!(event.payload["leaseId"], lease.lease_id.to_string());
    assert_eq!(event.source.kind, harness_contracts::EventSourceKind::User);
}

#[test]
fn lease_lifecycle_is_evented_without_opening_agent_runtime_database() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let owner = create_task(&store, "owner");
    let waiter = create_task(&store, "waiter");
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();

    let owner_lease = acquired(
        coordinator
            .acquire(request(
                owner,
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    waiting(
        coordinator
            .acquire(request(
                waiter,
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    coordinator.release(owner_lease.lease_id).unwrap();

    let owner_events = store.task_events_after(owner, 0, 16).unwrap();
    assert!(owner_events
        .iter()
        .any(|event| event.event_type == "workspace.acquired"));
    assert!(owner_events
        .iter()
        .any(|event| event.event_type == "workspace.released"));
    let acquired_event = owner_events
        .iter()
        .find(|event| event.event_type == "workspace.acquired")
        .unwrap();
    assert_eq!(
        acquired_event.payload["actorId"],
        owner_lease.actor_id.to_string()
    );
    assert_eq!(acquired_event.payload["state"], "active");
    let released_event = owner_events
        .iter()
        .find(|event| event.event_type == "workspace.released")
        .unwrap();
    assert_eq!(released_event.payload["lease"]["state"], "released");
    let waiter_events = store.task_events_after(waiter, 0, 16).unwrap();
    assert!(waiter_events
        .iter()
        .any(|event| event.event_type == "workspace.waiting"));
    assert!(waiter_events
        .iter()
        .any(|event| event.event_type == "workspace.acquired"));
    assert!(!workspace
        .join(".jyowo/runtime/agent-runtime.sqlite")
        .exists());
}

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

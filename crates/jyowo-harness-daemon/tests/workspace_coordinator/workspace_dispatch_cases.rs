use super::*;

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
                    requires_write: true,
                },
                |_| async { called.set(true) },
            )
            .await,
        Err(WorkspaceCoordinatorError::SandboxedCommandRequired)
    ));
    assert!(!called.get());
}

#[tokio::test]
async fn read_only_command_dispatch_does_not_require_an_exclusive_write_lease() {
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
                create_task(&store, "read-command"),
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::ReadOnly,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );

    coordinator
        .dispatch_sandboxed_command(
            lease.lease_id,
            workspace.clone(),
            false,
            harness_sandbox::LocalIsolation::Seatbelt,
            |authorization| async move {
                assert!(!authorization.writable);
            },
        )
        .await
        .unwrap();
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

#[cfg(unix)]
#[tokio::test]
async fn dispatched_write_replaces_a_workspace_hardlink_without_mutating_its_peer() {
    use std::os::unix::fs::MetadataExt;

    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let outside = root.path().join("outside.txt");
    let target = workspace.join("target.txt");
    std::fs::write(&outside, "outside").unwrap();
    std::fs::hard_link(&outside, &target).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let lease = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "hardlink writer"),
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );

    coordinator
        .dispatch_tool(
            lease.lease_id,
            WorkspaceToolAction::WritePath(target.clone()),
            |authorization| async move { authorization.write_bytes(b"inside") },
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "outside");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "inside");
    assert_ne!(
        std::fs::metadata(&outside).unwrap().ino(),
        std::fs::metadata(&target).unwrap().ino()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn dispatched_edit_rejects_a_target_swap_between_read_and_replace() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let target = workspace.join("target.txt");
    let moved = workspace.join("moved.txt");
    std::fs::write(&target, "original").unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let lease = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "swap editor"),
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );

    let result = coordinator
        .dispatch_tool(
            lease.lease_id,
            WorkspaceToolAction::WritePath(target.clone()),
            |authorization| async move {
                authorization.edit_bytes(|bytes| {
                    assert_eq!(bytes, b"original");
                    std::fs::rename(&target, &moved)?;
                    std::fs::write(&target, "replacement")?;
                    Ok((b"edited".to_vec(), ()))
                })
            },
        )
        .await
        .unwrap();

    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(workspace.join("target.txt")).unwrap(),
        "replacement"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("moved.txt")).unwrap(),
        "original"
    );
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn concurrent_dispatch_edits_are_serialized_for_the_same_lease_and_path() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let target = workspace.join("target.txt");
    std::fs::write(&target, "zero").unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let coordinator = Arc::new(
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap(),
    );
    let lease = acquired(
        coordinator
            .acquire(request(
                create_task(&store, "concurrent editor"),
                &workspace,
                Some(WorkspaceMode::Current),
                WorkspaceAccess::Write,
                WorkspaceExecutionKind::Foreground,
            ))
            .unwrap(),
    );
    let (first_entered_tx, first_entered_rx) = std::sync::mpsc::channel();
    let (release_first_tx, release_first_rx) = std::sync::mpsc::channel();
    let first_coordinator = Arc::clone(&coordinator);
    let first_target = target.clone();
    let first = tokio::spawn(async move {
        first_coordinator
            .dispatch_tool(
                lease.lease_id,
                WorkspaceToolAction::WritePath(first_target),
                |authorization| async move {
                    authorization.edit_bytes(|bytes| {
                        assert_eq!(bytes, b"zero");
                        first_entered_tx.send(()).unwrap();
                        release_first_rx.recv().unwrap();
                        Ok((b"first".to_vec(), ()))
                    })
                },
            )
            .await
            .unwrap()
            .unwrap();
    });
    first_entered_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();

    let second_entered = Arc::new(AtomicBool::new(false));
    let second_flag = Arc::clone(&second_entered);
    let second_coordinator = Arc::clone(&coordinator);
    let second_target = target.clone();
    let second = tokio::spawn(async move {
        second_coordinator
            .dispatch_tool(
                lease.lease_id,
                WorkspaceToolAction::WritePath(second_target),
                |authorization| async move {
                    authorization.edit_bytes(|bytes| {
                        second_flag.store(true, Ordering::SeqCst);
                        let mut edited = bytes.to_vec();
                        edited.extend_from_slice(b"+second");
                        Ok((edited, ()))
                    })
                },
            )
            .await
            .unwrap()
            .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!second_entered.load(Ordering::SeqCst));

    release_first_tx.send(()).unwrap();
    first.await.unwrap();
    second.await.unwrap();

    assert!(second_entered.load(Ordering::SeqCst));
    assert_eq!(std::fs::read_to_string(target).unwrap(), "first+second");
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

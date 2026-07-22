use super::*;

#[cfg(unix)]
#[tokio::test]
async fn secure_directory_visit_reads_regular_files_and_skips_symlinks() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let nested = workspace.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(workspace.join("visible.txt"), "visible").unwrap();
    std::fs::write(workspace.join(".hidden.txt"), "hidden").unwrap();
    std::fs::write(nested.join("child.txt"), "child").unwrap();
    let outside = root.path().join("outside.txt");
    std::fs::write(&outside, "outside").unwrap();
    symlink(outside, workspace.join("linked.txt")).unwrap();
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

    let entries = coordinator
        .dispatch_tool(
            lease.lease_id,
            WorkspaceToolAction::ReadPath(workspace.clone()),
            |authorization| async move {
                let mut entries = Vec::new();
                authorization.visit_directory(
                    WorkspaceDirectoryReadOptions {
                        max_depth: 2,
                        include_hidden: false,
                        read_file_contents: true,
                    },
                    |entry| {
                        entries.push((entry.relative_path, entry.kind, entry.content));
                        Ok(())
                    },
                )?;
                Ok::<_, WorkspaceCoordinatorError>(entries)
            },
        )
        .await
        .unwrap()
        .unwrap();

    assert!(entries.iter().any(|(path, kind, content)| {
        path == Path::new("visible.txt")
            && *kind == WorkspacePathKind::File
            && content.as_deref() == Some(b"visible".as_slice())
    }));
    assert!(entries.iter().any(
        |(path, kind, _)| path == Path::new("nested") && *kind == WorkspacePathKind::Directory
    ));
    assert!(entries
        .iter()
        .any(|(path, _, _)| path == Path::new("nested/child.txt")));
    assert!(!entries
        .iter()
        .any(|(path, _, _)| path == Path::new(".hidden.txt") || path == Path::new("linked.txt")));
}

#[cfg(unix)]
#[tokio::test]
async fn secure_directory_visit_rejects_a_symlink_swap_after_authorization() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let target = workspace.join("target");
    let moved = workspace.join("moved");
    let outside = root.path().join("outside");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "secret").unwrap();
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
            WorkspaceToolAction::ReadPath(target.clone()),
            move |authorization| async move {
                std::fs::rename(&target, moved).unwrap();
                symlink(outside, &target).unwrap();
                authorization.visit_directory(
                    WorkspaceDirectoryReadOptions {
                        max_depth: 1,
                        include_hidden: false,
                        read_file_contents: false,
                    },
                    |_| Ok(()),
                )
            },
        )
        .await
        .unwrap();

    assert!(result.is_err());
}

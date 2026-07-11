use std::time::{Duration, Instant};

use harness_daemon::{DaemonActivity, RuntimeGuard};

#[test]
fn user_instance_id_cannot_alias_a_runtime_parent() {
    let root = tempfile::tempdir().unwrap();
    assert!(RuntimeGuard::acquire(root.path(), ".").is_err());
}

#[cfg(unix)]
#[test]
fn runtime_token_and_lock_are_owner_only_and_reject_a_second_instance() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let guard = RuntimeGuard::acquire(root.path(), "user-a").unwrap();
    assert_eq!(
        std::fs::metadata(guard.runtime_dir())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for path in [guard.lock_path(), guard.token_path()] {
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(!guard.connection_token().is_empty());
    assert!(RuntimeGuard::acquire(root.path(), "user-a").is_err());
}

#[cfg(unix)]
#[test]
fn stale_socket_is_removed_only_by_the_lock_owner_and_symlinks_are_rejected() {
    use std::os::unix::net::UnixListener;

    let root = tempfile::tempdir().unwrap();
    let guard = RuntimeGuard::acquire(root.path(), "user-a").unwrap();
    let endpoint = guard.endpoint_path();
    drop(UnixListener::bind(endpoint).unwrap());
    guard.prepare_endpoint().unwrap();
    assert!(!endpoint.exists());

    std::os::unix::fs::symlink(root.path().join("target"), endpoint).unwrap();
    assert!(guard.prepare_endpoint().is_err());
}

#[test]
fn disconnect_never_stops_work_and_idle_shutdown_requires_all_activity_to_end() {
    let started = Instant::now();
    let mut activity = DaemonActivity::new(started);
    activity.client_connected();
    activity.task_started();
    activity.background_process_started();
    activity.client_disconnected(started + Duration::from_secs(1));

    let timeout = Duration::from_secs(300);
    assert!(!activity.should_shutdown(started + Duration::from_secs(600), timeout));
    assert_eq!(activity.active_tasks(), 1);

    activity.task_finished(started + Duration::from_secs(601));
    assert!(!activity.should_shutdown(started + Duration::from_secs(1_000), timeout));
    activity.background_process_finished(started + Duration::from_secs(1_001));
    assert!(!activity.should_shutdown(started + Duration::from_secs(1_300), timeout));
    assert!(activity.should_shutdown(started + Duration::from_secs(1_301), timeout));
}

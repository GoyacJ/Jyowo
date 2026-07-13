use std::collections::BTreeSet;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Instant;

use async_trait::async_trait;
use harness_contracts::{
    Event, KillScope, NetworkAccess, RedactRules, Redactor, SandboxBackendFailurePhase,
    SandboxError,
};
use harness_sandbox::{
    execute_with_lifecycle, preflight_exec, restore_with_lifecycle, shutdown_with_lifecycle,
    snapshot_with_lifecycle, ActivityHandle, EventSink, ExecContext, ExecOutcome, ExecSpec,
    NetworkPolicySupport, ProcessHandle, SandboxBackend, SandboxCapabilities, SessionSnapshotFile,
    SnapshotSpec,
};

#[cfg(feature = "local")]
use harness_sandbox::{LocalIsolation, LocalSandbox};

#[derive(Default)]
struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: Event) -> Result<(), SandboxError> {
        Ok(())
    }
}

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<Event>>,
}

impl RecordingSink {
    fn events(&self) -> Vec<Event> {
        self.events.lock().expect("events lock should work").clone()
    }
}

impl EventSink for RecordingSink {
    fn emit(&self, event: Event) -> Result<(), SandboxError> {
        self.events
            .lock()
            .expect("events lock should work")
            .push(event);
        Ok(())
    }
}

struct SecretRedactor;

#[cfg(feature = "local")]
#[test]
fn local_process_group_capability_matches_platform_enforcement() {
    let sandbox = LocalSandbox::new(std::env::temp_dir())
        .with_isolation(LocalIsolation::for_current_platform());

    let helpers_available = host_binary_available("sleep")
        && host_binary_available("kill")
        && absolute_host_binary_available("/bin/sh");
    assert_eq!(
        sandbox
            .capabilities()
            .supports_kill_scope
            .contains(&KillScope::ProcessGroup),
        cfg!(target_os = "linux") && helpers_available
    );
}

#[cfg(feature = "local")]
fn absolute_host_binary_available(path: &str) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(feature = "local")]
fn host_binary_available(binary: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        let path = directory.join(binary);
        let Ok(metadata) = path.metadata() else {
            return false;
        };
        if !metadata.is_file() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        {
            true
        }
    })
}

#[cfg(all(feature = "local", target_os = "macos"))]
#[test]
fn seatbelt_does_not_claim_process_tree_containment() {
    let capabilities = LocalSandbox::new(std::env::temp_dir())
        .with_isolation(LocalIsolation::Seatbelt)
        .capabilities();

    assert!(!capabilities
        .supports_kill_scope
        .contains(&KillScope::ProcessGroup));
    assert!(!capabilities
        .supports_synchronous_kill_scope
        .contains(&KillScope::ProcessGroup));
}

impl Redactor for SecretRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.replace("secret", "[MASK]")
    }
}

struct TestActivity {
    wait_error: Option<SandboxError>,
}

#[async_trait]
impl ActivityHandle for TestActivity {
    async fn wait(&self) -> Result<ExecOutcome, SandboxError> {
        if let Some(error) = &self.wait_error {
            return Err(error.clone());
        }
        Ok(ExecOutcome::default())
    }

    async fn kill(&self, _signal: i32, _scope: KillScope) -> Result<(), SandboxError> {
        Ok(())
    }

    fn touch(&self) {}

    fn last_activity(&self) -> Instant {
        Instant::now()
    }
}

struct TestBackend {
    id: String,
    before_execute_count: Arc<AtomicUsize>,
    after_execute_count: Arc<AtomicUsize>,
    execute_error: Option<SandboxError>,
    wait_error: Option<SandboxError>,
    after_execute_error: Option<SandboxError>,
    snapshot_error: Option<SandboxError>,
    restore_error: Option<SandboxError>,
    shutdown_error: Option<SandboxError>,
}

#[async_trait]
impl SandboxBackend for TestBackend {
    fn backend_id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            network: NetworkPolicySupport {
                none: true,
                loopback_only: false,
                allowlist: false,
                unrestricted: true,
            },
            max_concurrent_execs: 1,
            snapshot_kinds: BTreeSet::default(),
            ..SandboxCapabilities::default()
        }
    }

    async fn before_execute(
        &self,
        _spec: &ExecSpec,
        _ctx: &ExecContext,
    ) -> Result<(), SandboxError> {
        self.before_execute_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn execute(
        &self,
        _spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        if let Some(error) = &self.execute_error {
            return Err(error.clone());
        }
        Ok(ProcessHandle {
            pid: Some(42),
            stdout: None,
            stderr: None,
            stdin: None,
            cwd_marker: None,
            activity: Arc::new(TestActivity {
                wait_error: self.wait_error.clone(),
            }),
        })
    }

    async fn after_execute(
        &self,
        _outcome: &ExecOutcome,
        _ctx: &ExecContext,
    ) -> Result<(), SandboxError> {
        self.after_execute_count.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = &self.after_execute_error {
            return Err(error.clone());
        }
        Ok(())
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        if let Some(error) = &self.snapshot_error {
            return Err(error.clone());
        }
        Ok(SessionSnapshotFile::default())
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        if let Some(error) = &self.restore_error {
            return Err(error.clone());
        }
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        if let Some(error) = &self.shutdown_error {
            return Err(error.clone());
        }
        Ok(())
    }
}

#[tokio::test]
async fn sandbox_backend_is_object_safe_and_has_noop_hooks() {
    let after_execute_count = Arc::new(AtomicUsize::new(0));
    let backend: Arc<dyn SandboxBackend> = Arc::new(TestBackend {
        id: "test".to_owned(),
        before_execute_count: Arc::new(AtomicUsize::new(0)),
        after_execute_count: after_execute_count.clone(),
        execute_error: None,
        wait_error: None,
        after_execute_error: None,
        snapshot_error: None,
        restore_error: None,
        shutdown_error: None,
    });
    let spec = ExecSpec::default();
    let ctx = ExecContext::for_test(Arc::new(NullSink));

    backend.before_execute(&spec, &ctx).await.unwrap();
    let handle = backend.execute(spec, ctx.clone()).await.unwrap();
    let outcome = handle.activity.wait().await.unwrap();
    backend.after_execute(&outcome, &ctx).await.unwrap();

    assert_eq!(handle.pid, Some(42));
    assert_eq!(outcome.stdout_bytes_observed, 0);
    assert_eq!(after_execute_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn execute_with_lifecycle_runs_after_execute_once_when_wait_completes() {
    let before_execute_count = Arc::new(AtomicUsize::new(0));
    let after_execute_count = Arc::new(AtomicUsize::new(0));
    let backend: Arc<dyn SandboxBackend> = Arc::new(TestBackend {
        id: "test".to_owned(),
        before_execute_count: before_execute_count.clone(),
        after_execute_count: after_execute_count.clone(),
        execute_error: None,
        wait_error: None,
        after_execute_error: None,
        snapshot_error: None,
        restore_error: None,
        shutdown_error: None,
    });
    let ctx = ExecContext::for_test(Arc::new(NullSink));

    let handle = execute_with_lifecycle(backend, ExecSpec::default(), ctx)
        .await
        .expect("execute should succeed");
    let first = handle.activity.wait().await.expect("wait should succeed");
    let second = handle
        .activity
        .wait()
        .await
        .expect("second wait should return cached outcome");

    assert_eq!(first, second);
    assert_eq!(before_execute_count.load(Ordering::SeqCst), 1);
    assert_eq!(after_execute_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn execute_with_lifecycle_emits_preflight_passed_before_execute() {
    let before_execute_count = Arc::new(AtomicUsize::new(0));
    let backend: Arc<dyn SandboxBackend> = Arc::new(TestBackend {
        id: "test".to_owned(),
        before_execute_count: before_execute_count.clone(),
        after_execute_count: Arc::new(AtomicUsize::new(0)),
        execute_error: None,
        wait_error: None,
        after_execute_error: None,
        snapshot_error: None,
        restore_error: None,
        shutdown_error: None,
    });
    let sink = Arc::new(RecordingSink::default());
    let ctx = ExecContext::for_test(sink.clone());

    let _handle = execute_with_lifecycle(backend, ExecSpec::default(), ctx)
        .await
        .expect("execute should succeed");

    assert_eq!(before_execute_count.load(Ordering::SeqCst), 1);
    let events = sink.events();
    assert!(
        matches!(events.first(), Some(Event::SandboxPreflightPassed(passed)) if passed.backend_id == "test")
    );
}

#[tokio::test]
async fn preflight_exec_emits_failed_event_and_returns_report_without_execute() {
    let backend = TestBackend {
        id: "test".to_owned(),
        before_execute_count: Arc::new(AtomicUsize::new(0)),
        after_execute_count: Arc::new(AtomicUsize::new(0)),
        execute_error: None,
        wait_error: None,
        after_execute_error: None,
        snapshot_error: None,
        restore_error: None,
        shutdown_error: None,
    };
    let sink = Arc::new(RecordingSink::default());
    let ctx = ExecContext::for_test(sink.clone());
    let mut spec = ExecSpec::default();
    spec.policy.network = NetworkAccess::AllowList(Vec::new());

    let error = preflight_exec(&backend, &spec, &ctx)
        .expect_err("preflight must fail unsupported capability");

    assert_eq!(backend.before_execute_count.load(Ordering::SeqCst), 0);
    assert!(matches!(
        error,
        SandboxError::CapabilityMismatch {
            ref capability,
            ..
        } if capability == "network"
    ));
    assert!(matches!(
        sink.events().first(),
        Some(Event::SandboxPreflightFailed(failed))
            if failed.backend_id == "test" && failed.reason.contains("network")
    ));
}

#[tokio::test]
async fn execute_with_lifecycle_emits_backend_failure_when_execute_fails() {
    let backend: Arc<dyn SandboxBackend> = Arc::new(TestBackend {
        id: "test".to_owned(),
        before_execute_count: Arc::new(AtomicUsize::new(0)),
        after_execute_count: Arc::new(AtomicUsize::new(0)),
        execute_error: Some(SandboxError::Message("spawn secret failed".to_owned())),
        wait_error: None,
        after_execute_error: None,
        snapshot_error: None,
        restore_error: None,
        shutdown_error: None,
    });
    let sink = Arc::new(RecordingSink::default());
    let mut ctx = ExecContext::for_test(sink.clone());
    ctx.redactor = Arc::new(SecretRedactor);

    let error = match execute_with_lifecycle(backend, ExecSpec::default(), ctx).await {
        Ok(_) => panic!("execute failure should be returned"),
        Err(error) => error,
    };

    assert_eq!(error.to_string(), "spawn secret failed");
    assert!(sink.events().iter().any(|event| {
        matches!(
            event,
            Event::SandboxBackendFailed(failed)
                if failed.backend_id == "test"
                    && failed.phase == SandboxBackendFailurePhase::Execute
                    && failed.error.to_string() == "spawn [MASK] failed"
        )
    }));
}

#[tokio::test]
async fn execute_with_lifecycle_emits_backend_failure_when_wait_fails() {
    let backend: Arc<dyn SandboxBackend> = Arc::new(TestBackend {
        id: "test".to_owned(),
        before_execute_count: Arc::new(AtomicUsize::new(0)),
        after_execute_count: Arc::new(AtomicUsize::new(0)),
        execute_error: None,
        wait_error: Some(SandboxError::ResourceLimitExceeded {
            limit: "memory".to_owned(),
            detail: "wait secret failed".to_owned(),
        }),
        after_execute_error: None,
        snapshot_error: None,
        restore_error: None,
        shutdown_error: None,
    });
    let sink = Arc::new(RecordingSink::default());
    let mut ctx = ExecContext::for_test(sink.clone());
    ctx.redactor = Arc::new(SecretRedactor);

    let handle = execute_with_lifecycle(backend, ExecSpec::default(), ctx)
        .await
        .expect("execute should succeed");
    let error = handle
        .activity
        .wait()
        .await
        .expect_err("wait failure should be returned");

    assert!(error.to_string().contains("secret"));
    assert!(sink.events().iter().any(|event| {
        matches!(
            event,
            Event::SandboxBackendFailed(failed)
                if failed.backend_id == "test"
                    && failed.phase == SandboxBackendFailurePhase::Wait
                    && failed.error.to_string().contains("[MASK]")
                    && !failed.error.to_string().contains("secret")
        )
    }));
}

#[tokio::test]
async fn execute_with_lifecycle_emits_post_execution_failure_without_rewriting_outcome() {
    let after_execute_count = Arc::new(AtomicUsize::new(0));
    let backend: Arc<dyn SandboxBackend> = Arc::new(TestBackend {
        id: "test".to_owned(),
        before_execute_count: Arc::new(AtomicUsize::new(0)),
        after_execute_count: after_execute_count.clone(),
        execute_error: None,
        wait_error: None,
        after_execute_error: Some(SandboxError::Message("cleanup failed".to_owned())),
        snapshot_error: None,
        restore_error: None,
        shutdown_error: None,
    });
    let sink = Arc::new(RecordingSink::default());
    let ctx = ExecContext::for_test(sink.clone());

    let handle = execute_with_lifecycle(backend, ExecSpec::default(), ctx)
        .await
        .expect("execute should succeed");
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(outcome.stdout_bytes_observed, 0);
    assert_eq!(after_execute_count.load(Ordering::SeqCst), 1);
    assert!(sink.events().iter().any(|event| {
        matches!(
            event,
            Event::SandboxPostExecutionFailed(failed)
                if failed.backend_id == "test" && failed.error.to_string() == "cleanup failed"
        )
    }));
}

#[tokio::test]
async fn execute_with_lifecycle_redacts_post_execution_failure_event() {
    let after_execute_count = Arc::new(AtomicUsize::new(0));
    let backend: Arc<dyn SandboxBackend> = Arc::new(TestBackend {
        id: "test".to_owned(),
        before_execute_count: Arc::new(AtomicUsize::new(0)),
        after_execute_count,
        execute_error: None,
        wait_error: None,
        after_execute_error: Some(SandboxError::WorkspaceSyncFailed {
            direction: "pull".to_owned(),
            program: "rsync".to_owned(),
            detail: "cleanup secret failed".to_owned(),
        }),
        snapshot_error: None,
        restore_error: None,
        shutdown_error: None,
    });
    let sink = Arc::new(RecordingSink::default());
    let mut ctx = ExecContext::for_test(sink.clone());
    ctx.redactor = Arc::new(SecretRedactor);

    let handle = execute_with_lifecycle(backend, ExecSpec::default(), ctx)
        .await
        .expect("execute should succeed");
    handle.activity.wait().await.expect("wait should succeed");

    assert!(sink.events().iter().any(|event| {
        matches!(
            event,
            Event::SandboxPostExecutionFailed(failed)
                if failed.error.to_string().contains("[MASK]")
                    && !failed.error.to_string().contains("secret")
        )
    }));
}

#[tokio::test]
async fn snapshot_with_lifecycle_emits_backend_failure_when_snapshot_fails() {
    let backend: Arc<dyn SandboxBackend> = Arc::new(TestBackend {
        id: "test".to_owned(),
        before_execute_count: Arc::new(AtomicUsize::new(0)),
        after_execute_count: Arc::new(AtomicUsize::new(0)),
        execute_error: None,
        wait_error: None,
        after_execute_error: None,
        snapshot_error: Some(SandboxError::Message("snapshot secret failed".to_owned())),
        restore_error: None,
        shutdown_error: None,
    });
    let sink = Arc::new(RecordingSink::default());
    let mut ctx = ExecContext::for_test(sink.clone());
    ctx.redactor = Arc::new(SecretRedactor);

    let error = snapshot_with_lifecycle(backend, &SnapshotSpec::default(), &ctx)
        .await
        .expect_err("snapshot failure should be returned");

    assert_eq!(error.to_string(), "snapshot secret failed");
    assert!(sink.events().iter().any(|event| {
        matches!(
            event,
            Event::SandboxBackendFailed(failed)
                if failed.backend_id == "test"
                    && failed.phase == SandboxBackendFailurePhase::Snapshot
                    && failed.error.to_string() == "snapshot [MASK] failed"
        )
    }));
}

#[tokio::test]
async fn restore_with_lifecycle_emits_backend_failure_when_restore_fails() {
    let backend: Arc<dyn SandboxBackend> = Arc::new(TestBackend {
        id: "test".to_owned(),
        before_execute_count: Arc::new(AtomicUsize::new(0)),
        after_execute_count: Arc::new(AtomicUsize::new(0)),
        execute_error: None,
        wait_error: None,
        after_execute_error: None,
        snapshot_error: None,
        restore_error: Some(SandboxError::Message("restore secret failed".to_owned())),
        shutdown_error: None,
    });
    let sink = Arc::new(RecordingSink::default());
    let mut ctx = ExecContext::for_test(sink.clone());
    ctx.redactor = Arc::new(SecretRedactor);

    let error = restore_with_lifecycle(backend, &SessionSnapshotFile::default(), &ctx)
        .await
        .expect_err("restore failure should be returned");

    assert_eq!(error.to_string(), "restore secret failed");
    assert!(sink.events().iter().any(|event| {
        matches!(
            event,
            Event::SandboxBackendFailed(failed)
                if failed.backend_id == "test"
                    && failed.phase == SandboxBackendFailurePhase::Restore
                    && failed.error.to_string() == "restore [MASK] failed"
        )
    }));
}

#[tokio::test]
async fn shutdown_with_lifecycle_emits_backend_failure_when_shutdown_fails() {
    let backend: Arc<dyn SandboxBackend> = Arc::new(TestBackend {
        id: "test".to_owned(),
        before_execute_count: Arc::new(AtomicUsize::new(0)),
        after_execute_count: Arc::new(AtomicUsize::new(0)),
        execute_error: None,
        wait_error: None,
        after_execute_error: None,
        snapshot_error: None,
        restore_error: None,
        shutdown_error: Some(SandboxError::Message("shutdown secret failed".to_owned())),
    });
    let sink = Arc::new(RecordingSink::default());
    let mut ctx = ExecContext::for_test(sink.clone());
    ctx.redactor = Arc::new(SecretRedactor);

    let error = shutdown_with_lifecycle(backend, &ctx)
        .await
        .expect_err("shutdown failure should be returned");

    assert_eq!(error.to_string(), "shutdown secret failed");
    assert!(sink.events().iter().any(|event| {
        matches!(
            event,
            Event::SandboxBackendFailed(failed)
                if failed.backend_id == "test"
                    && failed.phase == SandboxBackendFailurePhase::Shutdown
                    && failed.error.to_string() == "shutdown [MASK] failed"
        )
    }));
}

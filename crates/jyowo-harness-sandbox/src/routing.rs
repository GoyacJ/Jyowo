//! Routing sandbox backend selects one child backend per execution.
//!
//! Selection uses deterministic strategy order. The router binds a selected child
//! backend to an execution id during `before_execute` and calls only that child for
//! the remaining lifecycle. Concurrent executions keep separate child bindings.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use harness_contracts::{KillScope, SandboxError};

use crate::{
    ActivityHandle, ExecContext, ExecOutcome, ExecSpec, ProcessHandle, SandboxBackend,
    SandboxCapabilities, SessionSnapshotFile, Signal, SnapshotSpec,
};

/// A leased child backend binding for one execution id.
struct RoutingSelectionLease {
    selected_backend: Arc<dyn SandboxBackend>,
}

/// A routing sandbox backend that delegates each execution to one selected child backend.
///
/// Selection traverses the configured backends in order, choosing the first whose
/// `preflight_execute` passes and whose `before_execute` succeeds. Once selected,
/// the child backend is bound to the execution id for the rest of the lifecycle.
pub struct RoutingSandboxBackend {
    backends: Vec<Arc<dyn SandboxBackend>>,
    leases: tokio::sync::Mutex<HashMap<u64, RoutingSelectionLease>>,
}

impl std::fmt::Debug for RoutingSandboxBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoutingSandboxBackend")
            .field("candidate_ids", &self.candidate_ids())
            .finish_non_exhaustive()
    }
}

impl RoutingSandboxBackend {
    /// Creates a new routing backend from an ordered list of candidate backends.
    ///
    /// Returns an error when the list is empty, because no execution could ever be routed.
    pub fn new(backends: Vec<Arc<dyn SandboxBackend>>) -> Result<Self, SandboxError> {
        if backends.is_empty() {
            return Err(SandboxError::CapabilityMismatch {
                capability: "routing".to_owned(),
                detail: "routing sandbox requires at least one child backend".to_owned(),
            });
        }
        Ok(Self {
            backends,
            leases: tokio::sync::Mutex::new(HashMap::new()),
        })
    }

    /// Returns the ordered list of child backend ids for diagnostic use.
    pub fn candidate_ids(&self) -> Vec<String> {
        self.backends
            .iter()
            .map(|backend| backend.backend_id().to_owned())
            .collect()
    }

    /// Picks the first backend that can enforce `spec` according to `preflight_execute`.
    /// Returns an error listing every candidate backend id together with its failure reason.
    pub fn select_backend(&self, spec: &ExecSpec) -> Result<Arc<dyn SandboxBackend>, SandboxError> {
        let mut reasons: Vec<String> = Vec::new();
        for backend in &self.backends {
            match backend.preflight_execute(spec) {
                Ok(()) => return Ok(Arc::clone(backend)),
                Err(error) => {
                    reasons.push(format!("{}: {error}", backend.backend_id()));
                }
            }
        }
        Err(SandboxError::CapabilityMismatch {
            capability: "routing".to_owned(),
            detail: format!(
                "no candidate backend can enforce the requested policy: {}",
                reasons.join("; ")
            ),
        })
    }
}

#[async_trait]
impl SandboxBackend for RoutingSandboxBackend {
    fn backend_id(&self) -> &str {
        "routing"
    }

    fn candidate_backend_ids(&self) -> Vec<String> {
        self.candidate_ids()
    }

    fn capabilities(&self) -> SandboxCapabilities {
        // The router reports the union of its children's capabilities. A policy is
        // supported when at least one child backend can enforce it.
        let mut caps = SandboxCapabilities::default();
        // Host filesystem isolation is safe to advertise only when every child
        // the router may select enforces it.
        caps.host_filesystem_isolation = true;
        for backend in &self.backends {
            let child = backend.capabilities();
            // Accumulate per-policy support.
            caps.network.none = caps.network.none || child.network.none;
            caps.network.loopback_only = caps.network.loopback_only || child.network.loopback_only;
            caps.network.allowlist = caps.network.allowlist || child.network.allowlist;
            caps.network.unrestricted = caps.network.unrestricted || child.network.unrestricted;
            caps.workspace.read_write_all =
                caps.workspace.read_write_all || child.workspace.read_write_all;
            caps.workspace.read_only = caps.workspace.read_only || child.workspace.read_only;
            caps.workspace.writable_subpaths =
                caps.workspace.writable_subpaths || child.workspace.writable_subpaths;
            caps.host_filesystem_isolation =
                caps.host_filesystem_isolation && child.host_filesystem_isolation;
            // Accumulate boolean capabilities.
            caps.supports_streaming = caps.supports_streaming || child.supports_streaming;
            caps.supports_stdin = caps.supports_stdin || child.supports_stdin;
            caps.supports_cwd_tracking = caps.supports_cwd_tracking || child.supports_cwd_tracking;
            caps.supports_activity_heartbeat =
                caps.supports_activity_heartbeat || child.supports_activity_heartbeat;
            caps.supports_interactive_shell =
                caps.supports_interactive_shell || child.supports_interactive_shell;
            caps.supports_gpu = caps.supports_gpu || child.supports_gpu;
            caps.supports_pty = caps.supports_pty || child.supports_pty;
            caps.supports_detach = caps.supports_detach || child.supports_detach;
            caps.supports_workspace_sync =
                caps.supports_workspace_sync || child.supports_workspace_sync;
            caps.supports_session_snapshot =
                caps.supports_session_snapshot || child.supports_session_snapshot;
            for scope in child.supports_kill_scope {
                if !caps.supports_kill_scope.contains(&scope) {
                    caps.supports_kill_scope.push(scope);
                }
            }
            for scope in child.supports_synchronous_kill_scope {
                if !caps.supports_synchronous_kill_scope.contains(&scope) {
                    caps.supports_synchronous_kill_scope.push(scope);
                }
            }
            // Take the max of max_concurrent_execs.
            caps.max_concurrent_execs = caps.max_concurrent_execs.max(child.max_concurrent_execs);
        }
        caps
    }

    /// Checks whether at least one child backend can enforce the requested spec.
    fn preflight_execute(&self, spec: &ExecSpec) -> Result<(), SandboxError> {
        // Delegate to select_backend; we don't need to return the selected backend here.
        self.select_backend(spec).map(|_| ())
    }

    /// Selects exactly one child backend, calls its `before_execute`, and stores a
    /// lease keyed by `ctx.execution_id`. The lease binds this execution id to the
    /// selected child for the rest of the lifecycle.
    async fn before_execute(&self, spec: &ExecSpec, ctx: &ExecContext) -> Result<(), SandboxError> {
        let mut reasons: Vec<String> = Vec::new();
        for backend in &self.backends {
            // Check capability first.
            let preflight = backend.preflight_execute(spec);
            if let Err(ref err) = preflight {
                reasons.push(format!("{}: {err}", backend.backend_id()));
                continue;
            }

            // Try to bind this backend.
            match backend.before_execute(spec, ctx).await {
                Ok(()) => {
                    self.leases.lock().await.insert(
                        ctx.execution_id,
                        RoutingSelectionLease {
                            selected_backend: Arc::clone(backend),
                        },
                    );
                    return Ok(());
                }
                Err(err) => {
                    reasons.push(format!("{}: {err}", backend.backend_id()));
                }
            }
        }

        Err(SandboxError::CapabilityMismatch {
            capability: "routing".to_owned(),
            detail: format!(
                "no candidate backend can execute the requested policy: {}",
                reasons.join("; ")
            ),
        })
    }

    /// Removes the lease for `ctx.execution_id` and delegates to the selected child
    /// backend's `execute`. Returns a `ProcessHandle` wrapped in a `RoutingActivityHandle`
    /// so the selected child's `after_execute` is called after `wait`.
    ///
    /// Fails closed when no lease exists for the execution id — the caller bypassed
    /// the lifecycle sequence.
    async fn execute(
        &self,
        spec: ExecSpec,
        ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        let execution_id = ctx.execution_id;
        let lease = self
            .leases
            .lock()
            .await
            .remove(&execution_id)
            .ok_or_else(|| SandboxError::CapabilityMismatch {
                capability: "routing".to_owned(),
                detail: format!(
                    "no routing lease for execution {execution_id}: execute called without preceding before_execute"
                ),
            })?;

        match lease.selected_backend.execute(spec, ctx.clone()).await {
            Ok(mut handle) => {
                handle.activity = Arc::new(RoutingActivityHandle {
                    selected_backend: lease.selected_backend,
                    inner: handle.activity,
                    ctx,
                    after_execute_started: AtomicBool::new(false),
                });
                Ok(handle)
            }
            Err(error) => {
                let _ = lease
                    .selected_backend
                    .after_execute(&ExecOutcome::default(), &ctx)
                    .await;
                Err(error)
            }
        }
    }

    /// Router-level `after_execute` is a no-op. The selected child's `after_execute`
    /// is called by `RoutingActivityHandle::wait`.
    async fn after_execute(
        &self,
        _outcome: &ExecOutcome,
        _ctx: &ExecContext,
    ) -> Result<(), SandboxError> {
        Ok(())
    }

    async fn snapshot_session(
        &self,
        spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        // Snapshot is not routed per-exec — delegate to the first backend that supports it.
        for backend in &self.backends {
            match backend.snapshot_session(spec).await {
                Ok(snapshot) => return Ok(snapshot),
                Err(SandboxError::SnapshotUnsupported { .. }) => continue,
                Err(err) => return Err(err),
            }
        }
        Err(SandboxError::SnapshotUnsupported {
            kind: "routing: no child backend supports snapshot".to_owned(),
        })
    }

    async fn restore_session(&self, snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        for backend in &self.backends {
            match backend.restore_session(snapshot).await {
                Ok(()) => return Ok(()),
                Err(SandboxError::SnapshotUnsupported { .. }) => continue,
                Err(err) => return Err(err),
            }
        }
        Err(SandboxError::SnapshotUnsupported {
            kind: "routing: no child backend supports restore".to_owned(),
        })
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        let mut last_error: Option<SandboxError> = None;
        for backend in &self.backends {
            if let Err(err) = backend.shutdown().await {
                last_error = Some(err);
            }
        }
        match last_error {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

/// Wraps a child `ActivityHandle` so the selected child backend's `after_execute`
/// is called exactly once after the inner `wait` completes.
struct RoutingActivityHandle {
    selected_backend: Arc<dyn SandboxBackend>,
    inner: Arc<dyn ActivityHandle>,
    ctx: ExecContext,
    after_execute_started: AtomicBool,
}

impl RoutingActivityHandle {
    fn mark_after_execute_started(&self) -> bool {
        !self.after_execute_started.swap(true, Ordering::SeqCst)
    }

    async fn run_after_execute_once(&self, outcome: &ExecOutcome) {
        if self.mark_after_execute_started() {
            let _ = self
                .selected_backend
                .after_execute(outcome, &self.ctx)
                .await;
        }
    }
}

#[async_trait]
impl ActivityHandle for RoutingActivityHandle {
    async fn wait(&self) -> Result<ExecOutcome, SandboxError> {
        match self.inner.wait().await {
            Ok(outcome) => {
                self.run_after_execute_once(&outcome).await;
                Ok(outcome)
            }
            Err(error) => {
                self.run_after_execute_once(&ExecOutcome::default()).await;
                Err(error)
            }
        }
    }

    async fn kill(&self, signal: Signal, scope: KillScope) -> Result<(), SandboxError> {
        let result = self.inner.kill(signal, scope).await;
        self.run_after_execute_once(&ExecOutcome::default()).await;
        result
    }

    fn kill_sync(&self, signal: Signal, scope: KillScope) -> Result<(), SandboxError> {
        self.inner.kill_sync(signal, scope)
    }

    fn touch(&self) {
        self.inner.touch();
    }

    fn last_activity(&self) -> Instant {
        self.inner.last_activity()
    }
}

impl Drop for RoutingActivityHandle {
    fn drop(&mut self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if self.mark_after_execute_started() {
            let selected_backend = Arc::clone(&self.selected_backend);
            let ctx = self.ctx.clone();
            handle.spawn(async move {
                let _ = selected_backend
                    .after_execute(&ExecOutcome::default(), &ctx)
                    .await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EventSink, ExecContext, ExecSpec, NetworkPolicySupport, SandboxCapabilities};
    use harness_contracts::{Event, NetworkAccess, SandboxPolicy};
    use std::sync::atomic::AtomicU64;
    use std::sync::Mutex;

    /// A stub backend whose capabilities and behavior are fully determined by
    /// constructor parameters. Callbacks record which lifecycle methods were invoked.
    struct StubBackend {
        id: String,
        caps: SandboxCapabilities,
        /// When `Some`, overrides the default `preflight_execute` (which checks capabilities).
        preflight_override: Mutex<Option<Result<(), SandboxError>>>,
        before_execute_count: AtomicU64,
        before_execute_result: Mutex<Result<(), SandboxError>>,
        execute_count: AtomicU64,
        /// When `Some`, execute returns this error instead of a successful handle.
        execute_err: Mutex<Option<SandboxError>>,
        activity_wait_result: Mutex<Result<ExecOutcome, SandboxError>>,
        after_execute_count: AtomicU64,
    }

    impl StubBackend {
        fn new(id: &str, caps: SandboxCapabilities) -> Arc<Self> {
            Arc::new(Self {
                id: id.to_owned(),
                caps,
                preflight_override: Mutex::new(None),
                before_execute_count: AtomicU64::new(0),
                before_execute_result: Mutex::new(Ok(())),
                execute_count: AtomicU64::new(0),
                execute_err: Mutex::new(None),
                activity_wait_result: Mutex::new(Ok(ExecOutcome::default())),
                after_execute_count: AtomicU64::new(0),
            })
        }

        fn set_before_execute_result(&self, result: Result<(), SandboxError>) {
            *self.before_execute_result.lock().unwrap() = result;
        }

        fn set_activity_wait_result(&self, result: Result<ExecOutcome, SandboxError>) {
            *self.activity_wait_result.lock().unwrap() = result;
        }

        fn set_execute_err(&self, err: SandboxError) {
            *self.execute_err.lock().unwrap() = Some(err);
        }

        fn before_execute_count(&self) -> u64 {
            self.before_execute_count.load(Ordering::SeqCst)
        }

        fn execute_count(&self) -> u64 {
            self.execute_count.load(Ordering::SeqCst)
        }

        fn after_execute_count(&self) -> u64 {
            self.after_execute_count.load(Ordering::SeqCst)
        }
    }

    struct RecordingActivityHandle {
        wait_result: Result<ExecOutcome, SandboxError>,
        wait_count: AtomicU64,
        kill_count: AtomicU64,
    }

    #[async_trait]
    impl ActivityHandle for RecordingActivityHandle {
        async fn wait(&self) -> Result<ExecOutcome, SandboxError> {
            self.wait_count.fetch_add(1, Ordering::SeqCst);
            self.wait_result.clone()
        }

        async fn kill(&self, _signal: Signal, _scope: KillScope) -> Result<(), SandboxError> {
            self.kill_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn touch(&self) {}

        fn last_activity(&self) -> Instant {
            Instant::now()
        }
    }

    #[async_trait]
    impl SandboxBackend for StubBackend {
        fn backend_id(&self) -> &str {
            &self.id
        }

        fn capabilities(&self) -> SandboxCapabilities {
            self.caps.clone()
        }

        fn preflight_execute(&self, spec: &ExecSpec) -> Result<(), SandboxError> {
            if let Some(overridden) = self.preflight_override.lock().unwrap().clone() {
                return overridden;
            }
            // Default: delegate to the real capability validation.
            crate::backend::validate_preflight_capabilities(self.backend_id(), &self.caps, spec)
        }

        async fn before_execute(
            &self,
            _spec: &ExecSpec,
            _ctx: &ExecContext,
        ) -> Result<(), SandboxError> {
            self.before_execute_count.fetch_add(1, Ordering::SeqCst);
            self.before_execute_result.lock().unwrap().clone()
        }

        async fn execute(
            &self,
            _spec: ExecSpec,
            _ctx: ExecContext,
        ) -> Result<ProcessHandle, SandboxError> {
            self.execute_count.fetch_add(1, Ordering::SeqCst);
            if let Some(err) = self.execute_err.lock().unwrap().clone() {
                return Err(err);
            }
            Ok(stub_handle_with_wait_result(
                self.activity_wait_result.lock().unwrap().clone(),
            ))
        }

        async fn after_execute(
            &self,
            _outcome: &ExecOutcome,
            _ctx: &ExecContext,
        ) -> Result<(), SandboxError> {
            self.after_execute_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn snapshot_session(
            &self,
            _spec: &SnapshotSpec,
        ) -> Result<SessionSnapshotFile, SandboxError> {
            Err(SandboxError::SnapshotUnsupported {
                kind: "stub".to_owned(),
            })
        }

        async fn restore_session(
            &self,
            _snapshot: &SessionSnapshotFile,
        ) -> Result<(), SandboxError> {
            Err(SandboxError::SnapshotUnsupported {
                kind: "stub".to_owned(),
            })
        }

        async fn shutdown(&self) -> Result<(), SandboxError> {
            Ok(())
        }
    }

    struct NullSink;

    impl EventSink for NullSink {
        fn emit(&self, _event: Event) -> Result<(), SandboxError> {
            Ok(())
        }
    }

    fn test_ctx() -> ExecContext {
        ExecContext::for_test(Arc::new(NullSink))
    }

    fn spec_with_network(network: NetworkAccess) -> ExecSpec {
        ExecSpec {
            policy: SandboxPolicy {
                network,
                ..ExecSpec::default().policy
            },
            ..ExecSpec::default()
        }
    }

    fn stub_handle_with_wait_result(
        wait_result: Result<ExecOutcome, SandboxError>,
    ) -> ProcessHandle {
        ProcessHandle {
            pid: None,
            stdout: None,
            stderr: None,
            stdin: None,
            cwd_marker: None,
            activity: Arc::new(RecordingActivityHandle {
                wait_result,
                wait_count: AtomicU64::new(0),
                kill_count: AtomicU64::new(0),
            }),
        }
    }

    // --- Tests ---

    #[test]
    fn new_rejects_empty_backends() {
        let result = RoutingSandboxBackend::new(vec![]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(&err, SandboxError::CapabilityMismatch { capability, .. } if capability == "routing")
        );
    }

    #[tokio::test]
    async fn preflight_picks_first_capable_backend() {
        let caps_none = SandboxCapabilities {
            network: NetworkPolicySupport {
                none: true,
                ..NetworkPolicySupport::default()
            },
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        };
        let backend_a = StubBackend::new("a", caps_none.clone());
        let backend_b = StubBackend::new("b", caps_none);
        let router =
            RoutingSandboxBackend::new(vec![backend_a.clone(), backend_b.clone()]).unwrap();

        // preflight should succeed: at least one candidate can enforce NetworkAccess::None.
        let spec = spec_with_network(NetworkAccess::None);
        router
            .preflight_execute(&spec)
            .expect("preflight should pass");

        // select_backend should pick the first (backend_a).
        let selected = router.select_backend(&spec).unwrap();
        assert_eq!(selected.backend_id(), "a");
    }

    #[tokio::test]
    async fn preflight_fails_when_no_backend_can_enforce() {
        let caps_unrestricted_only = SandboxCapabilities {
            network: NetworkPolicySupport {
                unrestricted: true,
                ..NetworkPolicySupport::default()
            },
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        };
        let backend = StubBackend::new("no_none", caps_unrestricted_only);
        let router = RoutingSandboxBackend::new(vec![backend]).unwrap();

        let spec = spec_with_network(NetworkAccess::None);
        let err = router
            .preflight_execute(&spec)
            .expect_err("preflight should fail");
        let msg = err.to_string();
        assert!(
            msg.contains("no_none"),
            "error should name candidate: {msg}"
        );
    }

    #[tokio::test]
    async fn before_execute_stores_and_uses_lease() {
        let caps = SandboxCapabilities {
            network: NetworkPolicySupport {
                unrestricted: true,
                ..NetworkPolicySupport::default()
            },
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        };
        let stub = StubBackend::new("selected", caps);

        let router = RoutingSandboxBackend::new(vec![stub.clone()]).unwrap();
        let spec = spec_with_network(NetworkAccess::Unrestricted);
        let mut ctx = test_ctx();
        // execute_with_lifecycle would ordinarily assign execution_id, so simulate it.
        ctx.execution_id = 42;

        // Phase 1: before_execute
        router
            .before_execute(&spec, &ctx)
            .await
            .expect("before_execute should succeed");
        assert_eq!(stub.before_execute_count(), 1);

        // Phase 2: execute must find the lease.
        let handle = router
            .execute(spec.clone(), ctx.clone())
            .await
            .expect("execute should succeed");
        assert_eq!(stub.execute_count(), 1);

        // Phase 3: wait on the handle and verify after_execute was called on the selected child.
        assert_eq!(
            stub.after_execute_count(),
            0,
            "after_execute not yet called before wait"
        );
        let outcome = handle.activity.wait().await.expect("wait should succeed");
        assert_eq!(
            stub.after_execute_count(),
            1,
            "after_execute should be called after wait"
        );

        // Wait again — must not call after_execute a second time.
        let _ = handle.activity.wait().await;
        assert_eq!(
            stub.after_execute_count(),
            1,
            "after_execute must be called exactly once"
        );
        drop(outcome);
    }

    #[tokio::test]
    async fn wait_error_still_calls_selected_child_after_execute_once() {
        let caps = SandboxCapabilities {
            network: NetworkPolicySupport {
                unrestricted: true,
                ..NetworkPolicySupport::default()
            },
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        };
        let stub = StubBackend::new("selected", caps);
        stub.set_activity_wait_result(Err(SandboxError::Unavailable {
            backend: "selected".to_owned(),
            detail: "wait failed".to_owned(),
        }));

        let router = RoutingSandboxBackend::new(vec![stub.clone()]).unwrap();
        let spec = spec_with_network(NetworkAccess::Unrestricted);
        let mut ctx = test_ctx();
        ctx.execution_id = 142;

        router.before_execute(&spec, &ctx).await.unwrap();
        let handle = router.execute(spec, ctx).await.unwrap();

        handle
            .activity
            .wait()
            .await
            .expect_err("inner wait failure should propagate");
        assert_eq!(
            stub.after_execute_count(),
            1,
            "selected child cleanup should run even when wait fails"
        );

        let _ = handle.activity.wait().await;
        assert_eq!(
            stub.after_execute_count(),
            1,
            "selected child cleanup must still be exactly once"
        );
    }

    #[tokio::test]
    async fn kill_calls_selected_child_after_execute_once() {
        let caps = SandboxCapabilities {
            network: NetworkPolicySupport {
                unrestricted: true,
                ..NetworkPolicySupport::default()
            },
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        };
        let stub = StubBackend::new("selected", caps);

        let router = RoutingSandboxBackend::new(vec![stub.clone()]).unwrap();
        let spec = spec_with_network(NetworkAccess::Unrestricted);
        let mut ctx = test_ctx();
        ctx.execution_id = 143;

        router.before_execute(&spec, &ctx).await.unwrap();
        let handle = router.execute(spec, ctx).await.unwrap();

        handle
            .activity
            .kill(15, KillScope::Process)
            .await
            .expect("kill should delegate to selected child");
        assert_eq!(
            stub.after_execute_count(),
            1,
            "selected child cleanup should run after kill"
        );

        let _ = handle.activity.wait().await;
        assert_eq!(
            stub.after_execute_count(),
            1,
            "wait after kill must not duplicate selected child cleanup"
        );
    }

    #[tokio::test]
    async fn dropped_activity_handle_calls_selected_child_after_execute_once() {
        let caps = SandboxCapabilities {
            network: NetworkPolicySupport {
                unrestricted: true,
                ..NetworkPolicySupport::default()
            },
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        };
        let stub = StubBackend::new("selected", caps);

        let router = RoutingSandboxBackend::new(vec![stub.clone()]).unwrap();
        let spec = spec_with_network(NetworkAccess::Unrestricted);
        let mut ctx = test_ctx();
        ctx.execution_id = 144;

        router.before_execute(&spec, &ctx).await.unwrap();
        let handle = router.execute(spec, ctx).await.unwrap();
        drop(handle);

        for _ in 0..20 {
            if stub.after_execute_count() == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(
            stub.after_execute_count(),
            1,
            "selected child cleanup should run when the routed handle is dropped"
        );
    }

    #[tokio::test]
    async fn execute_error_calls_selected_child_after_execute_once() {
        let caps = SandboxCapabilities {
            network: NetworkPolicySupport {
                unrestricted: true,
                ..NetworkPolicySupport::default()
            },
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        };
        let stub = StubBackend::new("selected", caps);
        stub.set_execute_err(SandboxError::Unavailable {
            backend: "selected".to_owned(),
            detail: "spawn failed".to_owned(),
        });

        let router = RoutingSandboxBackend::new(vec![stub.clone()]).unwrap();
        let spec = spec_with_network(NetworkAccess::Unrestricted);
        let mut ctx = test_ctx();
        ctx.execution_id = 145;

        router.before_execute(&spec, &ctx).await.unwrap();
        match router.execute(spec, ctx).await {
            Ok(_) => panic!("child execute failure should propagate"),
            Err(error) => assert!(
                error.to_string().contains("spawn failed"),
                "original child execute error should propagate: {error}"
            ),
        }

        assert_eq!(
            stub.after_execute_count(),
            1,
            "selected child cleanup should run when child execute fails"
        );
    }

    #[tokio::test]
    async fn execute_fails_closed_without_lease() {
        let caps = SandboxCapabilities {
            network: NetworkPolicySupport {
                unrestricted: true,
                ..NetworkPolicySupport::default()
            },
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        };
        let stub = StubBackend::new("child", caps);
        let router = RoutingSandboxBackend::new(vec![stub]).unwrap();
        let spec = spec_with_network(NetworkAccess::Unrestricted);
        let mut ctx = test_ctx();
        ctx.execution_id = 99; // no before_execute was called for this id

        let result = router.execute(spec, ctx).await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("execute without lease should fail"),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("99"),
            "error should mention the missing execution id: {msg}"
        );
    }

    #[tokio::test]
    async fn before_execute_cleans_up_on_child_before_execute_failure() {
        let caps_good = SandboxCapabilities {
            network: NetworkPolicySupport {
                unrestricted: true,
                ..NetworkPolicySupport::default()
            },
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        };
        let failing = StubBackend::new("failing", caps_good);
        failing.set_before_execute_result(Err(SandboxError::Unavailable {
            backend: "failing".to_owned(),
            detail: "not available".to_owned(),
        }));

        let router = RoutingSandboxBackend::new(vec![failing.clone()]).unwrap();
        let spec = spec_with_network(NetworkAccess::Unrestricted);
        let mut ctx = test_ctx();
        ctx.execution_id = 55;

        let err = router
            .before_execute(&spec, &ctx)
            .await
            .expect_err("before_execute should fail");
        let msg = err.to_string();
        assert!(
            msg.contains("failing"),
            "error should name failing backend: {msg}"
        );

        // No lease should exist.
        let has_lease = router.leases.lock().await.contains_key(&55);
        assert!(!has_lease, "lease should not be stored on failure");
    }

    #[tokio::test]
    async fn concurrent_executions_keep_separate_backends() {
        let caps_a = SandboxCapabilities {
            network: NetworkPolicySupport {
                unrestricted: true,
                ..NetworkPolicySupport::default()
            },
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        };
        let caps_b = caps_a.clone();

        let stub_a = StubBackend::new("a", caps_a);
        let stub_b = StubBackend::new("b", caps_b);

        let router =
            Arc::new(RoutingSandboxBackend::new(vec![stub_a.clone(), stub_b.clone()]).unwrap());

        let spec = spec_with_network(NetworkAccess::Unrestricted);
        let mut ctx1 = test_ctx();
        ctx1.execution_id = 1;
        let mut ctx2 = test_ctx();
        ctx2.execution_id = 2;

        // Run two before_execute calls concurrently.
        let router1 = Arc::clone(&router);
        let router2 = Arc::clone(&router);
        let spec1 = spec.clone();
        let spec2 = spec.clone();

        let (r1, r2) = tokio::join!(
            async move {
                router1
                    .before_execute(&spec1, &ctx1)
                    .await
                    .expect("before_execute 1");
                router1.execute(spec1, ctx1).await.expect("execute 1")
            },
            async move {
                router2
                    .before_execute(&spec2, &ctx2)
                    .await
                    .expect("before_execute 2");
                router2.execute(spec2, ctx2).await.expect("execute 2")
            },
        );

        // Both handles should route through the router — verify by waiting on them
        // and checking that the child after_execute was invoked.
        let outcome1 = r1.activity.wait().await.expect("wait 1");
        let outcome2 = r2.activity.wait().await.expect("wait 2");
        drop(outcome1);
        drop(outcome2);

        // Both should pick stub_a first (stub_b is second in list).
        assert_eq!(stub_a.execute_count(), 2);
        assert_eq!(stub_b.execute_count(), 0);
        // after_execute should be called on each selected child separately.
        assert_eq!(
            stub_a.after_execute_count(),
            2,
            "after_execute should be called per execution"
        );
    }

    #[tokio::test]
    async fn failure_message_lists_candidate_ids_and_reasons() {
        let caps_none_only = SandboxCapabilities {
            network: NetworkPolicySupport {
                none: true,
                ..NetworkPolicySupport::default()
            },
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        };
        let a = StubBackend::new("backend_a", caps_none_only.clone());
        let b = StubBackend::new("backend_b", caps_none_only);
        let router = RoutingSandboxBackend::new(vec![a, b]).unwrap();

        // Request Unrestricted — neither backend supports it.
        let spec = spec_with_network(NetworkAccess::Unrestricted);
        let result = router.select_backend(&spec);
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("select_backend should fail"),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("backend_a"),
            "error should list backend_a: {msg}"
        );
        assert!(
            msg.contains("backend_b"),
            "error should list backend_b: {msg}"
        );
    }

    #[tokio::test]
    async fn router_after_execute_is_noop() {
        let caps = SandboxCapabilities {
            network: NetworkPolicySupport {
                unrestricted: true,
                ..NetworkPolicySupport::default()
            },
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        };
        let stub = StubBackend::new("child", caps);
        let router = RoutingSandboxBackend::new(vec![stub]).unwrap();

        // Router-level after_execute should return Ok without calling child.
        router
            .after_execute(&ExecOutcome::default(), &test_ctx())
            .await
            .expect("router after_execute should no-op successfully");
    }

    #[tokio::test]
    async fn select_backend_falls_back_to_second_when_first_cannot_enforce() {
        let caps_restricted = SandboxCapabilities {
            network: NetworkPolicySupport {
                none: true,
                unrestricted: false,
                ..NetworkPolicySupport::default()
            },
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        };
        let caps_unrestricted = SandboxCapabilities {
            network: NetworkPolicySupport {
                unrestricted: true,
                ..NetworkPolicySupport::default()
            },
            max_concurrent_execs: 1,
            ..SandboxCapabilities::default()
        };

        let a = StubBackend::new("a", caps_restricted);
        let b = StubBackend::new("b", caps_unrestricted);
        let router = RoutingSandboxBackend::new(vec![a, b]).unwrap();

        let spec = spec_with_network(NetworkAccess::Unrestricted);
        let selected = router.select_backend(&spec).unwrap();
        assert_eq!(
            selected.backend_id(),
            "b",
            "should select the second backend when the first cannot enforce"
        );
    }
}

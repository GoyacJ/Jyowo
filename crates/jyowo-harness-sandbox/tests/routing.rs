//! Integration tests for RoutingSandboxBackend.
//!
//! These tests verify the routing sandbox lifecycle correctness and fail-closed
//! behavior using stub backends that do not depend on optional feature gates.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use harness_contracts::{Event, KillScope, NetworkAccess, SandboxError, SandboxPolicy};
use harness_sandbox::{
    ActivityHandle, EventSink, ExecContext, ExecOutcome, ExecSpec, NetworkPolicySupport,
    ProcessHandle, RoutingSandboxBackend, SandboxBackend, SandboxCapabilities, SessionSnapshotFile,
    Signal, SnapshotSpec,
};

// ---------------------------------------------------------------------------
// shared test infrastructure
// ---------------------------------------------------------------------------

#[derive(Default)]
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

fn stub_caps_with_network(caps: NetworkPolicySupport) -> SandboxCapabilities {
    SandboxCapabilities {
        network: caps,
        max_concurrent_execs: 1,
        ..SandboxCapabilities::default()
    }
}

/// A minimal backend that returns fixed capabilities and passes/fails preflight
/// according to those capabilities. execute always returns a simple handle.
struct MinimalStub {
    id: String,
    caps: SandboxCapabilities,
    before_count: AtomicU64,
    execute_count: AtomicU64,
    after_count: AtomicU64,
}

impl MinimalStub {
    fn new(id: &str, caps: SandboxCapabilities) -> Arc<Self> {
        Arc::new(Self {
            id: id.to_owned(),
            caps,
            before_count: AtomicU64::new(0),
            execute_count: AtomicU64::new(0),
            after_count: AtomicU64::new(0),
        })
    }
}

struct StubActivity {
    outcome: ExecOutcome,
}

#[async_trait::async_trait]
impl ActivityHandle for StubActivity {
    async fn wait(&self) -> Result<ExecOutcome, SandboxError> {
        Ok(self.outcome.clone())
    }

    async fn kill(&self, _signal: Signal, _scope: KillScope) -> Result<(), SandboxError> {
        Ok(())
    }

    fn touch(&self) {}

    fn last_activity(&self) -> Instant {
        Instant::now()
    }
}

fn stub_handle() -> ProcessHandle {
    ProcessHandle {
        pid: None,
        stdout: None,
        stderr: None,
        stdin: None,
        cwd_marker: None,
        activity: Arc::new(StubActivity {
            outcome: ExecOutcome::default(),
        }),
    }
}

#[async_trait::async_trait]
impl SandboxBackend for MinimalStub {
    fn backend_id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> SandboxCapabilities {
        self.caps.clone()
    }

    fn preflight_execute(&self, spec: &ExecSpec) -> Result<(), SandboxError> {
        harness_sandbox::validate_preflight_capabilities(self.backend_id(), &self.caps, spec)
    }

    async fn before_execute(
        &self,
        _spec: &ExecSpec,
        _ctx: &ExecContext,
    ) -> Result<(), SandboxError> {
        self.before_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn execute(
        &self,
        _spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        self.execute_count.fetch_add(1, Ordering::SeqCst);
        Ok(stub_handle())
    }

    async fn after_execute(
        &self,
        _outcome: &ExecOutcome,
        _ctx: &ExecContext,
    ) -> Result<(), SandboxError> {
        self.after_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "minimal".to_owned(),
        })
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "minimal".to_owned(),
        })
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[test]
fn new_rejects_empty_backends() {
    let result = RoutingSandboxBackend::new(vec![]);
    assert!(result.is_err());
}

#[tokio::test]
async fn selects_first_backend_capable_of_network_none() {
    let caps = stub_caps_with_network(NetworkPolicySupport {
        none: true,
        ..NetworkPolicySupport::default()
    });
    let a = MinimalStub::new("a", caps.clone());
    let b = MinimalStub::new("b", caps);
    let router = RoutingSandboxBackend::new(vec![a.clone(), b.clone()]).unwrap();

    let spec = spec_with_network(NetworkAccess::None);
    let selected = router.select_backend(&spec).unwrap();
    assert_eq!(selected.backend_id(), "a");
}

#[test]
fn selects_child_that_supports_required_synchronous_kill_scope() {
    let common_caps = SandboxCapabilities {
        network: NetworkPolicySupport {
            unrestricted: true,
            ..NetworkPolicySupport::default()
        },
        supports_kill_scope: vec![KillScope::ProcessGroup],
        max_concurrent_execs: 1,
        ..SandboxCapabilities::default()
    };
    let asynchronous_only = MinimalStub::new("asynchronous", common_caps.clone());
    let synchronous = MinimalStub::new(
        "synchronous",
        SandboxCapabilities {
            supports_synchronous_kill_scope: vec![KillScope::ProcessGroup],
            ..common_caps
        },
    );
    let router = RoutingSandboxBackend::new(vec![asynchronous_only, synchronous]).unwrap();
    let spec = ExecSpec {
        required_kill_scope: Some(KillScope::ProcessGroup),
        required_synchronous_kill_scope: Some(KillScope::ProcessGroup),
        ..spec_with_network(NetworkAccess::Unrestricted)
    };

    let selected = router.select_backend(&spec).unwrap();

    assert_eq!(selected.backend_id(), "synchronous");
}

#[tokio::test]
async fn refuses_restricted_network_when_only_unrestricted_available() {
    let caps = stub_caps_with_network(NetworkPolicySupport {
        unrestricted: true,
        ..NetworkPolicySupport::default()
    });
    let child = MinimalStub::new("child", caps);
    let router = RoutingSandboxBackend::new(vec![child]).unwrap();

    let spec = spec_with_network(NetworkAccess::None);
    let result = router.preflight_execute(&spec);
    assert!(
        result.is_err(),
        "router should refuse restricted policy when no child can enforce it"
    );
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("child"), "error should name child: {msg}");
}

#[tokio::test]
async fn lifecycle_runs_before_and_after_on_selected_child() {
    let caps = stub_caps_with_network(NetworkPolicySupport {
        unrestricted: true,
        ..NetworkPolicySupport::default()
    });
    let child = MinimalStub::new("selected", caps);
    let router = RoutingSandboxBackend::new(vec![child.clone()]).unwrap();

    let spec = spec_with_network(NetworkAccess::Unrestricted);
    let mut ctx = test_ctx();
    ctx.execution_id = 1;

    // before_execute
    router.before_execute(&spec, &ctx).await.unwrap();
    assert_eq!(child.before_count.load(Ordering::SeqCst), 1);

    // execute
    let handle = router.execute(spec.clone(), ctx.clone()).await.unwrap();
    assert_eq!(child.execute_count.load(Ordering::SeqCst), 1);

    // wait → after_execute
    assert_eq!(child.after_count.load(Ordering::SeqCst), 0);
    handle.activity.wait().await.unwrap();
    assert_eq!(
        child.after_count.load(Ordering::SeqCst),
        1,
        "after_execute should be called after wait"
    );
}

#[tokio::test]
async fn execute_fails_closed_without_preceding_before_execute() {
    let caps = stub_caps_with_network(NetworkPolicySupport {
        unrestricted: true,
        ..NetworkPolicySupport::default()
    });
    let child = MinimalStub::new("child", caps);
    let router = RoutingSandboxBackend::new(vec![child]).unwrap();

    let spec = spec_with_network(NetworkAccess::Unrestricted);
    let mut ctx = test_ctx();
    ctx.execution_id = 99;

    let result = router.execute(spec, ctx).await;
    match result {
        Err(e) => {
            assert!(
                e.to_string().contains("99"),
                "error should mention execution id"
            );
        }
        Ok(_) => panic!("execute without before_execute must fail closed"),
    }
}

#[tokio::test]
async fn failure_message_lists_candidate_ids() {
    let caps_restricted = stub_caps_with_network(NetworkPolicySupport {
        none: true,
        ..NetworkPolicySupport::default()
    });
    let a = MinimalStub::new("backend_a", caps_restricted.clone());
    let b = MinimalStub::new("backend_b", caps_restricted);
    let router = RoutingSandboxBackend::new(vec![a, b]).unwrap();

    let spec = spec_with_network(NetworkAccess::Unrestricted);
    let result = router.select_backend(&spec);
    match result {
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("backend_a"),
                "error should list backend_a: {msg}"
            );
            assert!(
                msg.contains("backend_b"),
                "error should list backend_b: {msg}"
            );
        }
        Ok(_) => panic!("should fail"),
    }
}

#[tokio::test]
async fn concurrent_executions_keep_separate_backends() {
    let caps = stub_caps_with_network(NetworkPolicySupport {
        unrestricted: true,
        ..NetworkPolicySupport::default()
    });
    let child_a = MinimalStub::new("a", caps.clone());
    let child_b = MinimalStub::new("b", caps);
    let router =
        Arc::new(RoutingSandboxBackend::new(vec![child_a.clone(), child_b.clone()]).unwrap());

    let spec = spec_with_network(NetworkAccess::Unrestricted);
    let mut ctx1 = test_ctx();
    ctx1.execution_id = 1;
    let mut ctx2 = test_ctx();
    ctx2.execution_id = 2;

    let r1 = Arc::clone(&router);
    let r2 = Arc::clone(&router);
    let s1 = spec.clone();
    let s2 = spec.clone();

    let (h1, h2) = tokio::join!(
        async move {
            r1.before_execute(&s1, &ctx1).await.unwrap();
            r1.execute(s1, ctx1).await.unwrap()
        },
        async move {
            r2.before_execute(&s2, &ctx2).await.unwrap();
            r2.execute(s2, ctx2).await.unwrap()
        },
    );

    // Both should route to child_a first.
    assert_eq!(child_a.execute_count.load(Ordering::SeqCst), 2);
    assert_eq!(child_b.execute_count.load(Ordering::SeqCst), 0);

    // Both should call after_execute separately.
    h1.activity.wait().await.unwrap();
    h2.activity.wait().await.unwrap();
    assert_eq!(child_a.after_count.load(Ordering::SeqCst), 2);
}

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use futures::stream;
use futures::stream::BoxStream;
use harness_contracts::{
    Event, KillScope, NetworkAccess, SandboxError, SandboxExitStatus, WorkspaceAccess,
};
use harness_sandbox::{
    execute_skill_script, ActivityHandle, EventSink, ExecContext, ExecOutcome, ExecSpec,
    NetworkPolicySupport, ProcessHandle, ResourceLimitSupport, SandboxBackend, SandboxBaseConfig,
    SandboxCapabilities, SessionSnapshotFile, SkillScriptPackFile, SkillScriptSandboxRequest,
    SkillScriptStatus, SnapshotSpec,
};
use harness_skill::{SkillScriptDecl, SkillScriptNetworkPolicy};
use serde_json::json;

#[derive(Default)]
struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: Event) -> Result<(), SandboxError> {
        Ok(())
    }
}

struct TestActivity {
    delay: Duration,
    exit_status: SandboxExitStatus,
    killed: Arc<AtomicUsize>,
}

#[async_trait]
impl ActivityHandle for TestActivity {
    async fn wait(&self) -> Result<ExecOutcome, SandboxError> {
        tokio::time::sleep(self.delay).await;
        Ok(ExecOutcome {
            exit_status: self.exit_status.clone(),
            started_at: Utc::now(),
            finished_at: Utc::now(),
            ..ExecOutcome::default()
        })
    }

    async fn kill(&self, _signal: i32, _scope: KillScope) -> Result<(), SandboxError> {
        self.killed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn touch(&self) {}

    fn last_activity(&self) -> Instant {
        Instant::now()
    }
}

struct TestBackend {
    network_deny: bool,
    executed: AtomicUsize,
    recorded: Mutex<Vec<ExecSpec>>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    delay: Duration,
    exit_status: SandboxExitStatus,
    artifacts: Vec<(String, Vec<u8>)>,
    pending_output: bool,
    allowed_env: BTreeSet<String>,
    killed: Arc<AtomicUsize>,
}

impl TestBackend {
    fn accepting() -> Self {
        Self {
            network_deny: true,
            executed: AtomicUsize::new(0),
            recorded: Mutex::new(Vec::new()),
            stdout: Vec::new(),
            stderr: Vec::new(),
            delay: Duration::ZERO,
            exit_status: SandboxExitStatus::Code(0),
            artifacts: Vec::new(),
            pending_output: false,
            allowed_env: BTreeSet::new(),
            killed: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl SandboxBackend for TestBackend {
    fn backend_id(&self) -> &str {
        "skill-script-test"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            supports_streaming: true,
            network: NetworkPolicySupport {
                none: self.network_deny,
                loopback_only: false,
                allowlist: false,
                unrestricted: false,
            },
            workspace: harness_sandbox::WorkspacePolicySupport {
                read_write_all: false,
                read_only: false,
                writable_subpaths: true,
            },
            max_concurrent_execs: 1,
            snapshot_kinds: BTreeSet::new(),
            resource_limit_support: ResourceLimitSupport {
                wall_clock: true,
                ..ResourceLimitSupport::default()
            },
            ..SandboxCapabilities::default()
        }
    }

    fn base_config(&self) -> SandboxBaseConfig {
        SandboxBaseConfig {
            passthrough_env_keys: self.allowed_env.clone(),
            ..SandboxBaseConfig::default()
        }
    }

    async fn execute(
        &self,
        spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        self.executed.fetch_add(1, Ordering::SeqCst);
        for (path, content) in &self.artifacts {
            let target = spec.cwd.as_ref().expect("runner must set cwd").join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| SandboxError::Message(error.to_string()))?;
            }
            std::fs::write(target, content)
                .map_err(|error| SandboxError::Message(error.to_string()))?;
        }
        self.recorded.lock().unwrap().push(spec);
        let stdout: Option<BoxStream<'static, Bytes>> = if self.pending_output {
            Some(Box::pin(stream::pending()))
        } else {
            (!self.stdout.is_empty())
                .then(|| Box::pin(stream::iter(vec![Bytes::copy_from_slice(&self.stdout)])) as _)
        };
        let stderr = (!self.stderr.is_empty())
            .then(|| Box::pin(stream::iter(vec![Bytes::copy_from_slice(&self.stderr)])) as _);
        Ok(ProcessHandle {
            pid: Some(42),
            stdout,
            stderr,
            stdin: None,
            cwd_marker: None,
            activity: Arc::new(TestActivity {
                delay: self.delay,
                exit_status: self.exit_status.clone(),
                killed: Arc::clone(&self.killed),
            }),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

#[tokio::test]
async fn rejects_backend_that_cannot_enforce_network_denial() {
    let backend = Arc::new(TestBackend {
        network_deny: false,
        ..TestBackend::accepting()
    });

    let error = execute_skill_script(backend.clone(), request(script_decl()), test_context())
        .await
        .expect_err("backend without network denial must be rejected");

    assert!(matches!(
        error,
        SandboxError::CapabilityMismatch { ref capability, .. } if capability == "network"
    ));
    assert_eq!(backend.executed.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn injects_only_explicit_declared_environment_values() {
    let backend = Arc::new(TestBackend {
        allowed_env: BTreeSet::from(["TOKEN".to_owned()]),
        ..TestBackend::accepting()
    });
    let mut declaration = script_decl();
    declaration.env.insert(
        "TOKEN".to_owned(),
        harness_skill::SkillScriptEnvDecl {
            config: "token".to_owned(),
            secret: true,
        },
    );
    let mut request = request(declaration);
    request.env.insert("TOKEN".to_owned(), "secret".to_owned());

    execute_skill_script(backend.clone(), request, test_context())
        .await
        .expect("declared environment should execute");

    let specs = backend.recorded.lock().unwrap();
    assert_eq!(
        specs[0].env,
        BTreeMap::from([("TOKEN".to_owned(), "secret".to_owned())])
    );
    assert_eq!(specs[0].policy.network, NetworkAccess::None);
    assert!(matches!(
        &specs[0].workspace_access,
        WorkspaceAccess::ReadWrite { allowed_writable_subpaths } if allowed_writable_subpaths.len() == 1
    ));
}

#[tokio::test]
async fn rejects_backend_that_would_drop_declared_environment_values() {
    let backend = Arc::new(TestBackend::accepting());
    let mut declaration = script_decl();
    declaration.env.insert(
        "TOKEN".to_owned(),
        harness_skill::SkillScriptEnvDecl {
            config: "token".to_owned(),
            secret: true,
        },
    );
    let mut request = request(declaration);
    request.env.insert("TOKEN".to_owned(), "secret".to_owned());

    let error = execute_skill_script(backend.clone(), request, test_context())
        .await
        .expect_err("backend that would drop declared env must be rejected");

    assert!(matches!(
        error,
        SandboxError::CapabilityMismatch { ref capability, .. } if capability == "environment"
    ));
    assert_eq!(backend.executed.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn rejects_undeclared_environment_values() {
    let backend = Arc::new(TestBackend::accepting());
    let mut request = request(script_decl());
    request
        .env
        .insert("UNDECLARED".to_owned(), "unsafe".to_owned());

    let error = execute_skill_script(backend.clone(), request, test_context())
        .await
        .expect_err("undeclared environment must be rejected");

    assert!(error.to_string().contains("undeclared environment"));
    assert_eq!(backend.executed.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn enforces_timeout_and_kills_backend_process() {
    let backend = Arc::new(TestBackend {
        delay: Duration::from_secs(2),
        ..TestBackend::accepting()
    });
    let mut declaration = script_decl();
    declaration.timeout_seconds = 1;

    let result = execute_skill_script(backend.clone(), request(declaration), test_context())
        .await
        .expect("timeout is a bounded result");

    assert_eq!(result.status, SkillScriptStatus::TimedOut);
    assert_eq!(backend.killed.load(Ordering::SeqCst), 1);
    assert_eq!(result.enforced_policy.timeout_ms, 1_000);
}

#[tokio::test]
async fn timeout_stays_bounded_when_backend_output_never_closes() {
    let backend = Arc::new(TestBackend {
        delay: Duration::from_secs(2),
        pending_output: true,
        ..TestBackend::accepting()
    });
    let mut declaration = script_decl();
    declaration.timeout_seconds = 1;

    let result = tokio::time::timeout(
        Duration::from_millis(1_500),
        execute_skill_script(backend, request(declaration), test_context()),
    )
    .await
    .expect("runner must not wait forever for output after timeout")
    .expect("timeout should be returned as a bounded result");

    assert_eq!(result.status, SkillScriptStatus::TimedOut);
}

#[tokio::test]
async fn truncates_stdout_stderr_and_combined_output() {
    let backend = Arc::new(TestBackend {
        stdout: b"abcdef".to_vec(),
        stderr: b"uvwxyz".to_vec(),
        ..TestBackend::accepting()
    });
    let mut declaration = script_decl();
    declaration.max_stdout_bytes = 4;
    declaration.max_stderr_bytes = 4;
    declaration.max_output_bytes = 6;

    let result = execute_skill_script(backend, request(declaration), test_context())
        .await
        .expect("bounded output should be returned");

    assert_eq!(result.status, SkillScriptStatus::OutputLimitExceeded);
    assert_eq!(result.stdout, "abcd");
    assert_eq!(result.stderr, "uv");
    assert_eq!(result.stdout.len() + result.stderr.len(), 6);
}

#[tokio::test]
async fn bounds_artifact_count_and_total_bytes() {
    let backend = Arc::new(TestBackend {
        artifacts: vec![
            ("a.txt".to_owned(), b"1234".to_vec()),
            ("b.txt".to_owned(), b"5678".to_vec()),
            ("c.txt".to_owned(), b"ignored".to_vec()),
        ],
        ..TestBackend::accepting()
    });
    let mut declaration = script_decl();
    declaration.max_artifact_count = 2;
    declaration.max_artifact_bytes = 6;

    let result = execute_skill_script(backend, request(declaration), test_context())
        .await
        .expect("bounded artifacts should be returned");

    assert_eq!(result.status, SkillScriptStatus::ArtifactLimitExceeded);
    assert_eq!(result.artifacts.len(), 2);
    assert_eq!(
        result
            .artifacts
            .iter()
            .map(|artifact| artifact.content.len())
            .sum::<usize>(),
        6
    );
    assert!(result.artifacts[1].truncated);
}

#[tokio::test]
async fn rejects_script_and_files_outside_materialized_package() {
    let backend = Arc::new(TestBackend::accepting());
    let mut declaration = script_decl();
    declaration.path = PathBuf::from("../outside.sh");
    let error = execute_skill_script(backend.clone(), request(declaration), test_context())
        .await
        .expect_err("package escape must be rejected");
    assert!(matches!(error, SandboxError::HostPathDenied { .. }));

    let mut unsafe_file = request(script_decl());
    unsafe_file.files.push(SkillScriptPackFile {
        path: "../outside.txt".to_owned(),
        content: "unsafe".to_owned(),
    });
    let error = execute_skill_script(backend.clone(), unsafe_file, test_context())
        .await
        .expect_err("file escape must be rejected");
    assert!(matches!(error, SandboxError::HostPathDenied { .. }));
    assert_eq!(backend.executed.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn result_serialization_contains_only_enforced_policy_fields() {
    let result = execute_skill_script(
        Arc::new(TestBackend::accepting()),
        request(script_decl()),
        test_context(),
    )
    .await
    .expect("script should execute");
    let value = serde_json::to_value(result).expect("result should serialize");

    assert!(value.get("enforced_policy").is_some());
    assert!(value.get("memory_mb").is_none());
    assert!(value.get("memory_limit_mb").is_none());
    assert!(value.get("network_enabled").is_none());
}

fn script_decl() -> SkillScriptDecl {
    SkillScriptDecl {
        id: "run".to_owned(),
        path: PathBuf::from("scripts/run.sh"),
        timeout_seconds: 2,
        network: SkillScriptNetworkPolicy::Deny,
        env: BTreeMap::new(),
        max_stdout_bytes: 64,
        max_stderr_bytes: 64,
        max_output_bytes: 128,
        max_artifact_count: 8,
        max_artifact_bytes: 1024,
    }
}

fn request(declaration: SkillScriptDecl) -> SkillScriptSandboxRequest {
    SkillScriptSandboxRequest {
        declaration,
        input: json!({ "name": "jyowo" }),
        files: vec![SkillScriptPackFile {
            path: "scripts/run.sh".to_owned(),
            content: "printf ok\n".to_owned(),
        }],
        env: BTreeMap::new(),
    }
}

fn test_context() -> ExecContext {
    ExecContext::for_test(Arc::new(NullSink))
}

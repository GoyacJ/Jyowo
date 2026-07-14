#![cfg(all(feature = "local", unix))]

use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use futures::StreamExt;
use harness_contracts::{
    BlobError, BlobMeta, BlobRef, BlobStore, Event, HostRule, KillScope, NetworkAccess,
    RedactRules, Redactor, SandboxError, SandboxExitStatus, SessionSnapshotKind, TenantId,
    WorkspaceAccess,
};
use harness_sandbox::{
    execute_with_lifecycle, preflight_exec, EventSink, ExecContext, ExecSpec, LocalIsolation,
    LocalSandbox, OutputOverflowPolicy, SandboxBackend, SandboxBaseConfig, SessionSnapshotFile,
    SnapshotMetadata, SnapshotSpec, StdioSpec,
};
use parking_lot::Mutex;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

struct RecordingSink {
    tx: UnboundedSender<Event>,
}

struct NullSink;

struct RejectStartedSink {
    pid_file: PathBuf,
}

struct BlockingRejectStartedSink {
    pid_file: PathBuf,
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
}

#[derive(Default)]
struct ReplacementRedactor;

impl Redactor for ReplacementRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.replace("secret", "[MASK]")
    }
}

#[derive(Default)]
struct RecordingBlobStore {
    puts: Mutex<Vec<(Bytes, BlobMeta)>>,
}

impl RecordingBlobStore {
    fn puts(&self) -> Vec<(Bytes, BlobMeta)> {
        self.puts.lock().clone()
    }
}

#[async_trait::async_trait]
impl BlobStore for RecordingBlobStore {
    fn store_id(&self) -> &'static str {
        "recording"
    }

    async fn put(
        &self,
        _tenant: TenantId,
        bytes: Bytes,
        meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        self.puts.lock().push((bytes, meta.clone()));
        Ok(BlobRef {
            id: harness_contracts::BlobId::new(),
            size: meta.size,
            content_hash: meta.content_hash,
            content_type: meta.content_type,
        })
    }

    async fn get(
        &self,
        _tenant: TenantId,
        _blob: &BlobRef,
    ) -> Result<futures::stream::BoxStream<'static, Bytes>, BlobError> {
        Err(BlobError::NotFound(harness_contracts::BlobId::new()))
    }

    async fn head(
        &self,
        _tenant: TenantId,
        _blob: &BlobRef,
    ) -> Result<Option<BlobMeta>, BlobError> {
        Ok(None)
    }

    async fn delete(&self, _tenant: TenantId, _blob: &BlobRef) -> Result<(), BlobError> {
        Ok(())
    }
}

impl EventSink for NullSink {
    fn emit(&self, _event: Event) -> Result<(), SandboxError> {
        Ok(())
    }
}

impl EventSink for RejectStartedSink {
    fn emit(&self, event: Event) -> Result<(), SandboxError> {
        if matches!(event, Event::SandboxExecutionStarted(_)) {
            let started = std::time::Instant::now();
            while !self.pid_file.exists() && started.elapsed() < Duration::from_secs(1) {
                std::thread::sleep(Duration::from_millis(1));
            }
            return Err(SandboxError::Message(
                "sandbox execution started event rejected".to_owned(),
            ));
        }
        Ok(())
    }
}

impl EventSink for BlockingRejectStartedSink {
    fn emit(&self, event: Event) -> Result<(), SandboxError> {
        if matches!(event, Event::SandboxExecutionStarted(_)) {
            let started = std::time::Instant::now();
            while !self.pid_file.exists() && started.elapsed() < Duration::from_secs(1) {
                std::thread::sleep(Duration::from_millis(1));
            }
            self.entered.wait();
            self.release.wait();
            return Err(SandboxError::Message(
                "sandbox execution started event rejected".to_owned(),
            ));
        }
        Ok(())
    }
}

impl EventSink for RecordingSink {
    fn emit(&self, event: Event) -> Result<(), SandboxError> {
        self.tx
            .send(event)
            .map_err(|error| SandboxError::Message(error.to_string()))
    }
}

fn recording_sink() -> (Arc<RecordingSink>, UnboundedReceiver<Event>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (Arc::new(RecordingSink { tx }), rx)
}

fn drain_events(rx: &mut UnboundedReceiver<Event>) -> Vec<Event> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

fn temp_root(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "jyowo-harness-sandbox-{name}-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("temp root should be created");
    root
}

fn shell_spec(script: &str) -> ExecSpec {
    let mut spec = ExecSpec {
        command: "/bin/sh".to_owned(),
        args: vec!["-c".to_owned(), script.to_owned()],
        stdin: StdioSpec::Null,
        stdout: StdioSpec::Piped,
        stderr: StdioSpec::Piped,
        ..ExecSpec::default()
    };
    spec.policy.network = NetworkAccess::Unrestricted;
    spec
}

async fn collect_stdout(mut stdout: futures::stream::BoxStream<'static, bytes::Bytes>) -> String {
    let mut bytes = Vec::new();
    while let Some(chunk) = stdout.next().await {
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).expect("stdout should be utf8")
}

#[tokio::test]
async fn local_sandbox_is_object_safe_and_streams_stdout() {
    let root = temp_root("echo");
    let (sink, mut rx) = recording_sink();
    let ctx = ExecContext::for_test(sink);
    let sandbox: Arc<dyn SandboxBackend> = Arc::new(LocalSandbox::new(&root));

    let mut handle = sandbox
        .execute(shell_spec("printf hello"), ctx)
        .await
        .expect("execute should spawn process");
    let stdout = handle.stdout.take().expect("stdout should be piped");
    let output = collect_stdout(stdout).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(output, "hello");
    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));
    assert_eq!(outcome.stdout_bytes_observed, 5);

    let events = drain_events(&mut rx);
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::SandboxExecutionStarted(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::SandboxExecutionCompleted(_))));
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires a live bubblewrap host with unprivileged user namespaces"]
async fn bubblewrap_workspace_only_hides_a_host_sentinel() {
    if !std::process::Command::new("bwrap")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        return;
    }

    let host_root = temp_root("bubblewrap-host-sentinel");
    let workspace = host_root.join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace should be created");
    let sentinel = host_root.join("host-secret.txt");
    std::fs::write(&sentinel, "must stay hidden").expect("sentinel should be written");
    let sandbox = LocalSandbox::new(&workspace).with_isolation(LocalIsolation::Bubblewrap);
    let mut spec = shell_spec(&format!(
        "if test -e '{}'; then printf visible; else printf hidden; fi",
        sentinel.display()
    ));
    spec.policy.network = NetworkAccess::None;
    spec.workspace_access = WorkspaceAccess::ReadOnly;

    let mut handle = sandbox
        .execute(spec, ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("bubblewrap should execute with the minimal runtime root");
    let stdout = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));
    assert_eq!(stdout, "hidden");
}

#[tokio::test]
async fn local_sandbox_emits_activity_heartbeat_when_output_is_observed() {
    let root = temp_root("heartbeat");
    let (sink, mut rx) = recording_sink();
    let sandbox = LocalSandbox::new(&root);

    let mut handle = sandbox
        .execute(shell_spec("printf hello"), ExecContext::for_test(sink))
        .await
        .expect("execute should spawn process");
    let stdout = handle.stdout.take().expect("stdout should be piped");
    let output = collect_stdout(stdout).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(output, "hello");
    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));
    assert!(drain_events(&mut rx).iter().any(|event| matches!(
        event,
        Event::SandboxActivityHeartbeat(heartbeat)
            if heartbeat.backend_id == "local" && heartbeat.since_last_io_ms <= 5_000
    )));
}

#[tokio::test]
async fn local_sandbox_applies_relative_cwd_inside_root_and_rejects_escape() {
    let root = temp_root("cwd");
    std::fs::create_dir_all(root.join("child")).expect("child dir should be created");
    let sandbox = LocalSandbox::new(&root);
    let ctx = ExecContext::for_test(Arc::new(NullSink));

    let mut spec = shell_spec("printf '%s' \"$(basename \"$PWD\")\"");
    spec.cwd = Some(PathBuf::from("./child/../child"));
    let mut handle = sandbox
        .execute(spec, ctx.clone())
        .await
        .expect("cwd inside root should spawn");
    let output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");
    assert_eq!(output, "child");
    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));

    let mut escaping = shell_spec("printf nope");
    escaping.cwd = Some(PathBuf::from("../"));
    let error = match sandbox.execute(escaping, ctx).await {
        Ok(_) => panic!("cwd escape should be rejected"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SandboxError::HostPathDenied { ref path } if path == ".."
    ));
}

#[tokio::test]
async fn local_sandbox_rejects_unsupported_network_restriction_and_denied_paths() {
    let root = temp_root("policy");
    std::fs::create_dir_all(root.join("secret")).expect("secret dir should be created");
    let sandbox = LocalSandbox::with_base(
        &root,
        SandboxBaseConfig {
            denied_host_paths: vec![PathBuf::from("secret")],
            ..SandboxBaseConfig::default()
        },
    );
    let ctx = ExecContext::for_test(Arc::new(NullSink));

    let mut restricted = shell_spec("printf nope");
    restricted.policy.network = NetworkAccess::None;
    let error = match sandbox.execute(restricted, ctx.clone()).await {
        Ok(_) => panic!("unsupported network-restricted local policy must fail closed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SandboxError::CapabilityMismatch {
            ref capability,
            ..
        } if capability == "network"
    ));

    let mut denied = shell_spec("printf nope");
    denied.cwd = Some(PathBuf::from("secret"));
    let error = match sandbox.execute(denied, ctx).await {
        Ok(_) => panic!("denied host path should be rejected before spawn"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SandboxError::HostPathDenied { ref path } if path.ends_with("secret")
    ));
}

#[tokio::test]
async fn local_sandbox_capability_truth_matches_unrestricted_only_without_os_isolation() {
    let root = temp_root("capability-truth");
    let sandbox = LocalSandbox::new(&root);
    let ctx = ExecContext::for_test(Arc::new(NullSink));

    assert!(!sandbox.capabilities().network.none);

    let unrestricted = shell_spec("printf ok");
    preflight_exec(&sandbox, &unrestricted, &ctx)
        .expect("unrestricted network does not require local network enforcement");

    for network in [
        NetworkAccess::None,
        NetworkAccess::LoopbackOnly,
        NetworkAccess::AllowList(vec![HostRule {
            pattern: "localhost".to_owned(),
            ports: None,
        }]),
    ] {
        let mut spec = shell_spec("printf nope");
        spec.policy.network = network;
        let error = preflight_exec(&sandbox, &spec, &ctx)
            .expect_err("unenforceable local network policy must fail preflight");
        assert!(matches!(
            error,
            SandboxError::CapabilityMismatch {
                ref capability,
                ..
            } if capability == "network"
        ));
    }
}

#[tokio::test]
async fn execute_with_lifecycle_emits_local_preflight_before_started() {
    let root = temp_root("local-lifecycle-preflight");
    let (sink, mut rx) = recording_sink();
    let sandbox: Arc<dyn SandboxBackend> = Arc::new(LocalSandbox::new(&root));

    let mut handle = execute_with_lifecycle(
        sandbox,
        shell_spec("printf ok"),
        ExecContext::for_test(sink),
    )
    .await
    .expect("execute should spawn process");
    let output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(output, "ok");
    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));
    let events = drain_events(&mut rx);
    let preflight_index = events
        .iter()
        .position(|event| matches!(event, Event::SandboxPreflightPassed(_)))
        .expect("preflight passed event should be emitted");
    let started_index = events
        .iter()
        .position(|event| matches!(event, Event::SandboxExecutionStarted(_)))
        .expect("execution started event should be emitted");
    assert!(preflight_index < started_index);
}

#[tokio::test]
async fn local_sandbox_rejects_unenforceable_workspace_access_modes() {
    let root = temp_root("workspace-access");
    let sandbox = LocalSandbox::new(&root);
    let ctx = ExecContext::for_test(Arc::new(NullSink));

    let mut read_only = shell_spec("printf nope > should-not-exist");
    read_only.workspace_access = WorkspaceAccess::ReadOnly;
    let error = match sandbox.execute(read_only, ctx.clone()).await {
        Ok(_) => panic!("read-only local workspace access must fail closed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SandboxError::CapabilityMismatch {
            ref capability,
            ..
        } if capability == "workspace_access"
    ));
    assert!(!root.join("should-not-exist").exists());

    let mut scoped_write = shell_spec("printf nope > outside-allowed");
    scoped_write.workspace_access = WorkspaceAccess::ReadWrite {
        allowed_writable_subpaths: vec![PathBuf::from("tmp")],
    };
    let error = match sandbox.execute(scoped_write, ctx).await {
        Ok(_) => panic!("scoped local workspace writes must fail closed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SandboxError::CapabilityMismatch {
            ref capability,
            ..
        } if capability == "workspace_access"
    ));
    assert!(!root.join("outside-allowed").exists());
}

#[tokio::test]
async fn local_sandbox_denied_host_paths_cover_command_cwd_and_stdio_preflight() {
    let root = temp_root("denied-preflight");
    let secret = root.join("secret");
    std::fs::create_dir_all(&secret).expect("secret dir should be created");
    let denied_command = secret.join("tool");
    std::fs::write(&denied_command, "#!/bin/sh\n").expect("denied tool should be written");
    let sandbox = LocalSandbox::with_base(
        &root,
        SandboxBaseConfig {
            denied_host_paths: vec![PathBuf::from("secret")],
            ..SandboxBaseConfig::default()
        },
    );
    let ctx = ExecContext::for_test(Arc::new(NullSink));

    let mut cwd = shell_spec("printf nope");
    cwd.cwd = Some(PathBuf::from("secret"));
    let error = match sandbox.execute(cwd, ctx.clone()).await {
        Ok(_) => panic!("denied cwd should fail before spawn"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SandboxError::HostPathDenied { ref path } if path.ends_with("secret")
    ));

    let mut command = shell_spec("printf nope");
    command.command = denied_command.display().to_string();
    command.args.clear();
    let error = match sandbox.execute(command, ctx.clone()).await {
        Ok(_) => panic!("denied absolute command should fail before spawn"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SandboxError::HostPathDenied { ref path } if path.ends_with("secret/tool")
    ));

    let mut stdout = shell_spec("printf nope");
    stdout.stdout = StdioSpec::File(PathBuf::from("secret/out"));
    let error = match sandbox.execute(stdout, ctx).await {
        Ok(_) => panic!("denied stdio file should fail before spawn"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SandboxError::HostPathDenied { ref path } if path.ends_with("secret/out")
    ));
}

#[tokio::test]
async fn local_sandbox_rejects_unimplemented_network_modes_even_with_isolation() {
    let root = temp_root("network-modes");
    let sandbox = LocalSandbox::new(&root).with_isolation(LocalIsolation::for_current_platform());
    let ctx = ExecContext::for_test(Arc::new(NullSink));

    let mut loopback = shell_spec("printf nope");
    loopback.policy.network = NetworkAccess::LoopbackOnly;
    let error = match sandbox.execute(loopback, ctx.clone()).await {
        Ok(_) => panic!("loopback-only network policy must fail closed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SandboxError::CapabilityMismatch {
            ref capability,
            ..
        } if capability == "network"
    ));

    let mut allowlist = shell_spec("printf nope");
    allowlist.policy.network = NetworkAccess::AllowList(vec![HostRule {
        pattern: "localhost".to_owned(),
        ports: None,
    }]);
    let error = match sandbox.execute(allowlist, ctx).await {
        Ok(_) => panic!("allow-list network policy must fail closed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SandboxError::CapabilityMismatch {
            ref capability,
            ..
        } if capability == "network"
    ));
}

#[tokio::test]
async fn local_sandbox_rejects_resource_limits_beyond_wall_clock() {
    let root = temp_root("resource-limits");
    let sandbox = LocalSandbox::new(&root).with_isolation(LocalIsolation::for_current_platform());
    for limit in ["memory", "cpu", "pids", "open_files"] {
        let mut spec = shell_spec("printf nope");
        match limit {
            "memory" => spec.policy.resource_limits.max_memory_bytes = Some(16 * 1024 * 1024),
            "cpu" => spec.policy.resource_limits.max_cpu_cores = Some(0.5),
            "pids" => spec.policy.resource_limits.max_pids = Some(8),
            "open_files" => spec.policy.resource_limits.max_open_files = Some(16),
            _ => unreachable!("test table is exhaustive"),
        }

        let error = match sandbox
            .execute(spec, ExecContext::for_test(Arc::new(NullSink)))
            .await
        {
            Ok(_) => panic!("{limit} resource limit must fail closed"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            SandboxError::CapabilityMismatch {
                ref capability,
                ..
            } if capability == "resource_limits"
        ));
    }
}

#[tokio::test]
async fn local_sandbox_reports_cwd_marker_over_side_fd_without_polluting_stdout() {
    let root = temp_root("cwd-marker");
    std::fs::create_dir_all(root.join("child")).expect("child dir should be created");
    let sandbox = LocalSandbox::new(&root);

    let mut handle = sandbox
        .execute(
            shell_spec("cd child && printf stdout-clean"),
            ExecContext::for_test(Arc::new(NullSink)),
        )
        .await
        .expect("execute should spawn process");
    let output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let marker = handle
        .cwd_marker
        .take()
        .expect("cwd marker should be piped")
        .next()
        .await
        .expect("cwd marker should be emitted");
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(output, "stdout-clean");
    assert_eq!(marker.sequence, 1);
    assert!(marker.cwd.ends_with("child"));
    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));
}

#[tokio::test]
async fn local_sandbox_snapshot_restore_roundtrips_filesystem() {
    let root = temp_root("snapshot");
    std::fs::write(root.join("state.txt"), "before").unwrap();
    let (sink, mut rx) = recording_sink();
    let sandbox = LocalSandbox::new(&root).with_snapshot_event_sink(sink);
    let snapshot = sandbox
        .snapshot_session(&SnapshotSpec::default())
        .await
        .expect("snapshot should succeed");

    std::fs::write(root.join("state.txt"), "after").unwrap();
    sandbox
        .restore_session(&snapshot)
        .await
        .expect("restore should succeed");

    assert_eq!(
        std::fs::read_to_string(root.join("state.txt")).unwrap(),
        "before"
    );
    assert!(snapshot.metadata.size_bytes > 0);
    assert!(drain_events(&mut rx)
        .iter()
        .any(|event| matches!(event, Event::SandboxSnapshotCreated(_))));
}

#[tokio::test]
async fn local_sandbox_shell_state_snapshot_roundtrips_cwd_metadata() {
    let root = temp_root("shell-state");
    std::fs::create_dir_all(root.join("child")).expect("child dir should be created");
    std::fs::write(root.join(".jyowo-shell-state"), "cwd=child\n").unwrap();
    let sandbox = LocalSandbox::new(&root);

    let snapshot = sandbox
        .snapshot_session(&SnapshotSpec {
            kind: SessionSnapshotKind::ShellState,
            ..SnapshotSpec::default()
        })
        .await
        .expect("shell state snapshot should succeed");

    std::fs::write(root.join(".jyowo-shell-state"), "cwd=other\n").unwrap();
    sandbox
        .restore_session(&snapshot)
        .await
        .expect("shell state restore should succeed");

    assert_eq!(
        std::fs::read_to_string(root.join(".jyowo-shell-state")).unwrap(),
        "cwd=child\n"
    );
}

#[tokio::test]
async fn local_sandbox_emits_periodic_heartbeat_without_output() {
    let root = temp_root("periodic-heartbeat");
    let (sink, mut rx) = recording_sink();
    let sandbox = LocalSandbox::new(&root);
    let mut spec = shell_spec("sleep 0.35");
    spec.timeout = Some(Duration::from_secs(2));

    let handle = sandbox
        .execute(spec, ExecContext::for_test(sink))
        .await
        .expect("execute should spawn process");
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));
    assert!(drain_events(&mut rx).iter().any(|event| {
        matches!(
            event,
            Event::SandboxActivityHeartbeat(heartbeat)
                if heartbeat.backend_id == "local" && heartbeat.since_last_io_ms > 0
        )
    }));
}

#[tokio::test]
async fn local_sandbox_restore_rejects_path_traversal_archive() {
    let root = temp_root("snapshot-traversal");
    let archive_path = root.join("malicious.tar");
    let file = std::fs::File::create(&archive_path).unwrap();
    let mut builder = tar::Builder::new(file);
    let mut header = tar::Header::new_gnu();
    header.as_mut_bytes()[..13].copy_from_slice(b"../escape.txt");
    header.set_size(4);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append(&header, Cursor::new(b"nope")).unwrap();
    builder.finish().unwrap();

    let sandbox = LocalSandbox::new(&root);
    let error = sandbox
        .restore_session(&SessionSnapshotFile {
            path: archive_path,
            metadata: SnapshotMetadata::default(),
            ..SessionSnapshotFile::default()
        })
        .await
        .expect_err("path traversal archive must be rejected");

    assert!(matches!(error, SandboxError::Message(_)));
}

#[tokio::test]
async fn local_sandbox_output_truncate_sets_overflow() {
    let root = temp_root("truncate");
    let sandbox = LocalSandbox::new(&root);
    let mut spec = shell_spec("printf abcdef");
    spec.output_policy.max_inline_bytes = 3;
    spec.output_policy.overflow = OutputOverflowPolicy::Truncate;

    let mut handle = sandbox
        .execute(spec, ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("execute should spawn process");
    let output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(output, "abc");
    assert_eq!(outcome.overflow.unwrap().effective_limit, 3);
}

#[tokio::test]
async fn local_sandbox_output_spill_records_blob_and_events() {
    let root = temp_root("spill");
    let (sink, mut rx) = recording_sink();
    let sandbox = LocalSandbox::new(&root);
    let mut ctx = ExecContext::for_test(sink);
    ctx.workspace_root = root.clone();
    let mut spec = shell_spec("printf abcdef");
    spec.output_policy.max_inline_bytes = 3;
    spec.output_policy.overflow = OutputOverflowPolicy::SpillToBlob {
        head_bytes: 2,
        tail_bytes: 1,
    };

    let mut handle = sandbox
        .execute(spec, ctx)
        .await
        .expect("execute should spawn process");
    let output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(output, "abf");
    let blob_ref = outcome
        .overflow
        .expect("overflow should be recorded")
        .blob_ref
        .expect("spill should store a blob");
    assert_eq!(blob_ref.size, 3);
    let events = drain_events(&mut rx);
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::SandboxOutputSpilled(spilled)
                if spilled.head_bytes == 2 && spilled.tail_bytes == 1
        )
    }));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::SandboxBackpressureApplied(_))));
}

#[tokio::test]
async fn local_sandbox_redacts_streamed_stdout_when_policy_enabled() {
    let root = temp_root("redact-stream");
    let sandbox = LocalSandbox::new(&root);
    let mut ctx = ExecContext::for_test(Arc::new(NullSink));
    ctx.redactor = Arc::new(ReplacementRedactor);
    let spec = shell_spec("printf 'prefix-secret'");

    let mut handle = sandbox
        .execute(spec, ctx)
        .await
        .expect("execute should spawn process");
    let output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(output, "prefix-[MASK]");
    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));
}

#[tokio::test]
async fn local_sandbox_redacts_streamed_stderr_when_policy_enabled() {
    let root = temp_root("redact-stderr");
    let sandbox = LocalSandbox::new(&root);
    let mut ctx = ExecContext::for_test(Arc::new(NullSink));
    ctx.redactor = Arc::new(ReplacementRedactor);
    let spec = shell_spec("printf 'prefix-secret' >&2");

    let mut handle = sandbox
        .execute(spec, ctx)
        .await
        .expect("execute should spawn process");
    let output = collect_stdout(handle.stderr.take().expect("stderr should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(output, "prefix-[MASK]");
    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));
}

#[tokio::test]
async fn local_sandbox_spills_redacted_output_to_blob_store() {
    let root = temp_root("spill-blob-store");
    let (sink, mut rx) = recording_sink();
    let blob_store = Arc::new(RecordingBlobStore::default());
    let sandbox = LocalSandbox::new(&root);
    let mut ctx = ExecContext::for_test(sink);
    ctx.workspace_root = root.clone();
    ctx.redactor = Arc::new(ReplacementRedactor);
    ctx.blob_store = Some(blob_store.clone());
    let mut spec = shell_spec("printf 'prefix-secret'");
    spec.output_policy.max_inline_bytes = 6;
    spec.output_policy.overflow = OutputOverflowPolicy::SpillToBlob {
        head_bytes: 4096,
        tail_bytes: 4096,
    };

    let mut handle = sandbox
        .execute(spec, ctx)
        .await
        .expect("execute should spawn process");
    let output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(output, "prefix");
    let blob_ref = outcome
        .overflow
        .expect("overflow should be recorded")
        .blob_ref
        .expect("spill should store a blob");
    assert_eq!(blob_ref.size, 7);
    assert_eq!(blob_store.puts().len(), 1);
    assert_eq!(blob_store.puts()[0].0, Bytes::from_static(b"-[MASK]"));
    assert!(!root.join(".jyowo").join("sandbox-output").exists());
    let events = drain_events(&mut rx);
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::SandboxOutputSpilled(spilled) if spilled.blob_ref == blob_ref
        )
    }));
}

#[tokio::test]
async fn local_sandbox_output_abort_exec_returns_budget_error() {
    let root = temp_root("abort-output");
    let sandbox = LocalSandbox::new(&root);
    let mut spec = shell_spec("printf abcdef");
    spec.output_policy.max_inline_bytes = 3;
    spec.output_policy.overflow = OutputOverflowPolicy::AbortExec;

    let handle = sandbox
        .execute(spec, ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("execute should spawn process");
    let error = handle
        .activity
        .wait()
        .await
        .expect_err("overflow should abort exec");

    assert!(matches!(
        error,
        SandboxError::OutputBudgetExceeded { limit: 3 }
    ));
}

#[tokio::test]
async fn local_sandbox_emits_backpressure_when_consumer_pauses() {
    let root = temp_root("backpressure");
    let (sink, mut rx) = recording_sink();
    let sandbox = LocalSandbox::new(&root);
    let mut spec = shell_spec("dd if=/dev/zero bs=8192 count=8 2>/dev/null | tr '\\0' x");
    spec.output_policy.max_inline_bytes = 100_000;
    spec.output_policy.overflow = OutputOverflowPolicy::Truncate;

    let mut handle = sandbox
        .execute(spec, ExecContext::for_test(sink))
        .await
        .expect("execute should spawn process");
    let stdout = handle.stdout.take().expect("stdout should be piped");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let output = collect_stdout(stdout).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(output.len(), 65_536);
    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));
    assert!(drain_events(&mut rx).iter().any(|event| matches!(
        event,
        Event::SandboxBackpressureApplied(backpressure) if backpressure.paused_for_ms > 0
    )));
}

#[tokio::test]
async fn local_sandbox_filters_environment_with_passthrough_and_per_exec_keys() {
    let root = temp_root("env");
    let sandbox = LocalSandbox::with_base(
        &root,
        SandboxBaseConfig {
            passthrough_env_keys: BTreeSet::from(["VISIBLE".to_owned()]),
            ..SandboxBaseConfig::default()
        },
    );

    let mut spec = shell_spec(
        "printf '%s:%s:%s' \"${VISIBLE:-missing}\" \"${EXPLICIT:-missing}\" \"${HIDDEN:-missing}\"",
    );
    spec.env.insert("VISIBLE".to_owned(), "yes".to_owned());
    spec.env
        .insert("EXPLICIT".to_owned(), "declared".to_owned());
    spec.env.insert("HIDDEN".to_owned(), "no".to_owned());
    spec.authorized_env_keys.insert("EXPLICIT".to_owned());

    let mut handle = sandbox
        .execute(spec, ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("execute should spawn process");
    let output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(output, "yes:declared:missing");
    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));
}

#[tokio::test]
async fn local_sandbox_timeout_and_activity_timeout_kill_processes() {
    let root = temp_root("timeouts");
    let (sink, mut rx) = recording_sink();
    let sandbox = LocalSandbox::new(&root);

    let mut timed = shell_spec("sleep 5");
    timed.timeout = Some(Duration::from_millis(50));
    let handle = sandbox
        .execute(timed, ExecContext::for_test(sink.clone()))
        .await
        .expect("execute should spawn timed process");
    let outcome = handle.activity.wait().await.expect("wait should succeed");
    assert_eq!(outcome.exit_status, SandboxExitStatus::Timeout);

    let mut inactive = shell_spec("sleep 5");
    inactive.activity_timeout = Some(Duration::from_millis(50));
    let handle = sandbox
        .execute(inactive, ExecContext::for_test(sink))
        .await
        .expect("execute should spawn inactive process");
    let outcome = handle.activity.wait().await.expect("wait should succeed");
    assert_eq!(outcome.exit_status, SandboxExitStatus::InactivityTimeout);

    assert!(drain_events(&mut rx).iter().any(|event| {
        matches!(
            event,
            Event::SandboxActivityTimeoutFired(timeout)
                if timeout.kill_scope == KillScope::ProcessGroup
        )
    }));
}

#[test]
fn local_sandbox_exposes_os_isolation_modes_and_capability_shape() {
    let root = temp_root("isolation-modes");
    let sandbox = LocalSandbox::new(&root).with_isolation(LocalIsolation::Bubblewrap);

    assert_eq!(sandbox.isolation(), LocalIsolation::Bubblewrap);
    assert!(sandbox
        .capabilities()
        .snapshot_kinds
        .contains(&SessionSnapshotKind::ShellState));
    assert!(!sandbox.capabilities().resource_limit_support.memory);
    assert!(!sandbox.capabilities().resource_limit_support.cpu);
    assert!(!sandbox.capabilities().resource_limit_support.pids);
    assert!(!sandbox.capabilities().resource_limit_support.open_files);
    assert!(matches!(
        LocalIsolation::for_current_platform(),
        LocalIsolation::Bubblewrap | LocalIsolation::Seatbelt | LocalIsolation::JobObject
    ));
}

#[tokio::test]
async fn local_sandbox_supports_process_and_process_group_kill_scope() {
    let root = temp_root("kill-scope");
    let sandbox = LocalSandbox::new(&root);
    let mut spec = shell_spec("sleep 5");
    spec.timeout = Some(Duration::from_secs(5));
    let handle = sandbox
        .execute(spec, ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("execute should spawn process");

    handle
        .activity
        .kill(15, KillScope::ProcessGroup)
        .await
        .expect("process group kill should be supported");
    let outcome = handle.activity.wait().await.expect("wait should succeed");
    assert_eq!(outcome.exit_status, SandboxExitStatus::Signal(15));

    let mut spec = shell_spec("sleep 5");
    spec.timeout = Some(Duration::from_secs(5));
    let handle = sandbox
        .execute(spec, ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("execute should spawn process");
    handle
        .activity
        .kill(15, KillScope::Process)
        .await
        .expect("process kill should be supported");
    let outcome = handle.activity.wait().await.expect("wait should succeed");
    assert_eq!(outcome.exit_status, SandboxExitStatus::Signal(15));
}

#[tokio::test]
async fn local_wait_reaps_background_processes_after_the_root_exits() {
    let root = temp_root("background-process-group");
    let sandbox = LocalSandbox::new(&root);
    let mut spec =
        shell_spec("sleep 30 </dev/null >/dev/null 2>&1 & printf '%s' \"$!\" > background.pid");
    spec.timeout = Some(Duration::from_secs(2));
    spec.required_kill_scope = Some(KillScope::ProcessGroup);
    let handle = execute_with_lifecycle(
        Arc::new(sandbox),
        spec,
        ExecContext::for_test(Arc::new(NullSink)),
    )
    .await
    .expect("execute should spawn a background process");

    let outcome = handle
        .activity
        .wait()
        .await
        .expect("root wait should succeed");
    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));

    let background_pid = std::fs::read_to_string(root.join("background.pid"))
        .expect("script should record the background pid")
        .parse::<u32>()
        .expect("background pid should be numeric");
    let reaped = wait_for_process_exit(background_pid, Duration::from_millis(500)).await;
    if !reaped {
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(background_pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    assert!(reaped, "background process survived the root activity wait");
}

#[tokio::test]
async fn local_execute_reaps_process_group_when_started_event_is_rejected() {
    let root = temp_root("started-event-rejected");
    let pid_file = root.join("processes.pid");
    let sandbox = LocalSandbox::new(&root);
    let mut spec = shell_spec(
        "trap '' TERM; sleep 30 </dev/null >/dev/null 2>&1 & background=$!; printf '%s %s' \"$$\" \"$background\" > processes.pid; wait",
    );
    spec.timeout = Some(Duration::from_secs(5));
    spec.required_kill_scope = Some(KillScope::ProcessGroup);

    let error = match sandbox
        .execute(
            spec,
            ExecContext::for_test(Arc::new(RejectStartedSink {
                pid_file: pid_file.clone(),
            })),
        )
        .await
    {
        Ok(_) => panic!("started event rejection must fail execute"),
        Err(error) => error,
    };

    assert!(
        matches!(error, SandboxError::Message(ref message) if message.contains("started event"))
    );
    let pids = std::fs::read_to_string(&pid_file)
        .expect("sink must wait until the script records its processes")
        .split_whitespace()
        .map(|pid| pid.parse::<u32>().expect("recorded pid must be numeric"))
        .collect::<Vec<_>>();
    assert_eq!(pids.len(), 2);

    for pid in pids {
        let reaped = wait_for_process_exit(pid, Duration::from_millis(500)).await;
        if !reaped {
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        assert!(reaped, "process {pid} survived failed execute handoff");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aborting_failed_execute_handoff_still_kills_the_process_group() {
    let root = temp_root("started-event-cleanup-aborted");
    let pid_file = root.join("processes.pid");
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let mut spec = shell_spec(
        "trap '' TERM; sleep 30 </dev/null >/dev/null 2>&1 & background=$!; printf '%s %s' \"$$\" \"$background\" > processes.pid; wait",
    );
    spec.timeout = Some(Duration::from_secs(5));
    spec.required_kill_scope = Some(KillScope::ProcessGroup);
    let task = tokio::spawn({
        let pid_file = pid_file.clone();
        let entered = Arc::clone(&entered);
        let release = Arc::clone(&release);
        async move {
            LocalSandbox::new(&root)
                .execute(
                    spec,
                    ExecContext::for_test(Arc::new(BlockingRejectStartedSink {
                        pid_file,
                        entered,
                        release,
                    })),
                )
                .await
        }
    });

    entered.wait();
    task.abort();
    release.wait();
    let error = match task.await {
        Ok(_) => panic!("failed handoff task must be cancelled"),
        Err(error) => error,
    };
    assert!(error.is_cancelled());

    let pids = std::fs::read_to_string(&pid_file)
        .expect("sink must wait until the script records its processes")
        .split_whitespace()
        .map(|pid| pid.parse::<u32>().expect("recorded pid must be numeric"))
        .collect::<Vec<_>>();
    assert_eq!(pids.len(), 2);
    for pid in pids {
        let terminated = wait_for_process_exit(pid, Duration::from_millis(500)).await;
        if !terminated {
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        assert!(
            terminated,
            "process {pid} survived cancellation during failed handoff cleanup"
        );
    }
}

#[tokio::test]
async fn process_group_kill_can_escalate_after_descendants_ignore_term() {
    let root = temp_root("process-group-signal-escalation");
    let sandbox = LocalSandbox::new(&root);
    let mut spec = shell_spec("trap '' TERM; sleep 30 & printf '%s' \"$!\" > background.pid; wait");
    spec.timeout = Some(Duration::from_secs(5));
    spec.required_kill_scope = Some(KillScope::ProcessGroup);
    let handle = execute_with_lifecycle(
        Arc::new(sandbox),
        spec,
        ExecContext::for_test(Arc::new(NullSink)),
    )
    .await
    .expect("execute should spawn a TERM-resistant process group");
    let root_pid = handle.pid.expect("root process should have a pid");
    let process_group_id = process_group_id(root_pid);
    let activity = Arc::clone(&handle.activity);

    activity
        .kill(15, KillScope::ProcessGroup)
        .await
        .expect("TERM should be delivered to the process group");
    tokio::time::sleep(Duration::from_millis(25)).await;
    activity
        .kill(9, KillScope::ProcessGroup)
        .await
        .expect("KILL should escalate after TERM");
    let waited = tokio::time::timeout(Duration::from_millis(500), activity.wait()).await;
    if waited.is_err() {
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(format!("-{process_group_id}"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    let outcome = waited
        .expect("SIGKILL must terminate a TERM-resistant process group")
        .expect("wait should succeed after escalation");

    assert_eq!(outcome.exit_status, SandboxExitStatus::Signal(9));
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn authorized_path_cannot_replace_the_seatbelt_launcher() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_root("seatbelt-launcher-path");
    let fake_bin = root.join("fake-bin");
    std::fs::create_dir_all(&fake_bin).expect("fake bin must exist");
    let marker = root.join("launcher-hijacked");
    let fake_launcher = fake_bin.join("sandbox-exec");
    std::fs::write(
        &fake_launcher,
        format!("#!/bin/sh\nprintf hijacked > '{}'\n", marker.display()),
    )
    .expect("fake launcher must be written");
    std::fs::set_permissions(&fake_launcher, std::fs::Permissions::from_mode(0o700))
        .expect("fake launcher must be executable");

    let sandbox = LocalSandbox::new(&root).with_isolation(LocalIsolation::Seatbelt);
    let mut spec = shell_spec("printf ok");
    spec.env
        .insert("PATH".to_owned(), fake_bin.display().to_string());
    spec.authorized_env_keys.insert("PATH".to_owned());

    if let Ok(handle) = sandbox
        .execute(spec, ExecContext::for_test(Arc::new(NullSink)))
        .await
    {
        let _ = tokio::time::timeout(Duration::from_secs(1), handle.activity.wait()).await;
    }

    assert!(
        !marker.exists(),
        "authorized child PATH replaced the host seatbelt launcher"
    );
}

fn process_group_id(pid: u32) -> u32 {
    let output = std::process::Command::new("ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .expect("process group query should run");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("process group should be utf8")
        .trim()
        .parse()
        .expect("process group should be numeric")
}

async fn wait_for_process_exit(pid: u32, timeout: Duration) -> bool {
    let started = std::time::Instant::now();
    loop {
        let alive = std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !alive {
            return true;
        }
        if started.elapsed() >= timeout {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#![cfg(all(feature = "ssh", unix))]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use harness_contracts::{Event, ResourceLimits, SandboxError, SandboxExitStatus};
use harness_sandbox::{
    EventSink, ExecContext, ExecSpec, OutputOverflowPolicy, SandboxBackend, SandboxBaseConfig,
    SnapshotSpec, SshAuth, SshSandbox, StdioSpec, WorkspaceSyncConfig, WorkspaceSyncStrategy,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

struct RecordingSink {
    tx: UnboundedSender<Event>,
}

struct NullSink;

impl EventSink for RecordingSink {
    fn emit(&self, event: Event) -> Result<(), SandboxError> {
        self.tx
            .send(event)
            .map_err(|error| SandboxError::Message(error.to_string()))
    }
}

impl EventSink for NullSink {
    fn emit(&self, _event: Event) -> Result<(), SandboxError> {
        Ok(())
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
        "jyowo-harness-ssh-{name}-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("temp root should be created");
    root
}

fn fake_ssh(root: &Path, log: &Path) -> PathBuf {
    let bin = root.join("ssh");
    let script = format!(
        r#"#!/bin/sh
printf 'ssh %s\n' "$*" >> "{}"
case "$*" in
  *"tar -C /workspace -cf -"*) tmp="$(mktemp -d)"; printf 'snapshot-data' > "$tmp/state.txt"; tar -C "$tmp" -cf - .; rm -rf "$tmp" ;;
  *"tar -C /workspace -xf -"*) cat >/dev/null ;;
  *"__jyowo_probe__"*) exit 0 ;;
  *) printf 'abcdef' ;;
esac
"#,
        log.display()
    );
    std::fs::write(&bin, script).expect("fake ssh should be written");
    let mut permissions = std::fs::metadata(&bin)
        .expect("fake ssh metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&bin, permissions).expect("fake ssh should be executable");
    bin
}

fn fake_slow_ssh(root: &Path, log: &Path) -> PathBuf {
    let bin = root.join("ssh-slow");
    let script = format!(
        r#"#!/bin/sh
printf 'ssh %s\n' "$*" >> "{}"
case "$*" in
  *"__jyowo_probe__"*) exit 0 ;;
  *) sleep 1; printf 'late' ;;
esac
"#,
        log.display()
    );
    std::fs::write(&bin, script).expect("fake ssh should be written");
    let mut permissions = std::fs::metadata(&bin)
        .expect("fake ssh metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&bin, permissions).expect("fake ssh should be executable");
    bin
}

fn fake_rsync(root: &Path, log: &Path, fail_direction: Option<&str>) -> PathBuf {
    let bin = root.join(format!(
        "rsync-{}",
        fail_direction.unwrap_or("ok").replace('/', "-")
    ));
    let fail_direction = fail_direction.unwrap_or("");
    let script = format!(
        r#"#!/bin/sh
prev=''
last=''
for arg in "$@"; do
  prev="$last"
  last="$arg"
done
direction='push'
case "$prev" in
  *'@example.internal:'*) direction='pull' ;;
esac
printf 'rsync:%s %s\n' "$direction" "$*" >> "{}"
if [ "{}" = "$direction" ]; then
  printf 'rsync %s failed\n' "$direction" >&2
  exit 23
fi
exit 0
"#,
        log.display(),
        fail_direction
    );
    std::fs::write(&bin, script).expect("fake rsync should be written");
    let mut permissions = std::fs::metadata(&bin)
        .expect("fake rsync metadata should exist")
        .permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    std::fs::set_permissions(&bin, permissions).expect("fake rsync should be executable");
    bin
}

fn shell_spec() -> ExecSpec {
    ExecSpec {
        command: "/bin/sh".to_owned(),
        args: vec!["-c".to_owned(), "printf ignored".to_owned()],
        stdin: StdioSpec::Null,
        stdout: StdioSpec::Piped,
        stderr: StdioSpec::Piped,
        ..ExecSpec::default()
    }
}

async fn collect_stdout(mut stdout: futures::stream::BoxStream<'static, bytes::Bytes>) -> String {
    let mut bytes = Vec::new();
    while let Some(chunk) = stdout.next().await {
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).expect("stdout should be utf8")
}

fn sandbox(ssh_binary: PathBuf) -> SshSandbox {
    SshSandbox::builder()
        .ssh_binary(ssh_binary)
        .host("example.internal")
        .port(2222)
        .user("jyowo")
        .auth(SshAuth::Agent)
        .remote_workspace("/workspace")
        .workspace_sync(WorkspaceSyncStrategy::None)
        .build()
        .expect("ssh sandbox should build")
}

fn sync_sandbox(
    ssh_binary: PathBuf,
    rsync_binary: PathBuf,
    local_workspace: PathBuf,
    strategy: WorkspaceSyncStrategy,
) -> SshSandbox {
    std::fs::create_dir_all(&local_workspace).expect("local workspace should be created");
    SshSandbox::builder()
        .ssh_binary(ssh_binary)
        .host("example.internal")
        .port(2222)
        .user("jyowo")
        .auth(SshAuth::Agent)
        .remote_workspace("/workspace")
        .workspace_sync(strategy)
        .workspace_sync_config(
            WorkspaceSyncConfig::rsync(local_workspace, "/remote/workspace")
                .rsync_binary(rsync_binary)
                .exclude(".git"),
        )
        .build()
        .expect("ssh sandbox should build")
}

fn parse_live_ssh_target(target: &str) -> (String, String, u16) {
    let (user, host_port) = target
        .split_once('@')
        .map(|(user, host_port)| (user.to_owned(), host_port))
        .unwrap_or_else(|| {
            (
                std::env::var("USER").unwrap_or_else(|_| "jyowo".to_owned()),
                target,
            )
        });
    let (host, port) = host_port
        .rsplit_once(':')
        .and_then(|(host, port)| port.parse::<u16>().ok().map(|port| (host, port)))
        .map(|(host, port)| (host.to_owned(), port))
        .unwrap_or_else(|| (host_port.to_owned(), 22));
    (user, host, port)
}

#[tokio::test]
async fn ssh_workspace_sync_fails_closed_without_explicit_config() {
    let root = temp_root("sync-missing");
    let sandbox = SshSandbox::builder()
        .ssh_binary(root.join("missing-ssh"))
        .host("example.internal")
        .user("jyowo")
        .workspace_sync(WorkspaceSyncStrategy::RsyncPush)
        .build()
        .expect("ssh sandbox should build");

    let error = sandbox
        .before_execute(&shell_spec(), &ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect_err("sync without config should fail closed before live ssh");
    assert!(error
        .to_string()
        .contains("workspace sync config is required"));
}

#[tokio::test]
async fn ssh_sandbox_rejects_non_wall_clock_resource_limits() {
    let root = temp_root("resource-policy");
    let log = root.join("ssh.log");
    let sandbox = sandbox(fake_ssh(&root, &log));

    for limit in ["memory", "cpu", "pids", "open_files"] {
        let mut spec = shell_spec();
        match limit {
            "memory" => spec.policy.resource_limits.max_memory_bytes = Some(67_108_864),
            "cpu" => spec.policy.resource_limits.max_cpu_cores = Some(1.5),
            "pids" => spec.policy.resource_limits.max_pids = Some(32),
            "open_files" => spec.policy.resource_limits.max_open_files = Some(64),
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
async fn ssh_sandbox_rejects_default_non_wall_clock_resource_limits() {
    let root = temp_root("default-resource-policy");
    let log = root.join("ssh.log");
    let mut base = SandboxBaseConfig::default();
    base.default_resource_limits = ResourceLimits {
        max_memory_bytes: None,
        max_cpu_cores: None,
        max_pids: None,
        max_wall_clock_ms: None,
        max_open_files: Some(64),
    };
    let sandbox = SshSandbox::builder()
        .ssh_binary(fake_ssh(&root, &log))
        .host("example.internal")
        .user("jyowo")
        .auth(SshAuth::Agent)
        .remote_workspace("/workspace")
        .base_config(base)
        .build()
        .expect("ssh sandbox should build");

    let error = match sandbox
        .execute(shell_spec(), ExecContext::for_test(Arc::new(NullSink)))
        .await
    {
        Ok(_) => panic!("default open-file limit must fail closed for ssh"),
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

#[tokio::test]
async fn ssh_sandbox_applies_wall_clock_resource_limit_as_timeout() {
    let root = temp_root("wall-clock");
    let log = root.join("ssh.log");
    let sandbox = sandbox(fake_slow_ssh(&root, &log));
    let mut spec = shell_spec();
    spec.policy.resource_limits.max_wall_clock_ms = Some(10);

    let handle = sandbox
        .execute(spec, ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("ssh should spawn");
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(outcome.exit_status, SandboxExitStatus::Timeout);
}

#[tokio::test]
async fn ssh_sandbox_caps_explicit_timeout_to_wall_clock_resource_limit() {
    let root = temp_root("wall-clock-caps-timeout");
    let log = root.join("ssh.log");
    let sandbox = sandbox(fake_slow_ssh(&root, &log));
    let mut spec = shell_spec();
    spec.timeout = Some(Duration::from_secs(2));
    spec.policy.resource_limits.max_wall_clock_ms = Some(10);

    let handle = sandbox
        .execute(spec, ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("ssh should spawn");
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(outcome.exit_status, SandboxExitStatus::Timeout);
}

#[test]
fn ssh_workspace_sync_generates_rsync_push_and_pull_plans_without_live_host() {
    let root = temp_root("sync-plan");
    let sandbox = SshSandbox::builder()
        .ssh_binary("/usr/bin/ssh")
        .host("example.internal")
        .port(2222)
        .user("jyowo")
        .auth(SshAuth::KeyFile(root.join("id_ed25519")))
        .workspace_sync(WorkspaceSyncStrategy::RsyncBidi)
        .workspace_sync_config(
            WorkspaceSyncConfig::rsync(root.join("workspace"), "/remote/workspace")
                .rsync_binary("/usr/bin/rsync")
                .exclude(".git"),
        )
        .build()
        .expect("ssh sandbox should build");

    let push = sandbox
        .before_execute_sync_plan()
        .expect("push plan should be generated")
        .expect("rsync bidi should push before execute");
    assert_eq!(push.program, PathBuf::from("/usr/bin/rsync"));
    assert!(push.args.contains(&"-az".to_owned()));
    assert!(push.args.contains(&"--delete".to_owned()));
    assert!(push.args.contains(&"--exclude".to_owned()));
    assert!(push.args.contains(&".git".to_owned()));
    assert!(push
        .args
        .iter()
        .any(|arg| arg.contains("-p 2222") && arg.contains("-i")));
    assert_eq!(
        push.args.last().expect("remote target should exist"),
        "jyowo@example.internal:/remote/workspace/"
    );

    let pull = sandbox
        .after_execute_sync_plan()
        .expect("pull plan should be generated")
        .expect("rsync bidi should pull after execute");
    assert_eq!(
        pull.args
            .get(pull.args.len() - 2)
            .expect("pull source should exist"),
        "jyowo@example.internal:/remote/workspace/"
    );
    assert!(pull
        .args
        .last()
        .expect("local target should exist")
        .ends_with("/workspace/"));
}

#[tokio::test]
async fn ssh_workspace_sync_executes_rsync_push_before_remote_command() {
    let root = temp_root("sync-push-executes");
    let log = root.join("sync.log");
    let ssh = fake_ssh(&root, &log);
    let rsync = fake_rsync(&root, &log, None);
    let sandbox = sync_sandbox(
        ssh,
        rsync,
        root.join("workspace"),
        WorkspaceSyncStrategy::RsyncPush,
    );

    let mut handle = sandbox
        .execute(shell_spec(), ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("ssh command should spawn after rsync push");
    let _ = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));
    let lines = std::fs::read_to_string(log)
        .expect("sync log should be written")
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let push_index = lines
        .iter()
        .position(|line| line.starts_with("rsync:push "))
        .expect("push rsync should be executed");
    let command_index = lines
        .iter()
        .position(|line| line.contains("/bin/sh -c printf ignored"))
        .expect("remote command should be executed");
    assert!(push_index < command_index);
}

#[tokio::test]
async fn ssh_workspace_sync_executes_push_and_pull_for_bidi() {
    let root = temp_root("sync-bidi-executes");
    let log = root.join("sync.log");
    let ssh = fake_ssh(&root, &log);
    let rsync = fake_rsync(&root, &log, None);
    let sandbox = sync_sandbox(
        ssh,
        rsync,
        root.join("workspace"),
        WorkspaceSyncStrategy::RsyncBidi,
    );
    let ctx = ExecContext::for_test(Arc::new(NullSink));

    let mut handle = sandbox
        .execute(shell_spec(), ctx.clone())
        .await
        .expect("ssh command should spawn after rsync push");
    let _ = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");
    sandbox
        .after_execute(&outcome, &ctx)
        .await
        .expect("bidi sync should pull after execute");

    let lines = std::fs::read_to_string(log)
        .expect("sync log should be written")
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let push_index = lines
        .iter()
        .position(|line| line.starts_with("rsync:push "))
        .expect("push rsync should be executed");
    let command_index = lines
        .iter()
        .position(|line| line.contains("/bin/sh -c printf ignored"))
        .expect("remote command should be executed");
    let pull_index = lines
        .iter()
        .position(|line| line.starts_with("rsync:pull "))
        .expect("pull rsync should be executed");
    assert!(push_index < command_index);
    assert!(command_index < pull_index);
}

#[tokio::test]
async fn ssh_workspace_sync_push_failure_fails_before_remote_execute() {
    let root = temp_root("sync-push-fails");
    let log = root.join("sync.log");
    let ssh = fake_ssh(&root, &log);
    let rsync = fake_rsync(&root, &log, Some("push"));
    let sandbox = sync_sandbox(
        ssh,
        rsync,
        root.join("workspace"),
        WorkspaceSyncStrategy::RsyncPush,
    );

    let error = match sandbox
        .execute(shell_spec(), ExecContext::for_test(Arc::new(NullSink)))
        .await
    {
        Ok(_) => panic!("rsync push failure should stop remote execution"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        SandboxError::WorkspaceSyncFailed { ref direction, .. } if direction == "push"
    ));
    let log_text = std::fs::read_to_string(log).expect("sync log should be written");
    assert!(log_text.contains("__jyowo_probe__"));
    assert!(log_text.contains("rsync:push "));
    assert!(!log_text.contains("/bin/sh -c printf ignored"));
}

#[tokio::test]
async fn ssh_workspace_sync_pull_failure_reports_after_execute_error() {
    let root = temp_root("sync-pull-fails");
    let log = root.join("sync.log");
    let ssh = fake_ssh(&root, &log);
    let rsync = fake_rsync(&root, &log, Some("pull"));
    let sandbox = sync_sandbox(
        ssh,
        rsync,
        root.join("workspace"),
        WorkspaceSyncStrategy::RsyncBidi,
    );
    let ctx = ExecContext::for_test(Arc::new(NullSink));

    let mut handle = sandbox
        .execute(shell_spec(), ctx.clone())
        .await
        .expect("ssh command should still complete before pull failure");
    let _ = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");
    let error = sandbox
        .after_execute(&outcome, &ctx)
        .await
        .expect_err("rsync pull failure should be reported by after_execute");

    assert!(matches!(
        error,
        SandboxError::WorkspaceSyncFailed { ref direction, .. } if direction == "pull"
    ));
    let log_text = std::fs::read_to_string(log).expect("sync log should be written");
    assert!(log_text.contains("rsync:push "));
    assert!(log_text.contains("/bin/sh -c printf ignored"));
    assert!(log_text.contains("rsync:pull "));
}

#[tokio::test]
async fn ssh_sandbox_detects_unavailable_host() {
    let root = temp_root("unavailable");
    let sandbox = sandbox(root.join("missing-ssh"));

    let error = match sandbox
        .execute(shell_spec(), ExecContext::for_test(Arc::new(NullSink)))
        .await
    {
        Ok(_) => panic!("missing ssh binary should fail"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        SandboxError::Unavailable { ref backend, .. } if backend == "ssh"
    ));
}

#[tokio::test]
async fn ssh_sandbox_executes_remote_command_and_applies_output_budget() {
    let root = temp_root("exec");
    let log = root.join("ssh.log");
    let ssh = fake_ssh(&root, &log);
    let (sink, mut rx) = recording_sink();
    let sandbox = sandbox(ssh);
    let mut spec = shell_spec();
    spec.output_policy.max_inline_bytes = 3;
    spec.output_policy.overflow = OutputOverflowPolicy::Truncate;

    let mut handle = sandbox
        .execute(spec, ExecContext::for_test(sink))
        .await
        .expect("ssh command should spawn");
    let output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(output, "abc");
    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));
    assert_eq!(outcome.overflow.unwrap().effective_limit, 3);

    let log_text = std::fs::read_to_string(log).expect("ssh log should be written");
    assert!(log_text.contains("-p 2222"));
    assert!(log_text.contains("jyowo@example.internal"));
    assert!(log_text.contains("/bin/sh -c printf ignored"));

    let events = drain_events(&mut rx);
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::SandboxExecutionStarted(started) if started.backend_id == "ssh")));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::SandboxExecutionCompleted(completed) if completed.backend_id == "ssh")));
}

#[tokio::test]
async fn ssh_sandbox_snapshots_and_restores_remote_workspace() {
    let root = temp_root("snapshot");
    let log = root.join("ssh.log");
    let ssh = fake_ssh(&root, &log);
    let snapshot_path = root.join("snapshot.tar");
    let sandbox = sandbox(ssh);

    let snapshot = sandbox
        .snapshot_session(&SnapshotSpec {
            target_path: Some(snapshot_path.clone()),
            ..SnapshotSpec::default()
        })
        .await
        .expect("snapshot should stream remote tar");
    sandbox
        .restore_session(&snapshot)
        .await
        .expect("restore should stream archive back");
    sandbox.shutdown().await.expect("shutdown should succeed");

    assert_eq!(snapshot.path, snapshot_path);
    assert!(snapshot.metadata.size_bytes > 0);
    let log_text = std::fs::read_to_string(log).expect("ssh log should be written");
    assert!(log_text.contains("tar -C /workspace -cf - ."));
    assert!(log_text.contains("tar -C /workspace -xf -"));
}

#[tokio::test]
async fn ssh_restore_rejects_path_traversal_archive_before_remote_extract() {
    let root = temp_root("restore-traversal");
    let log = root.join("ssh.log");
    let ssh = fake_ssh(&root, &log);
    let archive_path = root.join("malicious.tar");
    let file = std::fs::File::create(&archive_path).unwrap();
    let mut builder = tar::Builder::new(file);
    let mut header = tar::Header::new_gnu();
    header.as_mut_bytes()[..13].copy_from_slice(b"../escape.txt");
    header.set_size(4);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append(&header, std::io::Cursor::new(b"nope"))
        .unwrap();
    builder.finish().unwrap();
    let sandbox = sandbox(ssh);

    let error = sandbox
        .restore_session(&harness_sandbox::SessionSnapshotFile {
            path: archive_path,
            metadata: harness_sandbox::SnapshotMetadata::default(),
            ..harness_sandbox::SessionSnapshotFile::default()
        })
        .await
        .expect_err("path traversal archive must be rejected locally");

    assert!(matches!(error, SandboxError::Message(_)));
    let log_text = std::fs::read_to_string(log).unwrap_or_default();
    assert!(!log_text.contains("tar -C /workspace -xf -"));
}

#[tokio::test]
#[ignore = "requires a live SSH target and JYOWO_LIVE_SSH_TARGET"]
async fn live_ssh_executes_configured_target() {
    let Ok(target) = std::env::var("JYOWO_LIVE_SSH_TARGET") else {
        eprintln!("set JYOWO_LIVE_SSH_TARGET to run this live test");
        return;
    };
    let (user, host, port) = parse_live_ssh_target(&target);
    let sandbox = SshSandbox::builder()
        .host(host)
        .port(port)
        .user(user)
        .auth(SshAuth::Agent)
        .remote_workspace("/tmp/jyowo-harness-live")
        .workspace_sync(WorkspaceSyncStrategy::None)
        .build()
        .expect("ssh sandbox should build");

    let mut handle = sandbox
        .execute(shell_spec(), ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("live ssh command should spawn");
    let output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(output, "ignored");
    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));
}

#[tokio::test]
#[ignore = "requires a live SSH target and JYOWO_LIVE_SSH_TARGET"]
async fn live_ssh_snapshots_and_restores_configured_target() {
    let Ok(target) = std::env::var("JYOWO_LIVE_SSH_TARGET") else {
        eprintln!("set JYOWO_LIVE_SSH_TARGET to run this live test");
        return;
    };
    let (user, host, port) = parse_live_ssh_target(&target);
    let root = temp_root("live-snapshot");
    let sandbox = SshSandbox::builder()
        .host(host)
        .port(port)
        .user(user)
        .auth(SshAuth::Agent)
        .remote_workspace("/tmp/jyowo-harness-live")
        .workspace_sync(WorkspaceSyncStrategy::None)
        .build()
        .expect("ssh sandbox should build");
    let setup = ExecSpec {
        command: "/bin/sh".to_owned(),
        args: vec![
            "-c".to_owned(),
            "mkdir -p /tmp/jyowo-harness-live && printf state > /tmp/jyowo-harness-live/state.txt && printf ignored".to_owned(),
        ],
        stdin: StdioSpec::Null,
        stdout: StdioSpec::Piped,
        stderr: StdioSpec::Piped,
        ..ExecSpec::default()
    };

    let mut handle = sandbox
        .execute(setup, ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("live ssh setup command should spawn");
    let _output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    handle.activity.wait().await.expect("wait should succeed");

    let snapshot = sandbox
        .snapshot_session(&SnapshotSpec {
            target_path: Some(root.join("snapshot.tar")),
            ..SnapshotSpec::default()
        })
        .await
        .expect("live ssh snapshot should stream archive");
    sandbox
        .restore_session(&snapshot)
        .await
        .expect("live ssh restore should extract archive");

    assert!(snapshot.metadata.size_bytes > 0);
}

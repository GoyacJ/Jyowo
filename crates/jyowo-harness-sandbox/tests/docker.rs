#![cfg(all(feature = "docker", unix))]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use harness_contracts::{
    ContainerLifecycleReason, ContainerLifecycleState, Event, ResourceLimits, SandboxError,
    SandboxExitStatus, SessionSnapshotKind,
};
use harness_sandbox::{
    ContainerLifecycle, DockerSandbox, EventSink, ExecContext, ExecSpec, NetworkMode,
    OutputOverflowPolicy, SandboxBackend, SandboxBaseConfig, SnapshotSpec, StdioSpec, VolumeMount,
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
        "jyowo-harness-docker-{name}-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("temp root should be created");
    root
}

fn fake_docker(root: &Path, log: &Path) -> PathBuf {
    let bin = root.join("docker");
    let script = format!(
        r#"#!/bin/sh
printf '%s\n' "$*" >> "{}"
case "$1" in
  version) exit 0 ;;
  commit) exit 0 ;;
  save) printf 'fake-image-archive' > "$3"; exit 0 ;;
  load) exit 0 ;;
  rm) exit 0 ;;
  run)
    case "$*" in
      *" -d "*) printf 'fake-container\n' ;;
      *) printf 'abcdef' ;;
    esac
    exit 0
    ;;
  exec) printf 'abcdef'; exit 0 ;;
  *) exit 0 ;;
esac
"#,
        log.display()
    );
    std::fs::write(&bin, script).expect("fake docker should be written");
    let mut permissions = std::fs::metadata(&bin)
        .expect("fake docker metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&bin, permissions).expect("fake docker should be executable");
    bin
}

fn fake_slow_docker(root: &Path, log: &Path) -> PathBuf {
    let bin = root.join("docker-slow");
    let script = format!(
        r#"#!/bin/sh
printf '%s\n' "$*" >> "{}"
case "$1" in
  version) exit 0 ;;
  run|exec) sleep 1; printf 'late'; exit 0 ;;
  *) exit 0 ;;
esac
"#,
        log.display()
    );
    std::fs::write(&bin, script).expect("fake docker should be written");
    let mut permissions = std::fs::metadata(&bin)
        .expect("fake docker metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&bin, permissions).expect("fake docker should be executable");
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

#[tokio::test]
async fn docker_sandbox_detects_unavailable_host() {
    let root = temp_root("unavailable");
    let sandbox = DockerSandbox::builder()
        .docker_binary(root.join("missing-docker"))
        .image("jyowo-test:latest")
        .lifecycle(ContainerLifecycle::EphemeralPerExec)
        .build()
        .expect("docker sandbox should build");

    let error = match sandbox
        .execute(shell_spec(), ExecContext::for_test(Arc::new(NullSink)))
        .await
    {
        Ok(_) => panic!("missing docker binary should fail"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        SandboxError::Unavailable { ref backend, .. } if backend == "docker"
    ));
}

#[tokio::test]
async fn docker_sandbox_executes_ephemeral_container_and_applies_output_budget() {
    let root = temp_root("exec");
    let log = root.join("docker.log");
    let docker = fake_docker(&root, &log);
    let (sink, mut rx) = recording_sink();
    let sandbox = DockerSandbox::builder()
        .docker_binary(docker)
        .image("jyowo-test:latest")
        .mount(VolumeMount::workspace(&root, "/workspace"))
        .network(NetworkMode::None)
        .lifecycle(ContainerLifecycle::EphemeralPerExec)
        .build()
        .expect("docker sandbox should build");
    let mut spec = shell_spec();
    spec.policy.resource_limits.max_memory_bytes = Some(67_108_864);
    spec.policy.resource_limits.max_cpu_cores = Some(1.5);
    spec.policy.resource_limits.max_pids = Some(32);
    spec.policy.resource_limits.max_open_files = Some(64);
    spec.output_policy.max_inline_bytes = 3;
    spec.output_policy.overflow = OutputOverflowPolicy::SpillToBlob {
        head_bytes: 2,
        tail_bytes: 1,
    };
    let mut ctx = ExecContext::for_test(sink);
    ctx.workspace_root = root.clone();

    let mut handle = sandbox
        .execute(spec, ctx)
        .await
        .expect("docker run should spawn");
    let output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(output, "abf");
    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));
    let overflow = outcome.overflow.expect("overflow should be recorded");
    assert_eq!(overflow.effective_limit, 3);
    assert_eq!(
        overflow.blob_ref.expect("spill should store a blob").size,
        3
    );

    let log_text = std::fs::read_to_string(log).expect("docker log should be written");
    assert!(log_text.contains("run --rm"));
    assert!(log_text.contains("--network none"));
    assert!(log_text.contains("--memory 67108864"));
    assert!(log_text.contains("--cpus 1.5"));
    assert!(log_text.contains("--pids-limit 32"));
    assert!(log_text.contains("--ulimit nofile=64:64"));
    assert!(log_text.contains("-v "));
    assert!(log_text.contains("/workspace"));
    assert!(log_text.contains("jyowo-test:latest /bin/sh -c printf ignored"));

    let events = drain_events(&mut rx);
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::SandboxExecutionStarted(started) if started.backend_id == "docker")));
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::SandboxExecutionCompleted(completed) if completed.backend_id == "docker")));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::SandboxOutputSpilled(spilled)
                if spilled.head_bytes == 2 && spilled.tail_bytes == 1
        )
    }));
}

#[tokio::test]
async fn docker_sandbox_resource_limit_capabilities_follow_lifecycle() {
    let root = temp_root("resource-capabilities");
    let ephemeral = DockerSandbox::builder()
        .docker_binary(fake_docker(&root, &root.join("ephemeral.log")))
        .image("jyowo-test:latest")
        .lifecycle(ContainerLifecycle::EphemeralPerExec)
        .build()
        .expect("docker sandbox should build");
    let ephemeral_limits = ephemeral.capabilities().resource_limit_support;
    assert!(ephemeral_limits.memory);
    assert!(ephemeral_limits.cpu);
    assert!(ephemeral_limits.pids);
    assert!(ephemeral_limits.wall_clock);
    assert!(ephemeral_limits.open_files);

    let managed = DockerSandbox::builder()
        .docker_binary(fake_docker(&root, &root.join("managed.log")))
        .image("jyowo-test:latest")
        .lifecycle(ContainerLifecycle::CreatePerSession {
            keep_alive_after_exit: Duration::ZERO,
        })
        .build()
        .expect("docker sandbox should build");
    let managed_limits = managed.capabilities().resource_limit_support;
    assert!(!managed_limits.memory);
    assert!(!managed_limits.cpu);
    assert!(!managed_limits.pids);
    assert!(managed_limits.wall_clock);
    assert!(!managed_limits.open_files);
}

#[tokio::test]
async fn docker_sandbox_rejects_unenforceable_per_exec_resource_limits() {
    let root = temp_root("resource-policy");
    let log = root.join("docker.log");
    let docker = fake_docker(&root, &log);
    let managed = DockerSandbox::builder()
        .docker_binary(docker.clone())
        .image("jyowo-test:latest")
        .lifecycle(ContainerLifecycle::CreatePerSession {
            keep_alive_after_exit: Duration::ZERO,
        })
        .build()
        .expect("docker sandbox should build");
    let mut managed_spec = shell_spec();
    managed_spec.policy.resource_limits.max_memory_bytes = Some(67_108_864);
    let error = match managed
        .execute(managed_spec, ExecContext::for_test(Arc::new(NullSink)))
        .await
    {
        Ok(_) => panic!("managed docker per-exec memory limit must fail closed"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SandboxError::CapabilityMismatch {
            ref capability,
            ..
        } if capability == "resource_limits"
    ));

    let byo = DockerSandbox::builder()
        .docker_binary(docker)
        .image("jyowo-test:latest")
        .lifecycle(ContainerLifecycle::BringYourOwn {
            container_id: "existing".to_owned(),
        })
        .build()
        .expect("docker sandbox should build");
    let mut byo_spec = shell_spec();
    byo_spec.policy.resource_limits.max_open_files = Some(64);
    let error = match byo
        .execute(byo_spec, ExecContext::for_test(Arc::new(NullSink)))
        .await
    {
        Ok(_) => panic!("bring-your-own docker per-exec open-file limit must fail closed"),
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
async fn docker_sandbox_applies_wall_clock_resource_limit_as_timeout() {
    let root = temp_root("wall-clock");
    let log = root.join("docker.log");
    let docker = fake_slow_docker(&root, &log);
    let sandbox = DockerSandbox::builder()
        .docker_binary(docker)
        .image("jyowo-test:latest")
        .lifecycle(ContainerLifecycle::EphemeralPerExec)
        .build()
        .expect("docker sandbox should build");
    let mut spec = shell_spec();
    spec.policy.resource_limits.max_wall_clock_ms = Some(10);

    let handle = sandbox
        .execute(spec, ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("docker run should spawn");
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(outcome.exit_status, SandboxExitStatus::Timeout);
}

#[tokio::test]
async fn docker_sandbox_caps_explicit_timeout_to_wall_clock_resource_limit() {
    let root = temp_root("wall-clock-caps-timeout");
    let log = root.join("docker.log");
    let docker = fake_slow_docker(&root, &log);
    let sandbox = DockerSandbox::builder()
        .docker_binary(docker)
        .image("jyowo-test:latest")
        .lifecycle(ContainerLifecycle::EphemeralPerExec)
        .build()
        .expect("docker sandbox should build");
    let mut spec = shell_spec();
    spec.timeout = Some(Duration::from_secs(2));
    spec.policy.resource_limits.max_wall_clock_ms = Some(10);

    let handle = sandbox
        .execute(spec, ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("docker run should spawn");
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(outcome.exit_status, SandboxExitStatus::Timeout);
}

#[tokio::test]
async fn docker_sandbox_rejects_unsupported_base_resource_limits_for_existing_containers() {
    let root = temp_root("base-resource-policy");
    let log = root.join("docker.log");
    let mut base = SandboxBaseConfig::default();
    base.default_resource_limits = ResourceLimits {
        max_memory_bytes: None,
        max_cpu_cores: None,
        max_pids: None,
        max_wall_clock_ms: None,
        max_open_files: Some(64),
    };
    let sandbox = DockerSandbox::builder()
        .docker_binary(fake_docker(&root, &log))
        .image("jyowo-test:latest")
        .base_config(base)
        .lifecycle(ContainerLifecycle::BringYourOwn {
            container_id: "existing".to_owned(),
        })
        .build()
        .expect("docker sandbox should build");

    let error = match sandbox
        .execute(shell_spec(), ExecContext::for_test(Arc::new(NullSink)))
        .await
    {
        Ok(_) => panic!("bring-your-own docker base open-file limit must fail closed"),
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
async fn docker_sandbox_snapshots_restores_and_cleans_up_managed_container() {
    let root = temp_root("snapshot");
    let log = root.join("docker.log");
    let docker = fake_docker(&root, &log);
    let snapshot_path = root.join("snapshot.txt");
    let sandbox = DockerSandbox::builder()
        .docker_binary(docker)
        .image("jyowo-test:latest")
        .mount(VolumeMount::workspace(&root, "/workspace"))
        .lifecycle(ContainerLifecycle::CreatePerSession {
            keep_alive_after_exit: std::time::Duration::from_secs(30),
        })
        .build()
        .expect("docker sandbox should build");

    let snapshot = sandbox
        .snapshot_session(&SnapshotSpec {
            target_path: Some(snapshot_path.clone()),
            ..SnapshotSpec::default()
        })
        .await
        .expect("snapshot should commit container");
    sandbox
        .restore_session(&snapshot)
        .await
        .expect("restore should recreate container from snapshot");
    sandbox.shutdown().await.expect("shutdown should cleanup");

    assert_eq!(snapshot.path, snapshot_path);
    assert!(snapshot.metadata.size_bytes > 0);
    let log_text = std::fs::read_to_string(log).expect("docker log should be written");
    assert!(log_text.contains("commit jyowo-"));
    assert!(log_text.contains("rm -f jyowo-"));
    assert!(log_text.contains("run -d --name jyowo-"));
}

#[tokio::test]
async fn docker_sandbox_container_image_snapshot_uses_exported_image_artifact() {
    let root = temp_root("container-image-snapshot");
    let log = root.join("docker.log");
    let docker = fake_docker(&root, &log);
    let snapshot_path = root.join("snapshot-image.tar");
    let sandbox = DockerSandbox::builder()
        .docker_binary(docker)
        .image("jyowo-test:latest")
        .mount(VolumeMount::workspace(&root, "/workspace"))
        .lifecycle(ContainerLifecycle::CreatePerSession {
            keep_alive_after_exit: Duration::from_secs(30),
        })
        .build()
        .expect("docker sandbox should build");

    assert!(sandbox
        .capabilities()
        .snapshot_kinds
        .contains(&SessionSnapshotKind::ContainerImage));
    let snapshot = sandbox
        .snapshot_session(&SnapshotSpec {
            kind: SessionSnapshotKind::ContainerImage,
            target_path: Some(snapshot_path.clone()),
            ..SnapshotSpec::default()
        })
        .await
        .expect("container image snapshot should export artifact");
    sandbox
        .restore_session(&snapshot)
        .await
        .expect("container image restore should import artifact");

    assert_eq!(snapshot.kind, SessionSnapshotKind::ContainerImage);
    assert_eq!(
        std::fs::read_to_string(snapshot_path).unwrap(),
        "fake-image-archive"
    );
    let log_text = std::fs::read_to_string(log).expect("docker log should be written");
    assert!(log_text.contains("commit jyowo-"));
    assert!(log_text.contains("save -o "));
    assert!(log_text.contains("load -i "));
}

#[tokio::test]
async fn docker_sandbox_emits_lifecycle_event_for_managed_container_start() {
    let root = temp_root("lifecycle-event");
    let log = root.join("docker.log");
    let docker = fake_docker(&root, &log);
    let (sink, mut rx) = recording_sink();
    let sandbox = DockerSandbox::builder()
        .docker_binary(docker)
        .image("jyowo-test:latest")
        .mount(VolumeMount::workspace(&root, "/workspace"))
        .lifecycle(ContainerLifecycle::CreatePerSession {
            keep_alive_after_exit: std::time::Duration::from_secs(30),
        })
        .build()
        .expect("docker sandbox should build");

    let mut handle = sandbox
        .execute(shell_spec(), ExecContext::for_test(sink))
        .await
        .expect("docker exec should spawn");
    let _output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let _outcome = handle.activity.wait().await.expect("wait should succeed");

    let events = drain_events(&mut rx);
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::SandboxContainerLifecycleTransition(transition)
                if transition.backend_id == "docker"
                    && transition.from == ContainerLifecycleState::Provisioning
                    && transition.to == ContainerLifecycleState::Ready
                    && transition.reason == ContainerLifecycleReason::SessionAttached
                    && transition.container_ref.container_id.starts_with("jyowo-")
        )
    }));
}

#[tokio::test]
async fn docker_reuse_pooled_enforces_pool_size_until_activity_wait_releases_container() {
    let root = temp_root("reuse-pooled-size");
    let log = root.join("docker.log");
    let docker = fake_docker(&root, &log);
    let (sink, mut rx) = recording_sink();
    let sandbox = DockerSandbox::builder()
        .docker_binary(docker)
        .image("jyowo-test:latest")
        .mount(VolumeMount::workspace(&root, "/workspace"))
        .lifecycle(ContainerLifecycle::ReusePooled {
            pool_size: 1,
            idle_timeout: Duration::from_secs(60),
        })
        .build()
        .expect("docker sandbox should build");

    let mut first = sandbox
        .execute(shell_spec(), ExecContext::for_test(sink.clone()))
        .await
        .expect("first docker exec should spawn");
    let second_sandbox = sandbox.clone();
    let second_sink = sink.clone();
    let mut second = tokio::spawn(async move {
        second_sandbox
            .execute(shell_spec(), ExecContext::for_test(second_sink))
            .await
    });

    assert!(
        tokio::time::timeout(Duration::from_millis(25), &mut second)
            .await
            .is_err(),
        "second execute should wait for the single pooled container to be released"
    );

    let _output = collect_stdout(first.stdout.take().expect("stdout should be piped")).await;
    let _outcome = first
        .activity
        .wait()
        .await
        .expect("first wait should succeed");

    let mut second = second
        .await
        .expect("second task should not panic")
        .expect("second docker exec should spawn after release");
    let _output = collect_stdout(second.stdout.take().expect("stdout should be piped")).await;
    let _outcome = second
        .activity
        .wait()
        .await
        .expect("second wait should succeed");

    let log_text = std::fs::read_to_string(log).expect("docker log should be written");
    assert_eq!(log_text.matches("run -d --name").count(), 1);
    assert_eq!(log_text.matches("exec -i jyowo-").count(), 2);

    let events = drain_events(&mut rx);
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::SandboxContainerLifecycleTransition(transition)
                if transition.from == ContainerLifecycleState::Idle
                    && transition.to == ContainerLifecycleState::InUse
                    && transition.reason == ContainerLifecycleReason::PoolReused
        )
    }));
}

#[tokio::test]
async fn docker_reuse_pooled_evicts_idle_container_after_timeout() {
    let root = temp_root("reuse-pooled-idle");
    let log = root.join("docker.log");
    let docker = fake_docker(&root, &log);
    let (sink, mut rx) = recording_sink();
    let sandbox = DockerSandbox::builder()
        .docker_binary(docker)
        .image("jyowo-test:latest")
        .mount(VolumeMount::workspace(&root, "/workspace"))
        .lifecycle(ContainerLifecycle::ReusePooled {
            pool_size: 1,
            idle_timeout: Duration::from_millis(10),
        })
        .build()
        .expect("docker sandbox should build");

    let mut first = sandbox
        .execute(shell_spec(), ExecContext::for_test(sink.clone()))
        .await
        .expect("first docker exec should spawn");
    let _output = collect_stdout(first.stdout.take().expect("stdout should be piped")).await;
    let _outcome = first
        .activity
        .wait()
        .await
        .expect("first wait should succeed");

    tokio::time::sleep(Duration::from_millis(30)).await;

    let mut second = sandbox
        .execute(shell_spec(), ExecContext::for_test(sink))
        .await
        .expect("second docker exec should spawn");
    let _output = collect_stdout(second.stdout.take().expect("stdout should be piped")).await;
    let _outcome = second
        .activity
        .wait()
        .await
        .expect("second wait should succeed");

    let log_text = std::fs::read_to_string(log).expect("docker log should be written");
    assert_eq!(log_text.matches("run -d --name").count(), 2);
    assert!(log_text.contains("rm -f jyowo-"));

    let events = drain_events(&mut rx);
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::SandboxContainerLifecycleTransition(transition)
                if transition.from == ContainerLifecycleState::Idle
                    && transition.to == ContainerLifecycleState::Stopped
                    && transition.reason == ContainerLifecycleReason::PoolEvicted
        )
    }));
}

#[tokio::test]
async fn docker_create_per_session_keep_alive_after_exit_defers_container_removal() {
    let root = temp_root("keep-alive");
    let log = root.join("docker.log");
    let docker = fake_docker(&root, &log);
    let (sink, mut rx) = recording_sink();
    let sandbox = DockerSandbox::builder()
        .docker_binary(docker)
        .image("jyowo-test:latest")
        .mount(VolumeMount::workspace(&root, "/workspace"))
        .lifecycle(ContainerLifecycle::CreatePerSession {
            keep_alive_after_exit: Duration::from_millis(30),
        })
        .build()
        .expect("docker sandbox should build");

    let mut handle = sandbox
        .execute(shell_spec(), ExecContext::for_test(sink))
        .await
        .expect("docker exec should spawn");
    let _output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let _outcome = handle.activity.wait().await.expect("wait should succeed");

    sandbox.shutdown().await.expect("shutdown should detach");
    let immediate_log = std::fs::read_to_string(&log).expect("docker log should be written");
    assert!(!immediate_log.contains("rm -f jyowo-"));

    tokio::time::sleep(Duration::from_millis(60)).await;
    let delayed_log = std::fs::read_to_string(log).expect("docker log should be written");
    assert!(delayed_log.contains("rm -f jyowo-"));

    let events = drain_events(&mut rx);
    assert!(events.iter().any(|event| {
        matches!(
            event,
            Event::SandboxContainerLifecycleTransition(transition)
                if transition.from == ContainerLifecycleState::Idle
                    && transition.to == ContainerLifecycleState::Stopped
                    && transition.reason == ContainerLifecycleReason::PoolEvicted
        )
    }));
}

#[tokio::test]
async fn docker_create_per_session_zero_keep_alive_removes_container_on_shutdown() {
    let root = temp_root("keep-alive-zero");
    let log = root.join("docker.log");
    let docker = fake_docker(&root, &log);
    let sandbox = DockerSandbox::builder()
        .docker_binary(docker)
        .image("jyowo-test:latest")
        .mount(VolumeMount::workspace(&root, "/workspace"))
        .lifecycle(ContainerLifecycle::CreatePerSession {
            keep_alive_after_exit: Duration::ZERO,
        })
        .build()
        .expect("docker sandbox should build");

    let mut handle = sandbox
        .execute(shell_spec(), ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("docker exec should spawn");
    let _output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let _outcome = handle.activity.wait().await.expect("wait should succeed");
    sandbox.shutdown().await.expect("shutdown should cleanup");

    let log_text = std::fs::read_to_string(log).expect("docker log should be written");
    assert!(log_text.contains("rm -f jyowo-"));
}

#[tokio::test]
#[ignore = "requires a live Docker daemon and JYOWO_LIVE_DOCKER_IMAGE"]
async fn live_docker_executes_configured_image() {
    let Ok(image) = std::env::var("JYOWO_LIVE_DOCKER_IMAGE") else {
        eprintln!("set JYOWO_LIVE_DOCKER_IMAGE to run this live test");
        return;
    };
    let root = temp_root("live-exec");
    let sandbox = DockerSandbox::builder()
        .image(image)
        .mount(VolumeMount::workspace(&root, "/workspace"))
        .network(NetworkMode::None)
        .lifecycle(ContainerLifecycle::EphemeralPerExec)
        .build()
        .expect("docker sandbox should build");

    let mut handle = sandbox
        .execute(shell_spec(), ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("live docker command should spawn");
    let output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    let outcome = handle.activity.wait().await.expect("wait should succeed");

    assert_eq!(output, "ignored");
    assert_eq!(outcome.exit_status, SandboxExitStatus::Code(0));
}

#[tokio::test]
#[ignore = "requires a live Docker daemon and JYOWO_LIVE_DOCKER_IMAGE"]
async fn live_docker_snapshots_and_restores_configured_image() {
    let Ok(image) = std::env::var("JYOWO_LIVE_DOCKER_IMAGE") else {
        eprintln!("set JYOWO_LIVE_DOCKER_IMAGE to run this live test");
        return;
    };
    let root = temp_root("live-snapshot");
    let sandbox = DockerSandbox::builder()
        .image(image)
        .mount(VolumeMount::workspace(&root, "/workspace"))
        .network(NetworkMode::None)
        .lifecycle(ContainerLifecycle::CreatePerSession {
            keep_alive_after_exit: Duration::ZERO,
        })
        .build()
        .expect("docker sandbox should build");

    let mut handle = sandbox
        .execute(shell_spec(), ExecContext::for_test(Arc::new(NullSink)))
        .await
        .expect("live docker command should spawn");
    let _output = collect_stdout(handle.stdout.take().expect("stdout should be piped")).await;
    handle.activity.wait().await.expect("wait should succeed");

    let spec = SnapshotSpec {
        kind: SessionSnapshotKind::ContainerImage,
        target_path: Some(root.join("snapshot.tar")),
        ..SnapshotSpec::default()
    };
    let image_tag = format!("jyowo/snapshot:{}", spec.session_id);
    let snapshot = sandbox
        .snapshot_session(&spec)
        .await
        .expect("live docker snapshot should export image");
    sandbox
        .restore_session(&snapshot)
        .await
        .expect("live docker restore should import image");
    sandbox.shutdown().await.expect("shutdown should cleanup");
    let _ = std::process::Command::new("docker")
        .args(["image", "rm", "-f", &image_tag])
        .status();

    assert_eq!(snapshot.kind, SessionSnapshotKind::ContainerImage);
    assert!(snapshot.metadata.size_bytes > 0);
}

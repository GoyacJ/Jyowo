//! Docker sandbox backend.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{
    ContainerLifecycleReason, ContainerLifecycleState, ContainerRef, Event, KillScope,
    NetworkAccess, ResourceLimits, SandboxContainerLifecycleTransitionEvent, SandboxError,
    SessionSnapshotKind,
};
use tokio::process::Command;
use tokio::sync::{Mutex, Notify};

use crate::process::{sandbox_error, spawn_backend_process};
use crate::{
    backend::{
        apply_wall_clock_resource_limit, has_non_wall_clock_resource_limits,
        unsupported_resource_limits,
    },
    ActivityHandle, CwdMarkerSupport, ExecContext, ExecOutcome, ExecSpec, NetworkPolicySupport,
    ProcessHandle, ResourceLimitSupport, SandboxBackend, SandboxBaseConfig, SandboxCapabilities,
    SessionSnapshotFile, Signal, SnapshotMetadata, SnapshotSpec, WorkspacePolicySupport,
};

const BACKEND_ID: &str = "docker";

#[derive(Debug, Clone)]
pub struct DockerSandbox {
    base: SandboxBaseConfig,
    image: String,
    volumes: Vec<VolumeMount>,
    network: NetworkMode,
    user: Option<String>,
    docker_binary: PathBuf,
    lifecycle: ContainerLifecycle,
    managed_containers: Arc<DockerContainerPool>,
}

impl DockerSandbox {
    pub fn new() -> Self {
        Self::builder()
            .build()
            .expect("default docker sandbox config should be valid")
    }

    pub fn builder() -> DockerSandboxBuilder {
        DockerSandboxBuilder::default()
    }

    /// Checks whether the Docker daemon is reachable. Public so desktop runtime
    /// assembly can verify Docker availability before registering it as a router candidate.
    pub async fn ensure_available(&self) -> Result<(), SandboxError> {
        let result = Command::new(&self.docker_binary)
            .arg("version")
            .arg("--format")
            .arg("{{.Server.Version}}")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await;
        match result {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(SandboxError::Unavailable {
                backend: BACKEND_ID.to_owned(),
                detail: format!("docker version exited with {status}"),
            }),
            Err(error) => Err(SandboxError::Unavailable {
                backend: BACKEND_ID.to_owned(),
                detail: error.to_string(),
            }),
        }
    }

    async fn active_container_id(&self, ctx: Option<&ExecContext>) -> Result<String, SandboxError> {
        match &self.lifecycle {
            ContainerLifecycle::BringYourOwn { container_id } => Ok(container_id.clone()),
            ContainerLifecycle::CreatePerSession { .. }
            | ContainerLifecycle::ReusePooled { .. } => self.ensure_snapshot_container(ctx).await,
            ContainerLifecycle::EphemeralPerExec => Err(SandboxError::SnapshotUnsupported {
                kind: "ephemeral_per_exec".to_owned(),
            }),
        }
    }

    async fn checkout_managed_container(
        &self,
        ctx: &ExecContext,
    ) -> Result<DockerContainerLease, SandboxError> {
        self.ensure_available().await?;
        loop {
            let notified = {
                let mut state = self.managed_containers.state.lock().await;
                self.evict_idle_containers(&mut state, ctx).await?;
                if let Some(container) = state
                    .containers
                    .iter_mut()
                    .find(|container| container.state == PooledContainerState::Idle)
                {
                    container.state = PooledContainerState::InUse;
                    container.last_ctx = Some(ctx.clone());
                    let container_id = container.id.clone();
                    emit_container_lifecycle(
                        ctx,
                        &container_id,
                        ContainerLifecycleState::Idle,
                        ContainerLifecycleState::InUse,
                        ContainerLifecycleReason::PoolReused,
                    )?;
                    return Ok(DockerContainerLease {
                        container_id,
                        sandbox: self.clone(),
                    });
                }

                let max_containers = self.managed_pool_size().ok_or_else(|| {
                    SandboxError::ContainerLifecycleError {
                        detail: "managed container checkout requested for unmanaged lifecycle"
                            .to_owned(),
                    }
                })?;
                if state.containers.len() < max_containers {
                    let container_id = format!("jyowo-{}", harness_contracts::SessionId::new());
                    self.start_managed_container(&container_id, &self.image)
                        .await?;
                    state.containers.push(PooledContainer {
                        id: container_id.clone(),
                        state: PooledContainerState::InUse,
                        last_used: Instant::now(),
                        last_ctx: Some(ctx.clone()),
                    });
                    emit_container_lifecycle(
                        ctx,
                        &container_id,
                        ContainerLifecycleState::Provisioning,
                        ContainerLifecycleState::Ready,
                        ContainerLifecycleReason::SessionAttached,
                    )?;
                    emit_container_lifecycle(
                        ctx,
                        &container_id,
                        ContainerLifecycleState::Ready,
                        ContainerLifecycleState::InUse,
                        ContainerLifecycleReason::SessionAttached,
                    )?;
                    return Ok(DockerContainerLease {
                        container_id,
                        sandbox: self.clone(),
                    });
                }

                self.managed_containers.notify.notified()
            };
            notified.await;
        }
    }

    async fn ensure_snapshot_container(
        &self,
        ctx: Option<&ExecContext>,
    ) -> Result<String, SandboxError> {
        self.ensure_available().await?;
        let mut state = self.managed_containers.state.lock().await;
        if let Some(container) = state.containers.first() {
            return Ok(container.id.clone());
        }
        let container_id = format!("jyowo-{}", harness_contracts::SessionId::new());
        self.start_managed_container(&container_id, &self.image)
            .await?;
        state.containers.push(PooledContainer {
            id: container_id.clone(),
            state: PooledContainerState::Idle,
            last_used: Instant::now(),
            last_ctx: ctx.cloned(),
        });
        if let Some(ctx) = ctx {
            emit_container_lifecycle(
                ctx,
                &container_id,
                ContainerLifecycleState::Provisioning,
                ContainerLifecycleState::Ready,
                ContainerLifecycleReason::SessionAttached,
            )?;
        }
        Ok(container_id)
    }

    async fn start_managed_container(
        &self,
        container_id: &str,
        image: &str,
    ) -> Result<(), SandboxError> {
        let mut command = Command::new(&self.docker_binary);
        command.arg("run").arg("-d").arg("--name").arg(container_id);
        self.apply_run_options(
            &mut command,
            &self.base.default_resource_limits,
            &self.network,
        );
        command.arg(image).arg("tail").arg("-f").arg("/dev/null");
        let output = command.output().await.map_err(sandbox_error)?;
        if !output.status.success() {
            return Err(SandboxError::ContainerLifecycleError {
                detail: format!(
                    "docker container start failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            });
        }
        Ok(())
    }

    async fn evict_idle_containers(
        &self,
        state: &mut DockerPoolState,
        ctx: &ExecContext,
    ) -> Result<(), SandboxError> {
        let Some(idle_timeout) = self.managed_idle_timeout() else {
            return Ok(());
        };
        let now = Instant::now();
        let mut index = 0;
        while index < state.containers.len() {
            let should_evict = state.containers[index].state == PooledContainerState::Idle
                && now.duration_since(state.containers[index].last_used) >= idle_timeout;
            if !should_evict {
                index += 1;
                continue;
            }
            let container = state.containers.remove(index);
            self.remove_container(&container.id).await?;
            emit_container_lifecycle(
                container.last_ctx.as_ref().unwrap_or(ctx),
                &container.id,
                ContainerLifecycleState::Idle,
                ContainerLifecycleState::Stopped,
                ContainerLifecycleReason::PoolEvicted,
            )?;
        }
        Ok(())
    }

    async fn release_managed_container(
        &self,
        container_id: &str,
        ctx: &ExecContext,
    ) -> Result<(), SandboxError> {
        let mut state = self.managed_containers.state.lock().await;
        if let Some(container) = state
            .containers
            .iter_mut()
            .find(|container| container.id == container_id)
        {
            container.state = PooledContainerState::Idle;
            container.last_used = Instant::now();
            container.last_ctx = Some(ctx.clone());
            emit_container_lifecycle(
                ctx,
                container_id,
                ContainerLifecycleState::InUse,
                ContainerLifecycleState::Idle,
                ContainerLifecycleReason::SessionDetached,
            )?;
        }
        drop(state);
        self.managed_containers.notify.notify_one();
        Ok(())
    }

    async fn remove_container(&self, container_id: &str) -> Result<(), SandboxError> {
        let status = Command::new(&self.docker_binary)
            .arg("rm")
            .arg("-f")
            .arg(container_id)
            .status()
            .await
            .map_err(sandbox_error)?;
        if !status.success() {
            return Err(SandboxError::ContainerLifecycleError {
                detail: format!("docker rm failed for {container_id}: {status}"),
            });
        }
        Ok(())
    }

    fn managed_pool_size(&self) -> Option<usize> {
        match self.lifecycle {
            ContainerLifecycle::CreatePerSession { .. } => Some(1),
            ContainerLifecycle::ReusePooled { pool_size, .. } => Some(pool_size as usize),
            ContainerLifecycle::BringYourOwn { .. } | ContainerLifecycle::EphemeralPerExec => None,
        }
    }

    fn managed_idle_timeout(&self) -> Option<Duration> {
        match self.lifecycle {
            ContainerLifecycle::ReusePooled { idle_timeout, .. } => Some(idle_timeout),
            _ => None,
        }
    }

    fn apply_run_options(
        &self,
        command: &mut Command,
        resource_limits: &ResourceLimits,
        network: &NetworkMode,
    ) {
        command.arg("--network").arg(network.as_docker_arg());
        apply_resource_options(command, resource_limits);
        if let Some(user) = &self.user {
            command.arg("--user").arg(user);
        }
        for volume in &self.volumes {
            command.arg("-v").arg(volume.as_docker_arg());
        }
        if let Some(workdir) = self
            .volumes
            .iter()
            .find(|volume| !volume.read_only)
            .map(|volume| volume.container_path.clone())
        {
            command.arg("-w").arg(workdir);
        }
    }

    fn validate_resource_policy(&self, spec: &mut ExecSpec) -> Result<(), SandboxError> {
        apply_wall_clock_resource_limit(spec, &self.base.default_resource_limits);
        match self.lifecycle {
            ContainerLifecycle::EphemeralPerExec => Ok(()),
            ContainerLifecycle::CreatePerSession { .. }
            | ContainerLifecycle::ReusePooled { .. } => {
                if has_non_wall_clock_resource_limits(&spec.policy.resource_limits) {
                    return Err(unsupported_resource_limits(
                        "docker managed container per-exec resource limits are not enforceable",
                    ));
                }
                Ok(())
            }
            ContainerLifecycle::BringYourOwn { .. } => {
                if has_non_wall_clock_resource_limits(&self.base.default_resource_limits)
                    || has_non_wall_clock_resource_limits(&spec.policy.resource_limits)
                {
                    return Err(unsupported_resource_limits(
                        "docker bring-your-own containers cannot apply resource limits",
                    ));
                }
                Ok(())
            }
        }
    }

    fn validate_network_policy(&self, spec: &ExecSpec) -> Result<(), SandboxError> {
        let requested = network_mode_for_policy(&spec.policy.network)?;
        match self.lifecycle {
            ContainerLifecycle::EphemeralPerExec => Ok(()),
            ContainerLifecycle::CreatePerSession { .. }
            | ContainerLifecycle::ReusePooled { .. }
            | ContainerLifecycle::BringYourOwn { .. } => {
                if requested == self.network {
                    Ok(())
                } else {
                    Err(SandboxError::CapabilityMismatch {
                        capability: "network".to_owned(),
                        detail: format!(
                            "docker {} lifecycle cannot change per-exec network policy from {:?} to {:?}",
                            lifecycle_name(&self.lifecycle),
                            self.network,
                            requested
                        ),
                    })
                }
            }
        }
    }

    fn validate_execute_policy(&self, spec: &mut ExecSpec) -> Result<(), SandboxError> {
        self.validate_network_policy(spec)?;
        self.validate_resource_policy(spec)?;
        crate::backend::validate_preflight_capabilities(
            self.backend_id(),
            &self.capabilities(),
            spec,
        )
    }

    /// Resolves the container-side workdir for an ephemeral container execution.
    ///
    /// When a volume mount maps the host workspace root to `/workspace`, host cwd
    /// paths under that root are rewritten to container-relative paths. When no cwd
    /// is specified, defaults to `/workspace`.
    fn resolve_container_workdir(&self, spec_cwd: Option<&Path>) -> Option<PathBuf> {
        let workspace_mount = self
            .volumes
            .iter()
            .find(|v| v.container_path == Path::new("/workspace"))?;

        match spec_cwd {
            Some(cwd) => {
                // If the host cwd is under the workspace root, rewrite to container path.
                match cwd.strip_prefix(&workspace_mount.host_path) {
                    Ok(relative) => {
                        let container_path = workspace_mount.container_path.join(relative);
                        Some(container_path)
                    }
                    Err(_) => {
                        // cwd outside the workspace mount — keep the host path as-is;
                        // the container may not have this path.
                        Some(cwd.to_path_buf())
                    }
                }
            }
            None => Some(workspace_mount.container_path.clone()),
        }
    }

    fn command_for_execute(&self, spec: &ExecSpec, container_id: Option<&str>) -> Command {
        let mut command = Command::new(&self.docker_binary);
        if let Some(container_id) = container_id {
            command.arg("exec").arg("-i").arg(container_id);
            if let Some(cwd) = &spec.cwd {
                command.arg("-w").arg(cwd);
            }
        } else {
            command.arg("run").arg("--rm").arg("-i");
            let network = network_mode_for_policy(&spec.policy.network)
                .unwrap_or_else(|_| self.network.clone());
            self.apply_run_options(
                &mut command,
                &effective_resource_limits(spec, &self.base),
                &network,
            );
            // Set the container workdir for ephemeral containers.
            if let Some(workdir) = self.resolve_container_workdir(spec.cwd.as_deref()) {
                command.arg("-w").arg(workdir);
            }
            command.arg(&self.image);
        }
        command.arg(&spec.command).args(&spec.args);
        command
    }

    fn snapshot_target_path(&self, spec: &SnapshotSpec, image_tag: &str) -> PathBuf {
        spec.target_path.clone().unwrap_or_else(|| {
            std::env::temp_dir()
                .join("jyowo-docker-snapshots")
                .join(format!(
                    "{}-{}.txt",
                    spec.session_id,
                    sanitize_image_tag(image_tag)
                ))
        })
    }
}

impl Default for DockerSandbox {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SandboxBackend for DockerSandbox {
    fn backend_id(&self) -> &'static str {
        BACKEND_ID
    }

    fn capabilities(&self) -> SandboxCapabilities {
        let snapshot_kinds = if matches!(self.lifecycle, ContainerLifecycle::EphemeralPerExec) {
            BTreeSet::new()
        } else {
            BTreeSet::from([
                SessionSnapshotKind::FilesystemImage,
                SessionSnapshotKind::ContainerImage,
            ])
        };
        SandboxCapabilities {
            supports_streaming: true,
            supports_stdin: true,
            supports_cwd_tracking: false,
            cwd_marker_support: CwdMarkerSupport::Disabled,
            supports_activity_heartbeat: true,
            supports_interactive_shell: false,
            supports_per_exec_env: false,
            network: NetworkPolicySupport {
                none: true,
                loopback_only: false,
                allowlist: false,
                unrestricted: true,
            },
            workspace: WorkspacePolicySupport {
                read_write_all: self.volumes.iter().any(|volume| !volume.read_only),
                read_only: false,
                writable_subpaths: false,
            },
            supports_gpu: false,
            supports_pty: false,
            supports_detach: false,
            supports_workspace_sync: false,
            supports_session_snapshot: !snapshot_kinds.is_empty(),
            max_concurrent_execs: match &self.lifecycle {
                ContainerLifecycle::EphemeralPerExec => u32::MAX,
                ContainerLifecycle::ReusePooled { pool_size, .. } => *pool_size,
                _ => 1,
            },
            supports_kill_scope: vec![KillScope::Process],
            snapshot_kinds,
            resource_limit_support: match self.lifecycle {
                ContainerLifecycle::EphemeralPerExec => ResourceLimitSupport {
                    memory: true,
                    cpu: true,
                    pids: true,
                    wall_clock: true,
                    open_files: true,
                },
                _ => ResourceLimitSupport {
                    wall_clock: true,
                    ..ResourceLimitSupport::default()
                },
            },
            default_timeout: Duration::from_secs(300),
        }
    }

    fn base_config(&self) -> SandboxBaseConfig {
        self.base.clone()
    }

    fn preflight_execute(&self, spec: &ExecSpec) -> Result<(), SandboxError> {
        let mut spec = spec.clone();
        self.validate_execute_policy(&mut spec)
    }

    async fn before_execute(
        &self,
        spec: &ExecSpec,
        _ctx: &ExecContext,
    ) -> Result<(), SandboxError> {
        let mut spec = spec.clone();
        self.validate_execute_policy(&mut spec)
    }

    async fn execute(
        &self,
        mut spec: ExecSpec,
        ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        self.validate_execute_policy(&mut spec)?;
        self.ensure_available().await?;
        let lease = match self.lifecycle {
            ContainerLifecycle::CreatePerSession { .. }
            | ContainerLifecycle::ReusePooled { .. } => {
                Some(self.checkout_managed_container(&ctx).await?)
            }
            _ => None,
        };
        let container_id = match &self.lifecycle {
            ContainerLifecycle::EphemeralPerExec => None,
            ContainerLifecycle::BringYourOwn { container_id } => Some(container_id.as_str()),
            ContainerLifecycle::CreatePerSession { .. }
            | ContainerLifecycle::ReusePooled { .. } => {
                lease.as_ref().map(|lease| lease.container_id.as_str())
            }
        };
        let command = self.command_for_execute(&spec, container_id.as_deref());
        match spawn_backend_process(BACKEND_ID, command, spec, ctx.clone(), self.base.clone()).await
        {
            Ok(mut handle) => {
                if let Some(lease) = lease {
                    handle.activity = Arc::new(DockerLeaseActivity {
                        inner: Arc::clone(&handle.activity),
                        lease: Mutex::new(Some(lease)),
                        ctx,
                    });
                }
                Ok(handle)
            }
            Err(error) => {
                if let Some(lease) = lease {
                    let _ = lease
                        .sandbox
                        .release_managed_container(&lease.container_id, &ctx)
                        .await;
                }
                Err(error)
            }
        }
    }

    async fn snapshot_session(
        &self,
        spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        if !matches!(
            spec.kind,
            SessionSnapshotKind::FilesystemImage | SessionSnapshotKind::ContainerImage
        ) {
            return Err(SandboxError::SnapshotUnsupported {
                kind: format!("{:?}", spec.kind),
            });
        }
        let container_id = self.active_container_id(None).await?;
        let image_tag = format!("jyowo/snapshot:{}", spec.session_id);
        let output = Command::new(&self.docker_binary)
            .arg("commit")
            .arg(&container_id)
            .arg(&image_tag)
            .output()
            .await
            .map_err(sandbox_error)?;
        if !output.status.success() {
            return Err(SandboxError::ContainerLifecycleError {
                detail: format!(
                    "docker commit failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            });
        }

        let path = self.snapshot_target_path(spec, &image_tag);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(sandbox_error)?;
        }
        if spec.kind == SessionSnapshotKind::ContainerImage {
            let output = Command::new(&self.docker_binary)
                .arg("save")
                .arg("-o")
                .arg(&path)
                .arg(&image_tag)
                .output()
                .await
                .map_err(sandbox_error)?;
            if !output.status.success() {
                return Err(SandboxError::ContainerLifecycleError {
                    detail: format!(
                        "docker save failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ),
                });
            }
        } else {
            std::fs::write(&path, image_tag.as_bytes()).map_err(sandbox_error)?;
        }
        let metadata = snapshot_metadata(&path)?;
        Ok(SessionSnapshotFile {
            session_id: spec.session_id,
            kind: spec.kind,
            path,
            metadata,
        })
    }

    async fn restore_session(&self, snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        if !matches!(
            snapshot.kind,
            SessionSnapshotKind::FilesystemImage | SessionSnapshotKind::ContainerImage
        ) {
            return Err(SandboxError::SnapshotUnsupported {
                kind: format!("{:?}", snapshot.kind),
            });
        }
        let image_tag = if snapshot.kind == SessionSnapshotKind::ContainerImage {
            let output = Command::new(&self.docker_binary)
                .arg("load")
                .arg("-i")
                .arg(&snapshot.path)
                .output()
                .await
                .map_err(sandbox_error)?;
            if !output.status.success() {
                return Err(SandboxError::ContainerLifecycleError {
                    detail: format!(
                        "docker load failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ),
                });
            }
            format!("jyowo/snapshot:{}", snapshot.session_id)
        } else {
            std::fs::read_to_string(&snapshot.path)
                .map_err(sandbox_error)?
                .trim()
                .to_owned()
        };
        if image_tag.is_empty() {
            return Err(SandboxError::Message(
                "docker snapshot image tag is empty".to_owned(),
            ));
        }
        let mut state = self.managed_containers.state.lock().await;
        let containers = std::mem::take(&mut state.containers);
        drop(state);
        for container in containers {
            self.remove_container(&container.id).await?;
        }

        if matches!(
            self.lifecycle,
            ContainerLifecycle::CreatePerSession { .. } | ContainerLifecycle::ReusePooled { .. }
        ) {
            let container_id = format!("jyowo-{}", harness_contracts::SessionId::new());
            self.start_managed_container(&container_id, &image_tag)
                .await?;
            let mut state = self.managed_containers.state.lock().await;
            state.containers.push(PooledContainer {
                id: container_id,
                state: PooledContainerState::Idle,
                last_used: Instant::now(),
                last_ctx: None,
            });
        }
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        let mut state = self.managed_containers.state.lock().await;
        let containers = std::mem::take(&mut state.containers);
        drop(state);

        match self.lifecycle {
            ContainerLifecycle::CreatePerSession {
                keep_alive_after_exit,
            } if keep_alive_after_exit > Duration::ZERO => {
                for container in containers {
                    if let Some(ctx) = &container.last_ctx {
                        emit_container_lifecycle(
                            ctx,
                            &container.id,
                            container.state.as_lifecycle_state(),
                            ContainerLifecycleState::Idle,
                            ContainerLifecycleReason::SessionDetached,
                        )?;
                    }
                    let docker_binary = self.docker_binary.clone();
                    let container_id = container.id.clone();
                    let ctx = container.last_ctx.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(keep_alive_after_exit).await;
                        let _ = Command::new(&docker_binary)
                            .arg("rm")
                            .arg("-f")
                            .arg(&container_id)
                            .status()
                            .await;
                        if let Some(ctx) = ctx {
                            let _ = emit_container_lifecycle(
                                &ctx,
                                &container_id,
                                ContainerLifecycleState::Idle,
                                ContainerLifecycleState::Stopped,
                                ContainerLifecycleReason::PoolEvicted,
                            );
                        }
                    });
                }
            }
            ContainerLifecycle::CreatePerSession { .. }
            | ContainerLifecycle::ReusePooled { .. } => {
                for container in containers {
                    self.remove_container(&container.id).await?;
                    if let Some(ctx) = &container.last_ctx {
                        emit_container_lifecycle(
                            ctx,
                            &container.id,
                            container.state.as_lifecycle_state(),
                            ContainerLifecycleState::Stopped,
                            ContainerLifecycleReason::SessionDetached,
                        )?;
                    }
                }
            }
            ContainerLifecycle::BringYourOwn { .. } | ContainerLifecycle::EphemeralPerExec => {}
        }
        self.managed_containers.notify.notify_waiters();
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct DockerSandboxBuilder {
    base: SandboxBaseConfig,
    image: String,
    volumes: Vec<VolumeMount>,
    network: NetworkMode,
    user: Option<String>,
    docker_binary: PathBuf,
    lifecycle: ContainerLifecycle,
}

impl Default for DockerSandboxBuilder {
    fn default() -> Self {
        Self {
            base: SandboxBaseConfig::default(),
            image: "jyowo-workspace:latest".to_owned(),
            volumes: Vec::new(),
            network: NetworkMode::Bridge,
            user: None,
            docker_binary: PathBuf::from("docker"),
            lifecycle: ContainerLifecycle::EphemeralPerExec,
        }
    }
}

impl DockerSandboxBuilder {
    pub fn base_config(mut self, base: SandboxBaseConfig) -> Self {
        self.base = base;
        self
    }

    pub fn image(mut self, image: impl Into<String>) -> Self {
        self.image = image.into();
        self
    }

    pub fn mount(mut self, volume: VolumeMount) -> Self {
        self.volumes.push(volume);
        self
    }

    pub fn network(mut self, network: NetworkMode) -> Self {
        self.network = network;
        self
    }

    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    pub fn docker_binary(mut self, binary: impl Into<PathBuf>) -> Self {
        self.docker_binary = binary.into();
        self
    }

    pub fn lifecycle(mut self, lifecycle: ContainerLifecycle) -> Self {
        self.lifecycle = lifecycle;
        self
    }

    pub fn build(self) -> Result<DockerSandbox, SandboxError> {
        if self.image.trim().is_empty() {
            return Err(SandboxError::Message(
                "docker image must not be empty".to_owned(),
            ));
        }
        if matches!(
            self.lifecycle,
            ContainerLifecycle::ReusePooled { pool_size: 0, .. }
        ) {
            return Err(SandboxError::Message(
                "docker reuse pool size must be greater than zero".to_owned(),
            ));
        }
        Ok(DockerSandbox {
            base: self.base,
            image: self.image,
            volumes: self.volumes,
            network: self.network,
            user: self.user,
            docker_binary: self.docker_binary,
            lifecycle: self.lifecycle,
            managed_containers: Arc::new(DockerContainerPool::default()),
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum NetworkMode {
    Host,
    Bridge,
    None,
    Custom(String),
}

impl NetworkMode {
    fn as_docker_arg(&self) -> String {
        match self {
            Self::Host => "host".to_owned(),
            Self::Bridge => "bridge".to_owned(),
            Self::None => "none".to_owned(),
            Self::Custom(name) => name.clone(),
        }
    }
}

fn network_mode_for_policy(network: &NetworkAccess) -> Result<NetworkMode, SandboxError> {
    match network {
        NetworkAccess::None => Ok(NetworkMode::None),
        NetworkAccess::Unrestricted => Ok(NetworkMode::Bridge),
        NetworkAccess::LoopbackOnly | NetworkAccess::AllowList(_) => {
            Err(SandboxError::CapabilityMismatch {
                capability: "network".to_owned(),
                detail: format!("docker network policy is not implemented: {network:?}"),
            })
        }
        _ => Err(SandboxError::CapabilityMismatch {
            capability: "network".to_owned(),
            detail: "unsupported docker network policy".to_owned(),
        }),
    }
}

fn lifecycle_name(lifecycle: &ContainerLifecycle) -> &'static str {
    match lifecycle {
        ContainerLifecycle::CreatePerSession { .. } => "create-per-session",
        ContainerLifecycle::ReusePooled { .. } => "reuse-pooled",
        ContainerLifecycle::BringYourOwn { .. } => "bring-your-own",
        ContainerLifecycle::EphemeralPerExec => "ephemeral-per-exec",
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct VolumeMount {
    pub host_path: PathBuf,
    pub container_path: PathBuf,
    pub read_only: bool,
    pub propagation: MountPropagation,
}

impl VolumeMount {
    pub fn workspace(host_path: impl Into<PathBuf>, container_path: impl Into<PathBuf>) -> Self {
        Self {
            host_path: host_path.into(),
            container_path: container_path.into(),
            read_only: false,
            propagation: MountPropagation::Private,
        }
    }

    fn as_docker_arg(&self) -> String {
        let mode = if self.read_only { "ro" } else { "rw" };
        let propagation = match self.propagation {
            MountPropagation::Private => "rprivate",
            MountPropagation::RShared => "rshared",
        };
        format!(
            "{}:{}:{mode},{propagation}",
            self.host_path.display(),
            self.container_path.display()
        )
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum MountPropagation {
    Private,
    RShared,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum ContainerLifecycle {
    CreatePerSession {
        keep_alive_after_exit: Duration,
    },
    ReusePooled {
        pool_size: u32,
        idle_timeout: Duration,
    },
    BringYourOwn {
        container_id: String,
    },
    EphemeralPerExec,
}

#[derive(Debug, Default)]
struct DockerContainerPool {
    state: Mutex<DockerPoolState>,
    notify: Notify,
}

#[derive(Debug, Default)]
struct DockerPoolState {
    containers: Vec<PooledContainer>,
}

#[derive(Clone)]
struct PooledContainer {
    id: String,
    state: PooledContainerState,
    last_used: Instant,
    last_ctx: Option<ExecContext>,
}

impl fmt::Debug for PooledContainer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PooledContainer")
            .field("id", &self.id)
            .field("state", &self.state)
            .field("last_used", &self.last_used)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum PooledContainerState {
    Idle,
    InUse,
}

impl PooledContainerState {
    fn as_lifecycle_state(self) -> ContainerLifecycleState {
        match self {
            Self::Idle => ContainerLifecycleState::Idle,
            Self::InUse => ContainerLifecycleState::InUse,
        }
    }
}

struct DockerContainerLease {
    container_id: String,
    sandbox: DockerSandbox,
}

struct DockerLeaseActivity {
    inner: Arc<dyn ActivityHandle>,
    lease: Mutex<Option<DockerContainerLease>>,
    ctx: ExecContext,
}

#[async_trait]
impl ActivityHandle for DockerLeaseActivity {
    async fn wait(&self) -> Result<ExecOutcome, SandboxError> {
        let outcome = self.inner.wait().await;
        let release = if let Some(lease) = self.lease.lock().await.take() {
            lease
                .sandbox
                .release_managed_container(&lease.container_id, &self.ctx)
                .await
        } else {
            Ok(())
        };
        match (outcome, release) {
            (Ok(outcome), Ok(())) => Ok(outcome),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }

    async fn kill(&self, signal: Signal, scope: KillScope) -> Result<(), SandboxError> {
        self.inner.kill(signal, scope).await
    }

    fn touch(&self) {
        self.inner.touch();
    }

    fn last_activity(&self) -> Instant {
        self.inner.last_activity()
    }
}

fn snapshot_metadata(path: &Path) -> Result<SnapshotMetadata, SandboxError> {
    let bytes = std::fs::read(path).map_err(sandbox_error)?;
    let hash = blake3::hash(&bytes);
    Ok(SnapshotMetadata {
        size_bytes: bytes.len() as u64,
        content_hash: *hash.as_bytes(),
        created_at: Utc::now(),
    })
}

fn sanitize_image_tag(image_tag: &str) -> String {
    image_tag
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

fn effective_resource_limits(spec: &ExecSpec, base: &SandboxBaseConfig) -> ResourceLimits {
    ResourceLimits {
        max_memory_bytes: spec
            .policy
            .resource_limits
            .max_memory_bytes
            .or(base.default_resource_limits.max_memory_bytes),
        max_cpu_cores: spec
            .policy
            .resource_limits
            .max_cpu_cores
            .or(base.default_resource_limits.max_cpu_cores),
        max_pids: spec
            .policy
            .resource_limits
            .max_pids
            .or(base.default_resource_limits.max_pids),
        max_wall_clock_ms: spec
            .policy
            .resource_limits
            .max_wall_clock_ms
            .or(base.default_resource_limits.max_wall_clock_ms),
        max_open_files: spec
            .policy
            .resource_limits
            .max_open_files
            .or(base.default_resource_limits.max_open_files),
    }
}

fn apply_resource_options(command: &mut Command, limits: &ResourceLimits) {
    if let Some(memory) = limits.max_memory_bytes {
        command.arg("--memory").arg(memory.to_string());
    }
    if let Some(cpus) = limits.max_cpu_cores {
        command.arg("--cpus").arg(format_cpu_cores(cpus));
    }
    if let Some(pids) = limits.max_pids {
        command.arg("--pids-limit").arg(pids.to_string());
    }
    if let Some(open_files) = limits.max_open_files {
        command
            .arg("--ulimit")
            .arg(format!("nofile={open_files}:{open_files}"));
    }
}

fn emit_container_lifecycle(
    ctx: &ExecContext,
    container_id: &str,
    from: ContainerLifecycleState,
    to: ContainerLifecycleState,
    reason: ContainerLifecycleReason,
) -> Result<(), SandboxError> {
    ctx.event_sink
        .emit(Event::SandboxContainerLifecycleTransition(
            SandboxContainerLifecycleTransitionEvent {
                session_id: ctx.session_id,
                backend_id: BACKEND_ID.to_owned(),
                container_ref: ContainerRef {
                    backend_kind: BACKEND_ID.to_owned(),
                    container_id: container_id.to_owned(),
                },
                from,
                to,
                reason,
                at: Utc::now(),
            },
        ))
}

fn format_cpu_cores(cpus: f32) -> String {
    let formatted = format!("{cpus:.3}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

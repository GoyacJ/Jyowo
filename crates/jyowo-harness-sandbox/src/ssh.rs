//! SSH sandbox backend.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use harness_contracts::{KillScope, SandboxError, SessionSnapshotKind};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::process::{sandbox_error, spawn_backend_process};
use crate::{
    backend::{
        apply_wall_clock_resource_limit, has_non_wall_clock_resource_limits,
        unsupported_resource_limits,
    },
    CwdMarkerSupport, ExecContext, ExecSpec, ProcessHandle, ResourceLimitSupport, SandboxBackend,
    SandboxBaseConfig, SandboxCapabilities, SessionSnapshotFile, SnapshotMetadata, SnapshotSpec,
};

const BACKEND_ID: &str = "ssh";

#[derive(Debug, Clone)]
pub struct SshSandbox {
    base: SandboxBaseConfig,
    host: String,
    port: u16,
    user: String,
    auth: SshAuth,
    keepalive: Duration,
    multiplex: bool,
    workspace_sync: WorkspaceSyncStrategy,
    workspace_sync_config: Option<WorkspaceSyncConfig>,
    remote_workspace: PathBuf,
    ssh_binary: PathBuf,
}

impl SshSandbox {
    pub fn new() -> Self {
        Self::builder()
            .build()
            .expect("default ssh sandbox config should be valid")
    }

    pub fn builder() -> SshSandboxBuilder {
        SshSandboxBuilder::default()
    }

    async fn ensure_available(&self) -> Result<(), SandboxError> {
        let mut command = self.base_ssh_command();
        command.arg("--").arg("__jyowo_probe__");
        let result = command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await;
        match result {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(SandboxError::Unavailable {
                backend: BACKEND_ID.to_owned(),
                detail: format!("probe exited with {status}"),
            }),
            Err(error) => Err(SandboxError::Unavailable {
                backend: BACKEND_ID.to_owned(),
                detail: error.to_string(),
            }),
        }
    }

    fn base_ssh_command(&self) -> Command {
        let mut command = Command::new(&self.ssh_binary);
        command
            .arg("-p")
            .arg(self.port.to_string())
            .arg("-o")
            .arg(format!(
                "ServerAliveInterval={}",
                self.keepalive.as_secs().max(1)
            ));
        if self.multiplex {
            command.arg("-o").arg("ControlMaster=auto");
        }
        match &self.auth {
            SshAuth::KeyFile(path) => {
                command.arg("-i").arg(path);
            }
            SshAuth::Agent => {}
            SshAuth::KeyInline(_) | SshAuth::Password(_) => {
                command.arg("-o").arg("BatchMode=yes");
            }
        }
        command.arg(format!("{}@{}", self.user, self.host));
        command
    }

    fn command_for_execute(&self, spec: &ExecSpec) -> Command {
        let mut command = self.base_ssh_command();
        command.arg("--").arg(&spec.command).args(&spec.args);
        command
    }

    pub fn before_execute_sync_plan(
        &self,
    ) -> Result<Option<WorkspaceSyncCommandPlan>, SandboxError> {
        match self.workspace_sync {
            WorkspaceSyncStrategy::None => Ok(None),
            WorkspaceSyncStrategy::RsyncPush | WorkspaceSyncStrategy::RsyncBidi => {
                Ok(Some(self.rsync_plan(SyncDirection::Push)?))
            }
        }
    }

    pub fn after_execute_sync_plan(
        &self,
    ) -> Result<Option<WorkspaceSyncCommandPlan>, SandboxError> {
        match self.workspace_sync {
            WorkspaceSyncStrategy::RsyncBidi => Ok(Some(self.rsync_plan(SyncDirection::Pull)?)),
            WorkspaceSyncStrategy::None | WorkspaceSyncStrategy::RsyncPush => Ok(None),
        }
    }

    fn rsync_plan(
        &self,
        direction: SyncDirection,
    ) -> Result<WorkspaceSyncCommandPlan, SandboxError> {
        let config = self.workspace_sync_config.as_ref().ok_or_else(|| {
            SandboxError::Message("ssh workspace sync config is required".to_owned())
        })?;
        let mut args = vec![
            "-az".to_owned(),
            "-e".to_owned(),
            self.rsync_ssh_transport(),
        ];
        if config.delete {
            args.push("--delete".to_owned());
        }
        for exclude in &config.excludes {
            args.push("--exclude".to_owned());
            args.push(exclude.clone());
        }

        match direction {
            SyncDirection::Push => {
                args.push(path_with_trailing_slash(&config.local_workspace));
                args.push(format!(
                    "{}:{}/",
                    self.remote_target(),
                    config.remote_workspace.display()
                ));
            }
            SyncDirection::Pull => {
                args.push(format!(
                    "{}:{}/",
                    self.remote_target(),
                    config.remote_workspace.display()
                ));
                args.push(path_with_trailing_slash(&config.local_workspace));
            }
        }

        Ok(WorkspaceSyncCommandPlan {
            program: config.rsync_binary.clone(),
            args,
        })
    }

    fn rsync_ssh_transport(&self) -> String {
        let mut parts = vec![
            self.ssh_binary.display().to_string(),
            "-p".to_owned(),
            self.port.to_string(),
            "-o".to_owned(),
            format!("ServerAliveInterval={}", self.keepalive.as_secs().max(1)),
        ];
        if self.multiplex {
            parts.push("-o".to_owned());
            parts.push("ControlMaster=auto".to_owned());
        }
        if let SshAuth::KeyFile(path) = &self.auth {
            parts.push("-i".to_owned());
            parts.push(path.display().to_string());
        }
        parts.join(" ")
    }

    fn remote_target(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }

    fn snapshot_target_path(&self, spec: &SnapshotSpec) -> PathBuf {
        spec.target_path.clone().unwrap_or_else(|| {
            std::env::temp_dir()
                .join("jyowo-ssh-snapshots")
                .join(format!("{}.tar", spec.session_id))
        })
    }
}

impl Default for SshSandbox {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SandboxBackend for SshSandbox {
    fn backend_id(&self) -> &'static str {
        BACKEND_ID
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            supports_streaming: true,
            supports_stdin: true,
            supports_cwd_tracking: false,
            cwd_marker_support: CwdMarkerSupport::Disabled,
            supports_activity_heartbeat: true,
            supports_interactive_shell: false,
            supports_network: true,
            supports_filesystem_write: true,
            supports_gpu: false,
            supports_pty: false,
            supports_detach: false,
            supports_workspace_sync: !matches!(&self.workspace_sync, WorkspaceSyncStrategy::None),
            supports_session_snapshot: true,
            max_concurrent_execs: u32::MAX,
            supports_kill_scope: vec![KillScope::Process],
            snapshot_kinds: BTreeSet::from([SessionSnapshotKind::FilesystemImage]),
            resource_limit_support: ResourceLimitSupport {
                wall_clock: true,
                ..ResourceLimitSupport::default()
            },
            default_timeout: Duration::from_secs(300),
        }
    }

    fn base_config(&self) -> SandboxBaseConfig {
        self.base.clone()
    }

    async fn before_execute(
        &self,
        _spec: &ExecSpec,
        _ctx: &ExecContext,
    ) -> Result<(), SandboxError> {
        if let Some(plan) = self.before_execute_sync_plan()? {
            execute_rsync_plan(SyncDirection::Push, plan).await?;
        }
        Ok(())
    }

    async fn execute(
        &self,
        mut spec: ExecSpec,
        ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        self.validate_resource_policy(&mut spec)?;
        self.ensure_available().await?;
        self.before_execute(&spec, &ctx).await?;
        let command = self.command_for_execute(&spec);
        spawn_backend_process(BACKEND_ID, command, spec, ctx, self.base.clone()).await
    }

    async fn after_execute(
        &self,
        _outcome: &crate::ExecOutcome,
        _ctx: &ExecContext,
    ) -> Result<(), SandboxError> {
        if let Some(plan) = self.after_execute_sync_plan()? {
            execute_rsync_plan(SyncDirection::Pull, plan).await?;
        }
        Ok(())
    }

    async fn snapshot_session(
        &self,
        spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        if spec.kind != SessionSnapshotKind::FilesystemImage {
            return Err(SandboxError::SnapshotUnsupported {
                kind: format!("{:?}", spec.kind),
            });
        }
        self.ensure_available().await?;
        let path = self.snapshot_target_path(spec);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(sandbox_error)?;
        }
        let output = self
            .base_ssh_command()
            .arg("--")
            .arg("tar")
            .arg("-C")
            .arg(&self.remote_workspace)
            .arg("-cf")
            .arg("-")
            .arg(".")
            .output()
            .await
            .map_err(sandbox_error)?;
        if !output.status.success() {
            return Err(SandboxError::Message(format!(
                "ssh snapshot failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        std::fs::write(&path, &output.stdout).map_err(sandbox_error)?;
        let metadata = snapshot_metadata(&path)?;
        Ok(SessionSnapshotFile {
            session_id: spec.session_id,
            kind: spec.kind,
            path,
            metadata,
        })
    }

    async fn restore_session(&self, snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        if snapshot.kind != SessionSnapshotKind::FilesystemImage {
            return Err(SandboxError::SnapshotUnsupported {
                kind: format!("{:?}", snapshot.kind),
            });
        }
        validate_snapshot_archive(&snapshot.path)?;
        self.ensure_available().await?;
        let bytes = std::fs::read(&snapshot.path).map_err(sandbox_error)?;
        let mut child = self
            .base_ssh_command()
            .arg("--")
            .arg("tar")
            .arg("-C")
            .arg(&self.remote_workspace)
            .arg("-xf")
            .arg("-")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(sandbox_error)?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&bytes).await.map_err(sandbox_error)?;
        }
        let output = child.wait_with_output().await.map_err(sandbox_error)?;
        if !output.status.success() {
            return Err(SandboxError::Message(format!(
                "ssh restore failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

impl SshSandbox {
    fn validate_resource_policy(&self, spec: &mut ExecSpec) -> Result<(), SandboxError> {
        apply_wall_clock_resource_limit(spec, &self.base.default_resource_limits);
        if has_non_wall_clock_resource_limits(&self.base.default_resource_limits)
            || has_non_wall_clock_resource_limits(&spec.policy.resource_limits)
        {
            return Err(unsupported_resource_limits(
                "ssh resource limits are not implemented beyond wall-clock",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SshSandboxBuilder {
    base: SandboxBaseConfig,
    host: String,
    port: u16,
    user: String,
    auth: SshAuth,
    keepalive: Duration,
    multiplex: bool,
    workspace_sync: WorkspaceSyncStrategy,
    workspace_sync_config: Option<WorkspaceSyncConfig>,
    remote_workspace: PathBuf,
    ssh_binary: PathBuf,
}

impl Default for SshSandboxBuilder {
    fn default() -> Self {
        Self {
            base: SandboxBaseConfig::default(),
            host: "localhost".to_owned(),
            port: 22,
            user: whoami_fallback(),
            auth: SshAuth::Agent,
            keepalive: Duration::from_secs(30),
            multiplex: false,
            workspace_sync: WorkspaceSyncStrategy::None,
            workspace_sync_config: None,
            remote_workspace: PathBuf::from("/workspace"),
            ssh_binary: PathBuf::from("ssh"),
        }
    }
}

impl SshSandboxBuilder {
    pub fn base_config(mut self, base: SandboxBaseConfig) -> Self {
        self.base = base;
        self
    }

    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = user.into();
        self
    }

    pub fn auth(mut self, auth: SshAuth) -> Self {
        self.auth = auth;
        self
    }

    pub fn keepalive(mut self, keepalive: Duration) -> Self {
        self.keepalive = keepalive;
        self
    }

    pub fn multiplex(mut self, multiplex: bool) -> Self {
        self.multiplex = multiplex;
        self
    }

    pub fn workspace_sync(mut self, workspace_sync: WorkspaceSyncStrategy) -> Self {
        self.workspace_sync = workspace_sync;
        self
    }

    pub fn workspace_sync_config(mut self, config: WorkspaceSyncConfig) -> Self {
        self.workspace_sync_config = Some(config);
        self
    }

    pub fn remote_workspace(mut self, remote_workspace: impl Into<PathBuf>) -> Self {
        self.remote_workspace = remote_workspace.into();
        self
    }

    pub fn ssh_binary(mut self, ssh_binary: impl Into<PathBuf>) -> Self {
        self.ssh_binary = ssh_binary.into();
        self
    }

    pub fn build(self) -> Result<SshSandbox, SandboxError> {
        if self.host.trim().is_empty() {
            return Err(SandboxError::Message(
                "ssh host must not be empty".to_owned(),
            ));
        }
        if self.user.trim().is_empty() {
            return Err(SandboxError::Message(
                "ssh user must not be empty".to_owned(),
            ));
        }
        Ok(SshSandbox {
            base: self.base,
            host: self.host,
            port: self.port,
            user: self.user,
            auth: self.auth,
            keepalive: self.keepalive,
            multiplex: self.multiplex,
            workspace_sync: self.workspace_sync,
            workspace_sync_config: self.workspace_sync_config,
            remote_workspace: self.remote_workspace,
            ssh_binary: self.ssh_binary,
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum SshAuth {
    KeyFile(PathBuf),
    KeyInline(String),
    Agent,
    Password(String),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum WorkspaceSyncStrategy {
    None,
    RsyncPush,
    RsyncBidi,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct WorkspaceSyncConfig {
    pub local_workspace: PathBuf,
    pub remote_workspace: PathBuf,
    pub rsync_binary: PathBuf,
    pub excludes: Vec<String>,
    pub delete: bool,
}

impl WorkspaceSyncConfig {
    pub fn rsync(
        local_workspace: impl Into<PathBuf>,
        remote_workspace: impl Into<PathBuf>,
    ) -> Self {
        Self {
            local_workspace: local_workspace.into(),
            remote_workspace: remote_workspace.into(),
            rsync_binary: PathBuf::from("rsync"),
            excludes: Vec::new(),
            delete: true,
        }
    }

    #[must_use]
    pub fn rsync_binary(mut self, binary: impl Into<PathBuf>) -> Self {
        self.rsync_binary = binary.into();
        self
    }

    #[must_use]
    pub fn exclude(mut self, pattern: impl Into<String>) -> Self {
        self.excludes.push(pattern.into());
        self
    }

    #[must_use]
    pub fn delete(mut self, delete: bool) -> Self {
        self.delete = delete;
        self
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct WorkspaceSyncCommandPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
enum SyncDirection {
    Push,
    Pull,
}

impl SyncDirection {
    fn as_str(self) -> &'static str {
        match self {
            SyncDirection::Push => "push",
            SyncDirection::Pull => "pull",
        }
    }
}

async fn execute_rsync_plan(
    direction: SyncDirection,
    plan: WorkspaceSyncCommandPlan,
) -> Result<(), SandboxError> {
    let output = Command::new(&plan.program)
        .args(&plan.args)
        .output()
        .await
        .map_err(|error| SandboxError::WorkspaceSyncFailed {
            direction: direction.as_str().to_owned(),
            program: plan.program.display().to_string(),
            detail: error.to_string(),
        })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let detail = if stderr.is_empty() && stdout.is_empty() {
        format!("rsync exited with {}", output.status)
    } else if stderr.is_empty() {
        format!("rsync exited with {}: {stdout}", output.status)
    } else {
        format!("rsync exited with {}: {stderr}", output.status)
    };
    Err(SandboxError::WorkspaceSyncFailed {
        direction: direction.as_str().to_owned(),
        program: plan.program.display().to_string(),
        detail,
    })
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

fn validate_snapshot_archive(path: &Path) -> Result<(), SandboxError> {
    let file = std::fs::File::open(path).map_err(sandbox_error)?;
    let mut archive = tar::Archive::new(file);
    for entry in archive.entries().map_err(sandbox_error)? {
        let entry = entry.map_err(sandbox_error)?;
        let path = entry.path().map_err(sandbox_error)?;
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(SandboxError::Message(format!(
                "snapshot path escapes sandbox root: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn whoami_fallback() -> String {
    std::env::var("USER").unwrap_or_else(|_| "jyowo".to_owned())
}

fn path_with_trailing_slash(path: &Path) -> String {
    let value = path.display().to_string();
    if value.ends_with('/') {
        value
    } else {
        format!("{value}/")
    }
}

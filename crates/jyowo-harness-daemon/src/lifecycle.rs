use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs2::FileExt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LifecycleError {
    #[error("invalid user instance identifier")]
    InvalidUserInstance,
    #[error("another daemon instance owns the runtime lock")]
    AlreadyRunning,
    #[error("runtime path is not an owner-controlled regular file or directory: {0}")]
    UnsafeRuntimePath(PathBuf),
    #[error("runtime I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

pub struct RuntimeGuard {
    runtime_dir: PathBuf,
    lock_path: PathBuf,
    token_path: PathBuf,
    endpoint_path: PathBuf,
    connection_token: String,
    lock_file: File,
}

impl RuntimeGuard {
    pub fn acquire(root: impl AsRef<Path>, user_instance_id: &str) -> Result<Self, LifecycleError> {
        if user_instance_id.is_empty()
            || matches!(user_instance_id, "." | "..")
            || !user_instance_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(LifecycleError::InvalidUserInstance);
        }
        let runtime_dir = root.as_ref().join(user_instance_id);
        ensure_private_directory(&runtime_dir)?;

        let lock_path = runtime_dir.join("daemon.lock");
        let lock_file = open_private_file(&lock_path, false)?;
        lock_file
            .try_lock_exclusive()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::WouldBlock => LifecycleError::AlreadyRunning,
                _ => LifecycleError::Io(error),
            })?;

        let token_path = runtime_dir.join("connection.token");
        let connection_token = uuid::Uuid::new_v4().simple().to_string();
        let mut token_file = open_private_file(&token_path, true)?;
        token_file.write_all(connection_token.as_bytes())?;
        token_file.sync_all()?;

        Ok(Self {
            endpoint_path: runtime_dir.join("daemon.sock"),
            runtime_dir,
            lock_path,
            token_path,
            connection_token,
            lock_file,
        })
    }

    #[must_use]
    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }

    #[must_use]
    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    #[must_use]
    pub fn token_path(&self) -> &Path {
        &self.token_path
    }

    #[must_use]
    pub fn endpoint_path(&self) -> &Path {
        &self.endpoint_path
    }

    #[must_use]
    pub fn connection_token(&self) -> &str {
        &self.connection_token
    }

    #[cfg(unix)]
    pub fn prepare_endpoint(&self) -> Result<(), LifecycleError> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};

        let metadata = match std::fs::symlink_metadata(&self.endpoint_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_socket() || metadata.uid() != rustix::process::getuid().as_raw()
        {
            return Err(LifecycleError::UnsafeRuntimePath(
                self.endpoint_path.clone(),
            ));
        }
        std::fs::remove_file(&self.endpoint_path)?;
        Ok(())
    }

    #[cfg(not(unix))]
    pub fn prepare_endpoint(&self) -> Result<(), LifecycleError> {
        Ok(())
    }
}

impl Drop for RuntimeGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.token_path);
        let _ = FileExt::unlock(&self.lock_file);
    }
}

fn ensure_private_directory(path: &Path) -> Result<(), LifecycleError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() => {
            return Err(LifecycleError::UnsafeRuntimePath(path.to_path_buf()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(path)?;
        }
        Err(error) => return Err(error.into()),
    }
    validate_unix_owner(path, &std::fs::metadata(path)?, false)?;
    set_owner_only_directory(path)?;
    Ok(())
}

fn open_private_file(path: &Path, truncate: bool) -> Result<File, LifecycleError> {
    if std::fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.file_type().is_symlink() || !metadata.file_type().is_file())
    {
        return Err(LifecycleError::UnsafeRuntimePath(path.to_path_buf()));
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .truncate(truncate);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(LifecycleError::UnsafeRuntimePath(path.to_path_buf()));
    }
    validate_unix_owner(path, &metadata, true)?;
    set_owner_only_file(path)?;
    Ok(file)
}

#[cfg(unix)]
fn validate_unix_owner(
    path: &Path,
    metadata: &std::fs::Metadata,
    require_single_link: bool,
) -> Result<(), LifecycleError> {
    use std::os::unix::fs::MetadataExt;
    if metadata.uid() != rustix::process::getuid().as_raw()
        || (require_single_link && metadata.nlink() != 1)
    {
        return Err(LifecycleError::UnsafeRuntimePath(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_unix_owner(
    _path: &Path,
    _metadata: &std::fs::Metadata,
    _require_single_link: bool,
) -> Result<(), LifecycleError> {
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_directory(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_owner_only_directory(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_file(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_owner_only_file(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[derive(Debug, Clone)]
pub struct DaemonActivity {
    clients: usize,
    active_tasks: usize,
    background_processes: usize,
    idle_since: Option<Instant>,
}

impl DaemonActivity {
    #[must_use]
    pub const fn new(now: Instant) -> Self {
        Self {
            clients: 0,
            active_tasks: 0,
            background_processes: 0,
            idle_since: Some(now),
        }
    }

    pub fn client_connected(&mut self) {
        self.clients += 1;
        self.idle_since = None;
    }

    pub fn client_disconnected(&mut self, now: Instant) {
        self.clients = self.clients.saturating_sub(1);
        self.refresh_idle(now);
    }

    pub fn task_started(&mut self) {
        self.active_tasks += 1;
        self.idle_since = None;
    }

    pub fn task_finished(&mut self, now: Instant) {
        self.active_tasks = self.active_tasks.saturating_sub(1);
        self.refresh_idle(now);
    }

    pub fn background_process_started(&mut self) {
        self.background_processes += 1;
        self.idle_since = None;
    }

    pub fn background_process_finished(&mut self, now: Instant) {
        self.background_processes = self.background_processes.saturating_sub(1);
        self.refresh_idle(now);
    }

    #[must_use]
    pub const fn active_tasks(&self) -> usize {
        self.active_tasks
    }

    #[must_use]
    pub fn should_shutdown(&self, now: Instant, timeout: Duration) -> bool {
        self.idle_since
            .is_some_and(|idle_since| now.saturating_duration_since(idle_since) >= timeout)
    }

    fn refresh_idle(&mut self, now: Instant) {
        if self.clients == 0 && self.active_tasks == 0 && self.background_processes == 0 {
            self.idle_since.get_or_insert(now);
        }
    }
}

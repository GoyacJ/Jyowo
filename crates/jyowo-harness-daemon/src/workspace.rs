use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs::{File, Metadata};
use std::future::Future;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};

use chrono::{DateTime, Utc};
use harness_agent_runtime::{GitDiscovery, WorkspaceIsolationError, WorkspaceLeaseRepository};
use harness_contracts::{ActorId, CommandId, TaskId, WorkspaceLeaseId, WorkspaceMode};
use harness_journal::{
    AcceptedCommand, AcquireTaskWorkspaceLease, CommandOutcome, EventAuthority,
    ReleaseTaskWorkspaceLeaseOutcome, TaskStore, TaskStoreError, TaskWorkspaceAcquireOutcome,
    TaskWorkspaceLease, TaskWorkspaceLeaseState,
};
use harness_sandbox::LocalIsolation;
use serde_json::json;
use thiserror::Error;

use crate::WorkspaceToolAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceAccess {
    ReadOnly,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceExecutionKind {
    Foreground,
    Background,
    ParallelChild,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceLeaseRequest {
    pub task_id: TaskId,
    pub actor_id: ActorId,
    pub root: PathBuf,
    pub mode: Option<WorkspaceMode>,
    pub access: WorkspaceAccess,
    pub execution_kind: WorkspaceExecutionKind,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceAcquireOutcome {
    Acquired(TaskWorkspaceLease),
    Waiting(TaskWorkspaceLease),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceCleanupOutcome {
    Released(ReleaseTaskWorkspaceLeaseOutcome),
    CleanupBlocked {
        lease: TaskWorkspaceLease,
        patch_path: PathBuf,
    },
}

const MAX_WORKSPACE_READ_BYTES: u64 = 16 * 1024 * 1024;
#[cfg(unix)]
static WORKSPACE_TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct WorkspaceToolAuthorization {
    pub lease_id: WorkspaceLeaseId,
    pub writable: bool,
    root: Arc<File>,
    relative_path: PathBuf,
    write_lock: Arc<Mutex<()>>,
    state: Arc<WorkspaceToolAuthorizationState>,
}

impl WorkspaceToolAuthorization {
    #[must_use]
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    pub fn read_bytes(&self) -> Result<Vec<u8>, WorkspaceCoordinatorError> {
        let _operation = self.begin_operation()?;
        #[cfg(not(unix))]
        return Err(WorkspaceCoordinatorError::SecureWorkspaceIoUnavailable);
        #[cfg(unix)]
        {
            let (directory, file_name) = self.open_parent()?;
            read_workspace_file_at(&directory, &file_name).map(|(bytes, _, _)| bytes)
        }
    }

    pub fn write_bytes(&self, bytes: &[u8]) -> Result<(), WorkspaceCoordinatorError> {
        let _operation = self.begin_operation()?;
        if !self.writable {
            return Err(WorkspaceCoordinatorError::ReadOnlyAuthorization {
                lease_id: self.lease_id,
            });
        }
        let _write_guard = self
            .write_lock
            .lock()
            .map_err(|_| std::io::Error::other("workspace write lock poisoned"))?;
        #[cfg(not(unix))]
        return Err(WorkspaceCoordinatorError::SecureWorkspaceIoUnavailable);
        #[cfg(unix)]
        {
            let (directory, file_name) = self.open_parent()?;
            let mode = workspace_file_mode_at(&directory, &file_name)?.unwrap_or(0o600);
            replace_workspace_file_at(&directory, &file_name, bytes, mode, None)
        }
    }

    pub fn edit_bytes<T>(
        &self,
        edit: impl FnOnce(&[u8]) -> std::io::Result<(Vec<u8>, T)>,
    ) -> Result<T, WorkspaceCoordinatorError> {
        let _operation = self.begin_operation()?;
        if !self.writable {
            return Err(WorkspaceCoordinatorError::ReadOnlyAuthorization {
                lease_id: self.lease_id,
            });
        }
        // Daemon-authorized edits for the same lease/path are serialized through
        // the compare-and-rename section. External host writers do not share this lock.
        let _write_guard = self
            .write_lock
            .lock()
            .map_err(|_| std::io::Error::other("workspace write lock poisoned"))?;
        #[cfg(not(unix))]
        {
            let _ = edit;
            return Err(WorkspaceCoordinatorError::SecureWorkspaceIoUnavailable);
        }
        #[cfg(unix)]
        {
            let (directory, file_name) = self.open_parent()?;
            let (bytes, version, mode) = read_workspace_file_at(&directory, &file_name)?;
            let (edited, result) = edit(&bytes)?;
            replace_workspace_file_at(&directory, &file_name, &edited, mode, Some(&version))?;
            Ok(result)
        }
    }

    fn begin_operation(&self) -> Result<WorkspaceToolOperationGuard, WorkspaceCoordinatorError> {
        let mut state = self
            .state
            .inner
            .lock()
            .map_err(|_| std::io::Error::other("workspace authorization lock poisoned"))?;
        if !state.accepting_operations {
            return Err(WorkspaceCoordinatorError::ExpiredToolAuthorization {
                lease_id: self.lease_id,
            });
        }
        state.in_flight += 1;
        drop(state);
        Ok(WorkspaceToolOperationGuard(Arc::clone(&self.state)))
    }

    fn activation_guard(&self) -> WorkspaceToolActivationGuard {
        WorkspaceToolActivationGuard(Arc::clone(&self.state))
    }

    #[cfg(unix)]
    fn open_parent(&self) -> Result<(File, std::ffi::OsString), WorkspaceCoordinatorError> {
        let mut components = self.relative_path.components().peekable();
        let mut directory = self.root.try_clone()?;
        let mut file_name = None;
        while let Some(component) = components.next() {
            let std::path::Component::Normal(value) = component else {
                return Err(std::io::Error::other(
                    "workspace target contains an invalid path component",
                )
                .into());
            };
            if components.peek().is_none() {
                file_name = Some(value.to_os_string());
                break;
            }
            let fd = rustix::fs::openat(
                &directory,
                Path::new(value),
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(workspace_open_error)?;
            directory = File::from(fd);
        }
        let file_name =
            file_name.ok_or_else(|| std::io::Error::other("workspace target has no file name"))?;
        Ok((directory, file_name))
    }
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceFileVersion {
    device: u64,
    inode: u64,
    length: u64,
    modified: Option<std::time::SystemTime>,
    content_hash: [u8; 32],
}

#[cfg(unix)]
fn read_workspace_file_at(
    directory: &File,
    file_name: &OsStr,
) -> Result<(Vec<u8>, WorkspaceFileVersion, u32), WorkspaceCoordinatorError> {
    let file = open_workspace_file_at(directory, file_name)?.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "workspace target does not exist",
        )
    })?;
    let metadata = file.metadata()?;
    validate_workspace_regular_file(&metadata)?;
    if metadata.len() > MAX_WORKSPACE_READ_BYTES {
        return Err(WorkspaceCoordinatorError::WorkspaceReadLimitExceeded {
            limit: MAX_WORKSPACE_READ_BYTES,
        });
    }
    let mut bytes = Vec::new();
    file.take(MAX_WORKSPACE_READ_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_WORKSPACE_READ_BYTES {
        return Err(WorkspaceCoordinatorError::WorkspaceReadLimitExceeded {
            limit: MAX_WORKSPACE_READ_BYTES,
        });
    }
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let version = WorkspaceFileVersion {
        device: metadata.dev(),
        inode: metadata.ino(),
        length: metadata.len(),
        modified: metadata.modified().ok(),
        content_hash: *blake3::hash(&bytes).as_bytes(),
    };
    Ok((bytes, version, metadata.permissions().mode() & 0o777))
}

#[cfg(unix)]
fn open_workspace_file_at(
    directory: &File,
    file_name: &OsStr,
) -> Result<Option<File>, WorkspaceCoordinatorError> {
    match rustix::fs::openat(
        directory,
        Path::new(file_name),
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    ) {
        Ok(fd) => Ok(Some(File::from(fd))),
        Err(rustix::io::Errno::NOENT) => Ok(None),
        Err(error) => Err(workspace_open_error(error)),
    }
}

#[cfg(unix)]
fn workspace_file_mode_at(
    directory: &File,
    file_name: &OsStr,
) -> Result<Option<u32>, WorkspaceCoordinatorError> {
    let Some(file) = open_workspace_file_at(directory, file_name)? else {
        return Ok(None);
    };
    let metadata = file.metadata()?;
    validate_workspace_regular_file(&metadata)?;
    use std::os::unix::fs::PermissionsExt;
    Ok(Some(metadata.permissions().mode() & 0o777))
}

#[cfg(unix)]
fn validate_workspace_regular_file(metadata: &Metadata) -> Result<(), WorkspaceCoordinatorError> {
    if metadata.is_file() {
        Ok(())
    } else {
        Err(std::io::Error::other("workspace target is not a regular file").into())
    }
}

#[cfg(unix)]
fn replace_workspace_file_at(
    directory: &File,
    file_name: &OsStr,
    bytes: &[u8],
    mode: u32,
    expected: Option<&WorkspaceFileVersion>,
) -> Result<(), WorkspaceCoordinatorError> {
    let (temp_name, mut temp_file) = create_workspace_temp_file(directory, file_name, mode)?;
    let result = (|| {
        temp_file.write_all(bytes)?;
        temp_file.sync_all()?;
        drop(temp_file);

        if let Some(expected) = expected {
            let (_, actual, _) = read_workspace_file_at(directory, file_name)?;
            if &actual != expected {
                return Err(std::io::Error::other(
                    "workspace target changed while the edit was being prepared",
                )
                .into());
            }
        }

        rustix::fs::renameat(
            directory,
            Path::new(&temp_name),
            directory,
            Path::new(file_name),
        )
        .map_err(|error| {
            WorkspaceCoordinatorError::Io(std::io::Error::other(format!(
                "workspace atomic replace failed: {error}"
            )))
        })?;
        directory.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = rustix::fs::unlinkat(
            directory,
            Path::new(&temp_name),
            rustix::fs::AtFlags::empty(),
        );
    }
    result
}

#[cfg(unix)]
fn create_workspace_temp_file(
    directory: &File,
    file_name: &OsStr,
    mode: u32,
) -> Result<(OsString, File), WorkspaceCoordinatorError> {
    for _ in 0..64 {
        let sequence = WORKSPACE_TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = OsString::from(format!(
            ".{}.jyowo-write-{}-{sequence}",
            file_name.to_string_lossy(),
            std::process::id()
        ));
        match rustix::fs::openat(
            directory,
            Path::new(&name),
            rustix::fs::OFlags::WRONLY
                | rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::EXCL
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_raw_mode((mode & 0o777) as _),
        ) {
            Ok(fd) => return Ok((name, File::from(fd))),
            Err(rustix::io::Errno::EXIST) => continue,
            Err(error) => return Err(workspace_open_error(error)),
        }
    }
    Err(std::io::Error::other("workspace temporary file name collision limit exceeded").into())
}

#[derive(Debug)]
struct WorkspaceToolAuthorizationState {
    inner: Mutex<WorkspaceToolAuthorizationStateInner>,
    drained: Condvar,
}

#[derive(Debug)]
struct WorkspaceToolAuthorizationStateInner {
    accepting_operations: bool,
    in_flight: usize,
}

struct WorkspaceToolOperationGuard(Arc<WorkspaceToolAuthorizationState>);

impl Drop for WorkspaceToolOperationGuard {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.inner.lock() {
            state.in_flight = state.in_flight.saturating_sub(1);
            if state.in_flight == 0 {
                self.0.drained.notify_all();
            }
        }
    }
}

struct WorkspaceToolActivationGuard(Arc<WorkspaceToolAuthorizationState>);

impl Drop for WorkspaceToolActivationGuard {
    fn drop(&mut self) {
        let Ok(mut state) = self.0.inner.lock() else {
            return;
        };
        state.accepting_operations = false;
        while state.in_flight > 0 {
            let Ok(next) = self.0.drained.wait(state) else {
                return;
            };
            state = next;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceOverrideCommand {
    pub command_id: CommandId,
    pub task_id: TaskId,
    pub expected_stream_version: u64,
    pub lease_id: WorkspaceLeaseId,
    pub path: PathBuf,
    pub reason: String,
    pub authority: EventAuthority,
}

#[derive(Debug, Error)]
pub enum WorkspaceCoordinatorError {
    #[error("workspace path cannot be canonicalized: {path}: {source}")]
    Canonicalize {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("workspace coordinator io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("workspace task store error: {0}")]
    Store(#[from] TaskStoreError),
    #[error("workspace isolation error: {0}")]
    Isolation(#[from] WorkspaceIsolationError),
    #[error("workspace filesystem error: {0}")]
    Filesystem(#[from] harness_fs::FsError),
    #[error("workspace lease {lease_id} is not active")]
    InactiveLease { lease_id: WorkspaceLeaseId },
    #[error("workspace lease {lease_id} does not grant exclusive write access")]
    ExclusiveWriteLeaseRequired { lease_id: WorkspaceLeaseId },
    #[error("workspace lease {lease_id} authorization is read-only")]
    ReadOnlyAuthorization { lease_id: WorkspaceLeaseId },
    #[error("workspace lease {lease_id} tool authorization has expired")]
    ExpiredToolAuthorization { lease_id: WorkspaceLeaseId },
    #[error("workspace read exceeds the {limit} byte memory limit")]
    WorkspaceReadLimitExceeded { limit: u64 },
    #[error("secure workspace file I/O is unavailable on this platform")]
    SecureWorkspaceIoUnavailable,
    #[error("tool path {path} escapes workspace root {root}")]
    PathEscapesWorkspace { path: PathBuf, root: PathBuf },
    #[error("workspace override command was rejected: {message}")]
    OverrideRejected { message: String },
    #[error("managed workspace cleanup is blocked; patch retained at {patch_path}")]
    CleanupBlocked { patch_path: PathBuf },
    #[error("workspace path is not valid UTF-8: {path}")]
    NonUtf8Path { path: PathBuf },
    #[error("commands require an operating-system sandbox bound to the leased workspace")]
    SandboxedCommandRequired,
}

pub struct WorkspaceCoordinator {
    store: Arc<TaskStore>,
    lease_repository: Arc<dyn WorkspaceLeaseRepository>,
    managed_worktrees_root: PathBuf,
    workspace_write_locks: Mutex<HashMap<(WorkspaceLeaseId, PathBuf), Weak<Mutex<()>>>>,
}

impl WorkspaceCoordinator {
    pub fn new(
        store: Arc<TaskStore>,
        managed_worktrees_root: PathBuf,
    ) -> Result<Self, WorkspaceCoordinatorError> {
        std::fs::create_dir_all(&managed_worktrees_root)?;
        set_owner_only_directory(&managed_worktrees_root)?;
        let managed_worktrees_root = canonicalize(&managed_worktrees_root)?;
        let lease_repository: Arc<dyn WorkspaceLeaseRepository> = store.clone();
        let coordinator = Self {
            store,
            lease_repository,
            managed_worktrees_root,
            workspace_write_locks: Mutex::new(HashMap::new()),
        };
        coordinator.expire_stale(Utc::now())?;
        coordinator.reconcile_managed_worktrees()?;
        Ok(coordinator)
    }

    pub fn acquire(
        &self,
        request: WorkspaceLeaseRequest,
    ) -> Result<WorkspaceAcquireOutcome, WorkspaceCoordinatorError> {
        let canonical_root = canonicalize(&request.root)?;
        let mode = request.mode.unwrap_or(match request.execution_kind {
            WorkspaceExecutionKind::Foreground => WorkspaceMode::Current,
            WorkspaceExecutionKind::Background | WorkspaceExecutionKind::ParallelChild => {
                WorkspaceMode::ManagedWorktree
            }
        });
        let lease_id = WorkspaceLeaseId::new();
        let mut branch = None;
        let (worktree_path, baseline_commit, baseline_status) =
            if mode == WorkspaceMode::ManagedWorktree {
                let discovery = GitDiscovery::new(&canonical_root);
                if !discovery.is_git_repository()? {
                    return Err(WorkspaceIsolationError::NonGitWorkspace.into());
                }
                let baseline_commit = discovery.head_commit()?;
                let baseline_status = discovery.status_porcelain()?;
                let path = self.managed_worktrees_root.join(lease_id.to_string());
                let worktree_branch = format!("jyowo/task-{lease_id}");
                branch = Some(worktree_branch);
                (
                    Some(path_to_utf8(&path)?),
                    Some(baseline_commit),
                    baseline_status,
                )
            } else {
                let (commit, status) = current_workspace_baseline(&canonical_root)?;
                (None, commit, status)
            };
        let acquire = AcquireTaskWorkspaceLease {
            lease_id,
            task_id: request.task_id,
            actor_id: request.actor_id,
            mode: mode.clone(),
            canonical_root: path_to_utf8(&canonical_root)?,
            worktree_path: worktree_path.clone(),
            branch: branch.clone(),
            writable: request.access == WorkspaceAccess::Write,
            requested_at: Utc::now(),
            expires_at: request.expires_at,
            baseline_commit,
            baseline_status,
        };
        if mode == WorkspaceMode::ManagedWorktree {
            let lease = self.store.prepare_managed_workspace_lease(acquire)?;
            let path = PathBuf::from(lease.worktree_path.as_deref().ok_or_else(|| {
                TaskStoreError::ProjectionIntegrity("preparing lease has no worktree path".into())
            })?);
            let discovery = GitDiscovery::new(&canonical_root);
            if let Err(error) = discovery.create_worktree(
                &path,
                lease.branch.as_deref().ok_or_else(|| {
                    TaskStoreError::ProjectionIntegrity("preparing lease has no branch".into())
                })?,
                lease.baseline_commit.as_deref().ok_or_else(|| {
                    TaskStoreError::ProjectionIntegrity(
                        "preparing lease has no baseline commit".into(),
                    )
                })?,
            ) {
                if self
                    .store
                    .mark_workspace_cleanup_pending(lease.lease_id)
                    .is_ok()
                    && discovery
                        .discard_partial_worktree(&path, lease.branch.as_deref())
                        .is_ok()
                {
                    let _ = self
                        .store
                        .release_workspace_lease(lease.lease_id, "managed_prepare_failed");
                }
                return Err(error.into());
            }
            return Ok(WorkspaceAcquireOutcome::Acquired(
                self.store
                    .activate_managed_workspace_lease(lease.lease_id)?,
            ));
        }
        let outcome = self.lease_repository.acquire(acquire);
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => return Err(error.into()),
        };
        Ok(match outcome {
            TaskWorkspaceAcquireOutcome::Acquired(lease) => {
                WorkspaceAcquireOutcome::Acquired(lease)
            }
            TaskWorkspaceAcquireOutcome::Waiting(lease) => WorkspaceAcquireOutcome::Waiting(lease),
        })
    }

    pub fn active_for_root(
        &self,
        root: &Path,
    ) -> Result<Vec<TaskWorkspaceLease>, WorkspaceCoordinatorError> {
        let root = canonicalize(root)?;
        Ok(self
            .lease_repository
            .active_for_root(&path_to_utf8(&root)?)?)
    }

    pub fn release(
        &self,
        lease_id: WorkspaceLeaseId,
    ) -> Result<ReleaseTaskWorkspaceLeaseOutcome, WorkspaceCoordinatorError> {
        let lease = self.store.workspace_lease(lease_id)?.ok_or_else(|| {
            TaskStoreError::InvalidInput(format!("workspace lease {lease_id} does not exist"))
        })?;
        if lease.mode == WorkspaceMode::ManagedWorktree {
            return match self.cleanup_managed(lease_id)? {
                WorkspaceCleanupOutcome::Released(outcome) => Ok(outcome),
                WorkspaceCleanupOutcome::CleanupBlocked { patch_path, .. } => {
                    Err(WorkspaceCoordinatorError::CleanupBlocked { patch_path })
                }
            };
        }
        Ok(self.lease_repository.release(lease_id, "owner_released")?)
    }

    pub fn expire_stale(
        &self,
        at: DateTime<Utc>,
    ) -> Result<Vec<ReleaseTaskWorkspaceLeaseOutcome>, WorkspaceCoordinatorError> {
        let outcomes = self.store.expire_workspace_leases(at)?;
        for outcome in &outcomes {
            if outcome.released.mode == WorkspaceMode::ManagedWorktree {
                let _ = self.cleanup_managed(outcome.released.lease_id)?;
            }
        }
        Ok(outcomes)
    }

    pub fn release_task_leases(&self, task_id: TaskId) -> Result<(), WorkspaceCoordinatorError> {
        for lease in self.store.nonterminal_workspace_leases_for_task(task_id)? {
            if lease.state == TaskWorkspaceLeaseState::CleanupBlocked {
                continue;
            }
            match self.release(lease.lease_id) {
                Ok(_) | Err(WorkspaceCoordinatorError::CleanupBlocked { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub fn cleanup_managed(
        &self,
        lease_id: WorkspaceLeaseId,
    ) -> Result<WorkspaceCleanupOutcome, WorkspaceCoordinatorError> {
        let lease = self.store.workspace_lease(lease_id)?.ok_or_else(|| {
            TaskStoreError::InvalidInput(format!("workspace lease {lease_id} does not exist"))
        })?;
        if lease.mode != WorkspaceMode::ManagedWorktree {
            return Err(TaskStoreError::InvalidInput(format!(
                "workspace lease {lease_id} is not managed"
            ))
            .into());
        }
        let worktree = lease.worktree_path.as_deref().ok_or_else(|| {
            TaskStoreError::ProjectionIntegrity(format!(
                "managed workspace lease {lease_id} has no worktree path"
            ))
        })?;
        let worktree = PathBuf::from(worktree);
        let canonical_worktree = harness_fs::resolve_canonical_prefix(&worktree)?;
        if !canonical_worktree.starts_with(&self.managed_worktrees_root) {
            return Err(TaskStoreError::ProjectionIntegrity(format!(
                "managed workspace lease {lease_id} escapes the managed root"
            ))
            .into());
        }
        let lease = self.store.mark_workspace_cleanup_pending(lease_id)?;
        let canonical_root = PathBuf::from(&lease.canonical_root);
        let discovery = GitDiscovery::new(&canonical_root);
        if !worktree.exists() {
            return Ok(WorkspaceCleanupOutcome::Released(
                self.store
                    .release_workspace_lease(lease_id, "managed_cleanup")?,
            ));
        }
        if discovery.is_path_dirty_for_cleanup(&canonical_worktree)? {
            let patch_path = self
                .managed_worktrees_root
                .join(format!("{lease_id}.patch"));
            discovery.write_patch_artifact(&canonical_worktree, &patch_path)?;
            set_owner_only_file(&patch_path)?;
            let lease = self
                .store
                .mark_workspace_cleanup_blocked(lease_id, &path_to_utf8(&patch_path)?)?;
            return Ok(WorkspaceCleanupOutcome::CleanupBlocked { lease, patch_path });
        }
        if canonical_worktree.exists() {
            discovery.remove_worktree(&canonical_worktree, lease.branch.as_deref())?;
        }
        Ok(WorkspaceCleanupOutcome::Released(
            self.store
                .release_workspace_lease(lease_id, "managed_cleanup")?,
        ))
    }

    fn authorize_tool(
        &self,
        lease_id: WorkspaceLeaseId,
        action: WorkspaceToolAction,
        sandboxed_command: bool,
    ) -> Result<WorkspaceToolAuthorization, WorkspaceCoordinatorError> {
        if matches!(action, WorkspaceToolAction::Command { .. }) && !sandboxed_command {
            return Err(WorkspaceCoordinatorError::SandboxedCommandRequired);
        }
        let lease = self.store.workspace_lease(lease_id)?.ok_or_else(|| {
            TaskStoreError::InvalidInput(format!("workspace lease {lease_id} does not exist"))
        })?;
        if lease.state != TaskWorkspaceLeaseState::Active {
            return Err(WorkspaceCoordinatorError::InactiveLease { lease_id });
        }
        let execution_root = match lease.mode {
            WorkspaceMode::Current => PathBuf::from(&lease.canonical_root),
            WorkspaceMode::ManagedWorktree => {
                PathBuf::from(lease.worktree_path.as_deref().ok_or_else(|| {
                    TaskStoreError::ProjectionIntegrity(format!(
                        "managed workspace lease {lease_id} has no worktree path"
                    ))
                })?)
            }
        };
        let stored_execution_root = execution_root;
        let execution_root = harness_fs::resolve_canonical_prefix(&stored_execution_root)?;
        if execution_root != stored_execution_root {
            return Err(WorkspaceCoordinatorError::PathEscapesWorkspace {
                path: execution_root,
                root: stored_execution_root,
            });
        }
        let requested_path = if action.path().is_absolute() {
            action.path().to_path_buf()
        } else {
            execution_root.join(action.path())
        };
        let canonical_path = harness_fs::resolve_canonical_prefix(&requested_path)?;
        if !canonical_path.starts_with(&execution_root) {
            return Err(WorkspaceCoordinatorError::PathEscapesWorkspace {
                path: canonical_path,
                root: execution_root,
            });
        }
        if action.requires_write() {
            let exclusive = match lease.mode {
                WorkspaceMode::ManagedWorktree => lease.writable,
                WorkspaceMode::Current => {
                    lease.writable
                        && self
                            .store
                            .active_workspace_leases(&lease.canonical_root)?
                            .as_slice()
                            == [lease.clone()]
                }
            };
            if !exclusive {
                return Err(WorkspaceCoordinatorError::ExclusiveWriteLeaseRequired { lease_id });
            }
        }
        let write_lock = self.workspace_write_lock(lease_id, &canonical_path)?;
        workspace_authorization(
            lease_id,
            &execution_root,
            &canonical_path,
            action.requires_write(),
            write_lock,
        )
    }

    fn workspace_write_lock(
        &self,
        lease_id: WorkspaceLeaseId,
        path: &Path,
    ) -> Result<Arc<Mutex<()>>, WorkspaceCoordinatorError> {
        let mut locks = self
            .workspace_write_locks
            .lock()
            .map_err(|_| std::io::Error::other("workspace write-lock registry poisoned"))?;
        locks.retain(|_, lock| lock.strong_count() > 0);
        let key = (lease_id, path.to_path_buf());
        if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        let lock = Arc::new(Mutex::new(()));
        locks.insert(key, Arc::downgrade(&lock));
        Ok(lock)
    }

    pub async fn dispatch_tool<T, F>(
        &self,
        lease_id: WorkspaceLeaseId,
        action: WorkspaceToolAction,
        execute: impl FnOnce(WorkspaceToolAuthorization) -> F,
    ) -> Result<T, WorkspaceCoordinatorError>
    where
        F: Future<Output = T>,
    {
        self.authorize_tool(lease_id, action.clone(), false)?;
        let _dispatch_guard = self.store.begin_workspace_dispatch(lease_id)?;
        let authorization = self.authorize_tool(lease_id, action, false)?;
        let _activation_guard = authorization.activation_guard();
        Ok(execute(authorization).await)
    }

    pub async fn dispatch_sandboxed_command<T, F>(
        &self,
        lease_id: WorkspaceLeaseId,
        cwd: PathBuf,
        requires_write: bool,
        isolation: LocalIsolation,
        execute: impl FnOnce(WorkspaceToolAuthorization) -> F,
    ) -> Result<T, WorkspaceCoordinatorError>
    where
        F: Future<Output = T>,
    {
        if !workspace_command_isolation_enforced(isolation) {
            return Err(WorkspaceCoordinatorError::SandboxedCommandRequired);
        }
        let action = WorkspaceToolAction::Command {
            cwd,
            requires_write,
        };
        self.authorize_tool(lease_id, action.clone(), true)?;
        let _dispatch_guard = self.store.begin_workspace_dispatch(lease_id)?;
        let authorization = self.authorize_tool(lease_id, action, true)?;
        let _activation_guard = authorization.activation_guard();
        Ok(execute(authorization).await)
    }

    pub async fn dispatch_override<T, F>(
        &self,
        command: WorkspaceOverrideCommand,
        execute: impl FnOnce(WorkspaceToolAuthorization) -> F,
    ) -> Result<T, WorkspaceCoordinatorError>
    where
        F: Future<Output = T>,
    {
        if command.reason.trim().is_empty() {
            return Err(TaskStoreError::InvalidInput(
                "workspace override reason must not be empty".into(),
            )
            .into());
        }
        let lease = self
            .store
            .workspace_lease(command.lease_id)?
            .ok_or_else(|| {
                TaskStoreError::InvalidInput(format!(
                    "workspace lease {} does not exist",
                    command.lease_id
                ))
            })?;
        if lease.task_id != command.task_id {
            return Err(TaskStoreError::InvalidInput(
                "workspace override task does not own the lease".into(),
            )
            .into());
        }
        if lease.state != TaskWorkspaceLeaseState::Active {
            return Err(WorkspaceCoordinatorError::InactiveLease {
                lease_id: command.lease_id,
            });
        }
        if lease.mode != WorkspaceMode::Current {
            return Err(TaskStoreError::InvalidInput(
                "workspace override applies only to current workspace leases".into(),
            )
            .into());
        }
        let stored_root = PathBuf::from(&lease.canonical_root);
        let root = harness_fs::resolve_canonical_prefix(&stored_root)?;
        if root != stored_root {
            return Err(WorkspaceCoordinatorError::PathEscapesWorkspace {
                path: root,
                root: stored_root,
            });
        }
        let requested = if command.path.is_absolute() {
            command.path.clone()
        } else {
            root.join(&command.path)
        };
        let canonical_path = harness_fs::resolve_canonical_prefix(&requested)?;
        if !canonical_path.starts_with(&root) {
            return Err(WorkspaceCoordinatorError::PathEscapesWorkspace {
                path: canonical_path,
                root,
            });
        }
        let event_path = path_to_utf8(&canonical_path)?;
        let reason = command.reason.clone();
        let outcome = self.store.transact_command(
            AcceptedCommand {
                command_id: command.command_id,
                task_id: command.task_id,
                idempotency_key: format!("workspace-override-{}", command.command_id),
                expected_stream_version: command.expected_stream_version,
                authority: command.authority,
                payload: json!({
                    "type": "workspace_override",
                    "leaseId": command.lease_id,
                    "canonicalPath": event_path,
                    "reason": reason,
                }),
            },
            |_| {
                Ok(vec![
                    harness_journal::NewTaskEvent::workspace_override_applied(
                        command.command_id,
                        command.lease_id,
                        event_path,
                        reason,
                        Utc::now(),
                    ),
                ])
            },
        )?;
        if let CommandOutcome::Rejected { rejection, .. } = outcome {
            return Err(WorkspaceCoordinatorError::OverrideRejected {
                message: format!("{rejection:?}"),
            });
        }
        let _dispatch_guard = self.store.begin_workspace_dispatch(command.lease_id)?;
        let current = self
            .store
            .workspace_lease(command.lease_id)?
            .ok_or_else(|| {
                TaskStoreError::InvalidInput(format!(
                    "workspace lease {} does not exist",
                    command.lease_id
                ))
            })?;
        if current.state != TaskWorkspaceLeaseState::Active || current.task_id != command.task_id {
            return Err(WorkspaceCoordinatorError::InactiveLease {
                lease_id: command.lease_id,
            });
        }
        let write_lock = self.workspace_write_lock(command.lease_id, &canonical_path)?;
        let authorization =
            workspace_authorization(command.lease_id, &root, &canonical_path, true, write_lock)?;
        let _activation_guard = authorization.activation_guard();
        Ok(execute(authorization).await)
    }

    fn reconcile_managed_worktrees(&self) -> Result<(), WorkspaceCoordinatorError> {
        for lease in self.store.recoverable_managed_workspace_leases()? {
            let root = PathBuf::from(&lease.canonical_root);
            let path = PathBuf::from(lease.worktree_path.as_deref().ok_or_else(|| {
                TaskStoreError::ProjectionIntegrity(format!(
                    "managed lease {} has no worktree path",
                    lease.lease_id
                ))
            })?);
            let resolved_path = harness_fs::resolve_canonical_prefix(&path)?;
            if !resolved_path.starts_with(&self.managed_worktrees_root) {
                return Err(TaskStoreError::ProjectionIntegrity(format!(
                    "managed lease {} escapes managed worktree root",
                    lease.lease_id
                ))
                .into());
            }
            let discovery = GitDiscovery::new(&root);
            match lease.state {
                TaskWorkspaceLeaseState::Preparing => {
                    let branch = lease.branch.as_deref().ok_or_else(|| {
                        TaskStoreError::ProjectionIntegrity("preparing lease has no branch".into())
                    })?;
                    let baseline = lease.baseline_commit.as_deref().ok_or_else(|| {
                        TaskStoreError::ProjectionIntegrity(
                            "preparing lease has no baseline commit".into(),
                        )
                    })?;
                    if path.exists()
                        && !discovery.validate_registered_worktree(&path, branch, baseline)?
                    {
                        discovery.discard_partial_worktree(&path, Some(branch))?;
                    }
                    if !path.exists() {
                        discovery.create_worktree(&path, branch, baseline)?;
                    }
                    self.store
                        .activate_managed_workspace_lease(lease.lease_id)?;
                }
                TaskWorkspaceLeaseState::Expired | TaskWorkspaceLeaseState::CleanupPending => {
                    let _ = self.cleanup_managed(lease.lease_id)?;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

pub(crate) fn workspace_command_isolation_enforced(isolation: LocalIsolation) -> bool {
    matches!(
        isolation,
        LocalIsolation::Bubblewrap | LocalIsolation::Seatbelt
    )
}

fn current_workspace_baseline(
    root: &Path,
) -> Result<(Option<String>, String), WorkspaceCoordinatorError> {
    let discovery = GitDiscovery::new(root);
    if discovery.is_git_repository()? {
        Ok((
            Some(discovery.head_commit()?),
            discovery.status_porcelain()?,
        ))
    } else {
        Ok((None, String::new()))
    }
}

fn workspace_authorization(
    lease_id: WorkspaceLeaseId,
    root: &Path,
    canonical_path: &Path,
    writable: bool,
    write_lock: Arc<Mutex<()>>,
) -> Result<WorkspaceToolAuthorization, WorkspaceCoordinatorError> {
    let relative_path = canonical_path
        .strip_prefix(root)
        .map_err(|_| WorkspaceCoordinatorError::PathEscapesWorkspace {
            path: canonical_path.to_path_buf(),
            root: root.to_path_buf(),
        })?
        .to_path_buf();
    Ok(WorkspaceToolAuthorization {
        lease_id,
        writable,
        root: Arc::new(open_directory_no_follow(root)?),
        relative_path,
        write_lock,
        state: Arc::new(WorkspaceToolAuthorizationState {
            inner: Mutex::new(WorkspaceToolAuthorizationStateInner {
                accepting_operations: true,
                in_flight: 0,
            }),
            drained: Condvar::new(),
        }),
    })
}

#[cfg(unix)]
fn open_directory_no_follow(path: &Path) -> Result<File, WorkspaceCoordinatorError> {
    if !path.is_absolute() {
        return Err(std::io::Error::other("workspace root must be absolute").into());
    }
    let mut directory = File::open(Path::new("/"))?;
    for component in path.components() {
        match component {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(value) => {
                let fd = rustix::fs::openat(
                    &directory,
                    Path::new(value),
                    rustix::fs::OFlags::RDONLY
                        | rustix::fs::OFlags::DIRECTORY
                        | rustix::fs::OFlags::NOFOLLOW
                        | rustix::fs::OFlags::CLOEXEC,
                    rustix::fs::Mode::empty(),
                )
                .map_err(workspace_open_error)?;
                directory = File::from(fd);
            }
            _ => {
                return Err(
                    std::io::Error::other("workspace root contains an invalid component").into(),
                );
            }
        }
    }
    Ok(directory)
}

#[cfg(not(unix))]
fn open_directory_no_follow(_path: &Path) -> Result<File, WorkspaceCoordinatorError> {
    Err(WorkspaceCoordinatorError::SecureWorkspaceIoUnavailable)
}

#[cfg(unix)]
fn workspace_open_error(error: rustix::io::Errno) -> WorkspaceCoordinatorError {
    if error == rustix::io::Errno::LOOP || error == rustix::io::Errno::NOTDIR {
        std::io::Error::other("workspace path must not traverse symbolic links").into()
    } else {
        std::io::Error::other(format!("workspace path open failed: {error}")).into()
    }
}

fn path_to_utf8(path: &Path) -> Result<String, WorkspaceCoordinatorError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| WorkspaceCoordinatorError::NonUtf8Path {
            path: path.to_path_buf(),
        })
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

fn canonicalize(path: &Path) -> Result<PathBuf, WorkspaceCoordinatorError> {
    path.canonicalize()
        .map_err(|source| WorkspaceCoordinatorError::Canonicalize {
            path: path.to_path_buf(),
            source,
        })
}

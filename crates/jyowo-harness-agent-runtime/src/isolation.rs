use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use harness_contracts::{AgentWorkspaceIsolationMode, RunId};
use thiserror::Error;

use crate::store::{AgentRuntimeStore, AgentRuntimeStoreError, WorkspaceIsolationLease};

pub const AGENT_WORKTREES_DIR_NAME: &str = "agent-worktrees";

const LEASE_STATUS_ACTIVE: &str = "active";
const LEASE_STATUS_RELEASED: &str = "released";
const LEASE_STATUS_CLEANUP_BLOCKED: &str = "cleanup_blocked";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateWorkspaceIsolationLeaseRequest {
    pub conversation_id: String,
    pub run_id: String,
    pub agent_id: String,
    pub mode: AgentWorkspaceIsolationMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceIsolationCleanupResult {
    Released,
    CleanupBlocked { patch_path: PathBuf },
}

#[derive(Debug, Error)]
pub enum WorkspaceIsolationError {
    #[error("workspace isolation store error: {0}")]
    Store(#[from] AgentRuntimeStoreError),
    #[error("workspace isolation git error: {0}")]
    Git(String),
    #[error("workspace is not a git repository")]
    NonGitWorkspace,
    #[error("workspace worktree is dirty; git worktree isolation requires a clean base commit")]
    DirtyWorkspace,
    #[error("branch {branch} is already leased by another active write-capable agent")]
    DuplicateBranchLease { branch: String },
    #[error("workspace isolation is unavailable: {message}")]
    Unavailable { message: String },
    #[error("workspace isolation lease not found: {lease_id}")]
    LeaseNotFound { lease_id: String },
}

pub struct WorkspaceIsolationManager {
    workspace_root: PathBuf,
    store: AgentRuntimeStore,
}

impl WorkspaceIsolationManager {
    pub fn open(workspace_root: impl AsRef<Path>) -> Result<Self, WorkspaceIsolationError> {
        let workspace_root = workspace_root.as_ref().to_path_buf();
        let store = AgentRuntimeStore::open(&workspace_root)?;
        std::fs::create_dir_all(Self::worktrees_dir(&workspace_root)).map_err(|error| {
            WorkspaceIsolationError::Unavailable {
                message: format!("failed to create agent worktrees directory: {error}"),
            }
        })?;
        Ok(Self {
            workspace_root,
            store,
        })
    }

    #[must_use]
    pub fn worktrees_dir(workspace_root: &Path) -> PathBuf {
        workspace_root
            .join(".jyowo")
            .join("runtime")
            .join(AGENT_WORKTREES_DIR_NAME)
    }

    #[must_use]
    pub fn worktrees_dir_for_workspace(&self) -> PathBuf {
        Self::worktrees_dir(&self.workspace_root)
    }

    pub fn git_discovery(&self) -> GitDiscovery<'_> {
        GitDiscovery::new(&self.workspace_root)
    }

    pub fn is_available_for_mode(&self, mode: AgentWorkspaceIsolationMode) -> bool {
        match mode {
            AgentWorkspaceIsolationMode::ReadOnly => true,
            AgentWorkspaceIsolationMode::PatchOnly => true,
            AgentWorkspaceIsolationMode::GitWorktree => git_binary_available(),
        }
    }

    pub fn validate_write_mode(
        &self,
        mode: AgentWorkspaceIsolationMode,
    ) -> Result<(), WorkspaceIsolationError> {
        if !self.is_available_for_mode(mode) {
            return Err(WorkspaceIsolationError::Unavailable {
                message: "git is unavailable for git worktree isolation".to_owned(),
            });
        }

        if mode == AgentWorkspaceIsolationMode::GitWorktree {
            let discovery = self.git_discovery();
            if !discovery.is_git_repository()? {
                return Err(WorkspaceIsolationError::NonGitWorkspace);
            }
            if discovery.is_worktree_dirty()? {
                return Err(WorkspaceIsolationError::DirtyWorkspace);
            }
        }

        Ok(())
    }

    pub fn create_lease(
        &self,
        request: CreateWorkspaceIsolationLeaseRequest,
    ) -> Result<WorkspaceIsolationLease, WorkspaceIsolationError> {
        self.validate_write_mode(request.mode)?;

        if request.agent_id.trim().is_empty() {
            return Err(WorkspaceIsolationError::Unavailable {
                message: "agent id is required for workspace isolation lease".to_owned(),
            });
        }

        let lease_id = RunId::new().to_string();
        let now = Utc::now().to_rfc3339();
        let worktree_path = self.worktrees_dir_for_workspace().join(&lease_id);

        let (branch, base_commit) = match request.mode {
            AgentWorkspaceIsolationMode::ReadOnly => (None, None),
            AgentWorkspaceIsolationMode::PatchOnly => {
                std::fs::create_dir_all(&worktree_path).map_err(|error| {
                    WorkspaceIsolationError::Unavailable {
                        message: format!("failed to create patch workspace: {error}"),
                    }
                })?;
                (None, None)
            }
            AgentWorkspaceIsolationMode::GitWorktree => {
                let discovery = self.git_discovery();
                let base_commit = discovery.head_commit()?;
                let branch = format!("jyowo/agent-{}", request.agent_id);
                if self
                    .store
                    .find_active_workspace_isolation_lease_by_branch(&branch)?
                    .is_some()
                {
                    return Err(WorkspaceIsolationError::DuplicateBranchLease { branch });
                }
                discovery.create_worktree(&worktree_path, &branch, &base_commit)?;
                (Some(branch), Some(base_commit))
            }
        };

        let lease = WorkspaceIsolationLease {
            lease_id: lease_id.clone(),
            conversation_id: request.conversation_id,
            run_id: request.run_id,
            agent_id: request.agent_id,
            path: worktree_path.to_string_lossy().into_owned(),
            branch,
            base_commit,
            status: LEASE_STATUS_ACTIVE.to_owned(),
            created_at: now.clone(),
            updated_at: now,
        };
        self.store.insert_workspace_isolation_lease(&lease)?;
        Ok(lease)
    }

    pub fn get_lease(
        &self,
        lease_id: &str,
    ) -> Result<Option<WorkspaceIsolationLease>, WorkspaceIsolationError> {
        Ok(self.store.get_workspace_isolation_lease(lease_id)?)
    }

    pub fn list_active_leases(
        &self,
    ) -> Result<Vec<WorkspaceIsolationLease>, WorkspaceIsolationError> {
        Ok(self.store.list_active_workspace_isolation_leases()?)
    }

    pub fn cleanup_lease(
        &self,
        lease_id: &str,
    ) -> Result<WorkspaceIsolationCleanupResult, WorkspaceIsolationError> {
        let Some(lease) = self.store.get_workspace_isolation_lease(lease_id)? else {
            return Err(WorkspaceIsolationError::LeaseNotFound {
                lease_id: lease_id.to_owned(),
            });
        };

        let worktree_path = PathBuf::from(&lease.path);
        let dirty = if worktree_path.exists() {
            if lease.branch.is_some() {
                self.git_discovery()
                    .is_path_dirty_for_cleanup(&worktree_path)?
            } else {
                directory_has_changes(&worktree_path)?
            }
        } else {
            false
        };

        if dirty {
            let patch_path = self
                .worktrees_dir_for_workspace()
                .join(format!("{lease_id}.patch"));
            if lease.branch.is_some() {
                self.git_discovery()
                    .write_patch_artifact(&worktree_path, &patch_path)?;
            } else {
                write_directory_patch_marker(&worktree_path, &patch_path)?;
            }
            let now = Utc::now().to_rfc3339();
            self.store.update_workspace_isolation_lease_status(
                lease_id,
                LEASE_STATUS_CLEANUP_BLOCKED,
                &now,
            )?;
            return Ok(WorkspaceIsolationCleanupResult::CleanupBlocked { patch_path });
        }

        if lease.branch.is_some() && worktree_path.exists() {
            self.git_discovery()
                .remove_worktree(&worktree_path, lease.branch.as_deref())?;
        } else if worktree_path.exists() {
            std::fs::remove_dir_all(&worktree_path).map_err(|error| {
                WorkspaceIsolationError::Unavailable {
                    message: format!("failed to remove patch workspace: {error}"),
                }
            })?;
        }

        let now = Utc::now().to_rfc3339();
        self.store.update_workspace_isolation_lease_status(
            lease_id,
            LEASE_STATUS_RELEASED,
            &now,
        )?;
        Ok(WorkspaceIsolationCleanupResult::Released)
    }
}

pub struct GitDiscovery<'a> {
    workspace_root: &'a Path,
}

impl<'a> GitDiscovery<'a> {
    pub fn new(workspace_root: &'a Path) -> Self {
        Self { workspace_root }
    }

    pub fn is_git_repository(&self) -> Result<bool, WorkspaceIsolationError> {
        match run_git(self.workspace_root, ["rev-parse", "--is-inside-work-tree"]) {
            Ok(output) => Ok(output.trim() == "true"),
            Err(WorkspaceIsolationError::Git(_)) => Ok(false),
            Err(other) => Err(other),
        }
    }

    pub fn head_commit(&self) -> Result<String, WorkspaceIsolationError> {
        Ok(run_git(self.workspace_root, ["rev-parse", "HEAD"])?
            .trim()
            .to_owned())
    }

    pub fn current_branch(&self) -> Result<String, WorkspaceIsolationError> {
        Ok(
            run_git(self.workspace_root, ["rev-parse", "--abbrev-ref", "HEAD"])?
                .trim()
                .to_owned(),
        )
    }

    pub fn is_worktree_dirty(&self) -> Result<bool, WorkspaceIsolationError> {
        self.is_tracked_dirty(self.workspace_root)
    }

    fn is_tracked_dirty(&self, path: &Path) -> Result<bool, WorkspaceIsolationError> {
        let output = run_git_in(path, ["status", "--porcelain"])?;
        Ok(output.lines().any(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !line.starts_with("??")
        }))
    }

    pub fn is_path_dirty_for_cleanup(&self, path: &Path) -> Result<bool, WorkspaceIsolationError> {
        let output = run_git_in(path, ["status", "--porcelain"])?;
        Ok(output.lines().any(|line| !line.trim().is_empty()))
    }

    pub fn is_path_dirty(
        &self,
        path: &Path,
        branch: Option<&str>,
    ) -> Result<bool, WorkspaceIsolationError> {
        let mut args = vec!["status", "--porcelain"];
        if branch.is_some() {
            args.push("-uno");
        }
        let output = run_git_in(path, args)?;
        Ok(output.lines().any(|line| !line.trim().is_empty()))
    }

    pub fn create_worktree(
        &self,
        path: &Path,
        branch: &str,
        base_commit: &str,
    ) -> Result<(), WorkspaceIsolationError> {
        if path.exists() {
            return Err(WorkspaceIsolationError::Unavailable {
                message: format!("worktree path already exists: {}", path.display()),
            });
        }
        run_git(
            self.workspace_root,
            [
                "worktree",
                "add",
                "-b",
                branch,
                &path.to_string_lossy(),
                base_commit,
            ],
        )?;
        Ok(())
    }

    pub fn remove_worktree(
        &self,
        path: &Path,
        branch: Option<&str>,
    ) -> Result<(), WorkspaceIsolationError> {
        run_git(
            self.workspace_root,
            ["worktree", "remove", "--force", &path.to_string_lossy()],
        )?;
        if let Some(branch) = branch {
            let _ = run_git(self.workspace_root, ["branch", "-D", branch]);
        }
        Ok(())
    }

    pub fn write_patch_artifact(
        &self,
        path: &Path,
        patch_path: &Path,
    ) -> Result<(), WorkspaceIsolationError> {
        let patch = run_git_in(path, ["diff", "HEAD"])?;
        std::fs::write(patch_path, patch).map_err(|error| WorkspaceIsolationError::Unavailable {
            message: format!("failed to write patch artifact: {error}"),
        })
    }
}

fn git_binary_available() -> bool {
    Command::new("git")
        .arg("--version")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn run_git(
    workspace_root: &Path,
    args: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<String, WorkspaceIsolationError> {
    run_git_in(workspace_root, args)
}

fn run_git_in(
    cwd: &Path,
    args: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<String, WorkspaceIsolationError> {
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_owned())
        .collect();
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(&args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never")
        .output()
        .map_err(|error| WorkspaceIsolationError::Git(error.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(WorkspaceIsolationError::Git(detail));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn directory_has_changes(path: &Path) -> Result<bool, WorkspaceIsolationError> {
    if !path.is_dir() {
        return Ok(false);
    }
    let mut entries =
        std::fs::read_dir(path).map_err(|error| WorkspaceIsolationError::Unavailable {
            message: format!("failed to inspect patch workspace: {error}"),
        })?;
    Ok(entries.next().is_some())
}

fn write_directory_patch_marker(
    path: &Path,
    patch_path: &Path,
) -> Result<(), WorkspaceIsolationError> {
    let marker = format!(
        "patch-only workspace contained uncommitted files at {}\n",
        path.display()
    );
    std::fs::write(patch_path, marker).map_err(|error| WorkspaceIsolationError::Unavailable {
        message: format!("failed to write patch artifact: {error}"),
    })
}

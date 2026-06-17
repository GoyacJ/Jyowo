use std::path::{Path, PathBuf};

use harness_contracts::{SessionId, TenantId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPaths {
    pub workspace_root: PathBuf,
    pub events: PathBuf,
    pub blobs: PathBuf,
    pub db: PathBuf,
    pub memdir: PathBuf,
    pub runtime_sessions: PathBuf,
}

impl SessionPaths {
    pub fn from_workspace(root: impl AsRef<Path>, tenant: TenantId, session: SessionId) -> Self {
        let root = root.as_ref().to_path_buf();
        Self {
            events: root
                .join("runtime")
                .join("events")
                .join(tenant.to_string())
                .join(format!("{session}.jsonl")),
            blobs: root.join("data").join("blobs"),
            db: root.join("data").join("main.db"),
            memdir: root.join("data").join("memdir"),
            runtime_sessions: root.join("runtime").join("sessions"),
            workspace_root: root,
        }
    }
}

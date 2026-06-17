use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use harness_contracts::{TenantId, WorkspaceId};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::SessionOptions;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapFileSpec {
    pub relative_path: PathBuf,
    #[serde(default)]
    pub required: bool,
}

impl BootstrapFileSpec {
    #[must_use]
    pub fn optional(relative_path: impl Into<PathBuf>) -> Self {
        Self {
            relative_path: relative_path.into(),
            required: false,
        }
    }

    #[must_use]
    pub fn required(relative_path: impl Into<PathBuf>) -> Self {
        Self {
            relative_path: relative_path.into(),
            required: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceBootstrap {
    pub workspace_root: PathBuf,
    #[serde(default = "WorkspaceBootstrap::default_files")]
    pub files: Vec<BootstrapFileSpec>,
    #[serde(default)]
    pub system_prompt_addendum: Option<String>,
}

impl WorkspaceBootstrap {
    #[must_use]
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            files: Self::default_files(),
            system_prompt_addendum: None,
        }
    }

    #[must_use]
    pub fn with_files(mut self, files: Vec<BootstrapFileSpec>) -> Self {
        self.files = files;
        self
    }

    #[must_use]
    pub fn with_system_prompt_addendum(mut self, addendum: impl Into<String>) -> Self {
        self.system_prompt_addendum = Some(addendum.into());
        self
    }

    #[must_use]
    pub fn default_files() -> Vec<BootstrapFileSpec> {
        vec![
            BootstrapFileSpec::optional("AGENTS.md"),
            BootstrapFileSpec::optional(".jyowo/AGENTS.md"),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSpec {
    #[serde(default)]
    pub id: Option<WorkspaceId>,
    #[serde(default = "default_tenant_id")]
    pub tenant_id: TenantId,
    pub root_path: PathBuf,
    pub display_name: String,
    #[serde(default = "WorkspaceBootstrap::default_files")]
    pub bootstrap_files: Vec<BootstrapFileSpec>,
    #[serde(default)]
    pub default_session_options: Option<SessionOptions>,
}

impl WorkspaceSpec {
    #[must_use]
    pub fn new(root_path: impl Into<PathBuf>, display_name: impl Into<String>) -> Self {
        Self {
            id: None,
            tenant_id: TenantId::SINGLE,
            root_path: root_path.into(),
            display_name: display_name.into(),
            bootstrap_files: WorkspaceBootstrap::default_files(),
            default_session_options: None,
        }
    }

    #[must_use]
    pub fn with_id(mut self, id: WorkspaceId) -> Self {
        self.id = Some(id);
        self
    }

    #[must_use]
    pub fn with_tenant_id(mut self, tenant_id: TenantId) -> Self {
        self.tenant_id = tenant_id;
        self
    }

    #[must_use]
    pub fn with_bootstrap_files(mut self, files: Vec<BootstrapFileSpec>) -> Self {
        self.bootstrap_files = files;
        self
    }

    #[must_use]
    pub fn with_default_session_options(mut self, options: SessionOptions) -> Self {
        self.default_session_options = Some(options);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub tenant_id: TenantId,
    pub root_path: PathBuf,
    pub display_name: String,
    pub bootstrap_files: Vec<BootstrapFileSpec>,
    pub default_session_options: Option<SessionOptions>,
    pub created_at: DateTime<Utc>,
}

impl Workspace {
    #[must_use]
    pub fn bootstrap(&self) -> WorkspaceBootstrap {
        WorkspaceBootstrap {
            workspace_root: self.root_path.clone(),
            files: self.bootstrap_files.clone(),
            system_prompt_addendum: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct WorkspaceRegistry {
    workspaces: RwLock<HashMap<WorkspaceId, Workspace>>,
}

impl WorkspaceRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&self, spec: WorkspaceSpec) -> Workspace {
        let workspace = Workspace {
            id: spec.id.unwrap_or_else(WorkspaceId::new),
            tenant_id: spec.tenant_id,
            root_path: spec.root_path,
            display_name: spec.display_name,
            bootstrap_files: spec.bootstrap_files,
            default_session_options: spec.default_session_options,
            created_at: Utc::now(),
        };
        self.workspaces
            .write()
            .insert(workspace.id, workspace.clone());
        workspace
    }

    #[must_use]
    pub fn get(&self, id: WorkspaceId) -> Option<Workspace> {
        self.workspaces.read().get(&id).cloned()
    }

    #[must_use]
    pub fn list(&self, tenant_id: TenantId) -> Vec<Workspace> {
        let mut workspaces: Vec<_> = self
            .workspaces
            .read()
            .values()
            .filter(|workspace| workspace.tenant_id == tenant_id)
            .cloned()
            .collect();
        workspaces.sort_by_key(|workspace| workspace.created_at);
        workspaces
    }
}

fn default_tenant_id() -> TenantId {
    TenantId::SINGLE
}

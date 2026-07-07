use std::path::PathBuf;

use serde::Serialize;

use super::error::CommandErrorPayload;
use super::runtime::{
    canonical_workspace_root, runtime_state_for_no_workspace, runtime_state_for_workspace,
    ManagedDesktopRuntime,
};
use crate::project_registry::{ProjectRecord, ProjectRegistry};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListProjectsResponse {
    pub projects: Vec<ProjectRecord>,
    pub active_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchProjectResponse {
    pub project: ProjectRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProjectResponse {
    pub path: String,
    pub active_path: Option<String>,
    pub status: &'static str,
}

#[must_use]
pub fn list_projects_payload(project_registry: &ProjectRegistry) -> ListProjectsResponse {
    ListProjectsResponse {
        projects: project_registry.list_projects(),
        active_path: project_registry.active_path(),
    }
}

pub async fn switch_project_payload(
    path: String,
    runtime_handle: &ManagedDesktopRuntime,
    project_registry: &ProjectRegistry,
) -> Result<SwitchProjectResponse, CommandErrorPayload> {
    let workspace_root = canonical_workspace_root(PathBuf::from(path), "project path".to_owned())?;
    let project = project_registry.set_active(&workspace_root)?;
    let new_runtime = runtime_state_for_workspace(workspace_root).await?;
    *runtime_handle.write().await = new_runtime;
    Ok(SwitchProjectResponse { project })
}

pub async fn delete_project_payload(
    path: String,
    runtime_handle: &ManagedDesktopRuntime,
    project_registry: &ProjectRegistry,
) -> Result<DeleteProjectResponse, CommandErrorPayload> {
    if path.trim().is_empty() {
        return Err(CommandErrorPayload {
            code: "INVALID_PAYLOAD",
            message: "project path is required".to_owned(),
        });
    }

    let removed = project_registry.remove(&PathBuf::from(path))?;
    let active_path = project_registry.active_path();
    if active_path.is_none() {
        *runtime_handle.write().await = runtime_state_for_no_workspace().await?;
    }

    Ok(DeleteProjectResponse {
        path: removed.path,
        active_path,
        status: "deleted",
    })
}

pub async fn add_project_payload(
    path: String,
    runtime_handle: &ManagedDesktopRuntime,
    project_registry: &ProjectRegistry,
) -> Result<SwitchProjectResponse, CommandErrorPayload> {
    let workspace_root = canonical_workspace_root(PathBuf::from(path), "project path".to_owned())?;
    let project = project_registry.upsert_and_activate(&workspace_root)?;
    let new_runtime = runtime_state_for_workspace(workspace_root).await?;
    *runtime_handle.write().await = new_runtime;
    Ok(SwitchProjectResponse { project })
}

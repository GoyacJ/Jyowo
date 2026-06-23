use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::commands::CommandErrorPayload;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRecord {
    pub path: String,
    pub name: String,
    pub last_opened_at: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectRegistryFile {
    #[serde(default)]
    projects: Vec<ProjectRecord>,
    active_path: Option<String>,
}

#[derive(Clone)]
pub struct ProjectRegistry {
    path: PathBuf,
    data: Arc<Mutex<ProjectRegistryFile>>,
}

impl ProjectRegistry {
    pub fn load() -> Result<Self, CommandErrorPayload> {
        let path = registry_file_path()?;
        let data = if path.exists() {
            let contents = fs::read_to_string(&path).map_err(|error| {
                registry_io_failed(format!("failed to read project registry: {error}"))
            })?;
            serde_json::from_str(&contents).map_err(|error| {
                registry_io_failed(format!("failed to parse project registry: {error}"))
            })?
        } else {
            ProjectRegistryFile::default()
        };

        Ok(Self {
            path,
            data: Arc::new(Mutex::new(data)),
        })
    }

    #[must_use]
    pub fn list_projects(&self) -> Vec<ProjectRecord> {
        let data = self
            .data
            .lock()
            .expect("project registry lock should not be poisoned");
        let mut projects = data.projects.clone();
        projects.sort_by(|left, right| right.last_opened_at.cmp(&left.last_opened_at));
        projects
    }

    #[must_use]
    pub fn active_path(&self) -> Option<String> {
        self.data
            .lock()
            .expect("project registry lock should not be poisoned")
            .active_path
            .clone()
    }

    pub fn upsert_and_activate(
        &self,
        workspace_root: &Path,
    ) -> Result<ProjectRecord, CommandErrorPayload> {
        let path = workspace_root.to_string_lossy().into_owned();
        let name = workspace_project_name(workspace_root);
        let last_opened_at = chrono::Utc::now().to_rfc3339();
        let record = ProjectRecord {
            path: path.clone(),
            name,
            last_opened_at,
        };

        {
            let mut data = self
                .data
                .lock()
                .expect("project registry lock should not be poisoned");
            if let Some(existing) = data
                .projects
                .iter_mut()
                .find(|project| project.path == path)
            {
                existing.name = record.name.clone();
                existing.last_opened_at = record.last_opened_at.clone();
            } else {
                data.projects.push(record.clone());
            }
            data.active_path = Some(path);
        }

        self.persist()?;
        Ok(record)
    }

    pub fn set_active(&self, workspace_root: &Path) -> Result<ProjectRecord, CommandErrorPayload> {
        let path = workspace_root.to_string_lossy().into_owned();
        let record = {
            let mut data = self
                .data
                .lock()
                .expect("project registry lock should not be poisoned");
            let index = data
                .projects
                .iter()
                .position(|project| project.path == path)
                .ok_or_else(|| registry_not_found(path.clone()))?;
            data.projects[index].last_opened_at = chrono::Utc::now().to_rfc3339();
            data.active_path = Some(path);
            data.projects[index].clone()
        };

        self.persist()?;
        Ok(record)
    }

    #[must_use]
    pub fn has_active_project(&self) -> bool {
        self.active_path().is_some()
    }

    fn persist(&self) -> Result<(), CommandErrorPayload> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                registry_io_failed(format!(
                    "failed to create project registry directory: {error}"
                ))
            })?;
        }

        let data = self
            .data
            .lock()
            .expect("project registry lock should not be poisoned");
        let serialized = serde_json::to_string_pretty(&*data).map_err(|error| {
            registry_io_failed(format!("failed to serialize project registry: {error}"))
        })?;
        fs::write(&self.path, serialized).map_err(|error| {
            registry_io_failed(format!("failed to write project registry: {error}"))
        })
    }
}

pub fn registry_file_path() -> Result<PathBuf, CommandErrorPayload> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| registry_io_failed("home directory is unavailable".to_owned()))?;
    Ok(PathBuf::from(home).join(".jyowo").join("projects.json"))
}

pub fn unconfigured_workspace_root() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".jyowo")
        .join("unconfigured")
}

fn workspace_project_name(workspace_root: &Path) -> String {
    workspace_root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("Project")
        .to_owned()
}

fn registry_io_failed(message: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "INVALID_PAYLOAD",
        message,
    }
}

fn registry_not_found(path: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "INVALID_PAYLOAD",
        message: format!("project is not registered: {path}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn upsert_and_activate_persists_project_record() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let temp_dir = env::temp_dir().join(format!("jyowo-project-registry-{suffix}"));
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let registry_path = temp_dir.join("projects.json");
        let registry = ProjectRegistry {
            path: registry_path.clone(),
            data: Arc::new(Mutex::new(ProjectRegistryFile::default())),
        };

        let record = registry
            .upsert_and_activate(&temp_dir)
            .expect("project should be registered");

        assert_eq!(record.path, temp_dir.to_string_lossy());
        assert_eq!(
            registry.active_path(),
            Some(temp_dir.to_string_lossy().into_owned())
        );
        assert!(registry_path.exists());

        let _ = fs::remove_dir_all(temp_dir);
    }
}

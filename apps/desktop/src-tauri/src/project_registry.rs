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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectMoveDirection {
    Up,
    Down,
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
        let data = crate::commands::stores::read_json_file::<ProjectRegistryFile>(
            &path,
            "project registry",
        )?
        .unwrap_or_default();

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
        data.projects.clone()
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

    pub fn remove(&self, workspace_root: &Path) -> Result<ProjectRecord, CommandErrorPayload> {
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
            let record = data.projects.remove(index);
            if data.active_path.as_deref() == Some(record.path.as_str()) {
                data.active_path = None;
            }
            record
        };

        self.persist()?;
        Ok(record)
    }

    pub fn rename(
        &self,
        workspace_root: &Path,
        name: &str,
    ) -> Result<ProjectRecord, CommandErrorPayload> {
        let name = name.trim();
        if name.is_empty() {
            return Err(registry_io_failed("project name is required".to_owned()));
        }
        if name.chars().count() > 120 {
            return Err(registry_io_failed(
                "project name must be at most 120 characters".to_owned(),
            ));
        }
        let path = workspace_root.to_string_lossy().into_owned();
        let record = {
            let mut data = self
                .data
                .lock()
                .expect("project registry lock should not be poisoned");
            let project = data
                .projects
                .iter_mut()
                .find(|project| project.path == path)
                .ok_or_else(|| registry_not_found(path.clone()))?;
            project.name = name.to_owned();
            project.clone()
        };

        self.persist()?;
        Ok(record)
    }

    pub fn clear_active(&self) -> Result<(), CommandErrorPayload> {
        {
            let mut data = self
                .data
                .lock()
                .expect("project registry lock should not be poisoned");
            data.active_path = None;
        }

        self.persist()
    }

    pub fn contains(&self, workspace_root: &Path) -> bool {
        let path = workspace_root.to_string_lossy();
        self.data
            .lock()
            .expect("project registry lock should not be poisoned")
            .projects
            .iter()
            .any(|project| project.path == path)
    }

    pub fn move_project(
        &self,
        workspace_root: &Path,
        direction: ProjectMoveDirection,
    ) -> Result<(), CommandErrorPayload> {
        let path = workspace_root.to_string_lossy().into_owned();
        {
            let mut data = self
                .data
                .lock()
                .expect("project registry lock should not be poisoned");
            let index = data
                .projects
                .iter()
                .position(|project| project.path == path)
                .ok_or_else(|| registry_not_found(path.clone()))?;
            let target_index = match direction {
                ProjectMoveDirection::Up if index > 0 => Some(index - 1),
                ProjectMoveDirection::Down if index + 1 < data.projects.len() => Some(index + 1),
                _ => None,
            };
            if let Some(target_index) = target_index {
                data.projects.swap(index, target_index);
            }
        }

        self.persist()
    }

    #[must_use]
    pub fn has_active_project(&self) -> bool {
        self.active_path().is_some()
    }

    fn persist(&self) -> Result<(), CommandErrorPayload> {
        if let Some(parent) = self.path.parent() {
            crate::commands::stores::ensure_app_dir_no_symlink(
                parent,
                "project registry directory",
            )?;
        }

        let data = self
            .data
            .lock()
            .expect("project registry lock should not be poisoned");
        crate::commands::stores::write_json_file_atomic(&self.path, "project registry", &*data)
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

pub fn default_workspace_root() -> Result<PathBuf, CommandErrorPayload> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| registry_io_failed("home directory is unavailable".to_owned()))?;
    let root = PathBuf::from(home)
        .join(".jyowo")
        .join("workspaces")
        .join("default");
    crate::commands::stores::ensure_app_dir_no_symlink(&root, "default workspace")?;
    Ok(root)
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
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    static HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct HomeEnvGuard {
        previous: Option<std::ffi::OsString>,
    }

    impl HomeEnvGuard {
        fn set(home: &Path) -> Self {
            let previous = std::env::var_os("HOME");
            std::env::set_var("HOME", home.as_os_str());
            Self { previous }
        }
    }

    impl Drop for HomeEnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.take() {
                std::env::set_var("HOME", previous);
            } else {
                std::env::remove_var("HOME");
            }
        }
    }

    #[test]
    fn upsert_and_activate_persists_project_record() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let temp_dir = env::temp_dir().join(format!("jyowo-project-registry-{suffix}"));
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let temp_dir = temp_dir.canonicalize().expect("canonical temp dir");
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

    #[test]
    fn remove_clears_active_path_when_active_project_is_removed() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let temp_dir = env::temp_dir().join(format!("jyowo-project-registry-remove-{suffix}"));
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let temp_dir = temp_dir.canonicalize().expect("canonical temp dir");
        let workspace_root = temp_dir.join("workspace");
        fs::create_dir_all(&workspace_root).expect("workspace should be created");
        let registry_path = temp_dir.join("projects.json");
        let registry = ProjectRegistry {
            path: registry_path,
            data: Arc::new(Mutex::new(ProjectRegistryFile::default())),
        };

        registry
            .upsert_and_activate(&workspace_root)
            .expect("project should be registered");

        let removed = registry
            .remove(&workspace_root)
            .expect("project should be removed");

        assert_eq!(removed.path, workspace_root.to_string_lossy());
        assert!(registry.list_projects().is_empty());
        assert_eq!(registry.active_path(), None);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn set_active_preserves_project_list_order() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let temp_dir = env::temp_dir().join(format!("jyowo-project-registry-order-{suffix}"));
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let temp_dir = temp_dir.canonicalize().expect("canonical temp dir");
        let alpha = temp_dir.join("alpha");
        let beta = temp_dir.join("beta");
        fs::create_dir_all(&alpha).expect("alpha should be created");
        fs::create_dir_all(&beta).expect("beta should be created");
        let registry = ProjectRegistry {
            path: temp_dir.join("projects.json"),
            data: Arc::new(Mutex::new(ProjectRegistryFile::default())),
        };
        registry
            .upsert_and_activate(&alpha)
            .expect("alpha registered");
        registry
            .upsert_and_activate(&beta)
            .expect("beta registered");

        registry.set_active(&alpha).expect("alpha active");

        let paths = registry
            .list_projects()
            .into_iter()
            .map(|project| project.path)
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                alpha.to_string_lossy().to_string(),
                beta.to_string_lossy().to_string()
            ]
        );
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn move_project_reorders_persisted_projects() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let temp_dir = env::temp_dir().join(format!("jyowo-project-registry-move-{suffix}"));
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let temp_dir = temp_dir.canonicalize().expect("canonical temp dir");
        let alpha = temp_dir.join("alpha");
        let beta = temp_dir.join("beta");
        fs::create_dir_all(&alpha).expect("alpha should be created");
        fs::create_dir_all(&beta).expect("beta should be created");
        let registry_path = temp_dir.join("projects.json");
        let registry = ProjectRegistry {
            path: registry_path.clone(),
            data: Arc::new(Mutex::new(ProjectRegistryFile::default())),
        };
        registry
            .upsert_and_activate(&alpha)
            .expect("alpha registered");
        registry
            .upsert_and_activate(&beta)
            .expect("beta registered");

        registry
            .move_project(&beta, ProjectMoveDirection::Up)
            .expect("beta moved");

        let paths = registry
            .list_projects()
            .into_iter()
            .map(|project| project.path)
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                beta.to_string_lossy().to_string(),
                alpha.to_string_lossy().to_string()
            ]
        );
        assert!(registry_path.exists());
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[cfg(unix)]
    #[test]
    fn load_rejects_symlink_registry_file() {
        let _lock = HOME_ENV_LOCK.lock().expect("home env lock");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let home = temp_dir.path().join("home");
        let external = temp_dir.path().join("external-projects.json");
        fs::create_dir_all(home.join(".jyowo")).expect("home config dir");
        fs::write(&external, br#"{"projects":[],"activePath":null}"#).expect("external file");
        std::os::unix::fs::symlink(&external, home.join(".jyowo").join("projects.json"))
            .expect("registry symlink");
        let _home_guard = HomeEnvGuard::set(&home);

        let error = match ProjectRegistry::load() {
            Ok(_) => panic!("symlink registry must fail"),
            Err(error) => error,
        };

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(error.message.contains("symlink"));
    }

    #[cfg(unix)]
    #[test]
    fn default_workspace_root_is_private_and_rejects_symlinks() {
        use std::os::unix::fs::PermissionsExt;

        let _lock = HOME_ENV_LOCK.lock().expect("home env lock");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let temp_root = temp_dir.path().canonicalize().expect("canonical temp dir");
        let home = temp_root.join("home");
        fs::create_dir_all(&home).expect("home dir");
        let _home_guard = HomeEnvGuard::set(&home);

        let root = default_workspace_root().expect("default workspace");

        assert_eq!(root, home.join(".jyowo/workspaces/default"));
        assert!(root.is_dir());
        assert_eq!(
            fs::metadata(&root)
                .expect("default metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );

        fs::remove_dir(&root).expect("remove default root");
        let external = temp_root.join("external");
        fs::create_dir(&external).expect("external dir");
        std::os::unix::fs::symlink(&external, &root).expect("default symlink");

        let error = default_workspace_root().expect_err("default symlink must fail");
        assert!(error.message.contains("symlink"));
    }

    #[test]
    fn rename_project_preserves_identity_and_persists() {
        let _lock = HOME_ENV_LOCK.lock().expect("home env lock");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let temp_root = temp_dir.path().canonicalize().expect("canonical temp dir");
        let home = temp_root.join("home");
        let workspace = temp_root.join("workspace");
        fs::create_dir_all(&home).expect("home dir");
        fs::create_dir_all(&workspace).expect("workspace dir");
        let _home_guard = HomeEnvGuard::set(&home);
        let registry = ProjectRegistry::load().expect("registry loads");
        let original = registry
            .upsert_and_activate(&workspace)
            .expect("project registered");

        let renamed = registry
            .rename(&workspace, "  Workspace Alpha  ")
            .expect("project renamed");

        assert_eq!(renamed.path, original.path);
        assert_eq!(renamed.last_opened_at, original.last_opened_at);
        assert_eq!(renamed.name, "Workspace Alpha");
        drop(registry);
        let reloaded = ProjectRegistry::load().expect("registry reloads");
        assert_eq!(reloaded.list_projects(), vec![renamed]);
    }

    #[cfg(unix)]
    #[test]
    fn persist_rejects_symlink_parent_directory() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let temp_dir = env::temp_dir().join(format!("jyowo-project-registry-symlink-{suffix}"));
        let external = temp_dir.join("external");
        let link = temp_dir.join("link");
        fs::create_dir_all(&external).expect("external dir");
        std::os::unix::fs::symlink(&external, &link).expect("symlink");
        let registry = ProjectRegistry {
            path: link.join("projects.json"),
            data: Arc::new(Mutex::new(ProjectRegistryFile::default())),
        };

        let error = registry
            .upsert_and_activate(&temp_dir)
            .expect_err("symlink parent must fail");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(!external.join("projects.json").exists());
        let _ = fs::remove_dir_all(temp_dir);
    }
}

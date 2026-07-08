use super::*;
use crate::storage_layout::StorageLayout;

/// Scope-aware plugin store.
///
/// - **Global** (`workspace_root = None`): packages live under `~/.jyowo/plugins/packages/`.
/// - **Project** (`workspace_root = Some(<path>)`): packages live under `<workspace>/.jyowo/plugins/packages/`.
///
/// Enabled/disabled selection for project plugins is managed by [`ProjectConfigStore`]
/// via `plugins.json` config files.
#[derive(Clone)]
pub struct DesktopPluginStore {
    layout: StorageLayout,
    workspace_root: Option<PathBuf>,
}

impl DesktopPluginStore {
    /// Create a global plugin store (packages under `~/.jyowo/plugins/packages/`).
    pub fn global(layout: StorageLayout) -> Self {
        Self {
            layout,
            workspace_root: None,
        }
    }

    /// Create a project-scoped plugin store (packages under `<workspace>/.jyowo/plugins/packages/`).
    pub fn project(layout: StorageLayout, workspace_root: PathBuf) -> Self {
        Self {
            layout,
            workspace_root: Some(workspace_root),
        }
    }

    pub fn is_global(&self) -> bool {
        self.workspace_root.is_none()
    }

    fn root_dir(&self) -> PathBuf {
        match &self.workspace_root {
            Some(ws) => self.layout.project_plugins_root(ws),
            None => self.layout.global_plugins_root(),
        }
    }

    pub(crate) fn index_path(&self) -> PathBuf {
        self.root_dir().join("index.json")
    }

    fn package_dir(&self, package_dir: &str) -> PathBuf {
        self.package_root().join(package_dir)
    }
}

impl PluginStore for DesktopPluginStore {
    fn package_root(&self) -> PathBuf {
        self.root_dir().join("packages")
    }

    fn cargo_extension_root(&self) -> PathBuf {
        self.root_dir().join("extensions")
    }

    fn workspace_plugin_root(&self) -> PathBuf {
        match &self.workspace_root {
            Some(ws) => ws.join(".jyowo").join("plugins"),
            None => self.root_dir().to_path_buf(),
        }
    }

    fn load_record(&self) -> Result<PluginSettingsRecord, CommandErrorPayload> {
        match read_json_file::<PluginSettingsRecord>(&self.index_path(), "plugin index")? {
            Some(record) => {
                ensure_plugin_settings_record(&record)?;
                Ok(record)
            }
            None => Ok(PluginSettingsRecord::default()),
        }
    }

    fn save_record(&self, record: &PluginSettingsRecord) -> Result<(), CommandErrorPayload> {
        ensure_plugin_settings_record(record)?;
        write_plugin_settings_record(&self.index_path(), record)
    }

    fn write_plugin_package(
        &self,
        package_dir: &str,
        source_path: &Path,
    ) -> Result<String, CommandErrorPayload> {
        ensure_plugin_package_dir_name(package_dir)?;
        let destination = self.package_dir(package_dir);
        let parent = destination.parent().ok_or_else(|| {
            runtime_operation_failed("plugin package path has no parent".to_owned())
        })?;
        ensure_app_dir_no_symlink(parent, "plugin package directory")?;
        copy_plugin_package(source_path, &destination)
    }

    fn delete_plugin_package(&self, package_dir: &str) -> Result<(), CommandErrorPayload> {
        ensure_plugin_package_dir_name(package_dir)?;
        let path = self.package_dir(package_dir);
        ensure_no_symlink_components(&path, "plugin package")?;
        let root = self.package_root();
        let normalized_root = match root.canonicalize() {
            Ok(root) => root,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "plugin package root unavailable: {error}"
                )));
            }
        };
        let normalized_path = match path.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "plugin package unavailable: {error}"
                )));
            }
        };
        if normalized_path == normalized_root || !normalized_path.starts_with(&normalized_root) {
            return Err(invalid_payload(
                "plugin package path escaped package root".to_owned(),
            ));
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(runtime_operation_failed(format!(
                "plugin package delete failed: {error}"
            ))),
        }
    }
}

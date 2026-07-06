use super::*;
use crate::storage_layout::{JyowoHome, StorageLayout};

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
    /// Create a project-scoped plugin store.
    ///
    /// This is the compatibility constructor for the legacy `new(workspace_root)` API.
    /// For global stores use [`DesktopPluginStore::global`].
    pub fn new(workspace_root: PathBuf) -> Self {
        let home = JyowoHome::new(default_jyowo_home());
        Self::project(StorageLayout::new(home), workspace_root)
    }

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

fn default_jyowo_home() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".jyowo")
}

/// Migrate plugins from the old `<workspace>/.jyowo/runtime/plugins/` layout
/// to the new `<workspace>/.jyowo/plugins/` layout.
///
/// - Moves package directories from `user/<name>/` to `packages/<name>/`.
/// - Moves `index.json` to the new location.
/// - Returns the list of enabled plugin IDs from the old records so callers
///   can persist them as the project plugin selection config.
pub(crate) fn migrate_plugins_from_runtime(
    workspace_root: &Path,
) -> Result<(MigrationResult, Vec<String>), CommandErrorPayload> {
    let old_root = workspace_root
        .join(".jyowo")
        .join("runtime")
        .join("plugins");
    let new_root = workspace_root.join(".jyowo").join("plugins");

    if !old_root.exists() {
        if !new_root.join("index.json").exists() {
            return Ok((MigrationResult::NotNeeded, Vec::new()));
        }
        return Ok((MigrationResult::AlreadyMigrated, Vec::new()));
    }

    // Both old and new exist — conflict.
    if new_root.join("index.json").exists() {
        return Ok((
            MigrationResult::Conflict(MigrationConflict {
                kind: MigrationConflictKind::IdCollision,
                old_path: old_root.clone(),
                new_path: new_root.clone(),
                detail: "both old and new plugin store exist".to_owned(),
            }),
            Vec::new(),
        ));
    }

    ensure_no_symlink_components(&old_root, "old plugin root")?;
    ensure_no_symlink_components(&new_root, "new plugin root")?;

    // Read old index.
    let old_index = old_root.join("index.json");
    let record: PluginSettingsRecord =
        read_json_file(&old_index, "old plugin index")?.unwrap_or_default();

    let enabled_ids: Vec<String> = record
        .records
        .iter()
        .filter(|r| r.enabled)
        .map(|r| r.plugin_id.0.clone())
        .collect();

    // Create new target directories.
    let new_packages = new_root.join("packages");
    ensure_app_dir_no_symlink(&new_packages, "new plugin packages")?;

    // Migrate packages from old user/ dir.
    let old_user = old_root.join("user");
    if old_user.is_dir() {
        let entries = std::fs::read_dir(&old_user).map_err(|error| {
            runtime_operation_failed(format!("old plugin user read failed: {error}"))
        })?;
        let mut old_paths: Vec<PathBuf> = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                runtime_operation_failed(format!("old plugin entry read failed: {error}"))
            })?;
            old_paths.push(entry.path());
        }
        for old_path in old_paths {
            let Some(name) = old_path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            ensure_no_symlink_components(&old_path, "old plugin package")?;
            if old_path.is_dir() {
                let new_pkg = new_packages.join(name);
                ensure_no_symlink_components(&new_pkg, "new plugin package")?;
                std::fs::rename(&old_path, &new_pkg).map_err(|error| {
                    runtime_operation_failed(format!(
                        "plugin package move failed ({name}): {error}"
                    ))
                })?;
            }
        }
    }

    // Write index.json to new location.
    write_plugin_settings_record(&new_root.join("index.json"), &record)?;

    // Remove old runtime plugins directory.
    match std::fs::remove_dir_all(&old_root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_error) => {
            return Ok((MigrationResult::Migrated, enabled_ids));
        }
    }

    Ok((MigrationResult::Migrated, enabled_ids))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use harness_contracts::PluginId;

    use super::*;
    use crate::commands::stores::migration::MigrationResult;
    use crate::storage_layout::{JyowoHome, StorageLayout};

    #[test]
    fn global_plugin_store_uses_global_path() {
        let layout = StorageLayout::new(JyowoHome::new(Path::new("/home/alice/.jyowo")));
        let store = DesktopPluginStore::global(layout);
        assert!(store.is_global());
        assert_eq!(store.root_dir(), Path::new("/home/alice/.jyowo/plugins"));
        assert_eq!(
            store.package_root(),
            Path::new("/home/alice/.jyowo/plugins/packages")
        );
        assert_eq!(
            store.index_path(),
            Path::new("/home/alice/.jyowo/plugins/index.json")
        );
    }

    #[test]
    fn project_plugin_store_uses_project_path() {
        let layout = StorageLayout::new(JyowoHome::new(Path::new("/home/alice/.jyowo")));
        let workspace = Path::new("/workspaces/jyowo");
        let store = DesktopPluginStore::project(layout, workspace.to_path_buf());
        assert!(!store.is_global());
        assert_eq!(
            store.root_dir(),
            Path::new("/workspaces/jyowo/.jyowo/plugins")
        );
        assert_eq!(
            store.package_root(),
            Path::new("/workspaces/jyowo/.jyowo/plugins/packages")
        );
    }

    #[test]
    fn migrate_plugins_moves_user_packages_to_new_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().canonicalize().expect("canonical");

        // Set up old layout.
        let old_plugins = workspace.join(".jyowo").join("runtime").join("plugins");
        let old_user = old_plugins.join("user");
        std::fs::create_dir_all(&old_user).expect("create old user");

        // Create a package directory.
        let pkg_dir = old_user.join("my-plugin");
        std::fs::create_dir_all(&pkg_dir).expect("create pkg dir");
        std::fs::write(pkg_dir.join("manifest.json"), "{}").expect("write manifest");

        // Write index.json with one enabled record.
        let index_path = old_plugins.join("index.json");
        let record = PluginSettingsRecord {
            allow_project_plugins: false,
            records: vec![PluginStoreRecord {
                plugin_id: PluginId("my-plugin".to_owned()),
                name: "My Plugin".to_owned(),
                version: "1.0.0".to_owned(),
                enabled: true,
                package_dir: "my-plugin".to_owned(),
                source_path: String::new(),
                content_hash: "abc".to_owned(),
                imported_at: "2024-01-01T00:00:00Z".to_owned(),
                updated_at: "2024-01-01T00:00:00Z".to_owned(),
                config: serde_json::Value::Object(Default::default()),
                last_validation_error: None,
            }],
        };
        std::fs::write(&index_path, serde_json::to_vec_pretty(&record).unwrap())
            .expect("write index");

        let (result, enabled_ids) = migrate_plugins_from_runtime(&workspace).expect("migrate");

        assert_eq!(result, MigrationResult::Migrated);
        assert_eq!(enabled_ids, vec!["my-plugin".to_owned()]);

        // Old path should be gone.
        assert!(
            !old_plugins.exists(),
            "old runtime/plugins should be removed"
        );

        // New path should exist with packages under packages/.
        let new_pkg = workspace
            .join(".jyowo")
            .join("plugins")
            .join("packages")
            .join("my-plugin");
        assert!(new_pkg.exists(), "new package should exist");

        // New index should be at the new location.
        let new_index = workspace.join(".jyowo").join("plugins").join("index.json");
        assert!(new_index.exists(), "new index should exist");
    }

    #[test]
    fn migrate_plugins_returns_not_needed_when_old_path_absent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().canonicalize().expect("canonical");

        let (result, enabled_ids) = migrate_plugins_from_runtime(&workspace).expect("migrate");

        assert_eq!(result, MigrationResult::NotNeeded);
        assert!(enabled_ids.is_empty());
    }

    #[test]
    fn migrate_plugins_returns_conflict_when_both_exist() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().canonicalize().expect("canonical");

        let old_plugins = workspace.join(".jyowo").join("runtime").join("plugins");
        std::fs::create_dir_all(&old_plugins).expect("create old");
        std::fs::write(
            old_plugins.join("index.json"),
            r#"{"allowProjectPlugins":false,"records":[]}"#,
        )
        .expect("write old index");

        let new_plugins = workspace.join(".jyowo").join("plugins");
        std::fs::create_dir_all(&new_plugins).expect("create new");
        std::fs::write(
            new_plugins.join("index.json"),
            r#"{"allowProjectPlugins":false,"records":[]}"#,
        )
        .expect("write new index");

        let (result, _enabled_ids) = migrate_plugins_from_runtime(&workspace).expect("migrate");

        assert!(matches!(result, MigrationResult::Conflict(_)));
        assert!(
            old_plugins.exists(),
            "old should not be removed on conflict"
        );
    }
}

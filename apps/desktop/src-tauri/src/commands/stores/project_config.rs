use std::path::Path;

use harness_contracts::{ExecutionDefaultsRecord, ProviderSelectionRecord};

use crate::commands::error::CommandErrorPayload;
use crate::storage_layout::StorageLayout;

use super::{ensure_app_dir_no_symlink, read_json_file, write_json_file_atomic};

/// Typed store for project-scoped configuration under `<workspace>/.jyowo/config/`.
///
/// Uses [`StorageLayout`] for path resolution and delegates persistence to the
/// shared atomic I/O helpers. This store is intentionally lightweight — it owns
/// no cached state.
#[derive(Debug, Clone)]
pub struct ProjectConfigStore {
    layout: StorageLayout,
    workspace_root: Box<Path>,
}

impl ProjectConfigStore {
    pub fn new(layout: StorageLayout, workspace_root: impl Into<Box<Path>>) -> Self {
        Self {
            layout,
            workspace_root: workspace_root.into(),
        }
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn layout(&self) -> &StorageLayout {
        &self.layout
    }

    // ── Provider selection (project) ──────────────────────────────────

    pub fn load_project_provider_selection(
        &self,
    ) -> Result<ProviderSelectionRecord, CommandErrorPayload> {
        let path = self
            .layout
            .project_provider_selection_file(&self.workspace_root);
        ensure_config_dir(&path, "project provider selection")?;
        Ok(
            read_json_file::<ProviderSelectionRecord>(&path, "project provider selection")?
                .unwrap_or_default(),
        )
    }

    pub fn save_project_provider_selection(
        &self,
        record: &ProviderSelectionRecord,
    ) -> Result<(), CommandErrorPayload> {
        let path = self
            .layout
            .project_provider_selection_file(&self.workspace_root);
        ensure_config_dir(&path, "project provider selection")?;
        write_json_file_atomic(&path, "project provider selection", record)
    }

    // ── Execution overrides (project) ───────────────────────────────────

    pub fn load_execution_overrides(&self) -> Result<ExecutionDefaultsRecord, CommandErrorPayload> {
        let path = self
            .layout
            .project_execution_overrides_file(&self.workspace_root);
        ensure_config_dir(&path, "execution overrides")?;
        Ok(
            read_json_file::<ExecutionDefaultsRecord>(&path, "execution overrides")?
                .unwrap_or_default(),
        )
    }

    pub fn save_execution_overrides(
        &self,
        record: &ExecutionDefaultsRecord,
    ) -> Result<(), CommandErrorPayload> {
        let path = self
            .layout
            .project_execution_overrides_file(&self.workspace_root);
        ensure_config_dir(&path, "execution overrides")?;
        write_json_file_atomic(&path, "execution overrides", record)
    }
}

fn ensure_config_dir(path: &Path, label: &str) -> Result<(), CommandErrorPayload> {
    let parent = path.parent().ok_or_else(|| {
        crate::commands::error::runtime_operation_failed(format!(
            "{label} path has no parent directory"
        ))
    })?;
    ensure_app_dir_no_symlink(parent, &format!("{label} directory"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use harness_contracts::ProviderSelectionRecord;

    use crate::storage_layout::{JyowoHome, StorageLayout};

    use super::ProjectConfigStore;

    fn store(workspace_name: &str) -> (ProjectConfigStore, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_canonical = temp.path().canonicalize().expect("canonical tempdir");
        let home_root = temp_canonical.join(".jyowo");
        let layout = StorageLayout::new(JyowoHome::new(&home_root));
        let workspace_root = temp_canonical.join(workspace_name);
        std::fs::create_dir_all(&workspace_root).expect("create workspace dir");
        (ProjectConfigStore::new(layout, workspace_root), temp)
    }

    #[test]
    fn saves_and_loads_project_provider_selection() {
        let (store, _temp) = store("project-a");
        let record = ProviderSelectionRecord {
            default_config_id: Some("config-1".to_owned()),
        };
        store
            .save_project_provider_selection(&record)
            .expect("save");
        let loaded = store.load_project_provider_selection().expect("load");
        assert_eq!(loaded.default_config_id.as_deref(), Some("config-1"));
    }

    #[test]
    fn load_project_provider_selection_returns_default_when_missing() {
        let (store, _temp) = store("project-b");
        let loaded = store.load_project_provider_selection().expect("load");
        assert_eq!(loaded.default_config_id, None);
    }

    #[test]
    fn resolves_to_correct_project_config_path() {
        let layout = StorageLayout::new(JyowoHome::new(Path::new("/home/alice/.jyowo")));
        let workspace = Path::new("/workspaces/jyowo");
        let store = ProjectConfigStore::new(layout, workspace);

        let path = store
            .layout()
            .project_provider_selection_file(store.workspace_root());
        assert_eq!(
            path,
            Path::new("/workspaces/jyowo/.jyowo/config/provider-selection.json")
        );
    }

    #[test]
    fn project_provider_selection_is_not_global() {
        let (store_a, _temp_a) = store("project-a");
        let (store_b, _temp_b) = store("project-b");

        store_a
            .save_project_provider_selection(&ProviderSelectionRecord {
                default_config_id: Some("config-a".to_owned()),
            })
            .expect("save a");

        store_b
            .save_project_provider_selection(&ProviderSelectionRecord {
                default_config_id: Some("config-b".to_owned()),
            })
            .expect("save b");

        let loaded_a = store_a.load_project_provider_selection().expect("load a");
        let loaded_b = store_b.load_project_provider_selection().expect("load b");

        assert_eq!(loaded_a.default_config_id.as_deref(), Some("config-a"));
        assert_eq!(loaded_b.default_config_id.as_deref(), Some("config-b"));
    }
}

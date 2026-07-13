use super::*;
use crate::storage_layout::StorageLayout;

/// Scope-aware skill store.
///
/// - **Global** (`workspace_root = None`): packages live under `~/.jyowo/skills/packages/`.
/// - **Project** (`workspace_root = Some(<path>)`): packages live under `<workspace>/.jyowo/skills/packages/`.
///
/// Enabled/disabled selection is managed by [`GlobalConfigStore`] and [`ProjectConfigStore`]
/// via `skills.json` config files. This store manages package storage and metadata only.
#[derive(Clone)]
pub struct DesktopSkillStore {
    layout: StorageLayout,
    workspace_root: Option<PathBuf>,
}

impl DesktopSkillStore {
    /// Create a global skill store (packages under `~/.jyowo/skills/packages/`).
    pub fn global(layout: StorageLayout) -> Self {
        Self {
            layout,
            workspace_root: None,
        }
    }

    /// Create a project-scoped skill store (packages under `<workspace>/.jyowo/skills/packages/`).
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
            Some(ws) => self.layout.project_skills_root(ws),
            None => self.layout.global_skills_root(),
        }
    }

    fn index_path(&self) -> PathBuf {
        self.root_dir().join("index.json")
    }

    fn skill_dir(&self, id: &str, _enabled: bool) -> PathBuf {
        self.enabled_dir().join(id)
    }
}

impl SkillStore for DesktopSkillStore {
    fn enabled_dir(&self) -> PathBuf {
        self.root_dir().join("packages")
    }

    fn load_records(&self) -> Result<Vec<SkillStoreRecord>, CommandErrorPayload> {
        Ok(read_json_file(&self.index_path(), "skill index")?.unwrap_or_default())
    }

    fn save_records(&self, records: &[SkillStoreRecord]) -> Result<(), CommandErrorPayload> {
        write_skill_records(&self.index_path(), records)
    }

    fn current_package_hash(
        &self,
        record: &SkillStoreRecord,
    ) -> Result<Option<String>, CommandErrorPayload> {
        ensure_skill_id(&record.id)?;
        let package_root = self.skill_dir(&record.id, record.enabled);
        ensure_no_symlink_components(&package_root, "skill package")?;
        hash_skill_package(&package_root).map(Some)
    }

    fn write_skill_package(
        &self,
        id: &str,
        enabled: bool,
        source_path: &Path,
    ) -> Result<String, CommandErrorPayload> {
        ensure_skill_id(id)?;
        let path = self.skill_dir(id, enabled);
        let parent = path.parent().ok_or_else(|| {
            runtime_operation_failed("skill package path has no parent".to_owned())
        })?;
        ensure_app_dir_no_symlink(parent, "skill directory")?;
        ensure_no_symlink_components(&path, "skill package")?;
        copy_skill_package(source_path, &path)
    }

    fn read_skill_entry_file(
        &self,
        record: &SkillStoreRecord,
    ) -> Result<String, CommandErrorPayload> {
        ensure_skill_id(&record.id)?;
        let path = self
            .skill_dir(&record.id, record.enabled)
            .join(SKILL_PACKAGE_ENTRY_FILE);
        ensure_no_symlink_components(&path, "skill entry file")?;
        match std::fs::symlink_metadata(&path) {
            Ok(_) => return read_skill_entry_markdown_file(&path, "skill entry file"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(invalid_payload("skill entry file not found".to_owned()));
            }
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "skill entry file metadata failed: {error}"
                )));
            }
        }
    }

    fn list_skill_package_files(
        &self,
        record: &SkillStoreRecord,
    ) -> Result<Vec<SkillFilePayload>, CommandErrorPayload> {
        ensure_skill_id(&record.id)?;
        let package_root = self.skill_dir(&record.id, record.enabled);
        ensure_no_symlink_components(&package_root, "skill package")?;
        if package_root.is_dir() {
            return list_skill_package_files(&package_root);
        }
        Ok(Vec::new())
    }

    fn read_skill_package_file(
        &self,
        record: &SkillStoreRecord,
        relative_path: &str,
    ) -> Result<SkillFileContentPayload, CommandErrorPayload> {
        ensure_skill_id(&record.id)?;
        let relative_path = normalize_skill_relative_path(relative_path)?;
        let package_root = self.skill_dir(&record.id, record.enabled);
        ensure_no_symlink_components(&package_root, "skill package")?;
        if package_root.is_dir() {
            return read_skill_package_file(&package_root, &relative_path);
        }
        Err(invalid_payload("skill file not found".to_owned()))
    }

    fn move_skill_package(&self, id: &str, enabled: bool) -> Result<(), CommandErrorPayload> {
        ensure_skill_id(id)?;
        let package = self.skill_dir(id, enabled);
        let parent = package.parent().ok_or_else(|| {
            runtime_operation_failed("skill package path has no parent".to_owned())
        })?;
        ensure_app_dir_no_symlink(parent, "skill directory")?;
        ensure_no_symlink_components(&package, "skill package")?;
        Ok(())
    }

    fn delete_skill_package(&self, id: &str) -> Result<(), CommandErrorPayload> {
        ensure_skill_id(id)?;
        let path = self.skill_dir(id, true);
        ensure_no_symlink_components(&path, "skill package")?;
        match std::fs::remove_dir_all(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "skill package delete failed: {error}"
                )));
            }
        }
        Ok(())
    }
}

fn read_skill_entry_markdown_file(path: &Path, label: &str) -> Result<String, CommandErrorPayload> {
    let bytes = read_regular_file_no_follow(path, label, MAX_SKILL_MARKDOWN_BYTES)?;
    String::from_utf8(bytes).map_err(|_| invalid_payload(format!("{label} must be valid UTF-8")))
}

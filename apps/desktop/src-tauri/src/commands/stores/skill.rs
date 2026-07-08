use super::*;
use crate::storage_layout::{JyowoHome, StorageLayout};

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
    /// Create a project-scoped skill store.
    ///
    /// This is the compatibility constructor for the legacy `new(workspace_root)` API.
    /// For global stores use [`DesktopSkillStore::global`].
    pub fn new(workspace_root: PathBuf) -> Self {
        let home = JyowoHome::new(default_jyowo_home());
        Self::project(StorageLayout::new(home), workspace_root)
    }

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

    fn disabled_dir(&self) -> PathBuf {
        self.root_dir().join("packages").join("disabled")
    }

    fn skill_dir(&self, id: &str, _enabled: bool) -> PathBuf {
        self.enabled_dir().join(id)
    }

    fn legacy_skill_file_path(&self, id: &str, enabled: bool) -> PathBuf {
        let dir = if enabled {
            self.enabled_dir()
        } else {
            self.disabled_dir()
        };
        dir.join(format!("{id}.md"))
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
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "skill entry file metadata failed: {error}"
                )));
            }
        }
        let path = self.legacy_skill_file_path(&record.id, record.enabled);
        ensure_no_symlink_components(&path, "legacy skill file")?;
        read_skill_entry_markdown_file(&path, "legacy skill file")
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
        let legacy_path = self.legacy_skill_file_path(&record.id, record.enabled);
        ensure_no_symlink_components(&legacy_path, "legacy skill file")?;
        if legacy_path.is_file() {
            let metadata = std::fs::metadata(&legacy_path).map_err(|error| {
                runtime_operation_failed(format!("legacy skill file metadata failed: {error}"))
            })?;
            return Ok(vec![SkillFilePayload {
                path: SKILL_PACKAGE_ENTRY_FILE.to_owned(),
                name: SKILL_PACKAGE_ENTRY_FILE.to_owned(),
                kind: "file",
                depth: 0,
                size_bytes: Some(metadata.len()),
            }]);
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
        if path_to_workspace_string(&relative_path) != SKILL_PACKAGE_ENTRY_FILE {
            return Err(invalid_payload("skill file not found".to_owned()));
        }
        let legacy_path = self.legacy_skill_file_path(&record.id, record.enabled);
        ensure_no_symlink_components(&legacy_path, "legacy skill file")?;
        read_skill_package_file_at(&legacy_path, SKILL_PACKAGE_ENTRY_FILE)
    }

    fn move_skill_package(&self, id: &str, enabled: bool) -> Result<(), CommandErrorPayload> {
        ensure_skill_id(id)?;
        let package = self.skill_dir(id, enabled);
        let parent = package.parent().ok_or_else(|| {
            runtime_operation_failed("skill package path has no parent".to_owned())
        })?;
        ensure_app_dir_no_symlink(parent, "skill directory")?;
        ensure_no_symlink_components(&package, "skill package")?;
        if package.exists() {
            return Ok(());
        }
        let from = self.legacy_skill_file_path(id, !enabled);
        let to = self.legacy_skill_file_path(id, enabled);
        ensure_no_symlink_components(&from, "legacy skill file")?;
        ensure_no_symlink_components(&to, "legacy skill file")?;
        if from.exists() {
            std::fs::rename(&from, &to).map_err(|error| {
                runtime_operation_failed(format!("legacy skill file move failed: {error}"))
            })?;
        }
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
        for enabled in [true, false] {
            let legacy_path = self.legacy_skill_file_path(id, enabled);
            ensure_no_symlink_components(&legacy_path, "legacy skill file")?;
            match std::fs::remove_file(&legacy_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(runtime_operation_failed(format!(
                        "legacy skill file delete failed: {error}"
                    )));
                }
            }
        }
        Ok(())
    }
}

fn read_skill_entry_markdown_file(path: &Path, label: &str) -> Result<String, CommandErrorPayload> {
    let bytes = read_regular_file_no_follow(path, label, MAX_SKILL_MARKDOWN_BYTES)?;
    String::from_utf8(bytes).map_err(|_| invalid_payload(format!("{label} must be valid UTF-8")))
}

fn default_jyowo_home() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".jyowo")
}

/// Migrate skills from the old `<workspace>/.jyowo/runtime/skills/` layout
/// to the new `<workspace>/.jyowo/skills/` layout.
///
/// - Moves package directories from `enabled/<id>/` and `disabled/<id>/`
///   to `packages/<id>/`.
/// - Migrates legacy `<id>.md` files into `<id>/SKILL.md` package directories.
/// - Preserves the `index.json` with the same records.
/// - Returns the list of enabled skill IDs from the old records so callers
///   can persist them as the project skill selection config.
///
/// This is intentionally NOT using the generic `migrate_json_file` because
/// skills involve directory trees, not just JSON files.
pub(crate) fn migrate_skills_from_runtime(
    workspace_root: &Path,
) -> Result<(MigrationResult, Vec<String>), CommandErrorPayload> {
    let old_root = workspace_root.join(".jyowo").join("runtime").join("skills");
    let new_root = workspace_root.join(".jyowo").join("skills");

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
                detail: "both old and new skill store exist".to_owned(),
            }),
            Vec::new(),
        ));
    }

    ensure_no_symlink_components(&old_root, "old skill root")?;
    ensure_no_symlink_components(&new_root, "new skill root")?;

    // Read old index.
    let old_index = old_root.join("index.json");
    let records: Vec<SkillStoreRecord> =
        read_json_file(&old_index, "old skill index")?.unwrap_or_default();

    let enabled_ids: Vec<String> = records
        .iter()
        .filter(|r| r.enabled)
        .map(|r| r.id.clone())
        .collect();

    let staging_root = new_root.with_file_name("skills.migration-staging");
    let _ = std::fs::remove_dir_all(&staging_root);
    let staging_packages = staging_root.join("packages");
    ensure_app_dir_no_symlink(&staging_packages, "new skill packages")?;

    // Migrate packages from old enabled/ and disabled/ dirs.
    for old_subdir_name in &["enabled", "disabled"] {
        let old_subdir = old_root.join(old_subdir_name);
        if !old_subdir.is_dir() {
            continue;
        }

        let entries = std::fs::read_dir(&old_subdir).map_err(|error| {
            runtime_operation_failed(format!("old skill {old_subdir_name} read failed: {error}"))
        })?;

        // Collect entries first to avoid borrow issues.
        let mut old_paths: Vec<PathBuf> = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                runtime_operation_failed(format!("old skill entry read failed: {error}"))
            })?;
            old_paths.push(entry.path());
        }

        for old_path in old_paths {
            let Some(name) = old_path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };

            ensure_no_symlink_components(&old_path, "old skill entry")?;

            if old_path.is_dir() {
                // Package directory: copy to packages/<name>/.
                let new_pkg = staging_packages.join(name);
                let new_pkg_parent = new_pkg.parent().ok_or_else(|| {
                    runtime_operation_failed("new skill package path has no parent".to_owned())
                })?;
                ensure_app_dir_no_symlink(new_pkg_parent, "new skill package dir")?;
                ensure_no_symlink_components(&new_pkg, "new skill package")?;
                copy_skill_package(&old_path, &new_pkg).map_err(|error| {
                    let _ = std::fs::remove_dir_all(&staging_root);
                    runtime_operation_failed(format!(
                        "skill package copy failed ({name}): {}",
                        error.message
                    ))
                })?;
            } else if old_path.extension().and_then(|e| e.to_str()) == Some("md") {
                // Legacy .md file: wrap in package directory under packages/<id>/.
                let stem = name.trim_end_matches(".md");
                let new_pkg = staging_packages.join(stem);
                ensure_app_dir_no_symlink(&new_pkg, "new skill package dir")?;
                let new_entry = new_pkg.join("SKILL.md");
                ensure_no_symlink_components(&new_entry, "new skill entry file")?;
                std::fs::copy(&old_path, &new_entry).map_err(|error| {
                    let _ = std::fs::remove_dir_all(&staging_root);
                    runtime_operation_failed(format!(
                        "legacy skill file copy failed ({stem}): {error}"
                    ))
                })?;
            }
        }
    }

    // Write index.json to new location.
    write_skill_records(&staging_root.join("index.json"), &records)?;
    std::fs::rename(&staging_root, &new_root).map_err(|error| {
        let _ = std::fs::remove_dir_all(&staging_root);
        runtime_operation_failed(format!("skill migration commit failed: {error}"))
    })?;

    Ok((MigrationResult::Migrated, enabled_ids))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::commands::stores::migration::MigrationResult;
    use crate::storage_layout::{JyowoHome, StorageLayout};

    #[test]
    fn global_skill_store_uses_global_path() {
        let layout = StorageLayout::new(JyowoHome::new(Path::new("/home/alice/.jyowo")));
        let store = DesktopSkillStore::global(layout);
        assert!(store.is_global());
        assert_eq!(store.root_dir(), Path::new("/home/alice/.jyowo/skills"));
        assert_eq!(
            store.enabled_dir(),
            Path::new("/home/alice/.jyowo/skills/packages")
        );
        assert_eq!(
            store.index_path(),
            Path::new("/home/alice/.jyowo/skills/index.json")
        );
    }

    #[test]
    fn project_skill_store_uses_project_path() {
        let layout = StorageLayout::new(JyowoHome::new(Path::new("/home/alice/.jyowo")));
        let workspace = Path::new("/workspaces/jyowo");
        let store = DesktopSkillStore::project(layout, workspace.to_path_buf());
        assert!(!store.is_global());
        assert_eq!(
            store.root_dir(),
            Path::new("/workspaces/jyowo/.jyowo/skills")
        );
        assert_eq!(
            store.enabled_dir(),
            Path::new("/workspaces/jyowo/.jyowo/skills/packages")
        );
    }

    #[test]
    fn migrate_skills_moves_enabled_packages_to_new_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().canonicalize().expect("canonical");

        // Set up old layout.
        let old_skills = workspace.join(".jyowo").join("runtime").join("skills");
        let old_enabled = old_skills.join("enabled");
        std::fs::create_dir_all(&old_enabled).expect("create old enabled");

        // Create a package directory with SKILL.md.
        let pkg_dir = old_enabled.join("my-skill");
        std::fs::create_dir_all(&pkg_dir).expect("create pkg dir");
        std::fs::write(pkg_dir.join("SKILL.md"), "---\nname: my-skill\n---\nbody\n")
            .expect("write SKILL.md");

        // Write index.json with one enabled record.
        let index_path = old_skills.join("index.json");
        let records = vec![SkillStoreRecord {
            id: "my-skill".to_owned(),
            name: "My Skill".to_owned(),
            description: "Test".to_owned(),
            enabled: true,
            content_hash: "abc".to_owned(),
            package_dir: "my-skill".to_owned(),
            file_name: String::new(),
            imported_at: "2024-01-01T00:00:00Z".to_owned(),
            updated_at: "2024-01-01T00:00:00Z".to_owned(),
            tags: vec![],
            category: None,
            last_validation_error: None,
            origin: None,
        }];
        std::fs::write(&index_path, serde_json::to_vec_pretty(&records).unwrap())
            .expect("write index");

        let (result, enabled_ids) = migrate_skills_from_runtime(&workspace).expect("migrate");

        assert_eq!(result, MigrationResult::Migrated);
        assert_eq!(enabled_ids, vec!["my-skill".to_owned()]);

        // Store migration stages the new store but leaves old runtime data for startup commit.
        assert!(
            old_skills.exists(),
            "old runtime/skills should be preserved"
        );
        assert!(
            pkg_dir.join("SKILL.md").exists(),
            "old package should be preserved"
        );

        // New path should exist with packages under flat packages/<id>/.
        let new_pkg = workspace
            .join(".jyowo")
            .join("skills")
            .join("packages")
            .join("my-skill");
        assert!(
            new_pkg.join("SKILL.md").exists(),
            "new package should exist"
        );

        // New index should be at the new location.
        let new_index = workspace.join(".jyowo").join("skills").join("index.json");
        assert!(new_index.exists(), "new index should exist");
    }

    #[test]
    fn migrate_skills_returns_not_needed_when_old_path_absent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().canonicalize().expect("canonical");

        let (result, enabled_ids) = migrate_skills_from_runtime(&workspace).expect("migrate");

        assert_eq!(result, MigrationResult::NotNeeded);
        assert!(enabled_ids.is_empty());
    }

    #[test]
    fn migrate_skills_returns_conflict_when_both_exist() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().canonicalize().expect("canonical");

        // Create both old and new.
        let old_skills = workspace.join(".jyowo").join("runtime").join("skills");
        std::fs::create_dir_all(old_skills.join("enabled")).expect("create old");
        std::fs::write(old_skills.join("index.json"), "[]").expect("write old index");

        let new_skills = workspace.join(".jyowo").join("skills");
        std::fs::create_dir_all(&new_skills).expect("create new");
        std::fs::write(new_skills.join("index.json"), "[]").expect("write new index");

        let (result, _enabled_ids) = migrate_skills_from_runtime(&workspace).expect("migrate");

        assert!(matches!(result, MigrationResult::Conflict(_)));
        // Verify nothing was written.
        assert!(old_skills.exists(), "old should not be removed on conflict");
    }
}

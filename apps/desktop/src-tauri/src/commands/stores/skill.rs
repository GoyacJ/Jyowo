use super::*;

#[derive(Clone)]
pub struct DesktopSkillStore {
    workspace_root: PathBuf,
}

impl DesktopSkillStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    fn root_dir(&self) -> PathBuf {
        self.workspace_root
            .join(".jyowo")
            .join("runtime")
            .join("skills")
    }

    fn index_path(&self) -> PathBuf {
        self.root_dir().join("index.json")
    }

    fn disabled_dir(&self) -> PathBuf {
        self.root_dir().join("disabled")
    }

    fn skill_dir(&self, id: &str, enabled: bool) -> PathBuf {
        let dir = if enabled {
            self.enabled_dir()
        } else {
            self.disabled_dir()
        };
        dir.join(id)
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
        self.root_dir().join("enabled")
    }

    fn load_records(&self) -> Result<Vec<SkillStoreRecord>, CommandErrorPayload> {
        let index_path = self.index_path();
        ensure_no_symlink_components(&index_path, "skill index file")?;
        match std::fs::read(&index_path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                runtime_operation_failed(format!("skill index parse failed: {error}"))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(runtime_operation_failed(format!(
                "skill index read failed: {error}"
            ))),
        }
    }

    fn save_records(&self, records: &[SkillStoreRecord]) -> Result<(), CommandErrorPayload> {
        write_skill_records(&self.index_path(), records)
    }

    fn write_skill_package(
        &self,
        id: &str,
        enabled: bool,
        source_path: &Path,
    ) -> Result<(), CommandErrorPayload> {
        ensure_skill_id(id)?;
        let path = self.skill_dir(id, enabled);
        let parent = path.parent().ok_or_else(|| {
            runtime_operation_failed("skill package path has no parent".to_owned())
        })?;
        ensure_no_symlink_components(parent, "skill directory")?;
        std::fs::create_dir_all(parent).map_err(|error| {
            runtime_operation_failed(format!("skill directory unavailable: {error}"))
        })?;
        ensure_no_symlink_components(parent, "skill directory")?;
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
        if path.exists() {
            return std::fs::read_to_string(&path).map_err(|error| {
                runtime_operation_failed(format!("skill entry file read failed: {error}"))
            });
        }
        let path = self.legacy_skill_file_path(&record.id, record.enabled);
        ensure_no_symlink_components(&path, "legacy skill file")?;
        std::fs::read_to_string(&path).map_err(|error| {
            runtime_operation_failed(format!("legacy skill file read failed: {error}"))
        })
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
        let from = self.skill_dir(id, !enabled);
        let to = self.skill_dir(id, enabled);
        let parent = to.parent().ok_or_else(|| {
            runtime_operation_failed("skill package path has no parent".to_owned())
        })?;
        ensure_no_symlink_components(parent, "skill directory")?;
        std::fs::create_dir_all(parent).map_err(|error| {
            runtime_operation_failed(format!("skill directory unavailable: {error}"))
        })?;
        ensure_no_symlink_components(&from, "skill package")?;
        ensure_no_symlink_components(&to, "skill package")?;
        if from.exists() {
            std::fs::rename(&from, &to).map_err(|error| {
                runtime_operation_failed(format!("skill package move failed: {error}"))
            })?;
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
        for enabled in [true, false] {
            let path = self.skill_dir(id, enabled);
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

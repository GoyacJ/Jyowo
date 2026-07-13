use super::*;

struct SkillTestHome {
    skills_root: PathBuf,
    selection_path: PathBuf,
}

impl SkillTestHome {
    fn package_root(&self) -> PathBuf {
        self.skills_root.join("packages")
    }

    fn package_path(&self, skill_id: &str) -> PathBuf {
        self.package_root().join(skill_id)
    }

    fn index_path(&self) -> PathBuf {
        self.skills_root.join("index.json")
    }

    fn selection_path(&self) -> PathBuf {
        self.selection_path.clone()
    }
}

fn skill_test_home(workspace: &Path) -> SkillTestHome {
    let layout = test_storage_layout_for_workspace(workspace);
    SkillTestHome {
        skills_root: layout.global_skills_root(),
        selection_path: layout.global_skills_file(),
    }
}

#[tokio::test]
async fn import_skill_persists_enabled_skill_without_exposing_source_path() {
    let workspace = unique_workspace("skill-import");
    std::fs::create_dir_all(&workspace).unwrap();
    let home = skill_test_home(&workspace);
    let source_dir = unique_workspace("skill-source");
    let source_path = write_skill_package(
        &source_dir,
        "summarize",
        "summarize",
        "Summarize project notes",
        Some(("references/style.md", "Use concise bullets.")),
    );
    let state = runtime_state_with_settings_runtime_for_workspace(workspace.clone()).await;

    let imported = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: source_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    let serialized = serde_json::to_string(&imported).unwrap();

    assert_eq!(imported.skill.name, "summarize");
    assert!(imported.skill.enabled);
    assert!(imported.skill.manageable);
    assert_eq!(imported.skill.source_kind, "workspace");
    assert!(!serialized.contains(&source_dir.to_string_lossy().to_string()));
    assert!(home
        .package_path(&imported.skill.id)
        .join("SKILL.md")
        .exists());
    assert!(home
        .package_path(&imported.skill.id)
        .join("references/style.md")
        .exists());
    let selection: harness_contracts::SkillSelectionRecord =
        serde_json::from_str(&std::fs::read_to_string(home.selection_path()).unwrap()).unwrap();
    assert_eq!(selection.enabled, vec![imported.skill.id.clone()]);
}

#[tokio::test]
async fn missing_skill_selection_uses_current_index_enabled_state() {
    let workspace = unique_workspace("skill-missing-selection-current-index");
    std::fs::create_dir_all(&workspace).unwrap();
    let home = skill_test_home(&workspace);
    let skill_id = "current-skill";
    let package_root = home.package_root();
    write_skill_package(
        &package_root,
        skill_id,
        "current-skill",
        "Current package skill",
        None,
    );
    let content_hash =
        harness_skill::hash_skill_package(&home.package_path(skill_id)).expect("package hash");
    let index_path = home.index_path();
    std::fs::create_dir_all(index_path.parent().unwrap()).unwrap();
    std::fs::write(
        &index_path,
        serde_json::to_vec_pretty(&vec![SkillStoreRecord {
            id: skill_id.to_owned(),
            name: "current-skill".to_owned(),
            description: "Current package skill".to_owned(),
            enabled: true,
            content_hash,
            package_dir: skill_id.to_owned(),
            file_name: String::new(),
            imported_at: now().to_rfc3339(),
            updated_at: now().to_rfc3339(),
            tags: Vec::new(),
            category: None,
            last_validation_error: None,
            origin: None,
        }])
        .unwrap(),
    )
    .unwrap();

    let state = runtime_state_with_settings_runtime_for_workspace(workspace.clone()).await;
    let listed = list_skills_with_runtime_state(&state).await.unwrap();

    assert!(!home.selection_path().exists());
    assert!(listed
        .skills
        .iter()
        .any(|skill| skill.id == skill_id && skill.enabled && skill.status == "ready"));
}

#[tokio::test]
async fn import_skill_rejects_single_markdown_files() {
    let workspace = unique_workspace("skill-import-reject-file");
    std::fs::create_dir_all(&workspace).unwrap();
    let source_dir = unique_workspace("skill-file-source");
    std::fs::create_dir_all(&source_dir).unwrap();
    let source_path = source_dir.join("summarize.md");
    std::fs::write(
        &source_path,
        skill_markdown("summarize", "Summarize project notes"),
    )
    .unwrap();
    let source_path = source_path.canonicalize().unwrap();
    let state = runtime_state_with_settings_runtime_for_workspace(workspace).await;

    let error = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: source_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error
        .message
        .contains("skill source path must point to a directory"));
}

#[cfg(unix)]
#[tokio::test]
async fn import_skill_rejects_symlink_source_package() {
    let workspace = unique_workspace("skill-import-reject-source-symlink");
    std::fs::create_dir_all(&workspace).unwrap();
    let source_dir = unique_workspace("skill-source-real");
    let source_path = write_skill_package(
        &source_dir,
        "symlinked",
        "symlinked",
        "Should be rejected",
        None,
    );
    let link_dir = unique_workspace("skill-source-link");
    std::fs::create_dir_all(&link_dir).unwrap();
    let linked_path = link_dir.join("linked-package");
    std::os::unix::fs::symlink(&source_path, &linked_path).unwrap();
    let state = runtime_state_with_settings_runtime_for_workspace(workspace).await;

    let error = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: linked_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert!(error.message.contains("must not use symlinks"));
}

#[tokio::test]
async fn disabling_skill_moves_file_and_removes_it_from_runtime_list() {
    let workspace = unique_workspace("skill-disable");
    std::fs::create_dir_all(&workspace).unwrap();
    let home = skill_test_home(&workspace);
    let source_dir = unique_workspace("skill-disable-source");
    let source_path =
        write_skill_package(&source_dir, "draft", "draft", "Draft release notes", None);
    let state = runtime_state_with_settings_runtime_for_workspace(workspace.clone()).await;
    let imported = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: source_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap();

    let disabled = set_skill_enabled_with_runtime_state(
        SetSkillEnabledRequest {
            id: imported.skill.id.clone(),
            enabled: false,
        },
        &state,
    )
    .await
    .unwrap();
    let listed = list_skills_with_runtime_state(&state).await.unwrap();

    assert!(!disabled.skill.enabled);
    assert_eq!(disabled.skill.status, "disabled");
    assert!(home
        .package_path(&imported.skill.id)
        .join("SKILL.md")
        .exists());
    let selection: harness_contracts::SkillSelectionRecord =
        serde_json::from_str(&std::fs::read_to_string(home.selection_path()).unwrap()).unwrap();
    assert!(selection.enabled.is_empty());
    assert!(listed
        .skills
        .iter()
        .any(|skill| skill.id == imported.skill.id && !skill.enabled));
    assert!(listed
        .skills
        .iter()
        .all(|skill| skill.name != "draft" || !skill.enabled));

    let enabled = set_skill_enabled_with_runtime_state(
        SetSkillEnabledRequest {
            id: imported.skill.id.clone(),
            enabled: true,
        },
        &state,
    )
    .await
    .unwrap();
    let listed = list_skills_with_runtime_state(&state).await.unwrap();

    assert!(enabled.skill.enabled);
    assert_eq!(enabled.skill.status, "ready");
    assert!(home
        .package_path(&imported.skill.id)
        .join("SKILL.md")
        .exists());
    assert!(listed
        .skills
        .iter()
        .any(|skill| skill.id == imported.skill.id && skill.enabled));
}

#[tokio::test]
async fn enabling_skill_rejects_runtime_duplicate_name() {
    let workspace = unique_workspace("skill-enable-duplicate-runtime");
    std::fs::create_dir_all(&workspace).unwrap();
    let home = skill_test_home(&workspace);
    let disabled_id = "managed-disabled";
    let disabled_dir = home.package_path(disabled_id);
    std::fs::create_dir_all(&disabled_dir).unwrap();
    std::fs::write(
        disabled_dir.join("SKILL.md"),
        skill_markdown("shared-name", "Workspace skill"),
    )
    .unwrap();
    let content_hash = harness_skill::hash_skill_package(&disabled_dir).expect("package hash");
    let record = SkillStoreRecord {
        id: disabled_id.to_owned(),
        name: "shared-name".to_owned(),
        description: "Workspace skill".to_owned(),
        enabled: false,
        content_hash,
        package_dir: disabled_id.to_owned(),
        file_name: String::new(),
        imported_at: now().to_rfc3339(),
        updated_at: now().to_rfc3339(),
        tags: Vec::new(),
        category: None,
        last_validation_error: None,
        origin: None,
    };
    let index_path = home.index_path();
    std::fs::create_dir_all(index_path.parent().unwrap()).unwrap();
    std::fs::write(
        &index_path,
        serde_json::to_vec_pretty(&vec![record]).unwrap(),
    )
    .unwrap();
    let state = runtime_state_with_settings_runtime_for_workspace(workspace.clone()).await;
    register_test_skill(&state, "shared-name", "Runtime skill");

    let error = set_skill_enabled_with_runtime_state(
        SetSkillEnabledRequest {
            id: disabled_id.to_owned(),
            enabled: true,
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error
        .message
        .contains("active skill name already exists: shared-name"));
    assert!(home.package_path(disabled_id).join("SKILL.md").exists());
    let selection_path = home.selection_path();
    if selection_path.exists() {
        let selection: harness_contracts::SkillSelectionRecord =
            serde_json::from_str(&std::fs::read_to_string(selection_path).unwrap()).unwrap();
        assert!(!selection.enabled.iter().any(|id| id == disabled_id));
    }
}

#[tokio::test]
async fn delete_skill_removes_managed_record_and_file() {
    let workspace = unique_workspace("skill-delete");
    std::fs::create_dir_all(&workspace).unwrap();
    let home = skill_test_home(&workspace);
    let source_dir = unique_workspace("skill-delete-source");
    let source_path = write_skill_package(
        &source_dir,
        "cleanup",
        "cleanup",
        "Clean up workspace",
        None,
    );
    let state = runtime_state_with_settings_runtime_for_workspace(workspace.clone()).await;
    let imported = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: source_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap();

    let deleted = delete_skill_with_runtime_state(
        DeleteSkillRequest {
            id: imported.skill.id.clone(),
        },
        &state,
    )
    .await
    .unwrap();
    let listed = list_skills_with_runtime_state(&state).await.unwrap();

    assert_eq!(deleted.id, imported.skill.id);
    assert_eq!(deleted.status, "deleted");
    assert!(!home.package_path(&imported.skill.id).exists());
    assert!(listed
        .skills
        .iter()
        .all(|skill| skill.id != imported.skill.id));
}

#[tokio::test]
async fn delete_skill_keeps_record_and_package_when_selection_write_fails() {
    let workspace = unique_workspace("skill-delete-selection-fails");
    std::fs::create_dir_all(&workspace).unwrap();
    let home = skill_test_home(&workspace);
    let source_dir = unique_workspace("skill-delete-selection-fails-source");
    let source_path = write_skill_package(
        &source_dir,
        "keep-on-failure",
        "keep-on-failure",
        "Keep package when config write fails",
        None,
    );
    let state = runtime_state_with_settings_runtime_for_workspace(workspace.clone()).await;
    let imported = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: source_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    let selection_path = home.selection_path();
    std::fs::remove_file(&selection_path).unwrap();
    std::fs::create_dir(&selection_path).unwrap();

    let error = delete_skill_with_runtime_state(
        DeleteSkillRequest {
            id: imported.skill.id.clone(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert!(error.code.starts_with("RUNTIME_"));
    assert!(home
        .package_path(&imported.skill.id)
        .join("SKILL.md")
        .exists());
    let records: Vec<SkillStoreRecord> =
        serde_json::from_str(&std::fs::read_to_string(home.index_path()).unwrap()).unwrap();
    assert!(records.iter().any(|record| record.id == imported.skill.id));
}

#[tokio::test]
async fn delete_skill_removes_disabled_managed_record_and_file() {
    let workspace = unique_workspace("skill-delete-disabled");
    std::fs::create_dir_all(&workspace).unwrap();
    let home = skill_test_home(&workspace);
    let source_dir = unique_workspace("skill-delete-disabled-source");
    let source_path = write_skill_package(
        &source_dir,
        "disabled-cleanup",
        "disabled-cleanup",
        "Clean up disabled workspace",
        None,
    );
    let state = runtime_state_with_settings_runtime_for_workspace(workspace.clone()).await;
    let imported = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: source_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    set_skill_enabled_with_runtime_state(
        SetSkillEnabledRequest {
            id: imported.skill.id.clone(),
            enabled: false,
        },
        &state,
    )
    .await
    .unwrap();

    let deleted = delete_skill_with_runtime_state(
        DeleteSkillRequest {
            id: imported.skill.id.clone(),
        },
        &state,
    )
    .await
    .unwrap();
    let listed = list_skills_with_runtime_state(&state).await.unwrap();

    assert_eq!(deleted.id, imported.skill.id);
    assert_eq!(deleted.status, "deleted");
    assert!(!home.package_path(&imported.skill.id).exists());
    assert!(listed
        .skills
        .iter()
        .all(|skill| skill.id != imported.skill.id));
}

#[tokio::test]
async fn get_skill_detail_and_file_return_managed_skill_metadata_lazily() {
    let workspace = unique_workspace("skill-detail");
    std::fs::create_dir_all(&workspace).unwrap();
    let source_dir = unique_workspace("skill-detail-source");
    let source_path = source_dir.join("outline");
    std::fs::create_dir_all(&source_path).unwrap();
    std::fs::write(
        source_path.join("SKILL.md"),
        "---\nname: outline\ndescription: Build an outline\nparameters:\n  - name: topic\n    type: string\n    required: true\nconfig:\n  - key: STYLE_GUIDE\n    type: string\n---\nUse ${topic} and ${config.STYLE_GUIDE}.\n",
    )
    .unwrap();
    std::fs::create_dir_all(source_path.join("references")).unwrap();
    std::fs::write(
        source_path.join("references/style.md"),
        "Use terse outline headings.\n",
    )
    .unwrap();
    let source_path = source_path.canonicalize().unwrap();
    let state = runtime_state_with_settings_runtime_for_workspace(workspace).await;
    let imported = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: source_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap();

    let detail = get_skill_detail_with_runtime_state(
        GetSkillDetailRequest {
            id: imported.skill.id.clone(),
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(detail.skill.summary.name, "outline");
    assert_eq!(detail.skill.parameters[0].name, "topic");
    assert_eq!(detail.skill.config_keys, vec!["STYLE_GUIDE"]);
    assert_eq!(
        detail.skill.body_preview,
        "Use ${topic} and ${config.STYLE_GUIDE}.\n"
    );
    assert!(detail
        .skill
        .files
        .iter()
        .any(|file| file.path == "SKILL.md" && file.kind == "file"));
    assert!(detail
        .skill
        .files
        .iter()
        .any(|file| file.path == "references" && file.kind == "directory"));
    assert!(detail
        .skill
        .files
        .iter()
        .any(|file| file.path == "references/style.md" && file.kind == "file"));

    let selected = get_skill_file_with_runtime_state(
        GetSkillFileRequest {
            id: imported.skill.id,
            path: "references/style.md".to_owned(),
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(
        selected.file.content.as_str(),
        "Use terse outline headings.\n"
    );
}

#[cfg(unix)]
#[test]
fn desktop_skill_store_rejects_symlink_index_file() {
    let workspace = unique_workspace("skill-store-symlink-index");
    let external = unique_workspace("skill-store-external-target");
    let index_dir = workspace.join(".jyowo").join("skills");
    let index_path = index_dir.join("index.json");
    std::fs::create_dir_all(&index_dir).unwrap();
    std::fs::create_dir_all(&external).unwrap();
    std::fs::write(external.join("index.json"), "[]").unwrap();
    std::os::unix::fs::symlink(external.join("index.json"), &index_path).unwrap();
    let store =
        DesktopSkillStore::project(test_storage_layout_for_workspace(&workspace), workspace);

    let error = store.load_records().unwrap_err();

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert!(error.message.contains("must not use symlinks"));
}

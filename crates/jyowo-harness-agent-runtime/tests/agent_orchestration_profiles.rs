use std::fs;

use harness_agent_runtime::{AgentProfileRegistry, AgentRuntimeStore};
use harness_contracts::{
    AgentProfile, AgentProfileContextMode, AgentProfileMemoryScope, AgentProfileSandboxInheritance,
    AgentProfileScope, AgentWorkspaceIsolationMode,
};
use tempfile::{tempdir, TempDir};

fn canonical_workspace(workspace: &TempDir) -> std::path::PathBuf {
    workspace
        .path()
        .canonicalize()
        .expect("canonical workspace")
}

fn sample_user_profile(id: &str) -> AgentProfile {
    AgentProfile {
        id: id.to_owned(),
        scope: AgentProfileScope::User,
        role: "Worker".to_owned(),
        description: "User profile".to_owned(),
        model_config_override: None,
        tool_allowlist: None,
        tool_blocklist: vec![],
        sandbox_inheritance: AgentProfileSandboxInheritance::InheritParent,
        memory_scope: AgentProfileMemoryScope::ReadOnly,
        context_mode: AgentProfileContextMode::Focused,
        max_turns: 8,
        max_depth: 1,
        default_workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
    }
}

#[test]
fn list_includes_builtin_profiles_and_user_profiles() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_workspace(&workspace);
    let store = AgentRuntimeStore::open(&workspace_root).expect("store opens");
    let registry = AgentProfileRegistry::new(&store);

    registry
        .save(sample_user_profile("custom_worker"))
        .expect("save user profile");

    let profiles = registry.list().expect("list profiles");
    assert!(profiles.iter().any(|profile| profile.id == "reviewer"));
    assert!(profiles.iter().any(|profile| profile.id == "custom_worker"));
}

#[test]
fn save_list_delete_user_profile_roundtrip() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_workspace(&workspace);
    let store = AgentRuntimeStore::open(&workspace_root).expect("store opens");
    let registry = AgentProfileRegistry::new(&store);

    registry
        .save(sample_user_profile("custom_worker"))
        .expect("save profile");
    assert!(registry
        .list()
        .expect("list")
        .iter()
        .any(|profile| profile.id == "custom_worker"));

    registry.delete("custom_worker").expect("delete profile");
    assert!(!registry
        .list()
        .expect("list")
        .iter()
        .any(|profile| profile.id == "custom_worker"));
}

#[cfg(unix)]
#[test]
fn saved_user_profile_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_workspace(&workspace);
    let store = AgentRuntimeStore::open(&workspace_root).expect("store opens");
    let registry = AgentProfileRegistry::new(&store);

    registry
        .save(sample_user_profile("custom_worker"))
        .expect("save profile");

    let mode = fs::metadata(store.profiles_file_path())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn builtin_profile_delete_is_rejected() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_workspace(&workspace);
    let store = AgentRuntimeStore::open(&workspace_root).expect("store opens");
    let registry = AgentProfileRegistry::new(&store);

    let error = registry.delete("reviewer").expect_err("delete builtin");
    assert!(error.to_string().contains("read-only"));
}

#[test]
fn invalid_profile_file_is_quarantined() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_workspace(&workspace);
    let store = AgentRuntimeStore::open(&workspace_root).expect("store opens");
    let path = store.profiles_file_path();
    fs::write(&path, "{not-json").expect("write invalid profile file");

    let registry = AgentProfileRegistry::new(&store);
    let error = registry.list().expect_err("invalid profile file fails");
    assert!(matches!(
        error,
        harness_agent_runtime::AgentProfileRegistryError::Json(_)
    ));
    assert!(path.with_extension("json.invalid").exists());
}

#[test]
fn semantically_invalid_profile_file_is_quarantined() {
    let workspace = tempdir().expect("tempdir");
    let workspace_root = canonical_workspace(&workspace);
    let store = AgentRuntimeStore::open(&workspace_root).expect("store opens");
    let path = store.profiles_file_path();
    fs::write(
        &path,
        serde_json::json!({
            "profiles": [
                {
                    "id": "Invalid Profile",
                    "scope": "user",
                    "role": "Worker",
                    "description": "Invalid profile id",
                    "modelConfigOverride": null,
                    "toolAllowlist": null,
                    "toolBlocklist": [],
                    "sandboxInheritance": "inherit_parent",
                    "memoryScope": "read_only",
                    "contextMode": "focused",
                    "maxTurns": 8,
                    "maxDepth": 1,
                    "defaultWorkspaceIsolation": "read_only"
                }
            ]
        })
        .to_string(),
    )
    .expect("write semantically invalid profile file");

    let registry = AgentProfileRegistry::new(&store);
    let error = registry
        .list()
        .expect_err("semantically invalid profile file fails");
    assert!(matches!(
        error,
        harness_agent_runtime::AgentProfileRegistryError::Validation(_)
    ));
    assert!(path.with_extension("json.invalid").exists());
}

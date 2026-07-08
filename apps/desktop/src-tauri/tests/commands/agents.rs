use super::*;

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

#[tokio::test]
async fn agent_profile_commands_list_save_and_delete_user_profile() {
    let _lock = HOME_ENV_LOCK.lock().unwrap();
    let home = tempfile::tempdir().unwrap();
    let canonical_home = home.path().canonicalize().unwrap();
    let _home = EnvVarGuard::set(HOME_ENV, canonical_home.as_os_str());
    let workspace = unique_workspace("agent-profile-commands");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace.clone())
        .await
        .unwrap();
    let global_profiles_path = home
        .path()
        .join(".jyowo")
        .join("config")
        .join("agent-profiles.json");
    let old_profiles_path = workspace
        .join(".jyowo")
        .join("runtime")
        .join("agent-profiles.json");

    let listed = list_agent_profiles_with_runtime_state(&state)
        .await
        .unwrap();
    assert!(listed
        .profiles
        .iter()
        .any(|profile| profile.id == "reviewer"));
    assert!(listed.profiles.iter().any(|profile| profile.id == "worker"));

    let saved = save_agent_profile_with_runtime_state(sample_user_profile("custom_worker"), &state)
        .await
        .unwrap();
    assert_eq!(saved.status, "saved");
    assert_eq!(saved.profile.id, "custom_worker");
    assert!(global_profiles_path.is_file());
    assert!(!old_profiles_path.exists());

    let listed_after_save = list_agent_profiles_with_runtime_state(&state)
        .await
        .unwrap();
    assert!(listed_after_save
        .profiles
        .iter()
        .any(|profile| profile.id == "custom_worker"));

    let deleted = delete_agent_profile_with_runtime_state(
        DeleteAgentProfileRequest {
            id: "custom_worker".to_owned(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(deleted.status, "deleted");

    let listed_after_delete = list_agent_profiles_with_runtime_state(&state)
        .await
        .unwrap();
    assert!(!listed_after_delete
        .profiles
        .iter()
        .any(|profile| profile.id == "custom_worker"));
}

#[tokio::test]
async fn agent_profile_commands_reject_invalid_profile_id() {
    let _lock = HOME_ENV_LOCK.lock().unwrap();
    let home = tempfile::tempdir().unwrap();
    let canonical_home = home.path().canonicalize().unwrap();
    let _home = EnvVarGuard::set(HOME_ENV, canonical_home.as_os_str());
    let workspace = unique_workspace("agent-profile-invalid-id");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace).await.unwrap();

    let error = save_agent_profile_with_runtime_state(sample_user_profile("Invalid-ID"), &state)
        .await
        .expect_err("invalid profile id");

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn agent_profile_commands_reject_builtin_delete() {
    let _lock = HOME_ENV_LOCK.lock().unwrap();
    let home = tempfile::tempdir().unwrap();
    let canonical_home = home.path().canonicalize().unwrap();
    let _home = EnvVarGuard::set(HOME_ENV, canonical_home.as_os_str());
    let workspace = unique_workspace("agent-profile-builtin-delete");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace).await.unwrap();

    let error = delete_agent_profile_with_runtime_state(
        DeleteAgentProfileRequest {
            id: "reviewer".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("builtin delete");

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn agent_profile_commands_reject_invalid_global_profile_file() {
    let _lock = HOME_ENV_LOCK.lock().unwrap();
    let home = tempfile::tempdir().unwrap();
    let canonical_home = home.path().canonicalize().unwrap();
    let _home = EnvVarGuard::set(HOME_ENV, canonical_home.as_os_str());
    let workspace = unique_workspace("agent-profile-quarantine");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace.clone())
        .await
        .unwrap();
    let profile_path = home
        .path()
        .join(".jyowo")
        .join("config")
        .join("agent-profiles.json");
    std::fs::create_dir_all(profile_path.parent().unwrap()).unwrap();
    std::fs::write(&profile_path, "{not-json").unwrap();

    let error = list_agent_profiles_with_runtime_state(&state)
        .await
        .expect_err("invalid profile file");
    assert_eq!(error.code, "INVALID_PAYLOAD");
}

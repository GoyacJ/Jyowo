use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use jyowo_desktop_shell::commands::{
    get_provider_config_api_key_with_runtime_state,
    request_provider_config_api_key_reveal_with_runtime_state,
    save_provider_settings_with_runtime_state, save_provider_settings_with_store,
    CommandErrorPayload, DesktopProviderSettingsStore, DesktopRuntimeState,
    GetProviderConfigApiKeyRequest, ProviderSettingsRecord, ProviderSettingsRequest,
    ProviderSettingsStore, RequestProviderConfigApiKeyRevealRequest,
};
use jyowo_desktop_shell::storage_layout::{JyowoHome, StorageLayout};
use jyowo_harness_sdk::ext::SessionId;

fn rerun_with_isolated_home(test_name: &str) -> bool {
    const CHILD_MARKER: &str = "JYOWO_PROVIDER_TRANSACTION_CHILD";
    if std::env::var_os(CHILD_MARKER).is_some() {
        return false;
    }
    let home_dir = tempfile::tempdir().unwrap();
    let home = home_dir.path().canonicalize().unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env("HOME", home)
        .env(CHILD_MARKER, "1")
        .status()
        .unwrap();
    assert!(status.success(), "isolated test child failed");
    true
}

struct FailingSaveStore {
    record: Mutex<ProviderSettingsRecord>,
}

impl ProviderSettingsStore for FailingSaveStore {
    fn load_record(&self) -> Result<Option<ProviderSettingsRecord>, CommandErrorPayload> {
        Ok(Some(self.record.lock().unwrap().clone()))
    }

    fn save_record(&self, _record: &ProviderSettingsRecord) -> Result<(), CommandErrorPayload> {
        Err(CommandErrorPayload {
            code: "RUNTIME_OPERATION_FAILED",
            message: "injected provider persistence failure".to_owned(),
        })
    }
}

fn provider_request(
    config_id: &str,
    api_key: &str,
    model_id: &str,
    set_default: bool,
) -> ProviderSettingsRequest {
    ProviderSettingsRequest {
        api_key: Some(api_key.to_owned()),
        base_url: None,
        config_id: Some(config_id.to_owned()),
        display_name: Some(config_id.to_owned()),
        model_id: model_id.to_owned(),
        model_options: None,
        official_quota_api_key: None,
        provider_id: "openai".to_owned(),
        protocol: None,
        provider_defaults: None,
        set_default,
    }
}

async fn seeded_store(
    workspace: &std::path::Path,
) -> (DesktopProviderSettingsStore, ProviderSettingsRecord) {
    let layout = StorageLayout::new(JyowoHome::new(workspace.join(".jyowo-home")));
    let store = DesktopProviderSettingsStore::global_only_with_layout(layout);
    save_provider_settings_with_store(
        provider_request("openai", "old-token", "gpt-5.4-mini", true),
        &store,
    )
    .await
    .unwrap();
    let record = store.load_record().unwrap().unwrap();
    (store, record)
}

#[cfg(unix)]
#[tokio::test]
async fn candidate_runtime_failure_keeps_files_active_binding_and_reveal_token() {
    if rerun_with_isolated_home(
        "candidate_runtime_failure_keeps_files_active_binding_and_reveal_token",
    ) {
        return;
    }
    let workspace = tempfile::tempdir().unwrap();
    let workspace_root = workspace.path().canonicalize().unwrap();
    let (store, old_record) = seeded_store(&workspace_root).await;
    let mut state = DesktopRuntimeState::with_workspace_for_test(workspace_root).unwrap();
    state.set_provider_settings_store_for_test(Arc::new(store.clone()));
    state
        .set_active_runtime_provider_config_for_test(&old_record.configs[0])
        .unwrap();
    let old_model = state
        .settings_session_options(SessionId::new())
        .unwrap()
        .model_id;
    let reveal = request_provider_config_api_key_reveal_with_runtime_state(
        RequestProviderConfigApiKeyRevealRequest {
            config_id: "openai".to_owned(),
        },
        &state,
    )
    .await
    .unwrap();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let config_dir = home.join(".jyowo").join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::os::unix::fs::symlink(
        home.join("external-mcp-servers.json"),
        config_dir.join("mcp-servers.json"),
    )
    .unwrap();

    let error = save_provider_settings_with_runtime_state(
        provider_request("openai", "new-token", "gpt-5.4", true),
        &state,
    )
    .await
    .unwrap_err();

    assert!(error.message.contains("must not use symlinks"));
    assert_eq!(store.load_record().unwrap(), Some(old_record));
    assert!(state.settings_runtime().is_none());
    assert_eq!(
        state
            .settings_session_options(SessionId::new())
            .unwrap()
            .model_id,
        old_model
    );
    let revealed = get_provider_config_api_key_with_runtime_state(
        GetProviderConfigApiKeyRequest {
            config_id: "openai".to_owned(),
            reveal_token: reveal.reveal_token,
        },
        &state,
    )
    .await
    .expect("failed candidate runtime must preserve reveal token");
    assert_eq!(revealed.api_key, "old-token");
}

#[tokio::test]
async fn persistence_failure_keeps_active_binding_files_and_reveal_token() {
    if rerun_with_isolated_home("persistence_failure_keeps_active_binding_files_and_reveal_token") {
        return;
    }
    let workspace = tempfile::tempdir().unwrap();
    let workspace_root = workspace.path().canonicalize().unwrap();
    let (_store, old_record) = seeded_store(&workspace_root).await;
    let failing_store = Arc::new(FailingSaveStore {
        record: Mutex::new(old_record.clone()),
    });
    let mut state = DesktopRuntimeState::with_workspace_for_test(workspace_root).unwrap();
    state.set_provider_settings_store_for_test(failing_store.clone());
    state
        .set_active_runtime_provider_config_for_test(&old_record.configs[0])
        .unwrap();
    let old_model = state
        .settings_session_options(SessionId::new())
        .unwrap()
        .model_id;
    let reveal = request_provider_config_api_key_reveal_with_runtime_state(
        RequestProviderConfigApiKeyRevealRequest {
            config_id: "openai".to_owned(),
        },
        &state,
    )
    .await
    .unwrap();

    let error = save_provider_settings_with_runtime_state(
        provider_request("openai", "new-token", "gpt-5.4", true),
        &state,
    )
    .await
    .unwrap_err();

    assert!(error
        .message
        .contains("injected provider persistence failure"));
    assert_eq!(*failing_store.record.lock().unwrap(), old_record);
    assert!(state.settings_runtime().is_none());
    assert_eq!(
        state
            .settings_session_options(SessionId::new())
            .unwrap()
            .model_id,
        old_model
    );
    let revealed = get_provider_config_api_key_with_runtime_state(
        GetProviderConfigApiKeyRequest {
            config_id: "openai".to_owned(),
            reveal_token: reveal.reveal_token,
        },
        &state,
    )
    .await
    .expect("failed persistence must preserve reveal token");
    assert_eq!(revealed.api_key, "old-token");
}

#[tokio::test]
async fn successful_commit_updates_active_runtime_and_invalidates_reveal_token() {
    if rerun_with_isolated_home(
        "successful_commit_updates_active_runtime_and_invalidates_reveal_token",
    ) {
        return;
    }
    let workspace = tempfile::tempdir().unwrap();
    let workspace_root = workspace.path().canonicalize().unwrap();
    let (store, old_record) = seeded_store(&workspace_root).await;
    let mut state = DesktopRuntimeState::with_workspace_for_test(workspace_root).unwrap();
    state.set_provider_settings_store_for_test(Arc::new(store.clone()));
    state
        .set_active_runtime_provider_config_for_test(&old_record.configs[0])
        .unwrap();
    let reveal = request_provider_config_api_key_reveal_with_runtime_state(
        RequestProviderConfigApiKeyRevealRequest {
            config_id: "openai".to_owned(),
        },
        &state,
    )
    .await
    .unwrap();

    save_provider_settings_with_runtime_state(
        provider_request("openai", "new-token", "gpt-5.4", true),
        &state,
    )
    .await
    .unwrap();

    assert!(state.settings_runtime().is_some());
    assert_eq!(
        state
            .settings_session_options(SessionId::new())
            .unwrap()
            .model_id,
        Some("gpt-5.4".to_owned())
    );
    assert_eq!(
        store.load_record().unwrap().unwrap().configs[0].api_key,
        "new-token"
    );
    let error = get_provider_config_api_key_with_runtime_state(
        GetProviderConfigApiKeyRequest {
            config_id: "openai".to_owned(),
            reveal_token: reveal.reveal_token,
        },
        &state,
    )
    .await
    .unwrap_err();
    assert!(error.message.contains("invalid or expired"));
}

#[tokio::test]
async fn non_default_commit_does_not_replace_active_runtime() {
    if rerun_with_isolated_home("non_default_commit_does_not_replace_active_runtime") {
        return;
    }
    let workspace = tempfile::tempdir().unwrap();
    let workspace_root = workspace.path().canonicalize().unwrap();
    let (store, old_record) = seeded_store(&workspace_root).await;
    let mut state = DesktopRuntimeState::with_workspace_for_test(workspace_root).unwrap();
    state.set_provider_settings_store_for_test(Arc::new(store.clone()));
    state
        .set_active_runtime_provider_config_for_test(&old_record.configs[0])
        .unwrap();

    let response = save_provider_settings_with_runtime_state(
        provider_request("secondary", "secondary-token", "gpt-5.4", false),
        &state,
    )
    .await
    .unwrap();

    assert!(!response.config.is_default);
    assert!(state.settings_runtime().is_none());
    assert_eq!(
        state
            .settings_session_options(SessionId::new())
            .unwrap()
            .model_id,
        Some("gpt-5.4-mini".to_owned())
    );
    assert_eq!(
        store
            .load_record()
            .unwrap()
            .unwrap()
            .default_config_id
            .as_deref(),
        Some("openai")
    );
}

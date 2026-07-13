use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fs2::FileExt;
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

const GENERATION_READER_CHILD: &str = "provider_generation_reader_child";
const GENERATION_CRASH_WRITER_CHILD: &str = "provider_generation_crash_writer_child";

fn spawn_generation_reader(
    config_root: &Path,
    started_path: &Path,
    output_path: &Path,
    expect_blocked: bool,
) -> Child {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .arg("--exact")
        .arg(GENERATION_READER_CHILD)
        .arg("--nocapture")
        .env("JYOWO_PROVIDER_GENERATION_CONFIG_ROOT", config_root)
        .env("JYOWO_PROVIDER_GENERATION_READER_STARTED", started_path)
        .env("JYOWO_PROVIDER_GENERATION_READER_OUTPUT", output_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if expect_blocked {
        command.env("JYOWO_PROVIDER_GENERATION_READER_EXPECT_BLOCKED", "1");
    }
    command.spawn().expect("spawn provider generation reader")
}

fn wait_for_path(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(Instant::now() < deadline, "timed out waiting for {path:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn provider_generation_reader_child() {
    let Some(config_root) = std::env::var_os("JYOWO_PROVIDER_GENERATION_CONFIG_ROOT") else {
        return;
    };
    let config_root = PathBuf::from(config_root);
    let started_path = PathBuf::from(
        std::env::var_os("JYOWO_PROVIDER_GENERATION_READER_STARTED").expect("reader started path"),
    );
    let output_path = PathBuf::from(
        std::env::var_os("JYOWO_PROVIDER_GENERATION_READER_OUTPUT").expect("reader output path"),
    );
    if std::env::var_os("JYOWO_PROVIDER_GENERATION_READER_EXPECT_BLOCKED").is_some() {
        let lock_file = open_generation_lock(&config_root);
        let error = FileExt::try_lock_shared(&lock_file)
            .expect_err("writer must hold the provider generation lock");
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    }
    std::fs::write(&started_path, b"started").expect("signal reader start");
    let layout = StorageLayout::new(JyowoHome::new(
        config_root.parent().expect("config root parent"),
    ));
    let record = DesktopProviderSettingsStore::global_only_with_layout(layout)
        .load_record()
        .expect("load provider generation")
        .expect("provider generation exists");
    let selected = record.configs.first().expect("provider config exists");
    std::fs::write(
        output_path,
        serde_json::to_vec(&serde_json::json!({
            "configId": selected.id,
            "apiKey": selected.api_key,
            "defaultConfigId": record.default_config_id,
        }))
        .expect("serialize reader output"),
    )
    .expect("write reader output");
}

#[test]
fn provider_generation_crash_writer_child() {
    let Some(config_root) = std::env::var_os("JYOWO_PROVIDER_GENERATION_CRASH_CONFIG_ROOT") else {
        return;
    };
    let config_root = PathBuf::from(config_root);
    let lock_file = open_generation_lock(&config_root);
    FileExt::lock_exclusive(&lock_file).expect("lock provider generation");
    let profiles_path = config_root.join("provider-profiles.json");
    let secrets_path = config_root.join("provider-secrets.json");
    let selection_path = config_root.join("provider-selection.json");
    let marker_path = config_root.join("provider-generation.recovery.json");
    let marker = serde_json::json!({
        "version": 1,
        "profiles": std::fs::read(&profiles_path).ok(),
        "secrets": std::fs::read(&secrets_path).ok(),
        "selection": std::fs::read(&selection_path).ok(),
    });
    let mut marker_options = std::fs::OpenOptions::new();
    marker_options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        marker_options.mode(0o600);
    }
    let mut marker_file = marker_options
        .open(&marker_path)
        .expect("create recovery marker");
    use std::io::Write;
    marker_file
        .write_all(&serde_json::to_vec_pretty(&marker).unwrap())
        .and_then(|()| marker_file.sync_all())
        .expect("persist recovery marker");
    std::fs::File::open(&config_root)
        .and_then(|directory| directory.sync_all())
        .expect("sync config directory");

    let mut profiles: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&profiles_path).unwrap()).unwrap();
    profiles[0]["id"] = serde_json::Value::String("crashed-new".to_owned());
    profiles[0]["displayName"] = serde_json::Value::String("crashed-new".to_owned());
    let staged_path = config_root.join("provider-profiles.crash-stage.json");
    write_json(&staged_path, &profiles);
    std::fs::rename(staged_path, profiles_path).expect("replace provider profiles");
    std::process::exit(86);
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

fn open_generation_lock(config_root: &Path) -> std::fs::File {
    std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(config_root.join("provider-generation.lock"))
        .expect("open provider generation lock")
}

fn write_json(path: &Path, value: &(impl serde::Serialize + ?Sized)) {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("serialize JSON"),
    )
    .expect("write JSON");
}

#[tokio::test]
async fn provider_reader_waits_for_complete_generation_commit() {
    let workspace = tempfile::tempdir().unwrap();
    let workspace_root = workspace.path().canonicalize().unwrap();
    let (_store, _) = seeded_store(&workspace_root).await;
    let layout = StorageLayout::new(JyowoHome::new(workspace_root.join(".jyowo-home")));
    let config_root = layout.global_config_root();
    let lock_file = open_generation_lock(&config_root);
    FileExt::lock_exclusive(&lock_file).expect("lock provider generation");

    let profiles_path = config_root.join("provider-profiles.json");
    let mut profiles: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&profiles_path).unwrap()).unwrap();
    profiles[0]["id"] = serde_json::Value::String("new".to_owned());
    profiles[0]["displayName"] = serde_json::Value::String("new".to_owned());
    write_json(&profiles_path, &profiles);

    let started_path = workspace_root.join("commit-reader-started");
    let output_path = workspace_root.join("commit-reader-output.json");
    let mut child = spawn_generation_reader(&config_root, &started_path, &output_path, true);
    wait_for_path(&started_path);

    write_json(
        &config_root.join("provider-secrets.json"),
        &serde_json::json!([{"configId": "new", "apiKey": "new-secret"}]),
    );
    write_json(
        &config_root.join("provider-selection.json"),
        &serde_json::json!({"defaultConfigId": "new"}),
    );
    FileExt::unlock(&lock_file).expect("unlock provider generation");
    assert!(child.wait().expect("wait reader").success());
    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output_path).expect("read reader output"))
            .expect("decode reader output");

    assert_eq!(result["configId"], "new");
    assert_eq!(result["apiKey"], "new-secret");
    assert_eq!(result["defaultConfigId"], "new");
}

#[tokio::test]
async fn provider_reader_waits_for_complete_generation_rollback() {
    let workspace = tempfile::tempdir().unwrap();
    let workspace_root = workspace.path().canonicalize().unwrap();
    let (_store, _) = seeded_store(&workspace_root).await;
    let layout = StorageLayout::new(JyowoHome::new(workspace_root.join(".jyowo-home")));
    let config_root = layout.global_config_root();
    let profiles_path = config_root.join("provider-profiles.json");
    let old_profiles = std::fs::read(&profiles_path).unwrap();
    let lock_file = open_generation_lock(&config_root);
    FileExt::lock_exclusive(&lock_file).expect("lock provider generation");

    let mut profiles: serde_json::Value = serde_json::from_slice(&old_profiles).unwrap();
    profiles[0]["id"] = serde_json::Value::String("new".to_owned());
    profiles[0]["displayName"] = serde_json::Value::String("new".to_owned());
    write_json(&profiles_path, &profiles);

    let started_path = workspace_root.join("rollback-reader-started");
    let output_path = workspace_root.join("rollback-reader-output.json");
    let mut child = spawn_generation_reader(&config_root, &started_path, &output_path, true);
    wait_for_path(&started_path);

    std::fs::write(&profiles_path, old_profiles).unwrap();
    FileExt::unlock(&lock_file).expect("unlock provider generation");
    assert!(child.wait().expect("wait reader").success());
    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output_path).expect("read reader output"))
            .expect("decode reader output");

    assert_eq!(result["configId"], "openai");
    assert_eq!(result["apiKey"], "old-token");
    assert_eq!(result["defaultConfigId"], "openai");
}

#[tokio::test]
async fn provider_reader_recovers_generation_after_writer_process_crash() {
    let workspace = tempfile::tempdir().unwrap();
    let workspace_root = workspace.path().canonicalize().unwrap();
    let (_store, _) = seeded_store(&workspace_root).await;
    let layout = StorageLayout::new(JyowoHome::new(workspace_root.join(".jyowo-home")));
    let config_root = layout.global_config_root();
    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg(GENERATION_CRASH_WRITER_CHILD)
        .arg("--nocapture")
        .env("JYOWO_PROVIDER_GENERATION_CRASH_CONFIG_ROOT", &config_root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run crashing provider writer");
    assert_eq!(status.code(), Some(86));

    let started_path = workspace_root.join("crash-reader-started");
    let output_path = workspace_root.join("crash-reader-output.json");
    let mut child = spawn_generation_reader(&config_root, &started_path, &output_path, false);
    assert!(child.wait().expect("wait recovery reader").success());
    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output_path).expect("read reader output"))
            .expect("decode reader output");

    assert_eq!(result["configId"], "openai");
    assert_eq!(result["apiKey"], "old-token");
    assert_eq!(result["defaultConfigId"], "openai");
    assert!(!config_root
        .join("provider-generation.recovery.json")
        .exists());
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

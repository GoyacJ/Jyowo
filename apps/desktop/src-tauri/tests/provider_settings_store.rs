#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;

use jyowo_desktop_shell::commands::{
    provider_settings_process_lock_for_test, save_provider_settings_with_store,
    DesktopProviderSettingsStore, ProviderSettingsRecord, ProviderSettingsRequest,
    ProviderSettingsStore,
};
use jyowo_desktop_shell::storage_layout::{JyowoHome, StorageLayout};

struct ImmutableFile(PathBuf);

impl ImmutableFile {
    fn new(path: PathBuf) -> Self {
        let status = Command::new("chflags")
            .arg("uchg")
            .arg(&path)
            .status()
            .expect("run chflags uchg");
        assert!(status.success(), "mark test file immutable");
        Self(path)
    }
}

impl Drop for ImmutableFile {
    fn drop(&mut self) {
        let _ = Command::new("chflags").arg("nouchg").arg(&self.0).status();
    }
}

fn store(root: &Path) -> DesktopProviderSettingsStore {
    DesktopProviderSettingsStore::global_only_with_layout(StorageLayout::new(JyowoHome::new(
        root.join(".jyowo"),
    )))
}

fn request(config_id: &str, api_key: &str) -> ProviderSettingsRequest {
    ProviderSettingsRequest {
        api_key: Some(api_key.to_owned()),
        base_url: None,
        config_id: Some(config_id.to_owned()),
        display_name: Some(config_id.to_owned()),
        model_id: "gpt-5.4-mini".to_owned(),
        model_options: None,
        official_quota_api_key: None,
        provider_id: "openai".to_owned(),
        protocol: None,
        provider_defaults: None,
        set_default: true,
    }
}

async fn record(root: &Path, config_id: &str, api_key: &str) -> ProviderSettingsRecord {
    let store = store(root);
    save_provider_settings_with_store(request(config_id, api_key), &store)
        .await
        .unwrap();
    store.load_record().unwrap().unwrap()
}

fn generation_paths(root: &Path) -> [PathBuf; 3] {
    let config = root.join(".jyowo").join("config");
    [
        config.join("provider-profiles.json"),
        config.join("provider-secrets.json"),
        config.join("provider-selection.json"),
    ]
}

fn read_generation(paths: &[PathBuf; 3]) -> [Vec<u8>; 3] {
    paths
        .clone()
        .map(|path| std::fs::read(path).expect("read provider generation"))
}

#[tokio::test]
async fn all_store_instances_share_one_process_lock_for_load_and_save() {
    let root_dir = tempfile::tempdir().unwrap();
    let root = root_dir.path().canonicalize().unwrap();
    let candidate_dir = tempfile::tempdir().unwrap();
    let candidate_root = candidate_dir.path().canonicalize().unwrap();
    let old_record = record(&root, "old", "old-token").await;
    let new_record = record(&candidate_root, "new", "new-token").await;
    let load_store = store(&root);
    let save_store = store(&root);

    let process_lock = provider_settings_process_lock_for_test();
    let write_guard = process_lock.write().unwrap();
    let (load_ready_tx, load_ready_rx) = std::sync::mpsc::channel();
    let (load_tx, load_rx) = std::sync::mpsc::channel();
    let load_thread = std::thread::spawn(move || {
        load_ready_tx.send(()).unwrap();
        load_tx.send(load_store.load_record()).unwrap();
    });
    load_ready_rx.recv().unwrap();
    assert!(load_rx
        .recv_timeout(std::time::Duration::from_millis(100))
        .is_err());
    drop(write_guard);
    assert_eq!(
        load_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
            .unwrap(),
        Some(old_record)
    );
    load_thread.join().unwrap();

    let read_guard = process_lock.read().unwrap();
    let (save_ready_tx, save_ready_rx) = std::sync::mpsc::channel();
    let (save_tx, save_rx) = std::sync::mpsc::channel();
    let save_thread = std::thread::spawn(move || {
        save_ready_tx.send(()).unwrap();
        save_tx.send(save_store.save_record(&new_record)).unwrap();
    });
    save_ready_rx.recv().unwrap();
    assert!(save_rx
        .recv_timeout(std::time::Duration::from_millis(100))
        .is_err());
    drop(read_guard);
    save_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap()
        .unwrap();
    save_thread.join().unwrap();
}

#[tokio::test]
async fn profiles_write_failure_keeps_old_generation() {
    let root_dir = tempfile::tempdir().unwrap();
    let root = root_dir.path().canonicalize().unwrap();
    let candidate_dir = tempfile::tempdir().unwrap();
    let candidate_root = candidate_dir.path().canonicalize().unwrap();
    let old_record = record(&root, "old", "old-token").await;
    let new_record = record(&candidate_root, "new", "new-token").await;
    let paths = generation_paths(&root);
    let old_bytes = read_generation(&paths);
    let _immutable = ImmutableFile::new(paths[0].clone());

    let error = store(&root).save_record(&new_record).unwrap_err();

    assert!(error.message.contains("provider profiles"));
    assert_eq!(read_generation(&paths), old_bytes);
    assert_eq!(old_record.default_config_id.as_deref(), Some("old"));
}

#[tokio::test]
async fn secrets_write_failure_restores_profiles_and_keeps_old_generation() {
    let root_dir = tempfile::tempdir().unwrap();
    let root = root_dir.path().canonicalize().unwrap();
    let candidate_dir = tempfile::tempdir().unwrap();
    let candidate_root = candidate_dir.path().canonicalize().unwrap();
    let old_record = record(&root, "old", "old-token").await;
    let new_record = record(&candidate_root, "new", "new-token").await;
    let paths = generation_paths(&root);
    let old_bytes = read_generation(&paths);
    let immutable = ImmutableFile::new(paths[1].clone());

    let error = store(&root).save_record(&new_record).unwrap_err();

    assert!(error.message.contains("provider secrets"));
    assert!(error.message.contains("rollback failed"));
    assert_eq!(read_generation(&paths), old_bytes);
    drop(immutable);
    assert_eq!(store(&root).load_record().unwrap(), Some(old_record));
}

#[tokio::test]
async fn secrets_write_failure_removes_new_profile_when_old_profile_was_missing() {
    let root_dir = tempfile::tempdir().unwrap();
    let root = root_dir.path().canonicalize().unwrap();
    let candidate_dir = tempfile::tempdir().unwrap();
    let candidate_root = candidate_dir.path().canonicalize().unwrap();
    record(&root, "old", "old-token").await;
    let new_record = record(&candidate_root, "new", "new-token").await;
    let paths = generation_paths(&root);
    std::fs::remove_file(&paths[0]).unwrap();
    let old_secret = std::fs::read(&paths[1]).unwrap();
    let old_selection = std::fs::read(&paths[2]).unwrap();
    let _immutable = ImmutableFile::new(paths[1].clone());

    store(&root).save_record(&new_record).unwrap_err();

    assert!(!paths[0].exists());
    assert_eq!(std::fs::read(&paths[1]).unwrap(), old_secret);
    assert_eq!(std::fs::read(&paths[2]).unwrap(), old_selection);
}

#[tokio::test]
async fn selection_write_failure_restores_profiles_and_secrets() {
    let root_dir = tempfile::tempdir().unwrap();
    let root = root_dir.path().canonicalize().unwrap();
    let candidate_dir = tempfile::tempdir().unwrap();
    let candidate_root = candidate_dir.path().canonicalize().unwrap();
    let old_record = record(&root, "old", "old-token").await;
    let new_record = record(&candidate_root, "new", "new-token").await;
    let paths = generation_paths(&root);
    let old_bytes = read_generation(&paths);
    let _immutable = ImmutableFile::new(paths[2].clone());

    let error = store(&root).save_record(&new_record).unwrap_err();

    assert!(error.message.contains("provider selection"));
    assert!(error.message.contains("rollback failed"));
    assert_eq!(read_generation(&paths), old_bytes);
    assert_eq!(store(&root).load_record().unwrap(), Some(old_record));
}

#[cfg(unix)]
#[tokio::test]
async fn successful_generation_replaces_all_secrets_and_keeps_owner_only_mode() {
    use std::os::unix::fs::PermissionsExt;

    let root_dir = tempfile::tempdir().unwrap();
    let root = root_dir.path().canonicalize().unwrap();
    let store = store(&root);
    save_provider_settings_with_store(request("old", "old-token"), &store)
        .await
        .unwrap();
    save_provider_settings_with_store(request("new", "new-token"), &store)
        .await
        .unwrap();
    let mut generation = store.load_record().unwrap().unwrap();
    generation.configs.retain(|config| config.id == "new");
    generation.default_config_id = Some("new".to_owned());

    store.save_record(&generation).unwrap();

    let paths = generation_paths(&root);
    let secrets: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(&paths[1]).unwrap()).unwrap();
    assert_eq!(secrets.len(), 1);
    assert_eq!(secrets[0]["configId"], "new");
    assert_eq!(
        std::fs::metadata(&paths[1]).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

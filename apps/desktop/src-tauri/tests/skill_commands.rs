use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};

use harness_contracts::{NoopRedactor, SkillConfigDocument, SkillStatus};
use harness_skill::{
    parse_skill_markdown, SkillConfigDecl, SkillParamType, SkillPlatform, SkillSource,
};
use jyowo_desktop_shell::commands::stores::{
    DesktopSkillConfigStore, GlobalConfigStore, SkillConfigStoreFault,
};
use jyowo_desktop_shell::commands::{
    get_skill_config_with_runtime_state, get_skill_detail_with_runtime_state,
    import_skill_with_runtime_state, list_skills_with_runtime_state,
    reload_desktop_settings_runtime_after_plugin_change_for_test,
    runtime_state_with_skill_config_store_for_test, set_skill_config_value_with_runtime_state,
    set_skill_enabled_with_runtime_state, DesktopRuntimeState, GetSkillConfigRequest,
    GetSkillDetailRequest, ImportSkillRequest, SetSkillConfigValueRequest, SetSkillEnabledRequest,
    SkillStore, SkillStoreRecord,
};
use jyowo_desktop_shell::storage_layout::{JyowoHome, StorageLayout};
use jyowo_harness_sdk::ext::StreamBrokerConfig;
use jyowo_harness_sdk::skill_config::{SecretString, SkillConfigStoreError, SkillSecretStore};
use jyowo_harness_sdk::testing::{InMemoryEventStore, NoopSandbox, TestModelProvider};
use jyowo_harness_sdk::{DesktopSettingsRuntime, HarnessOptions, StreamPermissionRuntime};
use secrecy::ExposeSecret;
use serde_json::json;

#[derive(Debug, Default)]
struct MemorySecretStore {
    values: Mutex<BTreeMap<String, SecretString>>,
}

#[derive(Debug, Default)]
struct BlockingLoadGate {
    gate: Mutex<Option<(SyncSender<()>, Receiver<()>)>>,
}

#[tokio::test]
async fn package_integrity_list_marks_a_tampered_installed_skill_rejected() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().canonicalize().unwrap();
    let layout = test_layout(&workspace);
    let settings_runtime = test_settings_runtime(&workspace).await;
    let mut state = DesktopRuntimeState::with_settings_runtime_for_workspace(
        workspace.clone(),
        settings_runtime,
    )
    .unwrap();
    let skill_store = Arc::new(jyowo_desktop_shell::commands::DesktopSkillStore::global(
        layout.clone(),
    ));
    state.set_skill_store_for_test(skill_store.clone());
    state.set_config_stores_for_test(GlobalConfigStore::new(layout.clone()), None);

    let source = workspace.join("source-skill");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: integrity-test\ndescription: Integrity test\n---\nOriginal body.\n",
    )
    .unwrap();
    let imported = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: source.to_string_lossy().into_owned(),
        },
        &state,
    )
    .await
    .unwrap();
    let package = layout
        .global_skills_root()
        .join("packages")
        .join(&imported.skill.id);
    std::fs::write(
        package.join("SKILL.md"),
        "---\nname: integrity-test\ndescription: Integrity test\n---\nTampered body.\n",
    )
    .unwrap();

    let response = list_skills_with_runtime_state(&state).await.unwrap();
    let summary = response
        .skills
        .iter()
        .find(|skill| skill.id == imported.skill.id)
        .unwrap();
    assert_eq!(summary.status, "rejected");

    let detail = get_skill_detail_with_runtime_state(
        GetSkillDetailRequest {
            id: imported.skill.id.clone(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(detail.skill.summary.status, "rejected");
    assert!(detail.skill.validation_error.is_some());

    set_skill_enabled_with_runtime_state(
        SetSkillEnabledRequest {
            id: imported.skill.id.clone(),
            enabled: false,
        },
        &state,
    )
    .await
    .unwrap();
    let enabled = set_skill_enabled_with_runtime_state(
        SetSkillEnabledRequest {
            id: imported.skill.id.clone(),
            enabled: true,
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(enabled.skill.status, "rejected");
    assert!(state
        .settings_runtime()
        .unwrap()
        .view_runtime_skill("integrity-test", false)
        .unwrap()
        .is_none());
}

impl BlockingLoadGate {
    fn arm(&self) -> (Receiver<()>, SyncSender<()>) {
        let (entered_tx, entered_rx) = sync_channel(1);
        let (release_tx, release_rx) = sync_channel(1);
        *self.gate.lock().unwrap() = Some((entered_tx, release_rx));
        (entered_rx, release_tx)
    }

    fn block_if_armed(&self) {
        if let Some((entered, release)) = self.gate.lock().unwrap().take() {
            entered.send(()).unwrap();
            release.recv().unwrap();
        }
    }
}

#[tokio::test]
async fn initial_runtime_loads_the_persisted_global_skill_config_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().canonicalize().unwrap();
    let store = Arc::new(DesktopSkillConfigStore::new(
        test_layout(&workspace),
        Arc::new(MemorySecretStore::default()),
    ));
    store
        .set_public_value(
            "user:persisted-config-bootstrap-test",
            &required_public_config("region"),
            json!("cn-east"),
        )
        .unwrap();

    let state = runtime_state_with_skill_config_store_for_test(workspace, store)
        .await
        .unwrap();
    let runtime = state.settings_runtime().unwrap();
    runtime
        .skill_registry()
        .register_batch(vec![persisted_config_skill()])
        .unwrap();

    assert_eq!(
        runtime
            .view_runtime_skill("persisted-config-bootstrap-test", false)
            .unwrap()
            .unwrap()
            .summary
            .status,
        SkillStatus::Ready
    );
}

#[tokio::test]
async fn plugin_rebuild_reloads_the_persisted_global_skill_config_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().canonicalize().unwrap();
    let store = Arc::new(DesktopSkillConfigStore::new(
        test_layout(&workspace),
        Arc::new(MemorySecretStore::default()),
    ));
    let state = runtime_state_with_skill_config_store_for_test(workspace, store.clone())
        .await
        .unwrap();
    let initial_runtime = state.settings_runtime().unwrap();
    initial_runtime
        .skill_registry()
        .register_batch(vec![persisted_config_skill()])
        .unwrap();
    assert!(matches!(
        initial_runtime
            .view_runtime_skill("persisted-config-bootstrap-test", false)
            .unwrap()
            .unwrap()
            .summary
            .status,
        SkillStatus::PrerequisiteMissing { .. }
    ));
    store
        .set_public_value(
            "user:persisted-config-bootstrap-test",
            &required_public_config("region"),
            json!("cn-east"),
        )
        .unwrap();

    reload_desktop_settings_runtime_after_plugin_change_for_test(&state)
        .await
        .unwrap();
    let rebuilt_runtime = state.settings_runtime().unwrap();
    rebuilt_runtime
        .skill_registry()
        .register_batch(vec![persisted_config_skill()])
        .unwrap();

    assert_eq!(
        rebuilt_runtime
            .view_runtime_skill("persisted-config-bootstrap-test", false)
            .unwrap()
            .unwrap()
            .summary
            .status,
        SkillStatus::Ready
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skill_config_mutation_waits_for_an_in_flight_runtime_rebuild() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().canonicalize().unwrap();
    let snapshot_load_gate = Arc::new(BlockingLoadGate::default());
    let hook_gate = snapshot_load_gate.clone();
    let store = Arc::new(
        DesktopSkillConfigStore::new(
            test_layout(&workspace),
            Arc::new(MemorySecretStore::default()),
        )
        .with_snapshot_load_hook_for_test(Arc::new(move || hook_gate.block_if_armed())),
    );
    let state = runtime_state_with_skill_config_store_for_test(workspace, store)
        .await
        .unwrap();
    let (snapshot_load_entered, release_snapshot_load) = snapshot_load_gate.arm();

    let reload_state = state.clone();
    let reload = tokio::spawn(async move {
        reload_desktop_settings_runtime_after_plugin_change_for_test(&reload_state).await
    });
    tokio::task::spawn_blocking(move || {
        snapshot_load_entered
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("runtime rebuild must reach skill snapshot loading");
    })
    .await
    .unwrap();

    let mutation_state = state.clone();
    let mut mutation = tokio::spawn(async move {
        set_skill_config_value_with_runtime_state(
            SetSkillConfigValueRequest {
                skill_id: "missing-skill".to_owned(),
                key: "region".to_owned(),
                value: json!("cn-east"),
            },
            &mutation_state,
        )
        .await
    });
    let mutation_before_release =
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut mutation).await;
    release_snapshot_load.send(()).unwrap();
    reload.await.unwrap().unwrap();
    match mutation_before_release {
        Ok(result) => {
            let _ = result.unwrap();
            panic!("skill config mutation bypassed the runtime rebuild lock");
        }
        Err(_) => {
            let error = mutation
                .await
                .unwrap()
                .expect_err("invalid mutation must run only after rebuild releases the lock");
            assert_eq!(error.code, "INVALID_PAYLOAD");
        }
    }
}

#[tokio::test]
async fn skill_config_commands_share_one_canonical_namespace() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().canonicalize().unwrap();
    let layout = test_layout(&workspace);
    let settings_runtime = test_settings_runtime(&workspace).await;
    let skill = parse_skill_markdown(
        "---\nname: configured\ndescription: Configured skill\nconfig:\n  - key: region\n    type: string\n---\nUse ${config.region}.\n",
        SkillSource::User("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .unwrap();
    settings_runtime
        .skill_registry()
        .register_batch(vec![skill])
        .unwrap();
    let canonical_id = settings_runtime
        .list_runtime_skills()
        .unwrap()
        .into_iter()
        .find(|skill| skill.name == "configured")
        .unwrap()
        .id;
    assert!(settings_runtime
        .view_runtime_skill("configured", false)
        .unwrap()
        .is_some());
    let mut state = DesktopRuntimeState::with_settings_runtime_for_workspace(
        workspace.clone(),
        settings_runtime,
    )
    .unwrap();
    let skill_store = Arc::new(jyowo_desktop_shell::commands::DesktopSkillStore::global(
        layout.clone(),
    ));
    state.set_skill_store_for_test(skill_store.clone());
    let skill_index = layout.global_skills_root().join("index.json");
    std::fs::create_dir_all(skill_index.parent().unwrap()).unwrap();
    std::fs::write(
        skill_index,
        serde_json::to_vec_pretty(&vec![SkillStoreRecord {
            id: "managed-record-id".to_owned(),
            name: "configured".to_owned(),
            description: "Configured skill".to_owned(),
            enabled: true,
            content_hash: "test-hash".to_owned(),
            package_dir: "managed-record-id".to_owned(),
            file_name: String::new(),
            imported_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: "2026-01-01T00:00:00Z".to_owned(),
            tags: Vec::new(),
            category: None,
            last_validation_error: None,
            origin: None,
        }])
        .unwrap(),
    )
    .unwrap();
    assert_eq!(skill_store.load_records().unwrap()[0].name, "configured");
    assert!(state
        .settings_runtime()
        .unwrap()
        .view_runtime_skill("configured", false)
        .unwrap()
        .is_some());

    for request_id in [
        "managed-record-id".to_owned(),
        "configured".to_owned(),
        canonical_id.clone(),
    ] {
        let response = get_skill_config_with_runtime_state(
            GetSkillConfigRequest {
                skill_id: request_id.clone(),
            },
            &state,
        )
        .unwrap_or_else(|error| panic!("request {request_id} failed: {error:?}"));
        assert_eq!(response.skill_id, canonical_id);
    }
}

#[tokio::test]
async fn managed_record_id_does_not_resolve_to_a_workspace_skill_with_the_same_name() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().canonicalize().unwrap();
    let layout = test_layout(&workspace);
    let settings_runtime = test_settings_runtime(&workspace).await;
    let user_skill = parse_skill_markdown(
        "---\nname: configured\ndescription: User skill\n---\nUser.\n",
        SkillSource::User("user/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .unwrap();
    let workspace_skill = parse_skill_markdown(
        "---\nname: configured\ndescription: Workspace skill\n---\nWorkspace.\n",
        SkillSource::Workspace("workspace/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .unwrap();
    settings_runtime
        .skill_registry()
        .register_batch(vec![user_skill, workspace_skill])
        .unwrap();
    assert_eq!(
        settings_runtime
            .view_runtime_skill("configured", false)
            .unwrap()
            .unwrap()
            .summary
            .id,
        "workspace:configured"
    );

    let mut state =
        DesktopRuntimeState::with_settings_runtime_for_workspace(workspace, settings_runtime)
            .unwrap();
    state.set_skill_store_for_test(Arc::new(
        jyowo_desktop_shell::commands::DesktopSkillStore::global(layout.clone()),
    ));
    let skill_index = layout.global_skills_root().join("index.json");
    std::fs::create_dir_all(skill_index.parent().unwrap()).unwrap();
    std::fs::write(
        skill_index,
        serde_json::to_vec_pretty(&vec![SkillStoreRecord {
            id: "managed-record-id".to_owned(),
            name: "configured".to_owned(),
            description: "User skill".to_owned(),
            enabled: true,
            content_hash: "test-hash".to_owned(),
            package_dir: "managed-record-id".to_owned(),
            file_name: String::new(),
            imported_at: "2026-01-01T00:00:00Z".to_owned(),
            updated_at: "2026-01-01T00:00:00Z".to_owned(),
            tags: Vec::new(),
            category: None,
            last_validation_error: None,
            origin: None,
        }])
        .unwrap(),
    )
    .unwrap();

    let error = get_skill_config_with_runtime_state(
        GetSkillConfigRequest {
            skill_id: "managed-record-id".to_owned(),
        },
        &state,
    )
    .expect_err("managed record must not bind to a shadowing workspace skill");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("shadowed"));
}

#[tokio::test]
async fn skill_config_mutation_refreshes_the_current_settings_runtime_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().canonicalize().unwrap();
    let layout = test_layout(&workspace);
    let settings_runtime = test_settings_runtime(&workspace).await;
    let skill = parse_skill_markdown(
        "---\nname: configured\ndescription: Configured skill\nconfig:\n  - key: region\n    type: string\n    required: true\n---\nUse ${config.region}.\n",
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .unwrap();
    settings_runtime
        .skill_registry()
        .register_batch(vec![skill])
        .unwrap();
    assert!(matches!(
        settings_runtime
            .view_runtime_skill("configured", false)
            .unwrap()
            .unwrap()
            .summary
            .status,
        SkillStatus::PrerequisiteMissing { .. }
    ));

    let mut state = DesktopRuntimeState::with_settings_runtime_for_workspace(
        workspace,
        settings_runtime.clone(),
    )
    .unwrap();
    state.set_skill_config_store_for_test(Arc::new(DesktopSkillConfigStore::new(
        layout,
        Arc::new(MemorySecretStore::default()),
    )));

    set_skill_config_value_with_runtime_state(
        SetSkillConfigValueRequest {
            skill_id: "workspace:configured".to_owned(),
            key: "region".to_owned(),
            value: json!("cn-east"),
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(
        settings_runtime
            .view_runtime_skill("configured", false)
            .unwrap()
            .unwrap()
            .summary
            .status,
        SkillStatus::Ready
    );
}

#[tokio::test]
async fn get_skill_config_projects_the_persisted_entry_through_current_declarations() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().canonicalize().unwrap();
    let layout = test_layout(&workspace);
    let settings_runtime = test_settings_runtime(&workspace).await;
    let skill = parse_skill_markdown(
        "---\nname: configured\ndescription: Configured skill\nconfig:\n  - key: apiToken\n    type: string\n    secret: true\n---\nConfigured.\n",
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .unwrap();
    settings_runtime
        .skill_registry()
        .register_batch(vec![skill])
        .unwrap();
    let config_store = Arc::new(DesktopSkillConfigStore::new(
        layout,
        Arc::new(MemorySecretStore::default()),
    ));
    config_store
        .set_public_value(
            "workspace:configured",
            &public_config("apiToken"),
            json!("legacy-plaintext"),
        )
        .unwrap();
    let mut state =
        DesktopRuntimeState::with_settings_runtime_for_workspace(workspace, settings_runtime)
            .unwrap();
    state.set_skill_config_store_for_test(config_store);

    let response = get_skill_config_with_runtime_state(
        GetSkillConfigRequest {
            skill_id: "workspace:configured".to_owned(),
        },
        &state,
    )
    .unwrap();

    assert!(!response.config.values.contains_key("apiToken"));
    assert!(!response.config.secrets["apiToken"].configured);
    assert!(!serde_json::to_string(&response)
        .unwrap()
        .contains("legacy-plaintext"));
}

#[tokio::test]
async fn get_skill_config_propagates_secret_store_failure() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().canonicalize().unwrap();
    let layout = test_layout(&workspace);
    let settings_runtime = test_settings_runtime(&workspace).await;
    let skill = parse_skill_markdown(
        "---\nname: configured\ndescription: Configured skill\nconfig:\n  - key: apiToken\n    type: string\n    secret: true\n---\nConfigured.\n",
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .unwrap();
    settings_runtime
        .skill_registry()
        .register_batch(vec![skill])
        .unwrap();
    let mut state =
        DesktopRuntimeState::with_settings_runtime_for_workspace(workspace, settings_runtime)
            .unwrap();
    state.set_skill_config_store_for_test(Arc::new(DesktopSkillConfigStore::new(
        layout,
        Arc::new(UnavailableSecretStore),
    )));

    let error = get_skill_config_with_runtime_state(
        GetSkillConfigRequest {
            skill_id: "workspace:configured".to_owned(),
        },
        &state,
    )
    .expect_err("secure-store failure must not be shown as an unconfigured secret");

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert!(!format!("{error:?}").contains("must-not-leak-secret"));
}

impl SkillSecretStore for MemorySecretStore {
    fn get(
        &self,
        skill_id: &str,
        key: &str,
    ) -> Result<Option<SecretString>, SkillConfigStoreError> {
        Ok(self
            .values
            .lock()
            .unwrap()
            .get(&format!("{skill_id}/{key}"))
            .cloned())
    }

    fn set(
        &self,
        skill_id: &str,
        key: &str,
        value: SecretString,
    ) -> Result<(), SkillConfigStoreError> {
        self.values
            .lock()
            .unwrap()
            .insert(format!("{skill_id}/{key}"), value);
        Ok(())
    }

    fn delete(&self, skill_id: &str, key: &str) -> Result<(), SkillConfigStoreError> {
        self.values
            .lock()
            .unwrap()
            .remove(&format!("{skill_id}/{key}"));
        Ok(())
    }
}

#[derive(Debug)]
struct UnavailableSecretStore;

impl SkillSecretStore for UnavailableSecretStore {
    fn get(
        &self,
        _skill_id: &str,
        _key: &str,
    ) -> Result<Option<SecretString>, SkillConfigStoreError> {
        Err(SkillConfigStoreError::SecretStoreUnavailable)
    }

    fn set(
        &self,
        _skill_id: &str,
        _key: &str,
        _value: SecretString,
    ) -> Result<(), SkillConfigStoreError> {
        Err(SkillConfigStoreError::SecretStoreUnavailable)
    }

    fn delete(&self, _skill_id: &str, _key: &str) -> Result<(), SkillConfigStoreError> {
        Err(SkillConfigStoreError::SecretStoreUnavailable)
    }
}

#[derive(Debug, Default)]
struct FailCompensationSecretStore {
    inner: MemorySecretStore,
    delete_calls: AtomicUsize,
}

impl SkillSecretStore for FailCompensationSecretStore {
    fn get(
        &self,
        skill_id: &str,
        key: &str,
    ) -> Result<Option<SecretString>, SkillConfigStoreError> {
        self.inner.get(skill_id, key)
    }

    fn set(
        &self,
        skill_id: &str,
        key: &str,
        value: SecretString,
    ) -> Result<(), SkillConfigStoreError> {
        self.inner.set(skill_id, key, value)
    }

    fn delete(&self, skill_id: &str, key: &str) -> Result<(), SkillConfigStoreError> {
        let call = self.delete_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == 2 {
            return Err(SkillConfigStoreError::SecretStoreUnavailable);
        }
        self.inner.delete(skill_id, key)
    }
}

#[test]
fn skill_config_uses_the_global_document_path() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join(".jyowo");
    let layout = StorageLayout::new(JyowoHome::new(home.clone()));

    assert_eq!(
        layout.global_skill_config_file(),
        home.join("config/skill-config.json")
    );
}

#[test]
fn skill_config_persists_public_values_and_only_secret_presence() {
    let root = tempfile::tempdir().unwrap();
    let layout = StorageLayout::new(JyowoHome::new(
        root.path().canonicalize().unwrap().join(".jyowo"),
    ));
    let secrets = Arc::new(MemorySecretStore::default());
    let store = DesktopSkillConfigStore::new(layout.clone(), secrets.clone());
    let plaintext = "test-secret-plaintext";

    store
        .set_public_value("user:example", &public_config("region"), json!("cn-east"))
        .unwrap();
    store
        .set_secret(
            "user:example",
            &secret_config("apiToken"),
            SecretString::from(plaintext.to_owned()),
        )
        .unwrap();

    let document = store.load_document().unwrap();
    let json = std::fs::read_to_string(layout.global_skill_config_file()).unwrap();
    assert_eq!(document.version, SkillConfigDocument::CURRENT_VERSION);
    assert_eq!(
        document.skills["user:example"].values["region"],
        json!("cn-east")
    );
    assert!(document.skills["user:example"].secrets["apiToken"].configured);
    assert!(!json.contains(plaintext));
    assert!(!format!("{document:?}").contains(plaintext));
    assert_eq!(
        secrets
            .get("user:example", "apiToken")
            .unwrap()
            .unwrap()
            .expose_secret(),
        plaintext
    );

    store
        .clear_secret("user:example", &secret_config("apiToken"))
        .unwrap();
    let document = store.load_document().unwrap();
    assert!(!document.skills["user:example"].secrets["apiToken"].configured);
    assert!(secrets.get("user:example", "apiToken").unwrap().is_none());
}

#[test]
fn skill_config_namespaces_two_skills_and_never_echoes_failed_secret_writes() {
    let root = tempfile::tempdir().unwrap();
    let layout = StorageLayout::new(JyowoHome::new(
        root.path().canonicalize().unwrap().join(".jyowo"),
    ));
    let secrets = Arc::new(MemorySecretStore::default());
    let store = DesktopSkillConfigStore::new(layout, secrets);

    store
        .set_public_value("user:one", &public_config("region"), json!("one"))
        .unwrap();
    store
        .set_public_value("user:two", &public_config("region"), json!("two"))
        .unwrap();

    let document = store.load_document().unwrap();
    assert_eq!(document.skills["user:one"].values["region"], json!("one"));
    assert_eq!(document.skills["user:two"].values["region"], json!("two"));
}

#[test]
fn skill_config_rejects_secret_through_public_storage_and_removes_legacy_plaintext() {
    let root = tempfile::tempdir().unwrap();
    let layout = StorageLayout::new(JyowoHome::new(
        root.path().canonicalize().unwrap().join(".jyowo"),
    ));
    let secrets = Arc::new(MemorySecretStore::default());
    let store = DesktopSkillConfigStore::new(layout.clone(), secrets);
    let secret = secret_config("apiToken");

    let error = store
        .set_public_value("user:example", &secret, json!("plaintext"))
        .expect_err("secret declarations must not use public storage");
    assert!(!format!("{error:?}").contains("plaintext"));

    store
        .set_public_value(
            "user:example",
            &public_config("apiToken"),
            json!("legacy-plaintext"),
        )
        .unwrap();
    store
        .set_secret(
            "user:example",
            &secret,
            SecretString::from("replacement-secret".to_owned()),
        )
        .unwrap();

    let json = std::fs::read_to_string(layout.global_skill_config_file()).unwrap();
    assert!(!json.contains("legacy-plaintext"));
    assert!(!json.contains("replacement-secret"));
}

#[test]
fn independent_skill_config_stores_for_the_same_path_do_not_lose_updates() {
    let root = tempfile::tempdir().unwrap();
    let layout = StorageLayout::new(JyowoHome::new(
        root.path().canonicalize().unwrap().join(".jyowo"),
    ));
    let secrets = Arc::new(MemorySecretStore::default());
    let stores = [
        DesktopSkillConfigStore::new(layout.clone(), secrets.clone()),
        DesktopSkillConfigStore::new(layout, secrets),
    ];
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let threads = (0..8)
        .map(|index| {
            let store = stores[index % stores.len()].clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let key = format!("key{index}");
                store
                    .set_public_value(
                        "user:example",
                        &public_config(&key),
                        json!(index.to_string()),
                    )
                    .unwrap();
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }

    let document = stores[0].load_document().unwrap();
    assert_eq!(document.skills["user:example"].values.len(), 8);
}

#[test]
fn indeterminate_document_commit_is_typed_and_does_not_guess_at_compensation() {
    let root = tempfile::tempdir().unwrap();
    let layout = StorageLayout::new(JyowoHome::new(
        root.path().canonicalize().unwrap().join(".jyowo"),
    ));
    let secrets = Arc::new(MemorySecretStore::default());
    let seed_store = DesktopSkillConfigStore::new(layout.clone(), secrets.clone());
    seed_store
        .set_public_value(
            "user:example",
            &public_config("apiToken"),
            json!("legacy-plaintext"),
        )
        .unwrap();
    let fault_store = DesktopSkillConfigStore::new(layout, secrets.clone())
        .with_fault_for_test(SkillConfigStoreFault::SaveAndReadbackFail);

    let error = fault_store
        .set_secret(
            "user:example",
            &secret_config("apiToken"),
            SecretString::from("replacement-secret".to_owned()),
        )
        .expect_err("commit state must be indeterminate");

    assert_eq!(error.code, "SKILL_CONFIG_COMMIT_INDETERMINATE");
    assert_eq!(
        secrets
            .get("user:example", "apiToken")
            .unwrap()
            .unwrap()
            .expose_secret(),
        "replacement-secret",
        "indeterminate state must not trigger guessed compensation"
    );
    assert_eq!(
        seed_store.load_document().unwrap().skills["user:example"].values["apiToken"],
        json!("legacy-plaintext")
    );
    let rendered = format!("{error:?}");
    assert!(!rendered.contains("legacy-plaintext"));
    assert!(!rendered.contains("replacement-secret"));
}

#[test]
fn failed_document_write_restores_the_previous_secret() {
    let root = tempfile::tempdir().unwrap();
    let layout = StorageLayout::new(JyowoHome::new(
        root.path().canonicalize().unwrap().join(".jyowo"),
    ));
    let secrets = Arc::new(MemorySecretStore::default());
    let seed_store = DesktopSkillConfigStore::new(layout.clone(), secrets.clone());
    seed_store
        .set_public_value(
            "user:example",
            &public_config("apiToken"),
            json!("legacy-plaintext"),
        )
        .unwrap();
    let store = DesktopSkillConfigStore::new(layout, secrets.clone())
        .with_fault_for_test(SkillConfigStoreFault::SaveFail);

    let error = store
        .set_secret(
            "user:example",
            &secret_config("apiToken"),
            SecretString::from("replacement-secret".to_owned()),
        )
        .expect_err("document write must fail at the injected save point");

    assert!(secrets.get("user:example", "apiToken").unwrap().is_none());
    let document = seed_store.load_document().unwrap();
    assert_eq!(
        document.skills["user:example"].values["apiToken"],
        json!("legacy-plaintext")
    );
    let rendered = format!("{error:?}");
    assert!(!rendered.contains("legacy-plaintext"));
    assert!(!rendered.contains("replacement-secret"));
}

#[test]
fn rotating_an_existing_secret_does_not_rewrite_unchanged_metadata() {
    let root = tempfile::tempdir().unwrap();
    let layout = StorageLayout::new(JyowoHome::new(
        root.path().canonicalize().unwrap().join(".jyowo"),
    ));
    let secrets = Arc::new(MemorySecretStore::default());
    let seed_store = DesktopSkillConfigStore::new(layout.clone(), secrets.clone());
    seed_store
        .set_secret(
            "user:example",
            &secret_config("apiToken"),
            SecretString::from("previous-secret".to_owned()),
        )
        .unwrap();
    let store = DesktopSkillConfigStore::new(layout, secrets.clone())
        .with_fault_for_test(SkillConfigStoreFault::SaveFail);

    let result = store.set_secret(
        "user:example",
        &secret_config("apiToken"),
        SecretString::from("replacement-secret".to_owned()),
    );

    result.expect("unchanged presence metadata must not require a document write");
    assert_eq!(
        secrets
            .get("user:example", "apiToken")
            .unwrap()
            .unwrap()
            .expose_secret(),
        "replacement-secret"
    );
}

#[test]
fn compensation_failure_is_reported_without_exposing_secrets() {
    let root = tempfile::tempdir().unwrap();
    let layout = StorageLayout::new(JyowoHome::new(
        root.path().canonicalize().unwrap().join(".jyowo"),
    ));
    let secrets = Arc::new(FailCompensationSecretStore::default());
    let seed_store = DesktopSkillConfigStore::new(layout.clone(), secrets.clone());
    seed_store
        .set_public_value(
            "user:example",
            &public_config("apiToken"),
            json!("legacy-plaintext"),
        )
        .unwrap();
    let store = DesktopSkillConfigStore::new(layout, secrets)
        .with_fault_for_test(SkillConfigStoreFault::SaveFail);

    let error = store
        .set_secret(
            "user:example",
            &secret_config("apiToken"),
            SecretString::from("replacement-secret".to_owned()),
        )
        .expect_err("compensation failure must be returned");

    assert_eq!(error.code, "SKILL_CONFIG_COMPENSATION_FAILED");
    let rendered = format!("{error:?}");
    assert!(!rendered.contains("legacy-plaintext"));
    assert!(!rendered.contains("replacement-secret"));
}

fn public_config(key: &str) -> SkillConfigDecl {
    SkillConfigDecl {
        key: key.to_owned(),
        value_type: SkillParamType::String,
        secret: false,
        required: false,
        default: None,
        description: None,
    }
}

fn required_public_config(key: &str) -> SkillConfigDecl {
    SkillConfigDecl {
        required: true,
        ..public_config(key)
    }
}

fn persisted_config_skill() -> harness_skill::Skill {
    parse_skill_markdown(
        "---\nname: persisted-config-bootstrap-test\ndescription: Persisted config bootstrap test\nconfig:\n  - key: region\n    type: string\n    required: true\n---\nUse ${config.region}.\n",
        SkillSource::User("test/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .unwrap()
}

fn secret_config(key: &str) -> SkillConfigDecl {
    SkillConfigDecl {
        secret: true,
        ..public_config(key)
    }
}

fn test_layout(workspace: &std::path::Path) -> StorageLayout {
    StorageLayout::new(JyowoHome::new(
        workspace.join(".jyowo-test-home").join(".jyowo"),
    ))
}

async fn test_settings_runtime(workspace: &std::path::Path) -> Arc<DesktopSettingsRuntime> {
    let mut options = HarnessOptions::default();
    options.workspace_root = workspace.to_path_buf();
    options.model_id = "test-model".to_owned();
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(std::time::Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    Arc::new(
        DesktopSettingsRuntime::try_from(
            DesktopSettingsRuntime::builder()
                .with_options(options)
                .with_model(TestModelProvider::default())
                .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
                .with_sandbox(NoopSandbox::new())
                .with_stream_permission_broker_arc(
                    stream_permission_runtime.broker(),
                    stream_permission_runtime.resolver_handle(),
                )
                .build()
                .await
                .unwrap(),
        )
        .unwrap(),
    )
}

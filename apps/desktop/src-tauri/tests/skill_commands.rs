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
    delete_skill_with_runtime_state, get_or_create_skill_catalog_install_task,
    get_skill_config_with_runtime_state, get_skill_detail_with_runtime_state,
    import_skill_with_runtime_state, list_skill_catalog_install_tasks_with_runtime_state,
    list_skills_with_runtime_state, record_skill_catalog_install_task_progress,
    reload_desktop_settings_runtime_after_plugin_change_for_test,
    runtime_state_with_skill_config_store_for_test, set_skill_config_value_with_runtime_state,
    set_skill_enabled_with_runtime_state, start_skill_catalog_install_task_with_runtime_state,
    DeleteSkillRequest, DesktopRuntimeState, GetSkillConfigRequest, GetSkillDetailRequest,
    ImportSkillRequest, SetSkillConfigValueRequest, SetSkillEnabledRequest, SkillStore,
    SkillStoreRecord,
};
use jyowo_desktop_shell::skill_catalog::{
    fetch_catalog_http_for_test, CatalogHttpTimeouts, InstallSkillFromCatalogRequest,
    SkillInstallOriginRecord,
};
use jyowo_desktop_shell::storage_layout::{JyowoHome, StorageLayout};
use jyowo_harness_sdk::ext::StreamBrokerConfig;
use jyowo_harness_sdk::skill_config::{SecretString, SkillConfigStoreError, SkillSecretStore};
use jyowo_harness_sdk::testing::{InMemoryEventStore, NoopSandbox, TestModelProvider};
use jyowo_harness_sdk::{DesktopSettingsRuntime, HarnessOptions, StreamPermissionRuntime};
use secrecy::ExposeSecret;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Debug, Default)]
struct MemorySecretStore {
    values: Mutex<BTreeMap<String, SecretString>>,
}

#[derive(Debug, Default)]
struct BlockingLoadGate {
    gate: Mutex<Option<(SyncSender<()>, Receiver<()>)>>,
}

#[tokio::test]
async fn catalog_connect_timeout_is_typed() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut client_hello = [0_u8; 1024];
        stream.read(&mut client_hello).await.unwrap();
        std::future::pending::<()>().await;
    });

    let error = fetch_catalog_http_for_test(
        &format!("https://{address}/stalled-tls-handshake"),
        CatalogHttpTimeouts {
            connect: std::time::Duration::from_millis(40),
            request: std::time::Duration::from_secs(1),
            response_body: std::time::Duration::from_secs(1),
        },
    )
    .await
    .expect_err("stalled TLS handshake must hit the connect timeout");
    server.abort();

    assert_eq!(error.code, "CATALOG_CONNECT_TIMEOUT");
}

#[tokio::test]
async fn catalog_request_header_timeout_is_typed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/slow-headers"))
        .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_millis(200)))
        .mount(&server)
        .await;

    let error = fetch_catalog_http_for_test(
        &format!("{}/slow-headers", server.uri()),
        CatalogHttpTimeouts {
            connect: std::time::Duration::from_secs(1),
            request: std::time::Duration::from_millis(40),
            response_body: std::time::Duration::from_secs(1),
        },
    )
    .await
    .expect_err("stalled response headers must time out");

    assert_eq!(error.code, "CATALOG_REQUEST_TIMEOUT");
}

#[tokio::test]
async fn catalog_response_body_timeout_is_typed() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 1024];
        stream.read(&mut request).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nx")
            .await
            .unwrap();
        stream.flush().await.unwrap();
        std::future::pending::<()>().await;
    });

    let error = fetch_catalog_http_for_test(
        &format!("http://{address}/slow-body"),
        CatalogHttpTimeouts {
            connect: std::time::Duration::from_secs(1),
            request: std::time::Duration::from_secs(1),
            response_body: std::time::Duration::from_millis(40),
        },
    )
    .await
    .expect_err("stalled response body must time out");
    server.abort();

    assert_eq!(error.code, "CATALOG_RESPONSE_BODY_TIMEOUT", "{error:?}");
}

#[tokio::test]
async fn catalog_install_operations_persist_recover_and_keep_terminal_history() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().canonicalize().unwrap();
    let state = DesktopRuntimeState::with_workspace_for_test(workspace.clone()).unwrap();
    let request = InstallSkillFromCatalogRequest {
        source_id: "anthropic".to_owned(),
        entry_id: "anthropic:frontend-design".to_owned(),
        version: Some("main".to_owned()),
        operation_id: Some("catalog-operation-1".to_owned()),
    };

    let first = get_or_create_skill_catalog_install_task(&state, &request).unwrap();
    let duplicate = get_or_create_skill_catalog_install_task(&state, &request).unwrap();
    assert_eq!(duplicate.operation_id, first.operation_id);

    let reconstructed = DesktopRuntimeState::with_workspace_for_test(workspace.clone()).unwrap();
    assert_eq!(
        list_skill_catalog_install_tasks_with_runtime_state(&reconstructed)
            .await
            .unwrap()
            .tasks[0]
            .status,
        "running"
    );
    reconstructed
        .recover_skill_catalog_install_tasks_for_test()
        .unwrap();
    assert_eq!(
        list_skill_catalog_install_tasks_with_runtime_state(&reconstructed)
            .await
            .unwrap()
            .tasks[0]
            .status,
        "interrupted"
    );

    let retry = InstallSkillFromCatalogRequest {
        operation_id: Some("catalog-operation-2".to_owned()),
        ..request
    };
    get_or_create_skill_catalog_install_task(&reconstructed, &retry).unwrap();
    record_skill_catalog_install_task_progress(&reconstructed, &retry, "completed", 100, None)
        .await
        .unwrap();
    let reinstall = InstallSkillFromCatalogRequest {
        operation_id: Some("catalog-operation-3".to_owned()),
        ..retry
    };
    get_or_create_skill_catalog_install_task(&reconstructed, &reinstall).unwrap();

    let tasks = list_skill_catalog_install_tasks_with_runtime_state(&reconstructed)
        .await
        .unwrap()
        .tasks;
    assert_eq!(tasks.len(), 3);
    assert!(tasks.iter().any(|task| task.status == "completed"));
    assert!(tasks
        .iter()
        .any(|task| { task.operation_id == "catalog-operation-3" && task.status == "running" }));
}

#[tokio::test]
async fn catalog_install_delete_and_reinstall_runs_the_full_local_package_flow() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().canonicalize().unwrap();
    let layout = test_layout(&workspace);
    let settings_runtime = test_settings_runtime(&workspace).await;
    let mut state = DesktopRuntimeState::with_settings_runtime_for_workspace(
        workspace.clone(),
        settings_runtime.clone(),
    )
    .unwrap();
    let skill_store = Arc::new(jyowo_desktop_shell::commands::DesktopSkillStore::global(
        layout.clone(),
    ));
    state.set_skill_store_for_test(skill_store);
    state.set_catalog_task_runtime_root_for_test(layout.global_runtime_root());
    state.set_config_stores_for_test(GlobalConfigStore::new(layout), None);

    let source = workspace.join("catalog-reinstall-source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: catalog-reinstall-test\ndescription: Catalog reinstall test\n---\nBody.\n",
    )
    .unwrap();
    state.set_catalog_materialize_hook_for_test(Arc::new({
        let source = source.clone();
        move |request| {
            Ok((
                source.clone(),
                SkillInstallOriginRecord {
                    source_id: request.source_id.clone(),
                    source_label: "Local test catalog".to_owned(),
                    entry_id: request.entry_id.clone(),
                    version: request.version.clone(),
                    commit_sha: None,
                    homepage_url: None,
                    installed_from_catalog: true,
                },
            ))
        }
    }));

    let operation_suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let first_operation_id = format!("catalog-reinstall-operation-1-{operation_suffix}");
    let second_operation_id = format!("catalog-reinstall-operation-2-{operation_suffix}");
    let request = InstallSkillFromCatalogRequest {
        source_id: "local-test".to_owned(),
        entry_id: "local-test:catalog-reinstall-test".to_owned(),
        version: Some("v1".to_owned()),
        operation_id: Some(first_operation_id.clone()),
    };
    start_skill_catalog_install_task_with_runtime_state(request.clone(), state.clone(), None)
        .await
        .unwrap();
    wait_for_catalog_task_status(&state, &first_operation_id, "completed").await;

    let first = list_skills_with_runtime_state(&state)
        .await
        .unwrap()
        .skills
        .into_iter()
        .find(|skill| skill.name == "catalog-reinstall-test")
        .unwrap();
    delete_skill_with_runtime_state(DeleteSkillRequest { id: first.id }, &state)
        .await
        .unwrap();
    assert!(settings_runtime
        .view_runtime_skill("catalog-reinstall-test", false)
        .unwrap()
        .is_none());

    let retry = InstallSkillFromCatalogRequest {
        operation_id: Some(second_operation_id.clone()),
        ..request
    };
    start_skill_catalog_install_task_with_runtime_state(retry, state.clone(), None)
        .await
        .unwrap();
    wait_for_catalog_task_status(&state, &second_operation_id, "completed").await;

    let installed = list_skills_with_runtime_state(&state).await.unwrap().skills;
    assert_eq!(
        installed
            .iter()
            .filter(|skill| skill.name == "catalog-reinstall-test")
            .count(),
        1
    );
    let tasks = list_skill_catalog_install_tasks_with_runtime_state(&state)
        .await
        .unwrap()
        .tasks;
    for operation_id in [&first_operation_id, &second_operation_id] {
        assert!(tasks.iter().any(|task| {
            &task.operation_id == operation_id
                && task.entry_id == "local-test:catalog-reinstall-test"
                && task.status == "completed"
        }));
    }
}

#[tokio::test]
async fn global_skill_commit_reloads_other_live_project_runtimes() {
    let root = tempfile::tempdir().unwrap();
    let workspace_one = root.path().join("one");
    let workspace_two = root.path().join("two");
    std::fs::create_dir_all(&workspace_one).unwrap();
    std::fs::create_dir_all(&workspace_two).unwrap();
    let workspace_one = workspace_one.canonicalize().unwrap();
    let workspace_two = workspace_two.canonicalize().unwrap();
    let canonical_root = root.path().canonicalize().unwrap();
    let layout = StorageLayout::new(JyowoHome::new(canonical_root.join("home").join(".jyowo")));

    let runtime_one = test_settings_runtime(&workspace_one).await;
    let runtime_two = test_settings_runtime(&workspace_two).await;
    let mut state_one = DesktopRuntimeState::with_settings_runtime_for_workspace(
        workspace_one.clone(),
        runtime_one,
    )
    .unwrap();
    let mut state_two = DesktopRuntimeState::with_settings_runtime_for_workspace(
        workspace_two,
        runtime_two.clone(),
    )
    .unwrap();
    let skill_store = Arc::new(jyowo_desktop_shell::commands::DesktopSkillStore::global(
        layout.clone(),
    ));
    state_one.set_skill_store_for_test(skill_store.clone());
    state_two.set_skill_store_for_test(skill_store);
    state_one.set_config_stores_for_test(GlobalConfigStore::new(layout.clone()), None);
    state_two.set_config_stores_for_test(GlobalConfigStore::new(layout), None);

    let source = workspace_one.join("shared-runtime-source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("SKILL.md"),
        "---\nname: shared-runtime-test\ndescription: Shared runtime test\n---\nBody.\n",
    )
    .unwrap();
    assert!(runtime_two
        .view_runtime_skill("shared-runtime-test", false)
        .unwrap()
        .is_none());

    import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: source.to_string_lossy().into_owned(),
        },
        &state_one,
    )
    .await
    .unwrap();

    assert!(runtime_two
        .view_runtime_skill("shared-runtime-test", false)
        .unwrap()
        .is_some());
}

async fn wait_for_catalog_task_status(
    state: &DesktopRuntimeState,
    operation_id: &str,
    expected_status: &str,
) {
    let mut last_task = None;
    for _ in 0..1_000 {
        let tasks = list_skill_catalog_install_tasks_with_runtime_state(state)
            .await
            .unwrap()
            .tasks;
        if let Some(task) = tasks.iter().find(|task| task.operation_id == operation_id) {
            if task.status == expected_status {
                return;
            }
            assert_ne!(task.status, "failed", "catalog task failed: {task:?}");
            last_task = Some(task.clone());
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("catalog task {operation_id} did not reach {expected_status}; last task: {last_task:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catalog_download_does_not_hold_the_skill_store_lock() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().canonicalize().unwrap();
    let mut state = DesktopRuntimeState::with_workspace_for_test(workspace).unwrap();
    let (entered_tx, entered_rx) = sync_channel(1);
    let (release_tx, release_rx) = sync_channel(1);
    let release_rx = Mutex::new(release_rx);
    state.set_catalog_download_hook_for_test(Arc::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.lock().unwrap().recv().unwrap();
    }));
    let request = InstallSkillFromCatalogRequest {
        source_id: "anthropic".to_owned(),
        entry_id: "anthropic:frontend-design".to_owned(),
        version: Some("main".to_owned()),
        operation_id: Some("catalog-lock-scope".to_owned()),
    };

    jyowo_desktop_shell::commands::start_skill_catalog_install_task_with_runtime_state(
        request,
        state.clone(),
        None,
    )
    .await
    .unwrap();
    tokio::task::spawn_blocking(move || {
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
    })
    .await
    .unwrap();

    tokio::time::timeout(
        std::time::Duration::from_millis(100),
        list_skills_with_runtime_state(&state),
    )
    .await
    .expect("unrelated skill-store mutation must acquire the lock during download")
    .unwrap();
    release_tx.send(()).unwrap();
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
    assert_eq!(
        skill_store.load_records().unwrap()[0]
            .last_validation_error
            .as_deref(),
        Some("skill package content hash mismatch")
    );
    assert!(state
        .settings_runtime()
        .unwrap()
        .view_runtime_skill("integrity-test", false)
        .unwrap()
        .is_none());

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

#[tokio::test]
async fn package_integrity_detail_persists_rejection_and_evicts_runtime() {
    let fixture = installed_integrity_skill().await;
    fixture.tamper();

    let detail = get_skill_detail_with_runtime_state(
        GetSkillDetailRequest {
            id: fixture.skill_id.clone(),
        },
        &fixture.state,
    )
    .await
    .unwrap();

    assert_eq!(detail.skill.summary.status, "rejected");
    assert_eq!(
        fixture.store.load_records().unwrap()[0]
            .last_validation_error
            .as_deref(),
        Some("skill package content hash mismatch")
    );
    assert!(fixture
        .state
        .settings_runtime()
        .unwrap()
        .view_runtime_skill("integrity-test", false)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn package_integrity_same_state_enable_revalidates_and_evicts_runtime() {
    let fixture = installed_integrity_skill().await;
    fixture.tamper();

    let response = set_skill_enabled_with_runtime_state(
        SetSkillEnabledRequest {
            id: fixture.skill_id.clone(),
            enabled: true,
        },
        &fixture.state,
    )
    .await
    .unwrap();

    assert_eq!(response.skill.status, "rejected");
    assert_eq!(
        fixture.store.load_records().unwrap()[0]
            .last_validation_error
            .as_deref(),
        Some("skill package content hash mismatch")
    );
    assert!(fixture
        .state
        .settings_runtime()
        .unwrap()
        .view_runtime_skill("integrity-test", false)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn package_integrity_config_lookup_rejects_a_tampered_managed_skill() {
    let fixture = installed_integrity_skill().await;
    fixture.tamper();

    for request_id in [
        fixture.skill_id.clone(),
        "integrity-test".to_owned(),
        "user:integrity-test".to_owned(),
    ] {
        let result = get_skill_config_with_runtime_state(
            GetSkillConfigRequest {
                skill_id: request_id.clone(),
            },
            &fixture.state,
        )
        .await;
        let error = result.expect_err(&format!(
            "request {request_id} unexpectedly resolved a stale runtime view"
        ));
        assert_eq!(error.code, "INVALID_PAYLOAD");
    }
    assert_eq!(
        fixture.store.load_records().unwrap()[0]
            .last_validation_error
            .as_deref(),
        Some("skill package content hash mismatch")
    );
}

#[tokio::test]
async fn package_integrity_missing_selection_uses_legacy_enabled_records_and_hashes() {
    let fixture = installed_integrity_skill().await;
    std::fs::remove_file(fixture.layout.global_skills_file()).unwrap();
    fixture.tamper();

    let response = set_skill_enabled_with_runtime_state(
        SetSkillEnabledRequest {
            id: fixture.skill_id.clone(),
            enabled: true,
        },
        &fixture.state,
    )
    .await
    .unwrap();

    assert_eq!(response.skill.status, "rejected");
    assert!(fixture
        .state
        .settings_runtime()
        .unwrap()
        .view_runtime_skill("integrity-test", false)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn package_integrity_list_evicts_after_an_earlier_check_persisted_the_rejection() {
    let fixture = installed_integrity_skill().await;
    fixture.tamper();
    get_skill_config_with_runtime_state(
        GetSkillConfigRequest {
            skill_id: fixture.skill_id.clone(),
        },
        &fixture.state,
    )
    .await
    .expect_err("config lookup must persist the integrity rejection");
    assert!(fixture
        .state
        .settings_runtime()
        .unwrap()
        .view_runtime_skill("integrity-test", false)
        .unwrap()
        .is_some());

    list_skills_with_runtime_state(&fixture.state)
        .await
        .unwrap();

    assert!(fixture
        .state
        .settings_runtime()
        .unwrap()
        .view_runtime_skill("integrity-test", false)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn package_integrity_missing_recorded_hash_fails_closed() {
    let fixture = installed_integrity_skill().await;
    let mut records = fixture.store.load_records().unwrap();
    records[0].content_hash.clear();
    fixture.store.save_records(&records).unwrap();

    let response = set_skill_enabled_with_runtime_state(
        SetSkillEnabledRequest {
            id: fixture.skill_id.clone(),
            enabled: true,
        },
        &fixture.state,
    )
    .await
    .unwrap();

    assert_eq!(response.skill.status, "rejected");
    assert!(fixture
        .state
        .settings_runtime()
        .unwrap()
        .view_runtime_skill("integrity-test", false)
        .unwrap()
        .is_none());
}

struct InstalledIntegritySkill {
    _root: tempfile::TempDir,
    layout: StorageLayout,
    state: DesktopRuntimeState,
    store: Arc<jyowo_desktop_shell::commands::DesktopSkillStore>,
    skill_id: String,
}

impl InstalledIntegritySkill {
    fn tamper(&self) {
        std::fs::write(
            self.layout
                .global_skills_root()
                .join("packages")
                .join(&self.skill_id)
                .join("SKILL.md"),
            "---\nname: integrity-test\ndescription: Integrity test\n---\nTampered body.\n",
        )
        .unwrap();
    }
}

async fn installed_integrity_skill() -> InstalledIntegritySkill {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().canonicalize().unwrap();
    let layout = test_layout(&workspace);
    let settings_runtime = test_settings_runtime(&workspace).await;
    let mut state = DesktopRuntimeState::with_settings_runtime_for_workspace(
        workspace.clone(),
        settings_runtime,
    )
    .unwrap();
    let store = Arc::new(jyowo_desktop_shell::commands::DesktopSkillStore::global(
        layout.clone(),
    ));
    state.set_skill_store_for_test(store.clone());
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

    InstalledIntegritySkill {
        _root: root,
        layout,
        state,
        store,
        skill_id: imported.skill.id,
    }
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
    let package_source = workspace.join("managed-configured-package");
    std::fs::create_dir_all(&package_source).unwrap();
    std::fs::write(
        package_source.join("SKILL.md"),
        "---\nname: configured\ndescription: Configured skill\nconfig:\n  - key: region\n    type: string\n---\nUse ${config.region}.\n",
    )
    .unwrap();
    let content_hash = skill_store
        .write_skill_package("managed-record-id", true, &package_source)
        .unwrap();
    let skill_index = layout.global_skills_root().join("index.json");
    std::fs::create_dir_all(skill_index.parent().unwrap()).unwrap();
    std::fs::write(
        skill_index,
        serde_json::to_vec_pretty(&vec![SkillStoreRecord {
            id: "managed-record-id".to_owned(),
            name: "configured".to_owned(),
            description: "Configured skill".to_owned(),
            enabled: true,
            content_hash,
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
        .await
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
    let skill_store = Arc::new(jyowo_desktop_shell::commands::DesktopSkillStore::global(
        layout.clone(),
    ));
    state.set_skill_store_for_test(skill_store.clone());
    let package_source = layout.global_skills_root().join("shadow-package-source");
    std::fs::create_dir_all(&package_source).unwrap();
    std::fs::write(
        package_source.join("SKILL.md"),
        "---\nname: configured\ndescription: User skill\n---\nUser.\n",
    )
    .unwrap();
    let content_hash = skill_store
        .write_skill_package("managed-record-id", true, &package_source)
        .unwrap();
    let skill_index = layout.global_skills_root().join("index.json");
    std::fs::create_dir_all(skill_index.parent().unwrap()).unwrap();
    std::fs::write(
        skill_index,
        serde_json::to_vec_pretty(&vec![SkillStoreRecord {
            id: "managed-record-id".to_owned(),
            name: "configured".to_owned(),
            description: "User skill".to_owned(),
            enabled: true,
            content_hash,
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
    .await
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
    .await
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
    .await
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

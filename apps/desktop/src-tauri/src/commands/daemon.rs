use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::{engine::general_purpose, Engine as _};
use harness_contracts::{
    BlobId, ClientFrame, ClientRequest, ServerFrame, ServerMessage, StageBlobCommand, TaskId,
    MAX_DAEMON_BLOB_BYTES,
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

use crate::daemon_client::{DaemonClient, DaemonClientConfig};

use super::contracts::{ListReferenceCandidatesResponse, ReferenceCandidatePayload};

const DAEMON_SIDECAR_NAME: &str = "jyowo-harness-daemon";
const DAEMON_EVENT_NAME: &str = "jyowo://daemon-events";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct DaemonBridgeState {
    client: RwLock<Option<DaemonClient>>,
    subscriptions: Arc<Mutex<DaemonSubscriptionRegistry>>,
}

#[derive(Default)]
struct DaemonSubscriptionRegistry {
    subscriptions: HashMap<String, DaemonSubscription>,
    lifecycle_windows: HashMap<WindowInstanceId, WindowLifecycle>,
    next_window_generation: u64,
    next_subscription_token: u64,
}

struct DaemonSubscription {
    owner_window_generation: WindowGeneration,
    token: SubscriptionToken,
    task: JoinHandle<()>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct WindowInstanceId(usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowGeneration(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SubscriptionToken(u64);

struct WindowLifecycle {
    generation: WindowGeneration,
    _instance: WindowInstance,
}

struct WindowRegistration {
    generation: WindowGeneration,
    install_handler: bool,
}

struct WindowInstance {
    id: WindowInstanceId,
    // Retaining the handle keeps the instance's resource table allocation alive,
    // so its address cannot be reused before generation-specific cleanup runs.
    _window: Option<WebviewWindow>,
}

impl WindowInstance {
    fn from_window(window: &WebviewWindow) -> Self {
        let resources = window.resources_table();
        let id = WindowInstanceId((&*resources as *const tauri::ResourceTable) as usize);
        drop(resources);
        Self {
            id,
            _window: Some(window.clone()),
        }
    }

    #[cfg(test)]
    fn test(id: usize) -> Self {
        Self {
            id: WindowInstanceId(id),
            _window: None,
        }
    }
}

impl DaemonSubscriptionRegistry {
    fn register_window(&mut self, instance: WindowInstance) -> WindowRegistration {
        let instance_id = instance.id;
        if let Some(window) = self.lifecycle_windows.get(&instance_id) {
            return WindowRegistration {
                generation: window.generation,
                install_handler: false,
            };
        }
        self.next_window_generation = self
            .next_window_generation
            .checked_add(1)
            .expect("daemon window generation exhausted");
        let generation = WindowGeneration(self.next_window_generation);
        self.lifecycle_windows.insert(
            instance_id,
            WindowLifecycle {
                generation,
                _instance: instance,
            },
        );
        WindowRegistration {
            generation,
            install_handler: true,
        }
    }

    fn next_subscription_token(&mut self) -> SubscriptionToken {
        self.next_subscription_token = self
            .next_subscription_token
            .checked_add(1)
            .expect("daemon subscription token exhausted");
        SubscriptionToken(self.next_subscription_token)
    }

    fn remove_finished_subscription(&mut self, subscription_id: &str, token: SubscriptionToken) {
        if self
            .subscriptions
            .get(subscription_id)
            .is_some_and(|subscription| subscription.token == token)
        {
            self.subscriptions.remove(subscription_id);
        }
    }

    fn remove_window_subscriptions(&mut self, generation: WindowGeneration) -> Vec<JoinHandle<()>> {
        self.lifecycle_windows
            .retain(|_, window| window.generation != generation);
        let subscription_ids = self
            .subscriptions
            .iter()
            .filter_map(|(subscription_id, subscription)| {
                (subscription.owner_window_generation == generation)
                    .then(|| subscription_id.clone())
            })
            .collect::<Vec<_>>();
        subscription_ids
            .into_iter()
            .filter_map(|subscription_id| {
                self.subscriptions
                    .remove(&subscription_id)
                    .map(|subscription| subscription.task)
            })
            .collect()
    }
}

impl DaemonBridgeState {
    async fn client(&self) -> Result<DaemonClient, String> {
        self.client
            .read()
            .await
            .clone()
            .ok_or_else(|| "task daemon is not connected".into())
    }

    pub async fn agent_capabilities(&self) -> Option<harness_contracts::AgentCapabilities> {
        self.client.read().await.as_ref()?.agent_capabilities()
    }
}

#[tauri::command]
pub async fn daemon_connect(
    app: AppHandle,
    state: State<'_, DaemonBridgeState>,
) -> Result<ServerFrame, String> {
    let paths = daemon_paths(&app)?;
    let client = DaemonClient::new(paths.config());
    let response = match client.request(ClientRequest::ListTasks).await {
        Ok(response) => response,
        Err(_) => {
            launch_sidecar(&app, &paths)?;
            wait_until_ready(&client).await?
        }
    };
    *state.client.write().await = Some(client);
    Ok(response)
}

#[tauri::command]
pub async fn daemon_request(
    frame: ClientFrame,
    state: State<'_, DaemonBridgeState>,
) -> Result<ServerFrame, String> {
    state
        .client()
        .await?
        .send_frame(frame)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn daemon_subscribe(
    after_offset: u64,
    subscription_id: String,
    window: WebviewWindow,
    state: State<'_, DaemonBridgeState>,
) -> Result<String, String> {
    validate_subscription_id(&subscription_id)?;
    let client = state.client().await?;
    let mut subscription = client.subscribe(after_offset);
    let event_name = format!("{DAEMON_EVENT_NAME}/{subscription_id}");
    let subscription_registry = Arc::clone(&state.subscriptions);
    let cleanup_id = subscription_id.clone();
    let mut subscriptions = state.subscriptions.lock().await;
    if subscriptions.subscriptions.contains_key(&subscription_id) {
        return Err("daemon subscription already exists".into());
    }
    let owner_window_instance = WindowInstance::from_window(&window);
    let window_registration = subscriptions.register_window(owner_window_instance);
    if window_registration.install_handler {
        let window_subscriptions = Arc::clone(&state.subscriptions);
        let owner_window_generation = window_registration.generation;
        window.on_window_event(move |event| {
            cleanup_subscriptions_on_window_event(
                event,
                owner_window_generation,
                Arc::clone(&window_subscriptions),
            );
        });
    }
    let subscription_token = subscriptions.next_subscription_token();
    let emitter_window = window.clone();
    let (start_tx, start_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        if start_rx.await.is_err() {
            return;
        }
        while let Some(frame) = subscription.recv().await {
            if emitter_window.emit(&event_name, frame).is_err() {
                break;
            }
        }
        subscription_registry
            .lock()
            .await
            .remove_finished_subscription(&cleanup_id, subscription_token);
    });
    subscriptions.subscriptions.insert(
        subscription_id.clone(),
        DaemonSubscription {
            owner_window_generation: window_registration.generation,
            token: subscription_token,
            task,
        },
    );
    drop(subscriptions);
    let _ = start_tx.send(());
    Ok(subscription_id)
}

fn cleanup_subscriptions_on_window_event(
    event: &tauri::WindowEvent,
    window_generation: WindowGeneration,
    subscriptions: Arc<Mutex<DaemonSubscriptionRegistry>>,
) {
    if !matches!(event, tauri::WindowEvent::Destroyed) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let tasks = {
            let mut registry = subscriptions.lock().await;
            registry.remove_window_subscriptions(window_generation)
        };
        for task in tasks {
            task.abort();
        }
    });
}

#[tauri::command]
pub async fn daemon_unsubscribe(
    subscription_id: String,
    state: State<'_, DaemonBridgeState>,
) -> Result<(), String> {
    if let Some(subscription) = state
        .subscriptions
        .lock()
        .await
        .subscriptions
        .remove(&subscription_id)
    {
        subscription.task.abort();
    }
    Ok(())
}

#[tauri::command]
pub async fn daemon_read_blob(
    blob_id: BlobId,
    state: State<'_, DaemonBridgeState>,
) -> Result<ServerFrame, String> {
    state
        .client()
        .await?
        .read_blob(blob_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn daemon_stage_blob_from_path(
    task_id: TaskId,
    path: String,
    state: State<'_, DaemonBridgeState>,
) -> Result<ServerFrame, String> {
    let request =
        tokio::task::spawn_blocking(move || stage_blob_request(task_id, PathBuf::from(path)))
            .await
            .map_err(|error| format!("attachment reader failed: {error}"))??;
    state
        .client()
        .await?
        .request(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn daemon_list_reference_candidates(
    task_id: TaskId,
    state: State<'_, DaemonBridgeState>,
) -> Result<ListReferenceCandidatesResponse, String> {
    let frame = state
        .client()
        .await?
        .request(ClientRequest::LoadTask { task_id })
        .await
        .map_err(|error| error.to_string())?;
    let projection = match frame.message {
        ServerMessage::TaskSnapshot(snapshot) if snapshot.projection.task_id == task_id => {
            snapshot.projection
        }
        ServerMessage::Error(error) => return Err(error.message),
        _ => return Err("daemon returned an unexpected task reference response".into()),
    };
    let workspace = projection
        .workspace
        .ok_or_else(|| "task workspace is unavailable".to_owned())?;
    tokio::task::spawn_blocking(move || reference_candidates_from_workspace(&workspace.root))
        .await
        .map_err(|error| format!("reference candidate scan failed: {error}"))?
}

fn reference_candidates_from_workspace(
    workspace_root: &str,
) -> Result<ListReferenceCandidatesResponse, String> {
    let root = std::fs::canonicalize(workspace_root)
        .map_err(|error| format!("task workspace is invalid: {error}"))?;
    if !root.is_dir() {
        return Err("task workspace is not a directory".into());
    }
    let mut paths = Vec::new();
    let mut scanned = 0_usize;
    collect_reference_paths(&root, &root, 0, &mut scanned, &mut paths)?;
    paths.sort();
    paths.truncate(200);
    let files = paths
        .into_iter()
        .map(|path| ReferenceCandidatePayload {
            id: None,
            label: path.clone(),
            path: Some(path),
        })
        .collect();
    Ok(ListReferenceCandidatesResponse {
        artifacts: Vec::new(),
        conversations: Vec::new(),
        files,
        memories: Vec::new(),
        mcp_servers: Vec::new(),
        skills: Vec::new(),
        tools: Vec::new(),
    })
}

fn collect_reference_paths(
    root: &Path,
    directory: &Path,
    depth: usize,
    scanned: &mut usize,
    paths: &mut Vec<String>,
) -> Result<(), String> {
    if depth > 12 || *scanned >= 20_000 || paths.len() >= 200 {
        return Ok(());
    }
    let mut entries = std::fs::read_dir(directory)
        .map_err(|error| format!("cannot read task workspace: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot read task workspace entry: {error}"))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        if *scanned >= 20_000 || paths.len() >= 200 {
            break;
        }
        *scanned += 1;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let metadata = std::fs::symlink_metadata(entry.path())
            .map_err(|error| format!("cannot inspect task workspace entry: {error}"))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            if is_ignored_reference_directory(&name) {
                continue;
            }
            collect_reference_paths(root, &entry.path(), depth + 1, scanned, paths)?;
        } else if metadata.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|_| "task workspace entry escaped its root".to_owned())?
                .to_string_lossy()
                .replace('\\', "/");
            if !relative.is_empty() {
                paths.push(relative);
            }
        }
    }
    Ok(())
}

fn is_ignored_reference_directory(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".worktrees"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | ".next"
            | ".nuxt"
            | "coverage"
    )
}

fn stage_blob_request(task_id: TaskId, path: PathBuf) -> Result<ClientRequest, String> {
    let source_path = std::fs::canonicalize(&path)
        .map_err(|error| format!("attachment path is invalid: {error}"))?;
    let bytes = harness_fs::read_file_no_follow_bounded(&source_path, MAX_DAEMON_BLOB_BYTES)
        .map_err(|error| format!("attachment read failed: {error}"))?
        .ok_or_else(|| "attachment does not exist".to_owned())?;
    Ok(ClientRequest::StageBlob(StageBlobCommand {
        task_id,
        media_type: infer_mime_type(&source_path),
        base64_data: general_purpose::STANDARD.encode(bytes),
    }))
}

fn infer_mime_type(path: &Path) -> String {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "css" => "text/css",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "md" | "markdown" => "text/markdown",
        "rs" | "tsx" | "ts" | "js" | "jsx" | "txt" | "toml" | "yaml" | "yml" => "text/plain",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        _ => "application/octet-stream",
    }
    .to_owned()
}

fn validate_subscription_id(subscription_id: &str) -> Result<(), String> {
    if subscription_id.is_empty()
        || subscription_id.len() > 128
        || !subscription_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("invalid daemon subscription id".into());
    }
    Ok(())
}

struct DaemonPaths {
    runtime_root: PathBuf,
    user_instance_id: String,
    endpoint: PathBuf,
    token_path: PathBuf,
}

impl DaemonPaths {
    fn config(&self) -> DaemonClientConfig {
        DaemonClientConfig {
            endpoint: self.endpoint.clone(),
            token_path: self.token_path.clone(),
            user_instance_id: self.user_instance_id.clone(),
            client_version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}

fn daemon_paths(app: &AppHandle) -> Result<DaemonPaths, String> {
    let app_data = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?;
    let runtime_root = app_data.join("daemon");
    let digest = blake3::hash(app_data.to_string_lossy().as_bytes()).to_hex();
    let user_instance_id = format!("user-{}", &digest.as_str()[..16]);
    let instance_root = runtime_root.join(&user_instance_id);
    #[cfg(unix)]
    let endpoint = instance_root.join("daemon.sock");
    #[cfg(windows)]
    let endpoint = PathBuf::from(format!(r"\\.\pipe\jyowo-harness-daemon-{user_instance_id}"));
    Ok(DaemonPaths {
        token_path: instance_root.join("connection.token"),
        runtime_root,
        user_instance_id,
        endpoint,
    })
}

fn launch_sidecar(app: &AppHandle, paths: &DaemonPaths) -> Result<(), String> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let global_config_dir = daemon_global_config_dir(&home)?;
    let command = app
        .shell()
        .sidecar(DAEMON_SIDECAR_NAME)
        .map_err(|error| error.to_string())?
        .env("JYOWO_DAEMON_RUNTIME_DIR", &paths.runtime_root)
        .env("JYOWO_USER_INSTANCE_ID", &paths.user_instance_id)
        .env("JYOWO_CONFIG_DIR", global_config_dir);
    let (mut events, child) = command.spawn().map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn(async move {
        let _child_guard = child;
        while let Some(event) = events.recv().await {
            if matches!(event, CommandEvent::Terminated(_)) {
                break;
            }
        }
    });
    Ok(())
}

fn daemon_global_config_dir(home: &Path) -> Result<PathBuf, String> {
    let config_dir = home.join(".jyowo").join("config");
    std::fs::create_dir_all(&config_dir).map_err(|error| {
        format!(
            "failed to create daemon global config directory {}: {error}",
            config_dir.display()
        )
    })?;
    config_dir.canonicalize().map_err(|error| {
        format!(
            "failed to canonicalize daemon global config directory {}: {error}",
            config_dir.display()
        )
    })
}

async fn wait_until_ready(client: &DaemonClient) -> Result<ServerFrame, String> {
    let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
    loop {
        match client.request(ClientRequest::ListTasks).await {
            Ok(response) => return Ok(response),
            Err(error) if tokio::time::Instant::now() < deadline => {
                let _ = error;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;

    use super::*;

    #[test]
    fn daemon_global_config_directory_is_created_and_canonical() {
        let home = tempfile::tempdir().unwrap();

        let config = daemon_global_config_dir(home.path()).unwrap();

        assert_eq!(
            config,
            home.path().join(".jyowo/config").canonicalize().unwrap()
        );
        assert!(config.is_dir());
    }

    #[test]
    fn staged_attachment_request_contains_bytes_and_never_the_source_path() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("notes.txt");
        std::fs::write(&path, b"notes").unwrap();
        let task_id = TaskId::new();

        let request = stage_blob_request(task_id, path.clone()).unwrap();
        let value = serde_json::to_value(&request).unwrap();

        assert_eq!(value["type"], "stage_blob");
        assert_eq!(value["taskId"], task_id.to_string());
        assert_eq!(value["mediaType"], "text/plain");
        assert_eq!(value["base64Data"], "bm90ZXM=");
        assert!(!serde_json::to_string(&value)
            .unwrap()
            .contains(path.to_string_lossy().as_ref()));
    }

    #[test]
    fn task_reference_candidates_are_workspace_relative_and_skip_generated_trees() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src/nested")).unwrap();
        std::fs::create_dir_all(root.path().join("node_modules/pkg")).unwrap();
        std::fs::write(root.path().join("src/lib.rs"), b"").unwrap();
        std::fs::write(root.path().join("src/nested/mod.rs"), b"").unwrap();
        std::fs::write(root.path().join("node_modules/pkg/index.js"), b"").unwrap();

        let candidates =
            reference_candidates_from_workspace(root.path().to_str().unwrap()).unwrap();
        let paths = candidates
            .files
            .into_iter()
            .map(|candidate| candidate.path.unwrap())
            .collect::<Vec<_>>();

        assert_eq!(paths, vec!["src/lib.rs", "src/nested/mod.rs"]);
    }

    fn insert_subscription(
        registry: &mut DaemonSubscriptionRegistry,
        subscription_id: &str,
        owner_window_generation: WindowGeneration,
        task: JoinHandle<()>,
    ) -> SubscriptionToken {
        let token = registry.next_subscription_token();
        registry.subscriptions.insert(
            subscription_id.into(),
            DaemonSubscription {
                owner_window_generation,
                token,
                task,
            },
        );
        token
    }

    #[test]
    fn repeated_registration_reuses_one_handler_for_the_same_window_instance() {
        let mut registry = DaemonSubscriptionRegistry::default();

        let first = registry.register_window(WindowInstance::test(1));
        let second = registry.register_window(WindowInstance::test(1));

        assert!(first.install_handler);
        assert!(!second.install_handler);
        assert_eq!(first.generation, second.generation);
        assert_eq!(registry.lifecycle_windows.len(), 1);
    }

    #[tokio::test]
    async fn destroyed_window_aborts_only_its_daemon_subscriptions() {
        let state = DaemonBridgeState::default();
        let first = tokio::spawn(pending::<()>());
        let second = tokio::spawn(pending::<()>());
        let third = tokio::spawn(pending::<()>());
        let first_status = first.abort_handle();
        let second_status = second.abort_handle();
        let third_status = third.abort_handle();
        let window_a_generation;
        let window_b_generation;
        {
            let mut registry = state.subscriptions.lock().await;
            window_a_generation = registry.register_window(WindowInstance::test(1)).generation;
            window_b_generation = registry.register_window(WindowInstance::test(2)).generation;
            insert_subscription(&mut registry, "subscription-a", window_a_generation, first);
            insert_subscription(&mut registry, "subscription-b", window_b_generation, second);
            insert_subscription(&mut registry, "subscription-c", window_a_generation, third);
        }

        cleanup_subscriptions_on_window_event(
            &tauri::WindowEvent::Destroyed,
            window_a_generation,
            Arc::clone(&state.subscriptions),
        );
        tokio::time::timeout(Duration::from_secs(1), async {
            while !first_status.is_finished() || !third_status.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        let registry = state.subscriptions.lock().await;
        assert!(!registry.subscriptions.contains_key("subscription-a"));
        assert!(registry.subscriptions.contains_key("subscription-b"));
        assert!(!registry.subscriptions.contains_key("subscription-c"));
        assert!(!registry
            .lifecycle_windows
            .values()
            .any(|window| window.generation == window_a_generation));
        assert!(registry
            .lifecycle_windows
            .values()
            .any(|window| window.generation == window_b_generation));
        assert!(first_status.is_finished());
        assert!(!second_status.is_finished());
        assert!(third_status.is_finished());
    }

    #[tokio::test]
    async fn delayed_destroy_for_reused_window_label_does_not_abort_new_window_subscription() {
        let state = DaemonBridgeState::default();
        let old_task = tokio::spawn(pending::<()>());
        let new_task = tokio::spawn(pending::<()>());
        let old_status = old_task.abort_handle();
        let new_status = new_task.abort_handle();

        let mut registry = state.subscriptions.lock().await;
        let old_generation = registry.register_window(WindowInstance::test(1)).generation;
        insert_subscription(&mut registry, "old-subscription", old_generation, old_task);

        cleanup_subscriptions_on_window_event(
            &tauri::WindowEvent::Destroyed,
            old_generation,
            Arc::clone(&state.subscriptions),
        );

        let new_registration = registry.register_window(WindowInstance::test(2));
        assert_ne!(old_generation, new_registration.generation);
        assert!(new_registration.install_handler);
        insert_subscription(
            &mut registry,
            "new-subscription",
            new_registration.generation,
            new_task,
        );
        drop(registry);

        tokio::time::timeout(Duration::from_secs(1), async {
            while !old_status.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        let registry = state.subscriptions.lock().await;
        assert!(!registry.subscriptions.contains_key("old-subscription"));
        assert!(registry.subscriptions.contains_key("new-subscription"));
        assert!(!new_status.is_finished());
    }

    #[tokio::test]
    async fn delayed_completion_does_not_remove_a_reused_subscription_id() {
        let mut registry = DaemonSubscriptionRegistry::default();
        let generation = registry.register_window(WindowInstance::test(1)).generation;
        let old_task = tokio::spawn(pending::<()>());
        let old_token = insert_subscription(&mut registry, "subscription", generation, old_task);

        let old_subscription = registry.subscriptions.remove("subscription").unwrap();
        old_subscription.task.abort();

        let new_task = tokio::spawn(pending::<()>());
        let new_status = new_task.abort_handle();
        let new_token = insert_subscription(&mut registry, "subscription", generation, new_task);

        registry.remove_finished_subscription("subscription", old_token);

        assert_eq!(
            registry
                .subscriptions
                .get("subscription")
                .map(|subscription| subscription.token),
            Some(new_token)
        );
        assert!(!new_status.is_finished());
    }
}

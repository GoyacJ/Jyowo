use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use harness_contracts::{BlobId, ClientFrame, ClientRequest, ServerFrame};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

use crate::daemon_client::{DaemonClient, DaemonClientConfig};

const DAEMON_SIDECAR_NAME: &str = "jyowo-harness-daemon";
const DAEMON_EVENT_NAME: &str = "jyowo://daemon-events";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct DaemonBridgeState {
    client: RwLock<Option<DaemonClient>>,
    subscriptions: Arc<Mutex<HashMap<String, JoinHandle<()>>>>,
}

impl DaemonBridgeState {
    async fn client(&self) -> Result<DaemonClient, String> {
        self.client
            .read()
            .await
            .clone()
            .ok_or_else(|| "task daemon is not connected".into())
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
    let subscriptions = Arc::clone(&state.subscriptions);
    let cleanup_id = subscription_id.clone();
    let (start_tx, start_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        if start_rx.await.is_err() {
            return;
        }
        while let Some(frame) = subscription.recv().await {
            if window.emit(&event_name, frame).is_err() {
                break;
            }
        }
        subscriptions.lock().await.remove(&cleanup_id);
    });
    let mut subscriptions = state.subscriptions.lock().await;
    if subscriptions.contains_key(&subscription_id) {
        task.abort();
        return Err("daemon subscription already exists".into());
    }
    subscriptions.insert(subscription_id.clone(), task);
    drop(subscriptions);
    let _ = start_tx.send(());
    Ok(subscription_id)
}

#[tauri::command]
pub async fn daemon_unsubscribe(
    subscription_id: String,
    state: State<'_, DaemonBridgeState>,
) -> Result<(), String> {
    if let Some(task) = state.subscriptions.lock().await.remove(&subscription_id) {
        task.abort();
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
    let command = app
        .shell()
        .sidecar(DAEMON_SIDECAR_NAME)
        .map_err(|error| error.to_string())?
        .env("JYOWO_DAEMON_RUNTIME_DIR", &paths.runtime_root)
        .env("JYOWO_USER_INSTANCE_ID", &paths.user_instance_id);
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

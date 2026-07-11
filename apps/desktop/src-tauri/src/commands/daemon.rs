use std::collections::{HashMap, HashSet};
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
    subscriptions: Arc<Mutex<DaemonSubscriptionRegistry>>,
}

#[derive(Default)]
struct DaemonSubscriptionRegistry {
    subscriptions: HashMap<String, DaemonSubscription>,
    lifecycle_windows: HashSet<String>,
}

struct DaemonSubscription {
    owner_window_label: String,
    task: JoinHandle<()>,
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
    let subscription_registry = Arc::clone(&state.subscriptions);
    let cleanup_id = subscription_id.clone();
    let mut subscriptions = state.subscriptions.lock().await;
    if subscriptions.subscriptions.contains_key(&subscription_id) {
        return Err("daemon subscription already exists".into());
    }
    let owner_window_label = window.label().to_owned();
    if subscriptions
        .lifecycle_windows
        .insert(owner_window_label.clone())
    {
        let window_subscriptions = Arc::clone(&state.subscriptions);
        window.on_window_event(move |event| {
            cleanup_subscriptions_on_window_event(
                event,
                owner_window_label.clone(),
                Arc::clone(&window_subscriptions),
            );
        });
    }
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
            .subscriptions
            .remove(&cleanup_id);
    });
    subscriptions.subscriptions.insert(
        subscription_id.clone(),
        DaemonSubscription {
            owner_window_label: window.label().to_owned(),
            task,
        },
    );
    drop(subscriptions);
    let _ = start_tx.send(());
    Ok(subscription_id)
}

fn cleanup_subscriptions_on_window_event(
    event: &tauri::WindowEvent,
    window_label: String,
    subscriptions: Arc<Mutex<DaemonSubscriptionRegistry>>,
) {
    if !matches!(event, tauri::WindowEvent::Destroyed) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let tasks = {
            let mut registry = subscriptions.lock().await;
            registry.lifecycle_windows.remove(&window_label);
            let subscription_ids = registry
                .subscriptions
                .iter()
                .filter_map(|(subscription_id, subscription)| {
                    (subscription.owner_window_label == window_label)
                        .then(|| subscription_id.clone())
                })
                .collect::<Vec<_>>();
            subscription_ids
                .into_iter()
                .filter_map(|subscription_id| {
                    registry
                        .subscriptions
                        .remove(&subscription_id)
                        .map(|subscription| subscription.task)
                })
                .collect::<Vec<_>>()
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

#[cfg(test)]
mod tests {
    use std::future::pending;

    use super::*;

    #[tokio::test]
    async fn destroyed_window_aborts_only_its_daemon_subscriptions() {
        let state = DaemonBridgeState::default();
        let first = tokio::spawn(pending::<()>());
        let second = tokio::spawn(pending::<()>());
        let third = tokio::spawn(pending::<()>());
        let first_status = first.abort_handle();
        let second_status = second.abort_handle();
        let third_status = third.abort_handle();
        {
            let mut registry = state.subscriptions.lock().await;
            registry
                .lifecycle_windows
                .extend(["window-a".into(), "window-b".into()]);
            registry.subscriptions.insert(
                "subscription-a".into(),
                DaemonSubscription {
                    owner_window_label: "window-a".into(),
                    task: first,
                },
            );
            registry.subscriptions.insert(
                "subscription-b".into(),
                DaemonSubscription {
                    owner_window_label: "window-b".into(),
                    task: second,
                },
            );
            registry.subscriptions.insert(
                "subscription-c".into(),
                DaemonSubscription {
                    owner_window_label: "window-a".into(),
                    task: third,
                },
            );
        }

        cleanup_subscriptions_on_window_event(
            &tauri::WindowEvent::Destroyed,
            "window-a".into(),
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
        assert!(!registry.lifecycle_windows.contains("window-a"));
        assert!(registry.lifecycle_windows.contains("window-b"));
        assert!(first_status.is_finished());
        assert!(!second_status.is_finished());
        assert!(third_status.is_finished());
    }
}

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_contracts::{RunState, PROTOCOL_VERSION};
use harness_daemon::{IpcServerConfig, LocalIpcServer, RecoveryService, RuntimeGuard};
use harness_journal::TaskStore;

const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime_root = std::env::var_os("JYOWO_DAEMON_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("jyowo-daemon"));
    let user_instance_id =
        std::env::var("JYOWO_USER_INSTANCE_ID").unwrap_or_else(|_| default_user_instance_id());
    let runtime = RuntimeGuard::acquire(runtime_root, &user_instance_id)?;
    runtime.prepare_endpoint()?;

    let store = Arc::new(TaskStore::open(runtime.runtime_dir().join("tasks.sqlite"))?);
    RecoveryService::new(Arc::clone(&store)).recover_startup()?;
    let config = IpcServerConfig {
        daemon_version: env!("CARGO_PKG_VERSION").into(),
        user_instance_id: user_instance_id.clone(),
        connection_token: runtime.connection_token().into(),
        event_batch_capacity: 512,
    };

    #[cfg(unix)]
    let server =
        LocalIpcServer::bind_unix(runtime.endpoint_path(), Arc::clone(&store), config).await?;
    #[cfg(windows)]
    let server = LocalIpcServer::bind_named_pipe(
        format!(r"\\.\pipe\jyowo-harness-daemon-{user_instance_id}"),
        Arc::clone(&store),
        config,
    )
    .await?;

    println!(
        "{{\"status\":\"ready\",\"protocolVersion\":{PROTOCOL_VERSION},\"userInstanceId\":{}}}",
        serde_json::to_string(&user_instance_id)?
    );

    wait_for_shutdown(&server, &store).await?;
    server.shutdown().await?;
    Ok(())
}

async fn wait_for_shutdown(
    server: &LocalIpcServer,
    store: &TaskStore,
) -> Result<(), harness_journal::TaskStoreError> {
    let mut idle_since = None;
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                if signal.is_ok() {
                    return Ok(());
                }
            }
            _ = interval.tick() => {
                let active_tasks = store.task_projections()?.into_iter().any(|task| {
                    task.current_run.is_some_and(|run| matches!(
                        run.state,
                        RunState::Running | RunState::WaitingPermission | RunState::Yielding
                    ))
                });
                if server.connected_clients() == 0 && !active_tasks {
                    let since = idle_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= IDLE_TIMEOUT {
                        return Ok(());
                    }
                } else {
                    idle_since = None;
                }
            }
        }
    }
}

#[cfg(unix)]
fn default_user_instance_id() -> String {
    format!("user-{}", rustix::process::getuid().as_raw())
}

#[cfg(not(unix))]
fn default_user_instance_id() -> String {
    std::env::var("USERNAME")
        .unwrap_or_else(|_| "default".into())
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect()
}

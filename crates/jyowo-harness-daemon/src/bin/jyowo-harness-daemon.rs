use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_contracts::{Redactor, RunState, PROTOCOL_VERSION};
use harness_daemon::{
    AutomationScheduler, IpcServerConfig, LocalIpcServer, MemoryService, PermissionBroker,
    RecoveryService, RuntimeConfigResolver, RuntimeGuard, SdkRunCoordinatorFactory,
    SdkSubagentEngineRegistry, SdkWorkspaceSubagentRunnerFactory, Supervisor,
    SupervisorAutomationTaskSubmitter, SupervisorQuotas, WorkspaceSubagentRunnerFactory,
};
use harness_journal::TaskStore;
use harness_observability::DefaultRedactor;

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
    let blob_root = runtime.runtime_dir().join("blobs");
    let config = IpcServerConfig {
        daemon_version: env!("CARGO_PKG_VERSION").into(),
        user_instance_id: user_instance_id.clone(),
        connection_token: runtime.connection_token().into(),
        event_batch_capacity: 512,
        blob_root: blob_root.clone(),
    };
    let redactor: Arc<dyn Redactor> = Arc::new(DefaultRedactor::default());
    let permissions = Arc::new(PermissionBroker::new(
        Arc::clone(&store),
        Arc::clone(&redactor),
    ));
    let subagent_engines = Arc::new(SdkSubagentEngineRegistry::default());
    let config_root = config_root();
    let runtime_config = RuntimeConfigResolver::new(config_root.clone());
    let memory_service = Arc::new(MemoryService::new(runtime_config.clone()));
    let run_factory = Arc::new(SdkRunCoordinatorFactory::new_with_subagent_engines(
        Arc::clone(&store),
        runtime_config,
        blob_root,
        Arc::clone(&permissions),
        Arc::clone(&redactor),
        Arc::clone(&subagent_engines),
    ));
    let runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory> =
        Arc::new(SdkWorkspaceSubagentRunnerFactory::new(subagent_engines));
    let supervisor = Arc::new(Supervisor::start_with_runtime_components(
        Arc::clone(&store),
        run_factory,
        SupervisorQuotas::new(20, 8),
        runner_factory,
        redactor,
        8,
        permissions,
    )?);
    let automation_scheduler = Arc::new(AutomationScheduler::new(
        Arc::clone(&store),
        config_root,
        Arc::new(SupervisorAutomationTaskSubmitter::new(
            Arc::clone(&store),
            Arc::clone(&supervisor),
        )),
    ));
    let _automation_scheduler_task = automation_scheduler.start();

    #[cfg(unix)]
    let server = LocalIpcServer::bind_unix_with_runtime_services(
        runtime.endpoint_path(),
        Arc::clone(&store),
        config,
        Arc::clone(&supervisor),
        Arc::clone(&memory_service),
        Arc::clone(&automation_scheduler),
    )
    .await?;
    #[cfg(windows)]
    let server = LocalIpcServer::bind_named_pipe_with_runtime_services(
        format!(r"\\.\pipe\jyowo-harness-daemon-{user_instance_id}"),
        Arc::clone(&store),
        config,
        Arc::clone(&supervisor),
        Arc::clone(&memory_service),
        Arc::clone(&automation_scheduler),
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

fn config_root() -> PathBuf {
    std::env::var_os("JYOWO_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .map(|root| {
            if std::env::var_os("JYOWO_CONFIG_DIR").is_some() {
                root
            } else {
                root.join(".jyowo").join("config")
            }
        })
        .unwrap_or_else(|| PathBuf::from(".jyowo/config"))
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

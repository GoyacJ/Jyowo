use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use harness_contracts::{
    AgentRunOptions, BackgroundAgentId, BackgroundAgentState, ConversationTurnInput,
    InteractivityLevel, ModelProtocol, PermissionActorSource, PermissionMode, RedactPatternSet,
    RedactRules, RedactScope, TeamId, ToolProfile, ToolSearchMode,
};
use jyowo_harness_sdk::builtin::{DefaultRedactor, JsonlEventStore};
use jyowo_harness_sdk::ext::{EventStore, Redactor, SessionId, TenantId};
use jyowo_harness_sdk::{
    AgentRuntimeStore, BackgroundAgentManager, BackgroundAgentRecord, ConversationTurnRequest,
    Harness, SessionOptions, StreamPermissionRuntime,
};
use serde::{Deserialize, Serialize};
use tauri::Runtime;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

const SUPERVISOR_LOCK_FILE: &str = "agent-supervisor.lock";
const SUPERVISOR_TOKEN_FILE: &str = "agent-supervisor.token";
const SUPERVISOR_TOKEN_ENV: &str = "JYOWO_AGENT_SUPERVISOR_TOKEN";
const SUPERVISOR_TOKEN_EPOCH_ENV: &str = "JYOWO_AGENT_SUPERVISOR_TOKEN_EPOCH";
const SIDECAR_NAME: &str = "jyowo-agent-supervisor";
const SIDECAR_WORKSPACE_ARG: &str = "--workspace-root";
const CONTROL_READ_LIMIT: usize = 8192;
const SIDECAR_STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
pub const DEFAULT_HEARTBEAT_STALE_AFTER: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSupervisorLockFile {
    pub status: String,
    pub workspace_id: String,
    pub token_hash: String,
    pub token_epoch: u64,
    pub pid: u32,
    pub control_addr: String,
    pub started_at: DateTime<Utc>,
    pub heartbeat_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentSupervisorTokenFile {
    token: String,
    token_hash: String,
    token_epoch: u64,
    workspace_id: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SupervisorControlRequest {
    token: String,
    request: SupervisorControlRequestKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SupervisorControlRequestKind {
    Status,
    Wake,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SupervisorControlResponse {
    ok: bool,
    status: String,
}

#[derive(Debug, Clone)]
struct SupervisorToken {
    token: String,
    token_hash: String,
    token_epoch: u64,
}

#[derive(Clone)]
struct SupervisorBackend {
    store: Arc<AgentRuntimeStore>,
    harness: Arc<Harness>,
    _runtime_state: crate::commands::DesktopRuntimeState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundSupervisorPayload {
    #[serde(default)]
    supervisor_execution: Option<BackgroundSupervisorExecution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundSupervisorExecution {
    status: String,
    #[serde(default)]
    session: Option<BackgroundSupervisorSession>,
    #[serde(default)]
    session_options: Option<SessionOptions>,
    input: ConversationTurnInput,
    permission_mode: PermissionMode,
    agent_run_options: AgentRunOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BackgroundSupervisorSession {
    #[serde(default)]
    tenant_id: TenantId,
    session_id: SessionId,
    #[serde(default)]
    tool_search: ToolSearchMode,
    #[serde(default)]
    tool_profile: ToolProfile,
    #[serde(default)]
    model_id: Option<String>,
    #[serde(default)]
    protocol: Option<ModelProtocol>,
    #[serde(default = "default_background_supervisor_permission_mode")]
    permission_mode: PermissionMode,
    #[serde(default = "default_background_supervisor_interactivity")]
    interactivity: InteractivityLevel,
    #[serde(default)]
    team_id: Option<TeamId>,
    #[serde(default)]
    max_iterations: u32,
    #[serde(default)]
    context_compression_trigger_ratio: f32,
}

impl BackgroundSupervisorSession {
    pub(crate) fn from_session_options(options: &SessionOptions) -> Self {
        Self {
            tenant_id: options.tenant_id,
            session_id: options.session_id,
            tool_search: options.tool_search.clone(),
            tool_profile: options.tool_profile.clone(),
            model_id: options.model_id.clone(),
            protocol: options.protocol,
            permission_mode: options.permission_mode,
            interactivity: options.interactivity,
            team_id: options.team_id,
            max_iterations: options.max_iterations,
            context_compression_trigger_ratio: options.context_compression_trigger_ratio,
        }
    }

    fn into_session_options(self, workspace_root: &Path) -> SessionOptions {
        let mut options = SessionOptions::new(workspace_root);
        options.tenant_id = self.tenant_id;
        options.session_id = self.session_id;
        options.tool_search = self.tool_search;
        options.tool_profile = self.tool_profile;
        options.model_id = self.model_id;
        options.protocol = self.protocol;
        options.permission_mode = self.permission_mode;
        options.interactivity = self.interactivity;
        options.team_id = self.team_id;
        options.max_iterations = self.max_iterations;
        options.context_compression_trigger_ratio = self.context_compression_trigger_ratio;
        options
    }
}

fn default_background_supervisor_permission_mode() -> PermissionMode {
    PermissionMode::Default
}

fn default_background_supervisor_interactivity() -> InteractivityLevel {
    InteractivityLevel::NoInteractive
}

impl BackgroundSupervisorExecution {
    fn session_options_for_workspace(
        &self,
        workspace_root: &Path,
    ) -> Result<SessionOptions, AgentSupervisorError> {
        if let Some(session) = self.session.clone() {
            return Ok(session.into_session_options(workspace_root));
        }
        if let Some(mut options) = self.session_options.clone() {
            options.workspace_root = workspace_root.to_path_buf();
            return Ok(options);
        }
        Err(AgentSupervisorError::Runtime(
            "background supervisor session missing".to_owned(),
        ))
    }
}

#[derive(Clone)]
pub struct AgentSupervisorHandle {
    control_addr: SocketAddr,
    lock_path: PathBuf,
    shutdown: watch::Sender<bool>,
    token: String,
    token_hash: String,
}

impl AgentSupervisorHandle {
    #[must_use]
    pub fn control_addr(&self) -> SocketAddr {
        self.control_addr
    }

    #[must_use]
    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    #[must_use]
    pub fn token_hash(&self) -> &str {
        &self.token_hash
    }

    pub async fn shutdown(&self) {
        let _ = self.shutdown.send(true);
        let _ = send_control_request(
            self.control_addr,
            &self.token,
            SupervisorControlRequestKind::Shutdown,
        )
        .await;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgentSupervisorError {
    #[error("agent supervisor io: {0}")]
    Io(#[from] std::io::Error),
    #[error("agent supervisor json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("agent supervisor runtime: {0}")]
    Runtime(String),
    #[error("agent supervisor sidecar: {0}")]
    Sidecar(String),
}

pub async fn start_agent_supervisor(
    workspace_root: PathBuf,
) -> Result<AgentSupervisorHandle, AgentSupervisorError> {
    start_agent_supervisor_with_timing(
        workspace_root,
        DEFAULT_HEARTBEAT_INTERVAL,
        DEFAULT_HEARTBEAT_STALE_AFTER,
    )
    .await
}

pub async fn start_agent_supervisor_with_timing(
    workspace_root: PathBuf,
    heartbeat_interval: Duration,
    stale_after: Duration,
) -> Result<AgentSupervisorHandle, AgentSupervisorError> {
    let token = create_supervisor_token(&workspace_root)?;
    start_agent_supervisor_with_token(workspace_root, heartbeat_interval, stale_after, token).await
}

async fn start_agent_supervisor_with_token(
    workspace_root: PathBuf,
    heartbeat_interval: Duration,
    stale_after: Duration,
    token: SupervisorToken,
) -> Result<AgentSupervisorHandle, AgentSupervisorError> {
    let backend = open_supervisor_backend(&workspace_root).await?;
    recover_if_stale(&workspace_root, stale_after).await?;

    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let control_addr = listener.local_addr()?;
    let lock_path = supervisor_lock_path(&workspace_root);
    let lock = AgentSupervisorLockFile {
        status: "running".to_owned(),
        workspace_id: workspace_id(&workspace_root),
        token_hash: token.token_hash.clone(),
        token_epoch: token.token_epoch,
        pid: std::process::id(),
        control_addr: control_addr.to_string(),
        started_at: Utc::now(),
        heartbeat_at: Utc::now(),
    };
    write_supervisor_lock(&lock_path, &lock)?;

    let (shutdown, shutdown_rx) = watch::channel(false);
    tokio::spawn(run_supervisor_loop(
        workspace_root,
        lock_path.clone(),
        lock,
        listener,
        token.token_hash.clone(),
        heartbeat_interval,
        backend,
        shutdown_rx,
    ));

    Ok(AgentSupervisorHandle {
        control_addr,
        lock_path,
        shutdown,
        token: token.token,
        token_hash: token.token_hash,
    })
}

pub async fn run_supervisor_process(workspace_root: PathBuf) -> Result<(), AgentSupervisorError> {
    let token = match supervisor_token_from_env(&workspace_root)? {
        Some(token) => token,
        None => create_supervisor_token(&workspace_root)?,
    };
    let handle = start_agent_supervisor_with_token(
        workspace_root,
        DEFAULT_HEARTBEAT_INTERVAL,
        DEFAULT_HEARTBEAT_STALE_AFTER,
        token,
    )
    .await?;
    println!("jyowo-agent-supervisor running");
    tokio::signal::ctrl_c().await?;
    handle.shutdown().await;
    Ok(())
}

pub async fn launch_agent_supervisor_sidecar<R: Runtime>(
    app: &tauri::AppHandle<R>,
    workspace_root: PathBuf,
) -> Result<(), AgentSupervisorError> {
    if reconnect_to_existing_supervisor(&workspace_root, DEFAULT_HEARTBEAT_STALE_AFTER).await? {
        return Ok(());
    }

    let token = create_supervisor_token(&workspace_root)?;
    let command = app
        .shell()
        .sidecar(SIDECAR_NAME)
        .map_err(|error| AgentSupervisorError::Sidecar(error.to_string()))?
        .args(supervisor_sidecar_args(&workspace_root))
        .env(SUPERVISOR_TOKEN_ENV, &token.token)
        .env(SUPERVISOR_TOKEN_EPOCH_ENV, token.token_epoch.to_string())
        .current_dir(&workspace_root);
    let (mut events, child) = command
        .spawn()
        .map_err(|error| AgentSupervisorError::Sidecar(error.to_string()))?;
    tauri::async_runtime::spawn(async move {
        let _child_guard = child;
        let redactor = DefaultRedactor::default();
        while let Some(event) = events.recv().await {
            match event {
                CommandEvent::Stdout(bytes) | CommandEvent::Stderr(bytes) => {
                    let _ = redact_supervisor_output(&redactor, &bytes);
                }
                CommandEvent::Error(error) => {
                    let _ = redact_supervisor_output(&redactor, error.as_bytes());
                }
                CommandEvent::Terminated(_) => {}
                _ => {}
            }
        }
    });

    wait_for_supervisor_lock(&workspace_root, &token.token_hash, SIDECAR_STARTUP_TIMEOUT).await
}

pub fn supervisor_sidecar_args(workspace_root: &Path) -> Vec<String> {
    vec![
        SIDECAR_WORKSPACE_ARG.to_owned(),
        workspace_root.display().to_string(),
    ]
}

pub fn supervisor_lock_path(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(".jyowo")
        .join("runtime")
        .join(SUPERVISOR_LOCK_FILE)
}

fn supervisor_token_path(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(".jyowo")
        .join("runtime")
        .join(SUPERVISOR_TOKEN_FILE)
}

pub fn read_supervisor_lock(
    workspace_root: &Path,
) -> Result<Option<AgentSupervisorLockFile>, AgentSupervisorError> {
    let path = supervisor_lock_path(workspace_root);
    if !path.is_file() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&contents)?))
}

pub fn supervisor_lock_is_fresh(lock: &AgentSupervisorLockFile, stale_after: Duration) -> bool {
    if lock.status != "running" {
        return false;
    }
    let Ok(age) = (Utc::now() - lock.heartbeat_at).to_std() else {
        return false;
    };
    age <= stale_after
}

pub async fn recover_if_stale(
    workspace_root: &Path,
    stale_after: Duration,
) -> Result<(), AgentSupervisorError> {
    let Some(lock) = read_supervisor_lock(workspace_root)? else {
        return Ok(());
    };
    if supervisor_lock_is_fresh(&lock, stale_after) {
        return Ok(());
    }
    mark_running_agents_interrupted(workspace_root, "agent supervisor heartbeat missed").await
}

pub async fn reconnect_to_existing_supervisor(
    workspace_root: &Path,
    stale_after: Duration,
) -> Result<bool, AgentSupervisorError> {
    let Some(lock) = read_supervisor_lock(workspace_root)? else {
        return Ok(false);
    };
    if !supervisor_lock_is_fresh(&lock, stale_after) {
        mark_running_agents_interrupted(workspace_root, "agent supervisor heartbeat missed")
            .await?;
        return Ok(false);
    }
    let Some(token) = read_supervisor_token(workspace_root)? else {
        return Ok(false);
    };
    if token.workspace_id != lock.workspace_id
        || token.token_epoch != lock.token_epoch
        || token.token_hash != lock.token_hash
        || hash_token(&token.token) != lock.token_hash
        || lock.workspace_id != workspace_id(workspace_root)
    {
        return Ok(false);
    }
    let control_addr = parse_loopback_control_addr(&lock.control_addr)?;
    match send_control_request(
        control_addr,
        &token.token,
        SupervisorControlRequestKind::Status,
    )
    .await
    {
        Ok(response) if response.ok && response.status == "running" => Ok(true),
        Ok(_) | Err(_) => {
            mark_running_agents_interrupted(
                workspace_root,
                "agent supervisor control channel unavailable",
            )
            .await?;
            Ok(false)
        }
    }
}

pub async fn mark_running_agents_interrupted(
    workspace_root: &Path,
    reason: &str,
) -> Result<(), AgentSupervisorError> {
    let store = Arc::new(
        AgentRuntimeStore::open(workspace_root)
            .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?,
    );
    let event_store = Arc::new(
        JsonlEventStore::open(
            workspace_root.join(".jyowo").join("runtime").join("events"),
            Arc::new(DefaultRedactor::default()),
        )
        .await
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?,
    ) as Arc<dyn EventStore>;
    let manager = BackgroundAgentManager::new(
        Arc::clone(&store),
        event_store,
        TenantId::SINGLE,
        SessionId::new(),
        Arc::new(DefaultRedactor::default()) as Arc<dyn Redactor>,
    );
    let recovered = manager
        .recover_on_startup(reason)
        .await
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    for record in recovered {
        let normalized_payload = set_background_supervisor_payload_status(
            &record.payload_json,
            "interrupted",
            record.run_id.as_deref(),
        )?;
        if normalized_payload != record.payload_json {
            store
                .update_background_agent_payload_json(
                    &record.background_agent_id,
                    &normalized_payload,
                    &Utc::now().to_rfc3339(),
                )
                .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
        }
    }
    Ok(())
}

async fn open_supervisor_backend(
    workspace_root: &Path,
) -> Result<SupervisorBackend, AgentSupervisorError> {
    let store = Arc::new(
        AgentRuntimeStore::open(workspace_root)
            .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?,
    );
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::default());
    let runtime_state = crate::commands::runtime_state_from_stream_permission_runtime(
        workspace_root.to_path_buf(),
        stream_permission_runtime,
    )
    .await
    .map_err(|error| AgentSupervisorError::Runtime(error.message))?;
    let harness = runtime_state.harness().ok_or_else(|| {
        AgentSupervisorError::Runtime("agent supervisor SDK harness is unavailable".to_owned())
    })?;
    JsonlEventStore::open(
        workspace_root.join(".jyowo").join("runtime").join("events"),
        Arc::new(DefaultRedactor::default()),
    )
    .await
    .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    Ok(SupervisorBackend {
        store,
        harness,
        _runtime_state: runtime_state,
    })
}

async fn run_supervisor_loop(
    workspace_root: PathBuf,
    lock_path: PathBuf,
    mut lock: AgentSupervisorLockFile,
    listener: TcpListener,
    token_hash: String,
    heartbeat_interval: Duration,
    backend: SupervisorBackend,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut heartbeat = tokio::time::interval(heartbeat_interval);
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                lock.heartbeat_at = Utc::now();
                let _ = write_supervisor_lock(&lock_path, &lock);
                run_background_supervisor_scan(&workspace_root, &backend).await;
            }
            accepted = listener.accept() => {
                if let Ok((stream, peer)) = accepted {
                    tokio::spawn(handle_control_connection(
                        stream,
                        peer,
                        token_hash.clone(),
                        workspace_root.clone(),
                        backend.clone(),
                    ));
                }
            }
            changed = shutdown.changed() => {
                if changed.is_ok() && *shutdown.borrow() {
                    let _ = write_stopped_lock(&workspace_root, &lock_path, &mut lock);
                    break;
                }
            }
        }
    }
}

async fn handle_control_connection(
    mut stream: TcpStream,
    peer: SocketAddr,
    token_hash: String,
    workspace_root: PathBuf,
    backend: SupervisorBackend,
) {
    if !peer.ip().is_loopback() {
        let _ = write_control_response(&mut stream, false, "non_local_origin").await;
        return;
    }

    let mut buffer = vec![0_u8; CONTROL_READ_LIMIT];
    let Ok(read) = stream.read(&mut buffer).await else {
        return;
    };
    let Ok(request) = serde_json::from_slice::<SupervisorControlRequest>(&buffer[..read]) else {
        let _ = write_control_response(&mut stream, false, "invalid_request").await;
        return;
    };
    if hash_token(&request.token) != token_hash {
        let _ = write_control_response(&mut stream, false, "unauthorized").await;
        return;
    }
    let status = match request.request {
        SupervisorControlRequestKind::Status => "running",
        SupervisorControlRequestKind::Wake => {
            run_background_supervisor_scan(&workspace_root, &backend).await;
            "running"
        }
        SupervisorControlRequestKind::Shutdown => "shutdown_requested",
    };
    let _ = write_control_response(&mut stream, true, status).await;
}

pub async fn wake_agent_supervisor(workspace_root: &Path) -> Result<bool, AgentSupervisorError> {
    let Some(lock) = read_supervisor_lock(workspace_root)? else {
        return Ok(false);
    };
    let Some(token) = read_supervisor_token(workspace_root)? else {
        return Ok(false);
    };
    if lock.workspace_id != workspace_id(workspace_root)
        || token.workspace_id != lock.workspace_id
        || token.token_epoch != lock.token_epoch
        || token.token_hash != lock.token_hash
        || hash_token(&token.token) != lock.token_hash
    {
        return Ok(false);
    }
    let control_addr = parse_loopback_control_addr(&lock.control_addr)?;
    match send_control_request(
        control_addr,
        &token.token,
        SupervisorControlRequestKind::Wake,
    )
    .await
    {
        Ok(response) => Ok(response.ok),
        Err(_) => Ok(false),
    }
}

async fn run_background_supervisor_scan(workspace_root: &Path, backend: &SupervisorBackend) {
    let Ok(records) = backend.store.list_background_agents(false) else {
        return;
    };
    for record in records {
        if !matches!(
            record.state,
            BackgroundAgentState::Queued | BackgroundAgentState::Running
        ) {
            continue;
        }
        let record: BackgroundAgentRecord = record.into();
        if background_supervisor_payload_status(&record.payload_json).as_deref() != Some("queued") {
            continue;
        }
        let backend = backend.clone();
        let workspace_root = workspace_root.to_path_buf();
        tokio::spawn(async move {
            let _ = execute_background_record(&workspace_root, backend, record).await;
        });
    }
}

async fn execute_background_record(
    workspace_root: &Path,
    backend: SupervisorBackend,
    record: BackgroundAgentRecord,
) -> Result<(), AgentSupervisorError> {
    let payload = match serde_json::from_str::<BackgroundSupervisorPayload>(&record.payload_json) {
        Ok(payload) => payload,
        Err(_) => {
            fail_background_record(
                &backend,
                &record,
                &record.payload_json,
                "background supervisor payload invalid",
            )
            .await?;
            return Ok(());
        }
    };
    let Some(execution) = payload.supervisor_execution else {
        return Ok(());
    };
    if execution.status != "queued" {
        return Ok(());
    }
    let session_options = match execution.session_options_for_workspace(workspace_root) {
        Ok(options) => options,
        Err(_) => {
            fail_background_record(
                &backend,
                &record,
                &record.payload_json,
                "background supervisor payload invalid",
            )
            .await?;
            return Ok(());
        }
    };

    let running_payload = set_background_supervisor_payload_status(
        &record.payload_json,
        "running",
        record.run_id.as_deref(),
    )?;
    let claimed = backend
        .store
        .claim_background_agent_payload_json(
            &record.background_agent_id,
            &record.payload_json,
            &running_payload,
            &Utc::now().to_rfc3339(),
        )
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    if !claimed {
        return Ok(());
    }

    if run_claimed_background_record(
        &backend,
        &record,
        execution,
        session_options,
        &running_payload,
    )
    .await
    .is_err()
    {
        fail_background_record(
            &backend,
            &record,
            &running_payload,
            "background supervisor execution failed",
        )
        .await?;
    }
    Ok(())
}

async fn run_claimed_background_record(
    backend: &SupervisorBackend,
    record: &BackgroundAgentRecord,
    execution: BackgroundSupervisorExecution,
    session_options: SessionOptions,
    running_payload: &str,
) -> Result<(), AgentSupervisorError> {
    backend
        .harness
        .open_or_create_conversation_session(session_options.clone())
        .await
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    let manager = background_manager_for_record(backend, record, Some(session_options.session_id));
    let permission_actor_source = PermissionActorSource::BackgroundAgent {
        background_agent_id: parse_background_agent_id(&record.background_agent_id)?,
        conversation_id: session_options.session_id,
        attempt_id: parse_background_agent_attempt_id(record),
    };
    let receipt = backend
        .harness
        .submit_conversation_turn(ConversationTurnRequest {
            options: session_options,
            input: execution.input,
            permission_mode_override: Some(execution.permission_mode),
            permission_actor_source: Some(permission_actor_source),
            agent_run_options: Some(execution.agent_run_options),
        })
        .await
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    backend
        .store
        .update_background_agent_run_id(
            &record.background_agent_id,
            &receipt.run_id.to_string(),
            &Utc::now().to_rfc3339(),
        )
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    let completed_payload = set_background_supervisor_payload_status(
        running_payload,
        "completed",
        Some(&receipt.run_id.to_string()),
    )?;
    backend
        .store
        .update_background_agent_payload_json(
            &record.background_agent_id,
            &completed_payload,
            &Utc::now().to_rfc3339(),
        )
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    manager
        .complete(&record.background_agent_id, "background run completed")
        .await
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    Ok(())
}

fn parse_background_agent_id(value: &str) -> Result<BackgroundAgentId, AgentSupervisorError> {
    BackgroundAgentId::parse(value)
        .map_err(|_| AgentSupervisorError::Runtime("invalid background agent id".to_owned()))
}

fn parse_background_agent_attempt_id(
    record: &BackgroundAgentRecord,
) -> Option<harness_contracts::RunId> {
    record
        .run_id
        .as_deref()
        .and_then(|value| harness_contracts::RunId::parse(value).ok())
}

async fn fail_background_record(
    backend: &SupervisorBackend,
    record: &BackgroundAgentRecord,
    payload_json: &str,
    reason: &str,
) -> Result<(), AgentSupervisorError> {
    if let Ok(failed_payload) =
        set_background_supervisor_payload_status(payload_json, "failed", record.run_id.as_deref())
    {
        let _ = backend.store.update_background_agent_payload_json(
            &record.background_agent_id,
            &failed_payload,
            &Utc::now().to_rfc3339(),
        );
    }
    background_manager_for_record(backend, record, None)
        .fail(&record.background_agent_id, reason)
        .await
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    Ok(())
}

fn background_manager_for_record(
    backend: &SupervisorBackend,
    record: &BackgroundAgentRecord,
    session_id: Option<SessionId>,
) -> BackgroundAgentManager {
    let journal_session_id = session_id
        .or_else(|| SessionId::parse(&record.conversation_id).ok())
        .unwrap_or_else(SessionId::new);
    BackgroundAgentManager::new(
        Arc::clone(&backend.store),
        backend.harness.event_store(),
        TenantId::SINGLE,
        journal_session_id,
        Arc::new(DefaultRedactor::default()) as Arc<dyn Redactor>,
    )
}

fn background_supervisor_payload_status(payload_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(payload_json)
        .ok()?
        .get("supervisorExecution")?
        .get("status")?
        .as_str()
        .map(ToOwned::to_owned)
}

fn set_background_supervisor_payload_status(
    payload_json: &str,
    status: &str,
    run_id: Option<&str>,
) -> Result<String, AgentSupervisorError> {
    let mut payload = serde_json::from_str::<serde_json::Value>(payload_json)?;
    let Some(execution) = payload
        .get_mut("supervisorExecution")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return Ok(payload_json.to_owned());
    };
    let legacy_session_options = execution.remove("sessionOptions");
    if !execution.contains_key("session") {
        if let Some(session_options) = legacy_session_options {
            if let Ok(options) = serde_json::from_value::<SessionOptions>(session_options) {
                execution.insert(
                    "session".to_owned(),
                    serde_json::to_value(BackgroundSupervisorSession::from_session_options(
                        &options,
                    ))?,
                );
            }
        }
    }
    execution.insert(
        "status".to_owned(),
        serde_json::Value::String(status.to_owned()),
    );
    if let Some(run_id) = run_id {
        execution.insert(
            "runId".to_owned(),
            serde_json::Value::String(run_id.to_owned()),
        );
    }
    Ok(payload.to_string())
}

async fn write_control_response(
    stream: &mut TcpStream,
    ok: bool,
    status: &str,
) -> std::io::Result<()> {
    let response = SupervisorControlResponse {
        ok,
        status: status.to_owned(),
    };
    let bytes = serde_json::to_vec(&response)?;
    stream.write_all(&bytes).await
}

async fn send_control_request(
    control_addr: SocketAddr,
    token: &str,
    request: SupervisorControlRequestKind,
) -> Result<SupervisorControlResponse, AgentSupervisorError> {
    let mut stream = TcpStream::connect(control_addr).await?;
    let bytes = serde_json::to_vec(&SupervisorControlRequest {
        token: token.to_owned(),
        request,
    })?;
    stream.write_all(&bytes).await?;
    let mut buffer = vec![0_u8; CONTROL_READ_LIMIT];
    let read = stream.read(&mut buffer).await?;
    Ok(serde_json::from_slice(&buffer[..read])?)
}

fn write_stopped_lock(
    workspace_root: &Path,
    lock_path: &Path,
    lock: &mut AgentSupervisorLockFile,
) -> Result<(), AgentSupervisorError> {
    lock.status = "stopped".to_owned();
    lock.heartbeat_at = Utc::now();
    lock.workspace_id = workspace_id(workspace_root);
    write_supervisor_lock(lock_path, lock)
}

fn write_supervisor_lock(
    lock_path: &Path,
    lock: &AgentSupervisorLockFile,
) -> Result<(), AgentSupervisorError> {
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(lock)?;
    write_non_secret_file_atomic(lock_path, &bytes)?;
    Ok(())
}

fn write_non_secret_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), AgentSupervisorError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("agent-supervisor-file");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let tmp_path = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), nonce));
    std::fs::write(&tmp_path, bytes)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

fn create_supervisor_token(workspace_root: &Path) -> Result<SupervisorToken, AgentSupervisorError> {
    let token = new_local_token();
    let token_hash = hash_token(&token);
    let token_epoch = Utc::now().timestamp_millis().max(0) as u64;
    write_supervisor_token(
        workspace_root,
        &AgentSupervisorTokenFile {
            token: token.clone(),
            token_hash: token_hash.clone(),
            token_epoch,
            workspace_id: workspace_id(workspace_root),
            created_at: Utc::now(),
        },
    )?;
    Ok(SupervisorToken {
        token,
        token_hash,
        token_epoch,
    })
}

fn supervisor_token_from_env(
    workspace_root: &Path,
) -> Result<Option<SupervisorToken>, AgentSupervisorError> {
    let Some(token) =
        std::env::var_os(SUPERVISOR_TOKEN_ENV).map(|value| value.to_string_lossy().to_string())
    else {
        return Ok(None);
    };
    let token_epoch = std::env::var_os(SUPERVISOR_TOKEN_EPOCH_ENV)
        .and_then(|value| value.to_string_lossy().parse::<u64>().ok())
        .unwrap_or_else(|| Utc::now().timestamp_millis().max(0) as u64);
    let token_hash = hash_token(&token);
    write_supervisor_token(
        workspace_root,
        &AgentSupervisorTokenFile {
            token: token.clone(),
            token_hash: token_hash.clone(),
            token_epoch,
            workspace_id: workspace_id(workspace_root),
            created_at: Utc::now(),
        },
    )?;
    Ok(Some(SupervisorToken {
        token,
        token_hash,
        token_epoch,
    }))
}

fn read_supervisor_token(
    workspace_root: &Path,
) -> Result<Option<AgentSupervisorTokenFile>, AgentSupervisorError> {
    let path = supervisor_token_path(workspace_root);
    if !path.is_file() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&contents)?))
}

fn write_supervisor_token(
    workspace_root: &Path,
    token: &AgentSupervisorTokenFile,
) -> Result<(), AgentSupervisorError> {
    let path = supervisor_token_path(workspace_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(token)?;
    write_owner_only_file(&path, &bytes)?;
    Ok(())
}

#[cfg(unix)]
fn write_owner_only_file(path: &Path, bytes: &[u8]) -> Result<(), AgentSupervisorError> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_owner_only_file(path: &Path, bytes: &[u8]) -> Result<(), AgentSupervisorError> {
    std::fs::write(path, bytes)?;
    Ok(())
}

async fn wait_for_supervisor_lock(
    workspace_root: &Path,
    token_hash: &str,
    timeout: Duration,
) -> Result<(), AgentSupervisorError> {
    let started = std::time::Instant::now();
    while started.elapsed() <= timeout {
        if let Some(lock) = read_supervisor_lock(workspace_root)? {
            if lock.token_hash == token_hash
                && supervisor_lock_is_fresh(&lock, DEFAULT_HEARTBEAT_STALE_AFTER)
            {
                return Ok(());
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    Err(AgentSupervisorError::Runtime(
        "agent supervisor sidecar did not publish a fresh lock".to_owned(),
    ))
}

fn parse_loopback_control_addr(control_addr: &str) -> Result<SocketAddr, AgentSupervisorError> {
    let addr = control_addr.parse::<SocketAddr>().map_err(|error| {
        AgentSupervisorError::Runtime(format!("invalid control address: {error}"))
    })?;
    if !addr.ip().is_loopback() {
        return Err(AgentSupervisorError::Runtime(
            "agent supervisor control address is not loopback".to_owned(),
        ));
    }
    Ok(addr)
}

fn redact_supervisor_output(redactor: &DefaultRedactor, bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    redactor.redact(
        &text,
        &RedactRules {
            scope: RedactScope::LogOnly,
            replacement: "[REDACTED]".to_owned(),
            pattern_set: RedactPatternSet::AllBuiltins,
        },
    )
}

fn new_local_token() -> String {
    let seed = format!(
        "{}:{}:{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        uuid::Uuid::new_v4()
    );
    blake3::hash(seed.as_bytes()).to_hex().to_string()
}

fn hash_token(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

fn workspace_id(workspace_root: &Path) -> String {
    blake3::hash(workspace_root.display().to_string().as_bytes())
        .to_hex()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supervisor_lock_freshness_uses_heartbeat() {
        let lock = AgentSupervisorLockFile {
            status: "running".to_owned(),
            workspace_id: "workspace".to_owned(),
            token_hash: "hash".to_owned(),
            token_epoch: 1,
            pid: 1,
            control_addr: "127.0.0.1:1".to_owned(),
            started_at: Utc::now(),
            heartbeat_at: Utc::now(),
        };

        assert!(supervisor_lock_is_fresh(&lock, Duration::from_secs(5)));
    }

    #[test]
    fn supervisor_sidecar_args_only_include_workspace_root_arg() {
        let args = supervisor_sidecar_args(Path::new("/tmp/workspace"));
        assert_eq!(args[0], "--workspace-root");
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn supervisor_output_redaction_removes_secret_patterns() {
        let redactor = DefaultRedactor::default();
        let redacted =
            redact_supervisor_output(&redactor, b"Authorization: Bearer abcdef1234567890abcdef");
        assert!(!redacted.contains("abcdef1234567890abcdef"));
    }
}

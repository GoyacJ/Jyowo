use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use harness_contracts::{
    AgentProfile, AgentToolPolicy, BackgroundAgentId, BackgroundAgentState,
    BackgroundAgentToolSessionSnapshot, ConversationTurnInput, InteractivityLevel, ModelProtocol,
    PermissionActorSource, PermissionMode, RedactPatternSet, RedactRules, RedactScope, Redactor,
    TeamId, ToolProfile, ToolSearchMode,
};
use jyowo_harness_sdk::builtin::{DefaultRedactor, JsonlEventStore};
use jyowo_harness_sdk::ext::{EventStore, SessionId, TenantId};
use jyowo_harness_sdk::{
    builtin_agent_profiles, resolve_agent_runtime_policy, AgentCapabilitiesInput,
    AgentRuntimeStore, BackgroundAgentManager, BackgroundAgentRecord, ConversationRunOptions,
    ConversationTurnRequest, ExecutionSettingsAgentInput, Harness, SessionOptions,
    StreamPermissionRuntime,
};
use parking_lot::Mutex as ParkingMutex;
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
const SIDECAR_RUNTIME_ARG: &str = "--runtime-root";
const SIDECAR_WORKSPACE_ARG: &str = "--workspace-root";
const SIDECAR_CONVERSATION_ARG: &str = "--conversation-id";
const CONTROL_READ_LIMIT: usize = 8192;
const SIDECAR_STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
pub const DEFAULT_HEARTBEAT_STALE_AFTER: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSupervisorScope {
    runtime_root: PathBuf,
    workspace_root: Option<PathBuf>,
    conversation_id: Option<SessionId>,
}

impl AgentSupervisorScope {
    #[must_use]
    pub fn project(workspace_root: impl Into<PathBuf>) -> Self {
        let workspace_root = workspace_root.into();
        let workspace_root = workspace_root.canonicalize().unwrap_or(workspace_root);
        Self {
            runtime_root: workspace_root.join(".jyowo").join("runtime"),
            workspace_root: Some(workspace_root),
            conversation_id: None,
        }
    }

    #[must_use]
    pub fn runtime(runtime_root: impl Into<PathBuf>) -> Self {
        Self {
            runtime_root: runtime_root.into(),
            workspace_root: None,
            conversation_id: None,
        }
    }

    #[must_use]
    pub fn runtime_conversation(
        runtime_root: impl Into<PathBuf>,
        conversation_id: SessionId,
    ) -> Self {
        Self {
            runtime_root: runtime_root.into(),
            workspace_root: None,
            conversation_id: Some(conversation_id),
        }
    }

    #[must_use]
    pub fn runtime_root(&self) -> &Path {
        &self.runtime_root
    }

    #[must_use]
    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace_root.as_deref()
    }

    #[must_use]
    pub fn conversation_id(&self) -> Option<SessionId> {
        self.conversation_id
    }

    fn control_cwd(&self) -> &Path {
        self.workspace_root
            .as_deref()
            .unwrap_or_else(|| self.runtime_root.as_path())
    }

    fn session_cwd(&self, session_id: SessionId) -> PathBuf {
        self.workspace_root.clone().unwrap_or_else(|| {
            self.runtime_root
                .join("workdir")
                .join(session_id.to_string())
        })
    }

    fn identity(&self) -> String {
        let scope = match &self.workspace_root {
            Some(workspace_root) => format!("project:{}", workspace_root.display()),
            None => match self.conversation_id {
                Some(conversation_id) => {
                    format!("runtime:{}:{conversation_id}", self.runtime_root.display())
                }
                None => format!("runtime:{}", self.runtime_root.display()),
            },
        };
        blake3::hash(scope.as_bytes()).to_hex().to_string()
    }
}

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
    CancelBackgroundAgent { background_agent_id: String },
    PauseBackgroundAgent { background_agent_id: String },
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
    active_runs: Arc<ParkingMutex<HashMap<String, ActiveBackgroundRun>>>,
    scope: AgentSupervisorScope,
    _runtime_state: crate::commands::DesktopRuntimeState,
}

#[derive(Clone)]
struct ActiveBackgroundRun {
    harness: Arc<Harness>,
    run_id: Option<jyowo_harness_sdk::ext::RunId>,
    cancel_requested: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundSupervisorPayload {
    source: String,
    #[serde(default)]
    supervisor_execution: Option<BackgroundSupervisorExecution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundSupervisorExecution {
    status: String,
    #[serde(default)]
    session: Option<BackgroundSupervisorSession>,
    input: ConversationTurnInput,
    model_config_id: String,
    permission_mode: PermissionMode,
    agent_tool_policy: AgentToolPolicy,
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
    pub(crate) fn from_tool_session_snapshot(snapshot: BackgroundAgentToolSessionSnapshot) -> Self {
        Self {
            tenant_id: snapshot.tenant_id,
            session_id: snapshot.session_id,
            tool_search: snapshot.tool_search,
            tool_profile: snapshot.tool_profile,
            model_id: None,
            protocol: None,
            permission_mode: snapshot.permission_mode,
            interactivity: snapshot.interactivity,
            team_id: snapshot.team_id,
            max_iterations: snapshot.max_iterations,
            context_compression_trigger_ratio: snapshot.context_compression_trigger_ratio,
        }
    }

    fn into_session_options(self, scope: &AgentSupervisorScope) -> SessionOptions {
        let session_id = self.session_id;
        let mut options = SessionOptions::new(scope.session_cwd(session_id));
        options.agent_runtime_root = Some(scope.runtime_root().to_path_buf());
        if let Some(workspace_root) = scope.workspace_root() {
            options.project_workspace_root = Some(workspace_root.to_path_buf());
        }
        options.tenant_id = self.tenant_id;
        options.session_id = session_id;
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
    fn session_options_for_scope(
        &self,
        scope: &AgentSupervisorScope,
    ) -> Result<SessionOptions, AgentSupervisorError> {
        if let Some(session) = self.session.clone() {
            return Ok(session.into_session_options(scope));
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
    start_agent_supervisor_with_scope_timing(
        AgentSupervisorScope::project(workspace_root),
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
    start_agent_supervisor_with_scope_timing(
        AgentSupervisorScope::project(workspace_root),
        heartbeat_interval,
        stale_after,
    )
    .await
}

pub async fn start_agent_supervisor_with_scope_timing(
    scope: AgentSupervisorScope,
    heartbeat_interval: Duration,
    stale_after: Duration,
) -> Result<AgentSupervisorHandle, AgentSupervisorError> {
    let token = create_supervisor_token_scope(&scope)?;
    start_agent_supervisor_with_token(scope, heartbeat_interval, stale_after, token).await
}

async fn start_agent_supervisor_with_token(
    scope: AgentSupervisorScope,
    heartbeat_interval: Duration,
    stale_after: Duration,
    token: SupervisorToken,
) -> Result<AgentSupervisorHandle, AgentSupervisorError> {
    let backend = open_supervisor_backend_for_scope(&scope).await?;
    recover_if_stale_scope(&scope, stale_after).await?;

    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let control_addr = listener.local_addr()?;
    let lock_path = supervisor_lock_path_for_scope(&scope);
    let lock = AgentSupervisorLockFile {
        status: "running".to_owned(),
        workspace_id: scope.identity(),
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
        scope,
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
    run_supervisor_process_for_scope(AgentSupervisorScope::project(workspace_root)).await
}

pub async fn run_supervisor_process_for_scope(
    scope: AgentSupervisorScope,
) -> Result<(), AgentSupervisorError> {
    let token = match supervisor_token_from_env_scope(&scope)? {
        Some(token) => token,
        None => create_supervisor_token_scope(&scope)?,
    };
    let handle = start_agent_supervisor_with_token(
        scope,
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
    launch_agent_supervisor_sidecar_for_scope(app, AgentSupervisorScope::project(workspace_root))
        .await
}

pub async fn launch_agent_supervisor_sidecar_for_scope<R: Runtime>(
    app: &tauri::AppHandle<R>,
    scope: AgentSupervisorScope,
) -> Result<(), AgentSupervisorError> {
    if reconnect_to_existing_supervisor_scope(&scope, DEFAULT_HEARTBEAT_STALE_AFTER).await? {
        return Ok(());
    }

    let token = create_supervisor_token_scope(&scope)?;
    let command = app
        .shell()
        .sidecar(SIDECAR_NAME)
        .map_err(|error| AgentSupervisorError::Sidecar(error.to_string()))?
        .args(supervisor_sidecar_args_for_scope(&scope))
        .env(SUPERVISOR_TOKEN_ENV, &token.token)
        .env(SUPERVISOR_TOKEN_EPOCH_ENV, token.token_epoch.to_string())
        .current_dir(scope.control_cwd());
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

    wait_for_supervisor_lock_scope(&scope, &token.token_hash, SIDECAR_STARTUP_TIMEOUT).await
}

pub fn supervisor_sidecar_args(workspace_root: &Path) -> Vec<String> {
    supervisor_sidecar_args_for_scope(&AgentSupervisorScope::project(workspace_root.to_path_buf()))
}

pub fn supervisor_sidecar_args_for_scope(scope: &AgentSupervisorScope) -> Vec<String> {
    let mut args = vec![
        SIDECAR_RUNTIME_ARG.to_owned(),
        scope.runtime_root().display().to_string(),
    ];
    if let Some(workspace_root) = scope.workspace_root() {
        args.push(SIDECAR_WORKSPACE_ARG.to_owned());
        args.push(workspace_root.display().to_string());
    } else if let Some(conversation_id) = scope.conversation_id() {
        args.push(SIDECAR_CONVERSATION_ARG.to_owned());
        args.push(conversation_id.to_string());
    }
    args
}

pub fn supervisor_lock_path(workspace_root: &Path) -> PathBuf {
    supervisor_lock_path_for_scope(&AgentSupervisorScope::project(workspace_root.to_path_buf()))
}

fn supervisor_lock_path_for_scope(scope: &AgentSupervisorScope) -> PathBuf {
    scope.runtime_root().join(SUPERVISOR_LOCK_FILE)
}

fn supervisor_token_path_for_scope(scope: &AgentSupervisorScope) -> PathBuf {
    scope.runtime_root().join(SUPERVISOR_TOKEN_FILE)
}

pub fn legacy_supervisor_sidecar_args(workspace_root: &Path) -> Vec<String> {
    vec![
        SIDECAR_WORKSPACE_ARG.to_owned(),
        workspace_root.display().to_string(),
    ]
}

pub fn read_supervisor_lock(
    workspace_root: &Path,
) -> Result<Option<AgentSupervisorLockFile>, AgentSupervisorError> {
    read_supervisor_lock_scope(&AgentSupervisorScope::project(workspace_root.to_path_buf()))
}

pub fn read_supervisor_lock_scope(
    scope: &AgentSupervisorScope,
) -> Result<Option<AgentSupervisorLockFile>, AgentSupervisorError> {
    let path = supervisor_lock_path_for_scope(scope);
    crate::commands::stores::read_json_file(&path, "agent supervisor lock")
        .map_err(agent_supervisor_store_error)
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
    recover_if_stale_scope(
        &AgentSupervisorScope::project(workspace_root.to_path_buf()),
        stale_after,
    )
    .await
}

pub async fn recover_if_stale_scope(
    scope: &AgentSupervisorScope,
    stale_after: Duration,
) -> Result<(), AgentSupervisorError> {
    let Some(lock) = read_supervisor_lock_scope(scope)? else {
        return Ok(());
    };
    if supervisor_lock_is_fresh(&lock, stale_after) {
        return Ok(());
    }
    mark_running_agents_interrupted_scope(scope, "agent supervisor heartbeat missed").await
}

pub async fn reconnect_to_existing_supervisor(
    workspace_root: &Path,
    stale_after: Duration,
) -> Result<bool, AgentSupervisorError> {
    reconnect_to_existing_supervisor_scope(
        &AgentSupervisorScope::project(workspace_root.to_path_buf()),
        stale_after,
    )
    .await
}

pub async fn reconnect_to_existing_supervisor_scope(
    scope: &AgentSupervisorScope,
    stale_after: Duration,
) -> Result<bool, AgentSupervisorError> {
    let Some(lock) = read_supervisor_lock_scope(scope)? else {
        return Ok(false);
    };
    if !supervisor_lock_is_fresh(&lock, stale_after) {
        mark_running_agents_interrupted_scope(scope, "agent supervisor heartbeat missed").await?;
        return Ok(false);
    }
    let Some(token) = read_supervisor_token_scope(scope)? else {
        return Ok(false);
    };
    if token.workspace_id != lock.workspace_id
        || token.token_epoch != lock.token_epoch
        || token.token_hash != lock.token_hash
        || hash_token(&token.token) != lock.token_hash
        || lock.workspace_id != scope.identity()
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
            mark_running_agents_interrupted_scope(
                scope,
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
    mark_running_agents_interrupted_scope(
        &AgentSupervisorScope::project(workspace_root.to_path_buf()),
        reason,
    )
    .await
}

pub async fn mark_running_agents_interrupted_scope(
    scope: &AgentSupervisorScope,
    reason: &str,
) -> Result<(), AgentSupervisorError> {
    let runtime_dir = scope.runtime_root();
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(runtime_dir)
            .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?,
    );
    let event_store = Arc::new(
        JsonlEventStore::open(
            runtime_dir.join("events"),
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

async fn open_supervisor_backend_for_scope(
    scope: &AgentSupervisorScope,
) -> Result<SupervisorBackend, AgentSupervisorError> {
    let runtime_dir = scope.runtime_root();
    let store = Arc::new(
        AgentRuntimeStore::open_runtime_dir(runtime_dir)
            .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?,
    );
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::default());
    let runtime_state = if let Some(workspace_root) = scope.workspace_root() {
        crate::commands::runtime_state_from_stream_permission_runtime(
            workspace_root.to_path_buf(),
            stream_permission_runtime,
        )
        .await
        .map_err(|error| AgentSupervisorError::Runtime(error.message))?
    } else {
        let conversation_id = scope.conversation_id().ok_or_else(|| {
            AgentSupervisorError::Runtime(
                "runtime supervisor scope missing conversation id".to_owned(),
            )
        })?;
        crate::commands::runtime_state_for_global_conversation_with_runtime_root(
            conversation_id,
            runtime_dir.to_path_buf(),
            stream_permission_runtime,
        )
        .await
        .map_err(|error| AgentSupervisorError::Runtime(error.message))?
    };
    let harness = runtime_state.harness().ok_or_else(|| {
        AgentSupervisorError::Runtime("agent supervisor SDK harness is unavailable".to_owned())
    })?;
    JsonlEventStore::open(
        runtime_dir.join("events"),
        Arc::new(DefaultRedactor::default()),
    )
    .await
    .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    Ok(SupervisorBackend {
        store,
        harness,
        active_runs: Arc::new(ParkingMutex::new(HashMap::new())),
        scope: scope.clone(),
        _runtime_state: runtime_state,
    })
}

async fn run_supervisor_loop(
    scope: AgentSupervisorScope,
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
                run_background_supervisor_scan(&scope, &backend).await;
            }
            accepted = listener.accept() => {
                if let Ok((stream, peer)) = accepted {
                    tokio::spawn(handle_control_connection(
                        stream,
                        peer,
                        token_hash.clone(),
                        scope.clone(),
                        backend.clone(),
                    ));
                }
            }
            changed = shutdown.changed() => {
                if changed.is_ok() && *shutdown.borrow() {
                    let _ = write_stopped_lock_scope(&scope, &lock_path, &mut lock);
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
    scope: AgentSupervisorScope,
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
            run_background_supervisor_scan(&scope, &backend).await;
            "running"
        }
        SupervisorControlRequestKind::CancelBackgroundAgent {
            background_agent_id,
        } => match cancel_active_background_run(&backend, &background_agent_id).await {
            Ok(true) => "cancelled",
            Ok(false) => "not_active",
            Err(_) => "cancel_failed",
        },
        SupervisorControlRequestKind::PauseBackgroundAgent {
            background_agent_id,
        } => match cancel_active_background_run(&backend, &background_agent_id).await {
            Ok(true) => "paused",
            Ok(false) => "not_active",
            Err(_) => "pause_failed",
        },
        SupervisorControlRequestKind::Shutdown => "shutdown_requested",
    };
    let _ = write_control_response(&mut stream, true, status).await;
}

pub async fn wake_agent_supervisor(workspace_root: &Path) -> Result<bool, AgentSupervisorError> {
    wake_agent_supervisor_scope(&AgentSupervisorScope::project(workspace_root.to_path_buf())).await
}

pub async fn wake_agent_supervisor_scope(
    scope: &AgentSupervisorScope,
) -> Result<bool, AgentSupervisorError> {
    let Some(lock) = read_supervisor_lock_scope(scope)? else {
        return Ok(false);
    };
    let Some(token) = read_supervisor_token_scope(scope)? else {
        return Ok(false);
    };
    if lock.workspace_id != scope.identity()
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

pub fn agent_supervisor_available_for_scope(scope: &AgentSupervisorScope) -> bool {
    let Ok(Some(lock)) = read_supervisor_lock_scope(scope) else {
        return false;
    };
    if !supervisor_lock_is_fresh(&lock, DEFAULT_HEARTBEAT_STALE_AFTER) {
        return false;
    }
    let Ok(Some(token)) = read_supervisor_token_scope(scope) else {
        return false;
    };
    token.workspace_id == lock.workspace_id
        && token.token_epoch == lock.token_epoch
        && token.token_hash == lock.token_hash
        && hash_token(&token.token) == lock.token_hash
        && lock.workspace_id == scope.identity()
}

pub async fn cancel_background_agent_run(
    workspace_root: &Path,
    background_agent_id: &str,
) -> Result<bool, AgentSupervisorError> {
    cancel_background_agent_run_scope(
        &AgentSupervisorScope::project(workspace_root.to_path_buf()),
        background_agent_id,
    )
    .await
}

pub async fn cancel_background_agent_run_scope(
    scope: &AgentSupervisorScope,
    background_agent_id: &str,
) -> Result<bool, AgentSupervisorError> {
    send_background_agent_control_scope(
        scope,
        SupervisorControlRequestKind::CancelBackgroundAgent {
            background_agent_id: background_agent_id.to_owned(),
        },
    )
    .await
}

pub async fn pause_background_agent_run(
    workspace_root: &Path,
    background_agent_id: &str,
) -> Result<bool, AgentSupervisorError> {
    pause_background_agent_run_scope(
        &AgentSupervisorScope::project(workspace_root.to_path_buf()),
        background_agent_id,
    )
    .await
}

pub async fn pause_background_agent_run_scope(
    scope: &AgentSupervisorScope,
    background_agent_id: &str,
) -> Result<bool, AgentSupervisorError> {
    send_background_agent_control_scope(
        scope,
        SupervisorControlRequestKind::PauseBackgroundAgent {
            background_agent_id: background_agent_id.to_owned(),
        },
    )
    .await
}

pub fn requeue_background_agent_supervisor_payload(
    workspace_root: &Path,
    background_agent_id: &str,
) -> Result<bool, AgentSupervisorError> {
    requeue_background_agent_supervisor_payload_scope(
        &AgentSupervisorScope::project(workspace_root.to_path_buf()),
        background_agent_id,
    )
}

pub fn requeue_background_agent_supervisor_payload_scope(
    scope: &AgentSupervisorScope,
    background_agent_id: &str,
) -> Result<bool, AgentSupervisorError> {
    let store = AgentRuntimeStore::open_runtime_dir(scope.runtime_root())
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    let Some(record) = store
        .get_background_agent(background_agent_id)
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?
    else {
        return Ok(false);
    };
    let payload = set_background_supervisor_payload_status(
        &record.payload_json,
        "queued",
        record.run_id.as_deref(),
    )?;
    store
        .update_background_agent_payload_json(
            background_agent_id,
            &payload,
            &Utc::now().to_rfc3339(),
        )
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    Ok(true)
}

async fn send_background_agent_control_scope(
    scope: &AgentSupervisorScope,
    request: SupervisorControlRequestKind,
) -> Result<bool, AgentSupervisorError> {
    let Some(lock) = read_supervisor_lock_scope(scope)? else {
        return Ok(false);
    };
    let Some(token) = read_supervisor_token_scope(scope)? else {
        return Ok(false);
    };
    if lock.workspace_id != scope.identity()
        || token.workspace_id != lock.workspace_id
        || token.token_epoch != lock.token_epoch
        || token.token_hash != lock.token_hash
        || hash_token(&token.token) != lock.token_hash
    {
        return Ok(false);
    }
    let control_addr = parse_loopback_control_addr(&lock.control_addr)?;
    match send_control_request(control_addr, &token.token, request).await {
        Ok(response) => Ok(response.ok),
        Err(_) => Ok(false),
    }
}

async fn cancel_active_background_run(
    backend: &SupervisorBackend,
    background_agent_id: &str,
) -> Result<bool, AgentSupervisorError> {
    let active = {
        let mut active_runs = backend.active_runs.lock();
        let Some(active) = active_runs.get_mut(background_agent_id) else {
            return Ok(false);
        };
        active.cancel_requested = true;
        active.clone()
    };
    let Some(run_id) = active.run_id else {
        return Ok(true);
    };
    active
        .harness
        .cancel_conversation_run(run_id)
        .await
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    Ok(true)
}

async fn run_background_supervisor_scan(scope: &AgentSupervisorScope, backend: &SupervisorBackend) {
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
        let scope = scope.clone();
        tokio::spawn(async move {
            let _ = execute_background_record(&scope, backend, record).await;
        });
    }
}

async fn execute_background_record(
    scope: &AgentSupervisorScope,
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
    if payload.source != "background_agent_tool" {
        fail_background_record(
            &backend,
            &record,
            &record.payload_json,
            "background supervisor payload source unsupported",
        )
        .await?;
        return Ok(());
    }
    if execution.status != "queued" {
        return Ok(());
    }
    let session_options = match execution.session_options_for_scope(scope) {
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

    if let Err(error) = run_claimed_background_record(
        &backend,
        &record,
        execution,
        session_options,
        &running_payload,
    )
    .await
    {
        fail_background_record(
            &backend,
            &record,
            &running_payload,
            &format!("background supervisor execution failed: {error}"),
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
    let stream_permission_runtime = backend
        ._runtime_state
        .stream_permission_runtime
        .as_ref()
        .ok_or_else(|| {
            AgentSupervisorError::Runtime("agent supervisor runtime is unavailable".to_owned())
        })?;
    let session_id = session_options.session_id;
    let agent_tool_policy =
        resolve_background_supervisor_agent_tool_policy(backend, &execution, session_id)?;
    let layout = if let Some(workspace_root) = backend.scope.workspace_root() {
        crate::commands::project_runtime_layout(workspace_root)
    } else {
        crate::commands::global_conversation_runtime_layout_with_runtime_root(
            session_id,
            backend.scope.runtime_root().to_path_buf(),
        )
    };
    let (harness, model_id, protocol) = crate::commands::build_desktop_harness(
        &layout,
        Arc::clone(stream_permission_runtime),
        Some(&execution.model_config_id),
        Arc::clone(&backend._runtime_state.provider_capability_routes),
        Some(Arc::clone(&backend._runtime_state.provider_settings_store)),
    )
    .await
    .map_err(|error| AgentSupervisorError::Runtime(error.message))?;
    let harness = Arc::new(harness);
    let mut session_options = session_options;
    session_options.model_id = Some(model_id.clone());
    session_options.protocol = Some(protocol);
    session_options.agent_profiles = background_agent_profiles(backend)?;
    harness
        .open_or_create_conversation_session(session_options.clone())
        .await
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    let session_id = session_options.session_id;
    let manager = background_manager_for_record(backend, record, Some(session_id));
    let permission_actor_source = PermissionActorSource::BackgroundAgent {
        background_agent_id: parse_background_agent_id(&record.background_agent_id)?,
        conversation_id: session_id,
        attempt_id: parse_background_agent_attempt_id(record),
    };
    let model_config_id = execution.model_config_id.clone();
    let mut run_options = ConversationRunOptions::from_session_options(&session_options)
        .with_model_config_id(model_config_id.clone())
        .with_model_id(model_id)
        .with_protocol(protocol)
        .with_permission_mode(execution.permission_mode);
    run_options.agent_tool_policy = Some(agent_tool_policy);
    let after_event_id =
        crate::commands::conversation_tail_event_id(&harness, session_options.clone())
            .await
            .map_err(|error| AgentSupervisorError::Runtime(error.message))?;
    backend.active_runs.lock().insert(
        record.background_agent_id.clone(),
        ActiveBackgroundRun {
            harness: Arc::clone(&harness),
            run_id: None,
            cancel_requested: false,
        },
    );
    let run_harness = Arc::clone(&harness);
    let run_session_options = session_options.clone();
    let mut run_task = tokio::spawn(async move {
        run_harness
            .submit_conversation_turn(ConversationTurnRequest {
                options: run_session_options,
                run_options,
                input: execution.input,
                permission_actor_source: Some(permission_actor_source),
            })
            .await
    });
    let run_id = crate::commands::wait_for_started_conversation_run(
        &harness,
        session_options.clone(),
        after_event_id,
        &mut run_task,
    )
    .await
    .map_err(|error| AgentSupervisorError::Runtime(error.message))?;
    crate::commands::mark_conversation_metadata_active(
        session_id,
        Some(model_config_id),
        &backend._runtime_state,
    )
    .await
    .map_err(|error| AgentSupervisorError::Runtime(error.message))?;
    backend
        .store
        .update_background_agent_run_id(
            &record.background_agent_id,
            &run_id.to_string(),
            &Utc::now().to_rfc3339(),
        )
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    let cancel_after_start = {
        let mut active_runs = backend.active_runs.lock();
        match active_runs.get_mut(&record.background_agent_id) {
            Some(active) => {
                active.run_id = Some(run_id);
                active.cancel_requested
            }
            None => {
                active_runs.insert(
                    record.background_agent_id.clone(),
                    ActiveBackgroundRun {
                        harness: Arc::clone(&harness),
                        run_id: Some(run_id),
                        cancel_requested: false,
                    },
                );
                false
            }
        }
    };
    if cancel_after_start || !background_agent_can_finish(backend, &record.background_agent_id)? {
        let _ = harness.cancel_conversation_run(run_id).await;
    }
    let receipt = run_task
        .await
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()));
    backend
        .active_runs
        .lock()
        .remove(&record.background_agent_id);
    let receipt = receipt?.map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?;
    if !background_agent_can_finish(backend, &record.background_agent_id)? {
        return Ok(());
    }
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

fn resolve_background_supervisor_agent_tool_policy(
    backend: &SupervisorBackend,
    execution: &BackgroundSupervisorExecution,
    conversation_id: SessionId,
) -> Result<AgentToolPolicy, AgentSupervisorError> {
    let settings = backend
        ._runtime_state
        .effective_execution_settings(None)
        .map_err(|error| AgentSupervisorError::Runtime(error.message))?;
    let capabilities_payload =
        if let Some(project_workspace_root) = backend._runtime_state.project_workspace_root() {
            crate::commands::agent_capabilities_payload(
                &settings,
                project_workspace_root,
                Some(&backend._runtime_state.agent_capability_resolution_context()),
            )
        } else {
            crate::commands::no_workspace_agent_capabilities_payload_for_conversation(
                &settings,
                backend._runtime_state.runtime_root(),
                Some(conversation_id),
                Some(&backend._runtime_state.agent_capability_resolution_context()),
            )
        };
    let capabilities = AgentCapabilitiesInput {
        subagents_available: capabilities_payload.subagents_available,
        agent_teams_available: capabilities_payload.agent_teams_available,
        background_agents_available: capabilities_payload.background_agents_available,
    };
    let settings_input = ExecutionSettingsAgentInput {
        subagents_enabled: settings.subagents_enabled,
        agent_teams_enabled: settings.agent_teams_enabled,
        background_agents_enabled: settings.background_agents_enabled,
    };
    let profiles = background_agent_profiles(backend)?;
    let profile_ids: Vec<String> = profiles.into_iter().map(|profile| profile.id).collect();
    let policy_root = backend
        ._runtime_state
        .project_workspace_root()
        .unwrap_or_else(|| backend._runtime_state.conversation_cwd());
    resolve_agent_runtime_policy(
        policy_root,
        &settings_input,
        Some(&execution.agent_tool_policy),
        &capabilities,
        &profile_ids,
        &conversation_id.to_string(),
    )
    .map(|resolved| resolved.options)
    .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))
}

fn background_agent_profiles(
    backend: &SupervisorBackend,
) -> Result<Vec<AgentProfile>, AgentSupervisorError> {
    let mut profiles = builtin_agent_profiles();
    if let Some(global_config_store) = backend._runtime_state.global_config_store.as_ref() {
        profiles.extend(
            global_config_store
                .load_global_agent_profiles()
                .map_err(|error| AgentSupervisorError::Runtime(error.message))?,
        );
    }
    Ok(profiles)
}

fn background_agent_can_finish(
    backend: &SupervisorBackend,
    background_agent_id: &str,
) -> Result<bool, AgentSupervisorError> {
    let Some(record) = backend
        .store
        .get_background_agent(background_agent_id)
        .map_err(|error| AgentSupervisorError::Runtime(error.to_string()))?
    else {
        return Ok(false);
    };
    Ok(matches!(
        record.state,
        BackgroundAgentState::Running | BackgroundAgentState::Queued
    ))
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
    backend
        .active_runs
        .lock()
        .remove(&record.background_agent_id);
    if !background_agent_can_finish(backend, &record.background_agent_id)? {
        return Ok(());
    }
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
    execution.remove("sessionOptions");
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

fn write_stopped_lock_scope(
    scope: &AgentSupervisorScope,
    lock_path: &Path,
    lock: &mut AgentSupervisorLockFile,
) -> Result<(), AgentSupervisorError> {
    lock.status = "stopped".to_owned();
    lock.heartbeat_at = Utc::now();
    lock.workspace_id = scope.identity();
    write_supervisor_lock(lock_path, lock)
}

fn write_supervisor_lock(
    lock_path: &Path,
    lock: &AgentSupervisorLockFile,
) -> Result<(), AgentSupervisorError> {
    crate::commands::stores::write_json_file_atomic(lock_path, "agent supervisor lock", lock)
        .map_err(agent_supervisor_store_error)
}

fn create_supervisor_token_scope(
    scope: &AgentSupervisorScope,
) -> Result<SupervisorToken, AgentSupervisorError> {
    let token = new_local_token();
    let token_hash = hash_token(&token);
    let token_epoch = Utc::now().timestamp_millis().max(0) as u64;
    write_supervisor_token_scope(
        scope,
        &AgentSupervisorTokenFile {
            token: token.clone(),
            token_hash: token_hash.clone(),
            token_epoch,
            workspace_id: scope.identity(),
            created_at: Utc::now(),
        },
    )?;
    Ok(SupervisorToken {
        token,
        token_hash,
        token_epoch,
    })
}

fn supervisor_token_from_env_scope(
    scope: &AgentSupervisorScope,
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
    write_supervisor_token_scope(
        scope,
        &AgentSupervisorTokenFile {
            token: token.clone(),
            token_hash: token_hash.clone(),
            token_epoch,
            workspace_id: scope.identity(),
            created_at: Utc::now(),
        },
    )?;
    Ok(Some(SupervisorToken {
        token,
        token_hash,
        token_epoch,
    }))
}

#[cfg(test)]
fn read_supervisor_token(
    workspace_root: &Path,
) -> Result<Option<AgentSupervisorTokenFile>, AgentSupervisorError> {
    read_supervisor_token_scope(&AgentSupervisorScope::project(workspace_root.to_path_buf()))
}

fn read_supervisor_token_scope(
    scope: &AgentSupervisorScope,
) -> Result<Option<AgentSupervisorTokenFile>, AgentSupervisorError> {
    let path = supervisor_token_path_for_scope(scope);
    crate::commands::stores::read_secret_json_file(&path, "agent supervisor token")
        .map_err(agent_supervisor_store_error)
}

#[cfg(test)]
fn write_supervisor_token(
    workspace_root: &Path,
    token: &AgentSupervisorTokenFile,
) -> Result<(), AgentSupervisorError> {
    write_supervisor_token_scope(
        &AgentSupervisorScope::project(workspace_root.to_path_buf()),
        token,
    )
}

fn write_supervisor_token_scope(
    scope: &AgentSupervisorScope,
    token: &AgentSupervisorTokenFile,
) -> Result<(), AgentSupervisorError> {
    let path = supervisor_token_path_for_scope(scope);
    crate::commands::stores::write_secret_json_file_atomic(&path, "agent supervisor token", token)
        .map_err(agent_supervisor_store_error)
}

fn agent_supervisor_store_error(
    error: crate::commands::CommandErrorPayload,
) -> AgentSupervisorError {
    AgentSupervisorError::Runtime(error.message)
}

async fn wait_for_supervisor_lock_scope(
    scope: &AgentSupervisorScope,
    token_hash: &str,
    timeout: Duration,
) -> Result<(), AgentSupervisorError> {
    let started = std::time::Instant::now();
    while started.elapsed() <= timeout {
        if let Some(lock) = read_supervisor_lock_scope(scope)? {
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

#[cfg(test)]
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
    fn supervisor_sidecar_args_include_runtime_and_workspace_roots_for_project_scope() {
        let args = supervisor_sidecar_args(Path::new("/tmp/workspace"));
        assert_eq!(
            args,
            vec![
                "--runtime-root".to_owned(),
                "/tmp/workspace/.jyowo/runtime".to_owned(),
                "--workspace-root".to_owned(),
                "/tmp/workspace".to_owned(),
            ]
        );
    }

    #[test]
    fn supervisor_sidecar_args_include_conversation_for_runtime_scope() {
        let conversation_id = SessionId::new();
        let args = supervisor_sidecar_args_for_scope(&AgentSupervisorScope::runtime_conversation(
            "/tmp/jyowo/runtime/global-conversations",
            conversation_id,
        ));
        assert_eq!(
            args,
            vec![
                "--runtime-root".to_owned(),
                "/tmp/jyowo/runtime/global-conversations".to_owned(),
                "--conversation-id".to_owned(),
                conversation_id.to_string(),
            ]
        );
    }

    #[test]
    fn supervisor_output_redaction_removes_secret_patterns() {
        let redactor = DefaultRedactor::default();
        let redacted =
            redact_supervisor_output(&redactor, b"Authorization: Bearer abcdef1234567890abcdef");
        assert!(!redacted.contains("abcdef1234567890abcdef"));
    }

    #[tokio::test]
    async fn background_supervisor_revalidates_current_agent_policy_before_execution() {
        let workspace = tempfile::tempdir().expect("workspace");
        let workspace_root = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");
        let scope = AgentSupervisorScope::project(workspace_root);
        let backend = open_supervisor_backend_for_scope(&scope)
            .await
            .expect("supervisor backend");
        backend
            ._runtime_state
            .execution_settings_store
            .save_record(
                &harness_contracts::ExecutionDefaultsRecord {
                    permission_mode: PermissionMode::Default,
                    tool_profile: ToolProfile::Full,
                    context_compression_trigger_ratio: 0.8,
                    subagents_enabled: false,
                    agent_teams_enabled: false,
                    background_agents_enabled: false,
                },
                Some(&backend._runtime_state.agent_capability_resolution_context()),
            )
            .expect("save current settings");
        let execution = BackgroundSupervisorExecution {
            status: "queued".to_owned(),
            session: None,
            input: ConversationTurnInput::ask("queued background work"),
            model_config_id: "test-model-config".to_owned(),
            permission_mode: PermissionMode::Default,
            agent_tool_policy: AgentToolPolicy {
                subagents: harness_contracts::AgentUsePolicy::Off,
                agent_team: harness_contracts::AgentUsePolicy::Off,
                background_agents: harness_contracts::AgentUsePolicy::Allowed,
                team_config: None,
                workspace_isolation: harness_contracts::AgentWorkspaceIsolationMode::ReadOnly,
                max_depth: 1,
                max_concurrent_subagents: 1,
                max_team_members: 1,
            },
        };

        let error =
            resolve_background_supervisor_agent_tool_policy(&backend, &execution, SessionId::new())
                .expect_err("queued payload must be revalidated against current settings");

        assert!(
            error.to_string().contains("backgroundAgents"),
            "unexpected error: {error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn supervisor_lock_read_rejects_symlink_file() {
        let workspace = tempfile::tempdir().expect("workspace");
        let runtime_dir = workspace.path().join(".jyowo").join("runtime");
        std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
        let external = tempfile::tempdir().expect("external");
        let external_lock = external.path().join("agent-supervisor.lock");
        std::fs::write(
            &external_lock,
            serde_json::to_string(&AgentSupervisorLockFile {
                status: "running".to_owned(),
                workspace_id: workspace_id(workspace.path()),
                token_hash: hash_token("external-token"),
                token_epoch: 1,
                pid: 1,
                control_addr: "127.0.0.1:1".to_owned(),
                started_at: Utc::now(),
                heartbeat_at: Utc::now(),
            })
            .expect("lock json"),
        )
        .expect("external lock");
        std::os::unix::fs::symlink(&external_lock, runtime_dir.join("agent-supervisor.lock"))
            .expect("symlink");

        let error =
            read_supervisor_lock(workspace.path()).expect_err("symlink lock must be rejected");

        assert!(error.to_string().contains("symlink"));
    }

    #[cfg(unix)]
    #[test]
    fn supervisor_lock_write_rejects_symlink_file_without_overwriting_target() {
        let workspace = tempfile::tempdir().expect("workspace");
        let runtime_dir = workspace.path().join(".jyowo").join("runtime");
        std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
        let external = tempfile::tempdir().expect("external");
        let external_lock = external.path().join("agent-supervisor.lock");
        std::fs::write(&external_lock, "sentinel").expect("external lock");
        let lock_path = runtime_dir.join("agent-supervisor.lock");
        std::os::unix::fs::symlink(&external_lock, &lock_path).expect("symlink");

        let error = write_supervisor_lock(
            &lock_path,
            &AgentSupervisorLockFile {
                status: "running".to_owned(),
                workspace_id: workspace_id(workspace.path()),
                token_hash: hash_token("new-token"),
                token_epoch: 1,
                pid: 1,
                control_addr: "127.0.0.1:1".to_owned(),
                started_at: Utc::now(),
                heartbeat_at: Utc::now(),
            },
        )
        .expect_err("symlink lock must be rejected");

        assert!(error.to_string().contains("symlink"));
        assert_eq!(
            std::fs::read_to_string(external_lock).expect("external lock contents"),
            "sentinel"
        );
    }

    #[cfg(unix)]
    #[test]
    fn supervisor_token_read_rejects_symlink_file() {
        let workspace = tempfile::tempdir().expect("workspace");
        let runtime_dir = workspace.path().join(".jyowo").join("runtime");
        std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
        let external = tempfile::tempdir().expect("external");
        let external_token = external.path().join("agent-supervisor.token");
        std::fs::write(
            &external_token,
            serde_json::to_string(&AgentSupervisorTokenFile {
                token: "external-token".to_owned(),
                token_hash: hash_token("external-token"),
                token_epoch: 1,
                workspace_id: workspace_id(workspace.path()),
                created_at: Utc::now(),
            })
            .expect("token json"),
        )
        .expect("external token");
        std::os::unix::fs::symlink(&external_token, runtime_dir.join("agent-supervisor.token"))
            .expect("symlink");

        let error =
            read_supervisor_token(workspace.path()).expect_err("symlink token must be rejected");

        assert!(error.to_string().contains("symlink"));
    }

    #[cfg(unix)]
    #[test]
    fn supervisor_token_write_rejects_symlink_file_without_overwriting_target() {
        let workspace = tempfile::tempdir().expect("workspace");
        let runtime_dir = workspace.path().join(".jyowo").join("runtime");
        std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
        let external = tempfile::tempdir().expect("external");
        let external_token = external.path().join("agent-supervisor.token");
        std::fs::write(&external_token, "sentinel").expect("external token");
        std::os::unix::fs::symlink(&external_token, runtime_dir.join("agent-supervisor.token"))
            .expect("symlink");

        let error = write_supervisor_token(
            workspace.path(),
            &AgentSupervisorTokenFile {
                token: "new-token".to_owned(),
                token_hash: hash_token("new-token"),
                token_epoch: 1,
                workspace_id: workspace_id(workspace.path()),
                created_at: Utc::now(),
            },
        )
        .expect_err("symlink token must be rejected");

        assert!(error.to_string().contains("symlink"));
        assert_eq!(
            std::fs::read_to_string(external_token).expect("external token contents"),
            "sentinel"
        );
    }
}

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    ConfigHash, ContextPatchRequest, ContextPatchSinkCap, DeferredToolsDeltaAttachment, EndReason,
    Event, InteractivityLevel, MessageId, PermissionMode, RunId, SessionCreatedEvent,
    SessionEndedEvent, SessionError, SessionId, SnapshotId, TeamId, TenantId, ToolSearchMode,
    UsageSnapshot, WorkspaceId,
};
use harness_journal::EventStore;
use harness_model::ApiMode;
use harness_skill::SkillRegistration;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{watch, Mutex};

use crate::{
    SessionBuilder, SessionPaths, SessionProjection, SessionTurnRuntime, WorkspaceBootstrap,
};
#[cfg(feature = "steering")]
use crate::{SteeringQueue, SynthesizedUserMessage};

#[derive(Debug, Clone)]
pub struct SessionTurnContext {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub user_id: Option<String>,
    pub team_id: Option<TeamId>,
    pub workspace_root: PathBuf,
    pub snapshot_id: SnapshotId,
    pub config_snapshot_id: SnapshotId,
    pub effective_config_hash: ConfigHash,
    pub started_from_scope_set: Vec<String>,
    pub run_id: RunId,
    pub message_id: MessageId,
    pub turn_index: usize,
    pub permission_mode: PermissionMode,
    pub interactivity: InteractivityLevel,
    pub pending_deferred_tools_delta: Option<DeferredToolsDeltaAttachment>,
    #[cfg(feature = "steering")]
    pub steering_merge: Option<SynthesizedUserMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfigSnapshot {
    pub config_snapshot_id: SnapshotId,
    pub effective_config_hash: ConfigHash,
    pub started_from_scope_set: Vec<String>,
    options_hash: [u8; 32],
}

#[async_trait]
pub trait SessionTurnRunner: Send + Sync + 'static {
    async fn run_turn(
        &self,
        ctx: SessionTurnContext,
        prompt: String,
    ) -> Result<Vec<Event>, SessionError>;

    async fn push_context_patch(&self, request: ContextPatchRequest) -> Result<(), SessionError> {
        let _ = request;
        Err(SessionError::Message(
            "context patch runtime missing".to_owned(),
        ))
    }
}

#[async_trait]
pub trait SkillReloadCap: Send + Sync + 'static {
    async fn reload_skills(&self, registrations: &[SkillRegistration]) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionOptions {
    #[serde(default)]
    pub workspace_ref: Option<WorkspaceId>,
    #[serde(default = "default_workspace_root")]
    pub workspace_root: PathBuf,
    #[serde(default)]
    pub workspace_bootstrap: Option<WorkspaceBootstrap>,
    #[serde(default = "default_tenant_id")]
    pub tenant_id: TenantId,
    #[serde(default)]
    pub session_id: SessionId,
    #[serde(default)]
    pub tool_search: ToolSearchMode,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub api_mode: Option<ApiMode>,
    #[serde(default)]
    pub model_extra: Value,
    #[serde(default = "default_permission_mode")]
    pub permission_mode: PermissionMode,
    #[serde(default = "default_interactivity")]
    pub interactivity: InteractivityLevel,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub team_id: Option<TeamId>,
    #[serde(default)]
    pub system_prompt_addendum: Option<String>,
    #[serde(default)]
    pub max_iterations: u32,
}

impl SessionOptions {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: root.into(),
            workspace_ref: None,
            workspace_bootstrap: None,
            tenant_id: TenantId::SINGLE,
            session_id: SessionId::new(),
            tool_search: ToolSearchMode::default(),
            model_id: None,
            api_mode: None,
            model_extra: Value::Null,
            permission_mode: PermissionMode::Default,
            interactivity: InteractivityLevel::NoInteractive,
            user_id: None,
            team_id: None,
            system_prompt_addendum: None,
            max_iterations: 0,
        }
    }

    #[must_use]
    pub fn with_workspace(mut self, workspace: WorkspaceId) -> Self {
        self.workspace_ref = Some(workspace);
        self
    }

    #[must_use]
    pub fn with_workspace_bootstrap(mut self, root: impl Into<PathBuf>) -> Self {
        self.workspace_bootstrap = Some(WorkspaceBootstrap::new(root));
        self
    }

    #[must_use]
    pub fn with_tenant_id(mut self, tenant_id: TenantId) -> Self {
        self.tenant_id = tenant_id;
        self
    }

    #[must_use]
    pub fn with_session_id(mut self, session_id: SessionId) -> Self {
        self.session_id = session_id;
        self
    }

    #[must_use]
    pub fn with_tool_search_mode(mut self, tool_search: ToolSearchMode) -> Self {
        self.tool_search = tool_search;
        self
    }

    #[must_use]
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = Some(model_id.into());
        self
    }

    #[must_use]
    pub fn with_api_mode(mut self, api_mode: ApiMode) -> Self {
        self.api_mode = Some(api_mode);
        self
    }

    #[must_use]
    pub fn with_model_extra(mut self, model_extra: Value) -> Self {
        self.model_extra = model_extra;
        self
    }

    #[must_use]
    pub fn with_permission_mode(mut self, permission_mode: PermissionMode) -> Self {
        self.permission_mode = permission_mode;
        self
    }

    #[must_use]
    pub fn with_interactivity(mut self, interactivity: InteractivityLevel) -> Self {
        self.interactivity = interactivity;
        self
    }

    #[must_use]
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    #[must_use]
    pub fn with_team_id(mut self, team_id: TeamId) -> Self {
        self.team_id = Some(team_id);
        self
    }

    #[must_use]
    pub fn with_system_prompt_addendum(mut self, addendum: impl Into<String>) -> Self {
        self.system_prompt_addendum = Some(addendum.into());
        self
    }

    #[must_use]
    pub fn with_max_iterations(mut self, max_iterations: u32) -> Self {
        self.max_iterations = max_iterations;
        self
    }
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self::new(default_workspace_root())
    }
}

pub struct Session {
    options: SessionOptions,
    paths: SessionPaths,
    config_snapshot: SessionConfigSnapshot,
    event_store: Arc<dyn EventStore>,
    snapshot_tx: watch::Sender<SnapshotId>,
    snapshot_rx: watch::Receiver<SnapshotId>,
    turn_runtime: Option<SessionTurnRuntime>,
    turn_runner: Option<Arc<dyn SessionTurnRunner>>,
    skill_reload_cap: Option<Arc<dyn SkillReloadCap>>,
    #[cfg(feature = "steering")]
    steering: SteeringQueue,
    state: Mutex<SessionState>,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("options", &self.options)
            .field("paths", &self.paths)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
struct SessionState {
    ended: bool,
    projection: SessionProjection,
}

impl Session {
    pub fn builder() -> SessionBuilder {
        SessionBuilder::default()
    }

    pub(crate) async fn create(
        options: SessionOptions,
        paths: SessionPaths,
        event_store: Arc<dyn EventStore>,
        turn_runtime: Option<SessionTurnRuntime>,
        turn_runner: Option<Arc<dyn SessionTurnRunner>>,
        skill_reload_cap: Option<Arc<dyn SkillReloadCap>>,
        #[cfg(feature = "steering")] steering_policy: harness_contracts::SteeringPolicy,
    ) -> Result<Self, SessionError> {
        let projection = SessionProjection::empty(options.tenant_id, options.session_id);
        let (snapshot_tx, snapshot_rx) = watch::channel(projection.snapshot_id);
        let config_snapshot = SessionConfigSnapshot::from_options(&options);
        let session = Self {
            options,
            paths,
            config_snapshot,
            event_store,
            snapshot_tx,
            snapshot_rx,
            turn_runtime,
            turn_runner,
            skill_reload_cap,
            #[cfg(feature = "steering")]
            steering: SteeringQueue::new(steering_policy),
            state: Mutex::new(SessionState {
                ended: false,
                projection,
            }),
        };
        session.append_created().await?;
        Ok(session)
    }

    pub(crate) async fn from_projection(
        options: SessionOptions,
        paths: SessionPaths,
        event_store: Arc<dyn EventStore>,
        turn_runtime: Option<SessionTurnRuntime>,
        turn_runner: Option<Arc<dyn SessionTurnRunner>>,
        skill_reload_cap: Option<Arc<dyn SkillReloadCap>>,
        projection: SessionProjection,
    ) -> Result<Self, SessionError> {
        let (snapshot_tx, snapshot_rx) = watch::channel(projection.snapshot_id);
        Ok(Self {
            config_snapshot: SessionConfigSnapshot::from_options(&options),
            options,
            paths,
            event_store,
            snapshot_tx,
            snapshot_rx,
            turn_runtime,
            turn_runner,
            skill_reload_cap,
            #[cfg(feature = "steering")]
            steering: SteeringQueue::default(),
            state: Mutex::new(SessionState {
                ended: projection.end_reason.is_some(),
                projection,
            }),
        })
    }

    pub fn paths(&self) -> &SessionPaths {
        &self.paths
    }

    pub(crate) fn options(&self) -> &SessionOptions {
        &self.options
    }

    pub(crate) fn event_store(&self) -> &Arc<dyn EventStore> {
        &self.event_store
    }

    pub(crate) fn turn_runtime(&self) -> Option<SessionTurnRuntime> {
        self.turn_runtime.clone()
    }

    pub(crate) fn turn_runner(&self) -> Option<Arc<dyn SessionTurnRunner>> {
        self.turn_runner.as_ref().map(Arc::clone)
    }

    pub(crate) fn skill_reload_cap(&self) -> Option<Arc<dyn SkillReloadCap>> {
        self.skill_reload_cap.as_ref().map(Arc::clone)
    }

    pub(crate) fn tenant_id(&self) -> TenantId {
        self.options.tenant_id
    }

    pub(crate) fn session_id(&self) -> SessionId {
        self.options.session_id
    }

    pub(crate) fn config_snapshot_id(&self) -> SnapshotId {
        self.config_snapshot.config_snapshot_id
    }

    pub(crate) fn effective_config_hash(&self) -> ConfigHash {
        self.config_snapshot.effective_config_hash
    }

    pub(crate) fn started_from_scope_set(&self) -> Vec<String> {
        self.config_snapshot.started_from_scope_set.clone()
    }

    #[cfg(feature = "steering")]
    pub(crate) fn steering(&self) -> &SteeringQueue {
        &self.steering
    }

    pub async fn run_turn(&self, prompt: impl Into<String>) -> Result<(), SessionError> {
        if self.state.lock().await.ended {
            return Err(SessionError::Message("session already ended".to_owned()));
        }
        if let Some(runner) = self.turn_runner() {
            let (projection, pending_deferred_tools_delta) = self.projection_for_turn().await;
            let run_id = RunId::new();
            let message_id = MessageId::new();
            #[cfg(feature = "steering")]
            let steering_merge = self.drain_and_merge_into(run_id, Some(message_id)).await?;
            let ctx = SessionTurnContext {
                tenant_id: self.options.tenant_id,
                session_id: self.options.session_id,
                workspace_root: self.options.workspace_root.clone(),
                snapshot_id: projection.snapshot_id,
                config_snapshot_id: self.config_snapshot_id(),
                effective_config_hash: self.effective_config_hash(),
                started_from_scope_set: self.started_from_scope_set(),
                run_id,
                message_id,
                turn_index: projection.messages.len(),
                permission_mode: self.options.permission_mode,
                interactivity: self.options.interactivity,
                pending_deferred_tools_delta,
                user_id: self.options.user_id.clone(),
                team_id: self.options.team_id,
                #[cfg(feature = "steering")]
                steering_merge,
            };
            let events = runner.run_turn(ctx, prompt.into()).await?;
            self.apply_projection_events(&events).await;
            return Ok(());
        }
        let runtime = self
            .turn_runtime()
            .ok_or_else(|| SessionError::Message("turn runtime missing".to_owned()))?;
        let (_, pending_deferred_tools_delta) = self.projection_for_turn().await;
        if let Some(delta) = pending_deferred_tools_delta {
            runtime
                .context
                .push_deferred_tools_delta(self.tenant_id(), self.session_id(), delta)
                .map_err(session_error)?;
        }
        crate::turn::run_turn(self, runtime, prompt.into()).await
    }

    pub async fn interrupt(&self) -> Result<(), SessionError> {
        if self.state.lock().await.ended {
            return Err(SessionError::Message("session already ended".to_owned()));
        }
        Ok(())
    }

    pub async fn push_context_patch(
        &self,
        request: ContextPatchRequest,
    ) -> Result<(), SessionError> {
        if request.tenant_id != self.tenant_id() {
            return Err(SessionError::Message(
                "context patch tenant mismatch".to_owned(),
            ));
        }
        if request.session_id != self.session_id() {
            return Err(SessionError::Message(
                "context patch session mismatch".to_owned(),
            ));
        }
        if let Some(runner) = self.turn_runner() {
            return runner.push_context_patch(request).await;
        }
        let runtime = self
            .turn_runtime()
            .ok_or_else(|| SessionError::Message("context patch runtime missing".to_owned()))?;
        runtime
            .context
            .push_patch(request)
            .await
            .map_err(session_error)?;
        Ok(())
    }

    pub async fn end(&self, reason: EndReason) -> Result<(), SessionError> {
        let snapshot_id;
        {
            let mut state = self.state.lock().await;
            if state.ended {
                return Ok(());
            }
            state.ended = true;
            state.projection.end_reason = Some(reason.clone());
            state.projection.refresh_snapshot_id();
            snapshot_id = state.projection.snapshot_id;
        }
        self.snapshot_tx.send_replace(snapshot_id);
        #[cfg(feature = "steering")]
        self.drop_steering_for_session_end().await?;

        self.event_store
            .append(
                self.options.tenant_id,
                self.options.session_id,
                &[Event::SessionEnded(SessionEndedEvent {
                    session_id: self.options.session_id,
                    tenant_id: self.options.tenant_id,
                    reason,
                    final_usage: UsageSnapshot::default(),
                    at: harness_contracts::now(),
                })],
            )
            .await
            .map_err(session_error)?;
        Ok(())
    }

    pub async fn projection(&self) -> SessionProjection {
        self.state.lock().await.projection.clone()
    }

    async fn projection_for_turn(
        &self,
    ) -> (SessionProjection, Option<DeferredToolsDeltaAttachment>) {
        let (projection, pending_delta, snapshot_id) = {
            let mut state = self.state.lock().await;
            let pending_delta = state.projection.take_pending_deferred_tools_delta();
            let snapshot_id = state.projection.snapshot_id;
            (state.projection.clone(), pending_delta, snapshot_id)
        };
        if pending_delta.is_some() {
            self.snapshot_tx.send_replace(snapshot_id);
        }
        (projection, pending_delta)
    }

    pub fn snapshot_id(&self) -> SnapshotId {
        *self.snapshot_rx.borrow()
    }

    async fn append_created(&self) -> Result<(), SessionError> {
        let snapshot_id = self.state.lock().await.projection.snapshot_id;
        self.event_store
            .append(
                self.options.tenant_id,
                self.options.session_id,
                &[Event::SessionCreated(SessionCreatedEvent {
                    session_id: self.options.session_id,
                    tenant_id: self.options.tenant_id,
                    options_hash: self.config_snapshot.options_hash,
                    snapshot_id,
                    effective_config_hash: self.config_snapshot.effective_config_hash,
                    created_at: harness_contracts::now(),
                })],
            )
            .await
            .map_err(session_error)?;
        Ok(())
    }

    pub(crate) async fn append_events(&self, events: &[Event]) -> Result<(), SessionError> {
        self.event_store
            .append(self.options.tenant_id, self.options.session_id, events)
            .await
            .map_err(session_error)?;
        Ok(())
    }

    pub(crate) async fn apply_projection_events(&self, events: &[Event]) {
        let snapshot_id = {
            let mut state = self.state.lock().await;
            state.projection.apply_events(events);
            state.projection.snapshot_id
        };
        self.snapshot_tx.send_replace(snapshot_id);
    }
}

impl SessionConfigSnapshot {
    fn from_options(options: &SessionOptions) -> Self {
        let options_hash = session_options_hash(options);
        let effective_config_hash = effective_config_hash(options_hash);
        Self {
            config_snapshot_id: config_snapshot_id(effective_config_hash),
            effective_config_hash,
            started_from_scope_set: vec!["sdk:session_options".to_owned()],
            options_hash,
        }
    }
}

pub(crate) fn session_error(error: impl std::fmt::Display) -> SessionError {
    SessionError::Message(error.to_string())
}

fn session_options_hash(options: &SessionOptions) -> [u8; 32] {
    hash_json(&json!({
        "workspace_ref": options.workspace_ref,
        "workspace_root": options.workspace_root,
        "workspace_bootstrap": options.workspace_bootstrap,
        "tenant_id": options.tenant_id,
        "session_id": options.session_id,
        "tool_search": options.tool_search,
        "model_id": options.model_id,
        "api_mode": options.api_mode.map(api_mode_name),
        "model_extra": options.model_extra,
        "permission_mode": options.permission_mode,
        "interactivity": options.interactivity,
        "user_id": options.user_id,
        "team_id": options.team_id,
        "system_prompt_addendum": options.system_prompt_addendum,
        "max_iterations": options.max_iterations,
    }))
}

fn effective_config_hash(options_hash: [u8; 32]) -> ConfigHash {
    ConfigHash(hash_json(&json!({
        "kind": "sdk_session_effective_config",
        "options_hash": options_hash,
        "source_refs": [],
        "started_from_scope_set": ["sdk:session_options"],
    })))
}

fn config_snapshot_id(hash: ConfigHash) -> SnapshotId {
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&hash.0[..16]);
    SnapshotId::from_u128(u128::from_be_bytes(bytes))
}

fn hash_json(value: &Value) -> [u8; 32] {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    blake3::hash(&bytes).into()
}

fn api_mode_name(mode: ApiMode) -> &'static str {
    match mode {
        ApiMode::ChatCompletions => "chat_completions",
        ApiMode::Responses => "responses",
        ApiMode::Messages => "messages",
        ApiMode::GenerateContent => "generate_content",
    }
}

fn default_workspace_root() -> PathBuf {
    PathBuf::from(".")
}

fn default_tenant_id() -> TenantId {
    TenantId::SINGLE
}

fn default_permission_mode() -> PermissionMode {
    PermissionMode::Default
}

fn default_interactivity() -> InteractivityLevel {
    InteractivityLevel::NoInteractive
}

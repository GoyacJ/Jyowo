use std::sync::Arc;

#[cfg(feature = "steering")]
use harness_contracts::SteeringPolicy;
use harness_contracts::{ConfigHash, RunModelSnapshot, SessionError};
use harness_journal::EventStore;

use crate::{
    Session, SessionOptions, SessionPaths, SessionProjection, SessionTurnRunner, SkillReloadCap,
};

#[derive(Default)]
pub struct SessionBuilder {
    options: Option<SessionOptions>,
    event_store: Option<Arc<dyn EventStore>>,
    turn_runner: Option<Arc<dyn SessionTurnRunner>>,
    turn_model_snapshot: Option<RunModelSnapshot>,
    turn_model_config_id: Option<String>,
    skill_reload_cap: Option<Arc<dyn SkillReloadCap>>,
    projection: Option<SessionProjection>,
    effective_prompt_inputs_hash: Option<[u8; 32]>,
    runtime_prompt_context_hash: Option<[u8; 32]>,
    effective_config_hash: Option<ConfigHash>,
    #[cfg(feature = "steering")]
    steering_policy: Option<SteeringPolicy>,
}

impl SessionBuilder {
    #[must_use]
    pub fn with_options(mut self, options: SessionOptions) -> Self {
        self.options = Some(options);
        self
    }

    #[must_use]
    pub fn with_event_store(mut self, event_store: Arc<dyn EventStore>) -> Self {
        self.event_store = Some(event_store);
        self
    }

    #[must_use]
    pub fn with_turn_runner(mut self, turn_runner: Arc<dyn SessionTurnRunner>) -> Self {
        self.turn_runner = Some(turn_runner);
        self
    }

    #[must_use]
    pub fn with_turn_model_snapshot(mut self, snapshot: RunModelSnapshot) -> Self {
        self.turn_model_config_id = snapshot.model_config_id.clone();
        self.turn_model_snapshot = Some(snapshot);
        self
    }

    #[must_use]
    pub fn with_skill_reload_cap(mut self, skill_reload_cap: Arc<dyn SkillReloadCap>) -> Self {
        self.skill_reload_cap = Some(skill_reload_cap);
        self
    }

    #[must_use]
    pub fn with_projection(mut self, projection: SessionProjection) -> Self {
        self.projection = Some(projection);
        self
    }

    #[must_use]
    pub fn with_effective_prompt_inputs_hash(mut self, hash: [u8; 32]) -> Self {
        self.effective_prompt_inputs_hash = Some(hash);
        self
    }

    #[must_use]
    pub fn with_runtime_prompt_context_hash(mut self, hash: [u8; 32]) -> Self {
        self.runtime_prompt_context_hash = Some(hash);
        self
    }

    #[must_use]
    pub fn with_effective_config_hash(mut self, hash: ConfigHash) -> Self {
        self.effective_config_hash = Some(hash);
        self
    }

    #[cfg(feature = "steering")]
    #[must_use]
    pub fn with_steering_policy(mut self, policy: SteeringPolicy) -> Self {
        self.steering_policy = Some(policy);
        self
    }

    pub async fn build(self) -> Result<Session, SessionError> {
        let mut options = self
            .options
            .ok_or_else(|| SessionError::Message("session options missing".to_owned()))?;
        let event_store = self
            .event_store
            .ok_or_else(|| SessionError::Message("event store missing".to_owned()))?;

        options.workspace_root = options
            .workspace_root
            .canonicalize()
            .map_err(|error| SessionError::Message(format!("workspace_root invalid: {error}")))?;
        if let Some(project_workspace_root) = options.project_workspace_root.take() {
            options.project_workspace_root =
                Some(project_workspace_root.canonicalize().map_err(|error| {
                    SessionError::Message(format!("project_workspace_root invalid: {error}"))
                })?);
        }
        let paths = SessionPaths::from_workspace(
            &options.workspace_root,
            options.tenant_id,
            options.session_id,
        );
        if let Some(projection) = self.projection {
            if projection.tenant_id != options.tenant_id {
                return Err(SessionError::Message(format!(
                    "projection tenant_id {:?} does not match session options {:?}",
                    projection.tenant_id, options.tenant_id
                )));
            }
            if projection.session_id != options.session_id {
                return Err(SessionError::Message(format!(
                    "projection session_id {:?} does not match session options {:?}",
                    projection.session_id, options.session_id
                )));
            }
            return Session::from_projection(
                options,
                paths,
                event_store,
                self.turn_runner,
                self.turn_model_snapshot,
                self.turn_model_config_id,
                self.skill_reload_cap,
                self.effective_prompt_inputs_hash,
                self.runtime_prompt_context_hash,
                self.effective_config_hash,
                projection,
            )
            .await;
        }

        #[cfg(feature = "steering")]
        {
            Session::create(
                options,
                paths,
                event_store,
                self.turn_runner,
                self.turn_model_snapshot,
                self.turn_model_config_id,
                self.skill_reload_cap,
                self.effective_prompt_inputs_hash,
                self.runtime_prompt_context_hash,
                self.effective_config_hash,
                self.steering_policy.unwrap_or_default(),
            )
            .await
        }
        #[cfg(not(feature = "steering"))]
        {
            Session::create(
                options,
                paths,
                event_store,
                self.turn_runner,
                self.turn_model_snapshot,
                self.turn_model_config_id,
                self.skill_reload_cap,
                self.effective_prompt_inputs_hash,
                self.runtime_prompt_context_hash,
                self.effective_config_hash,
            )
            .await
        }
    }
}

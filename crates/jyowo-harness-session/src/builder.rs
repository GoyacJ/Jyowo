use std::sync::Arc;

use harness_contracts::SessionError;
#[cfg(feature = "steering")]
use harness_contracts::SteeringPolicy;
use harness_journal::EventStore;

use crate::{
    Session, SessionOptions, SessionPaths, SessionProjection, SessionTurnRunner,
    SessionTurnRuntime, SkillReloadCap,
};

#[derive(Default)]
pub struct SessionBuilder {
    options: Option<SessionOptions>,
    event_store: Option<Arc<dyn EventStore>>,
    turn_runtime: Option<SessionTurnRuntime>,
    turn_runner: Option<Arc<dyn SessionTurnRunner>>,
    skill_reload_cap: Option<Arc<dyn SkillReloadCap>>,
    projection: Option<SessionProjection>,
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
    pub fn with_turn_runtime(mut self, turn_runtime: SessionTurnRuntime) -> Self {
        self.turn_runtime = Some(turn_runtime);
        self
    }

    #[must_use]
    pub fn with_turn_runner(mut self, turn_runner: Arc<dyn SessionTurnRunner>) -> Self {
        self.turn_runner = Some(turn_runner);
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
                self.turn_runtime,
                self.turn_runner,
                self.skill_reload_cap,
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
                self.turn_runtime,
                self.turn_runner,
                self.skill_reload_cap,
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
                self.turn_runtime,
                self.turn_runner,
                self.skill_reload_cap,
            )
            .await
        }
    }
}

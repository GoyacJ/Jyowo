use super::*;

#[derive(Clone)]
pub(super) struct ActiveConversationRun {
    pub(super) tenant_id: TenantId,
    pub(super) session_id: SessionId,
    pub(super) cancellation: CancellationToken,
}

pub(super) struct EngineSessionTurnRunner {
    pub(super) engine: Engine,
    pub(super) controlled_run: Option<(RunId, RunControlHandle)>,
    pub(super) active_conversation_runs:
        Arc<parking_lot::Mutex<HashMap<RunId, ActiveConversationRun>>>,
    pub(super) active_conversation_sessions:
        Arc<parking_lot::Mutex<HashMap<(TenantId, SessionId), RunId>>>,
    pub(super) process_registry: Option<Arc<dyn RunScopedProcessRegistryCap>>,
    pub(super) skill_registry: Option<SkillRegistry>,
    pub(super) skill_registry_snapshot: Option<Arc<SkillRegistrySnapshot>>,
    pub(super) skill_metrics_sink: Option<Arc<dyn SkillMetricsSink>>,
    pub(super) skill_render_policy: SkillRenderPolicy,
    pub(super) skill_config_snapshot: SkillConfigSnapshot,
    pub(super) pending_skill_context_deliveries:
        parking_lot::Mutex<HashMap<(TenantId, SessionId, RunId), Vec<String>>>,
}

pub(super) struct ActiveConversationRunGuard {
    active_conversation_runs: Arc<parking_lot::Mutex<HashMap<RunId, ActiveConversationRun>>>,
    active_conversation_sessions: Arc<parking_lot::Mutex<HashMap<(TenantId, SessionId), RunId>>>,
    process_registry: Option<Arc<dyn RunScopedProcessRegistryCap>>,
    tenant_id: TenantId,
    session_id: SessionId,
    run_id: RunId,
}

pub(super) struct ActiveConversationSessionGuard {
    active_conversation_sessions: Arc<parking_lot::Mutex<HashMap<(TenantId, SessionId), RunId>>>,
    tenant_id: TenantId,
    session_id: SessionId,
    run_id: RunId,
}

impl ActiveConversationRunGuard {
    pub(super) fn register(
        active_conversation_runs: Arc<parking_lot::Mutex<HashMap<RunId, ActiveConversationRun>>>,
        active_conversation_sessions: Arc<
            parking_lot::Mutex<HashMap<(TenantId, SessionId), RunId>>,
        >,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
        cancellation: CancellationToken,
        process_registry: Option<Arc<dyn RunScopedProcessRegistryCap>>,
    ) -> Result<Self, SessionError> {
        let session_key = (tenant_id, session_id);
        let mut active_sessions = active_conversation_sessions.lock();
        if active_conversation_runs.lock().contains_key(&run_id) {
            return Err(SessionError::Message(
                "conversation run already active".to_owned(),
            ));
        }
        match active_sessions.get(&session_key).copied() {
            Some(active_run_id) if active_run_id != run_id => {
                return Err(SessionError::Message(
                    "conversation run already active for session".to_owned(),
                ));
            }
            Some(_) => {}
            None => {
                active_sessions.insert(session_key, run_id);
            }
        }

        active_conversation_runs.lock().insert(
            run_id,
            ActiveConversationRun {
                tenant_id,
                session_id,
                cancellation,
            },
        );
        drop(active_sessions);

        Ok(Self {
            active_conversation_runs,
            active_conversation_sessions,
            process_registry,
            tenant_id,
            session_id,
            run_id,
        })
    }
}

impl ActiveConversationSessionGuard {
    pub(super) fn register(
        active_conversation_sessions: Arc<
            parking_lot::Mutex<HashMap<(TenantId, SessionId), RunId>>,
        >,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
    ) -> Result<Self, SessionError> {
        let session_key = (tenant_id, session_id);
        let mut active_sessions = active_conversation_sessions.lock();
        if active_sessions.contains_key(&session_key) {
            return Err(SessionError::Message(
                "conversation run already active for session".to_owned(),
            ));
        }
        active_sessions.insert(session_key, run_id);
        drop(active_sessions);

        Ok(Self {
            active_conversation_sessions,
            tenant_id,
            session_id,
            run_id,
        })
    }
}

impl Drop for ActiveConversationSessionGuard {
    fn drop(&mut self) {
        let session_key = (self.tenant_id, self.session_id);
        let mut active_sessions = self.active_conversation_sessions.lock();
        if active_sessions.get(&session_key) == Some(&self.run_id) {
            active_sessions.remove(&session_key);
        }
    }
}

impl Drop for ActiveConversationRunGuard {
    fn drop(&mut self) {
        let session_key = (self.tenant_id, self.session_id);
        let mut active_sessions = self.active_conversation_sessions.lock();
        if active_sessions.get(&session_key) == Some(&self.run_id) {
            active_sessions.remove(&session_key);
        }
        self.active_conversation_runs.lock().remove(&self.run_id);
        if let Some(registry) = self.process_registry.clone() {
            let tenant_id = self.tenant_id;
            let session_id = self.session_id;
            let run_id = self.run_id;
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let _ = registry.cleanup_run(tenant_id, session_id, run_id).await;
                });
            }
        }
    }
}

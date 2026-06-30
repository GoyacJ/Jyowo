use super::*;

#[derive(Clone)]
pub(super) struct ActiveConversationRun {
    pub(super) tenant_id: TenantId,
    pub(super) session_id: SessionId,
    pub(super) cancellation: CancellationToken,
}

pub(super) struct EngineSessionTurnRunner {
    pub(super) engine: Engine,
    pub(super) active_conversation_runs:
        Arc<parking_lot::Mutex<HashMap<RunId, ActiveConversationRun>>>,
    pub(super) process_registry: Option<Arc<dyn RunScopedProcessRegistryCap>>,
    pub(super) skill_registry: Option<SkillRegistry>,
    pub(super) skill_metrics_sink: Option<Arc<dyn SkillMetricsSink>>,
    pub(super) skill_config_snapshot: SkillConfigSnapshot,
}

pub(super) struct ActiveConversationRunGuard {
    active_conversation_runs: Arc<parking_lot::Mutex<HashMap<RunId, ActiveConversationRun>>>,
    process_registry: Option<Arc<dyn RunScopedProcessRegistryCap>>,
    tenant_id: TenantId,
    session_id: SessionId,
    run_id: RunId,
}

impl ActiveConversationRunGuard {
    pub(super) fn register(
        active_conversation_runs: Arc<parking_lot::Mutex<HashMap<RunId, ActiveConversationRun>>>,
        tenant_id: TenantId,
        session_id: SessionId,
        run_id: RunId,
        cancellation: CancellationToken,
        process_registry: Option<Arc<dyn RunScopedProcessRegistryCap>>,
    ) -> Self {
        active_conversation_runs.lock().insert(
            run_id,
            ActiveConversationRun {
                tenant_id,
                session_id,
                cancellation,
            },
        );
        Self {
            active_conversation_runs,
            process_registry,
            tenant_id,
            session_id,
            run_id,
        }
    }
}

impl Drop for ActiveConversationRunGuard {
    fn drop(&mut self) {
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

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use harness_contracts::{Decision, ExecFingerprint, PermissionError, PermissionSubject, Severity};
use parking_lot::Mutex;

use crate::{
    canonical_permission_fingerprint, hard_policy_denies_from_context, PermissionBroker,
    PermissionContext, PermissionRequest, PersistedDecision,
};

const DEFAULT_DEDUP_WINDOW: Duration = Duration::from_secs(30);

pub struct DedupGate {
    inner: Arc<dyn PermissionBroker>,
    cache: Mutex<HashMap<ExecFingerprint, CachedDecision>>,
    config: DedupGateConfig,
}

#[derive(Debug, Clone)]
pub struct DedupGateConfig {
    pub window: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DedupHit {
    pub fingerprint: ExecFingerprint,
    pub original_request_id: harness_contracts::RequestId,
    pub decision: Decision,
    pub decided_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct CachedDecision {
    original_request_id: harness_contracts::RequestId,
    decision: Decision,
    decided_at: DateTime<Utc>,
}

impl DedupGate {
    #[must_use]
    pub fn new(inner: Arc<dyn PermissionBroker>) -> Self {
        Self::with_config(inner, DedupGateConfig::default())
    }

    #[must_use]
    pub fn with_config(inner: Arc<dyn PermissionBroker>, config: DedupGateConfig) -> Self {
        Self {
            inner,
            cache: Mutex::new(HashMap::new()),
            config,
        }
    }

    #[must_use]
    pub fn lookup(&self, request: &PermissionRequest) -> Option<DedupHit> {
        let fingerprint = canonical_permission_fingerprint(request);
        let cached = self.cache.lock().get(&fingerprint).cloned()?;
        if !self.is_fresh(cached.decided_at) || !can_reuse_decision(request, &cached.decision) {
            return None;
        }

        Some(DedupHit {
            fingerprint,
            original_request_id: cached.original_request_id,
            decision: cached.decision,
            decided_at: cached.decided_at,
        })
    }

    fn is_fresh(&self, decided_at: DateTime<Utc>) -> bool {
        let Ok(age) = Utc::now().signed_duration_since(decided_at).to_std() else {
            return false;
        };
        age <= self.config.window
    }
}

impl Default for DedupGateConfig {
    fn default() -> Self {
        Self {
            window: DEFAULT_DEDUP_WINDOW,
        }
    }
}

#[async_trait]
impl PermissionBroker for DedupGate {
    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        if self.hard_policy_denies(&request, &ctx).await {
            return Decision::DenyOnce;
        }

        if let Some(hit) = self.lookup(&request) {
            return hit.decision;
        }

        let fingerprint = canonical_permission_fingerprint(&request);
        let original_request_id = request.request_id;
        let decision = self.inner.decide(request.clone(), ctx).await;

        if should_cache_decision(&request, &decision) {
            self.cache.lock().insert(
                fingerprint,
                CachedDecision {
                    original_request_id,
                    decision: decision.clone(),
                    decided_at: Utc::now(),
                },
            );
        }

        decision
    }

    async fn hard_policy_denies(
        &self,
        request: &PermissionRequest,
        ctx: &PermissionContext,
    ) -> bool {
        self.inner.hard_policy_denies(request, ctx).await
            || hard_policy_denies_from_context(request, ctx)
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.inner.persist(decision).await
    }
}

fn should_cache_decision(request: &PermissionRequest, decision: &Decision) -> bool {
    can_reuse_decision(request, decision)
}

fn can_reuse_decision(request: &PermissionRequest, decision: &Decision) -> bool {
    if is_dangerous_or_high_risk(request) {
        return is_deny(decision);
    }

    matches!(
        decision,
        Decision::AllowOnce
            | Decision::AllowSession
            | Decision::AllowPermanent
            | Decision::DenyOnce
            | Decision::DenyPermanent
    )
}

fn is_deny(decision: &Decision) -> bool {
    matches!(decision, Decision::DenyOnce | Decision::DenyPermanent)
}

fn is_dangerous_or_high_risk(request: &PermissionRequest) -> bool {
    matches!(
        &request.subject,
        PermissionSubject::DangerousCommand { .. }
            | PermissionSubject::CommandExec { .. }
            | PermissionSubject::FileDelete { .. }
            | PermissionSubject::NetworkAccess { .. }
    ) || matches!(request.severity, Severity::High | Severity::Critical)
}

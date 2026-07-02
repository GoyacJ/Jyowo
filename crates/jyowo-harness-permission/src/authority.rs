use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use harness_contracts::{
    Decision, DecisionId, DecisionScope, ExecFingerprint, InteractivityLevel, PermissionError,
    PermissionMode, PermissionSubject, RequestId, RuleSource, SessionId, TenantId,
};

use crate::{
    canonical_permission_fingerprint, DecisionPersistence, DedupGate, NoopDecisionPersistence,
    PermissionBroker, PermissionContext, PermissionRequest, PersistedDecision,
};

pub struct PermissionAuthority {
    policy_broker: Arc<dyn PermissionBroker>,
    interactive_broker: Option<Arc<dyn PermissionBroker>>,
    dedup: DedupGate,
    decision_store: Arc<dyn DecisionStore>,
    durable_decision_store: bool,
}

#[derive(Default)]
pub struct PermissionAuthorityBuilder {
    policy_broker: Option<Arc<dyn PermissionBroker>>,
    interactive_broker: Option<Arc<dyn PermissionBroker>>,
    dedup_broker: Option<Arc<dyn PermissionBroker>>,
    decision_store: Option<Arc<dyn DecisionStore>>,
    durable_decision_store: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionAuthorityOutcome {
    pub decision: Decision,
    pub decided_by: PermissionAuthorityDecisionSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionAuthorityDecisionSource {
    ScopeMismatch,
    HardPolicy,
    Rule,
    PersistedDecision { decision_id: DecisionId },
    Dedup { original_request_id: RequestId },
    PermissionMode,
    NoInteractive,
    Interactive,
    PersistenceFailed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionLookup {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub requested_scope: DecisionScope,
    pub subject: PermissionSubject,
    pub fingerprint: ExecFingerprint,
    pub decision_source: RuleSource,
    pub permission_mode: PermissionMode,
    pub looked_up_at: DateTime<Utc>,
}

#[async_trait]
pub trait DecisionHistory: Send + Sync + 'static {
    async fn find_scoped_decision(
        &self,
        lookup: DecisionLookup,
    ) -> Result<Option<PersistedDecision>, PermissionError>;
}

pub trait DecisionStore: DecisionPersistence + DecisionHistory {}

impl<T> DecisionStore for T where T: DecisionPersistence + DecisionHistory {}

#[async_trait]
impl DecisionHistory for NoopDecisionPersistence {
    async fn find_scoped_decision(
        &self,
        _lookup: DecisionLookup,
    ) -> Result<Option<PersistedDecision>, PermissionError> {
        Ok(None)
    }
}

impl PermissionAuthority {
    #[must_use]
    pub fn builder() -> PermissionAuthorityBuilder {
        PermissionAuthorityBuilder::default()
    }

    pub async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        self.decide_with_audit(request, ctx).await.decision
    }

    pub async fn decide_with_audit(
        &self,
        request: PermissionRequest,
        ctx: PermissionContext,
    ) -> PermissionAuthorityOutcome {
        if request.tenant_id != ctx.tenant_id || request.session_id != ctx.session_id {
            return outcome(
                Decision::DenyOnce,
                PermissionAuthorityDecisionSource::ScopeMismatch,
            );
        }

        if self.policy_broker.hard_policy_denies(&request, &ctx).await {
            return outcome(
                Decision::DenyOnce,
                PermissionAuthorityDecisionSource::HardPolicy,
            );
        }

        let rule_decision = self
            .policy_broker
            .decide(request.clone(), ctx.clone())
            .await;
        if !matches!(rule_decision, Decision::Escalate) {
            return outcome(rule_decision, PermissionAuthorityDecisionSource::Rule);
        }

        if self.durable_decision_store {
            match self
                .decision_store
                .find_scoped_decision(DecisionLookup {
                    tenant_id: request.tenant_id,
                    session_id: request.session_id,
                    requested_scope: request.scope_hint.clone(),
                    subject: request.subject.clone(),
                    fingerprint: canonical_permission_fingerprint(&request),
                    decision_source: RuleSource::User,
                    permission_mode: ctx.permission_mode,
                    looked_up_at: Utc::now(),
                })
                .await
            {
                Ok(Some(decision)) => {
                    return outcome(
                        decision_from_persisted(&decision),
                        PermissionAuthorityDecisionSource::PersistedDecision {
                            decision_id: decision.decision_id,
                        },
                    )
                }
                Ok(None) => {}
                Err(_error) => {
                    return outcome(
                        Decision::DenyOnce,
                        PermissionAuthorityDecisionSource::PersistenceFailed,
                    );
                }
            }
        }

        if let Some(hit) = self.dedup.lookup(&request) {
            return outcome(
                hit.decision,
                PermissionAuthorityDecisionSource::Dedup {
                    original_request_id: hit.original_request_id,
                },
            );
        }

        if matches!(
            ctx.permission_mode,
            PermissionMode::BypassPermissions | PermissionMode::DontAsk
        ) {
            return outcome(
                Decision::AllowOnce,
                PermissionAuthorityDecisionSource::PermissionMode,
            );
        }

        if matches!(ctx.interactivity, InteractivityLevel::NoInteractive) {
            return outcome(
                Decision::DenyOnce,
                PermissionAuthorityDecisionSource::NoInteractive,
            );
        }

        let Some(interactive_broker) = &self.interactive_broker else {
            return outcome(
                Decision::DenyOnce,
                PermissionAuthorityDecisionSource::NoInteractive,
            );
        };

        let decision = interactive_broker.decide(request.clone(), ctx).await;
        if self.durable_decision_store && is_durable_decision(&decision) {
            let fingerprint = canonical_permission_fingerprint(&request);
            let persisted = PersistedDecision {
                decision_id: DecisionId::new(),
                decision: decision.clone(),
                scope: request.scope_hint.clone(),
                source: RuleSource::User,
                session_id: persisted_session_id(&request, &decision),
                fingerprint: Some(fingerprint),
            };
            if self.decision_store.persist(persisted).await.is_err() {
                return outcome(
                    Decision::DenyOnce,
                    PermissionAuthorityDecisionSource::PersistenceFailed,
                );
            }
        }
        self.dedup.remember(&request, &decision);

        outcome(decision, PermissionAuthorityDecisionSource::Interactive)
    }
}

impl PermissionAuthorityBuilder {
    #[must_use]
    pub fn with_policy_broker(mut self, broker: Arc<dyn PermissionBroker>) -> Self {
        self.policy_broker = Some(broker);
        self
    }

    #[must_use]
    pub fn with_interactive_broker(mut self, broker: Arc<dyn PermissionBroker>) -> Self {
        self.interactive_broker = Some(broker);
        self
    }

    #[must_use]
    pub fn with_dedup_broker(mut self, broker: Arc<dyn PermissionBroker>) -> Self {
        self.dedup_broker = Some(broker);
        self
    }

    #[must_use]
    pub fn with_decision_store(mut self, store: Arc<dyn DecisionStore>) -> Self {
        self.decision_store = Some(store);
        self.durable_decision_store = true;
        self
    }

    #[must_use]
    pub fn with_transient_decision_store(mut self, store: Arc<dyn DecisionStore>) -> Self {
        self.decision_store = Some(store);
        self.durable_decision_store = false;
        self
    }

    pub fn build(self) -> Result<PermissionAuthority, PermissionError> {
        let Some(policy_broker) = self.policy_broker else {
            return Err(PermissionError::Message(
                "permission authority requires a policy broker".to_owned(),
            ));
        };
        if !policy_broker.can_anchor_authority() {
            return Err(PermissionError::Message(
                "permission authority policy broker must own hard policy".to_owned(),
            ));
        }
        let Some(decision_store) = self.decision_store else {
            return Err(PermissionError::Message(
                "permission authority requires a decision store".to_owned(),
            ));
        };
        if self.durable_decision_store && !decision_store.supports_integrity() {
            return Err(PermissionError::Message(
                "permission authority decision store must support integrity verification"
                    .to_owned(),
            ));
        }

        let dedup_inner = self
            .dedup_broker
            .clone()
            .or_else(|| self.interactive_broker.clone())
            .unwrap_or_else(|| policy_broker.clone());

        Ok(PermissionAuthority {
            policy_broker,
            interactive_broker: self.interactive_broker,
            dedup: DedupGate::new(dedup_inner),
            decision_store,
            durable_decision_store: self.durable_decision_store,
        })
    }
}

fn is_durable_decision(decision: &Decision) -> bool {
    matches!(
        decision,
        Decision::AllowSession | Decision::AllowPermanent | Decision::DenyPermanent
    )
}

fn persisted_session_id(request: &PermissionRequest, decision: &Decision) -> Option<SessionId> {
    matches!(decision, Decision::AllowSession).then_some(request.session_id)
}

fn decision_from_persisted(decision: &PersistedDecision) -> Decision {
    match decision.decision {
        Decision::AllowSession | Decision::AllowPermanent => Decision::AllowOnce,
        Decision::DenyPermanent => Decision::DenyPermanent,
        Decision::DenyOnce => Decision::DenyOnce,
        Decision::AllowOnce => Decision::AllowOnce,
        Decision::Escalate => Decision::DenyOnce,
        _ => Decision::DenyOnce,
    }
}

fn outcome(
    decision: Decision,
    decided_by: PermissionAuthorityDecisionSource,
) -> PermissionAuthorityOutcome {
    PermissionAuthorityOutcome {
        decision,
        decided_by,
    }
}

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use harness_contracts::{
    DecidedBy, Decision, DecisionScope, FallbackPolicy, InteractivityLevel, PermissionError,
    PermissionMode, PermissionSubject,
};

use crate::{
    DecisionPersistence, NoopDecisionPersistence, PermissionBroker, PermissionContext,
    PermissionRequest, PersistedDecision,
};

pub struct ChainedBroker {
    chain: Vec<Arc<dyn PermissionBroker>>,
    terminator: Arc<dyn PermissionTerminator>,
    persistence: Arc<dyn DecisionPersistence>,
}

#[async_trait]
pub trait PermissionTerminator: Send + Sync + 'static {
    async fn terminate(&self, request: &PermissionRequest, ctx: &PermissionContext) -> Decision;
}

#[derive(Clone)]
pub struct FallbackTerminator {
    policy: FallbackPolicy,
    history: Arc<dyn DecisionHistoryQuery>,
}

#[async_trait]
pub trait DecisionHistoryQuery: Send + Sync + 'static {
    async fn find_closest(&self, scope: &DecisionScope) -> Option<PriorDecision>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct PriorDecision {
    pub scope: DecisionScope,
    pub decision: Decision,
    pub decided_at: DateTime<Utc>,
    pub decided_by: DecidedBy,
}

#[derive(Debug, Default)]
pub struct EmptyDecisionHistory;

#[async_trait]
impl DecisionHistoryQuery for EmptyDecisionHistory {
    async fn find_closest(&self, _scope: &DecisionScope) -> Option<PriorDecision> {
        None
    }
}

#[derive(Default)]
pub struct ChainedBrokerBuilder {
    chain: Vec<Arc<dyn PermissionBroker>>,
    terminator: Option<Arc<dyn PermissionTerminator>>,
    persistence: Option<Arc<dyn DecisionPersistence>>,
}

impl ChainedBroker {
    #[must_use]
    pub fn builder() -> ChainedBrokerBuilder {
        ChainedBrokerBuilder::default()
    }
}

impl ChainedBrokerBuilder {
    #[must_use]
    pub fn push(mut self, broker: Arc<dyn PermissionBroker>) -> Self {
        self.chain.push(broker);
        self
    }

    #[must_use]
    pub fn terminator(mut self, terminator: Arc<dyn PermissionTerminator>) -> Self {
        self.terminator = Some(terminator);
        self
    }

    #[must_use]
    pub fn with_persistence(mut self, persistence: Arc<dyn DecisionPersistence>) -> Self {
        self.persistence = Some(persistence);
        self
    }

    pub fn build(self) -> Result<ChainedBroker, PermissionError> {
        Ok(ChainedBroker {
            chain: self.chain,
            terminator: self.terminator.unwrap_or_else(|| {
                Arc::new(FallbackTerminator::new(
                    FallbackPolicy::AskUser,
                    Arc::new(EmptyDecisionHistory),
                ))
            }),
            persistence: self
                .persistence
                .unwrap_or_else(|| Arc::new(NoopDecisionPersistence)),
        })
    }
}

#[async_trait]
impl PermissionBroker for ChainedBroker {
    fn can_anchor_authority(&self) -> bool {
        self.chain
            .iter()
            .any(|broker| broker.can_anchor_authority())
    }

    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        if self.hard_policy_denies(&request, &ctx).await {
            return Decision::DenyOnce;
        }

        for broker in &self.chain {
            match broker.decide(request.clone(), ctx.clone()).await {
                Decision::Escalate => continue,
                decision => return decision,
            }
        }
        self.terminator.terminate(&request, &ctx).await
    }

    async fn hard_policy_denies(
        &self,
        request: &PermissionRequest,
        ctx: &PermissionContext,
    ) -> bool {
        for broker in &self.chain {
            if broker.hard_policy_denies(request, ctx).await {
                return true;
            }
        }
        false
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.persistence.persist(decision).await
    }
}

impl FallbackTerminator {
    #[must_use]
    pub fn new(policy: FallbackPolicy, history: Arc<dyn DecisionHistoryQuery>) -> Self {
        Self { policy, history }
    }
}

#[async_trait]
impl PermissionTerminator for FallbackTerminator {
    #[allow(clippy::match_same_arms)]
    async fn terminate(&self, request: &PermissionRequest, ctx: &PermissionContext) -> Decision {
        if matches!(
            ctx.permission_mode,
            PermissionMode::BypassPermissions | PermissionMode::DontAsk
        ) {
            return Decision::AllowOnce;
        }

        match self.policy {
            FallbackPolicy::AskUser => match ctx.interactivity {
                InteractivityLevel::FullyInteractive => Decision::Escalate,
                InteractivityLevel::DeferredInteractive | InteractivityLevel::NoInteractive | _ => {
                    Decision::DenyOnce
                }
            },
            FallbackPolicy::DenyAll => Decision::DenyOnce,
            FallbackPolicy::AllowReadOnly => {
                if is_read_only_subject(&request.subject) {
                    Decision::AllowOnce
                } else {
                    Decision::DenyOnce
                }
            }
            FallbackPolicy::ClosestMatchingRule => self
                .history
                .find_closest(&request.scope_hint)
                .await
                .map(|prior| prior.decision)
                .unwrap_or(Decision::DenyOnce),
            _ => Decision::DenyOnce,
        }
    }
}

fn is_read_only_subject(subject: &PermissionSubject) -> bool {
    matches!(subject, PermissionSubject::ToolInvocation { .. })
}

use super::*;

#[cfg(feature = "stream-permission")]
pub struct StreamPermissionRuntime {
    permission_broker: Arc<dyn PermissionBroker>,
    resolver: ResolverHandle,
}

#[cfg(feature = "stream-permission")]
impl StreamPermissionRuntime {
    #[must_use]
    pub fn new(config: harness_permission::StreamBrokerConfig) -> Self {
        let (stream_broker, mut receiver, resolver) =
            harness_permission::StreamBasedBroker::new(config);

        thread::spawn(move || while receiver.blocking_recv().is_some() {});

        let permission_broker = Arc::new(PermissionAuthorityBroker::new(
            Arc::new(NoRulePolicyBroker),
            Some(Arc::new(stream_broker)),
            Arc::new(TransientDecisionStore::default()),
        ));

        Self {
            permission_broker,
            resolver,
        }
    }

    #[must_use]
    pub fn broker(&self) -> Arc<dyn PermissionBroker> {
        Arc::clone(&self.permission_broker)
    }

    #[must_use]
    pub fn resolver_handle(&self) -> ResolverHandle {
        self.resolver.clone()
    }

    #[must_use]
    pub fn pending_requests(&self) -> Vec<PermissionRequest> {
        self.resolver.pending_requests()
    }

    #[must_use]
    pub fn pending_permission_requests(&self) -> Vec<PendingPermissionRequest> {
        self.resolver.pending_permission_requests()
    }

    pub async fn resolve_permission(
        &self,
        request_id: RequestId,
        decision: Decision,
    ) -> Result<(), HarnessError> {
        self.resolver
            .resolve(request_id, decision)
            .await
            .map_err(HarnessError::Permission)
    }
}

#[cfg(feature = "stream-permission")]
impl Default for StreamPermissionRuntime {
    fn default() -> Self {
        Self::new(harness_permission::StreamBrokerConfig {
            default_timeout: Some(Duration::from_secs(300)),
            heartbeat_interval: None,
            max_pending: 1024,
        })
    }
}

impl Harness {
    pub async fn resolve_permission(
        &self,
        request_id: harness_contracts::RequestId,
        decision: Decision,
    ) -> Result<(), HarnessError> {
        #[cfg(feature = "stream-permission")]
        {
            if let Some(resolver) = &self.inner.permission_resolver {
                return resolver
                    .resolve(request_id, decision)
                    .await
                    .map_err(HarnessError::Permission);
            }
        }

        let _ = (&request_id, &decision);
        Err(HarnessError::Other(
            "permission resolver is not configured".to_owned(),
        ))
    }
}

#[cfg(test)]
#[allow(dead_code)]
pub(super) async fn default_permission_broker(
    options: &HarnessOptions,
    rule_providers: &[Arc<dyn RuleProvider>],
    decision_store: Option<Arc<dyn DecisionStore>>,
) -> Result<Arc<dyn PermissionBroker>, HarnessError> {
    permission_authority_runtime(options, None, rule_providers, decision_store)
        .await
        .map(|runtime| runtime.permission_broker)
}

#[cfg(test)]
#[allow(dead_code)]
pub(super) async fn permission_authority_broker(
    options: &HarnessOptions,
    interactive_broker: Option<Arc<dyn PermissionBroker>>,
    rule_providers: &[Arc<dyn RuleProvider>],
    decision_store: Option<Arc<dyn DecisionStore>>,
) -> Result<Arc<dyn PermissionBroker>, HarnessError> {
    permission_authority_runtime(options, interactive_broker, rule_providers, decision_store)
        .await
        .map(|runtime| runtime.permission_broker)
}

pub(super) struct PermissionAuthorityRuntime {
    pub(super) permission_authority: Arc<harness_permission::PermissionAuthority>,
    #[cfg(test)]
    #[allow(dead_code)]
    pub(super) permission_broker: Arc<dyn PermissionBroker>,
}

pub(super) async fn permission_authority_runtime(
    options: &HarnessOptions,
    interactive_broker: Option<Arc<dyn PermissionBroker>>,
    rule_providers: &[Arc<dyn RuleProvider>],
    decision_store: Option<Arc<dyn DecisionStore>>,
) -> Result<PermissionAuthorityRuntime, HarnessError> {
    let base_policy_broker =
        policy_broker(options, interactive_broker.is_none(), rule_providers).await?;
    let policy_broker = match &interactive_broker {
        Some(interactive_broker) => Arc::new(AuthorityPolicyBroker {
            policy_broker: base_policy_broker,
            hard_policy_broker: Arc::clone(interactive_broker),
        }) as Arc<dyn PermissionBroker>,
        None => base_policy_broker,
    };
    let mut builder = harness_permission::PermissionAuthority::builder()
        .with_policy_broker(policy_broker.clone());
    let decision_store = match decision_store {
        Some(decision_store) => {
            builder = builder.with_decision_store(decision_store.clone());
            decision_store
        }
        None => {
            let decision_store = Arc::new(TransientDecisionStore::default());
            builder = builder.with_transient_decision_store(decision_store.clone());
            decision_store
        }
    };

    let authority = Arc::new(
        builder
            .with_optional_interactive_broker(interactive_broker)
            .build()
            .map_err(HarnessError::Permission)?,
    );
    #[cfg(test)]
    let permission_broker = Arc::new(PermissionAuthorityBroker {
        authority: Arc::clone(&authority),
        policy_broker,
        decision_store,
    }) as Arc<dyn PermissionBroker>;
    #[cfg(not(test))]
    let _ = (policy_broker, decision_store);
    Ok(PermissionAuthorityRuntime {
        permission_authority: authority,
        #[cfg(test)]
        permission_broker,
    })
}

async fn policy_broker(
    options: &HarnessOptions,
    apply_full_rule_semantics: bool,
    rule_providers: &[Arc<dyn RuleProvider>],
) -> Result<Arc<dyn PermissionBroker>, HarnessError> {
    #[cfg(feature = "rule-engine-permission")]
    {
        if !rule_providers.is_empty() || apply_full_rule_semantics {
            let mut builder = harness_permission::RuleEngineBroker::builder()
                .with_tenant(options.tenant_policy.id);
            if !apply_full_rule_semantics {
                builder = builder.policy_deny_only();
            }
            for provider in rule_providers {
                builder = builder.with_rule_provider(Arc::clone(provider));
            }
            return builder
                .build()
                .await
                .map(|broker| Arc::new(broker) as Arc<dyn PermissionBroker>)
                .map_err(HarnessError::Permission);
        }
    }

    #[cfg(not(feature = "rule-engine-permission"))]
    {
        let _ = apply_full_rule_semantics;
        if !rule_providers.is_empty() {
            return Err(HarnessError::PermissionDenied(
                "rule providers require the `rule-engine-permission` feature".to_owned(),
            ));
        }
    }

    let _ = (options, rule_providers);
    Ok(Arc::new(NoRulePolicyBroker))
}

struct AuthorityPolicyBroker {
    policy_broker: Arc<dyn PermissionBroker>,
    hard_policy_broker: Arc<dyn PermissionBroker>,
}

#[async_trait]
impl PermissionBroker for AuthorityPolicyBroker {
    fn can_anchor_authority(&self) -> bool {
        self.policy_broker.can_anchor_authority()
    }

    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        if self.hard_policy_denies(&request, &ctx).await {
            return Decision::DenyOnce;
        }
        self.policy_broker.decide(request, ctx).await
    }

    async fn hard_policy_denies(
        &self,
        request: &PermissionRequest,
        ctx: &PermissionContext,
    ) -> bool {
        self.policy_broker.hard_policy_denies(request, ctx).await
            || self
                .hard_policy_broker
                .hard_policy_denies(request, ctx)
                .await
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.policy_broker.persist(decision).await
    }
}

pub(super) struct PermissionAuthorityBroker {
    pub(super) authority: Arc<harness_permission::PermissionAuthority>,
    pub(super) policy_broker: Arc<dyn PermissionBroker>,
    pub(super) decision_store: Arc<dyn DecisionStore>,
}

#[async_trait]
impl PermissionBroker for PermissionAuthorityBroker {
    fn can_anchor_authority(&self) -> bool {
        true
    }

    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        self.authority.decide(request, ctx).await
    }

    async fn hard_policy_denies(
        &self,
        request: &PermissionRequest,
        ctx: &PermissionContext,
    ) -> bool {
        self.policy_broker.hard_policy_denies(request, ctx).await
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.decision_store.persist(decision).await
    }
}

impl PermissionAuthorityBroker {
    #[cfg(feature = "stream-permission")]
    fn new(
        policy_broker: Arc<dyn PermissionBroker>,
        interactive_broker: Option<Arc<dyn PermissionBroker>>,
        decision_store: Arc<dyn DecisionStore>,
    ) -> Self {
        let mut builder = harness_permission::PermissionAuthority::builder()
            .with_policy_broker(policy_broker.clone())
            .with_transient_decision_store(decision_store.clone());
        if let Some(interactive_broker) = interactive_broker {
            builder = builder.with_interactive_broker(interactive_broker);
        }
        let authority = Arc::new(
            builder
                .build()
                .expect("permission authority broker inputs must be valid"),
        );
        Self {
            authority,
            policy_broker,
            decision_store,
        }
    }
}

trait PermissionAuthorityBuilderExt {
    fn with_optional_interactive_broker(
        self,
        broker: Option<Arc<dyn PermissionBroker>>,
    ) -> harness_permission::PermissionAuthorityBuilder;
}

impl PermissionAuthorityBuilderExt for harness_permission::PermissionAuthorityBuilder {
    fn with_optional_interactive_broker(
        self,
        broker: Option<Arc<dyn PermissionBroker>>,
    ) -> harness_permission::PermissionAuthorityBuilder {
        match broker {
            Some(broker) => self.with_interactive_broker(broker),
            None => self,
        }
    }
}

struct NoRulePolicyBroker;

#[async_trait]
impl PermissionBroker for NoRulePolicyBroker {
    fn can_anchor_authority(&self) -> bool {
        true
    }

    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::Escalate
    }

    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Ok(())
    }
}

#[derive(Debug, Default)]
struct TransientDecisionStore {
    decisions: parking_lot::Mutex<Vec<PersistedDecision>>,
}

#[async_trait]
impl DecisionPersistence for TransientDecisionStore {
    fn supports_integrity(&self) -> bool {
        false
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.decisions.lock().push(decision);
        Ok(())
    }
}

#[async_trait]
impl harness_permission::DecisionHistory for TransientDecisionStore {
    async fn find_scoped_decision(
        &self,
        lookup: harness_permission::DecisionLookup,
    ) -> Result<Option<PersistedDecision>, PermissionError> {
        Ok(self
            .decisions
            .lock()
            .iter()
            .find(|decision| {
                decision.source == lookup.decision_source
                    && harness_permission::policy_scope_matches_request(
                        &decision.scope,
                        &lookup.requested_scope,
                    )
                    && decision
                        .fingerprint
                        .is_some_and(|fingerprint| fingerprint == lookup.fingerprint)
            })
            .cloned())
    }
}

#[cfg(all(test, not(feature = "rule-engine-permission")))]
mod no_rule_engine_permission_tests {
    use super::*;

    struct NoopRuleProvider;

    #[async_trait]
    impl RuleProvider for NoopRuleProvider {
        fn provider_id(&self) -> &str {
            "noop-rule-provider"
        }

        fn source(&self) -> harness_contracts::RuleSource {
            harness_contracts::RuleSource::Workspace
        }

        async fn resolve_rules(
            &self,
            _tenant: TenantId,
        ) -> Result<Vec<harness_permission::PermissionRule>, PermissionError> {
            Ok(Vec::new())
        }

        fn watch(&self) -> Option<BoxStream<'static, harness_permission::RulesUpdated>> {
            None
        }
    }

    struct AllowPermissionBroker;

    #[async_trait]
    impl PermissionBroker for AllowPermissionBroker {
        async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
            Decision::AllowOnce
        }

        async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn default_permission_broker_rejects_rule_providers_without_rule_engine_feature() {
        let providers: Vec<Arc<dyn RuleProvider>> = vec![Arc::new(NoopRuleProvider)];
        let result = default_permission_broker(&HarnessOptions::default(), &providers, None).await;

        match result {
            Err(HarnessError::PermissionDenied(message)) => {
                assert!(message.contains("rule-engine-permission"));
            }
            Err(error) => panic!("expected permission denied, got {error}"),
            Ok(_) => panic!("rule providers should fail closed without rule-engine-permission"),
        }
    }

    #[tokio::test]
    async fn permission_authority_broker_rejects_rule_providers_without_rule_engine_feature() {
        let providers: Vec<Arc<dyn RuleProvider>> = vec![Arc::new(NoopRuleProvider)];
        let broker: Arc<dyn PermissionBroker> = Arc::new(AllowPermissionBroker);
        let result = permission_authority_broker(
            &HarnessOptions::default(),
            Some(broker),
            &providers,
            Some(Arc::new(TransientDecisionStore::default())),
        )
        .await;

        match result {
            Err(HarnessError::PermissionDenied(message)) => {
                assert!(message.contains("rule-engine-permission"));
            }
            Err(error) => panic!("expected permission denied, got {error}"),
            Ok(_) => panic!("rule providers should fail closed without rule-engine-permission"),
        }
    }
}

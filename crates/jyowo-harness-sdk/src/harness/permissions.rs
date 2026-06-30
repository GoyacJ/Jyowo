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
        let (broker, mut receiver, resolver) = harness_permission::StreamBasedBroker::new(config);

        thread::spawn(move || while receiver.blocking_recv().is_some() {});

        Self {
            permission_broker: Arc::new(broker),
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

pub(super) async fn default_permission_broker(
    options: &HarnessOptions,
    rule_providers: &[Arc<dyn RuleProvider>],
    decision_persistence: Option<Arc<dyn DecisionPersistence>>,
) -> Result<Arc<dyn PermissionBroker>, HarnessError> {
    #[cfg(feature = "rule-engine-permission")]
    {
        if !rule_providers.is_empty() {
            let mut builder = harness_permission::RuleEngineBroker::builder()
                .with_tenant(options.tenant_policy.id);
            for provider in rule_providers {
                builder = builder.with_rule_provider(Arc::clone(provider));
            }
            if let Some(persistence) = decision_persistence {
                builder = builder.with_persistence(persistence);
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
        if !rule_providers.is_empty() {
            return Err(HarnessError::PermissionDenied(
                "rule providers require the `rule-engine-permission` feature".to_owned(),
            ));
        }
    }

    let _ = (options, rule_providers, decision_persistence);
    Ok(Arc::new(DenyAllPermissionBroker))
}

pub(super) async fn policy_gated_permission_broker(
    options: &HarnessOptions,
    broker: Arc<dyn PermissionBroker>,
    rule_providers: &[Arc<dyn RuleProvider>],
) -> Result<Arc<dyn PermissionBroker>, HarnessError> {
    #[cfg(feature = "rule-engine-permission")]
    {
        if !rule_providers.is_empty() {
            let mut builder = harness_permission::RuleEngineBroker::builder()
                .with_tenant(options.tenant_policy.id)
                .policy_deny_only();
            for provider in rule_providers {
                builder = builder.with_rule_provider(Arc::clone(provider));
            }
            let policy_gate = builder.build().await.map_err(HarnessError::Permission)?;
            return Ok(Arc::new(PolicyGatedPermissionBroker {
                policy_gate: Arc::new(policy_gate),
                inner: broker,
            }));
        }
    }

    #[cfg(not(feature = "rule-engine-permission"))]
    {
        if !rule_providers.is_empty() {
            return Err(HarnessError::PermissionDenied(
                "rule providers require the `rule-engine-permission` feature".to_owned(),
            ));
        }
    }

    let _ = (options, rule_providers);
    Ok(broker)
}

#[cfg(feature = "rule-engine-permission")]
struct PolicyGatedPermissionBroker {
    policy_gate: Arc<dyn PermissionBroker>,
    inner: Arc<dyn PermissionBroker>,
}

#[cfg(feature = "rule-engine-permission")]
#[async_trait]
impl PermissionBroker for PolicyGatedPermissionBroker {
    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        if self.hard_policy_denies(&request, &ctx).await {
            return Decision::DenyOnce;
        }

        match self.policy_gate.decide(request.clone(), ctx.clone()).await {
            Decision::Escalate => self.inner.decide(request, ctx).await,
            decision => decision,
        }
    }

    async fn hard_policy_denies(
        &self,
        request: &PermissionRequest,
        ctx: &PermissionContext,
    ) -> bool {
        self.policy_gate.hard_policy_denies(request, ctx).await
            || self.inner.hard_policy_denies(request, ctx).await
            || harness_permission::hard_policy_denies_from_context(request, ctx)
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.inner.persist(decision).await
    }
}

struct DenyAllPermissionBroker;

#[async_trait]
impl PermissionBroker for DenyAllPermissionBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::DenyOnce
    }

    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Ok(())
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
    async fn policy_gated_permission_broker_rejects_rule_providers_without_rule_engine_feature() {
        let providers: Vec<Arc<dyn RuleProvider>> = vec![Arc::new(NoopRuleProvider)];
        let broker: Arc<dyn PermissionBroker> = Arc::new(AllowPermissionBroker);
        let result =
            policy_gated_permission_broker(&HarnessOptions::default(), broker, &providers).await;

        match result {
            Err(HarnessError::PermissionDenied(message)) => {
                assert!(message.contains("rule-engine-permission"));
            }
            Err(error) => panic!("expected permission denied, got {error}"),
            Ok(_) => panic!("rule providers should fail closed without rule-engine-permission"),
        }
    }
}

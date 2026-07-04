#![cfg(feature = "testing")]

mod runtime_assembly_support;
use runtime_assembly_support::*;

#[cfg(feature = "rule-engine-permission")]
#[test]
fn rule_provider_policy_deny_works_through_full_runtime() {
    block_on(async {
        let workspace = unique_workspace("sdk-rule-provider-deny");
        std::fs::create_dir_all(&workspace).unwrap();
        let session_id = SessionId::new();
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let model = Arc::new(TestModelProvider::default());

        struct DenyWorkspaceRuleProvider;
        #[async_trait]
        impl harness_permission::RuleProvider for DenyWorkspaceRuleProvider {
            fn provider_id(&self) -> &str {
                "deny-workspace-test"
            }

            fn source(&self) -> RuleSource {
                RuleSource::Workspace
            }

            async fn resolve_rules(
                &self,
                _tenant: TenantId,
            ) -> Result<Vec<harness_permission::PermissionRule>, harness_contracts::PermissionError>
            {
                Ok(vec![harness_permission::PermissionRule {
                    id: "deny-all-workspace".to_owned(),
                    priority: 100,
                    scope: DecisionScope::Any,
                    action: harness_permission::RuleAction::Deny,
                    source: RuleSource::Workspace,
                }])
            }

            fn watch(
                &self,
            ) -> Option<futures::stream::BoxStream<'static, harness_permission::RulesUpdated>>
            {
                None
            }
        }

        let harness = Harness::builder()
            .with_model_arc(model)
            .with_store_arc(store.clone())
            .with_sandbox(NoopSandbox::new())
            .with_rule_provider(Arc::new(DenyWorkspaceRuleProvider))
            .build()
            .await
            .expect("harness should build with rule provider");

        let permission_broker = harness
            .permission_broker()
            .expect("harness should have permission broker");
        let request = harness_permission::PermissionRequest {
            request_id: RequestId::new(),
            tenant_id: TenantId::SINGLE,
            session_id,
            tool_use_id: ToolUseId::new(),
            tool_name: "Bash".to_owned(),
            subject: PermissionSubject::CommandExec {
                command: "rm".to_owned(),
                argv: vec!["rm".to_owned(), "-rf".to_owned(), "/".to_owned()],
                cwd: None,
                fingerprint: None,
            },
            severity: Severity::Critical,
            scope_hint: DecisionScope::Any,
            confirmation_expected: None,
            created_at: chrono::Utc::now(),
        };
        let ctx = harness_permission::PermissionContext {
            permission_mode: PermissionMode::Default,
            previous_mode: None,
            session_id,
            tenant_id: TenantId::SINGLE,
            run_id: None,
            interactivity: InteractivityLevel::FullyInteractive,
            timeout_policy: None,
            fallback_policy: FallbackPolicy::DenyAll,
            hook_overrides: Vec::new(),
        };

        let decision = permission_broker.decide(request, ctx).await;
        assert_eq!(
            decision,
            Decision::DenyOnce,
            "rule provider deny should prevent workspace-scoped Bash execution"
        );
    });
}

#[test]
fn permission_authority_builder_method_exposes_prebuilt_authority() {
    block_on(async {
        let workspace = unique_workspace("sdk-prebuilt-authority");
        std::fs::create_dir_all(&workspace).unwrap();
        let model = Arc::new(TestModelProvider::default());
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let rule_broker: Arc<dyn harness_permission::PermissionBroker> = Arc::new(
            harness_permission::RuleEngineBroker::builder()
                .with_tenant(TenantId::SINGLE)
                .build()
                .await
                .expect("rule engine broker should build"),
        );
        let decision_store: Arc<dyn harness_permission::DecisionStore> =
            Arc::new(harness_permission::NoopDecisionPersistence);
        let authority = harness_permission::PermissionAuthority::builder()
            .with_policy_broker(
                Arc::clone(&rule_broker) as Arc<dyn harness_permission::PermissionBroker>
            )
            .with_transient_decision_store(
                Arc::clone(&decision_store) as Arc<dyn harness_permission::DecisionStore>
            )
            .build()
            .expect("permission authority should build");

        let harness = Harness::builder()
            .with_model_arc(model)
            .with_store_arc(store)
            .with_sandbox(NoopSandbox::new())
            .with_permission_authority(authority)
            .build()
            .await
            .expect("harness should build with pre-built authority");

        assert!(
            harness.permission_authority().is_some(),
            "harness should expose the pre-built permission authority"
        );
    });
}

#[test]
fn sdk_always_builds_production_authorization_service() {
    block_on(async {
        let workspace = unique_workspace("sdk-production-authorization-service");
        std::fs::create_dir_all(&workspace).unwrap();
        let model = Arc::new(TestModelProvider::default());
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));

        let harness = Harness::builder()
            .with_model_arc(model)
            .with_store_arc(store)
            .with_sandbox(NoopSandbox::new())
            .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
            .build()
            .await
            .expect("harness should build");

        let _authorization_service = harness.authorization_service();
        assert!(
            harness.permission_authority().is_some(),
            "SDK production assembly must keep the service-backed PermissionAuthority"
        );
    });
}

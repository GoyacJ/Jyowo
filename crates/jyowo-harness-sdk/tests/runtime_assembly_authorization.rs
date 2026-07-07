#![cfg(feature = "testing")]

mod runtime_assembly_support;
use runtime_assembly_support::*;

use std::path::PathBuf;

use futures::future::BoxFuture;
use harness_contracts::{
    ActionPlanHash, ActionPlanId, OutboundUserMessage, PermissionActorSource, PermissionReview,
    ResourceLimits, SandboxMode, SandboxPolicy, SandboxScope, ToolCapability, UserMessageDelivery,
    UserMessengerCap,
};
use harness_execution::AuthorizationContext;
use harness_permission::{NoopDecisionPersistence, PermissionAuthority};

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
            action_plan_hash: harness_contracts::ActionPlanHash::default(),
            decision_options: Vec::new(),
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

#[test]
fn default_authorization_service_uses_builder_capability_registry() {
    block_on(async {
        let workspace = unique_workspace("sdk-authorization-service-cap-registry");
        std::fs::create_dir_all(&workspace).unwrap();
        let model = Arc::new(TestModelProvider::default());
        let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let messenger: Arc<dyn UserMessengerCap> = Arc::new(FakeUserMessenger);
        let authority = PermissionAuthority::builder()
            .with_policy_broker(Arc::new(TestBroker::new(vec![Decision::AllowOnce])))
            .with_transient_decision_store(Arc::new(NoopDecisionPersistence))
            .build()
            .expect("permission authority should build");

        let harness = Harness::builder()
            .with_model_arc(model)
            .with_store_arc(store)
            .with_sandbox(NoopSandbox::new())
            .with_permission_authority(authority)
            .with_capability::<dyn UserMessengerCap>(ToolCapability::UserMessenger, messenger)
            .build()
            .await
            .expect("harness should build");

        let outcome = harness
            .authorization_service()
            .authorize_plan(authorization_context(), external_user_messenger_plan())
            .await
            .expect("default authorization service should see builder-registered capabilities");

        assert!(
            outcome.sandbox_backend_id.contains("external_capability"),
            "external capability preflight should not route through process sandbox"
        );
    });
}

struct FakeUserMessenger;

impl UserMessengerCap for FakeUserMessenger {
    fn send(
        &self,
        _message: OutboundUserMessage,
    ) -> BoxFuture<'static, Result<UserMessageDelivery, ToolError>> {
        Box::pin(async {
            Ok(UserMessageDelivery {
                message_id: "msg-1".to_owned(),
                delivered: true,
            })
        })
    }
}

fn authorization_context() -> AuthorizationContext {
    AuthorizationContext {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        run_id: RunId::new(),
        permission_mode: PermissionMode::Default,
        interactivity: InteractivityLevel::NoInteractive,
        fallback_policy: FallbackPolicy::DenyAll,
        workspace_root: PathBuf::from("/workspace"),
    }
}

fn external_user_messenger_plan() -> ToolActionPlan {
    let tool_use_id = ToolUseId::new();
    ToolActionPlan {
        plan_id: ActionPlanId::new(),
        tool_use_id,
        tool_name: "send_message".to_owned(),
        actor_source: PermissionActorSource::ParentRun,
        subject: PermissionSubject::ToolInvocation {
            tool: "send_message".to_owned(),
            input: serde_json::json!({ "body": "hello" }),
        },
        scope: DecisionScope::ToolName("send_message".to_owned()),
        severity: Severity::Info,
        resources: Vec::new(),
        sandbox_policy: SandboxPolicy {
            mode: SandboxMode::None,
            scope: SandboxScope::WorkspaceOnly,
            network: NetworkAccess::None,
            resource_limits: ResourceLimits {
                max_memory_bytes: None,
                max_cpu_cores: None,
                max_pids: None,
                max_wall_clock_ms: None,
                max_open_files: None,
            },
            denied_host_paths: Vec::new(),
        },
        workspace_access: WorkspaceAccess::None,
        network_access: NetworkAccess::None,
        execution_channel: ToolExecutionChannel::ExternalCapability {
            capability: ToolCapability::UserMessenger,
        },
        review: PermissionReview::default(),
        plan_hash: ActionPlanHash::from_bytes([42; 32]),
        created_at: chrono::Utc::now(),
    }
}

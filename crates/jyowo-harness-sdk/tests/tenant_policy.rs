#![cfg(feature = "testing")]

use std::collections::HashSet;
use std::sync::Arc;

use futures::executor::block_on;
use harness_contracts::{EndReason, HarnessError, TenantId};
use harness_model::ModelProvider;
use harness_tool::ToolRegistry;
use jyowo_harness_sdk::{prelude::*, testing::*};

#[test]
fn tenant_policy_filters_tools_from_sdk_runtime() {
    block_on(async {
        let workspace = unique_workspace("sdk-tenant-tools");
        std::fs::create_dir_all(&workspace).unwrap();
        let model = Arc::new(TestModelProvider::default());
        let model_provider: Arc<dyn ModelProvider> = model.clone();
        let registry = ToolRegistry::builder()
            .with_tool(Box::new(TestTool::new("allowed_tool")))
            .with_tool(Box::new(TestTool::new("blocked_tool")))
            .build()
            .unwrap();
        let harness = Harness::builder()
            .with_model_arc(model_provider)
            .with_store(InMemoryEventStore::new(std::sync::Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_tool_registry(registry)
            .with_tenant_policy(TenantPolicy {
                allowed_tools: Some(HashSet::from(["allowed_tool".to_owned()])),
                ..TenantPolicy::default()
            })
            .build()
            .await
            .unwrap();

        let session = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .unwrap();
        session.run_turn("list tools").await.unwrap();

        let requests = model.requests().await;
        let tools = requests[0].tools.as_ref().expect("tools should be sent");
        let names = tools
            .iter()
            .map(|descriptor| descriptor.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["allowed_tool"]);
    });
}

#[test]
fn tenant_policy_rejects_disallowed_provider() {
    block_on(async {
        let workspace = unique_workspace("sdk-tenant-provider");
        std::fs::create_dir_all(&workspace).unwrap();
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(std::sync::Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_tenant_policy(TenantPolicy {
                allowed_providers: Some(HashSet::from(["openai".to_owned()])),
                ..TenantPolicy::default()
            })
            .build()
            .await
            .unwrap();

        let error = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .unwrap_err();

        assert!(
            matches!(error, HarnessError::PermissionDenied(message) if message.contains("provider `test`"))
        );
    });
}

#[test]
fn tenant_policy_enforces_max_concurrent_sessions() {
    block_on(async {
        let workspace = unique_workspace("sdk-tenant-session-limit");
        std::fs::create_dir_all(&workspace).unwrap();
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(std::sync::Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_tenant_policy(TenantPolicy {
                max_concurrent_sessions: Some(1),
                ..TenantPolicy::default()
            })
            .build()
            .await
            .unwrap();

        let first = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .unwrap();
        let error = harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .unwrap_err();
        assert!(
            matches!(error, HarnessError::PermissionDenied(message) if message.contains("session limit"))
        );

        first.end(EndReason::Completed).await.unwrap();

        harness
            .create_session(SessionOptions::new(&workspace))
            .await
            .expect("ended session should release tenant session slot");
    });
}

#[test]
fn tenant_policy_rejects_wrong_tenant_id() {
    block_on(async {
        let workspace = unique_workspace("sdk-tenant-id");
        std::fs::create_dir_all(&workspace).unwrap();
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(std::sync::Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .unwrap();

        let error = harness
            .create_session(SessionOptions::new(&workspace).with_tenant_id(TenantId::SHARED))
            .await
            .unwrap_err();

        assert!(matches!(error, HarnessError::InvalidTenant(tenant) if tenant == TenantId::SHARED));
    });
}

#[test]
fn tenant_policy_can_allow_adapter_scoped_tenant_ids() {
    block_on(async {
        let workspace = unique_workspace("sdk-tenant-scoped");
        std::fs::create_dir_all(&workspace).unwrap();
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(std::sync::Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_tenant_policy(TenantPolicy {
                allow_scoped_tenants: true,
                ..TenantPolicy::default()
            })
            .build()
            .await
            .unwrap();

        harness
            .create_session(SessionOptions::new(&workspace).with_tenant_id(TenantId::SHARED))
            .await
            .expect("scoped tenants should be accepted when policy enables them");
    });
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-{name}-{}-{}",
        std::process::id(),
        harness_contracts::SessionId::new()
    ))
}

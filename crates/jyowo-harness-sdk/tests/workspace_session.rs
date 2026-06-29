#![cfg(feature = "testing")]

use std::sync::Arc;

use futures::executor::block_on;
use harness_contracts::{NoopRedactor, TenantId};
use harness_model::ModelProtocol;
use jyowo_harness_sdk::{builtin::*, prelude::*, testing::*};
use serde_json::json;

#[test]
fn sdk_workspace_registry_creates_lists_and_gets_workspaces() {
    block_on(async {
        let harness = test_harness(None).await;
        let root = unique_workspace("sdk-workspace-registry");

        let workspace = harness
            .create_workspace(WorkspaceSpec::new(&root, "SDK Workspace"))
            .await
            .expect("workspace should be created");

        assert_eq!(workspace.root_path, root.canonicalize().unwrap());
        assert!(harness.get_workspace(workspace.id).await.unwrap().is_some());
        assert_eq!(
            harness
                .list_workspaces(TenantId::SINGLE)
                .await
                .unwrap()
                .len(),
            1
        );
    });
}

#[test]
fn workspace_bound_session_applies_defaults_and_bootstrap() {
    block_on(async {
        let model = Arc::new(TestModelProvider::default());
        let harness = test_harness(Some(model.clone())).await;
        let root = unique_workspace("sdk-workspace-bound-session");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("AGENTS.md"), "workspace bootstrap").unwrap();

        let workspace = harness
            .create_workspace(
                WorkspaceSpec::new(&root, "Bound")
                    .with_bootstrap_files(vec![BootstrapFileSpec::required("AGENTS.md")])
                    .with_default_session_options(
                        SessionOptions::default()
                            .with_model_id("test-model")
                            .with_protocol(ModelProtocol::Responses)
                            .with_model_extra(json!({ "from": "workspace" })),
                    ),
            )
            .await
            .expect("workspace should be registered");

        let session = harness
            .create_session(
                SessionOptions::default()
                    .with_workspace(workspace.id)
                    .with_system_prompt_addendum("session addendum"),
            )
            .await
            .expect("workspace-bound session should be created");
        session.run_turn("hello").await.unwrap();

        let requests = model.requests().await;
        assert_eq!(requests[0].model_id, "test-model");
        assert_eq!(requests[0].protocol, ModelProtocol::Responses);
        assert_eq!(requests[0].extra["from"], json!("workspace"));
        assert!(requests[0].extra["relay_logical_call_key"]
            .as_str()
            .is_some_and(|value| value.starts_with("engine_turn:")));
        let system = requests[0].system.as_deref().unwrap_or_default();
        assert!(system.contains("workspace bootstrap"));
        assert!(system.contains("session addendum"));
    });
}

#[test]
fn explicit_session_options_override_workspace_defaults() {
    block_on(async {
        let model = Arc::new(TestModelProvider::default());
        let harness = test_harness(Some(model.clone())).await;
        let root = unique_workspace("sdk-workspace-explicit-override");

        let workspace = harness
            .create_workspace(
                WorkspaceSpec::new(&root, "Overrides").with_default_session_options(
                    SessionOptions::default()
                        .with_model_id("test-model")
                        .with_model_extra(json!({ "from": "workspace" })),
                ),
            )
            .await
            .expect("workspace should be registered");

        let session = harness
            .create_session(
                SessionOptions::default()
                    .with_workspace(workspace.id)
                    .with_model_id("test-model")
                    .with_model_extra(json!({ "from": "explicit" })),
            )
            .await
            .expect("session should be created");
        session.run_turn("hello").await.unwrap();

        let requests = model.requests().await;
        assert_eq!(requests[0].model_id, "test-model");
        assert_eq!(requests[0].extra["from"], json!("explicit"));
        assert!(requests[0].extra["relay_logical_call_key"]
            .as_str()
            .is_some_and(|value| value.starts_with("engine_turn:")));
    });
}

#[test]
fn missing_workspace_fails_before_engine_assembly() {
    block_on(async {
        let model = Arc::new(TestModelProvider::default());
        let harness = test_harness(Some(model.clone())).await;

        let error = harness
            .create_session(SessionOptions::default().with_workspace(WorkspaceId::new()))
            .await
            .expect_err("missing workspace should fail");

        assert!(error.to_string().contains("workspace not found"));
        assert!(model.requests().await.is_empty());
    });
}

#[test]
fn workspace_bootstrap_rejects_paths_outside_workspace() {
    block_on(async {
        let harness = test_harness(None).await;
        let root = unique_workspace("sdk-workspace-bootstrap-escape");

        let workspace = harness
            .create_workspace(
                WorkspaceSpec::new(&root, "Escape")
                    .with_bootstrap_files(vec![BootstrapFileSpec::required("../AGENTS.md")]),
            )
            .await
            .expect("workspace should be registered");

        let error = harness
            .create_session(SessionOptions::default().with_workspace(workspace.id))
            .await
            .expect_err("escaping bootstrap path should fail");

        assert!(error.to_string().contains("must stay inside workspace"));
    });
}

#[test]
fn default_session_options_apply_to_normal_create_session() {
    block_on(async {
        let model = Arc::new(TestModelProvider::default());
        let root = unique_workspace("sdk-default-session-options");
        std::fs::create_dir_all(&root).unwrap();
        let harness = test_harness_with_defaults(
            model.clone(),
            SessionOptions::default()
                .with_model_id("test-model")
                .with_model_extra(json!({ "from": "default" }))
                .with_system_prompt_addendum("default addendum"),
        )
        .await;

        let session = harness
            .create_session(SessionOptions::new(&root))
            .await
            .expect("standalone session should use defaults");
        session.run_turn("hello").await.unwrap();

        let requests = model.requests().await;
        assert_eq!(requests[0].model_id, "test-model");
        assert_eq!(requests[0].extra["from"], json!("default"));
        assert!(requests[0].extra["relay_logical_call_key"]
            .as_str()
            .is_some_and(|value| value.starts_with("engine_turn:")));
        let system = requests[0].system.as_deref().unwrap_or_default();
        assert!(system.contains("Jyowo"));
        assert!(system.contains("不能以底层 model provider 身份自称"));
        assert!(system.contains("<session-addendum>"));
        assert!(system.contains("default addendum"));
    });
}

async fn test_harness(model: Option<Arc<TestModelProvider>>) -> Harness {
    let model = model.unwrap_or_else(|| Arc::new(TestModelProvider::default()));
    test_harness_with_defaults(model, SessionOptions::default()).await
}

async fn test_harness_with_defaults(
    model: Arc<TestModelProvider>,
    defaults: SessionOptions,
) -> Harness {
    let model_provider: Arc<dyn ModelProvider> = model;
    Harness::builder()
        .with_model_arc(model_provider)
        .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
        .with_sandbox(NoopSandbox::new())
        .with_default_session_options(defaults)
        .build()
        .await
        .expect("test harness should build")
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("{name}-{}", SessionId::new()))
}

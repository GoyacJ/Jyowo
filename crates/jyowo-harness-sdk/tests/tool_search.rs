#![cfg(all(feature = "tool-search", feature = "testing"))]

use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_contracts::{
    BudgetMetric, Decision, DeferPolicy, Event, NetworkAccess, OverflowAction, PermissionMode,
    ProviderRestriction, ToolActionPlan, ToolDescriptor, ToolError, ToolExecutionChannel,
    ToolGroup, ToolOrigin, ToolProperties, ToolResult, ToolSearchMode, ToolUseId, TrustLevel,
    WorkspaceAccess,
};
use harness_journal::EventStore;
use harness_model::{ContentDelta, ModelRequest, ModelStreamEvent};
use harness_tool::{
    action_plan_from_permission_check, AuthorizedToolInput, BuiltinToolset, PermissionCheck, Tool,
    ToolContext, ToolEvent, ToolRegistry, ToolStream, ValidationError,
};
use harness_tool_search::{ScoringContext, ScoringTerms, ToolSearchScorer};
use jyowo_harness_sdk::{testing, Harness, HarnessError, SessionOptions, TenantId, TenantPolicy};
use serde_json::{json, Value};

#[tokio::test]
async fn global_disable_tool_search_converts_auto_defer_to_always_loaded() {
    let model = Arc::new(testing::TestModelProvider::default());
    let harness = harness_with_registry(
        model.clone(),
        ToolRegistry::builder()
            .with_builtin_toolset(BuiltinToolset::Empty)
            .with_tool(Box::new(TestTool::new(
                "auto_deferred",
                DeferPolicy::AutoDefer,
            )))
            .build()
            .expect("tool registry should build"),
    )
    .disable_tool_search()
    .build()
    .await
    .expect("harness should build");

    let session = harness
        .create_session(
            SessionOptions::new(unique_workspace("sdk-tool-search-global-disabled-auto"))
                .with_tool_search_mode(ToolSearchMode::Always),
        )
        .await
        .expect("session should be created");
    session
        .run_turn("show tools")
        .await
        .expect("turn should run");

    let tool_names = first_request_tool_names(&model.requests().await);
    assert!(tool_names.contains(&"auto_deferred".to_owned()));
    assert!(!tool_names.contains(&"tool_search".to_owned()));
}

#[tokio::test]
async fn global_disable_tool_search_rejects_force_defer_during_session_assembly() {
    let harness = harness_with_registry(
        Arc::new(testing::TestModelProvider::default()),
        ToolRegistry::builder()
            .with_builtin_toolset(BuiltinToolset::Empty)
            .with_tool(Box::new(TestTool::new(
                "force_deferred",
                DeferPolicy::ForceDefer,
            )))
            .build()
            .expect("tool registry should build"),
    )
    .disable_tool_search()
    .build()
    .await
    .expect("harness should build");

    let error = harness
        .create_session(
            SessionOptions::new(unique_workspace("sdk-tool-search-global-disabled-force"))
                .with_tool_search_mode(ToolSearchMode::Always),
        )
        .await
        .expect_err("force-deferred tools must fail when tool search is globally disabled");

    assert!(matches!(
        error,
        HarnessError::Tool(ToolError::DeferralRequired { tool }) if tool == "force_deferred"
    ));
}

#[tokio::test]
async fn tenant_allowed_tools_cannot_enable_tool_search_when_tool_is_denied() {
    let model = Arc::new(testing::TestModelProvider::default());
    let mut allowed = std::collections::HashSet::new();
    allowed.insert("allowed_tool".to_owned());
    let harness = harness_with_registry(
        model.clone(),
        ToolRegistry::builder()
            .with_builtin_toolset(BuiltinToolset::Empty)
            .with_tool(Box::new(TestTool::new(
                "allowed_tool",
                DeferPolicy::AlwaysLoad,
            )))
            .build()
            .expect("tool registry should build"),
    )
    .with_tenant_policy(TenantPolicy {
        allowed_tools: Some(allowed),
        ..TenantPolicy::default()
    })
    .build()
    .await
    .expect("harness should build");

    let session = harness
        .create_session(
            SessionOptions::new(unique_workspace("sdk-tool-search-tenant-denied"))
                .with_tool_search_mode(ToolSearchMode::Always),
        )
        .await
        .expect("session should be created");
    session
        .run_turn("show tools")
        .await
        .expect("turn should run");

    let tool_names = first_request_tool_names(&model.requests().await);
    assert_eq!(tool_names, vec!["allowed_tool".to_owned()]);
}

#[tokio::test]
async fn admin_custom_tool_search_scorer_changes_result_order() {
    let store = Arc::new(testing::InMemoryEventStore::new(Arc::new(
        testing::NoopRedactor,
    )));
    let tool_use_id = ToolUseId::new();
    let model = Arc::new(testing::ScriptedProvider::new(vec![
        testing::ScriptedResponse::Stream(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::ToolUseComplete {
                    id: tool_use_id,
                    name: "tool_search".to_owned(),
                    input: json!({ "query": "rank" }),
                },
            },
            ModelStreamEvent::MessageStop,
        ]),
        testing::ScriptedResponse::Stream(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("done".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ]),
    ]));
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .with_tool(Box::new(TestTool::new(
            "alpha_tool",
            DeferPolicy::ForceDefer,
        )))
        .with_tool(Box::new(TestTool::new(
            "beta_tool",
            DeferPolicy::ForceDefer,
        )))
        .build()
        .expect("tool registry should build");

    let harness = Harness::builder()
        .with_model_arc(model)
        .with_store_arc(store.clone())
        .with_sandbox(testing::NoopSandbox::new())
        .with_permission_broker(testing::TestBroker::new(vec![Decision::AllowOnce]))
        .with_tool_registry(registry)
        .with_tool_search_scorer(PreferBetaScorer)
        .build()
        .await
        .expect("harness should build");

    let session_id = jyowo_harness_sdk::SessionId::new();
    let session = harness
        .create_session(
            SessionOptions::new(unique_workspace("sdk-tool-search-custom-scorer"))
                .with_session_id(session_id)
                .with_tool_search_mode(ToolSearchMode::Always)
                .with_permission_mode(PermissionMode::BypassPermissions),
        )
        .await
        .expect("session should be created");
    session
        .run_turn("rank deferred tools")
        .await
        .expect("tool search should run");

    let events: Vec<_> = store
        .read(
            TenantId::SINGLE,
            session_id,
            harness_journal::ReplayCursor::FromStart,
        )
        .await
        .expect("events should be readable")
        .collect()
        .await;
    let queried = events
        .iter()
        .find_map(|event| match event {
            Event::ToolSearchQueried(queried) => Some(queried),
            _ => None,
        })
        .expect("tool search query should be journaled");
    assert_eq!(
        queried.matched,
        vec!["beta_tool".to_owned(), "alpha_tool".to_owned()]
    );
    assert_eq!(
        queried.scored,
        vec![("beta_tool".to_owned(), 20), ("alpha_tool".to_owned(), 10)]
    );
}

fn harness_with_registry(
    model: Arc<testing::TestModelProvider>,
    registry: ToolRegistry,
) -> jyowo_harness_sdk::HarnessBuilder<
    jyowo_harness_sdk::Set<Arc<dyn harness_model::ModelProvider>>,
    jyowo_harness_sdk::Set<Arc<dyn harness_journal::EventStore>>,
    jyowo_harness_sdk::Set<Arc<dyn harness_sandbox::SandboxBackend>>,
> {
    Harness::builder()
        .with_model_arc(model)
        .with_store_arc(Arc::new(testing::InMemoryEventStore::new(Arc::new(
            testing::NoopRedactor,
        ))))
        .with_sandbox(testing::NoopSandbox::new())
        .with_tool_registry(registry)
}

fn first_request_tool_names(requests: &[ModelRequest]) -> Vec<String> {
    requests[0]
        .tools
        .as_ref()
        .expect("model request should include tool schemas")
        .iter()
        .map(|tool| tool.name.clone())
        .collect()
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{name}-{unique}"));
    std::fs::create_dir_all(&path).expect("workspace should be creatable");
    path
}

struct PreferBetaScorer;

#[async_trait]
impl ToolSearchScorer for PreferBetaScorer {
    async fn score(
        &self,
        tool: &ToolDescriptor,
        _properties: &ToolProperties,
        _terms: &ScoringTerms,
        _context: &ScoringContext,
    ) -> u32 {
        match tool.name.as_str() {
            "beta_tool" => 20,
            "alpha_tool" => 10,
            _ => 0,
        }
    }
}

#[derive(Debug, Clone)]
struct TestTool {
    descriptor: ToolDescriptor,
}

impl TestTool {
    fn new(name: &str, defer_policy: DeferPolicy) -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: name.to_owned(),
                display_name: name.to_owned(),
                description: "test tool".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::Custom("test".to_owned()),
                version: "0.1.0".to_owned(),
                input_schema: json!({ "type": "object" }),
                output_schema: None,
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: true,
                    is_destructive: false,
                    long_running: None,
                    defer_policy,
                },
                trust_level: TrustLevel::UserControlled,
                required_capabilities: Vec::new(),
                budget: harness_contracts::ResultBudget {
                    metric: BudgetMetric::Chars,
                    limit: 8_192,
                    on_overflow: OverflowAction::Truncate,
                    preview_head_chars: 1_024,
                    preview_tail_chars: 1_024,
                },
                provider_restriction: ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
                search_hint: None,
                service_binding: None,
            },
        }
    }
}

#[async_trait]
impl Tool for TestTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            PermissionCheck::Allowed,
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Text("ok".to_owned()),
        )])))
    }
}

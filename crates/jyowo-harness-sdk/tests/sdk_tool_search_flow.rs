#![cfg(all(feature = "tool-search", feature = "testing"))]

use std::sync::Arc;

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_contracts::{
    BudgetMetric, Decision, DeferPolicy, Event, OverflowAction, ProviderRestriction,
    ToolDescriptor, ToolError, ToolGroup, ToolOrigin, ToolProperties, ToolResult, ToolSearchMode,
    ToolUseId, TrustLevel,
};
use harness_journal::{EventStore, ReplayCursor};
use harness_model::{ContentDelta, ModelRequest, ModelStreamEvent};
use harness_tool::{
    BuiltinToolset, PermissionCheck, Tool, ToolContext, ToolEvent, ToolRegistry, ToolStream,
    ValidationError,
};
use harness_tool_search::{ScoringContext, ScoringTerms, ToolSearchScorer};
use jyowo_harness_sdk::{prelude::*, testing::*};
use serde_json::{json, Value};

#[tokio::test]
async fn sdk_tool_search_flow_searches_deferred_tools_by_default() {
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let tool_use_id = ToolUseId::new();
    let model = Arc::new(ScriptedProvider::new(vec![
        ScriptedResponse::Stream(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::ToolUseComplete {
                    id: tool_use_id,
                    name: "tool_search".to_owned(),
                    input: json!({ "query": "alpha" }),
                },
            },
            ModelStreamEvent::MessageStop,
        ]),
        ScriptedResponse::Stream(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("done".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ]),
    ]));
    let harness = harness_with_registry(
        model,
        store.clone(),
        ToolRegistry::builder()
            .with_builtin_toolset(BuiltinToolset::Empty)
            .with_tool(Box::new(TestTool::new(
                "alpha_tool",
                DeferPolicy::ForceDefer,
            )))
            .build()
            .expect("tool registry should build"),
    )
    .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
    .build()
    .await
    .expect("harness should build");

    let session_id = SessionId::new();
    let session = harness
        .create_session(
            SessionOptions::new(unique_workspace("sdk-tool-search-default"))
                .with_session_id(session_id)
                .with_tool_search_mode(ToolSearchMode::Always),
        )
        .await
        .expect("session should be created");
    session
        .run_turn("find deferred alpha tool")
        .await
        .expect("tool search should run");

    let queried = tool_search_query(&store, session_id).await;
    assert_eq!(queried, vec!["alpha_tool".to_owned()]);
}

#[tokio::test]
async fn sdk_tool_search_flow_session_disabled_removes_search_tool() {
    let model = Arc::new(TestModelProvider::default());
    let harness = harness_with_registry(
        model.clone(),
        Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))),
        ToolRegistry::builder()
            .with_builtin_toolset(BuiltinToolset::Empty)
            .with_tool(Box::new(TestTool::new("auto_tool", DeferPolicy::AutoDefer)))
            .build()
            .expect("tool registry should build"),
    )
    .build()
    .await
    .expect("harness should build");

    let session = harness
        .create_session(
            SessionOptions::new(unique_workspace("sdk-tool-search-session-disabled"))
                .with_tool_search_mode(ToolSearchMode::Disabled),
        )
        .await
        .expect("session should be created");
    session.run_turn("show direct tools").await.unwrap();

    let tools = first_request_tool_names(&model.requests().await);
    assert_eq!(tools, vec!["auto_tool".to_owned()]);
}

#[tokio::test]
async fn sdk_tool_search_flow_global_disabled_overrides_session_search() {
    let model = Arc::new(TestModelProvider::default());
    let harness = harness_with_registry(
        model.clone(),
        Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor))),
        ToolRegistry::builder()
            .with_builtin_toolset(BuiltinToolset::Empty)
            .with_tool(Box::new(TestTool::new("auto_tool", DeferPolicy::AutoDefer)))
            .build()
            .expect("tool registry should build"),
    )
    .disable_tool_search()
    .build()
    .await
    .expect("harness should build");

    let session = harness
        .create_session(
            SessionOptions::new(unique_workspace("sdk-tool-search-global-disabled"))
                .with_tool_search_mode(ToolSearchMode::Always),
        )
        .await
        .expect("session should be created");
    session.run_turn("show direct tools").await.unwrap();

    let tools = first_request_tool_names(&model.requests().await);
    assert_eq!(tools, vec!["auto_tool".to_owned()]);
}

#[tokio::test]
async fn sdk_tool_search_flow_custom_scorer_controls_match_order() {
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let tool_use_id = ToolUseId::new();
    let model = Arc::new(ScriptedProvider::new(vec![
        ScriptedResponse::Stream(vec![
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
        ScriptedResponse::Stream(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("done".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ]),
    ]));
    let harness = harness_with_registry(
        model,
        store.clone(),
        ToolRegistry::builder()
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
            .expect("tool registry should build"),
    )
    .with_permission_broker(TestBroker::new(vec![Decision::AllowOnce]))
    .with_tool_search_scorer(PreferBetaScorer)
    .build()
    .await
    .expect("harness should build");

    let session_id = SessionId::new();
    let session = harness
        .create_session(
            SessionOptions::new(unique_workspace("sdk-tool-search-custom"))
                .with_session_id(session_id)
                .with_tool_search_mode(ToolSearchMode::Always),
        )
        .await
        .expect("session should be created");
    session.run_turn("rank tools").await.unwrap();

    let queried = tool_search_query(&store, session_id).await;
    assert_eq!(
        queried,
        vec!["beta_tool".to_owned(), "alpha_tool".to_owned()]
    );
}

fn harness_with_registry(
    model: Arc<dyn harness_model::ModelProvider>,
    store: Arc<InMemoryEventStore>,
    registry: ToolRegistry,
) -> HarnessBuilder<
    jyowo_harness_sdk::Set<Arc<dyn harness_model::ModelProvider>>,
    jyowo_harness_sdk::Set<Arc<dyn harness_journal::EventStore>>,
    jyowo_harness_sdk::Set<Arc<dyn harness_sandbox::SandboxBackend>>,
> {
    Harness::builder()
        .with_model_arc(model)
        .with_store_arc(store)
        .with_sandbox(NoopSandbox::new())
        .with_tool_registry(registry)
}

async fn tool_search_query(store: &Arc<InMemoryEventStore>, session_id: SessionId) -> Vec<String> {
    let events: Vec<_> = store
        .read(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
        .await
        .expect("journal should be readable")
        .collect()
        .await;
    events
        .into_iter()
        .find_map(|event| match event {
            Event::ToolSearchQueried(queried) => Some(queried.matched),
            _ => None,
        })
        .expect("tool search query should be journaled")
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
    let path = std::env::temp_dir().join(format!("{name}-{}", SessionId::new()));
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

    async fn check_permission(&self, _input: &Value, _ctx: &ToolContext) -> PermissionCheck {
        PermissionCheck::Allowed
    }

    async fn execute(&self, _input: Value, _ctx: ToolContext) -> Result<ToolStream, ToolError> {
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Text("ok".to_owned()),
        )])))
    }
}

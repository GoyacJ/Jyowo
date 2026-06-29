use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    BudgetMetric, DeferPolicy, HookError, HookEventKind, OverflowAction, ProviderRestriction,
    ResultBudget, ToolDescriptor, ToolGroup, ToolOrigin, ToolProperties, ToolResult, TrustLevel,
};
use harness_hook::{HookContext, HookEvent, HookHandler, HookOutcome};
use harness_permission::PermissionCheck;
use harness_tool::{Tool, ToolContext, ToolEvent, ToolStream, ValidationError};
use serde_json::{json, Value};

pub use harness_contracts::NoopRedactor;
pub use harness_journal::{
    test_event_store, InMemoryBlobStore, InMemoryEventStore, TestEventStore,
};
pub use harness_memory::InMemoryMemoryProvider;
pub use harness_model::{
    ScriptedProvider, ScriptedResponse, TestCredentialSource, TestModelProvider,
};
pub use harness_permission::{TestBroker, TestBrokerCall};
pub use harness_sandbox::NoopSandbox;

#[derive(Debug, Clone)]
pub struct TestTool {
    descriptor: ToolDescriptor,
}

impl TestTool {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            descriptor: ToolDescriptor {
                name: name.clone(),
                display_name: name,
                description: "SDK testing tool".to_owned(),
                category: "testing".to_owned(),
                group: ToolGroup::Meta,
                version: "0.1.0".to_owned(),
                input_schema: json!({"type": "object"}),
                output_schema: None,
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: true,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                trust_level: TrustLevel::AdminTrusted,
                required_capabilities: Vec::new(),
                budget: ResultBudget {
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

    async fn check_permission(&self, _input: &Value, _ctx: &ToolContext) -> PermissionCheck {
        PermissionCheck::Allowed
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolContext,
    ) -> Result<ToolStream, harness_contracts::ToolError> {
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Text("test tool result".to_owned()),
        )])))
    }
}

#[derive(Debug, Clone)]
pub struct TestHookHandler {
    id: String,
    interested_events: Vec<HookEventKind>,
}

impl TestHookHandler {
    #[must_use]
    pub fn new(id: impl Into<String>, interested_events: Vec<HookEventKind>) -> Self {
        Self {
            id: id.into(),
            interested_events,
        }
    }
}

#[async_trait]
impl HookHandler for TestHookHandler {
    fn handler_id(&self) -> &str {
        &self.id
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &self.interested_events
    }

    async fn handle(&self, _event: HookEvent, _ctx: HookContext) -> Result<HookOutcome, HookError> {
        Ok(HookOutcome::Continue)
    }
}

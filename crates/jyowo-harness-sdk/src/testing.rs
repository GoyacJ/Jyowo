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
    mock_event_store, InMemoryBlobStore, InMemoryEventStore, MockEventStore,
};
pub use harness_memory::MockMemoryProvider;
pub use harness_model::{MockCredentialSource, MockProvider, ScriptedProvider, ScriptedResponse};
pub use harness_permission::{MockBroker, MockBrokerCall};
pub use harness_sandbox::NoopSandbox;

#[derive(Debug, Clone)]
pub struct MockTool {
    descriptor: ToolDescriptor,
}

impl MockTool {
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
            },
        }
    }
}

#[async_trait]
impl Tool for MockTool {
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
            ToolResult::Text("mock tool result".to_owned()),
        )])))
    }
}

#[derive(Debug, Clone)]
pub struct MockHookHandler {
    id: String,
    interested_events: Vec<HookEventKind>,
}

impl MockHookHandler {
    #[must_use]
    pub fn new(id: impl Into<String>, interested_events: Vec<HookEventKind>) -> Self {
        Self {
            id: id.into(),
            interested_events,
        }
    }
}

#[async_trait]
impl HookHandler for MockHookHandler {
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

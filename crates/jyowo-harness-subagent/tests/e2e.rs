use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::{
    BudgetMetric, CapabilityRegistry, DeferPolicy, OverflowAction, ProviderRestriction,
    ResultBudget, SubagentRunnerCap, ToolCapability, ToolDescriptor, ToolGroup, ToolOrigin,
    ToolProperties, ToolResult, TrustLevel, UsageSnapshot,
};
use harness_permission::PermissionCheck;
use harness_subagent::{
    AgentTool, ParentContext, SubagentAnnouncement, SubagentHandle, SubagentRunner,
    SubagentRunnerCapAdapter, SubagentSpec, SubagentStatus,
};
use harness_tool::{Tool, ToolContext, ToolEvent, ToolStream, ValidationError};
use serde_json::{json, Value};

#[tokio::test]
async fn parent_agent_tool_spawns_child_that_calls_tool_and_announces_back() {
    let child_tool = Arc::new(ChildTool::default());
    let runner = Arc::new(ChildToolRunner { child_tool });
    let mut registry = CapabilityRegistry::default();
    registry.install::<dyn SubagentRunnerCap>(
        ToolCapability::SubagentRunner,
        SubagentRunnerCapAdapter::from_runner(runner),
    );

    let parent_tool = AgentTool::default();
    let parent_ctx = harness_subagent::testing::tool_context_with_caps(Arc::new(registry));
    let parent_session_id = parent_ctx.session_id;
    let stream = parent_tool
        .execute(
            json!({
                "role": "reviewer",
                "task": "call child tool"
            }),
            parent_ctx,
        )
        .await
        .unwrap();

    let events: Vec<_> = stream.collect().await;
    let ToolEvent::Final(ToolResult::Structured(announcement)) = events.last().unwrap() else {
        panic!("agent tool should finish with a structured announcement");
    };

    assert_eq!(announcement["status"], "completed");
    assert_eq!(announcement["summary"], "child tool finished for reviewer");
    assert_eq!(announcement["result"]["child_tool"], "called");
    assert_eq!(announcement["result"]["task"], "call child tool");
    assert_eq!(
        announcement["result"]["parent_session_id"],
        parent_session_id.to_string()
    );
}

struct ChildToolRunner {
    child_tool: Arc<ChildTool>,
}

#[async_trait]
impl SubagentRunner for ChildToolRunner {
    async fn spawn(
        &self,
        spec: SubagentSpec,
        _input: harness_contracts::TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, harness_subagent::SubagentError> {
        let child_ctx = harness_subagent::testing::tool_context_with_caps(Arc::new(
            CapabilityRegistry::default(),
        ));
        let stream = self
            .child_tool
            .execute(json!({ "task": spec.task }), child_ctx)
            .await
            .map_err(|error| harness_subagent::SubagentError::Engine(error.to_string()))?;
        let events: Vec<_> = stream.collect().await;
        let Some(ToolEvent::Final(ToolResult::Structured(result))) = events.last() else {
            return Err(harness_subagent::SubagentError::Engine(
                "child tool did not return structured output".to_owned(),
            ));
        };

        Ok(SubagentHandle::ready(SubagentAnnouncement {
            subagent_id: harness_contracts::SubagentId::new(),
            parent_session_id: parent_ctx.parent_session_id,
            status: SubagentStatus::Completed,
            summary: format!("child tool finished for {}", spec.role),
            result: Some(json!({
                "child_tool": result["child_tool"],
                "task": result["task"],
                "parent_session_id": parent_ctx.parent_session_id.to_string()
            })),
            usage: UsageSnapshot::default(),
            transcript_ref: None,
            context_report: None,
        }))
    }
}

struct ChildTool {
    descriptor: ToolDescriptor,
}

impl Default for ChildTool {
    fn default() -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: "child_echo".to_owned(),
                display_name: "Child Echo".to_owned(),
                description: "Test child tool used by subagent e2e.".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::Custom("test".to_owned()),
                version: "0.1.0".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["task"],
                    "properties": {
                        "task": { "type": "string" }
                    }
                }),
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
                    limit: 1_000,
                    on_overflow: OverflowAction::Truncate,
                    preview_head_chars: 500,
                    preview_tail_chars: 500,
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
impl Tool for ChildTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        if input.get("task").and_then(Value::as_str).is_some() {
            Ok(())
        } else {
            Err(ValidationError::Message("task is required".to_owned()))
        }
    }

    async fn check_permission(&self, _input: &Value, _ctx: &ToolContext) -> PermissionCheck {
        PermissionCheck::Allowed
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolContext,
    ) -> Result<ToolStream, harness_contracts::ToolError> {
        let task = input
            .get("task")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        Ok(Box::pin(futures::stream::iter([ToolEvent::Final(
            ToolResult::Structured(json!({
                "child_tool": "called",
                "task": task
            })),
        )])))
    }
}

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    DeferPolicy, ProviderRestriction, ToolDescriptor, ToolError, ToolGroup, ToolOrigin,
    ToolProperties, ToolResult, TrustLevel,
};
use harness_permission::PermissionCheck;
use harness_tool::{
    default_result_budget, BuiltinToolset, RegistrationError, Tool, ToolContext, ToolEvent,
    ToolRegistry, ValidationError,
};
use serde_json::{json, Value};

#[test]
fn rejects_non_canonical_tool_names() {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .build()
        .unwrap();

    for name in ["", "bad name", "bad__reserved"] {
        let error = registry
            .register(Box::new(TestTool {
                descriptor: descriptor(name),
            }))
            .unwrap_err();
        assert!(
            matches!(error, RegistrationError::InvalidDescriptor(_)),
            "expected InvalidDescriptor for {name:?}, got {error:?}"
        );
    }

    registry
        .register(Box::new(TestTool {
            descriptor: descriptor("mcp__server__tool"),
        }))
        .expect("canonical MCP tool name should stay valid");
}

struct TestTool {
    descriptor: ToolDescriptor,
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
        input: Value,
        _ctx: ToolContext,
    ) -> Result<harness_tool::ToolStream, ToolError> {
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(input),
        )])))
    }
}

fn descriptor(name: &str) -> ToolDescriptor {
    ToolDescriptor {
        name: name.to_owned(),
        display_name: "test".to_owned(),
        description: "test".to_owned(),
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
            defer_policy: DeferPolicy::AlwaysLoad,
        },
        trust_level: TrustLevel::AdminTrusted,
        required_capabilities: Vec::new(),
        budget: default_result_budget(),
        provider_restriction: ProviderRestriction::All,
        origin: ToolOrigin::Builtin,
        search_hint: None,
    }
}

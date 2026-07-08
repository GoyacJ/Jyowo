#![cfg(feature = "server-adapter")]

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    BudgetMetric, CapabilityRegistry, DecisionScope, DeferPolicy, NetworkAccess, OverflowAction,
    PermissionSubject, ProviderRestriction, ResultBudget, SemverString, SessionId, TenantId,
    ToolActionPlan, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup, ToolOrigin,
    ToolProperties, ToolResult, ToolResultPart, ToolUseId, TrustLevel, WorkspaceAccess,
};
use harness_mcp::{
    JsonRpcRequest, JsonRpcResponse, McpServerAdapter, McpToolResult, StaticToolContextFactory,
};
use harness_tool::{
    action_plan_from_permission_check, AuthorizedToolInput, BuiltinToolset, InterruptToken,
    PermissionCheck, Tool, ToolContext, ToolEvent, ToolRegistry, ToolStream, ValidationError,
};
use serde_json::{json, Value};

#[tokio::test]
async fn server_initialize_returns_capabilities() {
    let server = adapter_with(vec![test_tool("echo", Behavior::Text("ok".into()))]);

    let response = server
        .handle_request(JsonRpcRequest::new(json!(1), "initialize", Some(json!({}))))
        .await;

    let result = expect_result(response);
    assert_eq!(result["protocolVersion"], "2025-03-26");
    assert_eq!(result["serverInfo"]["name"], "jyowo-harness-mcp");
    assert!(result["capabilities"]["tools"].is_object());
}

#[tokio::test]
async fn server_lists_registered_tools() {
    let server = adapter_with(vec![test_tool("echo", Behavior::Text("ok".into()))]);

    let response = server
        .handle_request(JsonRpcRequest::new(json!(2), "tools/list", Some(json!({}))))
        .await;

    let result = expect_result(response);
    let tools = result["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "echo");
    assert_eq!(tools[0]["description"], "echo tool");
    assert_eq!(tools[0]["inputSchema"]["type"], "object");
    assert_eq!(tools[0]["outputSchema"]["type"], "object");
}

#[tokio::test]
async fn server_calls_tool_and_maps_results() {
    let server = adapter_with(vec![
        test_tool("echo", Behavior::Text("hello".into())),
        test_tool("json", Behavior::Structured(json!({ "ok": true }))),
        test_tool(
            "mixed",
            Behavior::Mixed(vec![
                ToolResultPart::Text {
                    text: "head".into(),
                },
                ToolResultPart::Structured {
                    value: json!({ "n": 1 }),
                    schema_ref: None,
                },
            ]),
        ),
    ]);

    let text = call_tool(&server, "echo", json!({})).await;
    assert_eq!(text, McpToolResult::text("hello"));

    let structured = call_tool(&server, "json", json!({})).await;
    assert_eq!(
        structured.content[0],
        harness_mcp::McpContent::Json {
            value: json!({ "ok": true })
        }
    );

    let mixed = call_tool(&server, "mixed", json!({})).await;
    assert_eq!(mixed.content.len(), 2);
}

#[tokio::test]
async fn server_maps_typed_artifact_tool_results() {
    let server = adapter_with(vec![test_tool(
        "artifact",
        Behavior::Mixed(vec![ToolResultPart::Artifact {
            artifact_kind: harness_contracts::ModelModality::Image,
            content_type: "image/png".to_owned(),
            blob_ref: harness_contracts::BlobRef {
                id: harness_contracts::BlobId::new(),
                size: 128,
                content_hash: [3; 32],
                content_type: Some("image/png".to_owned()),
            },
            title: "Generated image".to_owned(),
            preview: Some("Generated image".to_owned()),
        }]),
    )]);

    let result = call_tool(&server, "artifact", json!({})).await;
    assert_eq!(result.content.len(), 1);
    assert!(matches!(
        &result.content[0],
        harness_mcp::McpContent::Json { value }
            if value["kind"] == "artifact"
                && value["title"] == "Generated image"
                && value["contentType"] == "image/png"
                && value.get("blobRef").is_none()
    ));
}

#[tokio::test]
async fn server_maps_validation_and_permission_failures_to_tool_errors() {
    let validate_server = adapter_with(vec![test_tool("bad_input", Behavior::ValidateError)]);
    let validate_result = call_tool(&validate_server, "bad_input", json!({})).await;
    assert!(validate_result.is_error);
    assert!(text_content(&validate_result).contains("validation"));

    let permission_server = adapter_with(vec![test_tool("ask", Behavior::AskPermission)]);
    let permission_result = call_tool(&permission_server, "ask", json!({})).await;
    assert!(permission_result.is_error);
    assert!(text_content(&permission_result).contains("permission"));

    let dangerous_server = adapter_with(vec![test_tool("danger", Behavior::DangerousPattern)]);
    let dangerous_result = call_tool(&dangerous_server, "danger", json!({})).await;
    assert!(dangerous_result.is_error);
    let dangerous_text = text_content(&dangerous_result);
    assert!(dangerous_text.contains("permission"));
    assert!(dangerous_text.contains("dangerous pattern"));
}

#[tokio::test]
async fn server_returns_jsonrpc_errors_for_bad_requests() {
    let server = adapter_with(vec![test_tool("echo", Behavior::Text("ok".into()))]);

    let unknown_tool = server
        .handle_request(JsonRpcRequest::new(
            json!(10),
            "tools/call",
            Some(json!({ "name": "missing", "arguments": {} })),
        ))
        .await;
    assert_eq!(expect_error_code(unknown_tool), -32602);

    let unknown_method = server
        .handle_request(JsonRpcRequest::new(json!(11), "unknown/method", None))
        .await;
    assert_eq!(expect_error_code(unknown_method), -32601);
}

#[tokio::test]
async fn server_returns_empty_resource_and_prompt_lists() {
    let server = adapter_with(vec![]);

    let resources = expect_result(
        server
            .handle_request(JsonRpcRequest::new(
                json!(20),
                "resources/list",
                Some(json!({})),
            ))
            .await,
    );
    assert_eq!(resources, json!({ "resources": [] }));

    let prompts = expect_result(
        server
            .handle_request(JsonRpcRequest::new(
                json!(21),
                "prompts/list",
                Some(json!({})),
            ))
            .await,
    );
    assert_eq!(prompts, json!({ "prompts": [] }));
}

fn adapter_with(tools: Vec<TestTool>) -> McpServerAdapter {
    let registry = tools
        .into_iter()
        .fold(
            ToolRegistry::builder().with_builtin_toolset(BuiltinToolset::Empty),
            |builder, tool| builder.with_tool(Box::new(tool)),
        )
        .build()
        .expect("registry");
    McpServerAdapter::builder(registry)
        .with_tool_context_factory(StaticToolContextFactory::new(tool_context()))
        .build()
        .expect("server adapter")
}

async fn call_tool(server: &McpServerAdapter, name: &str, arguments: Value) -> McpToolResult {
    let response = server
        .handle_request(JsonRpcRequest::new(
            json!(3),
            "tools/call",
            Some(json!({ "name": name, "arguments": arguments })),
        ))
        .await;
    serde_json::from_value(expect_result(response)).expect("mcp tool result")
}

fn expect_result(response: JsonRpcResponse) -> Value {
    assert!(
        response.error.is_none(),
        "unexpected error: {:?}",
        response.error
    );
    response.result.expect("result")
}

fn expect_error_code(response: JsonRpcResponse) -> i32 {
    response.error.expect("error").code
}

fn text_content(result: &McpToolResult) -> String {
    result
        .content
        .iter()
        .find_map(|content| match content {
            harness_mcp::McpContent::Text { text } => Some(text.clone()),
            harness_mcp::McpContent::Json { .. } => None,
        })
        .unwrap_or_default()
}

fn test_tool(name: &str, behavior: Behavior) -> TestTool {
    TestTool {
        descriptor: ToolDescriptor {
            name: name.to_owned(),
            display_name: name.to_owned(),
            description: format!("{name} tool"),
            category: "test".to_owned(),
            group: ToolGroup::Custom("test".to_owned()),
            version: SemverString::from("0.1.0"),
            input_schema: json!({ "type": "object" }),
            output_schema: Some(json!({ "type": "object" })),
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
                limit: 10_000,
                on_overflow: OverflowAction::Truncate,
                preview_head_chars: 1_000,
                preview_tail_chars: 200,
            },
            provider_restriction: ProviderRestriction::All,
            origin: ToolOrigin::Builtin,
            search_hint: None,
            service_binding: None,
        },
        behavior,
    }
}

#[derive(Clone)]
struct TestTool {
    descriptor: ToolDescriptor,
    behavior: Behavior,
}

#[derive(Clone)]
enum Behavior {
    Text(String),
    Structured(Value),
    Mixed(Vec<ToolResultPart>),
    ValidateError,
    AskPermission,
    DangerousPattern,
}

#[async_trait]
impl Tool for TestTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        if matches!(self.behavior, Behavior::ValidateError) {
            Err(ValidationError::from("invalid input"))
        } else {
            Ok(())
        }
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let check = if matches!(self.behavior, Behavior::AskPermission) {
            PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                scope: DecisionScope::ToolName(self.descriptor.name.clone()),
            }
        } else if matches!(self.behavior, Behavior::DangerousPattern) {
            PermissionCheck::DangerousPattern {
                kind: "url".to_owned(),
                pattern: "url-cloud-metadata".to_owned(),
                severity: harness_contracts::Severity::High,
                subject: PermissionSubject::NetworkAccess {
                    host: "169.254.169.254".to_owned(),
                    port: None,
                },
                scope: DecisionScope::Category("network".to_owned()),
            }
        } else {
            PermissionCheck::Allowed
        };
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            check,
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
        let result = match &self.behavior {
            Behavior::Text(text) => ToolResult::Text(text.clone()),
            Behavior::Structured(value) => ToolResult::Structured(value.clone()),
            Behavior::Mixed(parts) => ToolResult::Mixed(parts.clone()),
            Behavior::ValidateError | Behavior::AskPermission | Behavior::DangerousPattern => {
                ToolResult::Text("not executed".into())
            }
        };
        Ok(Box::pin(stream::iter([ToolEvent::Final(result)])))
    }
}

fn tool_context() -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: std::path::PathBuf::from("."),
        project_workspace_root: None,
        sandbox: None,
        cap_registry: Arc::new(CapabilityRegistry::default()),
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::new(),
        parent_run: None,
        model: None,
        model_config_id: None,
        memory_thread_settings: None,
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
    }
}

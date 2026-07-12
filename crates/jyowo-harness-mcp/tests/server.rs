#![cfg(feature = "server-adapter")]

use std::{path::PathBuf, sync::Arc};

#[cfg(unix)]
use std::{ffi::OsString, os::unix::ffi::OsStringExt};

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    BudgetMetric, CapabilityRegistry, DecisionScope, DeferPolicy, NetworkAccess, OverflowAction,
    PermissionSubject, ProviderRestriction, ResultBudget, SemverString, SessionId, Severity,
    TenantId, ToolActionPlan, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup,
    ToolOrigin, ToolProperties, ToolResult, ToolResultPart, ToolUseId, TrustLevel, WorkspaceAccess,
};
use harness_mcp::{
    JsonRpcRequest, JsonRpcResponse, McpServerAdapter, McpToolResult, StaticToolContextFactory,
    ToolCallAuthorizer,
};
use harness_tool::{
    action_plan_from_permission_check, AuthorizationTicketClaims, AuthorizedTicketSummary,
    AuthorizedToolInput, BuiltinToolset, InterruptToken, PermissionCheck, TicketLedger, Tool,
    ToolContext, ToolEvent, ToolRegistry, ToolStream, ValidationError,
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
    let mut tool = test_tool("echo", Behavior::Text("ok".into()));
    tool.descriptor.output_schema = Some(json!({ "type": "object" }));
    let server = adapter_with(vec![tool]);

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
            "scalar",
            Behavior::Structured(json!(["not", "an", "object"])),
        ),
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
        structured
            .structured_content
            .as_ref()
            .and_then(|content| content.get("ok")),
        Some(&json!(true))
    );
    assert!(text_content(&structured).contains("\"ok\": true"));

    let mixed = call_tool(&server, "mixed", json!({})).await;
    assert!(matches!(
        mixed.content.first(),
        Some(harness_mcp::McpContent::Text { text, .. }) if text == "head"
    ));
    assert_eq!(
        mixed.structured_content,
        json!({ "n": 1 }).as_object().cloned()
    );
    let mixed_json = serde_json::to_string(&mixed).unwrap();
    assert!(!mixed_json.contains("\"mixed\""));
    assert!(!mixed_json.contains("\"kind\""));

    let scalar = call_tool(&server, "scalar", json!({})).await;
    assert!(scalar.structured_content.is_none());
    assert!(text_content(&scalar).contains("not"));
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
    assert!(result.structured_content.is_none());
    assert!(text_content(&result).contains("Generated image"));
    let result_json = serde_json::to_string(&result).unwrap();
    assert!(!result_json.contains("artifact_kind"));
    assert!(!result_json.contains("blob_ref"));
}

#[tokio::test]
async fn server_preserves_non_url_reference_identities_without_labels() {
    let tool_use_id = ToolUseId::from_u128(11);
    let memory_id = harness_contracts::MemoryId::from_u128(12);
    let mut tool = test_tool(
        "references",
        Behavior::Mixed(vec![
            ToolResultPart::Reference {
                reference_kind: harness_contracts::ReferenceKind::File {
                    path: PathBuf::from("/workspace/src/main.rs"),
                    line_range: Some((12, 34)),
                },
                title: None,
                summary: None,
            },
            ToolResultPart::Reference {
                reference_kind: harness_contracts::ReferenceKind::ToolUse { tool_use_id },
                title: None,
                summary: None,
            },
            ToolResultPart::Reference {
                reference_kind: harness_contracts::ReferenceKind::Memory { memory_id },
                title: None,
                summary: None,
            },
        ]),
    );
    tool.descriptor.output_schema = None;
    let server = adapter_with(vec![tool]);

    let result = call_tool(&server, "references", json!({})).await;
    let texts = text_contents(&result);

    assert_eq!(
        texts,
        [
            "file:/workspace/src/main.rs#L12-L34".to_owned(),
            format!("tool-use:{tool_use_id}"),
            format!("memory:{memory_id}"),
        ]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn server_preserves_non_utf8_file_reference_identity() {
    let path = PathBuf::from(OsString::from_vec(b"/workspace/src/\x80.rs".to_vec()));
    let mut tool = test_tool(
        "non_utf8_reference",
        Behavior::Mixed(vec![ToolResultPart::Reference {
            reference_kind: harness_contracts::ReferenceKind::File {
                path,
                line_range: None,
            },
            title: None,
            summary: None,
        }]),
    );
    tool.descriptor.output_schema = None;
    let server = adapter_with(vec![tool]);

    let result = call_tool(&server, "non_utf8_reference", json!({})).await;

    assert_eq!(text_contents(&result), ["file:/workspace/src/%80.rs"]);
}

#[tokio::test]
async fn server_rejects_multiple_structured_objects_against_single_root_schema() {
    let output_schema = json!({
        "type": "object",
        "properties": { "n": { "type": "integer" } },
        "required": ["n"],
        "additionalProperties": false
    });
    let mut tool = test_tool(
        "multi_structured",
        Behavior::Mixed(vec![
            ToolResultPart::Structured {
                value: json!({ "n": 1 }),
                schema_ref: None,
            },
            ToolResultPart::Structured {
                value: json!({ "other": true }),
                schema_ref: None,
            },
        ]),
    );
    tool.descriptor.output_schema = Some(output_schema.clone());
    let server = adapter_with(vec![tool]);

    let listed = server
        .handle_request(JsonRpcRequest::new(json!(4), "tools/list", Some(json!({}))))
        .await;
    let listed = expect_result(listed);
    assert_eq!(listed["tools"][0]["outputSchema"], output_schema);
    assert!(listed["tools"][0]["outputSchema"].get("parts").is_none());

    let result = call_tool(&server, "multi_structured", json!({})).await;
    assert!(result.structured_content.is_none());
    assert!(result.is_error);
    let texts = text_contents(&result);
    assert_eq!(texts.len(), 3);
    assert!(texts[0].contains("\"n\": 1"));
    assert!(texts[1].contains("\"other\": true"));
    assert!(texts[2].contains("expected exactly one structured object, got 2"));
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("\"parts\""));
}

#[tokio::test]
async fn server_rejects_structured_object_that_violates_output_schema() {
    let mut tool = test_tool(
        "invalid_structured",
        Behavior::Structured(json!({ "other": true })),
    );
    tool.descriptor.output_schema = Some(json!({
        "type": "object",
        "properties": { "n": { "type": "integer" } },
        "required": ["n"],
        "additionalProperties": false
    }));
    let server = adapter_with(vec![tool]);

    let result = call_tool(&server, "invalid_structured", json!({})).await;

    assert!(result.structured_content.is_none());
    assert!(result.is_error);
    let texts = text_contents(&result);
    assert_eq!(texts.len(), 2);
    assert!(texts[0].contains("\"other\": true"));
    assert!(texts[1].contains("output schema validation failed"));
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
        .with_tool_authorizer(TestToolCallAuthorizer)
        .build()
        .expect("server adapter")
}

struct TestToolCallAuthorizer;

#[async_trait]
impl ToolCallAuthorizer for TestToolCallAuthorizer {
    async fn authorize_tool_call(
        &self,
        raw_input: Value,
        action_plan: ToolActionPlan,
        _context: &ToolContext,
    ) -> Result<AuthorizedToolInput, ToolError> {
        if action_plan.severity != Severity::Info {
            let reason = if action_plan.severity == Severity::High {
                "dangerous pattern approval is unavailable in this test"
            } else {
                "permission approval is unavailable in this test"
            };
            return Err(ToolError::PermissionDenied(reason.to_owned()));
        }
        let ticket = ticket_for(&action_plan);
        AuthorizedToolInput::new(raw_input, action_plan, ticket)
    }
}

fn ticket_for(plan: &ToolActionPlan) -> AuthorizedTicketSummary {
    let ledger = TicketLedger::default();
    let claims = AuthorizationTicketClaims {
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        run_id: harness_contracts::RunId::new(),
        tool_use_id: plan.tool_use_id,
        tool_name: plan.tool_name.clone(),
        action_plan_hash: plan.plan_hash.clone(),
    };
    let ticket = ledger
        .mint(claims.clone(), chrono::Utc::now())
        .expect("test ticket should mint");
    ledger
        .consume(ticket.id, &claims, chrono::Utc::now())
        .expect("test ticket should consume")
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
            harness_mcp::McpContent::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn text_contents(result: &McpToolResult) -> Vec<String> {
    result
        .content
        .iter()
        .filter_map(|content| match content {
            harness_mcp::McpContent::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
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
                limit: 10_000,
                on_overflow: OverflowAction::Truncate,
                preview_head_chars: 1_000,
                preview_tail_chars: 200,
            },
            provider_restriction: ProviderRestriction::All,
            origin: ToolOrigin::Builtin,
            search_hint: None,
            service_binding: None,
            metadata: Default::default(),
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

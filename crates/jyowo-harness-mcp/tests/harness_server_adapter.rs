#![cfg(feature = "server-adapter")]
#![allow(clippy::field_reassign_with_default)]

use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::TenantId;
use harness_mcp::{
    ExposedCapabilities, ExposedCapability, HarnessMcpBackend, HarnessMcpServer, JsonRpcRequest,
    McpServerError, McpServerPolicy, McpServerRateLimit, McpServerRequestContext, McpToolResult,
    TenantMapping,
};
use parking_lot::Mutex;
use serde_json::{json, Value};

#[derive(Default)]
struct FakeBackend {
    calls: Mutex<Vec<(TenantId, ExposedCapability, Value)>>,
}

#[async_trait]
impl HarnessMcpBackend for FakeBackend {
    async fn call_harness_tool(
        &self,
        context: &McpServerRequestContext,
        capability: ExposedCapability,
        arguments: Value,
    ) -> Result<Value, McpServerError> {
        self.calls
            .lock()
            .push((context.tenant_id, capability, arguments));
        Ok(json!({
            "capability": format!("{capability:?}"),
            "tenant_id": context.tenant_id,
        }))
    }
}

#[tokio::test]
async fn harness_server_default_policy_lists_read_only_tools() {
    let server = HarnessMcpServer::new(Arc::new(FakeBackend::default()))
        .build()
        .expect("server");

    let response = server
        .handle_request(JsonRpcRequest::new(json!(1), "tools/list", Some(json!({}))))
        .await;

    let tools = response.result.expect("result")["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("name").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        tools,
        vec![
            "sessions_list",
            "session_get",
            "messages_read",
            "attachments_fetch",
            "events_poll",
            "events_wait",
            "permissions_list_open",
            "channels_list",
        ]
    );
}

#[tokio::test]
async fn harness_server_permissions_respond_schema_requires_backend_option_and_session() {
    let mut policy = McpServerPolicy::default();
    policy.exposed_capabilities.permissions_respond = true;
    let server = HarnessMcpServer::new(Arc::new(FakeBackend::default()))
        .with_policy(policy)
        .build()
        .expect("server");

    let response = server
        .handle_request(JsonRpcRequest::new(
            json!(11),
            "tools/list",
            Some(json!({})),
        ))
        .await;

    let tools = response.result.expect("result")["tools"]
        .as_array()
        .expect("tools")
        .clone();
    let tool = tools
        .iter()
        .find(|tool| tool["name"] == "permissions_respond")
        .expect("permissions_respond tool");
    let schema = &tool["inputSchema"];
    assert_eq!(
        schema["required"],
        json!(["session_id", "request_id", "option_id", "decision"])
    );
    assert_eq!(
        schema["properties"]["decision"]["enum"],
        json!(["allow_once", "deny_once"])
    );
    assert!(schema["properties"].get("option_id").is_some());
}

#[tokio::test]
async fn harness_server_routes_tool_calls_to_backend_with_resolved_tenant() {
    let backend = Arc::new(FakeBackend::default());
    let mut policy = McpServerPolicy::default();
    policy.tenant_mapping = TenantMapping::Header("x-tenant-id".to_owned());
    let server = HarnessMcpServer::new(Arc::clone(&backend))
        .with_policy(policy)
        .build()
        .expect("server");
    let tenant = TenantId::new();

    let response = server
        .handle_request_with_context(
            JsonRpcRequest::new(
                json!(2),
                "tools/call",
                Some(json!({
                    "name": "messages_read",
                    "arguments": { "session_id": "01HF7YAT00TEST000000000000" }
                })),
            ),
            McpServerRequestContext::default().with_header("x-tenant-id", tenant.to_string()),
        )
        .await;

    let result: McpToolResult =
        serde_json::from_value(response.result.expect("result")).expect("tool result");
    assert!(!result.is_error);
    let calls = backend.calls.lock();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, tenant);
    assert_eq!(calls[0].1, ExposedCapability::MessagesRead);
}

#[tokio::test]
async fn harness_server_routes_each_9_plus_1_tool_over_jsonrpc() {
    let backend = Arc::new(FakeBackend::default());
    let mut policy = McpServerPolicy::default();
    policy.exposed_capabilities.messages_send = true;
    policy.exposed_capabilities.permissions_respond = true;
    let server = HarnessMcpServer::new(Arc::clone(&backend))
        .with_policy(policy)
        .build()
        .expect("server");
    let calls = [
        ("sessions_list", json!({})),
        (
            "session_get",
            json!({"session_id": "01HF7YAT00TEST000000000000"}),
        ),
        (
            "messages_read",
            json!({"session_id": "01HF7YAT00TEST000000000000"}),
        ),
        (
            "messages_send",
            json!({"session_id": "01HF7YAT00TEST000000000000", "message": "hello"}),
        ),
        (
            "attachments_fetch",
            json!({"blob_ref": {"id": "01HF7YAT00TEST000000000001", "size": 0, "content_hash": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0], "content_type": null}}),
        ),
        ("events_poll", json!({})),
        ("events_wait", json!({"timeout_ms": 0})),
        ("permissions_list_open", json!({})),
        (
            "permissions_respond",
            json!({
                "session_id": "01HF7YAT00TEST000000000000",
                "request_id": "01HF7YAT00TEST000000000002",
                "option_id": "allow-once",
                "decision": "allow_once",
            }),
        ),
        ("channels_list", json!({})),
    ];

    for (idx, (name, arguments)) in calls.into_iter().enumerate() {
        let response = server
            .handle_request(JsonRpcRequest::new(
                json!(100 + idx),
                "tools/call",
                Some(json!({ "name": name, "arguments": arguments })),
            ))
            .await;
        assert!(response.error.is_none(), "{name} should route");
    }

    assert_eq!(backend.calls.lock().len(), 10);
}

#[tokio::test]
async fn harness_server_hides_disabled_capabilities() {
    let mut policy = McpServerPolicy::default();
    policy.exposed_capabilities = ExposedCapabilities {
        messages_send: false,
        ..ExposedCapabilities::default()
    };
    let server = HarnessMcpServer::new(Arc::new(FakeBackend::default()))
        .with_policy(policy)
        .build()
        .expect("server");

    let listed = server
        .handle_request(JsonRpcRequest::new(json!(3), "tools/list", Some(json!({}))))
        .await
        .result
        .expect("result");
    assert!(!listed["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .any(|tool| tool["name"] == "messages_send"));

    let called = server
        .handle_request(JsonRpcRequest::new(
            json!(4),
            "tools/call",
            Some(json!({ "name": "messages_send", "arguments": {} })),
        ))
        .await;
    assert_eq!(called.error.expect("error").code, -32602);
}

#[tokio::test]
async fn harness_server_rate_limit_keeps_tenant_counters_separate() {
    let mut policy = McpServerPolicy::default();
    policy.tenant_mapping = TenantMapping::Header("x-tenant-id".to_owned());
    policy.rate_limit = McpServerRateLimit {
        global_rps: 0,
        per_tenant_rps: 1,
        per_capability_rps: Default::default(),
        burst: 1,
        audit_throttle: true,
    };
    let server = HarnessMcpServer::new(Arc::new(FakeBackend::default()))
        .with_policy(policy)
        .build()
        .expect("server");
    let first_tenant = TenantId::new();
    let second_tenant = TenantId::new();

    let first = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(7), "tools/list", Some(json!({}))),
            McpServerRequestContext::default().with_header("x-tenant-id", first_tenant.to_string()),
        )
        .await;
    let second_same_tenant = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(8), "tools/list", Some(json!({}))),
            McpServerRequestContext::default().with_header("x-tenant-id", first_tenant.to_string()),
        )
        .await;
    let first_other_tenant = server
        .handle_request_with_context(
            JsonRpcRequest::new(json!(9), "tools/list", Some(json!({}))),
            McpServerRequestContext::default()
                .with_header("x-tenant-id", second_tenant.to_string()),
        )
        .await;

    assert!(first.error.is_none());
    assert_eq!(second_same_tenant.error.expect("error").code, -32029);
    assert!(first_other_tenant.error.is_none());
}

#[tokio::test]
async fn harness_server_rate_limit_keeps_capability_counters_separate() {
    let mut per_capability_rps = std::collections::BTreeMap::new();
    per_capability_rps.insert(ExposedCapability::MessagesRead, 1);
    let mut policy = McpServerPolicy::default();
    policy.rate_limit = McpServerRateLimit {
        global_rps: 0,
        per_tenant_rps: 0,
        per_capability_rps,
        burst: 1,
        audit_throttle: true,
    };
    let server = HarnessMcpServer::new(Arc::new(FakeBackend::default()))
        .with_policy(policy)
        .build()
        .expect("server");

    let first = call_tool(&server, 10, "messages_read").await;
    let second_same_capability = call_tool(&server, 11, "messages_read").await;
    let other_capability = call_tool(&server, 12, "channels_list").await;

    assert!(first.error.is_none());
    assert_eq!(second_same_capability.error.expect("error").code, -32029);
    assert!(other_capability.error.is_none());
}

#[tokio::test]
async fn harness_server_rate_limit_returns_retry_after_metadata() {
    let mut policy = McpServerPolicy::default();
    policy.rate_limit = McpServerRateLimit {
        global_rps: 1,
        per_tenant_rps: 0,
        per_capability_rps: Default::default(),
        burst: 1,
        audit_throttle: true,
    };
    let server = HarnessMcpServer::new(Arc::new(FakeBackend::default()))
        .with_policy(policy)
        .build()
        .expect("server");

    let first = server
        .handle_request(JsonRpcRequest::new(json!(5), "tools/list", Some(json!({}))))
        .await;
    assert!(first.error.is_none());

    let second = server
        .handle_request(JsonRpcRequest::new(json!(6), "tools/list", Some(json!({}))))
        .await;
    let error = second.error.expect("error");
    assert_eq!(error.code, -32029);
    assert!(
        error.data.expect("data")["retry_after_ms"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
}

async fn call_tool(
    server: &HarnessMcpServer<FakeBackend>,
    id: u64,
    name: &str,
) -> harness_mcp::JsonRpcResponse {
    let arguments = match name {
        "messages_read" => json!({"session_id": "01HF7YAT00TEST000000000000"}),
        _ => json!({}),
    };
    server
        .handle_request(JsonRpcRequest::new(
            json!(id),
            "tools/call",
            Some(json!({ "name": name, "arguments": arguments })),
        ))
        .await
}

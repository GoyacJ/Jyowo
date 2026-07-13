#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
use std::sync::Arc;

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
use serde::de::DeserializeOwned;
#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
use serde::Deserialize;
#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
use serde_json::{json, Value};

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
use crate::{
    url_elicitations_from_jsonrpc_error, ElicitationClientCapability, ElicitationJsonRpcHandler,
    ElicitationRequestRouter, EmptyClientCapability, JsonRpcResponse, McpClientCapabilities,
    McpConnectContext, McpError, McpListPage, McpPrompt, McpPromptMessages, McpReadResourceResult,
    McpResource, McpResourceContents, McpServerSpec, McpToolDescriptor, McpToolResult,
    SamplingClientCapability, SamplingJsonRpcHandler, SamplingRequestRouter,
};

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
use crate::JsonRpcRequest;

#[cfg(feature = "http")]
mod http;
#[cfg(feature = "in-process")]
mod in_process;
#[cfg(any(feature = "http", feature = "sse", feature = "websocket"))]
mod network_endpoint;
#[cfg(any(feature = "http", feature = "sse"))]
mod sse;
#[cfg(any(feature = "http", feature = "sse"))]
mod sse_codec;
#[cfg(feature = "stdio")]
mod stdio;
#[cfg(any(feature = "http", feature = "sse"))]
mod streamable_http;
#[cfg(feature = "websocket")]
mod websocket;

#[cfg(feature = "http")]
pub use http::HttpTransport;
#[cfg(feature = "in-process")]
pub use in_process::InProcessTransport;
#[cfg(any(feature = "http", feature = "sse"))]
pub use sse::SseTransport;
#[cfg(any(feature = "http", feature = "sse"))]
pub use sse_codec::{SseDecoder, SseEvent, SseLimits};
#[cfg(feature = "stdio")]
pub use stdio::StdioTransport;
#[cfg(feature = "websocket")]
pub use websocket::WebsocketTransport;

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
#[derive(Debug, Deserialize)]
struct ListToolsResult {
    tools: Vec<McpToolDescriptor>,
    #[serde(rename = "nextCursor", default)]
    next_cursor: Option<String>,
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
#[derive(Debug, Deserialize)]
struct ListResourcesResult {
    resources: Vec<McpResource>,
    #[serde(rename = "nextCursor", default)]
    next_cursor: Option<String>,
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
#[derive(Debug, Deserialize)]
struct ReadResourceResult {
    contents: Vec<McpResourceContents>,
    #[serde(rename = "_meta", default)]
    meta: std::collections::BTreeMap<String, Value>,
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
#[derive(Debug, Deserialize)]
struct ListPromptsResult {
    prompts: Vec<McpPrompt>,
    #[serde(rename = "nextCursor", default)]
    next_cursor: Option<String>,
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) struct JsonRpcPeer {
    next_id: AtomicU64,
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
impl JsonRpcPeer {
    pub(crate) fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
        }
    }

    pub(crate) fn request(&self, method: &str, params: Option<Value>) -> JsonRpcRequest {
        JsonRpcRequest::new(
            json!(self.next_id.fetch_add(1, Ordering::SeqCst)),
            method,
            params,
        )
    }
}

#[cfg(all(
    test,
    any(
        feature = "stdio",
        feature = "http",
        feature = "websocket",
        feature = "sse"
    )
))]
mod lifecycle_compatibility_tests {
    use super::*;

    #[cfg(any(
        feature = "stdio",
        feature = "http",
        feature = "websocket",
        feature = "sse"
    ))]
    #[test]
    fn paginated_requests_only_send_a_cursor_when_present() {
        let peer = JsonRpcPeer::new();
        assert_eq!(list_tools_request(&peer, None).params, None);
        assert_eq!(
            list_resources_request(&peer, Some("resources-2")).params,
            Some(json!({ "cursor": "resources-2" }))
        );
        assert_eq!(
            list_prompts_request(&peer, Some("prompts-2")).params,
            Some(json!({ "cursor": "prompts-2" }))
        );
    }

    #[test]
    fn response_decoders_preserve_cursors_and_all_resource_contents() {
        let tools = decode_list_tools(JsonRpcResponse::success(
            json!(1),
            json!({ "tools": [], "nextCursor": "tools-2" }),
        ))
        .unwrap();
        assert_eq!(tools.next_cursor.as_deref(), Some("tools-2"));

        let resource = decode_read_resource(JsonRpcResponse::success(
            json!(2),
            json!({
                "contents": [
                    { "uri": "test://text", "text": "hello" },
                    { "uri": "test://blob", "blob": "AA==" }
                ],
                "_meta": { "trace": "abc" }
            }),
        ))
        .unwrap();
        assert_eq!(resource.contents.len(), 2);
        assert!(matches!(
            &resource.contents[1],
            McpResourceContents::Blob { blob, .. } if blob == "AA=="
        ));
        assert_eq!(resource.meta.get("trace"), Some(&json!("abc")));
    }
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn list_tools_request(peer: &JsonRpcPeer, cursor: Option<&str>) -> JsonRpcRequest {
    peer.request("tools/list", pagination_params(cursor))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn call_tool_request(peer: &JsonRpcPeer, name: &str, args: Value) -> JsonRpcRequest {
    peer.request("tools/call", call_tool_params(name, args))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn list_resources_request(peer: &JsonRpcPeer, cursor: Option<&str>) -> JsonRpcRequest {
    peer.request("resources/list", pagination_params(cursor))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn read_resource_request(peer: &JsonRpcPeer, uri: &str) -> JsonRpcRequest {
    peer.request("resources/read", read_resource_params(uri))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn subscribe_resource_request(peer: &JsonRpcPeer, uri: &str) -> JsonRpcRequest {
    peer.request("resources/subscribe", resource_subscription_params(uri))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn unsubscribe_resource_request(peer: &JsonRpcPeer, uri: &str) -> JsonRpcRequest {
    peer.request("resources/unsubscribe", resource_subscription_params(uri))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn list_prompts_request(peer: &JsonRpcPeer, cursor: Option<&str>) -> JsonRpcRequest {
    peer.request("prompts/list", pagination_params(cursor))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn get_prompt_request(peer: &JsonRpcPeer, name: &str, args: Value) -> JsonRpcRequest {
    peer.request("prompts/get", get_prompt_params(name, args))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn decode_list_tools(
    response: JsonRpcResponse,
) -> Result<McpListPage<McpToolDescriptor>, McpError> {
    let result = decode_success::<ListToolsResult>(response)?;
    Ok(McpListPage {
        items: result.tools,
        next_cursor: result.next_cursor,
    })
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn decode_list_resources(
    response: JsonRpcResponse,
) -> Result<McpListPage<McpResource>, McpError> {
    let result = decode_success::<ListResourcesResult>(response)?;
    Ok(McpListPage {
        items: result.resources,
        next_cursor: result.next_cursor,
    })
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn decode_read_resource(
    response: JsonRpcResponse,
) -> Result<McpReadResourceResult, McpError> {
    let result = decode_success::<ReadResourceResult>(response)?;
    Ok(McpReadResourceResult {
        contents: result.contents,
        meta: result.meta,
    })
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn decode_empty_result(response: JsonRpcResponse) -> Result<(), McpError> {
    let _: Value = decode_success(response)?;
    Ok(())
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn decode_list_prompts(
    response: JsonRpcResponse,
) -> Result<McpListPage<McpPrompt>, McpError> {
    let result = decode_success::<ListPromptsResult>(response)?;
    Ok(McpListPage {
        items: result.prompts,
        next_cursor: result.next_cursor,
    })
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn pagination_params(cursor: Option<&str>) -> Option<Value> {
    cursor.map(|cursor| json!({ "cursor": cursor }))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn call_tool_params(name: &str, args: Value) -> Option<Value> {
    Some(json!({ "name": name, "arguments": args }))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn read_resource_params(uri: &str) -> Option<Value> {
    Some(json!({ "uri": uri }))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn resource_subscription_params(uri: &str) -> Option<Value> {
    Some(json!({ "uri": uri }))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn get_prompt_params(name: &str, args: Value) -> Option<Value> {
    Some(json!({ "name": name, "arguments": args }))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn decode_prompt_messages(
    response: JsonRpcResponse,
) -> Result<McpPromptMessages, McpError> {
    decode_success(response)
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn decode_tool_result(response: JsonRpcResponse) -> Result<McpToolResult, McpError> {
    decode_success(response)
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn decode_success<T>(response: JsonRpcResponse) -> Result<T, McpError>
where
    T: DeserializeOwned,
{
    if let Some(error) = response.error {
        if let Some(requests) = url_elicitations_from_jsonrpc_error(&error) {
            return Err(McpError::UrlElicitationRequired(requests));
        }
        return Err(McpError::Protocol(format!(
            "{} ({})",
            error.message, error.code
        )));
    }

    let result = response
        .result
        .ok_or_else(|| McpError::InvalidResponse("missing result field".into()))?;
    serde_json::from_value(result).map_err(|error| McpError::InvalidResponse(error.to_string()))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) struct ClientInboundSupport {
    pub(crate) capabilities: McpClientCapabilities,
    pub(crate) sampling: Option<Arc<dyn SamplingRequestRouter>>,
    pub(crate) elicitation: Option<Arc<dyn ElicitationRequestRouter>>,
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn client_inbound_support(
    spec: &McpServerSpec,
    context: &McpConnectContext,
) -> ClientInboundSupport {
    let sampling = context
        .sampling_provider
        .as_ref()
        .filter(|_| !spec.sampling.is_denied())
        .zip(context.authorization.as_ref())
        .map(|(provider, authorization)| {
            let handler =
                SamplingJsonRpcHandler::new(spec.sampling.clone(), Arc::clone(&context.event_sink))
                    .with_timeouts(spec.timeouts)
                    .with_session_id(authorization.session_id)
                    .with_run_id(Some(authorization.run_id))
                    .with_server_id(spec.server_id.clone())
                    .with_permission_mode(context.permission_mode)
                    .with_server_trust(spec.trust)
                    .with_metrics_sink(context.metrics_sink_or(Arc::new(crate::NoopMcpMetricsSink)))
                    .with_provider(Arc::clone(provider))
                    .with_authorization_context(authorization.clone());
            Arc::new(handler) as Arc<dyn SamplingRequestRouter>
        });
    let elicitation = context.elicitation_handler.as_ref().map(|handler| {
        Arc::new(
            ElicitationJsonRpcHandler::new(
                spec.server_id.clone(),
                context.permission_mode,
                Arc::clone(handler),
            )
            .with_timeout(spec.timeouts.call_default),
        ) as Arc<dyn ElicitationRequestRouter>
    });
    let capabilities = McpClientCapabilities {
        sampling: sampling.as_ref().map(|_| SamplingClientCapability {
            tools: Some(EmptyClientCapability::default()),
            ..Default::default()
        }),
        elicitation: elicitation.as_ref().map(|_| ElicitationClientCapability {
            form: Some(EmptyClientCapability::default()),
            url: Some(EmptyClientCapability::default()),
            ..Default::default()
        }),
        ..Default::default()
    };
    ClientInboundSupport {
        capabilities,
        sampling,
        elicitation,
    }
}

#[cfg(any(feature = "http", feature = "websocket", feature = "sse"))]
pub(crate) fn response_key(id: &Value) -> String {
    serde_json::to_string(id).expect("json-rpc ids should serialize")
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn notification_change(
    method: &str,
    params: Option<&Value>,
) -> Option<crate::McpChange> {
    match method {
        "tools/list_changed" | "notifications/tools/list_changed" => {
            Some(crate::McpChange::ToolsListChanged)
        }
        "resources/list_changed" | "notifications/resources/list_changed" => {
            Some(crate::McpChange::ResourcesListChanged)
        }
        "resources/updated" | "notifications/resources/updated" => params
            .and_then(|params| params.get("uri"))
            .and_then(Value::as_str)
            .map(|uri| crate::McpChange::ResourceUpdated {
                uri: uri.to_owned(),
            }),
        "prompts/list_changed" | "notifications/prompts/list_changed" => {
            Some(crate::McpChange::PromptsListChanged)
        }
        "notifications/cancelled" => Some(crate::McpChange::Cancelled {
            request_id: params
                .and_then(|params| params.get("requestId").or_else(|| params.get("request_id")))
                .and_then(notification_token),
            reason: params
                .and_then(|params| params.get("reason"))
                .and_then(Value::as_str)
                .map(str::to_owned),
        }),
        "notifications/progress" => Some(crate::McpChange::Progress {
            progress_token: params
                .and_then(|params| {
                    params
                        .get("progressToken")
                        .or_else(|| params.get("progress_token"))
                })
                .and_then(notification_token),
            progress: params
                .and_then(|params| params.get("progress"))
                .and_then(Value::as_f64),
            total: params
                .and_then(|params| params.get("total"))
                .and_then(Value::as_f64),
            message: params
                .and_then(|params| params.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned),
        }),
        _ => None,
    }
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
fn notification_token(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}

#[cfg(any(feature = "http", feature = "websocket", feature = "sse"))]
pub(crate) fn tool_call_event_from_change(
    request_key: &str,
    change: crate::McpChange,
) -> Option<crate::McpToolCallEvent> {
    match change {
        crate::McpChange::Progress {
            progress_token,
            progress,
            total,
            message,
        } if progress_token.as_deref() == Some(request_key) => {
            Some(crate::McpToolCallEvent::Progress {
                progress_token,
                progress,
                total,
                message,
            })
        }
        crate::McpChange::Cancelled { request_id, reason }
            if request_id.as_deref() == Some(request_key) =>
        {
            Some(crate::McpToolCallEvent::Cancelled { request_id, reason })
        }
        _ => None,
    }
}

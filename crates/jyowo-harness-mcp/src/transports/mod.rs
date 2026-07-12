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
use harness_contracts::PermissionMode;

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
use crate::{
    elicitation_from_jsonrpc_error, handle_jsonrpc_elicitation_error, ElicitationHandler,
    InitializeParams, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, McpClientCapabilities,
    McpError, McpImplementation, McpListPage, McpPrompt, McpPromptMessages, McpReadResourceResult,
    McpResource, McpResourceContents, McpToolDescriptor, McpToolResult,
};

#[cfg(feature = "http")]
mod http;
#[cfg(feature = "in-process")]
mod in_process;
#[cfg(feature = "sse")]
mod sse;
#[cfg(feature = "stdio")]
mod stdio;
#[cfg(feature = "websocket")]
mod websocket;

#[cfg(feature = "http")]
pub use http::HttpTransport;
#[cfg(feature = "in-process")]
pub use in_process::InProcessTransport;
#[cfg(feature = "sse")]
pub use sse::SseTransport;
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

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn initialized_notification() -> JsonRpcNotification {
    JsonRpcNotification::new("notifications/initialized", None)
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn initialize_request(peer: &JsonRpcPeer) -> JsonRpcRequest {
    // Existing transports do not yet validate and retain InitializeResult. Keep their wire
    // version at the last implemented lifecycle revision until McpSession owns the handshake.
    const LEGACY_TRANSPORT_PROTOCOL_VERSION: &str = "2025-03-26";
    let params = InitializeParams {
        protocol_version: LEGACY_TRANSPORT_PROTOCOL_VERSION.to_owned(),
        capabilities: McpClientCapabilities::default(),
        client_info: McpImplementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
        extra: serde_json::Map::new(),
    };
    peer.request(
        "initialize",
        Some(serde_json::to_value(params).expect("initialize params serialize")),
    )
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

    #[test]
    fn legacy_transport_helper_does_not_advertise_unwired_latest_session() {
        let request = initialize_request(&JsonRpcPeer::new());
        let protocol_version = request
            .params
            .as_ref()
            .and_then(|params| params.get("protocolVersion"))
            .and_then(Value::as_str);

        assert_eq!(protocol_version, Some("2025-03-26"));
    }

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
    peer.request(
        "tools/call",
        Some(json!({
            "name": name,
            "arguments": args,
        })),
    )
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
    peer.request("resources/read", Some(json!({ "uri": uri })))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn subscribe_resource_request(peer: &JsonRpcPeer, uri: &str) -> JsonRpcRequest {
    peer.request("resources/subscribe", Some(json!({ "uri": uri })))
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn unsubscribe_resource_request(peer: &JsonRpcPeer, uri: &str) -> JsonRpcRequest {
    peer.request("resources/unsubscribe", Some(json!({ "uri": uri })))
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
    peer.request(
        "prompts/get",
        Some(json!({
            "name": name,
            "arguments": args,
        })),
    )
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
fn pagination_params(cursor: Option<&str>) -> Option<Value> {
    cursor.map(|cursor| json!({ "cursor": cursor }))
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
        if let Some(request) = elicitation_from_jsonrpc_error(&error) {
            let detail = request
                .detail
                .as_deref()
                .map(|detail| format!(": {detail}"))
                .unwrap_or_default();
            return Err(McpError::Elicitation(format!(
                "mcp server {} requires elicitation for {}{}",
                request.server_id.0, request.subject, detail
            )));
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
pub(crate) async fn continue_after_elicitation_response(
    response: &JsonRpcResponse,
    request: &JsonRpcRequest,
    peer: &JsonRpcPeer,
    handler: Option<&Arc<dyn ElicitationHandler>>,
    permission_mode: PermissionMode,
) -> Result<Option<JsonRpcRequest>, McpError> {
    let Some(error) = response.error.as_ref() else {
        return Ok(None);
    };
    let Some(handler) = handler else {
        return Ok(None);
    };
    let value = handle_jsonrpc_elicitation_error(error, permission_mode, Arc::clone(handler))
        .await
        .map_err(|error| McpError::Elicitation(error.to_string()))?;
    let Some(value) = value else {
        return Ok(None);
    };
    continue_tool_call_after_elicitation(request, value, peer).map(Some)
}

#[cfg(any(
    feature = "stdio",
    feature = "http",
    feature = "websocket",
    feature = "sse"
))]
pub(crate) fn continue_tool_call_after_elicitation(
    request: &JsonRpcRequest,
    value: Value,
    peer: &JsonRpcPeer,
) -> Result<JsonRpcRequest, McpError> {
    if request.method != "tools/call" {
        return Err(McpError::Elicitation(format!(
            "elicitation continuation is not supported for {}",
            request.method
        )));
    }
    let Value::Object(resolved) = value else {
        return Err(McpError::Elicitation(
            "elicitation handler returned non-object value".to_owned(),
        ));
    };
    let mut params = request
        .params
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));
    let Some(params_obj) = params.as_object_mut() else {
        return Err(McpError::Elicitation(
            "tools/call params are not an object".to_owned(),
        ));
    };
    let arguments = params_obj
        .entry("arguments")
        .or_insert_with(|| serde_json::json!({}));
    let Some(arguments_obj) = arguments.as_object_mut() else {
        return Err(McpError::Elicitation(
            "tools/call arguments are not an object".to_owned(),
        ));
    };
    for (key, value) in resolved {
        arguments_obj.insert(key, value);
    }
    Ok(peer.request("tools/call", Some(params)))
}

#[cfg(any(feature = "stdio", feature = "websocket", feature = "sse"))]
pub(crate) fn response_key(id: &Value) -> String {
    serde_json::to_string(id).expect("json-rpc ids should serialize")
}

#[cfg(any(feature = "stdio", feature = "websocket", feature = "sse"))]
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
                .and_then(Value::as_str)
                .map(str::to_owned),
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
                .and_then(Value::as_str)
                .map(str::to_owned),
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

#[cfg(any(feature = "stdio", feature = "websocket", feature = "sse"))]
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

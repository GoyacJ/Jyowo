#![cfg(any(feature = "http", feature = "sse", feature = "websocket"))]

use std::collections::BTreeMap;
use std::sync::Arc;

use harness_contracts::{McpServerId, McpServerSource};
use harness_mcp::{McpClient, McpClientAuth, McpServerSpec, TransportChoice};

#[cfg(feature = "http")]
#[tokio::test]
#[ignore = "requires JYOWO_MCP_LIVE_HTTP_URL"]
async fn live_http_mcp_lists_tools_when_env_is_set() {
    let Some(url) = std::env::var("JYOWO_MCP_LIVE_HTTP_URL").ok() else {
        return;
    };
    let mut spec = McpServerSpec::new(
        McpServerId("live-http".to_owned()),
        "live http mcp",
        TransportChoice::Http {
            url,
            headers: BTreeMap::new(),
        },
        McpServerSource::Workspace,
    );
    if let Ok(token) = std::env::var("JYOWO_MCP_LIVE_BEARER_TOKEN") {
        spec.auth = McpClientAuth::Bearer(token);
    }
    let connection = McpClient::new(Arc::new(harness_mcp::HttpTransport::new()))
        .connect(spec)
        .await
        .expect("live HTTP MCP should connect");
    connection
        .list_tools()
        .await
        .expect("live HTTP MCP should list tools");
}

#[cfg(feature = "sse")]
#[tokio::test]
#[ignore = "requires JYOWO_MCP_LIVE_SSE_URL"]
async fn live_sse_mcp_lists_tools_when_env_is_set() {
    let Some(url) = std::env::var("JYOWO_MCP_LIVE_SSE_URL").ok() else {
        return;
    };
    let mut spec = McpServerSpec::new(
        McpServerId("live-sse".to_owned()),
        "live sse mcp",
        TransportChoice::Sse {
            url,
            headers: BTreeMap::new(),
        },
        McpServerSource::Workspace,
    );
    if let Ok(token) = std::env::var("JYOWO_MCP_LIVE_BEARER_TOKEN") {
        spec.auth = McpClientAuth::Bearer(token);
    }
    let connection = McpClient::new(Arc::new(harness_mcp::SseTransport::new()))
        .connect(spec)
        .await
        .expect("live SSE MCP should connect");
    connection
        .list_tools()
        .await
        .expect("live SSE MCP should list tools");
}

#[cfg(feature = "websocket")]
#[tokio::test]
#[ignore = "requires JYOWO_MCP_LIVE_WEBSOCKET_URL"]
async fn live_websocket_mcp_lists_tools_when_env_is_set() {
    let Some(url) = std::env::var("JYOWO_MCP_LIVE_WEBSOCKET_URL").ok() else {
        return;
    };
    let mut spec = McpServerSpec::new(
        McpServerId("live-websocket".to_owned()),
        "live websocket mcp",
        TransportChoice::WebSocket {
            url,
            headers: BTreeMap::new(),
        },
        McpServerSource::Workspace,
    );
    if let Ok(token) = std::env::var("JYOWO_MCP_LIVE_BEARER_TOKEN") {
        spec.auth = McpClientAuth::Bearer(token);
    }
    let connection = McpClient::new(Arc::new(harness_mcp::WebsocketTransport::new()))
        .connect(spec)
        .await
        .expect("live WebSocket MCP should connect");
    connection
        .list_tools()
        .await
        .expect("live WebSocket MCP should list tools");
}

#![cfg(feature = "in-process")]

use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::{McpServerId, McpServerSource};
use harness_mcp::{
    InProcessTransport, ListChangedEvent, McpChange, McpClient, McpConnection, McpError,
    McpServerSpec, McpToolDescriptor, McpToolResult, TransportChoice,
};
use parking_lot::Mutex;
use serde_json::{json, Value};

#[tokio::test]
async fn in_process_transport_returns_supplied_connection() {
    let connection = TestConnection {
        tools: vec![tool("local")],
        results: Mutex::new(VecDeque::from([McpToolResult::text("ran")])),
    };
    let spec = McpServerSpec::new(
        McpServerId("inproc".into()),
        "in-process fixture",
        TransportChoice::InProcess,
        McpServerSource::Workspace,
    );

    let connection = McpClient::new(Arc::new(InProcessTransport::from_connection(Arc::new(
        connection,
    ))))
    .connect(spec)
    .await
    .expect("in-process connects");

    let tools = connection.list_tools().await.expect("tools list");
    assert_eq!(tools[0].name, "local");

    let result = connection
        .call_tool("local", json!({ "ok": true }))
        .await
        .expect("tool call");
    assert_eq!(result, McpToolResult::text("ran"));

    let mut changes = connection.subscribe_changes().await.expect("changes");
    assert_eq!(changes.next().await, Some(McpChange::ToolsListChanged));
}

#[tokio::test]
async fn in_process_transport_rejects_wrong_transport_choice() {
    let spec = McpServerSpec::new(
        McpServerId("bad".into()),
        "bad fixture",
        TransportChoice::Http {
            url: "http://127.0.0.1/mcp".into(),
            headers: BTreeMap::default(),
        },
        McpServerSource::Workspace,
    );

    let result = McpClient::new(Arc::new(InProcessTransport::from_connection(Arc::new(
        TestConnection::default(),
    ))))
    .connect(spec)
    .await;

    assert!(matches!(result, Err(McpError::Unsupported(_))));
}

fn tool(name: &str) -> McpToolDescriptor {
    McpToolDescriptor {
        name: name.into(),
        description: Some(format!("{name} tool")),
        input_schema: json!({ "type": "object" }),
        output_schema: None,
        annotations: None,
        meta: BTreeMap::default(),
    }
}

#[derive(Default)]
struct TestConnection {
    tools: Vec<McpToolDescriptor>,
    results: Mutex<VecDeque<McpToolResult>>,
}

#[async_trait]
impl McpConnection for TestConnection {
    fn connection_id(&self) -> &'static str {
        "test-in-process"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        Ok(self.tools.clone())
    }

    async fn call_tool(&self, _name: &str, _args: Value) -> Result<McpToolResult, McpError> {
        self.results
            .lock()
            .pop_front()
            .ok_or_else(|| McpError::Protocol("missing result".into()))
    }

    async fn subscribe_changes(&self) -> Result<ListChangedEvent, McpError> {
        Ok(Box::pin(futures::stream::iter([
            McpChange::ToolsListChanged,
        ])))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        Ok(())
    }
}

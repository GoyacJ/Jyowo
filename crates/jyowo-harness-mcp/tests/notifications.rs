#![cfg(feature = "websocket")]

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use harness_contracts::{Event, McpResourceUpdateKind, McpServerId, McpServerSource, SessionId};
use harness_mcp::{
    ListChangedEvent, McpChange, McpClient, McpConnection, McpError, McpEventSink, McpPrompt,
    McpPromptMessages, McpRegistry, McpResource, McpResourceContents, McpServerScope,
    McpServerSpec, McpToolDescriptor, McpToolResult, NoopMcpEventSink, TransportChoice,
    WebsocketTransport,
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

mod support;

#[tokio::test]
async fn websocket_transport_maps_mcp_notifications_to_changes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = accept_async(stream).await.expect("websocket accept");
        while let Some(message) = socket.next().await {
            let text = message.expect("message").into_text().expect("text");
            let value: Value = serde_json::from_str(&text).expect("json");
            match value.get("method").and_then(Value::as_str) {
                Some("initialize") => {
                    socket
                        .send(Message::text(
                            json!({
                                "jsonrpc": "2.0",
                                "id": value["id"].clone(),
                                "result": {
                                    "protocolVersion": "2025-03-26",
                                    "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
                                    "serverInfo": { "name": "fixture", "version": "0.1.0" }
                                }
                            })
                            .to_string(),
                        ))
                        .await
                        .expect("send initialize");
                }
                Some("tools/list") => {
                    socket
                        .send(Message::text(
                            json!({
                                "jsonrpc": "2.0",
                                "id": value["id"].clone(),
                                "result": { "tools": [] }
                            })
                            .to_string(),
                        ))
                        .await
                        .expect("send tools list");
                    for notification in [
                        json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" }),
                        json!({ "jsonrpc": "2.0", "method": "notifications/resources/list_changed" }),
                        json!({
                            "jsonrpc": "2.0",
                            "method": "notifications/resources/updated",
                            "params": { "uri": "jyowo://sessions/1" }
                        }),
                        json!({ "jsonrpc": "2.0", "method": "notifications/prompts/list_changed" }),
                        json!({
                            "jsonrpc": "2.0",
                            "method": "notifications/cancelled",
                            "params": { "requestId": "call-1", "reason": "client interrupted" }
                        }),
                        json!({
                            "jsonrpc": "2.0",
                            "method": "notifications/progress",
                            "params": {
                                "progressToken": "call-1",
                                "progress": 2,
                                "total": 4,
                                "message": "half"
                            }
                        }),
                    ] {
                        socket
                            .send(Message::text(notification.to_string()))
                            .await
                            .expect("send notification");
                    }
                }
                _ => {}
            }
        }
    });

    let spec = McpServerSpec::new(
        McpServerId("ws".into()),
        "websocket fixture",
        TransportChoice::WebSocket {
            url: format!("ws://{addr}"),
            headers: BTreeMap::default(),
        },
        McpServerSource::Workspace,
    );
    let connection = McpClient::new(Arc::new(WebsocketTransport::new()))
        .connect_with_context(spec, support::authorized_connect_context())
        .await
        .expect("websocket connects");
    let mut changes = connection.subscribe_changes().await.expect("changes");

    connection.list_tools().await.expect("tools");

    assert_eq!(changes.next().await, Some(McpChange::ToolsListChanged));
    assert_eq!(changes.next().await, Some(McpChange::ResourcesListChanged));
    assert_eq!(
        changes.next().await,
        Some(McpChange::ResourceUpdated {
            uri: "jyowo://sessions/1".into()
        })
    );
    assert_eq!(changes.next().await, Some(McpChange::PromptsListChanged));
    assert_eq!(
        changes.next().await,
        Some(McpChange::Cancelled {
            request_id: Some("call-1".into()),
            reason: Some("client interrupted".into())
        })
    );
    assert_eq!(
        changes.next().await,
        Some(McpChange::Progress {
            progress_token: Some("call-1".into()),
            progress: Some(2.0),
            total: Some(4.0),
            message: Some("half".into())
        })
    );
}

#[tokio::test]
async fn registry_maps_resource_and_prompt_notifications_to_harness_events() {
    let connection = Arc::new(MutableMetadata::default());
    connection.set_resources(vec![McpResource {
        uri: "jyowo://sessions/1".into(),
        name: "session 1".into(),
        title: None,
        description: None,
        mime_type: Some("application/json".into()),
        icons: None,
        annotations: None,
        size: None,
        meta: Default::default(),
    }]);
    connection.set_prompts(vec![McpPrompt {
        name: "triage".into(),
        title: None,
        description: None,
        icons: None,
        arguments: None,
        meta: Default::default(),
    }]);
    let registry = McpRegistry::new();
    registry
        .add_ready_server(
            spec(),
            McpServerScope::Session(SessionId::from_u128(9)),
            connection.clone(),
        )
        .await
        .expect("server");
    let sink = Arc::new(CollectingSink::default());

    registry
        .handle_resources_list_changed(&server_id(), sink.clone())
        .await
        .expect("resources list changed");
    registry
        .handle_resource_updated(&server_id(), "jyowo://sessions/1".into(), sink.clone())
        .await
        .expect("resource updated");
    registry
        .handle_prompts_list_changed(&server_id(), sink.clone())
        .await
        .expect("prompts list changed");

    let events = sink.events();
    assert!(matches!(
        events.first(),
        Some(Event::McpResourceUpdated(event))
            if event.server_id == server_id()
                && event.session_id == Some(SessionId::from_u128(9))
                && event.kind == McpResourceUpdateKind::ListChanged { added: 1, removed: 0 }
    ));
    assert!(matches!(
        events.get(1),
        Some(Event::McpResourceUpdated(event))
            if event.kind == McpResourceUpdateKind::ResourceUpdated {
                uri: "jyowo://sessions/1".into()
            }
    ));
    assert!(matches!(
        events.get(2),
        Some(Event::McpResourceUpdated(event))
            if event.kind == McpResourceUpdateKind::PromptsListChanged { added: 1, removed: 0 }
    ));
}

#[tokio::test]
async fn registry_delegates_resource_subscription_lifecycle_to_connection() {
    let connection = Arc::new(MutableMetadata::default());
    let registry = McpRegistry::new();
    registry
        .add_ready_server(
            spec(),
            McpServerScope::Session(SessionId::from_u128(9)),
            connection.clone(),
        )
        .await
        .expect("server");

    registry
        .subscribe_resource(&server_id(), "jyowo://sessions/1")
        .await
        .expect("subscribe");
    registry
        .unsubscribe_resource(&server_id(), "jyowo://sessions/1")
        .await
        .expect("unsubscribe");

    assert_eq!(
        connection.subscribed(),
        vec!["jyowo://sessions/1".to_owned()]
    );
    assert_eq!(
        connection.unsubscribed(),
        vec!["jyowo://sessions/1".to_owned()]
    );
}

#[tokio::test]
async fn registry_unsubscribes_noisy_resource_updates() {
    let connection = Arc::new(MutableMetadata::default());
    let registry = McpRegistry::new();
    let mut spec = spec();
    spec.resource_update_policy.max_updates_per_window = 1;
    registry
        .add_ready_server(
            spec,
            McpServerScope::Session(SessionId::from_u128(1)),
            connection.clone(),
        )
        .await
        .expect("server");

    registry
        .subscribe_resource(&server_id(), "jyowo://sessions/1")
        .await
        .expect("subscribe");
    registry
        .handle_resource_updated(
            &server_id(),
            "jyowo://sessions/1".to_owned(),
            Arc::new(NoopMcpEventSink),
        )
        .await
        .expect("first update");
    registry
        .handle_resource_updated(
            &server_id(),
            "jyowo://sessions/1".to_owned(),
            Arc::new(NoopMcpEventSink),
        )
        .await
        .expect("second update");

    assert_eq!(
        connection.unsubscribed(),
        vec!["jyowo://sessions/1".to_owned()]
    );
}

#[tokio::test]
async fn registry_marks_idle_resource_subscription_reconnecting() {
    let connection = Arc::new(MutableMetadata::default());
    let registry = McpRegistry::new();
    let mut spec = spec();
    spec.timeouts.idle = std::time::Duration::from_millis(1);
    registry
        .add_ready_server(
            spec,
            McpServerScope::Session(SessionId::from_u128(1)),
            connection,
        )
        .await
        .expect("server");

    registry
        .subscribe_resource(&server_id(), "jyowo://sessions/1")
        .await
        .expect("subscribe");
    registry
        .enforce_resource_update_idle_at(
            &server_id(),
            harness_contracts::now() + chrono::Duration::seconds(1),
        )
        .await
        .expect("idle governance");

    assert!(matches!(
        registry.connection_state(&server_id()).await,
        Some(harness_mcp::McpConnectionState::Reconnecting { .. })
    ));
}

fn spec() -> McpServerSpec {
    McpServerSpec::new(
        server_id(),
        "fixture",
        TransportChoice::InProcess,
        McpServerSource::Workspace,
    )
}

fn server_id() -> McpServerId {
    McpServerId("fixture".to_owned())
}

#[derive(Default)]
struct MutableMetadata {
    resources: Mutex<Vec<McpResource>>,
    prompts: Mutex<Vec<McpPrompt>>,
    subscribed: Mutex<Vec<String>>,
    unsubscribed: Mutex<Vec<String>>,
}

impl MutableMetadata {
    fn set_resources(&self, resources: Vec<McpResource>) {
        *self.resources.lock() = resources;
    }

    fn set_prompts(&self, prompts: Vec<McpPrompt>) {
        *self.prompts.lock() = prompts;
    }

    fn subscribed(&self) -> Vec<String> {
        self.subscribed.lock().clone()
    }

    fn unsubscribed(&self) -> Vec<String> {
        self.unsubscribed.lock().clone()
    }
}

#[async_trait]
impl McpConnection for MutableMetadata {
    fn connection_id(&self) -> &'static str {
        "metadata"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        Ok(Vec::new())
    }

    async fn call_tool(&self, _name: &str, _args: Value) -> Result<McpToolResult, McpError> {
        Ok(McpToolResult::text("ok"))
    }

    async fn list_resources(&self) -> Result<Vec<McpResource>, McpError> {
        Ok(self.resources.lock().clone())
    }

    async fn read_resource(
        &self,
        uri: &str,
    ) -> Result<harness_mcp::McpReadResourceResult, McpError> {
        Ok(harness_mcp::McpReadResourceResult {
            contents: vec![McpResourceContents::Text {
                uri: uri.into(),
                mime_type: None,
                text: String::new(),
                meta: Default::default(),
            }],
            meta: Default::default(),
        })
    }

    async fn subscribe_resource(&self, uri: &str) -> Result<(), McpError> {
        self.subscribed.lock().push(uri.to_owned());
        Ok(())
    }

    async fn unsubscribe_resource(&self, uri: &str) -> Result<(), McpError> {
        self.unsubscribed.lock().push(uri.to_owned());
        Ok(())
    }

    async fn list_prompts(&self) -> Result<Vec<McpPrompt>, McpError> {
        Ok(self.prompts.lock().clone())
    }

    async fn get_prompt(&self, _name: &str, _args: Value) -> Result<McpPromptMessages, McpError> {
        Ok(McpPromptMessages {
            description: None,
            messages: vec![],
            meta: Default::default(),
        })
    }

    async fn subscribe_changes(&self) -> Result<ListChangedEvent, McpError> {
        Ok(Box::pin(futures::stream::empty()))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        Ok(())
    }
}

#[derive(Default)]
struct CollectingSink {
    events: Mutex<Vec<Event>>,
}

impl CollectingSink {
    fn events(&self) -> Vec<Event> {
        self.events.lock().clone()
    }
}

impl McpEventSink for CollectingSink {
    fn emit(&self, event: Event) {
        self.events.lock().push(event);
    }
}

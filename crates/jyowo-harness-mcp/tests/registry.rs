use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use futures::{channel::mpsc, SinkExt};
use harness_contracts::{Event, McpServerId, McpServerSource, PluginId, TrustLevel};
use harness_mcp::{
    ListChangedEvent, McpChange, McpConnection, McpConnectionState, McpError, McpEventSink,
    McpRegistry, McpServerPattern, McpServerRef, McpServerScope, McpServerSpec, McpToolDescriptor,
    McpToolResult, RequiredEvaluation, TransportChoice,
};
use harness_tool::{BuiltinToolset, ToolRegistry};
use parking_lot::Mutex;
use serde_json::{json, Value};

#[tokio::test]
async fn list_changed_updates_tool_pool() {
    let tool_registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .build()
        .expect("registry");
    let (connection, mut sender) = NotifyingTools::new(vec![tool("old")]);
    let registry = McpRegistry::new();
    registry
        .add_ready_server(
            spec(McpServerSource::Workspace),
            McpServerScope::Global,
            connection.clone(),
        )
        .await
        .expect("server");
    registry
        .inject_tools_into(&tool_registry, &server_id())
        .await
        .expect("initial inject");
    registry
        .subscribe_list_changed(
            tool_registry.clone(),
            server_id(),
            Arc::new(CollectingSink::default()),
        )
        .await
        .expect("subscription");

    connection.set_tools(vec![tool("new")]);
    sender
        .send(McpChange::ToolsListChanged)
        .await
        .expect("send change");

    wait_for(|| tool_registry.get("mcp__fixture__new").is_some()).await;
    assert!(tool_registry.get("mcp__fixture__old").is_none());
}

#[tokio::test]
async fn plugin_mcp_registration_preserves_plugin_trust() {
    let registry = McpRegistry::new();
    registry
        .add_plugin_server(
            PluginId("admin-plugin@1.0.0".into()),
            TrustLevel::AdminTrusted,
            spec(McpServerSource::Workspace),
        )
        .await
        .expect("plugin server");

    let registered = registry
        .server_spec(&server_id())
        .await
        .expect("server spec");
    assert_eq!(registered.trust, TrustLevel::AdminTrusted);
    assert!(matches!(registered.source, McpServerSource::Plugin(_)));

    let mut user_spec = spec(McpServerSource::Workspace);
    user_spec.server_id = McpServerId("user-plugin-server".into());
    registry
        .add_plugin_server(
            PluginId("user-plugin@1.0.0".into()),
            TrustLevel::UserControlled,
            user_spec,
        )
        .await
        .expect("user plugin server");

    let registered = registry
        .server_spec(&McpServerId("user-plugin-server".into()))
        .await
        .expect("user plugin server spec");
    assert_eq!(registered.trust, TrustLevel::UserControlled);
    assert!(matches!(registered.source, McpServerSource::Plugin(_)));
}

#[tokio::test]
async fn schema_fingerprint_is_stable_across_tool_order_changes() {
    let tool_registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .build()
        .expect("registry");
    let connection = Arc::new(NotifyingTools::static_tools(vec![
        tool("alpha"),
        tool("beta"),
    ]));
    let registry = McpRegistry::new();
    registry
        .add_ready_server(
            spec(McpServerSource::Workspace),
            McpServerScope::Global,
            connection.clone(),
        )
        .await
        .expect("server");
    registry
        .inject_tools_into(&tool_registry, &server_id())
        .await
        .expect("initial inject");
    let before = registry
        .schema_fingerprint(&server_id())
        .await
        .expect("initial fingerprint");

    connection.set_tools(vec![tool("beta"), tool("alpha")]);
    registry
        .handle_list_changed(
            &tool_registry,
            &server_id(),
            Arc::new(CollectingSink::default()),
        )
        .await
        .expect("list changed");

    assert_eq!(
        registry.schema_fingerprint(&server_id()).await,
        Some(before)
    );
}

#[tokio::test]
async fn required_evaluation_reports_missing_server() {
    let registry = McpRegistry::new();

    let evaluations = registry
        .evaluate_required(&[], &[McpServerPattern::exact(server_id())])
        .await;

    assert_eq!(
        evaluations,
        vec![RequiredEvaluation::Missing {
            pattern: "fixture".to_owned()
        }]
    );
}

#[tokio::test]
async fn required_evaluation_reports_not_ready_server() {
    let (connection, _) = NotifyingTools::new(Vec::new());
    let registry = McpRegistry::new();
    registry
        .add_ready_server(
            spec(McpServerSource::Workspace),
            McpServerScope::Global,
            connection,
        )
        .await
        .expect("server");
    registry
        .set_connection_state(
            &server_id(),
            McpConnectionState::Reconnecting {
                attempt: 1,
                last_error: "transport reset".to_owned(),
            },
        )
        .await
        .expect("state");

    let evaluations = registry
        .evaluate_required(&[], &[McpServerPattern::exact(server_id())])
        .await;

    assert_eq!(
        evaluations,
        vec![RequiredEvaluation::NotReady {
            server_id: server_id(),
            state: McpConnectionState::Reconnecting {
                attempt: 1,
                last_error: "transport reset".to_owned()
            }
        }]
    );
}

#[tokio::test]
async fn required_evaluation_rejects_inline_when_pattern_disallows_it() {
    let registry = McpRegistry::new();
    let inline = spec(McpServerSource::Workspace);
    let pattern = McpServerPattern {
        pattern: server_id().0,
        require_ready: true,
        allow_inline: false,
    };

    let evaluations = registry
        .evaluate_required(&[McpServerRef::Inline(inline)], &[pattern])
        .await;

    assert_eq!(
        evaluations,
        vec![RequiredEvaluation::InlineDisallowed {
            pattern: "fixture".to_owned(),
            server_id: server_id()
        }]
    );
}

async fn wait_for(mut predicate: impl FnMut() -> bool) {
    for _ in 0..20 {
        if predicate() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(predicate());
}

fn spec(source: McpServerSource) -> McpServerSpec {
    McpServerSpec::new(server_id(), "fixture", TransportChoice::InProcess, source)
}

fn server_id() -> McpServerId {
    McpServerId("fixture".to_owned())
}

fn tool(name: &str) -> McpToolDescriptor {
    McpToolDescriptor {
        name: name.to_owned(),
        description: Some(format!("{name} tool")),
        input_schema: json!({ "type": "object" }),
        output_schema: None,
        annotations: None,
        meta: BTreeMap::new(),
    }
}

struct NotifyingTools {
    tools: Mutex<Vec<McpToolDescriptor>>,
    changes: Mutex<Option<mpsc::Receiver<McpChange>>>,
}

impl NotifyingTools {
    fn new(tools: Vec<McpToolDescriptor>) -> (Arc<Self>, mpsc::Sender<McpChange>) {
        let (sender, receiver) = mpsc::channel(8);
        (
            Arc::new(Self {
                tools: Mutex::new(tools),
                changes: Mutex::new(Some(receiver)),
            }),
            sender,
        )
    }

    fn static_tools(tools: Vec<McpToolDescriptor>) -> Self {
        Self {
            tools: Mutex::new(tools),
            changes: Mutex::new(None),
        }
    }

    fn set_tools(&self, tools: Vec<McpToolDescriptor>) {
        *self.tools.lock() = tools;
    }
}

#[async_trait]
impl McpConnection for NotifyingTools {
    fn connection_id(&self) -> &'static str {
        "notifying-tools"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        Ok(self.tools.lock().clone())
    }

    async fn call_tool(&self, _name: &str, _args: Value) -> Result<McpToolResult, McpError> {
        Ok(McpToolResult::text("ok"))
    }

    async fn subscribe_changes(&self) -> Result<ListChangedEvent, McpError> {
        Ok(Box::pin(
            self.changes
                .lock()
                .take()
                .expect("subscribe_changes called once"),
        ))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        Ok(())
    }
}

#[derive(Default)]
struct CollectingSink {
    events: Mutex<Vec<Event>>,
}

impl McpEventSink for CollectingSink {
    fn emit(&self, event: Event) {
        self.events.lock().push(event);
    }
}

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use harness_contracts::{
    Event, McpServerId, McpServerSource, SessionId, ToolPoolChangeSource,
    ToolsListChangedDisposition,
};
use harness_mcp::{
    ListChangedDisposition, McpConnection, McpError, McpEventSink, McpMetric, McpMetricsSink,
    McpRegistry, McpServerScope, McpServerSpec, McpToolDescriptor, McpToolResult, TransportChoice,
};
use harness_tool::{BuiltinToolset, ToolRegistry};
use parking_lot::Mutex;
use serde_json::{json, Value};

#[tokio::test]
async fn auto_defer_list_changed_updates_registry_and_events() {
    let tool_registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .build()
        .expect("registry");
    let connection = Arc::new(MutableTools::new(vec![tool("old", false)]));
    let metrics = Arc::new(CollectingMetrics::default());
    let registry = registry_with_metrics(
        connection.clone(),
        McpServerScope::Session(SessionId::from_u128(1)),
        metrics.clone(),
    )
    .await;
    registry
        .inject_tools_into(&tool_registry, &server_id())
        .await
        .expect("initial inject");
    connection.set_tools(vec![tool("new", false)]);
    let sink = Arc::new(CollectingSink::default());

    let outcome = registry
        .handle_list_changed(&tool_registry, &server_id(), sink.clone())
        .await
        .expect("list changed");

    assert_eq!(outcome.disposition, ListChangedDisposition::DeferredApplied);
    assert!(tool_registry.get("mcp__fixture__old").is_none());
    assert!(tool_registry.get("mcp__fixture__new").is_some());
    let events = sink.events();
    assert!(matches!(
        events.first(),
        Some(Event::McpToolsListChanged(event))
            if event.added_count == 1
                && event.removed_count == 1
                && event.disposition == ToolsListChangedDisposition::DeferredApplied
    ));
    assert!(matches!(
        events.get(1),
        Some(Event::ToolDeferredPoolChanged(event))
            if matches!(
                event.source,
                ToolPoolChangeSource::McpListChanged { server_id: ref changed_server_id }
                    if changed_server_id == &server_id()
            )
    ));
    assert!(metrics.metrics().iter().any(|metric| {
        matches!(
            metric,
            McpMetric::ListChanged {
                disposition: ListChangedDisposition::DeferredApplied,
                ..
            }
        )
    }));
}

#[tokio::test]
async fn resource_update_notifications_record_metrics() {
    let connection = Arc::new(MutableTools::new(Vec::new()));
    let metrics = Arc::new(CollectingMetrics::default());
    let registry = registry_with_metrics(connection, McpServerScope::Global, metrics.clone()).await;

    registry
        .handle_resource_updated(
            &server_id(),
            "jyowo://sessions/1".to_owned(),
            Arc::new(CollectingSink::default()),
        )
        .await
        .expect("resource update");

    assert!(metrics.metrics().iter().any(|metric| {
        matches!(metric, McpMetric::ResourceUpdated { server_id: changed_server_id, .. } if changed_server_id == &server_id())
    }));
}

#[tokio::test]
async fn always_load_list_changed_is_pending_for_reload() {
    let tool_registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .build()
        .expect("registry");
    let connection = Arc::new(MutableTools::new(vec![tool("old", false)]));
    let registry = registry_with(connection.clone(), McpServerScope::Global).await;
    registry
        .inject_tools_into(&tool_registry, &server_id())
        .await
        .expect("initial inject");
    connection.set_tools(vec![tool("old", false), tool("always", true)]);

    let outcome = registry
        .handle_list_changed(
            &tool_registry,
            &server_id(),
            Arc::new(CollectingSink::default()),
        )
        .await
        .expect("list changed");

    assert_eq!(
        outcome.disposition,
        ListChangedDisposition::PendingForReload
    );
    assert!(tool_registry.get("mcp__fixture__always").is_none());
    assert_eq!(
        registry.pending_list_changed_servers().await,
        vec![server_id()]
    );
}

#[tokio::test]
async fn schema_only_list_changed_updates_fingerprint_and_timestamp() {
    let tool_registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .build()
        .expect("registry");
    let connection = Arc::new(MutableTools::new(vec![tool_with_schema(
        "lookup",
        false,
        json!({ "type": "object" }),
    )]));
    let registry = registry_with(connection.clone(), McpServerScope::Global).await;
    registry
        .inject_tools_into(&tool_registry, &server_id())
        .await
        .expect("initial inject");
    let before = registry
        .schema_fingerprint(&server_id())
        .await
        .expect("initial schema fingerprint");

    connection.set_tools(vec![tool_with_schema(
        "lookup",
        false,
        json!({
            "type": "object",
            "required": ["query"],
            "properties": { "query": { "type": "string" } }
        }),
    )]);
    let outcome = registry
        .handle_list_changed(
            &tool_registry,
            &server_id(),
            Arc::new(CollectingSink::default()),
        )
        .await
        .expect("schema changed");

    assert_eq!(outcome.disposition, ListChangedDisposition::DeferredApplied);
    assert!(outcome.added.is_empty());
    assert!(outcome.removed.is_empty());
    assert!(registry.last_list_changed(&server_id()).await.is_some());
    assert_ne!(
        registry.schema_fingerprint(&server_id()).await,
        Some(before)
    );
}

#[tokio::test]
async fn unchanged_or_rejected_list_changed_does_not_mutate_registry() {
    let tool_registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Empty)
        .build()
        .expect("registry");
    let connection = Arc::new(MutableTools::new(vec![tool("old", false)]));
    let registry = registry_with(connection.clone(), McpServerScope::Global).await;
    registry
        .inject_tools_into(&tool_registry, &server_id())
        .await
        .expect("initial inject");

    let no_change = registry
        .handle_list_changed(
            &tool_registry,
            &server_id(),
            Arc::new(CollectingSink::default()),
        )
        .await
        .expect("no change");
    assert_eq!(no_change.disposition, ListChangedDisposition::NoChange);

    connection.set_tools(vec![tool("bad name", false)]);
    let rejected = registry
        .handle_list_changed(
            &tool_registry,
            &server_id(),
            Arc::new(CollectingSink::default()),
        )
        .await;
    assert!(matches!(rejected, Err(McpError::ToolNamingViolation(_))));
    assert!(tool_registry.get("mcp__fixture__old").is_some());
}

async fn registry_with(connection: Arc<MutableTools>, scope: McpServerScope) -> McpRegistry {
    registry_with_metrics(connection, scope, Arc::new(CollectingMetrics::default())).await
}

async fn registry_with_metrics(
    connection: Arc<MutableTools>,
    scope: McpServerScope,
    metrics: Arc<CollectingMetrics>,
) -> McpRegistry {
    let registry = McpRegistry::with_metrics_sink(metrics);
    registry
        .add_ready_server(spec(), scope, connection)
        .await
        .expect("server");
    registry
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

fn tool(name: &str, always_load: bool) -> McpToolDescriptor {
    tool_with_schema(name, always_load, json!({ "type": "object" }))
}

fn tool_with_schema(name: &str, always_load: bool, input_schema: Value) -> McpToolDescriptor {
    let mut meta = BTreeMap::new();
    if always_load {
        meta.insert("anthropic/alwaysLoad".to_owned(), json!(true));
    }
    McpToolDescriptor {
        name: name.to_owned(),
        title: None,
        icons: None,
        execution: None,
        description: Some(format!("{name} tool")),
        input_schema,
        output_schema: None,
        annotations: None,
        meta,
    }
}

struct MutableTools {
    tools: Mutex<Vec<McpToolDescriptor>>,
}

impl MutableTools {
    fn new(tools: Vec<McpToolDescriptor>) -> Self {
        Self {
            tools: Mutex::new(tools),
        }
    }

    fn set_tools(&self, tools: Vec<McpToolDescriptor>) {
        *self.tools.lock() = tools;
    }
}

#[async_trait]
impl McpConnection for MutableTools {
    fn connection_id(&self) -> &'static str {
        "mutable-tools"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        Ok(self.tools.lock().clone())
    }

    async fn call_tool(&self, _name: &str, _args: Value) -> Result<McpToolResult, McpError> {
        Ok(McpToolResult::text("ok"))
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

#[derive(Default)]
struct CollectingMetrics {
    metrics: Mutex<Vec<McpMetric>>,
}

impl CollectingMetrics {
    fn metrics(&self) -> Vec<McpMetric> {
        self.metrics.lock().clone()
    }
}

impl McpMetricsSink for CollectingMetrics {
    fn record(&self, metric: McpMetric) {
        self.metrics.lock().push(metric);
    }
}

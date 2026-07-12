use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::{
    Event, McpConnectionLostReason, McpServerId, McpServerSource, SessionId, TrustLevel,
};
use harness_mcp::{
    ListChangedEvent, ManagedMcpConnection, McpChange, McpConnection, McpConnectionState, McpError,
    McpEventSink, McpMetric, McpMetricConnectionState, McpMetricOutcome, McpMetricsSink,
    McpRegistry, McpServerScope, McpServerSpec, McpToolDescriptor, McpToolResult, McpTransport,
    ReconnectPolicy, TransportChoice,
};
use harness_tool::ToolRegistry;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::Notify;

#[test]
fn reconnect_policy_backoff_caps_and_unlimited_attempts() {
    let policy = ReconnectPolicy {
        initial_backoff: Duration::from_millis(100),
        max_backoff: Duration::from_millis(250),
        backoff_jitter: 0.0,
        ..ReconnectPolicy::default()
    };

    policy.validate().expect("policy validates");
    assert_eq!(policy.backoff_for_attempt(1), Duration::from_millis(100));
    assert_eq!(policy.backoff_for_attempt(2), Duration::from_millis(200));
    assert_eq!(policy.backoff_for_attempt(3), Duration::from_millis(250));
    assert!(!policy.is_exhausted(10));

    let invalid = ReconnectPolicy {
        backoff_jitter: 1.5,
        ..ReconnectPolicy::default()
    };
    assert!(invalid.validate().is_err());
}

#[tokio::test]
async fn managed_connection_emits_first_recovered_on_initial_connect() {
    let sink = Arc::new(RecordingSink::default());
    let managed = managed_connection(
        policy(0),
        TestTransport::new(vec![Ok(TestConnection::default())]),
        sink.clone(),
    )
    .await;

    assert_eq!(managed.state().await, McpConnectionState::Ready);
    assert!(sink.events().iter().any(|event| {
        matches!(
            event,
            Event::McpConnectionRecovered(recovered)
                if recovered.was_first
                    && recovered.server_id == McpServerId("reconnect".into())
                    && recovered.attempts_used == 0
        )
    }));
}

#[tokio::test]
async fn managed_connection_enters_reconnecting_after_call_connection_error() {
    let notify = Arc::new(Notify::new());
    let sink = Arc::new(RecordingSink::default());
    let managed = managed_connection(
        policy(0),
        TestTransport::new(vec![
            Ok(TestConnection::with_results(vec![Err(
                McpError::Connection("lost".into()),
            )])),
            Ok(TestConnection::with_results(vec![Ok(McpToolResult::text(
                "after",
            ))])),
        ])
        .with_attempt_notify(notify.clone()),
        sink.clone(),
    )
    .await;

    assert!(matches!(
        managed.call_tool("search", json!({})).await,
        Err(McpError::Connection(_))
    ));
    wait_for_reconnecting(&managed).await;
    assert_eq!(
        managed.call_tool("search", json!({})).await,
        Err(McpError::Connection("mcp server reconnecting".into()))
    );
    assert!(sink.events().iter().any(|event| {
        matches!(
            event,
            Event::McpConnectionLost(lost)
                if lost.attempts_so_far == 0
                    && !lost.terminal
                    && matches!(lost.reason, McpConnectionLostReason::Other(_))
        )
    }));

    notify.notified().await;
}

#[tokio::test]
async fn managed_connection_reconnects_and_allows_calls_again() {
    let notify = Arc::new(Notify::new());
    let sink = Arc::new(RecordingSink::default());
    let managed = managed_connection(
        policy(0),
        TestTransport::new(vec![
            Ok(TestConnection::with_results(vec![Err(
                McpError::Connection("lost".into()),
            )])),
            Ok(TestConnection::with_results(vec![Ok(McpToolResult::text(
                "after",
            ))])),
        ])
        .with_attempt_notify(notify.clone()),
        sink.clone(),
    )
    .await;

    assert!(managed.call_tool("search", json!({})).await.is_err());
    notify.notified().await;
    wait_for_ready(&managed).await;

    assert_eq!(
        managed.call_tool("search", json!({})).await,
        Ok(McpToolResult::text("after"))
    );
    assert!(sink.events().iter().any(|event| {
        matches!(
            event,
            Event::McpConnectionRecovered(recovered)
                if !recovered.was_first
                    && recovered.attempts_used == 1
                    && !recovered.schema_changed
        )
    }));
}

#[tokio::test]
async fn managed_connection_records_connection_and_reconnect_metrics() {
    let notify = Arc::new(Notify::new());
    let metrics = Arc::new(CollectingMetrics::default());
    let managed = ManagedMcpConnection::connect_with_metrics(
        Arc::new(
            TestTransport::new(vec![
                Ok(TestConnection::with_results(vec![Err(
                    McpError::Connection("lost".into()),
                )])),
                Ok(TestConnection::with_results(vec![Ok(McpToolResult::text(
                    "after",
                ))])),
            ])
            .with_attempt_notify(notify.clone()),
        ),
        spec(policy(0)),
        McpServerScope::Session(SessionId::new()),
        Arc::new(RecordingSink::default()),
        metrics.clone(),
    )
    .await
    .expect("managed connection");

    assert!(matches!(
        managed.call_tool("search", json!({})).await,
        Err(McpError::Connection(_))
    ));
    notify.notified().await;
    wait_for_ready(&managed).await;

    let recorded = metrics.metrics();
    assert!(recorded.iter().any(|metric| {
        matches!(
            metric,
            McpMetric::ConnectionTotal {
                outcome: McpMetricOutcome::Success,
                transport,
                ..
            } if transport == "test"
        )
    }));
    assert!(recorded.iter().any(|metric| {
        matches!(
            metric,
            McpMetric::ConnectionState {
                state: McpMetricConnectionState::Reconnecting,
                ..
            }
        )
    }));
    assert!(recorded.iter().any(|metric| {
        matches!(
            metric,
            McpMetric::ReconnectAttempt {
                attempt: 1,
                outcome: McpMetricOutcome::Success,
                ..
            }
        )
    }));
}

#[tokio::test]
async fn mark_unhealthy_starts_reconnect_and_recovers() {
    let managed = managed_connection(
        policy(0),
        TestTransport::new(vec![
            Ok(TestConnection::default()),
            Ok(TestConnection::default()),
        ]),
        Arc::new(RecordingSink::default()),
    )
    .await;

    managed
        .mark_unhealthy("cancel acknowledgement timed out".into())
        .await
        .expect("mark unhealthy");

    wait_for_ready(&managed).await;
    assert_eq!(managed.attempts_so_far(), 1);
}

#[tokio::test]
async fn mark_unhealthy_cannot_reopen_a_closed_connection() {
    let managed = managed_connection(
        policy(0),
        TestTransport::new(vec![Ok(TestConnection::default())]),
        Arc::new(RecordingSink::default()),
    )
    .await;
    managed.shutdown().await.expect("shutdown");

    managed
        .mark_unhealthy("late cancellation timeout".into())
        .await
        .expect("late mark unhealthy");

    assert_eq!(managed.state().await, McpConnectionState::Closed);
}

#[tokio::test]
async fn shutdown_cancels_pending_recovered_schema_probe_and_shuts_candidate_down() {
    let reconnect_started = Arc::new(Notify::new());
    let release_reconnect = Arc::new(Notify::new());
    let schema_probe_started = Arc::new(Notify::new());
    let candidate_shutdown_started = Arc::new(Notify::new());
    let release_candidate_shutdown = Arc::new(Notify::new());
    let candidate_shutdowns = Arc::new(AtomicUsize::new(0));
    let sink = Arc::new(RecordingSink::default());
    let transport = GatedReconnectTransport {
        outcomes: Arc::new(Mutex::new(VecDeque::from([
            Ok(TestConnection::with_results(vec![Err(
                McpError::Connection("lost".into()),
            )])),
            Ok(TestConnection {
                list_tools_started: Some(Arc::clone(&schema_probe_started)),
                list_tools_pending: true,
                shutdown_started: Some(Arc::clone(&candidate_shutdown_started)),
                shutdown_release: Some(Arc::clone(&release_candidate_shutdown)),
                shutdowns: Arc::clone(&candidate_shutdowns),
                ..Default::default()
            }),
        ]))),
        attempts: Arc::new(AtomicUsize::new(0)),
        reconnect_started: Arc::clone(&reconnect_started),
        release_reconnect: Arc::clone(&release_reconnect),
    };
    let managed = ManagedMcpConnection::connect(
        Arc::new(transport),
        spec(policy(0)),
        McpServerScope::Session(SessionId::new()),
        sink.clone(),
    )
    .await
    .expect("managed connection");

    assert!(managed.call_tool("search", json!({})).await.is_err());
    reconnect_started.notified().await;
    release_reconnect.notify_one();
    schema_probe_started.notified().await;
    let first_shutdown_managed = managed.clone();
    let second_shutdown_managed = managed.clone();
    let mut first_shutdown = tokio::spawn(async move { first_shutdown_managed.shutdown().await });
    let mut second_shutdown = tokio::spawn(async move { second_shutdown_managed.shutdown().await });
    candidate_shutdown_started.notified().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut first_shutdown)
            .await
            .is_err(),
        "first managed shutdown must wait for candidate cleanup"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut second_shutdown)
            .await
            .is_err(),
        "concurrent managed shutdown must wait for candidate cleanup"
    );
    release_candidate_shutdown.notify_one();
    tokio::time::timeout(Duration::from_millis(100), &mut first_shutdown)
        .await
        .expect("first managed shutdown must finish after candidate cleanup")
        .expect("first shutdown task")
        .expect("first shutdown");
    tokio::time::timeout(Duration::from_millis(100), &mut second_shutdown)
        .await
        .expect("second managed shutdown must finish after candidate cleanup")
        .expect("second shutdown task")
        .expect("second shutdown");
    assert_eq!(managed.state().await, McpConnectionState::Closed);
    assert_eq!(managed.attempts_so_far(), 0);
    assert_eq!(candidate_shutdowns.load(Ordering::SeqCst), 1);
    assert!(!sink.events().iter().any(|event| {
        matches!(
            event,
            Event::McpConnectionRecovered(recovered) if !recovered.was_first
        )
    }));
}

#[tokio::test]
async fn managed_shutdown_propagates_candidate_cleanup_error() {
    let reconnect_started = Arc::new(Notify::new());
    let release_reconnect = Arc::new(Notify::new());
    let schema_probe_started = Arc::new(Notify::new());
    let candidate_shutdown_started = Arc::new(Notify::new());
    let release_candidate_shutdown = Arc::new(Notify::new());
    let transport = GatedReconnectTransport {
        outcomes: Arc::new(Mutex::new(VecDeque::from([
            Ok(TestConnection::with_results(vec![Err(
                McpError::Connection("lost".into()),
            )])),
            Ok(TestConnection {
                list_tools_started: Some(Arc::clone(&schema_probe_started)),
                list_tools_pending: true,
                shutdown_started: Some(Arc::clone(&candidate_shutdown_started)),
                shutdown_release: Some(Arc::clone(&release_candidate_shutdown)),
                shutdown_error: Some("candidate cleanup failed".into()),
                ..Default::default()
            }),
        ]))),
        attempts: Arc::new(AtomicUsize::new(0)),
        reconnect_started: Arc::clone(&reconnect_started),
        release_reconnect: Arc::clone(&release_reconnect),
    };
    let managed = ManagedMcpConnection::connect(
        Arc::new(transport),
        spec(policy(0)),
        McpServerScope::Session(SessionId::new()),
        Arc::new(RecordingSink::default()),
    )
    .await
    .expect("managed connection");

    assert!(managed.call_tool("search", json!({})).await.is_err());
    reconnect_started.notified().await;
    release_reconnect.notify_one();
    schema_probe_started.notified().await;

    let first_shutdown_managed = managed.clone();
    let first_shutdown = tokio::spawn(async move { first_shutdown_managed.shutdown().await });
    candidate_shutdown_started.notified().await;
    first_shutdown.abort();
    let _ = first_shutdown.await;

    let second_shutdown_managed = managed.clone();
    let mut second_shutdown = tokio::spawn(async move { second_shutdown_managed.shutdown().await });
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut second_shutdown)
            .await
            .is_err(),
        "a later shutdown must keep waiting after the first caller is cancelled"
    );
    release_candidate_shutdown.notify_one();

    let expected = Err(McpError::Connection("candidate cleanup failed".into()));
    assert_eq!(
        second_shutdown.await.expect("second shutdown task"),
        expected
    );
    assert_eq!(managed.shutdown().await, expected);
    assert_eq!(managed.state().await, McpConnectionState::Closed);
}

#[tokio::test]
async fn managed_shutdown_converts_candidate_cleanup_panic_to_cached_error() {
    let reconnect_started = Arc::new(Notify::new());
    let release_reconnect = Arc::new(Notify::new());
    let schema_probe_started = Arc::new(Notify::new());
    let transport = GatedReconnectTransport {
        outcomes: Arc::new(Mutex::new(VecDeque::from([
            Ok(TestConnection::with_results(vec![Err(
                McpError::Connection("lost".into()),
            )])),
            Ok(TestConnection {
                list_tools_started: Some(Arc::clone(&schema_probe_started)),
                list_tools_pending: true,
                shutdown_panics: true,
                ..Default::default()
            }),
        ]))),
        attempts: Arc::new(AtomicUsize::new(0)),
        reconnect_started: Arc::clone(&reconnect_started),
        release_reconnect: Arc::clone(&release_reconnect),
    };
    let managed = ManagedMcpConnection::connect(
        Arc::new(transport),
        spec(policy(0)),
        McpServerScope::Session(SessionId::new()),
        Arc::new(RecordingSink::default()),
    )
    .await
    .expect("managed connection");

    assert!(managed.call_tool("search", json!({})).await.is_err());
    reconnect_started.notified().await;
    release_reconnect.notify_one();
    schema_probe_started.notified().await;

    let first_error = tokio::time::timeout(Duration::from_millis(100), managed.shutdown())
        .await
        .expect("cleanup panic must not hang shutdown")
        .expect_err("cleanup panic must become an error");
    assert!(first_error.to_string().contains("panicked"));
    assert_eq!(managed.shutdown().await, Err(first_error));
}

#[tokio::test]
async fn managed_shutdown_converts_active_cleanup_panic_to_cached_error() {
    let managed = managed_connection(
        policy(0),
        TestTransport::new(vec![Ok(TestConnection {
            shutdown_panics: true,
            ..Default::default()
        })]),
        Arc::new(RecordingSink::default()),
    )
    .await;

    let first_error = tokio::time::timeout(Duration::from_millis(100), managed.shutdown())
        .await
        .expect("active cleanup panic must not hang shutdown")
        .expect_err("active cleanup panic must become an error");
    assert!(first_error.to_string().contains("panicked"));
    assert_eq!(managed.shutdown().await, Err(first_error));
}

#[tokio::test]
async fn shutdown_cancels_and_joins_a_pending_reconnect_attempt() {
    let reconnect_started = Arc::new(Notify::new());
    let reconnect_cancelled = Arc::new(AtomicUsize::new(0));
    let sink = Arc::new(RecordingSink::default());
    let transport = PendingReconnectTransport {
        attempts: Arc::new(AtomicUsize::new(0)),
        reconnect_started: Arc::clone(&reconnect_started),
        reconnect_cancelled: Arc::clone(&reconnect_cancelled),
    };
    let managed = ManagedMcpConnection::connect(
        Arc::new(transport),
        spec(policy(0)),
        McpServerScope::Session(SessionId::new()),
        sink.clone(),
    )
    .await
    .expect("managed connection");

    assert!(managed.call_tool("search", json!({})).await.is_err());
    reconnect_started.notified().await;
    tokio::time::timeout(Duration::from_millis(100), managed.shutdown())
        .await
        .expect("shutdown must cancel a pending reconnect")
        .expect("shutdown");

    assert_eq!(managed.state().await, McpConnectionState::Closed);
    assert_eq!(managed.attempts_so_far(), 0);
    assert_eq!(reconnect_cancelled.load(Ordering::SeqCst), 1);
    assert!(!sink.events().iter().any(|event| {
        matches!(
            event,
            Event::McpConnectionRecovered(recovered) if !recovered.was_first
        )
    }));
}

#[tokio::test]
async fn managed_connection_reports_schema_changed_after_reconnect() {
    let notify = Arc::new(Notify::new());
    let sink = Arc::new(RecordingSink::default());
    let managed = managed_connection(
        policy(0),
        TestTransport::new(vec![
            Ok(TestConnection::with_schema_and_results(
                json!({ "type": "object" }),
                vec![Err(McpError::Connection("lost".into()))],
            )),
            Ok(TestConnection::with_schema_and_results(
                json!({
                    "type": "object",
                    "required": ["query"],
                    "properties": { "query": { "type": "string" } }
                }),
                vec![Ok(McpToolResult::text("after"))],
            )),
        ])
        .with_attempt_notify(notify.clone()),
        sink.clone(),
    )
    .await;
    let mut changes = managed
        .subscribe_changes()
        .await
        .expect("change stream should subscribe");
    managed.list_tools().await.expect("initial schema snapshot");

    assert!(managed.call_tool("search", json!({})).await.is_err());
    notify.notified().await;
    wait_for_ready(&managed).await;
    let change = tokio::time::timeout(Duration::from_millis(100), changes.next())
        .await
        .expect("schema change should notify")
        .expect("change stream should yield");

    assert_eq!(change, McpChange::ToolsListChanged);
    assert!(sink.events().iter().any(|event| {
        matches!(
            event,
            Event::McpConnectionRecovered(recovered)
                if !recovered.was_first
                    && recovered.attempts_used == 1
                    && recovered.schema_changed
        )
    }));
}

#[tokio::test]
async fn managed_connection_terminal_failure_after_max_attempts() {
    let notify = Arc::new(Notify::new());
    let sink = Arc::new(RecordingSink::default());
    let managed = managed_connection(
        policy(1),
        TestTransport::new(vec![
            Ok(TestConnection::with_results(vec![Err(
                McpError::Connection("lost".into()),
            )])),
            Err(McpError::Connection("still down".into())),
        ])
        .with_attempt_notify(notify.clone()),
        sink.clone(),
    )
    .await;

    assert!(managed.call_tool("search", json!({})).await.is_err());
    notify.notified().await;
    wait_for_failed(&managed).await;

    assert!(matches!(
        managed.call_tool("search", json!({})).await,
        Err(McpError::Connection(_))
    ));
    assert!(sink.events().iter().any(|event| {
        matches!(
            event,
            Event::McpConnectionLost(lost)
                if lost.terminal && lost.attempts_so_far == 1
        )
    }));
}

#[tokio::test]
async fn managed_connection_resets_attempts_after_success_reset_window() {
    let notify = Arc::new(Notify::new());
    let sink = Arc::new(RecordingSink::default());
    let mut reconnect = policy(0);
    reconnect.success_reset_after = Duration::from_millis(10);
    let managed = managed_connection(
        reconnect,
        TestTransport::new(vec![
            Ok(TestConnection::with_results(vec![Err(
                McpError::Connection("lost".into()),
            )])),
            Ok(TestConnection::with_results(vec![Ok(McpToolResult::text(
                "after",
            ))])),
        ])
        .with_attempt_notify(notify.clone()),
        sink,
    )
    .await;

    assert!(managed.call_tool("search", json!({})).await.is_err());
    notify.notified().await;
    wait_for_ready(&managed).await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    assert_eq!(managed.attempts_so_far(), 0);
}

#[tokio::test]
async fn registry_add_managed_server_injects_tools_after_initial_connect() {
    let registry = McpRegistry::new();
    let spec = spec(policy(0));
    let server_id = spec.server_id.clone();
    registry
        .add_managed_server(
            spec,
            McpServerScope::Session(SessionId::new()),
            Arc::new(TestTransport::new(vec![Ok(TestConnection {
                tools: vec![tool("search")],
                ..Default::default()
            })])),
            Arc::new(RecordingSink::default()),
        )
        .await
        .expect("managed server registered");

    let tool_registry = ToolRegistry::builder().build().expect("tool registry");
    let injected = registry
        .inject_tools_into(&tool_registry, &server_id)
        .await
        .expect("tools inject");

    assert_eq!(injected, vec!["mcp__reconnect__search"]);
    assert_eq!(
        tool_registry
            .snapshot()
            .descriptor("mcp__reconnect__search")
            .expect("descriptor exists")
            .trust_level,
        TrustLevel::AdminTrusted
    );
}

fn policy(max_attempts: u32) -> ReconnectPolicy {
    ReconnectPolicy {
        max_attempts,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(1),
        backoff_jitter: 0.0,
        success_reset_after: Duration::from_secs(60),
        keep_deferred_during_reconnect: true,
    }
}

fn spec(reconnect: ReconnectPolicy) -> McpServerSpec {
    let mut spec = McpServerSpec::new(
        McpServerId("reconnect".into()),
        "Reconnect",
        TransportChoice::InProcess,
        McpServerSource::Workspace,
    );
    spec.reconnect = reconnect;
    spec
}

async fn managed_connection(
    reconnect: ReconnectPolicy,
    transport: TestTransport,
    sink: Arc<RecordingSink>,
) -> ManagedMcpConnection {
    ManagedMcpConnection::connect(
        Arc::new(transport),
        spec(reconnect),
        McpServerScope::Session(SessionId::new()),
        sink,
    )
    .await
    .expect("managed connection")
}

async fn wait_for_ready(managed: &ManagedMcpConnection) {
    wait_for_state(managed, |state| matches!(state, McpConnectionState::Ready)).await;
}

async fn wait_for_reconnecting(managed: &ManagedMcpConnection) {
    wait_for_state(managed, |state| {
        matches!(state, McpConnectionState::Reconnecting { .. })
    })
    .await;
}

async fn wait_for_failed(managed: &ManagedMcpConnection) {
    wait_for_state(managed, |state| {
        matches!(state, McpConnectionState::Failed { .. })
    })
    .await;
}

async fn wait_for_state(
    managed: &ManagedMcpConnection,
    predicate: impl Fn(&McpConnectionState) -> bool,
) {
    for _ in 0..100 {
        let state = managed.state().await;
        if predicate(&state) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("state did not converge: {:?}", managed.state().await);
}

fn tool(name: &str) -> McpToolDescriptor {
    tool_with_schema(name, json!({ "type": "object" }))
}

fn tool_with_schema(name: &str, input_schema: Value) -> McpToolDescriptor {
    McpToolDescriptor {
        name: name.into(),
        title: None,
        icons: None,
        execution: None,
        description: Some(format!("{name} tool")),
        input_schema,
        output_schema: None,
        annotations: None,
        meta: BTreeMap::new(),
    }
}

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<Event>>,
}

impl RecordingSink {
    fn events(&self) -> Vec<Event> {
        self.events.lock().clone()
    }
}

impl McpEventSink for RecordingSink {
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

#[derive(Clone)]
struct TestTransport {
    outcomes: Arc<Mutex<VecDeque<Result<TestConnection, McpError>>>>,
    attempt_notify: Option<Arc<Notify>>,
}

#[derive(Clone)]
struct GatedReconnectTransport {
    outcomes: Arc<Mutex<VecDeque<Result<TestConnection, McpError>>>>,
    attempts: Arc<AtomicUsize>,
    reconnect_started: Arc<Notify>,
    release_reconnect: Arc<Notify>,
}

#[derive(Clone)]
struct PendingReconnectTransport {
    attempts: Arc<AtomicUsize>,
    reconnect_started: Arc<Notify>,
    reconnect_cancelled: Arc<AtomicUsize>,
}

struct PendingReconnectGuard(Arc<AtomicUsize>);

impl Drop for PendingReconnectGuard {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl McpTransport for PendingReconnectTransport {
    fn transport_id(&self) -> &'static str {
        "pending-reconnect-test"
    }

    async fn connect(&self, _spec: McpServerSpec) -> Result<Arc<dyn McpConnection>, McpError> {
        if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            return Ok(Arc::new(TestConnection::with_results(vec![Err(
                McpError::Connection("lost".into()),
            )])));
        }

        let _guard = PendingReconnectGuard(Arc::clone(&self.reconnect_cancelled));
        self.reconnect_started.notify_one();
        futures::future::pending::<()>().await;
        unreachable!("pending reconnect is cancelled by shutdown")
    }
}

#[async_trait]
impl McpTransport for GatedReconnectTransport {
    fn transport_id(&self) -> &'static str {
        "gated-test"
    }

    async fn connect(&self, _spec: McpServerSpec) -> Result<Arc<dyn McpConnection>, McpError> {
        if self.attempts.fetch_add(1, Ordering::SeqCst) > 0 {
            self.reconnect_started.notify_one();
            self.release_reconnect.notified().await;
        }
        self.outcomes
            .lock()
            .pop_front()
            .unwrap_or_else(|| Err(McpError::Connection("no test outcome".into())))
            .map(|connection| Arc::new(connection) as Arc<dyn McpConnection>)
    }
}

impl TestTransport {
    fn new(outcomes: Vec<Result<TestConnection, McpError>>) -> Self {
        Self {
            outcomes: Arc::new(Mutex::new(VecDeque::from(outcomes))),
            attempt_notify: None,
        }
    }

    fn with_attempt_notify(mut self, notify: Arc<Notify>) -> Self {
        self.attempt_notify = Some(notify);
        self
    }
}

#[async_trait]
impl McpTransport for TestTransport {
    fn transport_id(&self) -> &'static str {
        "test"
    }

    async fn connect(&self, _spec: McpServerSpec) -> Result<Arc<dyn McpConnection>, McpError> {
        if let Some(notify) = &self.attempt_notify {
            notify.notify_waiters();
        }
        self.outcomes
            .lock()
            .pop_front()
            .unwrap_or_else(|| Err(McpError::Connection("no test outcome".into())))
            .map(|connection| Arc::new(connection) as Arc<dyn McpConnection>)
    }
}

#[derive(Default)]
struct TestConnection {
    tools: Vec<McpToolDescriptor>,
    results: Mutex<VecDeque<Result<McpToolResult, McpError>>>,
    list_tools_started: Option<Arc<Notify>>,
    list_tools_pending: bool,
    shutdown_started: Option<Arc<Notify>>,
    shutdown_release: Option<Arc<Notify>>,
    shutdown_error: Option<String>,
    shutdown_panics: bool,
    shutdowns: Arc<AtomicUsize>,
}

impl TestConnection {
    fn with_results(results: Vec<Result<McpToolResult, McpError>>) -> Self {
        Self {
            tools: vec![tool("search")],
            results: Mutex::new(VecDeque::from(results)),
            ..Default::default()
        }
    }

    fn with_schema_and_results(
        schema: Value,
        results: Vec<Result<McpToolResult, McpError>>,
    ) -> Self {
        Self {
            tools: vec![tool_with_schema("search", schema)],
            results: Mutex::new(VecDeque::from(results)),
            ..Default::default()
        }
    }
}

#[async_trait]
impl McpConnection for TestConnection {
    fn connection_id(&self) -> &'static str {
        "test-connection"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        if let Some(started) = &self.list_tools_started {
            started.notify_one();
        }
        if self.list_tools_pending {
            futures::future::pending::<()>().await;
        }
        Ok(self.tools.clone())
    }

    async fn call_tool(&self, _name: &str, _args: Value) -> Result<McpToolResult, McpError> {
        self.results
            .lock()
            .pop_front()
            .unwrap_or_else(|| Ok(McpToolResult::text("ok")))
    }

    async fn subscribe_changes(&self) -> Result<ListChangedEvent, McpError> {
        Ok(Box::pin(futures::stream::iter([
            McpChange::ToolsListChanged,
        ])))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
        if let Some(started) = &self.shutdown_started {
            started.notify_one();
        }
        if let Some(release) = &self.shutdown_release {
            release.notified().await;
        }
        if let Some(error) = &self.shutdown_error {
            return Err(McpError::Connection(error.clone()));
        }
        assert!(!self.shutdown_panics, "candidate shutdown panic");
        Ok(())
    }
}

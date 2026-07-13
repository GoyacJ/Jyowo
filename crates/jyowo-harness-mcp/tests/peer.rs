use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier as ThreadBarrier, OnceLock,
    },
    time::Duration,
};

use async_trait::async_trait;
use harness_mcp::{
    ClientTasksCapability, ElicitationClientCapability, ElicitationRequestRouter,
    EmptyClientCapability, InitializeResult, JsonRpcError, JsonRpcNotification, JsonRpcRequest,
    McpClientCapabilities, McpError, McpExpectedCapabilities, McpImplementation, McpInboundOutcome,
    McpMessage, McpMessageSink, McpOrderedNotificationHandler, McpOutboundMessage, McpPeer,
    McpRoot, McpRootsListHandler, McpServerCapabilities, McpSession, RootsClientCapability,
    SamplingClientCapability, SamplingRequestRouter, ToolsServerCapability,
};
use serde_json::{json, Value};
use tokio::sync::{mpsc, Barrier, Notify};

struct ChannelSink {
    sender: mpsc::UnboundedSender<McpOutboundMessage>,
}

struct OrderedNotificationHandler {
    calls: Arc<AtomicUsize>,
}

struct PanickingOrderedNotificationHandler;

impl McpOrderedNotificationHandler for OrderedNotificationHandler {
    fn handle_notification(&self, _notification: JsonRpcNotification) -> Result<(), McpError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

impl McpOrderedNotificationHandler for PanickingOrderedNotificationHandler {
    fn handle_notification(&self, _notification: JsonRpcNotification) -> Result<(), McpError> {
        panic!("fixture ordered notification panic")
    }
}

#[async_trait]
impl McpMessageSink for ChannelSink {
    async fn send(&self, message: McpOutboundMessage) -> Result<(), McpError> {
        self.sender
            .send(message)
            .map_err(|_| McpError::Connection("message sink closed".to_owned()))
    }
}

struct StalledSink;

#[async_trait]
impl McpMessageSink for StalledSink {
    async fn send(&self, _message: McpOutboundMessage) -> Result<(), McpError> {
        std::future::pending().await
    }
}

struct GatedRequestSink {
    request_started: Arc<Notify>,
    release_request: Arc<Notify>,
    sent: mpsc::UnboundedSender<McpOutboundMessage>,
}

#[async_trait]
impl McpMessageSink for GatedRequestSink {
    async fn send(&self, message: McpOutboundMessage) -> Result<(), McpError> {
        if matches!(message.as_message(), McpMessage::Request(_)) {
            self.request_started.notify_one();
            self.release_request.notified().await;
        }
        self.sent
            .send(message)
            .map_err(|_| McpError::Connection("message sink closed".to_owned()))
    }
}

struct FailingResponseSink {
    sent: mpsc::UnboundedSender<McpOutboundMessage>,
    sends: AtomicUsize,
}

struct PanickingResponseSink {
    sent: mpsc::UnboundedSender<McpOutboundMessage>,
    sends: AtomicUsize,
}

struct FailingCancellationSink {
    sent: mpsc::UnboundedSender<McpOutboundMessage>,
}

struct PanickingCancellationSink {
    sent: mpsc::UnboundedSender<McpOutboundMessage>,
}

#[async_trait]
impl McpMessageSink for FailingResponseSink {
    async fn send(&self, message: McpOutboundMessage) -> Result<(), McpError> {
        if self.sends.fetch_add(1, Ordering::SeqCst) == 0 {
            return self
                .sent
                .send(message)
                .map_err(|_| McpError::Connection("message sink closed".to_owned()));
        }
        Err(McpError::Transport("fixture failure".to_owned()))
    }
}

#[async_trait]
impl McpMessageSink for PanickingResponseSink {
    async fn send(&self, message: McpOutboundMessage) -> Result<(), McpError> {
        if self.sends.fetch_add(1, Ordering::SeqCst) == 0 {
            return self
                .sent
                .send(message)
                .map_err(|_| McpError::Connection("message sink closed".to_owned()));
        }
        panic!("fixture sink panic")
    }
}

#[async_trait]
impl McpMessageSink for FailingCancellationSink {
    async fn send(&self, message: McpOutboundMessage) -> Result<(), McpError> {
        if matches!(
            message.as_message(),
            McpMessage::Notification(notification)
                if notification.method == "notifications/cancelled"
        ) {
            return Err(McpError::Transport(
                "fixture cancellation failure".to_owned(),
            ));
        }
        self.sent
            .send(message)
            .map_err(|_| McpError::Connection("message sink closed".to_owned()))
    }
}

#[async_trait]
impl McpMessageSink for PanickingCancellationSink {
    async fn send(&self, message: McpOutboundMessage) -> Result<(), McpError> {
        if matches!(
            message.as_message(),
            McpMessage::Notification(notification)
                if notification.method == "notifications/cancelled"
        ) {
            panic!("fixture cancellation panic")
        }
        self.sent
            .send(message)
            .map_err(|_| McpError::Connection("message sink closed".to_owned()))
    }
}

fn ready_session(capabilities: McpClientCapabilities) -> McpSession {
    ready_session_at("2025-11-25", capabilities)
}

fn ready_session_at(protocol_version: &str, capabilities: McpClientCapabilities) -> McpSession {
    let mut session = McpSession::new(
        McpExpectedCapabilities::default(),
        capabilities,
        McpImplementation::new("peer-test", "1.0.0"),
    );
    session.begin_initialization().unwrap();
    session
        .accept_initialize_result(InitializeResult {
            protocol_version: protocol_version.to_owned(),
            capabilities: McpServerCapabilities {
                tools: Some(ToolsServerCapability::default()),
                ..Default::default()
            },
            server_info: McpImplementation::new("server", "1.0.0"),
            instructions: None,
            extra: Default::default(),
        })
        .unwrap();
    session.mark_initialized_notification_sent().unwrap();
    session
}

fn test_peer(
    capabilities: McpClientCapabilities,
) -> (McpPeer, mpsc::UnboundedReceiver<McpOutboundMessage>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(capabilities),
    )
    .build()
    .unwrap();
    (peer, receiver)
}

fn incoming(value: Value) -> McpMessage {
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn peer_initializes_new_session_and_sends_initialized_without_params() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let session = McpSession::new(
        McpExpectedCapabilities::default(),
        McpClientCapabilities::default(),
        McpImplementation::new("peer-test", "1.0.0"),
    );
    let peer = McpPeer::builder(Arc::new(ChannelSink { sender }), session)
        .build()
        .unwrap();
    let initializing_peer = peer.clone();
    let initialize =
        tokio::spawn(async move { initializing_peer.initialize(Duration::from_secs(1)).await });

    let message = outbound.recv().await.unwrap();
    let McpMessage::Request(request) = message.as_message() else {
        panic!("expected initialize request")
    };
    assert_eq!(request.method, "initialize");
    assert_eq!(
        request.params.as_ref().unwrap()["protocolVersion"],
        "2025-11-25"
    );
    peer.receive(incoming(json!({
        "jsonrpc": "2.0",
        "id": request.id,
        "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "server", "version": "1.0.0" }
        }
    })))
    .await
    .unwrap();

    let message = outbound.recv().await.unwrap();
    let McpMessage::Notification(notification) = message.as_message() else {
        panic!("expected initialized notification")
    };
    assert_eq!(notification.method, "notifications/initialized");
    assert!(notification.params.is_none());
    initialize.await.unwrap().unwrap();
    assert_eq!(
        peer.session().unwrap().negotiated_protocol_version(),
        Some("2025-06-18")
    );
}

#[tokio::test]
async fn new_peer_rejects_application_requests_before_initialization() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        McpSession::new(
            McpExpectedCapabilities::default(),
            McpClientCapabilities::default(),
            McpImplementation::new("peer-test", "1.0.0"),
        ),
    )
    .build()
    .unwrap();

    let error = peer
        .request("tools/list", json!({}), Duration::from_millis(10))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("not ready"));
    assert!(outbound.try_recv().is_err());
}

#[tokio::test]
async fn pending_handle_exposes_peer_owned_request_id_and_waits_for_response() {
    let (peer, mut outbound) = test_peer(McpClientCapabilities::default());
    let starting_peer = peer.clone();
    let start = tokio::spawn(async move {
        starting_peer
            .start_request("tools/call", json!({}), Duration::from_secs(1))
            .await
    });
    let message = outbound.recv().await.unwrap();
    let McpMessage::Request(request) = message.as_message() else {
        panic!("expected request")
    };
    let handle = start.await.unwrap().unwrap();
    assert_eq!(handle.request_id(), &request.id);

    peer.receive(incoming(json!({
        "jsonrpc": "2.0",
        "id": request.id,
        "result": { "content": [] }
    })))
    .await
    .unwrap();
    assert_eq!(handle.wait().await.unwrap(), json!({ "content": [] }));
}

#[tokio::test]
async fn transport_can_fail_one_committed_request_without_closing_the_peer() {
    let (peer, mut outbound) = test_peer(McpClientCapabilities::default());
    let starting_peer = peer.clone();
    let start = tokio::spawn(async move {
        starting_peer
            .start_request("tools/call", json!({}), Duration::from_secs(1))
            .await
    });
    let message = outbound.recv().await.unwrap();
    let McpMessage::Request(request) = message.as_message() else {
        panic!("expected request")
    };
    let request_id = request.id.clone();
    let handle = start.await.unwrap().unwrap();

    assert!(peer
        .fail_request(
            &request_id,
            McpError::Transport("SSE stream failed".to_owned())
        )
        .unwrap());
    assert_eq!(
        handle.wait().await.unwrap_err(),
        McpError::Transport("SSE stream failed".to_owned())
    );
    assert!(!peer
        .fail_request(&request_id, McpError::Transport("late failure".to_owned()))
        .unwrap());
    assert_eq!(peer.pending_count(), 0);
}

#[tokio::test]
async fn pending_handle_can_build_params_from_its_peer_owned_request_id() {
    let (peer, mut outbound) = test_peer(McpClientCapabilities::default());
    let starting_peer = peer.clone();
    let start = tokio::spawn(async move {
        starting_peer
            .start_request_with("tools/call", Duration::from_secs(1), |request_id| {
                Some(json!({
                    "name": "slow",
                    "arguments": {},
                    "_meta": { "progressToken": request_id }
                }))
            })
            .await
    });
    let message = outbound.recv().await.unwrap();
    let McpMessage::Request(request) = message.as_message() else {
        panic!("expected request")
    };
    assert_eq!(
        request.params.as_ref().unwrap()["_meta"]["progressToken"],
        request.id
    );
    let handle = start.await.unwrap().unwrap();
    peer.receive(incoming(json!({
        "jsonrpc": "2.0",
        "id": request.id,
        "result": {}
    })))
    .await
    .unwrap();
    handle.wait().await.unwrap();
}

#[tokio::test]
async fn ordered_notification_handler_completes_before_receive_returns() {
    let (sender, _outbound) = mpsc::unbounded_channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities::default()),
    )
    .ordered_notification_handler(
        "notifications/progress",
        Arc::new(OrderedNotificationHandler {
            calls: calls.clone(),
        }),
    )
    .build()
    .unwrap();

    peer.receive(incoming(json!({
        "jsonrpc": "2.0",
        "method": "notifications/progress",
        "params": { "progressToken": 1, "progress": 1 }
    })))
    .await
    .unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ordered_notification_panic_becomes_an_error_that_closes_pending_requests() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities::default()),
    )
    .ordered_notification_handler(
        "notifications/progress",
        Arc::new(PanickingOrderedNotificationHandler),
    )
    .build()
    .unwrap();
    let requester = peer.clone();
    let pending = tokio::spawn(async move {
        requester
            .request("tools/list", json!({}), Duration::from_secs(10))
            .await
    });
    outbound.recv().await.expect("pending request committed");

    let reader_peer = peer.clone();
    let reader = tokio::spawn(async move {
        if let Err(error) = reader_peer
            .receive(incoming(json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": { "progressToken": 1, "progress": 1 }
            })))
            .await
        {
            reader_peer
                .close(format!("stdio inbound routing failed: {error}"))
                .await;
        }
    });
    reader.await.expect("reader task must not panic");

    let error = tokio::time::timeout(Duration::from_millis(100), pending)
        .await
        .expect("pending request must wake")
        .expect("pending task")
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("ordered notification handler panicked"));
}

#[tokio::test]
async fn outbound_response_resolves_the_matching_pending_request() {
    let (peer, mut outbound) = test_peer(McpClientCapabilities::default());
    let requester = peer.clone();
    let request = tokio::spawn(async move {
        requester
            .request("tools/list", json!({}), Duration::from_secs(1))
            .await
    });

    let message = outbound.recv().await.unwrap();
    let McpMessage::Request(request_message) = message.as_message() else {
        panic!("expected request")
    };
    assert_eq!(request_message.method, "tools/list");

    let outcome = peer
        .receive(incoming(json!({
            "jsonrpc": "2.0",
            "id": request_message.id,
            "result": { "tools": [] }
        })))
        .await
        .unwrap();

    assert_eq!(outcome, McpInboundOutcome::ResponseResolved);
    assert_eq!(request.await.unwrap().unwrap(), json!({ "tools": [] }));
    assert_eq!(peer.pending_count(), 0);
}

#[tokio::test]
async fn outbound_error_preserves_complete_jsonrpc_error() {
    let (peer, mut outbound) = test_peer(McpClientCapabilities::default());
    let requester = peer.clone();
    let request = tokio::spawn(async move {
        requester
            .request("tools/call", json!({}), Duration::from_secs(1))
            .await
    });
    let message = outbound.recv().await.unwrap();
    let McpMessage::Request(request_message) = message.as_message() else {
        panic!("expected request")
    };

    peer.receive(incoming(json!({
        "jsonrpc": "2.0",
        "id": request_message.id,
        "error": {
            "code": -32042,
            "message": "elicitation required",
            "data": { "request_id": "request-1" },
            "vendorError": true
        }
    })))
    .await
    .unwrap();

    let McpError::RemoteJsonRpc(error) = request.await.unwrap().unwrap_err() else {
        panic!("expected structured JSON-RPC error")
    };
    assert_eq!(error.code, -32042);
    assert_eq!(error.data, Some(json!({ "request_id": "request-1" })));
    assert_eq!(error.extra.get("vendorError"), Some(&json!(true)));
}

struct SamplingHandler {
    calls: Arc<AtomicUsize>,
    barrier: Arc<Barrier>,
}

struct BlockingSamplingHandler {
    started: Arc<Notify>,
    release: Arc<Notify>,
}

struct CountingBlockingSamplingHandler {
    calls: Arc<AtomicUsize>,
    started: Arc<Notify>,
    release: Arc<Notify>,
}

struct AbortAwareSamplingHandler {
    started: Arc<Notify>,
    dropped: Arc<Notify>,
}

struct DropRaceSamplingHandler {
    started: Arc<Notify>,
    release: Arc<ThreadBarrier>,
}

struct SelfClosingSamplingHandler {
    peer: Arc<OnceLock<McpPeer>>,
    close_returned: Arc<Notify>,
}

struct OtherPeerClosingSamplingHandler {
    peer: McpPeer,
    close_returned: Arc<Notify>,
}

struct PanickingSamplingHandler;

struct DropSignal(Arc<Notify>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

#[async_trait]
impl SamplingRequestRouter for BlockingSamplingHandler {
    async fn route_sampling_request(
        &self,
        _request: JsonRpcRequest,
    ) -> Result<Value, JsonRpcError> {
        self.started.notify_one();
        self.release.notified().await;
        Ok(json!({ "model": "fixture" }))
    }
}

#[async_trait]
impl SamplingRequestRouter for CountingBlockingSamplingHandler {
    async fn route_sampling_request(
        &self,
        _request: JsonRpcRequest,
    ) -> Result<Value, JsonRpcError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.started.notify_one();
        self.release.notified().await;
        Ok(json!({ "model": "fixture" }))
    }
}

#[async_trait]
impl SamplingRequestRouter for AbortAwareSamplingHandler {
    async fn route_sampling_request(
        &self,
        _request: JsonRpcRequest,
    ) -> Result<Value, JsonRpcError> {
        let _drop_signal = DropSignal(Arc::clone(&self.dropped));
        self.started.notify_one();
        std::future::pending().await
    }
}

#[async_trait]
impl SamplingRequestRouter for DropRaceSamplingHandler {
    async fn route_sampling_request(
        &self,
        _request: JsonRpcRequest,
    ) -> Result<Value, JsonRpcError> {
        self.started.notify_one();
        self.release.wait();
        Ok(json!({ "model": "fixture" }))
    }
}

#[async_trait]
impl SamplingRequestRouter for SelfClosingSamplingHandler {
    async fn route_sampling_request(
        &self,
        _request: JsonRpcRequest,
    ) -> Result<Value, JsonRpcError> {
        self.peer
            .get()
            .expect("peer fixture was not initialized")
            .close("closed by handler")
            .await;
        self.close_returned.notify_one();
        Ok(json!({ "model": "fixture" }))
    }
}

#[async_trait]
impl SamplingRequestRouter for OtherPeerClosingSamplingHandler {
    async fn route_sampling_request(
        &self,
        _request: JsonRpcRequest,
    ) -> Result<Value, JsonRpcError> {
        self.peer.close("closed by another peer handler").await;
        self.close_returned.notify_one();
        Ok(json!({ "model": "fixture" }))
    }
}

#[async_trait]
impl SamplingRequestRouter for PanickingSamplingHandler {
    async fn route_sampling_request(
        &self,
        _request: JsonRpcRequest,
    ) -> Result<Value, JsonRpcError> {
        panic!("fixture handler panic")
    }
}

#[async_trait]
impl SamplingRequestRouter for SamplingHandler {
    async fn route_sampling_request(&self, request: JsonRpcRequest) -> Result<Value, JsonRpcError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.barrier.wait().await;
        Ok(json!({ "model": "fixture", "echo": request.params }))
    }
}

struct ElicitationHandler {
    calls: Arc<AtomicUsize>,
    barrier: Arc<Barrier>,
}

#[async_trait]
impl ElicitationRequestRouter for ElicitationHandler {
    async fn route_elicitation_request(
        &self,
        request: JsonRpcRequest,
    ) -> Result<Value, JsonRpcError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.barrier.wait().await;
        Ok(json!({ "action": "accept", "content": request.params }))
    }
}

struct RootsHandler {
    calls: Arc<AtomicUsize>,
    barrier: Arc<Barrier>,
}

#[async_trait]
impl McpRootsListHandler for RootsHandler {
    async fn list_roots(&self) -> Result<Vec<McpRoot>, JsonRpcError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.barrier.wait().await;
        Ok(vec![McpRoot {
            uri: "file:///workspace".to_owned(),
            name: Some("workspace".to_owned()),
        }])
    }
}

#[tokio::test]
async fn concurrent_server_requests_call_handlers_and_send_responses() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(3));
    let capabilities = McpClientCapabilities {
        sampling: Some(SamplingClientCapability::default()),
        elicitation: Some(ElicitationClientCapability {
            form: Some(EmptyClientCapability::default()),
            ..Default::default()
        }),
        roots: Some(RootsClientCapability::default()),
        ..Default::default()
    };
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(capabilities),
    )
    .sampling_handler(Arc::new(SamplingHandler {
        calls: Arc::clone(&calls),
        barrier: Arc::clone(&barrier),
    }))
    .elicitation_handler(Arc::new(ElicitationHandler {
        calls: Arc::clone(&calls),
        barrier: Arc::clone(&barrier),
    }))
    .roots_handler(Arc::new(RootsHandler {
        calls: Arc::clone(&calls),
        barrier,
    }))
    .build()
    .unwrap();

    let requests = [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "ping", "params": {} }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "sampling/createMessage", "params": { "messages": [] } }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "elicitation/create", "params": { "message": "name?" } }),
        json!({ "jsonrpc": "2.0", "id": 4, "method": "roots/list", "params": {} }),
    ];
    let mut tasks = Vec::new();
    for request in requests {
        let peer = peer.clone();
        tasks.push(tokio::spawn(async move {
            peer.receive(incoming(request)).await.unwrap()
        }));
    }
    for task in tasks {
        assert_eq!(task.await.unwrap(), McpInboundOutcome::RequestHandled);
    }

    let mut responses = BTreeMap::new();
    for _ in 0..4 {
        let message = outbound.recv().await.unwrap();
        let value = serde_json::to_value(message.as_message()).unwrap();
        responses.insert(value["id"].as_u64().unwrap(), value);
    }
    assert_eq!(responses[&1]["result"], json!({}));
    assert_eq!(responses[&2]["result"]["model"], "fixture");
    assert_eq!(responses[&3]["result"]["action"], "accept");
    assert_eq!(
        responses[&4]["result"]["roots"][0]["uri"],
        "file:///workspace"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn blocked_server_handler_does_not_block_an_unrelated_response() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities {
            sampling: Some(SamplingClientCapability::default()),
            ..Default::default()
        }),
    )
    .sampling_handler(Arc::new(BlockingSamplingHandler {
        started: Arc::clone(&started),
        release: Arc::clone(&release),
    }))
    .build()
    .unwrap();

    let requester = peer.clone();
    let pending = tokio::spawn(async move {
        requester
            .request("tools/list", json!({}), Duration::from_secs(1))
            .await
    });
    let request = outbound.recv().await.unwrap();
    let McpMessage::Request(request) = request.as_message() else {
        panic!("expected request")
    };

    tokio::time::timeout(
        Duration::from_millis(50),
        peer.receive(incoming(json!({
            "jsonrpc": "2.0", "id": 91, "method": "sampling/createMessage", "params": {}
        }))),
    )
    .await
    .expect("receive waited for the user handler")
    .unwrap();
    started.notified().await;

    peer.receive(incoming(json!({
        "jsonrpc": "2.0", "id": request.id, "result": { "tools": [] }
    })))
    .await
    .unwrap();
    assert_eq!(pending.await.unwrap().unwrap(), json!({ "tools": [] }));

    release.notify_one();
    let response = outbound.recv().await.unwrap();
    let value = serde_json::to_value(response.as_message()).unwrap();
    assert_eq!(value["id"], 91);
}

#[tokio::test]
async fn inbound_handler_concurrency_is_bounded() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities {
            sampling: Some(SamplingClientCapability::default()),
            ..Default::default()
        }),
    )
    .max_inbound_handlers(1)
    .sampling_handler(Arc::new(CountingBlockingSamplingHandler {
        calls: Arc::clone(&calls),
        started: Arc::clone(&started),
        release: Arc::clone(&release),
    }))
    .build()
    .unwrap();

    peer.receive(incoming(json!({
        "jsonrpc": "2.0", "id": 1, "method": "sampling/createMessage", "params": {}
    })))
    .await
    .unwrap();
    started.notified().await;

    let error = peer
        .receive(incoming(json!({
            "jsonrpc": "2.0", "id": 2, "method": "sampling/createMessage", "params": {}
        })))
        .await
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("inbound handler concurrency limit"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    release.notify_one();
    let response = outbound.recv().await.unwrap();
    let value = serde_json::to_value(response.as_message()).unwrap();
    assert_eq!(value["id"], 1);
}

#[tokio::test]
async fn close_aborts_managed_inbound_handlers() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let started = Arc::new(Notify::new());
    let dropped = Arc::new(Notify::new());
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities {
            sampling: Some(SamplingClientCapability::default()),
            ..Default::default()
        }),
    )
    .sampling_handler(Arc::new(AbortAwareSamplingHandler {
        started: Arc::clone(&started),
        dropped: Arc::clone(&dropped),
    }))
    .build()
    .unwrap();

    peer.receive(incoming(json!({
        "jsonrpc": "2.0", "id": 1, "method": "sampling/createMessage", "params": {}
    })))
    .await
    .unwrap();
    started.notified().await;

    tokio::time::timeout(Duration::from_millis(100), peer.close("test close"))
        .await
        .expect("close waited for an aborted inbound handler");
    tokio::time::timeout(Duration::from_millis(100), dropped.notified())
        .await
        .expect("aborted handler future was not dropped");
    assert!(outbound.try_recv().is_err());
}

#[tokio::test]
async fn dropping_last_peer_aborts_managed_inbound_handlers() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let started = Arc::new(Notify::new());
    let dropped = Arc::new(Notify::new());
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities {
            sampling: Some(SamplingClientCapability::default()),
            ..Default::default()
        }),
    )
    .sampling_handler(Arc::new(AbortAwareSamplingHandler {
        started: Arc::clone(&started),
        dropped: Arc::clone(&dropped),
    }))
    .build()
    .unwrap();

    peer.receive(incoming(json!({
        "jsonrpc": "2.0", "id": 1, "method": "sampling/createMessage", "params": {}
    })))
    .await
    .unwrap();
    started.notified().await;

    drop(peer);

    tokio::time::timeout(Duration::from_millis(100), dropped.notified())
        .await
        .expect("dropping the final peer did not abort its managed handler");
    assert!(outbound.try_recv().is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_last_peer_prevents_a_concurrently_completing_handler_response() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let started = Arc::new(Notify::new());
    let release = Arc::new(ThreadBarrier::new(2));
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities {
            sampling: Some(SamplingClientCapability::default()),
            ..Default::default()
        }),
    )
    .sampling_handler(Arc::new(DropRaceSamplingHandler {
        started: Arc::clone(&started),
        release: Arc::clone(&release),
    }))
    .build()
    .unwrap();

    peer.receive(incoming(json!({
        "jsonrpc": "2.0", "id": 1, "method": "sampling/createMessage", "params": {}
    })))
    .await
    .unwrap();
    started.notified().await;

    drop(peer);
    release.wait();

    let response = tokio::time::timeout(Duration::from_millis(100), outbound.recv()).await;
    assert!(
        !matches!(response, Ok(Some(_))),
        "handler sent a response after the final peer was dropped"
    );
}

#[tokio::test]
async fn inbound_handler_can_close_its_own_peer_without_deadlocking() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let peer_cell = Arc::new(OnceLock::new());
    let close_returned = Arc::new(Notify::new());
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities {
            sampling: Some(SamplingClientCapability::default()),
            ..Default::default()
        }),
    )
    .sampling_handler(Arc::new(SelfClosingSamplingHandler {
        peer: Arc::clone(&peer_cell),
        close_returned: Arc::clone(&close_returned),
    }))
    .build()
    .unwrap();
    peer_cell.set(peer.clone()).ok().unwrap();

    peer.receive(incoming(json!({
        "jsonrpc": "2.0", "id": 1, "method": "sampling/createMessage", "params": {}
    })))
    .await
    .unwrap();

    tokio::time::timeout(Duration::from_millis(100), close_returned.notified())
        .await
        .expect("handler deadlocked while closing its own peer");
    assert!(outbound.try_recv().is_err());
}

#[tokio::test]
async fn closing_another_peer_does_not_exclude_a_same_numbered_task() {
    let (peer_b_sender, _peer_b_outbound) = mpsc::unbounded_channel();
    let peer_b_started = Arc::new(Notify::new());
    let peer_b_dropped = Arc::new(Notify::new());
    let peer_b = McpPeer::builder(
        Arc::new(ChannelSink {
            sender: peer_b_sender,
        }),
        ready_session(McpClientCapabilities {
            sampling: Some(SamplingClientCapability::default()),
            ..Default::default()
        }),
    )
    .sampling_handler(Arc::new(AbortAwareSamplingHandler {
        started: Arc::clone(&peer_b_started),
        dropped: Arc::clone(&peer_b_dropped),
    }))
    .build()
    .unwrap();
    peer_b
        .receive(incoming(json!({
            "jsonrpc": "2.0", "id": 1, "method": "sampling/createMessage", "params": {}
        })))
        .await
        .unwrap();
    peer_b_started.notified().await;

    let (peer_a_sender, _peer_a_outbound) = mpsc::unbounded_channel();
    let close_returned = Arc::new(Notify::new());
    let peer_a = McpPeer::builder(
        Arc::new(ChannelSink {
            sender: peer_a_sender,
        }),
        ready_session(McpClientCapabilities {
            sampling: Some(SamplingClientCapability::default()),
            ..Default::default()
        }),
    )
    .sampling_handler(Arc::new(OtherPeerClosingSamplingHandler {
        peer: peer_b,
        close_returned: Arc::clone(&close_returned),
    }))
    .build()
    .unwrap();
    peer_a
        .receive(incoming(json!({
            "jsonrpc": "2.0", "id": 1, "method": "sampling/createMessage", "params": {}
        })))
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_millis(100), close_returned.notified())
        .await
        .expect("other peer close did not return");
    tokio::time::timeout(Duration::from_millis(100), peer_b_dropped.notified())
        .await
        .expect("other peer close did not release its same-numbered handler");
}

#[tokio::test]
async fn background_sink_failure_closes_peer_and_wakes_pending_requests() {
    let (sent, mut outbound) = mpsc::unbounded_channel();
    let peer = McpPeer::builder(
        Arc::new(FailingResponseSink {
            sent,
            sends: AtomicUsize::new(0),
        }),
        ready_session(McpClientCapabilities::default()),
    )
    .build()
    .unwrap();
    let requester = peer.clone();
    let pending = tokio::spawn(async move {
        requester
            .request("tools/list", json!({}), Duration::from_secs(30))
            .await
    });
    outbound.recv().await.unwrap();

    peer.receive(incoming(json!({
        "jsonrpc": "2.0", "id": 1, "method": "ping", "params": {}
    })))
    .await
    .unwrap();

    let error = tokio::time::timeout(Duration::from_millis(100), pending)
        .await
        .expect("background sink failure did not wake the pending request")
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("fixture failure"));
    let error = peer
        .request("resources/list", json!({}), Duration::from_secs(1))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("closed"));
}

#[tokio::test]
async fn handler_panic_closes_peer_and_wakes_pending_requests() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities {
            sampling: Some(SamplingClientCapability::default()),
            ..Default::default()
        }),
    )
    .sampling_handler(Arc::new(PanickingSamplingHandler))
    .build()
    .unwrap();
    let requester = peer.clone();
    let pending = tokio::spawn(async move {
        requester
            .request("tools/list", json!({}), Duration::from_secs(30))
            .await
    });
    outbound.recv().await.unwrap();

    peer.receive(incoming(json!({
        "jsonrpc": "2.0", "id": 1, "method": "sampling/createMessage", "params": {}
    })))
    .await
    .unwrap();

    let error = tokio::time::timeout(Duration::from_millis(100), pending)
        .await
        .expect("handler panic did not wake the pending request")
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("fixture handler panic"));
}

#[tokio::test]
async fn background_sink_panic_closes_peer_and_wakes_pending_requests() {
    let (sent, mut outbound) = mpsc::unbounded_channel();
    let peer = McpPeer::builder(
        Arc::new(PanickingResponseSink {
            sent,
            sends: AtomicUsize::new(0),
        }),
        ready_session(McpClientCapabilities::default()),
    )
    .build()
    .unwrap();
    let requester = peer.clone();
    let pending = tokio::spawn(async move {
        requester
            .request("tools/list", json!({}), Duration::from_secs(30))
            .await
    });
    outbound.recv().await.unwrap();

    peer.receive(incoming(json!({
        "jsonrpc": "2.0", "id": 1, "method": "ping", "params": {}
    })))
    .await
    .unwrap();

    let error = tokio::time::timeout(Duration::from_millis(100), pending)
        .await
        .expect("background sink panic did not wake the pending request")
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("fixture sink panic"));
}

#[test]
fn builder_rejects_capability_and_handler_mismatches() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (sender, _outbound) = mpsc::unbounded_channel();
    let error = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities::default()),
    )
    .sampling_handler(Arc::new(SamplingHandler {
        calls: Arc::clone(&calls),
        barrier: Arc::new(Barrier::new(1)),
    }))
    .build()
    .err()
    .expect("unadvertised sampling handler was accepted");
    assert!(error.to_string().contains("sampling"));

    let (sender, _outbound) = mpsc::unbounded_channel();
    let error = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities {
            sampling: Some(SamplingClientCapability::default()),
            ..Default::default()
        }),
    )
    .build()
    .err()
    .expect("sampling capability without handler was accepted");
    assert!(error.to_string().contains("sampling"));

    let (sender, _outbound) = mpsc::unbounded_channel();
    let error = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities {
            roots: Some(RootsClientCapability::default()),
            ..Default::default()
        }),
    )
    .build()
    .err()
    .expect("roots capability without handler was accepted");
    assert!(error.to_string().contains("roots"));

    let (sender, _outbound) = mpsc::unbounded_channel();
    let error = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities {
            tasks: Some(ClientTasksCapability::default()),
            ..Default::default()
        }),
    )
    .build()
    .err()
    .expect("unwired tasks capability was accepted");
    assert!(error.to_string().contains("tasks"));
}

#[tokio::test]
async fn elicitation_request_is_gated_by_the_advertised_mode() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities {
            elicitation: Some(ElicitationClientCapability {
                form: Some(EmptyClientCapability::default()),
                url: None,
                ..Default::default()
            }),
            ..Default::default()
        }),
    )
    .elicitation_handler(Arc::new(ElicitationHandler {
        calls: Arc::clone(&calls),
        barrier: Arc::new(Barrier::new(1)),
    }))
    .build()
    .unwrap();

    peer.receive(incoming(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "elicitation/create",
        "params": { "mode": "url", "url": "https://example.com/continue" }
    })))
    .await
    .unwrap();

    let value = serde_json::to_value(outbound.recv().await.unwrap().as_message()).unwrap();
    assert_eq!(value["error"]["code"], -32601);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn elicitation_is_rejected_before_protocol_2025_06_18() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session_at(
            "2025-03-26",
            McpClientCapabilities {
                elicitation: Some(ElicitationClientCapability::default()),
                ..Default::default()
            },
        ),
    )
    .elicitation_handler(Arc::new(ElicitationHandler {
        calls: Arc::clone(&calls),
        barrier: Arc::new(Barrier::new(1)),
    }))
    .build()
    .unwrap();

    peer.receive(incoming(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "elicitation/create",
        "params": { "message": "name?", "requestedSchema": { "type": "object" } }
    })))
    .await
    .unwrap();

    let value = serde_json::to_value(outbound.recv().await.unwrap().as_message()).unwrap();
    assert_eq!(value["error"]["code"], -32601);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn protocol_2025_06_18_allows_legacy_form_elicitation() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session_at(
            "2025-06-18",
            McpClientCapabilities {
                elicitation: Some(ElicitationClientCapability::default()),
                ..Default::default()
            },
        ),
    )
    .elicitation_handler(Arc::new(ElicitationHandler {
        calls: Arc::clone(&calls),
        barrier: Arc::new(Barrier::new(1)),
    }))
    .build()
    .unwrap();

    peer.receive(incoming(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "elicitation/create",
        "params": { "message": "name?", "requestedSchema": { "type": "object" } }
    })))
    .await
    .unwrap();

    let value = serde_json::to_value(outbound.recv().await.unwrap().as_message()).unwrap();
    assert_eq!(value["result"]["action"], "accept");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

struct NotificationHandler(Arc<AtomicUsize>);

struct BlockingNotificationHandler {
    started: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl harness_mcp::McpNotificationHandler for BlockingNotificationHandler {
    async fn handle_notification(
        &self,
        _notification: JsonRpcNotification,
    ) -> Result<(), McpError> {
        self.started.notify_one();
        self.release.notified().await;
        Ok(())
    }
}

#[async_trait]
impl harness_mcp::McpNotificationHandler for NotificationHandler {
    async fn handle_notification(
        &self,
        _notification: JsonRpcNotification,
    ) -> Result<(), McpError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn notification_is_routed_without_touching_pending_requests() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities::default()),
    )
    .notification_handler(
        "notifications/tools/list_changed",
        Arc::new(NotificationHandler(Arc::clone(&calls))),
    )
    .build()
    .unwrap();
    let requester = peer.clone();
    let pending = tokio::spawn(async move {
        requester
            .request("tools/list", json!({}), Duration::from_secs(1))
            .await
    });
    let request = outbound.recv().await.unwrap();
    let McpMessage::Request(request) = request.as_message() else {
        panic!("expected request")
    };
    assert_eq!(peer.pending_count(), 1);

    let outcome = peer
        .receive(incoming(json!({
            "jsonrpc": "2.0",
            "method": "notifications/tools/list_changed",
            "params": {}
        })))
        .await
        .unwrap();
    assert_eq!(outcome, McpInboundOutcome::NotificationHandled);
    tokio::time::timeout(Duration::from_millis(100), async {
        while calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("notification handler did not run");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(peer.pending_count(), 1);

    peer.receive(incoming(json!({
        "jsonrpc": "2.0", "id": request.id, "result": {}
    })))
    .await
    .unwrap();
    pending.await.unwrap().unwrap();
}

#[tokio::test]
async fn blocked_notification_handler_does_not_block_an_unrelated_response() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities::default()),
    )
    .notification_handler(
        "notifications/test",
        Arc::new(BlockingNotificationHandler {
            started: Arc::clone(&started),
            release: Arc::clone(&release),
        }),
    )
    .build()
    .unwrap();
    let requester = peer.clone();
    let pending = tokio::spawn(async move {
        requester
            .request("tools/list", json!({}), Duration::from_secs(1))
            .await
    });
    let request = outbound.recv().await.unwrap();
    let McpMessage::Request(request) = request.as_message() else {
        panic!("expected request")
    };

    tokio::time::timeout(
        Duration::from_millis(50),
        peer.receive(incoming(json!({
            "jsonrpc": "2.0", "method": "notifications/test", "params": {}
        }))),
    )
    .await
    .expect("receive waited for the notification handler")
    .unwrap();
    started.notified().await;

    peer.receive(incoming(json!({
        "jsonrpc": "2.0", "id": request.id, "result": { "tools": [] }
    })))
    .await
    .unwrap();
    assert_eq!(pending.await.unwrap().unwrap(), json!({ "tools": [] }));
    release.notify_one();
}

#[tokio::test]
async fn pending_limit_and_close_are_bounded_and_wake_waiters() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities::default()),
    )
    .max_pending(1)
    .build()
    .unwrap();
    let requester = peer.clone();
    let first = tokio::spawn(async move {
        requester
            .request("tools/list", json!({}), Duration::from_secs(30))
            .await
    });
    outbound.recv().await.unwrap();

    let error = peer
        .request("resources/list", json!({}), Duration::from_secs(1))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("pending request limit"));
    peer.close("test close").await;
    let error = first.await.unwrap().unwrap_err();
    assert!(error.to_string().contains("test close"));
    assert_eq!(peer.pending_count(), 0);
}

#[tokio::test]
async fn closed_peer_rejects_late_inbound_work_without_calling_handlers() {
    let (sender, mut outbound) = mpsc::unbounded_channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let peer = McpPeer::builder(
        Arc::new(ChannelSink { sender }),
        ready_session(McpClientCapabilities::default()),
    )
    .notification_handler(
        "notifications/test",
        Arc::new(NotificationHandler(Arc::clone(&calls))),
    )
    .build()
    .unwrap();
    peer.close("closed by test").await;

    let error = peer
        .receive(incoming(json!({
            "jsonrpc": "2.0", "method": "notifications/test", "params": {}
        })))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("closed"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(outbound.try_recv().is_err());
}

#[tokio::test]
async fn timeout_removes_pending_and_sends_cancelled_notification() {
    let (peer, mut outbound) = test_peer(McpClientCapabilities::default());
    let requester = peer.clone();
    let task = tokio::spawn(async move {
        requester
            .request(
                "tools/call",
                json!({ "name": "slow" }),
                Duration::from_millis(20),
            )
            .await
    });
    let request = outbound.recv().await.unwrap();
    let McpMessage::Request(request) = request.as_message() else {
        panic!("expected request")
    };
    let cancel = outbound.recv().await.unwrap();
    let McpMessage::Notification(cancel) = cancel.as_message() else {
        panic!("expected cancellation notification")
    };
    assert_eq!(cancel.method, "notifications/cancelled");
    assert_eq!(cancel.params.as_ref().unwrap()["requestId"], request.id);
    assert!(task
        .await
        .unwrap()
        .unwrap_err()
        .to_string()
        .contains("timed out"));
    assert_eq!(peer.pending_count(), 0);
}

#[tokio::test]
async fn request_deadline_covers_a_stalled_transport_write() {
    let peer = McpPeer::builder(
        Arc::new(StalledSink),
        ready_session(McpClientCapabilities::default()),
    )
    .build()
    .unwrap();

    let result = tokio::time::timeout(
        Duration::from_millis(100),
        peer.request("tools/list", json!({}), Duration::from_millis(20)),
    )
    .await
    .expect("peer request exceeded its own deadline");

    assert!(result.unwrap_err().to_string().contains("timed out"));
    assert_eq!(peer.pending_count(), 0);
}

#[tokio::test]
async fn close_wakes_request_even_while_transport_write_is_stalled() {
    let peer = McpPeer::builder(
        Arc::new(StalledSink),
        ready_session(McpClientCapabilities::default()),
    )
    .build()
    .unwrap();
    let requester = peer.clone();
    let task = tokio::spawn(async move {
        requester
            .request("tools/list", json!({}), Duration::from_secs(30))
            .await
    });
    while peer.pending_count() == 0 {
        tokio::task::yield_now().await;
    }

    peer.close("closed during write").await;
    let result = tokio::time::timeout(Duration::from_millis(100), task)
        .await
        .expect("close did not wake the stalled request")
        .unwrap();

    assert!(result
        .unwrap_err()
        .to_string()
        .contains("closed during write"));
}

#[tokio::test]
async fn aborted_request_future_removes_its_pending_entry() {
    let (peer, mut outbound) = test_peer(McpClientCapabilities::default());
    let requester = peer.clone();
    let task = tokio::spawn(async move {
        requester
            .request(
                "tools/call",
                json!({ "name": "slow" }),
                Duration::from_secs(30),
            )
            .await
    });
    outbound.recv().await.unwrap();
    assert_eq!(peer.pending_count(), 1);

    task.abort();
    let _ = task.await;

    assert_eq!(peer.pending_count(), 0);
    let cancel = tokio::time::timeout(Duration::from_millis(100), outbound.recv())
        .await
        .expect("aborted request did not send cancellation")
        .unwrap();
    let McpMessage::Notification(cancel) = cancel.as_message() else {
        panic!("expected cancellation notification")
    };
    assert_eq!(cancel.method, "notifications/cancelled");
}

async fn assert_cancellation_sink_failure_closes_peer(
    sink: Arc<dyn McpMessageSink>,
    mut outbound: mpsc::UnboundedReceiver<McpOutboundMessage>,
    expected_error: &str,
) {
    let peer = McpPeer::builder(sink, ready_session(McpClientCapabilities::default()))
        .build()
        .unwrap();
    let first_requester = peer.clone();
    let first = tokio::spawn(async move {
        first_requester
            .request("tools/call", json!({}), Duration::from_secs(30))
            .await
    });
    let second_requester = peer.clone();
    let second = tokio::spawn(async move {
        second_requester
            .request("resources/list", json!({}), Duration::from_secs(30))
            .await
    });
    outbound.recv().await.unwrap();
    outbound.recv().await.unwrap();
    assert_eq!(peer.pending_count(), 2);

    first.abort();
    let _ = first.await;

    let error = tokio::time::timeout(Duration::from_millis(100), second)
        .await
        .expect("cancellation sink failure did not wake another pending request")
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains(expected_error));
    assert_eq!(peer.pending_count(), 0);
}

#[tokio::test]
async fn cancellation_sink_error_closes_peer_and_wakes_other_pending_requests() {
    let (sent, outbound) = mpsc::unbounded_channel();
    assert_cancellation_sink_failure_closes_peer(
        Arc::new(FailingCancellationSink { sent }),
        outbound,
        "fixture cancellation failure",
    )
    .await;
}

#[tokio::test]
async fn cancellation_sink_panic_closes_peer_and_wakes_other_pending_requests() {
    let (sent, outbound) = mpsc::unbounded_channel();
    assert_cancellation_sink_failure_closes_peer(
        Arc::new(PanickingCancellationSink { sent }),
        outbound,
        "fixture cancellation panic",
    )
    .await;
}

#[tokio::test]
async fn abort_before_sink_commit_does_not_send_cancellation() {
    let request_started = Arc::new(Notify::new());
    let release_request = Arc::new(Notify::new());
    let (sent, mut outbound) = mpsc::unbounded_channel();
    let peer = McpPeer::builder(
        Arc::new(GatedRequestSink {
            request_started: Arc::clone(&request_started),
            release_request: Arc::clone(&release_request),
            sent,
        }),
        ready_session(McpClientCapabilities::default()),
    )
    .build()
    .unwrap();
    let requester = peer.clone();
    let task = tokio::spawn(async move {
        requester
            .request("tools/list", json!({}), Duration::from_secs(30))
            .await
    });
    request_started.notified().await;

    task.abort();
    let _ = task.await;

    assert_eq!(peer.pending_count(), 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), outbound.recv())
            .await
            .is_err()
    );
    release_request.notify_one();
}

#[tokio::test]
async fn duplicate_and_unknown_responses_are_safely_ignored() {
    let (peer, _outbound) = test_peer(McpClientCapabilities::default());
    let response = incoming(json!({ "jsonrpc": "2.0", "id": 77, "result": {} }));
    assert_eq!(
        peer.receive(response.clone()).await.unwrap(),
        McpInboundOutcome::UnknownResponse
    );
    assert_eq!(
        peer.receive(response).await.unwrap(),
        McpInboundOutcome::UnknownResponse
    );
}

#[test]
fn outbound_boundary_rejects_invalid_params_and_results() {
    assert!(McpOutboundMessage::request(1, "ping", json!([])).is_err());
    assert!(McpOutboundMessage::success(1, json!("not-an-object")).is_err());
    assert!(McpOutboundMessage::notification("notifications/test", Value::Null).is_err());
}

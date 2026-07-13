#![cfg_attr(not(feature = "http"), allow(dead_code))]

use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicU32, AtomicU64, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::Duration,
};

use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt};
use harness_contracts::PermissionMode;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    StatusCode,
};
use serde_json::Value;
use tokio::{
    sync::{broadcast, mpsc, oneshot, watch, Mutex, Semaphore},
    task::{JoinHandle, JoinSet},
};
use tokio_stream::wrappers::BroadcastStream;

#[cfg(test)]
use super::network_endpoint::validate_network_address;
use super::network_endpoint::{
    normalize_endpoint_host, normalize_endpoint_host_key, resolve_network_endpoint,
    NetworkHostKind, ParsedNetworkEndpoint,
};

use crate::{
    authorize_mcp_transport_connect, call_tool_params, client_auth,
    continue_after_elicitation_params, decode_empty_result, decode_list_prompts,
    decode_list_resources, decode_list_tools, decode_prompt_messages, decode_read_resource,
    decode_tool_result, get_prompt_params, notification_change, pagination_params,
    read_resource_params, resource_subscription_params, ElicitationHandler, JsonRpcNotification,
    JsonRpcResponse, ListChangedEvent, McpChange, McpClientCapabilities, McpConnectContext,
    McpConnection, McpError, McpImplementation, McpListPage, McpMessage, McpMessageSink,
    McpOrderedNotificationHandler, McpOutboundMessage, McpPeer, McpPrompt, McpPromptMessages,
    McpReadResourceResult, McpResource, McpServerSpec, McpSession, McpToolDescriptor,
    McpToolResult, McpTransport, NoopMcpMetricsSink, ReconnectPolicy, SseDecoder, SseLimits,
    TransportChoice,
};

const OUTBOUND_CAPACITY: usize = 64;
const MAX_POST_WORKERS: usize = 16;
const MAX_POST_STREAM_WORKERS: usize = 16;
const MAX_JSON_BODY: usize = 4 * 1024 * 1024;
const MCP_SESSION_ID: &str = "mcp-session-id";
const MCP_PROTOCOL_VERSION: &str = "mcp-protocol-version";

pub struct HttpTransport {
    metrics_sink: Arc<dyn crate::McpMetricsSink>,
    pinned_resolutions: Vec<(String, Vec<SocketAddr>)>,
}

impl HttpTransport {
    pub fn new() -> Self {
        Self {
            metrics_sink: Arc::new(NoopMcpMetricsSink),
            pinned_resolutions: Vec::new(),
        }
    }

    pub fn with_metrics_sink(metrics_sink: Arc<dyn crate::McpMetricsSink>) -> Self {
        Self {
            metrics_sink,
            pinned_resolutions: Vec::new(),
        }
    }

    pub fn with_redirects_disabled(self) -> Self {
        self
    }

    pub fn with_pinned_resolution(
        mut self,
        host: impl Into<String>,
        addrs: Vec<SocketAddr>,
    ) -> Self {
        let host = host.into();
        let host = normalize_endpoint_host_key(&host).unwrap_or(host);
        self.pinned_resolutions.push((host, addrs));
        self
    }
}

impl Default for HttpTransport {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_http_endpoint(raw: &str) -> Result<ParsedNetworkEndpoint, McpError> {
    let mut url = reqwest::Url::parse(raw)
        .map_err(|_| McpError::Protocol("invalid MCP HTTP endpoint URL".to_owned()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(McpError::Protocol(
            "MCP HTTP endpoint must use http or https".to_owned(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(McpError::Protocol(
            "MCP HTTP endpoint must not contain userinfo".to_owned(),
        ));
    }
    let (host, kind) = normalize_endpoint_host(
        url.host()
            .ok_or_else(|| McpError::Protocol("MCP HTTP endpoint has no host".to_owned()))?,
    )?;
    if matches!(kind, NetworkHostKind::Localhost | NetworkHostKind::DnsName) {
        url.set_host(Some(&host))
            .map_err(|_| McpError::Protocol("invalid MCP HTTP endpoint host".to_owned()))?;
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| McpError::Protocol("MCP HTTP endpoint has no valid port".to_owned()))?;
    Ok(ParsedNetworkEndpoint {
        url,
        host,
        port,
        kind,
    })
}

fn is_transport_owned_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "accept"
            | "authorization"
            | "content-length"
            | "content-type"
            | "host"
            | "last-event-id"
            | "mcp-protocol-version"
            | "mcp-session-id"
            | "proxy-authorization"
            | "transfer-encoding"
    )
}

pub(super) async fn prepare_http_endpoint(
    raw_url: &str,
    headers: std::collections::BTreeMap<String, String>,
    pinned_resolutions: &[(String, Vec<SocketAddr>)],
) -> Result<(reqwest::Url, reqwest::Client), McpError> {
    let endpoint = parse_http_endpoint(raw_url)?;
    let mut default_headers = HeaderMap::new();
    for (key, value) in headers {
        let name = HeaderName::try_from(key.as_str())
            .map_err(|error| McpError::Transport(error.to_string()))?;
        if is_transport_owned_header(&name) {
            return Err(McpError::Protocol(format!(
                "HTTP header {name} is owned by the MCP transport"
            )));
        }
        let value = HeaderValue::try_from(value.as_str())
            .map_err(|error| McpError::Transport(error.to_string()))?;
        default_headers.insert(name, value);
    }
    let resolved = resolve_network_endpoint(
        &endpoint,
        pinned_resolutions
            .iter()
            .rev()
            .find(|(host, _)| host.eq_ignore_ascii_case(&endpoint.host))
            .map(|(_, addrs)| addrs.as_slice()),
    )
    .await?;
    let client = reqwest::Client::builder()
        .default_headers(default_headers)
        .pool_max_idle_per_host(0)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .resolve_to_addrs(&endpoint.host, &resolved)
        .build()
        .map_err(|error| McpError::Transport(error.to_string()))?;
    Ok((endpoint.url, client))
}

#[async_trait]
impl McpTransport for HttpTransport {
    fn transport_id(&self) -> &'static str {
        "http"
    }

    async fn connect(&self, spec: McpServerSpec) -> Result<Arc<dyn McpConnection>, McpError> {
        self.connect_with_context(spec, McpConnectContext::default())
            .await
    }

    async fn connect_with_context(
        &self,
        spec: McpServerSpec,
        context: McpConnectContext,
    ) -> Result<Arc<dyn McpConnection>, McpError> {
        authorize_mcp_transport_connect(&context, &spec).await?;
        let TransportChoice::Http { url, headers } = spec.transport.clone() else {
            return Err(McpError::Unsupported(
                "HttpTransport requires TransportChoice::Http".into(),
            ));
        };
        let (endpoint, client) =
            prepare_http_endpoint(&url, headers, &self.pinned_resolutions).await?;
        let auth_provider = client_auth::McpClientAuthProvider::new(&spec.auth)
            .with_metrics_sink(context.metrics_sink_or(Arc::clone(&self.metrics_sink)))
            .with_lifecycle_events(
                spec.server_id.clone(),
                self.transport_id(),
                Arc::clone(&context.event_sink),
            );
        let (state, _) = watch::channel(HttpConnectionState::Starting);
        let (changes, _) = broadcast::channel(64);
        let connection = Arc::new(HttpConnection {
            connection_id: format!("http:{}", spec.server_id.0),
            endpoint: endpoint.to_string(),
            client,
            auth_provider,
            expected: spec.capabilities_expected,
            timeouts: spec.timeouts,
            reconnect: spec.reconnect,
            elicitation_handler: context.elicitation_handler.clone(),
            permission_mode: context.permission_mode,
            state,
            changes,
            reinitialize: Mutex::new(()),
            session_expiries: SessionExpiryBudget::new(spec.reconnect.max_attempts),
            next_generation_id: AtomicU64::new(1),
            lifetime_cancel: watch::channel(false).0,
        });
        match connection.install_generation().await {
            Ok(_) => Ok(Arc::new(HttpConnectionHandle { inner: connection })),
            Err(McpError::StreamableHttpUnavailable(400 | 404 | 405)) => {
                connection.lifetime_cancel.send_replace(true);
                super::sse::connect_prepared(
                    format!("http:{}", spec.server_id.0),
                    spec,
                    context,
                    endpoint,
                    connection.client.clone(),
                    connection.auth_provider.clone(),
                )
                .await
            }
            Err(error) => Err(error),
        }
    }
}

struct HttpConnectionHandle {
    inner: Arc<HttpConnection>,
}

impl Drop for HttpConnectionHandle {
    fn drop(&mut self) {
        self.inner.lifetime_cancel.send_replace(true);
    }
}

impl std::ops::Deref for HttpConnectionHandle {
    type Target = Arc<HttpConnection>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

struct HttpConnection {
    connection_id: String,
    endpoint: String,
    client: reqwest::Client,
    auth_provider: client_auth::McpClientAuthProvider,
    expected: crate::McpExpectedCapabilities,
    timeouts: crate::McpTimeouts,
    reconnect: ReconnectPolicy,
    elicitation_handler: Option<Arc<dyn ElicitationHandler>>,
    permission_mode: PermissionMode,
    state: watch::Sender<HttpConnectionState>,
    changes: broadcast::Sender<McpChange>,
    reinitialize: Mutex<()>,
    session_expiries: SessionExpiryBudget,
    next_generation_id: AtomicU64,
    lifetime_cancel: watch::Sender<bool>,
}

#[derive(Clone)]
enum HttpConnectionState {
    Starting,
    Ready(Arc<HttpGeneration>),
    Rebuilding { expired_generation: u64 },
    Failed(McpError),
    Closed,
}

struct SessionExpiryBudget {
    consecutive: AtomicU32,
    limit: u32,
}

impl SessionExpiryBudget {
    fn new(configured_limit: u32) -> Self {
        Self {
            consecutive: AtomicU32::new(0),
            limit: configured_limit.clamp(1, 8),
        }
    }

    fn record(&self) -> Option<u32> {
        let attempt = self
            .consecutive
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        (attempt <= self.limit).then_some(attempt)
    }

    fn reset(&self) {
        self.consecutive.store(0, Ordering::Relaxed);
    }
}

struct HttpGeneration {
    id: u64,
    peer: McpPeer,
    headers: Arc<StdMutex<SessionHeaders>>,
    cancel: watch::Sender<bool>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

#[derive(Default)]
struct SessionHeaders {
    session_id: Option<String>,
    protocol_version: Option<String>,
}

struct HttpNotificationHandler {
    changes: broadcast::Sender<McpChange>,
}

impl McpOrderedNotificationHandler for HttpNotificationHandler {
    fn handle_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        if let Some(change) =
            notification_change(&notification.method, notification.params.as_ref())
        {
            let _ = self.changes.send(change);
        }
        Ok(())
    }
}

struct OutboundEnvelope {
    message: McpOutboundMessage,
    committed: oneshot::Sender<Result<(), McpError>>,
    deadline: tokio::time::Instant,
}

struct PostSseStart {
    response: reqwest::Response,
    target: Value,
    cancel: watch::Receiver<bool>,
}

struct PostSseJob {
    connection: Arc<HttpConnection>,
    generation: Arc<HttpGeneration>,
    peer: McpPeer,
    start: PostSseStart,
    deadline: tokio::time::Instant,
    request_completed: Option<watch::Receiver<bool>>,
    lifetime_cancel: watch::Receiver<bool>,
}

struct HttpMessageSink {
    outbound: mpsc::Sender<OutboundEnvelope>,
    handshake_timeout: Duration,
    call_timeout: Duration,
}

#[async_trait]
impl McpMessageSink for HttpMessageSink {
    async fn send(&self, message: McpOutboundMessage) -> Result<(), McpError> {
        let timeout = match message.as_message() {
            McpMessage::Request(request) if request.method == "initialize" => {
                self.handshake_timeout
            }
            _ => self.call_timeout,
        };
        let permit = self
            .outbound
            .reserve()
            .await
            .map_err(|_| McpError::Connection("MCP HTTP generation is closed".to_owned()))?;
        let (committed, receiver) = oneshot::channel();
        permit.send(OutboundEnvelope {
            message,
            committed,
            deadline: tokio::time::Instant::now() + timeout,
        });
        receiver
            .await
            .map_err(|_| McpError::Connection("MCP HTTP POST worker stopped".to_owned()))?
    }
}

impl HttpConnection {
    async fn install_generation(self: &Arc<Self>) -> Result<Arc<HttpGeneration>, McpError> {
        let lifetime_cancel = self.lifetime_cancel.subscribe();
        if *lifetime_cancel.borrow() {
            return Err(McpError::Connection(
                "MCP HTTP connection is closed".to_owned(),
            ));
        }
        let id = self.next_generation_id.fetch_add(1, Ordering::Relaxed);
        let (outbound, outbound_rx) = mpsc::channel(OUTBOUND_CAPACITY);
        let sink = Arc::new(HttpMessageSink {
            outbound,
            handshake_timeout: self.timeouts.handshake,
            call_timeout: self.timeouts.call_default,
        });
        let session = McpSession::new(
            self.expected.clone(),
            McpClientCapabilities::default(),
            McpImplementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
        );
        let notification_handler = Arc::new(HttpNotificationHandler {
            changes: self.changes.clone(),
        });
        let mut peer_builder = McpPeer::builder(sink, session);
        for method in [
            "tools/list_changed",
            "notifications/tools/list_changed",
            "resources/list_changed",
            "notifications/resources/list_changed",
            "resources/updated",
            "notifications/resources/updated",
            "prompts/list_changed",
            "notifications/prompts/list_changed",
            "notifications/cancelled",
            "notifications/progress",
        ] {
            peer_builder =
                peer_builder.ordered_notification_handler(method, notification_handler.clone());
        }
        let peer = peer_builder.build()?;
        let (cancel, cancel_rx) = watch::channel(false);
        let generation = Arc::new(HttpGeneration {
            id,
            peer: peer.clone(),
            headers: Arc::new(StdMutex::new(SessionHeaders::default())),
            cancel,
            tasks: Mutex::new(Vec::new()),
        });
        let dispatcher = tokio::spawn(run_dispatcher(
            Arc::clone(self),
            Arc::clone(&generation),
            peer,
            outbound_rx,
            cancel_rx,
            lifetime_cancel.clone(),
        ));
        generation.track_task(dispatcher).await;
        if let Err(error) = generation.peer.initialize(self.timeouts.handshake).await {
            generation
                .stop(format!("MCP HTTP initialize failed: {error}"))
                .await;
            return Err(error);
        }
        self.state
            .send_replace(HttpConnectionState::Ready(Arc::clone(&generation)));
        let get_task = tokio::spawn(run_get_channel(
            Arc::clone(self),
            Arc::clone(&generation),
            generation.cancel.subscribe(),
            lifetime_cancel,
        ));
        generation.track_task(get_task).await;
        Ok(generation)
    }

    async fn generation(&self) -> Result<Arc<HttpGeneration>, McpError> {
        let mut state = self.state.subscribe();
        loop {
            let snapshot = state.borrow().clone();
            match snapshot {
                HttpConnectionState::Ready(generation) => return Ok(generation),
                HttpConnectionState::Failed(error) => return Err(error),
                HttpConnectionState::Closed => {
                    return Err(McpError::Connection(
                        "MCP HTTP connection is closed".to_owned(),
                    ));
                }
                HttpConnectionState::Starting | HttpConnectionState::Rebuilding { .. } => {}
            }
            state
                .changed()
                .await
                .map_err(|_| McpError::Connection("MCP HTTP connection state closed".to_owned()))?;
        }
    }

    async fn request_value(
        self: &Arc<Self>,
        method: String,
        params: Option<Value>,
    ) -> Result<Value, McpError> {
        for attempt in 0..=1 {
            let generation = self.generation().await?;
            let result = generation
                .peer
                .request_optional(method.clone(), params.clone(), self.timeouts.call_default)
                .await;
            let session_expired = should_retry_session_request(&result, attempt);
            let stale_connection = attempt == 0
                && matches!(result, Err(McpError::Connection(_)))
                && self.generation_is_stale(generation.id);
            if matches!(&result, Ok(_) | Err(McpError::RemoteJsonRpc(_))) {
                self.reset_session_expiries_after_success(generation.id);
            }
            if !session_expired && !stale_connection {
                return result;
            }
            if session_expired {
                self.reinitialize_generation(generation.id).await?;
            }
            if !is_safe_session_retry(&method) {
                return Err(McpError::SessionExpired);
            }
        }
        unreachable!()
    }

    fn generation_is_stale(&self, generation_id: u64) -> bool {
        match self.state.borrow().clone() {
            HttpConnectionState::Ready(current) => current.id != generation_id,
            HttpConnectionState::Rebuilding { expired_generation } => {
                expired_generation == generation_id
            }
            HttpConnectionState::Starting
            | HttpConnectionState::Failed(_)
            | HttpConnectionState::Closed => false,
        }
    }

    fn reset_session_expiries_after_success(&self, generation_id: u64) {
        self.state.send_if_modified(|state| {
            if matches!(state, HttpConnectionState::Ready(current) if current.id == generation_id) {
                self.session_expiries.reset();
            }
            false
        });
    }

    async fn reinitialize_generation(self: &Arc<Self>, expired_id: u64) -> Result<(), McpError> {
        let current = match self.state.borrow().clone() {
            HttpConnectionState::Ready(current) if current.id == expired_id => Some(current),
            HttpConnectionState::Ready(_) => return Ok(()),
            HttpConnectionState::Failed(error) => return Err(error),
            HttpConnectionState::Closed => {
                return Err(McpError::Connection(
                    "MCP HTTP connection is closed".to_owned(),
                ));
            }
            HttpConnectionState::Starting | HttpConnectionState::Rebuilding { .. } => None,
        };
        if let Some(current) = current {
            request_reinitialize(Arc::clone(self), current);
        }
        self.generation().await.map(|_| ())
    }

    async fn perform_reinitialize(self: &Arc<Self>, old: Arc<HttpGeneration>, attempt: u32) {
        let _transition = self.reinitialize.lock().await;
        let still_rebuilding = matches!(
            self.state.borrow().clone(),
            HttpConnectionState::Rebuilding { expired_generation }
                if expired_generation == old.id
        );
        if !still_rebuilding {
            return;
        }

        let mut lifetime_cancel = self.lifetime_cancel.subscribe();
        tokio::select! {
            _ = wait_for_cancel(&mut lifetime_cancel) => return,
            _ = tokio::time::sleep(self.reconnect.backoff_for_attempt(attempt)) => {}
        }

        old.stop("MCP HTTP session expired").await;
        if let Err(error) = self.install_generation().await {
            self.state.send_replace(HttpConnectionState::Failed(error));
        }
    }

    async fn request_response(
        self: &Arc<Self>,
        method: &str,
        params: Option<Value>,
    ) -> Result<JsonRpcResponse, McpError> {
        match self.request_value(method.to_owned(), params).await {
            Ok(result) => Ok(JsonRpcResponse::success(Value::Null, result)),
            Err(McpError::RemoteJsonRpc(error)) => Ok(JsonRpcResponse::failure(Value::Null, error)),
            Err(error) => Err(error),
        }
    }

    async fn send_with_elicitation(
        self: &Arc<Self>,
        method: &str,
        params: Option<Value>,
    ) -> Result<JsonRpcResponse, McpError> {
        let response = self.request_response(method, params.clone()).await?;
        if let Some(retry_params) = continue_after_elicitation_params(
            &response,
            method,
            params.as_ref(),
            self.elicitation_handler.as_ref(),
            self.permission_mode,
        )
        .await?
        {
            return self.request_response(method, Some(retry_params)).await;
        }
        Ok(response)
    }

    async fn send_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        let generation = self.generation().await?;
        match notification.params {
            Some(params) => generation.peer.notify(notification.method, params).await,
            None => {
                generation
                    .peer
                    .notify_without_params(notification.method)
                    .await
            }
        }
    }
}

fn should_retry_session_request(result: &Result<Value, McpError>, attempt: usize) -> bool {
    attempt == 0 && matches!(result, Err(McpError::SessionExpired))
}

fn is_safe_session_retry(method: &str) -> bool {
    matches!(
        method,
        "tools/list" | "resources/list" | "resources/read" | "prompts/list" | "prompts/get"
    )
}

impl HttpGeneration {
    async fn track_task(&self, task: JoinHandle<()>) {
        let mut tasks = self.tasks.lock().await;
        if *self.cancel.borrow() {
            task.abort();
        } else {
            tasks.push(task);
        }
    }

    async fn stop(&self, reason: impl Into<String>) {
        let _ = self.cancel.send(true);
        self.peer.close(reason).await;
        let mut tasks = self.tasks.lock().await;
        for task in tasks.drain(..) {
            task.abort();
        }
    }
}

async fn run_dispatcher(
    connection: Arc<HttpConnection>,
    generation: Arc<HttpGeneration>,
    peer: McpPeer,
    mut outbound: mpsc::Receiver<OutboundEnvelope>,
    mut cancel: watch::Receiver<bool>,
    mut lifetime_cancel: watch::Receiver<bool>,
) {
    let short_permits = Arc::new(Semaphore::new(MAX_POST_WORKERS));
    let stream_permits = Arc::new(Semaphore::new(MAX_POST_STREAM_WORKERS));
    let mut short_workers = JoinSet::new();
    let mut stream_workers = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = wait_for_cancel(&mut cancel) => break,
            _ = wait_for_cancel(&mut lifetime_cancel) => break,
            Some(result) = short_workers.join_next(), if !short_workers.is_empty() => {
                if let Ok(Some(job)) = result {
                    let stream_permits = Arc::clone(&stream_permits);
                    stream_workers.spawn(async move {
                        run_post_sse_job(job, stream_permits).await;
                    });
                }
            },
            Some(_) = stream_workers.join_next(), if !stream_workers.is_empty() => {},
            envelope = outbound.recv() => {
                let Some(envelope) = envelope else { break; };
                let permit = tokio::select! {
                    biased;
                    _ = wait_for_cancel(&mut cancel) => break,
                    _ = wait_for_cancel(&mut lifetime_cancel) => break,
                    permit = Arc::clone(&short_permits).acquire_owned() => permit,
                };
                let Ok(permit) = permit else { break; };
                let connection = Arc::clone(&connection);
                let generation = Arc::clone(&generation);
                let peer = peer.clone();
                let cancel = cancel.clone();
                let lifetime_cancel = lifetime_cancel.clone();
                short_workers.spawn(async move {
                    let _permit = permit;
                    post_message(
                        connection,
                        generation,
                        peer,
                        envelope,
                        cancel,
                        lifetime_cancel,
                    )
                    .await
                });
            }
        }
    }
    short_workers.shutdown().await;
    stream_workers.shutdown().await;
    peer.close("MCP HTTP dispatcher stopped").await;
}

async fn post_message(
    connection: Arc<HttpConnection>,
    generation: Arc<HttpGeneration>,
    peer: McpPeer,
    envelope: OutboundEnvelope,
    mut cancel: watch::Receiver<bool>,
    mut lifetime_cancel: watch::Receiver<bool>,
) -> Option<PostSseJob> {
    let deadline = envelope.deadline;
    let message = envelope.message.into_message();
    let expected_id = match &message {
        McpMessage::Request(request) => Some(request.id.clone()),
        _ => None,
    };
    let mut request_completed = match expected_id.as_ref() {
        Some(id) => peer.request_completion(id).ok().flatten(),
        None => None,
    };
    let mut operation = Box::pin(post_message_inner(
        Arc::clone(&connection),
        Arc::clone(&generation),
        peer.clone(),
        message,
        expected_id.clone(),
        envelope.committed,
        cancel.clone(),
    ));
    let outcome = tokio::select! {
        biased;
        result = &mut operation => Some(result),
        _ = wait_for_cancel(&mut cancel) => None,
        _ = wait_for_cancel(&mut lifetime_cancel) => None,
        _ = tokio::time::sleep_until(deadline) => {
            if let Some(id) = expected_id.as_ref() {
                let _ = peer.fail_request(
                    id,
                    McpError::Connection("MCP HTTP request timed out".to_owned()),
                );
            }
            None
        },
        _ = async {
            if let Some(completed) = request_completed.as_mut() {
                let _ = completed.changed().await;
            } else {
                std::future::pending::<()>().await;
            }
        } => None,
    };
    match outcome {
        Some(Ok(Some(start))) => Some(PostSseJob {
            connection,
            generation,
            peer,
            start,
            deadline,
            request_completed,
            lifetime_cancel,
        }),
        Some(Err(error)) => {
            if let Some(id) = expected_id.as_ref() {
                let _ = peer.fail_request(id, error);
            }
            None
        }
        Some(Ok(None)) | None => None,
    }
}

async fn run_post_sse_job(mut job: PostSseJob, permits: Arc<Semaphore>) {
    let target = job.start.target.clone();
    let permit = tokio::select! {
        biased;
        _ = wait_for_cancel(&mut job.start.cancel) => return,
        _ = wait_for_cancel(&mut job.lifetime_cancel) => return,
        _ = wait_for_request_completion(&mut job.request_completed) => return,
        _ = tokio::time::sleep_until(job.deadline) => {
            let _ = job.peer.fail_request(
                &target,
                McpError::Connection("MCP HTTP request timed out".to_owned()),
            );
            return;
        },
        permit = permits.acquire_owned() => permit,
    };
    let Ok(_permit) = permit else {
        return;
    };
    let mut operation = Box::pin(process_post_sse(
        job.connection,
        job.generation,
        job.peer.clone(),
        job.start.response,
        target.clone(),
        job.start.cancel.clone(),
    ));
    let outcome = tokio::select! {
        biased;
        result = &mut operation => Some(result),
        _ = wait_for_cancel(&mut job.start.cancel) => None,
        _ = wait_for_cancel(&mut job.lifetime_cancel) => None,
        _ = wait_for_request_completion(&mut job.request_completed) => None,
        _ = tokio::time::sleep_until(job.deadline) => {
            let _ = job.peer.fail_request(
                &target,
                McpError::Connection("MCP HTTP request timed out".to_owned()),
            );
            None
        },
    };
    if let Some(Err(error)) = outcome {
        let _ = job.peer.fail_request(&target, error);
    }
}

async fn wait_for_request_completion(completed: &mut Option<watch::Receiver<bool>>) {
    let Some(completed) = completed else {
        std::future::pending::<()>().await;
        return;
    };
    if *completed.borrow() {
        return;
    }
    let _ = completed.changed().await;
}

async fn post_message_inner(
    connection: Arc<HttpConnection>,
    generation: Arc<HttpGeneration>,
    peer: McpPeer,
    message: McpMessage,
    expected_id: Option<Value>,
    mut committed: oneshot::Sender<Result<(), McpError>>,
    mut cancel: watch::Receiver<bool>,
) -> Result<Option<PostSseStart>, McpError> {
    let initialize =
        matches!(&message, McpMessage::Request(request) if request.method == "initialize");
    let response = tokio::select! {
        biased;
        _ = committed.closed() => return Ok(None),
        _ = wait_for_cancel(&mut cancel) => return Ok(None),
        result = send_http(&connection, &generation, reqwest::Method::POST, Some(&message), initialize, None) => result,
    };
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            let _ = committed.send(Err(error));
            return Ok(None);
        }
    };
    if response.status() == StatusCode::NOT_FOUND && !initialize && has_session(&generation) {
        if let Some(id) = expected_id.as_ref() {
            let _ = peer.fail_request(id, McpError::SessionExpired);
            let _ = committed.send(Ok(()));
        } else {
            let _ = committed.send(Err(McpError::SessionExpired));
        }
        request_reinitialize(Arc::clone(&connection), Arc::clone(&generation));
        return Ok(None);
    }
    if initialize
        && matches!(
            response.status(),
            StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
        )
    {
        let _ = committed.send(Err(McpError::StreamableHttpUnavailable(
            response.status().as_u16(),
        )));
        return Ok(None);
    }
    if let Err(error) = validate_response_session(&generation, &response, initialize) {
        let _ = committed.send(Err(error));
        return Ok(None);
    }
    if expected_id.is_none() {
        let result = validate_accepted(response, &mut cancel).await;
        let _ = committed.send(result);
        return Ok(None);
    }
    let status = response.status();
    if !status.is_success() {
        let _ = committed.send(Err(McpError::Transport(format!(
            "MCP HTTP POST failed with status {status}"
        ))));
        return Ok(None);
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    if content_type.eq_ignore_ascii_case("application/json") {
        let result =
            process_json_response(response, &peer, expected_id.as_ref().unwrap(), &mut cancel)
                .await;
        let _ = committed.send(result);
    } else if content_type.eq_ignore_ascii_case("text/event-stream") {
        let _ = committed.send(Ok(()));
        return Ok(Some(PostSseStart {
            response,
            target: expected_id.unwrap(),
            cancel,
        }));
    } else {
        let _ = committed.send(Err(McpError::InvalidResponse(format!(
            "MCP HTTP request returned unsupported Content-Type {content_type:?}"
        ))));
    }
    Ok(None)
}

async fn send_http(
    connection: &HttpConnection,
    generation: &HttpGeneration,
    method: reqwest::Method,
    message: Option<&McpMessage>,
    initialize: bool,
    last_event_id: Option<&str>,
) -> Result<reqwest::Response, McpError> {
    for attempt in 0..=1 {
        let mut request = connection
            .client
            .request(method.clone(), &connection.endpoint);
        request = if method == reqwest::Method::GET {
            request.header(ACCEPT, "text/event-stream")
        } else if method == reqwest::Method::DELETE {
            request
        } else {
            request.header(ACCEPT, "application/json, text/event-stream")
        };
        if !initialize {
            let mut headers = generation.headers.lock().map_err(|_| {
                McpError::Connection("MCP HTTP session header lock poisoned".to_owned())
            })?;
            if headers.protocol_version.is_none() {
                headers.protocol_version = generation
                    .peer
                    .session()?
                    .negotiated_protocol_version()
                    .map(str::to_owned);
            }
            if let Some(session_id) = &headers.session_id {
                request = request.header(MCP_SESSION_ID, session_id);
            }
            if let Some(protocol_version) = &headers.protocol_version {
                request = request.header(MCP_PROTOCOL_VERSION, protocol_version);
            }
        }
        if let Some(last_event_id) = last_event_id {
            request = request.header("last-event-id", last_event_id);
        }
        if let Some(message) = message {
            request = request.json(message);
        }
        let authorization = if attempt == 0 {
            connection.auth_provider.authorization_header().await?
        } else {
            connection
                .auth_provider
                .force_refresh_authorization_header()
                .await?
        };
        if let Some(authorization) = authorization {
            request = request.header(AUTHORIZATION, authorization);
        }
        let response = request
            .send()
            .await
            .map_err(|error| McpError::Transport(sanitize_reqwest_error(&error)))?;
        if attempt == 0
            && is_auth_expired(response.status())
            && connection.auth_provider.can_refresh()
        {
            continue;
        }
        return Ok(response);
    }
    unreachable!()
}

pub(super) fn sanitize_reqwest_error(error: &reqwest::Error) -> String {
    if let Some(status) = error.status() {
        format!("HTTP request failed with status {status}")
    } else if error.is_timeout() {
        "HTTP request timed out".to_owned()
    } else if error.is_connect() {
        "HTTP connection failed".to_owned()
    } else {
        "HTTP transport request failed".to_owned()
    }
}

fn is_auth_expired(status: StatusCode) -> bool {
    matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
}

fn has_session(generation: &HttpGeneration) -> bool {
    generation
        .headers
        .lock()
        .is_ok_and(|headers| headers.session_id.is_some())
}

fn validate_response_session(
    generation: &HttpGeneration,
    response: &reqwest::Response,
    initialize: bool,
) -> Result<(), McpError> {
    let received = parse_response_session_id(response.headers())?;
    let mut headers = generation
        .headers
        .lock()
        .map_err(|_| McpError::Connection("MCP HTTP session header lock poisoned".to_owned()))?;
    if initialize {
        headers.session_id = received;
    } else if received != headers.session_id && received.is_some() {
        return Err(McpError::InvalidResponse(
            "MCP-Session-Id changed within a generation".to_owned(),
        ));
    }
    Ok(())
}

fn parse_response_session_id(headers: &HeaderMap) -> Result<Option<String>, McpError> {
    let received_values = headers.get_all(MCP_SESSION_ID);
    let mut received_values = received_values.iter();
    let received = received_values
        .next()
        .map(|value| {
            let bytes = value.as_bytes();
            if bytes.is_empty() || bytes.iter().any(|byte| !(0x21..=0x7e).contains(byte)) {
                return Err(McpError::InvalidResponse(
                    "MCP-Session-Id must contain visible ASCII characters".to_owned(),
                ));
            }
            value
                .to_str()
                .map(str::to_owned)
                .map_err(|_| McpError::InvalidResponse("invalid MCP-Session-Id".to_owned()))
        })
        .transpose()?;
    if received_values.next().is_some() {
        return Err(McpError::InvalidResponse(
            "MCP response contains multiple MCP-Session-Id headers".to_owned(),
        ));
    }
    Ok(received)
}

async fn validate_accepted(
    response: reqwest::Response,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(), McpError> {
    if response.status() != StatusCode::ACCEPTED {
        return Err(McpError::Transport(format!(
            "MCP HTTP notification or response returned {}, expected 202",
            response.status()
        )));
    }
    let bytes = read_bounded(response, 1, cancel).await?;
    if !bytes.is_empty() {
        return Err(McpError::InvalidResponse(
            "MCP HTTP 202 response must have an empty body".to_owned(),
        ));
    }
    Ok(())
}

async fn process_json_response(
    response: reqwest::Response,
    peer: &McpPeer,
    expected_id: &Value,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(), McpError> {
    let bytes = read_bounded(response, MAX_JSON_BODY, cancel).await?;
    let message: McpMessage = serde_json::from_slice(&bytes).map_err(|error| {
        McpError::InvalidResponse(format!("invalid MCP JSON response: {error}"))
    })?;
    if message_response_id(&message) != Some(expected_id) {
        return Err(McpError::InvalidResponse(
            "MCP JSON response id does not match request".to_owned(),
        ));
    }
    peer.receive(message).await.map(|_| ())?;
    let protocol = peer
        .session()?
        .negotiated_protocol_version()
        .map(str::to_owned);
    if let Some(protocol) = protocol {
        // For non-initialize responses this is unchanged. Initialize is routed before
        // McpSession accepts the result, so the caller fills it immediately afterward.
        let _ = protocol;
    }
    Ok(())
}

async fn read_bounded(
    response: reqwest::Response,
    max: usize,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Vec<u8>, McpError> {
    let mut stream = response.bytes_stream();
    let mut output = Vec::new();
    loop {
        let chunk = tokio::select! {
            biased;
            _ = wait_for_cancel(cancel) => return Err(McpError::Connection("MCP HTTP generation closed".to_owned())),
            chunk = stream.try_next() => chunk.map_err(|error| McpError::Transport(sanitize_reqwest_error(&error)))?,
        };
        let Some(chunk) = chunk else {
            return Ok(output);
        };
        if output.len().saturating_add(chunk.len()) > max {
            return Err(McpError::InvalidResponse(
                "MCP HTTP response body exceeds configured limit".to_owned(),
            ));
        }
        output.extend_from_slice(&chunk);
    }
}

fn message_response_id(message: &McpMessage) -> Option<&Value> {
    match message {
        McpMessage::SuccessResponse(response) => Some(&response.id),
        McpMessage::ErrorResponse(response) => response.id.as_ref(),
        _ => None,
    }
}

struct SseOutcome {
    last_event_id: Option<String>,
    retry: Duration,
    target_received: bool,
}

async fn consume_sse(
    response: reqwest::Response,
    peer: &McpPeer,
    target: Option<&Value>,
    allow_responses: bool,
    mut cancel: watch::Receiver<bool>,
) -> Result<SseOutcome, McpError> {
    let mut decoder = SseDecoder::new(SseLimits::default());
    let mut stream = response.bytes_stream();
    let mut last_event_id = None;
    let mut retry = Duration::from_secs(1);
    let mut target_received = false;
    loop {
        let chunk_result = tokio::select! {
            biased;
            _ = wait_for_cancel(&mut cancel) => return Err(McpError::Connection("MCP HTTP SSE cancelled".to_owned())),
            chunk = stream.try_next() => chunk,
        };
        let chunk = match chunk_result {
            Ok(chunk) => chunk,
            Err(_) if last_event_id.is_some() => {
                return Ok(SseOutcome {
                    last_event_id,
                    retry,
                    target_received,
                });
            }
            Err(error) => {
                return Err(McpError::Transport(sanitize_reqwest_error(&error)));
            }
        };
        let events = match chunk {
            Some(ref chunk) => decoder.push(chunk)?,
            None => decoder.finish()?,
        };
        for event in events {
            if let Some(id) = &event.id {
                last_event_id = (!id.is_empty()).then(|| id.clone());
            }
            if let Some(retry_ms) = event.retry_ms {
                retry = Duration::from_millis(retry_ms);
            }
            if event.data.is_empty() {
                continue;
            }
            let message: McpMessage = serde_json::from_str(&event.data).map_err(|error| {
                McpError::InvalidResponse(format!("invalid MCP SSE message: {error}"))
            })?;
            if let Some(response_id) = message_response_id(&message) {
                if !allow_responses {
                    return Err(McpError::InvalidResponse(
                        "ordinary MCP GET stream carried a JSON-RPC response".to_owned(),
                    ));
                }
                if let Some(target) = target {
                    if response_id != target {
                        return Err(McpError::InvalidResponse(
                            "MCP SSE response id does not match its request stream".to_owned(),
                        ));
                    }
                    target_received = true;
                }
            }
            peer.receive(message).await?;
            if target_received {
                return Ok(SseOutcome {
                    last_event_id,
                    retry,
                    target_received,
                });
            }
        }
        if chunk.is_none() {
            return Ok(SseOutcome {
                last_event_id,
                retry,
                target_received,
            });
        }
    }
}

async fn process_post_sse(
    connection: Arc<HttpConnection>,
    generation: Arc<HttpGeneration>,
    peer: McpPeer,
    response: reqwest::Response,
    target: Value,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), McpError> {
    let mut outcome = consume_sse(response, &peer, Some(&target), true, cancel.clone()).await?;
    let max_attempts = connection.reconnect.max_attempts.clamp(1, 8);
    for _ in 0..max_attempts {
        if outcome.target_received {
            return Ok(());
        }
        if outcome.last_event_id.is_none() {
            return Err(McpError::Transport(
                "MCP POST SSE ended before its JSON-RPC response".to_owned(),
            ));
        }
        tokio::select! {
            _ = wait_for_cancel(&mut cancel) => return Ok(()),
            _ = tokio::time::sleep(outcome.retry) => {}
        }
        let response = send_http(
            &connection,
            &generation,
            reqwest::Method::GET,
            None,
            false,
            outcome.last_event_id.as_deref(),
        )
        .await?;
        if response.status() == StatusCode::NOT_FOUND && has_session(&generation) {
            return Err(McpError::SessionExpired);
        }
        validate_response_session(&generation, &response, false)?;
        if response.status() != StatusCode::OK || !is_sse(&response) {
            return Err(McpError::InvalidResponse(
                "MCP SSE resumption did not return text/event-stream".to_owned(),
            ));
        }
        outcome = consume_sse(response, &peer, Some(&target), true, cancel.clone()).await?;
    }
    if outcome.target_received {
        Ok(())
    } else {
        Err(McpError::Transport(
            "MCP POST SSE resumption limit reached before its JSON-RPC response".to_owned(),
        ))
    }
}

async fn run_get_channel(
    connection: Arc<HttpConnection>,
    generation: Arc<HttpGeneration>,
    mut cancel: watch::Receiver<bool>,
    mut lifetime_cancel: watch::Receiver<bool>,
) {
    let lifecycle = generation.peer.downgrade();
    let mut last_event_id = None;
    let mut retry;
    let max_attempts = connection.reconnect.max_attempts.clamp(1, 8);
    for _ in 0..=max_attempts {
        let response = tokio::select! {
            _ = wait_for_cancel(&mut cancel) => return,
            _ = wait_for_cancel(&mut lifetime_cancel) => return,
            response = send_http(&connection, &generation, reqwest::Method::GET, None, false, last_event_id.as_deref()) => response,
        };
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                lifecycle
                    .close(format!("MCP HTTP GET failed: {error}"))
                    .await;
                return;
            }
        };
        if response.status() == StatusCode::METHOD_NOT_ALLOWED {
            return;
        }
        if response.status() == StatusCode::NOT_FOUND && has_session(&generation) {
            request_reinitialize(Arc::clone(&connection), Arc::clone(&generation));
            return;
        }
        if let Err(error) = validate_response_session(&generation, &response, false) {
            lifecycle
                .close(format!("MCP HTTP GET session validation failed: {error}"))
                .await;
            return;
        }
        if response.status() != StatusCode::OK || !is_sse(&response) {
            lifecycle.close("invalid MCP HTTP GET SSE response").await;
            return;
        }
        let outcome = tokio::select! {
            biased;
            _ = wait_for_cancel(&mut lifetime_cancel) => return,
            outcome = consume_sse(response, &generation.peer, None, false, cancel.clone()) => outcome,
        };
        match outcome {
            Ok(outcome) => {
                last_event_id = outcome.last_event_id;
                retry = outcome.retry;
            }
            Err(McpError::Connection(_)) => return,
            Err(error) => {
                lifecycle
                    .close(format!("MCP HTTP GET SSE failed: {error}"))
                    .await;
                return;
            }
        }
        if last_event_id.is_none() {
            let inbound = generation.peer.wait_for_inbound_tasks();
            tokio::select! {
                _ = wait_for_cancel(&mut cancel) => return,
                _ = wait_for_cancel(&mut lifetime_cancel) => return,
                _ = tokio::time::sleep(connection.timeouts.cancel_ack) => {},
                _ = inbound => {},
            }
            lifecycle
                .close("MCP HTTP GET SSE ended without a resumable event id")
                .await;
            return;
        }
        tokio::select! {
            _ = wait_for_cancel(&mut cancel) => return,
            _ = wait_for_cancel(&mut lifetime_cancel) => return,
            _ = tokio::time::sleep(retry) => {}
        }
    }
    lifecycle
        .close("MCP HTTP GET SSE reconnect limit exhausted")
        .await;
}

async fn wait_for_cancel(cancel: &mut watch::Receiver<bool>) {
    if *cancel.borrow() {
        return;
    }
    let _ = cancel.changed().await;
}

fn is_sse(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/event-stream"))
        })
}

fn request_reinitialize(connection: Arc<HttpConnection>, generation: Arc<HttpGeneration>) {
    let expired_generation = generation.id;
    let mut rebuild_attempt = None;
    let mut exhausted = false;
    let started = connection.state.send_if_modified(|state| match state {
        HttpConnectionState::Ready(current) if current.id == expired_generation => {
            if let Some(attempt) = connection.session_expiries.record() {
                rebuild_attempt = Some(attempt);
                *state = HttpConnectionState::Rebuilding { expired_generation };
            } else {
                exhausted = true;
                *state = HttpConnectionState::Failed(McpError::Connection(
                    "MCP HTTP session expiry retry budget exhausted".to_owned(),
                ));
            }
            true
        }
        _ => false,
    });
    if !started {
        return;
    }
    if exhausted {
        std::mem::drop(tokio::spawn(async move {
            generation
                .stop("MCP HTTP session expiry retry budget exhausted")
                .await;
        }));
        return;
    }
    let attempt = rebuild_attempt.expect("rebuild attempt is set for a rebuilding transition");
    std::mem::drop(tokio::spawn(async move {
        connection.perform_reinitialize(generation, attempt).await;
    }));
}

#[async_trait]
impl McpConnection for HttpConnectionHandle {
    fn connection_id(&self) -> &str {
        &self.connection_id
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        self.list_tools_all().await
    }

    async fn list_tools_page(
        &self,
        cursor: Option<&str>,
    ) -> Result<McpListPage<McpToolDescriptor>, McpError> {
        decode_list_tools(
            self.request_response("tools/list", pagination_params(cursor))
                .await?,
        )
    }

    async fn call_tool(&self, name: &str, args: Value) -> Result<McpToolResult, McpError> {
        decode_tool_result(
            self.send_with_elicitation("tools/call", call_tool_params(name, args))
                .await?,
        )
    }

    async fn cancel_tool_call(
        &self,
        request_id: &str,
        reason: Option<String>,
    ) -> Result<(), McpError> {
        self.send_notification(JsonRpcNotification::new(
            "notifications/cancelled",
            Some(serde_json::json!({
                "requestId": request_id,
                "reason": reason,
            })),
        ))
        .await
    }

    async fn list_resources(&self) -> Result<Vec<McpResource>, McpError> {
        self.list_resources_all().await
    }

    async fn list_resources_page(
        &self,
        cursor: Option<&str>,
    ) -> Result<McpListPage<McpResource>, McpError> {
        decode_list_resources(
            self.request_response("resources/list", pagination_params(cursor))
                .await?,
        )
    }

    async fn read_resource(&self, uri: &str) -> Result<McpReadResourceResult, McpError> {
        decode_read_resource(
            self.request_response("resources/read", read_resource_params(uri))
                .await?,
        )
    }

    async fn subscribe_resource(&self, uri: &str) -> Result<(), McpError> {
        decode_empty_result(
            self.request_response("resources/subscribe", resource_subscription_params(uri))
                .await?,
        )
    }

    async fn unsubscribe_resource(&self, uri: &str) -> Result<(), McpError> {
        decode_empty_result(
            self.request_response("resources/unsubscribe", resource_subscription_params(uri))
                .await?,
        )
    }

    async fn list_prompts(&self) -> Result<Vec<McpPrompt>, McpError> {
        self.list_prompts_all().await
    }

    async fn list_prompts_page(
        &self,
        cursor: Option<&str>,
    ) -> Result<McpListPage<McpPrompt>, McpError> {
        decode_list_prompts(
            self.request_response("prompts/list", pagination_params(cursor))
                .await?,
        )
    }

    async fn get_prompt(&self, name: &str, args: Value) -> Result<McpPromptMessages, McpError> {
        decode_prompt_messages(
            self.request_response("prompts/get", get_prompt_params(name, args))
                .await?,
        )
    }

    async fn subscribe_changes(&self) -> Result<ListChangedEvent, McpError> {
        self.generation().await?;
        let stream = BroadcastStream::new(self.changes.subscribe())
            .filter_map(|event| async move { event.ok() });
        Ok(Box::pin(stream))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        self.lifetime_cancel.send_replace(true);
        let _transition = self.reinitialize.lock().await;
        let generation = match self.state.send_replace(HttpConnectionState::Closed) {
            HttpConnectionState::Ready(generation) => Some(generation),
            HttpConnectionState::Starting
            | HttpConnectionState::Rebuilding { .. }
            | HttpConnectionState::Failed(_)
            | HttpConnectionState::Closed => None,
        };
        let Some(generation) = generation else {
            return Ok(());
        };
        let session = generation
            .headers
            .lock()
            .ok()
            .and_then(|headers| headers.session_id.clone());
        generation.stop("MCP HTTP connection shutting down").await;
        if session.is_none() {
            return Ok(());
        }
        let response = tokio::time::timeout(
            self.timeouts.call_default,
            send_http(
                self,
                &generation,
                reqwest::Method::DELETE,
                None,
                false,
                None,
            ),
        )
        .await
        .map_err(|_| McpError::HttpCleanupTimeout)??;
        validate_response_session(&generation, &response, false)?;
        if response.status().is_success()
            || matches!(
                response.status(),
                StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
            )
        {
            Ok(())
        } else {
            Err(McpError::HttpCleanupStatus(response.status().as_u16()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener},
        sync::atomic::AtomicBool,
        thread,
    };

    struct NoopMessageSink;

    #[async_trait]
    impl McpMessageSink for NoopMessageSink {
        async fn send(&self, _message: McpOutboundMessage) -> Result<(), McpError> {
            Ok(())
        }
    }

    fn test_generation(id: u64) -> Arc<HttpGeneration> {
        let session = McpSession::new(
            crate::McpExpectedCapabilities::default(),
            McpClientCapabilities::default(),
            McpImplementation::new("test", "0"),
        );
        let peer = McpPeer::builder(Arc::new(NoopMessageSink), session)
            .build()
            .unwrap();
        let (cancel, _) = watch::channel(false);
        Arc::new(HttpGeneration {
            id,
            peer,
            headers: Arc::new(StdMutex::new(SessionHeaders::default())),
            cancel,
            tasks: Mutex::new(Vec::new()),
        })
    }

    fn test_connection_at(
        generation: Arc<HttpGeneration>,
        endpoint: String,
    ) -> Arc<HttpConnection> {
        let (state, _) = watch::channel(HttpConnectionState::Ready(generation));
        let (changes, _) = broadcast::channel(1);
        Arc::new(HttpConnection {
            connection_id: "http:test".to_owned(),
            endpoint,
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            auth_provider: client_auth::McpClientAuthProvider::new(&crate::McpClientAuth::None),
            expected: crate::McpExpectedCapabilities::default(),
            timeouts: crate::McpTimeouts::default(),
            reconnect: ReconnectPolicy::default(),
            elicitation_handler: None,
            permission_mode: PermissionMode::Default,
            state,
            changes,
            reinitialize: Mutex::new(()),
            session_expiries: SessionExpiryBudget::new(0),
            next_generation_id: AtomicU64::new(2),
            lifetime_cancel: watch::channel(false).0,
        })
    }

    fn test_connection(generation: Arc<HttpGeneration>) -> Arc<HttpConnection> {
        test_connection_at(generation, "http://127.0.0.1:1".to_owned())
    }

    struct StreamingGetServer {
        endpoint: String,
        started: Arc<AtomicBool>,
        closed: Arc<AtomicBool>,
        stop: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<()>>,
    }

    impl StreamingGetServer {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let started = Arc::new(AtomicBool::new(false));
            let closed = Arc::new(AtomicBool::new(false));
            let stop = Arc::new(AtomicBool::new(false));
            let worker_started = Arc::clone(&started);
            let worker_closed = Arc::clone(&closed);
            let worker_stop = Arc::clone(&stop);
            let worker = thread::spawn(move || loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let mut request = Vec::new();
                        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                            let mut chunk = [0_u8; 1024];
                            let size = stream.read(&mut chunk).unwrap();
                            assert!(size > 0 && request.len() + size <= 16 * 1024);
                            request.extend_from_slice(&chunk[..size]);
                        }
                        assert!(std::str::from_utf8(&request).unwrap().starts_with("GET "));
                        stream
                            .write_all(
                                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n",
                            )
                            .unwrap();
                        while !worker_stop.load(Ordering::SeqCst) {
                            if stream.write_all(b"3\r\n:\n\n\r\n").is_err()
                                || stream.flush().is_err()
                            {
                                worker_closed.store(true, Ordering::SeqCst);
                                break;
                            }
                            worker_started.store(true, Ordering::SeqCst);
                            thread::sleep(Duration::from_millis(10));
                        }
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("streaming GET server failed: {error}"),
                }
            });
            Self {
                endpoint,
                started,
                closed,
                stop,
                worker: Some(worker),
            }
        }
    }

    impl Drop for StreamingGetServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(worker) = self.worker.take() {
                worker.join().unwrap();
            }
        }
    }

    async fn wait_for_flag(flag: &AtomicBool, message: &str) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while !flag.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{message}"));
    }

    async fn start_streaming_get(
        connection: &Arc<HttpConnection>,
        generation: &Arc<HttpGeneration>,
    ) {
        let task = tokio::spawn(run_get_channel(
            Arc::clone(connection),
            Arc::clone(generation),
            generation.cancel.subscribe(),
            connection.lifetime_cancel.subscribe(),
        ));
        generation.track_task(task).await;
    }

    #[test]
    fn endpoint_parsing_canonicalizes_domains_and_classifies_ipv6_literals() {
        let domain = parse_http_endpoint("https://EXAMPLE.com.:8443/mcp").unwrap();
        assert_eq!(domain.host, "example.com");
        assert_eq!(domain.url.host_str(), Some("example.com"));
        assert!(matches!(domain.kind, NetworkHostKind::DnsName));

        let ipv6 = parse_http_endpoint("http://[::1]:3000/mcp").unwrap();
        assert_eq!(ipv6.host, "::1");
        assert!(matches!(
            ipv6.kind,
            NetworkHostKind::IpLiteral(IpAddr::V6(ip)) if ip == Ipv6Addr::LOCALHOST
        ));
    }

    #[test]
    fn metadata_literals_are_always_rejected() {
        assert!(validate_network_address(
            &NetworkHostKind::IpLiteral(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))),
            IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)),
            false,
        )
        .is_err());
        let metadata = "fd00:ec2::254".parse::<Ipv6Addr>().unwrap();
        assert!(validate_network_address(
            &NetworkHostKind::IpLiteral(IpAddr::V6(metadata)),
            IpAddr::V6(metadata),
            false,
        )
        .is_err());
    }

    #[test]
    fn permanent_address_denials_cannot_be_bypassed_by_explicit_pins() {
        let denied = [
            IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1)),
            IpAddr::V4(Ipv4Addr::new(169, 254, 170, 2)),
            IpAddr::V4(Ipv4Addr::new(100, 100, 100, 200)),
            IpAddr::V6("fe80::1".parse().unwrap()),
            IpAddr::V6("febf:ffff::1".parse().unwrap()),
            IpAddr::V6("fd00:ec2::254".parse().unwrap()),
            IpAddr::V6("::ffff:8.8.8.8".parse().unwrap()),
            IpAddr::V6("::ffff:169.254.170.2".parse().unwrap()),
        ];

        for ip in denied {
            assert!(
                validate_network_address(&NetworkHostKind::DnsName, ip, true).is_err(),
                "explicit pin unexpectedly allowed {ip}"
            );
        }
    }

    #[test]
    fn dns_names_only_accept_publicly_routable_addresses() {
        let denied = [
            IpAddr::V4(Ipv4Addr::new(0, 1, 2, 3)),
            IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(100, 100, 100, 100)),
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
            IpAddr::V4(Ipv4Addr::new(198, 18, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1)),
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
            IpAddr::V6("2001:db8::1".parse().unwrap()),
            IpAddr::V6("::ffff:8.8.8.8".parse().unwrap()),
        ];

        for ip in denied {
            for explicitly_pinned in [false, true] {
                assert!(
                    validate_network_address(
                        &NetworkHostKind::DnsName,
                        ip,
                        explicitly_pinned
                    )
                    .is_err(),
                    "DNS name unexpectedly accepted {ip} with explicitly_pinned={explicitly_pinned}"
                );
            }
        }

        for ip in [
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            IpAddr::V6("2606:4700:4700::1111".parse().unwrap()),
        ] {
            for explicitly_pinned in [false, true] {
                assert!(
                    validate_network_address(
                        &NetworkHostKind::DnsName,
                        ip,
                        explicitly_pinned
                    )
                    .is_ok(),
                    "DNS name unexpectedly rejected {ip} with explicitly_pinned={explicitly_pinned}"
                );
            }
        }
    }

    #[test]
    fn explicit_literals_and_localhost_keep_their_own_address_policy() {
        let private = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7));
        assert!(
            validate_network_address(&NetworkHostKind::IpLiteral(private), private, false).is_ok()
        );
        assert!(validate_network_address(
            &NetworkHostKind::Localhost,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            false,
        )
        .is_ok());
        assert!(validate_network_address(
            &NetworkHostKind::Localhost,
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            false,
        )
        .is_err());
    }

    #[test]
    fn pinned_resolution_keys_use_endpoint_host_normalization() {
        let address = SocketAddr::from(([127, 0, 0, 1], 9000));
        let transport = HttpTransport::new()
            .with_pinned_resolution("EXAMPLE.COM.", vec![address])
            .with_pinned_resolution("[::1]", vec![address]);

        let domain = parse_http_endpoint("https://example.com/mcp").unwrap();
        let ipv6 = parse_http_endpoint("http://[::1]/mcp").unwrap();

        assert!(transport
            .pinned_resolutions
            .iter()
            .any(|(host, _)| host == &domain.host));
        assert!(transport
            .pinned_resolutions
            .iter()
            .any(|(host, _)| host == &ipv6.host));
    }

    #[test]
    fn session_header_rejects_empty_whitespace_control_and_multiple_values() {
        for value in [b"".as_slice(), b" ".as_slice(), b"bad\tvalue".as_slice()] {
            let mut headers = HeaderMap::new();
            headers.insert(MCP_SESSION_ID, HeaderValue::from_bytes(value).unwrap());
            assert!(parse_response_session_id(&headers).is_err());
        }

        let mut headers = HeaderMap::new();
        headers.append(MCP_SESSION_ID, HeaderValue::from_static("first"));
        headers.append(MCP_SESSION_ID, HeaderValue::from_static("second"));
        assert!(parse_response_session_id(&headers).is_err());
    }

    #[tokio::test]
    async fn cancellation_wait_observes_an_already_set_signal() {
        let (cancel, mut receiver) = watch::channel(false);
        cancel.send(true).unwrap();
        tokio::time::timeout(Duration::from_millis(10), wait_for_cancel(&mut receiver))
            .await
            .expect("already-set cancellation is observed");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_before_rebuild_task_is_polled_cancels_the_connection_lifetime() {
        let generation = test_generation(1);
        let connection = test_connection(Arc::clone(&generation));
        request_reinitialize(Arc::clone(&connection), generation);
        assert!(matches!(
            connection.state.borrow().clone(),
            HttpConnectionState::Rebuilding {
                expired_generation: 1
            }
        ));

        let handle = HttpConnectionHandle {
            inner: Arc::clone(&connection),
        };
        handle.shutdown().await.expect("shutdown");

        assert!(*connection.lifetime_cancel.borrow());
        assert!(matches!(
            connection.state.borrow().clone(),
            HttpConnectionState::Closed
        ));
        assert!(matches!(
            connection.install_generation().await,
            Err(McpError::Connection(message)) if message.contains("closed")
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dropping_last_handle_before_rebuild_task_is_polled_latches_cancellation() {
        let generation = test_generation(1);
        let connection = test_connection(Arc::clone(&generation));
        request_reinitialize(Arc::clone(&connection), generation);
        let handle = HttpConnectionHandle {
            inner: Arc::clone(&connection),
        };

        drop(handle);

        assert!(*connection.lifetime_cancel.borrow());
        assert!(matches!(
            connection.install_generation().await,
            Err(McpError::Connection(message)) if message.contains("closed")
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_before_rebuild_task_poll_drops_an_active_get_body() {
        let server = StreamingGetServer::start();
        let generation = test_generation(1);
        let connection = test_connection_at(Arc::clone(&generation), server.endpoint.clone());
        start_streaming_get(&connection, &generation).await;
        wait_for_flag(&server.started, "GET stream did not start").await;
        request_reinitialize(Arc::clone(&connection), generation);

        let handle = HttpConnectionHandle {
            inner: Arc::clone(&connection),
        };
        handle.shutdown().await.expect("shutdown");

        wait_for_flag(
            &server.closed,
            "shutdown did not drop the active GET response body",
        )
        .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dropping_last_handle_drops_an_active_get_body() {
        let server = StreamingGetServer::start();
        let generation = test_generation(1);
        let connection = test_connection_at(Arc::clone(&generation), server.endpoint.clone());
        start_streaming_get(&connection, &generation).await;
        wait_for_flag(&server.started, "GET stream did not start").await;
        let handle = HttpConnectionHandle {
            inner: Arc::clone(&connection),
        };

        drop(handle);

        wait_for_flag(
            &server.closed,
            "dropping the last handle did not drop the active GET response body",
        )
        .await;
    }

    #[test]
    fn only_an_explicit_session_expired_result_is_retryable() {
        assert!(should_retry_session_request(
            &Err(McpError::SessionExpired),
            0
        ));
        assert!(!should_retry_session_request(&Ok(Value::Null), 0));
        assert!(!should_retry_session_request(
            &Err(McpError::Connection("generation replaced".to_owned())),
            0
        ));
        assert!(!should_retry_session_request(
            &Err(McpError::SessionExpired),
            1
        ));
    }

    #[test]
    fn business_success_does_not_reset_budget_after_rebuild_starts() {
        let generation = test_generation(1);
        let connection = test_connection(generation);
        assert_eq!(connection.session_expiries.record(), Some(1));
        connection
            .state
            .send_replace(HttpConnectionState::Rebuilding {
                expired_generation: 1,
            });

        connection.reset_session_expiries_after_success(1);

        assert_eq!(connection.session_expiries.record(), None);
    }
}

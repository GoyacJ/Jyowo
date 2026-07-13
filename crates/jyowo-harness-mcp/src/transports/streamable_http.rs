use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::Duration,
};

use async_trait::async_trait;
use futures::{future::BoxFuture, FutureExt, StreamExt, TryStreamExt};
use harness_contracts::PermissionMode;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    StatusCode,
};
use serde_json::Value;
use tokio::{
    sync::{broadcast, mpsc, oneshot, watch, Mutex, RwLock, Semaphore},
    task::{JoinHandle, JoinSet},
};
use tokio_stream::wrappers::BroadcastStream;
use url::Host;

use crate::{
    authorize_mcp_transport_connect, call_tool_request, client_auth,
    continue_after_elicitation_response, decode_empty_result, decode_list_prompts,
    decode_list_resources, decode_list_tools, decode_prompt_messages, decode_read_resource,
    decode_tool_result, get_prompt_request, list_prompts_request, list_resources_request,
    list_tools_request, notification_change, read_resource_request, subscribe_resource_request,
    unsubscribe_resource_request, ElicitationHandler, JsonRpcNotification, JsonRpcPeer,
    JsonRpcRequest, JsonRpcResponse, ListChangedEvent, McpChange, McpClientCapabilities,
    McpConnectContext, McpConnection, McpError, McpImplementation, McpListPage, McpMessage,
    McpMessageSink, McpOrderedNotificationHandler, McpOutboundMessage, McpPeer, McpPrompt,
    McpPromptMessages, McpReadResourceResult, McpResource, McpServerSpec, McpSession,
    McpToolDescriptor, McpToolResult, McpTransport, NoopMcpMetricsSink, ReconnectPolicy,
    SseDecoder, SseLimits, TransportChoice,
};

const OUTBOUND_CAPACITY: usize = 64;
const MAX_POST_WORKERS: usize = 16;
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
        self.pinned_resolutions.push((host.into(), addrs));
        self
    }
}

impl Default for HttpTransport {
    fn default() -> Self {
        Self::new()
    }
}

struct ParsedHttpEndpoint {
    url: reqwest::Url,
    host: String,
    port: u16,
    kind: HttpHostKind,
}

#[derive(Clone, Copy)]
enum HttpHostKind {
    Localhost,
    IpLiteral(IpAddr),
    DnsName,
}

fn parse_http_endpoint(raw: &str) -> Result<ParsedHttpEndpoint, McpError> {
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
    let (host, kind) = match url
        .host()
        .ok_or_else(|| McpError::Protocol("MCP HTTP endpoint has no host".to_owned()))?
    {
        Host::Domain(domain) => {
            let host = domain.trim_end_matches('.').to_ascii_lowercase();
            if host.is_empty() {
                return Err(McpError::Protocol(
                    "MCP HTTP endpoint has no host".to_owned(),
                ));
            }
            url.set_host(Some(&host))
                .map_err(|_| McpError::Protocol("invalid MCP HTTP endpoint host".to_owned()))?;
            let kind = if host == "localhost" {
                HttpHostKind::Localhost
            } else {
                HttpHostKind::DnsName
            };
            (host, kind)
        }
        Host::Ipv4(ip) => (ip.to_string(), HttpHostKind::IpLiteral(IpAddr::V4(ip))),
        Host::Ipv6(ip) => (ip.to_string(), HttpHostKind::IpLiteral(IpAddr::V6(ip))),
    };
    let port = url
        .port_or_known_default()
        .ok_or_else(|| McpError::Protocol("MCP HTTP endpoint has no valid port".to_owned()))?;
    Ok(ParsedHttpEndpoint {
        url,
        host,
        port,
        kind,
    })
}

async fn resolve_http_endpoint(
    endpoint: &ParsedHttpEndpoint,
    explicit: Option<&[SocketAddr]>,
) -> Result<Vec<SocketAddr>, McpError> {
    let explicitly_pinned = explicit.is_some();
    let mut addrs = if let Some(addrs) = explicit {
        addrs
            .iter()
            .map(|addr| SocketAddr::new(addr.ip(), endpoint.port))
            .collect::<Vec<_>>()
    } else if let HttpHostKind::IpLiteral(ip) = endpoint.kind {
        vec![SocketAddr::new(ip, endpoint.port)]
    } else {
        tokio::net::lookup_host((endpoint.host.as_str(), endpoint.port))
            .await
            .map_err(|_| McpError::Transport("MCP HTTP DNS resolution failed".to_owned()))?
            .collect::<Vec<_>>()
    };
    addrs.sort_unstable();
    addrs.dedup();
    if addrs.is_empty() {
        return Err(McpError::Transport(
            "MCP HTTP endpoint resolved to no addresses".to_owned(),
        ));
    }
    for addr in &addrs {
        validate_http_address(&endpoint.kind, addr.ip(), explicitly_pinned)?;
    }
    Ok(addrs)
}

fn validate_http_address(
    kind: &HttpHostKind,
    ip: IpAddr,
    explicitly_pinned: bool,
) -> Result<(), McpError> {
    let ip = normalize_mapped_ip(ip);
    if is_always_blocked_ip(ip) {
        return Err(McpError::PermissionDenied(
            "MCP HTTP endpoint resolved to a disallowed address".to_owned(),
        ));
    }
    let valid = match kind {
        HttpHostKind::Localhost => ip.is_loopback(),
        HttpHostKind::IpLiteral(expected) => ip == normalize_mapped_ip(*expected),
        HttpHostKind::DnsName if explicitly_pinned => !is_always_blocked_ip(ip),
        HttpHostKind::DnsName => !is_dns_rebinding_target(ip),
    };
    if valid {
        Ok(())
    } else {
        Err(McpError::PermissionDenied(
            "MCP HTTP endpoint resolved to a disallowed address".to_owned(),
        ))
    }
}

fn normalize_mapped_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(ip)),
        ip => ip,
    }
}

fn is_always_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_unspecified()
                || ip.is_multicast()
                || ip == Ipv4Addr::new(169, 254, 169, 254)
                || ip == Ipv4Addr::BROADCAST
        }
        IpAddr::V6(ip) => {
            ip.is_unspecified()
                || ip.is_multicast()
                || ip
                    == "fd00:ec2::254"
                        .parse::<Ipv6Addr>()
                        .expect("valid metadata IP")
        }
    }
}

fn is_dns_rebinding_target(ip: IpAddr) -> bool {
    if is_always_blocked_ip(ip) {
        return true;
    }
    match ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

fn is_transport_owned_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "accept"
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
        let endpoint = parse_http_endpoint(&url)?;
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
        let resolved = resolve_http_endpoint(
            &endpoint,
            self.pinned_resolutions
                .iter()
                .rev()
                .find(|(host, _)| host.eq_ignore_ascii_case(&endpoint.host))
                .map(|(_, addrs)| addrs.as_slice()),
        )
        .await?;
        let builder = reqwest::Client::builder()
            .default_headers(default_headers)
            .pool_max_idle_per_host(0)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .resolve_to_addrs(&endpoint.host, &resolved);
        let client = builder
            .build()
            .map_err(|error| McpError::Transport(error.to_string()))?;
        let auth_provider = client_auth::McpClientAuthProvider::new(&spec.auth)
            .with_metrics_sink(context.metrics_sink_or(Arc::clone(&self.metrics_sink)))
            .with_lifecycle_events(
                spec.server_id.clone(),
                self.transport_id(),
                Arc::clone(&context.event_sink),
            );
        let connection = Arc::new(HttpConnection {
            connection_id: format!("http:{}", spec.server_id.0),
            endpoint: endpoint.url.to_string(),
            client,
            auth_provider,
            expected: spec.capabilities_expected,
            timeouts: spec.timeouts,
            reconnect: spec.reconnect,
            elicitation_handler: context.elicitation_handler,
            permission_mode: context.permission_mode,
            current: RwLock::new(None),
            reinitialize: Mutex::new(()),
            next_generation: AtomicU64::new(1),
            legacy_request_builder: JsonRpcPeer::new(),
            lifetime_cancel: watch::channel(false).0,
        });
        connection.install_generation().await?;
        Ok(Arc::new(HttpConnectionHandle { inner: connection }))
    }
}

struct HttpConnectionHandle {
    inner: Arc<HttpConnection>,
}

impl Drop for HttpConnectionHandle {
    fn drop(&mut self) {
        let _ = self.inner.lifetime_cancel.send(true);
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
    current: RwLock<Option<Arc<HttpGeneration>>>,
    reinitialize: Mutex<()>,
    next_generation: AtomicU64,
    legacy_request_builder: JsonRpcPeer,
    lifetime_cancel: watch::Sender<bool>,
}

struct HttpGeneration {
    id: u64,
    peer: McpPeer,
    headers: Arc<StdMutex<SessionHeaders>>,
    cancel: watch::Sender<bool>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    changes: broadcast::Sender<HttpChangeEvent>,
}

#[derive(Default)]
struct SessionHeaders {
    session_id: Option<String>,
    protocol_version: Option<String>,
}

#[derive(Clone)]
struct HttpChangeEvent {
    generation: u64,
    change: McpChange,
}

struct HttpNotificationHandler {
    generation: u64,
    changes: broadcast::Sender<HttpChangeEvent>,
}

impl McpOrderedNotificationHandler for HttpNotificationHandler {
    fn handle_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        if let Some(change) =
            notification_change(&notification.method, notification.params.as_ref())
        {
            let _ = self.changes.send(HttpChangeEvent {
                generation: self.generation,
                change,
            });
        }
        Ok(())
    }
}

struct OutboundEnvelope {
    message: McpOutboundMessage,
    committed: oneshot::Sender<Result<(), McpError>>,
    deadline: tokio::time::Instant,
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
        let id = self.next_generation.fetch_add(1, Ordering::Relaxed);
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
        let (changes, _) = broadcast::channel(64);
        let notification_handler = Arc::new(HttpNotificationHandler {
            generation: id,
            changes: changes.clone(),
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
            changes,
        });
        let dispatcher = tokio::spawn(run_dispatcher(
            Arc::clone(self),
            Arc::clone(&generation),
            peer,
            outbound_rx,
            cancel_rx,
            lifetime_cancel.clone(),
        ));
        generation.tasks.lock().await.push(dispatcher);
        if let Err(error) = generation.peer.initialize(self.timeouts.handshake).await {
            generation
                .stop(format!("MCP HTTP initialize failed: {error}"))
                .await;
            return Err(error);
        }
        let get_task = tokio::spawn(run_get_channel(
            Arc::clone(self),
            Arc::clone(&generation),
            generation.cancel.subscribe(),
            lifetime_cancel,
        ));
        generation.tasks.lock().await.push(get_task);
        *self.current.write().await = Some(Arc::clone(&generation));
        Ok(generation)
    }

    async fn generation(&self) -> Result<Arc<HttpGeneration>, McpError> {
        self.current
            .read()
            .await
            .clone()
            .ok_or_else(|| McpError::Connection("MCP HTTP connection is closed".to_owned()))
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
            if !should_retry_session_request(&result, attempt) {
                return result;
            }
            self.reinitialize_generation(generation.id).await?;
        }
        unreachable!()
    }

    async fn reinitialize_generation(self: &Arc<Self>, expired_id: u64) -> Result<(), McpError> {
        let _guard = self.reinitialize.lock().await;
        let current = self.generation().await?;
        if current.id != expired_id {
            return Ok(());
        }
        current.stop("MCP HTTP session expired").await;
        self.install_generation().await.map(|_| ())
    }

    async fn send(self: &Arc<Self>, request: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let id = request.id;
        match self.request_value(request.method, request.params).await {
            Ok(result) => Ok(JsonRpcResponse::success(id, result)),
            Err(McpError::RemoteJsonRpc(error)) => Ok(JsonRpcResponse::failure(id, error)),
            Err(error) => Err(error),
        }
    }

    async fn send_with_elicitation(
        self: &Arc<Self>,
        request: JsonRpcRequest,
    ) -> Result<JsonRpcResponse, McpError> {
        let response = self.send(request.clone()).await?;
        if let Some(retry) = continue_after_elicitation_response(
            &response,
            &request,
            &self.legacy_request_builder,
            self.elicitation_handler.as_ref(),
            self.permission_mode,
        )
        .await?
        {
            return self.send(retry).await;
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

impl HttpGeneration {
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
    let permits = Arc::new(Semaphore::new(MAX_POST_WORKERS));
    let mut workers = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = wait_for_cancel(&mut cancel) => break,
            _ = wait_for_cancel(&mut lifetime_cancel) => break,
            Some(_) = workers.join_next(), if !workers.is_empty() => {},
            envelope = outbound.recv() => {
                let Some(envelope) = envelope else { break; };
                let permit = tokio::select! {
                    biased;
                    _ = wait_for_cancel(&mut cancel) => break,
                    _ = wait_for_cancel(&mut lifetime_cancel) => break,
                    permit = Arc::clone(&permits).acquire_owned() => permit,
                };
                let Ok(permit) = permit else { break; };
                let connection = Arc::clone(&connection);
                let generation = Arc::clone(&generation);
                let peer = peer.clone();
                let cancel = cancel.clone();
                let lifetime_cancel = lifetime_cancel.clone();
                workers.spawn(async move {
                    let _permit = permit;
                    post_message(
                        connection,
                        generation,
                        peer,
                        envelope,
                        cancel,
                        lifetime_cancel,
                    )
                    .await;
                });
            }
        }
    }
    workers.shutdown().await;
    peer.close("MCP HTTP dispatcher stopped").await;
}

async fn post_message(
    connection: Arc<HttpConnection>,
    generation: Arc<HttpGeneration>,
    peer: McpPeer,
    envelope: OutboundEnvelope,
    mut cancel: watch::Receiver<bool>,
    mut lifetime_cancel: watch::Receiver<bool>,
) {
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
    if let Some(Err(error)) = outcome {
        if let Some(id) = expected_id.as_ref() {
            let _ = peer.fail_request(id, error);
        }
    }
}

async fn post_message_inner(
    connection: Arc<HttpConnection>,
    generation: Arc<HttpGeneration>,
    peer: McpPeer,
    message: McpMessage,
    expected_id: Option<Value>,
    mut committed: oneshot::Sender<Result<(), McpError>>,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), McpError> {
    let initialize =
        matches!(&message, McpMessage::Request(request) if request.method == "initialize");
    let response = tokio::select! {
        biased;
        _ = committed.closed() => return Ok(()),
        _ = wait_for_cancel(&mut cancel) => return Ok(()),
        result = send_http(&connection, &generation, reqwest::Method::POST, Some(&message), initialize, None) => result,
    };
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            let _ = committed.send(Err(error));
            return Ok(());
        }
    };
    if response.status() == StatusCode::NOT_FOUND && !initialize && has_session(&generation) {
        if let Some(id) = expected_id.as_ref() {
            let _ = peer.fail_request(id, McpError::SessionExpired);
            let _ = committed.send(Ok(()));
        } else {
            let _ = committed.send(Err(McpError::SessionExpired));
        }
        spawn_reinitialize(Arc::clone(&connection), generation.id);
        return Ok(());
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
        return Ok(());
    }
    if let Err(error) = validate_response_session(&generation, &response, initialize) {
        let _ = committed.send(Err(error));
        return Ok(());
    }
    if expected_id.is_none() {
        let result = validate_accepted(response, &mut cancel).await;
        let _ = committed.send(result);
        return Ok(());
    }
    let status = response.status();
    if !status.is_success() {
        let _ = committed.send(Err(McpError::Transport(format!(
            "MCP HTTP POST failed with status {status}"
        ))));
        return Ok(());
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
        let expected_id = expected_id.unwrap();
        process_post_sse(
            Arc::clone(&connection),
            Arc::clone(&generation),
            peer.clone(),
            response,
            expected_id.clone(),
            cancel,
        )
        .await?;
    } else {
        let _ = committed.send(Err(McpError::InvalidResponse(format!(
            "MCP HTTP request returned unsupported Content-Type {content_type:?}"
        ))));
    }
    Ok(())
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

fn sanitize_reqwest_error(error: &reqwest::Error) -> String {
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
    let received_values = response.headers().get_all(MCP_SESSION_ID);
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
            lifecycle.close("MCP HTTP GET session expired").await;
            spawn_reinitialize(Arc::clone(&connection), generation.id);
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
        match consume_sse(response, &generation.peer, None, false, cancel.clone()).await {
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

fn spawn_reinitialize(connection: Arc<HttpConnection>, generation_id: u64) {
    let task: BoxFuture<'static, ()> = async move {
        let _ = connection.reinitialize_generation(generation_id).await;
    }
    .boxed();
    std::mem::drop(tokio::spawn(task));
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
            self.send(list_tools_request(&self.legacy_request_builder, cursor))
                .await?,
        )
    }

    async fn call_tool(&self, name: &str, args: Value) -> Result<McpToolResult, McpError> {
        decode_tool_result(
            self.send_with_elicitation(call_tool_request(&self.legacy_request_builder, name, args))
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
            self.send(list_resources_request(&self.legacy_request_builder, cursor))
                .await?,
        )
    }

    async fn read_resource(&self, uri: &str) -> Result<McpReadResourceResult, McpError> {
        decode_read_resource(
            self.send(read_resource_request(&self.legacy_request_builder, uri))
                .await?,
        )
    }

    async fn subscribe_resource(&self, uri: &str) -> Result<(), McpError> {
        decode_empty_result(
            self.send(subscribe_resource_request(
                &self.legacy_request_builder,
                uri,
            ))
            .await?,
        )
    }

    async fn unsubscribe_resource(&self, uri: &str) -> Result<(), McpError> {
        decode_empty_result(
            self.send(unsubscribe_resource_request(
                &self.legacy_request_builder,
                uri,
            ))
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
            self.send(list_prompts_request(&self.legacy_request_builder, cursor))
                .await?,
        )
    }

    async fn get_prompt(&self, name: &str, args: Value) -> Result<McpPromptMessages, McpError> {
        decode_prompt_messages(
            self.send(get_prompt_request(&self.legacy_request_builder, name, args))
                .await?,
        )
    }

    async fn subscribe_changes(&self) -> Result<ListChangedEvent, McpError> {
        let generation = self.generation().await?;
        let generation_id = generation.id;
        let stream =
            BroadcastStream::new(generation.changes.subscribe()).filter_map(move |event| {
                let change = event
                    .ok()
                    .and_then(|event| (event.generation == generation_id).then_some(event.change));
                async move { change }
            });
        Ok(Box::pin(stream))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        let generation = self.current.write().await.take();
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
        .map_err(|_| McpError::Transport("MCP HTTP DELETE timed out".to_owned()))??;
        validate_response_session(&generation, &response, false)?;
        if response.status().is_success() || response.status() == StatusCode::METHOD_NOT_ALLOWED {
            Ok(())
        } else {
            Err(McpError::Transport(format!(
                "MCP HTTP DELETE failed with status {}",
                response.status()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_parsing_canonicalizes_domains_and_classifies_ipv6_literals() {
        let domain = parse_http_endpoint("https://EXAMPLE.com.:8443/mcp").unwrap();
        assert_eq!(domain.host, "example.com");
        assert_eq!(domain.url.host_str(), Some("example.com"));
        assert!(matches!(domain.kind, HttpHostKind::DnsName));

        let ipv6 = parse_http_endpoint("http://[::1]:3000/mcp").unwrap();
        assert_eq!(ipv6.host, "::1");
        assert!(matches!(
            ipv6.kind,
            HttpHostKind::IpLiteral(IpAddr::V6(ip)) if ip == Ipv6Addr::LOCALHOST
        ));
    }

    #[test]
    fn metadata_literals_are_always_rejected() {
        assert!(validate_http_address(
            &HttpHostKind::IpLiteral(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))),
            IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)),
            false,
        )
        .is_err());
        let metadata = "fd00:ec2::254".parse::<Ipv6Addr>().unwrap();
        assert!(validate_http_address(
            &HttpHostKind::IpLiteral(IpAddr::V6(metadata)),
            IpAddr::V6(metadata),
            false,
        )
        .is_err());
    }

    #[tokio::test]
    async fn cancellation_wait_observes_an_already_set_signal() {
        let (cancel, mut receiver) = watch::channel(false);
        cancel.send(true).unwrap();
        tokio::time::timeout(Duration::from_millis(10), wait_for_cancel(&mut receiver))
            .await
            .expect("already-set cancellation is observed");
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
}

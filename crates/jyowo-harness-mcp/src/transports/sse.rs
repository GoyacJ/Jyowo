use std::{fmt::Display, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use reqwest::{
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    StatusCode, Url,
};
use serde_json::Value;
use tokio::{
    sync::{broadcast, mpsc, oneshot, watch, Mutex, Semaphore},
    task::{JoinHandle, JoinSet},
};
use tokio_stream::wrappers::BroadcastStream;

use crate::{
    authorize_mcp_transport_connect, call_tool_request, client_auth, client_inbound_support,
    decode_empty_result, decode_list_prompts, decode_list_resources, decode_list_tools,
    decode_prompt_messages, decode_read_resource, decode_tool_result, get_prompt_request,
    list_prompts_request, list_resources_request, list_tools_request, notification_change,
    read_resource_request, response_key, subscribe_resource_request, tool_call_event_from_change,
    unsubscribe_resource_request, JsonRpcNotification, JsonRpcPeer, JsonRpcRequest,
    JsonRpcResponse, ListChangedEvent, McpChange, McpConnectContext, McpConnection, McpError,
    McpImplementation, McpListPage, McpMessage, McpMessageSink, McpOrderedNotificationHandler,
    McpOutboundMessage, McpPeer, McpPrompt, McpPromptMessages, McpReadResourceResult, McpResource,
    McpServerSpec, McpSession, McpToolCallEvent, McpToolCallStream, McpToolDescriptor,
    McpToolResult, McpTransport, NoopMcpMetricsSink, SseDecoder, SseEvent, SseLimits,
    TransportChoice,
};

#[cfg(test)]
use crate::McpClientCapabilities;

const OUTBOUND_CAPACITY: usize = 64;
const MAX_POST_WORKERS: usize = 16;

pub struct SseTransport {
    metrics_sink: Arc<dyn crate::McpMetricsSink>,
}

impl SseTransport {
    pub fn new() -> Self {
        Self {
            metrics_sink: Arc::new(NoopMcpMetricsSink),
        }
    }

    pub fn with_metrics_sink(metrics_sink: Arc<dyn crate::McpMetricsSink>) -> Self {
        Self { metrics_sink }
    }
}

impl Default for SseTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl McpTransport for SseTransport {
    fn transport_id(&self) -> &'static str {
        "sse"
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
        let TransportChoice::Sse { url, headers } = spec.transport.clone() else {
            return Err(McpError::Unsupported(
                "SseTransport requires TransportChoice::Sse".to_owned(),
            ));
        };
        let auth_provider = client_auth::McpClientAuthProvider::new(&spec.auth)
            .with_metrics_sink(context.metrics_sink_or(Arc::clone(&self.metrics_sink)))
            .with_lifecycle_events(
                spec.server_id.clone(),
                self.transport_id(),
                Arc::clone(&context.event_sink),
            );
        let (configured_url, client) =
            super::streamable_http::prepare_http_endpoint(&url, headers, &[]).await?;
        connect_prepared(
            format!("sse:{}", spec.server_id.0),
            spec,
            context,
            configured_url,
            client,
            auth_provider,
        )
        .await
    }
}

pub(super) async fn connect_prepared(
    connection_id: String,
    spec: McpServerSpec,
    context: McpConnectContext,
    configured_url: Url,
    client: reqwest::Client,
    auth_provider: client_auth::McpClientAuthProvider,
) -> Result<Arc<dyn McpConnection>, McpError> {
    let response = open_event_stream(
        &client,
        &configured_url,
        &auth_provider,
        spec.timeouts.handshake,
    )
    .await?;
    let stream_url = response.url().clone();
    let mut byte_stream = response.bytes_stream();
    let mut decoder = SseDecoder::new(SseLimits::default());
    let (endpoint, buffered) = tokio::time::timeout(
        spec.timeouts.handshake,
        discover_endpoint(&mut byte_stream, &mut decoder, &stream_url),
    )
    .await
    .map_err(|_| McpError::Connection("legacy SSE endpoint discovery timed out".to_owned()))??;

    let (changes, _) = broadcast::channel(64);
    let (outbound_tx, outbound_rx) = mpsc::channel(OUTBOUND_CAPACITY);
    let sink = Arc::new(SseMessageSink::new(
        outbound_tx,
        spec.timeouts.handshake,
        spec.timeouts.call_default,
    ));
    let support = client_inbound_support(&spec, &context);
    let session = McpSession::new(
        spec.capabilities_expected,
        support.capabilities,
        McpImplementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
    );
    let notification_handler = Arc::new(SseNotificationHandler {
        changes: changes.clone(),
    });
    let mut peer_builder = McpPeer::builder(sink.clone(), session);
    if let Some(handler) = support.sampling {
        peer_builder = peer_builder.sampling_handler(handler);
    }
    if let Some(handler) = support.elicitation {
        peer_builder = peer_builder.elicitation_handler(handler);
    }
    for method in change_notification_methods() {
        peer_builder =
            peer_builder.ordered_notification_handler(method, notification_handler.clone());
    }
    let peer = peer_builder.build()?;
    let (cancel, cancel_rx) = watch::channel(false);
    let dispatcher = tokio::spawn(run_dispatcher(
        client,
        endpoint,
        auth_provider,
        peer.clone(),
        outbound_rx,
        cancel.clone(),
        cancel_rx.clone(),
    ));
    let reader = tokio::spawn(run_event_reader(
        byte_stream,
        decoder,
        buffered,
        peer.clone(),
        cancel.clone(),
        cancel_rx,
    ));

    if let Err(error) = peer.initialize(spec.timeouts.handshake).await {
        let _ = cancel.send(true);
        sink.close().await;
        dispatcher.abort();
        reader.abort();
        peer.close(format!("legacy SSE initialize failed: {error}"))
            .await;
        return Err(error);
    }
    if let Err(error) = sink.flush(spec.timeouts.handshake).await {
        let _ = cancel.send(true);
        sink.close().await;
        dispatcher.abort();
        reader.abort();
        peer.close(format!("legacy SSE initialized POST failed: {error}"))
            .await;
        return Err(error);
    }

    Ok(Arc::new(SseConnection {
        connection_id,
        changes,
        timeout: spec.timeouts.call_default,
        peer,
        sink,
        cancel,
        dispatcher: Mutex::new(Some(dispatcher)),
        reader: Mutex::new(Some(reader)),
        legacy_request_builder: JsonRpcPeer::new(),
    }))
}

struct SseMessageSink {
    sender: Mutex<Option<mpsc::Sender<SseOutbound>>>,
    handshake_timeout: Duration,
    call_timeout: Duration,
}

enum SseOutbound {
    Message(SseOutboundMessage),
    Flush(oneshot::Sender<Result<(), McpError>>),
}

struct SseOutboundMessage {
    message: McpOutboundMessage,
    deadline: tokio::time::Instant,
    committed: oneshot::Sender<Result<(), McpError>>,
}

impl SseMessageSink {
    fn new(
        sender: mpsc::Sender<SseOutbound>,
        handshake_timeout: Duration,
        call_timeout: Duration,
    ) -> Self {
        Self {
            sender: Mutex::new(Some(sender)),
            handshake_timeout,
            call_timeout,
        }
    }

    async fn close(&self) {
        self.sender.lock().await.take();
    }

    async fn flush(&self, timeout: Duration) -> Result<(), McpError> {
        let deadline = tokio::time::Instant::now() + timeout;
        let sender = self
            .sender
            .lock()
            .await
            .clone()
            .ok_or_else(|| McpError::Connection("legacy SSE writer is closed".to_owned()))?;
        let (reply, result) = oneshot::channel();
        tokio::time::timeout_at(deadline, sender.send(SseOutbound::Flush(reply)))
            .await
            .map_err(|_| McpError::Connection("legacy SSE POST flush timed out".to_owned()))?
            .map_err(|_| McpError::Connection("legacy SSE writer is closed".to_owned()))?;
        tokio::time::timeout_at(deadline, result)
            .await
            .map_err(|_| McpError::Connection("legacy SSE POST flush timed out".to_owned()))?
            .map_err(|_| McpError::Connection("legacy SSE writer is closed".to_owned()))?
    }
}

#[async_trait]
impl McpMessageSink for SseMessageSink {
    async fn send(&self, message: McpOutboundMessage) -> Result<(), McpError> {
        let cancellation = cancellation_request_key(&message).is_some();
        let timeout = if is_handshake_message(&message) {
            self.handshake_timeout
        } else {
            self.call_timeout
        };
        let deadline = tokio::time::Instant::now() + timeout;
        let (committed, result) = oneshot::channel();
        let message = SseOutboundMessage {
            message,
            deadline,
            committed,
        };
        let sender = self
            .sender
            .lock()
            .await
            .clone()
            .ok_or_else(|| McpError::Connection("legacy SSE writer is closed".to_owned()))?;
        let permit = tokio::time::timeout_at(deadline, sender.reserve_owned())
            .await
            .map_err(|_| McpError::Connection("legacy SSE POST timed out".to_owned()))?
            .map_err(|_| McpError::Connection("legacy SSE writer is closed".to_owned()))?;
        permit.send(SseOutbound::Message(message));
        match tokio::time::timeout_at(deadline, result).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(McpError::Connection(
                "legacy SSE writer is closed".to_owned(),
            )),
            Err(_) if cancellation => Ok(()),
            Err(_) => Err(McpError::Connection("legacy SSE POST timed out".to_owned())),
        }
    }
}

fn is_handshake_message(message: &McpOutboundMessage) -> bool {
    match message.as_message() {
        McpMessage::Request(request) => request.method == "initialize",
        McpMessage::Notification(notification) => {
            notification.method == "notifications/initialized"
        }
        _ => false,
    }
}

struct SseNotificationHandler {
    changes: broadcast::Sender<McpChange>,
}

impl McpOrderedNotificationHandler for SseNotificationHandler {
    fn handle_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        if let Some(change) =
            notification_change(&notification.method, notification.params.as_ref())
        {
            let _ = self.changes.send(change);
        }
        Ok(())
    }
}

pub struct SseConnection {
    connection_id: String,
    changes: broadcast::Sender<McpChange>,
    timeout: Duration,
    peer: McpPeer,
    sink: Arc<SseMessageSink>,
    cancel: watch::Sender<bool>,
    dispatcher: Mutex<Option<JoinHandle<()>>>,
    reader: Mutex<Option<JoinHandle<()>>>,
    legacy_request_builder: JsonRpcPeer,
}

impl SseConnection {
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let id = request.id;
        match self
            .peer
            .request_optional(request.method, request.params, self.timeout)
            .await
        {
            Ok(result) => Ok(JsonRpcResponse::success(id, result)),
            Err(McpError::RemoteJsonRpc(error)) => Ok(JsonRpcResponse::failure(id, error)),
            Err(error) => Err(error),
        }
    }

    async fn send_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        match notification.params {
            Some(params) => self.peer.notify(notification.method, params).await,
            None => self.peer.notify_without_params(notification.method).await,
        }
    }

    async fn call_tool_events_inner(
        &self,
        name: &str,
        args: Value,
    ) -> Result<McpToolCallStream, McpError> {
        let request = call_tool_request(&self.legacy_request_builder, name, args);
        let mut changes = BroadcastStream::new(self.changes.subscribe());
        let pending = self
            .peer
            .start_request_optional(request.method, request.params, self.timeout)
            .await?;
        let key = response_key(pending.request_id());

        Ok(Box::pin(async_stream::stream! {
            let response = pending.wait();
            tokio::pin!(response);
            let mut changes_open = true;
            loop {
                if changes_open {
                    tokio::select! {
                        biased;
                        change = changes.next() => match change {
                            Some(Ok(change)) => {
                                if let Some(event) = tool_call_event_from_change(&key, change) {
                                    yield event;
                                }
                            },
                            Some(Err(_)) => {},
                            None => changes_open = false,
                        },
                        result = &mut response => {
                            match decode_peer_tool_result(result) {
                                Ok(result) => yield McpToolCallEvent::Final(result),
                                Err(error) => yield McpToolCallEvent::Error(error),
                            }
                            break;
                        },
                    }
                } else {
                    match decode_peer_tool_result((&mut response).await) {
                        Ok(result) => yield McpToolCallEvent::Final(result),
                        Err(error) => yield McpToolCallEvent::Error(error),
                    }
                    break;
                }
            }
        }))
    }
}

#[async_trait]
impl McpConnection for SseConnection {
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
            self.send(call_tool_request(&self.legacy_request_builder, name, args))
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

    async fn call_tool_events(
        &self,
        name: &str,
        args: Value,
    ) -> Result<McpToolCallStream, McpError> {
        self.call_tool_events_inner(name, args).await
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
        self.peer.ensure_open()?;
        let stream = BroadcastStream::new(self.changes.subscribe())
            .filter_map(|event| async move { event.ok() });
        Ok(Box::pin(stream))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        self.cancel.send_replace(true);
        self.peer.close("legacy SSE connection shutting down").await;
        self.sink.close().await;
        stop_task(&self.reader).await;
        stop_task(&self.dispatcher).await;
        Ok(())
    }
}

impl Drop for SseConnection {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
    }
}

async fn stop_task(task: &Mutex<Option<JoinHandle<()>>>) {
    if let Some(task) = task.lock().await.take() {
        task.abort();
        let _ = task.await;
    }
}

async fn open_event_stream(
    client: &reqwest::Client,
    url: &Url,
    auth_provider: &client_auth::McpClientAuthProvider,
    timeout: Duration,
) -> Result<reqwest::Response, McpError> {
    let response = tokio::time::timeout(timeout, send_event_get(client, url, auth_provider))
        .await
        .map_err(|_| McpError::Connection("legacy SSE GET timed out".to_owned()))??;
    let response = if is_auth_expired(response.status()) && auth_provider.can_refresh() {
        auth_provider.force_refresh_authorization_header().await?;
        tokio::time::timeout(timeout, send_event_get(client, url, auth_provider))
            .await
            .map_err(|_| McpError::Connection("legacy SSE GET timed out".to_owned()))??
    } else {
        response
    };
    if !response.status().is_success() {
        return Err(McpError::Transport(format!(
            "legacy SSE GET failed with status {}",
            response.status()
        )));
    }
    require_event_stream(&response)?;
    Ok(response)
}

async fn send_event_get(
    client: &reqwest::Client,
    url: &Url,
    auth_provider: &client_auth::McpClientAuthProvider,
) -> Result<reqwest::Response, McpError> {
    let mut request = client.get(url.clone()).header(ACCEPT, "text/event-stream");
    if let Some(authorization) = auth_provider.authorization_header().await? {
        request = request.header(AUTHORIZATION, authorization);
    }
    request.send().await.map_err(|error| {
        McpError::Transport(super::streamable_http::sanitize_reqwest_error(&error))
    })
}

fn require_event_stream(response: &reqwest::Response) -> Result<(), McpError> {
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if content_type.is_some_and(|value| value.eq_ignore_ascii_case("text/event-stream")) {
        Ok(())
    } else {
        Err(McpError::InvalidResponse(
            "legacy SSE GET requires content type text/event-stream".to_owned(),
        ))
    }
}

async fn discover_endpoint<S, B, E>(
    stream: &mut S,
    decoder: &mut SseDecoder,
    stream_url: &Url,
) -> Result<(Url, Vec<SseEvent>), McpError>
where
    S: Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: Display,
{
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| McpError::Transport(error.to_string()))?;
        let mut events = decoder.push(chunk.as_ref())?;
        events.retain(|event| !event.data.is_empty());
        if events.is_empty() {
            continue;
        }
        let first = events.remove(0);
        if first.event.as_deref() != Some("endpoint") {
            return Err(McpError::InvalidResponse(
                "legacy SSE first event must be endpoint".to_owned(),
            ));
        }
        let endpoint = resolve_endpoint(stream_url, &first.data)?;
        return Ok((endpoint, events));
    }
    Err(McpError::Connection(
        "legacy SSE stream closed before endpoint discovery".to_owned(),
    ))
}

fn resolve_endpoint(stream_url: &Url, endpoint: &str) -> Result<Url, McpError> {
    let endpoint = stream_url.join(endpoint).map_err(|error| {
        McpError::InvalidResponse(format!("invalid legacy SSE endpoint: {error}"))
    })?;
    validate_http_url(&endpoint, "legacy SSE endpoint")?;
    if endpoint.fragment().is_some() {
        return Err(McpError::InvalidResponse(
            "legacy SSE endpoint must not contain a fragment".to_owned(),
        ));
    }
    if origin(&endpoint) != origin(stream_url) {
        return Err(McpError::InvalidResponse(
            "legacy SSE endpoint must use the event stream origin".to_owned(),
        ));
    }
    Ok(endpoint)
}

fn validate_http_url(url: &Url, label: &str) -> Result<(), McpError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(McpError::InvalidResponse(format!(
            "{label} must use http or https"
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(McpError::InvalidResponse(format!(
            "{label} must not contain userinfo"
        )));
    }
    if url.host_str().is_none() {
        return Err(McpError::InvalidResponse(format!(
            "{label} must contain a host"
        )));
    }
    Ok(())
}

fn origin(url: &Url) -> (String, String, Option<u16>) {
    (
        url.scheme().to_owned(),
        url.host_str().unwrap_or_default().to_ascii_lowercase(),
        url.port_or_known_default(),
    )
}

async fn run_event_reader<S, B, E>(
    mut stream: S,
    mut decoder: SseDecoder,
    buffered: Vec<SseEvent>,
    peer: McpPeer,
    cancel_tx: watch::Sender<bool>,
    mut cancel: watch::Receiver<bool>,
) where
    S: Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: Display,
{
    for event in buffered {
        if let Err(error) = receive_event(&peer, event).await {
            cancel_tx.send_replace(true);
            peer.close(format!("legacy SSE event failed: {error}"))
                .await;
            return;
        }
    }
    loop {
        tokio::select! {
            biased;
            _ = wait_for_cancel(&mut cancel) => return,
            chunk = stream.next() => match chunk {
                Some(Ok(chunk)) => match decoder.push(chunk.as_ref()) {
                    Ok(events) => {
                        for event in events {
                            if let Err(error) = receive_event(&peer, event).await {
                                cancel_tx.send_replace(true);
                                peer.close(format!("legacy SSE event failed: {error}")).await;
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        cancel_tx.send_replace(true);
                        peer.close(format!("legacy SSE decode failed: {error}")).await;
                        return;
                    }
                },
                Some(Err(error)) => {
                    let _ = error;
                    cancel_tx.send_replace(true);
                    peer.close("legacy SSE stream failed").await;
                    return;
                }
                None => {
                    let _ = decoder.finish();
                    cancel_tx.send_replace(true);
                    peer.close("legacy SSE stream closed").await;
                    return;
                }
            }
        }
    }
}

async fn receive_event(peer: &McpPeer, event: SseEvent) -> Result<(), McpError> {
    if event.data.is_empty() {
        return Ok(());
    }
    let message = serde_json::from_str::<McpMessage>(&event.data).map_err(|error| {
        McpError::InvalidResponse(format!("invalid legacy SSE message: {error}"))
    })?;
    peer.receive(message).await.map(|_| ())
}

async fn run_dispatcher(
    client: reqwest::Client,
    endpoint: Url,
    auth_provider: client_auth::McpClientAuthProvider,
    peer: McpPeer,
    mut outbound: mpsc::Receiver<SseOutbound>,
    cancel_tx: watch::Sender<bool>,
    mut cancel: watch::Receiver<bool>,
) {
    let permits = Arc::new(Semaphore::new(MAX_POST_WORKERS));
    let mut workers = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = wait_for_cancel(&mut cancel) => break,
            Some(worker) = workers.join_next(), if !workers.is_empty() => {
                if let Err(error) = post_worker_result(worker) {
                    cancel_tx.send_replace(true);
                    peer.close(format!("legacy SSE POST failed: {error}")).await;
                }
            },
            outbound = outbound.recv() => {
                let Some(outbound) = outbound else { break; };
                let SseOutbound::Message(mut message) = outbound else {
                    let SseOutbound::Flush(reply) = outbound else { unreachable!() };
                    let mut result = Ok(());
                    while let Some(worker) = workers.join_next().await {
                        if let Err(error) = post_worker_result(worker) {
                            cancel_tx.send_replace(true);
                            peer.close(format!("legacy SSE POST failed: {error}")).await;
                            result = Err(error);
                            break;
                        }
                    }
                    let failed = result.is_err();
                    let _ = reply.send(result);
                    if failed {
                        break;
                    }
                    continue;
                };
                let slot = tokio::select! {
                    biased;
                    _ = wait_for_cancel(&mut cancel) => PostSlot::Cancelled,
                    _ = tokio::time::sleep_until(message.deadline) => PostSlot::Expired,
                    _ = message.committed.closed() => PostSlot::Abandoned,
                    permit = Arc::clone(&permits).acquire_owned() => PostSlot::Permit(permit),
                };
                let permit = match slot {
                    PostSlot::Cancelled => break,
                    PostSlot::Expired => {
                        if let Err(error) = expire_outbound_message(message) {
                            cancel_tx.send_replace(true);
                            peer.close(format!("legacy SSE POST failed: {error}")).await;
                            break;
                        }
                        continue;
                    }
                    PostSlot::Abandoned => continue,
                    PostSlot::Permit(Ok(permit)) => permit,
                    PostSlot::Permit(Err(_)) => break,
                };
                let client = client.clone();
                let endpoint = endpoint.clone();
                let auth_provider = auth_provider.clone();
                let weak_peer = peer.downgrade();
                let worker_cancel = cancel_tx.clone();
                let worker_cancel_rx = cancel_tx.subscribe();
                workers.spawn(async move {
                    let _permit = permit;
                    let result = run_post_worker(
                        &client,
                        &endpoint,
                        &auth_provider,
                        message,
                        worker_cancel_rx,
                    )
                    .await;
                    if let Err(error) = &result {
                        worker_cancel.send_replace(true);
                        weak_peer
                            .close(format!("legacy SSE POST failed: {error}"))
                            .await;
                    }
                    result
                });
            }
        }
    }
    workers.shutdown().await;
    peer.close("legacy SSE dispatcher stopped").await;
}

enum PostSlot {
    Cancelled,
    Expired,
    Abandoned,
    Permit(Result<tokio::sync::OwnedSemaphorePermit, tokio::sync::AcquireError>),
}

fn cancellation_request_key(message: &McpOutboundMessage) -> Option<String> {
    match message.as_message() {
        McpMessage::Notification(notification)
            if notification.method == "notifications/cancelled" =>
        {
            notification
                .params
                .as_ref()?
                .get("requestId")
                .map(request_id_key)
        }
        _ => None,
    }
}

fn request_id_key(id: &Value) -> String {
    serde_json::to_string(id).expect("JSON-RPC request IDs are serializable")
}

fn expiry_is_benign(message: &McpOutboundMessage) -> bool {
    matches!(message.as_message(), McpMessage::Request(_))
        || cancellation_request_key(message).is_some()
}

fn expire_outbound_message(message: SseOutboundMessage) -> Result<(), McpError> {
    if cancellation_request_key(&message.message).is_some() {
        let _ = message.committed.send(Ok(()));
        return Ok(());
    }
    let error = McpError::Connection("legacy SSE POST timed out".to_owned());
    let benign = expiry_is_benign(&message.message);
    let _ = message.committed.send(Err(error.clone()));
    if benign {
        Ok(())
    } else {
        Err(error)
    }
}

async fn run_post_worker(
    client: &reqwest::Client,
    endpoint: &Url,
    auth_provider: &client_auth::McpClientAuthProvider,
    mut message: SseOutboundMessage,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), McpError> {
    let post = post_message(client, endpoint, auth_provider, message.message.clone());
    tokio::pin!(post);
    tokio::select! {
        biased;
        _ = wait_for_cancel(&mut cancel) => Ok(()),
        _ = tokio::time::sleep_until(message.deadline) => expire_outbound_message(message),
        _ = message.committed.closed() => Ok(()),
        result = &mut post => {
            match result {
                Ok(()) => {
                    let _ = message.committed.send(Ok(()));
                    Ok(())
                }
                Err(error) => {
                    let _ = message.committed.send(Err(error.clone()));
                    Err(error)
                }
            }
        },
    }
}

fn post_worker_result(
    result: Result<Result<(), McpError>, tokio::task::JoinError>,
) -> Result<(), McpError> {
    result
        .map_err(|error| McpError::Connection(format!("legacy SSE POST worker failed: {error}")))?
}

async fn post_message(
    client: &reqwest::Client,
    endpoint: &Url,
    auth_provider: &client_auth::McpClientAuthProvider,
    message: McpOutboundMessage,
) -> Result<(), McpError> {
    let body = message.into_message();
    let response = post_once(client, endpoint, auth_provider, &body).await?;
    let response = if is_auth_expired(response.status()) && auth_provider.can_refresh() {
        auth_provider.force_refresh_authorization_header().await?;
        post_once(client, endpoint, auth_provider, &body).await?
    } else {
        response
    };
    if response.status().is_success() {
        Ok(())
    } else {
        Err(McpError::Transport(format!(
            "legacy SSE POST failed with status {}",
            response.status()
        )))
    }
}

async fn post_once(
    client: &reqwest::Client,
    endpoint: &Url,
    auth_provider: &client_auth::McpClientAuthProvider,
    body: &McpMessage,
) -> Result<reqwest::Response, McpError> {
    let mut request = client.post(endpoint.clone()).json(body);
    if let Some(authorization) = auth_provider.authorization_header().await? {
        request = request.header(AUTHORIZATION, authorization);
    }
    request.send().await.map_err(|error| {
        McpError::Transport(super::streamable_http::sanitize_reqwest_error(&error))
    })
}

fn is_auth_expired(status: StatusCode) -> bool {
    matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
}

fn change_notification_methods() -> [&'static str; 10] {
    [
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
    ]
}

async fn wait_for_cancel(cancel: &mut watch::Receiver<bool>) {
    while !*cancel.borrow_and_update() {
        if cancel.changed().await.is_err() {
            break;
        }
    }
}

fn decode_peer_tool_result(result: Result<Value, McpError>) -> Result<McpToolResult, McpError> {
    let response = match result {
        Ok(result) => JsonRpcResponse::success(Value::Null, result),
        Err(McpError::RemoteJsonRpc(error)) => JsonRpcResponse::failure(Value::Null, error),
        Err(error) => return Err(error),
    };
    decode_tool_result(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        convert::Infallible,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Mutex as StdMutex,
        },
    };
    use tokio::sync::Notify;

    struct NoopSink;

    #[async_trait]
    impl McpMessageSink for NoopSink {
        async fn send(&self, _message: McpOutboundMessage) -> Result<(), McpError> {
            Ok(())
        }
    }

    fn test_peer() -> McpPeer {
        McpPeer::builder(
            Arc::new(NoopSink),
            McpSession::new(
                Default::default(),
                McpClientCapabilities::default(),
                McpImplementation::new("test", "0"),
            ),
        )
        .build()
        .expect("peer")
    }

    fn ready_session() -> McpSession {
        let mut session = McpSession::new(
            Default::default(),
            McpClientCapabilities::default(),
            McpImplementation::new("test", "0"),
        );
        session.begin_initialization().expect("begin initialize");
        session
            .accept_initialize_result(crate::InitializeResult {
                protocol_version: crate::LATEST_PROTOCOL_VERSION.to_owned(),
                capabilities: crate::McpServerCapabilities {
                    tools: Some(Default::default()),
                    ..Default::default()
                },
                server_info: McpImplementation::new("fixture", "0"),
                instructions: None,
                extra: Default::default(),
            })
            .expect("accept initialize");
        session
            .mark_initialized_notification_sent()
            .expect("mark initialized");
        session
    }

    async fn wait_for_count(count: &AtomicUsize, notify: &Notify, expected: usize) {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let changed = notify.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if count.load(Ordering::SeqCst) >= expected {
                    return;
                }
                changed.await;
            }
        })
        .await
        .expect("expected POST count");
    }

    #[tokio::test]
    async fn initialized_notification_uses_the_handshake_deadline() {
        let (outbound_tx, mut outbound_rx) = mpsc::channel(1);
        let sink = SseMessageSink::new(
            outbound_tx,
            Duration::from_secs(2),
            Duration::from_millis(20),
        );
        let earliest = tokio::time::Instant::now() + Duration::from_secs(1);

        let send = tokio::spawn(async move {
            sink.send(
                McpOutboundMessage::notification_without_params("notifications/initialized")
                    .expect("initialized notification"),
            )
            .await
        });

        let SseOutbound::Message(message) = outbound_rx.recv().await.expect("queued message")
        else {
            panic!("expected queued message");
        };
        assert!(message.deadline >= earliest);
        message
            .committed
            .send(Ok(()))
            .expect("commit initialized notification");
        send.await
            .expect("send task")
            .expect("queue initialized notification");
    }

    #[tokio::test]
    async fn dispatcher_does_not_post_a_request_that_timed_out_while_waiting_for_a_worker() {
        use axum::{body::Bytes, extract::State, http::StatusCode, routing::post, Router};

        #[derive(Clone)]
        struct StateData {
            methods: Arc<StdMutex<Vec<String>>>,
            blocked: Arc<AtomicUsize>,
            blocked_changed: Arc<Notify>,
            release: watch::Sender<bool>,
            barrier_seen: Arc<AtomicBool>,
            barrier_changed: Arc<Notify>,
        }

        async fn rpc(State(state): State<StateData>, body: Bytes) -> StatusCode {
            let message: Value = serde_json::from_slice(&body).expect("POST JSON");
            let method = message["method"].as_str().expect("POST method").to_owned();
            state.methods.lock().expect("methods").push(method.clone());
            match method.as_str() {
                "notifications/block" => {
                    state.blocked.fetch_add(1, Ordering::SeqCst);
                    state.blocked_changed.notify_waiters();
                    wait_for_cancel(&mut state.release.subscribe()).await;
                }
                "notifications/barrier" => {
                    state.barrier_seen.store(true, Ordering::SeqCst);
                    state.barrier_changed.notify_waiters();
                }
                _ => {}
            }
            StatusCode::ACCEPTED
        }

        let state = StateData {
            methods: Arc::new(StdMutex::new(Vec::new())),
            blocked: Arc::new(AtomicUsize::new(0)),
            blocked_changed: Arc::new(Notify::new()),
            release: watch::channel(false).0,
            barrier_seen: Arc::new(AtomicBool::new(false)),
            barrier_changed: Arc::new(Notify::new()),
        };
        let app = Router::new()
            .route("/rpc", post(rpc))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let endpoint = Url::parse(&format!(
            "http://{}/rpc",
            listener.local_addr().expect("local address")
        ))
        .expect("endpoint");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let (outbound_tx, outbound_rx) = mpsc::channel(OUTBOUND_CAPACITY);
        let sink = Arc::new(SseMessageSink::new(
            outbound_tx,
            Duration::from_secs(5),
            Duration::from_secs(5),
        ));
        let peer = McpPeer::builder(sink, ready_session())
            .build()
            .expect("peer");
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let dispatcher = tokio::spawn(run_dispatcher(
            reqwest::Client::builder()
                .pool_max_idle_per_host(0)
                .no_proxy()
                .build()
                .expect("client"),
            endpoint,
            client_auth::McpClientAuthProvider::new(&crate::McpClientAuth::None),
            peer.clone(),
            outbound_rx,
            cancel_tx.clone(),
            cancel_rx,
        ));

        let mut blocking_posts = JoinSet::new();
        for _ in 0..MAX_POST_WORKERS {
            let peer = peer.clone();
            blocking_posts
                .spawn(async move { peer.notify_without_params("notifications/block").await });
        }
        wait_for_count(&state.blocked, &state.blocked_changed, MAX_POST_WORKERS).await;

        let error = peer
            .request(
                "tools/call",
                serde_json::json!({}),
                Duration::from_millis(50),
            )
            .await
            .expect_err("queued request must time out");
        assert!(error.to_string().contains("timed out"), "{error}");

        state.release.send_replace(true);
        while let Some(post) = blocking_posts.join_next().await {
            post.expect("blocking POST task")
                .expect("blocking POST accepted");
        }
        peer.notify_without_params("notifications/barrier")
            .await
            .expect("queue barrier POST");
        let barrier = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let changed = state.barrier_changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if state.barrier_seen.load(Ordering::SeqCst) {
                    return;
                }
                changed.await;
            }
        })
        .await;
        assert!(
            barrier.is_ok(),
            "barrier POST; methods={:?}; cancelled={}; dispatcher_finished={}",
            state.methods.lock().expect("methods"),
            *cancel_tx.borrow(),
            dispatcher.is_finished()
        );

        let methods = state.methods.lock().expect("methods");
        assert!(!methods
            .iter()
            .any(|method| { matches!(method.as_str(), "tools/call" | "notifications/cancelled") }));
        drop(methods);

        cancel_tx.send_replace(true);
        dispatcher.await.expect("dispatcher");
        server.abort();
    }

    #[tokio::test]
    async fn completed_requests_cancel_hanging_posts_and_release_workers() {
        use axum::{body::Bytes, extract::State, http::StatusCode, routing::post, Router};

        #[derive(Clone)]
        struct StateData {
            requests: Arc<AtomicUsize>,
            requests_changed: Arc<Notify>,
            barrier_seen: Arc<AtomicBool>,
            barrier_changed: Arc<Notify>,
        }

        async fn rpc(State(state): State<StateData>, body: Bytes) -> StatusCode {
            let message: Value = serde_json::from_slice(&body).expect("POST JSON");
            match message["method"].as_str().expect("POST method") {
                "tools/list" => {
                    state.requests.fetch_add(1, Ordering::SeqCst);
                    state.requests_changed.notify_waiters();
                    std::future::pending::<()>().await;
                }
                "notifications/barrier" => {
                    state.barrier_seen.store(true, Ordering::SeqCst);
                    state.barrier_changed.notify_waiters();
                }
                method => panic!("unexpected POST method: {method}"),
            }
            StatusCode::ACCEPTED
        }

        let state = StateData {
            requests: Arc::new(AtomicUsize::new(0)),
            requests_changed: Arc::new(Notify::new()),
            barrier_seen: Arc::new(AtomicBool::new(false)),
            barrier_changed: Arc::new(Notify::new()),
        };
        let app = Router::new()
            .route("/rpc", post(rpc))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let endpoint = Url::parse(&format!(
            "http://{}/rpc",
            listener.local_addr().expect("local address")
        ))
        .expect("endpoint");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let (outbound_tx, outbound_rx) = mpsc::channel(OUTBOUND_CAPACITY);
        let sink = Arc::new(SseMessageSink::new(
            outbound_tx,
            Duration::from_secs(5),
            Duration::from_secs(5),
        ));
        let peer = McpPeer::builder(sink, ready_session())
            .build()
            .expect("peer");
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let dispatcher = tokio::spawn(run_dispatcher(
            reqwest::Client::builder()
                .pool_max_idle_per_host(0)
                .no_proxy()
                .build()
                .expect("client"),
            endpoint,
            client_auth::McpClientAuthProvider::new(&crate::McpClientAuth::None),
            peer.clone(),
            outbound_rx,
            cancel_tx.clone(),
            cancel_rx,
        ));

        let mut requests = JoinSet::new();
        for _ in 0..MAX_POST_WORKERS {
            let peer = peer.clone();
            requests.spawn(async move {
                peer.request("tools/list", serde_json::json!({}), Duration::from_secs(5))
                    .await
            });
        }
        wait_for_count(&state.requests, &state.requests_changed, MAX_POST_WORKERS).await;

        for id in 1..=MAX_POST_WORKERS {
            let response = serde_json::from_value(serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {}
            }))
            .expect("JSON-RPC response");
            peer.receive(response).await.expect("receive response");
        }
        while let Some(request) = requests.join_next().await {
            request.expect("request task").expect("request response");
        }

        peer.notify_without_params("notifications/barrier")
            .await
            .expect("queue barrier POST");
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let changed = state.barrier_changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if state.barrier_seen.load(Ordering::SeqCst) {
                    return;
                }
                changed.await;
            }
        })
        .await
        .expect("completed requests release POST workers");

        cancel_tx.send_replace(true);
        dispatcher.await.expect("dispatcher");
        server.abort();
    }

    #[tokio::test]
    async fn cancellation_waits_for_the_original_post_success_response() {
        use axum::{body::Bytes, extract::State, http::StatusCode, routing::post, Router};

        #[derive(Clone)]
        struct StateData {
            methods: Arc<StdMutex<Vec<String>>>,
            request_seen: Arc<AtomicBool>,
            request_changed: Arc<Notify>,
            cancellation_seen: Arc<AtomicBool>,
            cancellation_changed: Arc<Notify>,
            release_request: watch::Sender<bool>,
        }

        async fn rpc(State(state): State<StateData>, body: Bytes) -> StatusCode {
            let message: Value = serde_json::from_slice(&body).expect("POST JSON");
            let method = message["method"].as_str().expect("POST method").to_owned();
            state.methods.lock().expect("methods").push(method.clone());
            match method.as_str() {
                "tools/call" => {
                    state.request_seen.store(true, Ordering::SeqCst);
                    state.request_changed.notify_waiters();
                    wait_for_cancel(&mut state.release_request.subscribe()).await;
                }
                "notifications/cancelled" => {
                    state.cancellation_seen.store(true, Ordering::SeqCst);
                    state.cancellation_changed.notify_waiters();
                }
                _ => panic!("unexpected POST method: {method}"),
            }
            StatusCode::ACCEPTED
        }

        let state = StateData {
            methods: Arc::new(StdMutex::new(Vec::new())),
            request_seen: Arc::new(AtomicBool::new(false)),
            request_changed: Arc::new(Notify::new()),
            cancellation_seen: Arc::new(AtomicBool::new(false)),
            cancellation_changed: Arc::new(Notify::new()),
            release_request: watch::channel(false).0,
        };
        let app = Router::new()
            .route("/rpc", post(rpc))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let endpoint = Url::parse(&format!(
            "http://{}/rpc",
            listener.local_addr().expect("local address")
        ))
        .expect("endpoint");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let (outbound_tx, outbound_rx) = mpsc::channel(OUTBOUND_CAPACITY);
        let sink = Arc::new(SseMessageSink::new(
            outbound_tx,
            Duration::from_secs(5),
            Duration::from_secs(5),
        ));
        let peer = McpPeer::builder(sink, ready_session())
            .build()
            .expect("peer");
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let dispatcher = tokio::spawn(run_dispatcher(
            reqwest::Client::builder()
                .pool_max_idle_per_host(0)
                .no_proxy()
                .build()
                .expect("client"),
            endpoint,
            client_auth::McpClientAuthProvider::new(&crate::McpClientAuth::None),
            peer.clone(),
            outbound_rx,
            cancel_tx.clone(),
            cancel_rx,
        ));

        let request_peer = peer.clone();
        let request = tokio::spawn(async move {
            request_peer
                .start_request("tools/call", serde_json::json!({}), Duration::from_secs(5))
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let changed = state.request_changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if state.request_seen.load(Ordering::SeqCst) {
                    return;
                }
                changed.await;
            }
        })
        .await
        .expect("original request body observed");
        assert!(
            !request.is_finished(),
            "request must not be wire-committed before the POST success response"
        );

        state.release_request.send_replace(true);
        let pending = request
            .await
            .expect("request task")
            .expect("request POST committed");
        drop(pending);

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let changed = state.cancellation_changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if state.cancellation_seen.load(Ordering::SeqCst) {
                    return;
                }
                changed.await;
            }
        })
        .await
        .expect("cancellation POST observed");
        assert_eq!(
            state.methods.lock().expect("methods").as_slice(),
            &["tools/call", "notifications/cancelled"]
        );

        cancel_tx.send_replace(true);
        dispatcher.await.expect("dispatcher");
        server.abort();
    }

    #[tokio::test]
    async fn server_request_responses_are_posted_while_another_post_is_blocked() {
        use axum::{body::Bytes, extract::State, http::StatusCode, routing::post, Router};

        #[derive(Clone)]
        struct StateData {
            blocked_seen: Arc<AtomicBool>,
            blocked_changed: Arc<Notify>,
            response_seen: Arc<AtomicBool>,
            response_changed: Arc<Notify>,
            release: watch::Sender<bool>,
        }

        async fn rpc(State(state): State<StateData>, body: Bytes) -> StatusCode {
            let message: Value = serde_json::from_slice(&body).expect("POST JSON");
            if message["method"] == "notifications/block" {
                state.blocked_seen.store(true, Ordering::SeqCst);
                state.blocked_changed.notify_waiters();
                wait_for_cancel(&mut state.release.subscribe()).await;
            } else if message["id"] == 99 && message.get("error").is_some() {
                state.response_seen.store(true, Ordering::SeqCst);
                state.response_changed.notify_waiters();
            } else {
                panic!("unexpected POST: {message}");
            }
            StatusCode::ACCEPTED
        }

        let state = StateData {
            blocked_seen: Arc::new(AtomicBool::new(false)),
            blocked_changed: Arc::new(Notify::new()),
            response_seen: Arc::new(AtomicBool::new(false)),
            response_changed: Arc::new(Notify::new()),
            release: watch::channel(false).0,
        };
        let app = Router::new()
            .route("/rpc", post(rpc))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let endpoint = Url::parse(&format!(
            "http://{}/rpc",
            listener.local_addr().expect("local address")
        ))
        .expect("endpoint");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let (outbound_tx, outbound_rx) = mpsc::channel(OUTBOUND_CAPACITY);
        let sink = Arc::new(SseMessageSink::new(
            outbound_tx,
            Duration::from_secs(5),
            Duration::from_secs(5),
        ));
        let peer = McpPeer::builder(sink, ready_session())
            .build()
            .expect("peer");
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let dispatcher = tokio::spawn(run_dispatcher(
            reqwest::Client::builder()
                .pool_max_idle_per_host(0)
                .no_proxy()
                .build()
                .expect("client"),
            endpoint,
            client_auth::McpClientAuthProvider::new(&crate::McpClientAuth::None),
            peer.clone(),
            outbound_rx,
            cancel_tx.clone(),
            cancel_rx,
        ));

        let blocking_peer = peer.clone();
        let blocking_post = tokio::spawn(async move {
            blocking_peer
                .notify_without_params("notifications/block")
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let changed = state.blocked_changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if state.blocked_seen.load(Ordering::SeqCst) {
                    return;
                }
                changed.await;
            }
        })
        .await
        .expect("blocking POST started");

        let request = serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "fixture/unknown"
        }))
        .expect("server request");
        peer.receive(request).await.expect("receive server request");
        peer.wait_for_inbound_tasks().await;

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let changed = state.response_changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if state.response_seen.load(Ordering::SeqCst) {
                    return;
                }
                changed.await;
            }
        })
        .await
        .expect("server request response uses a concurrent POST worker");

        state.release.send_replace(true);
        blocking_post
            .await
            .expect("blocking POST task")
            .expect("blocking POST accepted");
        cancel_tx.send_replace(true);
        dispatcher.await.expect("dispatcher");
        server.abort();
    }

    #[test]
    fn resolves_relative_and_same_origin_absolute_endpoints() {
        let base = Url::parse("https://example.test/events/session").expect("base");
        assert_eq!(
            resolve_endpoint(&base, "../rpc?session=1")
                .expect("relative")
                .as_str(),
            "https://example.test/rpc?session=1"
        );
        assert_eq!(
            resolve_endpoint(&base, "https://example.test:443/rpc")
                .expect("absolute")
                .as_str(),
            "https://example.test/rpc"
        );
    }

    #[test]
    fn rejects_unsafe_discovered_endpoints() {
        let base = Url::parse("https://example.test/events").expect("base");
        for endpoint in [
            "http://example.test/rpc",
            "https://other.example.test/rpc",
            "https://user@example.test/rpc",
            "https://example.test/rpc#fragment",
            "file:///tmp/rpc",
        ] {
            assert!(resolve_endpoint(&base, endpoint).is_err(), "{endpoint}");
        }
    }

    #[tokio::test]
    async fn endpoint_discovery_ignores_control_only_events() {
        let mut stream = futures::stream::iter([Ok::<_, Infallible>(
            b"retry: 1000\nid: cursor\n\nevent: endpoint\ndata: /rpc\n\n".as_slice(),
        )]);
        let mut decoder = SseDecoder::new(SseLimits::default());
        let base = Url::parse("https://example.test/events").expect("base");

        let (endpoint, buffered) = discover_endpoint(&mut stream, &mut decoder, &base)
            .await
            .expect("endpoint after control event");

        assert_eq!(endpoint.as_str(), "https://example.test/rpc");
        assert!(buffered.is_empty());
    }

    #[tokio::test]
    async fn event_reader_ignores_control_only_events() {
        receive_event(
            &test_peer(),
            SseEvent {
                event: None,
                data: String::new(),
                id: Some("cursor".to_owned()),
                retry_ms: Some(1_000),
            },
        )
        .await
        .expect("control-only event");
    }

    #[tokio::test]
    async fn event_reader_cancels_transport_when_stream_ends() {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        run_event_reader(
            futures::stream::empty::<Result<Vec<u8>, Infallible>>(),
            SseDecoder::new(SseLimits::default()),
            Vec::new(),
            test_peer(),
            cancel_tx.clone(),
            cancel_rx,
        )
        .await;

        assert!(*cancel_tx.borrow());
    }
}

use std::{net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::{
    net::TcpStream,
    sync::{broadcast, mpsc, watch, Mutex},
    task::JoinHandle,
};
use tokio_stream::wrappers::BroadcastStream;
use tokio_tungstenite::{
    client_async_tls,
    tungstenite::{
        client::IntoClientRequest,
        http::{HeaderName, HeaderValue, StatusCode},
        Error as WebSocketError, Message,
    },
    MaybeTlsStream, WebSocketStream,
};

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
    McpToolResult, McpTransport, NoopMcpMetricsSink, TransportChoice,
};

#[cfg(test)]
use crate::McpClientCapabilities;

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type WsWriter = futures::stream::SplitSink<WsStream, Message>;
type WsReader = futures::stream::SplitStream<WsStream>;

const OUTBOUND_CAPACITY: usize = 64;
const CLOSE_TIMEOUT: Duration = Duration::from_millis(250);

pub struct WebsocketTransport {
    metrics_sink: Arc<dyn crate::McpMetricsSink>,
    pinned_resolutions: Vec<(String, Vec<SocketAddr>)>,
}

impl WebsocketTransport {
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

    pub fn with_pinned_resolution(
        mut self,
        host: impl Into<String>,
        addrs: Vec<SocketAddr>,
    ) -> Self {
        let host = host.into();
        let host = super::network_endpoint::normalize_endpoint_host_key(&host).unwrap_or(host);
        self.pinned_resolutions.push((host, addrs));
        self
    }
}

impl Default for WebsocketTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl McpTransport for WebsocketTransport {
    fn transport_id(&self) -> &'static str {
        "websocket"
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
        let TransportChoice::WebSocket { url, headers } = spec.transport.clone() else {
            return Err(McpError::Unsupported(
                "WebsocketTransport requires TransportChoice::WebSocket".to_owned(),
            ));
        };
        let endpoint = parse_websocket_endpoint(&url)?;
        validate_websocket_headers(&headers)?;

        let auth_provider = client_auth::McpClientAuthProvider::new(&spec.auth)
            .with_metrics_sink(context.metrics_sink_or(Arc::clone(&self.metrics_sink)));
        let handshake = async {
            let resolved = super::network_endpoint::resolve_network_endpoint(
                &endpoint,
                self.pinned_resolutions
                    .iter()
                    .rev()
                    .find(|(host, _)| host.eq_ignore_ascii_case(&endpoint.host))
                    .map(|(_, addrs)| addrs.as_slice()),
            )
            .await?;
            let request =
                websocket_request(endpoint.url.as_str(), &headers, &auth_provider).await?;
            match websocket_handshake(request, &resolved).await {
                Ok(connection) => Ok(connection),
                Err(error) if is_auth_expired_handshake(&error) && auth_provider.can_refresh() => {
                    auth_provider.force_refresh_authorization_header().await?;
                    let request =
                        websocket_request(endpoint.url.as_str(), &headers, &auth_provider).await?;
                    websocket_handshake(request, &resolved)
                        .await
                        .map_err(|error| McpError::Transport(error.to_string()))
                }
                Err(error) => Err(McpError::Transport(error.to_string())),
            }
        };
        let (socket, _) = tokio::time::timeout(spec.timeouts.handshake, handshake)
            .await
            .map_err(|_| McpError::Connection("websocket handshake timed out".to_owned()))??;
        let (writer, reader) = socket.split();
        let (changes, _) = broadcast::channel(64);
        let (outbound_tx, outbound_rx) = mpsc::channel(OUTBOUND_CAPACITY);
        let sink = Arc::new(WebsocketMessageSink::new(outbound_tx));
        let support = client_inbound_support(&spec, &context);
        let session = McpSession::new(
            spec.capabilities_expected,
            support.capabilities,
            McpImplementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
        );
        let notification_handler = Arc::new(WebsocketNotificationHandler {
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
        let writer_task = tokio::spawn(run_writer(
            writer,
            outbound_rx,
            peer.clone(),
            cancel.clone(),
            cancel_rx,
        ));
        let reader_task = tokio::spawn(run_reader(
            reader,
            peer.clone(),
            cancel.clone(),
            cancel.subscribe(),
        ));

        if let Err(error) = peer.initialize(spec.timeouts.handshake).await {
            cancel.send_replace(true);
            sink.close().await;
            writer_task.abort();
            reader_task.abort();
            peer.close(format!("websocket initialize failed: {error}"))
                .await;
            return Err(error);
        }

        Ok(Arc::new(WebsocketConnection {
            connection_id: format!("websocket:{}", spec.server_id.0),
            changes,
            timeout: spec.timeouts.call_default,
            peer,
            sink,
            cancel,
            writer_task: Mutex::new(Some(writer_task)),
            reader_task: Mutex::new(Some(reader_task)),
            legacy_request_builder: JsonRpcPeer::new(),
        }))
    }
}

fn parse_websocket_endpoint(
    raw: &str,
) -> Result<super::network_endpoint::ParsedNetworkEndpoint, McpError> {
    let mut url = url::Url::parse(raw)
        .map_err(|_| McpError::Protocol("invalid MCP WebSocket endpoint URL".to_owned()))?;
    if !matches!(url.scheme(), "ws" | "wss") {
        return Err(McpError::Protocol(
            "MCP WebSocket endpoint must use ws or wss".to_owned(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(McpError::Protocol(
            "MCP WebSocket endpoint must not contain userinfo".to_owned(),
        ));
    }
    if url.fragment().is_some() {
        return Err(McpError::Protocol(
            "MCP WebSocket endpoint must not contain a fragment".to_owned(),
        ));
    }
    let (host, kind) = super::network_endpoint::normalize_endpoint_host(
        url.host()
            .ok_or_else(|| McpError::Protocol("MCP WebSocket endpoint has no host".to_owned()))?,
    )?;
    if matches!(
        kind,
        super::network_endpoint::NetworkHostKind::Localhost
            | super::network_endpoint::NetworkHostKind::DnsName
    ) {
        url.set_host(Some(&host))
            .map_err(|_| McpError::Protocol("invalid MCP WebSocket endpoint host".to_owned()))?;
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| McpError::Protocol("MCP WebSocket endpoint has no valid port".to_owned()))?;
    Ok(super::network_endpoint::ParsedNetworkEndpoint {
        url,
        host,
        port,
        kind,
    })
}

fn validate_websocket_headers(
    headers: &std::collections::BTreeMap<String, String>,
) -> Result<(), McpError> {
    for key in headers.keys() {
        let name = HeaderName::try_from(key.as_str())
            .map_err(|error| McpError::Transport(error.to_string()))?;
        if is_websocket_transport_owned_header(&name) {
            return Err(McpError::Protocol(format!(
                "WebSocket header {name} is owned by the MCP transport"
            )));
        }
    }
    Ok(())
}

async fn websocket_handshake(
    request: tokio_tungstenite::tungstenite::handshake::client::Request,
    resolved: &[SocketAddr],
) -> Result<
    (
        WsStream,
        tokio_tungstenite::tungstenite::handshake::client::Response,
    ),
    WebSocketError,
> {
    let stream = connect_resolved(resolved).await?;
    client_async_tls(request, stream).await
}

async fn connect_resolved(resolved: &[SocketAddr]) -> Result<TcpStream, WebSocketError> {
    let mut last_error = None;
    for address in resolved {
        match TcpStream::connect(address).await {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
    }
    Err(WebSocketError::Io(last_error.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            "WebSocket endpoint resolved to no addresses",
        )
    })))
}

async fn websocket_request(
    url: &str,
    headers: &std::collections::BTreeMap<String, String>,
    auth_provider: &client_auth::McpClientAuthProvider,
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, McpError> {
    let mut request = url
        .into_client_request()
        .map_err(|error| McpError::Transport(error.to_string()))?;
    for (key, value) in headers {
        let name = HeaderName::try_from(key.as_str())
            .map_err(|error| McpError::Transport(error.to_string()))?;
        let value = HeaderValue::try_from(value.as_str())
            .map_err(|error| McpError::Transport(error.to_string()))?;
        request.headers_mut().insert(name, value);
    }
    if let Some(authorization) = auth_provider.authorization_header().await? {
        let value = HeaderValue::try_from(authorization.as_str())
            .map_err(|error| McpError::Transport(error.to_string()))?;
        request.headers_mut().insert("authorization", value);
    }
    Ok(request)
}

fn is_websocket_transport_owned_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "authorization"
            | "connection"
            | "content-length"
            | "host"
            | "proxy-authorization"
            | "transfer-encoding"
            | "upgrade"
    ) || name.as_str().starts_with("sec-websocket-")
}

fn is_auth_expired_handshake(error: &WebSocketError) -> bool {
    matches!(
        error,
        WebSocketError::Http(response)
            if matches!(
                response.status(),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            )
    )
}

struct WebsocketMessageSink {
    sender: Mutex<Option<mpsc::Sender<McpOutboundMessage>>>,
}

impl WebsocketMessageSink {
    fn new(sender: mpsc::Sender<McpOutboundMessage>) -> Self {
        Self {
            sender: Mutex::new(Some(sender)),
        }
    }

    async fn close(&self) {
        self.sender.lock().await.take();
    }
}

#[async_trait]
impl McpMessageSink for WebsocketMessageSink {
    async fn send(&self, message: McpOutboundMessage) -> Result<(), McpError> {
        let sender = self
            .sender
            .lock()
            .await
            .clone()
            .ok_or_else(|| McpError::Connection("websocket writer is closed".to_owned()))?;
        let permit = sender
            .reserve_owned()
            .await
            .map_err(|_| McpError::Connection("websocket writer is closed".to_owned()))?;
        permit.send(message);
        Ok(())
    }
}

struct WebsocketNotificationHandler {
    changes: broadcast::Sender<McpChange>,
}

impl McpOrderedNotificationHandler for WebsocketNotificationHandler {
    fn handle_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        if let Some(change) =
            notification_change(&notification.method, notification.params.as_ref())
        {
            let _ = self.changes.send(change);
        }
        Ok(())
    }
}

pub struct WebsocketConnection {
    connection_id: String,
    changes: broadcast::Sender<McpChange>,
    timeout: Duration,
    peer: McpPeer,
    sink: Arc<WebsocketMessageSink>,
    cancel: watch::Sender<bool>,
    writer_task: Mutex<Option<JoinHandle<()>>>,
    reader_task: Mutex<Option<JoinHandle<()>>>,
    legacy_request_builder: JsonRpcPeer,
}

impl WebsocketConnection {
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
impl McpConnection for WebsocketConnection {
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
        self.peer.close("websocket connection shutting down").await;
        self.sink.close().await;
        finish_writer(&self.writer_task).await;
        stop_task(&self.reader_task).await;
        Ok(())
    }
}

impl Drop for WebsocketConnection {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
        if let Some(task) = self.writer_task.get_mut().take() {
            task.abort();
        }
        if let Some(task) = self.reader_task.get_mut().take() {
            task.abort();
        }
    }
}

async fn run_writer(
    mut writer: WsWriter,
    mut outbound: mpsc::Receiver<McpOutboundMessage>,
    peer: McpPeer,
    cancel_tx: watch::Sender<bool>,
    mut cancel: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            biased;
            _ = wait_for_cancel(&mut cancel) => {
                let _ = writer.send(Message::Close(None)).await;
                let _ = writer.close().await;
                return;
            }
            message = outbound.recv() => {
                let Some(message) = message else {
                    let _ = writer.send(Message::Close(None)).await;
                    let _ = writer.close().await;
                    return;
                };
                let payload = match serde_json::to_string(message.as_message()) {
                    Ok(payload) => payload,
                    Err(error) => {
                        fail_writer(
                            &peer,
                            &cancel_tx,
                            format!("websocket encode failed: {error}"),
                        )
                        .await;
                        return;
                    }
                };
                if let Err(error) = writer.send(Message::text(payload)).await {
                    fail_writer(
                        &peer,
                        &cancel_tx,
                        format!("websocket write failed: {error}"),
                    )
                    .await;
                    return;
                }
            }
        }
    }
}

async fn fail_writer(peer: &McpPeer, cancel: &watch::Sender<bool>, reason: impl Into<String>) {
    cancel.send_replace(true);
    peer.close(reason.into()).await;
}

async fn run_reader(
    mut reader: WsReader,
    peer: McpPeer,
    cancel_tx: watch::Sender<bool>,
    mut cancel: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            biased;
            _ = wait_for_cancel(&mut cancel) => return,
            frame = reader.next() => {
                let message = match frame {
                    Some(Ok(Message::Text(text))) => serde_json::from_str::<McpMessage>(text.as_ref()),
                    Some(Ok(Message::Binary(bytes))) => serde_json::from_slice::<McpMessage>(bytes.as_ref()),
                    Some(Ok(Message::Close(_))) | None => {
                        cancel_tx.send_replace(true);
                        peer.close("websocket closed").await;
                        return;
                    }
                    Some(Ok(_)) => continue,
                    Some(Err(error)) => {
                        cancel_tx.send_replace(true);
                        peer.close(format!("websocket read failed: {error}")).await;
                        return;
                    }
                };
                match message {
                    Ok(message) => {
                        if let Err(error) = peer.receive(message).await {
                            cancel_tx.send_replace(true);
                            peer.close(format!("websocket message failed: {error}")).await;
                            return;
                        }
                    }
                    Err(error) => {
                        cancel_tx.send_replace(true);
                        peer.close(format!("websocket JSON failed: {error}")).await;
                        return;
                    }
                }
            }
        }
    }
}

async fn finish_writer(task: &Mutex<Option<JoinHandle<()>>>) {
    let Some(mut task) = task.lock().await.take() else {
        return;
    };
    if tokio::time::timeout(CLOSE_TIMEOUT, &mut task)
        .await
        .is_err()
    {
        task.abort();
        let _ = task.await;
    }
}

async fn stop_task(task: &Mutex<Option<JoinHandle<()>>>) {
    if let Some(task) = task.lock().await.take() {
        task.abort();
        let _ = task.await;
    }
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
    use tokio::sync::oneshot;

    struct TaskDropSignal(Option<oneshot::Sender<()>>);

    impl Drop for TaskDropSignal {
        fn drop(&mut self) {
            if let Some(signal) = self.0.take() {
                let _ = signal.send(());
            }
        }
    }

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

    #[test]
    fn endpoint_parser_accepts_ws_wss_and_explicit_local_addresses() {
        let ws = parse_websocket_endpoint("ws://127.0.0.1:3000/socket").expect("ws endpoint");
        assert_eq!(ws.port, 3000);
        assert!(matches!(
            ws.kind,
            super::super::network_endpoint::NetworkHostKind::IpLiteral(ip)
                if ip.is_loopback()
        ));

        let wss =
            parse_websocket_endpoint("wss://public.example.test/socket").expect("wss endpoint");
        assert_eq!(wss.port, 443);
        assert!(matches!(
            wss.kind,
            super::super::network_endpoint::NetworkHostKind::DnsName
        ));
    }

    #[tokio::test]
    async fn public_dns_pins_are_accepted_without_changing_the_url_port() {
        let endpoint =
            parse_websocket_endpoint("wss://public.example.test:8443/socket").expect("endpoint");
        let pinned = ["8.8.8.8:53".parse().expect("public address")];

        let resolved =
            super::super::network_endpoint::resolve_network_endpoint(&endpoint, Some(&pinned))
                .await
                .expect("public DNS pin");

        assert_eq!(
            resolved,
            ["8.8.8.8:8443".parse().expect("resolved address")]
        );
    }

    #[tokio::test]
    async fn bounded_sink_is_cancellation_safe_while_waiting_for_capacity() {
        let (sender, mut receiver) = mpsc::channel(1);
        let sink = Arc::new(WebsocketMessageSink::new(sender));
        sink.send(
            McpOutboundMessage::notification_without_params("notifications/first")
                .expect("first message"),
        )
        .await
        .expect("first send");

        let blocked_sink = Arc::clone(&sink);
        let blocked = tokio::spawn(async move {
            blocked_sink
                .send(
                    McpOutboundMessage::notification_without_params("notifications/second")
                        .expect("second message"),
                )
                .await
        });
        tokio::task::yield_now().await;
        assert!(!blocked.is_finished());
        blocked.abort();
        let _ = blocked.await;

        let first = receiver.recv().await.expect("first queued message");
        assert!(matches!(
            first.as_message(),
            McpMessage::Notification(notification)
                if notification.method == "notifications/first"
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn writer_failure_cancels_transport_and_closes_peer() {
        let peer = test_peer();
        let (cancel_tx, _) = watch::channel(false);

        fail_writer(&peer, &cancel_tx, "test failure").await;

        assert!(*cancel_tx.borrow());
        assert!(peer.ensure_open().is_err());
    }

    #[tokio::test]
    async fn dropping_connection_aborts_tasks_that_ignore_cancellation() {
        let (writer_dropped_tx, writer_dropped_rx) = oneshot::channel();
        let (writer_started_tx, writer_started_rx) = oneshot::channel();
        let writer_task = tokio::spawn(async move {
            let _signal = TaskDropSignal(Some(writer_dropped_tx));
            let _ = writer_started_tx.send(());
            std::future::pending::<()>().await;
        });
        let (reader_dropped_tx, reader_dropped_rx) = oneshot::channel();
        let (reader_started_tx, reader_started_rx) = oneshot::channel();
        let reader_task = tokio::spawn(async move {
            let _signal = TaskDropSignal(Some(reader_dropped_tx));
            let _ = reader_started_tx.send(());
            std::future::pending::<()>().await;
        });
        let (outbound_tx, _outbound_rx) = mpsc::channel(1);
        let (cancel, _) = watch::channel(false);
        let connection = WebsocketConnection {
            connection_id: "websocket:test".to_owned(),
            changes: broadcast::channel(1).0,
            timeout: Duration::from_secs(1),
            peer: test_peer(),
            sink: Arc::new(WebsocketMessageSink::new(outbound_tx)),
            cancel,
            writer_task: Mutex::new(Some(writer_task)),
            reader_task: Mutex::new(Some(reader_task)),
            legacy_request_builder: JsonRpcPeer::new(),
        };

        writer_started_rx.await.expect("writer task started");
        reader_started_rx.await.expect("reader task started");
        drop(connection);

        tokio::time::timeout(Duration::from_millis(100), writer_dropped_rx)
            .await
            .expect("writer task aborted")
            .expect("writer drop signal");
        tokio::time::timeout(Duration::from_millis(100), reader_dropped_rx)
            .await
            .expect("reader task aborted")
            .expect("reader drop signal");
    }
}

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use harness_contracts::PermissionMode;
use serde_json::Value;
use tokio::sync::{broadcast, oneshot, Mutex};
use tokio_stream::wrappers::BroadcastStream;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        http::{HeaderName, HeaderValue, StatusCode},
        Error as WebSocketError, Message,
    },
    MaybeTlsStream, WebSocketStream,
};

use crate::{
    authorize_mcp_transport_connect, call_tool_request, client_auth,
    continue_after_elicitation_response, decode_empty_result, decode_list_prompts,
    decode_list_resources, decode_list_tools, decode_prompt_messages, decode_read_resource,
    decode_tool_result, get_prompt_request, initialize_request, initialized_notification,
    list_prompts_request, list_resources_request, list_tools_request, notification_change,
    read_resource_request, response_key, subscribe_resource_request, tool_call_event_from_change,
    unsubscribe_resource_request, ElicitationHandler, JsonRpcNotification, JsonRpcPeer,
    JsonRpcRequest, JsonRpcResponse, ListChangedEvent, McpChange, McpConnectContext, McpConnection,
    McpError, McpListPage, McpMetricsSink, McpPrompt, McpPromptMessages, McpReadResourceResult,
    McpResource, McpServerSpec, McpToolCallEvent, McpToolCallStream, McpToolDescriptor,
    McpToolResult, McpTransport, NoopMcpMetricsSink, TransportChoice,
};

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type WsWriter = futures::stream::SplitSink<WsStream, Message>;
type PendingMap = Arc<Mutex<HashMap<String, oneshot::Sender<Result<JsonRpcResponse, McpError>>>>>;
type PendingReceiver = oneshot::Receiver<Result<JsonRpcResponse, McpError>>;

pub struct WebsocketTransport {
    metrics_sink: Arc<dyn McpMetricsSink>,
}

impl WebsocketTransport {
    pub fn new() -> Self {
        Self {
            metrics_sink: Arc::new(NoopMcpMetricsSink),
        }
    }

    pub fn with_metrics_sink(metrics_sink: Arc<dyn McpMetricsSink>) -> Self {
        Self { metrics_sink }
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
                "WebsocketTransport requires TransportChoice::WebSocket".into(),
            ));
        };

        let auth_provider = client_auth::McpClientAuthProvider::new(&spec.auth)
            .with_metrics_sink(context.metrics_sink_or(Arc::clone(&self.metrics_sink)));
        let request = websocket_request(&url, &headers, &auth_provider).await?;
        let (socket, _) = match connect_async(request).await {
            Ok(connection) => connection,
            Err(error) if is_auth_expired_handshake(&error) && auth_provider.can_refresh() => {
                auth_provider.force_refresh_authorization_header().await?;
                let request = websocket_request(&url, &headers, &auth_provider).await?;
                connect_async(request)
                    .await
                    .map_err(|error| McpError::Transport(error.to_string()))?
            }
            Err(error) => return Err(McpError::Transport(error.to_string())),
        };
        let (writer, reader) = socket.split();
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (changes, _) = broadcast::channel(64);
        spawn_reader(reader, Arc::clone(&pending), changes.clone());

        let connection = Arc::new(WebsocketConnection {
            connection_id: format!("websocket:{}", spec.server_id.0),
            writer: Arc::new(Mutex::new(writer)),
            pending,
            changes,
            timeout: spec.timeouts.call_default,
            peer: JsonRpcPeer::new(),
            elicitation_handler: context.elicitation_handler,
            permission_mode: context.permission_mode,
        });
        connection
            .send(initialize_request(&connection.peer))
            .await?;
        connection
            .send_notification(initialized_notification())
            .await?;
        Ok(connection)
    }
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

pub struct WebsocketConnection {
    connection_id: String,
    writer: Arc<Mutex<WsWriter>>,
    pending: PendingMap,
    changes: broadcast::Sender<McpChange>,
    timeout: std::time::Duration,
    peer: JsonRpcPeer,
    elicitation_handler: Option<Arc<dyn ElicitationHandler>>,
    permission_mode: PermissionMode,
}

impl WebsocketConnection {
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let method = request.method.clone();
        let key = response_key(&request.id);
        let receiver = self.begin_send(request).await?;

        match tokio::time::timeout(self.timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(McpError::Connection(
                "websocket response channel closed".into(),
            )),
            Err(_) => {
                self.pending.lock().await.remove(&key);
                Err(McpError::Connection(format!(
                    "websocket request timed out: {method}"
                )))
            }
        }
    }

    async fn begin_send(&self, request: JsonRpcRequest) -> Result<PendingReceiver, McpError> {
        let key = response_key(&request.id);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(key.clone(), sender);

        let payload = serde_json::to_string(&request)
            .map_err(|error| McpError::InvalidResponse(error.to_string()))?;
        if let Err(error) = self.writer.lock().await.send(Message::Text(payload)).await {
            self.pending.lock().await.remove(&key);
            return Err(McpError::Transport(error.to_string()));
        }

        Ok(receiver)
    }

    async fn send_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        let payload = serde_json::to_string(&notification)
            .map_err(|error| McpError::InvalidResponse(error.to_string()))?;
        self.writer
            .lock()
            .await
            .send(Message::Text(payload))
            .await
            .map_err(|error| McpError::Transport(error.to_string()))
    }

    async fn send_with_elicitation(
        &self,
        request: JsonRpcRequest,
    ) -> Result<JsonRpcResponse, McpError> {
        let response = self.send(request.clone()).await?;
        if let Some(retry) = continue_after_elicitation_response(
            &response,
            &request,
            &self.peer,
            self.elicitation_handler.as_ref(),
            self.permission_mode,
        )
        .await?
        {
            return self.send(retry).await;
        }
        Ok(response)
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
        decode_list_tools(self.send(list_tools_request(&self.peer, cursor)).await?)
    }

    async fn call_tool(&self, name: &str, args: Value) -> Result<McpToolResult, McpError> {
        decode_tool_result(
            self.send_with_elicitation(call_tool_request(&self.peer, name, args))
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
        let request = call_tool_request(&self.peer, name, args);
        let key = response_key(&request.id);
        let mut changes = BroadcastStream::new(self.changes.subscribe());
        let receiver = self.begin_send(request).await?;
        let timeout = self.timeout;
        let pending = Arc::clone(&self.pending);
        let timeout_key = key.clone();

        Ok(Box::pin(async_stream::stream! {
            let response = tokio::time::timeout(timeout, receiver);
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
                            None => {
                                changes_open = false;
                            },
                        },
                        result = &mut response => {
                            match result {
                                Ok(Ok(Ok(response))) => match decode_tool_result(response) {
                                    Ok(result) => yield McpToolCallEvent::Final(result),
                                    Err(error) => yield McpToolCallEvent::Error(error),
                                },
                                Ok(Ok(Err(error))) => yield McpToolCallEvent::Error(error),
                                Ok(Err(_)) => yield McpToolCallEvent::Error(McpError::Connection(
                                    "websocket response channel closed".into(),
                                )),
                                Err(_) => {
                                    pending.lock().await.remove(&timeout_key);
                                    yield McpToolCallEvent::Error(McpError::Connection(
                                        "websocket request timed out: tools/call".into(),
                                    ));
                                },
                            }
                            break;
                        },
                    }
                } else {
                    match (&mut response).await {
                        Ok(Ok(Ok(response))) => match decode_tool_result(response) {
                            Ok(result) => yield McpToolCallEvent::Final(result),
                            Err(error) => yield McpToolCallEvent::Error(error),
                        },
                        Ok(Ok(Err(error))) => yield McpToolCallEvent::Error(error),
                        Ok(Err(_)) => yield McpToolCallEvent::Error(McpError::Connection(
                            "websocket response channel closed".into(),
                        )),
                        Err(_) => {
                            pending.lock().await.remove(&timeout_key);
                            yield McpToolCallEvent::Error(McpError::Connection(
                                "websocket request timed out: tools/call".into(),
                            ));
                        },
                    }
                    break;
                }
            }
        }))
    }

    async fn list_resources(&self) -> Result<Vec<McpResource>, McpError> {
        self.list_resources_all().await
    }

    async fn list_resources_page(
        &self,
        cursor: Option<&str>,
    ) -> Result<McpListPage<McpResource>, McpError> {
        decode_list_resources(
            self.send(list_resources_request(&self.peer, cursor))
                .await?,
        )
    }

    async fn read_resource(&self, uri: &str) -> Result<McpReadResourceResult, McpError> {
        decode_read_resource(self.send(read_resource_request(&self.peer, uri)).await?)
    }

    async fn subscribe_resource(&self, uri: &str) -> Result<(), McpError> {
        decode_empty_result(
            self.send(subscribe_resource_request(&self.peer, uri))
                .await?,
        )
    }

    async fn unsubscribe_resource(&self, uri: &str) -> Result<(), McpError> {
        decode_empty_result(
            self.send(unsubscribe_resource_request(&self.peer, uri))
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
        decode_list_prompts(self.send(list_prompts_request(&self.peer, cursor)).await?)
    }

    async fn get_prompt(&self, name: &str, args: Value) -> Result<McpPromptMessages, McpError> {
        decode_prompt_messages(
            self.send(get_prompt_request(&self.peer, name, args))
                .await?,
        )
    }

    async fn subscribe_changes(&self) -> Result<ListChangedEvent, McpError> {
        let stream = BroadcastStream::new(self.changes.subscribe())
            .filter_map(|event| async move { event.ok() });
        Ok(Box::pin(stream))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        self.send_notification(JsonRpcNotification::new("shutdown", None))
            .await
    }
}

fn spawn_reader(
    mut reader: futures::stream::SplitStream<WsStream>,
    pending: PendingMap,
    changes: broadcast::Sender<McpChange>,
) {
    tokio::spawn(async move {
        while let Some(message) = reader.next().await {
            let text = match message {
                Ok(Message::Text(text)) => text,
                Ok(Message::Binary(bytes)) => match String::from_utf8(bytes) {
                    Ok(text) => text,
                    Err(error) => {
                        notify_all(&pending, McpError::InvalidResponse(error.to_string())).await;
                        break;
                    }
                },
                Ok(Message::Close(_)) => break,
                Ok(_) => continue,
                Err(error) => {
                    notify_all(&pending, McpError::Transport(error.to_string())).await;
                    break;
                }
            };

            let value = match serde_json::from_str::<Value>(&text) {
                Ok(value) => value,
                Err(error) => {
                    notify_all(&pending, McpError::InvalidResponse(error.to_string())).await;
                    break;
                }
            };

            if let Some(method) = value.get("method").and_then(Value::as_str) {
                if let Some(change) = notification_change(method, value.get("params")) {
                    let _ = changes.send(change);
                }
                continue;
            }

            let response = match serde_json::from_value::<JsonRpcResponse>(value) {
                Ok(response) => response,
                Err(error) => {
                    notify_all(&pending, McpError::InvalidResponse(error.to_string())).await;
                    break;
                }
            };
            let key = response_key(&response.id);
            if let Some(sender) = pending.lock().await.remove(&key) {
                let _ = sender.send(Ok(response));
            }
        }
        notify_all(&pending, McpError::Connection("websocket closed".into())).await;
    });
}

async fn notify_all(pending: &PendingMap, error: McpError) {
    let senders = pending
        .lock()
        .await
        .drain()
        .map(|(_, sender)| sender)
        .collect::<Vec<_>>();
    for sender in senders {
        let _ = sender.send(Err(error.clone()));
    }
}

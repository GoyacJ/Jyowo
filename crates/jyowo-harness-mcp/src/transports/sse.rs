use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::PermissionMode;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION},
    StatusCode,
};
use reqwest_eventsource::{Event, EventSource};
use serde_json::Value;
use tokio::sync::{broadcast, oneshot, Mutex};
use tokio_stream::wrappers::BroadcastStream;

use crate::{
    authorize_mcp_transport_connect, call_tool_request, client_auth,
    continue_after_elicitation_response, decode_empty_result, decode_list_prompts,
    decode_list_resources, decode_list_tools, decode_prompt_messages, decode_read_resource,
    decode_tool_result, get_prompt_request, initialize_request, initialized_notification,
    list_prompts_request, list_resources_request, list_tools_request, notification_change,
    read_resource_request, response_key, subscribe_resource_request, tool_call_event_from_change,
    unsubscribe_resource_request, ElicitationHandler, JsonRpcNotification, JsonRpcPeer,
    JsonRpcRequest, JsonRpcResponse, ListChangedEvent, McpChange, McpConnectContext, McpConnection,
    McpError, McpMetricsSink, McpPrompt, McpPromptMessages, McpResource, McpResourceContents,
    McpServerSpec, McpToolCallEvent, McpToolCallStream, McpToolDescriptor, McpToolResult,
    McpTransport, NoopMcpMetricsSink, TransportChoice,
};

type PendingMap = Arc<Mutex<HashMap<String, oneshot::Sender<Result<JsonRpcResponse, McpError>>>>>;
type PendingReceiver = oneshot::Receiver<Result<JsonRpcResponse, McpError>>;

pub struct SseTransport {
    metrics_sink: Arc<dyn McpMetricsSink>,
}

impl SseTransport {
    pub fn new() -> Self {
        Self {
            metrics_sink: Arc::new(NoopMcpMetricsSink),
        }
    }

    pub fn with_metrics_sink(metrics_sink: Arc<dyn McpMetricsSink>) -> Self {
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
                "SseTransport requires TransportChoice::Sse".into(),
            ));
        };

        let auth_provider = client_auth::McpClientAuthProvider::new(&spec.auth)
            .with_metrics_sink(context.metrics_sink_or(Arc::clone(&self.metrics_sink)));
        let authorization = auth_provider.authorization_header().await?;
        let default_headers = header_map(headers.clone(), None)?;
        let event_headers = event_header_map(headers, authorization.as_deref())?;
        let client = reqwest::Client::builder()
            .default_headers(default_headers)
            .pool_max_idle_per_host(0)
            .timeout(spec.timeouts.call_default)
            .build()
            .map_err(|error| McpError::Transport(error.to_string()))?;
        let event_client = reqwest_eventsource_client::Client::builder()
            .default_headers(event_headers)
            .timeout(spec.timeouts.call_default)
            .build()
            .map_err(|error| McpError::Transport(error.to_string()))?;
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (changes, _) = broadcast::channel(64);
        let events_url = format!("{}/events", url.trim_end_matches('/'));
        spawn_event_reader(
            event_client,
            events_url,
            Arc::clone(&pending),
            changes.clone(),
        )
        .await?;

        let connection = Arc::new(SseConnection {
            connection_id: format!("sse:{}", spec.server_id.0),
            endpoint: url,
            client,
            auth_provider,
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

pub struct SseConnection {
    connection_id: String,
    endpoint: String,
    client: reqwest::Client,
    auth_provider: client_auth::McpClientAuthProvider,
    pending: PendingMap,
    changes: broadcast::Sender<McpChange>,
    timeout: std::time::Duration,
    peer: JsonRpcPeer,
    elicitation_handler: Option<Arc<dyn ElicitationHandler>>,
    permission_mode: PermissionMode,
}

impl SseConnection {
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let method = request.method.clone();
        let key = response_key(&request.id);
        let receiver = self.begin_send(request).await?;

        match tokio::time::timeout(self.timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(McpError::Connection("sse response channel closed".into())),
            Err(_) => {
                self.pending.lock().await.remove(&key);
                Err(McpError::Connection(format!(
                    "sse request timed out: {method}"
                )))
            }
        }
    }

    async fn begin_send(&self, request: JsonRpcRequest) -> Result<PendingReceiver, McpError> {
        let key = response_key(&request.id);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(key.clone(), sender);

        if let Err(error) = self.post_json(&request).await {
            self.pending.lock().await.remove(&key);
            return Err(error);
        }

        Ok(receiver)
    }

    async fn send_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        self.post_json(&notification).await
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

    async fn post_json<T>(&self, body: &T) -> Result<(), McpError>
    where
        T: serde::Serialize + ?Sized,
    {
        let response = self.post_json_once(body).await?;
        let response = if is_auth_expired(response.status()) && self.auth_provider.can_refresh() {
            self.auth_provider
                .force_refresh_authorization_header()
                .await?;
            self.post_json_once(body).await?
        } else {
            response
        };
        response
            .error_for_status()
            .map_err(|error| McpError::Transport(error.to_string()))?;
        Ok(())
    }

    async fn post_json_once<T>(&self, body: &T) -> Result<reqwest::Response, McpError>
    where
        T: serde::Serialize + ?Sized,
    {
        let mut builder = self.client.post(&self.endpoint).json(body);
        if let Some(authorization) = self.auth_provider.authorization_header().await? {
            builder = builder.header(AUTHORIZATION, authorization);
        }
        builder
            .send()
            .await
            .map_err(|error| McpError::Transport(error.to_string()))
    }
}

fn is_auth_expired(status: StatusCode) -> bool {
    matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
}

#[async_trait]
impl McpConnection for SseConnection {
    fn connection_id(&self) -> &str {
        &self.connection_id
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        decode_list_tools(self.send(list_tools_request(&self.peer)).await?)
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
                                    "sse response channel closed".into(),
                                )),
                                Err(_) => {
                                    pending.lock().await.remove(&timeout_key);
                                    yield McpToolCallEvent::Error(McpError::Connection(
                                        "sse request timed out: tools/call".into(),
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
                            "sse response channel closed".into(),
                        )),
                        Err(_) => {
                            pending.lock().await.remove(&timeout_key);
                            yield McpToolCallEvent::Error(McpError::Connection(
                                "sse request timed out: tools/call".into(),
                            ));
                        },
                    }
                    break;
                }
            }
        }))
    }

    async fn list_resources(&self) -> Result<Vec<McpResource>, McpError> {
        decode_list_resources(self.send(list_resources_request(&self.peer)).await?)
    }

    async fn read_resource(&self, uri: &str) -> Result<McpResourceContents, McpError> {
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
        decode_list_prompts(self.send(list_prompts_request(&self.peer)).await?)
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

async fn spawn_event_reader(
    client: reqwest_eventsource_client::Client,
    events_url: String,
    pending: PendingMap,
    changes: broadcast::Sender<McpChange>,
) -> Result<(), McpError> {
    let mut stream = EventSource::new(client.get(events_url))
        .map_err(|error| McpError::Transport(error.to_string()))?;

    tokio::spawn(async move {
        while let Some(event) = stream.next().await {
            let data = match event {
                Ok(Event::Open) => continue,
                Ok(Event::Message(message)) => message.data,
                Err(error) => {
                    notify_all(&pending, McpError::Transport(error.to_string())).await;
                    break;
                }
            };
            handle_sse_data(&data, &pending, &changes).await;
        }
        notify_all(&pending, McpError::Connection("sse stream closed".into())).await;
    });

    Ok(())
}

async fn handle_sse_data(data: &str, pending: &PendingMap, changes: &broadcast::Sender<McpChange>) {
    let value = match serde_json::from_str::<Value>(data) {
        Ok(value) => value,
        Err(_) => return,
    };

    if let Some(method) = value.get("method").and_then(Value::as_str) {
        if let Some(change) = notification_change(method, value.get("params")) {
            let _ = changes.send(change);
        }
        return;
    }

    let response = match serde_json::from_value::<JsonRpcResponse>(value) {
        Ok(response) => response,
        Err(error) => {
            notify_all(pending, McpError::InvalidResponse(error.to_string())).await;
            return;
        }
    };
    let key = response_key(&response.id);
    if let Some(sender) = pending.lock().await.remove(&key) {
        let _ = sender.send(Ok(response));
    }
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

fn header_map(
    headers: std::collections::BTreeMap<String, String>,
    authorization: Option<&str>,
) -> Result<HeaderMap, McpError> {
    let mut default_headers = HeaderMap::new();
    for (key, value) in headers {
        let name = HeaderName::try_from(key.as_str())
            .map_err(|error| McpError::Transport(error.to_string()))?;
        let value = HeaderValue::try_from(value.as_str())
            .map_err(|error| McpError::Transport(error.to_string()))?;
        default_headers.insert(name, value);
    }
    if let Some(authorization) = authorization {
        let value = HeaderValue::try_from(authorization)
            .map_err(|error| McpError::Transport(error.to_string()))?;
        default_headers.insert(AUTHORIZATION, value);
    }
    Ok(default_headers)
}

fn event_header_map(
    headers: std::collections::BTreeMap<String, String>,
    authorization: Option<&str>,
) -> Result<reqwest_eventsource_client::header::HeaderMap, McpError> {
    let mut default_headers = reqwest_eventsource_client::header::HeaderMap::new();
    for (key, value) in headers {
        let name = reqwest_eventsource_client::header::HeaderName::try_from(key.as_str())
            .map_err(|error| McpError::Transport(error.to_string()))?;
        let value = reqwest_eventsource_client::header::HeaderValue::try_from(value.as_str())
            .map_err(|error| McpError::Transport(error.to_string()))?;
        default_headers.insert(name, value);
    }
    default_headers.insert(
        reqwest_eventsource_client::header::ACCEPT,
        reqwest_eventsource_client::header::HeaderValue::from_static("text/event-stream"),
    );
    if let Some(authorization) = authorization {
        let value = reqwest_eventsource_client::header::HeaderValue::try_from(authorization)
            .map_err(|error| McpError::Transport(error.to_string()))?;
        default_headers.insert(reqwest_eventsource_client::header::AUTHORIZATION, value);
    }
    Ok(default_headers)
}

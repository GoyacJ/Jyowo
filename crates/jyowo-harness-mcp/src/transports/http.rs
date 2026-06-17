use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::PermissionMode;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION},
    StatusCode,
};
use serde_json::Value;

use crate::{
    call_tool_request, client_auth, continue_after_elicitation_response, decode_empty_result,
    decode_list_prompts, decode_list_resources, decode_list_tools, decode_prompt_messages,
    decode_read_resource, decode_tool_result, get_prompt_request, initialize_request,
    initialized_notification, list_prompts_request, list_resources_request, list_tools_request,
    read_resource_request, subscribe_resource_request, unsubscribe_resource_request,
    ElicitationHandler, JsonRpcNotification, JsonRpcPeer, JsonRpcRequest, JsonRpcResponse,
    McpConnectContext, McpConnection, McpError, McpMetricsSink, McpPrompt, McpPromptMessages,
    McpResource, McpResourceContents, McpServerSpec, McpToolDescriptor, McpToolResult,
    McpTransport, NoopMcpMetricsSink, TransportChoice,
};

pub struct HttpTransport {
    metrics_sink: Arc<dyn McpMetricsSink>,
    redirects_disabled: bool,
    pinned_resolutions: Vec<(String, Vec<SocketAddr>)>,
}

impl HttpTransport {
    pub fn new() -> Self {
        Self {
            metrics_sink: Arc::new(NoopMcpMetricsSink),
            redirects_disabled: false,
            pinned_resolutions: Vec::new(),
        }
    }

    pub fn with_metrics_sink(metrics_sink: Arc<dyn McpMetricsSink>) -> Self {
        Self {
            metrics_sink,
            redirects_disabled: false,
            pinned_resolutions: Vec::new(),
        }
    }

    pub fn with_redirects_disabled(mut self) -> Self {
        self.redirects_disabled = true;
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
        let TransportChoice::Http { url, headers } = spec.transport.clone() else {
            return Err(McpError::Unsupported(
                "HttpTransport requires TransportChoice::Http".into(),
            ));
        };

        let mut default_headers = HeaderMap::new();
        for (key, value) in headers {
            let name = HeaderName::try_from(key.as_str())
                .map_err(|error| McpError::Transport(error.to_string()))?;
            let value = HeaderValue::try_from(value.as_str())
                .map_err(|error| McpError::Transport(error.to_string()))?;
            default_headers.insert(name, value);
        }
        let metrics_sink = context.metrics_sink_or(Arc::clone(&self.metrics_sink));
        let auth_provider = client_auth::McpClientAuthProvider::new(&spec.auth)
            .with_metrics_sink(metrics_sink)
            .with_lifecycle_events(
                spec.server_id.clone(),
                self.transport_id(),
                Arc::clone(&context.event_sink),
            );

        let mut client_builder = reqwest::Client::builder()
            .default_headers(default_headers)
            .pool_max_idle_per_host(0)
            .timeout(spec.timeouts.call_default);
        if self.redirects_disabled {
            client_builder = client_builder.redirect(reqwest::redirect::Policy::none());
        }
        for (host, addrs) in &self.pinned_resolutions {
            client_builder = client_builder.resolve_to_addrs(host, addrs);
        }
        let client = client_builder
            .build()
            .map_err(|error| McpError::Transport(error.to_string()))?;
        let connection = Arc::new(HttpConnection {
            connection_id: format!("http:{}", spec.server_id.0),
            endpoint: url,
            client,
            auth_provider,
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

pub struct HttpConnection {
    connection_id: String,
    endpoint: String,
    client: reqwest::Client,
    auth_provider: client_auth::McpClientAuthProvider,
    peer: JsonRpcPeer,
    elicitation_handler: Option<Arc<dyn ElicitationHandler>>,
    permission_mode: PermissionMode,
}

impl HttpConnection {
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let response = self.send_once(&request).await?;
        let response = if is_auth_expired(response.status()) && self.auth_provider.can_refresh() {
            self.auth_provider
                .force_refresh_authorization_header()
                .await?;
            self.send_once(&request).await?
        } else {
            response
        };
        response
            .error_for_status()
            .map_err(|error| McpError::Transport(error.to_string()))?
            .json::<JsonRpcResponse>()
            .await
            .map_err(|error| McpError::InvalidResponse(error.to_string()))
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

    async fn send_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        let response = self.send_notification_once(&notification).await?;
        let response = if is_auth_expired(response.status()) && self.auth_provider.can_refresh() {
            self.auth_provider
                .force_refresh_authorization_header()
                .await?;
            self.send_notification_once(&notification).await?
        } else {
            response
        };
        response
            .error_for_status()
            .map_err(|error| McpError::Transport(error.to_string()))?;
        Ok(())
    }

    async fn send_once(&self, request: &JsonRpcRequest) -> Result<reqwest::Response, McpError> {
        let mut builder = self.client.post(&self.endpoint).json(request);
        if let Some(authorization) = self.auth_provider.authorization_header().await? {
            builder = builder.header(AUTHORIZATION, authorization);
        }
        builder
            .send()
            .await
            .map_err(|error| McpError::Transport(error.to_string()))
    }

    async fn send_notification_once(
        &self,
        notification: &JsonRpcNotification,
    ) -> Result<reqwest::Response, McpError> {
        let mut builder = self.client.post(&self.endpoint).json(notification);
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
impl McpConnection for HttpConnection {
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

    async fn shutdown(&self) -> Result<(), McpError> {
        self.send_notification(JsonRpcNotification::new("shutdown", None))
            .await
    }
}

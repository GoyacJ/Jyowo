use std::{collections::BTreeSet, pin::Pin, sync::Arc};

use async_trait::async_trait;
use futures::{stream, Stream};
use harness_contracts::PermissionMode;
use serde_json::Value;

use crate::{
    ElicitationHandler, JsonRpcError, JsonRpcErrorResponse, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResultResponse, McpAuthorizationContext, McpConnectionState, McpError, McpEventSink,
    McpListPage, McpMessage, McpMetricsSink, McpPaginationLimits, McpPrompt, McpPromptMessages,
    McpReadResourceResult, McpResource, McpServerSpec, McpToolDescriptor, McpToolResult,
    NoopMcpEventSink,
};

/// An MCP message that passed the protocol shape checks at the transport boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct McpOutboundMessage(McpMessage);

impl McpOutboundMessage {
    pub fn request(
        id: impl Into<Value>,
        method: impl Into<String>,
        params: Value,
    ) -> Result<Self, McpError> {
        Self::checked(McpMessage::Request(JsonRpcRequest::new(
            id.into(),
            method,
            Some(params),
        )))
    }

    pub fn notification(method: impl Into<String>, params: Value) -> Result<Self, McpError> {
        Self::checked(McpMessage::Notification(JsonRpcNotification::new(
            method,
            Some(params),
        )))
    }

    pub fn notification_without_params(method: impl Into<String>) -> Result<Self, McpError> {
        Self::checked(McpMessage::Notification(JsonRpcNotification::new(
            method, None,
        )))
    }

    pub fn success(id: impl Into<Value>, result: Value) -> Result<Self, McpError> {
        Self::checked(McpMessage::SuccessResponse(JsonRpcResultResponse {
            jsonrpc: "2.0".to_owned(),
            id: id.into(),
            result,
            extra: Default::default(),
        }))
    }

    pub fn failure(id: impl Into<Value>, error: JsonRpcError) -> Result<Self, McpError> {
        Self::checked(McpMessage::ErrorResponse(JsonRpcErrorResponse {
            jsonrpc: "2.0".to_owned(),
            id: Some(id.into()),
            error,
            extra: Default::default(),
        }))
    }

    pub fn checked(message: McpMessage) -> Result<Self, McpError> {
        let value = serde_json::to_value(&message).map_err(|error| {
            McpError::Protocol(format!("invalid outbound MCP message: {error}"))
        })?;
        let checked = serde_json::from_value(value).map_err(|error| {
            McpError::Protocol(format!("invalid outbound MCP message: {error}"))
        })?;
        Ok(Self(checked))
    }

    #[must_use]
    pub fn as_message(&self) -> &McpMessage {
        &self.0
    }

    #[must_use]
    pub fn into_message(self) -> McpMessage {
        self.0
    }
}

#[async_trait]
pub trait McpMessageSink: Send + Sync + 'static {
    /// Commits one complete message to the transport.
    ///
    /// Implementations must be cancellation-safe: if this future is dropped before returning
    /// `Ok(())`, the message must not be committed later. `McpPeer` treats `Ok(())` as the exact
    /// point at which cancellation notifications become valid for the request.
    async fn send(&self, message: McpOutboundMessage) -> Result<(), McpError>;
}

pub type ListChangedEvent = Pin<Box<dyn Stream<Item = McpChange> + Send + 'static>>;
pub type McpToolCallStream = Pin<Box<dyn Stream<Item = McpToolCallEvent> + Send + 'static>>;

#[derive(Clone)]
pub struct McpConnectContext {
    pub event_sink: Arc<dyn McpEventSink>,
    pub metrics_sink: Option<Arc<dyn McpMetricsSink>>,
    pub elicitation_handler: Option<Arc<dyn ElicitationHandler>>,
    pub permission_mode: PermissionMode,
    pub authorization: Option<McpAuthorizationContext>,
    pub(crate) transport_authorized: bool,
}

impl Default for McpConnectContext {
    fn default() -> Self {
        Self {
            event_sink: Arc::new(NoopMcpEventSink),
            metrics_sink: None,
            elicitation_handler: None,
            permission_mode: PermissionMode::Default,
            authorization: None,
            transport_authorized: false,
        }
    }
}

impl McpConnectContext {
    #[must_use]
    pub fn with_event_sink(mut self, event_sink: Arc<dyn McpEventSink>) -> Self {
        self.event_sink = event_sink;
        self
    }

    #[must_use]
    pub fn with_metrics_sink(mut self, metrics_sink: Arc<dyn McpMetricsSink>) -> Self {
        self.metrics_sink = Some(metrics_sink);
        self
    }

    pub fn metrics_sink_or(&self, fallback: Arc<dyn McpMetricsSink>) -> Arc<dyn McpMetricsSink> {
        self.metrics_sink
            .as_ref()
            .map(Arc::clone)
            .unwrap_or(fallback)
    }

    #[must_use]
    pub fn with_elicitation_handler(mut self, handler: Arc<dyn ElicitationHandler>) -> Self {
        self.elicitation_handler = Some(handler);
        self
    }

    #[must_use]
    pub fn with_permission_mode(mut self, permission_mode: PermissionMode) -> Self {
        self.permission_mode = permission_mode;
        self
    }

    #[must_use]
    pub fn with_authorization(mut self, authorization: McpAuthorizationContext) -> Self {
        self.authorization = Some(authorization);
        self
    }

    pub(crate) fn with_transport_authorized(mut self) -> Self {
        self.transport_authorized = true;
        self
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum McpChange {
    ToolsListChanged,
    ResourcesListChanged,
    ResourceUpdated {
        uri: String,
    },
    PromptsListChanged,
    Cancelled {
        request_id: Option<String>,
        reason: Option<String>,
    },
    Progress {
        progress_token: Option<String>,
        progress: Option<f64>,
        total: Option<f64>,
        message: Option<String>,
    },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum McpToolCallEvent {
    Progress {
        progress_token: Option<String>,
        progress: Option<f64>,
        total: Option<f64>,
        message: Option<String>,
    },
    Cancelled {
        request_id: Option<String>,
        reason: Option<String>,
    },
    Final(McpToolResult),
    Error(McpError),
}

#[async_trait]
pub trait McpTransport: Send + Sync + 'static {
    fn transport_id(&self) -> &str;

    async fn connect(&self, spec: McpServerSpec) -> Result<Arc<dyn McpConnection>, McpError>;

    async fn connect_with_context(
        &self,
        spec: McpServerSpec,
        _context: McpConnectContext,
    ) -> Result<Arc<dyn McpConnection>, McpError> {
        self.connect(spec).await
    }
}

#[async_trait]
pub trait McpConnection: Send + Sync + 'static {
    fn connection_id(&self) -> &str;

    async fn connection_state(&self) -> McpConnectionState {
        McpConnectionState::Ready
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError>;

    async fn list_tools_page(
        &self,
        _cursor: Option<&str>,
    ) -> Result<McpListPage<McpToolDescriptor>, McpError> {
        Ok(McpListPage {
            items: self.list_tools().await?,
            next_cursor: None,
        })
    }

    async fn list_tools_all(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        self.list_tools_all_with_limits(McpPaginationLimits::default())
            .await
    }

    async fn list_tools_all_with_limits(
        &self,
        limits: McpPaginationLimits,
    ) -> Result<Vec<McpToolDescriptor>, McpError> {
        let mut items = Vec::new();
        let mut cursor = None;
        let mut seen = BTreeSet::new();
        for _ in 0..limits.max_pages {
            let page = self.list_tools_page(cursor.as_deref()).await?;
            ensure_item_limit(items.len(), page.items.len(), limits.max_items)?;
            items.extend(page.items);
            let Some(next_cursor) = page.next_cursor else {
                return Ok(items);
            };
            ensure_fresh_cursor(&mut seen, &next_cursor)?;
            cursor = Some(next_cursor);
        }
        Err(page_limit_error(limits.max_pages))
    }

    async fn call_tool(&self, name: &str, args: Value) -> Result<McpToolResult, McpError>;

    async fn call_tool_events(
        &self,
        name: &str,
        args: Value,
    ) -> Result<McpToolCallStream, McpError> {
        let result = self.call_tool(name, args).await?;
        Ok(Box::pin(stream::iter([McpToolCallEvent::Final(result)])))
    }

    async fn cancel_tool_call(
        &self,
        request_id: &str,
        reason: Option<String>,
    ) -> Result<(), McpError> {
        let _ = (request_id, reason);
        Err(McpError::Unsupported(
            "outbound cancellation is not implemented for this MCP connection".to_owned(),
        ))
    }

    async fn mark_unhealthy(&self, reason: String) -> Result<(), McpError> {
        let _ = reason;
        Ok(())
    }

    async fn list_resources(&self) -> Result<Vec<McpResource>, McpError> {
        Ok(Vec::new())
    }

    async fn list_resources_page(
        &self,
        _cursor: Option<&str>,
    ) -> Result<McpListPage<McpResource>, McpError> {
        Ok(McpListPage {
            items: self.list_resources().await?,
            next_cursor: None,
        })
    }

    async fn list_resources_all(&self) -> Result<Vec<McpResource>, McpError> {
        self.list_resources_all_with_limits(McpPaginationLimits::default())
            .await
    }

    async fn list_resources_all_with_limits(
        &self,
        limits: McpPaginationLimits,
    ) -> Result<Vec<McpResource>, McpError> {
        let mut items = Vec::new();
        let mut cursor = None;
        let mut seen = BTreeSet::new();
        for _ in 0..limits.max_pages {
            let page = self.list_resources_page(cursor.as_deref()).await?;
            ensure_item_limit(items.len(), page.items.len(), limits.max_items)?;
            items.extend(page.items);
            let Some(next_cursor) = page.next_cursor else {
                return Ok(items);
            };
            ensure_fresh_cursor(&mut seen, &next_cursor)?;
            cursor = Some(next_cursor);
        }
        Err(page_limit_error(limits.max_pages))
    }

    async fn read_resource(&self, uri: &str) -> Result<McpReadResourceResult, McpError> {
        Err(McpError::Protocol(format!(
            "resources/read not implemented for {uri}"
        )))
    }

    async fn subscribe_resource(&self, uri: &str) -> Result<(), McpError> {
        Err(McpError::Protocol(format!(
            "resources/subscribe not implemented for {uri}"
        )))
    }

    async fn unsubscribe_resource(&self, uri: &str) -> Result<(), McpError> {
        Err(McpError::Protocol(format!(
            "resources/unsubscribe not implemented for {uri}"
        )))
    }

    async fn list_prompts(&self) -> Result<Vec<McpPrompt>, McpError> {
        Ok(Vec::new())
    }

    async fn list_prompts_page(
        &self,
        _cursor: Option<&str>,
    ) -> Result<McpListPage<McpPrompt>, McpError> {
        Ok(McpListPage {
            items: self.list_prompts().await?,
            next_cursor: None,
        })
    }

    async fn list_prompts_all(&self) -> Result<Vec<McpPrompt>, McpError> {
        self.list_prompts_all_with_limits(McpPaginationLimits::default())
            .await
    }

    async fn list_prompts_all_with_limits(
        &self,
        limits: McpPaginationLimits,
    ) -> Result<Vec<McpPrompt>, McpError> {
        let mut items = Vec::new();
        let mut cursor = None;
        let mut seen = BTreeSet::new();
        for _ in 0..limits.max_pages {
            let page = self.list_prompts_page(cursor.as_deref()).await?;
            ensure_item_limit(items.len(), page.items.len(), limits.max_items)?;
            items.extend(page.items);
            let Some(next_cursor) = page.next_cursor else {
                return Ok(items);
            };
            ensure_fresh_cursor(&mut seen, &next_cursor)?;
            cursor = Some(next_cursor);
        }
        Err(page_limit_error(limits.max_pages))
    }

    async fn get_prompt(&self, name: &str, _args: Value) -> Result<McpPromptMessages, McpError> {
        Err(McpError::Protocol(format!(
            "prompts/get not implemented for {name}"
        )))
    }

    async fn subscribe_changes(&self) -> Result<ListChangedEvent, McpError> {
        Ok(Box::pin(stream::empty()))
    }

    async fn shutdown(&self) -> Result<(), McpError>;
}

fn ensure_item_limit(current: usize, added: usize, limit: usize) -> Result<(), McpError> {
    if current.saturating_add(added) <= limit {
        return Ok(());
    }
    Err(McpError::InvalidResponse(format!(
        "MCP pagination item limit exceeded ({limit})"
    )))
}

fn ensure_fresh_cursor(seen: &mut BTreeSet<String>, cursor: &str) -> Result<(), McpError> {
    if seen.insert(cursor.to_owned()) {
        return Ok(());
    }
    Err(McpError::InvalidResponse(
        "MCP pagination returned a repeated cursor".to_owned(),
    ))
}

fn page_limit_error(limit: usize) -> McpError {
    McpError::InvalidResponse(format!("MCP pagination page limit exhausted ({limit})"))
}

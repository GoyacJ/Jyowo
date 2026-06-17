use std::{pin::Pin, sync::Arc};

use async_trait::async_trait;
use futures::{stream, Stream};
use harness_contracts::PermissionMode;
use serde_json::Value;

use crate::{
    ElicitationHandler, McpConnectionState, McpError, McpEventSink, McpMetricsSink, McpPrompt,
    McpPromptMessages, McpResource, McpResourceContents, McpServerSpec, McpToolDescriptor,
    McpToolResult, NoopMcpEventSink,
};

pub type ListChangedEvent = Pin<Box<dyn Stream<Item = McpChange> + Send + 'static>>;
pub type McpToolCallStream = Pin<Box<dyn Stream<Item = McpToolCallEvent> + Send + 'static>>;

#[derive(Clone)]
pub struct McpConnectContext {
    pub event_sink: Arc<dyn McpEventSink>,
    pub metrics_sink: Option<Arc<dyn McpMetricsSink>>,
    pub elicitation_handler: Option<Arc<dyn ElicitationHandler>>,
    pub permission_mode: PermissionMode,
}

impl Default for McpConnectContext {
    fn default() -> Self {
        Self {
            event_sink: Arc::new(NoopMcpEventSink),
            metrics_sink: None,
            elicitation_handler: None,
            permission_mode: PermissionMode::Default,
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

    async fn read_resource(&self, uri: &str) -> Result<McpResourceContents, McpError> {
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

use serde_json::Map;

use crate::{
    InitializeParams, InitializeResult, JsonRpcNotification, McpClientCapabilities, McpError,
    McpExpectedCapabilities, McpImplementation, McpServerCapabilities, LATEST_PROTOCOL_VERSION,
    SUPPORTED_PROTOCOL_VERSIONS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpLifecycleState {
    New,
    Initializing,
    Negotiated,
    Ready,
    Failed,
}

#[derive(Debug, Clone)]
pub struct McpSession {
    state: McpLifecycleState,
    required_server_capabilities: McpExpectedCapabilities,
    offered_client_capabilities: McpClientCapabilities,
    client_info: McpImplementation,
    negotiated_protocol_version: Option<String>,
    server_capabilities: Option<McpServerCapabilities>,
    server_info: Option<McpImplementation>,
    instructions: Option<String>,
}

impl McpSession {
    pub fn new(
        required_server_capabilities: McpExpectedCapabilities,
        offered_client_capabilities: McpClientCapabilities,
        client_info: McpImplementation,
    ) -> Self {
        Self {
            state: McpLifecycleState::New,
            required_server_capabilities,
            offered_client_capabilities,
            client_info,
            negotiated_protocol_version: None,
            server_capabilities: None,
            server_info: None,
            instructions: None,
        }
    }

    pub fn begin_initialization(&mut self) -> Result<InitializeParams, McpError> {
        if self.state != McpLifecycleState::New {
            return Err(lifecycle_error("initialize request already created"));
        }
        self.state = McpLifecycleState::Initializing;
        Ok(InitializeParams {
            protocol_version: LATEST_PROTOCOL_VERSION.to_owned(),
            capabilities: self.offered_client_capabilities.clone(),
            client_info: self.client_info.clone(),
            extra: Map::new(),
        })
    }

    pub fn accept_initialize_result(&mut self, result: InitializeResult) -> Result<(), McpError> {
        if self.state != McpLifecycleState::Initializing {
            return Err(lifecycle_error(
                "initialize result received before initialize request",
            ));
        }
        if !SUPPORTED_PROTOCOL_VERSIONS.contains(&result.protocol_version.as_str()) {
            self.state = McpLifecycleState::Failed;
            return Err(McpError::Protocol(format!(
                "unsupported MCP protocol version {}",
                result.protocol_version
            )));
        }

        let missing = self
            .required_server_capabilities
            .missing_from(&result.capabilities);
        if !missing.is_empty() {
            self.state = McpLifecycleState::Failed;
            return Err(McpError::Protocol(format!(
                "MCP server is missing required capabilities: {}",
                missing.join(", ")
            )));
        }

        self.negotiated_protocol_version = Some(result.protocol_version);
        self.server_capabilities = Some(result.capabilities);
        self.server_info = Some(result.server_info);
        self.instructions = result.instructions;
        self.state = McpLifecycleState::Negotiated;
        Ok(())
    }

    pub fn initialized_notification(&self) -> Result<JsonRpcNotification, McpError> {
        if self.state != McpLifecycleState::Negotiated {
            return Err(lifecycle_error(
                "initialized notification requires a validated initialize result",
            ));
        }
        Ok(JsonRpcNotification::new("notifications/initialized", None))
    }

    pub fn mark_initialized_notification_sent(&mut self) -> Result<(), McpError> {
        if self.state != McpLifecycleState::Negotiated {
            return Err(lifecycle_error(
                "initialized notification was not ready to send",
            ));
        }
        self.state = McpLifecycleState::Ready;
        Ok(())
    }

    #[must_use]
    pub fn state(&self) -> McpLifecycleState {
        self.state
    }

    #[must_use]
    pub fn offered_client_capabilities(&self) -> &McpClientCapabilities {
        &self.offered_client_capabilities
    }

    #[must_use]
    pub fn negotiated_protocol_version(&self) -> Option<&str> {
        self.negotiated_protocol_version.as_deref()
    }

    #[must_use]
    pub fn server_capabilities(&self) -> Option<&McpServerCapabilities> {
        self.server_capabilities.as_ref()
    }

    #[must_use]
    pub fn server_info(&self) -> Option<&McpImplementation> {
        self.server_info.as_ref()
    }

    #[must_use]
    pub fn instructions(&self) -> Option<&str> {
        self.instructions.as_deref()
    }
}

fn lifecycle_error(message: impl Into<String>) -> McpError {
    McpError::Protocol(message.into())
}

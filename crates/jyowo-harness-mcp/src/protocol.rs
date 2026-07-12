use std::collections::BTreeMap;

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::{
    JsonRpcErrorResponse, JsonRpcNotification, JsonRpcRequest, JsonRpcResultResponse, McpError,
    McpExpectedCapabilities,
};

pub const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";
pub const SUPPORTED_PROTOCOL_VERSIONS: [&str; 4] = [
    LATEST_PROTOCOL_VERSION,
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
];

#[derive(Debug, Clone, PartialEq)]
pub enum McpMessage {
    Request(JsonRpcRequest),
    Notification(JsonRpcNotification),
    SuccessResponse(JsonRpcResultResponse),
    ErrorResponse(JsonRpcErrorResponse),
}

impl Serialize for McpMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Request(request) => request.serialize(serializer),
            Self::Notification(notification) => notification.serialize(serializer),
            Self::SuccessResponse(response) => response.serialize(serializer),
            Self::ErrorResponse(response) => response.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for McpMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| D::Error::custom("MCP message must be a JSON object"))?;

        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(D::Error::custom("MCP message must use JSON-RPC 2.0"));
        }

        let has_method = object.get("method").is_some();
        let has_id = object.get("id").is_some();
        let has_result = object.get("result").is_some();
        let has_error = object.get("error").is_some();

        if has_method {
            if has_result || has_error {
                return Err(D::Error::custom(
                    "JSON-RPC request or notification cannot contain result or error",
                ));
            }
            if object
                .get("params")
                .is_some_and(|params| !params.is_object())
            {
                return Err(D::Error::custom(
                    "JSON-RPC request or notification params must be an object",
                ));
            }
            if has_id {
                validate_request_id(object.get("id").expect("id presence checked"))
                    .map_err(D::Error::custom)?;
                return serde_json::from_value(value)
                    .map(Self::Request)
                    .map_err(D::Error::custom);
            }
            return serde_json::from_value(value)
                .map(Self::Notification)
                .map_err(D::Error::custom);
        }

        if has_result == has_error {
            return Err(D::Error::custom(
                "JSON-RPC response must contain exactly one of result or error",
            ));
        }
        if has_result {
            let id = object
                .get("id")
                .ok_or_else(|| D::Error::custom("JSON-RPC result response must contain an id"))?;
            validate_request_id(id).map_err(D::Error::custom)?;
            if !object
                .get("result")
                .expect("result presence checked")
                .is_object()
            {
                return Err(D::Error::custom(
                    "JSON-RPC result response result must be an object",
                ));
            }
            return serde_json::from_value(value)
                .map(Self::SuccessResponse)
                .map_err(D::Error::custom);
        }

        if let Some(id) = object.get("id") {
            validate_request_id(id).map_err(D::Error::custom)?;
        }
        serde_json::from_value(value)
            .map(Self::ErrorResponse)
            .map_err(D::Error::custom)
    }
}

fn validate_request_id(id: &Value) -> Result<(), &'static str> {
    if id.is_string() || id.is_i64() || id.is_u64() {
        Ok(())
    } else {
        Err("JSON-RPC request id must be a string or integer")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: McpClientCapabilities,
    pub client_info: McpImplementation,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: McpServerCapabilities,
    pub server_info: McpImplementation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpImplementation {
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icons: Option<Vec<McpIcon>>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl McpImplementation {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            title: None,
            description: None,
            website_url: None,
            icons: None,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpClientCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootsClientCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling: Option<SamplingClientCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<ElicitationClientCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasks: Option<ClientTasksCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<BTreeMap<String, Value>>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootsClientCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EmptyClientCapability {
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SamplingClientCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<EmptyClientCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<EmptyClientCapability>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ElicitationClientCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<EmptyClientCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<EmptyClientCapability>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClientTasksCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<EmptyClientCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<EmptyClientCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests: Option<ClientTaskRequestsCapability>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClientTaskRequestsCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling: Option<ClientTaskSamplingRequestsCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<ClientTaskElicitationRequestsCapability>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientTaskSamplingRequestsCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_message: Option<EmptyClientCapability>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClientTaskElicitationRequestsCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create: Option<EmptyClientCapability>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsServerCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesServerCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsServerCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<EmptyServerCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completions: Option<EmptyServerCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tasks: Option<ServerTasksCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<BTreeMap<String, Value>>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsServerCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcesServerCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptsServerCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EmptyServerCapability {
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerTasksCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<EmptyServerCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<EmptyServerCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests: Option<ServerTaskRequestsCapability>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerTaskRequestsCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<ServerTaskToolsRequestsCapability>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerTaskToolsRequestsCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<EmptyServerCapability>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpIcon {
    pub src: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sizes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<McpIconTheme>,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpIconTheme {
    Light,
    Dark,
}

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

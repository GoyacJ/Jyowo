use harness_tool::RegistrationError;

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
pub enum McpError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("remote JSON-RPC error ({code}): {message}", code = .0.code, message = .0.message)]
    RemoteJsonRpc(crate::JsonRpcError),
    #[error("connection: {0}")]
    Connection(String),
    #[error("server not found: {0}")]
    ServerNotFound(String),
    #[error("tool naming violation: {0}")]
    ToolNamingViolation(String),
    #[error("filter conflict: {0}")]
    FilterConflict(String),
    #[error("tool registry: {0}")]
    ToolRegistry(String),
    #[error("oauth: {0}")]
    OAuth(String),
    #[error("elicitation: {0}")]
    Elicitation(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
}

impl From<RegistrationError> for McpError {
    fn from(value: RegistrationError) -> Self {
        Self::ToolRegistry(value.to_string())
    }
}

use std::sync::Arc;

use crate::{McpConnectContext, McpConnection, McpError, McpServerSpec, McpTransport};

#[derive(Clone)]
pub struct McpClient {
    transport: Arc<dyn McpTransport>,
}

impl McpClient {
    pub fn new(transport: Arc<dyn McpTransport>) -> Self {
        Self { transport }
    }

    pub fn transport_id(&self) -> &str {
        self.transport.transport_id()
    }

    pub async fn connect(&self, spec: McpServerSpec) -> Result<Arc<dyn McpConnection>, McpError> {
        self.transport.connect(spec).await
    }

    pub async fn connect_with_context(
        &self,
        spec: McpServerSpec,
        context: McpConnectContext,
    ) -> Result<Arc<dyn McpConnection>, McpError> {
        self.transport.connect_with_context(spec, context).await
    }
}

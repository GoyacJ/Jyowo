use std::sync::Arc;

use async_trait::async_trait;

use crate::{McpConnection, McpError, McpServerSpec, McpTransport, TransportChoice};

pub struct InProcessTransport {
    connection: Arc<dyn McpConnection>,
}

impl InProcessTransport {
    pub fn from_connection(connection: Arc<dyn McpConnection>) -> Self {
        Self { connection }
    }
}

#[async_trait]
impl McpTransport for InProcessTransport {
    fn transport_id(&self) -> &'static str {
        "in-process"
    }

    async fn connect(&self, spec: McpServerSpec) -> Result<Arc<dyn McpConnection>, McpError> {
        if !matches!(spec.transport, TransportChoice::InProcess) {
            return Err(McpError::Unsupported(
                "InProcessTransport requires TransportChoice::InProcess".into(),
            ));
        }

        Ok(Arc::clone(&self.connection))
    }
}

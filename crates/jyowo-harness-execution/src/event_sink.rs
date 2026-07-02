use async_trait::async_trait;
use harness_contracts::{Event, SessionId, TenantId};

use crate::ExecutionError;

#[async_trait]
pub trait AuthorizationEventSink: Send + Sync + 'static {
    async fn emit_batch(
        &self,
        tenant_id: TenantId,
        session_id: SessionId,
        events: Vec<Event>,
    ) -> Result<(), ExecutionError>;
}

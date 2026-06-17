use async_trait::async_trait;
use harness_contracts::{ContextError, ContextStageId, SessionId};

use crate::ContextBuffer;

#[async_trait]
pub trait ContextProvider: Send + Sync + 'static {
    fn provider_id(&self) -> &str;
    fn stage(&self) -> ContextStageId;

    async fn apply(
        &self,
        ctx: &mut ContextBuffer,
        hint: &CompactHint,
    ) -> Result<ContextOutcome, ContextError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompactHint {
    pub estimated_tokens: u64,
    pub target_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextOutcome {
    NoChange,
    Modified { bytes_saved: u64 },
    Forked { new_session_id: SessionId },
}

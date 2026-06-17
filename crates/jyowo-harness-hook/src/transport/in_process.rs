use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{HookEventKind, HookFailureMode};

use crate::{HookContext, HookEvent, HookHandler, HookOutcome};

use super::{HookOutput, HookPayload, HookTransport};

#[derive(Clone)]
pub struct InProcessHookTransport {
    handler: Arc<dyn HookHandler>,
}

impl InProcessHookTransport {
    pub fn new(handler: Arc<dyn HookHandler>) -> Self {
        Self { handler }
    }

    pub fn inner(&self) -> Arc<dyn HookHandler> {
        Arc::clone(&self.handler)
    }
}

#[async_trait]
impl HookTransport for InProcessHookTransport {
    async fn invoke(&self, payload: HookPayload) -> HookOutput {
        self.handler.handle(payload.event, payload.ctx).await
    }
}

#[async_trait]
impl HookHandler for InProcessHookTransport {
    fn handler_id(&self) -> &str {
        self.handler.handler_id()
    }

    fn interested_events(&self) -> &[HookEventKind] {
        self.handler.interested_events()
    }

    fn priority(&self) -> i32 {
        self.handler.priority()
    }

    fn failure_mode(&self) -> HookFailureMode {
        self.handler.failure_mode()
    }

    async fn handle(
        &self,
        event: HookEvent,
        ctx: HookContext,
    ) -> Result<HookOutcome, harness_contracts::HookError> {
        self.invoke(HookPayload { event, ctx }).await
    }
}

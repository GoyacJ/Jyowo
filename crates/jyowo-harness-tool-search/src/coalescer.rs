use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use harness_contracts::{CacheImpact, HarnessError, RunId, SessionId, ToolName};
use tokio::sync::{oneshot, Mutex};

use crate::ReloadHandle;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct CoalesceKey {
    session_id: SessionId,
    run_id: RunId,
}

pub struct MaterializationCoalescer {
    window: Duration,
    max_batch: usize,
    inner: Arc<Mutex<CoalescerState>>,
}

#[derive(Default)]
struct CoalescerState {
    pending: HashMap<CoalesceKey, PendingBatch>,
}

struct PendingBatch {
    tools: Vec<ToolName>,
    waiters: Vec<oneshot::Sender<Result<CacheImpact, HarnessError>>>,
    handle: Arc<dyn ReloadHandle>,
}

impl MaterializationCoalescer {
    #[must_use]
    pub fn new(window: Duration, max_batch: usize) -> Arc<Self> {
        Arc::new(Self {
            window,
            max_batch: max_batch.max(1),
            inner: Arc::new(Mutex::new(CoalescerState::default())),
        })
    }

    pub async fn submit(
        self: &Arc<Self>,
        session_id: SessionId,
        run_id: RunId,
        tools: Vec<ToolName>,
        handle: Arc<dyn ReloadHandle>,
    ) -> Result<CacheImpact, HarnessError> {
        let key = CoalesceKey { session_id, run_id };
        let (tx, rx) = oneshot::channel();
        let should_flush_now = {
            let mut state = self.inner.lock().await;
            if let Some(batch) = state.pending.get_mut(&key) {
                push_unique(&mut batch.tools, tools);
                batch.waiters.push(tx);
                batch.tools.len() >= self.max_batch
            } else {
                state.pending.insert(
                    key,
                    PendingBatch {
                        tools: unique(tools),
                        waiters: vec![tx],
                        handle,
                    },
                );
                if self.window.is_zero() {
                    true
                } else {
                    let this = Arc::clone(self);
                    tokio::spawn(async move {
                        tokio::time::sleep(this.window).await;
                        this.flush_key(key).await;
                    });
                    false
                }
            }
        };

        if should_flush_now {
            self.flush_key(key).await;
        }

        rx.await
            .map_err(|_| HarnessError::Internal("coalescer closed".to_owned()))?
    }

    async fn flush_key(&self, key: CoalesceKey) {
        let Some(batch) = ({
            let mut state = self.inner.lock().await;
            state.pending.remove(&key)
        }) else {
            return;
        };
        let result = batch.handle.reload_with_add_tools(batch.tools).await;
        for waiter in batch.waiters {
            let _ = waiter.send(result.clone());
        }
    }
}

fn unique(tools: Vec<ToolName>) -> Vec<ToolName> {
    let mut result = Vec::new();
    push_unique(&mut result, tools);
    result
}

fn push_unique(target: &mut Vec<ToolName>, tools: Vec<ToolName>) {
    for tool in tools {
        if !target.contains(&tool) {
            target.push(tool);
        }
    }
}

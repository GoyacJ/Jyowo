use std::sync::Arc;

use harness_contracts::ToolUseId;
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RunControl {
    #[default]
    Continue,
    YieldAfterAtomicOperation,
    ForceStop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafePointDecision {
    Continue,
    Yield,
    ForceStop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnOutcome {
    YieldedAtSafePoint,
    ForceStopped {
        non_revertible_tool_use_ids: Vec<ToolUseId>,
    },
    ForceStopTimedOut {
        indeterminate_tool_use_ids: Vec<ToolUseId>,
    },
}

#[derive(Clone)]
pub struct RunControlHandle {
    inner: Arc<RunControlState>,
}

struct RunControlState {
    control: watch::Sender<RunControl>,
    outcome: watch::Sender<Option<TurnOutcome>>,
}

impl RunControlHandle {
    #[must_use]
    pub fn new() -> Self {
        let (control, _) = watch::channel(RunControl::Continue);
        let (outcome, _) = watch::channel(None);
        Self {
            inner: Arc::new(RunControlState { control, outcome }),
        }
    }

    pub fn request(&self, requested: RunControl) {
        self.inner.control.send_if_modified(|current| {
            let next = match (*current, requested) {
                (RunControl::ForceStop, _) | (_, RunControl::Continue) => *current,
                (_, RunControl::ForceStop) => RunControl::ForceStop,
                (RunControl::Continue, RunControl::YieldAfterAtomicOperation) => {
                    RunControl::YieldAfterAtomicOperation
                }
                (RunControl::YieldAfterAtomicOperation, RunControl::YieldAfterAtomicOperation) => {
                    *current
                }
            };
            if next == *current {
                false
            } else {
                *current = next;
                true
            }
        });
    }

    #[must_use]
    pub fn decision(&self) -> SafePointDecision {
        match *self.inner.control.borrow() {
            RunControl::Continue => SafePointDecision::Continue,
            RunControl::YieldAfterAtomicOperation => SafePointDecision::Yield,
            RunControl::ForceStop => SafePointDecision::ForceStop,
        }
    }

    pub async fn force_stop_requested(&self) {
        let mut control = self.inner.control.subscribe();
        loop {
            if *control.borrow_and_update() == RunControl::ForceStop {
                return;
            }
            let _ = control.changed().await;
        }
    }

    pub fn finish(&self, finished: TurnOutcome) {
        self.inner.outcome.send_if_modified(|outcome| {
            if outcome.is_some() {
                false
            } else {
                *outcome = Some(finished);
                true
            }
        });
    }

    pub async fn outcome(&self) -> TurnOutcome {
        let mut outcome = self.inner.outcome.subscribe();
        loop {
            if let Some(finished) = outcome.borrow_and_update().clone() {
                return finished;
            }
            let _ = outcome.changed().await;
        }
    }
}

impl Default for RunControlHandle {
    fn default() -> Self {
        Self::new()
    }
}

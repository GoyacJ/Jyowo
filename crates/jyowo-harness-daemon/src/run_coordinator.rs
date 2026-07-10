use chrono::{DateTime, Utc};
use harness_contracts::{
    IndeterminateToolDecision, RunSegmentId, RunTerminalReason, TaskId, ToolUseId,
};
use harness_engine::{RunControlHandle, TurnOutcome};
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartSegmentRequest {
    pub task_id: TaskId,
    pub segment_id: RunSegmentId,
    pub indeterminate_tools: Vec<IndeterminateToolDecision>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunCoordinatorEvent {
    Completed {
        segment_id: RunSegmentId,
        terminal_reason: RunTerminalReason,
        incomplete_output: bool,
        ended_at: DateTime<Utc>,
    },
    SafePointReached {
        segment_id: RunSegmentId,
        forced: bool,
        incomplete_output: bool,
        non_revertible_tool_use_ids: Vec<ToolUseId>,
        reached_at: DateTime<Utc>,
    },
    ForceStopTimedOut {
        segment_id: RunSegmentId,
        indeterminate_tool_use_ids: Vec<ToolUseId>,
        timed_out_at: DateTime<Utc>,
    },
}

pub struct RunningSegment {
    events: mpsc::UnboundedReceiver<RunCoordinatorEvent>,
    control: RunControlHandle,
}

impl RunningSegment {
    #[must_use]
    pub fn new(events: mpsc::UnboundedReceiver<RunCoordinatorEvent>) -> Self {
        Self {
            events,
            control: RunControlHandle::new(),
        }
    }

    #[must_use]
    pub fn with_control(
        segment_id: RunSegmentId,
        mut events: mpsc::UnboundedReceiver<RunCoordinatorEvent>,
        control: RunControlHandle,
    ) -> Self {
        let (sender, bridged_events) = mpsc::unbounded_channel();
        let outcome_control = control.clone();
        tokio::spawn(async move {
            let event = tokio::select! {
                biased;
                outcome = outcome_control.outcome() => Some(match outcome {
                    TurnOutcome::YieldedAtSafePoint => RunCoordinatorEvent::SafePointReached {
                        segment_id,
                        forced: false,
                        incomplete_output: true,
                        non_revertible_tool_use_ids: Vec::new(),
                        reached_at: Utc::now(),
                    },
                    TurnOutcome::ForceStopped { non_revertible_tool_use_ids } => {
                        RunCoordinatorEvent::SafePointReached {
                            segment_id,
                            forced: true,
                            incomplete_output: true,
                            non_revertible_tool_use_ids,
                            reached_at: Utc::now(),
                        }
                    }
                    TurnOutcome::ForceStopTimedOut { indeterminate_tool_use_ids } => {
                        RunCoordinatorEvent::ForceStopTimedOut {
                            segment_id,
                            indeterminate_tool_use_ids,
                            timed_out_at: Utc::now(),
                        }
                    }
                }),
                event = events.recv() => event,
            };
            if let Some(event) = event {
                let _ = sender.send(event);
            }
        });
        Self {
            events: bridged_events,
            control,
        }
    }

    pub(crate) fn into_events(self) -> mpsc::UnboundedReceiver<RunCoordinatorEvent> {
        self.events
    }

    #[must_use]
    pub(crate) fn control(&self) -> RunControlHandle {
        self.control.clone()
    }
}

pub trait RunCoordinatorFactory: Send + Sync + 'static {
    /// Durably accepts a segment start exactly once for the `(task_id, segment_id)` key.
    ///
    /// The daemon may call this again after a process crash before its outbox acknowledgement
    /// commits. Implementations must resume or reconnect the same logical segment without
    /// applying `indeterminate_tools` or starting tool execution more than once.
    fn spawn_idempotent(&self, request: StartSegmentRequest) -> RunningSegment;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_ready_control_outcome_wins_over_a_ready_completed_event() {
        for _ in 0..32 {
            let segment_id = RunSegmentId::new();
            let control = RunControlHandle::new();
            control.finish(TurnOutcome::ForceStopped {
                non_revertible_tool_use_ids: vec![ToolUseId::new()],
            });
            let (sender, receiver) = mpsc::unbounded_channel();
            sender
                .send(RunCoordinatorEvent::Completed {
                    segment_id,
                    terminal_reason: RunTerminalReason::Completed,
                    incomplete_output: false,
                    ended_at: Utc::now(),
                })
                .unwrap();
            let mut running =
                RunningSegment::with_control(segment_id, receiver, control).into_events();

            assert!(matches!(
                running.recv().await,
                Some(RunCoordinatorEvent::SafePointReached { forced: true, .. })
            ));
        }
    }
}

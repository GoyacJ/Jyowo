use chrono::{DateTime, Utc};
use harness_contracts::{RunSegmentId, RunTerminalReason, TaskId};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartSegmentRequest {
    pub task_id: TaskId,
    pub segment_id: RunSegmentId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunCoordinatorEvent {
    Completed {
        segment_id: RunSegmentId,
        terminal_reason: RunTerminalReason,
        incomplete_output: bool,
        ended_at: DateTime<Utc>,
    },
}

pub struct RunningSegment {
    events: mpsc::UnboundedReceiver<RunCoordinatorEvent>,
}

impl RunningSegment {
    #[must_use]
    pub const fn new(events: mpsc::UnboundedReceiver<RunCoordinatorEvent>) -> Self {
        Self { events }
    }

    pub(crate) fn into_events(self) -> mpsc::UnboundedReceiver<RunCoordinatorEvent> {
        self.events
    }
}

pub trait RunCoordinatorFactory: Send + Sync + 'static {
    fn spawn(&self, request: StartSegmentRequest) -> RunningSegment;
}

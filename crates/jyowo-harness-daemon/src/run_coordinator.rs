use std::{future::Future, sync::Arc};

use chrono::{DateTime, Utc};
use harness_contracts::{
    IndeterminateToolDecision, RunSegmentId, RunTerminalReason, TaskId, ToolUseId, WorkspaceLeaseId,
};
use harness_engine::{RunControlHandle, TurnOutcome};
use harness_journal::TaskStore;
use harness_subagent::SubagentRunner;
use tokio::sync::mpsc;

use crate::{
    SubagentParentBinding, SubagentStopMode, SubagentSupervisor, WorkspaceCoordinator,
    WorkspaceCoordinatorError, WorkspaceToolAuthorization,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceToolAction {
    ReadPath(std::path::PathBuf),
    WritePath(std::path::PathBuf),
    Command { cwd: std::path::PathBuf },
}

impl WorkspaceToolAction {
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        match self {
            Self::ReadPath(path) | Self::WritePath(path) => path,
            Self::Command { cwd } => cwd,
        }
    }

    #[must_use]
    pub const fn requires_write(&self) -> bool {
        match self {
            Self::ReadPath(_) => false,
            Self::WritePath(_) => true,
            Self::Command { .. } => true,
        }
    }
}

#[derive(Clone)]
pub struct WorkspaceToolDispatcher {
    coordinator: Arc<WorkspaceCoordinator>,
}

impl WorkspaceToolDispatcher {
    #[must_use]
    pub fn new(coordinator: Arc<WorkspaceCoordinator>) -> Self {
        Self { coordinator }
    }

    pub async fn dispatch<T, F>(
        &self,
        lease_id: WorkspaceLeaseId,
        action: WorkspaceToolAction,
        execute: impl FnOnce(WorkspaceToolAuthorization) -> F,
    ) -> Result<T, WorkspaceCoordinatorError>
    where
        F: Future<Output = T>,
    {
        self.coordinator
            .dispatch_tool(lease_id, action, execute)
            .await
    }
}

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
    fn spawn_idempotent(
        &self,
        request: StartSegmentRequest,
        workspace_tools: WorkspaceToolDispatcher,
        subagent_runner: Arc<dyn SubagentRunner>,
    ) -> RunningSegment;
}

pub(crate) struct WorkspaceBoundRunCoordinatorFactory {
    inner: Arc<dyn RunCoordinatorFactory>,
    workspace_tools: WorkspaceToolDispatcher,
    store: Arc<TaskStore>,
    subagents: Arc<SubagentSupervisor>,
}

impl WorkspaceBoundRunCoordinatorFactory {
    #[must_use]
    pub(crate) fn new(
        inner: Arc<dyn RunCoordinatorFactory>,
        workspace_tools: WorkspaceToolDispatcher,
        store: Arc<TaskStore>,
        subagents: Arc<SubagentSupervisor>,
    ) -> Self {
        Self {
            inner,
            workspace_tools,
            store,
            subagents,
        }
    }

    pub(crate) fn spawn_idempotent(&self, request: StartSegmentRequest) -> RunningSegment {
        let parent_actor_id = self
            .store
            .task_projection(request.task_id)
            .ok()
            .flatten()
            .and_then(|projection| projection.actor_id)
            .unwrap_or_else(|| {
                harness_contracts::ActorId::from_u128(u128::from_be_bytes(
                    request.task_id.as_bytes(),
                ))
            });
        let subagent_runner = self.subagents.bind(SubagentParentBinding {
            parent_task_id: request.task_id,
            parent_segment_id: request.segment_id,
            parent_actor_id,
            depth: 0,
        });
        self.inner
            .spawn_idempotent(request, self.workspace_tools.clone(), subagent_runner)
    }

    pub(crate) fn request_parent_stop(
        &self,
        task_id: TaskId,
        mode: SubagentStopMode,
    ) -> Result<(), harness_subagent::SubagentError> {
        self.subagents.request_parent_stop(task_id, mode)
    }
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

use std::{future::Future, sync::Arc};

use chrono::{DateTime, Utc};
use harness_contracts::{
    IndeterminateToolDecision, RunSegmentId, RunTerminalReason, TaskId, ToolUseId, WorkspaceLeaseId,
};
use harness_engine::{RunControlHandle, TurnOutcome};
use harness_journal::{SegmentRunInput, TaskStore};
use harness_sandbox::LocalIsolation;
use harness_subagent::SubagentRunner;
use tokio::sync::mpsc;

use crate::{
    DaemonAgentStarter, SubagentParentBinding, SubagentStopMode, SubagentSupervisor,
    WorkspaceCoordinator, WorkspaceCoordinatorError, WorkspaceToolAuthorization,
};

#[derive(Clone)]
pub struct AgentStarterCapabilities {
    pub background: Arc<dyn harness_contracts::BackgroundAgentStarterCap>,
    pub team: Arc<dyn harness_contracts::AgentTeamStarterCap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceToolAction {
    ReadPath(std::path::PathBuf),
    WritePath(std::path::PathBuf),
    Command {
        cwd: std::path::PathBuf,
        requires_write: bool,
    },
}

impl WorkspaceToolAction {
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        match self {
            Self::ReadPath(path) | Self::WritePath(path) => path,
            Self::Command { cwd, .. } => cwd,
        }
    }

    #[must_use]
    pub const fn requires_write(&self) -> bool {
        match self {
            Self::ReadPath(_) => false,
            Self::WritePath(_) => true,
            Self::Command { requires_write, .. } => *requires_write,
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

    pub async fn dispatch_sandboxed_command<T, F>(
        &self,
        lease_id: WorkspaceLeaseId,
        cwd: std::path::PathBuf,
        requires_write: bool,
        isolation: LocalIsolation,
        execute: impl FnOnce(WorkspaceToolAuthorization) -> F,
    ) -> Result<T, WorkspaceCoordinatorError>
    where
        F: Future<Output = T>,
    {
        self.coordinator
            .dispatch_sandboxed_command(lease_id, cwd, requires_write, isolation, execute)
            .await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartSegmentRequest {
    pub task_id: TaskId,
    pub segment_id: RunSegmentId,
    pub input: SegmentRunInput,
    pub indeterminate_tools: Vec<IndeterminateToolDecision>,
}

impl StartSegmentRequest {
    #[must_use]
    pub fn skill_context_delivery_key(&self, reference_index: usize) -> Option<String> {
        let queue_item_id = self.input.queue_item_id?;
        let queue_item_revision = self.input.queue_item_revision?;
        let encoded_reference_index = u64::try_from(reference_index).ok()?;
        if !matches!(
            self.input.context_references.get(reference_index),
            Some(harness_contracts::ConversationContextReference::Skill { .. })
        ) {
            return None;
        }

        let mut hasher = blake3::Hasher::new();
        hasher.update(b"jyowo.skill-context-delivery.v1\0");
        hasher.update(&self.task_id.as_bytes());
        hasher.update(&queue_item_id.as_bytes());
        hasher.update(&queue_item_revision.to_be_bytes());
        hasher.update(&encoded_reference_index.to_be_bytes());
        Some(format!("skill-context-v1:{}", hasher.finalize().to_hex()))
    }
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
        agent_starters: AgentStarterCapabilities,
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
        let binding = SubagentParentBinding {
            parent_task_id: request.task_id,
            parent_segment_id: request.segment_id,
            parent_actor_id,
            depth: 0,
        };
        let subagent_runner = self.subagents.bind(binding);
        let blob_root = self
            .store
            .database_path()
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("blobs");
        let starter = Arc::new(DaemonAgentStarter::new(
            Arc::clone(&self.store),
            Arc::clone(&self.subagents),
            binding,
            blob_root,
        ));
        let agent_starters = AgentStarterCapabilities {
            background: Arc::clone(&starter)
                as Arc<dyn harness_contracts::BackgroundAgentStarterCap>,
            team: starter as Arc<dyn harness_contracts::AgentTeamStarterCap>,
        };
        self.inner.spawn_idempotent(
            request,
            self.workspace_tools.clone(),
            subagent_runner,
            agent_starters,
        )
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
    use std::collections::BTreeMap;

    use harness_contracts::{
        ConversationContextReference, PermissionMode, QueueItemId, RunId, SessionId, SkillId,
        SkillSourceKind, CURRENT_CONTEXT_REFERENCE_VERSION,
    };

    use super::*;

    #[test]
    fn skill_context_delivery_key_is_stable_and_revision_scoped() {
        let task_id = TaskId::new();
        let queue_item_id = QueueItemId::new();
        let skill = || ConversationContextReference::Skill {
            version: CURRENT_CONTEXT_REFERENCE_VERSION,
            skill_id: SkillId("user:review".into()),
            label: "Review".into(),
            parameters: BTreeMap::new(),
            source: Some(SkillSourceKind::User),
        };
        let request = StartSegmentRequest {
            task_id,
            segment_id: RunSegmentId::new(),
            input: SegmentRunInput {
                queue_item_id: Some(queue_item_id),
                queue_item_revision: Some(2),
                content: "review".into(),
                attachments: Vec::new(),
                context_references: vec![skill(), skill()],
                model_config_id: None,
                permission_mode: PermissionMode::Default,
                workspace: None,
                session_id: SessionId::new(),
                run_id: RunId::new(),
                workspace_lease_id: None,
            },
            indeterminate_tools: Vec::new(),
        };

        let first = request.skill_context_delivery_key(0).unwrap();
        let mut resumed = request.clone();
        resumed.segment_id = RunSegmentId::new();
        assert_eq!(resumed.skill_context_delivery_key(0).unwrap(), first);

        let mut edited = request.clone();
        edited.input.queue_item_revision = Some(3);
        assert_ne!(edited.skill_context_delivery_key(0).unwrap(), first);
        assert_ne!(request.skill_context_delivery_key(1).unwrap(), first);
        assert_eq!(request.skill_context_delivery_key(2), None);
    }

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

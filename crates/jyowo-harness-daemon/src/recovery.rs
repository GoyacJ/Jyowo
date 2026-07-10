use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;
use harness_contracts::{
    CommandId, Event, RunSegmentId, RunState, RunTerminalReason, TaskId, TaskProjection, ToolUseId,
};
use harness_journal::{AcceptedCommand, CommandOutcome, NewTaskEvent, TaskStore, TaskStoreError};
use serde_json::json;

use crate::{CheckpointService, CheckpointState};

const RECOVERY_PAGE_SIZE: usize = 16;

pub struct RecoveryService {
    store: Arc<TaskStore>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    pub recovered_tasks: Vec<RecoveredTask>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredTask {
    pub task_id: TaskId,
    pub interrupted_segment_id: RunSegmentId,
    pub indeterminate_tool_use_ids: Vec<ToolUseId>,
}

impl RecoveryService {
    #[must_use]
    pub fn new(store: Arc<TaskStore>) -> Self {
        Self { store }
    }

    pub fn recover_startup(&self) -> Result<RecoveryReport, TaskStoreError> {
        let mut recovered_tasks = Vec::new();
        let mut after_task_id = None;
        loop {
            let projections = self
                .store
                .nonterminal_task_projections_after(after_task_id, RECOVERY_PAGE_SIZE)?;
            let Some(last_task_id) = projections.last().map(|projection| projection.task_id) else {
                break;
            };
            after_task_id = Some(last_task_id);
            for projection in projections {
                if let Some(recovered) = self.recover_projection(projection)? {
                    recovered_tasks.push(recovered);
                }
            }
        }
        Ok(RecoveryReport { recovered_tasks })
    }

    pub fn recover_task(&self, task_id: TaskId) -> Result<RecoveryReport, TaskStoreError> {
        let recovered_tasks = self
            .store
            .task_projection(task_id)?
            .map(|projection| self.recover_projection(projection))
            .transpose()?
            .into_iter()
            .flatten()
            .collect();
        Ok(RecoveryReport { recovered_tasks })
    }

    fn recover_projection(
        &self,
        projection: TaskProjection,
    ) -> Result<Option<RecoveredTask>, TaskStoreError> {
        let Some(run) = projection.current_run.as_ref() else {
            return Ok(None);
        };
        let segment_id = run.segment_id;
        if matches!(
            run.state,
            RunState::Running | RunState::WaitingPermission | RunState::Yielding
        ) && self
            .store
            .pending_segment_start(projection.task_id, segment_id)?
            .is_some()
        {
            return Ok(None);
        }
        if !matches!(
            run.state,
            RunState::Running | RunState::WaitingPermission | RunState::Yielding
        ) {
            if run.terminal_reason == Some(RunTerminalReason::InterruptedByRestart)
                && !self
                    .store
                    .latest_checkpoint(projection.task_id)?
                    .is_some_and(|checkpoint| {
                        checkpoint.run_segment_id == segment_id
                            && checkpoint.committed_global_offset == projection.last_global_offset
                    })
            {
                let indeterminate_tool_use_ids =
                    self.indeterminate_tools(projection.task_id, segment_id)?;
                self.persist_recovery_checkpoint(
                    projection.task_id,
                    segment_id,
                    indeterminate_tool_use_ids,
                )?;
            }
            return Ok(None);
        }
        let recovered_at = Utc::now();
        let indeterminate_tool_use_ids =
            self.indeterminate_tools(projection.task_id, segment_id)?;
        let mut events = Vec::new();
        if let Some(permission) = &projection.pending_permission {
            events.push(NewTaskEvent::permission_invalidated(
                permission.request_id,
                permission.revision,
                "expired_by_restart",
            ));
        }
        events.extend(
            indeterminate_tool_use_ids
                .iter()
                .copied()
                .map(|tool_use_id| {
                    NewTaskEvent::tool_indeterminate(segment_id, tool_use_id, recovered_at)
                }),
        );
        if run.state == RunState::Yielding {
            events.extend(
                projection
                    .queue
                    .iter()
                    .filter(|item| item.state == harness_contracts::QueueItemState::Promoting)
                    .map(|item| NewTaskEvent::message_recovered(item.queue_item_id, item.revision)),
            );
        }
        events.push(NewTaskEvent::run_completed(
            segment_id,
            recovered_at,
            RunTerminalReason::InterruptedByRestart,
            true,
        ));
        let command = AcceptedCommand {
            command_id: CommandId::new(),
            task_id: projection.task_id,
            idempotency_key: format!("startup-recovery-{}-{segment_id}", projection.task_id),
            expected_stream_version: projection.stream_version,
            authority: TaskStore::recovery_authority(),
            payload: json!({
                "type": "startup_recovery",
                "segmentId": segment_id,
            }),
        };
        let outcome = self.store.transact_command(command, |_| Ok(events))?;
        if !matches!(outcome, CommandOutcome::Accepted { .. }) {
            return Ok(None);
        }
        self.persist_recovery_checkpoint(
            projection.task_id,
            segment_id,
            indeterminate_tool_use_ids.clone(),
        )?;
        Ok(Some(RecoveredTask {
            task_id: projection.task_id,
            interrupted_segment_id: segment_id,
            indeterminate_tool_use_ids,
        }))
    }

    fn persist_recovery_checkpoint(
        &self,
        task_id: TaskId,
        segment_id: RunSegmentId,
        incomplete_tool_use_ids: Vec<ToolUseId>,
    ) -> Result<(), TaskStoreError> {
        let prior = self.store.latest_checkpoint(task_id)?;
        CheckpointService::new(Arc::clone(&self.store)).persist(
            task_id,
            segment_id,
            CheckpointState {
                context_cursor: prior.as_ref().map_or(0, |value| value.context_cursor),
                workspace_baseline: prior
                    .as_ref()
                    .and_then(|value| value.workspace_baseline.clone()),
                incomplete_tool_use_ids,
                child_actor_refs: prior
                    .as_ref()
                    .map_or_else(Vec::new, |value| value.child_actor_refs.clone()),
                context_blob_id: prior.as_ref().and_then(|value| value.context_blob_id),
            },
        )?;
        Ok(())
    }

    fn indeterminate_tools(
        &self,
        task_id: TaskId,
        segment_id: RunSegmentId,
    ) -> Result<Vec<ToolUseId>, TaskStoreError> {
        let checkpoint = self
            .store
            .latest_checkpoint(task_id)?
            .filter(|checkpoint| checkpoint.run_segment_id == segment_id);
        let (mut after_global_offset, mut started) = if let Some(checkpoint) = checkpoint {
            (
                checkpoint.committed_global_offset,
                checkpoint
                    .incomplete_tool_use_ids
                    .into_iter()
                    .collect::<HashSet<_>>(),
            )
        } else {
            let run_started_global_offset = self
                .store
                .run_started_global_offset(task_id, segment_id)?
                .ok_or_else(|| {
                    TaskStoreError::ProjectionIntegrity(format!(
                        "active run segment {segment_id} has no run.started event"
                    ))
                })?;
            (run_started_global_offset, HashSet::new())
        };
        loop {
            let events = self.store.task_events_after_global_offset(
                task_id,
                after_global_offset,
                RECOVERY_PAGE_SIZE,
            )?;
            if events.is_empty() {
                break;
            }
            for envelope in events {
                after_global_offset = envelope.global_offset;
                if !envelope.event_type.starts_with("engine.tool_use_") {
                    continue;
                }
                let event = envelope.payload.get("event").cloned().ok_or_else(|| {
                    TaskStoreError::ProjectionIntegrity(
                        "engine tool event payload is missing event".into(),
                    )
                })?;
                match serde_json::from_value::<Event>(event)? {
                    Event::ToolUseStarted(event) => {
                        started.insert(event.tool_use_id);
                    }
                    Event::ToolUseCompleted(event) => {
                        started.remove(&event.tool_use_id);
                    }
                    Event::ToolUseFailed(event) => {
                        started.remove(&event.tool_use_id);
                    }
                    Event::ToolUseDenied(event) => {
                        started.remove(&event.tool_use_id);
                    }
                    _ => {}
                }
            }
        }
        let mut indeterminate_tool_use_ids = started.into_iter().collect::<Vec<_>>();
        indeterminate_tool_use_ids.sort_by_key(ToString::to_string);
        Ok(indeterminate_tool_use_ids)
    }
}

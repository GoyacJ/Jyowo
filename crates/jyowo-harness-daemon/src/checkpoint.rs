use std::sync::Arc;

use chrono::Utc;
use harness_contracts::{ActorId, BlobId, CheckpointId, RunSegmentId, TaskId, ToolUseId};
use harness_journal::{ContextSummary, TaskCheckpoint, TaskStore, TaskStoreError};

pub use harness_journal::WorkspaceBaseline;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckpointState {
    pub context_cursor: u64,
    pub workspace_baseline: Option<WorkspaceBaseline>,
    pub incomplete_tool_use_ids: Vec<ToolUseId>,
    pub child_actor_refs: Vec<ActorId>,
    pub context_blob_id: Option<BlobId>,
}

pub struct CheckpointService {
    store: Arc<TaskStore>,
}

impl CheckpointService {
    #[must_use]
    pub fn new(store: Arc<TaskStore>) -> Self {
        Self { store }
    }

    pub fn persist(
        &self,
        task_id: TaskId,
        run_segment_id: RunSegmentId,
        state: CheckpointState,
    ) -> Result<TaskCheckpoint, TaskStoreError> {
        Self::persist_current(&self.store, task_id, run_segment_id, state)
    }

    pub fn persist_current(
        store: &TaskStore,
        task_id: TaskId,
        run_segment_id: RunSegmentId,
        state: CheckpointState,
    ) -> Result<TaskCheckpoint, TaskStoreError> {
        let projection = store
            .task_projection(task_id)?
            .ok_or_else(|| TaskStoreError::InvalidInput("checkpoint task does not exist".into()))?;
        if projection.current_run.as_ref().map(|run| run.segment_id) != Some(run_segment_id) {
            return Err(TaskStoreError::InvalidInput(
                "checkpoint segment is not the task's current segment".into(),
            ));
        }
        let checkpoint = TaskCheckpoint {
            checkpoint_id: CheckpointId::new(),
            task_id,
            run_segment_id,
            committed_global_offset: projection.last_global_offset,
            context_cursor: state.context_cursor,
            queue_revision: store.latest_queue_revision(task_id)?,
            workspace_baseline: state.workspace_baseline,
            incomplete_tool_use_ids: state.incomplete_tool_use_ids,
            child_actor_refs: state.child_actor_refs,
            context_blob_id: state.context_blob_id,
            created_at: Utc::now(),
        };
        store.save_checkpoint(&checkpoint)?;
        Ok(checkpoint)
    }
}

pub struct ContextCompactionService {
    store: Arc<TaskStore>,
}

impl ContextCompactionService {
    #[must_use]
    pub fn new(store: Arc<TaskStore>) -> Self {
        Self { store }
    }

    pub fn activate(
        &self,
        task_id: TaskId,
        source_start_global_offset: u64,
        source_end_global_offset: u64,
        blob_id: BlobId,
    ) -> Result<ContextSummary, TaskStoreError> {
        let summary = ContextSummary {
            summary_id: CheckpointId::new(),
            task_id,
            source_start_global_offset,
            source_end_global_offset,
            blob_id,
            created_at: Utc::now(),
        };
        self.store.activate_context_summary(&summary)?;
        Ok(summary)
    }
}

//! Durable daemon ownership for delegated subagent runs.

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use harness_contracts::{
    now, ActorId, RedactRules, Redactor, RunSegmentId, SessionId, SubagentActorState, SubagentId,
    SubagentProjection, SubagentStatus, TaskId, TenantId, WorkspaceMode,
};
use harness_journal::{
    CreateSubagentActorRequest, EventStore, ExpectedParentStopSubagent, ParentSubagentStopMode,
    RedactedSubagentSummary, SubagentLifecycleAuthority, SubagentLifecycleCommand,
    SubagentLifecycleTransition, TaskEventStoreAdapter, TaskStore, TaskWorkspaceLease,
};
use harness_subagent::{
    ParentContext, SubagentCancellationToken, SubagentError, SubagentHandle, SubagentRunner,
    SubagentSpec,
};
use tokio::sync::{oneshot, OwnedSemaphorePermit, Semaphore};
use tokio::task::AbortHandle;

use crate::{
    WorkspaceAccess, WorkspaceAcquireOutcome, WorkspaceCoordinator, WorkspaceExecutionKind,
    WorkspaceLeaseRequest,
};

const MAX_PARENT_SUMMARY_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubagentParentBinding {
    pub parent_task_id: TaskId,
    pub parent_segment_id: RunSegmentId,
    pub parent_actor_id: ActorId,
    pub depth: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentStopMode {
    SafePoint,
    Force,
}

pub struct WorkspaceSubagentRunContext {
    pub workspace_root: PathBuf,
    pub child_task_id: TaskId,
    pub actor_id: ActorId,
    pub parent_segment_id: RunSegmentId,
    pub segment_id: RunSegmentId,
    pub workspace_lease_id: harness_contracts::WorkspaceLeaseId,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub event_store: Arc<dyn EventStore>,
    pub subagent_runner: Arc<dyn SubagentRunner>,
}

pub trait WorkspaceSubagentRunnerFactory: Send + Sync + 'static {
    fn create(
        &self,
        context: WorkspaceSubagentRunContext,
    ) -> Result<Arc<dyn SubagentRunner>, SubagentError>;
}

impl<F> WorkspaceSubagentRunnerFactory for F
where
    F: Fn(WorkspaceSubagentRunContext) -> Result<Arc<dyn SubagentRunner>, SubagentError>
        + Send
        + Sync
        + 'static,
{
    fn create(
        &self,
        context: WorkspaceSubagentRunContext,
    ) -> Result<Arc<dyn SubagentRunner>, SubagentError> {
        self(context)
    }
}

#[derive(Clone)]
struct ActiveChild {
    projection: SubagentProjection,
    abort: AbortHandle,
    control: SubagentCancellationToken,
}

struct SpawnCallerGuard {
    supervisor: Arc<SubagentSupervisor>,
    child_task_id: TaskId,
    armed: bool,
}

struct AcquiredWorkspaceGuard {
    workspace: Arc<WorkspaceCoordinator>,
    lease_id: harness_contracts::WorkspaceLeaseId,
    armed: bool,
}

struct StartedChild {
    child_task_id: TaskId,
    actor_id: ActorId,
    session_id: SessionId,
    start_tx: oneshot::Sender<()>,
    finished_rx: oneshot::Receiver<Result<SubagentHandle, SubagentError>>,
    caller_guard: SpawnCallerGuard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DetachedChild {
    pub child_task_id: TaskId,
    pub actor_id: ActorId,
    pub session_id: SessionId,
}

impl AcquiredWorkspaceGuard {
    fn new(
        workspace: Arc<WorkspaceCoordinator>,
        lease_id: harness_contracts::WorkspaceLeaseId,
    ) -> Self {
        Self {
            workspace,
            lease_id,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for AcquiredWorkspaceGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.workspace.release(self.lease_id);
        }
    }
}

impl SpawnCallerGuard {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for SpawnCallerGuard {
    fn drop(&mut self) {
        if self.armed {
            self.supervisor.cancel_attached_child(self.child_task_id);
        }
    }
}

pub struct SubagentSupervisor {
    store: Arc<TaskStore>,
    workspace: Arc<WorkspaceCoordinator>,
    runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory>,
    redactor: Arc<dyn Redactor>,
    max_depth: u8,
    quota: Arc<Semaphore>,
    active: Mutex<HashMap<TaskId, ActiveChild>>,
}

impl SubagentSupervisor {
    #[must_use]
    pub fn new(
        store: Arc<TaskStore>,
        workspace: Arc<WorkspaceCoordinator>,
        runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory>,
        redactor: Arc<dyn Redactor>,
        max_depth: u8,
        max_global: usize,
    ) -> Self {
        Self {
            store,
            workspace,
            runner_factory,
            redactor,
            max_depth,
            quota: Arc::new(Semaphore::new(max_global)),
            active: Mutex::new(HashMap::new()),
        }
    }

    #[must_use]
    pub fn bind(self: &Arc<Self>, binding: SubagentParentBinding) -> Arc<dyn SubagentRunner> {
        Arc::new(BoundSubagentRunner {
            supervisor: Arc::clone(self),
            binding,
        })
    }

    pub fn request_parent_stop(
        &self,
        parent_task_id: TaskId,
        mode: SubagentStopMode,
    ) -> Result<(), SubagentError> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| SubagentError::Engine("subagent registry lock poisoned".into()))?;
        let parent = self
            .store
            .task_projection(parent_task_id)
            .map_err(subagent_store_error)?
            .ok_or_else(|| SubagentError::Engine("subagent parent task is missing".into()))?;
        let expected = parent
            .subagents
            .iter()
            .filter(|child| {
                !child.detached
                    && match mode {
                        SubagentStopMode::SafePoint => matches!(
                            child.state,
                            SubagentActorState::Starting | SubagentActorState::Running
                        ),
                        SubagentStopMode::Force => is_nonterminal_child_state(child.state),
                    }
            })
            .map(|child| ExpectedParentStopSubagent {
                child_task_id: child.child_task_id,
                actor_id: child.actor_id,
                expected_state: child.state,
            })
            .collect::<Vec<_>>();
        let updated = self
            .store
            .apply_parent_subagent_stop(
                parent_task_id,
                &expected,
                match mode {
                    SubagentStopMode::SafePoint => ParentSubagentStopMode::SafePoint,
                    SubagentStopMode::Force => ParentSubagentStopMode::Force { ended_at: now() },
                },
            )
            .map_err(subagent_store_error)?;
        let mut children = Vec::new();
        for projected in updated {
            let child_task_id = projected.child_task_id;
            if let Some(child) = active.get_mut(&child_task_id) {
                child.projection = projected;
                children.push(child.clone());
            }
        }
        drop(active);
        for child in children {
            match mode {
                SubagentStopMode::SafePoint => child.control.request_yield(),
                SubagentStopMode::Force => {
                    child.control.cancel();
                    child.abort.abort();
                }
            }
        }
        Ok(())
    }

    pub fn continue_in_background(
        &self,
        parent_task_id: TaskId,
        child_task_id: TaskId,
    ) -> Result<(), SubagentError> {
        let child = self
            .active
            .lock()
            .map_err(|_| SubagentError::Engine("subagent registry lock poisoned".into()))?
            .get(&child_task_id)
            .filter(|child| child.projection.parent_task_id == parent_task_id)
            .cloned()
            .ok_or_else(|| SubagentError::Engine("active child is not linked to parent".into()))?;
        let projected = self.apply_lifecycle(
            &child.projection,
            SubagentLifecycleAuthority::Supervisor,
            SubagentLifecycleTransition::Background,
        )?;
        self.active
            .lock()
            .map_err(|_| SubagentError::Engine("subagent registry lock poisoned".into()))?
            .get_mut(&child_task_id)
            .filter(|child| child.projection.parent_task_id == parent_task_id)
            .ok_or_else(|| SubagentError::Engine("active child is no longer running".into()))?
            .projection = projected;
        Ok(())
    }

    pub async fn reserve_permit(&self) -> Result<OwnedSemaphorePermit, SubagentError> {
        Arc::clone(&self.quota)
            .acquire_owned()
            .await
            .map_err(|_| SubagentError::ConcurrentLimitExceeded)
    }

    pub async fn start_detached(
        self: &Arc<Self>,
        binding: SubagentParentBinding,
        spec: SubagentSpec,
        input: harness_contracts::TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<TaskId, SubagentError> {
        self.start_detached_child(binding, spec, input, parent_ctx)
            .await
            .map(|child| child.child_task_id)
    }

    pub(crate) async fn start_detached_child(
        self: &Arc<Self>,
        binding: SubagentParentBinding,
        spec: SubagentSpec,
        input: harness_contracts::TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<DetachedChild, SubagentError> {
        let StartedChild {
            child_task_id,
            actor_id,
            session_id,
            start_tx,
            finished_rx,
            mut caller_guard,
        } = self.start_bound(binding, spec, input, parent_ctx).await?;
        if let Err(detach_error) =
            self.continue_in_background(binding.parent_task_id, child_task_id)
        {
            drop(start_tx);
            self.cancel_attached_child(child_task_id);
            let cleanup_result = finished_rx.await;
            caller_guard.disarm();
            return match cleanup_result {
                Ok(_) => Err(detach_error),
                Err(_) => Err(SubagentError::Engine(format!(
                    "{detach_error}; subagent finalizer stopped during detach cleanup"
                ))),
            };
        }
        caller_guard.disarm();
        start_tx
            .send(())
            .map_err(|_| SubagentError::Engine("detached subagent failed to start".into()))?;
        Ok(DetachedChild {
            child_task_id,
            actor_id,
            session_id,
        })
    }

    pub(crate) fn cancel_child(
        &self,
        parent_task_id: TaskId,
        child_task_id: TaskId,
    ) -> Result<(), SubagentError> {
        let child = self
            .active
            .lock()
            .map_err(|_| SubagentError::Engine("subagent registry lock poisoned".into()))?
            .get(&child_task_id)
            .filter(|child| child.projection.parent_task_id == parent_task_id)
            .cloned()
            .ok_or_else(|| SubagentError::Engine("active child is not linked to parent".into()))?;
        let projected = self.apply_lifecycle(
            &child.projection,
            SubagentLifecycleAuthority::Supervisor,
            SubagentLifecycleTransition::Cancelled { ended_at: now() },
        )?;
        if let Ok(mut active) = self.active.lock() {
            if let Some(active_child) = active.get_mut(&child_task_id) {
                active_child.projection = projected;
            }
        }
        child.control.cancel();
        child.abort.abort();
        Ok(())
    }

    async fn spawn_bound(
        self: &Arc<Self>,
        binding: SubagentParentBinding,
        spec: SubagentSpec,
        input: harness_contracts::TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, SubagentError> {
        let StartedChild {
            start_tx,
            finished_rx,
            mut caller_guard,
            ..
        } = self.start_bound(binding, spec, input, parent_ctx).await?;
        let _ = start_tx.send(());
        let finished = finished_rx.await.map_err(|_| {
            SubagentError::Engine("subagent finalizer stopped before returning a result".into())
        })?;
        caller_guard.disarm();
        finished
    }

    async fn start_bound(
        self: &Arc<Self>,
        binding: SubagentParentBinding,
        spec: SubagentSpec,
        input: harness_contracts::TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<StartedChild, SubagentError> {
        let child_depth = binding.depth.max(parent_ctx.depth).saturating_add(1);
        if child_depth > self.max_depth {
            return Err(SubagentError::DepthExceeded {
                current: child_depth,
                max: self.max_depth,
            });
        }
        let permit = Arc::clone(&self.quota)
            .try_acquire_owned()
            .map_err(|_| SubagentError::ConcurrentLimitExceeded)?;
        let parent_lease =
            parent_workspace_lease(&self.store, binding.parent_task_id, binding.parent_actor_id)?;
        let root = PathBuf::from(
            parent_lease
                .worktree_path
                .as_deref()
                .unwrap_or(&parent_lease.canonical_root),
        );
        let child_task_id = TaskId::new();
        let actor_id = ActorId::new();
        let segment_id = RunSegmentId::new();
        let started_at = now();
        let mut child = SubagentProjection {
            child_task_id,
            actor_id,
            segment_id,
            parent_task_id: binding.parent_task_id,
            parent_segment_id: binding.parent_segment_id,
            delegation_id: SubagentId::new(),
            context_cursor: 0,
            workspace_lease_id: None,
            state: SubagentActorState::Starting,
            detached: false,
            summary: None,
            started_at,
            ended_at: None,
        };
        self.store
            .create_subagent_actor_checked(CreateSubagentActorRequest {
                child_task_id,
                actor_id,
                segment_id,
                parent_task_id: binding.parent_task_id,
                parent_segment_id: binding.parent_segment_id,
                parent_actor_id: binding.parent_actor_id,
                parent_workspace_lease_id: parent_lease.lease_id,
                delegation_id: child.delegation_id,
                context_cursor: child.context_cursor,
                title: spec.role.clone(),
                started_at,
            })
            .map_err(subagent_store_error)?;
        let lease = match self.workspace.acquire(WorkspaceLeaseRequest {
            task_id: child_task_id,
            actor_id,
            root,
            mode: Some(WorkspaceMode::ManagedWorktree),
            access: WorkspaceAccess::Write,
            execution_kind: WorkspaceExecutionKind::ParallelChild,
            expires_at: None,
        }) {
            Ok(WorkspaceAcquireOutcome::Acquired(lease)) => lease,
            Ok(WorkspaceAcquireOutcome::Waiting(_)) => {
                child.state = SubagentActorState::Failed;
                child.ended_at = Some(now());
                self.apply_lifecycle(
                    &child,
                    SubagentLifecycleAuthority::Actor(actor_id),
                    SubagentLifecycleTransition::Failed {
                        ended_at: child.ended_at.expect("failed child has an end time"),
                    },
                )?;
                return Err(SubagentError::Engine(
                    "managed subagent workspace unexpectedly waited".into(),
                ));
            }
            Err(error) => {
                child.state = SubagentActorState::Failed;
                child.ended_at = Some(now());
                let _ = self.apply_lifecycle(
                    &child,
                    SubagentLifecycleAuthority::Actor(actor_id),
                    SubagentLifecycleTransition::Failed {
                        ended_at: child.ended_at.expect("failed child has an end time"),
                    },
                );
                return Err(SubagentError::Engine(error.to_string()));
            }
        };
        let mut workspace_guard =
            AcquiredWorkspaceGuard::new(Arc::clone(&self.workspace), lease.lease_id);
        child.workspace_lease_id = Some(lease.lease_id);
        child.state = SubagentActorState::Running;
        child = match self.apply_lifecycle(
            &child,
            SubagentLifecycleAuthority::Actor(actor_id),
            SubagentLifecycleTransition::Running {
                workspace_lease_id: lease.lease_id,
                context_cursor: child.context_cursor,
            },
        ) {
            Ok(child) => child,
            Err(running_error) => {
                let terminal = self.apply_lifecycle(
                    &child,
                    SubagentLifecycleAuthority::Actor(actor_id),
                    SubagentLifecycleTransition::Failed { ended_at: now() },
                );
                return match terminal {
                    Ok(_) => Err(running_error),
                    Err(terminal_error) => Err(SubagentError::Engine(format!(
                        "{running_error}; failed to record terminal child state: {terminal_error}"
                    ))),
                };
            }
        };

        let execution_root = lease
            .worktree_path
            .as_deref()
            .map(Path::new)
            .ok_or_else(|| {
                SubagentError::Engine("managed child lease has no worktree path".into())
            })?;
        let child_session_id = SessionId::new();
        let child_event_store: Arc<dyn EventStore> = Arc::new(TaskEventStoreAdapter::new(
            Arc::clone(&self.store),
            child_task_id,
            parent_ctx.tenant_id,
            child_session_id,
            Arc::clone(&self.redactor),
        ));
        let delegate_result = catch_unwind(AssertUnwindSafe(|| {
            self.runner_factory.create(WorkspaceSubagentRunContext {
                workspace_root: execution_root.to_path_buf(),
                child_task_id,
                actor_id,
                parent_segment_id: binding.parent_segment_id,
                segment_id,
                workspace_lease_id: lease.lease_id,
                tenant_id: parent_ctx.tenant_id,
                session_id: child_session_id,
                event_store: child_event_store,
                subagent_runner: self.bind(SubagentParentBinding {
                    parent_task_id: child_task_id,
                    parent_segment_id: segment_id,
                    parent_actor_id: actor_id,
                    depth: child_depth,
                }),
            })
        }))
        .unwrap_or_else(|_| {
            Err(SubagentError::Engine(
                "subagent runner factory panicked".into(),
            ))
        });
        let delegate = match delegate_result {
            Ok(delegate) => delegate,
            Err(error) => {
                child.state = SubagentActorState::Failed;
                child.ended_at = Some(now());
                self.apply_lifecycle(
                    &child,
                    SubagentLifecycleAuthority::Actor(actor_id),
                    SubagentLifecycleTransition::Failed {
                        ended_at: child.ended_at.expect("failed child has an end time"),
                    },
                )?;
                let _ = self.workspace.release(lease.lease_id);
                return Err(error);
            }
        };

        let control = SubagentCancellationToken::new();
        let execution_control = control.clone();
        let (start_tx, start_rx) = oneshot::channel();
        let execution = tokio::spawn(async move {
            let _permit = permit;
            if start_rx.await.is_err() {
                return Err(SubagentError::Cancelled);
            }
            delegate
                .spawn_controlled(spec, input, parent_ctx, execution_control)
                .await
        });
        let abort = execution.abort_handle();
        let should_abort = {
            let mut active = self
                .active
                .lock()
                .map_err(|_| SubagentError::Engine("subagent registry lock poisoned".into()))?;
            child = self.durable_child(&child)?;
            if is_nonterminal_child_state(child.state) {
                if child.state == SubagentActorState::Yielding {
                    control.request_yield();
                }
                active.insert(
                    child_task_id,
                    ActiveChild {
                        projection: child.clone(),
                        abort,
                        control,
                    },
                );
                false
            } else {
                true
            }
        };
        if should_abort {
            execution.abort();
        }
        let (finished_tx, finished_rx) = oneshot::channel();
        let supervisor = Arc::clone(self);
        tokio::spawn(async move {
            let result = execution.await;
            let active_child = supervisor
                .active
                .lock()
                .ok()
                .and_then(|active| active.get(&child_task_id).cloned())
                .map(|child| child.projection)
                .unwrap_or(child);
            let finished = supervisor.finish_child(active_child, result).await;
            if let Ok(mut active) = supervisor.active.lock() {
                active.remove(&child_task_id);
            }
            let _ = finished_tx.send(finished);
        });
        workspace_guard.disarm();
        let caller_guard = SpawnCallerGuard {
            supervisor: Arc::clone(self),
            child_task_id,
            armed: true,
        };
        Ok(StartedChild {
            child_task_id,
            actor_id,
            session_id: child_session_id,
            start_tx,
            finished_rx,
            caller_guard,
        })
    }

    async fn finish_child(
        &self,
        child: SubagentProjection,
        result: Result<Result<SubagentHandle, SubagentError>, tokio::task::JoinError>,
    ) -> Result<SubagentHandle, SubagentError> {
        let outcome = match result {
            Ok(Ok(handle)) => handle.wait().await,
            Ok(Err(error)) => Err(error),
            Err(join_error) if join_error.is_cancelled() => Err(SubagentError::Cancelled),
            Err(join_error) => Err(SubagentError::Engine(format!(
                "subagent actor crashed: {join_error}"
            ))),
        };
        let lifecycle_result = (|| {
            let durable = self.durable_child(&child)?;
            match outcome {
                Ok(mut announcement) => {
                    if durable.state == SubagentActorState::Cancelled {
                        return Err(SubagentError::Cancelled);
                    }
                    if durable.state == SubagentActorState::Failed {
                        return Err(SubagentError::Engine(
                            "subagent was already recorded as failed".into(),
                        ));
                    }
                    let summary =
                        bounded_redacted_summary(self.redactor.as_ref(), &announcement.summary);
                    announcement.subagent_id = durable.delegation_id;
                    announcement.summary.clone_from(&summary);
                    announcement.result = None;
                    announcement.transcript_ref = None;
                    announcement.context_report = None;
                    if !matches!(
                        durable.state,
                        SubagentActorState::Completed
                            | SubagentActorState::Cancelled
                            | SubagentActorState::Failed
                    ) {
                        let transition = match &announcement.status {
                            SubagentStatus::Completed
                            | SubagentStatus::MaxIterationsReached
                            | SubagentStatus::MaxBudget(_) => {
                                SubagentLifecycleTransition::Completed {
                                    summary: RedactedSubagentSummary::new(
                                        self.redactor.as_ref(),
                                        &summary,
                                    ),
                                    ended_at: now(),
                                }
                            }
                            SubagentStatus::Cancelled => {
                                SubagentLifecycleTransition::Cancelled { ended_at: now() }
                            }
                            SubagentStatus::Failed | SubagentStatus::Stalled => {
                                SubagentLifecycleTransition::Failed { ended_at: now() }
                            }
                            _ => SubagentLifecycleTransition::Failed { ended_at: now() },
                        };
                        self.apply_lifecycle(
                            &durable,
                            SubagentLifecycleAuthority::Actor(durable.actor_id),
                            transition,
                        )?;
                    }
                    Ok(SubagentHandle::ready(announcement))
                }
                Err(error) => {
                    if !matches!(
                        durable.state,
                        SubagentActorState::Completed
                            | SubagentActorState::Cancelled
                            | SubagentActorState::Failed
                    ) {
                        self.apply_lifecycle(
                            &durable,
                            SubagentLifecycleAuthority::Actor(durable.actor_id),
                            if matches!(error, SubagentError::Cancelled) {
                                SubagentLifecycleTransition::Cancelled { ended_at: now() }
                            } else {
                                SubagentLifecycleTransition::Failed { ended_at: now() }
                            },
                        )?;
                    }
                    Err(error)
                }
            }
        })();
        let release_result: Result<Option<()>, SubagentError> =
            self.durable_child(&child).and_then(|durable| {
                if matches!(
                    durable.state,
                    SubagentActorState::Completed
                        | SubagentActorState::Cancelled
                        | SubagentActorState::Failed
                ) {
                    child
                        .workspace_lease_id
                        .map(|lease_id| {
                            self.workspace
                                .release(lease_id)
                                .map(|_| ())
                                .map_err(|error| SubagentError::Engine(error.to_string()))
                        })
                        .transpose()
                } else {
                    Ok(None)
                }
            });
        match (lifecycle_result, release_result) {
            (result, Ok(_)) => result,
            (Ok(_), Err(error)) => Err(SubagentError::Engine(format!(
                "subagent workspace cleanup is pending: {error}"
            ))),
            (Err(lifecycle), Err(cleanup)) => Err(SubagentError::Engine(format!(
                "{lifecycle}; subagent workspace cleanup is pending: {cleanup}"
            ))),
        }
    }

    fn cancel_attached_child(&self, child_task_id: TaskId) {
        let Ok(active) = self.active.lock() else {
            return;
        };
        let Some(child) = active.get(&child_task_id) else {
            return;
        };
        if child.projection.detached {
            return;
        }
        child.control.cancel();
        child.abort.abort();
    }

    fn durable_child(
        &self,
        child: &SubagentProjection,
    ) -> Result<SubagentProjection, SubagentError> {
        self.store
            .task_projection(child.parent_task_id)
            .map_err(subagent_store_error)?
            .and_then(|parent| {
                parent
                    .subagents
                    .into_iter()
                    .find(|projected| projected.child_task_id == child.child_task_id)
            })
            .ok_or_else(|| SubagentError::Engine("durable subagent projection is missing".into()))
    }

    fn apply_lifecycle(
        &self,
        child: &SubagentProjection,
        authority: SubagentLifecycleAuthority,
        transition: SubagentLifecycleTransition,
    ) -> Result<SubagentProjection, SubagentError> {
        self.store
            .apply_subagent_lifecycle(SubagentLifecycleCommand {
                parent_task_id: child.parent_task_id,
                child_task_id: child.child_task_id,
                actor_id: child.actor_id,
                authority,
                transition,
            })
            .map_err(subagent_store_error)
    }
}

struct BoundSubagentRunner {
    supervisor: Arc<SubagentSupervisor>,
    binding: SubagentParentBinding,
}

#[async_trait]
impl SubagentRunner for BoundSubagentRunner {
    async fn spawn(
        &self,
        spec: SubagentSpec,
        input: harness_contracts::TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, SubagentError> {
        self.supervisor
            .spawn_bound(self.binding, spec, input, parent_ctx)
            .await
            .map_err(parent_safe_subagent_error)
    }
}

fn parent_workspace_lease(
    store: &TaskStore,
    task_id: TaskId,
    actor_id: ActorId,
) -> Result<TaskWorkspaceLease, SubagentError> {
    let leases = store
        .nonterminal_workspace_leases_for_task(task_id)
        .map_err(subagent_store_error)?;
    let lease = leases
        .into_iter()
        .filter(|lease| {
            lease.actor_id == actor_id
                && lease.state == harness_journal::TaskWorkspaceLeaseState::Active
        })
        .find(|lease| lease.mode == WorkspaceMode::Current)
        .or_else(|| {
            store
                .nonterminal_workspace_leases_for_task(task_id)
                .ok()
                .and_then(|leases| {
                    leases.into_iter().find(|lease| {
                        lease.actor_id == actor_id
                            && lease.state == harness_journal::TaskWorkspaceLeaseState::Active
                    })
                })
        })
        .ok_or_else(|| SubagentError::Engine("parent task has no workspace lease".into()))?;
    Ok(lease)
}

fn is_nonterminal_child_state(state: SubagentActorState) -> bool {
    matches!(
        state,
        SubagentActorState::Starting
            | SubagentActorState::Running
            | SubagentActorState::Yielding
            | SubagentActorState::Background
    )
}

fn bounded_redacted_summary(redactor: &dyn Redactor, summary: &str) -> String {
    redactor
        .redact(summary, &RedactRules::default())
        .chars()
        .take(MAX_PARENT_SUMMARY_CHARS)
        .collect()
}

fn subagent_store_error(error: impl std::fmt::Display) -> SubagentError {
    SubagentError::Engine(error.to_string())
}

fn parent_safe_subagent_error(error: SubagentError) -> SubagentError {
    match error {
        SubagentError::DepthExceeded { .. }
        | SubagentError::ConcurrentLimitExceeded
        | SubagentError::Cancelled
        | SubagentError::SpawningPaused => error,
        _ => SubagentError::Engine("subagent execution failed".into()),
    }
}

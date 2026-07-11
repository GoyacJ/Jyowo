use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use harness_contracts::{
    ClientId, CommandId, Event, NoopRedactor, PermissionProjection, PermissionRoute, PromotionMode,
    QueueItemId, QueueItemState, RequestId, RunId, RunSegmentId, RunState, RunTerminalReason,
    SessionId, TaskId, TaskState, TenantId, ToolUseId, ToolUseStartedEvent,
};
use harness_daemon::{
    QueueCommand, RunCoordinatorEvent, RunCoordinatorFactory, RunningSegment, StartSegmentRequest,
    Supervisor, SupervisorQuotas, ValidatedTaskCommand,
};
use harness_engine::{RunControlHandle, SafePointDecision, TurnOutcome};
use harness_journal::{
    AcceptedCommand, CommandOutcome, EventStore, NewTaskEvent, TaskEventStoreAdapter, TaskStore,
};
use serde_json::json;
use tokio::sync::mpsc;

#[derive(Clone, Default)]
struct SteeringFactory {
    state: Arc<Mutex<FactoryState>>,
}

#[derive(Default)]
struct FactoryState {
    starts: Vec<RunSegmentId>,
    requests: Vec<StartSegmentRequest>,
    controls: HashMap<RunSegmentId, RunControlHandle>,
    events: HashMap<RunSegmentId, mpsc::UnboundedSender<RunCoordinatorEvent>>,
    panic_next: bool,
}

impl SteeringFactory {
    fn control(&self, segment_id: RunSegmentId) -> RunControlHandle {
        self.state
            .lock()
            .unwrap()
            .controls
            .get(&segment_id)
            .unwrap()
            .clone()
    }

    fn reach_safe_point(
        &self,
        segment_id: RunSegmentId,
        forced: bool,
        non_revertible_tool_use_ids: Vec<ToolUseId>,
    ) {
        let control = self
            .state
            .lock()
            .unwrap()
            .controls
            .get(&segment_id)
            .unwrap()
            .clone();
        control.finish(if forced {
            TurnOutcome::ForceStopped {
                non_revertible_tool_use_ids,
            }
        } else {
            TurnOutcome::YieldedAtSafePoint
        });
    }

    fn complete(&self, segment_id: RunSegmentId) {
        self.state
            .lock()
            .unwrap()
            .events
            .get(&segment_id)
            .unwrap()
            .send(RunCoordinatorEvent::Completed {
                segment_id,
                terminal_reason: RunTerminalReason::Completed,
                incomplete_output: false,
                ended_at: Utc::now(),
            })
            .unwrap();
    }

    fn report_safe_point(&self, segment_id: RunSegmentId, forced: bool) {
        self.state
            .lock()
            .unwrap()
            .events
            .get(&segment_id)
            .unwrap()
            .send(RunCoordinatorEvent::SafePointReached {
                segment_id,
                forced,
                incomplete_output: true,
                non_revertible_tool_use_ids: Vec::new(),
                reached_at: Utc::now(),
            })
            .unwrap();
    }

    fn panic_on_next_start(&self) {
        self.state.lock().unwrap().panic_next = true;
    }

    fn starts(&self) -> Vec<RunSegmentId> {
        self.state.lock().unwrap().starts.clone()
    }

    fn requests(&self) -> Vec<StartSegmentRequest> {
        self.state.lock().unwrap().requests.clone()
    }
}

impl RunCoordinatorFactory for SteeringFactory {
    fn spawn_idempotent(
        &self,
        request: StartSegmentRequest,
        _workspace_tools: harness_daemon::WorkspaceToolDispatcher,
        _subagent_runner: Arc<dyn harness_subagent::SubagentRunner>,
    ) -> RunningSegment {
        let control = RunControlHandle::new();
        let (events, receiver) = mpsc::unbounded_channel();
        let mut state = self.state.lock().unwrap();
        if state.panic_next {
            state.panic_next = false;
            drop(state);
            panic!("coordinator start failed");
        }
        state.starts.push(request.segment_id);
        state.requests.push(request.clone());
        state.controls.insert(request.segment_id, control.clone());
        state.events.insert(request.segment_id, events);
        RunningSegment::with_control(request.segment_id, receiver, control)
    }
}

#[tokio::test]
async fn queue_commands_remain_available_and_promotion_invalidates_pending_permission() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = create_task(&store);
    let factory = Arc::new(SteeringFactory::default());
    let supervisor =
        Supervisor::start(store.clone(), factory.clone(), SupervisorQuotas::new(1, 1)).unwrap();
    let segment_id = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, segment_id))
            .await
            .unwrap()
    ));
    let editable_item = QueueItemId::new();
    let promoted_item = QueueItemId::new();
    for (queue_item_id, content) in [
        (editable_item, "edit then delete"),
        (promoted_item, "promote while waiting"),
    ] {
        assert!(accepted(
            supervisor
                .dispatch(
                    task_id,
                    queue_command(
                        &store,
                        task_id,
                        queue_item_id,
                        QueueCommand::Submit {
                            queue_item_id,
                            content: content.into(),
                            attachments: Vec::new(),
                            context_references: Vec::new(),
                            created_at: Utc::now(),
                        },
                    ),
                )
                .await
                .unwrap()
        ));
    }
    let permission_request_id = RequestId::new();
    assert!(accepted(
        store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: format!("permission-{permission_request_id}"),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                    authority: TaskStore::permission_broker_authority(),
                    payload: json!({ "type": "permission_request" }),
                },
                |_| {
                    Ok(vec![NewTaskEvent::permission_requested(
                        PermissionProjection {
                            request_id: permission_request_id,
                            revision: 1,
                            route: PermissionRoute::ForegroundTask,
                            details: None,
                        },
                    )])
                },
            )
            .unwrap()
    ));

    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                queue_command(
                    &store,
                    task_id,
                    editable_item,
                    QueueCommand::Edit {
                        expected_revision: 1,
                        content: "edited".into(),
                        attachments: Vec::new(),
                        context_references: Vec::new(),
                    },
                ),
            )
            .await
            .unwrap()
    ));
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                queue_command(
                    &store,
                    task_id,
                    editable_item,
                    QueueCommand::Delete {
                        expected_revision: 2,
                    },
                ),
            )
            .await
            .unwrap()
    ));
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                queue_command(
                    &store,
                    task_id,
                    promoted_item,
                    QueueCommand::Promote {
                        expected_revision: 1,
                        mode: PromotionMode::SafePoint,
                    },
                ),
            )
            .await
            .unwrap()
    ));

    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert!(projection.pending_permission.is_none());
    assert_eq!(projection.current_run.unwrap().state, RunState::Yielding);
    assert_eq!(
        factory.control(segment_id).decision(),
        SafePointDecision::Yield
    );
    let events = store.task_events_after(task_id, 0, 128).unwrap();
    let invalidated = events
        .iter()
        .position(|event| event.event_type == "permission.invalidated")
        .unwrap();
    let yielding = events
        .iter()
        .position(|event| event.event_type == "run.yield_requested")
        .unwrap();
    assert!(invalidated < yielding);
}

#[tokio::test]
async fn safe_promotion_yields_then_atomically_starts_the_promoted_message() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = create_task(&store);
    let factory = Arc::new(SteeringFactory::default());
    let supervisor =
        Supervisor::start(store.clone(), factory.clone(), SupervisorQuotas::new(1, 1)).unwrap();
    let first_segment = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, first_segment))
            .await
            .unwrap()
    ));
    let queue_item_id = QueueItemId::new();
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                queue_command(
                    &store,
                    task_id,
                    queue_item_id,
                    QueueCommand::Submit {
                        queue_item_id,
                        content: "switch now".to_owned(),
                        attachments: Vec::new(),
                        context_references: Vec::new(),
                        created_at: Utc::now(),
                    },
                ),
            )
            .await
            .unwrap()
    ));
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                queue_command(
                    &store,
                    task_id,
                    queue_item_id,
                    QueueCommand::Promote {
                        expected_revision: 1,
                        mode: PromotionMode::SafePoint,
                    },
                ),
            )
            .await
            .unwrap()
    ));

    let yielding = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(yielding.current_run.unwrap().state, RunState::Yielding);
    assert_eq!(yielding.queue[0].state, QueueItemState::Promoting);
    assert_eq!(
        factory.control(first_segment).decision(),
        SafePointDecision::Yield
    );

    factory.reach_safe_point(first_segment, false, Vec::new());
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let projection = store.task_projection(task_id).unwrap().unwrap();
            if projection.current_run.as_ref().is_some_and(|run| {
                run.segment_id != first_segment && run.state == RunState::Running
            }) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let consumed = store
        .queue_item_projection(task_id, queue_item_id)
        .unwrap()
        .unwrap();
    assert_eq!(consumed.state, QueueItemState::Consumed);
    assert_ne!(consumed.consumed_by, Some(first_segment));
    let requests = factory.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].input.queue_item_id, Some(queue_item_id));
    assert_eq!(requests[1].input.content, "switch now");
}

#[tokio::test]
async fn promotion_checkpoint_preserves_a_tool_that_is_still_running() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = create_task(&store);
    let factory = Arc::new(SteeringFactory::default());
    let supervisor =
        Supervisor::start(store.clone(), factory, SupervisorQuotas::new(1, 1)).unwrap();
    let segment_id = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, segment_id))
            .await
            .unwrap()
    ));
    let session_id = SessionId::new();
    let tool_use_id = ToolUseId::new();
    TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    )
    .append(
        TenantId::SINGLE,
        session_id,
        &[Event::ToolUseStarted(ToolUseStartedEvent {
            run_id: RunId::new(),
            tool_use_id,
            at: Utc::now(),
        })],
    )
    .await
    .unwrap();
    let queue_item_id = QueueItemId::new();
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                queue_command(
                    &store,
                    task_id,
                    queue_item_id,
                    QueueCommand::Submit {
                        queue_item_id,
                        content: "switch after tool".into(),
                        attachments: Vec::new(),
                        context_references: Vec::new(),
                        created_at: Utc::now(),
                    },
                ),
            )
            .await
            .unwrap()
    ));

    let result = supervisor
        .dispatch(
            task_id,
            queue_command(
                &store,
                task_id,
                queue_item_id,
                QueueCommand::Promote {
                    expected_revision: 1,
                    mode: PromotionMode::SafePoint,
                },
            ),
        )
        .await;

    assert!(matches!(result, Ok(CommandOutcome::Accepted { .. })));
    assert_eq!(
        store
            .latest_checkpoint(task_id)
            .unwrap()
            .unwrap()
            .incomplete_tool_use_ids,
        vec![tool_use_id]
    );
}

#[tokio::test]
async fn force_promotion_records_non_revertible_side_effects() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = create_task(&store);
    let factory = Arc::new(SteeringFactory::default());
    let supervisor =
        Supervisor::start(store.clone(), factory.clone(), SupervisorQuotas::new(1, 1)).unwrap();
    let first_segment = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, first_segment))
            .await
            .unwrap()
    ));
    let queue_item_id = QueueItemId::new();
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                queue_command(
                    &store,
                    task_id,
                    queue_item_id,
                    QueueCommand::Submit {
                        queue_item_id,
                        content: "force switch".to_owned(),
                        attachments: Vec::new(),
                        context_references: Vec::new(),
                        created_at: Utc::now(),
                    },
                ),
            )
            .await
            .unwrap()
    ));
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                queue_command(
                    &store,
                    task_id,
                    queue_item_id,
                    QueueCommand::Promote {
                        expected_revision: 1,
                        mode: PromotionMode::ForceStop,
                    },
                ),
            )
            .await
            .unwrap()
    ));
    assert_eq!(
        factory.control(first_segment).decision(),
        SafePointDecision::ForceStop
    );

    let tool_use_id = ToolUseId::new();
    factory.reach_safe_point(first_segment, true, vec![tool_use_id]);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let projection = store.task_projection(task_id).unwrap().unwrap();
            if projection.current_run.as_ref().is_some_and(|run| {
                run.segment_id != first_segment && run.state == RunState::Running
            }) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let events = store.events_after(0, 100).unwrap();
    let safe_point = events
        .iter()
        .find(|event| event.event_type == "run.safe_point_reached")
        .unwrap();
    assert_eq!(safe_point.payload["forced"], true);
    assert_eq!(
        safe_point.payload["nonRevertibleToolUseIds"][0],
        tool_use_id.to_string()
    );
    assert!(events.iter().any(|event| {
        event.event_type == "run.completed"
            && event.payload["terminalReason"] == "forced_interruption"
    }));
}

#[tokio::test]
async fn replaying_a_committed_promotion_does_not_control_the_new_segment() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = create_task(&store);
    let factory = Arc::new(SteeringFactory::default());
    let supervisor =
        Supervisor::start(store.clone(), factory.clone(), SupervisorQuotas::new(1, 1)).unwrap();
    let first_segment = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, first_segment))
            .await
            .unwrap()
    ));
    let queue_item_id = QueueItemId::new();
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                queue_command(
                    &store,
                    task_id,
                    queue_item_id,
                    QueueCommand::Submit {
                        queue_item_id,
                        content: "replay".to_owned(),
                        attachments: Vec::new(),
                        context_references: Vec::new(),
                        created_at: Utc::now(),
                    },
                ),
            )
            .await
            .unwrap()
    ));
    let promote = queue_command(
        &store,
        task_id,
        queue_item_id,
        QueueCommand::Promote {
            expected_revision: 1,
            mode: PromotionMode::SafePoint,
        },
    );
    assert!(accepted(
        supervisor.dispatch(task_id, promote.clone()).await.unwrap()
    ));
    factory.reach_safe_point(first_segment, false, Vec::new());
    let second_segment = wait_for_new_segment(&store, task_id, first_segment).await;

    assert!(accepted(
        supervisor.dispatch(task_id, promote).await.unwrap()
    ));
    assert_eq!(
        factory.control(second_segment).decision(),
        SafePointDecision::Continue
    );
}

#[tokio::test]
async fn natural_completion_during_yield_consumes_the_promoted_message() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = create_task(&store);
    let factory = Arc::new(SteeringFactory::default());
    let supervisor =
        Supervisor::start(store.clone(), factory.clone(), SupervisorQuotas::new(1, 1)).unwrap();
    let first_segment = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, first_segment))
            .await
            .unwrap()
    ));
    let queue_item_id = QueueItemId::new();
    submit_and_promote(
        &supervisor,
        &store,
        task_id,
        queue_item_id,
        PromotionMode::SafePoint,
    )
    .await;

    factory.complete(first_segment);
    let second_segment = wait_for_new_segment(&store, task_id, first_segment).await;
    let item = store
        .queue_item_projection(task_id, queue_item_id)
        .unwrap()
        .unwrap();
    assert_eq!(item.state, QueueItemState::Consumed);
    assert_eq!(item.consumed_by, Some(second_segment));
}

#[tokio::test]
async fn safe_point_mode_must_match_the_durable_request() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = create_task(&store);
    let factory = Arc::new(SteeringFactory::default());
    let supervisor =
        Supervisor::start(store.clone(), factory.clone(), SupervisorQuotas::new(1, 1)).unwrap();
    let first_segment = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, first_segment))
            .await
            .unwrap()
    ));
    let queue_item_id = QueueItemId::new();
    submit_and_promote(
        &supervisor,
        &store,
        task_id,
        queue_item_id,
        PromotionMode::SafePoint,
    )
    .await;

    factory.report_safe_point(first_segment, true);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(projection.current_run.unwrap().state, RunState::Yielding);
    assert_eq!(projection.queue[0].state, QueueItemState::Promoting);
    assert_eq!(factory.starts(), vec![first_segment]);
}

#[tokio::test]
async fn coordinator_panic_after_steering_retries_the_durable_new_segment_start() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = create_task(&store);
    let factory = Arc::new(SteeringFactory::default());
    let supervisor =
        Supervisor::start(store.clone(), factory.clone(), SupervisorQuotas::new(1, 1)).unwrap();
    let first_segment = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, first_segment))
            .await
            .unwrap()
    ));
    let queue_item_id = QueueItemId::new();
    submit_and_promote(
        &supervisor,
        &store,
        task_id,
        queue_item_id,
        PromotionMode::SafePoint,
    )
    .await;
    factory.panic_on_next_start();

    factory.reach_safe_point(first_segment, false, Vec::new());
    tokio::time::timeout(Duration::from_secs(2), async {
        while factory.starts().len() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(projection.state, TaskState::Running);
    assert_ne!(projection.current_run.unwrap().segment_id, first_segment);
    assert_eq!(
        store
            .queue_item_projection(task_id, queue_item_id)
            .unwrap()
            .unwrap()
            .state,
        QueueItemState::Consumed
    );
}

#[tokio::test]
async fn force_stop_timeout_returns_the_message_to_queue_without_starting_a_new_segment() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = create_task(&store);
    let factory = Arc::new(SteeringFactory::default());
    let supervisor =
        Supervisor::start(store.clone(), factory.clone(), SupervisorQuotas::new(1, 1)).unwrap();
    let first_segment = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, first_segment))
            .await
            .unwrap()
    ));
    let queue_item_id = QueueItemId::new();
    submit_and_promote(
        &supervisor,
        &store,
        task_id,
        queue_item_id,
        PromotionMode::ForceStop,
    )
    .await;
    let tool_use_id = ToolUseId::new();

    factory
        .control(first_segment)
        .finish(TurnOutcome::ForceStopTimedOut {
            indeterminate_tool_use_ids: vec![tool_use_id],
        });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let projection = store.task_projection(task_id).unwrap().unwrap();
            if projection.state == TaskState::Failed
                && projection.queue[0].state == QueueItemState::Queued
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    assert_eq!(factory.starts(), vec![first_segment]);
    let event = store
        .events_after(0, 100)
        .unwrap()
        .into_iter()
        .find(|event| event.event_type == "run.force_stop_timed_out")
        .unwrap();
    assert_eq!(
        event.payload["indeterminateToolUseIds"][0],
        tool_use_id.to_string()
    );
}

#[tokio::test]
async fn actor_restart_interrupts_a_yielding_run_and_recovers_its_promoting_message() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = create_task(&store);
    let first_factory = Arc::new(SteeringFactory::default());
    let first_supervisor =
        Supervisor::start(store.clone(), first_factory, SupervisorQuotas::new(1, 1)).unwrap();
    let first_segment = RunSegmentId::new();
    assert!(accepted(
        first_supervisor
            .dispatch(task_id, start_command(&store, task_id, first_segment))
            .await
            .unwrap()
    ));
    let queue_item_id = QueueItemId::new();
    submit_and_promote(
        &first_supervisor,
        &store,
        task_id,
        queue_item_id,
        PromotionMode::SafePoint,
    )
    .await;
    drop(first_supervisor);

    let second_supervisor = Supervisor::start(
        store.clone(),
        Arc::new(SteeringFactory::default()),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();
    let _ = second_supervisor
        .dispatch(
            task_id,
            queue_command(
                &store,
                task_id,
                queue_item_id,
                QueueCommand::Edit {
                    expected_revision: 2,
                    content: "after restart".to_owned(),
                    attachments: Vec::new(),
                    context_references: Vec::new(),
                },
            ),
        )
        .await
        .unwrap();

    let projection = store.task_projection(task_id).unwrap().unwrap();
    let run = projection.current_run.unwrap();
    assert_eq!(run.state, RunState::Interrupted);
    assert_eq!(
        run.terminal_reason,
        Some(RunTerminalReason::InterruptedByRestart)
    );
    assert_eq!(projection.queue[0].state, QueueItemState::Queued);
}

async fn submit_and_promote(
    supervisor: &Supervisor,
    store: &TaskStore,
    task_id: TaskId,
    queue_item_id: QueueItemId,
    mode: PromotionMode,
) {
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                queue_command(
                    store,
                    task_id,
                    queue_item_id,
                    QueueCommand::Submit {
                        queue_item_id,
                        content: "steer".to_owned(),
                        attachments: Vec::new(),
                        context_references: Vec::new(),
                        created_at: Utc::now(),
                    },
                ),
            )
            .await
            .unwrap()
    ));
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                queue_command(
                    store,
                    task_id,
                    queue_item_id,
                    QueueCommand::Promote {
                        expected_revision: 1,
                        mode,
                    },
                ),
            )
            .await
            .unwrap()
    ));
}

async fn wait_for_new_segment(
    store: &TaskStore,
    task_id: TaskId,
    old_segment: RunSegmentId,
) -> RunSegmentId {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let projection = store.task_projection(task_id).unwrap().unwrap();
            if let Some(run) = projection.current_run {
                if run.segment_id != old_segment && run.state == RunState::Running {
                    return run.segment_id;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}

fn create_task(store: &TaskStore) -> TaskId {
    let task_id = TaskId::new();
    let outcome = store
        .transact_command(command(task_id, 0, json!({ "create": true })), |_| {
            Ok(vec![NewTaskEvent::task_created("steering")])
        })
        .unwrap();
    assert!(accepted(outcome));
    task_id
}

fn start_command(
    store: &TaskStore,
    task_id: TaskId,
    segment_id: RunSegmentId,
) -> ValidatedTaskCommand {
    ValidatedTaskCommand::StartSegment {
        command: command(
            task_id,
            store.stream_version(task_id).unwrap(),
            json!({ "segmentId": segment_id }),
        ),
        segment_id,
        started_at: Utc::now(),
    }
}

fn queue_command(
    store: &TaskStore,
    task_id: TaskId,
    queue_item_id: QueueItemId,
    queue_command: QueueCommand,
) -> ValidatedTaskCommand {
    ValidatedTaskCommand::Queue {
        command: command(
            task_id,
            store.stream_version(task_id).unwrap(),
            json!({ "queueItemId": queue_item_id }),
        ),
        queue_item_id,
        queue_command,
    }
}

fn command(
    task_id: TaskId,
    expected_stream_version: u64,
    payload: serde_json::Value,
) -> AcceptedCommand {
    AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("steering-{}", CommandId::new()),
        expected_stream_version,
        authority: TaskStore::user_authority(ClientId::new()),
        payload,
    }
}

fn accepted(outcome: CommandOutcome) -> bool {
    matches!(outcome, CommandOutcome::Accepted { .. })
}

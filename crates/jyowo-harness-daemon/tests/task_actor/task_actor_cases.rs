use super::*;

#[tokio::test]
async fn first_submit_acquires_foreground_workspace() {
    let (store, root) = test_store();
    let workspace_root = root.path().join("workspace");
    std::fs::create_dir(&workspace_root).unwrap();
    let task_id = create_task_in_workspace(&store, "workspace submit", &workspace_root);
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    let outcome = supervisor
        .dispatch(task_id, submit_message_command(&store, task_id))
        .await
        .unwrap();
    assert!(
        matches!(outcome, CommandOutcome::Accepted { .. }),
        "{outcome:?}"
    );
    wait_for_start_count(&factory, task_id, 1).await;

    let projection = store.task_projection(task_id).unwrap().unwrap();
    let lease = store
        .nonterminal_workspace_leases_for_task(task_id)
        .unwrap()
        .into_iter()
        .find(|lease| lease.state == harness_contracts::WorkspaceLeaseState::Active)
        .expect("first submit acquires a workspace lease");
    assert_eq!(
        factory.requests(task_id)[0].input.workspace_lease_id,
        Some(lease.lease_id)
    );
    assert_eq!(projection.state, TaskState::Running);
}

#[tokio::test]
async fn completed_run_releases_workspace_for_next_task() {
    let (store, root) = test_store();
    let workspace_root = root.path().join("shared-workspace");
    std::fs::create_dir(&workspace_root).unwrap();
    let first = create_task_in_workspace(&store, "first", &workspace_root);
    let second = create_task_in_workspace(&store, "second", &workspace_root);
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    assert!(accepted(
        supervisor
            .dispatch(first, submit_message_command(&store, first))
            .await
            .unwrap()
    ));
    wait_for_start_count(&factory, first, 1).await;
    let segment_id = factory.requests(first)[0].segment_id;

    factory.complete(first, segment_id);
    wait_for_state(&store, first, TaskState::Completed).await;
    let terminal_events = store.events_after(0, 100).unwrap();
    let released_offset = terminal_events
        .iter()
        .find(|event| event.task_id == first && event.event_type == "workspace.released")
        .expect("completed task releases its workspace")
        .global_offset;
    let completed_offset = terminal_events
        .iter()
        .find(|event| event.task_id == first && event.event_type == "run.completed")
        .expect("run completion is persisted")
        .global_offset;
    assert!(
        released_offset < completed_offset,
        "workspace release must be visible before terminal state"
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if store
                .nonterminal_workspace_leases_for_task(first)
                .unwrap()
                .is_empty()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("completed task should release its workspace lease");

    let outcome = supervisor
        .dispatch(second, submit_message_command(&store, second))
        .await
        .unwrap();
    assert!(
        matches!(outcome, CommandOutcome::Accepted { .. }),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn removing_idle_task_releases_its_workspace_lease() {
    let (store, root) = test_store();
    let workspace_root = root.path().join("remove-workspace");
    std::fs::create_dir(&workspace_root).unwrap();
    let task_id = create_task_in_workspace(&store, "remove", &workspace_root);
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let lease = match coordinator
        .acquire(WorkspaceLeaseRequest {
            task_id,
            actor_id: ActorId::from_u128(u128::from_be_bytes(task_id.as_bytes())),
            root: workspace_root,
            mode: Some(WorkspaceMode::Current),
            access: WorkspaceAccess::Write,
            execution_kind: WorkspaceExecutionKind::Foreground,
            expires_at: None,
        })
        .unwrap()
    {
        harness_daemon::WorkspaceAcquireOutcome::Acquired(lease) => lease,
        harness_daemon::WorkspaceAcquireOutcome::Waiting(_) => panic!("workspace should be free"),
    };
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        Arc::new(ControlledFactory::default()),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    let outcome = supervisor
        .dispatch(task_id, remove_command(&store, task_id))
        .await
        .unwrap();
    assert!(
        matches!(outcome, CommandOutcome::Accepted { .. }),
        "{outcome:?}"
    );
    let removal_events = store.events_after(0, 100).unwrap();
    let released_offset = removal_events
        .iter()
        .find(|event| event.task_id == task_id && event.event_type == "workspace.released")
        .expect("removed task releases its workspace")
        .global_offset;
    let removed_offset = removal_events
        .iter()
        .find(|event| event.task_id == task_id && event.event_type == "task.removed")
        .expect("task removal is persisted")
        .global_offset;
    assert!(
        released_offset < removed_offset,
        "workspace release must be visible before task removal"
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if store
                .workspace_lease(lease.lease_id)
                .unwrap()
                .unwrap()
                .state
                == harness_contracts::WorkspaceLeaseState::Released
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("removed task should release its workspace lease");
}

#[tokio::test]
async fn force_stop_timeout_retries_workspace_release_after_dispatch_finishes() {
    let (store, root) = test_store();
    let workspace_root = root.path().join("force-stop-workspace");
    std::fs::create_dir(&workspace_root).unwrap();
    let task_id = create_task_in_workspace(&store, "force stop", &workspace_root);
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    assert!(accepted(
        supervisor
            .dispatch(task_id, submit_message_command(&store, task_id))
            .await
            .unwrap()
    ));
    wait_for_start_count(&factory, task_id, 1).await;
    let segment_id = factory.requests(task_id)[0].segment_id;
    let lease_id = store
        .nonterminal_workspace_leases_for_task(task_id)
        .unwrap()
        .into_iter()
        .find(|lease| lease.state == harness_contracts::WorkspaceLeaseState::Active)
        .unwrap()
        .lease_id;
    let dispatch = store.begin_workspace_dispatch(lease_id).unwrap();

    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                ValidatedTaskCommand::StopRun {
                    command: command(
                        task_id,
                        store.stream_version(task_id).unwrap(),
                        json!({ "type": "stop_run", "mode": "force" }),
                    ),
                    mode: StopMode::Force,
                },
            )
            .await
            .unwrap()
    ));
    factory.force_stop_timeout(task_id, segment_id);
    wait_for_state(&store, task_id, TaskState::Failed).await;
    assert_eq!(
        store.workspace_lease(lease_id).unwrap().unwrap().state,
        harness_contracts::WorkspaceLeaseState::Active
    );

    drop(dispatch);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if store.workspace_lease(lease_id).unwrap().unwrap().state
                == harness_contracts::WorkspaceLeaseState::Released
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("terminal workspace release should be retried");
}

#[tokio::test]
async fn queued_segment_retains_workspace_until_the_final_segment_completes() {
    let (store, root) = test_store();
    let workspace_root = root.path().join("queued-workspace");
    std::fs::create_dir(&workspace_root).unwrap();
    let task_id = create_task_in_workspace(&store, "queued", &workspace_root);
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    assert!(accepted(
        supervisor
            .dispatch(task_id, submit_message_command(&store, task_id))
            .await
            .unwrap()
    ));
    wait_for_start_count(&factory, task_id, 1).await;
    let first_segment = factory.requests(task_id)[0].segment_id;
    let queue_item_id = QueueItemId::new();
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                ValidatedTaskCommand::Queue {
                    command: command(
                        task_id,
                        store.stream_version(task_id).unwrap(),
                        json!({ "type": "queue", "queueItemId": queue_item_id }),
                    ),
                    queue_item_id,
                    queue_command: QueueCommand::Submit {
                        queue_item_id,
                        content: "next".into(),
                        attachments: Vec::new(),
                        context_references: Vec::new(),
                        created_at: Utc::now(),
                    },
                },
            )
            .await
            .unwrap()
    ));

    factory.complete(task_id, first_segment);
    wait_for_start_count(&factory, task_id, 2).await;
    assert!(store
        .nonterminal_workspace_leases_for_task(task_id)
        .unwrap()
        .iter()
        .any(|lease| lease.state == harness_contracts::WorkspaceLeaseState::Active));
    assert!(!store
        .events_after(0, 100)
        .unwrap()
        .iter()
        .any(|event| event.task_id == task_id && event.event_type == "workspace.released"));

    let second_segment = factory.requests(task_id)[1].segment_id;
    factory.complete(task_id, second_segment);
    wait_for_state(&store, task_id, TaskState::Completed).await;
    assert!(store
        .nonterminal_workspace_leases_for_task(task_id)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn supervisor_start_releases_a_stale_terminal_workspace_lease() {
    let (store, root) = test_store();
    let workspace_root = root.path().join("stale-terminal-workspace");
    std::fs::create_dir(&workspace_root).unwrap();
    let task_id = create_task_in_workspace(&store, "stale terminal", &workspace_root);
    let coordinator =
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap();
    let lease = match coordinator
        .acquire(WorkspaceLeaseRequest {
            task_id,
            actor_id: ActorId::from_u128(u128::from_be_bytes(task_id.as_bytes())),
            root: workspace_root,
            mode: Some(WorkspaceMode::Current),
            access: WorkspaceAccess::Write,
            execution_kind: WorkspaceExecutionKind::Foreground,
            expires_at: None,
        })
        .unwrap()
    {
        harness_daemon::WorkspaceAcquireOutcome::Acquired(lease) => lease,
        harness_daemon::WorkspaceAcquireOutcome::Waiting(_) => panic!("workspace should be free"),
    };
    let segment_id = RunSegmentId::new();
    let mut terminal_command = command(
        task_id,
        store.stream_version(task_id).unwrap(),
        json!({ "type": "seed_terminal_run" }),
    );
    terminal_command.authority = TaskStore::supervisor_authority();
    assert!(accepted(
        store
            .transact_command(terminal_command, |_| {
                Ok(vec![
                    NewTaskEvent::run_started(segment_id, Utc::now()),
                    NewTaskEvent::run_completed(
                        segment_id,
                        Utc::now(),
                        RunTerminalReason::Completed,
                        false,
                    ),
                ])
            },)
            .unwrap()
    ));

    let _supervisor = Supervisor::start(
        Arc::clone(&store),
        Arc::new(ControlledFactory::default()),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    assert_eq!(
        store
            .workspace_lease(lease.lease_id)
            .unwrap()
            .unwrap()
            .state,
        harness_contracts::WorkspaceLeaseState::Released
    );
}

#[tokio::test]
async fn missing_workspace_rejects_submit_before_run_start() {
    let (store, root) = test_store();
    let missing_root = root.path().join("missing-workspace");
    let task_id = create_task_in_workspace(&store, "missing workspace", &missing_root);
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    let outcome = supervisor
        .dispatch(task_id, submit_message_command(&store, task_id))
        .await
        .unwrap();

    assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    assert_eq!(factory.start_count(task_id), 0);
    assert!(store
        .nonterminal_workspace_leases_for_task(task_id)
        .unwrap()
        .is_empty());
    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(projection.state, TaskState::Idle);
    assert!(projection.current_run.is_none());
    assert!(projection.queue.is_empty());
    assert_eq!(projection.stream_version, 1);
}

#[tokio::test]
async fn one_task_has_one_foreground_run_while_another_task_runs_in_parallel() {
    let (store, _root) = test_store();
    let task_a = create_task(&store, "task A");
    let task_b = create_task(&store, "task B");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(2, 4),
    )
    .unwrap();

    let segment_a = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_a, start_command(&store, task_a, segment_a))
            .await
            .unwrap()
    ));
    let rejected = supervisor
        .dispatch(task_a, start_command(&store, task_a, RunSegmentId::new()))
        .await
        .unwrap();
    assert!(matches!(rejected, CommandOutcome::Rejected { .. }));

    let segment_b = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_b, start_command(&store, task_b, segment_b))
            .await
            .unwrap()
    ));

    assert_eq!(factory.start_count(task_a), 1);
    assert_eq!(factory.start_count(task_b), 1);
    assert_eq!(factory.maximum_active(task_a), 1);
    assert_eq!(factory.maximum_active(task_b), 1);

    factory.complete(task_a, segment_a);
    factory.complete(task_b, segment_b);
    wait_for_state(&store, task_a, TaskState::Completed).await;
    wait_for_state(&store, task_b, TaskState::Completed).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_full_task_mailbox_does_not_block_another_task() {
    let (store, _root) = test_store();
    let task_a = create_task(&store, "blocked mailbox");
    let task_b = create_task(&store, "independent mailbox");
    let factory = Arc::new(ControlledFactory::default());
    let gate = factory.block_next_start(task_a);
    let supervisor = Arc::new(
        Supervisor::start(Arc::clone(&store), factory, SupervisorQuotas::new(2, 1)).unwrap(),
    );
    assert!(accepted(
        supervisor
            .dispatch(task_a, start_command(&store, task_a, RunSegmentId::new()),)
            .await
            .unwrap()
    ));
    while !gate.entered.load(Ordering::Acquire) {
        tokio::task::yield_now().await;
    }

    let mut blocked_dispatches = Vec::new();
    for index in 0..129 {
        let supervisor = Arc::clone(&supervisor);
        let queue_item_id = QueueItemId::new();
        let queue_command = ValidatedTaskCommand::Queue {
            command: command(task_a, 3, json!({ "index": index })),
            queue_item_id,
            queue_command: QueueCommand::Submit {
                queue_item_id,
                content: format!("blocked {index}"),
                attachments: Vec::new(),
                context_references: Vec::new(),
                created_at: Utc::now(),
            },
        };
        blocked_dispatches.push(tokio::spawn(async move {
            supervisor.dispatch(task_a, queue_command).await
        }));
    }
    for _ in 0..256 {
        tokio::task::yield_now().await;
    }

    let task_b_result = tokio::time::timeout(
        Duration::from_millis(250),
        supervisor.dispatch(task_b, start_command(&store, task_b, RunSegmentId::new())),
    )
    .await;
    gate.release();
    for dispatch in blocked_dispatches {
        dispatch.abort();
    }

    assert!(matches!(
        task_b_result,
        Ok(Ok(CommandOutcome::Accepted { .. }))
    ));
}

#[tokio::test]
async fn accepted_start_command_replay_does_not_spawn_a_second_coordinator() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "idempotent start");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(2, 1),
    )
    .unwrap();

    let segment_id = RunSegmentId::new();
    let start = start_command(&store, task_id, segment_id);
    assert!(accepted(
        supervisor.dispatch(task_id, start.clone()).await.unwrap()
    ));
    assert!(accepted(supervisor.dispatch(task_id, start).await.unwrap()));

    assert_eq!(factory.start_count(task_id), 1);
    assert_eq!(factory.maximum_active(task_id), 1);
}

#[tokio::test]
async fn running_task_accepts_queue_edits_and_consumes_fifo_when_idle() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "queue actor");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    let active_segment = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, active_segment))
            .await
            .unwrap()
    ));

    let queue_item_id = QueueItemId::new();
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                ValidatedTaskCommand::Queue {
                    command: command(
                        task_id,
                        store.stream_version(task_id).unwrap(),
                        json!({ "type": "queue", "queueItemId": queue_item_id }),
                    ),
                    queue_item_id,
                    queue_command: QueueCommand::Submit {
                        queue_item_id,
                        content: "first draft".into(),
                        attachments: Vec::new(),
                        context_references: vec!["context:one".into()],
                        created_at: Utc::now(),
                    },
                },
            )
            .await
            .unwrap()
    ));
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                ValidatedTaskCommand::Queue {
                    command: command(
                        task_id,
                        store.stream_version(task_id).unwrap(),
                        json!({ "type": "edit", "queueItemId": queue_item_id }),
                    ),
                    queue_item_id,
                    queue_command: QueueCommand::Edit {
                        expected_revision: 1,
                        content: "edited draft".into(),
                        attachments: Vec::new(),
                        context_references: vec!["context:two".into()],
                    },
                },
            )
            .await
            .unwrap()
    ));
    let queued = store.task_projection(task_id).unwrap().unwrap().queue;
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].state, QueueItemState::Queued);
    assert_eq!(queued[0].revision, 2);
    assert_eq!(queued[0].content, "edited draft");

    factory.complete(task_id, active_segment);
    wait_for_start_count(&factory, task_id, 2).await;
    assert_eq!(factory.start_count(task_id), 2);
    let requests = factory.requests(task_id);
    assert_eq!(requests[1].input.queue_item_id, Some(queue_item_id));
    assert_eq!(requests[1].input.content, "edited draft");
    assert_eq!(requests[1].input.context_references, vec!["context:two"]);
    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert!(projection.queue.is_empty());
    assert_ne!(projection.current_run.unwrap().segment_id, active_segment);
}

#[tokio::test]
async fn queue_command_rejects_a_mismatched_item_identity() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "queue identity");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor =
        Supervisor::start(Arc::clone(&store), factory, SupervisorQuotas::new(1, 1)).unwrap();
    let segment_id = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, segment_id))
            .await
            .unwrap()
    ));

    let addressed_item = QueueItemId::new();
    let embedded_item = QueueItemId::new();
    let outcome = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::Queue {
                command: command(
                    task_id,
                    store.stream_version(task_id).unwrap(),
                    json!({ "type": "queue", "queueItemId": addressed_item }),
                ),
                queue_item_id: addressed_item,
                queue_command: QueueCommand::Submit {
                    queue_item_id: embedded_item,
                    content: "mismatched".into(),
                    attachments: Vec::new(),
                    context_references: Vec::new(),
                    created_at: Utc::now(),
                },
            },
        )
        .await
        .unwrap();

    assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    assert!(store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .queue
        .is_empty());
}

#[tokio::test]
async fn queue_capacity_rejection_does_not_kill_the_actor() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "queue capacity");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor =
        Supervisor::start(Arc::clone(&store), factory, SupervisorQuotas::new(1, 1)).unwrap();
    let mut events = supervisor.subscribe();
    let segment_id = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, segment_id))
            .await
            .unwrap()
    ));

    for index in 0..64 {
        let queue_item_id = QueueItemId::new();
        let outcome = supervisor
            .dispatch(
                task_id,
                ValidatedTaskCommand::Queue {
                    command: command(task_id, store.stream_version(task_id).unwrap(), json!({})),
                    queue_item_id,
                    queue_command: QueueCommand::Submit {
                        queue_item_id,
                        content: format!("queued {index}"),
                        attachments: Vec::new(),
                        context_references: Vec::new(),
                        created_at: Utc::now(),
                    },
                },
            )
            .await
            .unwrap();
        assert!(accepted(outcome));
    }

    let overflow_item_id = QueueItemId::new();
    let outcome = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::Queue {
                command: command(task_id, store.stream_version(task_id).unwrap(), json!({})),
                queue_item_id: overflow_item_id,
                queue_command: QueueCommand::Submit {
                    queue_item_id: overflow_item_id,
                    content: "overflow".into(),
                    attachments: Vec::new(),
                    context_references: Vec::new(),
                    created_at: Utc::now(),
                },
            },
        )
        .await
        .unwrap();

    assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), events.recv())
            .await
            .is_err()
    );
    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(projection.state, TaskState::Running);
    assert_eq!(projection.queue.len(), 64);
}

#[tokio::test]
async fn terminal_queue_item_id_cannot_be_submitted_again() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "queue tombstone");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor =
        Supervisor::start(Arc::clone(&store), factory, SupervisorQuotas::new(1, 1)).unwrap();
    let segment_id = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, segment_id))
            .await
            .unwrap()
    ));

    let queue_item_id = QueueItemId::new();
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                ValidatedTaskCommand::Queue {
                    command: command(task_id, store.stream_version(task_id).unwrap(), json!({})),
                    queue_item_id,
                    queue_command: QueueCommand::Submit {
                        queue_item_id,
                        content: "original".into(),
                        attachments: Vec::new(),
                        context_references: Vec::new(),
                        created_at: Utc::now(),
                    },
                },
            )
            .await
            .unwrap()
    ));
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                ValidatedTaskCommand::Queue {
                    command: command(task_id, store.stream_version(task_id).unwrap(), json!({})),
                    queue_item_id,
                    queue_command: QueueCommand::Delete {
                        expected_revision: 1,
                    },
                },
            )
            .await
            .unwrap()
    ));

    let outcome = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::Queue {
                command: command(task_id, store.stream_version(task_id).unwrap(), json!({})),
                queue_item_id,
                queue_command: QueueCommand::Submit {
                    queue_item_id,
                    content: "resurrected".into(),
                    attachments: Vec::new(),
                    context_references: Vec::new(),
                    created_at: Utc::now(),
                },
            },
        )
        .await
        .unwrap();

    assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    assert!(store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .queue
        .is_empty());
}

#[tokio::test]
async fn missing_queue_attachment_is_rejected_without_killing_the_actor() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "queue attachment");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();
    let mut events = supervisor.subscribe();
    let segment_id = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, segment_id))
            .await
            .unwrap()
    ));

    let queue_item_id = QueueItemId::new();
    let outcome = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::Queue {
                command: command(
                    task_id,
                    store.stream_version(task_id).unwrap(),
                    json!({ "type": "queue", "queueItemId": queue_item_id }),
                ),
                queue_item_id,
                queue_command: QueueCommand::Submit {
                    queue_item_id,
                    content: "missing attachment".into(),
                    attachments: vec![BlobId::new()],
                    context_references: Vec::new(),
                    created_at: Utc::now(),
                },
            },
        )
        .await
        .unwrap();

    assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), events.recv())
            .await
            .is_err()
    );
    assert_eq!(
        store.task_projection(task_id).unwrap().unwrap().state,
        TaskState::Running
    );

    factory.complete(task_id, segment_id);
    wait_for_state(&store, task_id, TaskState::Completed).await;
}

#[tokio::test]
async fn out_of_range_expected_version_is_rejected_without_killing_the_actor() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "version range");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();
    let mut events = supervisor.subscribe();
    let segment_id = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, segment_id))
            .await
            .unwrap()
    ));

    let queue_item_id = QueueItemId::new();
    let outcome = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::Queue {
                command: command(task_id, u64::MAX, json!({})),
                queue_item_id,
                queue_command: QueueCommand::Submit {
                    queue_item_id,
                    content: "invalid version".into(),
                    attachments: Vec::new(),
                    context_references: Vec::new(),
                    created_at: Utc::now(),
                },
            },
        )
        .await
        .unwrap();

    assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), events.recv())
            .await
            .is_err()
    );
    assert_eq!(
        store.task_projection(task_id).unwrap().unwrap().state,
        TaskState::Running
    );

    factory.complete(task_id, segment_id);
    wait_for_state(&store, task_id, TaskState::Completed).await;
}

#[tokio::test]
async fn queue_idempotency_binds_the_full_queue_command() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "queue idempotency");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor =
        Supervisor::start(Arc::clone(&store), factory, SupervisorQuotas::new(1, 1)).unwrap();
    let segment_id = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, segment_id))
            .await
            .unwrap()
    ));

    let queue_item_id = QueueItemId::new();
    let durable_command = command(
        task_id,
        store.stream_version(task_id).unwrap(),
        json!({ "untrusted": "sidecar omitted" }),
    );
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                ValidatedTaskCommand::Queue {
                    command: durable_command.clone(),
                    queue_item_id,
                    queue_command: QueueCommand::Submit {
                        queue_item_id,
                        content: "first".into(),
                        attachments: Vec::new(),
                        context_references: vec!["context:first".into()],
                        created_at: Utc::now(),
                    },
                },
            )
            .await
            .unwrap()
    ));

    let outcome = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::Queue {
                command: durable_command,
                queue_item_id,
                queue_command: QueueCommand::Edit {
                    expected_revision: 1,
                    content: "second".into(),
                    attachments: Vec::new(),
                    context_references: vec!["context:second".into()],
                },
            },
        )
        .await
        .unwrap();

    assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    let queued = &store.task_projection(task_id).unwrap().unwrap().queue[0];
    assert_eq!(queued.content, "first");
    assert_eq!(queued.revision, 1);
}

#[tokio::test]
async fn submit_replay_ignores_new_daemon_generated_identity_and_timestamp() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "queue derived replay");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor =
        Supervisor::start(Arc::clone(&store), factory, SupervisorQuotas::new(1, 1)).unwrap();
    let segment_id = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, segment_id))
            .await
            .unwrap()
    ));

    let durable_command = command(task_id, store.stream_version(task_id).unwrap(), json!({}));
    let first_item_id = QueueItemId::new();
    let first = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::Queue {
                command: durable_command.clone(),
                queue_item_id: first_item_id,
                queue_command: QueueCommand::Submit {
                    queue_item_id: first_item_id,
                    content: "stable request".into(),
                    attachments: Vec::new(),
                    context_references: vec!["context:stable".into()],
                    created_at: Utc::now(),
                },
            },
        )
        .await
        .unwrap();
    assert!(accepted(first.clone()));

    let replay_item_id = QueueItemId::new();
    let replayed = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::Queue {
                command: durable_command,
                queue_item_id: replay_item_id,
                queue_command: QueueCommand::Submit {
                    queue_item_id: replay_item_id,
                    content: "stable request".into(),
                    attachments: Vec::new(),
                    context_references: vec!["context:stable".into()],
                    created_at: Utc::now(),
                },
            },
        )
        .await
        .unwrap();

    assert_eq!(replayed, first);
    let queue = store.task_projection(task_id).unwrap().unwrap().queue;
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].queue_item_id, first_item_id);
}

#[tokio::test]
async fn client_queue_commands_cannot_consume_or_recover_promoting_messages() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "queue authority");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor =
        Supervisor::start(Arc::clone(&store), factory, SupervisorQuotas::new(1, 1)).unwrap();
    let segment_id = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, segment_id))
            .await
            .unwrap()
    ));
    let queue_item_id = QueueItemId::new();
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                ValidatedTaskCommand::Queue {
                    command: command(task_id, store.stream_version(task_id).unwrap(), json!({})),
                    queue_item_id,
                    queue_command: QueueCommand::Submit {
                        queue_item_id,
                        content: "promote me".into(),
                        attachments: Vec::new(),
                        context_references: Vec::new(),
                        created_at: Utc::now(),
                    },
                },
            )
            .await
            .unwrap()
    ));
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                ValidatedTaskCommand::Queue {
                    command: command(task_id, store.stream_version(task_id).unwrap(), json!({})),
                    queue_item_id,
                    queue_command: QueueCommand::Promote {
                        expected_revision: 1,
                        mode: harness_contracts::PromotionMode::SafePoint,
                    },
                },
            )
            .await
            .unwrap()
    ));

    for queue_command in [
        QueueCommand::Consume {
            expected_revision: 1,
            run_segment_id: segment_id,
        },
        QueueCommand::Recover,
    ] {
        let outcome = supervisor
            .dispatch(
                task_id,
                ValidatedTaskCommand::Queue {
                    command: command(task_id, store.stream_version(task_id).unwrap(), json!({})),
                    queue_item_id,
                    queue_command,
                },
            )
            .await
            .unwrap();
        assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    }

    let queued = &store.task_projection(task_id).unwrap().unwrap().queue[0];
    assert_eq!(queued.state, QueueItemState::Promoting);
}

#[tokio::test]
async fn a_completed_task_accepts_a_new_message_as_a_new_segment() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "repeatable");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    let first = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, first))
            .await
            .unwrap()
    ));
    factory.complete(task_id, first);
    wait_for_state(&store, task_id, TaskState::Completed).await;

    let second = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, second))
            .await
            .unwrap()
    ));
    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(projection.current_run.unwrap().segment_id, second);
    assert_eq!(factory.start_count(task_id), 2);
    assert_eq!(factory.maximum_active(task_id), 1);
}

#[tokio::test]
async fn actor_panic_retries_the_durable_start_and_does_not_stop_another_task() {
    let (store, _root) = test_store();
    let task_a = create_task(&store, "crashing");
    let task_b = create_task(&store, "survivor");
    let factory = Arc::new(ControlledFactory::default());
    factory.panic_on_next_start(task_a);
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(2, 2),
    )
    .unwrap();
    let failed_segment = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_a, start_command(&store, task_a, failed_segment),)
            .await
            .unwrap()
    ));

    tokio::time::timeout(Duration::from_secs(2), async {
        while factory.start_count(task_a) < 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let recovered = store.task_projection(task_a).unwrap().unwrap();
    assert_eq!(recovered.state, TaskState::Running);
    let recovered_run = recovered.current_run.unwrap();
    assert_eq!(recovered_run.segment_id, failed_segment);
    assert_eq!(recovered_run.state, RunState::Running);
    assert!(store
        .pending_segment_start(task_a, failed_segment)
        .unwrap()
        .is_none());

    let survivor_segment = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_b, start_command(&store, task_b, survivor_segment),)
            .await
            .unwrap()
    ));
    assert_eq!(factory.start_count(task_b), 1);
}

#[tokio::test]
async fn exhausted_global_quota_does_not_publish_a_run_that_never_started() {
    let (store, _root) = test_store();
    let task_a = create_task(&store, "occupies quota");
    let task_b = create_task(&store, "cannot start");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    let segment_a = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_a, start_command(&store, task_a, segment_a))
            .await
            .unwrap()
    ));
    let outcome = supervisor
        .dispatch(task_b, start_command(&store, task_b, RunSegmentId::new()))
        .await
        .unwrap();

    assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    assert_eq!(
        store.task_projection(task_b).unwrap().unwrap().state,
        TaskState::Idle
    );
    assert_eq!(factory.start_count(task_b), 0);
}

#[tokio::test]
async fn coordinator_channel_close_fails_the_run_and_releases_its_slot() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "coordinator closes");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    let first = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, first))
            .await
            .unwrap()
    ));
    factory.close_without_terminal_event(task_id, first);
    wait_for_state(&store, task_id, TaskState::Failed).await;

    let second = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, second))
            .await
            .unwrap()
    ));
    assert_eq!(factory.start_count(task_id), 2);
}

#[tokio::test]
async fn conflicting_client_command_is_rejected_without_killing_the_actor() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "command conflict");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();
    let mut events = supervisor.subscribe();

    let first_segment = RunSegmentId::new();
    let first_command = start_command(&store, task_id, first_segment);
    assert!(accepted(
        supervisor
            .dispatch(task_id, first_command.clone())
            .await
            .unwrap()
    ));
    let mut conflicting = first_command;
    let ValidatedTaskCommand::StartSegment {
        command,
        segment_id,
        ..
    } = &mut conflicting
    else {
        unreachable!()
    };
    *segment_id = RunSegmentId::new();
    command.payload = json!({ "segmentId": segment_id });

    let outcome = supervisor.dispatch(task_id, conflicting).await.unwrap();
    assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), events.recv())
            .await
            .is_err()
    );
    assert_eq!(
        store.task_projection(task_id).unwrap().unwrap().state,
        TaskState::Running
    );

    factory.complete(task_id, first_segment);
    wait_for_state(&store, task_id, TaskState::Completed).await;
}

#[tokio::test]
async fn conflicting_client_command_during_permission_wait_is_rejected_without_killing_the_actor() {
    let (store, _root) = test_store();
    let task_id = create_task(&store, "permission command conflict");
    let factory = Arc::new(ControlledFactory::default());
    let supervisor =
        Supervisor::start(Arc::clone(&store), factory, SupervisorQuotas::new(1, 1)).unwrap();
    let mut events = supervisor.subscribe();
    let segment_id = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, segment_id))
            .await
            .unwrap()
    ));
    let queue_item_id = QueueItemId::new();
    assert!(accepted(
        supervisor
            .dispatch(
                task_id,
                ValidatedTaskCommand::Queue {
                    command: command(task_id, store.stream_version(task_id).unwrap(), json!({}),),
                    queue_item_id,
                    queue_command: QueueCommand::Submit {
                        queue_item_id,
                        content: "promote after permission".into(),
                        attachments: Vec::new(),
                        context_references: Vec::new(),
                        created_at: Utc::now(),
                    },
                },
            )
            .await
            .unwrap()
    ));

    let durable_command = command(
        task_id,
        store.stream_version(task_id).unwrap(),
        json!({ "type": "reserved" }),
    );
    assert!(accepted(
        store
            .transact_command(durable_command.clone(), |_| {
                Ok(vec![NewTaskEvent::task_title_changed("reserved")])
            })
            .unwrap()
    ));
    let request_id = RequestId::new();
    supervisor
        .permission_broker()
        .request(PermissionRequestDraft {
            task_id,
            segment_id,
            request_id,
            request_revision: 1,
            expected_task_version: store.stream_version(task_id).unwrap(),
            kind: DaemonPermissionKind::Command,
            action_plan_hash: "plan-v1".into(),
            sandbox_policy_hash: "sandbox-v1".into(),
            workspace: "/workspace".into(),
            subject: json!({ "operation": "command" }),
            actor_source: json!({ "type": "parent_run" }),
            options: vec![PermissionOption {
                option_id: "deny-once".into(),
                label: "Deny once".into(),
            }],
            preview: "command".into(),
            expires_at: Utc::now() + chrono::Duration::minutes(5),
        })
        .unwrap();

    let outcome = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::Queue {
                command: durable_command,
                queue_item_id,
                queue_command: QueueCommand::Promote {
                    expected_revision: 1,
                    mode: harness_contracts::PromotionMode::SafePoint,
                },
            },
        )
        .await
        .unwrap();

    assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), events.recv())
            .await
            .is_err()
    );
    assert_eq!(
        store.task_projection(task_id).unwrap().unwrap().state,
        TaskState::WaitingPermission
    );
    assert_eq!(
        supervisor
            .permission_broker()
            .resolve(harness_daemon::PermissionDecisionInput {
                task_id,
                request_id,
                request_revision: 1,
                option_id: "deny-once".into(),
                expected_task_version: store.stream_version(task_id).unwrap(),
            })
            .is_ok(),
        true
    );
}

#[tokio::test]
async fn unknown_task_is_rejected_without_publishing_actor_failure() {
    let (store, _root) = test_store();
    let factory = Arc::new(ControlledFactory::default());
    let supervisor =
        Supervisor::start(Arc::clone(&store), factory, SupervisorQuotas::new(1, 1)).unwrap();
    let mut events = supervisor.subscribe();
    let task_id = TaskId::new();
    let command = ValidatedTaskCommand::StartSegment {
        command: command(task_id, 0, json!({ "missing": true })),
        segment_id: RunSegmentId::new(),
        started_at: Utc::now(),
    };

    let outcome = supervisor.dispatch(task_id, command).await.unwrap();
    assert!(matches!(outcome, CommandOutcome::Rejected { .. }));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), events.recv())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn subagent_quota_is_owned_by_the_supervisor_and_applies_backpressure() {
    let (store, _root) = test_store();
    let supervisor = Supervisor::start(
        store,
        Arc::new(ControlledFactory::default()),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    let first = supervisor.acquire_subagent_permit().await.unwrap();
    assert!(tokio::time::timeout(
        Duration::from_millis(50),
        supervisor.acquire_subagent_permit()
    )
    .await
    .is_err());
    drop(first);
    let _second =
        tokio::time::timeout(Duration::from_secs(1), supervisor.acquire_subagent_permit())
            .await
            .unwrap()
            .unwrap();
}

#[test]
fn zero_supervisor_quota_is_rejected() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _guard = runtime.enter();
    let (store, _root) = test_store();
    assert!(matches!(
        Supervisor::start(
            store,
            Arc::new(ControlledFactory::default()),
            SupervisorQuotas::new(0, 1),
        ),
        Err(harness_daemon::SupervisorError::InvalidQuota)
    ));
}

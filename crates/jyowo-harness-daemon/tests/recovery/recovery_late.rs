use super::*;

#[tokio::test]
async fn checkpoint_rejects_a_completed_tool_forged_as_incomplete() {
    let (store, root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("reject forged incomplete tool"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );
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
        &[
            Event::ToolUseStarted(ToolUseStartedEvent {
                run_id: RunId::new(),
                tool_use_id,
                at: Utc::now(),
            }),
            Event::ToolUseCompleted(ToolUseCompletedEvent {
                tool_use_id,
                result: ToolResult::Text("persisted result".into()),
                usage: None,
                duration_ms: 1,
                at: Utc::now(),
            }),
        ],
    )
    .await
    .unwrap();
    let checkpoint = store.latest_checkpoint(task_id).unwrap().unwrap();
    assert!(checkpoint.incomplete_tool_use_ids.is_empty());

    rusqlite::Connection::open(root.path().join("tasks.sqlite"))
        .unwrap()
        .execute(
            "UPDATE checkpoints
             SET checkpoint_json = json_set(
                 checkpoint_json,
                 '$.incompleteToolUseIds',
                 json_array(?1)
             )
             WHERE checkpoint_id = ?2",
            rusqlite::params![
                tool_use_id.to_string(),
                checkpoint.checkpoint_id.to_string()
            ],
        )
        .unwrap();

    assert!(matches!(
        store.latest_checkpoint(task_id),
        Err(harness_journal::TaskStoreError::ProjectionIntegrity(_))
    ));
    assert!(matches!(
        store.transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: format!("test-{}", CommandId::new()),
                expected_stream_version: store.stream_version(task_id).unwrap(),
                authority: TaskStore::supervisor_authority(),
                payload: json!({ "type": "test_setup" }),
            },
            |_| Ok(vec![NewTaskEvent::run_completed(
                segment_id,
                Utc::now(),
                RunTerminalReason::Completed,
                false,
            )]),
        ),
        Err(harness_journal::TaskStoreError::ProjectionIntegrity(_))
    ));
}

#[test]
fn checkpoint_offset_must_belong_to_its_declared_run_segment() {
    let (store, root) = test_store();
    let task_id = TaskId::new();
    let first_segment_id = RunSegmentId::new();
    let second_segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("bind checkpoint segment"),
            NewTaskEvent::run_started(first_segment_id, Utc::now()),
            NewTaskEvent::run_completed(
                first_segment_id,
                Utc::now(),
                RunTerminalReason::Completed,
                false,
            ),
            NewTaskEvent::run_started(second_segment_id, Utc::now()),
        ],
    );
    let checkpoint = store.latest_checkpoint(task_id).unwrap().unwrap();
    let first_run_offset = store
        .run_started_global_offset(task_id, first_segment_id)
        .unwrap()
        .unwrap();
    rusqlite::Connection::open(root.path().join("tasks.sqlite"))
        .unwrap()
        .execute(
            "UPDATE checkpoints
             SET committed_global_offset = ?1,
                 checkpoint_json = json_set(
                     checkpoint_json,
                     '$.committedGlobalOffset',
                     ?1
                 )
             WHERE checkpoint_id = ?2",
            rusqlite::params![
                i64::try_from(first_run_offset).unwrap(),
                checkpoint.checkpoint_id.to_string(),
            ],
        )
        .unwrap();

    assert!(matches!(
        store.latest_checkpoint(task_id),
        Err(harness_journal::TaskStoreError::ProjectionIntegrity(_))
    ));
}

#[tokio::test]
async fn completed_tool_call_advances_the_safe_checkpoint() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("checkpoint completed tool"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );
    CheckpointService::new(Arc::clone(&store))
        .persist(
            task_id,
            segment_id,
            CheckpointState {
                context_cursor: 9,
                ..CheckpointState::default()
            },
        )
        .unwrap();
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
        &[
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id: RunId::new(),
                tool_use_id,
                tool_name: "read_file".into(),
                input: json!({ "path": "result.txt" }),
                properties: ToolProperties {
                    is_concurrency_safe: false,
                    is_read_only: true,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                causation_id: EventId::new(),
                at: Utc::now(),
            }),
            Event::ToolUseCompleted(ToolUseCompletedEvent {
                tool_use_id,
                result: ToolResult::Text("persisted result".into()),
                usage: None,
                duration_ms: 1,
                at: Utc::now(),
            }),
        ],
    )
    .await
    .unwrap();

    let checkpoint = store.latest_checkpoint(task_id).unwrap().unwrap();
    assert_eq!(checkpoint.committed_global_offset, 4);
    assert_eq!(checkpoint.context_cursor, 9);
    assert!(checkpoint.incomplete_tool_use_ids.is_empty());
}

#[test]
fn permission_decision_advances_the_safe_checkpoint() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let request_id = RequestId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("checkpoint permission"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );
    CheckpointService::new(Arc::clone(&store))
        .persist(task_id, segment_id, CheckpointState::default())
        .unwrap();
    append_permission(
        &store,
        task_id,
        2,
        vec![NewTaskEvent::permission_requested(PermissionProjection {
            request_id,
            revision: 1,
            route: PermissionRoute::ForegroundTask,
            details: None,
        })],
    );
    append_permission(
        &store,
        task_id,
        3,
        vec![NewTaskEvent::permission_resolved(request_id, 1)],
    );

    assert_eq!(
        store
            .latest_checkpoint(task_id)
            .unwrap()
            .unwrap()
            .committed_global_offset,
        4
    );
}

#[test]
fn subagent_state_change_advances_the_checkpoint_and_child_refs() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let child_actor_id = ActorId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("checkpoint subagent"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );
    CheckpointService::new(Arc::clone(&store))
        .persist(task_id, segment_id, CheckpointState::default())
        .unwrap();
    append(
        &store,
        task_id,
        2,
        vec![NewTaskEvent::subagent_spawned(child_actor_id, Utc::now())],
    );

    let checkpoint = store.latest_checkpoint(task_id).unwrap().unwrap();
    assert_eq!(checkpoint.committed_global_offset, 3);
    assert_eq!(checkpoint.child_actor_refs, vec![child_actor_id]);
}

#[tokio::test]
async fn continue_task_requires_all_indeterminate_tool_decisions_and_starts_a_new_segment() {
    let (store, root) = test_store();
    let task_id = TaskId::new();
    let interrupted_segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            task_created_in_test_workspace(root.path(), task_id, "continue recovered task"),
            NewTaskEvent::run_started(interrupted_segment_id, Utc::now()),
        ],
    );
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
        &[
            Event::ToolUseRequested(ToolUseRequestedEvent {
                run_id: RunId::new(),
                tool_use_id,
                tool_name: "write_file".into(),
                input: json!({ "path": "result.txt" }),
                properties: ToolProperties {
                    is_concurrency_safe: false,
                    is_read_only: false,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                causation_id: EventId::new(),
                at: Utc::now(),
            }),
            Event::ToolUseStarted(ToolUseStartedEvent {
                run_id: RunId::new(),
                tool_use_id,
                at: Utc::now(),
            }),
        ],
    )
    .await
    .unwrap();
    RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();
    let factory = Arc::new(RecordingFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    let bypass = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::StartSegment {
                command: AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: format!("bypass-{}", CommandId::new()),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                    authority: TaskStore::user_authority(ClientId::new()),
                    payload: json!({ "type": "start_segment" }),
                },
                segment_id: RunSegmentId::new(),
                started_at: Utc::now(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(bypass, CommandOutcome::Rejected { .. }));
    assert!(factory.requests.lock().unwrap().is_empty());

    let rejected_segment_id = RunSegmentId::new();
    let rejected = supervisor
        .dispatch(
            task_id,
            continue_command(&store, task_id, rejected_segment_id, Vec::new()),
        )
        .await
        .unwrap();
    assert!(matches!(rejected, CommandOutcome::Rejected { .. }));
    assert!(factory.requests.lock().unwrap().is_empty());

    let continued_segment_id = RunSegmentId::new();
    let accepted = supervisor
        .dispatch(
            task_id,
            continue_command(
                &store,
                task_id,
                continued_segment_id,
                vec![IndeterminateToolDecision {
                    tool_use_id: tool_use_id.to_string(),
                    resolution: IndeterminateToolResolution::TreatAsFailed,
                }],
            ),
        )
        .await
        .unwrap();
    assert!(matches!(accepted, CommandOutcome::Accepted { .. }));
    assert_ne!(continued_segment_id, interrupted_segment_id);
    let requests = factory.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].task_id, task_id);
    assert_eq!(requests[0].segment_id, continued_segment_id);
    assert_eq!(
        requests[0].indeterminate_tools,
        vec![IndeterminateToolDecision {
            tool_use_id: tool_use_id.to_string(),
            resolution: IndeterminateToolResolution::TreatAsFailed,
        }]
    );
    assert!(store
        .pending_segment_start(task_id, continued_segment_id)
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .task_events_after(task_id, 0, 100)
            .unwrap()
            .iter()
            .filter(|event| event.event_type == "engine.tool_use_requested")
            .count(),
        1
    );
}

#[tokio::test]
async fn continue_task_reuses_the_interrupted_segments_immutable_run_input() {
    let (store, root) = test_store();
    let task_id = TaskId::new();
    let queue_item_id = QueueItemId::new();
    let interrupted_segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![task_created_in_test_workspace(
            root.path(),
            task_id,
            "continue immutable input",
        )],
    );
    append(
        &store,
        task_id,
        1,
        vec![
            NewTaskEvent::run_started(interrupted_segment_id, Utc::now()),
            NewTaskEvent::message_queued_with_runtime(
                queue_item_id,
                "resume this exact prompt",
                Vec::new(),
                vec!["context:durable".into()],
                Some("provider-config-continue".into()),
                PermissionMode::AcceptEdits,
                Utc::now(),
            ),
            NewTaskEvent::message_consumed(queue_item_id, 1, interrupted_segment_id),
        ],
    );
    RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();

    let factory = Arc::new(RecordingFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();
    let continued_segment_id = RunSegmentId::new();
    assert!(matches!(
        supervisor
            .dispatch(
                task_id,
                continue_command(&store, task_id, continued_segment_id, Vec::new()),
            )
            .await
            .unwrap(),
        CommandOutcome::Accepted { .. }
    ));

    let requests = factory.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].input.queue_item_id, Some(queue_item_id));
    assert_eq!(requests[0].input.content, "resume this exact prompt");
    assert_eq!(
        requests[0].input.context_references,
        vec!["context:durable"]
    );
    assert_eq!(
        requests[0].input.model_config_id.as_deref(),
        Some("provider-config-continue")
    );
    assert_eq!(
        requests[0].input.permission_mode,
        PermissionMode::AcceptEdits
    );
    assert_eq!(
        requests[0].input.session_id,
        SessionId::from_u128(u128::from_be_bytes(task_id.as_bytes()))
    );
    assert_eq!(
        requests[0].input.run_id,
        RunId::from_u128(u128::from_be_bytes(continued_segment_id.as_bytes()))
    );
}

#[tokio::test]
async fn committed_continue_decisions_are_redelivered_after_restart_before_spawn() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let interrupted_segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("redeliver committed continuation"),
            NewTaskEvent::run_started(interrupted_segment_id, Utc::now()),
        ],
    );
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
    RecoveryService::new(Arc::clone(&store))
        .recover_startup()
        .unwrap();
    let continued_segment_id = RunSegmentId::new();
    let decisions = vec![IndeterminateToolDecision {
        tool_use_id: tool_use_id.to_string(),
        resolution: IndeterminateToolResolution::ExecuteAgain,
    }];
    let command = AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("continue-{}", CommandId::new()),
        expected_stream_version: store.stream_version(task_id).unwrap(),
        authority: TaskStore::supervisor_authority(),
        payload: json!({
            "type": "continue_task",
            "segmentId": continued_segment_id,
            "indeterminateTools": decisions,
        }),
    };
    assert!(matches!(
        store.transact_command(command, |_| {
            Ok(vec![NewTaskEvent::run_started_with_recovery(
                continued_segment_id,
                Utc::now(),
                decisions.clone(),
            )])
        }),
        Ok(CommandOutcome::Accepted { .. })
    ));
    let expected_input = store
        .pending_segment_start(task_id, continued_segment_id)
        .unwrap()
        .unwrap()
        .input;

    let factory = Arc::new(RecordingFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();
    let queue_item_id = QueueItemId::new();
    let _ = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::Queue {
                command: AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: format!("route-{}", CommandId::new()),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                    authority: TaskStore::user_authority(ClientId::new()),
                    payload: json!({ "type": "queue" }),
                },
                queue_item_id,
                queue_command: harness_daemon::QueueCommand::Submit {
                    queue_item_id,
                    content: "route actor".into(),
                    attachments: Vec::new(),
                    context_references: Vec::new(),
                    created_at: Utc::now(),
                },
            },
        )
        .await;

    assert_eq!(
        factory.requests.lock().unwrap().as_slice(),
        &[StartSegmentRequest {
            task_id,
            segment_id: continued_segment_id,
            input: expected_input,
            indeterminate_tools: decisions,
        }]
    );
}

#[tokio::test]
async fn pending_continue_outbox_is_retried_after_actor_failure() {
    let (store, _root) = test_store();
    let task_id = TaskId::new();
    let interrupted_segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("retry pending continuation"),
            NewTaskEvent::run_started(interrupted_segment_id, Utc::now()),
            NewTaskEvent::run_completed(
                interrupted_segment_id,
                Utc::now(),
                RunTerminalReason::InterruptedByRestart,
                true,
            ),
        ],
    );
    let continued_segment_id = RunSegmentId::new();
    let command = AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("continue-{}", CommandId::new()),
        expected_stream_version: store.stream_version(task_id).unwrap(),
        authority: TaskStore::supervisor_authority(),
        payload: json!({
            "type": "continue_task",
            "segmentId": continued_segment_id,
            "indeterminateTools": [],
        }),
    };
    assert!(matches!(
        store.transact_command(command, |_| {
            Ok(vec![NewTaskEvent::run_started_with_recovery(
                continued_segment_id,
                Utc::now(),
                Vec::new(),
            )])
        }),
        Ok(CommandOutcome::Accepted { .. })
    ));

    let factory = Arc::new(PanicThreeTimesFactory::default());
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();
    let queue_item_id = QueueItemId::new();
    let dispatch = supervisor.dispatch(
        task_id,
        ValidatedTaskCommand::Queue {
            command: AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: format!("route-{}", CommandId::new()),
                expected_stream_version: store.stream_version(task_id).unwrap(),
                authority: TaskStore::user_authority(ClientId::new()),
                payload: json!({ "type": "queue" }),
            },
            queue_item_id,
            queue_command: harness_daemon::QueueCommand::Submit {
                queue_item_id,
                content: "route actor".into(),
                attachments: Vec::new(),
                context_references: Vec::new(),
                created_at: Utc::now(),
            },
        },
    );
    tokio::pin!(dispatch);
    tokio::select! {
        biased;
        _ = factory.first_attempt.notified() => {}
        _ = &mut dispatch => {
            panic!("dispatch returned before the first delivery attempt was observed");
        }
    }

    tokio::time::sleep(Duration::from_millis(10)).await;
    assert_eq!(factory.attempts.load(Ordering::SeqCst), 1);

    tokio::time::timeout(Duration::from_secs(1), async {
        while factory.attempts.load(Ordering::SeqCst) < 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("pending segment start should be retried");
    assert!(store
        .pending_segment_start(task_id, continued_segment_id)
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .task_projection(task_id)
            .unwrap()
            .unwrap()
            .current_run
            .unwrap()
            .state,
        RunState::Running
    );
}

#[test]
fn pending_continue_outbox_must_match_the_canonical_run_start() {
    let (store, root) = test_store();
    let task_id = TaskId::new();
    let interrupted_segment_id = RunSegmentId::new();
    append(
        &store,
        task_id,
        0,
        vec![
            NewTaskEvent::task_created("bind continuation outbox"),
            NewTaskEvent::run_started(interrupted_segment_id, Utc::now()),
            NewTaskEvent::run_completed(
                interrupted_segment_id,
                Utc::now(),
                RunTerminalReason::InterruptedByRestart,
                true,
            ),
        ],
    );
    let continued_segment_id = RunSegmentId::new();
    let command = AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("continue-{}", CommandId::new()),
        expected_stream_version: store.stream_version(task_id).unwrap(),
        authority: TaskStore::supervisor_authority(),
        payload: json!({
            "type": "continue_task",
            "segmentId": continued_segment_id,
            "indeterminateTools": [],
        }),
    };
    assert!(matches!(
        store.transact_command(command, |_| {
            Ok(vec![NewTaskEvent::run_started_with_recovery(
                continued_segment_id,
                Utc::now(),
                Vec::new(),
            )])
        }),
        Ok(CommandOutcome::Accepted { .. })
    ));
    rusqlite::Connection::open(root.path().join("tasks.sqlite"))
        .unwrap()
        .execute(
            "UPDATE segment_start_outbox
             SET request_json = json_set(
                 request_json,
                 '$.indeterminateTools',
                 json_array(json_object('toolUseId', ?1, 'resolution', 'execute_again'))
             )
             WHERE task_id = ?2 AND run_segment_id = ?3",
            rusqlite::params![
                ToolUseId::new().to_string(),
                task_id.to_string(),
                continued_segment_id.to_string(),
            ],
        )
        .unwrap();

    assert!(matches!(
        store.pending_segment_start(task_id, continued_segment_id),
        Err(harness_journal::TaskStoreError::ProjectionIntegrity(_))
    ));
    rusqlite::Connection::open(root.path().join("tasks.sqlite"))
        .unwrap()
        .execute(
            "DELETE FROM segment_start_outbox WHERE task_id = ?1 AND run_segment_id = ?2",
            rusqlite::params![task_id.to_string(), continued_segment_id.to_string()],
        )
        .unwrap();
    assert!(matches!(
        store.pending_segment_start(task_id, continued_segment_id),
        Err(harness_journal::TaskStoreError::ProjectionIntegrity(_))
    ));
}

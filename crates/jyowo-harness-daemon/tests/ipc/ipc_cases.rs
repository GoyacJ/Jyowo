use super::*;

#[test]
fn codec_handles_fragmented_and_coalesced_frames_and_rejects_bad_lengths() {
    let first = encode_frame(&handshake("token-a")).unwrap();
    let second = encode_frame(&frame(
        "subscribe",
        ClientRequest::SubscribeEvents { after_offset: 0 },
    ))
    .unwrap();
    let mut decoder = JsonFrameDecoder::new();
    assert!(decoder.push::<ClientFrame>(&first[..3]).unwrap().is_empty());
    let mut tail = first[3..].to_vec();
    tail.extend_from_slice(&second);
    let decoded = decoder.push::<ClientFrame>(&tail).unwrap();
    assert_eq!(decoded.len(), 2);

    assert!(JsonFrameDecoder::new()
        .push::<ClientFrame>(&[0, 0, 0, 0])
        .is_err());
    let oversized = u32::try_from(MAX_FRAME_BYTES + 1).unwrap().to_be_bytes();
    assert!(JsonFrameDecoder::new()
        .push::<ClientFrame>(&oversized)
        .is_err());
    let invalid_json = [vec![0, 0, 0, 1], vec![b'{']].concat();
    assert!(JsonFrameDecoder::new()
        .push::<ClientFrame>(&invalid_json)
        .is_err());
}

#[test]
fn handshake_rejects_protocol_token_and_instance_mismatches() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let mut connection = IpcConnection::new(Arc::clone(&store), config());
    let mut wrong_version = handshake("token-a");
    wrong_version.protocol_version += 1;
    assert!(matches!(
        connection.handle(wrong_version).unwrap()[0].message,
        ServerMessage::Error(_)
    ));

    let mut connection = IpcConnection::new(Arc::clone(&store), config());
    assert!(matches!(
        connection.handle(handshake("wrong")).unwrap()[0].message,
        ServerMessage::Error(_)
    ));

    let mut connection = IpcConnection::new(store, config());
    let mut wrong_instance = handshake("token-a");
    if let ClientRequest::Handshake(request) = &mut wrong_instance.request {
        request.user_instance_id = "other".into();
    }
    assert!(matches!(
        connection.handle(wrong_instance).unwrap()[0].message,
        ServerMessage::Error(_)
    ));

    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let mut connection = IpcConnection::new(store, config());
    let mut malformed_version = handshake("token-a");
    if let ClientRequest::Handshake(request) = &mut malformed_version.request {
        request.client_version = "0.invalid".into();
    }
    assert!(matches!(
        connection.handle(malformed_version).unwrap()[0].message,
        ServerMessage::Error(_)
    ));

    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let mut connection = IpcConnection::new(store, config());
    let mut future_offset = handshake("token-a");
    if let ClientRequest::Handshake(request) = &mut future_offset.request {
        request.last_acknowledged_offset = 1;
    }
    assert!(matches!(
        connection.handle(future_offset).unwrap()[0].message,
        ServerMessage::Error(_)
    ));
}

#[test]
fn protocol_v3_client_rejects_a_protocol_v1_daemon_connection() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let mut connection = IpcConnection::new(store, config());
    let mut legacy = handshake("token-a");
    legacy.protocol_version = 1;

    let response = connection.handle(legacy).unwrap();

    let ServerMessage::Error(error) = &response[0].message else {
        panic!("protocol v1 handshake must be rejected");
    };
    assert_eq!(
        error.code,
        harness_contracts::ProtocolErrorCode::ProtocolMismatch
    );
}

#[test]
fn handshake_reports_daemon_agent_capabilities() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let mut connection = IpcConnection::new(store, config());

    let response = connection.handle(handshake("token-a")).unwrap();
    let ServerMessage::Handshake(response) = &response[0].message else {
        panic!("expected handshake response");
    };

    assert!(response.agent_capabilities.subagents);
    assert!(response.agent_capabilities.agent_teams);
    assert!(response.agent_capabilities.background_agents);
}

#[test]
fn ipc_rejects_request_ids_larger_than_the_response_envelope_reserve() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let mut connection = IpcConnection::new(store, config());
    let mut request = handshake("token-a");
    request.request_id = "r".repeat(129);

    let response = connection.handle(request).unwrap();

    assert!(matches!(response[0].message, ServerMessage::Error(_)));
    assert!(encode_frame(&response[0]).is_ok());
}

#[test]
fn ipc_rejects_non_printable_ascii_request_ids() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());

    for request_id in ["request-\n1", "请求-1"] {
        let mut connection = IpcConnection::new(Arc::clone(&store), config());
        let mut request = handshake("token-a");
        request.request_id = request_id.into();

        let response = connection.handle(request).unwrap();

        assert!(response[0].request_id.is_none());
        assert!(matches!(response[0].message, ServerMessage::Error(_)));
        assert!(encode_frame(&response[0]).is_ok());
    }
}

#[test]
fn duplicate_commands_are_idempotent_and_clients_observe_identical_offsets() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let mut first = IpcConnection::new(Arc::clone(&store), config());
    let mut second = IpcConnection::new(Arc::clone(&store), config());
    first.handle(handshake("token-a")).unwrap();
    second.handle(handshake("token-a")).unwrap();

    let command_id = CommandId::new();
    let accepted = first
        .handle(create("create-1", command_id, "same-create"))
        .unwrap();
    let replayed = first
        .handle(create("create-2", command_id, "same-create"))
        .unwrap();
    let task_id = match &accepted[0].message {
        ServerMessage::CommandAccepted(accepted) => accepted.task_id,
        other => panic!("unexpected {other:?}"),
    };
    assert!(matches!(
        accepted[0].message,
        ServerMessage::CommandAccepted(_)
    ));
    assert!(matches!(
        replayed[0].message,
        ServerMessage::CommandAccepted(_)
    ));
    assert_eq!(store.latest_global_offset().unwrap(), 1);
    let projection = store.task_projection_snapshot(task_id).unwrap().unwrap().0;
    assert_eq!(
        projection.workspace,
        Some(WorkspaceSelection {
            mode: WorkspaceMode::Current,
            root: "/tmp/workspace".into(),
        })
    );

    let mut conflicting = create("create-3", command_id, "same-create");
    if let ClientRequest::CreateTask(command) = &mut conflicting.request {
        command.title = "different".into();
    }
    assert!(matches!(
        first.handle(conflicting).unwrap()[0].message,
        ServerMessage::CommandRejected(_)
    ));
    assert_eq!(store.latest_global_offset().unwrap(), 1);

    let a = first
        .handle(frame(
            "events-a",
            ClientRequest::SubscribeEvents { after_offset: 0 },
        ))
        .unwrap();
    let b = second
        .handle(frame(
            "events-b",
            ClientRequest::SubscribeEvents { after_offset: 0 },
        ))
        .unwrap();
    let offsets = |frames: Vec<harness_contracts::ServerFrame>| match &frames[0].message {
        ServerMessage::EventBatch(batch) => batch
            .events
            .iter()
            .map(|event| event.global_offset)
            .collect::<Vec<_>>(),
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(offsets(a), offsets(b));

    for index in 0..3 {
        first
            .handle(create(
                &format!("extra-{index}"),
                CommandId::new(),
                &format!("extra-{index}"),
            ))
            .unwrap();
    }
    let gap = first
        .handle(frame(
            "slow",
            ClientRequest::SubscribeEvents { after_offset: 0 },
        ))
        .unwrap();
    assert!(matches!(
        &gap[0].message,
        ServerMessage::EventBatch(batch) if batch.gap && batch.events.is_empty()
    ));
}

#[tokio::test]
async fn authenticated_submit_message_is_dispatched_through_the_task_supervisor() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let factory = Arc::new(ControlledRunFactory::default());
    let supervisor = Arc::new(
        Supervisor::start(
            Arc::clone(&store),
            factory.clone(),
            SupervisorQuotas::new(2, 2),
        )
        .unwrap(),
    );
    let mut connection =
        IpcConnection::with_supervisor(Arc::clone(&store), config(), Arc::clone(&supervisor));
    connection.handle(handshake("token-a")).unwrap();
    let created = connection
        .handle(create("create", CommandId::new(), "create-submit-task"))
        .unwrap();
    let task_id = match &created[0].message {
        ServerMessage::CommandAccepted(accepted) => accepted.task_id,
        other => panic!("unexpected {other:?}"),
    };
    let command_id = CommandId::new();
    let response = connection
        .handle_async(frame(
            "submit",
            ClientRequest::SubmitMessage(harness_contracts::SubmitMessageCommand {
                metadata: CommandMetadata {
                    command_id,
                    idempotency_key: "submit-through-supervisor".into(),
                    expected_stream_version: 1,
                },
                task_id,
                content: "run this task".into(),
                attachments: Vec::new(),
                context_references: Vec::new(),
                model_config_id: Some("provider-config-001".into()),
                permission_mode: harness_contracts::PermissionMode::Auto,
            }),
        ))
        .await
        .unwrap();

    assert!(
        matches!(
            &response[0].message,
            ServerMessage::CommandAccepted(accepted)
                if accepted.command_id == command_id && accepted.task_id == task_id
        ),
        "unexpected response: {response:?}"
    );
    let (projection, _, timeline) = store.task_projection_snapshot(task_id).unwrap().unwrap();
    assert_eq!(projection.state, harness_contracts::TaskState::Running);
    assert!(timeline
        .iter()
        .any(|item| item.kind == harness_contracts::TimelineEventKind::UserMessage));
    let queued = store
        .task_events_after(task_id, 0, 16)
        .unwrap()
        .into_iter()
        .find(|event| event.event_type == "message.queued")
        .expect("message.queued event");
    assert_eq!(queued.payload["modelConfigId"], "provider-config-001");
    assert_eq!(queued.payload["permissionMode"], "auto");
    let starts = factory.starts.lock().unwrap();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].input.content, "run this task");
    assert_eq!(
        starts[0].input.model_config_id.as_deref(),
        Some("provider-config-001")
    );
    assert_eq!(
        starts[0].input.permission_mode,
        harness_contracts::PermissionMode::Auto
    );
    assert_eq!(
        starts[0].input.session_id,
        harness_contracts::SessionId::from_u128(u128::from_be_bytes(task_id.as_bytes()))
    );
    assert_eq!(
        starts[0].input.run_id,
        harness_contracts::RunId::from_u128(u128::from_be_bytes(starts[0].segment_id.as_bytes()))
    );
}

#[tokio::test]
async fn authenticated_continue_task_is_dispatched_through_the_task_supervisor() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let factory = Arc::new(ControlledRunFactory::default());
    let supervisor = Arc::new(
        Supervisor::start(
            Arc::clone(&store),
            factory.clone(),
            SupervisorQuotas::new(2, 2),
        )
        .unwrap(),
    );
    let mut connection =
        IpcConnection::with_supervisor(Arc::clone(&store), config(), Arc::clone(&supervisor));
    connection.handle(handshake("token-a")).unwrap();
    let created = connection
        .handle(create("create", CommandId::new(), "create-continue-task"))
        .unwrap();
    let task_id = match &created[0].message {
        ServerMessage::CommandAccepted(accepted) => accepted.task_id,
        other => panic!("unexpected {other:?}"),
    };
    let interrupted_segment = RunSegmentId::new();
    store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: "start-before-recovery".into(),
                expected_stream_version: 1,
                authority: TaskStore::supervisor_authority(),
                payload: json!({ "type": "test_start" }),
            },
            |_| {
                Ok(vec![NewTaskEvent::run_started(
                    interrupted_segment,
                    chrono::Utc::now(),
                )])
            },
        )
        .unwrap();
    store
        .mark_segment_start_delivered(task_id, interrupted_segment)
        .unwrap();
    RecoveryService::new(Arc::clone(&store))
        .recover_task(task_id)
        .unwrap();
    assert_eq!(
        store
            .task_projection(task_id)
            .unwrap()
            .unwrap()
            .current_run
            .unwrap()
            .terminal_reason,
        Some(RunTerminalReason::InterruptedByRestart)
    );
    let command_id = CommandId::new();

    let response = connection
        .handle_async(frame(
            "continue",
            ClientRequest::ContinueTask(ContinueTaskCommand {
                metadata: CommandMetadata {
                    command_id,
                    idempotency_key: "continue-through-supervisor".into(),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                },
                task_id,
                indeterminate_tools: Vec::new(),
            }),
        ))
        .await
        .unwrap();

    assert!(matches!(
        &response[0].message,
        ServerMessage::CommandAccepted(accepted) if accepted.command_id == command_id
    ));
    let projection = store.task_projection(task_id).unwrap().unwrap();
    let continued = projection.current_run.unwrap();
    assert_eq!(continued.state, RunState::Running);
    assert_ne!(continued.segment_id, interrupted_segment);
    assert_eq!(factory.starts.lock().unwrap().len(), 1);
    assert_eq!(
        factory.starts.lock().unwrap()[0].segment_id,
        continued.segment_id
    );
}

#[tokio::test]
async fn authenticated_stop_run_controls_and_terminates_the_active_segment() {
    for (mode, expected_decision, expected_terminal) in [
        (
            StopMode::SafePoint,
            SafePointDecision::Yield,
            RunTerminalReason::Cancelled,
        ),
        (
            StopMode::Force,
            SafePointDecision::ForceStop,
            RunTerminalReason::ForcedInterruption,
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        let factory = Arc::new(ControlledRunFactory::default());
        let supervisor = Arc::new(
            Supervisor::start(
                Arc::clone(&store),
                factory.clone(),
                SupervisorQuotas::new(2, 2),
            )
            .unwrap(),
        );
        let mut connection =
            IpcConnection::with_supervisor(Arc::clone(&store), config(), supervisor);
        connection.handle(handshake("token-a")).unwrap();
        let created = connection
            .handle(create("create", CommandId::new(), "create-stop-task"))
            .unwrap();
        let task_id = match &created[0].message {
            ServerMessage::CommandAccepted(accepted) => accepted.task_id,
            other => panic!("unexpected {other:?}"),
        };
        connection
            .handle_async(frame(
                "submit",
                ClientRequest::SubmitMessage(harness_contracts::SubmitMessageCommand {
                    metadata: CommandMetadata {
                        command_id: CommandId::new(),
                        idempotency_key: "submit-before-stop".into(),
                        expected_stream_version: 1,
                    },
                    task_id,
                    content: "run until stopped".into(),
                    attachments: Vec::new(),
                    context_references: Vec::new(),
                    model_config_id: None,
                    permission_mode: harness_contracts::PermissionMode::Default,
                }),
            ))
            .await
            .unwrap();
        let segment_id = store
            .task_projection(task_id)
            .unwrap()
            .unwrap()
            .current_run
            .unwrap()
            .segment_id;
        let command_id = CommandId::new();

        let response = connection
            .handle_async(frame(
                "stop",
                ClientRequest::StopRun(StopRunCommand {
                    metadata: CommandMetadata {
                        command_id,
                        idempotency_key: "stop-through-supervisor".into(),
                        expected_stream_version: store.stream_version(task_id).unwrap(),
                    },
                    task_id,
                    mode,
                }),
            ))
            .await
            .unwrap();

        assert!(matches!(
            &response[0].message,
            ServerMessage::CommandAccepted(accepted) if accepted.command_id == command_id
        ));
        assert_eq!(factory.control(segment_id).decision(), expected_decision);
        assert_eq!(
            store
                .task_projection(task_id)
                .unwrap()
                .unwrap()
                .current_run
                .unwrap()
                .state,
            RunState::Yielding
        );
        factory.control(segment_id).finish(match expected_decision {
            SafePointDecision::Yield => TurnOutcome::YieldedAtSafePoint,
            SafePointDecision::ForceStop => TurnOutcome::ForceStopped {
                non_revertible_tool_use_ids: Vec::new(),
            },
            SafePointDecision::Continue => unreachable!(),
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let projection = store.task_projection(task_id).unwrap().unwrap();
                if projection
                    .current_run
                    .as_ref()
                    .and_then(|run| run.terminal_reason.as_ref())
                    == Some(&expected_terminal)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn force_stop_timeout_fails_a_standalone_stop_without_a_promoted_message() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let factory = Arc::new(ControlledRunFactory::default());
    let supervisor = Arc::new(
        Supervisor::start(
            Arc::clone(&store),
            factory.clone(),
            SupervisorQuotas::new(2, 2),
        )
        .unwrap(),
    );
    let mut connection = IpcConnection::with_supervisor(Arc::clone(&store), config(), supervisor);
    connection.handle(handshake("token-a")).unwrap();
    let created = connection
        .handle(create("create", CommandId::new(), "create-timeout-task"))
        .unwrap();
    let task_id = match &created[0].message {
        ServerMessage::CommandAccepted(accepted) => accepted.task_id,
        other => panic!("unexpected {other:?}"),
    };
    connection
        .handle_async(frame(
            "submit",
            ClientRequest::SubmitMessage(harness_contracts::SubmitMessageCommand {
                metadata: CommandMetadata {
                    command_id: CommandId::new(),
                    idempotency_key: "submit-before-timeout".into(),
                    expected_stream_version: 1,
                },
                task_id,
                content: "run until stop times out".into(),
                attachments: Vec::new(),
                context_references: Vec::new(),
                model_config_id: None,
                permission_mode: harness_contracts::PermissionMode::Default,
            }),
        ))
        .await
        .unwrap();
    let segment_id = store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .current_run
        .unwrap()
        .segment_id;
    connection
        .handle_async(frame(
            "stop",
            ClientRequest::StopRun(StopRunCommand {
                metadata: CommandMetadata {
                    command_id: CommandId::new(),
                    idempotency_key: "force-stop-before-timeout".into(),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                },
                task_id,
                mode: StopMode::Force,
            }),
        ))
        .await
        .unwrap();
    let tool_use_id = ToolUseId::new();

    factory
        .control(segment_id)
        .finish(TurnOutcome::ForceStopTimedOut {
            indeterminate_tool_use_ids: vec![tool_use_id],
        });

    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if store.task_projection(task_id).unwrap().unwrap().state == TaskState::Failed {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(store.events_after(0, 32).unwrap().iter().any(|event| {
        event.event_type == "run.force_stop_timed_out"
            && event.payload["indeterminateToolUseIds"][0] == tool_use_id.to_string()
    }));
}

#[tokio::test]
async fn authenticated_permission_decision_is_dispatched_through_the_task_supervisor() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let supervisor = Arc::new(
        Supervisor::start(
            Arc::clone(&store),
            Arc::new(IdleRunFactory),
            SupervisorQuotas::new(2, 2),
        )
        .unwrap(),
    );
    let mut connection =
        IpcConnection::with_supervisor(Arc::clone(&store), config(), Arc::clone(&supervisor));
    let client_a = ClientId::new();
    let client_b = ClientId::new();
    connection
        .handle(handshake_for_client("token-a", client_a))
        .unwrap();
    let created = connection
        .handle(create("create", CommandId::new(), "create-permission-task"))
        .unwrap();
    let task_id = match &created[0].message {
        ServerMessage::CommandAccepted(accepted) => accepted.task_id,
        other => panic!("unexpected {other:?}"),
    };
    connection
        .handle_async(frame(
            "submit-before-permission",
            ClientRequest::SubmitMessage(harness_contracts::SubmitMessageCommand {
                metadata: CommandMetadata {
                    command_id: CommandId::new(),
                    idempotency_key: "submit-before-permission".into(),
                    expected_stream_version: 1,
                },
                task_id,
                content: "run this task".into(),
                attachments: Vec::new(),
                context_references: Vec::new(),
                model_config_id: None,
                permission_mode: harness_contracts::PermissionMode::Default,
            }),
        ))
        .await
        .unwrap();
    let segment_id = store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .current_run
        .unwrap()
        .segment_id;
    let permission_request_id = RequestId::new();
    supervisor
        .permission_broker()
        .request(PermissionRequestDraft {
            task_id,
            segment_id,
            request_id: permission_request_id,
            request_revision: 1,
            expected_task_version: store.stream_version(task_id).unwrap(),
            kind: DaemonPermissionKind::Command,
            action_plan_hash: "plan-v1".into(),
            sandbox_policy_hash: "sandbox-v1".into(),
            workspace: "/tmp/workspace".into(),
            subject: json!({ "command": "cargo test" }),
            actor_source: json!({ "type": "parent_run" }),
            options: vec![PermissionOption {
                option_id: "allow-once".into(),
                label: "Allow once".into(),
            }],
            preview: "cargo test".into(),
            expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
        })
        .unwrap();
    let command_id = CommandId::new();
    let resolve = ClientRequest::ResolvePermission(ResolvePermissionCommand {
        metadata: CommandMetadata {
            command_id,
            idempotency_key: "resolve-permission-through-supervisor".into(),
            expected_stream_version: store.stream_version(task_id).unwrap(),
        },
        task_id,
        permission_request_id,
        request_revision: 1,
        option_id: "allow-once".into(),
    });

    let response = connection
        .handle_async(frame("resolve-permission", resolve.clone()))
        .await
        .unwrap();
    let replay = connection
        .handle_async(frame("resolve-permission-replay", resolve))
        .await
        .unwrap();

    assert!(
        matches!(
            &response[0].message,
            ServerMessage::CommandAccepted(accepted)
                if accepted.command_id == command_id && accepted.task_id == task_id
        ),
        "unexpected response: {response:?}"
    );
    assert!(
        matches!(
            &replay[0].message,
            ServerMessage::CommandAccepted(accepted)
                if accepted.command_id == command_id && accepted.task_id == task_id
        ),
        "unexpected replay: {replay:?}"
    );
    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert!(projection.pending_permission.is_none());
    assert_eq!(
        projection.current_run.unwrap().state,
        harness_contracts::RunState::Running
    );

    let conflicting = connection
        .handle_async(frame(
            "resolve-permission-conflict",
            ClientRequest::ResolvePermission(ResolvePermissionCommand {
                metadata: CommandMetadata {
                    command_id,
                    idempotency_key: "different-key-for-reused-command".into(),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                },
                task_id,
                permission_request_id,
                request_revision: 1,
                option_id: "allow-once".into(),
            }),
        ))
        .await
        .unwrap();
    assert!(
        matches!(
            &conflicting[0].message,
            ServerMessage::CommandRejected(rejected)
                if rejected.command_id == Some(command_id) && rejected.task_id == Some(task_id)
        ),
        "unexpected conflict response: {conflicting:?}"
    );
    assert!(matches!(
        connection
            .handle_async(frame("list-after-conflict", ClientRequest::ListTasks))
            .await
            .unwrap()[0]
            .message,
        ServerMessage::TaskList { .. }
    ));

    let second_permission_request_id = RequestId::new();
    supervisor
        .permission_broker()
        .request(PermissionRequestDraft {
            task_id,
            segment_id,
            request_id: second_permission_request_id,
            request_revision: 1,
            expected_task_version: store.stream_version(task_id).unwrap(),
            kind: DaemonPermissionKind::Command,
            action_plan_hash: "plan-v2".into(),
            sandbox_policy_hash: "sandbox-v1".into(),
            workspace: "/tmp/workspace".into(),
            subject: json!({ "command": "cargo check" }),
            actor_source: json!({ "type": "parent_run" }),
            options: vec![PermissionOption {
                option_id: "allow-once".into(),
                label: "Allow once".into(),
            }],
            preview: "cargo check".into(),
            expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
        })
        .unwrap();
    let mut second_connection =
        IpcConnection::with_supervisor(Arc::clone(&store), config(), Arc::clone(&supervisor));
    second_connection
        .handle(handshake_for_client("token-a", client_b))
        .unwrap();
    let second_command_id = CommandId::new();
    let second_response = second_connection
        .handle_async(frame(
            "resolve-permission-second-client",
            ClientRequest::ResolvePermission(ResolvePermissionCommand {
                metadata: CommandMetadata {
                    command_id: second_command_id,
                    idempotency_key: "resolve-permission-through-supervisor".into(),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                },
                task_id,
                permission_request_id: second_permission_request_id,
                request_revision: 1,
                option_id: "allow-once".into(),
            }),
        ))
        .await
        .unwrap();
    assert!(
        matches!(
            &second_response[0].message,
            ServerMessage::CommandAccepted(accepted)
                if accepted.command_id == second_command_id && accepted.task_id == task_id
        ),
        "unexpected second-client response: {second_response:?}"
    );
}

#[tokio::test]
async fn staged_blob_is_owned_by_the_target_task_and_can_be_submitted() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let supervisor = Arc::new(
        Supervisor::start(
            Arc::clone(&store),
            Arc::new(IdleRunFactory),
            SupervisorQuotas::new(2, 2),
        )
        .unwrap(),
    );
    let mut ipc_config = config();
    ipc_config.blob_root = root.path().join("blobs");
    let mut connection = IpcConnection::with_supervisor(Arc::clone(&store), ipc_config, supervisor);
    connection.handle(handshake("token-a")).unwrap();
    let created = connection
        .handle(create("create", CommandId::new(), "create-attachment-task"))
        .unwrap();
    let task_id = match &created[0].message {
        ServerMessage::CommandAccepted(accepted) => accepted.task_id,
        other => panic!("unexpected {other:?}"),
    };

    let staged = connection
        .handle(frame(
            "stage",
            ClientRequest::StageBlob(harness_contracts::StageBlobCommand {
                task_id,
                media_type: "text/plain".into(),
                base64_data: "bm90ZXM=".into(),
            }),
        ))
        .unwrap();
    let blob_id = match &staged[0].message {
        ServerMessage::Blob(blob) => blob.blob_id,
        other => panic!("unexpected {other:?}"),
    };
    let response = connection
        .handle_async(frame(
            "submit-attachment",
            ClientRequest::SubmitMessage(harness_contracts::SubmitMessageCommand {
                metadata: CommandMetadata {
                    command_id: CommandId::new(),
                    idempotency_key: "submit-staged-attachment".into(),
                    expected_stream_version: 1,
                },
                task_id,
                content: "inspect notes".into(),
                attachments: vec![blob_id],
                context_references: Vec::new(),
                model_config_id: None,
                permission_mode: harness_contracts::PermissionMode::Default,
            }),
        ))
        .await
        .unwrap();

    assert!(matches!(
        response[0].message,
        ServerMessage::CommandAccepted(_)
    ));
}

#[test]
fn task_snapshot_uses_the_global_cursor_even_when_other_tasks_advanced_it() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let mut connection = IpcConnection::new(store, config());
    connection.handle(handshake("token-a")).unwrap();

    let first = connection
        .handle(create("create-a", CommandId::new(), "create-a"))
        .unwrap();
    let task_id = match &first[0].message {
        ServerMessage::CommandAccepted(accepted) => accepted.task_id,
        other => panic!("unexpected {other:?}"),
    };
    connection
        .handle(create("create-b", CommandId::new(), "create-b"))
        .unwrap();

    let loaded = connection
        .handle(frame("load-a", ClientRequest::LoadTask { task_id }))
        .unwrap();
    match &loaded[0].message {
        ServerMessage::TaskSnapshot(snapshot) => {
            assert_eq!(snapshot.projection.last_global_offset, 1);
            assert_eq!(snapshot.snapshot_offset, 2);
            assert_eq!(
                snapshot
                    .timeline
                    .iter()
                    .map(|item| item.global_offset)
                    .collect::<Vec<_>>(),
                vec![1]
            );
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn task_snapshot_uses_the_real_journal_projector_and_audit_pages_preserve_raw_engine_history()
{
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let mut connection = IpcConnection::new(Arc::clone(&store), config());
    connection.handle(handshake("token-a")).unwrap();
    let created = connection
        .handle(create("create-engine", CommandId::new(), "create-engine"))
        .unwrap();
    let task_id = match &created[0].message {
        ServerMessage::CommandAccepted(accepted) => accepted.task_id,
        other => panic!("unexpected {other:?}"),
    };
    let segment_id = RunSegmentId::new();
    store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: "start-engine-projection".into(),
                expected_stream_version: 1,
                authority: TaskStore::supervisor_authority(),
                payload: json!({ "type": "test_start" }),
            },
            |_| Ok(vec![NewTaskEvent::run_started(segment_id, now())]),
        )
        .unwrap();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let message_id = MessageId::new();
    let adapter = TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    )
    .with_run_segment_id(segment_id);
    adapter
        .append_with_metadata(
            TenantId::SINGLE,
            session_id,
            AppendMetadata {
                run_id: Some(run_id),
                ..AppendMetadata::default()
            },
            &[
                Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                    run_id,
                    message_id,
                    delta: DeltaChunk::Text("Real ".into()),
                    at: now(),
                }),
                Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
                    run_id,
                    message_id,
                    content: MessageContent::Text("Real projector".into()),
                    tool_uses: Vec::new(),
                    usage: UsageSnapshot::default(),
                    pricing_snapshot_id: None,
                    stop_reason: StopReason::EndTurn,
                    at: now(),
                }),
                Event::RunEnded(RunEndedEvent {
                    run_id,
                    reason: EndReason::Completed,
                    usage: Some(UsageSnapshot::default()),
                    ended_at: now(),
                }),
            ],
        )
        .await
        .unwrap();

    let loaded = connection
        .handle(frame("load-engine", ClientRequest::LoadTask { task_id }))
        .unwrap();
    let ServerMessage::TaskSnapshot(snapshot) = &loaded[0].message else {
        panic!("unexpected {:?}", loaded[0].message);
    };
    let assistant = snapshot
        .timeline
        .iter()
        .filter(|item| item.kind == harness_contracts::TimelineEventKind::AssistantText)
        .collect::<Vec<_>>();
    assert_eq!(assistant.len(), 1);
    assert_eq!(assistant[0].summary, "Real ");
    assert_eq!(assistant[0].run_segment_id, Some(segment_id));
    assert!(!assistant[0].incomplete);
    assert!(!snapshot
        .timeline
        .iter()
        .any(|item| item.summary == "Run ended"));

    let audit = connection
        .handle(frame(
            "load-engine-events",
            ClientRequest::LoadTaskEvents {
                task_id,
                before_global_offset: None,
                limit: 16,
            },
        ))
        .unwrap();
    let ServerMessage::TaskEventPage(page) = &audit[0].message else {
        panic!("unexpected {:?}", audit[0].message);
    };
    assert_eq!(page.task_id, task_id);
    assert!(page
        .events
        .iter()
        .any(|event| event.event_type == "engine.run_ended"));
}

#[tokio::test]
async fn task_metadata_commands_update_projection_and_hide_removed_tasks() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let supervisor = Arc::new(
        Supervisor::start(
            Arc::clone(&store),
            Arc::new(IdleRunFactory),
            SupervisorQuotas::new(2, 2),
        )
        .unwrap(),
    );
    let mut connection = IpcConnection::with_supervisor(Arc::clone(&store), config(), supervisor);
    connection.handle(handshake("token-a")).unwrap();
    let created = connection
        .handle(create(
            "create-metadata",
            CommandId::new(),
            "create-metadata",
        ))
        .unwrap();
    let task_id = match &created[0].message {
        ServerMessage::CommandAccepted(accepted) => accepted.task_id,
        other => panic!("unexpected {other:?}"),
    };

    let remove_request = ClientRequest::RemoveTask(harness_contracts::RemoveTaskCommand {
        metadata: CommandMetadata {
            command_id: CommandId::new(),
            idempotency_key: "remove-task".into(),
            expected_stream_version: 4,
        },
        task_id,
    });
    let requests = [
        ClientRequest::RenameTask(harness_contracts::RenameTaskCommand {
            metadata: CommandMetadata {
                command_id: CommandId::new(),
                idempotency_key: "rename-task".into(),
                expected_stream_version: 1,
            },
            task_id,
            title: "  Renamed task  ".into(),
        }),
        ClientRequest::SetTaskPinned(harness_contracts::SetTaskPinnedCommand {
            metadata: CommandMetadata {
                command_id: CommandId::new(),
                idempotency_key: "pin-task".into(),
                expected_stream_version: 2,
            },
            task_id,
            pinned: true,
        }),
        ClientRequest::SetTaskArchived(harness_contracts::SetTaskArchivedCommand {
            metadata: CommandMetadata {
                command_id: CommandId::new(),
                idempotency_key: "archive-task".into(),
                expected_stream_version: 3,
            },
            task_id,
            archived: true,
        }),
        remove_request.clone(),
    ];

    for (index, request) in requests.into_iter().enumerate() {
        let response = connection
            .handle_async(frame(&format!("metadata-{index}"), request))
            .await
            .unwrap();
        assert!(
            matches!(response[0].message, ServerMessage::CommandAccepted(_)),
            "unexpected response: {response:?}"
        );
    }

    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(projection.title, "Renamed task");
    assert!(projection.pinned);
    assert!(projection.archived);
    assert!(projection.removed);

    let listed = connection
        .handle(frame("list-after-remove", ClientRequest::ListTasks))
        .unwrap();
    assert!(matches!(
        &listed[0].message,
        ServerMessage::TaskList { tasks } if tasks.iter().all(|task| task.task_id != task_id)
    ));
    let loaded = connection
        .handle(frame(
            "load-after-remove",
            ClientRequest::LoadTask { task_id },
        ))
        .unwrap();
    assert!(matches!(
        &loaded[0].message,
        ServerMessage::Error(error) if error.code == harness_contracts::ProtocolErrorCode::NotFound
    ));

    let hidden_rename = connection
        .handle_async(frame(
            "rename-after-remove",
            ClientRequest::RenameTask(harness_contracts::RenameTaskCommand {
                metadata: CommandMetadata {
                    command_id: CommandId::new(),
                    idempotency_key: "rename-after-remove".into(),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                },
                task_id,
                title: "Must stay hidden".into(),
            }),
        ))
        .await
        .unwrap();
    assert!(matches!(
        &hidden_rename[0].message,
        ServerMessage::CommandRejected(rejected)
            if rejected.reason == CommandRejectionReason::InvalidCommand
    ));
    assert_eq!(
        store.task_projection(task_id).unwrap().unwrap().title,
        "Renamed task"
    );

    let hidden_submit = connection
        .handle_async(frame(
            "submit-after-remove",
            ClientRequest::SubmitMessage(harness_contracts::SubmitMessageCommand {
                metadata: CommandMetadata {
                    command_id: CommandId::new(),
                    idempotency_key: "submit-after-remove".into(),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                },
                task_id,
                content: "must stay removed".into(),
                attachments: Vec::new(),
                context_references: Vec::new(),
                model_config_id: None,
                permission_mode: harness_contracts::PermissionMode::Default,
            }),
        ))
        .await
        .unwrap();
    assert!(matches!(
        &hidden_submit[0].message,
        ServerMessage::CommandRejected(rejected)
            if rejected.reason == CommandRejectionReason::InvalidCommand
    ));
    assert!(store
        .nonterminal_workspace_leases_for_task(task_id)
        .unwrap()
        .is_empty());

    let replayed_remove = connection
        .handle_async(frame("replay-remove", remove_request))
        .await
        .unwrap();
    assert!(matches!(
        &replayed_remove[0].message,
        ServerMessage::CommandAccepted(accepted) if accepted.stream_version == 5
    ));
}
#[tokio::test]
async fn task_event_pages_with_large_legal_payloads_fit_daemon_frames_without_skipping_events() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let mut connection = IpcConnection::new(Arc::clone(&store), config());
    connection.handle(handshake("token-a")).unwrap();
    let created = connection
        .handle(create(
            "create-large-audit",
            CommandId::new(),
            "create-large-audit",
        ))
        .unwrap();
    let task_id = match &created[0].message {
        ServerMessage::CommandAccepted(accepted) => accepted.task_id,
        other => panic!("unexpected {other:?}"),
    };
    let segment_id = RunSegmentId::new();
    store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: "start-large-audit".into(),
                expected_stream_version: 1,
                authority: TaskStore::supervisor_authority(),
                payload: json!({ "type": "test_start" }),
            },
            |_| Ok(vec![NewTaskEvent::run_started(segment_id, now())]),
        )
        .unwrap();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let adapter = TaskEventStoreAdapter::new(
        Arc::clone(&store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    )
    .with_run_segment_id(segment_id);
    for _ in 0..9 {
        adapter
            .append_with_metadata(
                TenantId::SINGLE,
                session_id,
                AppendMetadata {
                    run_id: Some(run_id),
                    ..AppendMetadata::default()
                },
                &[Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                    run_id,
                    message_id: MessageId::new(),
                    delta: DeltaChunk::Text("x".repeat(1_000_000)),
                    at: now(),
                })],
            )
            .await
            .unwrap();
    }

    let expected_offsets = store
        .task_events_after(task_id, 0, usize::MAX)
        .unwrap()
        .into_iter()
        .map(|event| event.global_offset)
        .collect::<Vec<_>>();
    assert_eq!(expected_offsets.len(), 11);
    let mut loaded_offsets = Vec::new();
    let mut before_global_offset = None;
    loop {
        let response = connection
            .handle(frame(
                &"r".repeat(128),
                ClientRequest::LoadTaskEvents {
                    task_id,
                    before_global_offset,
                    limit: 16,
                },
            ))
            .unwrap();
        let encoded = encode_frame(&response[0]).expect("audit page must fit one daemon frame");
        assert!(encoded.len() <= MAX_FRAME_BYTES + 4);
        let ServerMessage::TaskEventPage(page) = &response[0].message else {
            panic!("unexpected {:?}", response[0].message);
        };
        assert!(page.events.len() <= 16);
        assert!(page.events.iter().all(|event| event.task_id == task_id));
        loaded_offsets.extend(page.events.iter().map(|event| event.global_offset));
        let Some(next_before_offset) = page.next_before_offset else {
            break;
        };
        before_global_offset = Some(next_before_offset);
    }
    loaded_offsets.sort_unstable();
    assert_eq!(loaded_offsets, expected_offsets);
}

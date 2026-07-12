//! Task 19 fault-injection matrix.
//!
//! Most entries execute here through public daemon APIs. Managed-worktree cleanup has a dedicated
//! real-git fixture because combining that process-heavy setup with daemon actor faults obscures
//! both failures. Its auditable gate is:
//!
//! `cargo test -p jyowo-harness-daemon --test workspace_coordinator workspace_lease_cases::dirty_managed_worktree_retains_patch_and_emits_cleanup_blocked -- --exact`
//!
//! Provider retry uses a real SDK coordinator and HTTP provider. The mock server controls only the
//! upstream HTTP responses so the test can count model requests without replacing daemon runtime
//! behavior.

use std::collections::{HashMap, VecDeque};
use std::process::Command;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use chrono::Utc;
use harness_contracts::{
    ClientFrame, ClientId, ClientRequest, CommandId, DeferPolicy, Event, EventId, HandshakeRequest,
    ModelProtocol, NoopRedactor, PermissionMode, ProviderProfileConversationCapability,
    ProviderProfileDefinition, ProviderProfileModelDescriptor, ProviderProfileModelLifecycle,
    ProviderSecretEntry, ProviderSecretsRecord, ProviderSelectionRecord, QueueItemId, RunId,
    RunSegmentId, ServerMessage, SessionId, StopMode, TaskEventEnvelope, TaskId, TaskState,
    TenantId, ToolProperties, ToolResult, ToolUseCompletedEvent, ToolUseId, ToolUseRequestedEvent,
    ToolUseStartedEvent, WorkspaceMode, PROTOCOL_VERSION,
};
use harness_daemon::{
    encode_frame, DaemonActivity, IpcServerConfig, LocalIpcServer, PermissionBroker,
    RecoveryService, RunCoordinatorEvent, RunCoordinatorFactory, RunningSegment,
    RuntimeConfigResolver, SdkRunCoordinatorFactory, StartSegmentRequest, Supervisor,
    SupervisorQuotas, ValidatedTaskCommand, WorkspaceAccess, WorkspaceAcquireOutcome,
    WorkspaceCoordinator, WorkspaceExecutionKind, WorkspaceLeaseRequest,
    WorkspaceSubagentRunnerFactory,
};
use harness_engine::{RunControlHandle, TurnOutcome};
use harness_journal::{
    AcceptedCommand, CommandOutcome, EventStore, NewTaskEvent, TaskEventStoreAdapter, TaskStore,
};
use serde_json::json;
use tokio::sync::mpsc;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, Request, ResponseTemplate,
};

const CRASH_TASK_ID: u128 = 0xfeed_0000_0000_0000_0000_0000_0000_0001;
const CRASH_COMMAND_ID: u128 = 0xfeed_0000_0000_0000_0000_0000_0000_0002;

#[test]
fn production_binary_assembles_the_sdk_factory_and_real_subagent_runner() {
    let source = include_str!("../src/bin/jyowo-harness-daemon.rs");

    assert!(!source.contains("UnavailableRunFactory"));
    assert!(!source.contains("NoopRedactor"));
    assert!(source.contains("DefaultRedactor::default()"));
    assert!(source.contains("SdkRunCoordinatorFactory::new_with_subagent_engines"));
    assert!(source.contains("SdkWorkspaceSubagentRunnerFactory::new"));
    assert!(source.contains("Supervisor::start_with_runtime_components"));
    assert!(source.contains("JYOWO_CONFIG_DIR"));
}

#[test]
fn process_death_before_and_after_sqlite_commit_preserves_only_committed_state() {
    for (mode, expected_offset) in [("before", 0), ("after", 1)] {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("tasks.sqlite");
        let output = Command::new(std::env::current_exe().unwrap())
            .arg("--ignored")
            .arg("--exact")
            .arg("sqlite_commit_crash_child")
            .arg("--nocapture")
            .env("JYOWO_FAULT_CHILD", mode)
            .env("JYOWO_FAULT_DATABASE", &database)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "the {mode}-commit child must be terminated abruptly"
        );

        let reopened = TaskStore::open(&database).unwrap();
        assert_eq!(reopened.latest_global_offset().unwrap(), expected_offset);
        assert_eq!(
            reopened
                .task_projection(TaskId::from_u128(CRASH_TASK_ID))
                .unwrap()
                .is_some(),
            mode == "after"
        );
    }
}

#[test]
fn projection_failure_rolls_back_event_command_and_projection_together() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("tasks.sqlite");
    let store = TaskStore::open(&database).unwrap();
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER inject_task_projection_failure
             BEFORE INSERT ON task_projection
             BEGIN
               SELECT RAISE(ABORT, 'injected projection failure');
             END;",
        )
        .unwrap();
    let task_id = TaskId::new();

    let result = store.transact_command(create_task_command(task_id), |_| {
        Ok(vec![NewTaskEvent::task_created("must roll back")])
    });

    assert!(result.is_err());
    assert_eq!(store.latest_global_offset().unwrap(), 0);
    assert!(store.task_projection(task_id).unwrap().is_none());
    drop(store);

    let connection = rusqlite::Connection::open(&database).unwrap();
    let inbox_rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM command_inbox", [], |row| row.get(0))
        .unwrap();
    assert_eq!(inbox_rows, 0);
}

#[test]
fn blob_commit_rename_failure_preserves_destination_and_removes_temp_file() {
    let root = tempfile::tempdir().unwrap();
    let blob_parent = root.path().join("blobs").join("aa");
    std::fs::create_dir_all(&blob_parent).unwrap();
    let destination = blob_parent.join("content-addressed.blob");
    std::fs::create_dir(&destination).unwrap();

    let result = harness_fs::write_bytes_file_atomic(&destination, b"blob body", true);

    assert!(
        result.is_err(),
        "renaming a file over a directory must fail"
    );
    assert!(
        destination.is_dir(),
        "failed commit must preserve destination"
    );
    let leftovers = std::fs::read_dir(&blob_parent)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().ends_with(".tmp"))
        .collect::<Vec<_>>();
    assert!(
        leftovers.is_empty(),
        "failed commit leaked temp files: {leftovers:?}"
    );
}

#[test]
fn client_disconnect_does_not_cancel_active_task_or_background_work() {
    let started = Instant::now();
    let mut activity = DaemonActivity::new(started);
    activity.client_connected();
    activity.task_started();
    activity.background_process_started();

    activity.client_disconnected(started + Duration::from_secs(1));

    assert_eq!(activity.active_tasks(), 1);
    assert!(!activity.should_shutdown(started + Duration::from_secs(600), Duration::from_secs(300)));
}

#[cfg(unix)]
#[tokio::test]
async fn slow_subscriber_receives_gap_instead_of_blocking_commits() {
    use tokio::net::UnixStream;

    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let socket = root.path().join("daemon.sock");
    let server = LocalIpcServer::bind_unix(
        &socket,
        Arc::clone(&store),
        ipc_config(root.path().join("blobs"), 2),
    )
    .await
    .unwrap();
    let mut subscriber = UnixStream::connect(&socket).await.unwrap();
    send_frame(&mut subscriber, &handshake_frame()).await;
    receive_frame(&mut subscriber).await;
    send_frame(
        &mut subscriber,
        &client_frame(
            "subscribe",
            ClientRequest::SubscribeEvents { after_offset: 0 },
        ),
    )
    .await;
    receive_frame(&mut subscriber).await;

    let task_id = TaskId::new();
    store
        .transact_command(create_task_command(task_id), |_| {
            Ok(vec![
                NewTaskEvent::task_created("slow subscriber"),
                NewTaskEvent::task_title_changed("second event"),
                NewTaskEvent::task_title_changed("third event"),
            ])
        })
        .unwrap();

    let pushed = tokio::time::timeout(Duration::from_secs(2), receive_frame(&mut subscriber))
        .await
        .expect("slow subscriber must be told to reload after exceeding capacity");
    assert!(matches!(
        pushed.message,
        ServerMessage::EventBatch(batch)
            if batch.gap && batch.events.is_empty() && batch.latest_offset == 3
    ));
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn provider_disconnect_fails_only_its_task_and_releases_the_global_slot() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let disconnected = create_task(&store, "provider disconnect");
    let survivor = create_task(&store, "survivor");
    let factory = Arc::new(FaultRunFactory::new([
        FactoryBehavior::Disconnect,
        FactoryBehavior::Hold,
    ]));
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();

    assert!(accepted(
        supervisor
            .dispatch(
                disconnected,
                start_command(&store, disconnected, RunSegmentId::new())
            )
            .await
            .unwrap()
    ));
    wait_for_state(&store, disconnected, TaskState::Failed).await;

    assert!(accepted(
        supervisor
            .dispatch(
                survivor,
                start_command(&store, survivor, RunSegmentId::new())
            )
            .await
            .unwrap()
    ));
    wait_for_state(&store, survivor, TaskState::Running).await;
    assert_eq!(factory.start_count(), 2);
}

#[tokio::test]
async fn unresponsive_tool_force_stop_becomes_indeterminate_and_releases_the_slot() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = create_task(&store, "unresponsive tool");
    let factory = Arc::new(FaultRunFactory::new([FactoryBehavior::Hold]));
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(1, 1),
    )
    .unwrap();
    let segment_id = RunSegmentId::new();
    assert!(accepted(
        supervisor
            .dispatch(task_id, start_command(&store, task_id, segment_id))
            .await
            .unwrap()
    ));
    let tool_use_id = ToolUseId::new();
    record_started_tool(&store, task_id, tool_use_id).await;
    assert!(accepted(
        supervisor
            .dispatch(task_id, stop_command(&store, task_id))
            .await
            .unwrap()
    ));
    factory
        .control(segment_id)
        .finish(TurnOutcome::ForceStopTimedOut {
            indeterminate_tool_use_ids: vec![tool_use_id],
        });

    wait_for_state(&store, task_id, TaskState::Failed).await;
    wait_for_checkpoint_tools(&store, task_id, &[tool_use_id]).await;
    let events = store.events_after(0, 100).unwrap();
    assert!(events.iter().any(|event| {
        event.event_type == "run.force_stop_timed_out"
            && event.payload["indeterminateToolUseIds"][0] == tool_use_id.to_string()
    }));
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
async fn actor_panic_retries_the_durable_start_without_stopping_another_task() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let panicking = create_task(&store, "panicking actor");
    let survivor = create_task(&store, "survivor");
    let factory = Arc::new(FaultRunFactory::new([
        FactoryBehavior::Panic,
        FactoryBehavior::Hold,
        FactoryBehavior::Hold,
    ]));
    let supervisor = Supervisor::start(
        Arc::clone(&store),
        factory.clone(),
        SupervisorQuotas::new(2, 1),
    )
    .unwrap();

    assert!(accepted(
        supervisor
            .dispatch(
                panicking,
                start_command(&store, panicking, RunSegmentId::new())
            )
            .await
            .unwrap()
    ));
    wait_for_state(&store, panicking, TaskState::Running).await;
    assert!(accepted(
        supervisor
            .dispatch(
                survivor,
                start_command(&store, survivor, RunSegmentId::new())
            )
            .await
            .unwrap()
    ));
    wait_for_state(&store, survivor, TaskState::Running).await;
    wait_for_start_count(&factory, 3).await;
    assert_eq!(factory.start_count(), 3);
}

#[tokio::test]
async fn repeated_recovery_does_not_replay_completed_or_indeterminate_tools() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    append_events(
        &store,
        task_id,
        vec![
            NewTaskEvent::task_created("tool recovery"),
            NewTaskEvent::run_started(segment_id, Utc::now()),
        ],
    );
    store
        .mark_segment_start_delivered(task_id, segment_id)
        .unwrap();
    let completed = ToolUseId::new();
    let indeterminate = ToolUseId::new();
    let session_id = SessionId::new();
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
            tool_requested(completed),
            Event::ToolUseStarted(ToolUseStartedEvent {
                run_id: RunId::new(),
                tool_use_id: completed,
                at: Utc::now(),
            }),
            Event::ToolUseCompleted(ToolUseCompletedEvent {
                tool_use_id: completed,
                result: ToolResult::Text("persisted".into()),
                usage: None,
                duration_ms: 1,
                at: Utc::now(),
            }),
            tool_requested(indeterminate),
            Event::ToolUseStarted(ToolUseStartedEvent {
                run_id: RunId::new(),
                tool_use_id: indeterminate,
                at: Utc::now(),
            }),
        ],
    )
    .await
    .unwrap();

    let recovery = RecoveryService::new(Arc::clone(&store));
    let first = recovery.recover_startup().unwrap();
    let offset_after_first = store.latest_global_offset().unwrap();
    let second = recovery.recover_startup().unwrap();

    assert_eq!(
        first.recovered_tasks[0].indeterminate_tool_use_ids,
        vec![indeterminate]
    );
    assert!(second.recovered_tasks.is_empty());
    assert_eq!(store.latest_global_offset().unwrap(), offset_after_first);
    let events = store.events_after(0, 100).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "engine.tool_use_completed")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "tool.indeterminate")
            .count(),
        1
    );
}

#[tokio::test]
async fn provider_retry_repeats_only_the_model_request_after_a_completed_tool() {
    let server = MockServer::start().await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen_attempts = Arc::clone(&attempts);
    let tool_use_id = ToolUseId::new();
    let tool_response = format!(
        concat!(
            "data: {{\"id\":\"chatcmpl-tool\",\"choices\":[{{\"index\":0,",
            "\"delta\":{{\"tool_calls\":[{{\"index\":0,\"id\":\"{}\",",
            "\"type\":\"function\",\"function\":{{\"name\":\"FileRead\",",
            "\"arguments\":\"{{\\\"path\\\":\\\"hello.txt\\\"}}\"}}}}]}},",
            "\"finish_reason\":\"tool_calls\"}}],",
            "\"usage\":{{\"prompt_tokens\":1,\"completion_tokens\":1}}}}\n\n",
            "data: [DONE]\n\n"
        ),
        tool_use_id
    );
    let final_response = concat!(
        "data: {\"id\":\"chatcmpl-final\",\"choices\":[{\"index\":0,",
        "\"delta\":{\"content\":\"done\"},\"finish_reason\":\"stop\"}],",
        "\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_owned();
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(move |_request: &Request| {
            match seen_attempts.fetch_add(1, Ordering::SeqCst) {
                0 => ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(tool_response.clone(), "text/event-stream"),
                1 => ResponseTemplate::new(429)
                    .insert_header("retry-after", "0")
                    .set_body_json(json!({ "error": { "message": "rate limited" } })),
                _ => ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(final_response.clone(), "text/event-stream"),
            }
        })
        .mount(&server)
        .await;

    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(workspace.join("hello.txt"), "hello from the workspace").unwrap();
    let config = root.path().join("config");
    std::fs::create_dir(&config).unwrap();
    write_json(
        &config.join("provider-profiles.json"),
        &[openai_chat_profile(&server)],
    );
    write_json(
        &config.join("provider-secrets.json"),
        &ProviderSecretsRecord {
            entries: vec![ProviderSecretEntry {
                config_id: "retry-provider".into(),
                api_key: "test-key".into(),
                official_quota_api_key: None,
            }],
        },
    );
    write_json(
        &config.join("provider-selection.json"),
        &ProviderSelectionRecord {
            default_config_id: Some("retry-provider".into()),
        },
    );

    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = create_task(&store, "provider retry");
    let segment_id = RunSegmentId::new();
    let actor_id = store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .actor_id
        .unwrap();
    let coordinator = Arc::new(
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap(),
    );
    let _lease = match coordinator
        .acquire(WorkspaceLeaseRequest {
            task_id,
            actor_id,
            root: workspace,
            mode: Some(WorkspaceMode::Current),
            access: WorkspaceAccess::Write,
            execution_kind: WorkspaceExecutionKind::Foreground,
            expires_at: None,
        })
        .unwrap()
    {
        WorkspaceAcquireOutcome::Acquired(lease) => lease,
        WorkspaceAcquireOutcome::Waiting(_) => panic!("fixture lease must be active"),
    };
    let redactor = Arc::new(NoopRedactor);
    let permissions = Arc::new(PermissionBroker::new(Arc::clone(&store), redactor.clone()));
    let factory = Arc::new(SdkRunCoordinatorFactory::new(
        Arc::clone(&store),
        RuntimeConfigResolver::new(config),
        root.path().join("blobs"),
        Arc::clone(&permissions),
        Arc::clone(&redactor) as Arc<dyn harness_contracts::Redactor>,
    ));
    let runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory> =
        Arc::new(|_context: harness_daemon::WorkspaceSubagentRunContext| {
            Err(harness_subagent::SubagentError::Engine("unused".into()))
        });
    let supervisor = Supervisor::start_with_runtime_components(
        Arc::clone(&store),
        factory,
        SupervisorQuotas::new(1, 1),
        runner_factory,
        redactor,
        4,
        permissions,
    )
    .unwrap();
    let outcome = supervisor
        .dispatch(
            task_id,
            ValidatedTaskCommand::SubmitMessage {
                command: task_command(
                    task_id,
                    store.stream_version(task_id).unwrap(),
                    json!({ "type": "submit_message" }),
                ),
                queue_item_id: QueueItemId::new(),
                segment_id,
                content: "read hello.txt".into(),
                attachments: Vec::new(),
                context_references: Vec::new(),
                model_config_id: Some("retry-provider".into()),
                permission_mode: PermissionMode::BypassPermissions,
                submitted_at: Utc::now(),
            },
        )
        .await
        .unwrap();
    assert!(accepted(outcome));
    let completed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if store
                .task_projection(task_id)
                .unwrap()
                .is_some_and(|task| task.state == TaskState::Completed)
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    if completed.is_err() {
        panic!(
            "provider retry run did not complete; attempts={}; projection={:?}; events={:?}",
            attempts.load(Ordering::SeqCst),
            store.task_projection(task_id).unwrap(),
            all_task_events(&store, task_id)
                .into_iter()
                .map(|event| (event.event_type, event.payload))
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    let request_bodies = requests
        .iter()
        .map(|request| request.body_json::<serde_json::Value>().unwrap())
        .collect::<Vec<_>>();
    let request_summaries = request_bodies
        .iter()
        .map(|body| {
            let messages = body["messages"].as_array().cloned().unwrap_or_default();
            json!({
                "relay_logical_call_key": body.get("relay_logical_call_key"),
                "roles": messages
                    .iter()
                    .filter_map(|message| message["role"].as_str())
                    .collect::<Vec<_>>(),
                "assistant_has_tool_calls": messages.iter().any(|message| {
                    message["role"] == "assistant"
                        && message["tool_calls"]
                            .as_array()
                            .is_some_and(|calls| !calls.is_empty())
                }),
                "tool_messages": messages
                    .iter()
                    .filter(|message| message["role"] == "tool")
                    .map(|message| message["content"].clone())
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let request_bodies_equal = request_bodies
        .windows(2)
        .map(|pair| pair[0] == pair[1])
        .collect::<Vec<_>>();
    assert_ne!(request_bodies[0], request_bodies[1]);
    assert_eq!(request_bodies[1], request_bodies[2]);
    assert_eq!(request_summaries[1]["assistant_has_tool_calls"], true);
    assert_eq!(
        request_summaries[1]["roles"],
        json!(["system", "user", "assistant", "tool"])
    );
    assert_eq!(
        request_summaries[1]["tool_messages"],
        json!(["hello from the workspace\n"])
    );
    let persisted = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let events = all_task_events(&store, task_id);
            if events
                .iter()
                .any(|event| event.event_type == "engine.tool_use_completed")
            {
                return events;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "completed tool event must become durable; request_summaries={request_summaries:?}; request_bodies_equal={request_bodies_equal:?}; events={:?}",
            all_task_events(&store, task_id)
                .into_iter()
                .map(|event| (event.event_type, event.payload))
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(
        persisted
            .iter()
            .filter(|event| event.event_type == "engine.tool_use_completed")
            .count(),
        1,
        "persisted events: {:?}",
        persisted
            .iter()
            .map(|event| (&event.event_type, &event.payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        persisted
            .iter()
            .filter(|event| event.event_type == "tool.indeterminate")
            .count(),
        0
    );
}

#[test]
fn workspace_cleanup_failure_has_a_dedicated_executable_gate() {
    const SOURCE: &str = include_str!("workspace_coordinator/workspace_lease_cases.rs");
    const TEST: &str = "fn dirty_managed_worktree_retains_patch_and_emits_cleanup_blocked()";
    const COMMAND: &str = "cargo test -p jyowo-harness-daemon --test workspace_coordinator workspace_lease_cases::dirty_managed_worktree_retains_patch_and_emits_cleanup_blocked -- --exact";

    assert!(SOURCE.contains(TEST), "missing matrix gate: {COMMAND}");
}

#[derive(Clone, Copy)]
enum FactoryBehavior {
    Disconnect,
    Panic,
    Hold,
}

#[derive(Clone)]
struct FaultRunFactory {
    state: Arc<Mutex<FaultRunFactoryState>>,
}

struct FaultRunFactoryState {
    behaviors: VecDeque<FactoryBehavior>,
    starts: usize,
    controls: HashMap<RunSegmentId, RunControlHandle>,
    event_senders: HashMap<RunSegmentId, mpsc::UnboundedSender<RunCoordinatorEvent>>,
}

impl FaultRunFactory {
    fn new(behaviors: impl IntoIterator<Item = FactoryBehavior>) -> Self {
        Self {
            state: Arc::new(Mutex::new(FaultRunFactoryState {
                behaviors: behaviors.into_iter().collect(),
                starts: 0,
                controls: HashMap::new(),
                event_senders: HashMap::new(),
            })),
        }
    }

    fn start_count(&self) -> usize {
        self.state.lock().unwrap().starts
    }

    fn control(&self, segment_id: RunSegmentId) -> RunControlHandle {
        self.state
            .lock()
            .unwrap()
            .controls
            .get(&segment_id)
            .expect("held segment has a control handle")
            .clone()
    }
}

impl RunCoordinatorFactory for FaultRunFactory {
    fn spawn_idempotent(
        &self,
        request: StartSegmentRequest,
        _workspace_tools: harness_daemon::WorkspaceToolDispatcher,
        _subagent_runner: Arc<dyn harness_subagent::SubagentRunner>,
        _agent_starters: harness_daemon::AgentStarterCapabilities,
    ) -> RunningSegment {
        let behavior = {
            let mut state = self.state.lock().unwrap();
            state.starts += 1;
            state
                .behaviors
                .pop_front()
                .expect("test factory behavior was not configured")
        };
        match behavior {
            FactoryBehavior::Disconnect => {
                let (sender, receiver) = mpsc::unbounded_channel();
                drop(sender);
                RunningSegment::new(receiver)
            }
            FactoryBehavior::Panic => panic!("injected run coordinator factory panic"),
            FactoryBehavior::Hold => {
                let control = RunControlHandle::new();
                let (sender, receiver) = mpsc::unbounded_channel();
                let mut state = self.state.lock().unwrap();
                state.controls.insert(request.segment_id, control.clone());
                state.event_senders.insert(request.segment_id, sender);
                RunningSegment::with_control(request.segment_id, receiver, control)
            }
        }
    }
}

fn create_task(store: &TaskStore, title: &str) -> TaskId {
    let task_id = TaskId::new();
    let workspace_root = store
        .database_path()
        .parent()
        .unwrap()
        .join("workspaces")
        .join(task_id.to_string());
    std::fs::create_dir_all(&workspace_root).unwrap();
    let outcome = store
        .transact_command(create_task_command(task_id), |_| {
            Ok(vec![NewTaskEvent::task_created_in_workspace(
                title,
                harness_contracts::WorkspaceSelection {
                    mode: WorkspaceMode::Current,
                    root: workspace_root.to_string_lossy().into_owned(),
                },
            )])
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
        command: task_command(
            task_id,
            store.stream_version(task_id).unwrap(),
            json!({ "type": "start_segment", "segmentId": segment_id }),
        ),
        segment_id,
        started_at: Utc::now(),
    }
}

fn stop_command(store: &TaskStore, task_id: TaskId) -> ValidatedTaskCommand {
    ValidatedTaskCommand::StopRun {
        command: task_command(
            task_id,
            store.stream_version(task_id).unwrap(),
            json!({ "type": "stop_run", "mode": "force" }),
        ),
        mode: StopMode::Force,
    }
}

fn task_command(
    task_id: TaskId,
    expected_stream_version: u64,
    payload: serde_json::Value,
) -> AcceptedCommand {
    AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("fault-{}", CommandId::new()),
        expected_stream_version,
        authority: TaskStore::user_authority(ClientId::new()),
        payload,
    }
}

fn accepted(outcome: CommandOutcome) -> bool {
    matches!(outcome, CommandOutcome::Accepted { .. })
}

async fn wait_for_state(store: &TaskStore, task_id: TaskId, expected: TaskState) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if store
                .task_projection(task_id)
                .unwrap()
                .is_some_and(|task| task.state == expected)
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

async fn wait_for_checkpoint_tools(store: &TaskStore, task_id: TaskId, expected: &[ToolUseId]) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if store
                .latest_checkpoint(task_id)
                .unwrap()
                .is_some_and(|checkpoint| checkpoint.incomplete_tool_use_ids == expected)
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

async fn wait_for_start_count(factory: &FaultRunFactory, expected: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if factory.start_count() == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

fn append_events(store: &TaskStore, task_id: TaskId, events: Vec<NewTaskEvent>) {
    store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: format!("fault-seed-{}", CommandId::new()),
                expected_stream_version: 0,
                authority: TaskStore::supervisor_authority(),
                payload: json!({ "type": "seed" }),
            },
            |_| Ok(events),
        )
        .unwrap();
}

fn all_task_events(store: &TaskStore, task_id: TaskId) -> Vec<TaskEventEnvelope> {
    let mut after_stream_sequence = 0;
    let mut events = Vec::new();
    loop {
        let page = store
            .task_events_after(task_id, after_stream_sequence, usize::MAX)
            .unwrap();
        let Some(last) = page.last() else {
            break;
        };
        after_stream_sequence = last.stream_sequence;
        events.extend(page);
    }
    events
}

fn tool_requested(tool_use_id: ToolUseId) -> Event {
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
    })
}

fn openai_chat_profile(server: &MockServer) -> ProviderProfileDefinition {
    ProviderProfileDefinition {
        id: "retry-provider".into(),
        display_name: "retry-provider".into(),
        provider_id: "openai".into(),
        model_id: "gpt-5.4-mini".into(),
        protocol: ModelProtocol::ChatCompletions,
        model_options: Default::default(),
        base_url: Some(server.uri()),
        provider_defaults: None,
        model_descriptor: ProviderProfileModelDescriptor {
            protocol: ModelProtocol::ChatCompletions,
            context_window: 32_000,
            display_name: "gpt-5.4-mini".into(),
            lifecycle: ProviderProfileModelLifecycle::Stable,
            max_output_tokens: 4_096,
            model_id: "gpt-5.4-mini".into(),
            provider_id: "openai".into(),
            conversation_capability: ProviderProfileConversationCapability {
                input_modalities: vec!["text".into()],
                output_modalities: vec!["text".into()],
                context_window: 32_000,
                max_output_tokens: 4_096,
                streaming: true,
                tool_calling: true,
                reasoning: false,
                prompt_cache: false,
                structured_output: false,
            },
            runtime_semantics: None,
        },
    }
}

fn write_json(path: &std::path::Path, value: &(impl serde::Serialize + ?Sized)) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

async fn record_started_tool(store: &Arc<TaskStore>, task_id: TaskId, tool_use_id: ToolUseId) {
    let session_id = SessionId::new();
    TaskEventStoreAdapter::new(
        Arc::clone(store),
        task_id,
        TenantId::SINGLE,
        session_id,
        Arc::new(NoopRedactor),
    )
    .append(
        TenantId::SINGLE,
        session_id,
        &[
            tool_requested(tool_use_id),
            Event::ToolUseStarted(ToolUseStartedEvent {
                run_id: RunId::new(),
                tool_use_id,
                at: Utc::now(),
            }),
        ],
    )
    .await
    .unwrap();
}

fn ipc_config(blob_root: std::path::PathBuf, event_batch_capacity: usize) -> IpcServerConfig {
    IpcServerConfig {
        daemon_version: "0.1.0".into(),
        user_instance_id: "fault-user".into(),
        connection_token: "fault-token".into(),
        event_batch_capacity,
        blob_root,
    }
}

fn client_frame(request_id: &str, request: ClientRequest) -> ClientFrame {
    ClientFrame {
        request_id: request_id.into(),
        protocol_version: PROTOCOL_VERSION,
        request,
    }
}

fn handshake_frame() -> ClientFrame {
    client_frame(
        "handshake",
        ClientRequest::Handshake(HandshakeRequest {
            client_id: ClientId::new(),
            client_version: "0.1.0".into(),
            user_instance_id: "fault-user".into(),
            connection_token: "fault-token".into(),
            last_acknowledged_offset: 0,
        }),
    )
}

#[cfg(unix)]
async fn send_frame(stream: &mut tokio::net::UnixStream, frame: &ClientFrame) {
    use tokio::io::AsyncWriteExt;

    stream
        .write_all(&encode_frame(frame).unwrap())
        .await
        .unwrap();
}

#[cfg(unix)]
async fn receive_frame(stream: &mut tokio::net::UnixStream) -> harness_contracts::ServerFrame {
    use tokio::io::AsyncReadExt;

    let mut header = [0_u8; 4];
    stream.read_exact(&mut header).await.unwrap();
    let mut body = vec![0; u32::from_be_bytes(header) as usize];
    stream.read_exact(&mut body).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[test]
#[ignore = "child helper for process_death_before_and_after_sqlite_commit_preserves_only_committed_state"]
fn sqlite_commit_crash_child() {
    let Ok(mode) = std::env::var("JYOWO_FAULT_CHILD") else {
        return;
    };
    let database = std::env::var_os("JYOWO_FAULT_DATABASE").unwrap();
    if mode == "before" {
        std::process::abort();
    }

    let store = TaskStore::open(database).unwrap();
    let task_id = TaskId::from_u128(CRASH_TASK_ID);
    store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::from_u128(CRASH_COMMAND_ID),
                task_id,
                idempotency_key: "fault-child-create".into(),
                expected_stream_version: 0,
                authority: TaskStore::user_authority(ClientId::from_u128(1)),
                payload: json!({ "type": "create_task", "title": "committed child" }),
            },
            |_| Ok(vec![NewTaskEvent::task_created("committed child")]),
        )
        .unwrap();
    std::process::abort();
}

fn create_task_command(task_id: TaskId) -> AcceptedCommand {
    AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("create-{task_id}"),
        expected_stream_version: 0,
        authority: TaskStore::user_authority(ClientId::new()),
        payload: json!({ "type": "create_task", "title": "must roll back" }),
    }
}

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration as StdDuration;

use chrono::{Duration, Utc};
use harness_contracts::{
    ActorId, CommandId, DecisionLifetime, DecisionMatcherKind, DecisionMatcherSummary,
    PermissionDecisionOption, PermissionOptionId, PermissionRoute, RedactRules, Redactor,
    RequestId, RunSegmentId, TaskId, TimeoutPolicy, WorkspaceLeaseId, WorkspaceMode,
};
use harness_daemon::{
    DaemonPermissionKind, HarnessPermissionBroker, PermissionBroker, PermissionBrokerError,
    PermissionDecisionInput, PermissionOption, PermissionRequestDraft, PermissionRuntimeAuthority,
    SavedPermissionPolicy,
};
use harness_journal::{
    AcceptedCommand, AcquireTaskWorkspaceLease, CommandOutcome, NewTaskEvent, TaskStore,
    TaskWorkspaceAcquireOutcome,
};
use jyowo_harness_sdk::ext::{
    Decision, DecisionScope, FallbackPolicy, InteractivityLevel,
    PermissionBroker as EnginePermissionBroker, PermissionContext, PermissionMode,
    PermissionRequest, PermissionSubject, Severity, TenantId, ToolUseId,
};
use serde_json::json;

#[test]
fn broker_routes_supported_subjects_and_revalidates_ui_decisions() {
    for kind in [
        DaemonPermissionKind::Command,
        DaemonPermissionKind::Filesystem,
        DaemonPermissionKind::Network,
        DaemonPermissionKind::Mcp,
        DaemonPermissionKind::Automation,
    ] {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        let (task_id, segment_id) = create_running_task(&store, "permission");
        let broker = PermissionBroker::new(Arc::clone(&store), Arc::new(TokenRedactor));
        let draft = request_draft(&store, task_id, segment_id, kind);

        let outcome = broker.request(draft.clone()).unwrap();
        assert!(!outcome.auto_resolved);
        let projection = store.task_projection(task_id).unwrap().unwrap();
        let pending = projection.pending_permission.as_ref().unwrap();
        assert_eq!(pending.route, PermissionRoute::ForegroundTask);
        assert_eq!(pending.details.as_ref().unwrap().kind, kind);

        let persisted = store
            .task_events_after(task_id, 0, 64)
            .unwrap()
            .into_iter()
            .find(|event| event.event_type == "permission.requested")
            .unwrap()
            .payload
            .to_string();
        assert!(!persisted.contains("TOKEN"));

        broker
            .resolve(decision(&store, &draft, "allow-once"))
            .unwrap();
        assert!(store
            .task_projection(task_id)
            .unwrap()
            .unwrap()
            .pending_permission
            .is_none());
        assert!(broker
            .resolve(decision(&store, &draft, "allow-once"))
            .is_err());
    }
}

#[test]
fn task_store_rejects_permission_events_outside_broker_authority() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "authority");
    let draft = request_draft(
        &store,
        task_id,
        segment_id,
        DaemonPermissionKind::Filesystem,
    );
    let projection = harness_contracts::PermissionProjection {
        request_id: draft.request_id,
        revision: draft.request_revision,
        route: PermissionRoute::ForegroundTask,
        details: None,
    };
    let forged_request = store.transact_command(
        AcceptedCommand {
            command_id: CommandId::new(),
            task_id,
            idempotency_key: format!("forged-request-{}", draft.request_id),
            expected_stream_version: store.stream_version(task_id).unwrap(),
            authority: TaskStore::supervisor_authority(),
            payload: json!({ "type": "permission_request" }),
        },
        |_| Ok(vec![NewTaskEvent::permission_requested(projection)]),
    );
    assert!(forged_request.is_err());

    let broker = PermissionBroker::new(Arc::clone(&store), Arc::new(TokenRedactor));
    broker.request(draft.clone()).unwrap();
    let forged_resolution = store.transact_command(
        AcceptedCommand {
            command_id: CommandId::new(),
            task_id,
            idempotency_key: format!("forged-resolution-{}", draft.request_id),
            expected_stream_version: store.stream_version(task_id).unwrap(),
            authority: TaskStore::supervisor_authority(),
            payload: json!({ "type": "permission_resolve" }),
        },
        |_| {
            Ok(vec![NewTaskEvent::permission_resolved(
                draft.request_id,
                draft.request_revision,
            )])
        },
    );
    assert!(forged_resolution.is_err());
}

#[test]
fn invalidation_rejects_late_and_stale_decisions() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "invalidate");
    let broker = PermissionBroker::new(Arc::clone(&store), Arc::new(TokenRedactor));
    let draft = request_draft(
        &store,
        task_id,
        segment_id,
        DaemonPermissionKind::Filesystem,
    );
    broker.request(draft.clone()).unwrap();

    let mut stale = decision(&store, &draft, "allow-once");
    stale.request_revision += 1;
    assert!(broker.resolve(stale).is_err());
    broker
        .invalidate(
            task_id,
            draft.request_id,
            draft.request_revision,
            store.stream_version(task_id).unwrap(),
            "superseded by steering",
        )
        .unwrap();
    assert!(broker
        .resolve(decision(&store, &draft, "allow-once"))
        .is_err());
}

#[test]
fn request_ids_cannot_be_reused_for_a_different_unredacted_context() {
    for mutate in ["subject", "actor_source"] {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        let (task_id, segment_id) = create_running_task(&store, "context validation");
        let policy_calls = Arc::new(AtomicUsize::new(0));
        let broker = PermissionBroker::new(Arc::clone(&store), Arc::new(TokenRedactor))
            .with_saved_policy(Arc::new(CountingPolicy(Arc::clone(&policy_calls))));
        let draft = request_draft(&store, task_id, segment_id, DaemonPermissionKind::Command);
        broker.request(draft.clone()).unwrap();

        let mut changed = draft;
        if mutate == "subject" {
            changed.subject["secret"] = json!("TOKEN changed subject");
        } else {
            changed.actor_source["secret"] = json!("TOKEN changed actor");
        }
        assert!(
            broker.request(changed).is_err(),
            "accepted changed {mutate}"
        );
        assert_eq!(policy_calls.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn resolved_request_ids_cannot_be_reused_for_the_same_context() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "durable request identity");
    let broker = PermissionBroker::new(Arc::clone(&store), Arc::new(TokenRedactor));
    let mut draft = request_draft(
        &store,
        task_id,
        segment_id,
        DaemonPermissionKind::Filesystem,
    );

    broker.request(draft.clone()).unwrap();
    broker
        .resolve(decision(&store, &draft, "allow-once"))
        .unwrap();
    draft.expected_task_version = store.stream_version(task_id).unwrap();
    let restarted_broker = PermissionBroker::new(Arc::clone(&store), Arc::new(TokenRedactor));

    assert!(matches!(
        restarted_broker.request(draft),
        Err(PermissionBrokerError::Rejected(_))
    ));
    assert!(store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .pending_permission
        .is_none());
}

#[test]
fn concurrent_same_context_request_has_a_single_policy_owner() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "request reservation owner");
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let policy_calls = Arc::new(AtomicUsize::new(0));
    let broker = Arc::new(
        PermissionBroker::new(Arc::clone(&store), Arc::new(TokenRedactor)).with_saved_policy(
            Arc::new(BlockingFirstPolicy {
                calls: Arc::clone(&policy_calls),
                entered: entered_tx,
                resume: Mutex::new(resume_rx),
            }),
        ),
    );
    let draft = request_draft(&store, task_id, segment_id, DaemonPermissionKind::Command);
    let first_broker = Arc::clone(&broker);
    let first_draft = draft.clone();
    let first = thread::spawn(move || first_broker.request(first_draft));
    entered_rx.recv_timeout(StdDuration::from_secs(1)).unwrap();

    let duplicate = broker.request(draft);

    resume_tx.send(()).unwrap();
    let first_result = first.join().unwrap();
    assert!(duplicate.is_err());
    assert_eq!(policy_calls.load(Ordering::SeqCst), 1);
    assert!(first_result.is_ok());
}

#[test]
fn resolution_checks_expiry_against_daemon_time() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "expiry");
    let broker = PermissionBroker::new(Arc::clone(&store), Arc::new(TokenRedactor));
    let mut draft = request_draft(&store, task_id, segment_id, DaemonPermissionKind::Network);
    draft.expires_at = Utc::now() + Duration::milliseconds(20);
    broker.request(draft.clone()).unwrap();
    thread::sleep(StdDuration::from_millis(30));

    assert!(broker
        .resolve(decision(&store, &draft, "allow-once"))
        .is_err());
}

#[test]
fn request_expiry_is_rechecked_after_policy_evaluation() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "request expiry");
    let broker = PermissionBroker::new(Arc::clone(&store), Arc::new(TokenRedactor))
        .with_saved_policy(Arc::new(SlowPolicy));
    let mut draft = request_draft(
        &store,
        task_id,
        segment_id,
        DaemonPermissionKind::Automation,
    );
    draft.expires_at = Utc::now() + Duration::milliseconds(20);

    assert!(broker.request(draft).is_err());
    assert!(store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .pending_permission
        .is_none());
}

#[test]
fn engine_permission_request_retries_after_unrelated_task_version_change() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "request version retry");
    let broker = PermissionBroker::new(Arc::clone(&store), Arc::new(TokenRedactor))
        .with_saved_policy(Arc::new(VersionBumpingPolicy {
            store: Arc::clone(&store),
            task_id,
        }));
    let draft = request_draft(&store, task_id, segment_id, DaemonPermissionKind::Command);

    let outcome = broker.request(draft).unwrap();

    assert!(!outcome.auto_resolved);
    assert!(store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .pending_permission
        .is_some());
}

#[test]
fn saved_policy_resolves_child_requests_without_exposing_child_payloads() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "child permission");
    let broker = PermissionBroker::new(Arc::clone(&store), Arc::new(TokenRedactor))
        .with_saved_policy(Arc::new(AllowOncePolicy));
    let mut draft = request_draft(&store, task_id, segment_id, DaemonPermissionKind::Mcp);
    draft.actor_source = json!({
        "type": "subagent",
        "childTaskId": TaskId::new(),
        "secret": "TOKEN child"
    });

    let outcome = broker.request(draft).unwrap();
    assert!(outcome.auto_resolved);
    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert!(projection.pending_permission.is_none());
    let events = store.task_events_after(task_id, 0, 64).unwrap();
    assert!(events
        .iter()
        .any(|event| event.event_type == "permission.requested"
            && event.payload["route"] == "saved_policy"));
    assert!(events
        .iter()
        .any(|event| event.event_type == "permission.resolved"));
    assert!(!events
        .iter()
        .any(|event| event.payload.to_string().contains("TOKEN")));
}

#[tokio::test]
async fn engine_waiter_receives_the_ui_option_committed_by_the_daemon_broker() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "engine permission");
    let daemon = Arc::new(PermissionBroker::new(
        Arc::clone(&store),
        Arc::new(TokenRedactor),
    ));
    let engine = HarnessPermissionBroker::new(
        Arc::clone(&daemon),
        task_id,
        segment_id,
        permission_runtime_authority(&store, task_id),
    );
    let request = engine_permission_request(task_id, None);
    let allow_option = request
        .decision_options
        .iter()
        .find(|option| option.decision == Decision::AllowOnce)
        .unwrap()
        .option_id;
    let request_id = request.request_id;
    let context = engine_permission_context(&request, segment_id);

    let decision_task = tokio::spawn(async move { engine.decide(request, context).await });
    tokio::time::timeout(StdDuration::from_secs(1), async {
        loop {
            if store
                .task_projection(task_id)
                .unwrap()
                .unwrap()
                .pending_permission
                .as_ref()
                .is_some_and(|pending| pending.request_id == request_id)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    daemon
        .resolve(PermissionDecisionInput {
            task_id,
            request_id,
            request_revision: 1,
            option_id: allow_option.to_string(),
            expected_task_version: store.stream_version(task_id).unwrap(),
        })
        .unwrap();

    assert_eq!(decision_task.await.unwrap(), Decision::AllowOnce);
}

#[tokio::test(flavor = "current_thread")]
async fn committed_ui_decision_wins_when_waiter_and_timeout_are_both_ready() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "permission timeout race");
    let daemon = Arc::new(PermissionBroker::new(
        Arc::clone(&store),
        Arc::new(TokenRedactor),
    ));
    let engine = HarnessPermissionBroker::new(
        Arc::clone(&daemon),
        task_id,
        segment_id,
        permission_runtime_authority(&store, task_id),
    );
    let request = engine_permission_request(task_id, None);
    let request_id = request.request_id;
    let allow_option = request
        .decision_options
        .iter()
        .find(|option| option.decision == Decision::AllowOnce)
        .unwrap()
        .option_id;
    let mut context = engine_permission_context(&request, segment_id);
    context.timeout_policy = Some(TimeoutPolicy {
        deadline_ms: 20,
        default_on_timeout: Decision::DenyOnce,
        heartbeat_interval_ms: None,
    });

    let decision_task = tokio::spawn(async move { engine.decide(request, context).await });
    wait_for_pending_permission(&store, task_id, request_id).await;
    daemon
        .resolve(PermissionDecisionInput {
            task_id,
            request_id,
            request_revision: 1,
            option_id: allow_option.to_string(),
            expected_task_version: store.stream_version(task_id).unwrap(),
        })
        .unwrap();
    thread::sleep(StdDuration::from_millis(30));

    assert_eq!(decision_task.await.unwrap(), Decision::AllowOnce);
}

#[tokio::test]
async fn timeout_retries_invalidation_after_an_unrelated_version_change() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "permission timeout retry");
    let daemon = Arc::new(PermissionBroker::new(
        Arc::clone(&store),
        Arc::new(TimeoutVersionBumpingRedactor {
            store: Arc::clone(&store),
            task_id,
            bumped: AtomicBool::new(false),
        }),
    ));
    let engine = HarnessPermissionBroker::new(
        Arc::clone(&daemon),
        task_id,
        segment_id,
        permission_runtime_authority(&store, task_id),
    );
    let request = engine_permission_request(task_id, None);
    let mut context = engine_permission_context(&request, segment_id);
    context.timeout_policy = Some(TimeoutPolicy {
        deadline_ms: 20,
        default_on_timeout: Decision::DenyOnce,
        heartbeat_interval_ms: None,
    });

    let decision = engine.decide(request, context).await;

    assert_eq!(decision, Decision::DenyOnce);
    assert!(store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .pending_permission
        .is_none());
    assert!(store
        .events_after(0, 100)
        .unwrap()
        .iter()
        .any(|event| event.event_type == "permission.invalidated" && event.task_id == task_id));
    assert!(store
        .events_after(0, 100)
        .unwrap()
        .iter()
        .any(|event| event.event_type == "task.title_changed" && event.task_id == task_id));
}

#[tokio::test]
async fn externally_invalidated_permission_fails_closed_without_waiting_forever() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "external invalidation");
    let daemon = Arc::new(PermissionBroker::new(
        Arc::clone(&store),
        Arc::new(TokenRedactor),
    ));
    let engine = HarnessPermissionBroker::new(
        Arc::clone(&daemon),
        task_id,
        segment_id,
        permission_runtime_authority(&store, task_id),
    );
    let request = engine_permission_request(task_id, None);
    let request_id = request.request_id;
    let mut context = engine_permission_context(&request, segment_id);
    context.timeout_policy = Some(TimeoutPolicy {
        deadline_ms: 20,
        default_on_timeout: Decision::DenyOnce,
        heartbeat_interval_ms: None,
    });

    let decision_task = tokio::spawn(async move { engine.decide(request, context).await });
    wait_for_pending_permission(&store, task_id, request_id).await;
    store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: format!("external-permission-invalidation-{request_id}"),
                expected_stream_version: store.stream_version(task_id).unwrap(),
                authority: TaskStore::permission_broker_authority(),
                payload: json!({ "type": "external_permission_invalidation" }),
            },
            |_| {
                Ok(vec![NewTaskEvent::permission_invalidated(
                    request_id,
                    1,
                    "actor recovery",
                )])
            },
        )
        .unwrap();

    assert_eq!(
        tokio::time::timeout(StdDuration::from_millis(250), decision_task)
            .await
            .expect("permission waiter must not remain pending")
            .unwrap(),
        Decision::DenyOnce
    );
}

#[tokio::test]
async fn permission_resolution_rejects_a_released_workspace_authority() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "released permission authority");
    let daemon = Arc::new(PermissionBroker::new(
        Arc::clone(&store),
        Arc::new(TokenRedactor),
    ));
    let runtime_authority = permission_runtime_authority(&store, task_id);
    let lease_id = runtime_authority.workspace_lease_id;
    let engine =
        HarnessPermissionBroker::new(Arc::clone(&daemon), task_id, segment_id, runtime_authority);
    let request = engine_permission_request(task_id, None);
    let request_id = request.request_id;
    let allow_option = request
        .decision_options
        .iter()
        .find(|option| option.decision == Decision::AllowOnce)
        .unwrap()
        .option_id;
    let mut context = engine_permission_context(&request, segment_id);
    context.timeout_policy = Some(TimeoutPolicy {
        deadline_ms: 50,
        default_on_timeout: Decision::DenyOnce,
        heartbeat_interval_ms: None,
    });

    let decision_task = tokio::spawn(async move { engine.decide(request, context).await });
    wait_for_pending_permission(&store, task_id, request_id).await;
    store
        .release_workspace_lease(lease_id, "test authority change")
        .unwrap();

    assert!(daemon
        .resolve(PermissionDecisionInput {
            task_id,
            request_id,
            request_revision: 1,
            option_id: allow_option.to_string(),
            expected_task_version: store.stream_version(task_id).unwrap(),
        })
        .is_err());
    assert_eq!(decision_task.await.unwrap(), Decision::DenyOnce);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn saved_policy_resolution_rejects_a_released_workspace_authority() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "saved authority race");
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let daemon = Arc::new(
        PermissionBroker::new(Arc::clone(&store), Arc::new(TokenRedactor)).with_saved_policy(
            Arc::new(BlockingAllowPolicy {
                entered: entered_tx,
                resume: Mutex::new(resume_rx),
            }),
        ),
    );
    let runtime_authority = permission_runtime_authority(&store, task_id);
    let lease_id = runtime_authority.workspace_lease_id;
    let engine =
        HarnessPermissionBroker::new(Arc::clone(&daemon), task_id, segment_id, runtime_authority);
    let request = engine_permission_request(task_id, None);
    let context = engine_permission_context(&request, segment_id);
    let decision_task = tokio::spawn(async move { engine.decide(request, context).await });
    entered_rx.recv_timeout(StdDuration::from_secs(1)).unwrap();

    store
        .release_workspace_lease(lease_id, "saved policy authority race")
        .unwrap();
    resume_tx.send(()).unwrap();

    assert_eq!(decision_task.await.unwrap(), Decision::DenyOnce);
    assert!(store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .pending_permission
        .is_none());
}

#[tokio::test]
async fn duplicate_engine_request_id_does_not_replace_the_original_waiter() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "duplicate engine waiter");
    let daemon = Arc::new(PermissionBroker::new(
        Arc::clone(&store),
        Arc::new(TokenRedactor),
    ));
    let engine = HarnessPermissionBroker::new(
        Arc::clone(&daemon),
        task_id,
        segment_id,
        permission_runtime_authority(&store, task_id),
    );
    let request = engine_permission_request(task_id, None);
    let context = engine_permission_context(&request, segment_id);
    let request_id = request.request_id;
    let allow_option = request
        .decision_options
        .iter()
        .find(|option| option.decision == Decision::AllowOnce)
        .unwrap()
        .option_id;
    let first_engine = engine.clone();
    let first_request = request.clone();
    let first_context = context.clone();
    let first =
        tokio::spawn(async move { first_engine.decide(first_request, first_context).await });
    wait_for_pending_permission(&store, task_id, request_id).await;

    let duplicate = tokio::spawn(async move { engine.decide(request, context).await });
    assert_eq!(duplicate.await.unwrap(), Decision::DenyOnce);
    assert!(!first.is_finished(), "the original waiter was replaced");

    daemon
        .resolve(PermissionDecisionInput {
            task_id,
            request_id,
            request_revision: 1,
            option_id: allow_option.to_string(),
            expected_task_version: store.stream_version(task_id).unwrap(),
        })
        .unwrap();
    assert_eq!(first.await.unwrap(), Decision::AllowOnce);
}

#[tokio::test]
async fn engine_options_requiring_confirmation_cannot_be_approved_without_confirmation_text() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let (task_id, segment_id) = create_running_task(&store, "confirmation permission");
    let daemon = Arc::new(PermissionBroker::new(
        Arc::clone(&store),
        Arc::new(TokenRedactor),
    ));
    let engine = HarnessPermissionBroker::new(
        Arc::clone(&daemon),
        task_id,
        segment_id,
        permission_runtime_authority(&store, task_id),
    );
    let request = engine_permission_request(task_id, Some("type cargo test".into()));
    let context = engine_permission_context(&request, segment_id);
    let request_id = request.request_id;
    let allow_option = request
        .decision_options
        .iter()
        .find(|option| option.decision == Decision::AllowOnce)
        .unwrap()
        .option_id;
    let deny_option = request
        .decision_options
        .iter()
        .find(|option| option.decision == Decision::DenyOnce)
        .unwrap()
        .option_id;
    let decision_task = tokio::spawn(async move { engine.decide(request, context).await });
    wait_for_pending_permission(&store, task_id, request_id).await;

    assert!(daemon
        .resolve(PermissionDecisionInput {
            task_id,
            request_id,
            request_revision: 1,
            option_id: allow_option.to_string(),
            expected_task_version: store.stream_version(task_id).unwrap(),
        })
        .is_err());
    daemon
        .resolve(PermissionDecisionInput {
            task_id,
            request_id,
            request_revision: 1,
            option_id: deny_option.to_string(),
            expected_task_version: store.stream_version(task_id).unwrap(),
        })
        .unwrap();
    assert_eq!(decision_task.await.unwrap(), Decision::DenyOnce);
}

struct AllowOncePolicy;

impl SavedPermissionPolicy for AllowOncePolicy {
    fn resolve(&self, _request: &PermissionRequestDraft) -> Option<String> {
        Some("allow-once".into())
    }
}

struct SlowPolicy;

impl SavedPermissionPolicy for SlowPolicy {
    fn resolve(&self, _request: &PermissionRequestDraft) -> Option<String> {
        thread::sleep(StdDuration::from_millis(30));
        None
    }
}

struct CountingPolicy(Arc<AtomicUsize>);

impl SavedPermissionPolicy for CountingPolicy {
    fn resolve(&self, _request: &PermissionRequestDraft) -> Option<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        None
    }
}

struct BlockingFirstPolicy {
    calls: Arc<AtomicUsize>,
    entered: mpsc::Sender<()>,
    resume: Mutex<mpsc::Receiver<()>>,
}

impl SavedPermissionPolicy for BlockingFirstPolicy {
    fn resolve(&self, _request: &PermissionRequestDraft) -> Option<String> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.entered.send(()).unwrap();
            self.resume.lock().unwrap().recv().unwrap();
        }
        None
    }
}

struct BlockingAllowPolicy {
    entered: mpsc::Sender<()>,
    resume: Mutex<mpsc::Receiver<()>>,
}

impl SavedPermissionPolicy for BlockingAllowPolicy {
    fn resolve(&self, request: &PermissionRequestDraft) -> Option<String> {
        self.entered.send(()).unwrap();
        self.resume.lock().unwrap().recv().unwrap();
        request
            .options
            .first()
            .map(|option| option.option_id.clone())
    }
}

struct VersionBumpingPolicy {
    store: Arc<TaskStore>,
    task_id: TaskId,
}

impl SavedPermissionPolicy for VersionBumpingPolicy {
    fn resolve(&self, _request: &PermissionRequestDraft) -> Option<String> {
        let expected_stream_version = self.store.stream_version(self.task_id).unwrap();
        self.store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id: self.task_id,
                    idempotency_key: format!("permission-version-bump-{expected_stream_version}"),
                    expected_stream_version,
                    authority: TaskStore::supervisor_authority(),
                    payload: json!({ "type": "change_title" }),
                },
                |_| Ok(vec![NewTaskEvent::task_title_changed("version changed")]),
            )
            .unwrap();
        None
    }
}

struct TokenRedactor;

impl Redactor for TokenRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        if input.starts_with("TOKEN") {
            "[REDACTED]".into()
        } else {
            input.into()
        }
    }
}

struct TimeoutVersionBumpingRedactor {
    store: Arc<TaskStore>,
    task_id: TaskId,
    bumped: AtomicBool,
}

impl Redactor for TimeoutVersionBumpingRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        if input == "permission request expired while waiting for a client decision"
            && !self.bumped.swap(true, Ordering::SeqCst)
        {
            let expected_stream_version = self.store.stream_version(self.task_id).unwrap();
            self.store
                .transact_command(
                    AcceptedCommand {
                        command_id: CommandId::new(),
                        task_id: self.task_id,
                        idempotency_key: format!(
                            "permission-timeout-version-bump-{expected_stream_version}"
                        ),
                        expected_stream_version,
                        authority: TaskStore::supervisor_authority(),
                        payload: json!({ "type": "change_title" }),
                    },
                    |_| Ok(vec![NewTaskEvent::task_title_changed("timeout raced")]),
                )
                .unwrap();
        }
        input.into()
    }
}

fn request_draft(
    store: &TaskStore,
    task_id: TaskId,
    segment_id: RunSegmentId,
    kind: DaemonPermissionKind,
) -> PermissionRequestDraft {
    PermissionRequestDraft {
        task_id,
        segment_id,
        request_id: RequestId::new(),
        request_revision: 1,
        expected_task_version: store.stream_version(task_id).unwrap(),
        kind,
        action_plan_hash: "plan-v1".into(),
        sandbox_policy_hash: "sandbox-v1".into(),
        workspace: "/workspace".into(),
        subject: json!({ "operation": "write", "secret": "TOKEN subject", "TOKEN key": "value" }),
        actor_source: json!({ "type": "parent_run", "secret": "TOKEN actor" }),
        options: vec![PermissionOption {
            option_id: "allow-once".into(),
            label: "Allow once".into(),
        }],
        preview: "TOKEN preview".into(),
        expires_at: Utc::now() + Duration::minutes(5),
    }
}

fn decision(
    store: &TaskStore,
    draft: &PermissionRequestDraft,
    option_id: &str,
) -> PermissionDecisionInput {
    PermissionDecisionInput {
        task_id: draft.task_id,
        request_id: draft.request_id,
        request_revision: draft.request_revision,
        option_id: option_id.into(),
        expected_task_version: store.stream_version(draft.task_id).unwrap(),
    }
}

fn engine_permission_request(
    task_id: TaskId,
    confirmation_expected: Option<String>,
) -> PermissionRequest {
    let mut request = PermissionRequest {
        request_id: RequestId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: harness_contracts::SessionId::from_u128(u128::from_be_bytes(
            task_id.as_bytes(),
        )),
        tool_use_id: ToolUseId::new(),
        tool_name: "Bash".into(),
        subject: PermissionSubject::CommandExec {
            command: "cargo test".into(),
            argv: vec!["cargo".into(), "test".into()],
            cwd: Some("/workspace".into()),
            fingerprint: None,
        },
        severity: Severity::Medium,
        scope_hint: DecisionScope::ToolName("Bash".into()),
        action_plan_hash: Default::default(),
        decision_options: Vec::new(),
        confirmation_expected,
        created_at: Utc::now(),
    };
    request.decision_options = vec![
        PermissionDecisionOption {
            option_id: PermissionOptionId::new(),
            decision: Decision::AllowOnce,
            scope: request.scope_hint.clone(),
            lifetime: DecisionLifetime::Once,
            matcher_summary: DecisionMatcherSummary {
                kind: DecisionMatcherKind::ToolName,
                label: "Bash".into(),
            },
            label: "Allow once".into(),
            requires_confirmation: request.confirmation_expected.is_some(),
            action_plan_hash: request.action_plan_hash.clone(),
            fingerprint: None,
        },
        PermissionDecisionOption {
            option_id: PermissionOptionId::new(),
            decision: Decision::DenyOnce,
            scope: request.scope_hint.clone(),
            lifetime: DecisionLifetime::Once,
            matcher_summary: DecisionMatcherSummary {
                kind: DecisionMatcherKind::ToolName,
                label: "Bash".into(),
            },
            label: "Deny once".into(),
            requires_confirmation: false,
            action_plan_hash: request.action_plan_hash.clone(),
            fingerprint: None,
        },
    ];
    request
}

fn engine_permission_context(
    request: &PermissionRequest,
    segment_id: RunSegmentId,
) -> PermissionContext {
    PermissionContext {
        permission_mode: PermissionMode::Default,
        previous_mode: None,
        session_id: request.session_id,
        tenant_id: request.tenant_id,
        run_id: Some(harness_contracts::RunId::from_u128(u128::from_be_bytes(
            segment_id.as_bytes(),
        ))),
        interactivity: InteractivityLevel::FullyInteractive,
        timeout_policy: None,
        fallback_policy: FallbackPolicy::AskUser,
        hook_overrides: Vec::new(),
    }
}

fn permission_runtime_authority(store: &TaskStore, task_id: TaskId) -> PermissionRuntimeAuthority {
    let lease_id = WorkspaceLeaseId::new();
    let actor_id = ActorId::new();
    let lease = match store
        .acquire_workspace_lease(AcquireTaskWorkspaceLease {
            lease_id,
            task_id,
            actor_id,
            mode: WorkspaceMode::Current,
            canonical_root: "/workspace".into(),
            worktree_path: None,
            branch: None,
            writable: true,
            requested_at: Utc::now(),
            expires_at: None,
            baseline_commit: None,
            baseline_status: "clean".into(),
        })
        .unwrap()
    {
        TaskWorkspaceAcquireOutcome::Acquired(lease) => lease,
        TaskWorkspaceAcquireOutcome::Waiting(_) => {
            panic!("permission fixture lease must be active")
        }
    };
    PermissionRuntimeAuthority {
        workspace_lease_id: lease.lease_id,
        actor_id: lease.actor_id,
        execution_root: lease.canonical_root,
        writable: lease.writable,
        sandbox_policy_hash: "sandbox-v1".into(),
    }
}

async fn wait_for_pending_permission(store: &TaskStore, task_id: TaskId, request_id: RequestId) {
    tokio::time::timeout(StdDuration::from_secs(1), async {
        loop {
            if store
                .task_projection(task_id)
                .unwrap()
                .unwrap()
                .pending_permission
                .as_ref()
                .is_some_and(|pending| pending.request_id == request_id)
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

fn create_running_task(store: &TaskStore, title: &str) -> (TaskId, RunSegmentId) {
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let outcome = store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: format!("create-{task_id}"),
                expected_stream_version: 0,
                authority: TaskStore::supervisor_authority(),
                payload: json!({ "type": "create" }),
            },
            |_| {
                Ok(vec![
                    NewTaskEvent::task_created(title),
                    NewTaskEvent::run_started(segment_id, Utc::now()),
                ])
            },
        )
        .unwrap();
    assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
    (task_id, segment_id)
}

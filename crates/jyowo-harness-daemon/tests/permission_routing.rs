use std::sync::Arc;
use std::thread;
use std::time::Duration as StdDuration;

use chrono::{Duration, Utc};
use harness_contracts::{
    CommandId, PermissionRoute, RedactRules, Redactor, RequestId, RunSegmentId, TaskId,
};
use harness_daemon::{
    DaemonPermissionKind, PermissionBroker, PermissionDecisionInput, PermissionOption,
    PermissionRequestDraft, SavedPermissionPolicy,
};
use harness_journal::{AcceptedCommand, CommandOutcome, NewTaskEvent, TaskStore};
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
        let broker = PermissionBroker::new(Arc::clone(&store), Arc::new(TokenRedactor));
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
    }
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

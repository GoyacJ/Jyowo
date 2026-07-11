#![cfg(feature = "sqlite")]

use chrono::{Duration, Utc};
use harness_contracts::{
    ActorId, CommandId, EventSourceKind, RedactRules, Redactor, RunSegmentId, SubagentActorState,
    SubagentId, TaskId, WorkspaceLeaseId, WorkspaceMode,
};
use harness_journal::{
    AcceptedCommand, AcquireTaskWorkspaceLease, CommandOutcome, CreateSubagentActorRequest,
    ExpectedParentStopSubagent, NewTaskEvent, ParentSubagentStopMode, RedactedSubagentSummary,
    SubagentLifecycleAuthority, SubagentLifecycleCommand, SubagentLifecycleTransition, TaskStore,
    TaskWorkspaceAcquireOutcome,
};
use serde_json::json;

#[test]
fn checked_spawn_requires_the_current_parent_actor_running_segment_and_active_lease() {
    let root = TempRoot::new("checked-spawn");
    let store = TaskStore::open(root.path().join("tasks.sqlite")).unwrap();
    let parent_task_id = TaskId::new();
    let parent_segment_id = RunSegmentId::new();
    create_running_task(&store, parent_task_id, parent_segment_id);
    let parent = store.task_projection(parent_task_id).unwrap().unwrap();
    let parent_actor_id = parent.actor_id.unwrap();
    let request = create_request(
        &store,
        root.path(),
        parent_task_id,
        parent_segment_id,
        parent_actor_id,
    );

    let mut wrong_actor = request.clone();
    wrong_actor.parent_actor_id = ActorId::new();
    assert!(store.create_subagent_actor_checked(wrong_actor).is_err());

    let mut wrong_segment = request.clone();
    wrong_segment.parent_segment_id = RunSegmentId::new();
    assert!(store.create_subagent_actor_checked(wrong_segment).is_err());

    let mut wrong_lease = request.clone();
    wrong_lease.parent_workspace_lease_id = WorkspaceLeaseId::new();
    assert!(store.create_subagent_actor_checked(wrong_lease).is_err());

    store
        .create_subagent_actor_checked(request.clone())
        .unwrap();
    let projected = store.task_projection(parent_task_id).unwrap().unwrap();
    assert_eq!(projected.subagents[0].child_task_id, request.child_task_id);
    let child = store
        .task_projection(request.child_task_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        child.current_run.as_ref().map(|run| run.segment_id),
        Some(request.segment_id)
    );
    assert_eq!(
        child.current_run.as_ref().map(|run| run.state.clone()),
        Some(harness_contracts::RunState::Running)
    );
    assert!(store
        .task_events_after(request.child_task_id, 0, 16)
        .unwrap()
        .iter()
        .any(|event| event.event_type == "run.started"));
}

#[test]
fn lifecycle_commands_cas_state_and_validate_workspace_ownership() {
    let root = TempRoot::new("lifecycle-cas");
    let store = TaskStore::open(root.path().join("tasks.sqlite")).unwrap();
    let parent_task_id = TaskId::new();
    let parent_segment_id = RunSegmentId::new();
    create_running_task(&store, parent_task_id, parent_segment_id);
    let parent_actor_id = store
        .task_projection(parent_task_id)
        .unwrap()
        .unwrap()
        .actor_id
        .unwrap();
    let request = create_request(
        &store,
        root.path(),
        parent_task_id,
        parent_segment_id,
        parent_actor_id,
    );
    store
        .create_subagent_actor_checked(request.clone())
        .unwrap();

    let foreign_task_id = TaskId::new();
    create_task(&store, foreign_task_id, "foreign");
    let foreign_lease_id = active_managed_lease(
        &store,
        foreign_task_id,
        ActorId::new(),
        root.path().to_str().unwrap(),
    );
    assert!(store
        .apply_subagent_lifecycle(SubagentLifecycleCommand {
            parent_task_id,
            child_task_id: request.child_task_id,
            actor_id: request.actor_id,
            authority: SubagentLifecycleAuthority::Supervisor,
            transition: SubagentLifecycleTransition::Running {
                workspace_lease_id: foreign_lease_id,
                context_cursor: 0,
            },
        })
        .is_err());

    let child_lease_id = active_managed_lease(
        &store,
        request.child_task_id,
        request.actor_id,
        root.path().to_str().unwrap(),
    );
    store
        .apply_subagent_lifecycle(SubagentLifecycleCommand {
            parent_task_id,
            child_task_id: request.child_task_id,
            actor_id: request.actor_id,
            authority: SubagentLifecycleAuthority::Supervisor,
            transition: SubagentLifecycleTransition::Running {
                workspace_lease_id: child_lease_id,
                context_cursor: 4,
            },
        })
        .unwrap();

    assert!(store
        .apply_subagent_lifecycle(SubagentLifecycleCommand {
            parent_task_id,
            child_task_id: request.child_task_id,
            actor_id: request.actor_id,
            authority: SubagentLifecycleAuthority::Supervisor,
            transition: SubagentLifecycleTransition::Running {
                workspace_lease_id: child_lease_id,
                context_cursor: 5,
            },
        })
        .is_err());
}

#[test]
fn lifecycle_uses_explicit_authority_and_only_accepts_redacted_summary() {
    let root = TempRoot::new("lifecycle-authority");
    let store = TaskStore::open(root.path().join("tasks.sqlite")).unwrap();
    let parent_task_id = TaskId::new();
    let parent_segment_id = RunSegmentId::new();
    create_running_task(&store, parent_task_id, parent_segment_id);
    let parent_actor_id = store
        .task_projection(parent_task_id)
        .unwrap()
        .unwrap()
        .actor_id
        .unwrap();
    let request = create_request(
        &store,
        root.path(),
        parent_task_id,
        parent_segment_id,
        parent_actor_id,
    );
    store
        .create_subagent_actor_checked(request.clone())
        .unwrap();
    let lease_id = active_managed_lease(
        &store,
        request.child_task_id,
        request.actor_id,
        root.path().to_str().unwrap(),
    );
    store
        .apply_subagent_lifecycle(SubagentLifecycleCommand {
            parent_task_id,
            child_task_id: request.child_task_id,
            actor_id: request.actor_id,
            authority: SubagentLifecycleAuthority::Supervisor,
            transition: SubagentLifecycleTransition::Running {
                workspace_lease_id: lease_id,
                context_cursor: 0,
            },
        })
        .unwrap();

    for transition in [
        SubagentLifecycleTransition::Yielding,
        SubagentLifecycleTransition::Background,
    ] {
        assert!(store
            .apply_subagent_lifecycle(SubagentLifecycleCommand {
                parent_task_id,
                child_task_id: request.child_task_id,
                actor_id: request.actor_id,
                authority: SubagentLifecycleAuthority::Actor(request.actor_id),
                transition,
            })
            .is_err());
    }

    let summary = RedactedSubagentSummary::new(&TokenRedactor, "TOKEN secret");
    store
        .apply_subagent_lifecycle(SubagentLifecycleCommand {
            parent_task_id,
            child_task_id: request.child_task_id,
            actor_id: request.actor_id,
            authority: SubagentLifecycleAuthority::Actor(request.actor_id),
            transition: SubagentLifecycleTransition::Completed {
                summary,
                ended_at: Utc::now(),
            },
        })
        .unwrap();

    assert_eq!(
        store.workspace_lease(lease_id).unwrap().unwrap().state,
        harness_journal::TaskWorkspaceLeaseState::CleanupPending
    );

    let parent = store.task_projection(parent_task_id).unwrap().unwrap();
    assert_eq!(parent.subagents[0].state, SubagentActorState::Completed);
    assert_eq!(
        parent.subagents[0].summary.as_deref(),
        Some("[REDACTED] secret")
    );
    let events = store.task_events_after(parent_task_id, 0, 64).unwrap();
    for event in events.iter().filter(|event| {
        matches!(
            event.event_type.as_str(),
            "subagent.summary_updated" | "subagent.terminal"
        )
    }) {
        assert_eq!(event.source.kind, EventSourceKind::Subagent);
        assert_eq!(event.source.actor_id, Some(request.actor_id));
        assert!(!event.payload.to_string().contains("TOKEN"));
    }
}

#[test]
fn force_stop_atomically_marks_each_child_workspace_for_cleanup() {
    let root = TempRoot::new("force-stop-cleanup");
    let store = TaskStore::open(root.path().join("tasks.sqlite")).unwrap();
    let parent_task_id = TaskId::new();
    let parent_segment_id = RunSegmentId::new();
    create_running_task(&store, parent_task_id, parent_segment_id);
    let parent_actor_id = store
        .task_projection(parent_task_id)
        .unwrap()
        .unwrap()
        .actor_id
        .unwrap();
    let request = create_request(
        &store,
        root.path(),
        parent_task_id,
        parent_segment_id,
        parent_actor_id,
    );
    store
        .create_subagent_actor_checked(request.clone())
        .unwrap();
    let lease_id = active_managed_lease(
        &store,
        request.child_task_id,
        request.actor_id,
        root.path().to_str().unwrap(),
    );
    store
        .apply_subagent_lifecycle(SubagentLifecycleCommand {
            parent_task_id,
            child_task_id: request.child_task_id,
            actor_id: request.actor_id,
            authority: SubagentLifecycleAuthority::Supervisor,
            transition: SubagentLifecycleTransition::Running {
                workspace_lease_id: lease_id,
                context_cursor: 0,
            },
        })
        .unwrap();

    store
        .apply_parent_subagent_stop(
            parent_task_id,
            &[ExpectedParentStopSubagent {
                child_task_id: request.child_task_id,
                actor_id: request.actor_id,
                expected_state: SubagentActorState::Running,
            }],
            ParentSubagentStopMode::Force {
                ended_at: Utc::now(),
            },
        )
        .unwrap();

    assert_eq!(
        store.workspace_lease(lease_id).unwrap().unwrap().state,
        harness_journal::TaskWorkspaceLeaseState::CleanupPending
    );
    assert_eq!(
        store
            .task_projection(request.child_task_id)
            .unwrap()
            .unwrap()
            .current_run
            .unwrap()
            .state,
        harness_contracts::RunState::Interrupted
    );
}

#[test]
fn safe_stop_marks_a_starting_child_yielding_before_workspace_start() {
    let root = TempRoot::new("safe-stop-starting");
    let store = TaskStore::open(root.path().join("tasks.sqlite")).unwrap();
    let parent_task_id = TaskId::new();
    let parent_segment_id = RunSegmentId::new();
    create_running_task(&store, parent_task_id, parent_segment_id);
    let parent_actor_id = store
        .task_projection(parent_task_id)
        .unwrap()
        .unwrap()
        .actor_id
        .unwrap();
    let request = create_request(
        &store,
        root.path(),
        parent_task_id,
        parent_segment_id,
        parent_actor_id,
    );
    store
        .create_subagent_actor_checked(request.clone())
        .unwrap();

    let stopped = store
        .apply_parent_subagent_stop(
            parent_task_id,
            &[ExpectedParentStopSubagent {
                child_task_id: request.child_task_id,
                actor_id: request.actor_id,
                expected_state: SubagentActorState::Starting,
            }],
            ParentSubagentStopMode::SafePoint,
        )
        .unwrap();

    assert_eq!(stopped[0].state, SubagentActorState::Yielding);

    let lease_id = active_managed_lease(
        &store,
        request.child_task_id,
        request.actor_id,
        root.path().to_str().unwrap(),
    );
    let running = store
        .apply_subagent_lifecycle(SubagentLifecycleCommand {
            parent_task_id,
            child_task_id: request.child_task_id,
            actor_id: request.actor_id,
            authority: SubagentLifecycleAuthority::Actor(request.actor_id),
            transition: SubagentLifecycleTransition::Running {
                workspace_lease_id: lease_id,
                context_cursor: 0,
            },
        })
        .unwrap();

    assert_eq!(running.state, SubagentActorState::Yielding);
    assert_eq!(running.workspace_lease_id, Some(lease_id));
}

#[test]
fn nonterminal_query_excludes_completed_children_and_recovery_is_cas_guarded() {
    let root = TempRoot::new("recovery-query");
    let store = TaskStore::open(root.path().join("tasks.sqlite")).unwrap();
    let parent_task_id = TaskId::new();
    let parent_segment_id = RunSegmentId::new();
    create_running_task(&store, parent_task_id, parent_segment_id);
    let parent_actor_id = store
        .task_projection(parent_task_id)
        .unwrap()
        .unwrap()
        .actor_id
        .unwrap();
    let request = create_request(
        &store,
        root.path(),
        parent_task_id,
        parent_segment_id,
        parent_actor_id,
    );
    store
        .create_subagent_actor_checked(request.clone())
        .unwrap();
    let lease_id = active_managed_lease(
        &store,
        request.child_task_id,
        request.actor_id,
        root.path().to_str().unwrap(),
    );
    store
        .apply_subagent_lifecycle(SubagentLifecycleCommand {
            parent_task_id,
            child_task_id: request.child_task_id,
            actor_id: request.actor_id,
            authority: SubagentLifecycleAuthority::Supervisor,
            transition: SubagentLifecycleTransition::Running {
                workspace_lease_id: lease_id,
                context_cursor: 0,
            },
        })
        .unwrap();

    let pending = store.nonterminal_subagent_actors().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].child_task_id, request.child_task_id);
    store
        .recover_subagent_actor(request.child_task_id, request.actor_id, Utc::now())
        .unwrap();
    assert!(store.nonterminal_subagent_actors().unwrap().is_empty());
    assert_eq!(
        store.workspace_lease(lease_id).unwrap().unwrap().state,
        harness_journal::TaskWorkspaceLeaseState::CleanupPending
    );
    assert_eq!(
        store
            .task_projection(request.child_task_id)
            .unwrap()
            .unwrap()
            .current_run
            .unwrap()
            .state,
        harness_contracts::RunState::Failed
    );
    let recovered_again = store
        .recover_subagent_actor(request.child_task_id, request.actor_id, Utc::now())
        .unwrap();
    assert_eq!(recovered_again.state, SubagentActorState::Failed);

    let events = store.task_events_after(parent_task_id, 0, 64).unwrap();
    let terminals = events
        .iter()
        .filter(|event| event.event_type == "subagent.terminal")
        .collect::<Vec<_>>();
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0].source.kind, EventSourceKind::Recovery);
}

#[test]
fn parent_stop_batch_is_atomic_when_any_child_fails_cas() {
    let root = TempRoot::new("parent-stop-atomic");
    let store = TaskStore::open(root.path().join("tasks.sqlite")).unwrap();
    let parent_task_id = TaskId::new();
    let parent_segment_id = RunSegmentId::new();
    create_running_task(&store, parent_task_id, parent_segment_id);
    let parent_actor_id = store
        .task_projection(parent_task_id)
        .unwrap()
        .unwrap()
        .actor_id
        .unwrap();
    let first = create_request(
        &store,
        root.path(),
        parent_task_id,
        parent_segment_id,
        parent_actor_id,
    );
    let second = create_request(
        &store,
        root.path(),
        parent_task_id,
        parent_segment_id,
        parent_actor_id,
    );
    for request in [&first, &second] {
        store
            .create_subagent_actor_checked(request.clone())
            .unwrap();
        let lease_id = active_managed_lease(
            &store,
            request.child_task_id,
            request.actor_id,
            root.path().to_str().unwrap(),
        );
        store
            .apply_subagent_lifecycle(SubagentLifecycleCommand {
                parent_task_id,
                child_task_id: request.child_task_id,
                actor_id: request.actor_id,
                authority: SubagentLifecycleAuthority::Supervisor,
                transition: SubagentLifecycleTransition::Running {
                    workspace_lease_id: lease_id,
                    context_cursor: 0,
                },
            })
            .unwrap();
    }

    let stale_batch = vec![
        ExpectedParentStopSubagent {
            child_task_id: first.child_task_id,
            actor_id: first.actor_id,
            expected_state: SubagentActorState::Running,
        },
        ExpectedParentStopSubagent {
            child_task_id: second.child_task_id,
            actor_id: second.actor_id,
            expected_state: SubagentActorState::Starting,
        },
    ];
    assert!(store
        .apply_parent_subagent_stop(
            parent_task_id,
            &stale_batch,
            ParentSubagentStopMode::SafePoint,
        )
        .is_err());
    let unchanged = store.task_projection(parent_task_id).unwrap().unwrap();
    assert!(unchanged
        .subagents
        .iter()
        .all(|child| child.state == SubagentActorState::Running));

    let expected = unchanged
        .subagents
        .iter()
        .map(|child| ExpectedParentStopSubagent {
            child_task_id: child.child_task_id,
            actor_id: child.actor_id,
            expected_state: child.state,
        })
        .collect::<Vec<_>>();
    let updated = store
        .apply_parent_subagent_stop(parent_task_id, &expected, ParentSubagentStopMode::SafePoint)
        .unwrap();
    assert_eq!(updated.len(), 2);
    assert!(updated
        .iter()
        .all(|child| child.state == SubagentActorState::Yielding));
}

#[test]
fn stale_terminal_child_does_not_block_force_stopping_other_attached_children() {
    let root = TempRoot::new("parent-stop-stale-terminal");
    let store = TaskStore::open(root.path().join("tasks.sqlite")).unwrap();
    let parent_task_id = TaskId::new();
    let parent_segment_id = RunSegmentId::new();
    create_running_task(&store, parent_task_id, parent_segment_id);
    let parent_actor_id = store
        .task_projection(parent_task_id)
        .unwrap()
        .unwrap()
        .actor_id
        .unwrap();
    let first = create_request(
        &store,
        root.path(),
        parent_task_id,
        parent_segment_id,
        parent_actor_id,
    );
    let second = create_request(
        &store,
        root.path(),
        parent_task_id,
        parent_segment_id,
        parent_actor_id,
    );
    for request in [&first, &second] {
        store
            .create_subagent_actor_checked(request.clone())
            .unwrap();
        let lease_id = active_managed_lease(
            &store,
            request.child_task_id,
            request.actor_id,
            root.path().to_str().unwrap(),
        );
        store
            .apply_subagent_lifecycle(SubagentLifecycleCommand {
                parent_task_id,
                child_task_id: request.child_task_id,
                actor_id: request.actor_id,
                authority: SubagentLifecycleAuthority::Supervisor,
                transition: SubagentLifecycleTransition::Running {
                    workspace_lease_id: lease_id,
                    context_cursor: 0,
                },
            })
            .unwrap();
    }
    store
        .apply_subagent_lifecycle(SubagentLifecycleCommand {
            parent_task_id,
            child_task_id: first.child_task_id,
            actor_id: first.actor_id,
            authority: SubagentLifecycleAuthority::Actor(first.actor_id),
            transition: SubagentLifecycleTransition::Completed {
                summary: RedactedSubagentSummary::new(&TokenRedactor, "done"),
                ended_at: Utc::now(),
            },
        })
        .unwrap();

    let updated = store
        .apply_parent_subagent_stop(
            parent_task_id,
            &[
                ExpectedParentStopSubagent {
                    child_task_id: first.child_task_id,
                    actor_id: first.actor_id,
                    expected_state: SubagentActorState::Running,
                },
                ExpectedParentStopSubagent {
                    child_task_id: second.child_task_id,
                    actor_id: second.actor_id,
                    expected_state: SubagentActorState::Running,
                },
            ],
            ParentSubagentStopMode::Force {
                ended_at: Utc::now(),
            },
        )
        .unwrap();

    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].child_task_id, second.child_task_id);
    assert_eq!(updated[0].state, SubagentActorState::Cancelled);
    let parent = store.task_projection(parent_task_id).unwrap().unwrap();
    assert_eq!(parent.subagents[0].state, SubagentActorState::Completed);
    assert_eq!(parent.subagents[1].state, SubagentActorState::Cancelled);
}

fn create_request(
    store: &TaskStore,
    root: &std::path::Path,
    parent_task_id: TaskId,
    parent_segment_id: RunSegmentId,
    parent_actor_id: ActorId,
) -> CreateSubagentActorRequest {
    let parent_workspace_lease_id = active_managed_lease(
        store,
        parent_task_id,
        parent_actor_id,
        root.to_str().unwrap(),
    );
    CreateSubagentActorRequest {
        child_task_id: TaskId::new(),
        actor_id: ActorId::new(),
        segment_id: RunSegmentId::new(),
        parent_task_id,
        parent_segment_id,
        parent_actor_id,
        parent_workspace_lease_id,
        delegation_id: SubagentId::new(),
        context_cursor: 0,
        title: "reviewer".into(),
        started_at: Utc::now(),
    }
}

fn create_running_task(store: &TaskStore, task_id: TaskId, segment_id: RunSegmentId) {
    let outcome = store
        .transact_command(command(task_id, 0, "create-running"), |_| {
            Ok(vec![
                NewTaskEvent::task_created("parent"),
                NewTaskEvent::run_started(segment_id, Utc::now()),
            ])
        })
        .unwrap();
    assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
}

fn create_task(store: &TaskStore, task_id: TaskId, title: &str) {
    store
        .transact_command(command(task_id, 0, "create"), |_| {
            Ok(vec![NewTaskEvent::task_created(title)])
        })
        .unwrap();
}

fn command(task_id: TaskId, expected_stream_version: u64, suffix: &str) -> AcceptedCommand {
    AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("{suffix}-{}", CommandId::new()),
        expected_stream_version,
        authority: TaskStore::supervisor_authority(),
        payload: json!({ "type": suffix }),
    }
}

fn active_managed_lease(
    store: &TaskStore,
    task_id: TaskId,
    actor_id: ActorId,
    root: &str,
) -> WorkspaceLeaseId {
    let lease_id = WorkspaceLeaseId::new();
    let outcome = store
        .acquire_workspace_lease(AcquireTaskWorkspaceLease {
            lease_id,
            task_id,
            actor_id,
            mode: WorkspaceMode::ManagedWorktree,
            canonical_root: root.into(),
            worktree_path: Some(format!("{root}/{lease_id}")),
            branch: Some(format!("jyowo/task-{lease_id}")),
            writable: true,
            requested_at: Utc::now(),
            expires_at: Some(Utc::now() + Duration::hours(1)),
            baseline_commit: Some("0123456789abcdef".into()),
            baseline_status: String::new(),
        })
        .unwrap();
    assert!(matches!(outcome, TaskWorkspaceAcquireOutcome::Acquired(_)));
    lease_id
}

struct TokenRedactor;

impl Redactor for TokenRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.replace("TOKEN", "[REDACTED]")
    }
}

struct TempRoot(std::path::PathBuf);

impl TempRoot {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "jyowo-subagent-store-{label}-{}-{}",
            std::process::id(),
            TaskId::new()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

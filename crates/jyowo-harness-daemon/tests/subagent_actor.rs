use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    ActorId, BlobId, BlobRef, ContentHash, CorrelationId, EventSourceKind, JournalOffset, Message,
    MessageId, MessagePart, MessageRole, RunId, RunSegmentId, SessionId, SubagentActorState,
    SubagentContextReport, SubagentStatus, TaskId, TaskState, TenantId, TranscriptRef, TurnInput,
    UnexpectedErrorEvent, UsageSnapshot, WorkspaceMode,
};
use harness_daemon::{
    SubagentParentBinding, SubagentStopMode, SubagentSupervisor, WorkspaceAccess,
    WorkspaceCoordinator, WorkspaceExecutionKind, WorkspaceLeaseRequest,
    WorkspaceSubagentRunContext, WorkspaceSubagentRunnerFactory,
};
use harness_journal::{
    AcceptedCommand, CommandOutcome, EventStore, NewTaskEvent, SubagentLifecycleAuthority,
    SubagentLifecycleCommand, SubagentLifecycleTransition, TaskStore, TaskWorkspaceLeaseState,
};
use harness_subagent::{
    ChildRunOutcome, ChildRunRequest, DefaultSubagentRunner, DelegationPolicy, ParentContext,
    SubagentAnnouncement, SubagentEngineFactory, SubagentError, SubagentHandle, SubagentRunner,
    SubagentSpec,
};
use tokio::sync::{Mutex, Notify, Semaphore};

#[tokio::test]
async fn nested_delegation_links_the_grandchild_to_the_child_workspace() {
    let root = tempfile::tempdir().unwrap();
    let workspace_root = root.path().join("workspace");
    std::fs::create_dir(&workspace_root).unwrap();
    init_git_repo(&workspace_root);
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let workspace = Arc::new(
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap(),
    );
    let contexts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory> =
        Arc::new(NestedDelegatingRunnerFactory {
            contexts: Arc::clone(&contexts),
        });
    let subagents = Arc::new(SubagentSupervisor::new(
        Arc::clone(&store),
        Arc::clone(&workspace),
        runner_factory,
        Arc::new(TokenRedactor),
        2,
        4,
    ));
    let (parent_task, parent_actor, parent_segment) = create_running_parent(&store, "parent");
    acquire_parent_workspace_at(
        &store,
        &workspace,
        &workspace_root,
        parent_task,
        parent_actor,
    );

    subagents
        .bind(SubagentParentBinding {
            parent_task_id: parent_task,
            parent_segment_id: parent_segment,
            parent_actor_id: parent_actor,
            depth: 0,
        })
        .spawn(
            SubagentSpec::minimal("child", "delegate once"),
            input("delegate once"),
            parent_context(0),
        )
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    let parent = store.task_projection(parent_task).unwrap().unwrap();
    let child = parent.subagents.first().unwrap();
    let child_task = store.task_projection(child.child_task_id).unwrap().unwrap();
    let grandchild = child_task.subagents.first().unwrap();
    assert_eq!(grandchild.parent_task_id, child.child_task_id);
    assert_eq!(grandchild.parent_segment_id, child.segment_id);

    let child_lease = store
        .workspace_lease(child.workspace_lease_id.unwrap())
        .unwrap()
        .unwrap();
    let grandchild_lease = store
        .workspace_lease(grandchild.workspace_lease_id.unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        Path::new(&grandchild_lease.canonical_root),
        Path::new(child_lease.worktree_path.as_deref().unwrap())
    );
    assert_ne!(
        Path::new(&grandchild_lease.canonical_root),
        workspace_root.as_path()
    );
    assert_eq!(contexts.lock().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_stop_cancels_a_durable_child_before_active_registration() {
    let root = tempfile::tempdir().unwrap();
    let workspace_root = root.path().join("workspace");
    std::fs::create_dir(&workspace_root).unwrap();
    init_git_repo(&workspace_root);
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let workspace = Arc::new(
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap(),
    );
    let entered = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    let runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory> = Arc::new(BlockingRunnerFactory {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let subagents = Arc::new(SubagentSupervisor::new(
        Arc::clone(&store),
        Arc::clone(&workspace),
        runner_factory,
        Arc::new(TokenRedactor),
        2,
        4,
    ));
    let (parent_task, parent_actor, parent_segment) = create_running_parent(&store, "parent");
    acquire_parent_workspace_at(
        &store,
        &workspace,
        &workspace_root,
        parent_task,
        parent_actor,
    );
    let runner = subagents.bind(SubagentParentBinding {
        parent_task_id: parent_task,
        parent_segment_id: parent_segment,
        parent_actor_id: parent_actor,
        depth: 0,
    });
    let running = tokio::spawn(async move {
        runner
            .spawn(
                SubagentSpec::minimal("child", "wait in factory"),
                input("wait in factory"),
                parent_context(0),
            )
            .await
    });
    tokio::task::spawn_blocking(move || entered.wait())
        .await
        .unwrap();

    subagents
        .request_parent_stop(parent_task, SubagentStopMode::Force)
        .unwrap();
    let state_after_stop = store
        .task_projection(parent_task)
        .unwrap()
        .unwrap()
        .subagents
        .first()
        .unwrap()
        .state;
    tokio::task::spawn_blocking(move || release.wait())
        .await
        .unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), running)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        state_after_stop,
        harness_contracts::SubagentActorState::Cancelled
    );
    assert!(matches!(result, Err(SubagentError::Cancelled)));
}

#[tokio::test]
async fn child_gets_an_independent_actor_stream_context_and_managed_workspace() {
    let fixture = Fixture::new(2, 4);
    let (parent_task, parent_actor, parent_segment) =
        create_running_parent(&fixture.store, "parent");
    acquire_parent_workspace(&fixture, parent_task, parent_actor);
    fixture
        .delegate
        .set_summary("TOKEN secret child summary with details")
        .await;
    let runner = fixture.subagents.bind(SubagentParentBinding {
        parent_task_id: parent_task,
        parent_segment_id: parent_segment,
        parent_actor_id: parent_actor,
        depth: 0,
    });

    let running = tokio::spawn(async move {
        runner
            .spawn(
                SubagentSpec::minimal("reviewer", "inspect"),
                input("inspect"),
                parent_context(0),
            )
            .await
    });
    fixture.delegate.wait_started().await;

    let parent = fixture.store.task_projection(parent_task).unwrap().unwrap();
    let child = parent
        .subagents
        .first()
        .expect("child is projected")
        .clone();
    assert_ne!(child.actor_id, parent_actor);
    assert_ne!(child.segment_id, parent_segment);
    assert_eq!(child.context_cursor, 0);
    assert_eq!(child.parent_task_id, parent_task);
    assert_eq!(child.parent_segment_id, parent_segment);
    assert_eq!(child.state, harness_contracts::SubagentActorState::Running);

    let child_projection = fixture
        .store
        .task_projection(child.child_task_id)
        .unwrap()
        .unwrap();
    assert_eq!(child_projection.actor_id, Some(child.actor_id));
    assert_eq!(child_projection.context_cursor, 0);
    assert_eq!(
        child_projection.parent.as_ref().unwrap().parent_task_id,
        parent_task
    );
    let child_events = fixture
        .store
        .task_events_after(child.child_task_id, 0, 32)
        .unwrap();
    assert!(child_events
        .iter()
        .all(|event| event.task_id == child.child_task_id));
    assert!(child_events
        .iter()
        .any(|event| event.event_type == "subagent.linked"));
    assert!(child_events
        .iter()
        .any(|event| event.event_type == "run.started"));
    assert!(child_events
        .iter()
        .any(|event| event.event_type == "engine.unexpected_error"));
    assert_eq!(
        child_projection
            .current_run
            .as_ref()
            .map(|run| run.segment_id),
        Some(child.segment_id)
    );
    assert!(child_events.iter().any(|event| {
        event.event_type == "subagent.linked"
            && event.source.kind == EventSourceKind::Subagent
            && event.source.actor_id == Some(child.actor_id)
    }));
    let leases = fixture
        .store
        .nonterminal_workspace_leases_for_task(child.child_task_id)
        .unwrap();
    assert_eq!(leases.len(), 1);
    assert_eq!(leases[0].mode, WorkspaceMode::ManagedWorktree);
    assert_eq!(leases[0].actor_id, child.actor_id);
    assert_eq!(leases[0].state, TaskWorkspaceLeaseState::Active);
    assert_eq!(
        fixture.delegate.workspace().await.as_deref(),
        leases[0].worktree_path.as_deref().map(Path::new)
    );

    fixture.delegate.complete_one();
    let handle = running.await.unwrap().unwrap();
    let announcement = handle.wait().await.unwrap();
    assert!(!announcement.summary.contains("TOKEN"));
    assert!(announcement.summary.chars().count() <= 256);
    let completed_child = fixture
        .store
        .task_projection(parent_task)
        .unwrap()
        .unwrap()
        .subagents
        .into_iter()
        .find(|projected| projected.child_task_id == child.child_task_id)
        .unwrap();
    assert_eq!(
        completed_child.summary.as_deref(),
        Some(announcement.summary.as_str())
    );
    let parent_events = fixture.store.task_events_after(parent_task, 0, 64).unwrap();
    assert!(parent_events.iter().all(|event| {
        !event.payload.to_string().contains("TOKEN") && event.source.kind != EventSourceKind::Engine
    }));
}

#[tokio::test]
async fn daemon_default_runner_uses_the_child_event_scope_and_session() {
    let fixture = Fixture::new(2, 4);
    let (parent_task, parent_actor, parent_segment) =
        create_running_parent(&fixture.store, "parent");
    acquire_parent_workspace(&fixture, parent_task, parent_actor);
    let engine = Arc::new(DaemonRecordingEngineFactory::default());
    let runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory> =
        Arc::new(DaemonDefaultRunnerFactory {
            engine: Arc::clone(&engine),
        });
    let subagents = Arc::new(SubagentSupervisor::new(
        Arc::clone(&fixture.store),
        Arc::clone(&fixture.workspace),
        runner_factory,
        Arc::new(TokenRedactor),
        2,
        4,
    ));
    let runner = subagents.bind(SubagentParentBinding {
        parent_task_id: parent_task,
        parent_segment_id: parent_segment,
        parent_actor_id: parent_actor,
        depth: 0,
    });

    let announcement = runner
        .spawn(
            SubagentSpec::minimal("reviewer", "inspect"),
            input("inspect"),
            parent_context(0),
        )
        .await
        .unwrap()
        .wait()
        .await
        .unwrap();

    assert_eq!(announcement.status, SubagentStatus::Completed);
    let expected_session = engine.expected_session.lock().unwrap().unwrap();
    let request = engine.request.lock().await.clone().unwrap();
    assert_eq!(request.child_session_id, expected_session);
    let parent = fixture.store.task_projection(parent_task).unwrap().unwrap();
    let child = parent.subagents.first().unwrap();
    assert!(fixture
        .store
        .task_events_after(child.child_task_id, 0, 64)
        .unwrap()
        .iter()
        .any(|event| event.event_type == "engine.session_created"));
}

#[tokio::test]
async fn depth_and_global_child_quotas_are_enforced_before_delegate_execution() {
    let fixture = Fixture::new(1, 1);
    let (parent_task, parent_actor, parent_segment) =
        create_running_parent(&fixture.store, "parent");
    acquire_parent_workspace(&fixture, parent_task, parent_actor);
    let too_deep = fixture.subagents.bind(SubagentParentBinding {
        parent_task_id: parent_task,
        parent_segment_id: parent_segment,
        parent_actor_id: parent_actor,
        depth: 1,
    });
    assert!(matches!(
        too_deep
            .spawn(
                SubagentSpec::minimal("deep", "blocked"),
                input("blocked"),
                parent_context(1),
            )
            .await,
        Err(SubagentError::DepthExceeded { .. })
    ));

    let runner = fixture.subagents.bind(SubagentParentBinding {
        parent_task_id: parent_task,
        parent_segment_id: parent_segment,
        parent_actor_id: parent_actor,
        depth: 0,
    });
    let first_runner = Arc::clone(&runner);
    let first = tokio::spawn(async move {
        first_runner
            .spawn(
                SubagentSpec::minimal("first", "wait"),
                input("wait"),
                parent_context(0),
            )
            .await
    });
    fixture.delegate.wait_started().await;
    assert!(matches!(
        runner
            .spawn(
                SubagentSpec::minimal("second", "blocked"),
                input("blocked"),
                parent_context(0),
            )
            .await,
        Err(SubagentError::ConcurrentLimitExceeded)
    ));
    fixture.delegate.complete_one();
    first.await.unwrap().unwrap();
}

#[tokio::test]
async fn safe_stop_propagates_but_detached_children_continue_in_background() {
    let fixture = Fixture::new(2, 2);
    let (parent_task, parent_actor, parent_segment) =
        create_running_parent(&fixture.store, "parent");
    acquire_parent_workspace(&fixture, parent_task, parent_actor);
    let runner = fixture.subagents.bind(SubagentParentBinding {
        parent_task_id: parent_task,
        parent_segment_id: parent_segment,
        parent_actor_id: parent_actor,
        depth: 0,
    });

    let first_runner = Arc::clone(&runner);
    let first = tokio::spawn(async move {
        first_runner
            .spawn(
                SubagentSpec::minimal("first", "wait"),
                input("wait"),
                parent_context(0),
            )
            .await
    });
    fixture.delegate.wait_started().await;
    let first_child = fixture
        .store
        .task_projection(parent_task)
        .unwrap()
        .unwrap()
        .subagents[0]
        .child_task_id;
    fixture
        .subagents
        .request_parent_stop(parent_task, SubagentStopMode::SafePoint)
        .unwrap();
    tokio::time::timeout(
        std::time::Duration::from_millis(250),
        fixture.delegate.wait_yield_requested(),
    )
    .await
    .expect("safe-stop reaches the running child execution channel");
    assert!(fixture
        .store
        .task_events_after(parent_task, 0, 64)
        .unwrap()
        .iter()
        .any(|event| {
            event.event_type == "subagent.state_changed"
                && event
                    .payload
                    .get("childTaskId")
                    .and_then(|value| value.as_str())
                    == Some(first_child.to_string().as_str())
                && event.payload.get("state").and_then(|value| value.as_str()) == Some("yielding")
        }));
    first.await.unwrap().unwrap();

    let second_runner = Arc::clone(&runner);
    let second = tokio::spawn(async move {
        second_runner
            .spawn(
                SubagentSpec::minimal("second", "background"),
                input("background"),
                parent_context(0),
            )
            .await
    });
    fixture.delegate.wait_started().await;
    let second_child = fixture
        .store
        .task_projection(parent_task)
        .unwrap()
        .unwrap()
        .subagents
        .into_iter()
        .find(|child| child.child_task_id != first_child)
        .unwrap()
        .child_task_id;
    fixture
        .subagents
        .continue_in_background(parent_task, second_child)
        .unwrap();
    fixture
        .subagents
        .request_parent_stop(parent_task, SubagentStopMode::Force)
        .unwrap();
    assert_eq!(
        child_state(&fixture.store, parent_task, second_child),
        harness_contracts::SubagentActorState::Background
    );
    fixture.delegate.complete_one();
    second.await.unwrap().unwrap();
}

#[tokio::test]
async fn parent_stop_skips_a_durably_detached_stale_active_snapshot() {
    let fixture = Fixture::new(2, 2);
    let (parent_task, parent_actor, parent_segment) =
        create_running_parent(&fixture.store, "parent");
    acquire_parent_workspace(&fixture, parent_task, parent_actor);
    let runner = fixture.subagents.bind(SubagentParentBinding {
        parent_task_id: parent_task,
        parent_segment_id: parent_segment,
        parent_actor_id: parent_actor,
        depth: 0,
    });

    let running = tokio::spawn(async move {
        runner
            .spawn(
                SubagentSpec::minimal("background", "continue"),
                input("continue"),
                parent_context(0),
            )
            .await
    });
    fixture.delegate.wait_started().await;
    let child = fixture
        .store
        .task_projection(parent_task)
        .unwrap()
        .unwrap()
        .subagents[0]
        .clone();
    fixture
        .store
        .apply_subagent_lifecycle(SubagentLifecycleCommand {
            parent_task_id: parent_task,
            child_task_id: child.child_task_id,
            actor_id: child.actor_id,
            authority: SubagentLifecycleAuthority::Supervisor,
            transition: SubagentLifecycleTransition::Background,
        })
        .unwrap();

    assert!(fixture
        .subagents
        .request_parent_stop(parent_task, SubagentStopMode::Force)
        .is_ok());
    assert_eq!(
        child_state(&fixture.store, parent_task, child.child_task_id),
        harness_contracts::SubagentActorState::Background
    );

    fixture.delegate.complete_one();
    running.await.unwrap().unwrap();
}

#[tokio::test]
async fn detached_child_finalizes_after_the_parent_drops_its_spawn_future() {
    let fixture = Fixture::new(2, 2);
    let (parent_task, parent_actor, parent_segment) =
        create_running_parent(&fixture.store, "parent");
    acquire_parent_workspace(&fixture, parent_task, parent_actor);
    let runner = fixture.subagents.bind(SubagentParentBinding {
        parent_task_id: parent_task,
        parent_segment_id: parent_segment,
        parent_actor_id: parent_actor,
        depth: 0,
    });

    let caller = tokio::spawn(async move {
        runner
            .spawn(
                SubagentSpec::minimal("background", "continue"),
                input("continue"),
                parent_context(0),
            )
            .await
    });
    fixture.delegate.wait_started().await;
    let child = fixture
        .store
        .task_projection(parent_task)
        .unwrap()
        .unwrap()
        .subagents[0]
        .clone();
    fixture
        .subagents
        .continue_in_background(parent_task, child.child_task_id)
        .unwrap();

    caller.abort();
    let _ = caller.await;
    fixture.delegate.complete_one();

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let projected = fixture
                .store
                .task_projection(parent_task)
                .unwrap()
                .unwrap()
                .subagents
                .into_iter()
                .find(|projected| projected.child_task_id == child.child_task_id)
                .unwrap();
            let leases = fixture
                .store
                .nonterminal_workspace_leases_for_task(child.child_task_id)
                .unwrap();
            if projected.state == harness_contracts::SubagentActorState::Completed
                && leases.is_empty()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("detached finalizer persists completion and releases the workspace");
}

#[tokio::test]
async fn parent_announcement_uses_the_durable_id_and_withholds_child_artifacts() {
    let fixture = Fixture::new(2, 2);
    let (parent_task, parent_actor, parent_segment) =
        create_running_parent(&fixture.store, "parent");
    acquire_parent_workspace(&fixture, parent_task, parent_actor);
    fixture.delegate.set_summary("TOKEN parent summary").await;
    fixture.delegate.return_child_artifacts().await;
    let runner = fixture.subagents.bind(SubagentParentBinding {
        parent_task_id: parent_task,
        parent_segment_id: parent_segment,
        parent_actor_id: parent_actor,
        depth: 0,
    });

    let running = tokio::spawn(async move {
        runner
            .spawn(
                SubagentSpec::minimal("reviewer", "inspect"),
                input("inspect"),
                parent_context(0),
            )
            .await
    });
    fixture.delegate.wait_started().await;
    let child = fixture
        .store
        .task_projection(parent_task)
        .unwrap()
        .unwrap()
        .subagents[0]
        .clone();
    fixture.delegate.complete_one();
    let announcement = running.await.unwrap().unwrap().wait().await.unwrap();

    assert_eq!(announcement.subagent_id, child.delegation_id);
    assert_eq!(announcement.summary, "[REDACTED] parent summary");
    assert_eq!(announcement.result, None);
    assert_eq!(announcement.transcript_ref, None);
    assert_eq!(announcement.context_report, None);
}

#[tokio::test]
async fn announcement_status_controls_the_child_lifecycle_and_run_terminal() {
    for (status, expected_actor, expected_run) in [
        (
            SubagentStatus::Completed,
            harness_contracts::SubagentActorState::Completed,
            harness_contracts::RunState::Completed,
        ),
        (
            SubagentStatus::Cancelled,
            harness_contracts::SubagentActorState::Cancelled,
            harness_contracts::RunState::Interrupted,
        ),
        (
            SubagentStatus::Failed,
            harness_contracts::SubagentActorState::Failed,
            harness_contracts::RunState::Failed,
        ),
    ] {
        let fixture = Fixture::new(2, 2);
        let (parent_task, parent_actor, parent_segment) =
            create_running_parent(&fixture.store, "parent");
        acquire_parent_workspace(&fixture, parent_task, parent_actor);
        fixture.delegate.set_status(status.clone()).await;
        let runner = fixture.subagents.bind(SubagentParentBinding {
            parent_task_id: parent_task,
            parent_segment_id: parent_segment,
            parent_actor_id: parent_actor,
            depth: 0,
        });
        let running = tokio::spawn(async move {
            runner
                .spawn(
                    SubagentSpec::minimal("reviewer", "inspect"),
                    input("inspect"),
                    parent_context(0),
                )
                .await
        });
        fixture.delegate.wait_started().await;
        let child_task_id = fixture
            .store
            .task_projection(parent_task)
            .unwrap()
            .unwrap()
            .subagents[0]
            .child_task_id;
        fixture.delegate.complete_one();
        let announcement = running.await.unwrap().unwrap().wait().await.unwrap();
        assert_eq!(announcement.status, status);
        assert_eq!(
            child_state(&fixture.store, parent_task, child_task_id),
            expected_actor
        );
        let child = fixture
            .store
            .task_projection(child_task_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            child.current_run.as_ref().map(|run| run.state.clone()),
            Some(expected_run)
        );
    }
}

#[tokio::test]
async fn a_child_panic_is_recorded_without_failing_the_parent_task() {
    let fixture = Fixture::new(2, 2);
    let (parent_task, parent_actor, parent_segment) =
        create_running_parent(&fixture.store, "parent");
    acquire_parent_workspace(&fixture, parent_task, parent_actor);
    fixture.delegate.panic_next().await;
    let runner = fixture.subagents.bind(SubagentParentBinding {
        parent_task_id: parent_task,
        parent_segment_id: parent_segment,
        parent_actor_id: parent_actor,
        depth: 0,
    });

    assert!(runner
        .spawn(
            SubagentSpec::minimal("panic", "panic"),
            input("panic"),
            parent_context(0),
        )
        .await
        .is_err());

    let parent = fixture.store.task_projection(parent_task).unwrap().unwrap();
    assert_eq!(parent.state, TaskState::Running);
    assert_eq!(parent.subagents.len(), 1);
    assert_eq!(
        parent.subagents[0].state,
        harness_contracts::SubagentActorState::Failed
    );
    assert!(fixture
        .store
        .nonterminal_workspace_leases_for_task(parent.subagents[0].child_task_id)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn a_runner_factory_panic_is_recorded_without_unwinding_the_parent_task() {
    let root = tempfile::tempdir().unwrap();
    let workspace_root = root.path().join("workspace");
    std::fs::create_dir(&workspace_root).unwrap();
    init_git_repo(&workspace_root);
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let workspace = Arc::new(
        WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
            .unwrap(),
    );
    let factory: Arc<dyn WorkspaceSubagentRunnerFactory> = Arc::new(PanickingRunnerFactory);
    let subagents = Arc::new(SubagentSupervisor::new(
        Arc::clone(&store),
        Arc::clone(&workspace),
        factory,
        Arc::new(TokenRedactor),
        2,
        2,
    ));
    let (parent_task, parent_actor, parent_segment) = create_running_parent(&store, "parent");
    acquire_parent_workspace_at(
        &store,
        &workspace,
        &workspace_root,
        parent_task,
        parent_actor,
    );
    let runner = subagents.bind(SubagentParentBinding {
        parent_task_id: parent_task,
        parent_segment_id: parent_segment,
        parent_actor_id: parent_actor,
        depth: 0,
    });

    let result = tokio::spawn(async move {
        runner
            .spawn(
                SubagentSpec::minimal("panic", "panic in factory"),
                input("panic in factory"),
                parent_context(0),
            )
            .await
    })
    .await;

    assert!(matches!(result, Ok(Err(_))));
    let parent = store.task_projection(parent_task).unwrap().unwrap();
    assert_eq!(parent.state, TaskState::Running);
    assert_eq!(parent.subagents[0].state, SubagentActorState::Failed);
    assert!(store
        .nonterminal_workspace_leases_for_task(parent.subagents[0].child_task_id)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn child_failures_return_only_a_bounded_redacted_error_to_the_parent() {
    let fixture = Fixture::new(2, 2);
    let (parent_task, parent_actor, parent_segment) =
        create_running_parent(&fixture.store, "parent");
    acquire_parent_workspace(&fixture, parent_task, parent_actor);
    let injected_metric: &'static str =
        Box::leak(format!("TOKEN /private/child/path {}", "x".repeat(400)).into_boxed_str());
    fixture
        .delegate
        .fail_with(SubagentError::QuotaExceeded {
            metric: injected_metric,
            observed: 2,
            limit: 1,
        })
        .await;
    let runner = fixture.subagents.bind(SubagentParentBinding {
        parent_task_id: parent_task,
        parent_segment_id: parent_segment,
        parent_actor_id: parent_actor,
        depth: 0,
    });
    let running = tokio::spawn(async move {
        runner
            .spawn(
                SubagentSpec::minimal("reviewer", "inspect"),
                input("inspect"),
                parent_context(0),
            )
            .await
    });
    fixture.delegate.wait_started().await;
    fixture.delegate.complete_one();

    let error = running.await.unwrap().unwrap_err().to_string();
    assert!(!error.contains("TOKEN"));
    assert!(!error.contains("/private/child/path"));
    assert!(error.chars().count() <= 256 + "engine: ".len());
}

#[tokio::test]
async fn a_workspace_is_released_when_recording_the_running_child_fails() {
    let fixture = Fixture::new(2, 2);
    let (parent_task, parent_actor, parent_segment) =
        create_running_parent(&fixture.store, "parent");
    acquire_parent_workspace(&fixture, parent_task, parent_actor);
    fixture.fail_running_lifecycle_writes();
    let runner = fixture.subagents.bind(SubagentParentBinding {
        parent_task_id: parent_task,
        parent_segment_id: parent_segment,
        parent_actor_id: parent_actor,
        depth: 0,
    });

    assert!(runner
        .spawn(
            SubagentSpec::minimal("reviewer", "inspect"),
            input("inspect"),
            parent_context(0),
        )
        .await
        .is_err());

    let child_task_id = fixture
        .store
        .task_projection(parent_task)
        .unwrap()
        .unwrap()
        .subagents[0]
        .child_task_id;
    assert_eq!(
        child_state(&fixture.store, parent_task, child_task_id),
        harness_contracts::SubagentActorState::Failed
    );
    assert_eq!(
        fixture
            .store
            .task_projection(child_task_id)
            .unwrap()
            .unwrap()
            .current_run
            .unwrap()
            .state,
        harness_contracts::RunState::Failed
    );
    assert!(fixture
        .store
        .nonterminal_workspace_leases_for_task(child_task_id)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn a_terminal_write_failure_keeps_the_workspace_lease_recoverable() {
    let fixture = Fixture::new(2, 2);
    let (parent_task, parent_actor, parent_segment) =
        create_running_parent(&fixture.store, "parent");
    acquire_parent_workspace(&fixture, parent_task, parent_actor);
    let runner = fixture.subagents.bind(SubagentParentBinding {
        parent_task_id: parent_task,
        parent_segment_id: parent_segment,
        parent_actor_id: parent_actor,
        depth: 0,
    });

    let running = tokio::spawn(async move {
        runner
            .spawn(
                SubagentSpec::minimal("reviewer", "inspect"),
                input("inspect"),
                parent_context(0),
            )
            .await
    });
    fixture.delegate.wait_started().await;
    let child = fixture
        .store
        .task_projection(parent_task)
        .unwrap()
        .unwrap()
        .subagents[0]
        .clone();
    let lease_id = child.workspace_lease_id.unwrap();
    fixture.fail_terminal_lifecycle_writes();
    fixture.delegate.complete_one();

    assert!(running.await.unwrap().is_err());
    assert_eq!(
        child_state(&fixture.store, parent_task, child.child_task_id),
        harness_contracts::SubagentActorState::Running
    );
    assert_eq!(
        fixture
            .store
            .workspace_lease(lease_id)
            .unwrap()
            .unwrap()
            .state,
        TaskWorkspaceLeaseState::Active
    );
}

struct Fixture {
    _root: tempfile::TempDir,
    workspace_root: std::path::PathBuf,
    store: Arc<TaskStore>,
    workspace: Arc<WorkspaceCoordinator>,
    delegate: Arc<ControlledRunner>,
    subagents: Arc<SubagentSupervisor>,
}

impl Fixture {
    fn new(max_depth: u8, max_global: usize) -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace_root = root.path().join("workspace");
        std::fs::create_dir(&workspace_root).unwrap();
        init_git_repo(&workspace_root);
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        let workspace = Arc::new(
            WorkspaceCoordinator::new(Arc::clone(&store), root.path().join("managed-worktrees"))
                .unwrap(),
        );
        let delegate = Arc::new(ControlledRunner::default());
        let runner_factory: Arc<dyn WorkspaceSubagentRunnerFactory> =
            Arc::new(RecordingRunnerFactory {
                delegate: Arc::clone(&delegate),
            });
        let subagents = Arc::new(SubagentSupervisor::new(
            Arc::clone(&store),
            Arc::clone(&workspace),
            runner_factory,
            Arc::new(TokenRedactor),
            max_depth,
            max_global,
        ));
        Self {
            _root: root,
            workspace_root,
            store,
            workspace,
            delegate,
            subagents,
        }
    }

    fn fail_running_lifecycle_writes(&self) {
        let connection = rusqlite::Connection::open(self.store.database_path()).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER fail_running_subagent_lifecycle
                 BEFORE INSERT ON event_log
                 WHEN NEW.event_type = 'subagent.state_changed'
                  AND json_extract(NEW.payload_json, '$.state') = 'running'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected running lifecycle failure');
                 END;",
            )
            .unwrap();
    }

    fn fail_terminal_lifecycle_writes(&self) {
        let connection = rusqlite::Connection::open(self.store.database_path()).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER fail_terminal_subagent_lifecycle
                 BEFORE INSERT ON event_log
                 WHEN NEW.event_type = 'subagent.terminal'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected terminal lifecycle failure');
                 END;",
            )
            .unwrap();
    }
}

struct ControlledRunner {
    started: Semaphore,
    completed: Semaphore,
    summary: Mutex<String>,
    panic: Mutex<bool>,
    changed: Notify,
    workspace: Mutex<Option<std::path::PathBuf>>,
    yield_requested: Semaphore,
    return_artifacts: Mutex<bool>,
    status: Mutex<SubagentStatus>,
    failure: Mutex<Option<SubagentError>>,
}

impl Default for ControlledRunner {
    fn default() -> Self {
        Self {
            started: Semaphore::new(0),
            completed: Semaphore::new(0),
            summary: Mutex::new(String::new()),
            panic: Mutex::new(false),
            changed: Notify::new(),
            workspace: Mutex::new(None),
            yield_requested: Semaphore::new(0),
            return_artifacts: Mutex::new(false),
            status: Mutex::new(SubagentStatus::Completed),
            failure: Mutex::new(None),
        }
    }
}

impl ControlledRunner {
    async fn wait_started(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.started.acquire())
            .await
            .expect("child delegate did not start")
            .unwrap()
            .forget();
    }

    fn complete_one(&self) {
        self.completed.add_permits(1);
        self.changed.notify_waiters();
    }

    async fn set_summary(&self, summary: &str) {
        *self.summary.lock().await = summary.to_owned();
    }

    async fn panic_next(&self) {
        *self.panic.lock().await = true;
    }

    async fn workspace(&self) -> Option<std::path::PathBuf> {
        self.workspace.lock().await.clone()
    }

    async fn wait_yield_requested(&self) {
        self.yield_requested.acquire().await.unwrap().forget();
    }

    async fn return_child_artifacts(&self) {
        *self.return_artifacts.lock().await = true;
    }

    async fn set_status(&self, status: SubagentStatus) {
        *self.status.lock().await = status;
    }

    async fn fail_with(&self, error: SubagentError) {
        self.failure.lock().await.replace(error);
    }

    async fn run(
        &self,
        parent_ctx: ParentContext,
        control: Option<harness_subagent::SubagentCancellationToken>,
    ) -> Result<SubagentHandle, SubagentError> {
        self.started.add_permits(1);
        if std::mem::take(&mut *self.panic.lock().await) {
            panic!("child actor panic");
        }
        if let Some(control) = control {
            tokio::select! {
                permit = self.completed.acquire() => permit.unwrap().forget(),
                () = control.yield_requested() => {
                    self.yield_requested.add_permits(1);
                }
                () = control.cancelled() => return Err(SubagentError::Cancelled),
            }
        } else {
            self.completed.acquire().await.unwrap().forget();
        }
        if let Some(error) = self.failure.lock().await.take() {
            return Err(error);
        }
        let return_artifacts = *self.return_artifacts.lock().await;
        Ok(SubagentHandle::ready(SubagentAnnouncement {
            subagent_id: harness_contracts::SubagentId::new(),
            parent_session_id: parent_ctx.parent_session_id,
            status: self.status.lock().await.clone(),
            summary: self.summary.lock().await.clone(),
            result: return_artifacts.then(|| serde_json::json!({ "secret": "TOKEN result" })),
            usage: UsageSnapshot::default(),
            transcript_ref: return_artifacts.then(|| TranscriptRef {
                blob: BlobRef {
                    id: BlobId::new(),
                    size: 6,
                    content_hash: [7; 32],
                    content_type: Some("application/json".into()),
                },
                from_offset: JournalOffset(1),
                to_offset: JournalOffset(2),
            }),
            context_report: return_artifacts.then(|| SubagentContextReport {
                parent_system_hash: Some(ContentHash([1; 32])),
                child_system_hash: ContentHash([2; 32]),
                shared_system_prefix_hash: None,
                prompt_cache_prefix_reused: false,
                bootstrap_files_inherited: vec!["TOKEN/AGENTS.md".into()],
                system_header_extra_applied: false,
            }),
        }))
    }
}

struct RecordingRunnerFactory {
    delegate: Arc<ControlledRunner>,
}

struct NestedDelegatingRunnerFactory {
    contexts: Arc<std::sync::Mutex<Vec<(TaskId, std::path::PathBuf)>>>,
}

impl WorkspaceSubagentRunnerFactory for NestedDelegatingRunnerFactory {
    fn create(
        &self,
        context: WorkspaceSubagentRunContext,
    ) -> Result<Arc<dyn SubagentRunner>, SubagentError> {
        let mut contexts = self.contexts.lock().unwrap();
        contexts.push((context.child_task_id, context.workspace_root.clone()));
        let delegates = contexts.len() == 1;
        drop(contexts);
        Ok(Arc::new(NestedDelegatingRunner {
            nested: context.subagent_runner,
            delegates,
        }))
    }
}

struct NestedDelegatingRunner {
    nested: Arc<dyn SubagentRunner>,
    delegates: bool,
}

#[async_trait]
impl SubagentRunner for NestedDelegatingRunner {
    async fn spawn(
        &self,
        _spec: SubagentSpec,
        _input: TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, SubagentError> {
        if self.delegates {
            self.nested
                .spawn(
                    SubagentSpec::minimal("grandchild", "nested work"),
                    input("nested work"),
                    parent_context(parent_ctx.depth.saturating_add(1)),
                )
                .await?
                .wait()
                .await?;
        }
        Ok(ready_subagent_handle(parent_ctx, "nested complete"))
    }
}

struct BlockingRunnerFactory {
    entered: Arc<std::sync::Barrier>,
    release: Arc<std::sync::Barrier>,
}

struct PanickingRunnerFactory;

impl WorkspaceSubagentRunnerFactory for PanickingRunnerFactory {
    fn create(
        &self,
        _context: WorkspaceSubagentRunContext,
    ) -> Result<Arc<dyn SubagentRunner>, SubagentError> {
        panic!("injected runner factory panic")
    }
}

impl WorkspaceSubagentRunnerFactory for BlockingRunnerFactory {
    fn create(
        &self,
        _context: WorkspaceSubagentRunContext,
    ) -> Result<Arc<dyn SubagentRunner>, SubagentError> {
        self.entered.wait();
        self.release.wait();
        Ok(Arc::new(InstantRunner))
    }
}

struct InstantRunner;

#[async_trait]
impl SubagentRunner for InstantRunner {
    async fn spawn(
        &self,
        _spec: SubagentSpec,
        _input: TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, SubagentError> {
        Ok(ready_subagent_handle(parent_ctx, "complete"))
    }
}

fn ready_subagent_handle(parent_ctx: ParentContext, summary: &str) -> SubagentHandle {
    SubagentHandle::ready(SubagentAnnouncement {
        subagent_id: harness_contracts::SubagentId::new(),
        parent_session_id: parent_ctx.parent_session_id,
        status: SubagentStatus::Completed,
        summary: summary.to_owned(),
        result: None,
        usage: UsageSnapshot::default(),
        transcript_ref: None,
        context_report: None,
    })
}

struct DaemonDefaultRunnerFactory {
    engine: Arc<DaemonRecordingEngineFactory>,
}

impl WorkspaceSubagentRunnerFactory for DaemonDefaultRunnerFactory {
    fn create(
        &self,
        context: WorkspaceSubagentRunContext,
    ) -> Result<Arc<dyn SubagentRunner>, SubagentError> {
        self.engine
            .expected_session
            .lock()
            .unwrap()
            .replace(context.session_id);
        Ok(Arc::new(
            DefaultSubagentRunner::new_with_engine_factory(
                self.engine.clone(),
                context.event_store,
                context.workspace_root,
                DelegationPolicy::default(),
            )
            .with_child_session_id(context.session_id)
            .with_external_lifecycle_owner(),
        ))
    }
}

#[derive(Default)]
struct DaemonRecordingEngineFactory {
    expected_session: std::sync::Mutex<Option<SessionId>>,
    request: Mutex<Option<ChildRunRequest>>,
}

#[async_trait]
impl SubagentEngineFactory for DaemonRecordingEngineFactory {
    async fn run_child_engine(
        &self,
        request: ChildRunRequest,
    ) -> Result<ChildRunOutcome, SubagentError> {
        assert_eq!(
            request.child_session_id,
            self.expected_session.lock().unwrap().unwrap()
        );
        self.request.lock().await.replace(request);
        Ok(ChildRunOutcome {
            status: SubagentStatus::Completed,
            summary: "default runner completed".to_owned(),
            result: None,
            usage: UsageSnapshot::default(),
            transcript_ref: None,
            context_report: None,
        })
    }
}

impl WorkspaceSubagentRunnerFactory for RecordingRunnerFactory {
    fn create(
        &self,
        context: WorkspaceSubagentRunContext,
    ) -> Result<Arc<dyn SubagentRunner>, SubagentError> {
        Ok(Arc::new(WorkspaceRecordingRunner {
            delegate: Arc::clone(&self.delegate),
            workspace_root: context.workspace_root,
            event_store: context.event_store,
            tenant_id: context.tenant_id,
            session_id: context.session_id,
        }))
    }
}

struct WorkspaceRecordingRunner {
    delegate: Arc<ControlledRunner>,
    workspace_root: std::path::PathBuf,
    event_store: Arc<dyn EventStore>,
    tenant_id: TenantId,
    session_id: harness_contracts::SessionId,
}

#[async_trait]
impl SubagentRunner for WorkspaceRecordingRunner {
    async fn spawn(
        &self,
        spec: SubagentSpec,
        input: TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, SubagentError> {
        *self.delegate.workspace.lock().await = Some(self.workspace_root.clone());
        self.delegate.spawn(spec, input, parent_ctx).await
    }

    async fn spawn_controlled(
        &self,
        spec: SubagentSpec,
        input: TurnInput,
        parent_ctx: ParentContext,
        control: harness_subagent::SubagentCancellationToken,
    ) -> Result<SubagentHandle, SubagentError> {
        *self.delegate.workspace.lock().await = Some(self.workspace_root.clone());
        self.event_store
            .append(
                self.tenant_id,
                self.session_id,
                &[harness_contracts::Event::UnexpectedError(
                    UnexpectedErrorEvent {
                        session_id: Some(self.session_id),
                        run_id: None,
                        error: "child engine marker".into(),
                        at: harness_contracts::now(),
                    },
                )],
            )
            .await
            .map_err(|error| SubagentError::Engine(error.to_string()))?;
        self.delegate
            .spawn_controlled(spec, input, parent_ctx, control)
            .await
    }
}

#[async_trait]
impl SubagentRunner for ControlledRunner {
    async fn spawn(
        &self,
        _spec: SubagentSpec,
        _input: TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, SubagentError> {
        self.run(parent_ctx, None).await
    }

    async fn spawn_controlled(
        &self,
        _spec: SubagentSpec,
        _input: TurnInput,
        parent_ctx: ParentContext,
        control: harness_subagent::SubagentCancellationToken,
    ) -> Result<SubagentHandle, SubagentError> {
        self.run(parent_ctx, Some(control)).await
    }
}

struct TokenRedactor;

impl harness_contracts::Redactor for TokenRedactor {
    fn redact(&self, input: &str, _rules: &harness_contracts::RedactRules) -> String {
        input.replace("TOKEN", "[REDACTED]")
    }
}

fn acquire_parent_workspace(fixture: &Fixture, task_id: TaskId, actor_id: ActorId) {
    acquire_parent_workspace_at(
        &fixture.store,
        &fixture.workspace,
        &fixture.workspace_root,
        task_id,
        actor_id,
    );
}

fn acquire_parent_workspace_at(
    _store: &TaskStore,
    workspace: &WorkspaceCoordinator,
    workspace_root: &Path,
    task_id: TaskId,
    actor_id: ActorId,
) {
    let lease = workspace
        .acquire(WorkspaceLeaseRequest {
            task_id,
            actor_id,
            root: workspace_root.to_path_buf(),
            mode: Some(WorkspaceMode::Current),
            access: WorkspaceAccess::Write,
            execution_kind: WorkspaceExecutionKind::Foreground,
            expires_at: None,
        })
        .unwrap();
    assert!(matches!(
        lease,
        harness_daemon::WorkspaceAcquireOutcome::Acquired(_)
    ));
}

fn child_state(
    store: &TaskStore,
    parent_task_id: TaskId,
    child_task_id: TaskId,
) -> harness_contracts::SubagentActorState {
    store
        .task_projection(parent_task_id)
        .unwrap()
        .unwrap()
        .subagents
        .into_iter()
        .find(|child| child.child_task_id == child_task_id)
        .unwrap()
        .state
}

fn create_running_parent(store: &TaskStore, title: &str) -> (TaskId, ActorId, RunSegmentId) {
    let task_id = TaskId::new();
    let segment_id = RunSegmentId::new();
    let outcome = store
        .transact_command(
            AcceptedCommand {
                command_id: harness_contracts::CommandId::new(),
                task_id,
                idempotency_key: format!("create-{task_id}"),
                expected_stream_version: 0,
                authority: TaskStore::supervisor_authority(),
                payload: serde_json::json!({ "type": "create_task", "title": title }),
            },
            |_| {
                Ok(vec![
                    NewTaskEvent::task_created(title),
                    NewTaskEvent::run_started(segment_id, harness_contracts::now()),
                ])
            },
        )
        .unwrap();
    assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
    let actor_id = store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .actor_id
        .unwrap();
    (task_id, actor_id, segment_id)
}

fn parent_context(depth: u8) -> ParentContext {
    ParentContext {
        tenant_id: TenantId::SINGLE,
        parent_session_id: harness_contracts::SessionId::new(),
        parent_run_id: RunId::new(),
        depth,
        sibling_count: 0,
        trigger_tool_use_id: None,
        correlation_id: CorrelationId::new(),
        team_id: None,
        team_member_profile_id: None,
    }
}

fn input(text: &str) -> TurnInput {
    TurnInput {
        message: Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(text.to_owned())],
            created_at: harness_contracts::now(),
        },
        metadata: serde_json::Value::Null,
    }
}

fn init_git_repo(path: &Path) {
    git(path, ["init"]);
    git(path, ["config", "user.email", "test@example.com"]);
    git(path, ["config", "user.name", "Test User"]);
    std::fs::write(path.join("README.md"), "baseline\n").unwrap();
    git(path, ["add", "README.md"]);
    git(path, ["commit", "-m", "init"]);
}

fn git<const N: usize>(path: &Path, args: [&str; N]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

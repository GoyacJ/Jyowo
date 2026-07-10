use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::{
    AcceptedCommand, CommandOutcome, CommandRejection, EventAuthority, NewTaskEvent,
    SynchronousTaskProjector, TaskProjector, TaskStore, TaskStoreError,
};
use harness_contracts::{ClientId, CommandId, TaskId};
use serde_json::json;

#[test]
fn accepted_commands_are_idempotent_across_reopen() {
    let root = temp_root("idempotent");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let command = command(task_id, 0, json!({ "title": "First" }));
    let decisions = AtomicUsize::new(0);

    let first = {
        let store = TaskStore::open(&path).unwrap();
        let first = store
            .transact_command(command.clone(), |_| {
                decisions.fetch_add(1, Ordering::SeqCst);
                Ok(vec![NewTaskEvent::task_created("First")])
            })
            .unwrap();
        let duplicate = store
            .transact_command(command.clone(), |_| {
                decisions.fetch_add(1, Ordering::SeqCst);
                Ok(vec![NewTaskEvent::task_title_changed("must not run")])
            })
            .unwrap();

        assert_eq!(duplicate, first);
        assert_eq!(store.latest_global_offset().unwrap(), 1);
        first
    };

    let reopened = TaskStore::open(&path).unwrap();
    let replayed = reopened
        .transact_command(command, |_| {
            decisions.fetch_add(1, Ordering::SeqCst);
            Ok(vec![NewTaskEvent::task_title_changed("must not run")])
        })
        .unwrap();
    assert_eq!(replayed, first);
    assert_eq!(decisions.load(Ordering::SeqCst), 1);
    assert_eq!(reopened.latest_global_offset().unwrap(), 1);

    drop(reopened);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn command_ids_and_idempotency_keys_have_independent_replay_rules() {
    let root = temp_root("conflict");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let store = TaskStore::open(&path).unwrap();
    let original = command(task_id, 0, json!({ "content": "first" }));
    store
        .transact_command(original.clone(), |_| {
            Ok(vec![NewTaskEvent::task_created("Submitted")])
        })
        .unwrap();

    let mut changed_body = original.clone();
    changed_body.payload = json!({ "content": "changed" });
    assert!(matches!(
        store.transact_command(changed_body, |_| Ok(Vec::new())),
        Err(TaskStoreError::CommandConflict { .. })
    ));

    let same_payload_with_changed_id = AcceptedCommand {
        command_id: CommandId::new(),
        ..original.clone()
    };
    let original_outcome = store
        .transact_command(original.clone(), |_| {
            panic!("stored outcome must bypass decision")
        })
        .unwrap();
    let replayed = store
        .transact_command(same_payload_with_changed_id, |_| {
            panic!("idempotency replay must bypass decision")
        })
        .unwrap();
    assert_eq!(replayed, original_outcome);

    let mut changed_id_and_body = original.clone();
    changed_id_and_body.command_id = CommandId::new();
    changed_id_and_body.payload = json!({ "content": "changed again" });
    assert!(matches!(
        store.transact_command(changed_id_and_body, |_| Ok(Vec::new())),
        Err(TaskStoreError::CommandConflict { .. })
    ));

    let mut changed_key = original;
    changed_key.idempotency_key = format!("idem-{}", CommandId::new());
    assert!(matches!(
        store.transact_command(changed_key, |_| Ok(Vec::new())),
        Err(TaskStoreError::CommandConflict { .. })
    ));
    assert_eq!(store.latest_global_offset().unwrap(), 1);

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn idempotency_keys_are_scoped_to_the_authenticated_client() {
    let root = temp_root("principal-scope");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let store = TaskStore::open(&path).unwrap();
    let shared_key = "same-client-generated-key".to_owned();
    let first = AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: shared_key.clone(),
        expected_stream_version: 0,
        authority: TaskStore::user_authority(ClientId::new()),
        payload: json!({ "title": "First" }),
    };
    store
        .transact_command(first, |_| Ok(vec![NewTaskEvent::task_created("First")]))
        .unwrap();
    let second = AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: shared_key,
        expected_stream_version: 1,
        authority: TaskStore::user_authority(ClientId::new()),
        payload: json!({ "title": "Second" }),
    };

    assert!(matches!(
        store
            .transact_command(second, |_| {
                Ok(vec![NewTaskEvent::task_title_changed("Second")])
            })
            .unwrap(),
        CommandOutcome::Accepted {
            stream_version: 2,
            ..
        }
    ));

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn wrong_expected_version_is_persisted_without_projection_changes() {
    let root = temp_root("wrong-version");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let store = TaskStore::open(&path).unwrap();
    store
        .transact_command(command(task_id, 0, json!({ "title": "Original" })), |_| {
            Ok(vec![NewTaskEvent::task_created("Original")])
        })
        .unwrap();
    let before = store.task_projection(task_id).unwrap().unwrap();
    let stale = command(task_id, 0, json!({ "title": "Stale" }));

    let outcome = store
        .transact_command(stale.clone(), |_| {
            Ok(vec![NewTaskEvent::task_title_changed("Stale")])
        })
        .unwrap();
    assert_eq!(
        outcome,
        CommandOutcome::Rejected {
            command_id: stale.command_id,
            task_id,
            rejection: CommandRejection::WrongExpectedVersion {
                expected: 0,
                actual: 1,
            },
        }
    );
    assert_eq!(store.task_projection(task_id).unwrap().unwrap(), before);
    assert_eq!(store.latest_global_offset().unwrap(), 1);

    let replayed = store
        .transact_command(stale, |_| panic!("stored rejection must bypass decision"))
        .unwrap();
    assert_eq!(replayed, outcome);

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn projector_failure_rolls_back_event_inbox_and_projection() {
    let root = temp_root("projector-rollback");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let command = command(task_id, 0, json!({ "title": "Rollback" }));
    let failing = TaskStore::open_with_projector(&path, Arc::new(FailingProjector)).unwrap();

    assert!(matches!(
        failing.transact_command(command.clone(), |_| {
            Ok(vec![NewTaskEvent::task_created("Rollback")])
        }),
        Err(TaskStoreError::Projector(_))
    ));
    assert_eq!(failing.latest_global_offset().unwrap(), 0);
    assert!(failing.task_projection(task_id).unwrap().is_none());
    drop(failing);

    let recovered = TaskStore::open(&path).unwrap();
    assert!(matches!(
        recovered
            .transact_command(command, |_| {
                Ok(vec![NewTaskEvent::task_created("Recovered")])
            })
            .unwrap(),
        CommandOutcome::Accepted { .. }
    ));
    assert_eq!(recovered.latest_global_offset().unwrap(), 1);

    drop(recovered);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn projector_failure_after_partial_writes_rolls_back_every_projection() {
    let root = temp_root("partial-projector-rollback");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let command = command(task_id, 0, json!({ "title": "Rollback" }));
    let failing = TaskStore::open_with_projector(
        &path,
        Arc::new(FailAfterFirstProjection {
            calls: AtomicUsize::new(0),
        }),
    )
    .unwrap();

    assert!(matches!(
        failing.transact_command(command.clone(), |_| {
            Ok(vec![
                NewTaskEvent::task_created("Rollback"),
                NewTaskEvent::task_title_changed("Must disappear"),
            ])
        }),
        Err(TaskStoreError::Projector(_))
    ));
    assert_eq!(failing.latest_global_offset().unwrap(), 0);
    assert_eq!(failing.projection_counts().unwrap().tasks, 0);
    assert_eq!(failing.projection_counts().unwrap().timeline_items, 0);
    drop(failing);

    let recovered = TaskStore::open(&path).unwrap();
    recovered
        .transact_command(command, |_| {
            Ok(vec![NewTaskEvent::task_created("Recovered")])
        })
        .unwrap();
    assert_eq!(
        recovered.task_projection(task_id).unwrap().unwrap().title,
        "Recovered"
    );

    drop(recovered);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn missing_or_stale_projection_blocks_command_decision() {
    for corruption in ["missing", "stale", "state", "event_offset"] {
        let root = temp_root(corruption);
        let path = root.join("tasks.db");
        let task_id = TaskId::new();
        let store = TaskStore::open(&path).unwrap();
        store
            .transact_command(command(task_id, 0, json!({ "title": "Original" })), |_| {
                Ok(vec![NewTaskEvent::task_created("Original")])
            })
            .unwrap();
        drop(store);

        let connection = rusqlite::Connection::open(&path).unwrap();
        if corruption == "missing" {
            connection
                .execute(
                    "DELETE FROM task_projection WHERE task_id = ?1",
                    [task_id.to_string()],
                )
                .unwrap();
        } else if corruption == "event_offset" {
            connection
                .execute(
                    "UPDATE event_log SET global_offset = 99 WHERE task_id = ?1",
                    [task_id.to_string()],
                )
                .unwrap();
        } else {
            let mut projection: serde_json::Value = serde_json::from_str(
                &connection
                    .query_row(
                        "SELECT projection_json FROM task_projection WHERE task_id = ?1",
                        [task_id.to_string()],
                        |row| row.get::<_, String>(0),
                    )
                    .unwrap(),
            )
            .unwrap();
            if corruption == "stale" {
                projection["streamVersion"] = json!(0);
            } else {
                projection["state"] = json!("completed");
            }
            connection
                .execute(
                    "UPDATE task_projection SET projection_json = ?2 WHERE task_id = ?1",
                    rusqlite::params![task_id.to_string(), projection.to_string()],
                )
                .unwrap();
        }
        drop(connection);

        let store = TaskStore::open(&path).unwrap();
        let decisions = AtomicUsize::new(0);
        let result =
            store.transact_command(command(task_id, 1, json!({ "title": "Changed" })), |_| {
                decisions.fetch_add(1, Ordering::SeqCst);
                Ok(vec![NewTaskEvent::task_title_changed("Changed")])
            });
        assert!(matches!(
            result,
            Err(TaskStoreError::ProjectionIntegrity(_))
        ));
        assert_eq!(decisions.load(Ordering::SeqCst), 0);
        assert_eq!(
            store.latest_global_offset().unwrap(),
            if corruption == "event_offset" { 99 } else { 1 }
        );

        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn replay_rejects_an_outcome_with_fabricated_commit_facts() {
    let root = temp_root("tampered-outcome");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let command = command(task_id, 0, json!({ "title": "Original" }));
    let store = TaskStore::open(&path).unwrap();
    store
        .transact_command(command.clone(), |_| {
            Ok(vec![NewTaskEvent::task_created("Original")])
        })
        .unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    let fabricated = CommandOutcome::Accepted {
        command_id: command.command_id,
        task_id,
        stream_version: 999,
        committed_offset: 999,
    };
    connection
        .execute(
            "UPDATE command_inbox SET outcome_json = ?2 WHERE command_id = ?1",
            rusqlite::params![
                command.command_id.to_string(),
                serde_json::to_string(&fabricated).unwrap()
            ],
        )
        .unwrap();
    drop(connection);

    let store = TaskStore::open(&path).unwrap();
    assert!(matches!(
        store.transact_command(command, |_| panic!("must not decide")),
        Err(TaskStoreError::ProjectionIntegrity(_))
    ));

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn zero_event_decisions_are_durable_rejections() {
    let root = temp_root("zero-event-rejection");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let store = TaskStore::open(&path).unwrap();
    store
        .transact_command(command(task_id, 0, json!({ "title": "Created" })), |_| {
            Ok(vec![NewTaskEvent::task_created("Created")])
        })
        .unwrap();
    let command = command(task_id, 1, json!({ "noop": true }));

    let outcome = store
        .transact_command(command.clone(), |_| Ok(Vec::new()))
        .unwrap();
    assert!(matches!(
        outcome,
        CommandOutcome::Rejected {
            rejection: CommandRejection::InvalidCommand { .. },
            ..
        }
    ));
    assert_eq!(store.stream_version(task_id).unwrap(), 1);
    assert_eq!(
        store
            .transact_command(command, |_| panic!("stored rejection must bypass decision"))
            .unwrap(),
        outcome
    );

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn replay_rejects_a_fabricated_zero_event_acceptance() {
    let root = temp_root("fabricated-zero-event-acceptance");
    let path = root.join("tasks.db");
    let task_id = TaskId::new();
    let command = command(task_id, 0, json!({ "title": "Original" }));
    let store = TaskStore::open(&path).unwrap();
    store
        .transact_command(command.clone(), |_| {
            Ok(vec![NewTaskEvent::task_created("Original")])
        })
        .unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch("PRAGMA ignore_check_constraints = ON")
        .unwrap();
    let fabricated = CommandOutcome::Accepted {
        command_id: command.command_id,
        task_id,
        stream_version: 0,
        committed_offset: 999,
    };
    connection
        .execute(
            "UPDATE command_inbox
             SET outcome_json = ?2, result_stream_version = 0,
                 committed_offset = 999, event_count = 0
             WHERE command_id = ?1",
            rusqlite::params![
                command.command_id.to_string(),
                serde_json::to_string(&fabricated).unwrap()
            ],
        )
        .unwrap();
    drop(connection);

    let store = TaskStore::open(&path).unwrap();
    assert!(matches!(
        store.transact_command(command, |_| panic!("must not decide")),
        Err(TaskStoreError::ProjectionIntegrity(_))
    ));

    drop(store);
    let _ = std::fs::remove_dir_all(root);
}

struct FailingProjector;

impl TaskProjector for FailingProjector {
    fn apply(
        &self,
        _transaction: &rusqlite::Transaction<'_>,
        _event: &harness_contracts::TaskEventEnvelope,
    ) -> Result<(), TaskStoreError> {
        Err(TaskStoreError::Projector("injected failure".into()))
    }
}

struct FailAfterFirstProjection {
    calls: AtomicUsize,
}

impl TaskProjector for FailAfterFirstProjection {
    fn apply(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        event: &harness_contracts::TaskEventEnvelope,
    ) -> Result<(), TaskStoreError> {
        SynchronousTaskProjector.apply(transaction, event)?;
        if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
            return Err(TaskStoreError::Projector(
                "failed after partial writes".into(),
            ));
        }
        Ok(())
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
        idempotency_key: format!("idem-{}", CommandId::new()),
        expected_stream_version,
        authority: authority(),
        payload,
    }
}

fn authority() -> EventAuthority {
    TaskStore::user_authority(ClientId::new())
}

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "jyowo-task-command-{name}-{}-{}",
        std::process::id(),
        TaskId::new()
    ));
    std::fs::create_dir_all(&root).unwrap();
    root
}

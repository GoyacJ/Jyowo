use chrono::Utc;
use harness_contracts::{
    ClientId, CommandId, QueueItemId, QueueItemProjection, QueueItemState, RunSegmentId, TaskId,
};
use harness_daemon::{decide_consume_next, decide_queue, QueueCommand};
use harness_journal::{AcceptedCommand, CommandOutcome, CommandRejection, NewTaskEvent, TaskStore};
use proptest::prelude::*;
use serde_json::json;

#[test]
fn queue_transition_table_accepts_only_defined_edges() {
    assert!(decide_queue(None, submit()).is_ok());

    let queued = item(QueueItemState::Queued, 1);
    assert!(decide_queue(
        Some(&queued),
        QueueCommand::Edit {
            expected_revision: 1,
            content: "edited".into(),
            attachments: Vec::new(),
            context_references: vec!["context:edited".into()],
        },
    )
    .is_ok());
    assert!(decide_queue(
        Some(&queued),
        QueueCommand::Delete {
            expected_revision: 1,
        },
    )
    .is_ok());
    assert!(decide_queue(
        Some(&queued),
        QueueCommand::Promote {
            expected_revision: 1,
            mode: harness_contracts::PromotionMode::SafePoint,
        },
    )
    .is_ok());
    assert!(decide_queue(
        Some(&queued),
        QueueCommand::Consume {
            expected_revision: 1,
            run_segment_id: RunSegmentId::new(),
        },
    )
    .is_err());

    let promoting = item(QueueItemState::Promoting, 2);
    assert!(decide_queue(
        Some(&promoting),
        QueueCommand::Consume {
            expected_revision: 2,
            run_segment_id: RunSegmentId::new(),
        },
    )
    .is_ok());
    assert!(decide_queue(Some(&promoting), QueueCommand::Recover).is_ok());

    for terminal in [QueueItemState::Consumed, QueueItemState::Deleted] {
        let terminal = item(terminal, 3);
        assert!(decide_queue(
            Some(&terminal),
            QueueCommand::Edit {
                expected_revision: 3,
                content: "late".into(),
                attachments: Vec::new(),
                context_references: Vec::new(),
            },
        )
        .is_err());
        assert!(decide_queue(Some(&terminal), QueueCommand::Recover).is_err());
    }
}

#[test]
fn stale_edit_returns_the_latest_queue_item() {
    let queued = item(QueueItemState::Queued, 4);
    let result = decide_queue(
        Some(&queued),
        QueueCommand::Edit {
            expected_revision: 3,
            content: "stale".into(),
            attachments: Vec::new(),
            context_references: Vec::new(),
        },
    );

    assert!(matches!(
        result,
        Err(CommandRejection::StaleQueueRevision { latest }) if *latest == queued
    ));
}

#[test]
fn idle_consumption_selects_fifo_and_starts_the_segment_atomically() {
    let root = tempfile::tempdir().unwrap();
    let store = TaskStore::open(root.path().join("tasks.sqlite")).unwrap();
    let task_id = TaskId::new();
    accept(
        &store,
        command(task_id, 0, json!({ "type": "create" })),
        vec![NewTaskEvent::task_created("fifo")],
    );

    let first = QueueItemId::new();
    accept(
        &store,
        command(task_id, 1, json!({ "type": "queue", "item": first })),
        decide_queue(
            None,
            QueueCommand::Submit {
                queue_item_id: first,
                content: "first".into(),
                attachments: Vec::new(),
                context_references: Vec::new(),
                created_at: Utc::now(),
            },
        )
        .unwrap(),
    );
    let second = QueueItemId::new();
    accept(
        &store,
        command(task_id, 2, json!({ "type": "queue", "item": second })),
        decide_queue(
            None,
            QueueCommand::Submit {
                queue_item_id: second,
                content: "second".into(),
                attachments: Vec::new(),
                context_references: Vec::new(),
                created_at: Utc::now(),
            },
        )
        .unwrap(),
    );

    let segment_id = RunSegmentId::new();
    let outcome = store
        .transact_command(
            AcceptedCommand {
                authority: TaskStore::supervisor_authority(),
                ..command(
                    task_id,
                    3,
                    json!({ "type": "consume_next", "segmentId": segment_id }),
                )
            },
            |projection| decide_consume_next(projection, segment_id, Utc::now()),
        )
        .unwrap();
    assert!(matches!(outcome, CommandOutcome::Accepted { .. }));

    let projection = store.task_projection(task_id).unwrap().unwrap();
    assert_eq!(projection.current_run.unwrap().segment_id, segment_id);
    assert_eq!(projection.queue.len(), 1);
    assert_eq!(projection.queue[0].queue_item_id, second);
    let events = store.events_after(0, 100).unwrap();
    let tail = &events[events.len() - 2..];
    assert_eq!(tail[0].event_type, "run.started");
    assert_eq!(tail[1].event_type, "message.consumed");
    assert_eq!(tail[1].payload["queueItemId"], first.to_string());
    assert_eq!(tail[0].stream_sequence + 1, tail[1].stream_sequence);
}

#[test]
fn edit_promote_recover_and_delete_are_durable() {
    let root = tempfile::tempdir().unwrap();
    let store = TaskStore::open(root.path().join("tasks.sqlite")).unwrap();
    let task_id = TaskId::new();
    let queue_item_id = QueueItemId::new();
    accept(
        &store,
        command(task_id, 0, json!({ "type": "create" })),
        vec![NewTaskEvent::task_created("durable queue")],
    );
    accept(
        &store,
        command(task_id, 1, json!({ "type": "queue" })),
        decide_queue(
            None,
            QueueCommand::Submit {
                queue_item_id,
                content: "draft".into(),
                attachments: Vec::new(),
                context_references: vec!["context:one".into()],
                created_at: Utc::now(),
            },
        )
        .unwrap(),
    );

    let queued = store.task_projection(task_id).unwrap().unwrap().queue[0].clone();
    accept(
        &store,
        command(task_id, 2, json!({ "type": "edit" })),
        decide_queue(
            Some(&queued),
            QueueCommand::Edit {
                expected_revision: 1,
                content: "final".into(),
                attachments: Vec::new(),
                context_references: vec!["context:two".into()],
            },
        )
        .unwrap(),
    );
    let edited = store.task_projection(task_id).unwrap().unwrap().queue[0].clone();
    assert_eq!(edited.revision, 2);
    assert_eq!(edited.content, "final");

    accept(
        &store,
        command(task_id, 3, json!({ "type": "promote" })),
        decide_queue(
            Some(&edited),
            QueueCommand::Promote {
                expected_revision: 2,
                mode: harness_contracts::PromotionMode::SafePoint,
            },
        )
        .unwrap(),
    );
    let promoting = store.task_projection(task_id).unwrap().unwrap().queue[0].clone();
    assert_eq!(promoting.state, QueueItemState::Promoting);
    assert_eq!(promoting.revision, 2);
    assert_eq!(promoting.content, "final");

    let mut recover = command(task_id, 4, json!({ "type": "recover" }));
    recover.authority = TaskStore::recovery_authority();
    accept(
        &store,
        recover,
        decide_queue(Some(&promoting), QueueCommand::Recover).unwrap(),
    );
    let recovered = store.task_projection(task_id).unwrap().unwrap().queue[0].clone();
    assert_eq!(recovered.state, QueueItemState::Queued);
    assert_eq!(recovered.revision, 2);

    accept(
        &store,
        command(task_id, 5, json!({ "type": "delete" })),
        decide_queue(
            Some(&recovered),
            QueueCommand::Delete {
                expected_revision: 2,
            },
        )
        .unwrap(),
    );
    assert!(store
        .task_projection(task_id)
        .unwrap()
        .unwrap()
        .queue
        .is_empty());
    assert_eq!(
        store
            .events_after(0, 100)
            .unwrap()
            .last()
            .unwrap()
            .event_type,
        "message.deleted"
    );
}

#[derive(Debug, Clone)]
enum GeneratedQueueCommand {
    Submit,
    Edit(bool),
    Delete(bool),
    Promote(bool),
    Consume(bool),
    Recover,
}

fn generated_commands() -> impl Strategy<Value = Vec<GeneratedQueueCommand>> {
    prop::collection::vec(
        prop_oneof![
            Just(GeneratedQueueCommand::Submit),
            any::<bool>().prop_map(GeneratedQueueCommand::Edit),
            any::<bool>().prop_map(GeneratedQueueCommand::Delete),
            any::<bool>().prop_map(GeneratedQueueCommand::Promote),
            any::<bool>().prop_map(GeneratedQueueCommand::Consume),
            Just(GeneratedQueueCommand::Recover),
        ],
        1..80,
    )
}

proptest! {
    #[test]
    fn generated_sequences_keep_revisions_monotonic_and_terminal_states_final(
        commands in generated_commands()
    ) {
        let queue_item_id = QueueItemId::new();
        let mut current = None::<QueueItemProjection>;
        let mut last_revision = 0;
        let mut terminal_seen = false;

        for generated in commands {
            let current_revision = current.as_ref().map_or(0, |item| item.revision);
            let (command, expected_success) = match generated {
                GeneratedQueueCommand::Submit => (
                    QueueCommand::Submit {
                        queue_item_id,
                        content: "generated".into(),
                        attachments: Vec::new(),
                        context_references: Vec::new(),
                        created_at: Utc::now(),
                    },
                    current.is_none(),
                ),
                GeneratedQueueCommand::Edit(fresh) => (
                    QueueCommand::Edit {
                        expected_revision: if fresh { current_revision } else { current_revision.saturating_add(1) },
                        content: "edited".into(),
                        attachments: Vec::new(),
                        context_references: Vec::new(),
                    },
                    current.as_ref().is_some_and(|item| item.state == QueueItemState::Queued) && fresh,
                ),
                GeneratedQueueCommand::Delete(fresh) => (
                    QueueCommand::Delete {
                        expected_revision: if fresh { current_revision } else { current_revision.saturating_add(1) },
                    },
                    current.as_ref().is_some_and(|item| item.state == QueueItemState::Queued) && fresh,
                ),
                GeneratedQueueCommand::Promote(fresh) => (
                    QueueCommand::Promote {
                        expected_revision: if fresh { current_revision } else { current_revision.saturating_add(1) },
                        mode: harness_contracts::PromotionMode::SafePoint,
                    },
                    current.as_ref().is_some_and(|item| item.state == QueueItemState::Queued) && fresh,
                ),
                GeneratedQueueCommand::Consume(fresh) => (
                    QueueCommand::Consume {
                        expected_revision: if fresh { current_revision } else { current_revision.saturating_add(1) },
                        run_segment_id: RunSegmentId::new(),
                    },
                    current.as_ref().is_some_and(|item| {
                        item.state == QueueItemState::Promoting
                    }) && fresh,
                ),
                GeneratedQueueCommand::Recover => (
                    QueueCommand::Recover,
                    current.as_ref().is_some_and(|item| item.state == QueueItemState::Promoting),
                ),
            };

            let result = decide_queue(current.as_ref(), command);
            prop_assert_eq!(result.is_ok(), expected_success);
            if !expected_success {
                continue;
            }

            match generated {
                GeneratedQueueCommand::Submit => current = Some(item_with_id(queue_item_id, QueueItemState::Queued, 1)),
                GeneratedQueueCommand::Edit(_) => current.as_mut().unwrap().revision += 1,
                GeneratedQueueCommand::Delete(_) => current.as_mut().unwrap().state = QueueItemState::Deleted,
                GeneratedQueueCommand::Promote(_) => current.as_mut().unwrap().state = QueueItemState::Promoting,
                GeneratedQueueCommand::Consume(_) => current.as_mut().unwrap().state = QueueItemState::Consumed,
                GeneratedQueueCommand::Recover => current.as_mut().unwrap().state = QueueItemState::Queued,
            }
            let item = current.as_ref().unwrap();
            prop_assert!(item.revision >= last_revision);
            last_revision = item.revision;
            if matches!(item.state, QueueItemState::Consumed | QueueItemState::Deleted) {
                prop_assert!(!terminal_seen);
                terminal_seen = true;
            }
        }
    }
}

fn submit() -> QueueCommand {
    QueueCommand::Submit {
        queue_item_id: QueueItemId::new(),
        content: "queued".into(),
        attachments: Vec::new(),
        context_references: vec!["context:initial".into()],
        created_at: Utc::now(),
    }
}

fn item(state: QueueItemState, revision: u64) -> QueueItemProjection {
    item_with_id(QueueItemId::new(), state, revision)
}

fn item_with_id(
    queue_item_id: QueueItemId,
    state: QueueItemState,
    revision: u64,
) -> QueueItemProjection {
    QueueItemProjection {
        queue_item_id,
        state,
        revision,
        content: "queued".into(),
        attachments: Vec::new(),
        context_references: vec!["context:initial".into()],
        created_at: Utc::now(),
        created_global_offset: 7,
        consumed_by: None,
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
        idempotency_key: format!("queue-test-{}", CommandId::new()),
        expected_stream_version,
        authority: TaskStore::user_authority(ClientId::new()),
        payload,
    }
}

fn accept(
    store: &TaskStore,
    command: AcceptedCommand,
    events: Vec<NewTaskEvent>,
) -> CommandOutcome {
    let outcome = store.transact_command(command, |_| Ok(events)).unwrap();
    assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
    outcome
}

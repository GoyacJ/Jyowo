use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use harness_contracts::{ClientId, CommandId, TaskEventEnvelope, TaskId};
use harness_journal::{AcceptedCommand, CommandOutcome, NewTaskEvent, TaskStore};
use serde_json::json;

const TASK_COUNT: usize = 100;
const EVENTS_PER_TASK: usize = 1_000;
const TOTAL_EVENTS: usize = TASK_COUNT * EVENTS_PER_TASK;
const EVENTS_PER_APPEND: usize = 250;
const READ_SAMPLE_SIZE: usize = 1_000;

const APPEND_BUDGET: Duration = Duration::from_secs(30);
const REBUILD_BUDGET: Duration = Duration::from_secs(30);
const GLOBAL_READ_BUDGET: Duration = Duration::from_millis(250);
const SNAPSHOT_READ_BUDGET: Duration = Duration::from_millis(500);

#[test]
#[ignore = "release-only 100,000-event performance gate"]
fn task_store_handles_100_000_events_within_release_budgets() {
    assert!(
        !cfg!(debug_assertions),
        "the performance gate must run with --release"
    );

    let root = tempfile::tempdir().expect("create performance test directory");
    let database_path = root.path().join("tasks.sqlite");
    let store = TaskStore::open(&database_path).expect("open task store");
    let authority = TaskStore::user_authority(ClientId::new());
    let mut task_ids = Vec::with_capacity(TASK_COUNT);

    let append_started = Instant::now();
    for task_index in 0..TASK_COUNT {
        let task_id = TaskId::new();
        task_ids.push(task_id);

        for batch_start in (0..EVENTS_PER_TASK).step_by(EVENTS_PER_APPEND) {
            let batch_end = (batch_start + EVENTS_PER_APPEND).min(EVENTS_PER_TASK);
            let events = (batch_start..batch_end)
                .map(|event_index| mixed_timeline_event(task_index, event_index))
                .collect::<Vec<_>>();
            let outcome = store
                .transact_command(
                    AcceptedCommand {
                        command_id: CommandId::new(),
                        task_id,
                        idempotency_key: format!("performance-{task_index}-{batch_start}"),
                        expected_stream_version: batch_start as u64,
                        authority: authority.clone(),
                        payload: json!({
                            "type": "performance_seed",
                            "taskIndex": task_index,
                            "batchStart": batch_start,
                        }),
                    },
                    |_| Ok(events),
                )
                .expect("append and synchronously project performance events");

            assert!(matches!(
                outcome,
                CommandOutcome::Accepted {
                    stream_version,
                    ..
                } if stream_version == batch_end as u64
            ));
        }
    }
    let append_elapsed = append_started.elapsed();

    assert_eq!(task_ids.len(), TASK_COUNT);
    assert_eq!(
        store.latest_global_offset().expect("read final offset"),
        TOTAL_EVENTS as u64
    );
    assert_eq!(
        store
            .task_projections()
            .expect("load seeded task projections")
            .len(),
        TASK_COUNT
    );
    assert!(
        append_elapsed < APPEND_BUDGET,
        "append plus synchronous projection took {append_elapsed:?}, budget {APPEND_BUDGET:?}"
    );

    let rebuild_started = Instant::now();
    store
        .rebuild_projections()
        .expect("rebuild projections from the event log");
    let rebuild_elapsed = rebuild_started.elapsed();
    assert!(
        rebuild_elapsed < REBUILD_BUDGET,
        "projection rebuild took {rebuild_elapsed:?}, budget {REBUILD_BUDGET:?}"
    );
    let rebuilt = store
        .task_projections()
        .expect("load rebuilt task projections");
    assert_eq!(rebuilt.len(), TASK_COUNT);
    assert!(rebuilt.iter().all(|task| {
        task.stream_version == EVENTS_PER_TASK as u64
            && task.last_global_offset > 0
            && task.last_global_offset <= TOTAL_EVENTS as u64
    }));

    let global_read_started = Instant::now();
    let global_sample = read_events_after(&store, 0, READ_SAMPLE_SIZE);
    let global_read_elapsed = global_read_started.elapsed();
    assert_eq!(global_sample.len(), READ_SAMPLE_SIZE);
    assert!(global_sample
        .iter()
        .enumerate()
        .all(|(index, event)| event.global_offset == index as u64 + 1));
    assert!(
        global_read_elapsed < GLOBAL_READ_BUDGET,
        "events_after read of {READ_SAMPLE_SIZE} events took {global_read_elapsed:?}, budget {GLOBAL_READ_BUDGET:?}"
    );

    let sampled_task_id = task_ids[TASK_COUNT / 2];
    let snapshot_read_started = Instant::now();
    let (snapshot, snapshot_offset, timeline) = store
        .task_projection_snapshot(sampled_task_id)
        .expect("load task snapshot")
        .expect("sampled task exists");
    let first_timeline_page = store
        .task_events_after_global_offset(sampled_task_id, 0, READ_SAMPLE_SIZE)
        .expect("load first task timeline page");
    let snapshot_read_elapsed = snapshot_read_started.elapsed();

    assert_eq!(snapshot.task_id, sampled_task_id);
    assert_eq!(snapshot.stream_version, EVENTS_PER_TASK as u64);
    assert_eq!(snapshot_offset, TOTAL_EVENTS as u64);
    assert_eq!(timeline.len(), EVENTS_PER_TASK);
    assert_eq!(
        timeline.first().map(|item| item.global_offset),
        first_timeline_page.first().map(|event| event.global_offset)
    );
    assert!(!first_timeline_page.is_empty());
    assert!(first_timeline_page.len() <= READ_SAMPLE_SIZE);
    assert!(first_timeline_page
        .iter()
        .all(|event| event.task_id == sampled_task_id));
    assert!(
        snapshot_read_elapsed < SNAPSHOT_READ_BUDGET,
        "task snapshot plus first timeline page took {snapshot_read_elapsed:?}, budget {SNAPSHOT_READ_BUDGET:?}"
    );

    let database_bytes = file_size(&database_path);
    let wal_bytes = file_size(&sidecar_path(&database_path, "-wal"));
    let shm_bytes = file_size(&sidecar_path(&database_path, "-shm"));
    let total_bytes = database_bytes + wal_bytes + shm_bytes;
    println!(
        "task-store-100k append_ms={} rebuild_ms={} events_after_1000_ms={} snapshot_page_ms={} db_bytes={database_bytes} wal_bytes={wal_bytes} shm_bytes={shm_bytes} total_bytes={total_bytes}",
        append_elapsed.as_millis(),
        rebuild_elapsed.as_millis(),
        global_read_elapsed.as_millis(),
        snapshot_read_elapsed.as_millis(),
    );
}

fn mixed_timeline_event(task_index: usize, event_index: usize) -> NewTaskEvent {
    match event_index {
        0 => NewTaskEvent::task_created(format!("Performance task {task_index}")),
        index if index % 3 == 1 => {
            NewTaskEvent::task_title_changed(format!("Task {task_index} revision {index}"))
        }
        index => NewTaskEvent::task_archived(index % 2 == 0),
    }
}

fn read_events_after(store: &TaskStore, after_offset: u64, count: usize) -> Vec<TaskEventEnvelope> {
    let mut events = Vec::with_capacity(count);
    let mut cursor = after_offset;
    while events.len() < count {
        let page = store
            .events_after(cursor, count - events.len())
            .expect("read global event page");
        assert!(!page.is_empty(), "event log ended before {count} events");
        cursor = page.last().expect("non-empty event page").global_offset;
        events.extend(page);
    }
    events
}

fn sidecar_path(database_path: &Path, suffix: &str) -> PathBuf {
    let mut path = OsString::from(database_path.as_os_str());
    path.push(suffix);
    PathBuf::from(path)
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |metadata| metadata.len())
}

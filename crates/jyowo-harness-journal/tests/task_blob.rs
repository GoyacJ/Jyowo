use std::sync::Arc;

use harness_contracts::{now, BlobId, ClientId, CommandId, QueueItemId, TaskId};
use harness_journal::{
    AcceptedCommand, BlobRead, NewTaskEvent, TaskBlobStore, TaskStore, TaskStoreError,
};
use serde_json::json;

#[test]
fn task_blobs_are_content_addressed_deduplicated_and_owned() {
    let database_path = temp_path("owned", "db");
    let blob_root = temp_path("owned", "blobs");
    let store = Arc::new(TaskStore::open(&database_path).unwrap());
    let task_a = TaskId::new();
    let task_b = TaskId::new();
    create_task(&store, task_a);
    create_task(&store, task_b);
    let blobs_a = TaskBlobStore::open(Arc::clone(&store), task_a, &blob_root).unwrap();
    let blobs_b = TaskBlobStore::open(Arc::clone(&store), task_b, &blob_root).unwrap();
    let other_root = temp_path("other-root", "blobs");
    assert!(matches!(
        TaskBlobStore::open(Arc::clone(&store), task_a, &other_root),
        Err(TaskStoreError::BlobIntegrity(_))
    ));
    let first = blobs_a.put("text/plain", b"same bytes").unwrap();
    let duplicate = blobs_a.put("text/plain", b"same bytes").unwrap();
    attach_blob(&store, task_a, 1, first.id);
    let digest = blake3::hash(b"same bytes");

    assert_eq!(first, duplicate);
    assert_eq!(first.id.as_bytes(), digest.as_bytes()[..16]);
    assert!(matches!(
        blobs_a.read(&first.id).unwrap(),
        BlobRead::Available { bytes, .. } if bytes == b"same bytes"
    ));
    assert!(matches!(
        blobs_b.read(&first.id),
        Err(TaskStoreError::BlobOwnershipDenied { blob_id, task_id })
            if blob_id == first.id && task_id == task_b
    ));

    let shared = blobs_b.put("text/plain", b"same bytes").unwrap();
    assert_eq!(shared, first);
    attach_blob(&store, task_b, 1, shared.id);
    assert!(matches!(
        blobs_b.read(&first.id).unwrap(),
        BlobRead::Available { bytes, .. } if bytes == b"same bytes"
    ));

    drop((blobs_a, blobs_b, store));
    let reopened = Arc::new(TaskStore::open(&database_path).unwrap());
    let reopened_blobs = TaskBlobStore::open(Arc::clone(&reopened), task_a, &blob_root).unwrap();
    assert!(matches!(
        reopened_blobs.read(&first.id).unwrap(),
        BlobRead::Available { bytes, .. } if bytes == b"same bytes"
    ));
    drop((reopened_blobs, reopened));
    cleanup(&database_path, &blob_root);
    let _ = std::fs::remove_dir_all(other_root);
}

#[test]
fn task_blob_reads_report_missing_and_reject_corruption() {
    let database_path = temp_path("integrity", "db");
    let blob_root = temp_path("integrity", "blobs");
    let store = Arc::new(TaskStore::open(&database_path).unwrap());
    let task_id = TaskId::new();
    create_task(&store, task_id);
    let blobs = TaskBlobStore::open(Arc::clone(&store), task_id, &blob_root).unwrap();
    let reference = blobs.put("application/octet-stream", b"original").unwrap();
    attach_blob(&store, task_id, 1, reference.id);
    let body_path = blob_body_path(&blob_root, reference.id);

    std::fs::write(&body_path, b"tampered").unwrap();
    assert!(matches!(
        blobs.read(&reference.id),
        Err(TaskStoreError::BlobIntegrity(_))
    ));

    std::fs::remove_file(&body_path).unwrap();
    assert!(matches!(
        blobs.read(&reference.id).unwrap(),
        BlobRead::Missing { blob } if blob == reference
    ));
    rusqlite::Connection::open(&database_path)
        .unwrap()
        .execute(
            "UPDATE blob_metadata SET content_hash = ?1 WHERE blob_id = ?2",
            ["00".repeat(32), reference.id.to_string()],
        )
        .unwrap();
    assert!(matches!(
        blobs.put("application/octet-stream", b"original"),
        Err(TaskStoreError::BlobIntegrity(_))
    ));
    assert!(!body_path.exists());
    assert!(matches!(
        blobs.put("../invalid", b"body"),
        Err(TaskStoreError::InvalidInput(_))
    ));

    drop((blobs, store));
    cleanup(&database_path, &blob_root);
}

#[test]
fn queued_blob_references_require_metadata_and_task_ownership() {
    let database_path = temp_path("references", "db");
    let blob_root = temp_path("references", "blobs");
    let store = Arc::new(TaskStore::open(&database_path).unwrap());
    let owner = TaskId::new();
    let other = TaskId::new();
    create_task(&store, owner);
    create_task(&store, other);
    let blobs = TaskBlobStore::open(Arc::clone(&store), owner, &blob_root).unwrap();
    let reference = blobs.put("text/plain", b"owned").unwrap();
    attach_blob(&store, owner, 1, reference.id);

    let result = store.transact_command(command(other, 1), |_| {
        Ok(vec![NewTaskEvent::message_queued(
            QueueItemId::new(),
            "use attachment",
            vec![reference.id],
            Vec::new(),
            now(),
        )])
    });
    assert!(matches!(
        result,
        Err(TaskStoreError::BlobOwnershipDenied { blob_id, task_id })
            if blob_id == reference.id && task_id == other
    ));
    assert_eq!(store.stream_version(other).unwrap(), 1);

    drop((blobs, store));
    cleanup(&database_path, &blob_root);
}

#[test]
fn blob_metadata_and_ownership_commit_with_the_reference_event() {
    let database_path = temp_path("transactional", "db");
    let blob_root = temp_path("transactional", "blobs");
    let store = Arc::new(TaskStore::open(&database_path).unwrap());
    let task_id = TaskId::new();
    create_task(&store, task_id);
    let blobs = TaskBlobStore::open(Arc::clone(&store), task_id, &blob_root).unwrap();
    let reference = blobs.put("text/plain", b"staged").unwrap();

    assert_eq!(table_count(&database_path, "blob_metadata"), 0);
    assert_eq!(table_count(&database_path, "blob_ownership"), 0);
    assert_eq!(table_count(&database_path, "blob_staging"), 1);

    store
        .transact_command(command(task_id, 1), |_| {
            Ok(vec![NewTaskEvent::message_queued(
                QueueItemId::new(),
                "attach staged blob",
                vec![reference.id],
                Vec::new(),
                now(),
            )])
        })
        .unwrap();

    assert_eq!(table_count(&database_path, "blob_metadata"), 1);
    assert_eq!(table_count(&database_path, "blob_ownership"), 1);
    assert_eq!(table_count(&database_path, "blob_staging"), 0);
    assert!(matches!(
        blobs.read(&reference.id).unwrap(),
        BlobRead::Available { bytes, .. } if bytes == b"staged"
    ));

    drop((blobs, store));
    cleanup(&database_path, &blob_root);
}

#[test]
fn abandoned_staged_blobs_can_be_discarded_without_dangling_files() {
    let database_path = temp_path("discard", "db");
    let blob_root = temp_path("discard", "blobs");
    let store = Arc::new(TaskStore::open(&database_path).unwrap());
    let task_id = TaskId::new();
    create_task(&store, task_id);
    let blobs = TaskBlobStore::open(Arc::clone(&store), task_id, &blob_root).unwrap();
    let reference = blobs.put("text/plain", b"discard me").unwrap();
    let body_path = blob_body_path(&blob_root, reference.id);

    blobs.discard_staged(&reference.id).unwrap();

    assert_eq!(table_count(&database_path, "blob_staging"), 0);
    assert!(!body_path.exists());
    assert!(matches!(
        blobs.read(&reference.id),
        Err(TaskStoreError::BlobNotFound { .. })
    ));

    drop((blobs, store));
    cleanup(&database_path, &blob_root);
}

#[test]
fn blob_root_cannot_be_shared_by_different_task_databases() {
    let first_database = temp_path("root-owner-first", "db");
    let second_database = temp_path("root-owner-second", "db");
    let blob_root = temp_path("root-owner", "blobs");
    let first_store = Arc::new(TaskStore::open(&first_database).unwrap());
    let second_store = Arc::new(TaskStore::open(&second_database).unwrap());
    let first_task = TaskId::new();
    let second_task = TaskId::new();
    create_task(&first_store, first_task);
    create_task(&second_store, second_task);

    let first_blobs =
        TaskBlobStore::open(Arc::clone(&first_store), first_task, &blob_root).unwrap();
    let result = TaskBlobStore::open(Arc::clone(&second_store), second_task, &blob_root);

    assert!(matches!(result, Err(TaskStoreError::BlobIntegrity(_))));

    drop((first_blobs, first_store, second_store));
    let reopened = Arc::new(TaskStore::open(&first_database).unwrap());
    let reopened_blobs = TaskBlobStore::open(reopened, first_task, &blob_root).unwrap();
    drop(reopened_blobs);
    cleanup(&first_database, &blob_root);
    cleanup(&second_database, &blob_root);
}

#[test]
fn failed_blob_root_binding_releases_its_filesystem_claim() {
    let first_database = temp_path("failed-root-binding-first", "db");
    let second_database = temp_path("failed-root-binding-second", "db");
    let bound_root = temp_path("failed-root-binding-bound", "blobs");
    let rejected_root = temp_path("failed-root-binding-rejected", "blobs");
    let first_store = Arc::new(TaskStore::open(&first_database).unwrap());
    let first_task = TaskId::new();
    create_task(&first_store, first_task);
    let first_blobs =
        TaskBlobStore::open(Arc::clone(&first_store), first_task, &bound_root).unwrap();
    drop((first_blobs, first_store));
    let reopened_first_store = Arc::new(TaskStore::open(&first_database).unwrap());

    assert!(matches!(
        TaskBlobStore::open(
            Arc::clone(&reopened_first_store),
            first_task,
            &rejected_root
        ),
        Err(TaskStoreError::BlobIntegrity(_))
    ));
    assert!(rejected_root.join(".jyowo-task-store.lock").is_file());
    drop(reopened_first_store);

    let second_store = Arc::new(TaskStore::open(&second_database).unwrap());
    let second_task = TaskId::new();
    create_task(&second_store, second_task);
    let second_blobs =
        TaskBlobStore::open(Arc::clone(&second_store), second_task, &rejected_root).unwrap();

    drop((second_blobs, second_store));
    cleanup(&first_database, &bound_root);
    cleanup(&second_database, &rejected_root);
}

#[test]
fn empty_blob_root_claim_is_recovered_after_restart() {
    let database_path = temp_path("empty-root-claim", "db");
    let blob_root = temp_path("empty-root-claim", "blobs");
    std::fs::create_dir_all(blob_root.join(".jyowo-task-store")).unwrap();
    let store = Arc::new(TaskStore::open(&database_path).unwrap());
    let task_id = TaskId::new();
    create_task(&store, task_id);

    let blobs = TaskBlobStore::open(Arc::clone(&store), task_id, &blob_root).unwrap();

    drop((blobs, store));
    cleanup(&database_path, &blob_root);
}

#[cfg(unix)]
#[test]
fn blob_root_lock_rejects_a_symlink_file() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let database_path = temp_path("symlink-root-lock", "db");
    let blob_root = temp_path("symlink-root-lock", "blobs");
    let outside_lock = temp_path("symlink-root-lock-target", "lock");
    std::fs::create_dir_all(&blob_root).unwrap();
    std::fs::write(&outside_lock, b"outside").unwrap();
    std::fs::set_permissions(&outside_lock, std::fs::Permissions::from_mode(0o644)).unwrap();
    symlink(&outside_lock, blob_root.join(".jyowo-task-store.lock")).unwrap();
    let store = Arc::new(TaskStore::open(&database_path).unwrap());
    let task_id = TaskId::new();
    create_task(&store, task_id);

    assert!(TaskBlobStore::open(Arc::clone(&store), task_id, &blob_root).is_err());
    assert_eq!(std::fs::read(&outside_lock).unwrap(), b"outside");
    assert_eq!(
        std::fs::metadata(&outside_lock)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o644
    );

    drop(store);
    cleanup(&database_path, &blob_root);
    let _ = std::fs::remove_file(outside_lock);
}

#[test]
fn blob_root_allows_only_one_live_store_instance() {
    let database_path = temp_path("single-live-store", "db");
    let blob_root = temp_path("single-live-store", "blobs");
    let first_store = Arc::new(TaskStore::open(&database_path).unwrap());
    let task_id = TaskId::new();
    create_task(&first_store, task_id);
    let first_blobs = TaskBlobStore::open(Arc::clone(&first_store), task_id, &blob_root).unwrap();
    let second_store = Arc::new(TaskStore::open(&database_path).unwrap());

    assert!(matches!(
        TaskBlobStore::open(Arc::clone(&second_store), task_id, &blob_root),
        Err(TaskStoreError::BlobIntegrity(_))
    ));

    drop((first_blobs, first_store));
    let second_blobs = TaskBlobStore::open(second_store, task_id, &blob_root).unwrap();
    drop(second_blobs);
    cleanup(&database_path, &blob_root);
}

#[test]
fn reopening_the_blob_root_removes_files_without_database_references() {
    let database_path = temp_path("orphan-recovery", "db");
    let blob_root = temp_path("orphan-recovery", "blobs");
    let store = Arc::new(TaskStore::open(&database_path).unwrap());
    let task_id = TaskId::new();
    create_task(&store, task_id);
    let blobs = TaskBlobStore::open(Arc::clone(&store), task_id, &blob_root).unwrap();
    let digest = blake3::hash(b"crash orphan");
    let mut id_bytes = [0_u8; 16];
    id_bytes.copy_from_slice(&digest.as_bytes()[..16]);
    let orphan_id = BlobId::from_u128(u128::from_be_bytes(id_bytes));
    let orphan_path = blob_body_path(&blob_root, orphan_id);
    std::fs::create_dir_all(orphan_path.parent().unwrap()).unwrap();
    std::fs::write(&orphan_path, b"crash orphan").unwrap();
    drop((blobs, store));

    let reopened = Arc::new(TaskStore::open(&database_path).unwrap());
    let reopened_blobs = TaskBlobStore::open(reopened, task_id, &blob_root).unwrap();

    assert!(!orphan_path.exists());
    drop(reopened_blobs);
    cleanup(&database_path, &blob_root);
}

fn table_count(database_path: &std::path::Path, table: &str) -> u64 {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    let count: i64 = rusqlite::Connection::open(database_path)
        .unwrap()
        .query_row(&sql, [], |row| row.get(0))
        .unwrap();
    u64::try_from(count).unwrap()
}

fn blob_body_path(root: &std::path::Path, blob_id: BlobId) -> std::path::PathBuf {
    let id = blob_id.to_string();
    root.join(&id[..2]).join(format!("{id}.blob"))
}

fn temp_path(name: &str, suffix: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-task-blob-{name}-{}-{}.{suffix}",
        std::process::id(),
        TaskId::new()
    ))
}

fn cleanup(database_path: &std::path::Path, blob_root: &std::path::Path) {
    for suffix in ["", "-shm", "-wal"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", database_path.display()));
    }
    let _ = std::fs::remove_dir_all(blob_root);
}

fn create_task(store: &TaskStore, task_id: TaskId) {
    store
        .transact_command(command(task_id, 0), |_| {
            Ok(vec![NewTaskEvent::task_created("Blob")])
        })
        .unwrap();
}

fn attach_blob(store: &TaskStore, task_id: TaskId, expected: u64, blob_id: BlobId) {
    store
        .transact_command(command(task_id, expected), |_| {
            Ok(vec![NewTaskEvent::message_queued(
                QueueItemId::new(),
                "attach blob",
                vec![blob_id],
                Vec::new(),
                now(),
            )])
        })
        .unwrap();
}

fn command(task_id: TaskId, expected_stream_version: u64) -> AcceptedCommand {
    AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("blob-{}", CommandId::new()),
        expected_stream_version,
        authority: TaskStore::user_authority(ClientId::new()),
        payload: json!({ "expected": expected_stream_version }),
    }
}

//! Content-addressed blobs owned by daemon tasks.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_contracts::{BlobId, BlobRef, TaskId, MAX_DAEMON_BLOB_BYTES};

use crate::{TaskStore, TaskStoreError};

const MAX_MEDIA_TYPE_BYTES: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobRead {
    Available { blob: BlobRef, bytes: Vec<u8> },
    Missing { blob: BlobRef },
}

pub struct TaskBlobStore {
    store: Arc<TaskStore>,
    task_id: TaskId,
    root: PathBuf,
}

impl TaskBlobStore {
    pub fn open(
        store: Arc<TaskStore>,
        task_id: TaskId,
        root: impl AsRef<Path>,
    ) -> Result<Self, TaskStoreError> {
        let root = crate::app_controlled_path(root.as_ref())?;
        harness_fs::ensure_owner_only_app_dir(&root)?;
        store.bind_blob_root(&root)?;
        Ok(Self {
            store,
            task_id,
            root,
        })
    }

    pub fn put(&self, media_type: &str, bytes: &[u8]) -> Result<BlobRef, TaskStoreError> {
        self.put_with_after_file_check(media_type, bytes, || {})
    }

    fn put_with_after_file_check<F>(
        &self,
        media_type: &str,
        bytes: &[u8],
        after_file_check: F,
    ) -> Result<BlobRef, TaskStoreError>
    where
        F: FnOnce(),
    {
        validate_media_type(media_type)?;
        if bytes.len() > MAX_DAEMON_BLOB_BYTES {
            return Err(TaskStoreError::InvalidInput(format!(
                "task blob exceeds the {MAX_DAEMON_BLOB_BYTES} byte daemon IPC frame limit"
            )));
        }
        let byte_size =
            u64::try_from(bytes.len()).map_err(|_| TaskStoreError::IntegerOutOfRange)?;
        let digest = blake3::hash(bytes);
        let blob_id = blob_id_from_hash(digest);
        let _operation = self.store.lock_blob_operation(blob_id)?;
        let relative_path = relative_blob_path(blob_id);
        let body_path = self.root.join(&relative_path);
        let parent = body_path.parent().ok_or_else(|| {
            TaskStoreError::BlobIntegrity("task blob path has no parent directory".into())
        })?;
        harness_fs::ensure_app_dir_no_symlink(parent)?;
        let relative_path = relative_path.to_str().ok_or_else(|| {
            TaskStoreError::BlobIntegrity("task blob path is not valid UTF-8".into())
        })?;

        let wrote_file = if let Some(existing) =
            harness_fs::read_file_no_follow_bounded(&body_path, bytes.len())?
        {
            validate_body(blob_id, byte_size, *digest.as_bytes(), &existing)?;
            false
        } else {
            harness_fs::write_bytes_file_atomic(&body_path, bytes, true)?;
            true
        };
        after_file_check();
        let staged = self.store.stage_blob(
            self.task_id,
            blob_id,
            media_type,
            byte_size,
            *digest.as_bytes(),
            relative_path,
        );
        if let Err(error) = staged {
            if wrote_file {
                let _ = self.store.cleanup_blob_if_unreferenced(
                    blob_id,
                    byte_size,
                    *digest.as_bytes(),
                    relative_path,
                    || {
                        harness_fs::remove_file_no_follow(&body_path)?;
                        Ok(())
                    },
                );
            }
            return Err(error);
        }
        if let Some(existing) = harness_fs::read_file_no_follow_bounded(&body_path, bytes.len())? {
            validate_body(blob_id, byte_size, *digest.as_bytes(), &existing)?;
        } else {
            harness_fs::write_bytes_file_atomic(&body_path, bytes, true)?;
        }
        Ok(BlobRef {
            id: blob_id,
            size: byte_size,
            content_hash: *digest.as_bytes(),
            content_type: Some(media_type.to_owned()),
        })
    }

    pub fn read(&self, blob_id: &BlobId) -> Result<BlobRead, TaskStoreError> {
        let metadata = self.store.blob_metadata_for_task(self.task_id, *blob_id)?;
        validate_media_type(&metadata.media_type)?;
        if metadata.byte_size > MAX_DAEMON_BLOB_BYTES as u64 {
            return Err(TaskStoreError::BlobIntegrity(format!(
                "blob {blob_id} exceeds the maximum stored blob size"
            )));
        }
        let expected_id = blob_id_from_hash(blake3::Hash::from_bytes(metadata.content_hash));
        if expected_id != *blob_id {
            return Err(TaskStoreError::BlobIntegrity(format!(
                "blob {blob_id} does not match its content hash"
            )));
        }
        let expected_relative_path = relative_blob_path(*blob_id);
        if metadata.relative_path != expected_relative_path.to_string_lossy() {
            return Err(TaskStoreError::BlobIntegrity(format!(
                "blob {blob_id} has a non-canonical storage path"
            )));
        }
        let blob = BlobRef {
            id: *blob_id,
            size: metadata.byte_size,
            content_hash: metadata.content_hash,
            content_type: Some(metadata.media_type),
        };
        let body_path = self.root.join(expected_relative_path);
        let expected_size =
            usize::try_from(blob.size).map_err(|_| TaskStoreError::IntegerOutOfRange)?;
        let Some(bytes) = harness_fs::read_file_no_follow_bounded(&body_path, expected_size)?
        else {
            return Ok(BlobRead::Missing { blob });
        };
        validate_body(blob.id, blob.size, blob.content_hash, &bytes)?;
        Ok(BlobRead::Available { blob, bytes })
    }

    pub fn discard_staged(&self, blob_id: &BlobId) -> Result<(), TaskStoreError> {
        let _operation = self.store.lock_blob_operation(*blob_id)?;
        self.store
            .discard_staged_blob_with(self.task_id, *blob_id, || {
                harness_fs::remove_file_no_follow(&self.root.join(relative_blob_path(*blob_id)))?;
                Ok(())
            })
    }
}

fn blob_id_from_hash(hash: blake3::Hash) -> BlobId {
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&hash.as_bytes()[..16]);
    BlobId::from_u128(u128::from_be_bytes(bytes))
}

fn relative_blob_path(blob_id: BlobId) -> PathBuf {
    let id = blob_id.to_string();
    PathBuf::from(&id[..2]).join(format!("{id}.blob"))
}

fn validate_body(
    blob_id: BlobId,
    expected_size: u64,
    expected_hash: [u8; 32],
    bytes: &[u8],
) -> Result<(), TaskStoreError> {
    let actual_size = u64::try_from(bytes.len()).map_err(|_| TaskStoreError::IntegerOutOfRange)?;
    let actual_hash = *blake3::hash(bytes).as_bytes();
    if actual_size != expected_size || actual_hash != expected_hash {
        return Err(TaskStoreError::BlobIntegrity(format!(
            "blob {blob_id} content does not match stored size or hash"
        )));
    }
    Ok(())
}

fn validate_media_type(media_type: &str) -> Result<(), TaskStoreError> {
    if media_type.is_empty() || media_type.len() > MAX_MEDIA_TYPE_BYTES || !media_type.is_ascii() {
        return Err(TaskStoreError::InvalidInput(
            "task blob media type is empty, non-ASCII, or too long".into(),
        ));
    }
    let mut parts = media_type.split('/');
    let top = parts.next().unwrap_or_default();
    let sub = parts.next().unwrap_or_default();
    if parts.next().is_some()
        || top.is_empty()
        || sub.is_empty()
        || matches!(top, "." | "..")
        || matches!(sub, "." | "..")
        || !top.bytes().all(valid_media_type_byte)
        || !sub.bytes().all(valid_media_type_byte)
    {
        return Err(TaskStoreError::InvalidInput(
            "task blob media type is invalid".into(),
        ));
    }
    Ok(())
}

fn valid_media_type_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
        )
}

#[cfg(test)]
mod tests {
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    use harness_contracts::{ClientId, CommandId};
    use serde_json::json;

    use super::*;
    use crate::{AcceptedCommand, NewTaskEvent};

    #[test]
    fn concurrent_discard_cannot_leave_a_successful_put_without_its_file() {
        let temp = std::env::temp_dir().join(format!(
            "jyowo-task-blob-race-{}-{}",
            std::process::id(),
            TaskId::new()
        ));
        let database_path = temp.join("tasks.db");
        let blob_root = temp.join("blobs");
        let store = Arc::new(TaskStore::open(&database_path).unwrap());
        let task_a = TaskId::new();
        let task_b = TaskId::new();
        create_task(&store, task_a);
        create_task(&store, task_b);
        let blobs_a = TaskBlobStore::open(Arc::clone(&store), task_a, &blob_root).unwrap();
        let first = blobs_a.put("text/plain", b"shared").unwrap();
        let body_path = blob_root.join(relative_blob_path(first.id));
        let thread_store = Arc::clone(&store);
        let thread_root = blob_root.clone();
        let (checked_tx, checked_rx) = mpsc::sync_channel(0);
        let (resume_tx, resume_rx) = mpsc::sync_channel(0);
        let put = std::thread::spawn(move || {
            let blobs_b = TaskBlobStore::open(thread_store, task_b, thread_root).unwrap();
            blobs_b.put_with_after_file_check("text/plain", b"shared", || {
                checked_tx.send(()).unwrap();
                resume_rx.recv().unwrap();
            })
        });

        checked_rx.recv().unwrap();
        let discarded_id = first.id;
        let (discard_tx, discard_rx) = mpsc::sync_channel(1);
        let discard = std::thread::spawn(move || {
            discard_tx
                .send(blobs_a.discard_staged(&discarded_id))
                .unwrap();
        });
        assert!(matches!(
            discard_rx.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        resume_tx.send(()).unwrap();

        let second = put.join().unwrap().unwrap();
        discard_rx.recv().unwrap().unwrap();
        discard.join().unwrap();
        assert_eq!(second, first);
        assert_eq!(std::fs::read(body_path).unwrap(), b"shared");

        drop(store);
        let _ = std::fs::remove_dir_all(temp);
    }

    fn create_task(store: &TaskStore, task_id: TaskId) {
        store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: format!("create-{}", CommandId::new()),
                    expected_stream_version: 0,
                    authority: TaskStore::user_authority(ClientId::new()),
                    payload: json!({ "create": true }),
                },
                |_| Ok(vec![NewTaskEvent::task_created("Blob")]),
            )
            .unwrap();
    }
}

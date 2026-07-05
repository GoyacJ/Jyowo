use std::borrow::Cow;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::Utc;
use harness_contracts::{ContentHash, MemoryError};
use sha2::{Digest, Sha256};

use super::lock::LockedFile;
use super::{BuiltinMemory, MemdirFile, MemdirSnapshot, MemdirWriteOutcome, SnapshotStrategy};
use crate::MemoryMetric;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Edit {
    Append,
    Replace,
    Delete,
}

pub(crate) struct MemdirWriteResult {
    pub(crate) outcome: MemdirWriteOutcome,
    pub(crate) previous_content: String,
}

pub(crate) fn read_all(memory: &BuiltinMemory) -> Result<MemdirSnapshot, MemoryError> {
    ensure_layout(memory)?;

    let memory_content = read_existing(&path_for(memory, MemdirFile::Memory))?;
    let user_content = read_existing(&path_for(memory, MemdirFile::User))?;

    Ok(MemdirSnapshot {
        memory_chars: memory_content.chars().count(),
        user_chars: user_content.chars().count(),
        memory: memory_content,
        user: user_content,
        captured_at: Utc::now(),
    })
}

pub(crate) fn write_section_with_previous(
    memory: &BuiltinMemory,
    file: MemdirFile,
    section: &str,
    content: &str,
    edit: Edit,
) -> Result<MemdirWriteResult, MemoryError> {
    ensure_layout(memory)?;
    let path = path_for(memory, file);
    let lock_started = Instant::now();
    let _lock = match LockedFile::acquire(&lock_path_for(memory, file), memory.concurrency()) {
        Ok(lock) => {
            memory.record_metric(MemoryMetric::MemdirLockWait {
                file,
                waited_ms: elapsed_ms(lock_started),
            });
            lock
        }
        Err(error) => {
            if let MemoryError::ConcurrentWriteLockFailed { retries } = &error {
                memory.record_metric(MemoryMetric::MemdirLockFailed {
                    file,
                    retries: *retries,
                });
            }
            return Err(error);
        }
    };

    let previous = read_existing(&path)?;
    let content = content_for_edit(memory, content, edit)?;
    let next = apply_edit(memory.separator(), &previous, section, &content, edit);
    enforce_limit(file, &next, memory.limit_for(file))?;

    let snapshot_path = write_snapshot_if_needed(memory, file, &previous, edit)?;
    write_atomic(&path, &next)?;
    memory.record_memdir_bytes(file, next.len() as u64);

    Ok(MemdirWriteResult {
        outcome: MemdirWriteOutcome {
            bytes_written: next.len() as u64,
            previous_hash: hash(previous.as_bytes()),
            new_hash: hash(next.as_bytes()),
            snapshot_path,
            takes_effect: memory.write_takes_effect(),
        },
        previous_content: previous,
    })
}

pub(crate) fn restore_content(
    memory: &BuiltinMemory,
    file: MemdirFile,
    content: &str,
) -> Result<(), MemoryError> {
    ensure_layout(memory)?;
    let path = path_for(memory, file);
    let _lock = LockedFile::acquire(&lock_path_for(memory, file), memory.concurrency())?;
    write_atomic(&path, content)?;
    memory.record_memdir_bytes(file, content.len() as u64);
    Ok(())
}

fn content_for_edit<'a>(
    memory: &BuiltinMemory,
    content: &'a str,
    edit: Edit,
) -> Result<Cow<'a, str>, MemoryError> {
    let _ = memory;

    if matches!(edit, Edit::Delete) {
        return Ok(Cow::Borrowed(content));
    }

    Ok(Cow::Borrowed(content))
}

fn ensure_layout(memory: &BuiltinMemory) -> Result<(), MemoryError> {
    let tenant_dir = memory.tenant_dir();
    fs::create_dir_all(tenant_dir.join(".locks")).map_err(io_error)?;
    fs::create_dir_all(tenant_dir.join("snapshots")).map_err(io_error)
}

fn read_existing(path: &Path) -> Result<String, MemoryError> {
    if !path.exists() {
        return Ok(String::new());
    }

    fs::read_to_string(path).map_err(io_error)
}

fn apply_edit(separator: &str, current: &str, section: &str, content: &str, edit: Edit) -> String {
    let section_block = format_section(separator, section, content);

    match edit {
        Edit::Append => {
            let mut next = current.to_owned();
            if !next.is_empty() && !next.ends_with('\n') {
                next.push('\n');
            }
            next.push_str(&section_block);
            next
        }
        Edit::Replace => replace_section(separator, current, section, &section_block)
            .unwrap_or_else(|| apply_edit(separator, current, section, content, Edit::Append)),
        Edit::Delete => {
            delete_section(separator, current, section).unwrap_or_else(|| current.to_owned())
        }
    }
}

fn format_section(separator: &str, section: &str, content: &str) -> String {
    let mut out = format!("{separator} {section}\n");
    out.push_str(content.trim_end_matches('\n'));
    out.push('\n');
    out
}

fn replace_section(
    separator: &str,
    current: &str,
    section: &str,
    replacement: &str,
) -> Option<String> {
    section_bounds(separator, current, section).map(|(start, end)| {
        let mut out = String::with_capacity(current.len() - (end - start) + replacement.len());
        out.push_str(&current[..start]);
        out.push_str(replacement);
        out.push_str(&current[end..]);
        out
    })
}

fn delete_section(separator: &str, current: &str, section: &str) -> Option<String> {
    section_bounds(separator, current, section).map(|(start, end)| {
        let mut out = String::with_capacity(current.len() - (end - start));
        out.push_str(&current[..start]);
        out.push_str(&current[end..]);
        out
    })
}

fn section_bounds(separator: &str, current: &str, section: &str) -> Option<(usize, usize)> {
    let needle = format!("{separator} {section}");
    let header_prefix = format!("{separator} ");
    let mut target_start = None;
    let mut starts = vec![0];
    starts.extend(current.match_indices('\n').map(|(index, _)| index + 1));

    for start in starts {
        let line_end = current[start..]
            .find('\n')
            .map_or(current.len(), |relative| start + relative);
        let line = &current[start..line_end];

        if target_start.is_some() && line.starts_with(&header_prefix) {
            return target_start.map(|section_start| (section_start, start));
        }

        if line == needle {
            target_start = Some(start);
        }
    }

    target_start.map(|start| (start, current.len()))
}

fn enforce_limit(file: MemdirFile, content: &str, limit: usize) -> Result<(), MemoryError> {
    let chars = content.chars().count();
    if chars > limit {
        let _ = file;
        return Err(MemoryError::TooLarge {
            bytes: content.len() as u64,
            max: limit as u64,
        });
    }

    Ok(())
}

fn write_snapshot_if_needed(
    memory: &BuiltinMemory,
    file: MemdirFile,
    previous: &str,
    edit: Edit,
) -> Result<Option<PathBuf>, MemoryError> {
    if previous.is_empty() {
        return Ok(None);
    }

    let tenant_dir = memory.tenant_dir();
    let snapshots_dir = tenant_dir.join("snapshots");

    let path = match memory.snapshot_strategy() {
        SnapshotStrategy::DailyOnFirstWrite => {
            let date = Utc::now().format("%Y-%m-%d");
            let path = snapshots_dir.join(format!("{date}-{}.md", file_stem(file)));
            if path.exists() {
                return Ok(None);
            }
            path
        }
        SnapshotStrategy::BeforeEachReplace if matches!(edit, Edit::Replace | Edit::Delete) => {
            let stamp = Utc::now().format("%Y%m%dT%H%M%S%.fZ");
            snapshots_dir.join(format!("{stamp}-{}.md", file_stem(file)))
        }
        SnapshotStrategy::None | SnapshotStrategy::BeforeEachReplace => return Ok(None),
    };

    write_atomic(&path, previous)?;
    Ok(Some(path))
}

fn write_atomic(path: &Path, content: &str) -> Result<(), MemoryError> {
    let parent = path
        .parent()
        .ok_or_else(|| MemoryError::Io("memdir path has no parent".to_owned()))?;
    fs::create_dir_all(parent).map_err(io_error)?;

    let tmp_path = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("md")
    ));

    {
        let mut tmp = File::create(&tmp_path).map_err(io_error)?;
        tmp.write_all(content.as_bytes()).map_err(io_error)?;
        tmp.sync_all().map_err(io_error)?;
    }

    fs::rename(&tmp_path, path).map_err(io_error)?;
    sync_dir(parent)
}

fn sync_dir(path: &Path) -> Result<(), MemoryError> {
    let dir = OpenOptions::new().read(true).open(path).map_err(io_error)?;
    dir.sync_all().map_err(io_error)
}

fn hash(bytes: &[u8]) -> ContentHash {
    let digest = Sha256::digest(bytes);
    let mut out = [0_u8; 32];
    out.copy_from_slice(&digest);
    ContentHash(out)
}

fn path_for(memory: &BuiltinMemory, file: MemdirFile) -> PathBuf {
    memory.tenant_dir().join(file_name(file))
}

fn lock_path_for(memory: &BuiltinMemory, file: MemdirFile) -> PathBuf {
    memory
        .tenant_dir()
        .join(".locks")
        .join(format!("{}.lock", file_name(file)))
}

fn file_name(file: MemdirFile) -> &'static str {
    match file {
        MemdirFile::User => "USER.md",
        _ => "MEMORY.md",
    }
}

fn file_stem(file: MemdirFile) -> &'static str {
    match file {
        MemdirFile::User => "USER",
        _ => "MEMORY",
    }
}

fn io_error(error: std::io::Error) -> MemoryError {
    MemoryError::Io(format!("memdir io error: {error}"))
}

fn elapsed_ms(started: Instant) -> u32 {
    started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32
}

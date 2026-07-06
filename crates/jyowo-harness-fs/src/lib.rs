//! `jyowo-harness-fs`
//!
//! Canonical safe file I/O primitives for Jyowo harness crates.
//!
//! All write operations are atomic (temp-file + fsync + rename).
//! All path operations refuse symlink components.
//! Secret-bearing writes use owner-only permissions on Unix.

#![forbid(unsafe_code)]

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FsError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Symlink(String),
    #[error("{0}")]
    InvalidPath(String),
}

// ── Public API ──────────────────────────────────────────────────────

/// Read and deserialize a JSON file. Returns `None` if the file does not exist.
pub fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, FsError> {
    match read_file_no_follow(path)? {
        Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        None => Ok(None),
    }
}

/// Read and deserialize a JSON file, ensuring it is owner-only on Unix.
/// Returns `None` if the file does not exist.
pub fn read_secret_json_file<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, FsError> {
    ensure_no_symlink_components(path)?;
    set_owner_only_if_exists_unix(path)?;
    read_json_file(path)
}

/// Serialize `value` and atomically write it to `target_path`.
///
/// Uses temp-file + fsync + rename semantics.
/// If `owner_only` is true, the file is created with `0o600` on Unix.
pub fn write_json_file_atomic<T: Serialize>(
    target_path: &Path,
    value: &T,
    owner_only: bool,
) -> Result<(), FsError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_bytes_file_atomic(target_path, &bytes, owner_only)
}

/// Atomically write raw bytes to `target_path`.
///
/// Uses temp-file + fsync + rename semantics.
/// If `owner_only` is true, the file is created with `0o600` on Unix.
pub fn write_bytes_file_atomic(
    target_path: &Path,
    bytes: &[u8],
    owner_only: bool,
) -> Result<(), FsError> {
    #[cfg(unix)]
    {
        return write_bytes_file_atomic_unix(target_path, bytes, owner_only);
    }

    #[cfg(not(unix))]
    {
        write_bytes_file_atomic_non_unix(target_path, bytes, owner_only)
    }
}

/// Rename `path` to `path.json.invalid`, leaving the original path absent.
/// Returns the quarantine path.
pub fn quarantine_invalid_json_file(path: &Path) -> Result<PathBuf, FsError> {
    #[cfg(unix)]
    {
        let quarantine_path = path.with_extension("json.invalid");
        let Some(parent) = open_parent_dir_no_symlink_for_read(path)? else {
            return Ok(quarantine_path);
        };
        let Some(file) = parent.try_open_existing_file(parent.file_name())? else {
            return Ok(quarantine_path);
        };
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(FsError::InvalidPath("target path is not a file".to_owned()));
        }
        drop(file);
        let quarantine_name = quarantine_path
            .file_name()
            .ok_or_else(|| FsError::InvalidPath("quarantine path has no file name".to_owned()))?;
        parent.rename_file(parent.file_name(), quarantine_name)?;
        parent.sync_all()?;
        return Ok(quarantine_path);
    }

    #[cfg(not(unix))]
    {
        let quarantine_path = path.with_extension("json.invalid");
        ensure_no_symlink_components(path)?;
        ensure_no_symlink_components(&quarantine_path)?;
        if path.exists() {
            std::fs::rename(path, &quarantine_path)?;
        }
        Ok(quarantine_path)
    }
}

/// Remove `path` if it is a regular file (not a symlink).
/// Does nothing if the file does not exist.
pub fn remove_invalid_json_file(path: &Path) -> Result<(), FsError> {
    #[cfg(unix)]
    {
        let Some(parent) = open_parent_dir_no_symlink_for_read(path)? else {
            return Ok(());
        };
        let Some(file) = parent.try_open_existing_file(parent.file_name())? else {
            return Ok(());
        };
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(FsError::InvalidPath("target path is not a file".to_owned()));
        }
        drop(file);
        match parent.unlink_file(parent.file_name()) {
            Ok(()) | Err(rustix::io::Errno::NOENT) => {}
            Err(error) => {
                return Err(FsError::Io(std::io::Error::other(format!(
                    "cleanup failed: {error}"
                ))));
            }
        }
        parent.sync_all()?;
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        ensure_no_symlink_components(path)?;
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

/// Ensure every component of `path` is a real directory (no symlinks).
/// Creates missing directories with `0o700` on Unix.
pub fn ensure_app_dir_no_symlink(path: &Path) -> Result<(), FsError> {
    #[cfg(unix)]
    {
        return ensure_app_dir_no_symlink_unix(path);
    }

    #[cfg(not(unix))]
    {
        ensure_app_dir_no_symlink_non_unix(path)
    }
}

/// Verify that no component of `path` is a symlink.
pub fn ensure_no_symlink_components(path: &Path) -> Result<(), FsError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                current.push(component.as_os_str());
                continue;
            }
            Component::CurDir => continue,
            Component::ParentDir => {
                return Err(FsError::InvalidPath(
                    "path must not use parent directory components".to_owned(),
                ));
            }
            Component::Normal(value) => current.push(value),
        }
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(FsError::Symlink("path must not use symlinks".to_owned()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Read a regular file without following symlinks. Returns `None` if not found.
pub fn read_file_no_follow(path: &Path) -> Result<Option<Vec<u8>>, FsError> {
    #[cfg(unix)]
    {
        return read_file_no_follow_unix(path);
    }

    #[cfg(not(unix))]
    {
        ensure_no_symlink_components(path)?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        let mut file = match options.open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }
}

/// Set the file permissions to owner-only (0o600) on Unix. No-op on other platforms.
pub fn set_owner_only_file_if_unix(file: &File) -> Result<(), FsError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = file;
    }
    Ok(())
}

/// If `path` exists as a regular file, tighten permissions to owner-only.
pub fn set_owner_only_if_exists_unix(path: &Path) -> Result<(), FsError> {
    #[cfg(unix)]
    {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(FsError::Symlink("path must not use symlinks".to_owned()));
            }
            Ok(metadata) if metadata.is_file() => {
                let mut options = std::fs::OpenOptions::new();
                options.read(true);
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_NOFOLLOW);
                let file = options.open(path)?;
                set_owner_only_file_if_unix(&file)
            }
            Ok(_) => Err(FsError::InvalidPath("path is not a file".to_owned())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// Sync the parent directory of `path`.
pub fn sync_directory(path: &Path) -> Result<(), FsError> {
    #[cfg(unix)]
    {
        let dir = File::open(path)?;
        dir.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

// ── Unix: NoFollowParentDir ─────────────────────────────────────────

/// Low-level directory handle that refuses to follow symlinks.
/// All file operations through this handle are relative to the opened directory.
#[cfg(unix)]
pub struct NoFollowParentDir {
    pub directory: File,
    file_name: std::ffi::OsString,
}

#[cfg(unix)]
impl NoFollowParentDir {
    pub fn file_name(&self) -> &std::ffi::OsStr {
        &self.file_name
    }

    pub fn sync_all(&self) -> Result<(), FsError> {
        self.directory.sync_all()?;
        Ok(())
    }

    pub fn create_new_file(
        &self,
        file_name: &std::ffi::OsStr,
        owner_only: bool,
    ) -> Result<File, FsError> {
        use rustix::fs::{Mode, OFlags};

        let mode = if owner_only { 0o600 } else { 0o666 };
        self.open_file(
            file_name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(mode),
        )
    }

    pub fn open_existing_file(&self, file_name: &std::ffi::OsStr) -> Result<File, FsError> {
        self.try_open_existing_file(file_name)?
            .ok_or_else(|| FsError::Io(std::io::Error::other("open failed: not found")))
    }

    pub fn try_open_existing_file(
        &self,
        file_name: &std::ffi::OsStr,
    ) -> Result<Option<File>, FsError> {
        use rustix::fs::{Mode, OFlags};

        self.try_open_file(
            file_name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0),
        )
    }

    pub fn open_or_create_append_file(&self, file_name: &std::ffi::OsStr) -> Result<File, FsError> {
        use rustix::fs::{Mode, OFlags};

        self.open_file(
            file_name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::APPEND | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
    }

    pub fn open_or_create_read_write_file(
        &self,
        file_name: &std::ffi::OsStr,
    ) -> Result<File, FsError> {
        use rustix::fs::{Mode, OFlags};

        self.open_file(
            file_name,
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
    }

    pub fn rename_file(
        &self,
        source_name: &std::ffi::OsStr,
        destination_name: &std::ffi::OsStr,
    ) -> Result<(), FsError> {
        rustix::fs::renameat(
            &self.directory,
            Path::new(source_name),
            &self.directory,
            Path::new(destination_name),
        )
        .map_err(|error| FsError::Io(std::io::Error::other(format!("commit failed: {error}"))))
    }

    pub fn unlink_file(&self, file_name: &std::ffi::OsStr) -> Result<(), rustix::io::Errno> {
        rustix::fs::unlinkat(
            &self.directory,
            Path::new(file_name),
            rustix::fs::AtFlags::empty(),
        )
    }

    pub fn unlink_file_if_exists(&self, file_name: &std::ffi::OsStr) {
        match self.unlink_file(file_name) {
            Ok(()) | Err(rustix::io::Errno::NOENT) => {}
            Err(_) => {}
        }
    }

    fn open_file(
        &self,
        file_name: &std::ffi::OsStr,
        flags: rustix::fs::OFlags,
        mode: rustix::fs::Mode,
    ) -> Result<File, FsError> {
        self.try_open_file(file_name, flags, mode)?
            .ok_or_else(|| FsError::Io(std::io::Error::other("open failed: not found")))
    }

    fn try_open_file(
        &self,
        file_name: &std::ffi::OsStr,
        flags: rustix::fs::OFlags,
        mode: rustix::fs::Mode,
    ) -> Result<Option<File>, FsError> {
        match rustix::fs::openat(&self.directory, Path::new(file_name), flags, mode) {
            Ok(fd) => Ok(Some(File::from(fd))),
            Err(rustix::io::Errno::NOENT) => Ok(None),
            Err(rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) => {
                Err(FsError::Symlink("path must not use symlinks".to_owned()))
            }
            Err(error) => Err(FsError::Io(std::io::Error::other(format!(
                "open failed: {error}"
            )))),
        }
    }
}

// ── Unix: parent directory helpers ──────────────────────────────────

/// Open the parent directory of `path` for reading, rejecting symlink components.
/// Returns `None` if the parent directory (or an intermediate directory) does not exist.
#[cfg(unix)]
pub fn open_parent_dir_no_symlink_for_read(
    path: &Path,
) -> Result<Option<NoFollowParentDir>, FsError> {
    let mut components = Vec::new();
    let mut absolute = false;
    for component in path.components() {
        match component {
            Component::Prefix(_) => {
                return Err(FsError::InvalidPath(
                    "path has unsupported prefix".to_owned(),
                ));
            }
            Component::RootDir => absolute = true,
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(FsError::InvalidPath(
                    "path must not use parent directory components".to_owned(),
                ));
            }
            Component::Normal(value) => components.push(value.to_os_string()),
        }
    }
    let file_name = components
        .pop()
        .ok_or_else(|| FsError::InvalidPath("path has no file name".to_owned()))?;
    let mut directory = if absolute {
        File::open(Path::new("/"))?
    } else {
        File::open(Path::new("."))?
    };

    for component in components {
        let fd = match rustix::fs::openat(
            &directory,
            Path::new(&component),
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_raw_mode(0),
        ) {
            Ok(fd) => fd,
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) => {
                return Err(FsError::Symlink("path must not use symlinks".to_owned()));
            }
            Err(error) => {
                return Err(FsError::Io(std::io::Error::other(format!(
                    "directory unavailable: {error}"
                ))));
            }
        };
        directory = File::from(fd);
    }

    Ok(Some(NoFollowParentDir {
        directory,
        file_name,
    }))
}

/// Open the parent directory of `path` for writing, creating intermediate directories.
/// Creates missing directories with `0o700`. Rejects symlink components.
#[cfg(unix)]
pub fn open_parent_dir_no_symlink_for_write(path: &Path) -> Result<NoFollowParentDir, FsError> {
    let mut components = Vec::new();
    let mut absolute = false;
    for component in path.components() {
        match component {
            Component::Prefix(_) => {
                return Err(FsError::InvalidPath(
                    "path has unsupported prefix".to_owned(),
                ));
            }
            Component::RootDir => absolute = true,
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(FsError::InvalidPath(
                    "path must not use parent directory components".to_owned(),
                ));
            }
            Component::Normal(value) => components.push(value.to_os_string()),
        }
    }
    let file_name = components
        .pop()
        .ok_or_else(|| FsError::InvalidPath("path has no file name".to_owned()))?;
    let mut directory = if absolute {
        File::open(Path::new("/"))?
    } else {
        File::open(Path::new("."))?
    };

    for component in components {
        match rustix::fs::mkdirat(
            &directory,
            Path::new(&component),
            rustix::fs::Mode::from_raw_mode(0o700),
        ) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(error) => {
                return Err(FsError::Io(std::io::Error::other(format!(
                    "directory unavailable: {error}"
                ))));
            }
        }
        let fd = rustix::fs::openat(
            &directory,
            Path::new(&component),
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_raw_mode(0),
        )
        .map_err(|error| {
            if error == rustix::io::Errno::LOOP || error == rustix::io::Errno::NOTDIR {
                FsError::Symlink("path must not use symlinks".to_owned())
            } else {
                FsError::Io(std::io::Error::other(format!(
                    "directory unavailable: {error}"
                )))
            }
        })?;
        directory = File::from(fd);
    }

    Ok(NoFollowParentDir {
        directory,
        file_name,
    })
}

// ── Internal implementations ────────────────────────────────────────

#[cfg(unix)]
fn read_file_no_follow_unix(path: &Path) -> Result<Option<Vec<u8>>, FsError> {
    let Some(parent) = open_parent_dir_no_symlink_for_read(path)? else {
        return Ok(None);
    };
    let Some(mut file) = parent.try_open_existing_file(parent.file_name())? else {
        return Ok(None);
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(FsError::InvalidPath("path is not a file".to_owned()));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

#[cfg(unix)]
fn write_bytes_file_atomic_unix(
    target_path: &Path,
    bytes: &[u8],
    owner_only: bool,
) -> Result<(), FsError> {
    let parent = open_parent_dir_no_symlink_for_write(target_path)?;
    let temp_name = std::ffi::OsString::from(format!(
        "{}.{}.tmp",
        target_path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("store.json"),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let mut temp_file = parent.create_new_file(&temp_name, owner_only)?;
    if let Err(error) = temp_file.write_all(bytes) {
        let _ = parent.unlink_file(&temp_name);
        return Err(error.into());
    }
    if let Err(error) = temp_file.sync_all() {
        let _ = parent.unlink_file(&temp_name);
        return Err(error.into());
    }
    if owner_only {
        set_owner_only_file_if_unix(&temp_file)?;
    }
    drop(temp_file);
    parent.rename_file(&temp_name, parent.file_name())?;
    if owner_only {
        let file = parent.open_existing_file(parent.file_name())?;
        set_owner_only_file_if_unix(&file)?;
    }
    parent.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn write_bytes_file_atomic_non_unix(
    target_path: &Path,
    bytes: &[u8],
    owner_only: bool,
) -> Result<(), FsError> {
    let parent = target_path
        .parent()
        .ok_or_else(|| FsError::InvalidPath("path has no parent".to_owned()))?;
    ensure_app_dir_no_symlink(parent)?;
    let temp_path = target_path.with_file_name(format!(
        "{}.{}.tmp",
        target_path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("store.json"),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    ensure_no_symlink_components(&temp_path)?;
    let mut open_options = std::fs::OpenOptions::new();
    open_options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open_options.custom_flags(libc::O_NOFOLLOW);
        if owner_only {
            open_options.mode(0o600);
        }
    }
    let mut temp_file = open_options.open(&temp_path).map_err(|error| {
        FsError::Io(std::io::Error::other(format!("temp open failed: {error}")))
    })?;
    if let Err(error) = temp_file.write_all(bytes) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error.into());
    }
    if let Err(error) = temp_file.sync_all() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error.into());
    }
    drop(temp_file);
    ensure_no_symlink_components(target_path)?;
    std::fs::rename(&temp_path, target_path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        FsError::Io(std::io::Error::other(format!("commit failed: {error}")))
    })?;
    #[cfg(unix)]
    if owner_only {
        set_owner_only_if_exists_unix(target_path)?;
    }
    sync_directory(parent)
}

#[cfg(unix)]
fn ensure_app_dir_no_symlink_unix(path: &Path) -> Result<(), FsError> {
    let mut components = Vec::new();
    let mut absolute = false;
    for component in path.components() {
        match component {
            Component::Prefix(_) => {
                return Err(FsError::InvalidPath(
                    "path has unsupported prefix".to_owned(),
                ));
            }
            Component::RootDir => absolute = true,
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(FsError::InvalidPath(
                    "path must not use parent directory components".to_owned(),
                ));
            }
            Component::Normal(value) => components.push(value.to_os_string()),
        }
    }

    let mut directory = if absolute {
        File::open(Path::new("/"))?
    } else {
        File::open(Path::new("."))?
    };

    for component in components {
        match rustix::fs::mkdirat(
            &directory,
            Path::new(&component),
            rustix::fs::Mode::from_raw_mode(0o700),
        ) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(error) => {
                return Err(FsError::Io(std::io::Error::other(format!(
                    "directory unavailable: {error}"
                ))));
            }
        }
        let fd = rustix::fs::openat(
            &directory,
            Path::new(&component),
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_raw_mode(0),
        )
        .map_err(|error| {
            if error == rustix::io::Errno::LOOP || error == rustix::io::Errno::NOTDIR {
                FsError::Symlink("path must not use symlinks".to_owned())
            } else {
                FsError::Io(std::io::Error::other(format!(
                    "directory unavailable: {error}"
                )))
            }
        })?;
        directory = File::from(fd);
    }

    Ok(())
}

#[cfg(not(unix))]
fn ensure_app_dir_no_symlink_non_unix(path: &Path) -> Result<(), FsError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                current.push(component.as_os_str());
                continue;
            }
            Component::CurDir => continue,
            Component::ParentDir => {
                return Err(FsError::InvalidPath(
                    "path must not use parent directory components".to_owned(),
                ));
            }
            Component::Normal(value) => current.push(value),
        }

        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(FsError::Symlink("path must not use symlinks".to_owned()));
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(FsError::InvalidPath(
                    "component is not a directory".to_owned(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current)?;
                let metadata = std::fs::symlink_metadata(&current)?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(FsError::Symlink("path must not use symlinks".to_owned()));
                }
            }
            Err(error) => return Err(error.into()),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct TestRecord {
        value: String,
    }

    fn canonical_temp_root(temp: &tempfile::TempDir) -> PathBuf {
        temp.path().canonicalize().expect("canonical tempdir")
    }

    #[test]
    fn read_json_file_returns_none_for_missing_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("nonexistent.json");

        let result = read_json_file::<TestRecord>(&path).expect("read should succeed");
        assert!(result.is_none());
    }

    #[test]
    fn read_json_file_deserializes_existing_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("settings.json");
        std::fs::write(&path, br#"{"value":"hello"}"#).expect("seed file");

        let result = read_json_file::<TestRecord>(&path)
            .expect("read should succeed")
            .expect("record should exist");
        assert_eq!(
            result,
            TestRecord {
                value: "hello".to_owned()
            }
        );
    }

    #[test]
    fn read_json_file_reports_error_for_invalid_json() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("settings.json");
        std::fs::write(&path, b"{not-json").expect("invalid json");

        let error = read_json_file::<TestRecord>(&path).expect_err("should fail");
        assert!(matches!(error, FsError::Json(_)));
        assert!(path.exists(), "read should not quarantine");
    }

    #[test]
    fn write_json_file_atomic_replaces_via_temp_file() {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("settings.json");
        std::fs::write(&path, br#"{"value":"old"}"#).expect("seed old file");
        #[cfg(unix)]
        let old_inode = std::fs::metadata(&path).unwrap().ino();

        write_json_file_atomic(
            &path,
            &TestRecord {
                value: "new".to_owned(),
            },
            false,
        )
        .expect("write should succeed");

        let saved: TestRecord = serde_json::from_slice(&std::fs::read(&path).unwrap())
            .expect("saved json should parse");
        assert_eq!(
            saved,
            TestRecord {
                value: "new".to_owned()
            }
        );
        #[cfg(unix)]
        assert_ne!(std::fs::metadata(&path).unwrap().ino(), old_inode);
        assert!(
            std::fs::read_dir(temp_root).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp")),
            "no temp files left behind"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_json_file_atomic_owner_only_creates_0600() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("secret.json");

        write_json_file_atomic(
            &path,
            &TestRecord {
                value: "secret".to_owned(),
            },
            true,
        )
        .expect("write should succeed");

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn read_secret_json_file_tightens_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("secret.json");
        std::fs::write(&path, br#"{"value":"secret"}"#).expect("seed");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("wide permissions");

        let loaded = read_secret_json_file::<TestRecord>(&path)
            .expect("read should succeed")
            .expect("record should exist");
        assert_eq!(
            loaded,
            TestRecord {
                value: "secret".to_owned()
            }
        );
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn quarantine_invalid_json_file_moves_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("settings.json");
        std::fs::write(&path, b"{not-json").expect("invalid json");

        let quarantine_path =
            quarantine_invalid_json_file(&path).expect("quarantine should succeed");

        assert!(!path.exists());
        assert!(quarantine_path.exists());
        assert_eq!(quarantine_path.extension().unwrap(), "invalid");
    }

    #[cfg(unix)]
    #[test]
    fn quarantine_invalid_json_file_rejects_symlink_without_moving_target() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let target = temp_root.join("target.json");
        let path = temp_root.join("settings.json");
        std::fs::write(&target, b"{not-json").expect("invalid target");
        std::os::unix::fs::symlink(&target, &path).expect("symlink");

        let error = quarantine_invalid_json_file(&path).expect_err("symlink should be rejected");

        assert!(matches!(error, FsError::Symlink(_)));
        assert!(target.exists());
        assert!(std::fs::symlink_metadata(&path)
            .expect("link metadata")
            .file_type()
            .is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn remove_invalid_json_file_rejects_symlink_without_deleting_target() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let target = temp_root.join("target.json");
        let path = temp_root.join("settings.json");
        std::fs::write(&target, b"{not-json").expect("invalid target");
        std::os::unix::fs::symlink(&target, &path).expect("symlink");

        let error = remove_invalid_json_file(&path).expect_err("symlink should be rejected");

        assert!(matches!(error, FsError::Symlink(_)));
        assert!(target.exists());
        assert!(std::fs::symlink_metadata(&path)
            .expect("link metadata")
            .file_type()
            .is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn write_json_file_atomic_rejects_symlink_parent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let external = tempfile::tempdir().expect("external");
        let symlinked_parent = temp_root.join("config");
        std::os::unix::fs::symlink(external.path(), &symlinked_parent).expect("symlink");
        let path = symlinked_parent.join("settings.json");

        let error = write_json_file_atomic(
            &path,
            &TestRecord {
                value: "blocked".to_owned(),
            },
            false,
        )
        .expect_err("symlink parent should be rejected");

        assert!(matches!(error, FsError::Symlink(_)));
        assert!(!external.path().join("settings.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_no_symlink_components_rejects_symlink_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let target = tempfile::NamedTempFile::new().expect("target file");
        let path = temp_root.join("link.json");
        std::os::unix::fs::symlink(target.path(), &path).expect("symlink");

        let error = ensure_no_symlink_components(&path).expect_err("symlink should be rejected");

        assert!(matches!(error, FsError::Symlink(_)));
    }

    #[test]
    fn ensure_no_symlink_components_rejects_parent_dir() {
        // Use a temp directory as the root so we don't hit OS-level symlinks (/etc is a symlink on macOS).
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("subdir").join("..").join("file.json");

        let error = ensure_no_symlink_components(&path).expect_err("parent dir should be rejected");

        assert!(matches!(error, FsError::InvalidPath(_)));
    }

    #[cfg(unix)]
    #[test]
    fn set_owner_only_if_exists_unix_tightens_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("wide.json");
        std::fs::write(&path, b"{}").expect("seed");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("wide permissions");

        set_owner_only_if_exists_unix(&path).expect("tighten should succeed");

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn set_owner_only_if_exists_unix_noop_on_missing_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("nonexistent.json");

        set_owner_only_if_exists_unix(&path).expect("missing file should not error");
    }

    #[cfg(unix)]
    #[test]
    fn read_file_no_follow_rejects_symlink_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let target = tempfile::NamedTempFile::new().expect("target file");
        std::fs::write(target.path(), b"secret").expect("write target");
        let path = temp_root.join("link.json");
        std::os::unix::fs::symlink(target.path(), &path).expect("symlink");

        let error = read_file_no_follow(&path).expect_err("symlink should be rejected");

        assert!(matches!(error, FsError::Symlink(_)));
    }

    #[cfg(unix)]
    #[test]
    fn read_file_no_follow_rejects_symlink_parent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let outside = tempfile::tempdir().expect("outside tempdir");
        std::fs::write(outside.path().join("target.txt"), "secret").expect("target file");
        std::os::unix::fs::symlink(outside.path(), temp_root.join("linked"))
            .expect("symlink parent");

        let error = read_file_no_follow(&temp_root.join("linked/target.txt"))
            .expect_err("symlink parent should be rejected");

        assert!(matches!(error, FsError::Symlink(_)));
    }
}

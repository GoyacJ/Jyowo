#[allow(unused_imports)]
use super::app::*;
#[allow(unused_imports)]
use super::artifacts::*;
#[allow(unused_imports)]
use super::automations::*;
#[allow(unused_imports)]
use super::constants::*;
#[allow(unused_imports)]
use super::contracts::*;
#[allow(unused_imports)]
use super::conversations::*;
#[allow(unused_imports)]
use super::error::*;
#[allow(unused_imports)]
use super::evals::*;
#[allow(unused_imports)]
use super::mcp::*;
#[allow(unused_imports)]
use super::memory::*;
use super::model_settings::{OfficialQuotaFlights, ProviderProbeFlights};
#[allow(unused_imports)]
use super::plugins::*;
#[allow(unused_imports)]
use super::providers::*;
#[allow(unused_imports)]
use super::runtime::*;
#[allow(unused_imports)]
use super::skills::*;
#[allow(unused_imports)]
use super::validation::*;
use super::*;
use fs2::FileExt;
use harness_model::ProviderAccountUsageRegistry;
use serde::{de::DeserializeOwned, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};

pub(crate) mod automation;
mod global_config;
mod mcp;
pub(crate) mod plugin;
mod project_config;
pub(crate) mod skill;

pub use automation::DesktopAutomationStore;
pub use automation::NoWorkspaceAutomationStore;
pub use global_config::GlobalConfigStore;
pub use mcp::DesktopMcpDiagnosticStore;
pub(crate) use mcp::DesktopMcpServerStore;
pub use plugin::DesktopPluginStore;
pub use project_config::ProjectConfigStore;
pub use skill::DesktopSkillStore;

pub(crate) fn read_json_file<T: DeserializeOwned>(
    path: &Path,
    label: &str,
) -> Result<Option<T>, CommandErrorPayload> {
    match read_file_no_follow(path, label)? {
        Some(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| runtime_operation_failed(format!("{label} parse failed: {error}"))),
        None => Ok(None),
    }
}

pub(crate) fn read_json_file_invalid_payload<T: DeserializeOwned>(
    path: &Path,
    label: &str,
) -> Result<Option<T>, CommandErrorPayload> {
    match read_file_no_follow(path, label)? {
        Some(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| invalid_payload(format!("{label} parse failed: {error}"))),
        None => Ok(None),
    }
}

pub(crate) fn read_secret_json_file<T: DeserializeOwned>(
    path: &Path,
    label: &str,
) -> Result<Option<T>, CommandErrorPayload> {
    ensure_no_symlink_components(path, &format!("{label} file"))?;
    set_owner_only_if_exists_unix(path, label)?;
    read_json_file(path, label)
}

pub(crate) fn read_secret_json_file_or_default_on_blank<T: Default + DeserializeOwned>(
    path: &Path,
    label: &str,
) -> Result<T, CommandErrorPayload> {
    ensure_no_symlink_components(path, &format!("{label} file"))?;
    set_owner_only_if_exists_unix(path, label)?;
    let Some(bytes) = read_file_no_follow(path, label)? else {
        return Ok(T::default());
    };
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(T::default());
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| invalid_payload(format!("{label} parse failed: {error}")))
}

pub(crate) fn read_json_file_or_remove_invalid<T: DeserializeOwned>(
    path: &Path,
    label: &str,
) -> Result<Option<T>, CommandErrorPayload> {
    ensure_no_symlink_components(path, &format!("{label} file"))?;
    match read_file_no_follow(path, label)? {
        Some(bytes) => match serde_json::from_slice(&bytes) {
            Ok(record) => Ok(Some(record)),
            Err(_) => {
                remove_invalid_json_file(path, label)?;
                Ok(None)
            }
        },
        None => Ok(None),
    }
}

pub(crate) fn read_secret_json_file_or_remove_invalid<T: DeserializeOwned>(
    path: &Path,
    label: &str,
) -> Result<Option<T>, CommandErrorPayload> {
    ensure_no_symlink_components(path, &format!("{label} file"))?;
    set_owner_only_if_exists_unix(path, label)?;
    read_json_file_or_remove_invalid(path, label)
}

pub(crate) fn remove_invalid_json_file(
    path: &Path,
    label: &str,
) -> Result<(), CommandErrorPayload> {
    #[cfg(unix)]
    {
        let Some(parent) = open_parent_dir_no_symlink_for_read(path, &format!("{label} file"))?
        else {
            return Ok(());
        };
        let Some(file) = parent.try_open_existing_file(parent.file_name(), label)? else {
            return Ok(());
        };
        let metadata = file.metadata().map_err(|error| {
            runtime_operation_failed(format!("{label} metadata failed: {error}"))
        })?;
        if !metadata.is_file() {
            return Err(runtime_operation_failed(format!(
                "{label} target path is not a file"
            )));
        }
        drop(file);
        match parent.unlink_file(parent.file_name()) {
            Ok(()) | Err(rustix::io::Errno::NOENT) => {}
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "{label} cleanup failed: {error}"
                )));
            }
        }
        parent.sync_all(label)?;
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        ensure_no_symlink_components(path, &format!("{label} file"))?;
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(runtime_operation_failed(format!(
                "{label} cleanup failed: {error}"
            ))),
        }
    }
}

pub(crate) fn write_json_file_atomic<T: Serialize + ?Sized>(
    target_path: &Path,
    label: &str,
    value: &T,
) -> Result<(), CommandErrorPayload> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        runtime_operation_failed(format!("{label} serialization failed: {error}"))
    })?;
    write_bytes_file_atomic(target_path, "store.json", label, &bytes, false)
}

fn read_file_no_follow(path: &Path, label: &str) -> Result<Option<Vec<u8>>, CommandErrorPayload> {
    #[cfg(unix)]
    {
        return read_file_no_follow_unix(path, label);
    }

    #[cfg(not(unix))]
    {
        ensure_no_symlink_components(path, &format!("{label} file"))?;
        let mut options = OpenOptions::new();
        options.read(true);
        let mut file = match options.open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "{label} read failed: {error}"
                )));
            }
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| runtime_operation_failed(format!("{label} read failed: {error}")))?;
        Ok(Some(bytes))
    }
}

#[cfg(unix)]
fn read_file_no_follow_unix(
    path: &Path,
    label: &str,
) -> Result<Option<Vec<u8>>, CommandErrorPayload> {
    let Some(parent) = open_parent_dir_no_symlink_for_read(path, &format!("{label} directory"))?
    else {
        return Ok(None);
    };
    let Some(mut file) = parent.try_open_existing_file(parent.file_name(), label)? else {
        return Ok(None);
    };
    let metadata = file
        .metadata()
        .map_err(|error| runtime_operation_failed(format!("{label} metadata failed: {error}")))?;
    if !metadata.is_file() {
        return Err(runtime_operation_failed(format!(
            "{label} path is not a file"
        )));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| runtime_operation_failed(format!("{label} read failed: {error}")))?;
    Ok(Some(bytes))
}

#[cfg(unix)]
fn open_parent_dir_no_symlink_for_read(
    path: &Path,
    label: &str,
) -> Result<Option<NoFollowParentDir>, CommandErrorPayload> {
    let mut components = Vec::new();
    let mut absolute = false;
    for component in path.components() {
        match component {
            Component::Prefix(_) => {
                return Err(runtime_operation_failed(format!(
                    "{label} has unsupported path prefix"
                )));
            }
            Component::RootDir => absolute = true,
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(runtime_operation_failed(format!(
                    "{label} must not use parent directory components"
                )));
            }
            Component::Normal(value) => components.push(value.to_os_string()),
        }
    }
    let file_name = components
        .pop()
        .ok_or_else(|| runtime_operation_failed(format!("{label} path has no file name")))?;
    let mut directory = if absolute {
        File::open(Path::new("/")).map_err(|error| {
            runtime_operation_failed(format!("{label} root open failed: {error}"))
        })?
    } else {
        File::open(Path::new(".")).map_err(|error| {
            runtime_operation_failed(format!("{label} current directory open failed: {error}"))
        })?
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
                return Err(runtime_operation_failed(format!(
                    "{label} must not use symlinks"
                )));
            }
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "{label} directory unavailable: {error}"
                )));
            }
        };
        directory = File::from(fd);
    }

    Ok(Some(NoFollowParentDir {
        directory,
        file_name,
    }))
}

pub(crate) fn write_secret_json_file_atomic<T: Serialize + ?Sized>(
    target_path: &Path,
    label: &str,
    value: &T,
) -> Result<(), CommandErrorPayload> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        runtime_operation_failed(format!("{label} serialization failed: {error}"))
    })?;
    write_bytes_file_atomic(target_path, "store.json", label, &bytes, true)
}

#[allow(dead_code)]
pub(crate) fn quarantine_invalid_json_file(
    path: &Path,
    label: &str,
) -> Result<PathBuf, CommandErrorPayload> {
    #[cfg(unix)]
    {
        let quarantine_path = path.with_extension("json.invalid");
        let Some(parent) = open_parent_dir_no_symlink_for_read(path, &format!("{label} file"))?
        else {
            return Ok(quarantine_path);
        };
        let Some(file) = parent.try_open_existing_file(parent.file_name(), label)? else {
            return Ok(quarantine_path);
        };
        let metadata = file.metadata().map_err(|error| {
            runtime_operation_failed(format!("{label} metadata failed: {error}"))
        })?;
        if !metadata.is_file() {
            return Err(runtime_operation_failed(format!(
                "{label} target path is not a file"
            )));
        }
        drop(file);
        let quarantine_name = quarantine_path.file_name().ok_or_else(|| {
            runtime_operation_failed(format!("{label} quarantine path has no file name"))
        })?;
        parent.rename_file(parent.file_name(), quarantine_name, label)?;
        parent.sync_all(label)?;
        return Ok(quarantine_path);
    }

    #[cfg(not(unix))]
    {
        ensure_no_symlink_components(path, &format!("{label} file"))?;
        let quarantine_path = path.with_extension("json.invalid");
        ensure_no_symlink_components(&quarantine_path, &format!("{label} quarantine file"))?;
        if path.exists() {
            std::fs::rename(path, &quarantine_path).map_err(|error| {
                runtime_operation_failed(format!("{label} quarantine failed: {error}"))
            })?;
        }
        Ok(quarantine_path)
    }
}

pub(crate) fn ensure_app_dir_no_symlink(
    path: &Path,
    label: &str,
) -> Result<(), CommandErrorPayload> {
    #[cfg(unix)]
    {
        return ensure_app_dir_no_symlink_unix(path, label);
    }

    #[cfg(not(unix))]
    {
        ensure_app_dir_no_symlink_non_unix(path, label)
    }
}

#[cfg(unix)]
fn ensure_app_dir_no_symlink_unix(path: &Path, label: &str) -> Result<(), CommandErrorPayload> {
    let mut components = Vec::new();
    let mut absolute = false;
    for component in path.components() {
        match component {
            Component::Prefix(_) => {
                return Err(runtime_operation_failed(format!(
                    "{label} has unsupported path prefix"
                )));
            }
            Component::RootDir => absolute = true,
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(runtime_operation_failed(format!(
                    "{label} must not use parent directory components"
                )));
            }
            Component::Normal(value) => components.push(value.to_os_string()),
        }
    }

    let mut directory = if absolute {
        File::open(Path::new("/")).map_err(|error| {
            runtime_operation_failed(format!("{label} root open failed: {error}"))
        })?
    } else {
        File::open(Path::new(".")).map_err(|error| {
            runtime_operation_failed(format!("{label} current directory open failed: {error}"))
        })?
    };

    for component in components {
        match rustix::fs::mkdirat(
            &directory,
            Path::new(&component),
            rustix::fs::Mode::from_raw_mode(0o700),
        ) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "{label} directory unavailable: {error}"
                )));
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
                runtime_operation_failed(format!("{label} must not use symlinks"))
            } else {
                runtime_operation_failed(format!("{label} directory unavailable: {error}"))
            }
        })?;
        directory = File::from(fd);
    }

    Ok(())
}

#[cfg(not(unix))]
fn ensure_app_dir_no_symlink_non_unix(path: &Path, label: &str) -> Result<(), CommandErrorPayload> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                current.push(component.as_os_str());
                continue;
            }
            Component::CurDir => continue,
            Component::ParentDir => {
                return Err(runtime_operation_failed(format!(
                    "{label} must not use parent directory components"
                )));
            }
            Component::Normal(value) => current.push(value),
        }

        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(runtime_operation_failed(format!(
                    "{label} must not use symlinks"
                )));
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(runtime_operation_failed(format!(
                    "{label} component is not a directory"
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current).map_err(|error| {
                    runtime_operation_failed(format!("{label} directory unavailable: {error}"))
                })?;
                let metadata = std::fs::symlink_metadata(&current).map_err(|error| {
                    runtime_operation_failed(format!("{label} metadata unavailable: {error}"))
                })?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(runtime_operation_failed(format!(
                        "{label} must not use symlinks"
                    )));
                }
            }
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "{label} metadata unavailable: {error}"
                )));
            }
        }
    }

    Ok(())
}

#[cfg(unix)]
struct NoFollowParentDir {
    directory: File,
    file_name: std::ffi::OsString,
}

#[cfg(unix)]
impl NoFollowParentDir {
    fn file_name(&self) -> &OsStr {
        &self.file_name
    }

    fn sync_all(&self, label: &str) -> Result<(), CommandErrorPayload> {
        self.directory.sync_all().map_err(|error| {
            runtime_operation_failed(format!("{label} directory sync failed: {error}"))
        })
    }

    fn create_new_file(
        &self,
        file_name: &OsStr,
        owner_only: bool,
        label: &str,
    ) -> Result<File, CommandErrorPayload> {
        use rustix::fs::{Mode, OFlags};

        let mode = if owner_only { 0o600 } else { 0o666 };
        self.open_file(
            file_name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(mode),
            label,
            "open",
        )
    }

    fn open_existing_file(
        &self,
        file_name: &OsStr,
        label: &str,
    ) -> Result<File, CommandErrorPayload> {
        self.try_open_existing_file(file_name, label)?
            .ok_or_else(|| runtime_operation_failed(format!("{label} open failed: not found")))
    }

    fn try_open_existing_file(
        &self,
        file_name: &OsStr,
        label: &str,
    ) -> Result<Option<File>, CommandErrorPayload> {
        use rustix::fs::{Mode, OFlags};

        self.try_open_file(
            file_name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0),
            label,
            "open",
        )
    }

    fn ensure_existing_file_is_regular_or_missing(
        &self,
        file_name: &OsStr,
        label: &str,
    ) -> Result<(), CommandErrorPayload> {
        let Some(file) = self.try_open_existing_file(file_name, label)? else {
            return Ok(());
        };
        let metadata = file.metadata().map_err(|error| {
            runtime_operation_failed(format!("{label} metadata failed: {error}"))
        })?;
        if metadata.is_file() {
            Ok(())
        } else {
            Err(runtime_operation_failed(format!(
                "{label} target path is not a file"
            )))
        }
    }

    fn open_or_create_append_file(
        &self,
        file_name: &OsStr,
        label: &str,
    ) -> Result<File, CommandErrorPayload> {
        use rustix::fs::{Mode, OFlags};

        self.open_file(
            file_name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::APPEND | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
            label,
            "open",
        )
    }

    fn open_or_create_read_write_file(
        &self,
        file_name: &OsStr,
        label: &str,
    ) -> Result<File, CommandErrorPayload> {
        use rustix::fs::{Mode, OFlags};

        self.open_file(
            file_name,
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
            label,
            "open",
        )
    }

    fn open_file(
        &self,
        file_name: &OsStr,
        flags: rustix::fs::OFlags,
        mode: rustix::fs::Mode,
        label: &str,
        action: &str,
    ) -> Result<File, CommandErrorPayload> {
        self.try_open_file(file_name, flags, mode, label, action)?
            .ok_or_else(|| runtime_operation_failed(format!("{label} {action} failed: not found")))
    }

    fn try_open_file(
        &self,
        file_name: &OsStr,
        flags: rustix::fs::OFlags,
        mode: rustix::fs::Mode,
        label: &str,
        action: &str,
    ) -> Result<Option<File>, CommandErrorPayload> {
        match rustix::fs::openat(&self.directory, Path::new(file_name), flags, mode) {
            Ok(fd) => Ok(Some(File::from(fd))),
            Err(rustix::io::Errno::NOENT) => Ok(None),
            Err(rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) => {
                Err(runtime_operation_failed(format!(
                    "{label} must not use symlinks or non-directory components"
                )))
            }
            Err(error) => Err(runtime_operation_failed(format!(
                "{label} {action} failed: {error}"
            ))),
        }
    }

    fn rename_file(
        &self,
        source_name: &OsStr,
        destination_name: &OsStr,
        label: &str,
    ) -> Result<(), CommandErrorPayload> {
        rustix::fs::renameat(
            &self.directory,
            Path::new(source_name),
            &self.directory,
            Path::new(destination_name),
        )
        .map_err(|error| runtime_operation_failed(format!("{label} commit failed: {error}")))
    }

    fn hard_link_file(
        &self,
        source_name: &OsStr,
        destination_name: &OsStr,
        label: &str,
    ) -> Result<(), CommandErrorPayload> {
        rustix::fs::linkat(
            &self.directory,
            Path::new(source_name),
            &self.directory,
            Path::new(destination_name),
            rustix::fs::AtFlags::empty(),
        )
        .map_err(|error| {
            runtime_operation_failed(format!("{label} segment archive failed: {error}"))
        })
    }

    fn unlink_file(&self, file_name: &OsStr) -> Result<(), rustix::io::Errno> {
        rustix::fs::unlinkat(
            &self.directory,
            Path::new(file_name),
            rustix::fs::AtFlags::empty(),
        )
    }

    fn unlink_file_if_exists(&self, file_name: &OsStr) {
        match self.unlink_file(file_name) {
            Ok(()) | Err(rustix::io::Errno::NOENT) => {}
            Err(_) => {}
        }
    }
}

#[cfg(unix)]
fn open_parent_dir_no_symlink_for_write(
    path: &Path,
    label: &str,
) -> Result<NoFollowParentDir, CommandErrorPayload> {
    let mut components = Vec::new();
    let mut absolute = false;
    for component in path.components() {
        match component {
            Component::Prefix(_) => {
                return Err(runtime_operation_failed(format!(
                    "{label} has unsupported path prefix"
                )));
            }
            Component::RootDir => absolute = true,
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(runtime_operation_failed(format!(
                    "{label} must not use parent directory components"
                )));
            }
            Component::Normal(value) => components.push(value.to_os_string()),
        }
    }
    let file_name = components
        .pop()
        .ok_or_else(|| runtime_operation_failed(format!("{label} path has no file name")))?;
    let mut directory = if absolute {
        File::open(Path::new("/")).map_err(|error| {
            runtime_operation_failed(format!("{label} root open failed: {error}"))
        })?
    } else {
        File::open(Path::new(".")).map_err(|error| {
            runtime_operation_failed(format!("{label} current directory open failed: {error}"))
        })?
    };

    for component in components {
        match rustix::fs::mkdirat(
            &directory,
            Path::new(&component),
            rustix::fs::Mode::from_raw_mode(0o700),
        ) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "{label} directory unavailable: {error}"
                )));
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
                runtime_operation_failed(format!("{label} must not use symlinks"))
            } else {
                runtime_operation_failed(format!("{label} directory unavailable: {error}"))
            }
        })?;
        directory = File::from(fd);
    }

    Ok(NoFollowParentDir {
        directory,
        file_name,
    })
}

pub(crate) fn write_mcp_server_records(
    settings_path: &Path,
    records: &[McpServerConfigRecord],
) -> Result<(), CommandErrorPayload> {
    write_secret_json_file_atomic(settings_path, "mcp server settings", records)
}

pub(crate) fn write_automation_specs(
    automations_path: &Path,
    records: &[AutomationSpec],
) -> Result<(), CommandErrorPayload> {
    write_secret_json_file_atomic(automations_path, "automation settings", records)
}

#[cfg(test)]
pub(crate) fn append_jsonl_record_locked<T: Serialize + ?Sized>(
    target_path: &Path,
    label: &str,
    record: &T,
) -> Result<(), CommandErrorPayload> {
    let bytes = encode_jsonl_record(label, record)?;
    with_jsonl_file_lock(target_path, label, || {
        append_jsonl_bytes_unlocked(target_path, label, &bytes)
    })
}

pub(crate) fn append_jsonl_record_with_retention_locked<T, ParseError, Validate>(
    target_path: &Path,
    label: &str,
    record: &T,
    retention_limit: usize,
    parse_error: ParseError,
    validate: Validate,
) -> Result<(), CommandErrorPayload>
where
    T: Clone + DeserializeOwned + Serialize,
    ParseError: Fn(serde_json::Error) -> CommandErrorPayload,
    Validate: Fn(&T) -> Result<(), CommandErrorPayload>,
{
    let bytes = encode_jsonl_record(label, record)?;
    with_jsonl_file_lock(target_path, label, || {
        let mut records = read_jsonl_records_unlocked(target_path, label, &parse_error, &validate)?;
        records.push(record.clone());
        if retention_limit == 0 {
            records.clear();
            return rotate_jsonl_segment_unlocked(target_path, label, &records);
        }
        let keep_from = records.len().saturating_sub(retention_limit);
        if keep_from == 0 {
            append_jsonl_bytes_unlocked(target_path, label, &bytes)
        } else {
            records.drain(0..keep_from);
            rotate_jsonl_segment_unlocked(target_path, label, &records)
        }
    })
}

pub(crate) fn update_jsonl_records_locked<T, Update, ParseError, Validate>(
    target_path: &Path,
    label: &str,
    update: Update,
    parse_error: ParseError,
    validate: Validate,
) -> Result<(), CommandErrorPayload>
where
    T: DeserializeOwned + Serialize,
    Update: FnOnce(&mut Vec<T>),
    ParseError: Fn(serde_json::Error) -> CommandErrorPayload,
    Validate: Fn(&T) -> Result<(), CommandErrorPayload>,
{
    with_jsonl_file_lock(target_path, label, || {
        let mut records = read_jsonl_records_unlocked(target_path, label, &parse_error, &validate)?;
        update(&mut records);
        rotate_jsonl_segment_unlocked(target_path, label, &records)
    })
}

pub(crate) fn read_jsonl_records_locked<T, ParseError, Validate>(
    target_path: &Path,
    label: &str,
    parse_error: ParseError,
    validate: Validate,
) -> Result<Vec<T>, CommandErrorPayload>
where
    T: DeserializeOwned,
    ParseError: Fn(serde_json::Error) -> CommandErrorPayload,
    Validate: Fn(&T) -> Result<(), CommandErrorPayload>,
{
    with_jsonl_file_lock(target_path, label, || {
        read_jsonl_records_unlocked(target_path, label, &parse_error, &validate)
    })
}

pub(crate) fn write_atomic_runtime_file(
    target_path: &Path,
    fallback_name: &str,
    label: &str,
    bytes: &[u8],
) -> Result<(), CommandErrorPayload> {
    write_bytes_file_atomic(target_path, fallback_name, label, bytes, true)
}

fn write_bytes_file_atomic(
    target_path: &Path,
    fallback_name: &str,
    label: &str,
    bytes: &[u8],
    owner_only: bool,
) -> Result<(), CommandErrorPayload> {
    #[cfg(unix)]
    {
        return write_bytes_file_atomic_unix(target_path, fallback_name, label, bytes, owner_only);
    }

    #[cfg(not(unix))]
    {
        let parent = target_path
            .parent()
            .ok_or_else(|| runtime_operation_failed(format!("{label} path has no parent")))?;
        ensure_app_dir_no_symlink(parent, &format!("{label} directory"))?;
        let temp_path = target_path.with_file_name(format!(
            "{}.{}.tmp",
            target_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(fallback_name),
            RunId::new()
        ));
        ensure_no_symlink_components(&temp_path, &format!("{label} temp file"))?;
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
            runtime_operation_failed(format!("{label} temp open failed: {error}"))
        })?;
        if let Err(error) = temp_file.write_all(bytes) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(runtime_operation_failed(format!(
                "{label} write failed: {error}"
            )));
        }
        if let Err(error) = temp_file.sync_all() {
            let _ = std::fs::remove_file(&temp_path);
            return Err(runtime_operation_failed(format!(
                "{label} sync failed: {error}"
            )));
        }
        drop(temp_file);
        ensure_no_symlink_components(target_path, &format!("{label} file"))?;
        std::fs::rename(&temp_path, target_path).map_err(|error| {
            let _ = std::fs::remove_file(&temp_path);
            runtime_operation_failed(format!("{label} commit failed: {error}"))
        })?;
        #[cfg(unix)]
        if owner_only {
            set_owner_only_if_unix(target_path, label)?;
        }
        sync_directory(parent, label)
    }
}

#[cfg(unix)]
fn write_bytes_file_atomic_unix(
    target_path: &Path,
    fallback_name: &str,
    label: &str,
    bytes: &[u8],
    owner_only: bool,
) -> Result<(), CommandErrorPayload> {
    let parent = open_parent_dir_no_symlink_for_write(target_path, &format!("{label} directory"))?;
    let temp_name = std::ffi::OsString::from(format!(
        "{}.{}.tmp",
        target_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(fallback_name),
        RunId::new()
    ));
    let mut temp_file = parent.create_new_file(&temp_name, owner_only, label)?;
    if let Err(error) = temp_file.write_all(bytes) {
        let _ = parent.unlink_file(&temp_name);
        return Err(runtime_operation_failed(format!(
            "{label} write failed: {error}"
        )));
    }
    if let Err(error) = temp_file.sync_all() {
        let _ = parent.unlink_file(&temp_name);
        return Err(runtime_operation_failed(format!(
            "{label} sync failed: {error}"
        )));
    }
    if owner_only {
        set_owner_only_file_if_unix(&temp_file, label)?;
    }
    drop(temp_file);
    if let Err(error) = parent.ensure_existing_file_is_regular_or_missing(parent.file_name(), label)
    {
        let _ = parent.unlink_file(&temp_name);
        return Err(error);
    }
    parent.rename_file(&temp_name, parent.file_name(), label)?;
    if owner_only {
        let file = parent.open_existing_file(parent.file_name(), label)?;
        set_owner_only_file_if_unix(&file, label)?;
    }
    parent.sync_all(label)
}

fn encode_jsonl_record<T: Serialize + ?Sized>(
    label: &str,
    record: &T,
) -> Result<Vec<u8>, CommandErrorPayload> {
    let mut bytes = Vec::new();
    serde_json::to_writer(&mut bytes, record).map_err(|error| {
        runtime_operation_failed(format!("{label} serialization failed: {error}"))
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn read_jsonl_records_unlocked<T, ParseError, Validate>(
    target_path: &Path,
    label: &str,
    parse_error: &ParseError,
    validate: &Validate,
) -> Result<Vec<T>, CommandErrorPayload>
where
    T: DeserializeOwned,
    ParseError: Fn(serde_json::Error) -> CommandErrorPayload,
    Validate: Fn(&T) -> Result<(), CommandErrorPayload>,
{
    ensure_no_symlink_components(target_path, &format!("{label} file"))?;
    let mut open_options = OpenOptions::new();
    open_options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        open_options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = match open_options.open(target_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(runtime_operation_failed(format!(
                "{label} read failed: {error}"
            )));
        }
    };
    let mut records = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line
            .map_err(|error| runtime_operation_failed(format!("{label} read failed: {error}")))?;
        if line.trim().is_empty() {
            continue;
        }
        let record = serde_json::from_str::<T>(&line).map_err(parse_error)?;
        validate(&record)?;
        records.push(record);
    }
    Ok(records)
}

fn append_jsonl_bytes_unlocked(
    target_path: &Path,
    label: &str,
    bytes: &[u8],
) -> Result<(), CommandErrorPayload> {
    #[cfg(unix)]
    {
        let parent =
            open_parent_dir_no_symlink_for_write(target_path, &format!("{label} directory"))?;
        let mut file = parent.open_or_create_append_file(parent.file_name(), label)?;
        set_owner_only_file_if_unix(&file, label)?;
        file.write_all(bytes)
            .map_err(|error| runtime_operation_failed(format!("{label} write failed: {error}")))?;
        file.sync_all()
            .map_err(|error| runtime_operation_failed(format!("{label} sync failed: {error}")))?;
        return parent.sync_all(label);
    }

    #[cfg(not(unix))]
    {
        let parent = target_path
            .parent()
            .ok_or_else(|| runtime_operation_failed(format!("{label} path has no parent")))?;
        ensure_app_dir_no_symlink(parent, &format!("{label} directory"))?;
        ensure_no_symlink_components(target_path, &format!("{label} file"))?;
        set_owner_only_if_exists_unix(target_path, label)?;
        let mut open_options = OpenOptions::new();
        open_options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;

            open_options.custom_flags(libc::O_NOFOLLOW);
            open_options.mode(0o600);
        }
        let mut file = open_options
            .open(target_path)
            .map_err(|error| runtime_operation_failed(format!("{label} open failed: {error}")))?;
        set_owner_only_file_if_unix(&file, label)?;
        file.write_all(bytes)
            .map_err(|error| runtime_operation_failed(format!("{label} write failed: {error}")))?;
        file.sync_all()
            .map_err(|error| runtime_operation_failed(format!("{label} sync failed: {error}")))?;
        sync_directory(parent, label)
    }
}

fn rotate_jsonl_segment_unlocked<T: Serialize>(
    target_path: &Path,
    label: &str,
    records: &[T],
) -> Result<(), CommandErrorPayload> {
    #[cfg(unix)]
    {
        return rotate_jsonl_segment_unlocked_unix(target_path, label, records);
    }

    #[cfg(not(unix))]
    {
        let parent = target_path
            .parent()
            .ok_or_else(|| runtime_operation_failed(format!("{label} path has no parent")))?;
        ensure_app_dir_no_symlink(parent, &format!("{label} directory"))?;
        let mut bytes = Vec::new();
        for record in records {
            bytes.extend(encode_jsonl_record(label, record)?);
        }
        let file_name = target_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("store.jsonl");
        let suffix = format!(
            "{}.{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let temp_path = target_path.with_file_name(format!("{file_name}.{suffix}.tmp"));
        let segment_path = target_path.with_file_name(format!("{file_name}.{suffix}.segment"));
        ensure_no_symlink_components(&temp_path, &format!("{label} temp file"))?;
        ensure_no_symlink_components(&segment_path, &format!("{label} segment file"))?;

        let mut open_options = OpenOptions::new();
        open_options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;

            open_options.custom_flags(libc::O_NOFOLLOW);
            open_options.mode(0o600);
        }
        let mut temp_file = open_options.open(&temp_path).map_err(|error| {
            runtime_operation_failed(format!("{label} segment open failed: {error}"))
        })?;
        if let Err(error) = temp_file.write_all(&bytes) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(runtime_operation_failed(format!(
                "{label} segment write failed: {error}"
            )));
        }
        if let Err(error) = temp_file.sync_all() {
            let _ = std::fs::remove_file(&temp_path);
            return Err(runtime_operation_failed(format!(
                "{label} segment sync failed: {error}"
            )));
        }
        drop(temp_file);
        set_owner_only_if_unix(&temp_path, label)?;

        ensure_no_symlink_components(target_path, &format!("{label} file"))?;
        let archived_path = match std::fs::symlink_metadata(target_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let _ = std::fs::remove_file(&temp_path);
                return Err(runtime_operation_failed(format!(
                    "{label} must not use symlinks"
                )));
            }
            Ok(metadata) if metadata.is_file() => {
                std::fs::hard_link(target_path, &segment_path).map_err(|error| {
                    let _ = std::fs::remove_file(&temp_path);
                    runtime_operation_failed(format!("{label} segment archive failed: {error}"))
                })?;
                set_owner_only_if_unix(&segment_path, label)?;
                sync_directory(parent, label)?;
                Some(segment_path)
            }
            Ok(_) => {
                let _ = std::fs::remove_file(&temp_path);
                return Err(runtime_operation_failed(format!(
                    "{label} path is not a file"
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                let _ = std::fs::remove_file(&temp_path);
                return Err(runtime_operation_failed(format!(
                    "{label} metadata unavailable: {error}"
                )));
            }
        };

        if let Err(error) = std::fs::rename(&temp_path, target_path) {
            let _ = std::fs::remove_file(&temp_path);
            if let Some(archived_path) = archived_path {
                let _ = std::fs::remove_file(&archived_path);
            }
            return Err(runtime_operation_failed(format!(
                "{label} segment install failed: {error}"
            )));
        }
        set_owner_only_if_unix(target_path, label)?;
        sync_directory(parent, label)
    }
}

#[cfg(unix)]
fn rotate_jsonl_segment_unlocked_unix<T: Serialize>(
    target_path: &Path,
    label: &str,
    records: &[T],
) -> Result<(), CommandErrorPayload> {
    let parent = open_parent_dir_no_symlink_for_write(target_path, &format!("{label} directory"))?;
    let mut bytes = Vec::new();
    for record in records {
        bytes.extend(encode_jsonl_record(label, record)?);
    }
    let file_name = target_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("store.jsonl");
    let suffix = format!(
        "{}.{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let temp_name = std::ffi::OsString::from(format!("{file_name}.{suffix}.tmp"));
    let segment_name = std::ffi::OsString::from(format!("{file_name}.{suffix}.segment"));

    let mut temp_file = parent.create_new_file(&temp_name, true, label)?;
    if let Err(error) = temp_file.write_all(&bytes) {
        parent.unlink_file_if_exists(&temp_name);
        return Err(runtime_operation_failed(format!(
            "{label} segment write failed: {error}"
        )));
    }
    if let Err(error) = temp_file.sync_all() {
        parent.unlink_file_if_exists(&temp_name);
        return Err(runtime_operation_failed(format!(
            "{label} segment sync failed: {error}"
        )));
    }
    set_owner_only_file_if_unix(&temp_file, label)?;
    drop(temp_file);

    let archived = match parent.try_open_existing_file(parent.file_name(), label)? {
        Some(existing) => {
            let metadata = existing.metadata().map_err(|error| {
                parent.unlink_file_if_exists(&temp_name);
                runtime_operation_failed(format!("{label} metadata unavailable: {error}"))
            })?;
            if !metadata.is_file() {
                parent.unlink_file_if_exists(&temp_name);
                return Err(runtime_operation_failed(format!(
                    "{label} path is not a file"
                )));
            }
            drop(existing);
            parent.hard_link_file(parent.file_name(), &segment_name, label)?;
            let segment = parent.open_existing_file(&segment_name, label)?;
            set_owner_only_file_if_unix(&segment, label)?;
            parent.sync_all(label)?;
            true
        }
        None => false,
    };

    if let Err(error) = parent.rename_file(&temp_name, parent.file_name(), label) {
        parent.unlink_file_if_exists(&temp_name);
        if archived {
            parent.unlink_file_if_exists(&segment_name);
        }
        return Err(error);
    }
    let file = parent.open_existing_file(parent.file_name(), label)?;
    set_owner_only_file_if_unix(&file, label)?;
    parent.sync_all(label)
}

fn with_jsonl_file_lock<T>(
    target_path: &Path,
    label: &str,
    action: impl FnOnce() -> Result<T, CommandErrorPayload>,
) -> Result<T, CommandErrorPayload> {
    #[cfg(unix)]
    {
        let lock_path = jsonl_lock_path(target_path);
        let parent =
            open_parent_dir_no_symlink_for_write(&lock_path, &format!("{label} directory"))?;
        let lock_file = parent.open_or_create_read_write_file(parent.file_name(), label)?;
        set_owner_only_file_if_unix(&lock_file, &format!("{label} lock"))?;
        parent.sync_all(label)?;
        lock_file
            .lock_exclusive()
            .map_err(|error| runtime_operation_failed(format!("{label} lock failed: {error}")))?;
        let result = action();
        let unlock_result = lock_file
            .unlock()
            .map_err(|error| runtime_operation_failed(format!("{label} unlock failed: {error}")));
        return match (result, unlock_result) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(value), Ok(())) => Ok(value),
        };
    }

    #[cfg(not(unix))]
    {
        let parent = target_path
            .parent()
            .ok_or_else(|| runtime_operation_failed(format!("{label} path has no parent")))?;
        ensure_app_dir_no_symlink(parent, &format!("{label} directory"))?;
        let lock_path = jsonl_lock_path(target_path);
        ensure_no_symlink_components(&lock_path, &format!("{label} lock file"))?;
        let mut open_options = OpenOptions::new();
        open_options.create(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;

            open_options.custom_flags(libc::O_NOFOLLOW);
            open_options.mode(0o600);
        }
        let lock_file = open_options.open(&lock_path).map_err(|error| {
            runtime_operation_failed(format!("{label} lock open failed: {error}"))
        })?;
        set_owner_only_file_if_unix(&lock_file, &format!("{label} lock"))?;
        lock_file
            .lock_exclusive()
            .map_err(|error| runtime_operation_failed(format!("{label} lock failed: {error}")))?;
        let result = action();
        let unlock_result = lock_file
            .unlock()
            .map_err(|error| runtime_operation_failed(format!("{label} unlock failed: {error}")));
        match (result, unlock_result) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(value), Ok(())) => Ok(value),
        }
    }
}

fn jsonl_lock_path(target_path: &Path) -> PathBuf {
    let file_name = target_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("store.jsonl");
    target_path.with_file_name(format!("{file_name}.lock"))
}

#[cfg(unix)]
fn set_owner_only_if_exists_unix(path: &Path, label: &str) -> Result<(), CommandErrorPayload> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(runtime_operation_failed(
            format!("{label} must not use symlinks"),
        )),
        Ok(metadata) if metadata.is_file() => {
            let mut options = OpenOptions::new();
            options.read(true);
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
            let file = options.open(path).map_err(|error| {
                runtime_operation_failed(format!("{label} open failed: {error}"))
            })?;
            set_owner_only_file_if_unix(&file, label)
        }
        Ok(_) => Err(runtime_operation_failed(format!(
            "{label} path is not a file"
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(runtime_operation_failed(format!(
            "{label} metadata unavailable: {error}"
        ))),
    }
}

#[cfg(not(unix))]
fn set_owner_only_if_exists_unix(_path: &Path, _label: &str) -> Result<(), CommandErrorPayload> {
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_file_if_unix(file: &File, label: &str) -> Result<(), CommandErrorPayload> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|error| {
            runtime_operation_failed(format!("{label} permission update failed: {error}"))
        })
}

#[cfg(not(unix))]
fn set_owner_only_if_unix(_path: &Path, _label: &str) -> Result<(), CommandErrorPayload> {
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_file_if_unix(_file: &File, _label: &str) -> Result<(), CommandErrorPayload> {
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path, _label: &str) -> Result<(), CommandErrorPayload> {
    Ok(())
}

pub(crate) fn write_skill_records(
    index_path: &Path,
    records: &[SkillStoreRecord],
) -> Result<(), CommandErrorPayload> {
    write_json_file_atomic(index_path, "skill index", records)
}

pub(crate) fn write_plugin_settings_record(
    index_path: &Path,
    record: &PluginSettingsRecord,
) -> Result<(), CommandErrorPayload> {
    write_json_file_atomic(index_path, "plugin index", record)
}

pub(crate) fn ensure_skill_id(id: &str) -> Result<(), CommandErrorPayload> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(invalid_payload("skill id is invalid".to_owned()));
    }
    Ok(())
}

pub(crate) fn ensure_plugin_package_dir_name(value: &str) -> Result<(), CommandErrorPayload> {
    if value.is_empty()
        || value.len() > 128
        || value.starts_with('.')
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(invalid_payload(
            "plugin package directory is invalid".to_owned(),
        ));
    }
    let mut components = Path::new(value).components();
    if !matches!(components.next(), Some(Component::Normal(component)) if component == OsStr::new(value))
        || components.next().is_some()
    {
        return Err(invalid_payload(
            "plugin package directory is invalid".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_plugin_store_record(
    record: &PluginStoreRecord,
) -> Result<(), CommandErrorPayload> {
    ensure_plugin_package_dir_name(&record.package_dir)?;
    let expected_package_dir = plugin_package_dir_name(&record.plugin_id);
    if record.package_dir != expected_package_dir {
        return Err(invalid_payload(
            "plugin package directory does not match plugin id".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_plugin_settings_record(
    record: &PluginSettingsRecord,
) -> Result<(), CommandErrorPayload> {
    for plugin in &record.records {
        ensure_plugin_store_record(plugin)?;
    }
    Ok(())
}

pub(crate) fn ensure_no_symlink_components(
    path: &Path,
    label: &str,
) -> Result<(), CommandErrorPayload> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                current.push(component.as_os_str());
                continue;
            }
            Component::CurDir => continue,
            Component::ParentDir => {
                return Err(runtime_operation_failed(format!(
                    "{label} must not use parent directory components"
                )));
            }
            Component::Normal(value) => current.push(value),
        }
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(runtime_operation_failed(format!(
                    "{label} must not use symlinks"
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "{label} metadata unavailable: {error}"
                )));
            }
        }
    }

    Ok(())
}

pub(crate) fn hash_skill_package(source_path: &Path) -> Result<String, CommandErrorPayload> {
    let mut hasher = blake3::Hasher::new();
    let mut file_count = 0_usize;
    let mut total_bytes = 0_u64;
    hash_skill_package_dir(
        source_path,
        source_path,
        &mut hasher,
        &mut file_count,
        &mut total_bytes,
    )?;
    Ok(hasher.finalize().to_hex().to_string())
}

pub(crate) fn hash_skill_package_dir(
    root: &Path,
    dir: &Path,
    hasher: &mut blake3::Hasher,
    file_count: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), CommandErrorPayload> {
    ensure_no_symlink_components(dir, "skill package directory")?;
    let mut entries = std::fs::read_dir(dir)
        .map_err(|error| runtime_operation_failed(format!("skill package read failed: {error}")))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| runtime_operation_failed(format!("skill package read failed: {error}")))?;
    entries.sort();
    for path in entries {
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            runtime_operation_failed(format!("skill package metadata failed: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(runtime_operation_failed(
                "skill package must not use symlinks".to_owned(),
            ));
        }
        if metadata.is_dir() {
            hash_skill_package_dir(root, &path, hasher, file_count, total_bytes)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(invalid_payload(
                "skill package may contain only files and directories".to_owned(),
            ));
        }
        *file_count += 1;
        if *file_count > MAX_SKILL_PACKAGE_FILES {
            return Err(invalid_payload(
                "skill package has too many files".to_owned(),
            ));
        }
        let bytes =
            read_regular_file_no_follow(&path, "skill package file", MAX_SKILL_PACKAGE_FILE_BYTES)?;
        *total_bytes = total_bytes.saturating_add(bytes.len() as u64);
        if *total_bytes > MAX_SKILL_PACKAGE_BYTES {
            return Err(invalid_payload("skill package is too large".to_owned()));
        }
        let relative_path = path.strip_prefix(root).map_err(|_| {
            runtime_operation_failed("skill package path escaped its root".to_owned())
        })?;
        hasher.update(path_to_workspace_string(relative_path).as_bytes());
        hasher.update(&[0]);
        hasher.update(&bytes);
        hasher.update(&[0]);
    }
    Ok(())
}

fn remove_package_dir_if_exists(path: &Path, label: &str) -> Result<(), CommandErrorPayload> {
    #[cfg(unix)]
    {
        let Some(parent) = open_parent_dir_no_symlink_for_read(path, label)? else {
            return Ok(());
        };
        match rustix::fs::openat(
            &parent.directory,
            Path::new(parent.file_name()),
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_raw_mode(0),
        ) {
            Ok(fd) => {
                drop(File::from(fd));
            }
            Err(rustix::io::Errno::NOENT) => return Ok(()),
            Err(rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR) => {
                return Err(runtime_operation_failed(format!(
                    "{label} cleanup target must be a non-symlink directory"
                )));
            }
            Err(error) => {
                return Err(runtime_operation_failed(format!(
                    "{label} cleanup failed: {error}"
                )));
            }
        }

        let mut tombstone_name = parent.file_name().to_os_string();
        tombstone_name.push(format!(
            ".delete-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        parent.rename_file(parent.file_name(), &tombstone_name, label)?;
        parent.sync_all(label)?;
        let tombstone_path = path.with_file_name(tombstone_name);
        std::fs::remove_dir_all(&tombstone_path).map_err(|error| {
            runtime_operation_failed(format!("{label} cleanup failed: {error}"))
        })?;
        parent.sync_all(label)?;
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        ensure_no_symlink_components(path, label)?;
        match std::fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(runtime_operation_failed(format!(
                "{label} cleanup failed: {error}"
            ))),
        }
    }
}

pub(crate) fn copy_skill_package(
    source_path: &Path,
    destination_path: &Path,
) -> Result<String, CommandErrorPayload> {
    ensure_no_symlink_components(source_path, "skill source package")?;
    ensure_no_symlink_components(destination_path, "skill package")?;
    remove_package_dir_if_exists(destination_path, "skill package")?;
    ensure_app_dir_no_symlink(destination_path, "skill package directory")?;
    let mut file_count = 0_usize;
    let mut total_bytes = 0_u64;
    match copy_skill_package_dir(
        source_path,
        source_path,
        destination_path,
        &mut file_count,
        &mut total_bytes,
    ) {
        Ok(()) => hash_skill_package(destination_path),
        Err(error) => {
            let _ = remove_package_dir_if_exists(destination_path, "skill package");
            Err(error)
        }
    }
}

pub(crate) fn copy_skill_package_dir(
    root: &Path,
    dir: &Path,
    destination_root: &Path,
    file_count: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), CommandErrorPayload> {
    ensure_no_symlink_components(dir, "skill source package")?;
    let mut entries = std::fs::read_dir(dir)
        .map_err(|error| runtime_operation_failed(format!("skill package read failed: {error}")))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| runtime_operation_failed(format!("skill package read failed: {error}")))?;
    entries.sort();
    for path in entries {
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            runtime_operation_failed(format!("skill package metadata failed: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(runtime_operation_failed(
                "skill package must not use symlinks".to_owned(),
            ));
        }
        let relative_path = path.strip_prefix(root).map_err(|_| {
            runtime_operation_failed("skill package path escaped its root".to_owned())
        })?;
        let destination_path = destination_root.join(relative_path);
        ensure_no_symlink_components(&destination_path, "skill package")?;
        if metadata.is_dir() {
            ensure_app_dir_no_symlink(&destination_path, "skill package directory")?;
            copy_skill_package_dir(root, &path, destination_root, file_count, total_bytes)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(invalid_payload(
                "skill package may contain only files and directories".to_owned(),
            ));
        }
        let parent = destination_path.parent().ok_or_else(|| {
            runtime_operation_failed("skill package file path has no parent".to_owned())
        })?;
        ensure_app_dir_no_symlink(parent, "skill package directory")?;
        copy_regular_file_no_follow(&path, &destination_path, file_count, total_bytes)?;
    }
    Ok(())
}

pub(crate) fn read_regular_file_no_follow(
    path: &Path,
    label: &str,
    max_file_bytes: u64,
) -> Result<Vec<u8>, CommandErrorPayload> {
    #[cfg(unix)]
    {
        let Some(parent) = open_parent_dir_no_symlink_for_read(path, label)? else {
            return Err(runtime_operation_failed(format!(
                "{label} open failed: not found"
            )));
        };
        let mut file = parent.open_existing_file(parent.file_name(), label)?;
        let metadata = file.metadata().map_err(|error| {
            runtime_operation_failed(format!("{label} metadata failed: {error}"))
        })?;
        if !metadata.is_file() {
            return Err(invalid_payload(format!("{label} must be a file")));
        }
        if metadata.len() > max_file_bytes {
            return Err(invalid_payload(format!("{label} is too large")));
        }
        let mut bytes = Vec::with_capacity(metadata.len().try_into().unwrap_or(0));
        file.read_to_end(&mut bytes)
            .map_err(|error| runtime_operation_failed(format!("{label} read failed: {error}")))?;
        if bytes.len() as u64 > max_file_bytes {
            return Err(invalid_payload(format!("{label} is too large")));
        }
        return Ok(bytes);
    }

    #[cfg(not(unix))]
    {
        ensure_no_symlink_components(path, label)?;
        let mut open_options = OpenOptions::new();
        open_options.read(true);
        let mut file = open_options
            .open(path)
            .map_err(|error| runtime_operation_failed(format!("{label} open failed: {error}")))?;
        let metadata = file.metadata().map_err(|error| {
            runtime_operation_failed(format!("{label} metadata failed: {error}"))
        })?;
        if !metadata.is_file() {
            return Err(invalid_payload(format!("{label} must be a file")));
        }
        if metadata.len() > max_file_bytes {
            return Err(invalid_payload(format!("{label} is too large")));
        }
        let mut bytes = Vec::with_capacity(metadata.len().try_into().unwrap_or(0));
        file.read_to_end(&mut bytes)
            .map_err(|error| runtime_operation_failed(format!("{label} read failed: {error}")))?;
        if bytes.len() as u64 > max_file_bytes {
            return Err(invalid_payload(format!("{label} is too large")));
        }
        Ok(bytes)
    }
}

fn copy_regular_file_no_follow(
    source_path: &Path,
    destination_path: &Path,
    file_count: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), CommandErrorPayload> {
    let bytes = read_regular_file_no_follow(
        source_path,
        "skill package file",
        MAX_SKILL_PACKAGE_FILE_BYTES,
    )?;
    *file_count += 1;
    if *file_count > MAX_SKILL_PACKAGE_FILES {
        return Err(invalid_payload(
            "skill package has too many files".to_owned(),
        ));
    }
    *total_bytes = total_bytes.saturating_add(bytes.len() as u64);
    if *total_bytes > MAX_SKILL_PACKAGE_BYTES {
        return Err(invalid_payload("skill package is too large".to_owned()));
    }

    #[cfg(unix)]
    {
        let parent =
            open_parent_dir_no_symlink_for_write(destination_path, "skill package directory")?;
        let mut destination =
            parent.create_new_file(parent.file_name(), false, "skill package file")?;
        destination.write_all(&bytes).map_err(|error| {
            let _ = parent.unlink_file(parent.file_name());
            runtime_operation_failed(format!("skill package file write failed: {error}"))
        })?;
        destination.sync_all().map_err(|error| {
            let _ = parent.unlink_file(parent.file_name());
            runtime_operation_failed(format!("skill package file sync failed: {error}"))
        })?;
        parent.sync_all("skill package file")?;
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        let mut open_options = OpenOptions::new();
        open_options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;

            open_options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut destination = open_options.open(destination_path).map_err(|error| {
            runtime_operation_failed(format!("skill package file create failed: {error}"))
        })?;
        destination.write_all(&bytes).map_err(|error| {
            runtime_operation_failed(format!("skill package file write failed: {error}"))
        })?;
        destination.sync_all().map_err(|error| {
            runtime_operation_failed(format!("skill package file sync failed: {error}"))
        })
    }
}

pub(crate) fn hash_plugin_package(source_path: &Path) -> Result<String, CommandErrorPayload> {
    let mut hasher = blake3::Hasher::new();
    let mut file_count = 0_usize;
    let mut total_bytes = 0_u64;
    hash_plugin_package_dir(
        source_path,
        source_path,
        &mut hasher,
        &mut file_count,
        &mut total_bytes,
    )?;
    Ok(hasher.finalize().to_hex().to_string())
}

pub(crate) fn hash_plugin_package_dir(
    root: &Path,
    dir: &Path,
    hasher: &mut blake3::Hasher,
    file_count: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), CommandErrorPayload> {
    ensure_no_symlink_components(dir, "plugin package directory")?;
    let mut entries = std::fs::read_dir(dir)
        .map_err(|error| runtime_operation_failed(format!("plugin package read failed: {error}")))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            runtime_operation_failed(format!("plugin package read failed: {error}"))
        })?;
    entries.sort();
    for path in entries {
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            runtime_operation_failed(format!("plugin package metadata failed: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(runtime_operation_failed(
                "plugin package must not use symlinks".to_owned(),
            ));
        }
        if metadata.is_dir() {
            hash_plugin_package_dir(root, &path, hasher, file_count, total_bytes)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(invalid_payload(
                "plugin package may contain only files and directories".to_owned(),
            ));
        }
        *file_count += 1;
        if *file_count > MAX_PLUGIN_PACKAGE_FILES {
            return Err(invalid_payload(
                "plugin package has too many files".to_owned(),
            ));
        }
        if metadata.len() > MAX_PLUGIN_PACKAGE_FILE_BYTES {
            return Err(invalid_payload(
                "plugin package file is too large".to_owned(),
            ));
        }
        *total_bytes = total_bytes.saturating_add(metadata.len());
        if *total_bytes > MAX_PLUGIN_PACKAGE_BYTES {
            return Err(invalid_payload("plugin package is too large".to_owned()));
        }
        let relative_path = path.strip_prefix(root).map_err(|_| {
            runtime_operation_failed("plugin package path escaped its root".to_owned())
        })?;
        let bytes = read_regular_file_no_follow(
            &path,
            "plugin package file",
            MAX_PLUGIN_PACKAGE_FILE_BYTES,
        )?;
        hasher.update(path_to_workspace_string(relative_path).as_bytes());
        hasher.update(&[0]);
        hasher.update(&bytes);
        hasher.update(&[0]);
    }
    Ok(())
}

pub(crate) fn copy_plugin_package(
    source_path: &Path,
    destination_path: &Path,
) -> Result<String, CommandErrorPayload> {
    ensure_no_symlink_components(source_path, "plugin source package")?;
    ensure_no_symlink_components(destination_path, "plugin package")?;
    let _ = hash_plugin_package(source_path)?;
    remove_package_dir_if_exists(destination_path, "plugin package")?;
    ensure_app_dir_no_symlink(destination_path, "plugin package directory")?;
    let mut file_count = 0_usize;
    let mut total_bytes = 0_u64;
    match copy_plugin_package_dir(
        source_path,
        source_path,
        destination_path,
        &mut file_count,
        &mut total_bytes,
    ) {
        Ok(()) => hash_plugin_package(destination_path),
        Err(error) => {
            let _ = remove_package_dir_if_exists(destination_path, "plugin package");
            Err(error)
        }
    }
}

pub(crate) fn copy_plugin_package_dir(
    root: &Path,
    dir: &Path,
    destination_root: &Path,
    file_count: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), CommandErrorPayload> {
    ensure_no_symlink_components(dir, "plugin source package")?;
    let mut entries = std::fs::read_dir(dir)
        .map_err(|error| runtime_operation_failed(format!("plugin package read failed: {error}")))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            runtime_operation_failed(format!("plugin package read failed: {error}"))
        })?;
    entries.sort();
    for path in entries {
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            runtime_operation_failed(format!("plugin package metadata failed: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(runtime_operation_failed(
                "plugin package must not use symlinks".to_owned(),
            ));
        }
        let relative_path = path.strip_prefix(root).map_err(|_| {
            runtime_operation_failed("plugin package path escaped its root".to_owned())
        })?;
        let destination_path = destination_root.join(relative_path);
        ensure_no_symlink_components(&destination_path, "plugin package")?;
        if metadata.is_dir() {
            ensure_app_dir_no_symlink(&destination_path, "plugin package directory")?;
            copy_plugin_package_dir(root, &path, destination_root, file_count, total_bytes)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(invalid_payload(
                "plugin package may contain only files and directories".to_owned(),
            ));
        }
        let parent = destination_path.parent().ok_or_else(|| {
            runtime_operation_failed("plugin package file path has no parent".to_owned())
        })?;
        ensure_app_dir_no_symlink(parent, "plugin package directory")?;
        copy_plugin_regular_file_no_follow(&path, &destination_path, file_count, total_bytes)?;
    }
    Ok(())
}

fn copy_plugin_regular_file_no_follow(
    source_path: &Path,
    destination_path: &Path,
    file_count: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), CommandErrorPayload> {
    #[cfg(unix)]
    let source_mode = {
        use std::os::unix::fs::PermissionsExt;

        std::fs::symlink_metadata(source_path)
            .map_err(|error| {
                runtime_operation_failed(format!("plugin package metadata failed: {error}"))
            })?
            .permissions()
            .mode()
            & 0o777
    };
    let bytes = read_regular_file_no_follow(
        source_path,
        "plugin package file",
        MAX_PLUGIN_PACKAGE_FILE_BYTES,
    )?;
    *file_count += 1;
    if *file_count > MAX_PLUGIN_PACKAGE_FILES {
        return Err(invalid_payload(
            "plugin package has too many files".to_owned(),
        ));
    }
    *total_bytes = total_bytes.saturating_add(bytes.len() as u64);
    if *total_bytes > MAX_PLUGIN_PACKAGE_BYTES {
        return Err(invalid_payload("plugin package is too large".to_owned()));
    }

    #[cfg(unix)]
    {
        let parent =
            open_parent_dir_no_symlink_for_write(destination_path, "plugin package directory")?;
        let mut destination =
            parent.create_new_file(parent.file_name(), false, "plugin package file")?;
        destination.write_all(&bytes).map_err(|error| {
            let _ = parent.unlink_file(parent.file_name());
            runtime_operation_failed(format!("plugin package file write failed: {error}"))
        })?;
        {
            use std::os::unix::fs::PermissionsExt;

            destination
                .set_permissions(std::fs::Permissions::from_mode(source_mode))
                .map_err(|error| {
                    let _ = parent.unlink_file(parent.file_name());
                    runtime_operation_failed(format!(
                        "plugin package file permissions failed: {error}"
                    ))
                })?;
        }
        destination.sync_all().map_err(|error| {
            let _ = parent.unlink_file(parent.file_name());
            runtime_operation_failed(format!("plugin package file sync failed: {error}"))
        })?;
        parent.sync_all("plugin package file")?;
        return Ok(());
    }

    #[cfg(not(unix))]
    {
        let mut open_options = OpenOptions::new();
        open_options.write(true).create_new(true);
        let mut destination = open_options.open(destination_path).map_err(|error| {
            runtime_operation_failed(format!("plugin package file create failed: {error}"))
        })?;
        destination.write_all(&bytes).map_err(|error| {
            runtime_operation_failed(format!("plugin package file write failed: {error}"))
        })?;
        destination.sync_all().map_err(|error| {
            runtime_operation_failed(format!("plugin package file sync failed: {error}"))
        })
    }
}

pub(crate) fn list_skill_package_files(
    package_root: &Path,
) -> Result<Vec<SkillFilePayload>, CommandErrorPayload> {
    let mut files = Vec::new();
    collect_skill_package_files(package_root, package_root, &mut files)?;
    files.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.kind.cmp(right.kind))
    });
    Ok(files)
}

pub(crate) fn collect_skill_package_files(
    package_root: &Path,
    dir: &Path,
    files: &mut Vec<SkillFilePayload>,
) -> Result<(), CommandErrorPayload> {
    ensure_no_symlink_components(dir, "skill package directory")?;
    let mut entries = std::fs::read_dir(dir)
        .map_err(|error| runtime_operation_failed(format!("skill package read failed: {error}")))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| runtime_operation_failed(format!("skill package read failed: {error}")))?;
    entries.sort();
    for path in entries {
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            runtime_operation_failed(format!("skill package metadata failed: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(runtime_operation_failed(
                "skill package must not use symlinks".to_owned(),
            ));
        }
        let relative_path = path.strip_prefix(package_root).map_err(|_| {
            runtime_operation_failed("skill package path escaped its root".to_owned())
        })?;
        let normalized_path = path_to_workspace_string(relative_path);
        let name = path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| normalized_path.clone());
        let depth = relative_path.components().count().saturating_sub(1) as u32;
        if metadata.is_dir() {
            files.push(SkillFilePayload {
                path: normalized_path,
                name,
                kind: "directory",
                depth,
                size_bytes: None,
            });
            collect_skill_package_files(package_root, &path, files)?;
        } else if metadata.is_file() {
            files.push(SkillFilePayload {
                path: normalized_path,
                name,
                kind: "file",
                depth,
                size_bytes: Some(metadata.len()),
            });
        } else {
            return Err(invalid_payload(
                "skill package may contain only files and directories".to_owned(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn read_skill_package_file(
    package_root: &Path,
    relative_path: &Path,
) -> Result<SkillFileContentPayload, CommandErrorPayload> {
    let display_path = path_to_workspace_string(relative_path);
    let path = package_root.join(relative_path);
    ensure_no_symlink_components(&path, "skill package file")?;
    let normalized_root = package_root.canonicalize().map_err(|error| {
        runtime_operation_failed(format!("skill package path unavailable: {error}"))
    })?;
    let normalized_path = path.canonicalize().map_err(|error| {
        runtime_operation_failed(format!("skill package file unavailable: {error}"))
    })?;
    if !normalized_path.starts_with(normalized_root) {
        return Err(invalid_payload(
            "skill file path escaped package".to_owned(),
        ));
    }
    read_skill_package_file_at(&path, &display_path)
}

pub(crate) fn read_skill_package_file_at(
    path: &Path,
    display_path: &str,
) -> Result<SkillFileContentPayload, CommandErrorPayload> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        runtime_operation_failed(format!("skill package file metadata failed: {error}"))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(runtime_operation_failed(
            "skill package must not use symlinks".to_owned(),
        ));
    }
    if !metadata.is_file() {
        return Err(invalid_payload("skill file not found".to_owned()));
    }
    if metadata.len() > MAX_SKILL_PACKAGE_FILE_BYTES {
        return Err(invalid_payload(
            "skill package file is too large".to_owned(),
        ));
    }
    let bytes =
        read_regular_file_no_follow(path, "skill package file", MAX_SKILL_PACKAGE_FILE_BYTES)?;
    let content = String::from_utf8(bytes)
        .map_err(|_| invalid_payload("skill package file must be valid UTF-8".to_owned()))?;
    Ok(SkillFileContentPayload {
        path: display_path.to_owned(),
        content,
    })
}

pub(crate) fn normalize_skill_relative_path(value: &str) -> Result<PathBuf, CommandErrorPayload> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid_payload("skill file path is required".to_owned()));
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        return Err(invalid_payload(
            "skill file path must be relative".to_owned(),
        ));
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) => normalized.push(value),
            _ => return Err(invalid_payload("skill file path is invalid".to_owned())),
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(invalid_payload("skill file path is required".to_owned()));
    }
    Ok(normalized)
}

#[derive(Clone)]
pub struct DesktopProviderDiagnosticsStore {
    runtime_root: PathBuf,
}

impl DesktopProviderDiagnosticsStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            runtime_root: workspace_root.join(".jyowo").join("runtime"),
        }
    }

    pub fn new_runtime_root(runtime_root: PathBuf) -> Self {
        Self { runtime_root }
    }

    fn diagnostics_path(&self) -> PathBuf {
        self.runtime_root.join("provider-diagnostics.json")
    }
}

impl ProviderDiagnosticsStore for DesktopProviderDiagnosticsStore {
    fn load_record(&self) -> Result<ProviderDiagnosticsRecord, CommandErrorPayload> {
        let diagnostics_path = self.diagnostics_path();
        Ok(
            read_json_file_or_remove_invalid(&diagnostics_path, "provider diagnostics")?.unwrap_or(
                ProviderDiagnosticsRecord {
                    snapshots: Vec::new(),
                },
            ),
        )
    }

    fn upsert_snapshot(&self, snapshot: &ProviderProbeSnapshot) -> Result<(), CommandErrorPayload> {
        let diagnostics_path = self.diagnostics_path();
        let mut record = self.load_record()?;
        if let Some(existing) = record
            .snapshots
            .iter_mut()
            .find(|entry| entry.config_id == snapshot.config_id)
        {
            *existing = snapshot.clone();
        } else {
            record.snapshots.push(snapshot.clone());
        }
        let bytes = serde_json::to_vec_pretty(&record).map_err(|error| {
            runtime_operation_failed(format!(
                "provider diagnostics serialization failed: {error}"
            ))
        })?;
        write_atomic_runtime_file(
            &diagnostics_path,
            "provider-diagnostics.json",
            "provider diagnostics",
            &bytes,
        )
    }
}

#[derive(Clone)]
pub struct DesktopProviderQuotaCacheStore {
    runtime_root: PathBuf,
}

impl DesktopProviderQuotaCacheStore {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            runtime_root: workspace_root.join(".jyowo").join("runtime"),
        }
    }

    pub fn new_runtime_root(runtime_root: PathBuf) -> Self {
        Self { runtime_root }
    }

    fn quota_cache_path(&self) -> PathBuf {
        self.runtime_root.join("provider-quota-cache.json")
    }
}

impl super::ProviderQuotaCacheStore for DesktopProviderQuotaCacheStore {
    fn load_record(&self) -> Result<ProviderQuotaCacheRecord, CommandErrorPayload> {
        let quota_cache_path = self.quota_cache_path();
        Ok(
            read_json_file_or_remove_invalid(&quota_cache_path, "provider quota cache")?.unwrap_or(
                ProviderQuotaCacheRecord {
                    snapshots: Vec::new(),
                },
            ),
        )
    }

    fn upsert_snapshot(
        &self,
        snapshot: &harness_contracts::OfficialQuotaSnapshot,
    ) -> Result<(), CommandErrorPayload> {
        let quota_cache_path = self.quota_cache_path();
        let mut record = self.load_record()?;
        if let Some(existing) = record
            .snapshots
            .iter_mut()
            .find(|entry| entry.config_id == snapshot.config_id)
        {
            *existing = snapshot.clone();
        } else {
            record.snapshots.push(snapshot.clone());
        }
        let bytes = serde_json::to_vec_pretty(&record).map_err(|error| {
            runtime_operation_failed(format!(
                "provider quota cache serialization failed: {error}"
            ))
        })?;
        write_atomic_runtime_file(
            &quota_cache_path,
            "provider-quota-cache.json",
            "provider quota cache",
            &bytes,
        )
    }
}

#[derive(Clone)]
pub struct DesktopRuntimeState {
    pub(crate) active_runtime: Arc<RwLock<DesktopActiveRuntime>>,
    pub(crate) automation_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) automation_store: Arc<dyn AutomationStore>,
    pub(crate) conversation_metadata_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) conversation_metadata_store: Arc<dyn ConversationMetadataStore>,
    pub(crate) conversation_event_subscriptions:
        Arc<tokio::sync::Mutex<HashMap<String, ConversationSubscriptionHandle>>>,
    pub(crate) default_conversation_id: SessionId,
    pub(crate) deleted_conversation_ids: Arc<tokio::sync::Mutex<HashSet<SessionId>>>,
    pub(crate) memory_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) mcp_diagnostic_store: Arc<dyn McpDiagnosticStore>,
    pub(crate) mcp_diagnostic_subscriptions:
        Arc<tokio::sync::Mutex<HashMap<String, McpDiagnosticSubscriptionHandle>>>,
    pub(crate) mcp_server_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) mcp_server_store: Arc<dyn McpServerStore>,
    pub(crate) permission_resolver: Option<Arc<dyn PermissionResolver>>,
    pub(crate) provider_api_key_reveal_tokens:
        Arc<tokio::sync::Mutex<HashMap<String, ProviderConfigRevealTokenRecord>>>,
    pub(crate) plugin_store: Arc<dyn PluginStore>,
    pub(crate) plugin_store_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) provider_settings_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) provider_settings_store: Arc<dyn ProviderSettingsStore>,
    pub(crate) provider_diagnostics_store: Arc<dyn ProviderDiagnosticsStore>,
    pub(crate) provider_probe_flights: ProviderProbeFlights,
    pub(crate) provider_quota_cache_store: Arc<dyn ProviderQuotaCacheStore>,
    pub(crate) official_quota_flights: OfficialQuotaFlights,
    pub(crate) account_usage_registry: Arc<ProviderAccountUsageRegistry>,
    pub(crate) provider_capability_route_store: Arc<dyn ProviderCapabilityRouteStore>,
    pub(crate) provider_capability_routes: Arc<ParkingRwLock<ProviderCapabilityRouteSettings>>,
    pub(crate) execution_settings_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) execution_settings_store: Arc<DesktopExecutionSettingsStore>,
    pub(crate) global_config_store: Option<GlobalConfigStore>,
    pub(crate) project_config_store: Option<ProjectConfigStore>,
    pub(crate) skill_catalog_install_tasks:
        Arc<RwLock<HashMap<SkillCatalogInstallTaskKey, SkillCatalogInstallTaskPayload>>>,
    pub(crate) skill_store: Arc<dyn SkillStore>,
    pub(crate) skill_store_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) start_run_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) stream_permission_runtime: Option<Arc<StreamPermissionRuntime>>,
    pub(crate) runtime_layout: crate::storage_layout::RuntimeLayout,
    pub(crate) workspace_root: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct StoreTestRecord {
        value: String,
    }

    fn canonical_temp_root(temp: &tempfile::TempDir) -> PathBuf {
        temp.path().canonicalize().expect("canonical tempdir")
    }

    #[test]
    fn write_json_file_atomic_replaces_via_temp_file_without_leaving_temp_files() {
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
            "test settings",
            &StoreTestRecord {
                value: "new".to_owned(),
            },
        )
        .expect("write should succeed");

        let saved: StoreTestRecord = serde_json::from_slice(&std::fs::read(&path).unwrap())
            .expect("saved json should parse");
        assert_eq!(
            saved,
            StoreTestRecord {
                value: "new".to_owned(),
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
            "successful atomic write should not leave temp files"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_secret_json_file_atomic_creates_owner_only_file() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("secret-settings.json");

        write_secret_json_file_atomic(
            &path,
            "secret test settings",
            &StoreTestRecord {
                value: "secret".to_owned(),
            },
        )
        .expect("secret write should succeed");

        let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn read_secret_json_file_tightens_existing_file() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("secret-settings.json");
        std::fs::write(&path, br#"{"value":"secret"}"#).expect("seed secret file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("wide permissions");

        let loaded: StoreTestRecord = read_secret_json_file(&path, "secret test settings")
            .expect("read should succeed")
            .expect("record should exist");

        assert_eq!(
            loaded,
            StoreTestRecord {
                value: "secret".to_owned(),
            }
        );
        let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn write_automation_specs_creates_owner_only_file() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("automations.json");
        let now = Utc::now();
        let records = vec![AutomationSpec {
            id: "daily-check".to_owned(),
            enabled: true,
            prompt: "private prompt".to_owned(),
            schedule: harness_contracts::AutomationSchedule {
                interval_minutes: 60,
            },
            tool_profile: ToolProfile::Minimal,
            permission_mode: PermissionMode::Default,
            sandbox_mode: SandboxMode::None,
            workspace_scope: AutomationWorkspaceScope::CurrentWorkspace,
            workspace_access: WorkspaceAccess::ReadOnly,
            missed_run_policy: MissedRunPolicy::Skip,
            created_at: now,
            updated_at: now,
        }];

        write_automation_specs(&path, &records).expect("automation write should succeed");

        let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn read_json_file_reports_typed_error_for_invalid_json() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("settings.json");
        std::fs::write(&path, b"{not-json").expect("invalid json");

        let error = read_json_file::<StoreTestRecord>(&path, "test settings")
            .expect_err("invalid json should fail");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(error.message.contains("test settings parse failed"));
        assert!(path.exists(), "read should not quarantine unless asked");
    }

    #[test]
    fn quarantine_invalid_json_file_moves_file_when_requested() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("settings.json");
        std::fs::write(&path, b"{not-json").expect("invalid json");

        let quarantine_path =
            quarantine_invalid_json_file(&path, "test settings").expect("quarantine");

        assert!(!path.exists());
        assert!(quarantine_path.exists());
        assert_eq!(quarantine_path.extension().unwrap(), "invalid");
    }

    #[cfg(unix)]
    #[test]
    fn quarantine_invalid_json_file_rejects_symlink_file_without_moving_target() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let target = temp_root.join("target.json");
        let path = temp_root.join("settings.json");
        std::fs::write(&target, b"{not-json").expect("invalid json target");
        std::os::unix::fs::symlink(&target, &path).expect("symlink");

        let error = quarantine_invalid_json_file(&path, "test settings")
            .expect_err("symlink file must not be quarantined");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(error.message.contains("symlink"));
        assert!(target.exists());
        assert!(std::fs::symlink_metadata(&path)
            .expect("link metadata")
            .file_type()
            .is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn remove_invalid_json_file_rejects_symlink_file_without_deleting_target() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let target = temp_root.join("target.json");
        let path = temp_root.join("settings.json");
        std::fs::write(&target, b"{not-json").expect("invalid json target");
        std::os::unix::fs::symlink(&target, &path).expect("symlink");

        let error = remove_invalid_json_file(&path, "test settings")
            .expect_err("symlink file must not be removed");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(error.message.contains("symlink"));
        assert!(target.exists());
        assert!(std::fs::symlink_metadata(&path)
            .expect("link metadata")
            .file_type()
            .is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_app_dir_no_symlink_rejects_parent_symlink_components() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let external = tempfile::tempdir().expect("external tempdir");
        let symlinked_parent = temp_root.join("config");
        std::os::unix::fs::symlink(external.path(), &symlinked_parent).expect("symlink");
        let path = symlinked_parent.join("settings.json");

        let error = write_json_file_atomic(
            &path,
            "test settings",
            &StoreTestRecord {
                value: "blocked".to_owned(),
            },
        )
        .expect_err("write should reject symlink parent");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(error.message.contains("must not use symlinks"));
        assert!(!external.path().join("settings.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_uses_no_follow_parent_directory_handle() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("runtime").join("settings.json");

        let parent = open_parent_dir_no_symlink_for_write(&path, "test settings")
            .expect("parent directory handle");

        assert_eq!(
            parent.file_name().to_str().expect("utf8 file name"),
            "settings.json"
        );
        parent.sync_all("test settings").expect("sync parent");
    }

    #[cfg(unix)]
    #[test]
    fn plugin_package_copy_returns_hash_of_installed_snapshot() {
        let source = tempfile::tempdir().expect("source tempdir");
        let destination = tempfile::tempdir().expect("destination tempdir");
        let destination_path = destination
            .path()
            .canonicalize()
            .expect("canonical destination")
            .join("package");
        std::fs::write(source.path().join("plugin.json"), b"{\"name\":\"test\"}")
            .expect("manifest");

        let source_path = source.path().canonicalize().expect("canonical source");
        let copied_hash =
            copy_plugin_package(&source_path, &destination_path).expect("copy package");
        let installed_hash = hash_plugin_package(&destination_path).expect("hash installed");

        assert_eq!(copied_hash, installed_hash);
    }

    #[cfg(unix)]
    #[test]
    fn skill_package_copy_rejects_symlink_destination_without_deleting_target_dir() {
        let source = tempfile::tempdir().expect("source tempdir");
        let destination = tempfile::tempdir().expect("destination tempdir");
        let external = tempfile::tempdir().expect("external tempdir");
        let source_path = canonical_temp_root(&source);
        let destination_root = canonical_temp_root(&destination);
        let external_root = canonical_temp_root(&external);
        std::fs::write(source_path.join("SKILL.md"), b"# Test").expect("skill file");
        std::fs::write(external_root.join("keep.txt"), b"keep").expect("external file");
        let destination_path = destination_root.join("package");
        std::os::unix::fs::symlink(&external_root, &destination_path).expect("symlink");

        let error = copy_skill_package(&source_path, &destination_path)
            .expect_err("symlink destination must be rejected");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(error.message.contains("symlink"));
        assert!(external_root.join("keep.txt").exists());
        assert!(std::fs::symlink_metadata(&destination_path)
            .expect("link metadata")
            .file_type()
            .is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn plugin_package_copy_rejects_symlink_destination_without_deleting_target_dir() {
        let source = tempfile::tempdir().expect("source tempdir");
        let destination = tempfile::tempdir().expect("destination tempdir");
        let external = tempfile::tempdir().expect("external tempdir");
        let source_path = canonical_temp_root(&source);
        let destination_root = canonical_temp_root(&destination);
        let external_root = canonical_temp_root(&external);
        std::fs::write(source_path.join("plugin.json"), b"{\"name\":\"test\"}").expect("manifest");
        std::fs::write(external_root.join("keep.txt"), b"keep").expect("external file");
        let destination_path = destination_root.join("package");
        std::os::unix::fs::symlink(&external_root, &destination_path).expect("symlink");

        let error = copy_plugin_package(&source_path, &destination_path)
            .expect_err("symlink destination must be rejected");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(error.message.contains("symlink"));
        assert!(external_root.join("keep.txt").exists());
        assert!(std::fs::symlink_metadata(&destination_path)
            .expect("link metadata")
            .file_type()
            .is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn no_follow_regular_file_reader_rejects_symlink_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let target = temp_root.join("target.txt");
        let link = temp_root.join("link.txt");
        std::fs::write(&target, "secret").expect("target file");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");

        let error = read_regular_file_no_follow(&link, "test file", 64)
            .expect_err("symlink file must be rejected");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(error.message.contains("symlink"));
    }

    #[cfg(unix)]
    #[test]
    fn no_follow_regular_file_reader_rejects_symlink_parent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let outside = tempfile::tempdir().expect("outside tempdir");
        std::fs::write(outside.path().join("target.txt"), "secret").expect("target file");
        std::os::unix::fs::symlink(outside.path(), temp_root.join("linked"))
            .expect("symlink parent");

        let error =
            read_regular_file_no_follow(&temp_root.join("linked/target.txt"), "test file", 64)
                .expect_err("symlink parent must be rejected");

        assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
        assert!(error.message.contains("symlink"));
    }

    #[test]
    fn append_jsonl_record_appends_without_rewriting_existing_lines() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("runtime").join("mcp-diagnostics.jsonl");
        let first = McpDiagnosticRecord {
            event_type: "connected".to_owned(),
            id: "diagnostic-1".to_owned(),
            server_id: "server-a".to_owned(),
            severity: McpDiagnosticSeverity::Info,
            summary: "connected".to_owned(),
            timestamp: "2026-07-06T00:00:00Z".to_owned(),
        };
        let second = McpDiagnosticRecord {
            id: "diagnostic-2".to_owned(),
            timestamp: "2026-07-06T00:00:01Z".to_owned(),
            ..first.clone()
        };

        append_jsonl_record_locked(&path, "mcp diagnostics", &first).expect("first append");
        #[cfg(unix)]
        let first_inode = {
            use std::os::unix::fs::MetadataExt;

            std::fs::metadata(&path).unwrap().ino()
        };
        append_jsonl_record_locked(&path, "mcp diagnostics", &second).expect("second append");

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            assert_eq!(std::fs::metadata(&path).unwrap().ino(), first_inode);
        }
        let lines = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("diagnostic-1"));
        assert!(lines[1].contains("diagnostic-2"));
    }

    #[test]
    fn append_jsonl_record_with_retention_rotates_segment_and_keeps_recent_records() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_root = canonical_temp_root(&temp);
        let path = temp_root.join("runtime").join("mcp-diagnostics.jsonl");
        let record = |id: &str| McpDiagnosticRecord {
            event_type: "connected".to_owned(),
            id: id.to_owned(),
            server_id: "server-a".to_owned(),
            severity: McpDiagnosticSeverity::Info,
            summary: id.to_owned(),
            timestamp: "2026-07-06T00:00:00Z".to_owned(),
        };

        append_jsonl_record_with_retention_locked(
            &path,
            "mcp diagnostics",
            &record("diagnostic-1"),
            2,
            |error| runtime_operation_failed(format!("mcp diagnostics parse failed: {error}")),
            |_| Ok(()),
        )
        .expect("first append");
        append_jsonl_record_with_retention_locked(
            &path,
            "mcp diagnostics",
            &record("diagnostic-2"),
            2,
            |error| runtime_operation_failed(format!("mcp diagnostics parse failed: {error}")),
            |_| Ok(()),
        )
        .expect("second append");
        append_jsonl_record_with_retention_locked(
            &path,
            "mcp diagnostics",
            &record("diagnostic-3"),
            2,
            |error| runtime_operation_failed(format!("mcp diagnostics parse failed: {error}")),
            |_| Ok(()),
        )
        .expect("retention append");

        let lines = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("diagnostic-2"));
        assert!(lines[1].contains("diagnostic-3"));
        assert!(
            std::fs::read_dir(path.parent().unwrap())
                .unwrap()
                .any(|entry| entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains(".segment")),
            "retention rotation should archive the previous active segment"
        );
    }
}

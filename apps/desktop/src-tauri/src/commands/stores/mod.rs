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

mod automation;
mod mcp;
mod plugin;
mod skill;

pub use automation::DesktopAutomationStore;
pub use mcp::DesktopMcpDiagnosticStore;
pub(crate) use mcp::DesktopMcpServerStore;
pub use plugin::DesktopPluginStore;
pub use skill::DesktopSkillStore;

pub(crate) fn write_mcp_server_records(
    settings_path: &Path,
    records: &[McpServerConfigRecord],
) -> Result<(), CommandErrorPayload> {
    let parent = settings_path.parent().ok_or_else(|| {
        runtime_operation_failed("mcp server settings path has no parent".to_owned())
    })?;
    ensure_no_symlink_components(parent, "mcp server settings directory")?;
    std::fs::create_dir_all(parent).map_err(|error| {
        runtime_operation_failed(format!(
            "mcp server settings directory unavailable: {error}"
        ))
    })?;
    ensure_no_symlink_components(parent, "mcp server settings directory")?;
    let bytes = serde_json::to_vec_pretty(records).map_err(|error| {
        runtime_operation_failed(format!("mcp server settings serialization failed: {error}"))
    })?;
    let temp_path = settings_path.with_file_name(format!(
        "{}.{}.tmp",
        settings_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("mcp-servers.json"),
        RunId::new()
    ));
    ensure_no_symlink_components(&temp_path, "mcp server settings temp file")?;
    let mut temp_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .map_err(|error| {
            runtime_operation_failed(format!("mcp server settings temp open failed: {error}"))
        })?;
    if let Err(error) = temp_file.write_all(&bytes) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "mcp server settings write failed: {error}"
        )));
    }
    if let Err(error) = temp_file.sync_all() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "mcp server settings sync failed: {error}"
        )));
    }
    drop(temp_file);
    ensure_no_symlink_components(settings_path, "mcp server settings file")?;
    std::fs::rename(&temp_path, settings_path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        runtime_operation_failed(format!("mcp server settings commit failed: {error}"))
    })
}

pub(crate) fn write_mcp_diagnostic_records(
    diagnostics_path: &Path,
    records: &[McpDiagnosticRecord],
) -> Result<(), CommandErrorPayload> {
    let parent = diagnostics_path
        .parent()
        .ok_or_else(|| runtime_operation_failed("mcp diagnostics path has no parent".to_owned()))?;
    ensure_no_symlink_components(parent, "mcp diagnostics directory")?;
    std::fs::create_dir_all(parent).map_err(|error| {
        runtime_operation_failed(format!("mcp diagnostics directory unavailable: {error}"))
    })?;
    ensure_no_symlink_components(parent, "mcp diagnostics directory")?;
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, record).map_err(|error| {
            runtime_operation_failed(format!("mcp diagnostics serialization failed: {error}"))
        })?;
        bytes.push(b'\n');
    }
    let temp_path = diagnostics_path.with_file_name(format!(
        "{}.{}.tmp",
        diagnostics_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("mcp-diagnostics.jsonl"),
        RunId::new()
    ));
    ensure_no_symlink_components(&temp_path, "mcp diagnostics temp file")?;
    let mut temp_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .map_err(|error| {
            runtime_operation_failed(format!("mcp diagnostics temp open failed: {error}"))
        })?;
    if let Err(error) = temp_file.write_all(&bytes) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "mcp diagnostics write failed: {error}"
        )));
    }
    if let Err(error) = temp_file.sync_all() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "mcp diagnostics sync failed: {error}"
        )));
    }
    drop(temp_file);
    ensure_no_symlink_components(diagnostics_path, "mcp diagnostics file")?;
    std::fs::rename(&temp_path, diagnostics_path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        runtime_operation_failed(format!("mcp diagnostics commit failed: {error}"))
    })
}

pub(crate) fn write_automation_specs(
    automations_path: &Path,
    records: &[AutomationSpec],
) -> Result<(), CommandErrorPayload> {
    let parent = automations_path.parent().ok_or_else(|| {
        runtime_operation_failed("automation settings path has no parent".to_owned())
    })?;
    ensure_no_symlink_components(parent, "automation settings directory")?;
    std::fs::create_dir_all(parent).map_err(|error| {
        runtime_operation_failed(format!(
            "automation settings directory unavailable: {error}"
        ))
    })?;
    ensure_no_symlink_components(parent, "automation settings directory")?;
    let bytes = serde_json::to_vec_pretty(records).map_err(|error| {
        runtime_operation_failed(format!("automation settings serialization failed: {error}"))
    })?;
    write_atomic_runtime_file(
        automations_path,
        "automations.json",
        "automation settings",
        &bytes,
    )
}

pub(crate) fn write_automation_run_records(
    runs_path: &Path,
    records: &[AutomationRunRecord],
) -> Result<(), CommandErrorPayload> {
    let parent = runs_path.parent().ok_or_else(|| {
        runtime_operation_failed("automation run ledger path has no parent".to_owned())
    })?;
    ensure_no_symlink_components(parent, "automation run ledger directory")?;
    std::fs::create_dir_all(parent).map_err(|error| {
        runtime_operation_failed(format!(
            "automation run ledger directory unavailable: {error}"
        ))
    })?;
    ensure_no_symlink_components(parent, "automation run ledger directory")?;
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, record).map_err(|error| {
            runtime_operation_failed(format!(
                "automation run ledger serialization failed: {error}"
            ))
        })?;
        bytes.push(b'\n');
    }
    write_atomic_runtime_file(
        runs_path,
        "automation-runs.jsonl",
        "automation run ledger",
        &bytes,
    )
}

pub(crate) fn write_atomic_runtime_file(
    target_path: &Path,
    fallback_name: &str,
    label: &str,
    bytes: &[u8],
) -> Result<(), CommandErrorPayload> {
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

        open_options.mode(0o600);
    }
    let mut temp_file = open_options
        .open(&temp_path)
        .map_err(|error| runtime_operation_failed(format!("{label} temp open failed: {error}")))?;
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
    })
}

pub(crate) fn write_skill_records(
    index_path: &Path,
    records: &[SkillStoreRecord],
) -> Result<(), CommandErrorPayload> {
    let parent = index_path
        .parent()
        .ok_or_else(|| runtime_operation_failed("skill index path has no parent".to_owned()))?;
    ensure_no_symlink_components(parent, "skill index directory")?;
    std::fs::create_dir_all(parent).map_err(|error| {
        runtime_operation_failed(format!("skill index directory unavailable: {error}"))
    })?;
    ensure_no_symlink_components(parent, "skill index directory")?;
    let bytes = serde_json::to_vec_pretty(records).map_err(|error| {
        runtime_operation_failed(format!("skill index serialization failed: {error}"))
    })?;
    let temp_path = index_path.with_file_name(format!(
        "{}.{}.tmp",
        index_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("index.json"),
        RunId::new()
    ));
    ensure_no_symlink_components(&temp_path, "skill index temp file")?;
    let mut temp_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .map_err(|error| {
            runtime_operation_failed(format!("skill index temp open failed: {error}"))
        })?;
    if let Err(error) = temp_file.write_all(&bytes) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "skill index write failed: {error}"
        )));
    }
    if let Err(error) = temp_file.sync_all() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "skill index sync failed: {error}"
        )));
    }
    drop(temp_file);
    ensure_no_symlink_components(index_path, "skill index file")?;
    std::fs::rename(&temp_path, index_path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        runtime_operation_failed(format!("skill index commit failed: {error}"))
    })
}

pub(crate) fn write_plugin_settings_record(
    index_path: &Path,
    record: &PluginSettingsRecord,
) -> Result<(), CommandErrorPayload> {
    let parent = index_path
        .parent()
        .ok_or_else(|| runtime_operation_failed("plugin index path has no parent".to_owned()))?;
    ensure_no_symlink_components(parent, "plugin index directory")?;
    std::fs::create_dir_all(parent).map_err(|error| {
        runtime_operation_failed(format!("plugin index directory unavailable: {error}"))
    })?;
    ensure_no_symlink_components(parent, "plugin index directory")?;
    let bytes = serde_json::to_vec_pretty(record).map_err(|error| {
        runtime_operation_failed(format!("plugin index serialization failed: {error}"))
    })?;
    let temp_path = index_path.with_file_name(format!(
        "{}.{}.tmp",
        index_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("index.json"),
        RunId::new()
    ));
    ensure_no_symlink_components(&temp_path, "plugin index temp file")?;
    let mut temp_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .map_err(|error| {
            runtime_operation_failed(format!("plugin index temp open failed: {error}"))
        })?;
    if let Err(error) = temp_file.write_all(&bytes) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "plugin index write failed: {error}"
        )));
    }
    if let Err(error) = temp_file.sync_all() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(runtime_operation_failed(format!(
            "plugin index sync failed: {error}"
        )));
    }
    drop(temp_file);
    ensure_no_symlink_components(index_path, "plugin index file")?;
    std::fs::rename(&temp_path, index_path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        runtime_operation_failed(format!("plugin index commit failed: {error}"))
    })
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
        current.push(component.as_os_str());
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
        if metadata.len() > MAX_SKILL_PACKAGE_FILE_BYTES {
            return Err(invalid_payload(
                "skill package file is too large".to_owned(),
            ));
        }
        *total_bytes = total_bytes.saturating_add(metadata.len());
        if *total_bytes > MAX_SKILL_PACKAGE_BYTES {
            return Err(invalid_payload("skill package is too large".to_owned()));
        }
        let relative_path = path.strip_prefix(root).map_err(|_| {
            runtime_operation_failed("skill package path escaped its root".to_owned())
        })?;
        let bytes = std::fs::read(&path).map_err(|error| {
            runtime_operation_failed(format!("skill package file read failed: {error}"))
        })?;
        hasher.update(path_to_workspace_string(relative_path).as_bytes());
        hasher.update(&[0]);
        hasher.update(&bytes);
        hasher.update(&[0]);
    }
    Ok(())
}

pub(crate) fn copy_skill_package(
    source_path: &Path,
    destination_path: &Path,
) -> Result<(), CommandErrorPayload> {
    ensure_no_symlink_components(source_path, "skill source package")?;
    ensure_no_symlink_components(destination_path, "skill package")?;
    let _ = hash_skill_package(source_path)?;
    if destination_path.exists() {
        std::fs::remove_dir_all(destination_path).map_err(|error| {
            runtime_operation_failed(format!("skill package cleanup failed: {error}"))
        })?;
    }
    std::fs::create_dir_all(destination_path).map_err(|error| {
        runtime_operation_failed(format!("skill package directory unavailable: {error}"))
    })?;
    let mut file_count = 0_usize;
    let mut total_bytes = 0_u64;
    match copy_skill_package_dir(
        source_path,
        source_path,
        destination_path,
        &mut file_count,
        &mut total_bytes,
    ) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_dir_all(destination_path);
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
            std::fs::create_dir_all(&destination_path).map_err(|error| {
                runtime_operation_failed(format!("skill package directory copy failed: {error}"))
            })?;
            copy_skill_package_dir(root, &path, destination_root, file_count, total_bytes)?;
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
        if metadata.len() > MAX_SKILL_PACKAGE_FILE_BYTES {
            return Err(invalid_payload(
                "skill package file is too large".to_owned(),
            ));
        }
        *total_bytes = total_bytes.saturating_add(metadata.len());
        if *total_bytes > MAX_SKILL_PACKAGE_BYTES {
            return Err(invalid_payload("skill package is too large".to_owned()));
        }
        let parent = destination_path.parent().ok_or_else(|| {
            runtime_operation_failed("skill package file path has no parent".to_owned())
        })?;
        std::fs::create_dir_all(parent).map_err(|error| {
            runtime_operation_failed(format!("skill package directory copy failed: {error}"))
        })?;
        std::fs::copy(&path, &destination_path).map_err(|error| {
            runtime_operation_failed(format!("skill package file copy failed: {error}"))
        })?;
    }
    Ok(())
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
        let bytes = std::fs::read(&path).map_err(|error| {
            runtime_operation_failed(format!("plugin package file read failed: {error}"))
        })?;
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
) -> Result<(), CommandErrorPayload> {
    ensure_no_symlink_components(source_path, "plugin source package")?;
    ensure_no_symlink_components(destination_path, "plugin package")?;
    let _ = hash_plugin_package(source_path)?;
    if destination_path.exists() {
        std::fs::remove_dir_all(destination_path).map_err(|error| {
            runtime_operation_failed(format!("plugin package cleanup failed: {error}"))
        })?;
    }
    std::fs::create_dir_all(destination_path).map_err(|error| {
        runtime_operation_failed(format!("plugin package directory unavailable: {error}"))
    })?;
    let mut file_count = 0_usize;
    let mut total_bytes = 0_u64;
    match copy_plugin_package_dir(
        source_path,
        source_path,
        destination_path,
        &mut file_count,
        &mut total_bytes,
    ) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_dir_all(destination_path);
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
            std::fs::create_dir_all(&destination_path).map_err(|error| {
                runtime_operation_failed(format!("plugin package directory copy failed: {error}"))
            })?;
            copy_plugin_package_dir(root, &path, destination_root, file_count, total_bytes)?;
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
        let parent = destination_path.parent().ok_or_else(|| {
            runtime_operation_failed("plugin package file path has no parent".to_owned())
        })?;
        std::fs::create_dir_all(parent).map_err(|error| {
            runtime_operation_failed(format!("plugin package directory copy failed: {error}"))
        })?;
        std::fs::copy(&path, &destination_path).map_err(|error| {
            runtime_operation_failed(format!("plugin package file copy failed: {error}"))
        })?;
    }
    Ok(())
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
    let bytes = std::fs::read(path).map_err(|error| {
        runtime_operation_failed(format!("skill package file read failed: {error}"))
    })?;
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
pub struct DesktopRuntimeState {
    pub(crate) active_runtime: Arc<RwLock<DesktopActiveRuntime>>,
    pub(crate) automation_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) automation_store: Arc<dyn AutomationStore>,
    pub(crate) conversation_model_config_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) conversation_model_config_store: Arc<dyn ConversationModelConfigStore>,
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
    pub(crate) provider_capability_route_store: Arc<DesktopProviderCapabilityRouteStore>,
    pub(crate) provider_capability_routes: Arc<ParkingRwLock<ProviderCapabilityRouteSettings>>,
    pub(crate) execution_settings_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) execution_settings_store: Arc<DesktopExecutionSettingsStore>,
    pub(crate) skill_catalog_install_tasks:
        Arc<RwLock<HashMap<SkillCatalogInstallTaskKey, SkillCatalogInstallTaskPayload>>>,
    pub(crate) skill_store: Arc<dyn SkillStore>,
    pub(crate) skill_store_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) start_run_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) stream_permission_runtime: Option<Arc<StreamPermissionRuntime>>,
    pub(crate) workspace_root: PathBuf,
}

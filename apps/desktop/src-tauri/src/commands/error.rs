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
use super::stores::*;
#[allow(unused_imports)]
use super::validation::*;
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandErrorPayload {
    pub code: &'static str,
    pub message: String,
}

pub(crate) fn invalid_payload(message: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "INVALID_PAYLOAD",
        message,
    }
}

pub(crate) fn runtime_unavailable(message: &str) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "RUNTIME_UNAVAILABLE",
        message: message.to_owned(),
    }
}

pub(crate) fn runtime_init_failed(message: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "RUNTIME_INIT_FAILED",
        message,
    }
}

pub(crate) fn runtime_operation_failed(message: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "RUNTIME_OPERATION_FAILED",
        message,
    }
}

pub(crate) fn not_found(message: String) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "NOT_FOUND",
        message,
    }
}

pub(crate) fn conversation_read_error(error: impl std::fmt::Display) -> CommandErrorPayload {
    let message = error.to_string();
    if message.contains("session not found") {
        return not_found(message);
    }
    runtime_operation_failed(format!("conversation read failed: {message}"))
}

pub(crate) fn memory_operation_failed(message: &'static str) -> CommandErrorPayload {
    CommandErrorPayload {
        code: "RUNTIME_OPERATION_FAILED",
        message: message.to_owned(),
    }
}

pub(crate) fn support_bundle_operation_failed() -> CommandErrorPayload {
    CommandErrorPayload {
        code: "RUNTIME_OPERATION_FAILED",
        message: "Support bundle export could not be prepared.".to_owned(),
    }
}

pub(crate) fn support_bundle_read_error(error: CommandErrorPayload) -> CommandErrorPayload {
    if error.code == "INVALID_PAYLOAD" {
        return error;
    }

    support_bundle_operation_failed()
}

pub(crate) fn write_memory_export_file(
    path: &Path,
    content: &str,
) -> Result<(), CommandErrorPayload> {
    let Some(parent) = path.parent() else {
        return Err(memory_operation_failed(
            "Memory export could not be prepared.",
        ));
    };
    ensure_no_symlink_components(parent, "memory export directory")
        .map_err(|_| memory_operation_failed("Memory export could not be prepared."))?;
    std::fs::create_dir_all(parent)
        .map_err(|_| memory_operation_failed("Memory export could not be prepared."))?;
    std::fs::write(path, content)
        .map_err(|_| memory_operation_failed("Memory export could not be prepared."))
}

pub(crate) fn write_support_bundle_file(
    path: &Path,
    content: &str,
) -> Result<(), CommandErrorPayload> {
    write_support_bundle_bytes(path, content.as_bytes())
}

pub(crate) fn write_support_bundle_bytes(
    path: &Path,
    content: &[u8],
) -> Result<(), CommandErrorPayload> {
    let Some(parent) = path.parent() else {
        return Err(support_bundle_operation_failed());
    };
    ensure_no_symlink_components(parent, "support bundle export directory")
        .map_err(|_| support_bundle_operation_failed())?;
    std::fs::create_dir_all(parent).map_err(|_| support_bundle_operation_failed())?;
    ensure_no_symlink_components(parent, "support bundle export directory")
        .map_err(|_| support_bundle_operation_failed())?;
    ensure_no_symlink_components(path, "support bundle export file")
        .map_err(|_| support_bundle_operation_failed())?;

    let temp_path = path.with_file_name(format!(
        "{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("support-bundle"),
        RunId::new()
    ));
    ensure_no_symlink_components(&temp_path, "support bundle export temp file")
        .map_err(|_| support_bundle_operation_failed())?;

    let mut temp_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .map_err(|_| support_bundle_operation_failed())?;
    if temp_file.write_all(content).is_err() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(support_bundle_operation_failed());
    }
    if temp_file.sync_all().is_err() {
        let _ = std::fs::remove_file(&temp_path);
        return Err(support_bundle_operation_failed());
    }
    drop(temp_file);
    ensure_no_symlink_components(path, "support bundle export file")
        .map_err(|_| support_bundle_operation_failed())?;
    std::fs::rename(&temp_path, path).map_err(|_| {
        let _ = std::fs::remove_file(&temp_path);
        support_bundle_operation_failed()
    })
}

pub(crate) fn support_bundle_markdown(
    request: &ExportSupportBundleRequest,
    exported_at: String,
    event_count: u32,
) -> String {
    format!(
        "# Jyowo Support Bundle\n\n- exportedAt: {exported_at}\n- conversationId: {}\n- runId: {}\n- eventCount: {event_count}\n- redacted: true\n",
        request.conversation_id.as_deref().unwrap_or(""),
        request.run_id.as_deref().unwrap_or("")
    )
}

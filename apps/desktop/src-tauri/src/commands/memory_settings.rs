//! Tauri IPC helpers for memory settings.
//!
//! Settings are persisted in the workspace memory SQLite database. Rust owns
//! validation and storage; frontend callers only receive serde contract shapes.

use harness_contracts::{
    GetMemorySettingsRequest, GetMemorySettingsResponse, GetThreadMemorySettingsRequest,
    GetThreadMemorySettingsResponse, MemoryGlobalSettings, MemoryThreadSettings,
    UpdateMemorySettingsRequest, UpdateMemorySettingsResponse, UpdateThreadMemorySettingsRequest,
    UpdateThreadMemorySettingsResponse,
};

use super::error::{invalid_payload, memory_operation_failed, runtime_unavailable};
use super::{CommandErrorPayload, DesktopRuntimeState};

pub async fn get_memory_settings_with_runtime_state(
    request: GetMemorySettingsRequest,
    state: &DesktopRuntimeState,
) -> Result<GetMemorySettingsResponse, CommandErrorPayload> {
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Loading memory settings requires the runtime memory facade.",
        ));
    };
    let options = state.settings_session_options(state.default_conversation_id)?;
    settings_runtime
        .get_memory_settings(options, request)
        .await
        .map_err(|_| memory_operation_failed("Memory settings could not be loaded."))
}

pub async fn update_memory_settings_with_runtime_state(
    request: UpdateMemorySettingsRequest,
    state: &DesktopRuntimeState,
) -> Result<UpdateMemorySettingsResponse, CommandErrorPayload> {
    validate_global_settings(&request.settings)?;
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Saving memory settings requires the runtime memory facade.",
        ));
    };
    let options = state.settings_session_options(state.default_conversation_id)?;
    settings_runtime
        .update_memory_settings(options, request)
        .await
        .map_err(|_| memory_operation_failed("Memory settings could not be saved."))
}

pub async fn get_thread_memory_settings_with_runtime_state(
    request: GetThreadMemorySettingsRequest,
    state: &DesktopRuntimeState,
) -> Result<GetThreadMemorySettingsResponse, CommandErrorPayload> {
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Loading thread memory settings requires the runtime memory facade.",
        ));
    };
    let options = state.settings_session_options(state.default_conversation_id)?;
    settings_runtime
        .get_thread_memory_settings(options, request)
        .await
        .map_err(|_| memory_operation_failed("Memory settings could not be loaded."))
}

pub async fn update_thread_memory_settings_with_runtime_state(
    request: UpdateThreadMemorySettingsRequest,
    state: &DesktopRuntimeState,
) -> Result<UpdateThreadMemorySettingsResponse, CommandErrorPayload> {
    validate_thread_settings(&request.settings)?;
    let Some(settings_runtime) = state.settings_runtime() else {
        return Err(runtime_unavailable(
            "Saving thread memory settings requires the runtime memory facade.",
        ));
    };
    let options = state.settings_session_options(state.default_conversation_id)?;
    settings_runtime
        .update_thread_memory_settings(options, request)
        .await
        .map_err(|_| memory_operation_failed("Memory settings could not be saved."))
}

fn validate_global_settings(settings: &MemoryGlobalSettings) -> Result<(), CommandErrorPayload> {
    if settings.max_recall_records_per_turn == 0 {
        return Err(invalid_payload(
            "max_recall_records_per_turn must be greater than 0".to_owned(),
        ));
    }
    if settings.max_recall_chars_per_turn == 0 {
        return Err(invalid_payload(
            "max_recall_chars_per_turn must be greater than 0".to_owned(),
        ));
    }
    if settings.max_memory_bytes == 0 {
        return Err(invalid_payload(
            "max_memory_bytes must be greater than 0".to_owned(),
        ));
    }
    Ok(())
}

fn validate_thread_settings(settings: &MemoryThreadSettings) -> Result<(), CommandErrorPayload> {
    let _ = settings;
    Ok(())
}

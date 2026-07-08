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

use super::error::{invalid_payload, memory_operation_failed, not_found, runtime_unavailable};
use super::{CommandErrorPayload, DesktopRuntimeState};

pub async fn get_memory_settings_with_runtime_state(
    request: GetMemorySettingsRequest,
    state: &DesktopRuntimeState,
) -> Result<GetMemorySettingsResponse, CommandErrorPayload> {
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Loading memory settings requires the runtime memory facade.",
        ));
    };
    let options = state.conversation_session_options(state.default_conversation_id)?;
    harness
        .get_memory_settings(options, request)
        .await
        .map_err(|_| memory_operation_failed("Memory settings could not be loaded."))
}

pub async fn update_memory_settings_with_runtime_state(
    request: UpdateMemorySettingsRequest,
    state: &DesktopRuntimeState,
) -> Result<UpdateMemorySettingsResponse, CommandErrorPayload> {
    validate_global_settings(&request.settings)?;
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Saving memory settings requires the runtime memory facade.",
        ));
    };
    let options = state.conversation_session_options(state.default_conversation_id)?;
    harness
        .update_memory_settings(options, request)
        .await
        .map_err(|_| memory_operation_failed("Memory settings could not be saved."))
}

pub async fn get_thread_memory_settings_with_runtime_state(
    request: GetThreadMemorySettingsRequest,
    state: &DesktopRuntimeState,
) -> Result<GetThreadMemorySettingsResponse, CommandErrorPayload> {
    ensure_thread_memory_session_readable(request.session_id, state).await?;
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Loading thread memory settings requires the runtime memory facade.",
        ));
    };
    let options = state.conversation_session_options(state.default_conversation_id)?;
    harness
        .get_thread_memory_settings(options, request)
        .await
        .map_err(|_| memory_operation_failed("Memory settings could not be loaded."))
}

pub async fn update_thread_memory_settings_with_runtime_state(
    request: UpdateThreadMemorySettingsRequest,
    state: &DesktopRuntimeState,
) -> Result<UpdateThreadMemorySettingsResponse, CommandErrorPayload> {
    validate_thread_settings(&request.settings)?;
    ensure_thread_memory_session_readable(request.settings.session_id, state).await?;
    let Some(harness) = state.harness() else {
        return Err(runtime_unavailable(
            "Saving thread memory settings requires the runtime memory facade.",
        ));
    };
    let options = state.conversation_session_options(state.default_conversation_id)?;
    harness
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

async fn ensure_thread_memory_session_readable(
    session_id: harness_contracts::SessionId,
    state: &DesktopRuntimeState,
) -> Result<(), CommandErrorPayload> {
    if state
        .deleted_conversation_ids
        .lock()
        .await
        .contains(&session_id)
    {
        return Err(not_found(format!("conversation not found: {session_id}")));
    }
    Ok(())
}

//! Tauri IPC commands for memory settings.
//!
//! Exposes global and per-thread memory settings with Rust-owned validation.

use harness_contracts::{
    GetMemorySettingsRequest, GetMemorySettingsResponse, GetThreadMemorySettingsRequest,
    GetThreadMemorySettingsResponse, MemoryGlobalSettings, MemoryThreadMode, MemoryThreadSettings,
    UpdateMemorySettingsRequest, UpdateMemorySettingsResponse,
    UpdateThreadMemorySettingsRequest, UpdateThreadMemorySettingsResponse,
};
use super::CommandErrorPayload;

/// In-memory store for memory settings (backed by filesystem in production).
static GLOBAL_SETTINGS: std::sync::RwLock<Option<MemoryGlobalSettings>> =
    std::sync::RwLock::new(None);

fn default_settings() -> MemoryGlobalSettings {
    MemoryGlobalSettings {
        use_memories: true,
        generate_memories: true,
        disable_generation_when_external_context_used: false,
        retention_days: None,
        max_memory_bytes: 10_000_000,
        max_recall_records_per_turn: 20,
        max_recall_chars_per_turn: 50_000,
    }
}

fn ensure_global_settings() -> MemoryGlobalSettings {
    let settings = GLOBAL_SETTINGS.read().unwrap();
    settings.clone().unwrap_or_else(default_settings)
}

/// Get global memory settings.
#[tauri::command]
pub async fn get_memory_settings(
    request: GetMemorySettingsRequest,
) -> Result<GetMemorySettingsResponse, CommandErrorPayload> {
    let _ = request;
    let settings = ensure_global_settings();
    Ok(GetMemorySettingsResponse { settings })
}

/// Update global memory settings.
#[tauri::command]
pub async fn update_memory_settings(
    request: UpdateMemorySettingsRequest,
) -> Result<UpdateMemorySettingsResponse, CommandErrorPayload> {
    // Validate settings
    validate_global_settings(&request.settings)?;

    let mut guard = GLOBAL_SETTINGS.write().unwrap();
    *guard = Some(request.settings.clone());
    Ok(UpdateMemorySettingsResponse {
        settings: request.settings,
    })
}

/// Get per-thread memory settings.
#[tauri::command]
pub async fn get_thread_memory_settings(
    request: GetThreadMemorySettingsRequest,
) -> Result<GetThreadMemorySettingsResponse, CommandErrorPayload> {
    let _ = request;
    Ok(GetThreadMemorySettingsResponse {
        settings: MemoryThreadSettings {
            session_id: request.session_id,
            use_memories: None,
            generate_memories: None,
            memory_mode: MemoryThreadMode::ReadWrite,
        },
    })
}

/// Update per-thread memory settings.
#[tauri::command]
pub async fn update_thread_memory_settings(
    request: UpdateThreadMemorySettingsRequest,
) -> Result<UpdateThreadMemorySettingsResponse, CommandErrorPayload> {
    validate_thread_settings(&request.settings)?;
    Ok(UpdateThreadMemorySettingsResponse {
        settings: request.settings,
    })
}

fn validate_global_settings(settings: &MemoryGlobalSettings) -> Result<(), CommandErrorPayload> {
    if settings.max_recall_records_per_turn == 0 {
        return Err(CommandErrorPayload::new(
            "invalid_settings",
            "max_recall_records_per_turn must be greater than 0",
        ));
    }
    if settings.max_recall_chars_per_turn == 0 {
        return Err(CommandErrorPayload::new(
            "invalid_settings",
            "max_recall_chars_per_turn must be greater than 0",
        ));
    }
    if settings.max_memory_bytes == 0 {
        return Err(CommandErrorPayload::new(
            "invalid_settings",
            "max_memory_bytes must be greater than 0",
        ));
    }
    Ok(())
}

fn validate_thread_settings(settings: &MemoryThreadSettings) -> Result<(), CommandErrorPayload> {
    // Thread settings are always valid if they parse
    let _ = settings;
    Ok(())
}

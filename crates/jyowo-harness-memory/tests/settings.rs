//! Tests for durable memory settings storage.

use harness_contracts::{
    MemoryGlobalSettings, MemoryThreadMode, MemoryThreadSettings, SessionId, TenantId,
};
use harness_memory::settings::MemorySettingsStore;

fn global_settings(max_records: u32) -> MemoryGlobalSettings {
    MemoryGlobalSettings {
        use_memories: true,
        generate_memories: true,
        disable_generation_when_external_context_used: false,
        retention_days: Some(30),
        max_memory_bytes: 1_000_000,
        max_recall_records_per_turn: max_records,
        max_recall_chars_per_turn: 50_000,
    }
}

#[test]
fn sqlite_settings_persist_global_and_thread_overrides_after_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.sqlite3");
    let session_id = SessionId::new();

    {
        let store = MemorySettingsStore::open(db_path.to_str().unwrap()).unwrap();
        store
            .update_global(TenantId::SINGLE, global_settings(12))
            .unwrap();
        store
            .update_thread(
                TenantId::SINGLE,
                MemoryThreadSettings {
                    session_id,
                    use_memories: Some(false),
                    generate_memories: Some(false),
                    memory_mode: MemoryThreadMode::ReadOnly,
                },
            )
            .unwrap();
    }

    let reopened = MemorySettingsStore::open(db_path.to_str().unwrap()).unwrap();
    let global = reopened.get_global(TenantId::SINGLE).unwrap();
    let thread = reopened.get_thread(TenantId::SINGLE, session_id).unwrap();

    assert_eq!(global.max_recall_records_per_turn, 12);
    assert_eq!(thread.use_memories, Some(false));
    assert_eq!(thread.generate_memories, Some(false));
    assert_eq!(thread.memory_mode, MemoryThreadMode::ReadOnly);
}

#[test]
fn sqlite_settings_delete_thread_removes_only_that_thread_override() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("memory.sqlite3");
    let store = MemorySettingsStore::open(db_path.to_str().unwrap()).unwrap();
    let deleted_session_id = SessionId::new();
    let kept_session_id = SessionId::new();

    for session_id in [deleted_session_id, kept_session_id] {
        store
            .update_thread(
                TenantId::SINGLE,
                MemoryThreadSettings {
                    session_id,
                    use_memories: Some(false),
                    generate_memories: Some(false),
                    memory_mode: MemoryThreadMode::ReadOnly,
                },
            )
            .unwrap();
    }

    store
        .delete_thread(TenantId::SINGLE, deleted_session_id)
        .unwrap();

    let deleted = store
        .get_thread(TenantId::SINGLE, deleted_session_id)
        .unwrap();
    let kept = store.get_thread(TenantId::SINGLE, kept_session_id).unwrap();

    assert_eq!(deleted.session_id, deleted_session_id);
    assert_eq!(deleted.use_memories, None);
    assert_eq!(deleted.generate_memories, None);
    assert_eq!(deleted.memory_mode, MemoryThreadMode::ReadWrite);
    assert_eq!(kept.use_memories, Some(false));
    assert_eq!(kept.generate_memories, Some(false));
    assert_eq!(kept.memory_mode, MemoryThreadMode::ReadOnly);
}

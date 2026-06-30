use super::*;

#[tokio::test]
async fn memory_commands_list_inspect_update_delete_and_export_visible_items() {
    let provider = Arc::new(InMemoryMemoryProvider::new("test"));
    let state = runtime_state_with_memory_provider(provider.clone()).await;
    let session_id = state.default_conversation_id();
    let visible = test_memory_record(session_id, "Prefers concise Chinese responses");
    provider.upsert(visible.clone()).await.unwrap();
    provider
        .upsert(test_memory_record(
            SessionId::new(),
            "Hidden session memory",
        ))
        .await
        .unwrap();

    let list = list_memory_items_with_runtime_state(&state).await.unwrap();

    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].id, visible.id.to_string());
    assert_eq!(list.items[0].visibility, "private");
    assert_eq!(list.items[0].kind, "user_preference");

    let detail = get_memory_item_with_runtime_state(
        GetMemoryItemRequest {
            id: visible.id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(detail.item.content, "Prefers concise Chinese responses");

    let updated = update_memory_item_with_runtime_state(
        UpdateMemoryItemRequest {
            content: "Prefers terse Chinese responses".to_owned(),
            id: visible.id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(updated.item.content, "Prefers terse Chinese responses");

    let exported = export_memory_items_with_runtime_state(&state)
        .await
        .unwrap();
    assert_eq!(exported.format, "json");
    assert_eq!(exported.item_count, 1);
    assert!(exported.path.starts_with(".jyowo/runtime/exports/memory-"));
    let export_content = std::fs::read_to_string(state.workspace_root().join(&exported.path))
        .expect("memory export file should be readable");
    assert!(export_content.contains("Prefers terse Chinese responses"));

    let deleted = delete_memory_item_with_runtime_state(
        DeleteMemoryItemRequest {
            id: visible.id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(deleted.status, "deleted");

    let list_after_delete = list_memory_items_with_runtime_state(&state).await.unwrap();
    assert!(list_after_delete.items.is_empty());
}

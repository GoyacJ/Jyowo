use super::*;

#[tokio::test]
async fn authenticated_memory_requests_are_routed_to_the_daemon_memory_service() {
    let root = tempfile::tempdir().unwrap();
    let config_root = root.path().join("home/config");
    let workspace = root.path().join("workspace");
    std::fs::create_dir_all(&config_root).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let memory = Arc::new(MemoryService::new(RuntimeConfigResolver::new(config_root)));
    let mut connection =
        IpcConnection::new(store, config()).with_memory_service(Arc::clone(&memory));
    connection.handle(handshake("token-a")).unwrap();

    let response = connection
        .handle_async(frame(
            "list-memory",
            ClientRequest::ListMemoryItems {
                workspace_root: Some(workspace.to_string_lossy().into_owned()),
            },
        ))
        .await
        .unwrap();

    assert!(matches!(
        &response[0].message,
        ServerMessage::MemoryItems(items) if items.items.is_empty()
    ));
}

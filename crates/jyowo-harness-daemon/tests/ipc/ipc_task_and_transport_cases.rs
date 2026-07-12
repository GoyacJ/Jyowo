use super::*;

#[tokio::test]
async fn task_metadata_commands_reject_stale_versions_and_running_removal() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let factory = Arc::new(ControlledRunFactory::default());
    let supervisor = Arc::new(
        Supervisor::start(Arc::clone(&store), factory, SupervisorQuotas::new(2, 2)).unwrap(),
    );
    let mut connection = IpcConnection::with_supervisor(Arc::clone(&store), config(), supervisor);
    connection.handle(handshake("token-a")).unwrap();
    let created = connection
        .handle(create("create-running", CommandId::new(), "create-running"))
        .unwrap();
    let task_id = match &created[0].message {
        ServerMessage::CommandAccepted(accepted) => accepted.task_id,
        other => panic!("unexpected {other:?}"),
    };

    let stale = connection
        .handle_async(frame(
            "stale-pin",
            ClientRequest::SetTaskPinned(harness_contracts::SetTaskPinnedCommand {
                metadata: CommandMetadata {
                    command_id: CommandId::new(),
                    idempotency_key: "stale-pin".into(),
                    expected_stream_version: 0,
                },
                task_id,
                pinned: true,
            }),
        ))
        .await
        .unwrap();
    assert!(matches!(
        &stale[0].message,
        ServerMessage::CommandRejected(rejected)
            if rejected.reason == CommandRejectionReason::WrongExpectedVersion
    ));

    let empty_title_command_id = CommandId::new();
    let empty_title = connection
        .handle_async(frame(
            "empty-title",
            ClientRequest::RenameTask(harness_contracts::RenameTaskCommand {
                metadata: CommandMetadata {
                    command_id: empty_title_command_id,
                    idempotency_key: "empty-title".into(),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                },
                task_id,
                title: "   ".into(),
            }),
        ))
        .await
        .unwrap();
    assert!(matches!(
        &empty_title[0].message,
        ServerMessage::CommandRejected(rejected)
            if rejected.reason == CommandRejectionReason::InvalidCommand
    ));
    let reused_empty_title_identity = connection
        .handle_async(frame(
            "reuse-empty-title-identity",
            ClientRequest::RenameTask(harness_contracts::RenameTaskCommand {
                metadata: CommandMetadata {
                    command_id: empty_title_command_id,
                    idempotency_key: "empty-title".into(),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                },
                task_id,
                title: "Must not be accepted".into(),
            }),
        ))
        .await
        .unwrap();
    assert!(matches!(
        &reused_empty_title_identity[0].message,
        ServerMessage::CommandRejected(rejected)
            if rejected.reason == CommandRejectionReason::InvalidCommand
    ));
    assert_eq!(
        store.task_projection(task_id).unwrap().unwrap().title,
        "task"
    );

    let submit_version = store.stream_version(task_id).unwrap();
    connection
        .handle_async(frame(
            "submit-before-remove",
            ClientRequest::SubmitMessage(harness_contracts::SubmitMessageCommand {
                metadata: CommandMetadata {
                    command_id: CommandId::new(),
                    idempotency_key: "submit-before-remove".into(),
                    expected_stream_version: submit_version,
                },
                task_id,
                content: "keep running".into(),
                attachments: Vec::new(),
                context_references: Vec::new(),
                model_config_id: None,
                permission_mode: harness_contracts::PermissionMode::Default,
            }),
        ))
        .await
        .unwrap();
    let remove = connection
        .handle_async(frame(
            "remove-running",
            ClientRequest::RemoveTask(harness_contracts::RemoveTaskCommand {
                metadata: CommandMetadata {
                    command_id: CommandId::new(),
                    idempotency_key: "remove-running".into(),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                },
                task_id,
            }),
        ))
        .await
        .unwrap();
    assert!(matches!(
        &remove[0].message,
        ServerMessage::CommandRejected(rejected)
            if rejected.reason == CommandRejectionReason::InvalidCommand
    ));
    assert!(!store.task_projection(task_id).unwrap().unwrap().removed);
}

#[tokio::test]
async fn task_removal_rejects_a_nonempty_queue() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let supervisor = Arc::new(
        Supervisor::start(
            Arc::clone(&store),
            Arc::new(IdleRunFactory),
            SupervisorQuotas::new(2, 2),
        )
        .unwrap(),
    );
    let mut connection = IpcConnection::with_supervisor(Arc::clone(&store), config(), supervisor);
    connection.handle(handshake("token-a")).unwrap();
    let created = connection
        .handle(create("create-queued", CommandId::new(), "create-queued"))
        .unwrap();
    let task_id = match &created[0].message {
        ServerMessage::CommandAccepted(accepted) => accepted.task_id,
        other => panic!("unexpected {other:?}"),
    };
    store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: "queue-before-remove".into(),
                expected_stream_version: 1,
                authority: TaskStore::supervisor_authority(),
                payload: json!({ "type": "queue_before_remove" }),
            },
            |_| {
                Ok(vec![NewTaskEvent::message_queued(
                    QueueItemId::new(),
                    "queued",
                    Vec::new(),
                    Vec::new(),
                    now(),
                )])
            },
        )
        .unwrap();

    let response = connection
        .handle_async(frame(
            "remove-queued",
            ClientRequest::RemoveTask(harness_contracts::RemoveTaskCommand {
                metadata: CommandMetadata {
                    command_id: CommandId::new(),
                    idempotency_key: "remove-queued".into(),
                    expected_stream_version: store.stream_version(task_id).unwrap(),
                },
                task_id,
            }),
        ))
        .await
        .unwrap();

    assert!(matches!(
        &response[0].message,
        ServerMessage::CommandRejected(rejected)
            if rejected.reason == CommandRejectionReason::InvalidCommand
    ));
    assert!(!store.task_projection(task_id).unwrap().unwrap().removed);
}

#[test]
fn read_blob_returns_owned_bytes_and_metadata() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let blob_root = root.path().join("blobs");
    let mut ipc_config = config();
    ipc_config.blob_root.clone_from(&blob_root);
    let mut connection = IpcConnection::new(Arc::clone(&store), ipc_config);
    connection.handle(handshake("token-a")).unwrap();
    let created = connection
        .handle(create("create", CommandId::new(), "create-blob-task"))
        .unwrap();
    let task_id = match &created[0].message {
        ServerMessage::CommandAccepted(accepted) => accepted.task_id,
        other => panic!("unexpected {other:?}"),
    };
    let blobs = TaskBlobStore::open(Arc::clone(&store), task_id, blob_root).unwrap();
    let blob = blobs.put("text/plain", b"abc").unwrap();
    store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: "attach-ipc-blob".into(),
                expected_stream_version: 1,
                authority: TaskStore::user_authority(ClientId::new()),
                payload: json!({ "type": "attach_ipc_blob" }),
            },
            |_| {
                Ok(vec![NewTaskEvent::message_queued(
                    QueueItemId::new(),
                    "blob",
                    vec![blob.id],
                    Vec::new(),
                    now(),
                )])
            },
        )
        .unwrap();

    let response = connection
        .handle(frame(
            "read-blob",
            ClientRequest::ReadBlob { blob_id: blob.id },
        ))
        .unwrap();
    match &response[0].message {
        ServerMessage::Blob(payload) => {
            assert_eq!(payload.blob_id, blob.id);
            assert_eq!(payload.media_type, "text/plain");
            assert_eq!(payload.size, 3);
            assert_eq!(payload.base64_data.as_deref(), Some("YWJj"));
            assert!(!payload.missing);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn largest_task_blob_encodes_within_one_daemon_frame() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let blob_root = root.path().join("blobs");
    let mut ipc_config = config();
    ipc_config.blob_root.clone_from(&blob_root);
    let mut connection = IpcConnection::new(Arc::clone(&store), ipc_config);
    connection.handle(handshake("token-a")).unwrap();
    let created = connection
        .handle(create("create", CommandId::new(), "create-max-blob-task"))
        .unwrap();
    let task_id = match &created[0].message {
        ServerMessage::CommandAccepted(accepted) => accepted.task_id,
        other => panic!("unexpected {other:?}"),
    };
    let blobs = TaskBlobStore::open(Arc::clone(&store), task_id, blob_root).unwrap();
    let media_type = format!("application/{}", "a".repeat(243));
    assert_eq!(media_type.len(), 255);
    let blob = blobs
        .put(&media_type, &vec![0_u8; MAX_DAEMON_BLOB_BYTES])
        .unwrap();
    store
        .transact_command(
            AcceptedCommand {
                command_id: CommandId::new(),
                task_id,
                idempotency_key: "attach-max-ipc-blob".into(),
                expected_stream_version: 1,
                authority: TaskStore::user_authority(ClientId::new()),
                payload: json!({ "type": "attach_max_ipc_blob" }),
            },
            |_| {
                Ok(vec![NewTaskEvent::message_queued(
                    QueueItemId::new(),
                    "blob",
                    vec![blob.id],
                    Vec::new(),
                    now(),
                )])
            },
        )
        .unwrap();

    let response = connection
        .handle(frame(
            &"r".repeat(128),
            ClientRequest::ReadBlob { blob_id: blob.id },
        ))
        .unwrap();
    let encoded = encode_frame(&response[0]).unwrap();

    assert!(encoded.len() <= MAX_FRAME_BYTES + 4);
}

#[cfg(unix)]
#[tokio::test]
async fn unix_transport_is_owner_only_and_serves_framed_requests() {
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let socket = root.path().join("daemon.sock");
    let server = LocalIpcServer::bind_unix(&socket, store, config())
        .await
        .unwrap();
    assert_eq!(
        std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let mut stream = UnixStream::connect(&socket).await.unwrap();
    stream
        .write_all(&encode_frame(&handshake("token-a")).unwrap())
        .await
        .unwrap();
    let mut header = [0_u8; 4];
    stream.read_exact(&mut header).await.unwrap();
    let length = u32::from_be_bytes(header) as usize;
    let mut body = vec![0; length];
    stream.read_exact(&mut body).await.unwrap();
    let response: harness_contracts::ServerFrame = serde_json::from_slice(&body).unwrap();
    assert!(matches!(response.message, ServerMessage::Handshake(_)));
    server.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn unix_subscription_pushes_committed_events_without_another_request() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    async fn send(stream: &mut UnixStream, frame: &ClientFrame) {
        stream
            .write_all(&encode_frame(frame).unwrap())
            .await
            .unwrap();
    }

    async fn receive(stream: &mut UnixStream) -> harness_contracts::ServerFrame {
        let mut header = [0_u8; 4];
        stream.read_exact(&mut header).await.unwrap();
        let mut body = vec![0; u32::from_be_bytes(header) as usize];
        stream.read_exact(&mut body).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let socket = root.path().join("daemon.sock");
    let server = LocalIpcServer::bind_unix(&socket, store, config())
        .await
        .unwrap();

    let mut subscriber = UnixStream::connect(&socket).await.unwrap();
    send(&mut subscriber, &handshake("token-a")).await;
    receive(&mut subscriber).await;
    send(
        &mut subscriber,
        &frame(
            "subscribe",
            ClientRequest::SubscribeEvents { after_offset: 0 },
        ),
    )
    .await;
    receive(&mut subscriber).await;

    let mut writer = UnixStream::connect(&socket).await.unwrap();
    send(&mut writer, &handshake("token-a")).await;
    receive(&mut writer).await;
    send(
        &mut writer,
        &create("create", CommandId::new(), "push-create"),
    )
    .await;
    receive(&mut writer).await;

    let pushed = tokio::time::timeout(std::time::Duration::from_secs(1), receive(&mut subscriber))
        .await
        .expect("subscribed client receives a pushed event batch");
    assert!(matches!(
        pushed.message,
        ServerMessage::EventBatch(batch) if !batch.gap && batch.events.len() == 1
    ));
    server.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn server_shutdown_closes_existing_clients() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let socket = root.path().join("daemon.sock");
    let server = LocalIpcServer::bind_unix(&socket, store, config())
        .await
        .unwrap();
    let mut stream = UnixStream::connect(&socket).await.unwrap();
    stream
        .write_all(&encode_frame(&handshake("token-a")).unwrap())
        .await
        .unwrap();
    let mut header = [0_u8; 4];
    stream.read_exact(&mut header).await.unwrap();
    let mut body = vec![0; u32::from_be_bytes(header) as usize];
    stream.read_exact(&mut body).await.unwrap();

    server.shutdown().await.unwrap();
    assert_eq!(stream.read(&mut [0_u8; 1]).await.unwrap(), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn shutdown_does_not_remove_a_replaced_endpoint() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let socket = root.path().join("daemon.sock");
    let server = LocalIpcServer::bind_unix(&socket, store, config())
        .await
        .unwrap();
    std::fs::remove_file(&socket).unwrap();
    std::fs::write(&socket, b"replacement").unwrap();

    server.shutdown().await.unwrap();
    assert_eq!(std::fs::read(&socket).unwrap(), b"replacement");
}

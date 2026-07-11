use std::sync::Arc;

use harness_contracts::{
    now, ClientFrame, ClientId, ClientRequest, CommandId, CommandMetadata, CreateTaskCommand,
    HandshakeRequest, QueueItemId, ServerMessage, WorkspaceMode, WorkspaceSelection,
    PROTOCOL_VERSION,
};
use harness_daemon::{
    encode_frame, IpcConnection, IpcServerConfig, JsonFrameDecoder, LocalIpcServer, MAX_FRAME_BYTES,
};
use harness_journal::{AcceptedCommand, NewTaskEvent, TaskBlobStore, TaskStore};
use serde_json::json;

fn config() -> IpcServerConfig {
    IpcServerConfig {
        daemon_version: "0.1.0".into(),
        user_instance_id: "user-a".into(),
        connection_token: "token-a".into(),
        event_batch_capacity: 2,
        blob_root: std::env::temp_dir().join("jyowo-daemon-ipc-unused-blobs"),
    }
}

fn frame(request_id: &str, request: ClientRequest) -> ClientFrame {
    ClientFrame {
        request_id: request_id.into(),
        protocol_version: PROTOCOL_VERSION,
        request,
    }
}

fn handshake(token: &str) -> ClientFrame {
    frame(
        "handshake",
        ClientRequest::Handshake(HandshakeRequest {
            client_id: ClientId::new(),
            client_version: "0.1.0".into(),
            user_instance_id: "user-a".into(),
            connection_token: token.into(),
            last_acknowledged_offset: 0,
        }),
    )
}

fn create(request_id: &str, command_id: CommandId, key: &str) -> ClientFrame {
    frame(
        request_id,
        ClientRequest::CreateTask(CreateTaskCommand {
            metadata: CommandMetadata {
                command_id,
                idempotency_key: key.into(),
                expected_stream_version: 0,
            },
            title: "task".into(),
            workspace: WorkspaceSelection {
                mode: WorkspaceMode::Current,
                root: "/tmp/workspace".into(),
            },
        }),
    )
}

#[test]
fn codec_handles_fragmented_and_coalesced_frames_and_rejects_bad_lengths() {
    let first = encode_frame(&handshake("token-a")).unwrap();
    let second = encode_frame(&frame(
        "subscribe",
        ClientRequest::SubscribeEvents { after_offset: 0 },
    ))
    .unwrap();
    let mut decoder = JsonFrameDecoder::new();
    assert!(decoder.push::<ClientFrame>(&first[..3]).unwrap().is_empty());
    let mut tail = first[3..].to_vec();
    tail.extend_from_slice(&second);
    let decoded = decoder.push::<ClientFrame>(&tail).unwrap();
    assert_eq!(decoded.len(), 2);

    assert!(JsonFrameDecoder::new()
        .push::<ClientFrame>(&[0, 0, 0, 0])
        .is_err());
    let oversized = u32::try_from(MAX_FRAME_BYTES + 1).unwrap().to_be_bytes();
    assert!(JsonFrameDecoder::new()
        .push::<ClientFrame>(&oversized)
        .is_err());
    let invalid_json = [vec![0, 0, 0, 1], vec![b'{']].concat();
    assert!(JsonFrameDecoder::new()
        .push::<ClientFrame>(&invalid_json)
        .is_err());
}

#[test]
fn handshake_rejects_protocol_token_and_instance_mismatches() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let mut connection = IpcConnection::new(Arc::clone(&store), config());
    let mut wrong_version = handshake("token-a");
    wrong_version.protocol_version += 1;
    assert!(matches!(
        connection.handle(wrong_version).unwrap()[0].message,
        ServerMessage::Error(_)
    ));

    let mut connection = IpcConnection::new(Arc::clone(&store), config());
    assert!(matches!(
        connection.handle(handshake("wrong")).unwrap()[0].message,
        ServerMessage::Error(_)
    ));

    let mut connection = IpcConnection::new(store, config());
    let mut wrong_instance = handshake("token-a");
    if let ClientRequest::Handshake(request) = &mut wrong_instance.request {
        request.user_instance_id = "other".into();
    }
    assert!(matches!(
        connection.handle(wrong_instance).unwrap()[0].message,
        ServerMessage::Error(_)
    ));

    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let mut connection = IpcConnection::new(store, config());
    let mut malformed_version = handshake("token-a");
    if let ClientRequest::Handshake(request) = &mut malformed_version.request {
        request.client_version = "0.invalid".into();
    }
    assert!(matches!(
        connection.handle(malformed_version).unwrap()[0].message,
        ServerMessage::Error(_)
    ));

    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let mut connection = IpcConnection::new(store, config());
    let mut future_offset = handshake("token-a");
    if let ClientRequest::Handshake(request) = &mut future_offset.request {
        request.last_acknowledged_offset = 1;
    }
    assert!(matches!(
        connection.handle(future_offset).unwrap()[0].message,
        ServerMessage::Error(_)
    ));
}

#[test]
fn duplicate_commands_are_idempotent_and_clients_observe_identical_offsets() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let mut first = IpcConnection::new(Arc::clone(&store), config());
    let mut second = IpcConnection::new(Arc::clone(&store), config());
    first.handle(handshake("token-a")).unwrap();
    second.handle(handshake("token-a")).unwrap();

    let command_id = CommandId::new();
    let accepted = first
        .handle(create("create-1", command_id, "same-create"))
        .unwrap();
    let replayed = first
        .handle(create("create-2", command_id, "same-create"))
        .unwrap();
    assert!(matches!(
        accepted[0].message,
        ServerMessage::CommandAccepted(_)
    ));
    assert!(matches!(
        replayed[0].message,
        ServerMessage::CommandAccepted(_)
    ));
    assert_eq!(store.latest_global_offset().unwrap(), 1);

    let mut conflicting = create("create-3", command_id, "same-create");
    if let ClientRequest::CreateTask(command) = &mut conflicting.request {
        command.title = "different".into();
    }
    assert!(matches!(
        first.handle(conflicting).unwrap()[0].message,
        ServerMessage::CommandRejected(_)
    ));
    assert_eq!(store.latest_global_offset().unwrap(), 1);

    let a = first
        .handle(frame(
            "events-a",
            ClientRequest::SubscribeEvents { after_offset: 0 },
        ))
        .unwrap();
    let b = second
        .handle(frame(
            "events-b",
            ClientRequest::SubscribeEvents { after_offset: 0 },
        ))
        .unwrap();
    let offsets = |frames: Vec<harness_contracts::ServerFrame>| match &frames[0].message {
        ServerMessage::EventBatch(batch) => batch
            .events
            .iter()
            .map(|event| event.global_offset)
            .collect::<Vec<_>>(),
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(offsets(a), offsets(b));

    for index in 0..3 {
        first
            .handle(create(
                &format!("extra-{index}"),
                CommandId::new(),
                &format!("extra-{index}"),
            ))
            .unwrap();
    }
    let gap = first
        .handle(frame(
            "slow",
            ClientRequest::SubscribeEvents { after_offset: 0 },
        ))
        .unwrap();
    assert!(matches!(
        &gap[0].message,
        ServerMessage::EventBatch(batch) if batch.gap && batch.events.is_empty()
    ));
}

#[test]
fn task_snapshot_uses_the_global_cursor_even_when_other_tasks_advanced_it() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let mut connection = IpcConnection::new(store, config());
    connection.handle(handshake("token-a")).unwrap();

    let first = connection
        .handle(create("create-a", CommandId::new(), "create-a"))
        .unwrap();
    let task_id = match &first[0].message {
        ServerMessage::CommandAccepted(accepted) => accepted.task_id,
        other => panic!("unexpected {other:?}"),
    };
    connection
        .handle(create("create-b", CommandId::new(), "create-b"))
        .unwrap();

    let loaded = connection
        .handle(frame("load-a", ClientRequest::LoadTask { task_id }))
        .unwrap();
    match &loaded[0].message {
        ServerMessage::TaskSnapshot(snapshot) => {
            assert_eq!(snapshot.projection.last_global_offset, 1);
            assert_eq!(snapshot.snapshot_offset, 2);
            assert_eq!(
                snapshot
                    .timeline
                    .iter()
                    .map(|item| item.global_offset)
                    .collect::<Vec<_>>(),
                vec![1]
            );
        }
        other => panic!("unexpected {other:?}"),
    }
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

use std::sync::Arc;

use harness_contracts::{
    now, BlobId, ClientId, CommandId, QueueItemId, RuntimeCommand, RuntimeSessionStatus,
    RuntimeSpec, RuntimeView, TaskId,
};
use harness_daemon::{BrowserService, RuntimeService, RuntimeServiceError};
use harness_journal::{AcceptedCommand, NewTaskEvent, TaskBlobStore, TaskStore};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn html_runtime_serves_owned_content_with_restrictive_headers_and_stops() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let task_id = TaskId::new();
    create_task(&store, task_id);
    let blob_root = root.path().join("blobs");
    let blobs = TaskBlobStore::open(Arc::clone(&store), task_id, &blob_root).unwrap();
    let blob = blobs
        .put(
            "text/html",
            b"<!doctype html><title>Preview</title><h1>Runtime preview</h1>",
        )
        .unwrap();
    attach_blob(&store, task_id, 1, blob.id);
    let service = RuntimeService::new(
        Arc::clone(&store),
        blob_root,
        Arc::new(BrowserService::unavailable("not needed")),
    );

    let opened = service
        .handle(
            task_id,
            RuntimeCommand::Open {
                spec: RuntimeSpec::Html {
                    blob_id: blob.id,
                    title: "Preview".to_owned(),
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(opened.status, RuntimeSessionStatus::Ready);
    let RuntimeView::Url { url } = opened.view.expect("runtime URL") else {
        panic!("expected URL runtime view");
    };

    let response = get(&url).await;
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response
        .to_ascii_lowercase()
        .contains("content-security-policy: default-src 'none'"));
    assert!(response.contains("<h1>Runtime preview</h1>"));

    let status = service
        .handle(
            task_id,
            RuntimeCommand::Status {
                session_id: opened.session_id.clone(),
                kind: opened.kind,
            },
        )
        .await
        .unwrap();
    assert_eq!(status.status, RuntimeSessionStatus::Ready);

    let stopped = service
        .handle(
            task_id,
            RuntimeCommand::Close {
                session_id: opened.session_id,
                kind: opened.kind,
            },
        )
        .await
        .unwrap();
    assert_eq!(stopped.status, RuntimeSessionStatus::Stopped);
    assert!(get(&url).await.starts_with("HTTP/1.1 404 Not Found"));
    service.shutdown().await;
}

#[tokio::test]
async fn html_runtime_rejects_non_html_and_foreign_blobs() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
    let owner = TaskId::new();
    let foreign = TaskId::new();
    create_task(&store, owner);
    create_task(&store, foreign);
    let blob_root = root.path().join("blobs");
    let blobs = TaskBlobStore::open(Arc::clone(&store), owner, &blob_root).unwrap();
    let text = blobs.put("text/plain", b"not html").unwrap();
    let html = blobs.put("text/html", b"<h1>private</h1>").unwrap();
    attach_blob(&store, owner, 1, text.id);
    attach_blob(&store, owner, 2, html.id);
    let service = RuntimeService::new(
        Arc::clone(&store),
        blob_root,
        Arc::new(BrowserService::unavailable("not needed")),
    );

    let non_html = open_html(&service, owner, text.id).await;
    assert!(matches!(
        non_html,
        Err(RuntimeServiceError::InvalidInput(_))
    ));
    let foreign_blob = open_html(&service, foreign, html.id).await;
    assert!(matches!(foreign_blob, Err(RuntimeServiceError::Store(_))));
}

async fn open_html(
    service: &RuntimeService,
    task_id: TaskId,
    blob_id: BlobId,
) -> Result<harness_contracts::RuntimeSessionState, RuntimeServiceError> {
    service
        .handle(
            task_id,
            RuntimeCommand::Open {
                spec: RuntimeSpec::Html {
                    blob_id,
                    title: "Preview".to_owned(),
                },
            },
        )
        .await
}

async fn get(url: &str) -> String {
    let target = url.strip_prefix("http://").expect("loopback HTTP URL");
    let (authority, path) = target.split_once('/').expect("runtime URL path");
    let mut stream = tokio::net::TcpStream::connect(authority).await.unwrap();
    stream
        .write_all(
            format!("GET /{path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.unwrap();
    String::from_utf8(bytes).unwrap()
}

fn create_task(store: &TaskStore, task_id: TaskId) {
    store
        .transact_command(command(task_id, 0), |_| {
            Ok(vec![NewTaskEvent::task_created("Runtime")])
        })
        .unwrap();
}

fn attach_blob(store: &TaskStore, task_id: TaskId, expected: u64, blob_id: BlobId) {
    store
        .transact_command(command(task_id, expected), |_| {
            Ok(vec![NewTaskEvent::message_queued(
                QueueItemId::new(),
                "runtime blob",
                vec![blob_id],
                Vec::new(),
                now(),
            )])
        })
        .unwrap();
}

fn command(task_id: TaskId, expected_stream_version: u64) -> AcceptedCommand {
    AcceptedCommand {
        command_id: CommandId::new(),
        task_id,
        idempotency_key: format!("runtime-{}", CommandId::new()),
        expected_stream_version,
        authority: TaskStore::user_authority(ClientId::new()),
        payload: json!({ "type": "runtime_test" }),
    }
}

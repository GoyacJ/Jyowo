//! Thin, authority-free client for the local task daemon.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use harness_contracts::{
    BlobId, ClientFrame, ClientId, ClientRequest, HandshakeRequest, ServerFrame, ServerMessage,
    PROTOCOL_VERSION,
};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::sync::{mpsc, watch};

const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOKEN_BYTES: u64 = 4096;

#[cfg(unix)]
type LocalStream = tokio::net::UnixStream;
#[cfg(windows)]
type LocalStream = tokio::net::windows::named_pipe::NamedPipeClient;

#[derive(Debug, Clone)]
pub struct DaemonClientConfig {
    pub endpoint: PathBuf,
    pub token_path: PathBuf,
    pub user_instance_id: String,
    pub client_version: String,
}

#[derive(Debug, Error)]
pub enum DaemonClientError {
    #[error("daemon protocol version mismatch")]
    ProtocolMismatch,
    #[error("handshake frames are owned by the bridge")]
    HandshakeNotAllowed,
    #[error("event subscriptions require the dedicated bridge command")]
    StreamingRequestNotAllowed,
    #[error("daemon connection token is invalid")]
    InvalidToken,
    #[error("daemon returned an unexpected handshake response")]
    InvalidHandshake,
    #[error("daemon frame exceeds the 8 MiB limit")]
    FrameTooLarge,
    #[error("daemon disconnected")]
    Disconnected,
    #[error("daemon I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("daemon frame is invalid: {0}")]
    Json(#[from] serde_json::Error),
}

struct DaemonClientInner {
    config: DaemonClientConfig,
    client_id: ClientId,
    request_sequence: AtomicU64,
    connection: Mutex<Option<LocalStream>>,
}

#[derive(Clone)]
pub struct DaemonClient {
    inner: Arc<DaemonClientInner>,
}

pub struct DaemonSubscription {
    events: mpsc::Receiver<ServerFrame>,
    cancel: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl DaemonSubscription {
    pub async fn recv(&mut self) -> Option<ServerFrame> {
        self.events.recv().await
    }
}

impl Drop for DaemonSubscription {
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
        self.task.abort();
    }
}

impl DaemonClient {
    #[must_use]
    pub fn new(config: DaemonClientConfig) -> Self {
        Self {
            inner: Arc::new(DaemonClientInner {
                config,
                client_id: ClientId::new(),
                request_sequence: AtomicU64::new(1),
                connection: Mutex::new(None),
            }),
        }
    }

    pub async fn request(&self, request: ClientRequest) -> Result<ServerFrame, DaemonClientError> {
        if matches!(request, ClientRequest::Handshake(_)) {
            return Err(DaemonClientError::HandshakeNotAllowed);
        }
        let sequence = self.inner.request_sequence.fetch_add(1, Ordering::Relaxed);
        let frame = ClientFrame {
            request_id: format!("bridge-{}-{sequence}", self.inner.client_id),
            protocol_version: PROTOCOL_VERSION,
            request,
        };
        self.send_frame(frame).await
    }

    pub async fn send_frame(&self, frame: ClientFrame) -> Result<ServerFrame, DaemonClientError> {
        if frame.protocol_version != PROTOCOL_VERSION {
            return Err(DaemonClientError::ProtocolMismatch);
        }
        if matches!(frame.request, ClientRequest::Handshake(_)) {
            return Err(DaemonClientError::HandshakeNotAllowed);
        }
        if matches!(frame.request, ClientRequest::SubscribeEvents { .. }) {
            return Err(DaemonClientError::StreamingRequestNotAllowed);
        }
        let mut last_error = None;
        for _ in 0..2 {
            let mut connection = self.inner.connection.lock().await;
            if connection.is_none() {
                match connect_and_handshake(&self.inner.config, self.inner.client_id).await {
                    Ok(stream) => *connection = Some(stream),
                    Err(error) => {
                        last_error = Some(error);
                        continue;
                    }
                }
            }
            let result =
                request_on_connection(connection.as_mut().expect("connection initialized"), &frame)
                    .await;
            match result {
                Ok(response) => return Ok(response),
                Err(error) => {
                    *connection = None;
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap_or(DaemonClientError::Disconnected))
    }

    pub async fn read_blob(&self, blob_id: BlobId) -> Result<ServerFrame, DaemonClientError> {
        self.request(ClientRequest::ReadBlob { blob_id }).await
    }

    #[must_use]
    pub fn subscribe(&self, after_offset: u64) -> DaemonSubscription {
        let config = self.inner.config.clone();
        let client_id = ClientId::new();
        let (events_tx, events) = mpsc::channel(32);
        let (cancel, mut cancelled) = watch::channel(false);
        let task = tokio::spawn(async move {
            let mut cursor = after_offset;
            loop {
                if *cancelled.borrow() {
                    break;
                }
                let mut stream = match connect_and_handshake(&config, client_id).await {
                    Ok(stream) => stream,
                    Err(_) => {
                        if wait_or_cancel(&mut cancelled).await {
                            break;
                        }
                        continue;
                    }
                };
                let request = ClientFrame {
                    request_id: format!("subscribe-{client_id}-{cursor}"),
                    protocol_version: PROTOCOL_VERSION,
                    request: ClientRequest::SubscribeEvents {
                        after_offset: cursor,
                    },
                };
                if write_frame(&mut stream, &request).await.is_err() {
                    continue;
                }
                loop {
                    tokio::select! {
                        changed = cancelled.changed() => {
                            if changed.is_err() || *cancelled.borrow() {
                                return;
                            }
                        }
                        frame = read_frame::<ServerFrame>(&mut stream) => {
                            let Ok(frame) = frame else { break; };
                            if frame.protocol_version != PROTOCOL_VERSION {
                                return;
                            }
                            if let ServerMessage::EventBatch(batch) = &frame.message {
                                if batch.gap {
                                    cursor = batch.latest_offset;
                                } else if let Some(last) = batch.events.last() {
                                    cursor = last.global_offset;
                                } else {
                                    cursor = cursor.max(batch.latest_offset);
                                }
                            }
                            if events_tx.send(frame).await.is_err() {
                                return;
                            }
                        }
                    }
                }
                if wait_or_cancel(&mut cancelled).await {
                    break;
                }
            }
        });
        DaemonSubscription {
            events,
            cancel,
            task,
        }
    }
}

async fn wait_or_cancel(cancelled: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => false,
        changed = cancelled.changed() => changed.is_err() || *cancelled.borrow(),
    }
}

async fn request_on_connection(
    stream: &mut LocalStream,
    frame: &ClientFrame,
) -> Result<ServerFrame, DaemonClientError> {
    write_frame(stream, frame).await?;
    loop {
        let response: ServerFrame = read_frame(stream).await?;
        if response.protocol_version != PROTOCOL_VERSION {
            return Err(DaemonClientError::ProtocolMismatch);
        }
        if response.request_id.as_deref() == Some(frame.request_id.as_str()) {
            return Ok(response);
        }
        if response.request_id.is_none() && matches!(response.message, ServerMessage::Error(_)) {
            return Ok(response);
        }
    }
}

async fn connect_and_handshake(
    config: &DaemonClientConfig,
    client_id: ClientId,
) -> Result<LocalStream, DaemonClientError> {
    let token = read_token(&config.token_path).await?;
    let mut stream = connect_local(&config.endpoint).await?;
    let frame = ClientFrame {
        request_id: format!("handshake-{client_id}"),
        protocol_version: PROTOCOL_VERSION,
        request: ClientRequest::Handshake(HandshakeRequest {
            client_id,
            client_version: config.client_version.clone(),
            user_instance_id: config.user_instance_id.clone(),
            connection_token: token,
            last_acknowledged_offset: 0,
        }),
    };
    write_frame(&mut stream, &frame).await?;
    let response: ServerFrame = read_frame(&mut stream).await?;
    if response.protocol_version != PROTOCOL_VERSION
        || response.request_id.as_deref() != Some(frame.request_id.as_str())
        || !matches!(response.message, ServerMessage::Handshake(_))
    {
        return Err(DaemonClientError::InvalidHandshake);
    }
    Ok(stream)
}

async fn read_token(path: &PathBuf) -> Result<String, DaemonClientError> {
    let path = path.clone();
    tokio::task::spawn_blocking(move || read_token_file(&path))
        .await
        .map_err(|_| DaemonClientError::InvalidToken)?
}

fn read_token_file(path: &PathBuf) -> Result<String, DaemonClientError> {
    use std::io::Read;

    let link_metadata = std::fs::symlink_metadata(path)?;
    if link_metadata.file_type().is_symlink() {
        return Err(DaemonClientError::InvalidToken);
    }

    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let file = options
        .open(path)
        .map_err(|_| DaemonClientError::InvalidToken)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_TOKEN_BYTES {
        return Err(DaemonClientError::InvalidToken);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != rustix::process::getuid().as_raw()
            || metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
        {
            return Err(DaemonClientError::InvalidToken);
        }
    }

    let mut body = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_TOKEN_BYTES + 1).read_to_end(&mut body)?;
    if body.is_empty() || body.len() as u64 != metadata.len() || !body.is_ascii() {
        return Err(DaemonClientError::InvalidToken);
    }
    String::from_utf8(body).map_err(|_| DaemonClientError::InvalidToken)
}

async fn write_frame<T: serde::Serialize>(
    stream: &mut LocalStream,
    value: &T,
) -> Result<(), DaemonClientError> {
    let body = serde_json::to_vec(value)?;
    if body.is_empty() || body.len() > MAX_FRAME_BYTES {
        return Err(DaemonClientError::FrameTooLarge);
    }
    let length = u32::try_from(body.len()).map_err(|_| DaemonClientError::FrameTooLarge)?;
    stream.write_all(&length.to_be_bytes()).await?;
    stream.write_all(&body).await?;
    Ok(())
}

async fn read_frame<T: serde::de::DeserializeOwned>(
    stream: &mut LocalStream,
) -> Result<T, DaemonClientError> {
    let mut header = [0_u8; 4];
    stream.read_exact(&mut header).await?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(DaemonClientError::FrameTooLarge);
    }
    let mut body = vec![0_u8; length];
    stream.read_exact(&mut body).await?;
    Ok(serde_json::from_slice(&body)?)
}

#[cfg(unix)]
async fn connect_local(endpoint: &PathBuf) -> Result<LocalStream, std::io::Error> {
    LocalStream::connect(endpoint).await
}

#[cfg(windows)]
async fn connect_local(endpoint: &PathBuf) -> Result<LocalStream, std::io::Error> {
    tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint)
}

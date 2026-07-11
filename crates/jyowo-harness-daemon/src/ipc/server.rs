use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use base64::Engine as _;
use harness_contracts::{
    BlobPayload, ClientFrame, ClientId, ClientRequest, CommandAccepted, CommandRejected,
    CommandRejectionReason, HandshakeResponse, ProtocolError, ProtocolErrorCode, QueueItemId,
    RunSegmentId, ServerFrame, ServerMessage, TaskEventBatch, TaskId, TaskSnapshot,
    PROTOCOL_VERSION,
};
use harness_journal::{
    AcceptedCommand, BlobRead, CommandOutcome, CommandRejection, NewTaskEvent, TaskBlobStore,
    TaskStore,
};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use super::IpcError;
use crate::{QueueCommand, Supervisor, ValidatedTaskCommand};

#[derive(Debug, Clone)]
pub struct IpcServerConfig {
    pub daemon_version: String,
    pub user_instance_id: String,
    pub connection_token: String,
    pub event_batch_capacity: usize,
    pub blob_root: PathBuf,
}

pub struct IpcConnection {
    store: Arc<TaskStore>,
    config: IpcServerConfig,
    supervisor: Option<Arc<Supervisor>>,
    client_id: Option<ClientId>,
    subscription_offset: Option<u64>,
}

impl IpcConnection {
    #[must_use]
    pub fn new(store: Arc<TaskStore>, config: IpcServerConfig) -> Self {
        Self {
            store,
            config,
            supervisor: None,
            client_id: None,
            subscription_offset: None,
        }
    }

    #[must_use]
    pub fn with_supervisor(
        store: Arc<TaskStore>,
        config: IpcServerConfig,
        supervisor: Arc<Supervisor>,
    ) -> Self {
        Self {
            store,
            config,
            supervisor: Some(supervisor),
            client_id: None,
            subscription_offset: None,
        }
    }

    pub async fn handle_async(&mut self, frame: ClientFrame) -> Result<Vec<ServerFrame>, IpcError> {
        let request_id = frame.request_id.clone();
        let request = frame.request.clone();
        let response = self.handle(frame)?;
        if !requires_task_supervisor(&request) || !is_supervisor_required_response(&response) {
            return Ok(response);
        }
        let Some(supervisor) = self.supervisor.as_ref() else {
            return Ok(response);
        };
        let Some(client_id) = self.client_id else {
            return Ok(response);
        };
        let Some((task_id, command)) = validated_task_command(client_id, request)? else {
            return Ok(response);
        };
        let outcome = supervisor.dispatch(task_id, command).await?;
        Ok(vec![server_frame(
            Some(request_id),
            command_message(outcome),
        )])
    }

    pub fn handle(&mut self, frame: ClientFrame) -> Result<Vec<ServerFrame>, IpcError> {
        if !valid_request_id(&frame.request_id) {
            return Ok(vec![server_frame(
                None,
                protocol_error(
                    ProtocolErrorCode::InvalidFrame,
                    "request ID must be 1-128 printable ASCII characters",
                ),
            )]);
        }
        let request_id = frame.request_id.clone();
        if frame.protocol_version != PROTOCOL_VERSION {
            return Ok(vec![error_frame(
                request_id,
                ProtocolErrorCode::ProtocolMismatch,
                "protocol version mismatch",
            )]);
        }

        if let ClientRequest::Handshake(request) = frame.request {
            if self.client_id.is_some() {
                return Ok(vec![error_frame(
                    request_id,
                    ProtocolErrorCode::InvalidFrame,
                    "handshake already completed",
                )]);
            }
            if !versions_compatible(&request.client_version, &self.config.daemon_version) {
                return Ok(vec![error_frame(
                    request_id,
                    ProtocolErrorCode::ProtocolMismatch,
                    "client and daemon versions are incompatible",
                )]);
            }
            if request.user_instance_id != self.config.user_instance_id
                || !constant_time_eq(
                    request.connection_token.as_bytes(),
                    self.config.connection_token.as_bytes(),
                )
            {
                return Ok(vec![error_frame(
                    request_id,
                    ProtocolErrorCode::AuthenticationFailed,
                    "local daemon authentication failed",
                )]);
            }
            let latest_global_offset = self.store.latest_global_offset()?;
            if request.last_acknowledged_offset > latest_global_offset {
                return Ok(vec![error_frame(
                    request_id,
                    ProtocolErrorCode::InvalidFrame,
                    "acknowledged offset is ahead of the daemon",
                )]);
            }
            self.client_id = Some(request.client_id);
            return Ok(vec![server_frame(
                Some(request_id),
                ServerMessage::Handshake(HandshakeResponse {
                    daemon_version: self.config.daemon_version.clone(),
                    user_instance_id: self.config.user_instance_id.clone(),
                    latest_global_offset,
                }),
            )]);
        }

        let Some(client_id) = self.client_id else {
            return Ok(vec![error_frame(
                request_id,
                ProtocolErrorCode::AuthenticationFailed,
                "handshake required",
            )]);
        };

        let message = match frame.request {
            ClientRequest::CreateTask(command) => {
                let task_id =
                    TaskId::from_u128(u128::from_be_bytes(command.metadata.command_id.as_bytes()));
                let accepted = AcceptedCommand {
                    command_id: command.metadata.command_id,
                    task_id,
                    idempotency_key: command.metadata.idempotency_key.clone(),
                    expected_stream_version: command.metadata.expected_stream_version,
                    authority: TaskStore::user_authority(client_id),
                    payload: serde_json::to_value(&command)?,
                };
                let title = command.title;
                let workspace = command.workspace;
                let outcome = match self.store.transact_command(accepted, move |_| {
                    Ok(vec![NewTaskEvent::task_created_in_workspace(
                        title, workspace,
                    )])
                }) {
                    Ok(outcome) => outcome,
                    Err(harness_journal::TaskStoreError::CommandConflict { .. }) => {
                        CommandOutcome::Rejected {
                            command_id: command.metadata.command_id,
                            task_id,
                            rejection: CommandRejection::InvalidCommand {
                                message: "command identity was reused with different input".into(),
                            },
                        }
                    }
                    Err(error) => return Err(error.into()),
                };
                command_message(outcome)
            }
            ClientRequest::SubscribeEvents { after_offset } => {
                let batch = self.event_batch(after_offset)?;
                self.subscription_offset = (!batch.gap).then_some(batch.latest_offset);
                ServerMessage::EventBatch(batch)
            }
            ClientRequest::ListTasks => ServerMessage::TaskList {
                tasks: self.store.task_projections()?,
            },
            ClientRequest::LoadTask { task_id } => {
                match self.store.task_projection_snapshot(task_id)? {
                    Some((projection, snapshot_offset, timeline)) => {
                        ServerMessage::TaskSnapshot(TaskSnapshot {
                            snapshot_offset,
                            projection,
                            timeline,
                        })
                    }
                    None => protocol_error(ProtocolErrorCode::NotFound, "task not found"),
                }
            }
            ClientRequest::StageBlob(command) => {
                if self.store.task_projection(command.task_id)?.is_none() {
                    protocol_error(ProtocolErrorCode::NotFound, "task not found")
                } else {
                    let estimated_size = command.base64_data.len().saturating_mul(3) / 4;
                    if estimated_size > harness_contracts::MAX_DAEMON_BLOB_BYTES {
                        protocol_error(ProtocolErrorCode::InvalidFrame, "blob is too large")
                    } else {
                        match base64::engine::general_purpose::STANDARD
                            .decode(command.base64_data.as_bytes())
                        {
                            Ok(bytes)
                                if bytes.len() <= harness_contracts::MAX_DAEMON_BLOB_BYTES =>
                            {
                                let blobs = TaskBlobStore::open(
                                    Arc::clone(&self.store),
                                    command.task_id,
                                    &self.config.blob_root,
                                )?;
                                let blob = blobs.put(&command.media_type, &bytes)?;
                                ServerMessage::Blob(BlobPayload {
                                    blob_id: blob.id,
                                    media_type: blob.content_type.unwrap_or(command.media_type),
                                    size: blob.size,
                                    content_hash: blob.content_hash.to_vec(),
                                    base64_data: None,
                                    missing: false,
                                })
                            }
                            Ok(_) => {
                                protocol_error(ProtocolErrorCode::InvalidFrame, "blob is too large")
                            }
                            Err(_) => protocol_error(
                                ProtocolErrorCode::InvalidFrame,
                                "blob body is not valid base64",
                            ),
                        }
                    }
                }
            }
            ClientRequest::ReadBlob { blob_id } => match self.store.blob_owner_task(blob_id)? {
                Some(task_id) => {
                    let blobs = TaskBlobStore::open(
                        Arc::clone(&self.store),
                        task_id,
                        &self.config.blob_root,
                    )?;
                    let (blob, base64_data, missing) = match blobs.read(&blob_id)? {
                        BlobRead::Available { blob, bytes } => (
                            blob,
                            Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
                            false,
                        ),
                        BlobRead::Missing { blob } => (blob, None, true),
                    };
                    ServerMessage::Blob(BlobPayload {
                        blob_id: blob.id,
                        media_type: blob
                            .content_type
                            .unwrap_or_else(|| "application/octet-stream".into()),
                        size: blob.size,
                        content_hash: blob.content_hash.to_vec(),
                        base64_data,
                        missing,
                    })
                }
                None => protocol_error(ProtocolErrorCode::NotFound, "blob not found"),
            },
            ClientRequest::SubmitMessage(_)
            | ClientRequest::EditQueuedMessage(_)
            | ClientRequest::DeleteQueuedMessage(_)
            | ClientRequest::PromoteQueuedMessage(_)
            | ClientRequest::StopRun(_)
            | ClientRequest::ContinueTask(_)
            | ClientRequest::ResolvePermission(_) => protocol_error(
                ProtocolErrorCode::InvalidFrame,
                "command requires the task supervisor",
            ),
            ClientRequest::Handshake(_) => unreachable!("handshake handled above"),
        };
        Ok(vec![server_frame(Some(request_id), message)])
    }

    pub(super) fn poll_subscription(&mut self) -> Result<Option<ServerFrame>, IpcError> {
        let Some(after_offset) = self.subscription_offset else {
            return Ok(None);
        };
        let latest = self.store.latest_global_offset()?;
        if latest == after_offset {
            return Ok(None);
        }
        let batch = self.event_batch(after_offset)?;
        if batch.gap {
            self.subscription_offset = None;
        } else {
            self.subscription_offset = Some(batch.latest_offset);
        }
        Ok(Some(server_frame(None, ServerMessage::EventBatch(batch))))
    }

    fn event_batch(&self, after_offset: u64) -> Result<TaskEventBatch, IpcError> {
        let latest_offset = self.store.latest_global_offset()?;
        let capacity = self.config.event_batch_capacity.clamp(1, 1000);
        let lag = latest_offset.saturating_sub(after_offset);
        let (gap, events) = if lag > capacity as u64 {
            (true, Vec::new())
        } else {
            (false, self.store.events_after(after_offset, capacity)?)
        };
        Ok(TaskEventBatch {
            after_offset,
            latest_offset,
            gap,
            events,
        })
    }
}

fn requires_task_supervisor(request: &ClientRequest) -> bool {
    matches!(
        request,
        ClientRequest::SubmitMessage(_)
            | ClientRequest::EditQueuedMessage(_)
            | ClientRequest::DeleteQueuedMessage(_)
            | ClientRequest::PromoteQueuedMessage(_)
    )
}

fn is_supervisor_required_response(response: &[ServerFrame]) -> bool {
    matches!(
        response,
        [ServerFrame {
            message: ServerMessage::Error(ProtocolError {
                code: ProtocolErrorCode::InvalidFrame,
                message,
            }),
            ..
        }] if message == "command requires the task supervisor"
    )
}

fn validated_task_command(
    client_id: ClientId,
    request: ClientRequest,
) -> Result<Option<(TaskId, ValidatedTaskCommand)>, IpcError> {
    let command = match request {
        ClientRequest::SubmitMessage(request) => {
            let task_id = request.task_id;
            let command_id = request.metadata.command_id;
            let payload = serde_json::to_value(&request)?;
            ValidatedTaskCommand::SubmitMessage {
                command: accepted_command(client_id, task_id, request.metadata, payload),
                queue_item_id: QueueItemId::from_u128(u128::from_be_bytes(command_id.as_bytes())),
                segment_id: RunSegmentId::from_u128(u128::from_be_bytes(command_id.as_bytes())),
                content: request.content,
                attachments: request.attachments,
                context_references: request.context_references,
                model_config_id: request.model_config_id,
                permission_mode: request.permission_mode,
                submitted_at: chrono::Utc::now(),
            }
        }
        ClientRequest::EditQueuedMessage(request) => {
            let task_id = request.task_id;
            let payload = serde_json::to_value(&request)?;
            ValidatedTaskCommand::Queue {
                command: accepted_command(client_id, task_id, request.metadata, payload),
                queue_item_id: request.queue_item_id,
                queue_command: QueueCommand::Edit {
                    expected_revision: request.expected_revision,
                    content: request.content,
                    attachments: request.attachments,
                    context_references: request.context_references,
                },
            }
        }
        ClientRequest::DeleteQueuedMessage(request) => {
            let task_id = request.task_id;
            let payload = serde_json::to_value(&request)?;
            ValidatedTaskCommand::Queue {
                command: accepted_command(client_id, task_id, request.metadata, payload),
                queue_item_id: request.queue_item_id,
                queue_command: QueueCommand::Delete {
                    expected_revision: request.expected_revision,
                },
            }
        }
        ClientRequest::PromoteQueuedMessage(request) => {
            let task_id = request.task_id;
            let payload = serde_json::to_value(&request)?;
            ValidatedTaskCommand::Queue {
                command: accepted_command(client_id, task_id, request.metadata, payload),
                queue_item_id: request.queue_item_id,
                queue_command: QueueCommand::Promote {
                    expected_revision: request.expected_revision,
                    mode: request.mode,
                },
            }
        }
        _ => return Ok(None),
    };
    let task_id = match &command {
        ValidatedTaskCommand::SubmitMessage { command, .. }
        | ValidatedTaskCommand::StartSegment { command, .. }
        | ValidatedTaskCommand::ContinueTask { command, .. }
        | ValidatedTaskCommand::Queue { command, .. } => command.task_id,
    };
    Ok(Some((task_id, command)))
}

fn accepted_command(
    client_id: ClientId,
    task_id: TaskId,
    metadata: harness_contracts::CommandMetadata,
    payload: serde_json::Value,
) -> AcceptedCommand {
    AcceptedCommand {
        command_id: metadata.command_id,
        task_id,
        idempotency_key: metadata.idempotency_key,
        expected_stream_version: metadata.expected_stream_version,
        authority: TaskStore::user_authority(client_id),
        payload,
    }
}

fn valid_request_id(request_id: &str) -> bool {
    !request_id.is_empty()
        && request_id.len() <= harness_contracts::MAX_DAEMON_REQUEST_ID_BYTES
        && request_id
            .bytes()
            .all(|byte| byte == b' ' || byte.is_ascii_graphic())
}

fn versions_compatible(client: &str, daemon: &str) -> bool {
    let Ok(client) = semver::Version::parse(client) else {
        return false;
    };
    let Ok(daemon) = semver::Version::parse(daemon) else {
        return false;
    };
    client.major == daemon.major && (client.major != 0 || client.minor == daemon.minor)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let width = left.len().max(right.len());
    for index in 0..width {
        difference |= usize::from(
            left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0),
        );
    }
    difference == 0
}

fn command_message(outcome: CommandOutcome) -> ServerMessage {
    match outcome {
        CommandOutcome::Accepted {
            command_id,
            task_id,
            stream_version,
            committed_offset,
        } => ServerMessage::CommandAccepted(CommandAccepted {
            command_id,
            task_id,
            stream_version,
            committed_offset,
        }),
        CommandOutcome::Rejected {
            command_id,
            task_id,
            rejection,
        } => {
            let (reason, current_stream_version, latest_queue_item) = match rejection {
                CommandRejection::WrongExpectedVersion { actual, .. } => (
                    CommandRejectionReason::WrongExpectedVersion,
                    Some(actual),
                    None,
                ),
                CommandRejection::StaleQueueRevision { latest } => (
                    CommandRejectionReason::StaleQueueRevision,
                    None,
                    Some(*latest),
                ),
                CommandRejection::InvalidCommand { .. } => {
                    (CommandRejectionReason::InvalidCommand, None, None)
                }
            };
            ServerMessage::CommandRejected(CommandRejected {
                command_id: Some(command_id),
                task_id: Some(task_id),
                reason,
                current_stream_version,
                latest_queue_item,
            })
        }
    }
}

fn protocol_error(code: ProtocolErrorCode, message: &str) -> ServerMessage {
    ServerMessage::Error(ProtocolError {
        code,
        message: message.into(),
    })
}

fn error_frame(request_id: String, code: ProtocolErrorCode, message: &str) -> ServerFrame {
    server_frame(Some(request_id), protocol_error(code, message))
}

fn server_frame(request_id: Option<String>, message: ServerMessage) -> ServerFrame {
    ServerFrame {
        request_id,
        protocol_version: PROTOCOL_VERSION,
        message,
    }
}

pub struct LocalIpcServer {
    pub(super) shutdown: Option<oneshot::Sender<()>>,
    pub(super) join: JoinHandle<Result<(), IpcError>>,
    #[cfg(unix)]
    pub(super) endpoint: Option<EndpointCleanup>,
    pub(super) clients: Arc<AtomicUsize>,
}

impl LocalIpcServer {
    #[must_use]
    pub fn connected_clients(&self) -> usize {
        self.clients.load(Ordering::Acquire)
    }

    pub async fn shutdown(mut self) -> Result<(), IpcError> {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.join.await??;
        #[cfg(unix)]
        {
            if let Some(endpoint) = self.endpoint {
                endpoint.remove_if_same_socket()?;
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
pub(super) struct EndpointCleanup {
    path: PathBuf,
    device: u64,
    inode: u64,
}

#[cfg(unix)]
impl EndpointCleanup {
    pub(super) fn unix(path: PathBuf, metadata: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }

    fn remove_if_same_socket(self) -> Result<(), IpcError> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        let metadata = match std::fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_socket()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
        {
            std::fs::remove_file(self.path)?;
        }
        Ok(())
    }
}

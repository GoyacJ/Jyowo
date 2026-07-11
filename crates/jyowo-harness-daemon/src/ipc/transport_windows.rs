//! Windows Named Pipe transport. This module intentionally has no TCP fallback.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use harness_contracts::ClientFrame;
use harness_journal::TaskStore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::oneshot;

use super::{
    encode_frame, IpcConnection, IpcError, IpcServerConfig, LocalIpcServer, MAX_FRAME_BYTES,
};

impl LocalIpcServer {
    pub async fn bind_named_pipe(
        pipe_name: impl Into<String>,
        store: Arc<TaskStore>,
        config: IpcServerConfig,
    ) -> Result<Self, IpcError> {
        Self::bind_named_pipe_inner(pipe_name.into(), store, config, None).await
    }

    pub async fn bind_named_pipe_with_supervisor(
        pipe_name: impl Into<String>,
        store: Arc<TaskStore>,
        config: IpcServerConfig,
        supervisor: Arc<crate::Supervisor>,
    ) -> Result<Self, IpcError> {
        Self::bind_named_pipe_inner(pipe_name.into(), store, config, Some(supervisor)).await
    }

    async fn bind_named_pipe_inner(
        pipe_name: String,
        store: Arc<TaskStore>,
        config: IpcServerConfig,
        supervisor: Option<Arc<crate::Supervisor>>,
    ) -> Result<Self, IpcError> {
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
        let clients = Arc::new(AtomicUsize::new(0));
        let server_clients = Arc::clone(&clients);
        let join = tokio::spawn(async move {
            let mut first_instance = true;
            let mut client_tasks = tokio::task::JoinSet::new();
            loop {
                let mut options = ServerOptions::new();
                options
                    .first_pipe_instance(first_instance)
                    .reject_remote_clients(true);
                let server = options.create(&pipe_name)?;
                first_instance = false;
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    connected = server.connect() => {
                        connected?;
                        let connection = supervisor.as_ref().map_or_else(
                            || IpcConnection::new(Arc::clone(&store), config.clone()),
                            |supervisor| IpcConnection::with_supervisor(
                                Arc::clone(&store),
                                config.clone(),
                                Arc::clone(supervisor),
                            ),
                        );
                        let client_lease = ClientLease::new(Arc::clone(&server_clients));
                        client_tasks.spawn(async move {
                            let _client_lease = client_lease;
                            if let Err(error) = serve_named_pipe_client(server, connection).await {
                                tracing::debug!(error = %error, "local Named Pipe client disconnected");
                            }
                        });
                    }
                    Some(joined) = client_tasks.join_next(), if !client_tasks.is_empty() => {
                        if let Err(error) = joined {
                            tracing::debug!(error = %error, "local Named Pipe client task stopped");
                        }
                    }
                }
            }
            client_tasks.abort_all();
            while client_tasks.join_next().await.is_some() {}
            Ok(())
        });
        Ok(Self {
            shutdown: Some(shutdown_tx),
            join,
            clients,
        })
    }
}

async fn serve_named_pipe_client(
    mut pipe: NamedPipeServer,
    mut connection: IpcConnection,
) -> Result<(), IpcError> {
    let mut subscription_poll = tokio::time::interval(std::time::Duration::from_millis(10));
    subscription_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            frame = read_frame(&mut pipe) => {
                let Some(frame) = frame? else {
                    return Ok(());
                };
                for response in connection.handle_async(frame).await? {
                    pipe.write_all(&encode_frame(&response)?).await?;
                }
            }
            _ = subscription_poll.tick() => {
                if let Some(response) = connection.poll_subscription()? {
                    pipe.write_all(&encode_frame(&response)?).await?;
                }
            }
        }
    }
}

async fn read_frame(pipe: &mut NamedPipeServer) -> Result<Option<ClientFrame>, IpcError> {
    let mut header = [0_u8; 4];
    match pipe.read_exact(&mut header).await {
        Ok(_) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionReset
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    }
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 {
        return Err(IpcError::ZeroLengthFrame);
    }
    if length > MAX_FRAME_BYTES {
        return Err(IpcError::FrameTooLarge);
    }
    let mut body = vec![0_u8; length];
    pipe.read_exact(&mut body).await?;
    Ok(Some(serde_json::from_slice(&body)?))
}

struct ClientLease(Arc<AtomicUsize>);

impl ClientLease {
    fn new(clients: Arc<AtomicUsize>) -> Self {
        clients.fetch_add(1, Ordering::AcqRel);
        Self(clients)
    }
}

impl Drop for ClientLease {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

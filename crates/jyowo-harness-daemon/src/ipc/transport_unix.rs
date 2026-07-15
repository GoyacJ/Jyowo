use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use harness_contracts::ClientFrame;
use harness_journal::TaskStore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;

use super::{
    encode_frame, EndpointCleanup, IpcConnection, IpcError, IpcServerConfig, LocalIpcServer,
    MAX_FRAME_BYTES,
};

impl LocalIpcServer {
    pub async fn bind_unix(
        endpoint: impl AsRef<Path>,
        store: Arc<TaskStore>,
        config: IpcServerConfig,
    ) -> Result<Self, IpcError> {
        Self::bind_unix_inner(
            endpoint.as_ref(),
            store,
            config,
            None,
            None,
            None,
            None,
            None,
        )
        .await
    }

    pub async fn bind_unix_with_supervisor(
        endpoint: impl AsRef<Path>,
        store: Arc<TaskStore>,
        config: IpcServerConfig,
        supervisor: Arc<crate::Supervisor>,
    ) -> Result<Self, IpcError> {
        Self::bind_unix_inner(
            endpoint.as_ref(),
            store,
            config,
            Some(supervisor),
            None,
            None,
            None,
            None,
        )
        .await
    }

    pub async fn bind_unix_with_runtime_services(
        endpoint: impl AsRef<Path>,
        store: Arc<TaskStore>,
        config: IpcServerConfig,
        supervisor: Arc<crate::Supervisor>,
        skill_reference_candidates: Arc<crate::SkillReferenceCandidateService>,
        memory_service: Arc<crate::MemoryService>,
        automation_scheduler: Arc<crate::AutomationScheduler>,
        browser_service: Arc<crate::BrowserService>,
    ) -> Result<Self, IpcError> {
        Self::bind_unix_inner(
            endpoint.as_ref(),
            store,
            config,
            Some(supervisor),
            Some(skill_reference_candidates),
            Some(memory_service),
            Some(automation_scheduler),
            Some(browser_service),
        )
        .await
    }

    async fn bind_unix_inner(
        endpoint: &Path,
        store: Arc<TaskStore>,
        config: IpcServerConfig,
        supervisor: Option<Arc<crate::Supervisor>>,
        skill_reference_candidates: Option<Arc<crate::SkillReferenceCandidateService>>,
        memory_service: Option<Arc<crate::MemoryService>>,
        automation_scheduler: Option<Arc<crate::AutomationScheduler>>,
        browser_service: Option<Arc<crate::BrowserService>>,
    ) -> Result<Self, IpcError> {
        let endpoint = endpoint.to_path_buf();
        let listener = UnixListener::bind(&endpoint)?;
        std::fs::set_permissions(&endpoint, std::fs::Permissions::from_mode(0o600))?;
        let endpoint_cleanup =
            EndpointCleanup::unix(endpoint.clone(), &std::fs::metadata(&endpoint)?);
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
        let clients = Arc::new(AtomicUsize::new(0));
        let server_clients = Arc::clone(&clients);
        let join = tokio::spawn(async move {
            let mut client_tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => {
                        let (stream, _) = accepted?;
                        let mut connection = supervisor.as_ref().map_or_else(
                            || IpcConnection::new(Arc::clone(&store), config.clone()),
                            |supervisor| IpcConnection::with_supervisor(
                                Arc::clone(&store),
                                config.clone(),
                                Arc::clone(supervisor),
                            ),
                        );
                        if let Some(service) = skill_reference_candidates.as_ref() {
                            connection = connection.with_skill_reference_candidate_service(Arc::clone(service));
                        }
                        if let Some(memory_service) = memory_service.as_ref() {
                            connection = connection.with_memory_service(Arc::clone(memory_service));
                        }
                        if let Some(automation_scheduler) = automation_scheduler.as_ref() {
                            connection = connection.with_automation_scheduler(Arc::clone(automation_scheduler));
                        }
                        if let Some(browser_service) = browser_service.as_ref() {
                            connection = connection.with_browser_service(Arc::clone(browser_service));
                        }
                        let client_lease = ClientLease::new(Arc::clone(&server_clients));
                        client_tasks.spawn(async move {
                            let _client_lease = client_lease;
                            if let Err(error) = serve_unix_client(stream, connection).await {
                                tracing::debug!(error = %error, "local IPC client disconnected");
                            }
                        });
                    }
                    Some(joined) = client_tasks.join_next(), if !client_tasks.is_empty() => {
                        if let Err(error) = joined {
                            tracing::debug!(error = %error, "local IPC client task stopped");
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
            endpoint: Some(endpoint_cleanup),
            clients,
        })
    }
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

async fn serve_unix_client(
    mut stream: UnixStream,
    mut connection: IpcConnection,
) -> Result<(), IpcError> {
    let mut subscription_poll = tokio::time::interval(std::time::Duration::from_millis(10));
    subscription_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            frame = read_frame(&mut stream) => {
                let Some(frame) = frame? else {
                    return Ok(());
                };
                for response in connection.handle_async(frame).await? {
                    stream.write_all(&encode_frame(&response)?).await?;
                }
            }
            _ = subscription_poll.tick() => {
                if let Some(response) = connection.poll_subscription()? {
                    stream.write_all(&encode_frame(&response)?).await?;
                }
            }
        }
    }
}

async fn read_frame(stream: &mut UnixStream) -> Result<Option<ClientFrame>, IpcError> {
    let mut header = [0_u8; 4];
    match stream.read_exact(&mut header).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
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
    stream.read_exact(&mut body).await?;
    Ok(Some(serde_json::from_slice(&body)?))
}

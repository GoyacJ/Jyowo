use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex as StdMutex,
    },
};

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use harness_contracts::{now, Event, NoopRedactor, RedactRules, Redactor, UnexpectedErrorEvent};
use serde_json::Value;
use tokio::{
    io::AsyncWrite,
    process::{Child, Command},
    sync::{broadcast, mpsc, oneshot, Mutex, Notify},
    task::JoinHandle,
};
use tokio_stream::wrappers::BroadcastStream;
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

use crate::{
    authorize_mcp_transport_connect, call_tool_request, client_inbound_support,
    decode_empty_result, decode_list_prompts, decode_list_resources, decode_list_tools,
    decode_prompt_messages, decode_read_resource, decode_tool_result, get_prompt_request,
    list_prompts_request, list_resources_request, list_tools_request, notification_change,
    read_resource_request, subscribe_resource_request, unsubscribe_resource_request,
    JsonRpcNotification, JsonRpcPeer, JsonRpcRequest, JsonRpcResponse, ListChangedEvent, McpChange,
    McpConnectContext, McpConnection, McpError, McpImplementation, McpListPage, McpMessage,
    McpMessageSink, McpOrderedNotificationHandler, McpOutboundMessage, McpPeer, McpPrompt,
    McpPromptMessages, McpReadResourceResult, McpResource, McpServerSpec, McpSession,
    McpToolCallEvent, McpToolCallStream, McpToolDescriptor, McpToolResult, McpTransport,
    McpWeakPeer, NoopMcpEventSink, StdioEnv, StdioPolicy, TransportChoice,
};

#[cfg(test)]
use crate::McpClientCapabilities;

const STDIO_OUTBOUND_CAPACITY: usize = 64;

pub struct StdioTransport {
    event_sink: Arc<dyn crate::McpEventSink>,
    redactor: Arc<dyn Redactor>,
}

impl StdioTransport {
    pub fn new() -> Self {
        Self {
            event_sink: Arc::new(NoopMcpEventSink),
            redactor: Arc::new(NoopRedactor),
        }
    }

    pub fn with_event_sink(event_sink: Arc<dyn crate::McpEventSink>) -> Self {
        Self {
            event_sink,
            redactor: Arc::new(NoopRedactor),
        }
    }

    pub fn with_redactor(mut self, redactor: Arc<dyn Redactor>) -> Self {
        self.redactor = redactor;
        self
    }

    pub fn resolve_env(
        env: &StdioEnv,
        parent: &BTreeMap<String, String>,
    ) -> BTreeMap<String, String> {
        match env {
            StdioEnv::Allowlist { inherit, extra } => {
                let mut resolved = parent
                    .iter()
                    .filter(|(key, _)| inherit.contains(*key))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect::<BTreeMap<_, _>>();
                resolved.extend(extra.clone());
                resolved
            }
            StdioEnv::InheritWithDeny { deny, extra } => {
                let mut resolved = parent
                    .iter()
                    .filter(|(key, _)| {
                        !deny.iter().any(|pattern| env_pattern_matches(pattern, key))
                    })
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect::<BTreeMap<_, _>>();
                resolved.extend(extra.clone());
                resolved
            }
            StdioEnv::Empty { extra } => extra.clone(),
        }
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    fn transport_id(&self) -> &'static str {
        "stdio"
    }

    async fn connect(&self, spec: McpServerSpec) -> Result<Arc<dyn McpConnection>, McpError> {
        self.connect_with_context(spec, McpConnectContext::default())
            .await
    }

    async fn connect_with_context(
        &self,
        spec: McpServerSpec,
        context: McpConnectContext,
    ) -> Result<Arc<dyn McpConnection>, McpError> {
        authorize_mcp_transport_connect(&context, &spec).await?;
        let TransportChoice::Stdio {
            command,
            args,
            env,
            policy,
        } = spec.transport.clone()
        else {
            return Err(McpError::Unsupported(
                "StdioTransport requires TransportChoice::Stdio".into(),
            ));
        };

        let parent = std::env::vars().collect::<BTreeMap<_, _>>();
        let resolved_env = Self::resolve_env(&env, &parent);
        let (command, resolved_env) = prepare_stdio_process(&command, resolved_env, &parent);
        let mut command_builder = Command::new(&command);
        command_builder
            .args(args)
            .env_clear()
            .envs(resolved_env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(working_dir) = policy.working_dir.clone() {
            command_builder.current_dir(working_dir);
        }

        let mut child = command_builder
            .spawn()
            .map_err(|error| McpError::Transport(error.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Transport("stdio child missing stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Transport("stdio child missing stdout".into()))?;
        if let Some(stderr) = child.stderr.take() {
            let server_id = spec.server_id.clone();
            let event_sink = Arc::clone(&self.event_sink);
            let redactor = Arc::clone(&self.redactor);
            let policy = policy.clone();
            tokio::spawn(async move {
                let mut lines = FramedRead::new(stderr, LinesCodec::new());
                while let Some(line) = lines.next().await {
                    if let Ok(line) = line {
                        let line = stderr_line_for_journal(&line, &policy, redactor.as_ref());
                        event_sink.emit(Event::UnexpectedError(UnexpectedErrorEvent {
                            session_id: None,
                            run_id: None,
                            error: format!("mcp stdio stderr {}: {line}", server_id.0),
                            at: now(),
                        }));
                    }
                }
            });
        }

        let (changes, _) = broadcast::channel(64);
        let (outbound, outbound_rx) = mpsc::channel(STDIO_OUTBOUND_CAPACITY);
        let sink = Arc::new(StdioMessageSink::new(outbound));
        let support = client_inbound_support(&spec, &context);
        let session = McpSession::new(
            spec.capabilities_expected,
            support.capabilities,
            McpImplementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
        );
        let notification_handler = Arc::new(StdioNotificationHandler {
            changes: changes.clone(),
        });
        let mut peer_builder = McpPeer::builder(sink.clone(), session);
        if let Some(handler) = support.sampling {
            peer_builder = peer_builder.sampling_handler(handler);
        }
        if let Some(handler) = support.elicitation {
            peer_builder = peer_builder.elicitation_handler(handler);
        }
        for method in [
            "tools/list_changed",
            "notifications/tools/list_changed",
            "resources/list_changed",
            "notifications/resources/list_changed",
            "resources/updated",
            "notifications/resources/updated",
            "prompts/list_changed",
            "notifications/prompts/list_changed",
            "notifications/cancelled",
            "notifications/progress",
        ] {
            peer_builder =
                peer_builder.ordered_notification_handler(method, notification_handler.clone());
        }
        let peer = peer_builder.build()?;
        let writer_task = spawn_writer(stdin, outbound_rx, peer.downgrade(), sink.writer_state());
        let (reader_probe, reader_probes) = mpsc::channel(1);
        spawn_reader(stdout, peer.clone(), reader_probes);

        let handshake = async {
            peer.initialize(spec.timeouts.handshake).await?;
            sink.flush().await?;
            confirm_reader_open(&reader_probe).await?;
            peer.ensure_open()
        };
        let initialize_result = tokio::time::timeout(spec.timeouts.handshake, handshake)
            .await
            .map_err(|_| McpError::Connection("MCP initialize handshake timed out".to_owned()))
            .and_then(|result| result);
        if let Err(error) = initialize_result {
            peer.close(format!("stdio initialize failed: {error}"))
                .await;
            sink.close().await;
            writer_task.abort();
            let _ = writer_task.await;
            return Err(error);
        }

        let connection = Arc::new(StdioConnection {
            connection_id: format!("stdio:{}", spec.server_id.0),
            changes,
            child: Arc::new(Mutex::new(Some(child))),
            timeout: spec.timeouts.call_default,
            policy,
            peer,
            sink,
            writer_task: Arc::new(Mutex::new(Some(writer_task))),
            legacy_request_builder: JsonRpcPeer::new(),
            active_tool_calls: Arc::new(StdMutex::new(HashMap::new())),
        });
        Ok(connection)
    }
}

fn prepare_stdio_process(
    command: &str,
    env: BTreeMap<String, String>,
    parent: &BTreeMap<String, String>,
) -> (String, BTreeMap<String, String>) {
    let execution_path = stdio_execution_path(&env, parent);
    if command_has_path_separator(command) {
        return (command.to_owned(), env);
    }
    let resolved_command = find_executable_on_path(command, &execution_path)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| command.to_owned());
    (resolved_command, env)
}

fn stdio_execution_path(
    env: &BTreeMap<String, String>,
    parent: &BTreeMap<String, String>,
) -> String {
    let mut paths = Vec::new();
    extend_path_dirs(&mut paths, env.get("PATH"));
    extend_path_dirs(&mut paths, parent.get("PATH"));
    extend_common_node_paths(&mut paths, env.get("HOME").or_else(|| parent.get("HOME")));
    std::env::join_paths(paths)
        .ok()
        .and_then(|value| value.into_string().ok())
        .unwrap_or_default()
}

fn extend_path_dirs(paths: &mut Vec<PathBuf>, value: Option<&String>) {
    if let Some(value) = value {
        paths.extend(std::env::split_paths(value));
    }
}

fn extend_common_node_paths(paths: &mut Vec<PathBuf>, home: Option<&String>) {
    paths.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/opt/homebrew/sbin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ]);
    let Some(home) = home else {
        return;
    };
    let home = PathBuf::from(home);
    paths.extend([
        home.join(".volta/bin"),
        home.join(".asdf/shims"),
        home.join(".mise/shims"),
        home.join(".local/bin"),
        home.join("Library/pnpm"),
    ]);
    let nvm_versions = home.join(".nvm/versions/node");
    if let Ok(entries) = std::fs::read_dir(nvm_versions) {
        let mut bins = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("bin"))
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        bins.sort();
        bins.reverse();
        paths.extend(bins);
    }
}

fn command_has_path_separator(command: &str) -> bool {
    command.contains('/') || command.contains('\\')
}

fn find_executable_on_path(command: &str, path: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(command))
        .find(|candidate| is_executable_file(candidate))
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    let Ok(metadata) = path.metadata() else {
        return false;
    };
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

pub struct StdioConnection {
    connection_id: String,
    changes: broadcast::Sender<StdioChange>,
    child: Arc<Mutex<Option<Child>>>,
    timeout: std::time::Duration,
    policy: StdioPolicy,
    peer: McpPeer,
    sink: Arc<StdioMessageSink>,
    writer_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    legacy_request_builder: JsonRpcPeer,
    active_tool_calls: Arc<StdMutex<HashMap<String, Value>>>,
}

impl StdioConnection {
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let id = request.id;
        match self
            .peer
            .request_optional(request.method, request.params, self.timeout)
            .await
        {
            Ok(result) => Ok(JsonRpcResponse::success(id, result)),
            Err(McpError::RemoteJsonRpc(error)) => Ok(JsonRpcResponse::failure(id, error)),
            Err(error) => Err(error),
        }
    }

    async fn send_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        match notification.params {
            Some(params) => self.peer.notify(notification.method, params).await,
            None => self.peer.notify_without_params(notification.method).await,
        }?;
        self.sink.flush().await?;
        self.peer.ensure_open()
    }

    async fn call_tool_events_inner(
        &self,
        client_request_id: Option<&str>,
        name: &str,
        args: Value,
    ) -> Result<McpToolCallStream, McpError> {
        let request = call_tool_request(&self.legacy_request_builder, name, args);
        let mut changes = BroadcastStream::new(self.changes.subscribe());
        let pending = self
            .peer
            .start_request_with(request.method, self.timeout, move |request_id| {
                request
                    .params
                    .map(|params| with_progress_token(params, request_id))
            })
            .await?;
        let request_id = checked_request_id(pending.request_id())?;
        let key = request_id_key(&request_id)?;
        let active_call = match client_request_id {
            Some(client_request_id) => Some(ActiveToolCallGuard::insert(
                Arc::clone(&self.active_tool_calls),
                client_request_id.to_owned(),
                request_id,
            )?),
            None => None,
        };

        Ok(Box::pin(async_stream::stream! {
            let _active_call = active_call;
            let response = pending.wait();
            tokio::pin!(response);
            let mut changes_open = true;
            loop {
                if changes_open {
                    tokio::select! {
                        biased;
                        change = changes.next() => match change {
                            Some(Ok(change)) => {
                                if let Some(event) = stdio_tool_call_event_from_change(&key, change) {
                                    yield event;
                                }
                            },
                            Some(Err(_)) => {},
                            None => {
                                changes_open = false;
                            },
                        },
                        result = &mut response => {
                            match decode_peer_tool_result(result) {
                                Ok(result) => yield McpToolCallEvent::Final(result),
                                Err(error) => yield McpToolCallEvent::Error(error),
                            }
                            break;
                        },
                    }
                } else {
                    match decode_peer_tool_result((&mut response).await) {
                        Ok(result) => yield McpToolCallEvent::Final(result),
                        Err(error) => yield McpToolCallEvent::Error(error),
                    }
                    break;
                }
            }
        }))
    }
}

#[async_trait]
impl McpConnection for StdioConnection {
    fn connection_id(&self) -> &str {
        &self.connection_id
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        self.list_tools_all().await
    }

    async fn list_tools_page(
        &self,
        cursor: Option<&str>,
    ) -> Result<McpListPage<McpToolDescriptor>, McpError> {
        decode_list_tools(
            self.send(list_tools_request(&self.legacy_request_builder, cursor))
                .await?,
        )
    }

    async fn call_tool(&self, name: &str, args: Value) -> Result<McpToolResult, McpError> {
        decode_tool_result(
            self.send(call_tool_request(&self.legacy_request_builder, name, args))
                .await?,
        )
    }

    async fn cancel_tool_call(
        &self,
        request_id: &str,
        reason: Option<String>,
    ) -> Result<(), McpError> {
        let request_id = self
            .active_tool_calls
            .lock()
            .map_err(|_| McpError::Connection("stdio active tool call map poisoned".to_owned()))?
            .get(request_id)
            .cloned()
            .unwrap_or_else(|| Value::String(request_id.to_owned()));
        self.send_notification(JsonRpcNotification::new(
            "notifications/cancelled",
            Some(serde_json::json!({
                "requestId": request_id,
                "reason": reason,
            })),
        ))
        .await
    }

    async fn call_tool_events(
        &self,
        name: &str,
        args: Value,
    ) -> Result<McpToolCallStream, McpError> {
        self.call_tool_events_inner(None, name, args).await
    }

    async fn call_tool_events_for_request(
        &self,
        client_request_id: &str,
        name: &str,
        args: Value,
    ) -> Result<McpToolCallStream, McpError> {
        self.call_tool_events_inner(Some(client_request_id), name, args)
            .await
    }

    async fn list_resources(&self) -> Result<Vec<McpResource>, McpError> {
        self.list_resources_all().await
    }

    async fn list_resources_page(
        &self,
        cursor: Option<&str>,
    ) -> Result<McpListPage<McpResource>, McpError> {
        decode_list_resources(
            self.send(list_resources_request(&self.legacy_request_builder, cursor))
                .await?,
        )
    }

    async fn read_resource(&self, uri: &str) -> Result<McpReadResourceResult, McpError> {
        decode_read_resource(
            self.send(read_resource_request(&self.legacy_request_builder, uri))
                .await?,
        )
    }

    async fn subscribe_resource(&self, uri: &str) -> Result<(), McpError> {
        decode_empty_result(
            self.send(subscribe_resource_request(
                &self.legacy_request_builder,
                uri,
            ))
            .await?,
        )
    }

    async fn unsubscribe_resource(&self, uri: &str) -> Result<(), McpError> {
        decode_empty_result(
            self.send(unsubscribe_resource_request(
                &self.legacy_request_builder,
                uri,
            ))
            .await?,
        )
    }

    async fn list_prompts(&self) -> Result<Vec<McpPrompt>, McpError> {
        self.list_prompts_all().await
    }

    async fn list_prompts_page(
        &self,
        cursor: Option<&str>,
    ) -> Result<McpListPage<McpPrompt>, McpError> {
        decode_list_prompts(
            self.send(list_prompts_request(&self.legacy_request_builder, cursor))
                .await?,
        )
    }

    async fn get_prompt(&self, name: &str, args: Value) -> Result<McpPromptMessages, McpError> {
        decode_prompt_messages(
            self.send(get_prompt_request(&self.legacy_request_builder, name, args))
                .await?,
        )
    }

    async fn subscribe_changes(&self) -> Result<ListChangedEvent, McpError> {
        let stream = BroadcastStream::new(self.changes.subscribe())
            .filter_map(|event| async move { event.ok().map(|event| event.change) });
        Ok(Box::pin(stream))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        self.peer.close("stdio connection shutting down").await;
        self.sink.close().await;
        let mut errors = Vec::new();
        if let Some(writer_task) = self.writer_task.lock().await.take() {
            writer_task.abort();
            if let Err(error) = writer_task.await {
                if !error.is_cancelled() {
                    errors.push(format!("stdio writer task failed: {error}"));
                }
            }
        }
        if let Some(mut child) = self.child.lock().await.take() {
            if let Err(error) = shutdown_child(&mut child, self.policy.graceful_kill_after).await {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(McpError::Transport(errors.join("; ")))
        }
    }
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

struct StdioOutboundFrame {
    sequence: u64,
    payload: String,
}

#[derive(Default)]
struct StdioWriterState {
    last_written: AtomicU64,
    failure: StdMutex<Option<McpError>>,
    changed: Notify,
}

impl StdioWriterState {
    fn mark_written(&self, sequence: u64) {
        self.last_written.store(sequence, Ordering::Release);
        self.changed.notify_waiters();
    }

    fn fail(&self, error: McpError) {
        if let Ok(mut failure) = self.failure.lock() {
            if failure.is_none() {
                *failure = Some(error);
            }
        }
        self.changed.notify_waiters();
    }

    fn failure(&self) -> Option<McpError> {
        self.failure.lock().ok().and_then(|failure| failure.clone())
    }

    async fn wait_until_written(&self, sequence: u64) -> Result<(), McpError> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(error) = self.failure() {
                return Err(error);
            }
            if self.last_written.load(Ordering::Acquire) >= sequence {
                return Ok(());
            }
            changed.await;
        }
    }
}

struct StdioMessageSink {
    sender: Mutex<Option<mpsc::Sender<StdioOutboundFrame>>>,
    commit: StdMutex<StdioCommitState>,
    writer_state: Arc<StdioWriterState>,
    #[cfg(test)]
    before_commit: StdMutex<Option<Arc<dyn Fn(u64) + Send + Sync>>>,
}

struct StdioCommitState {
    next_sequence: u64,
}

impl StdioMessageSink {
    fn new(sender: mpsc::Sender<StdioOutboundFrame>) -> Self {
        Self {
            sender: Mutex::new(Some(sender)),
            commit: StdMutex::new(StdioCommitState { next_sequence: 1 }),
            writer_state: Arc::new(StdioWriterState::default()),
            #[cfg(test)]
            before_commit: StdMutex::new(None),
        }
    }

    fn writer_state(&self) -> Arc<StdioWriterState> {
        Arc::clone(&self.writer_state)
    }

    async fn flush(&self) -> Result<(), McpError> {
        let sequence = self
            .commit
            .lock()
            .map_err(|_| McpError::Connection("stdio commit mutex poisoned".to_owned()))?
            .next_sequence
            .saturating_sub(1);
        self.writer_state.wait_until_written(sequence).await
    }

    async fn close(&self) {
        self.writer_state.fail(McpError::Connection(
            "stdio message sink is closed".to_owned(),
        ));
        self.sender.lock().await.take();
    }
}

#[async_trait]
impl McpMessageSink for StdioMessageSink {
    async fn send(&self, message: McpOutboundMessage) -> Result<(), McpError> {
        let payload = serde_json::to_string(message.as_message()).map_err(|error| {
            McpError::Protocol(format!("failed to encode MCP message: {error}"))
        })?;
        let sender = self
            .sender
            .lock()
            .await
            .clone()
            .ok_or_else(|| McpError::Connection("stdio stdin is closed".to_owned()))?;
        let permit = sender
            .reserve_owned()
            .await
            .map_err(|_| McpError::Connection("stdio writer is closed".to_owned()))?;
        let mut commit = self
            .commit
            .lock()
            .map_err(|_| McpError::Connection("stdio commit mutex poisoned".to_owned()))?;
        let sequence = commit.next_sequence;
        commit.next_sequence = sequence
            .checked_add(1)
            .ok_or_else(|| McpError::Connection("stdio writer sequence exhausted".to_owned()))?;
        #[cfg(test)]
        let before_commit = self
            .before_commit
            .lock()
            .expect("stdio test commit hook mutex")
            .clone();
        #[cfg(test)]
        if let Some(before_commit) = before_commit {
            before_commit(sequence);
        }
        permit.send(StdioOutboundFrame { sequence, payload });
        drop(commit);
        Ok(())
    }
}

#[derive(Clone)]
struct StdioChange {
    change: McpChange,
    correlation_key: Option<String>,
}

struct StdioNotificationHandler {
    changes: broadcast::Sender<StdioChange>,
}

impl McpOrderedNotificationHandler for StdioNotificationHandler {
    fn handle_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        if let Some(change) =
            notification_change(&notification.method, notification.params.as_ref())
        {
            let correlation_key =
                notification_correlation_key(&notification.method, notification.params.as_ref());
            let _ = self.changes.send(StdioChange {
                change,
                correlation_key,
            });
        }
        Ok(())
    }
}

fn spawn_writer(
    stdin: tokio::process::ChildStdin,
    outbound: mpsc::Receiver<StdioOutboundFrame>,
    peer: McpWeakPeer,
    writer_state: Arc<StdioWriterState>,
) -> JoinHandle<()> {
    spawn_writer_io(stdin, outbound, peer, writer_state)
}

fn spawn_writer_io<W>(
    writer: W,
    mut outbound: mpsc::Receiver<StdioOutboundFrame>,
    peer: McpWeakPeer,
    writer_state: Arc<StdioWriterState>,
) -> JoinHandle<()>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut writer = FramedWrite::new(writer, LinesCodec::new());
        while let Some(frame) = outbound.recv().await {
            if let Err(error) = writer.send(frame.payload).await {
                let error = McpError::Transport(format!("stdio writer failed: {error}"));
                writer_state.fail(error.clone());
                peer.close(error.to_string()).await;
                return;
            }
            writer_state.mark_written(frame.sequence);
        }
    })
}

type StdioReaderProbe = oneshot::Sender<Result<(), McpError>>;

async fn confirm_reader_open(probes: &mpsc::Sender<StdioReaderProbe>) -> Result<(), McpError> {
    let (result, receiver) = oneshot::channel();
    probes
        .send(result)
        .await
        .map_err(|_| McpError::Connection("stdio reader is closed".to_owned()))?;
    receiver
        .await
        .map_err(|_| McpError::Connection("stdio reader probe was dropped".to_owned()))?
}

fn spawn_reader(
    stdout: tokio::process::ChildStdout,
    peer: McpPeer,
    mut probes: mpsc::Receiver<StdioReaderProbe>,
) {
    tokio::spawn(async move {
        let mut reader = FramedRead::new(stdout, LinesCodec::new());
        loop {
            let line = tokio::select! {
                biased;
                line = reader.next() => line,
                Some(probe) = probes.recv() => {
                    let _ = probe.send(peer.ensure_open());
                    continue;
                }
            };
            let Some(line) = line else {
                break;
            };
            let message = match line {
                Ok(line) => serde_json::from_str::<McpMessage>(&line)
                    .map_err(|error| McpError::InvalidResponse(error.to_string())),
                Err(error) => Err(McpError::Transport(error.to_string())),
            };
            let message = match message {
                Ok(message) => message,
                Err(error) => {
                    peer.close(format!("stdio reader failed: {error}")).await;
                    return;
                }
            };
            if let Err(error) = peer.receive(message).await {
                peer.close(format!("stdio inbound routing failed: {error}"))
                    .await;
                return;
            }
        }
        peer.close("stdio child stdout closed").await;
    });
}

fn request_id_key(id: &Value) -> Result<String, McpError> {
    checked_request_id(id)?;
    serde_json::to_string(id)
        .map_err(|error| McpError::Protocol(format!("failed to encode MCP request id: {error}")))
}

fn checked_request_id(id: &Value) -> Result<Value, McpError> {
    if matches!(id, Value::String(_))
        || matches!(id, Value::Number(number) if number.is_i64() || number.is_u64())
    {
        Ok(id.clone())
    } else {
        Err(McpError::Protocol(
            "MCP request id must be a string or integer".to_owned(),
        ))
    }
}

struct ActiveToolCallGuard {
    active_tool_calls: Arc<StdMutex<HashMap<String, Value>>>,
    client_request_id: String,
    peer_request_id: Value,
}

impl ActiveToolCallGuard {
    fn insert(
        active_tool_calls: Arc<StdMutex<HashMap<String, Value>>>,
        client_request_id: String,
        peer_request_id: Value,
    ) -> Result<Self, McpError> {
        active_tool_calls
            .lock()
            .map_err(|_| McpError::Connection("stdio active tool call map poisoned".to_owned()))?
            .insert(client_request_id.clone(), peer_request_id.clone());
        Ok(Self {
            active_tool_calls,
            client_request_id,
            peer_request_id,
        })
    }
}

impl Drop for ActiveToolCallGuard {
    fn drop(&mut self) {
        if let Ok(mut active_tool_calls) = self.active_tool_calls.lock() {
            if active_tool_calls.get(&self.client_request_id) == Some(&self.peer_request_id) {
                active_tool_calls.remove(&self.client_request_id);
            }
        }
    }
}

fn notification_correlation_key(method: &str, params: Option<&Value>) -> Option<String> {
    let token = match method {
        "notifications/cancelled" => params?
            .get("requestId")
            .or_else(|| params?.get("request_id")),
        "notifications/progress" => params?
            .get("progressToken")
            .or_else(|| params?.get("progress_token")),
        _ => None,
    }?;
    request_id_key(token).ok()
}

fn stdio_tool_call_event_from_change(
    request_key: &str,
    change: StdioChange,
) -> Option<McpToolCallEvent> {
    if change.correlation_key.as_deref() != Some(request_key) {
        return None;
    }
    match change.change {
        McpChange::Progress {
            progress_token,
            progress,
            total,
            message,
        } => Some(McpToolCallEvent::Progress {
            progress_token,
            progress,
            total,
            message,
        }),
        McpChange::Cancelled { request_id, reason } => {
            Some(McpToolCallEvent::Cancelled { request_id, reason })
        }
        _ => None,
    }
}

fn with_progress_token(mut params: Value, request_id: &Value) -> Value {
    let Some(params_object) = params.as_object_mut() else {
        return params;
    };
    let meta = params_object
        .entry("_meta")
        .or_insert_with(|| Value::Object(Default::default()));
    if let Some(meta) = meta.as_object_mut() {
        meta.insert("progressToken".to_owned(), request_id.clone());
    }
    params
}

fn decode_peer_tool_result(result: Result<Value, McpError>) -> Result<McpToolResult, McpError> {
    let response = match result {
        Ok(result) => JsonRpcResponse::success(serde_json::json!(0), result),
        Err(McpError::RemoteJsonRpc(error)) => {
            JsonRpcResponse::failure(serde_json::json!(0), error)
        }
        Err(error) => return Err(error),
    };
    decode_tool_result(response)
}

async fn shutdown_child(child: &mut Child, timeout: std::time::Duration) -> Result<(), McpError> {
    let mut errors = Vec::new();
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(_)) => return Ok(()),
        Ok(Err(error)) => errors.push(format!("failed waiting for stdio child exit: {error}")),
        Err(_) => {}
    }

    if let Err(error) = terminate_child(child) {
        errors.push(error.to_string());
    }
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(_)) => return shutdown_errors(errors),
        Ok(Err(error)) => errors.push(format!(
            "failed waiting for stdio child after termination: {error}"
        )),
        Err(_) => {}
    }

    if let Err(error) = child.start_kill() {
        errors.push(format!("failed to kill stdio child: {error}"));
    }
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => errors.push(format!("failed reaping stdio child: {error}")),
        Err(_) => errors.push(format!(
            "timed out reaping stdio child after {} ms",
            timeout.as_millis()
        )),
    }
    shutdown_errors(errors)
}

fn shutdown_errors(errors: Vec<String>) -> Result<(), McpError> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(McpError::Transport(errors.join("; ")))
    }
}

#[cfg(unix)]
fn terminate_child(child: &Child) -> Result<(), McpError> {
    let Some(pid) = child.id() else {
        return Ok(());
    };
    match nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid.cast_signed()),
        nix::sys::signal::Signal::SIGTERM,
    ) {
        Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
        Err(error) => Err(McpError::Transport(format!(
            "failed to terminate stdio child: {error}"
        ))),
    }
}

#[cfg(not(unix))]
fn terminate_child(_child: &Child) -> Result<(), McpError> {
    Ok(())
}

fn env_pattern_matches(pattern: &str, key: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        key.starts_with(prefix)
    } else {
        pattern == key
    }
}

fn stderr_line_for_journal(line: &str, policy: &StdioPolicy, redactor: &dyn Redactor) -> String {
    let capped = truncate_utf8_bytes(line, policy.stderr_line_max_bytes as usize);
    if policy.redact_stderr {
        redactor.redact(&capped, &RedactRules::default())
    } else {
        capped
    }
}

fn truncate_utf8_bytes(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    input[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc as std_mpsc;

    use super::*;

    #[test]
    fn request_ids_reject_fractional_numbers() {
        let error = request_id_key(&serde_json::json!(1.5)).expect_err("fractional request id");
        assert!(error.to_string().contains("string or integer"));
    }

    #[test]
    #[cfg(unix)]
    fn prepare_stdio_process_resolves_bare_command_from_parent_path() {
        use std::os::unix::fs::PermissionsExt;

        let root =
            std::env::temp_dir().join(format!("jyowo-mcp-stdio-path-{}", std::process::id()));
        let bin = root.join("bin");
        let executable = bin.join("fake-mcp");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();

        let parent = BTreeMap::from([
            ("PATH".to_owned(), bin.to_string_lossy().into_owned()),
            ("HOME".to_owned(), root.to_string_lossy().into_owned()),
            ("USER".to_owned(), "tester".to_owned()),
            ("TMPDIR".to_owned(), "/tmp".to_owned()),
        ]);
        let (command, env) = prepare_stdio_process("fake-mcp", BTreeMap::new(), &parent);

        assert_eq!(command, executable.to_string_lossy());
        assert!(env.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prepare_stdio_process_does_not_expand_resolved_env() {
        let parent = BTreeMap::from([
            ("PATH".to_owned(), "/usr/bin".to_owned()),
            ("HOME".to_owned(), "/parent-home".to_owned()),
        ]);
        let explicit = BTreeMap::from([("HOME".to_owned(), "/explicit-home".to_owned())]);

        let (_command, env) = prepare_stdio_process("/bin/sh", explicit, &parent);

        assert_eq!(env.get("HOME").map(String::as_str), Some("/explicit-home"));
        assert!(!env.contains_key("PATH"));
    }

    #[tokio::test]
    async fn dropping_last_peer_and_sink_lets_the_writer_task_finish() {
        let (writer, _reader) = tokio::io::duplex(64);
        let (sender, receiver) = mpsc::channel(1);
        let sink = Arc::new(StdioMessageSink::new(sender));
        let peer = McpPeer::builder(
            sink.clone(),
            McpSession::new(
                Default::default(),
                McpClientCapabilities::default(),
                McpImplementation::new("writer-test", "1.0.0"),
            ),
        )
        .build()
        .unwrap();
        let writer_state = Arc::new(StdioWriterState::default());
        let writer_task = spawn_writer_io(
            writer,
            receiver,
            peer.downgrade(),
            Arc::clone(&writer_state),
        );

        drop(peer);
        drop(sink);

        tokio::time::timeout(std::time::Duration::from_millis(100), writer_task)
            .await
            .expect("writer task must stop when its last sender is dropped")
            .expect("writer task");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn flush_cannot_pass_a_preempted_earlier_commit() {
        let (sender, mut receiver) = mpsc::channel(2);
        let sink = Arc::new(StdioMessageSink::new(sender));
        let (entered_tx, entered_rx) = std_mpsc::sync_channel(1);
        let (release_tx, release_rx) = std_mpsc::sync_channel(1);
        let release_rx = Arc::new(StdMutex::new(release_rx));
        *sink.before_commit.lock().expect("commit hook") = Some(Arc::new(move |sequence| {
            if sequence == 1 {
                entered_tx.send(()).expect("signal preemption point");
                release_rx
                    .lock()
                    .expect("release mutex")
                    .recv()
                    .expect("release first commit");
            }
        }));

        let first_sink = Arc::clone(&sink);
        let first = tokio::spawn(async move {
            first_sink
                .send(McpOutboundMessage::notification_without_params("first").unwrap())
                .await
        });
        tokio::task::spawn_blocking(move || entered_rx.recv().expect("first commit entered"))
            .await
            .expect("preemption waiter");

        let second_sink = Arc::clone(&sink);
        let second = tokio::spawn(async move {
            second_sink
                .send(McpOutboundMessage::notification_without_params("second").unwrap())
                .await
        });
        if let Ok(Some(second_frame)) =
            tokio::time::timeout(std::time::Duration::from_millis(20), receiver.recv()).await
        {
            sink.writer_state.mark_written(second_frame.sequence);
        }

        let flush_sink = Arc::clone(&sink);
        let mut flush = tokio::spawn(async move { flush_sink.flush().await });
        let flush_result =
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut flush).await;
        release_tx.send(()).expect("release first commit");
        first.await.expect("first sender").expect("first send");
        second.await.expect("second sender").expect("second send");
        if flush_result.is_err() {
            for _ in 0..2 {
                let frame = receiver.recv().await.expect("committed frame");
                sink.writer_state.mark_written(frame.sequence);
            }
            flush
                .await
                .expect("flush task")
                .expect("flush after both writes");
        }
        assert!(
            flush_result.is_err(),
            "flush must wait for the earlier commit"
        );
    }

    #[tokio::test]
    async fn closing_the_sink_wakes_a_pending_flush_with_an_error() {
        let (sender, _receiver) = mpsc::channel(1);
        let sink = Arc::new(StdioMessageSink::new(sender));
        sink.send(McpOutboundMessage::notification_without_params("pending").unwrap())
            .await
            .expect("enqueue notification");

        let flush_sink = Arc::clone(&sink);
        let mut flush = tokio::spawn(async move { flush_sink.flush().await });
        tokio::task::yield_now().await;
        sink.close().await;

        let result = tokio::time::timeout(std::time::Duration::from_millis(20), &mut flush).await;
        if result.is_err() {
            flush.abort();
            let _ = flush.await;
        }
        assert!(
            matches!(result, Ok(Ok(Err(_)))),
            "closing the sink must terminate pending flushes"
        );
    }
}

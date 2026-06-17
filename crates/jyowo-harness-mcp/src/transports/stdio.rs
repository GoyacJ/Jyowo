use std::{collections::BTreeMap, process::Stdio, sync::Arc};

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use harness_contracts::{
    now, Event, NoopRedactor, PermissionMode, RedactRules, Redactor, UnexpectedErrorEvent,
};
use serde_json::Value;
use tokio::{
    process::{Child, Command},
    sync::{broadcast, oneshot, Mutex},
};
use tokio_stream::wrappers::BroadcastStream;
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

use crate::{
    call_tool_request, continue_after_elicitation_response, decode_empty_result,
    decode_list_prompts, decode_list_resources, decode_list_tools, decode_prompt_messages,
    decode_read_resource, decode_tool_result, get_prompt_request, initialize_request,
    initialized_notification, list_prompts_request, list_resources_request, list_tools_request,
    notification_change, read_resource_request, response_key, subscribe_resource_request,
    tool_call_event_from_change, unsubscribe_resource_request, ElicitationHandler,
    JsonRpcNotification, JsonRpcPeer, JsonRpcRequest, JsonRpcResponse, ListChangedEvent, McpChange,
    McpConnectContext, McpConnection, McpError, McpPrompt, McpPromptMessages, McpResource,
    McpResourceContents, McpServerSpec, McpToolCallEvent, McpToolCallStream, McpToolDescriptor,
    McpToolResult, McpTransport, NoopMcpEventSink, StdioEnv, StdioPolicy, TransportChoice,
};

type PendingMap = Arc<
    Mutex<std::collections::HashMap<String, oneshot::Sender<Result<JsonRpcResponse, McpError>>>>,
>;
type PendingReceiver = oneshot::Receiver<Result<JsonRpcResponse, McpError>>;

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
        let mut command_builder = Command::new(&command);
        command_builder
            .args(args)
            .env_clear()
            .envs(resolved_env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
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

        let pending = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let (changes, _) = broadcast::channel(64);
        spawn_reader(stdout, Arc::clone(&pending), changes.clone());

        let connection = Arc::new(StdioConnection {
            connection_id: format!("stdio:{}", spec.server_id.0),
            writer: Arc::new(Mutex::new(FramedWrite::new(stdin, LinesCodec::new()))),
            pending,
            changes,
            child: Arc::new(Mutex::new(Some(child))),
            timeout: spec.timeouts.call_default,
            policy,
            peer: JsonRpcPeer::new(),
            elicitation_handler: context.elicitation_handler,
            permission_mode: context.permission_mode,
        });
        connection
            .send(initialize_request(&connection.peer))
            .await?;
        connection
            .send_notification(initialized_notification())
            .await?;
        Ok(connection)
    }
}

pub struct StdioConnection {
    connection_id: String,
    writer: Arc<Mutex<FramedWrite<tokio::process::ChildStdin, LinesCodec>>>,
    pending: PendingMap,
    changes: broadcast::Sender<McpChange>,
    child: Arc<Mutex<Option<Child>>>,
    timeout: std::time::Duration,
    policy: StdioPolicy,
    peer: JsonRpcPeer,
    elicitation_handler: Option<Arc<dyn ElicitationHandler>>,
    permission_mode: PermissionMode,
}

impl StdioConnection {
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let method = request.method.clone();
        let key = response_key(&request.id);
        let receiver = self.begin_send(request).await?;

        match tokio::time::timeout(self.timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(McpError::Connection("stdio response channel closed".into())),
            Err(_) => {
                self.pending.lock().await.remove(&key);
                Err(McpError::Connection(format!(
                    "stdio request timed out: {method}"
                )))
            }
        }
    }

    async fn begin_send(&self, request: JsonRpcRequest) -> Result<PendingReceiver, McpError> {
        let key = response_key(&request.id);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(key.clone(), sender);

        let payload = serde_json::to_string(&request)
            .map_err(|error| McpError::InvalidResponse(error.to_string()))?;
        if let Err(error) = self.writer.lock().await.send(payload).await {
            self.pending.lock().await.remove(&key);
            return Err(McpError::Transport(error.to_string()));
        }

        Ok(receiver)
    }

    async fn send_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError> {
        let payload = serde_json::to_string(&notification)
            .map_err(|error| McpError::InvalidResponse(error.to_string()))?;
        self.writer
            .lock()
            .await
            .send(payload)
            .await
            .map_err(|error| McpError::Transport(error.to_string()))
    }

    async fn send_with_elicitation(
        &self,
        request: JsonRpcRequest,
    ) -> Result<JsonRpcResponse, McpError> {
        let response = self.send(request.clone()).await?;
        if let Some(retry) = continue_after_elicitation_response(
            &response,
            &request,
            &self.peer,
            self.elicitation_handler.as_ref(),
            self.permission_mode,
        )
        .await?
        {
            return self.send(retry).await;
        }
        Ok(response)
    }
}

#[async_trait]
impl McpConnection for StdioConnection {
    fn connection_id(&self) -> &str {
        &self.connection_id
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        decode_list_tools(self.send(list_tools_request(&self.peer)).await?)
    }

    async fn call_tool(&self, name: &str, args: Value) -> Result<McpToolResult, McpError> {
        decode_tool_result(
            self.send_with_elicitation(call_tool_request(&self.peer, name, args))
                .await?,
        )
    }

    async fn cancel_tool_call(
        &self,
        request_id: &str,
        reason: Option<String>,
    ) -> Result<(), McpError> {
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
        let request = call_tool_request(&self.peer, name, args);
        let key = response_key(&request.id);
        let mut changes = BroadcastStream::new(self.changes.subscribe());
        let receiver = self.begin_send(request).await?;
        let timeout = self.timeout;
        let pending = Arc::clone(&self.pending);
        let timeout_key = key.clone();

        Ok(Box::pin(async_stream::stream! {
            let response = tokio::time::timeout(timeout, receiver);
            tokio::pin!(response);
            let mut changes_open = true;
            loop {
                if changes_open {
                    tokio::select! {
                        biased;
                        change = changes.next() => match change {
                            Some(Ok(change)) => {
                                if let Some(event) = tool_call_event_from_change(&key, change) {
                                    yield event;
                                }
                            },
                            Some(Err(_)) => {},
                            None => {
                                changes_open = false;
                            },
                        },
                        result = &mut response => {
                            match result {
                                Ok(Ok(Ok(response))) => match decode_tool_result(response) {
                                    Ok(result) => yield McpToolCallEvent::Final(result),
                                    Err(error) => yield McpToolCallEvent::Error(error),
                                },
                                Ok(Ok(Err(error))) => yield McpToolCallEvent::Error(error),
                                Ok(Err(_)) => yield McpToolCallEvent::Error(McpError::Connection(
                                    "stdio response channel closed".into(),
                                )),
                                Err(_) => {
                                    pending.lock().await.remove(&timeout_key);
                                    yield McpToolCallEvent::Error(McpError::Connection(
                                        "stdio request timed out: tools/call".into(),
                                    ));
                                },
                            }
                            break;
                        },
                    }
                } else {
                    match (&mut response).await {
                        Ok(Ok(Ok(response))) => match decode_tool_result(response) {
                            Ok(result) => yield McpToolCallEvent::Final(result),
                            Err(error) => yield McpToolCallEvent::Error(error),
                        },
                        Ok(Ok(Err(error))) => yield McpToolCallEvent::Error(error),
                        Ok(Err(_)) => yield McpToolCallEvent::Error(McpError::Connection(
                            "stdio response channel closed".into(),
                        )),
                        Err(_) => {
                            pending.lock().await.remove(&timeout_key);
                            yield McpToolCallEvent::Error(McpError::Connection(
                                "stdio request timed out: tools/call".into(),
                            ));
                        },
                    }
                    break;
                }
            }
        }))
    }

    async fn list_resources(&self) -> Result<Vec<McpResource>, McpError> {
        decode_list_resources(self.send(list_resources_request(&self.peer)).await?)
    }

    async fn read_resource(&self, uri: &str) -> Result<McpResourceContents, McpError> {
        decode_read_resource(self.send(read_resource_request(&self.peer, uri)).await?)
    }

    async fn subscribe_resource(&self, uri: &str) -> Result<(), McpError> {
        decode_empty_result(
            self.send(subscribe_resource_request(&self.peer, uri))
                .await?,
        )
    }

    async fn unsubscribe_resource(&self, uri: &str) -> Result<(), McpError> {
        decode_empty_result(
            self.send(unsubscribe_resource_request(&self.peer, uri))
                .await?,
        )
    }

    async fn list_prompts(&self) -> Result<Vec<McpPrompt>, McpError> {
        decode_list_prompts(self.send(list_prompts_request(&self.peer)).await?)
    }

    async fn get_prompt(&self, name: &str, args: Value) -> Result<McpPromptMessages, McpError> {
        decode_prompt_messages(
            self.send(get_prompt_request(&self.peer, name, args))
                .await?,
        )
    }

    async fn subscribe_changes(&self) -> Result<ListChangedEvent, McpError> {
        let stream = BroadcastStream::new(self.changes.subscribe())
            .filter_map(|event| async move { event.ok() });
        Ok(Box::pin(stream))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        let _ = self
            .send_notification(JsonRpcNotification::new("shutdown", None))
            .await;
        if let Some(mut child) = self.child.lock().await.take() {
            match tokio::time::timeout(self.policy.graceful_kill_after, child.wait()).await {
                Ok(_) => {}
                Err(_) => {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                }
            }
        }
        Ok(())
    }
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

fn spawn_reader(
    stdout: tokio::process::ChildStdout,
    pending: PendingMap,
    changes: broadcast::Sender<McpChange>,
) {
    tokio::spawn(async move {
        let mut reader = FramedRead::new(stdout, LinesCodec::new());
        while let Some(line) = reader.next().await {
            let value = match line {
                Ok(line) => serde_json::from_str::<Value>(&line)
                    .map_err(|error| McpError::InvalidResponse(error.to_string())),
                Err(error) => Err(McpError::Transport(error.to_string())),
            };
            let value = match value {
                Ok(value) => value,
                Err(error) => {
                    notify_all(&pending, error).await;
                    break;
                }
            };

            if let Some(method) = value.get("method").and_then(Value::as_str) {
                if let Some(change) = notification_change(method, value.get("params")) {
                    let _ = changes.send(change);
                }
                continue;
            }

            let response = match serde_json::from_value::<JsonRpcResponse>(value) {
                Ok(response) => response,
                Err(error) => {
                    notify_all(&pending, McpError::InvalidResponse(error.to_string())).await;
                    break;
                }
            };
            let key = response_key(&response.id);
            if let Some(sender) = pending.lock().await.remove(&key) {
                let _ = sender.send(Ok(response));
            }
        }
        notify_all(
            &pending,
            McpError::Connection("stdio child stdout closed".into()),
        )
        .await;
    });
}

async fn notify_all(pending: &PendingMap, error: McpError) {
    let senders = pending
        .lock()
        .await
        .drain()
        .map(|(_, sender)| sender)
        .collect::<Vec<_>>();
    for sender in senders {
        let _ = sender.send(Err(error.clone()));
    }
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

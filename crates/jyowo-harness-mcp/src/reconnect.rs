use std::{
    future::Future,
    panic::AssertUnwindSafe,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};

use async_trait::async_trait;
use futures::{stream, FutureExt};
use harness_contracts::{
    now, Event, McpConnectionLostEvent, McpConnectionLostReason, McpConnectionRecoveredEvent,
    SessionId,
};
use serde_json::Value;
use tokio::sync::{broadcast, watch, Mutex, Notify, RwLock};

use crate::{
    authorize_mcp_transport,
    registry::{effective_tool_schema_fingerprint, McpSchemaFingerprint},
    ListChangedEvent, McpChange, McpConnectContext, McpConnection, McpError, McpListPage,
    McpMetric, McpMetricConnectionState, McpMetricOutcome, McpMetricsSink, McpPrompt,
    McpPromptMessages, McpReadResourceResult, McpResource, McpServerScope, McpServerSpec,
    McpToolCallStream, McpToolDescriptor, McpToolResult, McpTransport, NoopMcpMetricsSink,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpConnectionState {
    Connecting,
    Ready,
    Reconnecting { attempt: u32, last_error: String },
    Failed { last_error: String },
    Closed,
}

pub trait McpEventSink: Send + Sync + 'static {
    fn emit(&self, event: Event);
}

#[derive(Debug, Default)]
pub struct NoopMcpEventSink;

impl McpEventSink for NoopMcpEventSink {
    fn emit(&self, _event: Event) {}
}

#[derive(Clone)]
pub struct ManagedMcpConnection {
    connection_id: String,
    transport: Arc<dyn McpTransport>,
    spec: McpServerSpec,
    connect_context: McpConnectContext,
    session_id: Option<SessionId>,
    state: Arc<RwLock<McpConnectionState>>,
    connection: Arc<RwLock<Option<Arc<dyn McpConnection>>>>,
    attempts: Arc<AtomicU32>,
    reconnecting: Arc<AtomicBool>,
    reconnect_task: Arc<Mutex<Option<tokio::task::JoinHandle<Result<(), McpError>>>>>,
    closed_notify: Arc<Notify>,
    shutdown_lock: Arc<Mutex<()>>,
    shutdown_started: Arc<AtomicBool>,
    shutdown_completion: watch::Sender<Option<Result<(), McpError>>>,
    downtime_started: Arc<Mutex<Option<Instant>>>,
    schema_fingerprint: Arc<RwLock<Option<McpSchemaFingerprint>>>,
    changes_tx: broadcast::Sender<McpChange>,
    change_generation: Arc<AtomicU64>,
    change_forwarder: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    retired_shutdown_tasks: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    event_sink: Arc<dyn McpEventSink>,
    metrics_sink: Arc<dyn McpMetricsSink>,
}

impl ManagedMcpConnection {
    pub async fn connect(
        transport: Arc<dyn McpTransport>,
        spec: McpServerSpec,
        scope: McpServerScope,
        event_sink: Arc<dyn McpEventSink>,
    ) -> Result<Self, McpError> {
        Self::connect_with_metrics(
            transport,
            spec,
            scope,
            event_sink,
            Arc::new(NoopMcpMetricsSink),
        )
        .await
    }

    pub async fn connect_with_metrics(
        transport: Arc<dyn McpTransport>,
        spec: McpServerSpec,
        scope: McpServerScope,
        event_sink: Arc<dyn McpEventSink>,
        metrics_sink: Arc<dyn McpMetricsSink>,
    ) -> Result<Self, McpError> {
        let context = McpConnectContext::default()
            .with_event_sink(Arc::clone(&event_sink))
            .with_metrics_sink(Arc::clone(&metrics_sink));
        Self::connect_with_context_and_metrics(transport, spec, scope, context).await
    }

    pub async fn connect_with_context_and_metrics(
        transport: Arc<dyn McpTransport>,
        spec: McpServerSpec,
        scope: McpServerScope,
        context: McpConnectContext,
    ) -> Result<Self, McpError> {
        spec.reconnect.validate()?;
        let session_id = session_id_for_scope(&scope);
        let transport_id = transport.transport_id().to_owned();
        let context = if let Some(authorization) = &context.authorization {
            authorize_mcp_transport(authorization, &spec).await?;
            context.with_transport_authorized()
        } else if !matches!(spec.transport, crate::TransportChoice::InProcess) {
            return Err(McpError::PermissionDenied(
                "mcp transport authorization context is required".to_owned(),
            ));
        } else {
            context
        };
        let metrics_sink = context.metrics_sink_or(Arc::new(NoopMcpMetricsSink));
        let event_sink = Arc::clone(&context.event_sink);
        let connection = match transport
            .connect_with_context(spec.clone(), context.clone())
            .await
        {
            Ok(connection) => {
                metrics_sink.record(McpMetric::ConnectionTotal {
                    server_id: spec.server_id.clone(),
                    transport: transport_id,
                    outcome: McpMetricOutcome::Success,
                });
                connection
            }
            Err(error) => {
                metrics_sink.record(McpMetric::ConnectionTotal {
                    server_id: spec.server_id.clone(),
                    transport: transport_id,
                    outcome: McpMetricOutcome::Error,
                });
                metrics_sink.record(McpMetric::ConnectionState {
                    server_id: spec.server_id.clone(),
                    state: McpMetricConnectionState::Failed,
                });
                return Err(error);
            }
        };
        let (changes_tx, _) = broadcast::channel(16);
        let managed = Self {
            connection_id: format!("managed:{}", spec.server_id.0),
            transport,
            spec,
            connect_context: context,
            session_id,
            state: Arc::new(RwLock::new(McpConnectionState::Ready)),
            connection: Arc::new(RwLock::new(Some(Arc::clone(&connection)))),
            attempts: Arc::new(AtomicU32::new(0)),
            reconnecting: Arc::new(AtomicBool::new(false)),
            reconnect_task: Arc::new(Mutex::new(None)),
            closed_notify: Arc::new(Notify::new()),
            shutdown_lock: Arc::new(Mutex::new(())),
            shutdown_started: Arc::new(AtomicBool::new(false)),
            shutdown_completion: watch::channel(None).0,
            downtime_started: Arc::new(Mutex::new(None)),
            schema_fingerprint: Arc::new(RwLock::new(None)),
            changes_tx,
            change_generation: Arc::new(AtomicU64::new(0)),
            change_forwarder: Arc::new(Mutex::new(None)),
            retired_shutdown_tasks: Arc::new(Mutex::new(Vec::new())),
            event_sink,
            metrics_sink,
        };
        managed.replace_change_forwarder(connection).await;
        managed.record_state(McpMetricConnectionState::Ready);
        managed.emit_recovered(true, 0, 0, false);
        Ok(managed)
    }

    pub async fn state(&self) -> McpConnectionState {
        self.state.read().await.clone()
    }

    pub fn attempts_so_far(&self) -> u32 {
        self.attempts.load(Ordering::SeqCst)
    }

    async fn current_connection(&self) -> Result<Arc<dyn McpConnection>, McpError> {
        match &*self.state.read().await {
            McpConnectionState::Ready => {}
            McpConnectionState::Reconnecting { .. } => {
                return Err(McpError::Connection("mcp server reconnecting".into()));
            }
            McpConnectionState::Failed { last_error } => {
                return Err(McpError::Connection(format!(
                    "mcp server failed: {last_error}"
                )));
            }
            McpConnectionState::Closed => {
                return Err(McpError::Connection("mcp server closed".into()));
            }
            McpConnectionState::Connecting => {
                return Err(McpError::Connection("mcp server connecting".into()));
            }
        }

        self.connection
            .read()
            .await
            .clone()
            .ok_or_else(|| McpError::Connection("mcp server missing active connection".into()))
    }

    async fn handle_operation_error(&self, error: McpError) -> McpError {
        if should_reconnect(&error) {
            self.start_reconnect(error.clone()).await;
        }
        error
    }

    async fn start_reconnect(&self, error: McpError) {
        if self.reconnecting.swap(true, Ordering::SeqCst) {
            return;
        }

        let reason = connection_lost_reason(&error);
        let last_error = error.to_string();
        let mut state = self.state.write().await;
        if *state == McpConnectionState::Closed {
            self.reconnecting.store(false, Ordering::SeqCst);
            return;
        }
        self.attempts.store(0, Ordering::SeqCst);
        *self.downtime_started.lock().await = Some(Instant::now());
        *state = McpConnectionState::Reconnecting {
            attempt: 0,
            last_error,
        };
        self.stop_change_forwarder().await;
        if let Some(connection) = self.connection.write().await.take() {
            self.spawn_retired_shutdown(connection).await;
        }
        self.record_state(McpMetricConnectionState::Reconnecting);
        self.emit_lost(reason, 0, false);

        let this = self.clone();
        let reconnect_task = tokio::spawn(async move { this.reconnect_loop().await });
        *self.reconnect_task.lock().await = Some(reconnect_task);
        drop(state);
    }

    async fn reconnect_loop(self) -> Result<(), McpError> {
        loop {
            if self.state().await == McpConnectionState::Closed {
                return Ok(());
            }

            let next_attempt = self.attempts.load(Ordering::SeqCst).saturating_add(1);
            if self
                .spec
                .reconnect
                .is_exhausted(next_attempt.saturating_sub(1))
            {
                self.fail_terminal("reconnect attempts exhausted".to_owned(), next_attempt - 1)
                    .await;
                return Ok(());
            }

            if self
                .run_until_closed(tokio::time::sleep(
                    self.spec.reconnect.backoff_for_attempt(next_attempt),
                ))
                .await
                .is_none()
            {
                return Ok(());
            }
            if self.state().await == McpConnectionState::Closed {
                return Ok(());
            }

            if let Some(authorization) = &self.connect_context.authorization {
                let Some(authorization_result) = self
                    .run_until_closed(authorize_mcp_transport(authorization, &self.spec))
                    .await
                else {
                    return Ok(());
                };
                if let Err(error) = authorization_result {
                    let mut state = self.state.write().await;
                    if *state == McpConnectionState::Closed {
                        return Ok(());
                    }
                    self.record_reconnect_attempt(next_attempt, McpMetricOutcome::Error);
                    let attempts_so_far = next_attempt;
                    self.attempts.store(attempts_so_far, Ordering::SeqCst);
                    let last_error = error.to_string();
                    if self.spec.reconnect.is_exhausted(attempts_so_far) {
                        drop(state);
                        self.fail_terminal(last_error, attempts_so_far).await;
                        return Ok(());
                    }
                    *state = McpConnectionState::Reconnecting {
                        attempt: attempts_so_far,
                        last_error,
                    };
                    continue;
                }
                if self.state().await == McpConnectionState::Closed {
                    return Ok(());
                }
            }

            let Some(connect_result) = self
                .run_until_closed(
                    self.transport
                        .connect_with_context(self.spec.clone(), self.connect_context.clone()),
                )
                .await
            else {
                return Ok(());
            };
            match connect_result {
                Ok(connection) => {
                    if self.state().await == McpConnectionState::Closed {
                        connection.shutdown().await?;
                        return Ok(());
                    }
                    let recovered_fingerprint = tokio::select! {
                        fingerprint = self.recovered_schema_fingerprint(&connection) => fingerprint,
                        () = self.closed_notify.notified() => {
                            connection.shutdown().await?;
                            return Ok(());
                        },
                    };
                    let mut state = self.state.write().await;
                    if *state == McpConnectionState::Closed {
                        drop(state);
                        connection.shutdown().await?;
                        return Ok(());
                    }
                    let schema_changed = if let Some(fingerprint) = recovered_fingerprint {
                        let mut previous = self.schema_fingerprint.write().await;
                        let changed = previous.is_some_and(|value| value != fingerprint);
                        *previous = Some(fingerprint);
                        changed
                    } else {
                        false
                    };
                    *self.connection.write().await = Some(Arc::clone(&connection));
                    self.replace_change_forwarder(connection).await;
                    *state = McpConnectionState::Ready;
                    self.record_reconnect_attempt(next_attempt, McpMetricOutcome::Success);
                    self.record_state(McpMetricConnectionState::Ready);
                    self.reconnecting.store(false, Ordering::SeqCst);
                    self.attempts.store(next_attempt, Ordering::SeqCst);
                    let downtime_ms = self.take_downtime_ms().await;
                    if schema_changed {
                        let _ = self.changes_tx.send(McpChange::ToolsListChanged);
                    }
                    self.emit_recovered(false, downtime_ms, next_attempt, schema_changed);
                    self.spawn_success_reset(next_attempt);
                    drop(state);
                    return Ok(());
                }
                Err(error) => {
                    let mut state = self.state.write().await;
                    if *state == McpConnectionState::Closed {
                        return Ok(());
                    }
                    self.record_reconnect_attempt(next_attempt, McpMetricOutcome::Error);
                    let attempts_so_far = next_attempt;
                    self.attempts.store(attempts_so_far, Ordering::SeqCst);
                    let last_error = error.to_string();
                    if self.spec.reconnect.is_exhausted(attempts_so_far) {
                        drop(state);
                        self.fail_terminal(last_error, attempts_so_far).await;
                        return Ok(());
                    }
                    *state = McpConnectionState::Reconnecting {
                        attempt: attempts_so_far,
                        last_error,
                    };
                }
            }
        }
    }

    async fn run_until_closed<F>(&self, future: F) -> Option<F::Output>
    where
        F: Future,
    {
        tokio::select! {
            output = future => Some(output),
            () = self.closed_notify.notified() => None,
        }
    }

    async fn fail_terminal(&self, last_error: String, attempts_so_far: u32) {
        let mut state = self.state.write().await;
        if *state == McpConnectionState::Closed {
            return;
        }
        *state = McpConnectionState::Failed {
            last_error: last_error.clone(),
        };
        self.record_state(McpMetricConnectionState::Failed);
        *self.connection.write().await = None;
        self.reconnecting.store(false, Ordering::SeqCst);
        self.emit_lost(
            McpConnectionLostReason::Other(last_error),
            attempts_so_far,
            true,
        );
    }

    async fn take_downtime_ms(&self) -> u64 {
        self.downtime_started
            .lock()
            .await
            .take()
            .map(|started| started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or(0)
    }

    async fn recovered_schema_fingerprint(
        &self,
        connection: &Arc<dyn McpConnection>,
    ) -> Option<McpSchemaFingerprint> {
        let Ok(tools) = connection.list_tools().await else {
            return None;
        };
        effective_tool_schema_fingerprint(&self.spec, tools).ok()
    }

    async fn remember_schema(&self, tools: &[McpToolDescriptor]) {
        let Ok(fingerprint) = effective_tool_schema_fingerprint(&self.spec, tools.to_vec()) else {
            return;
        };
        *self.schema_fingerprint.write().await = Some(fingerprint);
    }

    fn spawn_success_reset(&self, attempt_snapshot: u32) {
        let attempts = Arc::clone(&self.attempts);
        let state = Arc::clone(&self.state);
        let reset_after = self.spec.reconnect.success_reset_after;
        tokio::spawn(async move {
            tokio::time::sleep(reset_after).await;
            if *state.read().await == McpConnectionState::Ready
                && attempts.load(Ordering::SeqCst) == attempt_snapshot
            {
                attempts.store(0, Ordering::SeqCst);
            }
        });
    }

    fn emit_lost(&self, reason: McpConnectionLostReason, attempts_so_far: u32, terminal: bool) {
        self.event_sink
            .emit(Event::McpConnectionLost(McpConnectionLostEvent {
                session_id: self.session_id,
                server_id: self.spec.server_id.clone(),
                server_source: self.spec.source.clone(),
                reason,
                attempts_so_far,
                terminal,
                at: now(),
            }));
    }

    fn emit_recovered(
        &self,
        was_first: bool,
        total_downtime_ms: u64,
        attempts_used: u32,
        schema_changed: bool,
    ) {
        self.event_sink
            .emit(Event::McpConnectionRecovered(McpConnectionRecoveredEvent {
                session_id: self.session_id,
                server_id: self.spec.server_id.clone(),
                server_source: self.spec.source.clone(),
                was_first,
                total_downtime_ms,
                attempts_used,
                schema_changed,
                at: now(),
            }));
    }

    fn record_state(&self, state: McpMetricConnectionState) {
        self.metrics_sink.record(McpMetric::ConnectionState {
            server_id: self.spec.server_id.clone(),
            state,
        });
    }

    fn record_reconnect_attempt(&self, attempt: u32, outcome: McpMetricOutcome) {
        self.metrics_sink.record(McpMetric::ReconnectAttempt {
            server_id: self.spec.server_id.clone(),
            attempt,
            outcome,
        });
    }

    async fn replace_change_forwarder(&self, connection: Arc<dyn McpConnection>) {
        self.stop_change_forwarder().await;
        let generation = self.change_generation.load(Ordering::SeqCst);
        let current_generation = Arc::clone(&self.change_generation);
        let changes_tx = self.changes_tx.clone();
        let forwarder = tokio::spawn(async move {
            let Ok(mut changes) = connection.subscribe_changes().await else {
                return;
            };
            while let Some(change) = futures::StreamExt::next(&mut changes).await {
                if current_generation.load(Ordering::SeqCst) != generation {
                    return;
                }
                let _ = changes_tx.send(change);
            }
        });
        *self.change_forwarder.lock().await = Some(forwarder);
    }

    async fn stop_change_forwarder(&self) {
        self.change_generation.fetch_add(1, Ordering::SeqCst);
        if let Some(forwarder) = self.change_forwarder.lock().await.take() {
            forwarder.abort();
            let _ = forwarder.await;
        }
    }

    async fn spawn_retired_shutdown(&self, connection: Arc<dyn McpConnection>) {
        let task = tokio::spawn(async move {
            let _ = connection.shutdown().await;
        });
        let completed = {
            let mut tasks = self.retired_shutdown_tasks.lock().await;
            let mut pending = Vec::with_capacity(tasks.len().saturating_add(1));
            let mut completed = Vec::new();
            for task in tasks.drain(..) {
                if task.is_finished() {
                    completed.push(task);
                } else {
                    pending.push(task);
                }
            }
            pending.push(task);
            *tasks = pending;
            completed
        };
        for task in completed {
            let _ = task.await;
        }
    }

    async fn stop_retired_shutdown_tasks(&self) {
        let tasks = std::mem::take(&mut *self.retired_shutdown_tasks.lock().await);
        for task in tasks {
            task.abort();
            let _ = task.await;
        }
    }
}

#[async_trait]
impl McpConnection for ManagedMcpConnection {
    fn connection_id(&self) -> &str {
        &self.connection_id
    }

    async fn connection_state(&self) -> McpConnectionState {
        self.state().await
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        let connection = self.current_connection().await?;
        match connection.list_tools().await {
            Ok(result) => {
                self.remember_schema(&result).await;
                Ok(result)
            }
            Err(error) => Err(self.handle_operation_error(error).await),
        }
    }

    async fn list_tools_page(
        &self,
        cursor: Option<&str>,
    ) -> Result<McpListPage<McpToolDescriptor>, McpError> {
        let connection = self.current_connection().await?;
        match connection.list_tools_page(cursor).await {
            Ok(result) => Ok(result),
            Err(error) => Err(self.handle_operation_error(error).await),
        }
    }

    async fn call_tool(&self, name: &str, args: Value) -> Result<McpToolResult, McpError> {
        let connection = self.current_connection().await?;
        match connection.call_tool(name, args).await {
            Ok(result) => Ok(result),
            Err(error) => Err(self.handle_operation_error(error).await),
        }
    }

    async fn call_tool_events(
        &self,
        name: &str,
        args: Value,
    ) -> Result<McpToolCallStream, McpError> {
        let connection = self.current_connection().await?;
        match connection.call_tool_events(name, args).await {
            Ok(result) => Ok(result),
            Err(error) => Err(self.handle_operation_error(error).await),
        }
    }

    async fn call_tool_events_for_request(
        &self,
        client_request_id: &str,
        name: &str,
        args: Value,
    ) -> Result<McpToolCallStream, McpError> {
        let connection = self.current_connection().await?;
        match connection
            .call_tool_events_for_request(client_request_id, name, args)
            .await
        {
            Ok(result) => Ok(result),
            Err(error) => Err(self.handle_operation_error(error).await),
        }
    }

    async fn cancel_tool_call(
        &self,
        request_id: &str,
        reason: Option<String>,
    ) -> Result<(), McpError> {
        self.current_connection()
            .await?
            .cancel_tool_call(request_id, reason)
            .await
    }

    async fn mark_unhealthy(&self, reason: String) -> Result<(), McpError> {
        self.start_reconnect(McpError::Connection(reason)).await;
        Ok(())
    }

    async fn list_resources(&self) -> Result<Vec<McpResource>, McpError> {
        let connection = self.current_connection().await?;
        match connection.list_resources().await {
            Ok(result) => Ok(result),
            Err(error) => Err(self.handle_operation_error(error).await),
        }
    }

    async fn list_resources_page(
        &self,
        cursor: Option<&str>,
    ) -> Result<McpListPage<McpResource>, McpError> {
        let connection = self.current_connection().await?;
        match connection.list_resources_page(cursor).await {
            Ok(result) => Ok(result),
            Err(error) => Err(self.handle_operation_error(error).await),
        }
    }

    async fn read_resource(&self, uri: &str) -> Result<McpReadResourceResult, McpError> {
        let connection = self.current_connection().await?;
        match connection.read_resource(uri).await {
            Ok(result) => Ok(result),
            Err(error) => Err(self.handle_operation_error(error).await),
        }
    }

    async fn list_prompts(&self) -> Result<Vec<McpPrompt>, McpError> {
        let connection = self.current_connection().await?;
        match connection.list_prompts().await {
            Ok(result) => Ok(result),
            Err(error) => Err(self.handle_operation_error(error).await),
        }
    }

    async fn list_prompts_page(
        &self,
        cursor: Option<&str>,
    ) -> Result<McpListPage<McpPrompt>, McpError> {
        let connection = self.current_connection().await?;
        match connection.list_prompts_page(cursor).await {
            Ok(result) => Ok(result),
            Err(error) => Err(self.handle_operation_error(error).await),
        }
    }

    async fn get_prompt(&self, name: &str, args: Value) -> Result<McpPromptMessages, McpError> {
        let connection = self.current_connection().await?;
        match connection.get_prompt(name, args).await {
            Ok(result) => Ok(result),
            Err(error) => Err(self.handle_operation_error(error).await),
        }
    }

    async fn subscribe_changes(&self) -> Result<ListChangedEvent, McpError> {
        let receiver = self.changes_tx.subscribe();
        let changes = stream::unfold(receiver, |mut receiver| async move {
            loop {
                match receiver.recv().await {
                    Ok(change) => return Some((change, receiver)),
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        });
        Ok(Box::pin(changes))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        {
            let _shutdown = self.shutdown_lock.lock().await;
            if !self.shutdown_started.swap(true, Ordering::SeqCst) {
                let this = self.clone();
                tokio::spawn(async move {
                    let result = AssertUnwindSafe(this.perform_shutdown())
                        .catch_unwind()
                        .await
                        .unwrap_or_else(|_| {
                            Err(McpError::Connection(
                                "mcp shutdown cleanup panicked".to_owned(),
                            ))
                        });
                    this.shutdown_completion.send_replace(Some(result));
                });
            }
        }

        let mut completion = self.shutdown_completion.subscribe();
        loop {
            if let Some(result) = completion.borrow().clone() {
                return result;
            }
            completion.changed().await.map_err(|_| {
                McpError::Connection("mcp shutdown completion channel closed".to_owned())
            })?;
        }
    }
}

impl ManagedMcpConnection {
    async fn perform_shutdown(&self) -> Result<(), McpError> {
        let mut state = self.state.write().await;
        *state = McpConnectionState::Closed;
        self.closed_notify.notify_one();
        self.record_state(McpMetricConnectionState::Closed);
        self.reconnecting.store(false, Ordering::SeqCst);
        let reconnect_task = self.reconnect_task.lock().await.take();
        drop(state);

        self.stop_change_forwarder().await;
        self.stop_retired_shutdown_tasks().await;

        let connection_result = match self.connection.write().await.take() {
            Some(connection) => connection.shutdown().await,
            None => Ok(()),
        };
        let reconnect_result = match reconnect_task {
            Some(task) => match task.await {
                Ok(result) => result,
                Err(error) => Err(McpError::Connection(format!(
                    "mcp reconnect task failed during shutdown: {error}"
                ))),
            },
            None => Ok(()),
        };
        let result = connection_result.and(reconnect_result);
        result
    }
}

fn session_id_for_scope(scope: &McpServerScope) -> Option<SessionId> {
    match scope {
        McpServerScope::Session(session_id) => Some(*session_id),
        McpServerScope::Global | McpServerScope::Agent(_) => None,
        _ => None,
    }
}

fn should_reconnect(error: &McpError) -> bool {
    matches!(error, McpError::Connection(_) | McpError::Transport(_))
}

fn connection_lost_reason(error: &McpError) -> McpConnectionLostReason {
    match error {
        McpError::Transport(message) => McpConnectionLostReason::Network(message.clone()),
        McpError::Connection(message) => McpConnectionLostReason::Other(message.clone()),
        _ => McpConnectionLostReason::Other(error.to_string()),
    }
}

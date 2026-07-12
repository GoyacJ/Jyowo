use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};

use async_trait::async_trait;
use harness_contracts::{
    now, ElicitationOutcome, ElicitationSchemaSummary, Event, McpElicitationRequestedEvent,
    McpElicitationResolvedEvent, McpServerId, PermissionMode, RequestId, RunId, SessionId,
};
use serde_json::Value;
use tokio::sync::{oneshot, Mutex};

use crate::{JsonRpcError, JsonRpcRequest, McpEventSink, NoopMcpEventSink};

pub const MCP_ELICITATION_REQUIRED_CODE: i32 = -32042;

#[derive(Debug, Clone, PartialEq)]
pub struct ElicitationRequest {
    pub request_id: RequestId,
    pub server_id: McpServerId,
    pub schema: Value,
    pub subject: String,
    pub detail: Option<String>,
    pub timeout: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ElicitationError {
    #[error("user declined elicitation")]
    UserDeclined,
    #[error("elicitation timed out")]
    Timeout,
    #[error("invalid elicitation: {0}")]
    Invalid(String),
    #[error("no elicitation handler registered")]
    NoHandlerRegistered,
}

#[async_trait]
pub trait ElicitationHandler: Send + Sync + 'static {
    fn handler_id(&self) -> &str;

    async fn handle(&self, request: ElicitationRequest) -> Result<Value, ElicitationError>;
}

/// Routing boundary used by [`McpPeer`](crate::McpPeer) for server-initiated elicitation.
///
/// Wire-model decoding remains the responsibility of the installed handler until the
/// 2025-11-25 elicitation model is migrated.
#[async_trait]
pub trait ElicitationRequestRouter: Send + Sync + 'static {
    async fn route_elicitation_request(
        &self,
        request: JsonRpcRequest,
    ) -> Result<Value, JsonRpcError>;
}

#[derive(Debug, Clone, Default)]
pub struct RejectAllElicitationHandler;

#[async_trait]
impl ElicitationHandler for RejectAllElicitationHandler {
    fn handler_id(&self) -> &'static str {
        "reject-all"
    }

    async fn handle(&self, _request: ElicitationRequest) -> Result<Value, ElicitationError> {
        Err(ElicitationError::UserDeclined)
    }
}

#[derive(Clone)]
pub struct DirectElicitationHandler<F> {
    handler_id: String,
    handler: F,
}

impl<F> DirectElicitationHandler<F> {
    pub fn new<Fut>(handler: F) -> Self
    where
        F: Fn(ElicitationRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, ElicitationError>> + Send,
    {
        Self {
            handler_id: "direct".to_owned(),
            handler,
        }
    }

    #[must_use]
    pub fn with_handler_id(mut self, handler_id: impl Into<String>) -> Self {
        self.handler_id = handler_id.into();
        self
    }
}

#[async_trait]
impl<F, Fut> ElicitationHandler for DirectElicitationHandler<F>
where
    F: Fn(ElicitationRequest) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Value, ElicitationError>> + Send,
{
    fn handler_id(&self) -> &str {
        &self.handler_id
    }

    async fn handle(&self, request: ElicitationRequest) -> Result<Value, ElicitationError> {
        (self.handler)(request).await
    }
}

#[derive(Clone)]
pub struct StreamElicitationHandler {
    session_id: SessionId,
    run_id: Option<RunId>,
    event_sink: Arc<dyn McpEventSink>,
    pending: Arc<Mutex<HashMap<RequestId, oneshot::Sender<Result<Value, ElicitationError>>>>>,
}

impl Default for StreamElicitationHandler {
    fn default() -> Self {
        Self::new(SessionId::default(), None, Arc::new(NoopMcpEventSink))
    }
}

impl StreamElicitationHandler {
    pub fn new(
        session_id: SessionId,
        run_id: Option<RunId>,
        event_sink: Arc<dyn McpEventSink>,
    ) -> Self {
        Self {
            session_id,
            run_id,
            event_sink,
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn resolve_elicitation(
        &self,
        request_id: RequestId,
        value: Value,
    ) -> Result<(), ElicitationError> {
        self.complete(request_id, Ok(value)).await
    }

    pub async fn reject_elicitation(
        &self,
        request_id: RequestId,
        _reason: impl Into<String>,
    ) -> Result<(), ElicitationError> {
        self.complete(request_id, Err(ElicitationError::UserDeclined))
            .await
    }

    async fn complete(
        &self,
        request_id: RequestId,
        result: Result<Value, ElicitationError>,
    ) -> Result<(), ElicitationError> {
        let Some(sender) = self.pending.lock().await.remove(&request_id) else {
            return Err(ElicitationError::Invalid(format!(
                "unknown elicitation request: {request_id}"
            )));
        };
        sender
            .send(result)
            .map_err(|_| ElicitationError::Invalid("elicitation receiver closed".to_owned()))
    }

    fn emit_requested(&self, request: &ElicitationRequest) {
        self.event_sink.emit(Event::McpElicitationRequested(
            McpElicitationRequestedEvent {
                session_id: self.session_id,
                run_id: self.run_id,
                server_id: request.server_id.clone(),
                request_id: request.request_id,
                subject: request.subject.clone(),
                schema_summary: summarize_elicitation_schema(&request.schema),
                timeout: request.timeout,
                at: now(),
            },
        ));
    }

    fn emit_resolved(&self, request: &ElicitationRequest, outcome: ElicitationOutcome) {
        self.event_sink
            .emit(Event::McpElicitationResolved(McpElicitationResolvedEvent {
                session_id: self.session_id,
                run_id: self.run_id,
                server_id: request.server_id.clone(),
                request_id: request.request_id,
                outcome,
                at: now(),
            }));
    }
}

#[async_trait]
impl ElicitationHandler for StreamElicitationHandler {
    fn handler_id(&self) -> &'static str {
        "stream"
    }

    async fn handle(&self, request: ElicitationRequest) -> Result<Value, ElicitationError> {
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(request.request_id, sender);
        self.emit_requested(&request);

        let result = if let Some(timeout) = request.timeout {
            match tokio::time::timeout(timeout, receiver).await {
                Ok(Ok(result)) => result,
                Ok(Err(_closed)) => Err(ElicitationError::NoHandlerRegistered),
                Err(_elapsed) => {
                    self.pending.lock().await.remove(&request.request_id);
                    Err(ElicitationError::Timeout)
                }
            }
        } else {
            receiver
                .await
                .unwrap_or(Err(ElicitationError::NoHandlerRegistered))
        };

        self.emit_resolved(&request, outcome_for_result(&result));
        result
    }
}

pub fn summarize_elicitation_schema(schema: &Value) -> ElicitationSchemaSummary {
    let properties = schema.get("properties").and_then(Value::as_object);
    let required = schema.get("required").and_then(Value::as_array);
    let has_secret_field = properties
        .map(|fields| fields.keys().any(|name| is_secret_field(name)))
        .unwrap_or(false);
    ElicitationSchemaSummary {
        field_count: properties.map_or(0, |fields| fields.len().min(u16::MAX as usize) as u16),
        required_count: required.map_or(0, |fields| fields.len().min(u16::MAX as usize) as u16),
        has_secret_field,
    }
}

pub fn elicitation_from_jsonrpc_error(error: &JsonRpcError) -> Option<ElicitationRequest> {
    if error.code != MCP_ELICITATION_REQUIRED_CODE {
        return None;
    }
    let data = error.data.as_ref()?;
    let request_id = serde_json::from_value(data.get("request_id")?.clone()).ok()?;
    let server_id = McpServerId(data.get("server_id")?.as_str()?.to_owned());
    let subject = data.get("subject")?.as_str()?.to_owned();
    let schema = data.get("schema").cloned().unwrap_or(Value::Null);
    let detail = data
        .get("detail")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let timeout = data
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .map(Duration::from_millis);
    Some(ElicitationRequest {
        request_id,
        server_id,
        schema,
        subject,
        detail,
        timeout,
    })
}

pub async fn handle_jsonrpc_elicitation_error<H>(
    error: &JsonRpcError,
    permission_mode: PermissionMode,
    handler: Arc<H>,
) -> Result<Option<Value>, ElicitationError>
where
    H: ElicitationHandler + ?Sized,
{
    let Some(request) = elicitation_from_jsonrpc_error(error) else {
        return Ok(None);
    };

    if matches!(
        permission_mode,
        PermissionMode::BypassPermissions | PermissionMode::DontAsk
    ) {
        return RejectAllElicitationHandler.handle(request).await.map(Some);
    }

    handler.handle(request).await.map(Some)
}

fn outcome_for_result(result: &Result<Value, ElicitationError>) -> ElicitationOutcome {
    match result {
        Ok(value) => ElicitationOutcome::Provided {
            value_hash: blake3::hash(value.to_string().as_bytes()).into(),
        },
        Err(ElicitationError::UserDeclined) => ElicitationOutcome::UserDeclined,
        Err(ElicitationError::Timeout) => ElicitationOutcome::Timeout,
        Err(ElicitationError::Invalid(reason)) => ElicitationOutcome::Invalid {
            reason: reason.clone(),
        },
        Err(ElicitationError::NoHandlerRegistered) => ElicitationOutcome::NoHandlerRegistered,
    }
}

fn is_secret_field(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    [
        "secret",
        "token",
        "password",
        "api_key",
        "apikey",
        "credential",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};

use async_trait::async_trait;
use harness_contracts::{
    now, ElicitationOutcome, ElicitationSchemaSummary, Event, McpElicitationRequestedEvent,
    McpElicitationResolvedEvent, McpServerId, PermissionMode, RequestId, RunId, SessionId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
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
    pub mode: ElicitationMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElicitationMode {
    Form,
    Url { elicitation_id: String, url: String },
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ElicitRequestParams {
    Form {
        message: String,
        #[serde(rename = "requestedSchema")]
        requested_schema: Value,
        #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
        extra: Map<String, Value>,
    },
    Url(ElicitUrlRequestParams),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitUrlRequestParams {
    pub message: String,
    pub elicitation_id: String,
    pub url: String,
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

#[derive(Clone)]
pub struct ElicitationJsonRpcHandler {
    server_id: McpServerId,
    permission_mode: PermissionMode,
    handler: Arc<dyn ElicitationHandler>,
    timeout: Option<Duration>,
}

impl ElicitationJsonRpcHandler {
    pub fn new(
        server_id: McpServerId,
        permission_mode: PermissionMode,
        handler: Arc<dyn ElicitationHandler>,
    ) -> Self {
        Self {
            server_id,
            permission_mode,
            handler,
            timeout: None,
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub async fn route_elicitation_request(
        &self,
        request: JsonRpcRequest,
    ) -> Result<Value, JsonRpcError> {
        if request.method != "elicitation/create" {
            return Err(elicitation_error(-32601, "method not found"));
        }
        let params = request
            .params
            .ok_or_else(|| elicitation_error(-32602, "elicitation/create missing params"))?;
        let params = normalize_form_mode(params)?;
        let wire: ElicitRequestParams = serde_json::from_value(params).map_err(|error| {
            elicitation_error(
                -32602,
                &format!("invalid elicitation/create params: {error}"),
            )
        })?;
        let (subject, schema, mode) = match wire {
            ElicitRequestParams::Form {
                message,
                requested_schema,
                ..
            } => {
                validate_form_schema(&requested_schema)?;
                (message, requested_schema, ElicitationMode::Form)
            }
            ElicitRequestParams::Url(params) => {
                if url::Url::parse(&params.url).is_err() {
                    return Err(elicitation_error(
                        -32602,
                        "elicitation URL must be absolute",
                    ));
                }
                let mode = ElicitationMode::Url {
                    elicitation_id: params.elicitation_id,
                    url: params.url,
                };
                (params.message, Value::Null, mode)
            }
        };
        let form = matches!(mode, ElicitationMode::Form);
        let request = ElicitationRequest {
            request_id: RequestId::new(),
            server_id: self.server_id.clone(),
            schema,
            subject,
            detail: None,
            timeout: self.timeout,
            mode,
        };
        if matches!(
            self.permission_mode,
            PermissionMode::BypassPermissions | PermissionMode::DontAsk
        ) {
            return Ok(json!({ "action": "decline" }));
        }
        match self.handler.handle(request).await {
            Ok(content) if form => {
                validate_form_content(&content)?;
                Ok(json!({ "action": "accept", "content": content }))
            }
            Ok(_) => Ok(json!({ "action": "accept" })),
            Err(ElicitationError::UserDeclined) => Ok(json!({ "action": "decline" })),
            Err(ElicitationError::Timeout) => Ok(json!({ "action": "cancel" })),
            Err(ElicitationError::Invalid(message)) => Err(elicitation_error(-32602, &message)),
            Err(ElicitationError::NoHandlerRegistered) => Err(elicitation_error(
                -32601,
                "no elicitation handler registered",
            )),
        }
    }
}

#[async_trait]
impl ElicitationRequestRouter for ElicitationJsonRpcHandler {
    async fn route_elicitation_request(
        &self,
        request: JsonRpcRequest,
    ) -> Result<Value, JsonRpcError> {
        ElicitationJsonRpcHandler::route_elicitation_request(self, request).await
    }
}

fn normalize_form_mode(mut params: Value) -> Result<Value, JsonRpcError> {
    let object = params
        .as_object_mut()
        .ok_or_else(|| elicitation_error(-32602, "elicitation params must be an object"))?;
    object
        .entry("mode")
        .or_insert_with(|| Value::String("form".to_owned()));
    Ok(params)
}

fn validate_form_schema(schema: &Value) -> Result<(), JsonRpcError> {
    if schema.get("type").and_then(Value::as_str) != Some("object")
        || !schema.get("properties").is_some_and(Value::is_object)
    {
        return Err(elicitation_error(
            -32602,
            "elicitation requestedSchema must be an object schema",
        ));
    }
    Ok(())
}

fn validate_form_content(content: &Value) -> Result<(), JsonRpcError> {
    let object = content
        .as_object()
        .ok_or_else(|| elicitation_error(-32602, "accepted form content must be an object"))?;
    if object.values().all(|value| {
        value.is_string()
            || value.is_number()
            || value.is_boolean()
            || value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string))
    }) {
        Ok(())
    } else {
        Err(elicitation_error(
            -32602,
            "elicitation form content contains a non-primitive value",
        ))
    }
}

fn elicitation_error(code: i32, message: &str) -> JsonRpcError {
    JsonRpcError {
        code,
        message: message.to_owned(),
        data: None,
        extra: Default::default(),
    }
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

pub fn url_elicitations_from_jsonrpc_error(
    error: &JsonRpcError,
) -> Option<Vec<ElicitUrlRequestParams>> {
    if error.code != MCP_ELICITATION_REQUIRED_CODE {
        return None;
    }
    serde_json::from_value(error.data.as_ref()?.get("elicitations")?.clone()).ok()
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

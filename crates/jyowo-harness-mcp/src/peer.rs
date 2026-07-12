use std::{
    any::Any,
    collections::HashMap,
    future::Future,
    panic::AssertUnwindSafe,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
    time::Duration,
};

use async_trait::async_trait;
use futures::{
    future::{AbortHandle, Abortable},
    FutureExt,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{oneshot, Notify, Semaphore};

use crate::{
    ElicitationRequestRouter, JsonRpcError, JsonRpcNotification, JsonRpcRequest, McpError,
    McpLifecycleState, McpMessage, McpMessageSink, McpOutboundMessage, McpSession,
    SamplingRequestRouter,
};

const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;
const INTERNAL_ERROR: i32 = -32603;
const DEFAULT_MAX_PENDING: usize = 1_024;
const DEFAULT_MAX_INBOUND_HANDLERS: usize = 16;
const CANCEL_SEND_TIMEOUT: Duration = Duration::from_millis(250);

tokio::task_local! {
    static CURRENT_INBOUND_TASK: InboundTaskContext;
}

#[derive(Clone, Copy)]
struct InboundTaskContext {
    peer_id: usize,
    task_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpRoot {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[async_trait]
pub trait McpRootsListHandler: Send + Sync + 'static {
    async fn list_roots(&self) -> Result<Vec<McpRoot>, JsonRpcError>;
}

#[async_trait]
pub trait McpNotificationHandler: Send + Sync + 'static {
    async fn handle_notification(&self, notification: JsonRpcNotification) -> Result<(), McpError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpInboundOutcome {
    ResponseResolved,
    UnknownResponse,
    RequestHandled,
    NotificationHandled,
    NotificationIgnored,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RequestIdKey {
    Number(u64),
    Signed(i64),
    String(String),
}

type PendingResult = Result<Value, McpError>;

#[derive(Clone)]
struct McpInboundRouter {
    sink: Arc<dyn McpMessageSink>,
    session: McpSession,
    closed: Arc<AtomicBool>,
    sampling_handler: Option<Arc<dyn SamplingRequestRouter>>,
    elicitation_handler: Option<Arc<dyn ElicitationRequestRouter>>,
    roots_handler: Option<Arc<dyn McpRootsListHandler>>,
    notification_handlers: HashMap<String, Arc<dyn McpNotificationHandler>>,
}

struct McpPeerInner {
    inbound_router: McpInboundRouter,
    next_request_id: AtomicU64,
    pending: Mutex<HashMap<RequestIdKey, oneshot::Sender<PendingResult>>>,
    max_pending: usize,
    inbound_permits: Arc<Semaphore>,
    next_inbound_task_id: AtomicU64,
    inbound_tasks: Mutex<HashMap<u64, AbortHandle>>,
    inbound_tasks_changed: Notify,
}

impl Drop for McpPeerInner {
    fn drop(&mut self) {
        self.inbound_router.closed.store(true, Ordering::Release);
        let error = McpError::Connection("MCP peer dropped".to_owned());
        if let Ok(pending) = self.pending.get_mut() {
            for (_, sender) in pending.drain() {
                let _ = sender.send(Err(error.clone()));
            }
        }
        if let Ok(tasks) = self.inbound_tasks.get_mut() {
            for (_, task) in tasks.drain() {
                task.abort();
            }
        }
    }
}

struct PendingGuard {
    inner: Arc<McpPeerInner>,
    key: RequestIdKey,
    request_id: Value,
    wire_committed: Arc<AtomicBool>,
}

struct InboundTaskGuard {
    inner: Weak<McpPeerInner>,
    task_id: u64,
}

impl Drop for InboundTaskGuard {
    fn drop(&mut self) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        if let Ok(mut tasks) = inner.inbound_tasks.lock() {
            tasks.remove(&self.task_id);
        }
        inner.inbound_tasks_changed.notify_waiters();
    }
}

impl PendingGuard {
    fn remove(&self) -> bool {
        self.inner
            .pending
            .lock()
            .is_ok_and(|mut pending| pending.remove(&self.key).is_some())
    }

    fn cancel_with_reason(&self, reason: &'static str) {
        if !self.remove()
            || !self.wire_committed.load(Ordering::Acquire)
            || self.inner.inbound_router.closed.load(Ordering::Acquire)
        {
            return;
        }
        let Ok(cancel) = McpOutboundMessage::notification(
            "notifications/cancelled",
            json!({
                "requestId": self.request_id,
                "reason": reason
            }),
        ) else {
            return;
        };
        let sink = Arc::clone(&self.inner.inbound_router.sink);
        let inner = Arc::downgrade(&self.inner);
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        std::mem::drop(runtime.spawn(async move {
            let result =
                AssertUnwindSafe(tokio::time::timeout(CANCEL_SEND_TIMEOUT, sink.send(cancel)))
                    .catch_unwind()
                    .await;
            let close_reason = match result {
                Ok(Ok(Ok(()))) => None,
                Ok(Ok(Err(error))) => Some(format!("MCP cancellation send failed: {error}")),
                Ok(Err(_)) => Some(format!(
                    "MCP cancellation send timed out after {} ms",
                    CANCEL_SEND_TIMEOUT.as_millis()
                )),
                Err(payload) => Some(format!(
                    "MCP cancellation send panicked: {}",
                    panic_payload_message(payload.as_ref())
                )),
            };
            if let Some(reason) = close_reason {
                if let Some(inner) = inner.upgrade() {
                    McpPeer { inner }.close(reason).await;
                }
            }
        }));
    }
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        self.cancel_with_reason("client request cancelled");
    }
}

#[derive(Clone)]
pub struct McpPeer {
    inner: Arc<McpPeerInner>,
}

impl McpPeer {
    pub fn builder(sink: Arc<dyn McpMessageSink>, session: McpSession) -> McpPeerBuilder {
        McpPeerBuilder {
            sink,
            session,
            max_pending: DEFAULT_MAX_PENDING,
            max_inbound_handlers: DEFAULT_MAX_INBOUND_HANDLERS,
            sampling_handler: None,
            elicitation_handler: None,
            roots_handler: None,
            notification_handlers: HashMap::new(),
        }
    }

    #[must_use]
    pub fn session(&self) -> &McpSession {
        &self.inner.inbound_router.session
    }

    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.inner
            .pending
            .lock()
            .map(|pending| pending.len())
            .unwrap_or_default()
    }

    pub async fn request(
        &self,
        method: impl Into<String>,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, McpError> {
        if self.inner.inbound_router.closed.load(Ordering::Acquire) {
            return Err(McpError::Connection("MCP peer is closed".to_owned()));
        }
        let request_id = self
            .inner
            .next_request_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| McpError::Connection("MCP request id space exhausted".to_owned()))?;
        let id = Value::from(request_id);
        let key = RequestIdKey::Number(request_id);
        let message = McpOutboundMessage::request(id.clone(), method, params)?;
        let (sender, mut receiver) = oneshot::channel();
        let wire_committed = Arc::new(AtomicBool::new(false));

        {
            let mut pending = self
                .inner
                .pending
                .lock()
                .map_err(|_| McpError::Connection("MCP pending map poisoned".to_owned()))?;
            if pending.len() >= self.inner.max_pending {
                return Err(McpError::Connection(format!(
                    "MCP pending request limit reached ({})",
                    self.inner.max_pending
                )));
            }
            if self.inner.inbound_router.closed.load(Ordering::Acquire) {
                return Err(McpError::Connection("MCP peer is closed".to_owned()));
            }
            pending.insert(key.clone(), sender);
        }
        let pending_guard = PendingGuard {
            inner: Arc::clone(&self.inner),
            key: key.clone(),
            request_id: id.clone(),
            wire_committed: Arc::clone(&wire_committed),
        };

        let operation = async {
            tokio::select! {
                biased;
                result = &mut receiver => decode_pending_result(result),
                send_result = self.inner.inbound_router.sink.send(message) => {
                    send_result?;
                    wire_committed.store(true, Ordering::Release);
                    decode_pending_result(receiver.await)
                }
            }
        };
        match tokio::time::timeout(timeout, operation).await {
            Ok(result) => result,
            Err(_) => {
                pending_guard.cancel_with_reason("client request timed out");
                Err(McpError::Connection(format!(
                    "MCP request timed out after {} ms",
                    timeout.as_millis()
                )))
            }
        }
    }

    pub async fn notify(&self, method: impl Into<String>, params: Value) -> Result<(), McpError> {
        if self.inner.inbound_router.closed.load(Ordering::Acquire) {
            return Err(McpError::Connection("MCP peer is closed".to_owned()));
        }
        let message = McpOutboundMessage::notification(method, params)?;
        self.inner.inbound_router.sink.send(message).await
    }

    pub async fn receive(&self, message: McpMessage) -> Result<McpInboundOutcome, McpError> {
        if self.inner.inbound_router.closed.load(Ordering::Acquire) {
            return Err(McpError::Connection("MCP peer is closed".to_owned()));
        }
        match message {
            McpMessage::SuccessResponse(response) => {
                self.resolve_response(response.id, Ok(response.result))
            }
            McpMessage::ErrorResponse(response) => {
                let Some(id) = response.id else {
                    return Ok(McpInboundOutcome::UnknownResponse);
                };
                self.resolve_response(id, Err(McpError::RemoteJsonRpc(response.error)))
            }
            McpMessage::Request(request) => {
                let router = self.inner.inbound_router.clone();
                self.dispatch_inbound(async move { router.route_request(request).await })?;
                Ok(McpInboundOutcome::RequestHandled)
            }
            McpMessage::Notification(notification) => {
                let handler = self
                    .inner
                    .inbound_router
                    .notification_handlers
                    .get(&notification.method)
                    .cloned();
                let Some(handler) = handler else {
                    return Ok(McpInboundOutcome::NotificationIgnored);
                };
                self.dispatch_inbound(
                    async move { handler.handle_notification(notification).await },
                )?;
                Ok(McpInboundOutcome::NotificationHandled)
            }
        }
    }

    pub async fn close(&self, reason: impl Into<String>) {
        let first_close = !self
            .inner
            .inbound_router
            .closed
            .swap(true, Ordering::AcqRel);
        if first_close {
            let error = McpError::Connection(reason.into());
            let senders = match self.inner.pending.lock() {
                Ok(mut pending) => pending.drain().map(|(_, sender)| sender).collect(),
                Err(_) => Vec::new(),
            };
            for sender in senders {
                let _ = sender.send(Err(error.clone()));
            }
        }
        let peer_id = Arc::as_ptr(&self.inner) as usize;
        let current_task_id = CURRENT_INBOUND_TASK
            .try_with(|task| (task.peer_id == peer_id).then_some(task.task_id))
            .ok()
            .flatten();
        self.abort_inbound_tasks(current_task_id).await;
    }

    fn dispatch_inbound<F>(&self, future: F) -> Result<(), McpError>
    where
        F: Future<Output = Result<(), McpError>> + Send + 'static,
    {
        let permit = Arc::clone(&self.inner.inbound_permits)
            .try_acquire_owned()
            .map_err(|_| {
                McpError::Connection("MCP inbound handler concurrency limit reached".to_owned())
            })?;
        let task_id = self
            .inner
            .next_inbound_task_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| McpError::Connection("MCP inbound task id space exhausted".to_owned()))?;
        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        {
            let mut tasks =
                self.inner.inbound_tasks.lock().map_err(|_| {
                    McpError::Connection("MCP inbound task map poisoned".to_owned())
                })?;
            if self.inner.inbound_router.closed.load(Ordering::Acquire) {
                return Err(McpError::Connection("MCP peer is closed".to_owned()));
            }
            tasks.insert(task_id, abort_handle);
        }

        let inner = Arc::downgrade(&self.inner);
        let guard_inner = inner.clone();
        let peer_id = Arc::as_ptr(&self.inner) as usize;
        std::mem::drop(tokio::spawn(CURRENT_INBOUND_TASK.scope(
            InboundTaskContext { peer_id, task_id },
            async move {
                let guard = InboundTaskGuard {
                    inner: guard_inner,
                    task_id,
                };
                let result = AssertUnwindSafe(Abortable::new(
                    async move {
                        let _permit = permit;
                        future.await
                    },
                    abort_registration,
                ))
                .catch_unwind()
                .await;
                drop(guard);
                let close_reason = match result {
                    Ok(Ok(Err(error))) => Some(format!("MCP inbound handler failed: {error}")),
                    Err(payload) => Some(format!(
                        "MCP inbound handler panicked: {}",
                        panic_payload_message(payload.as_ref())
                    )),
                    Ok(Ok(Ok(())) | Err(_)) => None,
                };
                if let Some(reason) = close_reason {
                    if let Some(inner) = inner.upgrade() {
                        McpPeer { inner }.close(reason).await;
                    }
                }
            },
        )));
        Ok(())
    }

    async fn abort_inbound_tasks(&self, excluded_task_id: Option<u64>) {
        loop {
            let changed = self.inner.inbound_tasks_changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let empty = match self.inner.inbound_tasks.lock() {
                Ok(tasks) => {
                    let mut has_other_tasks = false;
                    for (task_id, task) in tasks.iter() {
                        if Some(*task_id) == excluded_task_id {
                            continue;
                        }
                        has_other_tasks = true;
                        task.abort();
                    }
                    !has_other_tasks
                }
                Err(_) => true,
            };
            if empty {
                return;
            }
            changed.await;
        }
    }

    fn resolve_response(
        &self,
        id: Value,
        result: PendingResult,
    ) -> Result<McpInboundOutcome, McpError> {
        let Some(key) = request_id_key(&id) else {
            return Ok(McpInboundOutcome::UnknownResponse);
        };
        let sender = self
            .inner
            .pending
            .lock()
            .map_err(|_| McpError::Connection("MCP pending map poisoned".to_owned()))?
            .remove(&key);
        let Some(sender) = sender else {
            return Ok(McpInboundOutcome::UnknownResponse);
        };
        let _ = sender.send(result);
        Ok(McpInboundOutcome::ResponseResolved)
    }
}

impl McpInboundRouter {
    async fn route_request(&self, request: JsonRpcRequest) -> Result<(), McpError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(McpError::Connection("MCP peer is closed".to_owned()));
        }
        let id = request.id.clone();
        let result = match request.method.as_str() {
            "ping" => Ok(json!({})),
            "sampling/createMessage" if self.sampling_advertised() => {
                match self.sampling_handler.clone() {
                    Some(handler) => handler.route_sampling_request(request).await,
                    None => Err(method_not_found(
                        "sampling/createMessage",
                        "handler not installed",
                    )),
                }
            }
            "elicitation/create" => match self.elicitation_mode_advertised(&request) {
                Ok(true) => match self.elicitation_handler.clone() {
                    Some(handler) => handler.route_elicitation_request(request).await,
                    None => Err(method_not_found(
                        "elicitation/create",
                        "handler not installed",
                    )),
                },
                Ok(false) => Err(method_not_found(
                    "elicitation/create",
                    "elicitation mode not advertised",
                )),
                Err(error) => Err(error),
            },
            "roots/list" if self.roots_advertised() => match self.roots_handler.clone() {
                Some(handler) => handler
                    .list_roots()
                    .await
                    .map(|roots| json!({ "roots": roots })),
                None => Err(method_not_found("roots/list", "handler not installed")),
            },
            method => Err(method_not_found(method, "capability not advertised")),
        };

        let message = match result {
            Ok(result) => McpOutboundMessage::success(id.clone(), result).or_else(|error| {
                McpOutboundMessage::failure(
                    id,
                    JsonRpcError {
                        code: INTERNAL_ERROR,
                        message: error.to_string(),
                        data: None,
                        extra: Default::default(),
                    },
                )
            })?,
            Err(error) => McpOutboundMessage::failure(id, error)?,
        };
        if self.closed.load(Ordering::Acquire) {
            return Err(McpError::Connection("MCP peer is closed".to_owned()));
        }
        self.sink.send(message).await
    }

    fn sampling_advertised(&self) -> bool {
        self.session
            .offered_client_capabilities()
            .sampling
            .is_some()
    }

    fn elicitation_mode_advertised(&self, request: &JsonRpcRequest) -> Result<bool, JsonRpcError> {
        let Some(capability) = self
            .session
            .offered_client_capabilities()
            .elicitation
            .as_ref()
        else {
            return Ok(false);
        };

        let mode = match request
            .params
            .as_ref()
            .and_then(|params| params.get("mode"))
        {
            None => None,
            Some(Value::String(mode)) => Some(mode.as_str()),
            Some(value) => {
                return Err(JsonRpcError {
                    code: INVALID_PARAMS,
                    message: "elicitation mode must be a string".to_owned(),
                    data: Some(json!({ "mode": value })),
                    extra: Default::default(),
                });
            }
        };
        if let Some(unknown) = mode.filter(|mode| !matches!(*mode, "form" | "url")) {
            return Err(JsonRpcError {
                code: INVALID_PARAMS,
                message: format!("unsupported elicitation mode: {unknown}"),
                data: Some(json!({ "mode": unknown })),
                extra: Default::default(),
            });
        }

        match self.session.negotiated_protocol_version() {
            Some("2025-11-25") => match mode.unwrap_or("form") {
                "form" => Ok(capability.form.is_some()),
                "url" => Ok(capability.url.is_some()),
                _ => unreachable!("elicitation mode validated above"),
            },
            Some("2025-06-18") => Ok(mode.is_none()),
            _ => Ok(false),
        }
    }

    fn roots_advertised(&self) -> bool {
        self.session.offered_client_capabilities().roots.is_some()
    }
}

pub struct McpPeerBuilder {
    sink: Arc<dyn McpMessageSink>,
    session: McpSession,
    max_pending: usize,
    max_inbound_handlers: usize,
    sampling_handler: Option<Arc<dyn SamplingRequestRouter>>,
    elicitation_handler: Option<Arc<dyn ElicitationRequestRouter>>,
    roots_handler: Option<Arc<dyn McpRootsListHandler>>,
    notification_handlers: HashMap<String, Arc<dyn McpNotificationHandler>>,
}

impl McpPeerBuilder {
    #[must_use]
    pub fn max_pending(mut self, max_pending: usize) -> Self {
        self.max_pending = max_pending;
        self
    }

    #[must_use]
    pub fn max_inbound_handlers(mut self, max_inbound_handlers: usize) -> Self {
        self.max_inbound_handlers = max_inbound_handlers;
        self
    }

    #[must_use]
    pub fn sampling_handler(mut self, handler: Arc<dyn SamplingRequestRouter>) -> Self {
        self.sampling_handler = Some(handler);
        self
    }

    #[must_use]
    pub fn elicitation_handler(mut self, handler: Arc<dyn ElicitationRequestRouter>) -> Self {
        self.elicitation_handler = Some(handler);
        self
    }

    #[must_use]
    pub fn roots_handler(mut self, handler: Arc<dyn McpRootsListHandler>) -> Self {
        self.roots_handler = Some(handler);
        self
    }

    #[must_use]
    pub fn notification_handler(
        mut self,
        method: impl Into<String>,
        handler: Arc<dyn McpNotificationHandler>,
    ) -> Self {
        self.notification_handlers.insert(method.into(), handler);
        self
    }

    pub fn build(self) -> Result<McpPeer, McpError> {
        if self.session.state() != McpLifecycleState::Ready {
            return Err(McpError::Protocol(
                "MCP peer requires a fully negotiated, ready session".to_owned(),
            ));
        }
        if self.max_pending == 0 {
            return Err(McpError::Protocol(
                "MCP peer pending request limit must be greater than zero".to_owned(),
            ));
        }
        if self.max_inbound_handlers == 0 {
            return Err(McpError::Protocol(
                "MCP peer inbound handler limit must be greater than zero".to_owned(),
            ));
        }
        let capabilities = self.session.offered_client_capabilities();
        ensure_capability_handler_match(
            "sampling",
            capabilities.sampling.is_some(),
            self.sampling_handler.is_some(),
        )?;
        ensure_capability_handler_match(
            "elicitation",
            capabilities.elicitation.is_some(),
            self.elicitation_handler.is_some(),
        )?;
        ensure_capability_handler_match(
            "roots",
            capabilities.roots.is_some(),
            self.roots_handler.is_some(),
        )?;
        if capabilities.tasks.is_some() {
            return Err(McpError::Protocol(
                "MCP client tasks capability has no installed peer router".to_owned(),
            ));
        }
        Ok(McpPeer {
            inner: Arc::new(McpPeerInner {
                inbound_router: McpInboundRouter {
                    sink: self.sink,
                    session: self.session,
                    closed: Arc::new(AtomicBool::new(false)),
                    sampling_handler: self.sampling_handler,
                    elicitation_handler: self.elicitation_handler,
                    roots_handler: self.roots_handler,
                    notification_handlers: self.notification_handlers,
                },
                next_request_id: AtomicU64::new(1),
                pending: Mutex::new(HashMap::new()),
                max_pending: self.max_pending,
                inbound_permits: Arc::new(Semaphore::new(self.max_inbound_handlers)),
                next_inbound_task_id: AtomicU64::new(1),
                inbound_tasks: Mutex::new(HashMap::new()),
                inbound_tasks_changed: Notify::new(),
            }),
        })
    }
}

fn ensure_capability_handler_match(
    capability: &str,
    advertised: bool,
    installed: bool,
) -> Result<(), McpError> {
    if advertised == installed {
        return Ok(());
    }
    Err(McpError::Protocol(format!(
        "MCP client {capability} capability and installed peer handler do not match"
    )))
}

fn decode_pending_result(
    result: Result<PendingResult, oneshot::error::RecvError>,
) -> PendingResult {
    result.unwrap_or_else(|_| {
        Err(McpError::Connection(
            "MCP pending request channel closed".to_owned(),
        ))
    })
}

fn request_id_key(id: &Value) -> Option<RequestIdKey> {
    if let Some(id) = id.as_u64() {
        return Some(RequestIdKey::Number(id));
    }
    if let Some(id) = id.as_i64() {
        return Some(RequestIdKey::Signed(id));
    }
    id.as_str().map(|id| RequestIdKey::String(id.to_owned()))
}

fn method_not_found(method: &str, reason: &str) -> JsonRpcError {
    JsonRpcError {
        code: METHOD_NOT_FOUND,
        message: format!("method not found: {method}"),
        data: Some(json!({ "method": method, "reason": reason })),
        extra: Default::default(),
    }
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic payload")
}

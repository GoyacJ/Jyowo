use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use dashmap::DashMap;
use harness_contracts::{
    Decision, InteractivityLevel, PermissionAwaitingHeartbeatEvent, PermissionDecisionOption,
    PermissionError, PermissionMode, PermissionOptionId, RequestId, SessionId, TenantId,
};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::{
    default_permission_decision_options, DecisionPersistence, NoopDecisionPersistence,
    PermissionBroker, PermissionContext, PermissionRequest, PersistedDecision,
};
use parking_lot::Mutex;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

pub struct StreamBasedBroker {
    requests: mpsc::Sender<PermissionRequest>,
    pending: Arc<DashMap<RequestId, PendingResolution>>,
    persistence: Arc<dyn DecisionPersistence>,
    config: StreamBrokerConfig,
    heartbeat_events: broadcast::Sender<PermissionAwaitingHeartbeatEvent>,
    sweeper: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Debug, Clone)]
pub struct StreamBrokerConfig {
    pub default_timeout: Option<Duration>,
    pub heartbeat_interval: Option<Duration>,
    pub max_pending: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelReason {
    UiClosed,
    SessionEnded,
    Cancelled(String),
}

struct PendingResolution {
    sender: oneshot::Sender<Decision>,
    request: PermissionRequest,
    context: PermissionContext,
    confirmation_expected: Option<String>,
    decision_options: Vec<PermissionDecisionOption>,
    enqueued_at: Instant,
    last_heartbeat_at: Instant,
    timeout_at: Instant,
    default_on_timeout: Decision,
}

impl PendingResolution {
    fn observe_metadata(&self) {
        let _ = (&self.request, self.enqueued_at, self.last_heartbeat_at);
    }
}

#[derive(Clone)]
pub struct ResolverHandle {
    pending: Arc<DashMap<RequestId, PendingResolution>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingPermissionRequest {
    pub request: PermissionRequest,
    pub context: PermissionContext,
    pub confirmation_expected: Option<String>,
    pub decision_options: Vec<PermissionDecisionOption>,
}

impl Default for StreamBrokerConfig {
    fn default() -> Self {
        Self {
            default_timeout: Some(DEFAULT_TIMEOUT),
            heartbeat_interval: None,
            max_pending: 1024,
        }
    }
}

impl StreamBasedBroker {
    pub fn new(
        config: StreamBrokerConfig,
    ) -> (Self, mpsc::Receiver<PermissionRequest>, ResolverHandle) {
        let channel_capacity = config.max_pending.max(1);
        let (requests, receiver) = mpsc::channel(channel_capacity);
        let pending = Arc::new(DashMap::new());
        let (heartbeat_events, _heartbeat_receiver) = broadcast::channel(channel_capacity);
        let resolver = ResolverHandle {
            pending: pending.clone(),
        };
        let broker = Self {
            requests,
            pending,
            persistence: Arc::new(NoopDecisionPersistence),
            config,
            heartbeat_events,
            sweeper: Mutex::new(None),
        };

        (broker, receiver, resolver)
    }

    #[must_use]
    pub fn with_persistence(mut self, persistence: Arc<dyn DecisionPersistence>) -> Self {
        self.persistence = persistence;
        self
    }

    fn timeout_for(&self, ctx: &PermissionContext) -> (Duration, Decision) {
        if let Some(policy) = &ctx.timeout_policy {
            return (
                Duration::from_millis(policy.deadline_ms),
                policy.default_on_timeout.clone(),
            );
        }

        (
            self.config.default_timeout.unwrap_or(DEFAULT_TIMEOUT),
            Decision::DenyOnce,
        )
    }

    pub fn subscribe_heartbeats(&self) -> broadcast::Receiver<PermissionAwaitingHeartbeatEvent> {
        self.heartbeat_events.subscribe()
    }

    fn ensure_sweeper(&self) {
        let mut sweeper = self.sweeper.lock();
        if matches!(sweeper.as_ref(), Some(handle) if !handle.is_finished()) {
            return;
        }
        if let Some(handle) = spawn_sweeper(
            self.pending.clone(),
            self.heartbeat_events.clone(),
            self.config.clone(),
        ) {
            *sweeper = Some(handle);
        }
    }
}

impl Drop for StreamBasedBroker {
    fn drop(&mut self) {
        if let Some(sweeper) = self.sweeper.get_mut().take() {
            sweeper.abort();
        }
    }
}

impl ResolverHandle {
    #[must_use]
    pub fn same_origin_as(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.pending, &other.pending)
    }

    #[must_use]
    pub fn pending_requests(&self) -> Vec<PermissionRequest> {
        self.pending
            .iter()
            .map(|pending| pending.request.clone())
            .collect()
    }

    #[must_use]
    pub fn pending_permission_requests(&self) -> Vec<PendingPermissionRequest> {
        self.pending
            .iter()
            .map(|pending| PendingPermissionRequest {
                request: pending.request.clone(),
                context: pending.context.clone(),
                confirmation_expected: pending.confirmation_expected.clone(),
                decision_options: pending.decision_options.clone(),
            })
            .collect()
    }

    pub async fn resolve_option_for(
        &self,
        request_id: RequestId,
        tenant_id: TenantId,
        session_id: SessionId,
        option_id: PermissionOptionId,
        submitted_decision: Decision,
        confirmation_text: Option<&str>,
    ) -> Result<Decision, PermissionError> {
        let decision = {
            let Some(pending) = self.pending.get(&request_id) else {
                return Err(PermissionError::Message(format!(
                    "permission request `{request_id}` is not pending"
                )));
            };
            if pending.request.tenant_id != tenant_id || pending.request.session_id != session_id {
                return Err(PermissionError::Message(format!(
                    "permission request `{request_id}` is not pending for this scope"
                )));
            }
            let Some(option) = pending
                .decision_options
                .iter()
                .find(|option| option.option_id == option_id)
            else {
                return Err(PermissionError::Message(format!(
                    "permission option `{option_id}` is not pending for request `{request_id}`"
                )));
            };

            if !same_decision_kind(&submitted_decision, &option.decision) {
                return Err(PermissionError::Message(
                    "submitted permission decision does not match option".to_owned(),
                ));
            }

            if option.requires_confirmation {
                let Some(expected) = pending.confirmation_expected.as_deref() else {
                    return Err(PermissionError::Message(
                        "confirmation text is required for this permission".to_owned(),
                    ));
                };
                let Some(actual) = confirmation_text else {
                    return Err(PermissionError::Message(
                        "confirmation text is required for this permission".to_owned(),
                    ));
                };
                if actual != expected {
                    return Err(PermissionError::Message(
                        "confirmation text does not match the required value".to_owned(),
                    ));
                }
            }

            option.decision.clone()
        };

        let Some((_request_id, pending)) = self.pending.remove(&request_id) else {
            return Err(PermissionError::Message(format!(
                "permission request `{request_id}` is not pending"
            )));
        };

        pending.observe_metadata();
        pending.sender.send(decision.clone()).map_err(|_| {
            PermissionError::Message(format!(
                "permission request `{request_id}` receiver is closed"
            ))
        })?;
        Ok(decision)
    }

    pub async fn cancel(
        &self,
        request_id: RequestId,
        _reason: CancelReason,
    ) -> Result<(), PermissionError> {
        let Some((_request_id, pending)) = self.pending.remove(&request_id) else {
            return Err(PermissionError::Message(format!(
                "permission request `{request_id}` is not pending"
            )));
        };

        pending.observe_metadata();
        let _ = pending.sender.send(Decision::DenyOnce);
        Ok(())
    }
}

#[async_trait]
impl PermissionBroker for StreamBasedBroker {
    fn can_anchor_authority(&self) -> bool {
        false
    }

    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        if matches!(
            ctx.permission_mode,
            PermissionMode::BypassPermissions | PermissionMode::DontAsk
        ) {
            return Decision::AllowOnce;
        }

        if matches!(ctx.interactivity, InteractivityLevel::NoInteractive) {
            return Decision::DenyOnce;
        }

        if self.pending.len() >= self.config.max_pending {
            return Decision::DenyOnce;
        }

        let request_id = request.request_id;
        let (timeout, default_on_timeout) = self.timeout_for(&ctx);
        let decision_options = if request.decision_options.is_empty() {
            default_permission_decision_options(&request)
        } else {
            request.decision_options.clone()
        };
        let (sender, receiver) = oneshot::channel();
        let now = Instant::now();
        self.pending.insert(
            request_id,
            PendingResolution {
                sender,
                request: request.clone(),
                context: ctx.clone(),
                confirmation_expected: request.confirmation_expected.clone(),
                decision_options,
                enqueued_at: now,
                last_heartbeat_at: now,
                timeout_at: now + timeout,
                default_on_timeout: default_on_timeout.clone(),
            },
        );
        self.ensure_sweeper();

        if self.requests.send(request).await.is_err() {
            self.pending.remove(&request_id);
            return Decision::DenyOnce;
        }

        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(decision)) => decision,
            Ok(Err(_closed)) => Decision::DenyOnce,
            Err(_elapsed) => {
                self.pending.remove(&request_id);
                default_on_timeout
            }
        }
    }

    async fn hard_policy_denies(
        &self,
        _request: &PermissionRequest,
        _ctx: &PermissionContext,
    ) -> bool {
        false
    }

    async fn persist(&self, decision: PersistedDecision) -> Result<(), PermissionError> {
        self.persistence.persist(decision).await
    }
}

fn spawn_sweeper(
    pending: Arc<DashMap<RequestId, PendingResolution>>,
    heartbeat_events: broadcast::Sender<PermissionAwaitingHeartbeatEvent>,
    config: StreamBrokerConfig,
) -> Option<JoinHandle<()>> {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return None;
    };

    Some(handle.spawn(async move {
        let heartbeat_interval = config.heartbeat_interval.unwrap_or(DEFAULT_TIMEOUT);
        let tick_interval =
            heartbeat_interval.min(config.default_timeout.unwrap_or(DEFAULT_TIMEOUT));
        let mut ticker = tokio::time::interval(tick_interval.max(Duration::from_millis(1)));

        loop {
            ticker.tick().await;
            let now = Instant::now();
            let mut timed_out = Vec::new();
            let mut heartbeat_due = Vec::new();

            for pending in pending.iter() {
                if now >= pending.timeout_at {
                    timed_out.push(*pending.key());
                } else if config.heartbeat_interval.is_some()
                    && now.duration_since(pending.last_heartbeat_at) >= heartbeat_interval
                {
                    heartbeat_due.push(*pending.key());
                }
            }

            for request_id in heartbeat_due {
                if let Some(mut pending) = pending.get_mut(&request_id) {
                    pending.last_heartbeat_at = now;
                    let _ = heartbeat_events.send(PermissionAwaitingHeartbeatEvent {
                        request_id,
                        at: Utc::now(),
                    });
                }
            }

            for request_id in timed_out {
                if let Some((_request_id, pending)) = pending.remove(&request_id) {
                    let _ = pending.sender.send(pending.default_on_timeout);
                }
            }
        }
    }))
}

fn same_decision_kind(submitted: &Decision, option: &Decision) -> bool {
    matches!(
        (submitted, option),
        (
            Decision::AllowOnce | Decision::AllowSession | Decision::AllowPermanent,
            Decision::AllowOnce | Decision::AllowSession | Decision::AllowPermanent
        ) | (
            Decision::DenyOnce | Decision::DenyPermanent,
            Decision::DenyOnce | Decision::DenyPermanent
        )
    )
}

//! `jyowo-harness-team`
//!
//! Team topologies, message bus, and coordinator patterns.
//!
//! SPEC: docs/architecture/harness/crates/harness-team.md
//! This crate is intentionally single-process only. The message bus is built
//! on in-process Tokio broadcast channels and does not provide cross-process
//! ordering or delivery.

#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use harness_contracts::{
    AgentId, AgentMessageRoutedEvent, AgentMessageSentEvent, BlobMeta, BlobRef, BlobRetention,
    BlobStore, ContentHash, CorrelationId, EngineError, EngineFailedEvent, Event,
    InteractivityLevel, JournalOffset, MemberLeaveReason, MemoryError, MemoryId, MemoryKind,
    MemorySource, MemoryUpsertedEvent, MemoryVisibility, MemoryWriteAction, Message, MessageId,
    MessagePart, MessageRole, ModelRef, PermissionMode, Recipient, RoutingPolicyKind, RunId,
    SessionId, StalledAction, TakesEffect, TeamCreatedEvent, TeamId, TeamMemberJoinedEvent,
    TeamMemberLeftEvent, TeamMemberStalledEvent, TeamTerminatedEvent, TeamTerminationReason,
    TeamTurnCompletedEvent, TenantId, TopologyKind, TranscriptRef, TurnInput, UsageSnapshot,
};
use harness_journal::{AppendMetadata, EventStore, ReplayCursor};
use harness_memory::{
    MemoryLifecycle, MemoryListScope, MemoryMetadata, MemoryQuery, MemoryRecord, MemoryStore,
    MemorySummary, MemoryVisibilityFilter,
};
use harness_model::{AuxExecutor, AuxModelProvider, AuxTask, ModelProtocol, ModelRequest};
use harness_session::{Session, SessionOptions};
use parking_lot::{Mutex as SyncMutex, MutexGuard as SyncMutexGuard};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Mutex};
use tokio_util::sync::CancellationToken as TokioCancellationToken;

pub use harness_budget::{ResourceQuota, TokenBudget};
pub use harness_contracts::{ContextVisibility, MessagePayload};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Topology {
    CoordinatorWorker,
    PeerToPeer,
    RoleRouted,
    Custom,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct TeamTopologyConfig {
    pub coordinator: Option<AgentId>,
    pub workers: Vec<AgentId>,
    pub role_routes: Vec<RoleRoute>,
    pub role_rules: Vec<RoleRoutingRule>,
    pub route_fallback: RouteFallback,
    pub custom_strategy_id: Option<String>,
}

impl Default for TeamTopologyConfig {
    fn default() -> Self {
        Self {
            coordinator: None,
            workers: Vec::new(),
            role_routes: Vec::new(),
            role_rules: Vec::new(),
            route_fallback: RouteFallback::DropMessage,
            custom_strategy_id: None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoleRoute {
    pub role: String,
    pub targets: Vec<AgentId>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct MessageBusSpec {
    pub buffer_size: usize,
    pub persistence: BusPersistence,
    pub ordering: MessageOrdering,
    pub replay_window: ReplayWindowSpec,
    pub backpressure: BusBackpressure,
    pub max_messages_per_correlation: u32,
}

impl Default for MessageBusSpec {
    fn default() -> Self {
        Self {
            buffer_size: 256,
            persistence: BusPersistence::InMemory,
            ordering: MessageOrdering::Fifo,
            replay_window: ReplayWindowSpec::All,
            backpressure: BusBackpressure::DropOldest,
            max_messages_per_correlation: 256,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusPersistence {
    InMemory,
    Journaled,
    Durable,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageOrdering {
    Fifo,
    Causal,
    Total,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayWindowSpec {
    None,
    Last(usize),
    Since(DateTime<Utc>),
    All,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusBackpressure {
    DropOldest,
    RejectNew,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedMemorySpec {
    Disabled,
    Enabled {
        provider_id: String,
        write_policy: SharedWritePolicy,
    },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamLifecycle {
    OneShot,
    Persistent { max_idle: Duration },
    ExplicitTerminate,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct TeamObservability {
    pub emit_member_events: bool,
    pub emit_routing_events: bool,
    pub capture_transcript: bool,
}

impl Default for TeamObservability {
    fn default() -> Self {
        Self {
            emit_member_events: true,
            emit_routing_events: true,
            capture_transcript: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassifierConfidenceObservation {
    pub classifier_id: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TeamObservationSnapshot {
    pub cyclic_routing_detected: u64,
    pub classifier_timeouts: u64,
    pub classifier_confidences: Vec<ClassifierConfidenceObservation>,
    pub context_visibility_blocked: u64,
    pub message_bus_backpressure: u64,
    pub dynamic_member_adds: u64,
    pub dynamic_member_removes: u64,
}

impl TeamObservationSnapshot {
    fn add_bus_snapshot(&mut self, snapshot: &TeamObservationSnapshot) {
        self.cyclic_routing_detected += snapshot.cyclic_routing_detected;
        self.message_bus_backpressure += snapshot.message_bus_backpressure;
    }
}

#[derive(Debug, Default)]
struct TeamObservationState {
    cyclic_routing_detected: u64,
    classifier_timeouts: u64,
    classifier_confidences: Vec<ClassifierConfidenceObservation>,
    context_visibility_blocked: u64,
    message_bus_backpressure: u64,
    dynamic_member_adds: u64,
    dynamic_member_removes: u64,
}

impl TeamObservationState {
    fn snapshot(&self) -> TeamObservationSnapshot {
        TeamObservationSnapshot {
            cyclic_routing_detected: self.cyclic_routing_detected,
            classifier_timeouts: self.classifier_timeouts,
            classifier_confidences: self.classifier_confidences.clone(),
            context_visibility_blocked: self.context_visibility_blocked,
            message_bus_backpressure: self.message_bus_backpressure,
            dynamic_member_adds: self.dynamic_member_adds,
            dynamic_member_removes: self.dynamic_member_removes,
        }
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TeamResourceQuota {
    pub max_members: Option<u32>,
    pub max_messages: Option<u32>,
    pub max_duration: Option<Duration>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamQuotaKind {
    Members,
    Messages,
    WallClock,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamMember {
    pub agent_id: AgentId,
    pub role: String,
    pub visibility: ContextVisibility,
    pub engine_config: TeamMemberEngineConfig,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamToolsetSelector {
    InheritAll,
    InheritWithBlocklist(HashSet<String>),
    Preset(String),
    Custom(Vec<String>),
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamSandboxPolicy {
    Inherit,
    Empty,
    RequireBackend(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamMemberEngineConfig {
    pub model_ref: Option<ModelRef>,
    pub toolset: TeamToolsetSelector,
    pub tool_blocklist: HashSet<String>,
    pub permission_mode: PermissionMode,
    pub interactivity: InteractivityLevel,
    pub sandbox_policy: TeamSandboxPolicy,
    pub max_iterations: u32,
    pub system_prompt_addendum: Option<String>,
    pub quota: Option<ResourceQuota>,
    pub token_budget: TokenBudget,
}

impl Default for TeamMemberEngineConfig {
    fn default() -> Self {
        Self {
            model_ref: None,
            toolset: TeamToolsetSelector::InheritAll,
            tool_blocklist: HashSet::new(),
            permission_mode: PermissionMode::Default,
            interactivity: InteractivityLevel::NoInteractive,
            sandbox_policy: TeamSandboxPolicy::Inherit,
            max_iterations: 25,
            system_prompt_addendum: None,
            quota: None,
            token_budget: TokenBudget::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamSpec {
    pub team_id: TeamId,
    pub name: String,
    pub topology: Topology,
    pub topology_config: TeamTopologyConfig,
    pub members: Vec<TeamMember>,
    pub message_bus: MessageBusSpec,
    pub shared_memory: SharedMemorySpec,
    pub lifecycle: TeamLifecycle,
    pub observability: TeamObservability,
    pub quota: TeamResourceQuota,
    pub single_process_only: bool,
    pub max_messages_per_correlation: u32,
    pub max_turns_per_goal: u32,
}

impl TeamSpec {
    #[must_use]
    pub fn new(name: impl Into<String>, topology: Topology) -> Self {
        Self {
            team_id: TeamId::new(),
            name: name.into(),
            topology,
            topology_config: TeamTopologyConfig::default(),
            members: Vec::new(),
            message_bus: MessageBusSpec::default(),
            shared_memory: SharedMemorySpec::Disabled,
            lifecycle: TeamLifecycle::OneShot,
            observability: TeamObservability::default(),
            quota: TeamResourceQuota::default(),
            single_process_only: true,
            max_messages_per_correlation: 256,
            max_turns_per_goal: 32,
        }
    }

    #[must_use]
    pub fn coordinator_id(&self) -> Option<AgentId> {
        self.topology_config.coordinator
    }

    #[must_use]
    pub fn coordinator_workers(&self) -> &[AgentId] {
        &self.topology_config.workers
    }

    pub fn validate(&self) -> Result<(), TeamError> {
        let member_ids = self
            .members
            .iter()
            .map(|member| member.agent_id)
            .collect::<HashSet<_>>();
        if let Some(max_members) = self.quota.max_members {
            if self.members.len() > max_members as usize {
                return Err(TeamError::InvalidSpec("max_members exceeded".to_owned()));
            }
        }
        if self.message_bus.buffer_size == 0 {
            return Err(TeamError::InvalidSpec(
                "message bus buffer_size must be greater than zero".to_owned(),
            ));
        }
        if matches!(
            self.lifecycle,
            TeamLifecycle::Persistent { max_idle } if max_idle.is_zero()
        ) {
            return Err(TeamError::InvalidSpec(
                "persistent team lifecycle requires max_idle".to_owned(),
            ));
        }
        if self.topology_config.route_fallback == RouteFallback::SendToCoordinator
            && self.topology_config.coordinator.is_none()
        {
            return Err(TeamError::InvalidSpec(
                "send_to_coordinator fallback requires coordinator".to_owned(),
            ));
        }
        let mut classifier_ids = HashSet::new();
        for rule in &self.topology_config.role_rules {
            match &rule.pattern {
                RoutingPattern::KeywordAny { keywords, roles } => {
                    if keywords.is_empty() {
                        return Err(TeamError::InvalidSpec(
                            "keyword routing rule requires at least one keyword".to_owned(),
                        ));
                    }
                    self.validate_route_roles(roles)?;
                }
                RoutingPattern::RegexMatch { pattern, roles } => {
                    Regex::new(pattern).map_err(|error| {
                        TeamError::InvalidSpec(format!("regex routing rule is invalid: {error}"))
                    })?;
                    self.validate_route_roles(roles)?;
                }
                RoutingPattern::Classifier { classifier_id } => {
                    if classifier_id.is_empty() {
                        return Err(TeamError::InvalidSpec(
                            "classifier routing rule requires classifier_id".to_owned(),
                        ));
                    }
                    if !classifier_ids.insert(classifier_id.as_str()) {
                        return Err(TeamError::InvalidSpec(
                            "classifier_id duplicated in role routing rules".to_owned(),
                        ));
                    }
                }
            }
        }
        match self.topology {
            Topology::CoordinatorWorker => {
                let coordinator = self.topology_config.coordinator.ok_or_else(|| {
                    TeamError::InvalidSpec("coordinator_worker requires coordinator".to_owned())
                })?;
                if !member_ids.contains(&coordinator) {
                    return Err(TeamError::InvalidSpec(
                        "coordinator_worker coordinator is not a member".to_owned(),
                    ));
                }
                let mut seen = HashSet::new();
                for worker in &self.topology_config.workers {
                    if *worker == coordinator {
                        return Err(TeamError::InvalidSpec(
                            "coordinator cannot also be a worker".to_owned(),
                        ));
                    }
                    if !member_ids.contains(worker) {
                        return Err(TeamError::InvalidSpec(
                            "coordinator_worker worker is not a member".to_owned(),
                        ));
                    }
                    if !seen.insert(*worker) {
                        return Err(TeamError::InvalidSpec(
                            "coordinator_worker worker duplicated".to_owned(),
                        ));
                    }
                }
            }
            Topology::RoleRouted => {
                let mut roles = HashSet::new();
                for route in &self.topology_config.role_routes {
                    if route.role.is_empty() {
                        return Err(TeamError::InvalidSpec(
                            "role route role cannot be empty".to_owned(),
                        ));
                    }
                    if !roles.insert(route.role.as_str()) {
                        return Err(TeamError::InvalidSpec(
                            "role route role duplicated".to_owned(),
                        ));
                    }
                    for target in &route.targets {
                        if !member_ids.contains(target) {
                            return Err(TeamError::InvalidSpec(
                                "role route target is not a member".to_owned(),
                            ));
                        }
                    }
                }
            }
            Topology::PeerToPeer => {}
            Topology::Custom => {
                if !self
                    .topology_config
                    .custom_strategy_id
                    .as_ref()
                    .is_some_and(|strategy_id| !strategy_id.is_empty())
                {
                    return Err(TeamError::InvalidSpec(
                        "custom topology requires custom_strategy_id".to_owned(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_route_roles(&self, roles: &[String]) -> Result<(), TeamError> {
        if roles.is_empty() {
            return Err(TeamError::InvalidSpec(
                "role routing rule roles cannot be empty".to_owned(),
            ));
        }
        for role in roles {
            let role_exists = self.members.iter().any(|member| member.role == *role)
                || self
                    .topology_config
                    .role_routes
                    .iter()
                    .any(|route| route.role == *role);
            if !role_exists {
                return Err(TeamError::InvalidSpec(
                    "role routing rule role has no members or explicit route".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

pub struct TeamBuilder {
    spec: TeamSpec,
}

impl TeamBuilder {
    #[must_use]
    pub fn new(name: impl Into<String>, topology: Topology) -> Self {
        Self {
            spec: TeamSpec::new(name, topology),
        }
    }

    #[must_use]
    pub fn member(
        mut self,
        agent_id: AgentId,
        role: impl Into<String>,
        visibility: ContextVisibility,
    ) -> Self {
        self.spec.members.push(TeamMember {
            agent_id,
            role: role.into(),
            visibility,
            engine_config: TeamMemberEngineConfig::default(),
        });
        self
    }

    #[must_use]
    pub fn member_with_engine_config(
        mut self,
        agent_id: AgentId,
        role: impl Into<String>,
        visibility: ContextVisibility,
        engine_config: TeamMemberEngineConfig,
    ) -> Self {
        self.spec.members.push(TeamMember {
            agent_id,
            role: role.into(),
            visibility,
            engine_config,
        });
        self
    }

    #[must_use]
    pub fn coordinator_worker(mut self, coordinator: AgentId, workers: Vec<AgentId>) -> Self {
        self.spec.topology_config.coordinator = Some(coordinator);
        self.spec.topology_config.workers = workers;
        self
    }

    #[must_use]
    pub fn role_route(mut self, role: impl Into<String>, targets: Vec<AgentId>) -> Self {
        self.spec.topology_config.role_routes.push(RoleRoute {
            role: role.into(),
            targets,
        });
        self
    }

    #[must_use]
    pub fn build(self) -> TeamSpec {
        self.spec
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentMessage {
    pub team_id: TeamId,
    pub from: AgentId,
    pub to: Recipient,
    pub payload: MessagePayload,
    pub message_id: MessageId,
    pub sent_at: chrono::DateTime<Utc>,
    pub correlation_id: CorrelationId,
}

impl AgentMessage {
    #[must_use]
    pub fn new(team_id: TeamId, from: AgentId, to: Recipient, payload: MessagePayload) -> Self {
        Self::with_correlation(team_id, from, to, payload, CorrelationId::new())
    }

    #[must_use]
    pub fn with_correlation(
        team_id: TeamId,
        from: AgentId,
        to: Recipient,
        payload: MessagePayload,
        correlation_id: CorrelationId,
    ) -> Self {
        Self {
            team_id,
            from,
            to,
            payload,
            message_id: MessageId::new(),
            sent_at: Utc::now(),
            correlation_id,
        }
    }

    #[must_use]
    pub fn text(team_id: TeamId, from: AgentId, to: Recipient, text: impl Into<String>) -> Self {
        Self::new(team_id, from, to, MessagePayload::Text(text.into()))
    }

    #[must_use]
    pub fn text_with_correlation(
        team_id: TeamId,
        from: AgentId,
        to: Recipient,
        text: impl Into<String>,
        correlation_id: CorrelationId,
    ) -> Self {
        Self::with_correlation(
            team_id,
            from,
            to,
            MessagePayload::Text(text.into()),
            correlation_id,
        )
    }
}

#[derive(Clone)]
pub struct MessageBus {
    team_id: TeamId,
    tx: broadcast::Sender<AgentMessage>,
    journal: Arc<Mutex<Vec<AgentMessage>>>,
    messages_by_correlation: Arc<Mutex<HashMap<CorrelationId, u32>>>,
    observations: Arc<Mutex<TeamObservationState>>,
    persistent_journal: TeamJournal,
    spec: MessageBusSpec,
}

#[derive(Clone)]
struct TeamJournal {
    tenant_id: TenantId,
    session_id: harness_contracts::SessionId,
    event_store: Arc<dyn EventStore>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct TeamJournalContext {
    pub tenant_id: TenantId,
    pub session_id: harness_contracts::SessionId,
}

impl MessageBus {
    #[must_use]
    pub fn journaled(
        team_id: TeamId,
        capacity: usize,
        context: TeamJournalContext,
        event_store: Arc<dyn EventStore>,
    ) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            team_id,
            tx,
            journal: Arc::new(Mutex::new(Vec::new())),
            messages_by_correlation: Arc::new(Mutex::new(HashMap::new())),
            observations: Arc::new(Mutex::new(TeamObservationState::default())),
            persistent_journal: TeamJournal {
                tenant_id: context.tenant_id,
                session_id: context.session_id,
                event_store,
            },
            spec: MessageBusSpec {
                buffer_size: capacity.max(1),
                ..MessageBusSpec::default()
            },
        }
    }

    #[must_use]
    pub fn with_spec(mut self, spec: MessageBusSpec) -> Self {
        self.spec = MessageBusSpec {
            buffer_size: spec.buffer_size.max(1),
            ..spec
        };
        self
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentMessage> {
        self.tx.subscribe()
    }

    pub async fn observation_snapshot(&self) -> TeamObservationSnapshot {
        self.observations.lock().await.snapshot()
    }

    pub async fn send(&self, message: AgentMessage) -> Result<(), TeamError> {
        self.send_routed(
            message,
            Vec::new(),
            RoutingPolicyKind::Custom("unresolved".to_owned()),
        )
        .await
    }

    pub(crate) async fn send_routed(
        &self,
        message: AgentMessage,
        resolved_recipients: Vec<AgentId>,
        routing_policy: RoutingPolicyKind,
    ) -> Result<(), TeamError> {
        if self
            .record_message_for_correlation(message.correlation_id)
            .await?
            == MessageLimitOutcome::Fallback
        {
            return Err(TeamError::RoutingLimitExceeded(
                "max_messages_per_correlation".to_owned(),
            ));
        }
        self.send_routed_with_delivery(message, resolved_recipients, routing_policy, true)
            .await
    }

    async fn record_message_for_correlation(
        &self,
        correlation_id: CorrelationId,
    ) -> Result<MessageLimitOutcome, TeamError> {
        let max_messages = self.spec.max_messages_per_correlation;
        let over_limit = {
            let mut messages = self.messages_by_correlation.lock().await;
            let count = messages.entry(correlation_id).or_insert(0);
            if *count >= max_messages {
                true
            } else {
                *count += 1;
                false
            }
        };
        if over_limit {
            self.observations.lock().await.cyclic_routing_detected += 1;
            self.write_correlation_limit_failure(correlation_id).await?;
            return Ok(MessageLimitOutcome::Fallback);
        }
        Ok(MessageLimitOutcome::Allowed)
    }

    async fn write_correlation_limit_failure(
        &self,
        correlation_id: CorrelationId,
    ) -> Result<(), TeamError> {
        let journal = &self.persistent_journal;
        journal
            .event_store
            .append_with_metadata(
                journal.tenant_id,
                journal.session_id,
                AppendMetadata {
                    correlation_id,
                    ..AppendMetadata::default()
                },
                &[Event::EngineFailed(EngineFailedEvent {
                    session_id: Some(journal.session_id),
                    run_id: None,
                    error: EngineError::Message(
                        "cyclic routing: max_messages_per_correlation".to_owned(),
                    ),
                    at: Utc::now(),
                })],
            )
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        Ok(())
    }

    async fn send_routed_with_delivery(
        &self,
        message: AgentMessage,
        resolved_recipients: Vec<AgentId>,
        routing_policy: RoutingPolicyKind,
        deliver: bool,
    ) -> Result<(), TeamError> {
        if message.team_id != self.team_id {
            return Err(TeamError::TeamMismatch);
        }
        {
            let messages = self.journal.lock().await;
            if messages.len() >= self.spec.buffer_size
                && self.spec.backpressure == BusBackpressure::RejectNew
            {
                self.observations.lock().await.message_bus_backpressure += 1;
                return Err(TeamError::MessageBusBackpressure {
                    team_id: self.team_id,
                    depth: messages.len(),
                });
            }
        }
        let journal = &self.persistent_journal;
        journal
            .event_store
            .append_with_metadata(
                journal.tenant_id,
                journal.session_id,
                AppendMetadata {
                    correlation_id: message.correlation_id,
                    ..AppendMetadata::default()
                },
                &[
                    Event::AgentMessageSent(AgentMessageSentEvent {
                        team_id: message.team_id,
                        from: message.from,
                        to: message.to.clone(),
                        payload: message.payload.clone(),
                        message_id: message.message_id,
                        at: message.sent_at,
                    }),
                    Event::AgentMessageRouted(AgentMessageRoutedEvent {
                        team_id: message.team_id,
                        message_id: message.message_id,
                        resolved_recipients,
                        routing_policy,
                        at: Utc::now(),
                    }),
                ],
            )
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        {
            let mut messages = self.journal.lock().await;
            if messages.len() >= self.spec.buffer_size
                && self.spec.backpressure == BusBackpressure::DropOldest
            {
                messages.remove(0);
            }
            messages.push(message.clone());
        }
        if deliver {
            let _ = self.tx.send(message);
        }
        Ok(())
    }

    pub async fn replay(&self) -> Vec<AgentMessage> {
        self.journal.lock().await.clone()
    }

    pub async fn replay_window(&self, window: ReplayWindow) -> Vec<AgentMessage> {
        self.replay_for_spec(ReplayWindowSpec::Last(window.max_messages))
            .await
    }

    pub async fn replay_for_spec(&self, window: ReplayWindowSpec) -> Vec<AgentMessage> {
        let messages = self.journal.lock().await;
        match window {
            ReplayWindowSpec::None => Vec::new(),
            ReplayWindowSpec::Last(max_messages) => {
                let start = messages.len().saturating_sub(max_messages);
                messages[start..].to_vec()
            }
            ReplayWindowSpec::Since(since) => messages
                .iter()
                .filter(|message| message.sent_at >= since)
                .cloned()
                .collect(),
            ReplayWindowSpec::All => messages.clone(),
        }
    }

    pub async fn replay_from_journal(&self) -> Result<Vec<AgentMessage>, TeamError> {
        let journal = &self.persistent_journal;
        let mut stream = journal
            .event_store
            .read_envelopes(
                journal.tenant_id,
                journal.session_id,
                ReplayCursor::FromStart,
            )
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        let mut messages = Vec::new();
        while let Some(envelope) = stream.next().await {
            if let Event::AgentMessageSent(event) = envelope.payload {
                if event.team_id == self.team_id {
                    messages.push(AgentMessage {
                        team_id: event.team_id,
                        from: event.from,
                        to: event.to,
                        payload: event.payload,
                        message_id: event.message_id,
                        sent_at: event.at,
                        correlation_id: envelope.correlation_id,
                    });
                }
            }
        }
        Ok(messages)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct ReplayWindow {
    pub max_messages: usize,
}

impl ReplayWindow {
    #[must_use]
    pub fn last(max_messages: usize) -> Self {
        Self { max_messages }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum MessageClass {
    Text,
    Request,
    Response,
    Structured,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassifierVerdict {
    pub roles: Vec<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
pub enum ClassifierError {
    #[error("classifier produced no match")]
    NoMatch,
    #[error("classifier failed: {0}")]
    Failed(String),
}

#[async_trait]
pub trait MessageClassifier: Send + Sync {
    fn classifier_id(&self) -> &str;

    fn timeout(&self) -> Duration {
        Duration::from_secs(5)
    }

    async fn classify(&self, message: &AgentMessage) -> Result<ClassifierVerdict, ClassifierError>;
}

#[async_trait]
pub trait RoleMessageClassifier: Send + Sync + 'static {
    fn classifier_id(&self) -> &str;

    fn timeout(&self) -> Duration;

    async fn classify(
        &self,
        message: &AgentMessage,
        team: &TeamSpec,
    ) -> Result<ClassifierVerdict, ClassifierError>;
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteFallback {
    DropMessage,
    SendToCoordinator,
    Broadcast,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingPattern {
    KeywordAny {
        keywords: Vec<String>,
        roles: Vec<String>,
    },
    RegexMatch {
        pattern: String,
        roles: Vec<String>,
    },
    Classifier {
        classifier_id: String,
    },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct RoleRoutingRule {
    pub rule_id: String,
    pub priority: u32,
    pub pattern: RoutingPattern,
}

#[derive(Clone)]
pub struct RoleRoutingTable {
    rules: Vec<RoleRoutingRule>,
    fallback: RouteFallback,
    classifiers: BTreeMap<String, Arc<dyn RoleMessageClassifier>>,
    classifier_timeout_total: Arc<AtomicU64>,
}

impl std::fmt::Debug for RoleRoutingTable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RoleRoutingTable")
            .field("rules", &self.rules)
            .field("fallback", &self.fallback)
            .field("classifiers", &self.classifiers.keys().collect::<Vec<_>>())
            .field(
                "classifier_timeout_total",
                &self.classifier_timeout_total.load(Ordering::Relaxed),
            )
            .finish()
    }
}

impl RoleRoutingTable {
    pub fn new(
        mut rules: Vec<RoleRoutingRule>,
        fallback: RouteFallback,
        classifiers: Vec<Arc<dyn RoleMessageClassifier>>,
    ) -> Result<Self, TeamError> {
        for rule in &rules {
            if let RoutingPattern::RegexMatch { pattern, .. } = &rule.pattern {
                Regex::new(pattern).map_err(|error| {
                    TeamError::Internal(format!(
                        "role routing rule {} has invalid regex: {error}",
                        rule.rule_id
                    ))
                })?;
            }
        }

        let mut classifier_map = BTreeMap::new();
        for classifier in classifiers {
            let id = classifier.classifier_id().to_owned();
            if classifier_map.insert(id.clone(), classifier).is_some() {
                return Err(TeamError::Internal(format!(
                    "duplicate role classifier id: {id}"
                )));
            }
        }
        for rule in &rules {
            if let RoutingPattern::Classifier { classifier_id } = &rule.pattern {
                if !classifier_map.contains_key(classifier_id) {
                    return Err(TeamError::Internal(format!(
                        "role routing rule {} references missing classifier: {classifier_id}",
                        rule.rule_id
                    )));
                }
            }
        }

        rules.sort_by(|left, right| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| left.rule_id.cmp(&right.rule_id))
        });

        Ok(Self {
            rules,
            fallback,
            classifiers: classifier_map,
            classifier_timeout_total: Arc::new(AtomicU64::new(0)),
        })
    }

    #[must_use]
    pub fn classifier_timeout_total(&self) -> u64 {
        self.classifier_timeout_total.load(Ordering::Relaxed)
    }

    async fn route(&self, message: &AgentMessage, team: &TeamSpec) -> Option<Recipient> {
        for rule in &self.rules {
            match &rule.pattern {
                RoutingPattern::KeywordAny { keywords, roles } => {
                    let payload = message_payload_for_classification(&message.payload);
                    if keywords.iter().any(|keyword| payload.contains(keyword)) {
                        return roles_to_recipient(roles, team).or_else(|| self.fallback());
                    }
                }
                RoutingPattern::RegexMatch { pattern, roles } => {
                    let payload = message_payload_for_classification(&message.payload);
                    if Regex::new(pattern)
                        .map(|regex| regex.is_match(&payload))
                        .unwrap_or(false)
                    {
                        return roles_to_recipient(roles, team).or_else(|| self.fallback());
                    }
                }
                RoutingPattern::Classifier { classifier_id } => {
                    let Some(classifier) = self.classifiers.get(classifier_id) else {
                        return self.fallback();
                    };
                    let result = tokio::time::timeout(
                        classifier.timeout(),
                        classifier.classify(message, team),
                    )
                    .await;
                    let verdict = match result {
                        Ok(Ok(verdict)) => verdict,
                        Ok(Err(_)) => return self.fallback(),
                        Err(_) => {
                            self.classifier_timeout_total
                                .fetch_add(1, Ordering::Relaxed);
                            return self.fallback();
                        }
                    };
                    return roles_to_recipient(&verdict.roles, team).or_else(|| self.fallback());
                }
            }
        }

        self.fallback()
    }

    fn fallback(&self) -> Option<Recipient> {
        match self.fallback {
            RouteFallback::DropMessage => None,
            RouteFallback::SendToCoordinator => Some(Recipient::Coordinator),
            RouteFallback::Broadcast => Some(Recipient::Broadcast),
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FallbackMessageClassifier;

impl FallbackMessageClassifier {
    #[must_use]
    pub fn classify_kind(&self, message: &AgentMessage) -> MessageClass {
        match message.payload {
            MessagePayload::Text(_) => MessageClass::Text,
            MessagePayload::Structured(_) => MessageClass::Structured,
            MessagePayload::Request { .. } => MessageClass::Request,
            MessagePayload::Response { .. } => MessageClass::Response,
            _ => MessageClass::Structured,
        }
    }
}

#[async_trait]
impl MessageClassifier for FallbackMessageClassifier {
    fn classifier_id(&self) -> &str {
        "fallback"
    }

    async fn classify(&self, message: &AgentMessage) -> Result<ClassifierVerdict, ClassifierError> {
        let role = match self.classify_kind(message) {
            MessageClass::Text => "text",
            MessageClass::Request => "request",
            MessageClass::Response => "response",
            MessageClass::Structured => "structured",
        };
        Ok(ClassifierVerdict {
            roles: vec![role.to_owned()],
            confidence: 1.0,
        })
    }
}

#[derive(Clone)]
pub struct AuxRoleClassifier {
    executor: AuxExecutor,
}

impl AuxRoleClassifier {
    #[must_use]
    pub fn new(aux_provider: Arc<dyn AuxModelProvider>) -> Self {
        Self {
            executor: AuxExecutor::new(aux_provider),
        }
    }

    #[must_use]
    pub fn from_executor(executor: AuxExecutor) -> Self {
        Self { executor }
    }

    pub async fn classify_role(&self, message: &AgentMessage, team: &TeamSpec) -> Option<String> {
        let req = aux_classify_request(self.executor.provider().as_ref(), message, team);
        let output = self
            .executor
            .call(AuxTask::Classify, req)
            .await
            .ok()
            .flatten()?;
        first_valid_role(&output, team)
    }
}

#[async_trait]
impl RoleMessageClassifier for AuxRoleClassifier {
    fn classifier_id(&self) -> &str {
        "aux-role-classifier"
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(2)
    }

    async fn classify(
        &self,
        message: &AgentMessage,
        team: &TeamSpec,
    ) -> Result<ClassifierVerdict, ClassifierError> {
        self.classify_role(message, team)
            .await
            .map(|role| ClassifierVerdict {
                roles: vec![role],
                confidence: 1.0,
            })
            .ok_or(ClassifierError::NoMatch)
    }
}

fn aux_classify_request(
    aux_provider: &dyn AuxModelProvider,
    message: &AgentMessage,
    team: &TeamSpec,
) -> ModelRequest {
    let descriptor = aux_provider.inner().supported_models().into_iter().next();
    let roles = team
        .members
        .iter()
        .map(|member| member.role.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    ModelRequest {
        model_id: descriptor
            .map(|descriptor| descriptor.model_id)
            .unwrap_or_else(|| "aux-classify".to_owned()),
        messages: vec![Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(format!(
                "Classify this team message to one target role.\navailable_roles: {roles}\nfrom: {}\npayload: {}",
                message.from,
                message_payload_for_classification(&message.payload)
            ))],
            created_at: harness_contracts::now(),
        }],
        tools: None,
        system: Some(
            "Return JSON only, using this shape: {\"roles\":[\"role-name\"]}. Use only available roles."
                .to_owned(),
        ),
        temperature: Some(0.0),
        max_tokens: Some(128),
        stream: false,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::Messages,
        extra: serde_json::Value::Null,
        provider_context: harness_model::ProviderRequestContext::default(),
    }
}

fn message_payload_for_classification(payload: &MessagePayload) -> String {
    match payload {
        MessagePayload::Text(text) => text.clone(),
        MessagePayload::Structured(value) => value.to_string(),
        MessagePayload::Request { reply_to } => format!("request reply_to={reply_to}"),
        MessagePayload::Response { in_reply_to, body } => {
            format!("response in_reply_to={in_reply_to} body={body}")
        }
        MessagePayload::Handoff { to, summary } => format!("handoff to={to} summary={summary}"),
        _ => serde_json::to_string(payload).unwrap_or_else(|_| "<unserializable>".to_owned()),
    }
}

fn first_valid_role(output: &str, team: &TeamSpec) -> Option<String> {
    let candidates = role_candidates(output);
    candidates
        .into_iter()
        .map(|role| role.trim().to_owned())
        .find(|role| !role.is_empty() && team.members.iter().any(|member| member.role == *role))
}

fn roles_to_recipient(roles: &[String], team: &TeamSpec) -> Option<Recipient> {
    roles
        .iter()
        .map(|role| role.trim())
        .find(|role| !role.is_empty() && team.members.iter().any(|member| member.role == *role))
        .map(|role| Recipient::Role(role.to_owned()))
}

fn role_candidates(output: &str) -> Vec<String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(output) {
        if let Some(roles) = value.get("roles").and_then(serde_json::Value::as_array) {
            return roles
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .collect();
        }
        if let Some(role) = value.get("role").and_then(serde_json::Value::as_str) {
            return vec![role.to_owned()];
        }
        if let Some(roles) = value.as_array() {
            return roles
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .collect();
        }
        if let Some(role) = value.as_str() {
            return vec![role.to_owned()];
        }
    }
    output
        .split([',', '\n'])
        .map(|part| part.trim().trim_matches('"').trim_matches('`').to_owned())
        .collect()
}

#[async_trait]
pub trait TopologyStrategy: Send + Sync {
    fn strategy_id(&self) -> &str;

    async fn route(
        &self,
        message: &AgentMessage,
        team: &TeamSpec,
    ) -> Result<Vec<AgentId>, TeamError>;
}

#[derive(Debug, Default, Clone)]
pub struct TeamAnnouncementRenderer;

impl TeamAnnouncementRenderer {
    #[must_use]
    pub fn member_joined(&self, member: &TeamMember) -> String {
        format!("{} joined as {}", member.agent_id, member.role)
    }

    #[must_use]
    pub fn member_left(&self, agent_id: AgentId, reason: &MemberLeaveReason) -> String {
        format!("{agent_id} left: {reason:?}")
    }

    #[must_use]
    pub fn worker_response(
        &self,
        worker: AgentId,
        body: &str,
    ) -> harness_contracts::RenderedAnnouncement {
        let input = harness_contracts::AnnouncementRenderInput::new("team_worker", body)
            .with_label("worker_id", worker.to_string())
            .with_rewrite_hint("Summarize the worker result before continuing.");
        <harness_contracts::XmlTaskNotificationRenderer as harness_contracts::AnnouncementRenderer>::render(
            &harness_contracts::XmlTaskNotificationRenderer,
            &input,
        )
    }
}

#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
pub enum TeamError {
    #[error("team mismatch")]
    TeamMismatch,
    #[error("no subscribers")]
    NoSubscribers,
    #[error("coordinator cannot execute normal tool: {0}")]
    CoordinatorCannotExecute(String),
    #[error("journal: {0}")]
    Journal(String),
    #[error("memory: {0}")]
    Memory(String),
    #[error("shared memory write denied")]
    SharedMemoryWriteDenied,
    #[error("worker missing: {0}")]
    WorkerMissing(AgentId),
    #[error("member already exists: {0}")]
    MemberExists(AgentId),
    #[error("team is paused")]
    Paused,
    #[error("routing limit exceeded: {0}")]
    RoutingLimitExceeded(String),
    #[error("internal: {0}")]
    Internal(String),
    #[error("turn limit exceeded: {limit} turns for team={team_id}")]
    TurnLimitExceeded { team_id: TeamId, limit: u32 },
    #[error("message bus backpressure for team {team_id}: depth {depth}")]
    MessageBusBackpressure { team_id: TeamId, depth: usize },
    #[error("team quota exceeded: {kind:?}")]
    QuotaExceeded { kind: TeamQuotaKind },
    #[error("team is terminated")]
    TeamTerminated,
    #[error("invalid team spec: {0}")]
    InvalidSpec(String),
    #[error("correlation mismatch for member {agent_id}: expected {expected}, got {actual}")]
    CorrelationMismatch {
        agent_id: AgentId,
        expected: CorrelationId,
        actual: CorrelationId,
    },
}

struct TeamInner {
    spec: Arc<Mutex<TeamSpec>>,
    bus: MessageBus,
    journal: TeamJournalContext,
    event_store: Arc<dyn EventStore>,
    blob_store: Arc<dyn BlobStore>,
    workspace_root: PathBuf,
    paused: AtomicBool,
    paused_members: Mutex<HashSet<AgentId>>,
    lifecycle_state: Mutex<TeamLifecycleState>,
    turns_by_goal: Mutex<HashMap<String, u32>>,
    member_sessions: Mutex<HashMap<AgentId, SessionId>>,
    active_member_correlations: Mutex<HashMap<AgentId, CorrelationId>>,
    messages_sent: AtomicU64,
    classifiers: Mutex<HashMap<String, Arc<dyn MessageClassifier>>>,
    topology_strategies: Mutex<HashMap<String, Arc<dyn TopologyStrategy>>>,
    observations: Mutex<TeamObservationState>,
}

#[derive(Debug, Clone)]
struct TeamLifecycleState {
    terminated: Option<TeamTerminationReason>,
    last_activity_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct Team {
    inner: Arc<TeamInner>,
}

impl Team {
    #[must_use]
    pub fn new(
        spec: TeamSpec,
        bus: MessageBus,
        journal: TeamJournalContext,
        event_store: Arc<dyn EventStore>,
        blob_store: Arc<dyn BlobStore>,
    ) -> Self {
        Self::new_with_workspace_root(
            spec,
            bus,
            journal,
            event_store,
            blob_store,
            PathBuf::from("."),
        )
    }

    #[must_use]
    pub fn new_with_workspace_root(
        spec: TeamSpec,
        bus: MessageBus,
        journal: TeamJournalContext,
        event_store: Arc<dyn EventStore>,
        blob_store: Arc<dyn BlobStore>,
        workspace_root: PathBuf,
    ) -> Self {
        let mut bus_spec = spec.message_bus.clone();
        bus_spec.max_messages_per_correlation = spec.max_messages_per_correlation;
        let bus = bus.with_spec(bus_spec);
        Self {
            inner: Arc::new(TeamInner {
                spec: Arc::new(Mutex::new(spec)),
                bus,
                journal,
                event_store,
                blob_store,
                workspace_root,
                paused: AtomicBool::new(false),
                paused_members: Mutex::new(HashSet::new()),
                lifecycle_state: Mutex::new(TeamLifecycleState {
                    terminated: None,
                    last_activity_at: Utc::now(),
                }),
                turns_by_goal: Mutex::new(HashMap::new()),
                member_sessions: Mutex::new(HashMap::new()),
                active_member_correlations: Mutex::new(HashMap::new()),
                messages_sent: AtomicU64::new(0),
                classifiers: Mutex::new(HashMap::new()),
                topology_strategies: Mutex::new(HashMap::new()),
                observations: Mutex::new(TeamObservationState::default()),
            }),
        }
    }

    #[must_use]
    fn inner(&self) -> Arc<TeamInner> {
        Arc::clone(&self.inner)
    }

    pub async fn team_id(&self) -> TeamId {
        self.inner.spec.lock().await.team_id
    }

    pub async fn observation_snapshot(&self) -> TeamObservationSnapshot {
        let mut snapshot = self.inner.observations.lock().await.snapshot();
        snapshot.add_bus_snapshot(&self.inner.bus.observation_snapshot().await);
        snapshot
    }

    pub async fn dispatch(
        &self,
        from: AgentId,
        to: Recipient,
        goal: impl Into<String>,
    ) -> Result<AgentMessage, TeamError> {
        if self.inner.paused.load(Ordering::SeqCst) {
            return Err(TeamError::Paused);
        }
        self.inner.ensure_not_terminated().await?;
        let goal = goal.into();
        self.enforce_turn_limit(&goal).await?;
        let team_id = self.inner.spec.lock().await.team_id;
        let message = AgentMessage::text(team_id, from, to, goal);
        self.post(message).await
    }

    pub async fn post(&self, message: AgentMessage) -> Result<AgentMessage, TeamError> {
        self.inner.ensure_not_terminated().await?;
        let spec = self.inner.spec.lock().await.clone();
        let routed = self.inner.route_for_topology(&message, &spec).await?;
        let policy = Coordinator::routing_policy_for(&message.to);
        self.inner
            .send_routed_guarded(message.clone(), routed, policy)
            .await?;
        Ok(message)
    }

    pub fn pause(&self) {
        self.inner.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.inner.paused.store(false, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.inner.paused.load(Ordering::SeqCst)
    }

    pub async fn register_classifier(
        &self,
        classifier: Arc<dyn MessageClassifier>,
    ) -> Result<(), TeamError> {
        let classifier_id = classifier.classifier_id().to_owned();
        if classifier_id.is_empty() {
            return Err(TeamError::InvalidSpec(
                "classifier_id cannot be empty".to_owned(),
            ));
        }
        let mut classifiers = self.inner.classifiers.lock().await;
        if classifiers.contains_key(&classifier_id) {
            return Err(TeamError::InvalidSpec(format!(
                "classifier_id duplicated: {classifier_id}"
            )));
        }
        classifiers.insert(classifier_id, classifier);
        Ok(())
    }

    pub async fn register_topology_strategy(
        &self,
        strategy: Arc<dyn TopologyStrategy>,
    ) -> Result<(), TeamError> {
        let strategy_id = strategy.strategy_id().to_owned();
        if strategy_id.is_empty() {
            return Err(TeamError::InvalidSpec(
                "topology strategy_id cannot be empty".to_owned(),
            ));
        }
        let mut strategies = self.inner.topology_strategies.lock().await;
        if strategies.contains_key(&strategy_id) {
            return Err(TeamError::InvalidSpec(format!(
                "topology strategy_id duplicated: {strategy_id}"
            )));
        }
        strategies.insert(strategy_id, strategy);
        Ok(())
    }

    pub async fn pause_member(&self, agent_id: AgentId) {
        self.inner.paused_members.lock().await.insert(agent_id);
    }

    pub async fn resume_member(&self, agent_id: AgentId) {
        self.inner.paused_members.lock().await.remove(&agent_id);
    }

    pub async fn is_member_paused(&self, agent_id: AgentId) -> bool {
        self.inner.paused_members.lock().await.contains(&agent_id)
    }

    #[must_use]
    pub fn control_handle(&self) -> TeamControlHandle {
        TeamControlHandle {
            inner: self.inner(),
            runner_registry: None,
        }
    }

    pub async fn add_member(&self, member: TeamMember) -> Result<(), TeamError> {
        let mut spec = self.inner.spec.lock().await;
        if spec
            .members
            .iter()
            .any(|existing| existing.agent_id == member.agent_id)
        {
            return Err(TeamError::MemberExists(member.agent_id));
        }
        if spec
            .quota
            .max_members
            .is_some_and(|limit| spec.members.len() >= limit as usize)
        {
            return Err(TeamError::QuotaExceeded {
                kind: TeamQuotaKind::Members,
            });
        }
        let session_id = SessionId::new();
        Session::builder()
            .with_options(
                SessionOptions::new(self.inner.workspace_root.clone())
                    .with_tenant_id(self.inner.journal.tenant_id)
                    .with_session_id(session_id),
            )
            .with_event_store(Arc::clone(&self.inner.event_store))
            .build()
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        let (spec_snapshot_id, spec_hash) = self.snapshot_member(&member, session_id).await?;
        self.inner
            .event_store
            .append_with_metadata(
                self.inner.journal.tenant_id,
                self.inner.journal.session_id,
                AppendMetadata::default(),
                &[Event::TeamMemberJoined(TeamMemberJoinedEvent {
                    team_id: spec.team_id,
                    agent_id: member.agent_id,
                    role: member.role.clone(),
                    session_id,
                    visibility: member.visibility.clone(),
                    spec_snapshot_id,
                    spec_hash,
                    joined_at: Utc::now(),
                })],
            )
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        self.inner
            .member_sessions
            .lock()
            .await
            .insert(member.agent_id, session_id);
        spec.members.push(member);
        self.inner.observations.lock().await.dynamic_member_adds += 1;
        Ok(())
    }

    pub async fn remove_member(&self, agent_id: AgentId) -> Result<(), TeamError> {
        self.remove_member_with_reason(agent_id, MemberLeaveReason::Removed)
            .await
    }

    pub async fn remove_member_with_reason(
        &self,
        agent_id: AgentId,
        reason: MemberLeaveReason,
    ) -> Result<(), TeamError> {
        let mut spec = self.inner.spec.lock().await;
        let original_len = spec.members.len();
        spec.members.retain(|member| member.agent_id != agent_id);
        if spec.members.len() == original_len {
            return Ok(());
        }
        self.inner
            .event_store
            .append_with_metadata(
                self.inner.journal.tenant_id,
                self.inner.journal.session_id,
                AppendMetadata::default(),
                &[Event::TeamMemberLeft(TeamMemberLeftEvent {
                    team_id: spec.team_id,
                    agent_id,
                    reason,
                    left_at: Utc::now(),
                })],
            )
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        self.inner.member_sessions.lock().await.remove(&agent_id);
        self.inner.observations.lock().await.dynamic_member_removes += 1;
        Ok(())
    }

    pub async fn terminate(&self, reason: TeamTerminationReason) -> Result<TeamReport, TeamError> {
        self.inner
            .terminate(reason, Instant::now(), AppendMetadata::default())
            .await
    }

    pub async fn lifecycle_tick(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<TeamReport>, TeamError> {
        self.inner.lifecycle_tick(now).await
    }

    pub async fn members(&self) -> Vec<TeamMember> {
        self.inner.spec.lock().await.members.clone()
    }

    async fn snapshot_member(
        &self,
        member: &TeamMember,
        session_id: SessionId,
    ) -> Result<(harness_contracts::BlobRef, [u8; 32]), TeamError> {
        let bytes =
            serde_json::to_vec(member).map_err(|error| TeamError::Journal(error.to_string()))?;
        let spec_hash = *blake3::hash(&bytes).as_bytes();
        let spec_snapshot_id = self
            .inner
            .blob_store
            .put(
                self.inner.journal.tenant_id,
                Bytes::from(bytes.clone()),
                BlobMeta {
                    content_type: Some("application/json".to_owned()),
                    size: bytes.len() as u64,
                    content_hash: spec_hash,
                    created_at: Utc::now(),
                    retention: BlobRetention::SessionScoped(session_id),
                },
            )
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        Ok((spec_snapshot_id, spec_hash))
    }

    async fn enforce_turn_limit(&self, goal: &str) -> Result<(), TeamError> {
        self.inner.enforce_turn_limit(goal).await
    }
}

#[derive(Clone)]
pub struct TeamControlHandle {
    inner: Arc<TeamInner>,
    runner_registry: Option<TeamRunnerRegistry>,
}

impl std::fmt::Debug for TeamControlHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TeamControlHandle")
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct TeamControlStatus {
    pub team_id: TeamId,
    pub member_count: usize,
    pub paused_members: Vec<AgentId>,
    pub terminated: Option<TeamTerminationReason>,
}

impl TeamControlHandle {
    pub async fn dispatch(
        &self,
        from: AgentId,
        to: Recipient,
        body: impl Into<String>,
    ) -> Result<AgentMessage, TeamError> {
        let team_id = self.inner.spec.lock().await.team_id;
        let message = AgentMessage::text(team_id, from, to.clone(), body);
        let spec = self.inner.spec.lock().await.clone();
        let resolved = self.inner.route_for_topology(&message, &spec).await?;
        let policy = Coordinator::routing_policy_for(&to);
        self.inner
            .send_routed_guarded(message.clone(), resolved, policy)
            .await?;
        Ok(message)
    }

    pub async fn message(
        &self,
        from: AgentId,
        to: Recipient,
        body: impl Into<String>,
    ) -> Result<AgentMessage, TeamError> {
        self.dispatch(from, to, body).await
    }

    pub async fn stop_team(&self) -> Result<TeamReport, TeamError> {
        self.inner
            .terminate(
                TeamTerminationReason::Cancelled,
                Instant::now(),
                AppendMetadata::default(),
            )
            .await
    }

    pub async fn status(&self) -> TeamControlStatus {
        let spec = self.inner.spec.lock().await.clone();
        let mut paused_members = self
            .inner
            .paused_members
            .lock()
            .await
            .iter()
            .copied()
            .collect::<Vec<_>>();
        paused_members.sort_by_key(ToString::to_string);
        let terminated = self.inner.lifecycle_state.lock().await.terminated.clone();
        TeamControlStatus {
            team_id: spec.team_id,
            member_count: spec.members.len(),
            paused_members,
            terminated,
        }
    }

    pub async fn spawn_worker(&self, member: TeamMember) -> Result<(), TeamError> {
        let team = Team {
            inner: Arc::clone(&self.inner),
        };
        team.add_member(member).await
    }

    pub async fn spawn_worker_with_runner(
        &self,
        member: TeamMember,
        runner: Arc<dyn TeamMemberRunner>,
    ) -> Result<(), TeamError> {
        let registry = self.runner_registry.as_ref().ok_or_else(|| {
            TeamError::InvalidSpec("team control handle has no runner registry".to_owned())
        })?;
        let agent_id = member.agent_id;
        self.spawn_worker(member).await?;
        lock_runner_registry(registry).insert(agent_id, runner);
        Ok(())
    }

    pub async fn pause_worker(&self, agent_id: AgentId) {
        self.inner.paused_members.lock().await.insert(agent_id);
    }

    pub async fn resume_worker(&self, agent_id: AgentId) {
        self.inner.paused_members.lock().await.remove(&agent_id);
    }
}

impl TeamInner {
    async fn ensure_not_terminated(&self) -> Result<(), TeamError> {
        if self.lifecycle_state.lock().await.terminated.is_some() {
            return Err(TeamError::TeamTerminated);
        }
        Ok(())
    }

    async fn touch_activity(&self, at: DateTime<Utc>) {
        self.lifecycle_state.lock().await.last_activity_at = at;
    }

    fn reserve_message_quota(&self, spec: &TeamSpec) -> Result<bool, TeamError> {
        let Some(limit) = spec.quota.max_messages.map(u64::from) else {
            return Ok(false);
        };
        self.messages_sent
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                (current < limit).then_some(current + 1)
            })
            .map(|_| true)
            .map_err(|_| TeamError::QuotaExceeded {
                kind: TeamQuotaKind::Messages,
            })
    }

    fn release_message_quota(&self, reserved: bool) {
        if reserved {
            self.messages_sent.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn enforce_duration_quota(
        &self,
        spec: &TeamSpec,
        started_at: Instant,
    ) -> Result<(), TeamError> {
        if spec
            .quota
            .max_duration
            .is_some_and(|limit| started_at.elapsed() >= limit)
        {
            return Err(TeamError::QuotaExceeded {
                kind: TeamQuotaKind::WallClock,
            });
        }
        Ok(())
    }

    async fn lifecycle_tick(&self, now: DateTime<Utc>) -> Result<Option<TeamReport>, TeamError> {
        let spec = self.spec.lock().await.clone();
        let TeamLifecycle::Persistent { max_idle } = spec.lifecycle else {
            return Ok(None);
        };
        let state = self.lifecycle_state.lock().await.clone();
        if state.terminated.is_some() {
            return Ok(None);
        }
        let Ok(idle_for) = now.signed_duration_since(state.last_activity_at).to_std() else {
            return Ok(None);
        };
        if idle_for < max_idle {
            return Ok(None);
        }
        self.terminate(
            TeamTerminationReason::IdleTimeout,
            Instant::now(),
            AppendMetadata::default(),
        )
        .await
        .map(Some)
    }

    async fn terminate(
        &self,
        reason: TeamTerminationReason,
        started_at: Instant,
        metadata: AppendMetadata,
    ) -> Result<TeamReport, TeamError> {
        self.ensure_not_terminated().await?;
        let spec = self.spec.lock().await.clone();
        let now = Utc::now();
        let mut events = spec
            .members
            .iter()
            .map(|member| {
                Event::TeamMemberLeft(TeamMemberLeftEvent {
                    team_id: spec.team_id,
                    agent_id: member.agent_id,
                    reason: MemberLeaveReason::Interrupted,
                    left_at: now,
                })
            })
            .collect::<Vec<_>>();
        events.push(Event::TeamTerminated(TeamTerminatedEvent {
            team_id: spec.team_id,
            reason: reason.clone(),
            at: now,
        }));
        self.event_store
            .append_with_metadata(
                self.journal.tenant_id,
                self.journal.session_id,
                metadata,
                &events,
            )
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        {
            let mut state = self.lifecycle_state.lock().await;
            state.terminated = Some(reason.clone());
            state.last_activity_at = now;
        }
        let message_count = self.bus.replay().await.len() as u64;
        build_team_report(
            spec.team_id,
            HashMap::new(),
            message_count,
            started_at.elapsed(),
            serde_json::json!({
                "terminated": reason
            }),
        )
    }

    async fn enforce_turn_limit(&self, goal: &str) -> Result<(), TeamError> {
        let spec = self.spec.lock().await.clone();
        let max_turns = spec.max_turns_per_goal;
        let mut turns = self.turns_by_goal.lock().await;
        let count = turns.entry(goal.to_owned()).or_insert(0);
        if *count >= max_turns {
            drop(turns);
            self.terminate(
                TeamTerminationReason::Timeout,
                Instant::now(),
                AppendMetadata::default(),
            )
            .await?;
            return Err(TeamError::TurnLimitExceeded {
                team_id: spec.team_id,
                limit: max_turns,
            });
        }
        *count += 1;
        Ok(())
    }

    async fn enforce_message_limit(
        &self,
        correlation_id: CorrelationId,
    ) -> Result<MessageLimitOutcome, TeamError> {
        self.bus
            .record_message_for_correlation(correlation_id)
            .await
    }

    async fn enforce_active_member_correlation(
        &self,
        message: &AgentMessage,
    ) -> Result<(), TeamError> {
        let expected = self
            .active_member_correlations
            .lock()
            .await
            .get(&message.from)
            .copied();
        if let Some(expected) = expected {
            if expected != message.correlation_id {
                return Err(TeamError::CorrelationMismatch {
                    agent_id: message.from,
                    expected,
                    actual: message.correlation_id,
                });
            }
        }
        Ok(())
    }

    async fn send_routed_guarded(
        &self,
        message: AgentMessage,
        resolved_recipients: Vec<AgentId>,
        routing_policy: RoutingPolicyKind,
    ) -> Result<(), TeamError> {
        if self.paused.load(Ordering::SeqCst) {
            return Err(TeamError::Paused);
        }
        self.ensure_not_terminated().await?;
        self.enforce_active_member_correlation(&message).await?;
        let message_limit = self.enforce_message_limit(message.correlation_id).await?;
        let spec = self.spec.lock().await.clone();
        if message.team_id != spec.team_id {
            return Err(TeamError::TeamMismatch);
        }
        let message_quota_reserved = self.reserve_message_quota(&spec)?;
        if let Recipient::Agent(agent_id) = &message.to {
            if self.paused_members.lock().await.contains(agent_id) {
                self.release_message_quota(message_quota_reserved);
                return Err(TeamError::Paused);
            }
        }
        let (resolved_recipients, routing_policy, deliver) =
            if message_limit == MessageLimitOutcome::Fallback {
                let fallback = self.route_fallback(&message, &spec).await?;
                let deliver = !fallback.recipients.is_empty();
                (
                    fallback.recipients,
                    RoutingPolicyKind::Custom("fallback".to_owned()),
                    deliver,
                )
            } else {
                (resolved_recipients, routing_policy, true)
            };
        let history = self.bus.replay().await;
        let paused_members = self.paused_members.lock().await.clone();
        let visible = filter_visible_recipients(
            &message,
            &spec,
            resolved_recipients.clone(),
            &history,
            &paused_members,
        );
        self.observations.lock().await.context_visibility_blocked +=
            resolved_recipients.len().saturating_sub(visible.len()) as u64;
        let send_result = self
            .bus
            .send_routed_with_delivery(message, visible, routing_policy, deliver)
            .await;
        if send_result.is_err() {
            self.release_message_quota(message_quota_reserved);
        }
        send_result?;
        self.touch_activity(Utc::now()).await;
        Ok(())
    }

    async fn route_for_topology(
        &self,
        message: &AgentMessage,
        team: &TeamSpec,
    ) -> Result<Vec<AgentId>, TeamError> {
        let routed = match team.topology {
            Topology::CoordinatorWorker => CoordinatorWorkerStrategy.route(message, team),
            Topology::PeerToPeer => PeerToPeerStrategy.route(message, team),
            Topology::RoleRouted => self.route_role_routed(message, team).await?,
            Topology::Custom => {
                let strategy_id = team
                    .topology_config
                    .custom_strategy_id
                    .as_ref()
                    .ok_or_else(|| {
                        TeamError::InvalidSpec(
                            "custom topology requires custom_strategy_id".to_owned(),
                        )
                    })?;
                let strategy = self
                    .topology_strategies
                    .lock()
                    .await
                    .get(strategy_id)
                    .cloned()
                    .ok_or_else(|| {
                        TeamError::InvalidSpec(format!(
                            "custom topology strategy is not registered: {strategy_id}"
                        ))
                    })?;
                strategy.route(message, team).await?
            }
        };
        Ok(routed)
    }

    async fn route_role_routed(
        &self,
        message: &AgentMessage,
        team: &TeamSpec,
    ) -> Result<Vec<AgentId>, TeamError> {
        let direct = RoleRoutedStrategy.route(message, team);
        if !direct.is_empty() {
            return Ok(direct);
        }
        let mut rules = team.topology_config.role_rules.clone();
        rules.sort_by(|left, right| right.priority.cmp(&left.priority));
        for rule in rules {
            let roles = match &rule.pattern {
                RoutingPattern::KeywordAny { keywords, roles } => {
                    if message_text(message)
                        .is_some_and(|text| keywords.iter().any(|keyword| text.contains(keyword)))
                    {
                        roles.clone()
                    } else {
                        Vec::new()
                    }
                }
                RoutingPattern::RegexMatch { pattern, roles } => {
                    if message_text(message).is_some_and(|text| {
                        Regex::new(pattern)
                            .map(|regex| regex.is_match(&text))
                            .unwrap_or(false)
                    }) {
                        roles.clone()
                    } else {
                        Vec::new()
                    }
                }
                RoutingPattern::Classifier { classifier_id } => {
                    self.classifier_roles(classifier_id, message).await
                }
            };
            let routed = route_roles_targets(team, &roles);
            if !routed.is_empty() {
                return Ok(routed);
            }
        }
        Ok(self.route_fallback(message, team).await?.recipients)
    }

    async fn classifier_roles(&self, classifier_id: &str, message: &AgentMessage) -> Vec<String> {
        let Some(classifier) = self.classifiers.lock().await.get(classifier_id).cloned() else {
            return Vec::new();
        };
        match tokio::time::timeout(classifier.timeout(), classifier.classify(message)).await {
            Ok(Ok(verdict)) => {
                self.observations.lock().await.classifier_confidences.push(
                    ClassifierConfidenceObservation {
                        classifier_id: classifier_id.to_owned(),
                        confidence: verdict.confidence,
                    },
                );
                verdict.roles
            }
            Ok(Err(_)) => Vec::new(),
            Err(_) => {
                self.observations.lock().await.classifier_timeouts += 1;
                Vec::new()
            }
        }
    }

    async fn route_fallback(
        &self,
        message: &AgentMessage,
        team: &TeamSpec,
    ) -> Result<RouteResolution, TeamError> {
        let recipients = match team.topology_config.route_fallback {
            RouteFallback::DropMessage => Vec::new(),
            RouteFallback::SendToCoordinator => {
                team.coordinator_id().map_or_else(Vec::new, |id| vec![id])
            }
            RouteFallback::Broadcast => team
                .members
                .iter()
                .filter(|member| member.agent_id != message.from)
                .map(|member| member.agent_id)
                .collect(),
        };
        Ok(RouteResolution { recipients })
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum MessageLimitOutcome {
    Allowed,
    Fallback,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct RouteResolution {
    recipients: Vec<AgentId>,
}

fn message_text(message: &AgentMessage) -> Option<&str> {
    match &message.payload {
        MessagePayload::Text(text) => Some(text),
        MessagePayload::Response { body, .. } => {
            body.get("body").and_then(serde_json::Value::as_str)
        }
        _ => None,
    }
}

fn route_role_targets(team: &TeamSpec, role: &str) -> Vec<AgentId> {
    team.topology_config
        .role_routes
        .iter()
        .find(|route| route.role == role)
        .map(|route| route.targets.clone())
        .unwrap_or_else(|| {
            team.members
                .iter()
                .filter(|member| member.role == role)
                .map(|member| member.agent_id)
                .collect()
        })
}

fn route_roles_targets(team: &TeamSpec, roles: &[String]) -> Vec<AgentId> {
    let mut routed = Vec::new();
    for role in roles {
        for agent_id in route_role_targets(team, role) {
            if !routed.contains(&agent_id) {
                routed.push(agent_id);
            }
        }
    }
    routed
}

fn filter_visible_recipients(
    message: &AgentMessage,
    team: &TeamSpec,
    recipients: Vec<AgentId>,
    history: &[AgentMessage],
    paused_members: &HashSet<AgentId>,
) -> Vec<AgentId> {
    recipients
        .into_iter()
        .filter(|agent_id| {
            if paused_members.contains(agent_id) {
                return false;
            }
            team.members
                .iter()
                .find(|member| member.agent_id == *agent_id)
                .is_some_and(|member| member_can_see_message(member, message, history))
        })
        .collect()
}

fn member_can_see_message(
    member: &TeamMember,
    message: &AgentMessage,
    history: &[AgentMessage],
) -> bool {
    let direct = matches!(message.to, Recipient::Agent(agent_id) if agent_id == member.agent_id);
    let self_authored = message.from == member.agent_id;
    match &member.visibility {
        ContextVisibility::All => true,
        ContextVisibility::Private => direct || self_authored,
        ContextVisibility::Allowlist(agents) => {
            direct || self_authored || agents.contains(&message.from)
        }
        ContextVisibility::AllowlistQuote(agents) => {
            direct || self_authored || message_quotes_allowlisted_history(message, history, agents)
        }
        _ => false,
    }
}

fn message_quotes_allowlisted_history(
    message: &AgentMessage,
    history: &[AgentMessage],
    agents: &[AgentId],
) -> bool {
    let Some(quoted_message_id) = quoted_message_id(&message.payload) else {
        return false;
    };
    history
        .iter()
        .any(|message| message.message_id == quoted_message_id && agents.contains(&message.from))
}

fn quoted_message_id(payload: &MessagePayload) -> Option<MessageId> {
    match payload {
        MessagePayload::Request { reply_to } => Some(*reply_to),
        MessagePayload::Response { in_reply_to, .. } => Some(*in_reply_to),
        _ => None,
    }
}

fn response_body_for_recipient(body: &str, from: AgentId, to: &Recipient) -> String {
    if !matches!(to, Recipient::Coordinator) {
        return body.to_owned();
    }
    TeamAnnouncementRenderer
        .worker_response(from, body)
        .user_message
}

pub trait RoutingStrategy {
    fn route(&self, message: &AgentMessage, team: &TeamSpec) -> Vec<AgentId>;
}

pub struct CoordinatorWorkerStrategy;
pub struct PeerToPeerStrategy;
pub struct RoleRoutedStrategy;

impl RoleRoutedStrategy {
    #[must_use]
    pub fn route(&self, message: &AgentMessage, team: &TeamSpec) -> Vec<AgentId> {
        <Self as RoutingStrategy>::route(self, message, team)
    }
}

impl RoutingStrategy for CoordinatorWorkerStrategy {
    fn route(&self, message: &AgentMessage, team: &TeamSpec) -> Vec<AgentId> {
        match &message.to {
            Recipient::Coordinator => team.coordinator_id().map_or_else(Vec::new, |id| vec![id]),
            _ => RoleRoutedStrategy.route(message, team),
        }
    }
}

impl RoutingStrategy for PeerToPeerStrategy {
    fn route(&self, message: &AgentMessage, team: &TeamSpec) -> Vec<AgentId> {
        match &message.to {
            Recipient::Broadcast => team
                .members
                .iter()
                .filter(|member| member.agent_id != message.from)
                .map(|member| member.agent_id)
                .collect(),
            Recipient::Agent(agent_id) => vec![*agent_id],
            _ => RoleRoutedStrategy.route(message, team),
        }
    }
}

impl RoutingStrategy for RoleRoutedStrategy {
    fn route(&self, message: &AgentMessage, team: &TeamSpec) -> Vec<AgentId> {
        match &message.to {
            Recipient::Agent(agent_id) => vec![*agent_id],
            Recipient::Role(role) => route_role_targets(team, role),
            Recipient::Broadcast => team.members.iter().map(|member| member.agent_id).collect(),
            Recipient::Coordinator => team.coordinator_id().map_or_else(Vec::new, |id| vec![id]),
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Coordinator {
    pending_dispatches: HashMap<AgentId, Vec<String>>,
}

impl Coordinator {
    pub fn dispatch(&self, _agent_id: AgentId, task: &str) -> Result<(), TeamError> {
        if task.is_empty() {
            return Err(TeamError::CoordinatorCannotExecute("empty task".to_owned()));
        }
        let _ = &self.pending_dispatches;
        Ok(())
    }

    pub fn execute_normal_tool(&self, tool_name: &str) -> Result<(), TeamError> {
        Err(TeamError::CoordinatorCannotExecute(tool_name.to_owned()))
    }

    #[must_use]
    pub fn routing_policy_for(recipient: &Recipient) -> RoutingPolicyKind {
        match recipient {
            Recipient::Agent(_) => RoutingPolicyKind::Direct,
            Recipient::Role(_) => RoutingPolicyKind::Role,
            Recipient::Broadcast => RoutingPolicyKind::Broadcast,
            Recipient::Coordinator => RoutingPolicyKind::Coordinator,
            _ => RoutingPolicyKind::Custom("unknown".to_owned()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TeamReport {
    pub team_id: TeamId,
    pub members_usage: HashMap<AgentId, UsageSnapshot>,
    pub message_count: u64,
    pub duration: Duration,
    pub report_hash: [u8; 32],
    pub final_state: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectedTeamMember {
    pub agent_id: AgentId,
    pub role: String,
    pub session_id: SessionId,
    pub visibility: ContextVisibility,
    pub engine_config: TeamMemberEngineConfig,
    pub spec_snapshot_id: BlobRef,
    pub spec_hash: [u8; 32],
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectedLeftMember {
    pub agent_id: AgentId,
    pub role: String,
    pub session_id: SessionId,
    pub visibility: ContextVisibility,
    pub engine_config: TeamMemberEngineConfig,
    pub spec_snapshot_id: BlobRef,
    pub spec_hash: [u8; 32],
    pub reason: MemberLeaveReason,
    pub left_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamProjection {
    pub team_id: TeamId,
    pub name: String,
    pub topology_kind: TopologyKind,
    pub members: HashMap<AgentId, ProjectedTeamMember>,
    pub left_members: HashMap<AgentId, ProjectedLeftMember>,
    pub messages: Vec<AgentMessage>,
    pub terminated: Option<TeamTerminationReason>,
}

impl TeamProjection {
    pub async fn replay(
        tenant_id: TenantId,
        session_id: SessionId,
        event_store: Arc<dyn EventStore>,
        blob_store: Arc<dyn BlobStore>,
    ) -> Result<Self, TeamError> {
        let mut stream = event_store
            .read_envelopes(tenant_id, session_id, ReplayCursor::FromStart)
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        let mut projection: Option<Self> = None;
        while let Some(envelope) = stream.next().await {
            match envelope.payload {
                Event::TeamCreated(created) => {
                    projection = Some(Self {
                        team_id: created.team_id,
                        name: created.name,
                        topology_kind: created.topology_kind,
                        members: HashMap::new(),
                        left_members: HashMap::new(),
                        messages: Vec::new(),
                        terminated: None,
                    });
                }
                Event::TeamMemberJoined(joined) => {
                    let projection = projection.get_or_insert_with(|| Self {
                        team_id: joined.team_id,
                        name: String::new(),
                        topology_kind: TopologyKind::Custom("unknown".to_owned()),
                        members: HashMap::new(),
                        left_members: HashMap::new(),
                        messages: Vec::new(),
                        terminated: None,
                    });
                    projection.left_members.remove(&joined.agent_id);
                    let member =
                        Self::read_member_snapshot(tenant_id, Arc::clone(&blob_store), &joined)
                            .await?;
                    projection.members.insert(
                        joined.agent_id,
                        ProjectedTeamMember {
                            agent_id: joined.agent_id,
                            role: joined.role,
                            session_id: joined.session_id,
                            visibility: joined.visibility,
                            engine_config: member.engine_config,
                            spec_snapshot_id: joined.spec_snapshot_id,
                            spec_hash: joined.spec_hash,
                            joined_at: joined.joined_at,
                        },
                    );
                }
                Event::TeamMemberLeft(left) => {
                    let projection = projection.get_or_insert_with(|| Self {
                        team_id: left.team_id,
                        name: String::new(),
                        topology_kind: TopologyKind::Custom("unknown".to_owned()),
                        members: HashMap::new(),
                        left_members: HashMap::new(),
                        messages: Vec::new(),
                        terminated: None,
                    });
                    let member = projection.members.remove(&left.agent_id).ok_or_else(|| {
                        TeamError::Journal("team member left before join".to_owned())
                    })?;
                    projection.left_members.insert(
                        left.agent_id,
                        ProjectedLeftMember {
                            agent_id: left.agent_id,
                            role: member.role,
                            session_id: member.session_id,
                            visibility: member.visibility,
                            engine_config: member.engine_config,
                            spec_snapshot_id: member.spec_snapshot_id,
                            spec_hash: member.spec_hash,
                            reason: left.reason,
                            left_at: left.left_at,
                        },
                    );
                }
                Event::AgentMessageSent(sent) => {
                    if let Some(projection) = projection.as_mut() {
                        if sent.team_id == projection.team_id {
                            projection.messages.push(AgentMessage {
                                team_id: sent.team_id,
                                from: sent.from,
                                to: sent.to,
                                payload: sent.payload,
                                message_id: sent.message_id,
                                sent_at: sent.at,
                                correlation_id: envelope.correlation_id,
                            });
                        }
                    }
                }
                Event::TeamTerminated(terminated) => {
                    if let Some(projection) = projection.as_mut() {
                        projection.terminated = Some(terminated.reason);
                    }
                }
                _ => {}
            }
        }
        projection.ok_or_else(|| TeamError::Journal("team created event missing".to_owned()))
    }

    async fn read_member_snapshot(
        tenant_id: TenantId,
        blob_store: Arc<dyn BlobStore>,
        joined: &TeamMemberJoinedEvent,
    ) -> Result<TeamMember, TeamError> {
        let mut stream = blob_store
            .get(tenant_id, &joined.spec_snapshot_id)
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            bytes.extend_from_slice(&chunk);
        }
        let spec_hash = *blake3::hash(&bytes).as_bytes();
        if spec_hash != joined.spec_hash || spec_hash != joined.spec_snapshot_id.content_hash {
            return Err(TeamError::Journal(
                "team member spec snapshot hash mismatch".to_owned(),
            ));
        }
        let member: TeamMember = serde_json::from_slice(&bytes)
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        if member.agent_id != joined.agent_id
            || member.role != joined.role
            || member.visibility != joined.visibility
        {
            return Err(TeamError::Journal(
                "team member spec snapshot does not match joined event".to_owned(),
            ));
        }
        Ok(member)
    }
}

#[derive(Debug, Clone)]
pub struct TeamMemberRunRequest {
    pub tenant_id: TenantId,
    pub team_id: TeamId,
    pub agent_id: AgentId,
    pub role: String,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub parent_run_id: Option<RunId>,
    pub input: TurnInput,
    pub goal: String,
    pub correlation_id: CorrelationId,
    pub engine_config: TeamMemberEngineConfig,
    pub shared_memory: Option<SharedMemory>,
    pub team_control: Option<TeamControlHandle>,
    pub control_tools_enabled: bool,
    pub cancellation: TeamMemberCancellationToken,
    memory_write_context: Option<TeamMemoryWriteContext>,
}

impl TeamMemberRunRequest {
    #[must_use]
    pub fn synthetic(
        tenant_id: TenantId,
        team_id: TeamId,
        agent_id: AgentId,
        role: impl Into<String>,
        session_id: SessionId,
        run_id: RunId,
        parent_run_id: Option<RunId>,
        input: TurnInput,
        goal: impl Into<String>,
        correlation_id: CorrelationId,
        engine_config: TeamMemberEngineConfig,
    ) -> Self {
        Self {
            tenant_id,
            team_id,
            agent_id,
            role: role.into(),
            session_id,
            run_id,
            parent_run_id,
            input,
            goal: goal.into(),
            correlation_id,
            engine_config,
            shared_memory: None,
            team_control: None,
            control_tools_enabled: false,
            cancellation: TeamMemberCancellationToken::new(),
            memory_write_context: None,
        }
    }

    #[must_use]
    fn runtime_memory_write_context(&self) -> TeamMemoryWriteContext {
        TeamMemoryWriteContext {
            tenant_id: self.tenant_id,
            team_id: self.team_id,
            agent_id: self.agent_id,
            session_id: self.session_id,
            correlation_id: self.correlation_id,
        }
    }

    pub fn memory_write_context(&self) -> Result<TeamMemoryWriteContext, TeamError> {
        self.memory_write_context.ok_or_else(|| {
            TeamError::InvalidSpec("team memory write context is not runtime-issued".to_owned())
        })
    }
}

#[derive(Clone)]
pub struct TeamMemberCancellationToken {
    inner: Arc<TeamMemberCancellationState>,
}

struct TeamMemberCancellationState {
    token: TokioCancellationToken,
}

impl std::fmt::Debug for TeamMemberCancellationToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TeamMemberCancellationToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl Default for TeamMemberCancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl TeamMemberCancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TeamMemberCancellationState {
                token: TokioCancellationToken::new(),
            }),
        }
    }

    pub fn cancel(&self) {
        self.inner.token.cancel();
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.token.is_cancelled()
    }

    pub async fn cancelled(&self) {
        self.inner.token.cancelled().await;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TeamMemberRunOutcome {
    pub body: String,
    pub usage: UsageSnapshot,
}

#[async_trait]
pub trait TeamMemberRunner: Send + Sync + 'static {
    async fn run_member(
        &self,
        request: TeamMemberRunRequest,
    ) -> Result<TeamMemberRunOutcome, TeamError>;
}

type TeamRunnerRegistry = Arc<SyncMutex<HashMap<AgentId, Arc<dyn TeamMemberRunner>>>>;

fn lock_runner_registry(
    registry: &TeamRunnerRegistry,
) -> SyncMutexGuard<'_, HashMap<AgentId, Arc<dyn TeamMemberRunner>>> {
    registry.lock()
}

struct TeamRuntimeCore {
    inner: Arc<TeamInner>,
    workers: TeamRunnerRegistry,
    initialized: Mutex<bool>,
    member_activity: Mutex<HashMap<AgentId, DateTime<Utc>>>,
    shared_memory: Mutex<Option<SharedMemory>>,
    active_member_cancellations: Mutex<HashMap<AgentId, TeamMemberCancellationToken>>,
}

impl TeamRuntimeCore {
    fn new(
        team: TeamSpec,
        bus: MessageBus,
        journal: TeamJournalContext,
        event_store: Arc<dyn EventStore>,
        blob_store: Arc<dyn BlobStore>,
    ) -> Self {
        let team = Team::new(team, bus, journal, event_store, blob_store);
        Self::from_team(team)
    }

    fn from_team(team: Team) -> Self {
        Self {
            inner: team.inner(),
            workers: Arc::new(SyncMutex::new(HashMap::new())),
            initialized: Mutex::new(false),
            member_activity: Mutex::new(HashMap::new()),
            shared_memory: Mutex::new(None),
            active_member_cancellations: Mutex::new(HashMap::new()),
        }
    }

    fn with_member_runner(self, agent_id: AgentId, runner: Arc<dyn TeamMemberRunner>) -> Self {
        lock_runner_registry(&self.workers).insert(agent_id, runner);
        self
    }

    fn control_handle(&self) -> TeamControlHandle {
        TeamControlHandle {
            inner: Arc::clone(&self.inner),
            runner_registry: Some(Arc::clone(&self.workers)),
        }
    }

    fn has_member_runner(&self, agent_id: AgentId) -> bool {
        lock_runner_registry(&self.workers).contains_key(&agent_id)
    }

    async fn ensure_initialized(&self, correlation_id: CorrelationId) -> Result<(), TeamError> {
        let mut initialized = self.initialized.lock().await;
        if *initialized {
            return Ok(());
        }
        let team = self.inner.spec.lock().await.clone();
        team.validate()?;
        *self.shared_memory.lock().await = self.build_shared_memory(&team);

        let member_specs = serde_json::to_vec(&team.members)
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        let member_specs_hash = *blake3::hash(&member_specs).as_bytes();
        self.inner
            .event_store
            .append_with_metadata(
                self.inner.journal.tenant_id,
                self.inner.journal.session_id,
                AppendMetadata {
                    correlation_id,
                    ..AppendMetadata::default()
                },
                &[Event::TeamCreated(TeamCreatedEvent {
                    team_id: team.team_id,
                    tenant_id: self.inner.journal.tenant_id,
                    name: team.name.clone(),
                    topology_kind: topology_kind(team.topology),
                    member_specs_hash,
                    created_at: Utc::now(),
                })],
            )
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;

        let mut member_sessions = self.inner.member_sessions.lock().await;
        for member in &team.members {
            if member_sessions.contains_key(&member.agent_id) {
                self.member_activity
                    .lock()
                    .await
                    .entry(member.agent_id)
                    .or_insert_with(Utc::now);
                continue;
            }
            let session_id = SessionId::new();
            Session::builder()
                .with_options(
                    SessionOptions::new(self.inner.workspace_root.clone())
                        .with_tenant_id(self.inner.journal.tenant_id)
                        .with_session_id(session_id),
                )
                .with_event_store(Arc::clone(&self.inner.event_store))
                .build()
                .await
                .map_err(|error| TeamError::Journal(error.to_string()))?;

            let member_bytes = serde_json::to_vec(member)
                .map_err(|error| TeamError::Journal(error.to_string()))?;
            let member_size = member_bytes.len() as u64;
            let spec_hash = *blake3::hash(&member_bytes).as_bytes();
            let spec_snapshot_id = self
                .inner
                .blob_store
                .put(
                    self.inner.journal.tenant_id,
                    Bytes::from(member_bytes),
                    BlobMeta {
                        content_type: Some("application/json".to_owned()),
                        size: member_size,
                        content_hash: spec_hash,
                        created_at: Utc::now(),
                        retention: BlobRetention::SessionScoped(session_id),
                    },
                )
                .await
                .map_err(|error| TeamError::Journal(error.to_string()))?;
            self.inner
                .event_store
                .append_with_metadata(
                    self.inner.journal.tenant_id,
                    self.inner.journal.session_id,
                    AppendMetadata {
                        correlation_id,
                        ..AppendMetadata::default()
                    },
                    &[Event::TeamMemberJoined(TeamMemberJoinedEvent {
                        team_id: team.team_id,
                        agent_id: member.agent_id,
                        role: member.role.clone(),
                        session_id,
                        visibility: member.visibility.clone(),
                        spec_snapshot_id,
                        spec_hash,
                        joined_at: Utc::now(),
                    })],
                )
                .await
                .map_err(|error| TeamError::Journal(error.to_string()))?;
            member_sessions.insert(member.agent_id, session_id);
            self.member_activity
                .lock()
                .await
                .insert(member.agent_id, Utc::now());
        }
        *initialized = true;
        Ok(())
    }

    async fn member(&self, agent_id: AgentId) -> Result<TeamMember, TeamError> {
        self.inner
            .spec
            .lock()
            .await
            .members
            .iter()
            .find(|member| member.agent_id == agent_id)
            .cloned()
            .ok_or(TeamError::WorkerMissing(agent_id))
    }

    async fn run_member(
        &self,
        member: &TeamMember,
        goal: &str,
        correlation_id: CorrelationId,
    ) -> Result<TeamMemberRunOutcome, TeamError> {
        let worker = lock_runner_registry(&self.workers)
            .get(&member.agent_id)
            .cloned()
            .ok_or(TeamError::WorkerMissing(member.agent_id))?;
        let session_id = self.ensure_member_session(member).await?;
        let team = self.inner.spec.lock().await.clone();
        let team_id = team.team_id;
        let control_tools_enabled =
            team.topology != Topology::PeerToPeer && team.coordinator_id() == Some(member.agent_id);
        let shared_memory = self.shared_memory.lock().await.clone();
        self.inner
            .active_member_correlations
            .lock()
            .await
            .insert(member.agent_id, correlation_id);
        let cancellation = TeamMemberCancellationToken::new();
        self.active_member_cancellations
            .lock()
            .await
            .insert(member.agent_id, cancellation.clone());
        let request = TeamMemberRunRequest {
            tenant_id: self.inner.journal.tenant_id,
            team_id,
            agent_id: member.agent_id,
            role: member.role.clone(),
            session_id,
            run_id: RunId::new(),
            parent_run_id: None,
            input: turn_input(goal),
            goal: goal.to_owned(),
            correlation_id,
            engine_config: member.engine_config.clone(),
            shared_memory,
            team_control: Some(self.control_handle()),
            control_tools_enabled,
            cancellation,
            memory_write_context: None,
        };
        let request = TeamMemberRunRequest {
            memory_write_context: Some(request.runtime_memory_write_context()),
            ..request
        };
        let outcome = worker.run_member(request).await;
        self.inner
            .active_member_correlations
            .lock()
            .await
            .remove(&member.agent_id);
        self.active_member_cancellations
            .lock()
            .await
            .remove(&member.agent_id);
        let outcome = outcome?;
        self.audit_member_correlation(member.agent_id, session_id, correlation_id)
            .await?;
        self.member_activity
            .lock()
            .await
            .insert(member.agent_id, Utc::now());
        Ok(outcome)
    }

    async fn audit_member_correlation(
        &self,
        agent_id: AgentId,
        session_id: SessionId,
        expected: CorrelationId,
    ) -> Result<(), TeamError> {
        let team_id = self.inner.spec.lock().await.team_id;
        let mut stream = self
            .inner
            .event_store
            .read_envelopes(
                self.inner.journal.tenant_id,
                session_id,
                ReplayCursor::FromStart,
            )
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        while let Some(envelope) = stream.next().await {
            if let Event::MemoryUpserted(event) = &envelope.payload {
                if matches!(event.visibility, MemoryVisibility::Team { team_id: event_team_id } if event_team_id == team_id)
                    && envelope.correlation_id != expected
                {
                    return Err(TeamError::CorrelationMismatch {
                        agent_id,
                        expected,
                        actual: envelope.correlation_id,
                    });
                }
            }
        }
        Ok(())
    }

    fn build_shared_memory(&self, team: &TeamSpec) -> Option<SharedMemory> {
        match &team.shared_memory {
            SharedMemorySpec::Disabled => None,
            SharedMemorySpec::Enabled {
                provider_id,
                write_policy,
            } => {
                let mut memory = SharedMemory::new(team.team_id, provider_id.clone())
                    .with_policy(write_policy.clone())
                    .with_journal(self.inner.journal, Arc::clone(&self.inner.event_store));
                for member in &team.members {
                    memory = memory.with_role(member.agent_id, member.role.clone());
                }
                Some(memory)
            }
        }
    }

    async fn ensure_member_session(&self, member: &TeamMember) -> Result<SessionId, TeamError> {
        let mut member_sessions = self.inner.member_sessions.lock().await;
        if let Some(session_id) = member_sessions.get(&member.agent_id) {
            self.member_activity
                .lock()
                .await
                .insert(member.agent_id, Utc::now());
            return Ok(*session_id);
        }
        let session_id = SessionId::new();
        Session::builder()
            .with_options(
                SessionOptions::new(self.inner.workspace_root.clone())
                    .with_tenant_id(self.inner.journal.tenant_id)
                    .with_session_id(session_id),
            )
            .with_event_store(Arc::clone(&self.inner.event_store))
            .build()
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        member_sessions.insert(member.agent_id, session_id);
        self.member_activity
            .lock()
            .await
            .insert(member.agent_id, Utc::now());
        Ok(session_id)
    }

    async fn watchdog_tick(&self, stalled_after: Duration) -> Result<Vec<AgentId>, TeamError> {
        self.watchdog_tick_with_action(stalled_after, StalledAction::Reported)
            .await
    }

    async fn watchdog_tick_with_action(
        &self,
        stalled_after: Duration,
        action: StalledAction,
    ) -> Result<Vec<AgentId>, TeamError> {
        let now = Utc::now();
        let team_id = self.inner.spec.lock().await.team_id;
        let sessions = self.inner.member_sessions.lock().await.clone();
        let activity = self.member_activity.lock().await.clone();
        let mut stalled = Vec::new();
        let mut events = Vec::new();
        for (agent_id, session_id) in sessions {
            let Some(last_activity_at) = activity.get(&agent_id).copied() else {
                continue;
            };
            let Ok(silent_for) = now.signed_duration_since(last_activity_at).to_std() else {
                continue;
            };
            if silent_for < stalled_after {
                continue;
            }
            stalled.push(agent_id);
            events.push(Event::TeamMemberStalled(TeamMemberStalledEvent {
                team_id,
                agent_id,
                session_id,
                last_activity_at,
                stalled_for: silent_for,
                action,
                at: now,
            }));
            if action == StalledAction::Removed {
                events.push(Event::TeamMemberLeft(TeamMemberLeftEvent {
                    team_id,
                    agent_id,
                    reason: MemberLeaveReason::StalledRemoved,
                    left_at: now,
                }));
            }
        }
        if !events.is_empty() {
            self.inner
                .event_store
                .append_with_metadata(
                    self.inner.journal.tenant_id,
                    self.inner.journal.session_id,
                    AppendMetadata::default(),
                    &events,
                )
                .await
                .map_err(|error| TeamError::Journal(error.to_string()))?;
        }
        if action == StalledAction::Removed && !stalled.is_empty() {
            let stalled_set = stalled.iter().copied().collect::<HashSet<_>>();
            self.inner
                .spec
                .lock()
                .await
                .members
                .retain(|member| !stalled_set.contains(&member.agent_id));
            self.inner
                .member_sessions
                .lock()
                .await
                .retain(|agent_id, _| !stalled_set.contains(agent_id));
            self.member_activity
                .lock()
                .await
                .retain(|agent_id, _| !stalled_set.contains(agent_id));
        }
        Ok(stalled)
    }

    async fn send_response(
        &self,
        from: AgentId,
        to: Recipient,
        resolved_recipients: Vec<AgentId>,
        routing_policy: RoutingPolicyKind,
        body: String,
        correlation_id: CorrelationId,
    ) -> Result<AgentMessage, TeamError> {
        let team_id = self.inner.spec.lock().await.team_id;
        let response_body = response_body_for_recipient(&body, from, &to);
        let response = AgentMessage::with_correlation(
            team_id,
            from,
            to,
            MessagePayload::Response {
                in_reply_to: MessageId::new(),
                body: serde_json::json!({ "body": response_body }),
            },
            correlation_id,
        );
        self.inner
            .send_routed_guarded(response.clone(), resolved_recipients, routing_policy)
            .await?;
        Ok(response)
    }

    async fn complete_turn(
        &self,
        started_at: Instant,
        correlation_id: CorrelationId,
        usage: UsageSnapshot,
        members_usage: HashMap<AgentId, UsageSnapshot>,
        responses: Vec<AgentMessage>,
    ) -> Result<TeamReport, TeamError> {
        let response_count = responses.len();
        self.complete_turn_with_state(
            started_at,
            correlation_id,
            usage,
            members_usage,
            serde_json::json!({
                "responses": response_count
            }),
            responses,
        )
        .await
    }

    async fn complete_turn_with_state(
        &self,
        started_at: Instant,
        correlation_id: CorrelationId,
        usage: UsageSnapshot,
        members_usage: HashMap<AgentId, UsageSnapshot>,
        final_state: serde_json::Value,
        responses: Vec<AgentMessage>,
    ) -> Result<TeamReport, TeamError> {
        let spec = self.inner.spec.lock().await.clone();
        self.inner.enforce_duration_quota(&spec, started_at)?;
        let team_id = spec.team_id;
        let mut participating_agents = members_usage.keys().copied().collect::<Vec<_>>();
        participating_agents.sort_by_key(ToString::to_string);
        let transcript_ref = self
            .turn_transcript_ref(
                &spec,
                correlation_id,
                &usage,
                &participating_agents,
                &responses,
            )
            .await?;
        self.inner
            .event_store
            .append_with_metadata(
                self.inner.journal.tenant_id,
                self.inner.journal.session_id,
                AppendMetadata {
                    correlation_id,
                    ..AppendMetadata::default()
                },
                &[Event::TeamTurnCompleted(TeamTurnCompletedEvent {
                    team_id,
                    turn_id: RunId::new(),
                    participating_agents,
                    usage: usage.clone(),
                    transcript_ref,
                    at: Utc::now(),
                })],
            )
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;

        build_team_report(
            team_id,
            members_usage,
            self.inner.bus.replay().await.len() as u64,
            started_at.elapsed(),
            final_state,
        )
    }

    async fn turn_transcript_ref(
        &self,
        spec: &TeamSpec,
        correlation_id: CorrelationId,
        usage: &UsageSnapshot,
        participating_agents: &[AgentId],
        responses: &[AgentMessage],
    ) -> Result<Option<TranscriptRef>, TeamError> {
        if !spec.observability.capture_transcript {
            return Ok(None);
        }
        let messages = self.inner.bus.replay().await;
        let transcript = serde_json::json!({
            "team_id": spec.team_id,
            "correlation_id": correlation_id,
            "participating_agents": participating_agents,
            "usage": usage,
            "messages": messages,
            "responses": responses,
        });
        let bytes = serde_json::to_vec(&transcript)
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        let content_hash = *blake3::hash(&bytes).as_bytes();
        let blob = self
            .inner
            .blob_store
            .put(
                self.inner.journal.tenant_id,
                Bytes::from(bytes.clone()),
                BlobMeta {
                    content_type: Some("application/json".to_owned()),
                    size: bytes.len() as u64,
                    content_hash,
                    created_at: Utc::now(),
                    retention: BlobRetention::SessionScoped(self.inner.journal.session_id),
                },
            )
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        let to_offset = self.current_team_journal_offset().await?;
        Ok(Some(TranscriptRef {
            blob,
            from_offset: JournalOffset(0),
            to_offset,
        }))
    }

    async fn current_team_journal_offset(&self) -> Result<JournalOffset, TeamError> {
        let mut stream = self
            .inner
            .event_store
            .read_envelopes(
                self.inner.journal.tenant_id,
                self.inner.journal.session_id,
                ReplayCursor::FromStart,
            )
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        let mut offset = JournalOffset(0);
        while let Some(envelope) = stream.next().await {
            offset = envelope.offset;
        }
        Ok(offset)
    }

    async fn terminate(&self, reason: TeamTerminationReason) -> Result<TeamReport, TeamError> {
        let started_at = Instant::now();
        let correlation_id = CorrelationId::new();
        self.ensure_initialized(correlation_id).await?;
        let cancellations = self
            .active_member_cancellations
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for cancellation in cancellations {
            cancellation.cancel();
        }
        self.inner
            .terminate(
                reason,
                started_at,
                AppendMetadata {
                    correlation_id,
                    ..AppendMetadata::default()
                },
            )
            .await
    }

    async fn lifecycle_tick(&self, now: DateTime<Utc>) -> Result<Option<TeamReport>, TeamError> {
        self.inner.lifecycle_tick(now).await
    }
}

pub struct CoordinatorWorkerRuntime {
    core: TeamRuntimeCore,
}

impl CoordinatorWorkerRuntime {
    #[must_use]
    pub fn new(
        team: TeamSpec,
        bus: MessageBus,
        journal: TeamJournalContext,
        event_store: Arc<dyn EventStore>,
        blob_store: Arc<dyn BlobStore>,
    ) -> Self {
        Self {
            core: TeamRuntimeCore::new(team, bus, journal, event_store, blob_store),
        }
    }

    #[must_use]
    pub fn from_team(team: Team) -> Self {
        Self {
            core: TeamRuntimeCore::from_team(team),
        }
    }

    #[must_use]
    pub fn with_member_runner(
        mut self,
        agent_id: AgentId,
        runner: Arc<dyn TeamMemberRunner>,
    ) -> Self {
        self.core = self.core.with_member_runner(agent_id, runner);
        self
    }

    #[must_use]
    pub fn control_handle(&self) -> TeamControlHandle {
        self.core.control_handle()
    }

    pub async fn dispatch_goal(&self, goal: &str) -> Result<TeamReport, TeamError> {
        let started_at = Instant::now();
        let correlation_id = CorrelationId::new();
        self.core.ensure_initialized(correlation_id).await?;
        self.core.inner.ensure_not_terminated().await?;
        self.core.inner.enforce_turn_limit(goal).await?;
        let team = self.core.inner.spec.lock().await.clone();
        let coordinator = team.coordinator_id().ok_or_else(|| {
            TeamError::InvalidSpec("coordinator_worker requires coordinator".to_owned())
        })?;
        if !self.core.has_member_runner(coordinator) {
            return Err(TeamError::InvalidSpec(
                "coordinator runner is required".to_owned(),
            ));
        }
        let member = self.core.member(coordinator).await?;
        let outcome = self.core.run_member(&member, goal, correlation_id).await?;
        let mut members_usage = HashMap::new();
        members_usage.insert(coordinator, outcome.usage.clone());
        self.core
            .complete_turn_with_state(
                started_at,
                correlation_id,
                outcome.usage,
                members_usage,
                serde_json::json!({
                    "coordinator_engine": true,
                    "responses": 0
                }),
                Vec::new(),
            )
            .await
    }

    pub async fn terminate(&self, reason: TeamTerminationReason) -> Result<TeamReport, TeamError> {
        self.core.terminate(reason).await
    }

    pub async fn lifecycle_tick(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<TeamReport>, TeamError> {
        self.core.lifecycle_tick(now).await
    }

    pub async fn watchdog_tick(&self, stalled_after: Duration) -> Result<Vec<AgentId>, TeamError> {
        self.core.watchdog_tick(stalled_after).await
    }

    pub async fn watchdog_tick_with_action(
        &self,
        stalled_after: Duration,
        action: StalledAction,
    ) -> Result<Vec<AgentId>, TeamError> {
        self.core
            .watchdog_tick_with_action(stalled_after, action)
            .await
    }
}

pub struct PeerToPeerRuntime {
    core: TeamRuntimeCore,
}

impl PeerToPeerRuntime {
    #[must_use]
    pub fn new(
        team: TeamSpec,
        bus: MessageBus,
        journal: TeamJournalContext,
        event_store: Arc<dyn EventStore>,
        blob_store: Arc<dyn BlobStore>,
    ) -> Self {
        Self {
            core: TeamRuntimeCore::new(team, bus, journal, event_store, blob_store),
        }
    }

    #[must_use]
    pub fn from_team(team: Team) -> Self {
        Self {
            core: TeamRuntimeCore::from_team(team),
        }
    }

    #[must_use]
    pub fn with_member_runner(
        mut self,
        agent_id: AgentId,
        runner: Arc<dyn TeamMemberRunner>,
    ) -> Self {
        self.core = self.core.with_member_runner(agent_id, runner);
        self
    }

    #[must_use]
    pub fn control_handle(&self) -> TeamControlHandle {
        self.core.control_handle()
    }

    pub async fn dispatch_goal(&self, from: AgentId, goal: &str) -> Result<TeamReport, TeamError> {
        let started_at = Instant::now();
        let correlation_id = CorrelationId::new();
        self.core.ensure_initialized(correlation_id).await?;
        self.core.inner.ensure_not_terminated().await?;
        self.core.inner.enforce_turn_limit(goal).await?;
        let team = self.core.inner.spec.lock().await.clone();
        let dispatch = AgentMessage::text_with_correlation(
            team.team_id,
            from,
            Recipient::Broadcast,
            goal,
            correlation_id,
        );
        let targets = PeerToPeerStrategy.route(&dispatch, &team);
        self.core
            .inner
            .send_routed_guarded(dispatch, targets.clone(), RoutingPolicyKind::Broadcast)
            .await?;

        let mut responses = Vec::new();
        let mut members_usage = HashMap::new();
        let mut total_usage = UsageSnapshot::default();
        for target in targets {
            let member = self.core.member(target).await?;
            let outcome = self.core.run_member(&member, goal, correlation_id).await?;
            add_usage(&mut total_usage, &outcome.usage);
            members_usage.insert(member.agent_id, outcome.usage.clone());
            responses.push(
                self.core
                    .send_response(
                        member.agent_id,
                        Recipient::Agent(from),
                        vec![from],
                        RoutingPolicyKind::Direct,
                        outcome.body,
                        correlation_id,
                    )
                    .await?,
            );
        }
        self.core
            .complete_turn(
                started_at,
                correlation_id,
                total_usage,
                members_usage,
                responses,
            )
            .await
    }

    pub async fn terminate(&self, reason: TeamTerminationReason) -> Result<TeamReport, TeamError> {
        self.core.terminate(reason).await
    }

    pub async fn lifecycle_tick(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<TeamReport>, TeamError> {
        self.core.lifecycle_tick(now).await
    }

    pub async fn watchdog_tick(&self, stalled_after: Duration) -> Result<Vec<AgentId>, TeamError> {
        self.core.watchdog_tick(stalled_after).await
    }

    pub async fn watchdog_tick_with_action(
        &self,
        stalled_after: Duration,
        action: StalledAction,
    ) -> Result<Vec<AgentId>, TeamError> {
        self.core
            .watchdog_tick_with_action(stalled_after, action)
            .await
    }
}

pub struct RoleRoutedRuntime {
    core: TeamRuntimeCore,
    aux_role_classifier: Option<Arc<AuxRoleClassifier>>,
    role_routing_table: Option<RoleRoutingTable>,
}

impl RoleRoutedRuntime {
    #[must_use]
    pub fn new(
        team: TeamSpec,
        bus: MessageBus,
        journal: TeamJournalContext,
        event_store: Arc<dyn EventStore>,
        blob_store: Arc<dyn BlobStore>,
    ) -> Self {
        Self {
            core: TeamRuntimeCore::new(team, bus, journal, event_store, blob_store),
            aux_role_classifier: None,
            role_routing_table: None,
        }
    }

    #[must_use]
    pub fn from_team(team: Team) -> Self {
        Self {
            core: TeamRuntimeCore::from_team(team),
            aux_role_classifier: None,
            role_routing_table: None,
        }
    }

    #[must_use]
    pub fn with_member_runner(
        mut self,
        agent_id: AgentId,
        runner: Arc<dyn TeamMemberRunner>,
    ) -> Self {
        self.core = self.core.with_member_runner(agent_id, runner);
        self
    }

    #[must_use]
    pub fn with_aux_role_classifier(mut self, classifier: Arc<AuxRoleClassifier>) -> Self {
        self.aux_role_classifier = Some(classifier);
        self
    }

    #[must_use]
    pub fn with_role_routing_table(mut self, table: RoleRoutingTable) -> Self {
        self.role_routing_table = Some(table);
        self
    }

    #[must_use]
    pub fn control_handle(&self) -> TeamControlHandle {
        self.core.control_handle()
    }

    pub async fn dispatch_goal(
        &self,
        from: AgentId,
        recipient: Recipient,
        goal: &str,
    ) -> Result<TeamReport, TeamError> {
        let started_at = Instant::now();
        self.dispatch_goal_to_recipient(from, recipient, goal, started_at)
            .await
    }

    pub async fn dispatch_goal_classified(
        &self,
        from: AgentId,
        goal: &str,
        fallback_recipient: Recipient,
    ) -> Result<TeamReport, TeamError> {
        let started_at = Instant::now();
        let correlation_id = CorrelationId::new();
        self.core.ensure_initialized(correlation_id).await?;
        self.core.inner.ensure_not_terminated().await?;
        self.core.inner.enforce_turn_limit(goal).await?;
        let team = self.core.inner.spec.lock().await.clone();
        let dispatch = AgentMessage::text_with_correlation(
            team.team_id,
            from,
            fallback_recipient.clone(),
            goal,
            correlation_id,
        );
        let recipient = match &self.role_routing_table {
            Some(table) => table.route(&dispatch, &team).await,
            None => match &self.aux_role_classifier {
                Some(classifier) => classifier
                    .classify_role(&dispatch, &team)
                    .await
                    .map(Recipient::Role)
                    .or(Some(fallback_recipient)),
                None => Some(fallback_recipient),
            },
        };
        let Some(recipient) = recipient else {
            return self
                .core
                .complete_turn(
                    started_at,
                    correlation_id,
                    UsageSnapshot::default(),
                    HashMap::new(),
                    Vec::new(),
                )
                .await;
        };
        self.dispatch_goal_prepared(from, recipient, goal, started_at, correlation_id, team)
            .await
    }

    async fn dispatch_goal_to_recipient(
        &self,
        from: AgentId,
        recipient: Recipient,
        goal: &str,
        started_at: Instant,
    ) -> Result<TeamReport, TeamError> {
        let correlation_id = CorrelationId::new();
        self.core.ensure_initialized(correlation_id).await?;
        self.core.inner.ensure_not_terminated().await?;
        self.core.inner.enforce_turn_limit(goal).await?;
        let team = self.core.inner.spec.lock().await.clone();
        self.dispatch_goal_prepared(from, recipient, goal, started_at, correlation_id, team)
            .await
    }

    async fn dispatch_goal_prepared(
        &self,
        from: AgentId,
        recipient: Recipient,
        goal: &str,
        started_at: Instant,
        correlation_id: CorrelationId,
        team: TeamSpec,
    ) -> Result<TeamReport, TeamError> {
        let dispatch = AgentMessage::text_with_correlation(
            team.team_id,
            from,
            recipient.clone(),
            goal,
            correlation_id,
        );
        let targets = self.core.inner.route_for_topology(&dispatch, &team).await?;
        let policy = Coordinator::routing_policy_for(&recipient);
        self.core
            .inner
            .send_routed_guarded(dispatch, targets.clone(), policy)
            .await?;

        let mut responses = Vec::new();
        let mut members_usage = HashMap::new();
        let mut total_usage = UsageSnapshot::default();
        for target in targets {
            let member = self.core.member(target).await?;
            let outcome = self.core.run_member(&member, goal, correlation_id).await?;
            add_usage(&mut total_usage, &outcome.usage);
            members_usage.insert(member.agent_id, outcome.usage.clone());
            responses.push(
                self.core
                    .send_response(
                        member.agent_id,
                        Recipient::Agent(from),
                        vec![from],
                        RoutingPolicyKind::Direct,
                        outcome.body,
                        correlation_id,
                    )
                    .await?,
            );
        }
        self.core
            .complete_turn(
                started_at,
                correlation_id,
                total_usage,
                members_usage,
                responses,
            )
            .await
    }

    pub async fn terminate(&self, reason: TeamTerminationReason) -> Result<TeamReport, TeamError> {
        self.core.terminate(reason).await
    }

    pub async fn lifecycle_tick(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<TeamReport>, TeamError> {
        self.core.lifecycle_tick(now).await
    }

    pub async fn watchdog_tick(&self, stalled_after: Duration) -> Result<Vec<AgentId>, TeamError> {
        self.core.watchdog_tick(stalled_after).await
    }

    pub async fn watchdog_tick_with_action(
        &self,
        stalled_after: Duration,
        action: StalledAction,
    ) -> Result<Vec<AgentId>, TeamError> {
        self.core
            .watchdog_tick_with_action(stalled_after, action)
            .await
    }
}

fn topology_kind(topology: Topology) -> TopologyKind {
    match topology {
        Topology::CoordinatorWorker => TopologyKind::CoordinatorWorker,
        Topology::PeerToPeer => TopologyKind::PeerToPeer,
        Topology::RoleRouted => TopologyKind::RoleRouted,
        Topology::Custom => TopologyKind::Custom("custom".to_owned()),
    }
}

fn turn_input(text: &str) -> TurnInput {
    TurnInput {
        message: Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(text.to_owned())],
            created_at: Utc::now(),
        },
        metadata: serde_json::Value::Null,
    }
}

fn add_usage(total: &mut UsageSnapshot, usage: &UsageSnapshot) {
    total.input_tokens = total.input_tokens.saturating_add(usage.input_tokens);
    total.output_tokens = total.output_tokens.saturating_add(usage.output_tokens);
    total.cache_read_tokens = total
        .cache_read_tokens
        .saturating_add(usage.cache_read_tokens);
    total.cache_write_tokens = total
        .cache_write_tokens
        .saturating_add(usage.cache_write_tokens);
    total.cost_micros = total.cost_micros.saturating_add(usage.cost_micros);
    total.tool_calls = total.tool_calls.saturating_add(usage.tool_calls);
}

fn build_team_report(
    team_id: TeamId,
    members_usage: HashMap<AgentId, UsageSnapshot>,
    message_count: u64,
    duration: Duration,
    final_state: serde_json::Value,
) -> Result<TeamReport, TeamError> {
    let duration_nanos = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
    let hash_input = serde_json::json!({
        "team_id": team_id,
        "members_usage": members_usage.clone(),
        "message_count": message_count,
        "duration_nanos": duration_nanos,
        "final_state": final_state.clone(),
    });
    let bytes =
        serde_json::to_vec(&hash_input).map_err(|error| TeamError::Journal(error.to_string()))?;
    Ok(TeamReport {
        team_id,
        members_usage,
        message_count,
        duration,
        report_hash: *blake3::hash(&bytes).as_bytes(),
        final_state,
    })
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedWritePolicy {
    Unrestricted,
    CoordinatorOnly { coordinator: AgentId },
    RoleGated { allowed_roles: Vec<String> },
    PerMemberQuota { max_entries: usize },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_field_names)]
pub struct TeamMemoryWriteContext {
    tenant_id: TenantId,
    team_id: TeamId,
    agent_id: AgentId,
    session_id: SessionId,
    correlation_id: CorrelationId,
}

#[derive(Clone)]
pub struct SharedMemory {
    provider_id: String,
    team_id: TeamId,
    write_policy: SharedWritePolicy,
    roles: HashMap<AgentId, String>,
    entries: Arc<Mutex<Vec<MemoryRecord>>>,
    writes_by_agent: Arc<Mutex<HashMap<AgentId, usize>>>,
    event_store: Option<Arc<dyn EventStore>>,
    journal: Option<TeamJournalContext>,
}

impl std::fmt::Debug for SharedMemory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SharedMemory")
            .field("provider_id", &self.provider_id)
            .field("team_id", &self.team_id)
            .field("write_policy", &self.write_policy)
            .field("roles", &self.roles)
            .field("journal", &self.journal)
            .finish_non_exhaustive()
    }
}

impl SharedMemory {
    #[must_use]
    pub fn new(team_id: TeamId, provider_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            team_id,
            write_policy: SharedWritePolicy::Unrestricted,
            roles: HashMap::new(),
            entries: Arc::new(Mutex::new(Vec::new())),
            writes_by_agent: Arc::new(Mutex::new(HashMap::new())),
            event_store: None,
            journal: None,
        }
    }

    #[must_use]
    pub fn with_policy(mut self, policy: SharedWritePolicy) -> Self {
        self.write_policy = policy;
        self
    }

    #[must_use]
    pub fn with_role(mut self, agent_id: AgentId, role: impl Into<String>) -> Self {
        self.roles.insert(agent_id, role.into());
        self
    }

    #[must_use]
    pub fn with_journal(
        mut self,
        journal: TeamJournalContext,
        event_store: Arc<dyn EventStore>,
    ) -> Self {
        self.journal = Some(journal);
        self.event_store = Some(event_store);
        self
    }

    async fn write_for_agent(
        &self,
        agent_id: AgentId,
        session_id: harness_contracts::SessionId,
        value: impl Into<String>,
        correlation_id: CorrelationId,
    ) -> Result<MemoryId, TeamError> {
        let (event_store, journal) = match (&self.event_store, self.journal) {
            (Some(event_store), Some(journal)) => (event_store, journal),
            _ => {
                return Err(TeamError::Journal(
                    "shared memory writes require journal".to_owned(),
                ));
            }
        };
        self.ensure_write_allowed(agent_id).await?;
        let now = Utc::now();
        let content = value.into();
        let record = MemoryRecord {
            id: MemoryId::new(),
            tenant_id: journal.tenant_id,
            kind: MemoryKind::ProjectFact,
            visibility: MemoryVisibility::Team {
                team_id: self.team_id,
            },
            content,
            metadata: MemoryMetadata {
                tags: Vec::new(),
                source: MemorySource::AgentDerived,
                confidence: 1.0,
                access_count: 0,
                last_accessed_at: None,
                recall_score: 1.0,
                ttl: None,
                redacted_segments: 0,
            },
            created_at: now,
            updated_at: now,
        };
        let memory_id = record.id;
        event_store
            .append_with_metadata(
                journal.tenant_id,
                session_id,
                AppendMetadata {
                    correlation_id,
                    ..AppendMetadata::default()
                },
                &[Event::MemoryUpserted(MemoryUpsertedEvent {
                    session_id,
                    run_id: None,
                    memory_id,
                    kind: record.kind.clone(),
                    visibility: record.visibility.clone(),
                    action: MemoryWriteAction::Upsert,
                    provider_id: self.provider_id.clone(),
                    source: record.metadata.source.clone(),
                    content_hash: hash_content(record.content.as_bytes()),
                    bytes_written: record.content.len() as u64,
                    takes_effect: TakesEffect::CurrentSession,
                    at: Utc::now(),
                })],
            )
            .await
            .map_err(|error| TeamError::Journal(error.to_string()))?;
        self.upsert_record_unchecked(record).await?;
        self.commit_write(agent_id).await;
        Ok(memory_id)
    }

    pub async fn write_from_context(
        &self,
        context: TeamMemoryWriteContext,
        value: impl Into<String>,
    ) -> Result<MemoryId, TeamError> {
        if context.team_id != self.team_id {
            return Err(TeamError::TeamMismatch);
        }
        if let Some(journal) = self.journal {
            if context.tenant_id != journal.tenant_id {
                return Err(TeamError::Journal(
                    "shared memory context does not match journal".to_owned(),
                ));
            }
        }
        self.write_for_agent(
            context.agent_id,
            context.session_id,
            value,
            context.correlation_id,
        )
        .await
    }

    async fn ensure_write_allowed(&self, agent_id: AgentId) -> Result<(), TeamError> {
        match &self.write_policy {
            SharedWritePolicy::Unrestricted => Ok(()),
            SharedWritePolicy::CoordinatorOnly { coordinator } if *coordinator == agent_id => {
                Ok(())
            }
            SharedWritePolicy::RoleGated { allowed_roles } => {
                let role = self.roles.get(&agent_id);
                if role.is_some_and(|role| allowed_roles.contains(role)) {
                    Ok(())
                } else {
                    Err(TeamError::SharedMemoryWriteDenied)
                }
            }
            SharedWritePolicy::PerMemberQuota { max_entries } => {
                let writes = self.writes_by_agent.lock().await;
                let count = writes.get(&agent_id).copied().unwrap_or(0);
                if count >= *max_entries {
                    Err(TeamError::SharedMemoryWriteDenied)
                } else {
                    Ok(())
                }
            }
            SharedWritePolicy::CoordinatorOnly { .. } => Err(TeamError::SharedMemoryWriteDenied),
        }
    }

    async fn commit_write(&self, agent_id: AgentId) {
        if matches!(self.write_policy, SharedWritePolicy::PerMemberQuota { .. }) {
            let mut writes = self.writes_by_agent.lock().await;
            *writes.entry(agent_id).or_insert(0) += 1;
        }
    }

    async fn upsert_record_unchecked(&self, record: MemoryRecord) -> Result<MemoryId, TeamError> {
        let mut entries = self.entries.lock().await;
        if let Some(existing) = entries.iter_mut().find(|entry| entry.id == record.id) {
            *existing = record.clone();
        } else {
            entries.push(record.clone());
        }
        Ok(record.id)
    }
}

#[async_trait]
impl MemoryStore for SharedMemory {
    fn provider_id(&self) -> &str {
        &self.provider_id
    }

    async fn recall(&self, query: MemoryQuery) -> Result<Vec<MemoryRecord>, MemoryError> {
        let entries = self.entries.lock().await;
        Ok(entries
            .iter()
            .filter(|record| record.tenant_id == query.tenant_id)
            .filter(|record| memory_record_visible(record, &query.visibility_filter))
            .take(query.max_records as usize)
            .cloned()
            .collect())
    }

    async fn upsert(&self, record: MemoryRecord) -> Result<MemoryId, MemoryError> {
        let _ = record;
        Err(MemoryError::Message(
            "use write_from_context for audited shared memory writes".to_owned(),
        ))
    }

    async fn forget(&self, id: MemoryId) -> Result<(), MemoryError> {
        let _ = id;
        Err(MemoryError::Message(
            "use write_from_context for audited shared memory writes".to_owned(),
        ))
    }

    async fn list(&self, scope: MemoryListScope) -> Result<Vec<MemorySummary>, MemoryError> {
        let entries = self.entries.lock().await;
        Ok(entries
            .iter()
            .filter(|record| match &scope {
                MemoryListScope::All => true,
                MemoryListScope::ByKind(kind) => &record.kind == kind,
                MemoryListScope::ByVisibility(visibility) => &record.visibility == visibility,
                MemoryListScope::ForActor(actor) => {
                    record.tenant_id == actor.tenant_id
                        && harness_memory::visibility_matches(&record.visibility, actor)
                }
            })
            .map(|record| MemorySummary {
                id: record.id,
                kind: record.kind.clone(),
                visibility: record.visibility.clone(),
                content_preview: harness_memory::content_preview(&record.content),
                metadata: record.metadata.clone(),
                updated_at: record.updated_at,
            })
            .collect())
    }
}

impl MemoryLifecycle for SharedMemory {}

fn memory_record_visible(record: &MemoryRecord, filter: &MemoryVisibilityFilter) -> bool {
    match filter {
        MemoryVisibilityFilter::Exact(visibility) => &record.visibility == visibility,
        MemoryVisibilityFilter::EffectiveFor(actor) => {
            record.tenant_id == actor.tenant_id
                && harness_memory::visibility_matches(&record.visibility, actor)
        }
    }
}

fn hash_content(bytes: &[u8]) -> ContentHash {
    ContentHash(*blake3::hash(bytes).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_contracts::NoopRedactor;
    use harness_journal::InMemoryEventStore;

    #[tokio::test]
    async fn shared_memory_internal_write_enforces_policy_and_journals() {
        let store: Arc<InMemoryEventStore> =
            Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let team_id = TeamId::new();
        let coordinator = AgentId::new();
        let worker = AgentId::new();
        let session_id = SessionId::new();
        let correlation_id = CorrelationId::new();
        let memory = SharedMemory::new(team_id, "team-shared")
            .with_policy(SharedWritePolicy::CoordinatorOnly { coordinator })
            .with_journal(
                TeamJournalContext {
                    tenant_id: TenantId::SINGLE,
                    session_id,
                },
                store.clone(),
            );

        assert!(matches!(
            memory
                .write_for_agent(worker, session_id, "hidden", correlation_id)
                .await
                .unwrap_err(),
            TeamError::SharedMemoryWriteDenied
        ));
        memory
            .write_for_agent(coordinator, session_id, "shared fact", correlation_id)
            .await
            .unwrap();

        let recalled = memory
            .recall(MemoryQuery {
                text: "shared".to_owned(),
                kind_filter: None,
                visibility_filter: MemoryVisibilityFilter::EffectiveFor(
                    harness_contracts::MemoryActor {
                        tenant_id: TenantId::SINGLE,
                        user_id: None,
                        team_id: Some(team_id),
                        session_id: Some(session_id),
                    },
                ),
                max_records: 8,
                min_similarity: 0.0,
                tenant_id: TenantId::SINGLE,
                session_id: Some(session_id),
                deadline: None,
            })
            .await
            .unwrap();
        assert_eq!(recalled.len(), 1);
        assert_eq!(recalled[0].content, "shared fact");

        let envelopes: Vec<_> = store
            .read_envelopes(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .unwrap()
            .collect()
            .await;
        assert!(envelopes.iter().any(|envelope| {
            envelope.correlation_id == correlation_id
                && matches!(envelope.payload, Event::MemoryUpserted(_))
        }));
    }

    #[tokio::test]
    async fn shared_memory_internal_write_enforces_role_and_quota() {
        let store: Arc<InMemoryEventStore> =
            Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
        let team_id = TeamId::new();
        let reviewer = AgentId::new();
        let coder = AgentId::new();
        let session_id = SessionId::new();
        let memory = SharedMemory::new(team_id, "team-shared")
            .with_policy(SharedWritePolicy::RoleGated {
                allowed_roles: vec!["reviewer".to_owned()],
            })
            .with_role(reviewer, "reviewer")
            .with_role(coder, "coder")
            .with_journal(
                TeamJournalContext {
                    tenant_id: TenantId::SINGLE,
                    session_id,
                },
                store.clone(),
            );

        assert!(matches!(
            memory
                .write_for_agent(coder, session_id, "coder fact", CorrelationId::new())
                .await
                .unwrap_err(),
            TeamError::SharedMemoryWriteDenied
        ));
        memory
            .write_for_agent(reviewer, session_id, "reviewer fact", CorrelationId::new())
            .await
            .unwrap();

        let quota_memory = SharedMemory::new(team_id, "team-shared")
            .with_policy(SharedWritePolicy::PerMemberQuota { max_entries: 1 })
            .with_journal(
                TeamJournalContext {
                    tenant_id: TenantId::SINGLE,
                    session_id,
                },
                store,
            );
        quota_memory
            .write_for_agent(coder, session_id, "first fact", CorrelationId::new())
            .await
            .unwrap();
        assert!(matches!(
            quota_memory
                .write_for_agent(coder, session_id, "second fact", CorrelationId::new())
                .await
                .unwrap_err(),
            TeamError::SharedMemoryWriteDenied
        ));
    }
}

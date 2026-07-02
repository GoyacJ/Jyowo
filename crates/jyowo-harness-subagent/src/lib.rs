//! `jyowo-harness-subagent`
//!
//! Agent tool delegation, blocklists, announcements, and concurrent pools.
//!
//! SPEC: docs/architecture/harness/crates/harness-subagent.md

#![forbid(unsafe_code)]

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use futures::{future::BoxFuture, stream, StreamExt};
use harness_contracts::{
    ActionPlanHash, AgentId, AgentRef, BudgetKind, CacheImpact, CapabilityRegistry, CorrelationId,
    DecidedBy, Decision, DecisionId, Event, ForkReason, JournalOffset, KillScope, Message,
    MessageContent, MessageId, MessageMetadata, MessagePart, MessageRole, NoopRedactor,
    PermissionActorSource, PermissionMode, PermissionRequestedEvent, PermissionResolvedEvent,
    PermissionReview, RunId, SandboxPolicy, SandboxPolicySummary, SessionForkedEvent, SessionId,
    SessionSnapshotKind, SnapshotId, SubagentAnnouncedEvent, SubagentCapAnnouncement,
    SubagentContextReport, SubagentId, SubagentParentContext, SubagentPermissionForwardedEvent,
    SubagentPermissionResolvedEvent, SubagentRunnerCap, SubagentSpawnHandle,
    SubagentSpawnPausedEvent, SubagentSpawnedEvent, SubagentStalledEvent, SubagentTerminatedEvent,
    SubagentTerminationReason, TeamId, TenantId, ToolCapability, ToolDescriptor, ToolError,
    ToolGroup, ToolOrigin, ToolProperties, ToolResult, ToolUseId, TranscriptRef, TurnInput,
    UsageSnapshot, UserMessageAppendedEvent,
};
use harness_journal::{AppendMetadata, EventStore, ReplayCursor};
use harness_model::{AuxExecutor, AuxModelProvider, AuxTask, ModelProtocol, ModelRequest};
use harness_permission::{
    canonical_permission_fingerprint, hard_policy_denies_from_context, PermissionBroker,
    PermissionCheck, PermissionContext, PermissionRequest,
};
use harness_session::{Session, SessionOptions};
use harness_tool::{Tool, ToolContext, ToolEvent, ToolStream, ValidationError};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};
use tokio::time;

pub use harness_budget::ResourceQuota;
pub use harness_contracts::SubagentStatus;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct PromptTemplate {
    pub body: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolsetSelector {
    InheritAll,
    InheritWithBlocklist(HashSet<String>),
    Preset(String),
    Custom(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxInheritance {
    Inherit,
    Empty,
    Require(RequiredSandboxCapabilities),
    Override(SandboxPolicy),
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RequiredSandboxCapabilities {
    pub backend_id: Option<String>,
    pub supports_streaming: bool,
    pub supports_stdin: bool,
    pub supports_cwd_tracking: bool,
    pub supports_activity_heartbeat: bool,
    pub supports_interactive_shell: bool,
    pub supports_network: bool,
    pub supports_filesystem_write: bool,
    pub supports_gpu: bool,
    pub supports_pty: bool,
    pub supports_detach: bool,
    pub supports_workspace_sync: bool,
    pub supports_session_snapshot: bool,
    pub min_concurrent_execs: Option<u32>,
    pub kill_scopes: Vec<KillScope>,
    pub snapshot_kinds: BTreeSet<SessionSnapshotKind>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentContextMode {
    Isolated,
    ForkFromParent { include_tool_results: bool },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnounceMode {
    #[serde(alias = "structured")]
    StructuredOnly,
    #[serde(alias = "summary")]
    SummaryText,
    FullTranscript,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractivityLevel {
    DeferredInteractive,
    FullyInteractive,
    NoInteractive,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentMemoryScope {
    Inherit,
    Empty,
    Subset { selectors: Vec<MemorySelector> },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySelector {
    Tag(String),
    Provider(String),
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentInputStrategy {
    LatestUserOnly,
    InheritAll,
    Custom { selector_id: String },
}

#[derive(Debug, Clone, Copy)]
pub struct SubagentInputSelection<'a> {
    pub parent: &'a ParentContext,
    pub child_session_id: SessionId,
    pub include_tool_results: bool,
    pub parent_transcript: &'a [Message],
}

pub trait SubagentInputSelector: Send + Sync + 'static {
    fn selector_id(&self) -> &str;

    fn select(&self, selection: SubagentInputSelection<'_>) -> Result<Vec<Message>, SubagentError>;
}

#[derive(Debug, Clone, Copy)]
pub struct SubagentMemoryScopeRequest<'a> {
    pub parent: &'a ParentContext,
    pub child_session_id: SessionId,
    pub selectors: &'a [MemorySelector],
}

pub trait SubagentMemoryScopeResolver: Send + Sync + 'static {
    fn resolve(
        &self,
        request: SubagentMemoryScopeRequest<'_>,
    ) -> Result<Vec<Message>, SubagentError>;
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct McpServerRef {
    server_id: String,
}

impl McpServerRef {
    #[must_use]
    pub fn new(server_id: impl Into<String>) -> Self {
        Self {
            server_id: server_id.into(),
        }
    }

    #[must_use]
    pub fn server_id(&self) -> &str {
        &self.server_id
    }
}

impl From<String> for McpServerRef {
    fn from(server_id: String) -> Self {
        Self::new(server_id)
    }
}

impl From<&str> for McpServerRef {
    fn from(server_id: &str) -> Self {
        Self::new(server_id)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct SubagentMcpServerPattern {
    pub pattern: String,
    pub require_ready: bool,
    pub allow_inline: bool,
}

impl SubagentMcpServerPattern {
    #[must_use]
    pub fn exact(server_id: impl Into<String>) -> Self {
        Self {
            pattern: server_id.into(),
            require_ready: true,
            allow_inline: true,
        }
    }
}

impl<'de> Deserialize<'de> for SubagentMcpServerPattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Compat {
            Legacy(String),
            Pattern(PatternFields),
        }

        #[derive(Deserialize)]
        struct PatternFields {
            pattern: String,
            #[serde(default = "default_true")]
            require_ready: bool,
            #[serde(default = "default_true")]
            allow_inline: bool,
        }

        match Compat::deserialize(deserializer)? {
            Compat::Legacy(server_id) => Ok(Self::exact(server_id)),
            Compat::Pattern(fields) => Ok(Self {
                pattern: fields.pattern,
                require_ready: fields.require_ready,
                allow_inline: fields.allow_inline,
            }),
        }
    }
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubagentSpec {
    pub role: String,
    pub task: String,
    pub prompt_template: PromptTemplate,
    pub toolset: ToolsetSelector,
    pub tool_blocklist: HashSet<String>,
    pub sandbox_policy: SandboxInheritance,
    pub context_mode: SubagentContextMode,
    pub permission_mode: harness_contracts::PermissionMode,
    pub max_turns: u32,
    pub max_depth: u8,
    pub announce_mode: AnnounceMode,
    pub mcp_servers: Vec<McpServerRef>,
    pub required_mcp_servers: Vec<McpServerRef>,
    pub interactivity: InteractivityLevel,
    pub quota: Option<ResourceQuota>,
    pub memory_scope: SubagentMemoryScope,
    pub input_strategy: SubagentInputStrategy,
    pub system_header_extra: Option<String>,
    pub bootstrap_filter: BootstrapFilter,
}

impl SubagentSpec {
    #[must_use]
    pub fn minimal(role: impl Into<String>, task: impl Into<String>) -> Self {
        let task = task.into();
        Self {
            role: role.into(),
            task: task.clone(),
            prompt_template: PromptTemplate { body: task },
            toolset: ToolsetSelector::InheritWithBlocklist(DelegationBlocklist::default().tools),
            tool_blocklist: DelegationBlocklist::default().tools,
            sandbox_policy: SandboxInheritance::Inherit,
            context_mode: SubagentContextMode::Isolated,
            permission_mode: harness_contracts::PermissionMode::Default,
            max_turns: 8,
            max_depth: 1,
            announce_mode: AnnounceMode::StructuredOnly,
            mcp_servers: Vec::new(),
            required_mcp_servers: Vec::new(),
            interactivity: InteractivityLevel::DeferredInteractive,
            quota: None,
            memory_scope: SubagentMemoryScope::Inherit,
            input_strategy: SubagentInputStrategy::LatestUserOnly,
            system_header_extra: None,
            bootstrap_filter: BootstrapFilter::ExcludeAll,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapFilter {
    ExcludeAll,
    Allow(Vec<String>),
    InheritAll,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DelegationBlocklist {
    tools: HashSet<String>,
}

impl DelegationBlocklist {
    #[must_use]
    pub fn contains(&self, tool: &str) -> bool {
        self.tools.contains(tool)
    }

    #[must_use]
    pub fn tools(&self) -> &HashSet<String> {
        &self.tools
    }
}

impl Default for DelegationBlocklist {
    fn default() -> Self {
        Self {
            tools: [
                "delegate",
                "agent",
                "clarify",
                "memory_write",
                "send_user_message",
                "execute_code",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DelegationPolicy {
    pub max_depth: u8,
    pub depth_cap: u8,
    pub max_concurrent_children: usize,
    pub max_global_children: usize,
    pub blocklist: DelegationBlocklist,
}

impl DelegationPolicy {
    #[must_use]
    pub fn filter_tool_names<'a, I>(&self, spec: &SubagentSpec, tools: I) -> Vec<String>
    where
        I: IntoIterator<Item = &'a str>,
    {
        tools
            .into_iter()
            .filter(|tool| self.allows_tool_name(spec, tool))
            .map(str::to_owned)
            .collect()
    }

    #[must_use]
    pub fn filter_tool_descriptors<'a, I>(
        &self,
        spec: &SubagentSpec,
        tools: I,
    ) -> Vec<&'a ToolDescriptor>
    where
        I: IntoIterator<Item = &'a ToolDescriptor>,
    {
        tools
            .into_iter()
            .filter(|tool| {
                self.allows_tool_name(spec, tool.name.as_str())
                    && self.allows_tool_origin(spec, tool)
            })
            .collect()
    }

    fn allows_tool_name(&self, spec: &SubagentSpec, tool: &str) -> bool {
        !self.blocklist.contains(tool)
            && !spec.tool_blocklist.contains(tool)
            && !matches!(
                &spec.toolset,
                ToolsetSelector::InheritWithBlocklist(blocklist) if blocklist.contains(tool)
            )
    }

    fn allows_tool_origin(&self, spec: &SubagentSpec, tool: &ToolDescriptor) -> bool {
        match &tool.origin {
            ToolOrigin::Mcp(origin) => spec
                .mcp_servers
                .iter()
                .chain(spec.required_mcp_servers.iter())
                .any(|server| server.server_id() == origin.server_id.0.as_str()),
            _ => !tool.name.starts_with("mcp__"),
        }
    }
}

impl Default for DelegationPolicy {
    fn default() -> Self {
        Self {
            max_depth: 1,
            depth_cap: 3,
            max_concurrent_children: 3,
            max_global_children: 128,
            blocklist: DelegationBlocklist::default(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParentContext {
    pub tenant_id: TenantId,
    pub parent_session_id: SessionId,
    pub parent_run_id: RunId,
    pub depth: u8,
    pub sibling_count: u32,
    pub trigger_tool_use_id: Option<ToolUseId>,
    pub correlation_id: CorrelationId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<TeamId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_member_profile_id: Option<String>,
}

impl ParentContext {
    #[must_use]
    pub fn for_test(depth: u8) -> Self {
        Self {
            tenant_id: TenantId::SINGLE,
            parent_session_id: SessionId::new(),
            parent_run_id: RunId::new(),
            depth,
            sibling_count: 0,
            trigger_tool_use_id: None,
            correlation_id: CorrelationId::new(),
            team_id: None,
            team_member_profile_id: None,
        }
    }
}

impl From<ParentContext> for SubagentParentContext {
    fn from(value: ParentContext) -> Self {
        Self {
            tenant_id: value.tenant_id,
            parent_session_id: value.parent_session_id,
            parent_run_id: value.parent_run_id,
            depth: value.depth,
            sibling_count: value.sibling_count,
            trigger_tool_use_id: value.trigger_tool_use_id,
            correlation_id: value.correlation_id,
        }
    }
}

impl From<SubagentParentContext> for ParentContext {
    fn from(value: SubagentParentContext) -> Self {
        Self {
            tenant_id: value.tenant_id,
            parent_session_id: value.parent_session_id,
            parent_run_id: value.parent_run_id,
            depth: value.depth,
            sibling_count: value.sibling_count,
            trigger_tool_use_id: value.trigger_tool_use_id,
            correlation_id: value.correlation_id,
            team_id: None,
            team_member_profile_id: None,
        }
    }
}

pub struct SubagentPermissionBridge {
    parent_broker: Arc<dyn PermissionBroker>,
    event_store: Arc<dyn EventStore>,
    tenant_id: TenantId,
    parent_session_id: SessionId,
    parent_run_id: RunId,
    subagent_id: SubagentId,
    child_context: Option<ChildPermissionContext>,
    team_id: Option<TeamId>,
    team_member_profile_id: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct ChildPermissionContext {
    session: SessionId,
    run: RunId,
    correlation: CorrelationId,
}

impl SubagentPermissionBridge {
    #[must_use]
    pub fn new(
        parent_broker: Arc<dyn PermissionBroker>,
        event_store: Arc<dyn EventStore>,
        tenant_id: TenantId,
        parent_session_id: SessionId,
        parent_run_id: RunId,
        subagent_id: SubagentId,
    ) -> Self {
        Self {
            parent_broker,
            event_store,
            tenant_id,
            parent_session_id,
            parent_run_id,
            subagent_id,
            child_context: None,
            team_id: None,
            team_member_profile_id: None,
        }
    }

    #[must_use]
    pub fn with_team_attribution(
        mut self,
        team_id: TeamId,
        team_member_profile_id: impl Into<String>,
    ) -> Self {
        self.team_id = Some(team_id);
        self.team_member_profile_id = Some(team_member_profile_id.into());
        self
    }

    #[must_use]
    pub fn with_child_context(
        mut self,
        child_session_id: SessionId,
        child_run_id: RunId,
        correlation_id: CorrelationId,
    ) -> Self {
        self.child_context = Some(ChildPermissionContext {
            session: child_session_id,
            run: child_run_id,
            correlation: correlation_id,
        });
        self
    }
}

#[async_trait]
impl PermissionBroker for SubagentPermissionBridge {
    async fn decide(&self, request: PermissionRequest, ctx: PermissionContext) -> Decision {
        let child_context = self.child_context.unwrap_or(ChildPermissionContext {
            session: request.session_id,
            run: self.parent_run_id,
            correlation: CorrelationId::new(),
        });
        let causation_id = harness_contracts::EventId::new();
        let parent_decided_by = DecidedBy::Broker {
            broker_id: "parent".to_owned(),
        };
        let auto_resolved = matches!(
            ctx.permission_mode,
            PermissionMode::BypassPermissions | PermissionMode::DontAsk
        );
        if self
            .event_store
            .append_with_metadata(
                self.tenant_id,
                child_context.session,
                AppendMetadata {
                    run_id: Some(child_context.run),
                    correlation_id: child_context.correlation,
                    ..AppendMetadata::default()
                },
                &[Event::PermissionRequested(PermissionRequestedEvent {
                    request_id: request.request_id,
                    run_id: child_context.run,
                    session_id: child_context.session,
                    tenant_id: request.tenant_id,
                    tool_use_id: request.tool_use_id,
                    tool_name: request.tool_name.clone(),
                    subject: request.subject.clone(),
                    severity: request.severity,
                    scope_hint: request.scope_hint.clone(),
                    fingerprint: None,
                    presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
                    interactivity: ctx.interactivity,
                    auto_resolved,
                    action_plan_hash: legacy_action_plan_hash(&request),
                    review: permission_review_from_request(&request),
                    effective_mode: ctx.permission_mode,
                    sandbox_policy: legacy_sandbox_policy_summary(),
                    actor_source: PermissionActorSource::Subagent {
                        subagent_id: self.subagent_id,
                        parent_session_id: self.parent_session_id,
                        parent_run_id: self.parent_run_id,
                        team_id: self.team_id,
                        team_member_profile_id: self.team_member_profile_id.clone(),
                    },
                    causation_id,
                    at: Utc::now(),
                })],
            )
            .await
            .is_err()
        {
            return Decision::DenyOnce;
        }
        if self
            .event_store
            .append_with_metadata(
                self.tenant_id,
                self.parent_session_id,
                AppendMetadata {
                    run_id: Some(self.parent_run_id),
                    correlation_id: child_context.correlation,
                    ..AppendMetadata::default()
                },
                &[Event::SubagentPermissionForwarded(
                    SubagentPermissionForwardedEvent {
                        parent_session_id: self.parent_session_id,
                        subagent_id: self.subagent_id,
                        original_request_id: request.request_id,
                        subject: request.subject.clone(),
                        presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
                        timeout_policy: ctx.timeout_policy.clone(),
                        team_id: self.team_id,
                        team_member_profile_id: self.team_member_profile_id.clone(),
                        forwarded_at: Utc::now(),
                    },
                )],
            )
            .await
            .is_err()
        {
            return Decision::DenyOnce;
        }

        let decision = if self.hard_policy_denies(&request, &ctx).await {
            Decision::DenyOnce
        } else {
            self.parent_broker.decide(request.clone(), ctx).await
        };
        let forwarded_decided_by = DecidedBy::ParentForwarded {
            parent_session_id: self.parent_session_id,
            original_decided_by: Box::new(parent_decided_by),
        };
        let decision_id = DecisionId::new();
        if self
            .event_store
            .append_with_metadata(
                self.tenant_id,
                child_context.session,
                AppendMetadata {
                    run_id: Some(child_context.run),
                    correlation_id: child_context.correlation,
                    ..AppendMetadata::default()
                },
                &[Event::PermissionResolved(PermissionResolvedEvent {
                    request_id: request.request_id,
                    action_plan_hash: legacy_action_plan_hash(&request),
                    decision_id,
                    decision: decision.clone(),
                    decided_by: forwarded_decided_by.clone(),
                    scope: request.scope_hint.clone(),
                    fingerprint: None,
                    auto_resolved,
                    rationale: None,
                    at: Utc::now(),
                })],
            )
            .await
            .is_err()
        {
            return Decision::DenyOnce;
        }
        if self
            .event_store
            .append_with_metadata(
                self.tenant_id,
                self.parent_session_id,
                AppendMetadata {
                    run_id: Some(self.parent_run_id),
                    correlation_id: child_context.correlation,
                    ..AppendMetadata::default()
                },
                &[Event::SubagentPermissionResolved(
                    SubagentPermissionResolvedEvent {
                        parent_session_id: self.parent_session_id,
                        subagent_id: self.subagent_id,
                        original_request_id: request.request_id,
                        decision: decision.clone(),
                        decided_by: forwarded_decided_by,
                        team_id: self.team_id,
                        team_member_profile_id: self.team_member_profile_id.clone(),
                        at: Utc::now(),
                    },
                )],
            )
            .await
            .is_err()
        {
            return Decision::DenyOnce;
        }
        decision
    }

    async fn hard_policy_denies(
        &self,
        request: &PermissionRequest,
        ctx: &PermissionContext,
    ) -> bool {
        self.parent_broker.hard_policy_denies(request, ctx).await
            || hard_policy_denies_from_context(request, ctx)
    }

    async fn persist(
        &self,
        decision: harness_permission::PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        self.parent_broker.persist(decision).await
    }
}

fn legacy_action_plan_hash(request: &PermissionRequest) -> ActionPlanHash {
    ActionPlanHash::from_bytes(canonical_permission_fingerprint(request).0)
}

fn permission_review_from_request(request: &PermissionRequest) -> PermissionReview {
    PermissionReview {
        summary: format!(
            "{} requests {}",
            request.tool_name,
            permission_subject_kind(&request.subject)
        ),
        details: vec![harness_contracts::PermissionReviewDetail {
            label: "subject".to_owned(),
            value: permission_subject_kind(&request.subject).to_owned(),
            redacted: true,
        }],
        confirmation: harness_contracts::PermissionConfirmation::None,
        redacted: true,
    }
}

fn permission_subject_kind(subject: &harness_contracts::PermissionSubject) -> &'static str {
    match subject {
        harness_contracts::PermissionSubject::ToolInvocation { .. } => "tool invocation access",
        harness_contracts::PermissionSubject::CommandExec { .. } => "command execution access",
        harness_contracts::PermissionSubject::FileWrite { .. } => "file write access",
        harness_contracts::PermissionSubject::FileDelete { .. } => "file delete access",
        harness_contracts::PermissionSubject::NetworkAccess { .. } => "network access",
        harness_contracts::PermissionSubject::DangerousCommand { .. } => "dangerous command access",
        harness_contracts::PermissionSubject::McpToolCall { .. } => "MCP tool access",
        harness_contracts::PermissionSubject::Custom { .. } => "custom permission access",
        _ => "runtime access",
    }
}

fn legacy_sandbox_policy_summary() -> SandboxPolicySummary {
    SandboxPolicySummary {
        mode: harness_contracts::SandboxMode::None,
        scope: harness_contracts::SandboxScope::WorkspaceOnly,
        network: harness_contracts::NetworkAccess::None,
        resource_limits: harness_contracts::ResourceLimits {
            max_memory_bytes: None,
            max_cpu_cores: None,
            max_pids: None,
            max_wall_clock_ms: None,
            max_open_files: None,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubagentAnnouncement {
    pub subagent_id: SubagentId,
    pub parent_session_id: SessionId,
    pub status: SubagentStatus,
    pub summary: String,
    pub result: Option<Value>,
    pub usage: UsageSnapshot,
    pub transcript_ref: Option<TranscriptRef>,
    pub context_report: Option<SubagentContextReport>,
}

#[derive(Debug)]
pub struct SubagentHandle {
    pub subagent_id: SubagentId,
    announcement: SubagentAnnouncement,
    events: Vec<SubagentHandleEvent>,
    cancellation: Option<SubagentCancellationToken>,
}

impl SubagentHandle {
    #[must_use]
    pub fn ready(announcement: SubagentAnnouncement) -> Self {
        let events = vec![SubagentHandleEvent::Announced(announcement.clone())];
        Self {
            subagent_id: announcement.subagent_id,
            announcement,
            events,
            cancellation: None,
        }
    }

    #[must_use]
    pub fn with_cancellation(mut self, cancellation: SubagentCancellationToken) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    pub fn cancel(&self) -> Result<(), SubagentError> {
        let Some(cancellation) = &self.cancellation else {
            return Err(SubagentError::Engine(
                "subagent handle has no cancellation token".to_owned(),
            ));
        };
        cancellation.cancel();
        Ok(())
    }

    #[must_use]
    pub fn events(&self) -> &[SubagentHandleEvent] {
        &self.events
    }

    pub fn event_stream(&self) -> futures::stream::BoxStream<'static, SubagentHandleEvent> {
        Box::pin(stream::iter(self.events.clone()))
    }

    pub async fn wait(self) -> Result<SubagentAnnouncement, SubagentError> {
        Ok(self.announcement)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SubagentHandleEvent {
    Announced(SubagentAnnouncement),
    Cancelled { subagent_id: SubagentId },
}

pub struct SubagentAnnouncementRenderer {
    renderer_id: String,
}

impl Default for SubagentAnnouncementRenderer {
    fn default() -> Self {
        Self {
            renderer_id: "xml-task-notification".to_owned(),
        }
    }
}

pub type RenderedAnnouncement = harness_contracts::RenderedAnnouncement;

pub trait AnnouncementRenderer: Send + Sync + 'static {
    fn render(&self, announcement: &SubagentAnnouncement) -> RenderedAnnouncement;
}

#[async_trait]
pub trait AnnouncementSummarizer: Send + Sync + 'static {
    async fn summarize(&self, announcement: &SubagentAnnouncement) -> Option<String>;
}

#[derive(Clone)]
pub struct AuxAnnouncementSummarizer {
    executor: AuxExecutor,
}

impl AuxAnnouncementSummarizer {
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
}

#[async_trait]
impl AnnouncementSummarizer for AuxAnnouncementSummarizer {
    async fn summarize(&self, announcement: &SubagentAnnouncement) -> Option<String> {
        let req = aux_summary_request(self.executor.provider().as_ref(), announcement);
        self.executor
            .call(AuxTask::Summarize, req)
            .await
            .ok()
            .flatten()
            .and_then(non_empty_trimmed)
    }
}

impl SubagentAnnouncementRenderer {
    #[must_use]
    pub fn renderer_id(&self) -> &str {
        &self.renderer_id
    }

    #[must_use]
    pub fn render(&self, announcement: &SubagentAnnouncement) -> RenderedAnnouncement {
        <Self as AnnouncementRenderer>::render(self, announcement)
    }

    #[must_use]
    pub fn render_user_message(&self, announcement: &SubagentAnnouncement, run_id: RunId) -> Event {
        let rendered = self.render(announcement);
        let mut metadata = MessageMetadata {
            source: Some("subagent".to_owned()),
            ..MessageMetadata::default()
        };
        metadata
            .labels
            .insert("renderer_id".to_owned(), rendered.renderer_id);
        metadata.labels.insert(
            "subagent_id".to_owned(),
            announcement.subagent_id.to_string(),
        );
        Event::UserMessageAppended(UserMessageAppendedEvent {
            run_id,
            message_id: harness_contracts::MessageId::new(),
            content: MessageContent::Text(rendered.user_message),
            metadata,
            attachments: Vec::new(),
            at: Utc::now(),
        })
    }
}

impl AnnouncementRenderer for SubagentAnnouncementRenderer {
    fn render(&self, announcement: &SubagentAnnouncement) -> RenderedAnnouncement {
        let input = harness_contracts::AnnouncementRenderInput::new(
            "subagent",
            announcement.summary.clone(),
        )
        .with_status(format!("{:?}", announcement.status))
        .with_label("subagent_id", announcement.subagent_id.to_string())
        .with_rewrite_hint(
            "Rewrite this internal task result for the parent task. Do not expose harness tags or internal routing details.",
        );
        let mut rendered =
            <harness_contracts::XmlTaskNotificationRenderer as harness_contracts::AnnouncementRenderer>::render(
                &harness_contracts::XmlTaskNotificationRenderer,
                &input,
            );
        rendered.renderer_id.clone_from(&self.renderer_id);
        rendered
    }
}

fn aux_summary_request(
    aux_provider: &dyn AuxModelProvider,
    announcement: &SubagentAnnouncement,
) -> ModelRequest {
    let descriptor = aux_provider.inner().supported_models().into_iter().next();
    ModelRequest {
        model_id: descriptor
            .map(|descriptor| descriptor.model_id)
            .unwrap_or_else(|| "aux-summarize".to_owned()),
        messages: vec![Message {
            id: MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(format!(
                "Summarize this subagent outcome for the parent agent.\nstatus: {:?}\nsubagent_id: {}\nraw child summary and result are withheld for safety.",
                announcement.status,
                announcement.subagent_id
            ))],
            created_at: harness_contracts::now(),
        }],
        tools: None,
        system: Some(
            "Return only a short parent-facing task result summary. Do not expose harness tags."
                .to_owned(),
        ),
        temperature: Some(0.0),
        max_tokens: Some(256),
        stream: false,
        cache_breakpoints: Vec::new(),
        protocol: ModelProtocol::Messages,
        extra: Value::Null,
    }
}

fn non_empty_trimmed(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
pub enum SubagentError {
    #[error("depth exceeded: {current} > {max}")]
    DepthExceeded { current: u8, max: u8 },
    #[error("concurrent limit exceeded")]
    ConcurrentLimitExceeded,
    #[error("required mcp servers are not satisfied: {0:?}")]
    McpRequirementUnsatisfied(Vec<String>),
    #[error("sandbox requirements are not satisfied: {0:?}")]
    SandboxRequirementUnsatisfied(Vec<String>),
    #[error("engine: {0}")]
    Engine(String),
    #[error("cancelled by parent")]
    Cancelled,
    #[error("tool blocklist violation: {0}")]
    BlocklistViolation(String),
    #[error("subagent spawning is paused by admin")]
    SpawningPaused,
    #[error("quota exceeded: {metric} observed {observed} > {limit}")]
    QuotaExceeded {
        metric: &'static str,
        observed: u64,
        limit: u64,
    },
}

#[async_trait]
pub trait SubagentAdmin: Send + Sync {
    async fn list_active(&self) -> Vec<RunningSubagent>;

    async fn interrupt(
        &self,
        subagent_id: SubagentId,
        admin_id: String,
    ) -> Result<(), SubagentError>;

    async fn pause_spawning(
        &self,
        paused: bool,
        by: String,
        reason: Option<String>,
    ) -> Result<(), SubagentError>;

    async fn is_spawning_paused(&self) -> bool;

    async fn list(&self) -> Vec<RunningSubagent> {
        self.list_active().await
    }

    async fn cancel(&self, subagent_id: SubagentId) -> Result<(), SubagentError> {
        self.interrupt(subagent_id, "cancel".to_owned()).await
    }

    async fn status(&self, subagent_id: SubagentId) -> Option<RunningSubagent> {
        self.list_active()
            .await
            .into_iter()
            .find(|running| running.subagent_id == subagent_id)
    }
}

#[async_trait]
pub trait SubagentRunner: Send + Sync + 'static {
    async fn spawn(
        &self,
        spec: SubagentSpec,
        input: TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, SubagentError>;
}

#[derive(Debug, Clone)]
pub struct ChildRunRequest {
    pub tenant_id: TenantId,
    pub child_session_id: SessionId,
    pub child_run_id: RunId,
    pub parent_session_id: SessionId,
    pub parent_run_id: RunId,
    pub subagent_id: SubagentId,
    pub spec: SubagentSpec,
    pub child_depth: u8,
    pub correlation_id: CorrelationId,
    pub context_seed: Vec<Message>,
    pub memory_scope_resolved: bool,
    pub input: TurnInput,
    pub cancellation: SubagentCancellationToken,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChildRunOutcome {
    pub status: SubagentStatus,
    pub summary: String,
    pub result: Option<Value>,
    pub usage: UsageSnapshot,
    pub transcript_ref: Option<TranscriptRef>,
    pub context_report: Option<SubagentContextReport>,
}

#[async_trait]
pub trait ChildSessionRunner: Send + Sync + 'static {
    async fn run_child(&self, request: ChildRunRequest) -> Result<ChildRunOutcome, SubagentError>;
}

#[async_trait]
pub trait SubagentEngineFactory: Send + Sync + 'static {
    async fn run_child_engine(
        &self,
        request: ChildRunRequest,
    ) -> Result<ChildRunOutcome, SubagentError>;
}

struct ChildSessionRunnerFactory {
    runner: Arc<dyn ChildSessionRunner>,
}

#[async_trait]
impl SubagentEngineFactory for ChildSessionRunnerFactory {
    async fn run_child_engine(
        &self,
        request: ChildRunRequest,
    ) -> Result<ChildRunOutcome, SubagentError> {
        self.runner.run_child(request).await
    }
}

#[derive(Debug, Default)]
struct SubagentCancellationState {
    cancelled: AtomicBool,
    notify: Notify,
}

#[derive(Clone, Default)]
pub struct SubagentCancellationToken {
    state: Arc<SubagentCancellationState>,
}

impl std::fmt::Debug for SubagentCancellationToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SubagentCancellationToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl SubagentCancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        if !self.state.cancelled.swap(true, Ordering::SeqCst) {
            self.state.notify.notify_waiters();
        }
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        while !self.is_cancelled() {
            self.state.notify.notified().await;
        }
    }
}

pub struct DefaultSubagentRunner {
    engine_factory: Arc<dyn SubagentEngineFactory>,
    event_store: Arc<dyn EventStore>,
    policy: DelegationPolicy,
    input_selectors: HashMap<String, Arc<dyn SubagentInputSelector>>,
    memory_scope_resolver: Option<Arc<dyn SubagentMemoryScopeResolver>>,
    pool: ConcurrentSubagentPool,
    workspace_root: PathBuf,
    watchdog_interval: Option<Duration>,
    watchdog_started: AtomicBool,
    spawning_paused: AtomicBool,
    admin_session_id: SessionId,
    announcement_renderer: Arc<dyn AnnouncementRenderer>,
    announcement_summarizer: Option<Arc<dyn AnnouncementSummarizer>>,
}

impl DefaultSubagentRunner {
    #[must_use]
    pub fn new(
        child_runner: Arc<dyn ChildSessionRunner>,
        event_store: Arc<dyn EventStore>,
        workspace_root: impl Into<PathBuf>,
        policy: DelegationPolicy,
    ) -> Self {
        Self::new_with_engine_factory(
            Arc::new(ChildSessionRunnerFactory {
                runner: child_runner,
            }),
            event_store,
            workspace_root,
            policy,
        )
    }

    #[must_use]
    pub fn new_with_engine_factory(
        engine_factory: Arc<dyn SubagentEngineFactory>,
        event_store: Arc<dyn EventStore>,
        workspace_root: impl Into<PathBuf>,
        policy: DelegationPolicy,
    ) -> Self {
        let pool = ConcurrentSubagentPool::new(policy.max_concurrent_children);
        Self {
            engine_factory,
            event_store,
            policy,
            input_selectors: HashMap::new(),
            memory_scope_resolver: None,
            pool,
            workspace_root: workspace_root.into(),
            watchdog_interval: None,
            watchdog_started: AtomicBool::new(false),
            spawning_paused: AtomicBool::new(false),
            admin_session_id: SessionId::new(),
            announcement_renderer: Arc::new(SubagentAnnouncementRenderer::default()),
            announcement_summarizer: None,
        }
    }

    #[must_use]
    pub fn with_pool(mut self, pool: ConcurrentSubagentPool) -> Self {
        self.pool = pool;
        self
    }

    #[must_use]
    pub fn pool(&self) -> &ConcurrentSubagentPool {
        &self.pool
    }

    #[must_use]
    pub fn with_watchdog_interval(mut self, interval: Duration) -> Self {
        self.watchdog_interval = Some(interval);
        self
    }

    #[must_use]
    pub fn with_announcement_renderer(mut self, renderer: Arc<dyn AnnouncementRenderer>) -> Self {
        self.announcement_renderer = renderer;
        self
    }

    #[must_use]
    pub fn with_announcement_summarizer(
        mut self,
        summarizer: Arc<dyn AnnouncementSummarizer>,
    ) -> Self {
        self.announcement_summarizer = Some(summarizer);
        self
    }

    #[must_use]
    pub fn with_input_selector(mut self, selector: Arc<dyn SubagentInputSelector>) -> Self {
        self.input_selectors
            .insert(selector.selector_id().to_owned(), selector);
        self
    }

    #[must_use]
    pub fn with_memory_scope_resolver(
        mut self,
        resolver: Arc<dyn SubagentMemoryScopeResolver>,
    ) -> Self {
        self.memory_scope_resolver = Some(resolver);
        self
    }

    #[must_use]
    pub fn admin_session_id(&self) -> SessionId {
        self.admin_session_id
    }

    pub fn spawn_watchdog(self: Arc<Self>, interval: Duration) -> Result<(), SubagentError> {
        self.start_watchdog(interval)
    }

    fn ensure_watchdog_started(&self) -> Result<(), SubagentError> {
        let Some(interval) = self.watchdog_interval else {
            return Ok(());
        };
        self.start_watchdog(interval)
    }

    fn start_watchdog(&self, interval: Duration) -> Result<(), SubagentError> {
        if self
            .watchdog_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(());
        }
        let handle = tokio::runtime::Handle::try_current().map_err(|error| {
            self.watchdog_started.store(false, Ordering::SeqCst);
            SubagentError::Engine(format!("subagent watchdog requires tokio runtime: {error}"))
        })?;
        let pool = self.pool.clone();
        let event_store = Arc::clone(&self.event_store);
        handle.spawn(async move {
            let mut ticker = time::interval(interval);
            loop {
                ticker.tick().await;
                let _ = watchdog_tick_for(&pool, event_store.as_ref()).await;
            }
        });
        Ok(())
    }
}

#[async_trait]
impl SubagentRunner for DefaultSubagentRunner {
    async fn spawn(
        &self,
        spec: SubagentSpec,
        input: TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, SubagentError> {
        self.ensure_watchdog_started()?;
        if self.spawning_paused.load(Ordering::SeqCst) {
            return Err(SubagentError::SpawningPaused);
        }
        let effective_max_depth = self.policy.max_depth.min(spec.max_depth);
        let hard_depth_cap = self.policy.depth_cap;
        if parent_ctx.depth >= hard_depth_cap {
            return Err(SubagentError::DepthExceeded {
                current: parent_ctx.depth,
                max: hard_depth_cap,
            });
        }
        if parent_ctx.depth >= effective_max_depth {
            return Err(SubagentError::DepthExceeded {
                current: parent_ctx.depth,
                max: effective_max_depth,
            });
        }

        let _slot = self.pool.acquire(&parent_ctx).await?;
        let child_session_id = SessionId::new();
        let child_run_id = RunId::new();
        let subagent_id = SubagentId::new();
        let cancellation = self
            .pool
            .register_running(subagent_id, &parent_ctx, spec.role.clone());

        let create_result = Session::builder()
            .with_options(
                SessionOptions::new(self.workspace_root.clone())
                    .with_tenant_id(parent_ctx.tenant_id)
                    .with_session_id(child_session_id),
            )
            .with_event_store(Arc::clone(&self.event_store))
            .build()
            .await
            .map_err(|error| SubagentError::Engine(error.to_string()));

        let outcome = match create_result {
            Ok(_) => match self
                .assemble_context_seed(&spec, &parent_ctx, child_session_id)
                .await
            {
                Ok((context_seed, memory_scope_resolved)) => {
                    if let Err(error) = self.append_spawned(&spec, &parent_ctx, subagent_id).await {
                        self.pool.finish(&subagent_id);
                        return Err(error);
                    }
                    let request = ChildRunRequest {
                        tenant_id: parent_ctx.tenant_id,
                        child_session_id,
                        child_run_id,
                        parent_session_id: parent_ctx.parent_session_id,
                        parent_run_id: parent_ctx.parent_run_id,
                        subagent_id,
                        spec: spec.clone(),
                        child_depth: parent_ctx.depth.saturating_add(1),
                        correlation_id: parent_ctx.correlation_id,
                        context_seed,
                        memory_scope_resolved,
                        input,
                        cancellation: cancellation.clone(),
                    };
                    let outcome = run_child_with_quota(
                        Arc::clone(&self.engine_factory),
                        request,
                        spec.quota.as_ref(),
                    )
                    .await;
                    if cancellation.is_cancelled() {
                        Err(SubagentError::Cancelled)
                    } else {
                        outcome
                    }
                }
                Err(error) => {
                    if let Err(spawn_error) =
                        self.append_spawned(&spec, &parent_ctx, subagent_id).await
                    {
                        self.pool.finish(&subagent_id);
                        return Err(spawn_error);
                    }
                    Err(error)
                }
            },
            Err(error) => {
                self.pool.finish(&subagent_id);
                Err(error)
            }
        };

        let running = self.pool.finish(&subagent_id);

        match outcome {
            Ok(outcome) => {
                let final_usage = outcome.usage.clone();
                let transcript_ref = if matches!(spec.announce_mode, AnnounceMode::FullTranscript) {
                    outcome.transcript_ref
                } else {
                    None
                };
                let mut announcement = SubagentAnnouncement {
                    subagent_id,
                    parent_session_id: parent_ctx.parent_session_id,
                    status: outcome.status,
                    summary: outcome.summary,
                    result: outcome.result,
                    usage: outcome.usage,
                    transcript_ref,
                    context_report: outcome.context_report,
                };
                if let Some(summarizer) = &self.announcement_summarizer {
                    if let Some(summary) = summarizer.summarize(&announcement).await {
                        announcement.summary = summary;
                    }
                }
                self.append_announced(
                    parent_ctx.tenant_id,
                    &announcement,
                    parent_ctx.parent_run_id,
                    parent_ctx.correlation_id,
                )
                .await?;
                if running.is_some() {
                    self.append_terminated(
                        parent_ctx.tenant_id,
                        parent_ctx.parent_session_id,
                        parent_ctx.parent_run_id,
                        parent_ctx.correlation_id,
                        subagent_id,
                        SubagentTerminationReason::NaturalCompletion,
                        final_usage,
                    )
                    .await?;
                }
                Ok(SubagentHandle::ready(announcement))
            }
            Err(error) => {
                if running.is_some() {
                    self.append_terminated(
                        parent_ctx.tenant_id,
                        parent_ctx.parent_session_id,
                        parent_ctx.parent_run_id,
                        parent_ctx.correlation_id,
                        subagent_id,
                        termination_reason_for_error(&error),
                        UsageSnapshot::default(),
                    )
                    .await?;
                }
                Err(error)
            }
        }
    }
}

#[async_trait]
impl SubagentAdmin for DefaultSubagentRunner {
    async fn list_active(&self) -> Vec<RunningSubagent> {
        self.pool.list_running()
    }

    async fn interrupt(
        &self,
        subagent_id: SubagentId,
        admin_id: String,
    ) -> Result<(), SubagentError> {
        let Ok(running) = self.pool.cancel(&subagent_id) else {
            return Ok(());
        };
        append_terminated_to(
            self.event_store.as_ref(),
            running.tenant_id,
            running.parent_session_id,
            running.parent_run_id,
            running.correlation_id,
            running.subagent_id,
            SubagentTerminationReason::AdminInterrupted { admin_id },
            UsageSnapshot::default(),
        )
        .await
    }

    async fn pause_spawning(
        &self,
        paused: bool,
        by: String,
        reason: Option<String>,
    ) -> Result<(), SubagentError> {
        self.spawning_paused.store(paused, Ordering::SeqCst);
        self.event_store
            .append_with_metadata(
                TenantId::SINGLE,
                self.admin_session_id,
                AppendMetadata::default(),
                &[Event::SubagentSpawnPaused(SubagentSpawnPausedEvent {
                    tenant_id: TenantId::SINGLE,
                    paused,
                    by,
                    reason,
                    at: Utc::now(),
                })],
            )
            .await
            .map_err(|error| SubagentError::Engine(error.to_string()))?;
        Ok(())
    }

    async fn is_spawning_paused(&self) -> bool {
        self.spawning_paused.load(Ordering::SeqCst)
    }
}

async fn run_child_with_quota(
    engine_factory: Arc<dyn SubagentEngineFactory>,
    request: ChildRunRequest,
    quota: Option<&ResourceQuota>,
) -> Result<ChildRunOutcome, SubagentError> {
    let mut result = if let Some(max_duration) = quota.and_then(|quota| quota.max_duration) {
        time::timeout(max_duration, engine_factory.run_child_engine(request))
            .await
            .unwrap_or_else(|_| {
                Ok(ChildRunOutcome {
                    status: SubagentStatus::MaxBudget(BudgetKind::WallClock),
                    summary: "subagent exceeded wall clock budget".to_owned(),
                    result: None,
                    usage: UsageSnapshot::default(),
                    transcript_ref: None,
                    context_report: None,
                })
            })
    } else {
        engine_factory.run_child_engine(request).await
    }?;
    if !matches!(result.status, SubagentStatus::MaxBudget(_)) {
        if let Some(kind) = quota_exceeded_kind(&result.usage, quota) {
            result.status = SubagentStatus::MaxBudget(kind);
        }
    }
    Ok(result)
}

fn quota_exceeded_kind(usage: &UsageSnapshot, quota: Option<&ResourceQuota>) -> Option<BudgetKind> {
    let quota = quota?;
    if let Some(limit) = quota.max_tokens {
        let observed = usage
            .input_tokens
            .saturating_add(usage.output_tokens)
            .saturating_add(usage.cache_read_tokens)
            .saturating_add(usage.cache_write_tokens);
        if observed >= limit {
            return Some(BudgetKind::Tokens);
        }
    }
    if quota
        .max_tool_calls
        .is_some_and(|limit| usage.tool_calls >= limit)
    {
        return Some(BudgetKind::ToolCalls);
    }
    if let Some(limit_cents) = quota.max_cost_cents {
        let limit = limit_cents.saturating_mul(10_000);
        if usage.cost_micros >= limit {
            return Some(BudgetKind::Cost);
        }
    }
    None
}

impl DefaultSubagentRunner {
    async fn assemble_context_seed(
        &self,
        spec: &SubagentSpec,
        parent: &ParentContext,
        child_session_id: SessionId,
    ) -> Result<(Vec<Message>, bool), SubagentError> {
        let SubagentContextMode::ForkFromParent {
            include_tool_results,
        } = spec.context_mode
        else {
            let (memory_seed, memory_scope_resolved) =
                self.resolve_memory_scope(spec, parent, child_session_id)?;
            return Ok((memory_seed, memory_scope_resolved));
        };

        let parent_envelopes: Vec<_> = self
            .event_store
            .read_envelopes(
                parent.tenant_id,
                parent.parent_session_id,
                ReplayCursor::FromStart,
            )
            .await
            .map_err(|error| SubagentError::Engine(error.to_string()))?
            .collect()
            .await;
        let from_offset = parent_envelopes
            .last()
            .map_or(JournalOffset(0), |envelope| envelope.offset);
        let transcript = parent_envelopes
            .iter()
            .filter_map(|envelope| {
                message_from_parent_event(&envelope.payload, include_tool_results)
            })
            .collect::<Vec<_>>();
        self.append_session_forked(parent, child_session_id, from_offset)
            .await?;

        let mut context_seed = match &spec.input_strategy {
            SubagentInputStrategy::LatestUserOnly => transcript
                .iter()
                .rev()
                .find(|message| message.role == MessageRole::User)
                .cloned()
                .into_iter()
                .collect(),
            SubagentInputStrategy::InheritAll => transcript.clone(),
            SubagentInputStrategy::Custom { selector_id } => {
                let selector = self.input_selectors.get(selector_id).ok_or_else(|| {
                    SubagentError::Engine(format!(
                        "subagent input selector is not configured: {selector_id}"
                    ))
                })?;
                selector.select(SubagentInputSelection {
                    parent,
                    child_session_id,
                    include_tool_results,
                    parent_transcript: &transcript,
                })?
            }
        };
        let (mut memory_seed, memory_scope_resolved) =
            self.resolve_memory_scope(spec, parent, child_session_id)?;
        memory_seed.append(&mut context_seed);
        Ok((memory_seed, memory_scope_resolved))
    }

    fn resolve_memory_scope(
        &self,
        spec: &SubagentSpec,
        parent: &ParentContext,
        child_session_id: SessionId,
    ) -> Result<(Vec<Message>, bool), SubagentError> {
        let SubagentMemoryScope::Subset { selectors } = &spec.memory_scope else {
            return Ok((Vec::new(), false));
        };
        let resolver = self.memory_scope_resolver.as_ref().ok_or_else(|| {
            SubagentError::Engine(
                "subagent memory_scope subset resolver is not configured".to_owned(),
            )
        })?;
        Ok((
            resolver.resolve(SubagentMemoryScopeRequest {
                parent,
                child_session_id,
                selectors,
            })?,
            true,
        ))
    }

    async fn append_session_forked(
        &self,
        parent: &ParentContext,
        child_session_id: SessionId,
        from_offset: JournalOffset,
    ) -> Result<(), SubagentError> {
        self.event_store
            .append_with_metadata(
                parent.tenant_id,
                parent.parent_session_id,
                AppendMetadata {
                    run_id: Some(parent.parent_run_id),
                    correlation_id: parent.correlation_id,
                    ..AppendMetadata::default()
                },
                &[Event::SessionForked(SessionForkedEvent {
                    parent_session_id: parent.parent_session_id,
                    child_session_id,
                    tenant_id: parent.tenant_id,
                    fork_reason: ForkReason::Isolation,
                    from_offset,
                    config_delta_hash: None,
                    cache_impact: CacheImpact {
                        prompt_cache_invalidated: false,
                        reason: None,
                    },
                    at: Utc::now(),
                })],
            )
            .await
            .map_err(|error| SubagentError::Engine(error.to_string()))?;
        Ok(())
    }

    async fn append_spawned(
        &self,
        spec: &SubagentSpec,
        parent: &ParentContext,
        subagent_id: SubagentId,
    ) -> Result<(), SubagentError> {
        let spec_bytes =
            serde_json::to_vec(spec).map_err(|error| SubagentError::Engine(error.to_string()))?;
        let spec_hash = *blake3::hash(&spec_bytes).as_bytes();
        self.event_store
            .append_with_metadata(
                parent.tenant_id,
                parent.parent_session_id,
                AppendMetadata {
                    run_id: Some(parent.parent_run_id),
                    correlation_id: parent.correlation_id,
                    ..AppendMetadata::default()
                },
                &[Event::SubagentSpawned(SubagentSpawnedEvent {
                    subagent_id,
                    parent_session_id: parent.parent_session_id,
                    parent_run_id: parent.parent_run_id,
                    agent_ref: AgentRef {
                        id: AgentId::new(),
                        name: spec.role.clone(),
                    },
                    spec_snapshot_id: snapshot_id_from_hash(&spec_hash),
                    spec_hash,
                    depth: parent.depth.saturating_add(1),
                    trigger_tool_use_id: parent.trigger_tool_use_id,
                    trigger_tool_name: Some("agent".to_owned()),
                    at: Utc::now(),
                })],
            )
            .await
            .map_err(|error| SubagentError::Engine(error.to_string()))?;
        Ok(())
    }

    async fn append_announced(
        &self,
        tenant_id: TenantId,
        announcement: &SubagentAnnouncement,
        run_id: RunId,
        correlation_id: CorrelationId,
    ) -> Result<(), SubagentError> {
        let rendered = self.announcement_renderer.render(announcement);
        let mut metadata = MessageMetadata {
            source: Some("subagent".to_owned()),
            ..MessageMetadata::default()
        };
        metadata
            .labels
            .insert("renderer_id".to_owned(), rendered.renderer_id.clone());
        metadata.labels.insert(
            "subagent_id".to_owned(),
            announcement.subagent_id.to_string(),
        );
        self.event_store
            .append_with_metadata(
                tenant_id,
                announcement.parent_session_id,
                AppendMetadata {
                    run_id: Some(run_id),
                    correlation_id,
                    ..AppendMetadata::default()
                },
                &[
                    Event::SubagentAnnounced(SubagentAnnouncedEvent {
                        subagent_id: announcement.subagent_id,
                        parent_session_id: announcement.parent_session_id,
                        status: announcement.status.clone(),
                        summary: announcement.summary.clone(),
                        result: announcement.result.clone(),
                        usage: announcement.usage.clone(),
                        transcript_ref: announcement.transcript_ref.clone(),
                        context_report: announcement.context_report.clone(),
                        renderer_id: rendered.renderer_id,
                        at: Utc::now(),
                    }),
                    Event::UserMessageAppended(UserMessageAppendedEvent {
                        run_id,
                        message_id: harness_contracts::MessageId::new(),
                        content: MessageContent::Text(rendered.user_message),
                        metadata,
                        attachments: Vec::new(),
                        at: Utc::now(),
                    }),
                ],
            )
            .await
            .map_err(|error| SubagentError::Engine(error.to_string()))?;
        Ok(())
    }

    async fn append_terminated(
        &self,
        tenant_id: TenantId,
        parent_session_id: SessionId,
        parent_run_id: RunId,
        correlation_id: CorrelationId,
        subagent_id: SubagentId,
        reason: SubagentTerminationReason,
        final_usage: UsageSnapshot,
    ) -> Result<(), SubagentError> {
        self.event_store
            .append_with_metadata(
                tenant_id,
                parent_session_id,
                AppendMetadata {
                    run_id: Some(parent_run_id),
                    correlation_id,
                    ..AppendMetadata::default()
                },
                &[Event::SubagentTerminated(SubagentTerminatedEvent {
                    subagent_id,
                    parent_session_id,
                    reason,
                    final_usage,
                    at: Utc::now(),
                })],
            )
            .await
            .map_err(|error| SubagentError::Engine(error.to_string()))?;
        Ok(())
    }

    pub async fn watchdog_tick(&self) -> Result<Vec<RunningSubagent>, SubagentError> {
        watchdog_tick_for(&self.pool, self.event_store.as_ref()).await
    }
}

fn message_from_parent_event(event: &Event, include_tool_results: bool) -> Option<Message> {
    match event {
        Event::RunStarted(started) => Some(started.input.message.clone()),
        Event::UserMessageAppended(appended) => Some(Message {
            id: appended.message_id,
            role: MessageRole::User,
            parts: message_parts(appended.content.clone()),
            created_at: appended.at,
        }),
        Event::AssistantMessageCompleted(completed) => Some(Message {
            id: completed.message_id,
            role: MessageRole::Assistant,
            parts: message_parts(completed.content.clone()),
            created_at: completed.at,
        }),
        Event::ToolUseCompleted(completed) if include_tool_results => Some(Message {
            id: MessageId::new(),
            role: MessageRole::Tool,
            parts: vec![MessagePart::ToolResult {
                tool_use_id: completed.tool_use_id,
                content: completed.result.clone(),
            }],
            created_at: completed.at,
        }),
        _ => None,
    }
}

fn message_parts(content: MessageContent) -> Vec<MessagePart> {
    match content {
        MessageContent::Text(text) => vec![MessagePart::Text(text)],
        MessageContent::Structured(value) => vec![MessagePart::Text(value.to_string())],
        MessageContent::Multimodal(parts) => parts,
    }
}

async fn watchdog_tick_for(
    pool: &ConcurrentSubagentPool,
    event_store: &dyn EventStore,
) -> Result<Vec<RunningSubagent>, SubagentError> {
    let stalled = pool.cancel_stalled();
    for running in &stalled {
        let stalled_for = Utc::now()
            .signed_duration_since(running.last_activity_at)
            .to_std()
            .unwrap_or_default();
        event_store
            .append_with_metadata(
                running.tenant_id,
                running.parent_session_id,
                AppendMetadata {
                    run_id: Some(running.parent_run_id),
                    correlation_id: running.correlation_id,
                    ..AppendMetadata::default()
                },
                &[
                    Event::SubagentStalled(SubagentStalledEvent {
                        subagent_id: running.subagent_id,
                        parent_session_id: running.parent_session_id,
                        parent_run_id: running.parent_run_id,
                        last_activity_at: running.last_activity_at,
                        stalled_for,
                        at: Utc::now(),
                    }),
                    Event::SubagentTerminated(SubagentTerminatedEvent {
                        subagent_id: running.subagent_id,
                        parent_session_id: running.parent_session_id,
                        reason: SubagentTerminationReason::Stalled {
                            silent_for_ms: stalled_for.as_millis() as u64,
                        },
                        final_usage: UsageSnapshot::default(),
                        at: Utc::now(),
                    }),
                ],
            )
            .await
            .map_err(|error| SubagentError::Engine(error.to_string()))?;
    }
    Ok(stalled)
}

async fn append_terminated_to(
    event_store: &dyn EventStore,
    tenant_id: TenantId,
    parent_session_id: SessionId,
    parent_run_id: RunId,
    correlation_id: CorrelationId,
    subagent_id: SubagentId,
    reason: SubagentTerminationReason,
    final_usage: UsageSnapshot,
) -> Result<(), SubagentError> {
    event_store
        .append_with_metadata(
            tenant_id,
            parent_session_id,
            AppendMetadata {
                run_id: Some(parent_run_id),
                correlation_id,
                ..AppendMetadata::default()
            },
            &[Event::SubagentTerminated(SubagentTerminatedEvent {
                subagent_id,
                parent_session_id,
                reason,
                final_usage,
                at: Utc::now(),
            })],
        )
        .await
        .map_err(|error| SubagentError::Engine(error.to_string()))?;
    Ok(())
}

fn snapshot_id_from_hash(hash: &[u8; 32]) -> SnapshotId {
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    SnapshotId::from_u128(u128::from_be_bytes(bytes))
}

fn termination_reason_for_error(error: &SubagentError) -> SubagentTerminationReason {
    match error {
        SubagentError::Cancelled => SubagentTerminationReason::ParentCancelled,
        _ => SubagentTerminationReason::Failed {
            detail: error.to_string(),
        },
    }
}

pub struct SubagentRunnerCapAdapter {
    inner: Arc<dyn SubagentRunner>,
    team_attribution: Option<SubagentTeamAttribution>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubagentTeamAttribution {
    team_id: TeamId,
    team_member_profile_id: String,
}

impl SubagentRunnerCapAdapter {
    #[must_use]
    pub fn from_runner(runner: Arc<dyn SubagentRunner>) -> Arc<dyn SubagentRunnerCap> {
        Arc::new(Self {
            inner: runner,
            team_attribution: None,
        })
    }

    #[must_use]
    pub fn from_runner_with_team_attribution(
        runner: Arc<dyn SubagentRunner>,
        team_id: TeamId,
        team_member_profile_id: impl Into<String>,
    ) -> Arc<dyn SubagentRunnerCap> {
        Arc::new(Self {
            inner: runner,
            team_attribution: Some(SubagentTeamAttribution {
                team_id,
                team_member_profile_id: team_member_profile_id.into(),
            }),
        })
    }
}

impl SubagentRunnerCap for SubagentRunnerCapAdapter {
    fn spawn(
        &self,
        spec: Value,
        parent: SubagentParentContext,
    ) -> BoxFuture<'static, Result<SubagentSpawnHandle, ToolError>> {
        let inner = Arc::clone(&self.inner);
        let team_attribution = self.team_attribution.clone();
        Box::pin(async move {
            let spec: SubagentSpec = serde_json::from_value(spec)
                .map_err(|error| ToolError::Validation(error.to_string()))?;
            let mut parent_ctx = ParentContext::from(parent);
            if let Some(attribution) = team_attribution {
                parent_ctx.team_id = Some(attribution.team_id);
                parent_ctx.team_member_profile_id = Some(attribution.team_member_profile_id);
            }
            let input = turn_input(&spec.task);
            let handle = inner
                .spawn(spec, input.clone(), parent_ctx)
                .await
                .map_err(|error| ToolError::Internal(error.to_string()))?;
            let announcement = handle
                .wait()
                .await
                .map_err(|error| ToolError::Internal(error.to_string()))?;
            Ok(SubagentSpawnHandle {
                subagent_id: announcement.subagent_id,
                input,
                announcement: SubagentCapAnnouncement {
                    subagent_id: announcement.subagent_id,
                    status: announcement.status,
                    summary: announcement.summary,
                    result: announcement.result,
                    usage: announcement.usage,
                    transcript_ref: announcement.transcript_ref,
                },
            })
        })
    }
}

pub struct AgentTool {
    descriptor: ToolDescriptor,
}

impl Default for AgentTool {
    fn default() -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: "agent".to_owned(),
                display_name: "Agent".to_owned(),
                description: "Delegate a bounded task to a subagent.".to_owned(),
                category: "builtin".to_owned(),
                group: ToolGroup::Coordinator,
                version: "0.1.0".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "required": ["role", "task"],
                    "properties": {
                        "role": { "type": "string" },
                        "task": { "type": "string" },
                        "prompt_template": { "type": "object" }
                    }
                }),
                output_schema: None,
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: false,
                    is_read_only: false,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: harness_contracts::DeferPolicy::AlwaysLoad,
                },
                trust_level: harness_contracts::TrustLevel::AdminTrusted,
                required_capabilities: vec![ToolCapability::SubagentRunner],
                budget: harness_contracts::ResultBudget {
                    metric: harness_contracts::BudgetMetric::Chars,
                    limit: 8_000,
                    on_overflow: harness_contracts::OverflowAction::Offload,
                    preview_head_chars: 2_000,
                    preview_tail_chars: 2_000,
                },
                provider_restriction: harness_contracts::ProviderRestriction::All,
                origin: ToolOrigin::Builtin,
                search_hint: Some("delegate task to subagent".to_owned()),
                service_binding: None,
            },
        }
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        let role = input
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let task = input
            .get("task")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if role.is_empty() || task.is_empty() {
            return Err(ValidationError::Message(
                "role and task are required".to_owned(),
            ));
        }
        Ok(())
    }

    async fn check_permission(&self, _input: &Value, _ctx: &ToolContext) -> PermissionCheck {
        PermissionCheck::Allowed
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolStream, ToolError> {
        let spec = normalize_agent_input(input)?;
        let runner = ctx.capability::<dyn SubagentRunnerCap>(ToolCapability::SubagentRunner)?;
        let parent = SubagentParentContext {
            tenant_id: ctx.tenant_id,
            parent_session_id: ctx.session_id,
            parent_run_id: ctx.run_id,
            depth: ctx.subagent_depth,
            sibling_count: 0,
            trigger_tool_use_id: Some(ctx.tool_use_id),
            correlation_id: ctx.correlation_id,
        };
        let handle = runner
            .spawn(
                serde_json::to_value(spec)
                    .map_err(|error| ToolError::Internal(error.to_string()))?,
                parent,
            )
            .await?;
        let announcement = handle.wait().await?;
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(announcement_json(announcement)),
        )])))
    }
}

fn normalize_agent_input(input: Value) -> Result<SubagentSpec, ToolError> {
    if input.get("toolset").is_some() {
        serde_json::from_value(input).map_err(|error| ToolError::Validation(error.to_string()))
    } else {
        let role = input
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::Validation("role is required".to_owned()))?;
        let task = input
            .get("task")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::Validation("task is required".to_owned()))?;
        Ok(SubagentSpec::minimal(role, task))
    }
}

fn announcement_json(announcement: SubagentCapAnnouncement) -> Value {
    json!({
        "subagent_id": announcement.subagent_id.to_string(),
        "status": announcement.status,
        "summary": announcement.summary,
        "result": announcement.result,
        "usage": announcement.usage,
        "transcript_ref": announcement.transcript_ref
    })
}

fn turn_input(text: &str) -> TurnInput {
    TurnInput {
        message: Message {
            id: harness_contracts::MessageId::new(),
            role: MessageRole::User,
            parts: vec![MessagePart::Text(text.to_owned())],
            created_at: Utc::now(),
        },
        metadata: Value::Null,
    }
}

#[derive(Clone)]
pub struct ConcurrentSubagentPool {
    policy: ConcurrencyPolicy,
    global: Arc<Semaphore>,
    buckets: Arc<DashMap<PoolBucket, Arc<Semaphore>>>,
    running: Arc<DashMap<SubagentId, RunningSubagent>>,
}

#[derive(Debug, Clone)]
pub struct ConcurrencyPolicy {
    pub per_bucket_limit: usize,
    pub global_limit: usize,
    pub acquire_timeout: Duration,
    pub activity_timeout: Duration,
}

impl Default for ConcurrencyPolicy {
    fn default() -> Self {
        Self {
            per_bucket_limit: 3,
            global_limit: 128,
            acquire_timeout: Duration::from_secs(30),
            activity_timeout: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
struct PoolBucket {
    parent_session_id: SessionId,
    depth: u8,
}

impl ConcurrentSubagentPool {
    #[must_use]
    pub fn new(max_concurrent_children: usize) -> Self {
        Self::with_policy(ConcurrencyPolicy {
            per_bucket_limit: max_concurrent_children,
            ..ConcurrencyPolicy::default()
        })
    }

    #[must_use]
    pub fn with_policy(policy: ConcurrencyPolicy) -> Self {
        Self {
            global: Arc::new(Semaphore::new(policy.global_limit)),
            policy,
            buckets: Arc::new(DashMap::new()),
            running: Arc::new(DashMap::new()),
        }
    }

    pub async fn acquire(&self, parent: &ParentContext) -> Result<SubagentSlot, SubagentError> {
        let bucket = self.semaphore_for(parent).clone();
        let acquire = async move {
            let global = self
                .global
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| SubagentError::ConcurrentLimitExceeded)?;
            let bucket = bucket
                .acquire_owned()
                .await
                .map_err(|_| SubagentError::ConcurrentLimitExceeded)?;
            Ok::<_, SubagentError>(SubagentSlot {
                _global_permit: global,
                _bucket_permit: bucket,
            })
        };
        time::timeout(self.policy.acquire_timeout, acquire)
            .await
            .map_err(|_| SubagentError::ConcurrentLimitExceeded)?
    }

    pub fn try_acquire(&self, parent: &ParentContext) -> Result<SubagentSlot, SubagentError> {
        let global = self
            .global
            .clone()
            .try_acquire_owned()
            .map_err(|_| SubagentError::ConcurrentLimitExceeded)?;
        let bucket = self
            .semaphore_for(parent)
            .clone()
            .try_acquire_owned()
            .map_err(|_| SubagentError::ConcurrentLimitExceeded)?;
        Ok(SubagentSlot {
            _global_permit: global,
            _bucket_permit: bucket,
        })
    }

    fn semaphore_for(&self, parent: &ParentContext) -> Arc<Semaphore> {
        let bucket = PoolBucket {
            parent_session_id: parent.parent_session_id,
            depth: parent.depth,
        };
        Arc::clone(
            &self
                .buckets
                .entry(bucket)
                .or_insert_with(|| Arc::new(Semaphore::new(self.policy.per_bucket_limit))),
        )
    }

    pub fn register_running(
        &self,
        subagent_id: SubagentId,
        parent: &ParentContext,
        role: String,
    ) -> SubagentCancellationToken {
        let cancellation = SubagentCancellationToken::new();
        let now = Utc::now();
        self.running.insert(
            subagent_id,
            RunningSubagent {
                subagent_id,
                parent_session_id: parent.parent_session_id,
                parent_run_id: parent.parent_run_id,
                tenant_id: parent.tenant_id,
                correlation_id: parent.correlation_id,
                depth: parent.depth,
                role,
                spawned_at: now,
                last_activity_at: now,
                cancellation: cancellation.clone(),
            },
        );
        cancellation
    }

    pub fn mark_activity(&self, subagent_id: &SubagentId) {
        if let Some(mut running) = self.running.get_mut(subagent_id) {
            running.last_activity_at = Utc::now();
        }
    }

    pub fn finish(&self, subagent_id: &SubagentId) -> Option<RunningSubagent> {
        self.running.remove(subagent_id).map(|(_, running)| running)
    }

    #[must_use]
    pub fn running_count(&self) -> usize {
        self.running.len()
    }

    #[must_use]
    pub fn list_running(&self) -> Vec<RunningSubagent> {
        self.running
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    #[must_use]
    pub fn stalled(&self) -> Vec<RunningSubagent> {
        let now = Utc::now();
        self.running
            .iter()
            .filter_map(|entry| {
                let running = entry.value();
                let stalled_for = now.signed_duration_since(running.last_activity_at);
                match stalled_for.to_std() {
                    Ok(duration) if duration >= self.policy.activity_timeout => {
                        Some(running.clone())
                    }
                    _ => None,
                }
            })
            .collect()
    }

    pub fn cancel_stalled(&self) -> Vec<RunningSubagent> {
        let stalled = self.stalled();
        let mut cancelled = Vec::new();
        for running in stalled {
            if let Some((_, running)) = self.running.remove(&running.subagent_id) {
                running.cancellation.cancel();
                cancelled.push(running);
            }
        }
        cancelled
    }

    pub fn cancel_all(&self) {
        for running in self.running.iter() {
            running.cancellation.cancel();
        }
    }

    pub fn cancel(&self, subagent_id: &SubagentId) -> Result<RunningSubagent, SubagentError> {
        let Some((_, running)) = self.running.remove(subagent_id) else {
            return Err(SubagentError::Cancelled);
        };
        running.cancellation.cancel();
        Ok(running)
    }
}

#[derive(Debug)]
pub struct SubagentSlot {
    _global_permit: OwnedSemaphorePermit,
    _bucket_permit: OwnedSemaphorePermit,
}

#[derive(Debug, Clone)]
pub struct RunningSubagent {
    pub subagent_id: SubagentId,
    pub parent_session_id: SessionId,
    pub parent_run_id: RunId,
    pub tenant_id: TenantId,
    pub correlation_id: CorrelationId,
    pub depth: u8,
    pub role: String,
    pub spawned_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
    pub cancellation: SubagentCancellationToken,
}

pub mod testing {
    use super::*;

    #[must_use]
    pub fn tool_context_with_caps(cap_registry: Arc<CapabilityRegistry>) -> ToolContext {
        ToolContext {
            tool_use_id: ToolUseId::new(),
            run_id: RunId::new(),
            session_id: SessionId::new(),
            tenant_id: TenantId::SINGLE,
            correlation_id: CorrelationId::new(),
            agent_id: AgentId::new(),
            subagent_depth: 0,
            workspace_root: PathBuf::from("."),
            sandbox: None,
            permission_broker: Arc::new(AllowBroker),
            cap_registry,
            redactor: Arc::new(NoopRedactor),
            interrupt: harness_tool::InterruptToken::new(),
            parent_run: None,
            model: None,
            model_config_id: None,
        }
    }

    struct AllowBroker;

    #[async_trait]
    impl PermissionBroker for AllowBroker {
        async fn decide(
            &self,
            _request: PermissionRequest,
            _ctx: PermissionContext,
        ) -> harness_contracts::Decision {
            harness_contracts::Decision::AllowOnce
        }

        async fn persist(
            &self,
            _decision: harness_permission::PersistedDecision,
        ) -> Result<(), harness_contracts::PermissionError> {
            Ok(())
        }
    }
}

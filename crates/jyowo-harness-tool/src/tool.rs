use std::collections::HashMap;
use std::fmt;
use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::Stream;
use harness_contracts::{
    ActionPlanHash, ActionPlanId, ActionResource, AuthorizationTicketId, DecisionScope, Event,
    MessagePart, NetworkAccess, PermissionReview, PermissionSubject, ResourceLimits, RunId,
    SandboxMode, SandboxPolicy, SandboxScope, SessionId, Severity, TenantId, ToolActionPlan,
    ToolDescriptor, ToolError, ToolExecutionChannel, ToolResult, ToolUseId, WorkspaceAccess,
};
use harness_permission::PermissionCheck;
use parking_lot::Mutex;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::Value;
use thiserror::Error;

use crate::{SchemaResolverContext, ToolContext, ValidationError};

pub type ToolStream = Pin<Box<dyn Stream<Item = ToolEvent> + Send + 'static>>;

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum ToolJournalAuthority {
    #[default]
    None,
    Clarification,
    Sandbox,
    ExecuteCode,
}

#[async_trait]
pub trait Tool: Send + Sync + 'static {
    fn descriptor(&self) -> &ToolDescriptor;

    fn input_schema(&self) -> &Value {
        &self.descriptor().input_schema
    }

    fn output_schema(&self) -> Option<&Value> {
        self.descriptor().output_schema.as_ref()
    }

    async fn resolve_schema(&self, _ctx: &SchemaResolverContext) -> Result<Value, ToolError> {
        Ok(self.input_schema().clone())
    }

    async fn validate(&self, input: &Value, ctx: &ToolContext) -> Result<(), ValidationError>;

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError>;

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError>;
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthorizationTicketClaims {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: ToolUseId,
    pub tool_name: String,
    pub action_plan_hash: ActionPlanHash,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuthorizationTicket {
    pub id: AuthorizationTicketId,
    pub claims: AuthorizationTicketClaims,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum TicketLedgerError {
    #[error("invalid ticket ttl: {reason}")]
    InvalidTtl { reason: String },
    #[error("authorization ticket {ticket_id} is unknown")]
    Unknown { ticket_id: AuthorizationTicketId },
    #[error("authorization ticket {ticket_id} expired at {expires_at}")]
    Expired {
        ticket_id: AuthorizationTicketId,
        expires_at: DateTime<Utc>,
    },
    #[error("authorization ticket {ticket_id} was already consumed")]
    Consumed { ticket_id: AuthorizationTicketId },
    #[error("authorization ticket {ticket_id} does not match the requested action")]
    ScopeMismatch { ticket_id: AuthorizationTicketId },
}

#[derive(Clone)]
pub struct AuthorizationTicketKey {
    secret: [u8; 32],
}

impl fmt::Debug for AuthorizationTicketKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthorizationTicketKey")
            .finish_non_exhaustive()
    }
}

impl AuthorizationTicketKey {
    #[must_use]
    pub fn generate() -> Self {
        let rng = SystemRandom::new();
        let mut secret = [0_u8; 32];
        rng.fill(&mut secret)
            .expect("secure randomness is required for authorization ticket proof keys");
        Self { secret }
    }

    #[must_use]
    pub fn verify_summary(&self, summary: &AuthorizedTicketSummary) -> bool {
        self.proof_for_summary(summary.ticket_id, &summary.claims(), summary.consumed_at)
            == summary.proof
    }

    fn proof_for_summary(
        &self,
        ticket_id: AuthorizationTicketId,
        claims: &AuthorizationTicketClaims,
        consumed_at: DateTime<Utc>,
    ) -> AuthorizedTicketProof {
        let mut hasher = blake3::Hasher::new_keyed(&self.secret);
        update_len_prefixed(&mut hasher, b"jyowo.authorized_ticket_summary.v1");
        update_len_prefixed(&mut hasher, ticket_id.to_string().as_bytes());
        update_len_prefixed(&mut hasher, claims.tenant_id.to_string().as_bytes());
        update_len_prefixed(&mut hasher, claims.session_id.to_string().as_bytes());
        update_len_prefixed(&mut hasher, claims.run_id.to_string().as_bytes());
        update_len_prefixed(&mut hasher, claims.tool_use_id.to_string().as_bytes());
        update_len_prefixed(&mut hasher, claims.tool_name.as_bytes());
        update_len_prefixed(&mut hasher, claims.action_plan_hash.as_bytes());
        update_len_prefixed(
            &mut hasher,
            consumed_at.timestamp_millis().to_string().as_bytes(),
        );
        AuthorizedTicketProof {
            mac: *hasher.finalize().as_bytes(),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
struct AuthorizedTicketProof {
    mac: [u8; 32],
}

impl fmt::Debug for AuthorizedTicketProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthorizedTicketProof")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct AuthorizedTicketSummary {
    ticket_id: AuthorizationTicketId,
    tenant_id: TenantId,
    session_id: SessionId,
    run_id: RunId,
    tool_use_id: ToolUseId,
    tool_name: String,
    action_plan_hash: ActionPlanHash,
    consumed_at: DateTime<Utc>,
    proof: AuthorizedTicketProof,
}

impl fmt::Debug for AuthorizedTicketSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthorizedTicketSummary")
            .field("ticket_id", &self.ticket_id)
            .field("tenant_id", &self.tenant_id)
            .field("session_id", &self.session_id)
            .field("run_id", &self.run_id)
            .field("tool_use_id", &self.tool_use_id)
            .field("tool_name", &self.tool_name)
            .field("action_plan_hash", &self.action_plan_hash)
            .field("consumed_at", &self.consumed_at)
            .finish_non_exhaustive()
    }
}

impl AuthorizedTicketSummary {
    #[must_use]
    pub fn ticket_id(&self) -> AuthorizationTicketId {
        self.ticket_id
    }

    #[must_use]
    pub fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }

    #[must_use]
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    #[must_use]
    pub fn run_id(&self) -> RunId {
        self.run_id
    }

    #[must_use]
    pub fn tool_use_id(&self) -> ToolUseId {
        self.tool_use_id
    }

    #[must_use]
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    #[must_use]
    pub fn action_plan_hash(&self) -> &ActionPlanHash {
        &self.action_plan_hash
    }

    #[must_use]
    pub fn consumed_at(&self) -> DateTime<Utc> {
        self.consumed_at
    }

    #[must_use]
    pub fn verify_authority(&self, key: &AuthorizationTicketKey) -> bool {
        key.verify_summary(self)
    }

    fn from_consumed_ticket(
        ticket: AuthorizationTicket,
        consumed_at: DateTime<Utc>,
        key: &AuthorizationTicketKey,
    ) -> Self {
        let proof = key.proof_for_summary(ticket.id, &ticket.claims, consumed_at);
        Self {
            ticket_id: ticket.id,
            tenant_id: ticket.claims.tenant_id,
            session_id: ticket.claims.session_id,
            run_id: ticket.claims.run_id,
            tool_use_id: ticket.claims.tool_use_id,
            tool_name: ticket.claims.tool_name,
            action_plan_hash: ticket.claims.action_plan_hash,
            consumed_at,
            proof,
        }
    }

    fn claims(&self) -> AuthorizationTicketClaims {
        AuthorizationTicketClaims {
            tenant_id: self.tenant_id,
            session_id: self.session_id,
            run_id: self.run_id,
            tool_use_id: self.tool_use_id,
            tool_name: self.tool_name.clone(),
            action_plan_hash: self.action_plan_hash.clone(),
        }
    }
}

#[derive(Debug)]
struct TicketRecord {
    ticket: AuthorizationTicket,
    consumed: bool,
}

#[derive(Debug)]
pub struct TicketLedger {
    ttl: Duration,
    authority_key: AuthorizationTicketKey,
    tickets: Mutex<HashMap<AuthorizationTicketId, TicketRecord>>,
}

impl Default for TicketLedger {
    fn default() -> Self {
        Self::new(Duration::from_secs(300))
    }
}

impl TicketLedger {
    #[must_use]
    pub fn new(ttl: Duration) -> Self {
        Self::with_authority_key(ttl, AuthorizationTicketKey::generate())
    }

    #[must_use]
    pub fn with_authority_key(ttl: Duration, authority_key: AuthorizationTicketKey) -> Self {
        Self {
            ttl,
            authority_key,
            tickets: Mutex::new(HashMap::new()),
        }
    }

    #[must_use]
    pub fn authority_key(&self) -> AuthorizationTicketKey {
        self.authority_key.clone()
    }

    pub fn mint(
        &self,
        claims: AuthorizationTicketClaims,
        now: DateTime<Utc>,
    ) -> Result<AuthorizationTicket, TicketLedgerError> {
        let ttl = chrono::Duration::from_std(self.ttl).map_err(|error| {
            TicketLedgerError::InvalidTtl {
                reason: error.to_string(),
            }
        })?;
        let ticket = AuthorizationTicket {
            id: AuthorizationTicketId::new(),
            claims,
            issued_at: now,
            expires_at: now + ttl,
        };
        self.tickets.lock().insert(
            ticket.id,
            TicketRecord {
                ticket: ticket.clone(),
                consumed: false,
            },
        );
        Ok(ticket)
    }

    pub fn consume(
        &self,
        ticket_id: AuthorizationTicketId,
        claims: &AuthorizationTicketClaims,
        now: DateTime<Utc>,
    ) -> Result<AuthorizedTicketSummary, TicketLedgerError> {
        let mut tickets = self.tickets.lock();
        let Some(record) = tickets.get_mut(&ticket_id) else {
            return Err(TicketLedgerError::Unknown { ticket_id });
        };
        if record.consumed {
            return Err(TicketLedgerError::Consumed { ticket_id });
        }
        if now > record.ticket.expires_at {
            return Err(TicketLedgerError::Expired {
                ticket_id,
                expires_at: record.ticket.expires_at,
            });
        }
        if &record.ticket.claims != claims {
            return Err(TicketLedgerError::ScopeMismatch { ticket_id });
        }

        record.consumed = true;
        Ok(AuthorizedTicketSummary::from_consumed_ticket(
            record.ticket.clone(),
            now,
            &self.authority_key,
        ))
    }

    pub fn revoke(&self, ticket_id: AuthorizationTicketId) {
        self.tickets.lock().remove(&ticket_id);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizedToolInput {
    raw_input: Value,
    action_plan: ToolActionPlan,
    ticket: AuthorizedTicketSummary,
}

impl AuthorizedToolInput {
    pub fn new(
        raw_input: Value,
        action_plan: ToolActionPlan,
        ticket: AuthorizedTicketSummary,
    ) -> Result<Self, ToolError> {
        if action_plan.tool_use_id != ticket.tool_use_id {
            return Err(ToolError::PermissionDenied(
                "authorization ticket tool use does not match action plan".to_owned(),
            ));
        }
        if action_plan.tool_name != ticket.tool_name {
            return Err(ToolError::PermissionDenied(
                "authorization ticket tool name does not match action plan".to_owned(),
            ));
        }
        if action_plan.plan_hash != canonical_action_plan_hash(&action_plan) {
            return Err(ToolError::PermissionDenied(
                "authorization ticket action plan hash does not match canonical action plan"
                    .to_owned(),
            ));
        }
        if action_plan.plan_hash != ticket.action_plan_hash {
            return Err(ToolError::PermissionDenied(
                "authorization ticket action plan hash does not match action plan".to_owned(),
            ));
        }

        Ok(Self {
            raw_input,
            action_plan,
            ticket,
        })
    }

    #[must_use]
    pub fn raw_input(&self) -> &Value {
        &self.raw_input
    }

    #[must_use]
    pub fn action_plan(&self) -> &ToolActionPlan {
        &self.action_plan
    }

    #[must_use]
    pub fn ticket(&self) -> &AuthorizedTicketSummary {
        &self.ticket
    }
}

pub fn action_plan_from_permission_check(
    descriptor: &ToolDescriptor,
    input: &Value,
    ctx: &ToolContext,
    check: PermissionCheck,
    resources: Vec<ActionResource>,
    workspace_access: WorkspaceAccess,
    network_access: NetworkAccess,
    execution_channel: ToolExecutionChannel,
) -> Result<ToolActionPlan, ToolError> {
    let (subject, severity, scope) = match check {
        PermissionCheck::Allowed => (
            PermissionSubject::ToolInvocation {
                tool: descriptor.name.clone(),
                input: input.clone(),
            },
            Severity::Info,
            DecisionScope::ToolName(descriptor.name.clone()),
        ),
        PermissionCheck::AskUser { subject, scope } => (subject, Severity::Medium, scope),
        PermissionCheck::DangerousPattern {
            severity,
            subject,
            scope,
            ..
        } => (subject, severity, scope),
        PermissionCheck::DangerousCommand {
            command,
            pattern,
            severity,
        } => (
            PermissionSubject::DangerousCommand {
                command,
                pattern_id: pattern,
                severity,
            },
            severity,
            DecisionScope::ToolName(descriptor.name.clone()),
        ),
        PermissionCheck::Denied { reason } => return Err(ToolError::PermissionDenied(reason)),
    };

    let mut plan = ToolActionPlan {
        plan_id: ActionPlanId::new(),
        tool_use_id: ctx.tool_use_id,
        tool_name: descriptor.name.clone(),
        actor_source: ctx.actor_source.clone(),
        subject,
        scope,
        severity,
        resources,
        sandbox_policy: default_sandbox_policy(network_access.clone()),
        workspace_access,
        network_access,
        execution_channel,
        review: PermissionReview::default(),
        plan_hash: ActionPlanHash::default(),
        created_at: Utc::now(),
    };
    plan.plan_hash = canonical_action_plan_hash(&plan);
    Ok(plan)
}

#[must_use]
pub fn canonical_action_plan_hash(plan: &ToolActionPlan) -> ActionPlanHash {
    let canonical = serde_json::json!({
        "version": 1_u8,
        "tool_use_id": plan.tool_use_id,
        "tool_name": plan.tool_name,
        "actor_source": plan.actor_source,
        "subject": plan.subject,
        "scope": plan.scope,
        "severity": plan.severity,
        "resources": plan.resources,
        "sandbox_policy": plan.sandbox_policy,
        "workspace_access": plan.workspace_access,
        "network_access": plan.network_access,
        "execution_channel": plan.execution_channel,
        "review": plan.review,
    });
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(b"jyowo.action_plan.v1".len() as u64).to_le_bytes());
    hasher.update(b"jyowo.action_plan.v1");
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(&bytes);
    ActionPlanHash::from_bytes(*hasher.finalize().as_bytes())
}

fn update_len_prefixed(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

pub fn authorized_file_path(
    authorized: &AuthorizedToolInput,
    kind: AuthorizedFileResourceKind,
) -> Result<std::path::PathBuf, ToolError> {
    authorized
        .action_plan
        .resources
        .iter()
        .find_map(|resource| match (kind, resource) {
            (AuthorizedFileResourceKind::Read, ActionResource::FileRead { path }) => {
                Some(path.clone())
            }
            (AuthorizedFileResourceKind::Write, ActionResource::FileWrite { path, .. }) => {
                Some(path.clone())
            }
            (AuthorizedFileResourceKind::Delete, ActionResource::FileDelete { path }) => {
                Some(path.clone())
            }
            _ => None,
        })
        .ok_or_else(|| ToolError::PermissionDenied("authorized file resource missing".to_owned()))
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AuthorizedFileResourceKind {
    Read,
    Write,
    Delete,
}

fn default_sandbox_policy(network: NetworkAccess) -> SandboxPolicy {
    SandboxPolicy {
        mode: SandboxMode::None,
        scope: SandboxScope::WorkspaceOnly,
        network,
        resource_limits: ResourceLimits {
            max_memory_bytes: None,
            max_cpu_cores: None,
            max_pids: None,
            max_wall_clock_ms: None,
            max_open_files: None,
        },
        denied_host_paths: Vec::new(),
    }
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum ToolEvent {
    Progress(ToolProgress),
    Partial(MessagePart),
    Journal(Event),
    Final(ToolResult),
    Error(ToolError),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolProgress {
    pub message: String,
    pub fraction: Option<f32>,
    pub at: DateTime<Utc>,
}

impl ToolProgress {
    pub fn now(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            fraction: None,
            at: Utc::now(),
        }
    }

    pub fn with_fraction(message: impl Into<String>, fraction: f32) -> Self {
        Self {
            message: message.into(),
            fraction: Some(fraction),
            at: Utc::now(),
        }
    }
}

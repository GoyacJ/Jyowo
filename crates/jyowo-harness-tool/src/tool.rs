use std::pin::Pin;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::Stream;
use harness_contracts::{
    ActionPlanHash, ActionPlanId, ActionResource, AuthorizationTicketId, DecisionScope, Event,
    MessagePart, NetworkAccess, PermissionActorSource, PermissionReview, PermissionSubject,
    ResourceLimits, RunId, SandboxMode, SandboxPolicy, SandboxScope, SessionId, Severity, TenantId,
    ToolActionPlan, ToolDescriptor, ToolError, ToolResult, ToolUseId, WorkspaceAccess,
};
use harness_permission::{canonical_permission_fingerprint, PermissionCheck, PermissionRequest};
use serde_json::Value;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedTicketSummary {
    pub ticket_id: AuthorizationTicketId,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_use_id: ToolUseId,
    pub tool_name: String,
    pub action_plan_hash: ActionPlanHash,
    pub consumed_at: DateTime<Utc>,
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
        PermissionCheck::DangerousCommand { pattern, severity } => (
            PermissionSubject::DangerousCommand {
                command: descriptor.name.clone(),
                pattern_id: pattern,
                severity,
            },
            severity,
            DecisionScope::ToolName(descriptor.name.clone()),
        ),
        PermissionCheck::Denied { reason } => return Err(ToolError::PermissionDenied(reason)),
    };

    let request = PermissionRequest {
        request_id: harness_contracts::RequestId::new(),
        tenant_id: ctx.tenant_id,
        session_id: ctx.session_id,
        tool_use_id: ctx.tool_use_id,
        tool_name: descriptor.name.clone(),
        subject: subject.clone(),
        severity,
        scope_hint: scope.clone(),
        created_at: Utc::now(),
    };
    let plan_hash = ActionPlanHash::from_bytes(canonical_permission_fingerprint(&request).0);

    Ok(ToolActionPlan {
        plan_id: ActionPlanId::new(),
        tool_use_id: ctx.tool_use_id,
        tool_name: descriptor.name.clone(),
        actor_source: PermissionActorSource::ParentRun,
        subject,
        scope,
        severity,
        resources,
        sandbox_policy: default_sandbox_policy(network_access.clone()),
        workspace_access,
        network_access,
        review: PermissionReview::default(),
        plan_hash,
        created_at: Utc::now(),
    })
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

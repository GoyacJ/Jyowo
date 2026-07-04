use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use harness_contracts::{
    ActionPlanHash, ActionResource, DecidedBy, Decision, DecisionLifetime, DecisionMatcherKind,
    DecisionMatcherSummary, DecisionScope, Event, FallbackPolicy, HostRule, InteractivityLevel,
    NetworkAccess, PermissionConfirmation, PermissionDecisionOption, PermissionMode,
    PermissionOptionId, PermissionRequestedEvent, PermissionResolvedEvent, ResourceLimits, RunId,
    SandboxPolicy, SandboxPolicyHash, SandboxPolicySummary, SandboxPreflightFailedEvent,
    SandboxPreflightPassedEvent, SandboxPreflightStatus, SessionId, TenantId, ToolActionPlan,
};
use harness_permission::{
    canonical_permission_fingerprint, PermissionAuthority, PermissionAuthorityDecisionSource,
    PermissionContext, PermissionRequest,
};
use harness_sandbox::{ExecSpec, SandboxBackend, SandboxCapabilities};
use harness_tool::{AuthorizedTicketSummary, AuthorizedToolInput};

use crate::{
    AuthorizationAudit, AuthorizationEventSink, AuthorizationTicket, AuthorizationTicketClaims,
    ExecutionError, TicketLedger,
};

pub struct AuthorizationService {
    permission_authority: Arc<PermissionAuthority>,
    sandbox_backend: Arc<dyn SandboxBackend>,
    event_sink: Arc<dyn AuthorizationEventSink>,
    ticket_ledger: Arc<TicketLedger>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizationContext {
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub permission_mode: PermissionMode,
    pub interactivity: InteractivityLevel,
    pub fallback_policy: FallbackPolicy,
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuthorizationOutcome {
    pub decision: Decision,
    pub ticket: AuthorizationTicket,
    pub action_plan_hash: ActionPlanHash,
    pub sandbox_backend_id: String,
    pub audit: AuthorizationAudit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedOperation {
    pub ticket: AuthorizedTicketSummary,
    pub action_plan_hash: ActionPlanHash,
    pub sandbox_backend_id: String,
}

impl AuthorizationService {
    #[must_use]
    pub fn new(
        permission_authority: Arc<PermissionAuthority>,
        sandbox_backend: Arc<dyn SandboxBackend>,
        event_sink: Arc<dyn AuthorizationEventSink>,
        ticket_ledger: Arc<TicketLedger>,
    ) -> Self {
        Self {
            permission_authority,
            sandbox_backend,
            event_sink,
            ticket_ledger,
        }
    }

    #[must_use]
    pub fn permission_authority(&self) -> Arc<PermissionAuthority> {
        Arc::clone(&self.permission_authority)
    }

    pub async fn authorize_plan(
        &self,
        context: AuthorizationContext,
        plan: ToolActionPlan,
    ) -> Result<AuthorizationOutcome, ExecutionError> {
        let request = PermissionRequest {
            request_id: harness_contracts::RequestId::new(),
            tenant_id: context.tenant_id,
            session_id: context.session_id,
            tool_use_id: plan.tool_use_id,
            tool_name: plan.tool_name.clone(),
            subject: plan.subject.clone(),
            severity: plan.severity,
            scope_hint: plan.scope.clone(),
            confirmation_expected: confirmation_expected(&plan.review.confirmation),
            created_at: Utc::now(),
        };
        let fingerprint = canonical_permission_fingerprint(&request);
        let permission_context = PermissionContext {
            permission_mode: context.permission_mode,
            previous_mode: None,
            session_id: context.session_id,
            tenant_id: context.tenant_id,
            run_id: Some(context.run_id),
            interactivity: context.interactivity,
            timeout_policy: None,
            fallback_policy: context.fallback_policy,
            hook_overrides: Vec::new(),
        };

        let requested = Event::PermissionRequested(PermissionRequestedEvent {
            request_id: request.request_id,
            run_id: context.run_id,
            session_id: context.session_id,
            tenant_id: context.tenant_id,
            tool_use_id: plan.tool_use_id,
            tool_name: plan.tool_name.clone(),
            subject: plan.subject.clone(),
            severity: plan.severity,
            scope_hint: plan.scope.clone(),
            fingerprint: Some(fingerprint),
            presented_options: vec![PermissionDecisionOption {
                option_id: PermissionOptionId::new(),
                decision: Decision::AllowOnce,
                scope: DecisionScope::Any,
                lifetime: DecisionLifetime::Once,
                matcher_summary: DecisionMatcherSummary {
                    kind: DecisionMatcherKind::Any,
                    label: "allow once".to_owned(),
                },
                label: "Allow once".to_owned(),
                requires_confirmation: false,
                action_plan_hash: ActionPlanHash::default(),
                fingerprint: None,
            }],
            interactivity: context.interactivity,
            auto_resolved: matches!(
                context.permission_mode,
                PermissionMode::BypassPermissions | PermissionMode::DontAsk
            ) || matches!(context.interactivity, InteractivityLevel::NoInteractive),
            actor_source: plan.actor_source.clone(),
            action_plan_hash: plan.plan_hash.clone(),
            review: plan.review.clone(),
            effective_mode: context.permission_mode,
            sandbox_policy: sandbox_policy_summary(&plan.sandbox_policy),
            causation_id: harness_contracts::EventId::new(),
            at: Utc::now(),
        });

        self.event_sink
            .emit_batch(context.tenant_id, context.session_id, vec![requested])
            .await?;

        let permission_outcome = self
            .permission_authority
            .decide_with_audit(request.clone(), permission_context)
            .await;
        let resolved = Event::PermissionResolved(PermissionResolvedEvent {
            request_id: request.request_id,
            decision: permission_outcome.decision.clone(),
            decided_by: decided_by(&permission_outcome.decided_by),
            scope: plan.scope.clone(),
            fingerprint: Some(fingerprint),
            rationale: None,
            action_plan_hash: plan.plan_hash.clone(),
            decision_id: Default::default(),
            auto_resolved: !matches!(
                permission_outcome.decided_by,
                PermissionAuthorityDecisionSource::Interactive
            ),
            at: Utc::now(),
        });

        if !is_allow_decision(&permission_outcome.decision) {
            self.event_sink
                .emit_batch(context.tenant_id, context.session_id, vec![resolved])
                .await?;
            return Err(ExecutionError::PermissionDenied {
                tool_use_id: plan.tool_use_id,
                decision: permission_outcome.decision,
            });
        }

        let preflight = match sandbox_preflight(
            self.sandbox_backend.as_ref(),
            &plan,
            &self.sandbox_backend.capabilities(),
            context.session_id,
            context.run_id,
        ) {
            Ok(event) => event,
            Err((event, error)) => {
                self.event_sink
                    .emit_batch(context.tenant_id, context.session_id, vec![resolved, event])
                    .await?;
                return Err(error);
            }
        };
        let claims = AuthorizationTicketClaims {
            tenant_id: context.tenant_id,
            session_id: context.session_id,
            run_id: context.run_id,
            tool_use_id: plan.tool_use_id,
            tool_name: plan.tool_name.clone(),
            action_plan_hash: plan.plan_hash.clone(),
        };
        let ticket = self.ticket_ledger.mint(claims, Utc::now())?;

        self.event_sink
            .emit_batch(
                context.tenant_id,
                context.session_id,
                vec![resolved, preflight],
            )
            .await?;

        Ok(AuthorizationOutcome {
            decision: permission_outcome.decision.clone(),
            ticket,
            action_plan_hash: plan.plan_hash,
            sandbox_backend_id: self.sandbox_backend.backend_id().to_owned(),
            audit: AuthorizationAudit {
                permission_decision: permission_outcome.decision,
                permission_source: permission_outcome.decided_by,
                sandbox_preflight: SandboxPreflightStatus::Passed,
            },
        })
    }

    pub async fn authorize_tool_input(
        &self,
        context: AuthorizationContext,
        plan: ToolActionPlan,
        raw_input: serde_json::Value,
    ) -> Result<AuthorizedToolInput, ExecutionError> {
        let outcome = self.authorize_plan(context, plan.clone()).await?;
        let ticket = self.consume_ticket(outcome.ticket)?;
        AuthorizedToolInput::new(raw_input, plan, ticket).map_err(|error| {
            ExecutionError::AuthorizationFailed {
                reason: error.to_string(),
            }
        })
    }

    pub async fn authorize_operation(
        &self,
        context: AuthorizationContext,
        plan: ToolActionPlan,
    ) -> Result<AuthorizedOperation, ExecutionError> {
        let outcome = self.authorize_plan(context, plan).await?;
        let ticket = self.consume_ticket(outcome.ticket)?;
        Ok(AuthorizedOperation {
            ticket,
            action_plan_hash: outcome.action_plan_hash,
            sandbox_backend_id: outcome.sandbox_backend_id,
        })
    }

    fn consume_ticket(
        &self,
        ticket: AuthorizationTicket,
    ) -> Result<AuthorizedTicketSummary, ExecutionError> {
        let consumed = self
            .ticket_ledger
            .consume(ticket.id, &ticket.claims, Utc::now())?;
        Ok(AuthorizedTicketSummary {
            ticket_id: consumed.id,
            tenant_id: consumed.claims.tenant_id,
            session_id: consumed.claims.session_id,
            run_id: consumed.claims.run_id,
            tool_use_id: consumed.claims.tool_use_id,
            tool_name: consumed.claims.tool_name,
            action_plan_hash: consumed.claims.action_plan_hash,
            consumed_at: Utc::now(),
        })
    }
}

fn sandbox_preflight(
    backend: &dyn SandboxBackend,
    plan: &ToolActionPlan,
    capabilities: &SandboxCapabilities,
    session_id: SessionId,
    run_id: RunId,
) -> Result<Event, (Event, ExecutionError)> {
    let backend_id = backend.backend_id().to_owned();
    let effective_policy = effective_sandbox_policy(plan);
    let policy = sandbox_policy_summary(&effective_policy);
    let policy_hash = sandbox_policy_hash(&effective_policy, &backend_id);

    if let Some(reason) = sandbox_preflight_failure(plan, capabilities, &backend_id) {
        let event = Event::SandboxPreflightFailed(SandboxPreflightFailedEvent {
            session_id,
            run_id,
            tool_use_id: Some(plan.tool_use_id),
            backend_id: backend_id.clone(),
            status: SandboxPreflightStatus::Failed,
            policy,
            policy_hash,
            reason: reason.clone(),
            at: Utc::now(),
        });
        return Err((
            event,
            ExecutionError::SandboxPreflightFailed { backend_id, reason },
        ));
    }

    if let Some(spec) = preflight_spec_for_plan(plan) {
        if let Err(error) = backend.preflight_execute(&spec) {
            let reason = error.to_string();
            let event = Event::SandboxPreflightFailed(SandboxPreflightFailedEvent {
                session_id,
                run_id,
                tool_use_id: Some(plan.tool_use_id),
                backend_id: backend_id.clone(),
                status: SandboxPreflightStatus::Failed,
                policy,
                policy_hash,
                reason: reason.clone(),
                at: Utc::now(),
            });
            return Err((
                event,
                ExecutionError::SandboxPreflightFailed { backend_id, reason },
            ));
        }
    }

    Ok(Event::SandboxPreflightPassed(SandboxPreflightPassedEvent {
        session_id,
        run_id,
        tool_use_id: Some(plan.tool_use_id),
        backend_id,
        status: SandboxPreflightStatus::Passed,
        policy,
        policy_hash,
        at: Utc::now(),
    }))
}

fn preflight_spec_for_plan(plan: &ToolActionPlan) -> Option<ExecSpec> {
    let command = plan.resources.iter().find_map(|resource| match resource {
        ActionResource::Command {
            command, argv, cwd, ..
        } => Some((command.clone(), argv.clone(), cwd.clone())),
        _ => None,
    });
    let has_network_resource = plan
        .resources
        .iter()
        .any(|resource| matches!(resource, ActionResource::Network { .. }));
    if command.is_none()
        && !has_network_resource
        && matches!(plan.network_access, NetworkAccess::None)
        && matches!(plan.sandbox_policy.network, NetworkAccess::None)
    {
        return None;
    }
    let (command, args, cwd) =
        command.unwrap_or_else(|| (plan.tool_name.clone(), Vec::new(), None));
    Some(ExecSpec {
        command,
        args,
        cwd,
        policy: effective_sandbox_policy(plan),
        workspace_access: plan.workspace_access.clone(),
        ..ExecSpec::default()
    })
}

fn sandbox_policy_hash(policy: &SandboxPolicy, backend_id: &str) -> SandboxPolicyHash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(b"jyowo.sandbox_policy.v1".len() as u64).to_le_bytes());
    hasher.update(b"jyowo.sandbox_policy.v1");
    hasher.update(&(backend_id.len() as u64).to_le_bytes());
    hasher.update(backend_id.as_bytes());
    let policy_json = serde_json::to_vec(policy).unwrap_or_default();
    hasher.update(&(policy_json.len() as u64).to_le_bytes());
    hasher.update(&policy_json);
    SandboxPolicyHash::from_bytes(*hasher.finalize().as_bytes())
}

fn effective_sandbox_policy(plan: &ToolActionPlan) -> SandboxPolicy {
    let mut policy = plan.sandbox_policy.clone();
    if !matches!(policy.network, NetworkAccess::None) {
        return policy;
    }

    if !matches!(plan.network_access, NetworkAccess::None) {
        policy.network = plan.network_access.clone();
        return policy;
    }

    let allowlist = plan
        .resources
        .iter()
        .filter_map(|resource| match resource {
            ActionResource::Network { host, port } => Some(HostRule {
                pattern: host.clone(),
                ports: port.map(|port| vec![port]),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();
    if !allowlist.is_empty() {
        policy.network = NetworkAccess::AllowList(allowlist);
    }
    policy
}

fn sandbox_preflight_failure(
    plan: &ToolActionPlan,
    capabilities: &SandboxCapabilities,
    backend_id: &str,
) -> Option<String> {
    if let Some(requested_backend) = plan.resources.iter().find_map(|resource| match resource {
        ActionResource::Sandbox { backend_id, .. } => Some(backend_id),
        _ => None,
    }) {
        if requested_backend != backend_id {
            return Some(format!(
                "action plan targets sandbox backend `{requested_backend}`, active backend is `{backend_id}`"
            ));
        }
    }

    if !matches!(plan.network_access, NetworkAccess::None)
        || !matches!(plan.sandbox_policy.network, NetworkAccess::None)
    {
        if !capabilities.supports_network {
            return Some("sandbox backend does not support requested network access".to_owned());
        }
    }

    if matches!(
        plan.workspace_access,
        harness_contracts::WorkspaceAccess::ReadWrite { .. }
    ) && !capabilities.supports_filesystem_write
    {
        return Some("sandbox backend does not support filesystem writes".to_owned());
    }

    unsupported_resource_limit(&plan.sandbox_policy.resource_limits, capabilities)
}

fn unsupported_resource_limit(
    limits: &ResourceLimits,
    capabilities: &SandboxCapabilities,
) -> Option<String> {
    if limits.max_memory_bytes.is_some() && !capabilities.resource_limit_support.memory {
        return Some("sandbox backend does not support memory limits".to_owned());
    }
    if limits.max_cpu_cores.is_some() && !capabilities.resource_limit_support.cpu {
        return Some("sandbox backend does not support cpu limits".to_owned());
    }
    if limits.max_pids.is_some() && !capabilities.resource_limit_support.pids {
        return Some("sandbox backend does not support pid limits".to_owned());
    }
    if limits.max_wall_clock_ms.is_some() && !capabilities.resource_limit_support.wall_clock {
        return Some("sandbox backend does not support wall clock limits".to_owned());
    }
    if limits.max_open_files.is_some() && !capabilities.resource_limit_support.open_files {
        return Some("sandbox backend does not support open file limits".to_owned());
    }
    None
}

fn confirmation_expected(confirmation: &PermissionConfirmation) -> Option<String> {
    match confirmation {
        PermissionConfirmation::TypeToConfirm { expected } => Some(expected.clone()),
        PermissionConfirmation::None | PermissionConfirmation::ExplicitButton { .. } => None,
        _ => None,
    }
}

fn sandbox_policy_summary(policy: &SandboxPolicy) -> SandboxPolicySummary {
    SandboxPolicySummary {
        mode: policy.mode.clone(),
        scope: policy.scope.clone(),
        network: policy.network.clone(),
        resource_limits: policy.resource_limits.clone(),
    }
}

fn is_allow_decision(decision: &Decision) -> bool {
    matches!(
        decision,
        Decision::AllowOnce | Decision::AllowSession | Decision::AllowPermanent
    )
}

fn decided_by(source: &PermissionAuthorityDecisionSource) -> DecidedBy {
    match source {
        PermissionAuthorityDecisionSource::PermissionMode => DecidedBy::DefaultMode,
        PermissionAuthorityDecisionSource::HardPolicy | PermissionAuthorityDecisionSource::Rule => {
            DecidedBy::Rule {
                rule_id: "permission_authority".to_owned(),
            }
        }
        PermissionAuthorityDecisionSource::PersistedDecision { .. }
        | PermissionAuthorityDecisionSource::Dedup { .. }
        | PermissionAuthorityDecisionSource::Interactive
        | PermissionAuthorityDecisionSource::NoInteractive
        | PermissionAuthorityDecisionSource::ScopeMismatch
        | PermissionAuthorityDecisionSource::PersistenceFailed => DecidedBy::Broker {
            broker_id: "permission_authority".to_owned(),
        },
    }
}

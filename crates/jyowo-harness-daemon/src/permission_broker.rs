//! Daemon-owned permission request routing and decision validation.

use std::collections::{hash_map::Entry, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use harness_contracts::{
    ActorId, CommandId, PermissionProjection, PermissionRequestDetails, PermissionRoute,
    RedactRules, Redactor, RequestId, RunSegmentId, RunState, TaskId, TaskProjection,
    WorkspaceLeaseId,
};
pub use harness_contracts::{DaemonPermissionKind, PermissionOption};
use harness_journal::{
    AcceptedCommand, CommandOutcome, CommandRejection, NewTaskEvent, TaskStore, TaskStoreError,
    WorkspaceCommandAuthority,
};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::sync::oneshot;

use jyowo_harness_sdk::ext::{
    Decision, PermissionBroker as EnginePermissionBroker, PermissionContext,
    PermissionError as EnginePermissionError, PermissionRequest as EnginePermissionRequest,
    PermissionSubject,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequestDraft {
    pub task_id: TaskId,
    pub segment_id: RunSegmentId,
    pub request_id: RequestId,
    pub request_revision: u64,
    pub expected_task_version: u64,
    pub kind: DaemonPermissionKind,
    pub action_plan_hash: String,
    pub sandbox_policy_hash: String,
    pub workspace: String,
    pub subject: Value,
    pub actor_source: Value,
    pub options: Vec<PermissionOption>,
    pub preview: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDecisionInput {
    pub task_id: TaskId,
    pub request_id: RequestId,
    pub request_revision: u64,
    pub option_id: String,
    pub expected_task_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRuntimeAuthority {
    pub workspace_lease_id: WorkspaceLeaseId,
    pub actor_id: ActorId,
    pub execution_root: String,
    pub writable: bool,
    pub sandbox_policy_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PermissionValidationContext {
    task_id: TaskId,
    segment_id: RunSegmentId,
    request_revision: u64,
    kind: DaemonPermissionKind,
    action_plan_hash: String,
    sandbox_policy_hash: String,
    workspace: String,
    subject: Value,
    actor_source: Value,
    options: Vec<PermissionOption>,
    expires_at: DateTime<Utc>,
    runtime_authority: Option<PermissionRuntimeAuthority>,
}

impl From<&PermissionRequestDraft> for PermissionValidationContext {
    fn from(draft: &PermissionRequestDraft) -> Self {
        Self {
            task_id: draft.task_id,
            segment_id: draft.segment_id,
            request_revision: draft.request_revision,
            kind: draft.kind,
            action_plan_hash: draft.action_plan_hash.clone(),
            sandbox_policy_hash: draft.sandbox_policy_hash.clone(),
            workspace: draft.workspace.clone(),
            subject: draft.subject.clone(),
            actor_source: draft.actor_source.clone(),
            options: draft.options.clone(),
            expires_at: draft.expires_at,
            runtime_authority: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequestOutcome {
    pub auto_resolved: bool,
    pub committed_offset: u64,
    pub selected_option_id: Option<String>,
}

struct EngineDecisionWaiter {
    options: HashMap<String, Decision>,
    sender: oneshot::Sender<Decision>,
}

pub trait SavedPermissionPolicy: Send + Sync + 'static {
    fn resolve(&self, request: &PermissionRequestDraft) -> Option<String>;
}

struct NoSavedPermissionPolicy;

impl SavedPermissionPolicy for NoSavedPermissionPolicy {
    fn resolve(&self, _request: &PermissionRequestDraft) -> Option<String> {
        None
    }
}

#[derive(Debug, Error)]
pub enum PermissionBrokerError {
    #[error(transparent)]
    Store(#[from] TaskStoreError),
    #[error("permission command was rejected: {0:?}")]
    Rejected(CommandRejection),
    #[error("permission request is invalid: {0}")]
    InvalidRequest(String),
    #[error("permission broker validation state lock was poisoned")]
    ValidationStatePoisoned,
}

#[derive(Clone)]
pub struct PermissionBroker {
    store: Arc<TaskStore>,
    redactor: Arc<dyn Redactor>,
    saved_policy: Arc<dyn SavedPermissionPolicy>,
    validation_contexts: Arc<Mutex<HashMap<RequestId, PermissionValidationContext>>>,
    engine_waiters: Arc<Mutex<HashMap<RequestId, EngineDecisionWaiter>>>,
}

impl PermissionBroker {
    #[must_use]
    pub fn new(store: Arc<TaskStore>, redactor: Arc<dyn Redactor>) -> Self {
        Self {
            store,
            redactor,
            saved_policy: Arc::new(NoSavedPermissionPolicy),
            validation_contexts: Arc::new(Mutex::new(HashMap::new())),
            engine_waiters: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[must_use]
    pub fn with_saved_policy(mut self, saved_policy: Arc<dyn SavedPermissionPolicy>) -> Self {
        self.saved_policy = saved_policy;
        self
    }

    pub fn request(
        &self,
        draft: PermissionRequestDraft,
    ) -> Result<PermissionRequestOutcome, PermissionBrokerError> {
        self.request_with_runtime_authority(draft, None)
    }

    fn request_with_runtime_authority(
        &self,
        draft: PermissionRequestDraft,
        runtime_authority: Option<PermissionRuntimeAuthority>,
    ) -> Result<PermissionRequestOutcome, PermissionBrokerError> {
        validate_request(&draft)?;
        let mut validation_context = PermissionValidationContext::from(&draft);
        validation_context.runtime_authority = runtime_authority;
        let reserved_new_context = {
            let mut validation_contexts = self
                .validation_contexts
                .lock()
                .map_err(|_| PermissionBrokerError::ValidationStatePoisoned)?;
            match validation_contexts.entry(draft.request_id) {
                Entry::Occupied(entry) if entry.get() != &validation_context => {
                    return Err(PermissionBrokerError::InvalidRequest(
                        "request id was reused for a different permission context".into(),
                    ));
                }
                Entry::Occupied(_) => {
                    return Err(PermissionBrokerError::InvalidRequest(
                        "permission request is already in flight".into(),
                    ));
                }
                Entry::Vacant(entry) => {
                    entry.insert(validation_context.clone());
                    true
                }
            }
        };
        let saved_option_id = self.saved_policy.resolve(&draft);
        if let Some(option_id) = saved_option_id.as_ref() {
            if let Err(error) = require_option(&draft.options, option_id) {
                self.release_validation_context(
                    draft.request_id,
                    &validation_context,
                    reserved_new_context,
                )?;
                return Err(error);
            }
        }
        let projection = self.redacted_projection(
            &draft,
            if saved_option_id.is_some() {
                PermissionRoute::SavedPolicy
            } else {
                PermissionRoute::ForegroundTask
            },
        );
        let segment_id = draft.segment_id;
        let request_id = draft.request_id;
        let request_revision = draft.request_revision;
        let auto_resolved = saved_option_id.is_some();
        let mut expected_stream_version = draft.expected_task_version;
        let mut retries_remaining = 8_u8;
        let outcome = loop {
            let command = AcceptedCommand {
                command_id: CommandId::new(),
                task_id: draft.task_id,
                idempotency_key: format!(
                    "permission-request:{}:{expected_stream_version}",
                    draft.request_id
                ),
                expected_stream_version,
                authority: TaskStore::permission_broker_authority(),
                payload: json!({
                    "type": "permission_request",
                    "permission": projection,
                    "savedOptionId": saved_option_id,
                }),
            };
            let projection = projection.clone();
            let decide = |task: &TaskProjection| {
                if validation_context.expires_at <= Utc::now() {
                    return Err(invalid_command("permission request has expired"));
                }
                if task.pending_permission.is_some()
                    || !task.current_run.as_ref().is_some_and(|run| {
                        run.segment_id == segment_id && run.state == RunState::Running
                    })
                {
                    return Err(invalid_command(
                        "permission request requires the current running segment",
                    ));
                }
                let mut events = vec![NewTaskEvent::permission_requested(projection)];
                if auto_resolved {
                    events.push(NewTaskEvent::permission_resolved_with_option(
                        request_id,
                        request_revision,
                        saved_option_id
                            .as_deref()
                            .expect("auto resolution has an option"),
                    ));
                }
                Ok(events)
            };
            let workspace_authority =
                validation_context
                    .runtime_authority
                    .as_ref()
                    .map(|authority| WorkspaceCommandAuthority {
                        lease_id: authority.workspace_lease_id,
                        task_id: draft.task_id,
                        actor_id: authority.actor_id,
                        execution_root: authority.execution_root.clone(),
                        writable: authority.writable,
                    });
            let result = self.store.transact_permission_request(
                command,
                request_id,
                workspace_authority,
                decide,
            );
            match result {
                Ok(CommandOutcome::Rejected {
                    rejection: CommandRejection::WrongExpectedVersion { actual, .. },
                    ..
                }) if retries_remaining > 0 => {
                    expected_stream_version = actual;
                    retries_remaining -= 1;
                }
                Ok(outcome) => break outcome,
                Err(error) => {
                    self.release_validation_context(
                        request_id,
                        &validation_context,
                        reserved_new_context,
                    )?;
                    return Err(error.into());
                }
            }
        };
        let (committed_offset, _) = match require_accepted(outcome) {
            Ok(accepted) => accepted,
            Err(error) => {
                self.release_validation_context(
                    request_id,
                    &validation_context,
                    reserved_new_context,
                )?;
                return Err(error);
            }
        };
        if auto_resolved {
            self.remove_validation_context_after_commit(request_id);
        }
        Ok(PermissionRequestOutcome {
            auto_resolved,
            committed_offset,
            selected_option_id: saved_option_id,
        })
    }

    fn release_validation_context(
        &self,
        request_id: RequestId,
        context: &PermissionValidationContext,
        reserved_new_context: bool,
    ) -> Result<(), PermissionBrokerError> {
        if reserved_new_context {
            let mut validation_contexts = self
                .validation_contexts
                .lock()
                .map_err(|_| PermissionBrokerError::ValidationStatePoisoned)?;
            if validation_contexts.get(&request_id) == Some(context) {
                validation_contexts.remove(&request_id);
            }
        }
        Ok(())
    }

    pub fn resolve(
        &self,
        input: PermissionDecisionInput,
    ) -> Result<CommandOutcome, PermissionBrokerError> {
        let command = AcceptedCommand {
            command_id: CommandId::new(),
            task_id: input.task_id,
            idempotency_key: format!(
                "permission-resolve:{}:{}:{}",
                input.request_id, input.request_revision, input.option_id
            ),
            expected_stream_version: input.expected_task_version,
            authority: TaskStore::permission_broker_authority(),
            payload: json!({
                "type": "permission_resolve",
                "requestId": input.request_id,
                "requestRevision": input.request_revision,
                "optionId": input.option_id,
            }),
        };
        self.resolve_with_command(command, input)
    }

    pub fn resolve_client_command(
        &self,
        mut command: AcceptedCommand,
        input: PermissionDecisionInput,
    ) -> Result<CommandOutcome, PermissionBrokerError> {
        let command_id = command.command_id;
        let task_id = command.task_id;
        command.authority = TaskStore::permission_broker_command_authority(&command.authority);
        match self.resolve_with_command(command, input) {
            Ok(outcome) => Ok(outcome),
            Err(PermissionBrokerError::Rejected(rejection)) => Ok(CommandOutcome::Rejected {
                command_id,
                task_id,
                rejection,
            }),
            Err(PermissionBrokerError::Store(
                TaskStoreError::CommandConflict { .. } | TaskStoreError::InvalidInput(_),
            )) => Ok(CommandOutcome::Rejected {
                command_id,
                task_id,
                rejection: invalid_command("permission command conflicts with durable input"),
            }),
            Err(error) => Err(error),
        }
    }

    fn resolve_with_command(
        &self,
        mut command: AcceptedCommand,
        input: PermissionDecisionInput,
    ) -> Result<CommandOutcome, PermissionBrokerError> {
        if command.task_id != input.task_id
            || command.expected_stream_version != input.expected_task_version
        {
            return Err(PermissionBrokerError::Rejected(invalid_command(
                "permission command metadata does not match its decision",
            )));
        }
        command.payload = json!({
            "type": "permission_resolve",
            "requestId": input.request_id,
            "requestRevision": input.request_revision,
            "optionId": input.option_id,
        });
        self.validate_engine_waiter_option(input.request_id, &input.option_id)?;
        let context = self
            .validation_contexts
            .lock()
            .map_err(|_| PermissionBrokerError::ValidationStatePoisoned)?
            .get(&input.request_id)
            .cloned();
        let decide = |task: &TaskProjection| {
            let context = context.as_ref().ok_or_else(|| {
                invalid_command("permission request has no live daemon validation context")
            })?;
            if context.task_id != input.task_id
                || context.request_revision != input.request_revision
            {
                return Err(invalid_command("permission request identity is stale"));
            }
            if Utc::now() > context.expires_at {
                return Err(invalid_command("permission request has expired"));
            }
            require_option_for_decision(&context.options, &input.option_id)?;
            let redacted_workspace = self
                .redactor
                .redact(&context.workspace, &RedactRules::default());
            let redacted_subject = redact_value(self.redactor.as_ref(), &context.subject);
            let redacted_actor_source = redact_value(self.redactor.as_ref(), &context.actor_source);
            let redacted_options = context
                .options
                .iter()
                .map(|option| PermissionOption {
                    option_id: option.option_id.clone(),
                    label: self.redactor.redact(&option.label, &RedactRules::default()),
                })
                .collect::<Vec<_>>();
            let pending = task
                .pending_permission
                .as_ref()
                .ok_or_else(|| invalid_command("permission request is no longer pending"))?;
            if pending.request_id != input.request_id || pending.revision != input.request_revision
            {
                return Err(invalid_command("permission request identity is stale"));
            }
            let details = pending.details.as_ref().ok_or_else(|| {
                invalid_command("permission request lacks daemon validation metadata")
            })?;
            if Utc::now() > details.expires_at || details.expires_at != context.expires_at {
                return Err(invalid_command("permission request has expired"));
            }
            if details.kind != context.kind
                || details.segment_id != context.segment_id
                || details.action_plan_hash != context.action_plan_hash
                || details.sandbox_policy_hash != context.sandbox_policy_hash
                || details.workspace != redacted_workspace
                || details.subject != redacted_subject
                || details.actor_source != redacted_actor_source
                || details.options != redacted_options
            {
                return Err(invalid_command("permission decision context changed"));
            }
            Ok(vec![NewTaskEvent::permission_resolved_with_option(
                input.request_id,
                input.request_revision,
                &input.option_id,
            )])
        };
        let outcome = if let Some(authority) = context
            .as_ref()
            .and_then(|context| context.runtime_authority.as_ref())
        {
            self.store.transact_command_with_workspace_authority(
                command,
                WorkspaceCommandAuthority {
                    lease_id: authority.workspace_lease_id,
                    task_id: input.task_id,
                    actor_id: authority.actor_id,
                    execution_root: authority.execution_root.clone(),
                    writable: authority.writable,
                },
                decide,
            )
        } else {
            self.store.transact_command(command, decide)
        }?;
        if matches!(outcome, CommandOutcome::Accepted { .. }) {
            self.remove_validation_context_after_commit(input.request_id);
            self.complete_engine_waiter(input.request_id, &input.option_id);
        }
        require_accepted(outcome.clone())?;
        Ok(outcome)
    }

    pub fn invalidate(
        &self,
        task_id: TaskId,
        request_id: RequestId,
        request_revision: u64,
        expected_task_version: u64,
        reason: impl Into<String>,
    ) -> Result<CommandOutcome, PermissionBrokerError> {
        let reason = self
            .redactor
            .redact(&reason.into(), &RedactRules::default());
        let command = AcceptedCommand {
            command_id: CommandId::new(),
            task_id,
            idempotency_key: format!(
                "permission-invalidate:{request_id}:{request_revision}:{expected_task_version}"
            ),
            expected_stream_version: expected_task_version,
            authority: TaskStore::permission_broker_authority(),
            payload: json!({
                "type": "permission_invalidate",
                "requestId": request_id,
                "requestRevision": request_revision,
                "reason": reason,
            }),
        };
        let outcome = self.store.transact_command(command, |task| {
            let pending = task
                .pending_permission
                .as_ref()
                .ok_or_else(|| invalid_command("permission request is no longer pending"))?;
            if pending.request_id != request_id || pending.revision != request_revision {
                return Err(invalid_command("permission request identity is stale"));
            }
            Ok(vec![NewTaskEvent::permission_invalidated(
                request_id,
                request_revision,
                reason,
            )])
        })?;
        if matches!(outcome, CommandOutcome::Accepted { .. }) {
            self.remove_validation_context_after_commit(request_id);
            self.cancel_engine_waiter(request_id);
        }
        require_accepted(outcome.clone())?;
        Ok(outcome)
    }

    fn invalidate_runtime_request(
        &self,
        task_id: TaskId,
        request_id: RequestId,
        request_revision: u64,
        reason: impl Into<String>,
    ) -> Result<CommandOutcome, PermissionBrokerError> {
        let reason = reason.into();
        let mut expected_task_version = self.store.stream_version(task_id)?;
        let mut retries_remaining = 8_u8;
        loop {
            match self.invalidate(
                task_id,
                request_id,
                request_revision,
                expected_task_version,
                reason.clone(),
            ) {
                Err(PermissionBrokerError::Rejected(CommandRejection::WrongExpectedVersion {
                    actual,
                    ..
                })) if retries_remaining > 0 => {
                    expected_task_version = actual;
                    retries_remaining -= 1;
                }
                result => return result,
            }
        }
    }

    pub(crate) fn transact_invalidating_command<F>(
        &self,
        command: AcceptedCommand,
        request_id: RequestId,
        request_revision: u64,
        reason: impl Into<String>,
        decide: F,
    ) -> Result<CommandOutcome, PermissionBrokerError>
    where
        F: FnOnce(&TaskProjection) -> Result<Vec<NewTaskEvent>, CommandRejection>,
    {
        let reason = self
            .redactor
            .redact(&reason.into(), &RedactRules::default());
        let outcome = self.store.transact_command(command, |task| {
            let pending = task
                .pending_permission
                .as_ref()
                .ok_or_else(|| invalid_command("permission request is no longer pending"))?;
            if pending.request_id != request_id || pending.revision != request_revision {
                return Err(invalid_command("permission request identity is stale"));
            }
            let mut events = vec![NewTaskEvent::permission_invalidated(
                request_id,
                request_revision,
                reason,
            )];
            events.extend(decide(task)?);
            Ok(events)
        })?;
        if matches!(outcome, CommandOutcome::Accepted { .. }) {
            self.remove_validation_context_after_commit(request_id);
            self.cancel_engine_waiter(request_id);
        }
        Ok(outcome)
    }

    fn redacted_projection(
        &self,
        draft: &PermissionRequestDraft,
        route: PermissionRoute,
    ) -> PermissionProjection {
        PermissionProjection {
            request_id: draft.request_id,
            revision: draft.request_revision,
            route,
            details: Some(PermissionRequestDetails {
                kind: draft.kind,
                segment_id: draft.segment_id,
                action_plan_hash: draft.action_plan_hash.clone(),
                sandbox_policy_hash: draft.sandbox_policy_hash.clone(),
                workspace: self
                    .redactor
                    .redact(&draft.workspace, &RedactRules::default()),
                subject: redact_value(self.redactor.as_ref(), &draft.subject),
                actor_source: redact_value(self.redactor.as_ref(), &draft.actor_source),
                options: draft
                    .options
                    .iter()
                    .map(|option| PermissionOption {
                        option_id: option.option_id.clone(),
                        label: self.redactor.redact(&option.label, &RedactRules::default()),
                    })
                    .collect(),
                preview: self
                    .redactor
                    .redact(&draft.preview, &RedactRules::default()),
                expires_at: draft.expires_at,
            }),
        }
    }

    fn register_engine_waiter(
        &self,
        request_id: RequestId,
        options: HashMap<String, Decision>,
        sender: oneshot::Sender<Decision>,
    ) -> Result<(), PermissionBrokerError> {
        let mut waiters = self
            .engine_waiters
            .lock()
            .map_err(|_| PermissionBrokerError::ValidationStatePoisoned)?;
        match waiters.entry(request_id) {
            Entry::Vacant(entry) => {
                entry.insert(EngineDecisionWaiter { options, sender });
                Ok(())
            }
            Entry::Occupied(_) => Err(PermissionBrokerError::InvalidRequest(
                "permission request already has an engine waiter".into(),
            )),
        }
    }

    fn validate_engine_waiter_option(
        &self,
        request_id: RequestId,
        option_id: &str,
    ) -> Result<(), PermissionBrokerError> {
        let waiters = self
            .engine_waiters
            .lock()
            .map_err(|_| PermissionBrokerError::ValidationStatePoisoned)?;
        if waiters
            .get(&request_id)
            .is_some_and(|waiter| !waiter.options.contains_key(option_id))
        {
            return Err(PermissionBrokerError::InvalidRequest(
                "resolved option has no matching engine decision".into(),
            ));
        }
        Ok(())
    }

    fn remove_validation_context_after_commit(&self, request_id: RequestId) {
        self.validation_contexts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&request_id);
    }

    fn complete_engine_waiter(&self, request_id: RequestId, option_id: &str) {
        let waiter = self
            .engine_waiters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&request_id);
        if let Some(mut waiter) = waiter {
            if let Some(decision) = waiter.options.remove(option_id) {
                let _ = waiter.sender.send(decision);
            }
        }
    }

    fn cancel_engine_waiter(&self, request_id: RequestId) {
        if let Some(waiter) = self
            .engine_waiters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&request_id)
        {
            let _ = waiter.sender.send(Decision::DenyOnce);
        }
    }
}

#[derive(Clone)]
pub struct HarnessPermissionBroker {
    daemon: Arc<PermissionBroker>,
    task_id: TaskId,
    segment_id: RunSegmentId,
    runtime_authority: PermissionRuntimeAuthority,
}

impl HarnessPermissionBroker {
    #[must_use]
    pub fn new(
        daemon: Arc<PermissionBroker>,
        task_id: TaskId,
        segment_id: RunSegmentId,
        runtime_authority: PermissionRuntimeAuthority,
    ) -> Self {
        Self {
            daemon,
            task_id,
            segment_id,
            runtime_authority,
        }
    }
}

#[async_trait::async_trait]
impl EnginePermissionBroker for HarnessPermissionBroker {
    async fn decide(&self, request: EnginePermissionRequest, ctx: PermissionContext) -> Decision {
        let decision_options = request
            .decision_options
            .iter()
            .filter(|option| {
                !option.requires_confirmation
                    && !(request.confirmation_expected.is_some()
                        && is_allow_decision(&option.decision))
            })
            .cloned()
            .collect::<Vec<_>>();
        let options = decision_options
            .iter()
            .map(|option| (option.option_id.to_string(), option.decision.clone()))
            .collect::<HashMap<_, _>>();
        if options.is_empty() {
            return Decision::DenyOnce;
        }
        let (sender, receiver) = oneshot::channel();
        if self
            .daemon
            .register_engine_waiter(request.request_id, options.clone(), sender)
            .is_err()
        {
            return Decision::DenyOnce;
        }
        let expires_at = request_expiry(&ctx);
        let draft = PermissionRequestDraft {
            task_id: self.task_id,
            segment_id: self.segment_id,
            request_id: request.request_id,
            request_revision: 1,
            expected_task_version: match self.daemon.store.stream_version(self.task_id) {
                Ok(version) => version,
                Err(_) => {
                    self.daemon.cancel_engine_waiter(request.request_id);
                    return Decision::DenyOnce;
                }
            },
            kind: permission_kind(&request.subject),
            action_plan_hash: request.action_plan_hash.to_hex(),
            sandbox_policy_hash: self.runtime_authority.sandbox_policy_hash.clone(),
            workspace: self.runtime_authority.execution_root.clone(),
            subject: serde_json::to_value(&request.subject).unwrap_or(Value::Null),
            actor_source: json!({
                "type": if ctx.run_id.is_some() { "parent_run" } else { "runtime" },
                "sessionId": ctx.session_id,
                "runId": ctx.run_id,
                "actorId": self.runtime_authority.actor_id,
                "workspaceLeaseId": self.runtime_authority.workspace_lease_id,
            }),
            options: decision_options
                .iter()
                .map(|option| PermissionOption {
                    option_id: option.option_id.to_string(),
                    label: option.label.clone(),
                })
                .collect(),
            preview: permission_preview(&request.subject),
            expires_at,
        };
        match self
            .daemon
            .request_with_runtime_authority(draft, Some(self.runtime_authority.clone()))
        {
            Ok(PermissionRequestOutcome {
                selected_option_id: Some(option_id),
                ..
            }) => {
                self.daemon
                    .complete_engine_waiter(request.request_id, &option_id);
            }
            Ok(_) => {}
            Err(_) => {
                self.daemon.cancel_engine_waiter(request.request_id);
                return Decision::DenyOnce;
            }
        }
        let wait = (expires_at - Utc::now())
            .to_std()
            .unwrap_or_else(|_| std::time::Duration::from_secs(0));
        let mut receiver = receiver;
        match tokio::time::timeout(wait, &mut receiver).await {
            Ok(Ok(decision)) => decision,
            _ => {
                let invalidation = self.daemon.invalidate_runtime_request(
                    self.task_id,
                    request.request_id,
                    1,
                    "permission request expired while waiting for a client decision",
                );
                if invalidation.is_err() {
                    if let Ok(Some(option_id)) = self.daemon.store.permission_resolution_option(
                        self.task_id,
                        request.request_id,
                        1,
                    ) {
                        self.daemon
                            .complete_engine_waiter(request.request_id, &option_id);
                        return receiver.try_recv().unwrap_or(Decision::DenyOnce);
                    }
                }
                self.daemon.cancel_engine_waiter(request.request_id);
                Decision::DenyOnce
            }
        }
    }

    async fn persist(
        &self,
        _decision: jyowo_harness_sdk::ext::PersistedDecision,
    ) -> Result<(), EnginePermissionError> {
        Ok(())
    }
}

fn request_expiry(ctx: &PermissionContext) -> DateTime<Utc> {
    let timeout = ctx
        .timeout_policy
        .as_ref()
        .map_or(std::time::Duration::from_secs(300), |policy| {
            std::time::Duration::from_millis(policy.deadline_ms.max(1))
        });
    Utc::now()
        + chrono::Duration::from_std(timeout).unwrap_or_else(|_| chrono::Duration::minutes(5))
}

fn permission_kind(subject: &PermissionSubject) -> DaemonPermissionKind {
    match subject {
        PermissionSubject::CommandExec { .. } | PermissionSubject::DangerousCommand { .. } => {
            DaemonPermissionKind::Command
        }
        PermissionSubject::FileWrite { .. } | PermissionSubject::FileDelete { .. } => {
            DaemonPermissionKind::Filesystem
        }
        PermissionSubject::NetworkAccess { .. } => DaemonPermissionKind::Network,
        PermissionSubject::McpToolCall { .. } => DaemonPermissionKind::Mcp,
        PermissionSubject::ToolInvocation { tool, .. } if tool.contains("automation") => {
            DaemonPermissionKind::Automation
        }
        PermissionSubject::ToolInvocation { .. } | PermissionSubject::Custom { .. } => {
            DaemonPermissionKind::Command
        }
        _ => DaemonPermissionKind::Command,
    }
}

fn permission_preview(subject: &PermissionSubject) -> String {
    match subject {
        PermissionSubject::CommandExec { command, .. }
        | PermissionSubject::DangerousCommand { command, .. } => command.clone(),
        PermissionSubject::FileWrite { path, .. } | PermissionSubject::FileDelete { path } => {
            path.to_string_lossy().into_owned()
        }
        PermissionSubject::NetworkAccess { host, port } => port
            .map(|port| format!("{host}:{port}"))
            .unwrap_or_else(|| host.clone()),
        PermissionSubject::McpToolCall { server, tool, .. } => format!("{server}/{tool}"),
        PermissionSubject::ToolInvocation { tool, .. } => tool.clone(),
        PermissionSubject::Custom { kind, .. } => kind.clone(),
        _ => "permission request".into(),
    }
}

fn is_allow_decision(decision: &Decision) -> bool {
    matches!(
        decision,
        Decision::AllowOnce | Decision::AllowSession | Decision::AllowPermanent
    )
}

fn validate_request(draft: &PermissionRequestDraft) -> Result<(), PermissionBrokerError> {
    if draft.request_revision == 0 {
        return Err(PermissionBrokerError::InvalidRequest(
            "request revision must be positive".into(),
        ));
    }
    if draft.expires_at <= Utc::now() {
        return Err(PermissionBrokerError::InvalidRequest(
            "request is already expired".into(),
        ));
    }
    if draft.options.is_empty() {
        return Err(PermissionBrokerError::InvalidRequest(
            "request must present at least one option".into(),
        ));
    }
    let mut option_ids = HashSet::with_capacity(draft.options.len());
    if draft
        .options
        .iter()
        .any(|option| option.option_id.is_empty() || !option_ids.insert(option.option_id.as_str()))
    {
        return Err(PermissionBrokerError::InvalidRequest(
            "permission option ids must be non-empty and unique".into(),
        ));
    }
    Ok(())
}

fn require_option(
    options: &[PermissionOption],
    option_id: &str,
) -> Result<(), PermissionBrokerError> {
    if options.iter().any(|option| option.option_id == option_id) {
        Ok(())
    } else {
        Err(PermissionBrokerError::InvalidRequest(
            "saved policy selected an option that was not presented".into(),
        ))
    }
}

fn require_option_for_decision(
    options: &[PermissionOption],
    option_id: &str,
) -> Result<(), CommandRejection> {
    if options.iter().any(|option| option.option_id == option_id) {
        Ok(())
    } else {
        Err(invalid_command(
            "permission option is not valid for this request",
        ))
    }
}

fn redact_value(redactor: &dyn Redactor, value: &Value) -> Value {
    match value {
        Value::String(value) => Value::String(redactor.redact(value, &RedactRules::default())),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| redact_value(redactor, value))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| {
                    (
                        redactor.redact(key, &RedactRules::default()),
                        redact_value(redactor, value),
                    )
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn invalid_command(message: impl Into<String>) -> CommandRejection {
    CommandRejection::InvalidCommand {
        message: message.into(),
    }
}

fn require_accepted(outcome: CommandOutcome) -> Result<(u64, u64), PermissionBrokerError> {
    match outcome {
        CommandOutcome::Accepted {
            stream_version,
            committed_offset,
            ..
        } => Ok((committed_offset, stream_version)),
        CommandOutcome::Rejected { rejection, .. } => {
            Err(PermissionBrokerError::Rejected(rejection))
        }
    }
}

#[cfg(test)]
mod lock_order_tests {
    use std::collections::HashMap;
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;
    use std::time::Duration as StdDuration;

    use chrono::{Duration, Utc};
    use harness_contracts::{
        ActionPlanHash, ActorId, CommandId, DecisionLifetime, DecisionMatcherKind,
        DecisionMatcherSummary, NoopRedactor, PermissionDecisionOption, PermissionOptionId,
        RequestId, RunSegmentId, SessionId, TaskId, TimeoutPolicy, WorkspaceLeaseId, WorkspaceMode,
    };
    use harness_journal::{
        AcceptedCommand, AcquireTaskWorkspaceLease, CommandOutcome, NewTaskEvent, TaskStore,
        TaskWorkspaceAcquireOutcome,
    };
    use rusqlite::Connection;
    use serde_json::json;
    use tokio::sync::oneshot;

    use jyowo_harness_sdk::ext::{
        Decision, DecisionScope, FallbackPolicy, InteractivityLevel,
        PermissionBroker as EnginePermissionBroker, PermissionContext, PermissionMode,
        PermissionRequest as EnginePermissionRequest, PermissionSubject, Severity, TenantId,
        ToolUseId,
    };

    use super::{
        DaemonPermissionKind, HarnessPermissionBroker, PermissionBroker, PermissionDecisionInput,
        PermissionOption, PermissionRequestDraft, PermissionRuntimeAuthority,
        SavedPermissionPolicy,
    };

    struct BlockingPolicy {
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }

    impl SavedPermissionPolicy for BlockingPolicy {
        fn resolve(&self, _request: &PermissionRequestDraft) -> Option<String> {
            self.entered.send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
            None
        }
    }

    #[test]
    fn permission_request_does_not_hold_validation_state_while_policy_runs() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        let task_id = TaskId::new();
        let segment_id = RunSegmentId::new();
        let created = store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: "create-policy-lock-task".into(),
                    expected_stream_version: 0,
                    authority: TaskStore::supervisor_authority(),
                    payload: json!({ "type": "create" }),
                },
                |_| {
                    Ok(vec![
                        NewTaskEvent::task_created("policy lock"),
                        NewTaskEvent::run_started(segment_id, Utc::now()),
                    ])
                },
            )
            .unwrap();
        assert!(matches!(created, CommandOutcome::Accepted { .. }));

        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let broker = PermissionBroker::new(Arc::clone(&store), Arc::new(NoopRedactor))
            .with_saved_policy(Arc::new(BlockingPolicy {
                entered: entered_tx,
                release: Mutex::new(release_rx),
            }));
        let requester = broker.clone();
        let request_thread = thread::spawn(move || {
            requester.request(PermissionRequestDraft {
                task_id,
                segment_id,
                request_id: RequestId::new(),
                request_revision: 1,
                expected_task_version: store.stream_version(task_id).unwrap(),
                kind: DaemonPermissionKind::Command,
                action_plan_hash: "plan".into(),
                sandbox_policy_hash: "sandbox".into(),
                workspace: "/workspace".into(),
                subject: json!({ "command": "cargo test" }),
                actor_source: json!({ "type": "parent_run" }),
                options: vec![PermissionOption {
                    option_id: "allow-once".into(),
                    label: "Allow once".into(),
                }],
                preview: "cargo test".into(),
                expires_at: Utc::now() + Duration::minutes(5),
            })
        });
        entered_rx.recv().unwrap();

        let validation_state_is_available = broker.validation_contexts.try_lock().is_ok();
        release_tx.send(()).unwrap();
        assert!(request_thread.join().unwrap().is_ok());

        assert!(
            validation_state_is_available,
            "saved policy evaluation held the global validation-state lock"
        );
    }

    #[test]
    fn permission_resolution_does_not_hold_the_store_while_waiting_for_validation_state() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        let task_id = TaskId::new();
        let segment_id = RunSegmentId::new();
        let created = store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: "create-lock-order-task".into(),
                    expected_stream_version: 0,
                    authority: TaskStore::supervisor_authority(),
                    payload: json!({ "type": "create" }),
                },
                |_| {
                    Ok(vec![
                        NewTaskEvent::task_created("lock order"),
                        NewTaskEvent::run_started(segment_id, Utc::now()),
                    ])
                },
            )
            .unwrap();
        assert!(matches!(created, CommandOutcome::Accepted { .. }));

        let broker = PermissionBroker::new(Arc::clone(&store), Arc::new(NoopRedactor));
        let request_id = RequestId::new();
        broker
            .request(PermissionRequestDraft {
                task_id,
                segment_id,
                request_id,
                request_revision: 1,
                expected_task_version: store.stream_version(task_id).unwrap(),
                kind: DaemonPermissionKind::Command,
                action_plan_hash: "plan".into(),
                sandbox_policy_hash: "sandbox".into(),
                workspace: "/workspace".into(),
                subject: json!({ "command": "cargo test" }),
                actor_source: json!({ "type": "parent_run" }),
                options: vec![PermissionOption {
                    option_id: "allow-once".into(),
                    label: "Allow once".into(),
                }],
                preview: "cargo test".into(),
                expires_at: Utc::now() + Duration::minutes(5),
            })
            .unwrap();

        let validation_guard = broker.validation_contexts.lock().unwrap();
        let resolver = broker.clone();
        let expected_task_version = store.stream_version(task_id).unwrap();
        let (resolver_started_tx, resolver_started_rx) = mpsc::channel();
        let resolver_thread = thread::spawn(move || {
            resolver_started_tx.send(()).unwrap();
            resolver.resolve(PermissionDecisionInput {
                task_id,
                request_id,
                request_revision: 1,
                option_id: "allow-once".into(),
                expected_task_version,
            })
        });
        resolver_started_rx.recv().unwrap();
        thread::sleep(StdDuration::from_millis(100));

        let reader_store = Arc::clone(&store);
        let (reader_tx, reader_rx) = mpsc::channel();
        let reader_thread = thread::spawn(move || {
            reader_tx
                .send(reader_store.stream_version(task_id))
                .unwrap();
        });
        let store_was_available = reader_rx.recv_timeout(StdDuration::from_millis(200));

        drop(validation_guard);
        let resolution = resolver_thread.join().unwrap();
        reader_thread.join().unwrap();

        assert!(
            store_was_available.is_ok(),
            "permission resolution acquired TaskStore before validation state"
        );
        assert!(resolution.is_ok());
    }

    #[test]
    fn permission_resolution_does_not_hold_validation_state_while_sqlite_is_busy() {
        let root = tempfile::tempdir().unwrap();
        let database_path = root.path().join("tasks.sqlite");
        let store = Arc::new(TaskStore::open(&database_path).unwrap());
        let task_id = TaskId::new();
        let segment_id = RunSegmentId::new();
        let created = store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: "create-sqlite-lock-task".into(),
                    expected_stream_version: 0,
                    authority: TaskStore::supervisor_authority(),
                    payload: json!({ "type": "create" }),
                },
                |_| {
                    Ok(vec![
                        NewTaskEvent::task_created("sqlite lock"),
                        NewTaskEvent::run_started(segment_id, Utc::now()),
                    ])
                },
            )
            .unwrap();
        assert!(matches!(created, CommandOutcome::Accepted { .. }));

        let broker = PermissionBroker::new(Arc::clone(&store), Arc::new(NoopRedactor));
        let request_id = RequestId::new();
        broker
            .request(PermissionRequestDraft {
                task_id,
                segment_id,
                request_id,
                request_revision: 1,
                expected_task_version: store.stream_version(task_id).unwrap(),
                kind: DaemonPermissionKind::Command,
                action_plan_hash: "plan".into(),
                sandbox_policy_hash: "sandbox".into(),
                workspace: "/workspace".into(),
                subject: json!({ "command": "cargo test" }),
                actor_source: json!({ "type": "parent_run" }),
                options: vec![PermissionOption {
                    option_id: "allow-once".into(),
                    label: "Allow once".into(),
                }],
                preview: "cargo test".into(),
                expires_at: Utc::now() + Duration::minutes(5),
            })
            .unwrap();

        let sqlite_writer = Connection::open(&database_path).unwrap();
        sqlite_writer
            .execute_batch("PRAGMA busy_timeout = 5000; BEGIN IMMEDIATE;")
            .unwrap();
        let resolver = broker.clone();
        let expected_task_version = store.stream_version(task_id).unwrap();
        let (resolver_started_tx, resolver_started_rx) = mpsc::channel();
        let resolver_thread = thread::spawn(move || {
            resolver_started_tx.send(()).unwrap();
            resolver.resolve(PermissionDecisionInput {
                task_id,
                request_id,
                request_revision: 1,
                option_id: "allow-once".into(),
                expected_task_version,
            })
        });
        resolver_started_rx.recv().unwrap();
        thread::sleep(StdDuration::from_millis(100));

        let validation_state_is_available = broker.validation_contexts.try_lock().is_ok();
        sqlite_writer.execute_batch("COMMIT;").unwrap();
        assert!(resolver_thread.join().unwrap().is_ok());

        assert!(
            validation_state_is_available,
            "SQLite transaction wait held the global validation-state lock"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    #[allow(clippy::await_holding_lock)]
    async fn committed_resolution_wins_if_timeout_fires_before_waiter_notification() {
        let root = tempfile::tempdir().unwrap();
        let database_path = root.path().join("tasks.sqlite");
        let store = Arc::new(TaskStore::open(&database_path).unwrap());
        let task_id = TaskId::new();
        let segment_id = RunSegmentId::new();
        store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: "create-timeout-notification-race-task".into(),
                    expected_stream_version: 0,
                    authority: TaskStore::supervisor_authority(),
                    payload: json!({ "type": "create" }),
                },
                |_| {
                    Ok(vec![
                        NewTaskEvent::task_created("timeout notification race"),
                        NewTaskEvent::run_started(segment_id, Utc::now()),
                    ])
                },
            )
            .unwrap();

        let broker = Arc::new(PermissionBroker::new(
            Arc::clone(&store),
            Arc::new(NoopRedactor),
        ));
        let engine = HarnessPermissionBroker::new(
            Arc::clone(&broker),
            task_id,
            segment_id,
            runtime_authority(&store, task_id),
        );
        let request = engine_request(task_id);
        let request_id = request.request_id;
        let allow_option = request.decision_options[0].option_id;
        let context = engine_context(&request, segment_id, 150);
        let decision_task = tokio::spawn(async move { engine.decide(request, context).await });

        tokio::time::timeout(StdDuration::from_secs(1), async {
            loop {
                if store
                    .task_projection(task_id)
                    .unwrap()
                    .unwrap()
                    .pending_permission
                    .as_ref()
                    .is_some_and(|pending| pending.request_id == request_id)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        let sqlite_writer = Connection::open(&database_path).unwrap();
        sqlite_writer
            .execute_batch("PRAGMA busy_timeout = 5000; BEGIN IMMEDIATE;")
            .unwrap();
        let resolver = Arc::clone(&broker);
        let expected_task_version = store.stream_version(task_id).unwrap();
        let resolver_thread = thread::spawn(move || {
            resolver.resolve(PermissionDecisionInput {
                task_id,
                request_id,
                request_revision: 1,
                option_id: allow_option.to_string(),
                expected_task_version,
            })
        });
        thread::sleep(StdDuration::from_millis(30));
        let validation_guard = broker.validation_contexts.lock().unwrap();
        sqlite_writer.execute_batch("COMMIT;").unwrap();

        tokio::time::timeout(StdDuration::from_secs(1), async {
            loop {
                if store
                    .task_projection(task_id)
                    .unwrap()
                    .unwrap()
                    .pending_permission
                    .is_none()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        tokio::time::sleep(StdDuration::from_millis(200)).await;
        drop(validation_guard);
        assert!(resolver_thread.join().unwrap().is_ok());
        let decision = tokio::time::timeout(StdDuration::from_secs(1), decision_task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(decision, Decision::AllowOnce);
    }

    #[test]
    fn invalid_engine_waiter_option_is_rejected_before_permission_commit() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        let task_id = TaskId::new();
        let segment_id = RunSegmentId::new();
        store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: "create-waiter-option-task".into(),
                    expected_stream_version: 0,
                    authority: TaskStore::supervisor_authority(),
                    payload: json!({ "type": "create" }),
                },
                |_| {
                    Ok(vec![
                        NewTaskEvent::task_created("waiter option"),
                        NewTaskEvent::run_started(segment_id, Utc::now()),
                    ])
                },
            )
            .unwrap();
        let broker = PermissionBroker::new(Arc::clone(&store), Arc::new(NoopRedactor));
        let request_id = RequestId::new();
        broker
            .request(PermissionRequestDraft {
                task_id,
                segment_id,
                request_id,
                request_revision: 1,
                expected_task_version: store.stream_version(task_id).unwrap(),
                kind: DaemonPermissionKind::Command,
                action_plan_hash: "plan".into(),
                sandbox_policy_hash: "sandbox".into(),
                workspace: "/workspace".into(),
                subject: json!({ "command": "cargo test" }),
                actor_source: json!({ "type": "parent_run" }),
                options: vec![PermissionOption {
                    option_id: "allow-once".into(),
                    label: "Allow once".into(),
                }],
                preview: "cargo test".into(),
                expires_at: Utc::now() + Duration::minutes(5),
            })
            .unwrap();
        let (sender, _receiver) = oneshot::channel();
        broker
            .register_engine_waiter(
                request_id,
                HashMap::from([("deny-once".into(), Decision::DenyOnce)]),
                sender,
            )
            .unwrap();

        let result = broker.resolve(PermissionDecisionInput {
            task_id,
            request_id,
            request_revision: 1,
            option_id: "allow-once".into(),
            expected_task_version: store.stream_version(task_id).unwrap(),
        });

        assert!(result.is_err());
        assert!(store
            .task_projection(task_id)
            .unwrap()
            .unwrap()
            .pending_permission
            .is_some());
    }

    #[test]
    fn steering_transaction_invalidates_permission_and_wakes_engine_waiter() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        let task_id = TaskId::new();
        let segment_id = RunSegmentId::new();
        store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: "create-steering-waiter-task".into(),
                    expected_stream_version: 0,
                    authority: TaskStore::supervisor_authority(),
                    payload: json!({ "type": "create" }),
                },
                |_| {
                    Ok(vec![
                        NewTaskEvent::task_created("steering waiter"),
                        NewTaskEvent::run_started(segment_id, Utc::now()),
                    ])
                },
            )
            .unwrap();
        let broker = PermissionBroker::new(Arc::clone(&store), Arc::new(NoopRedactor));
        let request_id = RequestId::new();
        broker
            .request(PermissionRequestDraft {
                task_id,
                segment_id,
                request_id,
                request_revision: 1,
                expected_task_version: store.stream_version(task_id).unwrap(),
                kind: DaemonPermissionKind::Command,
                action_plan_hash: "plan".into(),
                sandbox_policy_hash: "sandbox".into(),
                workspace: "/workspace".into(),
                subject: json!({ "command": "cargo test" }),
                actor_source: json!({ "type": "parent_run" }),
                options: vec![PermissionOption {
                    option_id: "allow-once".into(),
                    label: "Allow once".into(),
                }],
                preview: "cargo test".into(),
                expires_at: Utc::now() + Duration::minutes(5),
            })
            .unwrap();
        let (sender, mut receiver) = oneshot::channel();
        broker
            .register_engine_waiter(
                request_id,
                HashMap::from([("allow-once".into(), Decision::AllowOnce)]),
                sender,
            )
            .unwrap();
        let command = AcceptedCommand {
            command_id: CommandId::new(),
            task_id,
            idempotency_key: "steer-and-invalidate".into(),
            expected_stream_version: store.stream_version(task_id).unwrap(),
            authority: TaskStore::supervisor_authority(),
            payload: json!({ "type": "stop_run" }),
        };

        let outcome = broker
            .transact_invalidating_command(command, request_id, 1, "stopped", |_| {
                Ok(vec![NewTaskEvent::run_yield_requested(
                    segment_id,
                    false,
                    Utc::now(),
                )])
            })
            .unwrap();

        assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
        assert_eq!(receiver.try_recv().unwrap(), Decision::DenyOnce);
        assert!(store
            .task_projection(task_id)
            .unwrap()
            .unwrap()
            .pending_permission
            .is_none());
    }

    fn engine_request(task_id: TaskId) -> EnginePermissionRequest {
        let action_plan_hash = ActionPlanHash::default();
        let scope = DecisionScope::ToolName("Bash".into());
        EnginePermissionRequest {
            request_id: RequestId::new(),
            tenant_id: TenantId::SINGLE,
            session_id: SessionId::from_u128(u128::from_be_bytes(task_id.as_bytes())),
            tool_use_id: ToolUseId::new(),
            tool_name: "Bash".into(),
            subject: PermissionSubject::CommandExec {
                command: "cargo test".into(),
                argv: vec!["cargo".into(), "test".into()],
                cwd: Some("/workspace".into()),
                fingerprint: None,
            },
            severity: Severity::Medium,
            scope_hint: scope.clone(),
            action_plan_hash: action_plan_hash.clone(),
            decision_options: vec![
                PermissionDecisionOption {
                    option_id: PermissionOptionId::new(),
                    decision: Decision::AllowOnce,
                    scope: scope.clone(),
                    lifetime: DecisionLifetime::Once,
                    matcher_summary: DecisionMatcherSummary {
                        kind: DecisionMatcherKind::ToolName,
                        label: "Bash".into(),
                    },
                    label: "Allow once".into(),
                    requires_confirmation: false,
                    action_plan_hash: action_plan_hash.clone(),
                    fingerprint: None,
                },
                PermissionDecisionOption {
                    option_id: PermissionOptionId::new(),
                    decision: Decision::DenyOnce,
                    scope: scope.clone(),
                    lifetime: DecisionLifetime::Once,
                    matcher_summary: DecisionMatcherSummary {
                        kind: DecisionMatcherKind::ToolName,
                        label: "Bash".into(),
                    },
                    label: "Deny once".into(),
                    requires_confirmation: false,
                    action_plan_hash,
                    fingerprint: None,
                },
            ],
            confirmation_expected: None,
            created_at: Utc::now(),
        }
    }

    fn engine_context(
        request: &EnginePermissionRequest,
        segment_id: RunSegmentId,
        deadline_ms: u64,
    ) -> PermissionContext {
        PermissionContext {
            permission_mode: PermissionMode::Default,
            previous_mode: None,
            session_id: request.session_id,
            tenant_id: request.tenant_id,
            run_id: Some(harness_contracts::RunId::from_u128(u128::from_be_bytes(
                segment_id.as_bytes(),
            ))),
            interactivity: InteractivityLevel::FullyInteractive,
            timeout_policy: Some(TimeoutPolicy {
                deadline_ms,
                default_on_timeout: Decision::DenyOnce,
                heartbeat_interval_ms: None,
            }),
            fallback_policy: FallbackPolicy::AskUser,
            hook_overrides: Vec::new(),
        }
    }

    fn runtime_authority(store: &TaskStore, task_id: TaskId) -> PermissionRuntimeAuthority {
        let lease_id = WorkspaceLeaseId::new();
        let actor_id = ActorId::new();
        let lease = match store
            .acquire_workspace_lease(AcquireTaskWorkspaceLease {
                lease_id,
                task_id,
                actor_id,
                mode: WorkspaceMode::Current,
                canonical_root: "/workspace".into(),
                worktree_path: None,
                branch: None,
                writable: true,
                requested_at: Utc::now(),
                expires_at: None,
                baseline_commit: None,
                baseline_status: "clean".into(),
            })
            .unwrap()
        {
            TaskWorkspaceAcquireOutcome::Acquired(lease) => lease,
            TaskWorkspaceAcquireOutcome::Waiting(_) => panic!("test lease must be active"),
        };
        PermissionRuntimeAuthority {
            workspace_lease_id: lease.lease_id,
            actor_id: lease.actor_id,
            execution_root: lease.canonical_root,
            writable: lease.writable,
            sandbox_policy_hash: "sandbox-v1".into(),
        }
    }
}

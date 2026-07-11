//! Daemon-owned permission request routing and decision validation.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use harness_contracts::{
    CommandId, PermissionProjection, PermissionRequestDetails, PermissionRoute, RedactRules,
    Redactor, RequestId, RunSegmentId, RunState, TaskId,
};
pub use harness_contracts::{DaemonPermissionKind, PermissionOption};
use harness_journal::{
    AcceptedCommand, CommandOutcome, CommandRejection, NewTaskEvent, TaskStore, TaskStoreError,
};
use serde_json::{json, Value};
use thiserror::Error;

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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequestOutcome {
    pub auto_resolved: bool,
    pub committed_offset: u64,
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
}

impl PermissionBroker {
    #[must_use]
    pub fn new(store: Arc<TaskStore>, redactor: Arc<dyn Redactor>) -> Self {
        Self {
            store,
            redactor,
            saved_policy: Arc::new(NoSavedPermissionPolicy),
            validation_contexts: Arc::new(Mutex::new(HashMap::new())),
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
        validate_request(&draft)?;
        let validation_context = PermissionValidationContext::from(&draft);
        let mut validation_contexts = self
            .validation_contexts
            .lock()
            .map_err(|_| PermissionBrokerError::ValidationStatePoisoned)?;
        validation_contexts.retain(|_, context| context.expires_at > Utc::now());
        if validation_contexts
            .get(&draft.request_id)
            .is_some_and(|current| current != &validation_context)
        {
            return Err(PermissionBrokerError::InvalidRequest(
                "request id was reused for a different permission context".into(),
            ));
        }
        let saved_option_id = self.saved_policy.resolve(&draft);
        if let Some(option_id) = saved_option_id.as_ref() {
            require_option(&draft.options, option_id)?;
        }
        let projection = self.redacted_projection(
            &draft,
            if saved_option_id.is_some() {
                PermissionRoute::SavedPolicy
            } else {
                PermissionRoute::ForegroundTask
            },
        );
        let command = AcceptedCommand {
            command_id: CommandId::new(),
            task_id: draft.task_id,
            idempotency_key: format!("permission-request:{}", draft.request_id),
            expected_stream_version: draft.expected_task_version,
            authority: TaskStore::permission_broker_authority(),
            payload: json!({
                "type": "permission_request",
                "permission": projection,
                "savedOptionId": saved_option_id,
            }),
        };
        let segment_id = draft.segment_id;
        let request_id = draft.request_id;
        let request_revision = draft.request_revision;
        let auto_resolved = saved_option_id.is_some();
        let outcome = self.store.transact_command(command, |task| {
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
                events.push(NewTaskEvent::permission_resolved(
                    request_id,
                    request_revision,
                ));
            }
            Ok(events)
        })?;
        let (committed_offset, _) = require_accepted(outcome)?;
        if auto_resolved {
            validation_contexts.remove(&request_id);
        } else {
            validation_contexts.insert(request_id, validation_context);
        }
        Ok(PermissionRequestOutcome {
            auto_resolved,
            committed_offset,
        })
    }

    pub fn resolve(
        &self,
        input: PermissionDecisionInput,
    ) -> Result<CommandOutcome, PermissionBrokerError> {
        let mut validation_contexts = self
            .validation_contexts
            .lock()
            .map_err(|_| PermissionBrokerError::ValidationStatePoisoned)?;
        validation_contexts.retain(|_, context| context.expires_at > Utc::now());
        let context = validation_contexts
            .get(&input.request_id)
            .cloned()
            .ok_or_else(|| {
                PermissionBrokerError::Rejected(invalid_command(
                    "permission request has no live daemon validation context",
                ))
            })?;
        if context.task_id != input.task_id || context.request_revision != input.request_revision {
            return Err(PermissionBrokerError::Rejected(invalid_command(
                "permission request identity is stale",
            )));
        }
        if Utc::now() > context.expires_at {
            return Err(PermissionBrokerError::Rejected(invalid_command(
                "permission request has expired",
            )));
        }
        require_option_for_decision(&context.options, &input.option_id)
            .map_err(PermissionBrokerError::Rejected)?;

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
        let outcome = self.store.transact_command(command, |task| {
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
            Ok(vec![NewTaskEvent::permission_resolved(
                input.request_id,
                input.request_revision,
            )])
        })?;
        if matches!(outcome, CommandOutcome::Accepted { .. }) {
            validation_contexts.remove(&input.request_id);
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
        let mut validation_contexts = self
            .validation_contexts
            .lock()
            .map_err(|_| PermissionBrokerError::ValidationStatePoisoned)?;
        validation_contexts.retain(|_, context| context.expires_at > Utc::now());
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
            validation_contexts.remove(&request_id);
        }
        require_accepted(outcome.clone())?;
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

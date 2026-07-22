//! Daemon-owned routing for interactive model questions.

use std::collections::{hash_map::Entry, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use futures::future::BoxFuture;
use harness_contracts::{
    AskUserQuestion, AskUserQuestionAnswer, AskUserQuestionCap, AskUserQuestionOutcome,
    AskUserQuestionRequest, AskUserQuestionResponse, CommandId, PendingQuestionProjection,
    RedactRules, Redactor, RequestId, RunSegmentId, RunState, TaskId, TaskProjection, ToolError,
};
use harness_journal::{
    AcceptedCommand, CommandOutcome, CommandRejection, NewTaskEvent, TaskStore, TaskStoreError,
};
use serde_json::json;
use thiserror::Error;
use tokio::sync::oneshot;

const REQUEST_REVISION: u64 = 1;
const MAX_QUESTIONS: usize = 3;
const MAX_OPTIONS: usize = 4;
const MAX_ID_BYTES: usize = 64;
const MAX_HEADER_BYTES: usize = 32;
const MAX_QUESTION_BYTES: usize = 4_096;
const MAX_LABEL_BYTES: usize = 128;
const MAX_DESCRIPTION_BYTES: usize = 512;
const MAX_ANSWER_BYTES: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionDecisionInput {
    pub task_id: TaskId,
    pub request_id: RequestId,
    pub request_revision: u64,
    pub response: AskUserQuestionResponse,
    pub expected_task_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QuestionValidationContext {
    task_id: TaskId,
    segment_id: RunSegmentId,
    request_revision: u64,
    request: AskUserQuestionRequest,
}

#[derive(Debug, Error)]
pub enum QuestionBrokerError {
    #[error(transparent)]
    Store(#[from] TaskStoreError),
    #[error("question command was rejected: {0:?}")]
    Rejected(CommandRejection),
    #[error("question request is invalid: {0}")]
    InvalidRequest(String),
    #[error("question broker state lock was poisoned")]
    StatePoisoned,
}

#[derive(Clone)]
pub struct QuestionBroker {
    store: Arc<TaskStore>,
    redactor: Arc<dyn Redactor>,
    validation_contexts: Arc<Mutex<HashMap<RequestId, QuestionValidationContext>>>,
    waiters: Arc<Mutex<HashMap<RequestId, oneshot::Sender<AskUserQuestionOutcome>>>>,
}

impl QuestionBroker {
    #[must_use]
    pub fn new(store: Arc<TaskStore>, redactor: Arc<dyn Redactor>) -> Self {
        Self {
            store,
            redactor,
            validation_contexts: Arc::new(Mutex::new(HashMap::new())),
            waiters: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[must_use]
    pub fn channel(
        self: &Arc<Self>,
        task_id: TaskId,
        segment_id: RunSegmentId,
    ) -> Arc<dyn AskUserQuestionCap> {
        Arc::new(DaemonQuestionChannel {
            broker: Arc::clone(self),
            task_id,
            segment_id,
        })
    }

    async fn ask(
        &self,
        task_id: TaskId,
        segment_id: RunSegmentId,
        request: AskUserQuestionRequest,
    ) -> Result<AskUserQuestionOutcome, QuestionBrokerError> {
        validate_request(&request)?;
        let request_id = request.request_id;
        let expires_at = request.expires_at;
        let context = QuestionValidationContext {
            task_id,
            segment_id,
            request_revision: REQUEST_REVISION,
            request: request.clone(),
        };
        let (sender, mut receiver) = oneshot::channel();
        self.reserve_request(context.clone(), sender)?;
        if let Err(error) = self.commit_request(&context) {
            self.release_request(request_id);
            return Err(error);
        }

        let wait = (expires_at - Utc::now())
            .to_std()
            .unwrap_or_else(|_| std::time::Duration::from_secs(0));
        match tokio::time::timeout(wait, &mut receiver).await {
            Ok(Ok(outcome)) => Ok(outcome),
            _ => {
                let invalidation = self.invalidate_runtime_request(
                    task_id,
                    request_id,
                    REQUEST_REVISION,
                    "question request expired while waiting for user input",
                );
                if invalidation.is_err() {
                    if let Some(outcome) = self.store.question_resolution_outcome(
                        task_id,
                        request_id,
                        REQUEST_REVISION,
                    )? {
                        self.release_request(request_id);
                        return Ok(outcome);
                    }
                }
                self.release_request(request_id);
                Ok(AskUserQuestionOutcome::TimedOut)
            }
        }
    }

    fn reserve_request(
        &self,
        context: QuestionValidationContext,
        sender: oneshot::Sender<AskUserQuestionOutcome>,
    ) -> Result<(), QuestionBrokerError> {
        let request_id = context.request.request_id;
        let mut contexts = self
            .validation_contexts
            .lock()
            .map_err(|_| QuestionBrokerError::StatePoisoned)?;
        let mut waiters = self
            .waiters
            .lock()
            .map_err(|_| QuestionBrokerError::StatePoisoned)?;
        match (contexts.entry(request_id), waiters.entry(request_id)) {
            (Entry::Vacant(context_entry), Entry::Vacant(waiter_entry)) => {
                context_entry.insert(context);
                waiter_entry.insert(sender);
                Ok(())
            }
            _ => Err(QuestionBrokerError::InvalidRequest(
                "question request id is already in flight".into(),
            )),
        }
    }

    fn commit_request(
        &self,
        context: &QuestionValidationContext,
    ) -> Result<CommandOutcome, QuestionBrokerError> {
        let projection = self.redacted_projection(context);
        let request_id = context.request.request_id;
        let mut expected_stream_version = self.store.stream_version(context.task_id)?;
        let mut retries_remaining = 8_u8;
        loop {
            let command = AcceptedCommand {
                command_id: CommandId::new(),
                task_id: context.task_id,
                idempotency_key: format!("question-request:{request_id}:{expected_stream_version}"),
                expected_stream_version,
                authority: TaskStore::question_broker_authority(),
                payload: json!({
                    "type": "question_request",
                    "question": projection,
                }),
            };
            let outcome = self.store.transact_command(command, |task| {
                if context.request.expires_at <= Utc::now() {
                    return Err(invalid_command("question request has expired"));
                }
                if task.pending_question.is_some() || task.pending_permission.is_some() {
                    return Err(invalid_command(
                        "another foreground interaction is already pending",
                    ));
                }
                if !task.current_run.as_ref().is_some_and(|run| {
                    run.segment_id == context.segment_id && run.state == RunState::Running
                }) {
                    return Err(invalid_command(
                        "question request requires the current running segment",
                    ));
                }
                Ok(vec![NewTaskEvent::question_requested(projection.clone())])
            })?;
            match outcome {
                CommandOutcome::Rejected {
                    rejection: CommandRejection::WrongExpectedVersion { actual, .. },
                    ..
                } if retries_remaining > 0 => {
                    expected_stream_version = actual;
                    retries_remaining -= 1;
                }
                outcome => {
                    require_accepted(outcome.clone())?;
                    return Ok(outcome);
                }
            }
        }
    }

    pub fn resolve_client_command(
        &self,
        mut command: AcceptedCommand,
        input: QuestionDecisionInput,
    ) -> Result<CommandOutcome, QuestionBrokerError> {
        let command_id = command.command_id;
        let task_id = command.task_id;
        command.authority = TaskStore::question_broker_command_authority(&command.authority);
        match self.resolve_with_command(command, input) {
            Ok(outcome) => Ok(outcome),
            Err(QuestionBrokerError::Rejected(rejection)) => Ok(CommandOutcome::Rejected {
                command_id,
                task_id,
                rejection,
            }),
            Err(QuestionBrokerError::Store(
                TaskStoreError::CommandConflict { .. } | TaskStoreError::InvalidInput(_),
            )) => Ok(CommandOutcome::Rejected {
                command_id,
                task_id,
                rejection: invalid_command("question command conflicts with durable input"),
            }),
            Err(error) => Err(error),
        }
    }

    fn resolve_with_command(
        &self,
        mut command: AcceptedCommand,
        input: QuestionDecisionInput,
    ) -> Result<CommandOutcome, QuestionBrokerError> {
        if command.task_id != input.task_id
            || command.expected_stream_version != input.expected_task_version
        {
            return Err(QuestionBrokerError::Rejected(invalid_command(
                "question command metadata does not match its response",
            )));
        }
        let context = self
            .validation_contexts
            .lock()
            .map_err(|_| QuestionBrokerError::StatePoisoned)?
            .get(&input.request_id)
            .cloned();
        if let Some(context) = context.as_ref() {
            validate_response(&context.request.questions, &input.response)
                .map_err(|message| QuestionBrokerError::Rejected(invalid_command(message)))?;
        }
        let outcome = response_outcome(input.response.clone());
        let durable_outcome = self.redacted_outcome(&outcome);
        command.payload = json!({
            "type": "question_resolve",
            "requestId": input.request_id,
            "requestRevision": input.request_revision,
            "outcome": durable_outcome,
        });
        let result = self.store.transact_command(command, |task| {
            let context = context.as_ref().ok_or_else(|| {
                invalid_command("question request has no live daemon validation context")
            })?;
            if context.task_id != input.task_id
                || context.request_revision != input.request_revision
            {
                return Err(invalid_command("question request identity is stale"));
            }
            if Utc::now() > context.request.expires_at {
                return Err(invalid_command("question request has expired"));
            }
            let pending = task
                .pending_question
                .as_ref()
                .ok_or_else(|| invalid_command("question request is no longer pending"))?;
            if pending.request_id != input.request_id
                || pending.revision != input.request_revision
                || pending.segment_id != context.segment_id
                || pending.tool_use_id != context.request.tool_use_id
                || pending.expires_at != context.request.expires_at
                || pending.questions != self.redacted_questions(&context.request.questions)
            {
                return Err(invalid_command("question response context changed"));
            }
            Ok(vec![NewTaskEvent::question_resolved(
                input.request_id,
                input.request_revision,
                durable_outcome.clone(),
            )])
        })?;
        require_accepted(result.clone())?;
        self.complete(input.request_id, outcome);
        Ok(result)
    }

    pub fn invalidate(
        &self,
        task_id: TaskId,
        request_id: RequestId,
        request_revision: u64,
        expected_task_version: u64,
        reason: impl Into<String>,
    ) -> Result<CommandOutcome, QuestionBrokerError> {
        let reason = self
            .redactor
            .redact(&reason.into(), &RedactRules::default());
        let command = AcceptedCommand {
            command_id: CommandId::new(),
            task_id,
            idempotency_key: format!(
                "question-invalidate:{request_id}:{request_revision}:{expected_task_version}"
            ),
            expected_stream_version: expected_task_version,
            authority: TaskStore::question_broker_authority(),
            payload: json!({
                "type": "question_invalidate",
                "requestId": request_id,
                "requestRevision": request_revision,
                "reason": reason,
            }),
        };
        let outcome = self.store.transact_command(command, |task| {
            require_pending(task, request_id, request_revision)?;
            Ok(vec![NewTaskEvent::question_invalidated(
                request_id,
                request_revision,
                reason,
            )])
        })?;
        require_accepted(outcome.clone())?;
        self.complete(request_id, AskUserQuestionOutcome::Cancelled);
        Ok(outcome)
    }

    fn invalidate_runtime_request(
        &self,
        task_id: TaskId,
        request_id: RequestId,
        request_revision: u64,
        reason: impl Into<String>,
    ) -> Result<CommandOutcome, QuestionBrokerError> {
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
                Err(QuestionBrokerError::Rejected(CommandRejection::WrongExpectedVersion {
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
    ) -> Result<CommandOutcome, QuestionBrokerError>
    where
        F: FnOnce(&TaskProjection) -> Result<Vec<NewTaskEvent>, CommandRejection>,
    {
        let reason = self
            .redactor
            .redact(&reason.into(), &RedactRules::default());
        let outcome = self.store.transact_command(command, |task| {
            require_pending(task, request_id, request_revision)?;
            let mut events = vec![NewTaskEvent::question_invalidated(
                request_id,
                request_revision,
                reason,
            )];
            events.extend(decide(task)?);
            Ok(events)
        })?;
        if matches!(outcome, CommandOutcome::Accepted { .. }) {
            self.complete(request_id, AskUserQuestionOutcome::Cancelled);
        }
        Ok(outcome)
    }

    fn redacted_projection(
        &self,
        context: &QuestionValidationContext,
    ) -> PendingQuestionProjection {
        PendingQuestionProjection {
            request_id: context.request.request_id,
            revision: context.request_revision,
            segment_id: context.segment_id,
            tool_use_id: context.request.tool_use_id,
            questions: self.redacted_questions(&context.request.questions),
            expires_at: context.request.expires_at,
        }
    }

    fn redacted_questions(&self, questions: &[AskUserQuestion]) -> Vec<AskUserQuestion> {
        questions
            .iter()
            .map(|question| AskUserQuestion {
                id: question.id.clone(),
                header: question
                    .header
                    .as_ref()
                    .map(|value| self.redactor.redact(value, &RedactRules::default())),
                question: self
                    .redactor
                    .redact(&question.question, &RedactRules::default()),
                options: question
                    .options
                    .iter()
                    .map(|option| harness_contracts::AskUserQuestionOption {
                        id: option.id.clone(),
                        label: self.redactor.redact(&option.label, &RedactRules::default()),
                        description: option
                            .description
                            .as_ref()
                            .map(|value| self.redactor.redact(value, &RedactRules::default())),
                    })
                    .collect(),
                multi_select: question.multi_select,
                allow_custom: question.allow_custom,
            })
            .collect()
    }

    fn redacted_outcome(&self, outcome: &AskUserQuestionOutcome) -> AskUserQuestionOutcome {
        match outcome {
            AskUserQuestionOutcome::Answered { answers } => AskUserQuestionOutcome::Answered {
                answers: answers
                    .iter()
                    .map(|answer| AskUserQuestionAnswer {
                        question_id: answer.question_id.clone(),
                        selected_option_ids: answer.selected_option_ids.clone(),
                        text: answer
                            .text
                            .as_ref()
                            .map(|value| self.redactor.redact(value, &RedactRules::default())),
                    })
                    .collect(),
            },
            outcome => outcome.clone(),
        }
    }

    fn complete(&self, request_id: RequestId, outcome: AskUserQuestionOutcome) {
        self.validation_contexts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&request_id);
        if let Some(sender) = self
            .waiters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&request_id)
        {
            let _ = sender.send(outcome);
        }
    }

    fn release_request(&self, request_id: RequestId) {
        self.validation_contexts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&request_id);
        self.waiters
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&request_id);
    }
}

#[derive(Clone)]
struct DaemonQuestionChannel {
    broker: Arc<QuestionBroker>,
    task_id: TaskId,
    segment_id: RunSegmentId,
}

impl AskUserQuestionCap for DaemonQuestionChannel {
    fn ask(
        &self,
        request: AskUserQuestionRequest,
    ) -> BoxFuture<'static, Result<AskUserQuestionOutcome, ToolError>> {
        let broker = Arc::clone(&self.broker);
        let task_id = self.task_id;
        let segment_id = self.segment_id;
        Box::pin(async move {
            broker
                .ask(task_id, segment_id, request)
                .await
                .map_err(|error| ToolError::Message(error.to_string()))
        })
    }
}

fn validate_request(request: &AskUserQuestionRequest) -> Result<(), QuestionBrokerError> {
    if request.expires_at <= Utc::now() {
        return Err(QuestionBrokerError::InvalidRequest(
            "request is already expired".into(),
        ));
    }
    if !(1..=MAX_QUESTIONS).contains(&request.questions.len()) {
        return Err(QuestionBrokerError::InvalidRequest(format!(
            "request must contain between 1 and {MAX_QUESTIONS} questions"
        )));
    }
    let mut question_ids = HashSet::with_capacity(request.questions.len());
    for question in &request.questions {
        require_text(&question.id, "question id", MAX_ID_BYTES)?;
        require_text(&question.question, "question text", MAX_QUESTION_BYTES)?;
        if !question_ids.insert(question.id.as_str()) {
            return Err(QuestionBrokerError::InvalidRequest(
                "question ids must be unique".into(),
            ));
        }
        if question
            .header
            .as_ref()
            .is_some_and(|header| header.trim().is_empty() || header.len() > MAX_HEADER_BYTES)
        {
            return Err(QuestionBrokerError::InvalidRequest(
                "question header is empty or too long".into(),
            ));
        }
        if question.options.len() > MAX_OPTIONS
            || (!question.options.is_empty() && question.options.len() < 2)
        {
            return Err(QuestionBrokerError::InvalidRequest(format!(
                "question options must be empty or contain between 2 and {MAX_OPTIONS} items"
            )));
        }
        if question.multi_select && question.options.is_empty() {
            return Err(QuestionBrokerError::InvalidRequest(
                "multi-select questions require options".into(),
            ));
        }
        let mut option_ids = HashSet::with_capacity(question.options.len());
        for option in &question.options {
            require_text(&option.id, "option id", MAX_ID_BYTES)?;
            require_text(&option.label, "option label", MAX_LABEL_BYTES)?;
            if !option_ids.insert(option.id.as_str()) {
                return Err(QuestionBrokerError::InvalidRequest(
                    "option ids must be unique within a question".into(),
                ));
            }
            if option.description.as_ref().is_some_and(|description| {
                description.trim().is_empty() || description.len() > MAX_DESCRIPTION_BYTES
            }) {
                return Err(QuestionBrokerError::InvalidRequest(
                    "option description is empty or too long".into(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_response(
    questions: &[AskUserQuestion],
    response: &AskUserQuestionResponse,
) -> Result<(), &'static str> {
    let AskUserQuestionResponse::Answered { answers } = response else {
        return Ok(());
    };
    if answers.len() != questions.len() {
        return Err("response must answer every question exactly once");
    }
    let mut answered = HashSet::with_capacity(answers.len());
    for answer in answers {
        if !answered.insert(answer.question_id.as_str()) {
            return Err("response contains a duplicate question answer");
        }
        let Some(question) = questions
            .iter()
            .find(|question| question.id == answer.question_id)
        else {
            return Err("response contains an unknown question id");
        };
        let mut selected = HashSet::with_capacity(answer.selected_option_ids.len());
        if answer.selected_option_ids.iter().any(|option_id| {
            !selected.insert(option_id.as_str())
                || !question
                    .options
                    .iter()
                    .any(|option| option.id == *option_id)
        }) {
            return Err("response contains an invalid option id");
        }
        if !question.multi_select && answer.selected_option_ids.len() > 1 {
            return Err("single-select question has multiple selected options");
        }
        let has_text = answer
            .text
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty());
        if answer
            .text
            .as_ref()
            .is_some_and(|text| text.len() > MAX_ANSWER_BYTES)
        {
            return Err("custom response text is too long");
        }
        if has_text && !question.options.is_empty() && !question.allow_custom {
            return Err("question does not allow a custom response");
        }
        if answer.selected_option_ids.is_empty() && !has_text {
            return Err("question response is empty");
        }
    }
    Ok(())
}

fn response_outcome(response: AskUserQuestionResponse) -> AskUserQuestionOutcome {
    match response {
        AskUserQuestionResponse::Answered { answers } => {
            AskUserQuestionOutcome::Answered { answers }
        }
        AskUserQuestionResponse::Declined => AskUserQuestionOutcome::Declined,
    }
}

fn require_text(value: &str, field: &str, max_bytes: usize) -> Result<(), QuestionBrokerError> {
    if value.trim().is_empty() || value.len() > max_bytes {
        Err(QuestionBrokerError::InvalidRequest(format!(
            "{field} is empty or too long"
        )))
    } else {
        Ok(())
    }
}

fn require_pending(
    task: &TaskProjection,
    request_id: RequestId,
    revision: u64,
) -> Result<(), CommandRejection> {
    let pending = task
        .pending_question
        .as_ref()
        .ok_or_else(|| invalid_command("question request is no longer pending"))?;
    if pending.request_id == request_id && pending.revision == revision {
        Ok(())
    } else {
        Err(invalid_command("question request identity is stale"))
    }
}

fn require_accepted(outcome: CommandOutcome) -> Result<(), QuestionBrokerError> {
    match outcome {
        CommandOutcome::Accepted { .. } => Ok(()),
        CommandOutcome::Rejected { rejection, .. } => Err(QuestionBrokerError::Rejected(rejection)),
    }
}

fn invalid_command(message: impl Into<String>) -> CommandRejection {
    CommandRejection::InvalidCommand {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use chrono::Utc;
    use harness_contracts::{
        AskUserQuestion, AskUserQuestionAnswer, AskUserQuestionOption, AskUserQuestionOutcome,
        AskUserQuestionRequest, AskUserQuestionResponse, ClientId, CommandId, NoopRedactor,
        PermissionActorSource, RequestId, RunId, RunSegmentId, RunState, SessionId, TaskId,
        ToolUseId,
    };
    use harness_journal::{AcceptedCommand, CommandOutcome, NewTaskEvent, TaskStore};
    use serde_json::json;

    use super::{validate_request, validate_response, QuestionBroker, QuestionDecisionInput};

    fn choice_question() -> AskUserQuestion {
        AskUserQuestion {
            id: "choice".into(),
            header: None,
            question: "Pick one".into(),
            options: vec![
                AskUserQuestionOption {
                    id: "a".into(),
                    label: "A".into(),
                    description: None,
                },
                AskUserQuestionOption {
                    id: "b".into(),
                    label: "B".into(),
                    description: None,
                },
            ],
            multi_select: false,
            allow_custom: false,
        }
    }

    fn question_request(
        request_id: RequestId,
        tool_use_id: ToolUseId,
        expires_at: chrono::DateTime<Utc>,
    ) -> AskUserQuestionRequest {
        AskUserQuestionRequest {
            request_id,
            tool_use_id,
            run_id: RunId::new(),
            session_id: SessionId::new(),
            actor_source: PermissionActorSource::ParentRun,
            questions: vec![choice_question()],
            expires_at,
        }
    }

    async fn wait_for_pending(store: &TaskStore, task_id: TaskId) {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if store
                    .task_projection(task_id)
                    .unwrap()
                    .is_some_and(|projection| projection.pending_question.is_some())
                {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[test]
    fn request_validation_limits_batches_to_three_questions() {
        let mut request = question_request(
            RequestId::new(),
            ToolUseId::new(),
            Utc::now() + chrono::Duration::minutes(5),
        );
        request.questions = (1..=3)
            .map(|index| {
                let mut question = choice_question();
                question.id = format!("choice-{index}");
                question
            })
            .collect();
        assert!(validate_request(&request).is_ok());

        let mut fourth = choice_question();
        fourth.id = "choice-4".into();
        request.questions.push(fourth);
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn response_validation_covers_multi_select_custom_text_and_stale_ids() {
        let questions = vec![
            AskUserQuestion {
                id: "targets".into(),
                header: None,
                question: "Choose targets".into(),
                options: vec![
                    AskUserQuestionOption {
                        id: "a".into(),
                        label: "A".into(),
                        description: None,
                    },
                    AskUserQuestionOption {
                        id: "b".into(),
                        label: "B".into(),
                        description: None,
                    },
                ],
                multi_select: true,
                allow_custom: false,
            },
            AskUserQuestion {
                id: "details".into(),
                header: None,
                question: "Add details".into(),
                options: Vec::new(),
                multi_select: false,
                allow_custom: true,
            },
        ];
        let valid = AskUserQuestionResponse::Answered {
            answers: vec![
                AskUserQuestionAnswer {
                    question_id: "targets".into(),
                    selected_option_ids: vec!["a".into(), "b".into()],
                    text: None,
                },
                AskUserQuestionAnswer {
                    question_id: "details".into(),
                    selected_option_ids: Vec::new(),
                    text: Some("Use both".into()),
                },
            ],
        };
        assert!(validate_response(&questions, &valid).is_ok());
        assert!(validate_response(&questions, &AskUserQuestionResponse::Declined).is_ok());

        let stale = AskUserQuestionResponse::Answered {
            answers: vec![
                AskUserQuestionAnswer {
                    question_id: "targets".into(),
                    selected_option_ids: vec!["stale".into()],
                    text: None,
                },
                AskUserQuestionAnswer {
                    question_id: "details".into(),
                    selected_option_ids: Vec::new(),
                    text: Some("Use both".into()),
                },
            ],
        };
        assert_eq!(
            validate_response(&questions, &stale),
            Err("response contains an invalid option id")
        );
    }

    #[tokio::test]
    async fn request_and_resolution_round_trip_through_durable_projection() {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(TaskStore::open(root.path().join("tasks.sqlite")).unwrap());
        let task_id = TaskId::new();
        let segment_id = RunSegmentId::new();
        let create = store
            .transact_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: "create-question-task".into(),
                    expected_stream_version: 0,
                    authority: TaskStore::supervisor_authority(),
                    payload: json!({ "type": "create" }),
                },
                |_| {
                    Ok(vec![
                        NewTaskEvent::task_created("question"),
                        NewTaskEvent::run_started(segment_id, Utc::now()),
                    ])
                },
            )
            .unwrap();
        assert!(matches!(create, CommandOutcome::Accepted { .. }));

        let broker = Arc::new(QuestionBroker::new(
            Arc::clone(&store),
            Arc::new(NoopRedactor),
        ));
        let request_id = RequestId::new();
        let tool_use_id = ToolUseId::new();
        let channel = broker.channel(task_id, segment_id);
        let waiter = tokio::spawn(async move {
            channel
                .ask(question_request(
                    request_id,
                    tool_use_id,
                    Utc::now() + chrono::Duration::minutes(1),
                ))
                .await
        });

        wait_for_pending(&store, task_id).await;
        let pending = store.task_projection(task_id).unwrap().unwrap();
        assert_eq!(pending.current_run.unwrap().state, RunState::WaitingInput);

        let response = AskUserQuestionResponse::Answered {
            answers: vec![AskUserQuestionAnswer {
                question_id: "choice".into(),
                selected_option_ids: vec!["a".into()],
                text: None,
            }],
        };
        let expected_task_version = store.stream_version(task_id).unwrap();
        let stale = broker
            .resolve_client_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: "resolve-stale-question".into(),
                    expected_stream_version: expected_task_version,
                    authority: TaskStore::user_authority(ClientId::new()),
                    payload: json!({}),
                },
                QuestionDecisionInput {
                    task_id,
                    request_id,
                    request_revision: 2,
                    response: response.clone(),
                    expected_task_version,
                },
            )
            .unwrap();
        assert!(matches!(stale, CommandOutcome::Rejected { .. }));
        assert!(store
            .task_projection(task_id)
            .unwrap()
            .unwrap()
            .pending_question
            .is_some());

        let outcome = broker
            .resolve_client_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: "resolve-question".into(),
                    expected_stream_version: expected_task_version,
                    authority: TaskStore::user_authority(ClientId::new()),
                    payload: json!({}),
                },
                QuestionDecisionInput {
                    task_id,
                    request_id,
                    request_revision: 1,
                    response: response.clone(),
                    expected_task_version,
                },
            )
            .unwrap();
        assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
        assert_eq!(
            waiter.await.unwrap().unwrap(),
            AskUserQuestionOutcome::Answered {
                answers: vec![AskUserQuestionAnswer {
                    question_id: "choice".into(),
                    selected_option_ids: vec!["a".into()],
                    text: None,
                }],
            }
        );
        let projection = store.task_projection(task_id).unwrap().unwrap();
        assert!(projection.pending_question.is_none());
        assert_eq!(projection.current_run.unwrap().state, RunState::Running);

        let competing_version = store.stream_version(task_id).unwrap();
        let competing = broker
            .resolve_client_command(
                AcceptedCommand {
                    command_id: CommandId::new(),
                    task_id,
                    idempotency_key: "resolve-question-from-second-client".into(),
                    expected_stream_version: competing_version,
                    authority: TaskStore::user_authority(ClientId::new()),
                    payload: json!({}),
                },
                QuestionDecisionInput {
                    task_id,
                    request_id,
                    request_revision: 1,
                    response,
                    expected_task_version: competing_version,
                },
            )
            .unwrap();
        assert!(matches!(competing, CommandOutcome::Rejected { .. }));

        let timeout_request_id = RequestId::new();
        let timeout_outcome = broker
            .channel(task_id, segment_id)
            .ask(question_request(
                timeout_request_id,
                ToolUseId::new(),
                Utc::now() + chrono::Duration::milliseconds(20),
            ))
            .await
            .unwrap();
        assert_eq!(timeout_outcome, AskUserQuestionOutcome::TimedOut);
        assert!(store
            .task_projection(task_id)
            .unwrap()
            .unwrap()
            .pending_question
            .is_none());

        let cancelled_request_id = RequestId::new();
        let cancel_channel = broker.channel(task_id, segment_id);
        let cancel_waiter = tokio::spawn(async move {
            cancel_channel
                .ask(question_request(
                    cancelled_request_id,
                    ToolUseId::new(),
                    Utc::now() + chrono::Duration::minutes(1),
                ))
                .await
        });
        wait_for_pending(&store, task_id).await;
        let cancel_version = store.stream_version(task_id).unwrap();
        let cancelled = broker
            .invalidate(
                task_id,
                cancelled_request_id,
                1,
                cancel_version,
                "cancelled by test",
            )
            .unwrap();
        assert!(matches!(cancelled, CommandOutcome::Accepted { .. }));
        assert_eq!(
            cancel_waiter.await.unwrap().unwrap(),
            AskUserQuestionOutcome::Cancelled
        );
    }
}

use std::sync::Arc;

use chrono::Utc;
use harness_contracts::{
    BackgroundAgentArchivedEvent, BackgroundAgentCancelledEvent, BackgroundAgentCompletedEvent,
    BackgroundAgentDeletedEvent, BackgroundAgentFailedEvent, BackgroundAgentId,
    BackgroundAgentInputRequestedEvent, BackgroundAgentInputSubmittedEvent,
    BackgroundAgentInterruptedEvent, BackgroundAgentPermissionRequestedEvent,
    BackgroundAgentPermissionResolvedEvent, BackgroundAgentStartedEvent, BackgroundAgentState,
    BackgroundAgentStateChangedEvent, Decision, Event, JournalError, RedactRules, Redactor,
    RequestId, RunId, SessionId, TenantId, UiSafeText,
};
use harness_journal::{AppendMetadata, EventStore};
use thiserror::Error;

use crate::store::{BackgroundAgentAttemptRecord, BackgroundAgentStoreRecord};
use crate::{AgentRuntimeStore, AgentRuntimeStoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundAgentRecord {
    pub background_agent_id: String,
    pub conversation_id: String,
    pub run_id: Option<String>,
    pub state: BackgroundAgentState,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundAgentStartRequest {
    pub background_agent_id: Option<String>,
    pub conversation_id: SessionId,
    pub title: String,
    pub payload_json: String,
}

#[derive(Debug, Error)]
pub enum BackgroundAgentTransitionError {
    #[error("background agent not found: {0}")]
    NotFound(String),
    #[error("invalid background agent transition {operation} from {state:?}")]
    InvalidTransition {
        operation: &'static str,
        state: BackgroundAgentState,
    },
    #[error("invalid background agent id: {0}")]
    InvalidBackgroundAgentId(String),
    #[error("agent runtime store: {0}")]
    Store(#[from] AgentRuntimeStoreError),
    #[error("journal: {0}")]
    Journal(#[from] JournalError),
}

pub struct BackgroundAgentManager {
    store: Arc<AgentRuntimeStore>,
    event_store: Arc<dyn EventStore>,
    tenant_id: TenantId,
    journal_session_id: SessionId,
    redactor: Arc<dyn Redactor>,
}

impl BackgroundAgentManager {
    pub fn new(
        store: Arc<AgentRuntimeStore>,
        event_store: Arc<dyn EventStore>,
        tenant_id: TenantId,
        journal_session_id: SessionId,
        redactor: Arc<dyn Redactor>,
    ) -> Self {
        Self {
            store,
            event_store,
            tenant_id,
            journal_session_id,
            redactor,
        }
    }

    pub async fn start(
        &self,
        request: BackgroundAgentStartRequest,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        let background_agent_id = request
            .background_agent_id
            .unwrap_or_else(|| BackgroundAgentId::new().to_string());
        let typed_id = parse_background_agent_id(&background_agent_id)?;
        let attempt_id = RunId::new();
        let now = Utc::now().to_rfc3339();
        let title = self.safe_text(&request.title).into_string();
        self.append(&[
            Event::BackgroundAgentStarted(BackgroundAgentStartedEvent {
                background_agent_id: typed_id,
                conversation_id: request.conversation_id,
                attempt_id,
                title: self.safe_text(&title),
                at: Utc::now(),
            }),
            Event::BackgroundAgentStateChanged(BackgroundAgentStateChangedEvent {
                background_agent_id: typed_id,
                from: BackgroundAgentState::Queued,
                to: BackgroundAgentState::Running,
                attempt_id: Some(attempt_id),
                reason: None,
                at: Utc::now(),
            }),
        ])
        .await?;
        let queued = BackgroundAgentStoreRecord {
            background_agent_id: background_agent_id.clone(),
            conversation_id: request.conversation_id.to_string(),
            run_id: Some(attempt_id.to_string()),
            state: BackgroundAgentState::Queued,
            title: title.clone(),
            created_at: now.clone(),
            updated_at: now.clone(),
            payload_json: request.payload_json,
        };
        self.store.insert_background_agent(&queued)?;
        self.store
            .insert_background_agent_attempt(&BackgroundAgentAttemptRecord {
                attempt_id: attempt_id.to_string(),
                background_agent_id: background_agent_id.clone(),
                prior_attempt_id: None,
                attempt_number: 1,
                state: BackgroundAgentState::Running,
                started_at: now.clone(),
                ended_at: None,
                payload_json: "{}".to_owned(),
            })?;
        self.store.update_background_agent_state(
            &background_agent_id,
            BackgroundAgentState::Running,
            &now,
        )?;
        self.get(&background_agent_id)
    }

    pub fn list(
        &self,
        include_archived: bool,
    ) -> Result<Vec<BackgroundAgentRecord>, BackgroundAgentTransitionError> {
        Ok(self
            .store
            .list_background_agents(include_archived)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn get(
        &self,
        background_agent_id: &str,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        self.store
            .get_background_agent(background_agent_id)?
            .map(Into::into)
            .ok_or_else(|| BackgroundAgentTransitionError::NotFound(background_agent_id.to_owned()))
    }

    pub async fn pause(
        &self,
        background_agent_id: &str,
        reason: &str,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        self.transition(
            background_agent_id,
            "pause",
            &[
                BackgroundAgentState::Queued,
                BackgroundAgentState::Running,
                BackgroundAgentState::WaitingForInput,
            ],
            BackgroundAgentState::Paused,
            Some(reason),
            None,
        )
        .await
    }

    pub async fn resume(
        &self,
        background_agent_id: &str,
        reason: &str,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        let current = self.get(background_agent_id)?;
        ensure_state(
            "resume",
            current.state,
            &[
                BackgroundAgentState::Paused,
                BackgroundAgentState::Interrupted,
                BackgroundAgentState::Recoverable,
            ],
        )?;
        if current.state == BackgroundAgentState::Interrupted {
            let prior_attempt_id = self.latest_attempt_id(background_agent_id)?;
            let attempt_id = RunId::new();
            let started_event = Event::BackgroundAgentStarted(BackgroundAgentStartedEvent {
                background_agent_id: parse_background_agent_id(background_agent_id)?,
                conversation_id: current_conversation_id(&current)
                    .unwrap_or(self.journal_session_id),
                attempt_id,
                title: self.safe_text(&current.title),
                at: Utc::now(),
            });
            let attempt_record = self.build_attempt_record(
                background_agent_id,
                attempt_id,
                BackgroundAgentState::Running,
                prior_attempt_id,
            )?;
            return self
                .transition_from_record_with_attempt_and_events(
                    current,
                    "resume",
                    BackgroundAgentState::Running,
                    Some(reason),
                    Some(attempt_id),
                    None,
                    Some(attempt_record),
                    vec![started_event],
                )
                .await;
        }
        let queued = self
            .transition_from_record_with_attempt_and_events(
                current,
                "resume",
                BackgroundAgentState::Queued,
                Some(reason),
                None,
                None,
                None,
                Vec::new(),
            )
            .await?;
        let target = if recoverable_kind(&queued.payload_json) == Some(RecoverableKind::Input) {
            BackgroundAgentState::WaitingForInput
        } else {
            BackgroundAgentState::Running
        };
        self.transition_from_record(queued, "resume", target, Some(reason), None)
            .await
    }

    pub async fn wait_for_permission(
        &self,
        background_agent_id: &str,
        request_id: RequestId,
        reason: &str,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        let current = self.get(background_agent_id)?;
        ensure_state(
            "wait_for_permission",
            current.state,
            &[BackgroundAgentState::Queued, BackgroundAgentState::Running],
        )?;
        let payload_json = set_recoverable_request(
            &current.payload_json,
            RecoverableKind::Permission,
            request_id,
        );
        let permission_event =
            Event::BackgroundAgentPermissionRequested(BackgroundAgentPermissionRequestedEvent {
                background_agent_id: parse_background_agent_id(background_agent_id)?,
                tenant_id: self.tenant_id,
                conversation_id: current_conversation_id(&current)
                    .unwrap_or(self.journal_session_id),
                request_id,
                attempt_id: current_attempt_id(&current),
                reason: self.safe_text(reason),
                at: Utc::now(),
            });
        self.transition_from_record_with_events(
            current,
            "wait_for_permission",
            BackgroundAgentState::WaitingForPermission,
            Some(reason),
            None,
            Some(payload_json),
            vec![permission_event],
        )
        .await
    }

    pub async fn request_input(
        &self,
        background_agent_id: &str,
        request_id: RequestId,
        prompt: &str,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        let current = self.get(background_agent_id)?;
        ensure_state(
            "request_input",
            current.state,
            &[BackgroundAgentState::Queued, BackgroundAgentState::Running],
        )?;
        let payload_json =
            set_recoverable_request(&current.payload_json, RecoverableKind::Input, request_id);
        self.transition_from_record_with_events(
            current,
            "request_input",
            BackgroundAgentState::WaitingForInput,
            Some(prompt),
            None,
            Some(payload_json),
            vec![Event::BackgroundAgentInputRequested(
                BackgroundAgentInputRequestedEvent {
                    background_agent_id: parse_background_agent_id(background_agent_id)?,
                    request_id,
                    prompt: self.safe_text(prompt),
                    at: Utc::now(),
                },
            )],
        )
        .await
    }

    pub async fn send_input(
        &self,
        background_agent_id: &str,
        request_id: RequestId,
        input: &str,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        let current = self.get(background_agent_id)?;
        ensure_state(
            "send_input",
            current.state,
            &[
                BackgroundAgentState::WaitingForInput,
                BackgroundAgentState::Recoverable,
            ],
        )?;
        ensure_recoverable_request_matches(
            "send_input",
            &current,
            RecoverableKind::Input,
            request_id,
        )?;
        self.append(&[Event::BackgroundAgentInputSubmitted(
            BackgroundAgentInputSubmittedEvent {
                background_agent_id: parse_background_agent_id(background_agent_id)?,
                request_id,
                input: self.safe_text(input),
                at: Utc::now(),
            },
        )])
        .await?;
        let cleared_payload =
            clear_recoverable_request(&current.payload_json, RecoverableKind::Input);
        let queued = self
            .transition_from_record_with_events(
                current,
                "send_input",
                BackgroundAgentState::Queued,
                Some("input submitted"),
                None,
                Some(cleared_payload),
                Vec::new(),
            )
            .await?;
        self.transition_from_record(
            queued,
            "send_input",
            BackgroundAgentState::Running,
            Some("input submitted"),
            None,
        )
        .await
    }

    pub async fn resolve_permission(
        &self,
        background_agent_id: &str,
        request_id: RequestId,
        approved: bool,
        reason: &str,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        let current = self.get(background_agent_id)?;
        ensure_state(
            "resolve_permission",
            current.state,
            &[
                BackgroundAgentState::WaitingForPermission,
                BackgroundAgentState::Recoverable,
            ],
        )?;
        ensure_recoverable_request_matches(
            "resolve_permission",
            &current,
            RecoverableKind::Permission,
            request_id,
        )?;
        let cleared_payload =
            clear_recoverable_request(&current.payload_json, RecoverableKind::Permission);
        let permission_event =
            Event::BackgroundAgentPermissionResolved(BackgroundAgentPermissionResolvedEvent {
                background_agent_id: parse_background_agent_id(background_agent_id)?,
                tenant_id: self.tenant_id,
                conversation_id: current_conversation_id(&current)
                    .unwrap_or(self.journal_session_id),
                request_id,
                attempt_id: current_attempt_id(&current),
                decision: if approved {
                    Decision::AllowOnce
                } else {
                    Decision::DenyOnce
                },
                at: Utc::now(),
            });
        if !approved {
            let failed = self
                .transition_from_record_with_events(
                    current,
                    "resolve_permission",
                    BackgroundAgentState::Failed,
                    Some(reason),
                    None,
                    Some(cleared_payload),
                    vec![
                        permission_event,
                        Event::BackgroundAgentFailed(BackgroundAgentFailedEvent {
                            background_agent_id: parse_background_agent_id(background_agent_id)?,
                            error: self.safe_text(reason),
                            at: Utc::now(),
                        }),
                    ],
                )
                .await?;
            return Ok(failed);
        }
        let queued = self
            .transition_from_record_with_events(
                current,
                "resolve_permission",
                BackgroundAgentState::Queued,
                Some(reason),
                None,
                Some(cleared_payload),
                vec![permission_event],
            )
            .await?;
        self.transition_from_record(
            queued,
            "resolve_permission",
            BackgroundAgentState::Running,
            Some(reason),
            None,
        )
        .await
    }

    pub async fn cancel(
        &self,
        background_agent_id: &str,
        reason: &str,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        let current = self.get(background_agent_id)?;
        ensure_state(
            "cancel",
            current.state,
            &[
                BackgroundAgentState::Queued,
                BackgroundAgentState::Running,
                BackgroundAgentState::WaitingForPermission,
                BackgroundAgentState::WaitingForInput,
                BackgroundAgentState::Paused,
                BackgroundAgentState::Recoverable,
            ],
        )?;
        let cancelling = self
            .transition_from_record(
                current,
                "cancel",
                BackgroundAgentState::Cancelling,
                Some(reason),
                None,
            )
            .await?;
        let cancelled = self
            .transition_from_record_with_events(
                cancelling,
                "cancel",
                BackgroundAgentState::Cancelled,
                Some(reason),
                None,
                None,
                vec![Event::BackgroundAgentCancelled(
                    BackgroundAgentCancelledEvent {
                        background_agent_id: parse_background_agent_id(background_agent_id)?,
                        reason: Some(self.safe_text(reason)),
                        at: Utc::now(),
                    },
                )],
            )
            .await?;
        Ok(cancelled)
    }

    pub async fn complete(
        &self,
        background_agent_id: &str,
        summary: &str,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        let current = self.get(background_agent_id)?;
        ensure_state(
            "complete",
            current.state,
            &[BackgroundAgentState::Running, BackgroundAgentState::Queued],
        )?;
        self.transition_from_record_with_events(
            current,
            "complete",
            BackgroundAgentState::Succeeded,
            Some(summary),
            None,
            None,
            vec![Event::BackgroundAgentCompleted(
                BackgroundAgentCompletedEvent {
                    background_agent_id: parse_background_agent_id(background_agent_id)?,
                    summary: Some(self.safe_text(summary)),
                    at: Utc::now(),
                },
            )],
        )
        .await
    }

    pub async fn fail(
        &self,
        background_agent_id: &str,
        error: &str,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        let current = self.get(background_agent_id)?;
        ensure_state(
            "fail",
            current.state,
            &[BackgroundAgentState::Running, BackgroundAgentState::Queued],
        )?;
        self.transition_from_record_with_events(
            current,
            "fail",
            BackgroundAgentState::Failed,
            Some(error),
            None,
            None,
            vec![Event::BackgroundAgentFailed(BackgroundAgentFailedEvent {
                background_agent_id: parse_background_agent_id(background_agent_id)?,
                error: self.safe_text(error),
                at: Utc::now(),
            })],
        )
        .await
    }

    pub async fn archive(
        &self,
        background_agent_id: &str,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        let current = self.get(background_agent_id)?;
        ensure_state(
            "archive",
            current.state,
            &[
                BackgroundAgentState::Cancelled,
                BackgroundAgentState::Succeeded,
                BackgroundAgentState::Failed,
                BackgroundAgentState::Interrupted,
            ],
        )?;
        self.transition_from_record_with_events(
            current,
            "archive",
            BackgroundAgentState::Archived,
            Some("archived"),
            None,
            None,
            vec![Event::BackgroundAgentArchived(
                BackgroundAgentArchivedEvent {
                    background_agent_id: parse_background_agent_id(background_agent_id)?,
                    at: Utc::now(),
                },
            )],
        )
        .await
    }

    pub async fn delete_archived(
        &self,
        background_agent_id: &str,
    ) -> Result<(), BackgroundAgentTransitionError> {
        let record = self.get(background_agent_id)?;
        ensure_state("delete", record.state, &[BackgroundAgentState::Archived])?;
        self.append(
            &[Event::BackgroundAgentDeleted(BackgroundAgentDeletedEvent {
                background_agent_id: parse_background_agent_id(background_agent_id)?,
                at: Utc::now(),
            })],
        )
        .await?;
        self.store.delete_background_agent(background_agent_id)?;
        Ok(())
    }

    pub async fn recover_on_startup(
        &self,
        reason: &str,
    ) -> Result<Vec<BackgroundAgentRecord>, BackgroundAgentTransitionError> {
        let mut recovered = Vec::new();
        for record in self.store.list_background_agents(true)? {
            let next = match record.state {
                BackgroundAgentState::Running | BackgroundAgentState::Cancelling => {
                    BackgroundAgentState::Interrupted
                }
                BackgroundAgentState::WaitingForPermission => {
                    if has_recoverable_request(&record.payload_json, RecoverableKind::Permission) {
                        BackgroundAgentState::Recoverable
                    } else {
                        BackgroundAgentState::Interrupted
                    }
                }
                BackgroundAgentState::WaitingForInput => {
                    if has_recoverable_request(&record.payload_json, RecoverableKind::Input) {
                        BackgroundAgentState::Recoverable
                    } else {
                        BackgroundAgentState::Interrupted
                    }
                }
                _ => continue,
            };
            let additional_events = if next == BackgroundAgentState::Interrupted {
                vec![Event::BackgroundAgentInterrupted(
                    BackgroundAgentInterruptedEvent {
                        background_agent_id: parse_background_agent_id(
                            &record.background_agent_id,
                        )?,
                        reason: self.safe_text(reason),
                        at: Utc::now(),
                    },
                )]
            } else {
                Vec::new()
            };
            let transitioned = self
                .transition_from_record_with_events(
                    record.into(),
                    "startup_recovery",
                    next,
                    Some(reason),
                    None,
                    None,
                    additional_events,
                )
                .await?;
            recovered.push(transitioned);
        }
        Ok(recovered)
    }

    async fn transition(
        &self,
        background_agent_id: &str,
        operation: &'static str,
        allowed: &[BackgroundAgentState],
        to: BackgroundAgentState,
        reason: Option<&str>,
        attempt_id: Option<RunId>,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        let current = self.get(background_agent_id)?;
        ensure_state(operation, current.state, allowed)?;
        self.transition_from_record(current, operation, to, reason, attempt_id)
            .await
    }

    async fn transition_from_record(
        &self,
        current: BackgroundAgentRecord,
        operation: &'static str,
        to: BackgroundAgentState,
        reason: Option<&str>,
        attempt_id: Option<RunId>,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        self.transition_from_record_with_events(
            current,
            operation,
            to,
            reason,
            attempt_id,
            None,
            Vec::new(),
        )
        .await
    }

    async fn transition_from_record_with_events(
        &self,
        current: BackgroundAgentRecord,
        operation: &'static str,
        to: BackgroundAgentState,
        reason: Option<&str>,
        attempt_id: Option<RunId>,
        payload_json: Option<String>,
        additional_events: Vec<Event>,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        self.transition_from_record_with_attempt_and_events(
            current,
            operation,
            to,
            reason,
            attempt_id,
            payload_json,
            None,
            additional_events,
        )
        .await
    }

    async fn transition_from_record_with_attempt_and_events(
        &self,
        current: BackgroundAgentRecord,
        operation: &'static str,
        to: BackgroundAgentState,
        reason: Option<&str>,
        attempt_id: Option<RunId>,
        payload_json: Option<String>,
        attempt_record: Option<BackgroundAgentAttemptRecord>,
        mut additional_events: Vec<Event>,
    ) -> Result<BackgroundAgentRecord, BackgroundAgentTransitionError> {
        let _ = operation;
        let now = Utc::now().to_rfc3339();
        let mut events = Vec::new();
        additional_events.retain(|event| {
            if matches!(event, Event::BackgroundAgentStarted(_)) {
                events.push(event.clone());
                false
            } else {
                true
            }
        });
        events.push(Event::BackgroundAgentStateChanged(
            BackgroundAgentStateChangedEvent {
                background_agent_id: parse_background_agent_id(&current.background_agent_id)?,
                from: current.state,
                to,
                attempt_id,
                reason: reason.map(|value| self.safe_text(value)),
                at: Utc::now(),
            },
        ));
        events.append(&mut additional_events);
        self.append(&events).await?;
        if let Some(attempt_record) = attempt_record {
            self.store
                .insert_background_agent_attempt(&attempt_record)?;
        }
        match (payload_json, attempt_id) {
            (Some(payload_json), Some(attempt_id)) => {
                self.store
                    .update_background_agent_state_payload_json_and_run_id(
                        &current.background_agent_id,
                        to,
                        &payload_json,
                        &attempt_id.to_string(),
                        &now,
                    )?;
            }
            (Some(payload_json), None) => {
                self.store.update_background_agent_state_and_payload_json(
                    &current.background_agent_id,
                    to,
                    &payload_json,
                    &now,
                )?;
            }
            (None, Some(attempt_id)) => {
                self.store.update_background_agent_state_and_run_id(
                    &current.background_agent_id,
                    to,
                    &attempt_id.to_string(),
                    &now,
                )?;
            }
            (None, None) => {
                self.store
                    .update_background_agent_state(&current.background_agent_id, to, &now)?;
            }
        }
        self.get(&current.background_agent_id)
    }

    fn build_attempt_record(
        &self,
        background_agent_id: &str,
        attempt_id: RunId,
        state: BackgroundAgentState,
        prior_attempt_id: Option<String>,
    ) -> Result<BackgroundAgentAttemptRecord, BackgroundAgentTransitionError> {
        let attempt_number = self
            .store
            .list_background_agent_attempts(background_agent_id)?
            .len() as u32
            + 1;
        Ok(BackgroundAgentAttemptRecord {
            attempt_id: attempt_id.to_string(),
            background_agent_id: background_agent_id.to_owned(),
            prior_attempt_id,
            attempt_number,
            state,
            started_at: Utc::now().to_rfc3339(),
            ended_at: None,
            payload_json: "{}".to_owned(),
        })
    }

    fn latest_attempt_id(
        &self,
        background_agent_id: &str,
    ) -> Result<Option<String>, BackgroundAgentTransitionError> {
        Ok(self
            .store
            .list_background_agent_attempts(background_agent_id)?
            .last()
            .map(|attempt| attempt.attempt_id.clone()))
    }

    async fn append(&self, events: &[Event]) -> Result<(), BackgroundAgentTransitionError> {
        self.event_store
            .append_with_metadata(
                self.tenant_id,
                self.journal_session_id,
                AppendMetadata::default(),
                events,
            )
            .await?;
        Ok(())
    }

    fn safe_text(&self, value: &str) -> UiSafeText {
        let redacted = self.redactor.redact(value, &RedactRules::default());
        UiSafeText::from_redacted_display(redacted, self.redactor.as_ref())
    }
}

impl From<BackgroundAgentStoreRecord> for BackgroundAgentRecord {
    fn from(record: BackgroundAgentStoreRecord) -> Self {
        Self {
            background_agent_id: record.background_agent_id,
            conversation_id: record.conversation_id,
            run_id: record.run_id,
            state: record.state,
            title: record.title,
            created_at: record.created_at,
            updated_at: record.updated_at,
            payload_json: record.payload_json,
        }
    }
}

fn ensure_state(
    operation: &'static str,
    state: BackgroundAgentState,
    allowed: &[BackgroundAgentState],
) -> Result<(), BackgroundAgentTransitionError> {
    if allowed.contains(&state) {
        Ok(())
    } else {
        Err(BackgroundAgentTransitionError::InvalidTransition { operation, state })
    }
}

fn current_attempt_id(record: &BackgroundAgentRecord) -> Option<RunId> {
    record
        .run_id
        .as_deref()
        .and_then(|value| RunId::parse(value).ok())
}

fn current_conversation_id(record: &BackgroundAgentRecord) -> Option<SessionId> {
    SessionId::parse(&record.conversation_id).ok()
}

fn parse_background_agent_id(
    value: &str,
) -> Result<BackgroundAgentId, BackgroundAgentTransitionError> {
    BackgroundAgentId::parse(value).map_err(|error| {
        BackgroundAgentTransitionError::InvalidBackgroundAgentId(error.to_string())
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoverableKind {
    Input,
    Permission,
}

impl RecoverableKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Permission => "permission",
        }
    }
}

fn ensure_recoverable_request_matches(
    operation: &'static str,
    record: &BackgroundAgentRecord,
    expected_kind: RecoverableKind,
    request_id: RequestId,
) -> Result<(), BackgroundAgentTransitionError> {
    let actual_kind = recoverable_kind(&record.payload_json);
    if record.state == BackgroundAgentState::Recoverable || actual_kind.is_some() {
        let actual_request_id = recoverable_request_id(&record.payload_json);
        if actual_kind != Some(expected_kind)
            || actual_request_id.as_deref() != Some(&request_id.to_string())
        {
            return Err(BackgroundAgentTransitionError::InvalidTransition {
                operation,
                state: record.state,
            });
        }
    }
    Ok(())
}

fn set_recoverable_request(
    payload_json: &str,
    kind: RecoverableKind,
    request_id: RequestId,
) -> String {
    let mut payload = serde_json::from_str::<serde_json::Value>(payload_json)
        .unwrap_or_else(|_| serde_json::json!({}));
    if !payload.is_object() {
        payload = serde_json::json!({});
    }
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "backgroundRecovery".to_owned(),
            serde_json::json!({
                "kind": kind.as_str(),
                "requestId": request_id.to_string(),
            }),
        );
        match kind {
            RecoverableKind::Input => {
                object.insert(
                    "pendingInputRequest".to_owned(),
                    serde_json::Value::Bool(true),
                );
            }
            RecoverableKind::Permission => {
                object.insert(
                    "pendingPermissionDecision".to_owned(),
                    serde_json::Value::Bool(true),
                );
            }
        }
    }
    payload.to_string()
}

fn clear_recoverable_request(payload_json: &str, kind: RecoverableKind) -> String {
    let mut payload = serde_json::from_str::<serde_json::Value>(payload_json)
        .unwrap_or_else(|_| serde_json::json!({}));
    let Some(object) = payload.as_object_mut() else {
        return serde_json::json!({}).to_string();
    };
    if recoverable_kind(payload_json) == Some(kind) {
        object.remove("backgroundRecovery");
    }
    match kind {
        RecoverableKind::Input => {
            object.remove("pendingInputRequest");
        }
        RecoverableKind::Permission => {
            object.remove("pendingPermissionDecision");
            object.remove("pending_permission_decision");
        }
    }
    payload.to_string()
}

fn has_recoverable_request(payload_json: &str, kind: RecoverableKind) -> bool {
    recoverable_kind(payload_json) == Some(kind) && recoverable_request_id(payload_json).is_some()
}

fn recoverable_kind(payload_json: &str) -> Option<RecoverableKind> {
    let payload = serde_json::from_str::<serde_json::Value>(payload_json).ok()?;
    let kind = payload
        .get("backgroundRecovery")
        .and_then(|value| value.get("kind"))
        .and_then(serde_json::Value::as_str)?;
    match kind {
        "input" => Some(RecoverableKind::Input),
        "permission" => Some(RecoverableKind::Permission),
        _ => None,
    }
}

fn recoverable_request_id(payload_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(payload_json)
        .ok()
        .and_then(|payload| {
            payload
                .get("backgroundRecovery")
                .and_then(|value| value.get("requestId"))
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
}

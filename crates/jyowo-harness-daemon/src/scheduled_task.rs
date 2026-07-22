use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use harness_contracts::{
    ClientRequest, CommandId, PermissionMode, QueueItemId, RunSegmentId,
    ScheduledTaskDeletedResponse, ScheduledTaskEnabledResponse, ScheduledTaskRunRecord,
    ScheduledTaskRunResponse, ScheduledTaskRunStatus, ScheduledTaskRunsResponse,
    ScheduledTaskSavedResponse, ScheduledTaskSpec, ScheduledTasksResponse, ServerMessage, TaskId,
    TaskState, WorkspaceMode, WorkspaceSelection,
};
use harness_journal::{
    AcceptedCommand, CommandOutcome, NewTaskEvent, ScheduledTaskScheduleState, TaskStore,
    TaskStoreError,
};
use serde_json::json;
use thiserror::Error;
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;

use crate::{Supervisor, ValidatedTaskCommand};

const SCHEDULED_TASKS_FILE: &str = "scheduled-tasks.json";
const CONFIGURATION_SCHEDULED_TASK_ID: &str = "__configuration__";
const MAX_DIAGNOSTIC_BYTES: usize = 256;

#[async_trait]
pub trait ScheduledTaskTaskSubmitter: Send + Sync {
    async fn submit(
        &self,
        task_id: TaskId,
        workspace_root: Option<&Path>,
        prompt: &str,
        permission_mode: PermissionMode,
    ) -> Result<(), String>;
}

pub struct SupervisorScheduledTaskTaskSubmitter {
    store: Arc<TaskStore>,
    supervisor: Arc<Supervisor>,
}

impl SupervisorScheduledTaskTaskSubmitter {
    #[must_use]
    pub fn new(store: Arc<TaskStore>, supervisor: Arc<Supervisor>) -> Self {
        Self { store, supervisor }
    }
}

#[async_trait]
impl ScheduledTaskTaskSubmitter for SupervisorScheduledTaskTaskSubmitter {
    async fn submit(
        &self,
        task_id: TaskId,
        _workspace_root: Option<&Path>,
        prompt: &str,
        permission_mode: PermissionMode,
    ) -> Result<(), String> {
        let command_id = CommandId::new();
        let expected_stream_version = self
            .store
            .stream_version(task_id)
            .map_err(|error| bounded_diagnostic("task store", &error))?;
        let command = AcceptedCommand {
            command_id,
            task_id,
            idempotency_key: format!("scheduled_task-submit-{command_id}"),
            expected_stream_version,
            authority: TaskStore::supervisor_authority(),
            payload: json!({
                "type": "scheduled_task_submit",
                "taskId": task_id,
            }),
        };
        let outcome = self
            .supervisor
            .dispatch(
                task_id,
                ValidatedTaskCommand::SubmitMessage {
                    command,
                    queue_item_id: QueueItemId::from_u128(u128::from_be_bytes(
                        command_id.as_bytes(),
                    )),
                    segment_id: RunSegmentId::from_u128(u128::from_be_bytes(command_id.as_bytes())),
                    content: prompt.to_owned(),
                    attachments: Vec::new(),
                    context_references: Vec::new(),
                    model_config_id: None,
                    permission_mode,
                    submitted_at: Utc::now(),
                },
            )
            .await
            .map_err(|error| bounded_diagnostic("supervisor", &error))?;
        match outcome {
            CommandOutcome::Accepted { .. } => Ok(()),
            CommandOutcome::Rejected { .. } => {
                Err("scheduled task submission was rejected".to_owned())
            }
        }
    }
}

pub struct ScheduledTaskScheduler {
    store: Arc<TaskStore>,
    config_root: PathBuf,
    submitter: Arc<dyn ScheduledTaskTaskSubmitter>,
    operation_lock: Mutex<()>,
    wake: Notify,
}

impl ScheduledTaskScheduler {
    #[must_use]
    pub fn new(
        store: Arc<TaskStore>,
        config_root: PathBuf,
        submitter: Arc<dyn ScheduledTaskTaskSubmitter>,
    ) -> Self {
        Self {
            store,
            config_root,
            submitter,
            operation_lock: Mutex::new(()),
            wake: Notify::new(),
        }
    }

    pub fn start(self: &Arc<Self>) -> JoinHandle<()> {
        let scheduler = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(StdDuration::from_secs(60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = interval.tick() => {}
                    _ = scheduler.wake.notified() => {}
                }
                if let Err(error) = scheduler.tick_at(Utc::now()).await {
                    tracing::warn!(error = %error, "scheduled task scheduler tick failed");
                }
            }
        })
    }

    pub fn wake(&self) {
        self.wake.notify_one();
    }

    pub async fn handle(
        &self,
        request: ClientRequest,
    ) -> Result<ServerMessage, ScheduledTaskSchedulerError> {
        match request {
            ClientRequest::ListScheduledTasks => {
                Ok(ServerMessage::ScheduledTasks(ScheduledTasksResponse {
                    scheduled_tasks: self.list_scheduled_tasks()?,
                }))
            }
            ClientRequest::SaveScheduledTask { scheduled_task } => Ok(
                ServerMessage::ScheduledTaskSaved(ScheduledTaskSavedResponse {
                    scheduled_task: self.save_scheduled_task(scheduled_task)?,
                }),
            ),
            ClientRequest::SetScheduledTaskEnabled {
                scheduled_task_id,
                enabled,
            } => Ok(ServerMessage::ScheduledTaskEnabled(
                ScheduledTaskEnabledResponse {
                    scheduled_task: self.set_enabled(&scheduled_task_id, enabled)?,
                },
            )),
            ClientRequest::DeleteScheduledTask { scheduled_task_id } => {
                self.delete_scheduled_task(&scheduled_task_id)?;
                Ok(ServerMessage::ScheduledTaskDeleted(
                    ScheduledTaskDeletedResponse { scheduled_task_id },
                ))
            }
            ClientRequest::RunScheduledTaskNow { scheduled_task_id } => {
                Ok(ServerMessage::ScheduledTaskRun(ScheduledTaskRunResponse {
                    run: self.run_now(&scheduled_task_id, Utc::now()).await?,
                }))
            }
            ClientRequest::ListScheduledTaskRuns { scheduled_task_id } => Ok(
                ServerMessage::ScheduledTaskRuns(ScheduledTaskRunsResponse {
                    runs: self.list_runs(scheduled_task_id.as_deref())?,
                }),
            ),
            _ => Err(ScheduledTaskSchedulerError::InvalidConfiguration),
        }
    }

    pub fn list_scheduled_tasks(
        &self,
    ) -> Result<Vec<ScheduledTaskSpec>, ScheduledTaskSchedulerError> {
        self.register_scope()?;
        self.load_scheduled_tasks()
    }

    pub fn save_scheduled_task(
        &self,
        mut scheduled_task: ScheduledTaskSpec,
    ) -> Result<ScheduledTaskSpec, ScheduledTaskSchedulerError> {
        scheduled_task.workspace_root = canonical_workspace_string(scheduled_task.workspace_root)?;
        validate_scheduled_task(&scheduled_task)?;
        let scope_key = self.register_scope()?;
        let path = self.scheduled_tasks_path();
        let mut records = load_scheduled_task_file(&path)?;
        records.retain(|record| record.id != scheduled_task.id);
        records.push(scheduled_task.clone());
        records.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
        write_scheduled_task_file(&path, &records)?;
        if let Some(mut state) = self
            .store
            .scheduled_task_schedule_state(&scope_key, &scheduled_task.id)?
        {
            state.cursor_at = scheduled_task.updated_at;
            state.next_due_at = scheduled_task.updated_at
                + Duration::minutes(i64::from(scheduled_task.schedule.interval_minutes));
            self.store.save_scheduled_task_schedule_state(
                &scope_key,
                &scheduled_task.id,
                &state,
            )?;
        }
        self.wake();
        Ok(scheduled_task)
    }

    pub fn set_enabled(
        &self,
        scheduled_task_id: &str,
        enabled: bool,
    ) -> Result<ScheduledTaskSpec, ScheduledTaskSchedulerError> {
        validate_scheduled_task_id(scheduled_task_id)?;
        self.register_scope()?;
        let path = self.scheduled_tasks_path();
        let mut records = load_scheduled_task_file(&path)?;
        let record = records
            .iter_mut()
            .find(|record| record.id == scheduled_task_id)
            .ok_or(ScheduledTaskSchedulerError::NotFound)?;
        record.enabled = enabled;
        record.updated_at = Utc::now();
        let result = record.clone();
        write_scheduled_task_file(&path, &records)?;
        self.wake();
        Ok(result)
    }

    pub fn delete_scheduled_task(
        &self,
        scheduled_task_id: &str,
    ) -> Result<(), ScheduledTaskSchedulerError> {
        validate_scheduled_task_id(scheduled_task_id)?;
        let scope_key = self.register_scope()?;
        let path = self.scheduled_tasks_path();
        let mut records = load_scheduled_task_file(&path)?;
        let before = records.len();
        records.retain(|record| record.id != scheduled_task_id);
        if records.len() == before {
            return Err(ScheduledTaskSchedulerError::NotFound);
        }
        write_scheduled_task_file(&path, &records)?;
        self.store
            .delete_scheduled_task_schedule_state(&scope_key, scheduled_task_id)?;
        self.wake();
        Ok(())
    }

    pub fn list_runs(
        &self,
        scheduled_task_id: Option<&str>,
    ) -> Result<Vec<ScheduledTaskRunRecord>, ScheduledTaskSchedulerError> {
        let scope_key = self.register_scope()?;
        Ok(self
            .store
            .scheduled_task_runs(&scope_key, scheduled_task_id)?)
    }

    pub async fn run_now(
        &self,
        scheduled_task_id: &str,
        now: DateTime<Utc>,
    ) -> Result<ScheduledTaskRunRecord, ScheduledTaskSchedulerError> {
        let scope_key = self.register_scope()?;
        let scheduled_task = self
            .load_scheduled_tasks()?
            .into_iter()
            .find(|record| record.id == scheduled_task_id)
            .ok_or(ScheduledTaskSchedulerError::NotFound)?;
        self.run_scheduled_task(&scope_key, &scheduled_task, now)
            .await
    }

    pub async fn tick_at(&self, now: DateTime<Utc>) -> Result<(), ScheduledTaskSchedulerError> {
        let scope_key = self.register_scope()?;
        let scheduled_tasks = match self.load_scheduled_tasks() {
            Ok(records) => records,
            Err(_error) => {
                self.record_configuration_rejection(&scope_key, now)?;
                return Ok(());
            }
        };
        for scheduled_task in scheduled_tasks.into_iter().filter(|record| record.enabled) {
            let interval = Duration::minutes(i64::from(scheduled_task.schedule.interval_minutes));
            let mut state = self
                .store
                .scheduled_task_schedule_state(&scope_key, &scheduled_task.id)?
                .unwrap_or(ScheduledTaskScheduleState {
                    cursor_at: scheduled_task.updated_at,
                    next_due_at: scheduled_task.updated_at + interval,
                    active_task_id: None,
                });
            if let Some(task_id) = state.active_task_id {
                if scheduled_task_terminal_status(&self.store, task_id)?.is_none() {
                    self.store.save_scheduled_task_schedule_state(
                        &scope_key,
                        &scheduled_task.id,
                        &state,
                    )?;
                    continue;
                }
                let status = scheduled_task_terminal_status(&self.store, task_id)?
                    .unwrap_or(ScheduledTaskRunStatus::Failed);
                self.store
                    .complete_scheduled_task_run_for_task(&scope_key, task_id, now, status)?;
                state.active_task_id = None;
            }
            if now < state.next_due_at {
                self.store.save_scheduled_task_schedule_state(
                    &scope_key,
                    &scheduled_task.id,
                    &state,
                )?;
                continue;
            }
            let missed_multiple = now >= state.next_due_at + interval;
            if missed_multiple
                && scheduled_task.missed_run_policy == harness_contracts::MissedRunPolicy::Skip
            {
                state.cursor_at = now;
                state.next_due_at = now + interval;
                self.store.save_scheduled_task_schedule_state(
                    &scope_key,
                    &scheduled_task.id,
                    &state,
                )?;
                continue;
            }
            let _ = self
                .run_scheduled_task(&scope_key, &scheduled_task, now)
                .await?;
        }
        Ok(())
    }

    async fn run_scheduled_task(
        &self,
        scope_key: &str,
        scheduled_task: &ScheduledTaskSpec,
        now: DateTime<Utc>,
    ) -> Result<ScheduledTaskRunRecord, ScheduledTaskSchedulerError> {
        let _guard = self.operation_lock.lock().await;
        let interval = Duration::minutes(i64::from(scheduled_task.schedule.interval_minutes));
        let mut current_state = self
            .store
            .scheduled_task_schedule_state(scope_key, &scheduled_task.id)?;
        if let Some(active_task_id) = current_state
            .as_ref()
            .and_then(|state| state.active_task_id)
        {
            if let Some(status) = scheduled_task_terminal_status(&self.store, active_task_id)? {
                self.store.complete_scheduled_task_run_for_task(
                    scope_key,
                    active_task_id,
                    now,
                    status,
                )?;
                if let Some(state) = current_state.as_mut() {
                    state.active_task_id = None;
                }
            }
        }
        if current_state
            .as_ref()
            .and_then(|state| state.active_task_id)
            .is_some()
        {
            let record = run_record(
                scheduled_task,
                now,
                ScheduledTaskRunStatus::Rejected,
                None,
                Some("scheduled task already has an active run"),
            );
            self.store.append_scheduled_task_run(scope_key, &record)?;
            return Ok(record);
        }

        let workspace_root = scheduled_task.workspace_root.as_deref().map(Path::new);
        let task_id = TaskId::new();
        self.create_task(task_id, workspace_root, scheduled_task)?;
        let submission = self
            .submitter
            .submit(
                task_id,
                workspace_root,
                &scheduled_task.prompt,
                scheduled_task.permission_mode,
            )
            .await;
        let (status, message, active_task_id) = match submission {
            Ok(()) => (ScheduledTaskRunStatus::Started, None, Some(task_id)),
            Err(message) => (
                ScheduledTaskRunStatus::Failed,
                Some(bounded_message(&message)),
                None,
            ),
        };
        let record = run_record(
            scheduled_task,
            now,
            status,
            Some(task_id),
            message.as_deref(),
        );
        self.store.append_scheduled_task_run(scope_key, &record)?;
        self.store.save_scheduled_task_schedule_state(
            scope_key,
            &scheduled_task.id,
            &ScheduledTaskScheduleState {
                cursor_at: now,
                next_due_at: now + interval,
                active_task_id,
            },
        )?;
        Ok(record)
    }

    fn create_task(
        &self,
        task_id: TaskId,
        workspace_root: Option<&Path>,
        scheduled_task: &ScheduledTaskSpec,
    ) -> Result<(), ScheduledTaskSchedulerError> {
        let command_id = CommandId::from_u128(u128::from_be_bytes(task_id.as_bytes()));
        let command = AcceptedCommand {
            command_id,
            task_id,
            idempotency_key: format!("scheduled_task-create-{task_id}"),
            expected_stream_version: 0,
            authority: TaskStore::supervisor_authority(),
            payload: json!({ "scheduledTaskId": scheduled_task.id, "type": "scheduled_task_create_task" }),
        };
        let title = format!("Scheduled task: {}", scheduled_task.name);
        self.store.transact_command(command, |_| {
            Ok(vec![match workspace_root {
                Some(workspace_root) => NewTaskEvent::task_created_in_workspace(
                    title,
                    WorkspaceSelection {
                        mode: WorkspaceMode::Current,
                        root: workspace_root.to_string_lossy().into_owned(),
                    },
                ),
                None => NewTaskEvent::task_created(title),
            }])
        })?;
        Ok(())
    }

    fn register_scope(&self) -> Result<String, ScheduledTaskSchedulerError> {
        Ok(self.store.register_scheduled_task_scope(None)?)
    }

    fn scheduled_tasks_path(&self) -> PathBuf {
        self.config_root.join(SCHEDULED_TASKS_FILE)
    }

    fn load_scheduled_tasks(&self) -> Result<Vec<ScheduledTaskSpec>, ScheduledTaskSchedulerError> {
        let mut records = load_scheduled_task_file(&self.scheduled_tasks_path())?;
        for record in &records {
            validate_scheduled_task(&record)?;
        }
        records.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
        Ok(records)
    }

    fn record_configuration_rejection(
        &self,
        scope_key: &str,
        now: DateTime<Utc>,
    ) -> Result<(), ScheduledTaskSchedulerError> {
        let record = ScheduledTaskRunRecord {
            scheduled_task_id: CONFIGURATION_SCHEDULED_TASK_ID.to_owned(),
            completed_at: Some(now),
            id: CommandId::new().to_string(),
            message: Some("scheduled task configuration is invalid".to_owned()),
            task_id: None,
            started_at: now,
            status: ScheduledTaskRunStatus::Rejected,
        };
        self.store.append_scheduled_task_run(scope_key, &record)?;
        Ok(())
    }
}

fn run_record(
    scheduled_task: &ScheduledTaskSpec,
    now: DateTime<Utc>,
    status: ScheduledTaskRunStatus,
    task_id: Option<TaskId>,
    message: Option<&str>,
) -> ScheduledTaskRunRecord {
    ScheduledTaskRunRecord {
        scheduled_task_id: scheduled_task.id.clone(),
        completed_at: (status != ScheduledTaskRunStatus::Started).then_some(now),
        id: CommandId::new().to_string(),
        message: message.map(bounded_message),
        task_id: task_id.map(|task_id| task_id.to_string()),
        started_at: now,
        status,
    }
}

fn scheduled_task_terminal_status(
    store: &TaskStore,
    task_id: TaskId,
) -> Result<Option<ScheduledTaskRunStatus>, TaskStoreError> {
    let Some(task) = store.task_projection(task_id)? else {
        return Ok(Some(ScheduledTaskRunStatus::Failed));
    };
    if task.removed {
        return Ok(Some(ScheduledTaskRunStatus::Cancelled));
    }
    Ok(match task.state {
        TaskState::Completed => Some(ScheduledTaskRunStatus::Succeeded),
        TaskState::Failed => Some(ScheduledTaskRunStatus::Failed),
        TaskState::Interrupted => Some(ScheduledTaskRunStatus::Cancelled),
        TaskState::Idle
        | TaskState::Running
        | TaskState::WaitingPermission
        | TaskState::WaitingInput
        | TaskState::Yielding => None,
    })
}

fn canonical_workspace_string(
    workspace_root: Option<String>,
) -> Result<Option<String>, ScheduledTaskSchedulerError> {
    workspace_root
        .map(|root| {
            Path::new(&root)
                .canonicalize()
                .map(|path| path.to_string_lossy().into_owned())
                .map_err(|_| ScheduledTaskSchedulerError::InvalidConfiguration)
        })
        .transpose()
}

fn load_scheduled_task_file(
    path: &Path,
) -> Result<Vec<ScheduledTaskSpec>, ScheduledTaskSchedulerError> {
    match std::fs::read(path) {
        Ok(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => Ok(Vec::new()),
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| ScheduledTaskSchedulerError::InvalidConfiguration),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

fn write_scheduled_task_file(
    path: &Path,
    records: &[ScheduledTaskSpec],
) -> Result<(), ScheduledTaskSchedulerError> {
    let parent = path
        .parent()
        .ok_or(ScheduledTaskSchedulerError::InvalidConfiguration)?;
    std::fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(records)?)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

fn validate_scheduled_task(
    scheduled_task: &ScheduledTaskSpec,
) -> Result<(), ScheduledTaskSchedulerError> {
    validate_scheduled_task_id(&scheduled_task.id)?;
    if scheduled_task.name.trim().is_empty() || scheduled_task.name.len() > 128 {
        return Err(ScheduledTaskSchedulerError::InvalidConfiguration);
    }
    if scheduled_task.prompt.trim().is_empty() || scheduled_task.prompt.len() > 64 * 1024 {
        return Err(ScheduledTaskSchedulerError::InvalidConfiguration);
    }
    if scheduled_task.schedule.interval_minutes == 0 {
        return Err(ScheduledTaskSchedulerError::InvalidConfiguration);
    }
    Ok(())
}

fn validate_scheduled_task_id(id: &str) -> Result<(), ScheduledTaskSchedulerError> {
    if id.is_empty()
        || id.len() > 128
        || !id.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && b"._-".contains(&byte))
        })
    {
        return Err(ScheduledTaskSchedulerError::InvalidConfiguration);
    }
    Ok(())
}

fn bounded_message(message: &str) -> String {
    message.chars().take(MAX_DIAGNOSTIC_BYTES).collect()
}

fn bounded_diagnostic(kind: &str, _error: &dyn std::fmt::Display) -> String {
    bounded_message(&format!("scheduled task {kind} operation failed"))
}

#[derive(Debug, Error)]
pub enum ScheduledTaskSchedulerError {
    #[error("scheduled task configuration is invalid")]
    InvalidConfiguration,
    #[error("scheduled task not found")]
    NotFound,
    #[error("scheduled task storage failed")]
    Io(#[from] std::io::Error),
    #[error("scheduled task data encoding failed")]
    Json(#[from] serde_json::Error),
    #[error("scheduled task journal failed")]
    Store(#[from] TaskStoreError),
}

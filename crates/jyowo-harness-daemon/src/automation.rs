use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use harness_contracts::{
    AutomationDeletedResponse, AutomationEnabledResponse, AutomationRunRecord,
    AutomationRunResponse, AutomationRunStatus, AutomationRunsResponse, AutomationSavedResponse,
    AutomationSpec, AutomationsResponse, ClientRequest, CommandId, PermissionMode, QueueItemId,
    RunSegmentId, RunState, ServerMessage, TaskId, WorkspaceMode, WorkspaceSelection,
};
use harness_journal::{
    AcceptedCommand, AutomationScheduleState, CommandOutcome, NewTaskEvent, TaskStore,
    TaskStoreError,
};
use serde_json::json;
use thiserror::Error;
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;

use crate::{Supervisor, ValidatedTaskCommand};

const AUTOMATIONS_FILE: &str = "automations.json";
const CONFIGURATION_AUTOMATION_ID: &str = "__configuration__";
const MAX_DIAGNOSTIC_BYTES: usize = 256;

#[async_trait]
pub trait AutomationTaskSubmitter: Send + Sync {
    async fn submit(
        &self,
        task_id: TaskId,
        workspace_root: Option<&Path>,
        prompt: &str,
        permission_mode: PermissionMode,
    ) -> Result<(), String>;
}

pub struct SupervisorAutomationTaskSubmitter {
    store: Arc<TaskStore>,
    supervisor: Arc<Supervisor>,
}

impl SupervisorAutomationTaskSubmitter {
    #[must_use]
    pub fn new(store: Arc<TaskStore>, supervisor: Arc<Supervisor>) -> Self {
        Self { store, supervisor }
    }
}

#[async_trait]
impl AutomationTaskSubmitter for SupervisorAutomationTaskSubmitter {
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
            idempotency_key: format!("automation-submit-{command_id}"),
            expected_stream_version,
            authority: TaskStore::supervisor_authority(),
            payload: json!({
                "type": "automation_submit",
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
                Err("automation task submission was rejected".to_owned())
            }
        }
    }
}

pub struct AutomationScheduler {
    store: Arc<TaskStore>,
    config_root: PathBuf,
    submitter: Arc<dyn AutomationTaskSubmitter>,
    operation_lock: Mutex<()>,
    wake: Notify,
}

impl AutomationScheduler {
    #[must_use]
    pub fn new(
        store: Arc<TaskStore>,
        config_root: PathBuf,
        submitter: Arc<dyn AutomationTaskSubmitter>,
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
                    tracing::warn!(error = %error, "automation scheduler tick failed");
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
    ) -> Result<ServerMessage, AutomationSchedulerError> {
        match request {
            ClientRequest::ListAutomations { workspace_root } => {
                Ok(ServerMessage::Automations(AutomationsResponse {
                    automations: self.list_automations(workspace_root.as_deref().map(Path::new))?,
                }))
            }
            ClientRequest::SaveAutomation {
                workspace_root,
                automation,
            } => Ok(ServerMessage::AutomationSaved(AutomationSavedResponse {
                automation: self
                    .save_automation(workspace_root.as_deref().map(Path::new), automation)?,
            })),
            ClientRequest::SetAutomationEnabled {
                workspace_root,
                automation_id,
                enabled,
            } => Ok(ServerMessage::AutomationEnabled(
                AutomationEnabledResponse {
                    automation: self.set_enabled(
                        workspace_root.as_deref().map(Path::new),
                        &automation_id,
                        enabled,
                    )?,
                },
            )),
            ClientRequest::DeleteAutomation {
                workspace_root,
                automation_id,
            } => {
                self.delete_automation(workspace_root.as_deref().map(Path::new), &automation_id)?;
                Ok(ServerMessage::AutomationDeleted(
                    AutomationDeletedResponse { automation_id },
                ))
            }
            ClientRequest::RunAutomationNow {
                workspace_root,
                automation_id,
            } => Ok(ServerMessage::AutomationRun(AutomationRunResponse {
                run: self
                    .run_now(
                        workspace_root.as_deref().map(Path::new),
                        &automation_id,
                        Utc::now(),
                    )
                    .await?,
            })),
            ClientRequest::ListAutomationRuns {
                workspace_root,
                automation_id,
            } => Ok(ServerMessage::AutomationRuns(AutomationRunsResponse {
                runs: self.list_runs(
                    workspace_root.as_deref().map(Path::new),
                    automation_id.as_deref(),
                )?,
            })),
            _ => Err(AutomationSchedulerError::InvalidConfiguration),
        }
    }

    pub fn list_automations(
        &self,
        workspace_root: Option<&Path>,
    ) -> Result<Vec<AutomationSpec>, AutomationSchedulerError> {
        let workspace_root = canonical_workspace(workspace_root)?;
        self.register_scope(workspace_root.as_deref())?;
        self.load_effective_automations(workspace_root.as_deref())
    }

    pub fn save_automation(
        &self,
        workspace_root: Option<&Path>,
        automation: AutomationSpec,
    ) -> Result<AutomationSpec, AutomationSchedulerError> {
        validate_automation(&automation)?;
        let workspace_root = canonical_workspace(workspace_root)?;
        self.register_scope(workspace_root.as_deref())?;
        let path = self.automations_path(workspace_root.as_deref());
        let mut records = load_automation_file(&path)?;
        records.retain(|record| record.id != automation.id);
        records.push(automation.clone());
        records.sort_by(|left, right| left.id.cmp(&right.id));
        write_automation_file(&path, &records)?;
        self.wake();
        Ok(automation)
    }

    pub fn set_enabled(
        &self,
        workspace_root: Option<&Path>,
        automation_id: &str,
        enabled: bool,
    ) -> Result<AutomationSpec, AutomationSchedulerError> {
        validate_automation_id(automation_id)?;
        let workspace_root = canonical_workspace(workspace_root)?;
        self.register_scope(workspace_root.as_deref())?;
        let path = self.automations_path(workspace_root.as_deref());
        let mut records = load_automation_file(&path)?;
        let record = records
            .iter_mut()
            .find(|record| record.id == automation_id)
            .ok_or(AutomationSchedulerError::NotFound)?;
        record.enabled = enabled;
        record.updated_at = Utc::now();
        let result = record.clone();
        write_automation_file(&path, &records)?;
        self.wake();
        Ok(result)
    }

    pub fn delete_automation(
        &self,
        workspace_root: Option<&Path>,
        automation_id: &str,
    ) -> Result<(), AutomationSchedulerError> {
        validate_automation_id(automation_id)?;
        let workspace_root = canonical_workspace(workspace_root)?;
        self.register_scope(workspace_root.as_deref())?;
        let path = self.automations_path(workspace_root.as_deref());
        let mut records = load_automation_file(&path)?;
        records.retain(|record| record.id != automation_id);
        write_automation_file(&path, &records)?;
        self.wake();
        Ok(())
    }

    pub fn list_runs(
        &self,
        workspace_root: Option<&Path>,
        automation_id: Option<&str>,
    ) -> Result<Vec<AutomationRunRecord>, AutomationSchedulerError> {
        let workspace_root = canonical_workspace(workspace_root)?;
        let scope_key = self.register_scope(workspace_root.as_deref())?;
        Ok(self.store.automation_runs(&scope_key, automation_id)?)
    }

    pub async fn run_now(
        &self,
        workspace_root: Option<&Path>,
        automation_id: &str,
        now: DateTime<Utc>,
    ) -> Result<AutomationRunRecord, AutomationSchedulerError> {
        let workspace_root = canonical_workspace(workspace_root)?;
        let scope_key = self.register_scope(workspace_root.as_deref())?;
        let automation = self
            .load_effective_automations(workspace_root.as_deref())?
            .into_iter()
            .find(|record| record.id == automation_id)
            .ok_or(AutomationSchedulerError::NotFound)?;
        self.run_automation(&scope_key, workspace_root.as_deref(), &automation, now)
            .await
    }

    pub async fn tick_at(&self, now: DateTime<Utc>) -> Result<(), AutomationSchedulerError> {
        let mut scopes = self.store.automation_scopes()?;
        if scopes.is_empty() {
            scopes.push(None);
        }
        for workspace_root in scopes {
            let workspace_root = match canonical_workspace(workspace_root.as_deref()) {
                Ok(root) => root,
                Err(error) => {
                    self.record_configuration_rejection(None, now, &error)?;
                    continue;
                }
            };
            let scope_key = self.register_scope(workspace_root.as_deref())?;
            let automations = match self.load_effective_automations(workspace_root.as_deref()) {
                Ok(records) => records,
                Err(error) => {
                    self.record_configuration_rejection(Some(&scope_key), now, &error)?;
                    continue;
                }
            };
            for automation in automations.into_iter().filter(|record| record.enabled) {
                let interval = Duration::minutes(i64::from(automation.schedule.interval_minutes));
                let mut state = self
                    .store
                    .automation_schedule_state(&scope_key, &automation.id)?
                    .unwrap_or(AutomationScheduleState {
                        cursor_at: automation.updated_at,
                        next_due_at: automation.updated_at + interval,
                        active_task_id: None,
                    });
                if let Some(task_id) = state.active_task_id {
                    if task_is_active(&self.store, task_id)? {
                        self.store.save_automation_schedule_state(
                            &scope_key,
                            &automation.id,
                            &state,
                        )?;
                        continue;
                    }
                    self.store
                        .complete_automation_run_for_task(&scope_key, task_id, now)?;
                    state.active_task_id = None;
                }
                if now < state.next_due_at {
                    self.store.save_automation_schedule_state(
                        &scope_key,
                        &automation.id,
                        &state,
                    )?;
                    continue;
                }
                let missed_multiple = now >= state.next_due_at + interval;
                if missed_multiple
                    && automation.missed_run_policy == harness_contracts::MissedRunPolicy::Skip
                {
                    state.cursor_at = now;
                    state.next_due_at = now + interval;
                    self.store.save_automation_schedule_state(
                        &scope_key,
                        &automation.id,
                        &state,
                    )?;
                    continue;
                }
                let _ = self
                    .run_automation(&scope_key, workspace_root.as_deref(), &automation, now)
                    .await?;
            }
        }
        Ok(())
    }

    async fn run_automation(
        &self,
        scope_key: &str,
        workspace_root: Option<&Path>,
        automation: &AutomationSpec,
        now: DateTime<Utc>,
    ) -> Result<AutomationRunRecord, AutomationSchedulerError> {
        let _guard = self.operation_lock.lock().await;
        let interval = Duration::minutes(i64::from(automation.schedule.interval_minutes));
        if self
            .store
            .automation_schedule_state(scope_key, &automation.id)?
            .and_then(|state| state.active_task_id)
            .is_some()
        {
            let record = run_record(
                automation,
                now,
                AutomationRunStatus::Rejected,
                None,
                Some("automation already has an active run"),
            );
            self.store.append_automation_run(scope_key, &record)?;
            return Ok(record);
        }

        let task_id = TaskId::new();
        self.create_task(task_id, workspace_root, automation)?;
        let submission = self
            .submitter
            .submit(
                task_id,
                workspace_root,
                &automation.prompt,
                automation.permission_mode,
            )
            .await;
        let (status, message, active_task_id) = match submission {
            Ok(()) => (AutomationRunStatus::Started, None, Some(task_id)),
            Err(message) => (
                AutomationRunStatus::Failed,
                Some(bounded_message(&message)),
                None,
            ),
        };
        let record = run_record(automation, now, status, Some(task_id), message.as_deref());
        self.store.append_automation_run(scope_key, &record)?;
        self.store.save_automation_schedule_state(
            scope_key,
            &automation.id,
            &AutomationScheduleState {
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
        automation: &AutomationSpec,
    ) -> Result<(), AutomationSchedulerError> {
        let command_id = CommandId::from_u128(u128::from_be_bytes(task_id.as_bytes()));
        let command = AcceptedCommand {
            command_id,
            task_id,
            idempotency_key: format!("automation-create-{task_id}"),
            expected_stream_version: 0,
            authority: TaskStore::supervisor_authority(),
            payload: json!({ "automationId": automation.id, "type": "automation_create_task" }),
        };
        let title = format!("Automation: {}", automation.id);
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

    fn register_scope(
        &self,
        workspace_root: Option<&Path>,
    ) -> Result<String, AutomationSchedulerError> {
        Ok(self.store.register_automation_scope(workspace_root)?)
    }

    fn automations_path(&self, workspace_root: Option<&Path>) -> PathBuf {
        workspace_root.map_or_else(
            || self.config_root.join(AUTOMATIONS_FILE),
            |workspace_root| workspace_root.join(".jyowo/config").join(AUTOMATIONS_FILE),
        )
    }

    fn load_effective_automations(
        &self,
        workspace_root: Option<&Path>,
    ) -> Result<Vec<AutomationSpec>, AutomationSchedulerError> {
        let mut by_id = BTreeMap::new();
        for record in load_automation_file(&self.config_root.join(AUTOMATIONS_FILE))? {
            validate_automation(&record)?;
            by_id.insert(record.id.clone(), record);
        }
        if let Some(workspace_root) = workspace_root {
            for record in load_automation_file(&self.automations_path(Some(workspace_root)))? {
                validate_automation(&record)?;
                by_id.insert(record.id.clone(), record);
            }
        }
        Ok(by_id.into_values().collect())
    }

    fn record_configuration_rejection(
        &self,
        scope_key: Option<&str>,
        now: DateTime<Utc>,
        _error: &AutomationSchedulerError,
    ) -> Result<(), AutomationSchedulerError> {
        let scope_key = match scope_key {
            Some(scope_key) => scope_key.to_owned(),
            None => self.register_scope(None)?,
        };
        let record = AutomationRunRecord {
            automation_id: CONFIGURATION_AUTOMATION_ID.to_owned(),
            completed_at: Some(now),
            id: CommandId::new().to_string(),
            message: Some("automation configuration is invalid".to_owned()),
            run_id: None,
            started_at: now,
            status: AutomationRunStatus::Rejected,
        };
        self.store.append_automation_run(&scope_key, &record)?;
        Ok(())
    }
}

fn run_record(
    automation: &AutomationSpec,
    now: DateTime<Utc>,
    status: AutomationRunStatus,
    task_id: Option<TaskId>,
    message: Option<&str>,
) -> AutomationRunRecord {
    AutomationRunRecord {
        automation_id: automation.id.clone(),
        completed_at: (status != AutomationRunStatus::Started).then_some(now),
        id: CommandId::new().to_string(),
        message: message.map(bounded_message),
        run_id: task_id.map(|task_id| task_id.to_string()),
        started_at: now,
        status,
    }
}

fn task_is_active(store: &TaskStore, task_id: TaskId) -> Result<bool, TaskStoreError> {
    let Some(task) = store.task_projection(task_id)? else {
        return Ok(false);
    };
    if task.removed {
        return Ok(false);
    }
    Ok(task.current_run.is_none_or(|run| {
        matches!(
            run.state,
            RunState::Running | RunState::WaitingPermission | RunState::Yielding
        )
    }))
}

fn canonical_workspace(
    workspace_root: Option<&Path>,
) -> Result<Option<PathBuf>, AutomationSchedulerError> {
    workspace_root
        .map(|root| {
            root.canonicalize()
                .map_err(|_| AutomationSchedulerError::InvalidConfiguration)
        })
        .transpose()
}

fn load_automation_file(path: &Path) -> Result<Vec<AutomationSpec>, AutomationSchedulerError> {
    match std::fs::read(path) {
        Ok(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => Ok(Vec::new()),
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| AutomationSchedulerError::InvalidConfiguration),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

fn write_automation_file(
    path: &Path,
    records: &[AutomationSpec],
) -> Result<(), AutomationSchedulerError> {
    let parent = path
        .parent()
        .ok_or(AutomationSchedulerError::InvalidConfiguration)?;
    std::fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(records)?)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

fn validate_automation(automation: &AutomationSpec) -> Result<(), AutomationSchedulerError> {
    validate_automation_id(&automation.id)?;
    if automation.prompt.trim().is_empty() || automation.prompt.len() > 64 * 1024 {
        return Err(AutomationSchedulerError::InvalidConfiguration);
    }
    if automation.schedule.interval_minutes == 0 {
        return Err(AutomationSchedulerError::InvalidConfiguration);
    }
    Ok(())
}

fn validate_automation_id(id: &str) -> Result<(), AutomationSchedulerError> {
    if id.is_empty()
        || id.len() > 128
        || !id.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && b"._-".contains(&byte))
        })
    {
        return Err(AutomationSchedulerError::InvalidConfiguration);
    }
    Ok(())
}

fn bounded_message(message: &str) -> String {
    message.chars().take(MAX_DIAGNOSTIC_BYTES).collect()
}

fn bounded_diagnostic(kind: &str, _error: &dyn std::fmt::Display) -> String {
    bounded_message(&format!("automation {kind} operation failed"))
}

#[derive(Debug, Error)]
pub enum AutomationSchedulerError {
    #[error("automation configuration is invalid")]
    InvalidConfiguration,
    #[error("automation not found")]
    NotFound,
    #[error("automation storage failed")]
    Io(#[from] std::io::Error),
    #[error("automation data encoding failed")]
    Json(#[from] serde_json::Error),
    #[error("automation journal failed")]
    Store(#[from] TaskStoreError),
}

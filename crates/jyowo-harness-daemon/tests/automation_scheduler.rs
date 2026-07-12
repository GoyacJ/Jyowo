use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{Duration, TimeZone, Utc};
use harness_contracts::{
    AutomationRunStatus, AutomationSchedule, AutomationSpec, AutomationWorkspaceScope,
    MissedRunPolicy, PermissionMode, SandboxMode, TaskId, ToolProfile, WorkspaceAccess,
};
use harness_daemon::{AutomationScheduler, AutomationTaskSubmitter};
use harness_journal::TaskStore;
use tempfile::TempDir;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Submission {
    task_id: TaskId,
    workspace_root: Option<PathBuf>,
    prompt: String,
}

#[derive(Default)]
struct RecordingSubmitter {
    submissions: Mutex<Vec<Submission>>,
}

#[async_trait]
impl AutomationTaskSubmitter for RecordingSubmitter {
    async fn submit(
        &self,
        task_id: TaskId,
        workspace_root: Option<&Path>,
        prompt: &str,
        _permission_mode: PermissionMode,
    ) -> Result<(), String> {
        self.submissions.lock().unwrap().push(Submission {
            task_id,
            workspace_root: workspace_root.map(Path::to_path_buf),
            prompt: prompt.to_owned(),
        });
        Ok(())
    }
}

struct Fixture {
    _temp: TempDir,
    config_root: PathBuf,
    store: Arc<TaskStore>,
    submitter: Arc<RecordingSubmitter>,
    workspace: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let config_root = temp.path().join("home/config");
        std::fs::create_dir_all(&config_root).unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let store = Arc::new(TaskStore::open(temp.path().join("tasks.sqlite")).unwrap());
        Self {
            _temp: temp,
            config_root,
            store,
            submitter: Arc::new(RecordingSubmitter::default()),
            workspace,
        }
    }

    fn scheduler(&self) -> AutomationScheduler {
        AutomationScheduler::new(
            Arc::clone(&self.store),
            self.config_root.clone(),
            self.submitter.clone(),
        )
    }
}

fn timestamp(hour: u32, minute: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 12, hour, minute, 0).unwrap()
}

fn automation(policy: MissedRunPolicy, updated_at: chrono::DateTime<Utc>) -> AutomationSpec {
    AutomationSpec {
        id: "checks".to_owned(),
        enabled: true,
        prompt: "Run checks".to_owned(),
        schedule: AutomationSchedule {
            interval_minutes: 30,
        },
        tool_profile: ToolProfile::Coding,
        permission_mode: PermissionMode::Default,
        sandbox_mode: SandboxMode::None,
        workspace_scope: AutomationWorkspaceScope::CurrentWorkspace,
        workspace_access: WorkspaceAccess::ReadOnly,
        missed_run_policy: policy,
        created_at: updated_at,
        updated_at,
    }
}

#[tokio::test]
async fn run_now_creates_one_daemon_task_and_submits_the_saved_prompt() {
    let fixture = Fixture::new();
    let scheduler = fixture.scheduler();
    scheduler
        .save_automation(
            Some(&fixture.workspace),
            automation(MissedRunPolicy::Skip, timestamp(1, 0)),
        )
        .unwrap();

    let run = scheduler
        .run_now(Some(&fixture.workspace), "checks", timestamp(1, 5))
        .await
        .unwrap();

    assert_eq!(run.status, AutomationRunStatus::Started);
    let task_id = TaskId::parse(run.run_id.as_deref().unwrap()).unwrap();
    assert!(fixture.store.task_projection(task_id).unwrap().is_some());
    assert_eq!(
        fixture.submitter.submissions.lock().unwrap().as_slice(),
        &[Submission {
            task_id,
            workspace_root: Some(fixture.workspace.canonicalize().unwrap()),
            prompt: "Run checks".to_owned(),
        }]
    );
}

#[tokio::test]
async fn a_second_request_is_rejected_while_the_automation_task_is_active() {
    let fixture = Fixture::new();
    let scheduler = fixture.scheduler();
    scheduler
        .save_automation(None, automation(MissedRunPolicy::Skip, timestamp(1, 0)))
        .unwrap();

    scheduler
        .run_now(None, "checks", timestamp(1, 5))
        .await
        .unwrap();
    let rejected = scheduler
        .run_now(None, "checks", timestamp(1, 6))
        .await
        .unwrap();

    assert_eq!(rejected.status, AutomationRunStatus::Rejected);
    assert_eq!(fixture.submitter.submissions.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn one_interval_creates_one_run_after_the_due_time() {
    let fixture = Fixture::new();
    let scheduler = fixture.scheduler();
    scheduler
        .save_automation(None, automation(MissedRunPolicy::Skip, timestamp(1, 0)))
        .unwrap();

    scheduler.tick_at(timestamp(1, 30)).await.unwrap();

    assert_eq!(fixture.submitter.submissions.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn skip_advances_the_schedule_without_replaying_missed_intervals() {
    let fixture = Fixture::new();
    let scheduler = fixture.scheduler();
    scheduler
        .save_automation(None, automation(MissedRunPolicy::Skip, timestamp(1, 0)))
        .unwrap();

    scheduler.tick_at(timestamp(4, 0)).await.unwrap();

    assert!(fixture.submitter.submissions.lock().unwrap().is_empty());
    scheduler.tick_at(timestamp(4, 30)).await.unwrap();
    assert_eq!(fixture.submitter.submissions.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn run_once_creates_one_catch_up_run_after_restart() {
    let fixture = Fixture::new();
    fixture
        .scheduler()
        .save_automation(None, automation(MissedRunPolicy::RunOnce, timestamp(1, 0)))
        .unwrap();

    fixture.scheduler().tick_at(timestamp(4, 0)).await.unwrap();

    assert_eq!(fixture.submitter.submissions.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn committed_run_history_survives_restart() {
    let fixture = Fixture::new();
    let scheduler = fixture.scheduler();
    scheduler
        .save_automation(None, automation(MissedRunPolicy::Skip, timestamp(1, 0)))
        .unwrap();
    scheduler
        .run_now(None, "checks", timestamp(1, 5))
        .await
        .unwrap();
    drop(scheduler);

    let runs = fixture.scheduler().list_runs(None, Some("checks")).unwrap();

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].automation_id, "checks");
}

#[tokio::test]
async fn invalid_configuration_records_a_rejected_run_without_creating_a_task() {
    let fixture = Fixture::new();
    std::fs::write(fixture.config_root.join("automations.json"), b"{not-json").unwrap();
    fixture.store.register_automation_scope(None).unwrap();

    fixture.scheduler().tick_at(timestamp(1, 0)).await.unwrap();

    let runs = fixture.scheduler().list_runs(None, None).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, AutomationRunStatus::Rejected);
    assert!(runs[0].message.as_deref().unwrap().len() <= 256);
    assert!(fixture.submitter.submissions.lock().unwrap().is_empty());
    assert!(fixture.store.task_projections().unwrap().is_empty());
}

#[test]
fn interval_math_fixture_is_not_accidentally_equal() {
    assert_eq!(timestamp(1, 0) + Duration::minutes(30), timestamp(1, 30));
}

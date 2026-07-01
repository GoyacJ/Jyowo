#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

#[tokio::test]
async fn automation_store_missing_files_loads_empty_state() {
    let workspace = unique_workspace("automation-empty-state");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");

    let automations = list_automations_with_runtime_state(&state)
        .await
        .expect("missing automation file should load as empty");
    let runs = list_automation_runs_with_runtime_state(None, &state)
        .await
        .expect("missing automation run ledger should load as empty");

    assert!(automations.automations.is_empty());
    assert!(runs.runs.is_empty());
}

#[tokio::test]
async fn save_automation_writes_runtime_file_and_defaults_to_disabled() {
    let workspace = unique_workspace("automation-save-file");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    let automation = automation_spec("checks", false, MissedRunPolicy::Skip);

    let payload = save_automation_with_runtime_state(
        SaveAutomationRequest {
            automation: automation.clone(),
        },
        &state,
    )
    .await
    .expect("automation should save");

    assert_eq!(payload.automation.id, "checks");
    assert!(!payload.automation.enabled);
    let automations_path = state
        .workspace_root()
        .join(".jyowo")
        .join("runtime")
        .join("automations.json");
    let saved = std::fs::read_to_string(&automations_path).expect("automation file should exist");
    assert!(saved.contains("\"id\": \"checks\""));
    assert!(
        std::fs::read_dir(automations_path.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp")),
        "successful atomic write should not leave temp files"
    );
}

#[tokio::test]
async fn save_automation_rejects_secret_like_prompt() {
    let workspace = unique_workspace("automation-secret-prompt");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    let mut automation = automation_spec("checks", false, MissedRunPolicy::Skip);
    automation.prompt = "Use token=sk-test-secret-value".to_owned();

    let error = save_automation_with_runtime_state(SaveAutomationRequest { automation }, &state)
        .await
        .expect_err("secret-like automation prompts should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(list_automations_with_runtime_state(&state)
        .await
        .unwrap()
        .automations
        .is_empty());
}

#[tokio::test]
async fn save_automation_rejects_non_read_only_workspace_snapshot() {
    let workspace = unique_workspace("automation-rejects-write-scope");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    let mut automation = automation_spec("checks", false, MissedRunPolicy::Skip);
    automation.workspace_access = WorkspaceAccess::ReadWrite {
        allowed_writable_subpaths: Vec::new(),
    };

    let error = save_automation_with_runtime_state(SaveAutomationRequest { automation }, &state)
        .await
        .expect_err("automation MVP should reject writable workspace snapshots");

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn disabled_automation_is_not_run_by_due_scheduler() {
    let workspace = unique_workspace("automation-disabled-not-due");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    save_automation_with_runtime_state(
        SaveAutomationRequest {
            automation: automation_spec("checks", false, MissedRunPolicy::RunOnce),
        },
        &state,
    )
    .await
    .expect("automation should save");

    let records = run_due_automations_once_with_runtime_state(chrono::Utc::now(), &state)
        .await
        .expect("scheduler pass should succeed");

    assert!(records.is_empty());
    assert!(list_automation_runs_with_runtime_state(None, &state)
        .await
        .unwrap()
        .runs
        .is_empty());
}

#[tokio::test(start_paused = true)]
async fn automation_scheduler_task_runs_due_automations_in_process() {
    let workspace = unique_workspace("automation-scheduler-task");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    let now = chrono::Utc::now();
    save_automation_with_runtime_state(
        SaveAutomationRequest {
            automation: automation_spec_at(
                "checks",
                true,
                MissedRunPolicy::RunOnce,
                now - chrono::Duration::minutes(120),
            ),
        },
        &state,
    )
    .await
    .expect("automation should save");
    let runtime = Arc::new(tokio::sync::RwLock::new(state.clone()));

    let task = spawn_automation_scheduler(runtime);
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(60)).await;
    tokio::task::yield_now().await;
    task.abort();
    let runs = list_automation_runs_with_runtime_state(Some("checks".to_owned()), &state)
        .await
        .expect("scheduler should write ledger");

    assert_eq!(runs.runs.len(), 1);
    assert_eq!(runs.runs[0].status, AutomationRunStatus::Rejected);
}

#[tokio::test]
async fn run_automation_now_writes_rejected_ledger_without_runtime() {
    let workspace = unique_workspace("automation-run-now-ledger");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    save_automation_with_runtime_state(
        SaveAutomationRequest {
            automation: automation_spec("checks", true, MissedRunPolicy::Skip),
        },
        &state,
    )
    .await
    .expect("automation should save");

    let payload = run_automation_now_with_runtime_state("checks".to_owned(), &state)
        .await
        .expect("manual automation run should record a rejection");
    let runs = list_automation_runs_with_runtime_state(Some("checks".to_owned()), &state)
        .await
        .expect("automation ledger should load");

    assert_eq!(payload.record.status, AutomationRunStatus::Rejected);
    assert_eq!(runs.runs.len(), 1);
    assert_eq!(runs.runs[0].automation_id, "checks");
    assert_eq!(runs.runs[0].status, AutomationRunStatus::Rejected);
    let serialized = serde_json::to_string(&runs).unwrap();
    assert!(!serialized.contains("rawToolOutput"));
}

#[tokio::test]
async fn automation_missed_policy_skip_or_run_once_is_enforced() {
    let workspace = unique_workspace("automation-missed-policy");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    let now = chrono::Utc::now();
    save_automation_with_runtime_state(
        SaveAutomationRequest {
            automation: automation_spec_at(
                "skip-missed",
                true,
                MissedRunPolicy::Skip,
                now - chrono::Duration::minutes(120),
            ),
        },
        &state,
    )
    .await
    .expect("skip automation should save");
    save_automation_with_runtime_state(
        SaveAutomationRequest {
            automation: automation_spec_at(
                "run-once-missed",
                true,
                MissedRunPolicy::RunOnce,
                now - chrono::Duration::minutes(120),
            ),
        },
        &state,
    )
    .await
    .expect("run-once automation should save");

    let records = run_due_automations_once_with_runtime_state(now, &state)
        .await
        .expect("scheduler pass should handle missed policy");

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].automation_id, "run-once-missed");
    assert_eq!(records[0].status, AutomationRunStatus::Rejected);
}

#[tokio::test]
async fn automation_rejects_missing_permission_or_profile_snapshot() {
    let workspace = unique_workspace("automation-missing-snapshot");
    std::fs::create_dir_all(workspace.join(".jyowo").join("runtime"))
        .expect("runtime directory should exist");
    std::fs::write(
        workspace
            .join(".jyowo")
            .join("runtime")
            .join("automations.json"),
        r#"[{
          "id":"legacy",
          "enabled":true,
          "prompt":"Run checks",
          "schedule":{"intervalMinutes":30},
          "sandboxMode":"none",
          "workspaceScope":"current_workspace",
          "workspaceAccess":"read_only",
          "createdAt":"2026-06-30T01:00:00Z",
          "updatedAt":"2026-06-30T01:00:00Z"
        }]"#,
    )
    .expect("legacy automation file should write");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");

    let error = run_due_automations_once_with_runtime_state(chrono::Utc::now(), &state)
        .await
        .expect_err("missing snapshot should fail closed");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("automation"));
}

#[tokio::test]
async fn automation_delete_and_set_enabled_update_saved_state() {
    let workspace = unique_workspace("automation-delete-enable");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    save_automation_with_runtime_state(
        SaveAutomationRequest {
            automation: automation_spec("checks", false, MissedRunPolicy::Skip),
        },
        &state,
    )
    .await
    .expect("automation should save");

    let enabled = jyowo_desktop_shell::commands::set_automation_enabled_with_runtime_state(
        SetAutomationEnabledRequest {
            enabled: true,
            id: "checks".to_owned(),
        },
        &state,
    )
    .await
    .expect("automation should enable");
    assert!(enabled.automation.enabled);

    let deleted = delete_automation_with_runtime_state("checks".to_owned(), &state)
        .await
        .expect("automation should delete");
    assert_eq!(deleted.status, "deleted");
    assert!(list_automations_with_runtime_state(&state)
        .await
        .unwrap()
        .automations
        .is_empty());
}

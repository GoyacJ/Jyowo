use chrono::{TimeZone, Utc};
use harness_contracts::{
    AutomationRunRecord, AutomationRunStatus, AutomationSchedule, AutomationSpec,
    AutomationWorkspaceScope, MissedRunPolicy, PermissionMode, SandboxMode, ToolProfile,
    WorkspaceAccess,
};
use serde_json::json;

#[test]
fn automation_spec_serializes_stable_shape() {
    let spec = AutomationSpec {
        id: "automation-001".to_owned(),
        enabled: false,
        prompt: "Run checks".to_owned(),
        schedule: AutomationSchedule {
            interval_minutes: 60,
        },
        tool_profile: ToolProfile::Coding,
        permission_mode: PermissionMode::Default,
        sandbox_mode: SandboxMode::None,
        workspace_scope: AutomationWorkspaceScope::CurrentWorkspace,
        workspace_access: WorkspaceAccess::ReadOnly,
        missed_run_policy: MissedRunPolicy::RunOnce,
        created_at: Utc.with_ymd_and_hms(2026, 6, 30, 1, 0, 0).unwrap(),
        updated_at: Utc.with_ymd_and_hms(2026, 6, 30, 1, 0, 0).unwrap(),
    };

    assert_eq!(
        serde_json::to_value(&spec).unwrap(),
        json!({
            "id": "automation-001",
            "enabled": false,
            "prompt": "Run checks",
            "schedule": { "intervalMinutes": 60 },
            "toolProfile": "coding",
            "permissionMode": "default",
            "sandboxMode": "none",
            "workspaceScope": "current_workspace",
            "workspaceAccess": "read_only",
            "missedRunPolicy": "run_once",
            "createdAt": "2026-06-30T01:00:00Z",
            "updatedAt": "2026-06-30T01:00:00Z"
        })
    );
}

#[test]
fn automation_spec_defaults_to_disabled_and_skip_policy() {
    let spec: AutomationSpec = serde_json::from_value(json!({
        "id": "automation-001",
        "prompt": "Run checks",
        "schedule": { "intervalMinutes": 30 },
        "toolProfile": "full",
        "permissionMode": "default",
        "sandboxMode": "none",
        "workspaceScope": "current_workspace",
        "workspaceAccess": "read_only",
        "createdAt": "2026-06-30T01:00:00Z",
        "updatedAt": "2026-06-30T01:00:00Z"
    }))
    .unwrap();

    assert!(!spec.enabled);
    assert_eq!(spec.missed_run_policy, MissedRunPolicy::Skip);
}

#[test]
fn automation_rejects_unknown_missed_run_policy() {
    let error = serde_json::from_value::<AutomationSpec>(json!({
        "id": "automation-001",
        "prompt": "Run checks",
        "schedule": { "intervalMinutes": 30 },
        "toolProfile": "full",
        "permissionMode": "default",
        "sandboxMode": "none",
        "workspaceScope": "current_workspace",
        "workspaceAccess": "read_only",
        "missedRunPolicy": "catch_up_all",
        "createdAt": "2026-06-30T01:00:00Z",
        "updatedAt": "2026-06-30T01:00:00Z"
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn automation_run_record_serializes_without_tool_output() {
    let record = AutomationRunRecord {
        automation_id: "automation-001".to_owned(),
        completed_at: None,
        id: "automation-run-001".to_owned(),
        message: Some("Started".to_owned()),
        run_id: Some("01J00000000000000000000000".to_owned()),
        started_at: Utc.with_ymd_and_hms(2026, 6, 30, 1, 5, 0).unwrap(),
        status: AutomationRunStatus::Started,
    };

    let value = serde_json::to_value(&record).unwrap();

    assert_eq!(value["status"], "started");
    assert_eq!(value.get("rawToolOutput"), None);
}

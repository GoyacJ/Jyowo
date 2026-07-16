use chrono::{TimeZone, Utc};
use harness_contracts::{
    MissedRunPolicy, PermissionMode, ScheduledTaskRunRecord, ScheduledTaskRunStatus,
    ScheduledTaskSchedule, ScheduledTaskSpec,
};
use serde_json::json;

#[test]
fn scheduled_task_spec_serializes_stable_shape() {
    let spec = ScheduledTaskSpec {
        id: "scheduled_task-001".to_owned(),
        name: "Checks".to_owned(),
        enabled: false,
        prompt: "Run checks".to_owned(),
        schedule: ScheduledTaskSchedule {
            interval_minutes: 60,
        },
        workspace_root: Some("/tmp/project".to_owned()),
        permission_mode: PermissionMode::Default,
        missed_run_policy: MissedRunPolicy::RunOnce,
        created_at: Utc.with_ymd_and_hms(2026, 6, 30, 1, 0, 0).unwrap(),
        updated_at: Utc.with_ymd_and_hms(2026, 6, 30, 1, 0, 0).unwrap(),
    };

    assert_eq!(
        serde_json::to_value(&spec).unwrap(),
        json!({
            "id": "scheduled_task-001",
            "name": "Checks",
            "enabled": false,
            "prompt": "Run checks",
            "schedule": { "intervalMinutes": 60 },
            "workspaceRoot": "/tmp/project",
            "permissionMode": "default",
            "missedRunPolicy": "run_once",
            "createdAt": "2026-06-30T01:00:00Z",
            "updatedAt": "2026-06-30T01:00:00Z"
        })
    );
}

#[test]
fn scheduled_task_spec_defaults_to_disabled_and_skip_policy() {
    let spec: ScheduledTaskSpec = serde_json::from_value(json!({
        "id": "scheduled_task-001",
        "name": "Checks",
        "prompt": "Run checks",
        "schedule": { "intervalMinutes": 30 },
        "workspaceRoot": null,
        "permissionMode": "default",
        "createdAt": "2026-06-30T01:00:00Z",
        "updatedAt": "2026-06-30T01:00:00Z"
    }))
    .unwrap();

    assert!(!spec.enabled);
    assert_eq!(spec.missed_run_policy, MissedRunPolicy::Skip);
}

#[test]
fn scheduled_task_rejects_unknown_missed_run_policy() {
    let error = serde_json::from_value::<ScheduledTaskSpec>(json!({
        "id": "scheduled_task-001",
        "name": "Checks",
        "prompt": "Run checks",
        "schedule": { "intervalMinutes": 30 },
        "permissionMode": "default",
        "missedRunPolicy": "catch_up_all",
        "createdAt": "2026-06-30T01:00:00Z",
        "updatedAt": "2026-06-30T01:00:00Z"
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn scheduled_task_run_record_serializes_without_tool_output() {
    let record = ScheduledTaskRunRecord {
        scheduled_task_id: "scheduled_task-001".to_owned(),
        completed_at: None,
        id: "scheduled_task-run-001".to_owned(),
        message: Some("Started".to_owned()),
        task_id: Some("01J00000000000000000000000".to_owned()),
        started_at: Utc.with_ymd_and_hms(2026, 6, 30, 1, 5, 0).unwrap(),
        status: ScheduledTaskRunStatus::Started,
    };

    let value = serde_json::to_value(&record).unwrap();

    assert_eq!(value["status"], "started");
    assert_eq!(value.get("rawToolOutput"), None);
}

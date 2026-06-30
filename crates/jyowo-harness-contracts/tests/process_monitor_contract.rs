use harness_contracts::{
    ProcessReadRequest, ProcessReadResult, ProcessRuntimeStatus, ProcessStartRequest,
    ProcessStartResult, ProcessStopRequest, ProcessStopResult, SandboxExitStatus,
};

#[test]
fn process_requests_use_stable_snake_case_shape() {
    let start: ProcessStartRequest = serde_json::from_value(serde_json::json!({
        "command": "pnpm",
        "args": ["dev"],
        "cwd": "apps/desktop",
        "buffer_bytes": 4096
    }))
    .unwrap();
    assert_eq!(start.command, "pnpm");
    assert_eq!(start.args, ["dev"]);
    assert_eq!(start.cwd.as_deref(), Some("apps/desktop"));
    assert_eq!(start.buffer_bytes, Some(4096));

    let read: ProcessReadRequest =
        serde_json::from_value(serde_json::json!({ "process_id": "proc-1", "max_bytes": 1024 }))
            .unwrap();
    assert_eq!(read.process_id, "proc-1");
    assert_eq!(read.max_bytes, Some(1024));

    let stop: ProcessStopRequest =
        serde_json::from_value(serde_json::json!({ "process_id": "proc-1" })).unwrap();
    assert_eq!(stop.process_id, "proc-1");

    assert!(serde_json::from_value::<ProcessStartRequest>(
        serde_json::json!({ "command": "pnpm dev", "unknown": true })
    )
    .is_err());
}

#[test]
fn process_results_use_stable_status_and_exit_shape() {
    let start = ProcessStartResult {
        process_id: "proc-1".to_owned(),
        pid: Some(42),
        status: ProcessRuntimeStatus::Running,
        sandbox_events: Vec::new(),
    };
    assert_eq!(
        serde_json::to_value(start).unwrap(),
        serde_json::json!({ "process_id": "proc-1", "pid": 42, "status": "running" })
    );

    let read = ProcessReadResult {
        process_id: "proc-1".to_owned(),
        status: ProcessRuntimeStatus::Exited,
        stdout: "ok".to_owned(),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
        exit_status: Some(SandboxExitStatus::Code(0)),
    };
    assert_eq!(serde_json::to_value(read).unwrap()["status"], "exited");

    let stop = ProcessStopResult {
        process_id: "proc-1".to_owned(),
        status: ProcessRuntimeStatus::Stopped,
    };
    assert_eq!(
        serde_json::to_value(stop).unwrap(),
        serde_json::json!({ "process_id": "proc-1", "status": "stopped" })
    );
}

use jyowo_desktop_shell::commands::{get_app_info_payload, harness_healthcheck_payload};

#[test]
fn get_app_info_payload_returns_jyowo_identity() {
    let payload = get_app_info_payload();

    assert_eq!(payload.name, "Jyowo");
    assert_eq!(payload.shell, "tauri2-react");
    assert_eq!(payload.harness.sdk_crate, "jyowo_harness_sdk");
    assert_eq!(payload.harness.mode, "in-process");
}

#[test]
fn harness_healthcheck_payload_reports_available_sdk() {
    let payload = harness_healthcheck_payload();

    assert_eq!(payload.status, "available");
    assert_eq!(payload.sdk_crate, "jyowo_harness_sdk");
}

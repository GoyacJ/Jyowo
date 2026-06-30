use super::*;

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

#[test]
fn eval_lab_payloads_require_runtime_instead_of_static_support_cases() {
    let list_error = list_eval_cases_payload().unwrap_err();
    assert_eq!(list_error.code, "RUNTIME_UNAVAILABLE");

    let error = run_eval_case_payload(RunEvalCaseRequest {
        case_id: "regression-smoke".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");
}

#[test]
fn eval_lab_runtime_state_paths_require_eval_runtime() {
    let workspace = unique_workspace("eval-no-runtime");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("workspace state should initialize without a harness");

    let list_error = list_eval_cases_with_runtime_state(&state).unwrap_err();
    assert_eq!(list_error.code, "RUNTIME_UNAVAILABLE");

    let run_error = run_eval_case_with_runtime_state(
        RunEvalCaseRequest {
            case_id: "regression-smoke".to_owned(),
        },
        &state,
    )
    .unwrap_err();
    assert_eq!(run_error.code, "RUNTIME_UNAVAILABLE");
}

#[test]
fn run_eval_case_payload_requires_runtime_for_valid_case_ids_and_rejects_malformed_ids() {
    let unknown = run_eval_case_payload(RunEvalCaseRequest {
        case_id: "unknown-case".to_owned(),
    })
    .unwrap_err();
    assert_eq!(unknown.code, "RUNTIME_UNAVAILABLE");

    let malformed = run_eval_case_payload(RunEvalCaseRequest {
        case_id: "bad case".to_owned(),
    })
    .unwrap_err();
    assert_eq!(malformed.code, "INVALID_PAYLOAD");
}

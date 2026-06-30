use harness_contracts::{
    DiagnosticItem, DiagnosticLanguage, DiagnosticSeverity, DiagnosticsRequest, DiagnosticsResult,
    DiagnosticsRunnerKind,
};
use serde_json::json;

#[test]
fn diagnostics_request_uses_stable_runner_shape() {
    assert_eq!(
        serde_json::to_value(DiagnosticsRequest {
            runner: DiagnosticsRunnerKind::Rust,
        })
        .unwrap(),
        json!({ "runner": "rust" })
    );
}

#[test]
fn diagnostics_result_uses_workspace_relative_items() {
    let result = DiagnosticsResult {
        diagnostics: vec![DiagnosticItem {
            language: DiagnosticLanguage::TypeScript,
            severity: DiagnosticSeverity::Error,
            code: Some("TS2322".to_owned()),
            message: "Type 'string' is not assignable to type 'number'.".to_owned(),
            relative_path: "apps/desktop/src/App.tsx".to_owned(),
            line: Some(12),
            column: Some(8),
        }],
    };

    assert_eq!(
        serde_json::to_value(result).unwrap(),
        json!({
            "diagnostics": [{
                "language": "type_script",
                "severity": "error",
                "code": "TS2322",
                "message": "Type 'string' is not assignable to type 'number'.",
                "relative_path": "apps/desktop/src/App.tsx",
                "line": 12,
                "column": 8
            }]
        })
    );
}

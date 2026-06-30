#![cfg(feature = "builtin-toolset")]

use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use futures::{future::BoxFuture, StreamExt};
use harness_contracts::{
    CapabilityRegistry, Decision, DiagnosticLanguage, DiagnosticSeverity, DiagnosticsRawOutput,
    DiagnosticsRunRequest, DiagnosticsRunnerCap, DiagnosticsRunnerKind, Event, RedactRules,
    Redactor, TenantId, ToolCapability, ToolError, ToolResult, ToolUseHeartbeatEvent, ToolUseId,
};
use harness_permission::{PermissionBroker, PermissionContext, PermissionError, PermissionRequest};
use harness_tool::{
    builtin::{parse_cargo_diagnostics, parse_typescript_diagnostics, DiagnosticsTool},
    InterruptToken, Tool, ToolContext,
};
use serde_json::{json, Value};

#[tokio::test]
async fn diagnostics_tool_requires_runner_capability() {
    let tool = DiagnosticsTool::default();

    assert!(matches!(
        execute_error(
            &tool,
            json!({ "runner": "rust" }),
            tool_ctx(CapabilityRegistry::default()),
        )
        .await,
        ToolError::CapabilityMissing(ToolCapability::Custom(name)) if name == "diagnostics_runner"
    ));
}

#[tokio::test]
async fn diagnostics_tool_runs_runner_and_returns_redacted_workspace_relative_items() {
    let workspace = tempfile::tempdir().unwrap();
    let source = workspace.path().join("src/lib.rs");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, "").unwrap();
    let mut caps = CapabilityRegistry::default();
    caps.install::<dyn DiagnosticsRunnerCap>(
        ToolCapability::Custom("diagnostics_runner".to_owned()),
        Arc::new(FakeDiagnosticsRunner {
            output: DiagnosticsRawOutput {
                runner: DiagnosticsRunnerKind::Rust,
                stdout: format!(
                    r#"{{"reason":"compiler-message","message":{{"level":"error","code":{{"code":"E0308"}},"message":"token SECRET123","spans":[{{"file_name":"{}","line_start":4,"column_start":9,"is_primary":true}}]}}}}"#,
                    source.display()
                ),
                stderr: String::new(),
                sandbox_events: Vec::new(),
            },
        }),
    );

    let result = execute_final(
        &DiagnosticsTool::default(),
        json!({ "runner": "rust" }),
        tool_ctx_at(workspace.path(), caps, Arc::new(SecretRedactor)),
    )
    .await;

    let ToolResult::Structured(value) = result else {
        panic!("expected structured diagnostics result");
    };
    assert_eq!(value["diagnostics"][0]["language"], "rust");
    assert_eq!(value["diagnostics"][0]["severity"], "error");
    assert_eq!(value["diagnostics"][0]["code"], "E0308");
    assert_eq!(value["diagnostics"][0]["message"], "token [REDACTED]");
    assert_eq!(value["diagnostics"][0]["relative_path"], "src/lib.rs");
    assert_eq!(value["diagnostics"][0]["line"], 4);
    assert_eq!(value["diagnostics"][0]["column"], 9);
}

#[tokio::test]
async fn diagnostics_tool_streams_sandbox_events_before_final_result() {
    let workspace = tempfile::tempdir().unwrap();
    let mut caps = CapabilityRegistry::default();
    caps.install::<dyn DiagnosticsRunnerCap>(
        ToolCapability::Custom("diagnostics_runner".to_owned()),
        Arc::new(FakeDiagnosticsRunner {
            output: DiagnosticsRawOutput {
                runner: DiagnosticsRunnerKind::Rust,
                stdout: String::new(),
                stderr: String::new(),
                sandbox_events: vec![heartbeat_event()],
            },
        }),
    );

    let events = execute_events(
        &DiagnosticsTool::default(),
        json!({ "runner": "rust" }),
        tool_ctx_at(workspace.path(), caps, Arc::new(NoopRedactor)),
    )
    .await;

    assert!(matches!(
        events.first(),
        Some(harness_tool::ToolEvent::Journal(Event::ToolUseHeartbeat(_)))
    ));
    assert!(matches!(
        events.last(),
        Some(harness_tool::ToolEvent::Final(ToolResult::Structured(value)))
            if value == &json!({ "diagnostics": [] })
    ));
}

#[test]
fn cargo_diagnostics_parser_rejects_workspace_external_paths() {
    let workspace = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap().path().join("src/lib.rs");
    let output = format!(
        r#"{{"reason":"compiler-message","message":{{"level":"error","code":{{"code":"E0308"}},"message":"bad","spans":[{{"file_name":"{}","line_start":1,"column_start":2,"is_primary":true}}]}}}}"#,
        external.display()
    );

    assert!(parse_cargo_diagnostics(&output, workspace.path(), &NoopRedactor).is_empty());
}

#[test]
fn cargo_diagnostics_parser_redacts_private_paths_inside_messages() {
    let workspace = tempfile::tempdir().unwrap();
    let source = workspace.path().join("src/lib.rs");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, "").unwrap();
    let output = format!(
        r#"{{"reason":"compiler-message","message":{{"level":"error","code":{{"code":"E0308"}},"message":"failed reading /Users/goya/.ssh/config","spans":[{{"file_name":"{}","line_start":1,"column_start":2,"is_primary":true}}]}}}}"#,
        source.display()
    );

    let diagnostics = parse_cargo_diagnostics(&output, workspace.path(), &NoopRedactor);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "failed reading [REDACTED]");
    assert!(!diagnostics[0].message.contains("/Users/goya"));
}

#[test]
fn typescript_diagnostics_parser_extracts_structured_items() {
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("apps/desktop/src/App.tsx");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "").unwrap();
    let output = format!(
        "{}(12,8): error TS2322: Type 'string' is not assignable to type 'number'.",
        file.display()
    );

    let diagnostics = parse_typescript_diagnostics(&output, workspace.path(), &NoopRedactor);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].language, DiagnosticLanguage::TypeScript);
    assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Error);
    assert_eq!(diagnostics[0].code.as_deref(), Some("TS2322"));
    assert_eq!(
        diagnostics[0].message,
        "Type 'string' is not assignable to type 'number'."
    );
    assert_eq!(diagnostics[0].relative_path, "apps/desktop/src/App.tsx");
    assert_eq!(diagnostics[0].line, Some(12));
    assert_eq!(diagnostics[0].column, Some(8));
}

struct FakeDiagnosticsRunner {
    output: DiagnosticsRawOutput,
}

impl DiagnosticsRunnerCap for FakeDiagnosticsRunner {
    fn run_diagnostics(
        &self,
        _request: DiagnosticsRunRequest,
    ) -> BoxFuture<'_, Result<DiagnosticsRawOutput, ToolError>> {
        Box::pin(async move { Ok(self.output.clone()) })
    }
}

struct SecretRedactor;

impl Redactor for SecretRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.replace("SECRET123", "[REDACTED]")
    }
}

struct NoopRedactor;

impl Redactor for NoopRedactor {
    fn redact(&self, input: &str, _rules: &RedactRules) -> String {
        input.to_owned()
    }
}

async fn execute_final(tool: &dyn Tool, input: Value, ctx: ToolContext) -> ToolResult {
    let events = execute_events(tool, input, ctx).await;
    events
        .into_iter()
        .find_map(|event| match event {
            harness_tool::ToolEvent::Final(result) => Some(result),
            _ => None,
        })
        .expect("expected final result")
}

async fn execute_events(
    tool: &dyn Tool,
    input: Value,
    ctx: ToolContext,
) -> Vec<harness_tool::ToolEvent> {
    tool.validate(&input, &ctx).await.unwrap();
    let mut stream = tool.execute(input, ctx).await.unwrap();
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }
    events
}

async fn execute_error(tool: &dyn Tool, input: Value, ctx: ToolContext) -> ToolError {
    tool.validate(&input, &ctx).await.unwrap();
    match tool.execute(input, ctx).await {
        Ok(_) => panic!("expected tool error"),
        Err(error) => error,
    }
}

fn tool_ctx(cap_registry: CapabilityRegistry) -> ToolContext {
    tool_ctx_at(std::env::temp_dir(), cap_registry, Arc::new(NoopRedactor))
}

fn tool_ctx_at(
    workspace_root: impl AsRef<Path>,
    cap_registry: CapabilityRegistry,
    redactor: Arc<dyn Redactor>,
) -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: harness_contracts::SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: workspace_root.as_ref().to_path_buf(),
        sandbox: None,
        permission_broker: Arc::new(AllowBroker),
        cap_registry: Arc::new(cap_registry),
        redactor,
        interrupt: InterruptToken::default(),
        parent_run: None,
    }
}

#[derive(Debug)]
struct AllowBroker;

#[async_trait]
impl PermissionBroker for AllowBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::AllowOnce
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), PermissionError> {
        Ok(())
    }
}

fn heartbeat_event() -> Event {
    Event::ToolUseHeartbeat(ToolUseHeartbeatEvent {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        message: "sandbox started".to_owned(),
        fraction: None,
        silent_for_ms: 0,
        at: chrono::Utc::now(),
    })
}

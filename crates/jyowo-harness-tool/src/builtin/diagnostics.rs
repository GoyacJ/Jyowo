use std::path::{Component, Path};
use std::sync::OnceLock;

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    DecisionScope, DiagnosticItem, DiagnosticLanguage, DiagnosticSeverity, DiagnosticsRawOutput,
    DiagnosticsRequest, DiagnosticsResult, DiagnosticsRunRequest, DiagnosticsRunnerCap,
    DiagnosticsRunnerKind, PermissionSubject, RedactRules, Redactor, ToolActionPlan,
    ToolCapability, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup, ToolResult,
};
use harness_permission::PermissionCheck;
use regex::Regex;
use serde_json::{json, Value};

use crate::{AuthorizedToolInput, Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

const DIAGNOSTICS_RUNNER_CAPABILITY: &str = "diagnostics_runner";

#[derive(Clone)]
pub struct DiagnosticsTool {
    descriptor: ToolDescriptor,
}

impl Default for DiagnosticsTool {
    fn default() -> Self {
        Self {
            descriptor: super::descriptor(
                "Diagnostics",
                "Diagnostics",
                "Run read-only workspace diagnostics and return structured findings.",
                ToolGroup::Search,
                false,
                true,
                false,
                128_000,
                vec![diagnostics_runner_capability()],
                super::object_schema(
                    &["runner"],
                    json!({
                        "runner": {
                            "type": "string",
                            "enum": ["rust", "desktop_ts"]
                        }
                    }),
                ),
            ),
        }
    }
}

#[async_trait]
impl Tool for DiagnosticsTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        diagnostics_request(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        super::generic_action_plan(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                scope: DecisionScope::ToolName(self.descriptor.name.clone()),
            },
            ToolExecutionChannel::ProcessSandbox,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let request = diagnostics_request(authorized.raw_input()).map_err(validation_error)?;
        let runner = ctx.capability::<dyn DiagnosticsRunnerCap>(diagnostics_runner_capability())?;
        let output = runner
            .run_diagnostics(DiagnosticsRunRequest {
                tenant_id: ctx.tenant_id,
                session_id: ctx.session_id,
                run_id: ctx.run_id,
                workspace_root: ctx.workspace_root.clone(),
                runner: request.runner,
            })
            .await?;
        let diagnostics =
            parse_diagnostics_output(&output, &ctx.workspace_root, ctx.redactor.as_ref());
        let final_result = ToolEvent::Final(ToolResult::Structured(
            serde_json::to_value(DiagnosticsResult { diagnostics })
                .map_err(|error| ToolError::Message(error.to_string()))?,
        ));
        Ok(Box::pin(stream::iter(
            output
                .sandbox_events
                .into_iter()
                .map(ToolEvent::Journal)
                .chain(std::iter::once(final_result))
                .collect::<Vec<_>>(),
        )))
    }
}

#[must_use]
pub fn parse_cargo_diagnostics(
    output: &str,
    workspace_root: &Path,
    redactor: &dyn Redactor,
) -> Vec<DiagnosticItem> {
    output
        .lines()
        .filter_map(|line| parse_cargo_diagnostic_line(line, workspace_root, redactor))
        .collect()
}

#[must_use]
pub fn parse_typescript_diagnostics(
    output: &str,
    workspace_root: &Path,
    redactor: &dyn Redactor,
) -> Vec<DiagnosticItem> {
    let pattern = Regex::new(
        r"^(?P<path>.+)\((?P<line>\d+),(?P<column>\d+)\): (?P<severity>error|warning|info) (?P<code>TS\d+): (?P<message>.*)$",
    )
    .expect("typescript diagnostics regex should compile");
    output
        .lines()
        .filter_map(|line| {
            let captures = pattern.captures(line)?;
            let relative_path =
                workspace_relative_path(captures.name("path")?.as_str(), workspace_root)?;
            Some(DiagnosticItem {
                language: DiagnosticLanguage::TypeScript,
                severity: severity(captures.name("severity")?.as_str()),
                code: Some(captures.name("code")?.as_str().to_owned()),
                message: redact_message(captures.name("message")?.as_str(), redactor),
                relative_path,
                line: captures.name("line")?.as_str().parse().ok(),
                column: captures.name("column")?.as_str().parse().ok(),
            })
        })
        .collect()
}

fn parse_diagnostics_output(
    output: &DiagnosticsRawOutput,
    workspace_root: &Path,
    redactor: &dyn Redactor,
) -> Vec<DiagnosticItem> {
    let text = if output.stderr.trim().is_empty() {
        output.stdout.clone()
    } else if output.stdout.trim().is_empty() {
        output.stderr.clone()
    } else {
        format!("{}\n{}", output.stdout, output.stderr)
    };
    match output.runner {
        DiagnosticsRunnerKind::Rust => parse_cargo_diagnostics(&text, workspace_root, redactor),
        DiagnosticsRunnerKind::DesktopTs => {
            parse_typescript_diagnostics(&text, workspace_root, redactor)
        }
        _ => Vec::new(),
    }
}

fn parse_cargo_diagnostic_line(
    line: &str,
    workspace_root: &Path,
    redactor: &dyn Redactor,
) -> Option<DiagnosticItem> {
    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("reason").and_then(Value::as_str)? != "compiler-message" {
        return None;
    }
    let message = value.get("message")?;
    let span = message
        .get("spans")
        .and_then(Value::as_array)?
        .iter()
        .find(|span| {
            span.get("is_primary")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .or_else(|| message.get("spans").and_then(Value::as_array)?.first())?;
    let relative_path = workspace_relative_path(
        span.get("file_name").and_then(Value::as_str)?,
        workspace_root,
    )?;
    Some(DiagnosticItem {
        language: DiagnosticLanguage::Rust,
        severity: severity(
            message
                .get("level")
                .and_then(Value::as_str)
                .unwrap_or("info"),
        ),
        code: cargo_code(message.get("code")),
        message: redact_message(
            message.get("message").and_then(Value::as_str).unwrap_or(""),
            redactor,
        ),
        relative_path,
        line: span
            .get("line_start")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        column: span
            .get("column_start")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
    })
}

fn diagnostics_request(input: &Value) -> Result<DiagnosticsRequest, ValidationError> {
    serde_json::from_value(input.clone()).map_err(|error| ValidationError::from(error.to_string()))
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn diagnostics_runner_capability() -> ToolCapability {
    ToolCapability::Custom(DIAGNOSTICS_RUNNER_CAPABILITY.to_owned())
}

fn cargo_code(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(code) => Some(code.clone()),
        Value::Object(map) => map.get("code").and_then(Value::as_str).map(str::to_owned),
        _ => None,
    }
}

fn severity(value: &str) -> DiagnosticSeverity {
    match value {
        "error" => DiagnosticSeverity::Error,
        "warning" | "warn" => DiagnosticSeverity::Warning,
        _ => DiagnosticSeverity::Info,
    }
}

fn redact_message(message: &str, redactor: &dyn Redactor) -> String {
    redact_private_absolute_paths(&redactor.redact(message, &RedactRules::default()))
}

fn redact_private_absolute_paths(message: &str) -> String {
    static PRIVATE_PATH_PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PRIVATE_PATH_PATTERN.get_or_init(|| {
        Regex::new(
            r#"(?x)
            (?:
                /Users/[^\s'"`<>]+
              | /home/[^\s'"`<>]+
              | /private/var/[^\s'"`<>]+
              | [A-Za-z]:[\\/][^\s'"`<>]+
            )
            "#,
        )
        .expect("private path redaction regex should compile")
    });
    pattern.replace_all(message, "[REDACTED]").into_owned()
}

fn workspace_relative_path(path: &str, workspace_root: &Path) -> Option<String> {
    let path = Path::new(path);
    let relative = if path.is_absolute() {
        path.strip_prefix(workspace_root).ok()?
    } else {
        path
    };
    if relative.components().any(disallowed_relative_component) {
        return None;
    }
    Some(relative.to_string_lossy().replace('\\', "/"))
}

fn disallowed_relative_component(component: Component<'_>) -> bool {
    matches!(
        component,
        Component::Prefix(_) | Component::RootDir | Component::ParentDir
    )
}

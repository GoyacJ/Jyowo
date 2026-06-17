use std::{io, path::Path, process::Command};

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    DecisionScope, PermissionSubject, ToolDescriptor, ToolError, ToolGroup, ToolResult,
};
use harness_permission::PermissionCheck;
use regex::Regex;
use serde_json::{json, Value};

use crate::{Tool, ToolContext, ToolEvent, ToolStream, ValidationError};

#[derive(Clone)]
pub struct GrepTool {
    descriptor: ToolDescriptor,
}

impl Default for GrepTool {
    fn default() -> Self {
        Self {
            descriptor: super::descriptor(
                "Grep",
                "Grep",
                "Search files with ripgrep.",
                ToolGroup::Search,
                true,
                true,
                false,
                64_000,
                Vec::new(),
                super::object_schema(
                    &["path", "pattern"],
                    json!({
                        "path": { "type": "string" },
                        "pattern": { "type": "string" }
                    }),
                ),
            ),
        }
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        super::workspace_path::input_path(input)?;
        pattern(input)?;
        Ok(())
    }

    async fn check_permission(&self, input: &Value, ctx: &ToolContext) -> PermissionCheck {
        if let Ok(path) = super::workspace_path::scope_path(input, ctx) {
            if let Some(check) = super::workspace_path::dangerous_path_permission(
                input,
                ctx,
                PermissionSubject::ToolInvocation {
                    tool: self.descriptor.name.clone(),
                    input: input.clone(),
                },
                DecisionScope::PathPrefix(path),
            ) {
                return check;
            }
        }
        let path = match super::workspace_path::resolve_existing(input, ctx) {
            Ok(path) => path,
            Err(error) => {
                return PermissionCheck::Denied {
                    reason: error.to_string(),
                };
            }
        };
        PermissionCheck::AskUser {
            subject: PermissionSubject::ToolInvocation {
                tool: self.descriptor.name.clone(),
                input: input.clone(),
            },
            scope: DecisionScope::PathPrefix(path),
        }
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolStream, ToolError> {
        let root = super::workspace_path::resolve_existing(&input, &ctx)?;
        let pattern = pattern(&input).map_err(validation_error)?;
        let mut matches = match run_ripgrep(&root, pattern, &ctx) {
            Ok(matches) => matches,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                internal_grep(&root, pattern, &ctx)?
            }
            Err(error) => return Err(ToolError::Message(error.to_string())),
        };
        matches.sort_by(|left, right| {
            left["path"]
                .as_str()
                .unwrap_or_default()
                .cmp(right["path"].as_str().unwrap_or_default())
                .then_with(|| {
                    left["line"]
                        .as_u64()
                        .unwrap_or_default()
                        .cmp(&right["line"].as_u64().unwrap_or_default())
                })
        });

        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(Value::Array(matches)),
        )])))
    }
}

fn run_ripgrep(root: &Path, pattern: &str, ctx: &ToolContext) -> Result<Vec<Value>, io::Error> {
    let output = Command::new("rg")
        .arg("--line-number")
        .arg("--with-filename")
        .arg("--color")
        .arg("never")
        .arg("--no-heading")
        .arg("--no-follow")
        .arg("--")
        .arg(pattern)
        .arg(root)
        .output()?;

    if !output.status.success() && output.status.code() != Some(1) {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }

    let stdout =
        String::from_utf8(output.stdout).map_err(|error| io::Error::other(error.to_string()))?;
    let mut matches = Vec::new();
    for value in stdout.lines().filter_map(parse_rg_line) {
        let Some(path) = value["path"].as_str() else {
            continue;
        };
        super::workspace_path::ensure_inside_workspace(Path::new(path), ctx)
            .map_err(|error| io::Error::other(error.to_string()))?;
        matches.push(value);
    }
    Ok(matches)
}

fn internal_grep(root: &Path, pattern: &str, ctx: &ToolContext) -> Result<Vec<Value>, ToolError> {
    let regex = Regex::new(pattern).map_err(|error| ToolError::Message(error.to_string()))?;
    let mut matches = Vec::new();
    collect_internal_matches(root, &regex, ctx, &mut matches)?;
    Ok(matches)
}

fn collect_internal_matches(
    path: &Path,
    regex: &Regex,
    ctx: &ToolContext,
    matches: &mut Vec<Value>,
) -> Result<(), ToolError> {
    super::workspace_path::ensure_inside_workspace(path, ctx)?;
    let meta = path
        .metadata()
        .map_err(|error| ToolError::Message(error.to_string()))?;
    if meta.is_dir() {
        for entry in
            std::fs::read_dir(path).map_err(|error| ToolError::Message(error.to_string()))?
        {
            let entry = entry.map_err(|error| ToolError::Message(error.to_string()))?;
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            collect_internal_matches(&entry.path(), regex, ctx, matches)?;
        }
        return Ok(());
    }

    if !meta.is_file() {
        return Ok(());
    }

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::InvalidData => return Ok(()),
        Err(error) => return Err(ToolError::Message(error.to_string())),
    };
    for (index, line) in content.lines().enumerate() {
        if regex.is_match(line) {
            matches.push(json!({
                "path": path.to_string_lossy(),
                "line": index + 1,
                "text": line
            }));
        }
    }
    Ok(())
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn parse_rg_line(line: &str) -> Option<Value> {
    let mut parts = line.splitn(3, ':');
    let path = parts.next()?;
    let line_number = parts.next()?.parse::<u64>().ok()?;
    let text = parts.next()?.to_owned();
    Some(json!({
        "path": path,
        "line": line_number,
        "text": text
    }))
}

fn pattern(input: &Value) -> Result<&str, ValidationError> {
    input
        .get("pattern")
        .and_then(Value::as_str)
        .ok_or_else(|| ValidationError::from("pattern is required"))
}

#[cfg(test)]
mod tests {
    use harness_contracts::{
        AgentId, CapabilityRegistry, CorrelationId, RunId, SessionId, TenantId, ToolUseId,
    };
    use harness_permission::PermissionBroker;
    use tempfile::tempdir;

    use super::*;
    use crate::{InterruptToken, ToolContext};

    #[cfg(unix)]
    #[test]
    fn internal_grep_rejects_symlink_escape() {
        let root = tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let outside = root.path().join("outside.txt");
        std::fs::write(&outside, "needle\n").unwrap();
        std::os::unix::fs::symlink(&outside, workspace.join("link.txt")).unwrap();
        let ctx = tool_ctx_at(&workspace);

        let error = internal_grep(&workspace, "needle", &ctx).unwrap_err();

        assert!(matches!(error, ToolError::PermissionDenied(_)));
    }

    fn tool_ctx_at(workspace_root: &Path) -> ToolContext {
        ToolContext {
            tool_use_id: ToolUseId::new(),
            run_id: RunId::new(),
            session_id: SessionId::new(),
            tenant_id: TenantId::SINGLE,
            correlation_id: CorrelationId::new(),
            agent_id: AgentId::from_u128(1),
            subagent_depth: 0,
            workspace_root: workspace_root.to_path_buf(),
            sandbox: None,
            permission_broker: std::sync::Arc::new(NoopBroker),
            cap_registry: std::sync::Arc::new(CapabilityRegistry::default()),
            interrupt: InterruptToken::default(),
            parent_run: None,
        }
    }

    #[derive(Debug)]
    struct NoopBroker;

    #[async_trait::async_trait]
    impl PermissionBroker for NoopBroker {
        async fn decide(
            &self,
            _request: harness_permission::PermissionRequest,
            _ctx: harness_permission::PermissionContext,
        ) -> harness_contracts::Decision {
            harness_contracts::Decision::AllowOnce
        }

        async fn persist(
            &self,
            _decision: harness_permission::PersistedDecision,
        ) -> Result<(), harness_contracts::PermissionError> {
            Ok(())
        }
    }
}

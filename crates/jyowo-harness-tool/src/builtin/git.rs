use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    ActionResource, DecisionScope, ExecFingerprint, NetworkAccess, PermissionSubject,
    ToolActionPlan, ToolDescriptor, ToolDescriptorMetadata, ToolError, ToolExecutionChannel,
    ToolGroup, ToolIntegrationSource, ToolResult, ToolRiskLevel, WorkspaceAccess,
};
use harness_permission::PermissionCheck;
use harness_sandbox::ExecSpec;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::{
    action_plan_from_permission_check, AuthorizedToolInput, Tool, ToolContext, ToolEvent,
    ToolStream, ValidationError,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum GitOperation {
    Status,
    Diff,
    Show,
    Log,
    Stage,
    Commit,
    Branch,
    Pull,
    Push,
}

#[derive(Clone)]
struct GitTool {
    descriptor: ToolDescriptor,
    operation: GitOperation,
}

macro_rules! git_tool {
    ($name:ident, $op:ident, $tool_name:literal, $display:literal, $description:literal, $read_only:expr, $destructive:expr, $schema:expr) => {
        #[derive(Clone)]
        pub struct $name {
            inner: GitTool,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    inner: GitTool::new(
                        GitOperation::$op,
                        $tool_name,
                        $display,
                        $description,
                        $read_only,
                        $destructive,
                        $schema,
                    ),
                }
            }
        }

        #[async_trait]
        impl Tool for $name {
            fn descriptor(&self) -> &ToolDescriptor {
                self.inner.descriptor()
            }

            async fn validate(
                &self,
                input: &Value,
                ctx: &ToolContext,
            ) -> Result<(), ValidationError> {
                self.inner.validate(input, ctx).await
            }

            async fn plan(
                &self,
                input: &Value,
                ctx: &ToolContext,
            ) -> Result<ToolActionPlan, ToolError> {
                self.inner.plan(input, ctx).await
            }

            async fn execute_authorized(
                &self,
                authorized: AuthorizedToolInput,
                ctx: ToolContext,
            ) -> Result<ToolStream, ToolError> {
                self.inner.execute_authorized(authorized, ctx).await
            }
        }
    };
}

git_tool!(
    GitStatusTool,
    Status,
    "GitStatus",
    "Git Status",
    "Show repository status using a fixed git status command.",
    true,
    false,
    json!({ "type": "object", "properties": { "cwd": { "type": "string" } } })
);
git_tool!(
    GitDiffTool,
    Diff,
    "GitDiff",
    "Git Diff",
    "Show repository diff using fixed git diff commands.",
    true,
    false,
    json!({
        "type": "object",
        "properties": {
            "cwd": { "type": "string" },
            "staged": { "type": "boolean" },
            "paths": { "type": "array", "items": { "type": "string" } }
        }
    })
);
git_tool!(
    GitShowTool,
    Show,
    "GitShow",
    "Git Show",
    "Show a git revision using a fixed git show command.",
    true,
    false,
    json!({ "type": "object", "required": ["rev"], "properties": { "cwd": { "type": "string" }, "rev": { "type": "string" } } })
);
git_tool!(
    GitLogTool,
    Log,
    "GitLog",
    "Git Log",
    "Show recent git history using a fixed git log command.",
    true,
    false,
    json!({ "type": "object", "properties": { "cwd": { "type": "string" }, "limit": { "type": "integer", "minimum": 1, "maximum": 100 } } })
);
git_tool!(
    GitStageTool,
    Stage,
    "GitStage",
    "Git Stage",
    "Stage files using a fixed git add command.",
    false,
    false,
    json!({ "type": "object", "required": ["paths"], "properties": { "cwd": { "type": "string" }, "paths": { "type": "array", "items": { "type": "string" }, "minItems": 1 } } })
);
git_tool!(
    GitCommitTool,
    Commit,
    "GitCommit",
    "Git Commit",
    "Create a commit using a fixed git commit command.",
    false,
    false,
    json!({ "type": "object", "required": ["message"], "properties": { "cwd": { "type": "string" }, "message": { "type": "string" } } })
);
git_tool!(
    GitBranchTool,
    Branch,
    "GitBranch",
    "Git Branch",
    "List or create branches using fixed git branch commands.",
    false,
    false,
    json!({ "type": "object", "properties": { "cwd": { "type": "string" }, "name": { "type": "string" }, "create": { "type": "boolean" } } })
);
git_tool!(
    GitPullTool,
    Pull,
    "GitPull",
    "Git Pull",
    "Pull from a remote using a fixed git pull command.",
    false,
    false,
    json!({ "type": "object", "properties": { "cwd": { "type": "string" }, "remote": { "type": "string" }, "branch": { "type": "string" } } })
);
git_tool!(
    GitPushTool,
    Push,
    "GitPush",
    "Git Push",
    "Push to a remote using a fixed git push command.",
    false,
    false,
    json!({ "type": "object", "properties": { "cwd": { "type": "string" }, "remote": { "type": "string" }, "branch": { "type": "string" } } })
);

impl GitTool {
    fn new(
        operation: GitOperation,
        name: &str,
        display_name: &str,
        description: &str,
        is_read_only: bool,
        is_destructive: bool,
        mut input_schema: Value,
    ) -> Self {
        input_schema["additionalProperties"] = Value::Bool(false);
        let mut descriptor = super::with_output_schema(
            super::descriptor(
                name,
                display_name,
                description,
                ToolGroup::Git,
                false,
                is_read_only,
                is_destructive,
                256_000,
                Vec::new(),
                input_schema,
            ),
            json!({
                "type": "object",
                "required": ["status", "success", "stdout", "stderr"],
                "properties": {
                    "status": { "type": ["integer", "null"] },
                    "success": { "type": "boolean" },
                    "stdout": { "type": "string" },
                    "stderr": { "type": "string" }
                },
                "additionalProperties": false
            }),
        );
        descriptor.metadata = ToolDescriptorMetadata {
            aliases: vec![operation.alias().to_owned()],
            families: vec!["git".to_owned()],
            platforms: vec!["codex".to_owned(), "claude_code".to_owned()],
            examples: vec![operation.example().to_owned()],
            risk_level: if is_read_only {
                ToolRiskLevel::Low
            } else {
                ToolRiskLevel::Medium
            },
            effects: vec![operation.effect().to_owned()],
            modalities: vec!["text".to_owned()],
            integration_source: ToolIntegrationSource::Builtin,
        };
        Self {
            descriptor,
            operation,
        }
    }

    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        self.operation.argv(input).map_err(ValidationError::from)?;
        optional_string(input, "cwd").map_err(ValidationError::from)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let argv = self.operation.argv(input).map_err(validation_error)?;
        let cwd_path = git_cwd(input, ctx)?;
        let cwd_for_plan = Some(cwd_path.clone());
        let command = "git".to_owned();
        let fingerprint = command_fingerprint(&command, &argv, Some(&cwd_path));
        let check = if self.descriptor.properties.is_read_only {
            PermissionCheck::Allowed
        } else {
            PermissionCheck::AskUser {
                subject: PermissionSubject::CommandExec {
                    command: command.clone(),
                    argv: argv.clone(),
                    cwd: cwd_for_plan.clone(),
                    fingerprint: Some(fingerprint.clone()),
                },
                scope: DecisionScope::ExactCommand {
                    command: format!("git {}", argv.join(" ")),
                    cwd: cwd_for_plan.clone(),
                },
            }
        };
        action_plan_from_permission_check(
            &self.descriptor,
            input,
            ctx,
            check,
            vec![ActionResource::Command {
                command,
                argv,
                cwd: cwd_for_plan,
                fingerprint,
            }],
            if self.descriptor.properties.is_read_only {
                WorkspaceAccess::ReadOnly
            } else {
                WorkspaceAccess::ReadWrite {
                    allowed_writable_subpaths: Vec::new(),
                }
            },
            NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let (argv, cwd_path) = authorized_git_command(&authorized, &ctx, &self.descriptor)?;
        let mut command = Command::new("git");
        command.args(&argv).current_dir(cwd_path);
        let output = command
            .output()
            .await
            .map_err(|error| ToolError::Message(error.to_string()))?;
        let result = json!({
            "status": output.status.code(),
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
        });
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(result),
        )])))
    }
}

fn authorized_git_command(
    authorized: &AuthorizedToolInput,
    ctx: &ToolContext,
    descriptor: &ToolDescriptor,
) -> Result<(Vec<String>, std::path::PathBuf), ToolError> {
    let plan = authorized.action_plan();
    if plan.tool_name != descriptor.name {
        return Err(ToolError::PermissionDenied(
            "authorized plan tool mismatch".to_owned(),
        ));
    }
    let Some(ActionResource::Command {
        command,
        argv,
        cwd,
        fingerprint,
    }) = plan
        .resources
        .iter()
        .find(|resource| matches!(resource, ActionResource::Command { .. }))
    else {
        return Err(ToolError::PermissionDenied(
            "authorized git command resource missing".to_owned(),
        ));
    };
    if command != "git" {
        return Err(ToolError::PermissionDenied(
            "authorized command is not git".to_owned(),
        ));
    }
    let expected = command_fingerprint(command, argv, cwd.as_ref());
    if expected != *fingerprint {
        return Err(ToolError::PermissionDenied(
            "authorized git command fingerprint mismatch".to_owned(),
        ));
    }
    Ok((
        argv.clone(),
        cwd.clone().unwrap_or_else(|| ctx.workspace_root.clone()),
    ))
}

impl GitOperation {
    fn argv(self, input: &Value) -> Result<Vec<String>, String> {
        if !input.is_object() {
            return Err("git tool input must be an object".to_owned());
        }
        match self {
            Self::Status => Ok(vec![
                "status".to_owned(),
                "--short".to_owned(),
                "--branch".to_owned(),
            ]),
            Self::Diff => {
                let mut argv = vec!["diff".to_owned()];
                if bool_field(input, "staged") {
                    argv.push("--staged".to_owned());
                }
                append_paths(&mut argv, input)?;
                Ok(argv)
            }
            Self::Show => Ok(vec!["show".to_owned(), required_string(input, "rev")?]),
            Self::Log => Ok(vec![
                "log".to_owned(),
                "--oneline".to_owned(),
                format!(
                    "-{}",
                    integer_field(input, "limit").unwrap_or(20).clamp(1, 100)
                ),
            ]),
            Self::Stage => {
                let mut argv = vec!["add".to_owned(), "--".to_owned()];
                let paths = string_array(input, "paths")?;
                if paths.is_empty() {
                    return Err("paths must not be empty".to_owned());
                }
                argv.extend(paths);
                Ok(argv)
            }
            Self::Commit => Ok(vec![
                "commit".to_owned(),
                "-m".to_owned(),
                required_string(input, "message")?,
            ]),
            Self::Branch => {
                let name = optional_string(input, "name")?;
                if bool_field(input, "create") {
                    let name =
                        name.ok_or_else(|| "name is required when create is true".to_owned())?;
                    Ok(vec!["branch".to_owned(), name])
                } else {
                    Ok(vec!["branch".to_owned(), "--list".to_owned()])
                }
            }
            Self::Pull => remote_branch_args("pull", input),
            Self::Push => remote_branch_args("push", input),
        }
    }

    fn alias(self) -> &'static str {
        match self {
            Self::Status => "git status",
            Self::Diff => "git diff",
            Self::Show => "git show",
            Self::Log => "git log",
            Self::Stage => "git add",
            Self::Commit => "git commit",
            Self::Branch => "git branch",
            Self::Pull => "git pull",
            Self::Push => "git push",
        }
    }

    fn example(self) -> &'static str {
        match self {
            Self::Status => "Check repository status",
            Self::Diff => "Inspect unstaged changes",
            Self::Show => "Show a specific commit",
            Self::Log => "List recent commits",
            Self::Stage => "Stage selected files",
            Self::Commit => "Commit staged changes",
            Self::Branch => "List or create branches",
            Self::Pull => "Pull a remote branch",
            Self::Push => "Push the current branch",
        }
    }

    fn effect(self) -> &'static str {
        match self {
            Self::Status | Self::Diff | Self::Show | Self::Log => "reads_git",
            Self::Stage | Self::Commit | Self::Branch => "mutates_git",
            Self::Pull | Self::Push => "mutates_git_remote",
        }
    }
}

fn git_cwd(input: &Value, ctx: &ToolContext) -> Result<std::path::PathBuf, ToolError> {
    let path = optional_string(input, "cwd")
        .map_err(validation_error)?
        .map(std::path::PathBuf::from)
        .map_or_else(
            || ctx.workspace_root.clone(),
            |path| {
                if path.is_absolute() {
                    path
                } else {
                    ctx.workspace_root.join(path)
                }
            },
        );
    super::workspace_path::ensure_inside_workspace(&path, ctx)?;
    Ok(path)
}

fn command_fingerprint(
    command: &str,
    argv: &[String],
    cwd: Option<&std::path::PathBuf>,
) -> ExecFingerprint {
    ExecSpec {
        command: command.to_owned(),
        args: argv.to_vec(),
        cwd: cwd.cloned(),
        ..ExecSpec::default()
    }
    .canonical_fingerprint(&Default::default())
}

fn remote_branch_args(command: &str, input: &Value) -> Result<Vec<String>, String> {
    let mut argv = vec![command.to_owned()];
    if let Some(remote) = optional_string(input, "remote")? {
        argv.push(remote);
        if let Some(branch) = optional_string(input, "branch")? {
            argv.push(branch);
        }
    }
    Ok(argv)
}

fn append_paths(argv: &mut Vec<String>, input: &Value) -> Result<(), String> {
    let paths = string_array(input, "paths")?;
    if !paths.is_empty() {
        argv.push("--".to_owned());
        argv.extend(paths);
    }
    Ok(())
}

fn string_array(input: &Value, field: &str) -> Result<Vec<String>, String> {
    let Some(value) = input.get(field) else {
        return Ok(Vec::new());
    };
    let Some(values) = value.as_array() else {
        return Err(format!("{field} must be an array"));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{field} entries must be strings"))
        })
        .collect()
}

fn required_string(input: &Value, field: &str) -> Result<String, String> {
    optional_string(input, field)?.ok_or_else(|| format!("{field} is required"))
}

fn optional_string(input: &Value, field: &str) -> Result<Option<String>, String> {
    let Some(value) = input.get(field) else {
        return Ok(None);
    };
    value
        .as_str()
        .map(|value| Some(value.to_owned()))
        .ok_or_else(|| format!("{field} must be a string"))
}

fn bool_field(input: &Value, field: &str) -> bool {
    input.get(field).and_then(Value::as_bool).unwrap_or(false)
}

fn integer_field(input: &Value, field: &str) -> Option<i64> {
    input.get(field).and_then(Value::as_i64)
}

fn validation_error(message: String) -> ToolError {
    ToolError::Validation(message)
}

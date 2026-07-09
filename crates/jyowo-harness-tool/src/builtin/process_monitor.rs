use std::path::{Component, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    ActionResource, BudgetMetric, DecisionScope, Event, NetworkAccess, OverflowAction,
    PermissionSubject, ProcessReadInvocation, ProcessReadRequest, ProcessReadResult,
    ProcessStartInvocation, ProcessStartRequest, ProcessStartResult, ProcessStopInvocation,
    ProcessStopRequest, ProcessStopResult, RunScopedProcessRegistryCap, ToolActionPlan,
    ToolCapability, ToolDescriptor, ToolError, ToolExecutionChannel, ToolGroup, ToolResult,
    WorkspaceAccess, RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY,
};
use harness_permission::{DangerousPatternLibrary, PermissionCheck};
use harness_sandbox::{ExecSpec, StdioSpec};
use serde_json::{json, Value};

use crate::{
    action_plan_from_permission_check, AuthorizedToolInput, Tool, ToolContext, ToolEvent,
    ToolStream, ValidationError,
};

#[derive(Clone)]
pub struct ProcessStartTool {
    descriptor: ToolDescriptor,
}

impl Default for ProcessStartTool {
    fn default() -> Self {
        Self {
            descriptor: super::with_long_running(
                super::with_output_schema(
                    super::descriptor(
                        "ProcessStart",
                        "Process Start",
                        "Start a run-scoped process through the configured sandbox.",
                        ToolGroup::Shell,
                        false,
                        false,
                        true,
                        16_000,
                        vec![process_registry_capability()],
                        super::object_schema(
                            &["command"],
                            json!({
                                "command": { "type": "string" },
                                "args": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                },
                                "cwd": { "type": "string" },
                                "buffer_bytes": { "type": "integer", "minimum": 1 }
                            }),
                        ),
                    ),
                    serde_json::to_value(schemars::schema_for!(ProcessStartResult))
                        .unwrap_or_else(|_| json!({"type": "object"})),
                ),
                super::long_running_policy(Duration::from_secs(5), Duration::from_secs(120)),
            ),
        }
    }
}

#[async_trait]
impl Tool for ProcessStartTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        process_start_request(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let request = process_start_request(input).unwrap_or(ProcessStartRequest {
            command: String::new(),
            args: Vec::new(),
            cwd: None,
            buffer_bytes: None,
        });
        let command_display = display_command(&request);
        if let Some(rule) = DangerousPatternLibrary::default_unix().detect_command(&command_display)
        {
            return action_plan_from_permission_check(
                &self.descriptor,
                input,
                ctx,
                PermissionCheck::DangerousCommand {
                    command: command_display,
                    pattern: rule.id.clone(),
                    severity: rule.severity,
                },
                vec![command_resource(&request, ctx)],
                WorkspaceAccess::ReadWrite {
                    allowed_writable_subpaths: Vec::new(),
                },
                NetworkAccess::None,
                ToolExecutionChannel::ProcessSandbox,
            );
        }
        let spec = permission_exec_spec(&request);
        let base = ctx
            .sandbox
            .as_ref()
            .map(|sandbox| sandbox.base_config())
            .unwrap_or_default();
        let fingerprint = spec.canonical_fingerprint(&base);
        action_plan_from_permission_check(
            &self.descriptor,
            input,
            ctx,
            PermissionCheck::AskUser {
                subject: PermissionSubject::CommandExec {
                    command: request.command.clone(),
                    argv: request.args.clone(),
                    cwd: request.cwd.as_ref().map(PathBuf::from),
                    fingerprint: Some(fingerprint),
                },
                scope: DecisionScope::ExactCommand {
                    command: command_display,
                    cwd: spec.cwd,
                },
            },
            vec![ActionResource::Command {
                command: request.command,
                argv: request.args,
                cwd: request.cwd.map(PathBuf::from),
                fingerprint,
            }],
            WorkspaceAccess::ReadWrite {
                allowed_writable_subpaths: Vec::new(),
            },
            NetworkAccess::None,
            ToolExecutionChannel::ProcessSandbox,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let request = process_start_request_from_plan(&authorized, &ctx)?;
        let registry =
            ctx.capability::<dyn RunScopedProcessRegistryCap>(process_registry_capability())?;
        let result = registry
            .start_process(
                ProcessStartInvocation {
                    tenant_id: ctx.tenant_id,
                    session_id: ctx.session_id,
                    run_id: ctx.run_id,
                    tool_use_id: ctx.tool_use_id,
                    workspace_root: ctx.workspace_root,
                    request,
                    sandbox_policy: authorized.action_plan().sandbox_policy.clone(),
                    workspace_access: authorized.action_plan().workspace_access.clone(),
                },
                Arc::clone(&ctx.redactor),
            )
            .await?;
        let sandbox_events = result.sandbox_events.clone();
        structured_result_with_events(sandbox_events, result)
    }
}

#[derive(Clone)]
pub struct ProcessReadTool {
    descriptor: ToolDescriptor,
}

impl Default for ProcessReadTool {
    fn default() -> Self {
        Self {
            descriptor: super::with_long_running(
                super::with_result_budget(
                    super::with_output_schema(
                        super::descriptor(
                            "ProcessRead",
                            "Process Read",
                            "Read redacted output from a run-scoped process.",
                            ToolGroup::Shell,
                            true,
                            true,
                            false,
                            256_000,
                            vec![process_registry_capability()],
                            super::object_schema(
                                &["process_id"],
                                json!({
                                    "process_id": { "type": "string" },
                                    "max_bytes": { "type": "integer", "minimum": 1 }
                                }),
                            ),
                        ),
                        serde_json::to_value(schemars::schema_for!(ProcessReadResult))
                            .unwrap_or_else(|_| json!({"type": "object"})),
                    ),
                    super::result_budget(
                        BudgetMetric::Bytes,
                        256_000,
                        OverflowAction::Offload,
                        4_000,
                        4_000,
                    ),
                ),
                super::long_running_policy(Duration::from_secs(5), Duration::from_secs(30)),
            ),
        }
    }
}

#[async_trait]
impl Tool for ProcessReadTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        process_read_request(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let request = process_read_request(input).map_err(validation_error)?;
        action_plan_from_permission_check(
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
            vec![ActionResource::Process {
                process_id: request.process_id,
                operation: "read".to_owned(),
            }],
            WorkspaceAccess::None,
            NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let request = process_read_request(authorized.raw_input()).map_err(validation_error)?;
        let registry =
            ctx.capability::<dyn RunScopedProcessRegistryCap>(process_registry_capability())?;
        let result = registry
            .read_process(
                ProcessReadInvocation {
                    tenant_id: ctx.tenant_id,
                    session_id: ctx.session_id,
                    run_id: ctx.run_id,
                    request,
                },
                Arc::clone(&ctx.redactor),
            )
            .await?;
        structured_result(result)
    }
}

#[derive(Clone)]
pub struct ProcessStopTool {
    descriptor: ToolDescriptor,
}

impl Default for ProcessStopTool {
    fn default() -> Self {
        Self {
            descriptor: super::with_long_running(
                super::with_output_schema(
                    super::descriptor(
                        "ProcessStop",
                        "Process Stop",
                        "Stop a run-scoped process.",
                        ToolGroup::Shell,
                        false,
                        false,
                        true,
                        16_000,
                        vec![process_registry_capability()],
                        super::object_schema(
                            &["process_id"],
                            json!({
                                "process_id": { "type": "string" }
                            }),
                        ),
                    ),
                    serde_json::to_value(schemars::schema_for!(ProcessStopResult))
                        .unwrap_or_else(|_| json!({"type": "object"})),
                ),
                super::long_running_policy(Duration::from_secs(5), Duration::from_secs(30)),
            ),
        }
    }
}

#[async_trait]
impl Tool for ProcessStopTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        process_stop_request(input)?;
        Ok(())
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        let request = process_stop_request(input).map_err(validation_error)?;
        action_plan_from_permission_check(
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
            vec![ActionResource::Process {
                process_id: request.process_id,
                operation: "stop".to_owned(),
            }],
            WorkspaceAccess::None,
            NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let request = process_stop_request(authorized.raw_input()).map_err(validation_error)?;
        let registry =
            ctx.capability::<dyn RunScopedProcessRegistryCap>(process_registry_capability())?;
        let result = registry
            .stop_process(ProcessStopInvocation {
                tenant_id: ctx.tenant_id,
                session_id: ctx.session_id,
                run_id: ctx.run_id,
                request,
            })
            .await?;
        structured_result(result)
    }
}

fn process_start_request(input: &Value) -> Result<ProcessStartRequest, ValidationError> {
    let request: ProcessStartRequest =
        serde_json::from_value(input.clone()).map_err(|error| error.to_string())?;
    if request.command.trim().is_empty() {
        return Err(ValidationError::from("command must not be empty"));
    }
    if request.command.chars().any(char::is_whitespace) {
        return Err(ValidationError::from(
            "command must be an executable name; pass arguments with args",
        ));
    }
    if request.args.len() > 128 {
        return Err(ValidationError::from(
            "args must contain at most 128 values",
        ));
    }
    if request
        .args
        .iter()
        .any(|arg| arg.is_empty() || arg.len() > 4096)
    {
        return Err(ValidationError::from(
            "args must not contain empty or oversized values",
        ));
    }
    if let Some(cwd) = request.cwd.as_deref() {
        validate_relative_cwd(cwd)?;
    }
    Ok(request)
}

fn process_read_request(input: &Value) -> Result<ProcessReadRequest, ValidationError> {
    let request: ProcessReadRequest =
        serde_json::from_value(input.clone()).map_err(|error| error.to_string())?;
    if request.process_id.trim().is_empty() {
        return Err(ValidationError::from("process_id must not be empty"));
    }
    Ok(request)
}

fn process_stop_request(input: &Value) -> Result<ProcessStopRequest, ValidationError> {
    let request: ProcessStopRequest =
        serde_json::from_value(input.clone()).map_err(|error| error.to_string())?;
    if request.process_id.trim().is_empty() {
        return Err(ValidationError::from("process_id must not be empty"));
    }
    Ok(request)
}

fn validate_relative_cwd(value: &str) -> Result<(), ValidationError> {
    let path = PathBuf::from(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(ValidationError::from(
            "cwd must be a workspace-relative path",
        ));
    }
    Ok(())
}

fn permission_exec_spec(request: &ProcessStartRequest) -> ExecSpec {
    ExecSpec {
        command: request.command.clone(),
        args: request.args.clone(),
        cwd: request.cwd.as_ref().map(PathBuf::from),
        stdin: StdioSpec::Null,
        stdout: StdioSpec::Piped,
        stderr: StdioSpec::Piped,
        workspace_access: WorkspaceAccess::ReadWrite {
            allowed_writable_subpaths: Vec::new(),
        },
        ..ExecSpec::default()
    }
}

fn process_start_request_from_plan(
    authorized: &AuthorizedToolInput,
    ctx: &ToolContext,
) -> Result<ProcessStartRequest, ToolError> {
    let Some(ActionResource::Command {
        command,
        argv,
        cwd,
        fingerprint,
    }) = authorized
        .action_plan()
        .resources
        .iter()
        .find_map(|resource| {
            matches!(resource, ActionResource::Command { .. }).then_some(resource)
        })
    else {
        return Err(ToolError::PermissionDenied(
            "authorized command resource missing".to_owned(),
        ));
    };

    let request = ProcessStartRequest {
        command: command.clone(),
        args: argv.clone(),
        cwd: cwd.as_ref().map(|path| path.to_string_lossy().into_owned()),
        buffer_bytes: authorized
            .raw_input()
            .get("buffer_bytes")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
    };
    let spec = permission_exec_spec(&request);
    let base = ctx
        .sandbox
        .as_ref()
        .map(|sandbox| sandbox.base_config())
        .unwrap_or_default();
    if spec.canonical_fingerprint(&base) != *fingerprint {
        return Err(ToolError::PermissionDenied(
            "authorized command fingerprint mismatch".to_owned(),
        ));
    }
    Ok(request)
}

fn command_resource(request: &ProcessStartRequest, ctx: &ToolContext) -> ActionResource {
    let spec = permission_exec_spec(request);
    let base = ctx
        .sandbox
        .as_ref()
        .map(|sandbox| sandbox.base_config())
        .unwrap_or_default();
    ActionResource::Command {
        command: request.command.clone(),
        argv: request.args.clone(),
        cwd: request.cwd.as_ref().map(PathBuf::from),
        fingerprint: spec.canonical_fingerprint(&base),
    }
}

fn display_command(request: &ProcessStartRequest) -> String {
    std::iter::once(request.command.as_str())
        .chain(request.args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

fn structured_result(result: impl serde::Serialize) -> Result<ToolStream, ToolError> {
    Ok(Box::pin(stream::iter([ToolEvent::Final(
        ToolResult::Structured(
            serde_json::to_value(result).map_err(|error| ToolError::Message(error.to_string()))?,
        ),
    )])))
}

fn structured_result_with_events(
    events: Vec<Event>,
    result: impl serde::Serialize,
) -> Result<ToolStream, ToolError> {
    let final_result = ToolEvent::Final(ToolResult::Structured(
        serde_json::to_value(result).map_err(|error| ToolError::Message(error.to_string()))?,
    ));
    Ok(Box::pin(stream::iter(
        events
            .into_iter()
            .map(ToolEvent::Journal)
            .chain(std::iter::once(final_result))
            .collect::<Vec<_>>(),
    )))
}

fn validation_error(error: ValidationError) -> ToolError {
    ToolError::Validation(error.to_string())
}

fn process_registry_capability() -> ToolCapability {
    ToolCapability::Custom(RUN_SCOPED_PROCESS_REGISTRY_CAPABILITY.to_owned())
}

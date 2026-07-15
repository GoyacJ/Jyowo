use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures::{stream, StreamExt};
use harness_contracts::{
    CorrelationId, Decision, DecisionScope, DeferPolicy, Event, HookError, HookEventKind,
    HookFailureMode, ManifestValidationFailure as EventManifestValidationFailure, McpServerId,
    McpServerSource, NetworkAccess, NoopRedactor, PermissionSubject, PluginRuntimeRpcError,
    PluginRuntimeRpcRequest, PluginRuntimeRpcResponse, ProviderRestriction, RedactRules,
    ResourceLimits, RunId, SandboxError, SandboxExitStatus, SandboxMode, SandboxPolicy,
    SandboxScope, SessionId, TenantId, ToolActionPlan, ToolCapability, ToolDescriptor,
    ToolDescriptorMetadata, ToolError, ToolExecutionChannel, ToolGroup, ToolIntegrationSource,
    ToolOrigin, ToolProperties, ToolResult, WorkspaceAccess,
};
use harness_hook::{
    ContextPatch, ContextPatchRole, HookContext, HookEvent, HookHandler, HookOutcome,
    HookRegistrationKind, PreToolUseOutcome,
};
use harness_mcp::{
    McpConnection, McpError, McpServerSpec, McpToolDescriptor, McpToolResult, TransportChoice,
};
use harness_sandbox::{
    execute_with_lifecycle, EventSink as SandboxEventSink, ExecContext, ExecSpec, SandboxBackend,
    StdioSpec,
};
use harness_skill::{parse_skill_markdown, SkillPlatform, SkillSource};
use harness_tool::{
    action_plan_from_permission_check, default_result_budget, AuthorizedToolInput, PermissionCheck,
    SchemaResolverContext, Tool, ToolContext, ToolEvent, ToolStream, ValidationError,
};
use ring::digest;
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt as _;

use crate::sources::validate_manifest_schema;
use crate::{
    DiscoverySource, ManifestLoadReport, ManifestLoaderError, ManifestOrigin, ManifestRecord,
    ManifestSigner, ManifestValidationFailure, McpManifestEntry, Plugin, PluginActivationContext,
    PluginActivationResult, PluginError, PluginManifest, PluginManifestLoader, PluginRuntimeLoader,
    RuntimeLoaderError, SkillManifestEntry, ToolManifestEntry,
};

const DEFAULT_METADATA_TIMEOUT: Duration = Duration::from_secs(1);
const DEFAULT_RUNTIME_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct CargoExtensionManifestLoader {
    sandbox: Option<Arc<dyn SandboxBackend>>,
    sandbox_mode: Option<SandboxMode>,
    search_paths: Option<Vec<PathBuf>>,
    timeout: Duration,
    workspace_root: Option<PathBuf>,
}

impl std::fmt::Debug for CargoExtensionManifestLoader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CargoExtensionManifestLoader")
            .field("sandbox", &self.sandbox.is_some())
            .field("sandbox_mode", &self.sandbox_mode)
            .field("search_paths", &self.search_paths)
            .field("timeout", &self.timeout)
            .field("workspace_root", &self.workspace_root)
            .finish()
    }
}

impl Default for CargoExtensionManifestLoader {
    fn default() -> Self {
        Self {
            sandbox: None,
            sandbox_mode: None,
            search_paths: None,
            timeout: DEFAULT_METADATA_TIMEOUT,
            workspace_root: None,
        }
    }
}

impl CargoExtensionManifestLoader {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_search_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.search_paths = Some(paths);
        self
    }

    #[must_use]
    pub fn with_sandbox(
        mut self,
        sandbox: Arc<dyn SandboxBackend>,
        mode: SandboxMode,
        workspace_root: PathBuf,
    ) -> Self {
        self.sandbox = Some(sandbox);
        self.sandbox_mode = Some(mode);
        self.workspace_root = Some(workspace_root);
        self
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn paths(&self) -> Vec<PathBuf> {
        self.search_paths.clone().unwrap_or_default()
    }
}

#[async_trait]
impl PluginManifestLoader for CargoExtensionManifestLoader {
    async fn enumerate(
        &self,
        source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        self.load_report(source).await.map(|report| report.records)
    }

    async fn load_report(
        &self,
        source: &DiscoverySource,
    ) -> Result<ManifestLoadReport, ManifestLoaderError> {
        if !matches!(source, DiscoverySource::CargoExtension) {
            return Ok(ManifestLoadReport::default());
        }

        let mut report = ManifestLoadReport::default();
        for binary in discover_cargo_extension_binaries(&self.paths())? {
            let output = run_extension_command(
                &binary,
                &["--harness-manifest"],
                None,
                self.timeout,
                self.sandbox.clone(),
                self.sandbox_mode.clone(),
                self.workspace_root.clone(),
            )
            .await;
            match output {
                Ok(output) if output.status_success => {
                    match decode_manifest_metadata(&binary, &output.stdout) {
                        Ok(record) => report.records.push(record),
                        Err(failure) => report.failures.push(failure),
                    }
                }
                Ok(output) => {
                    report.failures.push(cargo_extension_failure(
                        binary,
                        output.stdout,
                        format!("metadata command exited with status {}", output.status_code),
                        None,
                        None,
                    ));
                }
                Err(details) => {
                    report.failures.push(cargo_extension_failure(
                        binary,
                        Vec::new(),
                        format!("metadata command failed: {details}"),
                        None,
                        None,
                    ));
                }
            }
        }

        Ok(report)
    }
}

#[derive(Clone)]
pub struct CargoExtensionRuntimeLoader {
    sandbox: Option<Arc<dyn SandboxBackend>>,
    sandbox_mode: Option<SandboxMode>,
    timeout: Duration,
    workspace_root: Option<PathBuf>,
}

impl std::fmt::Debug for CargoExtensionRuntimeLoader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CargoExtensionRuntimeLoader")
            .field("sandbox", &self.sandbox.is_some())
            .field("sandbox_mode", &self.sandbox_mode)
            .field("timeout", &self.timeout)
            .field("workspace_root", &self.workspace_root)
            .finish()
    }
}

impl Default for CargoExtensionRuntimeLoader {
    fn default() -> Self {
        Self {
            sandbox: None,
            sandbox_mode: None,
            timeout: DEFAULT_RUNTIME_TIMEOUT,
            workspace_root: None,
        }
    }
}

impl CargoExtensionRuntimeLoader {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_sandbox(
        mut self,
        sandbox: Arc<dyn SandboxBackend>,
        mode: SandboxMode,
        workspace_root: PathBuf,
    ) -> Self {
        self.sandbox = Some(sandbox);
        self.sandbox_mode = Some(mode);
        self.workspace_root = Some(workspace_root);
        self
    }
}

#[async_trait]
impl PluginRuntimeLoader for CargoExtensionRuntimeLoader {
    fn can_load(&self, _manifest: &PluginManifest, origin: &ManifestOrigin) -> bool {
        matches!(origin, ManifestOrigin::CargoExtension { .. })
    }

    async fn load(
        &self,
        manifest: &PluginManifest,
        origin: &ManifestOrigin,
    ) -> Result<Arc<dyn Plugin>, RuntimeLoaderError> {
        let ManifestOrigin::CargoExtension { binary, .. } = origin else {
            return Err(RuntimeLoaderError::UnsupportedOrigin(origin.to_string()));
        };

        Ok(Arc::new(CargoExtensionPlugin {
            manifest: manifest.clone(),
            binary: binary.clone(),
            sandbox: self.sandbox.clone(),
            sandbox_mode: self.sandbox_mode.clone(),
            timeout: self.timeout,
            workspace_root: self.workspace_root.clone(),
        }))
    }
}

struct CargoExtensionPlugin {
    manifest: PluginManifest,
    binary: PathBuf,
    sandbox: Option<Arc<dyn SandboxBackend>>,
    sandbox_mode: Option<SandboxMode>,
    timeout: Duration,
    workspace_root: Option<PathBuf>,
}

#[async_trait]
impl Plugin for CargoExtensionPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        let result = self
            .client()
            .call(
                "activate",
                json!({
                    "trust_level": ctx.trust_level,
                    "plugin_id": ctx.plugin_id,
                    "config": ctx.config,
                    "workspace_root": ctx.workspace_root,
                }),
            )
            .await
            .map_err(PluginError::ActivateFailed)?;
        self.register_proxy_capabilities(&ctx).await?;
        if result.is_null() {
            return Ok(PluginActivationResult::default());
        }
        serde_json::from_value(result)
            .map_err(|error| PluginError::ActivateFailed(error.to_string()))
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        self.client()
            .call("deactivate", Value::Null)
            .await
            .map(|_| ())
            .map_err(PluginError::DeactivateFailed)
    }
}

impl CargoExtensionPlugin {
    fn client(&self) -> CargoExtensionRuntimeClient {
        CargoExtensionRuntimeClient {
            binary: self.binary.clone(),
            sandbox: self.sandbox.clone(),
            sandbox_mode: self.sandbox_mode.clone(),
            timeout: self.timeout,
            workspace_root: self.workspace_root.clone(),
        }
    }

    async fn register_proxy_capabilities(
        &self,
        ctx: &PluginActivationContext,
    ) -> Result<(), PluginError> {
        if let Some(registration) = &ctx.tools {
            for entry in &self.manifest.capabilities.tools {
                registration
                    .register(Box::new(CargoExtensionToolProxy {
                        descriptor: tool_descriptor_for(&self.manifest, entry),
                        client: self.client(),
                    }))
                    .await?;
            }
        }
        if let Some(registration) = &ctx.hooks {
            for entry in &self.manifest.capabilities.hooks {
                registration
                    .register(Box::new(CargoExtensionHookProxy {
                        name: entry.name.clone(),
                        events: entry.events.clone(),
                        trust_level: self.manifest.trust_level,
                        client: self.client(),
                    }))
                    .await?;
            }
        }
        if let Some(registration) = &ctx.skills {
            for entry in &self.manifest.capabilities.skills {
                let markdown = self.read_skill(entry).await?;
                let skill = parse_skill_markdown(
                    &markdown,
                    SkillSource::Plugin {
                        plugin_id: ctx.plugin_id.clone(),
                        trust: self.manifest.trust_level,
                    },
                    None,
                    current_skill_platform(),
                )
                .map_err(|error| PluginError::ActivateFailed(error.to_string()))?;
                registration.register(skill).await?;
            }
        }
        if let Some(registration) = &ctx.mcp {
            for entry in &self.manifest.capabilities.mcp_servers {
                let server = mcp_server_spec_for(&self.manifest, entry);
                registration
                    .register_ready(
                        server,
                        Arc::new(CargoExtensionMcpConnection {
                            server_name: entry.name.clone(),
                            client: self.client(),
                        }),
                    )
                    .await?;
            }
        }
        Ok(())
    }

    async fn read_skill(&self, entry: &SkillManifestEntry) -> Result<String, PluginError> {
        let result = self
            .client()
            .call("skill.read", json!({ "skill_name": entry.name }))
            .await
            .map_err(PluginError::ActivateFailed)?;
        match result {
            Value::String(markdown) => Ok(markdown),
            Value::Object(object) => object
                .get("markdown")
                .or_else(|| object.get("body"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    PluginError::ActivateFailed(
                        "skill.read result must be a string or object with markdown".to_owned(),
                    )
                }),
            _ => Err(PluginError::ActivateFailed(
                "skill.read result must be a string or object with markdown".to_owned(),
            )),
        }
    }
}

#[derive(Clone)]
struct CargoExtensionRuntimeClient {
    binary: PathBuf,
    sandbox: Option<Arc<dyn SandboxBackend>>,
    sandbox_mode: Option<SandboxMode>,
    timeout: Duration,
    workspace_root: Option<PathBuf>,
}

impl CargoExtensionRuntimeClient {
    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let request = PluginRuntimeRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: 1,
            method: method.to_owned(),
            params,
        };
        let input = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        let output = run_extension_command(
            &self.binary,
            &["--harness-runtime"],
            Some(input),
            self.timeout,
            self.sandbox.clone(),
            self.sandbox_mode.clone(),
            self.workspace_root.clone(),
        )
        .await?;
        if !output.status_success {
            return Err(format!("runtime exited with status {}", output.status_code));
        }
        let response: PluginRuntimeRpcResponse =
            serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
        if response.jsonrpc != "2.0" {
            return Err("runtime response jsonrpc must be 2.0".to_owned());
        }
        if response.id != request.id {
            return Err(format!(
                "runtime response id mismatch: expected {}, got {}",
                request.id, response.id
            ));
        }
        if let Some(error) = response.error {
            return Err(runtime_error_message(error));
        }
        Ok(response.result.unwrap_or(Value::Null))
    }
}

struct CargoExtensionToolProxy {
    descriptor: ToolDescriptor,
    client: CargoExtensionRuntimeClient,
}

#[async_trait]
impl Tool for CargoExtensionToolProxy {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn resolve_schema(&self, _ctx: &SchemaResolverContext) -> Result<Value, ToolError> {
        Ok(self.descriptor.input_schema.clone())
    }

    async fn validate(&self, input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        let validator = jsonschema::validator_for(&self.descriptor.input_schema)
            .map_err(|error| ValidationError::Message(error.to_string()))?;
        if validator.is_valid(input) {
            return Ok(());
        }
        let message = validator.iter_errors(input).next().map_or_else(
            || "tool input does not match schema".to_owned(),
            |error| error.to_string(),
        );
        Err(ValidationError::Message(message))
    }

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
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
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
            ToolExecutionChannel::ExternalCapability {
                capability: ToolCapability::Custom("plugin_sidecar".to_owned()),
            },
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        let input = authorized.raw_input().clone();
        let result = self
            .client
            .call(
                "tool.execute",
                json!({
                    "tool_name": self.descriptor.name,
                    "input": input,
                    "context": {
                        "tool_use_id": ctx.tool_use_id,
                        "run_id": ctx.run_id,
                        "session_id": ctx.session_id,
                        "tenant_id": ctx.tenant_id,
                        "correlation_id": ctx.correlation_id,
                        "agent_id": ctx.agent_id,
                        "subagent_depth": ctx.subagent_depth,
                        "workspace_root": ctx.workspace_root,
                    }
                }),
            )
            .await
            .map_err(|error| {
                let redacted = ctx.redactor.redact(&error, &RedactRules::default());
                ToolError::Message(redacted)
            })?;
        let result = serde_json::from_value::<ToolResult>(result)
            .map_err(|error| ToolError::Message(error.to_string()))?;
        Ok(Box::pin(stream::once(
            async move { ToolEvent::Final(result) },
        )))
    }
}

struct CargoExtensionHookProxy {
    name: String,
    events: Vec<HookEventKind>,
    trust_level: harness_contracts::TrustLevel,
    client: CargoExtensionRuntimeClient,
}

#[async_trait]
impl HookHandler for CargoExtensionHookProxy {
    fn handler_id(&self) -> &str {
        &self.name
    }

    fn interested_events(&self) -> &[HookEventKind] {
        &self.events
    }

    fn failure_mode(&self) -> HookFailureMode {
        HookFailureMode::FailOpen
    }

    fn registration_kind(&self) -> HookRegistrationKind {
        HookRegistrationKind::InProcess
    }

    fn declared_trust(&self) -> Option<harness_contracts::TrustLevel> {
        Some(self.trust_level)
    }

    async fn handle(&self, event: HookEvent, ctx: HookContext) -> Result<HookOutcome, HookError> {
        let result = self
            .client
            .call(
                "hook.handle",
                json!({
                    "hook_name": self.name,
                    "event_kind": event.kind(),
                    "event": hook_event_payload(&event),
                    "context": hook_context_payload(&ctx),
                }),
            )
            .await
            .map_err(|error| HookError::Transport {
                kind: harness_contracts::TransportFailureKind::NetworkError,
                detail: ctx.view.redacted().redact(&error, &RedactRules::default()),
            })?;
        hook_outcome_from_value(result)
    }
}

struct CargoExtensionMcpConnection {
    server_name: String,
    client: CargoExtensionRuntimeClient,
}

#[async_trait]
impl McpConnection for CargoExtensionMcpConnection {
    fn connection_id(&self) -> &str {
        "cargo-extension-mcp"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        let result = self
            .client
            .call("mcp.list_tools", json!({ "server_name": self.server_name }))
            .await
            .map_err(McpError::Transport)?;
        serde_json::from_value(result).map_err(|error| McpError::InvalidResponse(error.to_string()))
    }

    async fn call_tool(&self, name: &str, args: Value) -> Result<McpToolResult, McpError> {
        let result = self
            .client
            .call(
                "mcp.tool.call",
                json!({
                    "server_name": self.server_name,
                    "tool_name": name,
                    "arguments": args,
                }),
            )
            .await
            .map_err(McpError::Transport)?;
        serde_json::from_value(result).map_err(|error| McpError::InvalidResponse(error.to_string()))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        Ok(())
    }
}

fn runtime_error_message(error: PluginRuntimeRpcError) -> String {
    format!("runtime error {}: {}", error.code, error.message)
}

struct CommandOutput {
    stdout: Vec<u8>,
    status_success: bool,
    status_code: String,
}

async fn run_extension_command(
    binary: &Path,
    args: &[&str],
    input: Option<Vec<u8>>,
    timeout: Duration,
    sandbox: Option<Arc<dyn SandboxBackend>>,
    sandbox_mode: Option<SandboxMode>,
    workspace_root: Option<PathBuf>,
) -> Result<CommandOutput, String> {
    let binary = binary.to_path_buf();
    let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    if let Some(sandbox) = sandbox {
        return run_extension_command_sandboxed(
            sandbox,
            sandbox_mode,
            workspace_root,
            binary,
            args,
            input,
            timeout,
        )
        .await;
    }
    tokio::task::spawn_blocking(move || {
        run_extension_command_blocking(&binary, &args, input, timeout)
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn run_extension_command_sandboxed(
    sandbox: Arc<dyn SandboxBackend>,
    sandbox_mode: Option<SandboxMode>,
    workspace_root: Option<PathBuf>,
    binary: PathBuf,
    args: Vec<String>,
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<CommandOutput, String> {
    if !binary.is_absolute() {
        return Err("sidecar binary path must be absolute".to_owned());
    }
    let Some(mode) = sandbox_mode else {
        return Err("sidecar sandbox mode missing".to_owned());
    };
    let Some(workspace_root) = workspace_root else {
        return Err("sidecar sandbox workspace root missing".to_owned());
    };
    let cwd = binary
        .parent()
        .ok_or_else(|| "sidecar binary parent directory unavailable".to_owned())?
        .to_path_buf();
    let scope = if workspace_root == cwd {
        SandboxScope::WorkspaceOnly
    } else {
        SandboxScope::WorkspacePlus(vec![workspace_root])
    };
    let max_wall_clock_ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);
    let spec = ExecSpec {
        command: binary.display().to_string(),
        args,
        env: BTreeMap::new(),
        authorized_env_keys: Default::default(),
        secret_env_keys: Default::default(),
        cwd: Some(cwd.clone()),
        stdin: if input.is_some() {
            StdioSpec::Piped
        } else {
            StdioSpec::Null
        },
        stdout: StdioSpec::Piped,
        stderr: StdioSpec::Null,
        timeout: Some(timeout),
        activity_timeout: Some(timeout),
        policy: SandboxPolicy {
            mode,
            scope,
            network: NetworkAccess::None,
            resource_limits: ResourceLimits {
                max_memory_bytes: None,
                max_cpu_cores: None,
                max_pids: None,
                max_wall_clock_ms: Some(max_wall_clock_ms),
                max_open_files: None,
            },
            denied_host_paths: Vec::new(),
        },
        workspace_access: WorkspaceAccess::ReadOnly,
        output_policy: Default::default(),
        required_kill_scope: None,
        required_synchronous_kill_scope: None,
    };
    let ctx = ExecContext {
        session_id: SessionId::new(),
        run_id: RunId::new(),
        tool_use_id: None,
        tenant_id: TenantId::SINGLE,
        workspace_root: cwd,
        correlation_id: CorrelationId::new(),
        event_sink: Arc::new(NoopSandboxEventSink),
        redactor: Arc::new(NoopRedactor),
        blob_store: None,
        execution_id: 0,
    };
    let mut handle = execute_with_lifecycle(sandbox, spec, ctx)
        .await
        .map_err(sidecar_sandbox_error_summary)?;

    let stdout = handle.stdout.take();
    let stdout_task = tokio::spawn(async move {
        let mut collected = Vec::new();
        if let Some(mut stdout) = stdout {
            while let Some(chunk) = stdout.next().await {
                collected.extend_from_slice(&chunk);
            }
        }
        collected
    });

    if let Some(input) = input {
        let mut stdin = handle
            .stdin
            .take()
            .ok_or_else(|| "sidecar sandbox stdin unavailable".to_owned())?;
        stdin
            .write_all(&input)
            .await
            .map_err(|_| "sidecar sandbox stdin write failed".to_owned())?;
        stdin
            .shutdown()
            .await
            .map_err(|_| "sidecar sandbox stdin shutdown failed".to_owned())?;
    }

    let outcome = handle
        .activity
        .wait()
        .await
        .map_err(sidecar_sandbox_error_summary)?;
    let stdout = stdout_task
        .await
        .map_err(|_| "sidecar sandbox stdout task failed".to_owned())?;
    Ok(CommandOutput {
        stdout,
        status_success: sandbox_status_success(&outcome.exit_status),
        status_code: sandbox_status_code(&outcome.exit_status),
    })
}

fn sandbox_status_success(status: &SandboxExitStatus) -> bool {
    matches!(status, SandboxExitStatus::Code(0))
}

fn sandbox_status_code(status: &SandboxExitStatus) -> String {
    match status {
        SandboxExitStatus::Code(code) => code.to_string(),
        SandboxExitStatus::Signal(signal) => format!("signal {signal}"),
        SandboxExitStatus::Timeout => "timeout".to_owned(),
        SandboxExitStatus::InactivityTimeout => "inactivity_timeout".to_owned(),
        SandboxExitStatus::OutputBudgetExceeded => "output_budget_exceeded".to_owned(),
        SandboxExitStatus::Cancelled => "cancelled".to_owned(),
        SandboxExitStatus::BackendError => "backend_error".to_owned(),
        _ => "unknown".to_owned(),
    }
}

fn sidecar_sandbox_error_summary(error: SandboxError) -> String {
    match error {
        SandboxError::Unavailable { detail, .. } => {
            format!("sidecar sandbox unavailable: {detail}")
        }
        SandboxError::CapabilityMismatch { capability, .. } => {
            format!("sidecar sandbox capability mismatch: {capability}")
        }
        SandboxError::Timeout { .. } => "sidecar sandbox timed out".to_owned(),
        SandboxError::InactivityTimeout { .. } => "sidecar sandbox inactivity timeout".to_owned(),
        SandboxError::OutputBudgetExceeded { limit } => {
            format!("sidecar sandbox output budget exceeded: {limit}")
        }
        SandboxError::HostPathDenied { .. } => "sidecar sandbox denied host path".to_owned(),
        SandboxError::ResourceLimitExceeded { limit, .. } => {
            format!("sidecar sandbox resource limit exceeded: {limit}")
        }
        SandboxError::SnapshotUnsupported { .. } => {
            "sidecar sandbox snapshot unsupported".to_owned()
        }
        SandboxError::ContainerLifecycleError { .. } => {
            "sidecar sandbox container lifecycle failed".to_owned()
        }
        SandboxError::WorkspaceSyncFailed { .. } => {
            "sidecar sandbox workspace sync failed".to_owned()
        }
        SandboxError::CodeRuntime { .. } => "sidecar sandbox code runtime failed".to_owned(),
        SandboxError::Message(_) => "sidecar sandbox execution failed".to_owned(),
        _ => "sidecar sandbox execution failed".to_owned(),
    }
}

struct NoopSandboxEventSink;

impl SandboxEventSink for NoopSandboxEventSink {
    fn emit(&self, _event: Event) -> Result<(), SandboxError> {
        Ok(())
    }
}

fn run_extension_command_blocking(
    binary: &Path,
    args: &[String],
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<CommandOutput, String> {
    let mut child = Command::new(binary)
        .args(args)
        .env_clear()
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("spawn failed: {error}"))?;

    if let Some(input) = input {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "runtime stdin unavailable".to_owned())?;
        stdin
            .write_all(&input)
            .map_err(|error| format!("runtime stdin write failed: {error}"))?;
    }

    let started = Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("wait failed: {error}"))?
        {
            let output = child
                .wait_with_output()
                .map_err(|error| format!("read output failed: {error}"))?;
            return Ok(CommandOutput {
                stdout: output.stdout,
                status_success: status.success(),
                status_code: status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "signal".to_owned()),
            });
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("timed out after {} ms", timeout.as_millis()));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn discover_cargo_extension_binaries(
    paths: &[PathBuf],
) -> Result<Vec<PathBuf>, ManifestLoaderError> {
    let mut binaries = BTreeSet::new();
    for path in paths {
        let path = match secure_cargo_extension_search_path(path) {
            Ok(Some(path)) => path,
            Ok(None) => continue,
            Err(error) => return Err(error),
        };
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(ManifestLoaderError::Io(error.to_string())),
        };
        for entry in entries {
            let entry = entry.map_err(|error| ManifestLoaderError::Io(error.to_string()))?;
            let file_name = entry.file_name();
            if is_cargo_extension_name(&file_name) && is_executable_regular_file(&entry.path()) {
                binaries.insert(entry.path().canonicalize().map_err(|error| {
                    ManifestLoaderError::Io(format!("cargo extension path unavailable: {error}"))
                })?);
            }
        }
    }
    Ok(binaries.into_iter().collect())
}

fn is_cargo_extension_name(name: &OsStr) -> bool {
    name.to_string_lossy().starts_with("jyowo-plugin-")
}

fn secure_cargo_extension_search_path(path: &Path) -> Result<Option<PathBuf>, ManifestLoaderError> {
    ensure_no_world_writable_ancestors(path, "cargo extension search path")?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ManifestLoaderError::Io(error.to_string())),
    };
    if metadata.file_type().is_symlink() {
        return Err(ManifestLoaderError::Io(
            "cargo extension search path must not be a symlink".to_owned(),
        ));
    }
    if !metadata.is_dir() {
        return Ok(None);
    }
    if is_world_writable(&metadata) {
        return Err(ManifestLoaderError::Io(
            "cargo extension search path must not be world-writable".to_owned(),
        ));
    }
    path.canonicalize()
        .map(Some)
        .map_err(|error| ManifestLoaderError::Io(error.to_string()))
}

#[cfg(unix)]
fn ensure_no_world_writable_ancestors(path: &Path, label: &str) -> Result<(), ManifestLoaderError> {
    use std::os::unix::fs::PermissionsExt;

    for ancestor in path.ancestors().skip(1) {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(ManifestLoaderError::Io(error.to_string())),
        };
        let mode = metadata.permissions().mode();
        if mode & 0o002 != 0 && mode & 0o1000 == 0 {
            return Err(ManifestLoaderError::Io(format!(
                "{label} ancestors must not be world-writable"
            )));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_no_world_writable_ancestors(
    _path: &Path,
    _label: &str,
) -> Result<(), ManifestLoaderError> {
    Ok(())
}

#[cfg(unix)]
fn is_executable_regular_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::symlink_metadata(path)
        .map(|metadata| {
            !metadata.file_type().is_symlink()
                && metadata.is_file()
                && metadata.permissions().mode() & 0o111 != 0
        })
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| !metadata.file_type().is_symlink() && metadata.is_file())
        .unwrap_or(false)
}

#[cfg(unix)]
fn is_world_writable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o002 != 0
}

#[cfg(not(unix))]
fn is_world_writable(_metadata: &fs::Metadata) -> bool {
    false
}

fn decode_manifest_metadata(
    binary: &Path,
    bytes: &[u8],
) -> Result<ManifestRecord, ManifestValidationFailure> {
    let raw_hash = sha256(bytes);
    let value = serde_json::from_slice::<Value>(bytes).map_err(|error| {
        cargo_extension_failure(
            binary.to_path_buf(),
            bytes.to_vec(),
            format!("metadata json parse failed: {error}"),
            None,
            None,
        )
    })?;
    let package_metadata = value
        .get("package_metadata")
        .and_then(Value::as_object)
        .map(|metadata| {
            metadata
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let manifest_value = value.get("manifest").cloned().unwrap_or(value);
    let origin = ManifestOrigin::CargoExtension {
        binary: binary.to_path_buf(),
        package_metadata,
    };
    let partial_name = manifest_value
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let partial_version = manifest_value
        .get("version")
        .and_then(Value::as_str)
        .map(str::to_owned);
    validate_manifest_schema(
        &manifest_value,
        &origin,
        partial_name.as_ref(),
        partial_version.as_ref(),
        raw_hash,
    )
    .map_err(|error| match error {
        ManifestLoaderError::Validation(mut failure) => {
            failure.raw_bytes_hash = raw_hash;
            failure
        }
        ManifestLoaderError::Io(details) => cargo_extension_failure(
            binary.to_path_buf(),
            bytes.to_vec(),
            details,
            partial_name.clone(),
            partial_version.clone(),
        ),
        ManifestLoaderError::UnsupportedSource(details) => cargo_extension_failure(
            binary.to_path_buf(),
            bytes.to_vec(),
            details,
            partial_name.clone(),
            partial_version.clone(),
        ),
    })?;
    let manifest = serde_json::from_value::<PluginManifest>(manifest_value).map_err(|error| {
        cargo_extension_failure(
            binary.to_path_buf(),
            bytes.to_vec(),
            format!("metadata manifest decode failed: {error}"),
            None,
            None,
        )
    })?;
    let canonical = ManifestSigner::canonical_payload(&manifest).map_err(|error| {
        cargo_extension_failure(
            binary.to_path_buf(),
            bytes.to_vec(),
            format!("metadata manifest canonicalization failed: {error}"),
            Some(manifest.name.to_string()),
            Some(manifest.version.to_string()),
        )
    })?;
    ManifestRecord::new(manifest.clone(), origin, sha256(&canonical))
        .map_err(|error| {
            cargo_extension_failure(
                binary.to_path_buf(),
                bytes.to_vec(),
                format!("metadata manifest validation failed: {error}"),
                Some(manifest.name.to_string()),
                Some(manifest.version.to_string()),
            )
        })
        .map_err(|mut failure| {
            failure.raw_bytes_hash = raw_hash;
            failure
        })
}

fn cargo_extension_failure(
    binary: PathBuf,
    bytes: Vec<u8>,
    details: String,
    partial_name: Option<String>,
    partial_version: Option<String>,
) -> ManifestValidationFailure {
    ManifestValidationFailure {
        origin: Some(ManifestOrigin::CargoExtension {
            binary,
            package_metadata: BTreeMap::new(),
        }),
        partial_name,
        partial_version,
        raw_bytes_hash: sha256(&bytes),
        failure: EventManifestValidationFailure::CargoExtensionMetadataMalformed {
            details: details.clone(),
        },
        details,
    }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = digest::digest(&digest::SHA256, bytes);
    let mut output = [0_u8; 32];
    output.copy_from_slice(digest.as_ref());
    output
}

fn hook_event_payload(event: &HookEvent) -> Value {
    match event {
        HookEvent::UserPromptSubmit { run_id, input } => {
            json!({ "kind": event.kind(), "run_id": run_id, "input": input })
        }
        HookEvent::PreToolUse {
            tool_use_id,
            tool_name,
            input,
        } => json!({
            "kind": event.kind(),
            "tool_use_id": tool_use_id,
            "tool_name": tool_name,
            "input": input,
        }),
        HookEvent::PostToolUse {
            tool_use_id,
            result,
        } => json!({ "kind": event.kind(), "tool_use_id": tool_use_id, "result": result }),
        HookEvent::PostToolUseFailure { tool_use_id, .. } => {
            json!({ "kind": event.kind(), "tool_use_id": tool_use_id, "error": "[withheld]" })
        }
        HookEvent::PermissionRequest {
            request_id,
            subject,
            detail,
        } => json!({
            "kind": event.kind(),
            "request_id": request_id,
            "subject": subject,
            "detail": detail,
        }),
        HookEvent::SessionStart { session_id } | HookEvent::SessionEnd { session_id, .. } => {
            json!({ "kind": event.kind(), "session_id": session_id })
        }
        HookEvent::Setup { workspace_root } => {
            json!({ "kind": event.kind(), "workspace_root_present": workspace_root.is_some() })
        }
        HookEvent::TransformToolResult {
            tool_use_id,
            result,
        } => json!({ "kind": event.kind(), "tool_use_id": tool_use_id, "result": result }),
        HookEvent::TransformTerminalOutput { tool_use_id, raw } => {
            json!({ "kind": event.kind(), "tool_use_id": tool_use_id, "raw_bytes": raw.len() })
        }
        HookEvent::PreToolSearch {
            tool_use_id,
            query,
            query_kind,
        } => json!({
            "kind": event.kind(),
            "tool_use_id": tool_use_id,
            "query": query,
            "query_kind": query_kind,
        }),
        HookEvent::PostToolSearchMaterialize {
            tool_use_id,
            materialized,
            backend,
            cache_impact,
        } => json!({
            "kind": event.kind(),
            "tool_use_id": tool_use_id,
            "materialized": materialized,
            "backend": backend,
            "cache_impact": cache_impact,
        }),
        _ => json!({ "kind": event.kind() }),
    }
}

fn hook_context_payload(ctx: &HookContext) -> Value {
    json!({
        "tenant_id": ctx.tenant_id,
        "session_id": ctx.session_id,
        "run_id": ctx.run_id,
        "turn_index": ctx.turn_index,
        "correlation_id": ctx.correlation_id,
        "causation_id": ctx.causation_id,
        "trust_level": ctx.trust_level,
        "permission_mode": ctx.permission_mode,
        "interactivity": ctx.interactivity,
        "replay_mode": format!("{:?}", ctx.replay_mode),
        "workspace_root_present": ctx.view.workspace_root().is_some(),
    })
}

fn hook_outcome_from_value(value: Value) -> Result<HookOutcome, HookError> {
    if value.is_null() {
        return Ok(HookOutcome::Continue);
    }
    let Some(object) = value.as_object() else {
        return Err(HookError::ProtocolParse(
            "hook.handle result must be an object or null".to_owned(),
        ));
    };
    let kind = object
        .get("type")
        .or_else(|| object.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("continue");
    match kind {
        "continue" => Ok(HookOutcome::Continue),
        "block" => Ok(HookOutcome::Block {
            reason: object
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("blocked by plugin hook")
                .to_owned(),
        }),
        "pre_tool_use" => Ok(HookOutcome::PreToolUse(PreToolUseOutcome {
            rewrite_input: object.get("rewrite_input").cloned(),
            override_permission: object
                .get("override_permission")
                .cloned()
                .map(plugin_permission_override_from_value)
                .transpose()?,
            additional_context: object
                .get("additional_context")
                .map(context_patch_from_value)
                .transpose()?,
            block: object
                .get("block")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })),
        "rewrite_input" => Ok(HookOutcome::RewriteInput(
            object.get("value").cloned().unwrap_or(Value::Null),
        )),
        "override_permission" => Ok(HookOutcome::OverridePermission(
            object
                .get("decision")
                .cloned()
                .ok_or_else(|| {
                    HookError::ProtocolParse("override_permission requires decision".to_owned())
                })
                .and_then(plugin_permission_override_from_value)?,
        )),
        "add_context" => Ok(HookOutcome::AddContext(context_patch_from_value(
            object
                .get("patch")
                .or_else(|| object.get("context"))
                .unwrap_or(&value),
        )?)),
        "transform" => Ok(HookOutcome::Transform(
            object.get("value").cloned().unwrap_or(Value::Null),
        )),
        other => Err(HookError::ProtocolParse(format!(
            "unsupported hook outcome: {other}"
        ))),
    }
}

fn plugin_permission_override_from_value(value: Value) -> Result<Decision, HookError> {
    let decision = serde_json::from_value::<Decision>(value)
        .map_err(|error| HookError::ProtocolParse(error.to_string()))?;
    if matches!(decision, Decision::DenyOnce | Decision::DenyPermanent) {
        return Ok(decision);
    }
    Err(HookError::ProtocolParse(
        "plugin hook permission override may only deny".to_owned(),
    ))
}

fn context_patch_from_value(value: &Value) -> Result<ContextPatch, HookError> {
    let Some(object) = value.as_object() else {
        return Err(HookError::ProtocolParse(
            "context patch must be an object".to_owned(),
        ));
    };
    let role = match object
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("assistant_hint")
    {
        "system_append" => ContextPatchRole::SystemAppend,
        "user_prefix" => ContextPatchRole::UserPrefix,
        "user_suffix" => ContextPatchRole::UserSuffix,
        "assistant_hint" => ContextPatchRole::AssistantHint,
        role => {
            return Err(HookError::ProtocolParse(format!(
                "unsupported context patch role: {role}"
            )));
        }
    };
    Ok(ContextPatch {
        role,
        content: object
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        apply_to_next_turn_only: object
            .get("apply_to_next_turn_only")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    })
}

fn current_skill_platform() -> SkillPlatform {
    if cfg!(target_os = "macos") {
        SkillPlatform::Macos
    } else if cfg!(target_os = "windows") {
        SkillPlatform::Windows
    } else {
        SkillPlatform::Linux
    }
}

fn mcp_server_spec_for(manifest: &PluginManifest, entry: &McpManifestEntry) -> McpServerSpec {
    McpServerSpec::new(
        McpServerId(entry.name.clone()),
        entry.name.clone(),
        TransportChoice::InProcess,
        McpServerSource::Plugin(manifest.plugin_id()),
    )
}

fn tool_descriptor_for(manifest: &PluginManifest, entry: &ToolManifestEntry) -> ToolDescriptor {
    ToolDescriptor {
        name: entry.name.clone(),
        display_name: entry.name.clone(),
        description: manifest
            .description
            .clone()
            .unwrap_or_else(|| format!("{} sidecar tool", manifest.name)),
        category: "plugin".to_owned(),
        group: ToolGroup::Custom("plugin".to_owned()),
        version: manifest.version.to_string(),
        input_schema: entry.input_schema.clone(),
        output_schema: None,
        dynamic_schema: false,
        properties: ToolProperties {
            is_concurrency_safe: false,
            is_read_only: !entry.destructive,
            is_destructive: entry.destructive,
            long_running: None,
            defer_policy: DeferPolicy::AlwaysLoad,
        },
        trust_level: manifest.trust_level,
        required_capabilities: Vec::new(),
        service_binding: None,
        budget: default_result_budget(),
        provider_restriction: ProviderRestriction::All,
        origin: ToolOrigin::Plugin {
            plugin_id: manifest.plugin_id(),
            trust: manifest.trust_level,
        },
        search_hint: None,
        metadata: ToolDescriptorMetadata {
            integration_source: ToolIntegrationSource::Plugin,
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use bytes::Bytes;
    use harness_contracts::{
        CapabilityRegistry, KillScope, LocalIsolationTag, ToolCapability, ToolUseId, TrustLevel,
    };
    use harness_sandbox::{
        ActivityHandle, ExecOutcome, NetworkPolicySupport, ProcessHandle, ResourceLimitSupport,
        SandboxCapabilities, SessionSnapshotFile, SnapshotSpec, WorkspacePolicySupport,
    };

    #[test]
    fn default_manifest_loader_has_no_implicit_search_paths() {
        assert!(CargoExtensionManifestLoader::new().paths().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn search_path_rejects_symlink_directory() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        let link = root.path().join("link");
        fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = secure_cargo_extension_search_path(&link).unwrap_err();

        assert!(
            matches!(error, ManifestLoaderError::Io(message) if message.contains("must not be a symlink"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn search_path_rejects_world_writable_directory() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("extensions");
        fs::create_dir(&path).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o777);
        fs::set_permissions(&path, permissions).unwrap();

        let error = secure_cargo_extension_search_path(&path).unwrap_err();

        assert!(
            matches!(error, ManifestLoaderError::Io(message) if message.contains("world-writable"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn search_path_rejects_world_writable_ancestor() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("writable-parent");
        let path = parent.join("extensions");
        fs::create_dir_all(&path).unwrap();
        let mut permissions = fs::metadata(&parent).unwrap().permissions();
        permissions.set_mode(0o777);
        fs::set_permissions(&parent, permissions).unwrap();

        let error = secure_cargo_extension_search_path(&path).unwrap_err();

        assert!(
            matches!(error, ManifestLoaderError::Io(message) if message.contains("world-writable"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn discovery_ignores_symlink_binaries() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("actual-plugin");
        let link = root.path().join("jyowo-plugin-linked");
        fs::write(&target, "#!/bin/sh\n").unwrap();
        let mut permissions = fs::metadata(&target).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&target, permissions).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let binaries = discover_cargo_extension_binaries(&[root.path().to_path_buf()]).unwrap();

        assert!(binaries.is_empty());
    }

    #[tokio::test]
    async fn sandboxed_extension_command_uses_backend_and_locked_policy() {
        let root = tempfile::tempdir().unwrap();
        let runtime_root = root.path().join("runtime");
        let workspace_root = root.path().join("workspace");
        fs::create_dir_all(&runtime_root).unwrap();
        fs::create_dir_all(&workspace_root).unwrap();
        let binary = runtime_root.join("jyowo-plugin-sidecar");
        let backend = Arc::new(RecordingSandbox::new(br#"{"ok":true}"#.to_vec()));
        let sandbox: Arc<dyn SandboxBackend> = backend.clone();

        let output = run_extension_command(
            &binary,
            &["--harness-runtime"],
            Some(br#"{"jsonrpc":"2.0"}"#.to_vec()),
            Duration::from_millis(250),
            Some(sandbox),
            Some(SandboxMode::OsLevel(LocalIsolationTag::Seatbelt)),
            Some(workspace_root.clone()),
        )
        .await
        .unwrap();

        assert_eq!(output.stdout, br#"{"ok":true}"#);
        assert!(output.status_success);
        assert_eq!(output.status_code, "0");

        let spec = backend.spec();
        assert_eq!(spec.command, binary.display().to_string());
        assert_eq!(spec.args, vec!["--harness-runtime"]);
        assert_eq!(spec.cwd, Some(runtime_root));
        assert_eq!(spec.stdin, StdioSpec::Piped);
        assert_eq!(spec.stdout, StdioSpec::Piped);
        assert_eq!(spec.stderr, StdioSpec::Null);
        assert_eq!(spec.workspace_access, WorkspaceAccess::ReadOnly);
        assert_eq!(spec.policy.network, NetworkAccess::None);
        assert_eq!(
            spec.policy.scope,
            SandboxScope::WorkspacePlus(vec![workspace_root])
        );
        assert!(matches!(
            spec.policy.mode,
            SandboxMode::OsLevel(LocalIsolationTag::Seatbelt)
        ));
        assert_eq!(spec.policy.resource_limits.max_wall_clock_ms, Some(250));
    }

    #[tokio::test]
    async fn sidecar_tool_plan_uses_external_plugin_capability_channel() {
        let manifest = PluginManifest {
            name: crate::PluginName::new("sidecar-plugin").unwrap(),
            version: semver::Version::parse("1.0.0").unwrap(),
            trust_level: TrustLevel::UserControlled,
            description: None,
            authors: Vec::new(),
            repository: None,
            signature: None,
            capabilities: crate::PluginCapabilities::default(),
            dependencies: Vec::new(),
            min_harness_version: semver::VersionReq::parse(">=0.0.0").unwrap(),
        };
        let entry = ToolManifestEntry {
            name: "sidecar_tool".to_owned(),
            destructive: false,
            input_schema: json!({ "type": "object" }),
        };
        let proxy = CargoExtensionToolProxy {
            descriptor: tool_descriptor_for(&manifest, &entry),
            client: CargoExtensionRuntimeClient {
                binary: PathBuf::from("/tmp/jyowo-plugin-sidecar"),
                sandbox: None,
                sandbox_mode: None,
                timeout: Duration::from_secs(1),
                workspace_root: None,
            },
        };

        let plan = proxy.plan(&json!({}), &tool_ctx()).await.unwrap();

        assert_eq!(
            plan.execution_channel,
            ToolExecutionChannel::ExternalCapability {
                capability: ToolCapability::Custom("plugin_sidecar".to_owned()),
            }
        );
    }

    #[test]
    fn hook_outcome_rejects_plugin_allow_permission_override() {
        let pre_tool_use = hook_outcome_from_value(json!({
            "type": "pre_tool_use",
            "override_permission": "allow_once"
        }));
        let permission_request = hook_outcome_from_value(json!({
            "type": "override_permission",
            "decision": "allow_session"
        }));

        assert!(
            matches!(pre_tool_use, Err(HookError::ProtocolParse(message)) if message.contains("may only deny"))
        );
        assert!(
            matches!(permission_request, Err(HookError::ProtocolParse(message)) if message.contains("may only deny"))
        );
    }

    #[test]
    fn hook_outcome_allows_plugin_deny_permission_override() {
        let outcome = hook_outcome_from_value(json!({
            "type": "pre_tool_use",
            "override_permission": "deny_once"
        }))
        .unwrap();

        assert!(matches!(
            outcome,
            HookOutcome::PreToolUse(PreToolUseOutcome {
                override_permission: Some(Decision::DenyOnce),
                ..
            })
        ));
    }

    fn tool_ctx() -> ToolContext {
        ToolContext {
            tool_use_id: ToolUseId::new(),
            run_id: RunId::new(),
            session_id: SessionId::new(),
            tenant_id: TenantId::SINGLE,
            correlation_id: CorrelationId::new(),
            agent_id: harness_contracts::AgentId::from_u128(1),
            subagent_depth: 0,
            workspace_root: std::env::temp_dir(),
            project_workspace_root: None,
            sandbox: None,
            cap_registry: Arc::new(CapabilityRegistry::default()),
            redactor: Arc::new(NoopRedactor),
            interrupt: harness_tool::InterruptToken::default(),
            parent_run: None,
            model: None,
            model_config_id: None,
            memory_thread_settings: None,
            actor_source: harness_contracts::PermissionActorSource::ParentRun,
        }
    }

    struct RecordingSandbox {
        spec: Mutex<Option<ExecSpec>>,
        stdout: Vec<u8>,
    }

    impl RecordingSandbox {
        fn new(stdout: Vec<u8>) -> Self {
            Self {
                spec: Mutex::new(None),
                stdout,
            }
        }

        fn spec(&self) -> ExecSpec {
            self.spec
                .lock()
                .expect("spec lock should work")
                .clone()
                .expect("sandbox should record exec spec")
        }
    }

    #[async_trait]
    impl SandboxBackend for RecordingSandbox {
        fn backend_id(&self) -> &str {
            "recording"
        }

        fn capabilities(&self) -> SandboxCapabilities {
            SandboxCapabilities {
                network: NetworkPolicySupport {
                    none: true,
                    loopback_only: false,
                    allowlist: false,
                    unrestricted: true,
                },
                max_concurrent_execs: 1,
                resource_limit_support: ResourceLimitSupport {
                    wall_clock: true,
                    ..ResourceLimitSupport::default()
                },
                workspace: WorkspacePolicySupport {
                    read_only: true,
                    ..WorkspacePolicySupport::default()
                },
                ..SandboxCapabilities::default()
            }
        }

        async fn execute(
            &self,
            spec: ExecSpec,
            _ctx: ExecContext,
        ) -> Result<ProcessHandle, SandboxError> {
            *self.spec.lock().expect("spec lock should work") = Some(spec);
            let stdout = self.stdout.clone();
            Ok(ProcessHandle {
                pid: Some(7),
                stdout: Some(Box::pin(stream::once(async move { Bytes::from(stdout) }))),
                stderr: None,
                stdin: Some(Box::pin(tokio::io::sink())),
                cwd_marker: None,
                activity: Arc::new(ReadyActivity),
            })
        }

        async fn snapshot_session(
            &self,
            _spec: &SnapshotSpec,
        ) -> Result<SessionSnapshotFile, SandboxError> {
            Ok(SessionSnapshotFile::default())
        }

        async fn restore_session(
            &self,
            _snapshot: &SessionSnapshotFile,
        ) -> Result<(), SandboxError> {
            Ok(())
        }

        async fn shutdown(&self) -> Result<(), SandboxError> {
            Ok(())
        }
    }

    struct ReadyActivity;

    #[async_trait]
    impl ActivityHandle for ReadyActivity {
        async fn wait(&self) -> Result<ExecOutcome, SandboxError> {
            Ok(ExecOutcome::default())
        }

        async fn kill(&self, _signal: i32, _scope: KillScope) -> Result<(), SandboxError> {
            Ok(())
        }

        fn touch(&self) {}

        fn last_activity(&self) -> Instant {
            Instant::now()
        }
    }
}

#![allow(dead_code)]
#![allow(unused_imports)]

use super::*;
use harness_sandbox::{NetworkPolicySupport, WorkspacePolicySupport};

pub(crate) fn permission_option_id_for_decision(
    pending: &jyowo_harness_sdk::ext::PendingPermissionRequest,
    decision: Decision,
) -> PermissionOptionId {
    pending
        .decision_options
        .iter()
        .find(|option| option.decision == decision)
        .map(|option| option.option_id.clone())
        .expect("pending permission should expose the requested decision option")
}

pub(crate) fn approve_permission_option_id(
    pending: &jyowo_harness_sdk::ext::PendingPermissionRequest,
) -> PermissionOptionId {
    permission_option_id_for_decision(pending, Decision::AllowOnce)
}

pub(crate) async fn run_with_mcp_transport_approval<T>(
    state: &DesktopRuntimeState,
    command: impl std::future::Future<Output = Result<T, jyowo_desktop_shell::commands::CommandErrorPayload>>
        + Send
        + 'static,
) -> Result<T, jyowo_desktop_shell::commands::CommandErrorPayload>
where
    T: Send + 'static,
{
    let command_task = tokio::spawn(command);
    // Restart first gives the previous MCP connection up to one second to
    // shut down before the replacement requests transport permission.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let pending = loop {
        if let Some(pending) = settings_permission_resolver(state)
            .pending_permission_requests()
            .into_iter()
            .find(|pending| {
                matches!(
                    &pending.request.subject,
                    PermissionSubject::Custom { kind, .. } if kind == "mcp_transport"
                )
            })
        {
            break pending;
        }

        if command_task.is_finished() {
            return command_task
                .await
                .expect("mcp command task should complete without panicking");
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("mcp transport permission request should become pending");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    };

    settings_permission_resolver(state)
        .resolve_option_for(
            pending.request.request_id,
            pending.request.tenant_id,
            pending.request.session_id,
            approve_permission_option_id(&pending),
            Decision::AllowOnce,
            None,
        )
        .await
        .map_err(|error| CommandErrorPayload {
            code: "RUNTIME_OPERATION_FAILED",
            message: error.to_string(),
        })?;

    command_task
        .await
        .expect("mcp command task should complete without panicking")
}

fn settings_permission_resolver(
    state: &DesktopRuntimeState,
) -> jyowo_harness_sdk::ext::ResolverHandle {
    state
        .settings_runtime()
        .expect("settings runtime should be available")
        .permission_resolver_handle()
        .expect("settings permission resolver should be available")
}

#[derive(Debug, Default)]
struct AllowExecPreflightSandbox;

#[async_trait]
impl jyowo_harness_sdk::ext::SandboxBackend for AllowExecPreflightSandbox {
    fn backend_id(&self) -> &'static str {
        "allow-exec-preflight"
    }

    fn capabilities(&self) -> jyowo_harness_sdk::ext::SandboxCapabilities {
        jyowo_harness_sdk::ext::SandboxCapabilities {
            network: NetworkPolicySupport {
                none: true,
                loopback_only: false,
                allowlist: false,
                unrestricted: true,
            },
            workspace: WorkspacePolicySupport {
                read_write_all: true,
                read_only: false,
                writable_subpaths: false,
            },
            max_concurrent_execs: 1,
            ..jyowo_harness_sdk::ext::SandboxCapabilities::default()
        }
    }

    fn preflight_execute(
        &self,
        _spec: &jyowo_harness_sdk::ext::ExecSpec,
    ) -> Result<(), harness_contracts::SandboxError> {
        Ok(())
    }

    async fn execute(
        &self,
        _spec: jyowo_harness_sdk::ext::ExecSpec,
        _ctx: jyowo_harness_sdk::ext::ExecContext,
    ) -> Result<jyowo_harness_sdk::ext::ProcessHandle, harness_contracts::SandboxError> {
        Err(harness_contracts::SandboxError::CapabilityMismatch {
            capability: "execute".to_owned(),
            detail: "test sandbox only supports preflight".to_owned(),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &jyowo_harness_sdk::ext::SnapshotSpec,
    ) -> Result<jyowo_harness_sdk::ext::SessionSnapshotFile, harness_contracts::SandboxError> {
        Err(harness_contracts::SandboxError::SnapshotUnsupported {
            kind: "allow_exec_preflight_snapshot".to_owned(),
        })
    }

    async fn restore_session(
        &self,
        _snapshot: &jyowo_harness_sdk::ext::SessionSnapshotFile,
    ) -> Result<(), harness_contracts::SandboxError> {
        Err(harness_contracts::SandboxError::SnapshotUnsupported {
            kind: "allow_exec_preflight_restore".to_owned(),
        })
    }

    async fn shutdown(&self) -> Result<(), harness_contracts::SandboxError> {
        Ok(())
    }
}

pub(crate) async fn runtime_state_with_settings_runtime_for_workspace(
    workspace: PathBuf,
) -> DesktopRuntimeState {
    std::fs::create_dir_all(&workspace).unwrap();
    write_test_provider_settings(&workspace);
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let settings_runtime: Arc<DesktopSettingsRuntime> = Arc::new(
        DesktopSettingsRuntime::try_from(
            DesktopSettingsRuntime::builder()
                .with_options(test_settings_options(&workspace))
                .with_model(TestModelProvider::default())
                .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
                .with_sandbox(NoopSandbox::new())
                .with_stream_permission_broker_arc(
                    stream_permission_runtime.broker(),
                    stream_permission_runtime.resolver_handle(),
                )
                .build()
                .await
                .expect("settings runtime should build with stream permission runtime"),
        )
        .expect("settings runtime must not own memory extraction"),
    );

    let mut state =
        DesktopRuntimeState::with_settings_runtime_for_workspace(workspace, settings_runtime)
            .expect("state should use the settings permission broker");
    let state_workspace = state.workspace_root().to_path_buf();
    use_test_provider_settings_store(&mut state, &state_workspace);
    state
}

pub(crate) async fn runtime_state_with_mcp_registry(
    registry: McpRegistry,
    server_ids_to_inject: Vec<McpServerId>,
) -> DesktopRuntimeState {
    runtime_state_with_mcp_registry_for_workspace(
        unique_workspace("mcp-registry"),
        registry,
        server_ids_to_inject,
    )
    .await
}

pub(crate) async fn runtime_state_with_mcp_registry_for_workspace(
    workspace: PathBuf,
    registry: McpRegistry,
    server_ids_to_inject: Vec<McpServerId>,
) -> DesktopRuntimeState {
    std::fs::create_dir_all(&workspace).unwrap();
    write_test_provider_settings(&workspace);
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let settings_runtime: Arc<DesktopSettingsRuntime> = Arc::new(
        DesktopSettingsRuntime::try_from(
            DesktopSettingsRuntime::builder()
                .with_options(test_settings_options(&workspace))
                .with_model(TestModelProvider::default())
                .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
                .with_sandbox(AllowExecPreflightSandbox)
                .with_stream_permission_broker_arc(
                    stream_permission_runtime.broker(),
                    stream_permission_runtime.resolver_handle(),
                )
                .with_mcp_config(McpConfig {
                    registry,
                    server_ids_to_inject,
                    event_sink: Arc::new(NoopMcpEventSink),
                })
                .build()
                .await
                .expect("harness should build with MCP registry"),
        )
        .expect("settings runtime must not own memory extraction"),
    );

    let mut state =
        DesktopRuntimeState::with_settings_runtime_for_workspace(workspace, settings_runtime)
            .expect("state should use the harness permission broker");
    let state_workspace = state.workspace_root().to_path_buf();
    use_test_provider_settings_store(&mut state, &state_workspace);
    state.set_mcp_server_store_for_test(Arc::new(RecordingMcpServerStore::default()));
    state
}

pub(crate) fn test_settings_options(workspace: &Path) -> HarnessOptions {
    let mut options = HarnessOptions::default();
    options.workspace_root = workspace.to_path_buf();
    options.model_id = "test-model".to_owned();
    options
}

pub(crate) fn unique_workspace(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-desktop-{name}-{}-{}",
        std::process::id(),
        SessionId::new()
    ))
}

pub(crate) fn provider_settings_store_for_workspace(
    workspace: &Path,
) -> DesktopProviderSettingsStore {
    let layout = test_storage_layout_for_workspace(workspace);
    DesktopProviderSettingsStore::new_with_layout(layout, workspace.to_path_buf())
}

pub(crate) fn execution_settings_store_for_workspace(
    workspace: &Path,
) -> DesktopExecutionSettingsStore {
    DesktopExecutionSettingsStore::global_only_with_layout(test_storage_layout_for_workspace(
        workspace,
    ))
}

pub(crate) fn use_test_provider_settings_store(state: &mut DesktopRuntimeState, workspace: &Path) {
    let layout = test_storage_layout_for_workspace(workspace);
    let store = provider_settings_store_for_workspace(workspace);
    let record = store
        .load_record()
        .expect("test provider settings should load")
        .expect("test provider settings should exist");
    let config_id = record
        .default_config_id
        .as_deref()
        .expect("test provider settings should have default config");
    let config = record
        .configs
        .iter()
        .find(|config| config.id == config_id)
        .expect("test provider settings default config should exist");
    state
        .set_active_runtime_provider_config_for_test(config)
        .expect("test active runtime provider binding should update");
    state.set_provider_settings_store_for_test(Arc::new(store));
    state.set_skill_store_for_test(Arc::new(DesktopSkillStore::global(layout.clone())));
    state.set_config_stores_for_test(
        jyowo_desktop_shell::commands::stores::GlobalConfigStore::new(layout.clone()),
        Some(
            jyowo_desktop_shell::commands::stores::ProjectConfigStore::new(
                layout,
                workspace.to_path_buf(),
            ),
        ),
    );
}

pub(crate) fn test_storage_layout_for_workspace(
    workspace: &Path,
) -> jyowo_desktop_shell::storage_layout::StorageLayout {
    jyowo_desktop_shell::storage_layout::StorageLayout::new(
        jyowo_desktop_shell::storage_layout::JyowoHome::new(
            workspace.join(".jyowo-test-home").join(".jyowo"),
        ),
    )
}

pub(crate) fn skill_markdown(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\nSkill body for {name}.\n")
}

pub(crate) fn write_skill_package(
    root: &std::path::Path,
    package_name: &str,
    skill_name: &str,
    description: &str,
    resource: Option<(&str, &str)>,
) -> PathBuf {
    let package_path = root.join(package_name);
    std::fs::create_dir_all(&package_path).unwrap();
    std::fs::write(
        package_path.join("SKILL.md"),
        skill_markdown(skill_name, description),
    )
    .unwrap();
    if let Some((relative_path, content)) = resource {
        let resource_path = package_path.join(relative_path);
        std::fs::create_dir_all(resource_path.parent().unwrap()).unwrap();
        std::fs::write(resource_path, content).unwrap();
    }
    package_path.canonicalize().unwrap()
}

pub(crate) fn register_test_skill(state: &DesktopRuntimeState, name: &str, description: &str) {
    let settings_runtime = state
        .settings_runtime()
        .expect("runtime state should include harness");
    let skill = parse_skill_markdown(
        &skill_markdown(name, description),
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("test skill should parse");
    settings_runtime
        .skill_registry()
        .register_batch(vec![skill])
        .expect("test skill should register");
}

pub(crate) fn stdio_mcp_fixture_script() -> String {
    r#"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"0.1.0"}}}'
      ;;
    *'"method":"tools/list"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","description":"Echo input","inputSchema":{"type":"object"}}]}}'
      ;;
  esac
done
"#
    .to_owned()
}

pub(crate) struct StaticMcpConnection {
    pub(crate) tools: Vec<McpToolDescriptor>,
}

#[async_trait]
impl McpConnection for StaticMcpConnection {
    fn connection_id(&self) -> &str {
        "static-test-mcp"
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpError> {
        Ok(self.tools.clone())
    }

    async fn call_tool(&self, _name: &str, _args: Value) -> Result<McpToolResult, McpError> {
        Ok(McpToolResult::text("ok"))
    }

    async fn shutdown(&self) -> Result<(), McpError> {
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct RecordingProviderSettingsStore {
    pub(crate) fail_record: bool,
    pub(crate) record: Mutex<Option<ProviderSettingsRecord>>,
}

impl ProviderSettingsStore for RecordingProviderSettingsStore {
    fn load_record(
        &self,
    ) -> Result<Option<ProviderSettingsRecord>, jyowo_desktop_shell::commands::CommandErrorPayload>
    {
        Ok(self.record.lock().unwrap().clone())
    }

    fn save_record(
        &self,
        record: &ProviderSettingsRecord,
    ) -> Result<(), jyowo_desktop_shell::commands::CommandErrorPayload> {
        if self.fail_record {
            return Err(jyowo_desktop_shell::commands::CommandErrorPayload {
                code: "RUNTIME_OPERATION_FAILED",
                message: "record write failed".to_owned(),
            });
        }

        *self.record.lock().unwrap() = Some(record.clone());
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct RecordingMcpServerStore {
    pub(crate) record: Mutex<Option<McpServerConfigRecord>>,
}

impl McpServerStore for RecordingMcpServerStore {
    fn load_records(
        &self,
    ) -> Result<Vec<McpServerConfigRecord>, jyowo_desktop_shell::commands::CommandErrorPayload>
    {
        Ok(self.record.lock().unwrap().clone().into_iter().collect())
    }

    fn save_record(
        &self,
        record: &McpServerConfigRecord,
    ) -> Result<(), jyowo_desktop_shell::commands::CommandErrorPayload> {
        *self.record.lock().unwrap() = Some(record.clone());
        Ok(())
    }

    fn delete_record(
        &self,
        id: &str,
    ) -> Result<(), jyowo_desktop_shell::commands::CommandErrorPayload> {
        let mut record = self.record.lock().unwrap();
        if record.as_ref().is_some_and(|record| record.id == id) {
            *record = None;
        }
        Ok(())
    }
}

pub(crate) struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    pub(crate) fn set(key: &'static str, value: &std::ffi::OsStr) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }

    pub(crate) fn remove(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

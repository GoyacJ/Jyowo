use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use bytes::Bytes;
use futures::{stream, stream::BoxStream};
use harness_contracts::{
    AgentId, CapabilityRegistry, KillScope, NetworkAccess, SandboxError, SandboxExitStatus,
    SkillId, SkillScriptRunDeclaration, SkillScriptRunFile, SkillScriptRunPreparation, TenantId,
    ToolUseId,
};
use harness_sandbox::{
    ActivityHandle, ExecContext, ExecOutcome, ExecSpec, NetworkPolicySupport, ProcessHandle,
    ResourceLimitSupport, SandboxBackend, SandboxCapabilities, SessionSnapshotFile, SnapshotSpec,
};
use harness_tool::{
    run_prepared_skill_script, skill_script_local_path_authorized, InterruptToken,
    SkillScriptLocalPathPolicy, SkillsRunScriptTool, Tool, ToolContext,
};
use serde_json::json;

#[test]
fn skill_script_local_runner_path_authorization_stays_inside_authorized_roots() {
    let policy = SkillScriptLocalPathPolicy {
        authorized_roots: vec!["/workspace/project".into()],
    };

    assert!(skill_script_local_path_authorized(
        Path::new("/workspace/project/scripts/run.sh"),
        &policy
    ));
    assert!(skill_script_local_path_authorized(
        Path::new("/workspace/project/tmp/../scripts/run.sh"),
        &policy
    ));
    assert!(!skill_script_local_path_authorized(
        Path::new("/workspace/other/run.sh"),
        &policy
    ));
}

#[tokio::test]
async fn skills_run_script_rejects_path_overrides() {
    let tool = SkillsRunScriptTool::default();
    let error = tool
        .validate(
            &json!({
                "name": "collector",
                "script_id": "collect",
                "path": "scripts/other.sh"
            }),
            &tool_context(),
        )
        .await
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("unknown skills_run_script field: path"));
}

#[tokio::test]
async fn skills_run_script_executes_prepared_request_through_the_sandbox() {
    let backend = Arc::new(RecordingBackend::default());
    let result = run_prepared_skill_script(prepared_request(), backend.clone(), &tool_context())
        .await
        .unwrap();

    assert_eq!(result.stdout, "sandbox-output");
    assert_eq!(
        result.enforced_policy.network,
        harness_skill::SkillScriptNetworkPolicy::Deny
    );
    let specs = backend.specs.lock().unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(
        specs[0].env,
        BTreeMap::from([("API_TOKEN".to_owned(), "secret-value".to_owned())])
    );
    assert_eq!(
        specs[0].secret_env_keys,
        BTreeSet::from(["API_TOKEN".to_owned()])
    );
    assert_eq!(specs[0].policy.network, NetworkAccess::None);
}

fn prepared_request() -> SkillScriptRunPreparation {
    SkillScriptRunPreparation {
        skill_id: SkillId("workspace:collector".to_owned()),
        skill_name: "collector".to_owned(),
        script_id: "collect".to_owned(),
        package_hash: "package-hash".to_owned(),
        arguments: json!({ "query": "open" }),
        declaration: SkillScriptRunDeclaration {
            path: PathBuf::from("scripts/collect.sh"),
            timeout_seconds: 30,
            max_stdout_bytes: 1024,
            max_stderr_bytes: 1024,
            max_output_bytes: 2048,
            max_artifact_count: 4,
            max_artifact_bytes: 4096,
            network_access: NetworkAccess::None,
            env_config_keys: BTreeMap::from([("API_TOKEN".to_owned(), "apiToken".to_owned())]),
            secret_env_keys: BTreeSet::from(["API_TOKEN".to_owned()]),
        },
        files: vec![SkillScriptRunFile {
            path: "scripts/collect.sh".to_owned(),
            content: "#!/bin/sh\n".to_owned(),
        }],
        env: BTreeMap::from([("API_TOKEN".to_owned(), "secret-value".to_owned())]),
    }
}

fn tool_context() -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: harness_contracts::SessionId::new(),
        tenant_id: TenantId::SINGLE,
        model: None,
        model_config_id: None,
        memory_thread_settings: None,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: std::env::temp_dir(),
        project_workspace_root: None,
        sandbox: None,
        cap_registry: Arc::new(CapabilityRegistry::default()),
        redactor: Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::default(),
        parent_run: None,
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
    }
}

#[derive(Default)]
struct RecordingBackend {
    specs: Mutex<Vec<ExecSpec>>,
}

#[async_trait]
impl SandboxBackend for RecordingBackend {
    fn backend_id(&self) -> &str {
        "recording-skill-script"
    }

    fn capabilities(&self) -> SandboxCapabilities {
        SandboxCapabilities {
            supports_streaming: true,
            supports_per_exec_env: true,
            network: NetworkPolicySupport {
                none: true,
                ..NetworkPolicySupport::default()
            },
            workspace: harness_sandbox::WorkspacePolicySupport {
                writable_subpaths: true,
                ..harness_sandbox::WorkspacePolicySupport::default()
            },
            max_concurrent_execs: 1,
            supports_kill_scope: vec![KillScope::Process, KillScope::ProcessGroup],
            supports_synchronous_kill_scope: vec![KillScope::ProcessGroup],
            resource_limit_support: ResourceLimitSupport {
                wall_clock: true,
                ..ResourceLimitSupport::default()
            },
            ..SandboxCapabilities::default()
        }
    }

    async fn execute(
        &self,
        spec: ExecSpec,
        _ctx: ExecContext,
    ) -> Result<ProcessHandle, SandboxError> {
        self.specs.lock().unwrap().push(spec);
        let stdout: Option<BoxStream<'static, Bytes>> =
            Some(Box::pin(stream::iter([Bytes::from("sandbox-output")])));
        Ok(ProcessHandle {
            pid: Some(42),
            stdout,
            stderr: None,
            stdin: None,
            cwd_marker: None,
            activity: Arc::new(DoneActivity),
        })
    }

    async fn snapshot_session(
        &self,
        _spec: &SnapshotSpec,
    ) -> Result<SessionSnapshotFile, SandboxError> {
        Err(SandboxError::SnapshotUnsupported {
            kind: "test".to_owned(),
        })
    }

    async fn restore_session(&self, _snapshot: &SessionSnapshotFile) -> Result<(), SandboxError> {
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), SandboxError> {
        Ok(())
    }
}

struct DoneActivity;

#[async_trait]
impl ActivityHandle for DoneActivity {
    async fn wait(&self) -> Result<ExecOutcome, SandboxError> {
        Ok(ExecOutcome {
            exit_status: SandboxExitStatus::Code(0),
            ..ExecOutcome::default()
        })
    }

    async fn kill(&self, _signal: i32, _scope: KillScope) -> Result<(), SandboxError> {
        Ok(())
    }

    fn kill_sync(&self, _signal: i32, _scope: KillScope) -> Result<(), SandboxError> {
        Ok(())
    }

    fn touch(&self) {}

    fn last_activity(&self) -> Instant {
        Instant::now()
    }
}

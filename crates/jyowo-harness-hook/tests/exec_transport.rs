#![cfg(feature = "exec")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use harness_contracts::{
    HookError, HookEventKind, HookFailureMode, InteractivityLevel, MessageRole, PermissionMode,
    RunId, TenantId, ToolUseId, TransportFailureKind, TrustLevel,
};
use harness_hook::{
    ExecHookTransport, HookContext, HookEvent, HookExecResourceLimits, HookExecSignalPolicy,
    HookExecSpec, HookMessageView, HookPayload, HookProtocolVersion, HookSessionView,
    HookTransport, ReplayMode, ToolDescriptorView, WorkingDir,
};
use serde_json::json;

#[test]
fn exec_transport_rejects_nonportable_custom_memory_limit() {
    let script = write_script("ok", "#!/bin/sh\nprintf '{}'\n");
    let mut spec = exec_spec("custom-memory", script);
    spec.resource_limits.memory_bytes = 64 * 1024 * 1024;

    let error = match ExecHookTransport::new(spec) {
        Ok(_) => panic!("limit unsupported"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        HookError::Transport {
            kind: TransportFailureKind::UnsupportedLimit { ref limit },
            ..
        } if limit == "memory_bytes"
    ));
}

#[tokio::test]
async fn exec_transport_counts_stderr_against_stdio_limit() {
    let script = write_script(
        "stderr",
        r#"#!/bin/sh
printf 'too much stderr' >&2
printf '{"protocol_version":"v1","outcome":{"continue":null}}'
"#,
    );
    let mut spec = exec_spec("stdio", script);
    spec.resource_limits.max_stdio_bytes = 8;
    let transport = ExecHookTransport::new(spec).unwrap();

    let error = transport
        .invoke(HookPayload {
            event: sample_pre_tool_use(),
            ctx: sample_context(),
        })
        .await
        .expect_err("stdio limit should fail");

    assert!(matches!(
        error,
        HookError::Transport {
            kind: TransportFailureKind::BodyTooLarge,
            ..
        }
    ));
}

fn exec_spec(handler_id: &str, command: PathBuf) -> HookExecSpec {
    let working_dir = WorkingDir::Pinned(command.parent().unwrap().to_path_buf());
    HookExecSpec {
        handler_id: handler_id.to_owned(),
        interested_events: vec![HookEventKind::PreToolUse],
        failure_mode: HookFailureMode::FailOpen,
        command,
        args: Vec::new(),
        env: BTreeMap::new(),
        working_dir,
        timeout: Duration::from_secs(5),
        resource_limits: HookExecResourceLimits::default(),
        signal_policy: HookExecSignalPolicy::default(),
        protocol_version: HookProtocolVersion::V1,
        trust: TrustLevel::AdminTrusted,
    }
}

fn write_script(name: &str, body: &str) -> PathBuf {
    static NEXT_SCRIPT_ID: AtomicU64 = AtomicU64::new(0);
    let script_id = NEXT_SCRIPT_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "jyowo-harness-hook-exec-transport-{}-{}-{}",
        name,
        std::process::id(),
        script_id
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hook.sh");
    std::fs::write(&path, body).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
    }

    path
}

#[derive(Debug)]
struct TestSessionView;

impl HookSessionView for TestSessionView {
    fn workspace_root(&self) -> Option<&Path> {
        Some(Path::new("/workspace"))
    }

    fn recent_messages(&self, limit: usize) -> Vec<HookMessageView> {
        vec![HookMessageView {
            role: MessageRole::User,
            text_snippet: "hello".to_owned(),
            tool_use_id: None,
        }]
        .into_iter()
        .take(limit)
        .collect()
    }

    fn permission_mode(&self) -> PermissionMode {
        PermissionMode::Default
    }

    fn redacted(&self) -> &dyn harness_contracts::Redactor {
        &harness_contracts::NoopRedactor
    }

    fn current_tool_descriptor(&self) -> Option<ToolDescriptorView> {
        None
    }
}

fn sample_pre_tool_use() -> HookEvent {
    HookEvent::PreToolUse {
        tool_use_id: ToolUseId::new(),
        tool_name: "bash".to_owned(),
        input: json!({ "command": "ls" }),
    }
}

fn sample_context() -> HookContext {
    HookContext {
        tenant_id: TenantId::SINGLE,
        session_id: harness_contracts::SessionId::new(),
        run_id: Some(RunId::new()),
        turn_index: Some(1),
        correlation_id: harness_contracts::CorrelationId::new(),
        causation_id: harness_contracts::CausationId::new(),
        trust_level: TrustLevel::AdminTrusted,
        permission_mode: PermissionMode::Default,
        interactivity: InteractivityLevel::FullyInteractive,
        at: chrono::Utc::now(),
        view: Arc::new(TestSessionView),
        upstream_outcome: None,
        replay_mode: ReplayMode::Live,
    }
}

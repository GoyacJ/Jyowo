#![cfg(feature = "exec")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use harness_contracts::{
    HookEventKind, HookFailureMode, InteractivityLevel, MessageRole, PermissionMode, RunId,
    TenantId, ToolUseId, TransportFailureKind, TrustLevel,
};
use harness_hook::{
    ExecHookTransport, HookContext, HookDispatcher, HookEvent, HookExecResourceLimits,
    HookExecSignalPolicy, HookExecSpec, HookFailureCause, HookMessageView, HookOutcome,
    HookPayload, HookProtocolVersion, HookRegistry, HookSessionView, HookTransport, ReplayMode,
    ToolDescriptorView, WorkingDir,
};
use serde_json::json;

#[tokio::test]
async fn exec_transport_invokes_process_and_parses_block_response() {
    let script = write_script(
        "block",
        r#"#!/bin/sh
printf '{"protocol_version":"v1","outcome":{"block":{"reason":"denied by exec"}}}'
"#,
    );
    let transport = ExecHookTransport::new(exec_spec("exec-block", script)).unwrap();

    assert_eq!(transport.handler_id(), "exec-block");
    assert_eq!(transport.interested_events(), &[HookEventKind::PreToolUse]);

    let output = transport
        .invoke(HookPayload {
            event: sample_pre_tool_use(),
            ctx: sample_context(),
        })
        .await
        .unwrap();

    assert_eq!(
        output,
        HookOutcome::Block {
            reason: "denied by exec".to_owned()
        }
    );
}

#[tokio::test]
async fn exec_nonzero_exit_is_recorded_as_transport_failure() {
    let script = write_script(
        "nonzero",
        r"#!/bin/sh
exit 7
",
    );
    let registry = HookRegistry::builder()
        .with_hook(Box::new(
            ExecHookTransport::new(exec_spec("exec-nonzero", script)).unwrap(),
        ))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(sample_pre_tool_use(), sample_context())
        .await
        .unwrap();

    assert_eq!(result.final_outcome, HookOutcome::Continue);
    assert_eq!(result.failures.len(), 1);
    assert!(
        matches!(
            result.failures[0].cause,
            HookFailureCause::Transport {
                kind: TransportFailureKind::NonZeroExit { code: 7 },
                ..
            }
        ),
        "{:?}",
        result.failures
    );
}

#[tokio::test]
async fn exec_protocol_mismatch_is_recorded_as_transport_failure() {
    let script = write_script(
        "mismatch",
        r#"#!/bin/sh
cat >/dev/null
printf '{"protocol_version":"v2","outcome":{"continue":null}}'
"#,
    );
    let registry = HookRegistry::builder()
        .with_hook(Box::new(
            ExecHookTransport::new(exec_spec("exec-mismatch", script)).unwrap(),
        ))
        .build()
        .unwrap();

    let result = HookDispatcher::new(registry.snapshot())
        .dispatch(sample_pre_tool_use(), sample_context())
        .await
        .unwrap();

    assert_eq!(result.final_outcome, HookOutcome::Continue);
    assert_eq!(result.failures.len(), 1);
    assert!(
        matches!(
            result.failures[0].cause,
            HookFailureCause::Transport {
                kind: TransportFailureKind::ProtocolVersionMismatch,
                ..
            }
        ),
        "{:?}",
        result.failures
    );
}

#[test]
fn exec_rejects_user_controlled_and_shell_metacharacter_commands() {
    let script = write_script("ok", "#!/bin/sh\nprintf '{}'\n");
    let mut user_spec = exec_spec("user", script.clone());
    user_spec.trust = TrustLevel::UserControlled;
    assert!(ExecHookTransport::new(user_spec).is_err());

    let mut bad_command = exec_spec("bad-command", PathBuf::from("/tmp/hook;rm"));
    bad_command.command = PathBuf::from("/tmp/hook;rm");
    assert!(ExecHookTransport::new(bad_command).is_err());
}

#[test]
fn exec_feature_keeps_hook_dependency_boundary() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();

    assert!(!manifest.contains("jyowo-harness-tool"));
    assert!(!manifest.contains("jyowo-harness-session"));
    assert!(!manifest.contains("jyowo-harness-journal"));
    assert!(!manifest.contains("jyowo-harness-observability"));
    assert!(!manifest.contains("jyowo-harness-engine"));
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
        timeout: Duration::from_secs(15),
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
        "jyowo-harness-hook-exec-{}-{}-{}",
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

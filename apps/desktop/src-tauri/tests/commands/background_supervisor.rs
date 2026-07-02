use super::*;

use harness_contracts::{
    AgentToolPolicy, AgentUsePolicy, AgentWorkspaceIsolationMode, BackgroundAgentState,
    ConversationTurnInput,
};
use jyowo_desktop_shell::agent_supervisor::{
    read_supervisor_lock, reconnect_to_existing_supervisor, start_agent_supervisor_with_timing,
    supervisor_lock_path, wake_agent_supervisor, AgentSupervisorError,
};
use jyowo_desktop_shell::commands::agent_supervisor_sidecar_startup_result_for_project_command;
use jyowo_harness_sdk::{
    AgentRuntimeStore, BackgroundAgentManager, BackgroundAgentStartRequest, SessionOptions,
};

fn supervisor_session_payload(options: &SessionOptions) -> serde_json::Value {
    serde_json::json!({
        "tenantId": options.tenant_id,
        "sessionId": options.session_id,
        "toolSearch": options.tool_search,
        "toolProfile": options.tool_profile,
        "modelId": options.model_id,
        "protocol": options.protocol,
        "permissionMode": options.permission_mode,
        "interactivity": options.interactivity,
        "teamId": options.team_id,
        "maxIterations": options.max_iterations,
        "contextCompressionTriggerRatio": options.context_compression_trigger_ratio,
    })
}

#[tokio::test]
async fn background_supervisor_writes_workspace_lock_without_raw_token() {
    let workspace = tempfile::tempdir().expect("tempdir");

    let handle = start_agent_supervisor_with_timing(
        workspace.path().to_path_buf(),
        Duration::from_millis(20),
        Duration::from_millis(100),
    )
    .await
    .expect("supervisor starts");

    let lock = read_supervisor_lock(workspace.path())
        .expect("read lock")
        .expect("lock exists");
    assert_eq!(lock.status, "running");
    assert_eq!(lock.token_hash, handle.token_hash());
    assert!(handle.control_addr().ip().is_loopback());
    let raw_lock = std::fs::read_to_string(handle.lock_path()).expect("lock contents");
    assert!(!raw_lock.contains("\"token\":"));

    handle.shutdown().await;
}

#[test]
fn project_switch_treats_agent_supervisor_startup_failure_as_capability_unavailable() {
    let result = agent_supervisor_sidecar_startup_result_for_project_command(Err(
        AgentSupervisorError::Sidecar("missing sidecar binary".to_owned()),
    ));

    assert!(
        result.is_ok(),
        "project switching must not fail when only background supervisor startup fails"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn background_supervisor_token_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let workspace = tempfile::tempdir().expect("tempdir");

    let handle = start_agent_supervisor_with_timing(
        workspace.path().to_path_buf(),
        Duration::from_millis(20),
        Duration::from_millis(100),
    )
    .await
    .expect("supervisor starts");

    let token_path = workspace
        .path()
        .join(".jyowo")
        .join("runtime")
        .join("agent-supervisor.token");
    let mode = std::fs::metadata(token_path)
        .expect("token metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);

    handle.shutdown().await;
}

#[tokio::test]
async fn background_supervisor_rejects_live_lock_for_different_workspace_id() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let handle = start_agent_supervisor_with_timing(
        workspace.path().to_path_buf(),
        Duration::from_millis(20),
        Duration::from_millis(100),
    )
    .await
    .expect("supervisor starts");
    let runtime_dir = workspace.path().join(".jyowo").join("runtime");
    let mut token: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(runtime_dir.join("agent-supervisor.token"))
            .expect("token contents"),
    )
    .expect("token json");
    let mut lock: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(supervisor_lock_path(workspace.path())).expect("lock contents"),
    )
    .expect("lock json");
    token["workspaceId"] = serde_json::Value::String("forged-workspace".to_owned());
    lock["workspaceId"] = serde_json::Value::String("forged-workspace".to_owned());
    std::fs::write(
        runtime_dir.join("agent-supervisor.token"),
        serde_json::to_string(&token).expect("token serialize"),
    )
    .expect("token write");
    std::fs::write(
        supervisor_lock_path(workspace.path()),
        serde_json::to_string(&lock).expect("lock serialize"),
    )
    .expect("lock write");

    let reconnected = reconnect_to_existing_supervisor(workspace.path(), Duration::from_secs(10))
        .await
        .expect("reconnect should finish");

    assert!(!reconnected);

    handle.shutdown().await;
}

#[tokio::test]
async fn background_supervisor_reconnects_to_existing_live_supervisor() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(AgentRuntimeStore::open(workspace.path()).expect("store opens"));
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let manager = BackgroundAgentManager::new(
        Arc::clone(&store),
        event_store,
        TenantId::SINGLE,
        conversation_id,
        Arc::new(DefaultRedactor::default()),
    );
    let record = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "live supervisor reconnect".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start background agent");
    let handle = start_agent_supervisor_with_timing(
        workspace.path().to_path_buf(),
        Duration::from_millis(20),
        Duration::from_millis(100),
    )
    .await
    .expect("supervisor starts");

    let reconnected = reconnect_to_existing_supervisor(workspace.path(), Duration::from_secs(10))
        .await
        .expect("reconnect succeeds");

    assert!(reconnected);
    let stored = store
        .get_background_agent(record.background_agent_id.as_str())
        .expect("background lookup")
        .expect("background exists");
    assert_eq!(stored.state, BackgroundAgentState::Running);

    handle.shutdown().await;
}

#[tokio::test]
async fn background_supervisor_executes_queued_background_record() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(AgentRuntimeStore::open(workspace.path()).expect("store opens"));
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let manager = BackgroundAgentManager::new(
        Arc::clone(&store),
        event_store,
        TenantId::SINGLE,
        conversation_id,
        Arc::new(DefaultRedactor::default()),
    );
    let session = SessionOptions::new(workspace.path())
        .with_tenant_id(TenantId::SINGLE)
        .with_session_id(conversation_id)
        .with_interactivity(InteractivityLevel::FullyInteractive)
        .with_model_id("llama3.1")
        .with_protocol(ModelProtocol::ChatCompletions);
    let record = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "supervisor owned execution".to_owned(),
            payload_json: serde_json::json!({
                "conversationId": conversation_id.to_string(),
                "source": "background_agent_tool",
                "supervisorExecution": {
                    "status": "queued",
                    "session": supervisor_session_payload(&session),
                    "input": ConversationTurnInput::ask(""),
                    "modelConfigId": "test-model-config",
                    "permissionMode": PermissionMode::Default,
                    "agentToolPolicy": AgentToolPolicy {
                        subagents: AgentUsePolicy::Off,
                        agent_team: AgentUsePolicy::Off,
                        team_config: None,
                        background_agents: AgentUsePolicy::Allowed,
                        workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
                        max_depth: 2,
                        max_concurrent_subagents: 2,
                        max_team_members: 4,
                    },
                },
            })
            .to_string(),
        })
        .await
        .expect("start background agent");
    let handle = start_agent_supervisor_with_timing(
        workspace.path().to_path_buf(),
        Duration::from_millis(10),
        Duration::from_secs(1),
    )
    .await
    .expect("supervisor starts");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let stored = store
            .get_background_agent(&record.background_agent_id)
            .expect("background lookup")
            .expect("background exists");
        if stored.state == BackgroundAgentState::Failed {
            assert_ne!(stored.payload_json, record.payload_json);
            handle.shutdown().await;
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            handle.shutdown().await;
            panic!("supervisor did not execute queued background record");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn background_supervisor_wake_executes_queued_background_record_without_waiting_for_heartbeat(
) {
    let workspace = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(AgentRuntimeStore::open(workspace.path()).expect("store opens"));
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let manager = BackgroundAgentManager::new(
        Arc::clone(&store),
        event_store,
        TenantId::SINGLE,
        conversation_id,
        Arc::new(DefaultRedactor::default()),
    );
    let handle = start_agent_supervisor_with_timing(
        workspace.path().to_path_buf(),
        Duration::from_secs(3600),
        Duration::from_secs(1),
    )
    .await
    .expect("supervisor starts");
    tokio::time::sleep(Duration::from_millis(50)).await;

    let session = SessionOptions::new(workspace.path())
        .with_tenant_id(TenantId::SINGLE)
        .with_session_id(conversation_id)
        .with_interactivity(InteractivityLevel::FullyInteractive)
        .with_model_id("llama3.1")
        .with_protocol(ModelProtocol::ChatCompletions);
    let record = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "supervisor wake execution".to_owned(),
            payload_json: serde_json::json!({
                "conversationId": conversation_id.to_string(),
                "source": "background_agent_tool",
                "supervisorExecution": {
                    "status": "queued",
                    "session": supervisor_session_payload(&session),
                    "input": ConversationTurnInput::ask(""),
                    "modelConfigId": "test-model-config",
                    "permissionMode": PermissionMode::Default,
                    "agentToolPolicy": AgentToolPolicy {
                        subagents: AgentUsePolicy::Off,
                        agent_team: AgentUsePolicy::Off,
                        team_config: None,
                        background_agents: AgentUsePolicy::Allowed,
                        workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
                        max_depth: 2,
                        max_concurrent_subagents: 2,
                        max_team_members: 4,
                    },
                },
            })
            .to_string(),
        })
        .await
        .expect("start background agent");

    assert!(wake_agent_supervisor(workspace.path())
        .await
        .expect("wake request succeeds"));

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let stored = store
            .get_background_agent(&record.background_agent_id)
            .expect("background lookup")
            .expect("background exists");
        let payload: serde_json::Value =
            serde_json::from_str(&stored.payload_json).expect("payload json");
        let status = payload
            .get("supervisorExecution")
            .and_then(|execution| execution.get("status"))
            .and_then(serde_json::Value::as_str);
        if status != Some("queued") {
            assert_ne!(stored.payload_json, record.payload_json);
            assert!(!stored
                .payload_json
                .contains(&workspace.path().to_string_lossy().to_string()));
            assert!(!stored
                .payload_json
                .contains("sk-legacy-model-extra1234567890"));
            assert!(!stored
                .payload_json
                .contains("sk-legacy-system-addendum1234567890"));
            assert!(payload["supervisorExecution"]["sessionOptions"].is_null());
            assert_eq!(
                payload["supervisorExecution"]["session"]["sessionId"],
                conversation_id.to_string()
            );
            handle.shutdown().await;
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            handle.shutdown().await;
            panic!("wake did not execute queued background record");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn background_supervisor_invalid_queued_payload_fails_record() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(AgentRuntimeStore::open(workspace.path()).expect("store opens"));
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let manager = BackgroundAgentManager::new(
        Arc::clone(&store),
        event_store,
        TenantId::SINGLE,
        conversation_id,
        Arc::new(DefaultRedactor::default()),
    );
    let record = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "invalid supervisor payload".to_owned(),
            payload_json: serde_json::json!({
                "conversationId": conversation_id.to_string(),
                "source": "background_agent_tool",
                "supervisorExecution": {
                    "status": "queued",
                    "session": "invalid",
                    "input": ConversationTurnInput::ask(""),
                    "modelConfigId": "test-model-config",
                    "permissionMode": PermissionMode::Default,
                    "agentToolPolicy": AgentToolPolicy {
                        subagents: AgentUsePolicy::Off,
                        agent_team: AgentUsePolicy::Off,
                        team_config: None,
                        background_agents: AgentUsePolicy::Allowed,
                        workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
                        max_depth: 2,
                        max_concurrent_subagents: 2,
                        max_team_members: 4,
                    },
                },
            })
            .to_string(),
        })
        .await
        .expect("start background agent");
    let handle = start_agent_supervisor_with_timing(
        workspace.path().to_path_buf(),
        Duration::from_millis(10),
        Duration::from_secs(1),
    )
    .await
    .expect("supervisor starts");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let stored = store
            .get_background_agent(&record.background_agent_id)
            .expect("background lookup")
            .expect("background exists");
        if stored.state == BackgroundAgentState::Failed {
            assert!(!stored.payload_json.contains("invalid supervisor payload"));
            handle.shutdown().await;
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            handle.shutdown().await;
            panic!("invalid queued supervisor payload did not fail the background record");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn background_supervisor_rejects_legacy_start_run_source() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(AgentRuntimeStore::open(workspace.path()).expect("store opens"));
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let manager = BackgroundAgentManager::new(
        Arc::clone(&store),
        event_store,
        TenantId::SINGLE,
        conversation_id,
        Arc::new(DefaultRedactor::default()),
    );
    let session = SessionOptions::new(workspace.path())
        .with_tenant_id(TenantId::SINGLE)
        .with_session_id(conversation_id)
        .with_interactivity(InteractivityLevel::FullyInteractive);
    let record = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "legacy start run payload".to_owned(),
            payload_json: serde_json::json!({
                "conversationId": conversation_id.to_string(),
                "source": "start_run",
                "supervisorExecution": {
                    "status": "queued",
                    "session": supervisor_session_payload(&session),
                    "input": ConversationTurnInput::ask(""),
                    "modelConfigId": "test-model-config",
                    "permissionMode": PermissionMode::Default,
                    "agentToolPolicy": AgentToolPolicy {
                        subagents: AgentUsePolicy::Off,
                        agent_team: AgentUsePolicy::Off,
                        team_config: None,
                        background_agents: AgentUsePolicy::Allowed,
                        workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
                        max_depth: 2,
                        max_concurrent_subagents: 2,
                        max_team_members: 4,
                    },
                },
            })
            .to_string(),
        })
        .await
        .expect("start background agent");
    let handle = start_agent_supervisor_with_timing(
        workspace.path().to_path_buf(),
        Duration::from_millis(10),
        Duration::from_secs(1),
    )
    .await
    .expect("supervisor starts");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let stored = store
            .get_background_agent(&record.background_agent_id)
            .expect("background lookup")
            .expect("background exists");
        if stored.state == BackgroundAgentState::Failed {
            handle.shutdown().await;
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            handle.shutdown().await;
            panic!("legacy start_run payload did not fail the background record");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn background_supervisor_startup_marks_agents_interrupted_after_stale_heartbeat() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(AgentRuntimeStore::open(workspace.path()).expect("store opens"));
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let manager = BackgroundAgentManager::new(
        Arc::clone(&store),
        event_store,
        TenantId::SINGLE,
        conversation_id,
        Arc::new(DefaultRedactor::default()),
    );
    let session = SessionOptions::new(workspace.path())
        .with_tenant_id(TenantId::SINGLE)
        .with_session_id(conversation_id)
        .with_interactivity(InteractivityLevel::FullyInteractive);
    let record = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "stale supervisor recovery".to_owned(),
            payload_json: serde_json::json!({
                "conversationId": conversation_id.to_string(),
                "source": "background_agent_tool",
                "supervisorExecution": {
                    "status": "running",
                    "session": supervisor_session_payload(&session),
                    "input": ConversationTurnInput::ask(""),
                    "modelConfigId": "test-model-config",
                    "permissionMode": PermissionMode::Default,
                    "agentToolPolicy": AgentToolPolicy {
                        subagents: AgentUsePolicy::Off,
                        agent_team: AgentUsePolicy::Off,
                        team_config: None,
                        background_agents: AgentUsePolicy::Allowed,
                        workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
                        max_depth: 2,
                        max_concurrent_subagents: 2,
                        max_team_members: 4,
                    },
                },
            })
            .to_string(),
        })
        .await
        .expect("start background agent");
    std::fs::create_dir_all(workspace.path().join(".jyowo/runtime")).expect("runtime dir");
    std::fs::write(
        supervisor_lock_path(workspace.path()),
        serde_json::json!({
            "status": "running",
            "workspaceId": "stale-workspace",
            "tokenHash": "stale-token-hash",
            "tokenEpoch": 1,
            "pid": 1,
            "controlAddr": "127.0.0.1:9",
            "startedAt": chrono::Utc::now() - chrono::Duration::seconds(60),
            "heartbeatAt": chrono::Utc::now() - chrono::Duration::seconds(60),
        })
        .to_string(),
    )
    .expect("stale lock");

    let handle = start_agent_supervisor_with_timing(
        workspace.path().to_path_buf(),
        Duration::from_millis(20),
        Duration::from_millis(10),
    )
    .await
    .expect("supervisor starts after stale heartbeat");

    let stored = store
        .get_background_agent(record.background_agent_id.as_str())
        .expect("background lookup")
        .expect("background exists");
    assert_eq!(
        stored.state,
        harness_contracts::BackgroundAgentState::Interrupted
    );
    assert_ne!(stored.payload_json, record.payload_json);
    assert!(!stored
        .payload_json
        .contains(&workspace.path().to_string_lossy().to_string()));
    assert!(!stored
        .payload_json
        .contains("sk-stale-model-extra1234567890"));
    assert!(!stored
        .payload_json
        .contains("sk-stale-system-addendum1234567890"));
    let payload: serde_json::Value =
        serde_json::from_str(&stored.payload_json).expect("payload json");
    assert!(payload["supervisorExecution"]["sessionOptions"].is_null());
    assert_eq!(payload["supervisorExecution"]["status"], "interrupted");

    handle.shutdown().await;
}

#[tokio::test]
async fn background_supervisor_marks_agents_interrupted_when_fresh_lock_cannot_reconnect() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(AgentRuntimeStore::open(workspace.path()).expect("store opens"));
    let event_store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let conversation_id = SessionId::new();
    let manager = BackgroundAgentManager::new(
        Arc::clone(&store),
        event_store,
        TenantId::SINGLE,
        conversation_id,
        Arc::new(DefaultRedactor::default()),
    );
    let record = manager
        .start(BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "supervisor crash recovery".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start background agent");
    let handle = start_agent_supervisor_with_timing(
        workspace.path().to_path_buf(),
        Duration::from_millis(20),
        Duration::from_millis(100),
    )
    .await
    .expect("supervisor starts");
    let mut lock = read_supervisor_lock(workspace.path())
        .expect("read lock")
        .expect("lock exists");
    handle.shutdown().await;

    lock.status = "running".to_owned();
    lock.control_addr = "127.0.0.1:9".to_owned();
    lock.heartbeat_at = chrono::Utc::now();
    std::fs::write(
        supervisor_lock_path(workspace.path()),
        serde_json::to_string(&lock).expect("serialize lock"),
    )
    .expect("fresh unreachable lock");

    let reconnected = reconnect_to_existing_supervisor(workspace.path(), Duration::from_secs(10))
        .await
        .expect("reconnect attempt completes");

    assert!(!reconnected);
    let stored = store
        .get_background_agent(record.background_agent_id.as_str())
        .expect("background lookup")
        .expect("background exists");
    assert_eq!(stored.state, BackgroundAgentState::Interrupted);
}

#![allow(unused_imports)]

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

fn write_test_supervisor_lock(workspace: &Path) -> TestSupervisorControl {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("control bind");
    listener.set_nonblocking(true).expect("control nonblocking");
    let control_addr = listener.local_addr().expect("control addr");
    let runtime_dir = workspace.join(".jyowo/runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let token = "test-background-supervisor-token";
    let token_hash = blake3::hash(token.as_bytes()).to_hex().to_string();
    let workspace_id = blake3::hash(workspace.display().to_string().as_bytes())
        .to_hex()
        .to_string();
    std::fs::write(
        runtime_dir.join("agent-supervisor.token"),
        serde_json::json!({
            "token": token,
            "tokenHash": token_hash,
            "tokenEpoch": 1,
            "workspaceId": workspace_id,
            "createdAt": chrono::Utc::now(),
        })
        .to_string(),
    )
    .unwrap();
    write_test_supervisor_json_atomic(
        &runtime_dir.join("agent-supervisor.lock"),
        &serde_json::json!({
            "status": "running",
            "workspaceId": workspace_id,
            "tokenHash": token_hash,
            "tokenEpoch": 1,
            "pid": std::process::id(),
            "controlAddr": control_addr.to_string(),
            "startedAt": chrono::Utc::now(),
            "heartbeatAt": chrono::Utc::now(),
        }),
    )
    .unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let thread = std::thread::spawn(move || {
        while !thread_stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, peer)) => {
                    let mut buffer = [0_u8; 8192];
                    let Ok(read) = stream.read(&mut buffer) else {
                        continue;
                    };
                    let request = serde_json::from_slice::<serde_json::Value>(&buffer[..read])
                        .unwrap_or_else(|_| serde_json::json!({}));
                    let ok = peer.ip().is_loopback()
                        && request.get("token").and_then(Value::as_str) == Some(token)
                        && request.get("request").and_then(Value::as_str).is_some();
                    let response = serde_json::json!({
                        "ok": ok,
                        "status": if ok { "running" } else { "unauthorized" },
                    });
                    let _ = stream.write_all(response.to_string().as_bytes());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });
    TestSupervisorControl {
        stop,
        thread: Some(thread),
    }
}

struct TestSupervisorControl {
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TestSupervisorControl {
    async fn shutdown(mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for TestSupervisorControl {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn write_test_supervisor_json_atomic(path: &Path, value: &Value) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("agent-supervisor-file");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let tmp_path = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), nonce));
    std::fs::write(&tmp_path, value.to_string())?;
    std::fs::rename(&tmp_path, path)
}

async fn enable_subagents_in_execution_settings(state: &DesktopRuntimeState) {
    set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: true,
            agent_teams_enabled: true,
            background_agents_enabled: false,
        },
        &DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf()),
        Some(&AgentCapabilityResolutionContext {
            stream_permission_runtime_available: true,
        }),
    )
    .expect("execution settings should save");
}

#[tokio::test]
async fn start_run_agent_policy_uses_disabled_settings_without_per_run_enable() {
    let state = runtime_state_with_harness().await;
    set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: false,
            agent_teams_enabled: false,
            background_agents_enabled: false,
        },
        &DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf()),
        Some(&AgentCapabilityResolutionContext {
            stream_permission_runtime_available: true,
        }),
    )
    .expect("execution settings should save");

    let policy = resolve_start_run_agent_policy(
        &StartRunRequest {
            attachments: None,
            client_message_id: None,
            context_references: None,
            conversation_id: SessionId::new().to_string(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Run with subagents".to_owned(),
        },
        &state,
    )
    .expect("disabled settings should resolve to foreground run policy");

    assert_eq!(policy.options.subagents, AgentUsePolicy::Off);
    assert_eq!(policy.options.agent_team, AgentUsePolicy::Off);
    assert_eq!(policy.options.background_agents, AgentUsePolicy::Off);
    assert!(policy.options.team_config.is_none());
}

#[tokio::test]
async fn start_run_agent_policy_enables_agents_from_settings_without_team_config() {
    let state = runtime_state_with_harness().await;
    enable_subagents_in_execution_settings(&state).await;

    let policy = resolve_start_run_agent_policy(
        &StartRunRequest {
            attachments: None,
            client_message_id: None,
            context_references: None,
            conversation_id: SessionId::new().to_string(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Run with configured agents".to_owned(),
        },
        &state,
    )
    .expect("enabled settings should resolve agent policy");

    assert_eq!(policy.options.subagents, AgentUsePolicy::Allowed);
    assert_eq!(policy.options.agent_team, AgentUsePolicy::Allowed);
    assert!(policy.options.team_config.is_none());
}

#[tokio::test]
async fn agent_team_runtime_tool_persists_team_task_and_mailbox() {
    let workspace = unique_workspace("agent-team-runtime-tool");
    std::fs::create_dir_all(&workspace).unwrap();
    jyowo_harness_sdk::list_agent_profiles(&workspace).expect("agent profiles should list");
    let agent_team_tool_use_id = ToolUseId::new();
    let state = runtime_state_with_scripted_model_for_workspace(
        workspace.clone(),
        vec![
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::ToolUseComplete {
                        id: agent_team_tool_use_id,
                        name: "agent_team".to_owned(),
                        input: json!({
                            "goal": "Review the run",
                            "maxTurnsPerGoal": 4
                        }),
                    },
                },
                ModelStreamEvent::MessageStop,
            ]),
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("team lead accepted".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]),
        ],
    )
    .await;
    enable_subagents_in_execution_settings(&state).await;
    let session_id = SessionId::new();

    let started = start_run_with_runtime_state(
        StartRunRequest {
            attachments: None,
            client_message_id: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: Some(PermissionMode::BypassPermissions),
            prompt: "Run with agent team".to_owned(),
        },
        &state,
    )
    .await
    .expect("agent_team tool run should succeed");

    let connection =
        rusqlite::Connection::open(workspace.join(".jyowo/runtime/agent-runtime.sqlite"))
            .expect("runtime sqlite opens");
    let mut task_statement = connection
        .prepare(
            "SELECT task_id, team_id, status, assignee_profile_id
             FROM agent_team_tasks
             WHERE run_id = ?1",
        )
        .expect("task query prepares");
    let mut task_rows = task_statement
        .query([started.run_id.clone()])
        .expect("task query runs");
    let mut tasks = Vec::new();
    while let Some(row) = task_rows.next().expect("task row loads") {
        tasks.push((
            row.get::<_, String>(0).expect("task_id"),
            row.get::<_, String>(1).expect("team_id"),
            row.get::<_, String>(2).expect("status"),
            row.get::<_, Option<String>>(3)
                .expect("assignee_profile_id"),
        ));
    }

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].2, "active");
    assert_eq!(tasks[0].3.as_deref(), Some("reviewer"));

    let mailbox_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM agent_team_mailbox WHERE team_id = ?1 AND summary = ?2",
            rusqlite::params![tasks[0].1, "Team run queued"],
            |row| row.get(0),
        )
        .expect("mailbox count loads");
    assert_eq!(mailbox_count, 1);
}

#[tokio::test]
async fn start_run_stays_foreground_when_background_agents_enabled() {
    let state = runtime_state_with_harness().await;
    let supervisor = write_test_supervisor_lock(state.workspace_root());
    set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: true,
            agent_teams_enabled: false,
            background_agents_enabled: true,
        },
        &DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf()),
        Some(&AgentCapabilityResolutionContext {
            stream_permission_runtime_available: true,
        }),
    )
    .expect("background settings should save");

    let session_id = SessionId::new();

    let started = start_run_with_runtime_state(
        StartRunRequest {
            attachments: None,
            client_message_id: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Run in background".to_owned(),
        },
        &state,
    )
    .await
    .expect("foreground start should succeed");

    assert!(!started.run_id.is_empty());

    let listed = list_background_agents_with_runtime_state(
        ListBackgroundAgentsRequest {
            conversation_id: Some(session_id.to_string()),
            include_archived: false,
        },
        &state,
    )
    .await
    .expect("background list succeeds");
    assert!(listed.agents.is_empty());

    supervisor.shutdown().await;
}

#[tokio::test]
async fn background_agent_tool_creates_durable_record() {
    let workspace = unique_workspace("background-agent-tool");
    std::fs::create_dir_all(&workspace).unwrap();
    jyowo_harness_sdk::list_agent_profiles(&workspace).expect("agent profiles should list");
    let background_tool_use_id = ToolUseId::new();
    let state = runtime_state_with_scripted_model_for_workspace(
        workspace.clone(),
        vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::ToolUseComplete {
                    id: background_tool_use_id,
                    name: "background_agent".to_owned(),
                    input: json!({
                        "goal": "Continue this investigation later",
                        "title": "Background investigation"
                    }),
                },
            },
            ModelStreamEvent::MessageStop,
        ])],
    )
    .await;
    let supervisor = write_test_supervisor_lock(state.workspace_root());
    set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Full,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: true,
            agent_teams_enabled: true,
            background_agents_enabled: true,
        },
        &DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf()),
        Some(&AgentCapabilityResolutionContext {
            stream_permission_runtime_available: true,
        }),
    )
    .expect("background settings should save");
    let session_id = SessionId::new();

    let started = start_run_with_runtime_state(
        StartRunRequest {
            attachments: None,
            client_message_id: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: Some(PermissionMode::BypassPermissions),
            prompt: "Decide whether to continue in background".to_owned(),
        },
        &state,
    )
    .await
    .expect("background_agent tool run should succeed");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let harness = state.harness().expect("harness should be available");
    loop {
        let events: Vec<_> = harness
            .event_store()
            .read_envelopes(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("session envelopes should be readable")
            .collect()
            .await;
        if events.iter().any(|envelope| {
            matches!(
                &envelope.payload,
                Event::ToolUseCompleted(completed)
                    if completed.tool_use_id == background_tool_use_id
            )
        }) {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            let event_types: Vec<_> = events
                .iter()
                .map(|envelope| format!("{:?}", envelope.payload))
                .collect();
            panic!("expected background_agent completion event, got: {event_types:?}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let listed = list_background_agents_with_runtime_state(
        ListBackgroundAgentsRequest {
            conversation_id: Some(session_id.to_string()),
            include_archived: false,
        },
        &state,
    )
    .await
    .expect("background list succeeds");
    assert_eq!(listed.agents.len(), 1);
    assert_eq!(listed.agents[0].title, "Background investigation");

    let store = jyowo_harness_sdk::AgentRuntimeStore::open(state.workspace_root())
        .expect("agent runtime store opens");
    let record = store
        .get_background_agent(&listed.agents[0].background_agent_id)
        .expect("background lookup succeeds")
        .expect("background record exists");
    let payload = serde_json::from_str::<Value>(&record.payload_json).expect("payload is json");
    assert_eq!(payload["source"], "background_agent_tool");
    assert_eq!(payload["parentRunId"], started.run_id);
    assert_eq!(payload["toolUseId"], background_tool_use_id.to_string());
    assert_eq!(
        payload["supervisorExecution"]["input"]["prompt"],
        "Continue this investigation later"
    );

    supervisor.shutdown().await;
}

#[tokio::test]
async fn workspace_isolation_defaults_to_read_only_from_settings_policy() {
    let state = runtime_state_with_harness().await;
    enable_subagents_in_execution_settings(&state).await;

    let policy = resolve_start_run_agent_policy(
        &StartRunRequest {
            attachments: None,
            client_message_id: None,
            context_references: None,
            conversation_id: SessionId::new().to_string(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: None,
            prompt: "Run with settings isolation".to_owned(),
        },
        &state,
    )
    .expect("settings policy should validate");

    assert_eq!(
        policy.options.workspace_isolation,
        AgentWorkspaceIsolationMode::ReadOnly
    );
}

#[tokio::test]
async fn subagent_runtime_start_run_invokes_agent_tool_in_scripted_flow() {
    let workspace = unique_workspace("subagent-runtime");
    std::fs::create_dir_all(&workspace).unwrap();
    jyowo_harness_sdk::list_agent_profiles(&workspace).expect("agent profiles should list");

    let agent_tool_use_id = ToolUseId::new();
    let state = runtime_state_with_scripted_model_for_workspace(
        workspace.clone(),
        vec![
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::ToolUseComplete {
                        id: agent_tool_use_id,
                        name: "agent".to_owned(),
                        input: json!({
                            "role": "worker",
                            "task": "inspect repository"
                        }),
                    },
                },
                ModelStreamEvent::MessageStop,
            ]),
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("child done".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]),
            ScriptedResponse::Stream(vec![
                ModelStreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: ContentDelta::Text("parent done".to_owned()),
                },
                ModelStreamEvent::MessageStop,
            ]),
        ],
    )
    .await;
    enable_subagents_in_execution_settings(&state).await;
    let session_id = SessionId::new();

    let _started = start_run_with_runtime_state(
        StartRunRequest {
            attachments: None,
            client_message_id: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: Some(PermissionMode::BypassPermissions),
            prompt: "Delegate inspection".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should accept subagent run options");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let harness = state.harness().expect("harness should be available");
    loop {
        let events: Vec<_> = harness
            .event_store()
            .read_envelopes(TenantId::SINGLE, session_id, ReplayCursor::FromStart)
            .await
            .expect("session envelopes should be readable")
            .collect()
            .await;
        if events.iter().any(|envelope| match &envelope.payload {
            Event::SubagentSpawned(_) | Event::SubagentAnnounced(_) => true,
            Event::ToolUseRequested(requested) => requested.tool_name == "agent",
            _ => false,
        }) {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            let event_types: Vec<_> = events
                .iter()
                .map(|envelope| format!("{:?}", envelope.payload))
                .collect();
            panic!("expected subagent lifecycle events, got: {event_types:?}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

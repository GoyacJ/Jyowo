use super::*;

#[tokio::test]
async fn background_agent_commands_cover_lifecycle_operations() {
    let state = runtime_state_with_harness().await;
    let supervisor = enable_background_agents(&state).await;
    let conversation_id = SessionId::new();
    let record = create_background_agent_record(&state, conversation_id, "command lifecycle").await;

    let listed = list_background_agents_with_runtime_state(
        ListBackgroundAgentsRequest {
            conversation_id: Some(conversation_id.to_string()),
            include_archived: false,
        },
        &state,
    )
    .await
    .expect("list command succeeds");
    assert_eq!(listed.agents.len(), 1);
    assert_eq!(
        listed.agents[0].background_agent_id,
        record.background_agent_id
    );
    assert_eq!(
        listed.agents[0].conversation_id,
        conversation_id.to_string()
    );
    assert_eq!(
        listed.agents[0].state,
        harness_contracts::BackgroundAgentState::Running
    );

    let fetched = get_background_agent_with_runtime_state(
        GetBackgroundAgentRequest {
            background_agent_id: record.background_agent_id.clone(),
            conversation_id: Some(conversation_id.to_string()),
        },
        &state,
    )
    .await
    .expect("get command succeeds");
    assert_eq!(fetched.agent.title, "command lifecycle");

    let paused = pause_background_agent_with_runtime_state(
        BackgroundAgentIdRequest {
            background_agent_id: record.background_agent_id.clone(),
            conversation_id: Some(conversation_id.to_string()),
        },
        &state,
    )
    .await
    .expect("pause command succeeds");
    assert_eq!(
        paused.agent.state,
        harness_contracts::BackgroundAgentState::Paused
    );

    let resumed = resume_background_agent_with_runtime_state(
        BackgroundAgentIdRequest {
            background_agent_id: record.background_agent_id.clone(),
            conversation_id: Some(conversation_id.to_string()),
        },
        &state,
    )
    .await
    .expect("resume command succeeds");
    assert_eq!(
        resumed.agent.state,
        harness_contracts::BackgroundAgentState::Running
    );

    let request_id = RequestId::new();
    background_manager(&state, conversation_id)
        .request_input(&record.background_agent_id, request_id, "Need input")
        .await
        .expect("manager asks input");
    let waiting = get_background_agent_with_runtime_state(
        GetBackgroundAgentRequest {
            background_agent_id: record.background_agent_id.clone(),
            conversation_id: Some(conversation_id.to_string()),
        },
        &state,
    )
    .await
    .expect("waiting input command succeeds");
    assert_eq!(
        waiting.agent.pending_input_request_id,
        Some(request_id.to_string())
    );
    let input_submitted = send_background_agent_input_with_runtime_state(
        SendBackgroundAgentInputRequest {
            background_agent_id: record.background_agent_id.clone(),
            conversation_id: Some(conversation_id.to_string()),
            input: "Continue safely".to_owned(),
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .expect("send input command succeeds");
    assert_eq!(
        input_submitted.agent.state,
        harness_contracts::BackgroundAgentState::Running
    );

    let cancelled = cancel_background_agent_with_runtime_state(
        BackgroundAgentIdRequest {
            background_agent_id: record.background_agent_id.clone(),
            conversation_id: Some(conversation_id.to_string()),
        },
        &state,
    )
    .await
    .expect("cancel command succeeds");
    assert_eq!(
        cancelled.agent.state,
        harness_contracts::BackgroundAgentState::Cancelled
    );

    let archived = archive_background_agent_with_runtime_state(
        BackgroundAgentIdRequest {
            background_agent_id: record.background_agent_id.clone(),
            conversation_id: Some(conversation_id.to_string()),
        },
        &state,
    )
    .await
    .expect("archive command succeeds");
    assert_eq!(
        archived.agent.state,
        harness_contracts::BackgroundAgentState::Archived
    );

    delete_background_agent_with_runtime_state(
        BackgroundAgentIdRequest {
            background_agent_id: record.background_agent_id.clone(),
            conversation_id: Some(conversation_id.to_string()),
        },
        &state,
    )
    .await
    .expect("delete archived command succeeds");

    let deleted = get_background_agent_with_runtime_state(
        GetBackgroundAgentRequest {
            background_agent_id: record.background_agent_id,
            conversation_id: Some(conversation_id.to_string()),
        },
        &state,
    )
    .await
    .expect_err("deleted background agent is not found");
    assert_eq!(deleted.code, "NOT_FOUND");

    supervisor.shutdown().await;
}

#[tokio::test]
async fn background_agent_commands_reject_invalid_state_and_scope() {
    let state = runtime_state_with_harness().await;
    let supervisor = enable_background_agents(&state).await;
    let conversation_id = SessionId::new();
    let other_conversation_id = SessionId::new();
    let record = create_background_agent_record(&state, conversation_id, "invalid states").await;

    let scope_error = get_background_agent_with_runtime_state(
        GetBackgroundAgentRequest {
            background_agent_id: record.background_agent_id.clone(),
            conversation_id: Some(other_conversation_id.to_string()),
        },
        &state,
    )
    .await
    .expect_err("conversation mismatch is rejected");
    assert_eq!(scope_error.code, "INVALID_PAYLOAD");

    let archive_error = archive_background_agent_with_runtime_state(
        BackgroundAgentIdRequest {
            background_agent_id: record.background_agent_id.clone(),
            conversation_id: Some(conversation_id.to_string()),
        },
        &state,
    )
    .await
    .expect_err("running agent cannot be archived");
    assert_eq!(archive_error.code, "INVALID_PAYLOAD");

    let delete_error = delete_background_agent_with_runtime_state(
        BackgroundAgentIdRequest {
            background_agent_id: record.background_agent_id.clone(),
            conversation_id: Some(conversation_id.to_string()),
        },
        &state,
    )
    .await
    .expect_err("non-archived agent cannot be deleted");
    assert_eq!(delete_error.code, "INVALID_PAYLOAD");

    let input_error = send_background_agent_input_with_runtime_state(
        SendBackgroundAgentInputRequest {
            background_agent_id: record.background_agent_id,
            conversation_id: Some(conversation_id.to_string()),
            input: "unexpected".to_owned(),
            request_id: RequestId::new().to_string(),
        },
        &state,
    )
    .await
    .expect_err("input requires waiting state");
    assert_eq!(input_error.code, "INVALID_PAYLOAD");

    supervisor.shutdown().await;
}

#[test]
fn background_agent_commands_do_not_add_public_start_command() {
    let commands_mod = include_str!("../../src/commands/mod.rs");
    let lib = include_str!("../../src/lib.rs");

    assert!(!commands_mod.contains("pub async fn start_background_agent("));
    assert!(!lib.contains("commands::start_background_agent"));
}

#[tokio::test]
async fn background_agent_tool_persists_record_without_copying_parent_context() {
    let workspace = unique_workspace("background-agent-tool-redaction");
    std::fs::create_dir_all(&workspace).unwrap();
    jyowo_harness_sdk::list_agent_profiles(&workspace).expect("agent profiles initialize");
    let background_tool_use_id = ToolUseId::new();
    let state = runtime_state_with_scripted_model_for_workspace(
        workspace,
        vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::ToolUseComplete {
                    id: background_tool_use_id,
                    name: "background_agent".to_owned(),
                    input: json!({
                        "goal": "Continue\nsk-12345678901234567890",
                        "title": "Background investigation"
                    }),
                },
            },
            ModelStreamEvent::MessageStop,
        ])],
    )
    .await;
    let supervisor = enable_background_agents(&state).await;
    let settings_store = DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf());
    let settings_context = AgentCapabilityResolutionContext {
        stream_permission_runtime_available: true,
    };
    set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::Default,
            tool_profile: ToolProfile::Coding,
            context_compression_trigger_ratio: 0.8,
            subagents_enabled: true,
            agent_teams_enabled: false,
            background_agents_enabled: true,
        },
        &settings_store,
        Some(&settings_context),
    )
    .expect("execution settings update");
    std::fs::write(state.workspace_root().join("notes.md"), "safe notes").unwrap();
    std::fs::write(state.workspace_root().join("attachment.txt"), "attachment").unwrap();
    let mut attachment = create_attachment_from_path_with_runtime_state(
        CreateAttachmentFromPathRequest {
            conversation_id: None,
            path: "attachment.txt".to_owned(),
        },
        &state,
    )
    .await
    .expect("attachment is created")
    .attachment;
    attachment.name = "sk-attachmentname1234567890.txt".to_owned();
    attachment.mime_type = "application/sk-mimetype1234567890".to_owned();
    attachment.blob_ref.content_type = Some("application/sk-contenttype1234567890".to_owned());
    let attachment_record_path = state
        .workspace_root()
        .join(".jyowo")
        .join("runtime")
        .join("attachments")
        .join("records")
        .join(format!("{}.json", attachment.id));
    let mut attachment_record =
        serde_json::from_str::<Value>(&std::fs::read_to_string(&attachment_record_path).unwrap())
            .expect("attachment record is json");
    attachment_record["attachment"] =
        serde_json::to_value(&attachment).expect("attachment serializes");
    attachment_record["blobRef"]["content_type"] =
        Value::String("application/sk-contenttype1234567890".to_owned());
    std::fs::write(
        &attachment_record_path,
        serde_json::to_vec_pretty(&attachment_record).unwrap(),
    )
    .unwrap();

    let conversation_id = SessionId::new();

    let started = start_run_with_runtime_state(
        StartRunRequest {
            attachments: Some(vec![attachment.clone()]),
            client_message_id: None,
            context_references: Some(vec![ContextReferencePayload::WorkspaceFile {
                path: "notes.md".to_owned(),
                label: "sk-contextlabel1234567890".to_owned(),
            }]),
            conversation_id: conversation_id.to_string(),
            model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
            permission_mode: Some(PermissionMode::BypassPermissions),
            prompt: "Run in background\nsk-12345678901234567890".to_owned(),
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
            .read_envelopes(TenantId::SINGLE, conversation_id, ReplayCursor::FromStart)
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

    let store = agent_runtime_store_for_workspace(state.workspace_root())
        .expect("agent runtime store opens");
    let listed = store
        .list_background_agents(false)
        .expect("background agents list");
    assert_eq!(listed.len(), 1);
    let background_agent_id = listed[0].background_agent_id.as_str();
    let record = store
        .get_background_agent(background_agent_id)
        .expect("background agent lookup succeeds")
        .expect("background agent record exists");
    let attempts = store
        .list_background_agent_attempts(background_agent_id)
        .expect("background attempts load");

    assert_eq!(record.conversation_id, conversation_id.to_string());
    assert_eq!(
        record.state,
        harness_contracts::BackgroundAgentState::Running
    );
    assert_eq!(record.title, "Background investigation");
    assert_eq!(attempts.len(), 1);
    assert_eq!(
        attempts[0].state,
        harness_contracts::BackgroundAgentState::Running
    );
    let payload = serde_json::from_str::<Value>(&record.payload_json).expect("payload is json");
    assert!(!record.payload_json.contains("sk-12345678901234567890"));
    assert!(!record.payload_json.contains("sk-contextlabel1234567890"));
    assert!(!record.payload_json.contains("sk-attachmentname1234567890"));
    assert!(!record.payload_json.contains("sk-mimetype1234567890"));
    assert!(!record.payload_json.contains("sk-contenttype1234567890"));
    assert!(!record
        .payload_json
        .contains(&state.workspace_root().to_string_lossy().to_string()));
    assert!(payload["supervisorExecution"]["sessionOptions"].is_null());
    assert_eq!(
        payload["supervisorExecution"]["session"]["sessionId"],
        conversation_id.to_string()
    );
    assert_eq!(
        payload["supervisorExecution"]["session"]["toolProfile"],
        "coding"
    );
    assert_eq!(payload["conversationId"], conversation_id.to_string());
    assert_eq!(payload["source"], "background_agent_tool");
    assert_eq!(payload["parentRunId"], started.run_id);
    assert_eq!(payload["toolUseId"], background_tool_use_id.to_string());
    assert_eq!(payload["supervisorExecution"]["status"], "queued");
    assert_eq!(
        payload["supervisorExecution"]["input"]["prompt"],
        "Continue\n[REDACTED]"
    );
    assert_eq!(
        payload["supervisorExecution"]["input"]["attachments"],
        json!([])
    );
    assert_eq!(
        payload["supervisorExecution"]["input"]["context_references"],
        json!([])
    );

    supervisor.shutdown().await;
}

#[tokio::test]
async fn background_agent_manager_rejects_recovered_permission_without_live_pending_request() {
    let state = runtime_state_with_harness().await;
    let conversation_id = SessionId::new();
    let request_id = RequestId::new();
    let store = Arc::new(
        agent_runtime_store_for_workspace(state.workspace_root())
            .expect("agent runtime store opens"),
    );
    let harness = state.harness().expect("harness exists");
    let manager = jyowo_harness_sdk::BackgroundAgentManager::new(
        Arc::clone(&store),
        harness.event_store(),
        TenantId::SINGLE,
        conversation_id,
        Arc::new(DefaultRedactor::default()),
    );
    let record = manager
        .start(jyowo_harness_sdk::BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: "permission recovery".to_owned(),
            payload_json: "{}".to_owned(),
        })
        .await
        .expect("start background record");
    manager
        .wait_for_permission(
            record.background_agent_id.as_str(),
            request_id,
            "permission required",
        )
        .await
        .expect("mark waiting permission");
    manager
        .recover_on_startup("process restart")
        .await
        .expect("recover background record");
    assert!(state.pending_permission_requests().is_empty());

    let response = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: conversation_id.to_string(),
            decision: PermissionDecision::Approve,
            option_id: PermissionOptionId::new().to_string(),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await;
    let resolved = store
        .get_background_agent(record.background_agent_id.as_str())
        .expect("background lookup")
        .expect("background exists");

    assert!(response.is_err());
    assert_eq!(
        resolved.state,
        harness_contracts::BackgroundAgentState::Recoverable
    );
}

async fn enable_background_agents(state: &DesktopRuntimeState) -> TestSupervisorControl {
    let supervisor = TestSupervisorControl::start(state.workspace_root());
    let request = SetExecutionSettingsRequest {
        permission_mode: PermissionMode::Default,
        tool_profile: ToolProfile::Full,
        context_compression_trigger_ratio: 0.8,
        subagents_enabled: true,
        agent_teams_enabled: false,
        background_agents_enabled: true,
    };
    let store = DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf());
    let context = AgentCapabilityResolutionContext {
        stream_permission_runtime_available: true,
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    loop {
        match set_execution_settings_with_store(request.clone(), &store, Some(&context)) {
            Ok(_) => break,
            Err(error)
                if error.message.contains("backgroundAgents cannot be enabled")
                    && std::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(error) => panic!("background settings should save: {error:?}"),
        }
    }
    supervisor
}

struct TestSupervisorControl {
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TestSupervisorControl {
    fn start(workspace: &Path) -> Self {
        use std::io::{Read, Write};
        use std::sync::atomic::{AtomicBool, Ordering};

        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("control bind");
        listener.set_nonblocking(true).expect("control nonblocking");
        let control_addr = listener.local_addr().expect("control addr");
        let runtime_dir = workspace.join(".jyowo/runtime");
        std::fs::create_dir_all(&runtime_dir).expect("runtime dir");
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
        .expect("token write");
        write_json_file_atomic(
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
        .expect("lock write");

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let lock_path = runtime_dir.join("agent-supervisor.lock");
        let thread = std::thread::spawn(move || {
            let mut last_heartbeat = std::time::Instant::now();
            while !thread_stop.load(Ordering::Relaxed) {
                if last_heartbeat.elapsed() >= Duration::from_millis(100) {
                    let _ = write_json_file_atomic(
                        &lock_path,
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
                    );
                    last_heartbeat = std::time::Instant::now();
                }
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
        Self {
            stop,
            thread: Some(thread),
        }
    }

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

fn write_json_file_atomic(path: &Path, value: &Value) -> std::io::Result<()> {
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

fn background_manager(
    state: &DesktopRuntimeState,
    conversation_id: SessionId,
) -> jyowo_harness_sdk::BackgroundAgentManager {
    let store = Arc::new(
        agent_runtime_store_for_workspace(state.workspace_root())
            .expect("agent runtime store opens"),
    );
    jyowo_harness_sdk::BackgroundAgentManager::new(
        store,
        state.harness().expect("harness exists").event_store(),
        TenantId::SINGLE,
        conversation_id,
        Arc::new(DefaultRedactor::default()),
    )
}

async fn create_background_agent_record(
    state: &DesktopRuntimeState,
    conversation_id: SessionId,
    title: &str,
) -> jyowo_harness_sdk::BackgroundAgentRecord {
    background_manager(state, conversation_id)
        .start(jyowo_harness_sdk::BackgroundAgentStartRequest {
            background_agent_id: None,
            conversation_id,
            title: title.to_owned(),
            payload_json: json!({
                "conversationId": conversation_id.to_string(),
                "source": "test",
            })
            .to_string(),
        })
        .await
        .expect("background agent starts")
}

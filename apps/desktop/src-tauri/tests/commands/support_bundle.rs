#![allow(unused_imports)]

use harness_provider_state::{
    ProviderContinuationKind, ProviderContinuationRecord, ProviderContinuationScope,
};

use super::automation_support::*;
use super::preview_support::*;
use super::provider_route_support::*;
use super::provider_support::*;
use super::support::*;
use super::*;

#[tokio::test]
async fn replay_and_support_bundle_require_conversation_id_with_run_filter() {
    let state = runtime_state_with_harness().await;

    let replay_error = get_replay_timeline_with_runtime_state(
        ReplayTimelineRequest {
            conversation_id: None,
            run_id: Some(RunId::new().to_string()),
        },
        &state,
    )
    .await
    .unwrap_err();
    let export_error = export_support_bundle_with_runtime_state(
        ExportSupportBundleRequest {
            conversation_id: None,
            run_id: Some(RunId::new().to_string()),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(replay_error.code, "INVALID_PAYLOAD");
    assert_eq!(export_error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn export_support_bundle_with_runtime_state_writes_redacted_files_under_workspace_exports() {
    let _lock = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("support-bundle-export");
    std::fs::create_dir_all(&workspace).unwrap();
    let secret_command =
        "git push https://ghp_abcdefghijklmnopqrstuvwxyz0123456789@github.com/org/repo";
    let state = runtime_state_with_scripted_model_for_workspace(
        workspace.clone(),
        vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::ToolUseComplete {
                    id: ToolUseId::new(),
                    name: "NeedsPermission".to_owned(),
                    input: json!({ "command": secret_command }),
                },
            },
            ModelStreamEvent::MessageStop,
        ])],
    )
    .await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            model_config_id: TEST_MODEL_CONFIG_ID.to_owned(),
            permission_mode: None,
            prompt: "Run a command".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let pending = wait_for_pending_permission_for_session(&state, session_id).await;
    let request_id = pending.request.request_id;

    let payload = export_support_bundle_with_runtime_state(
        ExportSupportBundleRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(started.run_id),
        },
        &state,
    )
    .await
    .unwrap();

    assert!(payload.redacted);
    assert!(payload.event_count >= 2);
    assert!(payload.bundle_path.starts_with(".jyowo/runtime/exports/"));
    assert!(payload.bundle_path.contains("support-bundle-"));
    assert!(payload.jsonl_path.starts_with(".jyowo/runtime/exports/"));
    assert!(payload.markdown_path.starts_with(".jyowo/runtime/exports/"));

    let bundle = std::fs::read_to_string(workspace.join(&payload.bundle_path)).unwrap();
    let jsonl = std::fs::read_to_string(workspace.join(&payload.jsonl_path)).unwrap();
    let markdown = std::fs::read_to_string(workspace.join(&payload.markdown_path)).unwrap();
    let exported = format!("{bundle}\n{jsonl}\n{markdown}");

    assert!(!exported.contains("ghp_abcdefghijklmnopqrstuvwxyz0123456789"));
    assert!(!exported.contains(secret_command));
    assert!(exported.contains("\"target\":\"git\""));
    assert!(exported.contains("\"redacted\":true"));

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Deny,
            option_id: deny_permission_option_id(&pending),
            request_id: request_id.to_string(),
            confirmation_text: None,
        },
        &state,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn support_bundle_does_not_export_provider_continuation_store_payload() {
    let _lock = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("support-bundle-provider-continuation");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace.clone()).await;
    let session_id = SessionId::new();
    open_conversation_session(&state, session_id).await;
    let sentinel = "PRIVATE_DEEPSEEK_REASONING_SENTINEL";
    let store_path = workspace.join(".jyowo/runtime/provider-continuations.jsonl");
    std::fs::create_dir_all(store_path.parent().unwrap()).unwrap();
    let record = ProviderContinuationRecord {
        provider_id: "deepseek".to_owned(),
        model_config_id: Some(TEST_MODEL_CONFIG_ID.to_owned()),
        protocol: ModelProtocol::ChatCompletions,
        dialect: "deepseek".to_owned(),
        tenant_id: TenantId::SINGLE,
        session_id,
        producing_run_id: RunId::new(),
        message_id: MessageId::new(),
        scope: ProviderContinuationScope::Conversation,
        kind: ProviderContinuationKind::ReasoningReplay,
        payload: json!({
            "format": format!("deepseek.{}{}.v1", "reasoning", "_content"),
            "reasoningContent": sentinel,
        }),
        created_at: now(),
    };
    std::fs::write(
        &store_path,
        format!("{}\n", serde_json::to_string(&record).unwrap()),
    )
    .unwrap();

    let payload = export_support_bundle_with_runtime_state(
        ExportSupportBundleRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .unwrap();

    let bundle = std::fs::read_to_string(workspace.join(&payload.bundle_path)).unwrap();
    let jsonl = std::fs::read_to_string(workspace.join(&payload.jsonl_path)).unwrap();
    let markdown = std::fs::read_to_string(workspace.join(&payload.markdown_path)).unwrap();
    let exported = format!("{bundle}\n{jsonl}\n{markdown}");

    assert!(store_path.exists());
    assert!(std::fs::read_to_string(&store_path)
        .unwrap()
        .contains(sentinel));
    assert!(!exported.contains(sentinel));
    assert!(!exported.contains("provider-continuations.jsonl"));
}

#[tokio::test]
async fn export_support_bundle_with_runtime_state_rejects_symlink_export_directory() {
    let _lock = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("support-bundle-symlink-export");
    let external = unique_workspace("support-bundle-external-target");
    std::fs::create_dir_all(workspace.join(".jyowo").join("runtime")).unwrap();
    std::fs::create_dir_all(&external).unwrap();
    std::os::unix::fs::symlink(
        &external,
        workspace.join(".jyowo").join("runtime").join("exports"),
    )
    .unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;
    open_conversation_session(&state, state.default_conversation_id()).await;

    let error = export_support_bundle_with_runtime_state(
        ExportSupportBundleRequest {
            conversation_id: Some(state.default_conversation_id().to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert_eq!(std::fs::read_dir(external).unwrap().count(), 0);
}

#[tokio::test]
async fn support_bundle_agent_redaction_exports_child_agent_summaries_without_internals() {
    let _lock = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("support-bundle-agent-redaction");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace.clone()).await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let subagent_id = harness_contracts::SubagentId::new();
    let team_id = harness_contracts::TeamId::new();
    let background_agent_id = harness_contracts::BackgroundAgentId::new();
    let request_id = RequestId::new();
    let secret = "sk-abcdefghijklmnopqrstuvwxyz";
    open_conversation_session(&state, session_id).await;
    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::RunStarted(test_run_started_event(session_id, run_id)),
                Event::SubagentSpawned(harness_contracts::SubagentSpawnedEvent {
                    subagent_id,
                    parent_session_id: session_id,
                    parent_run_id: run_id,
                    agent_ref: harness_contracts::AgentRef {
                        id: harness_contracts::AgentId::new(),
                        name: format!("Reviewer {secret}"),
                    },
                    spec_snapshot_id: SnapshotId::new(),
                    spec_hash: [0; 32],
                    depth: 1,
                    trigger_tool_use_id: None,
                    trigger_tool_name: Some("agent".to_owned()),
                    at: now(),
                }),
                Event::SubagentAnnounced(harness_contracts::SubagentAnnouncedEvent {
                    subagent_id,
                    parent_session_id: session_id,
                    status: harness_contracts::SubagentStatus::Completed,
                    summary: format!("child completed with {secret}"),
                    result: Some(json!({ "rawOutput": secret })),
                    usage: UsageSnapshot::default(),
                    transcript_ref: Some(harness_contracts::TranscriptRef {
                        blob: harness_contracts::BlobRef {
                            id: harness_contracts::BlobId::new(),
                            size: 64,
                            content_hash: [1; 32],
                            content_type: Some("application/json".to_owned()),
                        },
                        from_offset: harness_contracts::JournalOffset(1),
                        to_offset: harness_contracts::JournalOffset(2),
                    }),
                    context_report: None,
                    renderer_id: "default".to_owned(),
                    at: now(),
                }),
                Event::TeamCreated(harness_contracts::TeamCreatedEvent {
                    team_id,
                    tenant_id: TenantId::SINGLE,
                    name: format!("Team {secret}"),
                    topology_kind: harness_contracts::TopologyKind::CoordinatorWorker,
                    member_specs_hash: [0; 32],
                    created_at: now(),
                }),
                Event::TeamTaskUpdated(harness_contracts::TeamTaskUpdatedEvent {
                    team_id,
                    task_id: format!("task-{secret}"),
                    title: format!("Audit {secret}"),
                    status: "running".to_owned(),
                    assignee_profile_id: Some(format!("worker-{secret}")),
                    at: now(),
                }),
                Event::PermissionRequested(harness_contracts::PermissionRequestedEvent {
                    request_id,
                    run_id,
                    session_id,
                    tenant_id: TenantId::SINGLE,
                    tool_use_id: ToolUseId::new(),
                    tool_name: "NeedsPermission".to_owned(),
                    subject: harness_contracts::PermissionSubject::ToolInvocation {
                        tool: "NeedsPermission".to_owned(),
                        input: json!({}),
                    },
                    severity: harness_contracts::Severity::Medium,
                    scope_hint: harness_contracts::DecisionScope::ToolName(
                        "NeedsPermission".to_owned(),
                    ),
                    fingerprint: None,
                    presented_options: vec![PermissionDecisionOption {
                        option_id: PermissionOptionId::new(),
                        decision: Decision::AllowOnce,
                        scope: DecisionScope::Any,
                        lifetime: DecisionLifetime::Once,
                        matcher_summary: DecisionMatcherSummary {
                            kind: DecisionMatcherKind::Any,
                            label: "allow once".to_owned(),
                        },
                        label: "Allow once".to_owned(),
                        requires_confirmation: false,
                        action_plan_hash: ActionPlanHash::default(),
                        fingerprint: None,
                    }],
                    interactivity: harness_contracts::InteractivityLevel::FullyInteractive,
                    auto_resolved: false,
                    actor_source: PermissionActorSource::TeamMember {
                        team_id,
                        agent_id: harness_contracts::AgentId::new(),
                        role: format!("reviewer {secret}"),
                        parent_run_id: Some(run_id),
                    },
                    action_plan_hash: Default::default(),
                    review: Default::default(),
                    effective_mode: Default::default(),
                    sandbox_policy: Default::default(),
                    causation_id: EventId::new(),
                    at: now(),
                }),
                Event::BackgroundAgentStarted(harness_contracts::BackgroundAgentStartedEvent {
                    background_agent_id,
                    conversation_id: session_id,
                    attempt_id: run_id,
                    title: UiSafeText::from_redacted_display(
                        format!("Background {secret}"),
                        &DefaultRedactor::default(),
                    ),
                    at: now(),
                }),
                Event::BackgroundAgentPermissionRequested(
                    harness_contracts::BackgroundAgentPermissionRequestedEvent {
                        background_agent_id,
                        tenant_id: TenantId::SINGLE,
                        conversation_id: session_id,
                        request_id,
                        attempt_id: Some(run_id),
                        reason: UiSafeText::from_redacted_display(
                            format!("permission {secret}"),
                            &DefaultRedactor::default(),
                        ),
                        at: now(),
                    },
                ),
            ],
        )
        .await
        .unwrap();

    let payload = export_support_bundle_with_runtime_state(
        ExportSupportBundleRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .unwrap();

    let bundle = std::fs::read_to_string(workspace.join(&payload.bundle_path)).unwrap();
    let jsonl = std::fs::read_to_string(workspace.join(&payload.jsonl_path)).unwrap();
    let exported = format!("{bundle}\n{jsonl}");

    assert!(exported.contains("subagent.spawned"));
    assert!(exported.contains("subagent.announced"));
    assert!(exported.contains("team.task.updated"));
    assert!(exported.contains("background.permission.requested"));
    assert!(exported.contains(&subagent_id.to_string()));
    assert!(exported.contains(&team_id.to_string()));
    assert!(exported.contains(&background_agent_id.to_string()));
    assert!(!exported.contains(secret));
    assert!(!exported.contains("child completed"));
    assert!(!exported.contains("rawOutput"));
    assert!(!exported.contains("transcriptRef"));
}

use async_trait::async_trait;
use futures::stream;
use harness_contracts::{
    AssistantClarificationRequestedEvent, AssistantDeltaProducedEvent,
    AssistantMessageCompletedEvent, AssistantNoticeEvent, AssistantReviewRequestedEvent,
    ConfigHash, CorrelationId, DecidedBy, EngineError, EngineFailedEvent, EventId,
    McpConnectionLostEvent, McpConnectionLostReason, MessageContent, MessageId, MessageMetadata,
    PermissionRequestedEvent, PermissionResolvedEvent, ReasoningSummaryChunk, RunStartedEvent,
    SnapshotId, StopReason, ToolErrorPayload, ToolUseFailedEvent, ToolUseRequestedEvent,
    ToolUseSummary, TurnInput, UiSafeText, UserMessageAppendedEvent,
};
use harness_skill::{parse_skill_markdown, SkillPlatform, SkillSource};
use jyowo_desktop_shell::commands::{
    cancel_run_payload, cancel_run_with_runtime_state,
    create_attachment_from_path_with_runtime_state, create_conversation_with_runtime_state,
    delete_conversation_with_runtime_state, delete_mcp_server_with_runtime_state,
    delete_mcp_server_with_store, delete_memory_item_with_runtime_state,
    delete_skill_with_runtime_state, export_memory_items_with_runtime_state,
    export_support_bundle_with_runtime_state, get_app_info_payload,
    get_artifact_media_preview_with_runtime_state, get_context_snapshot_with_runtime_state,
    get_conversation_with_runtime_state, get_execution_settings_with_store,
    get_mcp_server_config_with_runtime_state, get_mcp_server_config_with_store,
    get_memory_item_with_runtime_state, get_provider_config_api_key_with_runtime_state,
    get_provider_config_api_key_with_store, get_replay_timeline_with_runtime_state,
    get_skill_detail_with_runtime_state, get_skill_file_with_runtime_state,
    harness_healthcheck_payload, import_skill_with_runtime_state, list_activity_payload,
    list_activity_with_runtime_state, list_artifacts_with_runtime_state,
    list_conversations_with_runtime_state, list_eval_cases_payload,
    list_eval_cases_with_runtime_state, list_mcp_diagnostics_with_store,
    list_mcp_servers_with_runtime_state, list_memory_items_with_runtime_state,
    list_model_provider_catalog_payload, list_provider_settings_with_store,
    list_reference_candidates_with_runtime_state, list_skills_with_runtime_state,
    mcp_diagnostic_record_from_event, page_conversation_timeline_with_runtime_state,
    page_conversation_worktree_with_runtime_state,
    request_provider_config_api_key_reveal_with_runtime_state,
    request_provider_config_api_key_reveal_with_store,
    resolve_permission_for_window_with_runtime_state, resolve_permission_payload,
    resolve_permission_with_runtime_state, restart_mcp_server_with_runtime_state,
    run_eval_case_payload, run_eval_case_with_runtime_state, runtime_state_async,
    runtime_state_for_workspace, save_mcp_server_with_runtime_state, save_mcp_server_with_store,
    save_provider_settings_with_store, set_conversation_model_config_with_runtime_state,
    set_execution_settings_with_store, set_mcp_server_enabled_with_runtime_state,
    set_skill_enabled_with_runtime_state, start_run_payload, start_run_with_runtime_state,
    subscribe_conversation_events_for_window_with_runtime_state,
    unsubscribe_conversation_events_for_window_with_runtime_state,
    update_memory_item_with_runtime_state, validate_provider_settings_payload,
    ArtifactSummaryPayload, AttachmentBlobRefPayload, AttachmentReferencePayload, CancelRunRequest,
    ContextReferencePayload, ConversationEventBatchPayload, ConversationModelCapabilityRecord,
    CreateAttachmentFromPathRequest, DeleteConversationRequest, DeleteMcpServerRequest,
    DeleteMemoryItemRequest, DeleteSkillRequest, DesktopExecutionSettingsStore,
    DesktopMcpDiagnosticStore, DesktopProviderSettingsStore, DesktopRuntimeState,
    DesktopSkillStore, ExportSupportBundleRequest, GetArtifactMediaPreviewRequest,
    GetContextSnapshotRequest, GetConversationRequest, GetMcpServerConfigRequest,
    GetMemoryItemRequest, GetProviderConfigApiKeyRequest, GetSkillDetailRequest,
    GetSkillFileRequest, ImportSkillRequest, ListActivityRequest, ListArtifactsRequest,
    ListReferenceCandidatesRequest, McpDiagnosticRecord, McpDiagnosticSeverity, McpDiagnosticStore,
    McpHeaderEnvRecord, McpNameValueRecord, McpServerConfigRecord, McpServerStore,
    McpServerTransportConfig, PageConversationTimelineRequest, PageConversationWorktreeDirection,
    PageConversationWorktreeRequest, PermissionDecision, ProviderConfigRecord,
    ProviderModelDescriptorRecord, ProviderModelLifecycleRecord, ProviderModelModalityRecord,
    ProviderSettingsRecord, ProviderSettingsRequest, ProviderSettingsStore, ReplayTimelineRequest,
    RequestProviderConfigApiKeyRevealRequest, ResolvePermissionRequest, RestartMcpServerRequest,
    RunEvalCaseRequest, SaveMcpServerRequest, SetConversationModelConfigRequest,
    SetExecutionSettingsRequest, SetMcpServerEnabledRequest, SetSkillEnabledRequest, SkillStore,
    SkillStoreRecord, StartRunRequest, SubscribeConversationEventsRequest,
    UnsubscribeConversationEventsRequest, UpdateMemoryItemRequest, ValidateProviderSettingsRequest,
};
use jyowo_harness_sdk::builtin::{DefaultRedactor, FileBlobStore};
use jyowo_harness_sdk::ext::{
    now, ArtifactCreatedEvent, ArtifactSource, ArtifactStatus, ArtifactUpdatedEvent, BlobMeta,
    BlobRetention, BlobStore, BudgetMetric, Decision, DecisionScope, DeferPolicy, DeltaChunk,
    Event, EventStore, FallbackPolicy, InteractivityLevel, McpConnection, McpError, McpRegistry,
    McpServerId, McpServerScope, McpServerSource, McpServerSpec, McpToolDescriptor, McpToolResult,
    MemoryId, MemoryKind, MemoryMetadata, MemoryRecord, MemorySource, MemoryStore,
    MemoryVisibility, Message, MessagePart, MessageRole, ModelError, ModelProtocol, OverflowAction,
    PermissionCheck, PermissionContext, PermissionMode, PermissionRequest, PermissionSubject,
    ProviderRestriction, RedactPatternSet, RedactRules, RedactScope, Redactor, RequestId,
    ResultBudget, RuleSnapshot, RunId, SessionId, Severity, StreamBrokerConfig, TenantId,
    ThinkingDelta, Tool, ToolContext, ToolDescriptor, ToolError, ToolEvent, ToolGroup,
    ToolProperties, ToolRegistry, ToolResult, ToolStream, ToolUseId, TransportChoice, TrustLevel,
    UsageSnapshot, ValidationError,
};
use jyowo_harness_sdk::ext::{ContentDelta, ModelStreamEvent};
use jyowo_harness_sdk::testing::{
    InMemoryEventStore, MockMemoryProvider, MockProvider, NoopRedactor, NoopSandbox,
    ScriptedProvider, ScriptedResponse,
};
use jyowo_harness_sdk::{
    ConversationEventsPageRequest, Harness, HarnessOptions, McpConfig, StreamPermissionRuntime,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static WORKSPACE_ROOT_ENV_LOCK: Mutex<()> = Mutex::new(());
const WORKSPACE_ROOT_ENV: &str = "JYOWO_WORKSPACE_ROOT";

#[test]
fn get_app_info_payload_returns_jyowo_identity() {
    let payload = get_app_info_payload();

    assert_eq!(payload.name, "Jyowo");
    assert_eq!(payload.shell, "tauri2-react");
    assert_eq!(payload.harness.sdk_crate, "jyowo_harness_sdk");
    assert_eq!(payload.harness.mode, "in-process");
}

#[test]
fn harness_healthcheck_payload_reports_available_sdk() {
    let payload = harness_healthcheck_payload();

    assert_eq!(payload.status, "available");
    assert_eq!(payload.sdk_crate, "jyowo_harness_sdk");
}

#[test]
fn eval_lab_payloads_require_runtime_instead_of_static_support_cases() {
    let list_error = list_eval_cases_payload().unwrap_err();
    assert_eq!(list_error.code, "RUNTIME_UNAVAILABLE");

    let error = run_eval_case_payload(RunEvalCaseRequest {
        case_id: "regression-smoke".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");
}

#[test]
fn eval_lab_runtime_state_paths_require_eval_runtime() {
    let workspace = unique_workspace("eval-no-runtime");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("workspace state should initialize without a harness");

    let list_error = list_eval_cases_with_runtime_state(&state).unwrap_err();
    assert_eq!(list_error.code, "RUNTIME_UNAVAILABLE");

    let run_error = run_eval_case_with_runtime_state(
        RunEvalCaseRequest {
            case_id: "regression-smoke".to_owned(),
        },
        &state,
    )
    .unwrap_err();
    assert_eq!(run_error.code, "RUNTIME_UNAVAILABLE");
}

#[test]
fn artifact_payload_skips_missing_optional_fields() {
    let value = serde_json::to_value(ArtifactSummaryPayload {
        action_label: "Open".to_owned(),
        description: "Generated implementation plan".to_owned(),
        id: "artifact-no-preview".to_owned(),
        kind: "markdown".to_owned(),
        preview: None,
        source_message_id: None,
        source_run_id: "run-001".to_owned(),
        status: "ready",
        title: "Generated output".to_owned(),
    })
    .unwrap();

    assert_eq!(value.get("preview"), None);
    assert_eq!(value.get("sourceMessageId"), None);
    assert_eq!(value.get("sourceRunId"), None);
}

#[tokio::test]
async fn import_skill_persists_enabled_skill_without_exposing_source_path() {
    let workspace = unique_workspace("skill-import");
    std::fs::create_dir_all(&workspace).unwrap();
    let source_dir = unique_workspace("skill-source");
    let source_path = write_skill_package(
        &source_dir,
        "summarize",
        "summarize",
        "Summarize project notes",
        Some(("references/style.md", "Use concise bullets.")),
    );
    let state = runtime_state_for_workspace(workspace.clone())
        .await
        .unwrap();

    let imported = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: source_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    let serialized = serde_json::to_string(&imported).unwrap();

    assert_eq!(imported.skill.name, "summarize");
    assert!(imported.skill.enabled);
    assert!(imported.skill.manageable);
    assert_eq!(imported.skill.source_kind, "workspace");
    assert!(!serialized.contains(&source_dir.to_string_lossy().to_string()));
    assert!(workspace
        .join(".jyowo/runtime/skills/enabled")
        .join(&imported.skill.id)
        .join("SKILL.md")
        .exists());
    assert!(workspace
        .join(".jyowo/runtime/skills/enabled")
        .join(&imported.skill.id)
        .join("references/style.md")
        .exists());
}

#[tokio::test]
async fn import_skill_rejects_single_markdown_files() {
    let workspace = unique_workspace("skill-import-reject-file");
    std::fs::create_dir_all(&workspace).unwrap();
    let source_dir = unique_workspace("skill-file-source");
    std::fs::create_dir_all(&source_dir).unwrap();
    let source_path = source_dir.join("summarize.md");
    std::fs::write(
        &source_path,
        skill_markdown("summarize", "Summarize project notes"),
    )
    .unwrap();
    let source_path = source_path.canonicalize().unwrap();
    let state = runtime_state_for_workspace(workspace).await.unwrap();

    let error = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: source_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error
        .message
        .contains("skill source path must point to a directory"));
}

#[cfg(unix)]
#[tokio::test]
async fn import_skill_rejects_symlink_source_package() {
    let workspace = unique_workspace("skill-import-reject-source-symlink");
    std::fs::create_dir_all(&workspace).unwrap();
    let source_dir = unique_workspace("skill-source-real");
    let source_path = write_skill_package(
        &source_dir,
        "symlinked",
        "symlinked",
        "Should be rejected",
        None,
    );
    let link_dir = unique_workspace("skill-source-link");
    std::fs::create_dir_all(&link_dir).unwrap();
    let linked_path = link_dir.join("linked-package");
    std::os::unix::fs::symlink(&source_path, &linked_path).unwrap();
    let state = runtime_state_for_workspace(workspace).await.unwrap();

    let error = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: linked_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert!(error.message.contains("must not use symlinks"));
}

#[tokio::test]
async fn disabling_skill_moves_file_and_removes_it_from_runtime_list() {
    let workspace = unique_workspace("skill-disable");
    std::fs::create_dir_all(&workspace).unwrap();
    let source_dir = unique_workspace("skill-disable-source");
    let source_path =
        write_skill_package(&source_dir, "draft", "draft", "Draft release notes", None);
    let state = runtime_state_for_workspace(workspace.clone())
        .await
        .unwrap();
    let imported = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: source_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap();

    let disabled = set_skill_enabled_with_runtime_state(
        SetSkillEnabledRequest {
            id: imported.skill.id.clone(),
            enabled: false,
        },
        &state,
    )
    .await
    .unwrap();
    let listed = list_skills_with_runtime_state(&state).await.unwrap();

    assert!(!disabled.skill.enabled);
    assert_eq!(disabled.skill.status, "disabled");
    assert!(workspace
        .join(".jyowo/runtime/skills/disabled")
        .join(&imported.skill.id)
        .join("SKILL.md")
        .exists());
    assert!(listed
        .skills
        .iter()
        .any(|skill| skill.id == imported.skill.id && !skill.enabled));
    assert!(listed
        .skills
        .iter()
        .all(|skill| skill.name != "draft" || !skill.enabled));

    let enabled = set_skill_enabled_with_runtime_state(
        SetSkillEnabledRequest {
            id: imported.skill.id.clone(),
            enabled: true,
        },
        &state,
    )
    .await
    .unwrap();
    let listed = list_skills_with_runtime_state(&state).await.unwrap();

    assert!(enabled.skill.enabled);
    assert_eq!(enabled.skill.status, "ready");
    assert!(workspace
        .join(".jyowo/runtime/skills/enabled")
        .join(&imported.skill.id)
        .join("SKILL.md")
        .exists());
    assert!(listed
        .skills
        .iter()
        .any(|skill| skill.id == imported.skill.id && skill.enabled));
}

#[tokio::test]
async fn enabling_skill_rejects_runtime_duplicate_name() {
    let workspace = unique_workspace("skill-enable-duplicate-runtime");
    std::fs::create_dir_all(&workspace).unwrap();
    let disabled_id = "managed-disabled";
    let disabled_dir = workspace
        .join(".jyowo/runtime/skills/disabled")
        .join(disabled_id);
    std::fs::create_dir_all(&disabled_dir).unwrap();
    std::fs::write(
        disabled_dir.join("SKILL.md"),
        skill_markdown("shared-name", "Workspace skill"),
    )
    .unwrap();
    let record = SkillStoreRecord {
        id: disabled_id.to_owned(),
        name: "shared-name".to_owned(),
        description: "Workspace skill".to_owned(),
        enabled: false,
        content_hash: "test-hash".to_owned(),
        package_dir: disabled_id.to_owned(),
        file_name: String::new(),
        imported_at: now().to_rfc3339(),
        updated_at: now().to_rfc3339(),
        tags: Vec::new(),
        category: None,
        last_validation_error: None,
        origin: None,
    };
    let index_path = workspace.join(".jyowo/runtime/skills/index.json");
    std::fs::write(
        &index_path,
        serde_json::to_vec_pretty(&vec![record]).unwrap(),
    )
    .unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace.clone()).await;
    register_test_skill(&state, "shared-name", "Runtime skill");

    let error = set_skill_enabled_with_runtime_state(
        SetSkillEnabledRequest {
            id: disabled_id.to_owned(),
            enabled: true,
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error
        .message
        .contains("active skill name already exists: shared-name"));
    assert!(workspace
        .join(".jyowo/runtime/skills/disabled")
        .join(disabled_id)
        .join("SKILL.md")
        .exists());
    assert!(!workspace
        .join(".jyowo/runtime/skills/enabled")
        .join(disabled_id)
        .exists());
}

#[tokio::test]
async fn delete_skill_removes_managed_record_and_file() {
    let workspace = unique_workspace("skill-delete");
    std::fs::create_dir_all(&workspace).unwrap();
    let source_dir = unique_workspace("skill-delete-source");
    let source_path = write_skill_package(
        &source_dir,
        "cleanup",
        "cleanup",
        "Clean up workspace",
        None,
    );
    let state = runtime_state_for_workspace(workspace.clone())
        .await
        .unwrap();
    let imported = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: source_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap();

    let deleted = delete_skill_with_runtime_state(
        DeleteSkillRequest {
            id: imported.skill.id.clone(),
        },
        &state,
    )
    .await
    .unwrap();
    let listed = list_skills_with_runtime_state(&state).await.unwrap();

    assert_eq!(deleted.id, imported.skill.id);
    assert_eq!(deleted.status, "deleted");
    assert!(!workspace
        .join(".jyowo/runtime/skills/enabled")
        .join(&imported.skill.id)
        .exists());
    assert!(listed
        .skills
        .iter()
        .all(|skill| skill.id != imported.skill.id));
}

#[tokio::test]
async fn delete_skill_removes_disabled_managed_record_and_file() {
    let workspace = unique_workspace("skill-delete-disabled");
    std::fs::create_dir_all(&workspace).unwrap();
    let source_dir = unique_workspace("skill-delete-disabled-source");
    let source_path = write_skill_package(
        &source_dir,
        "disabled-cleanup",
        "disabled-cleanup",
        "Clean up disabled workspace",
        None,
    );
    let state = runtime_state_for_workspace(workspace.clone())
        .await
        .unwrap();
    let imported = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: source_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    set_skill_enabled_with_runtime_state(
        SetSkillEnabledRequest {
            id: imported.skill.id.clone(),
            enabled: false,
        },
        &state,
    )
    .await
    .unwrap();

    let deleted = delete_skill_with_runtime_state(
        DeleteSkillRequest {
            id: imported.skill.id.clone(),
        },
        &state,
    )
    .await
    .unwrap();
    let listed = list_skills_with_runtime_state(&state).await.unwrap();

    assert_eq!(deleted.id, imported.skill.id);
    assert_eq!(deleted.status, "deleted");
    assert!(!workspace
        .join(".jyowo/runtime/skills/disabled")
        .join(&imported.skill.id)
        .exists());
    assert!(listed
        .skills
        .iter()
        .all(|skill| skill.id != imported.skill.id));
}

#[tokio::test]
async fn get_skill_detail_and_file_return_managed_skill_metadata_lazily() {
    let workspace = unique_workspace("skill-detail");
    std::fs::create_dir_all(&workspace).unwrap();
    let source_dir = unique_workspace("skill-detail-source");
    let source_path = source_dir.join("outline");
    std::fs::create_dir_all(&source_path).unwrap();
    std::fs::write(
        source_path.join("SKILL.md"),
        "---\nname: outline\ndescription: Build an outline\nparameters:\n  - name: topic\n    type: string\n    required: true\nconfig:\n  - key: STYLE_GUIDE\n    type: string\n---\nUse ${topic} and ${config.STYLE_GUIDE}.\n",
    )
    .unwrap();
    std::fs::create_dir_all(source_path.join("references")).unwrap();
    std::fs::write(
        source_path.join("references/style.md"),
        "Use terse outline headings.\n",
    )
    .unwrap();
    let source_path = source_path.canonicalize().unwrap();
    let state = runtime_state_for_workspace(workspace).await.unwrap();
    let imported = import_skill_with_runtime_state(
        ImportSkillRequest {
            source_path: source_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap();

    let detail = get_skill_detail_with_runtime_state(
        GetSkillDetailRequest {
            id: imported.skill.id.clone(),
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(detail.skill.summary.name, "outline");
    assert_eq!(detail.skill.parameters[0].name, "topic");
    assert_eq!(detail.skill.config_keys, vec!["STYLE_GUIDE"]);
    assert_eq!(
        detail.skill.body_preview,
        "Use ${topic} and ${config.STYLE_GUIDE}.\n"
    );
    assert!(detail
        .skill
        .files
        .iter()
        .any(|file| file.path == "SKILL.md" && file.kind == "file"));
    assert!(detail
        .skill
        .files
        .iter()
        .any(|file| file.path == "references" && file.kind == "directory"));
    assert!(detail
        .skill
        .files
        .iter()
        .any(|file| file.path == "references/style.md" && file.kind == "file"));

    let selected = get_skill_file_with_runtime_state(
        GetSkillFileRequest {
            id: imported.skill.id,
            path: "references/style.md".to_owned(),
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(
        selected.file.content.as_str(),
        "Use terse outline headings.\n"
    );
}

#[test]
fn run_eval_case_payload_requires_runtime_for_valid_case_ids_and_rejects_malformed_ids() {
    let unknown = run_eval_case_payload(RunEvalCaseRequest {
        case_id: "unknown-case".to_owned(),
    })
    .unwrap_err();
    assert_eq!(unknown.code, "RUNTIME_UNAVAILABLE");

    let malformed = run_eval_case_payload(RunEvalCaseRequest {
        case_id: "bad case".to_owned(),
    })
    .unwrap_err();
    assert_eq!(malformed.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn validate_provider_settings_payload_accepts_supported_provider_metadata() {
    let payload = validate_provider_settings_payload(ValidateProviderSettingsRequest {
        model_id: "gpt-5.4-mini".to_owned(),
        provider_id: "openai".to_owned(),
    })
    .await
    .unwrap();
    let value = serde_json::to_value(payload).unwrap();

    assert_eq!(
        value,
        json!({
            "modelId": "gpt-5.4-mini",
            "providerId": "openai",
            "status": "accepted"
        })
    );
}

#[test]
fn list_model_provider_catalog_payload_exposes_models_and_default_base_urls() {
    let payload = list_model_provider_catalog_payload();
    let value = serde_json::to_value(payload).unwrap();
    let providers = value["providers"].as_array().unwrap();

    let openai = providers
        .iter()
        .find(|provider| provider["providerId"] == "openai")
        .unwrap();
    assert_eq!(openai["displayName"], "OpenAI");
    assert_eq!(openai["defaultBaseUrl"], "https://api.openai.com");
    assert!(openai["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|model| model["modelId"] == "gpt-5.4-mini"));
    assert_eq!(openai["runtimeCapability"]["authScheme"], "bearer");
    assert!(openai["runtimeCapability"].get("auth_scheme").is_none());

    let anthropic = providers
        .iter()
        .find(|provider| provider["providerId"] == "anthropic")
        .unwrap();
    assert_eq!(anthropic["runtimeCapability"]["authScheme"], "x_api_key");

    let gemini = providers
        .iter()
        .find(|provider| provider["providerId"] == "gemini")
        .unwrap();
    assert_eq!(gemini["runtimeCapability"]["authScheme"], "api_key");

    let local_llama = providers
        .iter()
        .find(|provider| provider["providerId"] == "local-llama")
        .unwrap();
    assert_eq!(local_llama["runtimeCapability"]["authScheme"], "none");

    let km = providers
        .iter()
        .find(|provider| provider["providerId"] == "km")
        .unwrap();
    assert_eq!(km["displayName"], "Kimi");
    assert_eq!(km["defaultBaseUrl"], "https://api.moonshot.cn");
    assert!(km["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|model| model["modelId"] == "kimi-k2.5"));

    let minimax = providers
        .iter()
        .find(|provider| provider["providerId"] == "minimax")
        .unwrap();
    let service = minimax["serviceCapabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["operationId"] == "minimax.image_generation")
        .unwrap();
    assert_eq!(service["requiresPolling"], false);
    assert!(service.get("operation_id").is_none());
    assert!(!minimax["serviceCapabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|service| service["operationId"] == "minimax.text_to_speech.websocket"));
    assert!(!minimax["serviceCapabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|service| service["execution"] == "websocket"));
}

#[tokio::test]
async fn save_provider_settings_payload_stores_viewable_api_key_but_omits_key_from_list_payload() {
    let raw_key = "provider-test-token";
    let store = RecordingProviderSettingsStore::default();
    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some(raw_key.to_owned()),
            base_url: None,
            config_id: None,
            display_name: Some("OpenAI Mini".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            provider_id: "openai".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(serialized.contains("\"status\":\"saved\""));
    assert!(serialized.contains("\"displayName\":\"OpenAI Mini\""));
    assert!(serialized.contains("\"isDefault\":true"));
    assert!(serialized.contains("\"hasApiKey\":true"));
    assert!(!serialized.contains(raw_key));
    let record = store.record.lock().unwrap().clone().unwrap();
    assert_eq!(record.default_config_id.as_deref(), Some("openai"));
    assert_eq!(record.configs.len(), 1);
    assert_eq!(record.configs[0].protocol, ModelProtocol::Responses);
    assert!(!record.configs[0].api_key.trim().is_empty());
    assert_eq!(record.configs[0].display_name, "OpenAI Mini");
    assert_eq!(record.configs[0].model_descriptor.model_id, "gpt-5.4-mini");

    let listed = list_provider_settings_with_store(&store).await.unwrap();
    let listed_serialized = serde_json::to_string(&listed).unwrap();
    assert_eq!(listed.default_config_id.as_deref(), Some("openai"));
    assert!(listed_serialized.contains("\"hasApiKey\":true"));
    assert!(!listed_serialized.contains(raw_key));
}

#[tokio::test]
async fn get_provider_config_api_key_with_store_rejects_plaintext_reveal() {
    let raw_key = "provider-test-token";
    let store = RecordingProviderSettingsStore {
        record: Mutex::new(Some(ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: raw_key.to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })),
        ..RecordingProviderSettingsStore::default()
    };

    let error = get_provider_config_api_key_with_store(
        GetProviderConfigApiKeyRequest {
            config_id: "openai".to_owned(),
            reveal_token: "test-reveal-token".to_owned(),
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("disabled"));
}

#[tokio::test]
async fn get_provider_config_api_key_with_runtime_state_rejects_plaintext_reveal() {
    let raw_key = "provider-test-token";
    let workspace = unique_workspace("provider-key-reveal-token");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    DesktopProviderSettingsStore::new(workspace.clone())
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: raw_key.to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;

    let error = get_provider_config_api_key_with_runtime_state(
        GetProviderConfigApiKeyRequest {
            config_id: "openai".to_owned(),
            reveal_token: "test-reveal-token".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INVALID_PAYLOAD");

    let reveal_error = request_provider_config_api_key_reveal_with_runtime_state(
        RequestProviderConfigApiKeyRevealRequest {
            config_id: "openai".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(reveal_error.code, "INVALID_PAYLOAD");
    assert!(reveal_error.message.contains("disabled"));
}

#[tokio::test]
async fn request_provider_config_api_key_reveal_with_store_is_disabled() {
    let store = RecordingProviderSettingsStore {
        record: Mutex::new(Some(ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })),
        ..RecordingProviderSettingsStore::default()
    };

    let error = request_provider_config_api_key_reveal_with_store(
        RequestProviderConfigApiKeyRevealRequest {
            config_id: "openai".to_owned(),
        },
        &store,
    )
    .await
    .expect_err("plaintext key reveal should fail closed");
    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("disabled"));
}

#[tokio::test]
async fn save_provider_settings_payload_allows_same_provider_model_multiple_keys() {
    let store = RecordingProviderSettingsStore::default();

    let work = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("work-token".to_owned()),
            base_url: None,
            config_id: Some("openai-work".to_owned()),
            display_name: Some("OpenAI Work".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            provider_id: "openai".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();
    let personal = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("personal-token".to_owned()),
            base_url: None,
            config_id: Some("openai-personal".to_owned()),
            display_name: Some("OpenAI Personal".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            provider_id: "openai".to_owned(),
            set_default: false,
        },
        &store,
    )
    .await
    .unwrap();

    assert!(work.config.is_default);
    assert!(!personal.config.is_default);
    let record = store.record.lock().unwrap().clone().unwrap();
    assert_eq!(record.default_config_id.as_deref(), Some("openai-work"));
    assert_eq!(record.configs.len(), 2);
    assert_eq!(record.configs[0].model_id, record.configs[1].model_id);
    assert_ne!(record.configs[0].api_key, record.configs[1].api_key);
}

#[tokio::test]
async fn list_provider_settings_payload_returns_profiles_without_raw_keys() {
    let store = RecordingProviderSettingsStore {
        record: Mutex::new(Some(ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: Some("https://gateway.example.com".to_owned()),
                display_name: "OpenAI gateway".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })),
        ..RecordingProviderSettingsStore::default()
    };

    let payload = list_provider_settings_with_store(&store).await.unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(serialized.contains("\"defaultConfigId\":\"openai\""));
    assert!(serialized.contains("\"baseUrl\":\"https://gateway.example.com\""));
    assert!(serialized.contains("\"hasApiKey\":true"));
    assert!(!serialized.contains("provider-test-token"));
}

#[tokio::test]
async fn list_provider_settings_payload_returns_saved_openrouter_dynamic_descriptor() {
    let store = RecordingProviderSettingsStore {
        record: Mutex::new(Some(ProviderSettingsRecord {
            default_config_id: Some("openrouter".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::ChatCompletions,
                base_url: None,
                display_name: "OpenRouter dynamic".to_owned(),
                id: "openrouter".to_owned(),
                model_id: "dynamic/provider-model".to_owned(),
                provider_id: "openrouter".to_owned(),
                model_descriptor: openrouter_descriptor_record(
                    "dynamic/provider-model",
                    vec![ProviderModelModalityRecord::Text],
                    vec![ProviderModelModalityRecord::Text],
                    true,
                ),
            }],
        })),
        ..RecordingProviderSettingsStore::default()
    };

    let payload = list_provider_settings_with_store(&store).await.unwrap();

    assert_eq!(payload.configs[0].protocol, ModelProtocol::ChatCompletions);
    let descriptor = &payload.configs[0].model_descriptor;
    assert_eq!(descriptor.model_id, "dynamic/provider-model");
    assert_eq!(descriptor.runtime_status.kind, "runnable");
    assert_eq!(
        descriptor.conversation_capability.input_modalities,
        vec![ProviderModelModalityRecord::Text]
    );
}

#[tokio::test]
async fn list_provider_settings_payload_rejects_openrouter_descriptor_with_unsupported_modalities()
{
    let store = RecordingProviderSettingsStore {
        record: Mutex::new(Some(ProviderSettingsRecord {
            default_config_id: Some("openrouter".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::ChatCompletions,
                base_url: None,
                display_name: "OpenRouter image".to_owned(),
                id: "openrouter".to_owned(),
                model_id: "dynamic/image-model".to_owned(),
                provider_id: "openrouter".to_owned(),
                model_descriptor: openrouter_descriptor_record(
                    "dynamic/image-model",
                    vec![
                        ProviderModelModalityRecord::Text,
                        ProviderModelModalityRecord::Image,
                    ],
                    vec![ProviderModelModalityRecord::Text],
                    true,
                ),
            }],
        })),
        ..RecordingProviderSettingsStore::default()
    };

    let error = list_provider_settings_with_store(&store).await.unwrap_err();

    assert_eq!(error.code, "RUNTIME_INIT_FAILED");
}

#[tokio::test]
async fn list_provider_settings_payload_rejects_openrouter_descriptor_with_wrong_protocol() {
    let mut descriptor = openrouter_descriptor_record(
        "dynamic/messages-model",
        vec![ProviderModelModalityRecord::Text],
        vec![ProviderModelModalityRecord::Text],
        true,
    );
    descriptor.protocol = ModelProtocol::Messages;
    let store = RecordingProviderSettingsStore {
        record: Mutex::new(Some(ProviderSettingsRecord {
            default_config_id: Some("openrouter".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Messages,
                base_url: None,
                display_name: "OpenRouter wrong protocol".to_owned(),
                id: "openrouter".to_owned(),
                model_id: "dynamic/messages-model".to_owned(),
                provider_id: "openrouter".to_owned(),
                model_descriptor: descriptor,
            }],
        })),
        ..RecordingProviderSettingsStore::default()
    };

    let error = list_provider_settings_with_store(&store).await.unwrap_err();

    assert_eq!(error.code, "RUNTIME_INIT_FAILED");
}

#[test]
fn provider_settings_record_rejects_legacy_single_provider_shape() {
    let legacy = json!({
        "modelId": "gpt-5.4-mini",
        "providerId": "openai",
        "secretRef": "provider/workspace-local/openai/default"
    });

    assert!(serde_json::from_value::<ProviderSettingsRecord>(legacy).is_err());
}

#[test]
fn provider_settings_record_rejects_config_without_new_model_descriptor() {
    let legacy = json!({
        "defaultConfigId": "openai",
        "configs": [{
            "apiKey": "provider-test-token",
            "baseUrl": "https://gateway.example.com",
            "displayName": "OpenAI gateway",
            "id": "openai",
            "modelId": "gpt-5.4-mini",
            "providerId": "openai"
        }]
    });

    assert!(serde_json::from_value::<ProviderSettingsRecord>(legacy).is_err());
}

#[test]
fn desktop_provider_settings_store_deletes_legacy_provider_settings_file() {
    let workspace = unique_workspace("provider-settings-legacy-provider-settings");
    let settings_dir = workspace.join(".jyowo").join("runtime");
    std::fs::create_dir_all(&settings_dir).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let settings_dir = workspace.join(".jyowo").join("runtime");
    let settings_path = settings_dir.join("provider-settings.json");
    let mut descriptor = serde_json::to_value(openai_descriptor_record("gpt-5.4-mini")).unwrap();
    let descriptor_object = descriptor.as_object_mut().unwrap();
    let protocol = descriptor_object.remove("protocol").unwrap();
    descriptor_object.insert("apiMode".to_owned(), protocol);
    descriptor_object.remove("conversationCapability").unwrap();
    descriptor_object.insert(
        "capabilities".to_owned(),
        json!({
            "supportsTools": true,
            "supportsVision": true,
            "supportsThinking": false,
            "supportsStreaming": true,
            "supportsStructuredOutput": true,
            "supportsJsonMode": true,
            "supportsParallelToolCalls": true,
            "supportsBuiltinWebSearch": false,
            "supportsBuiltinCodeExecution": false,
            "supportsPromptCache": true,
            "inputModalities": ["text", "image"],
            "outputModalities": ["text"]
        }),
    );
    let record = json!({
        "defaultConfigId": "openai",
        "configs": [{
            "apiKey": "provider-test-token",
            "apiMode": "responses",
            "displayName": "OpenAI",
            "id": "openai",
            "modelId": "gpt-5.4-mini",
            "providerId": "openai",
            "modelDescriptor": descriptor
        }]
    });
    std::fs::write(&settings_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    let store = DesktopProviderSettingsStore::new(workspace);

    assert_eq!(store.load_record().unwrap(), None);
    assert!(!settings_path.exists());
}

fn openrouter_descriptor_record(
    model_id: &str,
    input_modalities: Vec<ProviderModelModalityRecord>,
    output_modalities: Vec<ProviderModelModalityRecord>,
    supports_streaming: bool,
) -> ProviderModelDescriptorRecord {
    ProviderModelDescriptorRecord {
        protocol: ModelProtocol::ChatCompletions,
        conversation_capability: ConversationModelCapabilityRecord {
            input_modalities,
            output_modalities,
            context_window: 128_000,
            max_output_tokens: 8_192,
            streaming: supports_streaming,
            tool_calling: true,
            reasoning: false,
            prompt_cache: false,
            structured_output: true,
        },
        context_window: 128_000,
        display_name: "Dynamic OpenRouter model".to_owned(),
        lifecycle: ProviderModelLifecycleRecord::Stable,
        max_output_tokens: 8_192,
        model_id: model_id.to_owned(),
        provider_id: "openrouter".to_owned(),
    }
}

fn openai_descriptor_record(model_id: &str) -> ProviderModelDescriptorRecord {
    ProviderModelDescriptorRecord {
        protocol: ModelProtocol::Responses,
        conversation_capability: ConversationModelCapabilityRecord {
            input_modalities: vec![ProviderModelModalityRecord::Text],
            output_modalities: vec![ProviderModelModalityRecord::Text],
            context_window: 128_000,
            max_output_tokens: 16_384,
            streaming: true,
            tool_calling: true,
            reasoning: false,
            prompt_cache: true,
            structured_output: true,
        },
        context_window: 128_000,
        display_name: "GPT-5.4 mini".to_owned(),
        lifecycle: ProviderModelLifecycleRecord::Stable,
        max_output_tokens: 16_384,
        model_id: model_id.to_owned(),
        provider_id: "openai".to_owned(),
    }
}

#[test]
fn provider_settings_record_rejects_config_secret_ref() {
    let record = json!({
        "defaultConfigId": "openai-gateway",
        "configs": [{
            "apiKey": "provider-test-token",
            "protocol": "responses",
            "baseUrl": "https://gateway.example.com",
            "displayName": "OpenAI gateway",
            "id": "openai-gateway",
            "modelId": "gpt-5.4-mini",
            "providerId": "openai",
            "secretRef": "provider/workspace-local/openai/default"
        }]
    });

    assert!(serde_json::from_value::<ProviderSettingsRecord>(record).is_err());
}

#[test]
fn provider_settings_record_rejects_config_without_api_key() {
    let record = json!({
        "defaultConfigId": "openai",
        "configs": [{
            "protocol": "responses",
            "displayName": "OpenAI",
            "id": "openai",
            "modelId": "gpt-5.4-mini",
            "providerId": "openai"
        }]
    });

    assert!(serde_json::from_value::<ProviderSettingsRecord>(record).is_err());
}

#[test]
fn provider_settings_record_rejects_configs_without_default_config_id() {
    let record = json!({
        "configs": [{
            "apiKey": "provider-test-token",
            "protocol": "responses",
            "displayName": "OpenAI",
            "id": "openai",
            "modelId": "gpt-5.4-mini",
            "providerId": "openai"
        }]
    });

    assert!(serde_json::from_value::<ProviderSettingsRecord>(record).is_err());
}

#[test]
fn provider_settings_record_rejects_default_config_id_missing_from_configs() {
    let record = json!({
        "defaultConfigId": "missing",
        "configs": [{
            "apiKey": "provider-test-token",
            "protocol": "responses",
            "displayName": "OpenAI",
            "id": "openai",
            "modelId": "gpt-5.4-mini",
            "providerId": "openai"
        }]
    });

    assert!(serde_json::from_value::<ProviderSettingsRecord>(record).is_err());
}

#[tokio::test]
async fn save_provider_settings_payload_reuses_saved_openrouter_dynamic_descriptor() {
    let store = RecordingProviderSettingsStore::default();
    *store.record.lock().unwrap() = Some(ProviderSettingsRecord {
        default_config_id: Some("openrouter".to_owned()),
        configs: vec![ProviderConfigRecord {
            api_key: "provider-test-token".to_owned(),
            protocol: ModelProtocol::ChatCompletions,
            base_url: Some("https://openrouter.ai/api".to_owned()),
            display_name: "OpenRouter dynamic".to_owned(),
            id: "openrouter".to_owned(),
            model_id: "dynamic/provider-model".to_owned(),
            provider_id: "openrouter".to_owned(),
            model_descriptor: openrouter_descriptor_record(
                "dynamic/provider-model",
                vec![ProviderModelModalityRecord::Text],
                vec![ProviderModelModalityRecord::Text],
                true,
            ),
        }],
    });

    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: None,
            base_url: Some("https://openrouter.ai/api".to_owned()),
            config_id: Some("openrouter".to_owned()),
            display_name: Some("OpenRouter dynamic".to_owned()),
            model_id: "dynamic/provider-model".to_owned(),
            provider_id: "openrouter".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    assert_eq!(payload.config.model_id, "dynamic/provider-model");
    assert_eq!(payload.config.protocol, ModelProtocol::ChatCompletions);
    assert_eq!(
        payload.config.model_descriptor.model_id,
        "dynamic/provider-model"
    );
}

#[tokio::test]
async fn save_provider_settings_payload_requires_api_key_when_base_url_changes() {
    let store = RecordingProviderSettingsStore::default();
    *store.record.lock().unwrap() = Some(ProviderSettingsRecord {
        default_config_id: Some("openai-gateway".to_owned()),
        configs: vec![ProviderConfigRecord {
            api_key: "provider-test-token".to_owned(),
            protocol: ModelProtocol::Responses,
            base_url: Some("https://gateway.example.com".to_owned()),
            display_name: "OpenAI gateway".to_owned(),
            id: "openai-gateway".to_owned(),
            model_id: "gpt-5.4-mini".to_owned(),
            provider_id: "openai".to_owned(),
            model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
        }],
    });

    let error = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: None,
            base_url: Some("https://attacker.example.com".to_owned()),
            config_id: Some("openai-gateway".to_owned()),
            display_name: Some("OpenAI gateway".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            provider_id: "openai".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("apiKey is required"));
}

#[tokio::test]
async fn save_provider_settings_payload_rejects_http_base_url_with_loopback_prefix_domain() {
    let store = RecordingProviderSettingsStore::default();
    let error = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: Some("http://127.attacker.example".to_owned()),
            config_id: None,
            display_name: Some("OpenAI gateway".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            provider_id: "openai".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error
        .message
        .contains("baseUrl must use https:// unless it targets localhost"));
}

#[tokio::test]
async fn save_provider_settings_payload_accepts_http_loopback_base_url() {
    let store = RecordingProviderSettingsStore::default();
    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: Some("http://127.0.0.1:11434/v1".to_owned()),
            config_id: None,
            display_name: Some("OpenAI gateway".to_owned()),
            model_id: "gpt-5.4-mini".to_owned(),
            provider_id: "openai".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap();

    assert_eq!(
        payload.config.base_url.as_deref(),
        Some("http://127.0.0.1:11434/v1")
    );
}

#[tokio::test]
async fn save_provider_settings_payload_does_not_save_record_when_record_write_fails() {
    let store = RecordingProviderSettingsStore {
        fail_record: true,
        ..RecordingProviderSettingsStore::default()
    };
    let error = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: None,
            config_id: None,
            display_name: None,
            model_id: "gpt-5.4-mini".to_owned(),
            provider_id: "openai".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
}

#[tokio::test]
async fn set_conversation_model_config_with_runtime_state_persists_selection() {
    let workspace = unique_workspace("conversation-model-config");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let provider_store = DesktopProviderSettingsStore::new(workspace.clone());
    provider_store
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai-work".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI Work".to_owned(),
                id: "openai-work".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace.clone()).await;
    let session_id = SessionId::new();
    open_conversation_session(&state, session_id).await;
    let conversation_id = session_id.to_string();

    let payload = set_conversation_model_config_with_runtime_state(
        SetConversationModelConfigRequest {
            conversation_id: conversation_id.clone(),
            model_config_id: "openai-work".to_owned(),
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(payload.conversation_id, conversation_id);
    assert_eq!(payload.model_config_id, "openai-work");
    assert_eq!(payload.status, "saved");
    let saved: HashMap<String, String> = serde_json::from_slice(
        &std::fs::read(
            workspace
                .join(".jyowo")
                .join("runtime")
                .join("conversation-model-settings.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        saved.get(&payload.conversation_id).map(String::as_str),
        Some("openai-work")
    );
}

#[tokio::test]
async fn set_conversation_model_config_with_runtime_state_rejects_unknown_conversation_id() {
    let workspace = unique_workspace("conversation-model-config-unknown");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    DesktopProviderSettingsStore::new(workspace.clone())
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai-work".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI Work".to_owned(),
                id: "openai-work".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace.clone()).await;
    let unknown_conversation_id = SessionId::new().to_string();

    let error = set_conversation_model_config_with_runtime_state(
        SetConversationModelConfigRequest {
            conversation_id: unknown_conversation_id.clone(),
            model_config_id: "openai-work".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "NOT_FOUND");
    assert!(error.message.contains(&unknown_conversation_id));
    assert!(!workspace
        .join(".jyowo")
        .join("runtime")
        .join("conversation-model-settings.json")
        .exists());
}

#[tokio::test]
async fn provider_settings_payload_rejects_invalid_provider_model_and_key() {
    let store = RecordingProviderSettingsStore::default();
    let invalid_provider = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some("provider-test-token".to_owned()),
            base_url: None,
            config_id: None,
            display_name: None,
            model_id: "gpt-5.4-mini".to_owned(),
            provider_id: "unknown".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(invalid_provider.code, "INVALID_PAYLOAD");

    let invalid_model = validate_provider_settings_payload(ValidateProviderSettingsRequest {
        model_id: "not-a-real-model".to_owned(),
        provider_id: "openai".to_owned(),
    })
    .await
    .unwrap_err();

    assert_eq!(invalid_model.code, "INVALID_PAYLOAD");

    let invalid_key = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: Some(String::new()),
            base_url: None,
            config_id: None,
            display_name: None,
            model_id: "gpt-5.4-mini".to_owned(),
            provider_id: "openai".to_owned(),
            set_default: true,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(invalid_key.code, "INVALID_PAYLOAD");

    let invalid_metadata = validate_provider_settings_payload(ValidateProviderSettingsRequest {
        model_id: String::new(),
        provider_id: "openai".to_owned(),
    })
    .await
    .unwrap_err();

    assert_eq!(invalid_metadata.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn validate_provider_settings_payload_does_not_require_api_key() {
    let payload = validate_provider_settings_payload(ValidateProviderSettingsRequest {
        model_id: "gpt-5.4-mini".to_owned(),
        provider_id: "openai".to_owned(),
    })
    .await
    .unwrap();

    assert_eq!(payload.status, "accepted");
}

#[tokio::test]
async fn save_mcp_server_payload_rejects_invalid_config_fail_closed() {
    let store = RecordingMcpServerStore::default();
    let error = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            display_name: String::new(),
            id: "bad id".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: String::new(),
                args: Vec::new(),
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(store.record.lock().unwrap().is_none());
}

#[test]
fn mcp_server_config_record_defaults_legacy_stdio_records_to_enabled() {
    let record = serde_json::from_value::<McpServerConfigRecord>(json!({
        "displayName": "Workspace GitHub",
        "id": "github",
        "scope": "global",
        "transport": {
            "kind": "stdio",
            "command": "node",
            "args": ["server.js"]
        }
    }))
    .unwrap();

    assert!(record.enabled);
    assert_eq!(record.display_name, "Workspace GitHub");
}

#[tokio::test]
async fn save_mcp_server_payload_rejects_secret_bearing_stdio_args() {
    let store = RecordingMcpServerStore::default();
    let error = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Workspace GitHub".to_owned(),
            id: "github".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "node".to_owned(),
                args: vec!["--token=mcp-secret-token".to_owned()],
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(store.record.lock().unwrap().is_none());
}

#[tokio::test]
async fn save_mcp_server_payload_rejects_raw_secret_like_stdio_args() {
    let store = RecordingMcpServerStore::default();
    let error = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Workspace GitHub".to_owned(),
            id: "github".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "node".to_owned(),
                args: vec!["ghp_abcdefghijklmnopqrstuvwxyz0123456789".to_owned()],
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(store.record.lock().unwrap().is_none());
}

#[tokio::test]
async fn save_mcp_server_payload_rejects_in_process_workspace_config() {
    let store = RecordingMcpServerStore::default();
    let error = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Internal".to_owned(),
            id: "internal".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::InProcess,
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(store.record.lock().unwrap().is_none());
}

#[tokio::test]
async fn save_mcp_server_payload_persists_http_config_without_secret_values() {
    let store = RecordingMcpServerStore::default();
    let payload = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Remote Context".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Http {
                url: "https://mcp.example.com/mcp".to_owned(),
                bearer_token_env_var: Some("MCP_BEARER_TOKEN".to_owned()),
                headers: vec![McpNameValueRecord {
                    key: "X-Workspace".to_owned(),
                    value: "jyowo".to_owned(),
                }],
                headers_from_env: vec![McpHeaderEnvRecord {
                    key: "X-Api-Key".to_owned(),
                    env_var: "MCP_CONTEXT7_TOKEN".to_owned(),
                }],
            },
        },
        &store,
    )
    .await
    .unwrap();
    let stored = store.record.lock().unwrap().clone().unwrap();

    assert!(payload.server.enabled);
    assert!(payload.server.manageable);
    assert_eq!(payload.server.transport, "http");
    assert_eq!(stored.enabled, true);
    assert_eq!(
        serde_json::to_string(&stored).unwrap().contains("secret"),
        false
    );
}

#[tokio::test]
async fn get_mcp_server_config_with_store_returns_workspace_managed_record() {
    let store = RecordingMcpServerStore::default();
    save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Remote Context".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Http {
                url: "https://mcp.example.com/mcp".to_owned(),
                bearer_token_env_var: Some("MCP_BEARER_TOKEN".to_owned()),
                headers: vec![McpNameValueRecord {
                    key: "X-Workspace".to_owned(),
                    value: "jyowo".to_owned(),
                }],
                headers_from_env: vec![McpHeaderEnvRecord {
                    key: "X-Api-Key".to_owned(),
                    env_var: "MCP_CONTEXT7_TOKEN".to_owned(),
                }],
            },
        },
        &store,
    )
    .await
    .unwrap();

    let payload = get_mcp_server_config_with_store(
        GetMcpServerConfigRequest {
            id: "context7".to_owned(),
        },
        &store,
    )
    .await
    .unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();

    assert_eq!(payload.server.display_name, "Remote Context");
    assert_eq!(payload.server.id, "context7");
    assert!(matches!(
        payload.server.transport,
        McpServerTransportConfig::Http { .. }
    ));
    assert!(serialized.contains("MCP_BEARER_TOKEN"));
    assert!(!serialized.contains("mcp-secret-token"));
}

#[tokio::test]
async fn get_mcp_server_config_with_runtime_state_rejects_unmanaged_runtime_server() {
    let server_id = McpServerId("plugin-context".to_owned());
    let mcp_registry = McpRegistry::new();
    mcp_registry
        .add_ready_server(
            McpServerSpec::new(
                server_id.clone(),
                "Plugin Context",
                TransportChoice::InProcess,
                McpServerSource::Plugin(harness_contracts::PluginId("context".to_owned())),
            ),
            McpServerScope::Global,
            Arc::new(StaticMcpConnection { tools: Vec::new() }),
        )
        .await
        .unwrap();
    let state = runtime_state_with_mcp_registry(mcp_registry, vec![server_id]).await;

    let error = get_mcp_server_config_with_runtime_state(
        GetMcpServerConfigRequest {
            id: "plugin-context".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "NOT_FOUND");
}

#[tokio::test]
async fn save_mcp_server_payload_rejects_secret_bearing_http_headers() {
    let store = RecordingMcpServerStore::default();
    let error = save_mcp_server_with_store(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Remote Context".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Http {
                url: "https://mcp.example.com/mcp".to_owned(),
                bearer_token_env_var: None,
                headers: vec![McpNameValueRecord {
                    key: "Authorization".to_owned(),
                    value: "Bearer mcp-secret-token".to_owned(),
                }],
                headers_from_env: Vec::new(),
            },
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(store.record.lock().unwrap().is_none());
}

#[test]
fn save_mcp_server_payload_rejects_unknown_transport_fields() {
    let error = serde_json::from_value::<SaveMcpServerRequest>(json!({
        "enabled": true,
        "displayName": "Workspace GitHub",
        "id": "github",
        "scope": "global",
        "transport": {
            "kind": "stdio",
            "command": "node",
            "args": [],
            "envMap": { "GITHUB_TOKEN": "secret" }
        }
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown field"));
}

#[tokio::test(flavor = "current_thread")]
async fn save_mcp_server_with_runtime_state_registers_and_injects_stdio_tools() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("mcp-save-registers");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;

    let payload = save_mcp_server_with_runtime_state(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Workspace Stdio".to_owned(),
            id: "stdio".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "/bin/sh".to_owned(),
                args: vec!["-c".to_owned(), stdio_mcp_fixture_script()],
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &state,
    )
    .await
    .unwrap();
    let harness = state.harness().unwrap();

    assert_eq!(payload.server.status, "ready");
    assert_eq!(payload.server.exposed_tool_count, 1);
    assert!(harness.tool_registry().get("mcp__stdio__echo").is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_mcp_server_with_runtime_state_does_not_register_or_inject_tools() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("mcp-disabled");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;

    let payload = save_mcp_server_with_runtime_state(
        SaveMcpServerRequest {
            enabled: false,
            display_name: "Workspace Stdio".to_owned(),
            id: "stdio".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "/bin/sh".to_owned(),
                args: vec!["-c".to_owned(), stdio_mcp_fixture_script()],
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &state,
    )
    .await
    .unwrap();
    let harness = state.harness().unwrap();

    assert_eq!(payload.server.status, "disabled");
    assert!(!payload.server.enabled);
    assert!(harness
        .mcp_config()
        .unwrap()
        .registry
        .server_spec(&McpServerId("stdio".to_owned()))
        .await
        .is_none());
    assert!(harness.tool_registry().get("mcp__stdio__echo").is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn set_mcp_server_enabled_registers_and_injects_tools() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("mcp-enable");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;
    save_mcp_server_with_runtime_state(
        SaveMcpServerRequest {
            enabled: false,
            display_name: "Workspace Stdio".to_owned(),
            id: "stdio".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "/bin/sh".to_owned(),
                args: vec!["-c".to_owned(), stdio_mcp_fixture_script()],
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &state,
    )
    .await
    .unwrap();

    let payload = set_mcp_server_enabled_with_runtime_state(
        SetMcpServerEnabledRequest {
            id: "stdio".to_owned(),
            enabled: true,
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(payload.server.status, "ready");
    assert!(payload.server.enabled);
    assert!(state
        .harness()
        .unwrap()
        .tool_registry()
        .get("mcp__stdio__echo")
        .is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn restart_mcp_server_removes_registers_and_injects_tools() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("mcp-restart");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;
    save_mcp_server_with_runtime_state(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Workspace Stdio".to_owned(),
            id: "stdio".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "/bin/sh".to_owned(),
                args: vec!["-c".to_owned(), stdio_mcp_fixture_script()],
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &state,
    )
    .await
    .unwrap();

    let payload = restart_mcp_server_with_runtime_state(
        RestartMcpServerRequest {
            id: "stdio".to_owned(),
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(payload.server.status, "ready");
    assert!(state
        .harness()
        .unwrap()
        .tool_registry()
        .get("mcp__stdio__echo")
        .is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn http_mcp_server_with_runtime_state_registers_as_http_transport() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let _env = EnvVarGuard::set(
        "MCP_TEST_BEARER",
        std::ffi::OsStr::new("not-secret-test-token"),
    );
    let workspace = unique_workspace("mcp-http-register");
    std::fs::create_dir_all(&workspace).unwrap();
    let state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;

    let payload = save_mcp_server_with_runtime_state(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Remote Context".to_owned(),
            id: "context7".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Http {
                url: "http://127.0.0.1:9/mcp".to_owned(),
                bearer_token_env_var: Some("MCP_TEST_BEARER".to_owned()),
                headers: vec![McpNameValueRecord {
                    key: "X-Workspace".to_owned(),
                    value: "jyowo".to_owned(),
                }],
                headers_from_env: Vec::new(),
            },
        },
        &state,
    )
    .await
    .unwrap();
    let spec = state
        .harness()
        .unwrap()
        .mcp_config()
        .unwrap()
        .registry
        .server_spec(&McpServerId("context7".to_owned()))
        .await
        .unwrap();

    assert_eq!(payload.server.transport, "http");
    assert!(matches!(spec.transport, TransportChoice::Http { .. }));
}

#[tokio::test]
async fn delete_mcp_server_payload_is_idempotent_for_missing_server() {
    let store = RecordingMcpServerStore::default();
    let payload = delete_mcp_server_with_store(
        DeleteMcpServerRequest {
            id: "github".to_owned(),
        },
        &store,
    )
    .await
    .unwrap();

    assert_eq!(payload.id, "github");
    assert_eq!(payload.status, "deleted");
}

#[tokio::test(flavor = "current_thread")]
async fn delete_mcp_server_with_runtime_state_removes_registry_server_and_injected_tools() {
    let _guard = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace = unique_workspace("mcp-delete-runtime");
    std::fs::create_dir_all(&workspace).unwrap();
    let server_id = McpServerId("github".to_owned());
    let mcp_registry = McpRegistry::new();
    mcp_registry
        .add_ready_server(
            McpServerSpec::new(
                server_id.clone(),
                "Workspace GitHub",
                TransportChoice::InProcess,
                McpServerSource::Workspace,
            ),
            McpServerScope::Global,
            Arc::new(StaticMcpConnection {
                tools: vec![McpToolDescriptor {
                    name: "search".to_owned(),
                    description: Some("Search".to_owned()),
                    input_schema: json!({ "type": "object" }),
                    output_schema: None,
                    annotations: None,
                    meta: Default::default(),
                }],
            }),
        )
        .await
        .unwrap();
    let state = runtime_state_with_mcp_registry_for_workspace(
        workspace,
        mcp_registry,
        vec![server_id.clone()],
    )
    .await;
    let harness = state.harness().unwrap();
    harness
        .mcp_config()
        .unwrap()
        .registry
        .inject_tools_into(harness.tool_registry(), &server_id)
        .await
        .unwrap();
    assert!(harness.tool_registry().get("mcp__github__search").is_some());

    let payload = delete_mcp_server_with_runtime_state(
        DeleteMcpServerRequest {
            id: "github".to_owned(),
        },
        &state,
    )
    .await
    .unwrap();
    let servers = list_mcp_servers_with_runtime_state(&state).await.unwrap();

    assert_eq!(payload.status, "deleted");
    assert!(servers.servers.is_empty());
    assert!(harness.tool_registry().get("mcp__github__search").is_none());
}

#[tokio::test]
async fn list_mcp_servers_with_runtime_state_includes_origin_scope_and_tool_count() {
    let server_id = McpServerId("github".to_owned());
    let mcp_registry = McpRegistry::new();
    mcp_registry
        .add_ready_server(
            McpServerSpec::new(
                server_id.clone(),
                "Workspace GitHub",
                TransportChoice::InProcess,
                McpServerSource::Workspace,
            ),
            McpServerScope::Global,
            Arc::new(StaticMcpConnection {
                tools: vec![
                    McpToolDescriptor {
                        name: "search".to_owned(),
                        description: Some("Search".to_owned()),
                        input_schema: json!({ "type": "object" }),
                        output_schema: None,
                        annotations: None,
                        meta: Default::default(),
                    },
                    McpToolDescriptor {
                        name: "issue".to_owned(),
                        description: Some("Issue".to_owned()),
                        input_schema: json!({ "type": "object" }),
                        output_schema: None,
                        annotations: None,
                        meta: Default::default(),
                    },
                ],
            }),
        )
        .await
        .unwrap();
    let tool_registry = ToolRegistry::builder().build().unwrap();
    mcp_registry
        .inject_tools_into(&tool_registry, &server_id)
        .await
        .unwrap();
    let state = runtime_state_with_mcp_registry(mcp_registry, vec![server_id]).await;
    let payload = list_mcp_servers_with_runtime_state(&state).await.unwrap();
    let value = serde_json::to_value(payload).unwrap();

    assert_eq!(
        value,
        json!({
            "servers": [
                {
                    "displayName": "Workspace GitHub",
                    "enabled": true,
                    "exposedToolCount": 2,
                    "id": "github",
                    "manageable": false,
                    "origin": "workspace",
                    "scope": "global",
                    "status": "ready",
                    "transport": "inProcess"
                }
            ]
        })
    );
}

#[test]
fn mcp_diagnostic_event_summary_does_not_expose_raw_connection_error() {
    let diagnostic =
        mcp_diagnostic_record_from_event(Event::McpConnectionLost(McpConnectionLostEvent {
            session_id: None,
            server_id: McpServerId("github".to_owned()),
            server_source: McpServerSource::Workspace,
            reason: McpConnectionLostReason::Network(
                "Authorization: Bearer mcp-secret-token".to_owned(),
            ),
            attempts_so_far: 1,
            terminal: false,
            at: now(),
        }))
        .unwrap();

    assert_eq!(diagnostic.server_id, "github");
    assert_eq!(diagnostic.severity, McpDiagnosticSeverity::Warning);
    assert_eq!(
        diagnostic.summary,
        "MCP server connection lost; reconnecting."
    );
    assert!(!serde_json::to_string(&diagnostic)
        .unwrap()
        .contains("mcp-secret-token"));
}

#[tokio::test]
async fn mcp_diagnostic_store_retains_recent_records_and_filters_by_server() {
    let workspace = unique_workspace("mcp-diagnostics");
    std::fs::create_dir_all(&workspace).unwrap();
    let store = DesktopMcpDiagnosticStore::new_with_limit(workspace, 3);

    for index in 0..5 {
        store
            .append_record(&McpDiagnosticRecord {
                event_type: "connection_lost".to_owned(),
                id: format!("event-{index}"),
                server_id: if index == 4 { "fetch" } else { "github" }.to_owned(),
                severity: McpDiagnosticSeverity::Warning,
                summary: format!("diagnostic {index}"),
                timestamp: format!("2026-06-17T00:00:0{index}.000Z"),
            })
            .unwrap();
    }

    let all = list_mcp_diagnostics_with_store(None, &store).await.unwrap();
    let github = list_mcp_diagnostics_with_store(Some("github".to_owned()), &store)
        .await
        .unwrap();

    assert_eq!(
        all.events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        vec!["event-2", "event-3", "event-4"]
    );
    assert_eq!(
        github
            .events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        vec!["event-2", "event-3"]
    );
}

#[tokio::test]
async fn memory_commands_list_inspect_update_delete_and_export_visible_items() {
    let provider = Arc::new(MockMemoryProvider::new("mock"));
    let state = runtime_state_with_memory_provider(provider.clone()).await;
    let session_id = state.default_conversation_id();
    let visible = test_memory_record(session_id, "Prefers concise Chinese responses");
    provider.upsert(visible.clone()).await.unwrap();
    provider
        .upsert(test_memory_record(
            SessionId::new(),
            "Hidden session memory",
        ))
        .await
        .unwrap();

    let list = list_memory_items_with_runtime_state(&state).await.unwrap();

    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].id, visible.id.to_string());
    assert_eq!(list.items[0].visibility, "private");
    assert_eq!(list.items[0].kind, "user_preference");

    let detail = get_memory_item_with_runtime_state(
        GetMemoryItemRequest {
            id: visible.id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(detail.item.content, "Prefers concise Chinese responses");

    let updated = update_memory_item_with_runtime_state(
        UpdateMemoryItemRequest {
            content: "Prefers terse Chinese responses".to_owned(),
            id: visible.id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(updated.item.content, "Prefers terse Chinese responses");

    let exported = export_memory_items_with_runtime_state(&state)
        .await
        .unwrap();
    assert_eq!(exported.format, "json");
    assert_eq!(exported.item_count, 1);
    assert!(exported.path.starts_with(".jyowo/runtime/exports/memory-"));
    let export_content = std::fs::read_to_string(state.workspace_root().join(&exported.path))
        .expect("memory export file should be readable");
    assert!(export_content.contains("Prefers terse Chinese responses"));

    let deleted = delete_memory_item_with_runtime_state(
        DeleteMemoryItemRequest {
            id: visible.id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(deleted.status, "deleted");

    let list_after_delete = list_memory_items_with_runtime_state(&state).await.unwrap();
    assert!(list_after_delete.items.is_empty());
}

#[tokio::test]
async fn list_conversations_with_runtime_state_returns_startable_conversation_id() {
    let state = runtime_state_with_harness().await;
    let payload = list_conversations_with_runtime_state(&state).await;
    let conversation_id = payload.conversations[0].id.clone();

    let session_id =
        SessionId::parse(&conversation_id).expect("conversation id should be a session id");
    assert_eq!(session_id.to_string(), conversation_id);

    let run = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id,
            prompt: "Continue implementation".to_owned(),
        },
        &state,
    )
    .await
    .expect("listed conversation should be startable");

    assert_eq!(run.status, "started");
    assert_eq!(
        RunId::parse(&run.run_id)
            .expect("run id should be canonical")
            .to_string(),
        run.run_id
    );
}

#[tokio::test]
async fn create_conversation_with_runtime_state_persists_empty_runtime_session() {
    let state = runtime_state_with_harness().await;

    let created = create_conversation_with_runtime_state(&state)
        .await
        .expect("create conversation should create a runtime session");
    let conversation_id = created.conversation.id.clone();
    assert!(created.conversation.is_empty);
    SessionId::parse(&conversation_id).expect("conversation id should be a session id");

    let listed = list_conversations_with_runtime_state(&state).await;
    assert!(listed
        .conversations
        .iter()
        .any(|conversation| conversation.id == conversation_id));

    let detail = get_conversation_with_runtime_state(
        GetConversationRequest {
            conversation_id: conversation_id.clone(),
        },
        &state,
    )
    .await
    .expect("created empty conversation should be readable");

    assert_eq!(detail.conversation.id, conversation_id);
    assert!(detail.conversation.messages.is_empty());
}

#[tokio::test]
async fn create_conversation_with_runtime_state_does_not_bind_default_model_config() {
    let workspace = unique_workspace("create-conversation-default-model");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    DesktopProviderSettingsStore::new(workspace.clone())
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai-work".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI Work".to_owned(),
                id: "openai-work".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;

    let created = create_conversation_with_runtime_state(&state)
        .await
        .expect("create conversation should create a runtime session");
    let detail = get_conversation_with_runtime_state(
        GetConversationRequest {
            conversation_id: created.conversation.id,
        },
        &state,
    )
    .await
    .expect("created conversation should be readable");

    assert_eq!(detail.conversation.model_config_id, None);
}

#[test]
fn desktop_provider_settings_store_rejects_config_without_api_key() {
    let workspace = unique_workspace("conversation-model-no-key");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let error = DesktopProviderSettingsStore::new(workspace)
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai-work".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: String::new(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI Work".to_owned(),
                id: "openai-work".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap_err();

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert!(error.message.contains("apiKey is required"));
}

#[tokio::test]
async fn set_conversation_model_config_with_runtime_state_allows_cross_provider_known_models() {
    let workspace = unique_workspace("conversation-cross-provider-model");
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let state = runtime_state_for_workspace(workspace.clone())
        .await
        .expect("runtime should start with local llama fallback");
    let created = create_conversation_with_runtime_state(&state)
        .await
        .expect("conversation should be created with fallback runtime");
    DesktopProviderSettingsStore::new(workspace)
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai-work".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: "provider-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI Work".to_owned(),
                id: "openai-work".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap();

    let saved = set_conversation_model_config_with_runtime_state(
        SetConversationModelConfigRequest {
            conversation_id: created.conversation.id.clone(),
            model_config_id: "openai-work".to_owned(),
        },
        &state,
    )
    .await
    .expect("known provider model switch should open the existing session");

    assert_eq!(saved.conversation_id, created.conversation.id);
    assert_eq!(saved.model_config_id, "openai-work");
}

#[tokio::test]
async fn list_conversations_with_runtime_state_returns_empty_list_without_harness() {
    let workspace = unique_workspace("no-harness");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("workspace state should initialize without a harness");
    let payload = list_conversations_with_runtime_state(&state).await;

    assert!(payload.conversations.is_empty());
}

#[tokio::test]
async fn list_conversations_with_runtime_state_opens_listed_empty_conversation() {
    let state = runtime_state_with_harness().await;
    let payload = list_conversations_with_runtime_state(&state).await;
    let conversation_id = payload.conversations[0].id.clone();

    let detail = get_conversation_with_runtime_state(
        GetConversationRequest {
            conversation_id: conversation_id.clone(),
        },
        &state,
    )
    .await
    .expect("listed empty conversation should be readable");

    assert_eq!(detail.conversation.id, conversation_id);
    assert!(detail.conversation.messages.is_empty());
    assert_eq!(detail.conversation.title, "New conversation");
    assert!(payload.conversations[0].is_empty);
    let serialized = serde_json::to_value(&payload).expect("payload should serialize");
    assert_eq!(
        serialized["conversations"][0].get("lastMessagePreview"),
        None,
        "empty conversation preview should be omitted instead of serialized as null",
    );
}

#[tokio::test]
async fn delete_conversation_with_runtime_state_removes_session_from_runtime_list() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Deleted conversation should not return".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let conversation_id = state.default_conversation_id().to_string();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: conversation_id.clone(),
            prompt: "Create a conversation".to_owned(),
        },
        &state,
    )
    .await
    .expect("conversation should be created before deletion");

    let deleted = delete_conversation_with_runtime_state(
        DeleteConversationRequest {
            conversation_id: conversation_id.clone(),
        },
        &state,
    )
    .await
    .expect("conversation deletion should succeed");

    assert_eq!(deleted.conversation_id, conversation_id);
    assert_eq!(deleted.status, "deleted");

    let payload = list_conversations_with_runtime_state(&state).await;
    assert!(!payload
        .conversations
        .iter()
        .any(|conversation| conversation.id == conversation_id));

    let detail_error = get_conversation_with_runtime_state(
        GetConversationRequest {
            conversation_id: conversation_id.clone(),
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(detail_error.code, "NOT_FOUND");

    let restart_error = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id,
            prompt: "Do not recreate a deleted conversation".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();
    assert_eq!(restart_error.code, "NOT_FOUND");
}

#[tokio::test]
async fn get_and_delete_conversation_with_runtime_state_survive_runtime_option_changes() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Readable after runtime option change".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let conversation_id = state.default_conversation_id().to_string();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: conversation_id.clone(),
            prompt: "Create a conversation before changing runtime options".to_owned(),
        },
        &state,
    )
    .await
    .expect("conversation should be created before runtime option changes");

    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    state.replace_harness(harness, "mock-model".to_owned(), ModelProtocol::Responses);

    let detail = get_conversation_with_runtime_state(
        GetConversationRequest {
            conversation_id: conversation_id.clone(),
        },
        &state,
    )
    .await
    .expect("conversation reads should survive runtime option changes");
    assert!(detail.conversation.messages.iter().any(|message| message
        .body
        .contains("Readable after runtime option change")));

    let deleted = delete_conversation_with_runtime_state(
        DeleteConversationRequest {
            conversation_id: conversation_id.clone(),
        },
        &state,
    )
    .await
    .expect("conversation delete should survive runtime option changes");
    assert_eq!(deleted.conversation_id, conversation_id);
    assert_eq!(deleted.status, "deleted");
}

#[tokio::test]
async fn listed_empty_conversation_returns_empty_activity() {
    let state = runtime_state_with_harness().await;
    let payload = list_conversations_with_runtime_state(&state).await;
    let conversation_id = payload.conversations[0].id.clone();

    let activity = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(conversation_id),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("listed empty conversation activity should be readable");

    assert!(activity.events.is_empty());
}

#[tokio::test]
async fn listed_empty_conversation_returns_workspace_context() {
    let state = runtime_state_with_harness().await;
    let payload = list_conversations_with_runtime_state(&state).await;
    let conversation_id = payload.conversations[0].id.clone();

    let context = get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id: Some(conversation_id),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("listed empty conversation context should be readable");

    assert!(!context.project.is_empty());
    assert!(context.active_artifact.is_none());
}

#[tokio::test]
async fn get_conversation_with_runtime_state_returns_runtime_messages() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Ready".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Tell me status".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

    loop {
        let payload = get_conversation_with_runtime_state(
            GetConversationRequest {
                conversation_id: session_id.to_string(),
            },
            &state,
        )
        .await
        .unwrap();

        if payload.conversation.messages.len() >= 2 {
            assert_eq!(payload.conversation.messages[0].author, "user");
            assert_eq!(payload.conversation.messages[0].body, "Tell me status");
            assert_eq!(payload.conversation.messages[1].author, "assistant");
            assert!(payload.conversation.messages[1].body.contains("Ready"));
            assert!(!payload.conversation.updated_at.is_empty());
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("conversation detail should include runtime messages");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn list_conversations_with_runtime_state_projects_runtime_summary() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Ready from runtime".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Tell me status\nwith details".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let payload = list_conversations_with_runtime_state(&state).await;
        let Some(summary) = payload
            .conversations
            .iter()
            .find(|conversation| conversation.id == session_id.to_string())
        else {
            if tokio::time::Instant::now() >= deadline {
                panic!("started session should be listed");
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
            continue;
        };

        if summary.last_message_preview.as_deref() == Some("Ready from runtime") {
            assert!(!summary.is_empty);
            assert_eq!(summary.title, "Tell me status");
            assert_ne!(summary.updated_at, "2026-06-17T00:00:00.000Z");
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("conversation summary should include runtime message projection");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn conversation_payloads_with_runtime_state_redact_private_paths() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Read /home/goya/.ssh/config".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Read /Users/goya/.ssh/config".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let detail = get_conversation_with_runtime_state(
            GetConversationRequest {
                conversation_id: session_id.to_string(),
            },
            &state,
        )
        .await
        .unwrap();

        if detail.conversation.messages.len() >= 2 {
            assert_eq!(detail.conversation.messages[0].body, "Read [REDACTED]");
            assert_eq!(detail.conversation.messages[1].body, "Read [REDACTED]");

            let list = list_conversations_with_runtime_state(&state).await;
            let Some(summary) = list
                .conversations
                .iter()
                .find(|conversation| conversation.id == session_id.to_string())
            else {
                if tokio::time::Instant::now() >= deadline {
                    panic!("started session should be listed");
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
                continue;
            };
            assert_eq!(summary.title, "Read [REDACTED]");
            assert_eq!(
                summary.last_message_preview.as_deref(),
                Some("Read [REDACTED]")
            );
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("conversation payloads should include redacted runtime messages");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_ignores_assistant_outputs() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text(
                "# Runtime artifact\n\nGenerated from the conversation.".to_owned(),
            ),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = state.default_conversation_id();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Create an artifact".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    loop {
        let conversation = get_conversation_with_runtime_state(
            GetConversationRequest {
                conversation_id: session_id.to_string(),
            },
            &state,
        )
        .await
        .expect("runtime conversation should load");
        if conversation
            .conversation
            .messages
            .iter()
            .any(|message| message.body.contains("Runtime artifact"))
        {
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("runtime assistant output should complete");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    let payload = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: state.default_conversation_id().to_string(),
        },
        &state,
    )
    .await
    .expect("runtime artifact projection should load");

    assert!(payload.artifacts.is_empty());
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_projects_artifact_events() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Created a durable artifact.".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = state.default_conversation_id();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Create an artifact".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    let run_id = loop {
        let conversation = get_conversation_with_runtime_state(
            GetConversationRequest {
                conversation_id: session_id.to_string(),
            },
            &state,
        )
        .await
        .expect("runtime conversation should load");
        if conversation
            .conversation
            .messages
            .iter()
            .any(|message| message.body.contains("Created a durable artifact"))
        {
            let activity = list_activity_with_runtime_state(
                ListActivityRequest {
                    conversation_id: Some(session_id.to_string()),
                    run_id: None,
                },
                &state,
            )
            .await
            .expect("activity should load");
            let run_id = activity
                .events
                .iter()
                .find(|event| event.event_type == "run.started")
                .map(|event| event.run_id.clone())
                .expect("run id should be visible in activity");
            break RunId::parse(&run_id).expect("run id should be canonical");
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("runtime assistant output should complete");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    };

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                artifact_id: "artifact-runtime-notes".to_owned(),
                at: now(),
                blob_ref: None,
                content_hash: None,
                kind: "markdown".to_owned(),
                preview: Some("# Runtime artifact\n\nGenerated as a durable result.".to_owned()),
                run_id,
                session_id,
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                status: ArtifactStatus::Ready,
                title: "Runtime artifact".to_owned(),
            })],
        )
        .await
        .expect("artifact event should append");

    let payload = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("runtime artifact projection should load");

    let artifact = payload
        .artifacts
        .first()
        .expect("artifact event should project");
    assert_eq!(artifact.id, "artifact-runtime-notes");
    assert_eq!(artifact.kind, "markdown");
    assert_eq!(artifact.status, "ready");
    assert_eq!(artifact.title, "Runtime artifact");
    assert!(artifact
        .preview
        .as_deref()
        .unwrap_or_default()
        .contains("Runtime artifact"));
    assert_eq!(artifact.source_message_id, None);
    assert_eq!(artifact.source_run_id, run_id.to_string());
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_returns_owned_image_data_url() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    let run_id = RunId::new();
    let image_bytes = b"\x89PNG\r\n\x1A\npreview".to_vec();
    let content_hash = *blake3::hash(&image_bytes).as_bytes();
    let blob_store = FileBlobStore::open(
        state
            .workspace_root()
            .join(".jyowo")
            .join("runtime")
            .join("blobs"),
    )
    .expect("blob store opens");
    let blob_ref = blob_store
        .put(
            TenantId::SINGLE,
            bytes::Bytes::from(image_bytes.clone()),
            BlobMeta {
                content_type: Some("image/png".to_owned()),
                size: image_bytes.len() as u64,
                content_hash,
                created_at: now(),
                retention: BlobRetention::SessionScoped(session_id),
            },
        )
        .await
        .expect("image blob writes");

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                artifact_id: "artifact-image".to_owned(),
                at: now(),
                blob_ref: Some(blob_ref),
                content_hash: Some(content_hash.to_vec()),
                kind: "image".to_owned(),
                preview: Some("生成的图片".to_owned()),
                run_id,
                session_id,
                source: ArtifactSource::Tool,
                source_message_id: None,
                source_tool_use_id: Some(ToolUseId::new()),
                status: ArtifactStatus::Ready,
                title: "生成的图片".to_owned(),
            })],
        )
        .await
        .expect("artifact event should append");

    let payload = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-image".to_owned(),
        },
        &state,
    )
    .await
    .expect("image preview should load");

    assert_eq!(payload.mime_type, "image/png");
    assert_eq!(payload.size_bytes, image_bytes.len() as u64);
    assert!(payload.data_url.starts_with("data:image/png;base64,"));
    assert!(!payload.data_url.contains(".jyowo"));
    assert!(!payload.data_url.contains("artifact-image"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_accepts_image_mime_artifact_kind() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-image-mime-kind",
        "image/png",
        ArtifactStatus::Ready,
        Some((
            "image/png",
            b"\x89PNG\r\n\x1A\npreview".to_vec(),
            session_id,
        )),
    )
    .await;

    let payload = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-image-mime-kind".to_owned(),
        },
        &state,
    )
    .await
    .expect("image MIME kind artifact should preview");

    assert_eq!(payload.mime_type, "image/png");
    assert!(payload.data_url.starts_with("data:image/png;base64,"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_falls_back_to_detected_image_mime() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-image-unsafe-mime",
        "image",
        ArtifactStatus::Ready,
        Some((
            "image/png /tmp/provider-output https://provider.example/blob",
            b"\x89PNG\r\n\x1A\npreview".to_vec(),
            session_id,
        )),
    )
    .await;

    let payload = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-image-unsafe-mime".to_owned(),
        },
        &state,
    )
    .await
    .expect("valid image bytes should preview without trusting unsafe MIME");

    assert_eq!(payload.mime_type, "image/png");
    assert!(payload.data_url.starts_with("data:image/png;base64,"));
    assert!(!payload.data_url.contains("/tmp/provider-output"));
    assert!(!payload.data_url.contains("provider.example"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_safe_non_image_mime() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-image-text-mime",
        "image",
        ArtifactStatus::Ready,
        Some((
            "text/plain",
            b"\x89PNG\r\n\x1A\npreview".to_vec(),
            session_id,
        )),
    )
    .await;

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-image-text-mime".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("safe non-image declared MIME should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("only available for images"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_cross_session_blob() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    let other_session_id = SessionId::new();
    open_conversation_session(&state, session_id).await;
    open_conversation_session(&state, other_session_id).await;
    let run_id = RunId::new();
    let image_bytes = b"\x89PNG\r\n\x1A\npreview".to_vec();
    let image_size = image_bytes.len() as u64;
    let content_hash = *blake3::hash(&image_bytes).as_bytes();
    let blob_store = FileBlobStore::open(
        state
            .workspace_root()
            .join(".jyowo")
            .join("runtime")
            .join("blobs"),
    )
    .expect("blob store opens");
    let blob_ref = blob_store
        .put(
            TenantId::SINGLE,
            bytes::Bytes::from(image_bytes),
            BlobMeta {
                content_type: Some("image/png".to_owned()),
                size: image_size,
                content_hash,
                created_at: now(),
                retention: BlobRetention::SessionScoped(other_session_id),
            },
        )
        .await
        .expect("image blob writes");

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                artifact_id: "artifact-image".to_owned(),
                at: now(),
                blob_ref: Some(blob_ref),
                content_hash: Some(content_hash.to_vec()),
                kind: "image".to_owned(),
                preview: Some("生成的图片".to_owned()),
                run_id,
                session_id,
                source: ArtifactSource::Tool,
                source_message_id: None,
                source_tool_use_id: Some(ToolUseId::new()),
                status: ArtifactStatus::Ready,
                title: "生成的图片".to_owned(),
            })],
        )
        .await
        .expect("artifact event should append");

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-image".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("cross-session blob should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_missing_artifact() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "missing-artifact".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("missing artifact should be rejected");

    assert_eq!(error.code, "NOT_FOUND");
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_not_ready_artifact() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-running",
        "image",
        ArtifactStatus::Running,
        None,
    )
    .await;

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-running".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("not-ready artifact should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("not ready"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_non_image_artifact() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-file",
        "file",
        ArtifactStatus::Ready,
        Some(("text/plain", b"hello".to_vec(), session_id)),
    )
    .await;

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-file".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("non-image artifact should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("only available for images"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_svg_image_blob() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-svg",
        "image",
        ArtifactStatus::Ready,
        Some((
            "image/svg+xml",
            br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#.to_vec(),
            session_id,
        )),
    )
    .await;

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-svg".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("svg image artifact should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("only available for images"));
    assert!(!error.message.contains(".jyowo"));
    assert!(!error.message.contains("artifact-svg"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_mislabeled_image_blob() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-mislabeled",
        "image",
        ArtifactStatus::Ready,
        Some((
            "image/png",
            br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#.to_vec(),
            session_id,
        )),
    )
    .await;

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-mislabeled".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("mislabeled image artifact should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("only available for images"));
    assert!(!error.message.contains(".jyowo"));
    assert!(!error.message.contains("artifact-mislabeled"));
}

#[tokio::test]
async fn get_artifact_media_preview_with_runtime_state_rejects_too_large_image_blob() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    append_artifact_event_for_preview(
        &state,
        session_id,
        "artifact-large",
        "image",
        ArtifactStatus::Ready,
        Some(("image/png", vec![0; 10 * 1024 * 1024 + 1], session_id)),
    )
    .await;

    let error = get_artifact_media_preview_with_runtime_state(
        GetArtifactMediaPreviewRequest {
            conversation_id: session_id.to_string(),
            artifact_id: "artifact-large".to_owned(),
        },
        &state,
    )
    .await
    .expect_err("too-large image artifact should be rejected");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("too large"));
}

async fn append_artifact_event_for_preview(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    artifact_id: &str,
    kind: &str,
    status: ArtifactStatus,
    blob: Option<(&str, Vec<u8>, SessionId)>,
) {
    let run_id = RunId::new();
    let (blob_ref, content_hash) = if let Some((content_type, bytes, retention_session_id)) = blob {
        let size = bytes.len() as u64;
        let content_hash = *blake3::hash(&bytes).as_bytes();
        let blob_store = FileBlobStore::open(
            state
                .workspace_root()
                .join(".jyowo")
                .join("runtime")
                .join("blobs"),
        )
        .expect("blob store opens");
        let blob_ref = blob_store
            .put(
                TenantId::SINGLE,
                bytes::Bytes::from(bytes),
                BlobMeta {
                    content_type: Some(content_type.to_owned()),
                    size,
                    content_hash,
                    created_at: now(),
                    retention: BlobRetention::SessionScoped(retention_session_id),
                },
            )
            .await
            .expect("blob writes");
        (Some(blob_ref), Some(content_hash.to_vec()))
    } else {
        (None, None)
    };

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                artifact_id: artifact_id.to_owned(),
                at: now(),
                blob_ref,
                content_hash,
                kind: kind.to_owned(),
                preview: Some("Generated artifact".to_owned()),
                run_id,
                session_id,
                source: ArtifactSource::Tool,
                source_message_id: None,
                source_tool_use_id: Some(ToolUseId::new()),
                status,
                title: "Generated artifact".to_owned(),
            })],
        )
        .await
        .expect("artifact event should append");
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_scopes_artifacts_to_requested_conversation() {
    let state = runtime_state_with_harness().await;
    let default_session_id = state.default_conversation_id();
    let other_session_id = SessionId::new();
    open_conversation_session(&state, default_session_id).await;
    open_conversation_session(&state, other_session_id).await;
    let run_id = RunId::new();

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            default_session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                artifact_id: "artifact-default".to_owned(),
                at: now(),
                blob_ref: None,
                content_hash: None,
                kind: "markdown".to_owned(),
                preview: Some("Default conversation artifact".to_owned()),
                run_id,
                session_id: default_session_id,
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                status: ArtifactStatus::Ready,
                title: "Default artifact".to_owned(),
            })],
        )
        .await
        .expect("default artifact should append");
    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            other_session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                artifact_id: "artifact-other".to_owned(),
                at: now(),
                blob_ref: None,
                content_hash: None,
                kind: "markdown".to_owned(),
                preview: Some("Other conversation artifact".to_owned()),
                run_id,
                session_id: other_session_id,
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                status: ArtifactStatus::Ready,
                title: "Other artifact".to_owned(),
            })],
        )
        .await
        .expect("other artifact should append");

    let payload = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: other_session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("runtime artifact projection should load");

    assert_eq!(payload.artifacts.len(), 1);
    assert_eq!(payload.artifacts[0].id, "artifact-other");
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_requires_conversation_id() {
    let state = runtime_state_with_harness().await;

    let error = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: String::new(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_ignores_mismatched_artifact_session_ids() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                artifact_id: "artifact-mismatched".to_owned(),
                at: now(),
                blob_ref: None,
                content_hash: None,
                kind: "markdown".to_owned(),
                preview: Some("Wrong session".to_owned()),
                run_id: RunId::new(),
                session_id: SessionId::new(),
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                status: ArtifactStatus::Ready,
                title: "Mismatched artifact".to_owned(),
            })],
        )
        .await
        .expect("artifact event should append");

    let payload = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("runtime artifact projection should load");

    assert!(payload.artifacts.is_empty());
}

#[tokio::test]
async fn list_reference_candidates_with_runtime_state_scopes_artifacts_to_requested_conversation() {
    let state = runtime_state_with_harness().await;
    let default_session_id = state.default_conversation_id();
    let other_session_id = SessionId::new();
    open_conversation_session(&state, default_session_id).await;
    open_conversation_session(&state, other_session_id).await;
    let run_id = RunId::new();

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            default_session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                artifact_id: "artifact-default".to_owned(),
                at: now(),
                blob_ref: None,
                content_hash: None,
                kind: "markdown".to_owned(),
                preview: Some("Default conversation artifact".to_owned()),
                run_id,
                session_id: default_session_id,
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                status: ArtifactStatus::Ready,
                title: "Default artifact".to_owned(),
            })],
        )
        .await
        .expect("default artifact should append");
    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            other_session_id,
            &[Event::ArtifactCreated(ArtifactCreatedEvent {
                artifact_id: "artifact-other".to_owned(),
                at: now(),
                blob_ref: None,
                content_hash: None,
                kind: "markdown".to_owned(),
                preview: Some("Other conversation artifact".to_owned()),
                run_id,
                session_id: other_session_id,
                source: ArtifactSource::Assistant,
                source_message_id: None,
                source_tool_use_id: None,
                status: ArtifactStatus::Ready,
                title: "Other artifact".to_owned(),
            })],
        )
        .await
        .expect("other artifact should append");

    let payload = list_reference_candidates_with_runtime_state(
        ListReferenceCandidatesRequest {
            conversation_id: other_session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("reference candidates should load");

    assert_eq!(payload.artifacts.len(), 1);
    assert_eq!(payload.artifacts[0].id.as_deref(), Some("artifact-other"));
}

#[tokio::test]
async fn list_reference_candidates_with_runtime_state_rejects_invalid_conversation_id() {
    let state = runtime_state_with_harness().await;

    let error = list_reference_candidates_with_runtime_state(
        ListReferenceCandidatesRequest {
            conversation_id: "not-a-session-id".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn list_reference_candidates_with_runtime_state_rejects_unknown_conversation_id() {
    let state = runtime_state_with_harness().await;
    open_conversation_session(&state, state.default_conversation_id()).await;

    let error = list_reference_candidates_with_runtime_state(
        ListReferenceCandidatesRequest {
            conversation_id: SessionId::new().to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "NOT_FOUND");
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_redacts_artifact_metadata() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    let token = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    open_conversation_session(&state, session_id).await;

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::ArtifactCreated(ArtifactCreatedEvent {
                    artifact_id: "artifact-sensitive".to_owned(),
                    at: now(),
                    blob_ref: None,
                    content_hash: None,
                    kind: format!("markdown {token} data:image/svg+xml,<svg onload=alert(1)>。"),
                    preview: Some(
                        "Blob:.jyowo/runtime/blobs/blob-001 log/tmp/provider-output".to_owned(),
                    ),
                    run_id: RunId::new(),
                    session_id,
                    source: ArtifactSource::Assistant,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: ArtifactStatus::Running,
                    title: format!("Review {token} https://provider.example/artifact"),
                }),
                Event::ArtifactUpdated(ArtifactUpdatedEvent {
                    artifact_id: "artifact-sensitive".to_owned(),
                    at: now(),
                    blob_ref: None,
                    content_hash: None,
                    kind: Some("markdown file:/tmp/provider-output".to_owned()),
                    preview: Some(
                        "Updated 路径：.jyowo/runtime/blobs/blob-002 home~/secret blob:null/provider"
                            .to_owned(),
                    ),
                    run_id: RunId::new(),
                    session_id,
                    source: ArtifactSource::Assistant,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: Some(ArtifactStatus::Ready),
                    title: Some("Updated链接https://provider.example/updated".to_owned()),
                }),
            ],
        )
        .await
        .expect("artifact event should append");

    let payload = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("runtime artifact projection should load");
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(!serialized.contains(token));
    assert!(!serialized.contains("https://provider.example"));
    assert!(!serialized.contains(".jyowo/runtime/blobs"));
    assert!(!serialized.contains("/tmp/provider-output"));
    assert!(!serialized.contains("~/secret"));
    assert!(!serialized.contains("data:image"));
    assert!(!serialized.contains("blob:null"));
    assert!(!serialized.contains("file:"));
    assert!(serialized.contains("[REDACTED]"));
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_hides_runtime_read_errors() {
    let state = runtime_state_with_harness().await;

    let error = list_artifacts_with_runtime_state(
        ListArtifactsRequest {
            conversation_id: SessionId::new().to_string(),
        },
        &state,
    )
    .await
    .expect_err("missing conversation session should fail safely");

    assert_eq!(error.code, "NOT_FOUND");
    assert!(!error
        .message
        .contains(&state.default_conversation_id().to_string()));
}

#[test]
fn start_run_payload_validates_prompt_and_requires_runtime() {
    let error = start_run_payload(StartRunRequest {
        client_message_id: None,
        attachments: None,
        context_references: Some(vec![ContextReferencePayload::WorkspaceFile {
            label: "Desktop app".to_owned(),
            path: "apps/desktop".to_owned(),
        }]),
        conversation_id: SessionId::new().to_string(),
        prompt: "Continue implementation".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");

    let error = start_run_payload(StartRunRequest {
        client_message_id: None,
        attachments: None,
        context_references: None,
        conversation_id: SessionId::new().to_string(),
        prompt: String::new(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");

    let error = start_run_payload(StartRunRequest {
        client_message_id: Some("00000000-0000-1000-8000-000000000001".to_owned()),
        attachments: None,
        context_references: None,
        conversation_id: SessionId::new().to_string(),
        prompt: "Continue implementation".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn create_attachment_from_path_writes_workspace_file_to_blob_store() {
    let workspace = unique_workspace("attachment-workspace-file");
    let attachment_path = workspace.join("notes.txt");
    std::fs::create_dir_all(attachment_path.parent().unwrap()).unwrap();
    std::fs::write(&attachment_path, "local notes").unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace.clone()).await;

    let payload = create_attachment_from_path_with_runtime_state(
        CreateAttachmentFromPathRequest {
            path: attachment_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .expect("workspace file should become an attachment reference");

    assert_eq!(payload.attachment.name, "notes.txt");
    assert_eq!(payload.attachment.mime_type, "text/plain");

    let record_path = workspace
        .join(".jyowo")
        .join("runtime")
        .join("attachments")
        .join("records")
        .join(format!("{}.json", payload.attachment.id));
    let record: Value = serde_json::from_slice(&std::fs::read(record_path).unwrap()).unwrap();
    assert_eq!(
        record["blobRef"]["size"].as_u64(),
        Some("local notes".len() as u64)
    );
    assert_eq!(
        record["attachment"]["blobRef"]["contentType"].as_str(),
        Some("text/plain")
    );
    assert_eq!(
        record["blobRef"]["content_type"].as_str(),
        Some("text/plain")
    );
}

#[tokio::test]
async fn create_attachment_from_path_rejects_external_file_before_read() {
    let workspace = unique_workspace("attachment-external-workspace");
    let external = unique_workspace("attachment-external-source");
    let attachment_path = external.join("outside.txt");
    std::fs::create_dir_all(attachment_path.parent().unwrap()).unwrap();
    std::fs::write(&attachment_path, "external notes").unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace.clone()).await;

    let error = create_attachment_from_path_with_runtime_state(
        CreateAttachmentFromPathRequest {
            path: attachment_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("workspace"));
}

#[tokio::test]
async fn create_attachment_from_path_does_not_reveal_external_path_existence() {
    let workspace = unique_workspace("attachment-existence-workspace");
    let external = unique_workspace("attachment-existence-source");
    let existing_path = external.join("outside.txt");
    let missing_path = external.join("missing.txt");
    std::fs::create_dir_all(existing_path.parent().unwrap()).unwrap();
    std::fs::write(&existing_path, "external notes").unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;

    let existing_error = create_attachment_from_path_with_runtime_state(
        CreateAttachmentFromPathRequest {
            path: existing_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();
    let missing_error = create_attachment_from_path_with_runtime_state(
        CreateAttachmentFromPathRequest {
            path: missing_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(existing_error.code, "INVALID_PAYLOAD");
    assert_eq!(missing_error.code, "INVALID_PAYLOAD");
    assert_eq!(existing_error.message, missing_error.message);
    assert!(existing_error.message.contains("workspace"));
}

#[tokio::test]
async fn create_attachment_from_path_rejects_files_larger_than_five_mb() {
    let workspace = unique_workspace("attachment-too-large");
    let attachment_path = workspace.join("large.txt");
    std::fs::create_dir_all(attachment_path.parent().unwrap()).unwrap();
    std::fs::write(&attachment_path, vec![b'x'; 5 * 1024 * 1024 + 1]).unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;

    let error = create_attachment_from_path_with_runtime_state(
        CreateAttachmentFromPathRequest {
            path: attachment_path.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("5 MB"));
}

#[tokio::test]
async fn start_run_with_runtime_state_rejects_untrusted_attachment_id_before_record_read() {
    let state = runtime_state_with_harness_for_workspace(unique_workspace("attachment-id")).await;

    let error = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: Some(vec![AttachmentReferencePayload {
                id: "../escape".to_owned(),
                mime_type: "text/plain".to_owned(),
                name: "notes.txt".to_owned(),
                size_bytes: 128,
                blob_ref: test_attachment_blob_ref(128, "text/plain"),
            }]),
            context_references: None,
            conversation_id: SessionId::new().to_string(),
            prompt: "Use this attachment".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("generated attachment id"));
}

#[tokio::test]
async fn list_reference_candidates_includes_workspace_files() {
    let workspace = unique_workspace("reference-candidates");
    let file_path = workspace.join("apps/desktop/src-tauri/src/commands.rs");
    std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    std::fs::write(&file_path, "fn main() {}").unwrap();
    let state =
        runtime_state_with_mcp_registry_for_workspace(workspace, McpRegistry::new(), Vec::new())
            .await;
    register_test_skill(&state, "shell-state", "Shell state");
    register_test_tool(&state, "list_dir", "List directory");
    save_mcp_server_with_runtime_state(
        SaveMcpServerRequest {
            enabled: true,
            display_name: "Workspace Stdio".to_owned(),
            id: "stdio".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "/bin/sh".to_owned(),
                args: vec!["-c".to_owned(), stdio_mcp_fixture_script()],
                env: Vec::new(),
                inherit_env: Vec::new(),
                working_dir: None,
            },
        },
        &state,
    )
    .await
    .expect("mcp server should register");

    let payload = list_reference_candidates_with_runtime_state(
        ListReferenceCandidatesRequest {
            conversation_id: state.default_conversation_id().to_string(),
        },
        &state,
    )
    .await
    .expect("reference candidates should load");

    assert!(payload.files.iter().any(|candidate| {
        candidate.path.as_deref() == Some("apps/desktop/src-tauri/src/commands.rs")
    }));
    assert!(payload
        .skills
        .iter()
        .any(|candidate| candidate.id.as_deref() == Some("shell-state")));
    assert!(payload
        .tools
        .iter()
        .any(|candidate| candidate.id.as_deref() == Some("mcp__stdio__echo")));
    assert!(payload
        .mcp_servers
        .iter()
        .any(|candidate| candidate.id.as_deref() == Some("stdio")));
}

#[tokio::test]
async fn list_reference_candidates_accepts_conversation_beyond_summary_page() {
    let state = runtime_state_with_harness().await;
    let file_path = state.workspace_root().join("apps/desktop/src/main.tsx");
    std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
    std::fs::write(&file_path, "export {}").unwrap();
    let requested_session_id = SessionId::new();
    open_conversation_session(&state, requested_session_id).await;
    for _ in 0..60 {
        open_conversation_session(&state, SessionId::new()).await;
    }

    let payload = list_reference_candidates_with_runtime_state(
        ListReferenceCandidatesRequest {
            conversation_id: requested_session_id.to_string(),
        },
        &state,
    )
    .await
    .expect("reference candidates should load for existing conversations beyond summaries");

    assert!(payload
        .files
        .iter()
        .any(|candidate| candidate.path.as_deref() == Some("apps/desktop/src/main.tsx")));
}

#[tokio::test]
async fn start_run_with_runtime_state_accepts_structured_context_and_attachments() {
    let workspace = unique_workspace("structured-start-run");
    let workspace_file = workspace.join("docs/notes.txt");
    std::fs::create_dir_all(workspace_file.parent().unwrap()).unwrap();
    std::fs::write(&workspace_file, "workspace context").unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;
    let attachment = create_attachment_from_path_with_runtime_state(
        CreateAttachmentFromPathRequest {
            path: workspace_file.to_string_lossy().to_string(),
        },
        &state,
    )
    .await
    .expect("attachment should be stored")
    .attachment;
    let session_id = SessionId::new();

    let payload = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: Some(vec![attachment]),
            context_references: Some(vec![ContextReferencePayload::WorkspaceFile {
                label: "Notes".to_owned(),
                path: "docs/notes.txt".to_owned(),
            }]),
            conversation_id: session_id.to_string(),
            prompt: "Run the relevant checks".to_owned(),
        },
        &state,
    )
    .await
    .expect("structured composer draft should start a run");

    assert_eq!(payload.status, "started");
    assert!(RunId::parse(&payload.run_id).is_ok());
    assert!(state.pending_permission_requests().is_empty());
}

#[tokio::test]
async fn start_run_with_runtime_state_rejects_workspace_file_reference_outside_workspace() {
    let workspace = unique_workspace("reference-workspace");
    let external = unique_workspace("reference-external");
    let external_file = external.join("outside.txt");
    std::fs::create_dir_all(external_file.parent().unwrap()).unwrap();
    std::fs::write(&external_file, "outside").unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace).await;

    let error = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: Some(vec![ContextReferencePayload::WorkspaceFile {
                label: "Outside".to_owned(),
                path: external_file.to_string_lossy().to_string(),
            }]),
            conversation_id: SessionId::new().to_string(),
            prompt: "Use this file".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error.message.contains("inside the workspace"));
}

#[tokio::test]
async fn start_run_with_runtime_state_returns_real_run_id_for_conversation() {
    let state = runtime_state_with_harness().await;
    let context_file = state.workspace_root().join("apps/desktop/src/main.tsx");
    std::fs::create_dir_all(context_file.parent().unwrap()).unwrap();
    std::fs::write(&context_file, "export {}").unwrap();
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let session_id = SessionId::new();
    let conversation_id = session_id.to_string();

    let payload = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: Some(vec![ContextReferencePayload::WorkspaceFile {
                label: "Desktop app".to_owned(),
                path: "apps/desktop/src/main.tsx".to_owned(),
            }]),
            conversation_id: conversation_id.clone(),
            prompt: "Continue implementation".to_owned(),
        },
        &state,
    )
    .await
    .expect("runtime state should start a conversation run");

    assert_eq!(payload.status, "started");
    let run_id = RunId::parse(&payload.run_id).expect("run id should be canonical");
    assert_eq!(run_id.to_string(), payload.run_id);

    let page = harness
        .page_conversation_events(ConversationEventsPageRequest {
            options: state.conversation_session_options(session_id),
            after_event_id: None,
            limit: 20,
        })
        .await
        .expect("conversation events should be readable after start_run");

    assert!(page.events.iter().any(|envelope| {
        matches!(
            &envelope.payload,
            Event::RunStarted(started)
                if started.session_id == session_id && started.run_id == run_id
        )
    }));
    assert_eq!(conversation_id, session_id.to_string());
}

#[tokio::test]
async fn subscribe_conversation_events_emits_live_batches_and_unsubscribes() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    open_conversation_session(&state, session_id).await;
    let conversation_id = session_id.to_string();
    let batches = Arc::new(Mutex::new(Vec::<ConversationEventBatchPayload>::new()));
    let emitted_batches = Arc::clone(&batches);

    let subscription = subscribe_conversation_events_for_window_with_runtime_state(
        SubscribeConversationEventsRequest {
            conversation_id: conversation_id.clone(),
            after_cursor: None,
        },
        "main".to_owned(),
        Arc::new(move |batch| {
            emitted_batches.lock().unwrap().push(batch);
            Ok(())
        }),
        &state,
    )
    .await
    .expect("subscription should be accepted");

    assert_eq!(subscription.conversation_id, conversation_id);
    assert!(subscription.replay_events.is_empty());
    assert!(!subscription.gap);

    let started = start_run_with_runtime_state(
        StartRunRequest {
            attachments: None,
            client_message_id: Some("00000000-0000-4000-8000-000000000001".to_owned()),
            context_references: None,
            conversation_id: conversation_id.clone(),
            prompt: "Continue implementation".to_owned(),
        },
        &state,
    )
    .await
    .expect("run should start after subscribing");

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if batches.lock().unwrap().iter().any(|batch| {
                batch.subscription_id == subscription.subscription_id
                    && batch.conversation_id == conversation_id
                    && batch.phase == "live"
                    && batch.events.iter().any(|event| {
                        event.run_id == started.run_id && event.event_type == "run.started"
                    })
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("live subscription should emit the new run event");

    let emitted = batches.lock().unwrap();
    let live_events = emitted
        .iter()
        .filter(|batch| batch.subscription_id == subscription.subscription_id)
        .flat_map(|batch| batch.events.iter())
        .collect::<Vec<_>>();
    let live_run_started = live_events
        .iter()
        .find(|event| event.run_id == started.run_id && event.event_type == "run.started")
        .expect("live batch should include the started run event");
    assert!(live_run_started.conversation_sequence > 0);
    assert!(live_events
        .windows(2)
        .all(|pair| pair[0].conversation_sequence < pair[1].conversation_sequence));

    let unsubscribed = unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: subscription.subscription_id.clone(),
        },
        "main".to_owned(),
        &state,
    )
    .await
    .expect("unsubscribe should succeed");
    assert_eq!(unsubscribed.status, "unsubscribed");

    let already_closed = unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: subscription.subscription_id,
        },
        "main".to_owned(),
        &state,
    )
    .await
    .expect("unsubscribe should be idempotent");
    assert_eq!(already_closed.status, "alreadyClosed");
}

#[tokio::test]
async fn unsubscribe_conversation_events_rejects_other_window_subscription() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    open_conversation_session(&state, session_id).await;
    let conversation_id = session_id.to_string();
    let subscription = subscribe_conversation_events_for_window_with_runtime_state(
        SubscribeConversationEventsRequest {
            conversation_id,
            after_cursor: None,
        },
        "main".to_owned(),
        Arc::new(|_batch| Ok(())),
        &state,
    )
    .await
    .expect("subscription should be created");

    let denied = unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: subscription.subscription_id.clone(),
        },
        "secondary".to_owned(),
        &state,
    )
    .await
    .expect_err("another window must not close the subscription");
    assert_eq!(denied.code, "INVALID_PAYLOAD");

    let unsubscribed = unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: subscription.subscription_id,
        },
        "main".to_owned(),
        &state,
    )
    .await
    .expect("owning window can close the subscription");
    assert_eq!(unsubscribed.status, "unsubscribed");
}

#[tokio::test]
async fn subscribe_conversation_events_accepts_cursor_after_replayed_permission_request() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "NeedsPermission".to_owned(),
                input: json!({ "command": "pwd" }),
            },
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let conversation_id = session_id.to_string();

    start_run_with_runtime_state(
        StartRunRequest {
            attachments: None,
            client_message_id: Some("00000000-0000-4000-8000-000000000001".to_owned()),
            context_references: None,
            conversation_id: conversation_id.clone(),
            prompt: "Run a command".to_owned(),
        },
        &state,
    )
    .await
    .expect("run should start and wait on permission");
    let pending = wait_for_pending_permission_for_session(&state, session_id).await;

    let first_subscription = subscribe_conversation_events_for_window_with_runtime_state(
        SubscribeConversationEventsRequest {
            conversation_id: conversation_id.clone(),
            after_cursor: None,
        },
        "main".to_owned(),
        Arc::new(|_batch| Ok(())),
        &state,
    )
    .await
    .expect("subscription replay should include pending permission");
    assert!(first_subscription
        .replay_events
        .iter()
        .any(|event| event.event_type == "permission.requested"));
    let cursor = first_subscription
        .cursor
        .clone()
        .expect("subscription replay should return a cursor");

    let second_subscription = subscribe_conversation_events_for_window_with_runtime_state(
        SubscribeConversationEventsRequest {
            conversation_id: conversation_id.clone(),
            after_cursor: Some(cursor),
        },
        "main".to_owned(),
        Arc::new(|_batch| Ok(())),
        &state,
    )
    .await
    .expect("cursor from permission replay should be accepted by the next subscription");
    assert!(second_subscription.replay_events.is_empty());

    unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: first_subscription.subscription_id,
        },
        "main".to_owned(),
        &state,
    )
    .await
    .unwrap();
    unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: second_subscription.subscription_id,
        },
        "main".to_owned(),
        &state,
    )
    .await
    .unwrap();
    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id,
            decision: PermissionDecision::Deny,
            request_id: pending.request.request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn start_run_with_runtime_state_exposes_runtime_permission_request_to_activity() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "NeedsPermission".to_owned(),
                input: json!({ "command": "printf desktop-permission" }),
            },
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let conversation_id = session_id.to_string();

    let started = tokio::time::timeout(Duration::from_secs(1), async {
        start_run_with_runtime_state(
            StartRunRequest {
                client_message_id: None,
                attachments: None,
                context_references: None,
                conversation_id: conversation_id.clone(),
                prompt: "Run a command".to_owned(),
            },
            &state,
        )
        .await
    })
    .await
    .expect("start_run should return while permission is pending")
    .expect("start_run should start a conversation run");
    let run_id = RunId::parse(&started.run_id).expect("run id should be canonical");

    let pending = wait_for_pending_permission_for_session(&state, session_id).await;
    let request_id = pending.request.request_id;
    assert_eq!(pending.context.run_id, Some(run_id));
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let page = harness
        .page_conversation_events(ConversationEventsPageRequest {
            options: state.conversation_session_options(session_id),
            after_event_id: None,
            limit: 20,
        })
        .await
        .expect("conversation events should be readable while permission is pending");
    assert!(page.events.iter().any(|envelope| {
        matches!(
            &envelope.payload,
            Event::PermissionRequested(requested) if requested.request_id == request_id
        )
    }));

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(conversation_id),
            run_id: Some(started.run_id),
        },
        &state,
    )
    .await
    .unwrap();
    let value = serde_json::to_value(&payload).unwrap();

    let permission_event = value["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["type"] == "permission.requested")
        .expect("activity should include the pending permission event");
    assert_eq!(
        permission_event["payload"]["requestId"],
        serde_json::Value::String(request_id.to_string())
    );
    assert_eq!(
        permission_event["payload"]["toolUseId"],
        serde_json::Value::String(pending.request.tool_use_id.to_string())
    );
    assert_eq!(
        permission_event["payload"]["operation"],
        serde_json::Value::String("Execute command".to_owned())
    );
    assert_eq!(
        permission_event["payload"]["target"],
        serde_json::Value::String("printf".to_owned())
    );
    assert!(permission_event["payload"].get("command").is_none());

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .unwrap();
    let value = serde_json::to_value(&payload).unwrap();
    let permission_event = value["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["type"] == "permission.requested")
        .expect("run-filtered activity should include the pending permission event");
    assert_eq!(
        permission_event["payload"]["requestId"],
        serde_json::Value::String(request_id.to_string())
    );
    assert_eq!(
        permission_event["payload"]["toolUseId"],
        serde_json::Value::String(pending.request.tool_use_id.to_string())
    );

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Deny,
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
}

#[test]
fn cancel_run_payload_validates_and_requires_runtime() {
    let error = cancel_run_payload(CancelRunRequest {
        run_id: "run-001".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");

    let error = cancel_run_payload(CancelRunRequest {
        run_id: String::new(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn cancel_run_with_runtime_state_cancels_active_run_through_sdk() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "NeedsPermission".to_owned(),
                input: json!({ "command": "printf cancel-me" }),
            },
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = state.default_conversation_id();
    let started = tokio::time::timeout(Duration::from_secs(1), async {
        start_run_with_runtime_state(
            StartRunRequest {
                client_message_id: None,
                attachments: None,
                context_references: None,
                conversation_id: session_id.to_string(),
                prompt: "Run a cancellable command".to_owned(),
            },
            &state,
        )
        .await
    })
    .await
    .expect("start_run should return while permission is pending")
    .expect("start_run should start a cancellable run");

    let payload = cancel_run_with_runtime_state(
        CancelRunRequest {
            run_id: started.run_id.clone(),
        },
        &state,
    )
    .await
    .expect("active run should cancel through runtime state");

    assert_eq!(payload.run_id, started.run_id);
    assert_eq!(payload.status, "cancelled");
}

#[test]
fn resolve_permission_payload_requires_runtime_permission_broker() {
    let conversation_id = SessionId::new().to_string();
    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id: conversation_id.clone(),
        decision: PermissionDecision::Approve,
        request_id: "01HZ0000000000000000000001".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");

    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id: conversation_id.clone(),
        decision: PermissionDecision::Deny,
        request_id: "01HZ0000000000000000000001".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");

    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id,
        decision: PermissionDecision::Approve,
        request_id: String::new(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[test]
fn resolve_permission_payload_rejects_invalid_request_id_before_runtime() {
    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id: SessionId::new().to_string(),
        decision: PermissionDecision::Approve,
        request_id: "permission-001".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[test]
fn resolve_permission_payload_rejects_noncanonical_request_id_before_runtime() {
    let error = resolve_permission_payload(ResolvePermissionRequest {
        conversation_id: SessionId::new().to_string(),
        decision: PermissionDecision::Approve,
        request_id: "01hz0000000000000000000001".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn runtime_state_routes_permission_decisions_to_permission_broker_resolver() {
    let workspace = unique_workspace("runtime-state-routes");
    std::fs::create_dir_all(&workspace).unwrap();
    let state = runtime_state_for_workspace(workspace)
        .await
        .expect("runtime state should initialize");
    assert!(state.harness().is_some());

    let error = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: SessionId::new().to_string(),
            decision: PermissionDecision::Approve,
            request_id: "01HZ0000000000000000000001".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "NOT_FOUND");
    assert!(error.message.contains("permission request not found"));
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_state_async_uses_explicit_workspace_root() {
    let _lock = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace_root = unique_workspace("explicit-workspace-root");
    std::fs::create_dir_all(&workspace_root).unwrap();
    let _env = EnvVarGuard::set(WORKSPACE_ROOT_ENV, workspace_root.as_os_str());

    let state = runtime_state_async()
        .await
        .expect("runtime state should initialize with explicit workspace root");
    let options = state.conversation_session_options(SessionId::new());

    assert_eq!(
        options.workspace_root,
        workspace_root.canonicalize().unwrap()
    );
}

#[test]
fn execution_settings_persist_permission_mode_for_session_options() {
    let workspace = unique_workspace("execution-settings-session-options");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    set_execution_settings_with_store(
        SetExecutionSettingsRequest {
            permission_mode: PermissionMode::BypassPermissions,
        },
        &DesktopExecutionSettingsStore::new(state.workspace_root().to_path_buf()),
    )
    .expect("execution settings should save");

    let options = state.conversation_session_options(SessionId::new());

    assert_eq!(options.permission_mode, PermissionMode::BypassPermissions);
}

#[test]
fn get_execution_settings_defaults_to_standard_mode() {
    let workspace = unique_workspace("execution-settings-default");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("runtime state should initialize");
    let settings = get_execution_settings_with_store(&DesktopExecutionSettingsStore::new(
        state.workspace_root().to_path_buf(),
    ))
    .expect("execution settings should load");

    assert_eq!(settings.permission_mode, PermissionMode::Default);
    assert_eq!(settings.auto_mode_available, cfg!(feature = "auto-mode"));
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_state_async_rejects_missing_explicit_workspace_root() {
    let _lock = WORKSPACE_ROOT_ENV_LOCK.lock().unwrap();
    let workspace_root = unique_workspace("missing-explicit-workspace-root");
    let _env = EnvVarGuard::set(WORKSPACE_ROOT_ENV, workspace_root.as_os_str());

    let error = match runtime_state_async().await {
        Ok(_) => panic!("runtime state should reject missing explicit workspace root"),
        Err(error) => error,
    };

    assert_eq!(error.code, "RUNTIME_INIT_FAILED");
    assert!(error.message.contains(WORKSPACE_ROOT_ENV));
    assert!(error
        .message
        .contains(&workspace_root.display().to_string()));
}

#[tokio::test]
async fn runtime_state_resolves_pending_permission_from_harness_broker() {
    let state = runtime_state_with_harness().await;
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let request = permission_request();
    let request_id = request.request_id;
    let request_session_id = request.session_id;

    let decision_task =
        tokio::spawn(async move { broker.decide(request, permission_context()).await });

    wait_for_pending_permission(&state, request_id).await;

    let payload = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Approve,
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();

    assert_eq!(payload.status, "resolved");
    assert_eq!(decision_task.await.unwrap(), Decision::AllowOnce);
}

#[tokio::test]
async fn runtime_state_rejects_permission_resolution_for_wrong_conversation() {
    let state = runtime_state_with_harness().await;
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let request = permission_request();
    let request_id = request.request_id;
    let request_session_id = request.session_id;

    let decision_task =
        tokio::spawn(async move { broker.decide(request, permission_context()).await });

    wait_for_pending_permission(&state, request_id).await;

    let error = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: SessionId::new().to_string(),
            decision: PermissionDecision::Approve,
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error
        .message
        .contains("permission request does not belong to conversationId"));

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Deny,
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(decision_task.await.unwrap(), Decision::DenyOnce);
}

#[tokio::test]
async fn runtime_state_requires_window_subscription_before_permission_resolution() {
    let state = runtime_state_with_harness().await;
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let request = permission_request();
    let request_id = request.request_id;
    let request_session_id = request.session_id;
    let conversation_id = request_session_id.to_string();
    open_conversation_session(&state, request_session_id).await;

    let decision_task =
        tokio::spawn(async move { broker.decide(request, permission_context()).await });

    wait_for_pending_permission(&state, request_id).await;

    let error = resolve_permission_for_window_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: conversation_id.clone(),
            decision: PermissionDecision::Approve,
            request_id: request_id.to_string(),
        },
        "main".to_owned(),
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(error
        .message
        .contains("permission request is not visible in this window"));

    let subscription = subscribe_conversation_events_for_window_with_runtime_state(
        SubscribeConversationEventsRequest {
            conversation_id: conversation_id.clone(),
            after_cursor: None,
        },
        "main".to_owned(),
        Arc::new(|_batch| Ok(())),
        &state,
    )
    .await
    .unwrap();

    let payload = resolve_permission_for_window_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id,
            decision: PermissionDecision::Approve,
            request_id: request_id.to_string(),
        },
        "main".to_owned(),
        &state,
    )
    .await
    .unwrap();

    assert_eq!(payload.status, "resolved");
    assert_eq!(decision_task.await.unwrap(), Decision::AllowOnce);

    let _ = unsubscribe_conversation_events_for_window_with_runtime_state(
        UnsubscribeConversationEventsRequest {
            subscription_id: subscription.subscription_id,
        },
        "main".to_owned(),
        &state,
    )
    .await;
}

#[tokio::test]
async fn list_activity_with_runtime_state_hides_pending_permission_without_durable_request_event() {
    let state = runtime_state_with_harness().await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let request = permission_request();
    let request_id = request.request_id;
    let request_session_id = request.session_id;
    let conversation_id = session_id.to_string();

    let decision_task =
        tokio::spawn(async move { broker.decide(request, permission_context()).await });

    wait_for_pending_permission(&state, request_id).await;

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(conversation_id),
            run_id: None,
        },
        &state,
    )
    .await
    .unwrap();

    assert!(payload.events.is_empty());

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Deny,
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(decision_task.await.unwrap(), Decision::DenyOnce);
}

#[tokio::test]
async fn list_activity_with_runtime_state_reads_journaled_permission_requests_by_run_id() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "NeedsPermission".to_owned(),
                input: json!({ "command": "pwd" }),
            },
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Run a command".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let run_id = RunId::parse(&started.run_id).expect("run id should be canonical");
    let pending = wait_for_pending_permission_for_session(&state, session_id).await;
    let request_id = pending.request.request_id;

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(started.run_id),
        },
        &state,
    )
    .await
    .unwrap();
    let value = serde_json::to_value(&payload).unwrap();

    let permission_event = value["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["type"] == "permission.requested")
        .expect("activity should include the pending permission event");
    assert_eq!(
        permission_event["payload"]["requestId"],
        serde_json::Value::String(request_id.to_string())
    );
    assert_eq!(pending.context.run_id, Some(run_id));

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Deny,
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn list_activity_with_runtime_state_requires_conversation_id() {
    let state = runtime_state_with_harness().await;

    let error = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: None,
            run_id: Some(RunId::new().to_string()),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn list_activity_with_runtime_state_reads_durable_run_events() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::MessageStart {
            message_id: "message-usage".to_owned(),
            usage: UsageSnapshot {
                input_tokens: 11,
                output_tokens: 0,
                cache_read_tokens: 3,
                cache_write_tokens: 5,
                cost_micros: 0,
                tool_calls: 0,
            },
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Done".to_owned()),
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: None,
            usage_delta: UsageSnapshot {
                input_tokens: 0,
                output_tokens: 7,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                cost_micros: 260,
                tool_calls: 0,
            },
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Complete the task".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    loop {
        let payload = list_activity_with_runtime_state(
            ListActivityRequest {
                conversation_id: Some(session_id.to_string()),
                run_id: Some(started.run_id.clone()),
            },
            &state,
        )
        .await
        .unwrap();

        if payload
            .events
            .iter()
            .any(|event| event.event_type == "assistant.completed")
        {
            assert!(payload
                .events
                .iter()
                .any(|event| event.event_type == "run.started"));
            let run_ended = payload
                .events
                .iter()
                .find(|event| event.event_type == "run.ended")
                .expect("activity should include run ended event");
            assert_eq!(run_ended.payload["usage"]["inputTokens"], json!(11));
            assert_eq!(run_ended.payload["usage"]["outputTokens"], json!(7));
            assert_eq!(run_ended.payload["usage"]["cacheReadTokens"], json!(3));
            assert_eq!(run_ended.payload["usage"]["cacheWriteTokens"], json!(5));
            assert_eq!(run_ended.payload["usage"]["costMicros"], json!(260));
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("activity should include durable run events");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn list_activity_with_runtime_state_does_not_expose_thinking_deltas() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Thinking(ThinkingDelta {
                text: Some("private chain of thought".to_owned()),
                provider_native: Some(json!({ "thinking": "provider native secret" })),
                signature: Some("signature-secret".to_owned()),
            }),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 1,
            delta: ContentDelta::Text("Visible answer".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Think privately".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    loop {
        let payload = list_activity_with_runtime_state(
            ListActivityRequest {
                conversation_id: Some(session_id.to_string()),
                run_id: Some(started.run_id.clone()),
            },
            &state,
        )
        .await
        .unwrap();

        if payload
            .events
            .iter()
            .any(|event| event.event_type == "assistant.completed")
        {
            let serialized = serde_json::to_string(&payload).unwrap();
            assert!(payload.events.iter().any(|event| {
                event.event_type == "assistant.delta"
                    && event.payload["text"] == json!("Visible answer")
                    && event.payload["messageId"].as_str().is_some()
            }));
            assert!(!serialized.contains("private chain of thought"));
            assert!(!serialized.contains("provider native secret"));
            assert!(!serialized.contains("signature-secret"));
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("activity should include completed assistant event");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn get_replay_timeline_with_runtime_state_does_not_expose_raw_thinking_delta_text() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Thinking(ThinkingDelta {
                text: Some("private chain of thought".to_owned()),
                provider_native: Some(json!({ "thinking": "provider native secret" })),
                signature: Some("signature-secret".to_owned()),
            }),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 1,
            delta: ContentDelta::Text("Visible answer".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Think privately".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let request = ReplayTimelineRequest {
        conversation_id: Some(session_id.to_string()),
        run_id: Some(started.run_id),
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    loop {
        let payload = get_replay_timeline_with_runtime_state(request.clone(), &state)
            .await
            .unwrap();

        if payload
            .events
            .iter()
            .any(|event| event.event_type == "assistant.completed")
        {
            let serialized = serde_json::to_string(&payload).unwrap();
            let thinking = payload
                .events
                .iter()
                .find(|event| event.event_type == "assistant.thinking.delta")
                .expect("thinking status event should be projected");
            assert_eq!(thinking.payload["status"], json!("running"));
            assert!(thinking.payload.get("text").is_none());
            assert!(thinking.payload.get("providerNative").is_none());
            assert!(thinking.payload.get("signature").is_none());
            assert!(!serialized.contains("private chain of thought"));
            assert!(!serialized.contains("provider native secret"));
            assert!(!serialized.contains("signature-secret"));
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("replay should include completed assistant event");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn list_activity_with_runtime_state_redacts_private_paths_from_assistant_deltas() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text(
                "Read /Users/alice/.ssh/config 链接https://provider.example/signed log/tmp/provider-output blob:.jyowo/runtime/blobs/blob-001"
                    .to_owned(),
            ),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Summarize path".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    loop {
        let payload = list_activity_with_runtime_state(
            ListActivityRequest {
                conversation_id: Some(session_id.to_string()),
                run_id: Some(started.run_id.clone()),
            },
            &state,
        )
        .await
        .unwrap();

        if let Some(delta) = payload
            .events
            .iter()
            .find(|event| event.event_type == "assistant.delta")
        {
            let serialized = serde_json::to_string(&payload).unwrap();
            assert!(!serialized.contains("/Users/alice/.ssh/config"));
            assert!(!serialized.contains("provider.example"));
            assert!(!serialized.contains("/tmp/provider-output"));
            assert!(!serialized.contains(".jyowo/runtime/blobs"));
            assert_eq!(
                delta.payload["text"],
                json!("Read [REDACTED] 链接[REDACTED] log[REDACTED] [REDACTED]")
            );
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("activity should include assistant delta event");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn list_activity_with_runtime_state_maps_artifact_lifecycle_events() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
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
                Event::ArtifactCreated(ArtifactCreatedEvent {
                    artifact_id: "artifact-runtime-notes".to_owned(),
                    at: now(),
                    blob_ref: None,
                    content_hash: None,
                    kind: "markdown javascript:alert(1)".to_owned(),
                    preview: Some(
                        "Blob:.jyowo/runtime/blobs/blob-001 log/tmp/provider-output".to_owned(),
                    ),
                    run_id,
                    session_id,
                    source: ArtifactSource::Assistant,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: ArtifactStatus::Running,
                    title: "Runtime notes https://provider.example/artifact".to_owned(),
                }),
                Event::ArtifactUpdated(ArtifactUpdatedEvent {
                    artifact_id: "artifact-runtime-notes".to_owned(),
                    at: now(),
                    blob_ref: None,
                    content_hash: None,
                    kind: Some("markdown /tmp/provider-output".to_owned()),
                    preview: Some(
                        "Updated 路径：.jyowo/runtime/blobs/blob-002 blob:null/provider".to_owned(),
                    ),
                    run_id,
                    session_id,
                    source: ArtifactSource::Assistant,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: Some(ArtifactStatus::Ready),
                    title: Some("Updated链接https://provider.example/updated".to_owned()),
                }),
                Event::ArtifactCreated(ArtifactCreatedEvent {
                    artifact_id: "artifact-wrong-session".to_owned(),
                    at: now(),
                    blob_ref: None,
                    content_hash: None,
                    kind: "markdown".to_owned(),
                    preview: Some("Wrong session".to_owned()),
                    run_id,
                    session_id: SessionId::new(),
                    source: ArtifactSource::Assistant,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: ArtifactStatus::Ready,
                    title: "Wrong session".to_owned(),
                }),
            ],
        )
        .await
        .expect("artifact event should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("activity should load");

    assert!(!payload
        .events
        .iter()
        .any(|event| event.payload["artifactId"] == json!("artifact-wrong-session")));
    let artifact_created = payload
        .events
        .iter()
        .find(|event| event.event_type == "artifact.created")
        .expect("activity should include artifact lifecycle event");
    assert_eq!(artifact_created.source, "engine");
    assert_eq!(artifact_created.visibility, "public");
    assert_eq!(
        artifact_created.payload["artifactId"],
        json!("artifact-runtime-notes")
    );
    assert_eq!(artifact_created.payload["status"], json!("running"));
    assert_eq!(
        artifact_created.payload["kind"],
        json!("markdown [REDACTED]")
    );
    assert_eq!(
        artifact_created.payload["title"],
        json!("Runtime notes [REDACTED]")
    );
    assert_eq!(
        artifact_created.payload["summary"],
        json!("[REDACTED] log[REDACTED]")
    );

    let artifact_updated = payload
        .events
        .iter()
        .find(|event| event.event_type == "artifact.updated")
        .expect("activity should include artifact update event");
    assert_eq!(
        artifact_updated.payload["artifactId"],
        json!("artifact-runtime-notes")
    );
    assert_eq!(artifact_updated.payload["status"], json!("ready"));
    assert_eq!(
        artifact_updated.payload["kind"],
        json!("markdown [REDACTED]")
    );
    assert_eq!(
        artifact_updated.payload["title"],
        json!("Updated链接[REDACTED]")
    );
    assert_eq!(
        artifact_updated.payload["summary"],
        json!("Updated 路径：[REDACTED] [REDACTED]")
    );
    let serialized = serde_json::to_string(&payload).unwrap();
    assert!(!serialized.contains("provider.example"));
    assert!(!serialized.contains(".jyowo/runtime/blobs"));
    assert!(!serialized.contains("/tmp/provider-output"));
    assert!(!serialized.contains("blob:null"));
    assert!(!serialized.contains("javascript:"));
}

#[tokio::test]
async fn list_activity_with_runtime_state_redacts_unsafe_artifact_media_mime_type() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
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
                Event::ArtifactCreated(ArtifactCreatedEvent {
                    artifact_id: "artifact-image".to_owned(),
                    at: now(),
                    blob_ref: Some(harness_contracts::BlobRef {
                        id: harness_contracts::BlobId::new(),
                        size: 42,
                        content_hash: [7; 32],
                        content_type: Some(
                            "image/png /tmp/provider-output https://provider.example/blob"
                                .to_owned(),
                        ),
                    }),
                    content_hash: Some(vec![9; 32]),
                    kind: "image".to_owned(),
                    preview: Some("Generated image".to_owned()),
                    run_id,
                    session_id,
                    source: ArtifactSource::Tool,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: ArtifactStatus::Ready,
                    title: "Generated image".to_owned(),
                }),
            ],
        )
        .await
        .expect("artifact event should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("activity should load");
    let artifact_created = payload
        .events
        .iter()
        .find(|event| event.event_type == "artifact.created")
        .expect("activity should include artifact lifecycle event");
    let serialized = serde_json::to_string(&payload).unwrap();

    assert_eq!(artifact_created.payload["media"]["kind"], json!("image"));
    assert_eq!(
        artifact_created.payload["media"]["mimeType"],
        json!("image/png")
    );
    assert_eq!(artifact_created.payload["media"]["sizeBytes"], json!(42));
    assert!(!serialized.contains("/tmp/provider-output"));
    assert!(!serialized.contains("provider.example"));
}

#[tokio::test]
async fn list_activity_with_runtime_state_does_not_project_secret_like_artifact_media_mime_token() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
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
                Event::ArtifactCreated(ArtifactCreatedEvent {
                    artifact_id: "artifact-video".to_owned(),
                    at: now(),
                    blob_ref: Some(harness_contracts::BlobRef {
                        id: harness_contracts::BlobId::new(),
                        size: 42,
                        content_hash: [7; 32],
                        content_type: Some(
                            "video/sk-abcdefghijklmnopqrstuvwxyz0123456789".to_owned(),
                        ),
                    }),
                    content_hash: Some(vec![9; 32]),
                    kind: "video".to_owned(),
                    preview: Some("Generated video".to_owned()),
                    run_id,
                    session_id,
                    source: ArtifactSource::Tool,
                    source_message_id: None,
                    source_tool_use_id: None,
                    status: ArtifactStatus::Ready,
                    title: "Generated video".to_owned(),
                }),
            ],
        )
        .await
        .expect("artifact event should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("activity should load");
    let artifact_created = payload
        .events
        .iter()
        .find(|event| event.event_type == "artifact.created")
        .expect("activity should include artifact lifecycle event");
    let serialized = serde_json::to_string(&payload).unwrap();

    assert_eq!(artifact_created.payload["media"]["kind"], json!("video"));
    assert_eq!(
        artifact_created.payload["media"]["mimeType"],
        json!("video/mp4")
    );
    assert!(!serialized.contains("sk-abcdefghijklmnopqrstuvwxyz0123456789"));
}

#[tokio::test]
async fn list_activity_with_runtime_state_maps_assistant_interaction_events() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let review_request_id = RequestId::new();
    let clarification_request_id = RequestId::new();
    let notice_id = RequestId::new();
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
                Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                    at: now(),
                    delta: DeltaChunk::ReasoningSummary(ReasoningSummaryChunk {
                        provider_id: "test".to_owned(),
                        provider_native: None,
                        text: "Checked https://provider.example/image，路径：.jyowo/runtime/blobs/blob-001 log/tmp/provider-output"
                            .to_owned(),
                    }),
                    message_id: MessageId::new(),
                    run_id,
                }),
                Event::AssistantReviewRequested(AssistantReviewRequestedEvent {
                    run_id,
                    request_id: review_request_id,
                    title: UiSafeText::from_trusted_redacted(
                        "Review https://provider.example/review",
                    ),
                    body: Some(UiSafeText::from_trusted_redacted(
                        "Approve blob:.jyowo/runtime/blobs/blob-001?",
                    )),
                    at: now(),
                }),
                Event::AssistantClarificationRequested(AssistantClarificationRequestedEvent {
                    run_id,
                    request_id: clarification_request_id,
                    prompt: UiSafeText::from_trusted_redacted(
                        "Which size链接https://provider.example/prompt?",
                    ),
                    at: now(),
                }),
                Event::AssistantNotice(AssistantNoticeEvent {
                    run_id,
                    notice_id,
                    body: UiSafeText::from_trusted_redacted(
                        "Generation queued at 路径：.jyowo/runtime/blobs/blob-002.",
                    ),
                    code: None,
                    at: now(),
                }),
            ],
        )
        .await
        .expect("assistant interaction events should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("activity should load");

    let event_types = payload
        .events
        .iter()
        .map(|event| event.event_type)
        .collect::<Vec<_>>();
    assert!(event_types.contains(&"assistant.review.requested"));
    assert!(event_types.contains(&"assistant.clarification.requested"));
    assert!(event_types.contains(&"assistant.notice"));
    let review = payload
        .events
        .iter()
        .find(|event| event.event_type == "assistant.review.requested")
        .expect("activity should include review");
    assert_eq!(review.payload["title"], json!("[REDACTED]"));
    assert!(review.payload["body"]
        .as_str()
        .is_some_and(|body| body.contains("[REDACTED]")));
    let clarification = payload
        .events
        .iter()
        .find(|event| event.event_type == "assistant.clarification.requested")
        .expect("activity should include clarification");
    assert!(clarification.payload["prompt"]
        .as_str()
        .is_some_and(|prompt| prompt.contains("[REDACTED]")));
    let notice = payload
        .events
        .iter()
        .find(|event| event.event_type == "assistant.notice")
        .expect("activity should include notice");
    assert!(notice.payload["body"]
        .as_str()
        .is_some_and(|body| body.contains("[REDACTED]")));
    let serialized = serde_json::to_string(&payload).unwrap();
    assert!(!serialized.contains("provider.example"));
    assert!(!serialized.contains(".jyowo/runtime/blobs"));
    assert!(!serialized.contains("/tmp/provider-output"));

    let replay = get_replay_timeline_with_runtime_state(
        ReplayTimelineRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("replay should load");
    let thinking = replay
        .events
        .iter()
        .find(|event| event.event_type == "assistant.thinking.delta")
        .expect("replay should include safe reasoning summary");
    assert_eq!(
        thinking.payload["safeSummaryDelta"],
        json!("Checked [REDACTED]，路径：[REDACTED] log[REDACTED]")
    );
    let replay_serialized = serde_json::to_string(&replay).unwrap();
    assert!(!replay_serialized.contains("provider.example"));
    assert!(!replay_serialized.contains(".jyowo/runtime/blobs"));
    assert!(!replay_serialized.contains("/tmp/provider-output"));
}

#[tokio::test]
async fn page_conversation_timeline_with_runtime_state_accepts_assistant_interaction_events() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
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
                Event::AssistantReviewRequested(AssistantReviewRequestedEvent {
                    run_id,
                    request_id: RequestId::new(),
                    title: UiSafeText::from_trusted_redacted(
                        "Review Authorization: Bearer synthetic-token",
                    ),
                    body: Some(UiSafeText::from_trusted_redacted(
                        "Approve /Users/example/private?",
                    )),
                    at: now(),
                }),
                Event::AssistantClarificationRequested(AssistantClarificationRequestedEvent {
                    run_id,
                    request_id: RequestId::new(),
                    prompt: UiSafeText::from_trusted_redacted("Which size uses sk-synthetic?"),
                    at: now(),
                }),
                Event::AssistantNotice(AssistantNoticeEvent {
                    run_id,
                    notice_id: RequestId::new(),
                    body: UiSafeText::from_trusted_redacted(
                        "Generation queued from /home/example/private.",
                    ),
                    code: None,
                    at: now(),
                }),
            ],
        )
        .await
        .expect("assistant interaction events should append");

    let page = page_conversation_timeline_with_runtime_state(
        PageConversationTimelineRequest {
            conversation_id: session_id.to_string(),
            after_cursor: None,
            limit: None,
        },
        &state,
    )
    .await
    .expect("timeline page should load");

    let event_types = page
        .events
        .iter()
        .map(|event| event.event_type)
        .collect::<Vec<_>>();
    assert!(event_types.contains(&"assistant.review.requested"));
    assert!(event_types.contains(&"assistant.clarification.requested"));
    assert!(event_types.contains(&"assistant.notice"));
    let review = page
        .events
        .iter()
        .find(|event| event.event_type == "assistant.review.requested")
        .expect("review event should be mapped");
    assert_eq!(
        review.payload["title"],
        json!("Review [REDACTED] [REDACTED] [REDACTED]")
    );
    assert_eq!(review.payload["body"], json!("Approve [REDACTED]"));
    let clarification = page
        .events
        .iter()
        .find(|event| event.event_type == "assistant.clarification.requested")
        .expect("clarification event should be mapped");
    assert_eq!(
        clarification.payload["prompt"],
        json!("Which size uses [REDACTED]")
    );
    let notice = page
        .events
        .iter()
        .find(|event| event.event_type == "assistant.notice")
        .expect("notice event should be mapped");
    assert_eq!(
        notice.payload["body"],
        json!("Generation queued from [REDACTED]")
    );
}

#[tokio::test]
async fn list_activity_with_runtime_state_filters_run_events_by_started_session() {
    let state = runtime_state_with_harness().await;
    let requested_session_id = SessionId::new();
    let other_session_id = SessionId::new();
    let requested_run_id = RunId::new();
    let other_run_id = RunId::new();
    open_conversation_session(&state, requested_session_id).await;
    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            requested_session_id,
            &[
                Event::RunStarted(test_run_started_event(other_session_id, other_run_id)),
                Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                    at: now(),
                    delta: DeltaChunk::Text("Wrong session answer".to_owned()),
                    message_id: MessageId::new(),
                    run_id: other_run_id,
                }),
                Event::RunStarted(test_run_started_event(
                    requested_session_id,
                    requested_run_id,
                )),
                Event::AssistantDeltaProduced(AssistantDeltaProducedEvent {
                    at: now(),
                    delta: DeltaChunk::Text("Requested session answer".to_owned()),
                    message_id: MessageId::new(),
                    run_id: requested_run_id,
                }),
            ],
        )
        .await
        .expect("activity events should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(requested_session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("activity should load");
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(serialized.contains("Requested session answer"));
    assert!(!serialized.contains("Wrong session answer"));
    assert!(!payload
        .events
        .iter()
        .any(|event| event.run_id == other_run_id.to_string()));
}

#[tokio::test]
async fn list_activity_with_runtime_state_filters_tool_and_permission_events_by_started_session() {
    let state = runtime_state_with_harness().await;
    let requested_session_id = SessionId::new();
    let other_session_id = SessionId::new();
    let requested_run_id = RunId::new();
    let other_run_id = RunId::new();
    let requested_tool_use_id = ToolUseId::new();
    let other_tool_use_id = ToolUseId::new();
    let requested_request_id = RequestId::new();
    let other_request_id = RequestId::new();
    open_conversation_session(&state, requested_session_id).await;
    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            requested_session_id,
            &[
                Event::RunStarted(test_run_started_event(other_session_id, other_run_id)),
                Event::ToolUseRequested(test_tool_use_requested_event(
                    other_run_id,
                    other_tool_use_id,
                    "wrong-session-tool",
                )),
                Event::PermissionRequested(test_permission_requested_event(
                    other_session_id,
                    other_run_id,
                    other_tool_use_id,
                    other_request_id,
                    "wrong-session-permission",
                )),
                Event::RunStarted(test_run_started_event(
                    requested_session_id,
                    requested_run_id,
                )),
                Event::ToolUseRequested(test_tool_use_requested_event(
                    requested_run_id,
                    requested_tool_use_id,
                    "requested-tool",
                )),
                Event::PermissionRequested(test_permission_requested_event(
                    requested_session_id,
                    requested_run_id,
                    requested_tool_use_id,
                    requested_request_id,
                    "requested-permission",
                )),
            ],
        )
        .await
        .expect("activity events should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(requested_session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("activity should load");
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(serialized.contains("requested-tool"));
    assert!(serialized.contains("requested-permission"));
    assert!(serialized.contains(&requested_request_id.to_string()));
    assert!(!serialized.contains("wrong-session-tool"));
    assert!(!serialized.contains("wrong-session-permission"));
    assert!(!serialized.contains(&other_request_id.to_string()));
    assert!(!payload
        .events
        .iter()
        .any(|event| event.run_id == other_run_id.to_string()));
}

#[tokio::test]
async fn list_activity_with_runtime_state_redacts_permission_decision_scope_values() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let tool_use_id = ToolUseId::new();
    let request_id = RequestId::new();
    let secret_scope = "secret-internal-tool-name";
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
                Event::PermissionRequested(PermissionRequestedEvent {
                    at: now(),
                    causation_id: EventId::new(),
                    fingerprint: None,
                    interactivity: InteractivityLevel::FullyInteractive,
                    presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
                    request_id,
                    run_id,
                    scope_hint: DecisionScope::ToolName(secret_scope.to_owned()),
                    session_id,
                    severity: Severity::Low,
                    subject: PermissionSubject::CommandExec {
                        argv: vec!["pwd".to_owned()],
                        command: "pwd".to_owned(),
                        cwd: None,
                        fingerprint: None,
                    },
                    tenant_id: TenantId::SINGLE,
                    tool_name: "pwd".to_owned(),
                    tool_use_id,
                }),
            ],
        )
        .await
        .expect("activity events should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("activity should load");
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(!serialized.contains(secret_scope));
    assert!(serialized.contains("\"decisionScope\":\"this tool\""));
    assert!(serialized.contains("\"target\":\"pwd\""));
}

#[tokio::test]
async fn list_activity_with_runtime_state_redacts_file_permission_targets() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    open_conversation_session(&state, session_id).await;

    let file_write_secret = "sk-abcdefghijklmnopqrstuvwxyz";
    let file_delete_data_url = "data:text,secret";
    let file_delete_script_url = "javascript:alert(1)";
    let permission_event =
        |request_id: RequestId, tool_use_id: ToolUseId, subject: PermissionSubject| {
            Event::PermissionRequested(PermissionRequestedEvent {
                at: now(),
                causation_id: EventId::new(),
                fingerprint: None,
                interactivity: InteractivityLevel::FullyInteractive,
                presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
                request_id,
                run_id,
                scope_hint: DecisionScope::PathPrefix(PathBuf::from("workspace")),
                session_id,
                severity: Severity::Medium,
                subject,
                tenant_id: TenantId::SINGLE,
                tool_name: "file-tool".to_owned(),
                tool_use_id,
            })
        };

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::RunStarted(test_run_started_event(session_id, run_id)),
                permission_event(
                    RequestId::new(),
                    ToolUseId::new(),
                    PermissionSubject::FileWrite {
                        path: PathBuf::from(format!("workspace/{file_write_secret}")),
                        bytes_preview: b"secret".to_vec(),
                    },
                ),
                permission_event(
                    RequestId::new(),
                    ToolUseId::new(),
                    PermissionSubject::FileDelete {
                        path: PathBuf::from(format!("workspace/{file_delete_data_url}")),
                    },
                ),
                permission_event(
                    RequestId::new(),
                    ToolUseId::new(),
                    PermissionSubject::FileDelete {
                        path: PathBuf::from(format!("workspace/{file_delete_script_url}")),
                    },
                ),
            ],
        )
        .await
        .expect("activity events should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("activity should load");
    let serialized = serde_json::to_string(&payload).unwrap();
    let targets = payload
        .events
        .iter()
        .filter(|event| event.event_type == "permission.requested")
        .map(|event| event.payload["target"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();

    assert_eq!(targets.len(), 3);
    assert!(targets.iter().all(|target| target.contains("[REDACTED]")));
    assert!(!serialized.contains(file_write_secret));
    assert!(!serialized.contains(file_delete_data_url));
    assert!(!serialized.contains(file_delete_script_url));
}

#[tokio::test]
async fn get_conversation_with_runtime_state_includes_safe_client_message_id() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text("Done".to_owned()),
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let client_message_id = "00000000-0000-4000-8000-000000000001".to_owned();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: Some(client_message_id.clone()),
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Complete the task".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        let payload = get_conversation_with_runtime_state(
            GetConversationRequest {
                conversation_id: session_id.to_string(),
            },
            &state,
        )
        .await
        .expect("conversation should load");

        if let Some(message) = payload
            .conversation
            .messages
            .iter()
            .find(|message| message.author == "user")
        {
            assert_eq!(
                message.client_message_id.as_deref(),
                Some(client_message_id.as_str())
            );
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("user message should be available");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn list_activity_with_runtime_state_withholds_tool_failure_messages() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let tool_use_id = ToolUseId::new();
    let raw_error = "failed with AKIAIOSFODNN7EXAMPLE";
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
                Event::ToolUseRequested(test_tool_use_requested_event(
                    run_id,
                    tool_use_id,
                    "ReadFile",
                )),
                Event::ToolUseFailed(ToolUseFailedEvent {
                    at: now(),
                    error: ToolErrorPayload {
                        code: "execution".to_owned(),
                        message: raw_error.to_owned(),
                        retriable: false,
                    },
                    tool_use_id,
                }),
            ],
        )
        .await
        .expect("activity events should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("activity should load");
    let serialized = serde_json::to_string(&payload).unwrap();
    let failed = payload
        .events
        .iter()
        .find(|event| event.event_type == "tool.failed")
        .expect("tool failure should be projected");

    assert!(!serialized.contains(raw_error));
    assert_eq!(
        failed.payload["message"],
        json!("Tool error withheld from conversation timeline.")
    );
}

#[tokio::test]
async fn public_runtime_event_views_redact_unsafe_tool_display_labels() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let message_id = MessageId::new();
    let tool_use_id = ToolUseId::new();
    let request_id = RequestId::new();
    let unsafe_tool_name =
        "UnsafeTool https://provider.example/.jyowo /Users/alice/private data:text/plain,secret";
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
                Event::ToolUseRequested(test_tool_use_requested_event(
                    run_id,
                    tool_use_id,
                    unsafe_tool_name,
                )),
                Event::PermissionRequested(PermissionRequestedEvent {
                    request_id,
                    run_id,
                    session_id,
                    tenant_id: TenantId::SINGLE,
                    tool_use_id,
                    tool_name: unsafe_tool_name.to_owned(),
                    subject: PermissionSubject::ToolInvocation {
                        tool: unsafe_tool_name.to_owned(),
                        input: json!({}),
                    },
                    severity: Severity::Medium,
                    scope_hint: DecisionScope::ToolName(unsafe_tool_name.to_owned()),
                    fingerprint: None,
                    presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
                    interactivity: InteractivityLevel::FullyInteractive,
                    causation_id: EventId::new(),
                    at: now(),
                }),
                Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
                    run_id,
                    message_id,
                    content: MessageContent::Text("Tool requested.".to_owned()),
                    tool_uses: vec![ToolUseSummary {
                        tool_use_id,
                        tool_name: unsafe_tool_name.to_owned(),
                    }],
                    usage: UsageSnapshot::default(),
                    pricing_snapshot_id: None,
                    stop_reason: StopReason::ToolUse,
                    at: now(),
                }),
            ],
        )
        .await
        .expect("activity events should append");

    let activity = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("activity should load");
    let replay = get_replay_timeline_with_runtime_state(
        ReplayTimelineRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("replay should load");
    let timeline = page_conversation_timeline_with_runtime_state(
        PageConversationTimelineRequest {
            conversation_id: session_id.to_string(),
            after_cursor: None,
            limit: Some(20),
        },
        &state,
    )
    .await
    .expect("timeline should load");

    for serialized in [
        serde_json::to_string(&activity).unwrap(),
        serde_json::to_string(&replay).unwrap(),
        serde_json::to_string(&timeline).unwrap(),
    ] {
        assert!(!serialized.contains("provider.example"));
        assert!(!serialized.contains(".jyowo"));
        assert!(!serialized.contains("/Users/alice/private"));
        assert!(!serialized.contains("data:text/plain"));
    }
}

#[tokio::test]
async fn page_conversation_worktree_with_runtime_state_returns_safe_turn_tree() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let user_message_id = MessageId::new();
    let assistant_message_id = MessageId::new();
    let empty_assistant_message_id = MessageId::new();
    let tool_use_id = ToolUseId::new();
    let request_id = RequestId::new();
    let artifact_blob_ref = harness_contracts::BlobRef {
        id: harness_contracts::BlobId::new(),
        size: 42,
        content_hash: [7; 32],
        content_type: Some("image/png".to_owned()),
    };
    let raw_error = "failed at /Users/alice/private with token=secret-token";
    open_conversation_session(&state, session_id).await;
    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[
                Event::UserMessageAppended(UserMessageAppendedEvent {
                    run_id,
                    message_id: user_message_id,
                    content: MessageContent::Text("请生成图片".to_owned()),
                    metadata: MessageMetadata::default(),
                    attachments: Vec::new(),
                    at: now(),
                }),
                Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
                    run_id,
                    message_id: empty_assistant_message_id,
                    content: MessageContent::Text("".to_owned()),
                    tool_uses: Vec::new(),
                    usage: UsageSnapshot::default(),
                    pricing_snapshot_id: None,
                    stop_reason: StopReason::ToolUse,
                    at: now(),
                }),
                Event::ToolUseRequested(test_tool_use_requested_event(
                    run_id,
                    tool_use_id,
                    "MiniMaxTextToImage",
                )),
                Event::PermissionRequested(PermissionRequestedEvent {
                    request_id,
                    run_id,
                    session_id,
                    tenant_id: TenantId::SINGLE,
                    tool_use_id,
                    tool_name: "MiniMaxTextToImage".to_owned(),
                    subject: PermissionSubject::ToolInvocation {
                        tool: "MiniMaxTextToImage".to_owned(),
                        input: json!({ "prompt": "image generation" }),
                    },
                    severity: Severity::Medium,
                    scope_hint: DecisionScope::Any,
                    fingerprint: None,
                    presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
                    interactivity: InteractivityLevel::FullyInteractive,
                    causation_id: EventId::new(),
                    at: now(),
                }),
                Event::PermissionResolved(PermissionResolvedEvent {
                    request_id,
                    decision: Decision::AllowOnce,
                    decided_by: DecidedBy::User,
                    scope: DecisionScope::Any,
                    fingerprint: None,
                    rationale: None,
                    at: now(),
                }),
                Event::ToolUseFailed(ToolUseFailedEvent {
                    at: now(),
                    error: ToolErrorPayload {
                        code: "execution".to_owned(),
                        message: raw_error.to_owned(),
                        retriable: false,
                    },
                    tool_use_id,
                }),
                Event::AssistantMessageCompleted(AssistantMessageCompletedEvent {
                    run_id,
                    message_id: assistant_message_id,
                    content: MessageContent::Text("图片工具当前不可用。".to_owned()),
                    tool_uses: Vec::new(),
                    usage: UsageSnapshot::default(),
                    pricing_snapshot_id: None,
                    stop_reason: StopReason::EndTurn,
                    at: now(),
                }),
                Event::ArtifactCreated(ArtifactCreatedEvent {
                    artifact_id: "artifact-minimax-prompt".to_owned(),
                    at: now(),
                    blob_ref: Some(artifact_blob_ref.clone()),
                    content_hash: Some(vec![9; 32]),
                    kind: "image_prompt".to_owned(),
                    preview: Some("可复用的图像生成提示词已准备好。".to_owned()),
                    run_id,
                    session_id,
                    source: ArtifactSource::Assistant,
                    source_message_id: Some(assistant_message_id),
                    source_tool_use_id: Some(tool_use_id),
                    status: ArtifactStatus::Ready,
                    title: "海报生成提示词".to_owned(),
                }),
            ],
        )
        .await
        .expect("events should append");

    let page = page_conversation_worktree_with_runtime_state(
        PageConversationWorktreeRequest {
            conversation_id: session_id.to_string(),
            page_cursor: None,
            direction: PageConversationWorktreeDirection::After,
            limit: Some(1),
        },
        &state,
    )
    .await
    .expect("worktree should load");
    let serialized = serde_json::to_string(&page).unwrap();

    assert_eq!(page.turns.len(), 1);
    assert_eq!(page.turns[0].user.body.as_str(), "请生成图片");
    let assistant = page.turns[0].assistant.as_ref().unwrap();
    assert_eq!(assistant.id, format!("assistant:{run_id}"));
    assert!(!serialized.contains(raw_error));
    assert!(!serialized.contains("/Users/alice/private"));
    assert!(!serialized.contains(&artifact_blob_ref.id.to_string()));
    assert!(!serialized.contains("Tool error withheld from conversation timeline."));
    assert!(!serialized.contains(&empty_assistant_message_id.to_string()));

    let tool = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            harness_contracts::AssistantSegment::ToolGroup(group) => group.attempts.first(),
            _ => None,
        })
        .expect("tool attempt should be nested");
    assert_eq!(tool.tool_use_id, tool_use_id.to_string());
    assert_eq!(
        tool.permission.as_ref().unwrap().request_id,
        request_id.to_string()
    );
    assert_eq!(
        tool.failure_summary.as_ref().unwrap().as_str(),
        "工具执行失败。详情可在 Activity 中查看。"
    );

    assert!(
        assistant
            .segments
            .iter()
            .all(|segment| !matches!(segment, harness_contracts::AssistantSegment::Artifact(_))),
        "ready image artifacts should be projected inside process steps"
    );
    let artifact_step = assistant
        .segments
        .iter()
        .find_map(|segment| match segment {
            harness_contracts::AssistantSegment::Process(process) => {
                process.steps.iter().find_map(|step| match &step.detail {
                    Some(harness_contracts::ProcessStepDetail::Artifact { artifact_id, media }) => {
                        Some((step, artifact_id, media))
                    }
                    _ => None,
                })
            }
            _ => None,
        })
        .expect("process artifact step should be present");
    assert_eq!(artifact_step.0.title.as_str(), "海报生成提示词");
    assert_eq!(artifact_step.1, "artifact-minimax-prompt");
    assert_eq!(
        artifact_step.2.kind,
        harness_contracts::ArtifactMediaKind::Image
    );
}

#[tokio::test]
async fn page_conversation_worktree_with_runtime_state_rejects_malformed_conversation_id_before_runtime(
) {
    let workspace = unique_workspace("worktree-malformed-conversation-id");
    std::fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let state = DesktopRuntimeState::with_workspace_for_test(workspace)
        .expect("workspace state should initialize without a harness");

    let error = page_conversation_worktree_with_runtime_state(
        PageConversationWorktreeRequest {
            conversation_id: "not-a-session-id".to_owned(),
            page_cursor: None,
            direction: PageConversationWorktreeDirection::After,
            limit: Some(1),
        },
        &state,
    )
    .await
    .expect_err("malformed conversation id should fail closed");

    assert_eq!(error.code, "INVALID_PAYLOAD");
    assert!(
        error.message.contains("conversation session id"),
        "unexpected error message: {}",
        error.message
    );
}

#[tokio::test]
async fn list_activity_with_runtime_state_withholds_failed_run_end_reason() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Error(
        ModelError::InvalidRequest("provider failed".to_owned()),
    )])
    .await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Trigger a provider failure".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    loop {
        let payload = list_activity_with_runtime_state(
            ListActivityRequest {
                conversation_id: Some(session_id.to_string()),
                run_id: Some(started.run_id.clone()),
            },
            &state,
        )
        .await
        .unwrap();

        if let Some(run_ended) = payload
            .events
            .iter()
            .find(|event| event.event_type == "run.ended")
        {
            assert_eq!(
                run_ended.payload["reason"],
                json!("Run error withheld from conversation timeline.")
            );
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("activity should include failed run ended event");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn list_activity_with_runtime_state_redacts_private_paths_from_engine_failed_events() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let private_path = "/Users/alice/workspace/secret.txt";
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
                Event::EngineFailed(EngineFailedEvent {
                    at: now(),
                    error: EngineError::Message(format!("failed to read {private_path}")),
                    run_id: Some(run_id),
                    session_id: Some(session_id),
                }),
            ],
        )
        .await
        .expect("activity events should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("activity should load");
    let serialized = serde_json::to_string(&payload).unwrap();
    let failed = payload
        .events
        .iter()
        .find(|event| event.event_type == "engine.failed")
        .expect("engine failure should be projected");

    assert!(!serialized.contains(private_path));
    assert_eq!(
        failed.payload["message"],
        json!("Engine error withheld from conversation timeline.")
    );
}

#[tokio::test]
async fn list_activity_with_runtime_state_withholds_engine_failed_raw_message() {
    let state = runtime_state_with_harness().await;
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let raw_error = "provider error Authorization: Bearer secret-token path=/Users/alice/private";
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
                Event::EngineFailed(EngineFailedEvent {
                    at: now(),
                    error: EngineError::Message(raw_error.to_owned()),
                    run_id: Some(run_id),
                    session_id: Some(session_id),
                }),
            ],
        )
        .await
        .expect("activity events should append");

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("activity should load");
    let serialized = serde_json::to_string(&payload).unwrap();
    let failed = payload
        .events
        .iter()
        .find(|event| event.event_type == "engine.failed")
        .expect("engine failure should be projected");

    assert!(!serialized.contains(raw_error));
    assert!(!serialized.contains("secret-token"));
    assert!(!serialized.contains("/Users/alice/private"));
    assert_eq!(
        failed.payload["message"],
        json!("Engine error withheld from conversation timeline.")
    );
}

#[tokio::test]
async fn list_activity_with_runtime_state_redacts_pending_permission_display_text() {
    let secret_command =
        "git push https://ghp_abcdefghijklmnopqrstuvwxyz0123456789@github.com/org/repo";
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "NeedsPermission".to_owned(),
                input: json!({ "command": secret_command }),
            },
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Run a command".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let pending = wait_for_pending_permission_for_session(&state, session_id).await;
    let request_id = pending.request.request_id;

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(started.run_id),
        },
        &state,
    )
    .await
    .unwrap();
    let value = serde_json::to_string(&payload).unwrap();

    assert!(!value.contains("ghp_abcdefghijklmnopqrstuvwxyz0123456789"));
    assert!(!value.contains(secret_command));
    assert!(value.contains("\"target\":\"git\""));

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Deny,
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn get_replay_timeline_with_runtime_state_reads_redacted_journal_events_without_running_tools(
) {
    let secret_command =
        "git push https://ghp_abcdefghijklmnopqrstuvwxyz0123456789@github.com/org/repo";
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "NeedsPermission".to_owned(),
                input: json!({ "command": secret_command }),
            },
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Run a command".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let pending = wait_for_pending_permission_for_session(&state, session_id).await;
    let request_id = pending.request.request_id;

    let payload = get_replay_timeline_with_runtime_state(
        ReplayTimelineRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(started.run_id.clone()),
        },
        &state,
    )
    .await
    .unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(payload.replayed);
    assert!(payload
        .events
        .iter()
        .any(|event| event.event_type == "run.started"));
    assert!(payload
        .events
        .iter()
        .any(|event| event.event_type == "permission.requested"));
    assert!(!serialized.contains("ghp_abcdefghijklmnopqrstuvwxyz0123456789"));
    assert!(!serialized.contains(secret_command));
    assert!(serialized.contains("\"target\":\"git\""));
    assert_eq!(
        state.pending_permission_requests().len(),
        1,
        "replay read mode must not resolve or execute pending tools"
    );

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Deny,
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn get_replay_timeline_with_runtime_state_reads_beyond_first_event_page() {
    let mut stream_events = (0..205)
        .map(|index| ModelStreamEvent::ContentBlockDelta {
            index,
            delta: ContentDelta::Text(format!("delta-{index}")),
        })
        .collect::<Vec<_>>();
    stream_events.push(ModelStreamEvent::MessageStop);
    let state =
        runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(stream_events)]).await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Write many deltas".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let request = ReplayTimelineRequest {
        conversation_id: Some(session_id.to_string()),
        run_id: Some(started.run_id),
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

    loop {
        let payload = get_replay_timeline_with_runtime_state(request.clone(), &state)
            .await
            .unwrap();
        let serialized = serde_json::to_string(&payload).unwrap();
        if payload.events.len() > 200 && serialized.contains("delta-204") {
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("replay timeline should include events past the first page");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

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
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
}

#[cfg(unix)]
#[test]
fn desktop_provider_settings_store_rejects_symlink_settings_file() {
    let workspace = unique_workspace("provider-settings-symlink-file");
    let external = unique_workspace("provider-settings-external-target");
    let settings_dir = workspace.join(".jyowo").join("runtime");
    let settings_path = settings_dir.join("provider-settings.json");
    std::fs::create_dir_all(&settings_dir).unwrap();
    std::fs::create_dir_all(&external).unwrap();
    std::os::unix::fs::symlink(external.join("provider-settings.json"), &settings_path).unwrap();
    let store = DesktopProviderSettingsStore::new(workspace);

    let error = store.load_record().unwrap_err();
    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");

    let error = store
        .save_record(&ProviderSettingsRecord {
            default_config_id: Some("openai".to_owned()),
            configs: vec![ProviderConfigRecord {
                api_key: String::new(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI".to_owned(),
                id: "openai".to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            }],
        })
        .unwrap_err();

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert!(!external.join("provider-settings.json").exists());
}

#[cfg(unix)]
#[test]
fn desktop_skill_store_rejects_symlink_index_file() {
    let workspace = unique_workspace("skill-store-symlink-index");
    let external = unique_workspace("skill-store-external-target");
    let index_dir = workspace.join(".jyowo").join("runtime").join("skills");
    let index_path = index_dir.join("index.json");
    std::fs::create_dir_all(&index_dir).unwrap();
    std::fs::create_dir_all(&external).unwrap();
    std::fs::write(external.join("index.json"), "[]").unwrap();
    std::os::unix::fs::symlink(external.join("index.json"), &index_path).unwrap();
    let store = DesktopSkillStore::new(workspace);

    let error = store.load_records().unwrap_err();

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert!(error.message.contains("must not use symlinks"));
}

#[cfg(unix)]
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
async fn list_activity_with_runtime_state_does_not_expose_other_conversation_pending_permissions() {
    let state = runtime_state_with_harness().await;
    let other_session_id = SessionId::new();
    open_conversation_session(&state, other_session_id).await;
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let request = permission_request();
    let request_id = request.request_id;
    let request_session_id = request.session_id;

    let decision_task =
        tokio::spawn(async move { broker.decide(request, permission_context()).await });

    wait_for_pending_permission(&state, request_id).await;

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(other_session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .unwrap();

    assert!(payload.events.is_empty());

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Deny,
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(decision_task.await.unwrap(), Decision::DenyOnce);
}

#[tokio::test]
async fn list_activity_with_runtime_state_rejects_conflicting_conversation_and_run_filters() {
    let state = runtime_state_with_harness().await;
    let other_session_id = SessionId::new();
    open_conversation_session(&state, other_session_id).await;
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let request = permission_request();
    let request_id = request.request_id;
    let request_session_id = request.session_id;
    let run_id = RunId::new();
    let run_id_string = run_id.to_string();

    let decision_task = tokio::spawn(async move {
        broker
            .decide(request, permission_context_with_run_id(Some(run_id)))
            .await
    });

    wait_for_pending_permission(&state, request_id).await;

    let payload = list_activity_with_runtime_state(
        ListActivityRequest {
            conversation_id: Some(other_session_id.to_string()),
            run_id: Some(run_id_string),
        },
        &state,
    )
    .await
    .unwrap();

    assert!(payload.events.is_empty());

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: request_session_id.to_string(),
            decision: PermissionDecision::Deny,
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(decision_task.await.unwrap(), Decision::DenyOnce);
}

#[tokio::test]
async fn runtime_state_rejects_harness_and_stream_permission_runtime_from_different_brokers() {
    let workspace = unique_workspace("mismatched-broker");
    std::fs::create_dir_all(&workspace).unwrap();
    let harness_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let state_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let harness = Arc::new(
        Harness::builder()
            .with_options(test_harness_options(&workspace))
            .with_model(MockProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_stream_permission_broker_arc(
                harness_runtime.broker(),
                harness_runtime.resolver_handle(),
            )
            .build()
            .await
            .expect("harness should build with stream permission runtime"),
    );

    let error = match DesktopRuntimeState::with_harness_and_stream_permission_runtime(
        harness,
        state_runtime,
    ) {
        Ok(_) => panic!("state should reject mismatched permission broker origins"),
        Err(error) => error,
    };

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");
}

#[test]
fn list_activity_payload_returns_empty_typed_event_list() {
    let payload = list_activity_payload(ListActivityRequest {
        conversation_id: Some("conversation-001".to_owned()),
        run_id: None,
    })
    .unwrap();

    assert!(payload.events.is_empty());

    let error = list_activity_payload(ListActivityRequest {
        conversation_id: Some(String::new()),
        run_id: None,
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

fn permission_request() -> PermissionRequest {
    permission_request_with_subject(PermissionSubject::CommandExec {
        command: "pwd".to_owned(),
        argv: vec!["pwd".to_owned()],
        cwd: None,
        fingerprint: None,
    })
}

struct NeedsPermissionTool {
    descriptor: ToolDescriptor,
}

impl Default for NeedsPermissionTool {
    fn default() -> Self {
        Self::named("NeedsPermission", "NeedsPermission")
    }
}

impl NeedsPermissionTool {
    fn named(name: &str, display_name: &str) -> Self {
        Self {
            descriptor: ToolDescriptor {
                name: name.to_owned(),
                display_name: display_name.to_owned(),
                description: "Requests command permission for desktop tests.".to_owned(),
                category: "test".to_owned(),
                group: ToolGroup::Custom("test".to_owned()),
                version: "0.1.0".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" }
                    },
                    "required": ["command"]
                }),
                output_schema: None,
                dynamic_schema: false,
                properties: ToolProperties {
                    is_concurrency_safe: true,
                    is_read_only: false,
                    is_destructive: false,
                    long_running: None,
                    defer_policy: DeferPolicy::AlwaysLoad,
                },
                trust_level: TrustLevel::UserControlled,
                required_capabilities: Vec::new(),
                budget: ResultBudget {
                    metric: BudgetMetric::Chars,
                    limit: 30_000,
                    on_overflow: OverflowAction::Offload,
                    preview_head_chars: 2_000,
                    preview_tail_chars: 2_000,
                },
                provider_restriction: ProviderRestriction::All,
                origin: jyowo_harness_sdk::ext::ToolOrigin::Builtin,
                search_hint: None,
            },
        }
    }
}

#[async_trait]
impl Tool for NeedsPermissionTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn check_permission(&self, input: &Value, _ctx: &ToolContext) -> PermissionCheck {
        let command = input
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("needs-permission")
            .to_owned();

        PermissionCheck::AskUser {
            subject: PermissionSubject::CommandExec {
                command: command.clone(),
                argv: vec![command.clone()],
                cwd: None,
                fingerprint: None,
            },
            scope: DecisionScope::ExactCommand { command, cwd: None },
        }
    }

    async fn execute(&self, _input: Value, _ctx: ToolContext) -> Result<ToolStream, ToolError> {
        Ok(Box::pin(stream::iter(vec![ToolEvent::Final(
            ToolResult::Text("done".to_owned()),
        )])))
    }
}

fn permission_request_with_subject(subject: PermissionSubject) -> PermissionRequest {
    let tenant_id = TenantId::SHARED;
    let session_id = SessionId::new();

    PermissionRequest {
        request_id: RequestId::new(),
        tenant_id,
        session_id,
        tool_use_id: ToolUseId::new(),
        tool_name: "shell".to_owned(),
        subject,
        severity: Severity::Low,
        scope_hint: DecisionScope::ToolName("shell".to_owned()),
        created_at: now(),
    }
}

fn permission_context() -> PermissionContext {
    permission_context_with_run_id(None)
}

fn permission_context_with_run_id(run_id: Option<RunId>) -> PermissionContext {
    PermissionContext {
        permission_mode: PermissionMode::Default,
        previous_mode: None,
        session_id: SessionId::new(),
        tenant_id: TenantId::SHARED,
        run_id,
        interactivity: InteractivityLevel::FullyInteractive,
        timeout_policy: None,
        fallback_policy: FallbackPolicy::AskUser,
        rule_snapshot: Arc::new(RuleSnapshot {
            rules: Vec::new(),
            generation: 0,
            built_at: now(),
        }),
        hook_overrides: Vec::new(),
    }
}

fn test_memory_record(session_id: SessionId, content: &str) -> MemoryRecord {
    MemoryRecord {
        id: MemoryId::new(),
        tenant_id: TenantId::SINGLE,
        kind: MemoryKind::UserPreference,
        visibility: MemoryVisibility::Private { session_id },
        content: content.to_owned(),
        metadata: MemoryMetadata {
            tags: Vec::new(),
            source: MemorySource::UserInput,
            confidence: 1.0,
            access_count: 0,
            last_accessed_at: None,
            recall_score: 1.0,
            ttl: None,
            redacted_segments: 0,
        },
        created_at: now(),
        updated_at: now(),
    }
}

async fn wait_for_pending_permission(state: &DesktopRuntimeState, request_id: RequestId) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        if state
            .pending_permission_requests()
            .iter()
            .any(|pending| pending.request.request_id == request_id)
        {
            return;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("permission request should become pending");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

async fn wait_for_pending_permission_for_session(
    state: &DesktopRuntimeState,
    session_id: SessionId,
) -> jyowo_harness_sdk::ext::PendingPermissionRequest {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(pending) = state
            .pending_permission_requests()
            .into_iter()
            .find(|pending| pending.request.session_id == session_id)
        {
            return pending;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("permission request should become pending for session");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

async fn open_conversation_session(state: &DesktopRuntimeState, session_id: SessionId) {
    state
        .harness()
        .expect("runtime state should retain the configured harness")
        .open_or_create_conversation_session(state.conversation_session_options(session_id))
        .await
        .expect("conversation session should open");
}

fn test_run_started_event(session_id: SessionId, run_id: RunId) -> RunStartedEvent {
    RunStartedEvent {
        correlation_id: CorrelationId::new(),
        effective_config_hash: ConfigHash([0; 32]),
        input: TurnInput {
            message: Message {
                created_at: now(),
                id: MessageId::new(),
                parts: vec![MessagePart::Text("Test run".to_owned())],
                role: MessageRole::User,
            },
            metadata: json!({}),
        },
        parent_run_id: None,
        run_id,
        session_id,
        snapshot_id: SnapshotId::new(),
        started_at: now(),
        tenant_id: TenantId::SINGLE,
    }
}

fn test_tool_use_requested_event(
    run_id: RunId,
    tool_use_id: ToolUseId,
    tool_name: &str,
) -> ToolUseRequestedEvent {
    ToolUseRequestedEvent {
        at: now(),
        causation_id: EventId::new(),
        input: json!({ "toolName": tool_name }),
        properties: ToolProperties {
            is_concurrency_safe: true,
            is_destructive: false,
            is_read_only: false,
            long_running: None,
            defer_policy: DeferPolicy::AlwaysLoad,
        },
        run_id,
        tool_name: tool_name.to_owned(),
        tool_use_id,
    }
}

fn test_permission_requested_event(
    session_id: SessionId,
    run_id: RunId,
    tool_use_id: ToolUseId,
    request_id: RequestId,
    tool_name: &str,
) -> PermissionRequestedEvent {
    PermissionRequestedEvent {
        at: now(),
        causation_id: EventId::new(),
        fingerprint: None,
        interactivity: InteractivityLevel::FullyInteractive,
        presented_options: vec![Decision::AllowOnce, Decision::DenyOnce],
        request_id,
        run_id,
        scope_hint: DecisionScope::ToolName(tool_name.to_owned()),
        session_id,
        severity: Severity::Low,
        subject: PermissionSubject::CommandExec {
            argv: vec![tool_name.to_owned()],
            command: tool_name.to_owned(),
            cwd: None,
            fingerprint: None,
        },
        tenant_id: TenantId::SINGLE,
        tool_name: tool_name.to_owned(),
        tool_use_id,
    }
}

async fn runtime_state_with_harness() -> DesktopRuntimeState {
    runtime_state_with_harness_for_workspace(unique_workspace("harness")).await
}

async fn runtime_state_with_harness_for_workspace(workspace: PathBuf) -> DesktopRuntimeState {
    std::fs::create_dir_all(&workspace).unwrap();
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let harness = Arc::new(
        Harness::builder()
            .with_options(test_harness_options(&workspace))
            .with_model(MockProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_stream_permission_broker_arc(
                stream_permission_runtime.broker(),
                stream_permission_runtime.resolver_handle(),
            )
            .build()
            .await
            .expect("harness should build with stream permission runtime"),
    );

    DesktopRuntimeState::with_harness_and_stream_permission_runtime_for_workspace(
        workspace,
        harness,
        stream_permission_runtime,
    )
    .expect("state should use the harness permission broker")
}

async fn runtime_state_with_memory_provider(
    provider: Arc<MockMemoryProvider>,
) -> DesktopRuntimeState {
    let workspace = unique_workspace("memory-provider");
    std::fs::create_dir_all(&workspace).unwrap();
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let harness = Arc::new(
        Harness::builder()
            .with_options(test_harness_options(&workspace))
            .with_model(MockProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_stream_permission_broker_arc(
                stream_permission_runtime.broker(),
                stream_permission_runtime.resolver_handle(),
            )
            .with_memory_provider_arc(provider)
            .build()
            .await
            .expect("harness should build with memory provider"),
    );

    DesktopRuntimeState::with_harness_and_stream_permission_runtime_for_workspace(
        workspace,
        harness,
        stream_permission_runtime,
    )
    .expect("state should use the harness permission broker")
}

async fn runtime_state_with_mcp_registry(
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

async fn runtime_state_with_mcp_registry_for_workspace(
    workspace: PathBuf,
    registry: McpRegistry,
    server_ids_to_inject: Vec<McpServerId>,
) -> DesktopRuntimeState {
    std::fs::create_dir_all(&workspace).unwrap();
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let harness = Arc::new(
        Harness::builder()
            .with_options(test_harness_options(&workspace))
            .with_model(MockProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_stream_permission_broker_arc(
                stream_permission_runtime.broker(),
                stream_permission_runtime.resolver_handle(),
            )
            .with_mcp_config(McpConfig {
                registry,
                server_ids_to_inject,
            })
            .build()
            .await
            .expect("harness should build with MCP registry"),
    );

    DesktopRuntimeState::with_harness_and_stream_permission_runtime_for_workspace(
        workspace,
        harness,
        stream_permission_runtime,
    )
    .expect("state should use the harness permission broker")
}

async fn runtime_state_with_scripted_model(
    responses: Vec<ScriptedResponse>,
) -> DesktopRuntimeState {
    runtime_state_with_scripted_model_for_workspace(unique_workspace("scripted-model"), responses)
        .await
}

async fn runtime_state_with_scripted_model_for_workspace(
    workspace: PathBuf,
    responses: Vec<ScriptedResponse>,
) -> DesktopRuntimeState {
    std::fs::create_dir_all(&workspace).unwrap();
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let harness = Arc::new(
        Harness::builder()
            .with_options(test_harness_options(&workspace))
            .with_model_arc(Arc::new(ScriptedProvider::new(responses)))
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_stream_permission_broker_arc(
                stream_permission_runtime.broker(),
                stream_permission_runtime.resolver_handle(),
            )
            .with_tool_registry(
                ToolRegistry::builder()
                    .with_tool(Box::<NeedsPermissionTool>::default())
                    .build()
                    .expect("test tool registry should build"),
            )
            .build()
            .await
            .expect("harness should build with stream permission runtime"),
    );

    DesktopRuntimeState::with_harness_and_stream_permission_runtime_for_workspace(
        workspace,
        harness,
        stream_permission_runtime,
    )
    .expect("state should use the harness permission broker")
}

fn test_harness_options(workspace: &Path) -> HarnessOptions {
    let mut options = HarnessOptions::default();
    options.workspace_root = workspace.to_path_buf();
    options.model_id = "mock-model".to_owned();
    options
}

fn unique_workspace(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-desktop-{name}-{}-{}",
        std::process::id(),
        SessionId::new()
    ))
}

fn test_attachment_blob_ref(size: u64, content_type: &str) -> AttachmentBlobRefPayload {
    AttachmentBlobRefPayload {
        id: "01J00000000000000000000000".to_owned(),
        size,
        content_hash: [1; 32],
        content_type: Some(content_type.to_owned()),
    }
}

fn skill_markdown(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\nSkill body for {name}.\n")
}

fn write_skill_package(
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

fn register_test_skill(state: &DesktopRuntimeState, name: &str, description: &str) {
    let harness = state
        .harness()
        .expect("runtime state should include harness");
    let skill = parse_skill_markdown(
        &skill_markdown(name, description),
        SkillSource::Workspace("data/skills".into()),
        None,
        SkillPlatform::Macos,
    )
    .expect("test skill should parse");
    harness
        .skill_registry()
        .register_batch(vec![skill])
        .expect("test skill should register");
}

fn register_test_tool(state: &DesktopRuntimeState, name: &str, display_name: &str) {
    let harness = state
        .harness()
        .expect("runtime state should include harness");
    harness
        .tool_registry()
        .register(Box::new(NeedsPermissionTool::named(name, display_name)))
        .expect("test tool should register");
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &std::ffi::OsStr) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
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

fn stdio_mcp_fixture_script() -> String {
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

struct StaticMcpConnection {
    tools: Vec<McpToolDescriptor>,
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
struct RecordingProviderSettingsStore {
    fail_record: bool,
    record: Mutex<Option<ProviderSettingsRecord>>,
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
struct RecordingMcpServerStore {
    record: Mutex<Option<McpServerConfigRecord>>,
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

#[test]
fn context_file_payload_skips_missing_optional_state() {
    let value = serde_json::to_value(jyowo_desktop_shell::commands::ContextFilePayload {
        label: "apps/desktop/src/main.tsx".to_owned(),
        state: None,
    })
    .unwrap();

    assert_eq!(
        value,
        json!({
            "label": "apps/desktop/src/main.tsx"
        })
    );
}

#[tokio::test]
async fn get_context_snapshot_with_runtime_state_returns_workspace_metadata_without_session() {
    let workspace = unique_workspace("context-snapshot-no-session");
    std::fs::create_dir_all(workspace.join("apps/desktop/src")).unwrap();
    std::fs::write(
        workspace.join("apps/desktop/src/main.tsx"),
        "console.log('ready')",
    )
    .unwrap();
    let state = runtime_state_with_harness_for_workspace(workspace.clone()).await;
    let session_id = SessionId::new();

    let context = get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("missing conversation events should still return workspace metadata");

    assert_eq!(
        context.project,
        workspace.file_name().unwrap().to_string_lossy()
    );
    assert_eq!(context.path, "workspace://local");
    assert!(context.active_artifact.is_none());
    assert!(context.decisions.is_empty());
    assert_eq!(context.next_actions, vec!["Continue the conversation"]);
    assert_eq!(
        context.files,
        vec![jyowo_desktop_shell::commands::ContextFilePayload {
            label: "apps/desktop/src/main.tsx".to_owned(),
            state: Some("ready"),
        }]
    );
}

#[tokio::test]
async fn get_context_snapshot_with_runtime_state_does_not_project_assistant_reply_as_artifact() {
    let workspace = unique_workspace("context-snapshot");
    std::fs::create_dir_all(workspace.join("apps/desktop/src")).unwrap();
    std::fs::write(
        workspace.join("apps/desktop/src/main.tsx"),
        "export const app = 'jyowo';",
    )
    .unwrap();
    let state = runtime_state_with_scripted_model_for_workspace(
        workspace.clone(),
        vec![ScriptedResponse::Stream(vec![
            ModelStreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::Text("# Runtime context artifact\n\nReady.".to_owned()),
            },
            ModelStreamEvent::MessageStop,
        ])],
    )
    .await;
    let session_id = state.default_conversation_id();

    start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Create a context artifact".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

    loop {
        let conversation = get_conversation_with_runtime_state(
            GetConversationRequest {
                conversation_id: session_id.to_string(),
            },
            &state,
        )
        .await
        .expect("runtime conversation should load");
        if conversation
            .conversation
            .messages
            .iter()
            .any(|message| message.body.contains("Runtime context artifact"))
        {
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("runtime assistant output should complete");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    let payload = get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("runtime context snapshot should load");

    assert_eq!(payload.active_artifact, None);
    assert_eq!(
        payload.project,
        workspace.file_name().unwrap().to_string_lossy()
    );
    assert_eq!(payload.path, "workspace://local");
    assert!(payload
        .files
        .iter()
        .any(|file| { file.label == "apps/desktop/src/main.tsx" && file.state == Some("ready") }));
    assert!(payload
        .next_actions
        .iter()
        .any(|action| action == "Continue the conversation"));
}

#[tokio::test]
async fn get_context_snapshot_with_runtime_state_exposes_pending_decisions() {
    let state = runtime_state_with_scripted_model(vec![ScriptedResponse::Stream(vec![
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: "NeedsPermission".to_owned(),
                input: json!({ "command": "pwd" }),
            },
        },
        ModelStreamEvent::MessageStop,
    ])])
    .await;
    let session_id = SessionId::new();
    let started = start_run_with_runtime_state(
        StartRunRequest {
            client_message_id: None,
            attachments: None,
            context_references: None,
            conversation_id: session_id.to_string(),
            prompt: "Run a command".to_owned(),
        },
        &state,
    )
    .await
    .expect("start_run should start a conversation run");
    let pending = wait_for_pending_permission_for_session(&state, session_id).await;

    let payload = get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(started.run_id),
        },
        &state,
    )
    .await
    .expect("runtime context snapshot should load pending decisions");

    assert!(payload.decisions.iter().any(|decision| {
        decision.title == "Approve NeedsPermission"
            && decision
                .detail
                .contains(&pending.request.request_id.to_string())
            && decision.request_id.as_deref() == Some(&pending.request.request_id.to_string())
    }));

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Deny,
            request_id: pending.request.request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn get_context_snapshot_with_runtime_state_redacts_pending_decision_tool_names() {
    let state = runtime_state_with_harness().await;
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let broker = harness
        .permission_broker()
        .expect("harness should use the stream permission broker");
    let session_id = state.default_conversation_id();
    let run_id = RunId::new();
    open_conversation_session(&state, session_id).await;
    let mut request = permission_request();
    request.session_id = session_id;
    request.tool_name = "sk-abcdefghijklmnopqrstuvwxyz".to_owned();
    let request_id = request.request_id;
    let expected_title = format!(
        "Approve {}",
        DefaultRedactor::default().redact(
            &request.tool_name,
            &RedactRules {
                scope: RedactScope::EventBody,
                replacement: "[REDACTED]".to_owned(),
                pattern_set: RedactPatternSet::AllBuiltins,
            },
        )
    );

    let decision_task = tokio::spawn(async move {
        broker
            .decide(request, permission_context_with_run_id(Some(run_id)))
            .await
    });
    wait_for_pending_permission(&state, request_id).await;

    let payload = get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: Some(run_id.to_string()),
        },
        &state,
    )
    .await
    .expect("runtime context snapshot should load pending decisions");
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(payload
        .decisions
        .iter()
        .any(|decision| decision.title == expected_title));
    assert!(!serialized.contains("sk-abcdefghijklmnopqrstuvwxyz"));

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            conversation_id: session_id.to_string(),
            decision: PermissionDecision::Deny,
            request_id: request_id.to_string(),
        },
        &state,
    )
    .await
    .unwrap();
    assert_eq!(decision_task.await.unwrap(), Decision::DenyOnce);
}

#[tokio::test]
async fn get_context_snapshot_with_runtime_state_redacts_workspace_display_fields() {
    let secret_workspace_segment = "sk-abcdefghijklmnopqrstuvwxyz";
    let workspace = unique_workspace(&format!("context-snapshot-{secret_workspace_segment}"));
    let state = runtime_state_with_harness_for_workspace(workspace).await;
    let session_id = state.default_conversation_id();
    open_conversation_session(&state, session_id).await;

    let payload = get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id: Some(session_id.to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("runtime context snapshot should load workspace display fields");
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(!serialized.contains(secret_workspace_segment));
    assert_eq!(payload.path, "workspace://local");
    assert!(payload.project.contains("[REDACTED]"));
}

#[tokio::test]
async fn get_context_snapshot_with_runtime_state_hides_runtime_read_errors() {
    let state = runtime_state_with_harness().await;

    let payload = get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id: Some(state.default_conversation_id().to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect("missing conversation session should still return workspace metadata");

    assert_eq!(
        payload.project,
        state
            .workspace_root()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap()
    );
    assert_eq!(payload.path, "workspace://local");
    assert!(payload.files.is_empty());
    assert!(payload.decisions.is_empty());
}

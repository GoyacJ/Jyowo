use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use futures::stream;
use harness_contracts::{
    AssistantClarificationRequestedEvent, AssistantDeltaProducedEvent,
    AssistantMessageCompletedEvent, AssistantNoticeEvent, AssistantReviewRequestedEvent,
    AutomationRunStatus, AutomationSchedule, AutomationSpec, AutomationWorkspaceScope,
    CapabilityRouteKind, ConfigHash, ConversationAttachmentReference, CorrelationId, DecidedBy,
    EngineError, EngineFailedEvent, EventId, McpConnectionLostEvent, McpConnectionLostReason,
    MessageContent, MessageId, MessageMetadata, MissedRunPolicy, ModelModality,
    PermissionRequestedEvent, PermissionResolvedEvent, ProviderCapabilityRoute,
    ProviderCapabilityRouteSettings, ProviderServiceAdapterAvailability, ReasoningSummaryChunk,
    RunStartedEvent, SandboxMode, SnapshotId, StopReason, ToolErrorPayload, ToolServiceBinding,
    ToolUseFailedEvent, ToolUseRequestedEvent, ToolUseSummary, TurnInput, UiSafeText,
    UserMessageAppendedEvent, WorkspaceAccess,
};
use harness_skill::{parse_skill_markdown, SkillPlatform, SkillSource};
use harness_tool::BuiltinToolset;
use image::codecs::{gif::GifEncoder, jpeg::JpegEncoder, webp::WebPEncoder};
use image::{ExtendedColorType, ImageEncoder};
use jyowo_desktop_shell::commands::{
    cancel_run_payload, cancel_run_with_runtime_state,
    create_attachment_from_path_with_runtime_state, create_conversation_with_runtime_state,
    delete_automation_with_runtime_state, delete_conversation_with_runtime_state,
    delete_mcp_server_with_runtime_state, delete_mcp_server_with_store,
    delete_memory_item_with_runtime_state, delete_provider_capability_route_with_store,
    delete_skill_with_runtime_state, desktop_provider_credential_resolver_with_stores,
    export_memory_items_with_runtime_state, export_support_bundle_with_runtime_state,
    get_app_info_payload, get_artifact_media_preview_with_runtime_state,
    get_attachment_media_preview_with_runtime_state, get_context_snapshot_with_runtime_state,
    get_conversation_with_runtime_state, get_execution_settings_for_request,
    get_execution_settings_with_store, get_mcp_server_config_with_runtime_state,
    get_mcp_server_config_with_store, get_memory_item_with_runtime_state,
    get_provider_config_api_key_with_runtime_state, get_provider_config_api_key_with_store,
    get_replay_timeline_with_runtime_state, get_skill_detail_with_runtime_state,
    get_skill_file_with_runtime_state, harness_healthcheck_payload,
    import_skill_with_runtime_state, list_activity_payload, list_activity_with_runtime_state,
    list_artifacts_with_runtime_state, list_automation_runs_with_runtime_state,
    list_automations_with_runtime_state, list_browser_mcp_presets_with_store,
    list_conversations_with_runtime_state, list_eval_cases_payload,
    list_eval_cases_with_runtime_state, list_mcp_diagnostics_with_store,
    list_mcp_servers_with_runtime_state, list_memory_items_with_runtime_state,
    list_model_provider_catalog_payload, list_provider_capability_route_options_from_inputs,
    list_provider_capability_routes_with_store, list_provider_settings_with_store,
    list_reference_candidates_with_runtime_state, list_skills_with_runtime_state,
    mcp_diagnostic_record_from_event, page_conversation_timeline_with_runtime_state,
    page_conversation_worktree_with_runtime_state,
    request_provider_config_api_key_reveal_with_runtime_state,
    request_provider_config_api_key_reveal_with_store,
    resolve_permission_for_window_with_runtime_state, resolve_permission_payload,
    resolve_permission_with_runtime_state, restart_mcp_server_with_runtime_state,
    run_automation_now_with_runtime_state, run_due_automations_once_with_runtime_state,
    run_eval_case_payload, run_eval_case_with_runtime_state, runtime_state_async,
    runtime_state_for_workspace, save_automation_with_runtime_state,
    save_browser_mcp_preset_with_store, save_mcp_server_with_runtime_state,
    save_mcp_server_with_store, save_provider_capability_route_settings_with_store,
    save_provider_capability_route_with_store, save_provider_settings_with_runtime_state,
    save_provider_settings_with_store, set_conversation_model_config_with_runtime_state,
    set_execution_settings_with_store, set_mcp_server_enabled_with_runtime_state,
    set_skill_enabled_with_runtime_state, spawn_automation_scheduler,
    spawn_automation_scheduler_on_tauri_runtime, start_run_payload, start_run_with_runtime_state,
    subscribe_conversation_events_for_window_with_runtime_state,
    unsubscribe_conversation_events_for_window_with_runtime_state,
    update_memory_item_with_runtime_state, validate_provider_settings_payload,
    ArtifactSummaryPayload, AttachmentBlobRefPayload, AttachmentReferencePayload,
    BrowserMcpPresetId, CancelRunRequest, ContextReferencePayload, ConversationEventBatchPayload,
    ConversationModelCapabilityRecord, CreateAttachmentFromPathRequest, DeleteConversationRequest,
    DeleteMcpServerRequest, DeleteMemoryItemRequest, DeleteProviderCapabilityRouteRequest,
    DeleteSkillRequest, DesktopConversationModelConfigStore, DesktopExecutionSettingsStore,
    DesktopMcpDiagnosticStore, DesktopProviderCapabilityRouteStore, DesktopProviderSettingsStore,
    DesktopRuntimeState, DesktopSkillStore, ExportSupportBundleRequest,
    GetArtifactMediaPreviewRequest, GetAttachmentMediaPreviewRequest, GetContextSnapshotRequest,
    GetConversationRequest, GetExecutionSettingsRequest, GetMcpServerConfigRequest,
    GetMemoryItemRequest, GetProviderConfigApiKeyRequest, GetSkillDetailRequest,
    GetSkillFileRequest, ImportSkillRequest, ListActivityRequest, ListArtifactsRequest,
    ListReferenceCandidatesRequest, McpDiagnosticRecord, McpDiagnosticSeverity, McpDiagnosticStore,
    McpHeaderEnvRecord, McpNameValueRecord, McpServerConfigRecord, McpServerStore,
    McpServerTransportConfig, PageConversationTimelineRequest, PageConversationWorktreeDirection,
    PageConversationWorktreeRequest, PermissionDecision, ProviderCapabilityRouteStore,
    ProviderConfigRecord, ProviderModelDescriptorRecord, ProviderModelLifecycleRecord,
    ProviderModelModalityRecord, ProviderSettingsRecord, ProviderSettingsRequest,
    ProviderSettingsStore, ReplayTimelineRequest, RequestProviderConfigApiKeyRevealRequest,
    ResolvePermissionRequest, RestartMcpServerRequest, RunEvalCaseRequest, SaveAutomationRequest,
    SaveBrowserMcpPresetRequest, SaveMcpServerRequest, SaveProviderCapabilityRouteRequest,
    SetAutomationEnabledRequest, SetConversationModelConfigRequest, SetExecutionSettingsRequest,
    SetMcpServerEnabledRequest, SetSkillEnabledRequest, SkillStore, SkillStoreRecord,
    StartRunRequest, SubscribeConversationEventsRequest, UnsubscribeConversationEventsRequest,
    UpdateMemoryItemRequest, ValidateProviderSettingsRequest,
};
use jyowo_desktop_shell::project_registry::ProjectRegistry;
use jyowo_harness_sdk::builtin::{DefaultRedactor, FileBlobStore};
use jyowo_harness_sdk::ext::{
    now, ArtifactCreatedEvent, ArtifactSource, ArtifactStatus, ArtifactUpdatedEvent, BlobMeta,
    BlobRetention, BlobStore, BudgetMetric, Decision, DecisionScope, DeferPolicy, DeltaChunk,
    Event, EventStore, FallbackPolicy, InteractivityLevel, McpConnection, McpError, McpRegistry,
    McpServerId, McpServerScope, McpServerSource, McpServerSpec, McpToolDescriptor, McpToolResult,
    MemoryId, MemoryKind, MemoryMetadata, MemoryRecord, MemorySource, MemoryStore,
    MemoryVisibility, Message, MessagePart, MessageRole, ModelError, ModelProtocol, OverflowAction,
    PermissionCheck, PermissionContext, PermissionMode, PermissionRequest, PermissionSubject,
    ProviderCredentialResolveContext, ProviderRestriction, RedactPatternSet, RedactRules,
    RedactScope, Redactor, RequestId, ResultBudget, RuleSnapshot, RunId, SessionId, Severity,
    StreamBrokerConfig, TenantId, ThinkingDelta, Tool, ToolCapability, ToolContext, ToolDescriptor,
    ToolError, ToolEvent, ToolGroup, ToolProfile, ToolProperties, ToolRegistry, ToolResult,
    ToolStream, ToolUseId, TransportChoice, TrustLevel, UsageSnapshot, ValidationError,
};
use jyowo_harness_sdk::ext::{ContentDelta, ModelStreamEvent};
use jyowo_harness_sdk::testing::{
    InMemoryEventStore, InMemoryMemoryProvider, NoopRedactor, NoopSandbox, ScriptedProvider,
    ScriptedResponse, TestModelProvider,
};
use jyowo_harness_sdk::{
    ConversationEventsPageRequest, Harness, HarnessOptions, McpConfig, StreamPermissionRuntime,
};
use parking_lot::RwLock as ParkingRwLock;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static WORKSPACE_ROOT_ENV_LOCK: Mutex<()> = Mutex::new(());
static HOME_ENV_LOCK: Mutex<()> = Mutex::new(());
const WORKSPACE_ROOT_ENV: &str = "JYOWO_WORKSPACE_ROOT";
const HOME_ENV: &str = "HOME";

#[path = "commands/activity_replay.rs"]
mod activity_replay;
#[path = "commands/app_eval.rs"]
mod app_eval;
#[path = "commands/artifacts.rs"]
mod artifacts;
#[path = "commands/automations.rs"]
mod automations;
#[path = "commands/context_snapshot.rs"]
mod context_snapshot;
#[path = "commands/conversations.rs"]
mod conversations;
#[path = "commands/mcp.rs"]
mod mcp;
#[path = "commands/memory.rs"]
mod memory;
#[path = "commands/providers.rs"]
mod providers;
#[path = "commands/runs_permissions.rs"]
mod runs_permissions;
#[path = "commands/skills.rs"]
mod skills;

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

fn provider_settings_record_with_minimax_config(
    config_id: &str,
    has_api_key: bool,
) -> ProviderSettingsRecord {
    ProviderSettingsRecord {
        default_config_id: Some(config_id.to_owned()),
        configs: vec![ProviderConfigRecord {
            api_key: if has_api_key {
                "provider-test-token".to_owned()
            } else {
                String::new()
            },
            protocol: ModelProtocol::ChatCompletions,
            base_url: None,
            display_name: "MiniMax service".to_owned(),
            id: config_id.to_owned(),
            model_id: "minimax-text-01".to_owned(),
            provider_id: "minimax".to_owned(),
            model_descriptor: ProviderModelDescriptorRecord {
                protocol: ModelProtocol::ChatCompletions,
                conversation_capability: ConversationModelCapabilityRecord {
                    input_modalities: vec![ProviderModelModalityRecord::Text],
                    output_modalities: vec![ProviderModelModalityRecord::Text],
                    context_window: 1_000_000,
                    max_output_tokens: 8_192,
                    streaming: true,
                    tool_calling: true,
                    reasoning: false,
                    prompt_cache: false,
                    structured_output: true,
                },
                context_window: 1_000_000,
                display_name: "MiniMax text".to_owned(),
                lifecycle: ProviderModelLifecycleRecord::Stable,
                max_output_tokens: 8_192,
                model_id: "minimax-text-01".to_owned(),
                provider_id: "minimax".to_owned(),
            },
        }],
    }
}

fn minimax_image_route(config_id: &str, enabled: bool) -> ProviderCapabilityRoute {
    ProviderCapabilityRoute {
        kind: CapabilityRouteKind::ImageGeneration,
        config_id: config_id.to_owned(),
        provider_id: "minimax".to_owned(),
        operation_ids: vec!["minimax.image_generation".to_owned()],
        enabled,
    }
}

fn minimax_image_adapter_availability() -> ProviderServiceAdapterAvailability {
    ProviderServiceAdapterAvailability {
        bindings: vec![ToolServiceBinding {
            provider_id: "minimax".to_owned(),
            operation_id: "minimax.image_generation".to_owned(),
            route_kind: CapabilityRouteKind::ImageGeneration,
            output_artifact: ModelModality::Image,
        }],
    }
}

fn minimax_image_and_video_adapter_availability() -> ProviderServiceAdapterAvailability {
    ProviderServiceAdapterAvailability {
        bindings: vec![
            ToolServiceBinding {
                provider_id: "minimax".to_owned(),
                operation_id: "minimax.image_generation".to_owned(),
                route_kind: CapabilityRouteKind::ImageGeneration,
                output_artifact: ModelModality::Image,
            },
            ToolServiceBinding {
                provider_id: "minimax".to_owned(),
                operation_id: "minimax.video_generation".to_owned(),
                route_kind: CapabilityRouteKind::VideoGeneration,
                output_artifact: ModelModality::Video,
            },
            ToolServiceBinding {
                provider_id: "minimax".to_owned(),
                operation_id: "minimax.video_generation.query".to_owned(),
                route_kind: CapabilityRouteKind::VideoGeneration,
                output_artifact: ModelModality::Video,
            },
        ],
    }
}

fn canonical_unique_workspace(name: &str) -> PathBuf {
    let workspace = unique_workspace(name);
    std::fs::create_dir_all(&workspace).unwrap();
    workspace.canonicalize().unwrap()
}

fn provider_capability_route_store(name: &str) -> DesktopProviderCapabilityRouteStore {
    DesktopProviderCapabilityRouteStore::new(canonical_unique_workspace(name))
}

fn empty_provider_capability_routes() -> Arc<ParkingRwLock<ProviderCapabilityRouteSettings>> {
    Arc::new(ParkingRwLock::new(ProviderCapabilityRouteSettings {
        version: 1,
        routes: Vec::new(),
    }))
}

fn provider_settings_with_openai_and_minimax(
    openai_config_id: &str,
    minimax_config_id: &str,
    minimax_api_key: &str,
) -> ProviderSettingsRecord {
    ProviderSettingsRecord {
        default_config_id: Some(openai_config_id.to_owned()),
        configs: vec![
            ProviderConfigRecord {
                api_key: "openai-test-token".to_owned(),
                protocol: ModelProtocol::Responses,
                base_url: None,
                display_name: "OpenAI main".to_owned(),
                id: openai_config_id.to_owned(),
                model_id: "gpt-5.4-mini".to_owned(),
                provider_id: "openai".to_owned(),
                model_descriptor: openai_descriptor_record("gpt-5.4-mini"),
            },
            ProviderConfigRecord {
                api_key: minimax_api_key.to_owned(),
                protocol: ModelProtocol::ChatCompletions,
                base_url: None,
                display_name: "MiniMax image".to_owned(),
                id: minimax_config_id.to_owned(),
                model_id: "minimax-text-01".to_owned(),
                provider_id: "minimax".to_owned(),
                model_descriptor: ProviderModelDescriptorRecord {
                    protocol: ModelProtocol::ChatCompletions,
                    conversation_capability: ConversationModelCapabilityRecord {
                        input_modalities: vec![ProviderModelModalityRecord::Text],
                        output_modalities: vec![ProviderModelModalityRecord::Text],
                        context_window: 1_000_000,
                        max_output_tokens: 8_192,
                        streaming: true,
                        tool_calling: true,
                        reasoning: false,
                        prompt_cache: false,
                        structured_output: true,
                    },
                    context_window: 1_000_000,
                    display_name: "MiniMax service".to_owned(),
                    lifecycle: ProviderModelLifecycleRecord::Stable,
                    max_output_tokens: 8_192,
                    model_id: "minimax-text-01".to_owned(),
                    provider_id: "minimax".to_owned(),
                },
            },
        ],
    }
}

fn minimax_video_route(config_id: &str, enabled: bool) -> ProviderCapabilityRoute {
    ProviderCapabilityRoute {
        kind: CapabilityRouteKind::VideoGeneration,
        config_id: config_id.to_owned(),
        provider_id: "minimax".to_owned(),
        operation_ids: vec![
            "minimax.video_generation".to_owned(),
            "minimax.video_generation.query".to_owned(),
        ],
        enabled,
    }
}

fn minimax_tts_route(config_id: &str, enabled: bool) -> ProviderCapabilityRoute {
    ProviderCapabilityRoute {
        kind: CapabilityRouteKind::TextToSpeech,
        config_id: config_id.to_owned(),
        provider_id: "minimax".to_owned(),
        operation_ids: vec!["minimax.text_to_speech.sync".to_owned()],
        enabled,
    }
}

fn model_request_tool_names(request: &jyowo_harness_sdk::ext::ModelRequest) -> Vec<String> {
    request
        .tools
        .as_ref()
        .map(|tools| tools.iter().map(|tool| tool.name.clone()).collect())
        .unwrap_or_default()
}

async fn wait_for_scripted_model_requests(
    provider: &ScriptedProvider,
) -> Vec<jyowo_harness_sdk::ext::ModelRequest> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let requests = provider.requests().await;
        if !requests.is_empty() {
            return requests;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timed out waiting for model requests");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn runtime_state_with_capability_route_harness(
    workspace: PathBuf,
    routes: ProviderCapabilityRouteSettings,
    provider: Arc<ScriptedProvider>,
    provider_settings: ProviderSettingsRecord,
) -> DesktopRuntimeState {
    std::fs::create_dir_all(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    DesktopProviderSettingsStore::new(workspace.clone())
        .save_record(&provider_settings)
        .expect("provider settings should save");
    let routes = Arc::new(ParkingRwLock::new(routes));
    let resolver = desktop_provider_credential_resolver_with_stores(
        Arc::new(DesktopConversationModelConfigStore::new(workspace.clone())),
        Arc::new(DesktopProviderSettingsStore::new(workspace.clone())),
        Arc::clone(&routes),
    );
    let stream_permission_runtime = Arc::new(StreamPermissionRuntime::new(StreamBrokerConfig {
        default_timeout: Some(Duration::from_secs(5)),
        heartbeat_interval: None,
        max_pending: 16,
    }));
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(BuiltinToolset::Default)
        .build()
        .expect("tool registry should build");
    std::fs::create_dir_all(workspace.join(".jyowo").join("runtime").join("blobs")).unwrap();
    let blob_store = FileBlobStore::open(workspace.join(".jyowo").join("runtime").join("blobs"))
        .expect("blob store should open");
    let harness = Arc::new(
        Harness::builder()
            .with_options(test_harness_options(&workspace))
            .with_model_arc(provider)
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_blob_store(blob_store)
            .with_stream_permission_broker_arc(
                stream_permission_runtime.broker(),
                stream_permission_runtime.resolver_handle(),
            )
            .with_tool_registry(registry)
            .with_shared_provider_capability_routes(routes)
            .with_capability(ToolCapability::ProviderCredentialResolver, resolver)
            .build()
            .await
            .expect("harness should build with capability routes"),
    );

    DesktopRuntimeState::with_harness_and_stream_permission_runtime_for_workspace(
        workspace,
        harness,
        stream_permission_runtime,
    )
    .expect("state should use the harness permission broker")
}

async fn append_user_message_attachment_for_preview(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    attachment_id: &str,
    name: &str,
    mime_type: &str,
    bytes: Vec<u8>,
    retention: BlobRetention,
) {
    append_user_message_attachment_for_preview_with_blob_mime(
        state,
        session_id,
        attachment_id,
        name,
        mime_type,
        mime_type,
        bytes,
        retention,
    )
    .await;
}

async fn append_user_message_attachment_for_preview_with_blob_mime(
    state: &DesktopRuntimeState,
    session_id: SessionId,
    attachment_id: &str,
    name: &str,
    attachment_mime_type: &str,
    blob_mime_type: &str,
    bytes: Vec<u8>,
    retention: BlobRetention,
) {
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
                content_type: Some(blob_mime_type.to_owned()),
                size,
                content_hash,
                created_at: now(),
                retention,
            },
        )
        .await
        .expect("attachment blob writes");

    state
        .harness()
        .expect("runtime harness should exist")
        .event_store()
        .append(
            TenantId::SINGLE,
            session_id,
            &[Event::UserMessageAppended(UserMessageAppendedEvent {
                run_id: RunId::new(),
                message_id: MessageId::new(),
                content: MessageContent::Text("attached file".to_owned()),
                metadata: MessageMetadata::default(),
                attachments: vec![ConversationAttachmentReference {
                    id: attachment_id.to_owned(),
                    name: name.to_owned(),
                    mime_type: attachment_mime_type.to_owned(),
                    size_bytes: size,
                    blob_ref,
                }],
                at: now(),
            })],
        )
        .await
        .expect("user message attachment event should append");
}

fn minimal_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

fn supported_preview_image_with_metadata(mime_type: &str, metadata: &[u8]) -> Vec<u8> {
    let rgba = [
        0xE5, 0x2B, 0x50, 0xFF, 0x1E, 0x88, 0xE5, 0xFF, 0xF9, 0xA8, 0x25, 0xFF, 0x12, 0x12, 0x12,
        0xFF,
    ];
    match mime_type {
        "image/jpeg" => {
            let rgb = rgba
                .chunks_exact(4)
                .flat_map(|pixel| pixel[..3].iter().copied())
                .collect::<Vec<_>>();
            let mut encoded = Vec::new();
            JpegEncoder::new(&mut encoded)
                .write_image(&rgb, 2, 2, ExtendedColorType::Rgb8)
                .expect("test JPEG encodes");
            jpeg_with_app_metadata(encoded, metadata)
        }
        "image/gif" => {
            let mut encoded = Vec::new();
            GifEncoder::new(&mut encoded)
                .encode(&rgba, 2, 2, ExtendedColorType::Rgba8)
                .expect("test GIF encodes");
            gif_with_comment_metadata(encoded, metadata)
        }
        "image/webp" => {
            let mut encoded = Vec::new();
            WebPEncoder::new_lossless(&mut encoded)
                .write_image(&rgba, 2, 2, ExtendedColorType::Rgba8)
                .expect("test WebP encodes");
            webp_with_exif_metadata(encoded, metadata)
        }
        "image/avif" => {
            let encoded = minimal_avif();
            if metadata.is_empty() {
                encoded
            } else {
                iso_bmff_with_free_metadata(encoded, metadata)
            }
        }
        _ => panic!("unsupported test MIME type: {mime_type}"),
    }
}

fn minimal_avif() -> Vec<u8> {
    general_purpose::STANDARD
        .decode(
            "AAAAIGZ0eXBhdmlmAAAAAGF2aWZtaWYxbWlhZk1BMUEAAADrbWV0YQAAAAAAAAAhaGRscgAAAAAAAAAAcGljdAAAAAAAAAAAAAAAAAAAAAAOcGl0bQAAAAAAAQAAAB5pbG9jAAAAAEQAAAEAAQAAAAEAAAETAAAAJAAAAChpaW5mAAAAAAABAAAAGmluZmUCAAAAAAEAAGF2MDFDb2xvcgAAAABqaXBycAAAAEtpcGNvAAAAFGlzcGUAAAAAAAAAQAAAAEAAAAAQcGl4aQAAAAADCAgIAAAADGF2MUOBIAAAAAAAE2NvbHJuY2x4AAEAAgAAgAAAABdpcG1hAAAAAAAAAAEAAQQBAoMEAAAALG1kYXQSAAoGOBV//YJAMhgQAAC0UbTwxPOBGQHm72pfRNB5F8X+BlQ=",
        )
        .expect("embedded AVIF fixture decodes")
}

fn avif_with_exif_metadata() -> Vec<u8> {
    general_purpose::STANDARD
        .decode(include_str!("fixtures/avif-with-exif-metadata.b64").replace(['\n', '\r'], ""))
        .expect("embedded AVIF Exif fixture decodes")
}

fn jpeg_with_app_metadata(encoded: Vec<u8>, metadata: &[u8]) -> Vec<u8> {
    assert!(encoded.starts_with(&[0xFF, 0xD8]));
    let segment_len = u16::try_from(metadata.len() + 2).expect("test metadata fits JPEG segment");
    let mut output = Vec::with_capacity(encoded.len() + metadata.len() + 4);
    output.extend_from_slice(&encoded[..2]);
    output.extend_from_slice(&[0xFF, 0xE1]);
    output.extend_from_slice(&segment_len.to_be_bytes());
    output.extend_from_slice(metadata);
    output.extend_from_slice(&encoded[2..]);
    output
}

fn gif_with_comment_metadata(mut encoded: Vec<u8>, metadata: &[u8]) -> Vec<u8> {
    assert_eq!(encoded.last(), Some(&0x3B));
    let mut comment = vec![0x21, 0xFE];
    for chunk in metadata.chunks(255) {
        comment.push(u8::try_from(chunk.len()).expect("GIF comment chunk length fits"));
        comment.extend_from_slice(chunk);
    }
    comment.push(0);
    encoded.splice(encoded.len() - 1..encoded.len() - 1, comment);
    encoded
}

fn webp_with_exif_metadata(mut encoded: Vec<u8>, metadata: &[u8]) -> Vec<u8> {
    assert!(encoded.len() >= 12 && encoded.starts_with(b"RIFF") && &encoded[8..12] == b"WEBP");
    encoded.extend_from_slice(b"EXIF");
    encoded.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
    encoded.extend_from_slice(metadata);
    if metadata.len() % 2 == 1 {
        encoded.push(0);
    }
    let riff_size = u32::try_from(encoded.len() - 8).expect("test WebP fits RIFF size");
    encoded[4..8].copy_from_slice(&riff_size.to_le_bytes());
    encoded
}

fn iso_bmff_with_free_metadata(mut encoded: Vec<u8>, metadata: &[u8]) -> Vec<u8> {
    assert!(encoded.len() >= 12 && &encoded[4..8] == b"ftyp");
    let box_size = u32::try_from(metadata.len() + 8).expect("test metadata fits BMFF box");
    encoded.extend_from_slice(&box_size.to_be_bytes());
    encoded.extend_from_slice(b"free");
    encoded.extend_from_slice(metadata);
    encoded
}

fn png_with_ancillary_chunk(chunk_type: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut png = minimal_png();
    let iend_offset = png
        .windows(4)
        .position(|window| window == b"IEND")
        .expect("minimal png has IEND")
        - 4;
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(&chunk_type);
    chunk.extend_from_slice(data);
    chunk.extend_from_slice(&[0, 0, 0, 0]);
    png.splice(iend_offset..iend_offset, chunk);
    png
}

fn png_with_dimensions(width: u32, height: u32) -> Vec<u8> {
    let mut png = minimal_png();
    png[16..20].copy_from_slice(&width.to_be_bytes());
    png[20..24].copy_from_slice(&height.to_be_bytes());
    png
}

fn attachment_preview_data_url_bytes(data_url: &str) -> Vec<u8> {
    attachment_preview_data_url_bytes_with_mime(data_url, "image/png")
}

fn attachment_preview_data_url_bytes_with_mime(data_url: &str, mime_type: &str) -> Vec<u8> {
    let encoded = data_url
        .strip_prefix(&format!("data:{mime_type};base64,"))
        .expect("preview uses expected data URL MIME type");
    general_purpose::STANDARD
        .decode(encoded)
        .expect("preview data URL decodes")
}

fn detect_test_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1A\n") {
        return Some("image/png");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let major_brand = &bytes[8..12];
        if major_brand == b"avif" || major_brand == b"avis" {
            return Some("image/avif");
        }
    }
    None
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

fn automation_spec(id: &str, enabled: bool, missed_run_policy: MissedRunPolicy) -> AutomationSpec {
    automation_spec_at(id, enabled, missed_run_policy, chrono::Utc::now())
}

fn automation_spec_at(
    id: &str,
    enabled: bool,
    missed_run_policy: MissedRunPolicy,
    created_at: chrono::DateTime<chrono::Utc>,
) -> AutomationSpec {
    AutomationSpec {
        id: id.to_owned(),
        enabled,
        prompt: "Run checks".to_owned(),
        schedule: AutomationSchedule {
            interval_minutes: 30,
        },
        tool_profile: ToolProfile::Coding,
        permission_mode: PermissionMode::Default,
        sandbox_mode: SandboxMode::None,
        workspace_scope: AutomationWorkspaceScope::CurrentWorkspace,
        workspace_access: WorkspaceAccess::ReadOnly,
        missed_run_policy,
        created_at,
        updated_at: created_at,
    }
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
                service_binding: None,
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
        permission_mode: PermissionMode::Default,
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
        auto_resolved: false,
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
            .with_model(TestModelProvider::default())
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
    provider: Arc<InMemoryMemoryProvider>,
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
            .with_model(TestModelProvider::default())
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
            .with_model(TestModelProvider::default())
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
    options.model_id = "test-model".to_owned();
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

use async_trait::async_trait;
use futures::stream;
use jyowo_desktop_shell::commands::{
    cancel_run_payload, cancel_run_with_runtime_state, delete_mcp_server_with_runtime_state,
    delete_mcp_server_with_store, delete_memory_item_with_runtime_state,
    export_memory_items_with_runtime_state, export_support_bundle_with_runtime_state,
    get_app_info_payload, get_context_snapshot_payload, get_context_snapshot_with_runtime_state,
    get_conversation_payload, get_conversation_with_runtime_state,
    get_memory_item_with_runtime_state, get_replay_timeline_with_runtime_state,
    harness_healthcheck_payload, list_activity_payload, list_activity_with_runtime_state,
    list_artifacts_payload, list_artifacts_with_runtime_state, list_conversations_payload,
    list_conversations_with_runtime_state, list_eval_cases_payload,
    list_mcp_servers_with_runtime_state, list_memory_items_with_runtime_state, provider_secret_ref,
    provider_secret_ref_prefix, resolve_permission_payload, resolve_permission_with_runtime_state,
    run_eval_case_payload, runtime_state_async, runtime_state_for_workspace,
    save_mcp_server_with_runtime_state, save_mcp_server_with_store,
    save_provider_settings_with_store, start_run_payload, start_run_with_runtime_state,
    update_memory_item_with_runtime_state, validate_provider_settings_payload,
    ArtifactSummaryPayload, CancelRunRequest, DeleteMcpServerRequest, DeleteMemoryItemRequest,
    DesktopProviderSettingsStore, DesktopRuntimeState, ExportSupportBundleRequest,
    GetContextSnapshotRequest, GetConversationRequest, GetMemoryItemRequest, ListActivityRequest,
    McpServerConfigRecord, McpServerStore, McpServerTransportConfig, PermissionDecision,
    ProviderSettingsRecord, ProviderSettingsRequest, ProviderSettingsStore, ReplayTimelineRequest,
    ResolvePermissionRequest, RunEvalCaseRequest, SaveMcpServerRequest, StartRunRequest,
    UpdateMemoryItemRequest, ValidateProviderSettingsRequest,
};
use jyowo_harness_sdk::builtin::DefaultRedactor;
use jyowo_harness_sdk::ext::{
    now, BudgetMetric, Decision, DecisionScope, DeferPolicy, Event, FallbackPolicy,
    InteractivityLevel, McpConnection, McpError, McpRegistry, McpServerId, McpServerScope,
    McpServerSource, McpServerSpec, McpToolDescriptor, McpToolResult, MemoryId, MemoryKind,
    MemoryMetadata, MemoryRecord, MemorySource, MemoryStore, MemoryVisibility, OverflowAction,
    PermissionCheck, PermissionContext, PermissionMode, PermissionRequest, PermissionSubject,
    ProviderRestriction, RedactPatternSet, RedactRules, RedactScope, Redactor, RequestId,
    ResultBudget, RuleSnapshot, RunId, SessionId, Severity, StreamBrokerConfig, TenantId, Tool,
    ToolContext, ToolDescriptor, ToolError, ToolEvent, ToolGroup, ToolProperties, ToolRegistry,
    ToolResult, ToolStream, ToolUseId, TransportChoice, TrustLevel, UsageSnapshot, ValidationError,
};
use jyowo_harness_sdk::ext::{ContentDelta, ModelStreamEvent};
use jyowo_harness_sdk::testing::{
    InMemoryEventStore, MockMemoryProvider, MockProvider, NoopRedactor, NoopSandbox,
    ScriptedProvider, ScriptedResponse,
};
use jyowo_harness_sdk::{
    ConversationEventsPageRequest, Harness, McpConfig, StreamPermissionRuntime,
};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
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
fn eval_lab_payloads_return_safe_local_support_cases() {
    let list = list_eval_cases_payload();
    let value = serde_json::to_value(&list).unwrap();

    assert_eq!(
        value,
        json!({
            "cases": [
                {
                    "id": "regression-smoke",
                    "lastRun": {
                        "completedAt": "2026-06-17T00:00:00.000Z",
                        "failed": 0,
                        "passed": 3,
                        "status": "passed"
                    },
                    "title": "Regression smoke"
                }
            ]
        })
    );

    let run = run_eval_case_payload(RunEvalCaseRequest {
        case_id: "regression-smoke".to_owned(),
    })
    .unwrap();
    let run_value = serde_json::to_value(&run).unwrap();

    assert_eq!(run_value["status"], "completed");
    assert_eq!(run_value["case"]["id"], "regression-smoke");
    assert_eq!(run_value["case"]["lastRun"]["passed"], 4);
}

#[test]
fn list_artifacts_payload_returns_safe_reviewable_artifacts() {
    let payload = list_artifacts_payload();
    let value = serde_json::to_value(&payload).unwrap();

    assert_eq!(
        value,
        json!({
            "artifacts": [
                {
                    "actionLabel": "Open",
                    "description": "Generated implementation plan and app shell review output.",
                    "id": "artifact-foundation-plan",
                    "kind": "markdown",
                    "preview": "# Foundation review\n\n- Conversation workspace restored.\n- Activity rail connected.\n- Support surfaces available from navigation.",
                    "sourceMessageId": "message-002",
                    "sourceRunId": "run-001",
                    "status": "ready",
                    "title": "Foundation implementation review"
                }
            ]
        })
    );
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
}

#[test]
fn run_eval_case_payload_rejects_unknown_or_malformed_case_ids() {
    let unknown = run_eval_case_payload(RunEvalCaseRequest {
        case_id: "unknown-case".to_owned(),
    })
    .unwrap_err();
    assert_eq!(unknown.code, "INVALID_PAYLOAD");

    let malformed = run_eval_case_payload(RunEvalCaseRequest {
        case_id: "bad case".to_owned(),
    })
    .unwrap_err();
    assert_eq!(malformed.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn validate_provider_settings_payload_accepts_supported_provider_metadata() {
    let payload = validate_provider_settings_payload(ValidateProviderSettingsRequest {
        model_id: "gpt-4o-mini".to_owned(),
        provider_id: "openai".to_owned(),
    })
    .await
    .unwrap();
    let value = serde_json::to_value(payload).unwrap();

    assert_eq!(
        value,
        json!({
            "modelId": "gpt-4o-mini",
            "providerId": "openai",
            "status": "accepted"
        })
    );
}

#[tokio::test]
async fn save_provider_settings_payload_stores_secret_and_returns_reference_without_raw_key() {
    let raw_key = "provider-test-token";
    let store = RecordingProviderSettingsStore::default();
    let expected_secret_ref = store.secret_ref("openai");
    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: raw_key.to_owned(),
            model_id: "gpt-4o-mini".to_owned(),
            provider_id: "openai".to_owned(),
        },
        &store,
    )
    .await
    .unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();

    assert!(serialized.contains(&format!("\"secretRef\":\"{expected_secret_ref}\"")));
    assert!(serialized.contains("\"status\":\"saved\""));
    assert!(!serialized.contains(raw_key));
    assert_eq!(
        store.secret.lock().unwrap().as_ref().unwrap(),
        &(expected_secret_ref.clone(), raw_key.to_owned())
    );
    assert_eq!(
        store.record.lock().unwrap().as_ref().unwrap(),
        &ProviderSettingsRecord {
            model_id: "gpt-4o-mini".to_owned(),
            provider_id: "openai".to_owned(),
            secret_ref: expected_secret_ref,
            stale_secret_refs: Vec::new(),
        }
    );
}

#[tokio::test]
async fn save_provider_settings_payload_rolls_back_secret_when_record_write_fails() {
    let store = RecordingProviderSettingsStore {
        fail_record: true,
        ..RecordingProviderSettingsStore::default()
    };
    let expected_secret_ref = store.secret_ref("openai");
    let error = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: "provider-test-token".to_owned(),
            model_id: "gpt-4o-mini".to_owned(),
            provider_id: "openai".to_owned(),
        },
        &store,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert_eq!(
        store.deleted_secrets.lock().unwrap().as_slice(),
        &[expected_secret_ref]
    );
}

#[tokio::test]
async fn save_provider_settings_payload_deletes_previous_secret_after_successful_rotation() {
    let previous_secret_ref = provider_secret_ref(
        PathBuf::from("/test/workspace").as_path(),
        "openai",
        "previous",
    );
    let store = RecordingProviderSettingsStore {
        record: Mutex::new(Some(ProviderSettingsRecord {
            model_id: "gpt-4o-mini".to_owned(),
            provider_id: "openai".to_owned(),
            secret_ref: previous_secret_ref.clone(),
            stale_secret_refs: Vec::new(),
        })),
        ..RecordingProviderSettingsStore::default()
    };
    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: "new-provider-token".to_owned(),
            model_id: "gpt-4o-mini".to_owned(),
            provider_id: "openai".to_owned(),
        },
        &store,
    )
    .await
    .unwrap();

    assert_ne!(payload.secret_ref, previous_secret_ref);
    assert_eq!(
        store.deleted_secrets.lock().unwrap().as_slice(),
        &[previous_secret_ref]
    );
    assert_eq!(
        store.record.lock().unwrap().as_ref().unwrap(),
        &ProviderSettingsRecord {
            model_id: "gpt-4o-mini".to_owned(),
            provider_id: "openai".to_owned(),
            secret_ref: payload.secret_ref,
            stale_secret_refs: Vec::new(),
        }
    );
}

#[tokio::test]
async fn save_provider_settings_payload_keeps_stale_secret_refs_when_cleanup_fails() {
    let older_secret_ref = provider_secret_ref(
        PathBuf::from("/test/workspace").as_path(),
        "openai",
        "older",
    );
    let previous_secret_ref = provider_secret_ref(
        PathBuf::from("/test/workspace").as_path(),
        "openai",
        "previous",
    );
    let store = RecordingProviderSettingsStore {
        delete_failures: Mutex::new(HashSet::from([older_secret_ref.clone()])),
        record: Mutex::new(Some(ProviderSettingsRecord {
            model_id: "gpt-4o-mini".to_owned(),
            provider_id: "openai".to_owned(),
            secret_ref: previous_secret_ref.clone(),
            stale_secret_refs: vec![older_secret_ref.clone()],
        })),
        ..RecordingProviderSettingsStore::default()
    };
    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: "new-provider-token".to_owned(),
            model_id: "gpt-4o-mini".to_owned(),
            provider_id: "openai".to_owned(),
        },
        &store,
    )
    .await
    .unwrap();

    assert_eq!(
        store.deleted_secrets.lock().unwrap().as_slice(),
        &[older_secret_ref.clone(), previous_secret_ref]
    );
    assert_eq!(
        store.record.lock().unwrap().as_ref().unwrap(),
        &ProviderSettingsRecord {
            model_id: "gpt-4o-mini".to_owned(),
            provider_id: "openai".to_owned(),
            secret_ref: payload.secret_ref,
            stale_secret_refs: vec![older_secret_ref],
        }
    );
}

#[tokio::test]
async fn save_provider_settings_payload_ignores_tampered_stale_secret_refs() {
    let store = RecordingProviderSettingsStore {
        record: Mutex::new(Some(ProviderSettingsRecord {
            model_id: "gpt-4o-mini".to_owned(),
            provider_id: "openai".to_owned(),
            secret_ref: "provider/workspace-other/openai/active".to_owned(),
            stale_secret_refs: vec!["provider/workspace-other/openai/stale".to_owned()],
        })),
        ..RecordingProviderSettingsStore::default()
    };
    let payload = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: "new-provider-token".to_owned(),
            model_id: "gpt-4o-mini".to_owned(),
            provider_id: "openai".to_owned(),
        },
        &store,
    )
    .await
    .unwrap();

    assert!(store.deleted_secrets.lock().unwrap().is_empty());
    assert_eq!(
        store.record.lock().unwrap().as_ref().unwrap(),
        &ProviderSettingsRecord {
            model_id: "gpt-4o-mini".to_owned(),
            provider_id: "openai".to_owned(),
            secret_ref: payload.secret_ref,
            stale_secret_refs: Vec::new(),
        }
    );
}

#[test]
fn provider_secret_ref_is_scoped_to_workspace_without_exposing_path() {
    let first = provider_secret_ref(
        PathBuf::from("/workspace/one").as_path(),
        "openai",
        "secret",
    );
    let second = provider_secret_ref(
        PathBuf::from("/workspace/two").as_path(),
        "openai",
        "secret",
    );

    assert_ne!(first, second);
    assert!(first.starts_with("provider/workspace-"));
    assert!(first.ends_with("/openai/secret"));
    assert!(!first.contains("/workspace/one"));
}

#[tokio::test]
async fn provider_settings_payload_rejects_invalid_provider_model_and_key() {
    let store = RecordingProviderSettingsStore::default();
    let invalid_provider = save_provider_settings_with_store(
        ProviderSettingsRequest {
            api_key: "provider-test-token".to_owned(),
            model_id: "gpt-4o-mini".to_owned(),
            provider_id: "unknown".to_owned(),
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
            api_key: String::new(),
            model_id: "gpt-4o-mini".to_owned(),
            provider_id: "openai".to_owned(),
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
        model_id: "gpt-4o-mini".to_owned(),
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
            display_name: String::new(),
            id: "bad id".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: String::new(),
                args: Vec::new(),
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
async fn save_mcp_server_payload_rejects_secret_bearing_stdio_args() {
    let store = RecordingMcpServerStore::default();
    let error = save_mcp_server_with_store(
        SaveMcpServerRequest {
            display_name: "Workspace GitHub".to_owned(),
            id: "github".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "node".to_owned(),
                args: vec!["--token=mcp-secret-token".to_owned()],
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
            display_name: "Workspace GitHub".to_owned(),
            id: "github".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "node".to_owned(),
                args: vec!["ghp_abcdefghijklmnopqrstuvwxyz0123456789".to_owned()],
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

#[test]
fn save_mcp_server_payload_rejects_unknown_transport_fields() {
    let error = serde_json::from_value::<SaveMcpServerRequest>(json!({
        "displayName": "Workspace GitHub",
        "id": "github",
        "scope": "global",
        "transport": {
            "kind": "stdio",
            "command": "node",
            "args": [],
            "env": { "GITHUB_TOKEN": "secret" }
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
            display_name: "Workspace Stdio".to_owned(),
            id: "stdio".to_owned(),
            scope: "global".to_owned(),
            transport: McpServerTransportConfig::Stdio {
                command: "/bin/sh".to_owned(),
                args: vec!["-c".to_owned(), stdio_mcp_fixture_script()],
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
                    "exposedToolCount": 2,
                    "id": "github",
                    "origin": "workspace",
                    "scope": "global",
                    "status": "ready",
                    "transport": "inProcess"
                }
            ]
        })
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

#[test]
fn list_conversations_payload_returns_typed_placeholder_data() {
    let payload = list_conversations_payload();
    let value = serde_json::to_value(payload).unwrap();

    assert_eq!(
        value,
        json!({
            "conversations": [
                {
                    "id": "conversation-placeholder",
                    "lastMessagePreview": "Runtime conversation history is not connected yet.",
                    "title": "Build the desktop foundation",
                    "updatedAt": "2026-06-17T00:00:00.000Z"
                }
            ]
        })
    );
}

#[tokio::test]
async fn list_conversations_with_runtime_state_returns_startable_conversation_id() {
    let state = runtime_state_with_harness().await;
    let payload = list_conversations_with_runtime_state(&state);
    let conversation_id = payload.conversations[0].id.clone();

    let session_id =
        SessionId::parse(&conversation_id).expect("conversation id should be a session id");
    assert_eq!(session_id.to_string(), conversation_id);

    let run = start_run_with_runtime_state(
        StartRunRequest {
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

#[test]
fn get_conversation_payload_rejects_empty_conversation_id() {
    let error = get_conversation_payload(GetConversationRequest {
        conversation_id: " ".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[test]
fn get_conversation_payload_returns_requested_conversation() {
    let payload = get_conversation_payload(GetConversationRequest {
        conversation_id: "conversation-001".to_owned(),
    })
    .unwrap();
    let value = serde_json::to_value(payload).unwrap();

    assert_eq!(
        value,
        json!({
            "conversation": {
                "id": "conversation-001",
                "messages": [],
                "title": "Build the desktop foundation",
                "updatedAt": "2026-06-17T00:00:00.000Z"
            }
        })
    );
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
            assert_eq!(
                payload.conversation.updated_at,
                payload.conversation.messages[1].timestamp
            );
            break;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("conversation detail should include runtime messages");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_projects_assistant_outputs() {
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
        let payload = list_artifacts_with_runtime_state(&state)
            .await
            .expect("runtime artifact projection should load");

        if let Some(artifact) = payload.artifacts.first() {
            assert_eq!(artifact.kind, "markdown");
            assert_eq!(artifact.status, "ready");
            assert!(artifact
                .preview
                .as_deref()
                .unwrap_or_default()
                .contains("Runtime artifact"));
            assert!(artifact.source_message_id.is_some());
            assert_eq!(
                RunId::parse(&artifact.source_run_id)
                    .expect("source run id should be canonical")
                    .to_string(),
                artifact.source_run_id
            );
            return;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("runtime assistant output should be projected as an artifact");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn list_artifacts_with_runtime_state_hides_runtime_read_errors() {
    let state = runtime_state_with_harness().await;

    let error = list_artifacts_with_runtime_state(&state)
        .await
        .expect_err("missing conversation session should fail safely");

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert_eq!(error.message, "artifact read failed");
    assert!(!error
        .message
        .contains(&state.default_conversation_id().to_string()));
}

#[test]
fn start_run_payload_validates_prompt_and_requires_runtime() {
    let error = start_run_payload(StartRunRequest {
        context_references: Some(vec!["apps/desktop".to_owned()]),
        conversation_id: SessionId::new().to_string(),
        prompt: "Continue implementation".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");

    let error = start_run_payload(StartRunRequest {
        context_references: None,
        conversation_id: SessionId::new().to_string(),
        prompt: String::new(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn start_run_with_runtime_state_returns_real_run_id_for_conversation() {
    let state = runtime_state_with_harness().await;
    let harness = state
        .harness()
        .expect("runtime state should retain the configured harness");
    let session_id = SessionId::new();
    let conversation_id = session_id.to_string();

    let payload = start_run_with_runtime_state(
        StartRunRequest {
            context_references: Some(vec!["apps/desktop".to_owned()]),
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
        permission_event["payload"]["command"]["executable"],
        serde_json::Value::String("printf desktop-permission".to_owned())
    );

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

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
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
    let error = resolve_permission_payload(ResolvePermissionRequest {
        decision: PermissionDecision::Approve,
        request_id: "01HZ0000000000000000000001".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");

    let error = resolve_permission_payload(ResolvePermissionRequest {
        decision: PermissionDecision::Deny,
        request_id: "01HZ0000000000000000000001".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "RUNTIME_UNAVAILABLE");

    let error = resolve_permission_payload(ResolvePermissionRequest {
        decision: PermissionDecision::Approve,
        request_id: String::new(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[test]
fn resolve_permission_payload_rejects_invalid_request_id_before_runtime() {
    let error = resolve_permission_payload(ResolvePermissionRequest {
        decision: PermissionDecision::Approve,
        request_id: "permission-001".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[test]
fn resolve_permission_payload_rejects_noncanonical_request_id_before_runtime() {
    let error = resolve_permission_payload(ResolvePermissionRequest {
        decision: PermissionDecision::Approve,
        request_id: "01hz0000000000000000000001".to_owned(),
    })
    .unwrap_err();

    assert_eq!(error.code, "INVALID_PAYLOAD");
}

#[tokio::test]
async fn runtime_state_routes_permission_decisions_to_permission_broker_resolver() {
    let state = runtime_state_for_workspace(unique_workspace("runtime-state-routes"))
        .await
        .expect("runtime state should initialize");
    assert!(state.harness().is_some());

    let error = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
            decision: PermissionDecision::Approve,
            request_id: "01HZ0000000000000000000001".to_owned(),
        },
        &state,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code, "PERMISSION_RESOLVE_FAILED");
    assert!(error.message.contains("not pending"));
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

    let decision_task =
        tokio::spawn(async move { broker.decide(request, permission_context()).await });

    wait_for_pending_permission(&state, request_id).await;

    let payload = resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
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
async fn list_activity_with_runtime_state_exposes_pending_permission_requests_by_run_id() {
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
    assert!(value.contains("[REDACTED]"));

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
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
    assert!(serialized.contains("[REDACTED]"));
    assert_eq!(
        state.pending_permission_requests().len(),
        1,
        "replay read mode must not resolve or execute pending tools"
    );

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
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
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

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

    assert!(exported.contains("[REDACTED]"));
    assert!(!exported.contains("ghp_abcdefghijklmnopqrstuvwxyz0123456789"));
    assert!(exported.contains("\"redacted\":true"));

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
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
            model_id: "gpt-4o-mini".to_owned(),
            provider_id: "openai".to_owned(),
            secret_ref: "jyowo-provider-secret:test".to_owned(),
            stale_secret_refs: Vec::new(),
        })
        .unwrap_err();

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert!(!external.join("provider-settings.json").exists());
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
        Self {
            descriptor: ToolDescriptor {
                name: "NeedsPermission".to_owned(),
                display_name: "NeedsPermission".to_owned(),
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

fn unique_workspace(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-desktop-{name}-{}-{}",
        std::process::id(),
        SessionId::new()
    ))
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
    delete_failures: Mutex<HashSet<String>>,
    deleted_secrets: Mutex<Vec<String>>,
    fail_record: bool,
    record: Mutex<Option<ProviderSettingsRecord>>,
    secret: Mutex<Option<(String, String)>>,
}

impl ProviderSettingsStore for RecordingProviderSettingsStore {
    fn secret_ref(&self, provider_id: &str) -> String {
        format!("{}recording", self.secret_ref_prefix(provider_id))
    }

    fn secret_ref_prefix(&self, provider_id: &str) -> String {
        provider_secret_ref_prefix(PathBuf::from("/test/workspace").as_path(), provider_id)
    }

    fn save_secret(
        &self,
        secret_ref: &str,
        api_key: &str,
    ) -> Result<(), jyowo_desktop_shell::commands::CommandErrorPayload> {
        *self.secret.lock().unwrap() = Some((secret_ref.to_owned(), api_key.to_owned()));
        Ok(())
    }

    fn delete_secret(
        &self,
        secret_ref: &str,
    ) -> Result<(), jyowo_desktop_shell::commands::CommandErrorPayload> {
        self.deleted_secrets
            .lock()
            .unwrap()
            .push(secret_ref.to_owned());
        if self.delete_failures.lock().unwrap().contains(secret_ref) {
            return Err(jyowo_desktop_shell::commands::CommandErrorPayload {
                code: "RUNTIME_OPERATION_FAILED",
                message: "secret cleanup failed".to_owned(),
            });
        }

        Ok(())
    }

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
fn get_context_snapshot_payload_returns_safe_placeholder_context() {
    let payload = get_context_snapshot_payload(GetContextSnapshotRequest {
        conversation_id: Some("conversation-001".to_owned()),
        run_id: None,
    })
    .unwrap();
    let value = serde_json::to_value(payload).unwrap();

    assert_eq!(
        value,
        json!({
            "activeArtifact": null,
            "decisions": [],
            "files": [],
            "nextActions": ["Connect the Rust runtime facade"],
            "path": "workspace://local",
            "project": "Local workspace"
        })
    );
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
async fn get_context_snapshot_with_runtime_state_projects_conversation_context() {
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
        let payload = get_context_snapshot_with_runtime_state(
            GetContextSnapshotRequest {
                conversation_id: Some(session_id.to_string()),
                run_id: None,
            },
            &state,
        )
        .await
        .expect("runtime context snapshot should load");

        if payload.active_artifact.as_deref() == Some("Runtime context artifact") {
            assert_eq!(
                payload.project,
                workspace.file_name().unwrap().to_string_lossy()
            );
            assert_eq!(
                payload.path,
                workspace.canonicalize().unwrap().display().to_string()
            );
            assert!(payload.files.iter().any(|file| {
                file.label == "apps/desktop/src/main.tsx" && file.state == Some("ready")
            }));
            assert!(payload
                .next_actions
                .iter()
                .any(|action| action == "Review Runtime context artifact"));
            return;
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("runtime context snapshot should include the latest assistant artifact");
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
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
    }));

    resolve_permission_with_runtime_state(
        ResolvePermissionRequest {
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
    assert!(payload.path.contains("[REDACTED]"));
    assert!(payload.project.contains("[REDACTED]"));
}

#[tokio::test]
async fn get_context_snapshot_with_runtime_state_hides_runtime_read_errors() {
    let state = runtime_state_with_harness().await;

    let error = get_context_snapshot_with_runtime_state(
        GetContextSnapshotRequest {
            conversation_id: Some(state.default_conversation_id().to_string()),
            run_id: None,
        },
        &state,
    )
    .await
    .expect_err("missing conversation session should fail safely");

    assert_eq!(error.code, "RUNTIME_OPERATION_FAILED");
    assert_eq!(error.message, "context snapshot read failed");
    assert!(!error
        .message
        .contains(&state.default_conversation_id().to_string()));
}

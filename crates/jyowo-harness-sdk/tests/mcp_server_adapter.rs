#![cfg(all(feature = "testing", feature = "mcp-server-adapter"))]

use std::sync::Arc;

use harness_contracts::{
    BlobMeta, BlobRetention, BlobStore, BudgetMetric, Decision, DecisionScope, DeferPolicy,
    InteractivityLevel, NetworkAccess, NoopRedactor, OverflowAction, PermissionSubject,
    ProviderRestriction, ResultBudget, SemverString, SessionId, StopReason, TenantId,
    ToolActionPlan, ToolDescriptor, ToolError, ToolGroup, ToolOrigin, ToolProperties, ToolResult,
    ToolUseId, TrustLevel, UsageSnapshot, WorkspaceAccess,
};
use harness_journal::{InMemoryBlobStore, InMemoryEventStore};
use harness_mcp::{ExposedCapability, HarnessMcpBackend, McpServerRequestContext};
use harness_model::{ContentDelta, ModelProtocol, ModelStreamEvent};
use harness_permission::PermissionCheck;
use harness_tool::{
    action_plan_from_permission_check, AuthorizedToolInput, Tool, ToolContext, ToolEvent,
    ToolRegistry, ToolStream, ValidationError,
};
use jyowo_harness_sdk::{prelude::*, testing::*};
use serde_json::json;

#[test]
fn harness_mcp_backend_exposes_sessions_messages_events_and_channels() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-mcp-server-adapter");
        std::fs::create_dir_all(&workspace).unwrap();
        let model = Arc::new(TestModelProvider::default());
        let model_provider: Arc<dyn ModelProvider> = model.clone();
        let blob_store = InMemoryBlobStore::default();
        let blob_bytes = bytes::Bytes::from_static(b"attachment");
        let blob_ref = blob_store
            .put(
                TenantId::SINGLE,
                blob_bytes.clone(),
                BlobMeta {
                    content_type: Some("text/plain".to_owned()),
                    size: blob_bytes.len() as u64,
                    content_hash: *blake3::hash(&blob_bytes).as_bytes(),
                    created_at: chrono::Utc::now(),
                    retention: BlobRetention::TenantScoped,
                },
            )
            .await
            .expect("blob should be stored");
        let harness = Harness::builder()
            .with_workspace_root(&workspace)
            .with_model_arc(model_provider)
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_blob_store(blob_store)
            .build()
            .await
            .expect("harness should build");
        let session_id = SessionId::new();
        harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created");
        let context = McpServerRequestContext::default().with_tenant_id(TenantId::SINGLE);

        let listed = harness
            .call_harness_tool(
                &context,
                ExposedCapability::SessionsList,
                json!({"limit": 10, "include_ended": true}),
            )
            .await
            .expect("sessions_list should succeed");
        assert_eq!(listed["sessions"][0]["session_id"], session_id.to_string());

        let metadata = harness
            .call_harness_tool(
                &context,
                ExposedCapability::SessionGet,
                json!({"session_id": session_id.to_string()}),
            )
            .await
            .expect("session_get should succeed");
        assert_eq!(metadata["session"]["session_id"], session_id.to_string());

        let sent = harness
            .call_harness_tool(
                &context,
                ExposedCapability::MessagesSend,
                json!({"session_id": session_id.to_string(), "message": "hello"}),
            )
            .await
            .expect("messages_send should run the turn");
        assert_eq!(sent["session_id"], session_id.to_string());
        assert_eq!(model.requests().await.len(), 1);

        let messages = harness
            .call_harness_tool(
                &context,
                ExposedCapability::MessagesRead,
                json!({"session_id": session_id.to_string(), "offset": 0, "limit": 10}),
            )
            .await
            .expect("messages_read should succeed");
        assert!(messages["messages"]
            .as_array()
            .expect("messages array")
            .iter()
            .any(|message| message["role"] == "user"));

        let events = harness
            .call_harness_tool(
                &context,
                ExposedCapability::EventsPoll,
                json!({"session_id": session_id.to_string(), "limit": 10}),
            )
            .await
            .expect("events_poll should succeed");
        assert!(!events["events"]
            .as_array()
            .expect("events array")
            .is_empty());

        let attachment = harness
            .call_harness_tool(
                &context,
                ExposedCapability::AttachmentsFetch,
                json!({"blob_ref": blob_ref}),
            )
            .await
            .expect("attachments_fetch should succeed");
        assert_eq!(attachment["content_base64"], "YXR0YWNobWVudA==");

        let channels = harness
            .call_harness_tool(&context, ExposedCapability::ChannelsList, json!({}))
            .await
            .expect("channels_list should succeed");
        assert_eq!(channels, json!({"count": 0, "channels": []}));
    });
}

#[test]
fn harness_mcp_messages_send_resumes_workspace_bootstrap() {
    tokio_runtime().block_on(async {
        let root = unique_workspace("sdk-mcp-server-adapter-workspace-resume");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("AGENTS.md"), "workspace MCP resume rule").unwrap();
        let model = Arc::new(TestModelProvider::default());
        let model_provider: Arc<dyn ModelProvider> = model.clone();
        let harness = Harness::builder()
            .with_model_arc(model_provider)
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");
        let workspace = harness
            .create_workspace(
                WorkspaceSpec::new(&root, "MCP Resume Workspace")
                    .with_bootstrap_files(vec![BootstrapFileSpec::required("AGENTS.md")]),
            )
            .await
            .expect("workspace should be registered");
        let session_id = SessionId::new();
        harness
            .create_session(
                SessionOptions::default()
                    .with_workspace(workspace.id)
                    .with_session_id(session_id),
            )
            .await
            .expect("workspace session should be created");

        harness
            .call_harness_tool(
                &McpServerRequestContext::default().with_tenant_id(TenantId::SINGLE),
                ExposedCapability::MessagesSend,
                json!({"session_id": session_id.to_string(), "message": "use workspace"}),
            )
            .await
            .expect("messages_send should resume session");

        let requests = model.requests().await;
        let system = requests[0].system.as_deref().unwrap_or_default();
        assert!(system.contains("workspace MCP resume rule"));
    });
}

#[test]
fn harness_mcp_messages_send_allows_changed_workspace_bootstrap() {
    tokio_runtime().block_on(async {
        let root = unique_workspace("sdk-mcp-server-adapter-workspace-resume-changed");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("AGENTS.md"), "workspace MCP resume rule v1").unwrap();
        let model = Arc::new(TestModelProvider::default());
        let model_provider: Arc<dyn ModelProvider> = model.clone();
        let harness = Harness::builder()
            .with_model_arc(model_provider)
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");
        let workspace = harness
            .create_workspace(
                WorkspaceSpec::new(&root, "MCP Resume Changed Workspace")
                    .with_bootstrap_files(vec![BootstrapFileSpec::required("AGENTS.md")]),
            )
            .await
            .expect("workspace should be registered");
        let session_id = SessionId::new();
        harness
            .create_session(
                SessionOptions::default()
                    .with_workspace(workspace.id)
                    .with_session_id(session_id),
            )
            .await
            .expect("workspace session should be created");
        std::fs::write(root.join("AGENTS.md"), "workspace MCP resume rule v2").unwrap();

        harness
            .call_harness_tool(
                &McpServerRequestContext::default().with_tenant_id(TenantId::SINGLE),
                ExposedCapability::MessagesSend,
                json!({"session_id": session_id.to_string(), "message": "use workspace"}),
            )
            .await
            .expect("messages_send should resume with changed workspace prompt inputs");

        let requests = model.requests().await;
        let system = requests[0].system.as_deref().unwrap_or_default();
        assert!(system.contains("workspace MCP resume rule v2"));
    });
}

#[test]
fn harness_mcp_messages_send_allows_model_protocol_hash_variants() {
    tokio_runtime().block_on(async {
        let root = unique_workspace("sdk-mcp-server-adapter-model-protocol-resume");
        std::fs::create_dir_all(&root).unwrap();
        let harness = Harness::builder()
            .with_workspace_root(&root)
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .build()
            .await
            .expect("harness should build");
        let context = McpServerRequestContext::default().with_tenant_id(TenantId::SINGLE);

        let model_session_id = SessionId::new();
        harness
            .create_session(
                SessionOptions::new(&root)
                    .with_session_id(model_session_id)
                    .with_model_id("test-model"),
            )
            .await
            .expect("model variant session should be created");
        harness
            .call_harness_tool(
                &context,
                ExposedCapability::MessagesSend,
                json!({"session_id": model_session_id.to_string(), "message": "resume"}),
            )
            .await
            .expect("messages_send should resume model hash variant");

        let protocol_session_id = SessionId::new();
        harness
            .create_session(
                SessionOptions::new(&root)
                    .with_session_id(protocol_session_id)
                    .with_protocol(ModelProtocol::Messages),
            )
            .await
            .expect("protocol variant session should be created");
        harness
            .call_harness_tool(
                &context,
                ExposedCapability::MessagesSend,
                json!({"session_id": protocol_session_id.to_string(), "message": "resume"}),
            )
            .await
            .expect("messages_send should resume protocol hash variant");
    });
}

#[test]
fn harness_mcp_backend_resolves_stream_permissions() {
    tokio_runtime().block_on(async {
        let (broker, mut receiver, resolver) =
            harness_permission::StreamBasedBroker::new(harness_permission::StreamBrokerConfig {
                default_timeout: Some(std::time::Duration::from_secs(5)),
                heartbeat_interval: None,
                max_pending: 8,
            });
        let harness = Harness::builder()
            .with_model(TestModelProvider::default())
            .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
            .with_sandbox(NoopSandbox::new())
            .with_stream_permission_broker(broker, resolver)
            .build()
            .await
            .expect("harness should build");
        let request = permission_request();
        let request_id = request.request_id;
        let broker = harness.permission_broker().expect("broker should exist");
        let decision_task =
            tokio::spawn(async move { broker.decide(request, permission_context()).await });
        receiver.recv().await.expect("request should be emitted");

        let result = harness
            .call_harness_tool(
                &McpServerRequestContext::default(),
                ExposedCapability::PermissionsRespond,
                json!({"request_id": request_id.to_string(), "decision": "allow_once"}),
            )
            .await
            .expect("permissions_respond should resolve");

        assert_eq!(result, json!({"resolved": true}));
        assert_eq!(
            decision_task.await.expect("decision task should join"),
            Decision::AllowOnce
        );
    });
}

#[test]
fn harness_mcp_permission_response_unblocks_waiting_messages_send_run() {
    tokio_runtime().block_on(async {
        let workspace = unique_workspace("sdk-mcp-server-adapter-permission-e2e");
        std::fs::create_dir_all(&workspace).unwrap();
        let (broker, mut receiver, resolver) =
            harness_permission::StreamBasedBroker::new(harness_permission::StreamBrokerConfig {
                default_timeout: Some(std::time::Duration::from_secs(5)),
                heartbeat_interval: None,
                max_pending: 8,
            });
        let model = Arc::new(ScriptedProvider::new(vec![
            ScriptedResponse::Stream(tool_call_events(
                "approval_tool",
                json!({ "message": "check" }),
            )),
            ScriptedResponse::Stream(text_events("approved")),
        ]));
        let model_provider: Arc<dyn ModelProvider> = model;
        let registry = ToolRegistry::builder()
            .with_builtin_toolset(harness_tool::BuiltinToolset::Empty)
            .with_tool(Box::new(ApprovalTool))
            .build()
            .expect("registry");
        let harness = Arc::new(
            Harness::builder()
                .with_workspace_root(&workspace)
                .with_default_session_options(
                    SessionOptions::new(&workspace)
                        .with_interactivity(InteractivityLevel::FullyInteractive),
                )
                .with_model_arc(model_provider)
                .with_store(InMemoryEventStore::new(Arc::new(NoopRedactor)))
                .with_sandbox(NoopSandbox::new())
                .with_stream_permission_broker(broker, resolver)
                .with_tool_registry(registry)
                .build()
                .await
                .expect("harness should build"),
        );
        let session_id = SessionId::new();
        harness
            .create_session(SessionOptions::new(&workspace).with_session_id(session_id))
            .await
            .expect("session should be created");
        let context = McpServerRequestContext::default().with_tenant_id(TenantId::SINGLE);
        let run_harness = Arc::clone(&harness);
        let run_context = context.clone();
        let mut run_task = tokio::spawn(async move {
            run_harness
                .call_harness_tool(
                    &run_context,
                    ExposedCapability::MessagesSend,
                    json!({"session_id": session_id.to_string(), "message": "needs approval"}),
                )
                .await
        });

        let pending = tokio::select! {
            pending = receiver.recv() => pending.expect("permission should be emitted"),
            result = &mut run_task => {
                panic!("messages_send completed before permission: {:?}", result);
            },
            () = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                panic!("permission was not emitted before timeout");
            }
        };
        let listed = harness
            .call_harness_tool(
                &context,
                ExposedCapability::PermissionsListOpen,
                json!({"session_id": session_id.to_string(), "limit": 10}),
            )
            .await
            .expect("permissions_list_open should succeed");
        assert!(listed["permissions"]
            .as_array()
            .expect("permissions")
            .iter()
            .any(|permission| permission["request_id"] == pending.request_id.to_string()));

        let resolved = harness
            .call_harness_tool(
                &context,
                ExposedCapability::PermissionsRespond,
                json!({"request_id": pending.request_id.to_string(), "decision": "allow_once"}),
            )
            .await
            .expect("permissions_respond should resolve");

        assert_eq!(resolved, json!({"resolved": true}));
        let sent = tokio::time::timeout(std::time::Duration::from_secs(5), run_task)
            .await
            .expect("messages_send should finish after approval")
            .expect("run task should join")
            .expect("messages_send should finish after approval");
        assert_eq!(sent["session_id"], session_id.to_string());
    });
}

fn unique_workspace(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jyowo-{name}-{}-{}",
        std::process::id(),
        SessionId::new()
    ))
}

fn tool_call_events(name: &str, input: serde_json::Value) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant-approval".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::ToolUseComplete {
                id: ToolUseId::new(),
                name: name.to_owned(),
                input,
            },
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::ToolUse),
            usage_delta: UsageSnapshot::default(),
        },
        ModelStreamEvent::MessageStop,
    ]
}

fn text_events(text: &str) -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::MessageStart {
            message_id: "assistant-final".to_owned(),
            usage: UsageSnapshot::default(),
        },
        ModelStreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentDelta::Text(text.to_owned()),
        },
        ModelStreamEvent::MessageDelta {
            stop_reason: Some(StopReason::EndTurn),
            usage_delta: UsageSnapshot::default(),
        },
        ModelStreamEvent::MessageStop,
    ]
}

struct ApprovalTool;

#[async_trait::async_trait]
impl Tool for ApprovalTool {
    fn descriptor(&self) -> &ToolDescriptor {
        static DESCRIPTOR: std::sync::OnceLock<ToolDescriptor> = std::sync::OnceLock::new();
        DESCRIPTOR.get_or_init(|| ToolDescriptor {
            name: "approval_tool".to_owned(),
            display_name: "approval_tool".to_owned(),
            description: "approval tool".to_owned(),
            category: "test".to_owned(),
            group: ToolGroup::Custom("test".to_owned()),
            version: SemverString::from("0.1.0"),
            input_schema: json!({"type": "object"}),
            output_schema: None,
            dynamic_schema: false,
            properties: ToolProperties {
                is_concurrency_safe: true,
                is_read_only: true,
                is_destructive: false,
                long_running: None,
                defer_policy: DeferPolicy::AlwaysLoad,
            },
            trust_level: TrustLevel::AdminTrusted,
            required_capabilities: Vec::new(),
            budget: ResultBudget {
                metric: BudgetMetric::Chars,
                limit: 10_000,
                on_overflow: OverflowAction::Truncate,
                preview_head_chars: 1_000,
                preview_tail_chars: 200,
            },
            provider_restriction: ProviderRestriction::All,
            origin: ToolOrigin::Builtin,
            search_hint: None,
            service_binding: None,
        })
    }

    async fn validate(
        &self,
        _input: &serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn plan(
        &self,
        input: &serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            PermissionCheck::AskUser {
                subject: PermissionSubject::ToolInvocation {
                    tool: "approval_tool".to_owned(),
                    input: input.clone(),
                },
                scope: DecisionScope::ToolName("approval_tool".to_owned()),
            },
            Vec::new(),
            WorkspaceAccess::None,
            NetworkAccess::None,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolStream, ToolError> {
        Ok(Box::pin(futures::stream::iter([ToolEvent::Final(
            ToolResult::Text("approved".to_owned()),
        )])))
    }
}

fn tokio_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
}

fn permission_request() -> harness_permission::PermissionRequest {
    harness_permission::PermissionRequest {
        request_id: harness_contracts::RequestId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        tool_use_id: harness_contracts::ToolUseId::new(),
        tool_name: "bash".to_owned(),
        subject: harness_contracts::PermissionSubject::CommandExec {
            command: "echo ok".to_owned(),
            argv: vec!["echo".to_owned(), "ok".to_owned()],
            cwd: None,
            fingerprint: None,
        },
        severity: harness_contracts::Severity::Low,
        scope_hint: harness_contracts::DecisionScope::ExactCommand {
            command: "echo ok".to_owned(),
            cwd: None,
        },
        confirmation_expected: None,
        created_at: chrono::Utc::now(),
    }
}

fn permission_context() -> harness_permission::PermissionContext {
    harness_permission::PermissionContext {
        permission_mode: harness_contracts::PermissionMode::Default,
        previous_mode: None,
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        run_id: None,
        interactivity: harness_contracts::InteractivityLevel::FullyInteractive,
        timeout_policy: None,
        fallback_policy: harness_contracts::FallbackPolicy::DenyAll,
        hook_overrides: Vec::new(),
    }
}

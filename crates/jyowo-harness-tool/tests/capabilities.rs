#![cfg(feature = "builtin-toolset")]

use std::sync::Arc;

use bytes::Bytes;
use chrono::Utc;
use futures::{future::BoxFuture, stream, StreamExt};
use harness_contracts::{
    BlobId, BlobReaderCap, BlobRef, CapabilityRegistry, OffloadedBlobAuthorizerCap,
    RunCancellerCap, TenantId, TodoItem, TodoStoreCap, ToolActionPlan, ToolCapability, ToolResult,
    ToolUseId,
};
use harness_tool::{
    builtin::{ReadBlobTool, TaskStopTool, TodoTool},
    AuthorizedTicketSummary, AuthorizedToolInput, InterruptToken, Tool, ToolContext, ToolEvent,
};
use parking_lot::Mutex;
use serde_json::json;

#[tokio::test]
async fn builtin_capabilities_expose_real_methods() {
    let todo_store = Arc::new(RecordingTodoStore::default());
    let canceller = Arc::new(RecordingCanceller::default());
    let blob_reader = Arc::new(StaticBlobReader);
    let mut caps = CapabilityRegistry::default();
    caps.install::<dyn TodoStoreCap>(ToolCapability::TodoStore, todo_store.clone());
    caps.install::<dyn RunCancellerCap>(ToolCapability::RunCanceller, canceller.clone());
    caps.install::<dyn BlobReaderCap>(ToolCapability::BlobReader, blob_reader);
    caps.install::<dyn OffloadedBlobAuthorizerCap>(
        ToolCapability::OffloadedBlobAuthorizer,
        Arc::new(AllowOffloadedBlobAuthorizer),
    );

    let todo_result = execute_final(
        &TodoTool::default(),
        json!({ "items": [{ "content": "ship", "status": "pending" }] }),
        tool_ctx(caps.clone()),
    )
    .await;
    assert!(matches!(todo_result, ToolResult::Structured(value) if value["items"] == 1));
    assert_eq!(todo_store.items.lock()[0].content, "ship");

    let stop_result = execute_final(
        &TaskStopTool::default(),
        json!({ "reason": "done" }),
        tool_ctx(caps.clone()),
    )
    .await;
    assert!(matches!(stop_result, ToolResult::Structured(value) if value["stopped"] == true));
    assert_eq!(canceller.reasons.lock()[0], "done");

    let blob = BlobRef {
        id: BlobId::new(),
        size: 5,
        content_hash: [0; 32],
        content_type: Some("text/plain".to_owned()),
    };
    let blob_result = execute_final(
        &ReadBlobTool::default(),
        json!({ "blob_ref": blob }),
        tool_ctx(caps),
    )
    .await;
    assert_eq!(blob_result, ToolResult::Text("hello".to_owned()));
}

#[derive(Default)]
struct RecordingTodoStore {
    items: Mutex<Vec<TodoItem>>,
}

impl TodoStoreCap for RecordingTodoStore {
    fn replace_todos<'a>(
        &'a self,
        _tenant_id: TenantId,
        _session_id: harness_contracts::SessionId,
        _run_id: harness_contracts::RunId,
        items: Vec<TodoItem>,
    ) -> BoxFuture<'a, Result<(), harness_contracts::ToolError>> {
        Box::pin(async move {
            *self.items.lock() = items;
            Ok(())
        })
    }
}

#[derive(Default)]
struct RecordingCanceller {
    reasons: Mutex<Vec<String>>,
}

impl RunCancellerCap for RecordingCanceller {
    fn request_stop<'a>(
        &'a self,
        _tenant_id: TenantId,
        _session_id: harness_contracts::SessionId,
        _run_id: harness_contracts::RunId,
        reason: String,
    ) -> BoxFuture<'a, Result<(), harness_contracts::ToolError>> {
        Box::pin(async move {
            self.reasons.lock().push(reason);
            Ok(())
        })
    }
}

struct StaticBlobReader;

struct AllowOffloadedBlobAuthorizer;

impl BlobReaderCap for StaticBlobReader {
    fn read_blob<'a>(
        &'a self,
        _tenant_id: TenantId,
        _blob: BlobRef,
    ) -> BoxFuture<
        'a,
        Result<futures::stream::BoxStream<'static, Bytes>, harness_contracts::ToolError>,
    > {
        Box::pin(async {
            Ok(Box::pin(stream::iter([Bytes::from_static(b"hello")]))
                as futures::stream::BoxStream<'static, Bytes>)
        })
    }
}

impl OffloadedBlobAuthorizerCap for AllowOffloadedBlobAuthorizer {
    fn authorize_offloaded_blob<'a>(
        &'a self,
        _tenant_id: TenantId,
        _session_id: harness_contracts::SessionId,
        _run_id: harness_contracts::RunId,
        _blob: BlobRef,
    ) -> BoxFuture<'a, Result<(), harness_contracts::ToolError>> {
        Box::pin(async { Ok(()) })
    }
}

async fn execute_final(tool: &dyn Tool, input: serde_json::Value, ctx: ToolContext) -> ToolResult {
    tool.validate(&input, &ctx).await.unwrap();
    let plan = tool.plan(&input, &ctx).await.unwrap();
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan)).unwrap();
    let mut stream = tool.execute_authorized(authorized, ctx).await.unwrap();
    match stream.next().await {
        Some(ToolEvent::Final(result)) => result,
        other => panic!("expected final result, got {other:?}"),
    }
}

fn tool_ctx(cap_registry: CapabilityRegistry) -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: harness_contracts::SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: std::env::temp_dir(),
        sandbox: None,
        cap_registry: Arc::new(cap_registry),
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::default(),
        parent_run: None,
        model: None,
        model_config_id: None,
        memory_thread_settings: None,
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
    }
}

fn ticket_for(plan: &ToolActionPlan) -> AuthorizedTicketSummary {
    AuthorizedTicketSummary {
        ticket_id: harness_contracts::AuthorizationTicketId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: harness_contracts::SessionId::new(),
        run_id: harness_contracts::RunId::new(),
        tool_use_id: plan.tool_use_id,
        tool_name: plan.tool_name.clone(),
        action_plan_hash: plan.plan_hash.clone(),
        consumed_at: Utc::now(),
    }
}

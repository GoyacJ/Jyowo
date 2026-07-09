use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures::future::BoxFuture;
use futures::stream;
use harness_contracts::{
    BlobError, BlobMeta, BlobReaderCap, BlobReaderCapAdapter, BlobRef, BlobStore, BudgetMetric,
    CapabilityRegistry, Event, OffloadedBlobAuthorizerCap, OverflowAction, ProviderRestriction,
    ResultBudget, TenantId, ToolActionPlan, ToolCapability, ToolDescriptor, ToolError,
    ToolExecutionChannel, ToolGroup, ToolOrigin, ToolProperties, ToolResult, ToolResultPart,
    ToolUseId, TrustLevel,
};
use harness_permission::PermissionCheck;
use harness_tool::{
    builtin::ReadBlobTool, AuthorizedTicketSummary, AuthorizedToolCall, AuthorizedToolInput,
    InterruptToken, OrchestratorContext, Tool, ToolCall, ToolContext, ToolEvent, ToolEventEmitter,
    ToolOrchestrator, ToolPool, ToolPoolFilter, ToolPoolModelProfile, ToolRegistry, ToolSearchMode,
    ValidationError,
};
use parking_lot::Mutex;
use serde_json::{json, Value};

#[tokio::test]
async fn result_under_budget_is_returned_without_overflow() {
    let (pool, call) = pool_with_tool(
        "under",
        budget(10, OverflowAction::Offload),
        vec![ToolEvent::Final(ToolResult::Text("small".to_owned()))],
    )
    .await;
    let blob_store = Arc::new(RecordingBlobStore::default());
    let emitter = Arc::new(RecordingEmitter::default());

    let results = dispatch(pool, call, Some(blob_store.clone()), emitter.clone()).await;

    assert!(matches!(results[0].result, Ok(ToolResult::Text(ref text)) if text == "small"));
    assert_eq!(results[0].overflow, None);
    assert!(blob_store.puts().is_empty());
    assert!(emitter.events().is_empty());
}

#[tokio::test]
async fn reject_budget_returns_result_too_large() {
    let (pool, call) = pool_with_tool(
        "reject",
        budget(3, OverflowAction::Reject),
        vec![ToolEvent::Final(ToolResult::Text("too long".to_owned()))],
    )
    .await;

    let results = dispatch(pool, call, None, Arc::new(RecordingEmitter::default())).await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::ResultTooLarge {
            original: 8,
            limit: 3,
            metric: BudgetMetric::Chars
        })
    ));
    assert_eq!(results[0].overflow, None);
}

#[tokio::test]
async fn truncate_budget_returns_truncated_text_without_blob() {
    let (pool, call) = pool_with_tool(
        "truncate",
        budget(4, OverflowAction::Truncate),
        vec![ToolEvent::Final(ToolResult::Text("abcdef".to_owned()))],
    )
    .await;
    let blob_store = Arc::new(RecordingBlobStore::default());

    let results = dispatch(
        pool,
        call,
        Some(blob_store.clone()),
        Arc::new(RecordingEmitter::default()),
    )
    .await;

    assert!(matches!(results[0].result, Ok(ToolResult::Text(ref text)) if text == "abcd"));
    assert_eq!(results[0].overflow, None);
    assert!(blob_store.puts().is_empty());
}

#[tokio::test]
async fn offload_budget_writes_full_text_and_returns_preview_with_metadata() {
    let (pool, call) = pool_with_tool(
        "offload",
        budget(5, OverflowAction::Offload),
        vec![ToolEvent::Final(ToolResult::Text("abcdefghij".to_owned()))],
    )
    .await;
    let blob_store = Arc::new(RecordingBlobStore::default());
    let emitter = Arc::new(RecordingEmitter::default());

    let results = dispatch(pool, call, Some(blob_store.clone()), emitter.clone()).await;

    let Ok(ToolResult::Mixed(parts)) = &results[0].result else {
        panic!("expected mixed offload result: {:?}", results[0].result);
    };
    assert_eq!(
        parts[0],
        ToolResultPart::Text {
            text: "ab".to_owned()
        }
    );
    assert!(
        matches!(parts[1], ToolResultPart::Blob { ref summary, .. } if summary.as_deref() == Some("tool result exceeded budget; content was offloaded"))
    );
    assert_eq!(
        parts[2],
        ToolResultPart::Text {
            text: "ij".to_owned()
        }
    );

    let overflow = results[0].overflow.as_ref().expect("overflow metadata");
    assert_eq!(overflow.original_size, 10);
    assert_eq!(overflow.original_metric, BudgetMetric::Chars);
    assert_eq!(overflow.effective_limit, 5);
    assert_eq!(overflow.head_chars, 2);
    assert_eq!(overflow.tail_chars, 2);

    let puts = blob_store.puts();
    assert_eq!(puts.len(), 1);
    assert_eq!(puts[0].0, Bytes::from_static(b"abcdefghij"));

    let events = emitter.events();
    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], Event::ToolResultOffloaded(event) if event.original_size == 10 && event.effective_limit == 5)
    );
}

#[tokio::test]
async fn offloaded_result_blob_can_be_read_back_with_read_blob() {
    let (pool, call) = pool_with_tool(
        "offload_readback",
        budget(5, OverflowAction::Offload),
        vec![ToolEvent::Final(ToolResult::Text("abcdefghij".to_owned()))],
    )
    .await;
    let blob_store = Arc::new(RecordingBlobStore::default());

    let results = dispatch(
        pool,
        call,
        Some(blob_store.clone()),
        Arc::new(RecordingEmitter::default()),
    )
    .await;

    let Ok(ToolResult::Mixed(parts)) = &results[0].result else {
        panic!("expected offloaded mixed result: {:?}", results[0].result);
    };
    let blob_ref = parts
        .iter()
        .find_map(|part| match part {
            ToolResultPart::Blob { blob_ref, .. } => Some(blob_ref.clone()),
            _ => None,
        })
        .expect("offload result should include blob ref");

    let reader: Arc<dyn BlobReaderCap> = Arc::new(BlobReaderCapAdapter::new(blob_store));
    let mut caps = CapabilityRegistry::default();
    caps.install(ToolCapability::BlobReader, reader);
    caps.install::<dyn OffloadedBlobAuthorizerCap>(
        ToolCapability::OffloadedBlobAuthorizer,
        Arc::new(AllowOffloadedBlobAuthorizer),
    );

    let read_result = execute_read_blob(blob_ref, caps).await;

    assert_eq!(read_result, ToolResult::Text("abcdefghij".to_owned()));
}

#[tokio::test]
async fn typed_artifact_result_is_preserved_without_budget_offload() {
    let blob_ref = harness_contracts::BlobRef {
        id: harness_contracts::BlobId::new(),
        size: 128,
        content_hash: [4; 32],
        content_type: Some("image/png".to_owned()),
    };
    let (pool, call) = pool_with_tool(
        "artifact",
        budget(5, OverflowAction::Offload),
        vec![ToolEvent::Final(ToolResult::Mixed(vec![
            ToolResultPart::Artifact {
                artifact_kind: harness_contracts::ModelModality::Image,
                content_type: "image/png".to_owned(),
                blob_ref: blob_ref.clone(),
                title: "Generated image".to_owned(),
                preview: Some("Generated image".to_owned()),
            },
        ]))],
    )
    .await;
    let blob_store = Arc::new(RecordingBlobStore::default());

    let results = dispatch(
        pool,
        call,
        Some(blob_store.clone()),
        Arc::new(RecordingEmitter::default()),
    )
    .await;

    let Ok(ToolResult::Mixed(parts)) = &results[0].result else {
        panic!("expected typed artifact result: {:?}", results[0].result);
    };
    assert!(matches!(
        parts.first(),
        Some(ToolResultPart::Artifact {
            title,
            preview: Some(preview),
            ..
        }) if title == "Generated image" && preview == "Generated image"
    ));
    assert!(blob_store.puts().is_empty());
    assert_eq!(results[0].overflow, None);
}

#[tokio::test]
async fn offload_budget_reports_blob_failures() {
    let (pool, call) = pool_with_tool(
        "offload_fail",
        budget(5, OverflowAction::Offload),
        vec![ToolEvent::Final(ToolResult::Text("abcdefghij".to_owned()))],
    )
    .await;
    let blob_store = Arc::new(RecordingBlobStore::failing());

    let results = dispatch(
        pool,
        call,
        Some(blob_store),
        Arc::new(RecordingEmitter::default()),
    )
    .await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::OffloadFailed(ref message)) if message.contains("backend")
    ));
}

#[tokio::test]
async fn text_partials_are_combined_with_final_before_budgeting() {
    let (pool, call) = pool_with_tool(
        "partials",
        budget(4, OverflowAction::Reject),
        vec![
            ToolEvent::Partial(harness_contracts::MessagePart::Text("abc".to_owned())),
            ToolEvent::Final(ToolResult::Text("de".to_owned())),
        ],
    )
    .await;

    let results = dispatch(pool, call, None, Arc::new(RecordingEmitter::default())).await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::ResultTooLarge {
            original: 5,
            limit: 4,
            metric: BudgetMetric::Chars
        })
    ));
}

async fn dispatch(
    pool: ToolPool,
    call: ToolCall,
    blob_store: Option<Arc<dyn BlobStore>>,
    event_emitter: Arc<dyn ToolEventEmitter>,
) -> Vec<harness_tool::ToolResultEnvelope> {
    let ctx = orchestrator_ctx(pool, blob_store, event_emitter);
    let mut tool_ctx = ctx.tool_context.clone();
    tool_ctx.tool_use_id = call.tool_use_id;
    let tool = ctx.pool.get(&call.tool_name).unwrap();
    let plan = tool.plan(&call.input, &tool_ctx).await.unwrap();
    let authorized = AuthorizedToolInput::new(call.input, plan.clone(), ticket_for(&plan)).unwrap();
    let call = AuthorizedToolCall {
        tool_use_id: call.tool_use_id,
        tool_name: call.tool_name,
        input: authorized,
    };
    ToolOrchestrator::default().dispatch(vec![call], ctx).await
}

async fn pool_with_tool(
    name: &str,
    budget: ResultBudget,
    events: Vec<ToolEvent>,
) -> (ToolPool, ToolCall) {
    let registry = ToolRegistry::builder()
        .with_builtin_toolset(harness_tool::BuiltinToolset::Empty)
        .with_tool(Box::new(BudgetTool {
            descriptor: descriptor(name, budget),
            events,
        }))
        .build()
        .unwrap();
    let pool = ToolPool::assemble(
        &registry.snapshot(),
        &ToolPoolFilter::default(),
        &ToolSearchMode::Disabled,
        &ToolPoolModelProfile::default(),
        &harness_tool::SchemaResolverContext {
            run_id: harness_contracts::RunId::new(),
            session_id: harness_contracts::SessionId::new(),
            tenant_id: TenantId::SINGLE,
        },
    )
    .await
    .unwrap();
    (
        pool,
        ToolCall {
            tool_use_id: ToolUseId::new(),
            tool_name: name.to_owned(),
            input: json!({}),
        },
    )
}

fn budget(limit: u64, on_overflow: OverflowAction) -> ResultBudget {
    ResultBudget {
        metric: BudgetMetric::Chars,
        limit,
        on_overflow,
        preview_head_chars: 2,
        preview_tail_chars: 2,
    }
}

fn budget_with_metric(
    metric: BudgetMetric,
    limit: u64,
    on_overflow: OverflowAction,
) -> ResultBudget {
    ResultBudget {
        metric,
        limit,
        on_overflow,
        preview_head_chars: 2,
        preview_tail_chars: 2,
    }
}

#[tokio::test]
async fn reject_budget_stops_on_oversized_partial_before_consuming_later_events() {
    let (pool, call) = pool_with_tool(
        "partial_reject",
        budget(3, OverflowAction::Reject),
        vec![
            ToolEvent::Partial(harness_contracts::MessagePart::Text("abcd".to_owned())),
            ToolEvent::Error(ToolError::Message("later event was consumed".to_owned())),
        ],
    )
    .await;

    let results = dispatch(pool, call, None, Arc::new(RecordingEmitter::default())).await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::ResultTooLarge {
            original: 4,
            limit: 3,
            metric: BudgetMetric::Chars
        })
    ));
}

#[tokio::test]
async fn truncate_budget_stops_on_oversized_partial_before_consuming_later_events() {
    let (pool, call) = pool_with_tool(
        "partial_truncate",
        budget(3, OverflowAction::Truncate),
        vec![
            ToolEvent::Partial(harness_contracts::MessagePart::Text("abcdef".to_owned())),
            ToolEvent::Error(ToolError::Message("later event was consumed".to_owned())),
        ],
    )
    .await;

    let results = dispatch(pool, call, None, Arc::new(RecordingEmitter::default())).await;

    assert!(matches!(results[0].result, Ok(ToolResult::Text(ref text)) if text == "abc"));
}

#[tokio::test]
async fn offload_budget_stops_on_oversized_partial_before_consuming_later_events() {
    let (pool, call) = pool_with_tool(
        "partial_offload",
        budget(3, OverflowAction::Offload),
        vec![
            ToolEvent::Partial(harness_contracts::MessagePart::Text("abcdef".to_owned())),
            ToolEvent::Error(ToolError::Message("later event was consumed".to_owned())),
        ],
    )
    .await;
    let blob_store = Arc::new(RecordingBlobStore::default());
    let emitter = Arc::new(RecordingEmitter::default());

    let results = dispatch(pool, call, Some(blob_store.clone()), emitter.clone()).await;

    let Ok(ToolResult::Mixed(parts)) = &results[0].result else {
        panic!("expected mixed offload result: {:?}", results[0].result);
    };
    assert!(
        matches!(parts.get(1), Some(ToolResultPart::Blob { ref summary, .. }) if summary.as_deref() == Some("tool result exceeded budget; content was offloaded"))
    );
    assert_eq!(results[0].overflow.as_ref().unwrap().original_size, 6);
    assert_eq!(blob_store.puts()[0].0, Bytes::from_static(b"abcdef"));
    assert_eq!(emitter.events().len(), 1);
}

#[tokio::test]
async fn offload_budget_includes_prior_partials_and_overflowing_partial_in_blob() {
    let (pool, call) = pool_with_tool(
        "partial_offload_all_received",
        budget(5, OverflowAction::Offload),
        vec![
            ToolEvent::Partial(harness_contracts::MessagePart::Text("abc".to_owned())),
            ToolEvent::Partial(harness_contracts::MessagePart::Text("def".to_owned())),
            ToolEvent::Error(ToolError::Message("later event was consumed".to_owned())),
        ],
    )
    .await;
    let blob_store = Arc::new(RecordingBlobStore::default());
    let emitter = Arc::new(RecordingEmitter::default());

    let results = dispatch(pool, call, Some(blob_store.clone()), emitter.clone()).await;

    assert!(matches!(results[0].result, Ok(ToolResult::Mixed(_))));
    assert_eq!(results[0].overflow.as_ref().unwrap().original_size, 6);
    assert_eq!(blob_store.puts()[0].0, Bytes::from_static(b"abcdef"));
    assert_eq!(emitter.events().len(), 1);
}

#[tokio::test]
async fn non_text_partials_are_rejected_instead_of_silently_dropped() {
    let blob_ref = harness_contracts::BlobRef {
        id: harness_contracts::BlobId::new(),
        size: 0,
        content_hash: [0; 32],
        content_type: Some("image/png".to_owned()),
    };
    let (pool, call) = pool_with_tool(
        "partial_image",
        budget(100, OverflowAction::Reject),
        vec![ToolEvent::Partial(harness_contracts::MessagePart::Image {
            mime_type: "image/png".to_owned(),
            blob_ref,
        })],
    )
    .await;

    let results = dispatch(pool, call, None, Arc::new(RecordingEmitter::default())).await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::Message(ref message)) if message == "non-text tool partials are not supported"
    ));
}

#[tokio::test]
async fn truncate_respects_byte_budget_on_utf8_boundary() {
    let (pool, call) = pool_with_tool(
        "truncate_bytes",
        budget_with_metric(BudgetMetric::Bytes, 4, OverflowAction::Truncate),
        vec![ToolEvent::Final(ToolResult::Text("ééé".to_owned()))],
    )
    .await;

    let results = dispatch(pool, call, None, Arc::new(RecordingEmitter::default())).await;

    assert!(matches!(results[0].result, Ok(ToolResult::Text(ref text)) if text == "éé"));
}

#[tokio::test]
async fn truncate_respects_line_budget() {
    let (pool, call) = pool_with_tool(
        "truncate_lines",
        budget_with_metric(BudgetMetric::Lines, 2, OverflowAction::Truncate),
        vec![ToolEvent::Final(ToolResult::Text(
            "one\ntwo\nthree".to_owned(),
        ))],
    )
    .await;

    let results = dispatch(pool, call, None, Arc::new(RecordingEmitter::default())).await;

    assert!(matches!(results[0].result, Ok(ToolResult::Text(ref text)) if text == "one\ntwo"));
}

#[tokio::test]
async fn line_budget_counts_blank_partial_append() {
    let (pool, call) = pool_with_tool(
        "line_partial_blank",
        budget_with_metric(BudgetMetric::Lines, 1, OverflowAction::Reject),
        vec![
            ToolEvent::Partial(harness_contracts::MessagePart::Text("one\n".to_owned())),
            ToolEvent::Partial(harness_contracts::MessagePart::Text("\n".to_owned())),
        ],
    )
    .await;

    let results = dispatch(pool, call, None, Arc::new(RecordingEmitter::default())).await;

    assert!(matches!(
        results[0].result,
        Err(ToolError::ResultTooLarge {
            original: 2,
            limit: 1,
            metric: BudgetMetric::Lines
        })
    ));
}

#[tokio::test]
async fn text_partials_are_prepended_to_all_final_mixed_parts() {
    let (pool, call) = pool_with_tool(
        "partials_mixed",
        budget(100, OverflowAction::Reject),
        vec![
            ToolEvent::Partial(harness_contracts::MessagePart::Text("prefix".to_owned())),
            ToolEvent::Final(ToolResult::Mixed(vec![
                ToolResultPart::Text {
                    text: "body".to_owned(),
                },
                ToolResultPart::Code {
                    language: "text".to_owned(),
                    text: "code".to_owned(),
                },
            ])),
        ],
    )
    .await;

    let results = dispatch(pool, call, None, Arc::new(RecordingEmitter::default())).await;

    assert!(matches!(
        &results[0].result,
        Ok(ToolResult::Mixed(parts))
            if parts == &vec![
                ToolResultPart::Text {
                    text: "prefix".to_owned()
                },
                ToolResultPart::Text {
                    text: "body".to_owned()
                },
                ToolResultPart::Code {
                    language: "text".to_owned(),
                    text: "code".to_owned()
                }
            ]
    ));
}

fn descriptor(name: &str, budget: ResultBudget) -> ToolDescriptor {
    ToolDescriptor {
        name: name.to_owned(),
        display_name: name.to_owned(),
        description: "budget test tool".to_owned(),
        category: "test".to_owned(),
        group: ToolGroup::Custom("test".to_owned()),
        version: "0.0.1".to_owned(),
        input_schema: json!({ "type": "object" }),
        output_schema: None,
        dynamic_schema: false,
        properties: ToolProperties {
            is_concurrency_safe: true,
            is_read_only: true,
            is_destructive: false,
            long_running: None,
            defer_policy: harness_contracts::DeferPolicy::AlwaysLoad,
        },
        trust_level: TrustLevel::AdminTrusted,
        required_capabilities: vec![],
        budget,
        provider_restriction: ProviderRestriction::All,
        origin: ToolOrigin::Builtin,
        search_hint: None,
        service_binding: None,
    }
}

#[derive(Clone)]
struct BudgetTool {
    descriptor: ToolDescriptor,
    events: Vec<ToolEvent>,
}

#[async_trait]
impl Tool for BudgetTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn plan(
        &self,
        input: &Value,
        ctx: &ToolContext,
    ) -> Result<harness_contracts::ToolActionPlan, ToolError> {
        harness_tool::action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            PermissionCheck::Allowed,
            Vec::new(),
            harness_contracts::WorkspaceAccess::None,
            harness_contracts::NetworkAccess::None,
            ToolExecutionChannel::DirectAuthorizedRust,
        )
    }

    async fn execute_authorized(
        &self,
        _authorized: harness_tool::AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<harness_tool::ToolStream, ToolError> {
        Ok(Box::pin(stream::iter(self.events.clone())))
    }
}

fn orchestrator_ctx(
    pool: ToolPool,
    blob_store: Option<Arc<dyn BlobStore>>,
    event_emitter: Arc<dyn ToolEventEmitter>,
) -> OrchestratorContext {
    let run_id = harness_contracts::RunId::new();
    let session_id = harness_contracts::SessionId::new();
    OrchestratorContext {
        pool,
        tool_context: ToolContext {
            tool_use_id: ToolUseId::new(),
            run_id,
            session_id,
            tenant_id: TenantId::SINGLE,
            correlation_id: harness_contracts::CorrelationId::new(),
            agent_id: harness_contracts::AgentId::from_u128(1),
            subagent_depth: 0,
            workspace_root: std::env::temp_dir(),
            project_workspace_root: None,
            sandbox: None,
            cap_registry: Arc::new(CapabilityRegistry::default()),
            redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
            interrupt: InterruptToken::default(),
            parent_run: None,
            model: None,
            model_config_id: None,
            memory_thread_settings: None,
            actor_source: harness_contracts::PermissionActorSource::ParentRun,
        },
        blob_store,
        event_emitter,
    }
}

fn ticket_for(plan: &ToolActionPlan) -> AuthorizedTicketSummary {
    {
        let ledger = harness_tool::TicketLedger::default();
        let claims = harness_tool::AuthorizationTicketClaims {
            tenant_id: harness_contracts::TenantId::SINGLE,
            session_id: harness_contracts::SessionId::new(),
            run_id: harness_contracts::RunId::new(),
            tool_use_id: plan.tool_use_id,
            tool_name: plan.tool_name.clone(),
            action_plan_hash: plan.plan_hash.clone(),
        };
        let ticket = ledger
            .mint(claims.clone(), chrono::Utc::now())
            .expect("test ticket should mint");
        ledger
            .consume(ticket.id, &claims, chrono::Utc::now())
            .expect("test ticket should consume")
    }
}

async fn execute_read_blob(blob_ref: BlobRef, cap_registry: CapabilityRegistry) -> ToolResult {
    let tool = ReadBlobTool::default();
    let mut ctx = bare_tool_context(Arc::new(cap_registry));
    ctx.tool_use_id = ToolUseId::new();
    let input = json!({ "blob_ref": blob_ref });
    tool.validate(&input, &ctx).await.expect("input validates");
    let plan = tool.plan(&input, &ctx).await.expect("plan builds");
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan))
        .expect("authorized input builds");
    let mut stream = tool
        .execute_authorized(authorized, ctx)
        .await
        .expect("read starts");
    let event = futures::StreamExt::next(&mut stream)
        .await
        .expect("read emits final event");
    match event {
        ToolEvent::Final(result) => result,
        other => panic!("expected final read result, got {other:?}"),
    }
}

fn bare_tool_context(cap_registry: Arc<CapabilityRegistry>) -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: harness_contracts::SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: std::env::temp_dir(),
        project_workspace_root: None,
        sandbox: None,
        cap_registry,
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::default(),
        parent_run: None,
        model: None,
        model_config_id: None,
        memory_thread_settings: None,
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
    }
}

struct AllowOffloadedBlobAuthorizer;

impl OffloadedBlobAuthorizerCap for AllowOffloadedBlobAuthorizer {
    fn authorize_offloaded_blob(
        &self,
        _tenant_id: TenantId,
        _session_id: harness_contracts::SessionId,
        _run_id: harness_contracts::RunId,
        _blob: BlobRef,
    ) -> BoxFuture<'_, Result<(), ToolError>> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Default)]
struct RecordingBlobStore {
    puts: Mutex<Vec<(BlobRef, Bytes, BlobMeta)>>,
    fail: bool,
}

impl RecordingBlobStore {
    fn failing() -> Self {
        Self {
            puts: Mutex::new(Vec::new()),
            fail: true,
        }
    }

    fn puts(&self) -> Vec<(Bytes, BlobMeta)> {
        self.puts
            .lock()
            .iter()
            .map(|(_, bytes, meta)| (bytes.clone(), meta.clone()))
            .collect()
    }
}

#[async_trait]
impl BlobStore for RecordingBlobStore {
    fn store_id(&self) -> &'static str {
        "recording"
    }

    async fn put(
        &self,
        _tenant: TenantId,
        bytes: Bytes,
        meta: BlobMeta,
    ) -> Result<BlobRef, BlobError> {
        if self.fail {
            return Err(BlobError::Backend("backend down".to_owned()));
        }
        let blob_ref = BlobRef {
            id: harness_contracts::BlobId::new(),
            size: meta.size,
            content_hash: meta.content_hash,
            content_type: meta.content_type.clone(),
        };
        self.puts
            .lock()
            .push((blob_ref.clone(), bytes, meta.clone()));
        Ok(blob_ref)
    }

    async fn get(
        &self,
        _tenant: TenantId,
        blob: &BlobRef,
    ) -> Result<futures::stream::BoxStream<'static, Bytes>, BlobError> {
        let bytes = self
            .puts
            .lock()
            .iter()
            .find_map(|(stored_blob, bytes, _)| (stored_blob.id == blob.id).then(|| bytes.clone()))
            .ok_or_else(|| BlobError::NotFound(blob.id))?;
        Ok(Box::pin(stream::once(async move { bytes })))
    }

    async fn head(&self, _tenant: TenantId, blob: &BlobRef) -> Result<Option<BlobMeta>, BlobError> {
        Ok(self
            .puts
            .lock()
            .iter()
            .find_map(|(stored_blob, _, meta)| (stored_blob.id == blob.id).then(|| meta.clone())))
    }

    async fn delete(&self, _tenant: TenantId, _blob: &BlobRef) -> Result<(), BlobError> {
        Ok(())
    }
}

#[derive(Default)]
struct RecordingEmitter {
    events: Mutex<Vec<Event>>,
}

impl RecordingEmitter {
    fn events(&self) -> Vec<Event> {
        self.events.lock().clone()
    }
}

impl ToolEventEmitter for RecordingEmitter {
    fn emit(&self, event: Event) {
        self.events.lock().push(event);
    }
}

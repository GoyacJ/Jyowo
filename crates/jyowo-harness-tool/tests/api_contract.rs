use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures::{future::BoxFuture, stream, stream::BoxStream, StreamExt};
use harness_contracts::{
    BlobReaderCap, BlobRef, BudgetMetric, CapabilityRegistry, Decision, DeferPolicy,
    OverflowAction, ProviderRestriction, ResultBudget, SessionId, TenantId, ToolCapability,
    ToolDescriptor, ToolError, ToolGroup, ToolOrigin, ToolProperties, ToolResult, ToolUseId,
    TrustLevel,
};
use harness_permission::{PermissionBroker, PermissionCheck, PermissionContext, PermissionRequest};
use harness_tool::{
    default_result_budget, InterruptToken, SchemaResolverContext, Tool, ToolContext, ToolEvent,
    ToolProgress, ValidationError,
};
use serde_json::{json, Value};

struct TestBlobReaderCap;

impl BlobReaderCap for TestBlobReaderCap {
    fn read_blob<'a>(
        &'a self,
        _tenant_id: TenantId,
        _blob: BlobRef,
    ) -> BoxFuture<'a, Result<BoxStream<'static, Bytes>, ToolError>> {
        Box::pin(async { Ok(Box::pin(stream::empty()) as BoxStream<'static, Bytes>) })
    }
}

struct TestBroker;

#[async_trait]
impl PermissionBroker for TestBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::AllowOnce
    }

    async fn persist(
        &self,
        _decision: harness_permission::PersistedDecision,
    ) -> Result<(), harness_contracts::PermissionError> {
        Ok(())
    }
}

struct EchoTool {
    descriptor: ToolDescriptor,
}

#[async_trait]
impl Tool for EchoTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &self.descriptor
    }

    async fn validate(&self, _input: &Value, _ctx: &ToolContext) -> Result<(), ValidationError> {
        Ok(())
    }

    async fn check_permission(&self, _input: &Value, _ctx: &ToolContext) -> PermissionCheck {
        PermissionCheck::Allowed
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolContext,
    ) -> Result<harness_tool::ToolStream, ToolError> {
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(input),
        )])))
    }
}

#[tokio::test]
async fn tool_trait_is_dyn_safe_and_defaults_to_descriptor_schemas() {
    let tool: Arc<dyn Tool> = Arc::new(EchoTool {
        descriptor: descriptor(true),
    });

    assert_eq!(tool.input_schema(), &json!({ "type": "object" }));
    assert_eq!(tool.output_schema(), Some(&json!({ "type": "string" })));
    assert!(tool.descriptor().properties.is_concurrency_safe);

    let ctx = SchemaResolverContext {
        run_id: harness_contracts::RunId::new(),
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
    };
    assert_eq!(
        tool.resolve_schema(&ctx).await.unwrap(),
        tool.input_schema().clone()
    );
}

#[tokio::test]
async fn tool_context_retrieves_capabilities_and_reports_missing_handles() {
    let installed: Arc<dyn BlobReaderCap> = Arc::new(TestBlobReaderCap);
    let mut registry = CapabilityRegistry::default();
    registry.install(ToolCapability::BlobReader, Arc::clone(&installed));

    let ctx = ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 0,
        workspace_root: std::env::temp_dir(),
        sandbox: None,
        permission_broker: Arc::new(TestBroker),
        cap_registry: Arc::new(registry),
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::default(),
        parent_run: None,
    };

    let recovered = ctx
        .capability::<dyn BlobReaderCap>(ToolCapability::BlobReader)
        .unwrap();
    assert!(Arc::ptr_eq(&installed, &recovered));

    match ctx.capability::<dyn BlobReaderCap>(ToolCapability::SubagentRunner) {
        Ok(_) => panic!("unexpected capability"),
        Err(error) => assert_eq!(
            error,
            ToolError::CapabilityMissing(ToolCapability::SubagentRunner)
        ),
    }
}

#[tokio::test]
async fn tool_events_and_interrupt_token_are_public_contract_surface() {
    let interrupt = InterruptToken::default();
    assert!(!interrupt.is_interrupted());
    interrupt.interrupt();
    assert!(interrupt.is_interrupted());

    let events = vec![
        ToolEvent::Progress(ToolProgress::now("working")),
        ToolEvent::Partial(harness_contracts::MessagePart::Text("chunk".to_owned())),
        ToolEvent::Final(ToolResult::Text("done".to_owned())),
        ToolEvent::Error(ToolError::Interrupted),
    ];

    let mut stream = stream::iter(events);
    assert!(matches!(
        stream.next().await,
        Some(ToolEvent::Progress(progress)) if progress.message == "working"
    ));
    assert!(matches!(
        stream.next().await,
        Some(ToolEvent::Partial(harness_contracts::MessagePart::Text(text))) if text == "chunk"
    ));
    assert!(matches!(
        stream.next().await,
        Some(ToolEvent::Final(ToolResult::Text(text))) if text == "done"
    ));
    assert_eq!(
        stream.next().await,
        Some(ToolEvent::Error(ToolError::Interrupted))
    );
}

#[test]
fn default_result_budget_uses_adr_010_defaults() {
    assert_eq!(
        default_result_budget(),
        ResultBudget {
            metric: BudgetMetric::Chars,
            limit: 30_000,
            on_overflow: OverflowAction::Offload,
            preview_head_chars: 2_000,
            preview_tail_chars: 2_000,
        }
    );
}

fn descriptor(is_concurrency_safe: bool) -> ToolDescriptor {
    ToolDescriptor {
        name: "echo".to_owned(),
        display_name: "Echo".to_owned(),
        description: "Echo input".to_owned(),
        category: "test".to_owned(),
        group: ToolGroup::Custom("test".to_owned()),
        version: "0.0.1".to_owned(),
        input_schema: json!({ "type": "object" }),
        output_schema: Some(json!({ "type": "string" })),
        dynamic_schema: false,
        properties: ToolProperties {
            is_concurrency_safe,
            is_read_only: true,
            is_destructive: false,
            long_running: None,
            defer_policy: DeferPolicy::AlwaysLoad,
        },
        trust_level: TrustLevel::AdminTrusted,
        required_capabilities: vec![ToolCapability::BlobReader],
        budget: default_result_budget(),
        provider_restriction: ProviderRestriction::All,
        origin: ToolOrigin::Builtin,
        search_hint: None,
    }
}

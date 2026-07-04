use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use futures::{future::BoxFuture, stream, stream::BoxStream, StreamExt};
use harness_contracts::{
    BlobReaderCap, BlobRef, BudgetMetric, CapabilityRegistry, DeferPolicy, OverflowAction,
    ProviderRestriction, ResultBudget, SessionId, TenantId, ToolActionPlan, ToolCapability,
    ToolDescriptor, ToolError, ToolGroup, ToolOrigin, ToolProperties, ToolResult, ToolUseId,
    TrustLevel,
};
use harness_permission::PermissionCheck;
use harness_tool::{
    action_plan_from_permission_check, default_result_budget, AuthorizedTicketSummary,
    AuthorizedToolInput, InterruptToken, SchemaResolverContext, Tool, ToolContext, ToolEvent,
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

    async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError> {
        action_plan_from_permission_check(
            self.descriptor(),
            input,
            ctx,
            PermissionCheck::Allowed,
            Vec::new(),
            harness_contracts::WorkspaceAccess::None,
            harness_contracts::NetworkAccess::None,
        )
    }

    async fn execute_authorized(
        &self,
        authorized: AuthorizedToolInput,
        _ctx: ToolContext,
    ) -> Result<harness_tool::ToolStream, ToolError> {
        Ok(Box::pin(stream::iter([ToolEvent::Final(
            ToolResult::Structured(authorized.raw_input().clone()),
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
        cap_registry: Arc::new(registry),
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
        interrupt: InterruptToken::default(),
        parent_run: None,
        model: None,
        model_config_id: None,
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
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
async fn authorized_tool_input_rejects_plan_hash_mismatch() {
    let tool: Arc<dyn Tool> = Arc::new(EchoTool {
        descriptor: descriptor(true),
    });
    let ctx = tool_ctx(CapabilityRegistry::default());
    let input = json!({ "message": "hello" });
    let plan = tool.plan(&input, &ctx).await.unwrap();
    let mut mismatched_plan = plan.clone();
    mismatched_plan.plan_hash = harness_contracts::ActionPlanHash::from_bytes([7; 32]);

    let error = AuthorizedToolInput::new(input, mismatched_plan, ticket_for(&plan)).unwrap_err();

    assert_eq!(
        error,
        ToolError::PermissionDenied(
            "authorization ticket action plan hash does not match action plan".to_owned()
        )
    );
}

fn tool_ctx(cap_registry: CapabilityRegistry) -> ToolContext {
    ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: SessionId::new(),
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
        actor_source: harness_contracts::PermissionActorSource::ParentRun,
    }
}

fn ticket_for(plan: &ToolActionPlan) -> AuthorizedTicketSummary {
    AuthorizedTicketSummary {
        ticket_id: harness_contracts::AuthorizationTicketId::new(),
        tenant_id: TenantId::SINGLE,
        session_id: SessionId::new(),
        run_id: harness_contracts::RunId::new(),
        tool_use_id: plan.tool_use_id,
        tool_name: plan.tool_name.clone(),
        action_plan_hash: plan.plan_hash.clone(),
        consumed_at: Utc::now(),
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
        service_binding: None,
    }
}

#[tokio::test]
async fn action_plan_propagates_actor_source_from_context() {
    use harness_contracts::PermissionActorSource;
    use harness_contracts::{NetworkAccess, WorkspaceAccess};
    use harness_permission::PermissionCheck;
    use harness_tool::{action_plan_from_permission_check, ToolContext};

    let ctx = ToolContext {
        tool_use_id: ToolUseId::new(),
        run_id: harness_contracts::RunId::new(),
        session_id: SessionId::new(),
        tenant_id: TenantId::SINGLE,
        correlation_id: harness_contracts::CorrelationId::new(),
        agent_id: harness_contracts::AgentId::from_u128(1),
        subagent_depth: 1,
        workspace_root: std::env::temp_dir(),
        sandbox: None,
        cap_registry: Arc::new(CapabilityRegistry::default()),
        redactor: std::sync::Arc::new(harness_contracts::NoopRedactor),
        interrupt: harness_tool::InterruptToken::default(),
        parent_run: None,
        model: None,
        model_config_id: None,
        actor_source: PermissionActorSource::Subagent {
            subagent_id: harness_contracts::SubagentId::new(),
            parent_session_id: SessionId::new(),
            parent_run_id: harness_contracts::RunId::new(),
            team_id: None,
            team_member_profile_id: None,
        },
    };

    let descriptor = descriptor(false);
    let plan = action_plan_from_permission_check(
        &descriptor,
        &serde_json::json!({"key": "val"}),
        &ctx,
        PermissionCheck::Allowed,
        Vec::new(),
        WorkspaceAccess::None,
        NetworkAccess::None,
    )
    .expect("plan created");

    assert!(
        matches!(plan.actor_source, PermissionActorSource::Subagent { .. }),
        "action plan should carry Subagent actor source, got {:?}",
        plan.actor_source
    );
}

use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use harness_contracts::{
    CapabilityRegistry, CorrelationId, NetworkAccess, ResourceLimits, SandboxMode, SandboxPolicy,
    SandboxScope, SubagentRunnerCap, ToolActionPlan, ToolCapability, ToolError, ToolResult,
    UsageSnapshot,
};
use harness_subagent::{
    AgentTool, AnnounceMode, DelegationBlocklist, DelegationPolicy, McpServerRef, MemorySelector,
    ParentContext, RequiredSandboxCapabilities, ResourceQuota, SandboxInheritance,
    SubagentAnnouncement, SubagentContextMode, SubagentHandle, SubagentInputStrategy,
    SubagentMemoryScope, SubagentRunner, SubagentRunnerCapAdapter, SubagentSpec, SubagentStatus,
};
use harness_tool::{
    AuthorizedTicketSummary, AuthorizedToolInput, Tool, ToolContext, ToolEvent, ToolStream,
};
use serde_json::json;
use tokio::sync::Mutex;

#[test]
fn default_blocklist_blocks_delegation_escape_tools() {
    let blocklist = DelegationBlocklist::default();

    for tool in [
        "agent",
        "delegate",
        "clarify",
        "memory_write",
        "send_user_message",
        "execute_code",
    ] {
        assert!(
            blocklist.contains(tool),
            "{tool} should be blocked by default"
        );
    }
}

#[test]
fn delegation_policy_uses_safe_defaults() {
    let policy = DelegationPolicy::default();

    assert_eq!(policy.max_depth, 1);
    assert_eq!(policy.max_concurrent_children, 3);
    assert!(policy.blocklist.contains("send_user_message"));
}

#[test]
fn subagent_announce_mode_uses_spec_names_and_decodes_legacy_values() {
    assert_eq!(
        serde_json::to_value(AnnounceMode::StructuredOnly).unwrap(),
        json!("structured_only")
    );
    assert_eq!(
        serde_json::to_value(AnnounceMode::SummaryText).unwrap(),
        json!("summary_text")
    );
    assert_eq!(
        serde_json::to_value(AnnounceMode::FullTranscript).unwrap(),
        json!("full_transcript")
    );

    assert_eq!(
        serde_json::from_value::<AnnounceMode>(json!("structured")).unwrap(),
        AnnounceMode::StructuredOnly
    );
    assert_eq!(
        serde_json::from_value::<AnnounceMode>(json!("summary")).unwrap(),
        AnnounceMode::SummaryText
    );
}

#[test]
fn subagent_spec_serializes_required_sandbox_capabilities() {
    let mut spec = SubagentSpec::minimal("worker", "needs sandbox capabilities");
    spec.sandbox_policy = SandboxInheritance::Require(RequiredSandboxCapabilities {
        supports_network: true,
        supports_filesystem_write: true,
        supports_session_snapshot: true,
        min_concurrent_execs: Some(2),
        ..RequiredSandboxCapabilities::default()
    });

    let value = serde_json::to_value(&spec).unwrap();
    assert_eq!(value["sandbox_policy"]["require"]["supports_network"], true);
    assert_eq!(
        value["sandbox_policy"]["require"]["supports_filesystem_write"],
        true
    );
    assert_eq!(
        value["sandbox_policy"]["require"]["supports_session_snapshot"],
        true
    );
    assert_eq!(
        value["sandbox_policy"]["require"]["min_concurrent_execs"],
        2
    );

    let decoded: SubagentSpec = serde_json::from_value(value).unwrap();
    assert_eq!(decoded.sandbox_policy, spec.sandbox_policy);
}

#[test]
fn subagent_spec_serializes_sandbox_override_policy() {
    let mut spec = SubagentSpec::minimal("worker", "override sandbox");
    let override_policy = SandboxPolicy {
        mode: SandboxMode::Container,
        scope: SandboxScope::WorkspaceOnly,
        network: NetworkAccess::LoopbackOnly,
        resource_limits: ResourceLimits {
            max_memory_bytes: Some(64 * 1024 * 1024),
            max_cpu_cores: None,
            max_pids: None,
            max_wall_clock_ms: Some(1_000),
            max_open_files: None,
        },
        denied_host_paths: Vec::new(),
    };
    spec.sandbox_policy = SandboxInheritance::Override(override_policy.clone());

    let value = serde_json::to_value(&spec).unwrap();
    assert_eq!(
        value["sandbox_policy"]["override"]["network"],
        "loopback_only"
    );

    let decoded: SubagentSpec = serde_json::from_value(value).unwrap();
    assert_eq!(
        decoded.sandbox_policy,
        SandboxInheritance::Override(override_policy)
    );
}

#[test]
fn subagent_spec_uses_isolated_latest_user_defaults() {
    let spec = SubagentSpec::minimal("worker", "inspect");

    assert_eq!(spec.context_mode, SubagentContextMode::Isolated);
    assert_eq!(spec.input_strategy, SubagentInputStrategy::LatestUserOnly);
}

#[test]
fn subagent_spec_serializes_shared_resource_quota_shape() {
    let mut spec = SubagentSpec::minimal("worker", "inspect");
    spec.quota = Some(ResourceQuota {
        max_tokens: Some(1_024),
        max_tool_calls: Some(3),
        max_duration: Some(std::time::Duration::from_secs(30)),
        max_cost_cents: Some(25),
    });

    let value = serde_json::to_value(&spec).unwrap();

    assert_eq!(value["quota"]["max_tokens"], 1_024);
    assert_eq!(value["quota"]["max_tool_calls"], 3);
    assert_eq!(value["quota"]["max_cost_cents"], 25);
    let decoded: SubagentSpec = serde_json::from_value(value).unwrap();
    assert_eq!(decoded.quota, spec.quota);
}

#[test]
fn subagent_context_mode_serializes_fork_from_parent() {
    let value = serde_json::to_value(SubagentContextMode::ForkFromParent {
        include_tool_results: true,
    })
    .unwrap();

    assert_eq!(
        value,
        json!({ "fork_from_parent": { "include_tool_results": true } })
    );
    let decoded: SubagentContextMode = serde_json::from_value(value).unwrap();
    assert_eq!(
        decoded,
        SubagentContextMode::ForkFromParent {
            include_tool_results: true
        }
    );
}

#[test]
fn subagent_input_strategy_serializes_custom_selector() {
    let value = serde_json::to_value(SubagentInputStrategy::Custom {
        selector_id: "recent-safe-context".to_owned(),
    })
    .unwrap();

    assert_eq!(
        value,
        json!({ "custom": { "selector_id": "recent-safe-context" } })
    );
    let decoded: SubagentInputStrategy = serde_json::from_value(value).unwrap();
    assert_eq!(
        decoded,
        SubagentInputStrategy::Custom {
            selector_id: "recent-safe-context".to_owned()
        }
    );
}

#[test]
fn subagent_mcp_servers_use_typed_refs_with_string_serde_compatibility() {
    let mut spec = SubagentSpec::minimal("worker", "inspect");
    spec.mcp_servers = vec![McpServerRef::new("srv-a")];
    spec.required_mcp_servers = vec![McpServerRef::new("srv-b")];

    let value = serde_json::to_value(&spec).unwrap();

    assert_eq!(value["mcp_servers"], json!(["srv-a"]));
    assert_eq!(value["required_mcp_servers"], json!(["srv-b"]));
    let decoded: SubagentSpec = serde_json::from_value(value).unwrap();
    assert_eq!(decoded.mcp_servers[0].server_id(), "srv-a");
    assert_eq!(decoded.required_mcp_servers[0].server_id(), "srv-b");
}

#[test]
fn subagent_memory_scope_serializes_subset_selectors() {
    let scope = SubagentMemoryScope::Subset {
        selectors: vec![MemorySelector::Tag("safe".to_owned())],
    };

    let value = serde_json::to_value(&scope).unwrap();

    assert_eq!(
        value,
        json!({ "subset": { "selectors": [{ "tag": "safe" }] } })
    );
    let decoded: SubagentMemoryScope = serde_json::from_value(value).unwrap();
    assert_eq!(decoded, scope);
}

#[tokio::test]
async fn cap_adapter_spawns_through_inner_runner() {
    let runner = Arc::new(RecordingRunner);
    let cap = SubagentRunnerCapAdapter::from_runner(runner);
    let parent = ParentContext::for_test(0);

    let handle = cap
        .spawn(
            serde_json::to_value(SubagentSpec::minimal("reviewer", "summarize")).unwrap(),
            parent.into(),
        )
        .await
        .unwrap();
    let announcement = handle.wait().await.unwrap();

    assert_eq!(announcement.status, SubagentStatus::Completed);
    assert_eq!(announcement.summary, "spawned reviewer");
}

#[tokio::test]
async fn agent_tool_uses_subagent_runner_capability() {
    let runner = Arc::new(RecordingRunner);
    let mut registry = CapabilityRegistry::default();
    registry.install::<dyn SubagentRunnerCap>(
        ToolCapability::SubagentRunner,
        SubagentRunnerCapAdapter::from_runner(runner),
    );

    let tool = AgentTool::default();
    let ctx = harness_subagent::testing::tool_context_with_caps(Arc::new(registry));
    let stream = execute_authorized_tool(
        &tool,
        json!({
            "role": "reviewer",
            "task": "summarize",
            "prompt_template": { "body": "summarize" }
        }),
        ctx,
    )
    .await
    .unwrap();
    let events: Vec<_> = stream.collect().await;

    assert!(matches!(
        events.last(),
        Some(ToolEvent::Final(ToolResult::Structured(value)))
            if value["summary"] == "spawned reviewer"
    ));
}

#[tokio::test]
async fn agent_tool_forwards_subagent_depth_and_correlation_from_tool_context() {
    let runner = Arc::new(CapturingRunner::default());
    let mut registry = CapabilityRegistry::default();
    registry.install::<dyn SubagentRunnerCap>(
        ToolCapability::SubagentRunner,
        SubagentRunnerCapAdapter::from_runner(runner.clone()),
    );

    let tool = AgentTool::default();
    let mut ctx = harness_subagent::testing::tool_context_with_caps(Arc::new(registry));
    ctx.subagent_depth = 2;
    let correlation_id = CorrelationId::new();
    ctx.correlation_id = correlation_id;

    let stream = execute_authorized_tool(
        &tool,
        json!({
            "role": "reviewer",
            "task": "summarize"
        }),
        ctx,
    )
    .await
    .unwrap();
    let _: Vec<_> = stream.collect().await;

    let captured = runner.parent.lock().await.clone().unwrap();
    assert_eq!(captured.depth, 2);
    assert_eq!(captured.correlation_id, correlation_id);
}

async fn execute_authorized_tool<T: Tool + ?Sized>(
    tool: &T,
    input: serde_json::Value,
    ctx: ToolContext,
) -> Result<ToolStream, ToolError> {
    tool.validate(&input, &ctx)
        .await
        .expect("test input validates");
    let plan = tool.plan(&input, &ctx).await?;
    let authorized = AuthorizedToolInput::new(input, plan.clone(), ticket_for(&plan))?;
    tool.execute_authorized(authorized, ctx).await
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

#[derive(Clone)]
struct RecordingRunner;

#[async_trait]
impl SubagentRunner for RecordingRunner {
    async fn spawn(
        &self,
        spec: SubagentSpec,
        _input: harness_contracts::TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, harness_subagent::SubagentError> {
        Ok(SubagentHandle::ready(SubagentAnnouncement {
            subagent_id: harness_contracts::SubagentId::new(),
            parent_session_id: parent_ctx.parent_session_id,
            status: SubagentStatus::Completed,
            summary: format!("spawned {}", spec.role),
            result: Some(json!({ "role": spec.role })),
            usage: UsageSnapshot::default(),
            transcript_ref: None,
            context_report: None,
        }))
    }
}

#[derive(Default)]
struct CapturingRunner {
    parent: Mutex<Option<ParentContext>>,
}

#[async_trait]
impl SubagentRunner for CapturingRunner {
    async fn spawn(
        &self,
        spec: SubagentSpec,
        _input: harness_contracts::TurnInput,
        parent_ctx: ParentContext,
    ) -> Result<SubagentHandle, harness_subagent::SubagentError> {
        self.parent.lock().await.replace(parent_ctx.clone());
        Ok(SubagentHandle::ready(SubagentAnnouncement {
            subagent_id: harness_contracts::SubagentId::new(),
            parent_session_id: parent_ctx.parent_session_id,
            status: SubagentStatus::Completed,
            summary: format!("spawned {}", spec.role),
            result: Some(json!({ "role": spec.role })),
            usage: UsageSnapshot::default(),
            transcript_ref: None,
            context_report: None,
        }))
    }
}

#![cfg(feature = "agents-subagent")]

use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use harness_agent_runtime::{
    assemble_subagent_runner, delegation_policy_from_run_options, should_install_subagent_runner,
    SubagentRunnerAssemblyInput,
};
use harness_contracts::{
    AgentToolPolicy, AgentUsePolicy, AgentWorkspaceIsolationMode, CorrelationId, DecidedBy,
    Decision, DecisionScope, Event, NoopRedactor, PermissionMode, PermissionSubject, RequestId,
    RunId, SessionId, Severity, SubagentId, TenantId, ToolUseId,
};
use harness_journal::{EventStore, InMemoryEventStore, ReplayCursor};
use harness_permission::{
    PermissionBroker, PermissionContext, PermissionError, PermissionRequest, PersistedDecision,
    RuleSnapshot,
};
use harness_subagent::SubagentPermissionBridge;

fn sample_subagent_tool_policy() -> AgentToolPolicy {
    AgentToolPolicy {
        subagents: AgentUsePolicy::Allowed,
        agent_team: AgentUsePolicy::Off,
        team_config: None,
        background_agents: AgentUsePolicy::Off,
        workspace_isolation: AgentWorkspaceIsolationMode::ReadOnly,
        max_depth: 2,
        max_concurrent_subagents: 2,
        max_team_members: 4,
    }
}

#[test]
fn should_install_subagent_runner_when_allowed() {
    let options = sample_subagent_tool_policy();
    assert!(should_install_subagent_runner(&options));
}

#[test]
fn should_not_install_subagent_runner_when_off() {
    let mut options = sample_subagent_tool_policy();
    options.subagents = AgentUsePolicy::Off;
    assert!(!should_install_subagent_runner(&options));
}

#[test]
fn should_not_install_subagent_runner_when_depth_disallows_delegation() {
    let mut options = sample_subagent_tool_policy();
    options.max_depth = 0;
    assert!(!should_install_subagent_runner(&options));
}

#[test]
fn delegation_policy_reflects_run_options() {
    let options = sample_subagent_tool_policy();
    let policy = delegation_policy_from_run_options(&options);
    assert_eq!(policy.max_depth, 2);
    assert_eq!(policy.max_concurrent_children, 2);
}

struct AllowBroker;

#[async_trait]
impl PermissionBroker for AllowBroker {
    async fn decide(&self, _request: PermissionRequest, _ctx: PermissionContext) -> Decision {
        Decision::AllowOnce
    }

    async fn persist(&self, _decision: PersistedDecision) -> Result<(), PermissionError> {
        Ok(())
    }
}

#[tokio::test]
async fn permission_bridge_attributes_subagent_source_on_forward_and_resolve() {
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let parent_session_id = SessionId::new();
    let parent_run_id = RunId::new();
    let child_session_id = SessionId::new();
    let child_run_id = RunId::new();
    let subagent_id = SubagentId::new();
    let correlation_id = CorrelationId::new();
    let bridge = SubagentPermissionBridge::new(
        Arc::new(AllowBroker),
        store.clone(),
        TenantId::SINGLE,
        parent_session_id,
        parent_run_id,
        subagent_id,
    )
    .with_child_context(child_session_id, child_run_id, correlation_id);
    let request_id = RequestId::new();
    let subject = PermissionSubject::ToolInvocation {
        tool: "FileWrite".to_owned(),
        input: serde_json::json!({ "path": "README.md" }),
    };

    let decision = bridge
        .decide(
            PermissionRequest {
                request_id,
                tenant_id: TenantId::SINGLE,
                session_id: child_session_id,
                tool_use_id: ToolUseId::new(),
                tool_name: "FileWrite".to_owned(),
                subject: subject.clone(),
                severity: Severity::High,
                scope_hint: DecisionScope::Any,
                created_at: harness_contracts::now(),
            },
            PermissionContext {
                permission_mode: PermissionMode::Default,
                previous_mode: None,
                session_id: child_session_id,
                tenant_id: TenantId::SINGLE,
                run_id: None,
                interactivity: harness_contracts::InteractivityLevel::FullyInteractive,
                timeout_policy: None,
                fallback_policy: harness_contracts::FallbackPolicy::DenyAll,
                rule_snapshot: Arc::new(RuleSnapshot {
                    rules: Vec::new(),
                    generation: 0,
                    built_at: harness_contracts::now(),
                }),
                hook_overrides: Vec::new(),
            },
        )
        .await;

    assert_eq!(decision, Decision::AllowOnce);
    let parent_events: Vec<_> = store
        .read(TenantId::SINGLE, parent_session_id, ReplayCursor::FromStart)
        .await
        .unwrap()
        .collect()
        .await;
    assert!(parent_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentPermissionForwarded(forwarded)
                if forwarded.parent_session_id == parent_session_id
                    && forwarded.subagent_id == subagent_id
                    && forwarded.original_request_id == request_id
        )
    }));
    assert!(parent_events.iter().any(|event| {
        matches!(
            event,
            Event::SubagentPermissionResolved(resolved)
                if resolved.parent_session_id == parent_session_id
                    && resolved.subagent_id == subagent_id
                    && resolved.original_request_id == request_id
                    && matches!(resolved.decided_by, DecidedBy::ParentForwarded { .. })
        )
    }));
}

struct NoopEngineFactory;

#[async_trait]
impl harness_subagent::SubagentEngineFactory for NoopEngineFactory {
    async fn run_child_engine(
        &self,
        _request: harness_subagent::ChildRunRequest,
    ) -> Result<harness_subagent::ChildRunOutcome, harness_subagent::SubagentError> {
        Ok(harness_subagent::ChildRunOutcome {
            status: harness_contracts::SubagentStatus::Completed,
            summary: "noop".to_owned(),
            result: None,
            usage: harness_contracts::UsageSnapshot::default(),
            transcript_ref: None,
            context_report: None,
        })
    }
}

#[test]
fn assemble_subagent_runner_builds_default_runner_with_policy() {
    let workspace = tempfile::tempdir().unwrap();
    let store: Arc<InMemoryEventStore> = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let options = sample_subagent_tool_policy();
    let runner = assemble_subagent_runner(SubagentRunnerAssemblyInput {
        agent_tool_policy: options.clone(),
        engine_factory: Arc::new(NoopEngineFactory),
        event_store: store,
        workspace_root: workspace.path().to_path_buf(),
        team_attribution: None,
    });
    assert!(Arc::strong_count(&runner) >= 1);
    let policy = delegation_policy_from_run_options(&options);
    assert_eq!(policy.max_concurrent_children, 2);
}

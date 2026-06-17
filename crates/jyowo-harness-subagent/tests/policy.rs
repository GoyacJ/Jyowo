use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_contracts::{BudgetKind, NoopRedactor, UsageSnapshot};
use harness_journal::InMemoryEventStore;
use harness_subagent::{
    ChildRunOutcome, ChildRunRequest, ChildSessionRunner, ConcurrencyPolicy,
    ConcurrentSubagentPool, DefaultSubagentRunner, DelegationPolicy, ParentContext, ResourceQuota,
    SubagentError, SubagentRunner, SubagentSpec,
};

#[tokio::test]
async fn policy_enforces_hard_depth_cap_separately_from_requested_depth() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let runner = DefaultSubagentRunner::new(
        Arc::new(UsageChildRunner::default()),
        store,
        workspace.path(),
        DelegationPolicy {
            max_depth: 8,
            depth_cap: 1,
            max_concurrent_children: 3,
            max_global_children: 128,
            blocklist: Default::default(),
        },
    );
    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.max_depth = 8;

    let error = runner
        .spawn(spec, test_input("inspect"), ParentContext::for_test(1))
        .await
        .expect_err("hard depth cap should reject nested spawn");

    assert_eq!(error, SubagentError::DepthExceeded { current: 1, max: 1 });
}

#[test]
fn pool_enforces_global_limit_across_parent_depth_buckets() {
    let pool = ConcurrentSubagentPool::with_policy(ConcurrencyPolicy {
        per_bucket_limit: 8,
        global_limit: 1,
        acquire_timeout: Duration::from_millis(1),
        activity_timeout: Duration::from_secs(30),
    });
    let parent = ParentContext::for_test(0);
    let other_parent = ParentContext::for_test(1);
    let _first = pool.try_acquire(&parent).unwrap();

    let error = pool
        .try_acquire(&other_parent)
        .expect_err("global limit should apply across buckets");

    assert_eq!(error, SubagentError::ConcurrentLimitExceeded);
}

#[tokio::test]
async fn policy_enforces_usage_quota_when_child_usage_is_available() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let runner = DefaultSubagentRunner::new(
        Arc::new(UsageChildRunner {
            usage: UsageSnapshot {
                input_tokens: 10,
                output_tokens: 5,
                ..UsageSnapshot::default()
            },
        }),
        store,
        workspace.path(),
        DelegationPolicy::default(),
    );
    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.quota = Some(ResourceQuota {
        max_tokens: Some(4),
        max_tool_calls: None,
        max_duration: None,
        max_cost_cents: None,
    });

    let handle = runner
        .spawn(spec, test_input("inspect"), ParentContext::for_test(0))
        .await
        .expect("token quota should announce max budget");
    let announcement = handle.wait().await.unwrap();

    assert_eq!(
        announcement.status,
        harness_subagent::SubagentStatus::MaxBudget(BudgetKind::Tokens)
    );
    assert_eq!(announcement.usage.input_tokens, 10);
    assert_eq!(announcement.usage.output_tokens, 5);
}

#[tokio::test]
async fn policy_maps_tool_call_quota_to_max_budget_announcement() {
    let workspace = tempfile::tempdir().unwrap();
    let store = Arc::new(InMemoryEventStore::new(Arc::new(NoopRedactor)));
    let runner = DefaultSubagentRunner::new(
        Arc::new(UsageChildRunner {
            usage: UsageSnapshot {
                tool_calls: 2,
                ..UsageSnapshot::default()
            },
        }),
        store,
        workspace.path(),
        DelegationPolicy::default(),
    );
    let mut spec = SubagentSpec::minimal("reviewer", "inspect");
    spec.quota = Some(ResourceQuota {
        max_tokens: None,
        max_tool_calls: Some(1),
        max_duration: None,
        max_cost_cents: None,
    });

    let handle = runner
        .spawn(spec, test_input("inspect"), ParentContext::for_test(0))
        .await
        .expect("tool call quota should announce max budget");
    let announcement = handle.wait().await.unwrap();

    assert_eq!(
        announcement.status,
        harness_subagent::SubagentStatus::MaxBudget(BudgetKind::ToolCalls)
    );
    assert_eq!(announcement.usage.tool_calls, 2);
}

#[derive(Default)]
struct UsageChildRunner {
    usage: UsageSnapshot,
}

#[async_trait]
impl ChildSessionRunner for UsageChildRunner {
    async fn run_child(&self, _request: ChildRunRequest) -> Result<ChildRunOutcome, SubagentError> {
        Ok(ChildRunOutcome {
            status: harness_subagent::SubagentStatus::Completed,
            summary: "done".to_owned(),
            result: None,
            usage: self.usage.clone(),
            transcript_ref: None,
            context_report: None,
        })
    }
}

fn test_input(text: &str) -> harness_contracts::TurnInput {
    harness_contracts::TurnInput {
        message: harness_contracts::Message {
            id: harness_contracts::MessageId::new(),
            role: harness_contracts::MessageRole::User,
            parts: vec![harness_contracts::MessagePart::Text(text.to_owned())],
            created_at: harness_contracts::now(),
        },
        metadata: serde_json::Value::Null,
    }
}

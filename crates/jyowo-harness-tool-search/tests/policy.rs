use harness_contracts::DeferPolicy;
use harness_tool_search::{ToolLoadingBackend, ToolSearchMode, ToolSearchScorer};

#[test]
fn policy_reexports_shared_tool_search_mode() {
    assert_eq!(ToolSearchMode::default().min_absolute_tokens(), 4_000);
}

#[test]
fn policy_reuses_contracts_defer_policy() {
    assert_eq!(DeferPolicy::ForceDefer, DeferPolicy::ForceDefer);
}

#[test]
fn traits_are_object_safe() {
    fn accepts_backend(_: Option<&dyn ToolLoadingBackend>) {}
    fn accepts_scorer(_: Option<&dyn ToolSearchScorer>) {}

    accepts_backend(None);
    accepts_scorer(None);
}

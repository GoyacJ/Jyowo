use harness_contracts::{ContentHash, SubagentContextReport};

#[test]
fn subagent_context_report_serializes_prompt_cache_and_bootstrap_facts() {
    let report = SubagentContextReport {
        parent_system_hash: Some(ContentHash([1; 32])),
        child_system_hash: ContentHash([2; 32]),
        shared_system_prefix_hash: Some(ContentHash([3; 32])),
        prompt_cache_prefix_reused: true,
        bootstrap_files_inherited: vec!["AGENTS.md".to_owned()],
        system_header_extra_applied: true,
    };

    let value = serde_json::to_value(&report).unwrap();
    assert_eq!(value["prompt_cache_prefix_reused"], true);
    assert_eq!(
        value["bootstrap_files_inherited"],
        serde_json::json!(["AGENTS.md"])
    );
    assert_eq!(value["system_header_extra_applied"], true);
    let roundtrip: SubagentContextReport = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, report);
}

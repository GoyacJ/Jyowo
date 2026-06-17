use chrono::{TimeZone, Utc};
use harness_contracts::{ToolPoolChangeSource, ToolSearchMode};
use harness_tool_search::{DeferredThresholdEvaluator, DeferredToolsDelta, TOOL_SEARCH_PROMPT};

#[test]
fn tool_search_prompt_keeps_expected_query_contract() {
    assert!(TOOL_SEARCH_PROMPT.contains("select:Read,Edit,Grep"));
    assert!(TOOL_SEARCH_PROMPT.contains("+slack send"));
    assert!(TOOL_SEARCH_PROMPT.contains("Deferred tools appear by name"));
}

#[test]
fn deferred_tools_delta_attachment_is_stable() {
    let delta = DeferredToolsDelta {
        added_names: vec![
            "mcp__slack__post_message".to_owned(),
            "mcp__slack__list_channels".to_owned(),
        ],
        removed_names: vec!["legacy_tool".to_owned()],
        source: ToolPoolChangeSource::InitialClassification,
        at: Utc.with_ymd_and_hms(2026, 4, 25, 10, 32, 11).unwrap(),
        initial: false,
    };

    assert_eq!(
        delta.to_attachment_text(),
        concat!(
            "<deferred-tools changed-at=\"2026-04-25T10:32:11+00:00\">\n",
            "  <added>\n",
            "    mcp__slack__post_message\n",
            "    mcp__slack__list_channels\n",
            "  </added>\n",
            "  <removed>\n",
            "    legacy_tool\n",
            "  </removed>\n",
            "</deferred-tools>"
        )
    );
}

#[test]
fn threshold_evaluator_uses_auto_ratio_and_absolute_floor() {
    let evaluator = DeferredThresholdEvaluator;
    let mode = ToolSearchMode::Auto {
        ratio: 0.10,
        min_absolute_tokens: 4_000,
    };

    let (disabled, metrics) = evaluator.evaluate_chars(&mode, 5_000, 200_000);
    assert!(!disabled);
    assert_eq!(metrics.threshold_tokens, 20_000);
    assert_eq!(metrics.absolute_floor, 4_000);

    let (enabled, _) = evaluator.evaluate_chars(&mode, 60_000, 200_000);
    assert!(enabled);
}

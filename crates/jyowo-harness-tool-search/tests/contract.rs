use chrono::{TimeZone, Utc};
use harness_contracts::{ToolPoolChangeSource, ToolSearchMode};
use harness_tool_search::{DeferredThresholdEvaluator, DeferredToolsDelta, TOOL_SEARCH_PROMPT};
use std::collections::BTreeMap;

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
        removed_names: vec!["old_tool".to_owned()],
        source: ToolPoolChangeSource::InitialClassification,
        at: Utc.with_ymd_and_hms(2026, 4, 25, 10, 32, 11).unwrap(),
        initial: false,
        reason: "deferred tool pool changed after initial classification".to_owned(),
        added_reasons: BTreeMap::from([
            (
                "mcp__slack__post_message".to_owned(),
                "matched messaging task".to_owned(),
            ),
            (
                "mcp__slack__list_channels".to_owned(),
                "matched messaging task".to_owned(),
            ),
        ]),
        removed_reasons: BTreeMap::from([(
            "old_tool".to_owned(),
            "tool is no longer deferred".to_owned(),
        )]),
    };

    assert_eq!(
        delta.to_attachment_text(),
        concat!(
            "<deferred-tools changed-at=\"2026-04-25T10:32:11+00:00\" reason=\"deferred tool pool changed after initial classification\">\n",
            "  <added>\n",
            "    <tool name=\"mcp__slack__post_message\" reason=\"matched messaging task\" />\n",
            "    <tool name=\"mcp__slack__list_channels\" reason=\"matched messaging task\" />\n",
            "  </added>\n",
            "  <removed>\n",
            "    <tool name=\"old_tool\" reason=\"tool is no longer deferred\" />\n",
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

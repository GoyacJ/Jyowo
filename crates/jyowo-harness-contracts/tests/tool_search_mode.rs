use harness_contracts::ToolSearchMode;

#[test]
fn tool_search_mode_default_matches_adr_009() {
    assert_eq!(
        ToolSearchMode::default(),
        ToolSearchMode::Auto {
            ratio: 0.10,
            min_absolute_tokens: 4_000,
        }
    );
}

#[test]
fn tool_search_mode_serializes_with_snake_case_tags() {
    let value = serde_json::to_value(ToolSearchMode::Always).unwrap();
    assert_eq!(value, serde_json::json!("always"));

    let value = serde_json::to_value(ToolSearchMode::Disabled).unwrap();
    assert_eq!(value, serde_json::json!("disabled"));
}

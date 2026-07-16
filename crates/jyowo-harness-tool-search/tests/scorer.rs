use std::collections::HashSet;
use std::sync::Arc;

use harness_contracts::{
    DeferPolicy, ProviderRestriction, ToolCapability, ToolDescriptor, ToolDescriptorMetadata,
    ToolGroup, ToolIntegrationSource, ToolOrigin, ToolProperties, ToolRiskLevel, TrustLevel,
};
use harness_tool_search::{
    parse_tool_name_parts, DefaultScorer, ScoringContext, ScoringTerms, ToolSearchScorer,
};
use serde_json::json;

#[tokio::test]
async fn parses_mcp_snake_and_camel_case_names() {
    assert_eq!(
        parse_tool_name_parts("mcp__slack_server__post_message"),
        ["slack", "server", "post", "message"]
    );
    assert_eq!(
        parse_tool_name_parts("file_read_tool"),
        ["file", "read", "tool"]
    );
    assert_eq!(
        parse_tool_name_parts("FileReadTool"),
        ["file", "read", "tool"]
    );
}

#[tokio::test]
async fn required_terms_filter_candidates() {
    let scorer = DefaultScorer::default();
    let context = ScoringContext::default();
    let terms = ScoringTerms::parse("+slack message");

    let matching = scorer
        .score(
            &descriptor("mcp__slack__post_message", "Post message", None),
            &props(),
            &terms,
            &context,
        )
        .await;
    let missing = scorer
        .score(
            &descriptor("mcp__github__create_issue", "Create issue", None),
            &props(),
            &terms,
            &context,
        )
        .await;

    assert!(matching > 0);
    assert_eq!(missing, 0);
}

#[tokio::test]
async fn search_hint_and_description_contribute_to_score() {
    let scorer = DefaultScorer::default();
    let context = ScoringContext::default();
    let terms = ScoringTerms::parse("notebook");
    let score = scorer
        .score(
            &descriptor(
                "RunCell",
                "Execute a Jupyter notebook cell",
                Some("notebook jupyter"),
            ),
            &props(),
            &terms,
            &context,
        )
        .await;

    assert!(score >= 6);
}

#[tokio::test]
async fn discovered_tools_are_penalized_but_still_searchable() {
    let scorer = DefaultScorer::default();
    let terms = ScoringTerms::parse("slack");
    let tool = descriptor("mcp__slack__post_message", "Post message", None);
    let normal = scorer
        .score(&tool, &props(), &terms, &ScoringContext::default())
        .await;
    let penalized = scorer
        .score(
            &tool,
            &props(),
            &terms,
            &ScoringContext {
                discovered: Arc::new(HashSet::from([tool.name.clone()])),
            },
        )
        .await;

    assert!(penalized > 0);
    assert!(penalized < normal);
}

#[tokio::test]
async fn aliases_and_examples_are_searchable() {
    let scorer = DefaultScorer::default();
    let context = ScoringContext::default();
    let terms = ScoringTerms::parse("commit");
    let mut tool = descriptor("GitStage", "Stage files", None);
    tool.metadata.aliases = vec!["git add".to_owned()];
    tool.metadata.examples = vec!["Stage files before commit".to_owned()];

    let score = scorer.score(&tool, &props(), &terms, &context).await;

    assert!(score > 0);
}

#[tokio::test]
async fn required_capabilities_contribute_to_score() {
    let scorer = DefaultScorer::default();
    let context = ScoringContext::default();
    let terms = ScoringTerms::parse("blob");
    let mut tool = descriptor("ReadBlob", "Read stored artifact", None);
    tool.required_capabilities = vec![ToolCapability::BlobReader];

    let score = scorer.score(&tool, &props(), &terms, &context).await;

    assert!(score > 0);
}

#[tokio::test]
async fn metadata_filters_select_matching_tools() {
    let scorer = DefaultScorer::default();
    let context = ScoringContext::default();
    let terms = ScoringTerms::parse("status group:git family:git platform:codex effect:reads_git risk:low modality:text source:builtin");
    let matching = git_descriptor("GitStatus", "Show git status");
    let mut missing = descriptor("WebSearch", "Search the web", None);
    missing.group = ToolGroup::Network;
    missing.metadata.platforms = vec!["codex".to_owned()];

    let matching_score = scorer.score(&matching, &props(), &terms, &context).await;
    let missing_score = scorer.score(&missing, &props(), &terms, &context).await;

    assert!(matching_score > 0);
    assert_eq!(missing_score, 0);
}

fn descriptor(name: &str, description: &str, search_hint: Option<&str>) -> ToolDescriptor {
    ToolDescriptor {
        name: name.to_owned(),
        display_name: name.to_owned(),
        description: description.to_owned(),
        category: "test".to_owned(),
        group: ToolGroup::Custom("test".to_owned()),
        version: "0.1.0".to_owned(),
        input_schema: json!({ "type": "object" }),
        output_schema: None,
        dynamic_schema: false,
        properties: props(),
        trust_level: TrustLevel::AdminTrusted,
        required_capabilities: Vec::new(),
        budget: harness_tool::default_result_budget(),
        provider_restriction: ProviderRestriction::All,
        origin: ToolOrigin::Builtin,
        search_hint: search_hint.map(str::to_owned),
        service_binding: None,
        metadata: ToolDescriptorMetadata::default(),
    }
}

fn git_descriptor(name: &str, description: &str) -> ToolDescriptor {
    let mut descriptor = descriptor(name, description, None);
    descriptor.group = ToolGroup::Git;
    descriptor.metadata = ToolDescriptorMetadata {
        aliases: vec!["git status".to_owned()],
        families: vec!["git".to_owned()],
        platforms: vec!["codex".to_owned()],
        examples: vec!["Check repository status".to_owned()],
        risk_level: ToolRiskLevel::Low,
        effects: vec!["reads_git".to_owned()],
        modalities: vec!["text".to_owned()],
        integration_source: ToolIntegrationSource::Builtin,
        configuration: None,
    };
    descriptor
}

fn props() -> ToolProperties {
    ToolProperties {
        is_concurrency_safe: true,
        is_read_only: true,
        is_destructive: false,
        long_running: None,
        defer_policy: DeferPolicy::AutoDefer,
    }
}

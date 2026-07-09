use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use harness_contracts::{
    ToolDescriptor, ToolGroup, ToolIntegrationSource, ToolName, ToolProperties,
};

#[async_trait]
pub trait ToolSearchScorer: Send + Sync + 'static {
    async fn score(
        &self,
        tool: &ToolDescriptor,
        properties: &ToolProperties,
        terms: &ScoringTerms,
        context: &ScoringContext,
    ) -> u32;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScoringTerms {
    pub optional: Vec<String>,
    pub required: Vec<String>,
    pub filters: Vec<ScoringFilter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoringFilter {
    pub key: String,
    pub value: String,
}

impl ScoringTerms {
    #[must_use]
    pub fn parse(query: &str) -> Self {
        let mut terms = Self::default();
        for token in query.split_whitespace() {
            let normalized = token.trim().to_ascii_lowercase();
            if normalized.is_empty() {
                continue;
            }
            if let Some((key, value)) = normalized.split_once(':') {
                if !key.is_empty() && !value.is_empty() {
                    terms.filters.push(ScoringFilter {
                        key: key.to_owned(),
                        value: value.to_owned(),
                    });
                }
            } else if let Some(required) = normalized.strip_prefix('+') {
                if !required.is_empty() {
                    terms.required.push(required.to_owned());
                }
            } else {
                terms.optional.push(normalized);
            }
        }
        terms
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScoringContext {
    pub discovered: Arc<HashSet<ToolName>>,
}

#[derive(Debug, Clone, Default)]
pub struct DefaultScorer {
    weights: ScoringWeights,
}

impl DefaultScorer {
    #[must_use]
    pub fn new(weights: ScoringWeights) -> Self {
        Self { weights }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoringWeights {
    pub name_part_exact_mcp: u32,
    pub name_part_exact_regular: u32,
    pub name_part_partial_mcp: u32,
    pub name_part_partial_regular: u32,
    pub full_name_fallback: u32,
    pub search_hint: u32,
    pub description: u32,
    pub required_capability: u32,
    pub metadata: u32,
    pub discovered_penalty_ratio: f32,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            name_part_exact_mcp: 12,
            name_part_exact_regular: 10,
            name_part_partial_mcp: 6,
            name_part_partial_regular: 5,
            full_name_fallback: 3,
            search_hint: 4,
            description: 2,
            required_capability: 3,
            metadata: 4,
            discovered_penalty_ratio: 0.3,
        }
    }
}

#[async_trait]
impl ToolSearchScorer for DefaultScorer {
    async fn score(
        &self,
        tool: &ToolDescriptor,
        _properties: &ToolProperties,
        terms: &ScoringTerms,
        context: &ScoringContext,
    ) -> u32 {
        if terms.required.is_empty() && terms.optional.is_empty() {
            return 0;
        }

        let name = tool.name.to_ascii_lowercase();
        let description = tool.description.to_ascii_lowercase();
        let search_hint = tool
            .search_hint
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let capabilities = required_capability_text(tool);
        let metadata = metadata_terms(tool);
        let is_mcp = name.starts_with("mcp__");
        let parts = parse_tool_name_parts(&tool.name);

        if !terms
            .filters
            .iter()
            .all(|filter| metadata_filter_matches(tool, filter))
        {
            return 0;
        }

        if !terms.required.iter().all(|term| {
            matches_any(
                term,
                &name,
                &description,
                &search_hint,
                &capabilities,
                &metadata,
                &parts,
            )
        }) {
            return 0;
        }

        let mut score = 0;
        for term in terms.required.iter().chain(terms.optional.iter()) {
            score += score_term(
                term,
                &name,
                &description,
                &search_hint,
                &capabilities,
                &metadata,
                &parts,
                is_mcp,
                self.weights,
            );
        }

        if score == 0 {
            return 0;
        }

        if context.discovered.contains(&tool.name) {
            (f64::from(score) * f64::from(self.weights.discovered_penalty_ratio))
                .round()
                .clamp(1.0, f64::from(u32::MAX)) as u32
        } else {
            score
        }
    }
}

fn matches_any(
    term: &str,
    name: &str,
    description: &str,
    search_hint: &str,
    capabilities: &str,
    metadata: &str,
    parts: &[String],
) -> bool {
    name.contains(term)
        || description.contains(term)
        || search_hint.contains(term)
        || capabilities.contains(term)
        || metadata.contains(term)
        || parts.iter().any(|part| part.contains(term))
}

fn score_term(
    term: &str,
    name: &str,
    description: &str,
    search_hint: &str,
    capabilities: &str,
    metadata: &str,
    parts: &[String],
    is_mcp: bool,
    weights: ScoringWeights,
) -> u32 {
    let mut score = 0;
    for part in parts {
        if part == term {
            score += if is_mcp {
                weights.name_part_exact_mcp
            } else {
                weights.name_part_exact_regular
            };
        } else if part.contains(term) {
            score += if is_mcp {
                weights.name_part_partial_mcp
            } else {
                weights.name_part_partial_regular
            };
        }
    }
    if score == 0 && name.contains(term) {
        score += weights.full_name_fallback;
    }
    if !search_hint.is_empty() && search_hint.contains(term) {
        score += weights.search_hint;
    }
    if description.contains(term) {
        score += weights.description;
    }
    if capabilities.contains(term) {
        score += weights.required_capability;
    }
    if !metadata.is_empty() && metadata.contains(term) {
        score += weights.metadata;
    }
    score
}

fn required_capability_text(tool: &ToolDescriptor) -> String {
    tool.required_capabilities
        .iter()
        .map(|capability| format!("{capability:?}").to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

fn metadata_terms(tool: &ToolDescriptor) -> String {
    let metadata = &tool.metadata;
    let mut terms = Vec::new();
    terms.extend(metadata.aliases.iter().cloned());
    terms.extend(metadata.families.iter().cloned());
    terms.extend(metadata.platforms.iter().cloned());
    terms.extend(metadata.examples.iter().cloned());
    terms.extend(metadata.effects.iter().cloned());
    terms.extend(metadata.modalities.iter().cloned());
    terms.push(format!("{:?}", metadata.risk_level));
    terms.push(format!("{:?}", metadata.integration_source));
    terms.join(" ").to_ascii_lowercase()
}

fn metadata_filter_matches(tool: &ToolDescriptor, filter: &ScoringFilter) -> bool {
    let value = filter.value.as_str();
    match filter.key.as_str() {
        "group" => group_wire_name(&tool.group) == value,
        "family" | "families" => contains_normalized(&tool.metadata.families, value),
        "platform" | "platforms" => contains_normalized(&tool.metadata.platforms, value),
        "effect" | "effects" => contains_normalized(&tool.metadata.effects, value),
        "modality" | "modalities" => contains_normalized(&tool.metadata.modalities, value),
        "risk" | "risk_level" => {
            format!("{:?}", tool.metadata.risk_level).eq_ignore_ascii_case(value)
        }
        "source" | "integration" | "integration_source" => {
            integration_source_wire_name(tool.metadata.integration_source) == value
        }
        _ => true,
    }
}

fn contains_normalized(values: &[String], needle: &str) -> bool {
    values
        .iter()
        .any(|value| value.eq_ignore_ascii_case(needle))
}

fn integration_source_wire_name(source: ToolIntegrationSource) -> &'static str {
    match source {
        ToolIntegrationSource::Builtin => "builtin",
        ToolIntegrationSource::Mcp => "mcp",
        ToolIntegrationSource::Plugin => "plugin",
        ToolIntegrationSource::Brokered => "brokered",
        ToolIntegrationSource::External => "external",
    }
}

fn group_wire_name(group: &ToolGroup) -> &str {
    match group {
        ToolGroup::FileSystem => "file_system",
        ToolGroup::Search => "search",
        ToolGroup::Network => "network",
        ToolGroup::Shell => "shell",
        ToolGroup::Git => "git",
        ToolGroup::Worktree => "worktree",
        ToolGroup::Session => "session",
        ToolGroup::Artifact => "artifact",
        ToolGroup::Browser => "browser",
        ToolGroup::Computer => "computer",
        ToolGroup::Image => "image",
        ToolGroup::Notebook => "notebook",
        ToolGroup::Lsp => "lsp",
        ToolGroup::Automation => "automation",
        ToolGroup::Workflow => "workflow",
        ToolGroup::Agent => "agent",
        ToolGroup::Coordinator => "coordinator",
        ToolGroup::Memory => "memory",
        ToolGroup::Clarification => "clarification",
        ToolGroup::Meta => "meta",
        ToolGroup::Custom(value) => value.as_str(),
        _ => "",
    }
}

#[must_use]
pub fn parse_tool_name_parts(name: &str) -> Vec<String> {
    let raw = if let Some(rest) = name.strip_prefix("mcp__") {
        rest
    } else {
        name
    };
    let mut parts = Vec::new();
    for chunk in raw.split("__").flat_map(|part| part.split('_')) {
        parts.extend(split_camel_case(chunk));
    }
    parts
        .into_iter()
        .map(|part| part.to_ascii_lowercase())
        .filter(|part| !part.is_empty())
        .collect()
}

fn split_camel_case(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    for ch in value.chars() {
        if ch.is_ascii_uppercase() && !current.is_empty() {
            parts.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

use std::ops::Range;

use harness_contracts::{ContentHash, MemoryError, Severity, ThreatAction, ThreatCategory};
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct MemoryThreatScanner {
    patterns: Vec<ThreatPattern>,
}

#[derive(Debug, Clone)]
pub struct ThreatPattern {
    pub id: String,
    pub expression: String,
    pub category: ThreatCategory,
    pub severity: Severity,
    pub action: ThreatAction,
    regex: Regex,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ThreatScanReport {
    pub action: ThreatAction,
    pub hits: Vec<ThreatHit>,
    pub redacted_content: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ThreatHit {
    pub pattern_id: String,
    pub category: ThreatCategory,
    pub severity: Severity,
    pub action: ThreatAction,
    pub range: Range<usize>,
}

#[derive(Debug, Clone, Deserialize)]
struct DefaultPatternData {
    patterns: Vec<DefaultPatternSpec>,
}

#[derive(Debug, Clone, Deserialize)]
struct DefaultPatternSpec {
    id: String,
    expression: String,
    category: ThreatCategory,
    severity: Severity,
    action: ThreatAction,
}

impl ThreatPattern {
    pub fn new(
        id: impl Into<String>,
        expression: impl Into<String>,
        category: ThreatCategory,
        severity: Severity,
        action: ThreatAction,
    ) -> Result<Self, MemoryError> {
        let id = id.into();
        let expression = expression.into();
        let regex = Regex::new(&expression).map_err(|error| {
            MemoryError::Message(format!("invalid threat pattern {id}: {error}"))
        })?;

        Ok(Self {
            id,
            expression,
            category,
            severity,
            action,
            regex,
        })
    }
}

impl MemoryThreatScanner {
    #[must_use]
    pub fn from_patterns(patterns: Vec<ThreatPattern>) -> Self {
        Self { patterns }
    }

    #[must_use]
    pub fn patterns(&self) -> &[ThreatPattern] {
        &self.patterns
    }

    #[must_use]
    pub fn scan(&self, content: &str) -> ThreatScanReport {
        let mut action = ThreatAction::Warn;
        let mut hits = Vec::new();

        for pattern in &self.patterns {
            for found in pattern.regex.find_iter(content) {
                action = strongest_action(action, pattern.action);
                hits.push(ThreatHit {
                    pattern_id: pattern.id.clone(),
                    category: pattern.category,
                    severity: pattern.severity,
                    action: pattern.action,
                    range: found.start()..found.end(),
                });
            }
        }

        let redacted_content = if action == ThreatAction::Redact {
            redact_content(content, &hits)
        } else {
            None
        };

        ThreatScanReport {
            action,
            hits,
            redacted_content,
        }
    }
}

#[must_use]
pub fn threat_content_hash(content: &str) -> ContentHash {
    ContentHash(*blake3::hash(content.as_bytes()).as_bytes())
}

impl Default for MemoryThreatScanner {
    fn default() -> Self {
        let data: DefaultPatternData = toml::from_str(include_str!("../data/threat-patterns.toml"))
            .expect("default threat pattern data must parse");
        let patterns = data
            .patterns
            .into_iter()
            .map(|spec| {
                ThreatPattern::new(
                    spec.id,
                    spec.expression,
                    spec.category,
                    spec.severity,
                    spec.action,
                )
                .expect("default threat pattern must compile")
            })
            .collect();

        Self { patterns }
    }
}

fn strongest_action(left: ThreatAction, right: ThreatAction) -> ThreatAction {
    if action_rank(right) > action_rank(left) {
        right
    } else {
        left
    }
}

fn action_rank(action: ThreatAction) -> u8 {
    if action == ThreatAction::Block {
        2
    } else if action == ThreatAction::Redact {
        1
    } else {
        0
    }
}

fn redact_content(content: &str, hits: &[ThreatHit]) -> Option<String> {
    let mut ranges = hits
        .iter()
        .filter(|hit| hit.action == ThreatAction::Redact)
        .map(|hit| (hit.range.clone(), hit.category))
        .collect::<Vec<_>>();

    if ranges.is_empty() {
        return None;
    }

    ranges.sort_by_key(|(range, _)| (range.start, range.end));

    let mut merged: Vec<(Range<usize>, ThreatCategory)> = Vec::new();
    for (range, category) in ranges {
        if let Some((last_range, _)) = merged.last_mut() {
            if range.start <= last_range.end {
                last_range.end = last_range.end.max(range.end);
                continue;
            }
        }

        merged.push((range, category));
    }

    let mut out = String::with_capacity(content.len());
    let mut cursor = 0;
    for (range, category) in merged {
        out.push_str(&content[cursor..range.start]);
        out.push_str("[REDACTED:");
        out.push_str(category_label(category));
        out.push(']');
        cursor = range.end;
    }
    out.push_str(&content[cursor..]);

    Some(out)
}

fn category_label(category: ThreatCategory) -> &'static str {
    match category {
        ThreatCategory::PromptInjection => "prompt_injection",
        ThreatCategory::Exfiltration => "exfiltration",
        ThreatCategory::Backdoor => "backdoor",
        ThreatCategory::Credential => "credential",
        ThreatCategory::Malicious => "malicious",
        ThreatCategory::SpecialToken => "special_token",
        _ => "unknown",
    }
}

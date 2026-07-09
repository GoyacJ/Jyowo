use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ToolDeferredPoolChangedEvent, ToolName, ToolPoolChangeSource};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DeferredToolsDeltaAttachment {
    pub added_names: Vec<ToolName>,
    pub removed_names: Vec<ToolName>,
    pub reason: String,
    pub added_reasons: BTreeMap<ToolName, String>,
    pub removed_reasons: BTreeMap<ToolName, String>,
    pub source: ToolPoolChangeSource,
    pub at: DateTime<Utc>,
    pub initial: bool,
}

impl DeferredToolsDeltaAttachment {
    #[must_use]
    pub fn from_pool_change(event: &ToolDeferredPoolChangedEvent) -> Self {
        let reason = reason_for_source(&event.source);
        let added_reasons = event
            .added
            .iter()
            .map(|hint| {
                (
                    hint.name.clone(),
                    hint.hint.clone().unwrap_or_else(|| reason.clone()),
                )
            })
            .collect();
        let removed_reasons = event
            .removed
            .iter()
            .map(|name| (name.clone(), "tool is no longer deferred".to_owned()))
            .collect();
        Self {
            added_names: event.added.iter().map(|hint| hint.name.clone()).collect(),
            removed_names: event.removed.clone(),
            reason,
            added_reasons,
            removed_reasons,
            source: event.source.clone(),
            at: event.at,
            initial: matches!(event.source, ToolPoolChangeSource::InitialClassification),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added_names.is_empty() && self.removed_names.is_empty()
    }

    pub fn remove_added_names(&mut self, names: &[ToolName]) {
        let materialized = names.iter().collect::<BTreeSet<_>>();
        self.added_names.retain(|name| !materialized.contains(name));
        for name in names {
            self.added_reasons.remove(name);
        }
    }

    pub fn merge(&mut self, next: Self) {
        let next_added = next.added_names.iter().collect::<BTreeSet<_>>();
        let next_removed = next.removed_names.iter().collect::<BTreeSet<_>>();
        self.added_names.retain(|name| !next_removed.contains(name));
        self.removed_names.retain(|name| !next_added.contains(name));

        for name in next.added_names {
            if !self.added_names.contains(&name) {
                self.added_names.push(name);
            }
        }
        for (name, reason) in next.added_reasons {
            self.added_reasons.insert(name, reason);
        }
        for name in next.removed_names {
            if !self.removed_names.contains(&name) {
                self.removed_names.push(name);
            }
        }
        for (name, reason) in next.removed_reasons {
            self.removed_reasons.insert(name, reason);
        }
        self.reason = next.reason;
        self.source = next.source;
        self.at = next.at;
        self.initial |= next.initial;
    }

    #[must_use]
    pub fn to_attachment_text(&self) -> String {
        let mut text = if self.initial {
            format!(
                "<deferred-tools initial=\"true\" changed-at=\"{}\" reason=\"{}\">\n",
                self.at.to_rfc3339(),
                escape_attr(&self.reason)
            )
        } else {
            format!(
                "<deferred-tools changed-at=\"{}\" reason=\"{}\">\n",
                self.at.to_rfc3339(),
                escape_attr(&self.reason)
            )
        };
        if !self.added_names.is_empty() {
            text.push_str("  <added>\n");
            for name in &self.added_names {
                let reason = self
                    .added_reasons
                    .get(name)
                    .map(String::as_str)
                    .unwrap_or(self.reason.as_str());
                text.push_str("    <tool name=\"");
                text.push_str(&escape_attr(name));
                text.push_str("\" reason=\"");
                text.push_str(&escape_attr(reason));
                text.push_str("\" />\n");
            }
            text.push_str("  </added>\n");
        }
        if !self.removed_names.is_empty() {
            text.push_str("  <removed>\n");
            for name in &self.removed_names {
                let reason = self
                    .removed_reasons
                    .get(name)
                    .map(String::as_str)
                    .unwrap_or("tool is no longer deferred");
                text.push_str("    <tool name=\"");
                text.push_str(&escape_attr(name));
                text.push_str("\" reason=\"");
                text.push_str(&escape_attr(reason));
                text.push_str("\" />\n");
            }
            text.push_str("  </removed>\n");
        }
        text.push_str("</deferred-tools>");
        text
    }
}

fn reason_for_source(source: &ToolPoolChangeSource) -> String {
    match source {
        ToolPoolChangeSource::InitialClassification => {
            "deferred tool pool changed after initial classification".to_owned()
        }
        ToolPoolChangeSource::McpListChanged { server_id } => {
            format!("MCP server {} changed its tool list", server_id.0)
        }
        ToolPoolChangeSource::PluginRegistration { plugin_id } => {
            format!("plugin {plugin_id} changed available tools")
        }
        ToolPoolChangeSource::SkillHotReload { skill_id } => {
            format!("skill {skill_id} changed available tools")
        }
    }
}

fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
